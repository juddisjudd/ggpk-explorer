//! PoE 1's atlas tree in the layout of GGG's atlastree-export: a file per tree that draws the graph, their Ruthless twins, and the sheets at five zoom levels.

use super::json::{self, J};
use super::sheets::{self, Packed, TextureStore};
use super::{png_path, tables, ExportSummary, TreeBuild, TreeExportOptions, TreeExportSource, BACKGROUND_TILE, PSS_CENTRE_INNER_RADIUS};
use crate::dat::psg::{PsgFile, PsgGroup};
use crate::export::ExportStatus;
use crate::skill_tree::art::NodeFrameKind;
use crate::skill_tree::atlas_node_db::{tree_context_for_graph_type, SkillGraphDatabase};
use image::RgbaImage;
use std::path::Path;
use std::sync::mpsc::Sender;

const ATLAS_GRAPH: u8 = 1;
const ZOOMS: [&str; 5] = ["0.1246", "0.2109", "0.2972", "0.3835", "0.5"];
const NATIVE_ZOOM: f32 = 0.5;
/// What GGG's atlas export states; no table holds the atlas point total.
const ATLAS_POINTS: i64 = 150;
const ICON_SIZES: [(&str, u32, u32); 3] = [("normal", 34, 34), ("notable", 49, 49), ("keystone", 68, 69)];
const WORMHOLE_ICON_SIZE: u32 = 86;
const MASTERY_ICON_SIZE: u32 = 113;
const MASTERY_OVERLAY_SIZE: u32 = 256;
/// GGG draws the atlas start this tall at zoom 0.5, far smaller than its art, which is over a thousand pixels.
const START_HEIGHT: f32 = 229.0;
const ARC_SIZES: [u32; 6] = [52, 92, 178, 257, 342, 434];
/// GGG's export nudges these league backdrops off their group's centre; no game file says by how much.
const BACKDROP_OFFSETS: [(&str, i64, i64); 2] = [("CleansingFire", 3, -14), ("Tangle", -7, -71)];
const SCREEN: &str = "Art/2DArt/UIImages/InGame/AtlasScreen/";
const WORMHOLE_ICONS: [(&str, &str); 2] = [
    ("wormholeActive", "Art/2DArt/SkillIcons/passives/AtlasTrees/AtlasPassiveWormholeActiveIcon"),
    ("wormholeInactive", "Art/2DArt/SkillIcons/passives/AtlasTrees/AtlasPassiveWormholeDefaultIcon"),
];
const WORMHOLE_FRAMES: [(&str, &str); 4] = [
    ("WormholeFrameUnallocated", "AtlasWormholeFrameNormal"),
    ("WormholeFrameHighlight", "AtlasWormholeFrameHighlight"),
    ("WormholeFrameCanAllocate", "AtlasWormholeFrameCanAllocate"),
    ("WormholeFrameAllocated", "AtlasWormholeFrameActive"),
];
const BACKDROP_LAYERS: [&str; 3] = ["NewAtlasPassiveBackground", "NewAtlasPassiveBackgroundOuterRing1", "NewAtlasPassiveBackgroundOuterRing2"];
const SPRITE_ORDER: [&str; 16] = [
    "background", "normalActive", "notableActive", "keystoneActive", "wormholeActive", "normalInactive", "notableInactive",
    "keystoneInactive", "wormholeInactive", "mastery", "masteryOverlay", "groupBackground", "startNode", "frame", "line",
    "atlasBackground",
];

pub fn is_atlas(psg: &PsgFile) -> bool {
    psg.is_poe1() && psg.graph_type == ATLAS_GRAPH
}

pub fn constants(psg: &PsgFile) -> J {
    let mut out = J::obj();
    out.set("classes", J::Arr(Vec::new()));
    out.set("characterAttributes", J::Arr(Vec::new()));
    out.set("PSSCentreInnerRadius", J::Int(PSS_CENTRE_INNER_RADIUS));
    out.set("skillsPerOrbit", J::ints(psg.passives_per_orbit.iter().map(|n| *n as i64)));
    out.set("orbitRadii", J::ints(psg.orbit_radii().iter().take(psg.passives_per_orbit.len()).map(|r| *r as i64)));
    out
}

/// The league a group of type 4 belongs to, named by one of its nodes' `AtlasGroup`.
fn league_of(group: &PsgGroup, db: &SkillGraphDatabase, tables: &tables::ExtraTables) -> Option<String> {
    if group.background_type != 4 {
        return None;
    }
    group
        .nodes
        .iter()
        .filter_map(|n| db.nodes.get(&n.skill_id)?.atlas_group)
        .find_map(|row| tables.atlas_group_types.get(row).filter(|id| !id.is_empty()).cloned())
}

