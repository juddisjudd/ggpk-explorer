//! `.mat` material viewer: the textures it samples as thumbnails, its fxgraph
//! instances with their parameters, and the full document underneath.

use crate::ui::json_viewer::{self, JsonTreeViewer};
use crate::ui::{links, plot};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::collections::HashMap;

pub const THUMB_PX: f32 = 128.0;

/// Every `.dds` path mentioned anywhere in the document, deduplicated case-insensitively.
pub fn texture_paths(doc: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                if s.to_ascii_lowercase().ends_with(".dds") && links::looks_like_path(s) {
                    let p = links::normalize(s);
                    if !out.iter().any(|o| o.eq_ignore_ascii_case(&p)) {
                        out.push(p);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(doc, &mut out);
    out
}

fn value_summary(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().map(value_summary).collect::<Vec<_>>().join(", "),
        Value::Object(o) if o.is_empty() => "—".into(),
        Value::Object(o) => o.keys().cloned().collect::<Vec<_>>().join(" "),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub struct MaterialViewer;

impl MaterialViewer {
    /// `thumbs` holds decoded thumbnails by path (`None` = failed); paths still needed are
    /// pushed to `wanted`. Returns a path when a link or thumbnail is clicked.
    pub fn show(ui: &mut egui::Ui, id: u64, doc: &Value, thumbs: &HashMap<String, Option<egui::TextureHandle>>, wanted: &mut Vec<String>) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        let dim = if dark { Color32::from_rgb(120, 120, 130) } else { Color32::from_rgb(140, 140, 150) };
        let textures = texture_paths(doc);
        let instances: Vec<&Value> = doc.get("graphinstances").and_then(|g| g.as_array()).map(|a| a.iter().collect()).unwrap_or_default();

        ui.horizontal_wrapped(|ui| {
            if let Some(v) = doc.get("version").and_then(|v| v.as_u64()) {
                crate::ui::components::badge(ui, &format!("v{}", v));
            }
            crate::ui::components::badge(ui, &format!("{} textures", textures.len()));
            crate::ui::components::badge(ui, &format!("{} graph instances", instances.len()));
            for key in ["blend", "blend_mode", "shader", "type"] {
                if let Some(s) = doc.get(key).and_then(|v| v.as_str()) {
                    crate::ui::components::badge(ui, &format!("{} {}", key, s));
                }
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().id_salt(("material_view", id)).auto_shrink([false, false]).show(ui, |ui| {
            if !textures.is_empty() {
                ui.label(RichText::new("Textures").strong());
                let per_row = ((ui.available_width() / (THUMB_PX + 16.0)).floor() as usize).max(1);
                for chunk in textures.chunks(per_row) {
                    ui.horizontal(|ui| {
                        for path in chunk {
                            ui.vertical(|ui| {
                                ui.set_width(THUMB_PX + 8.0);
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(THUMB_PX, THUMB_PX), egui::Sense::click());
                                match thumbs.get(path) {
                                    Some(Some(tex)) => {
                                        let size = tex.size_vec2();
                                        let scale = (THUMB_PX / size.x).min(THUMB_PX / size.y);
                                        let draw = egui::Rect::from_center_size(rect.center(), size * scale);
                                        ui.painter().rect_filled(rect, 3.0, if dark { Color32::from_rgb(20, 20, 24) } else { Color32::from_rgb(230, 230, 236) });
                                        ui.painter().image(tex.id(), draw, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                                    }
                                    Some(None) => {
                                        ui.painter().rect_filled(rect, 3.0, if dark { Color32::from_rgb(40, 24, 24) } else { Color32::from_rgb(250, 230, 230) });
                                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "no preview", egui::FontId::proportional(11.0), dim);
                                    }
                                    None => {
                                        wanted.push(path.clone());
                                        ui.painter().rect_filled(rect, 3.0, if dark { Color32::from_rgb(30, 30, 36) } else { Color32::from_rgb(236, 236, 240) });
                                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "loading…", egui::FontId::proportional(11.0), dim);
                                    }
                                }
                                if resp.clicked() {
                                    opened = Some(path.clone());
                                }
                                let name = path.rsplit('/').next().unwrap_or(path);
                                let r = ui.add(egui::Label::new(RichText::new(name).size(11.0)).truncate());
                                r.on_hover_text(path);
                                links::maybe_link(ui, path, false, &mut opened).on_hover_text("Open texture");
                            });
                        }
                    });
                }
                ui.add_space(6.0);
            }

            if !instances.is_empty() {
                ui.label(RichText::new("Graph instances").strong());
                for (i, inst) in instances.iter().enumerate() {
                    let parent = inst.get("parent").and_then(|p| p.as_str()).unwrap_or("(no parent)");
                    let title = parent.rsplit('/').next().unwrap_or(parent).to_string();
                    egui::CollapsingHeader::new(RichText::new(format!("{} · {}", i + 1, title)).strong()).id_salt(("mat_inst", id, i)).default_open(true).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("parent").weak());
                            links::maybe_link(ui, parent, true, &mut opened);
                        });
                        if let Some(params) = inst.get("custom_parameters").and_then(|p| p.as_array()) {
                            egui::Grid::new(("mat_params", id, i)).num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                                for (pi, p) in params.iter().enumerate() {
                                    ui.label(RichText::new(p.get("name").and_then(|n| n.as_str()).unwrap_or("?")).strong());
                                    ui.horizontal_wrapped(|ui| {
                                        for (vi, val) in p.get("parameters").and_then(|v| v.as_array()).map(|a| a.iter().collect::<Vec<_>>()).unwrap_or_default().into_iter().enumerate() {
                                            let mut drawn = false;
                                            if let Some(obj) = val.as_object() {
                                                for (k, v) in obj {
                                                    if let Some(pts) = json_viewer::curve_points(v) {
                                                        ui.vertical(|ui| {
                                                            ui.label(RichText::new(k).weak().size(10.0));
                                                            plot::draw(ui, &[plot::Series { points: &pts, color: plot::series_color(vi, dark) }], egui::vec2(140.0, 44.0));
                                                        });
                                                        drawn = true;
                                                    } else if let Some(s) = v.as_str() {
                                                        links::maybe_link(ui, s, true, &mut opened);
                                                        drawn = true;
                                                    } else if let Some(c) = v.as_array().and_then(|a| json_viewer::as_color(Some(k), a)) {
                                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                                        ui.painter().rect_filled(rect, 2.0, c);
                                                        drawn = true;
                                                    }
                                                }
                                            }
                                            if !drawn {
                                                ui.label(RichText::new(value_summary(val)).monospace());
                                            }
                                            let _ = pi;
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                        }
                    });
                }
                ui.add_space(6.0);
            }

            egui::CollapsingHeader::new(RichText::new("Full document").strong()).id_salt(("mat_json", id)).default_open(instances.is_empty() && textures.is_empty()).show(ui, |ui| {
                JsonTreeViewer::show_linked(ui, doc, &mut opened);
            });
        });
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collects_unique_texture_paths() {
        let doc = json!({ "textures": [ { "filename": "Art/A.dds" }, { "filename": "art/a.dds" } ], "graphinstances": [ { "custom_parameters": [ { "parameters": [ { "value": "Art/B.dds" } ] } ] } ] });
        let mut paths = texture_paths(&doc);
        paths.sort();
        assert_eq!(paths, vec!["Art/A.dds", "Art/B.dds"]);
    }
}
