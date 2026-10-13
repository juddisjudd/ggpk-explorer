//! What the app shows with an install open and nothing selected: where the patch stands and what can be done with it.

use crate::settings::Game;
use crate::ui::components::{card, modal_section};
use crate::ui::patch_page::{health_line, Health};
use eframe::egui;

const COLUMN: f32 = 680.0;

pub struct HomeView<'a> {
    pub game: Game,
    pub version: &'a str,
    /// The GGPK or the Steam folder being read.
    pub source: &'a str,
    pub files: usize,
    pub schema_date: &'a str,
    pub health: &'a Health,
    /// The last check in one line, when there is one.
    pub headline: Option<&'a str>,
}

#[derive(Default)]
pub struct HomeActions {
    pub open_patch: bool,
    pub open_palette: bool,
    pub export_data: bool,
    pub export_pob: bool,
    pub export_tables: bool,
    pub open_diff: bool,
    /// A kind of file the user clicked, to filter the tree to.
    pub filter_kind: Option<crate::ui::file_kind::FileKind>,
}

fn muted(ui: &egui::Ui) -> egui::Color32 {
    match ui.visuals().dark_mode {
        true => egui::Color32::from_rgb(140, 140, 150),
        false => egui::Color32::from_rgb(80, 80, 90),
    }
}

/// A button of fixed width with what it does beside it.
fn action(ui: &mut egui::Ui, label: &str, does: &str) -> bool {
    ui.horizontal(|ui| {
        let clicked = ui.add_sized([150.0, 26.0], egui::Button::new(label)).clicked();
        ui.label(egui::RichText::new(does).size(12.0).color(muted(ui)));
        clicked
    })
    .inner
}

pub fn show(ui: &mut egui::Ui, view: &HomeView) -> HomeActions {
    let mut actions = HomeActions::default();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let width = COLUMN.min(ui.available_width() - 24.0).max(320.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 5.0;
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(view.game.label()).size(22.0).strong());
                    let patch = if view.version.is_empty() { "patch unknown".to_string() } else { format!("patch {}", view.version) };
                    ui.label(egui::RichText::new(patch).size(14.0).color(muted(ui)));
                });
                ui.label(egui::RichText::new(view.source).size(11.5).monospace().color(muted(ui)));
                ui.label(egui::RichText::new(format!("{} files · schema {}", crate::ui::components::thousands(view.files), view.schema_date)).size(11.5).color(muted(ui)));

                modal_section(ui, "PATCH");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    health_line(ui, view.health, 13.0);
                    if let Some(headline) = view.headline {
                        ui.label(egui::RichText::new(headline).size(12.0).color(muted(ui)));
                    }
                    ui.add_space(2.0);
                    actions.open_patch = action(ui, "Patch report", "what the patch moved, what was re-fitted, and where the game client disagrees");
                });

                modal_section(ui, "FIND");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    actions.open_palette = action(ui, "Search files", "Ctrl+K, or browse the tree on the left");
                });

                modal_section(ui, "FILE TYPES");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Each file carries the colour of its kind. Click one to show only those files in the tree.").size(12.0).color(muted(ui)));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(14.0, 4.0);
                        for kind in crate::ui::file_kind::FileKind::NAMED {
                            let chip = ui.add(egui::Button::new(egui::RichText::new(format!("    {}", kind.name())).size(12.5)).frame(false));
                            crate::ui::file_kind::dot(ui.painter(), egui::pos2(chip.rect.left() + 6.0, chip.rect.center().y), kind, ui.visuals().dark_mode);
                            if chip.on_hover_text(kind.query()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                actions.filter_kind = Some(kind);
                            }
                        }
                    });
                });

                modal_section(ui, "EXPORT");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    actions.export_data = action(ui, "Game data…", "mods, skills, base items and stat translations as JSON");
                    actions.export_pob = action(ui, "PoB data…", "Path of Building's data files");
                    actions.export_tables = action(ui, "DAT tables…", "every table whole, as JSON or CSV");
                });

                modal_section(ui, "COMPARE");
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    actions.open_diff = action(ui, "Diff patches", "what a patch added, removed or changed");
                });

                ui.add_space(18.0);
            });
        });
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::patch_page::{painted_text, Tone};

    #[test]
    fn the_home_view_names_the_patch_and_what_can_be_done() {
        let health = Health { label: "37 tables read differently".to_string(), tone: Tone::Note };
        let view = HomeView { game: Game::Poe2, version: "0.5.5.4", source: "S:/Path of Exile 2/Content.ggpk", files: 912_345, schema_date: "2026-09-15", health: &health, headline: Some("Patch 0.5.5.4: 823 tables checked, nothing moved") };
        let text = painted_text(|ui| {
            show(ui, &view);
        });
        for expected in ["PoE 2", "patch 0.5.5.4", "912,345 files · schema 2026-09-15", "37 tables read differently", "823 tables checked", "Patch report", "Search files", "Game data…", "PoB data…", "DAT tables…", "Diff patches"] {
            assert!(text.contains(expected), "missing {:?} in:\n{}", expected, text);
        }
    }
}
