//! The passive skill graphs as the game describes them: node data and art
//! from the DAT tables, and where the client lays out each node. The viewer
//! draws from here and the tree exports write it out.

pub mod art;
pub mod atlas_node_db;
pub mod geom;
pub mod layout;
pub mod ui_atlas;

use crate::bundles::extract::{extract_bundle_file_sync, find_file_info_by_path};
use crate::bundles::index::Index;
use crate::bundles::steam::SteamBundleLoader;
use crate::dat::psg::PsgFile;
use crate::dat::schema::Schema;
use crate::ggpk::reader::GgpkReader;

/// Every DDS path the given `.psg`'s art (icons + tree-context frames,
/// connectors, group backgrounds) will need, deduplicated. Only 771 unique
/// node icons exist across the *entire* game's passive skill data, so a
/// single tree's subset is small enough to bulk-request in one go.
pub fn collect_needed_texture_paths(
    psg: &PsgFile,
    db: &atlas_node_db::SkillGraphDatabase,
) -> Vec<String> {
    let tree_context = atlas_node_db::tree_context_for_graph_type(psg.graph_type);
    let mut paths = Vec::new();

    let push_art_set = |art: &art::SkillTreeArtSet, paths: &mut Vec<String>| {
        paths.push(art.group_background.small.clone());
        paths.push(art.group_background.medium.clone());
        paths.push(art.group_background.large.clone());
        paths.push(art.connection.normal.clone());
        paths.push(art.connection.active.clone());
        for frame in art.frames.values() {
            paths.push(frame.normal.clone());
            paths.push(frame.active.clone());
        }
    };

    match psg.graph_type {
        1 => {
            paths.push(atlas_node_db::ATLAS_MAIN_TREE_BG_PATH.to_string());
            paths.push(art::ATLAS_START.to_string());
            for d in &db.decorators {
                paths.push(d.background.clone());
                paths.push(d.blocked.clone());
            }
        }
        2 => {
            paths.push(art::BREACH_BACKDROP.to_string());
            paths.push(art::BREACH_START.to_string());
        }
        _ => {
            paths.push(art::MAIN_CIRCLE.to_string());
            paths.push(art::MAIN_CIRCLE_ACTIVE.to_string());
            paths.push(art::PLUS_FRAME_NORMAL.to_string());
            paths.push(art::PLUS_FRAME_ACTIVE.to_string());
            for &c in &db.playable_characters() {
                let ch = &db.characters[c];
                if let Some(i) = &ch.illustration {
                    paths.push(i.clone());
                }
            }
            for (i, a) in db.ascendancies.iter().enumerate() {
                if !a.is_enabled() {
                    continue;
                }
                if let Some(img) = &a.illustration {
                    paths.push(img.clone());
                }
                if let Some(art) = db.ui_art_for_ascendancy(i) {
                    push_art_set(art, &mut paths);
                }
            }
            for frame in &db.node_frames {
                paths.push(frame.normal.clone());
                paths.push(frame.active.clone());
            }
        }
    }

    if let Some(art) = db.art_sets.get(tree_context) {
        push_art_set(art, &mut paths);
    }

    for group in &psg.groups {
        for node in &group.nodes {
            if let Some(info) = db.nodes.get(&node.skill_id) {
                if let Some(icon) = &info.icon {
                    paths.push(icon.clone());
                }
                if let Some(pattern) = info.mastery_group.and_then(|g| db.mastery_effect_images.get(&g)) {
                    paths.push(pattern.clone());
                }
                if let Some((bg, _, _)) = &info.atlas_subtree_background {
                    paths.push(bg.clone());
                }
                if let Some(icon) = &info.atlas_subtree_icon {
                    paths.push(icon.clone());
                }
            }
        }
    }

    paths.retain(|p| !p.is_empty());
    paths.sort();
    paths.dedup();
    paths
}

/// Fetches the DAT/CSD files needed to resolve skill graph nodes (passive
/// tree, atlas, and league/Brequel trees all share the `PassiveSkills` table,
/// just with different stat-description sources) and builds the resolved
/// database. Runs on a background thread — these files total several MB and
/// parsing them synchronously would stall a frame.
pub fn build_skill_graph_db(
    reader: Option<&GgpkReader>,
    index: &Index,
    steam_loader: Option<&SteamBundleLoader>,
    schema: &Schema,
) -> Result<atlas_node_db::SkillGraphDatabase, String> {
    let is_poe2 = crate::data_export::game_from_index(index).map(|g| g.is_poe2()).unwrap_or(true);
    let fetch = |path: &str| {
        find_file_info_by_path(index, path).and_then(|info| extract_bundle_file_sync(info, index, reader, steam_loader))
    };
    build_skill_graph_db_from(&fetch, schema, is_poe2, false)
}

