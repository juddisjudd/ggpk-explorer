//! Every DAT table in the install written out whole, as JSON and/or CSV (see `dat::dump` for the cell shapes).

use crate::dat::dump::{Dumper, Keys};
use crate::dat::reader::DatReader;
use crate::dat::relational::{FileSource, DAT_DIRS, DAT_EXT};
use crate::data_export::json::{self, Obj, J};
use crate::data_export::source::GameFiles;
use crate::data_export::DataExportOptions;
use crate::export::ExportStatus;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    pub json: bool,
    pub csv: bool,
}

impl Default for Formats {
    fn default() -> Self {
        Self { json: true, csv: false }
    }
}

impl Formats {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "json" => Ok(Self { json: true, csv: false }),
            "csv" => Ok(Self { json: false, csv: true }),
            "both" => Ok(Self { json: true, csv: true }),
            other => Err(format!("--format takes json, csv or both, not {}", other)),
        }
    }
}

/// A table left out, for `export_report.json`.
struct Note {
    table: String,
    status: &'static str,
    detail: String,
}

pub fn run(files: GameFiles, schema: crate::dat::schema::Schema, is_poe2: bool, out: PathBuf, options: DataExportOptions, tx: Sender<ExportStatus>) {
    let out = match (&options.version, options.flat) {
        (Some(version), false) => out.join(options.folder_name(version)),
        _ => out,
    };
    if let Err(e) = std::fs::create_dir_all(&out) {
        let _ = tx.send(ExportStatus::Error(format!("Could not create {}: {}", out.display(), e)));
        return;
    }
    if let Some(version) = &options.version {
        let _ = std::fs::write(out.join("version.txt"), format!("{}\n", version));
    }

    let wanted: HashSet<String> = options.only.iter().map(|n| n.to_ascii_lowercase()).collect();
    let tables: Vec<(String, String)> = tables_in(&files)
        .into_iter()
        .filter(|(stem, _)| wanted.is_empty() || wanted.contains(&stem.to_ascii_lowercase()))
        .collect();
    if tables.is_empty() {
        let _ = tx.send(ExportStatus::Error(match wanted.is_empty() {
            true => "This install has no DAT tables".to_string(),
            false => format!("No table in this install is named {:?}", options.only),
        }));
        return;
    }

    let keys = Keys::new(&files, &schema, is_poe2);
    let lookup = |t: &str| keys.get(t);
    let dumper = Dumper::new(&schema, is_poe2, &lookup);
    let formats = options.table_formats;
    let total = tables.len();
    let mut notes = Vec::new();
    let mut failures = Vec::new();
    let mut written = 0;
    for (i, (stem, path)) in tables.iter().enumerate() {
        let _ = tx.send(ExportStatus::Progress { current: i + 1, total, filename: stem.clone() });
        let Some(def) = schema.find_table(stem, is_poe2) else {
            notes.push(Note { table: stem.clone(), status: "no schema", detail: "dat-schema does not describe it".to_string() });
            continue;
        };
        let reader = match files.fetch(path).map(|bytes| DatReader::new(bytes, path)) {
            Some(Ok(reader)) => reader,
            Some(Err(e)) => {
                notes.push(Note { table: def.name.clone(), status: "unreadable", detail: e.to_string() });
                continue;
            }
            None => {
                notes.push(Note { table: def.name.clone(), status: "unreadable", detail: format!("could not read {}", path) });
                continue;
            }
        };
        let fit = crate::dat::analysis::check_fit(&reader, def, 40);
        if fit.is_broken() {
            notes.push(Note { table: def.name.clone(), status: "skipped", detail: fit.summary() });
            continue;
        }
        let mut wrote = Ok(());
        if formats.json {
            let rows = dumper.table(&reader, def);
            let rows = match options.strip_null {
                true => json::without_nulls(&rows),
                false => rows,
            };
            wrote = wrote.and(json::write_text(&out, &format!("{}.json", def.name), &json::pretty(&rows)));
        }
        if formats.csv {
            wrote = wrote.and(json::write_text(&out, &format!("{}.csv", def.name), &dumper.csv(&reader, def)));
        }
        match wrote {
            Ok(()) => written += 1,
            Err(e) => failures.push(format!("{}: {}", def.name, e)),
        }
    }

    let report = Obj::new()
        .or_null("version", options.version.as_deref().map(json::text))
        .set("game", json::text(crate::settings::Game::from_is_poe2(is_poe2).label()))
        .set("formats", json::strings([formats.json.then_some("json"), formats.csv.then_some("csv")].into_iter().flatten()))
        .set("tables_written", J::Int(written as i64))
        .set("tables_failed", json::strings(&failures))
        .set(
            "tables_left_out",
            json::arr(notes.iter().map(|n| {
                Obj::new().set("table", json::text(&n.table)).set("status", json::text(n.status)).set("detail", json::text(&n.detail)).build()
            })),
        )
        .build();
    let _ = json::write_text(&out, "export_report.json", &json::pretty(&report));
    if !failures.is_empty() {
        let _ = std::fs::write(out.join("data_export_errors.log"), failures.join("\n"));
    }

    let skipped = notes.iter().filter(|n| n.status == "skipped").count();
    let mut message = format!("Wrote {} of {} tables to {}.", written, total, out.display());
    if skipped > 0 {
        message.push_str(&format!(" {} did not match the schema (see export_report.json).", skipped));
    }
    let _ = tx.send(ExportStatus::Complete { count: written, errors: failures.len(), message });
}

