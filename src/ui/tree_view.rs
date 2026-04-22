use std::sync::Arc;
use eframe::egui;
use crate::bundles::index::Index;
use crate::ggpk::reader::GgpkReader;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

pub struct TreeView {
    reader: Option<Arc<GgpkReader>>,
    // Flattened Tree Storage
    nodes: Vec<FlatNode>,
    root_id: Option<usize>,

    search_term: String,
    active_search_term: String,
    
    // Search State
    search_tx: Option<Sender<(Vec<bool>, Vec<bool>, usize, u64)>>, 
    search_rx: Option<Receiver<(Vec<bool>, Vec<bool>, usize, u64)>>,
    matched_results: Vec<bool>, 
    matched_descendants: Vec<bool>,
    match_count: usize,
    is_searching: bool,
    search_generation: u64,
    // Display State
    render_limit: std::cell::Cell<usize>,
}

pub struct FlatNode {
    pub name: String,
    pub children: Vec<usize>,
    pub file_hash: Option<u64>,
    pub parent: Option<usize>,
}

pub enum TreeViewAction {
    None,
    Select,
    RequestExport { hashes: Vec<u64>, name: String, is_folder: bool, settings: Option<crate::ui::export_window::ExportSettings> },
}

impl Default for TreeView {
    fn default() -> Self {
        Self { 
            reader: None, 
            nodes: Vec::new(),
            root_id: None,
            search_term: String::new(), 
            active_search_term: String::new(),
            search_tx: None,
            search_rx: None,
            matched_results: Vec::new(),
            matched_descendants: Vec::new(),
            match_count: 0,
            is_searching: false,
            search_generation: 0,
            render_limit: std::cell::Cell::new(2000),
        }
    }
}

impl TreeView {
    pub fn is_searching(&self) -> bool {
        self.is_searching
    }

    pub fn new(reader: Arc<GgpkReader>) -> Self {
        Self { 
            reader: Some(reader), 
            nodes: Vec::new(),
            root_id: None, 
            ..Default::default()
        }
    }

    pub fn new_bundled(reader: Arc<GgpkReader>, index: &Index) -> Self {
        let nodes = Self::build_flat_tree(index);
        let root_id = if nodes.is_empty() { None } else { Some(0) };
        
        let (tx, rx) = channel();
        
        Self { 
            reader: Some(reader), 
            nodes,
            root_id,
            search_tx: Some(tx), 
            search_rx: Some(rx),
            ..Default::default()
        }
    }

    pub fn command_palette_items(&self, max_items: usize) -> Vec<crate::ui::command_palette::CommandPaletteItem> {
        if self.nodes.is_empty() {
            return Vec::new();
        }

        let mut items = Vec::new();
        for idx in 0..self.nodes.len() {
            let node = &self.nodes[idx];
            if let Some(hash) = node.file_hash {
                let label = self.node_path(idx);
                items.push(crate::ui::command_palette::CommandPaletteItem { label, hash });
                if items.len() >= max_items {
                    break;
                }
            }
        }
        items
    }

    fn node_path(&self, node_idx: usize) -> String {
        if node_idx >= self.nodes.len() {
            return String::new();
        }

        let mut parts = Vec::new();
        let mut current = Some(node_idx);

        while let Some(idx) = current {
            if idx >= self.nodes.len() {
                break;
            }
            let name = &self.nodes[idx].name;
            if idx != 0 && !name.is_empty() {
                parts.push(name.clone());
            }
            current = self.nodes[idx].parent;
        }

        parts.reverse();
        parts.join("/")
    }

