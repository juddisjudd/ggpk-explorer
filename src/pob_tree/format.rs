//! The two files PoB-PoE2's tree exporter writes: `tree.lua` through
//! `writeLuaTable` and `tree.json` through PoB's copy of dkjson, which sorts
//! every object's keys numbers first.

use crate::pob_export::lua::{self, Key, Lua, Table};
use std::cmp::Ordering;

/// Keys the way both writers order them: numbers before strings, numbers by
/// value, strings byte by byte.
fn sorted(t: &Table) -> Vec<(&Key, &Lua)> {
    let mut entries: Vec<_> = t.iter().collect();
    entries.sort_by(|(a, _), (b, _)| match (a, b) {
        (Key::Int(x), Key::Int(y)) => x.cmp(y),
        (Key::Int(_), Key::Str(_)) => Ordering::Less,
        (Key::Str(_), Key::Int(_)) => Ordering::Greater,
        (Key::Str(x), Key::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
    });
    entries
}

/// `out:write('return ') writeLuaTable(out, t, 1)`: one key per line, tabs,
/// every array index written out, no trailing newline.
pub fn lua_source(t: &Table) -> String {
    let mut out = String::from("return ");
    write_table(t, 1, &mut out);
    out
}

fn write_table(t: &Table, indent: usize, out: &mut String) {
    out.push_str("{\n");
    let entries = sorted(t);
    for (i, (key, value)) in entries.iter().enumerate() {
        out.extend(std::iter::repeat_n('\t', indent));
        match key {
            Key::Str(s) if bare(s) => {
                out.push_str(s);
                out.push('=');
            }
            Key::Str(s) => {
                out.push('[');
                quote_lua(s, out);
                out.push_str("]=");
            }
            Key::Int(n) => {
                out.push('[');
                out.push_str(&lua::tostring(*n as f64));
                out.push_str("]=");
            }
        }
        match value {
            Lua::Table(t) => write_table(t, indent + 1, out),
            Lua::Str(s) => quote_lua(s, out),
            Lua::Num(n) if *n == f64::INFINITY => out.push_str("math.huge"),
            Lua::Num(n) => out.push_str(&lua::tostring(*n)),
            Lua::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        }
        if i + 1 < entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.extend(std::iter::repeat_n('\t', indent - 1));
    out.push('}');
}

/// `k:match("^%a[%a%d]*$") and k ~= "hexproof" and k ~= "in"`.
fn bare(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric())
        && key != "hexproof"
        && key != "in"
}

/// `qFmt`: only newlines and double quotes are escaped.
fn quote_lua(s: &str, out: &mut String) {
    out.push('"');
    out.push_str(&s.replace('\n', "\\n").replace('"', "\\\""));
    out.push('"');
}

/// `json.encode(t)` with PoB's dkjson: no whitespace, keys sorted numbers
/// first, number keys quoted, arrays by dkjson's `isarray` rule.
pub fn json(value: &Lua) -> String {
    let mut out = String::new();
    encode(value, &mut out);
    out
}

fn encode(value: &Lua, out: &mut String) {
    match value {
        Lua::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Lua::Num(n) if !n.is_finite() => out.push_str("null"),
        Lua::Num(n) => out.push_str(&lua::tostring(*n)),
        Lua::Str(s) => lua::quote(s, out),
        Lua::Table(t) => {
            if let Some(max) = lua::array_length(t) {
                out.push('[');
                for i in 1..=max {
                    match t.get(i) {
                        Some(v) => encode(v, out),
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
            for (i, (key, value)) in sorted(t).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                match key {
                    Key::Str(s) => lua::quote(s, out),
                    Key::Int(n) => lua::quote(&lua::tostring(*n as f64), out),
                }
                out.push(':');
                encode(value, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut inner = Table::new();
        inner.set("max_x", 1.5).set("in", "a\"b\nc").set("x", 2);
        let mut t = Table::new();
        t.set("nodes", Table::list([7, 3])).set("empty", Table::new()).set(4, inner).set("tree", "Default");
        t
    }

    #[test]
    fn lua_matches_write_lua_table() {
        assert_eq!(
            lua_source(&sample()),
            "return {\n\t[4]={\n\t\t[\"in\"]=\"a\\\"b\\nc\",\n\t\t[\"max_x\"]=1.5,\n\t\tx=2\n\t},\n\
             \tempty={\n\t},\n\tnodes={\n\t\t[1]=7,\n\t\t[2]=3\n\t},\n\ttree=\"Default\"\n}"
        );
    }

    #[test]
    fn json_sorts_numbers_first_and_quotes_them() {
        assert_eq!(
            json(&Lua::Table(sample())),
            "{\"4\":{\"in\":\"a\\\"b\\nc\",\"max_x\":1.5,\"x\":2},\"empty\":[],\"nodes\":[7,3],\"tree\":\"Default\"}"
        );
    }
}
