#![allow(dead_code)]
use eframe::egui;
use crate::dat::psg::PsgFile;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

// Embedded Skill Tree Assets
const SKILLS_WEBP: &[u8] = include_bytes!("../../assets/skilltree/skills.webp");
const SKILLS_DISABLED_WEBP: &[u8] = include_bytes!("../../assets/skilltree/skills-disabled.webp");
const FRAME_WEBP: &[u8] = include_bytes!("../../assets/skilltree/frame.webp");

const SKILLS_COMPACT_JSON: &str = include_str!("../../assets/skilltree/skills_compact.json");
const NODES_COMPACT_JSON: &str = include_str!("../../assets/skilltree/nodes_compact.json");
const FRAME_JSON: &str = include_str!("../../assets/skilltree/frame.json");

// Embedded Class Background Assets
const BG_DRUID_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-druid.webp");
const BG_HUNTRESS_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-huntress.webp");
const BG_MERCENARY_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-mercenary.webp");
const BG_MONK_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-monk.webp");
const BG_RANGER_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-ranger.webp");
const BG_SORCERESS_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-sorceress.webp");
const BG_WARRIOR_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-warrior.webp");
const BG_WITCH_WEBP: &[u8] = include_bytes!("../../assets/skilltree/background-witch.webp");

const BG_DRUID_JSON: &str = include_str!("../../assets/skilltree/background-druid.json");
const BG_HUNTRESS_JSON: &str = include_str!("../../assets/skilltree/background-huntress.json");
const BG_MERCENARY_JSON: &str = include_str!("../../assets/skilltree/background-mercenary.json");
const BG_MONK_JSON: &str = include_str!("../../assets/skilltree/background-monk.json");
const BG_RANGER_JSON: &str = include_str!("../../assets/skilltree/background-ranger.json");
const BG_SORCERESS_JSON: &str = include_str!("../../assets/skilltree/background-sorceress.json");
const BG_WARRIOR_JSON: &str = include_str!("../../assets/skilltree/background-warrior.json");
const BG_WITCH_JSON: &str = include_str!("../../assets/skilltree/background-witch.json");

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub(crate) struct CompactNode {
    pub(crate) n: String,           // name
    pub(crate) i: String,           // icon path
    pub(crate) t: String,           // type: "normal", "notable", "keystone", "jewel", "mastery"
    pub(crate) s: Vec<String>,      // stats description
    #[serde(default)]
    pub(crate) a: Option<String>,   // ascendancyId
    #[serde(rename = "as")]
    #[serde(default)]
    pub(crate) as_start: bool,      // isAscendancyStart
}

#[derive(Deserialize, Debug, Clone)]
struct FrameMeta {
    frames: HashMap<String, FrameInfo>,
}

