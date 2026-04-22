use crate::ggpk::reader::GgpkReader;
use crate::dat::reader::DatReader;
use crate::dat::schema::Schema;
use eframe::egui;
use serde_json;
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct DatViewer {
    pub schema: Option<Schema>,
    pub schema_date: String,
    pub reader: Option<DatReader>,
    pub request_update_schema: bool,
    pub error_msg: Option<String>,
    pub row_cache: LruCache<u32, Vec<crate::dat::reader::DatValue>>,
}

impl Default for DatViewer {
    fn default() -> Self {
        Self {
            schema: None,
            schema_date: "Unknown".to_string(),
            reader: None,
            request_update_schema: false,
            error_msg: None,
            row_cache: LruCache::new(NonZeroUsize::new(5000).unwrap()),
        }
    }
}

impl DatViewer {
    pub fn loaded_filename(&self) -> Option<&str> {
        self.reader.as_ref().map(|r| r.filename.as_str())
    }

    pub fn set_schema(&mut self, schema: Schema, date: String) {
        self.schema = Some(schema);
        self.schema_date = date;
    }

    pub fn load(&mut self, reader: &GgpkReader, offset: u64) {
        // Read file content
        match reader.read_file_record(offset) {
             Ok(file) => {
                 match reader.get_data_slice(file.data_offset, file.data_length) {
                      Ok(data) => {
                          self.load_from_bytes(data.to_vec(), &file.name);
                      },
                      Err(e) => { self.error_msg = Some(format!("Read Slice Error: {}", e)); }
                 }
             },
             Err(e) => { self.error_msg = Some(format!("Read Record Error: {}", e)); }
        }
    }

    pub fn load_from_bytes(&mut self, data: Vec<u8>, filename: &str) {
        self.error_msg = None;
        self.row_cache.clear();
        match DatReader::new(data, filename) {
            Ok(dat_reader) => {
                println!("Successfully loaded DAT: {}", filename);
                self.reader = Some(dat_reader);
            },
            Err(e) => { 
                let msg = format!("Failed to create DatReader for {}: {}", filename, e);
                println!("{}", msg);
                self.error_msg = Some(msg);
                self.reader = None; 
            }
        }
    }
    
