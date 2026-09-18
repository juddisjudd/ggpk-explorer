//! `essences.json` and `liquid_emotions.json`: what a crafting currency puts on
//! an item, by the kind of item it is used on.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use std::collections::HashMap;

/// PoE 1 essence columns holding the mod each essence rolls on one item class.
const POE1_ESSENCE_CLASSES: [(&str, &str); 21] = [
    ("Helmet_ModsKey", "Helmet"),
    ("BodyArmour_ModsKey", "Body Armour"),
    ("Boots_ModsKey", "Boots"),
    ("Gloves_ModsKey", "Gloves"),
    ("Bow_ModsKey", "Bow"),
    ("Wand_ModsKey", "Wand"),
    ("Staff_ModsKey", "Staff"),
    ("TwoHandSword_ModsKey", "Two Hand Sword"),
    ("TwoHandAxe_ModsKey", "Two Hand Axe"),
    ("TwoHandMace_ModsKey", "Two Hand Mace"),
    ("Claw_ModsKey", "Claw"),
    ("Dagger_ModsKey", "Dagger"),
    ("OneHandSword_ModsKey", "One Hand Sword"),
    ("OneHandThrustingSword_ModsKey", "Thrusting One Hand Sword"),
    ("OneHandAxe_ModsKey", "One Hand Axe"),
    ("OneHandMace_ModsKey", "One Hand Mace"),
    ("Sceptre_ModsKey", "Sceptre"),
    ("Belt_ModsKey", "Belt"),
    ("Amulet_ModsKey", "Amulet"),
    ("Ring_ModsKey", "Ring"),
    ("Shield_ModsKey", "Shield"),
];

/// PoE 1 keeps an essence's mods on its own row, one column per item class,
/// plus the `Display_*` mods its tooltip names for groups of classes.
fn poe1_essences(ctx: &Ctx, essences: &crate::dat::relational::LoadedTable) -> Result<(), String> {
    let root = essences
        .rows()
        .filter_map(|row| {
            let base = ctx.rr.deref(row, "BaseItemTypesKey")?;
            let kind = ctx.rr.deref(row, "EssenceTypeKey");
            let mods = POE1_ESSENCE_CLASSES.iter().filter_map(|(column, class)| {
                let id = ctx.rr.deref_id(row, column)?;
                Some(Obj::new().set("item_classes", json::strings([class])).set("mod", text(id)).build())
            });
            let display: Vec<(String, J)> = essences
                .def
                .columns
                .iter()
                .filter_map(|c| {
                    let name = c.name.as_deref()?;
                    let label = name.strip_prefix("Display_")?.strip_suffix("_ModsKey")?;
                    Some((label.to_string(), text(ctx.rr.deref_id(row, name)?)))
                })
                .collect();
            let entry = Obj::new()
                .set("name", text(base.row().str("Name")))
                .set("tier", int(row.int("Level")))
                .or_null("type", kind.as_ref().map(|k| text(k.id())))
                .or_null("is_corrupted_type", kind.as_ref().map(|k| J::Bool(k.row().bool("IsCorruptedEssence"))))
                .set("is_screaming", J::Bool(row.bool("IsScreamingEssence")))
                .set("item_level_restriction", int(row.int("ItemLevelRestriction")))
                .set("drop_levels", J::Arr(row.list_int("DropLevel").into_iter().map(int).collect()))
                .set("monster_mods", json::strings(ctx.rr.deref_list_ids(row, "Monster_ModsKeys")))
                .set("memory_lines", json::strings(ctx.rr.deref_list_ids(row, "MemoryLines")))
                .set("mods", json::arr(mods))
                .set("display_mods", J::Obj(display))
                .build();
            Some((base.id(), entry))
        })
        .collect();
    ctx.write("essences", &J::Obj(root))
}