#[derive(Deserialize, Debug, Clone)]
struct FrameInfo {
    frame: FrameRect,
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub(crate) struct FrameRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

pub struct PsgDatabase {
    pub nodes: HashMap<u32, CompactNode>,
    pub skills: HashMap<String, [u32; 4]>,
    pub frames: HashMap<String, FrameRect>,
}

pub struct PsgTextures {
    pub skills: egui::TextureHandle,
    pub skills_disabled: egui::TextureHandle,
    pub frame: egui::TextureHandle,
}

struct PsgClassBg {
    webp: &'static [u8],
    json: &'static str,
}

fn get_class_bg_assets(class_name: &str) -> Option<PsgClassBg> {
    match class_name {
        "Druid" => Some(PsgClassBg { webp: BG_DRUID_WEBP, json: BG_DRUID_JSON }),
        "Huntress" => Some(PsgClassBg { webp: BG_HUNTRESS_WEBP, json: BG_HUNTRESS_JSON }),
        "Mercenary" => Some(PsgClassBg { webp: BG_MERCENARY_WEBP, json: BG_MERCENARY_JSON }),
        "Monk" => Some(PsgClassBg { webp: BG_MONK_WEBP, json: BG_MONK_JSON }),
        "Ranger" => Some(PsgClassBg { webp: BG_RANGER_WEBP, json: BG_RANGER_JSON }),
        "Sorceress" => Some(PsgClassBg { webp: BG_SORCERESS_WEBP, json: BG_SORCERESS_JSON }),
        "Warrior" => Some(PsgClassBg { webp: BG_WARRIOR_WEBP, json: BG_WARRIOR_JSON }),
        "Witch" => Some(PsgClassBg { webp: BG_WITCH_WEBP, json: BG_WITCH_JSON }),
        _ => None,
    }
}


// Angle (radians) of a node on its orbit, measured clockwise from north.
// PoE2 orbits are evenly spaced: theta = position / capacity * 2*pi.
fn get_node_angle(radius: u32, position: u32, passives_per_orbit: &[u8]) -> f32 {
    let r_idx = radius as usize;
    let capacity = if r_idx < passives_per_orbit.len() {
        passives_per_orbit[r_idx] as f32
    } else {
        12.0
    };

    if capacity <= 0.0 {
        return 0.0;
    }
    (position as f32 / capacity) * std::f32::consts::TAU
}

pub struct PsgViewerState {
    pub pan: egui::Vec2,
    pub zoom: f32,
    // Toggle for JSON view vs Graph view
    pub show_graph: bool,
    pub hovered_node: Option<u32>,
    pub textures: Option<Arc<PsgTextures>>,
    pub db: Option<Arc<PsgDatabase>>,
    pub selected_class: String,
    pub selected_ascendancy: usize,
    pub active_bg_textures: HashMap<String, (egui::TextureHandle, HashMap<String, FrameRect>)>,
    pub autoloaded: bool,
}

impl Default for PsgViewerState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::new(0.0, 0.0),
            zoom: 0.2, // Start zoomed out
            show_graph: true,
            hovered_node: None,
            textures: None,
            db: None,
            selected_class: "Witch".to_string(),
            selected_ascendancy: 0,
            active_bg_textures: HashMap::new(),
            autoloaded: false,
        }
    }
}

impl PsgViewerState {
    pub fn ensure_initialized(&mut self, ctx: &egui::Context) {
        if self.db.is_none() {
            let nodes: HashMap<u32, CompactNode> = serde_json::from_str(NODES_COMPACT_JSON)
                .unwrap_or_else(|e| {
                    log::error!("Failed to parse nodes_compact.json: {:?}", e);
                    HashMap::new()
                });
            let skills: HashMap<String, [u32; 4]> = serde_json::from_str(SKILLS_COMPACT_JSON)
                .unwrap_or_else(|e| {
                    log::error!("Failed to parse skills_compact.json: {:?}", e);
                    HashMap::new()
                });
            let frame_meta: FrameMeta = serde_json::from_str(FRAME_JSON)
                .unwrap_or_else(|e| {
                    log::error!("Failed to parse frame.json: {:?}", e);
                    FrameMeta { frames: HashMap::new() }
                });
            let frames = frame_meta.frames.into_iter().map(|(k, v)| (k, v.frame)).collect();

            self.db = Some(Arc::new(PsgDatabase { nodes, skills, frames }));
        }

        if self.textures.is_none() {
            let load_texture = |name: &str, bytes: &[u8]| -> Option<egui::TextureHandle> {
                match image::load_from_memory(bytes) {
                    Ok(img) => {
                        let size = [img.width() as usize, img.height() as usize];
                        let image_buffer = img.to_rgba8();
                        let pixels = image_buffer.as_flat_samples();
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                        Some(ctx.load_texture(name, color_image, Default::default()))
                    }
                    Err(e) => {
                        log::error!("Failed to load texture {}: {:?}", name, e);
                        None
                    }
                }
            };

            if let (Some(skills), Some(skills_disabled), Some(frame)) = (
                load_texture("skills", SKILLS_WEBP),
                load_texture("skills_disabled", SKILLS_DISABLED_WEBP),
                load_texture("frame", FRAME_WEBP),
            ) {
                self.textures = Some(Arc::new(PsgTextures {
                    skills,
                    skills_disabled,
                    frame,
                }));
            }
        }
    }