    fn build_flat_tree(index: &Index) -> Vec<FlatNode> {
        let start = std::time::Instant::now();
        
        // 1. Collect all paths and hashes
        let mut paths: Vec<(&u64, &crate::bundles::index::FileInfo)> = index.files.iter().collect();
        
        // 2. Sort by path string (Alphabetical)
        // This ensures that for any directory, we encounter its children in order
        paths.sort_by(|a, b| a.1.path.cmp(&b.1.path));
        
        // 3. Build Tree Linearly
        let mut nodes = Vec::new();
        // Create Root
        nodes.push(FlatNode {
            name: "Bundles".to_string(),
            children: Vec::new(),
            file_hash: None,
            parent: None,
        });
        
        // Stack of active directories: (NodeIndex, PathDepth)
        // Starts with Root (0) at depth 0
        let mut stack: Vec<usize> = vec![0];
        // We need to track the current path components for the stack to know when to pop
        // Or we can just look at the next path.
        // Actually, since paths are sorted:
        // A/B/C
        // A/B/D
        
        // We can keep a "current path parts" stack?
        // Or for each new path, we effectively define the path from root.
        
        // Optimization: Reuse the stack.
        // For path "A/B/C":
        // 1. Check if stack[1] == "A". If not, pop stack until match.
        // 2. If stack[1] ok, check stack[2] == "B".
        
        // To do this, we need to store the name in the stack? Or looking up nodes[id].name?
        // nodes[id].name works.
        
        for (_hash, info) in paths {
            let mut parts = info.path.split('/').filter(|s| !s.is_empty()).peekable();
            
            // Find shared prefix length with current stack
            let mut depth = 0;
            let mut matched = true;
            
            while let Some(part) = parts.next() {
                if matched {
                    if depth < stack.len() - 1 {
                        let node_idx = stack[depth + 1];
                        if nodes[node_idx].name == part {
                            depth += 1;
                            continue;
                        }
                    }
                    // Mismatch found, pop stack and switch to add mode
                    while stack.len() > depth + 1 {
                        stack.pop();
                    }
                    matched = false;
                }
                
                // Add new node
                let is_file = parts.peek().is_none();
                let name = part.to_string();
                let parent_idx = *stack.last().unwrap();
                
                let new_idx = nodes.len();
                
                let hash = if is_file { Some(*_hash) } else { None };
                
                nodes.push(FlatNode {
                     name,
                     children: Vec::new(),
                     file_hash: hash,
                     parent: Some(parent_idx),
                });
                
                // Add to parent
                nodes[parent_idx].children.push(new_idx);
                
                if !is_file {
                    stack.push(new_idx);
                }
            }
            
            // Logic handled inside loop, but we need to ensure stack is popped if we matched everything but stack was deeper?
            // "depth" tracks how much we matched.
            // If we matched "A/B" but stack was "A/B/C", we need to pop C?
            // Yes.
            // But the iterator finishes. We need to cleanup stack.
            // The loop above only pops if it finds a mismatching *new* part.
            // If the path is shorter than stack, we exit loop but stack is deep.
            
            if matched {
                 while stack.len() > depth + 1 {
                     stack.pop();
                 }
            }
        }
        
        println!("Tree Build (Linear) took {:?}", start.elapsed());

        // 4. Sort Children (Folders First)
        // Iterating is safe because we just modify children vectors
        let sort_start = std::time::Instant::now();
        for i in 0..nodes.len() {
            if !nodes[i].children.is_empty() {
                // We need to sort indices based on the *nodes* they point to.
                // We can't move `nodes` out, but we can access it via slice if we use split_at_mut?
                // Or just clone the children, sort, and put back?
                // Or use a helper that takes &nodes.
                
                // Rust doesn't let us have mutable reference to nodes[i] AND immutable ref to nodes[child] easily inside the closure.
                // But we only need to generic sort the `children` Vec of `nodes[i]`.
                // We can copy the sort criteria (is_dir, name) into a list, sort that, and apply order.
                
                let mut criteria: Vec<(bool, String, usize)> = nodes[i].children.iter().map(|&c_idx| {
                    let c = &nodes[c_idx];
                    let is_dir = c.file_hash.is_none() || !c.children.is_empty(); // Logic: Folders have no hash usually, or have children
                    (is_dir, c.name.clone(), c_idx)
                }).collect();
                
                criteria.sort_by(|a, b| {
                    if a.0 == b.0 {
                        a.1.cmp(&b.1)
                    } else {
                        b.0.cmp(&a.0) // Dirs (true) first
                    }
                });
                
                nodes[i].children = criteria.into_iter().map(|c| c.2).collect();
            }
        }
        println!("Tree Sort took {:?}", sort_start.elapsed());
        
        nodes
    }

