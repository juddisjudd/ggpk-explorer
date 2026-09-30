//! `Mod*.lua`, `ModScalability.lua` and PoE 1's `ModMaster.lua`, ported from
//! PoB's `mods.lua`, `modScalability.lua` and `masters.lua`.

use crate::dat::relational::Row;
use crate::data_export::Ctx;
use crate::pob_export::statdesc::{lua_literal, Descriptors, SpecValue, Stats};
use crate::pob_export::{describer, game, write, Lua, Table};
use crate::settings::Game;
use std::collections::HashMap;

/// A mod as PoB's exporter sees it: the columns `mods.lua` reads, under the
/// names PoB's spec gives them.
pub struct PobMod<'t> {
    pub row: Row<'t>,
    pub id: String,
    pub name: String,
    pub level: i64,
    pub domain: i64,
    pub generation_type: i64,
    /// `ModType.Id` in PoB's spec, the first column (`Name` in dat-schema).
    pub group: String,
    pub families: Vec<String>,
    /// Stat id, minimum, maximum and the stat's `HASH32`, for `Stat1`..`Stat6`
    /// in order, stopping at the first empty one.
    pub stats: Vec<(String, f64, f64, u32)>,
    pub spawn_tags: Vec<String>,
    pub spawn_weights: Vec<i64>,
    pub generation_tags: Vec<String>,
    pub generation_weights: Vec<i64>,
    pub tags: Vec<String>,
    pub implicit_tags: Vec<String>,
    pub aura_flags: usize,
    /// Stats a PoE 1 mod applies through its first buff template, which
    /// `describeMod` leaves to the mod's display stat.
    pub buff_stats: Vec<String>,
    pub unscalable: bool,
    pub node_type: Option<i64>,
}

/// Reads mods the way PoB's spec lays them out, whichever game's columns the
/// table has.
pub struct ModReader {
    stat_cols: Vec<(String, StatValue)>,
}

enum StatValue {
    Interval(String),
    Range(String, String),
}

impl ModReader {
    pub fn new(table: &crate::dat::relational::LoadedTable) -> Self {
        let stat_cols = (1..=6)
            .filter_map(|i| {
                if table.has_col(&format!("Stat{}", i)) {
                    Some((format!("Stat{}", i), StatValue::Interval(format!("Stat{}Value", i))))
                } else if table.has_col(&format!("StatsKey{}", i)) {
                    Some((format!("StatsKey{}", i), StatValue::Range(format!("Stat{}Min", i), format!("Stat{}Max", i))))
                } else {
                    None
                }
            })
            .collect();
        Self { stat_cols }
    }

    pub fn read<'t>(&self, ctx: &Ctx, row: Row<'t>) -> PobMod<'t> {
        let table = row.table;
        let ids = |names: &[&str]| -> Vec<String> {
            match table.pick(names) {
                Some(col) => ctx.rr.deref_list_ids(row, col),
                None => Vec::new(),
            }
        };
        let ints = |names: &[&str]| -> Vec<i64> { table.pick(names).map(|c| row.list_int(c)).unwrap_or_default() };
        let mut stats = Vec::new();
        for (key, value) in &self.stat_cols {
            let Some(stat) = ctx.rr.deref(row, key) else { break };
            let (min, max) = match value {
                StatValue::Interval(col) => row.interval(col).unwrap_or((row.int(col), row.int(col))),
                StatValue::Range(lo, hi) => (row.int(lo), row.int(hi)),
            };
            let hash = stat.row().int("HASH32") as u32;
            stats.push((stat.id(), min as f64, max as f64, hash));
        }
        let group = table
            .pick(&["ModType", "ModTypeKey"])
            .and_then(|c| ctx.rr.deref(row, c))
            .map(|t| t.row().string("Name"))
            .unwrap_or_default();
        let buff_stats = ["BuffTemplate1"]
            .iter()
            .filter(|c| table.has_col(c))
            .filter_map(|c| ctx.rr.deref(row, c))
            .flat_map(|t| {
                let col = t.table.pick(&["StatsKeys", "StatsKey", "Stats"]).unwrap_or("Stats");
                ctx.rr.deref_list_ids(t.row(), col)
            })
            .collect();
        PobMod {
            row,
            id: row.id().to_string(),
            name: row.string("Name"),
            level: row.int("Level"),
            domain: row.int("Domain"),
            generation_type: row.int("GenerationType"),
            group,
            families: ids(&["Families"]),
            stats,
            spawn_tags: ids(&["SpawnWeight_Tags", "SpawnWeight_TagsKeys"]),
            spawn_weights: ints(&["SpawnWeight_Values"]),
            generation_tags: ids(&["GenerationWeight_Tags", "GenerationWeight_TagsKeys"]),
            generation_weights: ints(&["GenerationWeight_Values"]),
            tags: ids(&["Tags", "TagsKeys"]),
            implicit_tags: ids(&["ImplicitTags", "ImplicitTagsKeys"]),
            aura_flags: table.pick(&["AuraFlags"]).map(|c| row.list_int(c).len()).unwrap_or(0),
            buff_stats,
            unscalable: ctx.rr.is_poe2 && row.bool("IsEssenceOnlyModifier"),
            node_type: table.pick(&["RadiusJewelType"]).map(|c| row.int(c)),
        }
    }
}

