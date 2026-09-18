//! Path of Exile 1 dumps with no PoE 2 counterpart: cluster jewels, the
//! crafting bench, tattoos, the pantheon and crucible weapon passives.

use crate::dat::relational::Row;
use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;

fn ints(values: Vec<i64>) -> J {
    J::Arr(values.into_iter().map(int).collect())
}

/// Stat ids and values rendered through one description file.
fn stat_block(ctx: &Ctx, ids: &[String], values: &[i64], file: &str) -> (J, J) {
    let pairs: Vec<(String, i64)> = ids.iter().cloned().zip(values.iter().copied()).collect();
    let ranges: Vec<(i32, i32)> = pairs.iter().map(|(_, v)| (*v as i32, *v as i32)).collect();
    let stat_ids: Vec<String> = pairs.iter().map(|(id, _)| id.clone()).collect();
    let lines = ctx.translations(file).translate_ranges(&stat_ids, &ranges);
    (J::Obj(pairs.into_iter().map(|(id, v)| (id, int(v))).collect()), json::strings(lines))
}

/// `cluster_jewels.json`: each cluster jewel base, the small passives it can
/// add by tag, and the notables and keystones it can carry.
pub fn cluster_jewels(ctx: &Ctx) -> Result<(), String> {
    let jewels = ctx.table("PassiveTreeExpansionJewels")?;
    let skills = ctx.table("PassiveTreeExpansionSkills")?;
    let translations = ctx.translations("passive_skill_stat_descriptions");
    let size_column = jewels.require(&["PassiveTreeExpansionJewelSizesKey", "Size"])?;
    let skill_size = skills.require(&["PassiveTreeExpansionJewelSizesKey", "JewelSize"])?;

    let size_name = |row: Row<'_>, column: &str| {
        ctx.rr.deref(row, column).map(|s| text(s.row().str("Name"))).unwrap_or(J::Null)
    };

    let root = jewels
        .rows()
        .filter_map(|row| {
            let base = ctx.rr.deref(row, "BaseItemTypesKey")?;
            let size = row.key(size_column);
            let added = skills.rows().filter(|s| s.key(skill_size) == size).map(|s| {
                Obj::new()
                    .or_null("tag", ctx.rr.deref_id(s, "TagsKey").map(text))
                    .or_null(
                        "passive",
                        ctx.rr.deref(s, "PassiveSkillsKey").map(|p| super::passives::passive(ctx, p.row(), Some(&translations))),
                    )
                    .or_null(
                        "mastery",
                        ctx.rr.deref(s, "Mastery_PassiveSkillsKey").map(|p| super::passives::passive(ctx, p.row(), Some(&translations))),
                    )
                    .build()
            });
            let entry = Obj::new()
                .set("name", text(base.row().str("Name")))
                .set("size", size_name(row, size_column))
                .or_null("size_index", size.map(|s| int(s as i64)))
                .set("min_nodes", int(row.int("MinNodes")))
                .set("max_nodes", int(row.int("MaxNodes")))
                .set("small_indices", ints(row.list_int("SmallIndices")))
                .set("notable_indices", ints(row.list_int("NotableIndices")))
                .set("socket_indices", ints(row.list_int("SocketIndices")))
                .set("total_indices", int(row.int("TotalIndices")))
                .set("skills", json::arr(added))
                .build();
            Some((base.id(), entry))
        })
        .collect();

    let special = ctx
        .optional_table("PassiveTreeExpansionSpecialSkills")
        .map(|table| {
            json::arr(table.rows().filter_map(|row| {
                let passive = ctx.rr.deref(row, "PassiveSkillsKey")?;
                Some(
                    Obj::new()
                        .or_null("stat", ctx.rr.deref_id(row, "StatsKey").map(text))
                        .set("passive", super::passives::passive(ctx, passive.row(), Some(&translations)))
                        .build(),
                )
            }))
        })
        .unwrap_or(J::Null);

    let out = Obj::new().set("jewels", J::Obj(root)).set("special_skills", special).build();
    ctx.write("cluster_jewels", &out)
}

/// `crafting_bench_options.json`: every bench craft, in table order.
pub fn crafting_bench_options(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("CraftingBenchOptions")?;
    let options = table.rows().map(|row| {
        let categories = ctx.rr.deref_list(row, "CraftingItemClassCategories");
        let mut classes: Vec<String> = ctx.rr.deref_list_ids(row, "ItemClasses");
        for category in &categories {
            for class in ctx.rr.deref_list_ids(category.row(), "ItemClasses") {
                if !classes.contains(&class) {
                    classes.push(class);
                }
            }
        }
        let amounts = row.list_int("Cost_Values");
        let cost = ctx.rr.deref_list(row, "Cost_BaseItemTypes").into_iter().enumerate().map(|(i, item)| {
            Obj::new().set("item", text(item.id())).or_null("amount", amounts.get(i).map(|a| int(*a))).build()
        });
        Obj::new()
            .set("name", text(row.str("Name")))
            .set("order", int(row.int("Order")))
            .or_null("mod", ctx.rr.deref_id(row, "AddMod").map(text))
            .or_null("enchantment", ctx.rr.deref_id(row, "AddEnchantment").map(text))
            .or_null("mod_type", ctx.rr.deref(row, "ModType").map(|m| text(m.row().str("Name"))))
            .set("is_disabled", J::Bool(row.bool("IsDisabled")))
            .set("is_area_option", J::Bool(row.bool("IsAreaOption")))
            .set("required_level", int(row.int("RequiredLevel")))
            .set("tier", int(row.int("Tier")))
            .set("cost", json::arr(cost))
            .set("item_categories", json::strings(categories.iter().map(|c| c.id())))
            .set("item_classes", json::strings(classes))
            .set("sockets", int(row.int("Sockets")))
            .set("links", int(row.int("Links")))
            .or_null("socket_colours", json::opt_text(row.str("SocketColours")))
            .set("item_quantity", int(row.int("ItemQuantity")))
            .or_null("sort_category", ctx.rr.deref_id(row, "SortCategory").map(text))
            .or_null("description", json::opt_text(row.str("Description")))
            .build()
    });
    ctx.write("crafting_bench_options", &json::arr(options))
}

