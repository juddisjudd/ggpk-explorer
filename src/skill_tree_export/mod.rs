//! Exports a `.psg` skill graph in the layout of GGG's official web-tree
//! export (`data.json` plus sprite sheets under `assets/`), together with a
//! standalone HTML viewer.
//!
//! Field-for-field the `data.json` follows the official file: node
//! positions are the raw `.psg` group positions plus the orbit offset (no
//! ascendancy relocation), edges are listed root-first then in graph order
//! with `orbit` being the zero-based ring index and `orbitX`/`orbitY` the
//! arc centre, and `keystonesInRadius` names the keystone within
//! [`KEYSTONE_RADIUS`] units that comes last in `PassiveSkills` row order.

pub mod json;
mod sheets;
mod tables;

use crate::bundles::index::{fnv1a64, murmur_hash64a, FileInfo, Index};
use crate::bundles::steam::SteamBundleLoader;
use crate::dat::psg::PsgFile;
use crate::dat::schema::Schema;
use crate::export::ExportStatus;
use crate::ggpk::reader::GgpkReader;
use crate::ui::atlas_node_db::{tree_context_for_graph_type, SkillGraphDatabase, SkillGraphNodeInfo};
use crate::ui::content_view::{build_skill_graph_db, dds_path_candidates, decompress_bundle, extract_bundle_file_sync};
use crate::ui::skill_tree_art::{FrameArt, NodeFrameKind};
use crate::ui::skill_tree_layout::{self, ASCENDANCY_PLATE_SIZE, CLASS_ILLUSTRATION_SIZE, MAIN_CIRCLE_SIZE};
use image::RgbaImage;
use json::J;
use sheets::TextureStore;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Keystones this close to a node are listed in its `keystonesInRadius`.
pub const KEYSTONE_RADIUS: f64 = 1380.0;

const VIEWER_HTML: &str = include_str!("viewer.html");

/// Icon sizes (sheet pixels) the official `skills` sheet uses per node type.
const ICON_SIZES: [(&str, u32, u32); 4] = [("normal", 34, 34), ("notable", 49, 49), ("keystone", 68, 69), ("mastery", 49, 49)];
/// Official mastery pattern width in the `mastery-effect-*` sheets.
const MASTERY_PATTERN_WIDTH: u32 = 244;
/// Quarter-arc frame sizes (sheet pixels) per orbit in the official `line` sheet.
const ARC_SIZES_ACTIVE: [u32; 9] = [52, 92, 178, 257, 342, 429, 135, 554, 674];
const ARC_SIZES_NORMAL: [u32; 9] = [44, 84, 170, 249, 334, 421, 127, 546, 666];
/// Rows of the straight connector strip inside a half-scale connector block.
const LINE_STRIP: (u32, u32) = (16, 17);

const MAIN_CIRCLE_ACTIVE_FULL: &str = "Art/2DArt/UIImages/InGame/PassiveTree/PassiveTreeMainCircleActive2";
const BACKGROUND_TILE: &str = "Art/2DArt/UIImages/Common/Background2";
const PLUS_FRAME_CAN_ALLOCATE: &str = "Art/2DArt/UIImages/InGame/PassiveSkillScreenPlusFrameCanAllocate";
const ORACLE_JEWEL_CIRCLES: [(&str, &str); 2] = [
    ("DruidOracleAscendancy1", "Art/2DArt/UIImages/InGame/OraclePassiveSkillScreenJewelCircle1"),
    ("DruidOracleAscendancy2", "Art/2DArt/UIImages/InGame/OraclePassiveSkillScreenJewelCircle2"),
];
/// The official `jewel` sheet (client-hardcoded socket overlays) plus the
/// PoE 2 jewel base types.
const JEWEL_SOCKET_ART: [(&str, &str); 19] = [
    ("JewelSocketActiveRedAlt", "PassiveSkillScreenJewelSocketActiveRedAlt"),
    ("JewelSocketActivePrismaticAlt", "PassiveSkillScreenJewelSocketActivePrismaticAlt"),
    ("JewelSocketActiveLegionAlt", "PassiveSkillScreenJewelSocketActiveLegionAlt"),
    ("JewelSocketActiveGreenAlt", "PassiveSkillScreenJewelSocketActiveGreenAlt"),
    ("JewelSocketActiveBlueAlt", "PassiveSkillScreenJewelSocketActive_BlueAlt"),
    ("JewelSocketActiveAltRed", "PassiveSkillScreenJewelSocketActiveAltRed"),
    ("JewelSocketActiveAltPurple", "PassiveSkillScreenJewelSocketActiveAltPurple"),
    ("JewelSocketActiveAltBlue", "PassiveSkillScreenJewelSocketActiveAltBlue"),
    ("JewelSocketActiveAbyssAlt", "PassiveSkillScreenJewelSocketActiveAbyssAlt"),
    ("JewelSocketActiveRed", "PassiveSkillScreenJewelSocketActiveRed"),
    ("JewelSocketActivePrismatic", "PassiveSkillScreenJewelSocketActivePrismatic"),
    ("JewelSocketActiveLegion", "PassiveSkillScreenJewelSocketActiveLegion"),
    ("JewelSocketActiveGreen", "PassiveSkillScreenJewelSocketActiveGreen"),
    ("JewelSocketActiveBlue", "PassiveSkillScreenJewelSocketActiveBlue"),
    ("JewelSocketActiveAbyss", "PassiveSkillScreenJewelSocketActiveAbyss"),
    ("JewelSocketActiveEmeraldJewel", "PassiveSkillScreenJewelSocketActiveEmeraldJewel"),
    ("JewelSocketActiveRubyJewel", "PassiveSkillScreenJewelSocketActiveRubyJewel"),
    ("JewelSocketActiveSapphireJewel", "PassiveSkillScreenJewelSocketActive_SapphireJewel"),
    ("JewelSocketActiveDiamondBaseJewel", "PassiveSkillScreenJewelSocketActiveDiamondBaseJewel"),
];

#[derive(Debug, Clone)]
pub struct TreeExportOptions {
    /// WebP quality for the sheets; zero or less writes lossless files.
    pub quality: f32,
    /// Also write `index.html` + `data.js`/`assets.js` next to the export.
    pub viewer: bool,
}

impl Default for TreeExportOptions {
    fn default() -> Self {
        Self { quality: 85.0, viewer: true }
    }
}

/// Everything needed to read game files for the export.
pub struct TreeExportSource {
    pub reader: Option<Arc<GgpkReader>>,
    pub index: Arc<Index>,
    pub steam: Option<SteamBundleLoader>,
    pub schema: Schema,
}

impl TreeExportSource {
    /// Index entry for a path, by hash (the index keys files by the hash of
    /// the lower-cased path).
    pub(crate) fn lookup(&self, path: &str) -> Option<&FileInfo> {
        if path.is_empty() {
            return None;
        }
        let lower = path.to_ascii_lowercase();
        [murmur_hash64a(lower.as_bytes()), fnv1a64(lower.as_bytes()), fnv1a64(path.as_bytes())]
            .iter()
            .find_map(|h| self.index.files.get(h))
            .filter(|f| f.path.eq_ignore_ascii_case(path))
    }

    pub(crate) fn extract(&self, info: &FileInfo) -> Option<Vec<u8>> {
        extract_bundle_file_sync(info, &self.index, self.reader.as_deref(), self.steam.as_ref())
    }

    pub(crate) fn fetch(&self, path: &str) -> Option<Vec<u8>> {
        self.lookup(path).and_then(|i| self.extract(i))
    }

    /// Index entry for a DAT-style texture path (with or without `.dds`,
    /// with or without the `Textures/Interface/2D` segment).
    pub(crate) fn resolve_texture(&self, path: &str) -> Option<&FileInfo> {
        dds_path_candidates(path).iter().find_map(|c| self.lookup(c))
    }

    pub(crate) fn decompress_bundle(&self, bundle_index: u32) -> Option<Vec<u8>> {
        decompress_bundle(bundle_index, &self.index, self.reader.as_deref(), self.steam.as_ref())
    }
}

