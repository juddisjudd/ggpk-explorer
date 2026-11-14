//! The pixel operations PoB's GIMP scripts run, as GIMP 3 does them on an 8-bit image.

use image::{Rgba, RgbaImage};

fn to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// How much of each source pixel along one axis falls into each target pixel.
fn coverage(from: u32, to: u32) -> Vec<Vec<(usize, f64)>> {
    let step = from as f64 / to as f64;
    (0..to)
        .map(|o| {
            let (start, end) = (o as f64 * step, (o + 1) as f64 * step);
            (start.floor() as usize..(end.ceil() as usize).min(from as usize))
                .map(|i| (i, (end.min(i as f64 + 1.0) - start.max(i as f64)).max(0.0)))
                .filter(|(_, w)| *w > 0.0)
                .collect()
        })
        .collect()
}

/// The cubic B-spline GEGL's cubic sampler uses.
fn b_spline(t: f64) -> f64 {
    let t = t.abs();
    match t {
        t if t < 1.0 => (3.0 * t * t * t - 6.0 * t * t + 4.0) / 6.0,
        t if t < 2.0 => (2.0 - t).powi(3) / 6.0,
        _ => 0.0,
    }
}

/// The B-spline's weights for each target pixel, sampled at its centre with the edges clamped.
fn cubic(from: u32, to: u32) -> Vec<Vec<(usize, f64)>> {
    let step = from as f64 / to as f64;
    (0..to)
        .map(|o| {
            let centre = (o as f64 + 0.5) * step - 0.5;
            let base = centre.floor() as i64;
            let mut weights: Vec<(usize, f64)> = Vec::new();
            for i in base - 1..=base + 2 {
                let at = i.clamp(0, from as i64 - 1) as usize;
                let w = b_spline(centre - i as f64);
                match weights.iter_mut().find(|(j, _)| *j == at) {
                    Some(slot) => slot.1 += w,
                    None => weights.push((at, w)),
                }
            }
            weights
        })
        .collect()
}

/// GEGL box-filters a footprint wider than two source pixels and samples a narrower one with its B-spline.
fn weights(from: u32, to: u32) -> Vec<Vec<(usize, f64)>> {
    if from > to * 2 {
        coverage(from, to)
    } else {
        cubic(from, to)
    }
}

/// Shrinks an image the way GIMP's layer scale does, in linear light with alpha premultiplied.
pub fn downscale(img: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if img.dimensions() == (width, height) {
        return img.clone();
    }
    let (xs, ys) = (weights(img.width(), width), weights(img.height(), height));
    let lut: Vec<f64> = (0..=255u8).map(to_linear).collect();
    let mut out = RgbaImage::new(width, height);
    for (oy, rows) in ys.iter().enumerate() {
        for (ox, cols) in xs.iter().enumerate() {
            let mut sum = [0.0f64; 4];
            let mut area = 0.0;
            for &(y, wy) in rows {
                for &(x, wx) in cols {
                    let p = img.get_pixel(x as u32, y as u32).0;
                    let w = wx * wy;
                    let a = p[3] as f64 / 255.0 * w;
                    for c in 0..3 {
                        sum[c] += lut[p[c] as usize] * a;
                    }
                    sum[3] += a;
                    area += w;
                }
            }
            let alpha = sum[3] / area;
            let colour = |c: usize| if sum[3] > 0.0 { from_linear(sum[c] / sum[3]) } else { 0 };
            out.put_pixel(ox as u32, oy as u32, Rgba([colour(0), colour(1), colour(2), (alpha * 255.0).round() as u8]));
        }
    }
    out
}

fn hsl(p: [u8; 4]) -> (f64, f64, f64) {
    let [r, g, b] = [p[0], p[1], p[2]].map(|c| c as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = if l < 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    let a = s * l.min(1.0 - l);
    [0.0, 8.0, 4.0].map(|n: f64| {
        let k = (n + h * 12.0).rem_euclid(12.0);
        let c = l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

/// `gimp-drawable-hue-saturation` with only a saturation change: `keep` is what is left of each pixel's HSL saturation.
pub fn desaturate(img: &RgbaImage, keep: f64) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let [r, g, b] = [p[0], p[1], p[2]].map(|c| c as f64 / 255.0);
        let l = (r.max(g).max(b) + r.min(g).min(b)) / 2.0;
        let [r, g, b] = [r, g, b].map(|c| ((l + (c - l) * keep).clamp(0.0, 1.0) * 255.0).round() as u8);
        *p = Rgba([r, g, b, p[3]]);
    }
    out
}

/// `gimp-drawable-hue-saturation` with only a hue change, in degrees.
pub fn rotate_hue(img: &RgbaImage, degrees: f64) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let (h, s, l) = hsl(p.0);
        let [r, g, b] = rgb((h + degrees / 360.0).rem_euclid(1.0), s, l);
        *p = Rgba([r, g, b, p[3]]);
    }
    out
}

