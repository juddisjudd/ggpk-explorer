use eframe::egui;
use crate::ggpk::reader::GgpkReader;
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

// Caps to keep memory bounded as the user browses many files.
// Textures live in VRAM (PoE DDS textures are frequently 2K-4K = tens of MB
// each); without eviction the GPU driver eventually spills to system memory and
// every repaint — including window drags — stalls. Dropping a TextureHandle
// frees the VRAM, so we keep a small most-recently-used set.
const MAX_CACHED_TEXTURES: usize = 16;
const MAX_RAW_CACHE_BYTES: usize = 96 * 1024 * 1024; // raw file bytes in system RAM

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
    texture_lru: Vec<u64>,
    raw_data_cache: HashMap<u64, Vec<u8>>,
    raw_cache_order: Vec<u64>,
    raw_cache_bytes: usize,
    pub csd_cache: HashMap<u64, csd::CsdFile>,
    csd_viewer_state: HashMap<u64, crate::ui::csd_viewer::CsdViewerState>,
    pub json_cache: HashMap<u64, serde_json::Value>,
    /// Decoded text of loaded text files, so viewers don't re-decode every frame.
    text_cache: HashMap<u64, std::sync::Arc<String>>,
    text_filters: HashMap<u64, String>,
    /// Files the user switched to the plain editor view.
    raw_text_view: std::collections::HashSet<u64>,
    object_cache: HashMap<u64, std::sync::Arc<crate::parsers::object_dsl::ObjectFile>>,
    object_viewer_state: HashMap<u64, crate::ui::object_viewer::ObjectViewerState>,
    curve_cache: HashMap<u64, std::sync::Arc<crate::parsers::curves::CurveFile>>,
    curve_viewer_state: HashMap<u64, crate::ui::curve_viewer::CurveViewerState>,
    level_cache: HashMap<u64, Result<std::sync::Arc<crate::parsers::level::DgrGraph>, String>>,
    level_viewer_state: HashMap<u64, crate::ui::level_viewer::LevelViewerState>,
    /// JSON body (after any `version` header) of `.mat/.env/.atl/.pet` files; `None` when not JSON.
    structured_json: HashMap<u64, Option<(String, std::sync::Arc<serde_json::Value>)>>,
    /// Downscaled texture previews by path (`None` = could not decode), oldest first in `thumb_order`.
    thumb_cache: HashMap<String, Option<egui::TextureHandle>>,
    thumb_order: Vec<String>,
    thumb_pending: std::collections::HashSet<String>,
    thumb_rx: Option<std::sync::mpsc::Receiver<Vec<(String, Option<egui::ColorImage>)>>>,
    /// Drawable geometry per model file; `None` when the file has no vertices.
    mesh_cache: HashMap<u64, Option<std::sync::Arc<crate::ui::mesh_preview::MeshData>>>,
    mesh_view_state: HashMap<u64, crate::ui::mesh_preview::MeshPreviewState>,
    /// Models the user switched from the 3D preview to the structure summary.
    model_summary_view: std::collections::HashSet<u64>,
    pub dat_viewer: DatViewer,
    /// The output device owns the mixer a player connects to.
    audio_device: Option<rodio::MixerDeviceSink>,
    audio_sink: Option<rodio::Player>,
    pub last_error: Option<String>,
    pub failed_loads: std::collections::HashSet<u64>,
    image_view_states: HashMap<u64, ImageViewState>,

    pub cdn_loader: Option<crate::bundles::cdn::CdnBundleLoader>,
    pub steam_loader: Option<crate::bundles::steam::SteamBundleLoader>,
    pub audio_volume: f32,

    pub export_requested: Option<(Vec<u64>, String, Option<crate::ui::export_window::ExportSettings>)>,
    pub selection_requested: Option<crate::ui::app::FileSelection>,

    // Remembers the search-results view a file was opened from (keyed by
    // that file's hash) so a "Back to results" link can return to it
    // without keeping a full navigation history.
    back_target: Option<(u64, crate::ui::app::FileSelection)>,

    // Extension filter for the search-results view. Keyed to the search
    // term so a brand-new search starts unfiltered, but persists across a
    // "back to results" round trip for the same search.
    search_filter_term: String,
    search_filter_exts: std::collections::HashSet<String>,

    pub psg_cache: HashMap<u64, crate::dat::psg::PsgFile>,
    pub psg_viewer_state: HashMap<u64, crate::ui::psg_viewer::PsgViewerState>,
    pub fxgraph_cache: HashMap<u64, crate::parsers::fxgraph::FxGraph>,
    pub fxgraph_viewer_state: HashMap<u64, crate::ui::fxgraph_viewer::FxGraphViewerState>,
    folder_children_cache: HashMap<String, Vec<(String, String, Vec<u64>)>>,
    folder_cache_index_size: usize,

    pub parsed_content_cache: HashMap<u64, crate::parsers::ParsedContent>,
    /// Parsed `.ast/.fmt/.tgm/.smd` files plus their display summary.
    pub model_cache: HashMap<u64, (std::sync::Arc<crate::parsers::model::ModelFile>, serde_json::Value)>,

    // FMOD .bank viewer state: parsed stream listings, decoded streams
    // (keyed by (file hash, stream index)), and the in-flight background
    // decode / export-all jobs.
    pub bank_info_cache: HashMap<u64, crate::parsers::fmod_bank::FmodBankInfo>,
    bank_stream_cache: HashMap<(u64, usize), Vec<u8>>,
    bank_decode_rx: Option<std::sync::mpsc::Receiver<(u64, usize, Result<Vec<u8>, String>)>>,
    bank_decoding: Option<(u64, usize)>,
    bank_decode_intent: BankStreamIntent,
    bank_playing: Option<(u64, usize)>,
    bank_export_rx: Option<std::sync::mpsc::Receiver<String>>,
    bank_export_status: Option<String>,

    // Atlas skill graph node database (PassiveSkillGraphId -> name/stats),
    // resolved once in the background from PassiveSkills/Stats DAT tables +
    // the atlas stat-description CSD files, shared by every open atlas .psg.
    pub skill_graph_db: Option<std::sync::Arc<crate::ui::atlas_node_db::SkillGraphDatabase>>,
    skill_graph_db_rx: Option<std::sync::mpsc::Receiver<Result<crate::ui::atlas_node_db::SkillGraphDatabase, String>>>,
    table_stats_rx: Option<std::sync::mpsc::Receiver<Vec<crate::dat::analysis::TableStats>>>,
    /// Identity of the index the DAT viewer's table stats were computed for.
    table_stats_for: usize,
    skill_graph_db_loading: bool,

    // DDS textures referenced by skill graph art (node icons, frames,
    // connectors, group backgrounds) — path-keyed, shared across every open
    // .psg viewer. Fetched/decoded lazily in small batches as `psg_viewer`
    // discovers which paths the currently-open tree actually needs.
    pub psg_texture_cache: HashMap<String, egui::TextureHandle>,
    /// In-flight skill tree export: file hash it was started from + status channel.
    tree_export_rx: Option<(u64, std::sync::mpsc::Receiver<crate::export::ExportStatus>)>,
    psg_texture_pending: std::collections::HashSet<String>,
    /// Paths that could not be resolved/decoded — never re-requested.
    psg_texture_failed: std::collections::HashSet<String>,
    psg_texture_rx: Option<std::sync::mpsc::Receiver<Vec<(String, Option<egui::ColorImage>)>>>,
}

#[derive(Clone, Copy, PartialEq)]
enum BankStreamIntent {
    Play,
    Export,
}

impl Default for ContentView {
    fn default() -> Self {
        Self {
            texture_cache: HashMap::new(),
            texture_lru: Vec::new(),
            raw_data_cache: HashMap::new(),
            raw_cache_order: Vec::new(),
            raw_cache_bytes: 0,
            csd_cache: HashMap::new(),
            csd_viewer_state: HashMap::new(),
            json_cache: HashMap::new(),
            text_cache: HashMap::new(),
            text_filters: HashMap::new(),
            raw_text_view: std::collections::HashSet::new(),
            object_cache: HashMap::new(),
            object_viewer_state: HashMap::new(),
            curve_cache: HashMap::new(),
            curve_viewer_state: HashMap::new(),
            level_cache: HashMap::new(),
            level_viewer_state: HashMap::new(),
            structured_json: HashMap::new(),
            thumb_cache: HashMap::new(),
            thumb_order: Vec::new(),
            thumb_pending: std::collections::HashSet::new(),
            thumb_rx: None,
            mesh_cache: HashMap::new(),
            mesh_view_state: HashMap::new(),
            model_summary_view: std::collections::HashSet::new(),
            dat_viewer: DatViewer::default(),
            audio_device: None,
            audio_sink: None,
            last_error: None,
            failed_loads: std::collections::HashSet::new(),
            image_view_states: HashMap::new(),

            cdn_loader: None,
            steam_loader: None,
            audio_volume: 0.5,
            export_requested: None,
            selection_requested: None,
            back_target: None,
            search_filter_term: String::new(),
            search_filter_exts: std::collections::HashSet::new(),

            psg_cache: HashMap::new(),
            psg_viewer_state: HashMap::new(),
            fxgraph_cache: HashMap::new(),
            fxgraph_viewer_state: HashMap::new(),
            folder_children_cache: HashMap::new(),
            folder_cache_index_size: 0,
            parsed_content_cache: HashMap::new(),
            model_cache: HashMap::new(),

            bank_info_cache: HashMap::new(),
            bank_stream_cache: HashMap::new(),
            bank_decode_rx: None,
            bank_decoding: None,
            bank_decode_intent: BankStreamIntent::Play,
            bank_playing: None,
            bank_export_rx: None,
            bank_export_status: None,

            skill_graph_db: None,
            skill_graph_db_rx: None,
            table_stats_rx: None,
            table_stats_for: 0,
            skill_graph_db_loading: false,

            psg_texture_cache: HashMap::new(),
            tree_export_rx: None,
            psg_texture_pending: std::collections::HashSet::new(),
            psg_texture_failed: std::collections::HashSet::new(),
            psg_texture_rx: None,
        }
    }
}

use crate::ui::app::FileSelection;


impl ContentView {
    pub fn set_cdn_loader(&mut self, loader: crate::bundles::cdn::CdnBundleLoader) {
        self.cdn_loader = Some(loader);
    }

    pub fn set_steam_loader(&mut self, loader: crate::bundles::steam::SteamBundleLoader) {
        self.steam_loader = Some(loader);
    }

    pub fn update_cdn_version(&mut self, ver: &str) {
        if let Some(loader) = &mut self.cdn_loader {
            loader.set_patch_version(ver);
        }
    }
    
    pub fn set_dat_schema(&mut self, schema: crate::dat::schema::Schema, created_at: String) {
        self.dat_viewer.set_schema(schema, created_at);
    }

    /// Insert a decoded texture, evicting the least-recently-used one(s) so the
    /// GPU texture set stays bounded (see `MAX_CACHED_TEXTURES`).
    fn insert_texture(&mut self, hash: u64, texture: egui::TextureHandle) {
        self.texture_cache.insert(hash, texture);
        self.touch_texture(hash);
        while self.texture_lru.len() > MAX_CACHED_TEXTURES {
            let evicted = self.texture_lru.remove(0);
            // Dropping the TextureHandle releases the VRAM on the next frame.
            self.texture_cache.remove(&evicted);
            self.image_view_states.remove(&evicted);
        }
    }

    /// Mark a texture as most-recently-used so it survives eviction.
    fn touch_texture(&mut self, hash: u64) {
        if let Some(pos) = self.texture_lru.iter().position(|&h| h == hash) {
            self.texture_lru.remove(pos);
        }
        self.texture_lru.push(hash);
    }

