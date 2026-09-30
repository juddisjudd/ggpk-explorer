//! Crafting and special-jewel data: `Essence.lua`, `LiquidEmotions.lua`,
//! `ModRunes.lua`, the `Enchantment*.lua` files, `Crucible.lua`,
//! `TattooPassives.lua`, `ClusterJewels.lua`,
//! `TimelessJewelData/LegionPassives.lua` and `Pantheons.lua`, ported from
//! PoB's `essence.lua`, `soulcores.lua`, `enchant.lua`, `crucible.lua`,
//! `tattooPassives.lua`, `cluster.lua`, `legionPassives.lua` and
//! `pantheons.lua`.

use crate::dat::relational::{LoadedTable, Row};
use crate::data_export::Ctx;
use crate::pob_export::lua::tostring;
use crate::pob_export::modules::mods::{describe_mod, murmur_hash2, ModReader, PobMod};
use crate::pob_export::statdesc::{lua_literal, Described, Descriptors, StatValue, Stats, Val};
use crate::pob_export::{describer, game, write, Lua, Table};
use crate::settings::Game;
use std::collections::{HashMap, HashSet};

/// The number Lua reads back from `tostring(x)` written into a file.
fn lua_num(x: f64) -> f64 {
    tostring(x).parse().unwrap_or(x)
}

/// `{ "a", "b" }` with each string written raw between quotes.
fn literals(items: &[String]) -> Table {
    Table::list(items.iter().map(|s| Lua::from(lua_literal(s))))
}

/// The first of `names` the table has, for names built at run time.
fn first_col(table: &LoadedTable, names: &[String]) -> Option<String> {
    names.iter().find(|n| table.has_col(n)).cloned()
}

/// `path:gsub("dds$", "png")`.
fn dds_to_png(path: &str) -> String {
    match path.strip_suffix("dds") {
        Some(stem) => format!("{}png", stem),
        None => path.to_string(),
    }
}

fn main_descriptions(game: Game) -> &'static str {
    match game {
        Game::Poe2 => "stat_descriptions.csd",
        Game::Poe1 => "stat_descriptions.txt",
    }
}

fn passive_descriptions(game: Game) -> &'static str {
    match game {
        Game::Poe2 => "passive_skill_stat_descriptions.csd",
        Game::Poe1 => "passive_skill_stat_descriptions.txt",
    }
}

/// Lua 5.1's `table.sort`, the quicksort LuaJIT keeps. It is not stable, so
/// PoB's output depends on where it leaves equal elements.
fn lua_sort<T: Clone>(items: &mut [T], lt: &dyn Fn(&T, &T) -> bool) {
    if items.len() > 1 {
        aux_sort(items, 1, items.len() as isize, lt);
    }
}

/// `auxsort` over the 1-based range `l..=u`.
fn aux_sort<T: Clone>(a: &mut [T], mut l: isize, mut u: isize, lt: &dyn Fn(&T, &T) -> bool) {
    let at = |i: isize| (i - 1) as usize;
    while l < u {
        if lt(&a[at(u)], &a[at(l)]) {
            a.swap(at(l), at(u));
        }
        if u - l == 1 {
            break;
        }
        let mut i = (l + u) / 2;
        if lt(&a[at(i)], &a[at(l)]) {
            a.swap(at(i), at(l));
        } else if lt(&a[at(u)], &a[at(i)]) {
            a.swap(at(i), at(u));
        }
        if u - l == 2 {
            break;
        }
        let pivot = a[at(i)].clone();
        a.swap(at(i), at(u - 1));
        i = l;
        let mut j = u - 1;
        loop {
            i += 1;
            while i < u && lt(&a[at(i)], &pivot) {
                i += 1;
            }
            j -= 1;
            while j > l && lt(&pivot, &a[at(j)]) {
                j -= 1;
            }
            if j < i {
                break;
            }
            a.swap(at(i), at(j));
        }
        a.swap(at(u - 1), at(i));
        if i - l < u - i {
            let (lo, hi) = (l, i - 1);
            l = i + 1;
            aux_sort(a, lo, hi, lt);
        } else {
            let (lo, hi) = (i + 1, u);
            u = i - 1;
            aux_sort(a, lo, hi, lt);
        }
    }
}

/// LuaJIT's `math.random` from a fresh state: the Tausworthe generator it
/// seeds with 0 when the math library opens. PoB's legion exporter places
/// every non-keystone node with it, so the draws have to be replayed in order.
struct LuaRandom {
    state: [u64; 4],
}

impl LuaRandom {
    fn new() -> Self {
        let mut state = [0u64; 4];
        let mut r: u32 = 0x1109_0601;
        let mut d = 0.0f64;
        for slot in state.iter_mut() {
            let m = 1u64 << (r & 255);
            r >>= 8;
            d = d * std::f64::consts::PI + std::f64::consts::E;
            let mut u = d.to_bits();
            if u < m {
                u += m;
            }
            *slot = u;
        }
        let mut rng = Self { state };
        for _ in 0..10 {
            rng.step();
        }
        rng
    }

    fn step(&mut self) -> u64 {
        const SHAPE: [(u32, u32, u32); 4] = [(63, 31, 18), (58, 19, 28), (55, 24, 7), (47, 21, 8)];
        let mut r = 0;
        for (z, (k, q, s)) in self.state.iter_mut().zip(SHAPE) {
            let v = *z;
            *z = (((v << q) ^ v) >> (k - s)) ^ ((v & (u64::MAX << (64 - k))) << s);
            r ^= *z;
        }
        r
    }

