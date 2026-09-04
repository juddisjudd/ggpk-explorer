//! `passive_skill_trees/<tree>.json` — one file per passive graph, with its
//! groups, node positions and the text each node shows.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use crate::dat::relational::{Ref, Row};
use crate::dat::stat_translation::TranslationLookup;
use std::collections::HashMap;

/// Ring radii the client lays nodes out on, in tree units.
const ORBIT_RADII: [i64; 10] = [0, 82, 162, 335, 493, 662, 846, 251, 1080, 1332];

/// Which description file names a tree's stats. Keyed off the graph path
/// rather than the panel title, which GGG renames between patches — that is
/// how the main tree came to ship with no rendered text at all.
fn translation_file(graph: &str) -> &'static str {
    if graph.contains("AtlasSkillGraphs") {
        "atlas_stat_descriptions"
    } else {
        "passive_skill_stat_descriptions"
    }
}

pub fn passives(ctx: &Ctx) -> Result<(), String> {
    let trees = ctx.table("PassiveSkillTrees")?;
    let skills = ctx.table("PassiveSkills")?;

    // Nodes are addressed by graph id, not row index.
    let mut by_hash: HashMap<i64, usize> = HashMap::new();
    for row in skills.rows() {
        by_hash.entry(row.int("PassiveSkillGraphId")).or_insert(row.index);
    }

    let mut written = 0;
    for tree in trees.rows() {
        let graph = tree.str("PassiveSkillGraph");
        if graph.is_empty() {
            continue;
        }
        let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, &format!("{}.psg", graph)) else {
            continue;
        };
        let psg = match crate::dat::psg::parse_psg(&bytes) {
            Ok(psg) => psg,
            Err(e) => {
                eprintln!("passives: {}.psg did not parse: {}", graph, e);
                continue;
            }
        };

        let name = ctx.rr.deref(tree, "Name");
        let descriptions = translation_file(graph);
        let translations = ctx.translations(descriptions);

        let mut nodes: Vec<(i64, J)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let add = |hash: i64, nodes: &mut Vec<(i64, J)>, seen: &mut std::collections::HashSet<i64>| {
            if !seen.insert(hash) {
                return;
            }
            if let Some(row) = by_hash.get(&hash).and_then(|&i| skills.row(i)) {
                nodes.push((hash, passive(ctx, row, Some(&translations))));
            }
        };

        for root in &psg.roots {
            add(*root as i64, &mut nodes, &mut seen);
        }
        let groups = psg.groups.iter().map(|group| {
            let members = group.nodes.iter().map(|node| {
                Obj::new()
                    .set("hash", int(node.skill_id as i64))
                    .set("radius", int(node.radius as i64))
                    .set("position_clockwise", int(node.position as i64))
                    .set("connections", json::arr(node.connections.iter().map(|c| int(c.node_id as i64))))
                    // Each connection curves along an orbit; one number per link.
                    .set("splines", json::arr(node.connections.iter().map(|c| int(c.orbit as i64))))
                    .build()
            });
            Obj::new()
                .set("x", json::float(group.x))
                .set("y", json::float(group.y))
                .set("flag", int(group.background_flag as i64))
                .set("passives", json::arr(members))
                .build()
        });
        let groups = json::arr(groups.collect::<Vec<_>>());
        for group in &psg.groups {
            for node in &group.nodes {
                add(node.skill_id as i64, &mut nodes, &mut seen);
            }
        }

        // A description file that renders nothing for a tree that does have
        // stats means the wrong one was picked, which is otherwise invisible.
        let carry_stats = nodes.iter().filter(|(_, node)| has_field(node, "stats")).count();
        let described = nodes.iter().filter(|(_, node)| has_field(node, "stat_text")).count();
        if described == 0 && carry_stats > 0 {
            eprintln!(
                "passives: {} has {} nodes with stats and none rendered from {}",
                tree.id(),
                carry_stats,
                descriptions
            );
        }

        let document = Obj::new()
            .set("title", text(name.as_ref().map(|n| n.row().string("Text")).unwrap_or_default()))
            .set("roots", json::arr(psg.roots.iter().map(|r| int(*r as i64))))
            .set("skills_per_orbit", json::arr(psg.passives_per_orbit.iter().map(|p| int(*p as i64))))
            .set("orbit_radii", json::arr(ORBIT_RADII.iter().map(|r| int(*r))))
            .set("groups", groups)
            .set("passives", J::Obj(nodes.into_iter().map(|(h, v)| (h.to_string(), v)).collect()))
            .set("art", ui_art(ctx, ctx.rr.deref(tree, "UIArt").as_ref()))
            .build();

        json::write(ctx.out, &format!("passive_skill_trees/{}", tree.id()), &json::sorted(document))?;
        written += 1;
    }

    if written == 0 {
        return Err("no passive skill graphs could be read".to_string());
    }
    Ok(())
}

/// Whether a rendered node carries a non-empty `stats` or `stat_text`.
fn has_field(node: &J, name: &str) -> bool {
    let J::Obj(fields) = node else { return false };
    fields.iter().any(|(key, value)| {
        key == name
            && match value {
                J::Arr(items) => !items.is_empty(),
                J::Obj(items) => !items.is_empty(),
                _ => false,
            }
    })
}

