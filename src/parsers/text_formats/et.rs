//! `.et` edge types: the two `.gt` ground types an edge separates, with optional virtual parts.

use super::{has_ext, Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EtFile {
    pub name: String,
    pub hex: Option<String>,
    /// A `.gt` path or `wildcard`.
    pub gt_files: [String; 2],
    pub num_line: Option<NumLine>,
    pub gt_file2: Option<String>,
    pub virtual_section: Option<VirtualSection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NumLine {
    pub uint1: u32,
    pub uint2: u32,
    pub bool1: bool,
    pub bool2: Option<bool>,
    pub bool3: Option<bool>,
    pub bool4: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VirtualSection {
    pub virtual_et_files: [VirtualEtFile; 2],
    pub virtual_rotations: [u32; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct VirtualEtFile {
    pub path: String,
    pub bool1: bool,
}

fn gt_file(c: &mut Cursor) -> Option<String> {
    let path = c.opt(|c| c.quoted()).or_else(|| c.word())?;
    (path == "wildcard" || has_ext(path, "gt")).then(|| path.to_string())
}

fn num_line(c: &mut Cursor) -> Option<NumLine> {
    let uint1 = c.uint()?;
    let uint2 = c.sp()?.uint()?;
    let bool1 = c.sp()?.bool()?;
    let bool2 = c.opt(|c| c.sp()?.bool());
    let bool3 = c.opt(|c| c.sp()?.bool());
    let bool4 = c.opt(|c| c.sp()?.bool());
    Some(NumLine { uint1, uint2, bool1, bool2, bool3, bool4 })
}

fn virtual_et_file(c: &mut Cursor) -> Option<VirtualEtFile> {
    let path = c.word().filter(|p| has_ext(p, "et"))?.to_string();
    let bool1 = c.sp()?.bool()?;
    Some(VirtualEtFile { path, bool1 })
}

fn virtual_section(lines: &mut Lines) -> Result<Option<VirtualSection>, String> {
    if lines.try_line(|c| c.lit("virtual").map(|_| ())).is_none() {
        return Ok(None);
    }
    let first = lines.line("virtual `.et` file", virtual_et_file)?;
    let second = lines.line("virtual `.et` file", virtual_et_file)?;
    let virtual_rotations = lines.line("two virtual rotations", |c| Some([c.uint()?, c.sp()?.uint()?]))?;
    Ok(Some(VirtualSection { virtual_et_files: [first, second], virtual_rotations }))
}

pub fn parse(text: &str) -> Result<EtFile, String> {
    let mut lines = Lines::new(text, None);
    let (name, hex) = lines.line("edge name", |c| {
        let name = c.word()?.to_string();
        let hex = c.opt(|c| {
            let hex = c.sp()?.lit("#")?.word()?;
            hex.chars().all(|ch| ch.is_ascii_hexdigit()).then(|| hex.to_string())
        });
        Some((name, hex))
    })?;
    let gt_files = [lines.line("`.gt` file", gt_file)?, lines.line("`.gt` file", gt_file)?];
    let num_line = lines.try_line(num_line);
    let gt_file2 = lines.try_line(|c| c.word().filter(|p| has_ext(p, "gt")).map(String::from));
    let virtual_section = virtual_section(&mut lines)?;
    lines.end()?;
    Ok(EtFile { name, hex, gt_files, num_line, gt_file2, virtual_section })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_virtual_edges() {
        let text = "IslandShoreLineVirtual #ff00aa\r\nMetadata/Terrain/Islands/island_ocean.gt\r\nwildcard\r\n0 3 1\r\nvirtual\r\nMetadata/Terrain/Islands/island_unwalkable_cliff.et 0\r\nMetadata/Terrain/Islands/island_shore_line.et 1\r\n15 45\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.hex.as_deref(), Some("ff00aa"));
        assert_eq!(f.gt_files[1], "wildcard");
        assert_eq!(f.num_line.as_ref().unwrap().uint2, 3);
        let v = f.virtual_section.unwrap();
        assert!(v.virtual_et_files[1].bool1);
        assert_eq!(v.virtual_rotations, [15, 45]);
    }

    #[test]
    fn parses_plain_edges() {
        let f = parse("TitanWalkwayMid\n\"Metadata/a.gt\"\nMetadata/b.gt\n0 3 1 0 0 0\nMetadata/c.gt\n").unwrap();
        assert_eq!(f.num_line.unwrap().bool4, Some(false));
        assert_eq!(f.gt_file2.as_deref(), Some("Metadata/c.gt"));
        assert!(parse("Name\nMetadata/a.gt\n").is_err());
    }
}
