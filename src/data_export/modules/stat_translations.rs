//! `stat_translations/*.json`, `stat_value_handlers.json` and
//! `stats_by_file.json` — the description files that turn a stat id and a
//! number into the line the client shows.

use crate::dat::csd::{parse_csd, CsdEntry, CsdSubEntry};
use crate::dat::stat_handlers::{self, Kind};
use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use std::collections::HashMap;

/// Where the description files live, in the casing RePoE reports them under.
const DIR: &str = "Data/StatDescriptions/";

/// The languages RePoE lists on every entry. Only English is filled in; the
/// rest are declared so consumers see the same keys.
const OTHER_LANGUAGES: [&str; 9] = [
    "French",
    "German",
    "Japanese",
    "Korean",
    "Portuguese",
    "Russian",
    "Spanish",
    "Thai",
    "Traditional Chinese",
];

pub fn stat_translations(ctx: &Ctx) -> Result<(), String> {
    let mut lookup = StatsByFile::default();
    let trade = ctx.options.trade_stats.then(fetch_trade_stats).unwrap_or_default();

    let mut written = 0;
    for path in ctx.files.list_dir(DIR) {
        if !path.to_ascii_lowercase().ends_with(".csd") {
            continue;
        }
        let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, &path) else { continue };
        let file = match parse_csd(&bytes, &path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("stat_translations: {} did not parse: {}", path, e);
                continue;
            }
        };
        // The index stores paths lower-cased; RePoE reports the real casing.
        let relative = path[DIR.len().min(path.len())..].to_string();
        let source = format!("{}{}", DIR, relative);

        let entries: Vec<J> = keep(&file.entries)
            .into_iter()
            .map(|entry| convert_entry(entry, &source, &mut lookup, &trade))
            .collect();
        let name = format!("stat_translations/{}", relative.trim_end_matches(".csd"));
        json::write(ctx.out, &name, &J::Arr(entries))?;
        written += 1;
    }

    if written == 0 {
        return Err(format!("no .csd files found under {}", DIR));
    }

    json::write(ctx.out, "stat_value_handlers", &value_handlers(ctx))?;
    json::write(ctx.out, "stats_by_file", &lookup.build())
}

/// The entries a description file actually contributes.
///
/// `no_description` stats have no text and never appear. A file may also state
/// the same set of stat ids more than once: repeating it word for word leaves
/// both in place, but a redefinition that says something different replaces
/// what came before it.
fn keep(entries: &[CsdEntry]) -> Vec<&CsdEntry> {
    let mut kept: Vec<&CsdEntry> = Vec::new();
    for entry in entries.iter().filter(|e| !e.descriptions.is_empty()) {
        let redefines = kept
            .iter()
            .any(|k| k.ids == entry.ids && k.descriptions != entry.descriptions);
        if redefines {
            kept.retain(|k| k.ids != entry.ids);
        }
        kept.push(entry);
    }
    kept
}

fn convert_entry(entry: &CsdEntry, source: &str, lookup: &mut StatsByFile, trade: &TradeStats) -> J {
    let english: Vec<(J, Parsed)> = entry
        .descriptions
        .iter()
        .filter(|sub| sub.language.is_none())
        .map(|sub| {
            let parsed = parse_sub(sub, entry.ids.len());
            (render_sub(&parsed), parsed)
        })
        .collect();

    for (_, parsed) in &english {
        lookup.add(parsed, &entry.ids, source);
    }
    let matched = trade.matches(english.iter().map(|(_, p)| p.string.as_str()));

    let mut obj = Obj::new()
        .set("English", J::Arr(english.into_iter().map(|(j, _)| j).collect()))
        .set("ids", json::strings(&entry.ids))
        .or_null("trade_stats", matched)
        .or_null("hidden", None);
    for language in OTHER_LANGUAGES {
        obj = obj.or_null(language, None);
    }
    obj.build()
}

/// Stat ids the official trade site uses, keyed by the text it shows.
#[derive(Default)]
struct TradeStats {
    by_text: HashMap<String, Vec<(String, J)>>,
}

