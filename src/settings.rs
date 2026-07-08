use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Filename for the cached, parsed `Bundles2/_.index.bin` (bincode).
pub const INDEX_CACHE_FILENAME: &str = "bundles2.cache";
/// Filename for the cached tree-view node list built from the index.
/// The `.v2.` marks the tree node schema version — bump it if `TreeView`'s
/// cached node layout changes so old caches are ignored rather than
/// deserialized into the wrong shape.
pub const TREE_CACHE_FILENAME: &str = "bundles2.tree.v2.cache";


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub ggpk_path: Option<String>,
    #[serde(default)]
    pub steam_path: Option<String>,
    pub recent_files: Vec<String>,
    #[serde(default = "default_patch_version")]
    pub poe2_patch_version: String,
    #[serde(default = "default_patch_source")]
    pub patch_version_source_url: String,
    #[serde(default = "default_auto_detect_patch_version")]
    pub auto_detect_patch_version: bool,
    #[serde(default = "default_auto_update_schema")]
    pub auto_update_schema: bool,
    pub schema_local_path: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_patch_version() -> String {
    "4.5.1.1.4".to_string()
}

fn default_patch_source() -> String {
    "https://poe-versions.obsoleet.org".to_string()
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
            patch_version_source_url: default_patch_source(),
            auto_detect_patch_version: default_auto_detect_patch_version(),
            auto_update_schema: default_auto_update_schema(),
            schema_local_path: None,
            theme: default_theme(),
        }
    }
}

use std::path::PathBuf;

impl AppSettings {
    pub fn fetch_latest_patch_version(source_url: &str) -> Result<String, String> {
        // Try direct patch server protocol first (most reliable)
        match Self::fetch_patch_version_direct() {
            Ok(version) => {
                println!("[PatchVersion] Got version from patch server: {}", version);
                return Ok(version);
            },
            Err(e) => {
                println!("[PatchVersion] Direct patch server failed: {}. Falling back to HTTP.", e);
            }
        }

        // Fallback to HTTP endpoint
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(source_url)
            .header("User-Agent", "ggpk-explorer/1.2.5")
            .send()
            .map_err(|e| format!("Network Error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP Error: {}", resp.status()));
        }

        let json = resp
            .json::<serde_json::Value>()
            .map_err(|e| format!("JSON Parse Error: {}", e))?;

        json.get("poe2")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .ok_or_else(|| "JSON missing 'poe2' field".to_string())
    }

    /// Fetch PoE2 patch version directly from the game's patch server.
    /// Protocol ported from poe-get-version (Go reference implementation).
    ///
    /// Protocol:
    /// 1. Connect TCP to patch.pathofexile2.com:13060
    /// 2. Send handshake bytes [0x01, 0x07]
    /// 3. Read response: 1 byte proto_ver + 32 bytes unknown + N bytes payload
    /// 4. Payload contains two UTF-16LE length-prefixed strings (CDN URLs)
    /// 5. Parse version from URL: https://patch-poe2.poecdn.com/{VERSION}/
    fn fetch_patch_version_direct() -> Result<String, String> {
        use std::net::{TcpStream, ToSocketAddrs};
        use std::time::Duration;

        let addr = "patch.pathofexile2.com:13060";
        // Resolve DNS first — connect_timeout only accepts SocketAddr (IP:port)
        let socket_addr = addr.to_socket_addrs()
            .map_err(|e| format!("DNS resolve failed for {}: {}", addr, e))?
            .next()
            .ok_or_else(|| format!("No addresses found for {}", addr))?;

        let mut stream = TcpStream::connect_timeout(
            &socket_addr,
            Duration::from_secs(5),
        ).map_err(|e| format!("TCP connect failed to {}: {}", addr, e))?;

        stream.set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Set timeout failed: {}", e))?;

        // Handshake: send [0x01, 0x07] for PoE2
        stream.write_all(&[0x01, 0x07])
            .map_err(|e| format!("Write handshake failed: {}", e))?;

        // Read response
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf)
            .map_err(|e| format!("Read response failed: {}", e))?;

        if n < 35 {
            return Err(format!("Response too short: {} bytes", n));
        }

        let data = &buf[..n];

        // Verify protocol version
        let proto_ver = data[0];
        if proto_ver != 2 {
            return Err(format!("Unexpected protocol version: {}", proto_ver));
        }

        // Skip header: 1 byte proto + 32 bytes unknown = 33 bytes
        let mut pos = 33;

        // Read two length-prefixed UTF-16LE strings
        // We want the second one (or either — they should be the same URL)
        let mut version_str = None;
        for _ in 0..2 {
            if pos + 2 > data.len() { break; }

            // Length is big-endian u16 (number of UTF-16 code units)
            let str_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;

            let byte_len = str_len * 2;
            if pos + byte_len > data.len() { break; }

            // Decode UTF-16LE
            let u16s: Vec<u16> = data[pos..pos + byte_len]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            let s = String::from_utf16_lossy(&u16s);
            pos += byte_len;

            // Parse version from URL: https://patch-poe2.poecdn.com/4.5.1.1.4/
            if let Some(ver) = Self::extract_version_from_cdn_url(&s) {
                version_str = Some(ver);
            }
        }

        version_str.ok_or_else(|| "Could not extract version from patch server response".to_string())
    }

    /// Extract version string from a CDN URL like
    /// "https://patch-poe2.poecdn.com/4.5.1.1.4/"
    fn extract_version_from_cdn_url(url: &str) -> Option<String> {
        let s = url.trim_start_matches("https://");
        let s = s.trim_start_matches("http://");
        // Remove host: "patch-poe2.poecdn.com/" or "patch.poecdn.com/"
        let s = if let Some(rest) = s.strip_prefix("patch-poe2.poecdn.com/") {
            rest
        } else if let Some(rest) = s.strip_prefix("patch.poecdn.com/") {
            rest
        } else {
            return None;
        };
        let version = s.trim_end_matches('/');
        if version.is_empty() {
            return None;
        }
        // Sanity: should look like a version (contains dots and digits)
        if version.contains('.') && version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            Some(version.to_string())
        } else {
            None
        }
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

    pub fn get_cache_size() -> u64 {
        let dir = Self::get_app_data_dir();
        let cache_dir = dir.join("cache");
        let cache_file = dir.join(INDEX_CACHE_FILENAME);
        let tree_cache = dir.join(TREE_CACHE_FILENAME);
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

        if tree_cache.exists() {
             if let Ok(metadata) = std::fs::metadata(&tree_cache) {
                 size += metadata.len();
             }
        }
        
        size
    }

    pub fn clear_cache() -> std::io::Result<()> {
        let dir = Self::get_app_data_dir();
        let cache_dir = dir.join("cache");
        let cache_file = dir.join(INDEX_CACHE_FILENAME);
        let tree_cache = dir.join(TREE_CACHE_FILENAME);

        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
            std::fs::create_dir_all(&cache_dir)?;
        }
        
        if cache_file.exists() {
            std::fs::remove_file(&cache_file)?;
        }

        if tree_cache.exists() {
            std::fs::remove_file(&tree_cache)?;
        }
        Ok(())
    }
}
