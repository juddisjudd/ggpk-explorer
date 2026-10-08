//! `.sm` skinned meshes: the wrapped `.smd` in the 3D preview, beside its materials by shape, bounding box and bone groups.

use crate::parsers::sm::SmFile;
use crate::ui::links;
use crate::ui::mesh_preview::{MeshData, MeshPreview, MeshPreviewState};
use crate::ui::object_viewer::Loader;
use eframe::egui::{self, Color32, RichText};

/// A parsed `.sm` and the mesh it names, or why that mesh cannot be drawn.
pub struct SkinnedMesh {
    pub file: SmFile,
    pub mesh: Result<MeshData, String>,
}

impl SkinnedMesh {
    pub fn load(text: &str, loader: &mut Loader<'_>) -> Result<Self, String> {
        let file = crate::parsers::sm::parse(text)?;
        let smd = links::normalize(&file.smd_file);
        let mesh = loader(&smd)
            .ok_or_else(|| format!("{} is not in the index", smd))
            .and_then(|bytes| crate::parsers::model::parse_model(&smd, &bytes))
            .and_then(|model| crate::ui::mesh_preview::extract(&model).ok_or_else(|| format!("{} has no vertices", smd)));
        Ok(Self { file, mesh })
    }
}

pub struct SkinnedMeshViewer;

impl SkinnedMeshViewer {
    /// Returns a path when a link is clicked.
    pub fn show(ui: &mut egui::Ui, id: u64, sm: &SkinnedMesh, state: &mut MeshPreviewState) -> Option<String> {
        let mut opened = None;
        egui::SidePanel::left(egui::Id::new(("sm_details", id))).resizable(true).default_width(360.0).show_inside(ui, |ui| {
            egui::ScrollArea::both().id_salt(("sm_details_scroll", id)).auto_shrink([false, false]).show(ui, |ui| {
                Self::details(ui, sm, state, &mut opened);
            });
        });
        match &sm.mesh {
            Ok(mesh) => MeshPreview::show(ui, id, mesh, state),
            Err(e) => {
                ui.colored_label(Color32::from_rgb(245, 158, 11), format!("Could not draw the mesh: {}", e));
            }
        }
        opened
    }

    fn details(ui: &mut egui::Ui, sm: &SkinnedMesh, state: &mut MeshPreviewState, opened: &mut Option<String>) {
        let file = &sm.file;
        let shape_names: &[(String, usize, usize)] = sm.mesh.as_ref().map(|m| m.shapes.as_slice()).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            crate::ui::components::badge(ui, &format!("v{}", file.version));
            crate::ui::components::badge(ui, &format!("{} materials", file.materials.len()));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("mesh").weak());
            links::maybe_link(ui, &file.smd_file, true, opened);
        });

        ui.add_space(6.0);
        ui.strong("Materials");
        let covered: usize = file.materials.iter().map(|m| m.shape_count as usize).sum();
        if !shape_names.is_empty() && covered != shape_names.len() {
            ui.colored_label(Color32::from_rgb(245, 158, 11), format!("The materials cover {} shapes; the mesh has {}.", covered, shape_names.len()));
        }
        let mut first = 0;
        for material in &file.materials {
            match &material.mat_file {
                Some(mat) => links::maybe_link(ui, mat, true, opened),
                None => ui.label(RichText::new("(no material)").weak()),
            };
            ui.indent(("sm_material", first), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for shape in first..first + material.shape_count as usize {
                        let name = shape_names.get(shape).map(|s| s.0.clone()).unwrap_or_else(|| format!("shape {}", shape));
                        let selected = state.shape == Some(shape);
                        let response = ui.selectable_label(selected, RichText::new(name).small()).on_hover_text("Show only this shape");
                        if response.clicked() && shape < shape_names.len() {
                            state.shape = if selected { None } else { Some(shape) };
                        }
                    }
                });
            });
            first += material.shape_count as usize;
        }

        if let Some(b) = file.bbox {
            ui.add_space(6.0);
            ui.strong("Bounding box");
            egui::Grid::new("sm_bbox").num_columns(4).show(ui, |ui| {
                for (label, v) in [("min", [b[0], b[1], b[2]]), ("max", [b[3], b[4], b[5]]), ("size", [b[3] - b[0], b[4] - b[1], b[5] - b[2]])] {
                    ui.label(RichText::new(label).weak());
                    for x in v {
                        ui.label(RichText::new(format!("{:.2}", x)).monospace());
                    }
                    ui.end_row();
                }
            });
        }

        if let Some(groups) = &file.bone_groups {
            ui.add_space(6.0);
            ui.strong(format!("Bone groups ({})", groups.len()));
            if groups.is_empty() {
                ui.label(RichText::new("none").weak());
            }
            for (i, group) in groups.iter().enumerate() {
                egui::CollapsingHeader::new(format!("{} · {} bones", group.name, group.bones.len())).id_salt(("sm_bone_group", i)).show(ui, |ui| {
                    for bone in &group.bones {
                        ui.label(RichText::new(bone).monospace());
                    }
                });
            }
        }
    }
}