/// `tattoos.json`: every passive override a tattoo or runegraft applies, with
/// the tattoo items that carry it and the nodes they may target.
pub fn tattoos(ctx: &Ctx) -> Result<(), String> {
    let overrides = ctx.table("PassiveSkillOverrides")?;
    let exchange = ctx.optional_table("CurrencyExchange");
    let targets_by_override = {
        let mut map: std::collections::HashMap<usize, Vec<J>> = std::collections::HashMap::new();
        if let Some(table) = ctx.optional_table("PassiveSkillTattoos") {
            for row in table.rows() {
                let Some(target) = row.key("Override") else { continue };
                let set = ctx.rr.deref(row, "Set");
                let base = ctx.rr.deref(row, "Tattoo");
                let listing = base.as_ref().and_then(|b| exchange.as_ref()?.rows().find(|e| e.key("Item") == Some(b.index)));
                let entry = Obj::new()
                    .or_null("item", base.as_ref().map(|b| text(b.id())))
                    .or_null("item_name", base.as_ref().map(|b| text(b.row().str("Name"))))
                    .or_null("target_set", set.as_ref().map(|s| text(s.id())))
                    .or_null("target_name", set.as_ref().map(|s| text(s.row().str("Name"))))
                    .or_null("target_qualifier", set.as_ref().and_then(|s| json::opt_text(s.row().str("Qualifier"))))
                    .or_null("tribe", Some(int(row.int("Tribe"))))
                    .or_null(
                        "enabled_in_challenge_league",
                        listing.map(|e| J::Bool(e.bool("EnabledInChallengeLeague"))),
                    )
                    .build();
                map.entry(target).or_default().push(entry);
            }
        }
        map
    };

    let root = overrides
        .rows()
        .map(|row| {
            let ids = ctx.rr.deref_list_ids(row, "Stats");
            let (stats, stat_text) = stat_block(ctx, &ids, &row.list_int("StatValues"), "passive_skill_stat_descriptions");
            let entry = Obj::new()
                .set("name", text(row.str("Name")))
                .or_null("icon", json::opt_text(row.str("NodeIcon")))
                .or_null("background", json::opt_text(row.str("PassiveBG")))
                .or_null("type", ctx.rr.deref_id(row, "Type").map(text))
                .or_null("limit", ctx.rr.deref(row, "Limit").map(|l| text(l.row().str("Description"))))
                .set("requires_adjacent", int(row.int("RequiresAdjacent")))
                .set("max_adjacent", int(row.int("MaxAdjacent")))
                .or_null("allocated_passive", ctx.rr.deref_id(row, "AllocatedPassiveSkill").map(text))
                .set("tattoo_blocking_passives", json::strings(ctx.rr.deref_list_ids(row, "TattooBlockingPassive")))
                .set("stats", stats)
                .set("stat_text", stat_text)
                .set("tattoos", J::Arr(targets_by_override.get(&row.index).cloned().unwrap_or_default()))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();
    ctx.write("tattoos", &J::Obj(root))
}

/// `pantheons.json`: each god and the souls that upgrade it.
pub fn pantheons(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("PantheonPanelLayout")?;
    let root = table
        .rows()
        .map(|row| {
            let souls = (1..=4).filter_map(|n| {
                let name = row.opt_str(&format!("GodName{}", n))?;
                let ids = ctx.rr.deref_list_ids(row, &format!("Effect{}_StatsKeys", n));
                let (stats, stat_text) = stat_block(ctx, &ids, &row.list_int(&format!("Effect{}_Values", n)), "stat_descriptions");
                Some(Obj::new().set("name", text(name)).set("stats", stats).set("stat_text", stat_text).build())
            });
            let entry = Obj::new()
                .set("is_major_god", J::Bool(row.bool("IsMajorGod")))
                .set("is_disabled", J::Bool(row.bool("IsDisabled")))
                .set("souls", json::arr(souls))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();
    ctx.write("pantheons", &J::Obj(root))
}

/// `weapon_passive_skills.json`: the crucible passive tree nodes a weapon can
/// grow, keyed by mod since every tier of a node shares one `Id`.
pub fn weapon_passive_skills(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("WeaponPassiveSkills")?;
    let tier = table.require(&["Tier", "ModTier"])?;
    let root = table
        .rows()
        .filter_map(|row| {
            let tags = ctx.rr.deref_list(row, "Tags").iter().map(|t| t.row().string("Tag")).collect::<Vec<_>>();
            let entry = Obj::new()
                .set("id", text(row.id()))
                .set("tier", int(row.int(tier)))
                .or_null("type", ctx.rr.deref_id(row, "Type").map(text))
                .set("node_spawn_location", ints(row.list_int("NodeSpawnLocation")))
                .or_null("icon", json::opt_text(row.str("Icon")))
                .set("tags", json::strings(tags))
                .build();
            Some((ctx.rr.deref_id(row, "Mod")?, entry))
        })
        .collect();
    ctx.write("weapon_passive_skills", &J::Obj(root))
}
