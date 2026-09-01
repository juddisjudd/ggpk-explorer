//! User-defined table layouts layered over the community schema. Stored as a
//! dat-schema JSON document so a table can be lifted straight into a poe-tool-dev PR.

use super::schema::{Schema, Table, SUPPORTED_SCHEMA_VERSION};
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "schema_overrides.json";

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub tables: Vec<Table>,
}

fn same_slot(a: &Table, b: &Table) -> bool {
    a.name.eq_ignore_ascii_case(&b.name) && a.valid_for == b.valid_for
}

impl Overrides {
    pub fn default_path() -> PathBuf {
        crate::settings::AppSettings::get_app_data_dir().join(FILE_NAME)
    }

    /// A missing file is an empty set; an unreadable one is reported and treated as empty.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<Schema>(&text) {
            Ok(schema) => Self { tables: schema.tables.into_iter().map(|t| Table { custom: true, ..t }).collect() },
            Err(e) => {
                eprintln!("Ignoring {}: {}", path.display(), e);
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&self.as_schema())?;
        std::fs::write(path, text)
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    pub fn as_schema(&self) -> Schema {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Schema { version: SUPPORTED_SCHEMA_VERSION, created_at, tables: self.tables.clone(), enumerations: Vec::new() }
    }

    pub fn upsert(&mut self, table: Table) {
        let table = Table { custom: true, ..table };
        match self.tables.iter_mut().find(|t| same_slot(t, &table)) {
            Some(slot) => *slot = table,
            None => self.tables.push(table),
        }
    }

    /// Drops every override for `name` that applies to the given game.
    pub fn remove(&mut self, name: &str, is_poe2: bool) -> bool {
        let before = self.tables.len();
        self.tables.retain(|t| !(t.name.eq_ignore_ascii_case(name) && t.is_valid_for(is_poe2)));
        self.tables.len() != before
    }

    /// One table in dat-schema JSON, ready to paste into a schema PR.
    pub fn table_json(table: &Table) -> String {
        serde_json::to_string_pretty(table).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::schema::{Column, VALID_FOR_POE2};

    fn col(name: &str, ty: &str) -> Column {
        Column {
            name: Some(name.into()),
            description: None,
            array: false,
            r#type: ty.into(),
            unique: false,
            localized: false,
            references: None,
            interval: false,
            file: None,
            files: None,
        }
    }

    fn table(name: &str, valid_for: Option<u32>) -> Table {
        Table { name: name.into(), columns: vec![col("Id", "string")], tags: None, valid_for, custom: false }
    }

    #[test]
    fn upsert_replaces_same_slot_only() {
        let mut o = Overrides::default();
        o.upsert(table("Foo", Some(VALID_FOR_POE2)));
        o.upsert(table("foo", Some(VALID_FOR_POE2)));
        o.upsert(table("Foo", None));
        assert_eq!(o.tables.len(), 2);
        assert!(o.tables.iter().all(|t| t.custom));
        assert!(o.remove("FOO", false));
        assert_eq!(o.tables.len(), 1);
        assert_eq!(o.tables[0].valid_for, Some(VALID_FOR_POE2));
        assert!(!o.remove("missing", true));
        assert!(o.remove("foo", true));
        assert!(o.is_empty());
    }

    #[test]
    fn round_trips_through_dat_schema_json() {
        let dir = std::env::temp_dir().join(format!("ggpk-overrides-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let mut o = Overrides::default();
        o.upsert(table("Foo", Some(VALID_FOR_POE2)));
        o.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let as_schema: Schema = serde_json::from_str(&text).unwrap();
        assert_eq!(as_schema.version, SUPPORTED_SCHEMA_VERSION);
        assert_eq!(as_schema.tables[0].valid_for, Some(VALID_FOR_POE2));
        assert!(text.contains("\"validFor\""), "must use dat-schema field names: {}", text);
        assert!(!text.contains("custom"));
        let back = Overrides::load(&path);
        assert_eq!(back.tables.len(), 1);
        assert!(back.tables[0].custom);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_overrides_replaces_matching_game_only() {
        let mut schema = Schema::empty();
        schema.tables.push(table("Foo", Some(1)));
        schema.tables.push(table("Foo", Some(2)));
        let mut custom = table("Foo", Some(2));
        custom.columns.push(col("Extra", "i32"));
        schema.apply_overrides(&[custom]);
        assert_eq!(schema.tables.len(), 2);
        let poe2 = schema.find_table("Foo", true).unwrap();
        assert!(poe2.custom);
        assert_eq!(poe2.columns.len(), 2);
        assert!(!schema.find_table("Foo", false).unwrap().custom);
    }
}
