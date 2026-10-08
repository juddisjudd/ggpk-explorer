//! `.arm` room layouts (tile slot grid, doodads, decals, zones), hand-ported from poe_data_tools.

use std::collections::BTreeMap;

use serde::Serialize;

/// Doodads, points of interest and zones are placed in subtiles; a tile is this many.
pub const SUBTILES_PER_TILE: f32 = 23.0;
/// Decals and doodad float positions use world units; a tile is this many.
pub const WORLD_UNITS_PER_TILE: f32 = 250.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Direction {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

// Upstream reads N, W, S, E, but then neighbouring tiles disagree on shared edges and the corners.
const CARDINALS: [Direction; 4] = [Direction::S, Direction::E, Direction::N, Direction::W];
const DIAGONALS: [Direction; 4] = [Direction::SW, Direction::SE, Direction::NE, Direction::NW];

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub direction: Direction,
    pub edge: Option<String>,
    pub exit: u32,
    pub virtual_exit: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Corner {
    pub direction: Direction,
    pub ground: Option<String>,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotK {
    pub width: u32,
    pub height: u32,
    pub edges: [Edge; 4],
    pub corners: [Corner; 4],
    pub slot_tag: Option<String>,
    pub origin: Direction,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Slot {
    K(SlotK),
    #[default]
    N,
    F {
        fill: Option<String>,
    },
    S,
    O,
}

impl Slot {
    pub fn footprint(&self) -> (u32, u32) {
        match self {
            Slot::K(k) => (k.width, k.height),
            _ => (1, 1),
        }
    }

    pub fn letter(&self) -> char {
        match self {
            Slot::K(_) => 'k',
            Slot::N => 'n',
            Slot::F { .. } => 'f',
            Slot::S => 's',
            Slot::O => 'o',
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PoI {
    pub x: u32,
    pub y: u32,
    pub rotation: f32,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Doodad {
    pub x: u32,
    pub y: u32,
    pub float_pairs: Option<Vec<(f32, f32)>>,
    pub radians1: f32,
    pub trig1: Option<f32>,
    pub trig2: Option<f32>,
    pub trig3: Option<f32>,
    pub trig4: Option<f32>,
    pub bool1: bool,
    pub bool2: Option<bool>,
    pub floats: Vec<f32>,
    pub scale: f32,
    pub ao_file: String,
    pub stub: String,
    pub key_values: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoodadConnection {
    pub from: u32,
    pub to: u32,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decal {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub bool1: Option<bool>,
    pub scale: f32,
    pub atlas_file: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Zone {
    pub name: String,
    pub x_min: i32,
    pub y_min: i32,
    pub x_max: i32,
    pub y_max: i32,
    pub disable_teleports: Option<String>,
    pub env_file: Option<String>,
    pub uint1: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Thingy {
    pub et_file: Option<String>,
    pub int: i32,
    pub bool1: Option<bool>,
    pub bool2: Option<bool>,
    pub bool3: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Dimension {
    pub side_length: u32,
    pub uint1: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ArmFile {
    pub version: u32,
    pub strings: Vec<String>,
    pub dimensions: Dimension,
    pub numbers1: Vec<u32>,
    pub tag: String,
    pub bools: Vec<bool>,
    pub root_slot: Slot,
    pub thingies: Vec<Thingy>,
    pub points_of_interest: Vec<Vec<PoI>>,
    pub string1: Option<String>,
    pub grid: Vec<Vec<Slot>>,
    pub doodads: Vec<Doodad>,
    pub doodad_connections: Option<Vec<DoodadConnection>>,
    pub decals: Vec<Decal>,
    pub boss_lines: Option<Vec<Vec<String>>>,
    pub zones: Option<Vec<Zone>>,
    pub tags: Option<Vec<String>>,
    pub ground_overrides: Option<Vec<Vec<Option<String>>>>,
}

struct Line<'a> {
    no: usize,
    rest: &'a str,
}

impl<'a> Line<'a> {
    fn err(&self, what: &str) -> String {
        format!("line {}: expected {} at {:?}", self.no, what, self.rest.chars().take(40).collect::<String>())
    }

    fn peek(&self) -> Option<&'a str> {
        let s = self.rest.trim_start();
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        (end > 0).then(|| &s[..end])
    }

    fn token(&mut self) -> Option<&'a str> {
        let t = self.peek()?;
        let s = self.rest.trim_start();
        self.rest = &s[t.len()..];
        Some(t)
    }

    fn parsed<T: std::str::FromStr>(&mut self, what: &str) -> Result<T, String> {
        let before = self.rest;
        match self.token().and_then(|t| t.parse().ok()) {
            Some(v) => Ok(v),
            None => {
                self.rest = before;
                Err(self.err(what))
            }
        }
    }

    fn uint(&mut self) -> Result<u32, String> {
        self.parsed("an unsigned int")
    }

    fn int(&mut self) -> Result<i32, String> {
        self.parsed("an int")
    }

    fn float(&mut self) -> Result<f32, String> {
        self.parsed("a float")
    }

    fn flag(&mut self) -> Result<bool, String> {
        match self.peek() {
            Some("0") => {
                self.token();
                Ok(false)
            }
            Some("1") => {
                self.token();
                Ok(true)
            }
            _ => Err(self.err("0 or 1")),
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        let s = self.rest.trim_start();
        let body = s.strip_prefix('"').ok_or_else(|| self.err("a quoted string"))?;
        let end = body.find('"').ok_or_else(|| self.err("a closing quote"))?;
        self.rest = &body[end + 1..];
        Ok(body[..end].replace('\0', ""))
    }

    fn at_quote(&self) -> bool {
        self.rest.trim_start().starts_with('"')
    }

    fn is_done(&self) -> bool {
        self.rest.trim().is_empty()
    }

    fn finish(&self) -> Result<(), String> {
        if self.is_done() { Ok(()) } else { Err(self.err("the end of the line")) }
    }
}

struct Reader<'a> {
    lines: Vec<(usize, &'a str)>,
    pos: usize,
    version: u32,
}

impl<'a> Reader<'a> {
    fn next(&mut self, what: &str) -> Result<Line<'a>, String> {
        let &(no, rest) = self.lines.get(self.pos).ok_or_else(|| format!("file ends before {}", what))?;
        self.pos += 1;
        Ok(Line { no, rest })
    }

    fn whole_line<T>(&mut self, what: &str, f: impl FnOnce(&mut Line<'a>) -> Result<T, String>) -> Result<T, String> {
        let mut line = self.next(what)?;
        let v = f(&mut line)?;
        line.finish()?;
        Ok(v)
    }

    fn optional_line<T>(&mut self, f: impl FnOnce(&mut Line<'a>) -> Result<T, String>) -> Option<T> {
        let start = self.pos;
        match self.whole_line("", f) {
            Ok(v) => Some(v),
            Err(_) => {
                self.pos = start;
                None
            }
        }
    }

    /// A count line then that many items before `terminated_from`, from then on items closed by a `-1` line.
    fn group<T>(&mut self, terminated_from: u32, what: &str, mut item: impl FnMut(&mut Line<'a>) -> Result<T, String>) -> Result<Vec<T>, String> {
        let mut out = Vec::new();
        if self.version >= terminated_from {
            loop {
                let line = self.next(what)?;
                if line.rest.trim() == "-1" {
                    break;
                }
                self.pos -= 1;
                out.push(self.whole_line(what, &mut item)?);
            }
        } else {
            let n = self.whole_line(what, |l| l.uint())?;
            for _ in 0..n {
                out.push(self.whole_line(what, &mut item)?);
            }
        }
        Ok(out)
    }
}

fn string_ref(strings: &[String], i: u32) -> Result<Option<String>, String> {
    match i {
        0 => Ok(None),
        _ => strings.get(i as usize - 1).cloned().map(Some).ok_or_else(|| format!("string index {} past the {} strings", i, strings.len())),
    }
}

fn slot_k(l: &mut Line, strings: &[String]) -> Result<SlotK, String> {
    let width = l.uint()?;
    let height = l.uint()?;
    let mut edge_refs = [0u32; 4];
    for e in &mut edge_refs {
        *e = l.uint()?;
    }
    let mut exits = [0u32; 8];
    for e in &mut exits {
        *e = l.uint()?;
    }
    let mut grounds = [0u32; 4];
    for g in &mut grounds {
        *g = l.uint()?;
    }
    let mut heights = [0i32; 4];
    for h in &mut heights {
        *h = l.int()?;
    }
    let slot_tag = string_ref(strings, l.uint()?)?;
    let origin = match l.peek().and_then(|t| t.parse::<usize>().ok()) {
        Some(i) => {
            l.token();
            *DIAGONALS.get(i).ok_or_else(|| l.err("an origin 0-3"))?
        }
        None => DIAGONALS[0],
    };
    let mut edges = Vec::with_capacity(4);
    for (i, direction) in CARDINALS.into_iter().enumerate() {
        edges.push(Edge { direction, edge: string_ref(strings, edge_refs[i])?, exit: exits[i * 2], virtual_exit: exits[i * 2 + 1] });
    }
    let mut corners = Vec::with_capacity(4);
    for (i, direction) in DIAGONALS.into_iter().enumerate() {
        corners.push(Corner { direction, ground: string_ref(strings, grounds[i])?, height: heights[i] });
    }
    Ok(SlotK {
        width,
        height,
        edges: edges.try_into().map_err(|_| "edge count")?,
        corners: corners.try_into().map_err(|_| "corner count")?,
        slot_tag,
        origin,
    })
}

fn slot(l: &mut Line, strings: &[String]) -> Result<Slot, String> {
    match l.peek() {
        Some("n") => {
            l.token();
            Ok(Slot::N)
        }
        Some("s") => {
            l.token();
            Ok(Slot::S)
        }
        Some("o") => {
            l.token();
            Ok(Slot::O)
        }
        Some("f") => {
            l.token();
            Ok(Slot::F { fill: string_ref(strings, l.uint()?)? })
        }
        Some("k") => {
            l.token();
            Ok(Slot::K(slot_k(l, strings)?))
        }
        _ => Err(l.err("a slot (k, n, f, s or o)")),
    }
}

fn thingy(l: &mut Line, strings: &[String]) -> Result<Thingy, String> {
    let et_file = string_ref(strings, l.uint()?)?;
    let int = l.int()?;
    let mut flags = [None; 3];
    for f in &mut flags {
        if l.is_done() {
            break;
        }
        *f = Some(l.flag()?);
    }
    let [bool1, bool2, bool3] = flags;
    Ok(Thingy { et_file, int, bool1, bool2, bool3 })
}

fn poi(l: &mut Line) -> Result<PoI, String> {
    Ok(PoI { x: l.uint()?, y: l.uint()?, rotation: l.float()?, tag: l.quoted()? })
}

fn key_value(l: &mut Line) -> Result<(String, String), String> {
    let s = l.rest.trim_start();
    let eq = s.find('=').ok_or_else(|| l.err("key=value"))?;
    let key = s[..eq].to_string();
    let after = &s[eq + 1..];
    let (value, rest) = match after.strip_prefix('"') {
        Some(body) => {
            let end = body.find('"').ok_or_else(|| l.err("a closing quote"))?;
            (body[..end].to_string(), &body[end + 1..])
        }
        None => {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..end].to_string(), &after[end..])
        }
    };
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err(l.err("key=value"));
    }
    l.rest = rest;
    Ok((key, value))
}

fn doodad(l: &mut Line, version: u32) -> Result<Doodad, String> {
    let x = l.uint()?;
    let y = l.uint()?;
    let float_pairs = if version >= 34 {
        let n = l.uint()?;
        let mut pairs = Vec::new();
        for _ in 0..n {
            pairs.push((l.float()?, l.float()?));
        }
        Some(pairs)
    } else {
        None
    };
    let radians1 = l.float()?;
    let [trig1, trig2, trig3, trig4] = if version >= 18 { [Some(l.float()?), Some(l.float()?), Some(l.float()?), Some(l.float()?)] } else { [None; 4] };
    let bool1 = l.flag()?;
    let bool2 = if version >= 25 { Some(l.flag()?) } else { None };
    let n = l.uint()?;
    let mut floats = Vec::new();
    for _ in 0..n {
        floats.push(l.float()?);
    }
    let scale = l.float()?;
    let ao_file = l.quoted()?;
    let stub = l.quoted()?;
    let key_values = if version >= 36 {
        let n = l.uint()?;
        let mut kv = BTreeMap::new();
        for _ in 0..n {
            let (k, v) = key_value(l)?;
            kv.insert(k, v);
        }
        Some(kv)
    } else {
        None
    };
    Ok(Doodad { x, y, float_pairs, radians1, trig1, trig2, trig3, trig4, bool1, bool2, floats, scale, ao_file, stub, key_values })
}

fn decal(l: &mut Line, version: u32) -> Result<Decal, String> {
    let (x, y, rotation) = (l.float()?, l.float()?, l.float()?);
    let bool1 = if version >= 17 { Some(l.flag()?) } else { None };
    Ok(Decal { x, y, rotation, bool1, scale: l.float()?, atlas_file: l.quoted()?, tag: l.quoted()? })
}

fn zone(l: &mut Line, version: u32) -> Result<Zone, String> {
    let name = if version >= 35 { l.quoted()? } else { l.token().ok_or_else(|| l.err("a zone name"))?.to_string() };
    let (x_min, y_min, x_max, y_max) = (l.int()?, l.int()?, l.int()?, l.int()?);
    let (disable_teleports, env_file, uint1) = if version >= 35 { (Some(l.quoted()?), Some(l.quoted()?), Some(l.uint()?)) } else { (None, None, None) };
    Ok(Zone { name, x_min, y_min, x_max, y_max, disable_teleports, env_file, uint1 })
}

/// Each line is one or more quoted strings; the last line often runs straight into the next one.
fn boss_lines(r: &mut Reader) -> Result<Vec<Vec<String>>, String> {
    let n = r.whole_line("the boss line count", |l| l.uint())?;
    let mut out = Vec::new();
    for i in 0..n {
        let mut line = r.next("a boss line")?;
        let mut strings = vec![line.quoted()?];
        while line.at_quote() {
            strings.push(line.quoted()?);
        }
        out.push(strings);
        let tail = line.rest.trim();
        if i + 1 == n && !tail.is_empty() {
            r.lines.insert(r.pos, (line.no, tail));
        }
    }
    Ok(out)
}

fn ground_overrides(l: &mut Line, strings: &[String], width: usize, height: usize) -> Result<Vec<Vec<Option<String>>>, String> {
    let cols = width.saturating_sub(1);
    let rows = height.saturating_sub(1);
    if cols == 0 || rows == 0 {
        return Err(l.err("a grid wider than one tile"));
    }
    let mut out = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            row.push(string_ref(strings, l.uint()?)?);
        }
        out.push(row);
    }
    Ok(out)
}

pub fn parse(text: &str) -> Result<ArmFile, String> {
    let lines = text
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| (i + 1, l))
        .collect();
    let mut r = Reader { lines, pos: 0, version: 0 };
    let version = r.whole_line("the version", |l| match l.token() {
        Some("version") => l.uint(),
        _ => Err(l.err("version")),
    })?;
    r.version = version;

    let n = r.whole_line("the string count", |l| l.uint())?;
    let mut strings = Vec::new();
    for _ in 0..n {
        strings.push(r.whole_line("a string", |l| l.quoted())?);
    }

    let dimensions = r.whole_line("the dimensions", |l| {
        let side_length = l.uint()?;
        if version < 31 {
            l.uint()?;
        }
        let uint1 = if version >= 22 { Some(l.uint()?) } else { None };
        Ok(Dimension { side_length, uint1 })
    })?;
    let numbers1 = r.whole_line("the counts line", |l| {
        let mut v = vec![l.uint()?];
        while !l.is_done() {
            v.push(l.uint()?);
        }
        Ok(v)
    })?;
    let tag = r.whole_line("the tag", |l| l.quoted())?;
    let bools = r.whole_line("the flags line", |l| {
        let mut v = vec![l.flag()?];
        while !l.is_done() {
            v.push(l.flag()?);
        }
        Ok(v)
    })?;
    let root_slot = r.whole_line("the root slot", |l| slot(l, &strings))?;
    let (width, height) = root_slot.footprint();
    let (width, height) = (width as usize, height as usize);

    let thingy_count = numbers1.iter().fold(0usize, |a, &n| a.saturating_add(n as usize)).saturating_mul(2);
    let mut thingies = Vec::new();
    for _ in 0..thingy_count {
        thingies.push(r.whole_line("an .et reference", |l| thingy(l, &strings))?);
    }

    let poi_groups = match version {
        ..20 => 9,
        20..26 => 10,
        26..29 => 5,
        29.. => 6,
    };
    let mut points_of_interest = Vec::with_capacity(poi_groups);
    for _ in 0..poi_groups {
        points_of_interest.push(r.group(32, "a point of interest", poi)?);
    }
    let string1 = if version >= 35 { Some(r.whole_line("the room string", |l| l.quoted())?) } else { None };

    let mut grid = Vec::new();
    for _ in 0..height {
        grid.push(r.whole_line("a grid row", |l| (0..width).map(|_| slot(l, &strings)).collect::<Result<Vec<_>, _>>())?);
    }

    let doodads = r.group(32, "a doodad", |l| doodad(l, version))?;
    let doodad_connections = if version >= 23 {
        Some(r.group(32, "a doodad connection", |l| Ok(DoodadConnection { from: l.uint()?, to: l.uint()?, tag: l.quoted()? }))?)
    } else {
        None
    };
    let decals = r.group(32, "a decal", |l| decal(l, version))?;
    let boss_lines = if version >= 22 { Some(boss_lines(&mut r)?) } else { None };
    let zones = if version >= 27 { Some(r.group(33, "a zone", |l| zone(l, version))?) } else { None };
    let tags = r.optional_line(|l| {
        let n = l.uint()?;
        (0..n).map(|_| l.token().map(str::to_string).ok_or_else(|| l.err("a tag"))).collect::<Result<Vec<_>, _>>()
    });
    let ground_overrides = r.optional_line(|l| ground_overrides(l, &strings, width, height));

    if let Some(&(no, rest)) = r.lines.get(r.pos) {
        return Err(format!("line {}: unexpected {:?}", no, rest.chars().take(40).collect::<String>()));
    }

    Ok(ArmFile {
        version,
        strings,
        dimensions,
        numbers1,
        tag,
        bools,
        root_slot,
        thingies,
        points_of_interest,
        string1,
        grid,
        doodads,
        doodad_connections,
        decals,
        boss_lines,
        zones,
        tags,
        ground_overrides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const V26_FILL: &str = r#"version 26
2
"forcedblank"
"Metadata/Terrain/Theopolis/OriathSquare/Feature/square_heavysnow_ground.gt"
2 2 1
1 1
""
0 1
k 2 2 0 0 0 0 6 6 6 6 6 6 6 6 0 0 0 0 0 0 0 0 0 0
0 0
0 0
0 0
0 0
0
0
0
0
0
f 1 f 1
f 1 f 1
2
27 7 0.536359 -0 0 0.264977 0.964255 0 0 0 1 "Metadata/Terrain/Doodads/Act5/Theopolis/OriathSquare/HayStack01.ao" "Metadata/MiscellaneousObjects/Doodad"
12 17 -2.38743 0 0 0.929744 -0.368207 1 0 0 1 "Metadata/Terrain/Doodads/Act5/Theopolis/OriathSquare/HayStack02.ao" "Metadata/MiscellaneousObjects/Doodad"
0
1
439.238 59.4268 0 0 77.6367  "Metadata/Decals/grungedecal.atlas" "slime_small"
0
2
"#;

    const V36_BOSS: &str = r#"version 36
3
"Metadata/Terrain/black_inside_wall.gt"
"Metadata/Terrain/wildcard.et"
"arena"
2 0
1 1
"sun_above_right"
1 1
k 2 2 0 0 0 0 6 6 6 6 6 6 6 6 1 1 1 1 0 0 0 0 0 0
0 0 0
0 0 0
0 0 0
0 0 0
385 276 5.23266 "mapboss"
-1
-1
-1
-1
345 276 3.14159 "entrance"
515 154 4.18879 "telepad"
-1
-1
""
k 2 2 2 0 0 0 6 6 6 6 6 6 6 6 1 1 1 1 0 0 0 0 3 0 n
n n
385 229 0 1.0472 0 0 0.5 0.866025 0 0 1 -188 1 "Metadata/Effects/Environment/area_transition/variants/generic_cylinder_palegreen.ao" "Metadata/MiscellaneousObjects/AreaTransition_Animate" 0
345 372 1 3751.41 4050.98 -0 0 0 0 1 0 0 0 1 "Metadata/Effects/Environment/area_transition/variants/generic_cylinder_palegreen.ao" "Metadata/MiscellaneousObjects/AreaTransition_Animate" 1 transition="teleport6disabled-Arena"
-1
0 1 "sun_visual"
-1
-1
0
"no_critters" 45 12 668 520 "" "" 0
"hide_teleports" 253 182 437 200 "Metadata/MiscellaneousObjects/SectorDisableInstantTeleports" "" 0
-1
1 arena
0
"#;

    const V30_BOSS_LINE: &str = r#"version 30
1
"Metadata/Terrain/wildcard.et"
1 1 0
1 1
"bosscliff"
1 0
k 2 2 1 0 1 0 6 6 6 6 6 6 6 6 0 0 0 0 0 0 0 0 0 0
0 0 0
0 0 0
0 0 0
0 0 0
0
0
0
0
0
0
k 1 1 0 0 0 0 3 3 3 3 3 3 3 3 0 0 0 0 0 0 0 0 0 0 s
o f 0
0
0
0
1
"" "bandit_outside" "" 0
0
"#;

    #[test]
    fn parses_length_prefixed_groups() {
        let arm = parse(V26_FILL).unwrap();
        assert_eq!(arm.version, 26);
        assert_eq!(arm.dimensions.side_length, 2);
        assert_eq!(arm.thingies.len(), 4);
        assert_eq!(arm.points_of_interest.len(), 5);
        assert_eq!(arm.grid.len(), 2);
        assert!(matches!(&arm.grid[1][0], Slot::F { fill: Some(f) } if f == "forcedblank"));
        assert_eq!(arm.doodads.len(), 2);
        let d = &arm.doodads[1];
        assert_eq!((d.x, d.y), (12, 17));
        assert_eq!((d.bool1, d.bool2), (true, Some(false)));
        assert_eq!(d.trig4, Some(-0.368207));
        assert!(d.ao_file.ends_with("HayStack02.ao"));
        assert_eq!(arm.doodad_connections.as_ref().map(Vec::len), Some(0));
        assert_eq!(arm.decals[0].atlas_file, "Metadata/Decals/grungedecal.atlas");
        assert_eq!(arm.decals[0].tag, "slime_small");
        assert!(arm.zones.is_none());
        assert!(arm.tags.is_none());
        let overrides = arm.ground_overrides.unwrap();
        assert_eq!(overrides.len(), 1);
        assert!(overrides[0][0].as_deref().unwrap().ends_with("square_heavysnow_ground.gt"));
    }

    #[test]
    fn parses_terminated_groups_and_key_values() {
        let arm = parse(V36_BOSS).unwrap();
        assert_eq!(arm.version, 36);
        assert_eq!(arm.dimensions.uint1, Some(0));
        assert_eq!(arm.points_of_interest.len(), 6);
        assert_eq!(arm.points_of_interest[0][0].tag, "mapboss");
        assert_eq!(arm.points_of_interest[4].len(), 2);
        assert_eq!(arm.string1.as_deref(), Some(""));
        let Slot::K(k) = &arm.grid[0][0] else { panic!("expected a k slot") };
        assert_eq!(k.slot_tag.as_deref(), Some("arena"));
        assert_eq!(k.origin, Direction::SW);
        assert_eq!(k.edges[0].direction, Direction::S);
        assert_eq!(k.edges[0].edge.as_deref(), Some("Metadata/Terrain/wildcard.et"));
        assert_eq!(k.corners[2].direction, Direction::NE);
        assert_eq!(arm.grid[1].iter().map(Slot::letter).collect::<String>(), "nn");
        let d = &arm.doodads[1];
        assert_eq!(d.float_pairs.as_deref(), Some(&[(3751.41, 4050.98)][..]));
        assert_eq!(d.key_values.as_ref().unwrap()["transition"], "teleport6disabled-Arena");
        assert_eq!(arm.doodads[0].floats, vec![-188.0]);
        assert_eq!(arm.doodad_connections.unwrap()[0].tag, "sun_visual");
        assert!(arm.decals.is_empty());
        let zones = arm.zones.unwrap();
        assert_eq!(zones.len(), 2);
        assert_eq!((zones[1].x_min, zones[1].y_max), (253, 200));
        assert_eq!(zones[1].disable_teleports.as_deref(), Some("Metadata/MiscellaneousObjects/SectorDisableInstantTeleports"));
        assert_eq!(arm.tags, Some(vec!["arena".to_string()]));
        assert_eq!(arm.ground_overrides, Some(vec![vec![None]]));
    }

    #[test]
    fn splits_a_boss_line_that_runs_into_the_next() {
        let arm = parse(V30_BOSS_LINE).unwrap();
        assert_eq!(arm.boss_lines.unwrap(), vec![vec!["".to_string(), "bandit_outside".to_string(), "".to_string()]]);
        assert_eq!(arm.zones.map(|z| z.len()), Some(0));
        assert_eq!(arm.tags, Some(vec![]));
        assert_eq!(arm.grid[0].iter().chain(&arm.grid[1]).map(Slot::letter).collect::<String>(), "ksof");
    }

    #[test]
    fn rejects_broken_files() {
        assert!(parse("nothing").is_err());
        assert!(parse(&V36_BOSS[..V36_BOSS.len() / 2]).is_err());
        assert!(parse(&V26_FILL.replace("f 1 f 1\nf 1", "f 1 f 3\nf 1")).is_err());
    }

    #[test]
    #[ignore]
    fn parse_real_arm() {
        let files = crate::parsers::real_files("arm", 400);
        let mut versions = BTreeMap::new();
        let mut failures = Vec::new();
        let mut ok = 0;
        for (path, bytes) in &files {
            let text = crate::parsers::utils::decode_text_lossy(bytes);
            match parse(&text) {
                Ok(arm) => {
                    ok += 1;
                    *versions.entry(arm.version).or_insert(0) += 1;
                }
                Err(e) => failures.push(format!("{}: {}", path, e)),
            }
        }
        println!("parsed {}/{} .arm files, versions {:?}", ok, files.len(), versions);
        for f in failures.iter().take(10) {
            println!("  {}", f);
        }
        assert!(!files.is_empty());
        assert!(ok * 100 >= files.len() * 95, "only {}/{} parsed", ok, files.len());
    }
}
