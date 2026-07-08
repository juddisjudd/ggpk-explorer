use eframe::egui;
use std::collections::HashMap;

use crate::parsers::fxgraph::FxGraph;

pub struct FxGraphViewerState {
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub show_graph: bool,
    initialized: bool,
}

impl Default for FxGraphViewerState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            show_graph: true,
            initialized: false,
        }
    }
}

/// Deterministic, distinct-ish color per node type so similar operations
/// (e.g. all `SampleTexture` nodes) are visually grouped without needing a
/// hand-maintained palette for the game's ever-growing node type list.
fn type_color(node_type: &str) -> egui::Color32 {
    let mut hash: u32 = 2166136261;
    for b in node_type.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let hue = (hash % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.55, 0.85, 1.0).into()
}

/// Samples a cubic bezier "wire" between two node ports, control points
/// pulled horizontally like a typical node-editor connection.
fn wire_points(src: egui::Pos2, dst: egui::Pos2) -> Vec<egui::Pos2> {
    let pull = ((dst.x - src.x).abs() * 0.5).max(40.0);
    let c1 = egui::pos2(src.x + pull, src.y);
    let c2 = egui::pos2(dst.x - pull, dst.y);
    let steps = 24;
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let mt = 1.0 - t;
            let x = mt * mt * mt * src.x
                + 3.0 * mt * mt * t * c1.x
                + 3.0 * mt * t * t * c2.x
                + t * t * t * dst.x;
            let y = mt * mt * mt * src.y
                + 3.0 * mt * mt * t * c1.y
                + 3.0 * mt * t * t * c2.y
                + t * t * t * dst.y;
            egui::pos2(x, y)
        })
        .collect()
}

pub struct FxGraphViewer<'a> {
    state: &'a mut FxGraphViewerState,
    graph: &'a FxGraph,
}

impl<'a> FxGraphViewer<'a> {
    pub fn new(state: &'a mut FxGraphViewerState, graph: &'a FxGraph) -> Self {
        Self { state, graph }
    }

    /// Renders the viewer. Returns `Some(path)` if the user clicked "Open"
    /// on a referenced texture, so the caller can resolve it against the
    /// bundle index and switch selection.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut open_texture: Option<String> = None;

        if !self.state.show_graph {
            if ui.button("Switch to Graph View").clicked() {
                self.state.show_graph = true;
            }
            ui.separator();
            ui.label("Raw JSON (Graph View Disabled):");
            return None;
        }

        ui.horizontal(|ui| {
            if ui.button("Switch to JSON View").clicked() {
                self.state.show_graph = false;
            }
            if ui.button("Reset View").clicked() {
                self.state.initialized = false;
            }
            ui.label(format!("Zoom: {:.2}", self.state.zoom));
            if ui.button("-").clicked() { self.state.zoom *= 0.8; }
            if ui.button("+").clicked() { self.state.zoom *= 1.25; }

            ui.separator();
            if !self.graph.shader_group.is_empty() {
                ui.label(format!("Shader Group: {}", self.graph.shader_group.join(", ")));
            }
            if let Some(mode) = &self.graph.overriden_blend_mode {
                ui.label(format!("Blend Mode: {}", mode));
            }
        });

        if !self.graph.textures.is_empty() {
            egui::CollapsingHeader::new(format!("Textures ({})", self.graph.textures.len()))
                .default_open(false)
                .show(ui, |ui| {
                    for tex in &self.graph.textures {
                        ui.horizontal(|ui| {
                            ui.monospace(&tex.filename);
                            if let Some(fmt) = &tex.format {
                                ui.label(format!("[{}]", fmt));
                            }
                            if ui.small_button("Open").clicked() {
                                open_texture = Some(tex.filename.clone());
                            }
                        });
                    }
                });
        }

        ui.separator();

        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (response, painter) = ui.allocate_painter(
                ui.available_size().max(egui::vec2(1.0, 1.0)),
                egui::Sense::drag(),
            );

            if response.dragged() {
                self.state.pan += response.drag_delta();
            }

            if response.hovered() {
                let zoom_delta = ui.input(|i| i.zoom_delta());
                self.state.zoom *= zoom_delta;

                ui.input(|i| {
                    if i.modifiers.is_none() {
                        let scroll = i.raw_scroll_delta.y;
                        if scroll > 0.0 {
                            self.state.zoom *= 1.1;
                        } else if scroll < 0.0 {
                            self.state.zoom *= 0.9;
                        }
                    }
                });

                self.state.zoom = self.state.zoom.clamp(0.05, 5.0);
            }

