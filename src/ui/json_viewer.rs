use egui::{CollapsingHeader, Color32, FontId, RichText, Ui};
use serde_json::Value;

pub struct JsonTreeViewer;

impl JsonTreeViewer {
    pub fn show(ui: &mut Ui, value: &Value) {
        ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
        ui.push_id("json_tree_root", |ui| {
             Self::show_recursive(ui, value, None, true);
        });
    }

    fn show_recursive(ui: &mut Ui, value: &Value, key: Option<&str>, is_last: bool) {
        let font_id = FontId::monospace(14.0);
        
        let (color_key, color_string, color_number, color_bool, color_null, color_punct) = if ui.visuals().dark_mode {
            (
                Color32::from_rgb(97, 175, 239), // Blue
                Color32::from_rgb(152, 195, 121), // Green
                Color32::from_rgb(209, 154, 102), // Orange
                Color32::from_rgb(209, 154, 102), // Orange
                Color32::from_rgb(86, 182, 194), // Cyan
                Color32::from_rgb(171, 178, 191), // Light Gray
            )
        } else {
            (
                Color32::from_rgb(9, 79, 172), // Dark Blue
                Color32::from_rgb(3, 117, 43), // Dark Green
                Color32::from_rgb(180, 83, 9), // Dark Orange/Brown
                Color32::from_rgb(180, 83, 9), // Dark Orange/Brown
                Color32::from_rgb(13, 116, 124), // Teal
                Color32::from_rgb(80, 80, 90), // Darker Gray
            )
        };

        let comma = if is_last { "" } else { "," };

        // Helper to render key prefix: "key": 
        // We inline this logic instead of a closure to avoid complex borrowing of font_id
        let render_key_inline = |ui: &mut Ui, font_id: &FontId| {
            if let Some(k) = key {
                ui.label(RichText::new("\"").color(color_key).font(font_id.clone()));
                ui.label(RichText::new(k).color(color_key).font(font_id.clone()));
                ui.label(RichText::new("\": ").color(color_key).font(font_id.clone()));
            }
        };

        match value {
            Value::Null => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    render_key_inline(ui, &font_id);
                    ui.label(RichText::new("null").color(color_null).font(font_id.clone()));
                    ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                });
            }
            Value::Bool(b) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    render_key_inline(ui, &font_id);
                    ui.label(RichText::new(b.to_string()).color(color_bool).font(font_id.clone()));
                    ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                });
            }
            Value::Number(n) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    render_key_inline(ui, &font_id);
                    ui.label(RichText::new(n.to_string()).color(color_number).font(font_id.clone()));
                    ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                });
            }
            Value::String(s) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    render_key_inline(ui, &font_id);
                    ui.label(RichText::new("\"").color(color_string).font(font_id.clone()));
                    ui.label(RichText::new(s).color(color_string).font(font_id.clone()));
                    ui.label(RichText::new("\"").color(color_string).font(font_id.clone()));
                    ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                });
            }
            Value::Array(arr) => {
                let id = if let Some(k) = key {
                    ui.make_persistent_id(k)
                } else {
                    ui.make_persistent_id(format!("arr_{:p}", arr))
                };

                // Custom header rendering
                let mut job = egui::text::LayoutJob::default();
                if let Some(k) = key {
                    job.append("\"", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_key, ..Default::default() });
                    job.append(k, 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_key, ..Default::default() });
                    job.append("\": [", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_punct, ..Default::default() });
                } else {
                    job.append("[", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_punct, ..Default::default() });
                }
                job.append(&format!(" {} ", arr.len()), 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: Color32::GRAY, ..Default::default() });

                CollapsingHeader::new(job)
                .id_salt(id)
                .default_open(true) 
                .show(ui, |ui| {
                    for (i, v) in arr.iter().enumerate() {
                        Self::show_recursive(ui, v, None, i == arr.len() - 1);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(RichText::new("]").color(color_punct).font(font_id.clone()));
                        ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                    });
                });
            }
            Value::Object(obj) => {
                 let id = if let Some(k) = key {
                    ui.make_persistent_id(k)
                } else {
                     ui.make_persistent_id(format!("obj_{:p}", obj))
                };
                
                let mut job = egui::text::LayoutJob::default();
                if let Some(k) = key {
                    job.append("\"", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_key, ..Default::default() });
                    job.append(k, 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_key, ..Default::default() });
                    job.append("\": {", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_punct, ..Default::default() });
                } else {
                    job.append("{", 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: color_punct, ..Default::default() });
                }
                 job.append(&format!(" {} ", obj.len()), 0.0, egui::text::TextFormat { font_id: font_id.clone(), color: Color32::GRAY, ..Default::default() });
                
                CollapsingHeader::new(job)
                .id_salt(id)
                .default_open(true)
                .show(ui, |ui| {
                    let count = obj.len();
                    for (i, (k, v)) in obj.iter().enumerate() {
                        Self::show_recursive(ui, v, Some(k), i == count - 1);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(RichText::new("}").color(color_punct).font(font_id.clone()));
                        ui.label(RichText::new(comma).color(color_punct).font(font_id.clone()));
                    });
                });
            }
        }
    }
}
