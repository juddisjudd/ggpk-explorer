//! `skill_gems.json`, `ascendancies.json` and `skills.json`.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::{statics, Ctx};
use crate::dat::relational::Row;
use crate::dat::stat_translation::TranslationLookup;
use std::collections::HashMap;

/// `GemType` values, in the order the client stores them.
const GEM_TYPES: [&str; 3] = ["active", "support", "spirit"];
/// `GemColour` values; zero means the gem has no colour.
const GEM_COLOURS: [Option<&str>; 5] = [None, Some("r"), Some("g"), Some("b"), Some("w")];

/// What lets a skill skip its cooldown. Value 4 means nothing does, and is
/// left off rather than reported.
const COOLDOWN_BYPASS: [(i64, &str); 3] = [
    (1, "expend_endurance_charge"),
    (2, "expend_frenzy_charge"),
    (3, "expend_power_charge"),
];

/// Skill icons ship at two resolutions; the data names the standard one and
/// the client looks for a `4k` sibling.
fn four_k(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let (dir, file) = path.rsplit_once('/')?;
    Some(format!("{}/4k/{}", dir, file))
}

/// The client works an attribute requirement out from the character level the
/// gem level asks for and the gem's weighting rather than storing one. This is
/// Path of Building's `calcLib.getGemStatRequirement`, which it labels as the
/// in-game formula; support gems ask for no attributes at all. PoB also
/// reports anything under 8 as no requirement, which is left off here because
/// the client does show those small numbers.
fn attribute_requirement(level: i64, weight: i64) -> i64 {
    let scaled = (5.0 + (level as f64 - 3.0) * 1.7) * (weight as f64 / 100.0).powf(0.9);
    4 + (scaled + 0.5).floor() as i64
}

/// Gem level to the character level it asks for, per `ItemExperienceTypes`
/// row. The curve starts at zero, which the client shows as level one.
fn level_curves(ctx: &Ctx) -> HashMap<usize, Vec<(i64, i64)>> {
    let Some(table) = ctx.rr.table("ItemExperiencePerLevel") else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<(i64, i64)>> = HashMap::new();
    for row in table.rows() {
        if let Some(kind) = row.key("ItemExperienceType") {
            out.entry(kind).or_default().push((row.int("ItemCurrentLevel"), row.int("Level")));
        }
    }
    for curve in out.values_mut() {
        curve.sort_unstable();
    }
    out
}

