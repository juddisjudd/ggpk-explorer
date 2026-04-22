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
    Folder {
        hashes: Vec<u64>,
        name: String,
        path: String,
    },
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
    pub patch_version_rx: Option<Receiver<Result<String, String>>>,
    pub schema_update_rx: Option<Receiver<Result<String, String>>>,
    pub schema_check_rx: Option<Receiver<Result<i64, String>>>,
    pub export_status_rx: Option<Receiver<crate::export::ExportStatus>>,
    is_loading: bool,

    pub settings: crate::settings::AppSettings,
    pub settings_window: crate::ui::settings_window::SettingsWindow,
    pub export_window: crate::ui::export_window::ExportWindow,
    pub show_about: bool,
    pub update_state: crate::update::UpdateState,
    pub sidebar_expanded: bool,
    pub inspector_open: bool,
    pub command_palette: crate::ui::command_palette::CommandPalette,
    pub command_palette_items: Vec<crate::ui::command_palette::CommandPaletteItem>,
    pub command_palette_needs_refresh: bool,
}

impl ExplorerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Apply premium dark theme
        let mut style = (*_cc.egui_ctx.style()).clone();
        crate::ui::theme::PremiumDarkTheme::apply_to_style(&mut style);
        _cc.egui_ctx.set_style(style);

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
            patch_version_rx: None,
            schema_update_rx: None,
            schema_check_rx: None,
            export_status_rx: None,
            is_loading: false,
            settings: settings.clone(),
            settings_window: crate::ui::settings_window::SettingsWindow::new(),
            export_window: crate::ui::export_window::ExportWindow::new(),
            show_about: false,
            update_state: crate::update::UpdateState::new(),
            sidebar_expanded: true,
            inspector_open: true,
            command_palette: crate::ui::command_palette::CommandPalette::default(),
            command_palette_items: Vec::new(),
            command_palette_needs_refresh: true,
        };


        if let Some(path) = &app.settings.ggpk_path {
            let p = std::path::PathBuf::from(path);
            if p.exists() {
               app.open_ggpk_path(p, &_cc.egui_ctx);
            }
        }

        // Start Auto-Check Schema
        let (tx, rx) = channel();
        app.schema_check_rx = Some(rx);
        thread::spawn(move || {
            let result = fetch_latest_schema_timestamp();
             let _ = tx.send(result);
        });

        if app.settings.auto_detect_patch_version {
            app.start_patch_version_refresh();
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

    fn start_patch_version_refresh(&mut self) {
        if self.patch_version_rx.is_some() {
            return;
        }

        let url = self.settings.patch_version_source_url.clone();
        let (tx, rx) = channel();
        self.patch_version_rx = Some(rx);

        thread::spawn(move || {
            let _ = tx.send(crate::settings::AppSettings::fetch_latest_patch_version(&url));
        });
    }

    fn start_schema_update(&mut self) {
        if self.schema_update_rx.is_some() {
            return;
        }

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

        thread::spawn(move || {
            let _ = tx.send(download_latest_schema(&target_path));
        });
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

    fn current_location_label(&self) -> String {
        match &self.selected_file {
            Some(FileSelection::BundleFile(hash)) => self
                .bundle_index
                .as_ref()
                .and_then(|index| index.files.get(hash))
                .map(|file| file.path.clone())
                .unwrap_or_else(|| format!("Bundle {:016x}", hash)),
            Some(FileSelection::Folder { path, .. }) => path.clone(),
            Some(FileSelection::GgpkOffset(offset)) => self
                .reader
                .as_ref()
                .and_then(|reader| reader.read_file_record(*offset).ok())
                .map(|file| file.name)
                .unwrap_or_else(|| format!("GGPK Offset 0x{:x}", offset)),
            None => {
                if self.reader.is_some() {
                    "Folder: Bundles".to_string()
                } else {
                    "No gppk mounted".to_string()
                }
            }
        }
    }

    fn format_bytes(size: u64) -> String {
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

    fn show_inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector_panel")
            .resizable(true)
            .min_width(220.0)
            .default_width(240.0)
            .frame(egui::Frame {
                inner_margin: egui::Margin::same(0.0),
                fill: ctx.style().visuals.panel_fill,
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                let header_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(header_w, 34.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("INSPECTOR")
                                .monospace()
                                .size(10.5)
                                .color(egui::Color32::from_rgb(113, 113, 122)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            let btn_size = egui::vec2(20.0, 20.0);
                            let (rect, response) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                            let color = if response.hovered() {
                                egui::Color32::from_rgb(239, 68, 68)
                            } else {
                                egui::Color32::from_rgb(113, 113, 122)
                            };
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "x",
                                egui::FontId::proportional(13.0),
                                color,
                            );
                            if response.on_hover_text("Close").clicked() {
                                self.inspector_open = false;
                            }
                        });
                    },
                );
                ui.separator();

                // Content area with left/right padding
                egui::Frame {
                    inner_margin: egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 8.0 },
                    ..Default::default()
                }
                .show(ui, |ui| {
                    match &self.selected_file {
                        Some(FileSelection::BundleFile(hash)) => {
                            if let Some(index) = &self.bundle_index {
                                if let Some(file) = index.files.get(hash) {
                                    ui.label(
                                        egui::RichText::new(&file.path)
                                            .monospace()
                                            .size(11.5),
                                    );
                                    ui.add_space(10.0);
                                    inspector_kv(ui, "Type", std::path::Path::new(&file.path).extension().and_then(|ext| ext.to_str()).unwrap_or("unknown"));
                                    inspector_kv(ui, "Size", &Self::format_bytes(file.file_size as u64));
                                    inspector_kv(ui, "Hash", &format!("{:016x}", hash));
                                }
                            }
                        }
                        Some(FileSelection::Folder { hashes, path, .. }) => {
                            ui.label(
                                egui::RichText::new(path)
                                    .monospace()
                                    .size(11.5),
                            );
                            ui.add_space(10.0);
                            inspector_kv(ui, "Type", "folder");
                            inspector_kv(ui, "Items", &hashes.len().to_string());
                        }
                        Some(FileSelection::GgpkOffset(offset)) => {
                            ui.label(
                                egui::RichText::new(self.current_location_label())
                                    .monospace()
                                    .size(11.5),
                            );
                            ui.add_space(10.0);
                            inspector_kv(ui, "Type", "ggpk file");
                            inspector_kv(ui, "Offset", &format!("0x{:x}", offset));
                        }
                        None => {
                            ui.add_space(ui.available_height() * 0.4);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("Select a file to inspect")
                                        .size(11.5)
                                        .color(egui::Color32::from_rgb(126, 126, 134)),
                                );
                            });
                        }
                    }
                });
            });
    }

}

