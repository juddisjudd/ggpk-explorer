//! The value model of a Path of Building data file, and the JSON it becomes.
//!
//! PoB's `src/Data/**/*.lua` files are Lua tables. repoe-fork publishes them
//! as JSON by evaluating each file and encoding the result with dkjson, every
//! key sorted (`lua/Generate.lua` in repoe-fork/pob-data). [`encode`] follows
//! dkjson 2.8 byte for byte, so a file written here diffs cleanly against
//! theirs: a table is an array only when its keys are 1..n with few holes, an
//! empty table is `[]`, numbers print the way LuaJIT's `tostring` does
//! (`%.14g`), and string keys sort before numeric ones.

use std::collections::BTreeMap;
use std::path::Path;

/// A table key. String keys sort first, byte by byte, then integer keys in
/// numeric order — the order `Generate.lua` hands dkjson.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Str(String),
    Int(i64),
}

impl From<&str> for Key {
    fn from(s: &str) -> Self {
        Key::Str(s.to_string())
    }
}

impl From<String> for Key {
    fn from(s: String) -> Self {
        Key::Str(s)
    }
}

impl From<&String> for Key {
    fn from(s: &String) -> Self {
        Key::Str(s.clone())
    }
}

macro_rules! int_key {
    ($($t:ty),*) => {$(
        impl From<$t> for Key {
            fn from(i: $t) -> Self {
                Key::Int(i as i64)
            }
        }
    )*};
}
int_key!(i64, i32, u32, u16, u8, usize, u64);

/// One Lua value. Lua has no null inside a table — an absent key is how PoB
/// says "none" — so there is no nil variant; leave the key out instead.
#[derive(Clone, Debug, PartialEq)]
pub enum Lua {
    Bool(bool),
    Num(f64),
    Str(String),
    Table(Table),
}

impl From<&str> for Lua {
    fn from(s: &str) -> Self {
        Lua::Str(s.to_string())
    }
}

impl From<String> for Lua {
    fn from(s: String) -> Self {
        Lua::Str(s)
    }
}

impl From<&String> for Lua {
    fn from(s: &String) -> Self {
        Lua::Str(s.clone())
    }
}

impl From<bool> for Lua {
    fn from(b: bool) -> Self {
        Lua::Bool(b)
    }
}

impl From<f64> for Lua {
    fn from(n: f64) -> Self {
        Lua::Num(n)
    }
}

/// Widened as-is, the way a DAT float lands in a Lua number. Where PoB rounds
/// on the way out, round before converting.
impl From<f32> for Lua {
    fn from(n: f32) -> Self {
        Lua::Num(n as f64)
    }
}

macro_rules! int_value {
    ($($t:ty),*) => {$(
        impl From<$t> for Lua {
            fn from(i: $t) -> Self {
                Lua::Num(i as f64)
            }
        }
    )*};
}
int_value!(i64, i32, u32, u16, u8, usize, u64);

impl From<Table> for Lua {
    fn from(t: Table) -> Self {
        Lua::Table(t)
    }
}

impl Lua {
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Lua::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut Table> {
        match self {
            Lua::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Lua::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Lua::Num(n) => Some(*n),
            _ => None,
        }
    }
}

