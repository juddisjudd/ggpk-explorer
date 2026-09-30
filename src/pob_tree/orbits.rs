//! The orbit arc PNGs PoB-PoE2 draws connections with, cut from the game's
//! connection art the way `Tree/GimpBatch/extract_lines.scm` cuts them.
//!
//! A connection texture holds a straight connector across the top and nine
//! quarter-circle arcs sharing the bottom-right corner as centre; its mask
//! draws the same shapes lit on black. The connector is cropped as a plain
//! rectangle between the black above and below it. Each arc keeps only the
//! pixels of its own ring in the mask and is cropped from two pixels inside
//! the ring's bounds to the texture's corner, outermost arc first. The script
//! grows the black around the innermost ring into it, so that ring loses its
//! outer pixels. The sizes come out as PoB's; the pixels at a ring's soft
//! edge do not, since GIMP's anti-aliased selections are not reproduced.

use crate::data_export::Ctx;
use image::RgbaImage;
use std::path::Path;

/// A mask pixel is black to GIMP's fuzzy select when no channel, alpha
/// included, is this far from opaque black. Its anti-aliased selection
/// reaches one and a half times the default threshold of 15.
const BLACK: u8 = 22;

/// How far GIMP's `gimp-selection-grow 4` reaches: a disc of radius 4.
const GROW: i32 = 4;

const ARCS: usize = 9;

/// `tree.assets` for one art and state: the connector, then orbits 1 to 9,
/// each naming the PNG its arc was saved as.
pub fn asset_names(art: &str, postfix: &str, basename: &str) -> Vec<(String, String)> {
    let mut curve = 9;
    (0..10)
        .map(|i| {
            let (name, file) = match i {
                0 => (format!("{}LineConnector{}", art, postfix), 0),
                3 => {
                    curve -= 1;
                    let file = curve;
                    curve -= 1;
                    (format!("{}Orbit{}{}", art, i, postfix), file)
                }
                7 => (format!("{}Orbit{}{}", art, i, postfix), 7),
                _ => {
                    let file = curve;
                    curve -= 1;
                    (format!("{}Orbit{}{}", art, i, postfix), file)
                }
            };
            (name, format!("{}{}.png", basename, file))
        })
        .collect()
}

/// Writes `<basename>0.png` (the connector) to `<basename>9.png` (the
/// innermost arc) into `dir`.
pub fn write(ctx: &Ctx, texture: &str, mask: &str, dir: &Path, basename: &str) -> Result<(), String> {
    let texture = decode(ctx, texture)?;
    let mask = decode(ctx, mask)?;
    if texture.dimensions() != mask.dimensions() {
        return Err(format!("{} and its mask differ in size", basename));
    }
    for (i, image) in cut(&texture, &mask)?.into_iter().enumerate() {
        let path = dir.join(format!("{}{}.png", basename, i));
        image.save_with_format(&path, image::ImageFormat::Png).map_err(|e| format!("{}: {}", path.display(), e))?;
    }
    Ok(())
}

fn decode(ctx: &Ctx, path: &str) -> Result<RgbaImage, String> {
    let bytes = crate::dat::relational::FileSource::fetch(ctx.files, path).ok_or_else(|| format!("{} is not in this install", path))?;
    let dds = ddsfile::Dds::read(&mut std::io::Cursor::new(&bytes)).map_err(|e| format!("{}: {}", path, e))?;
    image_dds::image_from_dds(&dds, 0).map_err(|e| format!("{}: {}", path, e))
}

/// Bounds of a set of pixels: min x, min y, max x, max y.
type Bounds = (u32, u32, u32, u32);

