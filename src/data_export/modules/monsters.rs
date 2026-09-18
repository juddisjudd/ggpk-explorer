//! The enemy side of a damage calculation: each monster variety's multipliers,
//! base defences and resistances, and how monsters scale with the player's
//! level, the area level and the map level.

use crate::dat::relational::Row;
use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;

pub fn monsters(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("MonsterVarieties")?;
    let object_type = table.pick(&["BaseMonsterTypeIndex", "ObjectType"]);
    let paths: Vec<String> = table
        .rows()
        .filter_map(|row| object_type.map(|column| row.string(column)))
        .filter(|path| !path.is_empty())
        .collect();
    let mut objects = ObjectStats::load(ctx, paths);
    let spectres = spectre_overrides(ctx);
    let root = table
        .rows()
        .filter(|row| !row.id().is_empty())
        .map(|row| {
            let path = object_type.map(|column| row.str(column)).unwrap_or_default();
            let mut entry = variety(ctx, row);
            entry.set("object_type", json::opt_text(path).unwrap_or(J::Null));
            entry.set("object_stats", if path.is_empty() { J::Null } else { J::Obj(objects.for_path(path)) });
            entry.set("spectre_override", spectres.get(&row.index).map(text).unwrap_or(J::Null));
            (row.id().to_string(), entry)
        })
        .collect::<Vec<_>>();
    ctx.write("monsters", &J::Obj(root))
}

/// The variety a monster becomes when raised as a spectre, where it differs.
fn spectre_overrides(ctx: &Ctx) -> std::collections::HashMap<usize, String> {
    let Some(table) = ctx.optional_table("SpectreOverrides") else { return Default::default() };
    table
        .rows()
        .filter_map(|row| Some((row.key("Monster")?, ctx.rr.deref_id(row, "Spectre")?)))
        .collect()
}

/// `Monster.ot` is where every chain ends; its stats are `character_constants.json`'s `monster`.
const MONSTER_ROOT: &str = "Metadata/Monsters/Monster";

/// The `Stats` blocks of a monster's `.ot` file and every file it extends
/// short of the shared root, parents first, a child's value replacing its
/// parent's. Cached per path, since varieties share a few hundred parents.
struct ObjectStats {
    files: std::collections::HashMap<String, crate::parsers::object_dsl::ObjectFile>,
    cache: std::collections::HashMap<String, Vec<(String, J)>>,
}

fn parents_of(file: &crate::parsers::object_dsl::ObjectFile) -> impl Iterator<Item = &str> {
    file.parents.iter().map(String::as_str).filter(|p| *p != "nothing" && *p != MONSTER_ROOT)
}

fn merge(stats: &mut Vec<(String, J)>, key: &str, value: J) {
    match stats.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value,
        None => stats.push((key.to_string(), value)),
    }
}

impl ObjectStats {
    /// Reads every file the chains need up front, one generation of parents
    /// at a time, so each batch goes through the bundles in order.
    fn load(ctx: &Ctx, paths: Vec<String>) -> Self {
        let mut files = std::collections::HashMap::new();
        let mut pending: Vec<String> = paths;
        while !pending.is_empty() {
            pending.sort_by_key(|p| p.to_ascii_lowercase());
            pending.dedup_by_key(|p| p.to_ascii_lowercase());
            let requests: Vec<String> = pending.iter().map(|p| format!("{}.ot", p)).collect();
            let fetched = ctx.files.fetch_many(&requests);
            let mut next = Vec::new();
            for path in pending.drain(..) {
                let key = path.to_ascii_lowercase();
                let Some(bytes) = fetched.get(&format!("{}.ot", path)) else { continue };
                let file = crate::parsers::object_dsl::parse(&crate::parsers::utils::decode_text_lossy(bytes));
                for parent in parents_of(&file) {
                    if !files.contains_key(&parent.to_ascii_lowercase()) {
                        next.push(parent.to_string());
                    }
                }
                files.insert(key, file);
            }
            next.retain(|p| !files.contains_key(&p.to_ascii_lowercase()));
            pending = next;
        }
        Self { files, cache: Default::default() }
    }

