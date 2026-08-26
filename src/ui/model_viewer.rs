use crate::parsers::model::ModelFile;
use crate::ui::json_viewer::JsonTreeViewer;
use eframe::egui::{self, RichText};

pub struct ModelViewer;

impl ModelViewer {
    pub fn show(ui: &mut egui::Ui, file_name: &str, model: &ModelFile, summary: &serde_json::Value) {
        ui.horizontal_wrapped(|ui| {
            crate::ui::components::badge(ui, &model.kind().to_uppercase());
            crate::ui::components::badge(ui, &format!("v{}", model.version()));
            if let Some(stats) = summary.get("stats").and_then(|s| s.as_object()) {
                for (k, v) in stats {
                    ui.label(RichText::new(format!("{} {}", v, k)).weak());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export JSON").on_hover_text("Full structure including vertex/index buffers").clicked() {
                    let stem = std::path::Path::new(file_name).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.json", stem)).save_file() {
                        if let Ok(f) = std::fs::File::create(path) {
                            let _ = serde_json::to_writer(std::io::BufWriter::new(f), model);
                        }
                    }
                }
                if ui.button("Copy summary").clicked() {
                    ui.ctx().copy_text(serde_json::to_string_pretty(summary).unwrap_or_default());
                }
            });
        });
        ui.separator();
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            JsonTreeViewer::show(ui, summary);
        });
    }
}