    /// `math.random()`: a float in `[0, 1)`.
    fn random(&mut self) -> f64 {
        let bits = (self.step() & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000;
        f64::from_bits(bits) - 1.0
    }
}

/// `describeStats`, also saying which of the passed stats a chosen
/// description touched: those are the ones PoB leaves a `fmt` on.
fn describe_marking(descriptors: &Descriptors, stats: &mut Stats) -> (Described, Vec<bool>) {
    let nonzero = |v: &StatValue| {
        matches!(v.min, Val::Num(n) if n != 0.0) || matches!(v.max, Val::Num(n) if n != 0.0)
    };
    let chosen: Vec<usize> = stats
        .0
        .iter()
        .filter(|(id, v)| id != "Type" && nonzero(v))
        .filter_map(|(id, _)| descriptors.by_stat.get(id).copied())
        .filter(|&d| descriptors.descriptors[d].stats.is_some())
        .collect();
    let touched = stats
        .0
        .iter()
        .map(|(id, _)| {
            chosen.iter().any(|&d| descriptors.descriptors[d].stats.as_ref().is_some_and(|s| s.contains(id)))
        })
        .collect();
    (descriptors.describe_stats(stats), touched)
}

/// A stat entry as the passive exporters keep it after `describeStats` has
/// run over it: the rewritten values, and `fmt` when a description formatted
/// it. Callers add the fields their script stores beside them.
fn stat_entry(stats: &Stats, id: &str, touched: bool) -> Table {
    let mut entry = Table::new();
    if let Some(v) = stats.get(id) {
        if let Val::Num(n) = v.min {
            entry.set("min", lua_num(n));
        }
        if let Val::Num(n) = v.max {
            entry.set("max", lua_num(n));
        }
        if touched {
            entry.set("fmt", v.fmt());
        }
    }
    entry
}

/// `Essence.lua` for either game, and PoE 2's `LiquidEmotions.lua`.
pub fn essence(ctx: &Ctx) -> Result<(), String> {
    match game(ctx) {
        Game::Poe2 => {
            poe2_essences(ctx)?;
            liquid_emotions(ctx)
        }
        Game::Poe1 => poe1_essences(ctx),
    }
}

/// `Essence.lua` for PoE 2: every essence whose base id has a name after
/// `Essence`, with the mod it adds for each item class.
fn poe2_essences(ctx: &Ctx) -> Result<(), String> {
    let essences = ctx.table("Essences")?;
    let essence_mods = ctx.table("EssenceMods")?;
    let base_col = essences.require(&["BaseItemType", "BaseItemTypesKey"])?;
    let drop_col = essences.require(&["DropLevel", "TankModValues"])?;
    let owner_col = essence_mods.require(&["Essence"])?;
    let category_col = essence_mods.require(&["TargetItemCategory"])?;
    let mod_cols: Vec<&str> =
        [&["Mod1", "Mod"][..], &["Mod2", "DisplayMod"][..]].iter().filter_map(|names| essence_mods.pick(names)).collect();
    let mut by_essence: HashMap<usize, Vec<Row>> = HashMap::new();
    for row in essence_mods.rows() {
        if let Some(owner) = row.key(owner_col) {
            by_essence.entry(owner).or_default().push(row);
        }
    }
    let mut out = Table::new();
    for row in essences.rows() {
        let Some(base) = ctx.rr.deref(row, base_col) else { continue };
        let id = base.id();
        let Some(kind) = id.find("Essence").map(|at| &id[at + "Essence".len()..]).filter(|k| !k.is_empty()) else {
            continue;
        };
        let mut mods = Table::new();
        for m in by_essence.get(&row.index).into_iter().flatten() {
            let Some(target) = mod_cols.iter().find_map(|c| ctx.rr.deref(*m, c)) else { continue };
            let Some(category) = ctx.rr.deref(*m, category_col) else { continue };
            let classes_col = category.table.pick(&["ItemClasses", "ItemClassesKeys"]).unwrap_or("ItemClasses");
            for class in ctx.rr.deref_list_ids(category.row(), classes_col) {
                mods.set(lua_literal(&class), lua_literal(&target.id()));
            }
        }
        let mut entry = Table::new()
            .with("name", lua_literal(base.row().str("Name")))
            .with("type", lua_literal(kind))
            .with("mods", mods);
        entry.set_opt("tierLevel", row.list_int(drop_col).first().copied());
        out.set(lua_literal(&id), entry);
    }
    write(ctx, "Essence", out)
}

/// `LiquidEmotions.lua`: the prefix and suffix each distilled emotion adds to
/// each jewel.
fn liquid_emotions(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("LiquidEmotionOutcomes")?;
    let base_col = table.require(&["BaseItemType", "BaseItemTypesKey"])?;
    let mut out = Table::new();
    for row in table.rows() {
        let Some(base) = ctx.rr.deref(row, base_col) else { continue };
        let mut mods = Table::new();
        for jewel in ["Ruby", "Sapphire", "Emerald", "Diamond"] {
            let mut slots = Table::new();
            for kind in ["Prefix", "Suffix"] {
                if let Some(m) = ctx.rr.deref(row, &format!("{}{}", jewel, kind)) {
                    slots.set(kind, lua_literal(&m.id()));
                }
            }
            mods.set(jewel, slots);
        }
        let entry = Table::new()
            .with("name", lua_literal(base.row().str("Name")))
            .with("radiusJewel", row.int("RadiusJewel") == 1)
            .with("tierLevel", base.row().int("DropLevel"))
            .with("mods", mods);
        out.set(lua_literal(&base.id()), entry);
    }
    write(ctx, "LiquidEmotions", out)
}

/// The item types PoE 1's `essence.lua` lists, each with the column it reads
/// (PoB's spec name, then dat-schema's).
const POE1_ESSENCE_COLUMNS: [(&str, [&str; 2]); 22] = [
    ("Amulet", ["AmuletMod", "Amulet_ModsKey"]),
    ("Ring", ["RingMod", "Ring_ModsKey"]),
    ("Belt", ["BeltMod", "Belt_ModsKey"]),
    ("Quiver", ["QuiverMod", "Display_Quiver_ModsKey"]),
    ("Helmet", ["HelmetMod", "Helmet_ModsKey"]),
    ("Body Armour", ["BodyArmourMod", "BodyArmour_ModsKey"]),
    ("Boots", ["BootsMod", "Boots_ModsKey"]),
    ("Gloves", ["GlovesMod", "Gloves_ModsKey"]),
    ("Shield", ["ShieldMod", "Shield_ModsKey"]),
    ("Bow", ["BowMod", "Bow_ModsKey"]),
    ("Claw", ["ClawMod", "Claw_ModsKey"]),
    ("Dagger", ["DaggerMod", "Dagger_ModsKey"]),
    ("Staff", ["StaffMod", "Staff_ModsKey"]),
    ("Wand", ["WandMod", "Wand_ModsKey"]),
    ("One Handed Axe", ["OneHandAxeMod", "OneHandAxe_ModsKey"]),
    ("One Handed Mace", ["OneHandMaceMod", "OneHandMace_ModsKey"]),
    ("One Handed Sword", ["OneHandSwordMod", "OneHandSword_ModsKey"]),
    ("Sceptre", ["SceptreMod", "Sceptre_ModsKey"]),
    ("Thrusting One Handed Sword", ["ThrustingOneHandSwordMod", "OneHandThrustingSword_ModsKey"]),
    ("Two Handed Axe", ["TwoHandAxeMod", "TwoHandAxe_ModsKey"]),
    ("Two Handed Mace", ["TwoHandMaceMod", "TwoHandMace_ModsKey"]),
    ("Two Handed Sword", ["TwoHandSwordMod", "TwoHandSword_ModsKey"]),
];

/// `Essence.lua` for PoE 1: every essence with a tier, and its mod for each
/// item type.
fn poe1_essences(ctx: &Ctx) -> Result<(), String> {
    let essences = ctx.table("Essences")?;
    let base_col = essences.require(&["BaseItemType", "BaseItemTypesKey"])?;
    let tier_col = essences.require(&["Tier", "Level"])?;
    let type_col = essences.require(&["Type", "EssenceTypeKey"])?;
    let columns = POE1_ESSENCE_COLUMNS
        .iter()
        .map(|(kind, names)| essences.require(names).map(|col| (*kind, col)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Table::new();
    for row in essences.rows() {
        let tier = row.int(tier_col);
        if tier <= 0 {
            continue;
        }
        let Some(base) = ctx.rr.deref(row, base_col) else { continue };
        let mut mods = Table::new();
        for (kind, col) in &columns {
            if let Some(id) = ctx.rr.deref_id(row, col) {
                mods.set(*kind, lua_literal(&id));
            }
        }
        let mut entry = Table::new().with("name", lua_literal(base.row().str("Name"))).with("tier", tier).with("mods", mods);
        entry.set_opt("type", row.key(type_col));
        out.set(lua_literal(&base.id()), entry);
    }
    write(ctx, "Essence", out)
}

/// `soulcores.lua`'s names for the stat categories that cover several slots.
/// Any other category comes out under its own id in lower case.
const RUNE_SLOTS: [(&str, &[&str]); 11] = [
    ("Martial Weapon", &["weapon"]),
    ("Caster Weapon", &["caster"]),
    ("Martial Or Caster Weapon", &["weapon", "caster"]),
    ("Armour", &["armour"]),
    ("Wand or Staff", &["wand", "staff"]),
    ("Maces or Talisman", &["one hand mace", "two hand mace", "talisman"]),
    ("One Hand Mace or Quarterstaff", &["one hand mace", "quarterstaff"]),
    ("Shield or Buckler", &["shield", "buckler"]),
    ("All", &["weapon", "armour", "caster"]),
    ("Quarterstaff or Spear", &["quarterstaff", "spear"]),
    ("Crossbow Bow or Spear", &["crossbow", "bow", "spear"]),
];

/// One slot of a rune, gathered over every stat row that covers it.
struct RuneSlot {
    slot: String,
    local: bool,
    label: Vec<String>,
    orders: Vec<f64>,
    bonded_label: Vec<String>,
    bonded_orders: Vec<f64>,
    trade: Vec<(u32, Vec<String>)>,
}

/// `ModRunes.lua`: every rune, soul core, idol and other socketable, by
/// display name, with what it grants in each slot. `Bases/soulcore.txt`
/// picks the bases by id prefix; this takes every base item the `SoulCores`
/// table has, which is the same set and keeps new kinds.
pub fn runes(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &["stat_descriptions.csd"]);
    let cores = ctx.table("SoulCores")?;
    let core_stats = ctx.table("SoulCoreStats")?;
    let bases = ctx.table("BaseItemTypes")?;
    let base_col = cores.require(&["BaseItemTypes", "BaseItemType"])?;
    let owner_col = core_stats.require(&["Id", "SoulCore"])?;
    let category_col = core_stats.require(&["Category", "StatCategory"])?;
    let values_col = core_stats.require(&["StatValue", "StatsValues"])?;
    let bonded_values_col = core_stats.require(&["BondedValues", "BondedStatsValues"])?;
    let chakra_col = cores.require(&["CanSocketInChakraSlots", "CanSocketInMartialArtistSlots"])?;
    let level_col = cores.require(&["LevelReq", "RequiredLevel"])?;

    let mut core_of_base: HashMap<usize, usize> = HashMap::new();
    for core in cores.rows() {
        if let Some(base) = core.key(base_col) {
            core_of_base.entry(base).or_insert(core.index);
        }
    }
    let mut stats_of_core: HashMap<usize, Vec<Row>> = HashMap::new();
    for row in core_stats.rows() {
        if let Some(owner) = row.key(owner_col) {
            stats_of_core.entry(owner).or_default().push(row);
        }
    }

    let mut out = Table::new();
    for base in bases.rows() {
        let Some(&core_index) = core_of_base.get(&base.index) else { continue };
        let core = cores.row(core_index).expect("indexed above");
        let name = base.str("Name");
        if name.contains("DNT") {
            continue;
        }
        let name = name.replace('\u{f6}', "o");
        let name = name.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'));

        let mut slots: Vec<RuneSlot> = Vec::new();
        for stat_row in stats_of_core.get(&core_index).into_iter().flatten() {
            let stat_refs = ctx.rr.deref_list(*stat_row, "Stats");
            let values = stat_row.list_int(values_col);
            let value = |i: usize| values.get(i).copied().unwrap_or(0) as f64;
            let bonded_ids = ctx.rr.deref_list_ids(*stat_row, "BondedStats");
            let bonded_values = stat_row.list_int(bonded_values_col);
            if stat_refs.is_empty() && bonded_ids.is_empty() {
                continue;
            }
            let category = ctx.rr.deref_id(*stat_row, category_col).unwrap_or_default();
            let slot_names: Vec<String> = match RUNE_SLOTS.iter().find(|(c, _)| *c == category) {
                Some((_, names)) => names.iter().map(|s| s.to_string()).collect(),
                None => vec![category.to_lowercase()],
            };
            for slot_name in slot_names {
                let mut stats = Stats::new();
                for (i, stat) in stat_refs.iter().enumerate() {
                    stats.set(&stat.id(), value(i), value(i));
                }
                let mut bonded = Stats::new();
                for (i, id) in bonded_ids.iter().enumerate() {
                    let v = bonded_values.get(i).copied().unwrap_or(0) as f64;
                    bonded.set(id, v, v);
                }
                let described = descriptors.describe_stats(&mut stats);
                let bonded_described = descriptors.describe_stats(&mut bonded);
                if described.orders.is_empty() && bonded_described.orders.is_empty() {
                    continue;
                }
                let mut trade = Vec::new();
                let mut local = true;
                let mut i = 0;
                while i < stat_refs.len() {
                    let stat = stat_refs[i].row();
                    if !(stat.bool("IsLocal") || stat.bool("IsWeaponLocal")) {
                        local = false;
                    }
                    let mut current = Stats::new();
                    current.set(stat.id(), value(i), value(i));
                    let mut bytes = (stat.int("HASH32") as u32).to_le_bytes().to_vec();
                    if stat.id().contains("minimum") {
                        if let Some(next) = stat_refs.get(i + 1).map(|r| r.row()) {
                            if next.id().contains("maximum") {
                                i += 1;
                                bytes.extend_from_slice(&(next.int("HASH32") as u32).to_le_bytes());
                                current.set(next.id(), value(i), value(i));
                            }
                        }
                    }
                    let lines = descriptors.describe_stats(&mut current).lines;
                    trade.push((murmur_hash2(&bytes, 0x0231_2233), lines));
                    i += 1;
                }
                match slots.iter_mut().find(|s| s.slot == slot_name) {
                    Some(slot) => {
                        slot.local = slot.local && local;
                        slot.label.extend(described.lines);
                        slot.orders.extend(described.orders);
                        slot.bonded_label.extend(bonded_described.lines);
                        slot.bonded_orders.extend(bonded_described.orders);
                        slot.trade.extend(trade);
                    }
                    None => slots.push(RuneSlot {
                        slot: slot_name,
                        local,
                        label: described.lines,
                        orders: described.orders,
                        bonded_label: bonded_described.lines,
                        bonded_orders: bonded_described.orders,
                        trade,
                    }),
                }
            }
        }

        let limit = ctx.rr.deref(core, "Limit");
        let kind = ctx.rr.deref_id(core, "Type").unwrap_or_default();
        let mut entry = Table::new();
        for slot in slots {
            let mut t = Table::new();
            t.set("type", lua_literal(&kind));
            if let Some(limit) = &limit {
                t.set("limit", limit.row().int("Limit"));
                let limit_id = limit.id();
                if limit_id != "GenericLimit1" {
                    t.set("limitId", lua_literal(&limit_id));
                }
            }
            t.set("localMod", slot.local);
            if !slot.label.is_empty() {
                for line in &slot.label {
                    t.push(lua_literal(line));
                }
                t.set("statOrder", Table::list(slot.orders.iter().map(|o| lua_num(*o))));
            }
            let mut trade = Table::new();
            for (hash, lines) in slot.trade {
                let lines = if lines.is_empty() { vec![String::new()] } else { lines };
                trade.set(hash as i64, literals(&lines));
            }
            t.set("tradeHashes", trade);
            if !slot.bonded_label.is_empty() {
                let mut bonded = literals(&slot.bonded_label);
                bonded.set("statOrder", Table::list(slot.bonded_orders.iter().map(|o| lua_num(*o))));
                t.set("bonded", bonded);
            }
            for (field, col) in [
                ("isSocketBound", "IsSocketBound"),
                ("canSocketInChakraSlots", chakra_col),
                ("canSocketInUniqueItems", "CanSocketInUniqueItems"),
                ("canSocketInJewellery", "CanSocketInJewellery"),
                ("canSocketInCorruptedSanctified", "CanSocketInCorruptedSanctified"),
            ] {
                if core.bool(col) {
                    t.set(field, true);
                }
            }
            t.set("levelReq", core.int(level_col));
            entry.set(lua_literal(&slot.slot), t);
        }
        out.set(lua_literal(name), entry);
    }
    write(ctx, "ModRunes", out)
}

/// The lab difficulty `enchant.lua` names for each enchantment level.
const LAB: [(i64, &str); 5] = [(32, "NORMAL"), (53, "CRUEL"), (66, "MERCILESS"), (75, "ENDGAME"), (83, "DEDICATION")];

/// The files `doLabEnchantment` writes, with the mod family each takes.
const LAB_FILES: [(&str, &str); 3] = [
    ("EnchantmentBoots", "ConditionalBuffEnchantment"),
    ("EnchantmentGloves", "TriggerEnchantment"),
    ("EnchantmentBelt", "BuffEnchantment"),
];

/// The files `doOtherEnchantment` writes: generation type, mod family, and
/// the source PoB files them under.
const OTHER_FILES: [(&str, &[(i64, &str, &str)]); 3] = [
    ("EnchantmentFlask", &[(21, "FlaskEnchantment", "ENKINDLING"), (22, "FlaskEnchantment", "INSTILLING")]),
    ("EnchantmentBody", &[(3, "AlternateArmourQuality", "HARVEST"), (3, "EnchantmentHeistArmour", "HEIST")]),
    ("EnchantmentWeapon", &[(3, "AlternateWeaponQuality", "HARVEST"), (3, "EnchantmentHeistWeapon", "HEIST")]),
];

/// `enchant.lua`'s fallback from a fragment of a helmet enchantment's mod id
/// (a Lua pattern) to the skill it names, for enchantments whose stats no
/// active skill lists. A name the game gives is kept unless a pattern
/// matches more of the id than the name is long.
const HELMET_SKILLS: &[(&str, &str)] = &[
    ("Summone?d?RagingSpirit", "Summon Raging Spirit"),
    ("SpiritOffering", "Spirit Offering"),
    ("Discharge", "Discharge"),
    ("AncestorTotem[^S][^l]", "Ancestral Protector"),
    ("AncestorTotemSlamMelee", "Ancestral Warchief"),
    ("AnimateGuardian", "Animate Guardian"),
    ("AnimateWeapon", "Animate Weapon"),
    ("BlinkArrow", "Blink Arrow"),
    ("ConversionTrap", "Conversion Trap"),
    ("MirrorArrow", "Mirror Arrow"),
    ("Spectre", "Raise Spectre"),
    ("Zombie", "Raise Zombie"),
    ("ChaosGolem", "Summon Chaos Golem"),
    ("FlameGolem", "Summon Flame Golem"),
    ("IceGolem", "Summon Ice Golem"),
    ("LightningGolem", "Summon Lightning Golem"),
    ("StoneGolem", "Summon Stone Golem"),
    ("Skeleton", "Summon Skeletons"),
    ("Bladefall", "Bladefall"),
    ("BlastRain", "Blast Rain"),
    ("ChargedAttack", "Blade Flurry"),
    ("Desecrate", "Desecrate"),
    ("DetonateDead", "Detonate Dead"),
    ("DevouringTotem", "Devouring Totem"),
    ("DominatingBlow", "Dominating Blow"),
    ("FireBeam", "Scorching Ray"),
    ("Firestorm", "Firestorm"),
    ("FreezeMine", "Freeze Mine"),
    ("EnchantmentFrenzy", "Frenzy"),
    ("GroundSlam", "Ground Slam"),
    ("HeavyStrike", "Heavy Strike"),
    ("IceSpear", "Ice Spear"),
    ("ImmortalCall", "Immortal Call"),
    ("Incinerate", "Incinerate"),
    ("KineticBlast", "Kinetic Blast"),
    ("LightningArrow", "Lightning Arrow"),
    ("ChargedDash", "Charged Dash"),
    ("PhaseRun", "Phase Run"),
    ("Puncture", "Puncture"),
    ("RejuvinationTotem", "Rejuvenation Totem"),
    ("ShockNova", "Shock Nova"),
    ("SpectralThrow", "Spectral Throw"),
    ("TectonicSlam", "Tectonic Slam"),
    ("VolatileDead", "Volatile Dead"),
    ("BoneLance", "Unearth"),
    ("CorpseEruption", "Cremation"),
    ("PowerSiphon", "Power Siphon"),
    ("Smite", "Smite"),
    ("ConsecratedPath", "Consecrated Path"),
    ("ScourgeArrow", "Scourge Arrow"),
    ("HolyRelic", "Summon Holy Relic"),
    ("HeraldOfAgony", "Herald of Agony"),
    ("HeraldOfPurity", "Herald of Purity"),
    ("Bane", "Bane"),
    ("DivineIre", "Divine Ire"),
    ("PurifyingFlame", "Purifying Flame"),
    ("Soulrend", "Soulrend"),
    ("StormBurst", "Storm Burst"),
    ("CarrionGolem", "Summon Carrion Golem"),
    ("Steelskin", "Steelskin"),
    ("[^d]Dash", "Dash"),
    ("Bladestorm", "Bladestorm"),
    ("Perforate", "Perforate"),
    ("Frostblink", "Frostblink"),
    ("ChainHook", "Chain Hook"),
    ("Berserk", "Berserk"),
    ("WitheringStep", "Withering Step"),
    ("SnappingAdder", "Venom Gyre"),
    ("PlagueBearer", "Plague Bearer"),
    ("SummonSkitterbots", "Summon Skitterbots"),
    ("ArtilleryBallista", "Artillery Ballista"),
    ("ArcaneCloak", "Arcane Cloak"),
    ("KineticBolt", "Kinetic Bolt"),
    ("BladeBlast", "Blade Blast"),
    ("RuneBlast", "Stormbind"),
    ("Spellslinger", "Spellslinger"),
    ("AncestralCry", "Ancestral Cry"),
    ("EnduringCry", "Enduring Cry"),
    ("SeismicCry", "Seismic Cry"),
    ("Sunder", "Sunder"),
    ("Earthshatter", "Earthshatter"),
    ("ArcanistBrand", "Arcanist Brand"),
    ("BlazingSalvo", "Blazing Salvo"),
    ("Anger", "Anger"),
    ("Clarity", "Clarity"),
    ("Determination", "Determination"),
    ("Discipline", "Discipline"),
    ("Grace", "Grace"),
    ("Haste", "Haste"),
    ("Hatred", "Hatred"),
    ("Malevolence", "Malevolence"),
    ("Precision", "Precision"),
    ("Pride", "Pride"),
    ("Vitality", "Vitality"),
    ("Wrath", "Wrath"),
    ("Zealotry", "Zealotry"),
    ("PurityOfElements", "Purity of Elements"),
    ("PurityOfFire", "Purity of Fire"),
    ("PurityOfIce", "Purity of Ice"),
    ("PurityOfLightning", "Purity of Lightning"),
    ("MortarBarrageMine", "Pyroclast Mine"),
    ("ColdProjectileMine", "Icicle Mine"),
    ("LightningExplosionMine", "Stormblast Mine"),
    ("FleshAndStone", "Flesh and Stone"),
    ("DreadBanner", "Dread Banner"),
    ("WarBanner", "War Banner"),
    ("FrostShield", "Frost Shield"),
    ("VoidSphere", "Void Sphere"),
    ("CracklingLance", "Crackling Lance"),
    ("SigilOfPower", "Sigil of Power"),
    ("Hexblast", "Hexblast"),
    ("FlameWall", "Flame Wall"),
    ("WaterSphere", "Hydrosphere"),
    ("CorruptingFever", "Corrupting Fever"),
    ("Bloodreap", "Reap"),
    ("BladeTrap", "Blade Trap"),
    ("EyeOfWinter", "Eye of Winter"),
    ("StormRain", "Storm Rain"),
    ("RageVortex", "Rage Vortex"),
    ("ShieldCrush", "Shield Crush"),
    ("SummonedReaper", "Summon Reaper"),
    ("Boneshatter", "Boneshatter"),
    ("SpectralHelix", "Spectral Helix"),
    ("DefianceBanner", "Defiance Banner"),
    ("EnergyBlade", "Energy Blade"),
    ("TornadoShot", "Tornado Shot"),
    ("Tornado", "Tornado"),
    ("VolcanicFissure", "Volcanic Fissure"),
    ("Table Charge", "Shield Charge"),
    ("Flame Dash", "Flame Dash"),
];

/// A mod's six stat columns, by PoB's spec name and dat-schema's.
const MOD_STAT_KEYS: [[&str; 2]; 6] = [
    ["Stat1", "StatsKey1"],
    ["Stat2", "StatsKey2"],
    ["Stat3", "StatsKey3"],
    ["Stat4", "StatsKey4"],
    ["Stat5", "StatsKey5"],
    ["Stat6", "StatsKey6"],
];

fn first_family<'a>(m: &'a PobMod) -> Option<&'a str> {
    m.families.first().map(String::as_str)
}