/// A Lua table. Keys are kept sorted, which is the only order the encoder
/// ever writes them in; `seq` is `#t`, kept current so appending stays cheap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    map: BTreeMap<Key, Lua>,
    seq: usize,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    /// `{ a, b, c }`: the values at keys 1..n.
    pub fn list<V: Into<Lua>>(items: impl IntoIterator<Item = V>) -> Self {
        let mut t = Self::new();
        for item in items {
            t.push(item);
        }
        t
    }

    /// `{ [k] = true, … }`, PoB's way of writing a set.
    pub fn set_of<K: Into<Key>>(keys: impl IntoIterator<Item = K>) -> Self {
        let mut t = Self::new();
        for key in keys {
            t.set(key, true);
        }
        t
    }

    /// `t[k] = v`, replacing any value already there.
    pub fn set(&mut self, key: impl Into<Key>, value: impl Into<Lua>) -> &mut Self {
        let key = key.into();
        let extends = key == Key::Int(self.seq as i64 + 1);
        self.map.insert(key, value.into());
        if extends {
            while self.map.contains_key(&Key::Int(self.seq as i64 + 1)) {
                self.seq += 1;
            }
        }
        self
    }

    /// Sets the key only when there is a value, since Lua cannot store nil.
    pub fn set_opt<V: Into<Lua>>(&mut self, key: impl Into<Key>, value: Option<V>) -> &mut Self {
        if let Some(value) = value {
            self.set(key, value);
        }
        self
    }

    /// Builder form of [`set`](Self::set).
    pub fn with(mut self, key: impl Into<Key>, value: impl Into<Lua>) -> Self {
        self.set(key, value);
        self
    }

    /// Builder form of [`set_opt`](Self::set_opt).
    pub fn with_opt<V: Into<Lua>>(mut self, key: impl Into<Key>, value: Option<V>) -> Self {
        self.set_opt(key, value);
        self
    }

    /// `table.insert(t, v)`: stores the value at `#t + 1`.
    pub fn push(&mut self, value: impl Into<Lua>) -> &mut Self {
        let next = self.seq as i64 + 1;
        self.set(next, value)
    }

    pub fn remove(&mut self, key: impl Into<Key>) -> Option<Lua> {
        let key = key.into();
        if let Key::Int(i) = key {
            if i >= 1 && i as usize <= self.seq {
                self.seq = i as usize - 1;
            }
        }
        self.map.remove(&key)
    }

    pub fn get(&self, key: impl Into<Key>) -> Option<&Lua> {
        self.map.get(&key.into())
    }

    pub fn get_mut(&mut self, key: impl Into<Key>) -> Option<&mut Lua> {
        self.map.get_mut(&key.into())
    }

    /// The table stored at a key, created empty when absent. Panics when the
    /// key holds something other than a table, which is a porting mistake.
    pub fn table_mut(&mut self, key: impl Into<Key>) -> &mut Table {
        let key = key.into();
        if !self.map.contains_key(&key) {
            self.set(key.clone(), Table::new());
        }
        match self.map.get_mut(&key).expect("just inserted") {
            Lua::Table(t) => t,
            other => panic!("expected a table, found {:?}", other),
        }
    }

    pub fn contains(&self, key: impl Into<Key>) -> bool {
        self.map.contains_key(&key.into())
    }

    /// Lua's `#t` for a sequence: the count of consecutive keys from 1.
    pub fn len(&self) -> usize {
        self.seq
    }

    /// No keys at all.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Number of keys of any kind.
    pub fn count(&self) -> usize {
        self.map.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Lua)> {
        self.map.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Key, &mut Lua)> {
        self.map.iter_mut()
    }

    /// The values at 1..#t, in order.
    pub fn values(&self) -> impl Iterator<Item = &Lua> {
        (1..=self.seq).filter_map(move |i| self.map.get(&Key::Int(i as i64)))
    }
}

