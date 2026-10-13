//! The patch page: what the last check of the open patch found, and what was done about it.

use crate::patch_check::Report;
use crate::settings::Game;
use crate::ui::components::{card, modal_section};
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Good,
    Note,
    Warn,
    Bad,
    Idle,
}

impl Tone {
    pub fn color(self, dark_mode: bool) -> egui::Color32 {
        let (dark, light) = match self {
            Tone::Good => ((74, 222, 128), (22, 163, 74)),
            Tone::Note => ((96, 165, 250), (37, 99, 235)),
            Tone::Warn => ((251, 191, 36), (180, 120, 0)),
            Tone::Bad => ((248, 113, 113), (220, 38, 38)),
            Tone::Idle => ((113, 113, 122), (120, 120, 130)),
        };
        let (r, g, b) = if dark_mode { dark } else { light };
        egui::Color32::from_rgb(r, g, b)
    }
}

/// How the open patch stands, in a few words.
#[derive(Clone, PartialEq, Debug)]
pub struct Health {
    pub label: String,
    pub tone: Tone,
}

fn count(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {}", noun),
        n => format!("{} {}s", n, noun),
    }
}

/// The worst thing the check found decides the line: broken, then re-fitted, then read differently by the client.
pub fn health(report: Option<&Report>, running: bool) -> Health {
    let (label, tone) = match report {
        _ if running => ("Checking patch…".to_string(), Tone::Idle),
        None => ("Patch not checked".to_string(), Tone::Idle),
        Some(r) if !r.unresolved.is_empty() => (format!("{} broken", count(r.unresolved.len(), "table")), Tone::Bad),
        Some(r) if !r.refitted.is_empty() => (format!("{} re-fitted", count(r.refitted.len(), "table")), Tone::Warn),
        Some(r) if !r.disputed.is_empty() => (format!("{} read differently", count(r.disputed.len(), "table")), Tone::Note),
        Some(_) => ("Patch checked".to_string(), Tone::Good),
    };
    Health { label, tone }
}

fn muted(ui: &egui::Ui) -> egui::Color32 {
    match ui.visuals().dark_mode {
        true => egui::Color32::from_rgb(140, 140, 150),
        false => egui::Color32::from_rgb(80, 80, 90),
    }
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.5).color(muted(ui)));
}

/// A dot in the tone's colour followed by the label.
pub fn health_line(ui: &mut egui::Ui, health: &Health, size: f32) {
    let color = health.tone.color(ui.visuals().dark_mode);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, size), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, color);
        ui.label(egui::RichText::new(&health.label).size(size).color(color));
    });
}

fn tile(ui: &mut egui::Ui, value: &str, label: &str, tone: Option<Tone>) {
    let color = tone.map_or(ui.visuals().text_color(), |t| t.color(ui.visuals().dark_mode));
    egui::Frame::none()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .rounding(egui::Rounding::same(6.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_min_width(96.0);
                ui.label(egui::RichText::new(value).size(20.0).strong().color(color));
                ui.label(egui::RichText::new(label).size(10.0).monospace().color(muted(ui)));
            });
        });
}

pub struct PatchView<'a> {
    /// The game of the open install, if one is open.
    pub game: Option<Game>,
    pub version: &'a str,
    pub report: Option<&'a Report>,
    pub running: bool,
    pub checked_at: Option<&'a str>,
}

#[derive(Default)]
pub struct PatchActions {
    pub check_again: bool,
    pub open_folder: bool,
    /// A table name the user clicked, to open in the viewer.
    pub open_table: Option<String>,
}

#[derive(Default)]
pub struct PatchPage {
    open: bool,
}

