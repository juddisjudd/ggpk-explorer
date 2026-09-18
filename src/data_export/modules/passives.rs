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
            .set("orbit_radii", json::arr(ORBIT_RADII.iter().take(psg.passives_per_orbit.len()).map(|r| int(*r))))
            .set("groups", groups)
            .set("passives", J::Obj(nodes.into_iter().map(|(h, v)| (h.to_string(), v)).collect()))
            .set("art", ui_art(ctx, ctx.rr.deref(tree, "UIArt").as_ref()))
            .build();

        ctx.write(&format!("passive_skill_trees/{}", tree.id()), &json::sorted(document))?;
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
    // A flag PoE 1 does not have reads as absent rather than as false.
    let flag = |name: &str| row.table.has_col(name).then(|| J::Bool(row.bool(name)));

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
        .or_null("is_attribute", flag("IsAttribute"))
        .set("is_multiple_choice", J::Bool(row.bool("IsMultipleChoice")))
        .set("is_multiple_choice_option", J::Bool(row.bool("IsMultipleChoiceOption")))
        .set("is_icon_only", J::Bool(row.bool("IsJustIcon")))
        .set("is_jewel_socket", J::Bool(row.bool("IsJewelSocket")))
        .set("is_ascendancy_starting_node", J::Bool(row.bool("IsAscendancyStartingNode")))
        .or_null("is_atlas_root", flag("IsRootOfAtlasTree"))
        .or_null("atlas_group", row.table.has_col("AtlasNodeGroup").then(|| text(row.str("AtlasNodeGroup"))))
        .or_null("weapon_set_points", row.table.has_col("WeaponPointsGranted").then(|| int(row.int("WeaponPointsGranted"))))
        .or_null("is_free", flag("IsFree"));

    let templates = ctx.rr.deref_list(row, "PassiveSkillBuffs");
    let definition = |template: Row<'_>| template.table.pick(&["BuffDefinition", "BuffDefinitionsKey"]).unwrap_or("BuffDefinition");
    let buffs = templates.iter().filter_map(|b| ctx.rr.deref_id(b.row(), definition(b.row()))).collect::<Vec<_>>();
    if !buffs.is_empty() {
        entry = entry.set("buff_definitions", json::strings(&buffs));
    }
    // PoE 1's tree shows the stats a node's aura or buff applies beside its own.
    if let (false, Some(translations)) = (ctx.rr.is_poe2, translations) {
        let mut lines = Vec::new();
        for template in &templates {
            let template = template.row();
            let Some(buff) = ctx.rr.deref(template, definition(template)) else { continue };
            let buff = buff.row();
            let stats = ctx.rr.deref_list_ids(buff, buff.table.pick(&["Stats", "StatsKeys"]).unwrap_or("Stats"));
            let mut values = template.list_int("Buff_StatValues");
            let mut covered: Vec<String> = stats.into_iter().take(values.len()).collect();
            values.truncate(covered.len());
            for flag in ctx.rr.deref_list_ids(buff, "GrantedFlags") {
                covered.push(flag);
                values.push(1);
            }
            let ranges: Vec<(i32, i32)> = values.iter().take(covered.len()).map(|&v| (v as i32, v as i32)).collect();
            let rendered = match template.int("AuraRadius") {
                0 => translations.translate_ranges(&covered, &ranges),
                _ => ctx.translations("passive_skill_aura_stat_descriptions").translate_ranges(&covered, &ranges),
            };
            lines.extend(rendered);
        }
        if !lines.is_empty() {
            entry = entry.set("buff_stat_text", json::strings(&lines));
        }
        if let Some(per_level) = ctx.rr.deref(row, "GrantedEffectsPerLevel") {
            let per_level = per_level.row();
            entry = entry.or_null(
                "granted_effect",
                ctx.rr.deref_id(per_level, "GrantedEffect").map(|id| {
                    Obj::new().set("id", text(id)).set("level", int(per_level.int("Level"))).build()
                }),
            );
        }
    }
    if let Some(ascendancy) = ctx.rr.deref_id(row, row.table.pick(&["Ascendancy", "AscendancyKey"]).unwrap_or("Ascendancy")) {
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

/// `timeless_jewels.json`: the passives a timeless jewel swaps in, the small
/// additions it grants, and the per-faction rules for which nodes it touches.
pub fn timeless_jewels(ctx: &Ctx) -> Result<(), String> {
    let skills = ctx.table("AlternatePassiveSkills")?;
    let additions = ctx.table("AlternatePassiveAdditions")?;
    let translations = ctx.translations("passive_skill_stat_descriptions");

    let range = |value: Option<(i64, i64)>| {
        let (min, max) = value.unwrap_or((0, 0));
        json::arr([int(min), int(max)])
    };
    let column = |row: Row<'_>, names: &[&'static str]| row.table.pick(names).unwrap_or(names[0]);
    let version = |row: Row<'_>| {
        ctx.rr.deref(row, column(row, &["AlternateTreeVersion", "AlternateTreeVersionsKey"])).map(|v| {
            let v = v.row();
            text(v.str(column(v, &["ConquerorType", "Id"])))
        })
    };
    // Past the last stat the schema names, PoE 1 keeps each stat's range in two unnamed columns.
    let stat_range = |row: Row<'_>, i: usize| {
        let named = (1..=6).rev().find(|j| row.table.has_col(&format!("Stat{}Max", j))).unwrap_or(0);
        let anchor = format!("Stat{}Max", named);
        pair(row, &format!("Stat{}", i), (&anchor, 1 + 2 * i.saturating_sub(named + 1)))
    };
    let stats = |row: Row<'_>| {
        let ids = ctx.rr.deref_list_ids(row, column(row, &["Stats", "StatsKeys"]));
        let ranges: Vec<(i32, i32)> = (1..=ids.len())
            .map(|i| stat_range(row, i).map(|(a, b)| (a as i32, b as i32)).unwrap_or((0, 0)))
            .collect();
        let lines = translations.translate_ranges(&ids, &ranges);
        let listed = ids.iter().zip(&ranges).map(|(id, (min, max))| {
            Obj::new().set("id", text(id)).set("min", int(*min)).set("max", int(*max)).build()
        });
        (json::arr(listed), json::strings(lines))
    };

    let versions = ctx
        .optional_table("AlternateTreeVersions")
        .map(|table| {
            J::Obj(
                table
                    .rows()
                    .map(|row| {
                        // PoE 1 names only the id; the rest keeps PoE 2's order.
                        let key = column(row, &["ConquerorType", "Id"]);
                        let flag = |name: &str, offset: usize| {
                            J::Bool(row.table.column_or_after(&[name], key, offset).is_some_and(|c| row.bool_at(c)))
                        };
                        let entry = Obj::new()
                            .set("small_attribute_replaced", flag("SmallAttributeReplaced", 1))
                            .set("small_normal_passive_replaced", flag("SmallNormalPassiveReplaced", 2))
                            .set("small_attribute_additions", range(pair(row, "SmallAttributePassiveSkillAdditions", (key, 3))))
                            .set("notable_additions", range(pair(row, "NotableAdditions", (key, 5))))
                            .set("small_normal_additions", range(pair(row, "SmallNormalPassiveSkillAdditions", (key, 7))))
                            .set(
                                "notable_replacement_spawn_weight",
                                int(row
                                    .table
                                    .column_or_after(&["NotableReplacementSpawnWeight"], key, 9)
                                    .map(|c| row.int_at(c))
                                    .unwrap_or(0)),
                            )
                            .build();
                        (row.string(key), entry)
                    })
                    .collect(),
            )
        })
        .unwrap_or(J::Null);

    let passives = skills
        .rows()
        .map(|row| {
            let (stat_list, stat_text) = stats(row);
            let entry = Obj::new()
                .or_null("version", version(row))
                .set("name", text(row.str("Name")))
                .set("passive_types", J::Arr(row.list_int("PassiveType").into_iter().map(int).collect()))
                .set("stats", stat_list)
                .set("stat_text", stat_text)
                .set("spawn_weight", int(row.int("SpawnWeight")))
                .set("conqueror_index", int(at(row, "ConquerorIndex", "SpawnWeight", 1)))
                .set("conqueror_version", int(at(row, "ConquerorVersion", "AchievementItemsKeys", 1)))
                .set("conqueror_spawn_weight", int(at(row, "ConquerorSpawnWeight", "AchievementItemsKeys", 2)))
                .set("random", range(pair(row, "Random", ("RandomMax", 1))))
                .or_null("flavour_text", json::opt_text(row.str("FlavourText")))
                .or_null("icon", json::opt_text(row.str("DDSIcon")))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();

    let added = additions
        .rows()
        .map(|row| {
            let (stat_list, stat_text) = stats(row);
            let entry = Obj::new()
                .or_null("version", version(row))
                .set("passive_types", J::Arr(row.list_int("PassiveType").into_iter().map(int).collect()))
                .set("stats", stat_list)
                .set("stat_text", stat_text)
                .set("spawn_weight", int(row.int("SpawnWeight")))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();

    let root = Obj::new()
        .set("versions", versions)
        .set("passives", J::Obj(passives))
        .set("additions", J::Obj(added))
        .build();
    ctx.write("timeless_jewels", &root)
}

/// A min/max pair. PoE 2 stores it as one interval column; PoE 1 as `<name>Min`
/// and `<name>Max`, or as two unnamed columns `offset` after `anchor`.
fn pair(row: Row<'_>, name: &str, (anchor, offset): (&str, usize)) -> Option<(i64, i64)> {
    if row.table.has_col(name) {
        return row.interval(name);
    }
    let (min, max) = (format!("{}Min", name), format!("{}Max", name));
    if row.table.has_col(&min) {
        return Some((row.int(&min), row.int(&max)));
    }
    let at = row.table.column_or_after(&[], anchor, offset)?;
    row.table.column_or_after(&[], anchor, offset + 1)?;
    Some((row.int_at(at), row.int_at(at + 1)))
}

/// An integer column by name, or the unnamed one `offset` after `anchor`.
fn at(row: Row<'_>, name: &str, anchor: &str, offset: usize) -> i64 {
    row.table.column_or_after(&[name], anchor, offset).map(|c| row.int_at(c)).unwrap_or(0)
}

/// `jewel_slots.json`: every tree node that holds a jewel, by graph id.
pub fn jewel_slots(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("PassiveJewelSlots")?;
    let slot = table.require(&["Slot", "Passive"])?;
    let proxy = table.pick(&["ProxySlot", "Proxy"]);
    let parent = table.pick(&["ReplacesSlot", "Parent"]);
    let hash = |r: &Ref| int(r.row().int("PassiveSkillGraphId"));
    let root = table
        .rows()
        .filter_map(|row| {
            let passive = ctx.rr.deref(row, slot)?;
            let entry = Obj::new()
                .set("passive", text(passive.id()))
                .or_null("cluster_size", ctx.rr.deref_id(row, "ClusterJewelSize").map(text))
                .set("cluster_index", int(row.int("ClusterIndex")))
                .or_null(
                    "replaces_slot",
                    parent
                        .and_then(|c| ctx.rr.deref(row, c))
                        .and_then(|p| ctx.rr.deref(p.row(), slot))
                        .map(|p| hash(&p)),
                )
                .or_null("proxy", proxy.and_then(|c| ctx.rr.deref(row, c)).map(|p| hash(&p)))
                .set("start_indices", J::Arr(row.list_int("StartIndices").into_iter().map(int).collect()))
                .set("is_sinister", J::Bool(row.bool("SinisterJewelSocket")))
                .build();
            Some((passive.row().int("PassiveSkillGraphId").to_string(), entry))
        })
        .collect();
    ctx.write("jewel_slots", &J::Obj(root))
}
