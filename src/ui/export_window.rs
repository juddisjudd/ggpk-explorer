use eframe::egui;
use crate::export::{AudioFormat, DataFormat, ExportSettings, PsgFormat, TextureFormat};
use crate::ui::components::button::{action, Kind};
use crate::ui::components::{modal, modal_section};

pub struct ExportWindow {
    open: bool,
    pub settings: ExportSettings,
    pub confirmed: bool,
    target_name: String,
    is_folder: bool,
    pub hashes: Vec<u64>,
    pub immediate_hashes: Option<Vec<u64>>,
}

impl Default for ExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            settings: ExportSettings::default(),
            confirmed: false,
            target_name: String::new(),
            is_folder: false,
            hashes: Vec::new(),
            immediate_hashes: None,
        }
    }
}

impl ExportWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_for(&mut self, name: &str, is_folder: bool) {
        self.open = true;
        self.confirmed = false;
        self.target_name = name.to_string();
        self.is_folder = is_folder;
        self.settings.recursive = is_folder;
        self.immediate_hashes = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = self.open;
        if !open { return false; }

        let mut confirmed_now = false;
        let mut should_close = false;

        let is_dds = self.target_name.ends_with(".dds");
        let is_ogg = self.target_name.ends_with(".ogg");
        let is_dat = self.target_name.contains(".dat") || self.target_name.ends_with(".csd");
        let is_psg = self.target_name.ends_with(".psg");
        let is_parsed = crate::parsers::translate::handles(&self.target_name);
        let show_all = self.is_folder;

        let close = modal::show(ctx, "export", "Export", 400.0, |ui| {
            modal::body(ui, |ui| {
                let name_color = if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(228, 228, 231)
                } else {
                    egui::Color32::from_rgb(24, 24, 28)
                };
                ui.label(
                    egui::RichText::new(&self.target_name)
                        .size(13.0)
                        .monospace()
                        .color(name_color),
                );

                if show_all || is_dds {
                    modal_section(ui, "TEXTURE");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::OriginalDds, "DDS");
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::WebP, "WebP");
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::Png, "PNG");
                    });
                }

                if show_all || is_ogg {
                    modal_section(ui, "AUDIO");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.audio_format, AudioFormat::Original, "OGG / WAV");
                        ui.radio_value(&mut self.settings.audio_format, AudioFormat::Wav, "WAV");
                    });
                }

                if show_all || is_dat {
                    modal_section(ui, "DATA (.dat / .csd)");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.data_format, DataFormat::Original, "Original");
                        ui.radio_value(&mut self.settings.data_format, DataFormat::Json, "JSON");
                    });
                }

                if show_all || is_parsed {
                    modal_section(ui, "GAME FILES (.ao, .mat, .fmt, ...)");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.parsed_format, DataFormat::Original, "Original");
                        ui.radio_value(&mut self.settings.parsed_format, DataFormat::Json, "JSON");
                    });
                }

                if show_all || is_psg {
                    modal_section(ui, "PSG");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.psg_format, PsgFormat::Original, "Original");
                        ui.radio_value(&mut self.settings.psg_format, PsgFormat::Json, "JSON");
                        if !self.is_folder {
                            ui.radio_value(&mut self.settings.psg_format, PsgFormat::Tree, "Skill tree (official layout + viewer)");
                        }
                    });
                }

                if self.is_folder {
                    modal_section(ui, "OPTIONS");
                    ui.checkbox(&mut self.settings.recursive, "Include subfolders");

                    let export_count = if self.settings.recursive {
                        self.hashes.len()
                    } else {
                        self.immediate_hashes
                            .as_ref()
                            .map(|h| h.len())
                            .unwrap_or(self.hashes.len())
                    };
                    let count_color = if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(113, 113, 122)
                    } else {
                        egui::Color32::from_rgb(80, 80, 90)
                    };
                    ui.label(
                        egui::RichText::new(format!("FILES TO EXPORT · {}", export_count))
                            .monospace()
                            .size(10.5)
                            .color(count_color),
                    );
                }

            });
            modal::footer(ui, |ui| {
                if action(ui, "Export", Kind::Primary, true).clicked() {
                    self.confirmed = true;
                    confirmed_now = true;
                    should_close = true;
                }
                if action(ui, "Cancel", Kind::Secondary, true).clicked() {
                    should_close = true;
                }
            });
        });

        if close || should_close {
            open = false;
        }
        self.open = open;
        confirmed_now
    }
}
