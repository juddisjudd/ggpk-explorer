use eframe::egui;
use crate::settings::AppSettings;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

pub struct SettingsWindow {
    open: bool,
    fetch_rx: Option<Receiver<Result<String, String>>>,
    is_fetching: bool,
    fetch_status_msg: Option<String>,
    pub request_update_schema: bool,
    pub schema_status_msg: Option<String>,
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

        // Poll fetcher
        if self.is_fetching {
            if let Some(rx) = &self.fetch_rx {
                match rx.try_recv() {
                    Ok(Ok(version)) => {
                        settings.poe2_patch_version = version;
                        settings.save(); // Save immediately
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
                        self.fetch_status_msg = Some("Wait, thread died".to_string());
                        self.is_fetching = false;
                        self.fetch_rx = None;
                    }
                }
            }
        }

        let mut open = self.open;
        let mut should_close = false;
        
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("General");
                ui.horizontal(|ui| {
                    ui.label("GGPK Path:");
                    // Handle Option<String>
                    let mut path = settings.ggpk_path.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut path).changed() {
                        settings.ggpk_path = if path.is_empty() { None } else { Some(path) };
                    }
                    
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("GGPK", &["ggpk"]).pick_file() {
                            settings.ggpk_path = Some(p.to_string_lossy().to_string());
                        }
                    }
                });

                ui.separator();
                ui.heading("Network & CDN");
                
                ui.horizontal(|ui| {
                    ui.label("PoE 2 Patch Version:");
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
                            
                            match reqwest::blocking::get(&url) {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        match resp.json::<serde_json::Value>() {
                                            Ok(json) => {
                                                if let Some(v) = json.get("poe2").and_then(|s| s.as_str()) {
                                                    let _ = tx.send(Ok(v.to_string()));
                                                } else {
                                                    let _ = tx.send(Err("JSON missing 'poe2' field".to_string()));
                                                }
                                            },
                                            Err(e) => { let _ = tx.send(Err(format!("JSON Parse Error: {}", e))); }
                                        }
                                    } else {
                                        let _ = tx.send(Err(format!("HTTP Error: {}", resp.status())));
                                    }
                                },
                                Err(e) => { let _ = tx.send(Err(format!("Network Error: {}", e))); }
                            }
                        });
                    }

                    if let Some(msg) = &self.fetch_status_msg {
                        ui.label(msg);
                    }
                });
                
                ui.horizontal(|ui| {
                    ui.label("Version Source:");
                    ui.text_edit_singleline(&mut settings.patch_version_source_url);
                });

                ui.label("(Used for CDN bundles)");
                ui.small(format!("Current: {}", settings.poe2_patch_version));
                
                ui.separator();
                ui.heading("Schema");
                 ui.horizontal(|ui| {
                    ui.label("Local Schema Path:");
                    let mut path = settings.schema_local_path.clone().unwrap_or_default();
                     if ui.text_edit_singleline(&mut path).changed() {
                        settings.schema_local_path = if path.is_empty() { None } else { Some(path) };
                    }
                    if ui.button("Browse...").clicked() {
                         if let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                            settings.schema_local_path = Some(p.to_string_lossy().to_string());
                         }
                    }
                });
                
                if let Some(date) = schema_date {
                    ui.label(format!("Schema Last Updated: {}", date));
                }

                ui.horizontal(|ui| {
                    if ui.button("Update Schema Now").clicked() {
                        self.schema_status_msg = Some("Updating...".to_string());
                        self.request_update_schema = true; 
                    }
                    if let Some(msg) = &self.schema_status_msg {
                        ui.label(msg);
                    }
                });

                ui.separator();
                ui.heading("Cache");

                // Poll cache calc
                if let Some(rx) = &self.cache_calc_rx {
                    if let Ok(size) = rx.try_recv() {
                        self.cache_calc_rx = None;
                        // Format bytes
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

                ui.horizontal(|ui| {
                     ui.label(format!("Current Cache Size: {}", self.cache_size_str));
                     
                     if ui.button("Clear Cache").clicked() {
                         match AppSettings::clear_cache() {
                             Ok(_) => {
                                 self.cache_status_msg = Some("Cache Cleared!".to_string());
                                 self.cache_size_str = "0 Bytes".to_string();
                             },
                             Err(e) => {
                                 self.cache_status_msg = Some(format!("Error: {}", e));
                             }
                         }
                     }
                });
                if let Some(msg) = &self.cache_status_msg {
                    ui.label(msg);
                }

                ui.separator();

                if ui.button("Save & Close").clicked() {
                    settings.save();
                    should_close = true;
                }
            });
        
        if !open && self.open {
            // Window was closed by user (X button), ensure we save
            settings.save();
        }
        
        if should_close {
            open = false;
        }
        self.open = open;
    }
}
