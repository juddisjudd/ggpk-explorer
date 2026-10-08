//! `.arm` room canvas: the tile slot grid with doodads, decals, zones and points of interest on top.

use std::collections::BTreeMap;

use crate::parsers::arm::{ArmFile, Decal, Slot, Zone, SUBTILES_PER_TILE, WORLD_UNITS_PER_TILE};
use crate::ui::links;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};

const DECORATION_STUB: &str = "Metadata/MiscellaneousObjects/Doodad";

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    Slot(usize, usize),
    Zone(usize),
    Poi(usize, usize),
    Doodad(usize),
    Decal(usize),
}

pub struct RoomViewerState {
    pan: Vec2,
    zoom: f32,
    selected: Option<Pick>,
    doodads: bool,
    decals: bool,
    zones: bool,
    points: bool,
    ground: bool,
}

impl Default for RoomViewerState {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0, selected: None, doodads: true, decals: true, zones: true, points: true, ground: true }
    }
}

/// Subtile space to screen, with y growing up so the corners' SW sits bottom-left.
struct View {
    origin: Pos2,
    centre: Vec2,
    scale: f32,
}

impl View {
    fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x - self.centre.x, self.centre.y - y) * self.scale
    }

    fn to_world(&self, p: Pos2) -> Vec2 {
        let d = (p - self.origin) / self.scale;
        Vec2::new(self.centre.x + d.x, self.centre.y - d.y)
    }

    fn tiles(&self, col: usize, row: usize, w: u32, h: u32) -> Rect {
        let t = SUBTILES_PER_TILE;
        Rect::from_two_pos(self.to_screen(col as f32 * t, row as f32 * t), self.to_screen((col + w as usize) as f32 * t, (row + h as usize) as f32 * t))
    }

    fn zone(&self, z: &Zone) -> Rect {
        Rect::from_two_pos(self.to_screen(z.x_min as f32, z.y_min as f32), self.to_screen(z.x_max as f32, z.y_max as f32))
    }
}

fn palette(i: usize, dark: bool) -> Color32 {
    let hue = (i as f32 * 0.618_034).fract();
    egui::ecolor::Hsva::new(hue, if dark { 0.5 } else { 0.65 }, if dark { 0.9 } else { 0.7 }, 1.0).into()
}

fn slot_rgb(slot: &Slot) -> (u8, u8, u8) {
    match slot {
        Slot::K(_) => (70, 110, 170),
        Slot::F { .. } => (130, 105, 70),
        Slot::S => (70, 70, 78),
        Slot::O => (150, 80, 170),
        Slot::N => (90, 90, 90),
    }
}

fn decal_pos(d: &Decal) -> (f32, f32) {
    let k = SUBTILES_PER_TILE / WORLD_UNITS_PER_TILE;
    (d.x * k, d.y * k)
}

fn diamond(at: Pos2, s: f32, fill: Color32, stroke: Color32) -> egui::Shape {
    let pts = vec![at + Vec2::new(0.0, -s), at + Vec2::new(s, 0.0), at + Vec2::new(0.0, s), at + Vec2::new(-s, 0.0)];
    egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0_f32, stroke))
}

fn or_untagged(tag: &str) -> &str {
    if tag.is_empty() { "(untagged)" } else { tag }
}

/// Covered `n` cells belong to the `k` at a lower row and column whose footprint spans them.
fn slot_at(arm: &ArmFile, col: usize, row: usize) -> Option<(usize, usize)> {
    if !matches!(arm.grid.get(row)?.get(col)?, Slot::N) {
        return Some((col, row));
    }
    for r in (0..=row).rev() {
        for c in (0..=col).rev() {
            if let Some(s @ Slot::K(_)) = arm.grid[r].get(c) {
                let (w, h) = s.footprint();
                if c + w as usize > col && r + h as usize > row {
                    return Some((c, r));
                }
            }
        }
    }
    None
}

fn slot_summary(slot: &Slot) -> String {
    match slot {
        Slot::K(k) => {
            let mut s = format!("k {}×{} · origin {:?}", k.width, k.height, k.origin);
            if let Some(tag) = &k.slot_tag {
                s += &format!(" · tag {}", tag);
            }
            for e in &k.edges {
                s += &format!("\n{:?} edge {} · exit {}/{}", e.direction, e.edge.as_deref().unwrap_or("-"), e.exit, e.virtual_exit);
            }
            for c in &k.corners {
                s += &format!("\n{:?} ground {} · height {}", c.direction, c.ground.as_deref().unwrap_or("-"), c.height);
            }
            s
        }
        Slot::F { fill } => format!("f · fill {}", fill.as_deref().unwrap_or("-")),
        other => other.letter().to_string(),
    }
}

