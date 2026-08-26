//! Column-type guessing for DAT tables that have no schema entry. Scans the fixed
//! section byte-by-byte the way poe-dat-viewer's analysis does, then greedily
//! assigns the widest type that every row satisfies.

use crate::dat::reader::DatReader;
use crate::dat::schema::{Column, Table};

#[derive(Debug, Clone, PartialEq)]
pub enum Guess {
    Bool,
    U8,
    I16,
    I32,
    F32,
    String,
    Row,
    ForeignRow,
    Array(Box<Guess>),
}

impl Guess {
    fn type_name(&self) -> String {
        match self {
            Guess::Bool => "bool".into(),
            Guess::U8 => "u8".into(),
            Guess::I16 => "i16".into(),
            Guess::I32 => "i32".into(),
            Guess::F32 => "f32".into(),
            Guess::String => "string".into(),
            Guess::Row => "row".into(),
            Guess::ForeignRow => "foreignrow".into(),
            Guess::Array(e) => format!("{}[]", e.type_name()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuessedColumn {
    pub offset: usize,
    pub size: usize,
    pub guess: Guess,
}

const NULL64: u64 = 0xfefe_fefe_fefe_fefe;
const MAX_STRING_UNITS: usize = 4096;
const MAX_ARRAY_LEN: u64 = 1 << 20;

struct Scan<'a> {
    fixed: &'a [u8],
    var: &'a [u8],
    rows: usize,
    row_len: usize,
    ptr: usize,
}

impl<'a> Scan<'a> {
    fn row(&self, i: usize) -> &'a [u8] {
        &self.fixed[i * self.row_len..(i + 1) * self.row_len]
    }

    fn u32_at(&self, i: usize, o: usize) -> u32 {
        let r = self.row(i);
        u32::from_le_bytes(r[o..o + 4].try_into().unwrap())
    }

    fn u64_at(&self, i: usize, o: usize) -> u64 {
        let r = self.row(i);
        u64::from_le_bytes(r[o..o + 8].try_into().unwrap())
    }

    /// Pointer-sized value (u64 in datc64, u32 in legacy dat).
    fn ptr_at(&self, i: usize, o: usize) -> u64 {
        if self.ptr == 8 { self.u64_at(i, o) } else { self.u32_at(i, o) as u64 }
    }

    fn null_ptr(&self) -> u64 {
        if self.ptr == 8 { NULL64 } else { 0xfefe_fefe }
    }

    fn max_byte(&self, o: usize) -> u8 {
        (0..self.rows).map(|i| self.row(i)[o]).max().unwrap_or(0)
    }

    /// `Some(non_empty)` when `p` is a plausible string pointer, `None` otherwise.
    fn string_at(&self, p: u64) -> Option<bool> {
        if p == 0 {
            return Some(false);
        }
        if p < 8 {
            return None;
        }
        let off = (p - 8) as usize;
        if off + 2 > self.var.len() {
            return None;
        }
        let mut units = 0usize;
        let mut pos = off;
        loop {
            if pos + 2 > self.var.len() || units > MAX_STRING_UNITS {
                return None;
            }
            let u = u16::from_le_bytes([self.var[pos], self.var[pos + 1]]);
            if u == 0 {
                break;
            }
            let printable = u >= 0x20 || u == 0x09 || u == 0x0a || u == 0x0d;
            if !printable || (0xd800..0xe000).contains(&u) && units == 0 {
                return None;
            }
            units += 1;
            pos += 2;
        }
        Some(units > 0)
    }

    fn is_string_col(&self, o: usize) -> bool {
        if o + self.ptr > self.row_len {
            return false;
        }
        let mut non_empty = 0;
        for i in 0..self.rows {
            match self.string_at(self.ptr_at(i, o)) {
                Some(true) => non_empty += 1,
                Some(false) => {}
                None => return false,
            }
        }
        non_empty > 0
    }

    /// Self-references are rare and a small int followed by zeros looks identical,
    /// so only claim `row` when the null sentinel actually appears.
    fn is_row_col(&self, o: usize) -> bool {
        if o + self.ptr > self.row_len || self.rows < 2 {
            return false;
        }
        let mut non_null = 0;
        let mut nulls = 0;
        for i in 0..self.rows {
            let v = self.ptr_at(i, o);
            if v == self.null_ptr() {
                nulls += 1;
                continue;
            }
            if v as usize >= self.rows {
                return false;
            }
            non_null += 1;
        }
        non_null > 0 && nulls > 0
    }

    fn is_foreign_col(&self, o: usize) -> bool {
        let size = self.ptr * 2;
        if o + size > self.row_len {
            return false;
        }
        let mut non_null = 0;
        for i in 0..self.rows {
            let lo = self.ptr_at(i, o);
            let hi = self.ptr_at(i, o + self.ptr);
            if lo == self.null_ptr() && hi == self.null_ptr() {
                continue;
            }
            if hi != 0 || lo >= 0x10_0000 {
                return false;
            }
            non_null += 1;
        }
        non_null > 0
    }

    /// Array columns are `(count, pointer)`; returns the element type when every row fits.
    fn array_col(&self, o: usize) -> Option<Guess> {
        let size = self.ptr * 2;
        if o + size > self.row_len {
            return None;
        }
        let mut with_items = Vec::new();
        for i in 0..self.rows {
            let count = self.ptr_at(i, o);
            let p = self.ptr_at(i, o + self.ptr);
            if count == 0 {
                continue;
            }
            if count > MAX_ARRAY_LEN || p < 8 {
                return None;
            }
            with_items.push((count as usize, (p - 8) as usize));
        }
        if with_items.is_empty() {
            return None;
        }
        let fits = |es: usize| with_items.iter().all(|(c, off)| off + c * es <= self.var.len());
        if !fits(1) {
            return None;
        }
        let elem_is_string = fits(self.ptr)
            && with_items.iter().all(|(c, off)| {
                (0..*c).all(|k| {
                    let e = off + k * self.ptr;
                    let p = if self.ptr == 8 {
                        u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap())
                    } else {
                        u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64
                    };
                    self.string_at(p).is_some()
                })
            })
            && with_items.iter().any(|(c, off)| {
                (0..*c).any(|k| {
                    let e = off + k * self.ptr;
                    let p = if self.ptr == 8 {
                        u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap())
                    } else {
                        u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64
                    };
                    self.string_at(p) == Some(true)
                })
            });
        if elem_is_string {
            return Some(Guess::Array(Box::new(Guess::String)));
        }
        let elem_is_foreign = fits(self.ptr * 2)
            && with_items.iter().all(|(c, off)| {
                (0..*c).all(|k| {
                    let e = off + k * self.ptr * 2;
                    let (lo, hi) = if self.ptr == 8 {
                        (
                            u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap()),
                            u64::from_le_bytes(self.var[e + 8..e + 16].try_into().unwrap()),
                        )
                    } else {
                        (
                            u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64,
                            u32::from_le_bytes(self.var[e + 4..e + 8].try_into().unwrap()) as u64,
                        )
                    };
                    (lo == self.null_ptr() && hi == self.null_ptr()) || (hi == 0 && lo < 0x10_0000)
                })
            });
        if elem_is_foreign {
            return Some(Guess::Array(Box::new(Guess::ForeignRow)));
        }
        if fits(4) {
            return Some(Guess::Array(Box::new(Guess::I32)));
        }
        Some(Guess::Array(Box::new(Guess::U8)))
    }

    fn is_bool_col(&self, o: usize) -> bool {
        if self.max_byte(o) > 1 {
            return false;
        }
        let any_set = (0..self.rows).any(|i| self.row(i)[o] == 1);
        if !any_set {
            return false;
        }
        // Three trailing zero bytes look more like a small i32 than a bool.
        if o + 4 <= self.row_len && (1..4).all(|k| self.max_byte(o + k) == 0) {
            return false;
        }
        true
    }

    fn numeric_4(&self, o: usize) -> Guess {
        let mut nonzero = 0usize;
        let mut float_like = 0usize;
        let mut int_like = 0usize;
        for i in 0..self.rows {
            let v = self.u32_at(i, o);
            if v == 0 {
                continue;
            }
            nonzero += 1;
            let f = f32::from_bits(v);
            let exp = (v >> 23) & 0xff;
            if f.is_finite() && exp != 0 && exp != 0xff && (1e-7..1e10).contains(&f.abs()) {
                float_like += 1;
            }
            if (v as i32).unsigned_abs() < 10_000_000 {
                int_like += 1;
            }
        }
        if nonzero > 0 && float_like * 100 >= nonzero * 95 && int_like * 2 < nonzero {
            Guess::F32
        } else {
            Guess::I32
        }
    }
}

