//! Whole tables as JSON and CSV, each foreign key written as the key of the row it names (poe_data_tools' `dump-tables` shape), so a reference survives a patch reordering its target.

use super::analysis::check_fit;
use super::reader::{get_column_size, DatReader, DatValue};
use super::relational::{fetch_table, FileSource};
use super::schema::{Column, Schema, Table};
use crate::data_export::json::{self, Obj, J};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What each row of a table is called, by table name; `None` when its rows can only be named by index.
pub type KeyLookup<'k> = dyn Fn(&str) -> Option<Rc<Vec<J>>> + 'k;

/// The name a column is written under; unnamed columns get their position, as in the table viewer.
pub fn column_name(col: &Column, index: usize) -> String {
    col.name.clone().unwrap_or_else(|| format!("_{}", index))
}

pub fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub struct Dumper<'a> {
    schema: &'a Schema,
    is_poe2: bool,
    keys: &'a KeyLookup<'a>,
    enums: RefCell<HashMap<String, Option<Rc<Vec<J>>>>>,
}

impl<'a> Dumper<'a> {
    pub fn new(schema: &'a Schema, is_poe2: bool, keys: &'a KeyLookup<'a>) -> Self {
        Self { schema, is_poe2, keys, enums: RefCell::default() }
    }

    /// Every row in order. A row that cannot be read is `null`, so a row's position is still its index.
    pub fn table(&self, reader: &DatReader, table: &Table) -> J {
        J::Arr(
            (0..reader.row_count)
                .map(|i| match reader.read_row(i, table) {
                    Ok(values) => self.row(reader, table, &values),
                    Err(_) => J::Null,
                })
                .collect(),
        )
    }

    pub fn row(&self, reader: &DatReader, table: &Table, values: &[DatValue]) -> J {
        J::Obj(
            table
                .columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let value = values.get(i).map(|v| self.cell(reader, table, col, v)).unwrap_or(J::Null);
                    (column_name(col, i), value)
                })
                .collect(),
        )
    }

    /// The table as CSV with a leading `_rid` column. References are written as the key they resolve to, lists as JSON.
    pub fn csv(&self, reader: &DatReader, table: &Table) -> String {
        let mut out = String::from("_rid");
        for (i, col) in table.columns.iter().enumerate() {
            out.push(',');
            out.push_str(&csv_escape(&column_name(col, i)));
        }
        out.push('\n');
        for i in 0..reader.row_count {
            out.push_str(&i.to_string());
            let values = reader.read_row(i, table).ok();
            for (j, col) in table.columns.iter().enumerate() {
                out.push(',');
                if let Some(value) = values.as_ref().and_then(|v| v.get(j)) {
                    out.push_str(&csv_escape(&csv_text(&self.cell(reader, table, col, value))));
                }
            }
            out.push('\n');
        }
        out
    }

    fn cell(&self, reader: &DatReader, table: &Table, col: &Column, value: &DatValue) -> J {
        if is_reference(col) {
            return self.reference(reader, table, col, value);
        }
        match value {
            DatValue::List(count, offset) => {
                let items = reader.read_list_values(*offset, *count, col).unwrap_or_default();
                J::Arr(items.iter().map(|v| scalar(col, v)).collect())
            }
            DatValue::Interval(a, b) => J::Arr(vec![scalar(col, a), scalar(col, b)]),
            v => scalar(col, v),
        }
    }

    fn reference(&self, reader: &DatReader, table: &Table, col: &Column, value: &DatValue) -> J {
        let target = match (&col.references, col.r#type.as_str()) {
            (Some(r), _) => Some(r.table.as_str()),
            (None, "row") => Some(table.name.as_str()),
            (None, _) => None,
        };
        let names = target.and_then(|t| self.names(col, t));
        let table_name = target.map(json::text).unwrap_or(J::Null);
        let rows: Vec<Option<usize>> = match value {
            DatValue::List(count, offset) => {
                reader.read_list_values(*offset, *count, col).unwrap_or_default().iter().map(row_of).collect()
            }
            DatValue::Interval(a, b) => vec![row_of(a), row_of(b)],
            single => {
                let Some(row) = row_of(single) else { return J::Null };
                let reference = Obj::new().set("TableName", table_name);
                return match name_at(names.as_deref(), row) {
                    Some(id) => reference.set("Id", id),
                    None => reference.set("RowIndex", J::Int(row as i64)),
                }
                .build();
            }
        };
        if rows.is_empty() {
            return J::Arr(Vec::new());
        }
        let reference = Obj::new().set("TableName", table_name);
        match names.as_deref() {
            Some(_) => reference.set(
                "Ids",
                J::Arr(
                    rows.iter()
                        .map(|row| match row {
                            None => J::Null,
                            Some(row) => name_at(names.as_deref(), *row)
                                .unwrap_or_else(|| Obj::new().set("RowIndex", J::Int(*row as i64)).build()),
                        })
                        .collect(),
                ),
            ),
            None => reference.set(
                "RowIndices",
                J::Arr(rows.iter().map(|row| row.map(|r| J::Int(r as i64)).unwrap_or(J::Null)).collect()),
            ),
        }
        .build()
    }

    /// What each row of `target` is called: an enumeration's labels, or a table's keys.
    fn names(&self, col: &Column, target: &str) -> Option<Rc<Vec<J>>> {
        if col.r#type != "enumrow" {
            return (self.keys)(target);
        }
        if let Some(hit) = self.enums.borrow().get(target) {
            return hit.clone();
        }
        let labels = self.schema.find_enumeration(target, self.is_poe2).map(|en| {
            let mut labels = vec![J::Null; en.indexing as usize];
            labels.extend(en.enumerators.iter().map(|e| e.as_deref().map(json::text).unwrap_or(J::Null)));
            Rc::new(labels)
        });
        self.enums.borrow_mut().insert(target.to_string(), labels.clone());
        labels
    }
}

