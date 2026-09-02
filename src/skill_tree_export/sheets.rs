//! Sprite sheets in the layout of the official export's `assets/` folder:
//! one `.webp` per sheet plus a `.json` listing every frame rectangle.

use super::json::J;
use super::TreeExportSource;
use image::imageops::FilterType;
use image::{Rgba, RgbaImage};
use std::collections::HashMap;
use std::path::Path;

/// Every official sheet is tagged with this scale: one sheet pixel is two
/// world units.
pub const SHEET_SCALE: f32 = 0.5;

/// Decoded textures by request path, fetched in bundle order so each bundle
/// is decompressed once.
pub struct TextureStore<'a> {
    source: &'a TreeExportSource,
    bytes: HashMap<String, Vec<u8>>,
    decoded: HashMap<String, Option<RgbaImage>>,
}

impl<'a> TextureStore<'a> {
    pub fn new(source: &'a TreeExportSource) -> Self {
        Self { source, bytes: HashMap::new(), decoded: HashMap::new() }
    }

    fn key(path: &str) -> String {
        path.to_ascii_lowercase()
    }

    /// Resolves and reads every path not yet held, grouped by bundle.
    pub fn prefetch(&mut self, paths: &[String]) {
        let mut wanted: Vec<(String, &crate::bundles::index::FileInfo)> = Vec::new();
        for p in paths {
            let key = Self::key(p);
            if p.is_empty() || self.bytes.contains_key(&key) || wanted.iter().any(|(k, _)| *k == key) {
                continue;
            }
            if let Some(info) = self.source.resolve_texture(p) {
                wanted.push((key, info));
            }
        }
        wanted.sort_by_key(|(_, info)| (info.bundle_index, info.file_offset));
        let mut current: Option<(u32, Vec<u8>)> = None;
        for (key, info) in wanted {
            let data = if info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL
                || info.bundle_index == crate::bundles::steam::LOOSE_FILE_SENTINEL
            {
                self.source.extract(info)
            } else {
                if current.as_ref().map(|(b, _)| *b != info.bundle_index).unwrap_or(true) {
                    current = self.source.decompress_bundle(info.bundle_index).map(|d| (info.bundle_index, d));
                }
                current.as_ref().and_then(|(_, data)| {
                    let start = info.file_offset as usize;
                    let end = start + info.file_size as usize;
                    (end <= data.len()).then(|| data[start..end].to_vec())
                })
            };
            if let Some(data) = data {
                self.bytes.insert(key, data);
            }
        }
    }

    pub fn get(&mut self, path: &str) -> Option<&RgbaImage> {
        let key = Self::key(path);
        if !self.decoded.contains_key(&key) {
            if !self.bytes.contains_key(&key) {
                self.prefetch(std::slice::from_ref(&path.to_string()));
            }
            let img = self.bytes.get(&key).and_then(|b| decode_full(b));
            self.decoded.insert(key.clone(), img);
        }
        self.decoded.get(&key).and_then(|i| i.as_ref())
    }
}

/// Decodes a DDS (or PNG/WebP) at full resolution.
fn decode_full(bytes: &[u8]) -> Option<RgbaImage> {
    let mut cursor = std::io::Cursor::new(bytes);
    if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
        if let Ok(img) = image_dds::image_from_dds(&dds, 0) {
            return Some(img);
        }
    }
    image::load_from_memory(bytes).ok().map(|i| i.to_rgba8())
}

pub fn resize(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width() == w && img.height() == h {
        return img.clone();
    }
    image::imageops::resize(img, w.max(1), h.max(1), FilterType::CatmullRom)
}

pub fn scale(img: &RgbaImage, factor: f32) -> RgbaImage {
    let w = (img.width() as f32 * factor).round() as u32;
    let h = (img.height() as f32 * factor).round() as u32;
    resize(img, w, h)
}

/// The "disabled" look of the official `skills-disabled` sheet: colour pulled
/// most of the way to grey and darkened.
pub fn disabled_icon(img: &RgbaImage) -> RgbaImage {
    map_pixels(img, |r, g, b| {
        let lum = 0.3 * r + 0.59 * g + 0.11 * b;
        (0.36 * r + 0.27 * lum, 0.36 * g + 0.27 * lum, 0.36 * b + 0.27 * lum)
    })
}

/// Mastery patterns go fully grey and much darker when inactive.
pub fn disabled_mastery(img: &RgbaImage) -> RgbaImage {
    map_pixels(img, |r, g, b| {
        let lum = (0.3 * r + 0.59 * g + 0.11 * b) * 0.3;
        (lum, lum, lum)
    })
}

