use crate::bundles::index::Index as BundleIndex;
use crate::dat::schema::Schema;
use crate::ggpk::reader::GgpkReader;
use crate::ui::export_window::{AudioFormat, DataFormat, ExportSettings, PsgFormat, TextureFormat};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc::Sender, Arc, Mutex};

#[derive(Debug, Clone)]
pub enum ExportStatus {
    Progress {
        current: usize,
        total: usize,
        filename: String,
    },
    Complete {
        count: usize,
        errors: usize,
        message: String,
    },
    Error(String),
}

/// Keeps the most recently decompressed bundles so bulk exports don't
/// re-decompress the same bundle for every file it contains.
struct BundleCache {
    entries: Vec<(u32, Vec<u8>)>,
}

struct DirectoryCache {
    created: HashSet<PathBuf>,
}

impl DirectoryCache {
    fn new() -> Self {
        Self {
            created: HashSet::new(),
        }
    }
}

fn mark_parent_dir_created(path: &Path, cache: &mut DirectoryCache) -> bool {
    path.parent()
        .map(|parent| cache.created.insert(parent.to_path_buf()))
        .unwrap_or(false)
}

fn ensure_parent_dir(path: &Path, cache: &mut DirectoryCache) -> Result<(), String> {
    if mark_parent_dir_created(path, cache) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

struct ProgressLimiter {
    interval: usize,
}

impl ProgressLimiter {
    fn new(interval: usize) -> Self {
        Self {
            interval: interval.max(1),
        }
    }

    fn should_send(&self, current: usize, total: usize) -> bool {
        current == total || current == 1 || current % self.interval == 0
    }
}

enum RawBundleData<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> RawBundleData<'a> {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(data) => data,
            Self::Owned(data) => data,
        }
    }
}

impl BundleCache {
    // Two entries is enough once hashes are sorted by bundle; the second
    // slot absorbs files whose paths interleave two bundles.
    const MAX_ENTRIES: usize = 2;

    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&mut self, bundle_index: u32) -> Option<&[u8]> {
        let pos = self.entries.iter().position(|(b, _)| *b == bundle_index)?;
        // Move to the back (most recently used)
        let entry = self.entries.remove(pos);
        self.entries.push(entry);
        Some(&self.entries.last().unwrap().1)
    }

    fn insert(&mut self, bundle_index: u32, data: Vec<u8>) -> &[u8] {
        while self.entries.len() >= Self::MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((bundle_index, data));
        &self.entries.last().unwrap().1
    }
}

#[derive(Debug, Clone)]
struct ExportWorkGroup {
    #[allow(dead_code)]
    bundle_index: u32,
    hashes: Vec<u64>,
}

fn build_export_work_groups(mut hashes: Vec<u64>, index: &BundleIndex) -> Vec<ExportWorkGroup> {
    hashes.sort_by_key(|h| {
        index
            .files
            .get(h)
            .map(|f| (f.bundle_index, f.file_offset))
            .unwrap_or((u32::MAX, 0))
    });

    let mut groups: Vec<ExportWorkGroup> = Vec::new();
    for hash in hashes {
        let bundle_index = index
            .files
            .get(&hash)
            .map(|f| f.bundle_index)
            .unwrap_or(u32::MAX);

        if let Some(group) = groups.last_mut() {
            if group.bundle_index == bundle_index {
                group.hashes.push(hash);
                continue;
            }
        }

        groups.push(ExportWorkGroup {
            bundle_index,
            hashes: vec![hash],
        });
    }

    groups
}

fn export_worker_count(group_count: usize) -> usize {
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    available.min(32).min(group_count.max(1))
}