fn is_reference(col: &Column) -> bool {
    matches!(col.r#type.as_str(), "foreignrow" | "foreign_row" | "row" | "enumrow")
}

fn row_of(value: &DatValue) -> Option<usize> {
    match value {
        DatValue::ForeignRow(usize::MAX) => None,
        DatValue::ForeignRow(row) => Some(*row),
        DatValue::Int(i) => usize::try_from(*i).ok(),
        _ => None,
    }
}

fn name_at(names: Option<&Vec<J>>, row: usize) -> Option<J> {
    names?.get(row).filter(|name| !matches!(name, J::Null)).cloned()
}

fn scalar(col: &Column, value: &DatValue) -> J {
    match value {
        DatValue::Bool(b) => J::Bool(*b),
        DatValue::Int(i) => J::Int(*i),
        DatValue::Long(l) if matches!(col.r#type.as_str(), "long" | "i64") => J::Int(*l as i64),
        DatValue::Long(l) => i64::try_from(*l).map(J::Int).unwrap_or_else(|_| J::Str(l.to_string())),
        DatValue::Float(f) => json::float32(*f),
        DatValue::String(s) => J::Str(s.clone()),
        DatValue::ForeignRow(usize::MAX) => J::Null,
        DatValue::ForeignRow(row) => J::Int(*row as i64),
        _ => J::Null,
    }
}

fn csv_text(cell: &J) -> String {
    match cell {
        J::Null => String::new(),
        J::Str(s) => s.clone(),
        J::Obj(fields) => match fields.iter().find(|(k, _)| k != "TableName") {
            Some((_, J::Str(s))) => s.clone(),
            Some((_, value)) => json::compact(value),
            None => String::new(),
        },
        other => json::compact(other),
    }
}

/// Each table's row keys, read on first use: its unique column, with a key that is itself a reference followed to the key it names.
pub struct Keys<'a> {
    source: &'a dyn FileSource,
    schema: &'a Schema,
    is_poe2: bool,
    cache: RefCell<HashMap<String, Option<Rc<Vec<J>>>>>,
    loading: RefCell<HashSet<String>>,
}

impl<'a> Keys<'a> {
    pub fn new(source: &'a dyn FileSource, schema: &'a Schema, is_poe2: bool) -> Self {
        Self { source, schema, is_poe2, cache: RefCell::default(), loading: RefCell::default() }
    }

    pub fn get(&self, table: &str) -> Option<Rc<Vec<J>>> {
        let name = table.to_ascii_lowercase();
        if let Some(hit) = self.cache.borrow().get(&name) {
            return hit.clone();
        }
        // Two tables keyed by references to each other would otherwise recurse forever.
        if !self.loading.borrow_mut().insert(name.clone()) {
            return None;
        }
        let keys = self.load(table).map(Rc::new);
        self.loading.borrow_mut().remove(&name);
        self.cache.borrow_mut().insert(name, keys.clone());
        keys
    }

    fn load(&self, table: &str) -> Option<Vec<J>> {
        let def = self.schema.find_table(table, self.is_poe2)?;
        let (index, col) = def.columns.iter().enumerate().find(|(_, c)| c.unique)?;
        let (path, bytes) = fetch_table(self.source, &def.name)?;
        let reader = DatReader::new(bytes, &path).ok()?;
        if check_fit(&reader, def, 40).is_broken() {
            return None;
        }
        let offset = def.columns[..index].iter().map(|c| get_column_size(c, reader.is_64bit)).sum();
        let lookup = |t: &str| self.get(t);
        let dumper = Dumper::new(self.schema, self.is_poe2, &lookup);
        Some(
            (0..reader.row_count)
                .map(|row| match reader.read_cell_at(row, offset, col) {
                    Ok(value) => key_of(dumper.cell(&reader, def, col, &value)),
                    Err(_) => J::Null,
                })
                .collect(),
        )
    }
}

/// A key names its row by value: a reference key contributes the key it resolves to, and an empty string names nothing.
fn key_of(cell: J) -> J {
    match cell {
        J::Obj(fields) => fields.into_iter().find(|(k, _)| k == "Id" || k == "Ids").map(|(_, v)| v).unwrap_or(J::Null),
        J::Str(s) if s.is_empty() => J::Null,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::schema::{Enumeration, TableReference};

    fn col(name: &str, ty: &str, references: Option<&str>, unique: bool) -> Column {
        Column {
            name: Some(name.to_string()),
            description: None,
            array: false,
            r#type: ty.to_string(),
            unique,
            localized: false,
            references: references.map(|t| TableReference { table: t.to_string(), column: None }),
            interval: false,
            file: None,
            files: None,
        }
    }

    fn table(name: &str, columns: Vec<Column>) -> Table {
        Table { name: name.to_string(), columns, tags: None, valid_for: None, custom: false }
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect()
    }

    /// A 64-bit table file: row count, rows, then the `0xBB` marker and the variable section.
    fn dat(rows: &[Vec<u8>], var: &[u8]) -> Vec<u8> {
        let mut out = (rows.len() as u32).to_le_bytes().to_vec();
        for row in rows {
            out.extend(row);
        }
        out.extend([0xBB; 8]);
        out.extend(var);
        out
    }

    fn foreign(row: u64) -> Vec<u8> {
        [row.to_le_bytes(), 0u64.to_le_bytes()].concat()
    }

    fn string_at(offset: u64) -> Vec<u8> {
        offset.to_le_bytes().to_vec()
    }

    fn schema() -> Schema {
        Schema {
            version: 7,
            created_at: 0,
            tables: vec![
                table("Stats", vec![col("Id", "string", None, true)]),
                table("Mods", vec![col("Stat", "foreignrow", Some("Stats"), false), col("Kind", "enumrow", Some("ModKind"), false)]),
                table("Essences", vec![col("Stat", "foreignrow", Some("Stats"), true)]),
            ],
            enumerations: vec![Enumeration {
                name: "ModKind".to_string(),
                valid_for: None,
                indexing: 1,
                enumerators: vec![Some("Prefix".to_string()), Some("Suffix".to_string())],
            }],
        }
    }

    fn files() -> HashMap<String, Vec<u8>> {
        let ids = [utf16z("life"), utf16z("mana")].concat();
        let stats = dat(&[string_at(8), string_at(8 + utf16z("life").len() as u64)], &ids);
        let essences = dat(&[foreign(1), foreign(5)], &[]);
        HashMap::from([
            ("data/balance/stats.datc64".to_string(), stats),
            ("data/balance/essences.datc64".to_string(), essences),
        ])
    }

    #[test]
    fn references_name_the_row_by_its_key() {
        let schema = schema();
        let files = files();
        let source = |p: &str| files.get(p).cloned();
        let keys = Keys::new(&source, &schema, true);
        let lookup = |t: &str| keys.get(t);
        let dumper = Dumper::new(&schema, true, &lookup);
        let mods = schema.find_table("Mods", true).unwrap();
        let mut row = foreign(1);
        row.extend(2u32.to_le_bytes());
        let mut missing = foreign(9);
        missing.extend(7u32.to_le_bytes());
        let mut null = vec![0xFE; 16];
        null.extend(1u32.to_le_bytes());
        let reader = DatReader::new(dat(&[row, missing, null], &[]), "mods.datc64").unwrap();
        let out = json::compact(&dumper.table(&reader, mods));
        assert_eq!(
            out,
            r#"[{"Stat":{"TableName":"Stats","Id":"mana"},"Kind":{"TableName":"ModKind","Id":"Suffix"}},{"Stat":{"TableName":"Stats","RowIndex":9},"Kind":{"TableName":"ModKind","RowIndex":7}},{"Stat":null,"Kind":{"TableName":"ModKind","Id":"Prefix"}}]"#
        );
        assert_eq!(dumper.csv(&reader, mods), "_rid,Stat,Kind\n0,mana,Suffix\n1,9,7\n2,,Prefix\n");
    }

    #[test]
    fn a_reference_key_resolves_through_its_target() {
        let schema = schema();
        let files = files();
        let source = |p: &str| files.get(p).cloned();
        let keys = Keys::new(&source, &schema, true);
        let essences = keys.get("Essences").unwrap();
        assert_eq!(*essences, vec![json::text("mana"), J::Null]);
        assert!(keys.get("Mods").is_none());
    }
}
