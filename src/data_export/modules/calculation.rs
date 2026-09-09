//! What the client's damage arithmetic reads: the tuning constants it looks up
//! by name, the stat ids each hit context draws on, and the base numbers the
//! character and monster objects start from. The formulas themselves live in
//! the client; these are their inputs.

use crate::dat::relational::{FileSource, LoadedTable, Row};
use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;

pub fn game_constants(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("GameConstants")?;
    let root = table
        .rows()
        .map(|row| {
            let value = row.int("Value") as f64;
            let divisor = row.int("Divisor") as f64;
            let ratio = if divisor == 0.0 {
                J::Null
            } else if (value / divisor).fract() == 0.0 {
                int((value / divisor) as i64)
            } else {
                J::Num(value / divisor)
            };
            (row.id().to_string(), ratio)
        })
        .collect::<Vec<_>>();
    ctx.write("game_constants", &J::Obj(root))
}

const ELEMENTS: [&str; 5] = ["physical", "fire", "cold", "lightning", "chaos"];

pub fn damage_calculation_types(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("DamageCalculationTypes")?;
    let root = table
        .rows()
        .map(|row| (row.id().to_string(), calculation_type(ctx, &table, row)))
        .collect::<Vec<_>>();
    ctx.write("damage_calculation_types", &J::Obj(root))
}

fn calculation_type(ctx: &Ctx, table: &LoadedTable, row: Row<'_>) -> J {
    let stat = |col: &str| -> J {
        match table.has_col(col) {
            true => ctx.rr.deref_id(row, col).map(text).unwrap_or(J::Null),
            false => J::Null,
        }
    };
    let per_element = |suffix: &str| -> J {
        let mut out = Obj::new();
        for element in ELEMENTS {
            let column = format!("{}{}{}", &element[..1].to_ascii_uppercase(), &element[1..], suffix);
            out = out.set(element, stat(&column));
        }
        out.build()
    };

    // Bucketed by the element the stat id names, not by column: the community
    // schema has had the cold and lightning columns crossed.
    let mut damage: Vec<(&str, Vec<J>)> = ELEMENTS.iter().map(|e| (*e, Vec::new())).collect();
    for col in ["PhysicalDamageStats", "FireDamageStats", "ColdDamageStats", "LightningDamageStats", "ChaosDamageStats"] {
        for id in ctx.rr.deref_list_ids(row, col) {
            if let Some(slot) = damage.iter_mut().find(|(e, _)| id.contains(&format!("_{}_", e))) {
                slot.1.push(text(&id));
            }
        }
    }
    let damage = damage.into_iter().map(|(e, ids)| (e.to_string(), J::Arr(ids))).collect();

    let crit = Obj::new()
        .set("chance", stat("CritChanceStat"))
        .set("base_chance", stat("BaseCritChanceStat"))
        .set("double_chance", stat("DoubleCritChanceStat"))
        .set("bonus", stat("CritBonusStat"))
        .set("always", stat("AlwaysCritStat"))
        .set("cannot", stat("CannotCritStat"))
        .set("eventual", stat("EventualCritStat"))
        .build();
    let accuracy = Obj::new()
        .set("rating", stat("AccuracyRatingStat"))
        .set("increased", stat("AccuracyRatingIncreasedStat"))
        .set("final", stat("AccuracyRatingFinalStat"))
        .set("from_minion_parent", stat("AccuracyOverrideFromMinionParentStat"))
        .set("unaffected_by_distance", stat("AccuracyUnaffectedByDistanceStat"))
        .set("always_hit", stat("AlwaysHitStat"))
        .set("can_be_evaded", stat("CanEvadeStat"))
        .build();
    let stun = Obj::new()
        .set("threshold", stat("StunThresholdStat"))
        .set("duration", stat("StunDurationStat"))
        .set("multipliers", per_element("StunMultiplierStat"))
        .build();
    let knockback = Obj::new()
        .set("chance", stat("KnockbackChanceStat"))
        .set("on_hit", stat("KnockbackStat"))
        .set("on_crit", stat("KnockbackOnCritStat"))
        .build();

    Obj::new()
        .set("text", text(row.str("Text")))
        .set("type", int(row.int("Type")))
        .set("is_attack", J::Bool(row.bool("IsAttack")))
        .set("context", ctx.rr.deref_id(row, "StatContextFlags").map(text).unwrap_or(J::Null))
        .set("fake_hit_for_ailments", stat("FakeHitForAilments"))
        .set(
            "is_fake_hit_for_ailments",
            match table.has_col("IsFakeHitForAilments") {
                true => J::Bool(row.bool("IsFakeHitForAilments")),
                false => J::Null,
            },
        )
        .set("damage", J::Obj(damage))
        .set("crit", crit)
        .set("accuracy", accuracy)
        .set("stun", stun)
        .set("freeze_multipliers", per_element("FreezeMultiplierStat"))
        .set("pin_multipliers", per_element("PinMultiplierStat"))
        .set("cannot_be_blocked", stat("CannotBlockStat"))
        .set("knockback", knockback)
        .build()
}

