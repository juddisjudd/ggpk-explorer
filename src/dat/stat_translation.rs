use crate::dat::csd::{CsdEntry, CsdFile, CsdSubEntry};
use std::collections::HashMap;

/// Resolves (stat id, numeric value) pairs from a DAT row into the rendered
/// text a `.csd` stat-description file specifies for that value.
pub struct TranslationLookup {
    /// Each entry as `(the file it came from, its position in that file)`.
    /// Hundreds of skill-specific files share the same two large includes, so
    /// the entries are pointed at rather than copied.
    entries: Vec<(std::rc::Rc<CsdFile>, usize)>,
    /// Entries mentioning each stat id, for subset matching.
    by_id: HashMap<String, Vec<usize>>,
    /// Entries redefined by a later file/entry with the same id set. Files
    /// are given generic first (`stat_descriptions.csd`) and tree-specific
    /// last, so the specific text wins.
    superseded: Vec<bool>,
}

impl TranslationLookup {
    pub fn build(csd_files: &[&CsdFile]) -> Self {
        let owned: Vec<std::rc::Rc<CsdFile>> =
            csd_files.iter().map(|f| std::rc::Rc::new((*f).clone())).collect();
        Self::build_shared(&owned)
    }

    /// Builds from files the caller already holds, sharing them rather than
    /// copying their entries.
    pub fn build_shared(csd_files: &[std::rc::Rc<CsdFile>]) -> Self {
        let mut entries: Vec<(std::rc::Rc<CsdFile>, usize)> = Vec::new();
        let mut by_id: HashMap<String, Vec<usize>> = HashMap::new();
        // Last entry to claim each set of ids; earlier ones are superseded.
        let mut last: HashMap<&[String], usize> = HashMap::new();
        for file in csd_files {
            for (position, entry) in file.entries.iter().enumerate() {
                if entry.ids.is_empty() {
                    continue;
                }
                let idx = entries.len();
                for id in &entry.ids {
                    by_id.entry(id.clone()).or_default().push(idx);
                }
                last.insert(entry.ids.as_slice(), idx);
                entries.push((std::rc::Rc::clone(file), position));
            }
        }
        let mut superseded = Vec::with_capacity(entries.len());
        for file in csd_files {
            for entry in file.entries.iter().filter(|e| !e.ids.is_empty()) {
                let i = superseded.len();
                superseded.push(last.get(entry.ids.as_slice()) != Some(&i));
            }
        }
        Self { entries, by_id, superseded }
    }

    fn entry(&self, index: usize) -> &CsdEntry {
        let (file, position) = &self.entries[index];
        &file.entries[*position]
    }

    /// Renders a node's stats the way the client does. Every entry that
    /// mentions one of the stats is a candidate; the one covering the most
    /// of them wins (then the most specific, then the latest definition),
    /// stats it names that the node lacks count as zero, and the lines come
    /// out in description-file order. Each string is one description, which
    /// may span several lines.
    pub fn translate_grouped(&self, stat_ids: &[String], values: &[i32]) -> Vec<String> {
        let ranges: Vec<(i32, i32)> = values.iter().map(|&v| (v, v)).collect();
        self.resolve(stat_ids, &ranges, Order::File).into_iter().map(|line| line.text).filter(|t| !t.is_empty()).collect()
    }

    /// Like [`translate_grouped`](Self::translate_grouped) but every stat
    /// carries a `(min, max)` range, which renders as `(min-max)` the way the
    /// text on an unrolled mod does. Lines follow the order the stats were
    /// given in, which is how a mod lists them.
    pub fn translate_ranges(&self, stat_ids: &[String], values: &[(i32, i32)]) -> Vec<String> {
        self.resolve(stat_ids, values, Order::Stat).into_iter().map(|line| line.text).filter(|t| !t.is_empty()).collect()
    }

    /// Like [`translate_grouped`](Self::translate_grouped) but reporting which
    /// stats each line consumed and where its description sits in the file, so
    /// a caller can key lines by stat and order them the way a tooltip does.
    pub fn translate_detailed(&self, stat_ids: &[String], values: &[i32]) -> Vec<Line> {
        let ranges: Vec<(i32, i32)> = values.iter().map(|&v| (v, v)).collect();
        self.resolve(stat_ids, &ranges, Order::File)
    }

