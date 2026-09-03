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

pub fn registry() -> Vec<(&'static str, ModuleFn)> {
    vec![
        ("active_skill_types", basics::active_skill_types as ModuleFn),
        ("characters", basics::characters),
        ("cost_types", basics::cost_types),
        ("default_monster_stats", basics::default_monster_stats),
        ("flavour", basics::flavour),
        ("gem_tags", basics::gem_tags),
        ("item_classes", basics::item_classes),
        ("keywords", basics::keywords),
        ("tags", basics::tags),
        ("stat_translations", stat_translations::stat_translations),
        ("base_items", items::base_items),
        ("uniques", items::uniques),
        ("augments", items::augments),
        ("mods", mods::mods),
        ("mods_by_base", mods::mods_by_base),
        ("skills", skills::skills),
        ("skill_gems", skills::skill_gems),
        ("ascendancies", skills::ascendancies),
        ("buffs", buffs::buffs),
        ("buff_visuals", buffs::buff_visuals),
        ("audio", buffs::audio),
        ("passives", passives::passives),
        ("world_areas", world_areas::world_areas),
    ]
}
