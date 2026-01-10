use serde::{Deserialize, Serialize};


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub ggpk_path: Option<String>,
    pub recent_files: Vec<String>,
    #[serde(default = "default_patch_version")]
    pub poe2_patch_version: String,
    #[serde(default = "default_patch_source")]
    pub patch_version_source_url: String,
    pub schema_local_path: Option<String>,
}

fn default_patch_version() -> String {
    "4.4.0.3.7".to_string()
}

fn default_patch_source() -> String {
    "https://poe-versions.obsoleet.org".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ggpk_path: None,
            recent_files: Vec::new(),
            poe2_patch_version: default_patch_version(),
            patch_version_source_url: default_patch_source(),
            schema_local_path: None,
        }
    }
}

use std::path::PathBuf;

impl AppSettings {
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

    pub fn get_cache_size() -> u64 {
        let dir = Self::get_app_data_dir();
        let cache_dir = dir.join("cache");
        let cache_file = dir.join("bundles2.cache");
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
        
        size
    }

    pub fn clear_cache() -> std::io::Result<()> {
        let dir = Self::get_app_data_dir();
        let cache_dir = dir.join("cache");
        let cache_file = dir.join("bundles2.cache");

        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
            std::fs::create_dir_all(&cache_dir)?;
        }
        
        if cache_file.exists() {
            std::fs::remove_file(&cache_file)?;
        }
        Ok(())
    }
}