            // Auto-fit the node bounding box into view on first show, or
            // after "Reset View".
            if !self.state.initialized {
                self.state.initialized = true;
                let positions: Vec<[f32; 2]> = self.graph.nodes.iter().map(|n| n.position()).collect();
                if !positions.is_empty() {
                    let min_x = positions.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
                    let max_x = positions.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
                    let min_y = positions.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                    let max_y = positions.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
                    let bbox_w = (max_x - min_x).max(1.0) + 300.0;
                    let bbox_h = (max_y - min_y).max(1.0) + 150.0;
                    let avail = response.rect.size();
                    let fit_zoom = (avail.x / bbox_w).min(avail.y / bbox_h).min(1.0).max(0.05);
                    self.state.zoom = fit_zoom;
                    let center_world = egui::vec2((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                    self.state.pan = -center_world * fit_zoom;
                } else {
                    self.state.zoom = 1.0;
                    self.state.pan = egui::Vec2::ZERO;
                }
            }

            let zoom = self.state.zoom;
            let pan = self.state.pan;
            let center = response.rect.center().to_vec2();
            let to_screen = move |pos: [f32; 2]| -> egui::Pos2 {
                (egui::vec2(pos[0], pos[1]) * zoom + pan).to_pos2() + center
            };

            // Node label + box size, measured once per node per frame (graphs
            // here run in the dozens to low hundreds of nodes, cheap either way).
            let font_id = egui::FontId::monospace(12.0 * zoom.clamp(0.4, 1.5));
            let mut key_to_rect: HashMap<(String, i64), egui::Rect> = HashMap::new();
            let mut node_labels: Vec<(String, egui::Rect, egui::Color32)> = Vec::new();

            for node in &self.graph.nodes {
                let label = if node.index != 0 {
                    format!("{} #{}", node.node_type, node.index)
                } else {
                    node.node_type.clone()
                };
                let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), font_id.clone(), egui::Color32::WHITE));
                let pad = egui::vec2(10.0, 6.0) * zoom.clamp(0.4, 1.5);
                let size = galley.size() + pad * 2.0;
                let center_pos = to_screen(node.position());
                let rect = egui::Rect::from_center_size(center_pos, size);
                key_to_rect.insert(node.key(), rect);
                node_labels.push((label, rect, type_color(&node.node_type)));
            }

            // Links (drawn first, under the node boxes).
            for link in &self.graph.links {
                if let (Some(src_rect), Some(dst_rect)) = (
                    key_to_rect.get(&link.src.key()),
                    key_to_rect.get(&link.dst.key()),
                ) {
                    if !response.rect.expand(200.0).intersects(*src_rect) && !response.rect.expand(200.0).intersects(*dst_rect) {
                        continue;
                    }
                    let src_pt = src_rect.right_center();
                    let dst_pt = dst_rect.left_center();
                    let points = wire_points(src_pt, dst_pt);
                    let stroke = egui::Stroke::new((1.5 * zoom).max(0.6), egui::Color32::from_rgb(140, 140, 150));
                    painter.add(egui::Shape::Path(egui::epaint::PathShape::line(points, stroke)));
                }
            }

            // Nodes.
            for (label, rect, accent) in &node_labels {
                if !response.rect.expand(100.0).intersects(*rect) {
                    continue;
                }
                painter.rect_filled(*rect, 4.0 * zoom, egui::Color32::from_rgb(45, 45, 52));
                painter.rect_stroke(*rect, 4.0 * zoom, egui::Stroke::new(1.5 * zoom.max(0.3), *accent));
                let accent_strip = egui::Rect::from_min_size(rect.min, egui::vec2(3.0 * zoom, rect.height()));
                painter.rect_filled(accent_strip, 0.0, *accent);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    font_id.clone(),
                    egui::Color32::from_rgb(230, 230, 235),
                );
            }

            // Hover tooltips with node parameters, drawn last so they sit atop.
            if let Some(hover_pos) = response.hover_pos() {
                for node in &self.graph.nodes {
                    if let Some(rect) = key_to_rect.get(&node.key()) {
                        if rect.contains(hover_pos) {
                            egui::show_tooltip(ui.ctx(), ui.layer_id(), egui::Id::new("fxgraph_node_tooltip"), |ui| {
                                ui.strong(&node.node_type);
                                if let Some(stage) = &node.stage {
                                    ui.label(format!("stage: {}", stage));
                                }
                                if let Some(cp) = &node.custom_parameter {
                                    ui.label(format!("custom_parameter: {}", cp));
                                }
                                if !node.parameters.is_empty() {
                                    if let Ok(pretty) = serde_json::to_string_pretty(&node.parameters) {
                                        ui.monospace(pretty);
                                    }
                                }
                            });
                            break;
                        }
                    }
                }
            }
        });

        open_texture
    }
}