pub fn run_export(
    hashes: Vec<u64>,
    reader: Option<Arc<GgpkReader>>,
    bundle_index: Option<Arc<BundleIndex>>,
    settings: ExportSettings,
    target_dir: PathBuf,
    cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    steam_loader: Option<crate::bundles::steam::SteamBundleLoader>,
    schema: Option<Schema>,
    tx: Sender<ExportStatus>,
    cancel_flag: Option<Arc<AtomicBool>>,
) {
    let total = hashes.len();
    if let Some(index) = bundle_index {
        let groups = build_export_work_groups(hashes, &index);
        run_grouped_export(
            groups,
            total,
            reader,
            index,
            settings,
            target_dir,
            cdn_loader,
            steam_loader,
            schema,
            tx,
            cancel_flag,
        );
        return;
    }

    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();
    let mut error_log: Option<std::fs::File> = None;
    let mut bundle_cache = BundleCache::new();
    let mut directory_cache = DirectoryCache::new();
    let progress_limiter = ProgressLimiter::new(64);

    for (i, hash) in hashes.iter().enumerate() {
        if cancel_flag
            .as_ref()
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            break;
        }

        // Send progress
        // We can't know the exact filename easily without looking it up, but we'll try to get it inside the loop

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match export_single_file(
                *hash,
                reader.as_deref(),
                bundle_index.as_deref(),
                &settings,
                &target_dir,
                &cdn_loader,
                &steam_loader,
                &schema,
                &mut bundle_cache,
                &mut directory_cache,
            ) {
                Ok(name) => Ok(name),
                Err(e) => Err(format!("Export failed: {}", e)),
            }
        }));

        match result {
            Ok(Ok(filename)) => {
                success_count += 1;
                if progress_limiter.should_send(i + 1, total) {
                    let _ = tx.send(ExportStatus::Progress {
                        current: i + 1,
                        total,
                        filename,
                    });
                }
            }
            Ok(Err(e)) => {
                error_count += 1;
                errors.push(e.clone());
                append_error_log(&mut error_log, &target_dir, &e);
                let _ = tx.send(ExportStatus::Progress {
                    current: i + 1,
                    total,
                    filename: format!("Error: {}", e),
                });
            }
            Err(payload) => {
                error_count += 1;
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    format!("PANIC: {}", s)
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    format!("PANIC: {}", s)
                } else {
                    "PANIC: Unknown error".to_string()
                };
                errors.push(msg.clone());
                append_error_log(&mut error_log, &target_dir, &msg);
                let _ = tx.send(ExportStatus::Progress {
                    current: i + 1,
                    total,
                    filename: msg,
                });
            }
        }
    }

    let final_msg = if error_count == 0 {
        format!("Successfully exported {} files.", success_count)
    } else {
        format!(
            "Exported {} files. {} errors occurred.",
            success_count, error_count
        )
    };

    // Errors were appended to export_errors.log as they happened (so a crash
    // mid-export still leaves a log); finish with a summary line.
    if error_count > 0 {
        append_error_log(
            &mut error_log,
            &target_dir,
            &format!("--- {} of {} files failed ---", error_count, total),
        );
        println!(
            "Export Errors (also in {}):",
            target_dir.join("export_errors.log").display()
        );
        for e in &errors {
            println!("  - {}", e);
        }
    }

    let _ = tx.send(ExportStatus::Complete {
        count: success_count,
        errors: error_count,
        message: final_msg,
    });
}

