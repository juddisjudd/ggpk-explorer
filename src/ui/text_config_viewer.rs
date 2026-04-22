use eframe::egui;

use crate::parsers::ParsedContent;
use crate::ui::json_viewer::JsonTreeViewer;

pub struct TextConfigViewer;

impl TextConfigViewer {
    pub fn show(ui: &mut egui::Ui, file_name: &str, parsed: &ParsedContent) {
        ui.label(format!("Text/Config Viewer: {}", file_name));
        ui.separator();

        match parsed {
            ParsedContent::Text { content, language } => {
                let lang = language.as_deref().unwrap_or("text");
                ui.label(format!("Language: {}", lang));
                ui.separator();

                let mut content_buf = content.clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut content_buf)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            }
            ParsedContent::Tree(value) => {
                JsonTreeViewer::show(ui, value);
            }
            ParsedContent::Table { rows, columns } => {
                ui.label(format!("Rows: {} | Columns: {}", rows.len(), columns.len()));
                ui.separator();

                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("text_config_table_header")
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for col in columns {
                                ui.strong(col);
                            }
                            ui.end_row();

                            for row in rows {
                                for col in columns {
                                    let value = row.get(col).map(|s| s.as_str()).unwrap_or("");
                                    ui.label(value);
                                }
                                ui.end_row();
                            }
                        });
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
            ParsedContent::Binary { data, .. } => {
                ui.label(format!("Unexpected binary payload ({} bytes)", data.len()));
            }
        }
    }
}
