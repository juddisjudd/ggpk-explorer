//! `base_items.json` (plus a file per item class), `uniques.json` and
//! `augments.json`.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use crate::dat::relational::{Ref, Row};
use std::collections::HashMap;

/// Items the game data still lists but that no player can obtain, or that
/// only ever exist with unique rarity. The client does not record this, so
/// the list is maintained by hand — as it is in RePoE.
const RELEASE_STATES: [(&str, &str); 14] = [
    ("Metadata/Items/Currency/CurrencyImprintOrb", "legacy"),
    ("Metadata/Items/Currency/CurrencyImprint", "legacy"),
    ("Metadata/Items/Currency/CurrencyIncursionCorrupt1", "unreleased"),
    ("Metadata/Items/Weapons/TwoHandWeapons/TwoHandSwords/TwoHandSwordDev", "unreleased"),
    ("Metadata/Items/Classic/MysteryLeaguestone", "unreleased"),
    ("Metadata/Items/Rings/RingDemigods1", "unique_only"),
    ("Metadata/Items/Belts/BeltDemigods1", "unique_only"),
    ("Metadata/Items/Armours/Shields/ShieldDemigods", "unique_only"),
    ("Metadata/Items/Armours/Helmets/HelmetWreath1", "unique_only"),
    ("Metadata/Items/Armours/Helmets/HelmetDemigods1", "unique_only"),
    ("Metadata/Items/Armours/BodyArmours/BodyDemigods1", "unique_only"),
    ("Metadata/Items/Armours/Boots/BootsDemigods1", "unique_only"),
    ("Metadata/Items/Armours/Gloves/GlovesDemigods1", "unique_only"),
    ("Metadata/Items/Jewels/JewelTimeless", "unique_only"),
];

/// The parts of a base item `mods_by_base` needs.
pub struct BaseItem {
    pub id: String,
    pub item_class: String,
    pub tags: Vec<String>,
    pub domain: String,
}

pub fn collect_bases(ctx: &Ctx) -> Result<Vec<BaseItem>, String> {
    let table = ctx.table("BaseItemTypes")?;
    let mut inherited = InheritedTags::default();
    Ok(table
        .rows()
        .filter(|row| !row.id().is_empty())
        .map(|row| BaseItem {
            id: row.id().to_string(),
            item_class: ctx.rr.deref_id(row, "ItemClass").unwrap_or_default(),
            tags: tags_of(ctx, row, &mut inherited),
            domain: domain_of(row),
        })
        .collect())
}

fn domain_of(row: Row<'_>) -> String {
    let domain = row.int("ModDomain");
    // 38 (mods_disallowed) means the item takes no mods at all.
    match super::mods::domain_name(domain) {
        Some(name) if domain != 38 => name.to_string(),
        _ => "undefined".to_string(),
    }
}

fn tags_of(ctx: &Ctx, row: Row<'_>, inherited: &mut InheritedTags) -> Vec<String> {
    let mut tags = ctx.rr.deref_list_ids(row, "Tags");
    tags.extend(inherited.for_path(ctx, row.str("InheritsFrom")));
    tags
}

/// Tags an item picks up from the `.it` file it inherits from, following the
/// `extends` chain. Cached per path — thousands of items share a few dozen.
#[derive(Default)]
struct InheritedTags {
    cache: HashMap<String, Vec<String>>,
}

impl InheritedTags {
    fn for_path(&mut self, ctx: &Ctx, path: &str) -> Vec<String> {
        if path.is_empty() {
            return Vec::new();
        }
        if let Some(hit) = self.cache.get(path) {
            return hit.clone();
        }
        let mut tags = Vec::new();
        let mut current = format!("{}.it", path);
        let mut seen = std::collections::HashSet::new();
        while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
            let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, &current) else { break };
            let file = crate::parsers::object_dsl::parse(&crate::parsers::utils::decode_text_lossy(&bytes));
            if let Some(base) = file.components.iter().find(|c| c.name == "Base") {
                tags.extend(base.props.iter().filter(|p| p.key == "tag").map(|p| p.value.clone()));
            }
            current = match file.extends {
                Some(next) => crate::parsers::object_dsl::resolve_extends(&next, &current),
                None => String::new(),
            };
        }
        self.cache.insert(path.to_string(), tags.clone());
        tags
    }
}