fn lab_difficulty(level: i64) -> Option<&'static str> {
    LAB.iter().find(|(l, _)| *l == level).map(|(_, name)| *name)
}

/// `Enchantment*.lua`: lab enchantments by difficulty (helmet ones by
/// skill), and flask, body armour and weapon enchantments by source.
pub fn enchantments(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &["stat_descriptions.txt"]);
    let mods_table = ctx.table("Mods")?;
    let reader = ModReader::new(&mods_table);
    let mods: Vec<PobMod> = mods_table.rows().map(|row| reader.read(ctx, row)).collect();
    let line = |m: &PobMod| describe_mod(Game::Poe1, &descriptors, m).lines;
    let lab_mod = |m: &PobMod, family: &str| {
        m.generation_type == 10 && first_family(m) == Some(family) && m.spawn_weights.first().is_some_and(|w| *w > 0)
    };

    for (file, family) in LAB_FILES {
        let mut out = Table::new();
        for m in mods.iter().filter(|m| lab_mod(m, family)) {
            let Some(diff) = lab_difficulty(m.level) else { continue };
            out.table_mut(diff).push(lua_literal(&line(m).join("/")));
        }
        write(ctx, file, out)?;
    }

    for (file, groups) in OTHER_FILES {
        let mut out = Table::new();
        for m in &mods {
            for (generation, family, source) in groups {
                if m.generation_type == *generation && first_family(m) == Some(family) {
                    out.table_mut(*source).push(lua_literal(&line(m).join("/")));
                }
            }
        }
        write(ctx, file, out)?;
    }

    let active = ctx.table("ActiveSkills")?;
    let input_col = active.require(&["SkillSpecificStat", "Input_StatKeys", "Input_Stats"])?;
    let secondary_col = active.require(&["SecondarySkillSpecificStat", "SecondarySkillSpecificStats"])?;
    let name_col = active.require(&["DisplayName", "DisplayedName"])?;
    let skills_by = |col: &str| {
        let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
        for row in active.rows() {
            let mut seen = HashSet::new();
            for stat in row.list_keys(col) {
                if seen.insert(stat) {
                    map.entry(stat).or_default().push(row.index);
                }
            }
        }
        map
    };
    let by_input = skills_by(input_col);
    let by_secondary = skills_by(secondary_col);
    let patterns: Vec<(regex::Regex, &str)> = HELMET_SKILLS
        .iter()
        .map(|(p, name)| (regex::Regex::new(p).expect("valid pattern"), *name))
        .collect();
    let stat_cols: Vec<&str> = MOD_STAT_KEYS.iter().filter_map(|names| mods_table.pick(names)).collect();

    let mut out = Table::new();
    for m in mods.iter().filter(|m| lab_mod(m, "SkillEnchantment")) {
        let mut skill: Option<String> = None;
        for col in &stat_cols {
            let Some(stat) = m.row.key(col) else { continue };
            for map in [&by_input, &by_secondary] {
                let found = map.get(&stat).into_iter().flatten().filter_map(|&i| active.row(i)).find(|a| {
                    !a.id().contains("vaal") && !a.str(name_col).is_empty()
                });
                if let Some(a) = found {
                    skill = Some(a.string(name_col));
                }
            }
        }
        let mut skill = skill.unwrap_or_default();
        for (pattern, name) in &patterns {
            if let Some(found) = pattern.find(&m.id) {
                if skill.len() < found.len() - 1 {
                    skill = name.to_string();
                }
            }
        }
        if let Some((_, name)) = HELMET_SKILLS.iter().find(|(p, _)| *p == skill) {
            skill = name.to_string();
        }
        let lines = line(m);
        if lines.is_empty() {
            continue;
        }
        let Some(diff) = lab_difficulty(m.level) else { continue };
        out.table_mut(lua_literal(&skill)).table_mut(diff).push(lua_literal(&lines.join("/")));
    }
    write(ctx, "EnchantmentHelmet", out)
}