/// Atlas groups name their backdrop by type, falling back to the widest orbit they fill as the character tree does.
pub fn group_background(group: &PsgGroup, db: &SkillGraphDatabase, tables: &tables::ExtraTables) -> Option<J> {
    if group.nodes.is_empty() {
        return None;
    }
    let mut out = J::obj();
    if let Some(league) = league_of(group, db, tables) {
        out.set("image", J::str(&format!("GroupBackground{}", league)));
        if let Some((_, x, y)) = BACKDROP_OFFSETS.iter().find(|(id, _, _)| *id == league) {
            out.set("offsetX", J::Int(*x));
            out.set("offsetY", J::Int(*y));
        }
        return Some(out);
    }
    let widest = group.nodes.iter().map(|n| n.radius).filter(|r| (1..=3).contains(r)).max();
    let image = match (group.background_type, widest) {
        (1, _) | (0, Some(1)) => "PSGroupBackground1",
        (2, _) | (0, Some(2)) => "PSGroupBackground2",
        (3, _) | (0, Some(3)) => "PSGroupBackground3",
        (5, _) => "PSGroupBackground1Alt",
        (6, _) => "PSGroupBackground2Alt",
        _ => return None,
    };
    out.set("image", J::str(image));
    Some(out)
}

fn file_name(first: &str, name: &str, ruthless: bool) -> String {
    let league = match name.strip_prefix(first) {
        Some("") => None,
        Some("CurrentLeague") => Some("league".to_string()),
        _ => Some(name.to_ascii_lowercase()),
    };
    match (ruthless, league) {
        (false, None) => "data.json".to_string(),
        (false, Some(l)) => format!("{}.json", l),
        (true, None) => "ruthless.json".to_string(),
        (true, Some(l)) => format!("ruthless-{}.json", l),
    }
}

pub(crate) fn export(
    source: &TreeExportSource,
    psg_path: &str,
    psg: &PsgFile,
    options: &TreeExportOptions,
    out_dir: &Path,
    tx: &Sender<ExportStatus>,
) -> Result<(ExportSummary, J), String> {
    const STEPS: usize = 4;
    let mut step = 0usize;
    let mut progress = |msg: &str| {
        step += 1;
        let _ = tx.send(ExportStatus::Progress { current: step.min(STEPS), total: STEPS, filename: msg.to_string() });
    };

    progress("Resolving atlas passives");
    let fetch = |p: &str| source.fetch(p);
    let db = crate::skill_tree::build_skill_graph_db_from(&fetch, &source.schema, false, false, true)?;
    let ruthless_db = crate::skill_tree::build_skill_graph_db_from(&fetch, &source.schema, false, true, true)?;
    let tables = tables::load(source, &db, psg_path);

    progress("Building the trees");
    let tree = super::build_tree(psg, &db, &tables);
    let ruthless = super::build_tree(psg, &ruthless_db, &tables);

    progress("Sheets");
    let assets_dir = out_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).map_err(|e| format!("Cannot create {}: {}", assets_dir.display(), e))?;
    let (sprites, mut files) = write_sheets(source, psg, &tree, &db, &tables, options.quality, &assets_dir)?;

    progress("Writing the trees");
    let mut tail = J::obj();
    tail.set("sprites", sprites);
    tail.set("imageZoomLevels", J::Arr(ZOOMS.iter().map(|z| J::num(z.parse::<f64>().unwrap_or(0.0))).collect()));
    let mut points = J::obj();
    points.set("totalPoints", J::Int(ATLAS_POINTS));
    points.set("ascendancyPoints", J::Int(0));
    tail.set("points", points);

    let names = match tables.tree_names.is_empty() {
        true => vec![tables.tree_name.clone()],
        false => tables.tree_names.clone(),
    };
    let first = names[0].clone();
    let mut main = J::Null;
    for (build, hard) in [(&tree, false), (&ruthless, true)] {
        for name in &names {
            let mut data = J::obj();
            data.set("tree", J::str(name));
            if hard {
                data.set("ruthless", J::Bool(true));
            }
            for source in [&build.data, &tail] {
                let J::Obj(fields) = source else { continue };
                for (key, value) in fields.iter().filter(|(key, _)| key != "tree") {
                    data.set(key, value.clone());
                }
            }
            std::fs::write(out_dir.join(file_name(&first, name, hard)), json::to_string_pretty(&data)).map_err(|e| e.to_string())?;
            files += 1;
            if !hard && *name == first {
                main = data;
            }
        }
    }

    let summary = ExportSummary {
        files,
        message: format!(
            "Exported atlas trees {} and their Ruthless versions ({} nodes) to {}",
            names.join(", "),
            tree.node_count,
            out_dir.display()
        ),
    };
    Ok((summary, main))
}

