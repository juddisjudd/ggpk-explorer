//! Stat-description (`.csd`) viewer: a searchable, virtualised table of every
//! description line with a detail panel that renders the text for sample values.

use crate::dat::csd::{CsdEntry, CsdFile};
use crate::dat::stat_translation;
use crate::ui::links;
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};

const ROW_H: f32 = 20.0;
const NO_DESC: usize = usize::MAX;

#[derive(Default)]
pub struct CsdViewerState {
    pub filter: String,
    /// `None` is the base (English) text.
    pub language: Option<String>,
    pub selected: Option<usize>,
    pub values: String,
    rows: Vec<(usize, usize)>,
    rows_key: Option<(String, Option<String>, usize)>,
}

fn sub_matches_language(sub_language: Option<&str>, wanted: Option<&str>) -> bool {
    sub_language == wanted
}

fn rebuild_rows(state: &mut CsdViewerState, file: &CsdFile) {
    let key = (state.filter.trim().to_ascii_lowercase(), state.language.clone(), file.entries.len());
    if state.rows_key.as_ref() == Some(&key) {
        return;
    }
    let needle = key.0.clone();
    let wanted = state.language.as_deref();
    let mut rows = Vec::new();
    for (ei, entry) in file.entries.iter().enumerate() {
        let ids_hit = needle.is_empty() || entry.ids.iter().any(|id| id.to_ascii_lowercase().contains(&needle));
        if entry.descriptions.is_empty() {
            if ids_hit {
                rows.push((ei, NO_DESC));
            }
            continue;
        }
        for (si, sub) in entry.descriptions.iter().enumerate() {
            if !sub_matches_language(sub.language.as_deref(), wanted) {
                continue;
            }
            if ids_hit || sub.description.to_ascii_lowercase().contains(&needle) {
                rows.push((ei, si));
            }
        }
    }
    state.rows = rows;
    state.rows_key = Some(key);
}

/// Description text with `{0}`-style placeholders in an accent colour.
fn description_job(text: &str, base: Color32, accent: Color32, font: egui::FontId) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |c: Color32| egui::text::TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let (before, after) = rest.split_at(start);
        job.append(before, 0.0, fmt(base));
        match after.find('}') {
            Some(end) => {
                job.append(&after[..=end], 0.0, fmt(accent));
                rest = &after[end + 1..];
            }
            None => {
                rest = after;
                break;
            }
        }
    }
    job.append(rest, 0.0, fmt(base));
    job
}

fn parse_values(text: &str, n: usize) -> Vec<i32> {
    let mut v: Vec<i32> = text.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|t| t.trim().parse::<i32>().ok()).collect();
    v.resize(n.max(1), 0);
    v
}

fn languages_in(entry: &CsdEntry) -> Vec<Option<String>> {
    let mut langs: Vec<Option<String>> = Vec::new();
    for s in &entry.descriptions {
        if !langs.contains(&s.language) {
            langs.push(s.language.clone());
        }
    }
    langs
}

pub struct CsdViewer;

