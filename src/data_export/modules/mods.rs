//! `mods.json` — every modifier with its stats, spawn weights and rendered
//! text — and `mods_by_base.json`, which inverts that into "what can roll on
//! this base item".

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use crate::dat::relational::{LoadedTable, Row};
use std::collections::{BTreeMap, HashMap, HashSet};

/// How many `StatN` columns a mod can carry. The schema currently declares
/// eight; reading them all costs nothing when the later ones are empty.
const MAX_STATS: usize = 8;

/// Mod domain names, as the published files spell them. The index is the
/// domain number stored on the mod.
const DOMAINS: [(i64, &str); 33] = [
    (1, "item"),
    (2, "flask"),
    (3, "monster"),
    (4, "chest"),
    (5, "strongbox"),
    (6, "area"),
    (8, "sanctum_relic"),
    (10, "crafted"),
    (11, "misc"),
    (12, "atlas"),
    (13, "leaguestone"),
    (15, "map_device"),
    (16, "dummy"),
    (18, "delve_area"),
    (19, "synthesis_a"),
    (20, "synthesis_globals"),
    (21, "synthesis_bonus"),
    (22, "affliction_jewel"),
    (23, "heist_area"),
    (24, "heist_npc"),
    (25, "heist_trinket"),
    (26, "watchstone"),
    (27, "veiled"),
    (28, "desecrated"),
    (29, "expedition_relic"),
    (31, "sentinel"),
    (32, "memory_line"),
    (33, "sanctified_relic"),
    (34, "tablet"),
    (35, "ultimatum_key"),
    (36, "vault_key"),
    (37, "incursion_limb"),
    (38, "mods_disallowed"),
];

const GENERATION_TYPES: [(i64, &str); 21] = [
    (1, "prefix"),
    (2, "suffix"),
    (3, "unique"),
    (4, "nemesis"),
    (5, "corrupted"),
    (6, "bloodlines"),
    (7, "torment"),
    (8, "tempest"),
    (9, "talisman"),
    (11, "essence"),
    (13, "bestiary"),
    (14, "delve_area"),
    (15, "synthesis_a"),
    (16, "synthesis_globals"),
    (17, "synthesis_bonus"),
    (18, "blight"),
    (20, "monster_affliction"),
    (23, "expedition_logbook"),
    (26, "scourge_gimmick"),
    (33, "instilled"),
    (34, "azmeri_empowered_monster"),
];

/// Which description file renders a domain's mods; anything unlisted uses the
/// general one.
fn translation_file(domain: i64) -> &'static str {
    match domain {
        3 => "monster_stat_descriptions",
        4 | 5 => "chest_stat_descriptions",
        6 | 10 | 15 | 18 => "map_stat_descriptions",
        8 => "sanctum_relic_stat_descriptions",
        12 => "atlas_stat_descriptions",
        13 => "leaguestone_stat_descriptions",
        24 => "heist_equipment_stat_descriptions",
        31 => "sentinel_stat_descriptions",
        34 => "tablet_stat_descriptions",
        _ => "stat_descriptions",
    }
}

