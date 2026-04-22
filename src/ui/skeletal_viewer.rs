use eframe::egui;

use crate::parsers::ParsedContent;
use crate::ui::hex_viewer::HexViewer;

pub struct SkeletalViewer;

impl SkeletalViewer {
    pub fn show(ui: &mut egui::Ui, file_name: &str, parsed: &ParsedContent) {
        ui.label(format!("Skeletal/Animation Viewer: {}", file_name));
        ui.separator();

        match parsed {
            ParsedContent::Binary { data, metadata } => {
                ui.horizontal(|ui| {
                    ui.strong("Data Size:");
                    ui.label(format!("{} bytes", data.len()));
                });

                if !metadata.is_empty() {
                    ui.separator();
                    ui.label("Skeleton Metadata");
                    egui::Grid::new("skeletal_metadata_grid")
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for (key, value) in metadata {
                                ui.strong(key);
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                }

                ui.separator();
                ui.label("Binary Preview");
                let preview_len = data.len().min(12288);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    HexViewer::show(ui, &data[..preview_len]);
                });
            }
            ParsedContent::Metadata(meta) => {
                ui.label("Metadata-only skeletal content");
                ui.separator();
                for (key, value) in meta {
                    ui.horizontal(|ui| {
                        ui.strong(key);
                        ui.label(value);
                    });
                }
            }
            ParsedContent::Text { content, .. } => {
                let mut text = content.clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
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
