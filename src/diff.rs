use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

use crate::bundles::index::{is_shader_cache_path, Index, GGPK_LOOSE_FILE_SENTINEL};
use crate::bundles::steam::LOOSE_FILE_SENTINEL;

pub const SNAPSHOT_DIR: &str = "snapshots";
pub const SNAPSHOT_EXT: &str = "snapshot";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub patch_version: String,
    pub source: String,
    pub created_at: i64,
    pub file_count: u64,
    pub bundle_count: u64,
}

impl SnapshotMeta {
    pub fn created_at_label(&self) -> String {
        chrono::DateTime::from_timestamp(self.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

pub fn snapshot_dir() -> PathBuf {
    let dir = crate::settings::AppSettings::get_app_data_dir().join(SNAPSHOT_DIR);
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Serializes `(meta, index)` as a bincode tuple so the meta can later be read
/// back on its own without deserializing the (much larger) index behind it.
pub fn save_snapshot(meta: &SnapshotMeta, index: &Index) -> io::Result<PathBuf> {
    let filename = format!("{}_{}.{}", meta.created_at, meta.patch_version, SNAPSHOT_EXT);
    let path = snapshot_dir().join(filename);
    let file = std::fs::File::create(&path)?;
    let mut writer = std::io::BufWriter::new(file);
    bincode::serialize_into(&mut writer, &(meta, index))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(path)
}

pub fn take_snapshot(index: &Index, patch_version: &str, source: &str) -> io::Result<PathBuf> {
    let meta = SnapshotMeta {
        patch_version: patch_version.to_string(),
        source: source.to_string(),
        created_at: chrono::Utc::now().timestamp(),
        file_count: index.files.len() as u64,
        bundle_count: index.bundles.len() as u64,
    };
    save_snapshot(&meta, index)
}

pub fn read_snapshot_meta(path: &Path) -> io::Result<SnapshotMeta> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    bincode::deserialize_from(&mut reader).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

pub fn load_snapshot(path: &Path) -> io::Result<(SnapshotMeta, Index)> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    bincode::deserialize_from(&mut reader).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// All saved snapshots, newest first. Unreadable files are skipped.
pub fn list_snapshots() -> Vec<(PathBuf, SnapshotMeta)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(snapshot_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(SNAPSHOT_EXT) {
                continue;
            }
            if let Ok(meta) = read_snapshot_meta(&path) {
                out.push((path, meta));
            }
        }
    }
    out.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
    out
}

pub fn has_snapshot_for_version(patch_version: &str) -> bool {
    list_snapshots().iter().any(|(_, m)| m.patch_version == patch_version)
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub path_hash: u64,
    pub path: String,
    pub old_size: Option<u32>,
    pub new_size: Option<u32>,
}

impl DiffEntry {
    pub fn display_path(&self) -> String {
        if self.path.is_empty() {
            format!("<unresolved {:016x}>", self.path_hash)
        } else {
            self.path.clone()
        }
    }
}

#[derive(Debug, Default)]
pub struct DiffResult {
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub modified: Vec<DiffEntry>,
    /// Same path and size, but the file's bundle placement changed — the
    /// containing bundle was repacked, so the content *may* have changed.
    pub touched: Vec<DiffEntry>,
}

impl DiffResult {
    pub fn total(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len() + self.touched.len()
    }
}

fn is_loose(f: &crate::bundles::index::FileInfo) -> bool {
    f.bundle_index == GGPK_LOOSE_FILE_SENTINEL || f.bundle_index == LOOSE_FILE_SENTINEL
}

/// Compares two index versions. Shader-cache blobs are always ignored; loose
/// (non-bundled) files are only compared when both sides recorded them,
/// otherwise an auto-snapshot taken from the index cache (which never contains
/// loose entries) would report every loose file as "added".
pub fn diff_indexes(old: &Index, new: &Index) -> DiffResult {
    let include_loose = old.files.values().any(is_loose) && new.files.values().any(is_loose);
    let skip = |f: &crate::bundles::index::FileInfo| -> bool {
        (!include_loose && is_loose(f)) || is_shader_cache_path(&f.path)
    };

    let bundle_of = |index: &Index, f: &crate::bundles::index::FileInfo| -> Option<(String, u32)> {
        index
            .bundles
            .get(f.bundle_index as usize)
            .map(|b| (b.name.clone(), b.uncompressed_size))
    };

    let mut result = DiffResult::default();

    for (hash, nf) in &new.files {
        if skip(nf) {
            continue;
        }
        match old.files.get(hash).filter(|of| !skip(of)) {
            None => result.added.push(DiffEntry {
                path_hash: *hash,
                path: nf.path.clone(),
                old_size: None,
                new_size: Some(nf.file_size),
            }),
            Some(of) => {
                let entry = DiffEntry {
                    path_hash: *hash,
                    path: if nf.path.is_empty() { of.path.clone() } else { nf.path.clone() },
                    old_size: Some(of.file_size),
                    new_size: Some(nf.file_size),
                };
                if of.file_size != nf.file_size {
                    result.modified.push(entry);
                } else if !is_loose(of) && !is_loose(nf) {
                    let moved = of.file_offset != nf.file_offset
                        || bundle_of(old, of) != bundle_of(new, nf);
                    if moved {
                        result.touched.push(entry);
                    }
                }
            }
        }
    }

    for (hash, of) in &old.files {
        if skip(of) {
            continue;
        }
        if new.files.get(hash).filter(|nf| !skip(nf)).is_none() {
            result.removed.push(DiffEntry {
                path_hash: *hash,
                path: of.path.clone(),
                old_size: Some(of.file_size),
                new_size: None,
            });
        }
    }

    let sort_key = |e: &DiffEntry| (e.path.is_empty(), e.path.clone(), e.path_hash);
    result.added.sort_by_key(sort_key);
    result.removed.sort_by_key(sort_key);
    result.modified.sort_by_key(sort_key);
    result.touched.sort_by_key(sort_key);

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::index::{BundleInfo, FileInfo};
    use std::collections::HashMap;

    fn make_index(bundles: Vec<(&str, u32)>, files: Vec<(u64, u32, u32, u32, &str)>) -> Index {
        let bundles = bundles
            .into_iter()
            .map(|(name, size)| BundleInfo { name: name.to_string(), uncompressed_size: size })
            .collect();
        let mut map = HashMap::new();
        for (hash, bundle_index, offset, size, path) in files {
            map.insert(
                hash,
                FileInfo {
                    path_hash: hash,
                    bundle_index,
                    file_offset: offset,
                    file_size: size,
                    path: path.to_string(),
                },
            );
        }
        Index { bundles, files: map }
    }

    #[test]
    fn diff_detects_all_categories() {
        let old = make_index(
            vec![("a.bundle.bin", 1000), ("b.bundle.bin", 2000)],
            vec![
                (1, 0, 0, 100, "data/kept.dat"),
                (2, 0, 100, 50, "data/resized.dat"),
                (3, 1, 0, 75, "data/gone.dat"),
                (4, 1, 75, 25, "data/moved.txt"),
            ],
        );
        // Bundle b repacked (new size), moved.txt shifted offset; gone.dat
        // removed; fresh.dat added; resized.dat grew.
        let new = make_index(
            vec![("a.bundle.bin", 1000), ("b.bundle.bin", 1800)],
            vec![
                (1, 0, 0, 100, "data/kept.dat"),
                (2, 0, 100, 60, "data/resized.dat"),
                (4, 1, 0, 25, "data/moved.txt"),
                (5, 1, 25, 10, "data/fresh.dat"),
            ],
        );

        let diff = diff_indexes(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].path, "data/fresh.dat");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].path, "data/gone.dat");
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].path, "data/resized.dat");
        assert_eq!(diff.modified[0].old_size, Some(50));
        assert_eq!(diff.modified[0].new_size, Some(60));
        assert_eq!(diff.touched.len(), 1);
        assert_eq!(diff.touched[0].path, "data/moved.txt");
    }

    #[test]
    fn diff_skips_loose_when_only_one_side_has_them() {
        let old = make_index(vec![("a.bundle.bin", 100)], vec![(1, 0, 0, 10, "data/x.dat")]);
        let new = make_index(
            vec![("a.bundle.bin", 100)],
            vec![
                (1, 0, 0, 10, "data/x.dat"),
                (2, GGPK_LOOSE_FILE_SENTINEL, 0, 500, "FMOD/audio.bank"),
            ],
        );
        let diff = diff_indexes(&old, &new);
        assert_eq!(diff.total(), 0);
    }

    #[test]
    fn diff_compares_loose_sizes_when_both_sides_have_them() {
        let old = make_index(vec![], vec![(2, GGPK_LOOSE_FILE_SENTINEL, 0, 500, "FMOD/audio.bank")]);
        let new = make_index(vec![], vec![(2, GGPK_LOOSE_FILE_SENTINEL, 0, 600, "FMOD/audio.bank")]);
        let diff = diff_indexes(&old, &new);
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.total(), 1);
    }

    #[test]
    fn diff_ignores_shader_cache() {
        let old = make_index(vec![("a.bundle.bin", 100)], vec![]);
        let new = make_index(
            vec![("a.bundle.bin", 100)],
            vec![(9, 0, 0, 10, "shadercachedx12/ab/somehash")],
        );
        let diff = diff_indexes(&old, &new);
        assert_eq!(diff.total(), 0);
    }

    #[test]
    fn snapshot_meta_reads_without_full_index() {
        let index = make_index(vec![("a.bundle.bin", 100)], vec![(1, 0, 0, 10, "data/x.dat")]);
        let meta = SnapshotMeta {
            patch_version: "1.2.3".to_string(),
            source: "test".to_string(),
            created_at: 1_700_000_000,
            file_count: 1,
            bundle_count: 1,
        };
        let bytes = bincode::serialize(&(&meta, &index)).unwrap();
        let head: SnapshotMeta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(head.patch_version, "1.2.3");
        let (meta2, index2): (SnapshotMeta, Index) = bincode::deserialize(&bytes).unwrap();
        assert_eq!(meta2.file_count, 1);
        assert_eq!(index2.files.len(), 1);
    }
}

