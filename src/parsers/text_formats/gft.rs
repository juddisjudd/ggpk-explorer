//! `.gft` random fill tiles: per ground-type sections of weighted `.tdt`/`.arm` fills.

use super::{has_ext, Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GftFile {
    pub version: u32,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: String,
    pub uint1: Option<u32>,
    pub files: Vec<GenFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenFile {
    pub weight: u32,
    pub path: String,
    pub rotations: Vec<String>,
}

fn file(c: &mut Cursor) -> Option<GenFile> {
    let weight = c.uint()?;
    let path = c.sp()?.quoted().filter(|p| has_ext(p, "tdt") || has_ext(p, "arm"))?.to_string();
    let rotations = c.many(|c| c.sp()?.word().map(String::from));
    Some(GenFile { weight, path, rotations })
}

pub fn parse(text: &str) -> Result<GftFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    if version == 1 {
        lines.line("section count", |c| c.uint())?;
    }
    let mut sections = Vec::new();
    while !lines.done() {
        let (name, uint1) = lines.line("quoted section name", |c| Some((c.quoted()?.to_string(), c.opt(|c| c.sp()?.uint()))))?;
        if version == 1 {
            lines.line("file count", |c| c.uint())?;
        }
        sections.push(Section { name, uint1, files: lines.many(file) });
    }
    Ok(GftFile { version, sections })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections() {
        let text = " version 2\r\n\"default\"\r\n200 \"Metadata/Terrain/Dungeon/blank_fill.tdt\"\r\n3 \"Metadata/Terrain/Rooms/Fill/WoodLog_1.arm\" R180 FR180\r\n//4 \"Metadata/x.arm\"\r\n\r\n\"BlackInsideWall\" 2\r\n1 \"Metadata/Terrain/Dungeon/black.tdt\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[0].files[1].rotations, vec!["R180", "FR180"]);
        assert_eq!(f.sections[1].uint1, Some(2));
    }

    #[test]
    fn version_1_carries_counts() {
        let f = parse("version 1\n1\n\"default\"\n1\n5 \"Metadata/a.tdt\"\n").unwrap();
        assert_eq!(f.sections[0].files[0].weight, 5);
    }
}