    /// Insert raw file bytes, evicting oldest entries to stay under the RAM cap.
    fn insert_raw(&mut self, hash: u64, data: Vec<u8>) {
        if let Some(old) = self.raw_data_cache.remove(&hash) {
            self.raw_cache_bytes = self.raw_cache_bytes.saturating_sub(old.len());
            if let Some(pos) = self.raw_cache_order.iter().position(|&h| h == hash) {
                self.raw_cache_order.remove(pos);
            }
        }
        self.raw_cache_bytes += data.len();
        self.raw_data_cache.insert(hash, data);
        self.raw_cache_order.push(hash);
        // Keep the just-inserted (currently viewed) entry; evict older ones.
        while self.raw_cache_bytes > MAX_RAW_CACHE_BYTES && self.raw_cache_order.len() > 1 {
            let evicted = self.raw_cache_order.remove(0);
            if let Some(d) = self.raw_data_cache.remove(&evicted) {
                self.raw_cache_bytes = self.raw_cache_bytes.saturating_sub(d.len());
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, reader: Option<std::sync::Arc<crate::ggpk::reader::GgpkReader>>, selection: Option<FileSelection>, is_poe2: bool, bundle_index: &Option<std::sync::Arc<crate::bundles::index::Index>>) {
        if let Some(rx) = &self.skill_graph_db_rx {
            if let Ok(result) = rx.try_recv() {
                self.skill_graph_db_rx = None;
                self.skill_graph_db_loading = false;
                match result {
                    Ok(db) => self.skill_graph_db = Some(std::sync::Arc::new(db)),
                    Err(e) => println!("Atlas skill DB build failed: {}", e),
                }
            }
        }

        self.poll_thumbnails(ui.ctx());
        self.poll_tree_export();
        if let Some(rx) = &self.psg_texture_rx {
            if let Ok(batch) = rx.try_recv() {
                self.psg_texture_rx = None;
                for (path, img) in batch {
                    self.psg_texture_pending.remove(&path);
                    if img.is_none() {
                        self.psg_texture_failed.insert(path.clone());
                    }
                    if let Some(color_image) = img {
                        // Connector sheets are tiled along straight lines.
                        let options = if crate::ui::psg_viewer::is_connector_sheet(&path) {
                            egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..Default::default() }
                        } else {
                            egui::TextureOptions::default()
                        };
                        let handle = ui.ctx().load_texture(&path, color_image, options);
                        self.psg_texture_cache.insert(path, handle);
                    }
                }
            }
        }

        if let Some(selection) = selection {
            match selection {
                FileSelection::GgpkOffset(offset) => {
                    if let Some(reader) = &reader {
                        self.show_ggpk_file(ui, reader, offset, is_poe2);
                    }
                },
                 FileSelection::Folder { hashes, name, path } => {
                     self.show_folder_list(ui, bundle_index, hashes, name, path);
                },
                FileSelection::SearchResults { term, hashes } => {
                    self.show_search_results(ui, bundle_index, term, hashes);
                },
                FileSelection::BundleFile(hash) => {
                    if let Some(index) = bundle_index {
                        if let Some(file_info) = index.files.get(&hash) {


                             // Auto-load logic
                             let mut perform_load = false;
                             
                             if is_image_path(&file_info.path) {
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
                             } else if file_info.path.ends_with(".fxgraph") {
                                 if !self.fxgraph_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if is_json_path(&file_info.path) {
                                 if !self.json_cache.contains_key(&hash) && !self.raw_data_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if crate::parsers::model::is_model_path(&file_info.path) {
                                 if !self.model_cache.contains_key(&hash) && !self.raw_data_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
                             } else if file_info.path.ends_with(".ogg") || file_info.path.ends_with(".wav") || file_info.path.ends_with(".mp3") {
                                 // Audio: play on demand, no auto-load needed
                             } else if file_info.path.ends_with(".bank") {
                                 // FMOD bank: load bytes + parse stream listing (no decode)
                                 if !self.bank_info_cache.contains_key(&hash) && !self.raw_data_cache.contains_key(&hash) {
                                     perform_load = true;
                                 }
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

                             if let Some((back_hash, _)) = &self.back_target {
                                 if *back_hash == hash {
                                     let response = ui.selectable_label(
                                         false,
                                         egui::RichText::new("← Back to search results").size(11.5),
                                     );
                                     if response.clicked() {
                                         self.selection_requested = self.back_target.take().map(|(_, sel)| sel);
                                     }
                                     ui.add_space(4.0);
                                 }
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
                                 self.load_bundled_content(ui.ctx(), reader.as_deref(), index, file_info, hash);
                             }
                             
                              if file_info.path.ends_with(".dat") || file_info.path.ends_with(".dat64") || file_info.path.ends_with(".datc64") || file_info.path.ends_with(".datl") || file_info.path.ends_with(".datl64") {
                                   // DatViewer handles its own scrolling via TableBuilder
                                   // If dat viewer has error, show generic hex views?
                                   if self.dat_viewer.error_msg.is_some() || self.dat_viewer.reader.is_none() {
                                       egui::ScrollArea::vertical().show(ui, |ui| {
                                           if let Some(last_err) = &self.last_error {
                                               ui.horizontal(|ui| {
                                                   ui.colored_label(egui::Color32::from_rgb(239, 68, 68), "❌ Failed to load DAT:");
                                               });
                                               ui.label(last_err);
                                               ui.add_space(8.0);
                                           }
                                           if let Some(data) = self.raw_data_cache.get(&hash) {
                                               ui.label("Showing raw hex view:");
                                               crate::ui::hex_viewer::HexViewer::show(ui, data);
                                           } else if self.last_error.is_none() {
                                               self.dat_viewer.show(ui, is_poe2, None); // Show failed state
                                           }
                                       });
                                   } else {
                                       let index_arc = index.clone();
                                       let reader_arc = reader.clone();
                                       let steam = self.steam_loader.clone();
                                       let mut loader = |p: &str| -> Option<Vec<u8>> {
                                           let fi = find_file_info_by_path(&index_arc, p)?;
                                           extract_bundle_file_sync(fi, &index_arc, reader_arc.as_deref(), steam.as_ref())
                                       };
                                       self.dat_viewer.show(ui, is_poe2, Some(&mut loader));
                                       self.handle_dat_nav(index);
                                       self.service_table_stats(ui.ctx(), reader.clone(), index);
                                   }
                              } else if file_info.path.ends_with(".csd") {
                                 let mut opened = None;
                                 if let Some(csd) = self.csd_cache.get(&hash) {
                                     let state = self.csd_viewer_state.entry(hash).or_default();
                                     opened = crate::ui::csd_viewer::CsdViewer::show(ui, hash, csd, state);
                                 } else if self.failed_loads.contains(&hash) {
                                     ui.colored_label(egui::Color32::RED, self.last_error.as_deref().unwrap_or("Failed to parse CSD."));
                                 } else {
                                     ui.spinner();
                                 }
                                 if let Some(p) = opened {
                                     self.open_path(index, &p);
                                 }
                            } else if file_info.path.ends_with(".psg") {
                                 if self.psg_cache.contains_key(&hash) {
                                     self.ensure_skill_graph_db_loading(reader.clone(), index);
                                     if let Some(db) = self.skill_graph_db.clone() {
                                         let needed = self.psg_cache.get(&hash).map(|psg| collect_needed_texture_paths(psg, &db));
                                         if let Some(needed) = needed {
                                             self.ensure_psg_textures_loading(reader.clone(), index, needed);
                                         }
                                     }
                                 }
                                 let is_loading_art = self.is_psg_art_loading();
                                 if let Some(psg_file) = self.psg_cache.get(&hash) {
                                     let state = self.psg_viewer_state.entry(hash).or_default();
                                     state.skill_db = self.skill_graph_db.clone();
                                     let show_graph = state.show_graph;
                                     let mut viewer = crate::ui::psg_viewer::PsgViewer::new(state, psg_file, &self.psg_texture_cache, is_loading_art);
                                     viewer.art_pending = self.psg_texture_pending.len();

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
                                     if state.export_requested {
                                         state.export_requested = false;
                                         let psg_file = psg_file.clone();
                                         let path = file_info.path.clone();
                                         self.start_tree_export(hash, path, psg_file, reader.clone(), index);
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
                            } else if file_info.path.ends_with(".fxgraph") {
                                if let Some(graph) = self.fxgraph_cache.get(&hash) {
                                    let state = self.fxgraph_viewer_state.entry(hash).or_default();
                                    let show_graph = state.show_graph;
                                    let mut viewer = crate::ui::fxgraph_viewer::FxGraphViewer::new(state, graph);
                                    let opened_texture = if show_graph {
                                        viewer.show(ui)
                                    } else {
                                        let opened = viewer.show(ui);
                                        if let Some(json) = self.json_cache.get(&hash) {
                                            crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                                        } else {
                                            ui.label("JSON representation not available.");
                                        }
                                        opened
                                    };
                                    if let Some(path) = opened_texture {
                                        if let Some(target_hash) = index.files.iter()
                                            .find(|(_, f)| f.path.eq_ignore_ascii_case(&path))
                                            .map(|(h, _)| *h)
                                        {
                                            self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(target_hash));
                                        } else {
                                            self.last_error = Some(format!("Texture not found in index: {}", path));
                                        }
                                    }
                                } else if let Some(json) = self.json_cache.get(&hash) {
                                    crate::ui::json_viewer::JsonTreeViewer::show(ui, json);
                                } else {
                                    if let Some(err) = &self.last_error {
                                        ui.colored_label(egui::Color32::RED, err);
                                    }
                                    if self.failed_loads.contains(&hash) {
                                        ui.colored_label(egui::Color32::RED, "Failed to load FX graph.");
                                    } else {
                                         ui.spinner();
                                         ui.label("Loading FX graph...");
                                    }
                                }
                            } else if crate::parsers::model::is_model_path(&file_info.path) {
                                 if let Some((model, summary)) = self.model_cache.get(&hash).cloned() {
                                     let mesh = self.mesh_cache.entry(hash).or_insert_with(|| crate::ui::mesh_preview::extract(&model).map(std::sync::Arc::new)).clone();
                                     let mut summary_view = self.model_summary_view.contains(&hash) || mesh.is_none();
                                     if mesh.is_some() {
                                         ui.horizontal(|ui| {
                                             if ui.toggle_value(&mut summary_view, "Summary").on_hover_text("Structure and stats instead of the 3D preview").changed() {
                                                 if summary_view { self.model_summary_view.insert(hash); } else { self.model_summary_view.remove(&hash); }
                                             }
                                         });
                                     }
                                     match (&mesh, summary_view) {
                                         (Some(mesh), false) => {
                                             let state = self.mesh_view_state.entry(hash).or_default();
                                             crate::ui::mesh_preview::MeshPreview::show(ui, hash, mesh, state);
                                         }
                                         _ => crate::ui::model_viewer::ModelViewer::show(ui, &file_info.path, &model, &summary),
                                     }
                                 } else if let Some(data) = self.raw_data_cache.get(&hash) {
                                     egui::ScrollArea::vertical().show(ui, |ui| {
                                         if let Some(err) = &self.last_error {
                                             ui.colored_label(egui::Color32::from_rgb(239, 68, 68), format!("❌ {}", err));
                                         }
                                         ui.label("Showing raw hex view:");
                                         crate::ui::hex_viewer::HexViewer::show(ui, data);
                                     });
                                 } else if self.failed_loads.contains(&hash) {
                                     ui.colored_label(egui::Color32::RED, self.last_error.as_deref().unwrap_or("Failed to load."));
                                 } else {
                                     ui.spinner();
                                 }
                            } else if is_json_path(&file_info.path) {
                                 let mut opened = None;
                                 if let Some(job) = self.json_cache.get(&hash) {
                                     egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                                         JsonTreeViewer::show_linked(ui, job, &mut opened);
                                     });
                                 } else if self.failed_loads.contains(&hash) {
                                      ui.label(format!("Failed to load JSON. Error: {}", self.last_error.as_deref().unwrap_or("Unknown")));
                                 } else {
                                      ui.label("Loading JSON...");
                                 }
                                 if let Some(p) = opened {
                                     self.open_path(index, &p);
                                 }
                            } else {
                                 // For other content, use ScrollArea
                                      if is_image_path(&file_info.path) {
                                          let texture_info = self.texture_cache.get(&hash)
                                              .map(|t| (t.id(), t.size_vec2()));
                                          if let Some((texture_id, texture_size)) = texture_info {
                                              self.touch_texture(hash);
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
                                                          .color(if ui.visuals().dark_mode {
                                                              egui::Color32::from_rgb(161, 161, 170)
                                                          } else {
                                                              egui::Color32::from_rgb(70, 70, 80)
                                                          }),
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
                                                          .color(if ui.visuals().dark_mode {
                                                              egui::Color32::from_rgb(113, 113, 122)
                                                          } else {
                                                              egui::Color32::from_rgb(100, 100, 110)
                                                          }),
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
                let is_loading_art = self.is_psg_art_loading();
                if let Some(psg_file) = self.psg_cache.get(&hash) {
                     let state = self.psg_viewer_state.entry(hash).or_default();
                     state.skill_db = self.skill_graph_db.clone();
                     let show_graph = state.show_graph;
                     let mut viewer = crate::ui::psg_viewer::PsgViewer::new(state, psg_file, &self.psg_texture_cache, is_loading_art);
                     viewer.art_pending = self.psg_texture_pending.len();
                     
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
                                           self.show_audio_player(ui, reader.as_deref(), index, file_info, hash);
                                      } else if file_info.path.ends_with(".bank") {
                                           if self.bank_info_cache.contains_key(&hash) {
                                               self.show_bank_viewer(ui, file_info, hash);
                                           } else {
                                               self.show_media_stub(ui, file_info, hash, reader.as_deref(), bundle_index.as_ref().map(|i| i.as_ref()));
                                           }
                                      } else if is_non_playable_media(&file_info.path) {
                                           self.show_media_stub(ui, file_info, hash, reader.as_deref(), bundle_index.as_ref().map(|i| i.as_ref()));
                                      } else if is_structured_text(&file_info.path) {
                                           match self.decoded_text(hash) {
                                                Some(text) => {
                                                    let ext = file_info.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                                                    let mut raw = self.raw_text_view.contains(&hash);
                                                    ui.horizontal(|ui| {
                                                        if ui.toggle_value(&mut raw, "Raw").on_hover_text("Show the file text instead of the dedicated view").changed() {
                                                            if raw { self.raw_text_view.insert(hash); } else { self.raw_text_view.remove(&hash); }
                                                        }
                                                    });
                                                    let mut wanted_thumbs: Vec<String> = Vec::new();
                                                    let opened = if raw {
                                                        self.show_linked_text(ui, hash, &text)
                                                    } else {
                                                        match ext.as_str() {
                                                            "dgr" => {
                                                                let parsed = self.level_cache.entry(hash).or_insert_with(|| crate::parsers::level::parse_dgr(&text).map(std::sync::Arc::new)).clone();
                                                                match parsed {
                                                                    Ok(graph) => {
                                                                        let state = self.level_viewer_state.entry(hash).or_default();
                                                                        crate::ui::level_viewer::LevelViewer::show(ui, hash, &graph, state)
                                                                    }
                                                                    Err(e) => {
                                                                        ui.colored_label(egui::Color32::from_rgb(245, 158, 11), format!("⚠ Could not read the graph: {}", e));
                                                                        self.show_linked_text(ui, hash, &text)
                                                                    }
                                                                }
                                                            }
                                                            "trl" => self.show_curves(ui, hash, &text),
                                                            "mat" => match self.structured_json(hash, &text) {
                                                                Some((_, doc)) => crate::ui::material_viewer::MaterialViewer::show(ui, hash, &doc, &self.thumb_cache, &mut wanted_thumbs),
                                                                None => self.show_linked_text(ui, hash, &text),
                                                            },
                                                            "atl" => match self.structured_json(hash, &text) {
                                                                Some((_, doc)) => crate::ui::timeline_viewer::TimelineViewer::show(ui, hash, &doc),
                                                                None => self.show_linked_text(ui, hash, &text),
                                                            },
                                                            _ => match self.structured_json(hash, &text) {
                                                                Some((header, doc)) => {
                                                                    let mut o = None;
                                                                    if !header.is_empty() {
                                                                        ui.label(egui::RichText::new(header).weak().monospace());
                                                                    }
                                                                    egui::ScrollArea::both().id_salt(("structured_json", hash)).auto_shrink([false, false]).show(ui, |ui| {
                                                                        JsonTreeViewer::show_linked(ui, &doc, &mut o);
                                                                    });
                                                                    o
                                                                }
                                                                None if ext == "pet" => self.show_curves(ui, hash, &text),
                                                                None => self.show_linked_text(ui, hash, &text),
                                                            },
                                                        }
                                                    };
                                                    if !wanted_thumbs.is_empty() {
                                                        self.ensure_thumbnails(reader.clone(), index, wanted_thumbs);
                                                    }
                                                    if let Some(p) = opened {
                                                        self.open_path(index, &p);
                                                    }
                                                }
                                                None => {
                                                    ui.label("Loading...");
                                                }
                                           }
                                      } else if crate::parsers::object_dsl::is_object_path(&file_info.path) {
                                           match self.decoded_text(hash) {
                                                Some(text) => {
                                                    let mut raw = self.raw_text_view.contains(&hash);
                                                    ui.horizontal(|ui| {
                                                        if ui.toggle_value(&mut raw, "Raw").on_hover_text("Show the file text instead of the inspector").changed() {
                                                            if raw { self.raw_text_view.insert(hash); } else { self.raw_text_view.remove(&hash); }
                                                        }
                                                    });
                                                    let opened = if raw {
                                                        let mut filter = self.text_filters.remove(&hash).unwrap_or_default();
                                                        let o = crate::ui::linked_text_viewer::LinkedTextViewer::show(ui, hash, &text, &mut filter);
                                                        self.text_filters.insert(hash, filter);
                                                        o
                                                    } else {
                                                        let doc = self.object_cache.entry(hash).or_insert_with(|| std::sync::Arc::new(crate::parsers::object_dsl::parse(&text))).clone();
                                                        let index_arc = index.clone();
                                                        let reader_arc = reader.clone();
                                                        let steam = self.steam_loader.clone();
                                                        let mut loader = |p: &str| -> Option<Vec<u8>> {
                                                            let fi = find_file_info_by_path(&index_arc, p)?;
                                                            extract_bundle_file_sync(fi, &index_arc, reader_arc.as_deref(), steam.as_ref())
                                                        };
                                                        let state = self.object_viewer_state.entry(hash).or_default();
                                                        crate::ui::object_viewer::ObjectViewer::show(ui, hash, &file_info.path, &doc, state, Some(&mut loader))
                                                    };
                                                    if let Some(p) = opened {
                                                        self.open_path(index, &p);
                                                    }
                                                }
                                                None => {
                                                    ui.label("Loading...");
                                                }
                                           }
                                      } else if is_text_file(&file_info.path) {
                                           match self.decoded_text(hash) {
                                                Some(text) => {
                                                    let shader = is_shader_source(&file_info.path);
                                                    let mut raw = shader || self.raw_text_view.contains(&hash);
                                                    if !shader {
                                                        ui.horizontal(|ui| {
                                                            if ui.toggle_value(&mut raw, "Raw").on_hover_text("Plain editor view with text selection").changed() {
                                                                if raw { self.raw_text_view.insert(hash); } else { self.raw_text_view.remove(&hash); }
                                                            }
                                                        });
                                                    }
                                                    if raw {
                                                        let language = if shader { "hlsl" } else { "text" };
                                                        let theme = if ui.visuals().dark_mode {
                                                            crate::ui::syntax::Theme::dark()
                                                        } else {
                                                            crate::ui::syntax::Theme::light()
                                                        };
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
                                                        let mut filter = self.text_filters.remove(&hash).unwrap_or_default();
                                                        let opened = crate::ui::linked_text_viewer::LinkedTextViewer::show(ui, hash, &text, &mut filter);
                                                        self.text_filters.insert(hash, filter);
                                                        if let Some(p) = opened {
                                                            self.open_path(index, &p);
                                                        }
                                                    }
                                                }
                                                None => {
                                                    ui.label("Loading text...");
                                                }
                                           }
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
                .color(if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(236, 236, 240)
                } else {
                    egui::Color32::from_rgb(24, 24, 28)
                }),
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
                .color(if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(113, 113, 122)
                } else {
                    egui::Color32::from_rgb(80, 80, 90)
                }),
        );
        ui.separator();

        if subfolders.is_empty() && hashes.is_empty() {
            ui.add_space(16.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} has no direct file entries.", name))
                        .color(if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(126, 126, 134)
                        } else {
                            egui::Color32::from_rgb(80, 80, 90)
                        }),
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

    fn show_search_results(&mut self, ui: &mut egui::Ui, bundle_index: &Option<std::sync::Arc<crate::bundles::index::Index>>, term: String, hashes: Vec<u64>) {
        ui.label(
            egui::RichText::new(format!("Search: \"{}\"", term))
                .heading()
                .color(if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(236, 236, 240)
                } else {
                    egui::Color32::from_rgb(24, 24, 28)
                }),
        );
        ui.add_space(4.0);

        // A brand-new search starts with no filter; returning to the same
        // search (e.g. via "Back to search results") keeps whatever the
        // user had selected.
        if self.search_filter_term != term {
            self.search_filter_term = term.clone();
            self.search_filter_exts.clear();
        }

        let mut files = Vec::new();
        if let Some(index) = bundle_index {
            for hash in &hashes {
                if let Some(file) = index.files.get(hash) {
                    files.push((*hash, file));
                }
            }
        }
        files.sort_by(|a, b| a.1.path.cmp(&b.1.path));

        fn extension_of(path: &str) -> String {
            path.rsplit('.').next().unwrap_or("").to_lowercase()
        }

        let mut ext_counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, file) in &files {
            *ext_counts.entry(extension_of(&file.path)).or_insert(0) += 1;
        }
        // Drop filter selections for extensions no longer present (e.g. after
        // returning to a search whose result set can't change, this is a no-op,
        // but keeps things correct if this is ever fed a live-updating list).
        self.search_filter_exts.retain(|e| ext_counts.contains_key(e));

        if ext_counts.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                let mut sorted_exts: Vec<(&String, &usize)> = ext_counts.iter().collect();
                sorted_exts.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

                let all_selected = self.search_filter_exts.is_empty();
                if ui.selectable_label(all_selected, format!("All ({})", files.len())).clicked() {
                    self.search_filter_exts.clear();
                }
                for (ext, count) in sorted_exts {
                    let selected = self.search_filter_exts.contains(ext);
                    let label = if ext.is_empty() { format!("(none) ({})", count) } else { format!(".{} ({})", ext, count) };
                    if ui.selectable_label(selected, label).clicked() {
                        if selected {
                            self.search_filter_exts.remove(ext);
                        } else {
                            self.search_filter_exts.insert(ext.clone());
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }

        let filtered: Vec<(u64, &crate::bundles::index::FileInfo)> = if self.search_filter_exts.is_empty() {
            files
        } else {
            files.into_iter().filter(|(_, f)| self.search_filter_exts.contains(&extension_of(&f.path))).collect()
        };

        ui.label(
            egui::RichText::new(if filtered.len() == ext_counts.values().sum::<usize>() {
                format!("MATCHES · {}", filtered.len())
            } else {
                format!("MATCHES · {} of {}", filtered.len(), ext_counts.values().sum::<usize>())
            })
                .monospace()
                .size(10.5)
                .color(if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(113, 113, 122)
                } else {
                    egui::Color32::from_rgb(80, 80, 90)
                }),
        );
        ui.separator();

        if filtered.is_empty() {
            ui.add_space(16.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(format!("No files match \"{}\".", term))
                        .color(if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(126, 126, 134)
                        } else {
                            egui::Color32::from_rgb(80, 80, 90)
                        }),
                );
            });
            return;
        }

        let files = filtered;

        TableBuilder::new(ui)
            .striped(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(240.0))
            .column(Column::exact(84.0))
            .column(Column::exact(88.0))
            .column(Column::exact(132.0))
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.label(egui::RichText::new("PATH").monospace().size(10.5).color(egui::Color32::from_rgb(113, 113, 122)));
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
                body.rows(22.0, files.len(), |mut row| {
                    let row_index = row.index();
                    let (hash, file_info) = files[row_index];

                    row.col(|ui| {
                        let response = ui.selectable_label(
                            false,
                            egui::RichText::new(&file_info.path).monospace().size(11.5),
                        );
                        if response.clicked() {
                            self.back_target = Some((hash, crate::ui::app::FileSelection::SearchResults {
                                term: term.clone(),
                                hashes: hashes.clone(),
                            }));
                            self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(hash));
                        }
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
                            egui::RichText::new(format!("{:016x}", hash))
                                .monospace()
                                .size(10.5)
                                .color(egui::Color32::from_rgb(161, 161, 170)),
                        );
                    });
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

    fn show_audio_player(&mut self, ui: &mut egui::Ui, reader: Option<&GgpkReader>, index: &std::sync::Arc<crate::bundles::index::Index>, file_info: &crate::bundles::index::FileInfo, hash: u64) {
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

    fn show_media_stub(&mut self, ui: &mut egui::Ui, file_info: &crate::bundles::index::FileInfo, hash: u64, reader: Option<&GgpkReader>, index: Option<&crate::bundles::index::Index>) {
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
            "bk2" => ("BINK 2 VIDEO", "Bink 2 encoded video. Played via RAD Video Tools (binkplay.exe), ffplay, or your system default."),
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

        ui.horizontal(|ui| {
            if ui.button("Export File").clicked() {
                self.export_requested = Some((vec![hash], file_info.path.clone(), None));
            }

            if ext == "bk2" && ui.button("▶  Play").clicked() {
                let game_root = self.steam_loader.as_ref().map(|s| s.game_root());
                // 1. Try loose file on disk (Steam install)
                let loose = self.steam_loader.as_ref()
                    .and_then(|s| s.loose_file_path(&file_info.path));
                if let Some(loose_path) = loose {
                    if let Err(e) = launch_bink_player(&loose_path, game_root.as_deref()) {
                        self.last_error = Some(format!("Play failed: {}", e));
                    }
                } else {
                    // 2. Extract from bundle to temp then play
                    let file_name = std::path::Path::new(&file_info.path)
                        .file_name()
                        .map(|n| n.to_os_string())
                        .unwrap_or_else(|| std::ffi::OsString::from("video.bk2"));
                    let temp_path = std::env::temp_dir().join(file_name);

                    // Load bytes synchronously from cache or bundle
                    let bytes = self.raw_data_cache.get(&hash).cloned()
                        .or_else(|| extract_bundle_file_sync(file_info, index?, reader, self.steam_loader.as_ref()));

                    match bytes {
                        Some(data) => {
                            match std::fs::write(&temp_path, &data) {
                                Ok(_) => {
                                    if let Err(e) = launch_bink_player(&temp_path, game_root.as_deref()) {
                                        self.last_error = Some(format!("Play failed: {}", e));
                                    }
                                }
                                Err(e) => self.last_error = Some(format!("Temp write failed: {}", e)),
                            }
                        }
                        None => self.last_error = Some("Could not read file data from bundle".to_string()),
                    }
                }
            }
        });

        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(description)
                .size(11.5)
                .color(egui::Color32::from_rgb(113, 113, 122))
                .italics(),
        );
    }

    /// Starts a background decode of one bank stream (Vorbis transcode is too
    /// slow for the UI thread). Result arrives via `bank_decode_rx`.
    fn start_bank_stream_decode(&mut self, hash: u64, index: usize, intent: BankStreamIntent) {
        if self.bank_decoding.is_some() {
            return; // one decode at a time
        }
        let raw = match self.raw_data_cache.get(&hash) {
            Some(r) => r.clone(),
            None => {
                self.last_error = Some("Bank data no longer cached — re-select the file".to_string());
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.bank_decode_rx = Some(rx);
        self.bank_decoding = Some((hash, index));
        self.bank_decode_intent = intent;
        std::thread::spawn(move || {
            let result = crate::parsers::fmod_bank::decode_stream(&raw, index);
            let _ = tx.send((hash, index, result));
        });
    }

    fn play_audio_bytes(&mut self, bytes: Vec<u8>) {
        if self.audio_device.is_none() {
            if let Ok(device) = rodio::DeviceSinkBuilder::open_default_sink() {
                self.audio_device = Some(device);
            }
        }
        if let Some(device) = &self.audio_device {
            match rodio::Decoder::new(std::io::Cursor::new(bytes)) {
                Ok(decoder) => {
                    let sink = rodio::Player::connect_new(device.mixer());
                    sink.set_volume(self.audio_volume);
                    sink.append(decoder);
                    sink.play();
                    self.audio_sink = Some(sink);
                }
                Err(e) => self.last_error = Some(format!("Failed to decode audio stream: {}", e)),
            }
        } else {
            self.last_error = Some("No audio output device available".to_string());
        }
    }

    fn save_bank_stream(&mut self, hash: u64, index: usize, bytes: &[u8]) {
        let (name, ext) = self
            .bank_info_cache
            .get(&hash)
            .and_then(|info| info.streams.get(index).map(|s| (s.name.clone(), info.extension)))
            .unwrap_or_else(|| (format!("stream_{:03}", index), "ogg"));
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}.{}", sanitize_filename(&name), ext))
            .save_file()
        {
            if let Err(e) = std::fs::write(&path, bytes) {
                self.last_error = Some(format!("Save failed: {}", e));
            }
        }
    }

    fn show_bank_viewer(&mut self, ui: &mut egui::Ui, _file_info: &crate::bundles::index::FileInfo, hash: u64) {
        // Poll the in-flight stream decode
        if let Some(rx) = &self.bank_decode_rx {
            if let Ok((h, idx, result)) = rx.try_recv() {
                self.bank_decode_rx = None;
                self.bank_decoding = None;
                match result {
                    Ok(bytes) => {
                        // Decoded WAVs are large (a music track is ~50MB) —
                        // only keep streams of the bank currently in view.
                        self.bank_stream_cache.retain(|(h2, _), _| *h2 == h);
                        self.bank_stream_cache.insert((h, idx), bytes.clone());
                        match self.bank_decode_intent {
                            BankStreamIntent::Play => {
                                self.play_audio_bytes(bytes);
                                self.bank_playing = Some((h, idx));
                            }
                            BankStreamIntent::Export => self.save_bank_stream(h, idx, &bytes),
                        }
                    }
                    Err(e) => self.last_error = Some(e),
                }
            }
        }

        // Poll the export-all job
        if let Some(rx) = &self.bank_export_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => self.bank_export_status = Some(msg),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.bank_export_rx = None;
                        break;
                    }
                }
            }
        }

        let info = match self.bank_info_cache.get(&hash) {
            Some(i) => i.clone(),
            None => return,
        };

        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal_wrapped(|ui| {
            crate::ui::components::badge(ui, "FMOD BANK");
            crate::ui::components::badge(ui, &info.format.to_uppercase());
            crate::ui::components::badge(ui, &format!("{} STREAMS", info.streams.len()));
        });

        if info.streams.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "This bank contains no audio streams — it holds FMOD event/mixer metadata \
                     (or GUID strings for a .strings.bank).",
                )
                .size(11.5)
                .color(egui::Color32::from_rgb(113, 113, 122))
                .italics(),
            );
            return;
        }

        let is_playing = self.audio_sink.as_ref().map(|s| !s.empty()).unwrap_or(false);
        if !is_playing {
            self.bank_playing = None;
        }

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("VOLUME")
                    .size(10.5)
                    .monospace()
                    .color(egui::Color32::from_rgb(113, 113, 122)),
            );
            ui.add_space(6.0);
            if ui
                .add_sized(
                    [140.0, 18.0],
                    egui::Slider::new(&mut self.audio_volume, 0.0..=1.0).show_value(false),
                )
                .changed()
            {
                if let Some(sink) = &self.audio_sink {
                    sink.set_volume(self.audio_volume);
                }
            }
            ui.add_space(12.0);
            if self.bank_export_rx.is_none() {
                if ui.button("Export All Streams...").clicked() {
                    self.export_all_bank_streams(hash, &info);
                }
            }
            if let Some(status) = &self.bank_export_status {
                if self.bank_export_rx.is_some() {
                    ui.spinner();
                }
                ui.label(
                    egui::RichText::new(status)
                        .size(11.0)
                        .color(egui::Color32::from_rgb(161, 161, 170)),
                );
            }
        });

        if let Some(err) = &self.last_error {
            ui.label(
                egui::RichText::new(err)
                    .size(11.5)
                    .color(egui::Color32::from_rgb(239, 68, 68)),
            );
        }

        ui.add_space(4.0);
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, stream) in info.streams.iter().enumerate() {
                let row_playing = is_playing && self.bank_playing == Some((hash, i));
                let row_decoding = self.bank_decoding == Some((hash, i));

                ui.horizontal(|ui| {
                    if row_decoding {
                        ui.add_sized([26.0, 18.0], egui::Spinner::new());
                    } else if row_playing {
                        if ui.add_sized([26.0, 18.0], egui::Button::new("■")).clicked() {
                            if let Some(sink) = &self.audio_sink {
                                sink.stop();
                            }
                            self.audio_sink = None;
                            self.bank_playing = None;
                        }
                    } else if ui.add_sized([26.0, 18.0], egui::Button::new("▶")).clicked() {
                        if let Some(sink) = &self.audio_sink {
                            sink.stop();
                        }
                        self.audio_sink = None;
                        if let Some(bytes) = self.bank_stream_cache.get(&(hash, i)).cloned() {
                            self.play_audio_bytes(bytes);
                            self.bank_playing = Some((hash, i));
                        } else {
                            self.start_bank_stream_decode(hash, i, BankStreamIntent::Play);
                        }
                    }

                    if ui
                        .add_sized([26.0, 18.0], egui::Button::new("💾"))
                        .on_hover_text(format!("Save as .{}", info.extension))
                        .clicked()
                    {
                        if let Some(bytes) = self.bank_stream_cache.get(&(hash, i)).cloned() {
                            self.save_bank_stream(hash, i, &bytes);
                        } else {
                            self.start_bank_stream_decode(hash, i, BankStreamIntent::Export);
                        }
                    }

                    ui.label(egui::RichText::new(&stream.name).monospace().size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let secs = stream.duration_secs();
                        ui.label(
                            egui::RichText::new(format!(
                                "{}:{:05.2}  {}ch  {:.1}kHz  {}",
                                (secs / 60.0) as u32,
                                secs % 60.0,
                                stream.channels,
                                stream.sample_rate as f32 / 1000.0,
                                format_file_size(stream.size as u64),
                            ))
                            .size(10.5)
                            .monospace()
                            .color(egui::Color32::from_rgb(113, 113, 122)),
                        );
                    });
                });
            }
        });

        if is_playing || self.bank_decoding.is_some() || self.bank_export_rx.is_some() {
            ui.ctx().request_repaint();
        }
    }

    fn export_all_bank_streams(&mut self, hash: u64, info: &crate::parsers::fmod_bank::FmodBankInfo) {
        let raw = match self.raw_data_cache.get(&hash) {
            Some(r) => r.clone(),
            None => {
                self.last_error = Some("Bank data no longer cached — re-select the file".to_string());
                return;
            }
        };
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let info = info.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.bank_export_rx = Some(rx);
        self.bank_export_status = Some("Exporting...".to_string());
        std::thread::spawn(move || {
            let total = info.streams.len();
            let mut ok = 0;
            let mut failed = 0;
            for (i, stream) in info.streams.iter().enumerate() {
                let _ = tx.send(format!("Exporting {}/{}: {}", i + 1, total, stream.name));
                match crate::parsers::fmod_bank::decode_stream(&raw, i) {
                    Ok(bytes) => {
                        let path = dir.join(format!(
                            "{}.{}",
                            sanitize_filename(&stream.name),
                            info.extension
                        ));
                        if std::fs::write(&path, bytes).is_ok() {
                            ok += 1;
                        } else {
                            failed += 1;
                        }
                    }
                    Err(_) => failed += 1,
                }
            }
            let _ = tx.send(if failed == 0 {
                format!("Exported {} streams.", ok)
            } else {
                format!("Exported {} streams ({} failed).", ok, failed)
            });
        });
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
                        if self.texture_cache.contains_key(&offset) {
                             self.touch_texture(offset);
                             if let Some(texture) = self.texture_cache.get(&offset) {
                                 ui.image(texture);
                             }
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
                                              self.insert_texture(offset, texture);
                                          },
                                          Err(e) => { ui.label(format!("Failed to load DDS: {}", e)); }
                                      }
                                  },
                                  Err(e) => { ui.label(format!("Read error: {}", e)); }
                             }
                        }
                    } else if file.name.ends_with(".dat") || file.name.ends_with(".dat64") {
                         if self.dat_viewer.loaded_filename() != Some(file.name.as_str()) {
                             self.dat_viewer.load(reader, offset);
                         }
                         self.dat_viewer.show(ui, is_poe2, None);
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
    //
    // Stored under the app's managed `cache/` dir (not the system temp dir)
    // so it's included in Settings' cache size display and "Clear Cache",
    // and gets wiped along with everything else on a patch-version change.
    fn get_cache_path(hash: u64) -> std::path::PathBuf {
        let mut path = crate::settings::AppSettings::get_app_data_dir().join("cache").join("parsed");
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

    /// Runs the background row-count scan the DAT viewer uses to suggest foreign-key
    /// targets — once per index, and only after the viewer asks for it.
    fn service_table_stats(&mut self, ctx: &egui::Context, reader: Option<std::sync::Arc<GgpkReader>>, index: &std::sync::Arc<crate::bundles::index::Index>) {
        let key = std::sync::Arc::as_ptr(index) as usize;
        if self.table_stats_for != key {
            self.table_stats_for = key;
            self.table_stats_rx = None;
            self.dat_viewer.table_stats = None;
            self.dat_viewer.table_stats_loading = false;
        }
        if let Some(rx) = &self.table_stats_rx {
            match rx.try_recv() {
                Ok(stats) => {
                    self.dat_viewer.set_table_stats(stats);
                    self.table_stats_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.table_stats_rx = None;
                    self.dat_viewer.table_stats_loading = false;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.dat_viewer.request_table_stats {
            return;
        }
        self.dat_viewer.request_table_stats = false;
        if self.table_stats_rx.is_some() || self.dat_viewer.table_stats.is_some() {
            return;
        }
        self.dat_viewer.table_stats_loading = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.table_stats_rx = Some(rx);
        let index = index.clone();
        let steam = self.steam_loader.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let stats = scan_table_stats(&index, reader.as_deref(), steam.as_ref());
            let _ = tx.send(stats);
            ctx.request_repaint();
        });
    }

    /// Decoded text for a loaded text file, cached per hash.
    fn decoded_text(&mut self, hash: u64) -> Option<std::sync::Arc<String>> {
        if let Some(t) = self.text_cache.get(&hash) {
            return Some(t.clone());
        }
        let data = self.raw_data_cache.get(&hash)?;
        let text = std::sync::Arc::new(decode_text_with_detection(data));
        if self.text_cache.len() > 32 {
            self.text_cache.clear();
        }
        self.text_cache.insert(hash, text.clone());
        Some(text)
    }

    fn show_linked_text(&mut self, ui: &mut egui::Ui, hash: u64, text: &str) -> Option<String> {
        let mut filter = self.text_filters.remove(&hash).unwrap_or_default();
        let opened = crate::ui::linked_text_viewer::LinkedTextViewer::show(ui, hash, text, &mut filter);
        self.text_filters.insert(hash, filter);
        opened
    }

    fn show_curves(&mut self, ui: &mut egui::Ui, hash: u64, text: &str) -> Option<String> {
        let file = self.curve_cache.entry(hash).or_insert_with(|| std::sync::Arc::new(crate::parsers::curves::parse(text))).clone();
        let state = self.curve_viewer_state.entry(hash).or_default();
        crate::ui::curve_viewer::CurveViewer::show(ui, hash, &file, state)
    }

    fn structured_json(&mut self, hash: u64, text: &str) -> Option<(String, std::sync::Arc<serde_json::Value>)> {
        self.structured_json.entry(hash).or_insert_with(|| json_body(text).map(|(h, v)| (h, std::sync::Arc::new(v)))).clone()
    }

    /// Starts decoding texture previews in the background (one batch at a time).
    fn ensure_thumbnails(&mut self, reader: Option<std::sync::Arc<GgpkReader>>, index: &std::sync::Arc<crate::bundles::index::Index>, paths: Vec<String>) {
        if self.thumb_rx.is_some() {
            return;
        }
        let missing: Vec<String> = paths.into_iter().filter(|p| !self.thumb_cache.contains_key(p) && !self.thumb_pending.contains(p)).collect();
        if missing.is_empty() {
            return;
        }
        for p in &missing {
            self.thumb_pending.insert(p.clone());
        }
        let index = index.clone();
        let steam = self.steam_loader.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.thumb_rx = Some(rx);
        std::thread::spawn(move || {
            let batch = fetch_thumbnails(reader.as_deref(), &index, steam.as_ref(), missing, 192);
            let _ = tx.send(batch);
        });
    }

    fn poll_thumbnails(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.thumb_rx else { return };
        match rx.try_recv() {
            Ok(batch) => {
                self.thumb_rx = None;
                for (path, img) in batch {
                    self.thumb_pending.remove(&path);
                    let handle = img.map(|i| ctx.load_texture(format!("thumb:{}", path), i, egui::TextureOptions::LINEAR));
                    self.thumb_cache.insert(path.clone(), handle);
                    self.thumb_order.push(path);
                }
                while self.thumb_order.len() > 96 {
                    let old = self.thumb_order.remove(0);
                    self.thumb_cache.remove(&old);
                }
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.thumb_rx = None;
                self.thumb_pending.clear();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Opens a game file named inside another file (a link click in any viewer).
    /// `.dds` names get the same fallbacks the texture loader uses.
    pub(crate) fn open_path(&mut self, index: &crate::bundles::index::Index, path: &str) {
        let path = crate::ui::links::normalize(path);
        let mut candidates = vec![path.clone()];
        if path.to_ascii_lowercase().ends_with(".dds") {
            candidates.extend(dds_path_candidates(&path));
        }
        for c in &candidates {
            if let Some(fi) = find_file_info_by_path(index, c) {
                self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(fi.path_hash));
                self.last_error = None;
                return;
            }
        }
        self.last_error = Some(format!("Not found in index: {}", path));
    }

    /// Turns a foreign-key / file-path click in the DAT viewer into a file selection.
    fn handle_dat_nav(&mut self, index: &crate::bundles::index::Index) {
        let Some(req) = self.dat_viewer.nav_request.take() else { return };
        let (path, row) = match req {
            crate::ui::dat_viewer::DatNavRequest::Table { path, row } => (path, row),
            crate::ui::dat_viewer::DatNavRequest::File(p) => (p, None),
        };
        match find_file_info_by_path(index, &path) {
            Some(fi) => {
                self.dat_viewer.pending_scroll_row = row;
                self.selection_requested = Some(crate::ui::app::FileSelection::BundleFile(fi.path_hash));
            }
            None => self.last_error = Some(format!("Not found in index: {}", path)),
        }
    }

    pub fn load_bundled_content(&mut self, ctx: &egui::Context, reader: Option<&GgpkReader>, index: &std::sync::Arc<crate::bundles::index::Index>, file_info: &crate::bundles::index::FileInfo, hash: u64) {
         // Reset previous state
         self.dat_viewer.reader = None;
         self.dat_viewer.error_msg = None;
         self.last_error = None;

         // Check persistent cache for JSON/PSG
         if is_json_path(&file_info.path) || file_info.path.ends_with(".psg") || file_info.path.ends_with(".fxgraph") {
             if self.try_load_from_cache(hash) {
                 println!("Loaded {} from disk cache.", file_info.path);
                 return;
             }
         }

         // Loose file (Steam Art/ directory) — read directly from disk
         if file_info.bundle_index == crate::bundles::steam::LOOSE_FILE_SENTINEL {
             // Resolve the path first so the immutable steam_loader borrow is
             // released before we mutate the caches.
             let loose_path = self.steam_loader.as_ref()
                 .and_then(|steam| steam.loose_file_path(&file_info.path));
             if let Some(loose_path) = loose_path {
                 match std::fs::read(&loose_path) {
                     Ok(data) => {
                         self.failed_loads.remove(&hash);
                         self.last_error = None;
                         self.route_file_data(ctx, &file_info.path, hash, data);
                     }
                     Err(e) => {
                         self.last_error = Some(format!("Failed to read loose file: {}", e));
                         self.failed_loads.insert(hash);
                     }
                 }
             } else if self.steam_loader.is_some() {
                 self.last_error = Some(format!("Loose file not found on disk: {}", file_info.path));
                 self.failed_loads.insert(hash);
             }
             return;
         }

         // Loose GGPK record (FMOD/*.bank, Media/*.bk2, ...) — read directly
         // from the GGPK file records instead of a bundle.
         if file_info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL {
             let data = reader.and_then(|r| {
                 r.read_file_by_path(&file_info.path).ok().flatten().and_then(|rec| {
                     r.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec())
                 })
             });
             match data {
                 Some(data) => {
                     self.failed_loads.remove(&hash);
                     self.last_error = None;
                     self.route_file_data(ctx, &file_info.path, hash, data);
                 }
                 None => {
                     self.last_error = Some(format!("Failed to read loose GGPK file: {}", file_info.path));
                     self.failed_loads.insert(hash);
                 }
             }
             return;
         }

         if let Some(bundle_info) = index.bundles.get(file_info.bundle_index as usize) {
             let mut raw_bundle_data: Option<Vec<u8>> = None;

             // 1. Try Local GGPK
             if let Some(reader) = reader {
                 let candidates = vec![
                     format!("Bundles2/{}", bundle_info.name),
                     format!("Bundles2/{}.bundle.bin", bundle_info.name),
                     bundle_info.name.clone(),
                     format!("{}.bundle.bin", bundle_info.name),
                 ];
                 for cand in &candidates {
                     if let Ok(Some(rec)) = reader.read_file_by_path(cand) {
                         println!("Bundle found in GGPK: {}", cand);
                         if let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) {
                             raw_bundle_data = Some(data.to_vec());
                             break;
                         }
                     }
                 }
             }

             // 1.5. Try Steam directory
             if raw_bundle_data.is_none() {
                 if let Some(steam) = &self.steam_loader {
                     if let Ok(data) = steam.fetch_bundle(&bundle_info.name) {
                         println!("Bundle found in Steam dir: {}", bundle_info.name);
                         raw_bundle_data = Some(data);
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

             let mut decompressed_bundle_data: Option<Vec<u8>> = None;

             if let Some(data) = raw_bundle_data {
                 let mut cursor = std::io::Cursor::new(data);
                 if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                     if let Ok(decompressed) = bundle.decompress(&mut cursor) {
                         decompressed_bundle_data = Some(decompressed);
                     }
                 }
             }

             if decompressed_bundle_data.is_none() {
                 if let Some(reader) = reader {
                     println!("Bundle not found or decompression failed. Attempting direct GGPK file lookup for: {}", file_info.path);
                     if let Ok(Some(rec)) = reader.read_file_by_path(&file_info.path) {
                         if let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) {
                             let start = file_info.file_offset as usize;
                             let end = start + data.len();
                             let mut fake_decompressed = vec![0u8; end];
                             fake_decompressed[start..end].copy_from_slice(data);
                             decompressed_bundle_data = Some(fake_decompressed);
                             println!("Direct GGPK fallback succeeded for: {}", file_info.path);
                         }
                     }
                 }
             }

             if let Some(decompressed_data) = decompressed_bundle_data {
                 self.failed_loads.remove(&hash);
                 let start = file_info.file_offset as usize;
                 let end = start + file_info.file_size as usize;
                 
                 if end <= decompressed_data.len() {
                     let file_data = decompressed_data[start..end].to_vec();
                     self.route_file_data(ctx, &file_info.path, hash, file_data);
                 } else {
                     let msg = format!("Decompressed bounds check failed for '{}'", file_info.path);
                     println!("{}", msg);
                     self.last_error = Some(msg);
                     self.failed_loads.insert(hash);
                 }
             } else {
                 let msg = format!("Failed to load or decompress data for '{}' (bundle not found and GGPK lookup failed)", file_info.path);
                 println!("{}", msg);
                 self.last_error = Some(msg);
                 self.failed_loads.insert(hash);
             }
          }
    }

    /// Kicks off a background build of the atlas skill node database (name +
    /// stat text per `PassiveSkillGraphId`), the first time an atlas `.psg`
    /// is opened. No-op if already loaded/loading or the DAT schema isn't
    /// ready yet (retried next frame in that case).
    fn ensure_skill_graph_db_loading(&mut self, reader: Option<std::sync::Arc<GgpkReader>>, index: &std::sync::Arc<crate::bundles::index::Index>) {
        if self.skill_graph_db.is_some() || self.skill_graph_db_loading {
            return;
        }
        let schema = match self.dat_viewer.schema.clone() {
            Some(s) => s,
            None => return,
        };

        self.skill_graph_db_loading = true;
        let index = index.clone();
        let steam_loader = self.steam_loader.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.skill_graph_db_rx = Some(rx);
        std::thread::spawn(move || {
            let result = build_skill_graph_db(reader.as_deref(), &index, steam_loader.as_ref(), &schema);
            let _ = tx.send(result);
        });
    }

    /// Asks for a destination folder and runs the official-format skill tree
    /// export for `psg` on a background thread.
    fn start_tree_export(
        &mut self,
        hash: u64,
        psg_path: String,
        psg: crate::dat::psg::PsgFile,
        reader: Option<std::sync::Arc<GgpkReader>>,
        index: &std::sync::Arc<crate::bundles::index::Index>,
    ) {
        if self.tree_export_rx.is_some() {
            return;
        }
        let Some(schema) = self.dat_viewer.schema.clone() else {
            if let Some(state) = self.psg_viewer_state.get_mut(&hash) {
                state.export_status = Some("Schema not loaded yet".to_string());
            }
            return;
        };
        let Some(out_dir) = rfd::FileDialog::new().set_title("Export skill tree to folder").pick_folder() else { return };
        let source = crate::skill_tree_export::TreeExportSource { reader, index: index.clone(), steam: self.steam_loader.clone(), schema };
        let db = self.skill_graph_db.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.tree_export_rx = Some((hash, rx));
        if let Some(state) = self.psg_viewer_state.get_mut(&hash) {
            state.export_status = Some("Exporting…".to_string());
        }
        std::thread::spawn(move || {
            crate::skill_tree_export::run_tree_export(source, psg_path, psg, db, crate::skill_tree_export::TreeExportOptions::default(), out_dir, tx);
        });
    }

    fn poll_tree_export(&mut self) {
        let Some((hash, rx)) = &self.tree_export_rx else { return };
        let hash = *hash;
        let mut finished = false;
        let mut status = None;
        while let Ok(msg) = rx.try_recv() {
            status = Some(match msg {
                crate::export::ExportStatus::Progress { current, total, filename } => format!("Exporting [{}/{}] {}", current, total, filename),
                crate::export::ExportStatus::Complete { message, .. } => {
                    finished = true;
                    message
                }
                crate::export::ExportStatus::Error(e) => {
                    finished = true;
                    format!("Export failed: {}", e)
                }
            });
        }
        if let (Some(status), Some(state)) = (status, self.psg_viewer_state.get_mut(&hash)) {
            state.export_status = Some(status);
        }
        if finished {
            self.tree_export_rx = None;
        }
    }

    /// True while the shared skill graph database and/or any of its art
    /// textures are still being fetched/decoded in the background.
    pub fn is_psg_art_loading(&self) -> bool {
        self.skill_graph_db_loading || self.psg_texture_rx.is_some() || !self.psg_texture_pending.is_empty()
    }

    /// Requests background fetch+decode of any of `paths` not already cached
    /// or in flight. Only one batch is ever in flight at a time — a caller
    /// asking again next frame with a still-missing path just gets it picked
    /// up once the current batch completes (textures are cached forever
    /// once loaded, so this converges within a couple of frames).
    pub fn ensure_psg_textures_loading(
        &mut self,
        reader: Option<std::sync::Arc<GgpkReader>>,
        index: &std::sync::Arc<crate::bundles::index::Index>,
        paths: Vec<String>,
    ) {
        if self.psg_texture_rx.is_some() {
            return;
        }
        let missing: Vec<String> = paths
            .into_iter()
            .filter(|p| {
                !p.is_empty()
                    && !self.psg_texture_cache.contains_key(p)
                    && !self.psg_texture_pending.contains(p)
                    && !self.psg_texture_failed.contains(p)
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        for p in &missing {
            self.psg_texture_pending.insert(p.clone());
        }

        let index = index.clone();
        let steam_loader = self.steam_loader.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.psg_texture_rx = Some(rx);
        std::thread::spawn(move || {
            let batch = fetch_and_decode_dds_batch(reader.as_deref(), &index, steam_loader.as_ref(), missing);
            let _ = tx.send(batch);
        });
    }

    /// Routes raw file bytes into the appropriate viewer state based on the
    /// file extension. Shared by bundled, Steam-loose, and GGPK-loose loads.
    fn route_file_data(&mut self, ctx: &egui::Context, path: &str, hash: u64, file_data: Vec<u8>) {
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
                      } else if is_image_path(path) {
                          // Try to load Image
                          self.last_error = None;

                          println!("Image Loading: Data Length {}", file_data.len());

                          // Special handling for DDS
                          if path.ends_with(".dds") || path.ends_with(".dds.header") {
                              let dds_bytes = dds_payload(&file_data);
                              if dds_bytes.len() > 16 {
                                  let magic = &dds_bytes[0..4];
                                  if magic != b"DDS " {
                                      println!("WARNING: Magic bytes mismatch! Expected 'DDS ', found {:?}", magic);
                                  }
                              }

                              // Method 1: Try image_dds first (better support for various DXT/BC formats for DDS)
                              let mut loaded = false;
                              let mut cursor = std::io::Cursor::new(dds_bytes);
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
                                              self.insert_texture(hash, texture);
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
                              self.insert_texture(hash, texture);
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
                          if self.audio_device.is_none() {
                              if let Ok(device) = rodio::DeviceSinkBuilder::open_default_sink() {
                                  self.audio_device = Some(device);
                              } else {
                                  println!("Failed to get default audio output device");
                              }
                          }
                          
                          if let Some(device) = &self.audio_device {
                              use std::io::Cursor;
                              let cursor = Cursor::new(file_data);
                              
                              if let Ok(decoder) = rodio::Decoder::new(cursor) {
                                   // Recreate sink for each playback to avoid state issues
                                   let sink = rodio::Player::connect_new(device.mixer());
                                   sink.set_volume(self.audio_volume);
                                   sink.append(decoder);
                                   sink.play();
                                   self.audio_sink = Some(sink);
                              } else {
                                  self.last_error = Some("Failed to decode Audio (Might be Wwise WEM)".to_string());
                              }
                          }
                      } else if path.ends_with(".csd") {
                          match csd::parse_csd(&file_data, path) {
                              Ok(csd_file) => {
                                  self.csd_cache.insert(hash, csd_file);
                                  self.last_error = None;
                              },
                              Err(e) => {
                                  self.last_error = Some(format!("CSD Parse Error: {}", e));
                                  self.failed_loads.insert(hash);
                              }
                          }
                      } else if crate::parsers::model::is_model_path(path) {
                          match crate::parsers::model::parse_model(path, &file_data) {
                              Ok(model) => {
                                  let summary = model.summary();
                                  self.model_cache.insert(hash, (std::sync::Arc::new(model), summary));
                                  self.last_error = None;
                              }
                              Err(e) => {
                                  self.last_error = Some(format!("Model parse error: {}", e));
                              }
                          }
                          self.insert_raw(hash, file_data);
                      } else if is_json_path(path) {
                           // Read file content as string
                           let text = decode_text_with_detection(&file_data);
                           match serde_json::from_str::<serde_json::Value>(&text) {
                               Ok(val) => {
                                    self.json_cache.insert(hash, val);
                                    self.last_error = None;
                               },
                               Err(e) => {
                                    self.last_error = Some(format!("JSON Parse Error: {}", e));
                                    self.failed_loads.insert(hash);
                               }
                           }
                      } else if path.ends_with(".psg") {
                          match psg::parse_psg(&file_data) {
                              Ok(psg_file) => {
                                  self.psg_cache.insert(hash, psg_file.clone());
                                  
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
                                  self.insert_raw(hash, file_data.clone());
                                  self.failed_loads.insert(hash);
                              }
                          }
                      } else if path.ends_with(".fxgraph") {
                          match crate::parsers::fxgraph::parse_fxgraph(&file_data) {
                              Ok(graph) => {
                                  self.fxgraph_cache.insert(hash, graph.clone());

                                  // Convert to Value for the JSON fallback/toggle view
                                  if let Ok(v) = serde_json::to_value(&graph) {
                                      Self::save_to_cache(hash, &v);
                                      self.json_cache.insert(hash, v);
                                      self.last_error = None;
                                  } else {
                                       self.last_error = Some("Failed to serialize FX graph to JSON".to_string());
                                  }
                              },
                              Err(e) => {
                                  self.last_error = Some(format!("FX Graph Parse Error: {}", e));
                                  self.insert_raw(hash, file_data.clone());
                                  self.failed_loads.insert(hash);
                              }
                          }
                      } else if is_text_file(path) {
                          // Just store raw data, we decode on render
                          self.insert_raw(hash, file_data);
                          self.last_error = None;
                      } else if path.ends_with(".bank") {
                          match crate::parsers::fmod_bank::parse_bank_info(&file_data) {
                              Ok(info) => {
                                  self.bank_info_cache.insert(hash, info);
                                  self.last_error = None;
                              }
                              Err(e) => {
                                  self.last_error = Some(format!("Bank parse failed: {}", e));
                              }
                          }
                          // Keep raw bytes for on-demand stream decode (and to stop re-loading)
                          self.insert_raw(hash, file_data);
                      } else {
                          // Fallback for unknown files - cache raw data to stop re-loading
                          self.insert_raw(hash, file_data);
                          self.last_error = None;
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
    p.ends_with(".chr") || p.ends_with(".tdf") || p.ends_with(".ot") || p.ends_with(".ais") ||
    // Plain UTF-16LE text formats that were previously falling through to the
    // hex viewer despite being fully readable (confirmed against real game data).
    p.ends_with(".tdt") || p.ends_with(".tmd") || p.ends_with(".epk") ||
    p.ends_with(".it") || p.ends_with(".fgp") || p.ends_with(".tgr") ||
    p.ends_with(".atl") || p.ends_with(".geal") || is_shader_source(&p)
}

/// Text formats with a dedicated view: JSON-bodied materials, environments, timelines
/// and v5 emitters; keyword trails/emitters; dungeon graphs.
fn is_structured_text(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".mat") || p.ends_with(".env") || p.ends_with(".atl") || p.ends_with(".pet") || p.ends_with(".trl") || p.ends_with(".dgr")
}

/// Splits a file into its plain-text header (`version 5`, …) and the JSON document that follows.
fn json_body(text: &str) -> Option<(String, serde_json::Value)> {
    let mut header = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with('{') || t.starts_with('[') {
            break;
        }
        header.push(line.trim().to_string());
        offset += line.len();
    }
    if offset >= text.len() {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(&text[offset..]).ok()?;
    Some((header.into_iter().filter(|h| !h.is_empty()).collect::<Vec<_>>().join(" · "), value))
}

/// Small RGBA previews for texture paths, decoded without touching the on-disk art cache.
fn fetch_thumbnails(
    reader: Option<&GgpkReader>,
    index: &crate::bundles::index::Index,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
    paths: Vec<String>,
    max_px: u32,
) -> Vec<(String, Option<egui::ColorImage>)> {
    paths
        .into_iter()
        .map(|path| {
            let img = resolve_texture_path(index, &path)
                .and_then(|info| extract_bundle_file_sync(info, index, reader, steam_loader))
                .and_then(|bytes| decode_dds_rgba(&bytes))
                .map(|img| {
                    let (w, h) = (img.width().max(1), img.height().max(1));
                    let scale = (max_px as f32 / w as f32).min(max_px as f32 / h as f32).min(1.0);
                    let small = image::imageops::thumbnail(&img, ((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
                    rgba_to_color_image(&small)
                });
            (path, img)
        })
        .collect()
}

/// Shader sources (and their includes) get HLSL highlighting.
fn is_shader_source(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".hlsl") || p.ends_with(".vshader") || p.ends_with(".pshader") ||
    p.ends_with(".fx") || p.ends_with(".inc") || p.ends_with(".h")
}

/// Files parsed and shown as a JSON tree. `.hideout` decoration files are UTF-8 JSON.
fn is_json_path(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".json") || p.ends_with(".hideout")
}

/// PoE 2 `.dds.header` files carry a small streaming prefix before a regular
/// DDS (header + lowest mips). Returns the slice starting at the `DDS ` magic.
fn dds_payload(data: &[u8]) -> &[u8] {
    let scan = data.len().min(64);
    data[..scan]
        .windows(4)
        .position(|w| w == b"DDS ")
        .map(|off| &data[off..])
        .unwrap_or(data)
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
    p.ends_with(".dds") || p.ends_with(".dds.header") || p.ends_with(".png") || p.ends_with(".jpg") ||
    p.ends_with(".jpeg") || p.ends_with(".webp") || p.ends_with(".bmp")
}

fn display_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
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
    } else if is_json_path(&p) || is_text_file(&p) {
        "TEXT"
    } else if crate::parsers::model::is_model_path(&p) {
        "MODEL"
    } else if p.ends_with(".psg") || p.ends_with(".fxgraph") {
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
    crate::parsers::utils::decode_text_lossy(data)
}

fn parse_with_new_formats(path: &str, data: &[u8]) -> Option<crate::parsers::ParsedContent> {
    if let Some(format) = is_supported_format(path) {
        crate::parsers::parse(format, data).ok()
    } else {
        None
    }
}

fn launch_bink_player(path: &std::path::Path, _game_root: Option<&std::path::Path>) -> Result<(), String> {
    let path_str = path.to_string_lossy().into_owned();

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        let raw_tail = format!(r#""{}" /W1280 /H720"#, path_str);

        let try_binkplay = |exe: &std::path::Path| -> bool {
            if !exe.exists() {
                return false;
            }
            std::process::Command::new(exe)
                .raw_arg(&raw_tail)
                .spawn()
                .is_ok()
        };

        let rad_paths = [
            r"C:\Program Files\RADVideo\binkplay.exe",
            r"C:\Program Files (x86)\RADVideo\binkplay.exe",
            r"C:\Program Files\RAD Game Tools\binkplay.exe",
            r"C:\Program Files (x86)\RAD Game Tools\binkplay.exe",
        ];
        for loc in &rad_paths {
            if try_binkplay(std::path::Path::new(loc)) {
                return Ok(());
            }
        }
    }

    if std::process::Command::new("ffplay")
        .args(["-autoexit", "-x", "1280", "-y", "720", "-window_title", "BK2 Preview"])
        .arg(&path_str)
        .spawn()
        .is_ok()
    {
        return Ok(());
    }

    // mpv — common on Linux/macOS, backed by ffmpeg so Bink 2 works
    #[cfg(not(target_os = "windows"))]
    if std::process::Command::new("mpv")
        .args(["--autofit=1280x720", "--title=BK2 Preview"])
        .arg(&path_str)
        .spawn()
        .is_ok()
    {
        return Ok(());
    }

    // vlc — widely available Linux/macOS fallback
    #[cfg(not(target_os = "windows"))]
    if std::process::Command::new("vlc")
        .arg(&path_str)
        .spawn()
        .is_ok()
    {
        return Ok(());
    }

    // Last resort: system default handler (ShellExecute on Windows, xdg-open on Linux,
    // open on macOS). On Windows this respects the .bk2 file-type association set by
    // the RAD installer, which is equivalent to double-clicking the file in Explorer.
    open::that(path).map_err(|e| e.to_string())
}

/// Synchronously extracts one file from the bundle system without going through the cache.
pub(crate) fn extract_bundle_file_sync(
    file_info: &crate::bundles::index::FileInfo,
    index: &crate::bundles::index::Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
) -> Option<Vec<u8>> {
    if file_info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL {
        let r = reader?;
        let rec = r.read_file_by_path(&file_info.path).ok().flatten()?;
        return r.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec());
    }

    let bundle_info = index.bundles.get(file_info.bundle_index as usize)?;
    let raw = fetch_bundle_raw(bundle_info, reader, steam_loader)?;

    let mut cursor = std::io::Cursor::new(raw);
    let header = crate::bundles::bundle::Bundle::read_header(&mut cursor).ok()?;
    let data = header.decompress(&mut cursor).ok()?;
    let start = file_info.file_offset as usize;
    let end = start + file_info.file_size as usize;
    if end <= data.len() { Some(data[start..end].to_vec()) } else { None }
}

/// Compressed bytes of one bundle, from the GGPK if it has them, else the Steam install.
fn fetch_bundle_raw(
    bundle_info: &crate::bundles::index::BundleInfo,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
) -> Option<Vec<u8>> {
    let from_ggpk = reader.and_then(|reader| {
        let candidates = [
            format!("Bundles2/{}", bundle_info.name),
            format!("Bundles2/{}.bundle.bin", bundle_info.name),
        ];
        candidates.iter().find_map(|c| {
            reader.read_file_by_path(c).ok().flatten().and_then(|rec| {
                reader.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec())
            })
        })
    });
    from_ggpk.or_else(|| steam_loader.and_then(|s| s.fetch_bundle(&bundle_info.name).ok()))
}

fn is_dat_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".dat") || p.ends_with(".dat64") || p.ends_with(".datc64") || p.ends_with(".datl") || p.ends_with(".datl64")
}

fn dat_row_count(data: &[u8]) -> Option<u32> {
    if data.len() < 12 {
        return None;
    }
    let rc = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    (rc as usize <= data.len()).then_some(rc)
}

/// Row count of every DAT in the index. Each bundle is decompressed once and only the
/// header of each file is read, so this is far cheaper than extracting the tables.
pub(crate) fn scan_table_stats(
    index: &crate::bundles::index::Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
) -> Vec<crate::dat::analysis::TableStats> {
    use crate::dat::analysis::TableStats;
    let mut by_bundle: HashMap<u32, Vec<&crate::bundles::index::FileInfo>> = HashMap::new();
    for f in index.files.values().filter(|f| is_dat_path(&f.path)) {
        by_bundle.entry(f.bundle_index).or_default().push(f);
    }
    let mut out = Vec::new();
    for (bundle_index, files) in by_bundle {
        if bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL {
            let Some(r) = reader else { continue };
            for f in files {
                let Ok(Some(rec)) = r.read_file_by_path(&f.path) else { continue };
                let Ok(data) = r.get_data_slice(rec.data_offset, rec.data_length) else { continue };
                if let Some(row_count) = dat_row_count(data) {
                    out.push(TableStats { path: f.path.clone(), row_count });
                }
            }
            continue;
        }
        let Some(bundle_info) = index.bundles.get(bundle_index as usize) else { continue };
        let Some(raw) = fetch_bundle_raw(bundle_info, reader, steam_loader) else { continue };
        let mut cursor = std::io::Cursor::new(raw);
        let Ok(header) = crate::bundles::bundle::Bundle::read_header(&mut cursor) else { continue };
        let Ok(data) = header.decompress(&mut cursor) else { continue };
        for f in files {
            let start = f.file_offset as usize;
            let end = start + f.file_size as usize;
            if end > data.len() {
                continue;
            }
            if let Some(row_count) = dat_row_count(&data[start..end]) {
                out.push(TableStats { path: f.path.clone(), row_count });
            }
        }
    }
    out
}

pub(crate) fn find_file_info_by_path<'a>(
    index: &'a crate::bundles::index::Index,
    path: &str,
) -> Option<&'a crate::bundles::index::FileInfo> {
    index.files.values().find(|f| f.path.eq_ignore_ascii_case(path))
}