impl TradeStats {
    /// Trade ids for one entry: whole strings first, then, if none matched,
    /// the individual lines of a multi-line description.
    fn matches<'a>(&self, strings: impl Iterator<Item = &'a str> + Clone) -> Option<J> {
        if self.by_text.is_empty() {
            return None;
        }
        let mut whole: Vec<(String, J)> = Vec::new();
        let mut partial: Vec<(String, J)> = Vec::new();
        for string in strings {
            let format = placeholder_form(string);
            match self.by_text.get(&format) {
                Some(hits) => whole.extend(hits.iter().cloned()),
                None if format.contains('\n') => {
                    for line in format.lines() {
                        if let Some(hits) = self.by_text.get(line) {
                            partial.extend(hits.iter().cloned());
                        }
                    }
                }
                None => {
                    let digits = replace_digits(&format);
                    if let Some(hits) = self.by_text.get(&digits) {
                        whole.extend(hits.iter().cloned());
                    }
                }
            }
        }
        // Buckets are already id-sorted; across buckets the trade site's own
        // order stands, which is what the published files show.
        let mut found = if whole.is_empty() { partial } else { whole };
        let mut seen = std::collections::HashSet::new();
        found.retain(|(id, _)| seen.insert(id.clone()));
        (!found.is_empty()).then(|| J::Arr(found.into_iter().map(|(_, v)| v).collect()))
    }
}

/// `{0}` and friends become the `#` the trade site shows.
fn placeholder_form(string: &str) -> String {
    let mut out = String::with_capacity(string.len());
    let chars: Vec<char> = string.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(rel) = chars[i..].iter().position(|&c| c == '}') {
                out.push('#');
                i += rel + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn replace_digits(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_number = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
                in_number = true;
            }
        } else {
            in_number = false;
            out.push(c);
        }
    }
    out
}

/// Pulls the trade site's stat list. Optional: without it every entry's
/// `trade_stats` stays null, exactly as when the request fails.
fn fetch_trade_stats() -> TradeStats {
    let url = "https://www.pathofexile.com/api/trade2/data/stats";
    let request = reqwest::blocking::Client::new()
        .get(url)
        .header("User-Agent", concat!("ggpk-explorer/", env!("CARGO_PKG_VERSION")))
        .send();
    let body: serde_json::Value = match request.and_then(|r| r.json()) {
        Ok(body) => body,
        Err(e) => {
            eprintln!("stat_translations: could not read the trade stat list: {}", e);
            return TradeStats::default();
        }
    };
    let mut by_text: HashMap<String, Vec<(String, J)>> = HashMap::new();
    let groups = body.get("result").and_then(|r| r.as_array()).map(Vec::as_slice).unwrap_or_default();
    for entry in groups.iter().filter_map(|g| g.get("entries")).filter_map(|e| e.as_array()).flatten() {
        let Some(base) = entry.get("text").and_then(|t| t.as_str()) else { continue };
        let id = entry.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
        let mut value = from_serde(entry);
        // The published files carry `option` on every trade stat, null or not.
        if value.get("option").is_none() {
            value.set("option", J::Null);
        }
        let options = entry
            .get("option")
            .and_then(|o| o.get("options"))
            .and_then(|o| o.as_array())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if options.is_empty() {
            by_text.entry(base.to_string()).or_default().push((id, value));
            continue;
        }
        for option in options {
            let Some(label) = option.get("text").and_then(|t| t.as_str()) else { continue };
            by_text
                .entry(base.replacen('#', label, 1))
                .or_default()
                .push((id.clone(), value.clone()));
        }
    }
    for bucket in by_text.values_mut() {
        bucket.sort_by(|a, b| a.0.cmp(&b.0));
    }
    TradeStats { by_text }
}

fn from_serde(value: &serde_json::Value) -> J {
    match value {
        serde_json::Value::Null => J::Null,
        serde_json::Value::Bool(b) => J::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => J::Int(i),
            None => J::Num(n.as_f64().unwrap_or_default()),
        },
        serde_json::Value::String(s) => J::Str(s.clone()),
        serde_json::Value::Array(items) => J::Arr(items.iter().map(from_serde).collect()),
        serde_json::Value::Object(fields) => {
            J::Obj(fields.iter().map(|(k, v)| (k.clone(), from_serde(v))).collect())
        }
    }
}