    fn resolve(&self, stat_ids: &[String], values: &[(i32, i32)], order: Order) -> Vec<Line> {
        let position = |id: &str| stat_ids.iter().position(|s| s == id).unwrap_or(usize::MAX);
        let mut remaining: Vec<(String, (i32, i32))> = stat_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), values.get(i).copied().unwrap_or((0, 0))))
            .collect();
        let mut out: Vec<(usize, usize, Line)> = Vec::new();
        while !remaining.is_empty() {
            let mut best: Option<(usize, usize)> = None; // (entry, covered)
            for (id, _) in &remaining {
                for &idx in self.by_id.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if self.superseded[idx] {
                        continue;
                    }
                    let entry = self.entry(idx);
                    let covered = entry.ids.iter().filter(|e| remaining.iter().any(|(r, _)| r == *e)).count();
                    let better = match best {
                        None => true,
                        Some((b, bc)) => {
                            let be = self.entry(b);
                            covered > bc || (covered == bc && (entry.ids.len() < be.ids.len() || (entry.ids.len() == be.ids.len() && idx > b)))
                        }
                    };
                    if better {
                        best = Some((idx, covered));
                    }
                }
            }
            let Some((idx, _)) = best else {
                remaining.remove(0);
                continue;
            };
            let entry = self.entry(idx);
            let vals: Vec<(i32, i32)> = entry
                .ids
                .iter()
                .map(|e| remaining.iter().find(|(r, _)| r == e).map(|(_, v)| *v).unwrap_or((0, 0)))
                .collect();
            // A described stat stays described even when none of the entry's
            // conditions accept its numbers; it just has nothing to say.
            if !entry.descriptions.is_empty() {
                let sub = matching_sub(entry, &vals);
                let first = entry.ids.iter().map(|id| position(id)).min().unwrap_or(usize::MAX);
                out.push((
                    idx,
                    first,
                    Line {
                        ids: entry.ids.clone(),
                        text: sub.map(|s| render(s, &vals)).unwrap_or_default(),
                        template: sub.map(|s| template(s, &entry.ids)).unwrap_or_default(),
                        index: idx,
                    },
                ));
            }
            remaining.retain(|(r, _)| !entry.ids.contains(r));
        }
        match order {
            Order::File => out.sort_by_key(|(idx, _, _)| *idx),
            Order::Stat => out.sort_by_key(|(_, first, _)| *first),
        }
        out.into_iter().map(|(_, _, line)| line).collect()
    }
}

/// One rendered description line.
pub struct Line {
    /// Every stat id the description covers, in its own order.
    pub ids: Vec<String>,
    pub text: String,
    /// The line with each slot written as `{stat_id/handler}` rather than a
    /// number, which is how quality effects are described.
    pub template: String,
    /// Position of the description in the file, for tooltip ordering.
    pub index: usize,
}

/// Which order rendered lines come out in.
#[derive(Clone, Copy, PartialEq)]
enum Order {
    /// As the description file lists them — what the passive tree shows.
    File,
    /// As the caller listed the stats — what mod and skill text does.
    Stat,
}

/// The first sub-entry whose conditions the values can satisfy. Nothing
/// matches when none of them can — a stat sitting at zero in a "more or less"
/// pair contributes no line at all.
fn matching_sub<'e>(entry: &'e CsdEntry, values: &[(i32, i32)]) -> Option<&'e CsdSubEntry> {
    if entry.descriptions.is_empty() {
        return None; // no_description: intentionally hidden stat
    }
    let n = entry.ids.len();
    entry
        .descriptions
        .iter()
        .filter(|s| s.language.is_none() && !is_variant(&s.operator))
        .filter(|s| ranges_match(&s.operator, n, values))
        // Several lines can accept the same numbers; the one that says most
        // about them wins, which is how the client picks between "Fires 8" and
        // "Fires +8". Equally specific lines are settled by which comes first,
        // so `1|#` beats the `-1|#` that follows it for a positive value.
        .fold(None, |best: Option<&CsdSubEntry>, sub| match best {
            Some(b) if specificity(&b.operator, n) >= specificity(&sub.operator, n) => Some(b),
            _ => Some(sub),
        })
}

