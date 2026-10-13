use serde::{Deserialize, Serialize};

/// Filename for the cached, parsed `Bundles2/_.index.bin` (bincode).
pub const INDEX_CACHE_FILENAME: &str = "bundles2.cache";
/// Filename for the cached tree-view node list built from the index.
/// The `.v2.` marks the tree node schema version — bump it if `TreeView`'s
/// cached node layout changes so old caches are ignored rather than
/// deserialized into the wrong shape.
pub const TREE_CACHE_FILENAME: &str = "bundles2.tree.v2.cache";
/// Tree cache built with shader-cache entries filtered out (see `AppSettings::hide_shader_cache`).
pub const TREE_CACHE_NOSHADER_FILENAME: &str = "bundles2.tree.v2.noshader.cache";

/// Filename recording which patch the disk caches were built for.
pub const CACHE_STAMP_FILENAME: &str = "cache.patch";

/// Whether caches stamped `stamped` can be trusted to describe `version`.
/// Caches written before stamping existed carry no stamp at all, and then the
/// saved patch version is the only other record of what they were built
/// against, so they are kept only when it agrees.
fn cache_matches_patch(stamped: Option<&str>, version: &str, saved_version: &str) -> bool {
    match stamped {
        Some(stamp) => stamp == version,
        None => saved_version == version,
    }
}

/// Which game an install, its caches and its exports belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Game {
    #[default]
    Poe2,
    Poe1,
}

impl Game {
    pub fn from_is_poe2(is_poe2: bool) -> Self {
        if is_poe2 { Game::Poe2 } else { Game::Poe1 }
    }

    pub fn is_poe2(self) -> bool {
        self == Game::Poe2
    }

    pub fn label(self) -> &'static str {
        match self {
            Game::Poe2 => "PoE 2",
            Game::Poe1 => "PoE 1",
        }
    }
}

pub fn tree_cache_filename(hide_shader_cache: bool) -> &'static str {
    if hide_shader_cache { TREE_CACHE_NOSHADER_FILENAME } else { TREE_CACHE_FILENAME }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub ggpk_path: Option<String>,
    #[serde(default)]
    pub steam_path: Option<String>,
    pub recent_files: Vec<String>,
    #[serde(default = "default_patch_version")]
    pub poe2_patch_version: String,
    #[serde(default = "default_auto_detect_patch_version")]
    pub auto_detect_patch_version: bool,
    #[serde(default = "default_auto_update_schema")]
    pub auto_update_schema: bool,
    pub schema_local_path: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Drop the ~2.8M `shadercache*/` blobs from the index at load time.
    #[serde(default = "default_hide_shader_cache")]
    pub hide_shader_cache: bool,
    /// The game the explorer opens; `ggpk_path`, `steam_path` and
    /// `poe2_patch_version` belong to PoE 2.
    #[serde(default)]
    pub game: Game,
    #[serde(default)]
    pub poe1_ggpk_path: Option<String>,
    #[serde(default)]
    pub poe1_steam_path: Option<String>,
    #[serde(default)]
    pub poe1_patch_version: String,
}

fn default_hide_shader_cache() -> bool {
    true
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_patch_version() -> String {
    "4.5.1.1.4".to_string()
}

fn default_auto_detect_patch_version() -> bool {
    true
}

fn default_auto_update_schema() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ggpk_path: None,
            steam_path: None,
            recent_files: Vec::new(),
            poe2_patch_version: default_patch_version(),
            auto_detect_patch_version: default_auto_detect_patch_version(),
            auto_update_schema: default_auto_update_schema(),
            schema_local_path: None,
            theme: default_theme(),
            hide_shader_cache: default_hide_shader_cache(),
            game: Game::default(),
            poe1_ggpk_path: None,
            poe1_steam_path: None,
            poe1_patch_version: String::new(),
        }
    }
}

use std::path::PathBuf;

impl AppSettings {
    pub fn patch_version(&self, game: Game) -> &str {
        match game {
            Game::Poe2 => &self.poe2_patch_version,
            Game::Poe1 => &self.poe1_patch_version,
        }
    }

    pub fn ggpk_path_for(&self, game: Game) -> Option<&String> {
        match game {
            Game::Poe2 => self.ggpk_path.as_ref(),
            Game::Poe1 => self.poe1_ggpk_path.as_ref(),
        }
    }

    pub fn steam_path_for(&self, game: Game) -> Option<&String> {
        match game {
            Game::Poe2 => self.steam_path.as_ref(),
            Game::Poe1 => self.poe1_steam_path.as_ref(),
        }
    }

