//! Insertion-ordered JSON value with a printer that mimics PHP's
//! `json_encode(..., JSON_PRETTY_PRINT)`: four-space indent, `\/` for
//! slashes, `\uXXXX` for anything outside ASCII, floats at six significant
//! digits and integral floats printed as integers.

#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj() -> J {
        J::Obj(Vec::new())
    }

    pub fn str(s: &str) -> J {
        J::Str(s.to_string())
    }

    pub fn strs<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> J {
        J::Arr(items.into_iter().map(|s| J::Str(s.as_ref().to_string())).collect())
    }

    pub fn ints<I: Into<i64>>(items: impl IntoIterator<Item = I>) -> J {
        J::Arr(items.into_iter().map(|i| J::Int(i.into())).collect())
    }

    /// A number rounded to six significant digits (values that round to an
    /// integer become integers, matching PHP's float output).
    pub fn num(v: f64) -> J {
        let r = round_sig(v, 6);
        if r.fract() == 0.0 && r.abs() < 1e15 {
            J::Int(r as i64)
        } else {
            J::Num(r)
        }
    }

    pub fn set(&mut self, key: &str, value: J) {
        if let J::Obj(fields) = self {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                fields.push((key.to_string(), value));
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            J::Arr(a) => a.is_empty(),
            J::Obj(o) => o.is_empty(),
            _ => false,
        }
    }
}

pub fn round_sig(v: f64, digits: i32) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return 0.0;
    }
    let exponent = v.abs().log10().floor() as i32;
    let scale = 10f64.powi(digits - 1 - exponent);
    let r = (v * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r }
}

pub fn to_string_pretty(value: &J) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

fn write_value(out: &mut String, value: &J, depth: usize) {
    match value {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Int(i) => out.push_str(&i.to_string()),
        J::Num(n) => out.push_str(&format_num(*n)),
        J::Str(s) => write_string(out, s),
        J::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        J::Obj(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                indent(out, depth + 1);
                write_string(out, k);
                out.push_str(": ");
                write_value(out, v, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

fn format_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let abs = n.abs();
    if abs < 1e-4 || abs >= 1e15 {
        // PHP switches to exponent form here: 5.82236e-5
        let s = format!("{:e}", n);
        return s;
    }
    format!("{}", n)
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if c.is_ascii() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_to_six_significant_digits() {
        assert_eq!(to_string_pretty(&J::num(-22597.396484375)), "-22597.4");
        assert_eq!(to_string_pretty(&J::num(733.0950317382812)), "733.095");
        assert_eq!(to_string_pretty(&J::num(1332.0)), "1332");
        assert_eq!(to_string_pretty(&J::num(0.004999999888241291)), "0.005");
        assert_eq!(to_string_pretty(&J::num(0.0)), "0");
    }

    #[test]
    fn strings_escape_like_php() {
        assert_eq!(to_string_pretty(&J::str("Art/2DArt/x.png")), "\"Art\\/2DArt\\/x.png\"");
        assert_eq!(to_string_pretty(&J::str("M\u{f3}rrigan")), "\"M\\u00f3rrigan\"");
        assert_eq!(to_string_pretty(&J::str("a\nb")), "\"a\\nb\"");
    }

    #[test]
    fn objects_keep_insertion_order_and_indent_four() {
        let mut o = J::obj();
        o.set("b", J::Int(1));
        o.set("a", J::Arr(vec![]));
        o.set("c", J::Obj(vec![("k".into(), J::Bool(true))]));
        assert_eq!(
            to_string_pretty(&o),
            "{\n    \"b\": 1,\n    \"a\": [],\n    \"c\": {\n        \"k\": true\n    }\n}"
        );
    }
}
