//! Turns strings that name other game files into clickable links. Every viewer
//! funnels clicks into an `Option<String>` the content view resolves against the index.

use eframe::egui;

const EXTS: &[&str] = &[
    "ao", "aoc", "ot", "otc", "it", "act", "epk", "pet", "mat", "fxgraph", "dds", "png", "jpg", "ogg", "wav", "bank", "bk2",
    "sm", "fmt", "tgm", "ast", "smd", "amd", "atl", "trl", "env", "ui", "csd", "dat", "datc64", "dat64", "tst", "tsi", "rs",
    "mtd", "dgr", "arm", "tdt", "tmd", "tgt", "gt", "et", "gft", "dct", "cht", "clt", "ddt", "ecf", "fgp", "tmo", "dlp",
    "mtp", "hideout", "json", "psg", "ffx", "hlsl", "ttf", "toy", "gcf", "tgr", "atlas", "filter", "ais", "chr", "tdf",
];

pub fn normalize(s: &str) -> String {
    s.trim().trim_matches('"').replace('\\', "/")
}

/// `Metadata/Items/Foo.ot`, `Art/…/tex.dds` and friends; bare words and numbers are not paths.
pub fn looks_like_path(s: &str) -> bool {
    let p = normalize(s);
    if p.len() < 5 || !p.contains('/') || p.contains('\n') {
        return false;
    }
    let lower = p.to_ascii_lowercase();
    lower.rsplit_once('.').map(|(_, e)| EXTS.contains(&e)).unwrap_or(false)
}

fn link_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(120, 180, 255) } else { egui::Color32::from_rgb(20, 90, 200) }
}

/// Draws `text` as a link when it names a game file; a click lands the path in `out`.
pub fn maybe_link(ui: &mut egui::Ui, text: &str, monospace: bool, out: &mut Option<String>) -> egui::Response {
    if looks_like_path(text) {
        let mut rich = egui::RichText::new(text).color(link_color(ui)).underline();
        if monospace {
            rich = rich.monospace();
        }
        let r = ui.add(egui::Label::new(rich).sense(egui::Sense::click())).on_hover_text("Open file");
        if r.clicked() {
            *out = Some(normalize(text));
        }
        r.context_menu(|ui| {
            if ui.button("Copy path").clicked() {
                ui.ctx().copy_text(normalize(text));
                ui.close_menu();
            }
        });
        r
    } else {
        let mut rich = egui::RichText::new(text);
        if monospace {
            rich = rich.monospace();
        }
        ui.label(rich)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_game_paths() {
        assert!(looks_like_path("Metadata/Items/Weapons/AbstractWeapon.it"));
        assert!(looks_like_path("\"Art\\Models\\Terrain\\x.fmt\""));
        assert!(looks_like_path("Audio/Sound Effects/Foo.ogg"));
        assert!(!looks_like_path("Linear"));
        assert!(!looks_like_path("1.5"));
        assert!(!looks_like_path("Audio/Foo/Up_$(#).ogg%40"));
        assert_eq!(normalize("\"Art\\x.dds\""), "Art/x.dds");
    }
}
