//! `Data/StatDescriptions/*.lua`, ported from `Export/Scripts/statdesc.lua`.

use crate::data_export::Ctx;
use crate::pob_export::{game, read_text, statdesc, write};
use crate::settings::Game;

const POE2_FILES: [&str; 10] = [
    "active_skill_gem_stat_descriptions",
    "advanced_mod_stat_descriptions",
    "gem_stat_descriptions",
    "meta_gem_stat_descriptions",
    "monster_stat_descriptions",
    "passive_skill_aura_stat_descriptions",
    "passive_skill_stat_descriptions",
    "skill_stat_descriptions",
    "stat_descriptions",
    "utility_flask_buff_stat_descriptions",
];

const POE1_FILES: [&str; 23] = [
    "active_skill_gem_stat_descriptions",
    "aura_skill_stat_descriptions",
    "banner_aura_skill_stat_descriptions",
    "beam_skill_stat_descriptions",
    "brand_skill_stat_descriptions",
    "curse_skill_stat_descriptions",
    "debuff_skill_stat_descriptions",
    "secondary_debuff_skill_stat_descriptions",
    "gem_stat_descriptions",
    "minion_attack_skill_stat_descriptions",
    "minion_skill_stat_descriptions",
    "minion_spell_skill_stat_descriptions",
    "minion_spell_damage_skill_stat_descriptions",
    "single_minion_spell_skill_stat_descriptions",
    "monster_stat_descriptions",
    "offering_skill_stat_descriptions",
    "skill_stat_descriptions",
    "stat_descriptions",
    "variable_duration_skill_stat_descriptions",
    "buff_skill_stat_descriptions",
    "tincture_stat_descriptions",
    "graft_stat_descriptions",
    "passive_skill_stat_descriptions",
];

/// PoE 2 also exports every skill's own file, flattening its folder into the
/// file name: `Specific_Skill_Stat_Descriptions/ancestral_cry/statset_0.csd`
/// becomes `ancestral_cry_statset_0`.
const SPECIFIC: &str = "Data/StatDescriptions/Specific_Skill_Stat_Descriptions/";

pub fn stat_descriptions(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let (dir, ext) = statdesc::description_dir(game);
    let names: &[&str] = match game {
        Game::Poe2 => &POE2_FILES,
        Game::Poe1 => &POE1_FILES,
    };
    let mut written = 0;
    for name in names {
        let Some(text) = read_text(ctx, &format!("{}{}{}", dir, name, ext)) else { continue };
        write(ctx, &format!("StatDescriptions/{}", name), statdesc::export_file(&text, game))?;
        written += 1;
    }
    if game == Game::Poe2 {
        for path in ctx.files.list_dir(SPECIFIC) {
            let Some(rest) = path.get(SPECIFIC.len()..) else { continue };
            let Some(stem) = rest.strip_suffix(ext) else { continue };
            let Some(text) = read_text(ctx, &path) else { continue };
            let flat = stem.to_ascii_lowercase().replace('/', "_");
            write(ctx, &format!("StatDescriptions/Specific_Skill_Stat_Descriptions/{}", flat), statdesc::export_file(&text, game))?;
            written += 1;
        }
    }
    match written {
        0 => Err(format!("no description files under {}", dir)),
        _ => Ok(()),
    }
}
