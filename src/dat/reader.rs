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
        // Minimum valid dat file: 4 bytes row count + 8 bytes separator = 12 bytes
        if data.len() < 12 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("File too small to be a valid dat file ({} bytes): {}", data.len(), filename),
            ));
        }

        let mut cursor = Cursor::new(data.as_slice());
        
        let is_64bit = filename.ends_with(".dat64") || filename.ends_with(".datc64")
            || filename.ends_with(".datl64");

        let row_count = read_u32(&mut cursor)?;
        println!("DatReader: Loading {}, Row Count: {}, Is 64bit: {}, Size: {} bytes", filename, row_count, is_64bit, data.len());

        // Sanity check: row_count should be reasonable relative to file size.
        // Each row must be at least 1 byte, plus 4 bytes for the count + 8 bytes for separator.
        // If row_count exceeds the file size, the data is clearly not a valid dat file.
        if row_count > 0 {
            let max_possible_rows = (data.len().saturating_sub(12)) as u64;
            if (row_count as u64) > max_possible_rows {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Invalid row_count {} for file of {} bytes (max possible: {}). \
                         Data is likely not a valid dat file: {}",
                        row_count, data.len(), max_possible_rows, filename
                    ),
                ));
            }
            // Additional sanity: reject absurdly large row counts (>10M rows)
            if row_count > 10_000_000 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Unreasonable row_count {} (>10M) for {}, first 16 bytes: {:02X?}",
                        row_count, filename, &data[..std::cmp::min(16, data.len())]
                    ),
                ));
            }
        }
        
        let mut row_length = None;
        let mut data_section_offset = 0;

        // The boundary marker is always 8 bytes of 0xBB for all modern dat files
        // (matching poe_data_tools reference implementation).
        let separator: [u8; 8] = [0xBB; 8];

        if row_count > 0 {
             let mut found_pattern = false;

             // Scan for the 8-byte separator after the fixed-data section.
             // The fixed data starts at byte 4 (after row_count).
             // We look for the separator at every position from byte 4 onwards.
             for i in 4..data.len().saturating_sub(7) {
                 if data[i..i+8] == separator {
                      let fixed_data_size = i - 4;
                      if fixed_data_size > 0 && fixed_data_size % (row_count as usize) == 0 {
                          row_length = Some(fixed_data_size / (row_count as usize));
                          data_section_offset = (i + 8) as u64; // Variable data starts after separator
                          found_pattern = true;
                          println!("DatReader: Found boundary at offset {}, row_length={}, var_data_offset={}",
                              i, row_length.unwrap(), data_section_offset);
                          break;
                      }
                      // If not aligned, keep scanning — could be a false positive in the data
                 }
             }

             if !found_pattern {
                 return Err(io::Error::new(
                     io::ErrorKind::InvalidData,
                     format!(
                         "Aligned data boundary not found for row_count {} in {} ({} bytes). \
                          First 16 bytes: {:02X?}",
                         row_count, filename, data.len(),
                         &data[..std::cmp::min(16, data.len())]
                     ),
                 ));
             }

        } else {
            println!("DatReader: Row count is 0 for {}", filename);
            row_length = Some(0);

            // For zero-row files, the separator should be immediately after the row count
            if data.len() >= 12 && data[4..12] == separator {
                data_section_offset = 12; // Variable data starts after row_count(4) + separator(8)
            } else {
                // Fallback: variable data starts right after row count
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

        
        let schema_row_len: usize = table.columns.iter().map(|c| get_column_size(c, self.is_64bit)).sum();
        let row_len = self.row_length.unwrap_or(schema_row_len);

        let start = 4 + (index as usize * row_len); // 4 bytes for row count
        if start >= self.data.len() {
             return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Row index out of bounds"));
        }
        

        let end = (start + row_len).min(self.data.len());
        let mut cursor = Cursor::new(&self.data[start..end]);
        
        let mut values = Vec::new();
        
        for col in &table.columns {

             let needed = get_column_size(col, self.is_64bit);
             let current_pos = cursor.position() as usize;
             if current_pos + needed > cursor.get_ref().len() {
                 values.push(DatValue::Unknown); // Or Error
                 continue;
             }
             

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
        // Arrays are always (length: u64, pointer: u64) = 16 bytes in 64-bit dat files
        return if is_64bit { 16 } else { 8 };
    }
    match col.r#type.as_str() {
        "bool" => 1,
        "byte" | "u8" => 1,
        "short" | "i16" | "u16" => 2,
        "ushort" => 2,
        "int" | "i32" | "u32" => 4,
        "uint" => 4,
        "enumrow" => 4, // Enum reference — always u32
        "float" | "f32" => 4,
        "long" | "u64" | "i64" => 8,
        "ulong" => 8,
        "ref|string" | "string" => if is_64bit { 8 } else { 4 },
        // "row" is a self-reference: u64 in 64-bit, u32 in 32-bit (nullable via 0xFEFEFEFE...)
        t if t.starts_with("ref|") || t == "row" => if is_64bit { 8 } else { 4 },
        // "foreignrow" is a cross-table reference: u128 in 64-bit (nullable via 0xFE×16), u64 in 32-bit
        "foreign_row" | "foreignrow" => if is_64bit { 16 } else { 8 },
        // "rid" / "_" — unknown/padding types, treat as 8 bytes (u64)
        "rid" | "_" => if is_64bit { 8 } else { 4 },
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
             Ok(DatValue::Int(LittleEndian::read_i16(&b[..]) as i64))
        },
        "ushort" | "u16" => {
             let mut b = [0u8; 2];
             cursor.read_exact(&mut b)?;
             Ok(DatValue::Int(LittleEndian::read_u16(&b[..]) as i64))
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
             let abs_offset = if offset_val >= 8 {
                 var_data_offset + (offset_val - 8)
             } else {
                 var_data_offset
             };
             if (abs_offset as usize) < file_data.len() {
                 let s = read_string_at(file_data, abs_offset as usize);
                 Ok(DatValue::String(s))
             } else {
                 Ok(DatValue::String("".to_string()))
             }
        },
        "foreign_row" | "foreignrow" => {
             // foreignrow: u128 in 64-bit, u64 in 32-bit
             // Null sentinel: all 0xFE bytes
             if is_64bit {
                 let lo = read_u64(cursor)?;
                 let hi = read_u64(cursor)?;
                 let combined = (hi as u128) << 64 | (lo as u128);
                 if combined == 0xfefefefe_fefefefe_fefefefe_fefefefe_u128 {
                     Ok(DatValue::ForeignRow(usize::MAX)) // Null reference
                 } else {
                     Ok(DatValue::ForeignRow(lo as usize))
                 }
             } else {
                 let lo = read_u32(cursor)? as u64;
                 let hi = read_u32(cursor)? as u64;
                 let combined = (hi << 32) | lo;
                 if combined == 0xfefefefe_fefefefe_u64 {
                     Ok(DatValue::ForeignRow(usize::MAX)) // Null reference
                 } else {
                     Ok(DatValue::ForeignRow(lo as usize))
                 }
             }
        },
        "enumrow" => {
             // Enum reference — always u32, non-nullable
             let val = read_u32(cursor)?;
             Ok(DatValue::Int(val as i64))
        },
        t if t.starts_with("ref|") || t == "row" => {
             // Self-reference: u64 in 64-bit, u32 in 32-bit
             // Null sentinel: all 0xFE bytes
             let val = if is_64bit {
                  let v = read_u64(cursor)?;
                  if v == 0xfefefefe_fefefefe_u64 {
                      return Ok(DatValue::ForeignRow(usize::MAX)); // Null
                  }
                  v
             } else {
                  let v = read_u32(cursor)? as u64;
                  if v == 0xfefefefe_u64 {
                      return Ok(DatValue::ForeignRow(usize::MAX)); // Null
                  }
                  v
             };
             Ok(DatValue::ForeignRow(val as usize))
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
    Ok(LittleEndian::read_u32(&buf[..]))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    cursor.read_exact(&mut buf)?;
    Ok(LittleEndian::read_u64(&buf[..]))
}

impl DatReader {
    pub fn read_list_values(&self, offset: u64, count: usize, col: &Column) -> io::Result<Vec<DatValue>> {
        if count == 0 {
             return Ok(Vec::new());
        }
        
        let start = if offset >= 8 {
             (self.data_section_offset + (offset - 8)) as usize
        } else {
             self.data_section_offset as usize
        };
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
            description: None,
            interval: false,
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

    pub fn value_to_json(&self, val: &DatValue, col: &Column) -> serde_json::Value {
        use serde_json::Value;
        match val {
            DatValue::Bool(b) => Value::from(*b),
            DatValue::Int(i) => Value::from(*i),
            DatValue::Long(l) => Value::from(*l),
            DatValue::Float(f) => Value::from(*f),
            DatValue::String(s) => Value::from(s.clone()),
            DatValue::List(count, offset) => {
                if let Ok(items) = self.read_list_values(*offset, *count, col) {
                    let json_items: Vec<Value> = items.iter().map(|item| {
                        match item {
                            DatValue::Bool(b) => Value::from(*b),
                            DatValue::Int(i) => Value::from(*i),
                            DatValue::Long(l) => Value::from(*l),
                            DatValue::Float(f) => Value::from(*f),
                            DatValue::String(s) => Value::from(s.clone()),
                            DatValue::ForeignRow(k) => Value::from(*k),
                            _ => Value::Null,
                        }
                    }).collect();
                    Value::Array(json_items)
                } else {
                    Value::Array(Vec::new())
                }
            },
            DatValue::ForeignRow(k) => Value::String(format!("Key({})", k)),
            _ => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_to_json() {
        let reader = DatReader {
            data: vec![],
            is_64bit: true,
            row_count: 0,
            row_length: None,
            data_section_offset: 0,
            filename: "test.dat64".to_string(),
        };

        let col = Column {
            name: Some("test_col".to_string()),
            r#type: "int".to_string(),
            references: None,
            array: false,
            unique: false,
            localized: false,
            description: None,
            interval: false,
        };

        assert_eq!(reader.value_to_json(&DatValue::Bool(true), &col), serde_json::json!(true));
        assert_eq!(reader.value_to_json(&DatValue::Int(42), &col), serde_json::json!(42));
        assert_eq!(reader.value_to_json(&DatValue::Long(123456789), &col), serde_json::json!(123456789));
        assert_eq!(reader.value_to_json(&DatValue::Float(1.23), &col), serde_json::json!(1.23f32));
        assert_eq!(reader.value_to_json(&DatValue::String("hello".to_string()), &col), serde_json::json!("hello"));
        assert_eq!(reader.value_to_json(&DatValue::ForeignRow(77), &col), serde_json::json!("Key(77)"));
        assert_eq!(reader.value_to_json(&DatValue::Unknown, &col), serde_json::Value::Null);
    }

    #[test]
    fn test_value_to_json_list() {
        // Let's create a reader with mock data for a 64-bit list of foreign row references.
        // Element type is "ref|Stats", size = 8 bytes.
        // Element 1: value 18478 (0x482E), followed by 4 bytes padding (all zeroes).
        // Element 2: value 42 (0x2A), followed by 4 bytes padding.
        // Total list size = 16 bytes.
        let mut data = vec![0u8; 32];
        let offset = 8;
        // Write element 1 at index 0 (offset 8 - 8)
        data[0] = 0x2E;
        data[1] = 0x48;
        // Write element 2 at index 8 (offset 8 - 8 + 8)
        data[8] = 0x2A;

        let reader = DatReader {
            data,
            is_64bit: true,
            row_count: 0,
            row_length: None,
            data_section_offset: 0,
            filename: "test.dat64".to_string(),
        };

        let col = Column {
            name: Some("Stats".to_string()),
            r#type: "ref|Stats".to_string(),
            references: None,
            array: true, // List
            unique: false,
            localized: false,
            description: None,
            interval: false,
        };

        let val = DatValue::List(2, offset);

        let json_val = reader.value_to_json(&val, &col);
        assert_eq!(json_val, serde_json::json!([18478, 42]));
    }
}