fn map_pixels(img: &RgbaImage, f: impl Fn(f32, f32, f32) -> (f32, f32, f32)) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (nr, ng, nb) = f(r as f32, g as f32, b as f32);
        *p = Rgba([nr.clamp(0.0, 255.0) as u8, ng.clamp(0.0, 255.0) as u8, nb.clamp(0.0, 255.0) as u8, a]);
    }
    out
}

pub struct Frame {
    pub key: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Packed {
    pub image: RgbaImage,
    pub frames: Vec<Frame>,
}

/// Shelf packing in input order: sprites fill a row left to right and start
/// a new row when `max_width` would be exceeded.
pub fn pack(sprites: &[(String, RgbaImage)], max_width: u32) -> Packed {
    let mut frames = Vec::with_capacity(sprites.len());
    let (mut x, mut y, mut row_h, mut width) = (0u32, 0u32, 0u32, 0u32);
    for (key, img) in sprites {
        let (w, h) = (img.width(), img.height());
        if x > 0 && x + w > max_width {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        frames.push(Frame { key: key.clone(), x, y, w, h });
        x += w;
        row_h = row_h.max(h);
        width = width.max(x);
    }
    let height = y + row_h;
    let mut image = RgbaImage::new(width.max(1), height.max(1));
    for (frame, (_, img)) in frames.iter().zip(sprites) {
        image::imageops::overlay(&mut image, img, frame.x as i64, frame.y as i64);
    }
    Packed { image, frames }
}

/// Width that keeps a sheet roughly square, rounded up to a multiple of 8.
pub fn square_width(sprites: &[(String, RgbaImage)]) -> u32 {
    let area: u64 = sprites.iter().map(|(_, i)| i.width() as u64 * i.height() as u64).sum();
    let widest = sprites.iter().map(|(_, i)| i.width()).max().unwrap_or(1);
    let w = ((area as f64).sqrt() * 1.15) as u32;
    (w.max(widest).max(64) + 7) / 8 * 8
}

pub fn frames_json(packed: &Packed, image_name: &str) -> J {
    let mut frames = J::obj();
    for f in &packed.frames {
        let mut rect = J::obj();
        rect.set("x", J::Int(f.x as i64));
        rect.set("y", J::Int(f.y as i64));
        rect.set("w", J::Int(f.w as i64));
        rect.set("h", J::Int(f.h as i64));
        let mut entry = J::obj();
        entry.set("frame", rect);
        frames.set(&f.key, entry);
    }
    let mut size = J::obj();
    size.set("w", J::Int(packed.image.width() as i64));
    size.set("h", J::Int(packed.image.height() as i64));
    let mut meta = J::obj();
    meta.set("image", J::str(image_name));
    meta.set("scale", J::str(&SHEET_SCALE.to_string()));
    meta.set("size", size);
    let mut root = J::obj();
    root.set("frames", frames);
    root.set("meta", meta);
    root
}

pub fn encode_webp(img: &RgbaImage, quality: f32) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height());
    if quality <= 0.0 {
        encoder.encode_lossless().to_vec()
    } else {
        encoder.encode(quality.min(100.0)).to_vec()
    }
}

/// Packs, encodes and writes `<dir>/<name>.webp` + `<dir>/<name>.json`.
/// Returns the frames JSON for embedding in the viewer.
pub fn write_sheet(dir: &Path, name: &str, sprites: &[(String, RgbaImage)], max_width: u32, quality: f32) -> Result<J, String> {
    let packed = pack(sprites, max_width);
    let image_name = format!("{}.webp", name);
    let json = frames_json(&packed, &image_name);
    std::fs::write(dir.join(&image_name), encode_webp(&packed.image, quality)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", name)), super::json::to_string_pretty(&json)).map_err(|e| e.to_string())?;
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([255, 0, 0, 255]))
    }

    #[test]
    fn shelf_packing_wraps_rows() {
        let sprites = vec![
            ("a".to_string(), blank(40, 40)),
            ("b".to_string(), blank(40, 30)),
            ("c".to_string(), blank(40, 40)),
        ];
        let packed = pack(&sprites, 90);
        assert_eq!((packed.frames[0].x, packed.frames[0].y), (0, 0));
        assert_eq!((packed.frames[1].x, packed.frames[1].y), (40, 0));
        assert_eq!((packed.frames[2].x, packed.frames[2].y), (0, 40));
        assert_eq!((packed.image.width(), packed.image.height()), (80, 80));
        let json = frames_json(&packed, "x.webp");
        assert_eq!(json.get("meta").and_then(|m| m.get("scale")), Some(&J::str("0.5")));
    }

    #[test]
    fn disabled_icon_is_darker() {
        let img = blank(2, 2);
        let out = disabled_icon(&img);
        assert!(out.get_pixel(0, 0)[0] < 255 && out.get_pixel(0, 0)[1] > 0);
    }
}
