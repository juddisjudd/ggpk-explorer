//! Line-oriented UTF-16 terrain and animation text formats, parsed into typed structs for JSON export.

pub mod amd;
pub mod cht;
pub mod clt;
pub mod dct;
pub mod ddt;
pub mod dlp;
pub mod ecf;
pub mod et;
pub mod gcf;
pub mod gft;
pub mod gt;
pub mod mtd;
pub mod rs;
pub mod tgt;
pub mod tmo;
pub mod toy;
pub mod tsi;
pub mod tst;

use serde::Serialize;

pub const EXTENSIONS: &[&str] = &[
    "amd", "cht", "clt", "dct", "ddt", "dlp", "ecf", "et", "gcf", "gft", "gt", "mtd", "rs", "tgt", "tmo", "toy", "tsi", "tst",
];

/// The parsed file as pretty JSON in field order, or `None` when `ext` is not one of these formats.
pub fn to_json(ext: &str, text: &str) -> Option<Result<String, String>> {
    fn json<T: Serialize>(parsed: Result<T, String>) -> Result<String, String> {
        parsed.and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
    }
    Some(match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "amd" => json(amd::parse(text)),
        "cht" => json(cht::parse(text)),
        "clt" => json(clt::parse(text)),
        "dct" => json(dct::parse(text)),
        "ddt" => json(ddt::parse(text)),
        "dlp" => json(dlp::parse(text)),
        "ecf" => json(ecf::parse(text)),
        "et" => json(et::parse(text)),
        "gcf" => json(gcf::parse(text)),
        "gft" => json(gft::parse(text)),
        "gt" => json(gt::parse(text)),
        "mtd" => json(mtd::parse(text)),
        "rs" => json(rs::parse(text)),
        "tgt" => json(tgt::parse(text)),
        "tmo" => json(tmo::parse(text)),
        "toy" => json(toy::parse(text)),
        "tsi" => json(tsi::parse(text)),
        "tst" => json(tst::parse(text)),
        _ => return None,
    })
}

fn has_ext(path: &str, ext: &str) -> bool {
    let (path, ext) = (path.as_bytes(), ext.as_bytes());
    path.len() > ext.len() && path[path.len() - ext.len() - 1] == b'.' && path[path.len() - ext.len()..].eq_ignore_ascii_case(ext)
}

/// A position within one line (or a whole file); every reader returns `None` without moving on failure.
pub(crate) struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { rest: text }
    }

    pub fn done(&self) -> bool {
        self.rest.is_empty()
    }

    pub fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    pub fn sp(&mut self) -> Option<&mut Self> {
        let trimmed = self.rest.trim_start();
        (trimmed.len() < self.rest.len()).then(|| {
            self.rest = trimmed;
            self
        })
    }

    pub fn skip_space(&mut self) -> &mut Self {
        self.rest = self.rest.trim_start();
        self
    }

    pub fn skip_inline_space(&mut self) -> &mut Self {
        self.rest = self.rest.trim_start_matches([' ', '\t']);
        self
    }

    pub fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let saved = self.rest;
        let out = f(self);
        if out.is_none() {
            self.rest = saved;
        }
        out
    }

    pub fn many<T>(&mut self, mut f: impl FnMut(&mut Self) -> Option<T>) -> Vec<T> {
        let mut out = Vec::new();
        while let Some(v) = self.opt(&mut f) {
            out.push(v);
        }
        out
    }

    pub fn lit(&mut self, s: &str) -> Option<&mut Self> {
        self.rest = self.rest.strip_prefix(s)?;
        Some(self)
    }

    pub fn eat(&mut self, s: &str) -> bool {
        self.lit(s).is_some()
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let end = self.rest.find(|c: char| !f(c)).unwrap_or(self.rest.len());
        let (head, tail) = self.rest.split_at(end);
        self.rest = tail;
        head
    }

    pub fn rest(&mut self) -> &'a str {
        std::mem::take(&mut self.rest)
    }

    pub fn remaining(&self) -> &'a str {
        self.rest
    }

    /// Up to and including the next `pat`, or to the end.
    pub fn skip_past(&mut self, pat: &str) {
        self.rest = self.rest.find(pat).map_or("", |i| &self.rest[i + pat.len()..]);
    }

    /// `f` after any whitespace, with the text it stopped at on failure.
    pub fn expect<T>(&mut self, what: &str, f: impl FnOnce(&mut Self) -> Option<T>) -> Result<T, String> {
        self.skip_space();
        self.opt(f).ok_or_else(|| {
            let found = self.rest.lines().next().unwrap_or("").trim();
            format!("expected {what}, found `{}`", found.chars().take(120).collect::<String>())
        })
    }

    pub fn rest_of_line(&mut self) -> &'a str {
        self.take_while(|c| c != '\n' && c != '\r')
    }

    pub fn word(&mut self) -> Option<&'a str> {
        let w = self.take_while(|c| !c.is_whitespace());
        (!w.is_empty()).then_some(w)
    }

    pub fn quoted(&mut self) -> Option<&'a str> {
        let body = self.rest.strip_prefix('"')?;
        let end = body.find('"')?;
        self.rest = &body[end + 1..];
        Some(&body[..end])
    }

    pub fn file(&mut self, ext: &str) -> Option<String> {
        self.opt(|c| c.quoted().filter(|p| has_ext(p, ext)).map(String::from))
    }

    /// Decimal digits not followed by a `.`, so a float is never read as its integer part.
    pub fn uint(&mut self) -> Option<u32> {
        self.opt(|c| {
            let digits = c.take_while(|ch| ch.is_ascii_digit());
            if c.peek() == Some('.') {
                return None;
            }
            digits.parse().ok()
        })
    }

    pub fn int(&mut self) -> Option<i32> {
        self.opt(|c| {
            let sign = c.lit("-").is_some();
            let v = c.uint()? as i64;
            i32::try_from(if sign { -v } else { v }).ok()
        })
    }

    pub fn float(&mut self) -> Option<f64> {
        self.opt(|c| c.take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E')).parse().ok())
    }

    pub fn bool(&mut self) -> Option<bool> {
        self.opt(|c| match c.uint()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        })
    }

    /// `-1` as `None`, anything from `0` up as itself.
    pub fn nullable_uint(&mut self) -> Option<Option<u32>> {
        self.opt(|c| match c.int()? {
            -1 => Some(None),
            v => u32::try_from(v).ok().map(Some),
        })
    }
}

