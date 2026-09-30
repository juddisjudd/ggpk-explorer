//! Path of Building's reading of the stat description files, ported from
//! `Export/statdesc.lua` (the describer every exporter renders stat text
//! with) and `Export/Scripts/statdesc.lua` (which turns each description file
//! into the Lua table PoB ships as `Data/StatDescriptions/*.lua`).
//!
//! Both read the file line by line with Lua patterns, and PoB's output
//! depends on their quirks: only the value's minimum is tested against a
//! line's limits, a value handler rewrites the caller's stat in place, and a
//! line is only reached through the `description` keyword. They are kept
//! here rather than reusing `dat::csd`, whose job is the client's rules.

use super::lua::{Lua, Table};
use super::text::{escape_ggg_string, format_number, round, sanitise_text};
use crate::settings::Game;
use std::collections::HashMap;

/// `convertUTF16to8`: little-endian UTF-16 to UTF-8, stopping at a NUL. The
/// byte-order mark is dropped; PoB keeps it but only ever on a line its
/// patterns match anywhere in.
pub fn decode_utf16(bytes: &[u8]) -> String {
    let units = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0);
    let text: String = char::decode_utf16(units).filter_map(Result::ok).collect();
    text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
}

/// The string Lua reads back from a literal PoB wrote with `"…"` around it.
/// PoB writes description text without escaping backslashes, so the `\n` a
/// line carries becomes a real newline in the loaded table.
pub fn lua_literal(raw: &str) -> String {
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        i += 2;
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' => out.push(c),
            b'0'..=b'9' => {
                let mut value = (c - b'0') as u32;
                let mut digits = 1;
                while digits < 3 && i < b.len() && b[i].is_ascii_digit() {
                    value = value * 10 + (b[i] - b'0') as u32;
                    i += 1;
                    digits += 1;
                }
                out.push(value as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Lua's `text:gmatch("[^\r\n]+")`.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\r', '\n']).filter(|l| !l.is_empty())
}

/// One side of a line's limit: `#`, `!`, a number, or anything else PoB
/// kept as a string.
#[derive(Clone, Debug, PartialEq)]
pub enum Limit {
    Any,
    Not,
    Num(f64),
    Str(String),
}

impl Limit {
    fn parse(s: &str) -> Limit {
        match s {
            "#" => Limit::Any,
            _ => match lua_tonumber(s) {
                Some(n) => Limit::Num(n),
                None => Limit::Str(s.to_string()),
            },
        }
    }

    fn to_lua(&self) -> Lua {
        match self {
            Limit::Any => "#".into(),
            Limit::Not => "!".into(),
            Limit::Num(n) => Lua::Num(*n),
            Limit::Str(s) => s.as_str().into(),
        }
    }
}

/// A handler token after a line's text: `negate 1`, `reminderstring X`,
/// `canonical_line`.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecValue {
    Num(f64),
    Str(String),
    True,
}

impl SpecValue {
    fn to_lua(&self) -> Lua {
        match self {
            SpecValue::Num(n) => Lua::Num(*n),
            SpecValue::Str(s) => s.as_str().into(),
            SpecValue::True => true.into(),
        }
    }
}

/// One wording of a description.
#[derive(Clone, Debug, PartialEq)]
pub struct Wording {
    /// As PoB stores it, before Lua reads the literal back.
    pub text: String,
    pub limits: Vec<(Limit, Limit)>,
    pub specs: Vec<(String, SpecValue)>,
    /// The quality word before the text, when it names `gem_quality`.
    pub quality: Option<String>,
}

/// Lua's `tonumber` on a limit or handler token.
fn lua_tonumber(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')) {
        t.parse().ok()
    } else {
        None
    }
}

/// Parses the part of a line after the description header:
/// `([%d%-#| !]+)%s*([%w_]*)%s*"(.-)"%s*(.*)`, searched anywhere in the line.
fn parse_wording(line: &str, game: Game, for_export: bool) -> Option<Wording> {
    let b = line.as_bytes();
    let in_limits = |c: u8| c.is_ascii_digit() || matches!(c, b'-' | b'#' | b'|' | b' ' | b'!');
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for start in 0..b.len() {
        if !in_limits(b[start]) {
            continue;
        }
        let mut i = start;
        while i < b.len() && in_limits(b[i]) {
            i += 1;
        }
        // The limit run is greedy, but Lua backs off from it when what
        // follows does not match; only whole runs can be followed by `"` or a
        // word here, so trying the full run and then shorter ones suffices.
        for end in (start + 1..=i).rev() {
            if let Some(w) = wording_after(b, start, end, &is_space, &is_word) {
                return Some(finish_wording(w, game, for_export));
            }
        }
    }
    None
}

struct RawWording<'a> {
    limits: &'a str,
    quality: &'a str,
    text: &'a str,
    special: &'a str,
}

