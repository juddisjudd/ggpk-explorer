//! `.ddt` doodad placement: per tile-area groups of weighted `.ao` doodads.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DdtFile {
    pub version: u32,
    pub line1: Line1,
    pub uint1: Option<u32>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line1 {
    pub scale: f64,
    pub uint1: Option<u32>,
    pub uint2: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub float1: Option<f64>,
    pub uint1: Option<u32>,
    pub objects: Vec<Object>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Object {
    pub weight: Weight,
    pub ao_file: String,
    pub uint1: Option<u32>,
    pub d: bool,
    pub float1: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Weight {
    Value(f64),
    Keyword(String),
}

fn object(c: &mut Cursor) -> Option<Object> {
    let weight = if c.eat("All") { Weight::Keyword("All".into()) } else { Weight::Value(c.float()?) };
    let ao_file = c.sp()?.file("ao")?;
    let uint1 = c.opt(|c| c.sp()?.uint());
    let d = c.opt(|c| c.sp()?.lit("D").map(|_| ())).is_some();
    let float1 = c.opt(|c| c.sp()?.float());
    Some(Object { weight, ao_file, uint1, d, float1 })
}

fn group<'a>(lines: &mut Lines<'a>, name: impl FnOnce(&mut Cursor<'a>) -> Option<&'a str>) -> Result<Group, String> {
    let (name, float1, uint1) = lines.line("group name", |c| {
        let name = name(c)?.to_string();
        let float1 = c.opt(|c| c.sp()?.float());
        let uint1 = float1.and_then(|_| c.opt(|c| c.sp()?.uint()));
        Some((name, float1, uint1))
    })?;
    Ok(Group { name, float1, uint1, objects: lines.many(object) })
}

pub fn parse(text: &str) -> Result<DdtFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let line1 = lines.line("scale line", |c| {
        let scale = c.float()?;
        let uint1 = c.opt(|c| c.sp()?.uint());
        let uint2 = c.opt(|c| c.sp()?.uint());
        Some(Line1 { scale, uint1, uint2 })
    })?;
    let uint1 = lines.try_line(|c| c.uint());
    let mut groups = vec![group(&mut lines, |c| c.word())?];
    while !lines.done() {
        groups.push(group(&mut lines, |c| c.quoted())?);
    }
    Ok(DdtFile { version, line1, uint1, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_of_doodads() {
        let text = "version 4\r\n0.25\r\n5\r\nDefault\r\n\r\n\"JungleWaterTrim\" 0.6 6\r\n10 \"Metadata/Effects/Environment/act1/vfx/flies.ao\"\r\n//1 \"Metadata/x.ao\"\r\nAll \"Metadata/Effects/snow.ao\" 20\r\n\"Empty\" \r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.uint1, Some(5));
        assert_eq!(f.groups.len(), 3);
        assert_eq!(f.groups[1].float1, Some(0.6));
        assert_eq!(f.groups[1].uint1, Some(6));
        assert!(matches!(f.groups[1].objects[1].weight, Weight::Keyword(_)));
        assert_eq!(f.groups[1].objects[1].uint1, Some(20));
        assert!(f.groups[2].objects.is_empty());
    }

    #[test]
    fn scale_line_takes_two_optional_counts() {
        let f = parse("version 3\n0.30 5\nDefault\n1 \"Metadata/a.ao\" D 0.5\n").unwrap();
        assert_eq!(f.line1.uint1, Some(5));
        assert!(f.groups[0].objects[0].d);
        assert_eq!(f.groups[0].objects[0].float1, Some(0.5));
    }
}