fn name_of(table: &[(i64, &'static str)], value: i64) -> Option<&'static str> {
    table.iter().find(|(v, _)| *v == value).map(|(_, name)| *name)
}

/// Name of a mod domain, shared with `base_items`.
pub fn domain_name(value: i64) -> Option<&'static str> {
    name_of(&DOMAINS, value)
}

/// Renders a mod's stat lines. Held apart from `mods` because `base_items`
/// reports the same text for the implicits it names.
pub struct ModText {
    stat_columns: Vec<(String, String)>,
}

impl ModText {
    pub fn new(table: &LoadedTable) -> Self {
        Self {
            stat_columns: (1..=MAX_STATS)
                .filter(|i| table.has_col(&format!("Stat{}", i)))
                .map(|i| (format!("Stat{}", i), format!("Stat{}Value", i)))
                .collect(),
        }
    }

    fn stats(&self, ctx: &Ctx, row: Row<'_>) -> Vec<Stat> {
        read_stats(ctx, row, &self.stat_columns)
    }

    fn describe(ctx: &Ctx, row: Row<'_>, stats: &[Stat]) -> Vec<String> {
        // A stat pinned to zero grants nothing, so it contributes no line.
        let described: Vec<&Stat> = stats.iter().filter(|s| s.min != 0 || s.max != 0).collect();
        let ids: Vec<String> = described.iter().map(|s| s.id.clone()).collect();
        let ranges: Vec<(i32, i32)> = described.iter().map(|s| (s.min as i32, s.max as i32)).collect();
        ctx.translations(translation_file(row.int("Domain"))).translate_ranges(&ids, &ranges)
    }

    /// The lines one mod shows, as the client draws them.
    pub fn lines(&self, ctx: &Ctx, row: Row<'_>) -> Vec<String> {
        Self::describe(ctx, row, &self.stats(ctx, row))
    }
}

/// Stat text keeps the client's link markup: `[Resistances|Fire Resistance]`
/// draws the part after the bar, `[Resistances]` the whole token.
pub fn display_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|i| open + i) else { break };
        out.push_str(&rest[..open]);
        let token = &rest[open + 1..close];
        out.push_str(token.rsplit('|').next().unwrap_or(token));
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

pub fn mods(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("Mods")?;
    let renderer = ModText::new(&table);

    let prices = gold_prices(ctx);

    let mut root: Vec<(String, J)> = Vec::new();
    let mut seen = HashSet::new();
    for row in table.rows() {
        let id = row.id().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue; // first definition wins, as RePoE reports duplicates
        }
        let domain = row.int("Domain");
        let stats = renderer.stats(ctx, row);
        let lines = ModText::describe(ctx, row, &stats);

        let entry = Obj::new()
            .set("adds_tags", json::strings(ctx.rr.deref_list_ids(row, "Tags")))
            .set("domain", text(name_of(&DOMAINS, domain).unwrap_or("<unknown>")))
            .set("generation_type", text(name_of(&GENERATION_TYPES, row.int("GenerationType")).unwrap_or("<unknown>")))
            .set("generation_weights", weights(ctx, row, "GenerationWeight_Tags", "GenerationWeight_Values"))
            .set("grants_effects", granted_effects(ctx, row))
            .set("groups", json::strings(ctx.rr.deref_list_ids(row, "Families")))
            .set("implicit_tags", json::strings(ctx.rr.deref_list_ids(row, "ImplicitTags")))
            .set("is_essence_only", J::Bool(row.bool("IsEssenceOnlyModifier")))
            .set("name", text(row.str("Name")))
            .set("required_level", int(row.int("Level")))
            .set("spawn_weights", weights(ctx, row, "SpawnWeight_Tags", "SpawnWeight_Values"))
            .set("stats", J::Arr(stats.iter().map(Stat::to_json).collect()))
            .or_null("text", (!lines.is_empty()).then(|| text(lines.join("\n"))))
            .set("type", text(ctx.rr.deref(row, "ModType").map(|m| m.row().string("Name")).unwrap_or_default()))
            .or_null("gold_value", prices.get(&row.index).map(|v| int(*v)))
            .build();
        root.push((id, entry));
    }

    ctx.write("mods", &J::Obj(root))
}

struct Stat {
    id: String,
    min: i64,
    max: i64,
}

impl Stat {
    fn to_json(&self) -> J {
        Obj::new().set("id", text(&self.id)).set("max", int(self.max)).set("min", int(self.min)).build()
    }
}

fn read_stats(ctx: &Ctx, row: Row<'_>, columns: &[(String, String)]) -> Vec<Stat> {
    columns
        .iter()
        .filter_map(|(key, value)| {
            let id = ctx.rr.deref_id(row, key)?;
            let (min, max) = row.interval(value).unwrap_or((0, 0));
            Some(Stat { id, min, max })
        })
        .collect()
}

fn weights(ctx: &Ctx, row: Row<'_>, tags: &str, values: &str) -> J {
    let weights = row.list_int(values);
    let entries = ctx.rr.deref_list(row, tags).into_iter().enumerate().map(|(i, tag)| {
        Obj::new()
            .set("tag", text(tag.id()))
            .set("weight", int(weights.get(i).copied().unwrap_or(0)))
            .build()
    });
    J::Arr(entries.collect())
}

