//! `TreeData/legion/`: the timeless jewel icons and Abyss art, laid out as PoB's `Export/Scripts/legionSprites.lua` lays them out.

use super::assets::{save, scaled, Art};
use super::pixels;
use crate::data_export::Ctx;
use image::RgbaImage;
use std::fmt::Write as _;
use std::path::Path;

const SHEET_WIDTH: u32 = 512;

/// Icon sizes by the kind the script sorts an alternate passive into.
const NORMAL: u32 = 27;
const NOTABLE: u32 = 38;
const KEYSTONE: u32 = 64;

/// Each orbit's ring on the connection art, as radii from its bottom-right corner, and its drawn size when normal and when active.
const ORBITS: [(f64, f64, u32, u32); 6] = [
    (40.0, 123.0, 33, 39),
    (123.0, 207.0, 64, 70),
    (292.0, 411.0, 130, 136),
    (411.0, 573.0, 191, 197),
    (573.0, 748.0, 256, 262),
    (748.0, 958.0, 322, 329),
];

/// The straight connector is this strip across the top of the connection art, drawn at this size.
const LINE_STRIP: (u32, u32) = (10, 60);
const LINE_SIZE: (u32, u32) = (368, 13);

/// Frame columns of `PassiveSkillTreeNodeFrameArt` and the state names PoB gives them.
const FRAME_STATES: [(&str, &str); 3] = [("Normal", "Unallocated"), ("CanAllocate", "CanAllocate"), ("Active", "Allocated")];

struct Entry {
    name: String,
    image: RgbaImage,
    x: u32,
    y: u32,
}

impl Entry {
    fn new(name: String, image: RgbaImage) -> Self {
        Self { name, image, x: 0, y: 0 }
    }
}

pub fn write(ctx: &Ctx, art: &mut Art, dir: &Path) -> Result<(), String> {
    let (mut icons, sections) = icons(ctx, art)?;
    let (icon_width, icon_height) = pack(&mut icons);
    let active = sheet(&icons, icon_width, icon_height);
    save(&active, &dir.join("legion-active.png"))?;
    save(&pixels::desaturate(&active, 0.4), &dir.join("legion-inactive.png"))?;

    let abyss = AbyssArt::read(ctx)?;
    let mut entries = Vec::new();
    for (row, prefix) in [(&abyss.notable, "AbyssNotableFrame"), (&abyss.keystone, "AbyssKeystoneFrame"), (&abyss.ascendancy, "AbyssAscendancyFrame")] {
        for (column, state) in FRAME_STATES {
            let image = art.ui(&row[column])?;
            let (w, h) = (scaled(image.width()), scaled(image.height()));
            entries.push(Entry::new(format!("{}{}", prefix, state), pixels::downscale(&image, w, h)));
        }
    }
    let normal = art.texture(&abyss.normal)?;
    let preview = pixels::screen_onto(&normal, &pixels::rotate_hue(&art.texture(&abyss.highlight)?, -135.0), 0.55);
    for (state, source) in [("Normal", normal), ("Intermediate", preview), ("Active", art.texture(&abyss.active)?)] {
        if source.width() != source.height() || source.height() < LINE_STRIP.0 + LINE_STRIP.1 {
            return Err(format!("the Abyss connection art is {}x{}, not the square the orbit cuts expect", source.width(), source.height()));
        }
        let strip = image::imageops::crop_imm(&source, 0, LINE_STRIP.0, source.width(), LINE_STRIP.1).to_image();
        let line = pixels::downscale(&strip, LINE_SIZE.0, LINE_SIZE.1);
        save(&line, &dir.join(format!("abyss-line-connector-{}.png", state.to_ascii_lowercase())))?;
        for (i, (inner, outer, normal_size, active_size)) in ORBITS.into_iter().enumerate() {
            let size = if state == "Active" { active_size } else { normal_size };
            let crop = (size as f64 / super::assets::ART_SCALE).round() as u32;
            let corner = source.width().checked_sub(crop).ok_or("the Abyss connection art is smaller than its outer orbit")?;
            let mut cut = image::imageops::crop_imm(&source, corner, corner, crop, crop).to_image();
            pixels::keep_ring(&mut cut, inner, outer);
            entries.push(Entry::new(format!("AbyssOrbit{}{}", i + 1, state), pixels::downscale(&cut, size, size)));
        }
    }
    let (art_width, art_height) = pack(&mut entries);
    save(&sheet(&entries, art_width, art_height), &dir.join("legion-art.png"))?;

    let path = dir.join("tree-legion.lua");
    std::fs::write(&path, lua(&icons, &sections, &entries)).map_err(|e| format!("{}: {}", path.display(), e))
}