pub fn base_items(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("BaseItemTypes")?;
    let armour = by_base(ctx, "ArmourTypes");
    let shield = by_base(ctx, "ShieldTypes");
    let flask = by_base(ctx, "Flasks");
    let charges = by_base(ctx, "ComponentCharges");
    let weapon = by_base(ctx, "WeaponTypes");
    let currency = by_base(ctx, "CurrencyItems");
    let requirements = by_base(ctx, "AttributeRequirements");
    let skills = inherent_skills(ctx);
    let states: HashMap<&str, &str> = RELEASE_STATES.iter().copied().collect();

    let mut inherited = InheritedTags::default();
    let mut root: Vec<(String, J)> = Vec::new();
    let mut by_class: Vec<(String, Vec<(String, J)>)> = Vec::new();

    for row in table.rows() {
        let id = row.id().to_string();
        if id.is_empty() {
            continue;
        }
        let item_class = ctx.rr.deref_id(row, "ItemClass").unwrap_or_default();
        let visual = ctx.rr.deref(row, "ItemVisualIdentity");
        if let Some(visual) = &visual {
            export_art(ctx, visual.row());
        }
        let visual_identity = Obj::new()
            .or_null("dds_file", visual.as_ref().and_then(|v| json::opt_text(v.row().str("DDSFile"))))
            .or_null("id", visual.as_ref().and_then(|v| json::opt_text(v.row().id())))
            .build();

        let drop_level = row.int("DropLevel");
        let requirements = requirements.get(&row.index).map(|&i| {
            let r = table_row(ctx, "AttributeRequirements", i);
            Obj::new()
                .set("dexterity", int(r.as_ref().map(|r| r.row().int("ReqDex")).unwrap_or(0)))
                .set("intelligence", int(r.as_ref().map(|r| r.row().int("ReqInt")).unwrap_or(0)))
                .set("level", int(drop_level))
                .set("strength", int(r.as_ref().map(|r| r.row().int("ReqStr")).unwrap_or(0)))
                .build()
        });

        let entry = Obj::new()
            .set("domain", text(domain_of(row)))
            .set("drop_level", int(drop_level))
            .set("implicits", json::strings(ctx.rr.deref_list_ids(row, "Implicit_Mods")))
            .set("inventory_height", int(row.int("Height")))
            .set("inventory_width", int(row.int("Width")))
            .set("inherits_from", text(row.str("InheritsFrom")))
            .set("item_class", text(&item_class))
            .set("name", text(row.str("Name")))
            .set(
                "properties",
                properties(ctx, row, &armour, &shield, &flask, &charges, &weapon, &currency),
            )
            .set("release_state", text(states.get(id.as_str()).copied().unwrap_or("released")))
            .set("tags", json::strings(tags_of(ctx, row, &mut inherited)))
            .set("visual_identity", visual_identity)
            .or_null("requirements", requirements)
            .or_null("grants_buff", None)
            .or_null(
                "skills_granted",
                skills.get(&row.index).map(|ids| json::strings(ids)),
            )
            .build();

        if !item_class.is_empty() {
            match by_class.iter_mut().find(|(name, _)| *name == item_class) {
                Some((_, items)) => items.push((id.clone(), entry.clone())),
                None => by_class.push((item_class.clone(), vec![(id.clone(), entry.clone())])),
            }
        }
        root.push((id, entry));
    }

    json::write(ctx.out, "base_items", &J::Obj(root))?;
    for (class, items) in by_class {
        json::write(ctx.out, &format!("base_items/{}", class), &J::Obj(items))?;
    }
    Ok(())
}

