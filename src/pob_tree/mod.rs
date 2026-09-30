//! Path of Building's passive tree data, the `src/TreeData/<version>/`
//! folders, written from the game files.
//!
//! PoE 2 follows PoB-PoE2's own exporter (`Export/Scripts/passivetree.lua`),
//! which reads the game files too, so its output is meant to match PoB's
//! byte for byte apart from the orbit PNGs. PoE 1's PoB builds its trees from
//! GGG's web-export `data.json` instead; that file is built here from the
//! game (the same builder as the tree viewer's export) and then put through
//! the two steps PoB's release process runs on it.

mod dds;
mod format;
mod luajit;
mod orbits;
mod poe1;
mod poe2;

use crate::data_export::modules::{module, poe1_only, Module};
use crate::data_export::{json, Ctx, DataExportOptions, RunKind};
use crate::export::ExportStatus;
use crate::settings::Game;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub fn registry() -> Vec<Module> {
    vec![
        module("tree", "The character tree: PoE 2's 0_X, PoE 1's 3_X", tree),
        poe1_only("ruthless", "PoE 1's Ruthless tree, 3_X_ruthless", ruthless, "PoE 2 has no Ruthless tree"),
    ]
}

pub fn module_names() -> Vec<&'static str> {
    registry().into_iter().map(|m| m.name).collect()
}

pub fn run(
    files: crate::data_export::source::GameFiles,
    schema: crate::dat::schema::Schema,
    is_poe2: bool,
    out: PathBuf,
    options: DataExportOptions,
    tx: Sender<ExportStatus>,
) {
    let kind = RunKind { file_suffix: "", unit: "passive trees", extra_report: Vec::<(String, json::J)>::new() };
    crate::data_export::run_with(registry(), kind, files, schema, is_poe2, out, options, tx);
}

/// The folder PoB keeps a patch's tree in: PoE 1's `3.29.x` is `3_29`, PoE
/// 2's `4.5.x` (0.5 to players) is `0_5`.
pub fn tree_version(game: Game, patch: &str) -> Option<String> {
    let mut parts = patch.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    match (game, major) {
        (Game::Poe1, 3) => Some(format!("3_{}", minor)),
        (Game::Poe2, 4) => Some(format!("0_{}", minor)),
        _ => None,
    }
}

fn folder(ctx: &Ctx) -> Result<String, String> {
    let game = crate::pob_export::game(ctx);
    let patch = ctx.options.version.as_deref().ok_or("the patch is unknown; name it with --version")?;
    tree_version(game, patch).ok_or_else(|| format!("{} is not a {} patch", patch, game.label()))
}

fn tree(ctx: &Ctx) -> Result<(), String> {
    let version = folder(ctx)?;
    match crate::pob_export::game(ctx) {
        Game::Poe2 => poe2::write(ctx, &version),
        Game::Poe1 => poe1::write(ctx, &version, false),
    }
}

fn ruthless(ctx: &Ctx) -> Result<(), String> {
    poe1::write(ctx, &format!("{}_ruthless", folder(ctx)?), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_versions_follow_pob_folder_names() {
        assert_eq!(tree_version(Game::Poe2, "4.5.5.4").as_deref(), Some("0_5"));
        assert_eq!(tree_version(Game::Poe1, "3.29.3.3").as_deref(), Some("3_29"));
        assert_eq!(tree_version(Game::Poe1, "4.5.5.4"), None);
    }
}