pub fn skill_gems(ctx: &Ctx) -> Result<(), String> {
    let gems = ctx.table("SkillGems")?;
    let supports = support_gems(ctx);
    let recommended = recommended_supports(ctx);
    let curves = level_curves(ctx);

    let mut root: Vec<(String, J)> = Vec::new();
    for gem in gems.rows() {
        for effect in ctx.rr.deref_list(gem, "GemEffects") {
            let effect = effect.row();
            // `[DNT]` marks an effect the developers left in but do not ship.
            if effect.str("Name").contains("[DNT]") {
                continue;
            }
            let Some(base) = ctx.rr.deref(gem, "BaseItemType") else { continue };
            let base = base.row();
            let gem_type = GEM_TYPES.get(gem.int("GemType") as usize).copied().unwrap_or("active");

            let mut granted = Vec::new();
            if let Some(id) = ctx.rr.deref_id(effect, "GrantedEffect") {
                granted.push(id);
            }
            granted.extend(ctx.rr.deref_list_ids(effect, "AdditionalGrantedEffects"));

            let support = supports.get(&gem.index).copied();
            let base_icon = if gem_type == "support" {
                support.and_then(|i| ctx.rr.table("SupportGems").and_then(|t| t.row(i).map(|r| r.string("Icon"))))
            } else {
                ctx.rr
                    .deref(effect, "GrantedEffect")
                    .and_then(|e| ctx.rr.deref(e.row(), "ActiveSkill"))
                    .map(|s| s.row().string("Icon_DDSFile"))
            };
            // Most skills ship a 4k icon; the ones that do not keep the plain one.
            let icon = base_icon.filter(|p| !p.is_empty()).map(|path| match four_k(&path) {
                Some(hi) if ctx.files.exists(&hi) => hi,
                _ => path,
            });
            if let Some(icon) = &icon {
                super::images::export(ctx, icon, None);
            }
            super::images::export(ctx, gem.str("UI_Image"), None);

            let crafting: Vec<String> = ctx
                .rr
                .deref_list(gem, "CraftingTypes")
                .iter()
                .map(|t| t.row().string("Name"))
                .collect();

            let mut entry = Obj::new()
                .set("tags", json::strings(ctx.rr.deref_list_ids(effect, "GemTags")))
                .set("gem_type", text(gem_type))
                .or_null("color", GEM_COLOURS.get(gem.int("GemColour") as usize).copied().flatten().map(text))
                .set(
                    "requirement_weights",
                    Obj::new()
                        .set("strength", int(gem.int("StrengthRequirementPercent")))
                        .set("dexterity", int(gem.int("DexterityRequirementPercent")))
                        .set("intelligence", int(gem.int("IntelligenceRequirementPercent")))
                        .build(),
                )
                .set("grants_skills", json::strings(&granted))
                .set(
                    "base_item",
                    Obj::new()
                        .set("id", text(base.id()))
                        .set("display_name", text(base.str("Name")))
                        .set("release_state", text("released"))
                        .build(),
                )
                .or_null("support_name", json::opt_text(effect.str("SupportName")))
                .or_null("support_text", json::opt_text(effect.str("SupportText")))
                .or_null("skill_name", json::opt_text(effect.str("Name")))
                .or_null("crafting_types", (!crafting.is_empty()).then(|| json::strings(&crafting)))
                .set("crafting_level", int(gem.int("CraftingLevel")))
                .or_null("tutorial_video", json::opt_text(gem.str("TutorialVideo")))
                .or_null("ui_image", json::opt_text(gem.str("UI_Image")))
                .or_null("icon_dds_file", icon.map(text));

            let weights = [
                ("strength", gem.int("StrengthRequirementPercent")),
                ("dexterity", gem.int("DexterityRequirementPercent")),
                ("intelligence", gem.int("IntelligenceRequirementPercent")),
            ];
            if let Some(curve) = gem.key("ItemExperienceType").and_then(|k| curves.get(&k)) {
                let requirements: Vec<(String, J)> = curve
                    .iter()
                    .map(|(gem_level, level)| {
                        let mut row = Obj::new().set("level", int((*level).max(1)));
                        for (attribute, weight) in weights {
                            if weight > 0 && gem_type != "support" {
                                row = row.set(attribute, int(attribute_requirement(*level, weight)));
                            }
                        }
                        (gem_level.to_string(), row.build())
                    })
                    .collect();
                entry = entry
                    .or_null("experience_type", ctx.rr.deref_id(gem, "ItemExperienceType").map(text))
                    .set("max_level", int(curve.last().map(|(gem_level, _)| *gem_level).unwrap_or(1)))
                    .set("requirements", J::Obj(requirements));
            }

            if gem_type == "support" {
                let lineage = support
                    .and_then(|i| ctx.rr.table("SupportGems").and_then(|t| t.row(i).map(|r| r.bool("IsLineage"))))
                    .unwrap_or(false);
                entry = entry.set("is_lineage", J::Bool(lineage));
            } else {
                let picks = recommended.get(&gem.index).cloned().unwrap_or_default();
                entry = entry.set("recommended_supports", json::strings(&picks));
            }

            root.push((base.id().to_string(), json::sorted(entry.build())));
        }
    }
    root.sort_by(|a, b| a.0.cmp(&b.0));
    root.dedup_by(|a, b| a.0 == b.0);

    json::write(ctx.out, "skill_gems", &J::Obj(root))
}

/// `SupportGems` row for each skill gem it describes.
fn support_gems(ctx: &Ctx) -> HashMap<usize, usize> {
    let Some(table) = ctx.rr.table("SupportGems") else { return HashMap::new() };
    let mut out = HashMap::new();
    for row in table.rows() {
        if let Some(gem) = row.key("SkillGem") {
            out.entry(gem).or_insert(row.index);
        }
    }
    out
}

/// Base item ids of the supports the client suggests for each skill gem.
fn recommended_supports(ctx: &Ctx) -> HashMap<usize, Vec<String>> {
    let Some(table) = ctx.rr.table("SkillGemSupports") else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<String>> = HashMap::new();
    for row in table.rows() {
        let Some(gem) = row.key("SkillGem") else { continue };
        if out.contains_key(&gem) {
            continue;
        }
        let picks = ctx
            .rr
            .deref_list(row, "Supports")
            .iter()
            .filter_map(|s| ctx.rr.deref_id(s.row(), "BaseItemType"))
            .collect();
        out.insert(gem, picks);
    }
    out
}