/// How tightly a set of conditions pins its values down: an exact number says
/// more than a half-open range, which says more than `#`.
fn specificity(operator: &str, n: usize) -> usize {
    let tokens: Vec<&str> = operator.split_whitespace().filter(|t| is_range_token(t)).collect();
    (0..n)
        .map(|i| {
            let token = tokens.get(i).copied().unwrap_or("#").trim_start_matches('!');
            let (min, max) = token.split_once('|').unwrap_or((token, token));
            1 + (min != "#") as usize + (max != "#") as usize
        })
        .sum()
}

/// A word among the conditions (`table_only`, `gem_quality`, …) marks wording
/// meant for one particular screen. The plain line is the one with conditions
/// alone, so a flagged variant is never the description of a stat.
fn is_variant(operator: &str) -> bool {
    operator.split_whitespace().any(|token| !is_range_token(token))
}

fn is_range_token(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit() || matches!(c, '#' | '|' | '!' | '-'))
}

/// The description with each value slot replaced by the stat it shows and the
/// handlers applied to it, e.g. `{support_damage_+%_final/divide_by_one_hundred}`.
fn template(sub: &CsdSubEntry, ids: &[String]) -> String {
    let mut out = String::with_capacity(sub.description.len());
    let chars: Vec<char> = sub.description.chars().collect();
    let mut i = 0;
    let mut anonymous = 0usize;
    while i < chars.len() {
        if chars[i] == '{' && i.checked_sub(1).map(|p| chars[p]) != Some('{') {
            if let Some(rel) = chars[i..].iter().position(|&c| c == '}') {
                let token: String = chars[i + 1..i + rel].iter().collect();
                let index_part = token.split(':').next().unwrap_or("");
                let index = if index_part.is_empty() {
                    anonymous += 1;
                    Some(anonymous - 1)
                } else {
                    index_part.parse::<usize>().ok()
                };
                if let Some(index) = index {
                    let mut parts = vec![ids.get(index).cloned().unwrap_or_default()];
                    parts.extend(
                        sub.parameters
                            .iter()
                            .filter(|p| p.value as usize == index + 1)
                            .map(|p| p.name.clone()),
                    );
                    out.push('{');
                    out.push_str(&parts.join("/"));
                    out.push('}');
                    i += rel + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Text one entry renders for `values` in `language` (`None` = base/English): the first
/// sub-entry of that language whose ranges match, falling back to the base language.
pub fn preview(entry: &CsdEntry, language: Option<&str>, values: &[i32]) -> Option<String> {
    let n = entry.ids.len().max(1);
    let values: Vec<(i32, i32)> = values.iter().map(|&v| (v, v)).collect();
    let pick = |lang: Option<&str>| {
        entry
            .descriptions
            .iter()
            .find(|s| s.language.as_deref() == lang && ranges_match(&s.operator, n, &values))
            .map(|s| render(s, &values))
    };
    pick(language).or_else(|| if language.is_some() { pick(None) } else { None })
}

/// Whether a description's conditions accept these values. A stat that rolls
/// a range is judged on the top of that range, so a mod rolling `1 to 3` reads
/// as the plural line rather than the singular one its floor would pick.
fn ranges_match(operator: &str, n: usize, values: &[(i32, i32)]) -> bool {
    let tokens: Vec<&str> = operator.split_whitespace().filter(|t| is_range_token(t)).collect();
    (0..n).all(|i| {
        let (_, top) = values.get(i).copied().unwrap_or((0, 0));
        condition_holds(tokens.get(i).copied().unwrap_or("#|#"), top)
    })
}

/// `#` accepts anything, `a|b` a span with either end open, a bare number an
/// exact value, and a leading `!` inverts the whole test.
fn condition_holds(token: &str, value: i32) -> bool {
    if let Some(rest) = token.strip_prefix('!') {
        return !condition_holds(rest, value);
    }
    let (min_s, max_s) = token.split_once('|').unwrap_or((token, token));
    let min = if min_s == "#" { i32::MIN } else { min_s.parse().unwrap_or(i32::MIN) };
    let max = if max_s == "#" { i32::MAX } else { max_s.parse().unwrap_or(i32::MAX) };
    value >= min && value <= max
}

/// A value being rendered. Mods carry a min/max range rather than a single
/// number, and a range whose ends differ prints as `(min-max)`.
#[derive(Clone, Copy)]
struct ValueFmt {
    min: f64,
    max: f64,
    /// `None` prints the value as short as it goes, which is what a handler
    /// with no stated precision does.
    decimals: Option<usize>,
    trim: bool,
}

impl ValueFmt {
    fn from_range(min: i32, max: i32) -> Self {
        Self { min: min as f64, max: max as f64, decimals: None, trim: false }
    }

    /// The end of the range a `+d` sign is decided on: a roll that can reach
    /// a positive number is written with a leading plus.
    fn value(&self) -> f64 {
        self.max
    }

    fn one(&self, v: f64) -> String {
        let Some(decimals) = self.decimals else {
            return if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{}", v) };
        };
        let s = format!("{:.*}", decimals, v);
        if self.trim && s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }

    fn format(&self) -> String {
        if self.min == self.max {
            return self.one(self.min);
        }
        // A range that is negative throughout shows the sign once, in front.
        if self.min < 0.0 && self.max < 0.0 {
            return format!("-({}-{})", self.one(-self.min), self.one(-self.max));
        }
        format!("({}-{})", self.one(self.min), self.one(self.max))
    }
}

/// Applies a named handler to both ends of a range. The ends keep the slots
/// they were stored in, so `negate` on `-40 to -36` reads `(40-36)`, exactly
/// as the client shows it.
fn apply_function(name: &str, fmt: ValueFmt) -> ValueFmt {
    let Some(handler) = crate::dat::stat_handlers::lookup(name) else { return fmt };
    let (precision, fixed) = crate::dat::stat_handlers::precision(&handler.kind);
    ValueFmt {
        min: crate::dat::stat_handlers::apply(&handler.kind, fmt.min),
        max: crate::dat::stat_handlers::apply(&handler.kind, fmt.max),
        decimals: precision.map(|p| p as usize).or(fmt.decimals),
        trim: !fixed,
    }
}

fn render(sub: &CsdSubEntry, raw_values: &[(i32, i32)]) -> String {
    let mut fmts: Vec<ValueFmt> = raw_values.iter().map(|&(a, b)| ValueFmt::from_range(a, b)).collect();
    for param in &sub.parameters {
        let idx = (param.value as usize).saturating_sub(1);
        if let Some(f) = fmts.get_mut(idx) {
            *f = apply_function(&param.name, *f);
        }
    }
    substitute(&sub.description, &fmts)
}

fn substitute(template: &str, values: &[ValueFmt]) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut anonymous = 0usize;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(rel_end) = chars[i..].iter().position(|&c| c == '}') {
                let end = i + rel_end;
                let token: String = chars[i + 1..end].iter().collect();
                let (idx_str, spec) = match token.split_once(':') {
                    Some((a, b)) => (a, Some(b)),
                    None => (token.as_str(), None),
                };
                // `{}` takes values in order of appearance.
                let idx = if idx_str.is_empty() {
                    anonymous += 1;
                    Ok(anonymous - 1)
                } else {
                    idx_str.parse::<usize>()
                };
                if let Ok(idx) = idx {
                    if let Some(v) = values.get(idx) {
                        let mut s = v.format();
                        if spec == Some("+d") && v.value() >= 0.0 {
                            s = format!("+{}", s);
                        }
                        out.push_str(&s);
                        i = end + 1;
                        continue;
                    }
                }
                out.push('{');
                out.push_str(&token);
                out.push('}');
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::csd::parse_csd;

    impl TranslationLookup {
        /// All lines for `stat_ids` joined, or `None` when nothing renders.
        fn translate(&self, stat_ids: &[String], values: &[i32]) -> Option<String> {
            let lines = self.translate_grouped(stat_ids, values);
            (!lines.is_empty()).then(|| lines.join("
"))
        }
    }

    fn parse(text: &str) -> CsdFile {
        let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        parse_csd(&bytes, "test.csd").expect("parse failed")
    }

    #[test]
    fn wildcard_single_value() {
        let file = parse(
            "description\n\t1 test_stat\n\t1\n\t\t# \"You gain {0} Life\"",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["test_stat".to_string()], &[50]),
            Some("You gain 50 Life".to_string())
        );
    }

    #[test]
    fn negate_and_range_selection() {
        let file = parse(
            "description\n\t1 test_stat_neg\n\t2\n\t\t1|# \"{0}% increased Pack Size\"\n\t\t#|-1 \"{0}% reduced Pack Size\" negate 1",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["test_stat_neg".to_string()], &[20]),
            Some("20% increased Pack Size".to_string())
        );
        assert_eq!(
            lookup.translate(&["test_stat_neg".to_string()], &[-30]),
            Some("30% reduced Pack Size".to_string())
        );
    }

    #[test]
    fn signed_format_spec() {
        let file = parse(
            "description\n\t1 test_stat_signed\n\t1\n\t\t# \"{0:+d}% chance to contain an Abyss\"",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["test_stat_signed".to_string()], &[12]),
            Some("+12% chance to contain an Abyss".to_string())
        );
        assert_eq!(
            lookup.translate(&["test_stat_signed".to_string()], &[-5]),
            Some("-5% chance to contain an Abyss".to_string())
        );
    }

    #[test]
    fn combined_multi_id_entry() {
        let file = parse(
            "description\n\t2 stat_a stat_b\n\t1\n\t\t1|# 1|# \"Stat A {0} and Stat B {1}\"",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["stat_a".to_string(), "stat_b".to_string()], &[3, 4]),
            Some("Stat A 3 and Stat B 4".to_string())
        );
    }

    #[test]
    fn grouped_translation_prefers_largest_matching_entry() {
        let file = parse(
            "description\n\t2 stat_a stat_b\n\t1\n\t\t# # \"A {0} B {1}\"\ndescription\n\t1 stat_c\n\t1\n\t\t# \"C {0}\"\ndescription\n\t1 stat_a\n\t1\n\t\t# \"A alone {0}\"\ndescription\n\t2 mode count\n\t2\n\t\t0 1 \"one\"\n\t\t0 2|# \"{1} of them\"",
        );
        let lookup = TranslationLookup::build(&[&file]);
        let ids = ["stat_c".to_string(), "stat_b".to_string(), "stat_a".to_string()];
        assert_eq!(lookup.translate_grouped(&ids, &[3, 2, 1]), vec!["A 1 B 2".to_string(), "C 3".to_string()]);
        // Output follows description-file order, whatever the stat order.
        let single = ["stat_a".to_string(), "stat_c".to_string()];
        assert_eq!(lookup.translate_grouped(&single, &[7, 3]), vec!["C 3".to_string(), "A alone 7".to_string()]);
        // Ids the node lacks count as zero.
        assert_eq!(lookup.translate_grouped(&["count".to_string()], &[1]), vec!["one".to_string()]);
        assert_eq!(lookup.translate_grouped(&["count".to_string()], &[3]), vec!["3 of them".to_string()]);
        // A later file redefining the same ids wins.
        let specific = parse("description\n\t1 stat_c\n\t1\n\t\t# \"Specific C {0}\"");
        let lookup = TranslationLookup::build(&[&file, &specific]);
        assert_eq!(lookup.translate_grouped(&["stat_c".to_string()], &[3]), vec!["Specific C 3".to_string()]);
    }

    #[test]
    fn no_description_is_suppressed() {
        let file = parse("no_description hidden_stat");
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(lookup.translate(&["hidden_stat".to_string()], &[5]), None);
    }

    #[test]
    fn falls_back_to_per_id_when_no_combined_entry() {
        let file = parse(
            "description\n\t1 stat_x\n\t1\n\t\t# \"X is {0}\"\n\ndescription\n\t1 stat_y\n\t1\n\t\t# \"Y is {0}\"",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["stat_x".to_string(), "stat_y".to_string()], &[1, 2]),
            Some("X is 1\nY is 2".to_string())
        );
    }

    #[test]
    fn divide_by_one_hundred_trims_trailing_zeros() {
        let file = parse(
            "description\n\t1 test_pct\n\t1\n\t\t# \"{0}% increased Effect\" divide_by_one_hundred_2dp_if_required 1",
        );
        let lookup = TranslationLookup::build(&[&file]);
        assert_eq!(
            lookup.translate(&["test_pct".to_string()], &[1250]),
            Some("12.5% increased Effect".to_string())
        );
        assert_eq!(
            lookup.translate(&["test_pct".to_string()], &[1200]),
            Some("12% increased Effect".to_string())
        );
    }
}
