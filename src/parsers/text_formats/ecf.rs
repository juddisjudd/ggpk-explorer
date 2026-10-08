//! `.ecf` edge combinations: which `.et` edge types may meet at a tile corner.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EcfFile {
    pub version: u32,
    pub combinations: Vec<Combination>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Combination {
    pub et_files: [Option<String>; 3],
    pub uint1: Option<u32>,
}

fn et_file(c: &mut Cursor) -> Option<Option<String>> {
    c.opt(|c| c.quoted().filter(|p| p.is_empty()).map(|_| None)).or_else(|| c.file("et").map(Some))
}

fn combination(c: &mut Cursor) -> Option<Combination> {
    let et_files = [et_file(c)?, et_file(c.sp()?)?, et_file(c.sp()?)?];
    let uint1 = c.opt(|c| c.sp()?.uint());
    Some(Combination { et_files, uint1 })
}

pub fn parse(text: &str) -> Result<EcfFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let combinations = lines.many(combination);
    lines.end()?;
    Ok(EcfFile { version, combinations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combinations_with_empty_slots() {
        let text = "version 1\r\n\"Metadata/Terrain/Manor/manorwall.et\" \"Metadata/Terrain/Manor/manorwall.et\" \"\"\r\n// note\r\n\"Metadata/a.et\"  \"Metadata/b.et\" \"Metadata/c.et\" 2\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.combinations.len(), 2);
        assert!(f.combinations[0].et_files[2].is_none());
        assert_eq!(f.combinations[1].uint1, Some(2));
        assert!(parse("version 1\n\"a.gt\" \"b.et\" \"\"\n").is_err());
    }
}