/// `Crucible.lua`: every crucible weapon passive's mod, with its tier, node
/// type and where it can spawn.
pub fn crucible(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &["stat_descriptions.txt"]);
    let table = ctx.table("WeaponPassiveSkills")?;
    let mods = ctx.table("Mods")?;
    let reader = ModReader::new(&mods);
    let tier_col = table.require(&["ModTier", "Tier"])?;
    let mut out = Table::new();
    for row in table.rows() {
        let Some(target) = ctx.rr.deref(row, "Mod") else { continue };
        let m = reader.read(ctx, target.row());
        if m.id.ends_with("HardMode") {
            continue;
        }
        let described = describe_mod(Game::Poe1, &descriptors, &m);
        if described.orders.is_empty() {
            continue;
        }
        let mut entry = Table::new();
        if m.generation_type == 31 {
            entry.set("type", "Spawn");
        } else if m.generation_type == 32 {
            entry.set("type", "MergeOnly");
        }
        entry.set("tier", row.int(tier_col));
        for line in &described.lines {
            entry.push(lua_literal(line));
        }
        entry.set("statOrder", Table::list(described.orders.iter().map(|o| lua_num(*o))));
        entry.set("level", m.level);
        entry.set("group", lua_literal(&m.group));
        entry.set_opt("nodeType", ctx.rr.deref_id(row, "Type").map(|t| lua_literal(&t)));
        entry.set("nodeLocation", Table::list(row.list_int("NodeSpawnLocation")));
        entry.set("weightKey", literals(&m.spawn_tags));
        entry.set("weightVal", Table::list(m.spawn_weights.iter().copied()));
        if !m.generation_tags.is_empty() {
            entry.set("weightMultiplierKey", literals(&m.generation_tags));
            entry.set("weightMultiplierVal", Table::list(m.generation_weights.iter().copied()));
            if !m.tags.is_empty() {
                entry.set("tags", literals(&m.tags));
            }
        }
        entry.set("modTags", literals(&m.implicit_tags));
        out.set(lua_literal(&m.id), entry);
    }
    write(ctx, "Crucible", out)
}

