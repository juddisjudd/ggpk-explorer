use std::sync::Arc;
use eframe::egui;
use crate::bundles::index::Index;
use crate::ggpk::reader::GgpkReader;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

pub struct TreeView {
    reader: Option<Arc<GgpkReader>>,
    bundle_root: Option<BundleNode>,
    search_term: String,
    active_search_term: String,
    search_index: Option<Arc<SearchIndex>>,

    search_tx: Option<Sender<(Vec<bool>, Vec<bool>, usize, u64)>>, 
    search_rx: Option<Receiver<(Vec<bool>, Vec<bool>, usize, u64)>>,
    matched_results: Vec<bool>, 
    matched_descendants: Vec<bool>,
    match_count: usize,
    is_searching: bool,
    search_generation: u64,
    search_category: SearchCategory,
    // Display State
    render_limit: std::cell::Cell<usize>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum SearchCategory {
    All,
    Texture, // .dds, .png
    Audio,   // .ogg, .wem
    Text,    // .txt, .sh, .hlsl
    Data,    // .dat*
}

struct SearchIndex {

    files: Vec<(usize, String)>,

    parents: Vec<usize>, 
    max_id: usize,
}

struct BundleNode {
    id: usize,
    name: String,
    children: HashMap<String, BundleNode>,
    file_hash: Option<u64>,
}

impl Default for TreeView {
    fn default() -> Self {
        Self { 
            reader: None, 
            bundle_root: None, 
            search_term: String::new(), 
            active_search_term: String::new(),
            search_index: None,
            search_tx: None,
            search_rx: None,
            matched_results: Vec::new(),
            matched_descendants: Vec::new(),
            match_count: 0,
            is_searching: false,
            search_generation: 0,
            search_category: SearchCategory::All,
            render_limit: std::cell::Cell::new(2000),
        }
    }
}

pub enum TreeViewAction {
    None,
    Select,
    RequestExport { hashes: Vec<u64>, name: String, is_folder: bool, settings: Option<crate::ui::export_window::ExportSettings> },
}

impl TreeView {
    pub fn is_searching(&self) -> bool {
        self.is_searching
    }

    pub fn new(reader: Arc<GgpkReader>) -> Self {
        Self { 
            reader: Some(reader), 
            bundle_root: None, 
            search_term: String::new(), 
            active_search_term: String::new(),
            search_index: None,
            search_tx: None,
            search_rx: None,
            matched_results: Vec::new(),
            matched_descendants: Vec::new(),
            match_count: 0,
            is_searching: false,
            search_generation: 0,
            search_category: SearchCategory::All,
            render_limit: std::cell::Cell::new(2000),
        }
    }

    pub fn new_bundled(reader: Arc<GgpkReader>, index: &Index) -> Self {
        let (root, search_index) = Self::build_bundle_tree(index);
        let (tx, rx) = channel();
        
        Self { 
            reader: Some(reader), 
            bundle_root: Some(root), 
            search_term: String::new(), 
            active_search_term: String::new(),
            search_index: Some(Arc::new(search_index)),
            search_tx: Some(tx), 
            search_rx: Some(rx),
            matched_results: Vec::new(),
            matched_descendants: Vec::new(),
            match_count: 0,
            is_searching: false,
            search_generation: 0,
            search_category: SearchCategory::All,
            render_limit: std::cell::Cell::new(2000),
        }
    }

    fn build_bundle_tree(index: &Index) -> (BundleNode, SearchIndex) {
        let mut root = BundleNode {
            id: 0,
            name: "Bundles".to_string(),
            children: HashMap::new(),
            file_hash: None,
        };
        
        let mut next_id = 1;
        let mut files = Vec::new();


        let mut parent_map = Vec::new(); 
        parent_map.push((0, 0)); // Root parent is self

        for (hash, info) in &index.files {
            let parts: Vec<&str> = info.path.split('/').collect();
            let mut current = &mut root;
            
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() { continue; }
                
                let is_file = i == parts.len() - 1;
                let parent_id = current.id;
                
                if is_file {
                    current.children.entry(part.to_string()).or_insert_with(|| {
                        let node = BundleNode {
                            id: next_id,
                            name: part.to_string(),
                            children: HashMap::new(),
                            file_hash: Some(*hash), // Store hash
                        };
                        files.push((next_id, part.to_lowercase()));
                        parent_map.push((next_id, parent_id));
                        next_id += 1;
                        node
                    });
                } else {

                    current = current.children.entry(part.to_string()).or_insert_with(|| {
                        let node = BundleNode {
                            id: next_id,
                            name: part.to_string(),
                            children: HashMap::new(),
                            file_hash: None,
                        };
                        files.push((next_id, part.to_lowercase()));
                        parent_map.push((next_id, parent_id));
                        next_id += 1;
                        node
                    });
                }
            }
        }


