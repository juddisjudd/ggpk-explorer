//! The individual dumps. Each function writes one or more JSON files and is
//! registered below under the name used by the CLI's `--only` filter.

pub mod basics;
pub mod buffs;
pub mod images;
pub mod items;
pub mod mods;
pub mod passives;
pub mod skills;
pub mod stat_translations;
pub mod world_areas;

use super::ModuleFn;

/// One dump: the name it is selected by, what it holds, and how to write it.
pub struct Module {
    pub name: &'static str,
    pub summary: &'static str,
    pub run: ModuleFn,
}

const fn module(name: &'static str, summary: &'static str, run: ModuleFn) -> Module {
    Module { name, summary, run }
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
        module(
            "stat_translations",
            "Stat text rules, value handlers and the stat index",
            stat_translations::stat_translations,
        ),
        module("base_items", "Every base item, plus one file per class", items::base_items),
        module("uniques", "Unique items from the stash layout", items::uniques),
        module("augments", "Soul cores and runes", items::augments),
        module("mods", "Every modifier with stats, weights and text", mods::mods),
        module("mods_by_base", "What can roll on each base item", mods::mods_by_base),
        module("skills", "Granted effects, per level and per stat set", skills::skills),
        module("skill_gems", "Gems, their tags and recommended supports", skills::skill_gems),
        module("ascendancies", "Ascendancy classes and passive overrides", skills::ascendancies),
        module("buffs", "Buff definitions, templates and sources", buffs::buffs),
        module("buff_visuals", "Buff art and what shows it", buffs::buff_visuals),
        module("audio", "NPC dialogue lines and their sound files", buffs::audio),
        module("passives", "One file per passive tree", passives::passives),
        module("world_areas", "Areas, monster packs and topologies", world_areas::world_areas),
    ]
}