pub fn ascendancies(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("Ascendancy")?;
    let overrides = ascendancy_overrides(ctx);
    let translations = ctx.translations("passive_skill_stat_descriptions");

    let mut root = table
        .rows()
        .map(|row| {
            let mut entry = Obj::new()
                .set("class_number", int(row.int("ClassNo")))
                .or_null("character", ctx.rr.deref_id(row, "Character").map(text))
                .set("coordinate_rect", text(row.str("CoordinateRect")))
                .set("name", text(row.str("Name")))
                .set("flavour_text", text(row.str("FlavourText")))
                .set("flavour_text_colour", text(row.str("RGBFlavourTextColour")))
                .set("string", text(row.str("OGGFile")))
                .set("passive_tree_image", text(row.str("PassiveTreeImage")))
                .set("tree_region_vector", int(row.int("TreeRegionVector")))
                .set("tree_region_angle", int(row.int("TreeRegionAngle")))
                .set("disabled", J::Bool(row.bool("Disabled")))
                .or_null("overrides_ascendancy", ctx.rr.deref_id(row, "BaseAscendancy").map(text))
                .set("art", super::passives::ui_art(ctx, ctx.rr.deref(row, "UIArt").as_ref()));

            if row.key("BaseAscendancy").is_some() {
                let table = ctx.rr.table("AscendancyPassiveSkillOverrides");
                let replacements = overrides.get(&row.index).map(Vec::as_slice).unwrap_or_default();
                entry = entry.set(
                    "passive_overrides",
                    json::arr(replacements.iter().filter_map(|&index| {
                        let over = table.as_ref()?.row(index)?;
                        let from = ctx.rr.deref(over, "SkillToOverride")?;
                        let to = ctx.rr.deref(over, "Override")?;
                        Some(
                            Obj::new()
                                .set("from_hash", int(from.row().int("PassiveSkillGraphId")))
                                .set("to_passive", super::passives::passive(ctx, to.row(), Some(&translations)))
                                .build(),
                        )
                    })),
                );
            }
            (row.id().to_string(), json::sorted(entry.build()))
        })
        .collect::<Vec<_>>();

    root.sort_by(|a, b| a.0.cmp(&b.0));
    json::write(ctx.out, "ascendancies", &J::Obj(root))
}

fn ascendancy_overrides(ctx: &Ctx) -> HashMap<usize, Vec<usize>> {
    let Some(table) = ctx.rr.table("AscendancyPassiveSkillOverrides") else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in table.rows() {
        if let Some(asc) = row.key("AscendancyToOverrideFor") {
            out.entry(asc).or_default().push(row.index);
        }
    }
    out
}

/// `skills.json`: every granted effect, its per-level numbers and the stat
/// sets that describe what it actually does.
pub fn skills(ctx: &Ctx) -> Result<(), String> {
    let effects = ctx.table("GrantedEffects")?;
    let levels_by_effect = group_by(ctx, "GrantedEffectsPerLevel", "GrantedEffect");
    let quality_by_effect = group_by(ctx, "GrantedEffectQualityStats", "GrantedEffect");
    let levels_by_set = group_by(ctx, "GrantedEffectStatSetsPerLevel", "StatSet");
    let totems = skill_totem_multipliers(ctx);

    let mut root: Vec<(String, J)> = Vec::new();
    for effect in effects.rows() {
        let id = effect.id().to_string();
        if id.is_empty() {
            continue;
        }
        let is_support = effect.bool("IsSupport");
        let active = ctx.rr.deref(effect, "ActiveSkill");
        let skill_id = active.as_ref().map(|a| a.row().string("Id"));

        let mut entry = Obj::new().set("is_support", J::Bool(is_support));
        if is_support {
            entry = entry.set(
                "support_gem",
                Obj::new()
                    .set("letter", text(effect.str("SupportGemLetter")))
                    .set("supports_gems_only", J::Bool(effect.bool("SupportsGemsOnly")))
                    .or_null("allowed_types", types(ctx, effect, "AllowedActiveSkillTypes"))
                    .or_null("excluded_types", types(ctx, effect, "ExcludedActiveSkillTypes"))
                    .or_null("added_types", types(ctx, effect, "AddedActiveSkillTypes"))
                    .or_null("added_minion_types", types(ctx, effect, "AddedMinionActiveSkillTypes"))
                    .build(),
            );
        }
        if let Some(active) = &active {
            entry = entry
                .set("cast_time", int(effect.int("CastTime")))
                .set("active_skill", active_skill(ctx, active.row(), &totems));
        }
        entry = entry.set("stats", J::Obj(Vec::new()));

        // Per level: costs, cooldowns and reservations.
        let mut level_keys: Vec<String> = Vec::new();
        let mut level_values: Vec<J> = Vec::new();
        let mut rows: Vec<usize> = levels_by_effect.get(&effect.index).cloned().unwrap_or_default();
        let per_level_table = ctx.rr.table("GrantedEffectsPerLevel");
        rows.sort_by_key(|&i| per_level_table.as_ref().and_then(|t| t.row(i)).map(|r| r.int("Level")).unwrap_or(0));
        for index in rows {
            let Some(level) = per_level_table.as_ref().and_then(|t| t.row(index)) else { continue };
            level_keys.push(level.int("Level").to_string());
            level_values.push(per_level(ctx, effect, level, is_support));
        }
        let shared = statics::extract(&mut level_values);
        entry = entry
            .set("per_level", J::Obj(level_keys.into_iter().zip(level_values).collect()))
            .set("static", tooltip_order(shared.unwrap_or(J::Obj(Vec::new())), &mut []));

        // Stat sets: what the skill grants at each gem level.
        let sets: Vec<crate::dat::relational::Ref> = ctx
            .rr
            .deref(effect, "StatSet")
            .into_iter()
            .chain(ctx.rr.deref_list(effect, "AdditionalStatSets"))
            .collect();
        if !sets.is_empty() {
            let primary = sets[0].clone();
            let quality_rows = quality_by_effect.get(&effect.index).cloned().unwrap_or_default();
            let set_json: Vec<J> = sets
                .iter()
                .enumerate()
                .map(|(i, set)| {
                    stat_set(ctx, set, &primary, i, skill_id.as_deref(), &levels_by_set, &quality_rows)
                })
                .collect();
            entry = entry.set("stat_sets", J::Arr(set_json));
        }

        root.push((id, json::sorted(entry.build())));
    }
    root.sort_by(|a, b| a.0.cmp(&b.0));

    json::write(ctx.out, "skills", &J::Obj(root))
}

