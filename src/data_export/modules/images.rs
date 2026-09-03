//! Writes the `.dds` art the dumps name out as `.png` and `.webp`, under the
//! export folder at the path the game uses. Only runs with `--images`.

use crate::data_export::Ctx;
use image::RgbaImage;

/// How a picture is assembled before it is written.
#[derive(Clone, Copy)]
pub enum Compose {
    /// Flask art ships as three panels side by side — fill, then the empty
    /// bottle over it, then the highlight — that the client stacks into one.
    Flask,
    /// One rectangle out of a shared UI sheet.
    Crop { x1: u32, y1: u32, x2: u32, y2: u32 },
}

/// Exports one texture, skipping the work when `--images` is off or the file
/// has already been written. Returns whether the art exists and was written.
pub fn export(ctx: &Ctx, dds_path: &str, compose: Option<Compose>) -> bool {
    if !ctx.options.images || dds_path.is_empty() {
        return false;
    }
    if !ctx.claim_image(dds_path) {
        return true; // already written this run
    }
    let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, dds_path) else {
        return false;
    };
    let Some(image) = decode(&bytes) else {
        eprintln!("images: {} is not readable as a texture", dds_path);
        return false;
    };
    let image = match compose {
        Some(Compose::Flask) => compose_flask(&image),
        Some(Compose::Crop { x1, y1, x2, y2 }) => crop(&image, x1, y1, x2, y2),
        None => image,
    };
    write(ctx, dds_path, &image)
}

/// Same as [`export`] but writing to a path of its own, for art pulled out of
/// a shared sheet.
pub fn export_as(ctx: &Ctx, dds_path: &str, destination: &str, compose: Option<Compose>) -> bool {
    if !ctx.options.images || dds_path.is_empty() || destination.is_empty() {
        return false;
    }
    if !ctx.claim_image(destination) {
        return true;
    }
    let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, dds_path) else {
        return false;
    };
    let Some(image) = decode(&bytes) else { return false };
    let image = match compose {
        Some(Compose::Flask) => compose_flask(&image),
        Some(Compose::Crop { x1, y1, x2, y2 }) => crop(&image, x1, y1, x2, y2),
        None => image,
    };
    write(ctx, destination, &image)
}

fn write(ctx: &Ctx, path: &str, image: &RgbaImage) -> bool {
    let stem = path.rsplit_once('.').map(|(head, _)| head).unwrap_or(path);
    let base = ctx.out.join(stem);
    if let Some(parent) = base.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let picture = image::DynamicImage::ImageRgba8(image.clone());
    let png = picture.save_with_format(base.with_extension("png"), image::ImageFormat::Png);
    let webp = picture.save_with_format(base.with_extension("webp"), image::ImageFormat::WebP);
    if let Err(e) = &png {
        eprintln!("images: could not write {}: {}", base.display(), e);
    }
    png.is_ok() && webp.is_ok()
}

fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    let mut cursor = std::io::Cursor::new(bytes);
    if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
        if let Ok(image) = image_dds::image_from_dds(&dds, 0) {
            return Some(image);
        }
    }
    image::load_from_memory(bytes).ok().map(|img| img.to_rgba8())
}

fn crop(image: &RgbaImage, x1: u32, y1: u32, x2: u32, y2: u32) -> RgbaImage {
    let width = x2.saturating_sub(x1).min(image.width().saturating_sub(x1));
    let height = y2.saturating_sub(y1).min(image.height().saturating_sub(y1));
    if width == 0 || height == 0 {
        return image.clone();
    }
    image::imageops::crop_imm(image, x1, y1, width, height).to_image()
}

/// Stacks the three panels of a flask sheet into the single picture the
/// client shows: fill at the back, then the bottle, then the highlight.
fn compose_flask(image: &RgbaImage) -> RgbaImage {
    let third = image.width() / 3;
    if third == 0 {
        return image.clone();
    }
    let panel = |n: u32| image::imageops::crop_imm(image, n * third, 0, third, image.height()).to_image();
    let mut out = panel(1);
    image::imageops::overlay(&mut out, &panel(2), 0, 0);
    image::imageops::overlay(&mut out, &panel(0), 0, 0);
    out
}

/// One entry of `Art/UIImages1.txt`: a rectangle of a shared sheet, with the
/// name the game refers to it by.
pub struct UiImage {
    pub source: String,
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

/// Parses the UI sheet index: `"name" "source.dds" x1 y1 x2 y2` per line.
pub fn parse_ui_images(text: &str) -> std::collections::HashMap<String, UiImage> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let mut quoted = line.split('"').skip(1).step_by(2);
        let (Some(name), Some(source)) = (quoted.next(), quoted.next()) else { continue };
        let numbers: Vec<u32> = line
            .rsplit('"')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if numbers.len() < 4 {
            continue;
        }
        out.insert(
            name.to_string(),
            UiImage {
                source: source.to_string(),
                x1: numbers[0],
                y1: numbers[1],
                // The index states an inclusive last pixel.
                x2: numbers[2] + 1,
                y2: numbers[3] + 1,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_sheet_lines_become_rectangles() {
        let sheet = parse_ui_images(
            "\"Art/2DArt/UIImages/Common/Background2\" \"Art/Textures/Background2.dds\" 0 0 1023 1023\nbroken line\n",
        );
        let entry = sheet.get("Art/2DArt/UIImages/Common/Background2").expect("parsed");
        assert_eq!(entry.source, "Art/Textures/Background2.dds");
        assert_eq!((entry.x1, entry.y1, entry.x2, entry.y2), (0, 0, 1024, 1024));
        assert_eq!(sheet.len(), 1);
    }

    #[test]
    fn a_flask_sheet_collapses_to_one_panel() {
        let sheet = RgbaImage::from_pixel(30, 10, image::Rgba([0, 0, 0, 0]));
        assert_eq!(compose_flask(&sheet).dimensions(), (10, 10));
    }
}
