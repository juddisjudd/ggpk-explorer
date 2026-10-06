//! Every patch's DAT tables, kept on disk. The patch CDN serves only the live
//! patch, so once the next one ships this is the only copy left to re-fit the
//! new layouts against.

use crate::data_export::source::GameFiles;
use crate::settings::{AppSettings, Game};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// How many patches to keep. One patch's tables are about 15 MB compressed.
const KEEP: usize = 8;
const COMPLETE: &str = "complete";
const EXT: &str = ".datc64.zst";

/// Where a game keeps its top-level tables; language copies sit one folder deeper.
pub fn table_dir(game: Game) -> &'static str {
    match game {
        Game::Poe2 => "data/balance/",
        Game::Poe1 => "data/",
    }
}

/// Outside `cache/`, so the wipe on a patch change leaves it alone.
pub fn store_dir(game: Game) -> PathBuf {
    AppSettings::cache_dir(game).join("tables")
}

fn version_dir(game: Game, version: &str) -> PathBuf {
    store_dir(game).join(version)
}

pub fn has(game: Game, version: &str) -> bool {
    version_dir(game, version).join(COMPLETE).exists()
}

/// Stored patches, newest first.
pub fn versions(game: Game) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(store_dir(game)) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join(COMPLETE).exists())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    out.sort_by(|a, b| compare_versions(game, b, a));
    out
}

/// The newest stored patch older than `current`.
pub fn previous(game: Game, current: &str) -> Option<String> {
    versions(game).into_iter().find(|v| compare_versions(game, v, current) == Ordering::Less)
}

/// Orders patches numerically, segment by segment; PoE 2's old 4.x ones sort just before the 0.x they became.
pub fn compare_versions(game: Game, a: &str, b: &str) -> Ordering {
    let key = |s: &str| {
        let old_poe2 = game == Game::Poe2 && s.starts_with("4.");
        let mut parts = s.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
        if old_poe2 {
            parts[0] = 0;
        }
        (parts, !old_poe2)
    };
    key(a).cmp(&key(b))
}

/// The top-level table paths `files` holds.
pub fn table_paths(files: &GameFiles, game: Game) -> Vec<String> {
    let dir = table_dir(game);
    files
        .list_dir(dir)
        .into_iter()
        .filter(|p| {
            let rest = &p.to_ascii_lowercase()[dir.len()..];
            !rest.contains('/') && rest.ends_with(".datc64")
        })
        .collect()
}

/// Saves `version`'s tables unless they are already stored. Returns how many
/// were written, or `None` when the patch was already there.
pub fn ensure_saved(files: &GameFiles, game: Game, version: &str) -> Result<Option<usize>, String> {
    if version.is_empty() || has(game, version) {
        return Ok(None);
    }
    save(files, game, version).map(Some)
}

pub fn save(files: &GameFiles, game: Game, version: &str) -> Result<usize, String> {
    let dir = version_dir(game, version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let tables = files.fetch_many(&table_paths(files, game));
    if tables.is_empty() {
        return Err(format!("no tables under {} to save", table_dir(game)));
    }
    for (path, bytes) in &tables {
        let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        let packed = zstd::bulk::compress(bytes, 3).map_err(|e| format!("{}: {}", name, e))?;
        let file = dir.join(format!("{}.zst", name));
        std::fs::write(&file, packed).map_err(|e| format!("{}: {}", file.display(), e))?;
    }
    std::fs::write(dir.join(COMPLETE), tables.len().to_string()).map_err(|e| format!("{}: {}", dir.display(), e))?;
    prune(game);
    Ok(tables.len())
}

fn prune(game: Game) {
    for old in versions(game).into_iter().skip(KEEP) {
        let _ = std::fs::remove_dir_all(version_dir(game, &old));
    }
}

/// One stored patch, read back as a file source.
pub struct StoredTables {
    dir: PathBuf,
}

impl StoredTables {
    pub fn open(game: Game, version: &str) -> Option<Self> {
        has(game, version).then(|| Self { dir: version_dir(game, version) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl crate::dat::relational::FileSource for StoredTables {
    fn fetch(&self, path: &str) -> Option<Vec<u8>> {
        let name = path.rsplit('/').next()?.to_ascii_lowercase();
        let name = name.strip_suffix(".datc64").unwrap_or(&name);
        let packed = std::fs::read(self.dir.join(format!("{}{}", name, EXT))).ok()?;
        zstd::stream::decode_all(&packed[..]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::compare_versions;
    use crate::settings::Game;
    use std::cmp::Ordering;

    #[test]
    fn versions_order_by_number() {
        assert_eq!(compare_versions(Game::Poe2, "4.5.5.1.5", "4.5.5.2"), Ordering::Less);
        assert_eq!(compare_versions(Game::Poe2, "0.5.10", "0.5.9"), Ordering::Greater);
        assert_eq!(compare_versions(Game::Poe1, "3.29.3.3", "3.29.3.3"), Ordering::Equal);
    }

    #[test]
    fn old_poe2_numbers_sort_before_what_they_became() {
        assert_eq!(compare_versions(Game::Poe2, "4.5.5.4", "0.5.5.4"), Ordering::Less);
        assert_eq!(compare_versions(Game::Poe2, "4.5.5.4", "0.5.5.5"), Ordering::Less);
        assert_eq!(compare_versions(Game::Poe2, "0.5.5.4", "4.5.5.3"), Ordering::Greater);
        assert_eq!(compare_versions(Game::Poe2, "4.5.5.4", "1.0.0.1"), Ordering::Less);
        assert_eq!(compare_versions(Game::Poe1, "4.0.0.1", "3.29.3.3"), Ordering::Greater);
    }
}
