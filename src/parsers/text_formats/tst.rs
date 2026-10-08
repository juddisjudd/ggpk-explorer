//! `.tst` tile sets: included `.tst` files and the `.tdt` tiles a level may place.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TstFile {
    pub includes: Vec<String>,
    pub tdt_files: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub weight: Option<u32>,
    pub tdt_file: String,
    pub rotations: Vec<String>,
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let weight = c.uint();
    let tdt_file = c.skip_space().file("tdt")?;
    let rotations = c.many(|c| c.sp()?.word().map(String::from));
    Some(Entry { weight, tdt_file, rotations })
}

pub fn parse(text: &str) -> Result<TstFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let includes = lines.many(|c| c.lit("include")?.sp()?.file("tst"));
    let tdt_files = lines.many(entry);
    lines.end()?;
    Ok(TstFile { includes, tdt_files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_includes_and_tiles() {
        let text = "include \"Metadata/Terrain/base.tst\"\r\n300 \"Metadata/Terrain/Blank.tdt\"\r\n\r\n\"Metadata/Terrain/MudPool_01.tdt\" I R90\r\n5\t\"Metadata/Terrain/MudPool_02.tdt\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.includes, vec!["Metadata/Terrain/base.tst"]);
        assert_eq!(f.tdt_files.len(), 3);
        assert_eq!(f.tdt_files[0].weight, Some(300));
        assert_eq!(f.tdt_files[1].rotations, vec!["I", "R90"]);
        assert_eq!(f.tdt_files[2].weight, Some(5));
    }
}
