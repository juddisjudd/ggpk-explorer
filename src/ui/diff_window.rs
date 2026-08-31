use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use crate::bundles::index::Index;
use crate::diff::{DiffEntry, DiffResult, SnapshotMeta};
use crate::ui::components::modal_section;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DiffTab {
    Modified,
    Added,
    Removed,
    Touched,
}

pub struct DiffWindow {
    open: bool,
    snapshots: Vec<(PathBuf, SnapshotMeta)>,
    selected: Option<usize>,
    confirm_delete: Option<usize>,

    snapshot_rx: Option<Receiver<Result<PathBuf, String>>>,
    diff_rx: Option<Receiver<Result<(SnapshotMeta, DiffResult), String>>>,
    result: Option<(SnapshotMeta, DiffResult)>,

    tab: DiffTab,
    filter: String,
    cached_tab: DiffTab,
    cached_filter: String,
    cached_rows: Vec<usize>,
    cache_valid: bool,

    status: Option<(String, bool)>,
    pub navigate_to: Option<u64>,
}

impl Default for DiffWindow {
    fn default() -> Self {
        Self {
            open: false,
            snapshots: Vec::new(),
            selected: None,
            confirm_delete: None,
            snapshot_rx: None,
            diff_rx: None,
            result: None,
            tab: DiffTab::Modified,
            filter: String::new(),
            cached_tab: DiffTab::Modified,
            cached_filter: String::new(),
            cached_rows: Vec::new(),
            cache_valid: false,
            status: None,
            navigate_to: None,
        }
    }
}

fn format_bytes(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let s = size as f64;
    if s >= GB {
        format!("{:.2} GB", s / GB)
    } else if s >= MB {
        format!("{:.1} MB", s / MB)
    } else if s >= KB {
        format!("{:.1} KB", s / KB)
    } else {
        format!("{} B", size)
    }
}

impl DiffWindow {
    pub fn open_window(&mut self) {
        self.open = true;
        self.confirm_delete = None;
        self.status = None;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        self.snapshots = crate::diff::list_snapshots();
        if self.selected.map_or(true, |i| i >= self.snapshots.len()) {
            self.selected = if self.snapshots.is_empty() { None } else { Some(0) };
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn entries<'a>(result: &'a DiffResult, tab: DiffTab) -> &'a [DiffEntry] {
        match tab {
            DiffTab::Modified => &result.modified,
            DiffTab::Added => &result.added,
            DiffTab::Removed => &result.removed,
            DiffTab::Touched => &result.touched,
        }
    }

    fn tab_color(tab: DiffTab) -> egui::Color32 {
        match tab {
            DiffTab::Modified => egui::Color32::from_rgb(250, 204, 21),
            DiffTab::Added => egui::Color32::from_rgb(74, 222, 128),
            DiffTab::Removed => egui::Color32::from_rgb(239, 68, 68),
            DiffTab::Touched => egui::Color32::from_rgb(120, 170, 210),
        }
    }

    fn start_take_snapshot(&mut self, ctx: &egui::Context, index: Arc<Index>, patch_version: String, source: String) {
        if self.snapshot_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.snapshot_rx = Some(rx);
        self.status = Some(("Saving snapshot...".to_string(), false));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::diff::take_snapshot(&index, &patch_version, &source)
                .map_err(|e| format!("Snapshot failed: {}", e));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn start_compare(&mut self, ctx: &egui::Context, snapshot_path: PathBuf, index: Arc<Index>) {
        if self.diff_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.diff_rx = Some(rx);
        self.status = Some(("Comparing...".to_string(), false));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let (meta, old_index) = crate::diff::load_snapshot(&snapshot_path)
                    .map_err(|e| format!("Failed to load snapshot: {}", e))?;
                Ok((meta.clone(), crate::diff::diff_indexes(&old_index, &index)))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.snapshot_rx {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    self.snapshot_rx = None;
                    self.status = Some((
                        format!("Snapshot saved: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("?")),
                        false,
                    ));
                    self.refresh();
                }
                Ok(Err(e)) => {
                    self.snapshot_rx = None;
                    self.status = Some((e, true));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.snapshot_rx = None;
                    self.status = Some(("Snapshot thread died".to_string(), true));
                }
            }
        }
        if let Some(rx) = &self.diff_rx {
            match rx.try_recv() {
                Ok(Ok((meta, result))) => {
                    self.diff_rx = None;
                    self.status = None;
                    // Land on the first tab that has entries.
                    self.tab = if !result.modified.is_empty() {
                        DiffTab::Modified
                    } else if !result.added.is_empty() {
                        DiffTab::Added
                    } else if !result.removed.is_empty() {
                        DiffTab::Removed
                    } else {
                        DiffTab::Touched
                    };
                    self.result = Some((meta, result));
                    self.cache_valid = false;
                }
                Ok(Err(e)) => {
                    self.diff_rx = None;
                    self.status = Some((e, true));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.diff_rx = None;
                    self.status = Some(("Diff thread died".to_string(), true));
                }
            }
        }
    }

