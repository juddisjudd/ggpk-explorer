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

pub struct ContentView {
    texture_cache: HashMap<u64, egui::TextureHandle>,
    raw_data_cache: HashMap<u64, Vec<u8>>,
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
}

impl Default for ContentView {
    fn default() -> Self {
        Self {
            texture_cache: HashMap::new(),
            raw_data_cache: HashMap::new(),
            dat_viewer: DatViewer::default(),
            audio_stream_handle: None,
            audio_sink: None,
            last_error: None,
            failed_loads: std::collections::HashSet::new(),
            zoom_level: 1.0,

            cdn_loader: None,
        }
    }
}

use crate::ui::app::FileSelection;
use crate::bundles::index::Index;

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

    pub fn show(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, selection: Option<FileSelection>, is_poe2: bool, bundle_index: &Option<Index>) {
        if let Some(selection) = selection {
            match selection {
                FileSelection::GgpkOffset(offset) => {
                    self.show_ggpk_file(ui, reader, offset, is_poe2);
                },
                FileSelection::BundleFile(hash) => {
                    if let Some(index) = bundle_index {
                        if let Some(file_info) = index.files.get(&hash) {
                             ui.heading(&file_info.path);
                            // Header Info (Hidden by default, maybe toggle?)
                             // ui.heading(&file_info.path);
                             // ui.label(format!("Size: {} bytes", file_info.file_size));
                             // ui.separator();
                             
                             if let Some(err) = &self.last_error {
                                 ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                                 ui.separator();
                             }

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
                             } else if file_info.path.ends_with(".ogg") {
                                 // Audio auto load?
                             } else {
                                 // For other files, auto load into raw cache for Hex View?
                                 if !self.raw_data_cache.contains_key(&hash) && file_info.file_size < 1024 * 1024 { // Only auto load small files < 1MB
                                     perform_load = true;
                                 }
                             }
                             
                             if self.failed_loads.contains(&hash) {
                                 perform_load = false;
                             }

                             ui.horizontal(|ui| {
                                 if ui.button("Reload Content").clicked() {
                                     self.load_bundled_content(ui.ctx(), reader, index, file_info, hash);
                                 }
                                 if ui.button("Export File").clicked() {
                                      self.export_bundled_content(reader, index, file_info);
                                 }
                                 if ui.button("Debug Header").clicked() {
                                     self.debug_bundled_header(reader, index, file_info);
                                 }
                             });

                             // Perform Auto-Load if needed
                             if perform_load {
                                 self.load_bundled_content(ui.ctx(), reader, index, file_info, hash);
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
                                                self.show_audio_player(ui, reader, index, file_info, hash);
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

    fn show_audio_player(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, index: &Index, file_info: &crate::bundles::index::FileInfo, hash: u64) {
        ui.group(|ui| {
            ui.label("Audio Player");
            
            ui.horizontal(|ui| {
                if ui.button("▶ Play").clicked() {
                    self.load_bundled_content(ui.ctx(), reader, index, file_info, hash);
                }
                
                if ui.button("⏹ Stop").clicked() {
                    if let Some(sink) = &self.audio_sink {
                        sink.stop();
                    }
                    self.audio_sink = None;
                }
            });
            
            if let Some(sink) = &self.audio_sink {
                 if sink.empty() {
                     ui.label("Status: Stopped / Finished");
                 } else {
                     ui.label("Status: Playing...");
                 }
            }
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

    fn load_bundled_content(&mut self, ctx: &egui::Context, reader: &GgpkReader, index: &Index, file_info: &crate::bundles::index::FileInfo, hash: u64) {
         // Reset previous state
         self.dat_viewer.reader = None;
         self.dat_viewer.error_msg = None;
         self.last_error = None;

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

    pub fn export_bundled_content(&self, reader: &GgpkReader, index: &Index, file_info: &crate::bundles::index::FileInfo) {
         if let Some(path) = rfd::FileDialog::new().set_file_name(&file_info.path).save_file() {
             if let Some(bundle_info) = index.bundles.get(file_info.bundle_index as usize) {
                 let bundle_path = format!("Bundles2/{}", bundle_info.name);
                 if let Ok(Some(file_record)) = reader.read_file_by_path(&bundle_path) {
                     if let Ok(data) = reader.get_data_slice(file_record.data_offset, file_record.data_length) {
                         let mut cursor = std::io::Cursor::new(data);
                         if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                             if let Ok(decompressed_data) = bundle.decompress(&mut cursor) {
                                  let start = file_info.file_offset as usize;
                                  let end = start + file_info.file_size as usize;
                                  if end <= decompressed_data.len() {
                                      let file_data = &decompressed_data[start..end];
                                      let _ = std::fs::write(path, file_data);
                                  }
                             }
                         }
                     }
                 }
             }
         }
    }

    fn debug_bundled_header(&self, reader: &GgpkReader, index: &Index, file_info: &crate::bundles::index::FileInfo) {
          if let Some(bundle_info) = index.bundles.get(file_info.bundle_index as usize) {
              let bundle_path = format!("Bundles2/{}", bundle_info.name);
              if let Ok(Some(file_record)) = reader.read_file_by_path(&bundle_path) {
                  if let Ok(data) = reader.get_data_slice(file_record.data_offset, file_record.data_length) {
                      let mut cursor = std::io::Cursor::new(data);
                      if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                          if let Ok(decompressed_data) = bundle.decompress(&mut cursor) {
                              let start = file_info.file_offset as usize;
                              let end = start + file_info.file_size as usize;
                              if end <= decompressed_data.len() {
                                  let file_data = &decompressed_data[start..end];
                                  println!("DEBUG HEADER for {}:", file_info.path);
                                  let len = std::cmp::min(64, file_data.len());
                                  let header = &file_data[0..len];
                                  println!("Bytes: {:02X?}", header);
                                  if file_data.len() >= 4 {
                                      use byteorder::{ByteOrder, LittleEndian};
                                      let u32_val = LittleEndian::read_u32(file_data);
                                      println!("First u32: {}", u32_val);
                                  }
                              }
                          }
                      }
                  }
              }
          }
    }
}