/// The full property block. Every key is present so consumers can rely on the
/// shape; the ones that do not apply to an item are null.
#[allow(clippy::too_many_arguments)]
fn properties(
    ctx: &Ctx,
    row: Row<'_>,
    armour: &HashMap<usize, usize>,
    shield: &HashMap<usize, usize>,
    flask: &HashMap<usize, usize>,
    charges: &HashMap<usize, usize>,
    weapon: &HashMap<usize, usize>,
    currency: &HashMap<usize, usize>,
) -> J {
    let armour = armour.get(&row.index).and_then(|&i| table_row(ctx, "ArmourTypes", i));
    let shield = shield.get(&row.index).and_then(|&i| table_row(ctx, "ShieldTypes", i));
    let flask = flask.get(&row.index).and_then(|&i| table_row(ctx, "Flasks", i));
    let charges = charges.get(&row.index).and_then(|&i| table_row(ctx, "ComponentCharges", i));
    let weapon = weapon.get(&row.index).and_then(|&i| table_row(ctx, "WeaponTypes", i));
    let currency = currency.get(&row.index).and_then(|&i| table_row(ctx, "CurrencyItems", i));

    // A defence only shows when it is non-zero, and always as a range.
    let defence = |column: &str| {
        armour.as_ref().map(|a| a.row().int(column)).filter(|v| *v > 0).map(|v| {
            Obj::new().set("max", int(v)).set("min", int(v)).build()
        })
    };
    let positive = |source: &Option<Ref>, column: &str| {
        source.as_ref().map(|r| r.row().int(column)).filter(|v| *v > 0).map(int)
    };
    let non_zero = |source: &Option<Ref>, column: &str| {
        source.as_ref().map(|r| r.row().int(column)).filter(|v| *v != 0).map(int)
    };
    let any = |source: &Option<Ref>, column: &str| {
        source.as_ref().map(|r| int(r.row().int(column)))
    };
    // An item that has the row reports the column even when it is blank; only
    // an item without the row at all reports nothing.
    let string = |source: &Option<Ref>, column: &str| {
        source.as_ref().map(|r| text(r.row().str(column)))
    };

    Obj::new()
        .or_null("armour", defence("Armour"))
        .or_null("energy_shield", defence("EnergyShield"))
        .or_null("evasion", defence("Evasion"))
        // ArmourTypes still has a Ward column, but nothing in PoE 2 reads it
        // and the values left in it are stale, so it stays unreported.
        .or_null("ward", None)
        .or_null("movement_speed", non_zero(&armour, "IncreasedMovementSpeed"))
        .or_null("block", any(&shield, "Block"))
        .or_null("description", string(&currency, "Description"))
        .or_null("directions", string(&currency, "Directions"))
        .or_null("stack_size", any(&currency, "StackSize"))
        .or_null("stack_size_currency_tab", any(&currency, "CurrencyTab_StackSize"))
        .or_null(
            "full_stack_turns_into",
            currency
                .as_ref()
                .and_then(|c| ctx.rr.deref_id(c.row(), "FullStack_BaseItemType"))
                .map(text),
        )
        .or_null("charges_max", any(&charges, "MaxCharges"))
        .or_null("charges_per_use", any(&charges, "PerCharge"))
        .or_null("duration", positive(&flask, "RecoveryTime"))
        .or_null("life_per_use", positive(&flask, "LifePerUse"))
        .or_null("mana_per_use", positive(&flask, "ManaPerUse"))
        .or_null("attack_time", any(&weapon, "Speed"))
        .or_null("critical_strike_chance", any(&weapon, "CritChance"))
        .or_null("physical_damage_max", any(&weapon, "DamageMax"))
        .or_null("physical_damage_min", any(&weapon, "DamageMin"))
        .or_null("range", any(&weapon, "RangeMax"))
        .or_null("mana_burn_ms", None)
        .or_null("cooldown_ms", None)
        .or_null("monster_id", None)
        .or_null("monster_ability_text", None)
        .or_null("monster_category", None)
        .build()
}

fn table_row(ctx: &Ctx, table: &str, index: usize) -> Option<Ref> {
    let table = ctx.rr.table(table)?;
    (index < table.len()).then_some(Ref { table, index })
}

/// Writes an item's inventory art. Flask art is a three-panel sheet the
/// client stacks, which `Composition` 1 marks.
fn export_art(ctx: &Ctx, visual: Row<'_>) {
    let compose = (visual.int("Composition") == 1).then_some(super::images::Compose::Flask);
    super::images::export(ctx, visual.str("DDSFile"), compose);
}

