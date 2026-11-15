//! `Data/StatDescriptions/*.lua`, ported from `Export/Scripts/statdesc.lua`.

use crate::dat::schema::{Column, Table as TableDef, TableReference};
use crate::data_export::Ctx;
use crate::pob_export::{game, read_text, statdesc, write, Table};
use crate::settings::Game;
use std::collections::HashMap;
use std::rc::Rc;

const POE2_FILES: [&str; 10] = [
    "active_skill_gem_stat_descriptions",
    "advanced_mod_stat_descriptions",
    "gem_stat_descriptions",
    "meta_gem_stat_descriptions",
    "monster_stat_descriptions",
    "passive_skill_aura_stat_descriptions",
    "passive_skill_stat_descriptions",
    "skill_stat_descriptions",
    "stat_descriptions",
    "utility_flask_buff_stat_descriptions",
];

const POE1_FILES: [&str; 23] = [
    "active_skill_gem_stat_descriptions",
    "aura_skill_stat_descriptions",
    "banner_aura_skill_stat_descriptions",
    "beam_skill_stat_descriptions",
    "brand_skill_stat_descriptions",
    "curse_skill_stat_descriptions",
    "debuff_skill_stat_descriptions",
    "secondary_debuff_skill_stat_descriptions",
    "gem_stat_descriptions",
    "minion_attack_skill_stat_descriptions",
    "minion_skill_stat_descriptions",
    "minion_spell_skill_stat_descriptions",
    "minion_spell_damage_skill_stat_descriptions",
    "single_minion_spell_skill_stat_descriptions",
    "monster_stat_descriptions",
    "offering_skill_stat_descriptions",
    "skill_stat_descriptions",
    "stat_descriptions",
    "variable_duration_skill_stat_descriptions",
    "buff_skill_stat_descriptions",
    "tincture_stat_descriptions",
    "graft_stat_descriptions",
    "passive_skill_stat_descriptions",
];

/// PoE 2 also exports every skill's own file, flattening its folder into the
/// file name: `Specific_Skill_Stat_Descriptions/ancestral_cry/statset_0.csd`
/// becomes `ancestral_cry_statset_0`.
const SPECIFIC: &str = "Data/StatDescriptions/Specific_Skill_Stat_Descriptions/";

pub fn stat_descriptions(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let (dir, ext) = statdesc::description_dir(game);
    let names: &[&str] = match game {
        Game::Poe2 => &POE2_FILES,
        Game::Poe1 => &POE1_FILES,
    };
    let mut written = 0;
    for name in names {
        let path = format!("{}{}{}", dir, name, ext);
        let text = read_text(ctx, &path).or_else(|| statdesc::other_format(&path).and_then(|other| read_text(ctx, &other)));
        let Some(text) = text else { continue };
        write(ctx, &format!("StatDescriptions/{}", name), statdesc::export_file(&text, game))?;
        written += 1;
    }
    if game == Game::Poe2 {
        for path in ctx.files.list_dir(SPECIFIC) {
            let Some(rest) = path.get(SPECIFIC.len()..) else { continue };
            let Some(stem) = rest.strip_suffix(ext) else { continue };
            let Some(text) = read_text(ctx, &path) else { continue };
            let flat = stem.to_ascii_lowercase().replace('/', "_");
            write(ctx, &format!("StatDescriptions/Specific_Skill_Stat_Descriptions/{}", flat), statdesc::export_file(&text, game))?;
            written += 1;
        }
    }
    if written == 0 {
        return Err(format!("no description files under {}", dir));
    }
    if game == Game::Poe1 {
        let names = formatter_names(ctx);
        let mut keys: Vec<&String> = names.keys().collect();
        keys.sort();
        let mut out = Table::new();
        for key in keys {
            out.set(key.as_str(), Table::list(names[key].iter().map(String::as_str)));
        }
        write(ctx, "StatDescriptionFormatters", out)?;
    }
    Ok(())
}

/// PoE 1's `getDescriptionFormatterNames` (3.30 on): each formatter function's names, by row.
pub fn formatter_names(ctx: &Ctx) -> Rc<statdesc::FormatterNames> {
    if let Some(hit) = ctx.pob.formatters.borrow().as_ref() {
        return Rc::clone(hit);
    }
    let names = Rc::new(read_formatter_names(ctx));
    *ctx.pob.formatters.borrow_mut() = Some(Rc::clone(&names));
    names
}

fn read_formatter_names(ctx: &Ctx) -> statdesc::FormatterNames {
    let mut out = HashMap::new();
    if let Some(radii) = ctx.rr.table("PassiveJewelRadii") {
        if let Some(col) = radii.pick(&["Name", "ClientString"]) {
            let names = radii.rows().map(|row| ctx.rr.deref(row, col).map(|s| s.row().string("Text")).unwrap_or_default());
            out.insert("passive_jewel_radius".to_string(), names.collect());
        }
    }
    if let Some(strings) = ctx.rr.table("ClientStrings") {
        let slots = strings.rows().filter(|row| row.id().starts_with("PelagicPearlSlotType")).map(|row| row.string("Text"));
        out.insert("pelagic_pearl_slot".to_string(), slots.collect());
    }
    for (key, table, columns, name) in formatter_tables() {
        let def = TableDef { name: table.to_string(), columns, tags: None, valid_for: None, custom: true };
        if let Some(loaded) = ctx.rr.table_as(table, def) {
            out.insert(key.to_string(), loaded.rows().map(|row| row.string(name)).collect());
        }
    }
    out
}

/// The tables PoB's spec lays out further than dat-schema does: formatter key, table, columns, name column.
fn formatter_tables() -> Vec<(&'static str, &'static str, Vec<Column>, &'static str)> {
    let col = |name: Option<&str>, ty: &str, references: Option<&str>| Column {
        name: name.map(str::to_string),
        description: None,
        array: false,
        r#type: ty.to_string(),
        unique: false,
        localized: false,
        references: references.map(|t| TableReference { table: t.to_string(), column: None }),
        interval: false,
        file: None,
        files: None,
    };
    let id = || col(Some("Id"), "i32", None);
    let name = || col(Some("Name"), "string", None);
    vec![
        ("fishing_lure_type", "FishingLureTypes", vec![id(), name()], "Name"),
        ("fishing_hook_type", "FishingHookTypes", vec![id(), name()], "Name"),
        (
            "shaper_apparition_ability",
            "ShaperApparitionAbility",
            vec![id(), col(Some("GrantedEffect"), "foreignrow", Some("GrantedEffects")), name()],
            "Name",
        ),
        (
            "minion_doubling_type",
            "MinionDoublingStatTypes",
            vec![
                id(),
                col(Some("Stat"), "foreignrow", Some("Stats")),
                col(Some("MinionType"), "foreignrow", Some("MinionType")),
                name(),
            ],
            "Name",
        ),
        (
            "atlas_exile_citadel_index",
            "AtlasExiles",
            vec![
                col(Some("Id"), "string", None),
                col(None, "i32", None),
                col(Some("InfluencedItemIncrStat"), "foreignrow", Some("Stats")),
                col(Some("MapIcon"), "string", None),
                col(Some("MapIcon2"), "string", None),
                col(Some("Influence"), "string", None),
                col(Some("CitadelName"), "string", None),
            ],
            "CitadelName",
        ),
        (
            "elder_guardian_index",
            "ElderGuardians",
            vec![col(Some("Id"), "string", None), col(Some("MapIcon"), "string", None), name()],
            "Name",
        ),
    ]
}
