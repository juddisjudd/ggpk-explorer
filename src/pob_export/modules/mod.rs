//! One module per PoB export script. Each writes the files that script
//! writes under `src/Data/`, named as PoB names them.

pub mod crafting;
pub mod items;
pub mod mods;
pub mod monsters;
pub mod skills;
pub mod stat_descriptions;

use crate::data_export::modules::{module, poe1_only, poe2_only, Module};

pub fn registry() -> Vec<Module> {
    vec![
        module(
            "stat_descriptions",
            "StatDescriptions/*: every description file as PoB loads it (statdesc.lua)",
            stat_descriptions::stat_descriptions,
        ),
        module("mods", "Mod*.json: item, flask, jewel, corrupted and other mod pools (mods.lua)", mods::mods),
        module("mod_scalability", "ModScalability.json: which stat lines scale (modScalability.lua)", mods::mod_scalability),
        poe1_only("mod_master", "ModMaster.json: crafting bench mods (masters.lua)", mods::mod_master, "PoE 2 has no crafting bench"),
        module("essence", "Essence.json, and LiquidEmotions.json for PoE 2 (essence.lua)", crafting::essence),
        poe2_only("runes", "ModRunes.json: runes and soul cores (soulcores.lua)", crafting::runes, "runes and soul cores are PoE 2 items"),
        poe1_only("enchantments", "Enchantment*.json: lab, flask and other enchantments (enchant.lua)", crafting::enchantments, "PoE 2 has no enchantments"),
        poe1_only("crucible", "Crucible.json: crucible weapon passives (crucible.lua)", crafting::crucible, "crucible is a PoE 1 league"),
        poe1_only("tattoos", "TattooPassives.json (tattooPassives.lua)", crafting::tattoos, "tattoos are a PoE 1 item"),
        poe1_only("cluster_jewels", "ClusterJewels.json (cluster.lua)", crafting::cluster_jewels, "cluster jewels are a PoE 1 item"),
        module("timeless_jewels", "TimelessJewelData/LegionPassives.json (legionPassives.lua)", crafting::timeless_jewels),
        module("pantheons", "Pantheons.json (pantheons.lua)", crafting::pantheons),
        module("bases", "Bases/*.json: every base item by PoB's item type (bases.lua)", items::bases),
        module("flavour_text", "FlavourText.json (flavourText.lua)", items::flavour_text),
        poe2_only("inventory_slots", "InventorySlots.json (buildplanner.lua)", items::inventory_slots, "BuildPlannerInventories is a PoE 2 table"),
        module("skills", "Skills/*.json, Gems.json and PoE 1's PearlSupports.json (skills.lua, skillGemList.lua)", skills::skills),
        poe2_only("assets", "Assets.json and Skills/SkillAssets.json (assets.lua)", skills::assets, "PoB exports these for PoE 2 only"),
        module("minions", "Minions.json and Spectres.json (minions.lua, spectreList.lua)", monsters::minions),
        poe1_only("bosses", "Bosses.json and BossSkills.json (bossData.lua)", monsters::bosses, "PoB-PoE2 switches bossData.lua off and ships PoE 1's boss files"),
        poe2_only("world_areas", "WorldAreas.json (worldAreas.lua)", monsters::world_areas, "PoB exports world areas for PoE 2 only"),
        module("misc", "Misc.json, CurrencyNames.json, and CharacterMeleeSkills.json for PoE 2 (miscdata.lua)", monsters::misc),
        module("costs", "Costs.json: skill cost types (costs.lua)", monsters::costs),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Game;

    #[test]
    fn every_module_runs_for_at_least_one_game() {
        for m in registry() {
            assert!(m.skip_reason(Game::Poe2).is_none() || m.skip_reason(Game::Poe1).is_none(), "{}", m.name);
        }
        let mut names: Vec<_> = registry().into_iter().map(|m| m.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), registry().len(), "module names are unique");
    }
}
