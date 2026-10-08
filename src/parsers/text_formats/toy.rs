//! `.toy` tile overlays: groups of weighted `.arm` overlay rooms with flags and rotations.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ToyFile {
    pub version: u32,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub header: Header,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub bool1: bool,
    pub bool2: bool,
    pub bool3: Option<bool>,
    pub file_order: Option<Order>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Order {
    File,
    Size,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub weight: u32,
    pub arm_file: String,
    pub key_values: Vec<(String, String)>,
    pub not_flags: Vec<String>,
    pub add_flags: Vec<String>,
    pub rotations: Vec<Rotation>,
}

/// One of `I`, `R90`, `R180`, `R270`, each optionally flipped with a leading `F`.
#[derive(Debug, Clone, Serialize)]
pub struct Rotation {
    pub flip: bool,
    pub angle: u32,
}

fn rotation(token: &str) -> Option<Rotation> {
    let (flip, rest) = match token.strip_prefix('F') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let angle = match rest {
        "I" => 0,
        _ => rest.strip_prefix('R')?.parse().ok()?,
    };
    Some(Rotation { flip, angle })
}

fn is_flag(token: &str) -> bool {
    token.len() > 1 && (token.starts_with('+') || token.starts_with('-'))
}

fn header(c: &mut Cursor) -> Option<Header> {
    let bool1 = c.bool()?;
    let bool2 = c.sp()?.bool()?;
    let bool3 = c.opt(|c| c.sp()?.bool());
    let file_order = c.opt(|c| match c.sp()?.word()? {
        "FileOrder" => Some(Order::File),
        "SizeOrder" => Some(Order::Size),
        _ => None,
    });
    let flags = c.many(|c| c.sp()?.word().filter(|w| is_flag(w)).map(String::from));
    Some(Header { bool1, bool2, bool3, file_order, flags })
}

fn entry(c: &mut Cursor) -> Option<Entry> {
    let weight = c.uint()?;
    let arm_file = c.sp()?.file("arm")?;
    let mut e = Entry { weight, arm_file, key_values: vec![], not_flags: vec![], add_flags: vec![], rotations: vec![] };
    while let Some(token) = c.opt(|c| c.sp()?.word()) {
        if let Some(flag) = token.strip_prefix('!').filter(|f| !f.is_empty()) {
            e.not_flags.push(flag.to_string());
        } else if let Some(r) = rotation(token) {
            e.rotations.push(r);
        } else if is_flag(token) {
            e.add_flags.push(token.to_string());
        } else {
            let (k, v) = token.split_once('=').filter(|(k, v)| !k.is_empty() && !v.is_empty())?;
            e.key_values.push((k.to_string(), v.to_string()));
        }
    }
    Some(e)
}

pub fn parse(text: &str) -> Result<ToyFile, String> {
    let mut lines = Lines::new(text, Some("//"));
    let version = lines.version()?;
    let mut groups = Vec::new();
    while !lines.done() {
        let header = lines.line("group header", header)?;
        groups.push(Group { header, entries: lines.many(entry) });
    }
    Ok(ToyFile { version, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_and_entry_tails() {
        let text = "version 1\r\n0 1 FileOrder +OverlayFeatureRooms +MatchAllTileKeys\r\n//small cart\r\n100 \"Metadata/Overlays/plow_03.arm\" R270 FR270 limit=[1] !StorageOverlayMarker +SuppressUnderlyingDoodads\r\n0 1 0 +MatchAllTileKeys\r\n20 \"Metadata/Overlays/woods_overlay_01.arm\" I FI\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.groups.len(), 2);
        let h = &f.groups[0].header;
        assert!(matches!(h.file_order, Some(Order::File)));
        assert_eq!(h.flags, vec!["+OverlayFeatureRooms", "+MatchAllTileKeys"]);
        let e = &f.groups[0].entries[0];
        assert_eq!(e.rotations.len(), 2);
        assert!(e.rotations[1].flip && e.rotations[1].angle == 270);
        assert_eq!(e.key_values, vec![("limit".to_string(), "[1]".to_string())]);
        assert_eq!(e.not_flags, vec!["StorageOverlayMarker"]);
        assert_eq!(e.add_flags, vec!["+SuppressUnderlyingDoodads"]);
        assert!(h.bool3.is_none());
        assert_eq!(f.groups[1].header.bool3, Some(false));
        assert_eq!(f.groups[1].entries[0].rotations[0].angle, 0);
    }
}
