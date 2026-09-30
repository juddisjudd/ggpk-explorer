//! Path of Building's string helpers, ported from `Modules/Common.lua`. PoB
//! runs them over every line it exports, so matching its output means
//! matching these byte for byte — including where they work on bytes rather
//! than characters.

use crate::settings::Game;

/// `sanitiseText`: strips markup and folds typographic characters to ASCII.
/// Only runs when the text has a byte of 128 or more or a `<`; anything
/// non-ASCII left at the end becomes one `?` per byte.
pub fn sanitise_text(text: &str, game: Game) -> String {
    let bytes = text.as_bytes();
    if !bytes.iter().any(|&b| b >= 128 || b == b'<') {
        return text.to_string();
    }
    let mut s = remove_balanced(bytes, b'<', b'>');
    let dash: &[&[u8]] = &[
        b"\xe2\x80\x90",
        b"\xe2\x80\x91",
        b"\xe2\x80\x92",
        b"\xe2\x80\x93",
        b"\xe2\x80\x94",
        b"\xe2\x80\x95",
        b"\xe2\x88\x92",
    ];
    for seq in dash {
        s = replace_bytes(&s, seq, b"-");
    }
    if game == Game::Poe2 {
        s = replace_bytes(&s, b"\xe2\x80\xa2 ", b"");
        s = replace_bytes(&s, b"\xe2\x80\xa2", b"");
    }
    s = replace_bytes(&s, b"\xc3\xa4", b"a");
    s = replace_bytes(&s, b"\xc3\xb6", b"o");
    if game == Game::Poe2 {
        s = replace_bytes(&s, b"\xc3\xad", b"i");
        s = replace_bytes(&s, b"\xc3\xb3", b"o");
    }
    let s: Vec<u8> = s
        .into_iter()
        .map(|b| match b {
            150 | 151 => b'-',
            228 => b'a',
            246 => b'o',
            b if b >= 128 => b'?',
            b => b,
        })
        .collect();
    String::from_utf8(s).expect("only ASCII remains")
}

