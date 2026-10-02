//! What a new patch moved, put right without waiting for the community schema:
//! keep the patch's tables, check every table the schema describes, re-fit the
//! broken ones from the last kept patch, and retire earlier re-fits once the
//! community schema describes those tables again.

use crate::dat::analysis::check_fit;
use crate::dat::overrides::Overrides;
use crate::dat::reader::DatReader;
use crate::dat::refit::{is_refit, refit_file, RefitError};
use crate::dat::relational::FileSource;
use crate::dat::schema::Schema;
use crate::dat::table_store::{self, table_paths, StoredTables};
use crate::data_export::source::GameFiles;
use crate::settings::Game;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Report {
    pub version: String,
    /// The kept patch broken tables were re-fitted from.
    pub previous: Option<String>,
    /// Tables kept this run (none when the patch was already kept).
    pub kept: Option<usize>,
    pub checked: usize,
    /// Tables the schema in use misreads on this patch.
    pub broken: Vec<String>,
    /// Re-fits that read cleanly, with columns they could not place.
    pub refitted: Vec<(String, Vec<String>)>,
    /// Broken tables left broken, and why.
    pub unresolved: Vec<(String, String)>,
    /// Re-fits dropped because the community schema reads the table again.
    pub retired: Vec<String>,
    /// Whether the re-fits were stored as overrides.
    pub written: bool,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.unresolved.is_empty() && self.refitted.iter().all(|(_, lost)| lost.is_empty())
    }

    /// One line for a status bar.
    pub fn headline(&self) -> String {
        let mut line = format!("Patch {}: {} tables checked", self.version, self.checked);
        match (self.broken.len(), self.refitted.len(), self.unresolved.len()) {
            (0, _, _) => line.push_str(", nothing moved"),
            (broken, fixed, open) => {
                line.push_str(&format!(", {} moved", broken));
                if fixed > 0 {
                    let from = self.previous.as_deref().unwrap_or("the last patch");
                    let stored = if self.written { "" } else { " (not saved)" };
                    line.push_str(&format!(", {} re-fitted from {}{}", fixed, from, stored));
                }
                if open > 0 {
                    line.push_str(&format!(", {} still broken", open));
                }
            }
        }
        if !self.retired.is_empty() {
            line.push_str(&format!(", {} re-fit(s) retired", self.retired.len()));
        }
        line
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![self.headline()];
        if let Some(n) = self.kept {
            out.push(format!("Kept {} table(s) of patch {}", n, self.version));
        }
        for name in &self.retired {
            out.push(format!("  retired {}: the community schema reads it again", name));
        }
        for (name, lost) in &self.refitted {
            match lost.is_empty() {
                true => out.push(format!("  re-fitted {}", name)),
                false => out.push(format!("  re-fitted {}, but could not place {}", name, lost.join(", "))),
            }
        }
        for (name, why) in &self.unresolved {
            out.push(format!("  still broken {}: {}", name, why));
        }
        out
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": self.version,
            "previous": self.previous,
            "checked": self.checked,
            "broken": self.broken,
            "refitted": self.refitted.iter().map(|(n, lost)| serde_json::json!({ "table": n, "lost": lost })).collect::<Vec<_>>(),
            "unresolved": self.unresolved.iter().map(|(n, why)| serde_json::json!({ "table": n, "reason": why })).collect::<Vec<_>>(),
            "retired": self.retired,
            "written": self.written,
        })
    }
}

/// Where a patch's check is recorded, beside its kept tables.
pub fn report_path(game: Game, version: &str) -> PathBuf {
    table_store::store_dir(game).join(version).join("patch_check.json")
}

pub fn was_checked(game: Game, version: &str) -> bool {
    report_path(game, version).exists()
}

/// Checks `version` (read through `files`) against `community` plus the
/// overrides at `overrides_path`, re-fitting what broke. With `write`, re-fits
/// that read cleanly and retirements are saved to the overrides file.
pub fn run(
    files: &GameFiles,
    game: Game,
    version: &str,
    community: &Schema,
    overrides_path: &Path,
    write: bool,
) -> Result<Report, String> {
    let kept = table_store::ensure_saved(files, game, version)?;
    let tables = files.fetch_many(&table_paths(files, game));
    let previous = table_store::previous(game, version);
    let old = previous.as_deref().and_then(|v| StoredTables::open(game, v));
    let old = previous.zip(old.as_ref().map(|o| o as &dyn FileSource));
    let mut report = check(&tables, game, version, community, overrides_path, write, old)?;
    report.kept = kept;
    let record = report_path(game, version);
    if let Some(dir) = record.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&record, serde_json::to_string_pretty(&report.to_json()).unwrap_or_default());
    Ok(report)
}

