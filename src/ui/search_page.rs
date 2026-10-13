//! The search page: find text inside the game's files. The scan is `crate::content_search`, run off the UI thread, with matches listed as they arrive.

use crate::bundles::index::Index;
use crate::bundles::steam::SteamBundleLoader;
use crate::content_search::{ContentQuery, Event, LineHit, MOST_HITS};
use crate::ggpk::reader::GgpkReader;
use crate::search::Query;
use crate::ui::components::button::{action, Kind};
use crate::ui::components::{hint, thousands};
use crate::ui::components::modal::shade;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const ROW_HEIGHT: f32 = 24.0;
const NAME_WIDTH: f32 = 250.0;

/// Where the open install's files come from.
pub struct Sources<'a> {
    pub index: &'a Arc<Index>,
    pub reader: Option<&'a Arc<GgpkReader>>,
    pub steam: Option<&'a SteamBundleLoader>,
}

/// A match the user clicked: the file, the line (0 inside a table) and the text that was searched for.
pub struct Opened {
    pub hash: u64,
    pub line: u32,
    pub text: String,
}

#[derive(Default)]
pub struct SearchActions {
    pub open: Option<Opened>,
}

struct Row {
    hit: LineHit,
    path: String,
}

struct Running {
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    started: std::time::Instant,
}

struct Finished {
    files: usize,
    stopped: bool,
    seconds: f32,
}

#[derive(Default)]
pub struct SearchPage {
    open: bool,
    focus: bool,
    text: String,
    scope: String,
    match_case: bool,
    tables: bool,
    rows: Vec<Row>,
    running: Option<Running>,
    progress: (usize, usize),
    finished: Option<Finished>,
    /// The text the listed matches were found for.
    searched: String,
}

/// A match's line, in a fixed-width hand, with the found text picked out.
fn line_text(dark_mode: bool, hit: &LineHit, width: f32) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(11.5);
    let ink = shade(dark_mode, [190, 190, 198], [50, 50, 58]);
    let mut job = egui::text::LayoutJob::default();
    let mark = hit.mark.start.min(hit.text.len())..hit.mark.end.min(hit.text.len());
    job.append(&hit.text[..mark.start], 0.0, egui::TextFormat::simple(font.clone(), ink));
    let found = egui::TextFormat { background: shade(dark_mode, [66, 54, 14], [253, 230, 138]), ..egui::TextFormat::simple(font.clone(), shade(dark_mode, [253, 224, 120], [60, 44, 0])) };
    job.append(&hit.text[mark.clone()], 0.0, found);
    job.append(&hit.text[mark.end..], 0.0, egui::TextFormat::simple(font, ink));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

/// The file a match is in: its name, then the line number. A table's strings have no lines.
fn name_text(dark_mode: bool, path: &str, line: u32, width: f32) -> egui::text::LayoutJob {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat::simple(egui::FontId::proportional(12.5), shade(dark_mode, [236, 236, 240], [24, 24, 28])));
    if line > 0 {
        job.append(&format!(":{}", line), 0.0, egui::TextFormat::simple(egui::FontId::monospace(11.0), shade(dark_mode, [118, 118, 128], [120, 120, 132])));
    }
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