/// Every alternate passive's icon once, scaled to its kind's size, and the icon names of each kind.
fn icons(ctx: &Ctx, art: &mut Art) -> Result<(Vec<Entry>, [Vec<String>; 3]), String> {
    let passives = ctx.table("AlternatePassiveSkills")?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut sections: [Vec<String>; 3] = Default::default();
    for row in passives.rows() {
        let icon = row.string("DDSIcon");
        if icon.is_empty() || entries.iter().any(|e| e.name == icon) {
            continue;
        }
        let kinds = row.list_int("PassiveType");
        let (section, size) = if kinds.contains(&4) {
            (2, KEYSTONE)
        } else if kinds.contains(&3) || kinds.contains(&5) {
            (1, NOTABLE)
        } else {
            (0, NORMAL)
        };
        let image = pixels::downscale(&art.texture(&icon)?, size, size);
        sections[section].push(icon.clone());
        entries.push(Entry::new(icon, image));
    }
    Ok((entries, sections))
}

/// The Abyss jewels' frames and connection art: `PassiveSkillTreeUIArt` through `AlternateTreeArt`, which every Abyss version must share.
struct AbyssArt {
    notable: std::collections::HashMap<String, String>,
    keystone: std::collections::HashMap<String, String>,
    ascendancy: std::collections::HashMap<String, String>,
    normal: String,
    highlight: String,
    active: String,
}

impl AbyssArt {
    fn read(ctx: &Ctx) -> Result<Self, String> {
        let (tree_art, versions, ui_art) = (ctx.table("AlternateTreeArt")?, ctx.table("AlternateTreeVersions")?, ctx.table("PassiveSkillTreeUIArt")?);
        let (frames, connections) = (ctx.table("PassiveSkillTreeNodeFrameArt")?, ctx.table("PassiveSkillTreeConnectionArt")?);
        let mut ui = None;
        for row in tree_art.rows() {
            let Some(version) = row.key("TreeVersion").and_then(|v| versions.row(v)) else { continue };
            if !version.str("Id").starts_with("Abyss") {
                continue;
            }
            let art = row.key("UIArt").ok_or_else(|| format!("Abyss version {} has no UIArt", version.str("Id")))?;
            if ui.is_some_and(|seen| seen != art) {
                return Err("the Abyss jewels no longer share one art set".to_string());
            }
            ui = Some(art);
        }
        let ui = ui.and_then(|i| ui_art.row(i)).ok_or("no AlternateTreeArt row names Abyss art")?;
        let frame = |row: Option<crate::dat::relational::Row<'_>>, what: &str| {
            let row = row.ok_or_else(|| format!("{} has no {} frame", ui.str("Id"), what))?;
            Ok::<_, String>(FRAME_STATES.iter().map(|(column, _)| (column.to_string(), row.string(column))).collect())
        };
        // No column names the ascendancy frame since 3.30; the game's own row is named after the art set.
        let ascendancy_id = format!("{}AscendancyNotable", ui.str("Id"));
        let connection = ui.key("ConnectionArt").and_then(|c| connections.row(c)).ok_or("the Abyss art has no connection art")?;
        Ok(Self {
            notable: frame(ui.key("NotableFrame").and_then(|f| frames.row(f)), "notable")?,
            keystone: frame(ui.key("KeystoneFrame").and_then(|f| frames.row(f)), "keystone")?,
            ascendancy: frame(frames.by_id(&ascendancy_id), "ascendancy")?,
            normal: connection.string("Normal"),
            highlight: connection.string("Active"),
            active: connection.string("Mask"),
        })
    }
}

/// `packEntries`: by height, then width, then name, in rows of one height no wider than the sheet.
fn pack(entries: &mut [Entry]) -> (u32, u32) {
    entries.sort_by(|a, b| {
        (a.image.height(), a.image.width(), a.name.to_ascii_lowercase()).cmp(&(b.image.height(), b.image.width(), b.name.to_ascii_lowercase()))
    });
    let (mut x, mut y, mut row_height, mut used_width) = (0, 0, 0, 0);
    for entry in entries.iter_mut() {
        let (w, h) = entry.image.dimensions();
        if x > 0 && (x + w > SHEET_WIDTH || h != row_height) {
            x = 0;
            y += row_height;
            row_height = 0;
        }
        entry.x = x;
        entry.y = y;
        x += w;
        row_height = row_height.max(h);
        used_width = used_width.max(x);
    }
    (used_width, y + row_height)
}