fn wording_after<'a>(
    b: &'a [u8],
    start: usize,
    end: usize,
    is_space: &dyn Fn(u8) -> bool,
    is_word: &dyn Fn(u8) -> bool,
) -> Option<RawWording<'a>> {
    let s = |from: usize, to: usize| std::str::from_utf8(&b[from..to]).ok();
    let mut i = end;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    let q_start = i;
    while i < b.len() && is_word(b[i]) {
        i += 1;
    }
    let q_end = i;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let t_start = i + 1;
    let t_end = t_start + b[t_start..].iter().position(|&c| c == b'"')?;
    let mut j = t_end + 1;
    while j < b.len() && is_space(b[j]) {
        j += 1;
    }
    Some(RawWording {
        limits: s(start, end)?,
        quality: s(q_start, q_end)?,
        text: s(t_start, t_end)?,
        special: s(j, b.len())?,
    })
}

fn finish_wording(raw: RawWording, game: Game, for_export: bool) -> Wording {
    let mut text = escape_ggg_string(raw.text);
    if game == Game::Poe2 {
        if for_export {
            text = sanitise_text(&text, game);
        }
        text = double_backslash_fix(&text);
    }
    let limits = raw
        .limits
        .split(|c: char| !(c.is_ascii_digit() || matches!(c, '!' | '-' | '#' | '|')))
        .filter(|s| !s.is_empty())
        .map(parse_limit)
        .collect();
    let tokens: Vec<&str> = raw
        .special
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '%' || c == '_'))
        .filter(|s| !s.is_empty())
        .collect();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "canonical_line" {
            specs.push(("canonical_line".to_string(), SpecValue::True));
            i += 1;
        } else if let Some(value) = tokens.get(i + 1) {
            let v = match lua_tonumber(value) {
                Some(n) => SpecValue::Num(n),
                None => SpecValue::Str(value.to_string()),
            };
            specs.push((tokens[i].to_string(), v));
            i += 2;
        } else {
            i += 1;
        }
    }
    let quality = raw.quality.contains("gem_quality").then(|| raw.quality.to_string());
    Wording { text, limits, specs, quality }
}

/// PoE 2's `gsub("\\([^nb])", "\\n%1")`: a backslash before anything but
/// `n` or `b` gains an `n`, so the literal PoB writes stays valid Lua.
fn double_backslash_fix(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] != b'n' && b[i + 1] != b'b' {
            out.extend_from_slice(b"\\n");
            out.push(b[i + 1]);
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn parse_limit(token: &str) -> (Limit, Limit) {
    if token == "#" {
        return (Limit::Any, Limit::Any);
    }
    let plain = token.strip_prefix('-').unwrap_or(token);
    if !plain.is_empty() && plain.bytes().all(|c| c.is_ascii_digit()) {
        let n = lua_tonumber(token).unwrap_or(0.0);
        return (Limit::Num(n), Limit::Num(n));
    }
    if let Some(rest) = token.strip_prefix('!') {
        let digits = rest.strip_prefix('-').unwrap_or(rest);
        if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
            return (Limit::Not, Limit::Num(lua_tonumber(rest).unwrap_or(0.0)));
        }
    }
    // `([%d%-#]+)|([%d%-#]+)`, searched anywhere in the token.
    let side = |c: char| c.is_ascii_digit() || c == '-' || c == '#';
    if let Some(bar) = token.find('|') {
        let left: String = token[..bar].chars().rev().take_while(|&c| side(c)).collect::<Vec<_>>().into_iter().rev().collect();
        let right: String = token[bar + 1..].chars().take_while(|&c| side(c)).collect();
        if !left.is_empty() && !right.is_empty() {
            return (Limit::parse(&left), Limit::parse(&right));
        }
    }
    (Limit::Str(String::new()), Limit::Str(String::new()))
}

/// Lua's `line:match("%d+%s+([%w_%+%-%% ]+)")`: the stat ids after a count.
fn stats_line(line: &str) -> Option<Vec<String>> {
    let b = line.as_bytes();
    let in_ids = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-' | b'%' | b' ');
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | 0x0b | 0x0c);
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            // `%d+` is greedy but backs off: any digit run followed by
            // whitespace works, so try each end point from the longest.
            for d_end in (i + 1..=j).rev() {
                let mut k = d_end;
                while k < b.len() && is_space(b[k]) {
                    k += 1;
                }
                if k == d_end {
                    continue;
                }
                // `%s+` then the capture: the capture may begin with spaces
                // the `%s+` left behind, but greedy `%s+` takes them all.
                let c_start = k;
                let mut c_end = c_start;
                while c_end < b.len() && in_ids(b[c_end]) {
                    c_end += 1;
                }
                if c_end > c_start {
                    let ids = &line[c_start..c_end];
                    return Some(
                        ids.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '%')))
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect(),
                    );
                }
                // Backing off `%s+` by one leaves a space to start the capture.
                if k - d_end >= 2 && b[k - 1] == b' ' {
                    let ids = &line[k - 1..];
                    let end = ids.bytes().position(|c| !in_ids(c)).unwrap_or(ids.len());
                    return Some(
                        ids[..end]
                            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '%')))
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect(),
                    );
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

