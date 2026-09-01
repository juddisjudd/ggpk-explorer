//! `.atl` timeline viewer: every animation's client events on a time strip and in a
//! table, with the effect/sound files they trigger as links.

use crate::ui::links;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke, Vec2};
use serde_json::Value;

struct Event<'a> {
    time: f32,
    end: Option<f32>,
    kind: &'a str,
    detail: String,
    deleted: bool,
}

fn events(anim: &Value) -> Vec<Event<'_>> {
    let mut out: Vec<Event> = anim
        .get("client_events")
        .and_then(|e| e.as_array())
        .map(|a| a.iter())
        .into_iter()
        .flatten()
        .map(|e| {
            let deleted = e.get("delete").and_then(|d| d.as_bool()).unwrap_or(false);
            let detail = ["filename", "name", "animation", "bone", "sound", "id"]
                .iter()
                .filter_map(|k| e.get(*k).map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }))
                .next()
                .unwrap_or_default();
            Event {
                time: e.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0) as f32,
                end: e.get("end_time").and_then(|t| t.as_f64()).map(|t| t as f32),
                kind: e.get("type").and_then(|t| t.as_str()).unwrap_or(if deleted { "delete" } else { "?" }),
                detail,
                deleted,
            }
        })
        .collect();
    out.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn kind_color(kind: &str) -> Color32 {
    let mut h: u32 = 2166136261;
    for b in kind.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    egui::ecolor::Hsva::new((h % 360) as f32 / 360.0, 0.55, 0.85, 1.0).into()
}

pub struct TimelineViewer;

impl TimelineViewer {
    pub fn show(ui: &mut egui::Ui, id: u64, doc: &Value) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        let dim = if dark { Color32::from_rgb(120, 120, 130) } else { Color32::from_rgb(140, 140, 150) };
        let anims: Vec<&Value> = doc.get("animations").and_then(|a| a.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            crate::ui::components::badge(ui, &format!("{} animations", anims.len()));
            if let Some(e) = doc.get("extends").and_then(|e| e.as_str()) {
                ui.label(RichText::new("extends").weak());
                links::maybe_link(ui, e, false, &mut opened);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("timeline_view", id)).auto_shrink([false, false]).show(ui, |ui| {
            for (ai, anim) in anims.iter().enumerate() {
                let name = anim.get("name").and_then(|n| n.as_str()).unwrap_or("(unnamed)");
                let evs = events(anim);
                let live = evs.iter().filter(|e| !e.deleted).count();
                let title = format!("{} · {} event{}{}", name, live, if live == 1 { "" } else { "s" }, if evs.len() > live { format!(" · {} removed", evs.len() - live) } else { String::new() });
                egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(("timeline_anim", id, ai)).default_open(anims.len() <= 8).show(ui, |ui| {
                    let span = evs.iter().map(|e| e.end.unwrap_or(e.time)).fold(0.0_f32, f32::max).max(0.001);
                    let width = ui.available_width().min(900.0);
                    let rows = evs.iter().filter(|e| !e.deleted).count();
                    if rows > 0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 14.0 * rows as f32 + 6.0), egui::Sense::hover());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 3.0, if dark { Color32::from_rgb(28, 28, 34) } else { Color32::from_rgb(246, 246, 250) });
                        let x = |t: f32| rect.left() + 4.0 + (t / span) * (rect.width() - 8.0);
                        for (row, e) in evs.iter().filter(|e| !e.deleted).enumerate() {
                            let y = rect.top() + 3.0 + row as f32 * 14.0;
                            let c = kind_color(e.kind);
                            match e.end {
                                Some(end) if end > e.time => {
                                    painter.rect_filled(egui::Rect::from_min_max(Pos2::new(x(e.time), y), Pos2::new(x(end).max(x(e.time) + 2.0), y + 11.0)), 2.0, c);
                                }
                                _ => {
                                    painter.line_segment([Pos2::new(x(e.time), y), Pos2::new(x(e.time), y + 11.0)], Stroke::new(2.0_f32, c));
                                }
                            }
                            painter.text(Pos2::new(x(e.time) + 3.0, y + 5.5), egui::Align2::LEFT_CENTER, e.kind.trim_end_matches("EventType"), egui::FontId::proportional(9.5), Color32::BLACK);
                        }
                        ui.label(RichText::new(format!("0 → {:.2}s", span)).weak().size(10.0));
                    }
                    egui::Grid::new(("timeline_grid", id, ai)).num_columns(4).spacing([12.0, 3.0]).striped(true).show(ui, |ui| {
                        ui.strong("time");
                        ui.strong("end");
                        ui.strong("type");
                        ui.strong("detail");
                        ui.end_row();
                        for e in &evs {
                            let t = |s: String| if e.deleted { RichText::new(s).color(dim).strikethrough() } else { RichText::new(s).monospace() };
                            ui.label(t(format!("{:.3}", e.time)));
                            ui.label(t(e.end.map(|v| format!("{:.3}", v)).unwrap_or_else(|| "—".into())));
                            ui.label(t(e.kind.trim_end_matches("EventType").to_string()));
                            if e.deleted {
                                ui.label(RichText::new(format!("removed from parent ({})", e.detail)).color(dim).italics());
                            } else {
                                links::maybe_link(ui, &e.detail, true, &mut opened);
                            }
                            ui.end_row();
                        }
                    });
                });
            }
        });
        opened
    }
}
