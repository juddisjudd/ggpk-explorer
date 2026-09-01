use crate::dat::analysis::{self, AlignReport, FkCandidate, TableStats};
use crate::dat::overrides::Overrides;
use crate::dat::reader::{get_column_size, DatReader, DatValue};
use crate::dat::schema::{game_mask, Column, Schema, Table, SUPPORTED_SCHEMA_VERSION};
use crate::ggpk::reader::GgpkReader;
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column as TCol, TableBuilder};
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::path::PathBuf;
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
    /// Community schema before user overrides are layered on.
    base_schema: Option<Schema>,
    overrides: Overrides,
    overrides_path: PathBuf,
    /// Schema layout re-fitted onto the file after its row width drifted (keyed by schema_gen).
    aligned: Option<(u64, Arc<Table>, AlignReport)>,
    use_aligned: bool,
    /// Row count of every DAT in the index, supplied by the content view for FK inference.
    pub table_stats: Option<Arc<Vec<TableStats>>>,
    pub request_table_stats: bool,
    pub table_stats_loading: bool,
    fk_suggestions: Option<(usize, HashMap<usize, Vec<FkCandidate>>)>,
    editor: Option<SchemaEditor>,
    notice: Option<String>,
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
            base_schema: None,
            overrides: Overrides::default(),
            overrides_path: Overrides::default_path(),
            aligned: None,
            use_aligned: true,
            table_stats: None,
            request_table_stats: false,
            table_stats_loading: false,
            fk_suggestions: None,
            editor: None,
            notice: None,
        }
    }
}

/// Where the column layout being shown came from.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Schema,
    Custom,
    Aligned,
    Guessed,
}

/// Mutations collected while the table is drawn and applied once its borrows end.
enum Deferred {
    Save { name: String, columns: Vec<Column> },
    Revert(String),
}

struct SchemaEditor {
    name: String,
    columns: Vec<Column>,
    dirty: bool,
}

#[derive(PartialEq)]
enum EditorAction {
    None,
    Save,
    Revert,
    Close,
}

const EDITOR_TYPES: &[&str] = &["bool", "u8", "i16", "u16", "i32", "u32", "f32", "i64", "u64", "string", "row", "foreignrow", "enumrow"];

fn blank_column(ty: &str) -> Column {
    Column {
        name: None,
        description: None,
        array: false,
        r#type: ty.to_string(),
        unique: false,
        localized: false,
        references: None,
        interval: false,
        file: None,
        files: None,
    }
}

/// Appends numeric columns until the layout covers `missing` more bytes.
fn pad_columns(columns: &mut Vec<Column>, mut missing: usize) {
    while missing >= 4 {
        columns.push(blank_column("i32"));
        missing -= 4;
    }
    if missing >= 2 {
        columns.push(blank_column("i16"));
        missing -= 2;
    }
    for _ in 0..missing {
        columns.push(blank_column("u8"));
    }
}

impl SchemaEditor {
    fn from_table(table: &Table) -> Self {
        Self { name: table.name.clone(), columns: table.columns.clone(), dirty: false }
    }

