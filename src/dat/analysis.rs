//! Column-type guessing for DAT tables that have no schema entry. Scans the fixed
//! section byte-by-byte the way poe-dat-viewer's analysis does, then greedily
//! assigns the widest type that every row satisfies. Also aligns a stale schema
//! onto a guessed layout and ranks likely foreign-key targets by row count.

use crate::dat::reader::{get_column_size, DatReader, DatValue};
use crate::dat::schema::{Column, Table, TableReference};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Guess {
    Bool,
    U8,
    I16,
    I32,
    F32,
    String,
    Row,
    ForeignRow,
    Array(Box<Guess>),
}

impl Guess {
    fn type_name(&self) -> String {
        match self {
            Guess::Bool => "bool".into(),
            Guess::U8 => "u8".into(),
            Guess::I16 => "i16".into(),
            Guess::I32 => "i32".into(),
            Guess::F32 => "f32".into(),
            Guess::String => "string".into(),
            Guess::Row => "row".into(),
            Guess::ForeignRow => "foreignrow".into(),
            Guess::Array(e) => format!("{}[]", e.type_name()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuessedColumn {
    pub offset: usize,
    pub size: usize,
    pub guess: Guess,
    /// Every row is all zero bytes here, so the guess is a default rather than evidence.
    pub blank: bool,
    /// Small non-negative integers with few distinct values — the shape of an `enumrow`.
    pub enum_like: bool,
}

const NULL64: u64 = 0xfefe_fefe_fefe_fefe;
const MAX_STRING_UNITS: usize = 4096;
const MAX_ARRAY_LEN: u64 = 1 << 20;

struct Scan<'a> {
    fixed: &'a [u8],
    var: &'a [u8],
    rows: usize,
    row_len: usize,
    ptr: usize,
}

impl<'a> Scan<'a> {
    fn row(&self, i: usize) -> &'a [u8] {
        &self.fixed[i * self.row_len..(i + 1) * self.row_len]
    }

    fn u32_at(&self, i: usize, o: usize) -> u32 {
        let r = self.row(i);
        u32::from_le_bytes(r[o..o + 4].try_into().unwrap())
    }

    fn u64_at(&self, i: usize, o: usize) -> u64 {
        let r = self.row(i);
        u64::from_le_bytes(r[o..o + 8].try_into().unwrap())
    }

    /// Pointer-sized value (u64 in datc64, u32 in legacy dat).
    fn ptr_at(&self, i: usize, o: usize) -> u64 {
        if self.ptr == 8 { self.u64_at(i, o) } else { self.u32_at(i, o) as u64 }
    }

    fn null_ptr(&self) -> u64 {
        if self.ptr == 8 { NULL64 } else { 0xfefe_fefe }
    }

    fn max_byte(&self, o: usize) -> u8 {
        (0..self.rows).map(|i| self.row(i)[o]).max().unwrap_or(0)
    }

    fn is_blank(&self, o: usize, size: usize) -> bool {
        (0..self.rows).all(|i| self.row(i)[o..o + size].iter().all(|&b| b == 0))
    }

    /// Every row carries the 0xFE null-reference sentinel at this byte.
    fn fe_byte(&self, o: usize) -> bool {
        o < self.row_len && (0..self.rows).all(|i| self.row(i)[o] == 0xFE)
    }

    /// A sentinel byte within the next three means no 4-byte number can start here.
    fn sentinel_follows(&self, o: usize) -> bool {
        (1..4).any(|k| self.fe_byte(o + k))
    }

    fn enum_like(&self, o: usize) -> bool {
        if self.rows < 8 || o + 4 > self.row_len {
            return false;
        }
        let mut seen = [false; 256];
        let mut distinct = 0usize;
        let mut max = 0usize;
        for i in 0..self.rows {
            let v = self.u32_at(i, o) as usize;
            if v >= seen.len() {
                return false;
            }
            if !seen[v] {
                seen[v] = true;
                distinct += 1;
            }
            max = max.max(v);
        }
        (2..=32).contains(&distinct) && max < distinct * 2
    }

    /// `Some(non_empty)` when `p` is a plausible string pointer, `None` otherwise.
    fn string_at(&self, p: u64) -> Option<bool> {
        if p == 0 {
            return Some(false);
        }
        if p < 8 {
            return None;
        }
        let off = (p - 8) as usize;
        if off + 2 > self.var.len() {
            return None;
        }
        let mut units = 0usize;
        let mut pos = off;
        loop {
            if pos + 2 > self.var.len() || units > MAX_STRING_UNITS {
                return None;
            }
            let u = u16::from_le_bytes([self.var[pos], self.var[pos + 1]]);
            if u == 0 {
                break;
            }
            let printable = u >= 0x20 || u == 0x09 || u == 0x0a || u == 0x0d;
            if !printable || (0xd800..0xe000).contains(&u) && units == 0 {
                return None;
            }
            units += 1;
            pos += 2;
        }
        Some(units > 0)
    }

    fn is_string_col(&self, o: usize) -> bool {
        if o + self.ptr > self.row_len {
            return false;
        }
        let mut non_empty = 0;
        for i in 0..self.rows {
            match self.string_at(self.ptr_at(i, o)) {
                Some(true) => non_empty += 1,
                Some(false) => {}
                None => return false,
            }
        }
        non_empty > 0
    }

    /// Self-references are rare and a small int followed by zeros looks identical,
    /// so only claim `row` when the null sentinel actually appears (even in every row).
    fn is_row_col(&self, o: usize) -> bool {
        if o + self.ptr > self.row_len || self.rows < 2 {
            return false;
        }
        let mut nulls = 0;
        for i in 0..self.rows {
            let v = self.ptr_at(i, o);
            if v == self.null_ptr() {
                nulls += 1;
                continue;
            }
            if v as usize >= self.rows {
                return false;
            }
        }
        nulls > 0
    }

    /// Accepts columns that are null in every row: PoE 2 tables carry many
    /// PoE 1-era references that are never set.
    fn is_foreign_col(&self, o: usize) -> bool {
        let size = self.ptr * 2;
        if o + size > self.row_len || self.rows == 0 {
            return false;
        }
        for i in 0..self.rows {
            let lo = self.ptr_at(i, o);
            let hi = self.ptr_at(i, o + self.ptr);
            if lo == self.null_ptr() && hi == self.null_ptr() {
                continue;
            }
            if hi != 0 || lo >= 0x10_0000 {
                return false;
            }
        }
        true
    }

    /// Array columns are `(count, pointer)`; returns the element type when every row fits.
    fn array_col(&self, o: usize) -> Option<Guess> {
        let size = self.ptr * 2;
        if o + size > self.row_len {
            return None;
        }
        let mut with_items = Vec::new();
        for i in 0..self.rows {
            let count = self.ptr_at(i, o);
            let p = self.ptr_at(i, o + self.ptr);
            if count == 0 {
                continue;
            }
            if count > MAX_ARRAY_LEN || p < 8 {
                return None;
            }
            with_items.push((count as usize, (p - 8) as usize));
        }
        if with_items.is_empty() {
            return None;
        }
        let fits = |es: usize| with_items.iter().all(|(c, off)| off + c * es <= self.var.len());
        if !fits(1) {
            return None;
        }
        let elem_is_string = fits(self.ptr)
            && with_items.iter().all(|(c, off)| {
                (0..*c).all(|k| {
                    let e = off + k * self.ptr;
                    let p = if self.ptr == 8 {
                        u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap())
                    } else {
                        u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64
                    };
                    self.string_at(p).is_some()
                })
            })
            && with_items.iter().any(|(c, off)| {
                (0..*c).any(|k| {
                    let e = off + k * self.ptr;
                    let p = if self.ptr == 8 {
                        u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap())
                    } else {
                        u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64
                    };
                    self.string_at(p) == Some(true)
                })
            });
        if elem_is_string {
            return Some(Guess::Array(Box::new(Guess::String)));
        }
        let elem_is_foreign = fits(self.ptr * 2)
            && with_items.iter().all(|(c, off)| {
                (0..*c).all(|k| {
                    let e = off + k * self.ptr * 2;
                    let (lo, hi) = if self.ptr == 8 {
                        (
                            u64::from_le_bytes(self.var[e..e + 8].try_into().unwrap()),
                            u64::from_le_bytes(self.var[e + 8..e + 16].try_into().unwrap()),
                        )
                    } else {
                        (
                            u32::from_le_bytes(self.var[e..e + 4].try_into().unwrap()) as u64,
                            u32::from_le_bytes(self.var[e + 4..e + 8].try_into().unwrap()) as u64,
                        )
                    };
                    (lo == self.null_ptr() && hi == self.null_ptr()) || (hi == 0 && lo < 0x10_0000)
                })
            });
        if elem_is_foreign {
            return Some(Guess::Array(Box::new(Guess::ForeignRow)));
        }
        if fits(4) {
            return Some(Guess::Array(Box::new(Guess::I32)));
        }
        Some(Guess::Array(Box::new(Guess::U8)))
    }