/// Lua's `line:match("no_description ([%w_%+%-%%]+)")`.
fn no_description(line: &str) -> Option<&str> {
    let at = line.find("no_description ")?;
    let rest = &line[at + "no_description ".len()..];
    let end = rest
        .bytes()
        .position(|c| !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-' | b'%')))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// Whether PoB takes the line as the start of a new description.
fn is_description_header(line: &str) -> bool {
    line.contains("handed_description") || (line.contains("description") && !line.contains("_description"))
}

/// Lua's `line:match("description ([%w_]+)")`.
fn description_name(line: &str) -> Option<String> {
    let at = line.find("description ")?;
    let rest = &line[at + "description ".len()..];
    let end = rest.bytes().position(|c| !(c.is_ascii_alphanumeric() || c == b'_')).unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

fn lang_line(line: &str) -> bool {
    // `line:match('lang "(.+)"')`: `lang "` followed by at least one
    // character and a later quote.
    line.match_indices("lang \"").any(|(at, _)| {
        let rest = &line[at + 6..];
        rest.len() >= 2 && rest[1..].contains('"')
    })
}

/// Where each game keeps its description files, and their extension.
pub fn description_dir(game: Game) -> (&'static str, &'static str) {
    match game {
        Game::Poe2 => ("Data/StatDescriptions/", ".csd"),
        Game::Poe1 => ("Metadata/StatDescriptions/", ".txt"),
    }
}

/// A description as the describer holds it.
#[derive(Clone, Debug)]
pub struct Descriptor {
    /// `None` for a `no_description` entry.
    pub stats: Option<Vec<String>>,
    pub order: f64,
    pub wordings: Vec<Wording>,
}

/// One stat's value going through the describer, which rewrites it in place.
#[derive(Clone, Debug, PartialEq)]
pub struct StatValue {
    pub min: Val,
    pub max: Val,
    fmt: String,
}

impl StatValue {
    pub fn new(min: f64, max: f64) -> Self {
        Self { min: Val::Num(min), max: Val::Num(max), fmt: "d".into() }
    }

    /// The `fmt` a description left on the value (`d`, `g`, …), which PoB's
    /// passive exporters write out with the stat.
    pub fn fmt(&self) -> &str {
        &self.fmt
    }
}

/// A value is a number until a handler turns it into a name.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
}

impl Val {
    fn num(&self) -> f64 {
        match self {
            Val::Num(n) => *n,
            Val::Str(_) => 0.0,
        }
    }

    fn format(&self, spec: &str) -> String {
        match self {
            Val::Num(n) => format_number(spec, *n),
            Val::Str(s) => s.clone(),
        }
    }
}

/// The stats handed to `describeStats`, in the order the caller built them.
/// Lookups are by id; the handlers mutate entries in place, as PoB's do.
#[derive(Clone, Debug, Default)]
pub struct Stats(pub Vec<(String, StatValue)>);

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a stat.
    pub fn set(&mut self, id: &str, min: f64, max: f64) -> &mut Self {
        match self.0.iter_mut().find(|(s, _)| s == id) {
            Some((_, v)) => *v = StatValue::new(min, max),
            None => self.0.push((id.to_string(), StatValue::new(min, max))),
        }
        self
    }

    pub fn get(&self, id: &str) -> Option<&StatValue> {
        self.0.iter().find(|(s, _)| s == id).map(|(_, v)| v)
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.0.iter().position(|(s, _)| s == id)
    }
}

/// What `describeStats` returns: the lines, the order of each line, and the
/// stats no description covers.
#[derive(Clone, Debug, Default)]
pub struct Described {
    pub lines: Vec<String>,
    pub orders: Vec<f64>,
    pub missing: Vec<String>,
}

/// One loaded description file with its includes, as `loadStatFile` builds
/// it.
#[derive(Clone, Debug, Default)]
pub struct Descriptors {
    pub game: Game,
    pub descriptors: Vec<Descriptor>,
    pub by_stat: HashMap<String, usize>,
}

impl Descriptors {
    /// `loadStatFile(name)`. `fetch` reads a game file's bytes; `name` is the
    /// file name under the game's description folder, extension included.
    pub fn load(game: Game, name: &str, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let mut out = Descriptors { game, ..Default::default() };
        out.parse_into(name, 1.0, fetch);
        out
    }

    /// PoE 1's `loadStatFile(first, …)` with more files: each later file is
    /// parsed on its own, its orders renumbered from 1, and its stats laid
    /// over the first file's.
    pub fn append(&mut self, name: &str, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let start = self
            .descriptors
            .iter()
            .filter(|d| d.order > 0.0)
            .map(|d| d.order + 1.0)
            .fold(1.0, f64::max);
        let mut extra = Descriptors { game: self.game, ..Default::default() };
        extra.parse_into(name, start, fetch);
        let offset = self.descriptors.len();
        for mut d in extra.descriptors {
            if d.order > 0.0 {
                d.order = (d.order - start + 1.0).max(1.0);
            }
            self.descriptors.push(d);
        }
        for (stat, index) in extra.by_stat {
            self.by_stat.insert(stat, index + offset);
        }
    }

    fn parse_into(&mut self, name: &str, first_order: f64, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) -> f64 {
        let mut state = ParseState { order: first_order, current: None, english: false };
        self.parse_file(name, &mut state, fetch);
        state.order
    }

    fn parse_file(&mut self, name: &str, state: &mut ParseState, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let (dir, _) = description_dir(self.game);
        let Some(bytes) = fetch(&format!("{}{}", dir, name)) else { return };
        let text = decode_utf16(&bytes);
        for line in lines(&text) {
            self.process_line(line, state, fetch);
        }
    }

    fn process_line(&mut self, line: &str, state: &mut ParseState, fetch: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let (dir, _) = description_dir(self.game);
        let include_prefix = format!("include \"{}", dir);
        if let Some(at) = line.find(&include_prefix) {
            let rest = &line[at + include_prefix.len()..];
            if let Some(name) = rest.strip_suffix('"').filter(|n| !n.is_empty()) {
                let name = name.to_string();
                self.parse_file(&name, state, fetch);
                return;
            }
        }
        if let Some(stat) = no_description(line) {
            self.descriptors.push(Descriptor { stats: None, order: 0.0, wordings: Vec::new() });
            self.by_stat.insert(stat.to_string(), self.descriptors.len() - 1);
        } else if is_description_header(line) {
            self.descriptors.push(Descriptor { stats: None, order: state.order, wordings: Vec::new() });
            state.current = Some(self.descriptors.len() - 1);
            state.english = true;
            state.order += 1.0;
        } else if state.current.is_some_and(|c| self.descriptors[c].stats.is_none()) {
            if let Some(stats) = stats_line(line) {
                let current = state.current.expect("checked above");
                for stat in &stats {
                    self.by_stat.insert(stat.clone(), current);
                }
                self.descriptors[current].stats = Some(stats);
            }
        } else if state.current.is_some() {
            if lang_line(line) {
                state.english = false;
            } else if !line.contains("table_only") && state.english {
                if let Some(w) = parse_wording(line, self.game, false) {
                    let current = state.current.expect("checked above");
                    self.descriptors[current].wordings.push(w);
                }
            }
        }
    }

    /// `describeStats`: renders every stat a description covers, in the
    /// file's order, rewriting the values in `stats` as the handlers do.
    pub fn describe_stats(&self, stats: &mut Stats) -> Described {
        let mut out = Described::default();
        let mut chosen: Vec<usize> = Vec::new();
        for (id, value) in &stats.0 {
            if id == "Type" {
                continue;
            }
            match self.by_stat.get(id).filter(|&&d| self.descriptors[d].stats.is_some()) {
                Some(&d) => {
                    if (value.min.num() != 0.0 || value.max.num() != 0.0) && !chosen.contains(&d) {
                        chosen.push(d);
                    }
                }
                None => out.missing.push(id.clone()),
            }
        }
        chosen.sort_by(|a, b| self.descriptors[*a].order.total_cmp(&self.descriptors[*b].order));

        for d in chosen {
            let descriptor = &self.descriptors[d];
            let ids = descriptor.stats.as_ref().expect("chosen descriptors have stats");
            // A stat the caller did not pass is a zero that belongs to this
            // call only; a stat it did pass is shared and gets rewritten.
            let mut locals: Vec<StatValue> = Vec::new();
            let mut slots: Vec<Slot> = ids
                .iter()
                .map(|id| match stats.index(id) {
                    Some(i) => Slot::Shared(i),
                    None => {
                        locals.push(StatValue::new(0.0, 0.0));
                        Slot::Local(locals.len() - 1)
                    }
                })
                .collect();
            for slot in &slots {
                slot.get_mut(stats, &mut locals).fmt = "d".into();
            }
            let mut wording = self.match_limit(&descriptor.wordings, &slots, stats, &locals);
            if wording.is_none() {
                let mut adjusted = Vec::new();
                for (i, slot) in slots.iter().enumerate() {
                    let v = slot.get_mut(stats, &mut locals);
                    let (min, max) = (v.min.num(), v.max.num());
                    if min == 0.0 && max > 0.0 {
                        v.min = Val::Num(1.0);
                        adjusted.push((i, true));
                    } else if min < 0.0 && max == 0.0 {
                        v.max = Val::Num(-1.0);
                        adjusted.push((i, false));
                    }
                }
                wording = self.match_limit(&descriptor.wordings, &slots, stats, &locals);
                for (i, was_min) in adjusted {
                    let v = slots[i].get_mut(stats, &mut locals);
                    if was_min {
                        v.min = Val::Num(0.0);
                    } else {
                        v.max = Val::Num(0.0);
                    }
                }
            }
            let Some(wording) = wording else { continue };
            for (key, arg) in &wording.specs {
                let SpecValue::Num(n) = arg else { continue };
                let Some(slot) = slots.get_mut((*n as usize).wrapping_sub(1)) else { continue };
                let v = slot.get_mut(stats, &mut locals);
                apply_handler(self.game, key, v);
            }
            let values: Vec<StatValue> = slots.iter_mut().map(|s| s.get_mut(stats, &mut locals).clone()).collect();
            let text = fill_placeholders(&wording.text, &values, self.game);
            let mut order = descriptor.order;
            for line in split_escaped_lines(&text) {
                out.lines.push(match self.game {
                    Game::Poe2 => sanitise_text(&line, self.game),
                    Game::Poe1 => line,
                });
                out.orders.push(order);
                order += 0.1;
            }
        }
        out
    }

    /// `matchLimit`: the first wording whose limits accept every value's
    /// minimum.
    fn match_limit<'w>(
        &self,
        wordings: &'w [Wording],
        slots: &[Slot],
        stats: &Stats,
        locals: &[StatValue],
    ) -> Option<&'w Wording> {
        wordings.iter().find(|w| {
            w.limits.iter().enumerate().all(|(i, (lo, hi))| {
                let Some(slot) = slots.get(i) else { return true };
                let min = slot.get(stats, locals).min.num();
                match lo {
                    Limit::Not => !matches!(hi, Limit::Num(n) if min == *n),
                    _ => {
                        let above = matches!(hi, Limit::Num(n) if min > *n);
                        let below = matches!(lo, Limit::Num(n) if min < *n);
                        !above && !below
                    }
                }
            })
        })
    }
}