/// `(name, path)` for each table at the top of the data folders, bundle order so each bundle is decompressed once.
fn tables_in(files: &GameFiles) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut tables = Vec::new();
    for dir in DAT_DIRS {
        for path in files.list_dir(dir) {
            let rest = &path[dir.len()..];
            let Some(stem) = rest.len().checked_sub(DAT_EXT.len()).and_then(|at| {
                (rest.is_char_boundary(at) && rest[at..].eq_ignore_ascii_case(DAT_EXT)).then(|| &rest[..at])
            }) else {
                continue;
            };
            if !stem.contains('/') && seen.insert(stem.to_ascii_lowercase()) {
                tables.push((stem.to_string(), path.clone()));
            }
        }
    }
    tables.sort_by_key(|(_, path)| files.lookup(path).map(|f| (f.bundle_index, f.file_offset)));
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every table of the configured install into the temp folder: `cargo test --release export_real_tables -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn export_real_tables() {
        let settings = crate::settings::AppSettings::load();
        let reader = std::sync::Arc::new(crate::ggpk::reader::GgpkReader::open(settings.ggpk_path.expect("no ggpk_path configured")).unwrap());
        let dir = crate::settings::AppSettings::get_app_data_dir();
        let index = crate::bundles::index::Index::load_from_cache(dir.join(crate::settings::INDEX_CACHE_FILENAME)).expect("run the app once to build the index cache");
        let schema: crate::dat::schema::Schema = serde_json::from_str(&std::fs::read_to_string(dir.join("schema.min.json")).unwrap()).unwrap();
        let files = GameFiles::new(Some(reader), std::sync::Arc::new(index), None, None);
        let out = std::env::temp_dir().join("ggpk-explorer-table-export");
        let _ = std::fs::remove_dir_all(&out);
        let options = DataExportOptions {
            flat: true,
            only: std::env::var("TABLES").map(|t| t.split(',').map(str::to_string).collect()).unwrap_or_default(),
            table_formats: Formats { json: true, csv: true },
            ..Default::default()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        run(files, schema, true, out.clone(), options, tx);
        let mut done = None;
        for status in rx {
            match status {
                ExportStatus::Complete { message, .. } => done = Some(message),
                ExportStatus::Error(e) => panic!("{}", e),
                ExportStatus::Progress { .. } => {}
            }
        }
        println!("{} ({:.0?})", done.expect("no completion message"), started.elapsed());
        println!("{}", std::fs::read_to_string(out.join("export_report.json")).unwrap().lines().take(40).collect::<Vec<_>>().join("\n"));
        let mods = std::fs::read_to_string(out.join("Mods.json")).unwrap();
        assert!(mods.contains("\"TableName\": \"Stats\",\n      \"Id\": "), "Mods.json references should name stats by id");
    }
}