/// One passive node: its flags, stats and the lines those stats render as.
pub fn passive(ctx: &Ctx, row: Row<'_>, translations: Option<&TranslationLookup>) -> J {
    let stat_ids = ctx.rr.deref_list_ids(row, "Stats");
    let values: Vec<i32> = (1..=stat_ids.len().max(1))
        .map(|i| row.int(&format!("Stat{}Value", i)) as i32)
        .collect();

    let mut entry = Obj::new()
        .set("id", text(row.id()))
        .set("hash", int(row.int("PassiveSkillGraphId")))
        .set("name", text(row.str("Name")))
        .set("flavour_text", text(row.str("FlavourText")))
        .set("reminder_text", json::strings(
            ctx.rr.deref_list(row, "ReminderStrings").iter().map(|r| r.row().string("Text")).collect::<Vec<_>>(),
        ))
        .set("skill_points", int(row.int("SkillPointsGranted")))
        .set("is_keystone", J::Bool(row.bool("IsKeystone")))
        .set("is_notable", J::Bool(row.bool("IsNotable")))
        // The attribute a node like this grants is picked when it is
        // allocated, so the data names none; the flag is all there is.
        .set("is_attribute", J::Bool(row.bool("IsAttribute")))
        .set("is_multiple_choice", J::Bool(row.bool("IsMultipleChoice")))
        .set("is_multiple_choice_option", J::Bool(row.bool("IsMultipleChoiceOption")))
        .set("is_icon_only", J::Bool(row.bool("IsJustIcon")))
        .set("is_jewel_socket", J::Bool(row.bool("IsJewelSocket")))
        .set("is_ascendancy_starting_node", J::Bool(row.bool("IsAscendancyStartingNode")))
        .set("is_atlas_root", J::Bool(row.bool("IsRootOfAtlasTree")))
        .set("atlas_group", text(row.str("AtlasNodeGroup")))
        .set("weapon_set_points", int(row.int("WeaponPointsGranted")))
        .set("is_free", J::Bool(row.bool("IsFree")));

    let buffs = ctx
        .rr
        .deref_list(row, "PassiveSkillBuffs")
        .iter()
        .filter_map(|b| ctx.rr.deref_id(b.row(), "BuffDefinition"))
        .collect::<Vec<_>>();
    if !buffs.is_empty() {
        entry = entry.set("buff_definitions", json::strings(&buffs));
    }
    if let Some(ascendancy) = ctx.rr.deref_id(row, "Ascendancy") {
        entry = entry.set("ascendancy", text(ascendancy));
    }
    if let Some(icon) = json::opt_text(row.str("Icon_DDSFile")) {
        entry = entry.set("icon", icon);
    }
    if let Some(subtree) = ctx.rr.deref(row, "AtlasSubTree") {
        entry = entry.set("atlas_subtree", atlas_subtree(subtree.row()));
    }
    if let Some(gem) = ctx.rr.deref(row, "GrantedSkill") {
        if let Some(base) = ctx.rr.deref_id(gem.row(), "BaseItemType") {
            entry = entry.set("granted_skill", text(base));
        }
    }

    let stats = stat_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), int(values.get(i).copied().unwrap_or(0) as i64)))
        .collect::<Vec<_>>();
    entry = entry.set("stats", J::Obj(stats));

    if let Some(translations) = translations {
        // Lines follow the node's own stat order, not the description file's.
        let ranges: Vec<(i32, i32)> = values.iter().map(|&v| (v, v)).collect();
        entry = entry.set("stat_text", json::strings(translations.translate_ranges(&stat_ids, &ranges)));
    }
    entry.build()
}

fn atlas_subtree(row: Row<'_>) -> J {
    Obj::new()
        .set("id", text(row.id()))
        .set("image", text(row.str("UI_Image")))
        .set("background", text(row.str("UI_Background")))
        .set(
            "illustration",
            Obj::new().set("x", int(row.int("IllustrationX"))).set("y", int(row.int("IllustrationY"))).build(),
        )
        .set(
            "counter",
            Obj::new().set("x", int(row.int("CounterX"))).set("y", int(row.int("CounterY"))).build(),
        )
        .build()
}

/// The art a tree draws itself with: group backgrounds and node frames.
pub fn ui_art(ctx: &Ctx, art: Option<&Ref>) -> J {
    let Some(art) = art else { return J::Null };
    let row = art.row();
    let mut out = Obj::new().set("id", text(row.id())).set("glow", text(row.str("Glow")));
    for size in ["Small", "Medium", "Large"] {
        out = out
            .set(&format!("group_bg_{}_normal", size.to_lowercase()), text(row.str(&format!("GroupBackground{}", size))))
            .set(
                &format!("group_bg_{}_blank", size.to_lowercase()),
                text(row.str(&format!("GroupBackground{}Blank", size))),
            );
    }
    for (kind, column) in [
        ("passive", "PassiveFrame"),
        ("notable", "NotableFrame"),
        ("keystone", "KeystoneFrame"),
        ("jewel", "JewelFrame"),
        ("ascendancystart", "AscendancyStart"),
    ] {
        out = out.or_null(&format!("{}_frame", kind), frame_art(ctx, row, column));
    }
    out.build()
}

fn frame_art(ctx: &Ctx, row: Row<'_>, column: &str) -> Option<J> {
    let frame = ctx.rr.deref(row, column)?;
    let frame = frame.row();
    Some(
        Obj::new()
            .set("unallocated", text(frame.str("Normal")))
            .set("allocated", text(frame.str("Active")))
            .set("allocatable", text(frame.str("CanAllocate")))
            .build(),
    )
}
