//! Options for the semantic data export: which dumps to write, whether to
//! pull the art with them, and where they land.

use crate::data_export::DataExportOptions;
use crate::ui::components::modal_section;
use eframe::egui;

pub struct DataExportWindow {
    open: bool,
    /// Selected state per module, in registry order.
    modules: Vec<(&'static str, &'static str, bool)>,
    images: bool,
    trade_stats: bool,
    /// Put the export in a folder named after the patch.
    versioned: bool,
    /// Patch read from the install, if the client log named one.
    version: Option<String>,
}

impl Default for DataExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            modules: crate::data_export::registry()
                .into_iter()
                .map(|m| (m.name, m.summary, true))
                .collect(),
            images: false,
            trade_stats: false,
            versioned: true,
            version: None,
        }
    }
}

impl DataExportWindow {
    pub fn open_with(&mut self, version: Option<String>) {
        self.open = true;
        self.version = version;
    }

    /// The options as chosen. `only` is left empty when everything is ticked,
    /// so the export runs its full set rather than a list that happens to
    /// match.
    pub fn options(&self) -> DataExportOptions {
        let all = self.modules.iter().all(|(_, _, on)| *on);
        DataExportOptions {
            only: if all {
                Vec::new()
            } else {
                self.modules.iter().filter(|(_, _, on)| *on).map(|(name, _, _)| name.to_string()).collect()
            },
            images: self.images,
            trade_stats: self.trade_stats,
            version: self.versioned.then(|| self.version.clone()).flatten(),
            flat: !self.versioned,
        }
    }

    fn selected(&self) -> usize {
        self.modules.iter().filter(|(_, _, on)| *on).count()
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

                ui.separator();
                modal_section(ui, "DESTINATION");
                match &self.version {
                    Some(version) => {
                        ui.checkbox(&mut self.versioned, format!("Put it in a folder named {}", version));
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

                ui.separator();
                ui.horizontal(|ui| {
                    modal_section(ui, "DUMPS");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("None").clicked() {
                            self.modules.iter_mut().for_each(|(_, _, on)| *on = false);
                        }
                        if ui.small_button("All").clicked() {
                            self.modules.iter_mut().for_each(|(_, _, on)| *on = true);
                        }
                    });
                });

                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    for (name, summary, on) in &mut self.modules {
                        ui.checkbox(on, *name).on_hover_text(*summary);
                    }
                });

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("SELECTED · {} of {}", self.selected(), self.modules.len()))
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
        w.modules.iter_mut().for_each(|(name, _, on)| *on = *name == "mods");
        assert_eq!(w.options().only, vec!["mods".to_string()]);
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