/// Runs the whole export, reporting through `tx` like `export::run_export`.
#[allow(clippy::too_many_arguments)]
pub fn run_tree_export(
    source: TreeExportSource,
    psg_path: String,
    psg: PsgFile,
    db: Option<Arc<SkillGraphDatabase>>,
    options: TreeExportOptions,
    out_dir: PathBuf,
    tx: Sender<ExportStatus>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        export_tree(&source, &psg_path, &psg, db, &options, &out_dir, &tx)
    }));
    match result {
        Ok(Ok(summary)) => {
            let _ = tx.send(ExportStatus::Complete { count: summary.files, errors: 0, message: summary.message });
        }
        Ok(Err(e)) => {
            let _ = tx.send(ExportStatus::Error(e));
        }
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            let _ = tx.send(ExportStatus::Error(format!("Skill tree export panicked: {}", msg)));
        }
    }
}

pub struct ExportSummary {
    pub files: usize,
    pub message: String,
}

const STEPS: usize = 14;

fn export_tree(
    source: &TreeExportSource,
    psg_path: &str,
    psg: &PsgFile,
    db: Option<Arc<SkillGraphDatabase>>,
    options: &TreeExportOptions,
    out_dir: &Path,
    tx: &Sender<ExportStatus>,
) -> Result<ExportSummary, String> {
    let mut step = 0usize;
    let mut progress = |msg: &str| {
        step += 1;
        let _ = tx.send(ExportStatus::Progress { current: step.min(STEPS), total: STEPS, filename: msg.to_string() });
    };

    progress("Resolving passive skill data");
    let db = match db {
        Some(db) => db,
        None => Arc::new(build_skill_graph_db(source.reader.as_deref(), &source.index, source.steam.as_ref(), &source.schema)?),
    };
    let tables = tables::load(source, &db, psg_path);

    progress("Building data.json");
    let tree = build_tree(psg, &db, &tables);
    std::fs::create_dir_all(out_dir).map_err(|e| format!("Cannot create {}: {}", out_dir.display(), e))?;
    let data_json = json::to_string_pretty(&tree.data);
    std::fs::write(out_dir.join("data.json"), &data_json).map_err(|e| e.to_string())?;
    let mut files = 1;

    let assets_dir = out_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).map_err(|e| e.to_string())?;
    let mut sheet_jsons: Vec<(String, J)> = Vec::new();
    let mut store = TextureStore::new(source);
    let write = |name: &str, sprites: &[(String, RgbaImage)], max_width: u32, sheet_jsons: &mut Vec<(String, J)>| -> Result<usize, String> {
        if sprites.is_empty() {
            return Ok(0);
        }
        let json = sheets::write_sheet(&assets_dir, name, sprites, max_width, options.quality)?;
        sheet_jsons.push((name.to_string(), json));
        Ok(2)
    };

    progress("Fetching textures");
    store.prefetch(&tree.texture_paths);

    progress("Skill icons");
    let (active, inactive) = icon_sprites(&mut store, &tree.icons);
    files += write("skills", &active, 1024, &mut sheet_jsons)?;
    files += write("skills-disabled", &inactive, 1024, &mut sheet_jsons)?;

    progress("Node frames");
    let frames = frame_sprites(&mut store, &tree.frames);
    files += write("frame", &frames, sheets::square_width(&frames), &mut sheet_jsons)?;

    progress("Connectors");
    if let Some((image, frames)) = line_sheet(&mut store, &tree.connectors) {
        let packed = sheets::Packed { image, frames };
        let json = sheets::frames_json(&packed, "line.webp");
        std::fs::write(assets_dir.join("line.webp"), sheets::encode_webp(&packed.image, options.quality)).map_err(|e| e.to_string())?;
        std::fs::write(assets_dir.join("line.json"), json::to_string_pretty(&json)).map_err(|e| e.to_string())?;
        sheet_jsons.push(("line".to_string(), json));
        files += 2;
    }

    progress("Centre ring");
    let group_bg = scaled_sprites(&mut store, &tree.group_backgrounds, 0.5);
    files += write("group-background", &group_bg, 4000, &mut sheet_jsons)?;

    progress("Background tile");
    let mut background = Vec::new();
    if let Some(img) = store.get(BACKGROUND_TILE) {
        background.push(("background:Background2".to_string(), sheets::resize(img, 128, 128)));
    }
    files += write("background", &background, 128, &mut sheet_jsons)?;

    progress("Jewel sockets");
    let jewel: Vec<(String, String)> = JEWEL_SOCKET_ART
        .iter()
        .map(|(name, file)| (format!("jewel:{}", name), format!("Art/2DArt/UIImages/InGame/{}", file)))
        .collect();
    let jewel = scaled_sprites(&mut store, &jewel, 0.5);
    files += write("jewel", &jewel, sheets::square_width(&jewel), &mut sheet_jsons)?;

    progress("Jewel radii");
    let radius = scaled_sprites(&mut store, &tree.jewel_radius, 0.5);
    files += write("jewel-radius", &radius, sheets::square_width(&radius), &mut sheet_jsons)?;

    progress("Mastery patterns");
    let (mastery_active, mastery_inactive) = mastery_sprites(&mut store, &tree.mastery_images);
    files += write("mastery-effect-active", &mastery_active, sheets::square_width(&mastery_active), &mut sheet_jsons)?;
    files += write("mastery-effect-disabled", &mastery_inactive, sheets::square_width(&mastery_inactive), &mut sheet_jsons)?;

    progress("Class illustrations");
    for class in &tree.class_sheets {
        let sprites = scaled_sprites(&mut store, &class.images, 1.0);
        files += write(&class.sheet, &sprites, 4500, &mut sheet_jsons)?;
    }

    if options.viewer {
        progress("Viewer");
        std::fs::write(out_dir.join("index.html"), VIEWER_HTML).map_err(|e| e.to_string())?;
        std::fs::write(out_dir.join("data.js"), format!("window.TREE_DATA = {};\n", data_json)).map_err(|e| e.to_string())?;
        let mut sheets_obj = J::obj();
        for (name, json) in &sheet_jsons {
            sheets_obj.set(name, json.clone());
        }
        let mut assets = J::obj();
        assets.set("sheets", sheets_obj);
        assets.set("extras", tree.extras.clone());
        std::fs::write(out_dir.join("assets.js"), format!("window.TREE_ASSETS = {};\n", json::to_string_pretty(&assets))).map_err(|e| e.to_string())?;
        files += 3;
    }

    progress("Done");
    Ok(ExportSummary {
        files,
        message: format!("Exported skill tree '{}' ({} nodes, {} sheets) to {}", tree.name, tree.node_count, sheet_jsons.len(), out_dir.display()),
    })
}

// ── data.json ───────────────────────────────────────────────────────────

pub struct TreeBuild {
    pub name: String,
    pub node_count: usize,
    pub data: J,
    /// Viewer-only facts the official format has no room for.
    pub extras: J,
    /// `(category, icon dds path)` for the skills sheets.
    icons: Vec<(String, String)>,
    /// `(frame name, texture path)` for the frame sheet.
    frames: Vec<(String, String)>,
    /// Connector sheets: active, normal, intermediate, ornament1, ornament2.
    connectors: [String; 5],
    /// `(sprite key, texture path)` for the centre ring sheet.
    group_backgrounds: Vec<(String, String)>,
    jewel_radius: Vec<(String, String)>,
    /// Mastery `ActiveEffectImage` paths used by this tree.
    mastery_images: Vec<String>,
    class_sheets: Vec<ClassSheet>,
    /// Every texture path above, for one batched fetch.
    texture_paths: Vec<String>,
}

struct ClassSheet {
    sheet: String,
    images: Vec<(String, String)>,
}

#[derive(Default)]
struct NodeCalc {
    group: usize,
    orbit: u32,
    orbit_index: u32,
    x: f64,
    y: f64,
    out: Vec<u32>,
    incoming: Vec<u32>,
    edge_in: Vec<usize>,
    edge_out: Vec<usize>,
}

fn png_path(dds: &str) -> String {
    if dds.is_empty() {
        return String::new();
    }
    let lower = dds.to_ascii_lowercase();
    if lower.ends_with(".dds") {
        format!("{}.png", &dds[..dds.len() - 4])
    } else {
        format!("{}.png", dds)
    }
}