/// Rows of a side table keyed by the base item row they describe. Some of
/// these tables point at the item by row and others by its id string, so both
/// are resolved back to the row.
fn by_base(ctx: &Ctx, name: &str) -> HashMap<usize, usize> {
    let Some(table) = ctx.rr.table(name) else { return HashMap::new() };
    let Some(column) = table.pick(&["BaseItemType", "BaseItemTypesKey"]) else { return HashMap::new() };
    let bases = ctx.rr.table("BaseItemTypes");
    let mut out = HashMap::new();
    for row in table.rows() {
        let base = row.key(column).or_else(|| {
            let id = row.str(column);
            (!id.is_empty()).then(|| bases.as_ref()?.by_id(id).map(|b| b.index)).flatten()
        });
        if let Some(base) = base {
            out.entry(base).or_insert(row.index);
        }
    }
    out
}

/// Skills an item grants just by being equipped, named by their gem's base item.
fn inherent_skills(ctx: &Ctx) -> HashMap<usize, Vec<String>> {
    let Some(table) = ctx.rr.table("ItemInherentSkills") else { return HashMap::new() };
    let mut out = HashMap::new();
    for row in table.rows() {
        let Some(base) = row.key("BaseItemType") else { continue };
        let granted: Vec<String> = ctx
            .rr
            .deref_list(row, "SkillsGranted")
            .into_iter()
            .filter_map(|gem| ctx.rr.deref_id(gem.row(), "BaseItemType"))
            .collect();
        if !granted.is_empty() {
            out.insert(base, granted);
        }
    }
    out
}

