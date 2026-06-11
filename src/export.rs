use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc::Sender};
use std::sync::atomic::AtomicBool;
use crate::ggpk::reader::GgpkReader;
use crate::bundles::index::Index as BundleIndex;
use crate::ui::export_window::{ExportSettings, TextureFormat, AudioFormat, DataFormat, PsgFormat};
use crate::dat::schema::Schema;

#[derive(Debug, Clone)]
pub enum ExportStatus {
    Progress { current: usize, total: usize, filename: String },
    Complete { count: usize, errors: usize, message: String },
    Error(String),
}

/// Keeps the most recently decompressed bundles so bulk exports don't
/// re-decompress the same bundle for every file it contains.
struct BundleCache {
    entries: Vec<(u32, Vec<u8>)>,
}

impl BundleCache {
    // Two entries is enough once hashes are sorted by bundle; the second
    // slot absorbs files whose paths interleave two bundles.
    const MAX_ENTRIES: usize = 2;

    fn new() -> Self {
        Self { entries: Vec::new() }
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
    _cancel_flag: Option<Arc<AtomicBool>>, // Future proofing for cancellation
) {
    let total = hashes.len();
    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();
    let mut error_log: Option<std::fs::File> = None;
    let mut bundle_cache = BundleCache::new();

    // Group files by bundle so each bundle is decompressed once, not once
    // per contained file.
    let mut hashes = hashes;
    if let Some(idx) = &bundle_index {
        hashes.sort_by_key(|h| {
            idx.files
                .get(h)
                .map(|f| (f.bundle_index, f.file_offset))
                .unwrap_or((u32::MAX, 0))
        });
    }

    for (i, hash) in hashes.iter().enumerate() {
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
            ) {
                Ok(name) => Ok(name),
                Err(e) => Err(format!("Export failed: {}", e)),
            }
        }));

        match result {
            Ok(Ok(filename)) => {
                success_count += 1;
                 let _ = tx.send(ExportStatus::Progress { 
                    current: i + 1, 
                    total, 
                    filename 
                });
            },
            Ok(Err(e)) => {
                error_count += 1;
                errors.push(e.clone());
                append_error_log(&mut error_log, &target_dir, &e);
                 let _ = tx.send(ExportStatus::Progress {
                    current: i + 1,
                    total,
                    filename: format!("Error: {}", e)
                });
            },
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
                    filename: msg
                });
            }
        }
    }

    let final_msg = if error_count == 0 {
        format!("Successfully exported {} files.", success_count)
    } else {
        format!("Exported {} files. {} errors occurred.", success_count, error_count)
    };
    
    // Errors were appended to export_errors.log as they happened (so a crash
    // mid-export still leaves a log); finish with a summary line.
    if error_count > 0 {
        append_error_log(
            &mut error_log,
            &target_dir,
            &format!("--- {} of {} files failed ---", error_count, total),
        );
        println!("Export Errors (also in {}):", target_dir.join("export_errors.log").display());
        for e in &errors {
            println!("  - {}", e);
        }
    }

    let _ = tx.send(ExportStatus::Complete { 
        count: success_count, 
        errors: error_count, 
        message: final_msg 
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

        let cache_path = crate::settings::AppSettings::get_app_data_dir().join("bundles2.cache");
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));

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
            Some(ExportStatus::Complete { count, errors, message }) => {
                println!("complete: {} exported, {} errors — {}", count, errors, message);
                assert!(count > 0);
            }
            other => panic!("export did not complete: {:?}", other),
        }
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
) -> Result<String, String> {
    let (path_str, file_data) = if let Some(idx) = bundle_index {
        let file_info = idx.files.get(&hash).ok_or("File hash not found in bundle index")?;
        let path = file_info.path.clone();

        if file_info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL {
            let r = reader.ok_or("GGPK reader required for loose GGPK file export")?;
            let rec = r
                .read_file_by_path(&path)
                .map_err(|e| format!("Failed to look up loose GGPK file {}: {}", path, e))?
                .ok_or_else(|| format!("Loose GGPK file not found: {}", path))?;
            let bytes = r
                .get_data_slice(rec.data_offset, rec.data_length)
                .map_err(|e| format!("Failed to read loose GGPK file data: {}", e))?
                .to_vec();
            (path, bytes)
        } else if file_info.bundle_index == crate::bundles::steam::LOOSE_FILE_SENTINEL {
            if let Some(steam) = steam_loader {
                if let Some(loose_path) = steam.loose_file_path(&path) {
                    let bytes = std::fs::read(&loose_path)
                        .map_err(|e| format!("Failed to read loose file {}: {}", loose_path.display(), e))?;
                    (path, bytes)
                } else {
                    return Err(format!("Loose file not found on disk: {}", path));
                }
            } else {
                return Err("Steam loader unavailable for loose-file export".to_string());
            }
        } else {
            let bundle_info = idx
                .bundles
                .get(file_info.bundle_index as usize)
                .ok_or("Bundle info not found")?;

            if bundle_cache.get(file_info.bundle_index).is_none() {
                let mut raw_bundle_data = None;
                let candidates = vec![
                    format!("Bundles2/{}", bundle_info.name),
                    format!("Bundles2/{}.bundle.bin", bundle_info.name),
                    bundle_info.name.clone(),
                    format!("{}.bundle.bin", bundle_info.name),
                ];

                if let Some(r) = reader {
                    for cand in &candidates {
                        if let Ok(Some(file_record)) = r.read_file_by_path(cand) {
                            if let Ok(data) = r.get_data_slice(file_record.data_offset, file_record.data_length) {
                                raw_bundle_data = Some(data.to_vec());
                                break;
                            }
                        }
                    }
                }

                if raw_bundle_data.is_none() {
                    if let Some(steam) = steam_loader {
                        if let Ok(data) = steam.fetch_bundle(&bundle_info.name) {
                            raw_bundle_data = Some(data);
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
                            raw_bundle_data = Some(data);
                        }
                    }
                }

                let data = raw_bundle_data.ok_or("Failed to load bundle data (local, Steam, or CDN)")?;
                let mut cursor = std::io::Cursor::new(data);
                let bundle = crate::bundles::bundle::Bundle::read_header(&mut cursor)
                    .map_err(|e| format!("Bundle Header: {}", e))?;
                let decompressed_data = bundle
                    .decompress(&mut cursor)
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

            (path, decompressed_data[start..end].to_vec())
        }
    } else {
        let r = reader.ok_or("GGPK reader is required for raw export")?;
        let file = r
            .read_file_record(hash)
            .map_err(|e| format!("Failed to read GGPK file record at offset {}: {}", hash, e))?;
        let bytes = r
            .get_data_slice(file.data_offset, file.data_length)
            .map_err(|e| format!("Failed to read GGPK file data: {}", e))?
            .to_vec();
        (file.name, bytes)
    };

    let relative_path = std::path::Path::new(&path_str);
    let full_path = target_dir.join(relative_path);
    
    if let Some(parent) = full_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    // File Extension Handling
    let filename_display = path_str.clone();
    let path_lower = path_str.to_ascii_lowercase();

    
    if path_lower.ends_with(".dds") {
        match settings.texture_format {
            TextureFormat::WebP => {
                let mut converted = false;
                let mut cursor = std::io::Cursor::new(&file_data);
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
                    std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
                }
            },
            TextureFormat::Png => {
                let mut converted = false;
                let mut cursor = std::io::Cursor::new(&file_data);
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
                    std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
                }
            },
            TextureFormat::OriginalDds => {
                 std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_lower.ends_with(".ogg") { 
         match settings.audio_format {
             AudioFormat::Wav => {
                 let cursor = std::io::Cursor::new(file_data.clone());
                 if let Ok(source) = rodio::Decoder::new(cursor) {
                      use rodio::Source;
                      let spec = hound::WavSpec {
                          channels: source.channels(),
                          sample_rate: source.sample_rate(),
                          bits_per_sample: 16,
                          sample_format: hound::SampleFormat::Int,
                      };
                      let dest = full_path.with_extension("wav");
                      let mut writer = hound::WavWriter::create(dest, spec).map_err(|e| e.to_string())?;
                      for sample in source {
                          let _ = writer.write_sample(sample);
                      }
                      writer.finalize().map_err(|e| e.to_string())?;
                 } else {
                      std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
                 }
             },
             AudioFormat::Original => {
                  std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
             }
         }
    } else if path_lower.ends_with(".dat") || path_lower.ends_with(".dat64") || path_lower.ends_with(".datc64") || path_lower.ends_with(".datl") || path_lower.ends_with(".datl64") {
         match settings.data_format {
             DataFormat::Json => {
                 let mut converted = false;
                  if let Some(schema) = schema {
                       let stem = std::path::Path::new(&path_str).file_stem().and_then(|s| s.to_str()).unwrap_or("");
                       if let Some(table_def) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(stem)) {
                           if let Ok(r) = crate::dat::reader::DatReader::new(file_data.clone(), path_str.as_str()) {
                               use serde_json::{Map, Value};
                               
                               let mut rows = Vec::new();
                               for i in 0..r.row_count {
                                   if let Ok(vals) = r.read_row(i, table_def) {
                                       let mut map = Map::new();
                                        for (j, val) in vals.iter().enumerate() {
                                            if let Some(col) = table_def.columns.get(j) {
                                                let col_name = col.name.clone().unwrap_or_else(|| format!("Col{}", j));
                                                let v = r.value_to_json(val, col);
                                                map.insert(col_name, v);
                                            }
                                        }
                                       rows.push(Value::Object(map));
                                   }
                               }
                               let json_out = Value::Array(rows);
                               let dest = full_path.with_extension("json");
                               let s = serde_json::to_string_pretty(&json_out).map_err(|e| e.to_string())?;
                               std::fs::write(dest, s).map_err(|e| e.to_string())?;
                               converted = true;
                           }
                       }
                  }
                 if !converted {
                       std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
                 }
             },
             DataFormat::Original => {
                  std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
             }
         }
     } else if path_lower.ends_with(".psg") {
         match settings.psg_format {
            PsgFormat::Json => {
                 let mut converted = false;
                 if let Ok(psg_file) = crate::dat::psg::parse_psg(&file_data) {
                     if let Ok(json_val) = serde_json::to_value(&psg_file) {
                         let dest = full_path.with_extension("json");
                         let s = serde_json::to_string_pretty(&json_val).map_err(|e| e.to_string())?;
                         std::fs::write(dest, s).map_err(|e| e.to_string())?;
                         converted = true; 
                     }
                 }
                 if !converted {
                      std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
                 }
            },
            PsgFormat::Original => {
                  std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
            }
         }
        } else if path_lower.ends_with(".png") || path_lower.ends_with(".jpg") || path_lower.ends_with(".jpeg") || path_lower.ends_with(".webp") {
            std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
     } else {
            std::fs::write(&full_path, &file_data).map_err(|e| e.to_string())?;
     }

    Ok(filename_display)
}