/// C's `%.<precision>g`: the shortest of fixed and exponent notation at that
/// many significant digits, trailing zeros removed.
pub fn format_g(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let precision = precision.max(1);
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // Rounding to the precision can carry into a new digit (9.99 -> 10.0),
    // so the exponent is read from the rounded scientific form.
    let sci = format!("{:.*e}", precision - 1, value);
    let (mantissa, exp) = sci.split_once('e').expect("scientific notation has an exponent");
    let exp: i32 = exp.parse().expect("exponent is an integer");
    if exp < -4 || exp >= precision as i32 {
        let mantissa = strip_zeros(mantissa);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", mantissa, sign, exp.abs())
    } else {
        let decimals = (precision as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{:.*}", decimals, value)).to_string()
    }
}

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// LuaJIT's `tostring` for a number.
pub fn tostring(value: f64) -> String {
    format_g(value, 14)
}

/// Writes `<dir>/<name>.json` and `<dir>/<name>.min.json`, as repoe-fork
/// publishes every file, creating folders as needed.
pub fn write(dir: &Path, name: &str, value: &Lua) -> Result<(), String> {
    for (suffix, indent) in [(".json", true), (".min.json", false)] {
        let path = dir.join(format!("{}{}", name, suffix));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        std::fs::write(&path, encode(value, indent)).map_err(|e| format!("{}: {}", path.display(), e))?;
    }
    Ok(())
}

/// dkjson's `json.encode(value, { indent = indent, keyorder = … })`.
pub fn encode(value: &Lua, indent: bool) -> String {
    let mut out = String::new();
    encode_value(value, indent, 0, &mut out);
    out
}

fn encode_value(value: &Lua, indent: bool, level: usize, out: &mut String) {
    match value {
        Lua::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Lua::Num(n) if !n.is_finite() => out.push_str("null"),
        Lua::Num(n) => out.push_str(&tostring(*n)),
        Lua::Str(s) => quote(s, out),
        Lua::Table(t) => encode_table(t, indent, level + 1, out),
    }
}

fn encode_table(t: &Table, indent: bool, level: usize, out: &mut String) {
    if let Some(max) = array_length(t) {
        out.push('[');
        for i in 1..=max {
            match t.map.get(&Key::Int(i as i64)) {
                Some(v) => encode_value(v, indent, level, out),
                None => out.push_str("null"),
            }
            if i < max {
                out.push(',');
            }
        }
        out.push(']');
        return;
    }
    out.push('{');
    for (i, (key, value)) in t.map.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        if indent {
            newline(level, out);
        }
        match key {
            Key::Str(s) => quote(s, out),
            Key::Int(n) => quote(&tostring(*n as f64), out),
        }
        out.push(':');
        encode_value(value, indent, level, out);
    }
    if indent {
        newline(level - 1, out);
    }
    out.push('}');
}

fn newline(level: usize, out: &mut String) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// dkjson's `isarray`: every key a positive integer, and not so sparse that
/// the holes outnumber the values. A numeric `n` field counts as a length.
/// Returns how many slots the array has.
pub(crate) fn array_length(t: &Table) -> Option<usize> {
    let (mut max, mut n, mut declared) = (0f64, 0usize, 0f64);
    for (key, value) in &t.map {
        match (key, value) {
            (Key::Str(s), Lua::Num(v)) if s == "n" => {
                declared = *v;
                max = max.max(*v);
            }
            (Key::Int(i), _) if *i >= 1 => {
                max = max.max(*i as f64);
                n += 1;
            }
            _ => return None,
        }
    }
    if max > 10.0 && max > declared && max > (n * 2) as f64 {
        return None;
    }
    Some(max.floor() as usize)
}

