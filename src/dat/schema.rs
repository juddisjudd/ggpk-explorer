#![allow(dead_code)]
use serde::{Deserialize, Serialize};

/// `SCHEMA_VERSION` from dat-schema's `src/types.ts`. Bumped only on breaking changes.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 7;

pub const VALID_FOR_POE1: u32 = 0x01;
pub const VALID_FOR_POE2: u32 = 0x02;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Schema {
    pub version: u32,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub tables: Vec<Table>,
    #[serde(default)]
    pub enumerations: Vec<Enumeration>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "validFor")]
    pub valid_for: Option<u32>,
    /// Set on tables that come from the user's override file rather than the community schema.
    #[serde(skip)]
    pub custom: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Column {
    pub name: Option<String>,
    pub description: Option<String>,
    pub array: bool,
    pub r#type: String, // "bool", "string", "i32", "f32", "foreignrow", "row", "enumrow", etc.
    pub unique: bool,
    pub localized: bool,
    pub references: Option<TableReference>,
    /// If true, this column contains two consecutive values (min, max) of the same type.
    #[serde(default)]
    pub interval: bool,
    /// Extension hint when the column holds a virtual file path (e.g. ".dds").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TableReference {
    pub table: String,
    pub column: Option<String>, // If null, row index?
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Enumeration {
    pub name: String,
    #[serde(rename = "validFor")]
    pub valid_for: Option<u32>,
    #[serde(default)]
    pub indexing: u32,
    #[serde(default)]
    pub enumerators: Vec<Option<String>>,
}

pub fn game_mask(is_poe2: bool) -> u32 {
    if is_poe2 { VALID_FOR_POE2 } else { VALID_FOR_POE1 }
}

fn valid_for_game(valid_for: Option<u32>, is_poe2: bool) -> bool {
    match valid_for {
        Some(v) => v & game_mask(is_poe2) != 0,
        None => true,
    }
}

fn games_overlap(a: Option<u32>, b: Option<u32>) -> bool {
    let all = VALID_FOR_POE1 | VALID_FOR_POE2;
    a.unwrap_or(all) & b.unwrap_or(all) != 0
}

impl Schema {
    pub fn empty() -> Self {
        Schema { version: SUPPORTED_SCHEMA_VERSION, created_at: 0, tables: Vec::new(), enumerations: Vec::new() }
    }

    /// Layers user-defined tables over the community ones: an override replaces every
    /// same-named definition for the games it is valid for and is marked `custom`.
    pub fn apply_overrides(&mut self, overrides: &[Table]) {
        for o in overrides {
            self.tables.retain(|t| !(t.name.eq_ignore_ascii_case(&o.name) && games_overlap(t.valid_for, o.valid_for)));
            self.tables.push(Table { custom: true, ..o.clone() });
        }
    }

    /// Table definition for `name` (case-insensitive) valid for the given game.
    /// Falls back to a definition for the other game if that is all the schema has.
    pub fn find_table(&self, name: &str, is_poe2: bool) -> Option<&Table> {
        let mut fallback = None;
        for t in self.tables.iter().filter(|t| t.name.eq_ignore_ascii_case(name)) {
            if valid_for_game(t.valid_for, is_poe2) {
                return Some(t);
            }
            if fallback.is_none() {
                fallback = Some(t);
            }
        }
        fallback
    }

    pub fn find_enumeration(&self, name: &str, is_poe2: bool) -> Option<&Enumeration> {
        let mut fallback = None;
        for e in self.enumerations.iter().filter(|e| e.name.eq_ignore_ascii_case(name)) {
            if valid_for_game(e.valid_for, is_poe2) {
                return Some(e);
            }
            if fallback.is_none() {
                fallback = Some(e);
            }
        }
        fallback
    }
}

impl Table {
    /// Fixed-section row width implied by the column list.
    pub fn row_width(&self, is_64bit: bool) -> usize {
        self.columns
            .iter()
            .map(|c| crate::dat::reader::get_column_size(c, is_64bit))
            .sum()
    }

    pub fn is_valid_for(&self, is_poe2: bool) -> bool {
        valid_for_game(self.valid_for, is_poe2)
    }
}

impl Enumeration {
    /// Enumerator label for a raw `enumrow` value, honouring 0- or 1-based indexing.
    pub fn label(&self, value: i64) -> Option<&str> {
        let idx = value.checked_sub(self.indexing as i64)?;
        if idx < 0 {
            return None;
        }
        self.enumerators.get(idx as usize).and_then(|e| e.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_schema_deserializes_with_enumerations() {
        let path = crate::settings::AppSettings::get_app_data_dir().join("schema.min.json");
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let schema: Schema = serde_json::from_str(&text).expect("schema.min.json should deserialize");
        assert_eq!(schema.version, SUPPORTED_SCHEMA_VERSION);
        assert!(!schema.enumerations.is_empty(), "enumerations should be populated");
    }

    #[test]
    fn find_table_prefers_matching_game() {
        let mk = |valid_for: u32| Table { name: "T".into(), columns: vec![], tags: None, valid_for: Some(valid_for), custom: false };
        let schema = Schema { version: 7, created_at: 0, tables: vec![mk(1), mk(2)], enumerations: vec![] };
        assert_eq!(schema.find_table("t", false).unwrap().valid_for, Some(1));
        assert_eq!(schema.find_table("t", true).unwrap().valid_for, Some(2));
        let only_poe1 = Schema { version: 7, created_at: 0, tables: vec![mk(1)], enumerations: vec![] };
        assert_eq!(only_poe1.find_table("T", true).unwrap().valid_for, Some(1));
    }

    #[test]
    fn enumeration_label_respects_indexing() {
        let e = Enumeration { name: "E".into(), valid_for: None, indexing: 1, enumerators: vec![Some("A".into()), None, Some("C".into())] };
        assert_eq!(e.label(1), Some("A"));
        assert_eq!(e.label(2), None);
        assert_eq!(e.label(3), Some("C"));
        assert_eq!(e.label(0), None);
    }
}
