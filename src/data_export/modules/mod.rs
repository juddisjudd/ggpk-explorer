//! The individual dumps. Each function writes one or more JSON files and is
//! registered below under the name used by the CLI's `--only` filter.

pub mod basics;
pub mod buffs;
pub mod calculation;
pub mod crafting;
pub mod images;
pub mod items;
pub mod mods;
pub mod monsters;
pub mod passives;
pub mod poe1;
pub mod skills;
pub mod stat_translations;
pub mod world_areas;

use super::ModuleFn;
use crate::settings::Game;

/// One dump: the name it is selected by, what it holds, and how to write it.
pub struct Module {
    pub name: &'static str,
    pub summary: &'static str,
    pub run: ModuleFn,
    /// The game this dump exists for, and why the other one has nothing to put
    /// in it; `None` means both.
    pub only: Option<(Game, &'static str)>,
}

impl Module {
    /// Why this dump is not written for `game`, if it is not.
    pub fn skip_reason(&self, game: Game) -> Option<&'static str> {
        self.only.filter(|(for_game, _)| *for_game != game).map(|(_, reason)| reason)
    }
}

const fn module(name: &'static str, summary: &'static str, run: ModuleFn) -> Module {
    Module { name, summary, run, only: None }
}

const fn poe2_only(name: &'static str, summary: &'static str, run: ModuleFn, reason: &'static str) -> Module {
    Module { name, summary, run, only: Some((Game::Poe2, reason)) }
}

const fn poe1_only(name: &'static str, summary: &'static str, run: ModuleFn, reason: &'static str) -> Module {
    Module { name, summary, run, only: Some((Game::Poe1, reason)) }
}