/// dkjson's `quotestring`: control characters, `"`, `\` and DEL escaped, plus
/// the invisible and line-breaking code points JavaScript chokes on.
pub(crate) fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn needs_escape(c: char) -> bool {
    matches!(c as u32,
        0x00..=0x1f | 0x7f | 0x80..=0x9f | 0xad | 0x600..=0x604 | 0x70f | 0x17b4 | 0x17b5
        | 0x200c..=0x200f | 0x2028..=0x202f | 0x2060..=0x206f | 0xfeff | 0xfff0..=0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_luajit_tostring() {
        // Expected values printed by `luajit -e "print(tostring(x))"`.
        let cases: [(f64, &str); 16] = [
            (0.1 + 0.2, "0.3"),
            (1e15, "1e+15"),
            (123456789012345.0, "1.2345678901234e+14"),
            (12345678901234.0, "12345678901234"),
            (-0.0, "-0"),
            (1e100, "1e+100"),
            (9007199254740992.0, "9.007199254741e+15"),
            (1.0 / 3.0, "0.33333333333333"),
            (1e14, "1e+14"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (0.13f32 as f64, "0.12999999523163"),
            (3.452f32 as f64, "3.4519999027252"),
            (-25.0, "-25"),
            (4294967295.0, "4294967295"),
            (2.5e-7, "2.5e-07"),
        ];
        for (value, expected) in cases {
            assert_eq!(tostring(value), expected, "tostring({:e})", value);
        }
        assert_eq!(format_g(1234567.0, 6), "1.23457e+06");
        assert_eq!(format_g(100000.0, 6), "100000");
        assert_eq!(format_g(2.0 / 3.0, 6), "0.666667");
        assert_eq!(format_g(9.9999999, 6), "10");
    }

    #[test]
    fn indented_output_matches_dkjson() {
        let costs = Table::list([
            Table::new().with("Divisor", 1).with("Resource", "Mana"),
            Table::new().with("Divisor", 60).with("Resource", "Life"),
        ]);
        assert_eq!(
            encode(&Lua::Table(costs), true),
            "[{\n    \"Divisor\":1,\n    \"Resource\":\"Mana\"\n  },{\n    \"Divisor\":60,\n    \"Resource\":\"Life\"\n  }]"
        );
        let item = Table::new().with(
            "Amber Amulet",
            Table::new()
                .with("req", Table::new().with("level", 8))
                .with("implicitModTypes", Table::list([Table::list(["attribute"])]))
                .with("tags", Table::set_of(["amulet", "default"]))
                .with("empty", Table::new()),
        );
        assert_eq!(
            encode(&Lua::Table(item), true),
            "{\n  \"Amber Amulet\":{\n    \"empty\":[],\n    \"implicitModTypes\":[[\"attribute\"]],\n    \
             \"req\":{\n      \"level\":8\n    },\n    \"tags\":{\n      \"amulet\":true,\n      \
             \"default\":true\n    }\n  }\n}"
        );
    }

    #[test]
    fn string_keys_sort_before_numeric_ones() {
        let mut t = Table::new();
        t.set(1, "Adds 1 to 2 Cold damage").set("weightVal", Table::list([1, 0])).set("affix", "Frosted");
        t.set("tradeHashes", Table::new().with(4067062424u64, Table::list(["x"])).with(12, true));
        assert_eq!(
            encode(&Lua::Table(t), false),
            "{\"affix\":\"Frosted\",\"tradeHashes\":{\"12\":true,\"4067062424\":[\"x\"]},\
             \"weightVal\":[1,0],\"1\":\"Adds 1 to 2 Cold damage\"}"
        );
    }

    #[test]
    fn sparse_tables_become_objects_and_small_holes_nulls() {
        let few_holes = Table::new().with(1, "a").with(3, "c");
        assert_eq!(encode(&Lua::Table(few_holes), false), "[\"a\",null,\"c\"]");
        let sparse = Table::new().with(1, "a").with(20, "t");
        assert_eq!(encode(&Lua::Table(sparse), false), "{\"1\":\"a\",\"20\":\"t\"}");
        let from_zero = Table::new().with(0, "zero").with(1, "one");
        assert_eq!(encode(&Lua::Table(from_zero), false), "{\"0\":\"zero\",\"1\":\"one\"}");
    }

    #[test]
    fn escapes_what_dkjson_escapes() {
        let s = Lua::from("a\"b\\c\n\u{7f}\u{2028}\u{feff}/é");
        assert_eq!(encode(&s, false), "\"a\\\"b\\\\c\\n\\u007f\\u2028\\ufeff/é\"");
    }

    #[test]
    fn non_finite_numbers_are_null() {
        assert_eq!(encode(&Lua::Num(f64::INFINITY), false), "null");
        assert_eq!(encode(&Lua::Table(Table::list([f64::NAN])), false), "[null]");
    }
}
