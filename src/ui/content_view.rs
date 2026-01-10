use eframe::egui;
use crate::ggpk::reader::GgpkReader;
// FileRecord is used in other modules but maybe not explicitly here if inferred?
// Wait, I use FileRecord in found_record return type?
// found_record is Option<FileRecord>.
// So I DO need FileRecord visible?
// The warning said `unused import`.
// Maybe it's available via GgpkReader preamble or just not needed if I don't name the type?
use std::collections::HashMap;

use crate::ui::dat_viewer::DatViewer;
use crate::dat::csd::{self};
use crate::dat::psg::{self};
use crate::ui::json_viewer::JsonTreeViewer;

pub struct ContentView {
    texture_cache: HashMap<u64, egui::TextureHandle>,
    raw_data_cache: HashMap<u64, Vec<u8>>,
    pub csd_cache: HashMap<u64, csd::CsdFile>,
    pub csd_language_filter: Option<String>,
    pub json_cache: HashMap<u64, serde_json::Value>,
    pub dat_viewer: DatViewer,
    // rodio::OutputStream does not implement Default, so we can't derive it.
    // We also can't easily store OutputStream in a struct that needs to be Default/Clone usually, 
    // but here we just need to initialize it.
    audio_stream_handle: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    audio_sink: Option<rodio::Sink>,
    pub last_error: Option<String>,
    pub failed_loads: std::collections::HashSet<u64>,
    pub zoom_level: f32,

    pub cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    pub audio_volume: f32,
    
    pub texture_loader: Option<crate::ui::texture_loader::TextureLoader>,
    // (hashes, name_for_title)
    pub export_requested: Option<(Vec<u64>, String, Option<crate::ui::export_window::ExportSettings>)>,
    pub selection_requested: Option<crate::ui::app::FileSelection>,
}

impl Default for ContentView {
    fn default() -> Self {
        Self {
            texture_cache: HashMap::new(),
            raw_data_cache: HashMap::new(),
            csd_cache: HashMap::new(),
            csd_language_filter: Some("English".to_string()),
            json_cache: HashMap::new(),
            dat_viewer: DatViewer::default(),
            audio_stream_handle: None,
            audio_sink: None,
            last_error: None,
            failed_loads: std::collections::HashSet::new(),
            zoom_level: 1.0,

            cdn_loader: None,
            audio_volume: 0.5,
            texture_loader: Some(crate::ui::texture_loader::TextureLoader::new()),
            export_requested: None,
            selection_requested: None,
        }
    }
}

use crate::ui::app::FileSelection;


impl ContentView {
    pub fn set_cdn_loader(&mut self, loader: crate::bundles::cdn::CdnBundleLoader) {
        self.cdn_loader = Some(loader);
    }

    pub fn update_cdn_version(&mut self, ver: &str) {
        if let Some(loader) = &mut self.cdn_loader {
            loader.set_patch_version(ver);
        }
    }
    