pub fn analyze(reader: &DatReader) -> Vec<GuessedColumn> {
    let data = reader.get_data();
    let rows = reader.row_count as usize;
    let Some(row_len) = reader.row_length else { return Vec::new() };
    let fixed_end = 4 + rows * row_len;
    if row_len == 0 || fixed_end > data.len() {
        return Vec::new();
    }
    let var_start = (reader.data_section_offset as usize).min(data.len());
    let scan = Scan {
        fixed: &data[4..fixed_end],
        var: &data[var_start..],
        rows,
        row_len,
        ptr: if reader.is_64bit { 8 } else { 4 },
    };

    let mut cols = Vec::new();
    let mut o = 0usize;
    let mut push = |offset: usize, size: usize, guess: Guess| cols.push(GuessedColumn { offset, size, guess });
    while o < row_len {
        let p2 = scan.ptr * 2;
        if o + p2 <= row_len {
            if let Some(g) = scan.array_col(o) {
                push(o, p2, g);
                o += p2;
                continue;
            }
        }
        if o + scan.ptr <= row_len && scan.is_string_col(o) {
            push(o, scan.ptr, Guess::String);
            o += scan.ptr;
            continue;
        }
        if o + p2 <= row_len && scan.is_foreign_col(o) {
            push(o, p2, Guess::ForeignRow);
            o += p2;
            continue;
        }
        if o + scan.ptr <= row_len && scan.is_row_col(o) {
            push(o, scan.ptr, Guess::Row);
            o += scan.ptr;
            continue;
        }
        if scan.is_bool_col(o) {
            push(o, 1, Guess::Bool);
            o += 1;
            continue;
        }
        if o + 4 <= row_len {
            push(o, 4, scan.numeric_4(o));
            o += 4;
        } else if o + 2 <= row_len {
            push(o, 2, Guess::I16);
            o += 2;
        } else {
            push(o, 1, Guess::U8);
            o += 1;
        }
    }
    cols
}