/// The checks and re-fits over one patch's top-level tables (path to bytes),
/// re-fitting from `old` — a patch version and its tables — when given.
#[allow(clippy::too_many_arguments)]
pub fn check(
    tables: &HashMap<String, Vec<u8>>,
    game: Game,
    version: &str,
    community: &Schema,
    overrides_path: &Path,
    write: bool,
    old: Option<(String, &dyn FileSource)>,
) -> Result<Report, String> {
    let is_poe2 = game.is_poe2();
    let mut report = Report { version: version.to_string(), written: write, ..Default::default() };
    let mut paths: Vec<&String> = tables.keys().collect();
    paths.sort();
    let dat = |path: &str| -> Option<DatReader> {
        let bytes = tables.get(path).or_else(|| tables.iter().find(|(p, _)| p.eq_ignore_ascii_case(path)).map(|(_, b)| b))?;
        DatReader::new(bytes.clone(), path).ok()
    };
    let file_of = |name: &str| format!("{}{}.datc64", table_store::table_dir(game), name.to_ascii_lowercase());

    let mut overrides = Overrides::load(overrides_path);
    let mut changed = false;

    // A re-fit stands in for the community schema only until it catches up.
    for name in overrides.tables.iter().filter(|t| is_refit(t)).map(|t| t.name.clone()).collect::<Vec<_>>() {
        let Some(def) = community.find_table(&name, is_poe2) else { continue };
        let Some(reader) = dat(&file_of(&name)) else { continue };
        if !check_fit(&reader, def, 40).is_broken() && overrides.remove(&name, is_poe2) {
            report.retired.push(name);
            changed = true;
        }
    }

    let mut schema = community.clone();
    schema.apply_overrides(&overrides.tables);
    let mut broken = Vec::new();
    for &path in &paths {
        let lower = path.to_ascii_lowercase();
        let stem = lower.rsplit('/').next().unwrap_or(&lower).trim_end_matches(".datc64");
        let Some(def) = schema.find_table(stem, is_poe2) else { continue };
        let Some(reader) = dat(path) else { continue };
        if reader.row_count < 4 {
            continue;
        }
        report.checked += 1;
        if check_fit(&reader, def, 40).is_broken() {
            broken.push((def.clone(), path.clone()));
        }
    }
    report.broken = broken.iter().map(|(d, _)| d.name.clone()).collect();
    report.broken.sort();

    report.previous = old.as_ref().map(|(v, _)| v.clone());
    for (def, path) in broken {
        let Some((_, old)) = &old else {
            report.unresolved.push((def.name.clone(), "no earlier patch is kept to re-fit from".to_string()));
            continue;
        };
        let (Some(old_bytes), Some(new_bytes)) = (old.fetch(&path), tables.get(&path).cloned()) else {
            report.unresolved.push((def.name.clone(), "not in both patches".to_string()));
            continue;
        };
        match refit_file(old_bytes, new_bytes, &path, &def) {
            Ok(refitted) if !refitted.after.is_broken() => {
                report.refitted.push((def.name.clone(), refitted.report.lost.clone()));
                if write {
                    overrides.upsert(refitted.report.table);
                    changed = true;
                }
            }
            Ok(refitted) => report.unresolved.push((def.name.clone(), format!("the re-fit still misreads: {}", refitted.after.summary()))),
            Err(RefitError::OldDoesNotFit) => report.unresolved.push((
                def.name.clone(),
                format!("the schema did not fit patch {} either", report.previous.as_deref().unwrap_or("?")),
            )),
            Err(RefitError::Failed(e)) => report.unresolved.push((def.name.clone(), e)),
        }
    }
    report.refitted.sort();
    report.unresolved.sort();

    if write && changed {
        overrides.save(overrides_path).map_err(|e| format!("{}: {}", overrides_path.display(), e))?;
    }
    Ok(report)
}