struct ParseState {
    order: f64,
    current: Option<usize>,
    english: bool,
}

#[derive(Clone, Copy)]
enum Slot {
    Shared(usize),
    Local(usize),
}

impl Slot {
    fn get<'a>(&self, stats: &'a Stats, locals: &'a [StatValue]) -> &'a StatValue {
        match *self {
            Slot::Shared(i) => &stats.0[i].1,
            Slot::Local(i) => &locals[i],
        }
    }

    fn get_mut<'a>(&self, stats: &'a mut Stats, locals: &'a mut [StatValue]) -> &'a mut StatValue {
        match *self {
            Slot::Shared(i) => &mut stats.0[i].1,
            Slot::Local(i) => &mut locals[i],
        }
    }
}

/// The value handlers of `describeStats`, with each game's own set and its
/// bugs (`divide_by_three` divides the maximum's already-divided minimum).
fn apply_handler(game: Game, key: &str, v: &mut StatValue) {
    let (min, max) = (v.min.num(), v.max.num());
    let set = |v: &mut StatValue, a: f64, b: f64| {
        v.min = Val::Num(a);
        v.max = Val::Num(b);
    };
    let poe2 = game == Game::Poe2;
    match key {
        "negate" => set(v, -max, -min),
        "invert_chance" => set(v, 100.0 - max, 100.0 - min),
        "negate_and_double" => set(v, -2.0 * max, -2.0 * min),
        "passive_hash" if min < 0.0 => set(v, min + 65536.0, max + 65536.0),
        "divide_by_two_0dp" if poe2 => set(v, round(min / 2.0, None), round(max / 2.0, None)),
        "divide_by_two_0dp" => set(v, min / 2.0, max / 2.0),
        "divide_by_three" if poe2 => {
            let m = min / 3.0;
            set(v, m, m / 3.0);
            v.fmt = "g".into();
        }
        "divide_by_four" => {
            let m = min / 4.0;
            set(v, m, if poe2 { m / 4.0 } else { max / 4.0 });
            v.fmt = "g".into();
        }
        "divide_by_five" => g(v, min / 5.0, max / 5.0),
        "divide_by_six" => g(v, min / 6.0, max / 6.0),
        "divide_by_ten_0dp" if poe2 => set(v, round(min / 10.0, None), round(max / 10.0, None)),
        "divide_by_ten_0dp" => set(v, min / 10.0, max / 10.0),
        "divide_by_ten_1dp" | "divide_by_ten_1dp_if_required" => {
            g(v, round(min / 10.0, Some(1)), round(max / 10.0, Some(1)))
        }
        "divide_by_twelve" => g(v, min / 12.0, max / 12.0),
        "divide_by_fifteen_0dp" if poe2 => set(v, round(min / 15.0, None), round(max / 15.0, None)),
        "divide_by_fifteen_0dp" => set(v, min / 15.0, max / 15.0),
        "divide_by_twenty" => g(v, min / 20.0, max / 20.0),
        "divide_by_twenty_then_double_0dp" if poe2 => {
            set(v, round(min / 20.0, None) * 2.0, round(max / 20.0, None) * 2.0)
        }
        "divide_by_fifty" if poe2 => g(v, min / 50.0, max / 50.0),
        "divide_by_one_hundred" => g(v, min / 100.0, max / 100.0),
        "divide_by_one_hundred_0dp" if poe2 => g(v, round(min / 100.0, None), round(max / 100.0, None)),
        "divide_by_one_hundred_1dp" => g(v, round(min / 100.0, Some(1)), round(max / 100.0, Some(1))),
        "divide_by_one_hundred_2dp_if_required" | "divide_by_one_hundred_2dp" => {
            g(v, round(min / 100.0, Some(2)), round(max / 100.0, Some(2)))
        }
        "divide_by_one_hundred_and_negate" if poe2 => g(v, -min / 100.0, -max / 100.0),
        "divide_by_one_thousand" if poe2 => g(v, min / 1000.0, max / 1000.0),
        "divide_by_one_thousand" => g(v, round(min / 1000.0, Some(1)), round(max / 1000.0, Some(1))),
        "divide_by_ten_thousand_1dp" if poe2 => g(v, round(min / 10000.0, Some(1)), round(max / 10000.0, Some(1))),
        "per_minute_to_per_second" if poe2 => g(v, min / 60.0, max / 60.0),
        "per_minute_to_per_second" => g(v, round(min / 60.0, Some(1)), round(max / 60.0, Some(1))),
        "per_minute_to_per_second_0dp" if poe2 => set(v, round(min / 60.0, None), round(max / 60.0, None)),
        "per_minute_to_per_second_0dp" => set(v, min / 60.0, max / 60.0),
        "per_minute_to_per_second_1dp" => g(v, round(min / 60.0, Some(1)), round(max / 60.0, Some(1))),
        "per_minute_to_per_second_2dp_if_required" | "per_minute_to_per_second_2dp" => {
            g(v, round(min / 60.0, Some(2)), round(max / 60.0, Some(2)))
        }
        "permyriad_per_minute_to_%_per_second" if !poe2 => {
            g(v, round(min / 60.0 / 100.0, Some(1)), round(max / 60.0 / 100.0, Some(1)))
        }
        "milliseconds_to_seconds" => g(v, min / 1000.0, max / 1000.0),
        "milliseconds_to_seconds_halved" if poe2 => g(v, min / 1000.0 / 2.0, max / 1000.0 / 2.0),
        "milliseconds_to_seconds_0dp" if poe2 => set(v, round(min / 1000.0, None), round(max / 1000.0, None)),
        "milliseconds_to_seconds_0dp" => set(v, min / 1000.0, max / 1000.0),
        "milliseconds_to_seconds_1dp" if poe2 => g(v, round(min / 1000.0, Some(1)), round(max / 1000.0, Some(1))),
        "milliseconds_to_seconds_2dp" | "milliseconds_to_seconds_2dp_if_required" => {
            g(v, round(min / 1000.0, Some(2)), round(max / 1000.0, Some(2)))
        }
        "deciseconds_to_seconds" => {
            set(v, min / 10.0, max / 10.0);
            v.fmt = ".2f".into();
        }
        "locations_to_metres" if !poe2 => g(v, min / 10.0, max / 10.0),
        "30%_of_value" => set(v, min * 0.3, max * 0.3),
        "60%_of_value" => set(v, min * 0.6, max * 0.6),
        "mod_value_to_item_class" => v.fmt = "s".into(),
        "one_hundred_divide_by_value" if poe2 => g(v, round(100.0 / min, Some(2)), round(100.0 / max, Some(2))),
        "multiplicative_damage_modifier" => set(v, 100.0 + min, 100.0 + max),
        "multiplicative_permyriad_damage_modifier" if poe2 => g(v, 100.0 + min / 100.0, 100.0 + max / 100.0),
        "times_one_point_five" => set(v, min * 1.5, max * 1.5),
        "double" => set(v, min * 2.0, max * 2.0),
        "multiply_by_four" => set(v, min * 4.0, max * 4.0),
        "multiply_by_four_and_negate" if poe2 => set(v, -min * 4.0, -max * 4.0),
        "multiply_by_ten" if poe2 => set(v, min * 10.0, max * 10.0),
        "times_twenty" => set(v, min * 20.0, max * 20.0),
        "multiply_by_one_hundred" if poe2 => set(v, min * 100.0, max * 100.0),
        "plus_two_hundred" => set(v, min + 200.0, max + 200.0),
        _ => {}
    }
}

