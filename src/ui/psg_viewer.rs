#![allow(dead_code)]
use crate::dat::psg::PsgFile;
use crate::ui::atlas_node_db::{tree_context_for_graph_type, SkillGraphDatabase, SkillGraphNodeInfo};
use crate::ui::skill_tree_art::{FrameArt, NodeFrameKind, SkillTreeArtSet};
use crate::ui::skill_tree_layout::{self, TreeLayout, ASCENDANCY_PLATE_SIZE, CLASS_ILLUSTRATION_SIZE, MAIN_CIRCLE_SIZE};
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Vec2};
use std::collections::HashMap;
use std::sync::Arc;

// Art the client hardcodes rather than referencing from a DAT row.
pub const PLUS_FRAME_NORMAL: &str = "Art/2DArt/UIImages/InGame/PassiveSkillScreenPlusFrameNormal";
pub const PLUS_FRAME_ACTIVE: &str = "Art/2DArt/UIImages/InGame/PassiveSkillScreenPlusFrameActive";
pub const MAIN_CIRCLE: &str = "Art/2DArt/UIImages/InGame/PassiveTree/PassiveTreeMainCircle";
pub const MAIN_CIRCLE_ACTIVE: &str = "Art/2DArt/UIImages/InGame/PassiveTree/PassiveTreeMainCircleActive";
pub const ATLAS_START: &str = "Art/2DArt/UIImages/InGame/AtlasScreen/AtlasPassiveSkillScreenStart";
pub const BREACH_BACKDROP: &str = "Art/2DArt/UIImages/InGame/BreachLeague/BreachTreePassiveBackground";
pub const BREACH_START: &str = "Art/2DArt/UIImages/InGame/BreachLeague/BreachTreePassiveSkillScreenStartingPoint";

/// `Art/2DArt/PassiveTree/*CurvesTogether.dds`: nine quarter-arcs (one per
/// orbit radius, centred on the sheet's bottom-right corner) plus a straight
/// strip along the top. Tiled for straight connectors, so it needs wrapping.
pub fn is_connector_sheet(path: &str) -> bool {
    path.to_ascii_lowercase().contains("2dart/passivetree/")
}

const SHEET_SIZE: f32 = 1436.0;
/// Rows of the straight strip inside the sheet and the world height it is drawn at.
const SHEET_LINE_ROWS: (f32, f32) = (32.0, 56.0);
const LINE_WORLD_HEIGHT: f32 = 24.0;
/// Half-width of the band sampled around an orbit ring (stroke is ~12 px).
const ARC_BAND: f32 = 12.0;

/// Frame textures are authored at one pixel per world unit (the official web
/// tree draws them that way); icons sit inside at this fraction of the frame.
const ICON_TO_FRAME: f32 = 0.69;
/// How far the atlas main-tree backdrop reaches past its nodes. The painted
/// machinery fills roughly 75% by 87% of the square texture, so a little over
/// one covers the tree; the texture is square, so the scale is applied to the
/// longer side and both axes get it.
pub const ATLAS_MAIN_TREE_BG_SCALE: f32 = 1.15;
/// The same, for the league subtree backdrops, which carry far more padding.
pub const ATLAS_SUBTREE_BG_SCALE: f32 = 1.9;
/// Smallest a subtree backdrop is drawn, for a subtree of one or two nodes.
pub const ATLAS_SUBTREE_BG_MIN: f32 = 200.0;
/// Fallback frame sizes (px) when a texture hasn't loaded yet.
const FRAME_PASSIVE: f32 = 104.0;
const FRAME_NOTABLE: f32 = 152.0;
const FRAME_KEYSTONE: f32 = 220.0;
const FRAME_JEWEL: f32 = 104.0;
const FRAME_ASC_SMALL: f32 = 160.0;
const FRAME_ASC_NOTABLE: f32 = 208.0;
const FRAME_ASC_MIDDLE: f32 = 92.0;
/// `PassiveTreeMainCircleActive` (356 px) lights up one class-start roundel.
/// It is authored for the roundel at the top of the ring — the ring segment
/// arcs over the quatrefoil, which sits 47 px below the texture centre — so it
/// is rotated by the start node's own polar angle and pushed outward to match.
/// Sizes are in centre-ring pixels (see [`skill_tree_layout::RING_PX`]).
const ACTIVE_MARKER_SIZE: f32 = 356.0 * skill_tree_layout::RING_PX;
const ACTIVE_MARKER_OFFSET: f32 = 47.0 * skill_tree_layout::RING_PX;

const DIM: Color32 = Color32::from_rgb(120, 120, 120);
const FULL: Color32 = Color32::WHITE;
/// Cluster "mastery" rows (`IsJustIcon`) are not nodes in PoE 2: the client
/// draws the group's `MasteryBackgroundGraphic` pattern behind the cluster,
/// faded until the cluster is highlighted.
const MASTERY_PATTERN_SIZE: f32 = 400.0;
const MASTERY_ALPHA: u8 = 120;

/// Fallback frame size (world units) by node type.
fn frame_size(info: &SkillGraphNodeInfo, in_ascendancy: bool) -> f32 {
    if info.is_ascendancy_start {
        FRAME_ASC_MIDDLE
    } else if info.is_keystone {
        FRAME_KEYSTONE
    } else if info.is_jewel_socket {
        FRAME_JEWEL
    } else if info.is_notable {
        if in_ascendancy { FRAME_ASC_NOTABLE } else { FRAME_NOTABLE }
    } else if in_ascendancy {
        FRAME_ASC_SMALL
    } else {
        FRAME_PASSIVE
    }
}

