use crate::dat::reader::{DatReader, DatValue};
use crate::dat::schema::{Column, Schema, Table, SUPPORTED_SCHEMA_VERSION};
use crate::ggpk::reader::GgpkReader;
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column as TCol, TableBuilder};
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Loads the bytes of another virtual file by path; used to resolve foreign keys.
pub type TableLoader<'l> = dyn FnMut(&str) -> Option<Vec<u8>> + 'l;

#[derive(Debug, Clone)]
pub enum DatNavRequest {
    Table { path: String, row: Option<u32> },
    File(String),
}

struct RelatedTable {
    reader: DatReader,
    table: Arc<Table>,
    display_col: Option<usize>,
}

const ROW_H: f32 = 20.0;
const INLINE_LIST_ITEMS: usize = 6;
const DETAIL_LIST_ITEMS: usize = 500;
const NULL_ROW: usize = usize::MAX;

pub struct DatViewer {
    pub schema: Option<Schema>,
    pub schema_date: String,
    pub reader: Option<DatReader>,
    pub request_update_schema: bool,
    pub error_msg: Option<String>,
    pub row_cache: LruCache<u32, Vec<DatValue>>,
    pub nav_request: Option<DatNavRequest>,
    /// Row to select once the next table finishes loading (foreign-key navigation).
    pub pending_scroll_row: Option<u32>,
    schema_gen: u64,
    table: Option<(u64, bool, Option<Arc<Table>>)>,
    filter: String,
    sort: Option<(usize, bool)>,
    view_rows: Option<Vec<u32>>,
    all_rows: Option<Vec<Vec<DatValue>>>,
    row_text: Option<Vec<String>>,
    selected_row: Option<u32>,
    show_detail: bool,
    hidden_cols: HashSet<usize>,
    scroll_to_row: Option<u32>,
    related: HashMap<String, Option<Arc<RelatedTable>>>,
    /// Column layout guessed from the data when the schema has no entry for this table.
    guessed: Option<Arc<Table>>,
    byte_view: bool,
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
            nav_request: None,
            pending_scroll_row: None,
            schema_gen: 0,
            table: None,
            filter: String::new(),
            sort: None,
            view_rows: None,
            all_rows: None,
            row_text: None,
            selected_row: None,
            show_detail: false,
            hidden_cols: HashSet::new(),
            scroll_to_row: None,
            related: HashMap::new(),
            guessed: None,
            byte_view: false,
        }
    }
}

struct CellOut {
    nav: Option<DatNavRequest>,
    scroll_to: Option<u32>,
    select: Option<u32>,
}

/// Resolves foreign-key targets lazily through the loader; failures are remembered so
/// a missing table is only attempted once per loaded file.
struct RelCtx<'a, 'l> {
    schema: &'a Schema,
    related: &'a mut HashMap<String, Option<Arc<RelatedTable>>>,
    loader: Option<&'a mut TableLoader<'l>>,
    base_dir: String,
    ext: String,
    is_poe2: bool,
}

impl<'a, 'l> RelCtx<'a, 'l> {
    fn path_for(&self, table: &str) -> String {
        if self.base_dir.is_empty() {
            format!("{}.{}", table.to_lowercase(), self.ext)
        } else {
            format!("{}/{}.{}", self.base_dir, table.to_lowercase(), self.ext)
        }
    }

    fn get(&mut self, table: &str) -> Option<Arc<RelatedTable>> {
        let key = table.to_ascii_lowercase();
        if let Some(v) = self.related.get(&key) {
            return v.clone();
        }
        let path = self.path_for(table);
        let tdef = self.schema.find_table(table, self.is_poe2).cloned();
        let loaded = match (self.loader.as_mut(), tdef) {
            (Some(loader), Some(tdef)) => loader(&path)
                .and_then(|bytes| DatReader::new(bytes, &path).ok())
                .map(|reader| {
                    let display_col = pick_display_col(&tdef);
                    Arc::new(RelatedTable { reader, table: Arc::new(tdef), display_col })
                }),
            _ => None,
        };
        self.related.insert(key, loaded.clone());
        loaded
    }

