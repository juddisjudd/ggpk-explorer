//! `.dgr` dungeon-graph canvas: the room grid with nodes and their connections.

use crate::parsers::level::{DgrGraph, CELL_PITCH};
use crate::ui::links;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke, Vec2};

#[derive(Default)]
pub struct LevelViewerState {
    pub selected: Option<usize>,
}

fn node_color(name: &str, dark: bool) -> Color32 {
    match name.to_ascii_lowercase().as_str() {
        "entrance" | "entrance1" | "stairsup1" => Color32::from_rgb(74, 222, 128),
        "boss" | "exit" => Color32::from_rgb(239, 68, 68),
        "" => {
            if dark { Color32::from_rgb(120, 130, 150) } else { Color32::from_rgb(150, 160, 180) }
        }
        _ => Color32::from_rgb(96, 165, 250),
    }
}

pub struct LevelViewer;

impl LevelViewer {
    pub fn show(ui: &mut egui::Ui, id: u64, graph: &DgrGraph, state: &mut LevelViewerState) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        ui.horizontal_wrapped(|ui| {
            if let Some(v) = graph.version {
                crate::ui::components::badge(ui, &format!("v{}", v));
            }
            crate::ui::components::badge(ui, &format!("{}×{}", graph.width, graph.height));
            crate::ui::components::badge(ui, &format!("{} nodes", graph.nodes.len()));
            crate::ui::components::badge(ui, &format!("{} edges", graph.edges.len()));
            if !graph.master.is_empty() {
                ui.label(RichText::new("master").weak());
                links::maybe_link(ui, &graph.master, false, &mut opened);
            }
        });
        ui.separator();

        let avail = ui.available_size();
        let cell = ((avail.x.min(avail.y - 140.0)).max(200.0) / graph.width.max(graph.height).max(1) as f32).clamp(28.0, 90.0);
        let size = Vec2::new(cell * graph.width as f32, cell * graph.height as f32);
        let (rect, resp) = ui.allocate_exact_size(size + Vec2::splat(2.0), egui::Sense::click());
        let painter = ui.painter_at(rect);
        let bg = if dark { Color32::from_rgb(26, 26, 32) } else { Color32::from_rgb(244, 244, 248) };
        let grid = if dark { Color32::from_rgb(50, 50, 58) } else { Color32::from_rgb(210, 210, 220) };
        let text_c = if dark { Color32::from_rgb(230, 230, 236) } else { Color32::from_rgb(30, 30, 36) };
        painter.rect_filled(rect, 4.0, bg);
        for i in 0..=graph.width {
            let x = rect.left() + 1.0 + i as f32 * cell;
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, grid));
        }
        for j in 0..=graph.height {
            let y = rect.top() + 1.0 + j as f32 * cell;
            painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, grid));
        }
        let center = |n: &crate::parsers::level::DgrNode| Pos2::new(rect.left() + 1.0 + n.x / CELL_PITCH * cell, rect.top() + 1.0 + n.y / CELL_PITCH * cell);
        let edge_c = if dark { Color32::from_rgb(160, 160, 175) } else { Color32::from_rgb(90, 90, 110) };
        for e in &graph.edges {
            if let (Some(a), Some(b)) = (graph.nodes.get(e.from), graph.nodes.get(e.to)) {
                painter.line_segment([center(a), center(b)], Stroke::new(2.0_f32, edge_c));
            }
        }
        let r = (cell * 0.22).clamp(6.0, 16.0);
        let mut clicked: Option<usize> = None;
        let hover = resp.hover_pos();
        for (i, n) in graph.nodes.iter().enumerate() {
            let c = center(n);
            let selected = state.selected == Some(i);
            painter.circle_filled(c, if selected { r + 2.0 } else { r }, node_color(&n.name, dark));
            painter.circle_stroke(c, if selected { r + 2.0 } else { r }, Stroke::new(1.5_f32, text_c));
            painter.text(c, egui::Align2::CENTER_CENTER, i.to_string(), egui::FontId::proportional((r * 1.1).max(9.0)), Color32::BLACK);
            if !n.name.is_empty() {
                painter.text(c + Vec2::new(0.0, r + 2.0), egui::Align2::CENTER_TOP, &n.name, egui::FontId::proportional(10.0), text_c);
            }
            if hover.map(|h| h.distance(c) <= r + 2.0).unwrap_or(false) {
                resp.clone().on_hover_text(format!("#{} {} {}\n{}", i, n.name, n.rotation, n.raw));
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
        }
        if let Some(i) = clicked {
            state.selected = if state.selected == Some(i) { None } else { Some(i) };
        }

        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt(("dgr_lists", id)).auto_shrink([false, false]).show(ui, |ui| {
            if let Some(i) = state.selected {
                if let Some(n) = graph.nodes.get(i) {
                    ui.label(RichText::new(format!("Node {} · {} · rotation {} · tiles {:?}", i, if n.name.is_empty() { "(unnamed)" } else { &n.name }, n.rotation, n.tile_refs)).strong());
                    ui.label(RichText::new(&n.raw).monospace().weak());
                    let connected: Vec<&crate::parsers::level::DgrEdge> = graph.edges.iter().filter(|e| e.from == i || e.to == i).collect();
                    for e in connected {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} ↔ {}", e.from, e.to)).monospace());
                            links::maybe_link(ui, &e.path, true, &mut opened);
                        });
                    }
                    ui.separator();
                }
            }
            egui::CollapsingHeader::new("Edges").id_salt(("dgr_edges", id)).default_open(state.selected.is_none()).show(ui, |ui| {
                egui::Grid::new(("dgr_edge_grid", id)).num_columns(3).spacing([12.0, 3.0]).striped(true).show(ui, |ui| {
                    for e in &graph.edges {
                        ui.label(RichText::new(format!("{} → {}", e.from, e.to)).monospace());
                        links::maybe_link(ui, &e.path, true, &mut opened);
                        ui.label(RichText::new(&e.raw).weak().size(10.5));
                        ui.end_row();
                    }
                });
            });
        });
        opened
    }
}
