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
use crate::ui::graphics_viewer::GraphicsViewer;
use crate::ui::json_viewer::JsonTreeViewer;
use crate::ui::skeletal_viewer::SkeletalViewer;
use crate::ui::text_config_viewer::TextConfigViewer;
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeMap;

struct ImageViewState {
    zoom: f32,
    pan: egui::Vec2,
    needs_fit: bool,
}

impl ImageViewState {
    fn new() -> Self {
        Self { zoom: 1.0, pan: egui::Vec2::ZERO, needs_fit: true }
    }
}

pub struct ContentView {
    texture_cache: HashMap<u64, egui::TextureHandle>,
    raw_data_cache: HashMap<u64, Vec<u8>>,
    pub csd_cache: HashMap<u64, csd::CsdFile>,
    pub csd_language_filter: Option<String>,
    pub json_cache: HashMap<u64, serde_json::Value>,
    pub dat_viewer: DatViewer,
    audio_stream_handle: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    audio_sink: Option<rodio::Sink>,
    pub last_error: Option<String>,
    pub failed_loads: std::collections::HashSet<u64>,
    image_view_states: HashMap<u64, ImageViewState>,

    pub cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    pub audio_volume: f32,

    pub export_requested: Option<(Vec<u64>, String, Option<crate::ui::export_window::ExportSettings>)>,
    pub selection_requested: Option<crate::ui::app::FileSelection>,

    pub psg_cache: HashMap<u64, crate::dat::psg::PsgFile>,
    pub psg_viewer_state: HashMap<u64, crate::ui::psg_viewer::PsgViewerState>,
    folder_children_cache: HashMap<String, Vec<(String, String, Vec<u64>)>>,
    folder_cache_index_size: usize,

    pub parsed_content_cache: HashMap<u64, crate::parsers::ParsedContent>,
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
            image_view_states: HashMap::new(),

            cdn_loader: None,
            audio_volume: 0.5,
            export_requested: None,
            selection_requested: None,
            