    pub fn show(&mut self, ui: &mut egui::Ui, selected_file: &mut Option<crate::ui::app::FileSelection>, schema: Option<&crate::dat::schema::Schema>) -> TreeViewAction {
        let mut action = TreeViewAction::None;
        let mut trigger_search = false;

        ui.label(
            egui::RichText::new("FILTER TREE")
                .monospace()
                .size(10.5)
                .color(egui::Color32::from_rgb(113, 113, 122)),
        );

        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search_term)
                    .id(ui.make_persistent_id("search_box"))
                    .hint_text(
                        egui::RichText::new("Search... (press enter)")
                            .color(egui::Color32::from_rgb(82, 82, 91)),
                    )
                    .desired_width(ui.available_width()),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                trigger_search = true;
            }
        });

        if self.is_searching {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Searching bundle index...")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(113, 113, 122)),
                );
            });
        } else if !self.active_search_term.is_empty() {
            ui.label(
                egui::RichText::new(format!("{} matches", self.match_count))
                    .size(11.0)
                    .color(egui::Color32::from_rgb(113, 113, 122)),
            );
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        // Handle Search Results (Async)
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

             if !term_lower.is_empty() {
                 if let Some(tx) = &self.search_tx {
                     // Prepare Search Data (Clone needed data to thread)
                     let tx = tx.clone();
                     let gen_id = self.search_generation;
                     
                     // We need a thread-safe copy of the index for searching.
                     // The FlatNode structure is basically the index. we can just clone the names and parents?
                     // Or just pass the nodes? `nodes` is owned by `self`.
                     // Cloning `nodes` is expensive (Memory).
                     // But we only need (id, name, parent_id).
                     // Let's create a lightweight search index on the fly or cached?
                     // For now, let's just clone the basic data: Vec<(bool, String, usize)> -> (is_file, name, parent)
                     
                     let search_data: Vec<(bool, String, usize)> = self.nodes.iter().enumerate().map(|(_i, n)| {
                         (n.file_hash.is_some(), n.name.to_lowercase(), n.parent.unwrap_or(0))
                     }).collect();

                     self.is_searching = true;
                     
                     thread::spawn(move || {
                         let start_time = std::time::Instant::now();
                         let max_id = search_data.len();
                         let mut results = vec![false; max_id];
                         let mut count = 0;
                         
                         for (id, (_is_file, name_lower, _)) in search_data.iter().enumerate() {
                             if name_lower.contains(&term_lower) {
                                 results[id] = true;
                                 count += 1;
                             }
                         }
                         
                         let mut descendants = vec![false; max_id];
                         for id in (1..max_id).rev() {
                             if results[id] || descendants[id] {
                                 let parent = search_data[id].2;
                                 if parent < descendants.len() {
                                     descendants[parent] = true;
                                 }
                             }
                         }

                         println!("Search Finished (Gen {}) in {:?}. Found {} matches.", gen_id, start_time.elapsed(), count);
                         let _ = tx.send((results, descendants, count, gen_id));
                     });
                 }
             }
        }
        
        if let Some(root_id) = self.root_id {
            if !self.nodes.is_empty() {
                let mut render_count = 0;
                for &child_idx in &self.nodes[root_id].children {
                    self.render_node(ui, child_idx, 0, selected_file, &mut action, schema, &mut render_count);
                    if render_count > 2000 {
                        break;
                    }
                }
            }
        } else if let Some(reader) = &self.reader {
            let root_offset = reader.root_offset;
            self.render_directory(ui, reader, root_offset, "Root", selected_file, schema);
        }
        
        action
    }

    fn render_node(&self, ui: &mut egui::Ui, node_idx: usize, _depth: usize, selected_file: &mut Option<crate::ui::app::FileSelection>, action: &mut TreeViewAction, schema: Option<&crate::dat::schema::Schema>, render_count: &mut usize) {
        if *render_count > 2000 {
            ui.label(egui::RichText::new("... Truncated (Too many items) ...").color(egui::Color32::YELLOW));
            return;
        }
        
        let node = &self.nodes[node_idx];

        // Search Filter
        if !self.active_search_term.is_empty() {
            let matches = if node_idx < self.matched_results.len() { self.matched_results[node_idx] } else { false };
            let has_matching_children = if node_idx < self.matched_descendants.len() { self.matched_descendants[node_idx] } else { false };
            
            if !matches && !has_matching_children {
                return;
            }
        }
        
        *render_count += 1;
        let use_filter_expand = !self.active_search_term.is_empty() && self.match_count < 500;

        if let Some(hash) = node.file_hash {
            let mut label = egui::RichText::new(&node.name);
            let is_selected = matches!(selected_file, Some(crate::ui::app::FileSelection::BundleFile(selected_hash)) if *selected_hash == hash);
            
            // Red Filename Logic
            if node.name.ends_with(".dat") || node.name.ends_with(".datc64") || node.name.ends_with(".datl") || node.name.ends_with(".datl64") {
                if let Some(s) = schema {
                    let stem = std::path::Path::new(&node.name).file_stem().and_then(|s| s.to_str());
                    // Check if table exists in schema
                    let in_schema = stem.map(|name| s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(name))).unwrap_or(false);
                    if !in_schema {
                        label = label.color(egui::Color32::RED);
                    }
                }
            }

            ui.push_id(hash, |ui| {
                let response = ui.selectable_label(is_selected, label);

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
            let node_path = self.node_path(node_idx);
            let is_selected = matches!(selected_file, Some(crate::ui::app::FileSelection::Folder { path, .. }) if path == &node_path);

            let open_state = if use_filter_expand { Some(true) } else { None };

            let response = egui::CollapsingHeader::new(
                egui::RichText::new(&node.name).color(if is_selected {
                    egui::Color32::from_rgb(236, 236, 240)
                } else {
                    egui::Color32::from_rgb(200, 200, 206)
                }),
            )
            .id_salt(ui.make_persistent_id(node_idx))
            .open(open_state)
            .show(ui, |ui| {
                for &child_idx in &node.children {
                    self.render_node(ui, child_idx, 0, selected_file, action, schema, render_count);
                    if *render_count > 2000 {
                        break;
                    }
                }
            });

            if response.header_response.clicked() {
                let mut hashes = Vec::new();
                self.collect_immediate_hashes(node_idx, &mut hashes);
                *selected_file = Some(crate::ui::app::FileSelection::Folder {
                    hashes,
                    name: node.name.clone(),
                    path: node_path,
                });
                *action = TreeViewAction::Select;
            }

            response.header_response.context_menu(|ui| {
                if ui.button("Export Folder...").clicked() {
                    let mut hashes = Vec::new();
                    self.collect_hashes(node_idx, &mut hashes);
                    *action = TreeViewAction::RequestExport { hashes, name: node.name.clone(), is_folder: true, settings: None };
                    ui.close_menu();
                }
            });
        }
    }

    fn collect_hashes(&self, node_idx: usize, hashes: &mut Vec<u64>) {
        if node_idx >= self.nodes.len() { return; }
        let node = &self.nodes[node_idx];
        if let Some(h) = node.file_hash {
            hashes.push(h);
        }
        for &child in &node.children {
            self.collect_hashes(child, hashes);
        }
    }

    fn collect_immediate_hashes(&self, node_idx: usize, hashes: &mut Vec<u64>) {
        if node_idx >= self.nodes.len() { return; }
        let node = &self.nodes[node_idx];
        for &child_idx in &node.children {
            let child = &self.nodes[child_idx];
            if let Some(h) = child.file_hash {
                hashes.push(h);
            }
        }
    }
    
    // Existing helper for raw view... can leave as is
    fn render_directory(&self, ui: &mut egui::Ui, reader: &GgpkReader, offset: u64, name: &str, selected_file: &mut Option<crate::ui::app::FileSelection>, schema: Option<&crate::dat::schema::Schema>) {
         // ... (Same as before, abbreviated here, but I must provide full content if replacing file?)
         // The prompt says "ReplacementContent" must be complete.
         // I will copy the previous logic for render_directory.
         
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
                                 b_is_dir.cmp(&a_is_dir) 
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
                                              if file.name.ends_with(".dat") || file.name.ends_with(".datc64") || file.name.ends_with(".datl") || file.name.ends_with(".datl64") {
                                                  if let Some(s) = schema {
                                                      let stem = std::path::Path::new(&file.name).file_stem().and_then(|s| s.to_str());
                                                      let in_schema = stem.map(|n| s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(n))).unwrap_or(false);
                                                      if !in_schema {
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