    fn is_bool_col(&self, o: usize) -> bool {
        if self.max_byte(o) > 1 {
            return false;
        }
        // An all-false bool is indistinguishable from a zero byte unless a null
        // sentinel starts right after it, which rules out a wider number.
        let any_set = (0..self.rows).any(|i| self.row(i)[o] == 1);
        if !any_set && !self.sentinel_follows(o) {
            return false;
        }
        // Three trailing zero bytes look more like a small i32 than a bool.
        if o + 4 <= self.row_len && (1..4).all(|k| self.max_byte(o + k) == 0) {
            return false;
        }
        true
    }

    fn numeric_4(&self, o: usize) -> Guess {
        let mut nonzero = 0usize;
        let mut float_like = 0usize;
        let mut int_like = 0usize;
        for i in 0..self.rows {
            let v = self.u32_at(i, o);
            if v == 0 {
                continue;
            }
            nonzero += 1;
            let f = f32::from_bits(v);
            let exp = (v >> 23) & 0xff;
            if f.is_finite() && exp != 0 && exp != 0xff && (1e-7..1e10).contains(&f.abs()) {
                float_like += 1;
            }
            if (v as i32).unsigned_abs() < 10_000_000 {
                int_like += 1;
            }
        }
        if nonzero > 0 && float_like * 100 >= nonzero * 95 && int_like * 2 < nonzero {
            Guess::F32
        } else {
            Guess::I32
        }
    }
}

pub fn analyze(reader: &DatReader) -> Vec<GuessedColumn> {
    let data = reader.get_data();
    let rows = reader.row_count as usize;
    let Some(row_len) = reader.row_length else { return Vec::new() };
    let fixed_end = 4 + rows * row_len;
    if row_len == 0 || fixed_end > data.len() {
        return Vec::new();
    }
    let var_start = (reader.data_section_offset as usize).min(data.len());
    let scan = Scan {
        fixed: &data[4..fixed_end],
        var: &data[var_start..],
        rows,
        row_len,
        ptr: if reader.is_64bit { 8 } else { 4 },
    };

    let mut cols = Vec::new();
    let mut o = 0usize;
    let mut push = |offset: usize, size: usize, guess: Guess| {
        let blank = scan.is_blank(offset, size);
        let enum_like = guess == Guess::I32 && !blank && scan.enum_like(offset);
        cols.push(GuessedColumn { offset, size, guess, blank, enum_like });
    };
    while o < row_len {
        let p2 = scan.ptr * 2;
        if o + p2 <= row_len {
            if let Some(g) = scan.array_col(o) {
                push(o, p2, g);
                o += p2;
                continue;
            }
        }
        if o + scan.ptr <= row_len && scan.is_string_col(o) {
            push(o, scan.ptr, Guess::String);
            o += scan.ptr;
            continue;
        }
        if o + p2 <= row_len && scan.is_foreign_col(o) {
            push(o, p2, Guess::ForeignRow);
            o += p2;
            continue;
        }
        if o + scan.ptr <= row_len && scan.is_row_col(o) {
            push(o, scan.ptr, Guess::Row);
            o += scan.ptr;
            continue;
        }
        if scan.is_bool_col(o) {
            push(o, 1, Guess::Bool);
            o += 1;
            continue;
        }
        // Inside or right before a null sentinel: step one byte so the next
        // reference is found on its real boundary instead of swallowed by an i32.
        if scan.fe_byte(o) || scan.sentinel_follows(o) {
            push(o, 1, Guess::U8);
            o += 1;
            continue;
        }
        if o + 4 <= row_len {
            push(o, 4, scan.numeric_4(o));
            o += 4;
        } else if o + 2 <= row_len {
            push(o, 2, Guess::I16);
            o += 2;
        } else {
            push(o, 1, Guess::U8);
            o += 1;
        }
    }
    cols
}