    pub fn ensure_background_loaded(&mut self, ctx: &egui::Context, class_name: &str) {
        if self.active_bg_textures.contains_key(class_name) {
            return;
        }

        if let Some(assets) = get_class_bg_assets(class_name) {
            match image::load_from_memory(assets.webp) {
                Ok(img) => {
                    let size = [img.width() as usize, img.height() as usize];
                    let image_buffer = img.to_rgba8();
                    let pixels = image_buffer.as_flat_samples();
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
                    let tex_name = format!("bg-{}", class_name.to_lowercase());
                    let texture = ctx.load_texture(tex_name, color_image, Default::default());
                    
                    if let Ok(meta) = serde_json::from_str::<FrameMeta>(assets.json) {
                        let frames = meta.frames.into_iter().map(|(k, v)| (k, v.frame)).collect();
                        self.active_bg_textures.insert(class_name.to_string(), (texture, frames));
                    }
                }
                Err(e) => {
                    log::error!("Failed to load background texture for {}: {:?}", class_name, e);
                }
            }
        }
    }
}

pub struct PsgViewer<'a> {
    pub state: &'a mut PsgViewerState,
    pub psg: &'a PsgFile,
}

impl<'a> PsgViewer<'a> {
    pub fn new(state: &'a mut PsgViewerState, psg: &'a PsgFile) -> Self {
        Self { state, psg }
    }