/// An export file whose size moved far more than a patch moves data. A stale
/// layout that slips past the fit check shows up here: `mods_by_base.json`
/// once went from 5.5 MB to 93 MB.
#[derive(Debug)]
pub struct SizeChange {
    pub file: String,
    pub before: u64,
    pub after: Option<u64>,
}

/// Compares two export folders file by file.
pub fn compare_exports(before: &Path, after: &Path) -> Vec<SizeChange> {
    const MIN_BYTES: u64 = 16 * 1024;
    const MAX_RATIO: f64 = 1.5;
    let mut changes = Vec::new();
    for entry in walkdir::WalkDir::new(before).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let Ok(rel) = entry.path().strip_prefix(before) else { continue };
        let name = rel.to_string_lossy().replace('\\', "/");
        if name == "export_report.json" || name.ends_with(".log") {
            continue;
        }
        let old = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let new = std::fs::metadata(after.join(rel)).ok().map(|m| m.len());
        let moved = match new {
            None => true,
            Some(new) => {
                let (lo, hi) = (old.min(new).max(1) as f64, old.max(new) as f64);
                old.max(new) >= MIN_BYTES && hi / lo > MAX_RATIO
            }
        };
        if moved {
            changes.push(SizeChange { file: name, before: old, after: new });
        }
    }
    changes.sort_by(|a, b| a.file.cmp(&b.file));
    changes
}

#[cfg(test)]
mod real_data_tests {
    use super::*;
    use crate::settings::AppSettings;

    /// A patch that inserts a 4-byte column into the middle of `Stats` rows:
    /// the check must flag it, re-fit it from the kept copy, store the
    /// re-fit, and retire it once the table reads with the community schema
    /// again. Uses the newest kept patch; no install needed.
    /// `cargo test --release patch_check_refits_a_moved_table -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn patch_check_refits_a_moved_table() {
        let game = Game::Poe2;
        let version = table_store::versions(game).into_iter().next().expect("no kept patch; run export-data once");
        let stored = StoredTables::open(game, &version).unwrap();
        let settings = AppSettings::load();
        let path = settings.schema_local_path.map(PathBuf::from).unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
        let community: Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

        let file = "data/balance/stats.datc64".to_string();
        let original = stored.fetch(&file).unwrap();
        let reader = DatReader::new(original.clone(), &file).unwrap();
        let (rows, row_len) = (reader.row_count as usize, reader.row_length.unwrap());
        let marker = 4 + rows * row_len;
        // After Id (8) and the u16 hash, where a new column would go.
        let insert_at = 10;
        let mut moved = Vec::with_capacity(original.len() + rows * 4);
        moved.extend_from_slice(&original[..4]);
        for r in 0..rows {
            let row = &original[4 + r * row_len..4 + (r + 1) * row_len];
            moved.extend_from_slice(&row[..insert_at]);
            moved.extend_from_slice(&(r as u32).to_le_bytes());
            moved.extend_from_slice(&row[insert_at..]);
        }
        moved.extend_from_slice(&original[marker..]);

        let dir = std::env::temp_dir().join(format!("ggpk-patch-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let overrides = dir.join("overrides.json");
        let new_patch: HashMap<String, Vec<u8>> = [(file.clone(), moved)].into();
        let report = check(&new_patch, game, "9.9.9", &community, &overrides, true, Some((version.clone(), &stored as &dyn FileSource))).unwrap();
        for line in report.lines() {
            println!("{}", line);
        }
        assert_eq!(report.broken, vec!["Stats".to_string()]);
        assert_eq!(report.refitted.len(), 1, "Stats should re-fit from {}", version);
        let saved = Overrides::load(&overrides);
        assert!(saved.tables.iter().any(|t| t.name == "Stats" && is_refit(t)));

        // The community schema reads the unmoved file, so the re-fit retires.
        let caught_up: HashMap<String, Vec<u8>> = [(file, original)].into();
        let report = check(&caught_up, game, "9.9.10", &community, &overrides, true, None).unwrap();
        assert_eq!(report.retired, vec!["Stats".to_string()]);
        assert!(Overrides::load(&overrides).tables.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
