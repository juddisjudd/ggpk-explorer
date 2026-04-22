use eframe::egui;
use crate::ui::tree_view::{TreeView, TreeViewAction};
use crate::ui::export_window::ExportWindow;
use crate::dat::schema::Schema;

pub struct Sidebar;

impl Sidebar {
    fn hover_icon_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
        let button_size = egui::vec2(24.0, 24.0);
        let (rect, response) = ui.allocate_exact_size(button_size, egui::Sense::click());

        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::Rounding::same(4.0),
                egui::Color32::from_rgb(42, 42, 46),
            );
        }

        let text_color = if response.hovered() {
            egui::Color32::from_rgb(228, 228, 231)
        } else {
            egui::Color32::from_rgb(161, 161, 170)
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(14.0),
            text_color,
        );

        response
    }

    pub fn show(
        ctx: &egui::Context,
        expanded: &mut bool,
        tree_view: &mut TreeView,
        selected_file: &mut Option<crate::ui::app::FileSelection>,
        schema: Option<&Schema>,
        reader_available: bool,
        export_window: &mut ExportWindow,
    ) {
        let panel_frame = egui::Frame {
            inner_margin: egui::Margin::same(0.0),
            fill: ctx.style().visuals.panel_fill,
            stroke: egui::Stroke::NONE,
            ..Default::default()
        };

        let panel = egui::SidePanel::left("tree_panel")
            .resizable(*expanded)
            .frame(panel_frame);

        let panel = if *expanded {
            panel.min_width(240.0).default_width(260.0)
        } else {
            panel.exact_width(32.0)
        };

        panel.show(ctx, |ui| {
            if *expanded {
                Self::show_expanded(ui, expanded, tree_view, selected_file, schema, reader_available, export_window);
            } else {
                Self::show_collapsed(ui, expanded);
            }
        });
    }

    fn show_expanded(
        ui: &mut egui::Ui,
        expanded: &mut bool,
        tree_view: &mut TreeView,
        selected_file: &mut Option<crate::ui::app::FileSelection>,
        schema: Option<&Schema>,
        reader_available: bool,
        export_window: &mut ExportWindow,
    ) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            // Fixed-height header row: 34px, content vertically centered
            let header_w = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(header_w, 34.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("GGPK Content")
                            .monospace()
                            .size(10.5)
                            .color(egui::Color32::from_rgb(113, 113, 122)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        if Self::hover_icon_button(ui, "«").on_hover_text("Collapse Sidebar").clicked() {
                            *expanded = false;
                        }
                    });
                },
            );
            ui.separator();

            if reader_available {
                // Transparent padding wrapper so tree content has left/right breathing room
                egui::Frame {
                    inner_margin: egui::Margin { left: 6.0, right: 4.0, top: 4.0, bottom: 8.0 },
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.push_id("tree_scroll", |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .drag_to_scroll(false)
                            .show(ui, |ui| {
                                #[allow(deprecated)]
                                { ui.style_mut().wrap = Some(false); }

                                let action = tree_view.show(ui, selected_file, schema);
                                Self::handle_tree_action(action, export_window);
                            });
                    });
                });
            } else {
                 ui.centered_and_justified(|ui| {
                    ui.label("No GGPK loaded");
                 });
            }
        });
    }

    fn show_collapsed(ui: &mut egui::Ui, expanded: &mut bool) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if Self::hover_icon_button(ui, ">")
                .on_hover_text("Expand Sidebar")
                .clicked()
            {
                *expanded = true;
            }
        });
    }

    fn handle_tree_action(action: TreeViewAction, export_window: &mut ExportWindow) {
        match action {
            TreeViewAction::None => {},
            TreeViewAction::Select => {}, 
            TreeViewAction::RequestExport { hashes, name, is_folder, settings } => {
                export_window.open_for(&name, is_folder);
                export_window.hashes = hashes;
                if let Some(s) = settings {
                    export_window.settings = s;
                }
            }
        }
    }
}