fn ground_files(arm: &ArmFile) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let corners = arm.grid.iter().flatten().filter_map(|s| match s {
        Slot::K(k) => Some(k.corners.iter().filter_map(|c| c.ground.as_deref())),
        _ => None,
    });
    let overrides = arm.ground_overrides.iter().flatten().flatten().flatten().map(String::as_str);
    for g in corners.flatten().chain(overrides) {
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

pub struct RoomViewer;

impl RoomViewer {
    pub fn show(ui: &mut egui::Ui, id: u64, arm: &ArmFile, state: &mut RoomViewerState) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        let (width, height) = arm.root_slot.footprint();
        let zones = arm.zones.as_deref().unwrap_or(&[]);
        let poi_count: usize = arm.points_of_interest.iter().map(Vec::len).sum();
        let grounds = ground_files(arm);
        let ground_color = |g: &str| palette(grounds.iter().position(|x| *x == g).unwrap_or(0), dark);

        ui.horizontal_wrapped(|ui| {
            crate::ui::components::badge(ui, &format!("v{}", arm.version));
            crate::ui::components::badge(ui, &format!("{}×{} tiles", width, height));
            if !arm.tag.is_empty() {
                crate::ui::components::badge(ui, &arm.tag);
            }
            ui.separator();
            ui.toggle_value(&mut state.doodads, format!("{} doodads", arm.doodads.len()));
            ui.toggle_value(&mut state.decals, format!("{} decals", arm.decals.len()));
            ui.toggle_value(&mut state.zones, format!("{} zones", zones.len()));
            ui.toggle_value(&mut state.points, format!("{} points", poi_count));
            ui.toggle_value(&mut state.ground, "Ground");
            if ui.button("Fit").clicked() {
                state.pan = Vec2::ZERO;
                state.zoom = 1.0;
            }
            ui.label(RichText::new("drag: pan · wheel: zoom · click: open or select").weak().size(10.5));
        });
        ui.separator();

        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail.x.max(200.0), (avail.y * 0.68).max(260.0)), egui::Sense::click_and_drag());
        if resp.dragged() {
            state.pan += resp.drag_delta();
        }
        if let (true, Some(m)) = (resp.hovered(), resp.hover_pos()) {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            let wheel = if scroll > 0.0 { 1.15 } else if scroll < 0.0 { 1.0 / 1.15 } else { 1.0 };
            let zoom = (state.zoom * wheel * ui.input(|i| i.zoom_delta())).clamp(0.2, 60.0);
            if zoom != state.zoom {
                let anchor = m - rect.center();
                state.pan = anchor - (anchor - state.pan) * (zoom / state.zoom);
                state.zoom = zoom;
            }
        }

        let room = Vec2::new(width.max(1) as f32, height.max(1) as f32) * SUBTILES_PER_TILE;
        let fit = ((rect.width() - 16.0) / room.x).min((rect.height() - 16.0) / room.y).max(0.01);
        let view = View { origin: rect.center() + state.pan, centre: room / 2.0, scale: fit * state.zoom };
        let tile_px = view.scale * SUBTILES_PER_TILE;
        let dot_r = (view.scale * 1.2).clamp(2.5, 7.0);

        let painter = ui.painter_at(rect);
        let text_c = if dark { Color32::from_rgb(230, 230, 236) } else { Color32::from_rgb(30, 30, 36) };
        let line_c = if dark { Color32::from_rgb(60, 60, 70) } else { Color32::from_rgb(200, 200, 212) };
        painter.rect_filled(rect, 4.0, if dark { Color32::from_rgb(22, 22, 28) } else { Color32::from_rgb(244, 244, 248) });

        let bounds = Rect::from_two_pos(view.to_screen(0.0, 0.0), view.to_screen(room.x, room.y));
        if tile_px >= 6.0 {
            for c in 0..=width {
                let x = view.to_screen(c as f32 * SUBTILES_PER_TILE, 0.0).x;
                painter.line_segment([Pos2::new(x, bounds.top()), Pos2::new(x, bounds.bottom())], Stroke::new(1.0_f32, line_c));
            }
            for r in 0..=height {
                let y = view.to_screen(0.0, r as f32 * SUBTILES_PER_TILE).y;
                painter.line_segment([Pos2::new(bounds.left(), y), Pos2::new(bounds.right(), y)], Stroke::new(1.0_f32, line_c));
            }
        }
        let font = egui::FontId::monospace((tile_px * 0.4).clamp(8.0, 14.0));
        for (r, row) in arm.grid.iter().enumerate() {
            for (c, slot) in row.iter().enumerate() {
                if matches!(slot, Slot::N) {
                    continue;
                }
                let (w, h) = slot.footprint();
                let cell = view.tiles(c, r, w, h).shrink(1.0);
                if !cell.intersects(rect) {
                    continue;
                }
                let one_ground = match slot {
                    Slot::K(k) if state.ground && w == 1 && h == 1 => k.corners[0].ground.as_deref().filter(|g| k.corners.iter().all(|c| c.ground.as_deref() == Some(*g))),
                    _ => None,
                };
                let (cr, cg, cb) = slot_rgb(slot);
                let fill = match one_ground {
                    Some(g) => ground_color(g).gamma_multiply(0.45),
                    None => Color32::from_rgba_unmultiplied(cr, cg, cb, if dark { 70 } else { 55 }),
                };
                painter.rect_filled(cell, 2.0, fill);
                painter.rect_stroke(cell, 2.0, Stroke::new(1.0_f32, Color32::from_rgb(cr, cg, cb)));
                if tile_px >= 16.0 {
                    let label = match slot {
                        Slot::K(k) if w > 1 || h > 1 => format!("k {}×{}", k.width, k.height),
                        _ => slot.letter().to_string(),
                    };
                    painter.text(cell.center(), egui::Align2::CENTER_CENTER, label, font.clone(), text_c.gamma_multiply(0.8));
                }
                if let (true, Slot::K(k)) = (state.ground && one_ground.is_none() && tile_px >= 12.0, slot) {
                    let size = (tile_px * 0.12).clamp(3.0, 8.0);
                    let at = [cell.left_bottom(), cell.right_bottom(), cell.right_top(), cell.left_top()];
                    for (corner, p) in k.corners.iter().zip(at) {
                        if let Some(g) = &corner.ground {
                            let inset = p + (cell.center() - p).normalized() * size * 1.4;
                            painter.rect_filled(Rect::from_center_size(inset, Vec2::splat(size)), 1.0, ground_color(g));
                        }
                    }
                }
            }
        }
        if let (true, Some(overrides)) = (state.ground && tile_px >= 8.0, &arm.ground_overrides) {
            let size = (tile_px * 0.12).clamp(3.0, 7.0);
            for (i, row) in overrides.iter().enumerate() {
                for (j, g) in row.iter().enumerate() {
                    if let Some(g) = g {
                        let at = view.to_screen((j + 1) as f32 * SUBTILES_PER_TILE, (i + 1) as f32 * SUBTILES_PER_TILE);
                        painter.add(diamond(at, size, ground_color(g), text_c));
                    }
                }
            }
        }
        painter.rect_stroke(bounds, 0.0, Stroke::new(1.5_f32, text_c.gamma_multiply(0.6)));

        if state.zones {
            for (i, z) in zones.iter().enumerate() {
                let zr = view.zone(z);
                let c = palette(i * 3 + 1, dark);
                painter.rect_filled(zr, 0.0, c.gamma_multiply(0.12));
                painter.rect_stroke(zr, 0.0, Stroke::new(1.5_f32, c));
                if zr.width() > 40.0 && zr.height() > 14.0 {
                    painter.text(zr.left_top() + Vec2::new(3.0, 2.0), egui::Align2::LEFT_TOP, &z.name, egui::FontId::proportional(10.0), c);
                }
            }
        }
        if state.decals {
            for d in &arm.decals {
                let (x, y) = decal_pos(d);
                painter.rect_stroke(Rect::from_center_size(view.to_screen(x, y), Vec2::splat(dot_r * 2.0)), 1.0, Stroke::new(1.5_f32, Color32::from_rgb(234, 140, 60)));
            }
        }
        if state.doodads {
            let link_c = if dark { Color32::from_rgb(200, 200, 120) } else { Color32::from_rgb(150, 120, 30) };
            for conn in arm.doodad_connections.iter().flatten() {
                if let (Some(a), Some(b)) = (arm.doodads.get(conn.from as usize), arm.doodads.get(conn.to as usize)) {
                    painter.line_segment([view.to_screen(a.x as f32, a.y as f32), view.to_screen(b.x as f32, b.y as f32)], Stroke::new(1.0_f32, link_c));
                }
            }
            for d in &arm.doodads {
                let c = if d.stub == DECORATION_STUB { Color32::from_rgb(74, 200, 120) } else { Color32::from_rgb(250, 204, 21) };
                painter.circle(view.to_screen(d.x as f32, d.y as f32), dot_r, c, Stroke::new(1.0_f32, Color32::from_black_alpha(160)));
            }
        }
        if state.points {
            for p in arm.points_of_interest.iter().flatten() {
                let at = view.to_screen(p.x as f32, p.y as f32);
                painter.add(diamond(at, dot_r + 2.0, Color32::from_rgb(239, 68, 68), Color32::from_black_alpha(180)));
                if !p.tag.is_empty() && tile_px >= 10.0 {
                    painter.text(at + Vec2::new(dot_r + 4.0, 0.0), egui::Align2::LEFT_CENTER, &p.tag, egui::FontId::proportional(10.0), text_c);
                }
            }
        }

        let hovered = resp.hover_pos().and_then(|m| {
            let near = |x: f32, y: f32, r: f32| view.to_screen(x, y).distance(m) <= r + 3.0;
            if state.points {
                for (g, group) in arm.points_of_interest.iter().enumerate() {
                    if let Some(i) = group.iter().position(|p| near(p.x as f32, p.y as f32, dot_r + 2.0)) {
                        return Some(Pick::Poi(g, i));
                    }
                }
            }
            if state.doodads {
                if let Some(i) = arm.doodads.iter().rposition(|d| near(d.x as f32, d.y as f32, dot_r)) {
                    return Some(Pick::Doodad(i));
                }
            }
            if state.decals {
                if let Some(i) = arm.decals.iter().rposition(|d| {
                    let (x, y) = decal_pos(d);
                    near(x, y, dot_r)
                }) {
                    return Some(Pick::Decal(i));
                }
            }
            let w = view.to_world(m);
            if state.zones {
                let inside = zones.iter().enumerate().filter(|(_, z)| w.x >= z.x_min as f32 && w.x <= z.x_max as f32 && w.y >= z.y_min as f32 && w.y <= z.y_max as f32);
                if let Some((i, _)) = inside.min_by_key(|(_, z)| (z.x_max - z.x_min) as i64 * (z.y_max - z.y_min) as i64) {
                    return Some(Pick::Zone(i));
                }
            }
            if w.x < 0.0 || w.y < 0.0 {
                return None;
            }
            slot_at(arm, (w.x / SUBTILES_PER_TILE) as usize, (w.y / SUBTILES_PER_TILE) as usize).map(|(c, r)| Pick::Slot(c, r))
        });

        if let Some(pick) = hovered {
            let tip = match pick {
                Pick::Poi(g, i) => {
                    let p = &arm.points_of_interest[g][i];
                    format!("Point of interest {}\ngroup {}\nat ({}, {}) · rotation {:.0}°", or_untagged(&p.tag), g + 1, p.x, p.y, p.rotation.to_degrees())
                }
                Pick::Doodad(i) => {
                    let d = &arm.doodads[i];
                    let mut s = format!("Doodad {}\n{}\n{}\nat ({}, {}) · rotation {:.0}° · scale {}", i, d.ao_file, d.stub, d.x, d.y, d.radians1.to_degrees(), d.scale);
                    for (k, v) in d.key_values.iter().flatten() {
                        s += &format!("\n{} = {}", k, v);
                    }
                    s + "\nClick to open"
                }
                Pick::Decal(i) => {
                    let d = &arm.decals[i];
                    let (x, y) = decal_pos(d);
                    format!("Decal {}\n{}\nat ({:.0}, {:.0}) · rotation {:.0}° · scale {}\nClick to open", or_untagged(&d.tag), d.atlas_file, x, y, d.rotation.to_degrees(), d.scale)
                }
                Pick::Zone(i) => {
                    let z = &zones[i];
                    let mut s = format!("Zone {}\n({}, {}) to ({}, {})", z.name, z.x_min, z.y_min, z.x_max, z.y_max);
                    for f in [&z.disable_teleports, &z.env_file].into_iter().flatten().filter(|f| !f.is_empty()) {
                        s += &format!("\n{}", f);
                    }
                    s
                }
                Pick::Slot(c, r) => format!("Tile ({}, {})\n{}", c, r, slot_summary(&arm.grid[r][c])),
            };
            resp.clone().on_hover_text_at_pointer(tip);
            if resp.clicked() {
                match pick {
                    Pick::Doodad(i) => opened = Some(links::normalize(&arm.doodads[i].ao_file)),
                    Pick::Decal(i) => opened = Some(links::normalize(&arm.decals[i].atlas_file)),
                    other => state.selected = if state.selected == Some(other) { None } else { Some(other) },
                }
            }
        } else if resp.clicked() {
            state.selected = None;
        }
        let highlight = match state.selected {
            Some(Pick::Slot(c, r)) => {
                let (w, h) = arm.grid[r][c].footprint();
                Some(view.tiles(c, r, w, h))
            }
            Some(Pick::Zone(i)) => zones.get(i).map(|z| view.zone(z)),
            Some(Pick::Poi(g, i)) => arm.points_of_interest.get(g).and_then(|g| g.get(i)).map(|p| Rect::from_center_size(view.to_screen(p.x as f32, p.y as f32), Vec2::splat(dot_r * 3.0 + 4.0))),
            _ => None,
        };
        if let Some(at) = highlight {
            painter.rect_stroke(at, 2.0, Stroke::new(2.0_f32, text_c));
        }

        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt(("arm_details", id)).auto_shrink([false, false]).show(ui, |ui| {
            match state.selected {
                Some(Pick::Slot(c, r)) => {
                    let slot = &arm.grid[r][c];
                    ui.label(RichText::new(format!("Tile ({}, {}) · {}", c, r, slot.letter())).strong());
                    match slot {
                        Slot::K(k) => {
                            ui.label(format!("{}×{} tiles · origin {:?}{}", k.width, k.height, k.origin, k.slot_tag.as_deref().map(|t| format!(" · tag {}", t)).unwrap_or_default()));
                            egui::Grid::new(("arm_slot", id)).num_columns(3).spacing([12.0, 3.0]).striped(true).show(ui, |ui| {
                                for e in &k.edges {
                                    ui.label(format!("{:?} edge", e.direction));
                                    links::maybe_link(ui, e.edge.as_deref().unwrap_or("-"), true, &mut opened);
                                    ui.label(RichText::new(format!("exit {} / {}", e.exit, e.virtual_exit)).weak());
                                    ui.end_row();
                                }
                                for corner in &k.corners {
                                    ui.label(format!("{:?} corner", corner.direction));
                                    links::maybe_link(ui, corner.ground.as_deref().unwrap_or("-"), true, &mut opened);
                                    ui.label(RichText::new(format!("height {}", corner.height)).weak());
                                    ui.end_row();
                                }
                            });
                        }
                        Slot::F { fill: Some(f) } => {
                            links::maybe_link(ui, f, true, &mut opened);
                        }
                        _ => {}
                    }
                    ui.separator();
                }
                Some(Pick::Zone(i)) => {
                    let z = &zones[i];
                    ui.label(RichText::new(format!("Zone {} · ({}, {}) to ({}, {})", z.name, z.x_min, z.y_min, z.x_max, z.y_max)).strong());
                    for f in [&z.disable_teleports, &z.env_file].into_iter().flatten().filter(|f| !f.is_empty()) {
                        links::maybe_link(ui, f, true, &mut opened);
                    }
                    ui.separator();
                }
                Some(Pick::Poi(g, i)) => {
                    let p = &arm.points_of_interest[g][i];
                    ui.label(RichText::new(format!("Point of interest {} · group {} · ({}, {}) · rotation {:.0}°", or_untagged(&p.tag), g + 1, p.x, p.y, p.rotation.to_degrees())).strong());
                    ui.separator();
                }
                _ => {}
            }
            if state.ground && !grounds.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Ground").weak());
                    for (i, g) in grounds.iter().enumerate() {
                        let (swatch, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                        ui.painter().rect_filled(swatch, 2.0, palette(i, dark));
                        links::maybe_link(ui, g, true, &mut opened);
                    }
                });
            }
            let mut placed: BTreeMap<&str, usize> = BTreeMap::new();
            for f in arm.doodads.iter().map(|d| d.ao_file.as_str()).chain(arm.decals.iter().map(|d| d.atlas_file.as_str())) {
                *placed.entry(f).or_default() += 1;
            }
            egui::CollapsingHeader::new(format!("Placed files ({})", placed.len())).id_salt(("arm_placed", id)).show(ui, |ui| {
                egui::Grid::new(("arm_placed_grid", id)).num_columns(2).spacing([12.0, 3.0]).striped(true).show(ui, |ui| {
                    for (f, n) in &placed {
                        ui.label(RichText::new(format!("{}×", n)).monospace().weak());
                        links::maybe_link(ui, f, true, &mut opened);
                        ui.end_row();
                    }
                });
            });
            egui::CollapsingHeader::new(format!("Referenced files ({})", arm.strings.len())).id_salt(("arm_strings", id)).show(ui, |ui| {
                for s in &arm.strings {
                    links::maybe_link(ui, s, true, &mut opened);
                }
            });
        });
        opened
    }
}