fn node_frame_kind(info: &SkillGraphNodeInfo) -> NodeFrameKind {
    if info.is_ascendancy_start {
        NodeFrameKind::AscendancyStart
    } else if info.is_keystone {
        NodeFrameKind::Keystone
    } else if info.is_notable {
        NodeFrameKind::Notable
    } else if info.is_jewel_socket {
        NodeFrameKind::Jewel
    } else if info.is_multiple_choice {
        NodeFrameKind::MultipleChoice
    } else {
        NodeFrameKind::Passive
    }
}

pub struct PsgViewerState {
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub show_graph: bool,
    pub hovered_node: Option<u32>,
    /// Skill graph node database (name/stats/art by PassiveSkillGraphId),
    /// shared across passive/atlas/league trees, set by content_view once
    /// resolved.
    pub skill_db: Option<Arc<SkillGraphDatabase>>,
    /// `Characters` row shown in the centre and highlighted at its start.
    pub selected_class: Option<usize>,
    /// `Ascendancy` row drawn at full colour; the others are dimmed.
    pub selected_ascendancy: Option<usize>,
    /// The user chose "None": show the base class with no ascendancy layer.
    pub ascendancy_none: bool,
    pub dim_other_ascendancies: bool,
    /// Set by the toolbar's export button; `content_view` picks it up,
    /// asks for a folder and runs the export.
    pub export_requested: bool,
    /// Progress/result text of the last skill tree export.
    pub export_status: Option<String>,
    layout: Option<(usize, Arc<TreeLayout>)>,
}

impl Default for PsgViewerState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 0.2,
            show_graph: true,
            hovered_node: None,
            skill_db: None,
            selected_class: None,
            selected_ascendancy: None,
            ascendancy_none: false,
            dim_other_ascendancies: true,
            export_requested: false,
            export_status: None,
            layout: None,
        }
    }
}

impl PsgViewerState {
    fn layout_for(&mut self, psg: &PsgFile) -> Arc<TreeLayout> {
        let key = self.skill_db.as_ref().map(|db| Arc::as_ptr(db) as usize).unwrap_or(0);
        if let Some((k, l)) = &self.layout {
            if *k == key {
                return l.clone();
            }
        }
        let layout = Arc::new(skill_tree_layout::compute(psg, self.skill_db.as_deref()));
        self.layout = Some((key, layout.clone()));
        layout
    }

    /// Default to the first playable class and its first ascendancy once the
    /// database is available.
    fn ensure_selection(&mut self) {
        let Some(db) = &self.skill_db else { return };
        if self.selected_class.map(|c| !db.playable_characters().contains(&c)).unwrap_or(true) {
            self.selected_class = db.playable_characters().first().copied();
            self.selected_ascendancy = None;
            self.ascendancy_none = false;
        }
        if let Some(c) = self.selected_class {
            let options = db.ascendancies_of(c);
            if self.selected_ascendancy.map(|a| !options.contains(&a)).unwrap_or(!self.ascendancy_none) {
                self.selected_ascendancy = options.first().copied();
            }
        }
    }
}

pub struct PsgViewer<'a> {
    pub state: &'a mut PsgViewerState,
    pub psg: &'a PsgFile,
    pub texture_cache: &'a HashMap<String, egui::TextureHandle>,
    /// True while the skill graph database and/or its art textures are
    /// still being fetched/decoded in the background — drives a small
    /// loading indicator so pop-in doesn't look like nothing is happening.
    pub is_loading_art: bool,
    /// Textures still queued (shown next to the spinner).
    pub art_pending: usize,
}

struct Canvas {
    rect: Rect,
    zoom: f32,
    pan: Vec2,
}

impl Canvas {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        (p.to_vec2() * self.zoom + self.pan).to_pos2() + self.rect.center().to_vec2()
    }

    fn world_rect(&self, center: Pos2, size: Vec2) -> Rect {
        Rect::from_center_size(self.to_screen(center), size * self.zoom)
    }

    fn visible(&self, r: Rect) -> bool {
        self.rect.intersects(r)
    }
}

const UV_FULL: Rect = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));

fn draw_image(painter: &egui::Painter, cv: &Canvas, tex: &egui::TextureHandle, center: Pos2, size: Vec2, tint: Color32) {
    let rect = cv.world_rect(center, size);
    if cv.visible(rect) {
        painter.image(tex.id(), rect, UV_FULL, tint);
    }
}