#[cfg(test)]
mod real_snapshot_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Diffs the two newest saved snapshots and prints a patch report, with the
    /// `data/` tables (the ones the schema has to keep up with) broken out.
    /// Run with: cargo test --release patch_report_real_snapshots -- --ignored --nocapture
    #[test]
    #[ignore]
    fn patch_report_real_snapshots() {
        let snaps = list_snapshots();
        assert!(snaps.len() >= 2, "need two snapshots, found {}", snaps.len());
        let (new_path, new_meta) = &snaps[0];
        let (old_path, old_meta) = &snaps[1];
        println!(
            "old {} ({}, {} files)\nnew {} ({}, {} files)",
            old_meta.patch_version, old_meta.created_at_label(), old_meta.file_count,
            new_meta.patch_version, new_meta.created_at_label(), new_meta.file_count
        );

        let (_, old_index) = load_snapshot(old_path).unwrap();
        let (_, new_index) = load_snapshot(new_path).unwrap();
        let diff = diff_indexes(&old_index, &new_index);
        println!(
            "\nadded {}  removed {}  modified {}  touched {}  (total {})",
            diff.added.len(), diff.removed.len(), diff.modified.len(), diff.touched.len(), diff.total()
        );

        let top = |p: &str| p.split('/').next().unwrap_or("<root>").to_string();
        let mut folders: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for (slot, list) in [(0, &diff.added), (1, &diff.removed), (2, &diff.modified), (3, &diff.touched)] {
            for e in list {
                folders.entry(top(&e.display_path())).or_insert([0; 4])[slot] += 1;
            }
        }
        let mut rows: Vec<_> = folders.into_iter().collect();
        rows.sort_by_key(|(_, c)| std::cmp::Reverse(c.iter().sum::<usize>()));
        println!("\n{:<28} {:>8} {:>8} {:>8} {:>8}", "top folder", "added", "removed", "modif", "touched");
        for (folder, c) in rows.iter().take(25) {
            println!("{:<28} {:>8} {:>8} {:>8} {:>8}", folder, c[0], c[1], c[2], c[3]);
        }

        let is_data = |p: &str| p.starts_with("data/") && (p.ends_with(".datc64") || p.ends_with(".dat64") || p.ends_with(".dat"));
        println!("\n--- data tables ---");
        for (label, list) in [("added", &diff.added), ("removed", &diff.removed), ("modified", &diff.modified)] {
            let hits: Vec<String> = list.iter().map(|e| e.display_path()).filter(|p| is_data(p)).collect();
            println!("{} ({}):", label, hits.len());
            for p in &hits {
                println!("  {}", p);
            }
        }
        let touched_data = diff.touched.iter().map(|e| e.display_path()).filter(|p| is_data(p)).count();
        println!("touched (repacked, content may be unchanged): {}", touched_data);
    }
}