/// Schema column for one guess; `note` says where the guess came from.
pub fn synth_column(c: &GuessedColumn, note: &str) -> Column {
    let (ty, array) = match &c.guess {
        Guess::Array(e) => (e.type_name(), true),
        g => (g.type_name(), false),
    };
    let mut name = format!("@{} {}", c.offset, c.guess.type_name());
    let mut description = format!("{}: {} bytes at offset {}", note, c.size, c.offset);
    if c.enum_like {
        name.push_str(" enum?");
        description.push_str(" · small contiguous values, looks like an enumrow");
    }
    if c.blank {
        description.push_str(" · all rows are zero, type is a default");
    }
    Column {
        name: Some(name),
        description: Some(description),
        array,
        r#type: ty,
        unique: false,
        localized: false,
        references: None,
        interval: false,
        file: None,
        files: None,
    }
}

/// Synthetic schema table the regular reader/viewer can consume.
pub fn to_table(cols: &[GuessedColumn], name: &str) -> Table {
    let columns = cols.iter().map(|c| synth_column(c, "Guessed from data (table not in schema)")).collect();
    Table { name: name.to_string(), columns, tags: None, valid_for: None, custom: false }
}

// ── Schema drift alignment ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct AlignReport {
    pub matched: usize,
    /// Indices in the aligned table with no schema counterpart.
    pub added: Vec<usize>,
    /// Schema columns that found no place in the file's layout.
    pub dropped: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bool,
    Byte,
    Short,
    Int,
    Float,
    Long,
    Str,
    Row,
    Foreign,
    Array,
    Other,
}

fn schema_kind(col: &Column) -> Kind {
    if col.array {
        return Kind::Array;
    }
    match col.r#type.as_str() {
        "bool" => Kind::Bool,
        "byte" | "u8" => Kind::Byte,
        "short" | "i16" | "u16" | "ushort" => Kind::Short,
        "int" | "i32" | "u32" | "uint" | "enumrow" => Kind::Int,
        "float" | "f32" => Kind::Float,
        "long" | "i64" | "u64" | "ulong" => Kind::Long,
        "string" | "ref|string" => Kind::Str,
        "foreignrow" | "foreign_row" => Kind::Foreign,
        t if t == "row" || t.starts_with("ref|") => Kind::Row,
        _ => Kind::Other,
    }
}

/// Numeric guesses carry little evidence; a blank or numeric span could be almost anything.
fn is_weak(g: &Guess) -> bool {
    matches!(g, Guess::Bool | Guess::U8 | Guess::I16 | Guess::I32 | Guess::F32)
}

fn scalar_fits(kind: Kind, g: &Guess) -> bool {
    match kind {
        Kind::Bool | Kind::Byte => matches!(g, Guess::Bool | Guess::U8),
        Kind::Short => matches!(g, Guess::I16),
        Kind::Int => matches!(g, Guess::I32),
        Kind::Float => matches!(g, Guess::F32 | Guess::I32),
        Kind::Str => matches!(g, Guess::String),
        Kind::Row => matches!(g, Guess::Row),
        Kind::Foreign => matches!(g, Guess::ForeignRow),
        Kind::Long | Kind::Other => is_weak(g),
        Kind::Array => false,
    }
}

fn array_fits(col: &Column, g: &Guess) -> bool {
    let Guess::Array(e) = g else { return false };
    match col.r#type.as_str() {
        "string" | "ref|string" => **e == Guess::String,
        "foreignrow" | "foreign_row" => **e == Guess::ForeignRow,
        _ => !matches!(**e, Guess::String | Guess::ForeignRow),
    }
}

/// Score for placing `col` over `run` (consecutive guesses); `None` when the data rules it out.
fn match_score(col: &Column, run: &[&GuessedColumn], is_64bit: bool) -> Option<u32> {
    let size: usize = run.iter().map(|g| g.size).sum();
    if size != get_column_size(col, is_64bit) {
        return None;
    }
    let kind = schema_kind(col);
    let exact = match kind {
        Kind::Array => run.len() == 1 && array_fits(col, &run[0].guess),
        Kind::Long | Kind::Other => run.iter().all(|g| is_weak(&g.guess)),
        k if col.interval => run.len() == 2 && run.iter().all(|g| scalar_fits(k, &g.guess)),
        k => run.len() == 1 && scalar_fits(k, &run[0].guess),
    };
    if exact {
        let strong = matches!(kind, Kind::Str | Kind::Row | Kind::Foreign | Kind::Array);
        return Some(if strong { 3 } else { 2 });
    }
    if run.iter().all(|g| g.blank) {
        return Some(1);
    }
    None
}

const MAX_RUN: usize = 8;

