//! What a new patch moved, put right without waiting for the community schema:
//! keep the patch's tables, check every table the schema describes, re-fit the
//! broken ones from the last kept patch, and retire earlier re-fits once the
//! community schema describes those tables again.

use crate::dat::analysis::check_fit;
use crate::dat::exe_layout::{self, ClientLayouts};
use crate::dat::overrides::Overrides;
use crate::dat::reader::DatReader;
use crate::dat::refit::{is_refit, refit_file, repair_from_client, RefitError};
use crate::dat::relational::FileSource;
use crate::dat::schema::{game_mask, Schema, Table};
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
    /// The re-fits that came from the game client's reading alone, with no earlier patch to carry from.
    pub repaired: Vec<String>,
    /// Broken tables left broken, and why.
    pub unresolved: Vec<(String, String)>,
    /// Re-fits dropped because the community schema reads the table again.
    pub retired: Vec<String>,
    /// Whether the re-fits were stored as overrides.
    pub written: bool,
    /// How many table layouts were read out of the game client, or why none were.
    pub client: Option<Result<usize, String>>,
    /// Tables the client reads differently from the layout in use, with each difference.
    pub disputed: Vec<(String, Vec<String>)>,
    /// Disputed tables with a finding the check of `previous` did not list; `None` when that patch was never checked.
    pub new_disputes: Option<Vec<String>>,
    /// Tables whose rows are not the size the client expects: the client is from another patch.
    pub mismatched: usize,
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
                let stored = if self.written { "" } else { " (not saved)" };
                let carried = fixed - self.repaired.len().min(fixed);
                if carried > 0 {
                    let from = self.previous.as_deref().unwrap_or("the last patch");
                    line.push_str(&format!(", {} re-fitted from {}{}", carried, from, stored));
                }
                if !self.repaired.is_empty() {
                    line.push_str(&format!(", {} repaired from the game client{}", self.repaired.len(), stored));
                }
                if open > 0 {
                    line.push_str(&format!(", {} still broken", open));
                }
            }
        }
        if !self.retired.is_empty() {
            line.push_str(&format!(", {} re-fit(s) retired", self.retired.len()));
        }
        if !self.disputed.is_empty() {
            line.push_str(&format!(", {} read differently by the client", self.disputed.len()));
            if let (Some(new), Some(before)) = (&self.new_disputes, &self.previous) {
                match new.len() {
                    0 => line.push_str(&format!(" (none new since {})", before)),
                    n => line.push_str(&format!(" ({} new since {})", n, before)),
                }
            }
        }
        line
    }

    pub fn is_new_dispute(&self, table: &str) -> bool {
        self.new_disputes.as_ref().is_some_and(|new| new.iter().any(|t| t == table))
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
            let how = if self.repaired.contains(name) { "repaired from the game client" } else { "re-fitted" };
            match lost.is_empty() {
                true => out.push(format!("  {} {}", how, name)),
                false => out.push(format!("  {} {}, but could not place {}", how, name, lost.join(", "))),
            }
        }
        for (name, why) in &self.unresolved {
            out.push(format!("  still broken {}: {}", name, why));
        }
        match &self.client {
            Some(Ok(n)) => out.push(format!("Read {} table layout(s) out of the game client", n)),
            Some(Err(why)) => out.push(format!("No table layouts from the game client: {}", why)),
            None => {}
        }
        if self.mismatched > 0 {
            out.push(format!("  {} table(s) have rows of a size the client does not expect; is it from another patch?", self.mismatched));
        }
        for (name, found) in &self.disputed {
            let new = if self.is_new_dispute(name) { " (new)" } else { "" };
            out.push(format!("  the client reads {} differently{}:", name, new));
            out.extend(found.iter().map(|line| format!("    {}", line)));
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
            "repaired": self.repaired,
            "unresolved": self.unresolved.iter().map(|(n, why)| serde_json::json!({ "table": n, "reason": why })).collect::<Vec<_>>(),
            "retired": self.retired,
            "written": self.written,
            "client_tables": self.client.as_ref().and_then(|c| c.as_ref().ok()),
            "client_error": self.client.as_ref().and_then(|c| c.as_ref().err()),
            "client_mismatched": self.mismatched,
            "disputed": self.disputed.iter().map(|(n, found)| serde_json::json!({ "table": n, "findings": found })).collect::<Vec<_>>(),
            "disputed_new": self.new_disputes,
        })
    }

    /// Reads a record back; one written before the client's layouts were read simply has none.
    fn from_json(value: &serde_json::Value) -> Option<Report> {
        let texts = |v: &serde_json::Value| v.as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
        let rows = |key: &str| value[key].as_array().cloned().unwrap_or_default();
        let table = |row: &serde_json::Value| row["table"].as_str().unwrap_or_default().to_string();
        let client = match (value["client_tables"].as_u64(), value["client_error"].as_str()) {
            (Some(n), _) => Some(Ok(n as usize)),
            (None, Some(why)) => Some(Err(why.to_string())),
            (None, None) => None,
        };
        Some(Report {
            version: value["version"].as_str()?.to_string(),
            previous: value["previous"].as_str().map(str::to_string),
            kept: None,
            checked: value["checked"].as_u64()? as usize,
            broken: texts(&value["broken"]),
            refitted: rows("refitted").iter().map(|r| (table(r), texts(&r["lost"]))).collect(),
            repaired: texts(&value["repaired"]),
            unresolved: rows("unresolved").iter().map(|r| (table(r), r["reason"].as_str().unwrap_or_default().to_string())).collect(),
            retired: texts(&value["retired"]),
            written: value["written"].as_bool().unwrap_or(false),
            client,
            disputed: rows("disputed").iter().map(|r| (table(r), texts(&r["findings"]))).collect(),
            new_disputes: value["disputed_new"].as_array().map(|_| texts(&value["disputed_new"])),
            mismatched: value["client_mismatched"].as_u64().unwrap_or(0) as usize,
        })
    }

    /// Disputed tables with a finding that `before` did not list.
    fn disputes_new_since(&self, before: &Report) -> Vec<String> {
        self.disputed
            .iter()
            .filter(|(name, found)| match before.disputed.iter().find(|(n, _)| n == name) {
                Some((_, old)) => found.iter().any(|f| !old.contains(f)),
                None => true,
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn compare_with_previous(&mut self, game: Game) {
        if let Some(before) = self.previous.as_deref().and_then(|v| read(game, v)) {
            self.new_disputes = Some(self.disputes_new_since(&before));
        }
    }
}

fn read(game: Game, version: &str) -> Option<Report> {
    let text = std::fs::read_to_string(report_path(game, version)).ok()?;
    Report::from_json(&serde_json::from_str(&text).ok()?)
}

/// The last check recorded for a patch.
pub fn load(game: Game, version: &str) -> Option<Report> {
    let mut report = read(game, version)?;
    if report.new_disputes.is_none() {
        report.compare_with_previous(game);
    }
    Some(report)
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
    let client = exe_layout::for_patch(files, game, version);
    let mut report = check(&tables, game, version, community, overrides_path, write, old, client.as_ref().ok())?;
    report.kept = kept;
    report.client = Some(client.map(|c| c.tables.len()));
    report.compare_with_previous(game);
    let record = report_path(game, version);
    if let Some(dir) = record.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&record, serde_json::to_string_pretty(&report.to_json()).unwrap_or_default());
    Ok(report)
}

/// Checks one patch's top-level tables (path to bytes), re-fits the broken ones from `old` (a patch version and its tables) and holds each layout against `client`.
#[allow(clippy::too_many_arguments)]
pub fn check(
    tables: &HashMap<String, Vec<u8>>,
    game: Game,
    version: &str,
    community: &Schema,
    overrides_path: &Path,
    write: bool,
    old: Option<(String, &dyn FileSource)>,
    client: Option<&ClientLayouts>,
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
    let mut mismatched = 0;
    let mut disputes = |def: &Table, row_len: Option<usize>| -> Option<(String, Vec<String>)> {
        let layout = client?.find(&def.name)?;
        if row_len? != layout.row_size {
            mismatched += 1;
            return None;
        }
        let found = client?.disagreements(def, layout);
        (!found.is_empty()).then(|| (def.name.clone(), found))
    };
    let mut broken = Vec::new();
    for &path in &paths {
        let lower = path.to_ascii_lowercase();
        let stem = lower.rsplit('/').next().unwrap_or(&lower).trim_end_matches(".datc64");
        let Some(def) = schema.find_table(stem, is_poe2) else { continue };
        let Some(reader) = dat(path) else { continue };
        // The fit check needs a few rows to judge by; the client's layout does not.
        let judged = reader.row_count >= 4;
        report.checked += usize::from(judged);
        if judged && check_fit(&reader, def, 40).is_broken() {
            broken.push((def.clone(), path.clone()));
        } else {
            report.disputed.extend(disputes(def, reader.row_length.filter(|_| reader.row_count > 0)));
        }
    }
    report.broken = broken.iter().map(|(d, _)| d.name.clone()).collect();
    report.broken.sort();

    report.previous = old.as_ref().map(|(v, _)| v.clone());
    for (def, path) in broken {
        let new_bytes = tables.get(&path).cloned();
        let carried = match (&old, new_bytes.clone()) {
            (None, _) => Err("no earlier patch is kept to re-fit from".to_string()),
            (Some((_, old)), new_bytes) => match old.fetch(&path).zip(new_bytes).map(|(old_bytes, new_bytes)| refit_file(old_bytes, new_bytes, &path, &def, client)) {
                None => Err("not in both patches".to_string()),
                Some(Ok(refitted)) if !refitted.after.is_broken() => Ok(refitted),
                Some(Ok(refitted)) => Err(format!("the re-fit still misreads: {}", refitted.after.summary())),
                Some(Err(RefitError::OldDoesNotFit)) => Err(format!("the schema did not fit patch {} either", report.previous.as_deref().unwrap_or("?"))),
                Some(Err(RefitError::Failed(e))) => Err(e),
            },
        };
        // With no earlier copy that reads right, the client's own reading of the table is what is left.
        let refitted = carried.or_else(|why| match client.zip(new_bytes).map(|(client, bytes)| repair_from_client(bytes, &path, &def, client)) {
            Some(Ok(repaired)) if !repaired.after.is_broken() => {
                report.repaired.push(def.name.clone());
                Ok(repaired)
            }
            _ => Err(why),
        });
        match refitted {
            Ok(refitted) => {
                report.refitted.push((def.name.clone(), refitted.report.lost.clone()));
                report.disputed.extend(disputes(&refitted.report.table, Some(refitted.report.new_row_len)));
                if write {
                    // A layout worked out from one game's file says nothing about the other game's copy of a shared table.
                    overrides.upsert(Table { valid_for: Some(game_mask(is_poe2)), ..refitted.report.table });
                    changed = true;
                }
            }
            Err(why) => report.unresolved.push((def.name.clone(), why)),
        }
    }
    report.repaired.sort();
    report.refitted.sort();
    report.unresolved.sort();
    report.disputed.sort();
    report.mismatched = mismatched;

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
mod tests {
    use super::*;

    #[test]
    fn a_record_reads_back_as_it_was_written() {
        let report = Report {
            version: "0.5.5.4".to_string(),
            previous: Some("0.5.5.3".to_string()),
            kept: Some(1023),
            checked: 823,
            broken: vec!["Mods".to_string(), "Stats".to_string()],
            refitted: vec![("MapDeviceLayouts".to_string(), Vec::new()), ("Stats".to_string(), vec!["IsLocal".to_string()])],
            repaired: vec!["MapDeviceLayouts".to_string()],
            unresolved: vec![("Mods".to_string(), "not in both patches".to_string())],
            retired: vec!["WorldAreas".to_string()],
            written: true,
            client: Some(Ok(801)),
            disputed: vec![("NPCTalk".to_string(), vec!["byte 300: the client reads a key to WorldAreas".to_string()])],
            new_disputes: Some(Vec::new()),
            mismatched: 2,
        };
        let back = Report::from_json(&report.to_json()).unwrap();
        assert_eq!(back.lines(), Report { kept: None, ..report }.lines());
        assert_eq!((back.checked, back.mismatched, back.written), (823, 2, true));
        assert_eq!(back.new_disputes, Some(Vec::new()));
        assert!(back.headline().contains("1 re-fitted from 0.5.5.3, 1 repaired from the game client"), "{}", back.headline());
        assert!(back.headline().ends_with("1 read differently by the client (none new since 0.5.5.3)"), "{}", back.headline());
    }

    #[test]
    fn a_dispute_is_new_when_the_last_check_lacked_one_of_its_findings() {
        let found = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let before = Report {
            disputed: vec![("Quest".to_string(), found(&["byte 103: a"])), ("MapDeviceRecipes".to_string(), found(&["byte 24: b", "byte 100: c"]))],
            ..Default::default()
        };
        let now = Report {
            previous: Some("0.5.5.4.2".to_string()),
            disputed: vec![
                ("MapDeviceRecipes".to_string(), found(&["byte 100: c"])),
                ("Quest".to_string(), found(&["byte 103: a", "byte 119: d"])),
                ("Mods".to_string(), found(&["byte 278: e"])),
            ],
            ..Default::default()
        };
        let new = now.disputes_new_since(&before);
        assert_eq!(new, vec!["Quest".to_string(), "Mods".to_string()]);
        let now = Report { new_disputes: Some(new), ..now };
        assert!(now.headline().ends_with("(2 new since 0.5.5.4.2)"), "{}", now.headline());
        assert!(now.is_new_dispute("Mods") && !now.is_new_dispute("MapDeviceRecipes"));
        assert!(now.lines().contains(&"  the client reads Mods differently (new):".to_string()));
    }

    #[test]
    fn a_record_from_before_the_client_was_read_still_loads() {
        let old = serde_json::json!({ "version": "3.29.3.3", "previous": null, "checked": 849, "broken": ["MapDeviceLayouts"], "refitted": [], "unresolved": [], "retired": [], "written": false });
        let report = Report::from_json(&old).unwrap();
        assert_eq!(report.broken, vec!["MapDeviceLayouts".to_string()]);
        assert!(report.client.is_none() && report.disputed.is_empty() && report.new_disputes.is_none());
    }
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
        let report = check(&new_patch, game, "9.9.9", &community, &overrides, true, Some((version.clone(), &stored as &dyn FileSource)), None).unwrap();
        for line in report.lines() {
            println!("{}", line);
        }
        assert_eq!(report.broken, vec!["Stats".to_string()]);
        assert_eq!(report.refitted.len(), 1, "Stats should re-fit from {}", version);
        let saved = Overrides::load(&overrides);
        assert!(saved.tables.iter().any(|t| t.name == "Stats" && is_refit(t) && t.valid_for == Some(game_mask(true))), "the re-fit is saved for PoE 2 alone");

        // The community schema reads the unmoved file, so the re-fit retires.
        let caught_up: HashMap<String, Vec<u8>> = [(file, original)].into();
        let report = check(&caught_up, game, "9.9.10", &community, &overrides, true, None, None).unwrap();
        assert_eq!(report.retired, vec!["Stats".to_string()]);
        assert!(Overrides::load(&overrides).tables.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test --release patch_check_holds_layouts_against_the_client -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn patch_check_holds_layouts_against_the_client() {
        let game = Game::Poe2;
        let version = table_store::versions(game).into_iter().next().expect("no kept patch; run export-data once");
        let stored = StoredTables::open(game, &version).unwrap();
        let settings = AppSettings::load();
        let exe = settings.ggpk_path_for(game).and_then(|p| Path::new(p).parent().and_then(exe_layout::client_binary));
        let client = ClientLayouts::read(&std::fs::read(exe.expect("no game client beside the configured GGPK")).unwrap()).unwrap();
        let path = settings.schema_local_path.map(PathBuf::from).unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
        let community: Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let tables: HashMap<String, Vec<u8>> = std::fs::read_dir(stored.dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = format!("{}{}", table_store::table_dir(game), e.file_name().to_str()?.strip_suffix(".zst")?);
                Some((path.clone(), stored.fetch(&path)?))
            })
            .collect();

        let no_overrides = std::env::temp_dir().join(format!("ggpk-patch-check-client-{}.json", std::process::id()));
        let report = check(&tables, game, &version, &community, &no_overrides, false, None, Some(&client)).unwrap();
        for line in report.lines() {
            println!("{}", line);
        }
        assert!(report.checked > 500, "only {} tables checked", report.checked);
        assert!(report.disputed.iter().all(|(_, found)| !found.is_empty()));
        assert!(report.disputed.len() * 5 < report.checked, "the client disputes {} of {} tables", report.disputed.len(), report.checked);
    }
}
