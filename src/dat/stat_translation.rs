use crate::dat::csd::{CsdEntry, CsdFile, CsdSubEntry};
use std::collections::HashMap;

/// Resolves (stat id, numeric value) pairs from a DAT row into the rendered
/// text a `.csd` stat-description file specifies for that value.
pub struct TranslationLookup {
    entries: Vec<CsdEntry>,
    /// Entries mentioning each stat id, for subset matching.
    by_id: HashMap<String, Vec<usize>>,
    /// Entries redefined by a later file/entry with the same id set. Files
    /// are given generic first (`stat_descriptions.csd`) and tree-specific
    /// last, so the specific text wins.
    superseded: Vec<bool>,
}

impl TranslationLookup {
    pub fn build(csd_files: &[&CsdFile]) -> Self {
        let mut entries = Vec::new();
        let mut index: HashMap<Vec<String>, usize> = HashMap::new();
        let mut by_id: HashMap<String, Vec<usize>> = HashMap::new();
        for file in csd_files {
            for entry in &file.entries {
                if entry.ids.is_empty() {
                    continue;
                }
                let idx = entries.len();
                index.insert(entry.ids.clone(), idx);
                for id in &entry.ids {
                    by_id.entry(id.clone()).or_default().push(idx);
                }
                entries.push(entry.clone());
            }
        }
        let superseded = entries.iter().enumerate().map(|(i, e)| index.get(&e.ids) != Some(&i)).collect();
        Self { entries, by_id, superseded }
    }