/// Synthetic schema table the regular reader/viewer can consume.
pub fn to_table(cols: &[GuessedColumn], name: &str) -> Table {
    let columns = cols
        .iter()
        .map(|c| {
            let (ty, array) = match &c.guess {
                Guess::Array(e) => (e.type_name(), true),
                g => (g.type_name(), false),
            };
            Column {
                name: Some(format!("@{} {}", c.offset, c.guess.type_name())),
                description: Some(format!("Guessed from data: {} bytes at offset {} (table not in schema)", c.size, c.offset)),
                array,
                r#type: ty,
                unique: false,
                localized: false,
                references: None,
                interval: false,
                file: None,
                files: None,
            }
        })
        .collect();
    Table { name: name.to_string(), columns, tags: None, valid_for: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a datc64 blob: rows of (i32, string ptr, bool, pad) with a variable section.
    fn synthetic() -> Vec<u8> {
        let strings = ["alpha", "beta", ""];
        let mut var: Vec<u8> = vec![0xBB; 8];
        let mut ptrs = Vec::new();
        for s in strings {
            if s.is_empty() {
                ptrs.push(0u64);
                continue;
            }
            ptrs.push((var.len()) as u64);
            for u in s.encode_utf16() {
                var.extend_from_slice(&u.to_le_bytes());
            }
            var.extend_from_slice(&[0, 0, 0, 0]);
        }
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        for (i, p) in ptrs.iter().enumerate() {
            data.extend_from_slice(&((i as i32 + 1) * 1000).to_le_bytes());
            data.extend_from_slice(&p.to_le_bytes());
            data.push((i % 2) as u8);
            data.extend_from_slice(&[7, 0, 0]);
        }
        data.extend_from_slice(&var);
        data
    }

    #[test]
    fn guesses_int_string_bool() {
        let reader = DatReader::new(synthetic(), "synthetic.datc64").unwrap();
        let cols = analyze(&reader);
        let kinds: Vec<_> = cols.iter().map(|c| c.guess.clone()).collect();
        assert_eq!(kinds[0], Guess::I32);
        assert_eq!(kinds[1], Guess::String);
        assert_eq!(kinds[2], Guess::Bool);
        let table = to_table(&cols, "synthetic");
        let row = reader.read_row(1, &table).unwrap();
        assert!(matches!(row[1], crate::dat::reader::DatValue::String(ref s) if s == "beta"));
    }

    /// Guesses must tile the whole row; prints them next to the schema when one is cached.
    /// Needs a local `examples/data/balance/itemvisualidentity.datc64` (not checked in).
    /// Run with: cargo test guesses_real_example_table -- --ignored --nocapture
    #[test]
    #[ignore]
    fn guesses_real_example_table() {
        let data = std::fs::read("examples/data/balance/itemvisualidentity.datc64").unwrap();
        let reader = DatReader::new(data, "itemvisualidentity.datc64").unwrap();
        let cols = analyze(&reader);
        assert!(!cols.is_empty());
        let covered: usize = cols.iter().map(|c| c.size).sum();
        assert_eq!(covered, reader.row_length.unwrap());
        let table = to_table(&cols, "ItemVisualIdentity");
        assert!(reader.read_row(0, &table).is_ok());
        for c in &cols {
            println!("guess  @{:>3} {:>2}B {}", c.offset, c.size, c.guess.type_name());
        }
        let path = crate::settings::AppSettings::get_app_data_dir().join("schema.min.json");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(schema) = serde_json::from_str::<crate::dat::schema::Schema>(&text) {
                if let Some(t) = schema.find_table("ItemVisualIdentity", true) {
                    let mut off = 0;
                    for col in &t.columns {
                        let size = crate::dat::reader::get_column_size(col, true);
                        println!("schema @{:>3} {:>2}B {}{} {}", off, size, col.r#type, if col.array { "[]" } else { "" }, col.name.as_deref().unwrap_or("_"));
                        off += size;
                    }
                }
            }
        }
    }
}
