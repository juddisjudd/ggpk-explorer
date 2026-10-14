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

/// How a surface map packs its channels, which the name of the graph parameter that samples it tells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceMap {
    /// `NormalGlossAO_TEX`: the normal in red and green, occlusion in blue, gloss in alpha.
    NormalGlossAo,
    /// `base_normalspec_texture`: the normal in alpha and green, specular in red.
    NormalSpec,
}

/// Each graph parameter's name with the texture it samples and whether it samples it as sRGB.
fn sampled(doc: &Value) -> impl Iterator<Item = (&str, String, bool)> + '_ {
    let parameters = doc.get("graphinstances").and_then(|g| g.as_array()).into_iter().flatten().filter_map(|inst| inst.get("custom_parameters")?.as_array()).flatten();
    parameters.filter_map(|p| {
        let texture = p.get("parameters")?.as_array()?.iter().find(|value| value.get("path").is_some())?;
        Some((p.get("name")?.as_str()?, links::normalize(texture.get("path")?.as_str()?), texture.get("srgb").and_then(|s| s.as_bool()) == Some(true)))
    })
}

/// The texture a material is coloured by: the one its colour parameter samples, else the first sampled as sRGB, else its first texture.
pub fn colour_texture(doc: &Value) -> Option<String> {
    let named = sampled(doc).find(|(name, _, _)| ["base_color_texture", "AlbedoTransparency_TEX"].contains(name));
    let colour = named.or_else(|| sampled(doc).find(|(_, _, srgb)| *srgb)).map(|(_, path, _)| path);
    colour.or_else(|| texture_paths(doc).into_iter().next())
}

/// The map a material's surface detail is packed in, for the two packings the preview reads.
pub fn surface_texture(doc: &Value) -> Option<(String, SurfaceMap)> {
    sampled(doc).find_map(|(name, path, _)| match name {
        "NormalGlossAO_TEX" => Some((path, SurfaceMap::NormalGlossAo)),
        "base_normalspec_texture" => Some((path, SurfaceMap::NormalSpec)),
        _ => None,
    })
}

/// The file name of each graph the material is built from, in lower case.
fn graph_names(doc: &Value) -> impl Iterator<Item = String> + '_ {
    let parents = doc.get("graphinstances").and_then(|g| g.as_array()).into_iter().flatten().filter_map(|inst| inst.get("parent")?.as_str());
    parents.map(|parent| parent.rsplit('/').next().unwrap_or(parent).to_ascii_lowercase())
}

/// Whether the material only casts a shadow, so the shapes it covers are never seen.
pub fn is_shadow_only(doc: &Value) -> bool {
    graph_names(doc).any(|graph| graph.contains("shadowonly"))
}

/// Whether the material draws without alpha, so its texture's alpha channel holds something other than coverage.
pub fn is_opaque(doc: &Value) -> bool {
    let blend = doc.pointer("/defaultgraph/overriden_blend_mode").and_then(|m| m.as_str());
    blend.is_none_or(|mode| mode.eq_ignore_ascii_case("Opaque")) && !graph_names(doc).any(|graph| graph.contains("alpha"))
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

    #[test]
    fn the_colour_texture_is_the_one_sampled_as_srgb() {
        let normal_first = json!({
            "textures": [{ "filename": "Art/Wolfns.dds" }, { "filename": "Art/Wolf_colour.dds" }],
            "defaultgraph": { "overriden_blend_mode": "Opaque" },
            "graphinstances": [{ "custom_parameters": [
                { "name": "base_normalspec_texture", "parameters": [{ "path": "Art/Wolfns.dds", "srgb": false }, {}] },
                { "name": "base_color_texture", "parameters": [{ "path": "Art/Wolf_colour.dds", "srgb": true }, {}] }
            ] }]
        });
        assert_eq!(colour_texture(&normal_first).as_deref(), Some("Art/Wolf_colour.dds"));
        assert_eq!(surface_texture(&normal_first), Some(("Art/Wolfns.dds".to_string(), SurfaceMap::NormalSpec)));
        assert!(is_opaque(&normal_first));

        let glow_first = json!({ "graphinstances": [{ "custom_parameters": [
            { "name": "SSS_TEX", "parameters": [{ "path": "Art/Twig_sss.dds", "srgb": true }, {}] },
            { "name": "AlbedoTransparency_TEX", "parameters": [{ "path": "Art/Twig_colour.dds", "srgb": true }, {}] },
            { "name": "NormalGlossAO_TEX", "parameters": [{ "path": "Art/Twig_normal.dds", "srgb": false }, {}] }
        ] }] });
        assert_eq!(colour_texture(&glow_first).as_deref(), Some("Art/Twig_colour.dds"));
        assert_eq!(surface_texture(&glow_first), Some(("Art/Twig_normal.dds".to_string(), SurfaceMap::NormalGlossAo)));

        let no_graph = json!({ "textures": [{ "filename": "Art/Leaf.dds" }], "defaultgraph": { "overriden_blend_mode": "AlphaTest" } });
        assert_eq!(colour_texture(&no_graph).as_deref(), Some("Art/Leaf.dds"));
        assert!(!is_opaque(&no_graph));
        assert_eq!(colour_texture(&json!({})), None);
    }

    #[test]
    fn graphs_say_when_a_material_is_cut_out_or_only_casts_a_shadow() {
        let shadow = json!({ "graphinstances": [{ "parent": "Metadata/Art/Textures/Misc/black_shadowonlyc.fxgraph" }, { "parent": "Metadata/Effects/Graphs/General/ForceOpaqueShadowOnly.fxgraph" }] });
        assert!(is_shadow_only(&shadow) && is_opaque(&shadow));
        let cut_out = json!({ "graphinstances": [{ "parent": "Metadata/Materials/SpecGlossSpecMask.fxgraph" }, { "parent": "Metadata/Effects/Graphs/General/ForceAlphaTestWithShadow.fxgraph" }] });
        assert!(!is_shadow_only(&cut_out) && !is_opaque(&cut_out));
        let plain = json!({ "graphinstances": [{ "parent": "Metadata/Effects/Graphs/General/ForceOpaqueNoShadow.fxgraph" }] });
        assert!(!is_shadow_only(&plain) && is_opaque(&plain));
    }
}
