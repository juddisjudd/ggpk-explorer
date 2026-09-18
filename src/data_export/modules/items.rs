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
            item_class: ctx.rr.deref_id(row, item_class_column(row)).unwrap_or_default(),
            tags: tags_of(ctx, row, &mut inherited),
            domain: domain_of(ctx, row),
        })
        .collect())
}

/// A base that takes no mods reports `undefined`: PoE 2 marks it
/// `mods_disallowed`, PoE 1 with a value its domain enum has no name for.
fn domain_of(ctx: &Ctx, row: Row<'_>) -> String {
    match super::mods::domain_label_in(ctx, row, "ModDomain") {
        Some(name) if name != "mods_disallowed" => name,
        _ => "undefined".to_string(),
    }
}

fn item_class_column(row: Row<'_>) -> &'static str {
    row.table.pick(&["ItemClass", "ItemClassesKey"]).unwrap_or("ItemClass")
}

fn tags_of(ctx: &Ctx, row: Row<'_>, inherited: &mut InheritedTags) -> Vec<String> {
    let mut tags = ctx.rr.deref_list_ids(row, row.table.pick(&["Tags", "TagsKeys"]).unwrap_or("Tags"));
    tags.extend(inherited.for_path(ctx, row.str("InheritsFrom")).tags);
    tags
}

/// What an item picks up from the `.it` file it inherits from, following the
/// `extends` chain. Cached per path — thousands of items share a few dozen.
#[derive(Default)]
struct InheritedTags {
    cache: HashMap<String, Inherited>,
}

#[derive(Clone, Default)]
struct Inherited {
    tags: Vec<String>,
    /// The nearest file in the chain that states one wins.
    max_quality: Option<i64>,
    /// Most sockets this item can roll, from the `Sockets` component.
    socket_limit: Option<i64>,
}

/// `socket_info` lists `sockets:item level:weight` for each count the item can
/// roll. A level of 9999 is the game's way of ruling a count out, so the limit
/// is the largest count a real item level can reach.
fn socket_limit(info: &str) -> Option<i64> {
    info.split_whitespace()
        .filter_map(|entry| {
            let mut parts = entry.split(':');
            let count: i64 = parts.next()?.trim().parse().ok()?;
            let level: i64 = parts.next()?.trim().parse().ok()?;
            (level < 9999).then_some(count)
        })
        .max()
}

impl InheritedTags {
    fn for_path(&mut self, ctx: &Ctx, path: &str) -> Inherited {
        if path.is_empty() {
            return Inherited::default();
        }
        if let Some(hit) = self.cache.get(path) {
            return hit.clone();
        }
        let mut found = Inherited::default();
        // A file's `remove_tag` drops what the files it extends add, not its own tags.
        let mut removed: Vec<String> = Vec::new();
        let mut current = format!("{}.it", path);
        let mut seen = std::collections::HashSet::new();
        while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
            let Some(bytes) = crate::dat::relational::FileSource::fetch(ctx.files, &current) else { break };
            let file = crate::parsers::object_dsl::parse(&crate::parsers::utils::decode_text_lossy(&bytes));
            if let Some(base) = file.components.iter().find(|c| c.name == "Base") {
                found.tags.extend(
                    base.props.iter().filter(|p| p.key == "tag" && !removed.contains(&p.value)).map(|p| p.value.clone()),
                );
                removed.extend(base.props.iter().filter(|p| p.key == "remove_tag").map(|p| p.value.clone()));
            }
            if found.socket_limit.is_none() {
                found.socket_limit = file
                    .components
                    .iter()
                    .filter(|c| c.name == "Sockets")
                    .flat_map(|c| &c.props)
                    .find(|p| p.key == "socket_info")
                    .and_then(|p| socket_limit(&p.value));
            }
            if found.max_quality.is_none() {
                found.max_quality = file
                    .components
                    .iter()
                    .filter(|c| c.name == "Quality")
                    .flat_map(|c| &c.props)
                    .find(|p| p.key == "max_quality")
                    .and_then(|p| p.value.trim().parse().ok());
            }
            current = match file.extends {
                Some(next) => crate::parsers::object_dsl::resolve_extends(&next, &current),
                None => String::new(),
            };
        }
        self.cache.insert(path.to_string(), found.clone());
        found
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
    let spirit = by_base(ctx, "ItemSpirit");
    // Tinctures are a PoE 1 item; PoE 2 has no such table to look for.
    let tincture = match ctx.rr.is_poe2 {
        true => HashMap::new(),
        false => by_base(ctx, "Tinctures"),
    };
    let requirement_table = ["AttributeRequirements", "ComponentAttributeRequirements"]
        .into_iter()
        .find(|name| ctx.rr.table(name).is_some())
        .unwrap_or("AttributeRequirements");
    let requirements = by_base(ctx, requirement_table);
    let skills = inherent_skills(ctx);
    let implicit_text = ctx.table("Mods").map(|mods| super::mods::ModText::new(&mods)).ok();
    let states: HashMap<&str, &str> = RELEASE_STATES.iter().copied().collect();

    let mut inherited = InheritedTags::default();
    let mut root: Vec<(String, J)> = Vec::new();
    let mut by_class: Vec<(String, Vec<(String, J)>)> = Vec::new();

    for row in table.rows() {
        let id = row.id().to_string();
        if id.is_empty() {
            continue;
        }
        let item_class = ctx.rr.deref_id(row, item_class_column(row)).unwrap_or_default();
        let visual = ctx.rr.deref(row, "ItemVisualIdentity");
        if let Some(visual) = &visual {
            export_art(ctx, visual.row());
        }
        let visual_identity = Obj::new()
            .or_null("dds_file", visual.as_ref().and_then(|v| json::opt_text(v.row().str("DDSFile"))))
            .or_null("id", visual.as_ref().and_then(|v| json::opt_text(v.row().id())))
            .build();

        let drop_level = row.int("DropLevel");
        let implicits = ctx.rr.deref_list(row, table.pick(&["Implicit_Mods", "Implicit_ModsKeys"]).unwrap_or("Implicit_Mods"));
        // A base a player can equip carries its drop level; anything else asks
        // only for what its implicit needs, which is four fifths of that mod's
        // own level.
        let equippable = armour.contains_key(&row.index) || weapon.contains_key(&row.index) || flask.contains_key(&row.index);
        let implicit_level = implicits.iter().map(|m| m.row().int("Level") * 4 / 5).max().unwrap_or(0);
        let required_level = match equippable {
            true => drop_level.max(implicit_level),
            false => implicit_level,
        };
        let requirements = requirements.get(&row.index).map(|&i| table_row(ctx, requirement_table, i)).or_else(|| {
            (!ctx.rr.is_poe2 && required_level > 1).then_some(None)
        });
        let requirements = requirements.map(|r| {
            Obj::new()
                .set("dexterity", int(r.as_ref().map(|r| r.row().int("ReqDex")).unwrap_or(0)))
                .set("intelligence", int(r.as_ref().map(|r| r.row().int("ReqInt")).unwrap_or(0)))
                .set("level", int(match ctx.rr.is_poe2 {
                    true => drop_level,
                    false => required_level.max(1),
                }))
                .set("strength", int(r.as_ref().map(|r| r.row().int("ReqStr")).unwrap_or(0)))
                .build()
        });

        let entry = Obj::new()
            .set("domain", text(domain_of(ctx, row)))
            .set("drop_level", int(drop_level))
            .set("implicits", json::strings(implicits.iter().map(|m| m.id()).collect::<Vec<_>>()))
            .set(
                "implicit_text",
                json::strings(
                    implicits
                        .iter()
                        .map(|m| {
                            let lines = implicit_text
                                .as_ref()
                                .map(|r| r.lines(ctx, m.row()))
                                .unwrap_or_default();
                            super::mods::display_text(&lines.join("\n"))
                        })
                        .collect::<Vec<_>>(),
                ),
            )
            .set("inventory_height", int(row.int("Height")))
            .set("inventory_width", int(row.int("Width")))
            .set("inherits_from", text(row.str("InheritsFrom")))
            .set("item_class", text(&item_class))
            .set("name", text(row.str("Name")))
            .set(
                "properties",
                properties(
                    ctx,
                    row,
                    &armour,
                    &shield,
                    &flask,
                    &charges,
                    &weapon,
                    &currency,
                    &spirit,
                    &tincture,
                    inherited.for_path(ctx, row.str("InheritsFrom")),
                ),
            )
            .set("release_state", text(states.get(id.as_str()).copied().unwrap_or("released")))
            .set("tags", json::strings(tags_of(ctx, row, &mut inherited)))
            .set("visual_identity", visual_identity)
            .or_null("requirements", requirements)
            .or_null(
                "grants_buff",
                flask.get(&row.index).and_then(|&i| table_row(ctx, "Flasks", i)).and_then(|f| flask_buff(ctx, f.row())),
            )
            .or_null(
                "skills_granted",
                skills.get(&row.index).map(|(ids, _)| json::strings(ids)),
            )
            .or_null(
                "skills_granted_no_reservation",
                skills.get(&row.index).and_then(|(_, flag)| flag.map(J::Bool)),
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

    ctx.write("base_items", &J::Obj(root))?;
    for (class, items) in by_class {
        ctx.write(&format!("base_items/{}", class), &J::Obj(items))?;
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
    spirit: &HashMap<usize, usize>,
    tincture: &HashMap<usize, usize>,
    inherited: Inherited,
) -> J {
    let armour = armour.get(&row.index).and_then(|&i| table_row(ctx, "ArmourTypes", i));
    let shield = shield.get(&row.index).and_then(|&i| table_row(ctx, "ShieldTypes", i));
    let flask = flask.get(&row.index).and_then(|&i| table_row(ctx, "Flasks", i));
    let charges = charges.get(&row.index).and_then(|&i| table_row(ctx, "ComponentCharges", i));
    let weapon = weapon.get(&row.index).and_then(|&i| table_row(ctx, "WeaponTypes", i));
    let currency = currency.get(&row.index).and_then(|&i| table_row(ctx, "CurrencyItems", i));
    let tincture = tincture.get(&row.index).and_then(|&i| table_row(ctx, "Tinctures", i));
    let tincture_column = |names: &[&str]| {
        tincture.as_ref().and_then(|t| t.table.pick(names).map(|column| int(t.row().int(column))))
    };
    let spirit = spirit.get(&row.index).and_then(|&i| table_row(ctx, "ItemSpirit", i));

    // A defence only shows when it is non-zero, and always as a range. PoE 1
    // stores the range itself; PoE 2 a single value.
    let defence = |column: &str| {
        armour.as_ref().and_then(|a| {
            let row = a.row();
            let (min, max) = match row.table.has_col(column) {
                true => (row.int(column), row.int(column)),
                false => (row.int(&format!("{}Min", column)), row.int(&format!("{}Max", column))),
            };
            (max > 0).then(|| Obj::new().set("max", int(max)).set("min", int(min)).build())
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
        .or_null("ward", defence("Ward"))
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
                .and_then(|c| {
                    let column = c.table.pick(&["FullStack_BaseItemType", "FullStack_BaseItemTypesKey"])?;
                    ctx.rr.deref_id(c.row(), column)
                })
                .map(text),
        )
        .or_null("charges_max", any(&charges, "MaxCharges"))
        .or_null("charges_per_use", any(&charges, "PerCharge"))
        .or_null("duration", positive(&flask, "RecoveryTime"))
        .or_null("life_per_use", positive(&flask, "LifePerUse"))
        .or_null("mana_per_use", positive(&flask, "ManaPerUse"))
        .or_null("attack_time", any(&weapon, "Speed"))
        .or_null(
            "critical_strike_chance",
            weapon.as_ref().and_then(|w| w.table.pick(&["CritChance", "Critical"])).and_then(|c| any(&weapon, c)),
        )
        .or_null("physical_damage_max", any(&weapon, "DamageMax"))
        .or_null("physical_damage_min", any(&weapon, "DamageMin"))
        .or_null("range", any(&weapon, "RangeMax"))
        .or_null("mana_burn_ms", tincture_column(&["DebuffInterval", "ManaBurn"]))
        .or_null("cooldown_ms", tincture_column(&["Cooldown", "CoolDown"]))
        .or_null("monster_id", None)
        .or_null("monster_ability_text", None)
        .or_null("monster_category", None)
        .or_null("reload_time", positive(&weapon, "ReloadTime"))
        .or_null(
            "spirit",
            spirit.as_ref().and_then(|s| s.table.pick(&["SpiritGranted", "Value"]).map(|column| int(s.row().int(column)))),
        )
        .or_null("max_quality", inherited.max_quality.map(int))
        .or_null("socket_limit", inherited.socket_limit.map(int))
        .build()
}

/// The buff a charm applies while active. Several buffs on one item share the
/// first one's id and pool their stats, as the client describes them together.
fn flask_buff(ctx: &Ctx, flask: Row<'_>) -> Option<J> {
    let mut ids: Vec<String> = Vec::new();
    let mut stats: Vec<(String, i64)> = Vec::new();
    // PoE 1 names one buff on the flask row, with its values beside it.
    if let Some(buff) = ctx.rr.deref(flask, "BuffDefinitionsKey") {
        ids.push(buff.id());
        let values = flask.list_int("BuffStatValues");
        for (stat, value) in ctx.rr.deref_list(buff.row(), "StatsKeys").iter().zip(values) {
            stats.push((stat.id(), value));
        }
        for flag in ctx.rr.deref_list(buff.row(), "GrantedFlags") {
            stats.push((flag.id(), 1));
        }
    }
    for utility in ctx.rr.deref_list(flask, "UtilityBuff") {
        let utility = utility.row();
        let Some(buff) = ctx.rr.deref(utility, "BuffDefinition") else { continue };
        ids.push(buff.id());
        let values = utility.list_int("StatValues");
        for (stat, value) in ctx.rr.deref_list(buff.row(), "GrantedStats").iter().zip(values) {
            stats.push((stat.id(), value));
        }
        for flag in ctx.rr.deref_list(buff.row(), "GrantedFlags") {
            stats.push((flag.id(), 1));
        }
    }
    let id = ids.first()?;
    let stat_ids: Vec<String> = stats.iter().map(|(id, _)| id.clone()).collect();
    let ranges: Vec<(i32, i32)> = stats.iter().map(|(_, v)| (*v as i32, *v as i32)).collect();
    let lines = ctx.translations("stat_descriptions").translate_ranges(&stat_ids, &ranges);
    Some(
        Obj::new()
            .set("id", text(id))
            .set("stats", J::Obj(stats.into_iter().map(|(id, v)| (id, int(v))).collect()))
            .set("stat_text", json::strings(lines.iter().map(|l| super::mods::display_text(l))))
            .build(),
    )
}

fn table_row(ctx: &Ctx, table: &str, index: usize) -> Option<Ref> {
    let table = ctx.optional_table(table)?;
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
    let Some(table) = ctx.optional_table(name) else { return HashMap::new() };
    let Some(column) = table.pick(&["BaseItemType", "BaseItemTypesKey", "BaseItem"]) else { return HashMap::new() };
    let bases = ctx.optional_table("BaseItemTypes");
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
/// The flag says the granted skills reserve nothing; dat-schema names that column `IsWeapon`.
fn inherent_skills(ctx: &Ctx) -> HashMap<usize, (Vec<String>, Option<bool>)> {
    let Some(table) = ctx.optional_table("ItemInherentSkills") else { return HashMap::new() };
    let no_reservation = table.pick(&["NoReservation", "IsWeapon"]);
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
            out.insert(base, (granted, no_reservation.map(|c| row.bool(c))));
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
    if let Some(visuals) = ctx.optional_table("ItemVisualIdentity") {
        for visual in visuals.rows() {
            if visual.str("DDSFile").to_ascii_lowercase().contains("unique") {
                export_art(ctx, visual);
            }
        }
    }

    ctx.write("uniques", &J::Obj(root))
}

/// An art id repeats its flavour id with trailing underscores, and a unique
/// drawn several ways suffixes a letter — `FourUniqueRing33_a`. Every form
/// names the one flavour entry.
fn flavour_key(id: &str) -> String {
    let trimmed = id.trim_end_matches('_');
    let bytes = trimmed.as_bytes();
    let stripped = match bytes.len() >= 2 {
        true if bytes[bytes.len() - 2] == b'_' && bytes[bytes.len() - 1].is_ascii_lowercase() => {
            &trimmed[..trimmed.len() - 2]
        }
        _ => trimmed,
    };
    stripped.trim_end_matches('_').to_string()
}

/// `unique_details.json` — what the client files say about a unique beyond its
/// stash entry: flavour text, vendor price, where it came from, and the limit
/// on wearing several at once. Keyed by the same row ids as `uniques.json`.
///
/// The mods a unique rolls are not here, because the game files do not say.
/// They sit in `Mods.dat` under `generation_type: "unique"`, but nothing binds
/// one to the item that carries it — see `docs/Data-Export.md`.
pub fn unique_details(ctx: &Ctx) -> Result<(), String> {
    let layout = ctx.table("UniqueStashLayout")?;

    // One name can own several layout rows — Grip of Kulemak has five, one per
    // art variant — so anything keyed by name feeds all of them.
    let mut by_word: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in layout.rows() {
        if let Some(word) = row.key("WordsKey") {
            by_word.entry(word).or_default().push(row.index);
        }
    }
    let spread = |table: &str, key: &str, read: &dyn Fn(Row<'_>) -> Option<J>| {
        let mut out: HashMap<usize, J> = HashMap::new();
        let Some(table) = ctx.optional_table(table) else { return out };
        for row in table.rows() {
            let (Some(word), Some(value)) = (row.key(key), read(row)) else { continue };
            for &entry in by_word.get(&word).into_iter().flatten() {
                out.insert(entry, value.clone());
            }
        }
        out
    };

    let price = spread("UniqueGoldPrices", "Name", &|row| {
        // Most rows are priced; an unpriced one says nothing, so it is dropped.
        let value = row.int("Price");
        (value > 0).then(|| int(value))
    });
    let origin = spread("UniqueOrigins", "Unique", &|row| {
        ctx.rr.deref(row, "Origin").and_then(|o| json::opt_text(o.row().id()))
    });
    let jewel_limit = spread("UniqueJewelLimits", "JewelName", &|row| Some(int(row.int("Limit"))));
    let flavour = flavour_by_art(ctx);
    let legacies = mages_legacies(ctx);

    let root = layout
        .rows()
        .map(|row| {
            let name = ctx.rr.deref(row, "WordsKey").map(|w| w.row().string("Text2")).unwrap_or_default();
            let art = ctx.rr.deref(row, "ItemVisualIdentityKey").map(|v| flavour_key(v.row().id()));
            let entry = Obj::new()
                .set("name", text(&name))
                .set(
                    "item_class",
                    text(ctx.rr.deref(row, "UniqueStashTypesKey").map(|s| s.row().string("Id")).unwrap_or_default()),
                )
                .or_null("flavour_text", art.and_then(|key| flavour.get(&key).cloned()).map(text))
                .or_null("gold_price", price.get(&row.index).cloned())
                .or_null("origin", origin.get(&row.index).cloned())
                .or_null("jewel_limit", jewel_limit.get(&row.index).cloned())
                .or_null("mages_legacies", legacies.as_ref().filter(|_| name == MAGES_LEGACY_ITEM).cloned())
                .build();
            (row.index.to_string(), entry)
        })
        .collect();

    ctx.write("unique_details", &J::Obj(root))
}

/// Flavour text under the art id that names it, with anything two entries
/// disagree on left out rather than guessed at.
fn flavour_by_art(ctx: &Ctx) -> HashMap<String, String> {
    let Some(table) = ctx.optional_table("FlavourText") else { return HashMap::new() };
    let mut out: HashMap<String, Option<String>> = HashMap::new();
    for row in table.rows() {
        // The game writes these with CRLF; `flavour.json` keeps them as found,
        // but nothing downstream of here wants the carriage returns.
        let body = row.str("Text").replace("\r\n", "\n");
        if body.is_empty() {
            continue; // Tabula Rasa, fittingly, carries no line at all.
        }
        out.entry(flavour_key(row.id()))
            .and_modify(|held| {
                if held.as_deref() != Some(body.as_str()) {
                    *held = None;
                }
            })
            .or_insert(Some(body));
    }
    out.into_iter().filter_map(|(k, v)| Some((k, v?))).collect()
}

/// The unique whose mods read `All Mage's Legacies…`. `UniqueMagesLegacy`
/// names no item, so the one item that uses it is named here.
const MAGES_LEGACY_ITEM: &str = "Mageblood";

/// The flask legacies Mageblood grants, each with the line it draws.
fn mages_legacies(ctx: &Ctx) -> Option<J> {
    let table = ctx.optional_table("UniqueMagesLegacy")?;
    let lookup = ctx.translations("stat_descriptions");
    let entries: Vec<J> = table
        .rows()
        .map(|row| {
            let ids = ctx.rr.deref_list_ids(row, "Stats");
            let values: Vec<i32> = row.list_int("StatValues").into_iter().map(|v| v as i32).collect();
            let lines = lookup.translate_grouped(&ids, &values);
            Obj::new()
                .set("id", text(row.str("Name")))
                .set("name", text(super::mods::display_text(row.str("DisplayText"))))
                .set("stats", json::strings(ids))
                .set("stat_values", J::Arr(values.iter().map(|v| int(*v as i64)).collect()))
                .set("text", text(super::mods::display_text(&lines.join("\n"))))
                .build()
        })
        .collect();
    (!entries.is_empty()).then(|| J::Arr(entries))
}

pub fn augments(ctx: &Ctx) -> Result<(), String> {
    let cores = ctx.table("SoulCores")?;
    let stats = ctx.table("SoulCoreStats")?;
    let values_column = stats.require(&["StatsValues", "StatValue"])?;
    let bonded_values_column = stats.require(&["BondedStatsValues", "BondedValues"])?;
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
                Some(display) => json::strings([display]),
                None => json::strings(
                    ctx.rr
                        .deref_list(category_row, "TargetItemClasses")
                        .iter()
                        .map(|c| c.row().string("Name"))
                        .collect::<Vec<_>>(),
                ),
            };
            let mut entry = Obj::new()
                .set("target", target)
                .set("target_item_classes", json::strings(ctx.rr.deref_list_ids(category_row, "TargetItemClasses")));
            for (list, values, stats_key, text_key) in [
                ("Stats", values_column, "stats", "stat_text"),
                ("BondedStats", bonded_values_column, "bonded_stats", "bonded_stat_text"),
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
                            .enumerate()
                            .map(|(i, s)| {
                                Obj::new()
                                    .set("id", text(s.id()))
                                    .set("local", J::Bool(s.row().bool("IsLocal")))
                                    .opt("value", numbers.get(i).map(|v| int(*v)))
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
            .opt("limit_id", ctx.rr.deref_id(core, "Limit").map(text))
            .opt("limit_count", ctx.rr.deref(core, "Limit").map(|l| int(l.row().int("Limit"))))
            .set("is_socket_bound", J::Bool(core.bool("IsSocketBound")))
            .set("can_socket_in_martial_artist_slots", J::Bool(core.bool("CanSocketInMartialArtistSlots")))
            .set("can_socket_in_unique_items", J::Bool(core.bool("CanSocketInUniqueItems")))
            .set("can_socket_in_jewellery", J::Bool(core.bool("CanSocketInJewellery")))
            .set("can_socket_in_corrupted_sanctified", J::Bool(core.bool("CanSocketInCorruptedSanctified")))
            .build();
        root.push((base, json::sorted(entry)));
    }
    root.sort_by(|a, b| a.0.cmp(&b.0));

    ctx.write("augments", &J::Obj(root))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_ids_reduce_to_the_flavour_entry_they_name() {
        // The plain form, and the same id padded with underscores.
        assert_eq!(flavour_key("FourUniqueAmulet15"), "FourUniqueAmulet15");
        assert_eq!(flavour_key("FourUniqueAmulet15_"), "FourUniqueAmulet15");
        assert_eq!(flavour_key("FourUniqueBodyDex1___"), "FourUniqueBodyDex1");
        // One unique drawn several ways suffixes a letter.
        assert_eq!(flavour_key("FourUniqueRing33_a"), "FourUniqueRing33");
        assert_eq!(flavour_key("FourUniqueRing33__e"), "FourUniqueRing33");
        // A letter that is part of the name is not a variant suffix.
        assert_eq!(flavour_key("FourUniqueBreach4b"), "FourUniqueBreach4b");
    }

    #[test]
    fn a_socket_count_no_item_level_reaches_is_not_the_limit() {
        // A body armour reaches six; a helmet's last two counts are ruled out.
        assert_eq!(socket_limit("1:1:50 2:1:120 3:2:100 4:25:30 5:35:5 6:50:1"), Some(6));
        assert_eq!(socket_limit("1:1:100 2:1:90 3:2:80 4:25:30 5:9999:20 6:9999:5"), Some(4));
        assert_eq!(socket_limit(""), None);
    }
}