/// 4-connected regions of pixels for which `inside` holds, with their bounds.
fn regions(width: u32, height: u32, inside: impl Fn(u32, u32) -> bool) -> (Vec<u32>, Vec<Bounds>) {
    let mut label = vec![u32::MAX; (width * height) as usize];
    let mut bounds = Vec::new();
    let mut stack = Vec::new();
    for start in 0..width * height {
        let (sx, sy) = (start % width, start / width);
        if label[start as usize] != u32::MAX || !inside(sx, sy) {
            continue;
        }
        let id = bounds.len() as u32;
        let mut b = (sx, sy, sx, sy);
        label[start as usize] = id;
        stack.push((sx, sy));
        while let Some((x, y)) = stack.pop() {
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            let neighbours = [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ];
            for (nx, ny) in neighbours {
                if nx >= width || ny >= height {
                    continue;
                }
                let at = (ny * width + nx) as usize;
                if label[at] == u32::MAX && inside(nx, ny) {
                    label[at] = id;
                    stack.push((nx, ny));
                }
            }
        }
        bounds.push(b);
    }
    (label, bounds)
}


fn cut(texture: &RgbaImage, mask: &RgbaImage) -> Result<Vec<RgbaImage>, String> {
    let (width, height) = mask.dimensions();
    let black = |x: u32, y: u32| {
        let p = mask.get_pixel(x, y).0;
        p[0].max(p[1]).max(p[2]).max(255 - p[3]) < BLACK
    };
    let (black_labels, bands) = regions(width, height, black);
    let top = black_labels[0];
    if top == u32::MAX {
        return Err("the orbit mask does not start on black".into());
    }
    let connector_top = bands[top as usize].3 + 1;
    let below: Vec<&Bounds> = bands.iter().enumerate().filter(|(i, _)| *i as u32 != top).map(|(_, b)| b).collect();
    let left = below.iter().map(|b| b.0).min().ok_or("nothing below the connector in the orbit mask")?;
    let right = below.iter().map(|b| b.2).max().unwrap_or(width - 1) + 1;
    let connector_bottom = below.iter().map(|b| b.1).min().unwrap_or(connector_top);
    let mut out =
        vec![image::imageops::crop_imm(texture, left, connector_top, right - left, connector_bottom - connector_top).to_image()];

    let (labels, shapes) = regions(width, height, |x, y| mask.get_pixel(x, y).0[3] > 0 && !black(x, y));
    let mut arcs: Vec<u32> = (0..shapes.len() as u32).filter(|&i| shapes[i as usize].1 >= connector_bottom).collect();
    arcs.sort_by_key(|&i| shapes[i as usize].0);
    if arcs.len() != ARCS {
        return Err(format!("expected {} orbit arcs in the mask, found {}", ARCS, arcs.len()));
    }
    for (n, &arc) in arcs.iter().enumerate() {
        let (x0, y0, x1, y1) = shapes[arc as usize];
        let in_ring = |x: u32, y: u32| labels[(y * width + x) as usize] == arc;
        let mut ring: Vec<(u32, u32)> =
            (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).filter(|&(x, y)| in_ring(x, y)).collect();
        if n + 1 == ARCS {
            ring.retain(|&(x, y)| !near_outside(x, y, width, height, &in_ring));
        }
        let min_x = ring.iter().map(|p| p.0).min().ok_or("an orbit arc vanished")?;
        let min_y = ring.iter().map(|p| p.1).min().ok_or("an orbit arc vanished")?;
        let (ox, oy) = ((min_x + 2).min(width - 1), (min_y + 2).min(height - 1));
        let mut image = RgbaImage::new(width - ox, height - oy);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = *texture.get_pixel(x + ox, y + oy);
            pixel.0[3] = 0;
        }
        for (x, y) in ring {
            if x >= ox && y >= oy && x < width - 1 && y < height - 1 {
                image.get_pixel_mut(x - ox, y - oy).0[3] = texture.get_pixel(x, y).0[3];
            }
        }
        out.push(image);
    }
    Ok(out)
}

/// Whether a ring pixel lies within GIMP's four-pixel selection growth of
/// anything outside the ring.
fn near_outside(x: u32, y: u32, width: u32, height: u32, in_ring: &impl Fn(u32, u32) -> bool) -> bool {
    for dy in -3i32..=3 {
        for dx in -3i32..=3 {
            if dx * dx + dy * dy >= GROW * GROW {
                continue;
            }
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 || !in_ring(nx as u32, ny as u32) {
                return true;
            }
        }
    }
    false
}
