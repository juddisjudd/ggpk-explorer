use eframe::egui;
use crate::dat::psg::PsgFile;

// Standard PoE Orbit Radii (Approximation for PoE2 or legacy)
// 0: Center
// 1: Inner circle (6 nodes)
// 2: Middle circle (12 nodes)
// 3: Large circle (12 nodes?) - varies
// 4: Outer circle (40 nodes) or similar.
const ORBIT_RADII: [f32; 7] = [0.0, 82.0, 162.0, 335.0, 493.0, 662.0, 846.0];
const ORBIT_NODES: [u32; 7] = [1, 6, 12, 12, 40, 72, 72]; // Capacities

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
            
            for group in &self.psg.groups {
                for node in &group.nodes {
                    // Calculate position
                    // Standard orbit calculation
                    let r_idx = node.radius as usize;
                    let radius = if r_idx < ORBIT_RADII.len() { ORBIT_RADII[r_idx] } else { node.radius as f32 * 50.0 };
                    let capacities = if r_idx < ORBIT_NODES.len() { ORBIT_NODES[r_idx] } else { 12 };
                    
                    // Angle
                    // Standard GGG: angle = -PI/2 + (2PI * position / capacity)
                    // Note: GGG angles start at top (-90 deg) and go clockwise usually?
                    let angle = -std::f32::consts::FRAC_PI_2 + (std::f32::consts::TAU * node.position as f32 / capacities as f32);
                    
                    let offset_x = angle.cos() * radius;
                    let offset_y = angle.sin() * radius;
                    
                    let pos = egui::Pos2::new(
                        group.x + offset_x,
                        group.y + offset_y
                    );
                    
                    node_positions.insert(node.skill_id, pos);
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
                 if let (Some(&start_pos), Some(&end_pos)) = (node_positions.get(&start_id), node_positions.get(&end_id)) {
                     let start_screen = to_screen(start_pos);
                     let end_screen = to_screen(end_pos);

                     // Check visibility (culling) - simple check
                     // Check if bounded box of line intersects screen?
                     // Or just check endpoints with margin
                     let margin = 500.0 * self.state.zoom;
                     if !response.rect.expand(margin).contains(start_screen) && !response.rect.expand(margin).contains(end_screen) {
                         continue;
                     }

                     let stroke = egui::Stroke::new(1.0 * self.state.zoom, egui::Color32::from_gray(100));

                     if orbit_idx != 0 {
                         // Draw Arc
                         // Calculate Orbit Radius
                         let orbit_abs = orbit_idx.abs() as usize;
                         let radius = if orbit_abs < ORBIT_RADII.len() { ORBIT_RADII[orbit_abs] } else { 0.0 };
                         
                         if radius > 0.0 {
                            // Calculate Center of the arc circle
                            // Distance between nodes
                            let dx = end_pos.x - start_pos.x;
                            let dy = end_pos.y - start_pos.y;
                            let dist = (dx*dx + dy*dy).sqrt();
                            
                            // Perpendicular distance to center
                            // r^2 = (d/2)^2 + h^2  => h = sqrt(r^2 - d^2/4)
                            // If dist > 2*r, then points are too far for this radius? (Shouldn't happen if valid)
                            if dist < radius * 2.0 {
                                let h = (radius * radius - (dist * dist) / 4.0).sqrt();
                                
                                // Direction of perpendicular: (dy, -dx) or (-dy, dx)
                                // PoB: (connection.orbit > 0 and 1 or -1)
                                let sign = if orbit_idx > 0 { 1.0 } else { -1.0 };
                                
                                let mid_x = start_pos.x + dx / 2.0;
                                let mid_y = start_pos.y + dy / 2.0;
                                
                                let perp_x = (dy / dist) * h * sign;
                                let perp_y = (-dx / dist) * h * sign; // Note: -dx because (x, y) -> (y, -x) is 90 deg clockwise?
                                // PoB: perp * (dy / dist), -perp * (dx / dist)
                                // Let's stick to standard vector math.
                                // Vec (dx, dy). Normal (-dy, dx) or (dy, -dx).
                                
                                let cx = mid_x + perp_x;
                                let cy = mid_y + perp_y;
                                
                                // Angles
                                let angle1 = (start_pos.y - cy).atan2(start_pos.x - cx);
                                let angle2 = (end_pos.y - cy).atan2(end_pos.x - cx);
                                
                                // Draw Arc Segments
                                let mut points = Vec::new();
                                let steps = 10;
                                
                                // We need to ensure we go the "short" way or "long" way?
                                // Usually short way for these connections.
                                let mut diff = angle2 - angle1;
                                // Normalize diff to -PI..PI
                                if diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
                                if diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
                                
                                for i in 0..=steps {
                                    let t = i as f32 / steps as f32;
                                    let a = angle1 + diff * t;
                                    let px = cx + radius * a.cos();
                                    let py = cy + radius * a.sin(); // Standard math
                                    points.push(to_screen(egui::Pos2::new(px, py)));
                                }
                                
                                painter.add(egui::Shape::Path(egui::epaint::PathShape::line(points, stroke)));
                                continue;
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
