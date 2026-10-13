//! Carrying a table's columns across a patch.
//!
//! The community schema is written by hand and lags every patch, so the first
//! days after one land are spent reading tables whose columns have shifted. The
//! previous patch's copy of the same file is the one place those names are
//! still attached to real values: match rows by id, look for where each old
//! column's values reappear in the new row, and the layout comes back without
//! anyone having to name a byte. Names it cannot place are reported rather than
//! guessed — bytes never say what a column is called.

use crate::dat::analysis;
use crate::dat::exe_layout::{self, ClientLayouts, Kind, Pointer, TableLayout};
use crate::dat::reader::{get_column_size, DatReader, DatValue};
use crate::dat::schema::{Column, Table};
use std::collections::HashMap;

/// How many rows to compare. Enough that a column of mostly-repeated values
/// still separates from its neighbours, few enough to stay interactive.
const SAMPLE_ROWS: usize = 160;
/// Rows an offset must survive before it is worth scoring in full.
const PROBE_ROWS: usize = 8;
/// Share of sampled rows that must agree for a column to be considered found.
const MIN_AGREEMENT: f32 = 0.7;
/// How far the best offset must beat the next-best one, so a column whose
/// values repeat all over the row is reported lost instead of placed wrongly.
const MIN_MARGIN: f32 = 0.1;
/// The bar for a shape-only match, which is far weaker evidence than a value.
const MIN_SHAPE_AGREEMENT: f32 = 0.95;

#[derive(Debug, Clone)]
pub struct CarriedColumn {
    pub name: String,
    /// Where the column sat before, and where it sits now.
    pub old_offset: usize,
    pub new_offset: usize,
    pub agreement: f32,
    /// Distinct values behind the match. One value agrees with far too much.
    pub distinct: usize,
    /// Placed by the bytes its neighbours left over, not by matching values.
    pub by_position: bool,
}

#[derive(Debug, Clone)]
pub struct RefitReport {
    /// The recovered layout, tiling the new row exactly, ready to be stored as
    /// a schema override.
    pub table: Table,
    pub carried: Vec<CarriedColumn>,
    /// Columns whose values no longer appear anywhere in the row: either the
    /// patch dropped them, or it changed every value they hold.
    pub lost: Vec<String>,
    pub matched_rows: usize,
    pub old_row_len: usize,
    pub new_row_len: usize,
}

impl RefitReport {
    pub fn summary(&self) -> String {
        format!(
            "{}: row {} → {} bytes, {} of {} columns carried across {} matched rows{}",
            self.table.name,
            self.old_row_len,
            self.new_row_len,
            self.carried.len(),
            self.carried.len() + self.lost.len(),
            self.matched_rows,
            match self.lost.len() {
                0 => String::new(),
                n => format!(", {} lost: {}", n, self.lost.join(", ")),
            }
        )
    }
}

/// Byte offset of every column in a schema, in declaration order.
fn offsets(table: &Table, is_64bit: bool) -> Vec<usize> {
    let mut at = 0;
    table
        .columns
        .iter()
        .map(|c| {
            let start = at;
            at += get_column_size(c, is_64bit);
            start
        })
        .collect()
}

/// Ids in file order, read from `offset`. Rows without an id are skipped: they
/// cannot be paired with anything.
fn ids_at(reader: &DatReader, offset: usize, limit: u32) -> Vec<(String, u32)> {
    let col = Column {
        name: None,
        description: None,
        array: false,
        r#type: "string".to_string(),
        unique: true,
        localized: false,
        references: None,
        interval: false,
        file: None,
        files: None,
    };
    (0..limit.min(reader.row_count))
        .filter_map(|row| match reader.read_cell_at(row, offset, &col) {
            Ok(DatValue::String(s)) if !s.is_empty() => Some((s, row)),
            _ => None,
        })
        .collect()
}

/// Where the new file keeps its id column. Almost always offset 0, but a patch
/// that prepends a column would move it, and every row pairing depends on it.
fn find_id_offset(old: &DatReader, old_id_offset: usize, new: &DatReader, client: Option<&TableLayout>) -> Option<usize> {
    let known: std::collections::HashSet<String> =
        ids_at(old, old_id_offset, 400).into_iter().map(|(id, _)| id).collect();
    if known.is_empty() {
        return None;
    }
    let row_len = new.row_length.unwrap_or(0);
    let step = if new.is_64bit { 8 } else { 4 };
    let candidates: Vec<usize> = match client {
        // The id is a string and the client fixes up every string, so it sits on one of those.
        Some(client) => client.pointers.iter().filter(|p| p.kind == Kind::String).map(|p| p.offset).collect(),
        None => (0..row_len.saturating_sub(step)).step_by(step).collect(),
    };
    candidates
        .into_iter()
        .map(|offset| {
            let hits = ids_at(new, offset, 400).into_iter().filter(|(id, _)| known.contains(id)).count();
            (hits, offset)
        })
        .filter(|(hits, _)| *hits >= 8)
        // Other string columns can name the same ids (a parent row, a list read
        // as a string); on a tie the column nearest the old id wins.
        .max_by_key(|&(hits, offset)| (hits, std::cmp::Reverse(offset.abs_diff(old_id_offset))))
        .map(|(_, offset)| offset)
}

/// Rows that exist in both patches, as (old row, new row), evenly spread.
fn pair_rows(old: &DatReader, old_id: usize, new: &DatReader, new_id: usize) -> Vec<(u32, u32)> {
    let new_by_id: HashMap<String, u32> =
        ids_at(new, new_id, new.row_count).into_iter().collect();
    let mut pairs: Vec<(u32, u32)> = ids_at(old, old_id, old.row_count)
        .into_iter()
        .filter_map(|(id, old_row)| new_by_id.get(&id).map(|&new_row| (old_row, new_row)))
        .collect();
    if pairs.len() > SAMPLE_ROWS {
        let step = pairs.len() / SAMPLE_ROWS;
        pairs = pairs.into_iter().step_by(step.max(1)).take(SAMPLE_ROWS).collect();
    }
    pairs
}

/// How closely two reads have to agree to count as the same column.
#[derive(Clone, Copy, PartialEq)]
enum Match {
    /// The value itself is unchanged.
    Exact,
    /// Only the shape is: which rows hold a key at all, and how long each list
    /// is. Row indices move whenever the table they point into gains a row, so
    /// a foreign key can be in the same column and still read differently on
    /// the next patch — its pattern of nulls across the table is what stays put.
    Shape,
}

/// Whether two reads describe the same value. Variable-length columns store an
/// offset into the data section, and those move every patch, so lists and
/// strings are compared by what they point at rather than by the pointer.
fn same_value(
    a: &DatValue,
    b: &DatValue,
    old: &DatReader,
    new: &DatReader,
    col: &Column,
    mode: Match,
) -> bool {
    match (a, b) {
        (DatValue::List(count_a, off_a), DatValue::List(count_b, off_b)) => {
            if count_a != count_b {
                return false;
            }
            if *count_a == 0 || mode == Match::Shape {
                return true;
            }
            let elem = Column { array: false, interval: false, ..col.clone() };
            match (
                old.read_list_values(*off_a, *count_a, col),
                new.read_list_values(*off_b, *count_b, col),
            ) {
                (Ok(xs), Ok(ys)) => {
                    xs.len() == ys.len()
                        && xs.iter().zip(&ys).all(|(x, y)| same_value(x, y, old, new, &elem, mode))
                }
                _ => false,
            }
        }
        (DatValue::Interval(a1, a2), DatValue::Interval(b1, b2)) => {
            let elem = Column { interval: false, ..col.clone() };
            same_value(a1, b1, old, new, &elem, mode) && same_value(a2, b2, old, new, &elem, mode)
        }
        (DatValue::ForeignRow(x), DatValue::ForeignRow(y)) => match mode {
            Match::Exact => x == y,
            Match::Shape => (*x == usize::MAX) == (*y == usize::MAX),
        },
        (DatValue::String(x), DatValue::String(y)) => x == y,
        (DatValue::Int(x), DatValue::Int(y)) => x == y,
        (DatValue::Long(x), DatValue::Long(y)) => x == y,
        (DatValue::Bool(x), DatValue::Bool(y)) => x == y,
        (DatValue::Float(x), DatValue::Float(y)) => x.to_bits() == y.to_bits(),
        _ => false,
    }
}