fn sheet(entries: &[Entry], width: u32, height: u32) -> RgbaImage {
    let mut out = RgbaImage::new(width.max(1), height.max(1));
    for entry in entries {
        image::imageops::replace(&mut out, &entry.image, entry.x as i64, entry.y as i64);
    }
    out
}

fn coord_line(out: &mut String, entry: &Entry) {
    let (w, h) = entry.image.dimensions();
    let _ = writeln!(out, "\t\t\t[\"{}\"] = {{ [\"x\"] = {}, [\"y\"] = {}, [\"w\"] = {}, [\"h\"] = {} }},", entry.name, entry.x, entry.y, w, h);
}

/// `tree-legion.lua` as the script writes it, apart from its first line.
fn lua(icons: &[Entry], sections: &[Vec<String>; 3], art: &[Entry]) -> String {
    let mut out = String::from("-- Written by ggpk-explorer export-tree in the layout of Export/Scripts/legionSprites.lua.\nreturn {\n");
    for (kind, names) in [("keystone", &sections[2]), ("notable", &sections[1]), ("normal", &sections[0])] {
        let mut names: Vec<&String> = names.iter().collect();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        for (state, file) in [("Active", "legion-active.png"), ("Inactive", "legion-inactive.png")] {
            let _ = write!(out, "\t[\"{}{}\"] = {{{{\n\t\t[\"filename\"] = \"{}\",\n\t\t[\"coords\"] = {{\n", kind, state, file);
            for name in &names {
                if let Some(entry) = icons.iter().find(|e| &&e.name == name) {
                    coord_line(&mut out, entry);
                }
            }
            out.push_str("\t\t},\n\t}},\n");
        }
    }
    out.push_str("\t[\"treeAssets\"] = {\n\t\t{ [\"filename\"] = \"legion-art.png\", [\"coords\"] = {\n");
    let mut sorted: Vec<&Entry> = art.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in sorted {
        coord_line(&mut out, entry);
    }
    out.push_str("\t\t} },\n");
    for state in ["Normal", "Intermediate", "Active"] {
        let _ = writeln!(
            out,
            "\t\t{{ [\"filename\"] = \"abyss-line-connector-{}.png\", [\"coords\"] = {{ [\"AbyssLineConnector{}\"] = {{ [\"x\"] = 0, [\"y\"] = 0, [\"w\"] = {}, [\"h\"] = {} }} }} }},",
            state.to_ascii_lowercase(),
            state,
            LINE_SIZE.0,
            LINE_SIZE.1
        );
    }
    out.push_str("\t},\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, w: u32, h: u32) -> Entry {
        Entry::new(name.to_string(), RgbaImage::new(w, h))
    }

    #[test]
    fn packing_keeps_one_height_per_row() {
        let mut entries = vec![entry("b", 64, 64), entry("A", 27, 27), entry("c", 27, 27), entry("d", 300, 64), entry("e", 300, 64)];
        assert_eq!(pack(&mut entries), (364, 27 + 64 + 64));
        let at = |n: &str| entries.iter().find(|e| e.name == n).map(|e| (e.x, e.y)).unwrap();
        assert_eq!((at("A"), at("c")), ((0, 0), (27, 0)));
        assert_eq!((at("b"), at("d"), at("e")), ((0, 27), (64, 27), (0, 91)));
    }

    #[test]
    fn the_lua_lists_each_kind_twice_and_the_art_once() {
        let icons = vec![entry("Art/K.dds", 64, 64), entry("Art/n.dds", 27, 27)];
        let sections = [vec!["Art/n.dds".to_string()], Vec::new(), vec!["Art/K.dds".to_string()]];
        let text = lua(&icons, &sections, &[entry("AbyssOrbit1Normal", 33, 33)]);
        assert!(text.contains("\t[\"keystoneActive\"] = {{\n\t\t[\"filename\"] = \"legion-active.png\",\n\t\t[\"coords\"] = {\n\t\t\t[\"Art/K.dds\"] = { [\"x\"] = 0, [\"y\"] = 0, [\"w\"] = 64, [\"h\"] = 64 },\n\t\t},\n\t}},\n"));
        assert!(text.contains("\t[\"normalInactive\"] = {{\n\t\t[\"filename\"] = \"legion-inactive.png\""));
        assert!(text.ends_with("[\"AbyssLineConnectorActive\"] = { [\"x\"] = 0, [\"y\"] = 0, [\"w\"] = 368, [\"h\"] = 13 } } },\n\t},\n}\n"));
    }
}