/// Every DDS path the given `.psg`'s art (icons + tree-context frames,
/// connectors, group backgrounds) will need, deduplicated. Only 771 unique
/// node icons exist across the *entire* game's passive skill data, so a
/// single tree's subset is small enough to bulk-request in one go.
pub(crate) fn collect_needed_texture_paths(
    psg: &crate::dat::psg::PsgFile,
    db: &crate::ui::atlas_node_db::SkillGraphDatabase,
) -> Vec<String> {
    use crate::ui::psg_viewer as pv;
    let tree_context = crate::ui::atlas_node_db::tree_context_for_graph_type(psg.graph_type);
    let mut paths = Vec::new();

    let push_art_set = |art: &crate::ui::skill_tree_art::SkillTreeArtSet, paths: &mut Vec<String>| {
        paths.push(art.group_background.small.clone());
        paths.push(art.group_background.medium.clone());
        paths.push(art.group_background.large.clone());
        paths.push(art.connection.normal.clone());
        paths.push(art.connection.active.clone());
        for frame in art.frames.values() {
            paths.push(frame.normal.clone());
            paths.push(frame.active.clone());
        }
    };

    match psg.graph_type {
        1 => {
            paths.push(crate::ui::atlas_node_db::ATLAS_MAIN_TREE_BG_PATH.to_string());
            paths.push(pv::ATLAS_START.to_string());
            for d in &db.decorators {
                paths.push(d.background.clone());
                paths.push(d.blocked.clone());
            }
        }
        2 => {
            paths.push(pv::BREACH_BACKDROP.to_string());
            paths.push(pv::BREACH_START.to_string());
        }
        _ => {
            paths.push(pv::MAIN_CIRCLE.to_string());
            paths.push(pv::MAIN_CIRCLE_ACTIVE.to_string());
            paths.push(pv::PLUS_FRAME_NORMAL.to_string());
            paths.push(pv::PLUS_FRAME_ACTIVE.to_string());
            for &c in &db.playable_characters() {
                let ch = &db.characters[c];
                if let Some(i) = &ch.illustration {
                    paths.push(i.clone());
                }
            }
            for (i, a) in db.ascendancies.iter().enumerate() {
                if !a.is_enabled() {
                    continue;
                }
                if let Some(img) = &a.illustration {
                    paths.push(img.clone());
                }
                if let Some(art) = db.ui_art_for_ascendancy(i) {
                    push_art_set(art, &mut paths);
                }
            }
            for frame in &db.node_frames {
                paths.push(frame.normal.clone());
                paths.push(frame.active.clone());
            }
        }
    }

    if let Some(art) = db.art_sets.get(tree_context) {
        push_art_set(art, &mut paths);
    }

    for group in &psg.groups {
        for node in &group.nodes {
            if let Some(info) = db.nodes.get(&node.skill_id) {
                if let Some(icon) = &info.icon {
                    paths.push(icon.clone());
                }
                if let Some(pattern) = info.mastery_group.and_then(|g| db.mastery_effect_images.get(&g)) {
                    paths.push(pattern.clone());
                }
                if let Some((bg, _, _)) = &info.atlas_subtree_background {
                    paths.push(bg.clone());
                }
                if let Some(icon) = &info.atlas_subtree_icon {
                    paths.push(icon.clone());
                }
            }
        }
    }

    paths.retain(|p| !p.is_empty());
    paths.sort();
    paths.dedup();
    paths
}