fn snap(v: f64) -> f64 {
    if v.abs() < 1e-3 { 0.0 } else { v }
}

fn polar(radius: f64, angle_deg: f64) -> (f64, f64) {
    let a = angle_deg.to_radians();
    (snap(radius * a.sin()), snap(-radius * a.cos()))
}

fn hex_colour(rgb: &str) -> String {
    let parts: Vec<u8> = rgb.split(',').filter_map(|p| p.trim().parse::<u8>().ok()).collect();
    if parts.len() == 3 {
        format!("{:02x}{:02x}{:02x}", parts[0], parts[1], parts[2])
    } else {
        String::new()
    }
}

fn rect_json(rect: &str) -> J {
    let parts: Vec<i64> = rect.split(',').filter_map(|p| p.trim().parse::<f64>().ok()).map(|v| v.round() as i64).collect();
    let mut o = J::obj();
    if parts.len() == 4 {
        o.set("x", J::Int(parts[0]));
        o.set("y", J::Int(parts[1]));
        o.set("width", J::Int(parts[2]));
        o.set("height", J::Int(parts[3]));
    }
    o
}

fn pairs_json(pairs: &[(u32, u32)]) -> J {
    if pairs.is_empty() {
        J::Arr(Vec::new())
    } else {
        J::Obj(pairs.iter().map(|(s, o)| (s.to_string(), J::Int(*o as i64))).collect())
    }
}

/// An ascendancy the web export lists: enabled ones by name, disabled ones
/// as a null placeholder. Rows named `[DNT-UNUSED] …` that are not disabled
/// belong to the legacy classes and are dropped altogether.
fn listed_ascendancies(db: &SkillGraphDatabase, character: usize) -> Vec<usize> {
    db.ascendancies
        .iter()
        .enumerate()
        .filter(|(_, a)| a.character == Some(character) && (a.disabled || !a.name.starts_with("[DNT")))
        .map(|(i, _)| i)
        .collect()
}

fn classes_json(db: &SkillGraphDatabase, tables: &tables::ExtraTables) -> (J, Vec<ClassSheet>) {
    let mut classes = Vec::new();
    let mut sheets = Vec::new();
    for (ci, ch) in db.characters.iter().enumerate() {
        let ascs = listed_ascendancies(db, ci);
        let mut o = J::obj();
        o.set("name", J::str(&ch.name));
        o.set("base_str", J::Int(ch.base_strength as i64));
        o.set("base_dex", J::Int(ch.base_dexterity as i64));
        o.set("base_int", J::Int(ch.base_intelligence as i64));
        o.set("image", J::str(&png_path(ch.illustration.as_deref().unwrap_or(""))));
        o.set("image_offset_x", J::num(ch.image_offset.0 as f64));
        o.set("image_offset_y", J::num(ch.image_offset.1 as f64));
        if !ascs.is_empty() {
            let pairs: Vec<(u32, u32)> = tables.class_overrides.iter().filter(|(c, _, _)| *c == ci).map(|(_, s, o)| (*s, *o)).collect();
            o.set("overridePairs", pairs_json(&pairs));
        }
        let mut list = Vec::new();
        let mut images = vec![(format!("class{}:Class0", ch.name), ch.illustration.clone().unwrap_or_default())];
        for &ai in &ascs {
            let a = &db.ascendancies[ai];
            let (ox, oy) = polar(a.tree_region_vector as f64, a.tree_region_angle as f64 + 180.0);
            let mut e = J::obj();
            e.set("id", J::str(&a.id));
            if a.disabled {
                e.set("name", J::Null);
                e.set("image", J::Null);
                e.set("offsetX", J::num(ox));
                e.set("offsetY", J::num(oy));
            } else {
                e.set("name", J::str(&a.name));
                e.set("image", J::str(&png_path(a.illustration.as_deref().unwrap_or(""))));
                e.set("offsetX", J::num(ox));
                e.set("offsetY", J::num(oy));
                e.set("flavourText", J::str(&a.flavour_text));
                e.set("flavourTextColour", J::str(&hex_colour(&a.flavour_text_colour)));
                e.set("flavourTextSize", J::Int(a.flavour_text_size as i64));
                e.set("flavourTextRect", rect_json(&a.coordinate_rect));
                if let Some(img) = &a.illustration {
                    images.push((format!("class{}:Class{}", ch.name, images.len()), img.clone()));
                }
            }
            let pairs: Vec<(u32, u32)> = tables.ascendancy_overrides.iter().filter(|(x, _, _)| *x == ai).map(|(_, s, o)| (*s, *o)).collect();
            e.set("overridePairs", pairs_json(&pairs));
            list.push(e);
        }
        if !ascs.is_empty() {
            sheets.push(ClassSheet { sheet: format!("background-{}", ch.name.to_ascii_lowercase()), images });
        }
        o.set("ascendancies", J::Arr(list));
        classes.push(o);
    }
    (J::Arr(classes), sheets)
}

struct NodeContext<'a> {
    db: &'a SkillGraphDatabase,
    tables: &'a tables::ExtraTables,
    keystones_in_radius: &'a HashMap<u32, u32>,
    choice_parent: &'a HashMap<u32, u32>,
}

/// Attribute points a node grants, counting the combined-attribute stats too.
fn granted_attribute(info: &SkillGraphNodeInfo, stats: &[&str]) -> i64 {
    info.stat_ids.iter().zip(info.stat_values.iter()).filter(|(id, _)| stats.contains(&id.as_str())).map(|(_, v)| *v as i64).sum()
}

const STRENGTH_STATS: [&str; 4] = ["base_strength", "base_strength_and_dexterity", "base_strength_and_intelligence", "additional_all_attributes"];
const DEXTERITY_STATS: [&str; 4] = ["base_dexterity", "base_strength_and_dexterity", "base_dexterity_and_intelligence", "additional_all_attributes"];
const INTELLIGENCE_STATS: [&str; 4] = ["base_intelligence", "base_strength_and_intelligence", "base_dexterity_and_intelligence", "additional_all_attributes"];

/// Stat lines plus the ones the client synthesises from other columns.
fn stat_lines(info: &SkillGraphNodeInfo, ctx: &NodeContext) -> Vec<String> {
    let mut lines = info.stat_texts.clone();
    if info.skill_points_granted > 0 {
        lines.push(format!("Grants {} Passive Skill Point{}", info.skill_points_granted, if info.skill_points_granted == 1 { "" } else { "s" }));
    }
    if info.weapon_points_granted > 0 {
        lines.push(format!("{} Passive Skill Points become [WeaponSetPassiveSkillPoints|Weapon Set Skill Points]", info.weapon_points_granted));
    }
    if let Some(gem) = info.granted_skill.and_then(|g| ctx.tables.granted_skills.get(&g)) {
        lines.push(format!("Grants Skill: <underline>{{{}}}", gem.name));
    }
    lines
}

/// Rows kept in the table for classes not in the game are exported as
/// nameless placeholders: anything named `[DNT…]` or belonging to an
/// ascendancy named that way.
fn is_placeholder(info: &SkillGraphNodeInfo, db: &SkillGraphDatabase) -> bool {
    info.name.starts_with("[DNT")
        || info.ascendancy.and_then(|a| db.ascendancies.get(a)).map(|a| a.name.starts_with("[DNT")).unwrap_or(false)
}