            psg_cache: HashMap::new(),
            psg_viewer_state: HashMap::new(),
            folder_children_cache: HashMap::new(),
            folder_cache_index_size: 0,
            parsed_content_cache: HashMap::new(),
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
                 FileSelection::Folder { hashes, name, path } => {
                     self.show_folder_list(ui, bundle_index, hashes, name, path);
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
                             } else if file_info.path.ends_with(".png") || file_info.path.ends_with(".jpg") || file_info.path.ends_with(".jpeg") || file_info.path.ends_with(".webp") {
                                 if !self.texture_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".dat") || file_info.path.ends_with(".dat64") || file_info.path.ends_with(".datc64") || file_info.path.ends_with(".datl") || file_info.path.ends_with(".datl64") {
                                 if self.dat_viewer.loaded_filename() != Some(file_info.path.as_str()) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".csd") {
                                 if !self.csd_cache.contains_key(&hash) && !self.raw_data_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".psg") {
                                 if !self.psg_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".json") {
                                 if !self.json_cache.contains_key(&hash) && !self.raw_data_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".ogg") || file_info.path.ends_with(".wav") || file_info.path.ends_with(".mp3") {
                                 // Audio: play on demand, no auto-load needed
                             } else if is_non_playable_media(&file_info.path) {
                                 // Non-playable media (bk2/wem/bank/mp4): never auto-load
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
                             
                             let label = egui::RichText::new(&file_info.path).heading();
                             let response = ui.label(label);
                             response.context_menu(|ui| {
                                 if ui.button("Export...").clicked() {
                                     self.export_requested = Some((vec![hash], file_info.path.clone(), None));
                                     ui.close_menu();
                                 }
                             });
                             ui.add_space(4.0);
                             ui.horizontal_wrapped(|ui| {
                                 crate::ui::components::badge(ui, file_kind_label(&file_info.path));
                                 crate::ui::components::badge(ui, &format_file_size(file_info.file_size as u64));
                                 crate::ui::components::badge(ui, &format!("{:016x}", hash));
                             });
                             ui.separator();

                             if perform_load {
                                 self.load_bundled_content(ui.ctx(), &reader, index, file_info, hash);
                             }
                             
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
                            } else if file_info.path.ends_with(".psg") {
                                 if let Some(psg_file) = self.psg_cache.get(&hash) {
                                     let state = self.psg_viewer_state.entry(hash).or_default();
                                     let show_graph = state.show_graph;
                                     let mut viewer = crate::ui::psg_viewer::PsgViewer::new(state, psg_file);
                                     
                                     if show_graph {
                                         viewer.show(ui);
                                     } else {
                                         // Still show the toggle button from the viewer
                                         viewer.show(ui); 
                                         // And show JSON below
                                         if let Some(json) = self.json_cache.get(&hash) {
                                             crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                                         } else {
                                             ui.label("JSON representation not available.");
                                         }
                                     }
                                } else if let Some(json) = self.json_cache.get(&hash) {
                                    // Fallback if PSG struct missing but JSON exists
                                    crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                                } else {
                                    if let Some(err) = &self.last_error {
                                        ui.colored_label(egui::Color32::RED, err);
                                    }
                                    if self.failed_loads.contains(&hash) {
                                        ui.colored_label(egui::Color32::RED, "Failed to load PSG.");
                                    } else {
                                         ui.spinner();
                                         ui.label("Loading PSG...");
                                    }
                                }
                            } else if file_info.path.ends_with(".json") {
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
                                      if file_info.path.ends_with(".dds") || file_info.path.ends_with(".png") || file_info.path.ends_with(".jpg") || file_info.path.ends_with(".jpeg") || file_info.path.ends_with(".webp") {
                                          let texture_info = self.texture_cache.get(&hash)
                                              .map(|t| (t.id(), t.size_vec2()));
                                          if let Some((texture_id, texture_size)) = texture_info {
                                              let state = self.image_view_states
                                                  .entry(hash)
                                                  .or_insert_with(ImageViewState::new);

                                              // Controls bar
                                              ui.horizontal(|ui| {
                                                  if ui.small_button("−").clicked() {
                                                      state.zoom = (state.zoom / 1.25).max(0.05);
                                                  }
                                                  ui.add_space(4.0);
                                                  ui.label(
                                                      egui::RichText::new(format!("{:.0}%", state.zoom * 100.0))
                                                          .size(11.5)
                                                          .monospace()
                                                          .color(egui::Color32::from_rgb(161, 161, 170)),
                                                  );
                                                  ui.add_space(4.0);
                                                  if ui.small_button("+").clicked() {
                                                      state.zoom = (state.zoom * 1.25).min(10.0);
                                                  }
                                                  ui.add_space(8.0);
                                                  if ui.small_button("Fit").clicked() {
                                                      state.needs_fit = true;
                                                  }
                                                  if ui.small_button("1:1").clicked() {
                                                      state.zoom = 1.0;
                                                      state.pan = egui::Vec2::ZERO;
                                                  }
                                                  ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                      ui.label(
                                                          egui::RichText::new(format!(
                                                              "{}×{}",
                                                              texture_size.x as u32,
                                                              texture_size.y as u32
                                                          ))
                                                          .size(11.0)
                                                          .color(egui::Color32::from_rgb(113, 113, 122)),
                                                      );
                                                  });
                                              });
                                              ui.separator();

                                              // Canvas — full remaining area
                                              let canvas_size = ui.available_size();
                                              let (canvas_rect, response) = ui.allocate_exact_size(
                                                  canvas_size,
                                                  egui::Sense::click_and_drag(),
                                              );

                                              // Auto-fit on first show
                                              if state.needs_fit && canvas_size.x > 1.0 && canvas_size.y > 1.0 {
                                                  state.zoom = (canvas_size.x / texture_size.x)
                                                      .min(canvas_size.y / texture_size.y)
                                                      .min(1.0)
                                                      .max(0.05);
                                                  state.pan = egui::Vec2::ZERO;
                                                  state.needs_fit = false;
                                              }

                                              // Scroll-wheel zoom toward cursor
                                              if response.hovered() {
                                                  let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                                                  if scroll != 0.0 {
                                                      let old_zoom = state.zoom;
                                                      let factor = if scroll > 0.0 { 1.12 } else { 1.0 / 1.12 };
                                                      let new_zoom = (old_zoom * factor).clamp(0.05, 10.0);
                                                      if let Some(cursor) = ui.input(|i| i.pointer.latest_pos()) {
                                                          let c = egui::vec2(
                                                              cursor.x - canvas_rect.center().x,
                                                              cursor.y - canvas_rect.center().y,
                                                          );
                                                          state.pan = c - (c - state.pan) * (new_zoom / old_zoom);
                                                      }
                                                      state.zoom = new_zoom;
                                                  }
                                              }

                                              // Drag to pan
                                              if response.dragged_by(egui::PointerButton::Primary) {
                                                  state.pan += response.drag_delta();
                                              }

                                              // Cursor feedback
                                              if response.hovered() {
                                                  if response.dragged() {
                                                      ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                                  } else {
                                                      ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                                  }
                                              }

                                              // Clamp pan so image can't be dragged fully offscreen
                                              let scaled = texture_size * state.zoom;
                                              let half_excess = ((scaled - canvas_size) * 0.5).max(egui::Vec2::ZERO);
                                              let max_pan = half_excess + canvas_size * 0.4;
                                              state.pan = state.pan.clamp(-max_pan, max_pan);

                                              // Draw clipped to canvas
                                              let painter = ui.painter().with_clip_rect(canvas_rect);
                                              painter.image(
                                                  texture_id,
                                                  egui::Rect::from_center_size(canvas_rect.center() + state.pan, scaled),
                                                  egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                                  egui::Color32::WHITE,
                                              );
                                          } else if self.failed_loads.contains(&hash) {
                                              ui.centered_and_justified(|ui| {
                                                  ui.label(
                                                      egui::RichText::new(format!(
                                                          "Failed to load image: {}",
                                                          self.last_error.as_deref().unwrap_or("Unknown error")
                                                      ))
                                                      .color(egui::Color32::from_rgb(239, 68, 68)),
                                                  );
                                              });
                                          } else {
                                              ui.centered_and_justified(|ui| { ui.spinner(); });
                                          }
                                      } else if file_info.path.ends_with(".psg") {
                if let Some(psg_file) = self.psg_cache.get(&hash) {
                     let state = self.psg_viewer_state.entry(hash).or_default();
                     let show_graph = state.show_graph;
                     let mut viewer = crate::ui::psg_viewer::PsgViewer::new(state, psg_file);
                     
                     if show_graph {
                         viewer.show(ui);
                     } else {
                         // Still show the toggle button from the viewer
                         viewer.show(ui); // It handles the "Switch Back" button internally via state check
                         
                         // And show JSON below
                         if let Some(json) = self.json_cache.get(&hash) {
                             crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                         } else {
                             ui.label("JSON representation not available.");
                         }
                     }
                } else if let Some(json) = self.json_cache.get(&hash) {
                    crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                } else {
                    if let Some(err) = &self.last_error {
                        ui.colored_label(egui::Color32::RED, err);
                    }
                    if self.failed_loads.contains(&hash) {
                        ui.colored_label(egui::Color32::RED, "Failed to load PSG.");
                    } else {
                         ui.spinner();
                         ui.label("Loading PSG...");
                    }
                }
            } else if file_info.path.ends_with(".ogg") || file_info.path.ends_with(".wav") || file_info.path.ends_with(".mp3") {
                                           self.show_audio_player(ui, &reader, index, file_info, hash);
                                      } else if is_non_playable_media(&file_info.path) {
                                           self.show_media_stub(ui, file_info, hash);
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
                crate::ui::components::card(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Select a file to view content");
                        ui.add_space(6.0);
                        ui.label("Use the tree, command palette, or folder browser to inspect assets, data tables, textures, audio, and parsed formats.");
                    });
                });
            });
        }
    }

    fn show_folder_list(&mut self, ui: &mut egui::Ui, bundle_index: &Option<std::sync::Arc<crate::bundles::index::Index>>, hashes: Vec<u64>, name: String, path: String) {
        ui.label(
            egui::RichText::new(&path)
                .heading()
                .color(egui::Color32::from_rgb(236, 236, 240)),
        );
        ui.add_space(4.0);
        if let Some(index) = bundle_index {
            if self.folder_cache_index_size != index.files.len() {
                self.folder_children_cache.clear();
                self.folder_cache_index_size = index.files.len();
            }
        }

        let subfolders = bundle_index
            .as_ref()
            .map(|index| self.cached_immediate_subfolders(index, &path))
            .unwrap_or_default();
        let total_entries = subfolders.len() + hashes.len();
        ui.label(
            egui::RichText::new(format!("ENTRIES · {}", total_entries))
                .monospace()
                .size(10.5)
                .color(egui::Color32::from_rgb(113, 113, 122)),
        );
        ui.separator();

        if subfolders.is_empty() && hashes.is_empty() {
            ui.add_space(16.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} has no direct file entries.", name))
                        .color(egui::Color32::from_rgb(126, 126, 134)),
                );
            });
            return;
        }

        let mut files = Vec::new();
        if let Some(index) = bundle_index {
            for hash in hashes {
                if let Some(file) = index.files.get(&hash) {
                    files.push((hash, file));
                }
            }
        }
        files.sort_by(|a, b| a.1.path.cmp(&b.1.path));

        TableBuilder::new(ui)
            .striped(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(28.0))
            .column(Column::remainder().at_least(240.0))
            .column(Column::exact(84.0))
            .column(Column::exact(88.0))
            .column(Column::exact(132.0))
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.label(egui::RichText::new("").size(10.5));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("NAME").monospace().size(10.5).color(egui::Color32::from_rgb(113, 113, 122)));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("TYPE").monospace().size(10.5).color(egui::Color32::from_rgb(113, 113, 122)));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("SIZE").monospace().size(10.5).color(egui::Color32::from_rgb(113, 113, 122)));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("HASH").monospace().size(10.5).color(egui::Color32::from_rgb(113, 113, 122)));
                });
            })
            .body(|body| {
                let total_rows = subfolders.len() + files.len();
                body.rows(22.0, total_rows, |mut row| {
                    let row_index = row.index();

                    if row_index < subfolders.len() {
                        let (folder_name, folder_path, child_hashes) = &subfolders[row_index];

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new("▸")
                                    .monospace()
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(113, 113, 122)),
                            );
                        });

                        row.col(|ui| {
                            let response = ui.selectable_label(
                                false,
                                egui::RichText::new(folder_name).monospace().size(11.5),
                            );
                            if response.clicked() {
                                self.selection_requested = Some(crate::ui::app::FileSelection::Folder {
                                    hashes: child_hashes.clone(),
                                    name: folder_name.clone(),
                                    path: folder_path.clone(),
                                });
                            }
                            response.on_hover_text(folder_path);
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new("FOLDER")
                                    .monospace()
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(161, 161, 170)),
                            );
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{} files", child_hashes.len()))
                                    .size(10.8)
                                    .color(egui::Color32::from_rgb(161, 161, 170)),
                            );
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new("—")
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(113, 113, 122)),
                            );
                        });
                    } else {
                        let file_index = row_index - subfolders.len();
                        let (hash, file_info) = files[file_index];

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new("·")
                                    .monospace()
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(113, 113, 122)),
                            );
                        });

                        row.col(|ui| {
                            let name_text = display_name_from_path(&file_info.path);
                            let response = ui.selectable_label(
                                false,
                                egui::RichText::new(name_text).monospace().size(11.5),
                            );
                            if response.clicked() {
                                self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(hash));
                            }
                            response.on_hover_text(&file_info.path);
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(file_kind_label(&file_info.path).to_uppercase())
                                    .monospace()
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(120, 170, 210)),
                            );
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(format_file_size(file_info.file_size as u64))
                                    .size(10.8)
                                    .color(egui::Color32::from_rgb(161, 161, 170)),
                            );
                        });

                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:08x}", hash as u32))
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(161, 161, 170)),
                            );
                        });
                    }
                });
            });
    }

    fn cached_immediate_subfolders(&mut self, index: &crate::bundles::index::Index, path: &str) -> Vec<(String, String, Vec<u64>)> {
        if let Some(cached) = self.folder_children_cache.get(path) {
            return cached.clone();
        }

        let computed = Self::build_immediate_subfolders(index, path);
        self.folder_children_cache.insert(path.to_string(), computed.clone());
        computed
    }

    fn build_immediate_subfolders(index: &crate::bundles::index::Index, path: &str) -> Vec<(String, String, Vec<u64>)> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{}/", path)
        };

        let mut by_folder: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for (hash, file) in &index.files {
            if !file.path.starts_with(&prefix) {
                continue;
            }

            let remainder = &file.path[prefix.len()..];
            if let Some((segment, tail)) = remainder.split_once('/') {
                if segment.is_empty() {
                    continue;
                }

                let folder_path = format!("{}{}", prefix, segment);
                let entry = by_folder.entry(folder_path).or_default();

                if !tail.contains('/') {
                    entry.push(*hash);
                }
            }
        }

        let mut rows = Vec::with_capacity(by_folder.len());
        for (folder_path, mut direct_hashes) in by_folder {
            direct_hashes.sort_by(|a, b| {
                let path_a = index.files.get(a).map(|file| file.path.as_str()).unwrap_or("");
                let path_b = index.files.get(b).map(|file| file.path.as_str()).unwrap_or("");
                path_a.cmp(path_b)
            });

            let folder_name = folder_path.rsplit('/').next().unwrap_or(&folder_path).to_string();
            rows.push((folder_name, folder_path, direct_hashes));
        }

        rows
    }

    fn show_audio_player(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, index: &std::sync::Arc<crate::bundles::index::Index>, file_info: &crate::bundles::index::FileInfo, hash: u64) {
        ui.spacing_mut().item_spacing.y = 6.0;

        let file_name = std::path::Path::new(&file_info.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_info.path);
        let ext = std::path::Path::new(&file_info.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("audio")
            .to_uppercase();

        ui.label(
            egui::RichText::new(file_name)
                .size(13.0)
                .monospace()
                .color(egui::Color32::from_rgb(228, 228, 231)),
        );
        ui.label(
            egui::RichText::new(ext)
                .size(10.5)
                .monospace()
                .color(egui::Color32::from_rgb(113, 113, 122)),
        );

        ui.add_space(8.0);

        let is_playing = self.audio_sink.as_ref().map(|s| !s.empty()).unwrap_or(false);

        ui.horizontal(|ui| {
            if is_playing {
                if ui.button("■  Stop").clicked() {
                    if let Some(sink) = &self.audio_sink {
                        sink.stop();
                    }
                    self.audio_sink = None;
                }
            } else {
                if ui.button("▶  Play").clicked() {
                    self.load_bundled_content(ui.ctx(), reader, index, file_info, hash);
                }
            }
        });

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("VOLUME")
                    .size(10.5)
                    .monospace()
                    .color(egui::Color32::from_rgb(113, 113, 122)),
            );
            ui.add_space(6.0);
            if ui.add_sized(
                [140.0, 18.0],
                egui::Slider::new(&mut self.audio_volume, 0.0..=1.0).show_value(false),
            ).changed() {
                if let Some(sink) = &self.audio_sink {
                    sink.set_volume(self.audio_volume);
                }
            }
            ui.label(
                egui::RichText::new(format!("{:.0}%", self.audio_volume * 100.0))
                    .size(11.5)
                    .color(egui::Color32::from_rgb(161, 161, 170)),
            );
        });

        ui.add_space(8.0);

        // Status dot + label
        let (dot_color, status_text) = if is_playing {
            (egui::Color32::from_rgb(74, 222, 128), "Playing")
        } else {
            (egui::Color32::from_rgb(82, 82, 91), "Stopped")
        };
        ui.horizontal(|ui| {
            let top_left = ui.cursor().min;
            let dot_pos = egui::pos2(top_left.x + 5.0, top_left.y + 8.0);
            ui.painter().circle_filled(dot_pos, 4.0, dot_color);
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(status_text)
                    .size(12.0)
                    .color(egui::Color32::from_rgb(161, 161, 170)),
            );
        });

        if let Some(err) = &self.last_error.clone() {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(err)
                    .size(11.5)
                    .color(egui::Color32::from_rgb(239, 68, 68)),
            );
        }

        if is_playing {
            ui.ctx().request_repaint();
        }
    }

    fn show_media_stub(&mut self, ui: &mut egui::Ui, file_info: &crate::bundles::index::FileInfo, hash: u64) {
        ui.spacing_mut().item_spacing.y = 6.0;

        let path = &file_info.path;
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        ui.label(
            egui::RichText::new(file_name)
                .size(13.0)
                .monospace()
                .color(egui::Color32::from_rgb(228, 228, 231)),
        );

        let (format_label, description) = match ext.as_str() {
            "bk2" => ("BINK 2 VIDEO", "Bink 2 encoded video by RAD Game Tools. Export to play with any media player that supports .bk2 (e.g. RAD Video Tools, VLC with Bink plugin, or ffmpeg-bink)."),
            "wem" => ("WWISE AUDIO",  "Wwise Encoded Media. Export and convert with vgmstream or ww2ogg to play as standard audio."),
            "bank" => ("FMOD BANK",   "FMOD Sound Bank. Export and unpack with FMOD Bank Tools or fsbext to extract individual audio tracks."),
            "mp4" => ("MP4 VIDEO",    "MPEG-4 video. Export to play in any standard media player."),
            _     => ("MEDIA FILE",   "Export to play or inspect this file with an external tool."),
        };

        ui.label(
            egui::RichText::new(format_label)
                .size(10.5)
                .monospace()
                .color(egui::Color32::from_rgb(113, 113, 122)),
        );

        ui.add_space(8.0);

        // Bink header metadata if data is cached
        if ext == "bk2" {
            if let Some(data) = self.raw_data_cache.get(&hash) {
                if let Some(meta) = parse_bink_meta(data) {
                    egui::Grid::new("bink_meta")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Codec").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                            ui.label(egui::RichText::new(&meta.codec).size(11.5).monospace());
                            ui.end_row();
                            if let (Some(w), Some(h)) = (meta.width, meta.height) {
                                ui.label(egui::RichText::new("Resolution").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                                ui.label(egui::RichText::new(format!("{}×{}", w, h)).size(11.5).monospace());
                                ui.end_row();
                            }
                            if let Some(frames) = meta.frame_count {
                                ui.label(egui::RichText::new("Frames").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                                ui.label(egui::RichText::new(frames.to_string()).size(11.5).monospace());
                                ui.end_row();
                            }
                            if let Some(fps) = meta.fps {
                                ui.label(egui::RichText::new("FPS").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                                ui.label(egui::RichText::new(format!("{:.2}", fps)).size(11.5).monospace());
                                ui.end_row();
                            }
                            if let (Some(frames), Some(fps)) = (meta.frame_count, meta.fps) {
                                if fps > 0.0 {
                                    let dur = frames as f32 / fps;
                                    ui.label(egui::RichText::new("Duration").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                                    ui.label(egui::RichText::new(format!("{:.1}s", dur)).size(11.5).monospace());
                                    ui.end_row();
                                }
                            }
                            if let Some(tracks) = meta.audio_tracks {
                                ui.label(egui::RichText::new("Audio Tracks").size(11.5).color(egui::Color32::from_rgb(113, 113, 122)));
                                ui.label(egui::RichText::new(tracks.to_string()).size(11.5).monospace());
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                }
            }
        }

        if ui.button("Export File").clicked() {
            self.export_requested = Some((vec![hash], file_info.path.clone(), None));
        }

        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(description)
                .size(11.5)
                .color(egui::Color32::from_rgb(113, 113, 122))
                .italics(),
        );
    }

    fn show_ggpk_file(&mut self, ui: &mut egui::Ui, reader: &GgpkReader, offset: u64, is_poe2: bool) {
            match reader.read_file_record(offset) {
                Ok(file) => {
                    ui.heading(&file.name);
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        crate::ui::components::badge(ui, file_kind_label(&file.name));
                        crate::ui::components::badge(ui, &format_file_size(file.data_length));
                        crate::ui::components::badge(ui, &format!("Offset {}", file.offset));
                    });
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
                        // Try new format parsers
                        match reader.get_data_slice(file.data_offset, file.data_length) {
                            Ok(data) => {
                                if let Some(parsed) = parse_with_new_formats(&file.name, data) {
                                    // Store in cache for potential later use
                                    self.parsed_content_cache.insert(offset, parsed.clone());

                                    render_parsed_content(ui, &file.name, &parsed);
                                } else {
                                    // Fallback to hex view
                                    ui.label("Hex View (TODO)");
                                }
                            },
                            Err(e) => {
                                ui.label(format!("Read error: {}", e));
                            }
                        }
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
                             self.failed_loads.insert(hash);
                         }
                     }
                 } else {
                     let msg = format!("Bundle not found in GGPK and CDN Loader not initialized. Hash: {}", hash);
                     println!("{}", msg);
                     self.last_error = Some(msg);
                     self.failed_loads.insert(hash);
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
                                  } else if path.ends_with(".dds") || path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".jpeg") || path.ends_with(".webp") {
                                      // Try to load Image
                                      self.last_error = None;
                                      
                                      println!("Image Loading: Data Length {}", file_data.len());
                                      
                                      // Special handling for DDS
                                      if path.ends_with(".dds") {
                                          if file_data.len() > 16 {
                                              println!("DDS First 16 bytes: {:02X?}", &file_data[0..16]);
                                              let magic = &file_data[0..4];
                                              if magic == b"DDS " {
                                                  println!("Magic 'DDS ' confirmed.");
                                              } else {
                                                  println!("WARNING: Magic bytes mismatch! Expected 'DDS ', found {:?}", magic);
                                              }
                                          }
                                          
                                          // Method 1: Try image_dds first (better support for various DXT/BC formats for DDS)
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
                                          
                                          if !loaded {
                                               // Fallback to Method 2 below
                                          } else {
                                              self.failed_loads.remove(&hash);
                                              self.last_error = None;
                                              return;
                                          }
                                      }

                                      // Method 2: Standard image crate (supports png, jpg, webp, and some dds)
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
                                          self.failed_loads.remove(&hash);
                                          self.last_error = None;
                                      } else {
                                          let msg = format!("Failed to decode image. File size: {}", file_data.len());
                                          self.last_error = Some(msg);
                                          self.failed_loads.insert(hash);
                                      }
                                 } else if path.ends_with(".ogg") || path.ends_with(".wav") || path.ends_with(".mp3") {
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
                                              self.psg_cache.insert(hash, psg_file.clone());
                                              self.psg_viewer_state.entry(hash).or_default();
                                              
                                              // Convert PSG to Value for JSON view (fallback)
                                              if let Ok(v) = serde_json::to_value(&psg_file) {
                                                  Self::save_to_cache(hash, &v);
                                                  self.json_cache.insert(hash, v);
                                                  self.last_error = None;
                                              } else {
                                                   self.last_error = Some("Failed to serialize PSG to JSON".to_string());
                                                   // self.failed_loads.insert(hash); // Don't fail load if graph works?
                                              }
                                          },
                                          Err(e) => {
                                              // println!("PSG Parse Error: {}", e);
                                              self.last_error = Some(format!("PSG Parse Error: {}", e));
                                              self.raw_data_cache.insert(hash, file_data.clone());
                                              self.failed_loads.insert(hash);
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
    p.ends_with(".pshader") || p.ends_with(".fx") || p.ends_with(".mat") || p.ends_with(".csv") ||
    p.ends_with(".ao") || p.ends_with(".arm") || p.ends_with(".ddt") || p.ends_with(".ecf") ||
    p.ends_with(".et") || p.ends_with(".gft") || p.ends_with(".gt") || p.ends_with(".rs") || p.ends_with(".tsi") ||
    p.ends_with(".amd") || p.ends_with(".pet") || p.ends_with(".trl") || p.ends_with(".tmf") ||
    // Additional UCS-2 text config formats
    p.ends_with(".cht") || p.ends_with(".clt") || p.ends_with(".dct") || p.ends_with(".dlp") ||
    p.ends_with(".act") || p.ends_with(".ais") || p.ends_with(".aoc") || p.ends_with(".config") ||
    p.ends_with(".env") || p.ends_with(".ffx") || p.ends_with(".ot") || p.ends_with(".otc") ||
    p.ends_with(".tgt") || p.ends_with(".ui") || p.ends_with(".dgr") || p.ends_with(".sm") ||
    p.ends_with(".tmo") || p.ends_with(".arl") || p.ends_with(".atlas") || p.ends_with(".filter") ||
    p.ends_with(".chr") || p.ends_with(".tdf") || p.ends_with(".ot") || p.ends_with(".ais")
}