/// Draws `tex` centred on `center` (world) rotated by `angle` (radians,
/// clockwise, 0 = as authored) — egui's image shape can't rotate, so build the quad.
fn draw_image_rotated(painter: &egui::Painter, cv: &Canvas, tex: &egui::TextureHandle, center: Pos2, size: Vec2, angle: f32, tint: Color32) {
    let rect = cv.world_rect(center, size);
    if !cv.visible(rect) {
        return;
    }
    let c = rect.center();
    let (s, co) = angle.sin_cos();
    let half = rect.size() / 2.0;
    let rot = |v: Vec2| vec2(v.x * co - v.y * s, v.x * s + v.y * co);
    let corners = [
        (c + rot(vec2(-half.x, -half.y)), pos2(0.0, 0.0)),
        (c + rot(vec2(half.x, -half.y)), pos2(1.0, 0.0)),
        (c + rot(vec2(half.x, half.y)), pos2(1.0, 1.0)),
        (c + rot(vec2(-half.x, half.y)), pos2(0.0, 1.0)),
    ];
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (p, uv) in corners {
        mesh.vertices.push(egui::epaint::Vertex { pos: p, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

impl<'a> PsgViewer<'a> {
    pub fn new(
        state: &'a mut PsgViewerState,
        psg: &'a PsgFile,
        texture_cache: &'a HashMap<String, egui::TextureHandle>,
        is_loading_art: bool,
    ) -> Self {
        Self { state, psg, texture_cache, is_loading_art, art_pending: 0 }
    }

    /// Looks up a DDS path in the shared texture cache, trying the literal
    /// path and a `.dds`-suffixed variant (DAT tables store some texture
    /// paths with the extension and some without).
    fn find_texture(&self, path: &str) -> Option<&egui::TextureHandle> {
        if path.is_empty() {
            return None;
        }
        self.texture_cache
            .get(path)
            .or_else(|| self.texture_cache.get(&format!("{}.dds", path)))
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Switch to JSON View").clicked() {
                self.state.show_graph = false;
            }
            if ui.button("Reset View").clicked() {
                self.state.pan = egui::Vec2::ZERO;
                self.state.zoom = 0.2;
            }
            ui.label(format!("Zoom: {:.2}", self.state.zoom));
            if ui.button("-").clicked() {
                self.state.zoom *= 0.8;
            }
            if ui.button("+").clicked() {
                self.state.zoom *= 1.25;
            }

            if self.psg.graph_type == 0 {
                if let Some(db) = self.state.skill_db.clone() {
                    ui.separator();
                    let class_name = self.state.selected_class.and_then(|c| db.characters.get(c)).map(|c| c.name.clone()).unwrap_or_else(|| "Class".into());
                    egui::ComboBox::from_id_salt("psg_class").selected_text(class_name).show_ui(ui, |ui| {
                        for c in db.playable_characters() {
                            if ui.selectable_label(self.state.selected_class == Some(c), &db.characters[c].name).clicked() {
                                self.state.selected_class = Some(c);
                                self.state.selected_ascendancy = None;
                                self.state.ascendancy_none = false;
                            }
                        }
                    });
                    if let Some(c) = self.state.selected_class {
                        let asc_name = self
                            .state
                            .selected_ascendancy
                            .and_then(|a| db.ascendancies.get(a))
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| if self.state.ascendancy_none { "None".into() } else { "Ascendancy".into() });
                        egui::ComboBox::from_id_salt("psg_ascendancy").selected_text(asc_name).show_ui(ui, |ui| {
                            if ui.selectable_label(self.state.ascendancy_none, "None (base class)").clicked() {
                                self.state.selected_ascendancy = None;
                                self.state.ascendancy_none = true;
                            }
                            for a in db.ascendancies_of(c) {
                                if ui.selectable_label(self.state.selected_ascendancy == Some(a), &db.ascendancies[a].name).clicked() {
                                    self.state.selected_ascendancy = Some(a);
                                    self.state.ascendancy_none = false;
                                }
                            }
                        });
                    }
                    ui.checkbox(&mut self.state.dim_other_ascendancies, "Dim other ascendancies");
                    if let Some(a) = self.state.selected_ascendancy {
                        if ui.small_button("Go to ascendancy").clicked() {
                            let layout = self.state.layout_for(self.psg);
                            if let Some(p) = layout.plates.iter().find(|p| p.ascendancy == a) {
                                self.state.pan = -p.center.to_vec2() * self.state.zoom;
                            }
                        }
                    }
                }
            }

            ui.separator();
            let can_export = self.state.skill_db.is_some();
            if ui
                .add_enabled(can_export, egui::Button::new("Export tree…"))
                .on_hover_text("Writes data.json, sprite sheets and an HTML viewer in the layout of GGG's official passive tree export")
                .on_disabled_hover_text("Waiting for node data…")
                .clicked()
            {
                self.state.export_requested = true;
            }
            if let Some(status) = &self.state.export_status {
                ui.label(egui::RichText::new(status).size(11.0));
            }

            if self.is_loading_art {
                ui.add_space(8.0);
                ui.spinner();
                if self.art_pending > 0 {
                    ui.label(format!("Loading art… {} left", self.art_pending));
                } else {
                    ui.label("Loading node data…");
                }
            }
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.state.show_graph {
            if ui.button("Switch to Graph View").clicked() {
                self.state.show_graph = true;
            }
            ui.separator();
            ui.label("Raw Data (Visualization Disabled):");
            return;
        }

        self.state.ensure_selection();
        self.show_toolbar(ui);

        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());

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
                self.state.zoom = self.state.zoom.clamp(0.02, 5.0);
            }

            let cv = Canvas { rect: response.rect, zoom: self.state.zoom, pan: self.state.pan };
            let layout = self.state.layout_for(self.psg);
            let db = self.state.skill_db.clone();
            let orbit_radii = self.psg.orbit_radii();

            // "None" shows the base class alone: no plates, ascendancy nodes or links.
            let hide_ascendancies = self.state.ascendancy_none && self.psg.graph_type == 0;
            let asc_hidden = move |asc: Option<usize>| hide_ascendancies && asc.is_some();

            // ── Hover ─────────────────────────────────────────────
            let mut hovered = None;
            if response.hovered() {
                if let Some(cursor) = ui.input(|i| i.pointer.latest_pos()) {
                    let mut best = f32::MAX;
                    for (&id, &pos) in &layout.node_pos {
                        if layout.is_node_hidden(id) || asc_hidden(layout.node_ascendancy(id)) {
                            continue;
                        }
                        let dist = (cursor - cv.to_screen(pos)).length();
                        let info = db.as_ref().and_then(|d| d.nodes.get(&id));
                        if info.map(|i| i.is_mastery || !i.characters.is_empty()).unwrap_or(false) {
                            continue;
                        }
                        let world_r = info.map(|i| frame_size(i, layout.node_ascendancy(id).is_some()) / 2.0).unwrap_or(20.0);
                        let hit = (world_r * cv.zoom).max(12.0);
                        if dist < hit && dist < best {
                            best = dist;
                            hovered = Some(id);
                        }
                    }
                }
            }
            self.state.hovered_node = hovered;

            let selected_asc = self.state.selected_ascendancy;
            let dim_enabled = self.state.dim_other_ascendancies;
            let tint_for = |asc: Option<usize>| -> Color32 {
                match asc {
                    Some(a) if dim_enabled && selected_asc.is_some() && selected_asc != Some(a) => DIM,
                    _ => FULL,
                }
            };

            // ── Backdrops ─────────────────────────────────────────
            match self.psg.graph_type {
                // The atlas backdrops, main tree and subtrees alike, are drawn
                // together in `draw_atlas_subtrees` — they all need the
                // per-root flood fill to know which nodes they cover.
                1 => {}
                2 => {
                    if let Some(tex) = self.find_texture(BREACH_BACKDROP) {
                        if let Some(bbox) = bbox_of(layout.node_pos.values()) {
                            // 9960×8728 painted backdrop at one px per world unit.
                            draw_image(&painter, &cv, tex, bbox.center(), vec2(9960.0, 8728.0), FULL);
                        }
                    }
                }
                _ => {
                    // Illustration first: the ring carries the six class-start
                    // roundels and its ornaments have to sit on top of the art.
                    if let Some(db) = &db {
                        if let Some(ch) = self.state.selected_class.and_then(|c| db.characters.get(c)) {
                            if let Some(tex) = ch.illustration.as_deref().and_then(|p| self.find_texture(p)) {
                                draw_image(&painter, &cv, tex, Pos2::ZERO, Vec2::splat(CLASS_ILLUSTRATION_SIZE), FULL);
                            }
                        }
                    }
                }
            }

            // ── Ascendancy plates ─────────────────────────────────
            if let Some(db) = &db {
                if self.psg.graph_type == 0 && !hide_ascendancies {
                    for plate in &layout.plates {
                        let a = &db.ascendancies[plate.ascendancy];
                        if let Some(tex) = a.illustration.as_deref().and_then(|p| self.find_texture(p)) {
                            draw_image(&painter, &cv, tex, plate.center, Vec2::splat(ASCENDANCY_PLATE_SIZE), tint_for(Some(plate.ascendancy)));
                        }
                    }
                } else if self.psg.graph_type == 2 {
                    if let Some(tex) = self.find_texture(BREACH_START) {
                        for root in &self.psg.roots {
                            if let Some(&pos) = layout.node_pos.get(root) {
                                draw_image(&painter, &cv, tex, pos, Vec2::splat(104.0), FULL);
                            }
                        }
                    }
                }
            }

            // ── Atlas subtree backgrounds + decorators ────────────
            if let Some(db) = &db {
                if self.psg.graph_type == 1 {
                    self.draw_atlas_subtrees(&painter, &cv, db, &layout);
                    for d in &db.decorators {
                        let Some(&pos) = layout.node_pos.get(&d.node) else { continue };
                        let center = pos + vec2(d.x, d.y);
                        let angle = d.rotation_deg.to_radians();
                        for path in [&d.background, &d.blocked] {
                            if let Some(tex) = self.find_texture(path) {
                                let [w, h] = tex.size();
                                draw_image_rotated(&painter, &cv, tex, center, vec2(w as f32, h as f32) * d.scale * 2.0, angle, FULL);
                            }
                        }
                    }
                    if let Some(tex) = self.find_texture(ATLAS_START) {
                        let [w, h] = tex.size();
                        draw_image(&painter, &cv, tex, Pos2::ZERO, vec2(w as f32, h as f32), FULL);
                    }
                }
            }

            let tree_context = tree_context_for_graph_type(self.psg.graph_type);
            let art_set = db.as_ref().and_then(|d| d.art_sets.get(tree_context));
            let art_for_node = |id: u32| -> Option<&SkillTreeArtSet> {
                let db = db.as_deref()?;
                match layout.node_ascendancy(id) {
                    Some(a) => db.ui_art_for_ascendancy(a).or(art_set),
                    None => art_set,
                }
            };

            // ── Group backgrounds (only groups the game flags) ────
            if let Some(art) = art_set {
                for (gi, group) in self.psg.groups.iter().enumerate() {
                    if group.is_proxy || group.nodes.is_empty() || layout.group_hidden[gi] || asc_hidden(layout.group_ascendancy[gi]) {
                        continue;
                    }
                    if group.background_type == 0 && group.background_flag == 0 {
                        continue;
                    }
                    let set = layout.group_ascendancy[gi].and_then(|a| db.as_ref().and_then(|d| d.ui_art_for_ascendancy(a))).unwrap_or(art);
                    let (path, half) = match group.background_type {
                        2 => (&set.group_background.small, false),
                        4 => (&set.group_background.medium, false),
                        _ => (&set.group_background.large, set.group_background.large.to_ascii_lowercase().contains("half")),
                    };
                    let Some(tex) = self.find_texture(path) else { continue };
                    let [w, h] = tex.size();
                    let (w, h) = (w as f32, h as f32);
                    let origin = pos2(group.x, group.y) + layout.group_offset[gi];
                    let tint = tint_for(layout.group_ascendancy[gi]);
                    if half {
                        // Half images hold the top half; the bottom is the mirror.
                        let top = cv.world_rect(pos2(origin.x, origin.y - h / 2.0), vec2(w, h));
                        let bottom = cv.world_rect(pos2(origin.x, origin.y + h / 2.0), vec2(w, h));
                        if cv.visible(top.union(bottom)) {
                            painter.image(tex.id(), top, UV_FULL, tint);
                            painter.image(tex.id(), bottom, Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)), tint);
                        }
                    } else {
                        draw_image(&painter, &cv, tex, origin, vec2(w, h), tint);
                    }
                }
            }

            // ── Cluster patterns ──────────────────────────────────
            // The faded glyph behind a cluster sits under its lines and nodes.
            if let Some(db) = &db {
                for (gi, group) in self.psg.groups.iter().enumerate() {
                    if group.is_proxy || layout.group_hidden[gi] || asc_hidden(layout.group_ascendancy[gi]) {
                        continue;
                    }
                    for node in &group.nodes {
                        let Some(info) = db.nodes.get(&node.skill_id).filter(|i| i.is_mastery) else { continue };
                        let Some(&pos) = layout.node_pos.get(&node.skill_id) else { continue };
                        let pattern = info.mastery_group.and_then(|g| db.mastery_effect_images.get(&g));
                        if let Some(tex) = pattern.and_then(|p| self.find_texture(p)) {
                            let tint = tint_for(layout.group_ascendancy[gi]);
                            let faded = Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), MASTERY_ALPHA);
                            draw_image(&painter, &cv, tex, pos, Vec2::splat(MASTERY_PATTERN_SIZE), faded);
                        }
                    }
                }
            }

            // ── Connectors ────────────────────────────────────────
            let mut unique: HashMap<(u32, u32), i32> = HashMap::new();
            for (gi, group) in self.psg.groups.iter().enumerate() {
                if group.is_proxy || layout.group_hidden[gi] {
                    continue;
                }
                for node in &group.nodes {
                    for conn in &node.connections {
                        if layout.is_node_hidden(conn.node_id) {
                            continue;
                        }
                        let (a, b) = if node.skill_id < conn.node_id { (node.skill_id, conn.node_id) } else { (conn.node_id, node.skill_id) };
                        let entry = unique.entry((a, b)).or_insert(0);
                        if conn.orbit != 0 && conn.orbit != i32::MAX {
                            let sign = if node.skill_id < conn.node_id { 1 } else { -1 };
                            *entry = conn.orbit * sign;
                        }
                    }
                }
            }

            for ((a, b), orbit_idx) in unique {
                let (Some(&pa), Some(&pb)) = (layout.node_pos.get(&a), layout.node_pos.get(&b)) else { continue };
                // Cluster "mastery" markers have no connectors; class starts
                // connect from their roundel on the ring.
                let (info_a, info_b) = (db.as_ref().and_then(|d| d.nodes.get(&a)), db.as_ref().and_then(|d| d.nodes.get(&b)));
                if info_a.map(|i| i.is_mastery).unwrap_or(false) || info_b.map(|i| i.is_mastery).unwrap_or(false) {
                    continue;
                }
                let (start_a, start_b) = (info_a.map(|i| !i.characters.is_empty()).unwrap_or(false), info_b.map(|i| !i.characters.is_empty()).unwrap_or(false));
                let pa = if start_a { skill_tree_layout::class_start_line_end(pa, pb) } else { pa };
                let pb = if start_b { skill_tree_layout::class_start_line_end(pb, pa) } else { pb };
                let asc_a = layout.node_ascendancy(a);
                let asc_b = layout.node_ascendancy(b);
                // Ascendancy clusters are self-contained; a cross-link is a data artefact.
                if asc_a != asc_b || asc_hidden(asc_a) {
                    continue;
                }
                let margin = 1400.0 * cv.zoom;
                let sa = cv.to_screen(pa);
                let sb = cv.to_screen(pb);
                if !cv.rect.expand(margin).contains(sa) && !cv.rect.expand(margin).contains(sb) {
                    continue;
                }
                let active = self.state.hovered_node == Some(a) || self.state.hovered_node == Some(b);
                let tint = tint_for(asc_a);
                let sheet = art_for_node(a).map(|s| if active { &s.connection.active } else { &s.connection.normal }).and_then(|p| self.find_texture(p));

                let same_group = layout.node_group.get(&a) == layout.node_group.get(&b);
                let (ga, gb) = (layout.node_group.get(&a).copied(), layout.node_group.get(&b).copied());
                let (na, nb) = (self.node_in_group(ga, a), self.node_in_group(gb, b));

                let mut drawn = false;
                if let (Some(na), Some(nb)) = (na, nb) {
                    if same_group && na.radius == nb.radius && na.radius > 0 {
                        let gi = ga.unwrap();
                        let center = pos2(self.psg.groups[gi].x, self.psg.groups[gi].y) + layout.group_offset[gi];
                        let r = orbit_radii[na.radius as usize];
                        let a1 = skill_tree_layout::orbit_angle(na.radius, na.position, &self.psg.passives_per_orbit);
                        let a2 = skill_tree_layout::orbit_angle(nb.radius, nb.position, &self.psg.passives_per_orbit);
                        self.draw_arc_between(&painter, &cv, sheet, center, r, na.radius as usize, a1, a2, tint, active);
                        drawn = true;
                    } else if orbit_idx != 0 {
                        let orbit = orbit_idx.unsigned_abs() as usize;
                        if let Some(&r) = orbit_radii.get(orbit) {
                            let d = pb - pa;
                            let dist = d.length();
                            if r > 0.0 && dist < r * 2.0 && dist > 0.0 {
                                let perp = (r * r - dist * dist / 4.0).sqrt() * if orbit_idx > 0 { 1.0 } else { -1.0 };
                                let center = pos2(pa.x + d.x / 2.0 + perp * (d.y / dist), pa.y + d.y / 2.0 - perp * (d.x / dist));
                                let a1 = std::f32::consts::FRAC_PI_2 + (pa.y - center.y).atan2(pa.x - center.x);
                                let a2 = std::f32::consts::FRAC_PI_2 + (pb.y - center.y).atan2(pb.x - center.x);
                                self.draw_arc_between(&painter, &cv, sheet, center, r, orbit, a1, a2, tint, active);
                                drawn = true;
                            }
                        }
                    }
                }
                if !drawn {
                    self.draw_line(&painter, &cv, sheet, pa, pb, tint, active);
                }
            }

            // ── Centre ring ───────────────────────────────────────
            // Painted after the connectors so the class-start lines end
            // under the roundel mounts, as in game.
            if self.psg.graph_type == 0 {
                if let Some(tex) = self.find_texture(MAIN_CIRCLE) {
                    draw_image(&painter, &cv, tex, Pos2::ZERO, Vec2::splat(MAIN_CIRCLE_SIZE), FULL);
                }
                // Lights up the selected class's roundel, drawn over the ring.
                if let Some(db) = &db {
                    let start = self.state.selected_class.and_then(|c| {
                        self.psg.roots.iter().find(|r| db.nodes.get(r).map(|i| i.characters.contains(&c)).unwrap_or(false))
                    });
                    if let (Some(root), Some(tex)) = (start, self.find_texture(MAIN_CIRCLE_ACTIVE)) {
                        if let Some(&p) = layout.node_pos.get(root) {
                            let outward = p.to_vec2() / p.to_vec2().length().max(1.0);
                            let angle = p.x.atan2(-p.y);
                            let center = skill_tree_layout::class_start_anchor(p) + outward * ACTIVE_MARKER_OFFSET;
                            draw_image_rotated(&painter, &cv, tex, center, Vec2::splat(ACTIVE_MARKER_SIZE), angle, FULL);
                        }
                    }
                }
            }

            // ── Nodes ─────────────────────────────────────────────
            for (gi, group) in self.psg.groups.iter().enumerate() {
                if group.is_proxy || layout.group_hidden[gi] || asc_hidden(layout.group_ascendancy[gi]) {
                    continue;
                }
                for node in &group.nodes {
                    let Some(&pos) = layout.node_pos.get(&node.skill_id) else { continue };
                    let screen_pos = cv.to_screen(pos);
                    if !cv.rect.expand(80.0).contains(screen_pos) {
                        continue;
                    }
                    let is_hovered = self.state.hovered_node == Some(node.skill_id);
                    let info = db.as_ref().and_then(|d| d.nodes.get(&node.skill_id));
                    let tint = tint_for(layout.group_ascendancy[gi]);

                    let Some(info) = info else {
                        painter.circle(screen_pos, 4.5 * cv.zoom, Color32::from_rgb(100, 150, 250), egui::Stroke::NONE);
                        continue;
                    };
                    if !info.characters.is_empty() {
                        // Class start: the plate is the node.
                        continue;
                    }
                    if info.is_mastery {
                        continue; // drawn as a cluster pattern before the connectors
                    }
                    // Every node in a subtree carries its subtree's emblem, but
                    // only the entry node wears it — the rest draw their own icon.
                    let is_subtree_entry = self.psg.roots.contains(&node.skill_id);
                    if self.psg.graph_type == 1 && is_subtree_entry && info.atlas_subtree_icon.is_some() {
                        if let Some(tex) = info.atlas_subtree_icon.as_deref().and_then(|p| self.find_texture(p)) {
                            let [w, h] = tex.size();
                            draw_image(&painter, &cv, tex, pos, vec2(w as f32, h as f32) * 2.0, tint);
                            continue;
                        }
                    }

                    let in_asc = layout.group_ascendancy[gi].is_some();
                    let art = art_for_node(node.skill_id);
                    let frame: Option<(String, String)> = if info.is_attribute {
                        Some((PLUS_FRAME_NORMAL.to_string(), PLUS_FRAME_ACTIVE.to_string()))
                    } else {
                        let f: Option<&FrameArt> = info
                            .node_frame_art
                            .and_then(|i| db.as_ref().and_then(|d| d.node_frames.get(i)))
                            .or_else(|| art.and_then(|a| a.frames.get(&node_frame_kind(info))));
                        f.map(|f| (f.normal.clone(), f.active.clone()))
                    };
                    let frame_tex = frame.as_ref().and_then(|(n, a)| self.find_texture(if is_hovered { a } else { n }));
                    // Frames are drawn at their pixel size; the icon fills the frame interior.
                    let frame_size = frame_tex.map(|t| t.size()[0] as f32).unwrap_or_else(|| frame_size(info, in_asc));
                    let icon_size = if info.is_ascendancy_start || info.is_jewel_socket { 0.0 } else { frame_size * ICON_TO_FRAME };
                    let icon_tex = if icon_size > 0.0 { info.icon.as_deref().and_then(|p| self.find_texture(p)) } else { None };

                    if icon_tex.is_none() && frame_tex.is_none() {
                        let (r, color) = if info.is_keystone {
                            (10.0, Color32::from_rgb(255, 90, 120))
                        } else if info.is_notable {
                            (7.5, Color32::from_rgb(255, 200, 50))
                        } else if info.is_jewel_socket {
                            (7.0, Color32::from_rgb(0, 220, 180))
                        } else {
                            (4.5, Color32::from_rgb(100, 150, 250))
                        };
                        painter.circle(screen_pos, r * cv.zoom, color, egui::Stroke::new(1.0_f32, Color32::from_gray(20)));
                        continue;
                    }
                    if let Some(tex) = icon_tex {
                        draw_image(&painter, &cv, tex, pos, Vec2::splat(icon_size), tint);
                    }
                    if let Some(tex) = frame_tex {
                        draw_image(&painter, &cv, tex, pos, Vec2::splat(frame_size), tint);
                    }
                }
            }

            if let Some(hovered_id) = self.state.hovered_node {
                self.show_tooltip(ui, hovered_id, db.as_deref());
            }
        });
    }

    fn node_in_group(&self, gi: Option<usize>, id: u32) -> Option<&crate::dat::psg::PsgNode> {
        self.psg.groups.get(gi?)?.nodes.iter().find(|n| n.skill_id == id)
    }

    /// Arc on orbit `orbit` (radius `r`) from angle `a1` to `a2` (clockwise
    /// from north). The sheet holds all nine orbit rings concentric on its
    /// bottom-right corner, so a single quad would show every smaller ring
    /// too; instead the arc is tessellated as a thin band whose UVs follow the
    /// ring at radius `r` inside the sheet. The sheet only holds a quarter
    /// ring, so the band restarts at every 90° boundary — a triangle spanning
    /// one would interpolate its UVs straight across the sheet.
    #[allow(clippy::too_many_arguments)]
    fn draw_arc_between(&self, painter: &egui::Painter, cv: &Canvas, sheet: Option<&egui::TextureHandle>, center: Pos2, r: f32, orbit: usize, a1: f32, a2: f32, tint: Color32, active: bool) {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let (mut lo, mut hi) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
        let mut arc = hi - lo;
        if arc >= PI {
            std::mem::swap(&mut lo, &mut hi);
            arc = TAU - arc;
        }
        let _ = orbit;
        let Some(sheet) = sheet else {
            self.draw_arc_fallback(painter, cv, center, r, lo, arc, tint, active);
            return;
        };
        let bounds = Rect::from_center_size(cv.to_screen(center), Vec2::splat((r + ARC_BAND) * 2.0 * cv.zoom));
        if !cv.visible(bounds) {
            return;
        }
        let mut mesh = egui::Mesh::with_texture(sheet.id());
        let end = lo + arc;
        let mut start = lo;
        while end - start > 1e-4 {
            let quadrant = ((start + 1e-4) / FRAC_PI_2).floor();
            let stop = end.min((quadrant + 1.0) * FRAC_PI_2);
            let span = stop - start;
            let steps = ((span / 4.0_f32.to_radians()).ceil() as usize).clamp(1, 96);
            let base = mesh.vertices.len() as u32;
            for i in 0..=steps {
                let t = start + span * i as f32 / steps as f32;
                let dir = vec2(t.sin(), -t.cos());
                // Position within this quarter of the ring, mapped onto the sheet's quarter-arc.
                let q = (t - quadrant * FRAC_PI_2).clamp(0.0, FRAC_PI_2);
                for rho in [r - ARC_BAND, r + ARC_BAND] {
                    let uv = pos2(1.0 - rho * q.cos() / SHEET_SIZE, 1.0 - rho * q.sin() / SHEET_SIZE);
                    mesh.vertices.push(egui::epaint::Vertex { pos: cv.to_screen(center + dir * rho), uv, color: tint });
                }
                if i > 0 {
                    let b = base + (i * 2) as u32;
                    mesh.add_triangle(b - 2, b - 1, b);
                    mesh.add_triangle(b - 1, b + 1, b);
                }
            }
            start = stop;
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn draw_arc_fallback(&self, painter: &egui::Painter, cv: &Canvas, center: Pos2, r: f32, start: f32, arc: f32, tint: Color32, active: bool) {
        let steps = 16;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let th = start + arc * i as f32 / steps as f32;
                cv.to_screen(pos2(center.x + r * th.sin(), center.y - r * th.cos()))
            })
            .collect();
        painter.add(egui::Shape::line(points, fallback_stroke(cv, tint, active)));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(&self, painter: &egui::Painter, cv: &Canvas, sheet: Option<&egui::TextureHandle>, a: Pos2, b: Pos2, tint: Color32, active: bool) {
        let Some(sheet) = sheet else {
            painter.line_segment([cv.to_screen(a), cv.to_screen(b)], fallback_stroke(cv, tint, active));
            return;
        };
        let d = b - a;
        let len = d.length();
        if len <= 0.0 {
            return;
        }
        let n = vec2(-d.y, d.x) / len * (LINE_WORLD_HEIGHT / 2.0);
        let (v0, v1) = (SHEET_LINE_ROWS.0 / SHEET_SIZE, SHEET_LINE_ROWS.1 / SHEET_SIZE);
        let u_end = len / SHEET_SIZE;
        let quad = [(a - n, pos2(0.0, v0)), (b - n, pos2(u_end, v0)), (b + n, pos2(u_end, v1)), (a + n, pos2(0.0, v1))];
        let bounds = Rect::from_points(&[cv.to_screen(a), cv.to_screen(b)]).expand(LINE_WORLD_HEIGHT * cv.zoom);
        if !cv.visible(bounds) {
            return;
        }
        let mut mesh = egui::Mesh::with_texture(sheet.id());
        for (pt, uv) in quad {
            mesh.vertices.push(egui::epaint::Vertex { pos: cv.to_screen(pt), uv, color: tint });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Atlas subtree themed backgrounds (Abyss/Breach/…): membership is a
    /// flood-fill from each root through the psg's own connections, the art
    /// comes from `PassiveSkills.AtlasSubTree` on the root node.
    fn draw_atlas_subtrees(&self, painter: &egui::Painter, cv: &Canvas, db: &SkillGraphDatabase, layout: &TreeLayout) {
        if self.psg.roots.is_empty() {
            return;
        }
        let root_of = self.psg.root_membership();
        let mut bbox: HashMap<u32, Rect> = HashMap::new();
        for (&node, &root) in &root_of {
            if let Some(&p) = layout.node_pos.get(&node) {
                bbox.entry(root).and_modify(|r| *r = r.union(Rect::from_min_max(p, p))).or_insert(Rect::from_min_max(p, p));
            }
        }
        // The main tree's root is the one with no subtree art of its own. Its
        // backdrop covers only the nodes that hang off it — sizing it to the
        // whole graph stretches it over the league subtrees sitting outside.
        for &root in &self.psg.roots {
            let Some(info) = db.nodes.get(&root) else { continue };
            if info.atlas_subtree_background.is_some() {
                continue;
            }
            let Some(b) = bbox.get(&root) else { continue };
            let Some(tex) = self.find_texture(crate::ui::atlas_node_db::ATLAS_MAIN_TREE_BG_PATH) else { continue };
            let side = b.width().max(b.height()) * ATLAS_MAIN_TREE_BG_SCALE;
            draw_image(painter, cv, tex, b.center(), Vec2::splat(side), FULL);
        }

        for &root in &self.psg.roots {
            let Some(info) = db.nodes.get(&root) else { continue };
            let Some((bg, ix, iy)) = &info.atlas_subtree_background else { continue };
            let (Some(&pos), Some(b)) = (layout.node_pos.get(&root), bbox.get(&root)) else { continue };
            let Some(tex) = self.find_texture(bg) else { continue };
            let diameter = (b.width().max(b.height()) * ATLAS_SUBTREE_BG_SCALE).max(ATLAS_SUBTREE_BG_MIN);
            draw_image(painter, cv, tex, pos + vec2(*ix, *iy), Vec2::splat(diameter), FULL);
        }
    }

    fn show_tooltip(&self, ui: &egui::Ui, hovered_id: u32, db: Option<&SkillGraphDatabase>) {
        egui::show_tooltip(ui.ctx(), ui.layer_id(), egui::Id::new(hovered_id), |ui| {
            let Some(info) = db.and_then(|d| d.nodes.get(&hovered_id)) else {
                ui.label(format!("Skill ID: {}", hovered_id));
                return;
            };
            let dark = ui.visuals().dark_mode;
            let name_color = if info.is_keystone {
                Color32::from_rgb(255, 90, 120)
            } else if info.is_notable {
                Color32::from_rgb(255, 200, 50)
            } else if info.is_jewel_socket {
                Color32::from_rgb(0, 220, 180)
            } else if dark {
                Color32::WHITE
            } else {
                Color32::from_rgb(24, 24, 28)
            };
            let display_name = if info.name.is_empty() { format!("Skill ID: {}", hovered_id) } else { info.name.clone() };
            ui.label(egui::RichText::new(&display_name).color(name_color).strong().size(15.0));
            let mut type_label = if info.is_keystone {
                "Keystone Passive Skill"
            } else if info.is_notable {
                "Notable Passive Skill"
            } else if info.is_jewel_socket {
                "Jewel Socket"
            } else if info.is_attribute {
                "Attribute"
            } else {
                "Passive Skill"
            }
            .to_string();
            if let Some(a) = info.ascendancy.and_then(|a| db.and_then(|d| d.ascendancies.get(a))) {
                type_label = format!("{} · {}", a.name, type_label);
            }
            let type_color = if dark { Color32::from_rgb(150, 150, 150) } else { Color32::from_rgb(100, 100, 110) };
            ui.label(egui::RichText::new(type_label).color(type_color).size(11.0).italics());
            if !info.stat_lines.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                let stat_color = if dark { Color32::from_rgb(180, 210, 255) } else { Color32::from_rgb(37, 99, 235) };
                for stat in &info.stat_lines {
                    ui.label(egui::RichText::new(stat).color(stat_color).size(12.0));
                }
            }
            if let Some(flavour) = &info.flavour_text {
                if !flavour.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(flavour).italics().size(11.0).color(Color32::GRAY));
                }
            }
        });
    }
}

fn fallback_stroke(cv: &Canvas, tint: Color32, active: bool) -> egui::Stroke {
    let base = if active { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(160, 115, 60) };
    let color = if tint == DIM { Color32::from_rgb(base.r() / 2, base.g() / 2, base.b() / 2) } else { base };
    egui::Stroke::new((if active { 2.5 } else { 1.0 }) * cv.zoom.max(0.5), color)
}

fn bbox_of<'a>(points: impl Iterator<Item = &'a Pos2>) -> Option<Rect> {
    let mut rect: Option<Rect> = None;
    for &p in points {
        rect = Some(match rect {
            Some(r) => r.union(Rect::from_min_max(p, p)),
            None => Rect::from_min_max(p, p),
        });
    }
    rect
}
