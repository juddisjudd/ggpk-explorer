use eframe::egui;
use crate::settings::AppSettings;
use crate::ui::components::modal_section;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

#[derive(PartialEq)]
pub enum SchemaUpdateStatus {
    Checking,
    UpToDate,
    UpdateAvailable,
    Error(String),
}

pub struct SettingsWindow {
    open: bool,
    fetch_rx: Option<Receiver<Result<String, String>>>,
    is_fetching: bool,
    fetch_status_msg: Option<String>,
    pub request_update_schema: bool,
    pub schema_status_msg: Option<String>,
    pub schema_update_status: SchemaUpdateStatus,
    pub cache_size_str: String,
    pub cache_status_msg: Option<String>,
    pub cache_calc_rx: Option<Receiver<u64>>,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            open: false,
            fetch_rx: None,
            is_fetching: false,
            fetch_status_msg: None,
            request_update_schema: false,
            schema_status_msg: None,
            schema_update_status: SchemaUpdateStatus::Checking,
            cache_size_str: "Unknown".to_string(),
            cache_status_msg: None,
            cache_calc_rx: None,
        }
    }
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.open = true;
        self.cache_status_msg = None;
        self.cache_size_str = "Calculating...".to_string();

        let (tx, rx) = channel();
        self.cache_calc_rx = Some(rx);
        thread::spawn(move || {
            let size = AppSettings::get_cache_size();
            let _ = tx.send(size);
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, settings: &mut AppSettings, schema_date: Option<&str>) {
        if !self.open { return; }

        if self.is_fetching {
            if let Some(rx) = &self.fetch_rx {
                match rx.try_recv() {
                    Ok(Ok(version)) => {
                        settings.poe2_patch_version = version;
                        settings.save();
                        self.fetch_status_msg = Some("Updated!".to_string());
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    },
                    Ok(Err(e)) => {
                        self.fetch_status_msg = Some(format!("Error: {}", e));
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => {},
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.fetch_status_msg = Some("Thread died".to_string());
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    }
                }
            }
        }

        if let Some(rx) = &self.cache_calc_rx {
            if let Ok(size) = rx.try_recv() {
                self.cache_calc_rx = None;
                const KB: u64 = 1024;
                const MB: u64 = KB * 1024;
                const GB: u64 = MB * 1024;
                self.cache_size_str = if size > GB {
                    format!("{:.2} GB", size as f64 / GB as f64)
                } else if size > MB {
                    format!("{:.2} MB", size as f64 / MB as f64)
                } else {
                    format!("{} Bytes", size)
                };
            }
        }

        let mut open = self.open;
        let mut should_close = false;

        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;

                // ── General ──────────────────────────────────────────
                modal_section(ui, "GENERAL");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("GGPK Path").size(12.5));
                    let mut path = settings.ggpk_path.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut path).changed() {
                        settings.ggpk_path = if path.is_empty() { None } else { Some(path) };
                    }
                    if ui.button("Browse").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("GGPK", &["ggpk"]).pick_file() {
                            settings.ggpk_path = Some(p.to_string_lossy().to_string());
                        }
                    }
                });

                ui.separator();

                // ── Network & CDN ─────────────────────────────────────
                modal_section(ui, "NETWORK & CDN");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Patch Version").size(12.5));
                    ui.text_edit_singleline(&mut settings.poe2_patch_version);
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
                        ui.label(
                            egui::RichText::new(msg)
                                .size(11.5)
                                .color(egui::Color32::from_rgb(161, 161, 170)),
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Version Source").size(12.5));
                    ui.text_edit_singleline(&mut settings.patch_version_source_url);
                });
                ui.checkbox(&mut settings.auto_detect_patch_version, "Auto-detect latest patch version on startup");

                ui.separator();

                // ── Schema ────────────────────────────────────────────
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
                    ui.label(
                        egui::RichText::new(format!("Last updated: {}", date))
                            .size(11.5)
                            .color(egui::Color32::from_rgb(113, 113, 122)),
                    );
                }
                ui.checkbox(&mut settings.auto_update_schema, "Auto-update schema when a newer release is available");
                ui.horizontal(|ui| {
                    if ui.button("Update Schema Now").clicked() {
                        self.schema_status_msg = Some("Updating...".to_string());
                        self.request_update_schema = true;
                    }
                    if let Some(msg) = &self.schema_status_msg {
                        ui.label(
                            egui::RichText::new(msg)
                                .size(11.5)
                                .color(egui::Color32::from_rgb(161, 161, 170)),
                        );
                    }
                    match &self.schema_update_status {
                        SchemaUpdateStatus::Checking => { ui.spinner(); },
                        SchemaUpdateStatus::UpToDate => {
                            ui.label(egui::RichText::new("Up to date").size(11.5).color(egui::Color32::from_rgb(74, 222, 128)));
                        },
                        SchemaUpdateStatus::UpdateAvailable => {
                            ui.label(egui::RichText::new("Update available").size(11.5).color(egui::Color32::from_rgb(250, 204, 21)));
                        },
                        SchemaUpdateStatus::Error(e) => {
                            ui.label(egui::RichText::new(format!("Check failed: {}", e)).size(11.5).color(egui::Color32::from_rgb(239, 68, 68)));
                        },
                    }
                });

                ui.separator();

                // ── Cache ─────────────────────────────────────────────
                modal_section(ui, "CACHE");
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Size: {}", self.cache_size_str))
                            .size(12.5),
                    );
                    if ui.button("Clear Cache").clicked() {
                        match AppSettings::clear_cache() {
                            Ok(_) => {
                                self.cache_status_msg = Some("Cache cleared".to_string());
                                self.cache_size_str = "0 Bytes".to_string();
                            },
                            Err(e) => {
                                self.cache_status_msg = Some(format!("Error: {}", e));
                            }
                        }
                    }
                    if let Some(msg) = &self.cache_status_msg {
                        ui.label(
                            egui::RichText::new(msg)
                                .size(11.5)
                                .color(egui::Color32::from_rgb(161, 161, 170)),
                        );
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(2.0);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save & Close").clicked() {
                        settings.save();
                        should_close = true;
                    }
                });
            });

        if !open && self.open {
            settings.save();
        }
        if should_close {
            open = false;
        }
        self.open = open;
    }
}
