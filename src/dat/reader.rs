use byteorder::{ByteOrder, LittleEndian};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use super::schema::{Table, Column};

pub struct DatReader {
    data: Vec<u8>,
    pub is_64bit: bool,
    pub row_count: u32,
    pub row_length: Option<usize>, // If fixed length
    pub data_section_offset: u64,
    pub filename: String,
}

impl DatReader {
    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub fn new(data: Vec<u8>, filename: &str) -> io::Result<Self> {
        // Use slice for initial read
        let mut cursor = Cursor::new(data.as_slice());
        
        let is_64bit = filename.ends_with(".dat64") || filename.ends_with(".datc64");

        // DAT format detection (very basic)
        let row_count = read_u32(&mut cursor)?;
        println!("DatReader: Loading {}, Row Count: {}, Is 64bit: {}", filename, row_count, is_64bit);
        
        let mut row_length = None;
        let mut data_section_offset = 0;
        
        // Heuristic: Find 0xBBBBBBBB pattern
        // In 64-bit, it might be 0xBBBBBBBBBBBBBBBB
        if row_count > 0 {
             let pattern_32 = [0xBB, 0xBB, 0xBB, 0xBB];
             let pattern_64 = [0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB];
             
             // Simple scan
             // max_search optimization unused
             
             let mut found_pattern = false;

             for i in 4..data.len().saturating_sub(4) {
                 if is_64bit {
                      if i + 8 <= data.len() && data[i..i+8] == pattern_64 {
                           let data_size = i - 4;
                           println!("DatReader: Found 64-bit pattern at {}, data_size={}, row_count={}", i, data_size, row_count);
                           if data_size % (row_count as usize) == 0 {
                               row_length = Some(data_size / (row_count as usize));
                               data_section_offset = i as u64;
                               found_pattern = true;
                               break;
                           }
                      }
                 } else {
                      if data[i..i+4] == pattern_32 {
                           let data_size = i - 4;
                           println!("DatReader: Found 32-bit pattern at {}, data_size={}, row_count={}", i, data_size, row_count);
                           if data_size % (row_count as usize) == 0 {
                               row_length = Some(data_size / (row_count as usize));
                               data_section_offset = i as u64;
                               found_pattern = true;
                               break;
                           }
                      }
                 }
             }

             if !found_pattern {
                 return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Aligned data boundary not found for row_count {}", row_count)));
             }

        } else {
            println!("DatReader: Row count is 0 for {}", filename);
            // Handle 0 rows?
            row_length = Some(0);
            // Scan for pattern anyway to find data section?
            // If 0 rows, fixed section size is 0?
            // Then pattern should be at offset 4?
             let pattern_32 = [0xBB, 0xBB, 0xBB, 0xBB];
             if data.len() >= 8 && data[4..8] == pattern_32 {
                 data_section_offset = 4;
             }
             // For 64-bit?
             let pattern_64 = [0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB];
             if is_64bit && data.len() >= 12 && data[4..12] == pattern_64 {
                 data_section_offset = 4;
             }
        }
        
        Ok(Self {
            data,
            is_64bit, 
            row_count,
            row_length,
            data_section_offset, 
            filename: filename.to_string(),
        })
    }