fn run_grouped_export(
    groups: Vec<ExportWorkGroup>,
    total: usize,
    reader: Option<Arc<GgpkReader>>,
    bundle_index: Arc<BundleIndex>,
    settings: ExportSettings,
    target_dir: PathBuf,
    cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    steam_loader: Option<crate::bundles::steam::SteamBundleLoader>,
    schema: Option<Schema>,
    tx: Sender<ExportStatus>,
    cancel_flag: Option<Arc<AtomicBool>>,
) {
    let worker_count = export_worker_count(groups.len());
    let work_queue = Arc::new(Mutex::new(VecDeque::from(groups)));
    let completed_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let error_log = Arc::new(Mutex::new(None));
    let target_dir = Arc::new(target_dir);
    let settings = Arc::new(settings);
    let schema = Arc::new(schema);
    let cdn_loader = Arc::new(cdn_loader);
    let steam_loader = Arc::new(steam_loader);

    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let work_queue = Arc::clone(&work_queue);
        let completed_count = Arc::clone(&completed_count);
        let success_count = Arc::clone(&success_count);
        let error_count = Arc::clone(&error_count);
        let errors = Arc::clone(&errors);
        let error_log = Arc::clone(&error_log);
        let target_dir = Arc::clone(&target_dir);
        let settings = Arc::clone(&settings);
        let schema = Arc::clone(&schema);
        let cdn_loader = Arc::clone(&cdn_loader);
        let steam_loader = Arc::clone(&steam_loader);
        let reader = reader.clone();
        let bundle_index = Arc::clone(&bundle_index);
        let tx = tx.clone();
        let cancel_flag = cancel_flag.clone();

        workers.push(std::thread::spawn(move || {
            let mut bundle_cache = BundleCache::new();
            let mut directory_cache = DirectoryCache::new();
            let progress_limiter = ProgressLimiter::new(64);

            loop {
                if cancel_flag
                    .as_ref()
                    .map(|flag| flag.load(Ordering::Relaxed))
                    .unwrap_or(false)
                {
                    break;
                }

                let group = {
                    let mut queue = work_queue.lock().unwrap();
                    queue.pop_front()
                };

                let Some(group) = group else {
                    break;
                };

                for hash in group.hashes {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        export_single_file(
                            hash,
                            reader.as_deref(),
                            Some(bundle_index.as_ref()),
                            &settings,
                            &target_dir,
                            cdn_loader.as_ref(),
                            steam_loader.as_ref(),
                            schema.as_ref(),
                            &mut bundle_cache,
                            &mut directory_cache,
                        )
                        .map_err(|e| format!("Export failed: {}", e))
                    }));

                    let current = completed_count.fetch_add(1, Ordering::Relaxed) + 1;
                    match result {
                        Ok(Ok(filename)) => {
                            success_count.fetch_add(1, Ordering::Relaxed);
                            if progress_limiter.should_send(current, total) {
                                let _ = tx.send(ExportStatus::Progress {
                                    current,
                                    total,
                                    filename,
                                });
                            }
                        }
                        Ok(Err(e)) => {
                            error_count.fetch_add(1, Ordering::Relaxed);
                            if let Ok(mut list) = errors.lock() {
                                list.push(e.clone());
                            }
                            if let Ok(mut log) = error_log.lock() {
                                append_error_log(&mut log, &target_dir, &e);
                            }
                            let _ = tx.send(ExportStatus::Progress {
                                current,
                                total,
                                filename: format!("Error: {}", e),
                            });
                        }
                        Err(payload) => {
                            error_count.fetch_add(1, Ordering::Relaxed);
                            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                                format!("PANIC: {}", s)
                            } else if let Some(s) = payload.downcast_ref::<String>() {
                                format!("PANIC: {}", s)
                            } else {
                                "PANIC: Unknown error".to_string()
                            };
                            if let Ok(mut list) = errors.lock() {
                                list.push(msg.clone());
                            }
                            if let Ok(mut log) = error_log.lock() {
                                append_error_log(&mut log, &target_dir, &msg);
                            }
                            let _ = tx.send(ExportStatus::Progress {
                                current,
                                total,
                                filename: msg,
                            });
                        }
                    }
                }
            }
        }));
    }

    for worker in workers {
        let _ = worker.join();
    }

    let success_count = success_count.load(Ordering::Relaxed);
    let error_count = error_count.load(Ordering::Relaxed);
    let final_msg = if error_count == 0 {
        format!("Successfully exported {} files.", success_count)
    } else {
        format!(
            "Exported {} files. {} errors occurred.",
            success_count, error_count
        )
    };

    if error_count > 0 {
        if let Ok(mut log) = error_log.lock() {
            append_error_log(
                &mut log,
                &target_dir,
                &format!("--- {} of {} files failed ---", error_count, total),
            );
        }
        if let Ok(errors) = errors.lock() {
            println!(
                "Export Errors (also in {}):",
                target_dir.join("export_errors.log").display()
            );
            for e in errors.iter() {
                println!("  - {}", e);
            }
        }
    }

    let _ = tx.send(ExportStatus::Complete {
        count: success_count,
        errors: error_count,
        message: final_msg,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reproduces the reported crash: bulk-export the whole
    // Art/Models/Items/Armours/ tree from the configured GGPK.
    // Run with: cargo test --release export_armours -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_export_armours_folder() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());

        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(
            BundleIndex::load_from_cache(&cache_path)
                .expect("run the app once to build the index cache"),
        );

        let hashes: Vec<u64> = index
            .files
            .iter()
            .filter(|(_, f)| f.path.starts_with("art/models/items/armours/"))
            .map(|(h, _)| *h)
            .collect();
        println!("exporting {} files", hashes.len());
        assert!(!hashes.is_empty());

        let target = std::env::temp_dir().join("ggpk_export_armours_test");
        let _ = std::fs::create_dir_all(&target);

        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::time::Instant::now();
        run_export(
            hashes,
            Some(reader),
            Some(index),
            ExportSettings::default(),
            target.clone(),
            None,
            None,
            None,
            tx,
            None,
        );
        let mut last = None;
        while let Ok(status) = rx.try_recv() {
            if let ExportStatus::Complete { .. } = &status {
                last = Some(status);
            }
        }
        println!("export took {:?}", t.elapsed());
        match last {
            Some(ExportStatus::Complete {
                count,
                errors,
                message,
            }) => {
                println!(
                    "complete: {} exported, {} errors — {}",
                    count, errors, message
                );
                assert!(count > 0);
            }
            other => panic!("export did not complete: {:?}", other),
        }
    }

    #[test]
    fn groups_bundle_exports_by_bundle_index() {
        let mut index = BundleIndex {
            bundles: Vec::new(),
            files: std::collections::HashMap::new(),
        };
        index.files.insert(
            10,
            crate::bundles::index::FileInfo {
                path_hash: 10,
                bundle_index: 2,
                file_offset: 30,
                file_size: 1,
                path: "c".to_string(),
            },
        );
        index.files.insert(
            20,
            crate::bundles::index::FileInfo {
                path_hash: 20,
                bundle_index: 1,
                file_offset: 20,
                file_size: 1,
                path: "b".to_string(),
            },
        );
        index.files.insert(
            30,
            crate::bundles::index::FileInfo {
                path_hash: 30,
                bundle_index: 1,
                file_offset: 10,
                file_size: 1,
                path: "a".to_string(),
            },
        );

        let groups = build_export_work_groups(vec![10, 20, 30], &index);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].hashes, vec![30, 20]);
        assert_eq!(groups[1].hashes, vec![10]);
    }

    #[test]
    fn progress_limiter_sends_first_interval_and_final_updates() {
        let limiter = ProgressLimiter::new(4);

        assert!(limiter.should_send(1, 10));
        assert!(!limiter.should_send(2, 10));
        assert!(limiter.should_send(4, 10));
        assert!(!limiter.should_send(9, 10));
        assert!(limiter.should_send(10, 10));
    }

    #[test]
    fn directory_cache_tracks_created_parent_paths() {
        let mut cache = DirectoryCache::new();
        let path = PathBuf::from("a/b/c.txt");

        assert!(mark_parent_dir_created(&path, &mut cache));
        assert!(!mark_parent_dir_created(&path, &mut cache));
    }
}

