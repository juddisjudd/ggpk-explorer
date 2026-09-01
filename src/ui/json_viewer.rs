use crate::ui::{links, plot};
use egui::{CollapsingHeader, Color32, FontId, RichText, Ui};
use serde_json::Value;

pub struct JsonTreeViewer;

struct Palette {
    key: Color32,
    string: Color32,
    number: Color32,
    boolean: Color32,
    null: Color32,
    punct: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            key: Color32::from_rgb(97, 175, 239),
            string: Color32::from_rgb(152, 195, 121),
            number: Color32::from_rgb(209, 154, 102),
            boolean: Color32::from_rgb(209, 154, 102),
            null: Color32::from_rgb(86, 182, 194),
            punct: Color32::from_rgb(171, 178, 191),
        }
    } else {
        Palette {
            key: Color32::from_rgb(9, 79, 172),
            string: Color32::from_rgb(3, 117, 43),
            number: Color32::from_rgb(180, 83, 9),
            boolean: Color32::from_rgb(180, 83, 9),
            null: Color32::from_rgb(13, 116, 124),
            punct: Color32::from_rgb(80, 80, 90),
        }
    }
}

/// `[r, g, b(, a)]` under a key that mentions colour, in 0..1 or 0..255.
pub fn as_color(key: Option<&str>, arr: &[Value]) -> Option<Color32> {
    let k = key?.to_ascii_lowercase();
    if !(k.contains("colour") || k.contains("color") || k.ends_with("_rgb") || k.ends_with("_rgba")) {
        return None;
    }
    if !(3..=4).contains(&arr.len()) {
        return None;
    }
    let nums: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();
    if nums.len() != arr.len() {
        return None;
    }
    let scale = if nums.iter().all(|n| *n <= 1.0) { 255.0 } else { 1.0 };
    let c = |i: usize| (nums.get(i).copied().unwrap_or(1.0) * scale).clamp(0.0, 255.0) as u8;
    Some(Color32::from_rgba_unmultiplied(c(0), c(1), c(2), if nums.len() == 4 { c(3) } else { 255 }))
}

/// `{ "points": [ { "time": t, "value": v }, … ] }` as a plottable series.
pub fn curve_points(value: &Value) -> Option<Vec<(f32, f32)>> {
    let pts = value.get("points")?.as_array()?;
    let series: Vec<(f32, f32)> = pts
        .iter()
        .filter_map(|p| {
            let t = p.get("time")?.as_f64()? as f32;
            let v = match p.get("value")? {
                Value::Number(n) => n.as_f64()? as f32,
                Value::Array(a) => a.first()?.as_f64()? as f32,
                _ => return None,
            };
            Some((t, v))
        })
        .collect();
    (series.len() >= 2).then_some(series)
}

fn swatch(ui: &mut Ui, color: Color32) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color()));
    let [r, g, b, a] = color.to_array();
    resp.on_hover_text(format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a));
}

impl JsonTreeViewer {
    pub fn show(ui: &mut Ui, value: &Value) {
        let mut sink = None;
        Self::show_linked(ui, value, &mut sink);
    }

    /// File paths are links (clicks land in `out`), colour arrays get a swatch and
    /// `points` curves get a small plot.
    pub fn show_linked(ui: &mut Ui, value: &Value, out: &mut Option<String>) {
        ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
        let pal = palette(ui.visuals().dark_mode);
        ui.push_id("json_tree_root", |ui| {
            Self::node(ui, &pal, value, None, true, out);
        });
    }

    fn key_prefix(ui: &mut Ui, pal: &Palette, font: &FontId, key: Option<&str>) {
        if let Some(k) = key {
            ui.label(RichText::new(format!("\"{}\": ", k)).color(pal.key).font(font.clone()));
        }
    }