/// One node in the official layout. `calc` is `None` for override nodes,
/// which have no position or links.
fn node_json(gid: u32, info: Option<&SkillGraphNodeInfo>, calc: Option<&NodeCalc>, ctx: &NodeContext) -> J {
    let mut o = J::obj();
    if let Some(info) = info {
        o.set("id", if is_placeholder(info, ctx.db) { J::Null } else { J::str(&info.id) });
    }
    o.set("skill", J::Int(gid as i64));
    if let Some(info) = info.filter(|i| is_placeholder(i, ctx.db)) {
        o.set("name", J::str(""));
        o.set("icon", J::str(""));
        o.set("stats", J::Arr(Vec::new()));
        if let Some(a) = info.ascendancy.and_then(|a| ctx.db.ascendancies.get(a)) {
            o.set("ascendancyId", J::str(&a.id));
        }
        if info.is_ascendancy_start {
            o.set("isAscendancyStart", J::Bool(true));
        }
    }
    if let Some(info) = info.filter(|i| !is_placeholder(i, ctx.db)) {
        o.set("name", J::str(&info.name));
        o.set("icon", J::str(&png_path(info.icon.as_deref().unwrap_or(""))));
        if info.is_keystone {
            o.set("isKeystone", J::Bool(true));
        }
        if info.is_notable {
            o.set("isNotable", J::Bool(true));
        }
        if let Some(k) = ctx.keystones_in_radius.get(&gid) {
            o.set("keystonesInRadius", J::ints([*k as i64]));
        }
        if info.is_jewel_socket {
            o.set("isJewelSocket", J::Bool(true));
        }
        if info.is_mastery {
            o.set("isMastery", J::Bool(true));
            if let Some(img) = info.mastery_group.and_then(|g| ctx.tables.mastery_effect_images.get(&g)) {
                o.set("activeEffectImage", J::str(&png_path(img)));
            }
        }
        if info.is_anointment_only {
            o.set("isBlighted", J::Bool(true));
        }
        if info.hide_connection {
            o.set("hideConnection", J::Bool(true));
        }
        if info.is_free {
            o.set("isFree", J::Bool(true));
        }
        if info.weapon_points_granted > 0 {
            o.set("weaponPassivePointsGranted", J::Int(info.weapon_points_granted as i64));
            o.set("passivePointsGranted", J::Int(-(info.weapon_points_granted as i64)));
        }
        if info.is_attribute {
            o.set("isGenericAttribute", J::Bool(true));
        }
        if let Some(a) = info.ascendancy.and_then(|a| ctx.db.ascendancies.get(a)) {
            o.set("ascendancyId", J::str(&a.id));
        }
        if info.is_ascendancy_start {
            o.set("isAscendancyStart", J::Bool(true));
        }
        if info.is_multiple_choice {
            o.set("isMultipleChoice", J::Bool(true));
        }
        if info.is_multiple_choice_option {
            o.set("isMultipleChoiceOption", J::Bool(true));
        }
        if info.skill_points_granted > 0 {
            o.set("grantedPassivePoints", J::Int(info.skill_points_granted as i64));
        }
        if let Some(gem) = info.granted_skill.and_then(|g| ctx.tables.granted_skills.get(&g)) {
            let mut g = J::obj();
            g.set("realm", J::str("poe2"));
            g.set("name", J::str(""));
            g.set("typeLine", J::str(&gem.name));
            g.set("baseType", J::str(&gem.name));
            if !gem.icon.is_empty() {
                g.set("icon", J::str(&png_path(&gem.icon)));
            }
            o.set("grantedSkill", g);
        }
        if let Some(recipe) = ctx.tables.recipes.get(&gid) {
            o.set("recipe", J::strs(recipe));
        }
        for (key, stats) in [("grantedStrength", &STRENGTH_STATS), ("grantedDexterity", &DEXTERITY_STATS), ("grantedIntelligence", &INTELLIGENCE_STATS)] {
            let v = granted_attribute(info, stats);
            if v != 0 {
                o.set(key, J::Int(v));
            }
        }
        if !info.unlocked_by.is_empty() || info.visible_for_ascendancy.is_some() {
            let mut c = J::obj();
            c.set("nodes", J::ints(info.unlocked_by.iter().map(|g| *g as i64)));
            if let Some(a) = info.visible_for_ascendancy.and_then(|a| ctx.db.ascendancies.get(a)) {
                c.set("ascendancy", J::str(&a.id));
            }
            o.set("unlockConstraint", c);
        }
        o.set("stats", J::strs(stat_lines(info, ctx)));
        if let Some(f) = info.flavour_text.as_deref().filter(|f| !f.is_empty()) {
            o.set("flavourText", J::strs(f.lines().map(|l| l.trim_end())));
        }
        if !info.characters.is_empty() {
            o.set("classStartIndex", J::ints(info.characters.iter().map(|c| *c as i64)));
        }
    }
    if let Some(calc) = calc {
        o.set("group", J::Int(calc.group as i64));
        o.set("orbit", J::Int(calc.orbit as i64));
        o.set("orbitIndex", J::Int(calc.orbit_index as i64));
        o.set("x", J::num(calc.x));
        o.set("y", J::num(calc.y));
        o.set("out", J::strs(calc.out.iter().map(|g| g.to_string())));
        if let Some(parent) = ctx.choice_parent.get(&gid) {
            o.set("multipleChoiceParent", J::Int(*parent as i64));
        }
        o.set("in", J::strs(calc.incoming.iter().map(|g| g.to_string())));
        o.set("edges", J::ints(calc.edge_in.iter().chain(calc.edge_out.iter()).map(|e| *e as i64)));
    }
    o
}

/// Arc centre for a connection on orbit `orbit` (signed ring index) between
/// two node positions, mirroring the client's chord construction.
fn arc_centre(orbit: i32, radii: &[f32; 10], a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let ring = orbit.unsigned_abs() as usize;
    if ring == 0 || ring >= radii.len() {
        return None;
    }
    let r = radii[ring] as f64;
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist <= 0.0 {
        return None;
    }
    let perp = (r * r - dist * dist / 4.0).max(0.0).sqrt() * if orbit > 0 { 1.0 } else { -1.0 };
    Some((a.0 + dx / 2.0 + perp * (dy / dist), a.1 + dy / 2.0 - perp * (dx / dist)))
}

