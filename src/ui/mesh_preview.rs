//! 3D preview for `.fmt`/`.tgm`/`.smd` meshes and `.ast` skeletons, projected in
//! software and drawn through egui's painter: orbit/zoom/pan camera, flat shading,
//! painter's-algorithm depth ordering, optional wireframe.

use crate::parsers::model::dolm::{Dolm, IndexBuffer};
use crate::parsers::model::{ast, fmt, smd, tgm, ModelFile};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

const MAX_TRIS: usize = 300_000;
const MAX_WIRE_TRIS: usize = 60_000;

pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Bone segments for skeletons.
    pub lines: Vec<(usize, usize)>,
    /// `(name, first triangle, triangle count)`.
    pub shapes: Vec<(String, usize, usize)>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl MeshData {
    fn finish(mut self) -> Self {
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &self.positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        if self.positions.is_empty() {
            min = [0.0; 3];
            max = [0.0; 3];
        }
        self.min = min;
        self.max = max;
        self
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

fn indices_of(buf: &IndexBuffer) -> Vec<u32> {
    match buf {
        IndexBuffer::U16(v) => v.iter().map(|i| *i as u32).collect(),
        IndexBuffer::U32(v) => v.clone(),
    }
}

fn empty() -> MeshData {
    MeshData { positions: Vec::new(), indices: Vec::new(), lines: Vec::new(), shapes: Vec::new(), min: [0.0; 3], max: [0.0; 3] }
}

/// Adds the most detailed LOD (the one with the most triangles).
fn append_dolm(out: &mut MeshData, dolm: &Dolm) {
    let Some(lod) = dolm.lods.iter().max_by_key(|l| l.indices.len()) else { return };
    let base = out.positions.len() as u32;
    out.positions.extend(lod.vertices.iter().map(|v| v.pos));
    out.indices.extend(indices_of(&lod.indices).into_iter().map(|i| i + base));
}

/// Geometry the preview can draw, or `None` when the file has no vertices.
pub fn extract(model: &ModelFile) -> Option<MeshData> {
    let mut out = empty();
    match model {
        ModelFile::Fmt(f) => {
            match &f.section {
                fmt::Section::V8(s) => {
                    out.positions.extend(s.vertex_buffer.iter().map(|v| v.pos));
                    out.indices = indices_of(&s.index_buffer);
                }
                fmt::Section::V9(d) => append_dolm(&mut out, d),
            }
            let total = out.indices.len() / 3;
            let starts: Vec<usize> = f.shapes.iter().map(|s| s.triangle_start as usize).collect();
            for (i, s) in f.shapes.iter().enumerate() {
                let start = starts[i].min(total);
                let end = starts.get(i + 1).copied().unwrap_or(total).clamp(start, total);
                out.shapes.push((if s.name.is_empty() { s.material.clone() } else { s.name.clone() }, start, end - start));
            }
        }
        ModelFile::Tgm(t) => match &t.section {
            tgm::Section::V8(s) => {
                for (mi, m) in s.meshes.iter().enumerate() {
                    let base = out.positions.len() as u32;
                    let first = out.indices.len() / 3;
                    out.positions.extend(m.vertices.iter().map(|v| v.pos));
                    out.indices.extend(indices_of(&m.indices).into_iter().map(|i| i + base));
                    out.shapes.push((if mi == 0 { "main".into() } else { "ground".into() }, first, out.indices.len() / 3 - first));
                }
            }
            tgm::Section::V9(s) => {
                for (gi, g) in s.geometries.iter().enumerate() {
                    let first = out.indices.len() / 3;
                    append_dolm(&mut out, &g.dolm);
                    out.shapes.push((if gi == 0 { "main".into() } else { "ground".into() }, first, out.indices.len() / 3 - first));
                }
            }
        },
        ModelFile::Smd(s) => match &s.section {
            smd::Section::V2(v2) => {
                out.positions.extend(v2.vertex_buffer.iter().map(|v| v.pos));
                out.indices = indices_of(&v2.index_buffer);
                let total = out.indices.len() / 3;
                let starts: Vec<usize> = v2.shape_extents.iter().map(|s| s.triangle_index as usize).collect();
                for (i, s) in v2.shape_extents.iter().enumerate() {
                    let start = starts[i].min(total);
                    let end = starts.get(i + 1).copied().unwrap_or(total).clamp(start, total);
                    out.shapes.push((s.name.clone(), start, end - start));
                }
            }
            smd::Section::V3(v3) => {
                append_dolm(&mut out, &v3.dolm);
                let total = out.indices.len() / 3;
                if let Some(lod) = v3.dolm.lods.first() {
                    for (i, ext) in lod.shape_extents.iter().enumerate() {
                        let name = v3.shape_names.get(i).cloned().unwrap_or_else(|| format!("shape {}", i));
                        let start = (ext.start_index as usize / 3).min(total);
                        let count = (ext.count_index as usize / 3).min(total - start);
                        out.shapes.push((name, start, count));
                    }
                }
            }
        },
        ModelFile::Ast(a) => {
            let bones: &[ast::Bone] = &a.bones;
            let n = bones.len();
            let mut parent = vec![None; n];
            for (i, b) in bones.iter().enumerate() {
                let mut c = b.child.map(|c| c as usize);
                while let Some(ci) = c {
                    if ci < n && parent[ci].is_none() && ci != i {
                        parent[ci] = Some(i);
                        c = bones[ci].sibling.map(|s| s as usize);
                    } else {
                        break;
                    }
                }
            }
            // Row-major transforms with the translation in the last row; local to parent.
            let mut world: Vec<Option<[[f32; 4]; 4]>> = vec![None; n];
            fn resolve(i: usize, bones: &[ast::Bone], parent: &[Option<usize>], world: &mut Vec<Option<[[f32; 4]; 4]>>, depth: usize) -> [[f32; 4]; 4] {
                if let Some(w) = world[i] {
                    return w;
                }
                let local = bones[i].transform;
                let w = match parent[i] {
                    Some(p) if depth < 64 => mul(&local, &resolve(p, bones, parent, world, depth + 1)),
                    _ => local,
                };
                world[i] = Some(w);
                w
            }
            for i in 0..n {
                let w = resolve(i, bones, &parent, &mut world, 0);
                out.positions.push([w[3][0], w[3][1], w[3][2]]);
            }
            for (i, p) in parent.iter().enumerate() {
                if let Some(p) = p {
                    out.lines.push((*p, i));
                }
            }
        }
    }
    if out.positions.is_empty() {
        return None;
    }
    Some(out.finish())
}

fn mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

pub struct MeshPreviewState {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
    pub wireframe: bool,
    pub cull: bool,
    pub z_up: bool,
    pub shape: Option<usize>,
}

impl Default for MeshPreviewState {
    fn default() -> Self {
        Self { yaw: 0.6, pitch: 0.45, zoom: 1.0, pan: Vec2::ZERO, wireframe: false, cull: true, z_up: true, shape: None }
    }
}

fn shape_color(i: usize, dark: bool) -> Color32 {
    let mut h: u32 = 2166136261 ^ (i as u32).wrapping_mul(2654435761);
    h ^= h >> 13;
    h = h.wrapping_mul(16777619);
    egui::ecolor::Hsva::new((h % 360) as f32 / 360.0, if dark { 0.35 } else { 0.45 }, if dark { 0.85 } else { 0.75 }, 1.0).into()
}

struct Camera {
    right: [f32; 3],
    up: [f32; 3],
    fwd: [f32; 3],
    eye: [f32; 3],
    focal: f32,
    center: Pos2,
}

impl Camera {
    fn project(&self, p: [f32; 3]) -> Option<(Pos2, f32)> {
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let z = dot(d, self.fwd);
        if z <= 1e-3 {
            return None;
        }
        let x = dot(d, self.right) / z * self.focal;
        let y = dot(d, self.up) / z * self.focal;
        Some((Pos2::new(self.center.x + x, self.center.y - y), z))
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

pub struct MeshPreview;

impl MeshPreview {
    pub fn show(ui: &mut egui::Ui, id: u64, mesh: &MeshData, state: &mut MeshPreviewState) {
        let dark = ui.visuals().dark_mode;
        let is_skeleton = mesh.indices.is_empty();
        ui.horizontal_wrapped(|ui| {
            if is_skeleton {
                crate::ui::components::badge(ui, &format!("{} bones", mesh.positions.len()));
            } else {
                crate::ui::components::badge(ui, &format!("{} triangles", mesh.triangle_count()));
                crate::ui::components::badge(ui, &format!("{} vertices", mesh.positions.len()));
            }
            let size = [mesh.max[0] - mesh.min[0], mesh.max[1] - mesh.min[1], mesh.max[2] - mesh.min[2]];
            crate::ui::components::badge(ui, &format!("{:.0}×{:.0}×{:.0}", size[0], size[1], size[2]));
            if !is_skeleton {
                ui.toggle_value(&mut state.wireframe, "Wireframe");
                ui.toggle_value(&mut state.cull, "Cull").on_hover_text("Hide triangles facing away from the camera");
            }
            ui.toggle_value(&mut state.z_up, "Z up");
            if ui.button("Reset view").clicked() {
                let keep = (state.wireframe, state.cull, state.z_up, state.shape);
                *state = MeshPreviewState::default();
                (state.wireframe, state.cull, state.z_up, state.shape) = keep;
            }
            if mesh.shapes.len() > 1 {
                egui::ComboBox::from_id_salt(("mesh_shape", id))
                    .selected_text(state.shape.and_then(|s| mesh.shapes.get(s)).map(|s| s.0.clone()).unwrap_or_else(|| "All shapes".into()))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.shape, None, "All shapes");
                        for (i, (name, _, count)) in mesh.shapes.iter().enumerate() {
                            ui.selectable_value(&mut state.shape, Some(i), format!("{} · {} tris", name, count));
                        }
                    });
            }
            ui.label(egui::RichText::new("drag: orbit · wheel: zoom · right-drag: pan").weak().size(10.5));
        });

        let avail = ui.available_size();
        let size = Vec2::new(avail.x.max(200.0), avail.y.max(280.0));
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            state.yaw += d.x * 0.01;
            state.pitch = (state.pitch + d.y * 0.01).clamp(-1.5, 1.5);
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            state.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            let zoom = ui.input(|i| i.zoom_delta());
            if scroll != 0.0 {
                state.zoom = (state.zoom * (1.0 - scroll * 0.0015)).clamp(0.05, 40.0);
            }
            if zoom != 1.0 {
                state.zoom = (state.zoom / zoom).clamp(0.05, 40.0);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, if dark { Color32::from_rgb(22, 22, 28) } else { Color32::from_rgb(240, 240, 246) });

        // Orbit around the bounding-box centre; radius scales with the model.
        let centre = [(mesh.min[0] + mesh.max[0]) * 0.5, (mesh.min[1] + mesh.max[1]) * 0.5, (mesh.min[2] + mesh.max[2]) * 0.5];
        let radius = (0..3).map(|k| mesh.max[k] - mesh.min[k]).fold(0.0_f32, f32::max).max(1.0) * 0.5;
        let dist = radius * 2.6 * state.zoom;
        let (cy, sy, cp, sp) = (state.yaw.cos(), state.yaw.sin(), state.pitch.cos(), state.pitch.sin());
        // Camera direction in a Y-up frame, then remapped when the model is Z-up.
        let (fwd, up_hint) = if state.z_up {
            (norm([-cp * sy, -cp * cy, -sp]), [0.0, 0.0, 1.0])
        } else {
            (norm([-cp * sy, -sp, -cp * cy]), [0.0, 1.0, 0.0])
        };
        let right = norm(cross(fwd, up_hint));
        let up = norm(cross(right, fwd));
        let eye = [centre[0] - fwd[0] * dist, centre[1] - fwd[1] * dist, centre[2] - fwd[2] * dist];
        let focal = rect.height() / (2.0 * (22.5_f32).to_radians().tan());
        let cam = Camera { right, up, fwd, eye, focal, center: rect.center() + state.pan };

        let projected: Vec<Option<(Pos2, f32)>> = mesh.positions.iter().map(|p| cam.project(*p)).collect();

        // Ground grid through the model's lowest point.
        let grid_c = if dark { Color32::from_rgb(45, 45, 54) } else { Color32::from_rgb(210, 210, 220) };
        let floor = if state.z_up { mesh.min[2] } else { mesh.min[1] };
        let step = (radius * 2.0 / 8.0).max(1.0);
        for i in -8..=8 {
            let o = i as f32 * step;
            let (a, b, c, d) = if state.z_up {
                ([centre[0] + o, centre[1] - 8.0 * step, floor], [centre[0] + o, centre[1] + 8.0 * step, floor], [centre[0] - 8.0 * step, centre[1] + o, floor], [centre[0] + 8.0 * step, centre[1] + o, floor])
            } else {
                ([centre[0] + o, floor, centre[2] - 8.0 * step], [centre[0] + o, floor, centre[2] + 8.0 * step], [centre[0] - 8.0 * step, floor, centre[2] + o], [centre[0] + 8.0 * step, floor, centre[2] + o])
            };
            for (p, q) in [(a, b), (c, d)] {
                if let (Some((p, _)), Some((q, _))) = (cam.project(p), cam.project(q)) {
                    painter.line_segment([p, q], Stroke::new(1.0_f32, grid_c));
                }
            }
        }

        if is_skeleton {
            let bone_c = if dark { Color32::from_rgb(229, 192, 123) } else { Color32::from_rgb(170, 110, 0) };
            for (a, b) in &mesh.lines {
                if let (Some(Some((p, _))), Some(Some((q, _)))) = (projected.get(*a), projected.get(*b)) {
                    painter.line_segment([*p, *q], Stroke::new(2.0_f32, bone_c));
                }
            }
            for p in projected.iter().flatten() {
                painter.circle_filled(p.0, 3.0, bone_c);
            }
            Self::gizmo(&painter, rect, &cam);
            return;
        }

        let (tri_start, tri_end) = match state.shape.and_then(|s| mesh.shapes.get(s)) {
            Some((_, start, count)) => (*start, start + count),
            None => (0, mesh.triangle_count()),
        };
        let total = tri_end - tri_start;
        let stride = (total / MAX_TRIS).max(1);
        let light = norm([fwd[0] * -0.4 + right[0] * 0.5 + up[0] * 0.75, fwd[1] * -0.4 + right[1] * 0.5 + up[1] * 0.75, fwd[2] * -0.4 + right[2] * 0.5 + up[2] * 0.75]);
        let shape_of = |tri: usize| mesh.shapes.iter().position(|(_, s, c)| tri >= *s && tri < s + c).unwrap_or(0);
        let mut tris: Vec<(f32, [Pos2; 3], Color32)> = Vec::with_capacity(total / stride + 1);
        let mut tri = tri_start;
        while tri < tri_end {
            let (ia, ib, ic) = (mesh.indices[tri * 3] as usize, mesh.indices[tri * 3 + 1] as usize, mesh.indices[tri * 3 + 2] as usize);
            if let (Some(Some(a)), Some(Some(b)), Some(Some(c))) = (projected.get(ia), projected.get(ib), projected.get(ic)) {
                let area = (b.0.x - a.0.x) * (c.0.y - a.0.y) - (b.0.y - a.0.y) * (c.0.x - a.0.x);
                if !(state.cull && area > 0.0) {
                    let (pa, pb, pc) = (mesh.positions[ia], mesh.positions[ib], mesh.positions[ic]);
                    let n = norm(cross([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]], [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]]));
                    let shade = dot(n, light).abs().clamp(0.0, 1.0) * 0.75 + 0.25;
                    let base = shape_color(shape_of(tri), dark);
                    let col = Color32::from_rgb((base.r() as f32 * shade) as u8, (base.g() as f32 * shade) as u8, (base.b() as f32 * shade) as u8);
                    tris.push(((a.1 + b.1 + c.1) / 3.0, [a.0, b.0, c.0], col));
                }
            }
            tri += stride;
        }
        tris.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut m = egui::Mesh::default();
        for (_, pts, col) in &tris {
            let base = m.vertices.len() as u32;
            for p in pts {
                m.colored_vertex(*p, *col);
            }
            m.add_triangle(base, base + 1, base + 2);
        }
        painter.add(egui::Shape::mesh(m));

        if state.wireframe {
            if tris.len() <= MAX_WIRE_TRIS {
                let wire = if dark { Color32::from_rgba_unmultiplied(255, 255, 255, 70) } else { Color32::from_rgba_unmultiplied(0, 0, 0, 70) };
                let mut segs = Vec::with_capacity(tris.len() * 3);
                for (_, pts, _) in &tris {
                    segs.push(egui::Shape::line_segment([pts[0], pts[1]], Stroke::new(1.0_f32, wire)));
                    segs.push(egui::Shape::line_segment([pts[1], pts[2]], Stroke::new(1.0_f32, wire)));
                    segs.push(egui::Shape::line_segment([pts[2], pts[0]], Stroke::new(1.0_f32, wire)));
                }
                painter.extend(segs);
            } else {
                painter.text(rect.left_top() + Vec2::new(8.0, 28.0), egui::Align2::LEFT_TOP, format!("wireframe off: more than {} triangles", MAX_WIRE_TRIS), egui::FontId::proportional(11.0), Color32::from_rgb(245, 158, 11));
            }
        }
        if stride > 1 {
            painter.text(rect.left_top() + Vec2::new(8.0, 8.0), egui::Align2::LEFT_TOP, format!("showing every {}th triangle", stride), egui::FontId::proportional(11.0), Color32::from_rgb(245, 158, 11));
        }
        Self::gizmo(&painter, rect, &cam);
    }

    /// Axis gizmo in the corner so the orientation is readable.
    fn gizmo(painter: &egui::Painter, rect: Rect, cam: &Camera) {
        let origin = Pos2::new(rect.left() + 36.0, rect.bottom() - 36.0);
        for (axis, color, label) in [([1.0, 0.0, 0.0], Color32::from_rgb(239, 68, 68), "x"), ([0.0, 1.0, 0.0], Color32::from_rgb(74, 222, 128), "y"), ([0.0, 0.0, 1.0], Color32::from_rgb(96, 165, 250), "z")] {
            let dx = dot(axis, cam.right);
            let dy = dot(axis, cam.up);
            let end = origin + Vec2::new(dx * 24.0, -dy * 24.0);
            painter.line_segment([origin, end], Stroke::new(2.0_f32, color));
            painter.text(end + Vec2::new(dx * 6.0, -dy * 6.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_points_in_front_of_the_camera() {
        let cam = Camera { right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], fwd: [0.0, 0.0, 1.0], eye: [0.0, 0.0, -10.0], focal: 100.0, center: Pos2::new(50.0, 50.0) };
        let (p, z) = cam.project([1.0, 2.0, 0.0]).unwrap();
        assert!((z - 10.0).abs() < 1e-6);
        assert!((p.x - 60.0).abs() < 1e-4 && (p.y - 30.0).abs() < 1e-4);
        assert!(cam.project([0.0, 0.0, -20.0]).is_none());
    }

    #[test]
    fn matrix_multiply_is_row_major() {
        let t = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        let r = mul(&t, &t);
        assert_eq!(r[3], [2.0, 4.0, 6.0, 1.0]);
    }
}