    fn header_job(pal: &Palette, font: &FontId, key: Option<&str>, open: &str, count: usize) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob::default();
        let fmt = |c: Color32| egui::text::TextFormat { font_id: font.clone(), color: c, ..Default::default() };
        if let Some(k) = key {
            job.append(&format!("\"{}\": {}", k, open), 0.0, fmt(pal.key));
        } else {
            job.append(open, 0.0, fmt(pal.punct));
        }
        job.append(&format!(" {} ", count), 0.0, fmt(Color32::GRAY));
        job
    }

    fn node(ui: &mut Ui, pal: &Palette, value: &Value, key: Option<&str>, is_last: bool, out: &mut Option<String>) {
        let font = FontId::monospace(14.0);
        let comma = if is_last { "" } else { "," };
        match value {
            Value::Null => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::key_prefix(ui, pal, &font, key);
                    ui.label(RichText::new("null").color(pal.null).font(font.clone()));
                    ui.label(RichText::new(comma).color(pal.punct).font(font.clone()));
                });
            }
            Value::Bool(b) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::key_prefix(ui, pal, &font, key);
                    ui.label(RichText::new(b.to_string()).color(pal.boolean).font(font.clone()));
                    ui.label(RichText::new(comma).color(pal.punct).font(font.clone()));
                });
            }
            Value::Number(n) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::key_prefix(ui, pal, &font, key);
                    ui.label(RichText::new(n.to_string()).color(pal.number).font(font.clone()));
                    ui.label(RichText::new(comma).color(pal.punct).font(font.clone()));
                });
            }
            Value::String(s) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::key_prefix(ui, pal, &font, key);
                    ui.label(RichText::new("\"").color(pal.string).font(font.clone()));
                    if links::looks_like_path(s) {
                        links::maybe_link(ui, s, true, out);
                    } else {
                        ui.label(RichText::new(s).color(pal.string).font(font.clone()));
                    }
                    ui.label(RichText::new("\"").color(pal.string).font(font.clone()));
                    ui.label(RichText::new(comma).color(pal.punct).font(font.clone()));
                });
            }
            Value::Array(arr) => {
                let id = match key {
                    Some(k) => ui.make_persistent_id(k),
                    None => ui.make_persistent_id(format!("arr_{:p}", arr)),
                };
                let color = as_color(key, arr);
                ui.horizontal(|ui| {
                    if let Some(c) = color {
                        swatch(ui, c);
                    }
                    CollapsingHeader::new(Self::header_job(pal, &font, key, "[", arr.len()))
                        .id_salt(id)
                        .default_open(color.is_none())
                        .show(ui, |ui| {
                            for (i, v) in arr.iter().enumerate() {
                                Self::node(ui, pal, v, None, i == arr.len() - 1, out);
                            }
                            ui.label(RichText::new(format!("]{}", comma)).color(pal.punct).font(font.clone()));
                        });
                });
            }
            Value::Object(obj) => {
                let id = match key {
                    Some(k) => ui.make_persistent_id(k),
                    None => ui.make_persistent_id(format!("obj_{:p}", obj)),
                };
                let curve = curve_points(value);
                ui.horizontal(|ui| {
                    CollapsingHeader::new(Self::header_job(pal, &font, key, "{", obj.len()))
                        .id_salt(id)
                        .default_open(curve.is_none())
                        .show(ui, |ui| {
                            let count = obj.len();
                            for (i, (k, v)) in obj.iter().enumerate() {
                                Self::node(ui, pal, v, Some(k), i == count - 1, out);
                            }
                            ui.label(RichText::new(format!("}}{}", comma)).color(pal.punct).font(font.clone()));
                        });
                    if let Some(pts) = &curve {
                        let color = plot::series_color(0, ui.visuals().dark_mode);
                        plot::draw(ui, &[plot::Series { points: pts, color }], egui::vec2(150.0, 46.0));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn colour_arrays_become_swatches() {
        let v = json!([0.5, 1.0, 0.0]);
        let c = as_color(Some("colour"), v.as_array().unwrap()).unwrap();
        assert_eq!(c.to_array(), [127, 255, 0, 255]);
        let v255 = json!([10, 20, 30, 255]);
        assert!(as_color(Some("ssf_color"), v255.as_array().unwrap()).is_some());
        assert!(as_color(Some("size"), v.as_array().unwrap()).is_none());
        assert!(as_color(Some("colour"), json!([1, 2]).as_array().unwrap()).is_none());
    }

    #[test]
    fn points_objects_become_series() {
        let v = json!({ "points": [ { "time": 0.0, "value": 1.1 }, { "time": 1.0, "value": [0.5, 0.0] } ] });
        assert_eq!(curve_points(&v), Some(vec![(0.0, 1.1), (1.0, 0.5)]));
        assert!(curve_points(&json!({ "points": [] })).is_none());
    }
}
