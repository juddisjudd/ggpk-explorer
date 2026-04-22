use eframe::egui;

use crate::parsers::ParsedContent;
use crate::ui::hex_viewer::HexViewer;

pub struct GraphicsViewer;

impl GraphicsViewer {
    pub fn show(ui: &mut egui::Ui, file_name: &str, parsed: &ParsedContent) {
        ui.label(format!("Graphics Viewer: {}", file_name));
        ui.separator();

        match parsed {
            ParsedContent::Binary { data, metadata } => {
                ui.label(format!("Binary Size: {} bytes", data.len()));

                if !metadata.is_empty() {
                    ui.separator();
                    ui.collapsing("Metadata", |ui| {
                        for (key, value) in metadata {
                            ui.horizontal(|ui| {
                                ui.strong(key);
                                ui.label(value);
                            });
                        }
                    });
                }

                ui.separator();
                ui.label("Hex Preview");
                let preview_len = data.len().min(8192);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    HexViewer::show(ui, &data[..preview_len]);
                });
            }
            ParsedContent::Metadata(meta) => {
                for (key, value) in meta {
                    ui.horizontal(|ui| {
                        ui.strong(key);
                        ui.label(value);
                    });
                }
            }
            ParsedContent::Text { content, .. } => {
                ui.label("Graphics parser returned text payload");
                ui.separator();
                let mut buf = content.clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut buf)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            }
            ParsedContent::Tree(value) => {
                ui.label(format!("Structured payload: {}", value));
            }
            ParsedContent::Table { rows, columns } => {
                ui.label(format!("Table payload: {} rows, {} columns", rows.len(), columns.len()));
            }
        }
    }
}
