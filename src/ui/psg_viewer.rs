use eframe::egui;
use crate::dat::psg::PsgFile;

// Standard PoE Orbit Radii (Approximation for PoE2 or legacy)
// 0: Center
// 1: Inner circle
// 2: Middle circle
// 3: Large circle
// 4: Outer circle
// 5: Greater outer circle
// 6: Master circle
// 7: Orbit 7
// 8: Orbit 8
// 9: Orbit 9
const ORBIT_RADII: [f32; 10] = [0.0, 82.0, 162.0, 335.0, 493.0, 662.0, 846.0, 251.0, 1080.0, 1322.0];

fn get_node_angle(radius: u32, position: u32, passives_per_orbit: &[u8]) -> f32 {
    let r_idx = radius as usize;
    let capacities = if r_idx < passives_per_orbit.len() {
        passives_per_orbit[r_idx] as u32
    } else {
        12
    };

    let degree = if capacities == 16 {
        let angles = [0, 30, 45, 60, 90, 120, 135, 150, 180, 210, 225, 240, 270, 300, 315, 330];
        if (position as usize) < angles.len() {
            angles[position as usize] as f32
        } else {
            (360 * position) as f32 / capacities as f32
        }
    } else if capacities == 40 {
        let angles = [
            0, 10, 20, 30, 40, 45, 50, 60, 70, 80, 90, 100, 110, 120, 130, 135, 140, 150, 160, 170, 180, 190,
            200, 210, 220, 225, 230, 240, 250, 260, 270, 280, 290, 300, 310, 315, 320, 330, 340, 350,
        ];
        if (position as usize) < angles.len() {
            angles[position as usize] as f32
        } else {
            (360 * position) as f32 / capacities as f32
        }
    } else {
        (360 * position) as f32 / capacities as f32
    };

    degree.to_radians()
}

pub struct PsgViewerState {
    pub pan: egui::Vec2,
    pub zoom: f32,
    // Toggle for JSON view vs Graph view
    pub show_graph: bool,
}