/// A layer in Screen mode at `opacity` merged onto `base`: blended in linear light and clipped to the base's alpha.
pub fn screen_onto(base: &RgbaImage, layer: &RgbaImage, opacity: f64) -> RgbaImage {
    let mut out = base.clone();
    for (p, top) in out.pixels_mut().zip(layer.pixels()) {
        let weight = top[3] as f64 / 255.0 * opacity;
        let blend = |c: usize| {
            let (b, t) = (to_linear(p[c]), to_linear(top[c]));
            from_linear(b * (1.0 - weight) + (1.0 - (1.0 - b) * (1.0 - t)) * weight)
        };
        *p = Rgba([blend(0), blend(1), blend(2), p[3]]);
    }
    out
}

/// Share of a pixel inside a circle, from a 4 by 4 grid of samples as an anti-aliased selection gives.
fn inside(x: u32, y: u32, cx: f64, cy: f64, radius: f64) -> f64 {
    let mut hits = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let (px, py) = (x as f64 + (sx as f64 + 0.5) / 4.0, y as f64 + (sy as f64 + 0.5) / 4.0);
            if (px - cx).powi(2) + (py - cy).powi(2) <= radius * radius {
                hits += 1;
            }
        }
    }
    hits as f64 / 16.0
}

/// Clears everything outside the ring between two circles centred on the bottom-right corner.
pub fn keep_ring(img: &mut RgbaImage, inner: f64, outer: f64) {
    let (cx, cy) = (img.width() as f64, img.height() as f64);
    for (x, y, p) in img.enumerate_pixels_mut() {
        if (x as f64 + 1.0 - cx).powi(2) + (y as f64 + 1.0 - cy).powi(2) > (outer + 2.0).powi(2) {
            p[3] = 0;
            continue;
        }
        let kept = inside(x, y, cx, cy, outer) - inside(x, y, cx, cy, inner);
        p[3] = (p[3] as f64 * kept).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_in_linear_light() {
        let img = RgbaImage::from_fn(2, 2, |x, _| if x == 0 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        assert_eq!(downscale(&img, 1, 1).get_pixel(0, 0).0, [188, 188, 188, 255]);
    }

    #[test]
    fn transparent_pixels_do_not_darken_their_neighbours() {
        let img = RgbaImage::from_fn(2, 1, |x, _| if x == 0 { Rgba([200, 100, 50, 255]) } else { Rgba([0, 0, 0, 0]) });
        assert_eq!(downscale(&img, 1, 1).get_pixel(0, 0).0, [200, 100, 50, 128]);
    }

    #[test]
    fn fractional_steps_weigh_partial_pixels() {
        let cover = coverage(3, 2);
        assert_eq!(cover[0], vec![(0, 1.0), (1, 0.5)]);
        assert_eq!(cover[1], vec![(1, 0.5), (2, 1.0)]);
    }

    #[test]
    fn desaturating_keeps_lightness() {
        let img = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 0, 255]));
        assert_eq!(desaturate(&img, 0.4).get_pixel(0, 0).0, [140, 100, 60, 255]);
        assert_eq!(desaturate(&img, 0.0).get_pixel(0, 0).0, [100, 100, 100, 255]);
    }

    #[test]
    fn a_full_turn_of_hue_changes_nothing() {
        let img = RgbaImage::from_pixel(1, 1, Rgba([30, 160, 220, 200]));
        assert_eq!(rotate_hue(&img, 360.0).get_pixel(0, 0).0, [30, 160, 220, 200]);
        assert_eq!(rotate_hue(&img, 120.0).get_pixel(0, 0).0, [220, 30, 160, 200]);
    }

    #[test]
    fn screen_leaves_the_base_alpha_alone() {
        let base = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 100]));
        let layer = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
        assert_eq!(screen_onto(&base, &layer, 1.0).get_pixel(0, 0).0, [255, 255, 255, 100]);
        assert_eq!(screen_onto(&base, &layer, 0.0).get_pixel(0, 0).0, [0, 0, 0, 100]);
    }

    #[test]
    fn a_ring_keeps_only_its_band() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        keep_ring(&mut img, 20.0, 40.0);
        assert_eq!(img.get_pixel(99, 99)[3], 0);
        assert_eq!(img.get_pixel(75, 75)[3], 255);
        assert_eq!(img.get_pixel(0, 0)[3], 0);
    }
}