pub fn build_tree(psg: &PsgFile, db: &SkillGraphDatabase, tables: &tables::ExtraTables) -> TreeBuild {
    let radii = psg.orbit_radii();
    let info_of = |gid: u32| db.nodes.get(&gid);

    // Positions: raw group coordinates plus the orbit offset.
    let mut order: Vec<u32> = Vec::new();
    let mut calc: HashMap<u32, NodeCalc> = HashMap::new();
    for (gi, group) in psg.groups.iter().enumerate() {
        for node in &group.nodes {
            let r = radii.get(node.radius as usize).copied().unwrap_or(0.0) as f64;
            let theta = skill_tree_layout::orbit_angle(node.radius, node.position, &psg.passives_per_orbit) as f64;
            let x = group.x as f64 + theta.sin() * r;
            let y = group.y as f64 - theta.cos() * r;
            order.push(node.skill_id);
            calc.insert(node.skill_id, NodeCalc { group: gi + 1, orbit: node.radius, orbit_index: node.position, x, y, ..Default::default() });
        }
    }
    fn pos_of(calc: &HashMap<u32, NodeCalc>, gid: u32) -> Option<(f64, f64)> {
        calc.get(&gid).map(|c| (c.x, c.y))
    }

    // Edges: root links first, then every connection in graph order.
    let mut edges: Vec<J> = Vec::new();
    let mut root_out: Vec<u32> = Vec::new();
    let mut root_edges: Vec<usize> = Vec::new();
    for &r in &psg.roots {
        let idx = edges.len();
        let mut e = J::obj();
        e.set("from", J::str("root"));
        e.set("to", J::Int(r as i64));
        edges.push(e);
        root_out.push(r);
        root_edges.push(idx);
        if let Some(c) = calc.get_mut(&r) {
            c.edge_in.push(idx);
        }
    }
    for group in &psg.groups {
        for node in &group.nodes {
            for conn in &node.connections {
                let idx = edges.len();
                let mut e = J::obj();
                e.set("from", J::Int(node.skill_id as i64));
                e.set("to", J::Int(conn.node_id as i64));
                if conn.orbit == i32::MAX {
                    e.set("orbit", J::Int(0));
                } else if conn.orbit != 0 {
                    if let (Some(a), Some(b)) = (pos_of(&calc, node.skill_id), pos_of(&calc, conn.node_id)) {
                        if let Some((cx, cy)) = arc_centre(conn.orbit, &radii, a, b) {
                            e.set("orbit", J::Int(conn.orbit.unsigned_abs() as i64 - 1));
                            e.set("orbitX", J::num(cx));
                            e.set("orbitY", J::num(cy));
                        }
                    }
                }
                edges.push(e);
                if let Some(c) = calc.get_mut(&node.skill_id) {
                    c.out.push(conn.node_id);
                    c.edge_out.push(idx);
                }
                if let Some(c) = calc.get_mut(&conn.node_id) {
                    c.incoming.push(node.skill_id);
                    c.edge_in.push(idx);
                }
            }
        }
    }

    // keystonesInRadius: the in-range keystone that comes last in PassiveSkills.
    let keystones: Vec<(u32, usize, (f64, f64))> = order
        .iter()
        .filter_map(|&g| info_of(g).filter(|i| i.is_keystone).and_then(|i| pos_of(&calc, g).map(|p| (g, i.row, p))))
        .collect();
    let mut keystones_in_radius: HashMap<u32, u32> = HashMap::new();
    for &g in &order {
        let Some(info) = info_of(g) else { continue };
        if info.is_keystone || info.is_mastery || info.ascendancy.is_some() || !info.characters.is_empty() || info.is_anointment_only {
            continue;
        }
        let Some(p) = pos_of(&calc, g) else { continue };
        let best = keystones
            .iter()
            .filter(|(_, _, k)| ((k.0 - p.0).powi(2) + (k.1 - p.1).powi(2)).sqrt() <= KEYSTONE_RADIUS)
            .max_by_key(|(_, row, _)| *row)
            .map(|(k, _, _)| *k);
        if let Some(k) = best {
            keystones_in_radius.insert(g, k);
        }
    }

    // Multiple-choice options point back at the notable that offers them.
    let mut choice_parent: HashMap<u32, u32> = HashMap::new();
    for &g in &order {
        if !info_of(g).map(|i| i.is_multiple_choice_option).unwrap_or(false) {
            continue;
        }
        let incoming = &calc[&g].incoming;
        let parent = incoming
            .iter()
            .find(|p| info_of(**p).map(|i| i.is_multiple_choice).unwrap_or(false))
            .or_else(|| incoming.first());
        if let Some(p) = parent {
            choice_parent.insert(g, *p);
        }
    }

    let ctx = NodeContext { db, tables, keystones_in_radius: &keystones_in_radius, choice_parent: &choice_parent };

    let (classes, class_sheets) = classes_json(db, tables);

    let mut groups = J::obj();
    for (gi, group) in psg.groups.iter().enumerate() {
        let mut orbits: Vec<u32> = group.nodes.iter().map(|n| n.radius).collect();
        orbits.sort_unstable();
        orbits.dedup();
        let mut g = J::obj();
        g.set("x", J::num(group.x as f64));
        g.set("y", J::num(group.y as f64));
        g.set("orbits", J::ints(orbits.iter().map(|o| *o as i64)));
        g.set("nodes", J::strs(group.nodes.iter().map(|n| n.skill_id.to_string())));
        groups.set(&(gi + 1).to_string(), g);
    }

    let mut nodes = J::obj();
    let mut root = J::obj();
    root.set("group", J::Int(0));
    root.set("orbit", J::Int(0));
    root.set("orbitIndex", J::Int(0));
    root.set("out", J::strs(root_out.iter().map(|g| g.to_string())));
    root.set("in", J::Arr(Vec::new()));
    root.set("edges", J::ints(root_edges.iter().map(|e| *e as i64)));
    nodes.set("root", root);
    for &g in &order {
        nodes.set(&g.to_string(), node_json(g, info_of(g), calc.get(&g), &ctx));
    }

    let mut overrides = J::obj();
    let mut seen = HashSet::new();
    let override_ids = tables.variants.iter().chain(tables.class_overrides.iter().map(|(_, _, o)| o)).chain(tables.ascendancy_overrides.iter().map(|(_, _, o)| o));
    for o in override_ids {
        if seen.insert(*o) {
            overrides.set(&o.to_string(), node_json(*o, info_of(*o), None, &ctx));
        }
    }

    // Bounds come from the group centres, truncated toward zero.
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for g in &psg.groups {
        min_x = min_x.min(g.x as f64);
        min_y = min_y.min(g.y as f64);
        max_x = max_x.max(g.x as f64);
        max_y = max_y.max(g.y as f64);
    }
    if psg.groups.is_empty() {
        (min_x, min_y, max_x, max_y) = (0.0, 0.0, 0.0, 0.0);
    }

    let mut data = J::obj();
    data.set("tree", J::str(&tables.tree_name));
    data.set("classes", classes);
    data.set("groups", groups);
    data.set("nodes", nodes);
    data.set("edges", J::Arr(edges));
    data.set("skillOverrides", overrides);
    data.set("jewelSlots", J::ints(tables.jewel_slots.iter().map(|g| *g as i64)));
    data.set("min_x", J::Int(min_x.trunc() as i64));
    data.set("min_y", J::Int(min_y.trunc() as i64));
    data.set("max_x", J::Int(max_x.trunc() as i64));
    data.set("max_y", J::Int(max_y.trunc() as i64));

    // ── Sprite inventory ────────────────────────────────────────────
    let mut icons: Vec<(String, String)> = Vec::new();
    let mut icon_seen = HashSet::new();
    let mut mastery_images: Vec<String> = Vec::new();
    for gid in order.iter().copied().chain(seen.iter().copied()) {
        let Some(info) = info_of(gid) else { continue };
        let category = if info.is_keystone {
            "keystone"
        } else if info.is_notable {
            "notable"
        } else if info.is_mastery {
            "mastery"
        } else {
            "normal"
        };
        if let Some(icon) = info.icon.as_deref().filter(|i| !i.is_empty()) {
            if icon_seen.insert((category, icon.to_string())) {
                icons.push((category.to_string(), icon.to_string()));
            }
        }
        if let Some(img) = info.mastery_group.and_then(|g| tables.mastery_effect_images.get(&g)) {
            if !mastery_images.iter().any(|m| m.eq_ignore_ascii_case(img)) {
                mastery_images.push(img.clone());
            }
        }
    }
    let category_rank = |c: &str| ICON_SIZES.iter().position(|(n, _, _)| *n == c).unwrap_or(9);
    icons.sort_by(|a, b| category_rank(&a.0).cmp(&category_rank(&b.0)).then_with(|| a.1.cmp(&b.1)));
    mastery_images.sort();

    let tree_context = tree_context_for_graph_type(psg.graph_type);
    let art_set = db.art_sets.get(tree_context);
    let frames = frame_list(db, art_set);
    let connectors = art_set
        .map(|a| [a.connection.active.clone(), a.connection.normal.clone(), a.connection.intermediate.clone(), a.connection.ornament1.clone(), a.connection.ornament2.clone()])
        .unwrap_or_default();

    let mut group_backgrounds = Vec::new();
    if psg.graph_type == 0 {
        group_backgrounds.push(("startNode:MainCircleActive".to_string(), MAIN_CIRCLE_ACTIVE_FULL.to_string()));
        group_backgrounds.push(("startNode:MainCircle".to_string(), crate::ui::psg_viewer::MAIN_CIRCLE.to_string()));
    }
    if let Some(a) = art_set {
        for path in [&a.group_background.small, &a.group_background.medium, &a.group_background.large] {
            if !path.is_empty() {
                group_backgrounds.push((format!("groupBackground:{}", art_name(path)), path.clone()));
            }
        }
    }

    let mut jewel_radius: Vec<(String, String)> = tables
        .jewel_radius_art
        .iter()
        .map(|p| (format!("jewelRadius:{}", art_name(p).replace("inverse", "Inverse")), p.clone()))
        .collect();
    for (name, path) in ORACLE_JEWEL_CIRCLES {
        jewel_radius.push((format!("jewelRadius:{}", name), path.to_string()));
    }

    let mut texture_paths: Vec<String> = Vec::new();
    texture_paths.extend(icons.iter().map(|(_, p)| p.clone()));
    texture_paths.extend(frames.iter().map(|(_, p)| p.clone()));
    texture_paths.extend(connectors.iter().cloned());
    texture_paths.extend(group_backgrounds.iter().map(|(_, p)| p.clone()));
    texture_paths.push(BACKGROUND_TILE.to_string());
    texture_paths.extend(JEWEL_SOCKET_ART.iter().map(|(_, f)| format!("Art/2DArt/UIImages/InGame/{}", f)));
    texture_paths.extend(jewel_radius.iter().map(|(_, p)| p.clone()));
    texture_paths.extend(mastery_images.iter().cloned());
    for c in &class_sheets {
        texture_paths.extend(c.images.iter().map(|(_, p)| p.clone()));
    }
    texture_paths.retain(|p| !p.is_empty());

    let extras = viewer_extras(psg, db, &radii, art_set.map(|a| &a.group_background));
    let node_count = order.len();
    TreeBuild {
        name: tables.tree_name.clone(),
        node_count,
        data,
        extras,
        icons,
        frames,
        connectors,
        group_backgrounds,
        jewel_radius,
        mastery_images,
        class_sheets,
        texture_paths,
    }
}