    /// Renders a node's stats the way the client does. Every entry that
    /// mentions one of the stats is a candidate; the one covering the most
    /// of them wins (then the most specific, then the latest definition),
    /// stats it names that the node lacks count as zero, and the lines come
    /// out in description-file order. Each string is one description, which
    /// may span several lines.
    pub fn translate_grouped(&self, stat_ids: &[String], values: &[i32]) -> Vec<String> {
        let mut remaining: Vec<(String, i32)> = stat_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), values.get(i).copied().unwrap_or(0)))
            .collect();
        let mut out: Vec<(usize, String)> = Vec::new();
        while !remaining.is_empty() {
            let mut best: Option<(usize, usize)> = None; // (entry, covered)
            for (id, _) in &remaining {
                for &idx in self.by_id.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if self.superseded[idx] {
                        continue;
                    }
                    let entry = &self.entries[idx];
                    let covered = entry.ids.iter().filter(|e| remaining.iter().any(|(r, _)| r == *e)).count();
                    let better = match best {
                        None => true,
                        Some((b, bc)) => {
                            let be = &self.entries[b];
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
            let entry = &self.entries[idx];
            let vals: Vec<i32> = entry
                .ids
                .iter()
                .map(|e| remaining.iter().find(|(r, _)| r == e).map(|(_, v)| *v).unwrap_or(0))
                .collect();
            if let Some(text) = render_entry(entry, &vals) {
                out.push((idx, text));
            }
            remaining.retain(|(r, _)| !entry.ids.contains(r));
        }
        out.sort_by_key(|(idx, _)| *idx);
        out.into_iter().map(|(_, text)| text).collect()
    }

}

/// Base-language text of the first sub-entry whose ranges accept `values`.
fn render_entry(entry: &CsdEntry, values: &[i32]) -> Option<String> {
    if entry.descriptions.is_empty() {
        return None; // no_description: intentionally hidden stat
    }
    for sub in &entry.descriptions {
        if sub.language.is_some() {
            continue; // v1: base/English text only
        }
        if ranges_match(&sub.operator, entry.ids.len(), values) {
            return Some(render(sub, values));
        }
    }
    None
}

/// Text one entry renders for `values` in `language` (`None` = base/English): the first
/// sub-entry of that language whose ranges match, falling back to the base language.
pub fn preview(entry: &CsdEntry, language: Option<&str>, values: &[i32]) -> Option<String> {
    let n = entry.ids.len().max(1);
    let pick = |lang: Option<&str>| {
        entry
            .descriptions
            .iter()
            .find(|s| s.language.as_deref() == lang && ranges_match(&s.operator, n, values))
            .map(|s| render(s, values))
    };
    pick(language).or_else(|| if language.is_some() { pick(None) } else { None })
}

fn ranges_match(operator: &str, n: usize, values: &[i32]) -> bool {
    let tokens: Vec<&str> = operator.split_whitespace().collect();
    for i in 0..n {
        let val = values.get(i).copied().unwrap_or(0);
        let token = tokens.get(i).copied().unwrap_or("#|#");
        if !range_matches_one(token, val) {
            return false;
        }
    }
    true
}

fn range_matches_one(token: &str, val: i32) -> bool {
    let (min_s, max_s) = token.split_once('|').unwrap_or((token, token));
    let min = if min_s == "#" { i32::MIN } else { min_s.parse().unwrap_or(i32::MIN) };
    let max = if max_s == "#" { i32::MAX } else { max_s.parse().unwrap_or(i32::MAX) };
    val >= min && val <= max
}

#[derive(Clone, Copy)]
struct ValueFmt {
    value: f64,
    decimals: usize,
    trim: bool,
}

impl ValueFmt {
    fn from_i32(v: i32) -> Self {
        Self { value: v as f64, decimals: 0, trim: false }
    }

    fn format(&self) -> String {
        if self.decimals == 0 {
            format!("{}", self.value.round() as i64)
        } else {
            let s = format!("{:.*}", self.decimals, self.value);
            if self.trim {
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                s
            }
        }
    }
}

fn apply_function(name: &str, mut fmt: ValueFmt) -> ValueFmt {
    match name {
        "negate" => {
            fmt.value = -fmt.value;
        }
        "subtract_one" => {
            fmt.value -= 1.0;
        }
        "multiply_by_ten" => {
            fmt.value *= 10.0;
        }
        "divide_by_ten_1dp" => {
            fmt.value /= 10.0;
            fmt.decimals = 1;
            fmt.trim = false;
        }
        "divide_by_ten_1dp_if_required" => {
            fmt.value /= 10.0;
            fmt.decimals = 1;
            fmt.trim = true;
        }
        "divide_by_one_hundred" => {
            fmt.value /= 100.0;
            fmt.decimals = 2;
            fmt.trim = true;
        }
        "divide_by_one_hundred_0dp" => {
            fmt.value /= 100.0;
            fmt.decimals = 0;
            fmt.trim = false;
        }
        "divide_by_one_hundred_1dp" => {
            fmt.value /= 100.0;
            fmt.decimals = 1;
            fmt.trim = false;
        }
        "divide_by_one_hundred_2dp_if_required" => {
            fmt.value /= 100.0;
            fmt.decimals = 2;
            fmt.trim = true;
        }
        "milliseconds_to_seconds_0dp" => {
            fmt.value /= 1000.0;
            fmt.decimals = 0;
            fmt.trim = false;
        }
        "milliseconds_to_seconds_2dp_if_required" => {
            fmt.value /= 1000.0;
            fmt.decimals = 2;
            fmt.trim = true;
        }
        "milliseconds_to_seconds" | "milliseconds_to_seconds_2dp" => {
            fmt.value /= 1000.0;
            fmt.decimals = 2;
            fmt.trim = name == "milliseconds_to_seconds";
        }
        "milliseconds_to_seconds_1dp" => {
            fmt.value /= 1000.0;
            fmt.decimals = 1;
            fmt.trim = false;
        }
        "deciseconds_to_seconds" => {
            fmt.value /= 10.0;
            fmt.decimals = 1;
            fmt.trim = true;
        }
        "per_minute_to_per_second" | "per_minute_to_per_second_0dp" => {
            fmt.value /= 60.0;
            fmt.decimals = 1;
            fmt.trim = true;
        }
        "per_minute_to_per_second_1dp" => {
            fmt.value /= 60.0;
            fmt.decimals = 1;
            fmt.trim = false;
        }
        "per_minute_to_per_second_2dp" => {
            fmt.value /= 60.0;
            fmt.decimals = 2;
            fmt.trim = false;
        }
        "per_minute_to_per_second_2dp_if_required" => {
            fmt.value /= 60.0;
            fmt.decimals = 2;
            fmt.trim = true;
        }
        "divide_by_two_0dp" => {
            fmt.value = (fmt.value / 2.0).floor();
        }
        "divide_by_three" => fmt.value /= 3.0,
        "divide_by_four" => fmt.value /= 4.0,
        "divide_by_five" => fmt.value /= 5.0,
        "divide_by_six" => fmt.value /= 6.0,
        "divide_by_twelve" => fmt.value /= 12.0,
        "divide_by_fifteen_0dp" => fmt.value = (fmt.value / 15.0).floor(),
        "divide_by_twenty_then_double_0dp" => fmt.value = (fmt.value / 20.0).floor() * 2.0,
        "divide_by_fifty" => fmt.value /= 50.0,
        "divide_by_one_thousand" => fmt.value /= 1000.0,
        "double" => fmt.value *= 2.0,
        "times_one_point_five" => fmt.value *= 1.5,
        "times_twenty" => fmt.value *= 20.0,
        "multiply_by_four" => fmt.value *= 4.0,
        "plus_two_hundred" => fmt.value += 200.0,
        "30%_of_value" => fmt.value *= 0.3,
        "60%_of_value" => fmt.value *= 0.6,
        "negate_and_double" => fmt.value *= -2.0,
        _ => {}
    }
    fmt
}

fn render(sub: &CsdSubEntry, raw_values: &[i32]) -> String {
    let mut fmts: Vec<ValueFmt> = raw_values.iter().map(|&v| ValueFmt::from_i32(v)).collect();
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
                        if spec == Some("+d") && v.value >= 0.0 {
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