/// `{ [1e9] = { x = -6500, y = -6500, oo = {}, n = … } }`: the one group the
/// tattoo and legion files put every node in.
fn passive_group(nodes: Table) -> Table {
    let group = Table::new().with("x", -6500).with("y", -6500).with("oo", Table::new()).with("n", nodes);
    Table::new().with(1_000_000_000i64, group)
}

/// A passive skill's stats with their values (`Stats` with `Stat1`..).
fn passive_stats(ctx: &Ctx, node: Row) -> Vec<(String, f64)> {
    let stats_col = node.table.pick(&["Stats", "StatsKeys"]).unwrap_or("Stats");
    ctx.rr
        .deref_list_ids(node, stats_col)
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let names = [format!("Stat{}", i + 1), format!("Stat{}Value", i + 1)];
            let value = first_col(node.table, &names).map_or(0, |c| node.int(&c));
            (id, value as f64)
        })
        .collect()
}

/// Sorts description lines by the order each was described at, as the
/// passive exporters re-sort `sd` with `table.sort`.
fn sort_lines(lines: &mut [String], orders: &HashMap<String, f64>) {
    let order = |l: &String| orders.get(l).copied().unwrap_or(f64::NAN);
    lua_sort(lines, &|a, b| order(a) < order(b));
}

