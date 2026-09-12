//! The enemy side of a damage calculation: each monster variety's multipliers,
//! base defences and resistances, and how monsters scale with the player's
//! level, the area level and the map level.

use crate::dat::relational::Row;
use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;

pub fn monsters(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("MonsterVarieties")?;
    let root = table
        .rows()
        .filter(|row| !row.id().is_empty())
        .map(|row| (row.id().to_string(), variety(ctx, row)))
        .collect::<Vec<_>>();
    ctx.write("monsters", &J::Obj(root))
}

fn variety(ctx: &Ctx, row: Row<'_>) -> J {
    let ids = |col: &str| -> Option<J> {
        let ids = ctx.rr.deref_list_ids(row, col);
        (!ids.is_empty()).then(|| json::strings(&ids))
    };
    let kind = ctx.rr.deref(row, "MonsterType");
    let base = kind.as_ref().map(|t| {
        let t = t.row();
        Obj::new()
            .set("armour", int(t.int("Armour")))
            .set("evasion", int(t.int("Evasion")))
            .set("energy_shield_from_life", int(t.int("EnergyShieldFromLife")))
            .set("damage_spread", int(t.int("DamageSpread")))
            .set("is_summoned", J::Bool(t.bool("IsSummoned")))
            .build()
    });
    let resistances = kind
        .as_ref()
        .and_then(|t| ctx.rr.deref_list(t.row(), "MonsterResistances").into_iter().next())
        .map(|r| resistances(r.row()));

    Obj::new()
        .set("name", text(row.str("Name")))
        .or_null("monster_type", kind.as_ref().map(|t| text(t.id())))
        .or_null("category", ctx.rr.deref(row, "MonsterCategory").map(|c| text(c.row().str("Name"))))
        .or_null("tags", ids("Tags"))
        .set("life_multiplier", int(row.int("LifeMultiplier")))
        .set("damage_multiplier", int(row.int("DamageMultiplier")))
        .set("experience_multiplier", int(row.int("ExperienceMultiplier")))
        .set("attack_time", int(row.int("AttackSpeed")))
        .or_null("attack_crit_chance", row.opt_int("AttackCrit").map(|_| json::float(row.float("AttackCrit"))))
        .or_null("poise_threshold", row.opt_int("PoiseThreshold").map(int))
        .set("movement_speed", int(row.int("MovementSpeed")))
        .set("object_size", int(row.int("ObjectSize")))
        .set("model_size_multiplier", int(row.int("ModelSizeMultiplier")))
        .set(
            "attack_distance",
            json::arr([int(row.int("MinimumAttackDistance")), int(row.int("MaximumAttackDistance"))]),
        )
        .set("has_boss_health_bar", J::Bool(row.bool("BossHealthBar")))
        .or_null("main_hand_item_class", ctx.rr.deref_id(row, "MainHand_ItemClass").map(text))
        .or_null("off_hand_item_class", ctx.rr.deref_id(row, "OffHand_ItemClass").map(text))
        .or_null("base", base)
        .or_null("resistances", resistances)
        .or_null("mods", ids("Mods"))
        .or_null("special_mods", ids("Special_Mods"))
        .or_null("endgame_mods", ids("Endgame_Mods"))
        .or_null("granted_effects", ids("GrantedEffects"))
        .or_null("inherits_from", {
            let parents = row.list_str("InheritsFrom");
            (!parents.is_empty()).then(|| json::strings(parents))
        })
        .build()
}

pub fn level_scaling(ctx: &Ctx) -> Result<(), String> {
    let by_player = ctx.table("LevelRelativePlayerScaling")?;
    let player_level = by_player
        .rows()
        .map(|row| {
            let entry = Obj::new().set("monster_level", int(row.int("MonsterLevel"))).build();
            (row.int("PlayerLevel").to_string(), entry)
        })
        .collect::<Vec<_>>();

    let by_area = ctx.table("ResistancePenaltyPerAreaLevel")?;
    let area_level = by_area
        .rows()
        .map(|row| {
            let entry = Obj::new().set("resistance_penalty", int(row.int("Penalty"))).build();
            (row.int("AreaLevel").to_string(), entry)
        })
        .collect::<Vec<_>>();

    let mut map_level: Vec<(String, Obj)> = Vec::new();
    if let Some(packs) = ctx.optional_table("MonsterMapDifficulty") {
        let level = packs.require(&["MapLevel", "AreaLevel"])?;
        for row in packs.rows() {
            let at = slot(&mut map_level, row.int(level));
            let entry = std::mem::take(&mut map_level[at].1)
                .set("life_percent_increase", int(row.int("LifePercentIncrease")))
                .set("damage_percent_increase", int(row.int("DamagePercentIncrease")));
            map_level[at].1 = entry;
        }
    }
    if let Some(bosses) = ctx.optional_table("MonsterMapBossDifficulty") {
        let level = bosses.require(&["MapLevel", "AreaLevel"])?;
        for row in bosses.rows() {
            let at = slot(&mut map_level, row.int(level));
            let entry = std::mem::take(&mut map_level[at].1)
                .set("boss_life_percent_increase", int(row.int("BossLifePercentIncrease")))
                .set("boss_damage_percent_increase", int(row.int("BossDamagePercentIncrease")))
                .set("boss_ailment_percent_decrease", int(row.int("BossAilmentPercentDecrease")));
            map_level[at].1 = entry;
        }
    }
    map_level.sort_by_key(|(k, _)| k.parse::<i64>().unwrap_or(0));
    let map_level = map_level.into_iter().map(|(k, obj)| (k, obj.build())).collect::<Vec<_>>();

    let root = Obj::new()
        .set("player_level", J::Obj(player_level))
        .set("area_level", J::Obj(area_level))
        .set("map_level", J::Obj(map_level))
        .build();
    ctx.write("level_scaling", &root)
}

/// Index of the entry for `level`, adding one when it is new.
fn slot(entries: &mut Vec<(String, Obj)>, level: i64) -> usize {
    let key = level.to_string();
    match entries.iter().position(|(k, _)| *k == key) {
        Some(at) => at,
        None => {
            entries.push((key, Obj::new()));
            entries.len() - 1
        }
    }
}

/// PoE 1 lists a value per difficulty; PoE 2 lists five unnamed slots per
/// element, each an array, which are reported as the table holds them.
fn resistances(row: Row<'_>) -> J {
    const ELEMENTS: [(&str, &str); 4] = [("fire", "Fire"), ("cold", "Cold"), ("lightning", "Lightning"), ("chaos", "Chaos")];
    let mut out = Obj::new().set("id", text(row.id()));
    if row.table.has_col("FireNormal") {
        for tier in ["Normal", "Cruel", "Merciless"] {
            let mut values = Obj::new();
            for (key, column) in ELEMENTS {
                values = values.set(key, int(row.int(&format!("{}{}", column, tier))));
            }
            out = out.set(&tier.to_ascii_lowercase(), values.build());
        }
    } else {
        for (key, column) in ELEMENTS {
            let slots = (1..=5)
                .map(|n| json::arr(row.list_int(&format!("{}{}", column, n)).into_iter().map(int)))
                .collect::<Vec<_>>();
            out = out.set(key, J::Arr(slots));
        }
    }
    out.build()
}