impl PatchPage {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, view: &PatchView) -> PatchActions {
        let mut actions = PatchActions::default();
        ui.horizontal(|ui| {
            ui.heading("Patch");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    self.open = false;
                }
                let ready = view.game.is_some() && !view.version.is_empty() && !view.running;
                let again = ui.add_enabled(ready, egui::Button::new("Check again"));
                if again.on_hover_text("Check every table against the schema and the game client, and re-fit what moved").clicked() {
                    actions.check_again = true;
                }
                if view.report.is_some() && ui.button("Open folder").on_hover_text("The kept tables, the client's layouts and this report").clicked() {
                    actions.open_folder = true;
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        let Some(game) = view.game else {
            ui.add_space(8.0);
            note(ui, "Open a GGPK or a Steam folder to check its patch.");
            return actions;
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} · patch {}", game.label(), if view.version.is_empty() { "unknown" } else { view.version })).size(15.0).strong());
            ui.add_space(8.0);
            health_line(ui, &health(view.report, view.running), 12.5);
        });
        if view.running {
            ui.horizontal(|ui| {
                ui.spinner();
                note(ui, "Checking every table against the schema and the game client…");
            });
        }
        let Some(report) = view.report else {
            if !view.running {
                ui.add_space(4.0);
                note(ui, "This patch has not been checked yet. A check keeps its tables, tests each one against the schema, and re-fits what the patch moved.");
            }
            return actions;
        };
        let from = report.previous.as_deref().map(|v| format!(" · re-fits come from patch {}", v)).unwrap_or_default();
        note(ui, &format!("{}{}", view.checked_at.map(|at| format!("Checked {}", at)).unwrap_or_else(|| "Checked".to_string()), from));

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let flag = |n: usize, tone: Tone| (n > 0).then_some(tone);
            tile(ui, &report.checked.to_string(), "TABLES CHECKED", None);
            tile(ui, &report.broken.len().to_string(), "MOVED", flag(report.broken.len(), Tone::Warn));
            tile(ui, &report.refitted.len().to_string(), "RE-FITTED", flag(report.refitted.len(), Tone::Warn));
            tile(ui, &report.unresolved.len().to_string(), "STILL BROKEN", flag(report.unresolved.len(), Tone::Bad));
            tile(ui, &report.disputed.len().to_string(), "READ DIFFERENTLY", flag(report.disputed.len(), Tone::Note));
            let client = match &report.client {
                Some(Ok(n)) => n.to_string(),
                _ => "none".to_string(),
            };
            tile(ui, &client, "CLIENT LAYOUTS", None);
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // The theme overrides text colour, so a link only looks like one when its text is coloured.
            let blue = Tone::Note.color(ui.visuals().dark_mode);
            let mut link = |ui: &mut egui::Ui, name: &str| {
                if ui.link(egui::RichText::new(name).size(13.0).color(blue)).on_hover_text("Open this table").clicked() {
                    actions.open_table = Some(name.to_string());
                }
            };

            if !report.unresolved.is_empty() {
                modal_section(ui, &format!("STILL BROKEN · {}", report.unresolved.len()));
                note(ui, "The layout in use misreads these tables, and they could not be put right from an earlier patch.");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (name, why) in &report.unresolved {
                        ui.horizontal_wrapped(|ui| {
                            link(ui, name);
                            ui.label(egui::RichText::new(why).size(12.0).color(Tone::Bad.color(ui.visuals().dark_mode)));
                        });
                    }
                });
            }

            if !report.refitted.is_empty() {
                modal_section(ui, &format!("RE-FITTED · {}", report.refitted.len()));
                let saved = if report.written { "They are saved as custom layouts until the community schema catches up." } else { "They were not saved." };
                note(ui, &format!("The schema misread these tables; their columns were carried over from the previous patch, or lined up with the game client's own reading. {}", saved));
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (name, lost) in &report.refitted {
                        ui.horizontal_wrapped(|ui| {
                            link(ui, name);
                            if report.repaired.contains(name) {
                                ui.label(egui::RichText::new("from the game client").size(12.0).color(ui.visuals().weak_text_color()));
                            }
                            if !lost.is_empty() {
                                ui.label(egui::RichText::new(format!("could not place {}", lost.join(", "))).size(12.0).color(Tone::Warn.color(ui.visuals().dark_mode)));
                            }
                        });
                    }
                });
            }

            if !report.retired.is_empty() {
                modal_section(ui, &format!("RETIRED · {}", report.retired.len()));
                note(ui, "Earlier re-fits that were dropped, because the community schema reads these tables again.");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| report.retired.iter().for_each(|name| link(ui, name)));
                });
            }

            match &report.client {
                Some(Err(why)) => {
                    modal_section(ui, "GAME CLIENT");
                    note(ui, &format!("No table layouts could be read from the game client: {}", why));
                }
                Some(Ok(_)) if report.mismatched > 0 => {
                    modal_section(ui, "GAME CLIENT");
                    note(ui, &format!("{} have rows of a size the client does not expect. The client may be from another patch.", count(report.mismatched, "table")));
                }
                _ => {}
            }

            if !report.disputed.is_empty() {
                modal_section(ui, &format!("READ DIFFERENTLY BY THE GAME CLIENT · {}", report.disputed.len()));
                note(ui, "These tables open, but the game's own loader reads the columns below another way than the layout in use. Treat those columns as suspect.");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, (name, found)) in report.disputed.iter().enumerate() {
                        if i > 0 {
                            ui.separator();
                        }
                        link(ui, name);
                        for line in found {
                            ui.label(egui::RichText::new(line).size(11.5).monospace().color(muted(ui)));
                        }
                    }
                });
            }

            if report.unresolved.is_empty() && report.refitted.is_empty() && report.disputed.is_empty() {
                ui.add_space(6.0);
                note(ui, "Every table reads the way the layout in use describes it.");
            }
        });
        actions
    }
}