fn handle_resize_zones(ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let border: f32 = 6.0;

    let pos = match ctx.input(|i| i.pointer.latest_pos()) {
        Some(p) => p,
        None => return,
    };

    let on_left   = pos.x < screen.min.x + border;
    let on_right  = pos.x > screen.max.x - border;
    let on_top    = pos.y < screen.min.y + border;
    let on_bottom = pos.y > screen.max.y - border;

    let dir = match (on_left, on_right, on_top, on_bottom) {
        (true,  _,     true,  _    ) => Some(egui::ResizeDirection::NorthWest),
        (_,     true,  true,  _    ) => Some(egui::ResizeDirection::NorthEast),
        (true,  _,     _,     true ) => Some(egui::ResizeDirection::SouthWest),
        (_,     true,  _,     true ) => Some(egui::ResizeDirection::SouthEast),
        (true,  false, false, false) => Some(egui::ResizeDirection::West),
        (false, true,  false, false) => Some(egui::ResizeDirection::East),
        (false, false, true,  false) => Some(egui::ResizeDirection::North),
        (false, false, false, true ) => Some(egui::ResizeDirection::South),
        _ => None,
    };

    if let Some(dir) = dir {
        let cursor = match dir {
            egui::ResizeDirection::North | egui::ResizeDirection::South => egui::CursorIcon::ResizeVertical,
            egui::ResizeDirection::East  | egui::ResizeDirection::West  => egui::CursorIcon::ResizeHorizontal,
            egui::ResizeDirection::NorthEast | egui::ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
            egui::ResizeDirection::NorthWest | egui::ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

fn inspector_kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(key)
                .size(10.5)
                .color(egui::Color32::from_rgb(113, 113, 122)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(11.0)
                    .color(egui::Color32::from_rgb(228, 228, 231)),
            );
        });
    });
}

fn fetch_latest_schema_timestamp() -> Result<i64, String> {
    let url = "https://github.com/poe-tool-dev/dat-schema/releases/latest/download/schema.min.json";
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(url)
        .header("User-Agent", "ggpk-explorer/0.1.0")
        .send()
        .map_err(|e| format!("Network Error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP Error: {}", resp.status()));
    }

    let val: serde_json::Value = resp.json().map_err(|e| format!("JSON Error: {}", e))?;
    val.get("createdAt")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "JSON missing 'createdAt'".to_string())
}