/// One description line, broken into the pieces the JSON needs.
struct Parsed {
    /// Per-id condition from the leading range tokens.
    condition: Vec<Condition>,
    /// Per-id `#`, `+#` or `ignore`.
    format: Vec<&'static str>,
    /// Per-id handler names, in the order the file lists them.
    index_handlers: Vec<Vec<String>>,
    /// The text with every placeholder normalised to `{n}`.
    string: String,
    /// Literal runs around the placeholders; one more than `tags`.
    literals: Vec<String>,
    /// Which id each placeholder refers to, in order of appearance.
    tags: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Condition {
    min: Option<i64>,
    max: Option<i64>,
    negated: bool,
}

fn parse_sub(sub: &CsdSubEntry, id_count: usize) -> Parsed {
    let tokens: Vec<&str> = sub.operator.split_whitespace().collect();
    let condition = (0..id_count)
        .map(|i| parse_condition(tokens.get(i).copied().unwrap_or("#")))
        .collect();

    let (string, placeholders, literals) = scan_placeholders(&sub.description);

    let mut format = vec!["ignore"; id_count];
    for &(tag, signed) in &placeholders {
        if tag < id_count {
            format[tag] = if signed { "+#" } else { "#" };
        }
    }
    let tags: Vec<usize> = placeholders.iter().map(|&(tag, _)| tag).collect();

    // Every handler given a value index is listed against that value, whether
    // or not it changes the number; `canonical_line` takes none and is not.
    let mut index_handlers = vec![Vec::new(); id_count];
    for param in sub.parameters.iter().filter(|p| p.value >= 1) {
        if let Some(slot) = index_handlers.get_mut(param.value as usize - 1) {
            slot.push(param.name.clone());
        }
    }

    Parsed { condition, format, index_handlers, string, literals, tags }
}

fn parse_condition(token: &str) -> Condition {
    if let Some(rest) = token.strip_prefix('!') {
        let v = rest.parse().ok();
        return Condition { min: v, max: v, negated: true };
    }
    let (min, max) = token.split_once('|').unwrap_or((token, token));
    let parse = |s: &str| if s == "#" { None } else { s.parse::<i64>().ok() };
    Condition { min: parse(min), max: parse(max), negated: false }
}

/// Rewrites `{}`/`{0}`/`{0:+d}` to plain `{n}` and reports, per placeholder,
/// which id it names and whether it prints signed, plus the literal text
/// between them.
fn scan_placeholders(description: &str) -> (String, Vec<(usize, bool)>, Vec<String>) {
    let mut string = String::with_capacity(description.len());
    let mut tags = Vec::new();
    let mut literals = Vec::new();
    let mut literal = String::new();
    let mut anonymous = 0usize;

    let bytes: Vec<char> = description.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // `{{` escapes a brace, so the `{` after one never opens a placeholder
        // — `<enchanted>{{{0}%…}}` shows a literal `{0}`, not a value.
        if bytes[i] == '{' && i.checked_sub(1).map(|p| bytes[p]) != Some('{') {
            if let Some(rel) = bytes[i..].iter().position(|&c| c == '}') {
                let token: String = bytes[i + 1..i + rel].iter().collect();
                let index_part = token.split(':').next().unwrap_or("");
                let index = if index_part.is_empty() {
                    anonymous += 1;
                    anonymous - 1
                } else {
                    match index_part.parse::<usize>() {
                        Ok(n) => n,
                        Err(_) => {
                            // Not a placeholder — keep the braces as text.
                            literal.push('{');
                            string.push('{');
                            i += 1;
                            continue;
                        }
                    }
                };
                string.push_str(&format!("{{{}}}", index));
                let signed = token.split_once(':').map(|(_, spec)| spec == "+d").unwrap_or(false);
                tags.push((index, signed));
                literals.push(std::mem::take(&mut literal));
                i += rel + 1;
                continue;
            }
        }
        literal.push(bytes[i]);
        string.push(bytes[i]);
        i += 1;
    }
    literals.push(literal);
    (string, tags, literals)
}

fn render_sub(parsed: &Parsed) -> J {
    let condition = parsed.condition.iter().map(|c| {
        Obj::new()
            .or_null("min", c.min.map(int))
            .or_null("max", c.max.map(int))
            .or_null("negated", c.negated.then_some(J::Bool(true)))
            .build()
    });
    Obj::new()
        .set("condition", json::arr(condition))
        .set("format", json::strings(&parsed.format))
        .set("index_handlers", json::arr(parsed.index_handlers.iter().map(json::strings)))
        .set("string", text(&parsed.string))
        .or_null("reminder_text", None)
        .or_null("is_markup", None)
        .build()
}