    fn rebuild_filter_cache(&mut self) {
        if self.cache_valid && self.cached_tab == self.tab && self.cached_filter == self.filter {
            return;
        }
        self.cached_rows.clear();
        if let Some((_, result)) = &self.result {
            let entries = Self::entries(result, self.tab);
            let needle = self.filter.to_lowercase();
            for (i, e) in entries.iter().enumerate() {
                if needle.is_empty() || e.path.to_lowercase().contains(&needle) {
                    self.cached_rows.push(i);
                }
            }
        }
        self.cached_tab = self.tab;
        self.cached_filter = self.filter.clone();
        self.cache_valid = true;
    }

    fn export_csv(&mut self) {
        let Some((meta, result)) = &self.result else { return };
        let default_name = format!("ggpk-diff-{}.csv", meta.patch_version);
        let Some(target) = rfd::FileDialog::new().set_file_name(&default_name).save_file() else {
            return;
        };
        let escape = |s: &str| -> String {
            if s.contains(',') || s.contains('"') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };
        let mut out = String::from("status,path,path_hash,old_size,new_size\n");
        let sections: [(&str, &[DiffEntry]); 4] = [
            ("modified", &result.modified),
            ("added", &result.added),
            ("removed", &result.removed),
            ("touched", &result.touched),
        ];
        for (status, entries) in sections {
            for e in entries {
                out.push_str(&format!(
                    "{},{},{:016x},{},{}\n",
                    status,
                    escape(&e.display_path()),
                    e.path_hash,
                    e.old_size.map(|s| s.to_string()).unwrap_or_default(),
                    e.new_size.map(|s| s.to_string()).unwrap_or_default(),
                ));
            }
        }
        match std::fs::write(&target, out) {
            Ok(()) => self.status = Some((format!("Exported to {}", target.display()), false)),
            Err(e) => self.status = Some((format!("CSV export failed: {}", e), true)),
        }
    }