    fn for_path(&mut self, path: &str) -> Vec<(String, J)> {
        let key = path.to_ascii_lowercase();
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        // Guards a cycle; the real entry replaces it below.
        self.cache.insert(key.clone(), Vec::new());
        let mut stats = Vec::new();
        if let Some(file) = self.files.get(&key).cloned() {
            for parent in parents_of(&file) {
                for (k, v) in self.for_path(parent) {
                    merge(&mut stats, &k, v);
                }
            }
            for prop in file.components.iter().filter(|c| c.name == "Stats").flat_map(|c| &c.props) {
                merge(&mut stats, &prop.key, super::calculation::scalar(&prop.value));
            }
        }
        self.cache.insert(key, stats.clone());
        stats
    }
}

fn variety(ctx: &Ctx, row: Row<'_>) -> J {
    let ids = |col: &str| -> Option<J> {
        let ids = ctx.rr.deref_list_ids(row, col);
        (!ids.is_empty()).then(|| json::strings(&ids))
    };
    let column = |names: &[&'static str]| row.table.pick(names).unwrap_or(names[0]);
    let kind = ctx.rr.deref(row, column(&["MonsterType", "MonsterTypesKey"]));
    let base = kind.as_ref().map(|t| {
        let t = t.row();
        let flag = |name: &str| t.table.has_col(name).then(|| J::Bool(t.bool(name)));
        let mut base = Obj::new()
            .set("armour", int(t.int("Armour")))
            .set("evasion", int(t.int("Evasion")))
            .or_null("energy_shield_from_life", t.table.has_col("EnergyShieldFromLife").then(|| int(t.int("EnergyShieldFromLife"))))
            .set("damage_spread", int(t.int("DamageSpread")))
            .or_null("is_summoned", flag("IsSummoned"));
        if !ctx.rr.is_poe2 {
            base = base
                .set("energy_shield", int(t.int("EnergyShield")))
                .set("accuracy", int(t.int("Accuracy")))
                .or_null("is_player_minion", flag("IsPlayerMinion"));
        }
        base
            .or_null(
                "base_damage_ignores_attack_speed",
                t.table
                    .column_or_after(&["BaseDamageIgnoresAttackSpeed"], "MonsterResistances", 1)
                    .map(|c| J::Bool(t.bool_at(c))),
            )
            .build()
    });
    // PoE 2 lists resistance rows, PoE 1 names one.
    let resistances = kind
        .as_ref()
        .and_then(|t| {
            ctx.rr.deref_list(t.row(), "MonsterResistances").into_iter().next().or_else(|| ctx.rr.deref(t.row(), "Resistances"))
        })
        .map(|r| resistances(r.row()));

    Obj::new()
        .set("name", text(row.str("Name")))
        .or_null("monster_type", kind.as_ref().map(|t| text(t.id())))
        .or_null("category", ctx.rr.deref(row, "MonsterCategory").map(|c| text(c.row().str("Name"))))
        .or_null("tags", ids(column(&["Tags", "TagsKeys"])))
        .set("life_multiplier", int(row.int("LifeMultiplier")))
        .set("damage_multiplier", int(row.int("DamageMultiplier")))
        .set("experience_multiplier", int(row.int("ExperienceMultiplier")))
        .set("attack_time", int(row.int("AttackSpeed")))
        .or_null("attack_crit_chance", row.opt_int("AttackCrit").map(|_| json::float32(row.float("AttackCrit"))))
        .or_null("poise_threshold", row.opt_int("PoiseThreshold").map(int))
        .set("movement_speed", int(row.int("MovementSpeed")))
        .set("object_size", int(row.int("ObjectSize")))
        .set("model_size_multiplier", int(row.int("ModelSizeMultiplier")))
        .set(
            "attack_distance",
            json::arr([int(row.int("MinimumAttackDistance")), int(row.int("MaximumAttackDistance"))]),
        )
        .set("has_boss_health_bar", J::Bool(row.bool("BossHealthBar")))
        .or_null("main_hand_item_class", ctx.rr.deref_id(row, column(&["MainHand_ItemClass", "MainHand_ItemClassesKey"])).map(text))
        .or_null("off_hand_item_class", ctx.rr.deref_id(row, column(&["OffHand_ItemClass", "OffHand_ItemClassesKey"])).map(text))
        .or_null("base", base)
        .or_null("resistances", resistances)
        .or_null("mods", ids(column(&["Mods", "ModsKeys"])))
        .or_null("special_mods", ids(column(&["Special_Mods", "Special_ModsKeys"])))
        .or_null("endgame_mods", ids(column(&["Endgame_Mods", "Endgame_ModsKeys"])))
        .or_null("granted_effects", ids(column(&["GrantedEffects", "GrantedEffectsKeys"])))
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

    // PoE 1 applies its resistance penalty by act, which no table holds.
    let area_level = ctx.optional_table("ResistancePenaltyPerAreaLevel").map(|by_area| {
        J::Obj(
            by_area
                .rows()
                .map(|row| {
                    let entry = Obj::new().set("resistance_penalty", int(row.int("Penalty"))).build();
                    (row.int("AreaLevel").to_string(), entry)
                })
                .collect(),
        )
    });

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

    let minion_gem_level = ctx.optional_table("MinionGemLevelScaling").and_then(|table| {
        let gem = table.pick(&["GemLevel"])?;
        let minion = table.pick(&["MinionLevel"])?;
        Some(J::Obj(
            table
                .rows()
                .map(|row| (row.int(gem).to_string(), Obj::new().set("minion_level", int(row.int(minion))).build()))
                .collect(),
        ))
    });

    let gold_respec_prices = ctx.optional_table("GoldRespecPrices").and_then(|table| {
        let level = table.pick(&["Level"])?;
        let cost = table.pick(&["Cost"])?;
        Some(J::Obj(
            table.rows().map(|row| (row.int(level).to_string(), Obj::new().set("cost", int(row.int(cost))).build())).collect(),
        ))
    });

    let root = Obj::new()
        .set("player_level", J::Obj(player_level))
        .or_null("area_level", area_level)
        .set("map_level", J::Obj(map_level))
        .or_null("minion_gem_level", minion_gem_level)
        .or_null("gold_respec_prices", gold_respec_prices)
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
        let mut tiers = Obj::new();
        for (key, column) in ELEMENTS {
            tiers = tiers.or_null(key, resistance_tiers(row, column));
        }
        out = out.set("tiers", tiers.build());
    }
    out.build()
}

/// PoE 2 stores four resistance lists per element, each paired with the
/// levels its values start at. The schema names only `<Element>1` to
/// `<Element>5`: the first three lists are followed by an unnamed level list,
/// and `<Element>5` is the level list of `<Element>4`.
fn resistance_tiers(row: Row<'_>, element: &str) -> Option<J> {
    let first = row.table.col(&format!("{}1", element))?;
    let named = [(2, first + 2), (3, first + 4), (4, first + 6), (5, first + 7)];
    if !named.iter().all(|(n, at)| row.table.col(&format!("{}{}", element, n)) == Some(*at)) {
        return None;
    }
    let columns = [(first, first + 1), (first + 2, first + 3), (first + 4, first + 5), (first + 6, first + 7)];
    let tiers = columns.iter().map(|&(values, levels)| {
        Obj::new()
            .set("values", J::Arr(row.list_int_at(values).into_iter().map(int).collect()))
            .set("levels", J::Arr(row.list_int_at(levels).into_iter().map(int).collect()))
            .build()
    });
    Some(json::arr(tiers))
}