/// `stats_by_file.json`: every distinct description string, the files it came
/// from, and the string broken into literal/number/enum tokens.
#[derive(Default)]
struct StatsByFile {
    order: Vec<String>,
    entries: HashMap<String, Entry>,
}

struct Entry {
    files: Vec<String>,
    generated_name: String,
    tokens: J,
    implied: Vec<(String, i64)>,
}

impl StatsByFile {
    fn add(&mut self, parsed: &Parsed, ids: &[String], source: &str) {
        if let Some(entry) = self.entries.get_mut(&parsed.string) {
            entry.files.push(source.to_string());
            return;
        }
        let generated_name = format!("Stat_{}", self.order.len());
        let entry = Entry {
            files: vec![source.to_string()],
            generated_name,
            tokens: tokens(parsed, ids),
            implied: implied_stats(parsed, ids),
        };
        self.order.push(parsed.string.clone());
        self.entries.insert(parsed.string.clone(), entry);
    }

    fn build(self) -> J {
        let fields = self
            .order
            .iter()
            .filter_map(|key| self.entries.get(key).map(|e| (key.clone(), e)))
            .map(|(key, entry)| {
                let implied = (!entry.implied.is_empty()).then(|| {
                    J::Obj(entry.implied.iter().map(|(id, v)| (id.clone(), int(*v))).collect())
                });
                let value = Obj::new()
                    .set("files", json::strings(&entry.files))
                    .set("generated_name", text(&entry.generated_name))
                    .set("tokens", entry.tokens.clone())
                    .or_null("implied_stats", implied)
                    .build();
                (key, value)
            })
            .collect();
        J::Obj(fields)
    }
}

fn tokens(parsed: &Parsed, ids: &[String]) -> J {
    let mut out = Vec::new();
    let literal = |s: &String| Obj::new().set("type", text("literal")).set("value", text(s)).build();

    for (position, &tag) in parsed.tags.iter().enumerate() {
        if let Some(before) = parsed.literals.get(position).filter(|s| !s.is_empty()) {
            out.push(literal(before));
        }
        let stat = ids.get(tag).cloned().unwrap_or_default();
        let handlers = parsed.index_handlers.get(tag).cloned().unwrap_or_default();
        let first = handlers.iter().find_map(|h| stat_handlers::lookup(h));
        out.push(match first.map(|h| &h.kind) {
            Some(Kind::Relational { .. }) => Obj::new()
                .set("type", text("enum"))
                .set("index", int(tag as i64))
                .set("stat", text(&stat))
                .set("stat_value_handler", text(first.map(|h| h.name).unwrap_or_default()))
                .build(),
            Some(Kind::Int { .. }) => Obj::new()
                .set("type", text("number"))
                .set("index", int(tag as i64))
                .set("stat", text(&stat))
                .set("stat_value_handlers", json::strings(&handlers))
                .build(),
            _ => Obj::new()
                .set("type", text("number"))
                .set("index", int(tag as i64))
                .set("stat", text(&stat))
                .or_null("stat_value_handlers", None)
                .build(),
        });
    }
    if let Some(tail) = parsed.literals.last().filter(|s| !s.is_empty()) {
        out.push(literal(tail));
    }
    J::Arr(out)
}

/// Stats a description only applies to at one value — a condition pinned to a
/// single number tells you the stat must hold that number.
fn implied_stats(parsed: &Parsed, ids: &[String]) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for (i, condition) in parsed.condition.iter().enumerate() {
        // A placeholder makes the value free rather than implied.
        if parsed.tags.contains(&i) {
            continue;
        }
        let Some(id) = ids.get(i) else { continue };
        let value = match (condition.min, condition.max, condition.negated) {
            (Some(0), Some(0), true) => Some(1),
            (_, _, true) => None,
            (Some(min), Some(max), false) if min == max => Some(min),
            (Some(min), None, false) if min > 0 => Some(min),
            (None, Some(max), false) if max < 0 => Some(max),
            _ => None,
        };
        if let Some(value) = value {
            out.push((id.clone(), value));
        }
    }
    out
}