    fn display(&mut self, table: &str, row: usize) -> Option<String> {
        let rt = self.get(table)?;
        let col = rt.display_col?;
        if row as u32 >= rt.reader.row_count {
            return None;
        }
        let vals = rt.reader.read_row(row as u32, &rt.table).ok()?;
        match vals.get(col)? {
            DatValue::String(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    fn enum_label(&self, enum_name: &str, value: i64) -> Option<String> {
        self.schema
            .find_enumeration(enum_name, self.is_poe2)
            .and_then(|e| e.label(value))
            .map(|s| s.to_string())
    }
}

fn pick_display_col(table: &Table) -> Option<usize> {
    let by_name = |n: &str| {
        table.columns.iter().position(|c| {
            !c.array && c.r#type == "string" && c.name.as_deref().map(|x| x.eq_ignore_ascii_case(n)).unwrap_or(false)
        })
    };
    by_name("Id")
        .or_else(|| by_name("Name"))
        .or_else(|| table.columns.iter().position(|c| !c.array && c.r#type == "string" && c.name.is_some()))
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn col_name(col: &Column, idx: usize) -> String {
    col.name.clone().unwrap_or_else(|| format!("_{}", idx))
}

fn col_width(col: &Column) -> f32 {
    if col.array {
        return 150.0;
    }
    match col.r#type.as_str() {
        "bool" => 52.0,
        "string" => 200.0,
        "foreignrow" | "row" | "rid" => 160.0,
        "enumrow" => 130.0,
        "f32" | "float" => 84.0,
        _ => 72.0,
    }
}

fn scalar_text(val: &DatValue) -> String {
    match val {
        DatValue::Bool(b) => b.to_string(),
        DatValue::Int(i) => i.to_string(),
        DatValue::Long(l) => l.to_string(),
        DatValue::Float(f) => format_float(*f),
        DatValue::String(s) => s.clone(),
        DatValue::ForeignRow(NULL_ROW) => "null".to_string(),
        DatValue::ForeignRow(k) => k.to_string(),
        DatValue::List(count, _) => format!("[{} items]", count),
        DatValue::Interval(a, b) => format!("{}..{}", scalar_text(a), scalar_text(b)),
        DatValue::Unknown => "?".to_string(),
    }
}

fn format_float(f: f32) -> String {
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s == "NaN" || s.contains("inf") { s } else { format!("{}.0", s) }
}

#[derive(PartialEq)]
enum SortKey {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering::*;
        let rank = |k: &SortKey| match k {
            SortKey::Null => 0,
            SortKey::Bool(_) => 1,
            SortKey::Num(_) => 2,
            SortKey::Str(_) => 3,
        };
        match (self, other) {
            (SortKey::Bool(a), SortKey::Bool(b)) => a.partial_cmp(b),
            (SortKey::Num(a), SortKey::Num(b)) => a.partial_cmp(b).or(Some(Equal)),
            (SortKey::Str(a), SortKey::Str(b)) => Some(a.cmp(b)),
            _ => Some(rank(self).cmp(&rank(other))),
        }
    }
}

fn sort_key(val: &DatValue) -> SortKey {
    match val {
        DatValue::Bool(b) => SortKey::Bool(*b),
        DatValue::Int(i) => SortKey::Num(*i as f64),
        DatValue::Long(l) => SortKey::Num(*l as f64),
        DatValue::Float(f) => SortKey::Num(*f as f64),
        DatValue::String(s) => {
            if s.is_empty() { SortKey::Null } else { SortKey::Str(s.to_lowercase()) }
        }
        DatValue::ForeignRow(NULL_ROW) => SortKey::Null,
        DatValue::ForeignRow(k) => SortKey::Num(*k as f64),
        DatValue::List(count, _) => SortKey::Num(*count as f64),
        DatValue::Interval(a, _) => sort_key(a),
        DatValue::Unknown => SortKey::Null,
    }
}

fn row_search_text(reader: &DatReader, vals: &[DatValue], table: &Table) -> String {
    let mut out = String::new();
    for (i, v) in vals.iter().enumerate() {
        match v {
            DatValue::List(count, offset) if *count > 0 => {
                if let Some(col) = table.columns.get(i) {
                    if let Ok(items) = reader.read_list_values(*offset, (*count).min(16), col) {
                        for item in items {
                            out.push_str(&scalar_text(&item));
                            out.push('\u{1f}');
                        }
                    }
                }
            }
            _ => out.push_str(&scalar_text(v)),
        }
        out.push('\u{1f}');
    }
    out.to_lowercase()
}

fn elem_column(col: &Column) -> Column {
    Column { array: false, interval: false, ..col.clone() }
}

fn number_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(209, 154, 102) } else { Color32::from_rgb(180, 83, 9) }
}

fn bool_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(86, 182, 194) } else { Color32::from_rgb(13, 116, 124) }
}

fn null_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(113, 113, 122) } else { Color32::from_rgb(140, 140, 150) }
}

