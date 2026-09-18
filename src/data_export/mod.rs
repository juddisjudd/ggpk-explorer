//! Semantic game-data dumps in the shape RePoE publishes: instead of one JSON
//! per DAT table, each module joins the tables behind one game concept
//! (`mods.json`, `skills.json`, `base_items.json`, …) into a single file.
//!
//! Text is English only. Every module is independent and optional, so a table
//! the current patch renamed costs one file rather than the whole run.

pub mod json;
pub mod modules;
pub mod source;
pub mod statics;

pub use crate::dat::stat_handlers;
use json::J;


use crate::dat::relational::RelationalReader;
use crate::export::ExportStatus;
use source::GameFiles;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Default)]
pub struct DataExportOptions {
    /// Module names to run; empty runs all of them.
    pub only: Vec<String>,
    /// Also export the icons the dumps point at, as png and webp.
    pub images: bool,
    /// Look up trade stat ids from the official trade API (needs network).
    pub trade_stats: bool,
    /// Patch the data is being read from. The export goes in a folder named
    /// after it, so several patches can sit side by side.
    pub version: Option<String>,
    /// Write straight into the chosen folder, without a version subfolder.
    pub flat: bool,
    /// Drop null-valued keys from every dump, so an entry lists only what it
    /// has. Off by default — the published files keep them.
    pub strip_null: bool,
}

impl DataExportOptions {
    /// The folder a run lands in. A stripped export is kept beside the full
    /// one rather than on top of it — the two are the same patch in two
    /// shapes, and overwriting one with the other loses data silently.
    pub fn folder_name(&self, version: &str) -> String {
        match self.strip_null {
            true => format!("{}-stripped", version),
            false => version.to_string(),
        }
    }
}

/// The patch an install is on, read from the client log it writes on every
/// launch (`Web root: https://patch-poe2.poecdn.com/<version>/`).
pub fn detect_version(install_root: &Path) -> Option<String> {
    for name in ["logs/LatestClient.txt", "logs/Client.txt"] {
        let Ok(text) = read_tail(&install_root.join(name), 512 * 1024) else { continue };
        if let Some(version) = scan_version(&text) {
            return Some(version);
        }
    }
    None
}

/// Which game an install belongs to, from the patch host its client log names.
pub fn detect_game(install_root: &Path) -> Option<crate::settings::Game> {
    for name in ["logs/LatestClient.txt", "logs/Client.txt"] {
        let Ok(text) = read_tail(&install_root.join(name), 512 * 1024) else { continue };
        if let Some((game, _)) = scan_patch(&text) {
            return Some(game);
        }
    }
    None
}

/// Which game a loaded index belongs to, from where it keeps its tables: PoE 2
/// under `data/balance/`, PoE 1 in `data/` itself. Read when no client log
/// names the game.
pub fn game_from_index(index: &crate::bundles::index::Index) -> Option<crate::settings::Game> {
    let starts = |path: &str, prefix: &str| path.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix));
    let mut poe1 = false;
    for file in index.files.values() {
        let path = file.path.as_str();
        if starts(path, "data/balance/") {
            return Some(crate::settings::Game::Poe2);
        }
        poe1 |= starts(path, "data/") && path.len() > 5 && path.to_ascii_lowercase().ends_with(".datc64");
    }
    poe1.then_some(crate::settings::Game::Poe1)
}

/// The last patch a client log mentions fetching from, which is the one the
/// install currently holds.
fn scan_version(log: &str) -> Option<String> {
    scan_patch(log).map(|(_, version)| version)
}

fn scan_patch(log: &str) -> Option<(crate::settings::Game, String)> {
    const MARKER: &str = "poecdn.com/";
    log.match_indices(MARKER)
        .filter_map(|(at, _)| {
            let rest = &log[at + MARKER.len()..];
            let version = &rest[..rest.find('/')?];
            let numbered = version.split('.').count() >= 3
                && version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
            let game = match log[..at].ends_with("patch-poe2.") {
                true => crate::settings::Game::Poe2,
                false => crate::settings::Game::Poe1,
            };
            numbered.then(|| (game, version.to_string()))
        })
        .last()
}