fn download_latest_schema(target_path: &str) -> Result<String, String> {
    let url = "https://github.com/poe-tool-dev/dat-schema/releases/latest/download/schema.min.json";
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(url)
        .header("User-Agent", "ggpk-explorer/0.1.0")
        .send()
        .map_err(|e| format!("Network Error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP Error: {}", resp.status()));
    }

    let text = resp.text().map_err(|e| format!("Failed to read text: {}", e))?;

    if let Err(e) = serde_json::from_str::<serde_json::Value>(&text) {
        return Err(format!("Invalid JSON received: {}", e));
    }

    std::fs::write(target_path, &text)
        .map_err(|e| format!("Failed to write schema to {}: {}", target_path, e))?;

    Ok(text)
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
                                 self.command_palette_needs_refresh = true;
                                 
                                 let version = reader.version;
                                 let game_ver = if self.is_poe2 { "PoE 2" } else { "PoE 1" };
                                 println!("Opened {:?} (v{}, {}){}", path, version, game_ver, extra_status);
                                 self.status_msg = String::new();
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
        
    
        let chrome_actions = crate::ui::chrome::AppChrome::show(
            ctx,
            &self.current_location_label(),
            &self.status_msg,
            self.reader.is_some(),
            self.is_loading,
            &mut self.inspector_open,
        );

        if chrome_actions.open_ggpk {
            self.open_ggpk(ctx);
        }
        if chrome_actions.open_settings {
            self.settings_window.open();
        }
        if chrome_actions.open_about {
            self.show_about = true;
        }
        if chrome_actions.toggle_inspector {
            self.inspector_open = !self.inspector_open;
        }
        if chrome_actions.open_command_palette {
            if self.command_palette_needs_refresh || self.command_palette_items.is_empty() {
                self.command_palette_items = self.tree_view.command_palette_items(120_000);
                self.command_palette_needs_refresh = false;
            }
            self.command_palette.open();
        }

        // Bottom Panel (Status Bar)
        // Extract schema date from content view
        let schema_date = self.content_view.dat_viewer.schema_date.clone();
        let poe_version = self.settings.poe2_patch_version.clone();
        
        crate::ui::status_bar::StatusBar::show(
            ctx,
            &self.status_msg,
            self.is_loading,
            self.reader.is_some(),
            &poe_version,
            &schema_date
        );

        // Export Window logic
        let _ = self.export_window.show(ctx);
        if self.export_window.confirmed {
             self.export_window.confirmed = false;
             if let Some(target_dir) = rfd::FileDialog::new().set_directory("/").pick_folder() {
                 let hashes = self.export_window.hashes.clone();
                 let settings = self.export_window.settings.clone();
                 
                 if let Some(reader) = &self.reader {
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

        if let Some(rx) = &self.patch_version_rx {
            match rx.try_recv() {
                Ok(Ok(version)) => {
                    if self.settings.poe2_patch_version != version {
                        self.settings.poe2_patch_version = version.clone();
                        self.settings.save();
                        self.content_view.update_cdn_version(&version);
                        self.status_msg = format!("Updated PoE 2 patch version to {}", version);
                    }
                    self.patch_version_rx = None;
                },
                Ok(Err(e)) => {
                    if self.status_msg == "Ready" {
                        self.status_msg = format!("Patch auto-detect failed: {}", e);
                    }
                    self.patch_version_rx = None;
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {},
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.patch_version_rx = None;
                }
            }
        }

        if self.command_palette.handle_shortcut(ctx) {
            if self.command_palette_needs_refresh || self.command_palette_items.is_empty() {
                self.command_palette_items = self.tree_view.command_palette_items(120_000);
                self.command_palette_needs_refresh = false;
            }
        }

        // Inspector toggle shortcut (Ctrl+I)
        if ctx.input(|i| i.key_pressed(egui::Key::I) && i.modifiers.ctrl && !i.modifiers.shift) {
            self.inspector_open = !self.inspector_open;
        }

        if self.command_palette.is_open() {
            ctx.request_repaint();
        }

        // Sidebar
        let reader_available = self.reader.is_some();
        let schema_ref = self.content_view.dat_viewer.schema.as_ref();
        
        crate::ui::sidebar::Sidebar::show(
            ctx,
            &mut self.sidebar_expanded,
            &mut self.tree_view,
            &mut self.selected_file,
            schema_ref,
            reader_available,
            &mut self.export_window
        );

        if self.inspector_open {
            self.show_inspector(ctx);
        }

        // Central Panel
        egui::CentralPanel::default().show(ctx, |ui| {
             if let Some(reader) = &self.reader {
                 self.content_view.show(ui, reader.clone(), self.selected_file.clone(), self.is_poe2, &self.bundle_index);
             } else {
                 ui.centered_and_justified(|ui| {
                     if self.is_loading {
                        ui.label(
                            egui::RichText::new("Mounting GGPK...")
                                .color(egui::Color32::from_rgb(126, 126, 134)),
                        );
                     } else {
                         crate::ui::components::card(ui, |ui| {
                             ui.vertical_centered(|ui| {
                                 ui.heading("Open a Content.ggpk file to begin");
                                 ui.add_space(6.0);
                                 ui.label("The reference design is mostly about layout rhythm, spacing, and hierarchy. This build now uses the same direction inside egui.");
                             });
                         });
                     }
                 });
             }
        });

        // Handle Export Requests from Content View
        if let Some((hashes, name, settings)) = self.content_view.export_requested.take() {
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

        if let Some(hash) = self.command_palette.show(ctx, &self.command_palette_items) {
            self.selected_file = Some(FileSelection::BundleFile(hash));
            self.status_msg = format!("Navigated to bundle file hash {:016x}", hash);
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

        // Poll Schema Check
        if let Some(rx) = &self.schema_check_rx {
             match rx.try_recv() {
                 Ok(Ok(remote_ts)) => {
                     // Get local timestamp
                     let local_ts = self.content_view.dat_viewer.schema.as_ref()
                        .map(|_| self.content_view.dat_viewer.schema_date.clone()) 
                        .and_then(|s| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S UTC").ok())
                        .map(|dt| dt.and_utc().timestamp());
                     
                     if let Some(local) = local_ts {
                         if remote_ts > local {
                             self.settings_window.schema_update_status = crate::ui::settings_window::SchemaUpdateStatus::UpdateAvailable;
                             if self.settings.auto_update_schema {
                                 self.settings_window.request_update_schema = true;
                             }
                         } else {
                             self.settings_window.schema_update_status = crate::ui::settings_window::SchemaUpdateStatus::UpToDate;
                         }
                     } else {
                         // Local schema might be missing or invalid date
                         self.settings_window.schema_update_status = crate::ui::settings_window::SchemaUpdateStatus::UpdateAvailable;
                         if self.settings.auto_update_schema {
                             self.settings_window.request_update_schema = true;
                         }
                     }
                     self.schema_check_rx = None;
                 },
                 Ok(Err(e)) => {
                     self.settings_window.schema_update_status = crate::ui::settings_window::SchemaUpdateStatus::Error(e);
                     self.schema_check_rx = None;
                 },
                 Err(std::sync::mpsc::TryRecvError::Empty) => {},
                 Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                     self.schema_check_rx = None;
                 }
             }
        }

        if (self.content_view.dat_viewer.request_update_schema || self.settings_window.request_update_schema) && self.schema_update_rx.is_none() {
            self.start_schema_update();
        }

        handle_resize_zones(ctx);
        
        if self.show_about {
            egui::Window::new("About")
                .open(&mut self.show_about)
                .collapsible(false)
                .resizable(false)
                .default_width(300.0)
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;

                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("GGPK Explorer")
                                .size(15.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .size(11.5)
                                .color(egui::Color32::from_rgb(113, 113, 122)),
                        );
                    });

                    ui.separator();
                    crate::ui::components::modal_section(ui, "AUTHOR");
                    ui.label(egui::RichText::new("Created by Judd").size(12.5));
                    ui.horizontal(|ui| {
                        ui.hyperlink_to("GitHub", "https://github.com/juddisjudd/ggpk-explorer");
                        ui.label(egui::RichText::new("·").color(egui::Color32::from_rgb(82, 82, 91)));
                        ui.hyperlink_to("Ko-fi", "https://ko-fi.com/ohitsjudd");
                    });

                    ui.separator();
                    crate::ui::components::modal_section(ui, "UPDATES");
                    if self.update_state.pending {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Checking...").size(12.5).color(egui::Color32::from_rgb(161, 161, 170)));
                        });
                    } else if let Some(ver) = &self.update_state.latest_version {
                        ui.label(egui::RichText::new(format!("New version: {}", ver)).size(12.5).color(egui::Color32::from_rgb(74, 222, 128)));
                        if let Some(url) = &self.update_state.release_url {
                            if ui.button("Download Update").clicked() {
                                let _ = open::that(url);
                            }
                        }
                    } else if let Some(err) = &self.update_state.error_msg {
                        ui.label(egui::RichText::new(format!("Error: {}", err)).size(12.5).color(egui::Color32::from_rgb(239, 68, 68)));
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Up to date").size(12.5).color(egui::Color32::from_rgb(113, 113, 122)));
                            if ui.small_button("Check again").clicked() {
                                self.update_state = crate::update::UpdateState::new();
                            }
                        });
                    }

                    ui.separator();
                    crate::ui::components::modal_section(ui, "CREDITS");
                    ui.hyperlink_to("ooz", "https://github.com/zao/ooz");
                    ui.hyperlink_to("dat-schema", "https://github.com/poe-tool-dev/dat-schema");
                    ui.hyperlink_to("poe-dat-viewer", "https://github.com/SnosMe/poe-dat-viewer");
                    ui.hyperlink_to("LibGGPK3", "https://github.com/aianlinb/LibGGPK3");
                });
        }
    }
}