fn zoom_factor(zoom: &str) -> f32 {
    zoom.parse::<f32>().unwrap_or(NATIVE_ZOOM) / NATIVE_ZOOM
}

/// One image and its size at zoom 0.5; every zoom truncates that size, so a 147-pixel frame is 36 pixels at the smallest zoom, not 37.
struct Sprite {
    key: String,
    image: RgbaImage,
    size: (f32, f32),
}

impl Sprite {
    fn exact(key: String, image: &RgbaImage, w: u32, h: u32) -> Self {
        Sprite { key, image: image.clone(), size: (w as f32, h as f32) }
    }

    /// GGG scales a node icon from an odd size, one pixel wider than twice its size at zoom 0.5.
    fn icon(key: String, image: &RgbaImage, w: u32, h: u32) -> Self {
        Sprite { key, image: image.clone(), size: (w as f32 + 0.5, h as f32 + 0.5) }
    }

    /// Interface art at half its size, counting the last pixel of its rectangle, which the sheet index leaves out.
    fn halved(key: String, image: &RgbaImage) -> Self {
        Sprite { key, size: ((image.width() + 1) as f32 * 0.5, (image.height() + 1) as f32 * 0.5), image: image.clone() }
    }

    fn at(&self, factor: f32) -> RgbaImage {
        let side = |v: f32| ((v * factor) as u32).max(1);
        sheets::resize(&self.image, side(self.size.0), side(self.size.1))
    }
}

fn shrink_packed(packed: &Packed, factor: f32) -> Packed {
    let at = |v: u32| (v as f32 * factor) as u32;
    Packed {
        image: sheets::resize(&packed.image, at(packed.image.width()).max(1), at(packed.image.height()).max(1)),
        frames: packed.frames.iter().map(|f| sheets::Frame { key: f.key.clone(), x: at(f.x), y: at(f.y), w: at(f.w).max(1), h: at(f.h).max(1) }).collect(),
    }
}

type Entries = Vec<(String, String, J)>;

fn write_packed(dir: &Path, name: &str, packed: &Packed, png: bool, quality: f32, zoom: &str, entries: &mut Entries) -> Result<(), String> {
    let bytes = match png {
        true => {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(packed.image.clone()).write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            out.into_inner()
        }
        false => sheets::encode_webp(&packed.image, quality),
    };
    std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
    for (sprite, entry) in sheets::sprite_json_split(packed, name, "") {
        entries.push((sprite, zoom.to_string(), entry));
    }
    Ok(())
}

fn write_zoomed(dir: &Path, file: &str, sprites: &[Sprite], png: bool, quality: f32, entries: &mut Entries) -> Result<usize, String> {
    if sprites.is_empty() {
        return Ok(0);
    }
    let ext = if png { "png" } else { "webp" };
    let mut width = 0;
    for (i, zoom) in ZOOMS.iter().enumerate().rev() {
        let factor = zoom_factor(zoom);
        let scaled: Vec<(String, RgbaImage)> = sprites.iter().map(|s| (s.key.clone(), s.at(factor))).collect();
        if width == 0 {
            width = sheets::square_width(&scaled);
        }
        let packed = sheets::pack(&scaled, ((width as f32 * factor) as u32).max(1));
        write_packed(dir, &format!("{}-{}.{}", file, i, ext), &packed, png, quality, zoom, entries)?;
    }
    Ok(ZOOMS.len())
}