fn is_non_playable_media(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".bk2") || p.ends_with(".wem") || p.ends_with(".bank") || p.ends_with(".mp4")
}

struct BinkMeta {
    codec: String,
    frame_count: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    audio_tracks: Option<u32>,
}

fn parse_bink_meta(data: &[u8]) -> Option<BinkMeta> {
    if data.len() < 4 { return None; }
    let magic = std::str::from_utf8(&data[0..3]).ok()?;
    let version = data[3] as char;
    match magic {
        "BIK" => {
            let codec = format!("Bink 1 (v{})", version);
            if data.len() < 44 {
                return Some(BinkMeta { codec, frame_count: None, width: None, height: None, fps: None, audio_tracks: None });
            }
            let frame_count = u32::from_le_bytes(data[8..12].try_into().ok()?);
            let width       = u32::from_le_bytes(data[20..24].try_into().ok()?);
            let height      = u32::from_le_bytes(data[24..28].try_into().ok()?);
            let fps_num     = u32::from_le_bytes(data[28..32].try_into().ok()?);
            let fps_den     = u32::from_le_bytes(data[32..36].try_into().ok()?);
            let audio_tracks = u32::from_le_bytes(data[40..44].try_into().ok()?);
            let fps = if fps_den > 0 { Some(fps_num as f32 / fps_den as f32) } else { None };
            Some(BinkMeta { codec, frame_count: Some(frame_count), width: Some(width), height: Some(height), fps, audio_tracks: Some(audio_tracks) })
        }
        "KB2" => {
            // Bink 2: 0=magic(4), 4=filesize, 8=num_frames, 12=largest_frame, 16=fps_num, 20=fps_den, 24=flags, 28=num_audio_tracks, 32=width, 36=height
            let codec = format!("Bink 2 (v{})", version);
            if data.len() < 40 {
                return Some(BinkMeta { codec, frame_count: None, width: None, height: None, fps: None, audio_tracks: None });
            }
            let frame_count  = u32::from_le_bytes(data[8..12].try_into().ok()?);
            let fps_num      = u32::from_le_bytes(data[16..20].try_into().ok()?);
            let fps_den      = u32::from_le_bytes(data[20..24].try_into().ok()?);
            let audio_tracks = u32::from_le_bytes(data[28..32].try_into().ok()?);
            let width        = u32::from_le_bytes(data[32..36].try_into().ok()?);
            let height       = u32::from_le_bytes(data[36..40].try_into().ok()?);
            let fps = if fps_den > 0 { Some(fps_num as f32 / fps_den as f32) } else { None };
            Some(BinkMeta { codec, frame_count: Some(frame_count), width: Some(width), height: Some(height), fps, audio_tracks: Some(audio_tracks) })
        }
        _ => None,
    }
}

