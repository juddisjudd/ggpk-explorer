use eframe::egui;
use crate::ui::components::modal_section;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextureFormat {
    OriginalDds,
    WebP,
    Png,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioFormat {
    Original,
    Wav,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataFormat {
    Original,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PsgFormat {
    Original,
    Json,
}

#[derive(Clone)]
pub struct ExportSettings {
    pub texture_format: TextureFormat,
    pub audio_format: AudioFormat,
    pub data_format: DataFormat,
    pub psg_format: PsgFormat,
    pub recursive: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            texture_format: TextureFormat::OriginalDds,
            audio_format: AudioFormat::Original,
            data_format: DataFormat::Original,
            psg_format: PsgFormat::Original,
            recursive: true,
        }
    }
}

pub struct ExportWindow {
    open: bool,
    pub settings: ExportSettings,
    pub confirmed: bool,
    target_name: String,
    is_folder: bool,
    pub hashes: Vec<u64>,
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
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = self.open;
        if !open { return false; }

        let mut confirmed_now = false;
        let mut should_close = false;

        let is_dds = self.target_name.ends_with(".dds");
        let is_ogg = self.target_name.ends_with(".ogg");
        let is_dat = self.target_name.contains(".dat");
        let is_psg = self.target_name.ends_with(".psg");
        let show_all = self.is_folder;

        egui::Window::new("Export")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;

                ui.label(
                    egui::RichText::new(&self.target_name)
                        .size(13.0)
                        .monospace()
                        .color(egui::Color32::from_rgb(228, 228, 231)),
                );

                if show_all || is_dds {
                    ui.separator();
                    modal_section(ui, "TEXTURE");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::OriginalDds, "DDS");
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::WebP, "WebP");
                        ui.radio_value(&mut self.settings.texture_format, TextureFormat::Png, "PNG");
                    });
                }

                if show_all || is_ogg {
                    ui.separator();
                    modal_section(ui, "AUDIO");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.audio_format, AudioFormat::Original, "OGG / WAV");
                        ui.radio_value(&mut self.settings.audio_format, AudioFormat::Wav, "WAV");
                    });
                }

                if show_all || is_dat {
                    ui.separator();
                    modal_section(ui, "DATA");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.data_format, DataFormat::Original, "Original");
                        ui.radio_value(&mut self.settings.data_format, DataFormat::Json, "JSON");
                    });
                }

                if show_all || is_psg {
                    ui.separator();
                    modal_section(ui, "PSG");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.settings.psg_format, PsgFormat::Original, "Original");
                        ui.radio_value(&mut self.settings.psg_format, PsgFormat::Json, "JSON");
                    });
                }

                if self.is_folder {
                    ui.separator();
                    modal_section(ui, "OPTIONS");
                    ui.checkbox(&mut self.settings.recursive, "Include subfolders");
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Export").clicked() {
                            self.confirmed = true;
                            confirmed_now = true;
                            should_close = true;
                        }
                    });
                });
            });

        if should_close {
            open = false;
        }
        self.open = open;
        confirmed_now
    }
}