/// `TattooPassives.lua`: every passive override a tattoo or runegraft applies,
/// keyed by its name.
pub fn tattoos(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &["passive_skill_stat_descriptions.txt"]);
    let overrides = ctx.table("PassiveSkillOverrides")?;
    let tattoos = ctx.table("PassiveSkillTattoos")?;
    let client_strings = ctx.table("ClientStrings")?;
    let bases = ctx.table("BaseItemTypes")?;
    let exchange = ctx.table("CurrencyExchange")?;
    let type_col = overrides.require(&["OverrideType", "Type"])?;
    let min_col = overrides.require(&["MinimumConnected", "RequiresAdjacent"])?;
    let max_col = overrides.require(&["MaximumConnected", "MaxAdjacent"])?;
    let background_col = overrides.require(&["Background", "PassiveBG"])?;
    let passive_col = overrides.require(&["PassiveSkill", "AllocatedPassiveSkill"])?;
    let stats_col = overrides.require(&["StatsKeys", "Stats"])?;
    let icon_col = overrides.require(&["Icon", "NodeIcon"])?;
    let target_col = tattoos.require(&["NodeTarget", "Set"])?;
    let exchange_base_col = exchange.require(&["BaseItemType", "Item"])?;
    let exchange_enabled_col = exchange.require(&["EnabledInLeague", "EnabledInChallengeLeague"])?;
    let text = |id: &str| client_strings.by_id(id).map(|r| r.string("Text")).unwrap_or_default();
    let mut base_by_name: HashMap<&str, usize> = HashMap::new();
    for base in bases.rows() {
        base_by_name.entry(base.str("Name")).or_insert(base.index);
    }
    let mut listing_by_base: HashMap<usize, Row> = HashMap::new();
    for listing in exchange.rows() {
        if let Some(base) = listing.key(exchange_base_col) {
            listing_by_base.entry(base).or_insert(listing);
        }
    }

    let mut tattoo_of: HashMap<String, Row> = HashMap::new();
    for row in tattoos.rows() {
        if let Some(target) = ctx.rr.deref_id(row, "Override") {
            tattoo_of.insert(target, row);
        }
    }

    let mut nodes = Table::new();
    for row in overrides.rows() {
        let id = row.id();
        let Some(tattoo) = tattoo_of.get(id).or_else(|| tattoo_of.get("DisplayRandomKeystone")) else { continue };
        let target = ctx.rr.deref(*tattoo, target_col);
        let target_type = target.as_ref().map(|t| t.row().string(t.table.pick(&["Type", "Name"]).unwrap_or("Name")));
        let target_value = target.as_ref().map(|t| t.row().string(t.table.pick(&["Value", "Qualifier"]).unwrap_or("Qualifier")));
        let override_type = ctx.rr.deref_id(row, type_col).unwrap_or_default();

        let mut node = Table::new();
        node.set("id", lua_literal(id));
        node.set("isTattoo", true);
        node.set("overrideType", lua_literal(&override_type));
        node.set("not", target_type.as_deref() == Some("Notable"));
        node.set("m", override_type == "AlternateMastery");
        node.set_opt("targetType", target_type.map(|s| lua_literal(&s)));
        node.set_opt("targetValue", target_value.map(|s| lua_literal(&s)));
        let min = row.int(min_col);
        let max = row.int(max_col);
        if min > 0 {
            let reminder = text("PassiveSkillTattooAdjacentRequirementLower").replace("{}", &tostring(min as f64));
            node.set("reminderText", Table::list([lua_literal(&reminder)]));
        }
        node.set("MinimumConnected", min);
        if max > 0 {
            let reminder = text("PassiveSkillTattooAdjacentRequirementUpper").replace("{}", &tostring(max as f64));
            node.set("reminderText", Table::list([lua_literal(&reminder)]));
        }
        node.set("MaximumConnected", if max > 0 { max } else { 100 });
        let limit_text = ctx.rr.deref(row, "Limit").map(|limit| {
            text("PassiveSkillTattooLimitReminder").replace("{0}", limit.row().str("Description"))
        });
        node.set("activeEffectImage", lua_literal(&format!("{}.png", row.str(background_col))));

        let keystone = override_type == "KeystoneTattoo";
        let passive = if keystone { ctx.rr.deref(row, passive_col) } else { None };
        if keystone && passive.is_none() {
            continue;
        }
        let (source_id, source_name, source_icon, mut sd, stats) = match &passive {
            Some(p) => {
                let p = p.row();
                let (sd, stats) = keystone_stats(&descriptors, &passive_stats(ctx, p));
                (p.string("Id"), p.string("Name"), p.string(p.table.pick(&["Icon", "Icon_DDSFile"]).unwrap_or("Icon_DDSFile")), sd, stats)
            }
            None => {
                let ids = ctx.rr.deref_list_ids(row, stats_col);
                let values = row.list_int("StatValues");
                let pairs: Vec<(String, f64)> =
                    ids.into_iter().enumerate().map(|(i, id)| (id, values.get(i).copied().unwrap_or(0) as f64)).collect();
                let (sd, stats) = override_stats(&descriptors, &pairs);
                (id.to_string(), row.string("Name"), row.string(icon_col), sd, stats)
            }
        };
        node.set("ks", keystone);
        if override_type == "AlternateMastery" {
            node.set("name", "Runegraft Mastery");
        }
        node.set("stats", stats);
        node.set("dn", lua_literal(&source_name));
        if !source_name.is_empty() && !keystone {
            let listing = base_by_name.get(source_name.as_str()).and_then(|b| listing_by_base.get(b));
            if let Some(listing) = listing {
                node.set("legacy", !listing.bool(exchange_enabled_col));
            }
        }
        let icon = match source_icon.strip_suffix(".dds") {
            Some(stem) => format!("{}.png", stem),
            None => source_icon.clone(),
        };
        node.set("icon", lua_literal(&icon));
        if let Some(limit) = limit_text {
            sd.push(limit);
        }
        node.set("sd", literals(&sd));
        if source_id != "DisplayRandomKeystone" && !source_name.contains("DNT") && !source_name.contains("of the Test") {
            nodes.set(lua_literal(&source_name), node);
        }
    }
    let data = Table::new().with("nodes", nodes).with("groups", passive_group(Table::new()));
    write(ctx, "TattooPassives", data)
}

/// `parsePassiveStats`: a keystone's stats, each described on its own for its
/// order, and the lines sorted by it.
fn keystone_stats(descriptors: &Descriptors, stats: &[(String, f64)]) -> (Vec<String>, Table) {
    let mut sd = Vec::new();
    let mut orders = HashMap::new();
    let mut table = Table::new();
    for (index, (id, value)) in stats.iter().enumerate() {
        let mut one = Stats::new();
        one.set(id, *value, *value);
        let (described, touched) = describe_marking(descriptors, &mut one);
        let mut entry = stat_entry(&one, id, touched[0]);
        entry.set("index", index + 1);
        entry.set_opt("statOrder", described.orders.first().map(|o| lua_num(*o)));
        table.set(lua_literal(id), entry);
        for (line, order) in described.lines.into_iter().zip(described.orders) {
            orders.insert(line.clone(), order);
            sd.push(line);
        }
    }
    sort_lines(&mut sd, &orders);
    (sd, table)
}

/// `parseStats` of `tattooPassives.lua`: an override's stats described
/// together.
fn override_stats(descriptors: &Descriptors, stats: &[(String, f64)]) -> (Vec<String>, Table) {
    let mut all = Stats::new();
    for (id, value) in stats {
        all.set(id, *value, *value);
    }
    let (described, touched) = describe_marking(descriptors, &mut all);
    let mut table = Table::new();
    for ((id, _), touched) in all.0.iter().zip(&touched) {
        table.set(lua_literal(id), stat_entry(&all, id, *touched));
    }
    let mut orders = HashMap::new();
    let mut sd = Vec::new();
    for (line, order) in described.lines.into_iter().zip(described.orders) {
        orders.insert(line.clone(), order);
        sd.push(line);
    }
    sort_lines(&mut sd, &orders);
    (sd, table)
}