impl Default for PsgViewerState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::new(0.0, 0.0),
            zoom: 0.2, // Start zoomed out
            show_graph: true, 
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

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.state.show_graph {
            if ui.button("Switch to Graph View").clicked() {
                self.state.show_graph = true;
            }
            ui.separator();
            ui.label("Raw Data (Visualization Disabled):");
            // Caller handles generic fallback if we return, or we can just render nothing here?
            // Actually content_view calls this. If show_graph is false, content_view should probably show JSON.
            // But we can put the toggle logic here.
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
                // User requested "mouse scroll wheel to move in and out"
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
            let to_screen = |pos: egui::Pos2| -> egui::Pos2 {
                (pos.to_vec2() * self.state.zoom + self.state.pan).to_pos2() + response.rect.center().to_vec2()
            };

            // Calculate Node Positions
            // To draw lines, we need positions of all nodes. 
            // PSG structure: Groups -> Nodes.
            // Nodes only know their local Group? 
            // PsgNode has "connections" which are u32 IDs.
            // We need a lookup of ID -> Position.
            // PsgFile data structure is hierarchical. Roots -> Groups -> Nodes.
            // But connections might cross groups?
            // Let's first compute absolute positions for all nodes.
            
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
                for node in &group.nodes {
                    // Calculate position
                    // Standard orbit calculation
                    let r_idx = node.radius as usize;
                    let radius = if r_idx < ORBIT_RADII.len() { ORBIT_RADII[r_idx] } else { node.radius as f32 * 50.0 };
                    
                    let poe_arc = get_node_angle(node.radius, node.position, &self.psg.passives_per_orbit);
                    let angle = -std::f32::consts::FRAC_PI_2 + poe_arc;
                    
                    let offset_x = angle.cos() * radius;
                    let offset_y = angle.sin() * radius;
                    
                    let pos = egui::Pos2::new(
                        group.x + offset_x,
                        group.y + offset_y
                    );
                    
                    node_positions.insert(node.skill_id, pos);
                    node_info.insert(node.skill_id, PsgNodeInfo {
                        pos,
                        group_x: group.x,
                        group_y: group.y,
                        poe_arc,
                        radius,
                    });
                }
            }
            
            // Draw Edges (Connections)
            // First, collect all unique connections to avoid duplicates and handle bidirectional definitions.
            // We want to prioritize drawing curves (orbit != 0) if one side defines it.
            let mut unique_connections: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();

            for group in &self.psg.groups {
                for node in &group.nodes {
                    for conn in &node.connections {
                        let (a, b) = if node.skill_id < conn.node_id {
                            (node.skill_id, conn.node_id)
                        } else {
                            (conn.node_id, node.skill_id)
                        };
                        
                        let entry = unique_connections.entry((a, b)).or_insert(0);
                        // If we found a curved connection, overwrite a straight one.
                        // If we already have a curve, keep it (or maybe pick the one with larger abs orbit?)
                        if conn.orbit != 0 {
                            *entry = conn.orbit;
                        }
                    }
                }
            }

            for ((start_id, end_id), orbit_idx) in unique_connections {
                 if let (Some(start_node), Some(end_node)) = (node_info.get(&start_id), node_info.get(&end_id)) {
                     let start_screen = to_screen(start_node.pos);
                     let end_screen = to_screen(end_node.pos);

                     // Check visibility (culling) - simple check
                     let margin = 500.0 * self.state.zoom;
                     if !response.rect.expand(margin).contains(start_screen) && !response.rect.expand(margin).contains(end_screen) {
                         continue;
                     }

                     let stroke = egui::Stroke::new(1.0 * self.state.zoom, egui::Color32::from_gray(100));

                     if orbit_idx != 0 {
                         // Draw Arc
                         // 1. Preferred method: If they are in the same group, draw using the group center
                         if (start_node.group_x - end_node.group_x).abs() < 0.1 
                            && (start_node.group_y - end_node.group_y).abs() < 0.1 
                            && (start_node.radius - end_node.radius).abs() < 0.1 
                         {
                             let group_x = start_node.group_x;
                             let group_y = start_node.group_y;
                             let radius = start_node.radius;

                             let arc1 = start_node.poe_arc;
                             let arc2 = end_node.poe_arc;

                             let mut start_arc = if arc1 < arc2 { arc1 } else { arc2 };
                             let mut end_arc = if arc1 < arc2 { arc2 } else { arc1 };

                             let diff = end_arc - start_arc;
                             if diff >= std::f32::consts::PI {
                                 let c = std::f32::consts::TAU - diff;
                                 start_arc = end_arc;
                                 end_arc = start_arc + c;
                             }

                             let angle1 = -std::f32::consts::FRAC_PI_2 + start_arc;
                             let angle2 = -std::f32::consts::FRAC_PI_2 + end_arc;

                             let mut points = Vec::new();
                             let steps = 15;
                             for i in 0..=steps {
                                 let t = i as f32 / steps as f32;
                                 let a = angle1 + (angle2 - angle1) * t;
                                 let px = group_x + radius * a.cos();
                                 let py = group_y + radius * a.sin();
                                 points.push(to_screen(egui::Pos2::new(px, py)));
                             }

                             painter.add(egui::Shape::Path(egui::epaint::PathShape::line(points, stroke)));
                             continue;
                         } else {
                             // 2. Fallback method: Perpendicular sphere intersection
                             let start_pos = start_node.pos;
                             let end_pos = end_node.pos;
                             let orbit_abs = orbit_idx.abs() as usize;
                             let radius = if orbit_abs < ORBIT_RADII.len() { ORBIT_RADII[orbit_abs] } else { 0.0 };
                             
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
                     painter.line_segment(
                         [start_screen, end_screen],
                         stroke
                     );
                 }
            }

            // Draw Nodes
            for group in &self.psg.groups {
                for node in &group.nodes {
                    if let Some(&pos) = node_positions.get(&node.skill_id) {
                        let screen_pos = to_screen(pos);
                        
                        // Culling
                        if !response.rect.expand(50.0).contains(screen_pos) {
                            continue;
                        }

                        let radius = 6.0 * self.state.zoom;
                        let color = if self.psg.roots.contains(&node.skill_id) {
                            egui::Color32::GOLD
                        } else {
                            egui::Color32::from_rgb(100, 100, 200)
                        };
                        
                        painter.circle_filled(screen_pos, radius, color);
                        
                        // Hover interaction
                        let screen_radius = radius.max(4.0); // Minimum hit size
                        if ui.rect_contains_pointer(egui::Rect::from_center_size(screen_pos, egui::Vec2::splat(screen_radius * 2.0))) {
                             egui::show_tooltip(ui.ctx(), ui.layer_id(), egui::Id::new(node.skill_id), |ui| {
                                 ui.label(format!("Skill ID: {}", node.skill_id));
                                 ui.label(format!("Group Pos: {:.1}, {:.1}", group.x, group.y));
                                 ui.label(format!("Radius: {} (idx), Pos: {}", node.radius, node.position));
                             });
                        }
                    }
                }
            }
        });
    }
}