    pub fn show(&mut self, ui: &mut egui::Ui, is_poe2: bool) {
         if let Some(err) = &self.error_msg {
             ui.colored_label(egui::Color32::RED, err);
         }
         
         if self.reader.is_none() {
             ui.label("No Dat loaded");
             return;
         }
         
         ui.label("Dat Viewer (WIP)");
         ui.label(if is_poe2 { "Schema Mode: Path of Exile 2" } else { "Schema Mode: Path of Exile 1" });
         if self.schema.is_some() {
             ui.horizontal(|ui| {
                 ui.label(egui::RichText::new("Schema: Loaded").color(egui::Color32::GREEN));
                 ui.label(format!("(Updated: {})", self.schema_date));
             });
         } else {
             ui.colored_label(egui::Color32::RED, "Schema: Not Loaded");
         }
         
         if let Some(reader) = &self.reader {
             ui.label(format!("Rows: {}", reader.row_count));
         }
         
         if let Some(schema) = &self.schema {
             if let Some(reader) = &self.reader {
                 // Match table name (insensitive) and pick best valid_for
                 let path = std::path::Path::new(&reader.filename);
                 let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                 
                 let mut candidates: Vec<_> = schema.tables.iter()
                    .filter(|t| t.name.eq_ignore_ascii_case(&stem))
                    .collect();
                 
                 // Sort by validFor descending (assuming higher version = newer/more specific)
                 candidates.sort_by(|a, b| b.valid_for.unwrap_or(0).cmp(&a.valid_for.unwrap_or(0)));
                 
                 let table = candidates.first().map(|&t| t);
                 
                 if let Some(table) = table {
                 ui.horizontal(|ui| {
                     ui.label(format!("Table: {} (ver: {})", table.name, table.valid_for.unwrap_or(0)));
                 });

                 use egui_extras::{TableBuilder, Column};
                 
                 egui::ScrollArea::horizontal().show(ui, |ui| {
                     TableBuilder::new(ui)
                         .striped(true)
                         .resizable(true)
                         .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                         .column(Column::initial(60.0).resizable(true)) // Index
                         .columns(Column::initial(150.0).resizable(true).clip(true), table.columns.len())
                         .min_scrolled_height(0.0)
                         .header(20.0, |mut header| {
                             header.col(|ui| { ui.strong("Index"); });
                             for col in &table.columns {
                                 let name = col.name.as_deref().unwrap_or("?");
                                 header.col(|ui| { ui.strong(name).on_hover_text(format!("Type: {}\nArray: {}", col.r#type, col.array)); });
                             }
                         })
                         .body(|body| {
                             if let Some(reader) = &self.reader {
                                 body.rows(20.0, reader.row_count as usize, |mut row| {
                                     let row_index = row.index();
                                     row.col(|ui| { ui.label(row_index.to_string()); });
                                     
                                     // Check cache first
                                     let values = if let Some(cached) = self.row_cache.get(&(row_index as u32)) {
                                         Some(cached.clone())
                                     } else {
                                         // Read and cache
                                         match reader.read_row(row_index as u32, table) {
                                             Ok(v) => {
                                                 self.row_cache.put(row_index as u32, v.clone());
                                                 Some(v)
                                             },
                                             Err(_) => None
                                         }
                                     };

                                     if let Some(values) = values {
                                             for (col_idx, val) in values.iter().enumerate() {
                                                 row.col(|ui| {
                                                     match val {
                                                         crate::dat::reader::DatValue::Bool(b) => { ui.label(b.to_string()); },
                                                         crate::dat::reader::DatValue::Int(i) => { ui.label(i.to_string()); },
                                                         crate::dat::reader::DatValue::Long(l) => { ui.label(l.to_string()); },
                                                         crate::dat::reader::DatValue::Float(f) => { ui.label(f.to_string()); },
                                                         crate::dat::reader::DatValue::String(s) => { 
                                                             ui.label(s).on_hover_text(s); 
                                                         },
                                                         crate::dat::reader::DatValue::ForeignRow(idx) => { 
                                                             if ui.link(format!("Row {}", idx)).clicked() {
                                                                 // TODO: Navigate
                                                             }
                                                         },
                                                         crate::dat::reader::DatValue::List(count, offset) => {
                                                             if *count > 0 {
                                                                 ui.menu_button(format!("List({})", count), |ui| {
                                                                      ui.set_max_height(200.0);
                                                                      egui::ScrollArea::vertical().show(ui, |ui| {
                                                                          let col_def = &table.columns[col_idx];
                                                                          match reader.read_list_values(*offset, *count, col_def) {
                                                                              Ok(items) => {
                                                                                  for (i, item) in items.iter().enumerate() {
                                                                                      ui.label(format!("{}: {:?}", i, item));
                                                                                  }
                                                                              },
                                                                              Err(e) => { ui.colored_label(egui::Color32::RED, format!("Error: {}", e)); }
                                                                          }
                                                                      });
                                                                 });
                                                             } else {
                                                                 ui.label("[]");
                                                             }
                                                         },
                                                         crate::dat::reader::DatValue::Unknown => { 
                                                             ui.label("?"); 
                                                         },
    
                                                     }
                                                 });
                                             }
                                     } else {
                                              for _ in 0..table.columns.len() {
                                                  row.col(|ui| { ui.label("ERR"); });
                                              }
                                     }
                                 });
                             }
                         });
                 });
                 } else {
                     ui.label(format!("Table not found for file: {}", reader.filename));
                     self.show_generic_view(ui, reader);
                 }
             } else {
                 if let Some(reader) = &self.reader {
                     self.show_generic_view(ui, reader);
                 }
             }
         } else {
              if let Some(reader) = &self.reader {
                  self.show_generic_view(ui, reader);
              }
         }
         
         ui.separator();
         ui.horizontal(|ui| {
             if ui.button("Update Schema from Web").clicked() {
                self.request_update_schema = true;
             }
             if ui.button("Debug Match").clicked() {
                 if let Some(schema) = &self.schema {
                     println!("DEBUG: Schema Loaded. Tables: {}", schema.tables.len());
                     if let Some(reader) = &self.reader {
                         let path = std::path::Path::new(&reader.filename);
                         let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                         println!("DEBUG: Current File Stem: '{}'", stem);
                         let match_res = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&stem));
                         if let Some(m) = match_res {
                             println!("DEBUG: Match Found: {}", m.name);
                         } else {
                             println!("DEBUG: No match found.");
                             // Print first 10 tables
                             println!("DEBUG: First 10 tables in schema:");
                             for t in schema.tables.iter().take(10) {
                                 println!(" - {}", t.name);
                             }
                         }
                     }
                 } else {
                     println!("DEBUG: Schema NOT Loaded.");
                 }
             }
         });
    }



    fn _export_json(&self, table: &crate::dat::schema::Table) {
        if let Some(reader) = &self.reader {
             if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.json", table.name)).save_file() {
                 let mut all_rows = Vec::new();
                 for i in 0..reader.row_count {
                     if let Ok(values) = reader.read_row(i, table) {
                         let mut row_map = std::collections::HashMap::new();
                         row_map.insert("Index".to_string(), serde_json::to_value(i).unwrap());
                         
                         for (j, col) in table.columns.iter().enumerate() {
                             if let Some(val) = values.get(j) {
                                 let key = col.name.clone().unwrap_or_else(|| format!("Col{}", j));
                                 row_map.insert(key, serde_json::to_value(val).unwrap_or(serde_json::Value::Null));
                             }
                         }
                         all_rows.push(row_map);
                     }
                 }
                 
                 let f = std::fs::File::create(path).ok();
                 if let Some(f) = f {
                     let _ = serde_json::to_writer_pretty(f, &all_rows);
                 }
             }
        }
    }

    pub fn show_generic_view(&self, ui: &mut egui::Ui, reader: &DatReader) {
        ui.label("Generic View (No Schema / Unknown Table)");
        if let Some(row_len) = reader.row_length {
             // Treat as raw bytes column? Or try to split into 4-byte ints?
             // Let's just show raw bytes for now, maybe in chunks of 8
             
             use egui_extras::{TableBuilder, Column};
             
             let num_cols = (row_len + 7) / 8; // 8 bytes per visual column
             
             TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(60.0).resizable(true)) // Index
                .columns(Column::initial(150.0).resizable(true), num_cols)
                .min_scrolled_height(0.0)
                .header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Index"); });
                    for i in 0..num_cols {
                        header.col(|ui| { ui.strong(format!("Bytes {}-{}", i*8, (i+1)*8)); });
                    }
                })
                .body(|body| {
                     body.rows(20.0, reader.row_count as usize, |mut row| {
                         let row_index = row.index();
                         row.col(|ui| { ui.label(row_index.to_string()); });
                         
                         // Read raw row
                         let start = 4 + (row_index * row_len);
                         if start + row_len <= reader.get_data().len() {
                             let row_data = &reader.get_data()[start..start+row_len];
                             
                             for i in 0..num_cols {
                                 row.col(|ui| {
                                     let s = i*8;
                                     let e = std::cmp::min(s+8, row_len);
                                     if s < e {
                                         let chunk = &row_data[s..e];
                                         let hex: Vec<String> = chunk.iter().map(|b| format!("{:02X}", b)).collect();
                                         ui.label(hex.join(" "));
                                     }
                                 });
                             }
                         }
                     });
                });
        } else {
            ui.label("Unknown Row Length (Cannot display table)");
        }
    }

    #[allow(dead_code)]
    pub fn convert_to_json(&self, data: &[u8], filename: &str) -> Option<String> {
        let path = std::path::Path::new(filename);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        
        if let Some(schema) = &self.schema {
            if let Some(table) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&stem)) {
                if let Ok(reader) = DatReader::new(data.to_vec(), filename) {
                     let mut all_rows = Vec::new();
                     for i in 0..reader.row_count {
                         if let Ok(values) = reader.read_row(i, table) {
                             let mut row_map = std::collections::BTreeMap::new();
                             
                             for (j, col) in table.columns.iter().enumerate() {
                                 if let Some(val) = values.get(j) {
                                     let key = col.name.clone().unwrap_or_else(|| format!("Col{}", j));
                                     if let Ok(v) = serde_json::to_value(val) {
                                          row_map.insert(key, v);
                                     }
                                 }
                             }
                             all_rows.push(row_map);
                         }
                     }
                     return serde_json::to_string_pretty(&all_rows).ok();
                }
            }
        }
        None
    }
}
