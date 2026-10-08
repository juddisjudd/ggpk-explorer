//! `.dct` decal placement: per tile-area groups of weighted atlas decals.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DctFile {
    pub version: u32,
    pub float: f64,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub area: String,
    pub float: Option<f64>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub weight: u32,
    pub atlas_file: String,
    pub tag: String,
    pub float1: f64,
    pub float2: Option<f64>,
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let weight = c.uint()?;
    let atlas_file = c.sp()?.file("atlas")?;
    let tag = c.sp()?.quoted()?.to_string();
    let float1 = c.sp()?.float()?;
    let float2 = c.opt(|c| c.sp()?.float());
    Some(Entry { weight, atlas_file, tag, float1, float2 })
}

pub fn parse(text: &str) -> Result<DctFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let float = lines.line("float", |c| c.float())?;
    lines.line("`Default`", |c| c.lit("Default").map(|_| ()))?;
    let mut groups = vec![Group { area: "Default".into(), float: None, entries: lines.many(entry) }];
    while !lines.done() {
        let (area, float) =
            lines.line("quoted area name", |c| Some((c.quoted()?.to_string(), c.opt(|c| c.sp()?.float()))))?;
        groups.push(Group { area, float, entries: lines.many(entry) });
    }
    Ok(DctFile { version, float, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_of_decals() {
        let text = "version 2\r\n0.8\r\nDefault\r\n1 \"Metadata/Decals/grungedecal3dim.atlas\" \"slime_large\" 120.0\r\n\r\n\"ForestGrass\" 2\r\n1 \"Metadata/Decals/forestdecals.atlas\" \"flower1\" 25.0 0.5\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.float, 0.8);
        assert_eq!(f.groups[0].entries[0].tag, "slime_large");
        assert_eq!(f.groups[1].area, "ForestGrass");
        assert_eq!(f.groups[1].float, Some(2.0));
        assert_eq!(f.groups[1].entries[0].float2, Some(0.5));
        assert!(parse("version 2\n0.8\n\"NoDefault\"\n").is_err());
    }
}