/// Reads the last `limit` bytes of a file; logs grow to hundreds of megabytes
/// and only the newest lines say which patch is installed.
fn read_tail(path: &Path, limit: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(limit)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// What a module needs to do its job.
pub struct Ctx<'a> {
    pub rr: &'a RelationalReader<'a>,
    pub files: &'a GameFiles,
    pub out: &'a Path,
    pub options: &'a DataExportOptions,
    translations: std::cell::RefCell<
        std::collections::HashMap<String, std::rc::Rc<crate::dat::stat_translation::TranslationLookup>>,
    >,
    /// Art already written this run, so a texture many items share is decoded
    /// once.
    images: std::cell::RefCell<std::collections::HashSet<String>>,
    /// `Art/UIImages1.txt`, read the first time a sheet rectangle is needed.
    ui_images: std::cell::RefCell<
        Option<std::rc::Rc<std::collections::HashMap<String, modules::images::UiImage>>>,
    >,
    /// Parsed description files. Hundreds of skill-specific files include the
    /// same two large shared ones, which are worth parsing only once.
    csd: std::cell::RefCell<
        std::collections::HashMap<String, Option<std::rc::Rc<crate::dat::csd::CsdFile>>>,
    >,
    /// Tables a module asked for and had to go without.
    tables: std::cell::RefCell<Vec<TableNote>>,
    /// The module running now, so a table note can say who asked.
    module: std::cell::Cell<&'static str>,
}

/// A table a module could not use as asked, for `export_report.json`.
#[derive(Clone)]
pub struct TableNote {
    pub table: String,
    /// `skipped` (optional, layout broken), `refused` (required, layout
    /// broken), `other game` (the schema gives it to the other game) or
    /// `missing` (not in this install or schema).
    pub status: &'static str,
    pub detail: String,
    pub modules: Vec<&'static str>,
}

impl<'a> Ctx<'a> {
    pub fn new(
        rr: &'a RelationalReader<'a>,
        files: &'a GameFiles,
        out: &'a Path,
        options: &'a DataExportOptions,
    ) -> Self {
        Self {
            rr,
            files,
            out,
            options,
            translations: Default::default(),
            images: Default::default(),
            ui_images: Default::default(),
            csd: Default::default(),
            tables: Default::default(),
            module: std::cell::Cell::new(""),
        }
    }

    /// Writes one dump, applying whatever shaping the run asked for. Every
    /// module goes through here so the options are honoured in one place.
    pub fn write(&self, name: &str, value: &J) -> Result<(), String> {
        match self.options.strip_null {
            true => json::write(self.out, name, &json::without_nulls(value)),
            false => json::write(self.out, name, value),
        }
    }

    /// Claims a texture for writing, returning false when this run already
    /// wrote it.
    pub fn claim_image(&self, path: &str) -> bool {
        self.images.borrow_mut().insert(path.to_ascii_lowercase())
    }

    /// The UI sheet index, which says where a named picture sits in a shared
    /// texture.
    pub fn ui_images(&self) -> std::rc::Rc<std::collections::HashMap<String, modules::images::UiImage>> {
        if let Some(hit) = self.ui_images.borrow().as_ref() {
            return std::rc::Rc::clone(hit);
        }
        let text = crate::dat::relational::FileSource::fetch(self.files, "Art/UIImages1.txt")
            .map(|bytes| crate::parsers::utils::decode_text_lossy(&bytes))
            .unwrap_or_default();
        let sheet = std::rc::Rc::new(modules::images::parse_ui_images(&text));
        *self.ui_images.borrow_mut() = Some(std::rc::Rc::clone(&sheet));
        sheet
    }

    /// Loads a table, refusing one whose layout this patch moved out from under
    /// the schema. Reading it would not fail — it would return values taken from
    /// the wrong bytes — so the module stops instead of writing fiction.
    pub fn table(&self, name: &str) -> Result<std::rc::Rc<crate::dat::relational::LoadedTable>, String> {
        let Some(table) = self.rr.table(name) else {
            let (status, detail) = self.absent(name);
            self.note(name, status, detail);
            return Err(format!("table {} is missing from this install", name));
        };
        if table.fit.is_broken() {
            self.note(name, "refused", table.fit.summary());
            return Err(format!(
                "table {} does not match the schema on this patch: {}. \
                 Re-fit it with `ggpk-explorer refit --old <previous version>` \
                 or wait for dat-schema to catch up",
                name,
                table.fit.summary()
            ));
        }
        Ok(table)
    }

    /// A table a module can do without. Absent and unreadable come back the
    /// same way — as `None` — but an unreadable one is recorded so the run
    /// says what it left out rather than quietly thinning the dump.
    pub fn optional_table(&self, name: &str) -> Option<std::rc::Rc<crate::dat::relational::LoadedTable>> {
        let Some(table) = self.rr.table(name) else {
            let (status, detail) = self.absent(name);
            self.note(name, status, detail);
            return None;
        };
        if table.fit.is_broken() {
            self.note(name, "skipped", table.fit.summary());
            return None;
        }
        Some(table)
    }

    /// Why a table is not here: the other game owns it, or it is a real gap.
    fn absent(&self, name: &str) -> (&'static str, String) {
        match self.rr.schema.belongs_to_other_game(name, self.rr.is_poe2) {
            true => {
                let other = crate::settings::Game::from_is_poe2(!self.rr.is_poe2);
                ("other game", format!("a {} table", other.label()))
            }
            false => ("missing", "not in this install or schema".to_string()),
        }
    }