    pub fn set_patch_version(&mut self, game: Game, version: String) {
        match game {
            Game::Poe2 => self.poe2_patch_version = version,
            Game::Poe1 => self.poe1_patch_version = version,
        }
    }

    /// Records where one game's install is, leaving the other game's alone.
    pub fn set_ggpk_path(&mut self, game: Game, path: Option<String>) {
        match game {
            Game::Poe2 => self.ggpk_path = path,
            Game::Poe1 => self.poe1_ggpk_path = path,
        }
    }

    pub fn set_steam_path(&mut self, game: Game, path: Option<String>) {
        match game {
            Game::Poe2 => self.steam_path = path,
            Game::Poe1 => self.poe1_steam_path = path,
        }
    }

    fn install_root(&self, game: Game) -> Option<&std::path::Path> {
        std::path::Path::new(self.ggpk_path_for(game).or(self.steam_path_for(game))?).parent()
    }

    /// The patch the saved install of `game` is on, by its client log.
    pub fn installed_patch(&self, game: Game) -> Option<String> {
        crate::data_export::detect_version(self.install_root(game)?)
    }

    /// The protocol number the saved install of `game` last sent its patch server.
    pub fn patch_protocol(&self, game: Game) -> Option<u8> {
        crate::patch_server::logged_protocol(self.install_root(game)?)
    }

    pub fn get_app_data_dir() -> PathBuf {

        if let Ok(app_data) = std::env::var("APPDATA") {
            let path = PathBuf::from(app_data).join("ggpk-explorer");
            if !path.exists() {
                let _ = std::fs::create_dir_all(&path);
            }
            return path;
        }
        

        PathBuf::from(".")
    }

    pub fn load() -> Self {
        let dir = Self::get_app_data_dir();
        let path = dir.join("settings.json");
        
        if let Ok(content) = std::fs::read_to_string(path) {
             if let Ok(settings) = serde_json::from_str(&content) {
                 return settings;
             }
        }
        Self::default()
    }

    pub fn save(&self) {
        let dir = Self::get_app_data_dir();
        let path = dir.join("settings.json");
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }

    /// One game's cache folder. PoE 2 keeps the app data folder its caches have
    /// always used; PoE 1 gets its own, so the two indexes and patch stamps do
    /// not overwrite each other.
    pub fn cache_dir(game: Game) -> PathBuf {
        let dir = Self::get_app_data_dir();
        match game {
            Game::Poe2 => dir,
            Game::Poe1 => {
                let dir = dir.join("poe1");
                let _ = std::fs::create_dir_all(&dir);
                dir
            }
        }
    }

    pub fn index_cache_path(game: Game) -> PathBuf {
        Self::cache_dir(game).join(INDEX_CACHE_FILENAME)
    }

    pub fn tree_cache_path(game: Game, hide_shader_cache: bool) -> PathBuf {
        Self::cache_dir(game).join(tree_cache_filename(hide_shader_cache))
    }

    /// Downloaded CDN bundles and other per-file caches.
    pub fn file_cache_dir(game: Game) -> PathBuf {
        Self::cache_dir(game).join("cache")
    }

    pub fn get_cache_size(game: Game) -> u64 {
        let dir = Self::cache_dir(game);
        let cache_dir = dir.join("cache");
        let cache_file = dir.join(INDEX_CACHE_FILENAME);
        let tree_caches = [dir.join(TREE_CACHE_FILENAME), dir.join(TREE_CACHE_NOSHADER_FILENAME)];
        let mut size = 0;

        if cache_dir.exists() {
             for entry in walkdir::WalkDir::new(&cache_dir).into_iter().filter_map(|e| e.ok()) {
                 if let Ok(metadata) = entry.metadata() {
                     if metadata.is_file() {
                         size += metadata.len();
                     }
                 }
             }
        }

        if cache_file.exists() {
             if let Ok(metadata) = std::fs::metadata(&cache_file) {
                 size += metadata.len();
             }
        }

        for tree_cache in &tree_caches {
            if let Ok(metadata) = std::fs::metadata(tree_cache) {
                size += metadata.len();
            }
        }
        
        size
    }

    /// Records which patch the caches now hold, so a later run can tell whether
    /// they still describe the installed game.
    pub fn stamp_cache_patch(version: &str, game: Game) -> std::io::Result<()> {
        std::fs::write(Self::cache_dir(game).join(CACHE_STAMP_FILENAME), version)
    }