/// Fetches the DAT/CSD files needed to resolve skill graph nodes (passive
/// tree, atlas, and league/Brequel trees all share the `PassiveSkills` table,
/// just with different stat-description sources) and builds the resolved
/// database. Runs on a background thread — these files total several MB and
/// parsing them synchronously would stall a frame.
pub(crate) fn build_skill_graph_db(
    reader: Option<&GgpkReader>,
    index: &crate::bundles::index::Index,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
    schema: &crate::dat::schema::Schema,
) -> Result<crate::ui::atlas_node_db::SkillGraphDatabase, String> {
    let fetch = |path: &str| -> Result<Vec<u8>, String> {
        let info = find_file_info_by_path(index, path)
            .ok_or_else(|| format!("File not found in bundle index: {}", path))?;
        extract_bundle_file_sync(info, index, reader, steam_loader)
            .ok_or_else(|| format!("Failed to read file: {}", path))
    };
    let fetch_optional = |path: &str| -> Option<Vec<u8>> { fetch(path).ok() };
    let passiveskills_bytes = fetch("data/balance/passiveskills.datc64")?;
    let stats_bytes = fetch("data/balance/stats.datc64")?;

    // Covers all three known graph types: character/ascendancy, atlas, and
    // Brequel (Chayula league tree). Later files redefine earlier entries,
    // so the generic `stat_descriptions.csd` goes before the passive-tree
    // files; the atlas files come first so they never shadow passive text.
    let csd_paths = [
        "data/statdescriptions/atlas_stat_descriptions.csd",
        "data/statdescriptions/atlas_variant_stat_descriptions.csd",
        "data/statdescriptions/stat_descriptions.csd",
        "data/statdescriptions/passive_skill_stat_descriptions.csd",
        "data/statdescriptions/passive_skill_variant_stat_descriptions.csd",
    ];
    let mut stat_csd_sources = Vec::new();
    for path in csd_paths {
        let bytes = fetch(path)?;
        stat_csd_sources.push(crate::ui::atlas_node_db::StatCsdSource {
            path: path.to_string(),
            bytes,
        });
    }

    let extra = crate::ui::atlas_node_db::ExtraTables {
        ascendancy: fetch_optional("data/balance/ascendancy.datc64"),
        atlas_subtrees: fetch_optional("data/balance/atlaspassiveskillsubtrees.datc64"),
        characters: fetch_optional("data/balance/characters.datc64"),
        decorators: fetch_optional("data/balance/passivetreedecorators.datc64"),
        mastery_groups: fetch_optional("data/balance/passiveskillmasterygroups.datc64"),
        mastery_art: fetch_optional("data/balance/passiveskilltreemasteryart.datc64"),
    };

    let mut db = crate::ui::atlas_node_db::build(
        passiveskills_bytes,
        stats_bytes,
        &stat_csd_sources,
        extra,
        schema,
    )?;

    let node_frame_bytes = fetch("data/balance/passiveskilltreenodeframeart.datc64")?;
    let connection_bytes = fetch("data/balance/passiveskilltreeconnectionart.datc64")?;
    let ui_art_bytes = fetch("data/balance/passiveskilltreeuiart.datc64")?;
    let node_frames = crate::ui::skill_tree_art::parse_node_frame_art(node_frame_bytes, schema)?;
    let connections = crate::ui::skill_tree_art::parse_connection_art(connection_bytes, schema)?;
    let (art_sets, ui_art_ids) = crate::ui::skill_tree_art::parse_ui_art(ui_art_bytes, &node_frames, &connections)?;
    db.art_sets = art_sets;
    db.ui_art_ids = ui_art_ids;
    db.node_frames = node_frames;

    Ok(db)
}