    fn note(&self, table: &str, status: &'static str, detail: String) {
        let module = self.module.get();
        let mut notes = self.tables.borrow_mut();
        match notes.iter_mut().find(|n| n.table == table && n.status == status) {
            Some(existing) if !existing.modules.contains(&module) => existing.modules.push(module),
            Some(_) => {}
            None => notes.push(TableNote { table: table.to_string(), status, detail, modules: vec![module] }),
        }
    }

    /// Tables left out of this run because their layout no longer matches the
    /// schema.
    pub fn skipped_tables(&self) -> Vec<String> {
        self.table_notes()
            .into_iter()
            .filter(|n| n.status == "skipped")
            .map(|n| format!("{}: {}", n.table, n.detail))
            .collect()
    }

    pub fn table_notes(&self) -> Vec<TableNote> {
        self.tables.borrow().clone()
    }

    /// Stat text for one description file, with everything it `include`s
    /// folded in first so the file's own wording wins. Cached per file.
    pub fn translations(
        &self,
        file: &str,
    ) -> std::rc::Rc<crate::dat::stat_translation::TranslationLookup> {
        if let Some(hit) = self.translations.borrow().get(file) {
            return std::rc::Rc::clone(hit);
        }
        let mut chain = Vec::new();
        self.collect_csd(file, &mut chain, &mut std::collections::HashSet::new());
        let lookup =
            std::rc::Rc::new(crate::dat::stat_translation::TranslationLookup::build_shared(&chain));
        self.translations.borrow_mut().insert(file.to_string(), std::rc::Rc::clone(&lookup));
        lookup
    }

    /// Where this game keeps its stat description files, and their extension.
    pub fn description_files(&self) -> (&'static str, &'static str) {
        match self.rr.is_poe2 {
            true => ("Data/StatDescriptions/", ".csd"),
            false => ("Metadata/StatDescriptions/", ".txt"),
        }
    }

    /// Appends a description file's includes before the file itself, so later
    /// definitions override earlier ones.
    fn collect_csd(
        &self,
        file: &str,
        out: &mut Vec<std::rc::Rc<crate::dat::csd::CsdFile>>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        // Callers name a file as `stat_descriptions`, with either game's
        // extension, as a path under the description folder, or as a full path.
        // All of them mean one file in this game's folder.
        let (dir, ext) = self.description_files();
        let name = file.trim_end_matches(".csd").trim_end_matches(".txt");
        let path = if name.to_ascii_lowercase().starts_with(&dir.to_ascii_lowercase()) {
            format!("{}{}", name, ext)
        } else {
            format!("{}{}{}", dir, name, ext)
        };
        let key = path.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            return;
        }
        let cached = self.csd.borrow().get(&key).cloned();
        let parsed = match cached {
            Some(hit) => hit,
            None => {
                let parsed = crate::dat::relational::FileSource::fetch(self.files, &path)
                    .and_then(|bytes| crate::dat::csd::parse_csd(&bytes, &path).ok())
                    .map(std::rc::Rc::new);
                self.csd.borrow_mut().insert(key, parsed.clone());
                parsed
            }
        };
        let Some(parsed) = parsed else { return };
        for include in &parsed.includes {
            self.collect_csd(include, out, seen);
        }
        out.push(parsed);
    }
}

pub type ModuleFn = fn(&Ctx) -> Result<(), String>;

/// Every module, in the order they run.
pub fn registry() -> Vec<modules::Module> {
    modules::registry()
}

pub fn module_names() -> Vec<&'static str> {
    registry().into_iter().map(|m| m.name).collect()
}