    /// Clears the disk caches unless they were already built for `version`, and
    /// re-stamps them either way. The index, tree and CDN caches key on file
    /// names alone, and a patch rewrites bundle contents under unchanged names,
    /// so carrying them across a patch reads back wrong data rather than merely
    /// stale data. Returns whether anything was cleared.
    /// Saves the outgoing patch's index before it is wiped. The index cache is
    /// the only full record of what the last patch shipped, so a diff against
    /// the next one is impossible once it is gone.
    fn snapshot_outgoing(old_version: &str, game: Game) {
        if old_version.is_empty() || crate::diff::has_snapshot_for_version(old_version, game) {
            return;
        }
        let cache = Self::index_cache_path(game);
        if !cache.exists() {
            return;
        }
        match crate::bundles::index::Index::load_from_cache(&cache) {
            Ok(mut index) => {
                index.drop_shader_cache();
                match crate::diff::take_snapshot(&index, old_version, "auto (pre-patch index cache)", game) {
                    Ok(_) => println!("Saved a snapshot of patch {} before clearing the caches", old_version),
                    Err(e) => println!("Could not snapshot patch {}: {}", old_version, e),
                }
            }
            Err(e) => println!("Could not read the index cache to snapshot patch {}: {}", old_version, e),
        }
    }

    pub fn sync_cache_to_patch(version: &str, game: Game) -> std::io::Result<bool> {
        let stamped = std::fs::read_to_string(Self::cache_dir(game).join(CACHE_STAMP_FILENAME)).ok();
        let stamped = stamped.as_deref().map(str::trim);
        if stamped == Some(version) {
            return Ok(false);
        }
        let settings = Self::load();
        let saved_version = settings.patch_version(game).to_string();
        let adopt = cache_matches_patch(stamped, version, &saved_version);
        if !adopt {
            Self::snapshot_outgoing(stamped.unwrap_or(&saved_version), game);
            Self::clear_cache(game)?;
        }
        Self::stamp_cache_patch(version, game)?;
        Ok(!adopt)
    }

    pub fn clear_cache(game: Game) -> std::io::Result<()> {
        let dir = Self::cache_dir(game);
        let cache_dir = dir.join("cache");
        let cache_file = dir.join(INDEX_CACHE_FILENAME);
        let tree_caches = [dir.join(TREE_CACHE_FILENAME), dir.join(TREE_CACHE_NOSHADER_FILENAME)];

        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
            std::fs::create_dir_all(&cache_dir)?;
        }
        
        if cache_file.exists() {
            std::fs::remove_file(&cache_file)?;
        }

        for tree_cache in &tree_caches {
            if tree_cache.exists() {
                std::fs::remove_file(tree_cache)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod cache_stamp_tests {
    use super::cache_matches_patch;

    #[test]
    fn a_stamp_decides_on_its_own() {
        assert!(cache_matches_patch(Some("4.5.5.1"), "4.5.5.1", "anything"));
        assert!(!cache_matches_patch(Some("4.5.5.1"), "4.5.5.2", "4.5.5.2"));
    }

    /// Caches written before stamping existed fall back to the saved version.
    #[test]
    fn without_a_stamp_the_saved_version_decides() {
        assert!(cache_matches_patch(None, "4.5.5.1", "4.5.5.1"));
        assert!(!cache_matches_patch(None, "4.5.5.2", "4.5.5.1"));
    }
}

#[cfg(test)]
mod saved_settings_tests {
    use super::{AppSettings, Game};

    #[test]
    fn settings_saved_by_an_earlier_version_still_load() {
        let saved = r#"{
            "ggpk_path": "D:/PoE2/Content.ggpk",
            "steam_path": null,
            "recent_files": ["data/balance/mods.datc64"],
            "poe2_patch_version": "0.5.5.4",
            "auto_detect_patch_version": true,
            "auto_update_schema": false,
            "schema_local_path": null,
            "patch_version_source_url": "https://example.invalid",
            "theme": "dark",
            "hide_shader_cache": true,
            "game": "Poe1",
            "poe1_ggpk_path": "D:/PoE1/Content.ggpk",
            "poe1_steam_path": null,
            "poe1_patch_version": "3.29.3.3"
        }"#;
        let settings: AppSettings = serde_json::from_str(saved).expect("a field that was removed must not stop the file loading");
        assert_eq!(settings.game, Game::Poe1);
        assert_eq!(settings.ggpk_path_for(Game::Poe2).map(String::as_str), Some("D:/PoE2/Content.ggpk"));
        assert_eq!(settings.ggpk_path_for(Game::Poe1).map(String::as_str), Some("D:/PoE1/Content.ggpk"));
        assert_eq!((settings.patch_version(Game::Poe2), settings.patch_version(Game::Poe1)), ("0.5.5.4", "3.29.3.3"));
        assert!(!settings.auto_update_schema && settings.theme == "dark");
    }
}
