#![allow(dead_code)]
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Schema {
    pub version: u32,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub tables: Vec<Table>,
    pub enumeration: Option<Vec<Enumeration>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "validFor")]
    pub valid_for: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct TableReference {
    pub table: String,
    pub column: Option<String>, // If null, row index?
}

#[derive(Debug, Deserialize, Clone)]
pub struct Enumeration {
    pub name: String,
    pub enumerators: Vec<String>,
}