/// Lua's `gsub("%b<>", "")`: removes each balanced `<…>` run, leaving an
/// unclosed `<` in place.
fn remove_balanced(bytes: &[u8], open: u8, close: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == open {
            let mut depth = 0;
            let mut end = None;
            for (j, &b) in bytes.iter().enumerate().skip(i) {
                if b == close {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                } else if b == open {
                    depth += 1;
                }
            }
            if let Some(end) = end {
                i = end + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// `escapeGGGString`: reduces the client's link markup to the text it draws.
/// `<tag>{text}` becomes `text`, `[Word]` becomes `Word` and `[Link|Text]`
/// becomes `Text`.
pub fn escape_ggg_string(text: &str) -> String {
    let s = text.as_bytes();
    let s = gsub_tagged_braces(s);
    let s = gsub_plain_links(&s);
    let s = gsub_piped_links(&s);
    String::from_utf8(s).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// `gsub("<[^>]+>{([^}]+)}", "%1")`.
fn gsub_tagged_braces(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    'scan: while i < s.len() {
        if s[i] == b'<' {
            let tag_end = run_end(s, i + 1, |b| b != b'>');
            if tag_end > i + 1 && s.get(tag_end) == Some(&b'>') && s.get(tag_end + 1) == Some(&b'{') {
                let start = tag_end + 2;
                let end = run_end(s, start, |b| b != b'}');
                if end > start && s.get(end) == Some(&b'}') {
                    out.extend_from_slice(&s[start..end]);
                    i = end + 1;
                    continue 'scan;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `gsub("%[([^|%]]+)%]", "%1")`.
fn gsub_plain_links(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'[' {
            let end = run_end(s, i + 1, |b| b != b'|' && b != b']');
            if end > i + 1 && s.get(end) == Some(&b']') {
                out.extend_from_slice(&s[i + 1..end]);
                i = end + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `gsub("%[[^|]+|([^|]+)%]", "%1")`. The second run is greedy and backs
/// off to the last `]` before the next `|`, as Lua's matcher does.
fn gsub_piped_links(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'[' {
            let bar = run_end(s, i + 1, |b| b != b'|');
            if bar > i + 1 && s.get(bar) == Some(&b'|') {
                let start = bar + 1;
                let run = run_end(s, start, |b| b != b'|');
                if let Some(close) = (start + 1..run).rev().find(|&p| s[p] == b']') {
                    out.extend_from_slice(&s[start..close]);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn run_end(s: &[u8], from: usize, keep: impl Fn(u8) -> bool) -> usize {
    let mut end = from;
    while end < s.len() && keep(s[end]) {
        end += 1;
    }
    end
}

/// PoB's `round(val, dec)`: half rounds up, `-2.5` to `-2`.
pub fn round(value: f64, decimals: Option<i32>) -> f64 {
    match decimals {
        Some(dec) => {
            let scale = 10f64.powi(dec);
            (value * scale + 0.5).floor() / scale
        }
        None => (value + 0.5).floor(),
    }
}

/// Lua's `string.format` for the single conversions PoB's exporters use:
/// `%d` (truncating, as LuaJIT does with a fractional number), `%g`, `%s`,
/// `%.Nf` and `%.Ng`, each optionally with a `+` or `-` flag.
pub fn format_number(spec: &str, value: f64) -> String {
    let body = spec.trim_start_matches('%');
    let (flags, rest) = body.split_at(body.find(|c: char| c != '+' && c != '-' && c != ' ').unwrap_or(body.len()));
    let plus = flags.contains('+');
    let conv = rest.chars().last().unwrap_or('d');
    let precision = rest.strip_prefix('.').map(|p| p.trim_end_matches(conv).parse::<usize>().unwrap_or(6));
    let mut text = match conv {
        'd' | 'i' => format!("{}", value.trunc() as i64),
        'f' => format!("{:.*}", precision.unwrap_or(6), value),
        'g' => super::lua::format_g(value, precision.unwrap_or(6)),
        's' => super::lua::tostring(value),
        _ => super::lua::tostring(value),
    };
    if text == "-0" && conv == 'd' {
        text = "0".into();
    }
    if plus && !text.starts_with('-') {
        text.insert(0, '+');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_link_markup_like_pob() {
        assert_eq!(escape_ggg_string("[Resistances|Fire Resistance]"), "Fire Resistance");
        assert_eq!(escape_ggg_string("[Resistances]"), "Resistances");
        assert_eq!(escape_ggg_string("<unique>{Mjölner} and [A|B] or [C]"), "Mjölner and B or C");
        assert_eq!(escape_ggg_string("no markup"), "no markup");
    }

    #[test]
    fn sanitises_like_pob() {
        assert_eq!(sanitise_text("plain", Game::Poe2), "plain");
        assert_eq!(sanitise_text("Mjölner", Game::Poe2), "Mjolner");
        assert_eq!(sanitise_text("a\u{2013}b <i>x</i>", Game::Poe1), "a-b x");
        assert_eq!(sanitise_text("\u{2022} Point", Game::Poe2), "Point");
        assert_eq!(sanitise_text("\u{2022} Point", Game::Poe1), "??? Point");
        assert_eq!(sanitise_text("é", Game::Poe2), "??");
        assert_eq!(sanitise_text("unclosed < tag", Game::Poe2), "unclosed < tag");
    }

    #[test]
    fn rounds_and_formats_like_lua() {
        assert_eq!(round(2.5, None), 3.0);
        assert_eq!(round(-2.5, None), -2.0);
        assert_eq!(round(1.25, Some(1)), 1.3);
        assert_eq!(format_number("%d", 2.7), "2");
        assert_eq!(format_number("%d", -0.5), "0");
        assert_eq!(format_number("%+d", 5.0), "+5");
        assert_eq!(format_number("%+d", -5.0), "-5");
        assert_eq!(format_number("%-d", 5.0), "5");
        assert_eq!(format_number("%g", 0.1 + 0.2), "0.3");
        assert_eq!(format_number("%.2f", 1.5), "1.50");
    }
}