        let mut parents = vec![0; next_id];
        for (child, parent) in parent_map {
            if child < parents.len() {
                parents[child] = parent;
            }
        }
        
        (root, SearchIndex { files, parents, max_id: next_id })
    }
    
    pub fn show(&mut self, ui: &mut egui::Ui, selected_file: &mut Option<crate::ui::app::FileSelection>, schema: Option<&crate::dat::schema::Schema>) -> TreeViewAction {
        let mut action = TreeViewAction::None;
        let mut trigger_search = false;

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                 let is_committed = !self.search_term.is_empty() && self.search_term == self.active_search_term;
                 

                 egui::ComboBox::from_id_salt("search_filter")
                     .selected_text(format!("{:?}", self.search_category))
                     .show_ui(ui, |ui| {
                         ui.selectable_value(&mut self.search_category, SearchCategory::All, "All");
                         ui.selectable_value(&mut self.search_category, SearchCategory::Texture, "Texture");
                         ui.selectable_value(&mut self.search_category, SearchCategory::Audio, "Audio");
                         ui.selectable_value(&mut self.search_category, SearchCategory::Text, "Text");
                         ui.selectable_value(&mut self.search_category, SearchCategory::Data, "Data");
                     });


                 if is_committed {
                     if ui.button("Clear").clicked() {
                         self.search_term.clear();
                         trigger_search = true;
                     }
                 } else {
                     if ui.add_enabled(!self.search_term.is_empty(), egui::Button::new("Search")).clicked() {
                         trigger_search = true;
                     }
                 }
                 

                 let response = ui.add_sized(ui.available_size(), egui::TextEdit::singleline(&mut self.search_term).id(ui.make_persistent_id("search_box")));
                 if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                     trigger_search = true;
                 }
            });
        });
        ui.separator();
        

        if let Some(rx) = &self.search_rx {
            if let Ok((results, descendants, count, gen)) = rx.try_recv() {
                if gen == self.search_generation {
                    self.matched_results = results;
                    self.matched_descendants = descendants;
                    self.match_count = count;
                    self.is_searching = false;
                }

            }
        }

        if trigger_search {
             self.active_search_term = self.search_term.clone();
             let term_lower = self.active_search_term.to_lowercase();
             self.search_generation += 1;
             

             

             self.render_limit.set(2000);

             if term_lower.is_empty() {
                 
             } else {
                 if let Some(index) = &self.search_index {
                     if let Some(tx) = &self.search_tx {
                         let tx = tx.clone();
                         let index = index.clone();
                         let gen_id = self.search_generation;
                         let category = self.search_category;
                         
                         self.is_searching = true;
                         
                         let start_time = std::time::Instant::now();
                         println!("Starting Search '{}' (Gen {})", self.active_search_term, gen_id);
                         
                         thread::spawn(move || {
                             let max_id = index.max_id;

                             let mut results = vec![false; max_id];
                             let mut count = 0;
                             
                             for (id, name_lower) in &index.files {
                                 if name_lower.contains(&term_lower) {

                                     let is_match = match category {
                                         SearchCategory::All => true,
                                         SearchCategory::Texture => name_lower.ends_with(".dds") || name_lower.ends_with(".png"),
                                         SearchCategory::Audio => name_lower.ends_with(".ogg") || name_lower.ends_with(".wem") || name_lower.ends_with(".wav"),
                                         SearchCategory::Text => name_lower.ends_with(".txt") || name_lower.ends_with(".sh") || name_lower.ends_with(".hlsl") || name_lower.ends_with(".vshader") || name_lower.ends_with(".pshader"),
                                         SearchCategory::Data => name_lower.starts_with("data/") || name_lower.ends_with(".dat") || name_lower.ends_with(".datc64") || name_lower.ends_with(".datl") || name_lower.ends_with(".datl64"),
                                     };

                                     if is_match {
                                         if *id < results.len() {
                                             results[*id] = true;
                                             count += 1;
                                         }
                                     }
                                 }
                             }
                             

                             let mut descendants = vec![false; max_id];
                             
                             for id in (1..max_id).rev() {
                                 let is_match = results[id];
                                 let has_desc = descendants[id];
                                 
                                 if is_match || has_desc {

                                     let parent_id = index.parents[id];
                                     if parent_id < descendants.len() { // Root parent 0 is ok
                                          descendants[parent_id] = true;
                                     }
                                 }
                             }

                             println!("Search Finished (Gen {}) in {:?}. Found {} matches.", gen_id, start_time.elapsed(), count);
                             let _ = tx.send((results, descendants, count, gen_id));
                         });
                     }
                 }
             }
        }
        
        if let Some(root) = &self.bundle_root {
            let mut render_count = 0;
            self.render_bundle_node(ui, root, selected_file, &mut action, schema, &mut render_count);
        } else if let Some(reader) = &self.reader {
            let root_offset = reader.root_offset;
            self.render_directory(ui, reader, root_offset, "Root", selected_file, schema);
        }
        
        action
    }

    fn render_bundle_node(&self, ui: &mut egui::Ui, node: &BundleNode, selected_file: &mut Option<crate::ui::app::FileSelection>, action: &mut TreeViewAction, schema: Option<&crate::dat::schema::Schema>, render_count: &mut usize) {
        if *render_count > 2000 {
            ui.label(egui::RichText::new("... Truncated (Too many items) ...").color(egui::Color32::YELLOW));
            return;
        }
        *render_count += 1;

        if !self.active_search_term.is_empty() {

            let id = node.id;

            let matches = if id < self.matched_results.len() { 
                self.matched_results[id] 
            } else { 
                self.active_search_term.is_empty() 
            };
            let has_matching_children = if id < self.matched_descendants.len() { 
                self.matched_descendants[id] 
            } else { 
                self.active_search_term.is_empty() 
            };
            
            if !matches && !has_matching_children {
                return;
            }
            
        }
        
        let use_filter_expand = !self.active_search_term.is_empty() && self.match_count < 500;

        if let Some(hash) = node.file_hash {
            let mut label = egui::RichText::new(&node.name);
            

            if node.name.ends_with(".dat") || node.name.ends_with(".datc64") || node.name.ends_with(".datl") || node.name.ends_with(".datl64") {
                if let Some(s) = schema {
                    // Assuming node.name is filename like "Stats.dat"
                    let stem = std::path::Path::new(&node.name).file_stem().and_then(|s| s.to_str());
                    if let Some(stem) = stem {
                         if !s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(stem)) {
                             label = label.color(egui::Color32::RED);
                         }
                    } else {
                         label = label.color(egui::Color32::RED);
                    }
                }
            }

            ui.push_id(hash, |ui| {
                let response = ui.button(label);
                if response.clicked() {
                     *selected_file = Some(crate::ui::app::FileSelection::BundleFile(hash));
                     *action = TreeViewAction::Select;
                }
                response.context_menu(|ui| {
                    if ui.button("Export...").clicked() {
                        *action = TreeViewAction::RequestExport { hashes: vec![hash], name: node.name.clone(), is_folder: false, settings: None };
                        ui.close_menu();
                    }
                });
            });
        } else {
            let mut id = ui.make_persistent_id(&node.name).with(&node.children.len());
            if use_filter_expand {
                id = id.with("filtered");
            }

 
            let mut children: Vec<&BundleNode> = node.children.values().collect();
            children.sort_by(|a, b| {

                let a_is_dir = !a.children.is_empty();
                let b_is_dir = !b.children.is_empty();
                if a_is_dir == b_is_dir {
                    a.name.cmp(&b.name)
                } else {
                    b_is_dir.cmp(&a_is_dir)
                }
            });


            let open_state = if !self.active_search_term.is_empty() {
                 Some(true)
            } else {
                 None
            };
            
            let response = egui::CollapsingHeader::new(&node.name)
                .id_salt(id)
                .open(open_state)
                .show(ui, |ui| {
                     let mut skipped_count = 0;
                     for child in children {
                         if *render_count >= self.render_limit.get() {
                             skipped_count += 1;
                             continue;
                         }
                         self.render_bundle_node(ui, child, selected_file, action, schema, render_count);
                     }
                     
                     if skipped_count > 0 {
                         ui.horizontal(|ui| {
                             ui.label(format!("... {} items hidden", skipped_count));
                             if ui.button("Load More (+2000)").clicked() {
                                 self.render_limit.set(self.render_limit.get() + 2000);
                             }
                         });
                     }
                });
            
            if response.header_response.clicked() {
                let mut hashes = Vec::new();
                self.collect_immediate_files(node, &mut hashes);
                *selected_file = Some(crate::ui::app::FileSelection::Folder(hashes, node.name.clone()));
                *action = TreeViewAction::Select;
            }

            response.header_response.context_menu(|ui| {
                if ui.button("Export Folder...").clicked() {
                    let mut hashes = Vec::new();
                    self.collect_hashes(node, &mut hashes);
                    *action = TreeViewAction::RequestExport { hashes, name: node.name.clone(), is_folder: true, settings: None };
                    ui.close_menu();
                }
            });
        }
    }

    fn collect_immediate_files(&self, node: &BundleNode, hashes: &mut Vec<u64>) {
        for child in node.children.values() {
            if let Some(h) = child.file_hash {
                hashes.push(h);
            }
        }
        // sort by name?
    }

    fn collect_hashes(&self, node: &BundleNode, hashes: &mut Vec<u64>) {
        if let Some(h) = node.file_hash {
            hashes.push(h);
        }
        for child in node.children.values() {
            self.collect_hashes(child, hashes);
        }
    }


    fn render_directory(&self, ui: &mut egui::Ui, reader: &GgpkReader, offset: u64, name: &str, selected_file: &mut Option<crate::ui::app::FileSelection>, schema: Option<&crate::dat::schema::Schema>) {
        let id = ui.make_persistent_id(offset);
        egui::CollapsingHeader::new(name)
            .id_salt(id)
            .show(ui, |ui| {
                match reader.read_directory(offset) {
                    Ok(dir) => {
                        use crate::ggpk::record::RecordTag;
                        

                        let mut valid_entries = Vec::new();
                        for entry in dir.entries {
                             if let Ok(header) = reader.read_record_header(entry.offset) {
                                  valid_entries.push((entry, header));
                             }
                        }
                        

                        valid_entries.sort_by(|a, b| {
                            let tag_a = a.1.tag;
                            let tag_b = b.1.tag;
                            
                            let a_is_dir = matches!(tag_a, RecordTag::PDIR);
                            let b_is_dir = matches!(tag_b, RecordTag::PDIR);
                            
                            if a_is_dir != b_is_dir {
                                b_is_dir.cmp(&a_is_dir) // True > False
                            } else {

                                a.0.offset.cmp(&b.0.offset)
                            }
                        });


                        for (entry, header) in valid_entries {
                            match header.tag {
                                RecordTag::PDIR => {
                                    match reader.read_directory(entry.offset) {
                                        Ok(sub_dir) => {
                                            self.render_directory(ui, reader, entry.offset, &sub_dir.name, selected_file, schema);
                                        },
                                        Err(_) => { ui.label("<Read Error>"); }
                                    }
                                },
                                RecordTag::FILE => {
                                     match reader.read_file_record(entry.offset) {
                                         Ok(file) => {
                                             let mut label = egui::RichText::new(&file.name);
                                             // Schema Check
                                             if file.name.ends_with(".dat") || file.name.ends_with(".datc64") || file.name.ends_with(".datl") || file.name.ends_with(".datl64") {
                                                 if let Some(s) = schema {
                                                     let stem = std::path::Path::new(&file.name).file_stem().and_then(|s| s.to_str());
                                                     if let Some(stem) = stem {
                                                          if !s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(stem)) {
                                                              label = label.color(egui::Color32::RED);
                                                          }
                                                     } else {
                                                          label = label.color(egui::Color32::RED);
                                                     }
                                                 }
                                             }

                                             if ui.button(label).clicked() {
                                                 *selected_file = Some(crate::ui::app::FileSelection::GgpkOffset(entry.offset));
                                             }
                                         },
                                         Err(_) => { ui.label("<Read Error>"); }
                                     }
                                },
                                _ => {}
                            }
                        }
                    },
                    Err(e) => {
                         ui.label(format!("Error reading directory: {}", e));
                    }
                }
            });
    }
}

