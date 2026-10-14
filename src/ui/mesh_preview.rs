//! 3D preview for `.fmt`/`.tgm`/`.smd` meshes and `.ast` skeletons, projected in
//! software into a picture with a depth test per pixel: orbit/zoom/pan camera, each shape's colour
//! texture, and a lit view from its normal and gloss maps with a shadow; optional wireframe.

use crate::parsers::model::dolm::{Dolm, IndexBuffer};
use crate::parsers::model::{ast, fmt, smd, tgm, ModelFile};
use crate::ui::material_viewer::SurfaceMap;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

const MAX_TRIS: usize = 300_000;
const MAX_WIRE_TRIS: usize = 60_000;

/// What each material gives the preview, by material path (`None` = the material could not be read).
pub type Materials = HashMap<String, Option<Arc<Material>>>;

/// A material as the preview draws it.
pub struct Material {
    pub colour: Option<MeshTexture>,
    /// The map the lit picture reads surface detail from, and how its channels are packed.
    pub surface: Option<(MeshTexture, SurfaceMap)>,
    /// A material that only casts a shadow in the game, so its shapes are left out.
    pub hidden: bool,
}

/// A texture at each halving of its size, so a triangle drawn small reads texels near its own size.
pub struct MeshTexture {
    levels: Vec<Level>,
    /// Whether any texel is clear enough to cut a hole in what it covers.
    cut_out: bool,
}

struct Level {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
}

impl MeshTexture {
    pub fn new(image: &image::RgbaImage) -> Self {
        let mut levels = vec![Level { width: image.width().max(1) as usize, height: image.height().max(1) as usize, pixels: image.pixels().map(|p| p.0).collect() }];
        while let Some(smaller) = levels.last().filter(|l| l.width > 1 || l.height > 1).map(Level::halved) {
            levels.push(smaller);
        }
        Self { cut_out: image.pixels().any(|p| p.0[3] < 128), levels }
    }

    /// The level for a triangle that spends `share` of the texture's area on each pixel it fills.
    fn level(&self, share: f32) -> &Level {
        let full = &self.levels[0];
        let halvings = ((share * (full.width * full.height) as f32).max(1.0).log2() * 0.5).round() as usize;
        &self.levels[halvings.min(self.levels.len() - 1)]
    }
}

impl Level {
    fn halved(&self) -> Level {
        let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let at = |x: usize, y: usize| self.pixels[y.min(self.height - 1) * self.width + x.min(self.width - 1)];
        let pixels = (0..width * height)
            .map(|i| {
                let (x, y) = (i % width * 2, i / width * 2);
                let quad = [at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1)];
                std::array::from_fn(|channel| (quad.iter().map(|p| p[channel] as u32).sum::<u32>() / 4) as u8)
            })
            .collect();
        Level { width, height, pixels }
    }

    /// Blends the four texels around a point, wrapping at the edges the way the game's samplers do.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (x, y) = (u * self.width as f32 - 0.5, v * self.height as f32 - 0.5);
        let (left, top) = (x.floor(), y.floor());
        let (across, down) = (x - left, y - top);
        let wrap = |i: f32, n: usize| (i as i64).rem_euclid(n as i64) as usize;
        let (xa, xb, ya, yb) = (wrap(left, self.width), wrap(left + 1.0, self.width), wrap(top, self.height), wrap(top + 1.0, self.height));
        let texel = |x: usize, y: usize| self.pixels[y * self.width + x];
        let (a, b, c, d) = (texel(xa, ya), texel(xb, ya), texel(xa, yb), texel(xb, yb));
        std::array::from_fn(|i| {
            let upper = a[i] as f32 + (b[i] as f32 - a[i] as f32) * across;
            let lower = c[i] as f32 + (d[i] as f32 - c[i] as f32) * across;
            upper + (lower - upper) * down
        })
    }
}

/// A colour byte as the amount of light it stands for, and that amount back as a byte.
static LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| std::array::from_fn(|i| (i as f32 / 255.0).powf(2.2)));
static DISPLAY: LazyLock<[u8; 1025]> = LazyLock::new(|| std::array::from_fn(|i| ((i as f32 / 1024.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8));

const SHADOW_SIZE: usize = 1024;

/// How far toward the key light the model reaches, over a square that faces that light.
struct ShadowMap {
    right: [f32; 3],
    up: [f32; 3],
    toward: [f32; 3],
    centre: [f32; 3],
    radius: f32,
    reach: Vec<f32>,
}

impl ShadowMap {
    fn new(triangles: impl Iterator<Item = [[f32; 3]; 3]>, toward: [f32; 3], centre: [f32; 3], radius: f32) -> Self {
        let right = norm(cross(if toward[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] }, toward));
        let mut map = Self { right, up: cross(toward, right), toward, centre, radius, reach: vec![f32::NEG_INFINITY; SHADOW_SIZE * SHADOW_SIZE] };
        let edge = SHADOW_SIZE as f32 - 1.0;
        for corners in triangles {
            let [a, b, c] = corners.map(|p| map.place(p));
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let (min_x, max_x) = (a[0].min(b[0]).min(c[0]).floor().max(0.0), a[0].max(b[0]).max(c[0]).ceil().min(edge));
            let (min_y, max_y) = (a[1].min(b[1]).min(c[1]).floor().max(0.0), a[1].max(b[1]).max(c[1]).ceil().min(edge));
            if area == 0.0 || !area.is_finite() || min_x > max_x || min_y > max_y {
                continue;
            }
            for y in min_y as usize..=max_y as usize {
                for x in min_x as usize..=max_x as usize {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let wa = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                    let wb = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                    let wc = 1.0 - wa - wb;
                    if wa >= 0.0 && wb >= 0.0 && wc >= 0.0 {
                        let cell = &mut map.reach[y * SHADOW_SIZE + x];
                        *cell = cell.max(wa * a[2] + wb * b[2] + wc * c[2]);
                    }
                }
            }
        }
        map
    }

    /// Where a point falls on the map, and how far toward the light it is.
    fn place(&self, p: [f32; 3]) -> [f32; 3] {
        let from_centre = [p[0] - self.centre[0], p[1] - self.centre[1], p[2] - self.centre[2]];
        let half = SHADOW_SIZE as f32 * 0.5;
        [half + dot(from_centre, self.right) / self.radius * half, half + dot(from_centre, self.up) / self.radius * half, dot(from_centre, self.toward)]
    }

    /// The share of the key light that reaches a point, judged over the nine cells around it for a soft edge.
    fn light(&self, p: [f32; 3]) -> f32 {
        let [x, y, height] = self.place(p);
        let clear = height + self.radius * 0.012;
        let mut open = 0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (cx, cy) = (x as i32 + dx, y as i32 + dy);
                let outside = cx < 0 || cy < 0 || cx >= SHADOW_SIZE as i32 || cy >= SHADOW_SIZE as i32;
                open += (outside || self.reach[cy as usize * SHADOW_SIZE + cx as usize] <= clear) as u32;
            }
        }
        open as f32 / 9.0
    }
}