    pub fn set_dat_schema(&mut self, schema: crate::dat::schema::Schema, created_at: String) {
        self.dat_viewer.set_schema(schema, created_at);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, reader: std::sync::Arc<crate::ggpk::reader::GgpkReader>, selection: Option<FileSelection>, is_poe2: bool, bundle_index: &Option<std::sync::Arc<crate::bundles::index::Index>>) {
        if let Some(selection) = selection {
            match selection {
                FileSelection::GgpkOffset(offset) => {
                    self.show_ggpk_file(ui, &reader, offset, is_poe2);
                },
                FileSelection::Folder(hashes, name) => {
                     self.show_folder_grid(ui, reader, bundle_index, hashes, name);
                },
                FileSelection::BundleFile(hash) => {
                    if let Some(index) = bundle_index {
                        if let Some(file_info) = index.files.get(&hash) {


                             // Auto-load logic
                             let mut perform_load = false;
                             
                             if file_info.path.ends_with(".dds") {
                                 if !self.texture_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".dat") || file_info.path.ends_with(".dat64") || file_info.path.ends_with(".datc64") || file_info.path.ends_with(".datl") || file_info.path.ends_with(".datl64") {
                                 if self.dat_viewer.loaded_filename() != Some(file_info.path.as_str()) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".csd") {
                                 if !self.csd_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".psg") {
                                 if !self.json_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".json") {
                                 if !self.json_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".ogg") {
                                 // Audio auto load?
                             } else if is_text_file(&file_info.path) {
                                 if !self.raw_data_cache.contains_key(&hash) && file_info.file_size < 2 * 1024 * 1024 { // Auto load text < 2MB
                                     perform_load = true;
                                 }
                             } else {
                                 // For other files, auto load into raw cache for Hex View?
                                 if !self.raw_data_cache.contains_key(&hash) && file_info.file_size < 1024 * 1024 { // Only auto load small files < 1MB
                                     perform_load = true;
                                 }
                             }
                             
                             if self.failed_loads.contains(&hash) {
                                 perform_load = false;
                             }
                             
                             // Header with Context Menu
                             let label = egui::RichText::new(&file_info.path).heading();
                             let response = ui.label(label);
                             response.context_menu(|ui| {
                                 if ui.button("Export...").clicked() {
                                     self.export_requested = Some((vec![hash], file_info.path.clone(), None));
                                     ui.close_menu();
                                 }
                             });

                             // Perform Auto-Load if needed
                             if perform_load {
                                 self.load_bundled_content(ui.ctx(), &reader, index, file_info, hash);
                             }

                             // Display Content
                             ui.separator();
                             
                             if file_info.path.ends_with(".dat") || file_info.path.ends_with(".dat64") || file_info.path.ends_with(".datc64") || file_info.path.ends_with(".datl") || file_info.path.ends_with(".datl64") {
                                  // DatViewer handles its own scrolling via TableBuilder
                                  // If dat viewer has error, show generic hex views?
                                  if self.dat_viewer.error_msg.is_some() || self.dat_viewer.reader.is_none() {
                                      egui::ScrollArea::vertical().show(ui, |ui| {
                                          if let Some(data) = self.raw_data_cache.get(&hash) {
                                              ui.label("Dat Load Failed. Showing raw hex view:");
                                              crate::ui::hex_viewer::HexViewer::show(ui, data);
                                          } else {
                                              self.dat_viewer.show(ui, is_poe2); // Show failed state
                                          }
                                      });
                                  } else {
                                      // Ensure it takes available space
                                      self.dat_viewer.show(ui, is_poe2);
                                  }
                             } else if file_info.path.ends_with(".csd") {
                                 self.show_csd(ui, hash);
                             } else if file_info.path.ends_with(".json") || file_info.path.ends_with(".psg") {
                                 if let Some(job) = self.json_cache.get(&hash) {
                                     egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                                         JsonTreeViewer::show(ui, job);
                                     });
                                 } else if self.failed_loads.contains(&hash) {
                                      ui.label(format!("Failed to load JSON. Error: {}", self.last_error.as_deref().unwrap_or("Unknown")));
                                 } else {
                                      ui.label("Loading JSON...");
                                 }
                             } else {
                                 // For other content, use ScrollArea
                                      if file_info.path.ends_with(".dds") {
                                          if let Some(texture) = self.texture_cache.get(&hash) {
                                               // Static Controls
                                               ui.horizontal(|ui| {
                                                    if ui.button("-").clicked() {
                                                        self.zoom_level = (self.zoom_level - 0.1).max(0.1);
                                                    }
                                                    ui.add(egui::Slider::new(&mut self.zoom_level, 0.1..=5.0).text("Zoom"));
                                                    if ui.button("+").clicked() {
                                                        self.zoom_level = (self.zoom_level + 0.1).min(5.0);
                                                    }
                                                    if ui.button("Fits Window").clicked() {
                                                         let available_width = ui.available_width();
                                                         let size = texture.size_vec2();
                                                         if size.x > 0.0 {
                                                             self.zoom_level = (available_width / size.x).min(1.0);
                                                         }
                                                    }
                                                    if ui.button("Reset (100%)").clicked() {
                                                        self.zoom_level = 1.0;
                                                    }
                                               });
                                               
                                               ui.separator();

                                               egui::ScrollArea::both().show(ui, |ui| {
                                                  ui.vertical_centered(|ui| {
                                                      let size = texture.size_vec2() * self.zoom_level;
                                                      ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                                                  });
                                               });
                                          } else {
                                              egui::ScrollArea::vertical().show(ui, |ui| {
                                                 if self.failed_loads.contains(&hash) {
                                                      ui.label(format!("Failed to load image. Error: {}", self.last_error.as_deref().unwrap_or("Unknown")));
                                                 } else {
                                                      ui.label("Loading image...");
                                                 }
                                              });
                                          }
                                      } else if file_info.path.ends_with(".ogg") {
                                           egui::ScrollArea::vertical().show(ui, |ui| {
                                                self.show_audio_player(ui, &reader, index, file_info, hash);
                                           });
                                      } else if is_text_file(&file_info.path) {
                                           egui::ScrollArea::vertical().show(ui, |ui| {
                                                if let Some(data) = self.raw_data_cache.get(&hash) {
                                                     let text = decode_text_with_detection(data);
                                                      // Show read-only text edit
                                                      let language = if file_info.path.ends_with(".hlsl") || file_info.path.ends_with(".vshader") || file_info.path.ends_with(".pshader") || file_info.path.ends_with(".fx") {
                                                          "hlsl"
                                                      } else {
                                                          "text"
                                                      };

                                                      let theme = crate::ui::syntax::Theme::dark();
                                                      let mut layouter = |ui: &egui::Ui, string: &str, _wrap_width: f32| {
                                                          let mut layout_job = crate::ui::syntax::highlight(ui.ctx(), &theme, string, language);
                                                          layout_job.wrap.max_width = f32::INFINITY; 
                                                          ui.fonts(|f| f.layout_job(layout_job))
                                                      };

                                                      egui::ScrollArea::both().show(ui, |ui| {
                                                          ui.add(egui::TextEdit::multiline(&mut text.as_str())
                                                              .code_editor()
                                                              .lock_focus(false)
                                                              .desired_width(f32::INFINITY)
                                                              .layouter(&mut layouter)
                                                          );
                                                      });
                                                } else {
                                                     ui.label("Loading text...");
                                                }
                                           });
                                      } else {
                                          egui::ScrollArea::vertical().show(ui, |ui| {
                                              if let Some(data) = self.raw_data_cache.get(&hash) {
                                                  crate::ui::hex_viewer::HexViewer::show(ui, data);
                                              } else {
                                                  if file_info.file_size >= 1024 * 1024 {
                                                      ui.label("File too large for auto-preview. Click Reload Content to force load.");
                                                  } else {
                                                      ui.label("Loading...");
                                                  }
                                              }
                                          });
                                      }
                             }

                        } else {
                            ui.label("File info not found in index");
                        }
                    } else {
                        ui.label("No bundle index loaded");
                    }
                }
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Select a file to view content.");
            });
        }
    }

    fn show_folder_grid(&mut self, ui: &mut egui::Ui, reader: std::sync::Arc<crate::ggpk::reader::GgpkReader>, bundle_index: &Option<std::sync::Arc<crate::bundles::index::Index>>, hashes: Vec<u64>, name: String) {
        ui.heading(format!("Folder: {}", name));
        ui.separator();
        
        // Filter for DDS files
        // TODO: This filtering happens every frame. For really large folders, we should cache this result.
        // For now, it's likely fast enough (linear scan of u64s).
        let dds_files: Vec<u64> = if let Some(idx) = bundle_index {
            hashes.into_iter().filter(|h| {
                if let Some(info) = idx.files.get(h) {
                    info.path.ends_with(".dds")
                } else {
                    false
                }
            }).collect()
        } else {
            Vec::new()
        };

        if dds_files.is_empty() {
             ui.label("No images found in this folder.");
             return;
        }

        ui.horizontal(|ui| {
            ui.label(format!("Found {} images.", dds_files.len()));
             if ui.button("Clear Texture Cache").clicked() {
                 self.texture_cache.clear();
             }
        });
        ui.separator();

        // Ensure loader exists
        if self.texture_loader.is_none() {
            self.texture_loader = Some(crate::ui::texture_loader::TextureLoader::new());
        }
        
        // Poll loader
        // We poll up to X items per frame to avoid choking if many return at once
        if let Some(loader) = &mut self.texture_loader {
            let mut updates = 0;
            while let Some((hash, image)) = loader.poll() {
                // Create texture
                let texture = ui.ctx().load_texture(
                    format!("thumb_{}", hash),
                    image,
                    egui::TextureOptions::default()
                );
                self.texture_cache.insert(hash, texture);
                updates += 1;
                if updates > 50 { break; } 
            }
             if updates > 0 {
                 ui.ctx().request_repaint();
             }
        }

        // Layout Constants
        let thumbnail_size = 128.0;
        let padding = 8.0;
        let item_width = thumbnail_size + padding;
        let item_height = thumbnail_size + 30.0 + padding; // Space for label
        
        let available_width = ui.available_width();
        let cols = (available_width / item_width).floor().max(1.0) as usize;
        let rows = (dds_files.len() + cols - 1) / cols;

        egui::ScrollArea::vertical().show_rows(ui, item_height, rows, |ui, row_range| {
             let reader_arc = reader.clone(); 
             
             // We manually implement the grid layout for the visible rows
             for row in row_range {
                 ui.horizontal(|ui| {
                     for col in 0..cols {
                         let index = row * cols + col;
                         if index >= dds_files.len() {
                             break;
                         }
                         
                         let hash = dds_files[index];
                         
                         // Allocate item space
                         let (rect, _response) = ui.allocate_exact_size(egui::vec2(thumbnail_size, item_height), egui::Sense::hover());
                         
                         // Render Item inside rect
                         ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                             ui.vertical_centered(|ui| {
                                 // 1. Texture / Placeholder
                                 if let Some(texture) = self.texture_cache.get(&hash) {
                                      // Scale to fit
                                      let mut size = texture.size_vec2();
                                      let scale = (thumbnail_size / size.x).min(thumbnail_size / size.y).min(1.0);
                                      size *= scale;
                                      
                                      if ui.add(egui::Image::new(texture).fit_to_exact_size(size).sense(egui::Sense::click())).clicked() {
                                          self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(hash));
                                      }
                                 } else {
                                     // Placeholder
                                     let (p_rect, _) = ui.allocate_exact_size(egui::vec2(thumbnail_size, thumbnail_size), egui::Sense::hover());
                                     ui.painter().rect_filled(p_rect, 4.0, egui::Color32::from_gray(30));
                                     ui.allocate_new_ui(egui::UiBuilder::new().max_rect(p_rect), |ui| {
                                         ui.centered_and_justified(|ui| ui.spinner());
                                     });
                                     
                                     // Request Load (Lazy Loading)
                                     if let Some(idx) = bundle_index {
                                         if let Some(info) = idx.files.get(&hash) {
                                             if let Some(loader) = &mut self.texture_loader {
                                                 if !loader.is_loading(hash) {
                                                     loader.request(hash, info.path.clone(), reader_arc.clone(), idx.clone(), info);
                                                 }
                                             }
                                         }
                                     }
                                 }
                                 
                                 // 2. Label
                                 if let Some(idx) = bundle_index {
                                      if let Some(info) = idx.files.get(&hash) {
                                          let name = std::path::Path::new(&info.path).file_name().unwrap_or_default().to_string_lossy();
                                          ui.label(egui::RichText::new(name).small().weak()).on_hover_text(&info.path);
                                      }
                                 }
                             });
                         });
                         
                         // Spacing between columns
                         ui.add_space(padding);
                     }
                 });
                 // Spacing between rows
                 ui.add_space(padding);
             }
        });
    }

    fn show_audio_player(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, index: &std::sync::Arc<crate::bundles::index::Index>, file_info: &crate::bundles::index::FileInfo, hash: u64) {
        ui.group(|ui| {
            ui.heading("Audio Player");
            
            ui.horizontal(|ui| {
                if ui.button("▶ Play").clicked() {
                    self.load_bundled_content(ui.ctx(), &reader, index, file_info, hash);
                }
                
                if ui.button("⏹ Stop").clicked() {
                    if let Some(sink) = &self.audio_sink {
                        sink.stop();
                    }
                    self.audio_sink = None;
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Volume:");
                if ui.add(egui::Slider::new(&mut self.audio_volume, 0.0..=1.0).show_value(true)).changed() {
                     if let Some(sink) = &self.audio_sink {
                         sink.set_volume(self.audio_volume);
                     }
                }
            });
            ui.add_space(4.0);
            
            let status = if let Some(sink) = &self.audio_sink {
                 if sink.empty() { "Stopped" } else { "Playing..." }
            } else {
                 "Stopped"
            };
            
            ui.horizontal(|ui| {
                 ui.label("Status:");
                 if status == "Playing..." {
                     ui.colored_label(egui::Color32::GREEN, status);
                 } else {
                     ui.label(status);
                 }
            });
        });
    }

    fn show_ggpk_file(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, offset: u64, is_poe2: bool) {
            match reader.read_file_record(offset) {
                Ok(file) => {
                    ui.heading(&file.name);
                    ui.label(format!("Size: {} bytes", file.data_length));
                    ui.label(format!("Offset: {}", file.offset));
                    if ui.button("Export").clicked() {
                        // TODO
                    }
                    ui.separator();
                    
                    if file.name.ends_with(".dds") {
                        if let Some(texture) = self.texture_cache.get(&offset) {
                             ui.image(texture);
                        } else {
                             match reader.get_data_slice(file.data_offset, file.data_length) {
                                  Ok(data) => {
                                      match image::load_from_memory(data) {
                                          Ok(img) => {
                                              let size = [img.width() as usize, img.height() as usize];
                                              let image_buffer = img.to_rgba8();
                                              let pixels = image_buffer.as_flat_samples();
                                              let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                  size,
                                                  pixels.as_slice(),
                                              );
                                              
                                              let texture = ui.ctx().load_texture(
                                                  &file.name,
                                                  color_image,
                                                  egui::TextureOptions::default()
                                              );
                                              ui.image(&texture);
                                              self.texture_cache.insert(offset, texture);
                                          },
                                          Err(e) => { ui.label(format!("Failed to load DDS: {}", e)); }
                                      }
                                  },
                                  Err(e) => { ui.label(format!("Read error: {}", e)); }
                             }
                        }
                    } else if file.name.ends_with(".dat") || file.name.ends_with(".dat64") {
                         self.dat_viewer.load(reader, offset);
                         self.dat_viewer.show(ui, is_poe2);
                    } else {
                        // Reset DatViewer if switching away? 
                        // Or keep state?
                        // For now just show "Hex View (TODO)"
                        ui.label("Hex View (TODO)");
                    }
                },
                Err(e) => {
                    ui.label(format!("Error reading file: {}", e));
                }
            }
    }

    // Caching helpers
    fn get_cache_path(hash: u64) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push("ggpk-explorer-cache");
        let _ = std::fs::create_dir_all(&path);
        path.push(format!("{:x}.bin", hash));
        path
    }

    fn try_load_from_cache(&mut self, hash: u64) -> bool {
        let path = Self::get_cache_path(hash);
        if path.exists() {
             if let Ok(file) = std::fs::File::open(&path) {
                 if let Ok(value) = bincode::deserialize_from::<_, serde_json::Value>(std::io::BufReader::new(file)) {
                     self.json_cache.insert(hash, value);
                     return true;
                 }
             }
        }
        false
    }

    fn save_to_cache(hash: u64, value: &serde_json::Value) {
        let path = Self::get_cache_path(hash);
        if let Ok(file) = std::fs::File::create(&path) {
            let _ = bincode::serialize_into(std::io::BufWriter::new(file), value);
        }
    }

    pub fn load_bundled_content(&mut self, ctx: &egui::Context, reader: &GgpkReader, index: &std::sync::Arc<crate::bundles::index::Index>, file_info: &crate::bundles::index::FileInfo, hash: u64) {
         // Reset previous state
         self.dat_viewer.reader = None;
         self.dat_viewer.error_msg = None;
         self.last_error = None;

         // Check persistent cache for JSON/PSG
         if file_info.path.ends_with(".json") || file_info.path.ends_with(".psg") {
             if self.try_load_from_cache(hash) {
                 println!("Loaded {} from disk cache.", file_info.path);
                 return;
             }
         }

         if let Some(bundle_info) = index.bundles.get(file_info.bundle_index as usize) {
             let mut raw_bundle_data: Option<Vec<u8>> = None;
             
             // 1. Try Local GGPK
             // Candidate paths to try
             let candidates = vec![
                 format!("Bundles2/{}", bundle_info.name),
                 format!("Bundles2/{}.bundle.bin", bundle_info.name),
                 bundle_info.name.clone(),
                 format!("{}.bundle.bin", bundle_info.name),
             ];

             for cand in &candidates {
                 // println!("Attempting to load bundle from GGPK: {}", cand);
                 if let Ok(Some(rec)) = reader.read_file_by_path(cand) {
                     println!("Bundle found in GGPK: {}", cand);
                     if let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) {
                         raw_bundle_data = Some(data.to_vec());
                         break;
                     }
                 }
             }

             // 2. Try CDN Fallback
             if raw_bundle_data.is_none() {
                 if let Some(cdn) = &self.cdn_loader {
                     // PoE2 CDN expects .bundle.bin suffix usually
                     let fetch_name = if bundle_info.name.ends_with(".bundle.bin") {
                         bundle_info.name.clone()
                     } else {
                         format!("{}.bundle.bin", bundle_info.name)
                     };
                     
                     println!("Bundle missing from GGPK. Attempting CDN fetch for: {}", fetch_name);
                     match cdn.fetch_bundle(&fetch_name) {
                         Ok(data) => {
                             println!("Bundle fetched from CDN. Size: {}", data.len());
                             raw_bundle_data = Some(data);
                         },
                         Err(e) => {
                             let msg = format!("CDN Fetch Failed: {}", e);
                             println!("{}", msg);
                             self.last_error = Some(msg);
                         }
                     }
                 } else {
                     let msg = format!("Bundle not found in GGPK and CDN Loader not initialized. Hash: {}", hash);
                     println!("{}", msg);
                     self.last_error = Some(msg);
                 }
             }

             if let Some(data) = raw_bundle_data {
                 self.failed_loads.remove(&hash);
                 let mut cursor = std::io::Cursor::new(data);
                 match crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                    Ok(bundle) => {
                         println!("Bundle header read success. Uncompressed Size: {}", bundle.uncompressed_size);
                         match bundle.decompress(&mut cursor) {
                            Ok(decompressed_data) => {
                                 println!("Bundle decompressed success. Size: {}", decompressed_data.len());
                                     let start = file_info.file_offset as usize;
                                     let end = start + file_info.file_size as usize;
                             
                             if end <= decompressed_data.len() {
                                 let file_data = decompressed_data[start..end].to_vec();
                                 let path = &file_info.path;
                                 
                                 // Debug print
                                 println!("Loaded content for: {}", path);

                                 if path.ends_with(".dat") || path.ends_with(".dat64") || path.ends_with(".datc64") || path.ends_with(".datl") || path.ends_with(".datl64") {
                                      println!("Loading DAT: {} ({} bytes)", path, file_data.len());
                                      self.dat_viewer.load_from_bytes(file_data, path);
                                      if self.dat_viewer.reader.is_none() {
                                          self.last_error = Some(format!("Failed to parse DAT file: {}", self.dat_viewer.error_msg.as_deref().unwrap_or("Unknown error")));
                                          // Prevent retry loop
                                          self.failed_loads.insert(hash);
                                      } else {
                                          self.last_error = None;
                                      }
                                  } else if path.ends_with(".dds") {
                                      // Try to load DDS
                                      self.last_error = None;
                                      
                                      println!("DDS Loading: Data Length {}", file_data.len());
                                      if file_data.len() > 16 {
                                          println!("DDS First 16 bytes: {:02X?}", &file_data[0..16]);
                                          let magic = &file_data[0..4];
                                          if magic == b"DDS " {
                                              println!("Magic 'DDS ' confirmed.");
                                          } else {
                                              println!("WARNING: Magic bytes mismatch! Expected 'DDS ', found {:?}", magic);
                                          }
                                      }
                                      
                                      // Method 1: Try image_dds first (better support for various DXT/BC formats)
                                      let mut loaded = false;
                                      
                                      let mut cursor = std::io::Cursor::new(&file_data);
                                      match ddsfile::Dds::read(&mut cursor) {
                                          Ok(dds) => {
                                              println!("DDS Header Read OK.");
                                              match image_dds::image_from_dds(&dds, 0) {
                                                  Ok(image) => {
                                                      println!("image_dds conversion OK. Size: {}x{}", image.width(), image.height());
                                                      let size = [image.width() as usize, image.height() as usize];
                                                      let pixels = image.as_raw();
                                                      let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                          size,
                                                          pixels,
                                                      );
                                                      let texture = ctx.load_texture(
                                                          path,
                                                          color_image,
                                                          egui::TextureOptions::default()
                                                      );
                                                      self.texture_cache.insert(hash, texture);
                                                      loaded = true;
                                                  },
                                                  Err(e) => {
                                                      println!("image_dds failed to convert: {:?}", e);
                                                  }
                                              }
                                          },
                                          Err(e) => {
                                              println!("DDS Header Read Failed: {:?}", e);
                                          }
                                      }
                                      
                                      // Method 2: Fallback to image crate (built-in dds support)
                                      if !loaded {
                                          if let Ok(img) = image::load_from_memory(&file_data) {
                                              let size = [img.width() as usize, img.height() as usize];
                                              let image_buffer = img.to_rgba8();
                                              let pixels = image_buffer.as_flat_samples();
                                              let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                  size,
                                                  pixels.as_slice(),
                                              );
                                              
                                              let texture = ctx.load_texture(
                                                  path,
                                                  color_image,
                                                  egui::TextureOptions::default()
                                              );
                                              self.texture_cache.insert(hash, texture);
                                              loaded = true;
                                          }
                                      }
                                      
                                      if !loaded {
                                          let msg = format!("Failed to decode DDS image (unsupported format? type maybe: BC7/DXT10/etc). File size: {}", file_data.len());
                                          self.last_error = Some(msg);
                                          self.failed_loads.insert(hash);
                                      } else {
                                          self.failed_loads.remove(&hash);
                                          self.last_error = None;
                                      }
                                 } else if path.ends_with(".ogg") {
                                      println!("Audio file selected: {}", path);
                                      
                                      // Initialize audio if needed
                                      if self.audio_stream_handle.is_none() {
                                          if let Ok(stream_handle) = rodio::OutputStream::try_default() {
                                              self.audio_stream_handle = Some(stream_handle);
                                          } else {
                                              println!("Failed to get default audio output device");
                                          }
                                      }
                                      
                                      if let Some((_, stream_handle)) = &self.audio_stream_handle {
                                          use std::io::Cursor;
                                          let cursor = Cursor::new(file_data);
                                          
                                          if let Ok(decoder) = rodio::Decoder::new(cursor) {
                                               // Recreate sink for each playback to avoid state issues
                                               if let Ok(sink) = rodio::Sink::try_new(stream_handle) {
                                                   sink.set_volume(self.audio_volume);
                                                   sink.append(decoder);
                                                   sink.play(); 
                                                   self.audio_sink = Some(sink);
                                               } else {
                                                    self.last_error = Some("Failed to create audio sink".to_string());
                                               }
                                          } else {
                                              self.last_error = Some("Failed to decode Audio (Might be Wwise WEM)".to_string());
                                          }
                                      }
                                  } else if path.ends_with(".csd") {
                                     println!("Loading CSD file: {}", path);
                                     match csd::parse_csd(&file_data, path) {
                                         Ok(csd_file) => {
                                             println!("CSD parsed successfully: {} entries", csd_file.entries.len());
                                             self.csd_cache.insert(hash, csd_file);
                                             self.last_error = None;
                                         },
                                         Err(e) => {
                                             println!("CSD Parse Error: {}", e);
                                             self.last_error = Some(format!("CSD Parse Error: {}", e));
                                             // Fallback to raw data?
                                             self.raw_data_cache.insert(hash, file_data.clone());
                                         }
                                     }
                                                                   } else if path.ends_with(".json") {
                                      // println!("Loading JSON file: {}", path);
                                      let json_str = decode_text_with_detection(&file_data);
                                      match serde_json::from_str::<serde_json::Value>(&json_str) {
                                          Ok(v) => {
                                              Self::save_to_cache(hash, &v);
                                              self.json_cache.insert(hash, v);
                                              self.last_error = None;
                                          },
                                          Err(e) => {
                                              self.last_error = Some(format!("Invalid JSON: {}", e));
                                              // Fallback: Store raw string as a Value::String if possible
                                              self.json_cache.insert(hash, serde_json::Value::String(json_str));
                                          }
                                      }
                                  } else if path.ends_with(".psg") {
                                      // println!("Loading PSG file: {}", path);
                                      match psg::parse_psg(&file_data) {
                                          Ok(psg_file) => {
                                              // Convert PSG to Value
                                              if let Ok(v) = serde_json::to_value(&psg_file) {
                                                  Self::save_to_cache(hash, &v);
                                                  self.json_cache.insert(hash, v);
                                                  self.last_error = None;
                                              } else {
                                                   self.last_error = Some("Failed to serialize PSG".to_string());
                                                   self.failed_loads.insert(hash);
                                              }
                                          },
                                          Err(e) => {
                                              // println!("PSG Parse Error: {}", e);
                                              self.last_error = Some(format!("PSG Parse Error: {}", e));
                                              self.raw_data_cache.insert(hash, file_data.clone());
                                          }
                                      }
                                  } else if is_text_file(path) {
                                      // Just store raw data, we decode on render
                                      self.raw_data_cache.insert(hash, file_data);
                                      self.last_error = None;
                                  } else {
                                      // Fallback for unknown files - cache raw data to stop re-loading
                                      self.raw_data_cache.insert(hash, file_data);
                                      self.last_error = None;
                                  }
                             }},
                             Err(e) => {
                                 println!("Bundle decompression failed: {:?}", e);
                                 self.last_error = Some(format!("Decompression failed: {}", e));
                                 self.failed_loads.insert(hash);
                             }
                        }
                     },
                     Err(e) => {
                         println!("Bundle header read failed: {:?}", e);
                         self.last_error = Some(format!("Header read failed: {}", e));
                         self.failed_loads.insert(hash);
                     }
                  }
              }
          }
    }

    fn show_csd(&mut self, ui: &mut egui::Ui, hash: u64) {
        if let Some(csd_file) = self.csd_cache.get(&hash) {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Export JSON").clicked() {
                           if let Some(path) = rfd::FileDialog::new()
                               .set_file_name("csd_export.json")
                               .save_file() 
                           {
                               if let Ok(file) = std::fs::File::create(path) {
                                   let _ = serde_json::to_writer_pretty(file, csd_file);
                               }
                           }
                        }
                        
                        ui.separator();
                        
                        egui::ComboBox::from_id_salt("csd_lang_filter")
                            .selected_text(self.csd_language_filter.as_deref().unwrap_or("All"))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.csd_language_filter, None, "All");
                                for lang in &csd_file.languages {
                                    ui.selectable_value(&mut self.csd_language_filter, Some(lang.clone()), lang);
                                }
                            });
                        ui.label("Language:");
                    });
                });
                
                ui.separator();
                for (idx, entry) in csd_file.entries.iter().enumerate() {
                    // Filter check: If ANY sub-entry matches filter, show the group?
                    // Or show group but only matching sub-entries?
                    // Usually we want to see the ID and then the translation.
                    // If filter is set, we only show sub-entries matching.
                    // If no sub-entries match, maybe hide the whole entry?
                    
                    let filtered_subs: Vec<_> = entry.descriptions.iter().filter(|sub| {
                         match &self.csd_language_filter {
                             None => true,
                             Some(filter) => {
                                 // If sub has no language, show it (defaults/params?)
                                 // Or if it matches filter.
                                 // Usually defaults have no language?
                                 // In sample: `1 ...` then `lang ...` then `1 ...`
                                 // If filter is English, show None and English.
                                 // If filter is French, show French.
                                 sub.language.as_ref().map(|l| l == filter).unwrap_or_else(|| filter == "English")
                             }
                         }
                    }).collect();
                    
                    if filtered_subs.is_empty() {
                        continue;
                    }

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!("Entry #{}:", idx + 1));
                            for id in &entry.ids {
                                ui.code(id);
                            }
                        });
                        
                        ui.indent("descriptions", |ui| {
                            for sub in filtered_subs {
                                ui.horizontal(|ui| {
                                    if sub.is_canonical {
                                        ui.colored_label(egui::Color32::from_rgb(255, 215, 0), "★");
                                    }
                                    if let Some(lang) = &sub.language {
                                        ui.monospace(format!("[{}]", lang));
                                    }
                                    ui.label(egui::RichText::new(&sub.operator).strong());
                                    ui.label(&sub.description);
                                });
                                if !sub.parameters.is_empty() {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Params:").italics());
                                        for param in &sub.parameters {
                                            ui.monospace(format!("{}={}", param.name, param.value));
                                        }
                                    });
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        } else if self.failed_loads.contains(&hash) {
             ui.label(format!("Failed to load CSD. Error: {}", self.last_error.as_deref().unwrap_or("Unknown")));
             // Fallback to hex viewer
             if let Some(data) = self.raw_data_cache.get(&hash) {
                  ui.separator();
                  ui.label("Raw Data Fallback:");
                  crate::ui::hex_viewer::HexViewer::show(ui, data);
             }
        } else {
             ui.label("Loading CSD...");
        }
    }








}



fn is_text_file(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".txt") || p.ends_with(".xml") || p.ends_with(".ini") || 
    p.ends_with(".sh") || p.ends_with(".hlsl") || p.ends_with(".vshader") || 
    p.ends_with(".pshader") || p.ends_with(".fx") || p.ends_with(".mat") || p.ends_with(".csv")
}

fn decode_text_with_detection(data: &[u8]) -> String {
    // Check for UTF-16 LE BOM
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
        let u16s: Vec<u16> = data[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    // Check for UTF-16 BE BOM
    if data.len() >= 2 && data[0] == 0xFE && data[1] == 0xFF {
        let u16s: Vec<u16> = data[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    
    // Check for UTF-8 BOM
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        return String::from_utf8_lossy(&data[3..]).to_string();
    }

    // Default to UTF-8 lossy
    String::from_utf8_lossy(data).to_string()
}