/// Sprite name for a UI texture: the file name without the game's
/// `PassiveSkillScreen` prefix (`…/PassiveSkillScreenVaalJewelCircle1` → `VaalJewelCircle1`).
fn art_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    let base = base.strip_suffix(".dds").unwrap_or(base);
    base.strip_prefix("PassiveSkillScreen").unwrap_or(base).to_string()
}

/// Frame textures with the names the official `frame` sheet uses.
fn frame_list(db: &SkillGraphDatabase, art_set: Option<&crate::ui::skill_tree_art::SkillTreeArtSet>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let push = |name: &str, path: &str, out: &mut Vec<(String, String)>| {
        if !path.is_empty() && !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), path.to_string()));
        }
    };
    let push3 = |names: [&str; 3], f: &FrameArt, out: &mut Vec<(String, String)>| {
        push(names[0], &f.normal, out);
        push(names[1], &f.can_allocate, out);
        push(names[2], &f.active, out);
    };
    let by_kind: [(NodeFrameKind, [&str; 3]); 5] = [
        (NodeFrameKind::Keystone, ["KeystoneFrameUnallocated", "KeystoneFrameCanAllocate", "KeystoneFrameAllocated"]),
        (NodeFrameKind::Notable, ["NotableFrameUnallocated", "NotableFrameCanAllocate", "NotableFrameAllocated"]),
        (NodeFrameKind::Jewel, ["JewelFrameUnallocated", "JewelFrameCanAllocate", "JewelFrameAllocated"]),
        (NodeFrameKind::Passive, ["PSSkillFrame", "PSSkillFrameHighlighted", "PSSkillFrameActive"]),
        (NodeFrameKind::MultipleChoice, ["MultipleChoiceFrameUnallocated", "MultipleChoiceFrameCanAllocate", "MultipleChoiceFrameAllocated"]),
    ];
    if let Some(set) = art_set {
        for (kind, names) in by_kind {
            if let Some(f) = set.frames.get(&kind) {
                push3(names, f, &mut out);
            }
        }
        if let Some(f) = set.frames.get(&NodeFrameKind::AscendancyStart) {
            push("AscendancyStartNode", &f.normal, &mut out);
        }
    }
    let by_id: [(&str, [&str; 3]); 8] = [
        ("AscendancyNotable", ["AscendancyFrameNotableUnallocated", "AscendancyFrameNotableCanAllocate", "AscendancyFrameNotableAllocated"]),
        ("AscendancySmall", ["AscendancyFrameNormalUnallocated", "AscendancyFrameNormalCanAllocate", "AscendancyFrameNormalAllocated"]),
        ("DruidOracleAscendancyNotable", ["OracleFrameNotableUnallocated", "OracleFrameNotableCanAllocate", "OracleFrameNotableAllocated"]),
        ("DruidOracleAscendancySmall", ["OracleFrameUnallocated", "OracleFrameCanAllocate", "OracleFrameAllocated"]),
        ("DruidOracleAscendancyKeystone", ["OracleKeystoneFrameUnallocated", "OracleKeystoneFrameCanAllocate", "OracleKeystoneFrameAllocated"]),
        ("BlightedNotable", ["BlightedNotableFrameUnallocated", "BlightedNotableFrameCanAllocate", "BlightedNotableFrameAllocated"]),
        ("CharacterDeliriumAnointNotable", ["AnointNotableFrameUnallocated", "AnointNotableFrameCanAllocate", "AnointNotableFrameAllocated"]),
        ("AscendancyStart", ["AscendancyStartNode", "AscendancyStartNode", "AscendancyStartNode"]),
    ];
    for (id, names) in by_id {
        if let Some(f) = db.node_frames.iter().find(|f| f.id == id) {
            push3(names, f, &mut out);
            if id == "AscendancyNotable" {
                push("AscendancyFrameNotableBacking", &f.normal.replace("Normal", "Backing"), &mut out);
            }
            if id == "AscendancySmall" {
                push("AscendancyFrameNormalBacking", &f.normal.replace("Normal", "Backing"), &mut out);
            }
        }
    }
    push("AttributeFrameUnallocated", crate::ui::psg_viewer::PLUS_FRAME_NORMAL, &mut out);
    push("AttributeFrameCanAllocate", PLUS_FRAME_CAN_ALLOCATE, &mut out);
    push("AttributeFrameAllocated", crate::ui::psg_viewer::PLUS_FRAME_ACTIVE, &mut out);
    out
}

fn viewer_extras(psg: &PsgFile, db: &SkillGraphDatabase, radii: &[f32; 10], group_art: Option<&crate::ui::skill_tree_art::GroupBackground>) -> J {
    let mut extras = J::obj();
    extras.set("tree", J::Int(psg.graph_type as i64));
    extras.set("orbitRadii", J::ints(radii.iter().map(|r| *r as i64)));
    extras.set("skillsPerOrbit", J::ints(psg.passives_per_orbit.iter().map(|n| *n as i64)));
    extras.set("keystoneRadius", J::num(KEYSTONE_RADIUS));

    let mut backgrounds = J::obj();
    for (gi, group) in psg.groups.iter().enumerate() {
        if group.background_type == 0 && group.background_flag == 0 {
            continue;
        }
        let path = match (group.background_type, group_art) {
            (2, Some(a)) => &a.small,
            (4, Some(a)) => &a.medium,
            (_, Some(a)) => &a.large,
            _ => continue,
        };
        if !path.is_empty() {
            backgrounds.set(&(gi + 1).to_string(), J::str(&art_name(path)));
        }
    }
    extras.set("groupBackgrounds", backgrounds);

    let mut plates = J::obj();
    for group in &psg.groups {
        for node in &group.nodes {
            let Some(info) = db.nodes.get(&node.skill_id) else { continue };
            if !info.is_ascendancy_start {
                continue;
            }
            let Some(a) = info.ascendancy.and_then(|a| db.ascendancies.get(a)) else { continue };
            let nudge = skill_tree_layout::plate_nudge(&a.id);
            plates.set(&a.id, J::Arr(vec![J::num(group.x as f64 - nudge.x as f64), J::num(group.y as f64 - nudge.y as f64)]));
        }
    }
    extras.set("ascendancyPlates", plates);

    let mut starts = J::obj();
    for &root in &psg.roots {
        if let Some(info) = db.nodes.get(&root) {
            for c in &info.characters {
                starts.set(&c.to_string(), J::Int(root as i64));
            }
        }
    }
    extras.set("classStart", starts);

    let mut sizes = J::obj();
    sizes.set("classIllustration", J::num(CLASS_ILLUSTRATION_SIZE as f64));
    sizes.set("mainCircle", J::num(MAIN_CIRCLE_SIZE as f64));
    sizes.set("ascendancyPlate", J::num(ASCENDANCY_PLATE_SIZE as f64));
    extras.set("sizes", sizes);
    extras
}

// ── Sprite sheets ───────────────────────────────────────────────────────

fn icon_sprites(store: &mut TextureStore, icons: &[(String, String)]) -> (Vec<(String, RgbaImage)>, Vec<(String, RgbaImage)>) {
    let mut active = Vec::new();
    let mut inactive = Vec::new();
    for (category, path) in icons {
        let Some((_, w, h)) = ICON_SIZES.iter().find(|(c, _, _)| c == category) else { continue };
        let Some(img) = store.get(path) else { continue };
        let scaled = sheets::resize(img, *w, *h);
        let png = png_path(path);
        inactive.push((format!("{}Inactive:{}", category, png), sheets::disabled_icon(&scaled)));
        active.push((format!("{}Active:{}", category, png), scaled));
    }
    (active, inactive)
}

