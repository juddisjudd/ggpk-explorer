use eframe::egui;
use crate::ggpk::reader::GgpkReader;
use crate::ui::tree_view::TreeView;
use crate::ui::content_view::ContentView;
use rfd::FileDialog;
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug)]
pub enum FileSelection {
    GgpkOffset(u64),
    BundleFile(u64),
    Folder(Vec<u64>, String),
}

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;


pub struct ExplorerApp {
    reader: Option<Arc<GgpkReader>>,
    tree_view: TreeView,
    pub content_view: ContentView,
    pub status_msg: String,
    pub selected_file: Option<FileSelection>,
    pub is_poe2: bool,
    pub bundle_index: Option<Arc<crate::bundles::index::Index>>,
    

    load_rx: Option<Receiver<Result<(Arc<GgpkReader>, Option<Arc<crate::bundles::index::Index>>, bool, PathBuf, String, TreeView), String>>>,
    pub schema_update_rx: Option<Receiver<Result<String, String>>>,
    pub export_status_rx: Option<Receiver<crate::export::ExportStatus>>,
    is_loading: bool,

    pub settings: crate::settings::AppSettings,
    pub settings_window: crate::ui::settings_window::SettingsWindow,
    pub export_window: crate::ui::export_window::ExportWindow,
    pub show_about: bool,
    pub update_state: crate::update::UpdateState,
}

