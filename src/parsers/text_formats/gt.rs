//! `.gt` ground types: a name, walkability flags and an optional ground material.

use super::Lines;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GtFile {
    pub name: String,
    pub bool1: bool,
    pub bool2: bool,
    pub bool3: Option<bool>,
    pub bool4: Option<bool>,
    pub bool5: Option<bool>,
    pub string1: Option<String>,
}

pub fn parse(text: &str) -> Result<GtFile, String> {
    let mut lines = Lines::new(text, None);
    let name = lines.line("ground type name", |c| c.word().map(String::from))?;
    let (bool1, bool2, bool3, bool4, bool5) = lines.line("two to five 0/1 flags", |c| {
        let first = c.bool()?;
        let second = c.sp()?.bool()?;
        let third = c.opt(|c| c.sp()?.bool());
        let fourth = c.opt(|c| c.sp()?.bool());
        let fifth = c.opt(|c| c.sp()?.bool());
        Some((first, second, third, fourth, fifth))
    })?;
    let string1 = lines.try_line(|c| c.quoted().map(String::from));
    lines.end()?;
    Ok(GtFile { name, bool1, bool2, bool3, bool4, bool5, string1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_flags() {
        let f = parse("dungeon_floor_unwalkable\r\n1 0 0 0 \r\n").unwrap();
        assert_eq!(f.name, "dungeon_floor_unwalkable");
        assert!(f.bool1 && !f.bool2);
        assert_eq!(f.bool4, Some(false));
        assert!(f.bool5.is_none());
        let f = parse("CosmicGround\n0 0 0 0 0\n\"Art/ground.mat\"\n").unwrap();
        assert_eq!(f.string1.as_deref(), Some("Art/ground.mat"));
        assert!(parse("Name\n2 0\n").is_err());
    }
}