fn frame_sprites(store: &mut TextureStore, frames: &[(String, String)]) -> Vec<(String, RgbaImage)> {
    let mut out: Vec<(String, RgbaImage)> = frames
        .iter()
        .filter_map(|(name, path)| store.get(path).map(|img| (format!("frame:{}", name), sheets::scale(img, 0.5))))
        .collect();
    out.sort_by(|a, b| b.1.height().cmp(&a.1.height()).then_with(|| a.0.cmp(&b.0)));
    out
}

fn scaled_sprites(store: &mut TextureStore, items: &[(String, String)], factor: f32) -> Vec<(String, RgbaImage)> {
    items.iter().filter_map(|(key, path)| store.get(path).map(|img| (key.clone(), sheets::scale(img, factor)))).collect()
}

fn mastery_sprites(store: &mut TextureStore, images: &[String]) -> (Vec<(String, RgbaImage)>, Vec<(String, RgbaImage)>) {
    let mut active = Vec::new();
    let mut inactive = Vec::new();
    for path in images {
        let Some(img) = store.get(path) else { continue };
        let h = (img.height() as f32 * MASTERY_PATTERN_WIDTH as f32 / img.width().max(1) as f32).round() as u32;
        let scaled = sheets::resize(img, MASTERY_PATTERN_WIDTH, h);
        let png = png_path(path);
        inactive.push((format!("masteryEffectInactive:{}", png), sheets::disabled_mastery(&scaled)));
        active.push((format!("masteryEffectActive:{}", png), scaled));
    }
    (active, inactive)
}