    fn detect_class_and_ascendancy(psg: &PsgFile, db: &PsgDatabase) -> Option<(String, usize)> {
        let mut counts = std::collections::HashMap::new();
        for group in &psg.groups {
            if group.is_proxy {
                continue;
            }
            for node in &group.nodes {
                if let Some(compact) = db.nodes.get(&node.skill_id) {
                    if let Some(ref asc_id) = compact.a {
                        if !asc_id.is_empty() {
                            *counts.entry(asc_id.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        let mut best_asc = None;
        let mut max_count = 0;
        for (asc_id, count) in counts {
            if count > max_count {
                max_count = count;
                best_asc = Some(asc_id);
            }
        }

        if let Some(asc_id) = best_asc {
            let len = asc_id.len();
            if len > 1 {
                let (class_part, num_part) = asc_id.split_at(len - 1);
                if let Ok(num) = num_part.parse::<usize>() {
                    let class_name = match class_part {
                        "Witch" => "Witch",
                        "Sorceress" => "Sorceress",
                        "Druid" => "Druid",
                        "Monk" => "Monk",
                        "Ranger" => "Ranger",
                        "Huntress" => "Huntress",
                        "Warrior" => "Warrior",
                        "Mercenary" => "Mercenary",
                        _ => class_part,
                    };
                    return Some((class_name.to_string(), num));
                }
            }
        }

        None
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.state.ensure_initialized(ui.ctx());

        if !self.state.autoloaded {
            if let Some(db) = &self.state.db {
                if let Some((detected_class, detected_asc)) = Self::detect_class_and_ascendancy(self.psg, db) {
                    self.state.selected_class = detected_class;
                    self.state.selected_ascendancy = detected_asc;
                }
                self.state.autoloaded = true;
            }
        }

        if !self.state.show_graph {
            if ui.button("Switch to Graph View").clicked() {
                self.state.show_graph = true;
            }
            ui.separator();
            ui.label("Raw Data (Visualization Disabled):");
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Switch to JSON View").clicked() {
                self.state.show_graph = false;
            }
            if ui.button("Reset View").clicked() {
                self.state.pan = egui::Vec2::ZERO;
                self.state.zoom = 0.2;
            }
            ui.label(format!("Zoom: {:.2}", self.state.zoom));
            
            // Zoom controls
            if ui.button("-").clicked() { self.state.zoom *= 0.8; }
            if ui.button("+").clicked() { self.state.zoom *= 1.25; }

            ui.separator();

            // Class Picker
            ui.label("Class:");
            let old_class = self.state.selected_class.clone();
            egui::ComboBox::from_id_salt("class_picker")
                .selected_text(&self.state.selected_class)
                .show_ui(ui, |ui| {
                    for c in &["Druid", "Huntress", "Mercenary", "Monk", "Ranger", "Sorceress", "Warrior", "Witch"] {
                        ui.selectable_value(&mut self.state.selected_class, c.to_string(), *c);
                    }
                });
                
            if old_class != self.state.selected_class {
                self.state.selected_ascendancy = 0; // Reset ascendancy when class changes
            }
            
            // Ascendancy Picker (dynamic based on selected class)
            ui.label("Ascendancy:");
            let ascendancies = match self.state.selected_class.as_str() {
                "Witch" => vec!["None / Witch", "Infernalist", "Blood Mage", "Lich"],
                "Sorceress" => vec!["None / Sorceress", "Stormweaver", "Chronomancer", "Disciple of Varashta"],
                "Druid" => vec!["None / Druid", "Oracle", "Shaman"],
                "Monk" => vec!["None / Monk", "Martial Artist", "Invoker", "Acolyte of Chayula"],
                "Ranger" => vec!["None / Ranger", "Deadeye", "Pathfinder"],
                "Huntress" => vec!["None / Huntress", "Amazon", "Spirit Walker", "Ritualist"],
                "Warrior" => vec!["None / Warrior", "Titan", "Warbringer", "Smith of Kitava"],
                "Mercenary" => vec!["None / Mercenary", "Tactician", "Witchhunter", "Gemling Legionnaire"],
                _ => vec!["None"],
            };
            
            egui::ComboBox::from_id_salt("ascendancy_picker")
                .selected_text(ascendancies.get(self.state.selected_ascendancy).copied().unwrap_or("None"))
                .show_ui(ui, |ui| {
                    for (i, name) in ascendancies.iter().enumerate() {
                        ui.selectable_value(&mut self.state.selected_ascendancy, i, *name);
                    }
                });
        });

        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (response, painter) = ui.allocate_painter(
                ui.available_size(),
                egui::Sense::drag(),
            );
            
            // Handle input
            if response.dragged() {
                self.state.pan += response.drag_delta();
            }
            
            // Handle Zoom
            if response.hovered() {
                // 1. Pinch or Ctrl+Scroll
                let zoom_delta = ui.input(|i| i.zoom_delta());
                self.state.zoom *= zoom_delta;
                
                // 2. Mouse Wheel (No Modifier)
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
                
                // Clamp Zoom
                self.state.zoom = self.state.zoom.clamp(0.05, 5.0);
            }

            // Transform helper
            let zoom = self.state.zoom;
            let pan = self.state.pan;
            let center = response.rect.center().to_vec2();
            let to_screen = move |pos: egui::Pos2| -> egui::Pos2 {
                (pos.to_vec2() * zoom + pan).to_pos2() + center
            };

            // Orbit radii depend on graph type (passive tree vs atlas).
            let orbit_radii = self.psg.orbit_radii();

            // Calculate Node Positions
            let mut node_positions: std::collections::HashMap<u32, egui::Pos2> = std::collections::HashMap::new();
            struct PsgNodeInfo {
                pos: egui::Pos2,
                group_x: f32,
                group_y: f32,
                poe_arc: f32,
                radius: f32,
            }
            let mut node_info: std::collections::HashMap<u32, PsgNodeInfo> = std::collections::HashMap::new();
            
            for group in &self.psg.groups {
                if group.is_proxy {
                    continue;
                }
                for node in &group.nodes {
                    let r_idx = node.radius as usize;
                    let radius = if r_idx < orbit_radii.len() { orbit_radii[r_idx] } else { node.radius as f32 * 50.0 };

                    // Canonical PoE orbit placement: theta measured clockwise from north.
                    //   x = group.x + r * sin(theta)
                    //   y = group.y - r * cos(theta)
                    // (matches the reference skilltree export exactly; egui y is already down).
                    let theta = get_node_angle(node.radius, node.position, &self.psg.passives_per_orbit);

                    let pos = egui::Pos2::new(
                        group.x + theta.sin() * radius,
                        group.y - theta.cos() * radius,
                    );

                    node_positions.insert(node.skill_id, pos);
                    node_info.insert(node.skill_id, PsgNodeInfo {
                        pos,
                        group_x: group.x,
                        group_y: group.y,
                        poe_arc: theta,
                        radius,
                    });
                }
            }

            // Determine Hovered Node
            let db_opt = self.state.db.clone();
            let mut new_hovered_node = None;

            if response.hovered() {
                if let Some(cursor_pos) = ui.input(|i| i.pointer.latest_pos()) {
                    let mut closest_dist = f32::MAX;
                    for (&skill_id, &pos) in &node_positions {
                        let screen_pos = to_screen(pos);
                        let dist = (cursor_pos - screen_pos).length();
                        
                        let mut hit_radius = 16.0 * self.state.zoom;
                        if let Some(db) = &db_opt {
                            if let Some(compact) = db.nodes.get(&skill_id) {
                                hit_radius = match compact.t.as_str() {
                                    "keystone" => 45.0 * self.state.zoom,
                                    "notable" => 30.0 * self.state.zoom,
                                    "jewel" => 35.0 * self.state.zoom,
                                    _ => 20.0 * self.state.zoom,
                                };
                            }
                        }
                        hit_radius = hit_radius.max(12.0); // Minimum hit size
                        
                        if dist < hit_radius && dist < closest_dist {
                            closest_dist = dist;
                            new_hovered_node = Some(skill_id);
                        }
                    }
                }
            }
            self.state.hovered_node = new_hovered_node;

            // Class backgrounds disabled (as requested, they don't align properly)
            
            // Draw Edges (Connections)
            let mut unique_connections: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();

            for group in &self.psg.groups {
                if group.is_proxy {
                    continue;
                }
                for node in &group.nodes {
                    for conn in &node.connections {
                        let (a, b) = if node.skill_id < conn.node_id {
                            (node.skill_id, conn.node_id)
                        } else {
                            (conn.node_id, node.skill_id)
                        };
                        
                        let entry = unique_connections.entry((a, b)).or_insert(0);
                        if conn.orbit != 0 && conn.orbit != 2147483647 {
                            let sign_multiplier = if node.skill_id < conn.node_id { 1 } else { -1 };
                            *entry = conn.orbit * sign_multiplier;
                        }
                    }
                }
            }

             for ((start_id, end_id), orbit_idx) in unique_connections {
                  if let (Some(start_node), Some(end_node)) = (node_info.get(&start_id), node_info.get(&end_id)) {
                      // Skip connections crossing the ascendancy boundary to keep layout clean
                      if let Some(db) = &db_opt {
                          let start_asc = db.nodes.get(&start_id).and_then(|n| n.a.as_deref()).unwrap_or("");
                          let end_asc = db.nodes.get(&end_id).and_then(|n| n.a.as_deref()).unwrap_or("");
                          if start_asc != end_asc {
                              continue;
                          }
                      }
                     let start_screen = to_screen(start_node.pos);
                     let end_screen = to_screen(end_node.pos);

                     // Check visibility (culling)
                     let margin = 500.0 * self.state.zoom;
                     if !response.rect.expand(margin).contains(start_screen) && !response.rect.expand(margin).contains(end_screen) {
                          continue;
                     }

                     let is_active = self.state.hovered_node == Some(start_id) || self.state.hovered_node == Some(end_id);
                     let stroke = if is_active {
                         egui::Stroke::new(2.5 * self.state.zoom, egui::Color32::from_rgb(0, 220, 255))
                     } else {
                         egui::Stroke::new(1.0 * self.state.zoom, egui::Color32::from_rgb(160, 115, 60))
                     };

                     if orbit_idx != 0 {
                          // Draw Arc
                          // 1. Same group, same radius -> draw using the group center
                          if (start_node.group_x - end_node.group_x).abs() < 0.1 
                             && (start_node.group_y - end_node.group_y).abs() < 0.1 
                             && (start_node.radius - end_node.radius).abs() < 0.1 
                          {
                              let group_x = start_node.group_x;
                              let group_y = start_node.group_y;
                              let radius = start_node.radius;

                              let arc1 = start_node.poe_arc;
                              let arc2 = end_node.poe_arc;

                              let lo = if arc1 < arc2 { arc1 } else { arc2 };
                              let hi = if arc1 < arc2 { arc2 } else { arc1 };

                              let (start_arc, end_arc) = if hi - lo >= std::f32::consts::PI {
                                  // Sweep the short way around the orbit instead of the long way.
                                  (hi, lo + std::f32::consts::TAU)
                              } else {
                                  (lo, hi)
                              };

                              let mut points = Vec::new();
                              let steps = 15;
                              for i in 0..=steps {
                                  let t = i as f32 / steps as f32;
                                  let th = start_arc + (end_arc - start_arc) * t;
                                  let px = group_x + radius * th.sin();
                                  let py = group_y - radius * th.cos();
                                  points.push(to_screen(egui::Pos2::new(px, py)));
                              }

                              painter.add(egui::Shape::Path(egui::epaint::PathShape::line(points, stroke)));
                              continue;
                          } else {
                              // 2. Fallback: Perpendicular sphere intersection
                              let start_pos = start_node.pos;
                              let end_pos = end_node.pos;
                              let orbit_abs = orbit_idx.abs() as usize;
                              let radius = if orbit_abs < orbit_radii.len() { orbit_radii[orbit_abs] } else { 0.0 };
                              
                              if radius > 0.0 {
                                 let dx = end_pos.x - start_pos.x;
                                 let dy = end_pos.y - start_pos.y;
                                 let dist = (dx*dx + dy*dy).sqrt();
                                 
                                 if dist < radius * 2.0 {
                                     let h = (radius * radius - (dist * dist) / 4.0).sqrt();
                                     let sign = if orbit_idx > 0 { 1.0 } else { -1.0 };
                                     
                                     let mid_x = start_pos.x + dx / 2.0;
                                     let mid_y = start_pos.y + dy / 2.0;
                                     
                                     let perp_x = (dy / dist) * h * sign;
                                     let perp_y = (-dx / dist) * h * sign;
                                     
                                     let cx = mid_x + perp_x;
                                     let cy = mid_y + perp_y;
                                     
                                     let angle1 = (start_pos.y - cy).atan2(start_pos.x - cx);
                                     let angle2 = (end_pos.y - cy).atan2(end_pos.x - cx);
                                     
                                     let mut diff = angle2 - angle1;
                                     if diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
                                     if diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
                                     
                                     let mut points = Vec::new();
                                     let steps = 15;
                                     for i in 0..=steps {
                                         let t = i as f32 / steps as f32;
                                         let a = angle1 + diff * t;
                                         let px = cx + radius * a.cos();
                                         let py = cy + radius * a.sin();
                                         points.push(to_screen(egui::Pos2::new(px, py)));
                                     }
                                     
                                     painter.add(egui::Shape::Path(egui::epaint::PathShape::line(points, stroke)));
                                     continue;
                                 }
                              }
                          }
                      }

                      // Straight Line Fallback
                      painter.line_segment([start_screen, end_screen], stroke);
                  }
             }

             // Draw Nodes (Simple Vector Circles for cleaner presentation and proper alignment)
             for group in &self.psg.groups {
                 if group.is_proxy {
                     continue;
                 }
                 for node in &group.nodes {
                     if let Some(&pos) = node_positions.get(&node.skill_id) {
                          let screen_pos = to_screen(pos);
                          
                          // Culling
                          if !response.rect.expand(50.0).contains(screen_pos) {
                              continue;
                          }

                          let mut radius = 6.0 * self.state.zoom;
                          let mut color = if self.psg.roots.contains(&node.skill_id) {
                              egui::Color32::GOLD
                          } else {
                              egui::Color32::from_rgb(100, 100, 200)
                          };
                          let is_hovered = self.state.hovered_node == Some(node.skill_id);

                          if let Some(db) = &db_opt {
                              if let Some(compact) = db.nodes.get(&node.skill_id) {
                                  let (r_val, c_val) = match compact.t.as_str() {
                                      "keystone" => (10.0, egui::Color32::from_rgb(255, 90, 120)),   // Keystone: larger pink/rose
                                      "notable" => (7.5, egui::Color32::from_rgb(255, 200, 50)),     // Notable: orange/gold
                                      "jewel" => (7.0, egui::Color32::from_rgb(0, 220, 180)),        // Jewel: teal
                                      "mastery" => (6.0, egui::Color32::from_rgb(180, 100, 255)),    // Mastery: purple
                                      _ => (4.5, egui::Color32::from_rgb(100, 150, 250)),            // Normal: blue-gray
                                  };
                                  radius = r_val * self.state.zoom;
                                  color = c_val;
                              }
                          }

                          // Highlight hovered and root nodes
                          let stroke = if is_hovered {
                              let stroke_color = if ui.visuals().dark_mode {
                                  egui::Color32::WHITE
                              } else {
                                  egui::Color32::from_rgb(20, 20, 20)
                              };
                              egui::Stroke::new(2.0 * self.state.zoom, stroke_color)
                          } else if self.psg.roots.contains(&node.skill_id) {
                              egui::Stroke::new(1.5 * self.state.zoom, egui::Color32::from_rgb(255, 215, 0)) // Gold
                          } else {
                              let border_color = if ui.visuals().dark_mode {
                                  egui::Color32::from_rgb(20, 20, 20)
                              } else {
                                  egui::Color32::from_rgb(160, 160, 165)
                              };
                              egui::Stroke::new(1.0 * self.state.zoom, border_color)  // Subtle border
                          };

                          painter.circle(screen_pos, radius, color, stroke);
                     }
                 }
             }

             // Hover interaction & Detailed tooltip
             if let Some(hovered_id) = self.state.hovered_node {
                 egui::show_tooltip(ui.ctx(), ui.layer_id(), egui::Id::new(hovered_id), |ui| {
                     if let Some(db) = &db_opt {
                         if let Some(compact) = db.nodes.get(&hovered_id) {
                             let name_color = match compact.t.as_str() {
                                 "keystone" => egui::Color32::from_rgb(255, 90, 120),  // Pink/Rose
                                 "notable" => egui::Color32::from_rgb(255, 200, 50),   // Orange/Gold
                                 "jewel" => egui::Color32::from_rgb(0, 220, 180),      // Teal
                                 _ => if ui.visuals().dark_mode {
                                     egui::Color32::WHITE
                                 } else {
                                     egui::Color32::from_rgb(24, 24, 28)
                                 },
                             };
                             
                             ui.vertical(|ui| {
                                 ui.label(egui::RichText::new(&compact.n).color(name_color).strong().size(15.0));
                                 
                                 let type_label = match compact.t.as_str() {
                                     "keystone" => "Keystone Passive Skill",
                                     "notable" => "Notable Passive Skill",
                                     "jewel" => "Jewel Socket",
                                     "mastery" => "Passive Skill Mastery",
                                     _ => "Passive Skill",
                                 };
                                 let type_color = if ui.visuals().dark_mode {
                                     egui::Color32::from_rgb(150, 150, 150)
                                 } else {
                                     egui::Color32::from_rgb(100, 100, 110)
                                 };
                                 ui.label(egui::RichText::new(type_label).color(type_color).size(11.0).italics());
                                 
                                 if !compact.s.is_empty() {
                                     ui.add_space(4.0);
                                     ui.separator();
                                     ui.add_space(4.0);
                                     for stat in &compact.s {
                                         let stat_color = if ui.visuals().dark_mode {
                                             egui::Color32::from_rgb(180, 210, 255)
                                         } else {
                                             egui::Color32::from_rgb(37, 99, 235)
                                         };
                                         ui.label(egui::RichText::new(stat).color(stat_color).size(12.0));
                                     }
                                 }
                             });
                         } else {
                             ui.label(format!("Skill ID: {}", hovered_id));
                         }
                     } else {
                         ui.label(format!("Skill ID: {}", hovered_id));
                     }
                 });
             }
        });
    }
}
