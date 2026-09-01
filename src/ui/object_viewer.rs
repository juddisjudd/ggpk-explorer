//! Inspector for the object DSL (`.ao`, `.ot`, `.it`, `.act`, `.epk`): components as
//! collapsible sections, file references as links, and the `extends` chain resolved so
//! inherited components can be shown merged into the child.

use crate::parsers::object_dsl::{self, Component, ObjectFile, Prop};
use crate::ui::json_viewer::JsonTreeViewer;
use crate::ui::links;
use eframe::egui::{self, Color32, RichText};
use std::sync::Arc;

pub type Loader<'l> = dyn FnMut(&str) -> Option<Vec<u8>> + 'l;

const MAX_CHAIN: usize = 12;

#[derive(Default)]
pub struct ObjectViewerState {
    pub filter: String,
    pub show_inherited: bool,
    /// Ancestors from the direct parent outward, with their parsed file when it loaded.
    chain: Option<Vec<(String, Option<Arc<ObjectFile>>)>>,
}

/// A property with the file it came from (0 = the file itself, 1 = parent, …).
struct MergedProp {
    prop: Prop,
    origin: usize,
}

struct MergedComponent {
    name: String,
    args: Vec<String>,
    props: Vec<MergedProp>,
    children: Vec<MergedComponent>,
    json: Option<(serde_json::Value, usize)>,
    origin: usize,
}

fn lift(c: &Component, origin: usize) -> MergedComponent {
    MergedComponent {
        name: c.name.clone(),
        args: c.args.clone(),
        props: c.props.iter().map(|p| MergedProp { prop: p.clone(), origin }).collect(),
        children: c.children.iter().map(|k| lift(k, origin)).collect(),
        json: c.json.clone().map(|j| (j, origin)),
        origin,
    }
}

/// Overlays `child` onto `base`: same-named components merge, a child's `key = value`
/// replaces the parent's unless the key repeats (like `walk_tri`), in which case both stay.
fn merge_into(base: &mut Vec<MergedComponent>, child: &Component, origin: usize) {
    let Some(slot) = base.iter_mut().find(|b| b.name == child.name && b.args == child.args && !child.name.is_empty()) else {
        base.push(lift(child, origin));
        return;
    };
    slot.origin = origin;
    if let Some(j) = &child.json {
        slot.json = Some((j.clone(), origin));
    }
    for p in &child.props {
        let repeats = child.props.iter().filter(|q| q.key == p.key).count() > 1;
        if !repeats {
            slot.props.retain(|mp| mp.prop.key != p.key);
        }
        slot.props.push(MergedProp { prop: p.clone(), origin });
    }
    for k in &child.children {
        merge_into(&mut slot.children, k, origin);
    }
}

fn origin_label(origin: usize, path: &str, chain: &[(String, Option<Arc<ObjectFile>>)]) -> String {
    if origin == 0 {
        path.to_string()
    } else {
        chain.get(origin - 1).map(|(p, _)| p.clone()).unwrap_or_default()
    }
}

fn matches_filter(needle: &str, c: &MergedComponent) -> bool {
    if needle.is_empty() {
        return true;
    }
    c.name.to_ascii_lowercase().contains(needle)
        || c.args.iter().any(|a| a.to_ascii_lowercase().contains(needle))
        || c.props.iter().any(|p| p.prop.key.to_ascii_lowercase().contains(needle) || p.prop.value.to_ascii_lowercase().contains(needle))
        || c.children.iter().any(|k| matches_filter(needle, k))
}