/// Carries names, types and references from a stale schema onto the layout guessed
/// from the file. Columns are matched by size and type evidence with a longest-common-
/// subsequence pass; on ties the earliest positions keep their schema names, so an
/// inserted column shows up as a new `@offset` column rather than shifting every name.
pub fn align_schema(schema: &Table, guessed: &[GuessedColumn], is_64bit: bool) -> (Table, AlignReport) {
    // Reversed inputs make the forward DP's "prefer a match at the end" tie-break
    // anchor the *start* of the row in original order.
    let cols: Vec<&Column> = schema.columns.iter().rev().collect();
    let gs: Vec<&GuessedColumn> = guessed.iter().rev().collect();
    let (n, m) = (cols.len(), gs.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    let mut how = vec![vec![0u8; m + 1]; n + 1];
    for i in 0..=n {
        for j in 0..=m {
            if i == 0 && j == 0 {
                continue;
            }
            let mut best = 0;
            let mut step = 0u8;
            if i > 0 {
                best = dp[i - 1][j];
                step = 1;
            }
            if j > 0 && (step == 0 || dp[i][j - 1] > best) {
                best = dp[i][j - 1];
                step = 2;
            }
            if i > 0 {
                for k in 1..=MAX_RUN.min(j) {
                    if let Some(s) = match_score(cols[i - 1], &gs[j - k..j], is_64bit) {
                        let v = dp[i - 1][j - k] + s;
                        if v >= best {
                            best = v;
                            step = 2 + k as u8;
                        }
                    }
                }
            }
            dp[i][j] = best;
            how[i][j] = step;
        }
    }

    // (schema index, guessed start, guessed end) in original order.
    let mut matches: Vec<(usize, usize, usize)> = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match how[i][j] {
            1 => i -= 1,
            2 => j -= 1,
            s => {
                let k = (s - 2) as usize;
                matches.push((n - i, m - j, m - (j - k)));
                i -= 1;
                j -= k;
            }
        }
    }
    matches.sort_by_key(|&(_, lo, _)| lo);

    let mut columns = Vec::with_capacity(guessed.len());
    let mut report = AlignReport::default();
    let mut placed = vec![false; schema.columns.len()];
    let mut j = 0;
    let mut mi = 0;
    while j < guessed.len() {
        if mi < matches.len() && matches[mi].1 == j {
            let (si, _, hi) = matches[mi];
            columns.push(schema.columns[si].clone());
            placed[si] = true;
            report.matched += 1;
            j = hi;
            mi += 1;
        } else {
            report.added.push(columns.len());
            columns.push(synth_column(&guessed[j], "New column, not in the schema"));
            j += 1;
        }
    }
    report.dropped = schema
        .columns
        .iter()
        .enumerate()
        .filter(|(i, _)| !placed[*i])
        .map(|(i, c)| c.name.clone().unwrap_or_else(|| format!("_{}", i)))
        .collect();
    let table = Table { name: schema.name.clone(), columns, tags: schema.tags.clone(), valid_for: schema.valid_for, custom: false };
    (table, report)
}

// ── Foreign-key target inference ────────────────────────────────────────────