/// `ClusterJewels.lua`: each cluster jewel's size, node indices and the small
/// passives it can add, the notables' sort order, the keystones, and each
/// jewel socket's orbit offsets.
pub fn cluster_jewels(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &["passive_skill_stat_descriptions.txt"]);
    let jewels = ctx.table("PassiveTreeExpansionJewels")?;
    let skills = ctx.table("PassiveTreeExpansionSkills")?;
    let special = ctx.table("PassiveTreeExpansionSpecialSkills")?;
    let slots = ctx.table("PassiveJewelSlots")?;
    let jewel_base_col = jewels.require(&["BaseItemType", "BaseItemTypesKey"])?;
    let jewel_size_col = jewels.require(&["Size", "PassiveTreeExpansionJewelSizesKey"])?;
    let skill_size_col = skills.require(&["JewelSize", "PassiveTreeExpansionJewelSizesKey"])?;
    let node_col = skills.require(&["Node", "PassiveSkillsKey"])?;
    let mastery_col = skills.require(&["Mastery", "Mastery_PassiveSkillsKey"])?;
    let tag_col = skills.require(&["Tag", "TagsKey"])?;
    let small_col = jewels.require(&["SmallIndicies", "SmallIndices"])?;
    let notable_col = jewels.require(&["NotableIndicies", "NotableIndices"])?;
    let socket_col = jewels.require(&["SocketIndicies", "SocketIndices"])?;
    let total_col = jewels.require(&["TotalIndicies", "TotalIndices"])?;
    let icon = |row: Row| row.string(row.table.pick(&["Icon", "Icon_DDSFile"]).unwrap_or("Icon_DDSFile"));

    let mut jewel_table = Table::new();
    for jewel in jewels.rows() {
        let Some(base) = ctx.rr.deref(jewel, jewel_base_col) else { continue };
        let Some(size) = ctx.rr.deref(jewel, jewel_size_col) else { continue };
        let size_name = size.row().string(size.table.pick(&["Id", "Name"]).unwrap_or("Name"));
        let mut skill_table = Table::new();
        for skill in skills.rows().filter(|s| s.key(skill_size_col) == Some(size.index)) {
            let Some(node) = ctx.rr.deref(skill, node_col) else { continue };
            let node_row = node.row();
            let tag = ctx.rr.deref_id(skill, tag_col).unwrap_or_default();
            let mut name = node_row.string("Name");
            if tag.contains("old_do_not_use") {
                name.push_str(" (Legacy)");
            }
            let mut entry = Table::new().with("name", lua_literal(&name)).with("icon", lua_literal(&dds_to_png(&icon(node_row))));
            if let Some(mastery) = ctx.rr.deref(skill, mastery_col) {
                entry.set("masteryIcon", lua_literal(&dds_to_png(&icon(mastery.row()))));
            }
            entry.set("tag", lua_literal(&tag));
            let mut stats = Stats::new();
            for (id, value) in passive_stats(ctx, node_row) {
                stats.set(&id, value, value);
            }
            let lines = descriptors.describe_stats(&mut stats).lines;
            let shown = if lines.is_empty() { vec![String::new()] } else { lines.clone() };
            entry.set("stats", literals(&shown));
            let enchant: Vec<String> = lines.iter().map(|l| format!("Added Small Passive Skills grant: {}", l)).collect();
            entry.set("enchant", literals(&enchant));
            skill_table.set(lua_literal(node_row.id()), entry);
        }
        let entry = Table::new()
            .with("size", lua_literal(&size_name))
            .with("sizeIndex", size.index)
            .with("minNodes", jewel.int("MinNodes"))
            .with("maxNodes", jewel.int("MaxNodes"))
            .with("smallIndicies", Table::list(jewel.list_int(small_col)))
            .with("notableIndicies", Table::list(jewel.list_int(notable_col)))
            .with("socketIndicies", Table::list(jewel.list_int(socket_col)))
            .with("totalIndicies", jewel.int(total_col))
            .with("skills", skill_table);
        jewel_table.set(lua_literal(base.row().str("Name")), entry);
    }

    let special_node_col = special.require(&["Node", "PassiveSkillsKey"])?;
    let special_stat_col = special.require(&["Stat", "StatsKey"])?;
    let mut notables = Table::new();
    let mut keystones = Table::new();
    for row in special.rows() {
        let Some(node) = ctx.rr.deref(row, special_node_col) else { continue };
        let node = node.row();
        let name = lua_literal(node.str("Name"));
        if node.bool(node.table.pick(&["Notable", "IsNotable"]).unwrap_or("IsNotable")) {
            notables.set_opt(name.as_str(), row.key(special_stat_col).map(|i| i + 1));
        }
        if node.bool(node.table.pick(&["Keystone", "IsKeystone"]).unwrap_or("IsKeystone")) {
            keystones.push(name);
        }
    }

    let slot_size_col = slots.require(&["ClusterSize", "ClusterJewelSize"])?;
    let proxy_col = slots.require(&["Proxy", "ProxySlot"])?;
    let mut offsets = Table::new();
    for slot in slots.rows() {
        if ctx.rr.deref(slot, slot_size_col).is_none() {
            continue;
        }
        let Some(proxy) = ctx.rr.deref(slot, proxy_col) else { continue };
        let proxy = proxy.row();
        let node_id = proxy.int(proxy.table.pick(&["PassiveSkillNodeId", "PassiveSkillGraphId"]).unwrap_or("PassiveSkillGraphId"));
        let mut starts = Table::new();
        for (i, start) in slot.list_int("StartIndices").into_iter().take(3).enumerate() {
            starts.set(i as i64, start);
        }
        offsets.set(node_id, starts);
    }

    let data = Table::new()
        .with("jewels", jewel_table)
        .with("notableSortOrder", notables)
        .with("keystones", keystones)
        .with("orbitOffsets", offsets);
    write(ctx, "ClusterJewels", data)
}

/// A legion row's stats with their ranges: `StatsKeys` with `Stat1`.., each an
/// interval in PoB's spec. PoE 1's schema splits them into `Stat1Min` and
/// `Stat1Max` and leaves the later pairs unnamed after the last named one.
fn legion_stat_ranges(ctx: &Ctx, row: Row) -> Vec<(String, f64, f64)> {
    let table = row.table;
    let stats_col = table.pick(&["StatsKeys", "Stats"]).unwrap_or("Stats");
    let named = (1..=6).take_while(|n| table.has_col(&format!("Stat{}Max", n))).last().unwrap_or(0);
    let anchor = format!("Stat{}Max", named);
    ctx.rr
        .deref_list_ids(row, stats_col)
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let n = i + 1;
            let (min, max) = if let Some((a, b)) = row.interval(&format!("Stat{}", n)) {
                (a, b)
            } else if n <= named {
                (row.int(&format!("Stat{}Min", n)), row.int(&format!("Stat{}Max", n)))
            } else {
                let unnamed = |offset: usize| table.column_or_after(&[], &anchor, offset).map_or(0, |c| row.int_at(c));
                (unnamed((n - named) * 2 - 1), unnamed((n - named) * 2))
            };
            (id, min as f64, max as f64)
        })
        .collect()
}

