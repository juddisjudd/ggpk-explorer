//! The images PoB loads from outside the tree folders: the legion sheets, class backgrounds, jewel radius art and rings.

use super::legion;
use crate::data_export::Ctx;
use crate::settings::Game;
use image::{GrayAlphaImage, LumaA, RgbaImage};
use std::collections::HashMap;
use std::path::Path;

/// The share of the game's art size PoB draws the tree at.
pub const ART_SCALE: f64 = 0.3835;

/// `TreeData/Background<attribute>.png`, the class illustrations behind each start, from `Art/2DArt/BaseClassIllustrations`.
const BACKGROUNDS: [&str; 6] = ["Str", "Dex", "Int", "StrDex", "StrInt", "DexInt"];

/// The game's own jewel radius ring, which `Assets/Shaded*Ring.png` stand in for.
const RADIUS_RING: &str = "Art/2DArt/UIImages/InGame/PassiveSkillScreenJewelCircle1";

/// `Assets/ring.png` is PoB's own drawing: a ring this wide at the image's edge, shaded by distance from the centre.
const RING: (u32, f64, f64) = (1024, 507.75, 512.0);
const RING_SHADE: [(f64, f64); 9] = [
    (507.6, 60.0),
    (508.1, 126.0),
    (508.6, 191.0),
    (509.1, 218.0),
    (509.9, 249.0),
    (510.6, 220.0),
    (511.1, 196.0),
    (511.6, 182.0),
    (512.6, 174.0),
];

pub fn write(ctx: &Ctx) -> Result<(), String> {
    let mut art = Art::new(ctx);
    if crate::pob_export::game(ctx) == Game::Poe1 {
        legion::write(ctx, &mut art, &ctx.out.join("legion"))?;
        backgrounds(&mut art, ctx.out)?;
        jewel_circles(ctx, &mut art, ctx.out)?;
    }
    rings(&mut art, &ctx.out.join("Assets"))
}

fn backgrounds(art: &mut Art, dir: &Path) -> Result<(), String> {
    for attribute in BACKGROUNDS {
        let image = art.texture(&format!("Art/2DArt/BaseClassIllustrations/{}.dds", attribute))?;
        let (w, h) = (scaled(image.width()), scaled(image.height()));
        save(&super::pixels::downscale(&image, w, h), &dir.join(format!("Background{}.png", attribute)))?;
    }
    Ok(())
}

