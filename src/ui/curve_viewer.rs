//! `.trl` / keyword `.pet` viewer: each emitter or trail block as a card with its
//! material link, plain properties, and every curve drawn as a small plot.

use crate::parsers::curves::CurveFile;
use crate::ui::{links, plot};
use eframe::egui::{self, Color32, RichText};

#[derive(Default)]
pub struct CurveViewerState {
    pub filter: String,
    pub hide_constant: bool,
}

pub struct CurveViewer;

impl CurveViewer {
    pub fn show(ui: &mut egui::Ui, id: u64, file: &CurveFile, state: &mut CurveViewerState) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        let dim = if dark { Color32::from_rgb(120, 120, 130) } else { Color32::from_rgb(140, 140, 150) };
        let total_curves: usize = file.blocks.iter().map(|b| b.curves.len()).sum();
        ui.horizontal(|ui| {
            if let Some(v) = file.version {
                crate::ui::components::badge(ui, &format!("v{}", v));
            }
            crate::ui::components::badge(ui, &format!("{} blocks", file.blocks.len()));
            crate::ui::components::badge(ui, &format!("{} curves", total_curves));
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter keys").desired_width(180.0));
            ui.toggle_value(&mut state.hide_constant, "Hide flat").on_hover_text("Skip curves whose value never changes");
        });
        ui.separator();
        let needle = state.filter.trim().to_ascii_lowercase();
        egui::ScrollArea::vertical().id_salt(("curve_view", id)).auto_shrink([false, false]).show(ui, |ui| {
            for (bi, block) in file.blocks.iter().enumerate() {
                let title = if block.title.is_empty() { format!("Block {}", bi + 1) } else { format!("{} · block {}", block.title, bi + 1) };
                egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(("curve_block", id, bi)).default_open(true).show(ui, |ui| {
                    let props: Vec<&(String, String)> = block.props.iter().filter(|(k, v)| needle.is_empty() || k.to_ascii_lowercase().contains(&needle) || v.to_ascii_lowercase().contains(&needle)).collect();
                    if !props.is_empty() {
                        egui::Grid::new(("curve_props", id, bi)).num_columns(2).spacing([16.0, 3.0]).striped(true).show(ui, |ui| {
                            for (k, v) in props {
                                ui.label(RichText::new(k).strong());
                                if v.is_empty() {
                                    ui.label(RichText::new("—").color(dim));
                                } else {
                                    links::maybe_link(ui, v, true, &mut opened);
                                }
                                ui.end_row();
                            }
                        });
                    }
                    let curves: Vec<_> = block
                        .curves
                        .iter()
                        .filter(|c| (needle.is_empty() || c.key.to_ascii_lowercase().contains(&needle)) && !(state.hide_constant && c.constant))
                        .collect();
                    if curves.is_empty() {
                        return;
                    }
                    ui.add_space(4.0);
                    let card_w = 190.0;
                    let per_row = ((ui.available_width() / (card_w + 10.0)).floor() as usize).max(1);
                    for chunk in curves.chunks(per_row) {
                        ui.horizontal(|ui| {
                            for (ci, c) in chunk.iter().enumerate() {
                                ui.vertical(|ui| {
                                    ui.set_width(card_w);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&c.key).size(11.5).strong());
                                        let kind = if c.keyframed { "keys" } else { "samples" };
                                        ui.label(RichText::new(format!("{} {}", c.points.len(), kind)).weak().size(10.0));
                                    });
                                    let color = plot::series_color(ci, dark);
                                    plot::draw(ui, &[plot::Series { points: &c.points, color }], egui::vec2(card_w, 64.0)).on_hover_text(
                                        c.points.iter().map(|(x, y)| format!("{:.3} → {:.3}", x, y)).collect::<Vec<_>>().join("\n"),
                                    );
                                });
                            }
                        });
                    }
                });
            }
        });
        opened
    }
}