/// The lit picture's lights: a key light fixed to the model that casts the shadow, a fill beside the camera, and sky over ground.
struct Lights {
    key: [f32; 3],
    fill: [f32; 3],
    eye: [f32; 3],
    up: [f32; 3],
    shadow: Arc<ShadowMap>,
}

/// A triangle in picture pixels, with what the fill needs at each corner.
struct ScreenTriangle<'a> {
    points: [[f32; 2]; 3],
    inv_depth: [f32; 3],
    uv: [[f32; 2]; 3],
    /// How lit each corner is, from 0 to 1, for the picture that is not lit per pixel.
    shade: [f32; 3],
    tint: [f32; 3],
    texture: Option<&'a Level>,
    cut_out: bool,
    /// For the lit picture: where each corner is, which way it faces, and which way its texture runs across and down.
    world: [[f32; 3]; 3],
    normal: [[f32; 3]; 3],
    tangent: [[f32; 4]; 3],
    surface: Option<(&'a Level, SurfaceMap)>,
}

/// The ground under the model, drawn out to the horizon: a grid that fades as its squares shrink, and the model's shadow.
struct Ground {
    eye: [f32; 3],
    corner: [f32; 3],
    across: [f32; 3],
    down: [f32; 3],
    /// How far along its ray a point is for each unit of depth, which is the picture's focal length in pixels.
    reach: f32,
    /// The axis that is up, and the ground's height on it.
    axis: usize,
    level: f32,
    step: f32,
    line: Color32,
}

impl Ground {
    /// Where the ray through a pixel meets the ground, counted in grid squares, and one over its depth; `None` when it never does.
    fn meet(&self, x: f32, y: f32) -> Option<([f32; 3], [f32; 2], f32)> {
        let ray = [0, 1, 2].map(|i| self.corner[i] + self.across[i] * x + self.down[i] * y);
        let along = (self.level - self.eye[self.axis]) / ray[self.axis];
        let point = [0, 1, 2].map(|i| self.eye[i] + ray[i] * along);
        let flat = if self.axis == 2 { [point[0], point[1]] } else { [point[0], point[2]] };
        (along > 0.0 && along.is_finite()).then_some((point, flat.map(|side| side / self.step), 1.0 / (along * self.reach)))
    }

    /// The ground's colour over a pixel, alpha already multiplied in, with one over its depth.
    fn colour(&self, x: f32, y: f32, lights: Option<&Lights>) -> Option<([f32; 4], f32)> {
        let (point, square, inv_depth) = self.meet(x, y)?;
        let (right, below) = (self.meet(x + 1.0, y)?.1, self.meet(x, y + 1.0)?.1);
        let line = [0, 1].map(|side| {
            // How many squares one pixel spans here: lines closer than two pixels would only shimmer, so they fade out first.
            let span = (right[side] - square[side]).abs().max((below[side] - square[side]).abs()).max(1e-6);
            let off = (square[side] - square[side].round()).abs();
            (1.0 - off / span).clamp(0.0, 1.0) * ((0.5 - span) / 0.4).clamp(0.0, 1.0)
        });
        let grid = line[0].max(line[1]) * 0.9;
        let shadow = lights.map_or(0.0, |lights| (1.0 - lights.shadow.light(point)) * 0.41);
        let ink = [self.line.r(), self.line.g(), self.line.b()].map(|channel| channel as f32 * grid * (1.0 - shadow));
        Some(([ink[0], ink[1], ink[2], (grid + shadow - grid * shadow) * 255.0], inv_depth))
    }
}

fn mix<const N: usize>(corners: [[f32; N]; 3], share: [f32; 3]) -> [f32; N] {
    std::array::from_fn(|i| corners[0][i] * share[0] + corners[1][i] * share[1] + corners[2][i] * share[2])
}

