//! `.tsi` tile set index: the `Key value` settings naming a level's other generator files.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TsiFile {
    pub version: Option<u32>,
    /// In file order; keys such as `EnvironmentPreload` repeat.
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub key: String,
    /// Unquoted when the value is one quoted string, otherwise the text as written.
    pub value: String,
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let key = c.word()?.to_string();
    let rest = c.sp()?.rest();
    let mut inner = Cursor::new(rest);
    let value = match inner.quoted().filter(|_| inner.done()) {
        Some(v) => v.to_string(),
        None => rest.to_string(),
    };
    Some(Entry { key, value })
}

pub fn parse(text: &str) -> Result<TsiFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.try_line(super::version);
    let entries = lines.many(entry);
    lines.end()?;
    Ok(TsiFile { version, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_repeated_keys_in_order() {
        let text = "version 3\r\nStoreyHeight\t48\r\nRoomSet\t\t\t\"generate.rs\"\r\nEnvironmentSector\t\t\"SanctumAirlock\" \"sanctum_airlock\"\r\nEnvironmentPreload\t\t\"SanctumDeath\"\r\nEnvironmentPreload\t\t\"G2_13_death_zone\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.version, Some(3));
        assert_eq!(f.entries.len(), 5);
        assert_eq!(f.entries[1].value, "generate.rs");
        assert_eq!(f.entries[2].value, "\"SanctumAirlock\" \"sanctum_airlock\"");
        assert_eq!(f.entries[4].value, "G2_13_death_zone");
    }
}