/// The `line` sheet: the active, normal and intermediate connector sheets
/// stacked at half scale, each block carrying the straight strip and the
/// nine quarter-arc frames anchored to its bottom-right corner, then the
/// two ornaments.
fn line_sheet(store: &mut TextureStore, connectors: &[String; 5]) -> Option<(RgbaImage, Vec<sheets::Frame>)> {
    let blocks: Vec<(&str, &[u32; 9], RgbaImage)> = [("Active", &ARC_SIZES_ACTIVE), ("Normal", &ARC_SIZES_NORMAL), ("Intermediate", &ARC_SIZES_NORMAL)]
        .iter()
        .zip(connectors.iter())
        .filter_map(|((label, sizes), path)| store.get(path).map(|img| (*label, *sizes, sheets::scale(img, 0.5))))
        .collect();
    if blocks.is_empty() {
        return None;
    }
    let ornaments: Vec<(&str, RgbaImage)> = [("PSLineDeco", &connectors[3]), ("PSLineDecoHighlighted", &connectors[4])]
        .iter()
        .filter_map(|(name, path)| store.get(path).map(|img| (*name, sheets::scale(img, 0.5))))
        .collect();

    let width = blocks.iter().map(|(_, _, b)| b.width()).chain(ornaments.iter().map(|(_, o)| o.width())).max().unwrap_or(1);
    let height: u32 = blocks.iter().map(|(_, _, b)| b.height()).sum::<u32>() + ornaments.iter().map(|(_, o)| o.height() + 1).sum::<u32>();
    let mut image = RgbaImage::new(width, height);
    let mut frames = Vec::new();
    let mut y = 0u32;
    for (label, sizes, block) in &blocks {
        image::imageops::overlay(&mut image, block, 0, y as i64);
        let (bw, bh) = (block.width(), block.height());
        let ratio = bw as f32 / 718.0;
        frames.push(sheets::Frame { key: format!("line:LineConnector{}", label), x: 0, y: y + (LINE_STRIP.0 as f32 * ratio) as u32, w: bw, h: (LINE_STRIP.1 as f32 * ratio).round().max(1.0) as u32 });
        for (orbit, size) in sizes.iter().enumerate() {
            let s = ((*size as f32) * ratio).round() as u32;
            frames.push(sheets::Frame { key: format!("line:Orbit{}{}", orbit + 1, label), x: bw - s, y: y + bh - s, w: s, h: s });
        }
        y += bh;
    }
    for (name, orn) in &ornaments {
        image::imageops::overlay(&mut image, orn, 0, y as i64);
        frames.push(sheets::Frame { key: format!("line:{}", name), x: 0, y, w: orn.width(), h: orn.height() });
        y += orn.height() + 1;
    }
    Some((image, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_centre_side_follows_orbit_sign() {
        let radii = [0.0, 82.0, 164.0, 334.0, 488.0, 657.0, 839.0, 250.0, 1076.0, 1320.0];
        let a = (0.0, 0.0);
        let b = (100.0, 0.0);
        let plus = arc_centre(2, &radii, a, b).unwrap();
        let minus = arc_centre(-2, &radii, a, b).unwrap();
        assert!((plus.0 - 50.0).abs() < 1e-9 && plus.1 < 0.0);
        assert!((minus.0 - 50.0).abs() < 1e-9 && minus.1 > 0.0);
        let r = ((plus.0 - a.0).powi(2) + (plus.1 - a.1).powi(2)).sqrt();
        assert!((r - 164.0).abs() < 1e-6);
        assert!(arc_centre(0, &radii, a, b).is_none());
        // A chord longer than the diameter still yields the midpoint.
        let far = arc_centre(1, &radii, a, (400.0, 0.0)).unwrap();
        assert!((far.0 - 200.0).abs() < 1e-9 && far.1.abs() < 1e-9);
    }

    #[test]
    fn helpers_format_like_the_official_export() {
        assert_eq!(png_path("Art/2DArt/SkillIcons/passives/damage.dds"), "Art/2DArt/SkillIcons/passives/damage.png");
        assert_eq!(png_path("Art/x/Pattern"), "Art/x/Pattern.png");
        assert_eq!(png_path(""), "");
        assert_eq!(hex_colour("246,184,138"), "f6b88a");
        let (x, y) = polar(1332.0, 240.0 + 180.0);
        assert!((x - 1153.55).abs() < 0.01 && (y + 666.0).abs() < 0.01);
        assert_eq!(polar(1332.0, 180.0), (0.0, 1332.0));
        assert_eq!(art_name("Art/2DArt/UIImages/InGame/PassiveSkillScreenVaalJewelCircle1"), "VaalJewelCircle1");
        assert_eq!(art_name("Art/2DArt/UIImages/InGame/PassiveSkillScreenGroupBackgroundLargeHalf.dds"), "GroupBackgroundLargeHalf");
        let r = rect_json("0,-600,1000,500");
        assert_eq!(r.get("y"), Some(&J::Int(-600)));
        assert_eq!(r.get("width"), Some(&J::Int(1000)));
    }

    #[test]
    fn tree_json_from_synthetic_graph() {
        use crate::dat::psg::{PsgConnection, PsgGroup, PsgNode};
        let psg = PsgFile {
            graph_type: 0,
            roots: vec![1],
            passives_per_orbit: vec![1, 12, 24, 24, 72, 72, 72, 24, 72, 144],
            groups: vec![
                PsgGroup { x: 100.0, y: 200.0, is_proxy: false, background_type: 0, background_flag: 0, nodes: vec![PsgNode { skill_id: 1, radius: 0, position: 0, connections: vec![PsgConnection { node_id: 2, orbit: 0 }, PsgConnection { node_id: 3, orbit: -2 }] }] },
                PsgGroup { x: 300.0, y: 200.0, is_proxy: false, background_type: 0, background_flag: 0, nodes: vec![
                    PsgNode { skill_id: 2, radius: 1, position: 3, connections: vec![PsgConnection { node_id: 3, orbit: i32::MAX }] },
                    PsgNode { skill_id: 3, radius: 1, position: 9, connections: vec![] },
                ] },
            ],
        };
        let db = SkillGraphDatabase::default();
        let tables = tables::ExtraTables { tree_name: "Test".into(), ..Default::default() };
        let tree = build_tree(&psg, &db, &tables);
        let text = json::to_string_pretty(&tree.data);
        assert!(text.starts_with("{\n    \"tree\": \"Test\","));
        let nodes = tree.data.get("nodes").unwrap();
        let root = nodes.get("root").unwrap();
        assert_eq!(root.get("out"), Some(&J::strs(["1"])));
        assert_eq!(root.get("edges"), Some(&J::ints([0i64])));
        let n2 = nodes.get("2").unwrap();
        // orbit 1 (radius 82), position 3 of 12 = 90 degrees clockwise from north
        assert_eq!(n2.get("x"), Some(&J::Int(382)));
        assert_eq!(n2.get("y"), Some(&J::Int(200)));
        assert_eq!(n2.get("in"), Some(&J::strs(["1"])));
        assert_eq!(n2.get("edges"), Some(&J::ints([1i64, 3])));
        let n1 = nodes.get("1").unwrap();
        assert_eq!(n1.get("edges"), Some(&J::ints([0i64, 1, 2])));
        let edges = match tree.data.get("edges") { Some(J::Arr(e)) => e, _ => panic!() };
        assert_eq!(edges.len(), 4);
        assert_eq!(edges[2].get("orbit"), Some(&J::Int(1)));
        assert!(edges[2].get("orbitX").is_some());
        assert_eq!(edges[3].get("orbit"), Some(&J::Int(0)));
        assert!(edges[3].get("orbitX").is_none());
        assert_eq!(tree.data.get("min_x"), Some(&J::Int(100)));
        assert_eq!(tree.data.get("max_x"), Some(&J::Int(300)));
        assert_eq!(tree.data.get("groups").unwrap().get("2").unwrap().get("orbits"), Some(&J::ints([1i64])));
    }
}

#[cfg(test)]
mod real_data_tests {
    use super::*;
    use crate::bundles::index::Index as BundleIndex;

    /// Dumps the DAT tables named in `TREE_TABLES` (comma separated) as JSON
    /// into `TREE_EXPORT_OUT`, for checking a column by hand.
    #[test]
    #[ignore]
    fn dump_tables() {
        use crate::ui::export_window::{DataFormat, ExportSettings};
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json")).unwrap();
        let schema: Schema = serde_json::from_str(&schema_text).unwrap();
        let out_dir = PathBuf::from(std::env::var("TREE_EXPORT_OUT").expect("TREE_EXPORT_OUT"));
        let hashes: Vec<u64> = std::env::var("TREE_TABLES")
            .unwrap_or_default()
            .split(',')
            .filter_map(|t| {
                let t = t.trim();
                let path = if t.contains('/') { t.to_string() } else { format!("data/balance/{}.datc64", t.to_ascii_lowercase()) };
                index.files.iter().find(|(_, f)| f.path.eq_ignore_ascii_case(&path)).map(|(h, _)| *h)
            })
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let settings = ExportSettings { data_format: DataFormat::Json, is_poe2: true, ..ExportSettings::default() };
        crate::export::run_export(hashes, Some(reader), Some(index), settings, out_dir, None, None, Some(schema), tx, None);
        while let Ok(status) = rx.try_recv() {
            if let ExportStatus::Complete { message, .. } = status {
                println!("{}", message);
            }
        }
    }

    /// Prints stat ids, values and rendered text for the graph ids in
    /// `TREE_NODES` (comma separated), for checking translations by hand.
    /// `TREE_NODES=12033,15606 cargo test --release dump_node_stats -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_node_stats() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json")).unwrap();
        let schema: Schema = serde_json::from_str(&schema_text).unwrap();
        let db = build_skill_graph_db(Some(&reader), &index, None, &schema).unwrap();
        for id in std::env::var("TREE_NODES").unwrap_or_default().split(',').filter_map(|s| s.trim().parse::<u32>().ok()) {
            match db.nodes.get(&id) {
                Some(n) => println!("{} {:?}\n   ids {:?}\n   values {:?}\n   text {:?}", id, n.name, n.stat_ids, n.stat_values, n.stat_texts),
                None => println!("{} not found", id),
            }
        }
    }

    /// Exports the real character tree and, when `OFFICIAL_TREE_JSON` points
    /// at GGG's `data.json`, reports how closely the two agree.
    /// `OFFICIAL_TREE_JSON=S:/.../data.json cargo test --release real_tree_export -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_tree_export() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json")).unwrap();
        let schema: Schema = serde_json::from_str(&schema_text).unwrap();
        let source = TreeExportSource { reader: Some(reader), index, steam: None, schema };

        let psg_path = "metadata/passiveskillgraph.psg";
        let psg = crate::dat::psg::parse_psg(&source.fetch(psg_path).expect("psg")).unwrap();
        let out_dir = std::env::var("TREE_EXPORT_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("ggpk_tree_export_test"));
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::time::Instant::now();
        run_tree_export(source, psg_path.to_string(), psg, None, TreeExportOptions::default(), out_dir.clone(), tx);
        let mut done = false;
        while let Ok(status) = rx.try_recv() {
            match status {
                ExportStatus::Progress { current, total, filename } => println!("[{}/{}] {}", current, total, filename),
                ExportStatus::Complete { message, .. } => {
                    println!("{} in {:?}", message, t.elapsed());
                    done = true;
                }
                ExportStatus::Error(e) => panic!("export failed: {}", e),
            }
        }
        assert!(done);

        let Ok(official_path) = std::env::var("OFFICIAL_TREE_JSON") else { return };
        let ours: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out_dir.join("data.json")).unwrap()).unwrap();
        let official: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&official_path).unwrap()).unwrap();
        let (on, tn) = (&official["nodes"], &ours["nodes"]);
        let official_nodes = on.as_object().unwrap();
        let our_nodes = tn.as_object().unwrap();
        println!("nodes: ours {} official {}", our_nodes.len(), official_nodes.len());
        let mut pos_diff_max = 0.0f64;
        let mut missing = 0;
        let mut key_mismatch: HashMap<String, usize> = HashMap::new();
        let mut stat_mismatch = 0;
        let mut stat_examples = Vec::new();
        for (id, node) in official_nodes {
            let Some(mine) = our_nodes.get(id) else {
                missing += 1;
                continue;
            };
            if let (Some(x), Some(mx)) = (node["x"].as_f64(), mine["x"].as_f64()) {
                pos_diff_max = pos_diff_max.max((x - mx).abs()).max((node["y"].as_f64().unwrap() - mine["y"].as_f64().unwrap()).abs());
            }
            for key in ["keystonesInRadius", "recipe", "isBlighted", "hideConnection", "isFree", "grantedPassivePoints", "activeEffectImage", "unlockConstraint", "classStartIndex", "ascendancyId", "grantedStrength", "grantedDexterity", "grantedIntelligence", "multipleChoiceParent", "edges", "out", "in", "flavourText", "icon", "name", "id", "isGenericAttribute"] {
                if node.get(key) != mine.get(key) {
                    *key_mismatch.entry(key.to_string()).or_default() += 1;
                }
            }
            if node.get("stats") != mine.get("stats") {
                stat_mismatch += 1;
                if stat_examples.len() < 5 {
                    stat_examples.push(format!("{} {}: official {:?} ours {:?}", id, node["name"], node["stats"], mine["stats"]));
                }
            }
        }
        println!("missing nodes: {}, max position diff: {:.3}", missing, pos_diff_max);
        let mut keys: Vec<_> = key_mismatch.into_iter().collect();
        keys.sort();
        println!("field mismatches: {:?}", keys);
        println!("stats mismatches: {}", stat_mismatch);
        for e in stat_examples {
            println!("  {}", e);
        }
        println!("edges: ours {} official {}", ours["edges"].as_array().unwrap().len(), official["edges"].as_array().unwrap().len());
        println!("skillOverrides: ours {} official {}", ours["skillOverrides"].as_object().map(|o| o.len()).unwrap_or(0), official["skillOverrides"].as_object().map(|o| o.len()).unwrap_or(0));
        println!("jewelSlots equal: {}", ours["jewelSlots"] == official["jewelSlots"]);
        println!("classes: ours {:?}", ours["classes"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap_or("").to_string(), c["ascendancies"].as_array().map(|a| a.len()).unwrap_or(0))).collect::<Vec<_>>());
        println!("classes: official {:?}", official["classes"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap_or("").to_string(), c["ascendancies"].as_array().map(|a| a.len()).unwrap_or(0))).collect::<Vec<_>>());
        for key in ["min_x", "min_y", "max_x", "max_y", "tree"] {
            println!("{}: ours {} official {}", key, ours[key], official[key]);
        }
    }
}