/// `stat_value_handlers.json`, with the relational handlers' lookup tables
/// read out of the game rather than hard-coded.
fn value_handlers(ctx: &Ctx) -> J {
    let fields = stat_handlers::HANDLERS
        .iter()
        .map(|handler| {
            let value = match &handler.kind {
                Kind::Noop => Obj::new().set("type", text("noop")).build(),
                Kind::Str => Obj::new().set("type", text("string")).build(),
                Kind::Int { multiplier, divisor, addend, precision, fixed } => Obj::new()
                    .or_null("addend", addend.map(J::Num))
                    .or_null("divisor", divisor.map(J::Num))
                    .or_null("multiplier", multiplier.map(J::Num))
                    .or_null("precision", precision.map(|p| int(p as i64)))
                    .or_null("fixed", fixed.then_some(J::Bool(true)))
                    .set("type", text("int"))
                    .build(),
                Kind::Relational { dat_file, value_column, index_column, predicate } => {
                    let predicate_json = predicate.map(|(column, value)| {
                        Obj::new().set("column", text(column)).set("value", int(value)).build()
                    });
                    Obj::new()
                        .set("type", text("relational"))
                        .set("dat_file", text(format!("{}.dat64", dat_file)))
                        .set("value_column", text(*value_column))
                        .or_null("index_column", index_column.map(text))
                        .or_null("predicate", predicate_json)
                        .set("values", relational_values(ctx, dat_file, value_column, *index_column, *predicate))
                        .build()
                }
            };
            (handler.name.to_string(), value)
        })
        .collect();
    J::Obj(fields)
}

fn relational_values(
    ctx: &Ctx,
    dat_file: &str,
    value_column: &str,
    index_column: Option<&str>,
    predicate: Option<(&str, i64)>,
) -> J {
    let Some(table) = ctx.rr.table(dat_file) else { return J::Obj(Vec::new()) };
    let mut out = Vec::new();
    for row in table.rows() {
        if let Some((column, wanted)) = predicate {
            if row.opt_int(column) != Some(wanted) {
                continue;
            }
        }
        let Some(display) = display_value(ctx, row, value_column) else { continue };
        let key = match index_column {
            Some(column) => match row.opt_int(column) {
                Some(v) => v.to_string(),
                None => continue,
            },
            None => row.index.to_string(),
        };
        out.push((key, text(display)));
    }
    J::Obj(out)
}

/// The text a lookup column shows: its own string, or the name of the row it
/// points at — a skill gem, for instance, is named by its base item type.
fn display_value(ctx: &Ctx, row: crate::dat::relational::Row<'_>, column: &str) -> Option<String> {
    let direct = row.str(column);
    if !direct.is_empty() {
        return Some(direct.to_string());
    }
    let target = ctx
        .rr
        .deref(row, column)
        .or_else(|| ctx.rr.deref_list(row, column).into_iter().next())?;
    let target = target.row();
    for name in ["Name", "Text", "DisplayText", "Id"] {
        let value = target.str(name);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let base = ctx.rr.deref(target, "BaseItemType")?;
    Some(base.row().str("Name").to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_numbered_and_split() {
        let (string, tags, literals) = scan_placeholders("Boss drops {0} additional Rare {1}");
        assert_eq!(string, "Boss drops {0} additional Rare {1}");
        assert_eq!(tags, vec![(0, false), (1, false)]);
        assert_eq!(literals, vec!["Boss drops ", " additional Rare ", ""]);

        let (string, tags, _) = scan_placeholders("{} and {} again");
        assert_eq!(string, "{0} and {1} again");
        assert_eq!(tags, vec![(0, false), (1, false)]);

        // A signed placeholder is reported, and one behind an escaped brace is not.
        assert_eq!(scan_placeholders("{0:+d} to Deflection").1, vec![(0, true)]);
        assert_eq!(scan_placeholders("<enchanted>{{{0}% more}}").1, vec![]);
    }

    #[test]
    fn conditions_cover_the_range_forms() {
        let c = parse_condition("#");
        assert!(c.min.is_none() && c.max.is_none() && !c.negated);
        let c = parse_condition("1|#");
        assert_eq!((c.min, c.max), (Some(1), None));
        let c = parse_condition("#|-1");
        assert_eq!((c.min, c.max), (None, Some(-1)));
        let c = parse_condition("!0");
        assert_eq!((c.min, c.max, c.negated), (Some(0), Some(0), true));
    }

}
