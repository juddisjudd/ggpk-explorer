use std::path::PathBuf;
use std::io;
use crate::bundles::index::{Index, FileInfo, murmur_hash64a};
use crate::bundles::bundle::Bundle;

/// Sentinel bundle_index value meaning "read this file from disk, not from a bundle".
pub const LOOSE_FILE_SENTINEL: u32 = u32::MAX;

#[derive(Clone)]
pub struct SteamBundleLoader {
    pub bundles2_dir: PathBuf,
}

impl SteamBundleLoader {
    pub fn new(bundles2_dir: PathBuf) -> Self {
        Self { bundles2_dir }
    }

    /// The game install root — one level above Bundles2/.
    pub fn game_root(&self) -> PathBuf {
        self.bundles2_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.bundles2_dir.clone())
    }

    /// Returns the path if this virtual file exists as a loose file on disk.
    pub fn loose_file_path(&self, virtual_path: &str) -> Option<PathBuf> {
        // Try both the path as-is and with forward-slash → backslash conversion
        let root = self.game_root();
        let p = root.join(virtual_path);
        if p.exists() {
            return Some(p);
        }
        // Try replacing forward slashes with OS separator
        let native = virtual_path.replace('/', std::path::MAIN_SEPARATOR_STR);
        let p2 = root.join(&native);
        if p2.exists() { Some(p2) } else { None }
    }

    /// Scans loose files under the game root and injects them into `index` with
    /// `bundle_index = LOOSE_FILE_SENTINEL` so the tree and loader can find them.
    pub fn add_loose_files_to_index(&self, index: &mut Index) {
        let root = self.game_root();
        // Only scan well-known loose directories to avoid noise
        for top_dir in &["Art"] {
            let dir = root.join(top_dir);
            if !dir.is_dir() {
                continue;
            }
            self.scan_loose_dir(&dir, &root, index);
        }
        println!("Steam loose file scan complete");
    }

    fn scan_loose_dir(&self, dir: &std::path::Path, root: &std::path::Path, index: &mut Index) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_loose_dir(&path, root, index);
            } else if path.is_file() {
                // Build the virtual path relative to game root using forward slashes
                let rel = match path.strip_prefix(root) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                // Skip files that are already in the bundle index (avoid duplicates)
                let lower = rel.to_ascii_lowercase();
                let hash = murmur_hash64a(lower.as_bytes());
                if index.files.contains_key(&hash) {
                    // Already present — update path string if it's empty
                    if let Some(f) = index.files.get_mut(&hash) {
                        if f.path.is_empty() {
                            f.path = rel;
                        }
                    }
                    continue;
                }
                let file_size = path.metadata().map(|m| m.len() as u32).unwrap_or(0);
                index.files.insert(hash, FileInfo {
                    path_hash: hash,
                    bundle_index: LOOSE_FILE_SENTINEL,
                    file_offset: 0,
                    file_size,
                    path: rel,
                });
            }
        }
    }

    pub fn load_index_bytes(&self) -> io::Result<Vec<u8>> {
        let path = self.bundles2_dir.join("_.index.bin");
        std::fs::read(&path)
    }

    /// Reads a raw (compressed) bundle file from the Bundles2 directory.
    pub fn fetch_bundle(&self, bundle_name: &str) -> io::Result<Vec<u8>> {
        let name = if bundle_name.ends_with(".bundle.bin") {
            bundle_name.to_string()
        } else {
            format!("{}.bundle.bin", bundle_name)
        };
        let path = self.bundles2_dir.join(&name);
        std::fs::read(&path)
    }

    /// Convenience: decompresses a bundle and extracts one file by hash.
    pub fn load_file(&self, index: &Index, hash: u64) -> Option<Vec<u8>> {
        let file_info = index.files.get(&hash)?;
        let bundle_info = index.bundles.get(file_info.bundle_index as usize)?;
        let raw = self.fetch_bundle(&bundle_info.name).ok()?;
        let mut cursor = std::io::Cursor::new(raw);
        let header = Bundle::read_header(&mut cursor).ok()?;
        let data = header.decompress(&mut cursor).ok()?;
        let start = file_info.file_offset as usize;
        let end = start + file_info.file_size as usize;
        if end <= data.len() {
            Some(data[start..end].to_vec())
        } else {
            None
        }
    }
}
