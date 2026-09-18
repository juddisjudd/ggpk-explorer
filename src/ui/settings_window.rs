//! The settings page. It fills the main area rather than a modal, with a
//! section per area down the side, so each game can show its own paths, patch
//! and cache.

use crate::settings::{AppSettings, Game};
use crate::ui::components::{card, modal_section};
use eframe::egui;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

#[derive(PartialEq)]
pub enum SchemaUpdateStatus {
    Checking,
    UpToDate,
    UpdateAvailable,
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Games,
    General,
    Network,
    Schema,
    Cache,
}

impl Section {
    const ALL: [Section; 5] = [Section::Games, Section::General, Section::Network, Section::Schema, Section::Cache];

    fn label(self) -> &'static str {
        match self {
            Section::Games => "Games",
            Section::General => "General",
            Section::Network => "Network",
            Section::Schema => "Schema",
            Section::Cache => "Cache",
        }
    }
}

const GAMES: [Game; 2] = [Game::Poe2, Game::Poe1];

pub struct SettingsWindow {
    open: bool,
    section: Section,
    fetch_rx: Option<Receiver<Result<String, String>>>,
    is_fetching: bool,
    fetch_status_msg: Option<String>,
    pub request_update_schema: bool,
    pub schema_status_msg: Option<String>,
    pub schema_update_status: SchemaUpdateStatus,
    /// Cache size per game, indexed by `Game::is_poe2()`, once it is measured.
    cache_sizes: [Option<u64>; 2],
    cache_rx: Option<Receiver<(bool, u64)>>,
    cache_status_msg: Option<String>,
    /// The game the user just picked, for the app to open that install.
    pub requested_game: Option<Game>,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            open: false,
            section: Section::Games,
            fetch_rx: None,
            is_fetching: false,
            fetch_status_msg: None,
            request_update_schema: false,
            schema_status_msg: None,
            schema_update_status: SchemaUpdateStatus::Checking,
            cache_sizes: [None, None],
            cache_rx: None,
            cache_status_msg: None,
            requested_game: None,
        }
    }
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.cache_status_msg = None;
        self.measure_caches();
    }

    /// Sizes both games' caches off the UI thread; each answer arrives on its own.
    fn measure_caches(&mut self) {
        self.cache_sizes = [None, None];
        let (tx, rx) = channel();
        self.cache_rx = Some(rx);
        thread::spawn(move || {
            for game in GAMES {
                let _ = tx.send((game.is_poe2(), AppSettings::get_cache_size(game)));
            }
        });
    }

    fn poll(&mut self, settings: &mut AppSettings) {
        if self.is_fetching {
            if let Some(rx) = &self.fetch_rx {
                match rx.try_recv() {
                    Ok(Ok(version)) => {
                        settings.set_patch_version(Game::Poe2, version);
                        settings.save();
                        self.fetch_status_msg = Some("Updated".to_string());
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    }
                    Ok(Err(e)) => {
                        self.fetch_status_msg = Some(format!("Error: {}", e));
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.fetch_status_msg = Some("Thread died".to_string());
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    }
                }
            }
        }
        if let Some(rx) = &self.cache_rx {
            while let Ok((is_poe2, size)) = rx.try_recv() {
                self.cache_sizes[usize::from(is_poe2)] = Some(size);
            }
        }
    }

    /// Draws the page. `loaded` is the game the open install belongs to, so the
    /// cards can say which one the app is actually reading.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut AppSettings,
        schema_date: Option<&str>,
        loaded: Option<Game>,
    ) {
        self.poll(settings);

        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    settings.save();
                    self.open = false;
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                ui.add_space(6.0);
                for section in Section::ALL {
                    if ui.selectable_label(self.section == section, section.label()).clicked() {
                        self.section = section;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(420.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    match self.section {
                        Section::Games => self.games(ui, settings, loaded),
                        Section::General => Self::general(ui, settings),
                        Section::Network => self.network(ui, settings),
                        Section::Schema => self.schema(ui, settings, schema_date),
                        Section::Cache => self.cache(ui),
                    }
                });
            });
        });
    }

    fn games(&mut self, ui: &mut egui::Ui, settings: &mut AppSettings, loaded: Option<Game>) {
        modal_section(ui, "GAMES");
        note(ui, "Each game keeps its own install paths, caches and exports. Opening one switches what the app reads.");
        for game in GAMES {
            ui.add_space(4.0);
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(game.label()).size(14.0).strong());
                    if loaded == Some(game) {
                        ui.label(egui::RichText::new("LOADED").size(10.0).monospace().color(accent(ui)));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_install =
                            settings.ggpk_path_for(game).is_some() || settings.steam_path_for(game).is_some();
                        let label = if loaded == Some(game) { "Reopen" } else { "Open" };
                        if ui.add_enabled(has_install, egui::Button::new(label)).clicked() {
                            settings.game = game;
                            settings.save();
                            self.requested_game = Some(game);
                        }
                    });
                });

                let mut ggpk = settings.ggpk_path_for(game).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("GGPK").size(12.0));
                    if ui.text_edit_singleline(&mut ggpk).changed() {
                        settings.set_ggpk_path(game, (!ggpk.is_empty()).then_some(ggpk.clone()));
                    }
                    if ui.button("Browse").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("GGPK", &["ggpk"]).pick_file() {
                            settings.set_ggpk_path(game, Some(p.to_string_lossy().to_string()));
                            settings.set_steam_path(game, None);
                            settings.game = game;
                            settings.save();
                            self.requested_game = Some(game);
                        }
                    }
                });

                let mut steam = settings.steam_path_for(game).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Steam").size(12.0));
                    if ui.text_edit_singleline(&mut steam).changed() {
                        settings.set_steam_path(game, (!steam.is_empty()).then_some(steam.clone()));
                    }
                    if ui.button("Browse").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            settings.set_steam_path(game, Some(p.to_string_lossy().to_string()));
                            settings.set_ggpk_path(game, None);
                            settings.game = game;
                            settings.save();
                            self.requested_game = Some(game);
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Patch").size(12.0));
                    let mut version = settings.patch_version(game).to_string();
                    if ui.text_edit_singleline(&mut version).changed() {
                        settings.set_patch_version(game, version);
                    }
                    // The version source is PoE 2's; PoE 1's patch comes from its install log.
                    if game == Game::Poe2 {
                        if self.is_fetching {
                            ui.spinner();
                        } else if ui.button("Auto Detect").clicked() {
                            self.is_fetching = true;
                            self.fetch_status_msg = Some("Fetching...".to_string());
                            let (tx, rx) = channel();
                            self.fetch_rx = Some(rx);
                            let url = settings.patch_version_source_url.clone();
                            thread::spawn(move || {
                                let _ = tx.send(AppSettings::fetch_latest_patch_version(&url));
                            });
                        }
                        if let Some(msg) = &self.fetch_status_msg {
                            note(ui, msg);
                        }
                    } else {
                        note(ui, "read from the install's client log");
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Cache").size(12.0));
                    note(ui, &size_text(self.cache_sizes[usize::from(game.is_poe2())]));
                    if ui.small_button("Clear").clicked() {
                        match AppSettings::clear_cache(game) {
                            Ok(_) => {
                                self.cache_sizes[usize::from(game.is_poe2())] = Some(0);
                                self.cache_status_msg = Some(format!("{} cache cleared", game.label()));
                            }
                            Err(e) => self.cache_status_msg = Some(format!("Error: {}", e)),
                        }
                    }
                });
            });
        }
        if let Some(msg) = &self.cache_status_msg {
            note(ui, msg);
        }
    }

    fn general(ui: &mut egui::Ui, settings: &mut AppSettings) {
        modal_section(ui, "APPEARANCE");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Theme").size(12.5));
            egui::ComboBox::from_id_salt("theme_selector")
                .selected_text(match settings.theme.as_str() {
                    "dark" => "Premium Dark",
                    "light" => "Premium Light",
                    _ => "System Preference",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.theme, "system".to_string(), "System Preference");
                    ui.selectable_value(&mut settings.theme, "dark".to_string(), "Premium Dark");
                    ui.selectable_value(&mut settings.theme, "light".to_string(), "Premium Light");
                });
        });

        ui.separator();
        modal_section(ui, "FILE TREE");
        ui.checkbox(&mut settings.hide_shader_cache, "Hide shader cache files").on_hover_text(
            "shadercache*/ holds ~2.8 million compiled shader blobs (two thirds of the index). Hiding them cuts memory and search time.",
        );
        note(ui, "Takes effect the next time the data source is opened.");
    }

    fn network(&mut self, ui: &mut egui::Ui, settings: &mut AppSettings) {
        modal_section(ui, "NETWORK & CDN");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Version Source").size(12.5));
            ui.text_edit_singleline(&mut settings.patch_version_source_url);
        });
        ui.checkbox(&mut settings.auto_detect_patch_version, "Auto-detect latest patch version on startup");
        note(ui, "The source is PoE 2's patch server. Each game's patch is on its card under Games.");
    }

    fn schema(&mut self, ui: &mut egui::Ui, settings: &mut AppSettings, schema_date: Option<&str>) {
        modal_section(ui, "SCHEMA");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Local Path").size(12.5));
            let mut path = settings.schema_local_path.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut path).changed() {
                settings.schema_local_path = if path.is_empty() { None } else { Some(path) };
            }
            if ui.button("Browse").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                    settings.schema_local_path = Some(p.to_string_lossy().to_string());
                }
            }
        });
        if let Some(date) = schema_date {
            note(ui, &format!("Last updated: {}", date));
        }
        ui.checkbox(&mut settings.auto_update_schema, "Auto-update schema when a newer release is available");
        ui.horizontal(|ui| {
            if ui.button("Update Schema Now").clicked() {
                self.schema_status_msg = Some("Updating...".to_string());
                self.request_update_schema = true;
            }
            if let Some(msg) = &self.schema_status_msg {
                note(ui, msg);
            }
            match &self.schema_update_status {
                SchemaUpdateStatus::Checking => {
                    ui.spinner();
                }
                SchemaUpdateStatus::UpToDate => {
                    let color = if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(74, 222, 128)
                    } else {
                        egui::Color32::from_rgb(22, 163, 74)
                    };
                    ui.label(egui::RichText::new("Up to date").size(11.5).color(color));
                }
                SchemaUpdateStatus::UpdateAvailable => {
                    let color = if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(250, 204, 21)
                    } else {
                        egui::Color32::from_rgb(217, 119, 6)
                    };
                    ui.label(egui::RichText::new("Update available").size(11.5).color(color));
                }
                SchemaUpdateStatus::Error(e) => {
                    ui.label(
                        egui::RichText::new(format!("Check failed: {}", e))
                            .size(11.5)
                            .color(egui::Color32::from_rgb(239, 68, 68)),
                    );
                }
            }
        });
    }

    fn cache(&mut self, ui: &mut egui::Ui) {
        modal_section(ui, "CACHE");
        note(ui, "The bundle index, the file tree and CDN downloads, kept per game beside the app settings.");
        for game in GAMES {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(game.label()).size(12.5));
                note(ui, &size_text(self.cache_sizes[usize::from(game.is_poe2())]));
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Clear both").clicked() {
                match AppSettings::clear_cache(Game::Poe2).and(AppSettings::clear_cache(Game::Poe1)) {
                    Ok(_) => {
                        self.cache_sizes = [Some(0), Some(0)];
                        self.cache_status_msg = Some("Cache cleared".to_string());
                    }
                    Err(e) => self.cache_status_msg = Some(format!("Error: {}", e)),
                }
            }
            if ui.button("Recalculate").clicked() {
                self.measure_caches();
            }
            if let Some(msg) = &self.cache_status_msg {
                note(ui, msg);
            }
        });
    }
}

fn accent(ui: &egui::Ui) -> egui::Color32 {
    match ui.visuals().dark_mode {
        true => egui::Color32::from_rgb(74, 222, 128),
        false => egui::Color32::from_rgb(22, 163, 74),
    }
}

fn note(ui: &mut egui::Ui, text: &str) {
    let color = if ui.visuals().dark_mode {
        egui::Color32::from_rgb(113, 113, 122)
    } else {
        egui::Color32::from_rgb(80, 80, 90)
    };
    ui.label(egui::RichText::new(text).size(11.5).color(color));
}

fn size_text(size: Option<u64>) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match size {
        None => "measuring…".to_string(),
        Some(size) if size > GB => format!("{:.2} GB", size as f64 / GB as f64),
        Some(size) if size > MB => format!("{:.2} MB", size as f64 / MB as f64),
        Some(size) => format!("{} bytes", size),
    }
}
