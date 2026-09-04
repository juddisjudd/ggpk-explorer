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
fn find_id_offset(old: &DatReader, old_id_offset: usize, new: &DatReader) -> Option<usize> {
    let known: std::collections::HashSet<String> =
        ids_at(old, old_id_offset, 400).into_iter().map(|(id, _)| id).collect();
    if known.is_empty() {
        return None;
    }
    let row_len = new.row_length.unwrap_or(0);
    let step = if new.is_64bit { 8 } else { 4 };
    (0..row_len.saturating_sub(step))
        .step_by(step)
        .map(|offset| {
            let hits = ids_at(new, offset, 400).into_iter().filter(|(id, _)| known.contains(id)).count();
            (hits, offset)
        })
        .filter(|(hits, _)| *hits >= 8)
        .max_by_key(|(hits, _)| *hits)
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

/// Re-derives `old_def`'s layout against a newer copy of the same file.
pub fn carry_across_patch(
    old: &DatReader,
    old_def: &Table,
    new: &DatReader,
) -> Result<RefitReport, String> {
    let old_row_len = old.row_length.ok_or("the old file has no fixed row length")?;
    let new_row_len = new.row_length.ok_or("the new file has no fixed row length")?;
    let old_offsets = offsets(old_def, old.is_64bit);

    let id_index = old_def
        .columns
        .iter()
        .position(|c| c.name.as_deref() == Some("Id"))
        .or_else(|| old_def.columns.iter().position(|c| c.unique && c.r#type == "string"))
        .ok_or("the old schema has no Id column to match rows on")?;
    let old_id_offset = old_offsets[id_index];
    let new_id_offset = find_id_offset(old, old_id_offset, new)
        .ok_or("could not find the id column in the new file")?;

    let pairs = pair_rows(old, old_id_offset, new, new_id_offset);
    if pairs.len() < 8 {
        return Err(match pairs.len() {
            1 => "only 1 row appears in both patches — too few to compare".to_string(),
            n => format!("only {} rows appear in both patches — too few to compare", n),
        });
    }

    // Where each named schema column ended up, in schema order.
    let mut placed: Vec<Option<CarriedColumn>> = vec![None; old_def.columns.len()];
    let mut floor = 0usize;

    for (index, col) in old_def.columns.iter().enumerate() {
        let Some(name) = col.name.clone() else { continue };
        let size = get_column_size(col, old.is_64bit);
        if index == id_index {
            placed[index] = Some(CarriedColumn {
                name,
                old_offset: old_id_offset,
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

    let sample_rows: Vec<u32> = pairs.iter().map(|&(_, new_row)| new_row).take(40).collect();
    fill_gaps(old_def, new, &old_offsets, &sample_rows, changed_window(old, new), &mut placed);

    let mut carried: Vec<CarriedColumn> = Vec::new();
    let mut lost: Vec<String> = Vec::new();
    for (index, col) in old_def.columns.iter().enumerate() {
        let Some(name) = col.name.clone() else { continue };
        match placed[index].take() {
            Some(column) => carried.push(column),
            None => lost.push(name),
        }
    }

    let table = assemble(old_def, new, &carried);
    Ok(RefitReport {
        table,
        carried,
        lost,
        matched_rows: pairs.len(),
        old_row_len,
        new_row_len,
    })
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
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
        .min(after.len().saturating_sub(prefix));
    let lo: usize = after[..prefix].iter().sum();
    let hi: usize = after[..after.len() - suffix].iter().sum();
    Some((lo.min(hi), hi.max(lo)))
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
/// Places the columns the value search could not claim. Between two columns it
/// did place, the bytes left over are the unplaced columns' own — but only when
/// they add up exactly. Anything else is left unplaced rather than guessed at.
fn fill_gaps(
    old_def: &Table,
    new: &DatReader,
    old_offsets: &[usize],
    rows: &[u32],
    window: Option<(usize, usize)>,
    placed: &mut [Option<CarriedColumn>],
) {
    let is_64bit = new.is_64bit;
    let new_row_len = new.row_length.unwrap_or(0);
    let size_of = |index: usize| get_column_size(&old_def.columns[index], is_64bit);
    // Every column counts towards the arithmetic, named or not: the schema's
    // unnamed padding takes up bytes just the same, and a span that leaves it
    // out never balances.
    let named: Vec<usize> = (0..old_def.columns.len()).collect();
    let anchors: Vec<usize> = named.iter().copied().filter(|i| placed[*i].is_some()).collect();
    if anchors.is_empty() {
        return;
    }

    let boundaries: std::collections::HashSet<usize> =
        analysis::analyze(new).iter().map(|g| g.offset).collect();

    // Each run of unplaced columns, with the byte span it has to fit into.
    let mut runs: Vec<(Vec<usize>, usize, usize)> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut span_start = 0usize;
    for &index in &named {
        match &placed[index] {
            Some(column) => {
                if !run.is_empty() {
                    runs.push((std::mem::take(&mut run), span_start, column.new_offset));
                }
                span_start = column.new_offset + size_of(index);
            }
            None => run.push(index),
        }
    }
    if !run.is_empty() {
        runs.push((run, span_start, new_row_len));
    }

    for (indices, start, end) in runs {
        let needed: usize = indices.iter().map(|i| size_of(*i)).sum();
        let span = end.saturating_sub(start);
        if needed > span {
            continue;
        }
        // The patch added `span - needed` bytes somewhere inside this run. Each
        // split puts them at a different point; the right one is the split the
        // file can be read through, and failing that the one whose columns start
        // where the file's own layout says columns start.
        let mut candidates: Vec<((usize, usize, usize), usize)> = Vec::new();
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
            candidates.push(((impossible, adrift, strays), split));
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
        if best.0 > 0 || contested {
            continue;
        }

        for (position, index) in indices.iter().enumerate() {
            placed[*index] = Some(CarriedColumn {
                name: old_def.columns[*index].name.clone().unwrap_or_default(),
                old_offset: old_offsets[*index],
                new_offset: place_run(&indices, start, end, split, &size_of)[position],
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
fn assemble(old_def: &Table, new: &DatReader, carried: &[CarriedColumn]) -> Table {
    let row_len = new.row_length.unwrap_or(0);
    let by_name: HashMap<&str, &Column> = old_def
        .columns
        .iter()
        .filter_map(|c| c.name.as_deref().map(|n| (n, c)))
        .collect();
    let placed: HashMap<usize, &CarriedColumn> =
        carried.iter().map(|c| (c.new_offset, c)).collect();
    let guessed = analysis::analyze(new);
    let guessed_at: HashMap<usize, usize> =
        guessed.iter().enumerate().map(|(i, g)| (g.offset, i)).collect();

    let mut columns = Vec::new();
    let mut at = 0usize;
    while at < row_len {
        if let Some(hit) = placed.get(&at) {
            if let Some(col) = by_name.get(hit.name.as_str()) {
                let size = get_column_size(col, new.is_64bit);
                columns.push((*col).clone());
                at += size;
                continue;
            }
        }
        // An unclaimed stretch: take the file's own reading of it where that
        // fits, and pad a byte at a time where it does not.
        let next_placed = placed.keys().filter(|o| **o > at).min().copied().unwrap_or(row_len);
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
        let report = carry_across_patch(&old, &def, &new).unwrap();
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
        let report = carry_across_patch(&old, &def, &new).unwrap();
        let total: usize =
            report.table.columns.iter().map(|c| get_column_size(c, true)).sum();
        assert_eq!(total, 24, "layout must tile the 24-byte row");
    }
}