/// The non-blank, non-comment lines of a file, trimmed, read one at a time.
pub(crate) struct Lines<'a> {
    lines: Vec<(usize, &'a str)>,
    pos: usize,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str, comment: Option<&str>) -> Self {
        Self::split(text, &['\n'], comment)
    }

    pub fn split(text: &'a str, seps: &[char], comment: Option<&str>) -> Self {
        let lines = text
            .lines()
            .enumerate()
            .flat_map(|(n, line)| line.split(seps).map(move |part| (n + 1, part.trim())))
            .filter(|(_, l)| !l.is_empty() && !comment.is_some_and(|c| l.starts_with(c)))
            .collect();
        Self { lines, pos: 0 }
    }

    pub fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.pos).map(|(_, l)| *l)
    }

    pub fn done(&self) -> bool {
        self.pos >= self.lines.len()
    }

    pub fn error(&self, what: &str) -> String {
        match self.lines.get(self.pos) {
            Some((n, line)) => format!("line {n}: expected {what}, found `{}`", line.chars().take(120).collect::<String>()),
            None => format!("expected {what}, found the end of the file"),
        }
    }

    /// The next line read whole by `f`, without consuming it on failure.
    pub fn try_line<T>(&mut self, f: impl FnOnce(&mut Cursor<'a>) -> Option<T>) -> Option<T> {
        let mut c = Cursor::new(self.peek()?);
        let v = f(&mut c).filter(|_| c.done())?;
        self.pos += 1;
        Some(v)
    }

    /// Runs `f`, rewinding to the current line when it fails.
    pub fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let saved = self.pos;
        let out = f(self);
        if out.is_none() {
            self.pos = saved;
        }
        out
    }

    pub fn line<T>(&mut self, what: &str, f: impl FnOnce(&mut Cursor<'a>) -> Option<T>) -> Result<T, String> {
        self.try_line(f).ok_or_else(|| self.error(what))
    }

    pub fn many<T>(&mut self, mut f: impl FnMut(&mut Cursor<'a>) -> Option<T>) -> Vec<T> {
        let mut out = Vec::new();
        while let Some(v) = self.try_line(&mut f) {
            out.push(v);
        }
        out
    }

    pub fn count<T>(&mut self, n: usize, what: &str, mut f: impl FnMut(&mut Cursor<'a>) -> Option<T>) -> Result<Vec<T>, String> {
        (0..n).map(|_| self.line(what, &mut f)).collect()
    }

    pub fn version(&mut self) -> Result<u32, String> {
        self.line("`version N`", version)
    }

    pub fn end(&self) -> Result<(), String> {
        if self.done() { Ok(()) } else { Err(self.error("the end of the file")) }
    }
}

fn version(c: &mut Cursor) -> Option<u32> {
    let v = c.lit("version")?.sp()?.uint()?;
    if c.skip_inline_space().eat("//") {
        c.rest_of_line();
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_reads_tokens() {
        let mut c = Cursor::new("12 \"a b\" -1.5e2 7.5");
        assert_eq!(c.uint(), Some(12));
        assert_eq!(c.sp().and_then(|c| c.quoted()), Some("a b"));
        assert_eq!(c.sp().and_then(|c| c.float()), Some(-150.0));
        assert_eq!(c.sp().and_then(|c| c.uint()), None);
        assert_eq!(c.float(), Some(7.5));
        assert!(c.done());
        assert!(has_ext("Metadata/a.AO", "ao"));
        assert!(!has_ext("Metadata/a.mao", "ao"));
    }

    #[test]
    fn lines_skip_blanks_and_comments() {
        let mut lines = Lines::new("version 2\r\n\r\n// note\n  x  \n", Some("//"));
        assert_eq!(lines.version(), Ok(2));
        assert_eq!(lines.line("x", |c| c.word()), Ok("x"));
        assert!(lines.end().is_ok());
        assert!(to_json("txt", "").is_none());
    }

    #[test]
    #[ignore]
    fn text_formats_real_data() {
        let mut short = Vec::new();
        for ext in EXTENSIONS {
            let files = crate::parsers::real_files(ext, 200);
            if files.is_empty() {
                println!("{ext}: none in the index");
                continue;
            }
            let mut failures = Vec::new();
            for (path, bytes) in &files {
                let text = crate::parsers::utils::decode_text_lossy(bytes);
                if let Some(Err(e)) = to_json(ext, &text) {
                    failures.push(format!("{path}: {e}"));
                }
            }
            let parsed = files.len() - failures.len();
            println!("{ext}: sampled {}, parsed {parsed}", files.len());
            for f in failures.iter().take(3) {
                println!("    {f}");
            }
            if parsed * 100 < files.len() * 95 {
                short.push(*ext);
            }
        }
        assert!(short.is_empty(), "below 95%: {short:?}");
    }
}