/// Renders a value, turning quoted or bare file references into links.
fn show_value(ui: &mut egui::Ui, value: &str, out: &mut Option<String>) {
    let dark = ui.visuals().dark_mode;
    let num_c = if dark { Color32::from_rgb(209, 154, 102) } else { Color32::from_rgb(180, 83, 9) };
    let str_c = if dark { Color32::from_rgb(152, 195, 121) } else { Color32::from_rgb(3, 117, 43) };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if !value.contains('"') {
            if value.parse::<f64>().is_ok() {
                ui.label(RichText::new(value).monospace().color(num_c));
            } else {
                links::maybe_link(ui, value, true, out);
            }
            return;
        }
        for (i, seg) in value.split('"').enumerate() {
            if seg.is_empty() {
                continue;
            }
            if i % 2 == 1 {
                if links::looks_like_path(seg) {
                    links::maybe_link(ui, seg, true, out);
                } else {
                    ui.label(RichText::new(format!("\"{}\"", seg)).monospace().color(str_c));
                }
            } else {
                for w in seg.split_whitespace() {
                    ui.label(RichText::new(w).monospace());
                }
            }
        }
    });
}

pub struct ObjectViewer;

impl ObjectViewer {
    /// Returns a path when a link is clicked.
    pub fn show(ui: &mut egui::Ui, id: u64, path: &str, doc: &ObjectFile, state: &mut ObjectViewerState, mut loader: Option<&mut Loader<'_>>) -> Option<String> {
        let mut opened = None;
        let dark = ui.visuals().dark_mode;
        let dim = if dark { Color32::from_rgb(120, 120, 130) } else { Color32::from_rgb(140, 140, 150) };

        if state.chain.is_none() {
            let mut chain = Vec::new();
            let mut current = doc.extends.clone();
            let mut current_path = path.to_string();
            while let Some(ext) = current.take() {
                if chain.len() >= MAX_CHAIN {
                    break;
                }
                let target = object_dsl::resolve_extends(&ext, &current_path);
                if chain.iter().any(|(p, _): &(String, Option<Arc<ObjectFile>>)| p.eq_ignore_ascii_case(&target)) || target.eq_ignore_ascii_case(path) {
                    break;
                }
                let parsed = loader.as_mut().and_then(|l| l(&target)).map(|bytes| Arc::new(object_dsl::parse(&crate::parsers::utils::decode_text_lossy(&bytes))));
                current = parsed.as_ref().and_then(|p| p.extends.clone());
                current_path = target.clone();
                chain.push((target, parsed));
            }
            state.chain = Some(chain);
        }
        let chain = state.chain.clone().unwrap_or_default();

        ui.horizontal_wrapped(|ui| {
            if let Some(v) = doc.version {
                crate::ui::components::badge(ui, &format!("v{}", v));
            }
            if doc.is_abstract {
                crate::ui::components::badge(ui, "abstract");
            }
            crate::ui::components::badge(ui, &format!("{} components", doc.components.len()));
            if !chain.is_empty() {
                ui.label(RichText::new("extends").weak());
                for (i, (p, parsed)) in chain.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("→").weak());
                    }
                    let r = links::maybe_link(ui, p, false, &mut opened);
                    if parsed.is_none() {
                        r.on_hover_text("Not found in the index");
                    }
                }
            } else if let Some(e) = &doc.extends {
                ui.label(RichText::new("extends").weak());
                links::maybe_link(ui, &object_dsl::resolve_extends(e, path), false, &mut opened);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let loaded = chain.iter().any(|(_, p)| p.is_some());
                ui.add_enabled_ui(loaded, |ui| {
                    ui.toggle_value(&mut state.show_inherited, "Inherited").on_hover_text("Merge the components of every ancestor into this view");
                });
                if ui.small_button("Copy JSON").clicked() {
                    ui.ctx().copy_text(serde_json::to_string_pretty(doc).unwrap_or_default());
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter components, keys, values").desired_width(260.0));
            if !state.filter.is_empty() && ui.small_button("✕").clicked() {
                state.filter.clear();
            }
        });
        ui.separator();