/// Appends one line to export_errors.log in the export destination,
/// creating the file on first use and flushing immediately.
fn append_error_log(log: &mut Option<std::fs::File>, target_dir: &Path, msg: &str) {
    use std::io::Write;
    if log.is_none() {
        *log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(target_dir.join("export_errors.log"))
            .ok();
    }
    if let Some(f) = log {
        let _ = writeln!(f, "{}", msg);
        let _ = f.flush();
    }
}

fn export_single_file(
    hash: u64,
    reader: Option<&GgpkReader>,
    bundle_index: Option<&BundleIndex>,
    settings: &ExportSettings,
    target_dir: &Path,
    cdn_loader: &Option<crate::bundles::cdn::CdnBundleLoader>,
    steam_loader: &Option<crate::bundles::steam::SteamBundleLoader>,
    schema: &Option<Schema>,
    bundle_cache: &mut BundleCache,
    directory_cache: &mut DirectoryCache,
) -> Result<String, String> {
    if let Some(idx) = bundle_index {
        let file_info = idx
            .files
            .get(&hash)
            .ok_or("File hash not found in bundle index")?;
        let path = file_info.path.clone();

        if file_info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL {
            let r = reader.ok_or("GGPK reader required for loose GGPK file export")?;
            let rec = r
                .read_file_by_path(&path)
                .map_err(|e| format!("Failed to look up loose GGPK file {}: {}", path, e))?
                .ok_or_else(|| format!("Loose GGPK file not found: {}", path))?;
            let bytes = r
                .get_data_slice(rec.data_offset, rec.data_length)
                .map_err(|e| format!("Failed to read loose GGPK file data: {}", e))?;
            export_file_data(&path, bytes, settings, target_dir, schema, directory_cache)?;
            Ok(path)
        } else if file_info.bundle_index == crate::bundles::steam::LOOSE_FILE_SENTINEL {
            if let Some(steam) = steam_loader {
                if let Some(loose_path) = steam.loose_file_path(&path) {
                    let bytes = std::fs::read(&loose_path).map_err(|e| {
                        format!("Failed to read loose file {}: {}", loose_path.display(), e)
                    })?;
                    export_file_data(&path, &bytes, settings, target_dir, schema, directory_cache)?;
                    Ok(path)
                } else {
                    Err(format!("Loose file not found on disk: {}", path))
                }
            } else {
                Err("Steam loader unavailable for loose-file export".to_string())
            }
        } else {
            let bundle_info = idx
                .bundles
                .get(file_info.bundle_index as usize)
                .ok_or("Bundle info not found")?;

            if bundle_cache.get(file_info.bundle_index).is_none() {
                let mut raw_bundle_data = None;

                if let Some(r) = reader {
                    let candidates = vec![
                        format!("Bundles2/{}", bundle_info.name),
                        format!("Bundles2/{}.bundle.bin", bundle_info.name),
                        bundle_info.name.clone(),
                        format!("{}.bundle.bin", bundle_info.name),
                    ];

                    for cand in &candidates {
                        if let Ok(Some(file_record)) = r.read_file_by_path(cand) {
                            if let Ok(data) =
                                r.get_data_slice(file_record.data_offset, file_record.data_length)
                            {
                                raw_bundle_data = Some(RawBundleData::Borrowed(data));
                                break;
                            }
                        }
                    }
                }

                if raw_bundle_data.is_none() {
                    if let Some(steam) = steam_loader {
                        if let Ok(data) = steam.fetch_bundle(&bundle_info.name) {
                            raw_bundle_data = Some(RawBundleData::Owned(data));
                        }
                    }
                }

                if raw_bundle_data.is_none() {
                    if let Some(cdn) = cdn_loader {
                        let fetch_name = if bundle_info.name.ends_with(".bundle.bin") {
                            bundle_info.name.clone()
                        } else {
                            format!("{}.bundle.bin", bundle_info.name)
                        };
                        if let Ok(data) = cdn.fetch_bundle(&fetch_name) {
                            raw_bundle_data = Some(RawBundleData::Owned(data));
                        }
                    }
                }

                let raw_bundle_data =
                    raw_bundle_data.ok_or("Failed to load bundle data (local, Steam, or CDN)")?;
                let data = raw_bundle_data.as_slice();
                let mut cursor = std::io::Cursor::new(data);
                let bundle = crate::bundles::bundle::Bundle::read_header(&mut cursor)
                    .map_err(|e| format!("Bundle Header: {}", e))?;
                let decompressed_data = bundle
                    .decompress_from_slice(data)
                    .map_err(|e| format!("Decompress: {}", e))?;
                bundle_cache.insert(file_info.bundle_index, decompressed_data);
            }

            let decompressed_data = bundle_cache
                .get(file_info.bundle_index)
                .ok_or("Bundle cache miss")?;

            let start = file_info.file_offset as usize;
            let end = start + file_info.file_size as usize;
            if end > decompressed_data.len() {
                return Err(format!(
                    "File range {}..{} out of bundle bounds {}",
                    start,
                    end,
                    decompressed_data.len()
                ));
            }

            export_file_data(
                &path,
                &decompressed_data[start..end],
                settings,
                target_dir,
                schema,
                directory_cache,
            )?;
            Ok(path)
        }
    } else {
        let r = reader.ok_or("GGPK reader is required for raw export")?;
        let file = r
            .read_file_record(hash)
            .map_err(|e| format!("Failed to read GGPK file record at offset {}: {}", hash, e))?;
        let bytes = r
            .get_data_slice(file.data_offset, file.data_length)
            .map_err(|e| format!("Failed to read GGPK file data: {}", e))?;
        export_file_data(
            &file.name,
            bytes,
            settings,
            target_dir,
            schema,
            directory_cache,
        )?;
        Ok(file.name)
    }
}