fn g(v: &mut StatValue, min: f64, max: f64) {
    v.min = Val::Num(min);
    v.max = Val::Num(max);
    v.fmt = "g".into();
}

/// The three `gsub` passes that put values into a line, then `%%` to `%`.
fn fill_placeholders(text: &str, values: &[StatValue], game: Game) -> String {
    let value = |n: usize| values.get(n).cloned().unwrap_or_else(|| StatValue::new(0.0, 0.0));
    let plain = |v: &StatValue| -> String {
        let spec = format!("%{}", v.fmt);
        if v.min == v.max {
            v.min.format(&spec)
        } else {
            format!("({}-{})", v.min.format(&spec), v.max.format(&spec))
        }
    };
    // `{(%d)}`
    let pass1 = replace_braces(text, |inner| {
        let b = inner.as_bytes();
        (b.len() == 1 && b[0].is_ascii_digit()).then(|| plain(&value((b[0] - b'0') as usize)))
    });
    // `{}`
    let pass2 = pass1.replace("{}", &plain(&value(0)));
    // `{(%d?):([%+%-]?)d?}` — PoE 1 only knows `+`.
    let pass3 = replace_braces(&pass2, |inner| {
        let b = inner.as_bytes();
        let mut i = 0;
        let n = if i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            (b[0] - b'0') as usize
        } else {
            0
        };
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        let sign = match b.get(i) {
            Some(b'+') => {
                i += 1;
                "+"
            }
            Some(b'-') if game == Game::Poe2 => {
                i += 1;
                "-"
            }
            _ => "",
        };
        if b.get(i) == Some(&b'd') {
            i += 1;
        }
        if i != b.len() {
            return None;
        }
        let v = value(n);
        let fmt = v.fmt.clone();
        Some(if v.min == v.max {
            v.min.format(&format!("%{}{}", sign, fmt))
        } else if sign == "+" || sign == "-" {
            if v.max.num() < 0.0 {
                let neg = |x: &Val| match x {
                    Val::Num(n) => Val::Num(-n),
                    other => other.clone(),
                };
                format!("-({}-{})", neg(&v.min).format(&format!("%{}", fmt)), neg(&v.max).format(&format!("%{}", fmt)))
            } else {
                format!("+({}-{})", v.min.format(&format!("%{}", fmt)), v.max.format(&format!("%{}", fmt)))
            }
        } else {
            format!("({}-{})", v.min.format(&format!("%{}{}", sign, fmt)), v.max.format(&format!("%{}{}", sign, fmt)))
        })
    });
    pass3.replace("%%", "%")
}