impl ExplorerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let settings = crate::settings::AppSettings::load();
        let mut content_view = ContentView::default();
        

        let app_data_dir = crate::settings::AppSettings::get_app_data_dir();
        let default_schema_path = app_data_dir.join("schema.min.json");
        let default_schema_path_str = default_schema_path.to_string_lossy().to_string();
        
        let schema_path = settings.schema_local_path.as_deref().unwrap_or(&default_schema_path_str);

        if let Ok(data) = std::fs::read(schema_path) {
             if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                 let created_at = value.get("createdAt")
                    .and_then(|v| v.as_i64())
                    .map(|ts| {
                         let dt = chrono::DateTime::from_timestamp(ts, 0);
                         dt.map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                           .unwrap_or_else(|| "Invalid Timestamp".to_string())
                    })
                    .unwrap_or_else(|| "Unknown".to_string());
                 
                 if let Ok(s) = serde_json::from_value::<crate::dat::schema::Schema>(value) {
                     content_view.set_dat_schema(s, created_at);
                 } else {
                     println!("Failed to parse schema structure");
                 }
             } else {
                 println!("Failed to parse schema JSON");
             }
        } else {
             println!("Failed to read schema.min.json at {}", schema_path);
        }


        let patch_ver = settings.poe2_patch_version.as_str();
        

        let cache_root = app_data_dir.join("cache");
        if !cache_root.exists() {
            let _ = std::fs::create_dir_all(&cache_root);
        }
        
        let cdn = crate::bundles::cdn::CdnBundleLoader::new(&cache_root, Some(patch_ver));
        content_view.set_cdn_loader(cdn);

        let mut app = Self {
            reader: None,
            tree_view: TreeView::default(),
            content_view,
            status_msg: "Ready".into(),
            selected_file: None,
            is_poe2: false,
            bundle_index: None,
            load_rx: None,
            schema_update_rx: None,
            export_status_rx: None,
            is_loading: false,
            settings: settings.clone(),
            settings_window: crate::ui::settings_window::SettingsWindow::new(),
            export_window: crate::ui::export_window::ExportWindow::new(),
            show_about: false,
            update_state: crate::update::UpdateState::new(),
        };


        if let Some(path) = &app.settings.ggpk_path {
            let p = std::path::PathBuf::from(path);
            if p.exists() {
               app.open_ggpk_path(p, &_cc.egui_ctx);
            }
        }

        app
    }

    fn open_ggpk(&mut self, ctx: &egui::Context) {
        if let Some(path) = FileDialog::new().add_filter("GGPK", &["ggpk"]).pick_file() {
            self.settings.ggpk_path = Some(path.to_string_lossy().to_string());
            self.settings.save();
            self.open_ggpk_path(path, ctx);
        }
    }

    fn open_ggpk_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.status_msg = format!("Opening {}... (This may take a moment)", path.display());
            self.is_loading = true;
            self.reader = None;
            self.bundle_index = None;
            self.tree_view = TreeView::default();
            
            let (tx, rx) = channel();
            self.load_rx = Some(rx);
            
            let path_clone = path.clone();
            let ctx_clone = ctx.clone();
            
            thread::spawn(move || {
                let start_total = std::time::Instant::now();
                let result = (|| -> Result<(Arc<GgpkReader>, Option<Arc<crate::bundles::index::Index>>, bool, PathBuf, String, TreeView), String> {
                    let start_open = std::time::Instant::now();
                    let reader_inner = GgpkReader::open(&path_clone)
                        .map_err(|e| format!("Failed to open GGPK: {}", e))?;
                    println!("GgpkReader::open took {:?}", start_open.elapsed());
                    
                    let reader = Arc::new(reader_inner);
                    
                    let mut bundle_index = None;
                    let mut extra_status = String::new();
                    let mut found_bundle_index = false;


                    let cache_path = crate::settings::AppSettings::get_app_data_dir().join("bundles2.cache");
                    let mut loaded_from_cache = false;

                    if cache_path.exists() {
                         eprintln!("Found cache file, attempting to load...");
                         let start_cache = std::time::Instant::now();
                         match crate::bundles::index::Index::load_from_cache(&cache_path) {
                             Ok(index) => {
                                 println!("Index::load_from_cache took {:?}", start_cache.elapsed());
                                 bundle_index = Some(Arc::new(index));
                                 extra_status = " (Cached)".to_string();
                                 found_bundle_index = true;
                                 loaded_from_cache = true;
                                 eprintln!("Index loaded from cache successfully.");
                             },
                             Err(e) => {
                                 eprintln!("Failed to load cache: {}", e);

                             }
                         }
                    }


                    if !loaded_from_cache {
                        let start_scan = std::time::Instant::now();
                        eprintln!("Cache missing or invalid. Parsing Bundles2/_.index.bin...");
                        
                        match reader.read_file_by_path("Bundles2/_.index.bin") {
                            Ok(Some(file_record)) => {
                                match reader.get_data_slice(file_record.data_offset, file_record.data_length) {
                                    Ok(data) => {
                                        let mut cursor = std::io::Cursor::new(data);
                                        match crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                                            Ok(bundle) => {
                                                eprintln!("Decompressing Index Bundle ({} bytes)...", bundle.uncompressed_size);
                                                match bundle.decompress(&mut cursor) {
                                                    Ok(decompressed) => {
                                                        eprintln!("Parsing Decompressed Index...");
                                                        match crate::bundles::index::Index::read(&decompressed) {
                                                            Ok(index) => {
                                                                println!("Bundle Index parsing took {:?}", start_scan.elapsed());
                                                                

                                                                eprintln!("Saving Index to cache...");
                                                                if let Err(e) = index.save_to_cache(cache_path) {
                                                                    println!("Failed to save cache: {}", e);
                                                                } else {
                                                                    println!("Cache saved successfully.");
                                                                }
                                                                
                                                                bundle_index = Some(Arc::new(index));
                                                                extra_status = " (Bundled)".to_string();
                                                                found_bundle_index = true;
                                                            },
                                                            Err(e) => extra_status = format!(" (Index Parse Error: {})", e),
                                                        }
                                                    },
                                                    Err(e) => extra_status = format!(" (Decompress Error: {})", e),
                                                }
                                            },
                                            Err(e) => extra_status = format!(" (Bundle Header Error: {})", e),
                                        }
                                    },
                                    Err(e) => extra_status = format!(" (Read Error: {})", e),
                                }
                            },
                            Ok(None) => {
                                eprintln!("Bundles2/_.index.bin not found. This is normal for PoE 1 or un-bundled GGPKs.");
                            }, 
                            Err(e) => extra_status = format!(" (Find Error: {})", e),
                        }
                    }
                    
                    let is_poe2 = reader.version >= 4 || found_bundle_index;
                    
                    let start_tree = std::time::Instant::now();
                    let tree_view = if let Some(idx) = &bundle_index {
                        TreeView::new_bundled(reader.clone(), idx)
                    } else {
                        TreeView::new(reader.clone())
                    };
                    println!("TreeView creation took {:?}", start_tree.elapsed());
                    
                    println!("Total Loading Thread took {:?}", start_total.elapsed());
                    
                    Ok((reader, bundle_index, is_poe2, path_clone, extra_status, tree_view))
                })();
                
                let _ = tx.send(result);
                ctx_clone.request_repaint();
            });
    }

}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_state.poll();
        // Poll loader
        if self.is_loading {
             if let Some(rx) = &self.load_rx {
                 match rx.try_recv() {
                     Ok(result) => {
                         self.is_loading = false;
                         self.load_rx = None;
                         
                         match result {
                             Ok((reader, index, is_poe2, path, extra_status, tree_view)) => {
                                 // Update state with result
                                 self.reader = Some(reader.clone());
                                 self.bundle_index = index;
                                 self.is_poe2 = is_poe2;
                                 self.tree_view = tree_view;
                                 
                                 let version = reader.version;
                                 let game_ver = if self.is_poe2 { "PoE 2" } else { "PoE 1" };
                                 println!("Opened {:?} (v{}, {}){}", path, version, game_ver, extra_status);
                                 self.status_msg = format!("GGPK Mounted ({})", game_ver);
                             },
                             Err(e) => {
                                 self.status_msg = format!("Error: {}", e);
                             }
                         }
                     },
                     Err(std::sync::mpsc::TryRecvError::Empty) => {},
                     Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                         self.is_loading = false;
                         self.load_rx = None; // clear it
                         self.status_msg = "Error: Loaing thread disconnected (Panic?)".to_string();
                         eprintln!("Loading thread disconnected!");
                     }
                 }
             }
        }
        

    
        // ... top panel ...
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open GGPK...").clicked() {
                        self.open_ggpk(ui.ctx());
                        ui.close_menu();
                    }
                    if ui.button("Settings").clicked() {
                         self.settings_window.open();
                         ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                
                if ui.button("About").clicked() {
                    self.show_about = true;
                }
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                     ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                     
                     if let Some(ver) = &self.update_state.latest_version {
                          ui.separator();
                          if ui.link(egui::RichText::new(format!("Update Available: {}", ver)).color(egui::Color32::GREEN).strong()).clicked() {
                              if let Some(url) = &self.update_state.release_url {
                                  let _ = open::that(url);
                              }
                          }
                     }
                     
                     ui.separator();
                     ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if self.is_loading {
                            ui.spinner();
                            ui.label("Mounting GGPK...");
                        }
                        if self.status_msg.starts_with("GGPK Mounted") {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 10.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, egui::Color32::GREEN);
                        }
                        ui.label(&self.status_msg);
                     });
                });
            });
        });


        let _ = self.export_window.show(ctx);
        if self.export_window.confirmed {
             self.export_window.confirmed = false;
             if let Some(target_dir) = rfd::FileDialog::new().set_directory("/").pick_folder() {
                 let hashes = self.export_window.hashes.clone();
                 let settings = self.export_window.settings.clone();
                 
                 if let Some(reader) = &self.reader {
                     // We need the index for Bundle export. For raw GGPK, we might need adjustments.
                     // For now, pass what we have.
                     let bundle_index = self.bundle_index.clone();
                     let reader_clone = reader.clone();
                     
                     let (tx, rx) = std::sync::mpsc::channel();
                     self.export_status_rx = Some(rx);
                     self.status_msg = "Starting Export...".to_string();
                     self.is_loading = true;
                     
                     let schema_clone = self.content_view.dat_viewer.schema.clone();
                     let cdn_loader = self.content_view.cdn_loader.clone();
                     
                     std::thread::spawn(move || {
                         crate::export::run_export(
                            hashes,
                            reader_clone,
                            bundle_index,
                            settings,
                            target_dir,
                            cdn_loader,
                            schema_clone,
                            tx,
                            None
                         );
                     });
            }
        }
    }


        if self.tree_view.is_searching() {
            ctx.request_repaint();
        }

        egui::SidePanel::left("tree_panel")
            .resizable(true)
            .default_width(480.0)
            .min_width(360.0)
            .show(ctx, |ui| {
             if self.reader.is_some() {
                 ui.push_id("tree_scroll", |ui| {
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        #[allow(deprecated)]
                        { ui.style_mut().wrap = Some(false); }
                        let action = self.tree_view.show(ui, &mut self.selected_file, self.content_view.dat_viewer.schema.as_ref());
                 match action {
                     crate::ui::tree_view::TreeViewAction::None => {},
                     crate::ui::tree_view::TreeViewAction::Select => {}, // Handled by mut ref
                      crate::ui::tree_view::TreeViewAction::RequestExport { hashes, name, is_folder, settings } => {
                          self.export_window.open_for(&name, is_folder);
                          self.export_window.hashes = hashes;
                          if let Some(s) = settings {
                              self.export_window.settings = s;
                          }
                      }
                 }

                    });
                 });
             } else {
                 ui.label("No GGPK loaded");
             }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
             if let Some(reader) = &self.reader {
                 self.content_view.show(ui, reader.clone(), self.selected_file.clone(), self.is_poe2, &self.bundle_index);
             } else {
                 ui.centered_and_justified(|ui| {
                     if self.is_loading {
                         
                     } else {
                         ui.label("Open a Content.ggpk file to begin.");
                     }
                 });
             }
        });

        // Handle Export Requests from Content View
        if let Some((hashes, name, settings)) = self.content_view.export_requested.take() {
             // Determine if it's a folder or single file based on count
             let is_folder = hashes.len() > 1; 
             
             self.export_window.open_for(&name, is_folder);
             self.export_window.hashes = hashes;
             if let Some(s) = settings {
                 self.export_window.settings = s;
             }
        }
        
        if let Some(selection) = self.content_view.selection_requested.take() {
            self.selected_file = Some(selection);
        }

        // Poll Export Status
        if let Some(rx) = &self.export_status_rx {
             match rx.try_recv() {
                 Ok(status) => {
                     match status {
                         crate::export::ExportStatus::Progress { current, total, filename } => {
                             self.status_msg = format!("Exporting: [{}/{}] {}", current, total, filename);
                         },
                         crate::export::ExportStatus::Complete { count: _, errors: _, message } => {
                             self.status_msg = message;
                             self.is_loading = false;
                             self.export_status_rx = None;
                         },
                         crate::export::ExportStatus::Error(e) => {
                             self.status_msg = format!("Export Critical Error: {}", e);
                             self.is_loading = false;
                             self.export_status_rx = None;
                         }
                     }
                 },
                 Err(std::sync::mpsc::TryRecvError::Empty) => {},
                 Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                     self.status_msg = "Export Thread Disconnected".to_string();
                     self.is_loading = false;
                     self.export_status_rx = None;
                 }
             }
        }


        let old_patch_ver = self.settings.poe2_patch_version.clone();
        

        let schema_date = self.content_view.dat_viewer.schema_date.clone();
        self.settings_window.show(ctx, &mut self.settings, Some(&schema_date));
        
        if self.settings.poe2_patch_version != old_patch_ver {
             println!("Patch version changed to: {}", self.settings.poe2_patch_version);
             self.content_view.update_cdn_version(&self.settings.poe2_patch_version);
        }


        if let Some(rx) = &self.schema_update_rx {
             match rx.try_recv() {
                 Ok(Ok(text)) => {
                     self.status_msg = "Schema Updated Successfully!".to_string();
                     self.settings_window.schema_status_msg = Some("Updated!".to_string());
                     self.is_loading = false;
                     

                     if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                          let created_at = value.get("createdAt")
                             .and_then(|v| v.as_i64())
                             .map(|ts| {
                                 let dt = chrono::DateTime::from_timestamp(ts, 0);
                                 dt.map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                   .unwrap_or_else(|| "Invalid Timestamp".to_string())
                             })
                             .unwrap_or_else(|| "Unknown".to_string());
                          
                          if let Ok(s) = serde_json::from_value::<crate::dat::schema::Schema>(value) {
                              self.content_view.set_dat_schema(s, created_at);
                          } else {
                              self.status_msg = "Failed to parse new schema structure".to_string();
                          }
                      } else {
                          self.status_msg = "Failed to parse new schema JSON".to_string();
                      }
                     
                     self.schema_update_rx = None;
                 },
                 Ok(Err(e)) => {
                     self.status_msg = format!("Schema Update Failed: {}", e);
                     self.settings_window.schema_status_msg = Some("Failed".to_string());
                     self.is_loading = false;
                     self.schema_update_rx = None;
                 },
                 Err(std::sync::mpsc::TryRecvError::Empty) => {},
                 Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                     self.status_msg = "Schema Update Thread Died".to_string();
                     self.is_loading = false;
                     self.schema_update_rx = None;
                 }
             }
        }

        if (self.content_view.dat_viewer.request_update_schema || self.settings_window.request_update_schema) && self.schema_update_rx.is_none() {
             self.content_view.dat_viewer.request_update_schema = false;
             self.settings_window.request_update_schema = false;
             
             self.status_msg = "Updating Schema...".to_string();
             self.settings_window.schema_status_msg = Some("Updating...".to_string());
             
             self.is_loading = true;
             
             let app_data_dir = crate::settings::AppSettings::get_app_data_dir();
             let default_path = app_data_dir.join("schema.min.json");
             let default_path_str = default_path.to_string_lossy().to_string();
             
             let target_path = self.settings.schema_local_path.clone().unwrap_or(default_path_str);
             
             let (tx, rx) = channel();
             self.schema_update_rx = Some(rx);

             std::thread::spawn(move || {
                  let url = "https://github.com/poe-tool-dev/dat-schema/releases/latest/download/schema.min.json";
                  let result: Result<String, String> = (|| {
                      let resp = reqwest::blocking::get(url).map_err(|e| format!("Network Error: {}", e))?;
                      if !resp.status().is_success() {
                          return Err(format!("HTTP Error: {}", resp.status()));
                      }
                      let text = resp.text().map_err(|e| format!("Failed to read text: {}", e))?;
                      if let Err(e) = std::fs::write(&target_path, &text) {
                           return Err(format!("Failed to write schema to {}: {}", target_path, e));
                      }
                      Ok(text)
                  })();
                   let _ = tx.send(result);
              });
        }
        
        if self.show_about {
            egui::Window::new("About")
                .open(&mut self.show_about)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("GGPK Explorer");
                        ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        ui.separator();
                        ui.label("Created by Judd");
                        ui.add_space(8.0);
                        ui.hyperlink_to("GitHub Repository", "https://github.com/juddisjudd/ggpk-explorer");
                        ui.add_space(4.0);
                        ui.hyperlink_to("Support on Ko-fi", "https://ko-fi.com/ohitsjudd");
                        ui.add_space(8.0);
                        
                        ui.separator();
                        ui.label("Update Status:");
                        if self.update_state.pending {
                            ui.label("Checking for updates...");
                            ui.spinner();
                        } else if let Some(ver) = &self.update_state.latest_version {
                             ui.label(egui::RichText::new(format!("New version available: {}", ver)).color(egui::Color32::GREEN));
                             if let Some(url) = &self.update_state.release_url {
                                 if ui.button("Download Update").clicked() {
                                     let _ = open::that(url);
                                 }
                             }
                        } else if let Some(err) = &self.update_state.error_msg {
                             ui.label(egui::RichText::new(format!("Error checking updates: {}", err)).color(egui::Color32::RED));
                        } else {
                             ui.label("You are up to date.");
                             if ui.button("Check again").clicked() {
                                 self.update_state = crate::update::UpdateState::new();
                             }
                        }
                        ui.add_space(8.0);
                        
                        ui.separator();
                        if self.tree_view.is_searching() {
                             ui.label("Searching... ⏳");
                             ui.separator();
                        }
                        ui.label("Credits & Acknowledgements:");
                        ui.hyperlink_to("ooz (Oodle Decompression)", "https://github.com/zao/ooz");
                        ui.hyperlink_to("dat-schema", "https://github.com/poe-tool-dev/dat-schema");
                        ui.hyperlink_to("poe-dat-viewer", "https://github.com/SnosMe/poe-dat-viewer");
                        ui.hyperlink_to("LibGGPK3", "https://github.com/aianlinb/LibGGPK3");
                    });
                });
        }
    }
}