fn rgba_to_color_image(img: &image::RgbaImage) -> egui::ColorImage {
    let size = [img.width() as usize, img.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
}

/// Decodes DDS (or PNG/JPG/WebP) bytes, picking a mip that fits the tree-art budget.
fn decode_dds_rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
    /// Tree art is drawn at world scale, never 1:1, so pick the first mip that
    /// fits — the 4000² centre ring and 9960² Breach backdrop would otherwise
    /// cost hundreds of MB of GPU memory each.
    const MAX_DIM: u32 = 1024;
    let mut cursor = std::io::Cursor::new(bytes);
    if let Ok(dds) = ddsfile::Dds::read(&mut cursor) {
        let mips = dds.get_num_mipmap_levels().max(1);
        let mut mip = 0u32;
        let mut dim = dds.get_width().max(dds.get_height());
        while dim > MAX_DIM && mip + 1 < mips {
            mip += 1;
            dim /= 2;
        }
        if let Ok(image) = image_dds::image_from_dds(&dds, mip) {
            return Some(image);
        }
    }
    image::load_from_memory(bytes).ok().map(|img| img.to_rgba8())
}

/// Decoded skill-tree art is cached on disk as PNG, keyed by the file's index
/// entry (path, size, bundle) so it is refreshed when the game updates.
fn tree_art_cache_path(path: &str, info: &crate::bundles::index::FileInfo) -> std::path::PathBuf {
    let key = crate::bundles::index::murmur_hash64a(
        format!("{}|{}|{}", path.to_ascii_lowercase(), info.file_size, info.bundle_index).as_bytes(),
    );
    crate::settings::AppSettings::get_app_data_dir()
        .join("cache")
        .join("tree_art")
        .join(format!("{:016x}.png", key))
}