/// Replaces each `{…}` (no nested braces) whose inside `f` accepts.
fn replace_braces(text: &str, f: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if !after[..close].contains('{') => match f(&after[..close]) {
                Some(rep) => {
                    out.push_str(&rep);
                    rest = &after[close + 1..];
                }
                None => {
                    out.push('{');
                    rest = after;
                }
            },
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `(text.."\\n"):gmatch("([^\\]+)\\n")`: the lines between literal `\n`
/// pairs. A run broken by any other backslash is found again from each of
/// its later characters, as gmatch does.
fn split_escaped_lines(text: &str) -> Vec<String> {
    let s = format!("{}\\n", text);
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
            continue;
        }
        let end = i + b[i..].iter().position(|&c| c == b'\\').unwrap_or(b.len() - i);
        if b.get(end + 1) == Some(&b'n') {
            out.push(String::from_utf8_lossy(&b[i..end]).into_owned());
            i = end + 2;
        } else {
            i += 1;
        }
    }
    out
}

/// `Data/StatDescriptions/<name>.lua` as `Export/Scripts/statdesc.lua`
/// writes it: every description in file order at 1..n, each stat id mapped to
/// the index of its description, and `parent` naming the file this one
/// includes.
pub fn export_file(text: &str, game: Game) -> Table {
    let mut out = Table::new();
    let mut count: i64 = 0;
    let mut current: Option<i64> = None;
    let mut english = false;
    let mut prepend = String::new();
    let (dir, ext) = description_dir(game);
    for raw_line in lines(text) {
        let joined;
        let line = if prepend.is_empty() {
            raw_line
        } else {
            joined = format!("{}{}", prepend, raw_line);
            prepend.clear();
            &joined
        };
        let include = format!("include \"{}", dir);
        if let Some(at) = line.find(&include) {
            let rest = &line[at + include.len()..];
            if let Some(parent) = rest.strip_suffix(&format!("{}\"", ext)).filter(|p| !p.is_empty()) {
                let parent = match game {
                    Game::Poe2 => parent.replace('\\', "/").replace("/statset", "_statset"),
                    Game::Poe1 => parent.to_string(),
                };
                out.set("parent", parent);
                continue;
            }
        }
        if let Some(stat) = no_description(line) {
            count += 1;
            out.set(count, Table::new().with("stats", Table::list([stat])));
            out.set(stat, count);
        } else if is_description_header(line) {
            count += 1;
            let mut descriptor = Table::new().with(1, Table::new());
            descriptor.set_opt("name", description_name(line));
            out.set(count, descriptor);
            current = Some(count);
            english = true;
        } else if current.map_or(true, |c| !has_stats(&out, c)) {
            match stats_line(line) {
                Some(stats) => {
                    if let Some(c) = current {
                        for stat in &stats {
                            out.set(stat.as_str(), count);
                        }
                        descriptor_mut(&mut out, c).set("stats", Table::list(stats));
                    }
                }
                None => prepend = line.to_string(),
            }
        } else if lang_line(line) {
            english = false;
        } else if english && !line.contains("table_only") {
            if let (Some(c), Some(w)) = (current, parse_wording(line, game, true)) {
                let mut desc = Table::new()
                    .with("text", lua_literal(&w.text))
                    .with("limit", Table::list(w.limits.iter().map(|(a, b)| Table::list([a.to_lua(), b.to_lua()]))));
                for (k, v) in &w.specs {
                    desc.push(Table::new().with("k", k.as_str()).with("v", v.to_lua()));
                }
                if let Some(q) = &w.quality {
                    desc.set(q.as_str(), true);
                }
                descriptor_mut(&mut out, c).table_mut(1).push(desc);
            }
        }
    }
    out
}