    fn as_table(&self, is_poe2: bool) -> Table {
        Table { name: self.name.clone(), columns: self.columns.clone(), tags: None, valid_for: Some(game_mask(is_poe2)), custom: true }
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

fn candidate_label(c: &FkCandidate) -> String {
    format!("{}{} ({:.0}%)", if c.name_match { "★ " } else { "" }, c.stem, c.fit * 100.0)
}

fn candidate_menu_label(c: &FkCandidate) -> String {
    format!(
        "{}{} · {} rows · {:.0}% fit{}",
        if c.name_match { "★ " } else { "" },
        c.stem,
        c.row_count,
        c.fit * 100.0,
        if c.name_match { " · name match" } else { "" }
    )
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
        self.base_schema = Some(schema);
        self.schema_date = date;
        self.rebuild_schema();
    }

    /// Reads `schema_overrides.json` from the app data dir and layers it over the schema.
    pub fn load_overrides(&mut self) {
        self.overrides = Overrides::load(&self.overrides_path);
        if self.base_schema.is_some() || !self.overrides.is_empty() {
            self.rebuild_schema();
        }
    }

    fn rebuild_schema(&mut self) {
        let mut merged = self.base_schema.clone().unwrap_or_else(Schema::empty);
        merged.apply_overrides(&self.overrides.tables);
        self.schema = Some(merged);
        self.schema_gen += 1;
        self.table = None;
        self.aligned = None;
        self.fk_suggestions = None;
        self.related.clear();
        self.row_cache.clear();
        self.invalidate_view();
    }

    fn persist_overrides(&mut self) {
        self.notice = Some(match self.overrides.save(&self.overrides_path) {
            Ok(()) => format!("Saved {}", self.overrides_path.display()),
            Err(e) => format!("Could not write {}: {}", self.overrides_path.display(), e),
        });
    }

    fn save_override(&mut self, name: &str, columns: Vec<Column>, is_poe2: bool) {
        self.overrides.upsert(Table { name: name.to_string(), columns, tags: None, valid_for: Some(game_mask(is_poe2)), custom: true });
        self.persist_overrides();
        self.rebuild_schema();
    }

    fn revert_override(&mut self, name: &str, is_poe2: bool) {
        if self.overrides.remove(name, is_poe2) {
            self.persist_overrides();
            self.rebuild_schema();
        }
    }

    pub fn set_table_stats(&mut self, stats: Vec<TableStats>) {
        self.table_stats = Some(Arc::new(stats));
        self.table_stats_loading = false;
        self.fk_suggestions = None;
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
        self.aligned = None;
        self.fk_suggestions = None;
        self.editor = None;
        self.notice = None;
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
            Some(table) => {
                let (row_len, is_64bit) = {
                    let r = self.reader.as_ref().unwrap();
                    (r.row_length, r.is_64bit)
                };
                let drifted = row_len.map(|l| l != table.row_width(is_64bit)).unwrap_or(false);
                let layout = if table.custom { Layout::Custom } else { Layout::Schema };
                if drifted && !table.custom && self.use_aligned {
                    match self.ensure_aligned(&table) {
                        Some(aligned) => self.show_table(ui, is_poe2, aligned, loader, Layout::Aligned),
                        None => self.show_table(ui, is_poe2, table, loader, layout),
                    }
                } else {
                    self.show_table(ui, is_poe2, table, loader, layout);
                }
            }
            None => {
                self.show_unknown_banner(ui, is_poe2);
                let guessed = if self.byte_view { None } else { self.ensure_guessed() };
                match guessed {
                    Some(t) => self.show_table(ui, is_poe2, t, loader, Layout::Guessed),
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
            let cols = analysis::analyze(reader);
            if cols.is_empty() {
                return None;
            }
            let name = file_stem(&reader.filename);
            self.guessed = Some(Arc::new(analysis::to_table(&cols, &name)));
        }
        self.guessed.clone()
    }

    fn ensure_aligned(&mut self, schema_table: &Arc<Table>) -> Option<Arc<Table>> {
        if let Some((gen, t, _)) = &self.aligned {
            if *gen == self.schema_gen {
                return Some(t.clone());
            }
        }
        let reader = self.reader.as_ref()?;
        let cols = analysis::analyze(reader);
        if cols.is_empty() {
            return None;
        }
        let (table, report) = analysis::align_schema(schema_table, &cols, reader.is_64bit);
        let table = Arc::new(table);
        self.aligned = Some((self.schema_gen, table.clone(), report));
        Some(table)
    }

    /// Likely `references` targets for foreignrow columns that have none, keyed by column
    /// index. Asks the content view for a table scan the first time it is needed.
    fn ensure_fk_suggestions(&mut self, table: &Arc<Table>, base_dir: &str, ext: &str, is_poe2: bool) -> HashMap<usize, Vec<FkCandidate>> {
        let key = Arc::as_ptr(table) as usize;
        if let Some((k, s)) = &self.fk_suggestions {
            if *k == key {
                return s.clone();
            }
        }
        if !analysis::has_unresolved_foreign(table) {
            return HashMap::new();
        }
        let Some(stats) = self.table_stats.clone() else {
            self.request_table_stats = true;
            return HashMap::new();
        };
        let Some(reader) = self.reader.as_ref() else { return HashMap::new() };
        let current = reader.filename.to_ascii_lowercase();
        let mut out = HashMap::new();
        for (ci, st) in analysis::foreign_key_stats(reader, table).into_iter().enumerate() {
            let Some(st) = st else { continue };
            if table.columns[ci].references.is_some() {
                continue;
            }
            let name = table.columns[ci].name.as_deref().filter(|n| !n.starts_with('@'));
            let mut ranked = analysis::rank_targets(&st, &stats, base_dir, ext, &current, 5, name);
            if let Some(schema) = &self.schema {
                for c in &mut ranked {
                    if let Some(t) = schema.find_table(&c.stem, is_poe2) {
                        c.stem = t.name.clone();
                    }
                }
            }
            if !ranked.is_empty() {
                out.insert(ci, ranked);
            }
        }
        self.fk_suggestions = Some((key, out.clone()));
        out
    }

    fn show_table(&mut self, ui: &mut egui::Ui, is_poe2: bool, table: Arc<Table>, loader: Option<&mut TableLoader<'_>>, layout: Layout) {
        let (row_count, row_len, is_64bit, filename) = {
            let r = self.reader.as_ref().unwrap();
            (r.row_count, r.row_length, r.is_64bit, r.filename.clone())
        };
        let schema_width = table.row_width(is_64bit);
        let width_ok = matches!(layout, Layout::Aligned | Layout::Guessed) || row_len.map(|l| l == schema_width).unwrap_or(true);
        let base_dir = filename.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        let ext = filename.rsplit('.').next().unwrap_or("datc64").to_string();
        let suggestions = self.ensure_fk_suggestions(&table, &base_dir, &ext, is_poe2);
        let amber = Color32::from_rgb(245, 158, 11);
        let mut pending: Vec<Deferred> = Vec::new();

        // ── Toolbar ─────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(RichText::new(&table.name).strong().size(15.0));
            ui.label(RichText::new(format!("{} rows · {} columns", row_count, table.columns.len())).weak());
            match layout {
                Layout::Schema => {
                    ui.label(RichText::new(format!("{} · schema {}", if is_poe2 { "PoE 2" } else { "PoE 1" }, self.schema_date)).weak().size(11.0));
                }
                Layout::Custom => {
                    ui.label(RichText::new("custom layout").color(Color32::from_rgb(96, 165, 250)).size(11.0))
                        .on_hover_text(format!("From {}", self.overrides_path.display()));
                }
                Layout::Aligned => {
                    ui.label(RichText::new("schema re-fitted to this file").color(amber).size(11.0));
                }
                Layout::Guessed => {
                    ui.label(RichText::new("column types guessed from data").color(amber).size(11.0));
                }
            }
            if self.table_stats_loading {
                ui.spinner();
                ui.label(RichText::new("scanning tables for reference targets…").weak().size(11.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Update schema").on_hover_text("Download the latest community schema").clicked() {
                    self.request_update_schema = true;
                }
                let mut editing = self.editor.is_some();
                if ui
                    .toggle_value(&mut editing, "Edit columns")
                    .on_hover_text("Rename, retype or re-reference columns and save the result as a custom layout")
                    .changed()
                {
                    self.editor = if editing { Some(SchemaEditor::from_table(&table)) } else { None };
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
            ui.colored_label(amber, format!("⚠ {}", w));
        }
        if let Some(n) = self.notice.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n).weak().size(11.0));
                if ui.small_button("✕").clicked() {
                    self.notice = None;
                }
            });
        }
        let file_width = row_len.unwrap_or(0);
        match layout {
            Layout::Aligned => {
                let report = self.aligned.as_ref().map(|a| a.2.clone()).unwrap_or_default();
                let community_width = self.table.as_ref().and_then(|t| t.2.as_ref()).map(|t| t.row_width(is_64bit)).unwrap_or(0);
                ui.horizontal_wrapped(|ui| {
                    let mut msg = format!(
                        "⚠ Schema drift: this file has {}-byte rows, the schema describes {}. {} column(s) re-fitted by type, {} new column(s) named by offset.",
                        file_width,
                        community_width,
                        report.matched,
                        report.added.len()
                    );
                    if !report.dropped.is_empty() {
                        msg.push_str(&format!(" Not placed: {}.", report.dropped.join(", ")));
                    }
                    ui.colored_label(amber, msg);
                    if ui.small_button("Show schema layout").clicked() {
                        self.use_aligned = false;
                    }
                    if ui.small_button("Save as custom layout").on_hover_text("Keep this layout in schema_overrides.json").clicked() {
                        pending.push(Deferred::Save { name: table.name.clone(), columns: table.columns.clone() });
                    }
                });
            }
            Layout::Schema if !width_ok => {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        amber,
                        format!("⚠ Schema describes {}-byte rows but this file has {}-byte rows. Columns after the first mismatch are misaligned.", schema_width, file_width),
                    );
                    if ui.small_button("Re-fit to data").clicked() {
                        self.use_aligned = true;
                    }
                });
            }
            Layout::Custom if !width_ok => {
                ui.colored_label(amber, format!("⚠ Custom layout describes {}-byte rows but this file has {}-byte rows — fix it under Edit columns.", schema_width, file_width));
            }
            _ => {}
        }

        if self.editor.is_some() {
            match self.show_editor(ui, &table, row_len, is_64bit, is_poe2, &suggestions, layout) {
                EditorAction::Save => {
                    if let Some(e) = &mut self.editor {
                        e.dirty = false;
                        pending.push(Deferred::Save { name: e.name.clone(), columns: e.columns.clone() });
                    }
                }
                EditorAction::Revert => {
                    pending.push(Deferred::Revert(table.name.clone()));
                    self.editor = None;
                }
                EditorAction::Close => self.editor = None,
                EditorAction::None => {}
            }
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
        let had_schema = self.schema.is_some();
        let schema = self.schema.take().unwrap_or_else(Schema::empty);
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
        let mut set_ref: Option<(usize, String)> = None;
        let mut out = CellOut { nav: None, scroll_to: None, select: None };

        let mut rel = RelCtx { schema: &schema, related: &mut related, loader, base_dir: base_dir.clone(), ext: ext.clone(), is_poe2 };
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
                        if let Some(c) = suggestions.get(&ci) {
                            let list: Vec<String> = c.iter().take(3).map(candidate_label).collect();
                            tip.push_str(&format!("\nLikely target: {} — right-click to set", list.join(", ")));
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
                            if let Some(c) = suggestions.get(&ci) {
                                ui.separator();
                                ui.menu_button("Set reference target", |ui| {
                                    for cand in c {
                                        if ui.button(candidate_menu_label(cand)).on_hover_text("Saves a custom layout with this reference").clicked() {
                                            set_ref = Some((ci, cand.stem.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
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
        self.schema = had_schema.then_some(schema);
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
        if let Some((ci, target)) = set_ref {
            let mut columns = table.columns.clone();
            if let Some(c) = columns.get_mut(ci) {
                c.references = Some(analysis::reference_to(&target));
            }
            pending.push(Deferred::Save { name: table.name.clone(), columns });
        }
        for d in pending {
            match d {
                Deferred::Save { name, columns } => self.save_override(&name, columns, is_poe2),
                Deferred::Revert(name) => self.revert_override(&name, is_poe2),
            }
        }
    }

    /// Side panel for editing the column layout; returns what the user asked for.
    #[allow(clippy::too_many_arguments)]
    fn show_editor(
        &mut self,
        ui: &mut egui::Ui,
        table: &Arc<Table>,
        row_len: Option<usize>,
        is_64bit: bool,
        is_poe2: bool,
        suggestions: &HashMap<usize, Vec<FkCandidate>>,
        layout: Layout,
    ) -> EditorAction {
        let Some(mut ed) = self.editor.take() else { return EditorAction::None };
        let mut action = EditorAction::None;
        let green = Color32::from_rgb(74, 222, 128);
        let red = Color32::from_rgb(239, 68, 68);

        egui::SidePanel::right("dat_schema_editor")
            .resizable(true)
            .default_width(520.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Column layout").strong());
                    if ed.dirty {
                        ui.label(RichText::new("unsaved").color(Color32::from_rgb(245, 158, 11)).size(11.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            action = EditorAction::Close;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Table");
                    if ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(180.0)).changed() {
                        ed.dirty = true;
                    }
                });
                let width: usize = ed.columns.iter().map(|c| get_column_size(c, is_64bit)).sum();
                let file_width = row_len.unwrap_or(0);
                let fits = file_width == 0 || width == file_width;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Row width {} · file {}", width, file_width)).color(if fits { green } else { red }));
                    if width < file_width && ui.small_button("Pad to file width").on_hover_text("Append numeric columns for the missing bytes").clicked() {
                        pad_columns(&mut ed.columns, file_width - width);
                        ed.dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(fits, egui::Button::new("Save")).on_hover_text("Write to schema_overrides.json").clicked() {
                        action = EditorAction::Save;
                    }
                    if layout == Layout::Custom && ui.button("Revert to schema").on_hover_text("Delete the override for this table").clicked() {
                        action = EditorAction::Revert;
                    }
                    if ui.button("Copy JSON").on_hover_text("dat-schema table definition for a poe-tool-dev PR").clicked() {
                        ui.ctx().copy_text(Overrides::table_json(&ed.as_table(is_poe2)));
                    }
                    ui.menu_button("Reset from…", |ui| {
                        if ui.button("Current view").clicked() {
                            ed.columns = table.columns.clone();
                            ed.dirty = true;
                            ui.close_menu();
                        }
                        if let Some(t) = self.table.as_ref().and_then(|t| t.2.clone()) {
                            if !t.custom && ui.button("Community schema").clicked() {
                                ed.columns = t.columns.clone();
                                ed.dirty = true;
                                ui.close_menu();
                            }
                        }
                        if ui.button("Guessed from data").clicked() {
                            if let Some(g) = self.ensure_guessed() {
                                ed.columns = g.columns.clone();
                                ed.dirty = true;
                            }
                            ui.close_menu();
                        }
                    });
                });
                ui.separator();

                let mut dirty = false;
                let mut insert_at: Option<usize> = None;
                let mut delete_at: Option<usize> = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("dat_editor_grid").num_columns(6).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("Offset");
                        ui.strong("Name");
                        ui.strong("Type");
                        ui.strong("Reference");
                        ui.label("");
                        ui.end_row();
                        let mut offset = 0usize;
                        for (i, col) in ed.columns.iter_mut().enumerate() {
                            ui.label(RichText::new(i.to_string()).weak().monospace());
                            ui.label(RichText::new(format!("@{}", offset)).weak().monospace());
                            let mut name = col.name.clone().unwrap_or_default();
                            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0)).changed() {
                                col.name = if name.is_empty() { None } else { Some(name) };
                                dirty = true;
                            }
                            let label = format!("{}{}{}", col.r#type, if col.array { "[]" } else { "" }, if col.interval { " ×2" } else { "" });
                            egui::ComboBox::from_id_salt(("dat_editor_type", i)).selected_text(label).width(110.0).show_ui(ui, |ui| {
                                for t in EDITOR_TYPES {
                                    if ui.selectable_label(!col.array && col.r#type == *t, *t).clicked() {
                                        col.r#type = t.to_string();
                                        col.array = false;
                                        col.interval = false;
                                        dirty = true;
                                    }
                                }
                                ui.separator();
                                for t in EDITOR_TYPES {
                                    if ui.selectable_label(col.array && col.r#type == *t, format!("{}[]", t)).clicked() {
                                        col.r#type = t.to_string();
                                        col.array = true;
                                        col.interval = false;
                                        dirty = true;
                                    }
                                }
                            });
                            if analysis::is_foreign(col) || col.r#type == "enumrow" {
                                ui.horizontal(|ui| {
                                    let mut target = col.references.as_ref().map(|r| r.table.clone()).unwrap_or_default();
                                    if ui.add(egui::TextEdit::singleline(&mut target).desired_width(130.0).hint_text("target table")).changed() {
                                        col.references = if target.is_empty() { None } else { Some(analysis::reference_to(&target)) };
                                        dirty = true;
                                    }
                                    if let Some(c) = suggestions.get(&i) {
                                        ui.menu_button("▾", |ui| {
                                            for cand in c {
                                                if ui.button(candidate_menu_label(cand)).clicked() {
                                                    col.references = Some(analysis::reference_to(&cand.stem));
                                                    dirty = true;
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    }
                                });
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("+").on_hover_text("Insert an i32 after this column").clicked() {
                                    insert_at = Some(i + 1);
                                }
                                if ui.small_button("✕").on_hover_text("Delete this column").clicked() {
                                    delete_at = Some(i);
                                }
                            });
                            ui.end_row();
                            offset += get_column_size(col, is_64bit);
                        }
                    });
                    if ed.columns.is_empty() && ui.button("Add column").clicked() {
                        insert_at = Some(0);
                    }
                });
                if let Some(i) = delete_at {
                    ed.columns.remove(i);
                    dirty = true;
                }
                if let Some(i) = insert_at {
                    ed.columns.insert(i.min(ed.columns.len()), blank_column("i32"));
                    dirty = true;
                }
                if dirty {
                    ed.dirty = true;
                }
            });

        self.editor = Some(ed);
        action
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