/// `build_skill_graph_db` over any source of game files. `hardmode` reads
/// PoE 1's passives as Ruthless has them.
pub fn build_skill_graph_db_from(
    fetch_file: &dyn Fn(&str) -> Option<Vec<u8>>,
    schema: &Schema,
    is_poe2: bool,
    hardmode: bool,
) -> Result<atlas_node_db::SkillGraphDatabase, String> {
    let fetch = |path: &str| -> Result<Vec<u8>, String> {
        fetch_file(path).ok_or_else(|| format!("Could not read {}", path))
    };
    let fetch_optional = |path: &str| -> Option<Vec<u8>> { fetch_file(path) };
    // PoE 2 keeps its tables under `data/balance/` and its descriptions as
    // `.csd`; PoE 1 keeps both one level up.
    let table = |name: &str| match is_poe2 {
        true => format!("data/balance/{}.datc64", name),
        false => format!("data/{}.datc64", name),
    };
    let passiveskills_bytes = fetch(&table("passiveskills"))?;
    let stats_bytes = fetch(&table("stats"))?;

    // Covers all three known graph types: character/ascendancy, atlas, and
    // Brequel (Chayula league tree). Later files redefine earlier entries,
    // so the generic `stat_descriptions.csd` goes before the passive-tree
    // files; the atlas files come first so they never shadow passive text.
    let csd_paths: Vec<String> = match is_poe2 {
        true => ["atlas_stat_descriptions", "atlas_variant_stat_descriptions", "stat_descriptions", "passive_skill_stat_descriptions", "passive_skill_variant_stat_descriptions"]
            .iter()
            .map(|n| format!("data/statdescriptions/{}.csd", n))
            .collect(),
        // A node states what it grants you; the aura file rewords the same
        // stats as what nearby enemies or allies get, and belongs to buffs.
        false => ["atlas_stat_descriptions", "stat_descriptions", "passive_skill_stat_descriptions"]
            .iter()
            .map(|n| format!("metadata/statdescriptions/{}.txt", n))
            .collect(),
    };
    let mut stat_csd_sources = Vec::new();
    for path in csd_paths {
        // A tree file one game does not ship simply adds nothing.
        let Some(bytes) = fetch_optional(&path) else { continue };
        stat_csd_sources.push(atlas_node_db::StatCsdSource { path, bytes });
    }
    if stat_csd_sources.is_empty() {
        return Err("no passive tree stat descriptions found".to_string());
    }

    let extra = atlas_node_db::ExtraTables {
        ascendancy: fetch_optional(&table("ascendancy")),
        descendancy: fetch_optional(&table("descendancy")),
        mastery_effects: fetch_optional(&table("passiveskillmasteryeffects")),
        reminder_text: fetch_optional(&table("remindertext")),
        aura_descriptions: fetch_optional("metadata/statdescriptions/passive_skill_aura_stat_descriptions.txt").map(|bytes| {
            atlas_node_db::StatCsdSource {
                path: "metadata/statdescriptions/passive_skill_aura_stat_descriptions.txt".to_string(),
                bytes,
            }
        }),
        buff_templates: fetch_optional(&table("bufftemplates")),
        buff_definitions: fetch_optional(&table("buffdefinitions")),
        atlas_subtrees: fetch_optional(&table("atlaspassiveskillsubtrees")),
        characters: fetch_optional(&table("characters")),
        decorators: fetch_optional(&table("passivetreedecorators")),
        mastery_groups: fetch_optional(&table("passiveskillmasterygroups")),
        mastery_art: fetch_optional(&table("passiveskilltreemasteryart")),
    };

    let mut db = atlas_node_db::build(
        passiveskills_bytes,
        stats_bytes,
        &stat_csd_sources,
        extra,
        schema,
        is_poe2,
        hardmode,
    )?;

    let node_frames = match fetch_optional(&table("passiveskilltreenodeframeart")) {
        Some(bytes) => art::parse_node_frame_art(bytes, schema, is_poe2)?,
        None => Vec::new(),
    };
    let ui_art_bytes = fetch(&table("passiveskilltreeuiart"))?;
    // PoE 1 points its UI art at a background-art row and names no connector art.
    let (art_sets, ui_art_ids) = match is_poe2 {
        true => {
            let connection_bytes = fetch(&table("passiveskilltreeconnectionart"))?;
            let connections = art::parse_connection_art(connection_bytes, schema, is_poe2)?;
            art::parse_ui_art(ui_art_bytes, &node_frames, &connections)?
        }
        false => {
            let backgrounds = match fetch_optional(&table("passiveskilltreegroupbackgroundart")) {
                Some(bytes) => art::parse_group_background_art(bytes, schema, is_poe2)?,
                None => Vec::new(),
            };
            art::parse_ui_art_poe1(ui_art_bytes, schema, &node_frames, &backgrounds)?
        }
    };
    db.art_sets = art_sets;
    db.ui_art_ids = ui_art_ids;
    db.node_frames = node_frames;
    // PoE 1 names its interface art inside sheets rather than shipping files.
    db.ui_atlas = fetch_optional(ui_atlas::ATLAS_PATH)
        .map(|bytes| ui_atlas::UiAtlas::parse(&crate::parsers::utils::decode_text_lossy(&bytes)))
        .filter(|atlas| !atlas.is_empty())
        .map(std::sync::Arc::new);

    Ok(db)
}