    pub fn read_row(&self, index: u32, table: &Table) -> io::Result<Vec<DatValue>> {
        // Graceful handling logic:
        // 1. Calculate expected schema length.
        // 2. Read what we can.
        // 3. If we hit EOF unexpectedly, return what we have or an error value.
        
        let schema_row_len: usize = table.columns.iter().map(|c| get_column_size(c, self.is_64bit)).sum();
        let row_len = self.row_length.unwrap_or(schema_row_len);

        let start = 4 + (index as usize * row_len); // 4 bytes for row count
        if start >= self.data.len() {
             return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Row index out of bounds"));
        }
        
        // Ensure we don't read past EOF even for a valid index if file is truncated
        let end = (start + row_len).min(self.data.len());
        let mut cursor = Cursor::new(&self.data[start..end]);
        
        let mut values = Vec::new();
        
        for col in &table.columns {
             // If we don't have enough bytes for this column, push Error/Unknown
             let needed = get_column_size(col, self.is_64bit);
             let current_pos = cursor.position() as usize;
             if current_pos + needed > cursor.get_ref().len() {
                 values.push(DatValue::Unknown); // Or Error
                 continue;
             }
             
             // Pass separate slice to helper
             match read_column_value(&mut cursor, col, &self.data, self.data_section_offset, self.is_64bit) {
                 Ok(val) => values.push(val),
                 Err(_) => values.push(DatValue::Unknown),
             }
        }
        
        Ok(values)
    }
}

fn get_column_size(col: &Column, is_64bit: bool) -> usize {
    if col.array {
        return if is_64bit { 16 } else { 8 };
    }
    match col.r#type.as_str() {
        "bool" => 1,
        "byte" | "u8" => 1,
        "short" | "u16" => 2,
        "ushort" => 2,
        "int" | "i32" | "u32" => 4,
        "uint" => 4,
        "float" | "f32" => 4,
        "long" | "u64" | "i64" => 8,
        "ulong" => 8,
        "ref|string" | "string" => if is_64bit { 8 } else { 4 },
        t if t.starts_with("ref|") || t == "row" => if is_64bit { 8 } else { 4 }, // Generic ref size
        "foreign_row" | "foreignrow" => if is_64bit { 16 } else { 8 }, // Key(8)+Ptr(8) or Key(4)+Ptr(4)? Usually 16/8 is safe guess for complex foreign keys
        _ => 4,
    }
}

fn read_column_value(cursor: &mut Cursor<&[u8]>, col: &Column, file_data: &[u8], var_data_offset: u64, is_64bit: bool) -> io::Result<DatValue> {
    if col.array {
        let (count, offset) = if is_64bit {
             let c = read_u32(cursor)? as u64;
             let _ = read_u32(cursor)?; // padding
             let o = read_u32(cursor)? as u64;
             let _ = read_u32(cursor)?; // padding
             (c, o)
        } else {
             (read_u32(cursor)? as u64, read_u32(cursor)? as u64)
        };
        return Ok(DatValue::List(count as usize, offset));
    }

    match col.r#type.as_str() {
        "bool" => {
             let mut b = [0u8; 1];
             cursor.read_exact(&mut b)?;
             Ok(DatValue::Bool(b[0] != 0))
        },
        "byte" | "u8" => {
             let mut b = [0u8; 1];
             cursor.read_exact(&mut b)?;
             Ok(DatValue::Int(b[0] as i64)) // Treat as int
        },
        "short" | "i16" => {
             let mut b = [0u8; 2];
             cursor.read_exact(&mut b)?;
             Ok(DatValue::Int(LittleEndian::read_i16(&b) as i64))
        },
        "ushort" | "u16" => {
             let mut b = [0u8; 2];
             cursor.read_exact(&mut b)?;
             Ok(DatValue::Int(LittleEndian::read_u16(&b) as i64))
        },
        "int" | "i32" => {
             Ok(DatValue::Int(read_u32(cursor)? as i32 as i64))
        },
        "uint" | "u32" => {
             Ok(DatValue::Int(read_u32(cursor)? as i64))
        },
        "float" | "f32" => {
             let val = read_u32(cursor)?;
             Ok(DatValue::Float(f32::from_bits(val)))
        },
        "long" | "i64" => {
             Ok(DatValue::Long(read_u64(cursor)?))
        },
        "ulong" | "u64" => {
             Ok(DatValue::Long(read_u64(cursor)?))
        },
        "string" | "ref|string" => {
             let offset_val = if is_64bit {
                 let v = read_u32(cursor)? as u64;
                 let _ = read_u32(cursor)?; // padding/flags?
                 v
             } else {
                 read_u32(cursor)? as u64
             };
             if offset_val == 0 {
                 return Ok(DatValue::String("".to_string()));
             }
             let abs_offset = var_data_offset + offset_val;
             if (abs_offset as usize) < file_data.len() {
                 let s = read_string_at(file_data, abs_offset as usize);
                 Ok(DatValue::String(s))
             } else {
                 Ok(DatValue::String("".to_string()))
             }
        },
        "foreign_row" | "foreignrow" => {
             let idx = if is_64bit {
                 let v = read_u32(cursor)? as u64;
                 let _ = read_u32(cursor)?; // padding
                 let _ = read_u64(cursor)?; // unknown 2nd part (8 bytes)
                 v
             } else {
                 read_u32(cursor)? as u64
             };
             // Existing 32-bit logic was just read_u32.
             Ok(DatValue::ForeignRow(idx as usize))
        },
        t if t.starts_with("ref|") || t == "row" => {
             // Generic ref
             let val = if is_64bit {
                  let v = read_u32(cursor)? as u64;
                  let _ = read_u32(cursor)?; // padding
                  v
             } else {
                  read_u32(cursor)? as u64
             };
             Ok(DatValue::ForeignRow(val as usize)) // Treat as foreign row index
        },
        _ => {
             let size = get_column_size(col, is_64bit);
             if size > 0 { cursor.seek(SeekFrom::Current(size as i64))?; }
             Ok(DatValue::Unknown)
        }
    }
}

    // Helper to read string