    fn show_snapshot_list(&mut self, ui: &mut egui::Ui, index_loaded: bool) -> Option<SnapshotAction> {
        let mut action = None;
        modal_section(ui, "SNAPSHOTS");

        if self.snapshots.is_empty() {
            ui.label(
                egui::RichText::new(
                    "No snapshots yet. A snapshot records every file's path, size and bundle \
                     placement so it can be compared against the index after a game patch.",
                )
                .size(11.5)
                .color(egui::Color32::from_rgb(126, 126, 134)),
            );
        }

        for i in 0..self.snapshots.len() {
            let (path, meta) = &self.snapshots[i];
            let disk_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let label = format!(
                "{}  ·  {}  ·  {} files  ·  {}",
                meta.patch_version,
                meta.created_at_label(),
                meta.file_count,
                format_bytes(disk_size),
            );
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.selected == Some(i), egui::RichText::new(label).monospace().size(11.5))
                    .on_hover_text(meta.source.as_str())
                    .clicked()
                {
                    self.selected = Some(i);
                    self.confirm_delete = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_delete == Some(i) {
                        if ui
                            .button(egui::RichText::new("Confirm delete").size(11.0).color(egui::Color32::from_rgb(239, 68, 68)))
                            .clicked()
                        {
                            action = Some(SnapshotAction::Delete(i));
                        }
                    } else if ui.button(egui::RichText::new("Delete").size(11.0)).clicked() {
                        self.confirm_delete = Some(i);
                    }
                });
            });
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let busy = self.snapshot_rx.is_some() || self.diff_rx.is_some();
            let snap_btn = ui.add_enabled(index_loaded && !busy, egui::Button::new("Take Snapshot"));
            if snap_btn
                .on_disabled_hover_text("Open a GGPK file or Steam folder first")
                .clicked()
            {
                action = Some(SnapshotAction::Take);
            }
            let can_compare = index_loaded && !busy && self.selected.is_some();
            if ui
                .add_enabled(can_compare, egui::Button::new("Compare with current"))
                .clicked()
            {
                action = Some(SnapshotAction::Compare(self.selected.unwrap()));
            }
            if busy {
                ui.spinner();
            }
        });

        action
    }

    fn show_results(&mut self, ui: &mut egui::Ui, current_index: &Option<Arc<Index>>) {
        let Some((meta, _)) = &self.result else { return };
        let old_version = meta.patch_version.clone();

        ui.separator();
        modal_section(ui, "RESULT");
        ui.label(
            egui::RichText::new(format!("Snapshot {} ({}) vs current index", old_version, meta.created_at_label()))
                .size(11.5)
                .color(egui::Color32::from_rgb(161, 161, 170)),
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let counts = {
                let (_, result) = self.result.as_ref().unwrap();
                [
                    (DiffTab::Modified, "Modified", result.modified.len()),
                    (DiffTab::Added, "Added", result.added.len()),
                    (DiffTab::Removed, "Removed", result.removed.len()),
                    (DiffTab::Touched, "Repacked", result.touched.len()),
                ]
            };
            for (tab, name, count) in counts {
                let text = egui::RichText::new(format!("{} ({})", name, count))
                    .size(11.5)
                    .color(Self::tab_color(tab));
                if ui.selectable_label(self.tab == tab, text).clicked() {
                    self.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new("Export CSV...").size(11.0)).clicked() {
                    self.export_csv();
                }
            });
        });

        if self.tab == DiffTab::Touched {
            ui.label(
                egui::RichText::new(
                    "Same size, but the containing bundle was repacked — content may or may not have changed.",
                )
                .size(10.5)
                .color(egui::Color32::from_rgb(113, 113, 122)),
            );
        }

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Filter").size(11.0).color(egui::Color32::from_rgb(113, 113, 122)));
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(240.0).hint_text("path contains..."));
        });
        ui.add_space(4.0);

        self.rebuild_filter_cache();

        let (_, result) = self.result.as_ref().unwrap();
        let entries = Self::entries(result, self.tab);
        let rows = &self.cached_rows;
        let clickable = self.tab != DiffTab::Removed;

        let mut navigate = None;
        TableBuilder::new(ui)
            .striped(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(300.0))
            .column(Column::exact(88.0))
            .column(Column::exact(88.0))
            .min_scrolled_height(160.0)
            .header(22.0, |mut header| {
                let header_color = egui::Color32::from_rgb(113, 113, 122);
                header.col(|ui| {
                    ui.label(egui::RichText::new("PATH").monospace().size(10.5).color(header_color));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("OLD SIZE").monospace().size(10.5).color(header_color));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("NEW SIZE").monospace().size(10.5).color(header_color));
                });
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let entry = &entries[rows[row.index()]];
                    row.col(|ui| {
                        let text = egui::RichText::new(entry.display_path())
                            .monospace()
                            .size(11.0)
                            .color(Self::tab_color(self.tab));
                        if clickable {
                            if ui.selectable_label(false, text).clicked() {
                                navigate = Some(entry.path_hash);
                            }
                        } else {
                            ui.label(text);
                        }
                    });
                    let size_color = egui::Color32::from_rgb(161, 161, 170);
                    row.col(|ui| {
                        let s = entry.old_size.map(|s| format_bytes(s as u64)).unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(s).size(10.5).color(size_color));
                    });
                    row.col(|ui| {
                        let s = entry.new_size.map(|s| format_bytes(s as u64)).unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(s).size(10.5).color(size_color));
                    });
                });
            });

        if let Some(hash) = navigate {
            let exists = current_index.as_ref().map_or(false, |i| i.files.contains_key(&hash));
            if exists {
                self.navigate_to = Some(hash);
            } else {
                self.status = Some(("File not present in the current index".to_string(), true));
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        current_index: &Option<Arc<Index>>,
        patch_version: &str,
        source: &str,
    ) {
        if !self.open {
            return;
        }
        self.poll();

        let mut open = self.open;
        let mut pending: Option<SnapshotAction> = None;

        egui::Window::new("Patch Diff")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([860.0, 560.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;

                pending = self.show_snapshot_list(ui, current_index.is_some());

                if let Some((msg, is_error)) = &self.status {
                    let color = if *is_error {
                        egui::Color32::from_rgb(239, 68, 68)
                    } else {
                        egui::Color32::from_rgb(113, 113, 122)
                    };
                    ui.label(egui::RichText::new(msg).size(11.0).color(color));
                }

                if self.result.is_some() {
                    self.show_results(ui, current_index);
                }
            });
        self.open = open;

        match pending {
            Some(SnapshotAction::Take) => {
                if let Some(index) = current_index {
                    self.start_take_snapshot(ctx, index.clone(), patch_version.to_string(), source.to_string());
                }
            }
            Some(SnapshotAction::Compare(i)) => {
                if let (Some(index), Some((path, _))) = (current_index, self.snapshots.get(i)) {
                    self.start_compare(ctx, path.clone(), index.clone());
                }
            }
            Some(SnapshotAction::Delete(i)) => {
                if let Some((path, _)) = self.snapshots.get(i) {
                    match std::fs::remove_file(path) {
                        Ok(()) => self.status = Some(("Snapshot deleted".to_string(), false)),
                        Err(e) => self.status = Some((format!("Delete failed: {}", e), true)),
                    }
                }
                self.confirm_delete = None;
                self.refresh();
            }
            None => {}
        }
    }
}

enum SnapshotAction {
    Take,
    Compare(usize),
    Delete(usize),
}