impl SearchPage {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Forgets the last search: its matches name files of the install that was open then.
    pub fn reset(&mut self) {
        self.stop();
        self.rows.clear();
        self.finished = None;
        self.progress = (0, 0);
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn start(&mut self, ctx: &egui::Context, sources: &Sources) {
        self.reset();
        self.searched = self.text.trim().to_string();
        let query = ContentQuery { text: self.searched.clone(), scope: Query::parse(&self.scope), match_case: self.match_case, tables: self.tables };
        let (tx, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(Running { events, cancel: Arc::clone(&cancel), started: std::time::Instant::now() });
        let (index, reader, steam, ctx) = (Arc::clone(sources.index), sources.reader.cloned(), sources.steam.cloned(), ctx.clone());
        std::thread::spawn(move || {
            crate::content_search::run(&index, reader.as_deref(), steam.as_ref(), &query, &cancel, &|event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            });
        });
    }

    fn poll(&mut self, index: &Index) {
        let Some(running) = &self.running else { return };
        let mut done = None;
        for event in running.events.try_iter().take(400) {
            match event {
                Event::Hits(hits) => self.rows.extend(hits.into_iter().filter_map(|hit| Some(Row { path: index.files.get(&hit.hash)?.path.clone(), hit }))),
                Event::Progress { files, of } => self.progress = (self.progress.0.max(files), of),
                Event::Done { files, stopped } => done = Some(Finished { files, stopped, seconds: running.started.elapsed().as_secs_f32() }),
            }
        }
        if done.is_some() {
            self.running = None;
            self.finished = done;
            self.rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.hit.line.cmp(&b.hit.line)));
        }
    }

    fn status(&self) -> String {
        let matches = thousands(self.rows.len());
        match (&self.running, &self.finished) {
            (Some(_), _) => format!("Read {} of {} files · {} matches", thousands(self.progress.0), thousands(self.progress.1), matches),
            (None, Some(finished)) => {
                let capped = if finished.stopped && self.rows.len() >= MOST_HITS { " · stopped here: narrow the search to see the rest" } else if finished.stopped { " · stopped" } else { "" };
                format!("{} matches in {} files · {:.1} s{}", matches, thousands(finished.files), finished.seconds, capped)
            }
            (None, None) => "Text files and, if ticked, the strings inside data tables. Textures, meshes and audio are skipped.".to_string(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, sources: Option<Sources>) -> SearchActions {
        let mut actions = SearchActions::default();
        let dark_mode = ui.visuals().dark_mode;
        ui.horizontal(|ui| {
            ui.heading("Search in files");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    self.open = false;
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(6.0);

        let Some(sources) = sources else {
            ui.label(hint(ui, "Open a GGPK or a Steam folder first."));
            return actions;
        };
        self.poll(sources.index);

        let mut go = false;
        egui::Grid::new("search_fields").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            ui.label(egui::RichText::new("Find").size(12.5));
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut self.text).hint_text(hint(ui, "Text to look for, such as Hellion")).desired_width(420.0));
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                go |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                match self.running.is_some() {
                    true => {
                        if action(ui, "Stop", Kind::Secondary, true).clicked() {
                            self.stop();
                            self.finished = Some(Finished { files: self.progress.0, stopped: true, seconds: 0.0 });
                        }
                    }
                    false => go |= action(ui, "Search", Kind::Primary, self.text.trim().chars().count() >= 2).clicked(),
                }
            });
            ui.end_row();
            ui.label(egui::RichText::new("In").size(12.5));
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut self.scope).hint_text(hint(ui, "Every text file. Narrow with *.ao, metadata/ or !french")).desired_width(420.0));
                go |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.checkbox(&mut self.match_case, "Match case");
                ui.checkbox(&mut self.tables, "Data tables").on_hover_text("Also read the strings inside DAT tables");
            });
            ui.end_row();
        });
        if go && self.text.trim().chars().count() >= 2 {
            self.start(ui.ctx(), &sources);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.running.is_some() {
                ui.spinner();
            }
            ui.label(hint(ui, &self.status()).size(12.0));
        });
        if self.running.is_some() && self.progress.1 > 0 {
            ui.add(egui::ProgressBar::new(self.progress.0 as f32 / self.progress.1 as f32).desired_height(3.0));
        }
        ui.add_space(4.0);
        if self.rows.is_empty() {
            return actions;
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
            for i in range {
                let row = &self.rows[i];
                // A file's name is written once, on the first of its matches.
                let first_of_file = i == 0 || self.rows[i - 1].hit.hash != row.hit.hash;
                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, name_text(dark_mode, &row.path, row.hit.line, f32::INFINITY).text));
                if response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, shade(dark_mode, [28, 28, 33], [236, 238, 244]));
                }
                let name_width = NAME_WIDTH.min(rect.width() * 0.4);
                if first_of_file {
                    crate::ui::file_kind::dot(ui.painter(), egui::pos2(rect.left() + 12.0, rect.center().y), crate::ui::file_kind::FileKind::of(&row.path), dark_mode);
                    let galley = ui.painter().layout_job(name_text(dark_mode, &row.path, row.hit.line, name_width - 32.0));
                    ui.painter().galley(egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0), galley, egui::Color32::PLACEHOLDER);
                } else if row.hit.line > 0 {
                    let galley = ui.painter().layout_no_wrap(format!(":{}", row.hit.line), egui::FontId::monospace(11.0), shade(dark_mode, [92, 92, 100], [150, 150, 160]));
                    ui.painter().galley(egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0), galley, egui::Color32::PLACEHOLDER);
                }
                let galley = ui.painter().layout_job(line_text(dark_mode, &row.hit, rect.width() - name_width - 16.0));
                ui.painter().galley(egui::pos2(rect.left() + name_width, rect.center().y - galley.size().y / 2.0), galley, egui::Color32::PLACEHOLDER);
                if response.on_hover_text(row.path.as_str()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    actions.open = Some(Opened { hash: row.hit.hash, line: row.hit.line, text: self.searched.clone() });
                }
            }
        });
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_is_written_with_the_found_text_picked_out() {
        let hit = LineHit { hash: 1, line: 12, text: "foo = \"Hellion roar\"".to_string(), mark: 7..14 };
        let job = line_text(true, &hit, 600.0);
        assert_eq!(job.text, hit.text);
        let lit: Vec<&str> = job.sections.iter().filter(|section| section.format.background != egui::Color32::TRANSPARENT).map(|section| &job.text[section.byte_range.clone()]).collect();
        assert_eq!(lit, ["Hellion"]);
        assert_eq!(name_text(true, "metadata/monsters/hellion/hellion.ao", 12, 300.0).text, "hellion.ao:12");
        assert_eq!(name_text(true, "data/mods.datc64", 0, 300.0).text, "mods.datc64");
    }

    #[test]
    fn counts_are_written_with_thousands_separators() {
        assert_eq!([thousands(0), thousands(999), thousands(1000), thousands(1234567)], ["0", "999", "1,000", "1,234,567"]);
    }

    #[test]
    fn the_status_says_what_a_search_found_and_when_it_stopped_short() {
        let mut page = SearchPage::default();
        assert!(page.status().starts_with("Text files"));
        page.rows = (0..3).map(|i| Row { hit: LineHit { hash: i, line: 1, text: String::new(), mark: 0..0 }, path: String::new() }).collect();
        page.finished = Some(Finished { files: 1200, stopped: false, seconds: 1.3 });
        assert_eq!(page.status(), "3 matches in 1,200 files · 1.3 s");
    }
}
