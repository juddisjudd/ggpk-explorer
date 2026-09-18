//! Options for the semantic data export: which dumps to write, whether to
//! pull the art with them, and where they land.

use crate::data_export::DataExportOptions;
use crate::settings::Game;
use crate::ui::components::modal_section;
use eframe::egui;

/// One dump in the list: its name, what it holds, whether it is ticked, and
/// why this game leaves it out.
struct ModuleChoice {
    name: &'static str,
    summary: &'static str,
    on: bool,
    skipped: Option<&'static str>,
}

pub struct DataExportWindow {
    open: bool,
    /// Which game the open install is, so dumps the other game owns are shown as such.
    game: Game,
    /// Selected state per module, in registry order.
    modules: Vec<ModuleChoice>,
    images: bool,
    trade_stats: bool,
    /// Leave null-valued keys out of the JSON.
    strip_null: bool,
    /// Put the export in a folder named after the patch.
    versioned: bool,
    /// Patch read from the install, if the client log named one.
    version: Option<String>,
}

impl Default for DataExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            game: Game::default(),
            modules: crate::data_export::registry()
                .into_iter()
                .map(|m| ModuleChoice { name: m.name, summary: m.summary, on: true, skipped: None })
                .collect(),
            images: false,
            trade_stats: false,
            strip_null: false,
            versioned: true,
            version: None,
        }
    }
}

impl DataExportWindow {
    pub fn open_with(&mut self, version: Option<String>, game: Game) {
        self.open = true;
        self.version = version;
        self.game = game;
        // Every dump this game writes starts ticked: a dump greyed out for the
        // other game must not stay unticked once it applies again.
        for (module, choice) in crate::data_export::registry().into_iter().zip(&mut self.modules) {
            choice.skipped = module.skip_reason(game);
            choice.on = choice.skipped.is_none();
        }
    }

    /// The dumps this game can write.
    fn available(&self) -> impl Iterator<Item = &ModuleChoice> {
        self.modules.iter().filter(|m| m.skipped.is_none())
    }

    /// The options as chosen. `only` is left empty when everything is ticked,
    /// so the export runs its full set rather than a list that happens to
    /// match.
    pub fn options(&self) -> DataExportOptions {
        let all = self.available().all(|m| m.on);
        DataExportOptions {
            only: if all {
                Vec::new()
            } else {
                self.available().filter(|m| m.on).map(|m| m.name.to_string()).collect()
            },
            images: self.images,
            trade_stats: self.trade_stats,
            version: self.versioned.then(|| self.version.clone()).flatten(),
            flat: !self.versioned,
            strip_null: self.strip_null,
        }
    }

    fn selected(&self) -> usize {
        self.available().filter(|m| m.on).count()
    }

    /// Draws the dialog; true means the user asked to export.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = self.open;
        if !open {
            return false;
        }
        let mut confirmed = false;
        let mut should_close = false;