/// Decompresses a whole bundle (the raw payload for `bundle_index`).
pub(crate) fn decompress_bundle(
    bundle_index: u32,
    index: &crate::bundles::index::Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
) -> Option<Vec<u8>> {
    let bundle_info = index.bundles.get(bundle_index as usize)?;
    let raw = reader
        .and_then(|reader| {
            [format!("Bundles2/{}", bundle_info.name), format!("Bundles2/{}.bundle.bin", bundle_info.name)]
                .iter()
                .find_map(|c| {
                    reader.read_file_by_path(c).ok().flatten().and_then(|rec| {
                        reader.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec())
                    })
                })
        })
        .or_else(|| steam_loader.and_then(|s| s.fetch_bundle(&bundle_info.name).ok()))?;
    let mut cursor = std::io::Cursor::new(raw);
    let header = crate::bundles::bundle::Bundle::read_header(&mut cursor).ok()?;
    header.decompress(&mut cursor).ok()
}

/// DAT-stored texture paths under `Art/2DArt/UIImages/...` (group
/// backgrounds, node frames) are missing a `Textures/Interface/2D/` segment
/// that the actual bundle path has — confirmed against the real index:
/// `Art/2DArt/UIImages/InGame/PassiveSkillScreenGroupBackgroundSmall` in the
/// DAT resolves to
/// `Art/Textures/Interface/2D/2DArt/UIImages/InGame/PassiveSkillScreenGroupBackgroundSmall.dds`
/// on disk. Icon (`SkillIcons`) and connector (`PassiveTree`) paths don't
/// need this — only try it for the `UIImages` case.
pub(crate) fn dds_path_candidates(path: &str) -> Vec<String> {
    let mut candidates = vec![path.to_string(), format!("{}.dds", path)];
    let lower = path.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("art/2dart/uiimages") {
        let suffix = &path[path.len() - rest.len()..];
        let corrected = format!("Art/Textures/Interface/2D/2DArt/UIImages{}", suffix);
        candidates.push(format!("{}.dds", corrected));
        candidates.push(corrected);
    }
    candidates
}

