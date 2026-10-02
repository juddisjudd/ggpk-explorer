//! PoE 1 packs its interface art into sheets and names the rectangle each
//! image occupies in `Art/UIImages1.txt`, so a path out of the DAT tables —
//! `Art/2DArt/UIImages/InGame/PassiveSkillScreenPassiveFrameNormal` — is not a
//! file at all. PoE 2 ships those images as files, so this is only ever
//! loaded for a PoE 1 install.

use std::collections::HashMap;

/// Where one image sits: the sheet holding it and its pixel rectangle.
#[derive(Debug, Clone)]
pub struct AtlasEntry {
    pub sheet: String,
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl AtlasEntry {
    pub fn width(&self) -> u32 {
        self.x2.saturating_sub(self.x1)
    }

    pub fn height(&self) -> u32 {
        self.y2.saturating_sub(self.y1)
    }
}

#[derive(Debug, Default)]
pub struct UiAtlas {
    entries: HashMap<String, AtlasEntry>,
}

/// The descriptor holding the passive tree's art. The other files beside it
/// (`uips4`, `uixbox`, …) are console and shop art.
pub const ATLAS_PATH: &str = "art/uiimages1.txt";

impl UiAtlas {
    /// One image per line: `"<name>" "<sheet>" x1 y1 x2 y2`.
    pub fn parse(text: &str) -> Self {
        let mut entries = HashMap::new();
        for line in text.lines() {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let (Some(name), Some(sheet)) = (quoted.next(), quoted.next()) else { continue };
            let numbers: Vec<u32> = line
                .rsplit('"')
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            let [x1, y1, x2, y2] = numbers[..] else { continue };
            entries.insert(key(name), AtlasEntry { sheet: sheet.to_string(), x1, y1, x2, y2 });
        }
        Self { entries }
    }

    pub fn lookup(&self, path: &str) -> Option<&AtlasEntry> {
        self.entries.get(&key(path))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Names are matched without case or extension, as the tables spell them
/// either way.
fn key(path: &str) -> String {
    let path = path.trim();
    let path = path.strip_suffix(".dds").unwrap_or(path);
    path.to_ascii_lowercase().replace('\\', "/")
}

/// Cuts one image out of its sheet.
pub fn crop(sheet: &image::RgbaImage, entry: &AtlasEntry) -> Option<image::RgbaImage> {
    let (w, h) = (entry.width(), entry.height());
    if w == 0 || h == 0 || entry.x2 > sheet.width() || entry.y2 > sheet.height() {
        return None;
    }
    Some(image::imageops::crop_imm(sheet, entry.x1, entry.y1, w, h).to_image())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        "\"Art/2DArt/UIImages/Common/PanelBottom\" \"Art/Textures/Interface/2D/2DArt/UIImages/Common/1.dds\" 936 8 1905 323\n",
        "\"Art/2DArt/UIImages/InGame/PassiveSkillScreenPassiveFrameNormal\" \"Art/Textures/Interface/2D/2DArt/UIImages/InGame/4.dds\" 10 20 40 60\n",
    );

    #[test]
    fn an_entry_is_found_by_name_whatever_its_case_or_extension() {
        let atlas = UiAtlas::parse(SAMPLE);
        assert_eq!(atlas.len(), 2);
        let entry = atlas.lookup("art/2dart/uiimages/ingame/PassiveSkillScreenPassiveFrameNormal.dds").unwrap();
        assert_eq!(entry.sheet, "Art/Textures/Interface/2D/2DArt/UIImages/InGame/4.dds");
        assert_eq!((entry.x1, entry.y1, entry.width(), entry.height()), (10, 20, 30, 40));
        assert!(atlas.lookup("Art/2DArt/UIImages/Common/Missing").is_none());
    }

    #[test]
    fn a_line_without_a_full_rectangle_is_skipped() {
        let atlas = UiAtlas::parse("\"Art/Thing\" \"Sheet.dds\" 1 2 3\n\n\"bad line\n");
        assert!(atlas.is_empty());
    }
}