impl ScreenTriangle<'_> {
    /// Each corner's share of a point on the triangle itself, from its share on screen: nearer corners count for more.
    fn shares(&self, screen: [f32; 3]) -> [f32; 3] {
        let near = [screen[0] * self.inv_depth[0], screen[1] * self.inv_depth[1], screen[2] * self.inv_depth[2]];
        near.map(|share| share / (near[0] + near[1] + near[2]))
    }

    fn texel(&self, share: [f32; 3]) -> [f32; 4] {
        let [u, v] = mix(self.uv, share);
        self.texture.map_or([255.0; 4], |level| level.sample(u, v))
    }

    fn colour(&self, share: [f32; 3], lights: Option<&Lights>) -> Color32 {
        let texel = self.texel(share);
        let Some(lights) = lights else {
            let shade = share[0] * self.shade[0] + share[1] * self.shade[1] + share[2] * self.shade[2];
            return Color32::from_rgb((texel[0] * self.tint[0] * shade) as u8, (texel[1] * self.tint[1] * shade) as u8, (texel[2] * self.tint[2] * shade) as u8);
        };
        let point = mix(self.world, share);
        let facing = norm(mix(self.normal, share));
        let (mut normal, mut occlusion, mut gloss, mut shine) = (facing, 1.0, 0.3, 0.06);
        if let Some((level, packing)) = self.surface {
            let [u, v] = mix(self.uv, share);
            let map = level.sample(u, v).map(|channel| channel / 255.0);
            let (x, y) = match packing {
                SurfaceMap::NormalGlossAo => (map[0] * 2.0 - 1.0, map[1] * 2.0 - 1.0),
                SurfaceMap::NormalSpec => (map[3] * 2.0 - 1.0, map[1] * 2.0 - 1.0),
            };
            (occlusion, gloss, shine) = match packing {
                SurfaceMap::NormalGlossAo => (map[2], map[3], 0.04 + 0.5 * map[3]),
                SurfaceMap::NormalSpec => (1.0, 0.35, map[0] * 0.9),
            };
            let tangent = mix(self.tangent, share);
            let along = [0, 1, 2].map(|i| tangent[i] - facing[i] * (tangent[0] * facing[0] + tangent[1] * facing[1] + tangent[2] * facing[2]));
            if dot(along, along) > 1e-6 {
                let along = norm(along);
                let down = cross(facing, along).map(|axis| axis * tangent[3].signum());
                let out = (1.0 - x * x - y * y).max(0.0).sqrt();
                normal = norm([0, 1, 2].map(|i| along[i] * x + down[i] * y + facing[i] * out));
            }
        }
        let view = norm([lights.eye[0] - point[0], lights.eye[1] - point[1], lights.eye[2] - point[2]]);
        let direct = dot(normal, lights.key).max(0.0);
        // The lookup starts a little off the surface, or a face would shadow itself through the map's coarse cells.
        let lifted = [0, 1, 2].map(|i| point[i] + facing[i] * lights.shadow.radius * 0.01);
        let reach = if direct > 0.0 { lights.shadow.light(lifted) } else { 0.0 };
        let sky = dot(normal, lights.up) * 0.5 + 0.5;
        let spread = 1.7 * direct * reach + 0.4 * dot(normal, lights.fill).max(0.0) + (0.16 + 0.3 * sky) * occlusion;
        let half = norm([lights.key[0] + view[0], lights.key[1] + view[1], lights.key[2] + view[2]]);
        let glint = dot(normal, half).max(0.0).powf(8.0 + gloss * gloss * 240.0) * shine * (1.0 + gloss * 3.0) * reach;
        let rim = (1.0 - dot(normal, view).max(0.0)).powi(3) * 0.1 * occlusion * sky;
        let lit = [0, 1, 2].map(|i| DISPLAY[((LINEAR[texel[i] as usize] * self.tint[i] * spread + glint + rim).clamp(0.0, 1.0) * 1024.0) as usize]);
        Color32::from_rgb(lit[0], lit[1], lit[2])
    }
}

const BAND_ROWS: usize = 32;

/// Finds the nearest triangle over each pixel, then colours it once, with the ground laid under or over it by depth; a texel under half alpha leaves the pixel to what lies behind.
fn rasterize(width: usize, height: usize, triangles: &[ScreenTriangle], lights: Option<&Lights>, ground: Option<&Ground>) -> egui::ColorImage {
    let mut bands: Vec<Vec<u32>> = vec![Vec::new(); height.div_ceil(BAND_ROWS)];
    for (index, triangle) in triangles.iter().enumerate() {
        let ys = triangle.points.map(|p| p[1]);
        let (top, bottom) = (ys.iter().copied().fold(f32::INFINITY, f32::min), ys.iter().copied().fold(f32::NEG_INFINITY, f32::max));
        if bottom >= 0.0 && top < height as f32 {
            let (first, last) = (top.max(0.0) as usize / BAND_ROWS, (bottom as usize).min(height - 1) / BAND_ROWS);
            bands[first..=last].iter_mut().for_each(|band| band.push(index as u32));
        }
    }
    let mut pixels = vec![Color32::TRANSPARENT; width * height];
    pixels.par_chunks_mut(width * BAND_ROWS).zip(&bands).enumerate().for_each(|(band, (rows, members))| {
        let (first_row, last_row) = (band * BAND_ROWS, band * BAND_ROWS + rows.len() / width - 1);
        let mut nearest = vec![0.0_f32; rows.len()];
        let mut seen: Vec<(u32, f32, f32)> = vec![(u32::MAX, 0.0, 0.0); rows.len()];
        for &index in members {
            let triangle = &triangles[index as usize];
            let [a, b, c] = triangle.points;
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let (min_x, max_x) = (a[0].min(b[0]).min(c[0]).floor(), a[0].max(b[0]).max(c[0]).ceil());
            let (min_y, max_y) = (a[1].min(b[1]).min(c[1]).floor().max(first_row as f32), a[1].max(b[1]).max(c[1]).ceil().min(last_row as f32));
            if area == 0.0 || !area.is_finite() || max_x < 0.0 || min_x >= width as f32 || min_y > max_y {
                continue;
            }
            for y in min_y as usize..=max_y as usize {
                let py = y as f32 + 0.5;
                for x in min_x.max(0.0) as usize..=(max_x as usize).min(width - 1) {
                    let px = x as f32 + 0.5;
                    let wa = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                    let wb = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                    let wc = 1.0 - wa - wb;
                    let at = (y - first_row) * width + x;
                    let inv_depth = wa * triangle.inv_depth[0] + wb * triangle.inv_depth[1] + wc * triangle.inv_depth[2];
                    if wa < 0.0 || wb < 0.0 || wc < 0.0 || inv_depth <= nearest[at] {
                        continue;
                    }
                    if triangle.cut_out && triangle.texel(triangle.shares([wa, wb, wc]))[3] < 127.5 {
                        continue;
                    }
                    nearest[at] = inv_depth;
                    seen[at] = (index, wa, wb);
                }
            }
        }
        for (at, (pixel, &(index, wa, wb))) in rows.iter_mut().zip(&seen).enumerate() {
            if let Some(triangle) = triangles.get(index as usize) {
                *pixel = triangle.colour(triangle.shares([wa, wb, 1.0 - wa - wb]), lights);
            }
            let over = ground.and_then(|ground| ground.colour((at % width) as f32 + 0.5, (first_row + at / width) as f32 + 0.5, lights)).filter(|(_, inv_depth)| *inv_depth > nearest[at]);
            if let Some((ink, _)) = over {
                let under = 1.0 - ink[3] / 255.0;
                *pixel = Color32::from_rgba_premultiplied((ink[0] + pixel.r() as f32 * under) as u8, (ink[1] + pixel.g() as f32 * under) as u8, (ink[2] + pixel.b() as f32 * under) as u8, (ink[3] + pixel.a() as f32 * under) as u8);
            }
        }
    });
    egui::ColorImage { size: [width, height], pixels }
}

pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    /// One per position; zero where the file gives the vertex none.
    pub uvs: Vec<[f32; 2]>,
    /// One per position, unit length; zero where the file gives the vertex none.
    pub normals: Vec<[f32; 3]>,
    /// One per position: the way the texture runs across, then the sign that turns normal x tangent to run down it; zero where the file gives none.
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Bone segments for skeletons.
    pub lines: Vec<(usize, usize)>,
    /// `(name, first triangle, triangle count)`.
    pub shapes: Vec<(String, usize, usize)>,
    /// The `.mat` each shape is drawn with, as a key into `Materials`, where the file names one.
    pub materials: Vec<Option<String>>,
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
        self.uvs.resize(self.positions.len(), [0.0, 0.0]);
        self.normals.resize(self.positions.len(), [0.0; 3]);
        self.tangents.resize(self.positions.len(), [0.0; 4]);
        self.materials.resize(self.shapes.len(), None);
        self
    }

    /// Gives each run of shapes its material, in the order a `.sm` lists them.
    pub fn set_materials<'a>(&mut self, runs: impl Iterator<Item = (Option<&'a str>, usize)>) {
        self.materials = runs.flat_map(|(material, shapes)| std::iter::repeat_n(material.and_then(material_key), shapes)).collect();
        self.materials.resize(self.shapes.len(), None);
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
    MeshData { positions: Vec::new(), uvs: Vec::new(), normals: Vec::new(), tangents: Vec::new(), indices: Vec::new(), lines: Vec::new(), shapes: Vec::new(), materials: Vec::new(), min: [0.0; 3], max: [0.0; 3] }
}

/// The key a material's texture is cached under, for a path that names a `.mat`.
pub fn material_key(path: &str) -> Option<String> {
    let key = crate::ui::links::normalize(path).to_ascii_lowercase();
    key.ends_with(".mat").then_some(key)
}

/// A direction as the files store one, each axis a byte around 127.5; zero when the bytes are not a direction.
fn direction(bytes: [u8; 3]) -> [f32; 3] {
    let v = bytes.map(|byte| (byte as f32 - 127.5) / 127.5);
    if dot(v, v) > 0.25 { norm(v) } else { [0.0; 3] }
}

/// A tangent with its fourth byte, which is above 127 when normal x tangent runs up the texture, not down it.
fn tangent(bytes: [u8; 4]) -> [f32; 4] {
    let [x, y, z] = direction([bytes[0], bytes[1], bytes[2]]);
    if [x, y, z] == [0.0; 3] { [0.0; 4] } else { [x, y, z, if bytes[3] > 127 { -1.0 } else { 1.0 }] }
}

fn push_vertices(out: &mut MeshData, vertices: &[crate::parsers::model::dolm::Vertex]) {
    out.uvs.resize(out.positions.len(), [0.0, 0.0]);
    out.normals.resize(out.positions.len(), [0.0; 3]);
    out.tangents.resize(out.positions.len(), [0.0; 4]);
    out.positions.extend(vertices.iter().map(|v| v.pos));
    out.uvs.extend(vertices.iter().map(|v| v.tex_coord0.unwrap_or([0.0, 0.0])));
    out.normals.extend(vertices.iter().map(|v| direction([v.normal[0] as u8, v.normal[1] as u8, v.normal[2] as u8])));
    out.tangents.extend(vertices.iter().map(|v| tangent(v.tangent.map(|byte| byte as u8))));
}

/// Adds the most detailed LOD and returns its shapes as `(first triangle, triangle count)`.
fn append_dolm(out: &mut MeshData, dolm: &Dolm) -> Vec<(usize, usize)> {
    let Some(lod) = dolm.lods.iter().max_by_key(|l| l.indices.len()) else { return Vec::new() };
    let base = out.positions.len() as u32;
    let first = out.indices.len() / 3;
    push_vertices(out, &lod.vertices);
    out.indices.extend(indices_of(&lod.indices).into_iter().map(|i| i + base));
    let total = out.indices.len() / 3 - first;
    lod.shape_extents
        .iter()
        .map(|ext| {
            let start = (ext.start_index as usize / 3).min(total);
            (first + start, (ext.count_index as usize / 3).min(total - start))
        })
        .collect()
}

/// Triangle ranges for shapes that record only where each one starts.
fn ranges_from_starts(starts: impl Iterator<Item = usize>, total: usize) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = starts.map(|s| s.min(total)).collect();
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| (start, starts.get(i + 1).copied().unwrap_or(total).clamp(start, total) - start))
        .collect()
}

