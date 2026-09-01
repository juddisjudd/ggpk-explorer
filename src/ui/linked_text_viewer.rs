//! Read-only text view where every file reference is a link. Rows are virtualised so
//! multi-megabyte room and tile lists stay responsive.

use crate::ui::links;
use eframe::egui::{self, Color32, RichText};

const ROW_H: f32 = 18.0;

/// One display token of a line.
enum Tok<'a> {
    Text(&'a str),
    Quoted(&'a str),
    Number(&'a str),
    Comment(&'a str),
}

fn flush<'a>(out: &mut Vec<Tok<'a>>, s: &'a str) {
    if s.is_empty() {
        return;
    }
    for (k, word) in s.split(' ').enumerate() {
        if k > 0 {
            out.push(Tok::Text(" "));
        }
        if word.is_empty() {
            continue;
        }
        if word.parse::<f64>().is_ok() {
            out.push(Tok::Number(word));
        } else {
            out.push(Tok::Text(word));
        }
    }
}

fn tokenize(line: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut start = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            flush(&mut out, &line[start..i]);
            out.push(Tok::Comment(&line[i..]));
            return out;
        }
        if bytes[i] == b'"' {
            flush(&mut out, &line[start..i]);
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = j.min(bytes.len());
            out.push(Tok::Quoted(&line[i + 1..end]));
            i = end + 1;
            start = i;
            continue;
        }
        i += 1;
    }
    flush(&mut out, &line[start.min(line.len())..]);
    out
}

pub struct LinkedTextViewer;

impl LinkedTextViewer {
    /// Shows `text` with a filter box; returns a path when a link is clicked.
    pub fn show(ui: &mut egui::Ui, id: u64, text: &str, filter: &mut String) -> Option<String> {
        let mut out = None;
        let lines: Vec<&str> = text.lines().collect();
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(filter).hint_text("Filter lines").desired_width(240.0));
            if !filter.is_empty() && ui.small_button("✕").clicked() {
                filter.clear();
            }
            ui.label(RichText::new(format!("{} lines", lines.len())).weak());
        });
        let needle = filter.trim().to_ascii_lowercase();
        let visible: Vec<usize> = if needle.is_empty() {
            (0..lines.len()).collect()
        } else {
            lines.iter().enumerate().filter(|(_, l)| l.to_ascii_lowercase().contains(&needle)).map(|(i, _)| i).collect()
        };
        let dark = ui.visuals().dark_mode;
        let dim = if dark { Color32::from_rgb(110, 110, 120) } else { Color32::from_rgb(150, 150, 158) };
        let string_c = if dark { Color32::from_rgb(152, 195, 121) } else { Color32::from_rgb(3, 117, 43) };
        let num_c = if dark { Color32::from_rgb(209, 154, 102) } else { Color32::from_rgb(180, 83, 9) };
        let gutter = format!("{}", lines.len()).len().max(3);

        egui::ScrollArea::both().id_salt(("linked_text", id)).auto_shrink([false, false]).show_rows(ui, ROW_H, visible.len(), |ui, range| {
            for vi in range {
                let li = visible[vi];
                let line = lines[li];
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(RichText::new(format!("{:>w$} ", li + 1, w = gutter)).monospace().color(dim));
                    let indent = line.chars().take_while(|c| *c == '\t' || *c == ' ').map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
                    ui.add_space(indent as f32 * 7.0);
                    for tok in tokenize(line.trim_start()) {
                        match tok {
                            Tok::Text(t) => {
                                links::maybe_link(ui, t, true, &mut out);
                            }
                            Tok::Quoted(q) => {
                                if links::looks_like_path(q) {
                                    ui.label(RichText::new("\"").monospace().color(string_c));
                                    links::maybe_link(ui, q, true, &mut out);
                                    ui.label(RichText::new("\"").monospace().color(string_c));
                                } else {
                                    ui.label(RichText::new(format!("\"{}\"", q)).monospace().color(string_c));
                                }
                            }
                            Tok::Number(n) => {
                                ui.label(RichText::new(n).monospace().color(num_c));
                            }
                            Tok::Comment(c) => {
                                ui.label(RichText::new(c).monospace().color(dim).italics());
                            }
                        }
                    }
                });
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_quotes_numbers_and_comments() {
        let toks = tokenize("10 \"Metadata/A.tdt\" x // note \"not\"");
        let kinds: Vec<&str> = toks
            .iter()
            .map(|t| match t {
                Tok::Text(s) if s.trim().is_empty() => "sp",
                Tok::Text(_) => "text",
                Tok::Quoted(_) => "quoted",
                Tok::Number(_) => "num",
                Tok::Comment(_) => "comment",
            })
            .collect();
        assert_eq!(kinds, vec!["num", "sp", "quoted", "sp", "text", "sp", "comment"]);
    }
}