/// Adds a right-click "Copy" menu to a cell response.
fn copy_menu(resp: egui::Response, text: impl Fn() -> String) -> egui::Response {
    resp.context_menu(|ui| {
        if ui.button("Copy value").clicked() {
            ui.ctx().copy_text(text());
            ui.close_menu();
        }
    });
    resp
}

#[allow(clippy::too_many_arguments)]
fn show_value(
    ui: &mut egui::Ui,
    val: &DatValue,
    col: &Column,
    reader: &DatReader,
    rel: &mut RelCtx,
    current_table: &str,
    expanded: bool,
    out: &mut CellOut,
) {
    match val {
        DatValue::Bool(b) => {
            let r = ui.add(egui::Label::new(RichText::new(b.to_string()).color(bool_color(ui))).sense(egui::Sense::click()));
            copy_menu(r, || b.to_string());
        }
        DatValue::Int(i) => {
            let label = if col.r#type == "enumrow" {
                col.references.as_ref().and_then(|r| rel.enum_label(&r.table, *i))
            } else {
                None
            };
            let text = match &label {
                Some(l) => format!("{} ({})", l, i),
                None => i.to_string(),
            };
            let r = ui.add(egui::Label::new(RichText::new(&text).color(number_color(ui))).sense(egui::Sense::click()));
            let r = if let Some(refr) = col.references.as_ref().filter(|_| col.r#type == "enumrow") {
                r.on_hover_text(format!("enum {}", refr.table))
            } else {
                r
            };
            copy_menu(r, || text.clone());
        }
        DatValue::Long(l) => {
            let r = ui.add(egui::Label::new(RichText::new(l.to_string()).color(number_color(ui))).sense(egui::Sense::click()));
            copy_menu(r, || l.to_string());
        }
        DatValue::Float(f) => {
            let text = format_float(*f);
            let r = ui.add(egui::Label::new(RichText::new(&text).color(number_color(ui))).sense(egui::Sense::click()));
            copy_menu(r, || text.clone());
        }
        DatValue::String(s) => {
            if s.is_empty() {
                ui.label(RichText::new("empty").color(null_color(ui)).italics());
            } else if col.file.is_some() || col.files.is_some() {
                let r = ui.link(s.as_str()).on_hover_text("Open file");
                if r.clicked() {
                    let mut path = s.replace('\\', "/");
                    if let Some(ext) = &col.file {
                        if !path.to_lowercase().ends_with(&ext.to_lowercase()) {
                            path.push_str(ext);
                        }
                    }
                    out.nav = Some(DatNavRequest::File(path));
                }
                copy_menu(r, || s.clone());
            } else {
                let r = ui.add(egui::Label::new(s.as_str()).truncate().sense(egui::Sense::click()));
                let r = if !expanded { r.on_hover_text(s.as_str()) } else { r };
                copy_menu(r, || s.clone());
            }
        }
        DatValue::ForeignRow(NULL_ROW) => {
            ui.label(RichText::new("null").color(null_color(ui)).italics());
        }
        DatValue::ForeignRow(k) => {
            let target = col.references.as_ref().map(|r| r.table.clone());
            let is_foreign = col.r#type == "foreignrow"
                && target.as_deref().map(|t| !t.eq_ignore_ascii_case(current_table)).unwrap_or(false);
            if is_foreign {
                let t = target.unwrap();
                let display = rel.display(&t, *k);
                let text = match &display {
                    Some(d) => d.clone(),
                    None => format!("{}#{}", t, k),
                };
                let r = ui.link(text.as_str()).on_hover_text(format!("{} row {}", t, k));
                if r.clicked() {
                    out.nav = Some(DatNavRequest::Table { path: rel.path_for(&t), row: Some(*k as u32) });
                }
                copy_menu(r, || text.clone());
            } else {
                let r = ui.link(format!("#{}", k)).on_hover_text("Row in this table");
                if r.clicked() {
                    out.scroll_to = Some(*k as u32);
                    out.select = Some(*k as u32);
                }
                copy_menu(r, || k.to_string());
            }
        }
        DatValue::List(count, offset) => {
            if *count == 0 {
                ui.label(RichText::new("[]").color(null_color(ui)));
                return;
            }
            let limit = if expanded { DETAIL_LIST_ITEMS } else { INLINE_LIST_ITEMS };
            let items = reader.read_list_values(*offset, (*count).min(limit), col).unwrap_or_default();
            let elem = elem_column(col);
            if expanded {
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{} items", count)).color(null_color(ui)));
                    for (i, item) in items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}:", i)).color(null_color(ui)).monospace());
                            show_value(ui, item, &elem, reader, rel, current_table, true, out);
                        });
                    }
                    if *count > items.len() {
                        ui.label(RichText::new(format!("… {} more", count - items.len())).color(null_color(ui)));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(RichText::new("[").color(null_color(ui)));
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            ui.label(RichText::new(",").color(null_color(ui)));
                        }
                        show_value(ui, item, &elem, reader, rel, current_table, false, out);
                    }
                    if *count > items.len() {
                        ui.label(RichText::new(format!("… +{}", count - items.len())).color(null_color(ui)));
                    }
                    ui.label(RichText::new("]").color(null_color(ui)));
                });
            }
        }
        DatValue::Interval(a, b) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let elem = elem_column(col);
                show_value(ui, a, &elem, reader, rel, current_table, expanded, out);
                ui.label(RichText::new("..").color(null_color(ui)));
                show_value(ui, b, &elem, reader, rel, current_table, expanded, out);
            });
        }
        DatValue::Unknown => {
            ui.label(RichText::new("?").color(null_color(ui)));
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
        self.schema_gen += 1;
        self.table = None;
        self.related.clear();
        self.row_cache.clear();
        self.invalidate_view();
    }

    pub fn schema_version_warning(&self) -> Option<String> {
        let schema = self.schema.as_ref()?;
        (schema.version != SUPPORTED_SCHEMA_VERSION).then(|| {
            format!(
                "Schema format v{} is newer than this build understands (v{}). Tables may not decode correctly; check for an app update.",
                schema.version, SUPPORTED_SCHEMA_VERSION
            )
        })
    }

    pub fn load(&mut self, reader: &GgpkReader, offset: u64) {
        match reader.read_file_record(offset) {
            Ok(file) => match reader.get_data_slice(file.data_offset, file.data_length) {
                Ok(data) => self.load_from_bytes(data.to_vec(), &file.name),
                Err(e) => self.error_msg = Some(format!("Read Slice Error: {}", e)),
            },
            Err(e) => self.error_msg = Some(format!("Read Record Error: {}", e)),
        }
    }

    pub fn load_from_bytes(&mut self, data: Vec<u8>, filename: &str) {
        self.error_msg = None;
        self.row_cache.clear();
        self.table = None;
        self.filter.clear();
        self.sort = None;
        self.invalidate_view();
        self.hidden_cols.clear();
        self.related.clear();
        self.guessed = None;
        self.scroll_to_row = self.pending_scroll_row.take();
        self.selected_row = self.scroll_to_row;
        match DatReader::new(data, filename) {
            Ok(dat_reader) => self.reader = Some(dat_reader),
            Err(e) => {
                self.error_msg = Some(format!("Failed to create DatReader for {}: {}", filename, e));
                self.reader = None;
            }
        }
    }

    fn invalidate_view(&mut self) {
        self.view_rows = None;
        self.all_rows = None;
        self.row_text = None;
    }

    fn ensure_table(&mut self, is_poe2: bool) {
        if let Some((gen, poe2, _)) = &self.table {
            if *gen == self.schema_gen && *poe2 == is_poe2 {
                return;
            }
        }
        let table = match (&self.schema, &self.reader) {
            (Some(schema), Some(reader)) => schema.find_table(&file_stem(&reader.filename), is_poe2).cloned().map(Arc::new),
            _ => None,
        };
        self.table = Some((self.schema_gen, is_poe2, table));
    }

    fn materialize(&mut self, table: &Table) {
        if self.all_rows.is_some() {
            return;
        }
        let Some(reader) = &self.reader else { return };
        let rows: Vec<Vec<DatValue>> = (0..reader.row_count)
            .map(|i| reader.read_row(i, table).unwrap_or_default())
            .collect();
        let text: Vec<String> = rows.iter().map(|r| row_search_text(reader, r, table)).collect();
        self.all_rows = Some(rows);
        self.row_text = Some(text);
    }

    fn rebuild_view(&mut self, table: &Table) {
        self.materialize(table);
        let (Some(all), Some(text)) = (&self.all_rows, &self.row_text) else { return };
        let needle = self.filter.trim().to_lowercase();
        let mut rows: Vec<u32> = (0..all.len() as u32)
            .filter(|&i| needle.is_empty() || text[i as usize].contains(&needle))
            .collect();
        if let Some((col, asc)) = self.sort {
            let keys: Vec<SortKey> = all.iter().map(|r| r.get(col).map(sort_key).unwrap_or(SortKey::Null)).collect();
            rows.sort_by(|&a, &b| {
                let ord = keys[a as usize].partial_cmp(&keys[b as usize]).unwrap_or(std::cmp::Ordering::Equal);
                if asc { ord } else { ord.reverse() }
            });
        }
        self.view_rows = Some(rows);
    }

    /// `loader` resolves other tables by virtual path (foreign keys); pass `None` when
    /// browsing a loose file with no index available.
    pub fn show(&mut self, ui: &mut egui::Ui, is_poe2: bool, loader: Option<&mut TableLoader<'_>>) {
        if let Some(err) = &self.error_msg {
            ui.colored_label(Color32::from_rgb(239, 68, 68), err);
            if let Some(reader) = &self.reader {
                ui.label(RichText::new(format!("{} · {} bytes", reader.filename, reader.get_data().len())).size(11.0).weak());
            }
        }
        if self.reader.is_none() {
            if self.error_msg.is_none() {
                ui.label("No DAT loaded");
            }
            return;
        }

        self.ensure_table(is_poe2);
        let table = self.table.as_ref().and_then(|t| t.2.clone());
        match table {
            Some(table) => self.show_table(ui, is_poe2, table, loader, false),
            None => {
                self.show_unknown_banner(ui, is_poe2);
                let guessed = if self.byte_view { None } else { self.ensure_guessed() };
                match guessed {
                    Some(t) => self.show_table(ui, is_poe2, t, loader, true),
                    None => {
                        let reader = self.reader.take().unwrap();
                        self.show_generic_view(ui, &reader);
                        self.reader = Some(reader);
                    }
                }
            }
        }
    }

    fn ensure_guessed(&mut self) -> Option<Arc<Table>> {
        if self.guessed.is_none() {
            let reader = self.reader.as_ref()?;
            let cols = crate::dat::analysis::analyze(reader);
            if cols.is_empty() {
                return None;
            }
            let name = file_stem(&reader.filename);
            self.guessed = Some(Arc::new(crate::dat::analysis::to_table(&cols, &name)));
        }
        self.guessed.clone()
    }

    fn show_table(&mut self, ui: &mut egui::Ui, is_poe2: bool, table: Arc<Table>, loader: Option<&mut TableLoader<'_>>, guessed: bool) {
        let (row_count, row_len, is_64bit, filename) = {
            let r = self.reader.as_ref().unwrap();
            (r.row_count, r.row_length, r.is_64bit, r.filename.clone())
        };
        let schema_width = table.row_width(is_64bit);
        let width_ok = guessed || row_len.map(|l| l == schema_width).unwrap_or(true);

        // ── Toolbar ─────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(RichText::new(&table.name).strong().size(15.0));
            ui.label(RichText::new(format!("{} rows · {} columns", row_count, table.columns.len())).weak());
            if guessed {
                ui.label(RichText::new("column types guessed from data").color(Color32::from_rgb(245, 158, 11)).size(11.0));
            } else {
                ui.label(RichText::new(format!("{} · schema {}", if is_poe2 { "PoE 2" } else { "PoE 1" }, self.schema_date)).weak().size(11.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Update schema").on_hover_text("Download the latest community schema").clicked() {
                    self.request_update_schema = true;
                }
                ui.toggle_value(&mut self.show_detail, "Details").on_hover_text("Show the selected row in a side panel");
                ui.menu_button("Columns", |ui| {
                    ui.set_min_width(180.0);
                    if ui.button("Show all").clicked() {
                        self.hidden_cols.clear();
                    }
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        for (i, col) in table.columns.iter().enumerate() {
                            let mut shown = !self.hidden_cols.contains(&i);
                            if ui.checkbox(&mut shown, col_name(col, i)).changed() {
                                if shown { self.hidden_cols.remove(&i); } else { self.hidden_cols.insert(i); }
                            }
                        }
                    });
                });
                if ui.button("Export CSV").clicked() {
                    self.export_csv(&table);
                }
                if ui.button("Export JSON").clicked() {
                    self.export_json(&table);
                }
            });
        });

        if let Some(w) = self.schema_version_warning() {
            ui.colored_label(Color32::from_rgb(245, 158, 11), format!("⚠ {}", w));
        }
        if !width_ok {
            ui.colored_label(
                Color32::from_rgb(245, 158, 11),
                format!(
                    "⚠ Schema describes {}-byte rows but this file has {}-byte rows. Columns after the first mismatch are misaligned — update the schema.",
                    schema_width,
                    row_len.unwrap_or(0)
                ),
            );
        }

        ui.horizontal(|ui| {
            ui.label("🔍");
            let resp = ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter rows (any column)").desired_width(240.0));
            if resp.changed() {
                self.view_rows = None;
            }
            if !self.filter.is_empty() && ui.small_button("✕").clicked() {
                self.filter.clear();
                self.view_rows = None;
            }
            if let Some((c, asc)) = self.sort {
                let name = table.columns.get(c).map(|col| col_name(col, c)).unwrap_or_default();
                ui.label(RichText::new(format!("Sorted by {} {}", name, if asc { "▲" } else { "▼" })).weak());
                if ui.small_button("Clear sort").clicked() {
                    self.sort = None;
                    self.view_rows = None;
                }
            }
            if let Some(v) = &self.view_rows {
                ui.label(RichText::new(format!("{} of {} rows", v.len(), row_count)).weak());
            }
            if let Some(sel) = self.selected_row {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Row {} selected", sel)).weak());
                });
            }
        });
        ui.separator();

        if (self.sort.is_some() || !self.filter.trim().is_empty()) && self.view_rows.is_none() {
            self.rebuild_view(&table);
        }
        if self.sort.is_none() && self.filter.trim().is_empty() {
            self.view_rows = None;
        }

        // Pull state out of `self` so the table closures borrow only locals.
        let reader = self.reader.take().unwrap();
        let schema = self.schema.take().unwrap();
        let mut related = std::mem::take(&mut self.related);
        let mut row_cache = std::mem::replace(&mut self.row_cache, LruCache::new(NonZeroUsize::new(1).unwrap()));
        let all_rows = self.all_rows.take();
        let view_rows = self.view_rows.take();
        let scroll_to = self.scroll_to_row.take();
        let selected = self.selected_row;
        let show_detail = self.show_detail;
        let hidden = self.hidden_cols.clone();
        let mut sort = self.sort;
        let mut hide_request: Option<usize> = None;
        let mut out = CellOut { nav: None, scroll_to: None, select: None };

        let base_dir = filename.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        let ext = filename.rsplit('.').next().unwrap_or("datc64").to_string();
        let mut rel = RelCtx { schema: &schema, related: &mut related, loader, base_dir, ext, is_poe2 };
        let current_table = table.name.clone();

        let get_row = |i: u32, row_cache: &mut LruCache<u32, Vec<DatValue>>| -> Vec<DatValue> {
            if let Some(all) = &all_rows {
                all.get(i as usize).cloned().unwrap_or_default()
            } else {
                row_cache.get_or_insert(i, || reader.read_row(i, &table).unwrap_or_default()).clone()
            }
        };

        if show_detail {
            if let Some(sel) = selected {
                egui::SidePanel::right("dat_row_detail")
                    .resizable(true)
                    .default_width(360.0)
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Row {}", sel)).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Copy JSON").clicked() {
                                    let vals = get_row(sel, &mut row_cache);
                                    let mut map = serde_json::Map::new();
                                    map.insert("_rid".into(), serde_json::Value::from(sel));
                                    for (j, col) in table.columns.iter().enumerate() {
                                        if let Some(v) = vals.get(j) {
                                            map.insert(col_name(col, j), reader.value_to_json(v, col));
                                        }
                                    }
                                    ui.ctx().copy_text(serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default());
                                }
                            });
                        });
                        ui.separator();
                        let vals = get_row(sel, &mut row_cache);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            egui::Grid::new("dat_detail_grid").num_columns(2).spacing([12.0, 6.0]).striped(true).show(ui, |ui| {
                                for (j, col) in table.columns.iter().enumerate() {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(col_name(col, j)).strong());
                                        let ty = format!("{}{}", col.r#type, if col.array { "[]" } else { "" });
                                        let ty = match &col.references {
                                            Some(r) => format!("{} → {}", ty, r.table),
                                            None => ty,
                                        };
                                        ui.label(RichText::new(ty).size(10.5).weak());
                                    });
                                    if let Some(v) = vals.get(j) {
                                        show_value(ui, v, col, &reader, &mut rel, &current_table, true, &mut out);
                                    } else {
                                        ui.label("?");
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    });
            }
        }

        let visible_cols: Vec<usize> = (0..table.columns.len()).filter(|i| !hidden.contains(i)).collect();
        let n_rows = view_rows.as_ref().map(|v| v.len()).unwrap_or(row_count as usize);

        egui::ScrollArea::horizontal().id_salt("dat_table_hscroll").auto_shrink([false, false]).show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .auto_shrink([true, false])
            .min_scrolled_height(0.0)
            .column(TCol::initial(56.0).at_least(40.0).clip(true));
        for &ci in &visible_cols {
            builder = builder.column(TCol::initial(col_width(&table.columns[ci])).at_least(48.0).clip(true).resizable(true));
        }
        if let Some(target) = scroll_to {
            let view_idx = match &view_rows {
                Some(v) => v.iter().position(|&r| r == target),
                None => Some(target as usize),
            };
            if let Some(vi) = view_idx {
                builder = builder.scroll_to_row(vi, Some(egui::Align::Center));
            }
        }

        builder
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.strong("#");
                });
                for &ci in &visible_cols {
                    let col = &table.columns[ci];
                    header.col(|ui| {
                        let name = col_name(col, ci);
                        let arrow = match sort {
                            Some((c, true)) if c == ci => " ▲",
                            Some((c, false)) if c == ci => " ▼",
                            _ => "",
                        };
                        let resp = ui.add(egui::Label::new(RichText::new(format!("{}{}", name, arrow)).strong()).truncate().sense(egui::Sense::click()));
                        let mut tip = format!("{}{}", col.r#type, if col.array { "[]" } else { "" });
                        if let Some(r) = &col.references {
                            tip.push_str(&format!(" → {}", r.table));
                        }
                        if let Some(d) = &col.description {
                            tip.push('\n');
                            tip.push_str(d);
                        }
                        tip.push_str("\nClick to sort · right-click for options");
                        let resp = resp.on_hover_text(tip);
                        if resp.clicked() {
                            sort = match sort {
                                Some((c, true)) if c == ci => Some((ci, false)),
                                Some((c, false)) if c == ci => None,
                                _ => Some((ci, true)),
                            };
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Sort ascending").clicked() { sort = Some((ci, true)); ui.close_menu(); }
                            if ui.button("Sort descending").clicked() { sort = Some((ci, false)); ui.close_menu(); }
                            if ui.button("Hide column").clicked() { hide_request = Some(ci); ui.close_menu(); }
                            if ui.button("Copy column name").clicked() { ui.ctx().copy_text(name.clone()); ui.close_menu(); }
                        });
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, n_rows, |mut row| {
                    let vi = row.index();
                    let real = match &view_rows {
                        Some(v) => v[vi],
                        None => vi as u32,
                    };
                    row.set_selected(selected == Some(real));
                    let vals = get_row(real, &mut row_cache);
                    row.col(|ui| {
                        ui.label(RichText::new(real.to_string()).weak().monospace());
                    });
                    for &ci in &visible_cols {
                        let col = &table.columns[ci];
                        row.col(|ui| {
                            match vals.get(ci) {
                                Some(v) => show_value(ui, v, col, &reader, &mut rel, &current_table, false, &mut out),
                                None => { ui.label(RichText::new("ERR").color(Color32::from_rgb(239, 68, 68))); }
                            }
                        });
                    }
                    if row.response().clicked() {
                        out.select = Some(real);
                    }
                });
            });
        });

        // Restore state and apply interactions.
        drop(rel);
        self.reader = Some(reader);
        self.schema = Some(schema);
        self.related = related;
        self.row_cache = row_cache;
        self.all_rows = all_rows;
        self.view_rows = view_rows;
        if sort != self.sort {
            self.sort = sort;
            self.view_rows = None;
        }
        if let Some(h) = hide_request {
            self.hidden_cols.insert(h);
        }
        if let Some(s) = out.select {
            self.selected_row = if self.selected_row == Some(s) && out.scroll_to.is_none() { None } else { Some(s) };
        }
        if let Some(s) = out.scroll_to {
            self.scroll_to_row = Some(s);
            self.selected_row = Some(s);
        }
        if out.nav.is_some() {
            self.nav_request = out.nav;
        }
    }

    fn show_unknown_banner(&mut self, ui: &mut egui::Ui, is_poe2: bool) {
        ui.horizontal(|ui| {
            if self.schema.is_none() {
                ui.colored_label(Color32::from_rgb(239, 68, 68), "Schema not loaded.");
                if ui.small_button("Download schema").clicked() {
                    self.request_update_schema = true;
                }
            } else {
                ui.colored_label(Color32::from_rgb(245, 158, 11), format!(
                    "Table not defined in the {} schema.",
                    if is_poe2 { "PoE 2" } else { "PoE 1" }
                ));
                if ui.small_button("Update schema").clicked() {
                    self.request_update_schema = true;
                }
            }
            ui.toggle_value(&mut self.byte_view, "Byte view").on_hover_text("Raw fixed-section bytes, 8 per column");
            if !self.byte_view {
                ui.label(RichText::new("Column types are guessed from the data and may be wrong.").weak());
            }
        });
    }

    fn output_rows(&self, table: &Table) -> Vec<Vec<DatValue>> {
        let Some(reader) = &self.reader else { return Vec::new() };
        (0..reader.row_count).map(|i| reader.read_row(i, table).unwrap_or_default()).collect()
    }

    fn export_json(&self, table: &Table) {
        let Some(reader) = &self.reader else { return };
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.json", table.name)).save_file() else { return };
        let mut all = Vec::with_capacity(reader.row_count as usize);
        for (i, vals) in self.output_rows(table).into_iter().enumerate() {
            let mut map = serde_json::Map::new();
            map.insert("_rid".to_string(), serde_json::Value::from(i));
            for (j, col) in table.columns.iter().enumerate() {
                if let Some(v) = vals.get(j) {
                    map.insert(col_name(col, j), reader.value_to_json(v, col));
                }
            }
            all.push(serde_json::Value::Object(map));
        }
        if let Ok(f) = std::fs::File::create(path) {
            let _ = serde_json::to_writer_pretty(std::io::BufWriter::new(f), &all);
        }
    }

    fn export_csv(&self, table: &Table) {
        let Some(reader) = &self.reader else { return };
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.csv", table.name)).save_file() else { return };
        let mut out = String::new();
        out.push_str("_rid");
        for (j, col) in table.columns.iter().enumerate() {
            out.push(',');
            out.push_str(&csv_escape(&col_name(col, j)));
        }
        out.push('\n');
        for (i, vals) in self.output_rows(table).into_iter().enumerate() {
            out.push_str(&i.to_string());
            for (j, col) in table.columns.iter().enumerate() {
                out.push(',');
                let cell = match vals.get(j) {
                    Some(v @ DatValue::List(..)) | Some(v @ DatValue::Interval(..)) => reader.value_to_json(v, col).to_string(),
                    Some(v) => scalar_text(v),
                    None => String::new(),
                };
                out.push_str(&csv_escape(&cell));
            }
            out.push('\n');
        }
        let _ = std::fs::write(path, out);
    }

    pub fn show_generic_view(&self, ui: &mut egui::Ui, reader: &DatReader) {
        let Some(row_len) = reader.row_length else {
            ui.label("Unknown row length (cannot display table)");
            return;
        };
        let num_cols = row_len.div_ceil(8);

        egui::ScrollArea::horizontal().id_salt("dat_bytes_hscroll").auto_shrink([false, false]).show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .auto_shrink([true, false])
            .column(TCol::initial(56.0).resizable(true))
            .columns(TCol::initial(190.0).resizable(true), num_cols)
            .min_scrolled_height(0.0)
            .header(22.0, |mut header| {
                header.col(|ui| { ui.strong("#"); });
                for i in 0..num_cols {
                    header.col(|ui| { ui.strong(format!("{}–{}", i * 8, ((i + 1) * 8).min(row_len))); });
                }
            })
            .body(|body| {
                body.rows(ROW_H, reader.row_count as usize, |mut row| {
                    let row_index = row.index();
                    row.col(|ui| { ui.label(RichText::new(row_index.to_string()).weak().monospace()); });
                    let start = 4 + (row_index * row_len);
                    if start + row_len <= reader.get_data().len() {
                        let row_data = &reader.get_data()[start..start + row_len];
                        for i in 0..num_cols {
                            row.col(|ui| {
                                let s = i * 8;
                                let e = std::cmp::min(s + 8, row_len);
                                if s < e {
                                    let hex: Vec<String> = row_data[s..e].iter().map(|b| format!("{:02X}", b)).collect();
                                    ui.label(RichText::new(hex.join(" ")).monospace());
                                }
                            });
                        }
                    }
                });
            });
        });
    }

    #[allow(dead_code)]
    pub fn convert_to_json(&self, data: &[u8], filename: &str) -> Option<String> {
        let stem = file_stem(filename);
        let schema = self.schema.as_ref()?;
        let table = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&stem))?;
        let reader = DatReader::new(data.to_vec(), filename).ok()?;
        let mut all_rows = Vec::new();
        for i in 0..reader.row_count {
            if let Ok(values) = reader.read_row(i, table) {
                let mut row_map = std::collections::BTreeMap::new();
                for (j, col) in table.columns.iter().enumerate() {
                    if let Some(val) = values.get(j) {
                        row_map.insert(col_name(col, j), reader.value_to_json(val, col));
                    }
                }
                all_rows.push(row_map);
            }
        }
        serde_json::to_string_pretty(&all_rows).ok()
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
