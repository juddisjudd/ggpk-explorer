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

pub fn run_export(
    hashes: Vec<u64>,
    reader: Arc<GgpkReader>,
    bundle_index: Option<Arc<BundleIndex>>,
    settings: ExportSettings,
    target_dir: PathBuf,
    cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    schema: Option<Schema>,
    tx: Sender<ExportStatus>,
    _cancel_flag: Option<Arc<AtomicBool>>, // Future proofing for cancellation
) {
    let total = hashes.len();
    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();

    for (i, hash) in hashes.iter().enumerate() {
        // Send progress
        // We can't know the exact filename easily without looking it up, but we'll try to get it inside the loop
        
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match export_single_file(
                *hash, 
                &reader, 
                bundle_index.as_deref(), 
                &settings, 
                &target_dir, 
                &cdn_loader, 
                &schema
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
    
    // Log errors to a file if there are many? For now just print them
    if error_count > 0 {
        println!("Export Errors:");
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

fn export_single_file(
    hash: u64,
    reader: &GgpkReader,
    bundle_index: Option<&BundleIndex>,
    settings: &ExportSettings,
    target_dir: &Path,
    cdn_loader: &Option<crate::bundles::cdn::CdnBundleLoader>,
    schema: &Option<Schema>,
) -> Result<String, String> {
    
    // 1. Identify File Info
    // This part logic is taken from app.rs but needs to be adapted to look up by hash
    // The previous app.rs logic iterated hashes and then looked up in index.
    
    let file_info = if let Some(idx) = bundle_index {
        idx.files.get(&hash).ok_or("File hash not found in bundle index")?
    } else {
        // Fallback for GGPK (non-bundled) mode?
        // The current app.rs structure for GGPK mode wasn't clearly using hashes for tree view same way, 
        // wait, GGPK mode uses offsets?
        // TreeView uses `FileSelection` which has `GgpkOffset(u64)` or `BundleFile(u64)`.
        // BUT `ExportWindow` uses `hashes: Vec<u64>`.
        // In `TreeView::collect_hashes` it collects `file_hash`.
        // In GGPK mode (non-bundled), `file_hash` might be the offset?
        
        // Let's verify how `TreeView` sets `file_hash` for GGPK mode.
        // `TreeView::build_bundle_tree` is only called for bundled mode.
        // For GGPK mode, `render_directory` is used, but wait, `render_directory` context menu says:
        // `if ui.button("Export...").clicked()`... NO, `render_directory` does NOT currently implement export context menu in the code I saw earlier?
        // Let's re-read `tree_view.rs` lines 463+.
        return Err("Exporting from raw GGPK not fully supported in this refactor yet (hash/offset ambiguity)".to_string());
    };
    
    // Assuming Bundled Mode for now based on the file_info usage in app.rs
    // "if let Some(file_info) = index_clone.files.get(&hash)"
    
    let bundle_info = if let Some(idx) = bundle_index {
        idx.bundles.get(file_info.bundle_index as usize).ok_or("Bundle info not found")?
    } else {
        return Err("Bundle index missing".to_string());
    };


    
    let mut raw_bundle_data = None;

    // Try reading local bundle file
    // Candidate paths to try (matching content_view.rs logic)
    let candidates = vec![
        format!("Bundles2/{}", bundle_info.name),
        format!("Bundles2/{}.bundle.bin", bundle_info.name),
        bundle_info.name.clone(),
        format!("{}.bundle.bin", bundle_info.name),
    ];

    for cand in &candidates {
         if let Ok(Some(file_record)) = reader.read_file_by_path(cand) {
             if let Ok(data) = reader.get_data_slice(file_record.data_offset, file_record.data_length) {
                 raw_bundle_data = Some(data.to_vec());
                 break;
             }
         }
    }

    // Try CDN
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

    let data = raw_bundle_data.ok_or("Failed to load bundle data (Local or CDN)")?;
    
    let mut cursor = std::io::Cursor::new(data);
    let bundle = crate::bundles::bundle::Bundle::read_header(&mut cursor).map_err(|e| format!("Bundle Header: {}", e))?;
    let decompressed_data = bundle.decompress(&mut cursor).map_err(|e| format!("Decompress: {}", e))?;
    
    let start = file_info.file_offset as usize;
    let end = start + file_info.file_size as usize;
    
    if end > decompressed_data.len() {
        return Err(format!("File range {}..{} out of bundle bounds {}", start, end, decompressed_data.len()));
    }
    
    let file_data = &decompressed_data[start..end];
    let path_str = &file_info.path;
    let relative_path = std::path::Path::new(path_str);
    let full_path = target_dir.join(relative_path);
    
    if let Some(parent) = full_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    // File Extension Handling
    let filename_display = path_str.to_string();

    // Skip .header files as per user request
    if path_str.ends_with(".header") {
        return Ok(format!("Skipped header: {}", filename_display));
    }
    
    if path_str.ends_with(".dds") {
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
            },
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
            },
            TextureFormat::OriginalDds => {
                 std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
        }
    } else if path_str.ends_with(".ogg") { 
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
                      let mut writer = hound::WavWriter::create(dest, spec).map_err(|e| e.to_string())?;
                      for sample in source {
                          let _ = writer.write_sample(sample);
                      }
                      writer.finalize().map_err(|e| e.to_string())?;
                 } else {
                      std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                 }
             },
             AudioFormat::Original => {
                  std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
             }
         }
    } else if path_str.ends_with(".dat") || path_str.ends_with(".datc64") || path_str.ends_with(".datl") || path_str.ends_with(".datl64") {
         match settings.data_format {
             DataFormat::Json => {
                 let mut converted = false;
                  if let Some(schema) = schema {
                       let stem = std::path::Path::new(path_str).file_stem().and_then(|s| s.to_str()).unwrap_or("");
                       if let Some(table_def) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(stem)) {
                           if let Ok(r) = crate::dat::reader::DatReader::new(file_data.to_vec(), path_str) {
                               use serde_json::{Map, Value};
                               use crate::dat::reader::DatValue;
                               
                               let mut rows = Vec::new();
                               for i in 0..r.row_count {
                                   if let Ok(vals) = r.read_row(i, table_def) {
                                       let mut map = Map::new();
                                       for (j, val) in vals.iter().enumerate() {
                                           let col_name = table_def.columns.get(j).and_then(|c| c.name.clone()).unwrap_or_else(|| format!("Col{}", j));
                                           let v = match val {
                                               DatValue::Bool(b) => Value::from(*b),
                                               DatValue::Int(i) => Value::from(*i),
                                               DatValue::Long(l) => Value::from(*l),
                                               DatValue::Float(f) => Value::from(*f),
                                               DatValue::String(s) => Value::from(s.clone()),
                                               DatValue::List(count, _) => Value::String(format!("List(len={})", count)), 
                                               DatValue::ForeignRow(k) => Value::String(format!("Key({})", k)), 
                                               _ => Value::Null,
                                           };
                                           map.insert(col_name, v);
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
                       std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                 }
             },
             DataFormat::Original => {
                  std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
             }
         }
     } else if path_str.ends_with(".psg") {
         match settings.psg_format {
            PsgFormat::Json => {
                 let mut converted = false;
                 if let Ok(psg_file) = crate::dat::psg::parse_psg(file_data) {
                     if let Ok(json_val) = serde_json::to_value(&psg_file) {
                         let dest = full_path.with_extension("json");
                         let s = serde_json::to_string_pretty(&json_val).map_err(|e| e.to_string())?;
                         std::fs::write(dest, s).map_err(|e| e.to_string())?;
                         converted = true; 
                     }
                 }
                 if !converted {
                      std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
                 }
            },
            PsgFormat::Original => {
                 std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
            }
         }
     } else {
         std::fs::write(&full_path, file_data).map_err(|e| e.to_string())?;
     }

    Ok(filename_display)
}