fn is_image_path(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".dds") || p.ends_with(".png") || p.ends_with(".jpg") || p.ends_with(".jpeg") || p.ends_with(".webp")
}

fn display_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn format_file_size(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let size_f = size as f64;
    if size_f >= GB {
        format!("{:.2} GB", size_f / GB)
    } else if size_f >= MB {
        format!("{:.1} MB", size_f / MB)
    } else if size_f >= KB {
        format!("{:.1} KB", size_f / KB)
    } else {
        format!("{} B", size)
    }
}

fn file_kind_label(path: &str) -> &'static str {
    let p = path.to_lowercase();
    if is_image_path(&p) {
        "IMAGE"
    } else if p.ends_with(".bk2") || p.ends_with(".mp4") {
        "VIDEO"
    } else if p.ends_with(".ogg") || p.ends_with(".wem") || p.ends_with(".wav") || p.ends_with(".mp3") || p.ends_with(".bank") {
        "AUDIO"
    } else if p.ends_with(".dat") || p.ends_with(".dat64") || p.ends_with(".datc64") || p.ends_with(".datl") || p.ends_with(".datl64") {
        "DATA"
    } else if p.ends_with(".json") || is_text_file(&p) {
        "TEXT"
    } else if p.ends_with(".psg") {
        "GRAPH"
    } else {
        "BINARY"
    }
}

fn is_supported_format(path: &str) -> Option<crate::parsers::FileFormat> {
    let format = crate::parsers::FileFormat::from_extension(path);
    if format != crate::parsers::FileFormat::Unknown {
        Some(format)
    } else {
        None
    }
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

fn parse_with_new_formats(path: &str, data: &[u8]) -> Option<crate::parsers::ParsedContent> {
    if let Some(format) = is_supported_format(path) {
        match crate::parsers::parse(format, data) {
            Ok(content) => Some(content),
            Err(_) => None, // Fallback to other viewers
        }
    } else {
        None
    }
}

fn render_parsed_content(ui: &mut egui::Ui, file_name: &str, parsed: &crate::parsers::ParsedContent) {
    let format = crate::parsers::FileFormat::from_extension(file_name);

    match format {
        crate::parsers::FileFormat::FMT | crate::parsers::FileFormat::GT | crate::parsers::FileFormat::GFT | crate::parsers::FileFormat::ECF => {
            GraphicsViewer::show(ui, file_name, parsed);
        }
        crate::parsers::FileFormat::SMD => {
            SkeletalViewer::show(ui, file_name, parsed);
        }
        _ => {
            TextConfigViewer::show(ui, file_name, parsed);
        }
    }
}


