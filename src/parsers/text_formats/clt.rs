//! `.clt` critter spawns: per tile-area groups of critter types with spawn ranges.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CltFile {
    pub version: u32,
    pub float1: f64,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub float: Option<f64>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub uint1: u32,
    pub stub: String,
    pub ao_file: Option<String>,
    pub float1: f64,
    pub float2: Option<f64>,
    pub uint2: u32,
    pub uint3: u32,
}

fn item(c: &mut Cursor, version: u32) -> Option<Item> {
    let uint1 = c.uint()?;
    let stub = c.sp()?.quoted()?.to_string();
    let ao_file = if version >= 4 { Some(c.sp()?.file("ao")?) } else { None };
    let float1 = c.sp()?.float()?;
    let float2 = if version >= 3 { Some(c.sp()?.float()?) } else { None };
    let uint2 = c.sp()?.uint()?;
    let uint3 = c.sp()?.uint()?;
    Some(Item { uint1, stub, ao_file, float1, float2, uint2, uint3 })
}

pub fn parse(text: &str) -> Result<CltFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let float1 = lines.line("float", |c| c.float())?;
    let mut groups = Vec::new();
    while groups.is_empty() || !lines.done() {
        let (name, float) = lines.line("group name", |c| {
            let name = c.opt(|c| c.quoted()).or_else(|| c.word())?.to_string();
            Some((name, c.opt(|c| c.sp()?.float())))
        })?;
        groups.push(Group { name, float, items: lines.many(|c| item(c, version)) });
    }
    Ok(CltFile { version, float1, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_quoted_groups() {
        let text = "version 4\r\n0.04\r\nDefault\r\n2 \"Metadata/Critters/Seagull/SeagullNew\" \"Metadata/Pet/Seagull/Seagull.ao\" 100 300 1 3\r\n\r\n\"IslandWetSand\" 0.5\r\n//1 \"x\" \"x.ao\" 20.0 100.0 0 1\r\n1 \"Metadata/Critters/Crab/Crab\" \"Metadata/Critters/Crab/Crab.ao\" 20.0 100.0 0 1\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.groups.len(), 2);
        assert_eq!(f.groups[0].name, "Default");
        assert_eq!(f.groups[0].items[0].ao_file.as_deref(), Some("Metadata/Pet/Seagull/Seagull.ao"));
        assert_eq!(f.groups[1].float, Some(0.5));
        assert_eq!(f.groups[1].items[0].uint3, 1);
    }

    #[test]
    fn version_3_has_no_ao_file() {
        let f = parse("version 3\n0.1\nDefault\n1 \"Metadata/Critters/Rat\" 1.0 2.0 0 1\n").unwrap();
        assert!(f.groups[0].items[0].ao_file.is_none());
    }
}
