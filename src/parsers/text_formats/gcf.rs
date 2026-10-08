//! `.gcf` ground combinations: which three `.gt` ground types may meet.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GcfFile {
    pub version: u32,
    pub combinations: Vec<Combination>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Combination {
    pub gt_files: [String; 3],
}

fn combination(c: &mut Cursor) -> Option<Combination> {
    let gt_files = [c.file("gt")?, c.sp()?.file("gt")?, c.sp()?.file("gt")?];
    Some(Combination { gt_files })
}

pub fn parse(text: &str) -> Result<GcfFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let combinations = lines.many(combination);
    lines.end()?;
    Ok(GcfFile { version, combinations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_triples() {
        let f = parse("version 1\r\n\"Metadata/a.gt\" \"Metadata/b.gt\" \"Metadata/c.gt\"\r\n").unwrap();
        assert_eq!(f.combinations[0].gt_files[2], "Metadata/c.gt");
        assert!(parse("version 1\n\"Metadata/a.gt\" \"Metadata/b.gt\"\n").is_err());
    }
}