        egui::Window::new("Export Game Data")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                let muted = if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(113, 113, 122)
                } else {
                    egui::Color32::from_rgb(80, 80, 90)
                };

                ui.label(
                    egui::RichText::new(
                        "Joined JSON for mods, skills, base items and stat text — the shape RePoE publishes.",
                    )
                    .size(11.5)
                    .color(muted),
                );
                let skipped = self.modules.len() - self.available().count();
                let reading = match skipped {
                    0 => format!("Reading {}", self.game.label()),
                    n => format!("Reading {} — {} dumps are the other game's", self.game.label(), n),
                };
                ui.label(egui::RichText::new(reading).size(11.5).color(muted));

                ui.separator();
                modal_section(ui, "DESTINATION");
                match &self.version {
                    Some(version) => {
                        let folder = self.options().folder_name(version);
                        ui.checkbox(&mut self.versioned, format!("Put it in a folder named {}", folder))
                            .on_hover_text(
                                "A stripped export lands in its own folder, so it sits beside the \
                                 full one instead of overwriting it.",
                            );
                    }
                    None => {
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Patch folder"));
                        ui.label(
                            egui::RichText::new("Could not read the patch from the client log")
                                .size(10.5)
                                .color(muted),
                        );
                    }
                }

                ui.separator();
                modal_section(ui, "EXTRAS");
                ui.checkbox(&mut self.images, "Include item, skill and buff icons")
                    .on_hover_text(
                        "Exports the art the dumps point at, as PNG and WebP under the same paths \
                         the game uses. About 5,000 images, and roughly a minute.",
                    );
                ui.checkbox(&mut self.trade_stats, "Add trade site search ids to stat text")
                    .on_hover_text(
                        "Looks each stat's wording up on the official trade site and records the ids \
                         it searches under, so a mod can be turned into a trade filter. \
                         Matches about 400 stats; needs the site to be reachable.",
                    );
                ui.checkbox(&mut self.strip_null, "Leave out keys with no value")
                    .on_hover_text(
                        "Drops every null from the JSON, so an entry lists only what it has — an \
                         amulet stops carrying the 28 weapon and armour fields it has no use for. \
                         Halves base_items.json and takes about 14% off the whole export. Off by \
                         default, because the published files keep the nulls.",
                    );

                ui.separator();
                ui.horizontal(|ui| {
                    modal_section(ui, "DUMPS");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("None").clicked() {
                            self.modules.iter_mut().for_each(|m| m.on = false);
                        }
                        if ui.small_button("All").clicked() {
                            self.modules.iter_mut().for_each(|m| m.on = m.skipped.is_none());
                        }
                    });
                });

                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    for module in &mut self.modules {
                        let enabled = module.skipped.is_none();
                        ui.add_enabled(enabled, egui::Checkbox::new(&mut module.on, module.name))
                            .on_hover_text(module.skipped.unwrap_or(module.summary));
                    }
                });

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("SELECTED · {} of {}", self.selected(), self.available().count()))
                        .monospace()
                        .size(10.5)
                        .color(muted),
                );

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = self.selected() > 0;
                        if ui
                            .add_enabled(ready, egui::Button::new("Choose folder and export"))
                            .clicked()
                        {
                            confirmed = true;
                            should_close = true;
                        }
                    });
                });
            });

        if should_close {
            open = false;
        }
        self.open = open;
        confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> DataExportWindow {
        DataExportWindow { version: Some("4.5.4.11".into()), ..Default::default() }
    }

    /// Opening the dialog on one game and then the other must not leave the
    /// first game's dumps unticked, or that export quietly runs a short list.
    #[test]
    fn switching_game_restores_the_dumps_that_apply_again() {
        let mut w = window();
        w.open_with(Some("3.29.3.3".into()), Game::Poe1);
        assert!(w.modules.iter().any(|m| m.name == "augments" && m.skipped.is_some() && !m.on));
        assert!(w.options().only.is_empty(), "a full PoE 1 selection runs every PoE 1 dump");

        w.open_with(Some("4.5.5.2".into()), Game::Poe2);
        let augments = w.modules.iter().find(|m| m.name == "augments").unwrap();
        assert!(augments.skipped.is_none() && augments.on, "augments is a PoE 2 dump and comes back ticked");
        assert!(w.options().only.is_empty(), "a full PoE 2 selection runs every PoE 2 dump");
    }

    #[test]
    fn everything_ticked_asks_for_no_filter() {
        let options = window().options();
        assert!(options.only.is_empty(), "a full selection runs the whole set");
        assert_eq!(options.version.as_deref(), Some("4.5.4.11"));
        assert!(!options.flat);
    }

    #[test]
    fn a_subset_is_passed_through_by_name() {
        let mut w = window();
        w.modules.iter_mut().for_each(|m| m.on = m.name == "mods");
        assert_eq!(w.options().only, vec!["mods".to_string()]);
    }

    #[test]
    fn a_stripped_export_gets_its_own_patch_folder() {
        let mut w = window();
        assert_eq!(w.options().folder_name("4.5.4.11"), "4.5.4.11");
        w.strip_null = true;
        assert_eq!(w.options().folder_name("4.5.4.11"), "4.5.4.11-stripped");
        assert!(w.options().strip_null);
    }

    #[test]
    fn unticking_the_patch_folder_writes_flat() {
        let mut w = window();
        w.versioned = false;
        let options = w.options();
        assert!(options.flat);
        assert_eq!(options.version, None);
    }
}