/// A legion node or addition's `sd`, `stats` and `sortedStats`, as each
/// game's `parseStats` builds them. `text` is how the file writes a string.
fn legion_stats(
    game: Game,
    descriptors: &Descriptors,
    ranges: &[(String, f64, f64)],
    text: &dyn Fn(&str) -> String,
) -> (Vec<String>, Table, Vec<String>) {
    struct Entry {
        id: String,
        table: Table,
        order: f64,
        index: usize,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut put = |entry: Entry| match entries.iter_mut().find(|e| e.id == entry.id) {
        Some(e) => *e = entry,
        None => entries.push(entry),
    };
    let mut sd = Vec::new();
    match game {
        Game::Poe2 => {
            let mut orders = HashMap::new();
            for (i, (id, min, max)) in ranges.iter().enumerate() {
                let mut one = Stats::new();
                one.set(id, *min, *max);
                let (described, touched) = describe_marking(descriptors, &mut one);
                let order = described.orders.first().copied().unwrap_or(99999.0);
                let mut table = stat_entry(&one, id, touched[0]);
                table.set("index", i + 1);
                table.set("statOrder", lua_num(order));
                put(Entry { id: id.clone(), table, order, index: i + 1 });
                for (line, o) in described.lines.into_iter().zip(described.orders) {
                    orders.insert(line.clone(), o);
                    sd.push(line);
                }
            }
            sort_lines(&mut sd, &orders);
        }
        Game::Poe1 => {
            let mut all = Stats::new();
            let mut meta: Vec<(String, usize, Option<f64>)> = Vec::new();
            for (i, (id, min, max)) in ranges.iter().enumerate() {
                let mut one = Stats::new();
                one.set(id, *min, *max);
                let order = descriptors.describe_stats(&mut one).orders.first().copied();
                all.set(id, *min, *max);
                meta.retain(|(m, _, _)| m != id);
                meta.push((id.clone(), i + 1, order));
            }
            let (described, touched) = describe_marking(descriptors, &mut all);
            sd = described.lines;
            for ((id, _), touched) in all.0.iter().zip(&touched) {
                let (_, index, order) = meta.iter().find(|(m, _, _)| m == id).cloned().expect("every stat has its meta");
                let mut table = stat_entry(&all, id, *touched);
                table.set("index", index);
                table.set_opt("statOrder", order.map(lua_num));
                put(Entry { id: id.clone(), table, order: order.unwrap_or(f64::INFINITY), index });
            }
        }
    }
    let mut sorted: Vec<usize> = (0..entries.len()).collect();
    sorted.sort_by(|a, b| entries[*a].id.as_bytes().cmp(entries[*b].id.as_bytes()));
    match game {
        Game::Poe2 => lua_sort(&mut sorted, &|a, b| entries[*a].order < entries[*b].order),
        Game::Poe1 => lua_sort(&mut sorted, &|a, b| {
            let (x, y) = (&entries[*a], &entries[*b]);
            x.order < y.order || (x.order == y.order && x.index < y.index)
        }),
    }
    let sorted_ids = sorted.iter().map(|&i| entries[i].id.clone()).collect();
    let mut stats = Table::new();
    for e in entries {
        stats.set(text(&e.id), e.table);
    }
    (sd, stats, sorted_ids)
}

/// A string as the legion file writes it: PoE 1 raw between quotes, PoE 2
/// with `%q` after turning line breaks into spaces.
fn legion_string(game: Game, s: &str) -> String {
    match game {
        Game::Poe1 => lua_literal(s),
        Game::Poe2 => s.replace("\r\n", " ").replace(['\r', '\n'], " "),
    }
}

/// `dn` of an addition: the id with `_` as spaces, its first two words
/// dropped, and each word's first lower-case letter raised.
fn addition_name(id: &str) -> String {
    let spaced = id.replace('_', " ");
    let mut rest = spaced.as_str();
    for _ in 0..2 {
        let word = rest.bytes().take_while(|b| b.is_ascii_alphanumeric()).count();
        if rest.as_bytes().get(word) == Some(&b' ') {
            rest = &rest[word + 1..];
        }
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_lowercase() {
            out.push(b[i].to_ascii_uppercase());
            i += 1;
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                out.push(b[i]);
                i += 1;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `TimelessJewelData/LegionPassives.lua`: every node a timeless jewel can
/// put on the tree, and every stat it can add to a node.
pub fn timeless_jewels(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let descriptors = describer(ctx, &[passive_descriptions(game)]);
    let skills = ctx.table("AlternatePassiveSkills")?;
    let additions = ctx.table("AlternatePassiveAdditions")?;
    let text = |s: &str| legion_string(game, s);
    let lines = |sd: &[String]| Table::list(sd.iter().map(|l| Lua::from(text(l))));
    let mut rng = LuaRandom::new();
    let mut keystones: i64 = -1;

    let mut nodes = Table::new();
    for row in skills.rows() {
        let id = row.id().to_string();
        let types = row.list_int("PassiveType");
        let keystone = types.contains(&4);
        if keystone {
            keystones += 1;
        }
        let (sd, stats, sorted) = legion_stats(game, &descriptors, &legion_stat_ranges(ctx, row), &text);
        let oidx = if keystone { keystones * 3 } else { (rng.random() * 1e5).floor() as i64 };
        let node = Table::new()
            .with("id", text(&id))
            .with("icon", text(row.str("DDSIcon")))
            .with("ks", keystone)
            .with("not", types.contains(&3))
            .with("dn", text(row.str("Name")))
            .with("m", false)
            .with("isJewelSocket", false)
            .with("isMultipleChoice", false)
            .with("isMultipleChoiceOption", false)
            .with("passivePointsGranted", 0)
            .with("spc", Table::new())
            .with("sd", lines(&sd))
            .with("stats", stats)
            .with("sortedStats", Table::list(sorted.iter().map(|s| Lua::from(text(s)))))
            .with("g", 1_000_000_000i64)
            .with("o", if keystone { 4 } else { 3 })
            .with("oidx", oidx)
            .with("sa", 0)
            .with("da", 0)
            .with("ia", 0)
            .with("out", Table::new())
            .with("in", Table::new());
        nodes.push(node);
    }
    let group = Table::list(1..=nodes.len());

    let mut added = Table::new();
    for row in additions.rows() {
        let id = row.id().to_string();
        let (sd, stats, sorted) = legion_stats(game, &descriptors, &legion_stat_ranges(ctx, row), &text);
        added.push(
            Table::new()
                .with("id", text(&id))
                .with("dn", text(&addition_name(&id)))
                .with("sd", lines(&sd))
                .with("stats", stats)
                .with("sortedStats", Table::list(sorted.iter().map(|s| Lua::from(text(s))))),
        );
    }

    let data = Table::new().with("nodes", nodes).with("groups", passive_group(group)).with("additions", added);
    write(ctx, "TimelessJewelData/LegionPassives", data)
}

/// `Pantheons.lua`: each enabled god, and each soul's stat lines with their
/// values.
pub fn pantheons(ctx: &Ctx) -> Result<(), String> {
    let descriptors = describer(ctx, &[main_descriptions(game(ctx))]);
    let table = ctx.table("PantheonPanelLayout")?;
    let disabled_col = table.column_or_after(&["IsDisabled"], "QuestFlag4", 1);
    let mut out = Table::new();
    for row in table.rows() {
        if disabled_col.is_some_and(|c| row.bool_at(c)) {
            continue;
        }
        let mut souls = Table::new();
        for god in 1..=4 {
            let names = [format!("Effect{}StatsKey", god), format!("Effect{}_StatsKeys", god), format!("Effect{}_Stats", god)];
            let Some(stats_col) = first_col(&table, &names) else { continue };
            let keys = ctx.rr.deref_list_ids(row, &stats_col);
            if keys.is_empty() {
                continue;
            }
            let values_names = [format!("Effect{}Values", god), format!("Effect{}_Values", god)];
            let values = first_col(&table, &values_names).map(|c| row.list_int(&c)).unwrap_or_default();
            let mut mods = Table::new();
            for (key, value) in keys.iter().zip(&values) {
                let mut stats = Stats::new();
                stats.set(key, *value as f64, *value as f64);
                let line = descriptors.describe_stats(&mut stats).lines.join(" ");
                mods.push(Table::new().with("line", lua_literal(&line)).with("value", Table::list([*value])));
            }
            let name = row.str(&format!("GodName{}", god));
            souls.set(god, Table::new().with("name", lua_literal(name)).with("mods", mods));
        }
        let god = Table::new().with("isMajorGod", row.bool("IsMajorGod")).with("souls", souls);
        out.set(lua_literal(row.id()), god);
    }
    write(ctx, "Pantheons", out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_random_replays_luajit() {
        // The first non-keystone `oidx` values in PoB's LegionPassives.lua.
        let mut rng = LuaRandom::new();
        let draws: Vec<i64> = (0..4).map(|_| (rng.random() * 1e5).floor() as i64).collect();
        assert_eq!(draws, [79420, 69885, 59010, 75322]);
    }

    #[test]
    fn lua_sort_matches_luajit_on_ties() {
        let sorted = |keys: &[i32]| {
            let mut v: Vec<(i32, char)> = keys.iter().enumerate().map(|(i, k)| (*k, (b'a' + i as u8) as char)).collect();
            lua_sort(&mut v, &|a, b| a.0 < b.0);
            v.into_iter().map(|(_, c)| c).collect::<String>()
        };
        // What LuaJIT 2.1's table.sort gives for the same keys.
        assert_eq!(sorted(&[1, 1]), "ab");
        assert_eq!(sorted(&[2, 1, 1]), "cba");
        assert_eq!(sorted(&[3, 1, 2, 1, 3, 2, 1, 1, 2, 3, 1, 2]), "ghkdbilfceja");
        assert_eq!(sorted(&[5; 15]), "ajikmlnhfbgcedo");
        assert_eq!(sorted(&[2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2]), "jhnrpfldbqmoaickegs");
    }

    #[test]
    fn addition_names_follow_the_gsubs() {
        assert_eq!(addition_name("karui_notable_add_damage_from_crits"), "Add Damage From Crits");
        assert_eq!(addition_name("vaal_small_fire_damage"), "Fire Damage");
        assert_eq!(addition_name("a_b_Cold_x2y"), "COld X2y");
    }
}