/// `describeMod`: the mod's lines and their orders. PoE 1 leaves out the
/// stats its buff template applies.
pub fn describe_mod(game: Game, descriptors: &Descriptors, m: &PobMod) -> crate::pob_export::statdesc::Described {
    describe_mod_stats(game, descriptors, &m.stats, &m.buff_stats)
}

/// [`describe_mod`] over stats the caller has adjusted.
pub fn describe_mod_stats(
    game: Game,
    descriptors: &Descriptors,
    mod_stats: &[(String, f64, f64, u32)],
    buff_stats: &[String],
) -> crate::pob_export::statdesc::Described {
    let mut stats = Stats::new();
    for (id, min, max, _) in mod_stats {
        if game == Game::Poe1 && buff_stats.contains(id) {
            continue;
        }
        stats.set(id, *min, *max);
    }
    descriptors.describe_stats(&mut stats)
}

/// PoB's `murmurHash2`: 32-bit MurmurHash2, as the trade site hashes stats.
pub fn murmur_hash2(key: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h = seed ^ key.len() as u32;
    let mut chunks = key.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut tail = [0u8; 4];
        tail[..rest.len()].copy_from_slice(rest);
        h ^= u32::from_le_bytes(tail);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// `HashStats(stats, extraStat)`: the trade site's id for a set of stats.
pub fn hash_stats(stats: &[String], extra: Option<&str>) -> u32 {
    let mut bytes = Vec::new();
    for stat in stats.iter().map(String::as_str).chain(extra) {
        bytes.extend_from_slice(&murmur_hash2(stat.as_bytes(), 0xC58F_1A7B).to_le_bytes());
    }
    murmur_hash2(&bytes, 0x0231_2233)
}

/// `{ "a", "b" }` as PoB writes a string list.
fn strings(items: &[String]) -> Table {
    Table::list(items.iter().map(|s| Lua::from(lua_literal(s))))
}

/// The lines of a mod, the way `mods.lua` writes them: each a string at 1..n.
fn set_lines(entry: &mut Table, lines: &[String]) {
    for line in lines {
        entry.push(lua_literal(line));
    }
}

fn orders_table(orders: &[f64]) -> Table {
    Table::list(orders.iter().copied())
}

// PoE 2's domain and generation numbers, as `mods.lua` names them.
mod poe2 {
    pub const GENERIC: i64 = 1;
    pub const FLASK_CHARM: i64 = 2;
    pub const JEWEL: i64 = 11;
    pub const UNIQUE_JEWEL: i64 = 22;
    pub const VEILED: i64 = 28;
    pub const INCURSION_LIMB: i64 = 37;
    pub const PREFIX: i64 = 1;
    pub const SUFFIX: i64 = 2;
    pub const INTRINSIC: i64 = 3;
    pub const CORRUPTION: i64 = 5;
}

// PoE 1's, as its `mods.lua` names them.
mod poe1 {
    pub const ITEM: i64 = 1;
    pub const FLASK: i64 = 2;
    pub const JEWEL: i64 = 10;
    pub const ABYSS_JEWEL: i64 = 13;
    pub const DELVE_FOSSIL: i64 = 16;
    pub const CLUSTER_JEWEL: i64 = 21;
    pub const UNVEILED: i64 = 28;
    pub const TINCTURE: i64 = 34;
    pub const CHARM: i64 = 35;
    pub const GRAFT: i64 = 38;
    pub const MERCENARY: i64 = 41;
    pub const PREFIX: i64 = 1;
    pub const SUFFIX: i64 = 2;
    pub const INTRINSIC: i64 = 3;
    pub const CORRUPTED: i64 = 5;
    pub const SCOURGE_BENEFIT: i64 = 24;
    pub const SCOURGE_DOWNSIDE: i64 = 25;
    pub const SEARING_EXARCH: i64 = 28;
    pub const EATER_OF_WORLDS: i64 = 29;
}

type Filter = fn(&PobMod) -> bool;

fn affix(m: &PobMod) -> bool {
    m.generation_type == 1 || m.generation_type == 2
}

fn first_family_not(m: &PobMod, id: &str) -> bool {
    m.families.first().map_or(true, |f| f != id)
}

const POE2_FILES: [(&str, Filter); 8] = [
    ("ModItem", |m| {
        m.domain == poe2::GENERIC && affix(m) && first_family_not(m, "AuraBonus") && !m.id.contains("Cowards")
            && !m.id.contains("Master")
    }),
    ("ModCorrupted", |m| {
        (m.domain == poe2::JEWEL || m.domain == poe2::GENERIC)
            && ((m.generation_type == poe2::INTRINSIC && m.id.contains("SpecialCorruption"))
                || m.generation_type == poe2::CORRUPTION)
    }),
    ("ModFlask", |m| m.domain == poe2::FLASK_CHARM && affix(m) && m.id.starts_with("Flask")),
    ("ModCharm", |m| {
        m.domain == poe2::FLASK_CHARM
            && ((m.generation_type == poe2::PREFIX && m.id.starts_with("Charm"))
                || (m.generation_type == poe2::SUFFIX && !m.id.contains("Immunity")))
    }),
    ("ModJewel", |m| m.domain == poe2::JEWEL && affix(m)),
    ("ModIncursionLimb", |m| m.domain == poe2::INCURSION_LIMB && m.generation_type == poe2::INTRINSIC),
    ("ModItemExclusive", |m| {
        matches!(m.domain, poe2::GENERIC | poe2::FLASK_CHARM | poe2::JEWEL | poe2::UNIQUE_JEWEL)
            && m.generation_type == poe2::INTRINSIC
            && first_family_not(m, "AuraBonus")
            && !m.id.starts_with("Synthesis")
            && !["Royale", "Cowards", "Map", "Ultimatum", "SpecialCorruption"].iter().any(|s| m.id.contains(s))
    }),
    ("ModVeiled", |m| m.domain == poe2::VEILED && !m.id.contains("Map")),
];

/// The two names PoE 1 moves between its delve files.
fn delve_name(m: &PobMod) -> bool {
    m.name == "Subterranean" || m.name == "of the Underground"
}

const POE1_FILES: [(&str, Filter); 21] = [
    ("ModExplicit", |m| {
        m.domain == poe1::ITEM
            && affix(m)
            && !m.id.contains("Royale")
            && !m.id.contains("Necropolis")
            && !m.id.starts_with("Synthesis")
            && !delve_name(m)
            && m.aura_flags == 0
    }),
    ("ModCorrupted", |m| m.generation_type == poe1::CORRUPTED && m.domain == poe1::ITEM),
    ("ModDelve", |m| m.domain == poe1::DELVE_FOSSIL || delve_name(m)),
    ("ModSynthesis", |m| m.generation_type == poe1::INTRINSIC && m.domain == poe1::ITEM && m.id.starts_with("Synthesis")),
    ("ModScourge", |m| {
        m.domain == poe1::ITEM
            && (m.generation_type == poe1::SCOURGE_BENEFIT || m.generation_type == poe1::SCOURGE_DOWNSIDE)
            && !is_hellscape_map(&m.id)
    }),
    ("ModEldritch", |m| {
        m.domain == poe1::ITEM && (m.generation_type == poe1::SEARING_EXARCH || m.generation_type == poe1::EATER_OF_WORLDS)
    }),
    ("ModFlask", |m| m.domain == poe1::FLASK && affix(m)),
    ("ModTincture", |m| m.domain == poe1::TINCTURE && affix(m)),
    ("ModJewel", |m| {
        (m.domain == poe1::JEWEL || m.domain == poe1::DELVE_FOSSIL) && (affix(m) || m.generation_type == poe1::CORRUPTED)
    }),
    ("ModJewelAbyss", |m| {
        (m.domain == poe1::ABYSS_JEWEL || m.domain == poe1::DELVE_FOSSIL) && (affix(m) || m.generation_type == poe1::CORRUPTED)
    }),
    ("ModJewelCluster", |m| {
        (m.domain == poe1::CLUSTER_JEWEL && affix(m)) || (m.domain == poe1::JEWEL && m.generation_type == poe1::CORRUPTED)
    }),
    ("ModJewelCharm", |m| m.domain == poe1::CHARM && affix(m)),
    ("Uniques/Special/WatchersEye", |m| {
        m.families.first().is_some_and(|f| f == "AuraBonus" || f == "ArbalestBonus")
            && m.generation_type == poe1::INTRINSIC
            && !m.id.starts_with("Synthesis")
    }),
    ("Uniques/Special/BoundByDestiny", |m| m.families.get(1).is_some_and(|f| f.contains("MatchedInfluencesTier"))),
    ("ModVeiled", |m| m.domain == poe1::UNVEILED && affix(m)),
    ("ModNecropolis", |m| m.domain == poe1::ITEM && m.id.starts_with("NecropolisCrafting")),
    ("ModItemExclusive", |m| {
        matches!(m.domain, poe1::ITEM | poe1::FLASK | poe1::JEWEL | poe1::CLUSTER_JEWEL | poe1::TINCTURE)
            && m.generation_type == poe1::INTRINSIC
            && m.families.first().is_some_and(|f| f != "AuraBonus")
            && !m.id.starts_with("Synthesis")
            && !["Royale", "Cowards", "Map", "Ultimatum", "UNUSED"].iter().any(|s| m.id.contains(s))
            && !m.id.starts_with("MutatedUnique")
    }),
    ("ModGraft", |m| m.domain == poe1::GRAFT && (affix(m) || m.generation_type == poe1::CORRUPTED)),
    ("BeastCraft", |m| m.id.contains("Aspect") && m.generation_type == poe1::SUFFIX),
    ("ModFoulborn", |m| {
        (m.domain == poe1::ITEM || m.domain == poe1::JEWEL) && m.generation_type == poe1::INTRINSIC
            && m.id.starts_with("MutatedUnique")
    }),
    ("ModMercenary", |m| m.domain == poe1::MERCENARY && affix(m)),
];

/// `^Hellscape[UpDown]+sideMap`.
fn is_hellscape_map(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("Hellscape") else { return false };
    let letters = rest.bytes().take_while(|b| b"UpDown".contains(b)).count();
    letters > 0 && rest[letters..].starts_with("sideMap")
}

pub fn mods(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let table = ctx.table("Mods")?;
    let reader = ModReader::new(&table);
    let descriptors = match game {
        Game::Poe2 => describer(ctx, &["stat_descriptions.csd"]),
        Game::Poe1 => describer(ctx, &["tincture_stat_descriptions.txt", "graft_stat_descriptions.txt"]),
    };
    let mods: Vec<PobMod> = table.rows().map(|row| reader.read(ctx, row)).collect();
    let files: &[(&str, Filter)] = match game {
        Game::Poe2 => &POE2_FILES,
        Game::Poe1 => &POE1_FILES,
    };
    for (name, filter) in files {
        let mut out = Table::new();
        for m in mods.iter().filter(|m| filter(m)) {
            if game == Game::Poe1 && m.domain == poe1::DELVE_FOSSIL && !delve_kept(name, m) {
                continue;
            }
            if let Some(entry) = mod_entry(game, &descriptors, m, name) {
                out.set(m.id.as_str(), entry);
            }
        }
        write(ctx, name, out)?;
    }
    Ok(())
}

/// PoE 1 sorts fossil mods between the item and jewel files by spawn tag.
fn delve_kept(file: &str, m: &PobMod) -> bool {
    if file.contains("Item") {
        !(m.spawn_tags.first().is_some_and(|t| t == "abyss_jewel")
            && m.spawn_tags.get(1).is_some_and(|t| t == "jewel")
            && m.spawn_tags.len() == 3)
    } else if file.contains("JewelAbyss") {
        m.spawn_tags.iter().any(|t| t == "abyss_jewel")
    } else if file.contains("Jewel") {
        m.spawn_tags.iter().any(|t| t == "jewel")
    } else {
        true
    }
}

/// One mod's entry, or `None` when it has nothing to show.
fn mod_entry(game: Game, descriptors: &Descriptors, m: &PobMod, file: &str) -> Option<Table> {
    let described = describe_mod(game, descriptors, m);
    let mut lines = described.lines;
    let mut orders = described.orders;
    if orders.is_empty() {
        return None;
    }
    if game == Game::Poe1 && lines.first().is_some_and(|l| l.starts_with("DNT")) {
        return None;
    }
    let mut entry = Table::new();
    let kind = match game {
        Game::Poe2 => match m.generation_type {
            poe2::PREFIX => Some("Prefix".to_string()),
            poe2::SUFFIX => Some("Suffix".to_string()),
            poe2::INTRINSIC if m.id.contains("SpecialCorruption") => Some("SpecialCorrupted".to_string()),
            poe2::CORRUPTION => Some("Corrupted".to_string()),
            _ => None,
        },
        Game::Poe1 => poe1_type(m),
    };
    entry.set_opt("type", kind);
    entry.set("affix", lua_literal(&m.name));
    if game == Game::Poe2 {
        for (index, family) in m.families.iter().enumerate() {
            let index = index + 1;
            if family.contains("LocalDisplayNearbyEnemy") && lines.len() > index && orders.len() > index {
                lines.remove(index - 1);
                orders.remove(index - 1);
                break;
            }
        }
    } else {
        let prefix = if m.id.contains("EldritchImplicitUniquePresence") {
            Some("While a Unique Enemy is in your Presence, ")
        } else if m.id.contains("EldritchImplicitPinnaclePresence") {
            Some("While a Pinnacle Atlas Boss is in your Presence, ")
        } else {
            None
        };
        if let Some(prefix) = prefix {
            for line in lines.iter_mut() {
                *line = format!("{}{}", prefix, line);
            }
        }
    }
    set_lines(&mut entry, &lines);
    entry.set("statOrder", orders_table(&orders));
    entry.set("level", m.level);
    entry.set("group", lua_literal(&m.group));
    let spawn_tags: Vec<String> = m
        .spawn_tags
        .iter()
        .map(|t| match (game, m.domain, t.as_str()) {
            (Game::Poe2, poe2::JEWEL, "default") => "jewel".to_string(),
            _ => t.clone(),
        })
        .collect();
    entry.set("weightKey", strings(&spawn_tags));
    entry.set("weightVal", Table::list(m.spawn_weights.iter().copied()));
    if !m.generation_tags.is_empty() {
        let notable_cluster = game == Game::Poe1
            && m.generation_type == poe1::SUFFIX
            && file == "ModJewelCluster"
            && m.tags.first().is_some_and(|t| t == "has_affliction_notable");
        let mut keys = m.generation_tags.clone();
        let mut values = m.generation_weights.clone();
        let mut tags = m.tags.clone();
        if notable_cluster {
            keys.insert(0, "has_affliction_notable2".into());
            values.insert(0, 0);
            tags.insert(0, "has_affliction_notable2".into());
        }
        entry.set("weightMultiplierKey", strings(&keys));
        entry.set("weightMultiplierVal", Table::list(values));
        if game == Game::Poe1 && !m.tags.is_empty() {
            entry.set("tags", strings(&tags));
        }
    }
    if game == Game::Poe2 && !m.tags.is_empty() {
        entry.set("tags", strings(&m.tags));
    }
    entry.set("modTags", strings(&m.implicit_tags));
    if game == Game::Poe2 {
        if m.unscalable {
            entry.set("unscalable", true);
        }
        if let Some(node) = m.node_type.filter(|n| *n != 3) {
            entry.set("nodeType", node);
        }
    }
    entry.set(
        "tradeHashes",
        match game {
            Game::Poe2 => poe2_trade_hashes(descriptors, m),
            Game::Poe1 => poe1_trade_hashes(descriptors, &m.stats),
        },
    );
    Some(entry)
}

/// PoE 1's `type` for the intrinsic, corrupted, scourge and eldritch mods.
fn poe1_type(m: &PobMod) -> Option<String> {
    match m.generation_type {
        poe1::PREFIX => Some("Prefix".into()),
        poe1::SUFFIX => Some("Suffix".into()),
        poe1::INTRINSIC => match m.domain {
            poe1::ITEM if m.id.starts_with("Synthesis") => Some("Synthesis".into()),
            poe1::ITEM => {
                let second = m.families.get(1)?;
                if !second.contains("MatchedInfluencesTier") {
                    return None;
                }
                let tier: String = second.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
                let first = m.families.first()?;
                let influence = first.find("Influence").map(|at| &first[..at])?;
                Some(format!("{}{}", tier, influence))
            }
            poe1::DELVE_FOSSIL => Some("DelveImplicit".into()),
            _ => None,
        },
        poe1::CORRUPTED => Some("Corrupted".into()),
        poe1::SCOURGE_BENEFIT => Some("ScourgeUpside".into()),
        poe1::SCOURGE_DOWNSIDE => Some("ScourgeDownside".into()),
        poe1::SEARING_EXARCH => Some("Exarch".into()),
        poe1::EATER_OF_WORLDS => Some("Eater".into()),
        _ => None,
    }
}

/// `{ [hash] = { lines… } }`: each stat group's trade id with the text the
/// group describes. An empty description still writes one empty string.
fn trade_entry(lines: Vec<String>) -> Table {
    if lines.is_empty() {
        return Table::list([""]);
    }
    Table::list(lines.into_iter().map(|l| lua_literal(&l)))
}

fn poe2_trade_hashes(descriptors: &Descriptors, m: &PobMod) -> Table {
    let mut out = Table::new();
    let (extra, prefix) = match m.node_type {
        Some(2) => (Some("local_jewel_mod_stats_added_to_notable_passives"), Some("Notable Passive Skills in Radius also grant ")),
        Some(1) => (Some("local_jewel_mod_stats_added_to_small_passives"), Some("Small Passive Skills in Radius also grant ")),
        _ => (None, None),
    };
    for (stat_id, _, _, _) in &m.stats {
        let Some(&index) = descriptors.by_stat.get(stat_id) else { continue };
        let Some(ids) = descriptors.descriptors[index].stats.as_ref() else { continue };
        let mut current = Stats::new();
        for id in ids {
            for (sid, min, max, _) in &m.stats {
                if sid == id {
                    current.set(id, *min, *max);
                }
            }
        }
        let mut lines = descriptors.describe_stats(&mut current).lines;
        if let Some(prefix) = prefix {
            for line in lines.iter_mut() {
                *line = format!("{}{}", prefix, line);
            }
        }
        out.set(hash_stats(ids, extra) as i64, trade_entry(lines));
    }
    out
}

fn poe1_trade_hashes(descriptors: &Descriptors, stats: &[(String, f64, f64, u32)]) -> Table {
    let mut out = Table::new();
    let mut i = 0;
    while i < stats.len() {
        if i == 5 {
            break;
        }
        let (id, min, max, hash) = &stats[i];
        let mut current = Stats::new();
        current.set(id, *min, *max);
        let mut bytes = hash.to_le_bytes().to_vec();
        if id.contains("minimum") {
            if let Some((next_id, next_min, next_max, next_hash)) = stats.get(i + 1) {
                if next_id.contains("maximum") {
                    i += 1;
                    bytes.extend_from_slice(&next_hash.to_le_bytes());
                    current.set(next_id, *next_min, *next_max);
                }
            }
        }
        let lines = descriptors.describe_stats(&mut current).lines;
        out.set(murmur_hash2(&bytes, 0x0231_2233) as i64, trade_entry(lines));
        i += 1;
    }
    out
}

/// `ModScalability.lua`: for every wording in `stat_descriptions`, with its
/// values replaced by `#`, whether each value scales and which handlers
/// format it.
pub fn mod_scalability(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let file = match game {
        Game::Poe2 => "stat_descriptions.csd",
        Game::Poe1 => "stat_descriptions.txt",
    };
    let descriptors = describer(ctx, &[file]);
    let stats = ctx.table("Stats")?;
    let scalable: HashMap<&str, bool> = stats.rows().map(|r| (r.id(), r.bool("IsScalable"))).collect();

    struct Slot {
        scalable: Option<bool>,
        formats: Option<Vec<String>>,
    }
    let mut out: HashMap<String, Vec<Slot>> = HashMap::new();
    let mut keys: Vec<&String> = descriptors.by_stat.keys().collect();
    keys.sort();
    for key in keys {
        let descriptor = &descriptors.descriptors[descriptors.by_stat[key]];
        let Some(ids) = descriptor.stats.as_ref() else { continue };
        let per_stat: Vec<Option<bool>> = ids.iter().map(|id| scalable.get(id.as_str()).copied()).collect();
        for wording in &descriptor.wordings {
            let mut formats: HashMap<usize, Vec<String>> = HashMap::new();
            for (k, v) in &wording.specs {
                if let SpecValue::Num(n) = v {
                    formats.entry(*n as usize).or_default().push(k.clone());
                }
            }
            let (stripped, in_order) = strip_values(&wording.text);
            if game == Game::Poe1 && stripped.starts_with("DNT") {
                continue;
            }
            let slots: Vec<Slot> = in_order
                .iter()
                .map(|n| Slot { scalable: per_stat.get(n - 1).copied().flatten(), formats: formats.get(n).cloned() })
                .collect();
            match out.get_mut(&stripped) {
                Some(prior) => {
                    for (j, p) in prior.iter_mut().enumerate() {
                        let prior_count = p.formats.as_ref().map_or(0, Vec::len);
                        let this_count = formats.get(&(j + 1)).map_or(0, Vec::len);
                        if prior_count > this_count {
                            *p = match slots.get(j) {
                                Some(s) => Slot { scalable: s.scalable, formats: s.formats.clone() },
                                None => Slot { scalable: None, formats: None },
                            };
                        }
                    }
                }
                None => {
                    out.insert(stripped, slots);
                }
            }
        }
    }
    let mut table = Table::new();
    for (line, slots) in out {
        let list = Table::list(slots.into_iter().map(|s| {
            Table::new()
                .with_opt("isScalable", s.scalable)
                .with_opt("formats", s.formats.map(|f| Table::list(f)))
        }));
        table.set(lua_literal(&line), list);
    }
    write(ctx, "ModScalability", table)
}

/// `text:gsub("[%+%-]?(%b{})", …)`: each value slot, with any sign before
/// it, becomes `#`; returns the stat number (1-based) each slot shows.
fn strip_values(text: &str) -> (String, Vec<usize>) {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut slots = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let sign = matches!(b[i], b'+' | b'-');
        let open = if sign { i + 1 } else { i };
        if b.get(open) == Some(&b'{') {
            let mut depth = 0;
            let mut close = None;
            for (j, &c) in b.iter().enumerate().skip(open) {
                if c == b'{' {
                    depth += 1;
                } else if c == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
            }
            if let Some(close) = close {
                let inner = &b[open..=close];
                let n = inner.iter().find(|c| c.is_ascii_digit()).map_or(0, |c| (c - b'0') as usize);
                slots.push(n + 1);
                out.push(b'#');
                i = close + 1;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    (String::from_utf8_lossy(&out).into_owned(), slots)
}

/// PoE 1's crafting bench item classes, by the names `ModMaster.lua` uses.
const MASTER_CLASSES: [(&str, &str); 32] = [
    ("LifeFlask", "Flask"),
    ("ManaFlask", "Flask"),
    ("HybridFlask", "Flask"),
    ("Amulet", "Amulet"),
    ("Ring", "Ring"),
    ("Claw", "Claw"),
    ("Dagger", "Dagger"),
    ("Rune Dagger", "Dagger"),
    ("Wand", "Wand"),
    ("One Hand Sword", "One Handed Sword"),
    ("Thrusting One Hand Sword", "Thrusting One Handed Sword"),
    ("One Hand Axe", "One Handed Axe"),
    ("One Hand Mace", "One Handed Mace"),
    ("Bow", "Bow"),
    ("Fishing Rod", "Fishing Rod"),
    ("Staff", "Staff"),
    ("Warstaff", "Staff"),
    ("Two Hand Sword", "Two Handed Sword"),
    ("Two Hand Axe", "Two Handed Axe"),
    ("Two Hand Mace", "Two Handed Mace"),
    ("Quiver", "Quiver"),
    ("Belt", "Belt"),
    ("Gloves", "Gloves"),
    ("Boots", "Boots"),
    ("Body Armour", "Body Armour"),
    ("Helmet", "Helmet"),
    ("Shield", "Shield"),
    ("Sceptre", "Sceptre"),
    ("UtilityFlask", "Flask"),
    ("UtilityFlaskCritical", "Flask"),
    ("Map", "Map"),
    ("Jewel", "Jewel"),
];

/// `ModMaster.lua`: every enabled crafting bench option that adds a mod.
pub fn mod_master(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let options = ctx.table("CraftingBenchOptions")?;
    let mods = ctx.table("Mods")?;
    let reader = ModReader::new(&mods);
    let descriptors = describer(ctx, &["stat_descriptions.txt"]);
    let mod_col = options.require(&["AddMod", "ModsKey", "Mod"])?;
    let categories_col = options.require(&["CraftingItemClassCategories", "ItemCategories"])?;
    let mut out = Table::new();
    for row in options.rows() {
        if row.bool("IsDisabled") {
            continue;
        }
        let Some(target) = ctx.rr.deref(row, mod_col) else { continue };
        let m = reader.read(ctx, target.row());
        let mut entry = Table::new();
        match m.generation_type {
            1 => {
                entry.set("type", "Prefix");
            }
            2 => {
                entry.set("type", "Suffix");
            }
            _ => {}
        }
        entry.set("affix", lua_literal(&m.name));
        let described = describe_mod(game, &descriptors, &m);
        entry.set("modTags", strings(&m.implicit_tags));
        set_lines(&mut entry, &described.lines);
        entry.set("statOrder", orders_table(&described.orders));
        entry.set("level", m.level);
        entry.set("group", lua_literal(&m.group));
        let mut types = Table::new();
        for category in ctx.rr.deref_list(row, categories_col) {
            let classes_col = category.table.pick(&["ItemClassesKeys", "ItemClasses"]).unwrap_or("ItemClassesKeys");
            for class in ctx.rr.deref_list_ids(category.row(), classes_col) {
                if let Some((_, name)) = MASTER_CLASSES.iter().find(|(id, _)| *id == class) {
                    types.set(*name, true);
                }
            }
        }
        entry.set("types", types);
        out.push(entry);
    }
    write(ctx, "ModMaster", out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_hashes_match_pob() {
        assert_eq!(murmur_hash2(b"", 0), 0);
        assert_eq!(murmur_hash2(b"a", 0), 0x92685f5e);
        // `AddedColdDamage1` in PoB's ModItem.lua.
        let stats = ["attack_minimum_added_cold_damage".to_string(), "attack_maximum_added_cold_damage".to_string()];
        assert_eq!(hash_stats(&stats, None), 4067062424);
    }

    #[test]
    fn stripping_values_reports_which_stat_each_slot_shows() {
        let (line, slots) = strip_values("Adds {0} to {1} Fire damage, +{0:+d}% and {}");
        assert_eq!(line, "Adds # to # Fire damage, #% and #");
        assert_eq!(slots, [1, 2, 1, 1]);
        assert!(is_hellscape_map("HellscapeUpsideMap1"));
        assert!(!is_hellscape_map("HellscapeUpside1"));
    }

}
