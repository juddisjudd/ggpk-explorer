//! Loads DAT tables by name and follows the foreign keys between them, so
//! callers can walk the game's data model instead of raw row indices.
//!
//! Tables are materialised once and cached; a `Row` borrows the table it came
//! from, and dereferencing a key hands back an owned [`Ref`] because the
//! target lives in the reader's cache.

use super::reader::{DatReader, DatValue};
use super::schema::{Schema, Table};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Raw game-file access by virtual GGPK/bundle path (lower-case, forward slashes).
pub trait FileSource {
    fn fetch(&self, path: &str) -> Option<Vec<u8>>;
}

impl<F> FileSource for F
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    fn fetch(&self, path: &str) -> Option<Vec<u8>> {
        self(path)
    }
}

/// One table, fully read into memory.
pub struct LoadedTable {
    pub name: String,
    pub def: Table,
    pub reader: DatReader,
    rows: Vec<Vec<DatValue>>,
    cols: HashMap<String, usize>,
    by_id: HashMap<String, usize>,
}

impl LoadedTable {
    fn new(name: &str, def: Table, reader: DatReader) -> Self {
        let cols = def
            .columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.name.clone().map(|n| (n, i)))
            .collect::<HashMap<_, _>>();
        let rows: Vec<Vec<DatValue>> =
            (0..reader.row_count).filter_map(|i| reader.read_row(i, &def).ok()).collect();
        let mut by_id = HashMap::new();
        if let Some(&id_col) = cols.get("Id") {
            for (i, row) in rows.iter().enumerate() {
                if let Some(DatValue::String(s)) = row.get(id_col) {
                    if !s.is_empty() {
                        by_id.entry(s.clone()).or_insert(i);
                    }
                }
            }
        }
        Self { name: name.to_string(), def, reader, rows, cols, by_id }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn col(&self, name: &str) -> Option<usize> {
        self.cols.get(name).copied()
    }

    pub fn has_col(&self, name: &str) -> bool {
        self.cols.contains_key(name)
    }

    /// First of `names` this table actually has. Columns get renamed between
    /// patches and between the PoE 1 and PoE 2 schemas, so exporters name the
    /// alternatives they know instead of silently reading zeroes.
    pub fn pick<'n>(&self, names: &[&'n str]) -> Option<&'n str> {
        names.iter().copied().find(|n| self.cols.contains_key(*n))
    }

    /// Like [`pick`](Self::pick) but says which table came up short.
    pub fn require<'n>(&self, names: &[&'n str]) -> Result<&'n str, String> {
        self.pick(names)
            .ok_or_else(|| format!("{} has none of the columns {:?}", self.name, names))
    }

    pub fn row(&self, index: usize) -> Option<Row<'_>> {
        (index < self.rows.len()).then_some(Row { table: self, index })
    }

    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> + '_ {
        (0..self.rows.len()).map(move |index| Row { table: self, index })
    }

    /// Row whose `Id` column equals `id` (first match wins, as in the client).
    pub fn by_id(&self, id: &str) -> Option<Row<'_>> {
        self.by_id.get(id).and_then(|&i| self.row(i))
    }
}

/// A borrowed handle to one row.
#[derive(Clone, Copy)]
pub struct Row<'t> {
    pub table: &'t LoadedTable,
    pub index: usize,
}