fn write_sheets(
    source: &TreeExportSource,
    psg: &PsgFile,
    tree: &TreeBuild,
    db: &SkillGraphDatabase,
    tables: &tables::ExtraTables,
    quality: f32,
    dir: &Path,
) -> Result<(J, usize), String> {
    let art = db.art_sets.get(tree_context_for_graph_type(psg.graph_type));
    let leagues: Vec<String> = {
        let mut out: Vec<String> = Vec::new();
        for league in psg.groups.iter().filter_map(|g| league_of(g, db, tables)) {
            if !out.contains(&league) {
                out.push(league);
            }
        }
        out
    };
    let has_wormhole = psg.groups.iter().flat_map(|g| &g.nodes).any(|n| db.nodes.get(&n.skill_id).is_some_and(|i| i.is_wormhole));

    let mut group_art: Vec<(String, String)> =
        leagues.iter().map(|l| (format!("groupBackground:GroupBackground{}", l), format!("{}AtlasPassiveSkillScreenGroupBackgroundLarge{}", SCREEN, l))).collect();
    if let Some(bg) = art.map(|a| &a.group_background) {
        for (name, path) in [
            ("PSGroupBackground3Alt", &bg.large_blank),
            ("PSGroupBackground3", &bg.large),
            ("PSGroupBackground2Alt", &bg.medium_blank),
            ("PSGroupBackground2", &bg.medium),
            ("PSGroupBackground1Alt", &bg.small_blank),
            ("PSGroupBackground1", &bg.small),
        ] {
            group_art.push((format!("groupBackground:{}", name), path.clone()));
        }
    }
    let mut frame_art: Vec<(String, String)> = WORMHOLE_FRAMES.iter().filter(|_| has_wormhole).map(|(name, file)| (name.to_string(), format!("{}{}", SCREEN, file))).collect();
    for (kind, names) in [
        (NodeFrameKind::Keystone, ["KeystoneFrameUnallocated", "KeystoneFrameCanAllocate", "KeystoneFrameAllocated"]),
        (NodeFrameKind::Notable, ["NotableFrameUnallocated", "NotableFrameCanAllocate", "NotableFrameAllocated"]),
        (NodeFrameKind::Passive, ["PSSkillFrame", "PSSkillFrameHighlighted", "PSSkillFrameActive"]),
    ] {
        if let Some(f) = art.and_then(|a| a.frames.get(&kind)) {
            for (name, path) in names.iter().zip([&f.normal, &f.can_allocate, &f.active]) {
                frame_art.push((name.to_string(), path.clone()));
            }
        }
    }
    let start = format!("{}AtlasPassiveSkillScreenStart", SCREEN);
    let layers: Vec<String> = BACKDROP_LAYERS.iter().map(|l| format!("{}{}", SCREEN, l)).collect();
    let connectors: [String; 5] = match tables.connection_art.get(tree_context_for_graph_type(psg.graph_type)) {
        Some([active, normal, intermediate]) => [active.clone(), normal.clone(), intermediate.clone(), String::new(), String::new()],
        None => tree.connectors.clone(),
    };

    let mut store = TextureStore::new(source);
    let mut wanted: Vec<String> = tree.icons.iter().map(|(_, p)| p.clone()).collect();
    wanted.extend(group_art.iter().chain(frame_art.iter()).map(|(_, p)| p.clone()));
    wanted.extend(WORMHOLE_ICONS.iter().map(|(_, p)| p.to_string()));
    wanted.extend(connectors.iter().cloned());
    wanted.extend(layers.iter().cloned());
    wanted.push(start.clone());
    wanted.push(BACKGROUND_TILE.to_string());
    wanted.retain(|p| !p.is_empty());
    store.prefetch(&wanted);

    let mut entries: Entries = Vec::new();
    let mut files = 0;

    let tile = store.get(BACKGROUND_TILE).map(|img| sheets::resize(img, 128, 128));
    let background: Vec<Sprite> = tile.iter().map(|t| Sprite::exact("background:Background2".to_string(), t, 128, 128)).collect();
    files += write_zoomed(dir, "background", &background, true, quality, &mut entries)?;

    let (mut active, mut inactive, mut mastery, mut overlay) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (category, path) in &tree.icons {
        let Some(img) = store.get(path) else { continue };
        let png = png_path(path);
        if category == "mastery" {
            mastery.push(Sprite::exact(format!("mastery:{}", png), img, MASTERY_ICON_SIZE, MASTERY_ICON_SIZE));
            overlay.push((format!("masteryOverlay:{}", png), sheets::resize(img, MASTERY_OVERLAY_SIZE, MASTERY_OVERLAY_SIZE)));
            continue;
        }
        let Some((_, w, h)) = ICON_SIZES.iter().find(|(c, _, _)| c == category) else { continue };
        inactive.push(Sprite::icon(format!("{}Inactive:{}", category, png), &sheets::disabled_icon(img), *w, *h));
        active.push(Sprite::icon(format!("{}Active:{}", category, png), img, *w, *h));
    }
    if has_wormhole {
        for ((sheet, path), list) in WORMHOLE_ICONS.iter().zip([&mut active, &mut inactive]) {
            if let Some(img) = store.get(path) {
                list.push(Sprite::icon(format!("{}:Wormhole", sheet), img, WORMHOLE_ICON_SIZE, WORMHOLE_ICON_SIZE));
            }
        }
    }
    files += write_zoomed(dir, "atlas-skills", &active, false, quality, &mut entries)?;
    files += write_zoomed(dir, "atlas-skills-disabled", &inactive, false, quality, &mut entries)?;
    files += write_zoomed(dir, "atlas-mastery", &mastery, false, quality, &mut entries)?;
    if !overlay.is_empty() {
        let packed = sheets::pack(&overlay, sheets::square_width(&overlay));
        write_packed(dir, "atlas-mastery-overlay.webp", &packed, false, quality, "1", &mut entries)?;
        files += 1;
    }

    let mut groups: Vec<Sprite> = group_art.iter().filter_map(|(key, path)| store.get(path).map(|img| Sprite::halved(key.clone(), img))).collect();
    if let Some(img) = store.get(&start) {
        let size = (img.width() as f32 * START_HEIGHT / img.height().max(1) as f32, START_HEIGHT);
        groups.push(Sprite { key: "startNode:AtlasPassiveSkillScreenStart".to_string(), image: img.clone(), size });
    }
    files += write_zoomed(dir, "atlas-group-background", &groups, false, quality, &mut entries)?;

    let frames: Vec<Sprite> = frame_art.iter().filter_map(|(name, path)| store.get(path).map(|img| Sprite::halved(format!("frame:{}", name), img))).collect();
    files += write_zoomed(dir, "atlas-frame", &frames, false, quality, &mut entries)?;

    if let Some((image, frames)) = super::line_sheet(&mut store, &connectors, [&ARC_SIZES, &ARC_SIZES, &ARC_SIZES], false) {
        let packed = Packed { image, frames };
        for (i, zoom) in ZOOMS.iter().enumerate() {
            write_packed(dir, &format!("atlas-line-{}.webp", i), &shrink_packed(&packed, zoom_factor(zoom)), false, quality, zoom, &mut entries)?;
            files += 1;
        }
    }

    let layers: Vec<RgbaImage> = layers.iter().filter_map(|l| store.get(l).cloned()).collect();
    if let (Some(size), Some(tile)) = (layers.iter().map(|l| l.width().max(l.height())).max(), tile.as_ref()) {
        let mut backdrop = RgbaImage::new(size, size);
        for y in (0..size).step_by(tile.height() as usize) {
            for x in (0..size).step_by(tile.width() as usize) {
                image::imageops::replace(&mut backdrop, tile, x as i64, y as i64);
            }
        }
        for layer in &layers {
            image::imageops::overlay(&mut backdrop, layer, ((size - layer.width()) / 2) as i64, ((size - layer.height()) / 2) as i64);
        }
        let sprite = Sprite::exact("atlasBackground:AtlasPassiveBackground".to_string(), &backdrop, size, size);
        files += write_zoomed(dir, "atlas-background", &[sprite], false, quality, &mut entries)?;
    }

    let mut sprites = J::obj();
    for name in SPRITE_ORDER {
        let mut by_zoom = J::obj();
        for zoom in ZOOMS.iter().copied().chain(std::iter::once("1")) {
            if let Some((_, _, entry)) = entries.iter().find(|(s, z, _)| s == name && z == zoom) {
                by_zoom.set(zoom, entry.clone());
            }
        }
        if !by_zoom.is_empty() {
            sprites.set(name, by_zoom);
        }
    }
    Ok((sprites, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tree_and_its_ruthless_twin_get_the_file_ggg_names_them() {
        let names = [("Atlas", false), ("AtlasCurrentLeague", false), ("Atlas", true), ("AtlasCurrentLeague", true)];
        let files: Vec<String> = names.iter().map(|(name, hard)| file_name("Atlas", name, *hard)).collect();
        assert_eq!(files, ["data.json", "league.json", "ruthless.json", "ruthless-league.json"]);
    }

    #[test]
    fn sprites_shrink_by_truncating_from_their_zoom_half_size() {
        let image = RgbaImage::new(289, 293);
        let frame = Sprite::halved("frame:WormholeFrameUnallocated".to_string(), &image);
        assert_eq!(frame.at(1.0).dimensions(), (145, 147));
        assert_eq!(frame.at(zoom_factor("0.1246")).dimensions(), (36, 36));
        let keystone = Sprite::icon("keystoneActive:x".to_string(), &RgbaImage::new(64, 64), 68, 69);
        assert_eq!(keystone.at(zoom_factor("0.1246")).dimensions(), (17, 17));
        assert_eq!(keystone.at(zoom_factor("0.2109")).dimensions(), (28, 29));
    }
}
