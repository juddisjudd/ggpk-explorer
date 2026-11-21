//! `keywords.json` beside each tree: its linked stat lines and the keyword popups they reach.

use crate::data_export::Ctx;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Plain stat line, as `tree.json` has it, to the same line with its links.
pub type Lines = BTreeMap<String, String>;

/// False when `raw` has links but draws something other than `plain`.
pub fn record(lines: &mut Lines, plain: &str, raw: &str) -> bool {
    let (linked, found) = walk(&raw.chars().collect::<Vec<_>>());
    if !found {
        return true;
    }
    if display(&linked) != plain {
        return false;
    }
    lines.insert(plain.to_string(), linked);
    true
}

pub fn write(ctx: &Ctx, dir: &Path, lines: &Lines) -> Result<(usize, usize), String> {
    let table = ctx.table("KeywordPopups")?;
    let mut all: BTreeMap<String, (String, String)> = BTreeMap::new();
    for row in table.rows() {
        let definition = clean(row.str("Definition"));
        if !definition.trim().is_empty() {
            all.insert(row.id().to_string(), (row.str("Term").to_string(), definition));
        }
    }
    let mut queue: Vec<String> = lines.values().flat_map(|l| ids(l)).collect();
    let mut reached = BTreeSet::new();
    while let Some(id) = queue.pop() {
        if let Some((_, definition)) = all.get(&id) {
            if reached.insert(id) {
                queue.extend(ids(definition));
            }
        }
    }
    let keywords: BTreeMap<&str, serde_json::Value> = reached
        .iter()
        .map(|id| (id.as_str(), serde_json::json!({ "term": all[id].0, "definition": all[id].1 })))
        .collect();
    let out = serde_json::json!({ "lines": lines, "keywords": keywords });
    let path = dir.join("keywords.json");
    let mut text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok((lines.len(), keywords.len()))
}

fn clean(raw: &str) -> String {
    let mut text = raw.replace("\r\n", "\n");
    while let Some(start) = text.find("<<") {
        let Some(end) = text[start..].find(">>") else { break };
        text.replace_range(start..start + end + 2, "");
    }
    walk(&text.chars().collect::<Vec<_>>()).0
}

/// Tags reduced to their text, links rewritten as `[Id]` or `[Id|Label]`, and whether any link was found.
fn walk(chars: &[char]) -> (String, bool) {
    let mut out = String::with_capacity(chars.len());
    let mut found = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some((id, label, next)) = link(chars, i) {
                let shown = label.map(|l| display(&walk(&l.chars().collect::<Vec<_>>()).0));
                match shown {
                    Some(shown) if shown != id => out.push_str(&format!("[{}|{}]", id, shown)),
                    _ => out.push_str(&format!("[{}]", id)),
                }
                found = true;
                i = next;
                continue;
            }
        } else if chars[i] == '<' {
            if let Some((inner, next)) = tagged(chars, i) {
                let (text, inner_found) = walk(&inner.chars().collect::<Vec<_>>());
                out.push_str(&text);
                found |= inner_found;
                i = next;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, found)
}

/// Reads `[Id|Label]`, `[Id]`, `[Id::10|Label]` and `[Id<gemlevel 20>|Label]`.
fn link(chars: &[char], start: usize) -> Option<(String, Option<String>, usize)> {
    let mut parameters = 0;
    let mut bar = None;
    for (i, &c) in chars.iter().enumerate().skip(start + 1) {
        match c {
            '<' => parameters += 1,
            '>' if parameters > 0 => parameters -= 1,
            _ if parameters > 0 => {}
            '[' => return None,
            '|' if bar.is_none() => bar = Some(i),
            ']' => {
                let id = bare_id(&chars[start + 1..bar.unwrap_or(i)]);
                if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return None;
                }
                let label = bar.map(|b| chars[b + 1..i].iter().collect());
                return Some((id, label, i + 1));
            }
            _ => {}
        }
    }
    None
}

fn bare_id(chars: &[char]) -> String {
    let mut out = String::new();
    let mut parameters = 0;
    for &c in chars {
        match c {
            '<' => parameters += 1,
            '>' if parameters > 0 => parameters -= 1,
            _ if parameters > 0 => {}
            c => out.push(c),
        }
    }
    match out.find("::") {
        Some(at) => out[..at].to_string(),
        None => out,
    }
}

fn tagged(chars: &[char], start: usize) -> Option<(String, usize)> {
    let close = (start + 1..chars.len()).find(|&i| chars[i] == '>' || chars[i] == '<')?;
    if chars[close] != '>' || close == start + 1 || chars.get(close + 1) != Some(&'{') {
        return None;
    }
    let mut depth = 0;
    for (i, &c) in chars.iter().enumerate().skip(close + 1) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((chars[close + 2..i].iter().collect(), i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn display(linked: &str) -> String {
    let mut out = String::with_capacity(linked.len());
    let mut rest = linked;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|i| open + i) else { break };
        out.push_str(&rest[..open]);
        let token = &rest[open + 1..close];
        out.push_str(token.split_once('|').map_or(token, |(_, label)| label));
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

fn ids(linked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = linked;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|i| open + i) else { break };
        let token = &rest[open + 1..close];
        out.push(token.split_once('|').map_or(token, |(id, _)| id).to_string());
        rest = &rest[close + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_written_one_way() {
        let (text, found) = walk(&"12% increased [Fire] Damage".chars().collect::<Vec<_>>());
        assert!(found);
        assert_eq!(text, "12% increased [Fire] Damage");
        let raw = "[Resistances|Fire Resistance] and [Onslaught::10|Onslaught] with [Spark<gemlevel 20>|Spark]";
        let (text, _) = walk(&raw.chars().collect::<Vec<_>>());
        assert_eq!(text, "[Resistances|Fire Resistance] and [Onslaught] with [Spark]");
        assert_eq!(display(&text), "Fire Resistance and Onslaught with Spark");
    }

    #[test]
    fn tags_reduce_to_their_text() {
        assert_eq!(clean("<<Icon>><rgb(1,2,3)>{Fire Rune}\r\n<italic>{[Hit|Hits]}"), "Fire Rune\n[Hit|Hits]");
    }

    #[test]
    fn a_line_is_kept_only_when_it_draws_the_plain_text() {
        let mut lines = Lines::new();
        assert!(record(&mut lines, "Gain Onslaught", "Gain [Onslaught]"));
        assert!(record(&mut lines, "No links here", "No links here"));
        assert!(!record(&mut lines, "Something else", "Gain [Onslaught]"));
        assert_eq!(lines.len(), 1);
        assert_eq!(ids(&lines["Gain Onslaught"]), vec!["Onslaught"]);
    }
}