pub fn essences(ctx: &Ctx) -> Result<(), String> {
    let essences = ctx.table("Essences")?;
    if !ctx.rr.is_poe2 {
        return poe1_essences(ctx, &essences);
    }
    let tank_values = essences.pick(&["TankModValues", "DropLevel"]);
    let tank_mods = essences.pick(&["MonsterTankMods", "MonsterMod1"]);
    let monster_mod = essences.pick(&["MonsterMod", "MonsterMod2"]);
    let upgrade = essences.pick(&["UpgradeResult", "GreaterVariant"]);

    let mut mods_by_essence: HashMap<usize, Vec<J>> = HashMap::new();
    if let Some(table) = ctx.optional_table("EssenceMods") {
        let mod_column = table.require(&["Mod", "Mod1"])?;
        let display_column = table.pick(&["DisplayMod", "Mod2"]);
        for row in table.rows() {
            let Some(essence) = row.key("Essence") else { continue };
            let category = ctx.rr.deref(row, "TargetItemCategory");
            let weights = row.list_int("OutcomeModWeights");
            let outcomes = ctx.rr.deref_list(row, "OutcomeMods").into_iter().enumerate().map(|(i, m)| {
                Obj::new().set("mod", text(m.id())).or_null("weight", weights.get(i).map(|w| int(*w))).build()
            });
            let entry = Obj::new()
                .or_null("category", category.as_ref().map(|c| text(c.id())))
                .set(
                    "item_classes",
                    json::strings(category.as_ref().map(|c| ctx.rr.deref_list_ids(c.row(), "ItemClasses")).unwrap_or_default()),
                )
                .or_null("mod", ctx.rr.deref_id(row, mod_column).map(text))
                .or_null("display_mod", display_column.and_then(|c| ctx.rr.deref_id(row, c)).map(text))
                .or_null("text", json::opt_text(row.str("Text")))
                .set("outcomes", json::arr(outcomes))
                .build();
            mods_by_essence.entry(essence).or_default().push(entry);
        }
    }

    let root = essences
        .rows()
        .filter_map(|row| {
            let base = ctx.rr.deref(row, "BaseItemType")?;
            let upgrade = upgrade
                .and_then(|c| ctx.rr.deref(row, c))
                .and_then(|e| ctx.rr.deref_id(e.row(), "BaseItemType"));
            let entry = Obj::new()
                .set("name", text(base.row().str("Name")))
                .set("tier", int(row.int("Tier")))
                .set("is_perfect", J::Bool(row.bool("Perfect")))
                .or_null("upgrade_result", upgrade.map(text))
                .set("tank_mod_values", J::Arr(tank_values.map(|c| row.list_int(c)).unwrap_or_default().into_iter().map(int).collect()))
                .set("monster_tank_mods", json::strings(tank_mods.map(|c| ctx.rr.deref_list_ids(row, c)).unwrap_or_default()))
                .or_null("monster_mod", monster_mod.and_then(|c| ctx.rr.deref_id(row, c)).map(text))
                .or_null("map_stat", ctx.rr.deref_id(row, "MapStat").map(text))
                .set("replacement_types", json::strings(ctx.rr.deref_list_ids(row, "ReplacementType")))
                .set("mods", J::Arr(mods_by_essence.remove(&row.index).unwrap_or_default()))
                .build();
            Some((base.id(), entry))
        })
        .collect();
    ctx.write("essences", &J::Obj(root))
}

/// The jewel each emotion is instilled into, by colour and affix.
const EMOTION_SLOTS: [(&str, &str, &str); 4] = [
    ("ruby", "RubyPrefix", "RubySuffix"),
    ("emerald", "EmeraldPrefix", "EmeraldSuffix"),
    ("sapphire", "SapphirePrefix", "SapphireSuffix"),
    ("diamond", "DiamondPrefix", "DiamondSuffix"),
];

pub fn liquid_emotions(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("LiquidEmotionOutcomes")?;
    let root = table
        .rows()
        .filter_map(|row| {
            let base = ctx.rr.deref(row, "BaseItemType")?;
            let mut mods = Obj::new();
            for (jewel, prefix, suffix) in EMOTION_SLOTS {
                mods = mods.set(
                    jewel,
                    Obj::new()
                        .or_null("prefix", ctx.rr.deref_id(row, prefix).map(text))
                        .or_null("suffix", ctx.rr.deref_id(row, suffix).map(text))
                        .build(),
                );
            }
            let entry = Obj::new()
                .set("name", text(base.row().str("Name")))
                .set("drop_level", int(base.row().int("DropLevel")))
                .set("radius_jewel", J::Bool(row.int("RadiusJewel") == 1))
                .set("mods", mods.build())
                .build();
            Some((base.id(), entry))
        })
        .collect();
    ctx.write("liquid_emotions", &J::Obj(root))
}
