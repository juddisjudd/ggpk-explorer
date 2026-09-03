//! Single-table dumps: enumerations, lookup tables and the character/monster
//! base stat sheets.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;

pub fn active_skill_types(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("ActiveSkillType")?;
    let types = json::strings(table.rows().map(|r| r.id().to_string()));
    json::write(ctx.out, "active_skill_types", &types)
}

pub fn characters(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("Characters")?;
    let root = table
        .rows()
        .map(|row| {
            let unarmed = Obj::new()
                .set("attack_time", int(row.int("WeaponSpeed")))
                .set("max_physical_damage", int(row.int("MaxDamage")))
                .set("min_physical_damage", int(row.int("MinDamage")))
                .set("range", int(row.int("MaxAttackDistance")))
                .build();
            let base_stats = Obj::new()
                .set("dexterity", int(row.int("BaseDexterity")))
                .set("intelligence", int(row.int("BaseIntelligence")))
                .set("life", int(row.int("BaseMaxLife")))
                .set("mana", int(row.int("BaseMaxMana")))
                .set("strength", int(row.int("BaseStrength")))
                .set("unarmed", unarmed)
                .build();
            Obj::new()
                .set("base_stats", base_stats)
                .set("integer_id", int(row.int("IntegerId")))
                .set("metadata_id", text(row.id()))
                .set("name", text(row.str("Name")))
                .set("description", text(row.str("Description")))
                .build()
        })
        .collect::<Vec<_>>();
    json::write(ctx.out, "characters", &J::Arr(root))
}

pub fn cost_types(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("CostTypes")?;
    let stat = table.require(&["Stat", "StatsKey"])?;
    let root = table
        .rows()
        .map(|row| {
            let entry = Obj::new()
                .set("format_text", text(row.str("FormatText")))
                .or_null("stat", ctx.rr.deref_id(row, stat).map(text))
                .build();
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    json::write(ctx.out, "cost_types", &J::Obj(root))
}

pub fn default_monster_stats(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("DefaultMonsterStats")?;
    let life = table.require(&["MonsterLife", "Life"])?;
    let ally_life = table.require(&["AllyLife", "MinionLife"])?;
    let root = table
        .rows()
        .map(|row| {
            let entry = Obj::new()
                .set("accuracy", int(row.int("Accuracy")))
                .set("ally_life", int(row.int(ally_life)))
                .set("armour", int(row.int("Armour")))
                .set("evasion", int(row.int("Evasion")))
                .set("life", int(row.int(life)))
                .set("experience", int(row.int("Experience")))
                .set("physical_damage", json::float(row.float("Damage")))
                .build();
            (row.str("DisplayLevel").to_string(), entry)
        })
        .collect::<Vec<_>>();
    json::write(ctx.out, "default_monster_stats", &J::Obj(root))
}

pub fn flavour(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("FlavourText")?;
    let mut root: Vec<(String, J)> = Vec::new();
    for row in table.rows() {
        let id = row.id().to_string();
        if root.iter().any(|(k, _)| *k == id) {
            continue; // first definition wins, as in RePoE
        }
        root.push((id, text(row.str("Text"))));
    }
    json::write(ctx.out, "flavour", &J::Obj(root))
}

pub fn gem_tags(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("GemTags")?;
    let root = table
        .rows()
        .map(|row| (row.id().to_string(), json::opt_text(row.str("Name")).unwrap_or(J::Null)))
        .collect::<Vec<_>>();
    json::write(ctx.out, "gem_tags", &J::Obj(root))
}

pub fn item_classes(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("ItemClasses")?;
    let root = table
        .rows()
        .map(|row| {
            let category = ctx.rr.deref(row, "ItemClassCategory");
            let entry = Obj::new()
                .or_null("category", category.as_ref().map(|c| text(c.row().str("Text"))))
                .or_null("category_id", category.as_ref().map(|c| text(c.row().id())))
                .set("name", text(row.str("Name")))
                .or_null("influence_tags", None)
                .build();
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    json::write(ctx.out, "item_classes", &J::Obj(root))
}

pub fn keywords(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("KeywordPopups")?;
    let mut root = table
        .rows()
        .map(|row| {
            let entry = Obj::new()
                .set("definition", text(row.str("Definition")))
                .set("term", text(row.str("Term")))
                .build();
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    root.sort_by(|a, b| a.0.cmp(&b.0));
    json::write(ctx.out, "keywords", &J::Obj(root))
}

pub fn tags(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("Tags")?;
    let names = json::strings(table.rows().map(|r| r.id().to_string()));
    json::write(ctx.out, "tags", &names)?;

    let mut details = table
        .rows()
        .map(|row| {
            let display = row.str("DisplayString");
            let entry = Obj::new()
                .set("name", text(display))
                .set("used_in_crafting", J::Bool(!display.is_empty()))
                .build();
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    details.sort_by(|a, b| a.0.cmp(&b.0));
    json::write(ctx.out, "tag_details", &J::Obj(details))
}