/// What a value contributes when only the shape is being compared.
fn shape_fingerprint(value: &DatValue) -> String {
    match value {
        DatValue::ForeignRow(k) => format!("k{}", *k == usize::MAX),
        DatValue::List(count, _) => format!("n{}", count),
        other => fingerprint(other),
    }
}

/// A cheap stand-in for value identity, used only to count how much a column
/// actually varies across the sample.
fn fingerprint(value: &DatValue) -> String {
    match value {
        DatValue::String(s) => format!("s{}", s),
        DatValue::Int(i) => format!("i{}", i),
        DatValue::Long(l) => format!("l{}", l),
        DatValue::Bool(b) => format!("b{}", b),
        DatValue::Float(f) => format!("f{}", f.to_bits()),
        DatValue::ForeignRow(k) => format!("k{}", k),
        DatValue::List(count, _) => format!("n{}", count),
        DatValue::Interval(a, b) => format!("[{},{}]", fingerprint(a), fingerprint(b)),
        DatValue::Unknown => "?".to_string(),
    }
}

/// Tag on layouts derived here, so a later check can retire them once the
/// community schema describes the table again.
pub const REFIT_TAG: &str = "refit";

pub fn is_refit(table: &Table) -> bool {
    table.tags.as_ref().is_some_and(|tags| tags.iter().any(|t| t == REFIT_TAG))
}

pub enum RefitError {
    /// The layout misreads the older copy too, so it holds no names to carry.
    OldDoesNotFit,
    Failed(String),
}

/// A table carried across a patch, and how its new layout reads.
pub struct Refitted {
    pub report: RefitReport,
    pub after: analysis::FitReport,
}

/// Carries `def` from `file`'s copy in an earlier patch to its copy in this one; `client` holds the new patch's layouts as the game reads them.
pub fn refit_file(old_bytes: Vec<u8>, new_bytes: Vec<u8>, file: &str, def: &Table, client: Option<&ClientLayouts>) -> Result<Refitted, RefitError> {
    let old = DatReader::new(old_bytes, file).map_err(|e| RefitError::Failed(e.to_string()))?;
    let new = DatReader::new(new_bytes, file).map_err(|e| RefitError::Failed(e.to_string()))?;
    if analysis::check_fit(&old, def, 40).is_broken() {
        return Err(RefitError::OldDoesNotFit);
    }
    // A client layout speaks only for a file whose rows are the size that client expects.
    let layout = client.and_then(|c| c.find(&def.name)).filter(|l| l.walked && new.row_count > 0 && new.row_length == Some(l.row_size));
    let report = carry_across_patch(&old, def, &new, layout).map_err(RefitError::Failed)?;
    Ok(tagged(report, &new))
}

/// Repairs a layout that misreads `file` from the client's reading of it alone, for a table no earlier patch reads right.
pub fn repair_from_client(bytes: Vec<u8>, file: &str, def: &Table, client: &ClientLayouts) -> Result<Refitted, RefitError> {
    let new = DatReader::new(bytes, file).map_err(|e| RefitError::Failed(e.to_string()))?;
    let layout = client.find(&def.name).filter(|l| l.walked && new.row_count > 0 && new.row_length == Some(l.row_size));
    let layout = layout.ok_or_else(|| RefitError::Failed("the game client's reading of this table is not known".to_string()))?;
    // A layout that misreads its own file has no values worth matching, so only its structure is held against the client.
    let unmatched = Err("the game client's columns do not line up with the schema's".to_string());
    let report = carry(&new, def, &new, Some(layout), unmatched).map_err(RefitError::Failed)?;
    Ok(tagged(report, &new))
}

fn tagged(mut report: RefitReport, new: &DatReader) -> Refitted {
    let tags = report.table.tags.get_or_insert_with(Vec::new);
    if !tags.iter().any(|t| t == REFIT_TAG) {
        tags.push(REFIT_TAG.to_string());
    }
    let after = analysis::check_fit(new, &report.table, 40);
    Refitted { report, after }
}

/// The id column on both sides and the rows that exist in both patches, as (old row, new row).
struct Paired {
    id_index: usize,
    new_id_offset: usize,
    rows: Vec<(u32, u32)>,
}