impl CsdViewer {
    /// Returns a path when an `include` link is clicked.
    pub fn show(ui: &mut egui::Ui, id: u64, file: &CsdFile, state: &mut CsdViewerState) -> Option<String> {
        let mut opened = None;
        rebuild_rows(state, file);
        let dark = ui.visuals().dark_mode;
        let accent = if dark { Color32::from_rgb(229, 192, 123) } else { Color32::from_rgb(170, 110, 0) };
        let dim = if dark { Color32::from_rgb(113, 113, 122) } else { Color32::from_rgb(140, 140, 150) };
        let id_color = if dark { Color32::from_rgb(97, 175, 239) } else { Color32::from_rgb(9, 79, 172) };
        let star = Color32::from_rgb(255, 215, 0);

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter by stat id or text").desired_width(260.0));
            if !state.filter.is_empty() && ui.small_button("✕").clicked() {
                state.filter.clear();
            }
            ui.label("Language");
            let mut langs: Vec<String> = file.languages.clone();
            langs.sort();
            egui::ComboBox::from_id_salt(("csd_lang", id))
                .selected_text(state.language.as_deref().unwrap_or("English (base)"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.language, None, "English (base)");
                    for lang in langs {
                        ui.selectable_value(&mut state.language, Some(lang.clone()), lang);
                    }
                });
            ui.label(RichText::new(format!("{} entries · {} lines", file.entries.len(), state.rows.len())).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export JSON").clicked() {
                    let stem = std::path::Path::new(&file.path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "csd".into());
                    if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.json", stem)).save_file() {
                        if let Ok(f) = std::fs::File::create(path) {
                            let _ = serde_json::to_writer_pretty(std::io::BufWriter::new(f), file);
                        }
                    }
                }
                ui.label(RichText::new("click a row for every language and a live preview").weak().size(11.0));
            });
        });
        if !file.includes.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("includes").weak());
                for inc in &file.includes {
                    links::maybe_link(ui, inc, true, &mut opened);
                }
            });
        }
        ui.separator();

        if let Some(sel) = state.selected {
            if let Some(entry) = file.entries.get(sel) {
                let mut close = false;
                egui::SidePanel::right(egui::Id::new(("csd_detail", id))).resizable(true).default_width(420.0).show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Entry").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                close = true;
                            }
                            if ui.small_button("Copy ids").clicked() {
                                ui.ctx().copy_text(entry.ids.join(" "));
                            }
                        });
                    });
                    for stat in &entry.ids {
                        ui.label(RichText::new(stat).monospace().color(id_color));
                    }
                    ui.separator();
                    ui.label(RichText::new("Preview").strong());
                    ui.horizontal(|ui| {
                        ui.label(format!("{} value{}", entry.ids.len(), if entry.ids.len() == 1 { "" } else { "s" }));
                        ui.add(egui::TextEdit::singleline(&mut state.values).hint_text("e.g. 10, -5").desired_width(160.0));
                    });
                    let values = parse_values(&state.values, entry.ids.len());
                    let mut shown: Vec<Option<String>> = vec![None];
                    if state.language.is_some() && !shown.contains(&state.language) {
                        shown.push(state.language.clone());
                    }
                    for lang in shown {
                        let rendered = stat_translation::preview(entry, lang.as_deref(), &values);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(lang.as_deref().unwrap_or("English")).weak().size(11.0));
                            match rendered {
                                Some(t) => {
                                    ui.label(RichText::new(t).strong());
                                }
                                None => {
                                    ui.label(RichText::new("no line matches these values").color(dim).italics());
                                }
                            }
                        });
                    }
                    ui.separator();
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if entry.descriptions.is_empty() {
                            ui.label(RichText::new("no_description — the stat is intentionally hidden").color(dim).italics());
                        }
                        for lang in languages_in(entry) {
                            ui.label(RichText::new(lang.as_deref().unwrap_or("English")).strong().size(12.0));
                            for sub in entry.descriptions.iter().filter(|s| s.language == lang) {
                                ui.horizontal_wrapped(|ui| {
                                    if sub.is_canonical {
                                        ui.colored_label(star, "★");
                                    }
                                    ui.label(RichText::new(&sub.operator).monospace().color(dim));
                                    ui.label(description_job(&sub.description, ui.visuals().text_color(), accent, egui::FontId::proportional(13.0)));
                                });
                                if !sub.parameters.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.add_space(18.0);
                                        for p in &sub.parameters {
                                            ui.label(RichText::new(format!("{} {}", p.name, p.value)).weak().size(11.0));
                                        }
                                    });
                                }
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
                if close {
                    state.selected = None;
                }
            }
        }

        let rows = &state.rows;
        let mut select: Option<usize> = None;
        let selected = state.selected;
        egui::ScrollArea::horizontal().id_salt(("csd_hscroll", id)).auto_shrink([false, false]).show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .auto_shrink([true, false])
                .min_scrolled_height(0.0)
                .column(Column::initial(44.0).at_least(36.0).clip(true))
                .column(Column::initial(300.0).at_least(80.0).clip(true).resizable(true))
                .column(Column::initial(90.0).at_least(40.0).clip(true).resizable(true))
                .column(Column::initial(520.0).at_least(120.0).clip(true).resizable(true))
                .column(Column::remainder().at_least(120.0).clip(true))
                .header(22.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("#");
                    });
                    header.col(|ui| {
                        ui.strong("Stat ids");
                    });
                    header.col(|ui| {
                        ui.strong("Condition").on_hover_text("Value ranges this line applies to: # = any, 1|# = 1 or more, #|-1 = negative");
                    });
                    header.col(|ui| {
                        ui.strong("Text");
                    });
                    header.col(|ui| {
                        ui.strong("Functions").on_hover_text("Value transforms applied before substitution (index is 1-based)");
                    });
                })
                .body(|body| {
                    body.rows(ROW_H, rows.len(), |mut row| {
                        let (ei, si) = rows[row.index()];
                        let entry = &file.entries[ei];
                        row.set_selected(selected == Some(ei));
                        row.col(|ui| {
                            ui.label(RichText::new(ei.to_string()).weak().monospace());
                        });
                        row.col(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for stat in &entry.ids {
                                let r = ui.add(egui::Label::new(RichText::new(stat).monospace().color(id_color)).truncate().sense(egui::Sense::click()));
                                if r.on_hover_text("Click to copy").clicked() {
                                    ui.ctx().copy_text(stat.clone());
                                }
                            }
                        });
                        if si == NO_DESC {
                            row.col(|ui| {
                                ui.label(RichText::new("—").color(dim));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new("no_description").color(dim).italics());
                            });
                            row.col(|_| {});
                        } else {
                            let sub = &entry.descriptions[si];
                            row.col(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                if sub.is_canonical {
                                    ui.colored_label(star, "★");
                                }
                                ui.label(RichText::new(&sub.operator).monospace());
                            });
                            row.col(|ui| {
                                let job = description_job(&sub.description, ui.visuals().text_color(), accent, egui::FontId::proportional(13.0));
                                let r = ui.add(egui::Label::new(job).truncate());
                                r.on_hover_text(&sub.description);
                            });
                            row.col(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for p in &sub.parameters {
                                    ui.label(RichText::new(format!("{} {}", p.name, p.value)).weak().size(11.0));
                                }
                            });
                        }
                        if row.response().clicked() {
                            select = Some(ei);
                        }
                    });
                });
        });
        if let Some(s) = select {
            state.selected = if state.selected == Some(s) { None } else { Some(s) };
        }
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_and_pad() {
        assert_eq!(parse_values("10, -5 x 3", 2), vec![10, -5]);
        assert_eq!(parse_values("", 2), vec![0, 0]);
        assert_eq!(parse_values("7", 1), vec![7]);
    }

    #[test]
    fn placeholders_get_their_own_sections() {
        let job = description_job("Gain {0}% of {1:+d} Life", Color32::WHITE, Color32::RED, egui::FontId::proportional(12.0));
        assert_eq!(job.sections.len(), 5);
        assert_eq!(job.sections[1].format.color, Color32::RED);
        assert_eq!(&job.text[job.sections[1].byte_range.clone()], "{0}");
    }
}