        // Root-first so the child overrides win.
        let mut merged: Vec<MergedComponent> = Vec::new();
        let mut top_props: Vec<MergedProp> = Vec::new();
        if state.show_inherited {
            for (i, (_, parsed)) in chain.iter().enumerate().rev() {
                if let Some(p) = parsed {
                    for c in &p.components {
                        merge_into(&mut merged, c, i + 1);
                    }
                    top_props.extend(p.props.iter().map(|pr| MergedProp { prop: pr.clone(), origin: i + 1 }));
                }
            }
        }
        for c in &doc.components {
            merge_into(&mut merged, c, 0);
        }
        top_props.extend(doc.props.iter().map(|pr| MergedProp { prop: pr.clone(), origin: 0 }));

        let needle = state.filter.trim().to_ascii_lowercase();
        egui::ScrollArea::both().id_salt(("object_view", id)).auto_shrink([false, false]).show(ui, |ui| {
            if !top_props.is_empty() {
                egui::Grid::new(("object_top_props", id)).num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for (i, mp) in top_props.iter().enumerate() {
                        if !needle.is_empty() && !(mp.prop.key.to_ascii_lowercase().contains(&needle) || mp.prop.value.to_ascii_lowercase().contains(&needle)) {
                            continue;
                        }
                        Self::show_prop(ui, id, i, mp, path, &chain, dim, &mut opened);
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
            for (i, c) in merged.iter().enumerate() {
                if !matches_filter(&needle, c) {
                    continue;
                }
                Self::show_component(ui, id, i, c, path, &chain, dim, &mut opened, 0);
            }
        });
        opened
    }

    #[allow(clippy::too_many_arguments)]
    fn show_prop(ui: &mut egui::Ui, id: u64, i: usize, mp: &MergedProp, path: &str, chain: &[(String, Option<Arc<ObjectFile>>)], dim: Color32, out: &mut Option<String>) {
        let key = RichText::new(if mp.prop.key.is_empty() { "·" } else { mp.prop.key.as_str() }).strong();
        let key = if mp.origin > 0 { key.color(dim) } else { key };
        let r = ui.label(key);
        if mp.origin > 0 {
            r.on_hover_text(format!("inherited from {}", origin_label(mp.origin, path, chain)));
        }
        match &mp.prop.json {
            Some(j) => {
                ui.push_id(("object_prop_json", id, i), |ui| {
                    ui.vertical(|ui| JsonTreeViewer::show_linked(ui, j, out));
                });
            }
            None => show_value(ui, &mp.prop.value, out),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_component(ui: &mut egui::Ui, id: u64, i: usize, c: &MergedComponent, path: &str, chain: &[(String, Option<Arc<ObjectFile>>)], dim: Color32, out: &mut Option<String>, depth: usize) {
        let title = if c.args.is_empty() { c.name.clone() } else { format!("{} {}", c.name, c.args.join(" ")) };
        let title = if title.is_empty() { "(block)".to_string() } else { title };
        let mut text = RichText::new(&title).strong();
        if c.origin > 0 {
            text = text.color(dim);
        }
        let header = egui::CollapsingHeader::new(text).id_salt(("object_component", id, depth, i, &c.name)).default_open(depth < 2);
        let resp = header.show(ui, |ui| {
            if !c.args.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for a in &c.args {
                        links::maybe_link(ui, a, true, out);
                    }
                });
            }
            if let Some((j, _)) = &c.json {
                ui.push_id(("object_component_json", id, depth, i), |ui| JsonTreeViewer::show_linked(ui, j, out));
            }
            if !c.props.is_empty() {
                egui::Grid::new(("object_props", id, depth, i)).num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for (k, mp) in c.props.iter().enumerate() {
                        Self::show_prop(ui, id, k + depth * 1000 + i * 100_000, mp, path, chain, dim, out);
                        ui.end_row();
                    }
                });
            }
            if c.props.is_empty() && c.json.is_none() && c.children.is_empty() {
                ui.label(RichText::new("empty").color(dim).italics());
            }
            for (k, child) in c.children.iter().enumerate() {
                Self::show_component(ui, id, k, child, path, chain, dim, out, depth + 1);
            }
        });
        if c.origin > 0 {
            resp.header_response.on_hover_text(format!("inherited from {}", origin_label(c.origin, path, chain)));
        }
    }
}
