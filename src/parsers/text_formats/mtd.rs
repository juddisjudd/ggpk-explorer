//! `.mtd` material lists: per ground-type groups of `.mat` files (with `.dlp` doodad layers) and blend weights.

use super::{has_ext, Cursor};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MtdFile {
    pub version: u32,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: Option<String>,
    pub entries: Vec<Entry>,
    pub weight_line: Option<(Vec<u32>, u32)>,
    pub extra_line: Option<(u32, bool)>,
    pub extra_entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub mat_file: String,
    pub dlp_files: Vec<String>,
}

fn gap(c: &mut Cursor) -> Option<()> {
    let start = c.remaining().len();
    loop {
        c.skip_space();
        if c.eat("//") {
            c.rest_of_line();
        } else if c.eat("/*") {
            c.skip_past("*/");
        } else {
            break;
        }
    }
    (c.remaining().len() < start).then_some(())
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let mat_file = c.file("mat")?;
    let dlp_files = c.many(|c| c.skip_inline_space().quoted().filter(|p| has_ext(p, "dlp")).map(String::from));
    Some(Entry { mat_file, dlp_files })
}

fn entry_list(c: &mut Cursor, n: usize) -> Option<Vec<Entry>> {
    (0..n)
        .map(|_| {
            gap(c)?;
            entry(c)
        })
        .collect()
}

fn read_weights(c: &mut Cursor, n: usize) -> Option<(Vec<u32>, u32)> {
    let weights = (0..n)
        .map(|_| {
            gap(c)?;
            c.uint()
        })
        .collect::<Option<_>>()?;
    gap(c)?;
    Some((weights, c.uint()?))
}

fn read_extra(c: &mut Cursor) -> Option<(u32, bool)> {
    gap(c)?;
    let n = c.uint()?;
    gap(c)?;
    Some((n, c.bool()?))
}

fn group(c: &mut Cursor) -> Option<Group> {
    let name = c.quoted().map(String::from);
    c.opt(gap);
    let num_a = c.uint()? as usize;
    let num_b = c.sp()?.uint()? as usize;
    let entries = entry_list(c, num_a)?;
    let weight_line = if num_a > 1 { Some(read_weights(c, num_a)?) } else { None };
    let extra_line = if num_b > 0 { Some(read_extra(c)?) } else { None };
    let extra_entries = entry_list(c, num_b)?;
    Some(Group { name, entries, weight_line, extra_line, extra_entries })
}

pub fn parse(text: &str) -> Result<MtdFile, String> {
    let mut c = Cursor::new(text);
    let version = super::version(c.skip_space()).ok_or("expected `version N`")?;
    let mut groups = Vec::new();
    while c.opt(gap).is_some() && !c.done() {
        let at = c.remaining();
        let group = c.opt(group).ok_or_else(|| {
            let line = text[..text.len() - at.len()].lines().count() + 1;
            format!("line {line}: expected material group, found `{}`", at.lines().next().unwrap_or("").trim())
        })?;
        groups.push(group);
    }
    if !c.done() {
        return Err(format!("expected the end of the file, found `{}`", c.remaining().lines().next().unwrap_or("")));
    }
    if groups.is_empty() {
        return Err("no material groups".into());
    }
    Ok(MtdFile { version, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_with_weights_and_extras() {
        let text = "version 5\r\n\r\n// BEACH ====\r\n3 0\r\n\"Art/G_TRO_DrySand01.mat\"\r\n\"Art/G_TRO_DrySand02.mat\" \"Metadata/a.dlp\"\"Metadata/b.dlp\"\r\n\"Art/G_TRO_DrySand03.mat\"\r\n40 30 30 16\r\n\r\n/* block */\r\n\"TropicalCoastTrim\"1 1 \"Art/Sand01.mat\"\r\n2 1\r\n\t\"Art/Extra.mat\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.groups.len(), 2);
        let g = &f.groups[0];
        assert!(g.name.is_none());
        assert_eq!(g.entries[1].dlp_files, vec!["Metadata/a.dlp", "Metadata/b.dlp"]);
        assert_eq!(g.weight_line, Some((vec![40, 30, 30], 16)));
        let g = &f.groups[1];
        assert_eq!(g.name.as_deref(), Some("TropicalCoastTrim"));
        assert!(g.weight_line.is_none());
        assert_eq!(g.extra_line, Some((2, true)));
        assert_eq!(g.extra_entries[0].mat_file, "Art/Extra.mat");
    }

    #[test]
    fn rejects_trailing_text() {
        assert!(parse("version 5\n1 0\n\"Art/a.mat\"\nstray\n").is_err());
    }
}
