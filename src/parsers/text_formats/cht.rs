//! `.cht` chest spawns: per tile-area groups of weighted chest type lists.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ChtFile {
    pub version: u32,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub areas: Vec<String>,
    pub nums: Option<NumLine>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum NumLine {
    Float(f64),
    Full(Nums),
}

#[derive(Debug, Clone, Serialize)]
pub struct Nums {
    pub float1: f64,
    pub float2: f64,
    pub uint1: u32,
    pub uint2: u32,
    pub uint3: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub weight: u32,
    pub chest_types: Vec<String>,
}

fn comma_list(c: &mut Cursor) -> Option<Vec<String>> {
    let list = c.quoted()?;
    Some(list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
}

fn nums(c: &mut Cursor) -> Option<Nums> {
    let float1 = c.float()?;
    let float2 = c.sp()?.float()?;
    let uint1 = c.sp()?.uint()?;
    let uint2 = c.sp()?.uint()?;
    let uint3 = c.opt(|c| c.sp()?.uint());
    Some(Nums { float1, float2, uint1, uint2, uint3 })
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let weight = c.uint()?;
    let chest_types = comma_list(c.sp()?)?;
    Some(Entry { weight, chest_types })
}

pub fn parse(text: &str) -> Result<ChtFile, String> {
    let mut lines = Lines::new(text, Some("#"));
    let version = lines.version()?;
    let nums_line = lines.line("default group numbers", nums)?;
    let mut groups = vec![Group { areas: vec!["Default".into()], nums: Some(NumLine::Full(nums_line)), entries: lines.many(entry) }];
    while !lines.done() {
        let (areas, nums) = lines.line("quoted area list", |c| {
            let areas = comma_list(c)?;
            let numbers = c.opt(|c| {
                let c = c.sp()?;
                c.opt(|c| nums(c).map(NumLine::Full)).or_else(|| c.float().map(NumLine::Float))
            });
            Some((areas, numbers))
        })?;
        groups.push(Group { areas, nums, entries: lines.many(entry) });
    }
    Ok(ChtFile { version, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_named_groups() {
        let text = "version 2\r\n0.3 0.2 20 10\r\n100 \"FoundryChests/FoundryChest01, FoundryChests/FoundryChest03\"\r\n\r\n\"AggoratCeremonialSacrifice, Upper\" 0.5\r\n10 \"VaalPotCluster_02\"\r\n\"Plain\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.groups.len(), 3);
        assert_eq!(f.groups[0].entries[0].chest_types.len(), 2);
        assert!(matches!(f.groups[1].nums, Some(NumLine::Float(v)) if v == 0.5));
        assert_eq!(f.groups[1].areas, vec!["AggoratCeremonialSacrifice", "Upper"]);
        assert!(f.groups[2].nums.is_none());
    }

    #[test]
    fn reads_an_optional_fifth_number() {
        let f = parse("version 2\n0.60 0.40 30 0 20\n\"Area\" 0.1 0.1 1 2\n").unwrap();
        assert!(matches!(&f.groups[0].nums, Some(NumLine::Full(n)) if n.uint3 == Some(20)));
        assert!(matches!(&f.groups[1].nums, Some(NumLine::Full(n)) if n.uint3.is_none()));
    }
}
