//! Writes the insertion-ordered [`J`] value as Python's `json.dump(indent=2)`
//! does, which is the shape RePoE publishes: two-space indent, `": "` between
//! key and value, floats printed at shortest round-trip precision.

pub use crate::skill_tree_export::json::J;
use std::path::Path;

/// Builds an object without the `let mut` dance, keeping field order.
#[derive(Default)]
pub struct Obj(Vec<(String, J)>);

impl Obj {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn set(mut self, key: &str, value: J) -> Self {
        self.0.push((key.to_string(), value));
        self
    }

    /// Sets the key only when there is a value, so absent data stays absent.
    pub fn opt(self, key: &str, value: Option<J>) -> Self {
        match value {
            Some(v) => self.set(key, v),
            None => self,
        }
    }

    /// Sets the key to the value or to `null`.
    pub fn or_null(self, key: &str, value: Option<J>) -> Self {
        self.set(key, value.unwrap_or(J::Null))
    }

    pub fn build(self) -> J {
        J::Obj(self.0)
    }
}

/// The same value with every object key sorted, which is how RePoE writes the
/// files it builds from plain dictionaries.
pub fn sorted(value: J) -> J {
    match value {
        J::Obj(mut fields) => {
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            J::Obj(fields.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        J::Arr(items) => J::Arr(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The same value with every null-valued object key dropped, so an entry
/// carries only what the thing it describes actually has. Array elements keep
/// their places — position is meaning there.
pub fn without_nulls(value: &J) -> J {
    match value {
        J::Obj(fields) => J::Obj(
            fields
                .iter()
                .filter(|(_, v)| !matches!(v, J::Null))
                .map(|(k, v)| (k.clone(), without_nulls(v)))
                .collect(),
        ),
        J::Arr(items) => J::Arr(items.iter().map(without_nulls).collect()),
        other => other.clone(),
    }
}

pub fn text(s: impl AsRef<str>) -> J {
    J::Str(s.as_ref().to_string())
}

pub fn opt_text(s: impl AsRef<str>) -> Option<J> {
    let s = s.as_ref();
    (!s.is_empty()).then(|| J::Str(s.to_string()))
}

pub fn int(v: impl Into<i64>) -> J {
    J::Int(v.into())
}

/// A float straight from the game data, unrounded — `f32` values widen the
/// way Python's `float()` shows them.
pub fn float(v: f32) -> J {
    J::Num(v as f64)
}

pub fn arr(items: impl IntoIterator<Item = J>) -> J {
    J::Arr(items.into_iter().collect())
}

pub fn strings(items: impl IntoIterator<Item = impl AsRef<str>>) -> J {
    J::Arr(items.into_iter().map(|s| text(s)).collect())
}

/// Writes `<dir>/<name>.json` plus the compact `<name>.min.json`, creating
/// parent folders as needed.
pub fn write(dir: &Path, name: &str, value: &J) -> Result<(), String> {
    let path = dir.join(format!("{}.json", name));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, pretty(value)).map_err(|e| format!("{}: {}", path.display(), e))?;
    let min = dir.join(format!("{}.min.json", name));
    std::fs::write(&min, compact(value)).map_err(|e| format!("{}: {}", min.display(), e))?;
    Ok(())
}

pub fn write_text(dir: &Path, name: &str, body: &str) -> Result<(), String> {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, body).map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn pretty(value: &J) -> String {
    let mut out = String::new();
    render(value, 0, true, &mut out);
    out.push('\n');
    out
}

pub fn compact(value: &J) -> String {
    let mut out = String::new();
    render(value, 0, false, &mut out);
    out
}

fn render(value: &J, depth: usize, spaced: bool, out: &mut String) {
    match value {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Int(i) => out.push_str(&i.to_string()),
        J::Num(n) => out.push_str(&number(*n)),
        J::Str(s) => escape(s, out),
        J::Arr(items) if items.is_empty() => out.push_str("[]"),
        J::Obj(fields) if fields.is_empty() => out.push_str("{}"),
        J::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(depth + 1, spaced, out);
                render(item, depth + 1, spaced, out);
            }
            newline(depth, spaced, out);
            out.push(']');
        }
        J::Obj(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(depth + 1, spaced, out);
                escape(key, out);
                out.push(':');
                if spaced {
                    out.push(' ');
                }
                render(item, depth + 1, spaced, out);
            }
            newline(depth, spaced, out);
            out.push('}');
        }
    }
}

fn newline(depth: usize, spaced: bool, out: &mut String) {
    if spaced {
        out.push('\n');
        out.extend(std::iter::repeat(' ').take(depth * 2));
    }
}

/// Integral floats keep a `.0` the way Python prints them; everything else
/// uses Rust's shortest round-trip form, which matches `repr()`.
fn number(n: f64) -> String {
    if !n.is_finite() {
        return "null".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e16 {
        format!("{:.1}", n)
    } else {
        format!("{}", n)
    }
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_like_python_json_dump() {
        let value = Obj::new()
            .set("name", text("Marauder"))
            .set("life", int(16))
            .set("damage", float(9.16))
            .set("tags", strings(["fire", "melee"]))
            .or_null("missing", None)
            .build();
        assert_eq!(
            pretty(&value),
            "{\n  \"name\": \"Marauder\",\n  \"life\": 16,\n  \"damage\": 9.15999984741211,\n  \
             \"tags\": [\n    \"fire\",\n    \"melee\"\n  ],\n  \"missing\": null\n}\n"
        );
        assert_eq!(
            compact(&value),
            "{\"name\":\"Marauder\",\"life\":16,\"damage\":9.15999984741211,\
             \"tags\":[\"fire\",\"melee\"],\"missing\":null}"
        );
    }

    #[test]
    fn stripping_nulls_leaves_arrays_and_their_positions_alone() {
        let value = Obj::new()
            .set("name", text("Gold Amulet"))
            .or_null("requirements", None)
            .set("properties", Obj::new().or_null("armour", None).build())
            .set("implicits", strings(["AmuletImplicitItemFoundRarityIncrease1"]))
            .set("weights", J::Arr(vec![int(1), J::Null, int(3)]))
            .build();
        assert_eq!(
            compact(&without_nulls(&value)),
            "{\"name\":\"Gold Amulet\",\"properties\":{},\
             \"implicits\":[\"AmuletImplicitItemFoundRarityIncrease1\"],\
             \"weights\":[1,null,3]}"
        );
    }

    #[test]
    fn integral_floats_keep_a_decimal_point() {
        assert_eq!(compact(&float(5.0)), "5.0");
        assert_eq!(compact(&int(5)), "5");
        assert_eq!(compact(&J::Arr(Vec::new())), "[]");
    }
}