/// Every ring a timeless jewel draws, at the game's size, named as the game names it.
fn jewel_circles(ctx: &Ctx, art: &mut Art, dir: &Path) -> Result<(), String> {
    let radii = ctx.table("PassiveJewelRadiiArt")?;
    let mut names: Vec<String> = Vec::new();
    for row in radii.rows() {
        for column in ["Circle1", "Circle2"] {
            let name = row.string(column);
            // PoB draws the plain ring from Assets/ instead.
            if !name.is_empty() && !name.eq_ignore_ascii_case(RADIUS_RING) && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    for name in names {
        let file = format!("{}.png", name.rsplit('/').next().unwrap_or(&name));
        save(&art.ui(&name)?, &dir.join(file))?;
    }
    Ok(())
}

/// PoB draws a jewel's inner and outer radius with these, each mirrored top to bottom as a second layer.
fn rings(art: &mut Art, dir: &Path) -> Result<(), String> {
    for (base, image) in [("ShadedOuterRing", art.ui(RADIUS_RING)?), ("ShadedInnerRing", art.ui(&format!("{}inverse", RADIUS_RING))?)] {
        save(&image, &dir.join(format!("{}.png", base)))?;
        save(&image::imageops::flip_vertical(&image), &dir.join(format!("{}Flipped.png", base)))?;
    }
    let path = dir.join("ring.png");
    ring().save_with_format(&path, image::ImageFormat::Png).map_err(|e| format!("{}: {}", path.display(), e))
}

fn ring() -> GrayAlphaImage {
    let (size, inner, outer) = RING;
    let centre = size as f64 / 2.0;
    GrayAlphaImage::from_fn(size, size, |x, y| {
        let mut covered = 0;
        for s in 0..16 {
            let (px, py) = (x as f64 + (s % 4) as f64 / 4.0 + 0.125, y as f64 + (s / 4) as f64 / 4.0 + 0.125);
            let d = (px - centre).hypot(py - centre);
            covered += (d >= inner && d <= outer) as u32;
        }
        let d = (x as f64 + 0.5 - centre).hypot(y as f64 + 0.5 - centre);
        LumaA([shade(d), (covered as f64 * 255.0 / 16.0).round() as u8])
    })
}

fn shade(distance: f64) -> u8 {
    let first = RING_SHADE[0];
    let last = RING_SHADE[RING_SHADE.len() - 1];
    if distance <= first.0 {
        return first.1 as u8;
    }
    for pair in RING_SHADE.windows(2) {
        let ((d0, v0), (d1, v1)) = (pair[0], pair[1]);
        if distance <= d1 {
            return (v0 + (v1 - v0) * (distance - d0) / (d1 - d0)).round() as u8;
        }
    }
    last.1 as u8
}

/// `round(size * 0.3835)` as Lua's `math.floor(x + 0.5)`.
pub fn scaled(size: u32) -> u32 {
    (size as f64 * ART_SCALE + 0.5).floor() as u32
}

pub fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    image.save_with_format(path, image::ImageFormat::Png).map_err(|e| format!("{}: {}", path.display(), e))
}

/// Game textures and interface images, each sheet decoded once.
pub struct Art<'a> {
    ctx: &'a Ctx<'a>,
    textures: HashMap<String, RgbaImage>,
}

impl<'a> Art<'a> {
    pub fn new(ctx: &'a Ctx<'a>) -> Self {
        Self { ctx, textures: HashMap::new() }
    }

    pub fn texture(&mut self, path: &str) -> Result<RgbaImage, String> {
        let key = path.to_ascii_lowercase();
        if !self.textures.contains_key(&key) {
            let image = super::orbits::decode(self.ctx, path)?;
            self.textures.insert(key.clone(), image);
        }
        Ok(self.textures[&key].clone())
    }

    /// An interface image by its game name: cut from its sheet in PoE 1, a file of its own in PoE 2.
    pub fn ui(&mut self, name: &str) -> Result<RgbaImage, String> {
        let images = self.ctx.ui_images();
        let entry = images.get(name).or_else(|| images.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v));
        let Some(entry) = entry else {
            let file = format!("Art/Textures/Interface/2D/{}.dds", name.strip_prefix("Art/").unwrap_or(name));
            return self.texture(&file).map_err(|_| format!("{} is neither in Art/UIImages1.txt nor a file", name));
        };
        let sheet = self.texture(&entry.source)?;
        if entry.x2 > sheet.width() || entry.y2 > sheet.height() || entry.x2 <= entry.x1 || entry.y2 <= entry.y1 {
            return Err(format!("{} lies outside {}", name, entry.source));
        }
        Ok(image::imageops::crop_imm(&sheet, entry.x1, entry.y1, entry.x2 - entry.x1, entry.y2 - entry.y1).to_image())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_like_lua() {
        assert_eq!(scaled(152), 58);
        assert_eq!(scaled(153), 59);
        assert_eq!(scaled(218), 84);
        assert_eq!(scaled(222), 85);
    }

    #[test]
    fn the_ring_touches_the_edge_and_is_empty_inside() {
        let ring = ring();
        assert_eq!(ring.dimensions(), (1024, 1024));
        assert_eq!(ring.get_pixel(0, 512)[1], 255);
        assert_eq!(ring.get_pixel(512, 512)[1], 0);
        assert_eq!(ring.get_pixel(20, 20)[1], 0);
        assert!(shade(509.9) > shade(507.6));
    }
}