#[cfg(test)]
pub(crate) fn painted_text(draw: impl Fn(&mut egui::Ui)) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| draw(ui));
    });
    fn walk(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = String::new();
    output.shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            version: "0.5.5.4".to_string(),
            previous: Some("0.5.5.3".to_string()),
            checked: 823,
            broken: vec!["Stats".to_string(), "Mods".to_string()],
            refitted: vec![("Stats".to_string(), vec!["IsLocal".to_string()])],
            unresolved: vec![("Mods".to_string(), "not in both patches".to_string())],
            written: true,
            client: Some(Ok(801)),
            disputed: vec![("NPCTalk".to_string(), vec!["byte 300: the client reads a key to WorldAreas".to_string()])],
            ..Default::default()
        }
    }

    #[test]
    fn the_worst_finding_names_the_health() {
        let mut r = report();
        assert_eq!(health(Some(&r), false), Health { label: "1 table broken".to_string(), tone: Tone::Bad });
        r.unresolved.clear();
        assert_eq!(health(Some(&r), false).label, "1 table re-fitted");
        r.refitted.clear();
        assert_eq!(health(Some(&r), false), Health { label: "1 table read differently".to_string(), tone: Tone::Note });
        r.disputed.clear();
        assert_eq!(health(Some(&r), false).tone, Tone::Good);
        assert_eq!(health(Some(&r), true).label, "Checking patch…");
        assert_eq!(health(None, false).label, "Patch not checked");
    }

    #[test]
    fn the_page_lists_what_the_check_found() {
        let r = report();
        let view = PatchView { game: Some(Game::Poe2), version: "0.5.5.4", report: Some(&r), running: false, checked_at: Some("2026-10-13 09:13") };
        let text = painted_text(|ui| {
            PatchPage { open: true }.show(ui, &view);
        });
        for expected in ["PoE 2 · patch 0.5.5.4", "Checked 2026-10-13 09:13 · re-fits come from patch 0.5.5.3", "823", "STILL BROKEN · 1", "not in both patches", "could not place IsLocal", "NPCTalk", "byte 300: the client reads a key to WorldAreas"] {
            assert!(text.contains(expected), "missing {:?} in:\n{}", expected, text);
        }
    }

    #[test]
    fn an_unchecked_patch_says_so() {
        let view = PatchView { game: Some(Game::Poe1), version: "3.29.3.3", report: None, running: false, checked_at: None };
        let text = painted_text(|ui| {
            PatchPage { open: true }.show(ui, &view);
        });
        assert!(text.contains("has not been checked yet"), "{}", text);
        let nothing = PatchView { game: None, version: "", report: None, running: false, checked_at: None };
        let text = painted_text(|ui| {
            PatchPage { open: true }.show(ui, &nothing);
        });
        assert!(text.contains("Open a GGPK or a Steam folder"), "{}", text);
    }
}