fn export_file_data(
    path_str: &str,
    file_data: &[u8],
    settings: &ExportSettings,
    target_dir: &Path,
    schema: &Option<Schema>,
    directory_cache: &mut DirectoryCache,
) -> Result<(), String> {
    let relative_path = std::path::Path::new(&path_str);
    let full_path = target_dir.join(relative_path);

    ensure_parent_dir(&full_path, directory_cache)?;

    // File Extension Handling
    let path_lower = path_str.to_ascii_lowercase();

    if path_lower.ends_with(".dds") {
        match settings.texture_format {
            TextureFormat::WebP => {
                let mut converted = false;
                let mut cursor = std::io::Cursor::new(file_data);
                if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
                    if let Ok(image) = image_dds::image_from_dds(&dds, 0) {
                        let img = image::DynamicImage::ImageRgba8(image);
                        let dest = full_path.with_extension("webp");
                        if img.save_with_format(dest, image::ImageFormat::WebP).is_ok() {
                            converted = true;
                        }
                    }
                }
                if !converted {
                    std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                }
            }
            TextureFormat::Png => {
                let mut converted = false;
                let mut cursor = std::io::Cursor::new(file_data);
                if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
                    if let Ok(image) = image_dds::image_from_dds(&dds, 0) {
                        let img = image::DynamicImage::ImageRgba8(image);
                        let dest = full_path.with_extension("png");
                        if img.save_with_format(dest, image::ImageFormat::Png).is_ok() {
                            converted = true;
                        }
                    }
                }
                if !converted {
                    std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                }
            }
            TextureFormat::OriginalDds => {
                std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_lower.ends_with(".ogg") {
        match settings.audio_format {
            AudioFormat::Wav => {
                let cursor = std::io::Cursor::new(file_data.to_vec());
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    use rodio::Source;
                    let spec = hound::WavSpec {
                        channels: source.channels(),
                        sample_rate: source.sample_rate(),
                        bits_per_sample: 16,
                        sample_format: hound::SampleFormat::Int,
                    };
                    let dest = full_path.with_extension("wav");
                    let mut writer =
                        hound::WavWriter::create(dest, spec).map_err(|e| e.to_string())?;
                    for sample in source {
                        let _ = writer.write_sample(sample);
                    }
                    writer.finalize().map_err(|e| e.to_string())?;
                } else {
                    std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                }
            }
            AudioFormat::Original => {
                std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_lower.ends_with(".dat")
        || path_lower.ends_with(".dat64")
        || path_lower.ends_with(".datc64")
        || path_lower.ends_with(".datl")
        || path_lower.ends_with(".datl64")
    {
        match settings.data_format {
            DataFormat::Json => {
                let mut converted = false;
                if let Some(schema) = schema {
                    let stem = std::path::Path::new(&path_str)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if let Some(table_def) = schema
                        .tables
                        .iter()
                        .find(|t| t.name.eq_ignore_ascii_case(stem))
                    {
                        if let Ok(r) =
                            crate::dat::reader::DatReader::new(file_data.to_vec(), path_str)
                        {
                            use serde_json::{Map, Value};

                            let mut rows = Vec::new();
                            for i in 0..r.row_count {
                                if let Ok(vals) = r.read_row(i, table_def) {
                                    let mut map = Map::new();
                                    for (j, val) in vals.iter().enumerate() {
                                        if let Some(col) = table_def.columns.get(j) {
                                            let col_name = col
                                                .name
                                                .clone()
                                                .unwrap_or_else(|| format!("Col{}", j));
                                            let v = r.value_to_json(val, col);
                                            map.insert(col_name, v);
                                        }
                                    }
                                    rows.push(Value::Object(map));
                                }
                            }
                            let json_out = Value::Array(rows);
                            let dest = full_path.with_extension("json");
                            let s = serde_json::to_string_pretty(&json_out)
                                .map_err(|e| e.to_string())?;
                            std::fs::write(dest, s).map_err(|e| e.to_string())?;
                            converted = true;
                        }
                    }
                }
                if !converted {
                    std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                }
            }
            DataFormat::Original => {
                std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_lower.ends_with(".psg") {
        match settings.psg_format {
            PsgFormat::Json => {
                let mut converted = false;
                if let Ok(psg_file) = crate::dat::psg::parse_psg(file_data) {
                    if let Ok(json_val) = serde_json::to_value(&psg_file) {
                        let dest = full_path.with_extension("json");
                        let s =
                            serde_json::to_string_pretty(&json_val).map_err(|e| e.to_string())?;
                        std::fs::write(dest, s).map_err(|e| e.to_string())?;
                        converted = true;
                    }
                }
                if !converted {
                    std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                }
            }
            PsgFormat::Original => {
                std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_lower.ends_with(".png")
        || path_lower.ends_with(".jpg")
        || path_lower.ends_with(".jpeg")
        || path_lower.ends_with(".webp")
    {
        std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
    }

    Ok(())
}