/// Row count of one DAT file in the index, gathered once per session.
#[derive(Debug, Clone)]
pub struct TableStats {
    pub path: String,
    pub row_count: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FkColumnStats {
    pub max_index: u64,
    pub distinct: usize,
    pub non_null: usize,
}

#[derive(Debug, Clone)]
pub struct FkCandidate {
    pub stem: String,
    pub row_count: u32,
    /// Share of the candidate's rows the column's indices span; 1.0 is a perfect fit.
    pub fit: f32,
    /// The column's name points at this table (`StatsKeys` → `Stats`).
    pub name_match: bool,
}

fn normalized_name(name: &str) -> String {
    let mut n: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    n.make_ascii_lowercase();
    for suffix in ["keys", "key", "ids", "id"] {
        if n.ends_with(suffix) && n.len() > suffix.len() + 2 {
            n.truncate(n.len() - suffix.len());
            break;
        }
    }
    n
}

/// How strongly a column name points at a table: 3 exact (± plural), 2 prefix, 1 substring.
fn name_affinity(column: Option<&str>, stem: &str) -> u8 {
    let Some(column) = column else { return 0 };
    let n = normalized_name(column);
    let s = stem.to_ascii_lowercase();
    if n.len() < 3 {
        return 0;
    }
    if n == s || format!("{}s", n) == s || n == format!("{}s", s) {
        return 3;
    }
    let singular = s.strip_suffix('s').unwrap_or(&s);
    if n.len() >= 4 && singular.len() >= 4 && (s.starts_with(&n) || n.starts_with(singular)) {
        return 2;
    }
    if n.len() >= 5 && s.len() >= 5 && (s.contains(&n) || n.contains(&s)) {
        return 1;
    }
    0
}

pub fn is_foreign(col: &Column) -> bool {
    matches!(col.r#type.as_str(), "foreignrow" | "foreign_row")
}

pub fn has_unresolved_foreign(table: &Table) -> bool {
    table.columns.iter().any(|c| is_foreign(c) && c.references.is_none())
}

const MAX_LIST_SCAN: usize = 4096;

/// Index statistics for every `foreignrow` column (None for the others).
pub fn foreign_key_stats(reader: &DatReader, table: &Table) -> Vec<Option<FkColumnStats>> {
    let mut out = vec![None; table.columns.len()];
    let fk_cols: Vec<usize> = table.columns.iter().enumerate().filter(|(_, c)| is_foreign(c)).map(|(i, _)| i).collect();
    if fk_cols.is_empty() {
        return out;
    }
    let mut seen: Vec<HashSet<u64>> = vec![HashSet::new(); table.columns.len()];
    let mut stats = vec![FkColumnStats::default(); table.columns.len()];
    let note = |ci: usize, k: usize, seen: &mut Vec<HashSet<u64>>, stats: &mut Vec<FkColumnStats>| {
        if k == usize::MAX {
            return;
        }
        let s = &mut stats[ci];
        s.non_null += 1;
        s.max_index = s.max_index.max(k as u64);
        if seen[ci].insert(k as u64) {
            s.distinct += 1;
        }
    };
    for r in 0..reader.row_count {
        let Ok(vals) = reader.read_row(r, table) else { continue };
        for &ci in &fk_cols {
            match vals.get(ci) {
                Some(DatValue::ForeignRow(k)) => note(ci, *k, &mut seen, &mut stats),
                Some(DatValue::List(count, off)) if *count > 0 => {
                    if let Ok(items) = reader.read_list_values(*off, (*count).min(MAX_LIST_SCAN), &table.columns[ci]) {
                        for item in items {
                            if let DatValue::ForeignRow(k) = item {
                                note(ci, k, &mut seen, &mut stats);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for ci in fk_cols {
        out[ci] = Some(stats[ci]);
    }
    out
}

fn path_stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).to_string()
}

/// Tables that could be the target of a column with these statistics: tables the column's
/// name points at come first, then the tightest fit by row count. Only files in the same
/// directory and with the same extension as the current file count, which keeps the
/// per-language copies under `data/balance/<lang>/` out of the running.
pub fn rank_targets(
    stats: &FkColumnStats,
    tables: &[TableStats],
    base_dir: &str,
    ext: &str,
    exclude_path: &str,
    limit: usize,
    column_name: Option<&str>,
) -> Vec<FkCandidate> {
    if stats.non_null == 0 {
        return Vec::new();
    }
    let prefix = if base_dir.is_empty() { String::new() } else { format!("{}/", base_dir.to_ascii_lowercase()) };
    let suffix = format!(".{}", ext.to_ascii_lowercase());
    let mut scored: Vec<(u8, FkCandidate)> = tables
        .iter()
        .filter(|t| {
            let p = t.path.to_ascii_lowercase();
            p.starts_with(&prefix)
                && p.ends_with(&suffix)
                && !p[prefix.len()..].contains('/')
                && !p.eq_ignore_ascii_case(exclude_path)
                && (t.row_count as u64) > stats.max_index
        })
        .map(|t| {
            let stem = path_stem(&t.path);
            let affinity = name_affinity(column_name, &stem);
            let cand = FkCandidate {
                stem,
                row_count: t.row_count,
                fit: (stats.max_index + 1) as f32 / t.row_count as f32,
                name_match: affinity > 0,
            };
            (affinity, cand)
        })
        .collect();
    scored.sort_by(|(aa, a), (ab, b)| ab.cmp(aa).then(a.row_count.cmp(&b.row_count)).then_with(|| a.stem.cmp(&b.stem)));
    scored.truncate(limit);
    scored.into_iter().map(|(_, c)| c).collect()
}

pub fn reference_to(table: &str) -> TableReference {
    TableReference { table: table.to_string(), column: None }
}


/// Evidence that a schema no longer describes a file. Every variable-length
/// column stores an offset into the data section, so a read taken at the wrong
/// offset almost always yields a pointer that cannot be real — which is the
/// difference between a table that merely grew a column at the end (harmless,
/// the named columns still land where the schema says) and one whose columns
/// shifted (every value past the shift is fiction).
#[derive(Debug, Default, Clone)]
pub struct FitReport {
    /// Bytes the schema accounts for, and what the file's rows actually are.
    pub schema_len: usize,
    pub row_len: usize,
    pub sampled: usize,
    /// Named columns that read impossible values, worst first, with the share
    /// of sampled rows that came out impossible.
    pub impossible: Vec<(String, f32)>,
}

impl FitReport {
    /// The share of sampled rows misread by the worst-hit column.
    pub fn worst(&self) -> f32 {
        self.impossible.first().map(|(_, share)| *share).unwrap_or(0.0)
    }

    /// Whether the schema is too far off this file to read from. A single
    /// column misreading a few rows is normal noise in hand-written schemas;
    /// a shifted layout wrecks most rows of every column past the shift.
    pub fn is_broken(&self) -> bool {
        self.worst() >= 0.25
    }

    /// One line naming what went wrong, for an error a user has to act on.
    pub fn summary(&self) -> String {
        let cols: Vec<String> = self
            .impossible
            .iter()
            .take(4)
            .map(|(name, share)| format!("{} ({:.0}% of rows)", name, share * 100.0))
            .collect();
        format!(
            "the schema describes a {}-byte row but the file's rows are {} bytes, and {} read impossible values: {}",
            self.schema_len,
            self.row_len,
            self.impossible.len(),
            cols.join(", ")
        )
    }
}

/// Reads `sample` rows through `table` and counts the values that cannot be
/// real: list and string offsets outside the file, absurd list lengths, and
/// foreign keys past any plausible row count. Cheap enough to run before every
/// table read.
pub fn check_fit(reader: &DatReader, table: &Table, sample: usize) -> FitReport {
    let data_len = reader.get_data().len();
    let var_start = reader.data_section_offset as usize;
    let mut report = FitReport {
        schema_len: table.columns.iter().map(|c| get_column_size(c, reader.is_64bit)).sum(),
        row_len: reader.row_length.unwrap_or(0),
        ..Default::default()
    };
    if reader.row_count == 0 {
        return report;
    }

    let step = ((reader.row_count as usize) / sample.max(1)).max(1);
    let indices: Vec<u32> = (0..reader.row_count).step_by(step).take(sample).collect();
    report.sampled = indices.len();

    let mut bad = vec![0usize; table.columns.len()];
    for &i in &indices {
        let Ok(values) = reader.read_row(i, table) else { continue };
        for (c, value) in values.iter().enumerate() {
            let Some(col) = table.columns.get(c) else { continue };
            if !value_is_possible(value, col, reader, var_start, data_len) {
                bad[c] += 1;
            }
        }
    }

    let sampled = report.sampled.max(1) as f32;
    for (c, count) in bad.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let name = table.columns[c]
            .name
            .clone()
            .unwrap_or_else(|| format!("column {}", c));
        report.impossible.push((name, *count as f32 / sampled));
    }
    report.impossible.sort_by(|a, b| b.1.total_cmp(&a.1));
    report
}

/// No table in the game has this many rows, so a key at or past it is a misread
/// rather than a key into something large.
const MAX_PLAUSIBLE_ROWS: usize = 5_000_000;

pub fn value_is_possible(
    value: &DatValue,
    col: &Column,
    reader: &DatReader,
    var_start: usize,
    data_len: usize,
) -> bool {
    match value {
        DatValue::List(count, offset) => {
            if *count == 0 {
                return true;
            }
            if *count > 100_000 {
                return false;
            }
            let elem = Column { array: false, interval: false, ..col.clone() };
            let size = get_column_size(&elem, reader.is_64bit);
            let start = var_start + (*offset as usize).saturating_sub(8);
            match size.checked_mul(*count) {
                Some(span) => start.saturating_add(span) <= data_len,
                None => false,
            }
        }
        DatValue::ForeignRow(index) => *index == usize::MAX || *index < MAX_PLAUSIBLE_ROWS,
        DatValue::Interval(a, b) => {
            let elem = Column { interval: false, ..col.clone() };
            value_is_possible(a, &elem, reader, var_start, data_len)
                && value_is_possible(b, &elem, reader, var_start, data_len)
        }
        DatValue::Unknown => false,
        _ => true,
    }
}

/// A schema claim the data contradicts. Every finding here is decidable from
/// the file alone: a key must land inside the table it names, a bool holds 0 or
/// 1, an enum index must exist in the enumeration it points at.
#[derive(Debug, Clone)]
pub struct LintFinding {
    pub table: String,
    pub column: String,
    pub kind: &'static str,
    pub detail: String,
    /// Rows checked, and how many broke the rule.
    pub sampled: usize,
    pub violations: usize,
}

/// Checks one table's declared references, bools and enum indices against the
/// values the file actually holds. `row_counts` maps a lower-case table name to
/// its row count, read from the file headers.
pub fn lint_table(
    reader: &DatReader,
    table: &Table,
    schema: &crate::dat::schema::Schema,
    row_counts: &std::collections::HashMap<String, u32>,
    is_poe2: bool,
    sample: usize,
) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    if reader.row_count == 0 {
        return findings;
    }
    let step = ((reader.row_count as usize) / sample.max(1)).max(1);
    let rows: Vec<u32> = (0..reader.row_count).step_by(step).take(sample).collect();

    for (index, col) in table.columns.iter().enumerate() {
        let name = col.name.clone().unwrap_or_else(|| format!("column {}", index));
        let mut checked = 0usize;
        let mut bad = 0usize;
        let mut worst = String::new();

        // The row count of whatever this column claims to point at.
        let limit = col.references.as_ref().and_then(|r| {
            let target = r.table.to_ascii_lowercase();
            row_counts.get(&target).copied().map(|n| (r.table.clone(), n))
        });
        let enumeration = (col.r#type == "enumrow")
            .then(|| col.references.as_ref().map(|r| r.table.clone()))
            .flatten()
            .and_then(|n| schema.find_enumeration(&n, is_poe2).map(|e| (n, e.indexing as u64, e.enumerators.len())));

        for &row in &rows {
            let Ok(values) = reader.read_row(row, table) else { continue };
            if std::env::var("LINT_DEBUG").as_deref() == Ok(table.name.as_str()) && row == 0 {
                println!("  [{}] {} : {} = {:?}", index, name, col.r#type, values.get(index));
            }
            let Some(value) = values.get(index) else { continue };
            let mut keys: Vec<u64> = Vec::new();
            let mut bools: Vec<bool> = Vec::new();
            collect(value, reader, col, &mut keys, &mut bools);

            if let Some((target, count)) = &limit {
                for key in &keys {
                    checked += 1;
                    if *key >= *count as u64 {
                        bad += 1;
                        if worst.is_empty() {
                            worst = format!("row {} holds key {} but {} has {} rows", row, key, target, count);
                        }
                    }
                }
            } else if let Some((enum_name, first, size)) = &enumeration {
                // Enumerations declare where their numbering starts, so the
                // valid span is first..first + entries, not 0..entries.
                for key in &keys {
                    checked += 1;
                    if *size > 0 && (*key < *first || *key >= *first + *size as u64) {
                        bad += 1;
                        if worst.is_empty() {
                            worst = format!(
                                "row {} holds {} but enum {} covers {}..{}",
                                row, key, enum_name, first, *first + *size as u64 - 1
                            );
                        }
                    }
                }
            }
            let _ = bools;
        }

        if bad > 0 {
            let kind = if limit.is_some() { "key outside the table it references" } else { "index outside its enumeration" };
            findings.push(LintFinding {
                table: table.name.clone(),
                column: name,
                kind,
                detail: worst,
                sampled: checked,
                violations: bad,
            });
        }
    }
    findings
}

/// Flattens a value into the keys and bools it contains, following arrays.
fn collect(value: &DatValue, reader: &DatReader, col: &Column, keys: &mut Vec<u64>, bools: &mut Vec<bool>) {
    match value {
        DatValue::ForeignRow(k) if *k != usize::MAX => keys.push(*k as u64),
        DatValue::Int(i) if col.r#type == "enumrow" && *i >= 0 => keys.push(*i as u64),
        DatValue::Bool(b) => bools.push(*b),
        DatValue::List(count, offset) => {
            if *count == 0 || *count > 10_000 {
                return;
            }
            let elem = Column { array: false, interval: false, ..col.clone() };
            if let Ok(items) = reader.read_list_values(*offset, *count, col) {
                for item in &items {
                    collect(item, reader, &elem, keys, bools);
                }
            }
        }
        DatValue::Interval(a, b) => {
            let elem = Column { interval: false, ..col.clone() };
            collect(a, reader, &elem, keys, bools);
            collect(b, reader, &elem, keys, bools);
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a datc64 blob: rows of (i32, string ptr, bool, pad) with a variable section.
    fn synthetic() -> Vec<u8> {
        let strings = ["alpha", "beta", ""];
        let mut var: Vec<u8> = vec![0xBB; 8];
        let mut ptrs = Vec::new();
        for s in strings {
            if s.is_empty() {
                ptrs.push(0u64);
                continue;
            }
            ptrs.push((var.len()) as u64);
            for u in s.encode_utf16() {
                var.extend_from_slice(&u.to_le_bytes());
            }
            var.extend_from_slice(&[0, 0, 0, 0]);
        }
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        for (i, p) in ptrs.iter().enumerate() {
            data.extend_from_slice(&((i as i32 + 1) * 1000).to_le_bytes());
            data.extend_from_slice(&p.to_le_bytes());
            data.push((i % 2) as u8);
            data.extend_from_slice(&[7, 0, 0]);
        }
        data.extend_from_slice(&var);
        data
    }

    #[test]
    fn guesses_int_string_bool() {
        let reader = DatReader::new(synthetic(), "synthetic.datc64").unwrap();
        let cols = analyze(&reader);
        let kinds: Vec<_> = cols.iter().map(|c| c.guess.clone()).collect();
        assert_eq!(kinds[0], Guess::I32);
        assert_eq!(kinds[1], Guess::String);
        assert_eq!(kinds[2], Guess::Bool);
        let table = to_table(&cols, "synthetic");
        let row = reader.read_row(1, &table).unwrap();
        assert!(matches!(row[1], crate::dat::reader::DatValue::String(ref s) if s == "beta"));
    }

    fn col(name: &str, ty: &str) -> Column {
        Column {
            name: Some(name.into()),
            description: None,
            array: false,
            r#type: ty.into(),
            unique: false,
            localized: false,
            references: None,
            interval: false,
            file: None,
            files: None,
        }
    }

    fn g(offset: usize, size: usize, guess: Guess) -> GuessedColumn {
        GuessedColumn { offset, size, guess, blank: false, enum_like: false }
    }

    fn table(cols: Vec<Column>) -> Table {
        Table { name: "T".into(), columns: cols, tags: None, valid_for: None, custom: false }
    }

    fn names(t: &Table) -> Vec<String> {
        t.columns.iter().map(|c| c.name.clone().unwrap_or_default()).collect()
    }

    #[test]
    fn align_keeps_names_when_column_inserted() {
        let schema = table(vec![col("A", "i32"), col("B", "string"), col("C", "bool")]);
        let guessed = vec![g(0, 4, Guess::I32), g(4, 4, Guess::I32), g(8, 8, Guess::String), g(16, 1, Guess::Bool)];
        let (t, report) = align_schema(&schema, &guessed, true);
        assert_eq!(names(&t), vec!["A", "@4 i32", "B", "C"]);
        assert_eq!(report.matched, 3);
        assert_eq!(report.added, vec![1]);
        assert!(report.dropped.is_empty());
        assert_eq!(t.row_width(true), 17);
    }

    #[test]
    fn align_prefers_earliest_position_on_ties() {
        let schema = table(vec![col("A", "i32"), col("B", "i32")]);
        let guessed = vec![g(0, 4, Guess::I32), g(4, 4, Guess::I32), g(8, 4, Guess::I32)];
        let (t, report) = align_schema(&schema, &guessed, true);
        assert_eq!(names(&t), vec!["A", "B", "@8 i32"]);
        assert_eq!(report.added, vec![2]);
    }

    #[test]
    fn align_drops_removed_columns_and_spans_intervals() {
        let mut range = col("Range", "i32");
        range.interval = true;
        let schema = table(vec![col("Id", "string"), col("Gone", "foreignrow"), range, col("Flag", "bool")]);
        let guessed = vec![g(0, 8, Guess::String), g(8, 4, Guess::I32), g(12, 4, Guess::I32), g(16, 1, Guess::Bool)];
        let (t, report) = align_schema(&schema, &guessed, true);
        assert_eq!(names(&t), vec!["Id", "Range", "Flag"]);
        assert_eq!(report.dropped, vec!["Gone"]);
        assert!(t.columns[1].interval);
    }

    #[test]
    fn align_rejects_type_conflicts_unless_blank() {
        let schema = table(vec![col("Id", "string")]);
        let (t, _) = align_schema(&schema, &[g(0, 8, Guess::Row), g(8, 1, Guess::Bool)], true);
        assert_eq!(names(&t)[0], "@0 row");
        let mut blank = g(0, 8, Guess::I32);
        blank.size = 8;
        blank.blank = true;
        let (t, _) = align_schema(&schema, &[blank], true);
        assert_eq!(names(&t), vec!["Id"]);
    }

    #[test]
    fn ranks_tightest_fitting_table_first() {
        let stats = FkColumnStats { max_index: 7, distinct: 3, non_null: 3 };
        let tables = vec![
            TableStats { path: "Data/Small.datc64".into(), row_count: 5 },
            TableStats { path: "Data/Right.datc64".into(), row_count: 8 },
            TableStats { path: "Data/Big.datc64".into(), row_count: 100 },
            TableStats { path: "Data/Self.datc64".into(), row_count: 50 },
            TableStats { path: "Other/Elsewhere.datc64".into(), row_count: 9 },
            TableStats { path: "Data/Legacy.dat".into(), row_count: 9 },
            TableStats { path: "Data/French/Right.datc64".into(), row_count: 8 },
        ];
        let ranked = rank_targets(&stats, &tables, "Data", "datc64", "data/self.datc64", 10, None);
        let stems: Vec<_> = ranked.iter().map(|c| c.stem.as_str()).collect();
        assert_eq!(stems, vec!["Right", "Big"]);
        assert!((ranked[0].fit - 1.0).abs() < 1e-6);
        assert!(ranked.iter().all(|c| !c.name_match));
        assert!(rank_targets(&FkColumnStats::default(), &tables, "Data", "datc64", "", 10, None).is_empty());

        let named = rank_targets(&stats, &tables, "Data", "datc64", "data/self.datc64", 10, Some("BigKeys"));
        assert_eq!(named[0].stem, "Big");
        assert!(named[0].name_match);
        assert!(!named[1].name_match);
    }

    #[test]
    fn name_affinity_handles_plurals_and_suffixes() {
        assert_eq!(name_affinity(Some("StatsKeys"), "Stats"), 3);
        assert_eq!(name_affinity(Some("Mod"), "Mods"), 3);
        assert_eq!(name_affinity(Some("BaseItemType"), "BaseItemTypes"), 3);
        assert_eq!(name_affinity(Some("LeagueInfoPanel"), "LeagueInfoPanelVersions"), 2);
        assert_eq!(name_affinity(Some("QuestFlagHeard"), "QuestFlags"), 2);
        assert_eq!(name_affinity(Some("T17SpawnChanceStatsKey"), "Stats"), 1);
        assert_eq!(name_affinity(Some("Mod"), "ModFamily"), 0);
        assert_eq!(name_affinity(Some("@16 foreignrow"), "Mods"), 0);
        assert_eq!(name_affinity(None, "Mods"), 0);
    }

    #[test]
    fn sentinel_spans_are_null_references_not_ints() {
        // Rows: bool(false in every row) · foreignrow(null) · row(null) · i32
        let mut data = 4u32.to_le_bytes().to_vec();
        for i in 0..4u32 {
            data.push(0);
            data.extend_from_slice(&[0xFE; 16]);
            data.extend_from_slice(&[0xFE; 8]);
            data.extend_from_slice(&(i * 10).to_le_bytes());
        }
        data.extend_from_slice(&[0xBB; 8]);
        let reader = DatReader::new(data, "s.datc64").unwrap();
        let kinds: Vec<_> = analyze(&reader).iter().map(|c| c.guess.clone()).collect();
        assert_eq!(kinds, vec![Guess::Bool, Guess::ForeignRow, Guess::Row, Guess::I32]);
    }

    #[test]
    fn enum_hint_needs_small_contiguous_values() {
        let build = |values: &[u32]| {
            let mut data = (values.len() as u32).to_le_bytes().to_vec();
            for v in values {
                data.extend_from_slice(&v.to_le_bytes());
                data.extend_from_slice(&[0u8; 4]);
            }
            data.extend_from_slice(&[0xBB; 8]);
            DatReader::new(data, "e.datc64").unwrap()
        };
        let enumish = build(&[0, 1, 2, 3, 0, 1, 2, 3, 1, 2]);
        assert!(analyze(&enumish)[0].enum_like);
        let sparse = build(&[0, 100, 0, 100, 0, 100, 0, 100, 0, 100]);
        assert!(!analyze(&sparse)[0].enum_like);
        assert!(analyze(&sparse)[1].blank);
    }

    /// Guesses must tile the whole row; prints them next to the schema when one is cached.
    /// Needs a local `examples/data/balance/itemvisualidentity.datc64` (not checked in).
    /// Run with: cargo test guesses_real_example_table -- --ignored --nocapture
    #[test]
    #[ignore]
    fn guesses_real_example_table() {
        let data = std::fs::read("examples/data/balance/itemvisualidentity.datc64").unwrap();
        let reader = DatReader::new(data, "itemvisualidentity.datc64").unwrap();
        let cols = analyze(&reader);
        assert!(!cols.is_empty());
        let covered: usize = cols.iter().map(|c| c.size).sum();
        assert_eq!(covered, reader.row_length.unwrap());
        let table = to_table(&cols, "ItemVisualIdentity");
        assert!(reader.read_row(0, &table).is_ok());
        for c in &cols {
            println!("guess  @{:>3} {:>2}B {}", c.offset, c.size, c.guess.type_name());
        }
        let path = crate::settings::AppSettings::get_app_data_dir().join("schema.min.json");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(schema) = serde_json::from_str::<crate::dat::schema::Schema>(&text) {
                if let Some(t) = schema.find_table("ItemVisualIdentity", true) {
                    let mut off = 0;
                    for col in &t.columns {
                        let size = crate::dat::reader::get_column_size(col, true);
                        println!("schema @{:>3} {:>2}B {}{} {}", off, size, col.r#type, if col.array { "[]" } else { "" }, col.name.as_deref().unwrap_or("_"));
                        off += size;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod fit_real_data_tests {
    use super::*;
    use crate::bundles::index::Index as BundleIndex;

    /// Scores every schema table on the installed patch, so the fit check's
    /// threshold can be set against how the community schema really behaves.
    /// Run with: cargo test --release fit_check_real_data -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fit_check_real_data() {
        let settings = crate::settings::AppSettings::load();
        let ggpk = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = std::sync::Arc::new(crate::ggpk::reader::GgpkReader::open(&ggpk).unwrap());
        let cache = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = BundleIndex::load_from_cache(&cache).expect("run the app once first");
        let schema_text = std::fs::read_to_string(
            crate::settings::AppSettings::get_app_data_dir().join("schema.min.json"),
        )
        .unwrap();
        let schema: crate::dat::schema::Schema = serde_json::from_str(&schema_text).unwrap();

        let mut scored: Vec<(f32, String, FitReport)> = Vec::new();
        for file in index.files.values() {
            let path = file.path.to_ascii_lowercase();
            let Some(rest) = path.strip_prefix("data/balance/") else { continue };
            if rest.contains('/') || !rest.ends_with(".datc64") {
                continue;
            }
            let name = rest.trim_end_matches(".datc64");
            let Some(def) = schema.find_table(name, true) else { continue };
            let Some(bytes) = crate::ui::content_view::extract_bundle_file_sync(file, &index, Some(&reader), None)
            else {
                continue;
            };
            let Ok(dat) = DatReader::new(bytes, &path) else { continue };
            if dat.row_count < 4 {
                continue;
            }
            let report = check_fit(&dat, def, 40);
            scored.push((report.worst(), def.name.clone(), report));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));

        let broken = scored.iter().filter(|(_, _, r)| r.is_broken()).count();
        let grew = scored.iter().filter(|(_, _, r)| r.schema_len != r.row_len).count();
        println!(
            "scored {} tables · row length differs from the schema on {} · fit check calls {} broken",
            scored.len(), grew, broken
        );
        println!("\nworst 25:");
        for (worst, name, r) in scored.iter().take(25) {
            println!(
                "  {:>5.1}%  {:<40} schema {:>4}B file {:>4}B  {}",
                worst * 100.0, name, r.schema_len, r.row_len,
                r.impossible.iter().take(3).map(|(c, s)| format!("{} {:.0}%", c, s * 100.0)).collect::<Vec<_>>().join(", ")
            );
        }
        println!("\nscore distribution:");
        for (lo, hi) in [(0.0, 0.001), (0.001, 0.05), (0.05, 0.25), (0.25, 0.75), (0.75, 1.01)] {
            let n = scored.iter().filter(|(w, _, _)| *w >= lo && *w < hi).count();
            println!("  {:>5.1}%–{:<5.1}% : {}", lo * 100.0, hi * 100.0, n);
        }
    }
}

