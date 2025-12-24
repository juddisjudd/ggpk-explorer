use std::sync::Arc;
use eframe::egui;
use crate::bundles::index::Index;
use crate::ggpk::reader::GgpkReader;
use std::collections::HashMap;

pub struct TreeView {
    reader: Option<Arc<GgpkReader>>,
    bundle_root: Option<BundleNode>,
}

struct BundleNode {
    name: String,
    children: HashMap<String, BundleNode>,
    file_hash: Option<u64>,
}

impl Default for TreeView {
    fn default() -> Self {
        Self { reader: None, bundle_root: None }
    }
}

pub enum TreeViewAction {
    None,
    Select,
    ExportBundleFolder(Vec<u64>, String),
}

impl TreeView {
    pub fn new(reader: Arc<GgpkReader>) -> Self {
        Self { reader: Some(reader), bundle_root: None }
    }

    pub fn new_bundled(reader: Arc<GgpkReader>, index: &Index) -> Self {
        let root = Self::build_bundle_tree(index);
        Self { reader: Some(reader), bundle_root: Some(root) }
    }

    fn build_bundle_tree(index: &Index) -> BundleNode {
        let mut root = BundleNode {
            name: "Bundles".to_string(),
            children: HashMap::new(),
            file_hash: None,
        };
        
        for (hash, file) in &index.files {
            if file.path.is_empty() { continue; }
            
            let parts: Vec<&str> = file.path.split(|c| c == '/' || c == '\\').collect();
            let mut current = &mut root;
            
            for (i, part) in parts.iter().enumerate() {
                if i == parts.len() - 1 {
                    // File
                    current.children.insert(part.to_string(), BundleNode {
                        name: part.to_string(),
                        children: HashMap::new(),
                        file_hash: Some(*hash),
                    });
                } else {
                    // Directory
                    current = current.children.entry(part.to_string()).or_insert_with(|| BundleNode {
                        name: part.to_string(),
                        children: HashMap::new(),
                        file_hash: None,
                    });
                }
            }
        }
        root
    }
    
    pub fn show(&mut self, ui: &mut egui::Ui, selected_file: &mut Option<crate::ui::app::FileSelection>, schema: Option<&crate::dat::schema::Schema>) -> TreeViewAction {
        let mut action = TreeViewAction::None;
        
        if let Some(root) = &self.bundle_root {
            self.render_bundle_node(ui, root, selected_file, &mut action, schema);
        } else if let Some(reader) = &self.reader {
            let root_offset = reader.root_offset;
            self.render_directory(ui, reader, root_offset, "Root", selected_file, schema);
        }
        
        action
    }

    fn render_bundle_node(&self, ui: &mut egui::Ui, node: &BundleNode, selected_file: &mut Option<crate::ui::app::FileSelection>, action: &mut TreeViewAction, schema: Option<&crate::dat::schema::Schema>) {
        if let Some(hash) = node.file_hash {
            let mut label = egui::RichText::new(&node.name);
            
            // Check schema if .dat file
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

            if ui.button(label).clicked() {
                 *selected_file = Some(crate::ui::app::FileSelection::BundleFile(hash));
                 *action = TreeViewAction::Select;
            }
        } else {
            let id = ui.make_persistent_id(&node.name).with(&node.children.len()); 
            let header = egui::CollapsingHeader::new(&node.name)
                .id_salt(id);
                
                let response = header.show(ui, |ui| {
                    let mut children: Vec<&BundleNode> = node.children.values().collect();
                    children.sort_by(|a, b| {
                        let a_is_dir = a.file_hash.is_none();
                        let b_is_dir = b.file_hash.is_none();
                        if a_is_dir != b_is_dir {
                            b_is_dir.cmp(&a_is_dir) // True (Dir) > False (File)
                        } else {
                            a.name.cmp(&b.name)
                        }
                    });

                    for child in children {
                        self.render_bundle_node(ui, child, selected_file, action, schema);
                    }
                });
                
            response.header_response.context_menu(|ui| {
                if ui.button("Export Folder...").clicked() {
                    let mut hashes = Vec::new();
                    self.collect_hashes(node, &mut hashes);
                    *action = TreeViewAction::ExportBundleFolder(hashes, node.name.clone());
                    ui.close_menu();
                }
            });
        }
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
                        
                        // Collect valid entries with headers
                        let mut valid_entries = Vec::new();
                        for entry in dir.entries {
                             if let Ok(header) = reader.read_record_header(entry.offset) {
                                  valid_entries.push((entry, header));
                             }
                        }
                        
                        // Sort: PDIR first, then Name (we don't have name handy easily without reading record? Wait, file name is in file record...)
                        // PDIR name is in PDIR record.
                        // We can sort by TAG primarily. PDIR < FILE?
                        // If we want alphabetical within type, we need to read the full record.
                        // Let's settle for Type sorting first to match user request "Directories should always be first".
                        // Sorting by name within type is implicit if the directory list was already sorted?
                        // GGPK entries might be hash ordered.
                        // To sort by name, we'd need to read the names.
                        
                        // For now, let's sort by TAG: PDIR (Dir) < FILE (File).
                        // RecordTag enum usually has PDIR=... FILE=...
                        // Let's assume we want PDIR first.
                        valid_entries.sort_by(|a, b| {
                            let tag_a = a.1.tag;
                            let tag_b = b.1.tag;
                            
                            let a_is_dir = matches!(tag_a, RecordTag::PDIR);
                            let b_is_dir = matches!(tag_b, RecordTag::PDIR);
                            
                            if a_is_dir != b_is_dir {
                                b_is_dir.cmp(&a_is_dir) // True > False
                            } else {
                                // Fallback to offset if we can't read name easily without potentially expensive reads
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
                         // Different error handling in original? "Err(_) => {" vs "Err(e) => {"
                         // Original line 220: "Err(_) => {"
                         // NO, I read "Err(_) => { ui.label... }" at line 220. 
                         // Check line 220 in viewed file 1293.
                         // Line 220: "Err(_) => {"
                         // Wait, I am replacing lines 151 to 214?
                         // Line 214 corresponds to `Err(_) => { ui.label("<Read Error>"); }` inside FILE match.
                         // Line 220 is the error arm for `read_directory`.
                         // I am NOT replacing line 220.
                         // My replacement content ends with closing brace for FILE match `}`.
                         // And then `_ => {}` and `}` loop end. 
                         // My replacement content is FULL function body?
                         // No, my replacement starts at line 151 (function signature).
                         // Ends at line 214?
                         // Wait, `read_directory` has nested match.
                         // The structure is large.
                         // I should replace the WHOLE function.
                         // I need to see where the function ends.
                         ui.label(format!("Error reading directory: {}", e));
                    }
                }
            });
    }
}