fn granted_effects(ctx: &Ctx, row: Row<'_>) -> J {
    let entries = ctx.rr.deref_list(row, "GrantedEffectsPerLevel").into_iter().filter_map(|per_level| {
        let level_row = per_level.row();
        let effect = ctx.rr.deref_id(level_row, "GrantedEffect")?;
        Some(
            Obj::new()
                .set("granted_effect_id", text(effect))
                .set("level", int(level_row.int("Level")))
                .build(),
        )
    });
    J::Arr(entries.collect())
}

/// `GoldModPrices` keyed by the mod row it prices.
fn gold_prices(ctx: &Ctx) -> HashMap<usize, i64> {
    let Some(table) = ctx.optional_table("GoldModPrices") else { return HashMap::new() };
    let mut out = HashMap::new();
    for row in table.rows() {
        if let Some(target) = row.key("Mod") {
            out.entry(target).or_insert_with(|| row.int("Value"));
        }
    }
    out
}

/// `mods_by_base.json`: for every base item, the mods that can roll on it,
/// grouped by item class, then by the base's tag set, then generation type and
/// mod group.
pub fn mods_by_base(ctx: &Ctx) -> Result<(), String> {
    let bases = super::items::collect_bases(ctx)?;
    let classes = ctx.table("ItemClasses")?;
    let mods = ctx.table("Mods")?;
    let by_domain = mods_by_domain(ctx, &mods);

    // class name -> joined tags -> the mods that set can roll
    let mut root: Vec<(String, Vec<(String, TagSet)>)> = Vec::new();
    for base in &bases {
        let Some(class) = classes.by_id(&base.item_class) else { continue };
        let class_name = class.string("Name");
        let by_class = match root.iter_mut().find(|(name, _)| *name == class_name) {
            Some((_, sets)) => sets,
            None => {
                root.push((class_name.clone(), Vec::new()));
                &mut root.last_mut().unwrap().1
            }
        };
        let key = base.tags.join(",");
        let set = match by_class.iter_mut().find(|(k, _)| *k == key) {
            Some((_, set)) => set,
            None => {
                by_class.push((key.clone(), TagSet::default()));
                &mut by_class.last_mut().unwrap().1
            }
        };
        set.bases.push(base.id.clone());
        set.absorb(by_domain.get(base.domain.as_str()).map(Vec::as_slice).unwrap_or_default(), &base.tags);
    }

    let fields = root
        .into_iter()
        .map(|(class_name, sets)| {
            let by_tags =
                sets.into_iter().map(|(tags, set)| (tags, set.to_json())).collect::<Vec<_>>();
            (class_name, J::Obj(by_tags))
        })
        .collect();
    ctx.write("mods_by_base", &J::Obj(fields))
}

/// One base-item tag set: which bases share it and what can roll on them.
#[derive(Default)]
struct TagSet {
    bases: Vec<String>,
    /// generation type -> mod group -> mod id -> required level
    mods: BTreeMap<String, BTreeMap<String, BTreeMap<String, i64>>>,
    /// Mods that only become available once another mod adds a tag.
    conditional: HashSet<String>,
}