/// Fetches and decodes a batch of skill-graph art textures by path.
fn fetch_and_decode_dds_batch(
    reader: Option<&GgpkReader>,
    index: &crate::bundles::index::Index,
    steam_loader: Option<&crate::bundles::steam::SteamBundleLoader>,
    paths: Vec<String>,
) -> Vec<(String, Option<egui::ColorImage>)> {
    let cache_dir = crate::settings::AppSettings::get_app_data_dir().join("cache").join("tree_art");
    let _ = std::fs::create_dir_all(&cache_dir);

    // Group by bundle so each bundle is decompressed once per batch.
    let mut jobs: Vec<(String, Option<&crate::bundles::index::FileInfo>)> =
        paths.into_iter().map(|p| { let info = resolve_texture_path(index, &p); (p, info) }).collect();
    jobs.sort_by_key(|(_, info)| info.map(|i| (i.bundle_index, i.file_offset)).unwrap_or((u32::MAX, 0)));

    let mut current_bundle: Option<(u32, Vec<u8>)> = None;
    jobs.into_iter()
        .map(|(path, info)| {
            let Some(info) = info else { return (path, None) };
            let cache_file = tree_art_cache_path(&path, info);
            if let Ok(img) = image::open(&cache_file) {
                return (path, Some(rgba_to_color_image(&img.to_rgba8())));
            }
            let bytes = if info.bundle_index == crate::bundles::index::GGPK_LOOSE_FILE_SENTINEL
                || info.bundle_index == crate::bundles::steam::LOOSE_FILE_SENTINEL
            {
                extract_bundle_file_sync(info, index, reader, steam_loader)
            } else {
                if current_bundle.as_ref().map(|(b, _)| *b != info.bundle_index).unwrap_or(true) {
                    current_bundle = decompress_bundle(info.bundle_index, index, reader, steam_loader).map(|d| (info.bundle_index, d));
                }
                current_bundle.as_ref().and_then(|(_, data)| {
                    let start = info.file_offset as usize;
                    let end = start + info.file_size as usize;
                    (end <= data.len()).then(|| data[start..end].to_vec())
                })
            };
            let img = bytes.and_then(|b| decode_dds_rgba(&b));
            if let Some(img) = &img {
                let _ = img.save(&cache_file);
            }
            (path, img.map(|i| rgba_to_color_image(&i)))
        })
        .collect()
}

/// Index entry for a DAT-style texture path (with or without `.dds`, with or
/// without the `Textures/Interface/2D` segment).
pub(crate) fn resolve_texture_path<'a>(index: &'a crate::bundles::index::Index, path: &str) -> Option<&'a crate::bundles::index::FileInfo> {
    dds_path_candidates(path)
        .iter()
        .find_map(|candidate| find_file_info_by_path(index, candidate))
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

#[cfg(test)]
mod skill_graph_pipeline_tests {
    use super::*;
    use crate::bundles::index::Index as BundleIndex;
    use std::sync::Arc;

    /// End-to-end check against the real GGPK: builds the full skill graph
    /// database (nodes + art) for a given `.psg`, derives the texture paths
    /// it needs, and fetches+decodes a sample of them — catching any
    /// regression in the fetch/decode wiring that a GUI click can't easily
    /// be automated to exercise in CI.
    fn check_tree(psg_path: &str, min_nodes: usize) {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json"))
            .expect("schema.min.json not found (run the app once first)");
        let schema: crate::dat::schema::Schema = serde_json::from_str(&schema_text).unwrap();

        let db = build_skill_graph_db(Some(&reader), &index, None, &schema).expect("skill graph db build failed");
        assert!(db.nodes.len() > 1000, "expected the shared PassiveSkills table to have >1000 rows");
        assert!(!db.art_sets.is_empty(), "expected at least one resolved UIArt tree context");

        let psg_info = find_file_info_by_path(&index, psg_path).expect("psg file not found in index");
        let psg_bytes = extract_bundle_file_sync(psg_info, &index, Some(&reader), None).expect("failed to read psg");
        let psg = crate::dat::psg::parse_psg(&psg_bytes).expect("failed to parse psg");

        let node_count = psg.groups.iter().flat_map(|g| &g.nodes).filter(|n| db.nodes.contains_key(&n.skill_id)).count();
        assert!(node_count >= min_nodes, "{}: expected >= {} resolved nodes, got {}", psg_path, min_nodes, node_count);

        let needed = collect_needed_texture_paths(&psg, &db);
        assert!(!needed.is_empty(), "{}: expected at least one texture path to fetch", psg_path);

        // Test each texture "family" separately — icons (SkillIcons),
        // connectors (PassiveTree), and UI chrome (UIImages: group
        // backgrounds + node frames) resolve via different path rules (see
        // `dds_path_candidates`), so a plain alphabetical sample could
        // silently skip one family entirely.
        for (label, filter) in [
            ("icons", "skillicons"),
            ("connectors", "passivetree"),
            ("ui chrome (backgrounds/frames)", "uiimages"),
        ] {
            let family: Vec<String> = needed
                .iter()
                .filter(|p| p.to_ascii_lowercase().contains(filter))
                .take(10)
                .cloned()
                .collect();
            if family.is_empty() {
                continue; // not every tree references every family (e.g. atlas has no jewel sockets)
            }
            let decoded = fetch_and_decode_dds_batch(Some(&reader), &index, None, family.clone());
            let ok_count = decoded.iter().filter(|(_, img)| img.is_some()).count();
            assert!(
                ok_count * 2 >= family.len(),
                "{}: expected at least half of the sampled {} textures to decode, got {}/{} ({:?})",
                psg_path, label, ok_count, family.len(), family
            );
        }
    }

    #[test]
    #[ignore]
    fn atlas_tree_art_pipeline() {
        check_tree("metadata/atlasskillgraphs/atlasskillgraph.psg", 100);
    }

    #[test]
    #[ignore]
    fn chayula_tree_art_pipeline() {
        check_tree("metadata/leagueskillgraphs/chayulatreepassiveskillgraph.psg", 100);
    }

    #[test]
    #[ignore]
    fn passive_tree_art_pipeline() {
        check_tree("metadata/passiveskillgraph.psg", 100);
    }
}

#[cfg(test)]
mod schema_discovery_tests {
    use super::*;
    use crate::bundles::index::Index as BundleIndex;
    use crate::dat::analysis::{self, TableStats};
    use crate::dat::reader::DatReader;
    use std::sync::Arc;