fn match_rows(old: &DatReader, old_def: &Table, old_offsets: &[usize], new: &DatReader, client: Option<&TableLayout>) -> Result<Paired, String> {
    let id_index = old_def
        .columns
        .iter()
        .position(|c| c.name.as_deref() == Some("Id") && c.r#type == "string" && !c.array)
        .or_else(|| old_def.columns.iter().position(|c| c.unique && c.r#type == "string" && !c.array))
        .ok_or("the old schema has no Id column to match rows on")?;
    let new_id_offset = find_id_offset(old, old_offsets[id_index], new, client).ok_or("could not find the id column in the new file")?;
    let rows = pair_rows(old, old_offsets[id_index], new, new_id_offset);
    match rows.len() {
        1 => Err("only 1 row appears in both patches — too few to compare".to_string()),
        n if n < 8 => Err(format!("only {} rows appear in both patches — too few to compare", n)),
        _ => Ok(Paired { id_index, new_id_offset, rows }),
    }
}

/// Re-derives `old_def`'s layout against a newer copy of the same file, on structure alone when too few rows are shared and `client` is given.
pub fn carry_across_patch(
    old: &DatReader,
    old_def: &Table,
    new: &DatReader,
    client: Option<&TableLayout>,
) -> Result<RefitReport, String> {
    let matched = match_rows(old, old_def, &offsets(old_def, old.is_64bit), new, client);
    carry(old, old_def, new, client, matched)
}

fn carry(old: &DatReader, old_def: &Table, new: &DatReader, client: Option<&TableLayout>, matched: Result<Paired, String>) -> Result<RefitReport, String> {
    let old_row_len = old.row_length.ok_or("the old file has no fixed row length")?;
    let new_row_len = new.row_length.ok_or("the new file has no fixed row length")?;
    let old_offsets = offsets(old_def, old.is_64bit);

    let (mut placed, pairs) = match (&matched, client) {
        (Ok(paired), _) => (place_by_value(old, old_def, new, &old_offsets, paired, client), paired.rows.clone()),
        (Err(_), Some(_)) => (vec![None; old_def.columns.len()], Vec::new()),
        (Err(why), None) => return Err(why.clone()),
    };

    if let Some(client) = client {
        anchor_pointers(old_def, &old_offsets, new, client, &mut placed);
    }
    let sample_rows: Vec<u32> = match pairs.is_empty() {
        true => (0..new.row_count.min(40)).collect(),
        false => pairs.iter().map(|&(_, new_row)| new_row).take(40).collect(),
    };
    fill_gaps(old, old_def, new, &old_offsets, &pairs, &sample_rows, changed_window(old, new), client, &mut placed);

    // Unnamed columns that found a place keep the type the schema gave them, rather than a fresh guess at their bytes.
    let kept: HashMap<usize, &Column> = placed.iter().zip(&old_def.columns).filter_map(|(column, col)| Some((column.as_ref()?.new_offset, col))).collect();
    let mut carried: Vec<CarriedColumn> = Vec::new();
    let mut lost: Vec<String> = Vec::new();
    for (index, col) in old_def.columns.iter().enumerate() {
        let Some(name) = col.name.clone() else { continue };
        match placed[index].take() {
            Some(column) => carried.push(column),
            None => lost.push(name),
        }
    }
    if let (true, Err(why)) = (carried.is_empty(), matched) {
        return Err(why);
    }

    let table = assemble(old_def, new, &kept, client, true);
    // A list the client does not walk has only a guessed element type, and stays opaque when that guess misreads.
    let table = match client.is_some() && analysis::check_fit(new, &table, 40).is_broken() {
        true => assemble(old_def, new, &kept, client, false),
        false => table,
    };
    Ok(RefitReport {
        table,
        carried,
        lost,
        matched_rows: pairs.len(),
        old_row_len,
        new_row_len,
    })
}

/// Where each named column's values reappear in the new rows, in schema order.
fn place_by_value(old: &DatReader, old_def: &Table, new: &DatReader, old_offsets: &[usize], paired: &Paired, client: Option<&TableLayout>) -> Vec<Option<CarriedColumn>> {
    let (id_index, new_id_offset, pairs) = (paired.id_index, paired.new_id_offset, &paired.rows);
    let new_row_len = new.row_length.unwrap_or(0);
    let mut placed: Vec<Option<CarriedColumn>> = vec![None; old_def.columns.len()];
    let mut floor = 0usize;

    for (index, col) in old_def.columns.iter().enumerate() {
        let Some(name) = col.name.clone() else { continue };
        let size = get_column_size(col, old.is_64bit);
        if index == id_index {
            placed[index] = Some(CarriedColumn {
                name,
                old_offset: old_offsets[id_index],
                new_offset: new_id_offset,
                agreement: 1.0,
                distinct: pairs.len(),
                by_position: false,
            });
            floor = new_id_offset + size;
            continue;
        }

        let old_values: Vec<DatValue> = pairs
            .iter()
            .map(|&(old_row, _)| {
                old.read_cell_at(old_row, old_offsets[index], col).unwrap_or(DatValue::Unknown)
            })
            .collect();

        // The value itself is the strongest evidence. Where it fails — keys
        // into a table that gained rows read differently on both sides — the
        // column's shape across the sample is tried instead, at a higher bar.
        for mode in [Match::Exact, Match::Shape] {
            let distinct = match mode {
                Match::Exact => old_values.iter().map(fingerprint).collect::<std::collections::HashSet<_>>().len(),
                Match::Shape => old_values.iter().map(shape_fingerprint).collect::<std::collections::HashSet<_>>().len(),
            };
            // One repeated value agrees with every offset that happens to hold
            // it, so such a column is left to the arithmetic pass below.
            if distinct < 2 {
                continue;
            }
            let bar = match mode {
                Match::Exact => MIN_AGREEMENT,
                Match::Shape => MIN_SHAPE_AGREEMENT,
            };

            let mut scores: Vec<(f32, usize)> = Vec::new();
            for offset in floor..=new_row_len.saturating_sub(size) {
                let agrees = |rows: usize| -> usize {
                    pairs
                        .iter()
                        .zip(&old_values)
                        .take(rows)
                        .filter(|((_, new_row), old_value)| {
                            new.read_cell_at(*new_row, offset, col)
                                .map(|got| same_value(old_value, &got, old, new, col, mode))
                                .unwrap_or(false)
                        })
                        .count()
                };
                // Probe a handful of rows before paying for the whole sample.
                let probe_rows = PROBE_ROWS.min(pairs.len());
                if (agrees(probe_rows) as f32) < bar * probe_rows as f32 {
                    continue;
                }
                scores.push((agrees(pairs.len()) as f32 / pairs.len() as f32, offset));
            }
            scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            // The client settles a tie between offsets that agree equally; a better match elsewhere stands, as the schema may have the type wrong.
            if let Some(client) = client {
                let agreed: Vec<(f32, usize)> = scores.iter().copied().filter(|(_, offset)| !client.contradicts(col, *offset)).collect();
                if agreed.first().is_some_and(|(score, _)| *score >= bar && *score >= scores[0].0) {
                    scores = agreed;
                }
            }

            let Some(&(best, offset)) = scores.first() else { continue };
            let runner_up = scores.iter().find(|(_, o)| *o != offset).map(|(s, _)| *s).unwrap_or(0.0);
            if best >= bar && best - runner_up >= MIN_MARGIN {
                placed[index] = Some(CarriedColumn {
                    name,
                    old_offset: old_offsets[index],
                    new_offset: offset,
                    agreement: best,
                    distinct,
                    by_position: false,
                });
                floor = offset + size;
                break;
            }
        }
    }
    placed
}

/// Each run of unplaced columns with the byte span it must fit; unnamed columns count, or a span never balances.
fn unplaced_runs(sizes: &[usize], row_len: usize, placed: &[Option<CarriedColumn>]) -> Vec<(Vec<usize>, usize, usize)> {
    let mut runs: Vec<(Vec<usize>, usize, usize)> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut span_start = 0usize;
    for (index, column) in placed.iter().enumerate() {
        match column {
            Some(column) => {
                if !run.is_empty() {
                    runs.push((std::mem::take(&mut run), span_start, column.new_offset));
                }
                span_start = column.new_offset + sizes[index];
            }
            None => run.push(index),
        }
    }
    if !run.is_empty() {
        runs.push((run, span_start, row_len));
    }
    runs
}

/// Puts unplaced pointer columns on the client's own, where a run's columns pair with them in order in exactly one best way.
fn anchor_pointers(old_def: &Table, old_offsets: &[usize], new: &DatReader, client: &TableLayout, placed: &mut [Option<CarriedColumn>]) {
    let sizes: Vec<usize> = old_def.columns.iter().map(|c| get_column_size(c, new.is_64bit)).collect();
    for (indices, start, end) in unplaced_runs(&sizes, new.row_length.unwrap_or(0), placed) {
        let mut before = vec![0usize; indices.len() + 1];
        for (position, index) in indices.iter().enumerate() {
            before[position + 1] = before[position] + sizes[*index];
        }
        let columns: Vec<usize> = (0..indices.len()).filter(|p| exe_layout::schema_kind(&old_def.columns[indices[*p]]).is_some()).collect();
        let pointers: Vec<&Pointer> = client.pointers.iter().filter(|p| p.offset >= start && p.offset + p.width() <= end).collect();
        let fits = |a: usize, b: usize| pointers[b].fits(&old_def.columns[indices[columns[a]]]);
        // Bytes the run's other columns need before the first pairing, between two, and after the last.
        let room = Room { before: &before, columns: &columns, starts: pointers.iter().map(|p| p.offset).collect(), ends: pointers.iter().map(|p| p.offset + p.width()).collect(), start, end };
        for (a, b) in line_up(columns.len(), pointers.len(), fits, &room) {
            let index = indices[columns[a]];
            placed[index] = Some(CarriedColumn {
                name: old_def.columns[index].name.clone().unwrap_or_default(),
                old_offset: old_offsets[index],
                new_offset: pointers[b].offset,
                agreement: 0.0,
                distinct: 0,
                by_position: true,
            });
        }
    }
}

/// The room a pairing must leave for the columns between: a patch adds columns far more often than it removes them.
struct Room<'a> {
    before: &'a [usize],
    columns: &'a [usize],
    starts: Vec<usize>,
    ends: Vec<usize>,
    start: usize,
    end: usize,
}

impl Room<'_> {
    fn leads(&self, a: usize, b: usize) -> bool {
        self.starts[b] - self.start >= self.before[self.columns[a]]
    }

    fn follows(&self, from: (usize, usize), to: (usize, usize)) -> bool {
        self.starts[to.1] >= self.ends[from.1] && self.starts[to.1] - self.ends[from.1] >= self.before[self.columns[to.0]] - self.before[self.columns[from.0] + 1]
    }

    fn closes(&self, a: usize, b: usize) -> bool {
        self.end - self.ends[b] >= self.before[self.before.len() - 1] - self.before[self.columns[a] + 1]
    }
}

/// The longest in-order pairing of `m` columns with `n` pointers; nothing when two are equally long, since a tie means the layout cannot say.
fn line_up(m: usize, n: usize, fits: impl Fn(usize, usize) -> bool, room: &Room) -> Vec<(usize, usize)> {
    // For each pairing ending at (a, b): its length, how many reach that length, and one way back.
    let mut best = vec![vec![(0usize, 0u64, None::<(usize, usize)>); n]; m];
    for a in 0..m {
        for b in 0..n {
            if !fits(a, b) {
                continue;
            }
            let mut cell = if room.leads(a, b) { (1, 1, None) } else { (0, 0, None) };
            for (from_a, earlier) in best.iter().enumerate().take(a) {
                for (from_b, &(length, ways, _)) in earlier.iter().enumerate().take(b) {
                    if length == 0 || !room.follows((from_a, from_b), (a, b)) {
                        continue;
                    }
                    match (length + 1).cmp(&cell.0) {
                        std::cmp::Ordering::Greater => cell = (length + 1, ways, Some((from_a, from_b))),
                        std::cmp::Ordering::Equal => cell.1 = cell.1.saturating_add(ways),
                        std::cmp::Ordering::Less => {}
                    }
                }
            }
            best[a][b] = cell;
        }
    }
    let ends: Vec<(usize, usize)> = (0..m).flat_map(|a| (0..n).map(move |b| (a, b))).filter(|&(a, b)| best[a][b].0 > 0 && room.closes(a, b)).collect();
    let longest = ends.iter().map(|&(a, b)| best[a][b].0).max().unwrap_or(0);
    let tied: Vec<(usize, usize)> = ends.into_iter().filter(|&(a, b)| best[a][b].0 == longest).collect();
    let [last] = tied[..] else { return Vec::new() };
    if best[last.0][last.1].1 != 1 {
        return Vec::new();
    }
    let mut pairing = vec![last];
    while let Some(previous) = best[pairing[pairing.len() - 1].0][pairing[pairing.len() - 1].1].2 {
        pairing.push(previous);
    }
    pairing.reverse();
    pairing
}

/// The stretch of the new row where this patch changed the layout, worked out
/// from the two files' own column widths rather than from the schema: the run
/// of widths at the start that still reads the same, and the run at the end
/// that lines up again, bracket whatever GGG added or removed in between.
fn changed_window(old: &DatReader, new: &DatReader) -> Option<(usize, usize)> {
    let widths = |reader: &DatReader| -> Vec<usize> {
        analysis::analyze(reader).iter().map(|g| g.size).collect()
    };
    let (before, after) = (widths(old), widths(new));
    if before.is_empty() || after.is_empty() {
        return None;
    }
    let prefix = before.iter().zip(&after).take_while(|(x, y)| x == y).count();
    let tail = before.iter().rev().zip(after.iter().rev()).take_while(|(x, y)| x == y).count();
    let suffix = tail.min(after.len().saturating_sub(prefix));
    let lo: usize = after[..prefix].iter().sum();
    let hi: usize = after[..after.len() - suffix].iter().sum();
    // Among columns of one width the tail lines up from any of them, so the change may sit anywhere in that run.
    let earliest: usize = after[..before.len().saturating_sub(tail).min(prefix)].iter().sum();
    Some((lo.min(hi).min(earliest), hi.max(lo)))
}
/// How many of the sampled reads at these offsets could not be real: the test
/// that tells one candidate placement from another when the values themselves
/// have nothing left to say.
fn count_impossible(
    old_def: &Table,
    new: &DatReader,
    indices: &[usize],
    offsets: &[usize],
    rows: &[u32],
) -> usize {
    let data_len = new.get_data().len();
    let var_start = new.data_section_offset as usize;
    let mut bad = 0;
    for (position, index) in indices.iter().enumerate() {
        let col = &old_def.columns[*index];
        for &row in rows {
            match new.read_cell_at(row, offsets[position], col) {
                Ok(value) => {
                    if !analysis::value_is_possible(&value, col, new, var_start, data_len) {
                        bad += 1;
                    }
                }
                Err(_) => bad += 1,
            }
        }
    }
    bad
}
/// What counts against one way of splitting a run, strongest evidence first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Split {
    impossible: usize,
    conflicts: usize,
    changed: usize,
    adrift: usize,
    strays: usize,
}