impl TagSet {
    /// Adds every mod the tag set can roll. A mod that adds tags can unlock
    /// further mods, so the tag set grows until it stops changing — those
    /// second-order mods are recorded as conditional.
    fn absorb(&mut self, candidates: &[ModEntry], base_tags: &[String]) {
        let direct: HashSet<&str> = base_tags.iter().map(String::as_str).collect();
        let mut reachable = direct.clone();
        loop {
            let mut added = false;
            for entry in candidates {
                let weight = entry.weight_for(&direct);
                let conditional_weight = entry.weight_for(&reachable);
                if weight != conditional_weight {
                    self.conditional.insert(entry.id.clone());
                }
                // A tag matched at weight zero says the mod cannot roll here.
                let can_roll = |w: Option<i64>| matches!(w, Some(v) if v != 0);
                if !can_roll(weight) && !can_roll(conditional_weight) {
                    continue;
                }
                self.mods
                    .entry(entry.generation_type.clone())
                    .or_default()
                    .entry(entry.mod_type.clone())
                    .or_default()
                    .insert(entry.id.clone(), entry.level);
                for tag in &entry.adds_tags {
                    if reachable.insert(tag.as_str()) {
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
    }

    fn to_json(self) -> J {
        let mods = self
            .mods
            .into_iter()
            .map(|(generation, groups)| {
                let groups = groups
                    .into_iter()
                    .map(|(group, entries)| {
                        (group, J::Obj(entries.into_iter().map(|(id, lvl)| (id, int(lvl))).collect()))
                    })
                    .collect();
                (generation, J::Obj(groups))
            })
            .collect();
        let mut conditional: Vec<String> = self.conditional.into_iter().collect();
        conditional.sort();
        Obj::new()
            .set("bases", json::strings(&self.bases))
            .set("mods", J::Obj(mods))
            .or_null("conditional_mods", (!conditional.is_empty()).then(|| json::strings(&conditional)))
            .build()
    }
}

/// The parts of a mod `mods_by_base` needs, kept out of the JSON layer.
struct ModEntry {
    id: String,
    level: i64,
    generation_type: String,
    mod_type: String,
    adds_tags: Vec<String>,
    spawn_weights: Vec<(String, i64)>,
}

impl ModEntry {
    /// Weight of the first spawn tag the item actually has.
    fn weight_for(&self, tags: &HashSet<&str>) -> Option<i64> {
        self.spawn_weights.iter().find(|(tag, _)| tags.contains(tag.as_str())).map(|(_, w)| *w)
    }
}

fn mods_by_domain(ctx: &Ctx, table: &LoadedTable) -> HashMap<String, Vec<ModEntry>> {
    let mut out: HashMap<String, Vec<ModEntry>> = HashMap::new();
    for row in table.rows() {
        let id = row.id().to_string();
        if id.is_empty() {
            continue;
        }
        let weights = row.list_int("SpawnWeight_Values");
        let spawn_weights = ctx
            .rr
            .deref_list(row, "SpawnWeight_Tags")
            .into_iter()
            .enumerate()
            .map(|(i, tag)| (tag.id(), weights.get(i).copied().unwrap_or(0)))
            .collect();
        let domain = name_of(&DOMAINS, row.int("Domain")).unwrap_or("undefined").to_string();
        out.entry(domain).or_default().push(ModEntry {
            id,
            level: row.int("Level"),
            generation_type: name_of(&GENERATION_TYPES, row.int("GenerationType"))
                .unwrap_or("<unknown>")
                .to_string(),
            mod_type: ctx.rr.deref(row, "ModType").map(|m| m.row().string("Name")).unwrap_or_default(),
            adds_tags: ctx.rr.deref_list_ids(row, "Tags"),
            spawn_weights,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_and_generation_names_are_looked_up_by_value() {
        assert_eq!(name_of(&DOMAINS, 1), Some("item"));
        assert_eq!(name_of(&DOMAINS, 34), Some("tablet"));
        assert_eq!(name_of(&DOMAINS, 7), None);
        assert_eq!(name_of(&GENERATION_TYPES, 2), Some("suffix"));
        assert_eq!(translation_file(4), "chest_stat_descriptions");
        assert_eq!(translation_file(1), "stat_descriptions");
    }

    #[test]
    fn a_mod_only_counts_the_first_tag_the_item_has() {
        let entry = ModEntry {
            id: "m".into(),
            level: 1,
            generation_type: "prefix".into(),
            mod_type: "t".into(),
            adds_tags: Vec::new(),
            spawn_weights: vec![("ring".into(), 500), ("default".into(), 0)],
        };
        assert_eq!(entry.weight_for(&HashSet::from(["ring"])), Some(500));
        assert_eq!(entry.weight_for(&HashSet::from(["default"])), Some(0));
        assert_eq!(entry.weight_for(&HashSet::from(["amulet"])), None);
    }

    #[test]
    fn link_markup_is_reduced_to_what_the_client_draws() {
        assert_eq!(display_text("+(20-30)% to [Resistances|Fire Resistance]"), "+(20-30)% to Fire Resistance");
        assert_eq!(display_text("+(7-10)% to all [ElementalDamage|Elemental] [Resistances]"), "+(7-10)% to all Elemental Resistances");
        assert_eq!(display_text("+1 Prefix Modifier allowed\n-1 Suffix Modifier allowed"), "+1 Prefix Modifier allowed\n-1 Suffix Modifier allowed");
        // An unclosed bracket is left as it stands rather than swallowing the rest.
        assert_eq!(display_text("adds [Fire to attacks"), "adds [Fire to attacks");
    }
}