pub fn uniques(ctx: &Ctx) -> Result<(), String> {
    let layout = ctx.table("UniqueStashLayout")?;
    let mut rows: Vec<(String, String, Row<'_>)> = layout
        .rows()
        .filter_map(|row| {
            let stash = ctx.rr.deref(row, "UniqueStashTypesKey")?;
            let word = ctx.rr.deref(row, "WordsKey")?;
            Some((stash.row().string("Name"), word.row().string("Text2"), row))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // The renamed/base links point at other layout rows, named by their word.
    let name_of = |index: usize| -> Option<J> {
        let row = layout.row(index)?;
        let word = ctx.rr.deref(row, "WordsKey")?;
        Some(Obj::new().set("rowid", int(index as i64)).set("name", text(word.row().str("Text2"))).build())
    };

    let root = rows
        .into_iter()
        .map(|(_, name, row)| {
            let stash = ctx.rr.deref(row, "UniqueStashTypesKey");
            let word = ctx.rr.deref(row, "WordsKey");
            let visual = ctx.rr.deref(row, "ItemVisualIdentityKey");
            if let Some(visual) = &visual {
                export_art(ctx, visual.row());
            }
            let fallback = |column: &str| stash.as_ref().map(|s| s.row().int(column)).unwrap_or(0);
            let size = |override_column: &str, column: &str| {
                let v = row.int(override_column);
                int(if v != 0 { v } else { fallback(column) })
            };
            let entry = Obj::new()
                .set("id", text(word.as_ref().map(|w| w.row().string("Text")).unwrap_or_default()))
                .set("inventory_height", size("OverrideHeight", "Height"))
                .set("inventory_width", size("OverrideWidth", "Width"))
                .set("is_alternate_art", J::Bool(row.bool("IsAlternateArt")))
                .set("item_class", text(stash.as_ref().map(|s| s.row().string("Id")).unwrap_or_default()))
                .set("name", text(&name))
                .set(
                    "visual_identity",
                    Obj::new()
                        .or_null("dds_file", visual.as_ref().and_then(|v| json::opt_text(v.row().str("DDSFile"))))
                        .or_null("id", visual.as_ref().and_then(|v| json::opt_text(v.row().id())))
                        .build(),
                )
                .or_null("renamed_version", row.key("RenamedVersion").and_then(name_of))
                .or_null("base_version", row.key("BaseVersion").and_then(name_of))
                .build();
            (row.index.to_string(), entry)
        })
        .collect();

    // The stash layout misses some unique art, so anything whose file name
    // says "unique" is written too — RePoE lists those separately.
    if let Some(visuals) = ctx.rr.table("ItemVisualIdentity") {
        for visual in visuals.rows() {
            if visual.str("DDSFile").to_ascii_lowercase().contains("unique") {
                export_art(ctx, visual);
            }
        }
    }

    json::write(ctx.out, "uniques", &J::Obj(root))
}

pub fn augments(ctx: &Ctx) -> Result<(), String> {
    let cores = ctx.table("SoulCores")?;
    let stats = ctx.table("SoulCoreStats")?;
    let translations = ctx.translations("stat_descriptions");

    let mut by_core: HashMap<usize, Vec<Row<'_>>> = HashMap::new();
    for row in stats.rows() {
        if let Some(core) = row.key("SoulCore") {
            by_core.entry(core).or_default().push(row);
        }
    }

    let mut root: Vec<(String, J)> = Vec::new();
    for core in cores.rows() {
        let Some(base) = ctx.rr.deref_id(core, "BaseItemType") else { continue };
        let kind = ctx.rr.deref(core, "Type");
        let limit = ctx.rr.deref(core, "Limit").map(|l| {
            let row = l.row();
            let value = row.int("Limit");
            match row.opt_str("Text") {
                // The limit text carries a single `{0}`-style slot for the count.
                Some(template) => text(template.replacen("{0}", &value.to_string(), 1)),
                None => text(value.to_string()),
            }
        });

        let mut categories: Vec<(String, J)> = Vec::new();
        for stat_row in by_core.get(&core.index).map(Vec::as_slice).unwrap_or_default() {
            let Some(category) = ctx.rr.deref(*stat_row, "StatCategory") else { continue };
            let category_row = category.row();
            let target = match category_row.opt_str("Display") {
                Some(display) => text(display),
                None => json::strings(
                    ctx.rr
                        .deref_list(category_row, "TargetItemClasses")
                        .iter()
                        .map(|c| c.row().string("Name"))
                        .collect::<Vec<_>>(),
                ),
            };
            let mut entry = Obj::new().set("target", target);
            for (list, values, stats_key, text_key) in [
                ("Stats", "StatsValues", "stats", "stat_text"),
                ("BondedStats", "BondedStatsValues", "bonded_stats", "bonded_stat_text"),
            ] {
                let refs = ctx.rr.deref_list(*stat_row, list);
                if refs.is_empty() {
                    continue;
                }
                let ids: Vec<String> = refs.iter().map(|s| s.id()).collect();
                let numbers: Vec<i32> = stat_row.list_int(values).iter().map(|v| *v as i32).collect();
                let ranges: Vec<(i32, i32)> = numbers.iter().map(|&v| (v, v)).collect();
                let described = translations.translate_ranges(&ids, &ranges);
                entry = entry.set(
                    stats_key,
                    J::Arr(
                        refs.iter()
                            .map(|s| {
                                Obj::new()
                                    .set("id", text(s.id()))
                                    .set("local", J::Bool(s.row().bool("IsLocal")))
                                    .build()
                            })
                            .collect(),
                    ),
                );
                if !described.is_empty() {
                    entry = entry.set(text_key, json::strings(&described));
                }
            }
            categories.push((category_row.id().to_string(), json::sorted(entry.build())));
        }
        categories.sort_by(|a, b| a.0.cmp(&b.0));

        let entry = Obj::new()
            .set("categories", J::Obj(categories))
            .opt("limit", limit)
            .opt("required_level", Some(core.int("RequiredLevel")).filter(|v| *v != 0).map(int))
            .opt("type_id", kind.as_ref().and_then(|k| json::opt_text(k.row().id())))
            .opt("type_name", kind.as_ref().and_then(|k| json::opt_text(k.row().str("Name"))))
            .build();
        root.push((base, json::sorted(entry)));
    }
    root.sort_by(|a, b| a.0.cmp(&b.0));

    json::write(ctx.out, "augments", &J::Obj(root))
}