pub fn registry() -> Vec<Module> {
    vec![
        module("active_skill_types", "Names of the skill type flags", basics::active_skill_types),
        module("characters", "Starting stats of each class", basics::characters),
        module("cost_types", "Mana, life and spirit cost kinds", basics::cost_types),
        module("default_monster_stats", "Monster base stats per level", basics::default_monster_stats),
        module("flavour", "Flavour text by id", basics::flavour),
        module("gem_tags", "Gem tag names", basics::gem_tags),
        module("item_classes", "Item classes and their categories", basics::item_classes),
        module("keywords", "In-game keyword popups", basics::keywords),
        module("tags", "Every item tag, plus tag_details", basics::tags),
        module("game_constants", "Tuning constants the client reads by name, as Value / Divisor", calculation::game_constants),
        poe2_only(
            "damage_calculation_types",
            "The stat ids each hit context reads for damage, crit, accuracy and stun",
            calculation::damage_calculation_types,
            "DamageCalculationTypes is a PoE 2 table",
        ),
        module("character_constants", "Base stats of the character and monster object definitions", calculation::character_constants),
        module("stats", "Every stat with its aggregation semantic, locality and calculation contexts", calculation::stats),
        poe2_only(
            "minion_stats",
            "Which player stats feed each minion stat",
            calculation::minion_stats,
            "PoE 1 has no MinionStats table",
        ),
        poe2_only(
            "attack_damage_scaling",
            "Attack damage multipliers and flat physical damage per gem level",
            calculation::attack_damage_scaling,
            "AttackSkillDamageScalingType and its value tables are PoE 2 tables",
        ),
        module("monsters", "Every monster variety: multipliers, base defences, resistances and mods", monsters::monsters),
        module("level_scaling", "Monster level per player level, resistance penalty per area, map difficulty", monsters::level_scaling),
        module(
            "stat_translations",
            "Stat text rules, value handlers and the stat index",
            stat_translations::stat_translations,
        ),
        module("base_items", "Every base item, plus one file per class", items::base_items),
        module("uniques", "Unique items from the stash layout", items::uniques),
        module("unique_details", "Flavour text, price and origin per unique", items::unique_details),
        poe2_only("augments", "Soul cores and runes", items::augments, "soul cores and runes are PoE 2 items (SoulCores)"),
        module("mods", "Every modifier with stats, weights and text", mods::mods),
        module("mods_by_base", "What can roll on each base item", mods::mods_by_base),
        module("skills", "Granted effects, per level and per stat set", skills::skills),
        module("skill_gems", "Gems, their tags and recommended supports", skills::skill_gems),
        module("ascendancies", "Ascendancy classes and passive overrides", skills::ascendancies),
        module("buffs", "Buff definitions, templates and sources", buffs::buffs),
        module("buff_visuals", "Buff art and what shows it", buffs::buff_visuals),
        module("audio", "NPC dialogue lines and their sound files", buffs::audio),
        module("passives", "One file per passive tree", passives::passives),
        module("timeless_jewels", "Timeless jewel passives, additions and faction rules", passives::timeless_jewels),
        module("jewel_slots", "Tree nodes that hold jewels", passives::jewel_slots),
        module("world_areas", "Areas, monster packs and topologies", world_areas::world_areas),
        poe2_only(
            "endgame_maps",
            "Atlas maps with their native monster packs and pin text",
            world_areas::endgame_maps,
            "EndgameMaps is the PoE 2 Atlas",
        ),
        module("essences", "Essences and the mod each puts on each kind of item", crafting::essences),
        poe2_only(
            "liquid_emotions",
            "Distilled emotions and the jewel mods they instil",
            crafting::liquid_emotions,
            "LiquidEmotionOutcomes is a PoE 2 table; PoE 1 anoints through BlightCraftingRecipes",
        ),
        poe1_only(
            "cluster_jewels",
            "Cluster jewel bases, the passives they add and their notables",
            poe1::cluster_jewels,
            "cluster jewels are a PoE 1 item",
        ),
        poe1_only(
            "crafting_bench_options",
            "Every crafting bench option, its mod, cost and item classes",
            poe1::crafting_bench_options,
            "PoE 2 has no crafting bench",
        ),
        poe1_only(
            "enchantments",
            "Lab, trigger, buff and flask enchantments by family, with the skills they touch",
            mods::enchantments,
            "PoE 2 has no enchantment generation types",
        ),
        poe1_only("tattoos", "Tattoo and runegraft passive overrides", poe1::tattoos, "tattoos are a PoE 1 item"),
        poe1_only("pantheons", "Pantheon gods and their souls", poe1::pantheons, "PoE 2 has no pantheon"),
        poe1_only(
            "weapon_passive_skills",
            "Crucible weapon passive tree nodes",
            poe1::weapon_passive_skills,
            "the crucible weapon tree is a PoE 1 mechanic",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_for(game: Game) -> Vec<&'static str> {
        registry().into_iter().filter(|m| m.skip_reason(game).is_none()).map(|m| m.name).collect()
    }

    /// The dumps that belong to one game, which the other reports as skipped.
    const POE2_ONLY: [&str; 6] = [
        "augments",
        "minion_stats",
        "attack_damage_scaling",
        "damage_calculation_types",
        "endgame_maps",
        "liquid_emotions",
    ];
    const POE1_ONLY: [&str; 6] = [
        "cluster_jewels",
        "crafting_bench_options",
        "enchantments",
        "tattoos",
        "pantheons",
        "weapon_passive_skills",
    ];

    #[test]
    fn each_game_runs_its_own_dumps_and_every_shared_one() {
        let (poe2, poe1) = (names_for(Game::Poe2), names_for(Game::Poe1));
        for name in POE2_ONLY {
            assert!(poe2.contains(&name), "{} must run for PoE 2", name);
            assert!(!poe1.contains(&name), "{} must be skipped for PoE 1", name);
        }
        for name in POE1_ONLY {
            assert!(poe1.contains(&name), "{} must run for PoE 1", name);
            assert!(!poe2.contains(&name), "{} must be skipped for PoE 2", name);
        }
        let total = registry().len();
        assert_eq!(poe2.len(), total - POE1_ONLY.len(), "PoE 2 runs everything but PoE 1's own dumps");
        assert_eq!(poe1.len(), total - POE2_ONLY.len(), "PoE 1 runs everything but PoE 2's own dumps");
    }

    #[test]
    fn a_skipped_dump_says_which_game_it_belongs_to() {
        for module in registry() {
            let (poe2, poe1) = (module.skip_reason(Game::Poe2), module.skip_reason(Game::Poe1));
            assert!(poe2.is_none() || poe1.is_none(), "{} is skipped for both games", module.name);
            for reason in [poe2, poe1].into_iter().flatten() {
                assert!(!reason.is_empty(), "{} is skipped without saying why", module.name);
            }
        }
    }
}
