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

/// The last patch a client log mentions fetching from, which is the one the
/// install currently holds.
fn scan_version(log: &str) -> Option<String> {
    const MARKER: &str = "poecdn.com/";
    log.match_indices(MARKER)
        .filter_map(|(at, _)| {
            let rest = &log[at + MARKER.len()..];
            let version = &rest[..rest.find('/')?];
            let numbered = version.split('.').count() >= 3
                && version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
            numbered.then(|| version.to_string())
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

    /// Loads a table or reports which module went without it.
    pub fn table(&self, name: &str) -> Result<std::rc::Rc<crate::dat::relational::LoadedTable>, String> {
        self.rr.table(name).ok_or_else(|| format!("table {} is missing from this install", name))
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

    /// Appends a description file's includes before the file itself, so later
    /// definitions override earlier ones.
    fn collect_csd(
        &self,
        file: &str,
        out: &mut Vec<std::rc::Rc<crate::dat::csd::CsdFile>>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        // Callers name a file as `stat_descriptions`, as `…​.txt` (the name PoE 1
        // used), as a path under the description folder, or as a full path.
        // All of them mean one `.csd` in that folder.
        const DIR: &str = "Data/StatDescriptions/";
        let name = file.trim_end_matches(".csd").trim_end_matches(".txt");
        let path = if name.to_ascii_lowercase().starts_with(&DIR.to_ascii_lowercase()) {
            format!("{}.csd", name)
        } else {
            format!("{}{}.csd", DIR, name)
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
    let selected: Vec<modules::Module> = registry()
        .into_iter()
        .filter(|m| options.only.is_empty() || options.only.iter().any(|n| n == m.name))
        .collect();

    if selected.is_empty() {
        let _ = tx.send(ExportStatus::Error(format!(
            "No modules matched {:?}. Known modules: {}",
            options.only,
            module_names().join(", ")
        )));
        return;
    }

    // Each patch gets its own folder so exports do not overwrite each other.
    let out = match (&options.version, options.flat) {
        (Some(version), false) => out.join(version),
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

    if !failures.is_empty() {
        let log = out.join("data_export_errors.log");
        let _ = std::fs::write(&log, failures.join("\n"));
    }

    let _ = tx.send(ExportStatus::Complete {
        count: total - failures.len(),
        errors: failures.len(),
        message: if failures.is_empty() {
            format!("Wrote {} data files to {}.", total, out.display())
        } else {
            format!(
                "Wrote {} of {} data files. {} failed (see data_export_errors.log).",
                total - failures.len(),
                total,
                failures.len()
            )
        },
    });
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
}