pub fn character_constants(ctx: &Ctx) -> Result<(), String> {
    let root = Obj::new()
        .set("character", object_stats(ctx, "Metadata/Characters/Character.ot", &["Stats", "Pathfinding"])?)
        .set("monster", object_stats(ctx, "Metadata/Monsters/Monster.ot", &["Stats"])?)
        .build();
    ctx.write("character_constants", &root)
}

/// The `key = value` lines of the named blocks, in file order.
fn object_stats(ctx: &Ctx, path: &str, blocks: &[&str]) -> Result<J, String> {
    let bytes = ctx.files.fetch(path).ok_or_else(|| format!("{} is not in the index", path))?;
    let file = crate::parsers::object_dsl::parse(&crate::parsers::utils::decode_text_lossy(&bytes));
    let mut out: Vec<(String, J)> = Vec::new();
    for component in file.components.iter().filter(|c| blocks.contains(&c.name.as_str())) {
        for prop in &component.props {
            out.push((prop.key.clone(), scalar(&prop.value)));
        }
    }
    Ok(J::Obj(out))
}

fn scalar(value: &str) -> J {
    match value {
        "true" => J::Bool(true),
        "false" => J::Bool(false),
        v => v
            .parse::<i64>()
            .map(int)
            .or_else(|_| v.parse::<f64>().map(J::Num))
            .unwrap_or_else(|_| text(v)),
    }
}

/// Every stat with the flags the client combines it by: how it aggregates,
/// whether it is local to the item or weapon hand, whether the client computes
/// it, and which calculation contexts it belongs to.
pub fn stats(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("Stats")?;
    let root = table
        .rows()
        .filter(|row| !row.id().is_empty())
        .map(|row| {
            let ids = |col: &str| -> Option<J> {
                let ids = ctx.rr.deref_list_ids(row, col);
                (!ids.is_empty()).then(|| json::strings(&ids))
            };
            let entry = Obj::new()
                .or_null("semantic", ctx.rr.enum_label(row, "Semantic").map(|s| text(s.to_ascii_lowercase())))
                .set("is_local", J::Bool(row.bool("IsLocal")))
                .set("is_weapon_local", J::Bool(row.bool("IsWeaponLocal")))
                .set("is_virtual", J::Bool(row.bool("IsVirtual")))
                .set("is_scalable", J::Bool(row.bool("IsScalable")))
                .set("weapon_hand_check", J::Bool(row.bool("WeaponHandCheck")))
                .or_null("main_hand_alias", ctx.rr.deref_id(row, "MainHandAlias_Stat").map(text))
                .or_null("off_hand_alias", ctx.rr.deref_id(row, "OffHandAlias_Stat").map(text))
                .or_null("context_flags", ids("ContextFlags"))
                .or_null("dot_flags", ids("DotFlag"))
                .or_null("category", ctx.rr.deref_id(row, "Category").map(text))
                .or_null("active_skills", ids("BelongsActiveSkills"))
                .build();
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    ctx.write("stats", &J::Obj(root))
}

/// Which player stats each minion stat is fed from, and the minion types it
/// applies to.
pub fn minion_stats(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("MinionStats")?;
    let root = table
        .rows()
        .filter_map(|row| {
            let id = ctx.rr.deref_id(row, "MinionStat")?;
            let mut types = ctx.rr.deref_list_ids(row, "MinionType");
            types.extend(ctx.rr.deref_list_ids(row, "MinionType2"));
            let entry = Obj::new()
                .set("player_stats", json::strings(ctx.rr.deref_list_ids(row, "PlayerStat")))
                .set("minion_types", json::strings(types))
                .set("is_companion_stat", J::Bool(row.bool("CompanionStat")))
                .build();
            Some((id, entry))
        })
        .collect::<Vec<_>>();
    ctx.write("minion_stats", &J::Obj(root))
}

/// How attack skills scale with gem level: the damage multiplier per level for
/// each scaling type, and the flat physical damage the unarmed curve adds.
pub fn attack_damage_scaling(ctx: &Ctx) -> Result<(), String> {
    let types = ctx.table("AttackSkillDamageScalingType")?;
    let multipliers = ctx.table("AttackSkillDamageScalingValues")?;
    let flat = ctx.table("FlatPhysicalDamageValues")?;
    let root = types
        .rows()
        .map(|kind| {
            let per_level = |table: &crate::dat::relational::LoadedTable, key: &str, value: &dyn Fn(Row<'_>) -> J| {
                let mut out: Vec<(String, J)> = table
                    .rows()
                    .filter(|r| r.key(key) == Some(kind.index))
                    .map(|r| (r.int("GemLevel").to_string(), value(r)))
                    .collect();
                out.sort_by_key(|(level, _)| level.parse::<i64>().unwrap_or(0));
                (!out.is_empty()).then(|| J::Obj(out))
            };
            let entry = Obj::new()
                .or_null("multipliers", per_level(&multipliers, "SkillType", &|r| json::float(r.float("Scaling"))))
                .or_null(
                    "flat_physical",
                    per_level(&flat, "ScalingType", &|r| json::arr([int(r.int("MinPhys")), int(r.int("MaxPhys"))])),
                )
                .build();
            (kind.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    ctx.write("attack_damage_scaling", &J::Obj(root))
}