/// Geometry the preview can draw, or `None` when the file has no vertices.
pub fn extract(model: &ModelFile) -> Option<MeshData> {
    let mut out = empty();
    match model {
        ModelFile::Fmt(f) => {
            // v9 shapes carry no triangle start; their ranges are the DOLm LOD's shape extents.
            let ranges = match &f.section {
                fmt::Section::V8(s) => {
                    out.positions.extend(s.vertex_buffer.iter().map(|v| v.pos));
                    out.uvs.extend(s.vertex_buffer.iter().map(|v| v.uv));
                    out.normals.extend(s.vertex_buffer.iter().map(|v| direction([v.unk[0], v.unk[1], v.unk[2]])));
                    out.tangents.extend(s.vertex_buffer.iter().map(|v| tangent([v.unk[4], v.unk[5], v.unk[6], v.unk[7]])));
                    out.indices = indices_of(&s.index_buffer);
                    ranges_from_starts(f.shapes.iter().map(|s| s.triangle_start as usize), out.indices.len() / 3)
                }
                fmt::Section::V9(d) => append_dolm(&mut out, d),
            };
            for (s, (start, count)) in f.shapes.iter().zip(ranges) {
                out.shapes.push((if s.name.is_empty() { s.material.clone() } else { s.name.clone() }, start, count));
                out.materials.push(material_key(&s.material));
            }
        }
        ModelFile::Tgm(t) => match &t.section {
            tgm::Section::V8(s) => {
                for (mi, m) in s.meshes.iter().enumerate() {
                    let base = out.positions.len() as u32;
                    let first = out.indices.len() / 3;
                    push_vertices(&mut out, &m.vertices);
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
                push_vertices(&mut out, &v2.vertex_buffer);
                out.indices = indices_of(&v2.index_buffer);
                let ranges = ranges_from_starts(v2.shape_extents.iter().map(|s| s.triangle_index as usize), out.indices.len() / 3);
                for (s, (start, count)) in v2.shape_extents.iter().zip(ranges) {
                    out.shapes.push((s.name.clone(), start, count));
                }
            }
            smd::Section::V3(v3) => {
                for (i, (start, count)) in append_dolm(&mut out, &v3.dolm).into_iter().enumerate() {
                    let name = v3.shape_names.get(i).cloned().unwrap_or_else(|| format!("shape {}", i));
                    out.shapes.push((name, start, count));
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
    pub textured: bool,
    pub lit: bool,
    /// The key light's yaw and pitch, around the model like the camera's.
    light: [f32; 2],
    pub shape: Option<usize>,
    /// The last picture drawn and the hash of everything it was drawn from.
    picture: Option<(u64, egui::TextureHandle)>,
    shadow: Option<(u64, Arc<ShadowMap>)>,
}

impl Default for MeshPreviewState {
    fn default() -> Self {
        Self { yaw: 0.6, pitch: 0.45, zoom: 1.0, pan: Vec2::ZERO, wireframe: false, cull: true, z_up: true, textured: true, lit: true, light: [-0.35, 0.95], shape: None, picture: None, shadow: None }
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

/// The way a yaw and a pitch look, down onto the model from above for a pitch over zero.
fn look(yaw: f32, pitch: f32, z_up: bool) -> [f32; 3] {
    let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    // The game's up is -Z: a model stands on z = 0 and rises into negative z.
    if z_up { norm([-cp * sy, cp * cy, sp]) } else { norm([-cp * sy, -sp, -cp * cy]) }
}

impl Camera {
    fn orbit(state: &MeshPreviewState, centre: [f32; 3], dist: f32, rect: Rect) -> Self {
        let fwd = look(state.yaw, state.pitch, state.z_up);
        let up_hint = if state.z_up { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
        let right = norm(cross(fwd, up_hint));
        let up = norm(cross(right, fwd));
        let eye = [centre[0] - fwd[0] * dist, centre[1] - fwd[1] * dist, centre[2] - fwd[2] * dist];
        let focal = rect.height() / (2.0 * (22.5_f32).to_radians().tan());
        Self { right, up, fwd, eye, focal, center: rect.center() + state.pan }
    }

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
    /// Materials that are not in `materials` yet are pushed to `wanted`.
    pub fn show(ui: &mut egui::Ui, id: u64, mesh: &MeshData, state: &mut MeshPreviewState, materials: &Materials, wanted: &mut Vec<String>) {
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
            if mesh.materials.iter().any(|m| m.is_some()) {
                ui.toggle_value(&mut state.textured, "Textures").on_hover_text("Draw each shape with its material's colour texture");
            }
            if !is_skeleton {
                ui.toggle_value(&mut state.lit, "Lit").on_hover_text("Light the model with its normal and gloss maps and cast a shadow; shift-drag moves the light");
            }
            ui.toggle_value(&mut state.z_up, "-Z up").on_hover_text("The game's own up axis; turn off for a model made Y-up");
            if ui.button("Reset view").clicked() {
                let keep = (state.wireframe, state.cull, state.z_up, state.textured, state.lit, state.shape);
                *state = MeshPreviewState::default();
                (state.wireframe, state.cull, state.z_up, state.textured, state.lit, state.shape) = keep;
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
            if state.lit && ui.input(|i| i.modifiers.shift) {
                state.light = [state.light[0] + d.x * 0.01, (state.light[1] + d.y * 0.01).clamp(0.1, 1.5)];
            } else {
                state.yaw += d.x * 0.01;
                state.pitch = (state.pitch + d.y * 0.01).clamp(-1.5, 1.5);
            }
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
        let cam = Camera::orbit(state, centre, dist, rect);
        let (fwd, right, up) = (cam.fwd, cam.right, cam.up);

        let projected: Vec<Option<(Pos2, f32)>> = mesh.positions.iter().map(|p| cam.project(*p)).collect();

        let grid_c = if dark { Color32::from_rgb(45, 45, 54) } else { Color32::from_rgb(210, 210, 220) };
        let floor = if state.z_up { mesh.max[2] } else { mesh.min[1] };
        let step = (radius * 2.0 / 8.0).max(1.0);

        if is_skeleton {
            // Bones are drawn as lines, not into a picture, so their ground is a patch of lines too.
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
        let lit = state.lit;
        let light = norm([fwd[0] * -0.4 + right[0] * 0.5 + up[0] * 0.75, fwd[1] * -0.4 + right[1] * 0.5 + up[1] * 0.75, fwd[2] * -0.4 + right[2] * 0.5 + up[2] * 0.75]);
        let shape_of = |tri: usize| mesh.shapes.iter().position(|(_, s, c)| tri >= *s && tri < s + c).unwrap_or(0);
        let shape_materials: Vec<Option<&Material>> = mesh
            .materials
            .iter()
            .map(|material| {
                let material = material.as_ref().filter(|_| state.textured)?;
                match materials.get(material) {
                    Some(known) => known.as_deref(),
                    None => {
                        wanted.push(material.clone());
                        None
                    }
                }
            })
            .collect();
        let hidden = |tri: usize| shape_materials.get(shape_of(tri)).copied().flatten().is_some_and(|m| m.hidden);
        // Below 1.5 pixels a point, edges are drawn at twice the size and averaged down by the sampler.
        let scale = ui.ctx().pixels_per_point() * if ui.ctx().pixels_per_point() < 1.5 { 2.0 } else { 1.0 };
        let (width, height) = ((rect.width() * scale).round().clamp(1.0, 4096.0) as usize, (rect.height() * scale).round().clamp(1.0, 4096.0) as usize);
        let to_raster = |p: Pos2| [(p.x - rect.left()) * scale, (p.y - rect.top()) * scale];
        let mut tris: Vec<ScreenTriangle> = Vec::with_capacity(total / stride + 1);
        let mut tri = tri_start;
        while tri < tri_end {
            let corners = [mesh.indices[tri * 3] as usize, mesh.indices[tri * 3 + 1] as usize, mesh.indices[tri * 3 + 2] as usize];
            if let ([Some(Some(a)), Some(Some(b)), Some(Some(c))], false) = (corners.map(|i| projected.get(i)), hidden(tri)) {
                let area = (b.0.x - a.0.x) * (c.0.y - a.0.y) - (b.0.y - a.0.y) * (c.0.x - a.0.x);
                if !(state.cull && area > 0.0) {
                    let world = corners.map(|i| mesh.positions[i]);
                    let [pa, pb, pc] = world;
                    let (ab, ac) = ([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]], [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]]);
                    let n = norm(cross(ab, ac));
                    // Winding does not say which side is out, so the face is taken to look at the camera, as every drawn one does.
                    let face = if dot(n, [cam.eye[0] - pa[0], cam.eye[1] - pa[1], cam.eye[2] - pa[2]]) < 0.0 { n.map(|axis| -axis) } else { n };
                    let normal = corners.map(|i| {
                        let own = mesh.normals[i];
                        match dot(own, face) {
                            agreement if agreement > 0.2 => own,
                            agreement if agreement < -0.2 => own.map(|axis| -axis),
                            _ => face,
                        }
                    });
                    let shape = shape_of(tri);
                    let uv = corners.map(|i| mesh.uvs[i]);
                    let (du, dv) = ([uv[1][0] - uv[0][0], uv[2][0] - uv[0][0]], [uv[1][1] - uv[0][1], uv[2][1] - uv[0][1]]);
                    let spread = du[0] * dv[1] - du[1] * dv[0];
                    let share = spread.abs() / (area.abs() * scale * scale).max(1e-6);
                    let material = shape_materials.get(shape).copied().flatten();
                    let colour = material.and_then(|m| m.colour.as_ref());
                    // A corner with no tangent of its own takes the triangle's, worked out from how its texture is laid over it.
                    let across = [0, 1, 2].map(|i| (ab[i] * dv[1] - ac[i] * dv[0]) / spread);
                    let down = [0, 1, 2].map(|i| (ac[i] * du[0] - ab[i] * du[1]) / spread);
                    let laid = if spread.abs() > 1e-12 { [across[0], across[1], across[2], dot(cross(face, across), down).signum()] } else { [0.0; 4] };
                    let tangent = corners.map(|i| if mesh.tangents[i][3] != 0.0 { mesh.tangents[i] } else { laid });
                    // A texture carries its own shading, so the plain light darkens it less than it does a plain colour.
                    let plain = shape_color(shape, dark);
                    let (tint, darkest) = match (colour, lit) {
                        (Some(_), _) => ([1.0; 3], 0.45),
                        (None, false) => ([plain.r(), plain.g(), plain.b()].map(|channel| channel as f32 / 255.0), 0.25),
                        (None, true) => ([plain.r(), plain.g(), plain.b()].map(|channel| LINEAR[channel as usize] / LINEAR[255]), 0.25),
                    };
                    tris.push(ScreenTriangle {
                        points: [to_raster(a.0), to_raster(b.0), to_raster(c.0)],
                        inv_depth: [1.0 / a.1, 1.0 / b.1, 1.0 / c.1],
                        uv,
                        shade: normal.map(|normal| darkest + (1.0 - darkest) * (dot(normal, light) * 0.5 + 0.5)),
                        tint,
                        texture: colour.map(|texture| texture.level(share)),
                        cut_out: colour.is_some_and(|texture| texture.cut_out),
                        world,
                        normal,
                        tangent,
                        surface: material.and_then(|m| m.surface.as_ref()).filter(|_| lit).map(|(map, packing)| (map.level(share), *packing)),
                    });
                }
            }
            tri += stride;
        }

        let lights = lit.then(|| {
            let key = look(state.light[0], state.light[1], state.z_up).map(|axis| -axis);
            let reach = (0..3).map(|k| (mesh.max[k] - mesh.min[k]).powi(2)).sum::<f32>().sqrt().max(1.0) * 0.55;
            // The shadow depends on the light and the shapes drawn, not on the camera, so orbiting reuses it.
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&(state.light.map(f32::to_bits), state.z_up, state.shape, stride, shape_materials.iter().map(|m| m.is_some_and(|m| m.hidden)).collect::<Vec<_>>()), &mut hasher);
            let cast = std::hash::Hasher::finish(&hasher);
            if state.shadow.as_ref().is_none_or(|(drawn, _)| *drawn != cast) {
                let casting = (tri_start..tri_end).step_by(stride).filter(|tri| !hidden(*tri)).map(|tri| [0, 1, 2].map(|corner| mesh.positions[mesh.indices[tri * 3 + corner] as usize]));
                state.shadow = Some((cast, Arc::new(ShadowMap::new(casting, key, centre, reach))));
            }
            let fill = norm([0, 1, 2].map(|i| -fwd[i] + right[i] * 0.6 + up[i] * 0.15));
            let world_up = if state.z_up { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
            Lights { key, fill, eye: cam.eye, up: world_up, shadow: state.shadow.as_ref().map(|(_, map)| map.clone()).expect("the shadow was just made") }
        });
        // A ray through each pixel: the one through the picture's corner, and how it turns a pixel across and a pixel down.
        let (reach, middle) = (cam.focal * scale, to_raster(cam.center));
        let ground = Ground {
            eye: cam.eye,
            corner: [0, 1, 2].map(|i| fwd[i] * reach - right[i] * middle[0] + up[i] * middle[1]),
            across: right,
            down: up.map(|axis| -axis),
            reach,
            axis: if state.z_up { 2 } else { 1 },
            level: floor,
            step,
            line: grid_c,
        };

        // The picture changes only when the view, the size, the light or the textures on hand do, so it is drawn once and kept.
        let mut hasher = std::hash::DefaultHasher::new();
        let view = [state.yaw, state.pitch, state.zoom, state.pan.x, state.pan.y, rect.width(), rect.height(), scale, state.light[0], state.light[1]];
        std::hash::Hash::hash(&(view.map(f32::to_bits), state.cull, state.z_up, state.shape, lit, dark, tris.len()), &mut hasher);
        std::hash::Hash::hash(&shape_materials.iter().map(|m| m.map_or(0, |m| m as *const Material as usize)).collect::<Vec<_>>(), &mut hasher);
        let key = std::hash::Hasher::finish(&hasher);
        if state.picture.as_ref().is_none_or(|(drawn, _)| *drawn != key) {
            let image = rasterize(width, height, &tris, lights.as_ref(), Some(&ground));
            match &mut state.picture {
                Some((drawn, texture)) => {
                    *drawn = key;
                    texture.set(image, egui::TextureOptions::LINEAR);
                }
                None => state.picture = Some((key, ui.ctx().load_texture(format!("mesh_preview:{}", id), image, egui::TextureOptions::LINEAR))),
            }
        }
        if let Some((_, texture)) = &state.picture {
            painter.image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }

        if state.wireframe {
            if tris.len() <= MAX_WIRE_TRIS {
                let wire = if dark { Color32::from_rgba_unmultiplied(255, 255, 255, 70) } else { Color32::from_rgba_unmultiplied(0, 0, 0, 70) };
                let mut segs = Vec::with_capacity(tris.len() * 3);
                for triangle in &tris {
                    let pts = triangle.points.map(|p| rect.left_top() + Vec2::new(p[0], p[1]) / scale);
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
    fn a_model_that_rises_into_negative_z_is_drawn_upright() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0));
        let cam = Camera::orbit(&MeshPreviewState::default(), [0.0, 0.0, -50.0], 300.0, rect);
        let (feet, _) = cam.project([0.0, 0.0, 0.0]).unwrap();
        let (head, _) = cam.project([0.0, 0.0, -100.0]).unwrap();
        assert!(head.y < feet.y, "the head ({}) should be above the feet ({}) on screen", head.y, feet.y);
    }

    #[test]
    fn materials_follow_the_runs_of_shapes_a_skinned_mesh_lists() {
        let mut mesh = empty();
        mesh.shapes = (0..4).map(|i| (format!("shape {}", i), i, 1)).collect();
        mesh.set_materials([(Some("Art/Textures/A.mat"), 2), (None, 1)].into_iter());
        assert_eq!(mesh.materials, vec![Some("art/textures/a.mat".to_string()), Some("art/textures/a.mat".to_string()), None, None]);
        assert_eq!(material_key("Art/Models/Thing.fmt"), None);
    }

    fn flat(points: [[f32; 2]; 3], depth: f32, tint: [f32; 3]) -> ScreenTriangle<'static> {
        ScreenTriangle { points, inv_depth: [1.0 / depth; 3], uv: [[0.0; 2]; 3], shade: [1.0; 3], tint, texture: None, cut_out: false, world: [[0.0; 3]; 3], normal: [[0.0, 0.0, -1.0]; 3], tangent: [[0.0; 4]; 3], surface: None }
    }

    #[test]
    fn the_nearer_triangle_covers_the_farther_whatever_the_order() {
        let cover = [[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]];
        let (near, far) = (flat(cover, 5.0, [1.0, 0.0, 0.0]), flat(cover, 9.0, [0.0, 0.0, 1.0]));
        for order in [[&near, &far], [&far, &near]] {
            let scene: Vec<ScreenTriangle> = order.iter().map(|t| flat(t.points, 1.0 / t.inv_depth[0], t.tint)).collect();
            let image = rasterize(40, 40, &scene, None, None);
            assert_eq!(image.pixels[5 * 40 + 5], Color32::from_rgb(255, 0, 0));
            assert_eq!(image.pixels[39 * 40 + 39], Color32::TRANSPARENT, "nothing covers the far corner");
        }
    }

    #[test]
    fn a_texel_under_half_alpha_shows_what_is_behind() {
        let mut image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]));
        image.put_pixel(0, 0, image::Rgba([0, 255, 0, 0]));
        let texture = MeshTexture::new(&image);
        assert_eq!(texture.levels.iter().map(|l| (l.width, l.height)).collect::<Vec<_>>(), vec![(2, 2), (1, 1)]);
        assert!(texture.cut_out);

        let cover = [[0.0, 0.0], [160.0, 0.0], [0.0, 160.0]];
        let uv = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        let leaf = ScreenTriangle { uv, texture: Some(&texture.levels[0]), cut_out: true, ..flat(cover, 1.0, [1.0; 3]) };
        let picture = rasterize(80, 80, &[flat(cover, 4.0, [0.0, 0.0, 1.0]), leaf], None, None);
        assert_eq!(picture.pixels[20 * 80 + 20], Color32::from_rgb(0, 0, 255), "the clear texel lets the triangle behind through");
        assert_eq!(picture.pixels[20 * 80 + 60], Color32::from_rgb(0, 255, 0));
    }

    #[test]
    fn directions_are_bytes_around_the_middle_of_their_range() {
        assert_eq!(direction([255, 128, 128]).map(|axis| (axis * 100.0).round()), [100.0, 0.0, 0.0]);
        assert_eq!(direction([128, 128, 0]).map(|axis| (axis * 100.0).round()), [0.0, 0.0, -100.0]);
        assert_eq!(direction([128, 128, 128]), [0.0; 3], "three middle bytes are no direction");
        assert_eq!((tangent([255, 128, 128, 255])[3], tangent([255, 128, 128, 1])[3]), (-1.0, 1.0));
    }

    #[test]
    fn the_ground_is_ruled_to_the_horizon_and_hidden_by_what_stands_on_it() {
        // A camera 100 above the ground at z = 0, looking straight down it; -Z is up.
        let ground = Ground { eye: [0.0, 0.0, -100.0], corner: [-50.0, -50.0, 100.0], across: [1.0, 0.0, 0.0], down: [0.0, 1.0, 0.0], reach: 100.0, axis: 2, level: 0.0, step: 10.0, line: Color32::WHITE };
        let (on_a_line, depth) = ground.colour(50.0, 50.0, None).unwrap();
        assert!(on_a_line[3] > 200.0 && (depth - 0.01).abs() < 1e-6, "{:?} at {}", on_a_line, depth);
        assert_eq!(ground.colour(55.0, 55.0, None).unwrap().0[3], 0.0, "the middle of a square is clear");
        assert!(Ground { corner: [-50.0, -50.0, -100.0], ..ground }.colour(50.0, 50.0, None).is_none(), "a ray that looks up never meets the ground");

        // A pixel is sampled at its middle, so the corner ray starts half a pixel out to put one on a line.
        let ground = Ground { corner: [-50.5, -50.5, 100.0], ..ground };
        let cover = [[0.0, 0.0], [200.0, 0.0], [0.0, 200.0]];
        let picture = rasterize(100, 100, &[flat(cover, 50.0, [1.0, 0.0, 0.0])], None, Some(&ground));
        assert_eq!(picture.pixels[50 * 100 + 50], Color32::from_rgb(255, 0, 0), "the triangle is nearer than the ground");
        let picture = rasterize(100, 100, &[], None, Some(&ground));
        assert!(picture.pixels[50 * 100 + 50].a() > 200 && picture.pixels[55 * 100 + 55].a() == 0);
    }

    #[test]
    fn a_shadow_falls_behind_what_stands_in_the_light() {
        let roof = [[-10.0, -10.0, -20.0], [10.0, -10.0, -20.0], [0.0, 10.0, -20.0]];
        let map = ShadowMap::new([roof].into_iter(), [0.0, 0.0, -1.0], [0.0, 0.0, -10.0], 30.0);
        assert_eq!(map.light([0.0, -2.0, 0.0]), 0.0, "the floor under the roof");
        assert_eq!(map.light([0.0, -2.0, -25.0]), 1.0, "above the roof");
        assert_eq!(map.light([25.0, 0.0, 0.0]), 1.0, "the floor beside it");
    }

    #[test]
    fn the_lit_picture_is_brighter_facing_the_key_light() {
        let map = Arc::new(ShadowMap::new(std::iter::empty(), [0.0, 0.0, -1.0], [0.0; 3], 10.0));
        let lights = Lights { key: [0.0, 0.0, -1.0], fill: [1.0, 0.0, 0.0], eye: [0.0, -50.0, -50.0], up: [0.0, 0.0, -1.0], shadow: map };
        let cover = [[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]];
        let facing = |normal: [f32; 3]| rasterize(40, 40, &[ScreenTriangle { normal: [normal; 3], ..flat(cover, 5.0, [0.5; 3]) }], Some(&lights), None).pixels[5 * 40 + 5];
        let (toward, away) = (facing([0.0, 0.0, -1.0]), facing([0.0, 0.0, 1.0]));
        assert!(toward.r() > away.r() + 60, "{:?} against {:?}", toward, away);
    }

    #[test]
    #[ignore]
    fn lit_picture_speed() {
        let texture = MeshTexture::new(&image::RgbaImage::from_fn(1024, 1024, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])));
        let (cells, side) = (84, 1000.0 / 84.0);
        let mut scene = Vec::new();
        for (row, col) in (0..cells).flat_map(|row| (0..cells).map(move |col| (row, col))) {
            let (x, y) = (300.0 + col as f32 * side, 100.0 + row as f32 * side);
            for points in [[[x, y], [x + side, y], [x, y + side]], [[x + side, y], [x + side, y + side], [x, y + side]]] {
                let (uv, world) = (points.map(|p| [p[0] / 1000.0, p[1] / 1000.0]), points.map(|p| [p[0] / 10.0, p[1] / 10.0, 0.0]));
                scene.push(ScreenTriangle { uv, world, texture: Some(&texture.levels[0]), surface: Some((&texture.levels[0], SurfaceMap::NormalGlossAo)), tangent: [[1.0, 0.0, 0.0, 1.0]; 3], ..flat(points, 5.0, [1.0; 3]) });
            }
        }
        let map = Arc::new(ShadowMap::new(scene.iter().map(|t| t.world), [0.0, 0.0, -1.0], [80.0, 60.0, 0.0], 120.0));
        let lights = Lights { key: [0.0, 0.0, -1.0], fill: [1.0, 0.0, 0.0], eye: [80.0, 60.0, -300.0], up: [0.0, 0.0, -1.0], shadow: map };
        let ground = Ground { eye: [0.0, 0.0, -100.0], corner: [-800.0, -600.0, 1000.0], across: [1.0, 0.0, 0.0], down: [0.0, 1.0, 0.0], reach: 1000.0, axis: 2, level: 0.0, step: 10.0, line: Color32::WHITE };
        for (name, lights) in [("plain", None), ("lit", Some(&lights))] {
            let start = std::time::Instant::now();
            (0..10).for_each(|_| drop(std::hint::black_box(rasterize(1600, 1200, &scene, lights, Some(&ground)))));
            println!("{}: {:.1} ms a picture, {} triangles over a million pixels", name, start.elapsed().as_secs_f64() * 100.0, scene.len());
        }
    }

    #[test]
    fn dolm_shapes_come_from_the_drawn_lod() {
        use crate::parsers::model::dolm::{Mesh, ShapeExtents};
        let lod = |tris: u16, extents: &[(u32, u32)]| Mesh {
            shape_extents: extents.iter().map(|&(start_index, count_index)| ShapeExtents { start_index, count_index }).collect(),
            indices: IndexBuffer::U16(vec![0; tris as usize * 3]),
            vertices: Vec::new(),
        };
        let dolm = Dolm {
            c0h: 0,
            vertex_format: 0,
            lod_extents: Vec::new(),
            lods: vec![lod(2, &[(0, 6)]), lod(5, &[(0, 6), (6, 9)])],
            extra_vformat_6: None,
            extra_vformat_6_c0h_2: None,
            extra_c0h_4: None,
        };
        let mut out = empty();
        out.indices = vec![0; 3];
        assert_eq!(append_dolm(&mut out, &dolm), vec![(1, 2), (3, 3)]);
        assert_eq!(out.triangle_count(), 6);
    }

    #[test]
    fn shape_starts_become_ranges() {
        assert_eq!(ranges_from_starts([0, 4, 4].into_iter(), 10), vec![(0, 4), (4, 0), (4, 6)]);
        assert_eq!(ranges_from_starts([0, 20].into_iter(), 10), vec![(0, 10), (10, 0)]);
    }

    #[test]
    fn matrix_multiply_is_row_major() {
        let t = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        let r = mul(&t, &t);
        assert_eq!(r[3], [2.0, 4.0, 6.0, 1.0]);
    }
}