    /// Runs the discovery heuristics over the real game data: every schema table is
    /// checked for drift and re-fitted, and every known foreign key is re-derived from
    /// row counts alone to measure how often tightest-fit ranking finds the true target.
    /// Run with: cargo test --release schema_discovery_real_data -- --ignored --nocapture
    #[test]
    #[ignore]
    fn schema_discovery_real_data() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let is_poe2 = reader.is_poe2_heuristic();
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"));
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json"))
            .expect("schema.min.json not found (run the app once first)");
        let schema: crate::dat::schema::Schema = serde_json::from_str(&schema_text).unwrap();

        // DISCOVERY_EXTS=1 prints a histogram of file extensions in the index.
        if std::env::var("DISCOVERY_EXTS").is_ok() {
            let mut counts: HashMap<String, (usize, u64)> = HashMap::new();
            for f in index.files.values() {
                let ext = f.path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
                let e = counts.entry(ext).or_default();
                e.0 += 1;
                e.1 += f.file_size as u64;
            }
            let mut rows: Vec<_> = counts.into_iter().collect();
            rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            for (ext, (n, bytes)) in rows.iter().take(60) {
                println!("ext {:<12} {:>7} files {:>8.1} MB", ext, n, *bytes as f64 / 1e6);
            }
        }

        // DISCOVERY_SAMPLE=ao,mat,pet prints the head of a few files per extension (UTF-16 decoded when BOM'd).
        if let Ok(exts) = std::env::var("DISCOVERY_SAMPLE") {
            for ext in exts.split(',') {
                let suffix = format!(".{}", ext.trim().to_ascii_lowercase());
                let mut files: Vec<&crate::bundles::index::FileInfo> = index
                    .files
                    .values()
                    .filter(|f| f.path.to_ascii_lowercase().ends_with(&suffix) && f.file_size > 64)
                    .collect();
                files.sort_by_key(|f| f.file_size);
                let picks = [files.get(files.len() / 4), files.get(files.len() / 2), files.get(files.len() * 3 / 4)];
                for fi in picks.into_iter().flatten() {
                    let Some(bytes) = extract_bundle_file_sync(fi, &index, Some(&reader), None) else { continue };
                    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
                        let u: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                        Some(String::from_utf16_lossy(&u))
                    } else if bytes.iter().take(512).all(|b| *b == 9 || *b == 10 || *b == 13 || (32..127).contains(b)) {
                        Some(String::from_utf8_lossy(&bytes).to_string())
                    } else {
                        None
                    };
                    println!("----- {} ({} bytes) -----", fi.path, bytes.len());
                    match text {
                        Some(t) => println!("{}", t.chars().take(700).collect::<String>()),
                        None => println!("binary: {:02X?}", &bytes[..bytes.len().min(48)]),
                    }
                }
            }
        }

        // DISCOVERY_VIEWERS=1 runs every dedicated parser over samples of real files and reports success rates.
        if std::env::var("DISCOVERY_VIEWERS").is_ok() {
            let sample = |ext: &str, n: usize| -> Vec<&crate::bundles::index::FileInfo> {
                let suffix = format!(".{}", ext);
                let mut files: Vec<&crate::bundles::index::FileInfo> =
                    index.files.values().filter(|f| f.path.to_ascii_lowercase().ends_with(&suffix) && f.file_size > 32).collect();
                files.sort_by_key(|f| f.path_hash);
                let step = (files.len() / n).max(1);
                files.into_iter().step_by(step).take(n).collect()
            };
            let text_of = |fi: &crate::bundles::index::FileInfo| extract_bundle_file_sync(fi, &index, Some(&reader), None).map(|b| crate::parsers::utils::decode_text_lossy(&b));
            for ext in ["ao", "ot", "it", "act", "epk"] {
                let files = sample(ext, 60);
                let (mut ok, mut with_extends, mut fails) = (0, 0, Vec::new());
                for fi in &files {
                    let Some(t) = text_of(fi) else { continue };
                    let d = crate::parsers::object_dsl::parse(&t);
                    if !d.components.is_empty() || !d.props.is_empty() {
                        ok += 1;
                    } else if fails.len() < 3 {
                        fails.push(fi.path.clone());
                    }
                    if d.extends.is_some() {
                        with_extends += 1;
                    }
                }
                println!("viewer {:<4} object    {:>3}/{:<3} parsed · {} with extends · empty: {:?}", ext, ok, files.len(), with_extends, fails);
                for path in &fails {
                    if let Some(t) = find_file_info_by_path(&index, path).and_then(text_of) {
                        println!("    head of {}: {:?}", path, t.chars().take(160).collect::<String>());
                    }
                }
            }
            for ext in ["trl", "pet"] {
                let files = sample(ext, 60);
                let (mut curves_ok, mut json_ok, mut fails) = (0, 0, Vec::new());
                for fi in &files {
                    let Some(t) = text_of(fi) else { continue };
                    if json_body(&t).is_some() {
                        json_ok += 1;
                        continue;
                    }
                    let c = crate::parsers::curves::parse(&t);
                    if c.blocks.iter().any(|b| !b.curves.is_empty()) {
                        curves_ok += 1;
                    } else if fails.len() < 3 {
                        fails.push(fi.path.clone());
                    }
                }
                println!("viewer {:<4} curves    {:>3}/{:<3} with curves · {} json · no curves: {:?}", ext, curves_ok, files.len(), json_ok, fails);
            }
            for ext in ["mat", "env", "atl"] {
                let files = sample(ext, 80);
                let (mut ok, mut fails) = (0, Vec::new());
                for fi in &files {
                    let Some(t) = text_of(fi) else { continue };
                    if json_body(&t).is_some() {
                        ok += 1;
                    } else if fails.len() < 3 {
                        fails.push(fi.path.clone());
                    }
                }
                println!("viewer {:<4} json      {:>3}/{:<3} parsed · not json: {:?}", ext, ok, files.len(), fails);
            }
            {
                let files = sample("dgr", 60);
                let (mut ok, mut fails) = (0, Vec::new());
                for fi in &files {
                    let Some(t) = text_of(fi) else { continue };
                    match crate::parsers::level::parse_dgr(&t) {
                        Ok(g) if !g.nodes.is_empty() => ok += 1,
                        Ok(_) => fails.push(format!("{} (no nodes)", fi.path)),
                        Err(e) => {
                            if fails.len() < 3 {
                                fails.push(format!("{} ({})", fi.path, e));
                            }
                        }
                    }
                }
                println!("viewer dgr  graph     {:>3}/{:<3} parsed · {:?}", ok, files.len(), fails);
                for f in &fails {
                    let path = f.split(" (").next().unwrap_or(f);
                    if let Some(t) = find_file_info_by_path(&index, path).and_then(text_of) {
                        println!("    head of {}: {:?}", path, t.chars().take(220).collect::<String>());
                    }
                }
            }
            for ext in ["fmt", "tgm", "smd", "ast"] {
                let files = sample(ext, 25);
                let (mut parsed, mut drawable, mut fails) = (0, 0, Vec::new());
                let t0 = std::time::Instant::now();
                for fi in &files {
                    let Some(bytes) = extract_bundle_file_sync(fi, &index, Some(&reader), None) else { continue };
                    match crate::parsers::model::parse_model(&fi.path, &bytes) {
                        Ok(m) => {
                            parsed += 1;
                            if crate::ui::mesh_preview::extract(&m).is_some() {
                                drawable += 1;
                            } else if fails.len() < 3 {
                                let summary = m.summary().to_string();
                                fails.push(format!("{} :: {}", fi.path, summary.chars().take(260).collect::<String>()));
                            }
                        }
                        Err(e) => {
                            if fails.len() < 3 {
                                fails.push(format!("{} ({})", fi.path, e));
                            }
                        }
                    }
                }
                println!("viewer {:<4} mesh      {:>3}/{:<3} parsed · {} drawable · {:.1?} · {:?}", ext, parsed, files.len(), drawable, t0.elapsed(), fails);
            }
        }

        let t0 = std::time::Instant::now();
        let stats = scan_table_stats(&index, Some(&reader), None);
        println!("scan_table_stats: {} tables in {:.1?}", stats.len(), t0.elapsed());
        assert!(stats.len() > 100, "expected the index to hold hundreds of DAT files");

        // DISCOVERY_FIND=mat_table prints every DAT whose path contains the text and whether it parses.
        if let Ok(needle) = std::env::var("DISCOVERY_FIND") {
            let needle = needle.to_ascii_lowercase();
            for ts in stats.iter().filter(|t| t.path.to_ascii_lowercase().contains(&needle)) {
                let parsed = find_file_info_by_path(&index, &ts.path)
                    .and_then(|fi| extract_bundle_file_sync(fi, &index, Some(&reader), None))
                    .map(|bytes| {
                        let len = bytes.len();
                        match DatReader::new(bytes, &ts.path) {
                            Ok(d) => format!("{} bytes · {} rows · row_len {:?} · 64-bit {}", len, d.row_count, d.row_length, d.is_64bit),
                            Err(e) => format!("{} bytes · not a DAT table: {}", len, e),
                        }
                    })
                    .unwrap_or_else(|| "could not extract".into());
                let stem = ts.path.rsplit('/').next().unwrap_or("").rsplit_once('.').map(|(s, _)| s).unwrap_or("");
                let in_schema = schema.tables.iter().any(|t| t.name.eq_ignore_ascii_case(stem));
                println!("find {:<50} schema:{} · {}", ts.path, in_schema, parsed);
            }
        }

        let mut tables: Vec<&TableStats> = stats
            .iter()
            .filter(|t| {
                // PoE 1 keeps tables in data/, PoE 2 in data/balance/; language copies sit one level deeper.
                let p = t.path.to_ascii_lowercase();
                let dir = p.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                p.ends_with(".datc64") && (dir == "data" || dir == "data/balance")
            })
            .collect();
        tables.sort_by(|a, b| a.path.cmp(&b.path));
        println!("{} base-language tables", tables.len());

        let (mut checked, mut drifted, mut unknown) = (0usize, 0usize, 0usize);
        let (mut fk_total, mut fk_top1, mut fk_top5) = (0usize, 0usize, 0usize);
        let mut misses: Vec<String> = Vec::new();
        let t1 = std::time::Instant::now();
        for ts in tables {
            let stem = ts.path.rsplit('/').next().unwrap().trim_end_matches(".datc64");
            let Some(tdef) = schema.find_table(stem, is_poe2) else {
                unknown += 1;
                println!("unknown {:<36} {} rows", stem, ts.row_count);
                continue;
            };
            let Some(fi) = find_file_info_by_path(&index, &ts.path) else { continue };
            let Some(bytes) = extract_bundle_file_sync(fi, &index, Some(&reader), None) else { continue };
            let Ok(dat) = DatReader::new(bytes, &ts.path) else { continue };
            let Some(row_len) = dat.row_length else { continue };
            if dat.row_count == 0 {
                continue;
            }
            checked += 1;
            let schema_width = tdef.row_width(dat.is_64bit);
            if row_len != schema_width {
                drifted += 1;
                let cols = analysis::analyze(&dat);
                let (aligned, report) = analysis::align_schema(tdef, &cols, dat.is_64bit);
                assert_eq!(aligned.row_width(dat.is_64bit), row_len, "aligned layout must tile the row: {}", ts.path);
                println!(
                    "drift {:<36} file {:>4} schema {:>4} → matched {:>3} added {:>2} dropped {:?}",
                    tdef.name, row_len, schema_width, report.matched, report.added.len(), report.dropped
                );
                // DISCOVERY_DUMP=SkillGems prints the newly exposed columns with sample values.
                if std::env::var("DISCOVERY_DUMP").map(|v| v.eq_ignore_ascii_case(&tdef.name)).unwrap_or(false) {
                    let rows: Vec<_> = (0..dat.row_count.min(3)).filter_map(|r| dat.read_row(r, &aligned).ok()).collect();
                    for &ci in &report.added {
                        let col = &aligned.columns[ci];
                        let samples: Vec<String> = rows
                            .iter()
                            .filter_map(|vals| vals.get(ci))
                            .map(|v| {
                                let mut s = dat.value_to_json(v, col).to_string();
                                if s.len() > 60 {
                                    s.truncate(60);
                                    s.push('…');
                                }
                                s
                            })
                            .collect();
                        println!("    new {:<22} {}", col.name.clone().unwrap_or_default(), samples.join(" | "));
                    }
                }
                continue;
            }
            let base_dir = ts.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            for (ci, st) in analysis::foreign_key_stats(&dat, tdef).iter().enumerate() {
                let (Some(st), Some(refr)) = (st, tdef.columns[ci].references.as_ref()) else { continue };
                if st.non_null == 0 {
                    continue;
                }
                let ranked = analysis::rank_targets(st, &stats, base_dir, "datc64", &ts.path.to_ascii_lowercase(), 5, tdef.columns[ci].name.as_deref());
                fk_total += 1;
                match ranked.iter().position(|c| c.stem.eq_ignore_ascii_case(&refr.table)) {
                    Some(0) => {
                        fk_top1 += 1;
                        fk_top5 += 1;
                    }
                    Some(_) => fk_top5 += 1,
                    None => {
                        if misses.len() < 20 {
                            let col = tdef.columns[ci].name.clone().unwrap_or_default();
                            let got: Vec<&str> = ranked.iter().map(|c| c.stem.as_str()).collect();
                            misses.push(format!("{}.{} → {} (max index {}, got {:?})", tdef.name, col, refr.table, st.max_index, got));
                        }
                    }
                }
            }
        }
        println!("checked {} tables in {:.1?} · drifted {} · not in schema {}", checked, t1.elapsed(), drifted, unknown);
        println!(
            "foreign keys {} · true target ranked #1: {} ({:.0}%) · in top 5: {} ({:.0}%)",
            fk_total,
            fk_top1,
            100.0 * fk_top1 as f32 / fk_total.max(1) as f32,
            fk_top5,
            100.0 * fk_top5 as f32 / fk_total.max(1) as f32
        );
        for m in &misses {
            println!("  miss {}", m);
        }
        assert!(checked > 100);
        assert!(fk_total > 50);
        assert!(fk_top5 * 2 >= fk_total, "tightest-fit ranking should put the true target in the top 5 at least half the time");
    }
}