/// Runs the export, reporting progress the way `export::run_export` does.
pub fn run(
    files: GameFiles,
    schema: crate::dat::schema::Schema,
    is_poe2: bool,
    out: PathBuf,
    options: DataExportOptions,
    tx: Sender<ExportStatus>,
) {
    let game = crate::settings::Game::from_is_poe2(is_poe2);
    // A dump the other game owns is always reported as skipped, whether or not
    // this run asked for it; `only` then picks from what is left.
    let (runnable, skipped_modules): (Vec<modules::Module>, Vec<modules::Module>) =
        registry().into_iter().partition(|m| m.skip_reason(game).is_none());
    let selected: Vec<modules::Module> = runnable
        .into_iter()
        .filter(|m| options.only.is_empty() || options.only.iter().any(|n| n == m.name))
        .collect();

    if selected.is_empty() {
        let _ = tx.send(ExportStatus::Error(match skipped_modules.is_empty() {
            true => format!("No modules matched {:?}. Known modules: {}", options.only, module_names().join(", ")),
            false => format!("None of {:?} exist for {}", options.only, game.label()),
        }));
        return;
    }

    // Each patch gets its own folder so exports do not overwrite each other.
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

    let rr = RelationalReader::new(&files, &schema, is_poe2);
    let ctx = Ctx::new(&rr, &files, &out, &options);

    let total = selected.len();
    let mut failures = Vec::new();
    for (i, module) in selected.iter().enumerate() {
        ctx.module.set(module.name);
        let _ = tx.send(ExportStatus::Progress {
            current: i + 1,
            total,
            filename: format!("{}.json", module.name),
        });
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (module.run)(&ctx)));
        let message = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(payload) => Some(match payload.downcast_ref::<&str>() {
                Some(s) => format!("panicked: {}", s),
                None => match payload.downcast_ref::<String>() {
                    Some(s) => format!("panicked: {}", s),
                    None => "panicked".to_string(),
                },
            }),
        };
        if let Some(message) = message {
            eprintln!("data export: {} failed: {}", module.name, message);
            failures.push(format!("{}: {}", module.name, message));
        }
    }

    let _ = json::write_text(
        &out,
        "export_report.json",
        &json::pretty(&report(&options, game, &selected, &skipped_modules, &failures, &ctx.table_notes())),
    );

    let skipped = ctx.skipped_tables();
    for note in &skipped {
        eprintln!("data export: left out {}", note);
    }

    if !failures.is_empty() || !skipped.is_empty() {
        let log = out.join("data_export_errors.log");
        let lines: Vec<String> = failures
            .iter()
            .cloned()
            .chain(skipped.iter().map(|note| format!("left out {}", note)))
            .collect();
        let _ = std::fs::write(&log, lines.join("\n"));
    }
    let unusable = ctx.table_notes().iter().filter(|n| n.status != "missing" && n.status != "other game").count();
    let mut message = if failures.is_empty() {
        format!("Wrote {} data files to {}.", total, out.display())
    } else {
        format!(
            "Wrote {} of {} data files. {} failed (see data_export_errors.log).",
            total - failures.len(),
            total,
            failures.len()
        )
    };
    if unusable > 0 {
        message.push_str(&format!(" {} table(s) did not match the schema (see export_report.json).", unusable));
    }
    let _ = tx.send(ExportStatus::Complete { count: total - failures.len(), errors: failures.len(), message });
}

/// `export_report.json`: which modules ran, which failed, and every table a
/// module asked for and could not use, so a thinned export can be told apart
/// from a complete one after the fact.
fn report(
    options: &DataExportOptions,
    game: crate::settings::Game,
    selected: &[modules::Module],
    skipped: &[modules::Module],
    failures: &[String],
    tables: &[TableNote],
) -> J {
    use json::{text, Obj};
    let failed: Vec<(String, J)> =
        failures.iter().filter_map(|f| f.split_once(": ").map(|(m, e)| (m.to_string(), text(e)))).collect();
    let notes = tables.iter().map(|n| {
        Obj::new()
            .set("table", text(&n.table))
            .set("status", text(n.status))
            .set("detail", text(&n.detail))
            .set("modules", json::strings(&n.modules))
            .build()
    });
    Obj::new()
        .or_null("version", options.version.as_deref().map(text))
        .set("game", text(game.label()))
        .set("modules_run", json::strings(selected.iter().map(|m| m.name)))
        .set(
            "modules_skipped",
            J::Obj(skipped.iter().map(|m| (m.name.to_string(), text(m.skip_reason(game).unwrap_or_default()))).collect()),
        )
        .set("modules_failed", J::Obj(failed))
        .set("tables", json::arr(notes))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_patch_in_the_log_wins() {
        let log = "\
[INFO Client] Web root: https://patch-poe2.poecdn.com/4.4.0.13/\n\
[INFO Client] Connecting to 64.87.52.91\n\
[INFO Client] Web root: https://patch-poe2.poecdn.com/4.5.4.11/\n";
        assert_eq!(scan_version(log).as_deref(), Some("4.5.4.11"));
    }

    #[test]
    fn addresses_and_other_urls_are_not_versions() {
        assert_eq!(scan_version("no patch url here at all"), None);
        assert_eq!(scan_version("https://web.poecdn.com/image/thing.png"), None);
        // Path of Exile 1 uses the same log line on its own host.
        assert_eq!(
            scan_version("Web root: https://patch.poecdn.com/3.25.0.1/").as_deref(),
            Some("3.25.0.1")
        );
    }

    #[test]
    fn the_patch_host_names_the_game() {
        use crate::settings::Game;
        assert_eq!(scan_patch("Web root: https://patch.poecdn.com/3.29.3.2/").map(|p| p.0), Some(Game::Poe1));
        assert_eq!(scan_patch("Web root: https://patch-poe2.poecdn.com/4.5.5.2/").map(|p| p.0), Some(Game::Poe2));
    }
}
