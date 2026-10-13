//! Ctrl+K: jump to any file in the index. The search itself is `crate::search`, run off the UI thread as the query changes.

use crate::bundles::index::Index;
use crate::search::{highlights, search, Mode, Query};
use crate::ui::components::modal::{self, shade};
use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const SHOWN: usize = 200;
const REMEMBERED: usize = 40;

struct Row {
    hash: u64,
    path: String,
    size: u32,
}

struct Answer {
    generation: u64,
    rows: Vec<Row>,
    total: usize,
    typos: bool,
    took_ms: u128,
}

#[derive(Default)]
pub struct CommandPalette {
    open: bool,
    query: String,
    /// The text the rows on screen answer; `None` until the first search of this opening.
    searched: Option<String>,
    selected_index: usize,
    focus_input: bool,
    rows: Vec<Row>,
    total: usize,
    typos: bool,
    took_ms: u128,
    generation: u64,
    answers: Option<Receiver<Answer>>,
    /// Files opened this session, newest first; they rank higher and fill the list before anything is typed.
    recent: VecDeque<u64>,
}

fn size_text(bytes: u32) -> String {
    match bytes {
        0..=1023 => format!("{} B", bytes),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// A row's text: the file name first, then its folder in a quieter hand, with the query's words picked out.
fn row_text(dark_mode: bool, path: &str, query: &Query, width: f32) -> egui::text::LayoutJob {
    let name_start = path.rfind('/').map_or(0, |slash| slash + 1);
    let marks = highlights(path, query);
    let accent = shade(dark_mode, [125, 178, 255], [29, 78, 216]);
    let mut job = egui::text::LayoutJob::default();
    let push = |job: &mut egui::text::LayoutJob, range: std::ops::Range<usize>, font: egui::FontId, colour: egui::Color32| {
        let mut at = range.start;
        for mark in marks.iter().filter(|mark| mark.start < range.end && mark.end > range.start) {
            let (start, end) = (mark.start.max(range.start), mark.end.min(range.end));
            if start > at {
                job.append(&path[at..start], 0.0, egui::TextFormat::simple(font.clone(), colour));
            }
            job.append(&path[start..end], 0.0, egui::TextFormat::simple(font.clone(), accent));
            at = end;
        }
        if at < range.end {
            job.append(&path[at..range.end], 0.0, egui::TextFormat::simple(font.clone(), colour));
        }
    };
    // Room for the dot that says what kind of file this is.
    job.append(" ", 10.0, egui::TextFormat::simple(egui::FontId::proportional(13.5), egui::Color32::TRANSPARENT));
    push(&mut job, name_start..path.len(), egui::FontId::proportional(13.5), shade(dark_mode, [236, 236, 240], [24, 24, 28]));
    job.append("   ", 0.0, egui::TextFormat::default());
    push(&mut job, 0..name_start, egui::FontId::monospace(11.0), shade(dark_mode, [118, 118, 128], [120, 120, 132]));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

impl CommandPalette {
    pub fn open(&mut self) {
        self.open = true;
        self.selected_index = 0;
        self.focus_input = true;
        self.searched = None;
    }

    pub fn handle_shortcut(&mut self, ctx: &egui::Context) -> bool {
        let pressed = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K));
        if pressed {
            match self.open {
                true => self.close(),
                false => self.open(),
            }
        }
        pressed
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.answers = None;
    }

    /// Remembers a file the user opened, however it was opened.
    pub fn note_opened(&mut self, hash: u64) {
        if self.recent.front() != Some(&hash) {
            self.recent.retain(|seen| *seen != hash);
            self.recent.push_front(hash);
            self.recent.truncate(REMEMBERED);
        }
    }

    fn start_search(&mut self, ctx: &egui::Context, index: &Arc<Index>) {
        self.searched = Some(self.query.clone());
        self.generation += 1;
        self.selected_index = 0;
        if self.query.trim().is_empty() {
            self.answers = None;
            self.rows = self.recent.iter().filter_map(|hash| index.files.get(hash).map(|file| Row { hash: *hash, path: file.path.clone(), size: file.file_size })).collect();
            (self.total, self.typos, self.took_ms) = (self.rows.len(), false, 0);
            return;
        }
        let (tx, rx) = channel();
        self.answers = Some(rx);
        let (index, ctx, text, generation) = (Arc::clone(index), ctx.clone(), self.query.clone(), self.generation);
        let lift: HashMap<u64, i32> = self.recent.iter().enumerate().map(|(rank, hash)| (*hash, 40 - rank as i32)).collect();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let found = search(&index, &Query::parse(&text), Mode::Ranked, SHOWN, &|hash| lift.get(&hash).copied().unwrap_or(0));
            let rows = found.hits.iter().filter_map(|hit| index.files.get(&hit.hash).map(|file| Row { hash: hit.hash, path: file.path.clone(), size: file.file_size })).collect();
            let _ = tx.send(Answer { generation, rows, total: found.total, typos: found.typos, took_ms: started.elapsed().as_millis() });
            ctx.request_repaint();
        });
    }

    fn summary(&self, files: usize) -> String {
        let searching = self.answers.is_some();
        match (self.query.trim().is_empty(), searching, self.total) {
            (true, _, _) => format!("{} files", crate::ui::components::thousands(files)),
            (false, true, _) => "Searching…".to_string(),
            (false, false, 0) => "No matches".to_string(),
            (false, false, total) => {
                let near = if self.typos { "near matches · " } else { "" };
                format!("{}{} of {} · {} ms", near, self.rows.len(), crate::ui::components::thousands(total), self.took_ms)
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, index: Option<&Arc<Index>>) -> Option<u64> {
        if !self.open {
            return None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close();
            return None;
        }

        if let Some(index) = index {
            if self.searched.as_deref() != Some(self.query.as_str()) {
                self.start_search(ctx, index);
            }
        }
        if let Some(answer) = self.answers.as_ref().and_then(|answers| answers.try_recv().ok()) {
            if answer.generation == self.generation {
                (self.rows, self.total, self.typos, self.took_ms) = (answer.rows, answer.total, answer.typos, answer.took_ms);
                self.answers = None;
            }
        }

        let (move_down, move_up, submit) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter)));
        if move_down && !self.rows.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.rows.len() - 1);
        }
        if move_up {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
        let moved_by_key = move_down || move_up;

        let mut picked: Option<u64> = None;
        let dismissed = modal::scrim(ctx, egui::Id::new("command_palette_scrim"));
        let query = Query::parse(self.searched.as_deref().unwrap_or_default());

        egui::Area::new(egui::Id::new("command_palette_overlay")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0)).show(ctx, |ui| {
            let dark_mode = ui.visuals().dark_mode;
            modal::frame(dark_mode).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                ui.set_width(780.0_f32.min(ctx.screen_rect().width() - 48.0));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Search files").size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        crate::ui::components::badge(ui, "Ctrl+K");
                        let files = index.map_or(0, |index| index.files.len());
                        ui.label(crate::ui::components::hint(ui, &self.summary(files)).size(11.5));
                    });
                });
                ui.add_space(2.0);

                let input_id = ui.make_persistent_id("command_palette_input");
                let response = ui.add(egui::TextEdit::singleline(&mut self.query).id(input_id).hint_text(crate::ui::components::hint(ui, "A name, or part of one")).desired_width(f32::INFINITY));
                if self.focus_input {
                    response.request_focus();
                    self.focus_input = false;
                }
                ui.add_space(6.0);

                if index.is_none() {
                    ui.label(crate::ui::components::hint(ui, "Open a GGPK or a Steam folder first."));
                    return;
                }
                if self.rows.is_empty() {
                    let empty = match self.query.trim().is_empty() {
                        true => "Files you open show up here. Start typing to search them all.",
                        false if self.answers.is_some() => "Searching…",
                        false => "Nothing matches. Fewer letters, or another spelling, may find it.",
                    };
                    ui.label(crate::ui::components::hint(ui, empty));
                } else if self.query.trim().is_empty() {
                    crate::ui::components::modal_section(ui, "RECENT");
                }

                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (row, item) in self.rows.iter().enumerate() {
                        let selected = row == self.selected_index;
                        let fill = if selected { shade(dark_mode, [34, 34, 40], [232, 236, 246]) } else { egui::Color32::TRANSPARENT };
                        let res = egui::Frame::none().fill(fill).rounding(egui::Rounding::same(6.0)).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let room = (ui.available_width() - 80.0).max(120.0);
                                let label = ui.label(row_text(dark_mode, &item.path, &query, room));
                                crate::ui::file_kind::dot(ui.painter(), egui::pos2(label.rect.left() + 5.0, label.rect.center().y), crate::ui::file_kind::FileKind::of(&item.path), dark_mode);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(crate::ui::components::hint(ui, &size_text(item.size)).monospace().size(10.5));
                                });
                            });
                        });
                        let res = res.response.interact(egui::Sense::click());
                        if selected && moved_by_key {
                            res.scroll_to_me(None);
                        }
                        if res.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            self.selected_index = row;
                        }
                        if res.clicked() {
                            picked = Some(item.hash);
                        }
                    }
                });

                ui.add_space(4.0);
                ui.label(crate::ui::components::hint(ui, "Up and Down to move  ·  Enter to open  ·  Esc to close  ·  narrow with *.dds, art/ or !word").size(11.0));
            });
        });

        if submit {
            picked = picked.or(self.rows.get(self.selected_index).map(|row| row.hash));
        }
        if picked.is_some() || dismissed {
            self.close();
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_the_file_name_before_its_folder() {
        let job = row_text(true, "art/2dart/items/Sword.dds", &Query::parse("sword"), 600.0);
        assert_eq!(job.text, " Sword.dds   art/2dart/items/");
        let accent = shade(true, [125, 178, 255], [29, 78, 216]);
        let lit: String = job.sections.iter().filter(|section| section.format.color == accent).map(|section| &job.text[section.byte_range.clone()]).collect();
        assert_eq!(lit, "Sword");
    }

    #[test]
    fn files_opened_lately_come_first_and_are_not_repeated() {
        let mut palette = CommandPalette::default();
        for hash in [1, 2, 1, 3, 3] {
            palette.note_opened(hash);
        }
        assert_eq!(palette.recent, [3, 1, 2]);
    }
}