/// Places the columns the value search could not claim. Between two columns it
/// did place, the bytes left over are the unplaced columns' own — but only when
/// they add up exactly. Anything else is left unplaced rather than guessed at.
#[allow(clippy::too_many_arguments)]
fn fill_gaps(
    old: &DatReader,
    old_def: &Table,
    new: &DatReader,
    old_offsets: &[usize],
    pairs: &[(u32, u32)],
    rows: &[u32],
    window: Option<(usize, usize)>,
    client: Option<&TableLayout>,
    placed: &mut [Option<CarriedColumn>],
) {
    let is_64bit = new.is_64bit;
    let new_row_len = new.row_length.unwrap_or(0);
    let size_of = |index: usize| get_column_size(&old_def.columns[index], is_64bit);
    if placed.iter().all(|column| column.is_none()) {
        return;
    }

    let boundaries: std::collections::HashSet<usize> =
        analysis::analyze(new).iter().map(|g| g.offset).collect();
    let sizes: Vec<usize> = (0..old_def.columns.len()).map(size_of).collect();

    for (indices, start, end) in unplaced_runs(&sizes, new_row_len, placed) {
        let needed: usize = indices.iter().map(|i| size_of(*i)).sum();
        let span = end.saturating_sub(start);
        if needed > span {
            continue;
        }
        let shared = &pairs[..pairs.len().min(40)];
        let held_before: Vec<Vec<DatValue>> = indices
            .iter()
            .map(|i| shared.iter().map(|&(old_row, _)| old.read_cell_at(old_row, old_offsets[*i], &old_def.columns[*i]).unwrap_or(DatValue::Unknown)).collect())
            .collect();
        // The patch added `span - needed` bytes somewhere inside this run. Each
        // split puts them at a different point; the right one is the split the
        // file can be read through, and failing that the one whose columns start
        // where the file's own layout says columns start.
        let mut candidates: Vec<(Split, usize)> = Vec::new();
        for split in 0..=indices.len() {
            let offsets = place_run(&indices, start, end, split, &size_of);
            let impossible = count_impossible(old_def, new, &indices, &offsets, rows);
            // Where this split says the new bytes went, against where the two
            // files themselves say the layout changed.
            let gap = start + indices[..split].iter().map(|i| size_of(*i)).sum::<usize>();
            let adrift = match window {
                Some((lo, hi)) if gap >= lo && gap <= hi => 0,
                Some((lo, hi)) => lo.abs_diff(gap).min(hi.abs_diff(gap)),
                None => 0,
            };
            let strays = offsets.iter().filter(|o| !boundaries.contains(o)).count();
            // With no shared rows to hold a split against, the file's guessed boundaries are too little to choose by.
            let (adrift, strays) = if pairs.is_empty() { (0, 0) } else { (adrift, strays) };
            // Columns this split would lay over bytes the client reads as something else.
            let conflicts = client.map_or(0, |c| indices.iter().zip(&offsets).filter(|(index, offset)| c.contradicts(&old_def.columns[**index], **offset)).count());
            // A constant column says nothing to the value search, but laid over other bytes it stops being constant.
            let changed: usize = indices
                .iter()
                .zip(&offsets)
                .zip(&held_before)
                .map(|((index, offset), held)| {
                    let col = &old_def.columns[*index];
                    let mode = if matches!(col.r#type.as_str(), "foreignrow" | "foreign_row" | "row") { Match::Shape } else { Match::Exact };
                    let same = |before: &DatValue, new_row: u32| new.read_cell_at(new_row, *offset, col).is_ok_and(|now| same_value(before, &now, old, new, col, mode));
                    shared.iter().zip(held).filter(|((_, new_row), before)| !same(before, *new_row)).count()
                })
                .sum();
            candidates.push((Split { impossible, conflicts, changed, adrift, strays }, split));
            if needed == span {
                break; // Nothing was inserted, so there is only one placement.
            }
        }
        candidates.sort_by_key(|(score, split)| (*score, *split));
        if std::env::var("REFIT_DEBUG").is_ok() {
            let names: Vec<String> = indices.iter().map(|i| old_def.columns[*i].name.clone().unwrap_or_else(|| format!("_{}", i))).collect();
            eprintln!("run {}..{} span {} needed {} · {:?} · candidates {:?}", start, end, span, needed, names, &candidates[..candidates.len().min(6)]);
        }
        let Some(&(best, split)) = candidates.first() else { continue };
        // Placed only when one split reads better than every other. A tie means
        // the file cannot say where the new bytes went, and a guess there would
        // put every following column one field out.
        let contested = candidates.iter().filter(|(score, _)| *score == best).count() > 1;
        let offsets = place_run(&indices, start, end, split, &size_of);
        // With no shared rows to vouch for a placement, one the client reads otherwise has nothing behind it.
        let clashes: Vec<usize> = match (pairs.is_empty(), client) {
            (true, Some(client)) => (0..indices.len()).filter(|p| client.contradicts(&old_def.columns[indices[*p]], offsets[*p])).collect(),
            _ => Vec::new(),
        };
        // In a run whose widths still add up, a side of the clash keeps its places only when the clash ends on the client's own column edge there.
        let keep: Vec<usize> = match (clashes.first(), clashes.last(), client) {
            (Some(&first), Some(&last), Some(client)) if needed == span => {
                let after = offsets[last] + sizes[indices[last]];
                let (leads, trails) = (client.pointers.iter().any(|p| p.offset == offsets[first]), client.pointers.iter().any(|p| p.offset + p.width() == after));
                (0..indices.len()).filter(|p| (*p < first && leads) || (*p > last && trails)).collect()
            }
            (Some(_), _, _) => Vec::new(),
            _ => (0..indices.len()).collect(),
        };
        let (kept_indices, kept_offsets): (Vec<usize>, Vec<usize>) = keep.iter().map(|p| (indices[*p], offsets[*p])).unzip();
        let unreadable = match clashes.is_empty() {
            true => best.impossible > 0,
            false => count_impossible(old_def, new, &kept_indices, &kept_offsets, rows) > 0,
        };
        if unreadable || contested {
            continue;
        }

        for (index, offset) in kept_indices.into_iter().zip(kept_offsets) {
            placed[index] = Some(CarriedColumn {
                name: old_def.columns[index].name.clone().unwrap_or_default(),
                old_offset: old_offsets[index],
                new_offset: offset,
                agreement: 0.0,
                distinct: 0,
                by_position: true,
            });
        }
    }
}

/// Offsets for a run of columns when the bytes the patch inserted are taken to
/// sit at `split`: everything before it follows `start`, everything from it is
/// packed against `end`.
fn place_run(
    indices: &[usize],
    start: usize,
    end: usize,
    split: usize,
    size_of: &impl Fn(usize) -> usize,
) -> Vec<usize> {
    let mut at = start;
    let mut offsets = Vec::with_capacity(indices.len());
    for (position, index) in indices.iter().enumerate() {
        if position == split {
            at = end - indices[position..].iter().map(|i| size_of(*i)).sum::<usize>();
        }
        offsets.push(at);
        at += size_of(*index);
    }
    offsets
}

/// Builds a layout that tiles the new row: the columns that were placed, with
/// the file's own guessed columns filling the gaps between them.
fn assemble(old_def: &Table, new: &DatReader, placed: &HashMap<usize, &Column>, client: Option<&TableLayout>, guess_lists: bool) -> Table {
    let row_len = new.row_length.unwrap_or(0);
    let guessed = analysis::analyze(new);
    let guessed_at: HashMap<usize, usize> =
        guessed.iter().enumerate().map(|(i, g)| (g.offset, i)).collect();

    let mut columns = Vec::new();
    let mut at = 0usize;
    while at < row_len {
        if let Some(col) = placed.get(&at) {
            columns.push((*col).clone());
            at += get_column_size(col, new.is_64bit);
            continue;
        }
        // An unclaimed stretch: take the file's own reading of it where that
        // fits, and pad a byte at a time where it does not.
        let next_placed = placed.keys().filter(|o| **o > at).min().copied().unwrap_or(row_len);
        let pointers = client.map(|c| c.pointers.as_slice()).unwrap_or_default();
        if let Some(pointer) = pointers.iter().find(|p| p.offset == at && at + p.width() <= next_placed) {
            // A list the client does not walk keeps the element type the file suggests.
            let unwalked = guess_lists && pointer.kind == Kind::List && pointer.element.is_none();
            let guess = guessed_at.get(&at).map(|i| &guessed[*i]).filter(|g| unwalked && g.size == pointer.width() && matches!(g.guess, analysis::Guess::Array(_)));
            columns.push(match guess {
                Some(guess) => Column { name: None, ..analysis::synth_column(guess, "unclaimed by the previous patch") },
                None => pointer.column(),
            });
            at += pointer.width();
            continue;
        }
        let next_placed = pointers.iter().map(|p| p.offset).find(|o| *o > at).map_or(next_placed, |o| o.min(next_placed));
        if let Some(&i) = guessed_at.get(&at) {
            let guess = &guessed[i];
            if at + guess.size <= next_placed {
                let mut col = analysis::synth_column(guess, "unclaimed by the previous patch");
                col.name = None;
                columns.push(col);
                at += guess.size;
                continue;
            }
        }
        columns.push(Column {
            name: None,
            description: None,
            array: false,
            r#type: "u8".to_string(),
            unique: false,
            localized: false,
            references: None,
            interval: false,
            file: None,
            files: None,
        });
        at += 1;
    }

    Table {
        name: old_def.name.clone(),
        columns,
        tags: old_def.tags.clone(),
        valid_for: old_def.valid_for,
        custom: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> Column {
        Column {
            name: Some(name.to_string()),
            description: None,
            array: false,
            r#type: ty.to_string(),
            unique: name == "Id",
            localized: false,
            references: None,
            interval: false,
            file: None,
            files: None,
        }
    }

    /// Builds a 64-bit dat file: a row count, `rows` of fixed bytes, the 8-byte
    /// separator, then the UTF-16 strings each row's first column points at.
    fn build(rows: &[Vec<u8>], strings: &[&str]) -> DatReader {
        let row_len = rows[0].len();
        let mut data = Vec::new();
        data.extend_from_slice(&(rows.len() as u32).to_le_bytes());
        for row in rows {
            data.extend_from_slice(row);
        }
        data.extend_from_slice(&[0xBB; 8]);
        let var_start = data.len();
        let mut offsets = Vec::new();
        for s in strings {
            // Stored offsets count from the data section and carry a +8 bias.
            offsets.push((data.len() - var_start) as u32 + 8);
            for unit in s.encode_utf16() {
                data.extend_from_slice(&unit.to_le_bytes());
            }
            data.extend_from_slice(&[0, 0]);
        }
        for (i, offset) in offsets.iter().enumerate() {
            let at = 4 + i * row_len;
            data[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        }
        DatReader::new(data, "t.datc64").unwrap()
    }

    /// A column that moved because the patch widened the row keeps its name.
    #[test]
    fn carries_a_column_that_moved() {
        // old row: [id ptr (8)][value i32 (4)][pad (4)]
        let old_rows: Vec<Vec<u8>> = (0..12u32)
            .map(|i| {
                let mut row = vec![0u8; 16];
                row[8..12].copy_from_slice(&(1000 + i).to_le_bytes());
                row
            })
            .collect();
        let names: Vec<String> = (0..12).map(|i| format!("Row{}", i)).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let old = build(&old_rows, &refs);

        // new row: the same value, four bytes further along.
        let new_rows: Vec<Vec<u8>> = (0..12u32)
            .map(|i| {
                let mut row = vec![0u8; 20];
                row[12..16].copy_from_slice(&(1000 + i).to_le_bytes());
                row
            })
            .collect();
        let new = build(&new_rows, &refs);

        let def = Table {
            name: "T".into(),
            columns: vec![col("Id", "string"), col("Value", "i32")],
            tags: None,
            valid_for: Some(2),
            custom: false,
        };
        let report = carry_across_patch(&old, &def, &new, None).unwrap();
        let value = report.carried.iter().find(|c| c.name == "Value").expect("Value was not carried");
        assert_eq!(value.new_offset, 12);
        assert!(value.agreement > 0.9, "agreement was {}", value.agreement);
        assert!(report.lost.is_empty(), "lost {:?}", report.lost);
    }

    /// The recovered layout has to tile the row, or nothing can read it.
    #[test]
    fn assembled_layout_tiles_the_row() {
        let old_rows: Vec<Vec<u8>> = (0..12u32)
            .map(|i| {
                let mut row = vec![0u8; 16];
                row[8..12].copy_from_slice(&(7 + i).to_le_bytes());
                row
            })
            .collect();
        let names: Vec<String> = (0..12).map(|i| format!("Row{}", i)).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let old = build(&old_rows, &refs);
        let new_rows: Vec<Vec<u8>> = (0..12u32)
            .map(|i| {
                let mut row = vec![0u8; 24];
                row[16..20].copy_from_slice(&(7 + i).to_le_bytes());
                row
            })
            .collect();
        let new = build(&new_rows, &refs);
        let def = Table {
            name: "T".into(),
            columns: vec![col("Id", "string"), col("Value", "i32")],
            tags: None,
            valid_for: Some(2),
            custom: false,
        };
        let report = carry_across_patch(&old, &def, &new, None).unwrap();
        let total: usize =
            report.table.columns.iter().map(|c| get_column_size(c, true)).sum();
        assert_eq!(total, 24, "layout must tile the 24-byte row");
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;
    use crate::dat::schema::TableReference;

    fn col(name: &str, ty: &str, array: bool, target: Option<&str>) -> Column {
        Column {
            name: Some(name.to_string()),
            description: None,
            array,
            r#type: ty.to_string(),
            unique: name == "Id",
            localized: false,
            references: target.map(|t| TableReference { table: t.to_string(), column: None }),
            interval: false,
            file: None,
            files: None,
        }
    }

    fn table(columns: Vec<Column>) -> Table {
        Table { name: "T".into(), columns, tags: None, valid_for: Some(2), custom: false }
    }

    fn pointer(offset: usize, kind: Kind, target: Option<&str>) -> Pointer {
        let element = (kind == Kind::List).then_some(16);
        Pointer { offset, kind, element, target: target.map(str::to_string), proven: target.is_some() }
    }

    fn layout(row_size: usize, pointers: Vec<Pointer>) -> TableLayout {
        TableLayout { table: "T".into(), row_size, walked: true, pointers }
    }

    /// UTF-16 strings laid end to end, and the offset a row stores for each.
    fn strings(names: &[String]) -> (Vec<u8>, Vec<u64>) {
        let (mut var, mut at) = (Vec::new(), Vec::new());
        for name in names {
            at.push(var.len() as u64 + 8);
            name.encode_utf16().chain([0]).for_each(|unit| var.extend_from_slice(&unit.to_le_bytes()));
        }
        (var, at)
    }

    fn file(rows: &[Vec<u8>], var: &[u8]) -> DatReader {
        let mut data = (rows.len() as u32).to_le_bytes().to_vec();
        rows.iter().for_each(|row| data.extend_from_slice(row));
        data.extend_from_slice(&[0xBB; 8]);
        data.extend_from_slice(var);
        DatReader::new(data, "t.datc64").unwrap()
    }

    fn row(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    fn offset_of(report: &RefitReport, name: &str) -> Option<usize> {
        report.carried.iter().find(|c| c.name == name).map(|c| c.new_offset)
    }

    const EMPTY_LIST: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];

    #[test]
    fn the_client_tells_a_new_list_from_the_empty_lists_beside_it() {
        let names: Vec<String> = (0..12).map(|i| format!("Row{}", i)).collect();
        let (var, ids) = strings(&names);
        let value = |i: usize| (1000 + i as u32).to_le_bytes();
        let old_rows: Vec<Vec<u8>> = (0..12).map(|i| row(&[&ids[i].to_le_bytes(), &EMPTY_LIST, &EMPTY_LIST, &value(i)])).collect();
        let new_rows: Vec<Vec<u8>> = (0..12).map(|i| row(&[&ids[i].to_le_bytes(), &EMPTY_LIST, &EMPTY_LIST, &EMPTY_LIST, &value(i)])).collect();
        let (old, new) = (file(&old_rows, &var), file(&new_rows, &var));
        let def = table(vec![col("Id", "string", false, None), col("Tags", "foreignrow", true, Some("Tags")), col("Stats", "foreignrow", true, Some("Stats")), col("Value", "i32", false, None)]);
        let client = layout(60, vec![pointer(0, Kind::String, None), pointer(8, Kind::List, Some("Tags")), pointer(24, Kind::List, Some("Mods")), pointer(40, Kind::List, Some("Stats"))]);

        let blind = carry_across_patch(&old, &def, &new, None).unwrap();
        assert_ne!(offset_of(&blind, "Stats"), Some(40), "three empty lists look alike without the client");

        let report = carry_across_patch(&old, &def, &new, Some(&client)).unwrap();
        assert_eq!((offset_of(&report, "Tags"), offset_of(&report, "Stats"), offset_of(&report, "Value")), (Some(8), Some(40), Some(56)));
        assert!(report.lost.is_empty(), "lost {:?}", report.lost);
        let added = &report.table.columns[2];
        assert_eq!((added.name.as_deref(), added.r#type.as_str(), added.array), (None, "foreignrow", true));
        assert_eq!(added.references.as_ref().map(|r| r.table.as_str()), Some("Mods"));
        assert_eq!(report.table.columns.iter().map(|c| get_column_size(c, true)).sum::<usize>(), 60);
    }

    #[test]
    fn a_table_with_no_shared_rows_is_carried_on_its_structure() {
        let texts: Vec<String> = (0..12).flat_map(|i| [format!("Old{}", i), format!("New{}", i), format!("name {}", i)]).collect();
        let (var, at) = strings(&texts);
        let key = |i: usize| row(&[&(i as u64).to_le_bytes(), &0u64.to_le_bytes()]);
        let count = |i: usize| (i as u32 * 3).to_le_bytes();
        let old_rows: Vec<Vec<u8>> = (0..12).map(|i| row(&[&at[i * 3].to_le_bytes(), &count(i), &at[i * 3 + 2].to_le_bytes(), &key(i)])).collect();
        let new_rows: Vec<Vec<u8>> = (0..12).map(|i| row(&[&at[i * 3 + 1].to_le_bytes(), &count(i), &[0; 4], &at[i * 3 + 2].to_le_bytes(), &key(i)])).collect();
        let (old, new) = (file(&old_rows, &var), file(&new_rows, &var));
        let def = table(vec![col("Id", "string", false, None), col("Count", "i32", false, None), col("Name", "string", false, None), col("Stat", "foreignrow", false, Some("Stats"))]);
        let client = layout(40, vec![pointer(0, Kind::String, None), pointer(16, Kind::String, None), pointer(24, Kind::Key, Some("Stats"))]);

        assert!(carry_across_patch(&old, &def, &new, None).is_err(), "no row pairs and no client leaves nothing to go on");
        let report = carry_across_patch(&old, &def, &new, Some(&client)).unwrap();
        assert_eq!(report.matched_rows, 0);
        assert_eq!((offset_of(&report, "Id"), offset_of(&report, "Name"), offset_of(&report, "Stat")), (Some(0), Some(16), Some(24)));
        assert!(report.carried.iter().all(|c| c.by_position));
        assert_eq!(report.table.columns.iter().map(|c| get_column_size(c, true)).sum::<usize>(), 40);
    }

    #[test]
    fn a_layout_no_patch_read_right_is_repaired_from_the_client() {
        let names: Vec<String> = (0..12).map(|i| format!("Row{}", i)).collect();
        // Each row's string and its two one-number lists, written row by row the way the game writes them.
        let mut var: Vec<u8> = Vec::new();
        let mut written = |bytes: &[u8]| {
            let at = var.len() as u64 + 8;
            var.extend_from_slice(bytes);
            at
        };
        let rows: Vec<Vec<u8>> = (0..12u32)
            .map(|i| {
                let text: Vec<u8> = names[i as usize].encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
                let id = written(&text);
                let lists = [i * 7 + 3, i * 11 + 5].map(|value| row(&[&1u64.to_le_bytes(), &written(&value.to_le_bytes()).to_le_bytes()]));
                row(&[&id.to_le_bytes(), &(i + 1).to_le_bytes(), &[(i % 2) as u8], &lists[0], &lists[1], &(900 + i).to_le_bytes()])
            })
            .collect();
        let bytes = {
            let mut data = (rows.len() as u32).to_le_bytes().to_vec();
            rows.iter().for_each(|row| data.extend_from_slice(row));
            data.extend_from_slice(&[0xBB; 8]);
            data.extend_from_slice(&var);
            data
        };
        // The schema's widths add up to the 49-byte row, but it reads a string and two numbers where the flag and first list are.
        let mut def = table(vec![
            col("Id", "string", false, None),
            col("Count", "i32", false, None),
            col("Name", "string", false, None),
            col("Flag", "bool", false, None),
            col("A", "i32", false, None),
            col("B", "i32", false, None),
            col("Values", "i32", true, None),
            col("Last", "i32", false, None),
        ]);
        def.columns[4].name = None;
        def.columns[5].name = None;
        let reader = DatReader::new(bytes.clone(), "t.datc64").unwrap();
        assert!(analysis::check_fit(&reader, &def, 40).is_broken(), "the schema should misread this file");

        let unwalked = |offset| Pointer { offset, kind: Kind::List, element: None, target: None, proven: false };
        let client = ClientLayouts { tables: vec![layout(49, vec![pointer(0, Kind::String, None), unwalked(13), unwalked(29)])] };
        let repaired = repair_from_client(bytes, "t.datc64", &def, &client).map_err(|_| "no repair").unwrap();
        assert!(!repaired.after.is_broken(), "{}", repaired.after.summary());
        let report = repaired.report;
        assert_eq!((offset_of(&report, "Id"), offset_of(&report, "Values"), offset_of(&report, "Last")), (Some(0), Some(29), Some(45)));
        // Name starts a byte before the client's list, so nothing says the number before it is where the schema has it.
        assert_eq!(report.lost, vec!["Count".to_string(), "Name".to_string(), "Flag".to_string()]);
        assert_eq!(report.table.columns.iter().map(|c| get_column_size(c, true)).sum::<usize>(), 49);
        assert!(is_refit(&report.table));
        let starts: Vec<usize> = offsets(&report.table, true);
        let list = &report.table.columns[starts.iter().position(|at| *at == 13).expect("no column starts at byte 13")];
        assert!(list.array && list.name.is_none());
    }

    #[test]
    fn a_column_behind_a_stretch_the_client_reads_whole_keeps_its_place() {
        let names: Vec<String> = (0..12).map(|i| format!("Row{}", i)).collect();
        let (var, ids) = strings(&names);
        let key = |i: usize| row(&[&(i as u64 % 3).to_le_bytes(), &0u64.to_le_bytes()]);
        let rows: Vec<Vec<u8>> = (0..12).map(|i| row(&[&ids[i].to_le_bytes(), &key(i), &(40 + i as u32).to_le_bytes()])).collect();
        let bytes = {
            let mut data = (rows.len() as u32).to_le_bytes().to_vec();
            rows.iter().for_each(|row| data.extend_from_slice(row));
            data.extend_from_slice(&[0xBB; 8]);
            data.extend_from_slice(&var);
            data
        };
        // The schema splits the 16 bytes of a key into a string and two numbers; Family sits right behind them.
        let def = table(vec![col("Id", "string", false, None), col("Text", "string", false, None), col("A", "i32", false, None), col("B", "i32", false, None), col("Family", "i32", false, None)]);
        let client = ClientLayouts { tables: vec![layout(28, vec![pointer(0, Kind::String, None), pointer(8, Kind::Key, Some("Sounds"))])] };
        let repaired = repair_from_client(bytes, "t.datc64", &def, &client).map_err(|_| "no repair").unwrap();
        assert!(!repaired.after.is_broken(), "{}", repaired.after.summary());
        let report = repaired.report;
        assert_eq!((offset_of(&report, "Id"), offset_of(&report, "Family")), (Some(0), Some(24)));
        assert_eq!(report.lost, vec!["Text".to_string(), "A".to_string(), "B".to_string()]);
        let key = &report.table.columns[1];
        assert_eq!((key.r#type.as_str(), key.references.as_ref().map(|r| r.table.as_str())), ("foreignrow", Some("Sounds")));
    }

    #[test]
    fn a_pairing_that_ties_places_nothing() {
        let room = |starts: Vec<usize>| Room { before: &[0, 8], columns: &[0], ends: starts.iter().map(|s| s + 8).collect(), starts, start: 0, end: 32 };
        assert_eq!(line_up(1, 1, |_, _| true, &room(vec![8])), vec![(0, 0)]);
        assert!(line_up(1, 2, |_, _| true, &room(vec![8, 16])).is_empty());
        assert_eq!(line_up(1, 2, |_, b| b == 1, &room(vec![8, 16])), vec![(0, 1)]);
    }

    /// `cargo test --release client_repair_misplaces_nothing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn client_repair_misplaces_nothing() {
        use crate::dat::relational::FileSource;
        use crate::dat::schema::Schema;
        use crate::dat::table_store::{self, StoredTables};
        use crate::settings::{AppSettings, Game};
        use std::path::PathBuf;

        let path = AppSettings::load().schema_local_path.map(PathBuf::from).unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
        let schema: Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for game in [Game::Poe2, Game::Poe1] {
            let Some(version) = table_store::versions(game).into_iter().next() else { continue };
            let (Some(stored), Some(client)) = (StoredTables::open(game, &version), exe_layout::kept(game, &version)) else { continue };
            // Named columns kept in place, moved and lost: for the layout as it is, then with one string or list read as numbers.
            let (mut whole, mut damaged, mut restored, mut tables) = ([0usize; 3], [0usize; 3], 0, 0);
            for layout in client.tables.iter().filter(|l| l.walked) {
                let file = format!("{}{}.datc64", table_store::table_dir(game), layout.table.to_ascii_lowercase());
                let Some(def) = schema.find_table(&layout.table, game.is_poe2()) else { continue };
                let Some(bytes) = stored.fetch(&file) else { continue };
                let Ok(reader) = DatReader::new(bytes.clone(), &file) else { continue };
                let fits = reader.row_length == Some(layout.row_size) && reader.row_count >= 12;
                if !fits || analysis::check_fit(&reader, def, 40).is_broken() || !client.disagreements(def, layout).is_empty() {
                    continue;
                }
                tables += 1;
                let starts = offsets(def, true);
                let named: Vec<(&str, usize)> = def.columns.iter().zip(&starts).filter_map(|(c, at)| Some((c.name.as_deref()?, *at))).collect();
                let tally = |report: &RefitReport, counts: &mut [usize; 3], skip: Option<usize>| {
                    for (name, at) in named.iter().filter(|(_, at)| Some(*at) != skip) {
                        match offset_of(report, name) {
                            Some(found) if found == *at => counts[0] += 1,
                            Some(found) => {
                                counts[1] += 1;
                                println!("  {} {}: {} moved from byte {} to {}", game.label(), layout.table, name, at, found);
                            }
                            None => counts[2] += 1,
                        }
                    }
                };
                if let Ok(repaired) = repair_from_client(bytes.clone(), &file, def, &client) {
                    assert!(!repaired.after.is_broken(), "{}: {}", layout.table, repaired.after.summary());
                    tally(&repaired.report, &mut whole, None);
                }

                let Some(hit) = (1..def.columns.len()).find(|i| matches!(exe_layout::schema_kind(&def.columns[*i]), Some(Kind::String | Kind::List))) else { continue };
                let mut wrong = def.clone();
                let numbers = get_column_size(&def.columns[hit], true) / 4;
                wrong.columns.splice(hit..=hit, (0..numbers).map(|_| Column { name: None, ..col("", "i32", false, None) }));
                if let Ok(repaired) = repair_from_client(bytes, &file, &wrong, &client) {
                    assert!(!repaired.after.is_broken(), "{} with column {} read as numbers: {}", layout.table, hit, repaired.after.summary());
                    tally(&repaired.report, &mut damaged, Some(starts[hit]));
                    let at = offsets(&repaired.report.table, true).iter().position(|at| *at == starts[hit]);
                    restored += usize::from(at.is_some_and(|i| exe_layout::schema_kind(&repaired.report.table.columns[i]) == exe_layout::schema_kind(&def.columns[hit])));
                }
            }
            println!("{} {}: {} tables the schema and the client agree on", game.label(), version, tables);
            println!("  repairing a layout that is right: {} columns in place, {} moved, {} lost", whole[0], whole[1], whole[2]);
            println!("  with a string or list read as numbers: {} in place, {} moved, {} lost; the column came back in {} tables", damaged[0], damaged[1], damaged[2], restored);
            assert_eq!((whole[1], damaged[1]), (0, 0), "a repair must lose a column rather than move it");
        }
    }

    /// `cargo test --release client_anchors_improve_refits -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn client_anchors_improve_refits() {
        use crate::dat::relational::FileSource;
        use crate::dat::schema::Schema;
        use crate::dat::table_store::{self, StoredTables};
        use crate::settings::{AppSettings, Game};
        use std::path::{Path, PathBuf};

        let game = Game::Poe2;
        let settings = AppSettings::load();
        let version = table_store::versions(game).into_iter().next().expect("no kept patch; run export-data once");
        let stored = StoredTables::open(game, &version).unwrap();
        let exe = settings.ggpk_path_for(game).and_then(|p| Path::new(p).parent().and_then(exe_layout::client_binary));
        let client = ClientLayouts::read(&std::fs::read(exe.expect("no game client beside the configured GGPK")).unwrap()).unwrap();
        let path = settings.schema_local_path.map(PathBuf::from).unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
        let schema: Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

        // What the patch adds in the middle of each row, and the pointer column the client would read there.
        type Added = (&'static str, fn(usize) -> Vec<u8>, Option<Kind>);
        let added: [Added; 3] = [
            ("an all-zero number", |_| vec![0; 4], None),
            ("a number that differs in every row", |row| (row as u32).to_le_bytes().to_vec(), None),
            ("a key no row sets", |_| vec![0xFE; 16], Some(Kind::Key)),
        ];
        // Per addition: (right, wrong, lost) without the client, then with it; then the tables carried whole each way.
        let mut tally = [[0usize; 8]; 3];
        let mut tables = 0;
        for layout in client.tables.iter().filter(|l| l.walked) {
            let file = format!("{}{}.datc64", table_store::table_dir(game), layout.table.to_ascii_lowercase());
            let Some(def) = schema.find_table(&layout.table, true) else { continue };
            let Some(bytes) = stored.fetch(&file) else { continue };
            let Ok(old) = DatReader::new(bytes.clone(), &file) else { continue };
            let starts = offsets(def, true);
            let schema_len: usize = def.columns.iter().map(|c| get_column_size(c, true)).sum();
            let fits = old.row_length == Some(layout.row_size) && schema_len == layout.row_size && old.row_count >= 12;
            if !fits || def.columns.len() < 3 || analysis::check_fit(&old, def, 40).is_broken() || !client.disagreements(def, layout).is_empty() {
                continue;
            }
            tables += 1;
            let insert_at = starts[def.columns.len() / 2];
            let (rows, row_len) = (old.row_count as usize, layout.row_size);
            for ((_, bytes_for, kind), tally) in added.iter().zip(tally.iter_mut()) {
                let width = bytes_for(0).len();
                let after = |offset: usize| offset + if offset >= insert_at { width } else { 0 };
                let mut pointers: Vec<Pointer> = layout.pointers.iter().map(|p| Pointer { offset: after(p.offset), ..p.clone() }).collect();
                pointers.extend(kind.map(|kind| Pointer { offset: insert_at, kind, element: None, target: None, proven: false }));
                pointers.sort_by_key(|p| p.offset);
                let moved = TableLayout { row_size: row_len + width, pointers, ..layout.clone() };

                let mut data = bytes[..4].to_vec();
                for r in 0..rows {
                    let row = &bytes[4 + r * row_len..4 + (r + 1) * row_len];
                    data.extend_from_slice(&row[..insert_at]);
                    data.extend_from_slice(&bytes_for(r));
                    data.extend_from_slice(&row[insert_at..]);
                }
                data.extend_from_slice(&bytes[4 + rows * row_len..]);
                let new = DatReader::new(data, &file).unwrap();
                for (side, anchors) in [None, Some(&moved)].into_iter().enumerate() {
                    let named = def.columns.iter().filter(|c| c.name.is_some()).count();
                    let Ok(report) = carry_across_patch(&old, def, &new, anchors) else {
                        tally[side * 3 + 2] += named;
                        continue;
                    };
                    for column in &report.carried {
                        tally[side * 3 + usize::from(column.new_offset != after(column.old_offset))] += 1;
                    }
                    tally[side * 3 + 2] += report.lost.len();
                    let whole = report.lost.is_empty() && report.carried.iter().all(|c| c.new_offset == after(c.old_offset));
                    tally[6 + side] += usize::from(whole && !analysis::check_fit(&new, &report.table, 40).is_broken());
                }
            }
        }
        println!("{} tables, a column added in the middle of each", tables);
        for ((what, _, _), t) in added.iter().zip(&tally) {
            println!("  {}: without the client {} right, {} wrong, {} lost; with it {} right, {} wrong, {} lost", what, t[0], t[1], t[2], t[3], t[4], t[5]);
            println!("    tables carried whole and reading cleanly: {} without the client, {} with it", t[6], t[7]);
            assert!(t[3] >= t[0] && t[4] <= t[1] && t[7] >= t[6], "the client's layout must place more columns and misplace no more");
        }
    }
}