fn types(ctx: &Ctx, row: Row<'_>, column: &str) -> Option<J> {
    let ids = ctx.rr.deref_list_ids(row, column);
    (!ids.is_empty()).then(|| json::strings(&ids))
}

/// Item classes the skill can be used with. The skill names a requirement
/// group such as `Any Mace`, which in turn names the wieldable classes.
fn weapon_restrictions(ctx: &Ctx, row: Row<'_>) -> Vec<String> {
    let Some(requirement) = ctx.rr.deref(row, "WeaponRequirements") else { return Vec::new() };
    let wieldable = ctx.rr.deref_list(requirement.row(), "WieldableClasses");
    let mut out: Vec<String> = Vec::new();
    for class in wieldable {
        if let Some(id) = ctx.rr.deref_id(class.row(), "ItemClass") {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

fn active_skill(ctx: &Ctx, row: Row<'_>, totems: &HashMap<i64, f64>) -> J {
    let inputs = ctx.rr.deref_list_ids(row, "Input_Stats");
    let outputs = ctx.rr.deref_list_ids(row, "Output_Stats");
    let conversions: Vec<(String, J)> =
        inputs.into_iter().zip(outputs).map(|(from, to)| (from, text(to))).collect();

    let totem = row.int("SkillTotemId");
    let multiplier = totems.get(&totem).copied();
    let mut out = Obj::new()
        .set("id", text(row.id()))
        .set("display_name", text(row.str("DisplayedName")))
        .set("description", text(row.str("Description")))
        .or_null("types", types(ctx, row, "ActiveSkillTypes"))
        .set("weapon_restrictions", json::strings(weapon_restrictions(ctx, row)))
        .set("is_skill_totem", J::Bool(multiplier.is_some()))
        .set("is_manually_casted", J::Bool(row.bool("IsManuallyCasted")))
        .set("stat_conversions", J::Obj(conversions));
    if let Some(multiplier) = multiplier {
        out = out.set("skill_totem_life_multiplier", J::Num(multiplier));
    }
    if let Some(minion) = types(ctx, row, "MinionActiveSkillTypes") {
        out = out.set("minion_types", minion);
    }
    out.build()
}

/// A totem skill is one the client has a totem monster for; the monster's life
/// multiplier is what the totem is worth.
fn skill_totem_multipliers(ctx: &Ctx) -> HashMap<i64, f64> {
    let Some(table) = ctx.rr.table("SkillTotemVariations") else { return HashMap::new() };
    let mut out = HashMap::new();
    for row in table.rows() {
        let Some(monster) = ctx.rr.deref(row, "MonsterVarietiesKey") else { continue };
        out.entry(row.int("SkillTotemsKey"))
            .or_insert(monster.row().int("LifeMultiplier") as f64 / 100.0);
    }
    out
}

fn per_level(ctx: &Ctx, effect: Row<'_>, level: Row<'_>, is_support: bool) -> J {
    let mut out = Obj::new();
    let cooldown = level.int("Cooldown");
    if cooldown > 0 {
        out = out.set("cooldown", int(cooldown));
        let bypass = level.int("CooldownBypassType");
        if let Some((_, name)) = COOLDOWN_BYPASS.iter().find(|(v, _)| *v == bypass) {
            out = out.set("cooldown_bypass_type", text(*name));
        }
    }
    let stored = level.int("StoredUses");
    if stored > 0 {
        out = out.set("stored_uses", int(stored));
    }

    if is_support {
        out = out.set("cost_multiplier", int(level.int("CostMultiplier")));
    } else {
        let amounts = level.list_int("CostAmounts");
        let costs: Vec<(String, J)> = ctx
            .rr
            .deref_list(effect, "CostTypes")
            .iter()
            .enumerate()
            .filter_map(|(i, kind)| amounts.get(i).map(|amount| (kind.id(), int(*amount))))
            .collect();
        out = out.set("costs", J::Obj(costs));
        let attack_speed = level.int("AttackSpeedMultiplier");
        if attack_speed != 0 {
            out = out.set("attack_speed_multiplier", int(attack_speed));
        }
        let souls = level.int("VaalSouls");
        if souls > 0 {
            out = out.set(
                "vaal",
                Obj::new()
                    .set("souls", int(souls))
                    .set("stored_uses", int(level.int("VaalStoredUses")))
                    .build(),
            );
        }
    }

    let reservation = level.int("Reservation");
    out.or_null("reservations", (reservation > 0).then(|| Obj::new().set("spirit", int(reservation)).build()))
        .build()
}

/// Turns the `stat_order` maps collected across levels into one
/// `tooltip_order` list, which is the order a client tooltip prints stats in.
fn tooltip_order(shared: J, levels: &mut [J]) -> J {
    let mut order: Vec<(String, i64)> = Vec::new();
    let mut absorb = |value: Option<&J>| {
        if let Some(J::Obj(fields)) = value {
            for (stat, index) in fields {
                let index = match index {
                    J::Int(i) => *i,
                    _ => 0,
                };
                match order.iter_mut().find(|(s, _)| *s == *stat) {
                    Some(slot) => slot.1 = index,
                    None => order.push((stat.clone(), index)),
                }
            }
        }
    };
    for level in levels.iter() {
        absorb(level.get("stat_order"));
    }
    for level in levels.iter_mut() {
        statics::remove(level, "stat_order");
    }
    absorb(shared.get("stat_order"));

    let mut shared = shared;
    statics::remove(&mut shared, "stat_order");
    if !order.is_empty() {
        order.sort_by_key(|(_, index)| *index);
        shared.set("tooltip_order", json::strings(order.iter().map(|(stat, _)| stat.clone())));
    }
    shared
}

/// How the client heads the stat set's block, e.g. `Initial Strike` for the
/// `InitialStrike` label.
fn label_text(ctx: &Ctx, set: &crate::dat::relational::Ref) -> Option<String> {
    ctx.rr
        .deref(set.row(), "Label")
        .map(|label| label.row().string("Text"))
        .filter(|text| !text.is_empty())
}

#[allow(clippy::too_many_arguments)]
fn stat_set(
    ctx: &Ctx,
    set: &crate::dat::relational::Ref,
    primary: &crate::dat::relational::Ref,
    position: usize,
    skill_id: Option<&str>,
    levels_by_set: &HashMap<usize, Vec<usize>>,
    quality_rows: &[usize],
) -> J {
    let (file_name, translations) = pick_translation_file(ctx, skill_id, position);
    let per_level_table = ctx.rr.table("GrantedEffectStatSetsPerLevel");

    let order = |index: usize| -> i64 {
        per_level_table.as_ref().and_then(|t| t.row(index)).map(|r| r.int("GemLevel")).unwrap_or(0)
    };
    let mut own: Vec<usize> = levels_by_set.get(&set.index).cloned().unwrap_or_default();
    let mut base: Vec<usize> = levels_by_set.get(&primary.index).cloned().unwrap_or_default();
    own.sort_by_key(|&i| order(i));
    base.sort_by_key(|&i| order(i));

    // Quality describes the effect itself, not one level of it, so it is
    // worked out once and repeated — the static pass then hoists it back out.
    let quality = translations
        .as_deref()
        .map(|t| quality_stats(ctx, t, quality_rows, position))
        .unwrap_or_default();

    let mut level_keys: Vec<String> = Vec::new();
    let mut level_values: Vec<J> = Vec::new();
    for (n, &index) in own.iter().enumerate() {
        let Some(level) = per_level_table.as_ref().and_then(|t| t.row(index)) else { continue };
        let primary_level = base.get(n).and_then(|&i| per_level_table.as_ref().and_then(|t| t.row(i)));
        level_keys.push(level.int("GemLevel").to_string());
        level_values.push(stat_set_level(
            ctx,
            set.row(),
            level,
            primary.row(),
            primary_level,
            translations.as_deref(),
            &quality,
        ));
    }

    let shared = statics::extract(&mut level_values).unwrap_or(J::Obj(Vec::new()));
    let shared = tooltip_order(shared, &mut level_values);

    Obj::new()
        .set("id", text(set.row().id()))
        .or_null("label", ctx.rr.deref_id(set.row(), "Label").map(text))
        .or_null("label_text", label_text(ctx, set).map(text))
        .set("per_level", J::Obj(level_keys.into_iter().zip(level_values).collect()))
        .set("static", shared)
        .or_null("translation_file", file_name.map(text))
        .build()
}

/// A skill's own description file if it has one, then the generic one. The
/// name is reported as RePoE reports it, with the PoE 1 `.txt` extension on
/// the shared files.
fn pick_translation_file(
    ctx: &Ctx,
    skill_id: Option<&str>,
    position: usize,
) -> (Option<String>, Option<std::rc::Rc<TranslationLookup>>) {
    let candidates: Vec<String> = match skill_id {
        None => vec!["gem_stat_descriptions.txt".to_string()],
        Some(skill) => {
            let skill = skill.to_ascii_lowercase();
            vec![
                format!("specific_skill_stat_descriptions/{}/statset_{}.csd", skill, position),
                format!("specific_skill_stat_descriptions/{}.csd", skill),
                "skill_stat_descriptions.txt".to_string(),
            ]
        }
    };
    for candidate in candidates {
        let path = if candidate.contains('/') {
            format!("Data/StatDescriptions/{}", candidate)
        } else {
            format!("Data/StatDescriptions/{}", candidate.replace(".txt", ".csd"))
        };
        if ctx.files.exists(&path) {
            return (Some(candidate.clone()), Some(ctx.translations(&candidate)));
        }
    }
    (None, None)
}

#[allow(clippy::too_many_arguments)]
fn stat_set_level(
    ctx: &Ctx,
    set: Row<'_>,
    level: Row<'_>,
    primary: Row<'_>,
    primary_level: Option<Row<'_>>,
    translations: Option<&TranslationLookup>,
    quality: &Quality,
) -> J {
    let mut out = Obj::new();
    let multiplier = level.int("BaseMultiplier");
    if multiplier != 0 {
        let value = 100.0 + multiplier as f64 / 100.0;
        out = out.set("damage_multiplier", if value.fract() == 0.0 { int(value as i64) } else { J::Num(value) });
    }
    let crit = match (level.int("SpellCritChance"), level.int("AttackCritChance")) {
        (0, 0) => None,
        (0, attack) => Some(attack),
        (spell, _) => Some(spell),
    };
    if let Some(crit) = crit {
        out = out.set("crit_chance", int(crit));
    }

    let mut stats: Vec<(String, i64, &'static str)> = Vec::new();
    collect_stats(ctx, set, level, &mut stats);
    // A secondary set inherits the primary's numbers except the ones it names
    // as ignored.
    if set.index != primary.index {
        let ignored = ctx.rr.deref_list_ids(set, "IgnoredStats");
        let mut inherited: Vec<(String, i64, &'static str)> = Vec::new();
        if let Some(primary_level) = primary_level {
            collect_stats(ctx, primary, primary_level, &mut inherited);
        }
        stats.extend(inherited.into_iter().filter(|(id, _, _)| !ignored.contains(id)));
    }

    // The same stat listed twice adds up, except an implicit which just is.
    let mut merged: Vec<(String, i64, &'static str)> = Vec::new();
    for (id, value, kind) in stats {
        match merged.iter_mut().find(|(existing, _, _)| *existing == id) {
            Some(slot) if kind != "implicit" => slot.1 += value,
            Some(_) => {}
            None => merged.push((id, value, kind)),
        }
    }

    out = out.set(
        "stats",
        json::arr(merged.iter().map(|(id, value, kind)| {
            Obj::new().set("id", text(id)).set("type", text(*kind)).set("value", int(*value)).build()
        })),
    );

    if let Some(translations) = translations {
        let described: Vec<(String, i64)> =
            merged.iter().filter(|(_, v, _)| *v != 0).map(|(id, v, _)| (id.clone(), *v)).collect();
        let ids: Vec<String> = described.iter().map(|(id, _)| id.clone()).collect();
        let values: Vec<i32> = described.iter().map(|(_, v)| *v as i32).collect();
        let mut text_by_stats: Vec<(String, J)> = Vec::new();
        let mut order: Vec<(String, J)> = Vec::new();
        for line in translations.translate_detailed(&ids, &values) {
            let key = line.ids.iter().filter(|id| ids.contains(id)).cloned().collect::<Vec<_>>().join("\n");
            text_by_stats.push((key.clone(), text(&line.text)));
            order.push((key, int(line.index as i64)));
        }
        // Quality lines take their place in the tooltip too.
        for (key, index) in &quality.order {
            if !order.iter().any(|(existing, _)| existing == key) {
                order.push((key.clone(), int(*index as i64)));
            }
        }
        out = out.set("stat_order", J::Obj(order)).set("stat_text", J::Obj(text_by_stats));
        if !quality.shown.is_empty() {
            out = out.set("quality_stats", J::Arr(quality.shown.clone()));
        }
        if !quality.alternate.is_empty() {
            out = out.set("alternate_quality_stats", J::Arr(quality.alternate.clone()));
        }
    }

    out.build()
}

/// Reads one stat set level into `(id, value, kind)` triples.
fn collect_stats(ctx: &Ctx, set: Row<'_>, level: Row<'_>, out: &mut Vec<(String, i64, &'static str)>) {
    let float_values = level.list_int("BaseResolvedValues");
    for (i, stat) in ctx.rr.deref_list(level, "FloatStats").iter().enumerate() {
        out.push((stat.id(), float_values.get(i).copied().unwrap_or(0), "float"));
    }
    let constant_values = set.list_int("ConstantStatsValues");
    for (i, stat) in ctx.rr.deref_list(set, "ConstantStats").iter().enumerate() {
        out.push((stat.id(), constant_values.get(i).copied().unwrap_or(0), "constant"));
    }
    let additional_values = level.list_int("AdditionalStatsValues");
    for (i, stat) in ctx.rr.deref_list(level, "AdditionalStats").iter().enumerate() {
        out.push((stat.id(), additional_values.get(i).copied().unwrap_or(0), "additional"));
    }
    for stat in ctx.rr.deref_list(set, "ImplicitStats") {
        out.push((stat.id(), 1, "implicit"));
    }
    for stat in ctx.rr.deref_list(level, "AdditionalFlags") {
        out.push((stat.id(), 1, "flag"));
    }
}

/// The quality lines for one stat set. A `GrantedEffectQualityStats` row can
/// carry two bonuses; the client's gem tooltip draws the first and leaves the
/// second out, so they are reported apart rather than merged.
#[derive(Default)]
struct Quality {
    shown: Vec<J>,
    alternate: Vec<J>,
    order: Vec<(String, usize)>,
}

/// A bonus that names no stat sets belongs to all of them.
fn applies_to_set(row: Row<'_>, column: &str, position: usize) -> bool {
    let sets = row.list_int(column);
    sets.is_empty() || sets.contains(&(position as i64))
}

/// What quality adds to the stat set at `position`, described with the stat
/// names left in place of numbers so a consumer can scale them itself.
fn quality_stats(
    ctx: &Ctx,
    translations: &TranslationLookup,
    rows: &[usize],
    position: usize,
) -> Quality {
    let table = ctx.rr.table("GrantedEffectQualityStats");
    let mut out = Quality::default();
    for &index in rows {
        let Some(row) = table.as_ref().and_then(|t| t.row(index)) else { continue };
        if applies_to_set(row, "ApplyToStatSets", position) {
            if let Some((line, order)) = quality_bonus(ctx, translations, row, "Stats", "StatsValuesPermille") {
                out.shown.push(line);
                for (key, index) in order {
                    match out.order.iter_mut().find(|(existing, _)| *existing == key) {
                        Some(slot) => slot.1 = index,
                        None => out.order.push((key, index)),
                    }
                }
            }
        }
        if applies_to_set(row, "AltApplyToStatSets", position) {
            if let Some((line, _)) = quality_bonus(ctx, translations, row, "AltStats", "AltStatValuesPermille") {
                out.alternate.push(line);
            }
        }
    }
    out
}

/// One quality bonus rendered, with where each of its lines sits in the
/// tooltip.
fn quality_bonus(
    ctx: &Ctx,
    translations: &TranslationLookup,
    row: Row<'_>,
    stats_column: &str,
    values_column: &str,
) -> Option<(J, Vec<(String, usize)>)> {
    let permille = row.list_int(values_column);
    let stats: Vec<(String, i64)> = ctx
        .rr
        .deref_list(row, stats_column)
        .iter()
        .enumerate()
        .filter_map(|(i, stat)| permille.get(i).map(|v| (stat.id(), *v)))
        .collect();
    if stats.is_empty() {
        return None;
    }

    // Quality values are per mille, so the text depends on how far it is
    // scaled. The scale that fills in the most slots describes it best.
    let mut divisors: Vec<i64> = stats.iter().map(|(_, v)| v.abs().min(1000)).filter(|v| *v != 0).collect();
    divisors.push(25);
    divisors.sort_unstable();
    divisors.dedup();

    let ids: Vec<String> = stats.iter().map(|(id, _)| id.clone()).collect();
    let mut order: Vec<(String, usize)> = Vec::new();
    let mut best: Option<(usize, String)> = None;
    for divisor in divisors {
        let values: Vec<i32> = stats.iter().map(|(_, v)| (*v / divisor.max(1)) as i32).collect();
        let lines = translations.translate_detailed(&ids, &values);
        let slots: usize = lines.iter().map(|l| l.template.matches('{').count()).sum();
        let rendered = lines.iter().map(|l| l.template.clone()).collect::<Vec<_>>().join("\n");
        for line in &lines {
            let key = line.ids.iter().filter(|id| ids.contains(id)).cloned().collect::<Vec<_>>().join("\n");
            match order.iter_mut().find(|(existing, _)| *existing == key) {
                Some(slot) => slot.1 = line.index,
                None => order.push((key, line.index)),
            }
        }
        if best.as_ref().map(|(count, _)| slots > *count).unwrap_or(true) {
            best = Some((slots, rendered));
        }
    }

    let line = Obj::new()
        .set("stats", J::Obj(stats.into_iter().map(|(id, v)| (id, int(v))).collect()))
        .or_null("stat", best.map(|(_, rendered)| text(rendered)))
        .build();
    Some((line, order))
}

/// Row indices of `table` grouped by the row `column` points at.
fn group_by(ctx: &Ctx, table: &str, column: &str) -> HashMap<usize, Vec<usize>> {
    let Some(table) = ctx.rr.table(table) else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in table.rows() {
        if let Some(target) = row.key(column) {
            out.entry(target).or_default().push(row.index);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_k_icons_sit_in_a_sibling_folder() {
        assert_eq!(
            four_k("Art/2DArt/SkillIcons/AbyssalLivingBomb.dds").as_deref(),
            Some("Art/2DArt/SkillIcons/4k/AbyssalLivingBomb.dds")
        );
        assert_eq!(four_k(""), None);
    }

    /// The character level each of the twenty gem levels asks for, on the
    /// `PoE2GemProgressionRegular` curve every full-length gem uses.
    const CURVE: [i64; 20] = [0, 3, 6, 10, 14, 18, 22, 26, 31, 36, 41, 46, 52, 58, 64, 66, 72, 78, 84, 90];

    #[test]
    fn attribute_requirements_match_the_client() {
        // Boneshatter (100% strength), Frozen Locus and Armour Piercing Rounds
        // (50/50), and Malice's two halves (25% and 75%), read off the client.
        let expected: [(i64, [i64; 20]); 4] = [
            (100, [4, 9, 14, 21, 28, 35, 41, 48, 57, 65, 74, 82, 92, 103, 113, 116, 126, 137, 147, 157]),
            (75, [4, 8, 12, 17, 22, 28, 33, 38, 45, 51, 58, 64, 72, 80, 88, 91, 98, 106, 114, 122]),
            (50, [4, 7, 9, 13, 17, 20, 24, 28, 32, 37, 41, 46, 51, 57, 62, 64, 70, 75, 80, 86]),
            (25, [4, 5, 7, 9, 11, 13, 15, 17, 19, 22, 24, 26, 29, 32, 35, 36, 39, 42, 45, 48]),
        ];
        for (weight, wanted) in expected {
            let got: Vec<i64> = CURVE.iter().map(|level| attribute_requirement(*level, weight)).collect();
            assert_eq!(got, wanted, "weight {}%", weight);
        }
    }
}