fn read_string_at(data: &[u8], offset: usize) -> String {
    // Try to find null terminator.
    // Try UTF-16 first (double null aligned)
    // Safety
    if offset >= data.len() { return "".to_string(); }
    
    // Heuristic: iterate u16s
    let mut vec_u16 = Vec::new();
    let mut i = offset;
    while i + 1 < data.len() {
        let u = LittleEndian::read_u16(&data[i..i+2]);
        if u == 0 { break; } // Null terminator
        vec_u16.push(u);
        i += 2;
        if vec_u16.len() > 1000 { break; } // Limit
    }
    
    if !vec_u16.is_empty() {
        return String::from_utf16_lossy(&vec_u16);
    }
    
    // Fallback? empty string
    "".to_string()
}


use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub enum DatValue {
    Bool(bool),
    Int(i64),
    Long(u64),
    Float(f32),
    String(String),
    ForeignRow(usize),
    List(usize, u64), // Count, Offset
    Unknown,
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(LittleEndian::read_u32(&buf))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    cursor.read_exact(&mut buf)?;
    Ok(LittleEndian::read_u64(&buf))
}

impl DatReader {
    pub fn read_list_values(&self, offset: u64, count: usize, col: &Column) -> io::Result<Vec<DatValue>> {
        if count == 0 {
             return Ok(Vec::new());
        }
        
        let start = (self.data_section_offset + offset) as usize;
        if start >= self.data.len() {
             return Ok(vec![DatValue::Unknown]); 
        }
        
        // Element type is same as column type but `array` is false
        let elem_col = Column {
            name: None,
            r#type: col.r#type.clone(),
            references: col.references.clone(),
            array: false, 
            unique: false,
            localized: false,
            // until: None, // Removed
             description: None,
        };
        
        let elem_size = get_column_size(&elem_col, self.is_64bit);
        
        // Safety check for size
        if elem_size == 0 { return Ok(vec![DatValue::Unknown; count]); }

        let total_size = elem_size * count;
        let end = (start + total_size).min(self.data.len());
        let slice = &self.data[start..end];
        let mut cursor = Cursor::new(slice);
        
        let mut values = Vec::new();
        for _ in 0..count {
             match read_column_value(&mut cursor, &elem_col, &self.data, self.data_section_offset, self.is_64bit) {
                 Ok(v) => values.push(v),
                 Err(_) => values.push(DatValue::Unknown),
             }
        }
        
        Ok(values)
    }
}