fn has_stats(out: &Table, index: i64) -> bool {
    out.get(index).and_then(Lua::as_table).is_some_and(|d| d.contains("stats"))
}

fn descriptor_mut(out: &mut Table, index: i64) -> &mut Table {
    out.table_mut(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "no_description dummy_stat\r\n\
description\r\n\
\t1 base_fire_damage_resistance_%\r\n\
\t2\r\n\
\t\t1|# \"{0:+d}% to [Resistances|Fire Resistance]\"\r\n\
\t\t#|-1 \"{0}% reduced [Fire] Resistance\" negate 1\r\n\
\tlang \"French\"\r\n\
\t1\r\n\
\t\t# \"Résistance\"\r\n\
description\r\n\
\t2 local_minimum_added_fire_damage local_maximum_added_fire_damage\r\n\
\t1\r\n\
\t\t# # \"Adds {0} to {1} Fire damage\"\r\n";

    fn descriptors() -> Descriptors {
        let bytes: Vec<u8> = SAMPLE.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        Descriptors::load(Game::Poe2, "x.csd", &|path| (path == "Data/StatDescriptions/x.csd").then(|| bytes.clone()))
    }

    #[test]
    fn describes_a_range_and_negates_in_place() {
        let d = descriptors();
        let mut stats = Stats::new();
        stats.set("base_fire_damage_resistance_%", 10.0, 15.0);
        let out = d.describe_stats(&mut stats);
        assert_eq!(out.lines, ["+(10-15)% to Fire Resistance"]);
        let mut stats = Stats::new();
        stats.set("base_fire_damage_resistance_%", -15.0, -10.0);
        let out = d.describe_stats(&mut stats);
        assert_eq!(out.lines, ["(10-15)% reduced Fire Resistance"]);
        // The handler rewrote the caller's value, as PoB's does.
        assert_eq!(stats.get("base_fire_damage_resistance_%").unwrap().min, Val::Num(10.0));
    }

    #[test]
    fn renders_several_stats_and_reports_the_undescribed() {
        let d = descriptors();
        let mut stats = Stats::new();
        stats.set("local_maximum_added_fire_damage", 5.0, 8.0);
        stats.set("local_minimum_added_fire_damage", 2.0, 2.0);
        stats.set("dummy_stat", 1.0, 1.0);
        let out = d.describe_stats(&mut stats);
        assert_eq!(out.lines, ["Adds 2 to (5-8) Fire damage"]);
        assert_eq!(out.orders, [2.0]);
        assert_eq!(out.missing, ["dummy_stat"]);
    }

    #[test]
    fn exports_the_file_as_pob_writes_it() {
        let t = export_file(SAMPLE, Game::Poe2);
        let json = super::super::lua::encode(&Lua::Table(t), false);
        assert_eq!(
            json,
            "{\"base_fire_damage_resistance_%\":2,\"dummy_stat\":1,\"local_maximum_added_fire_damage\":3,\
             \"local_minimum_added_fire_damage\":3,\"1\":{\"stats\":[\"dummy_stat\"]},\
             \"2\":{\"stats\":[\"base_fire_damage_resistance_%\"],\"1\":[{\"limit\":[[1,\"#\"]],\
             \"text\":\"{0:+d}% to Fire Resistance\"},{\"limit\":[[\"#\",-1]],\
             \"text\":\"{0}% reduced Fire Resistance\",\"1\":{\"k\":\"negate\",\"v\":1}}]},\
             \"3\":{\"stats\":[\"local_minimum_added_fire_damage\",\"local_maximum_added_fire_damage\"],\
             \"1\":[{\"limit\":[[\"#\",\"#\"],[\"#\",\"#\"]],\"text\":\"Adds {0} to {1} Fire damage\"}]}}"
        );
    }

    #[test]
    fn splits_lines_on_literal_backslash_n() {
        assert_eq!(split_escaped_lines("a\\nb"), ["a", "b"]);
        // gmatch restarts inside the second pair, as Lua does.
        assert_eq!(split_escaped_lines("a\\n\\nb"), ["a", "nb"]);
        assert_eq!(lua_literal("line one\\nline two"), "line one\nline two");
    }
}