impl<'t> Row<'t> {
    pub fn get(&self, col: &str) -> Option<&'t DatValue> {
        self.cell(self.table.col(col)?)
    }

    /// A cell by column position, for the columns the schema has not named.
    pub fn cell(&self, column: usize) -> Option<&'t DatValue> {
        self.table.rows[self.index].get(column)
    }

    /// `Id` of this row, or an empty string when the table has no `Id` column.
    pub fn id(&self) -> &'t str {
        self.str("Id")
    }

    pub fn str(&self, col: &str) -> &'t str {
        match self.get(col) {
            Some(DatValue::String(s)) => s.as_str(),
            _ => "",
        }
    }

    pub fn string(&self, col: &str) -> String {
        self.str(col).to_string()
    }

    /// `None` for an empty string, so optional text stays `null` in JSON.
    pub fn opt_str(&self, col: &str) -> Option<&'t str> {
        Some(self.str(col)).filter(|s| !s.is_empty())
    }

    pub fn int(&self, col: &str) -> i64 {
        self.opt_int(col).unwrap_or(0)
    }

    pub fn opt_int(&self, col: &str) -> Option<i64> {
        match self.get(col)? {
            DatValue::Int(i) => Some(*i),
            DatValue::Long(l) => Some(*l as i64),
            DatValue::Bool(b) => Some(*b as i64),
            DatValue::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn float(&self, col: &str) -> f32 {
        match self.get(col) {
            Some(DatValue::Float(f)) => *f,
            Some(DatValue::Int(i)) => *i as f32,
            Some(DatValue::Long(l)) => *l as f32,
            _ => 0.0,
        }
    }

    pub fn bool(&self, col: &str) -> bool {
        match self.get(col) {
            Some(DatValue::Bool(b)) => *b,
            Some(DatValue::Int(i)) => *i != 0,
            _ => false,
        }
    }

    /// Both halves of an `@interval` column.
    pub fn interval(&self, col: &str) -> Option<(i64, i64)> {
        match self.get(col)? {
            DatValue::Interval(a, b) => {
                let n = |v: &DatValue| match v {
                    DatValue::Int(i) => *i,
                    DatValue::Long(l) => *l as i64,
                    DatValue::Float(f) => *f as i64,
                    _ => 0,
                };
                Some((n(a), n(b)))
            }
            _ => None,
        }
    }

    /// Target row index of a key column, or `None` when the key is null.
    pub fn key(&self, col: &str) -> Option<usize> {
        match self.get(col)? {
            DatValue::ForeignRow(usize::MAX) => None,
            DatValue::ForeignRow(i) => Some(*i),
            DatValue::Int(i) if *i >= 0 => Some(*i as usize),
            _ => None,
        }
    }

    fn list(&self, col: &str) -> Vec<DatValue> {
        let Some(c) = self.table.col(col) else { return Vec::new() };
        match self.table.rows[self.index].get(c) {
            Some(DatValue::List(count, offset)) if *count > 0 => self
                .table
                .reader
                .read_list_values(*offset, *count, &self.table.def.columns[c])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn list_keys(&self, col: &str) -> Vec<usize> {
        self.list(col)
            .iter()
            .filter_map(|v| match v {
                DatValue::ForeignRow(usize::MAX) => None,
                DatValue::ForeignRow(i) => Some(*i),
                DatValue::Int(i) if *i >= 0 => Some(*i as usize),
                _ => None,
            })
            .collect()
    }

    pub fn list_int(&self, col: &str) -> Vec<i64> {
        self.list(col)
            .iter()
            .filter_map(|v| match v {
                DatValue::Int(i) => Some(*i),
                DatValue::Long(l) => Some(*l as i64),
                DatValue::Float(f) => Some(*f as i64),
                DatValue::Bool(b) => Some(*b as i64),
                _ => None,
            })
            .collect()
    }

    pub fn list_float(&self, col: &str) -> Vec<f32> {
        self.list(col)
            .iter()
            .filter_map(|v| match v {
                DatValue::Float(f) => Some(*f),
                DatValue::Int(i) => Some(*i as f32),
                DatValue::Long(l) => Some(*l as f32),
                _ => None,
            })
            .collect()
    }

    pub fn list_str(&self, col: &str) -> Vec<String> {
        self.list(col)
            .iter()
            .filter_map(|v| match v {
                DatValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

/// An owned pointer into a cached table, handed back by key dereferencing.
#[derive(Clone)]
pub struct Ref {
    pub table: Rc<LoadedTable>,
    pub index: usize,
}

impl Ref {
    pub fn row(&self) -> Row<'_> {
        Row { table: &self.table, index: self.index }
    }

    pub fn id(&self) -> String {
        self.row().id().to_string()
    }
}

/// Table cache plus foreign-key resolution.
pub struct RelationalReader<'a> {
    source: &'a dyn FileSource,
    pub schema: &'a Schema,
    pub is_poe2: bool,
    tables: RefCell<HashMap<String, Option<Rc<LoadedTable>>>>,
}

impl<'a> RelationalReader<'a> {
    pub fn new(source: &'a dyn FileSource, schema: &'a Schema, is_poe2: bool) -> Self {
        Self { source, schema, is_poe2, tables: RefCell::new(HashMap::new()) }
    }

    /// Loads a table by its schema name, e.g. `"BaseItemTypes"`. Cached, and a
    /// failure is cached too so a missing table is only looked for once.
    pub fn table(&self, name: &str) -> Option<Rc<LoadedTable>> {
        if let Some(hit) = self.tables.borrow().get(name) {
            return hit.clone();
        }
        let loaded = self.load(name).map(Rc::new);
        self.tables.borrow_mut().insert(name.to_string(), loaded.clone());
        loaded
    }

    fn load(&self, name: &str) -> Option<LoadedTable> {
        let def = self.schema.find_table(name, self.is_poe2)?.clone();
        let lower = name.to_ascii_lowercase();
        let (path, bytes) = DAT_DIRS.iter().find_map(|dir| {
            let p = format!("{}{}{}", dir, lower, DAT_EXT);
            self.source.fetch(&p).map(|b| (p, b))
        })?;
        match DatReader::new(bytes, &path) {
            Ok(reader) => Some(LoadedTable::new(name, def, reader)),
            Err(e) => {
                eprintln!("relational: could not read {}: {}", path, e);
                None
            }
        }
    }

    /// Follows a key column to the table its schema names.
    pub fn deref(&self, row: Row<'_>, col: &str) -> Option<Ref> {
        let index = row.key(col)?;
        let target = self.target_table(row.table, col)?;
        (index < target.len()).then_some(Ref { table: target, index })
    }

    /// `Id` of the row a key column points at.
    pub fn deref_id(&self, row: Row<'_>, col: &str) -> Option<String> {
        self.deref(row, col).map(|r| r.id()).filter(|s| !s.is_empty())
    }

    /// Follows every key in an array column.
    pub fn deref_list(&self, row: Row<'_>, col: &str) -> Vec<Ref> {
        let Some(target) = self.target_table(row.table, col) else { return Vec::new() };
        row.list_keys(col)
            .into_iter()
            .filter(|i| *i < target.len())
            .map(|index| Ref { table: Rc::clone(&target), index })
            .collect()
    }

    pub fn deref_list_ids(&self, row: Row<'_>, col: &str) -> Vec<String> {
        self.deref_list(row, col).iter().map(|r| r.id()).collect()
    }

    fn target_table(&self, table: &LoadedTable, col: &str) -> Option<Rc<LoadedTable>> {
        let c = table.col(col)?;
        let def = table.def.columns.get(c)?;
        match def.references.as_ref().map(|r| r.table.as_str()) {
            Some(name) => self.table(name),
            // A `row` column with no reference points back into its own table.
            None if def.r#type == "row" => self.table(&table.name),
            None => None,
        }
    }

    /// Label of an `enumrow` column, from the schema's enumerations.
    pub fn enum_label(&self, row: Row<'_>, col: &str) -> Option<String> {
        let c = row.table.col(col)?;
        let def = row.table.def.columns.get(c)?;
        let name = def.references.as_ref()?.table.as_str();
        let en = self.schema.find_enumeration(name, self.is_poe2)?;
        en.label(row.int(col)).map(str::to_string)
    }
}

/// Where PoE 2 keeps its tables; PoE 1 GGPKs use the flat `data/` folder.
const DAT_DIRS: [&str; 2] = ["data/balance/", "data/"];
const DAT_EXT: &str = ".datc64";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::schema::{Column, TableReference};

    fn col(name: &str, ty: &str, references: Option<&str>) -> Column {
        Column {
            name: Some(name.to_string()),
            description: None,
            array: false,
            r#type: ty.to_string(),
            unique: false,
            localized: false,
            references: references.map(|t| TableReference { table: t.to_string(), column: None }),
            interval: false,
            file: None,
            files: None,
        }
    }

    /// Two rows of (Id: string, Other: foreignrow) plus a target table of ids.
    fn build_dat(strings: &[&str], rows: &[(u32, u64)]) -> Vec<u8> {
        let mut fixed = Vec::new();
        let mut var: Vec<u8> = vec![0; 8];
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(var.len() as u32 + 8);
            for u in s.encode_utf16() {
                var.extend_from_slice(&u.to_le_bytes());
            }
            var.extend_from_slice(&[0, 0]);
        }
        for (str_idx, fk) in rows {
            fixed.extend_from_slice(&offsets[*str_idx as usize].to_le_bytes());
            fixed.extend_from_slice(&0u32.to_le_bytes());
            fixed.extend_from_slice(&fk.to_le_bytes());
            fixed.extend_from_slice(&0u64.to_le_bytes());
        }
        let mut out = (rows.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&fixed);
        out.extend_from_slice(&[0xBB; 8]);
        out.extend_from_slice(&var);
        out
    }

    #[test]
    fn resolves_keys_between_tables() {
        let schema = Schema {
            version: 7,
            created_at: 0,
            tables: vec![
                crate::dat::schema::Table {
                    name: "Left".into(),
                    columns: vec![col("Id", "string", None), col("Right", "foreignrow", Some("Right"))],
                    tags: None,
                    valid_for: Some(crate::dat::schema::VALID_FOR_POE2),
                    custom: false,
                },
                crate::dat::schema::Table {
                    name: "Right".into(),
                    columns: vec![col("Id", "string", None), col("Unused", "foreignrow", Some("Right"))],
                    tags: None,
                    valid_for: Some(crate::dat::schema::VALID_FOR_POE2),
                    custom: false,
                },
            ],
            enumerations: Vec::new(),
        };

        let null = 0xfefefefe_fefefefeu64;
        let left = build_dat(&["alpha", "beta"], &[(0, 1), (1, null)]);
        let right = build_dat(&["one", "two"], &[(0, 0), (1, 0)]);

        let source = move |path: &str| -> Option<Vec<u8>> {
            match path {
                "data/balance/left.datc64" => Some(left.clone()),
                "data/balance/right.datc64" => Some(right.clone()),
                _ => None,
            }
        };
        let rr = RelationalReader::new(&source, &schema, true);
        let left = rr.table("Left").expect("Left loads");
        assert_eq!(left.len(), 2);
        let first = left.row(0).unwrap();
        assert_eq!(first.id(), "alpha");
        assert_eq!(rr.deref_id(first, "Right").as_deref(), Some("two"));
        // A null key resolves to nothing rather than row 0.
        assert_eq!(rr.deref_id(left.row(1).unwrap(), "Right"), None);
        assert_eq!(left.by_id("beta").map(|r| r.index), Some(1));
    }
}
