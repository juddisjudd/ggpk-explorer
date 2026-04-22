use eframe::egui;

pub struct StatusBar;

impl StatusBar {
    pub fn show(
        ctx: &egui::Context,
        status_msg: &str,
        is_loading: bool,
        is_mounted: bool,
        poe_version: &str,
        schema_date: &str,
    ) {
        egui::TopBottomPanel::bottom("status_panel")
            .frame(egui::Frame {
                inner_margin: egui::Margin::same(0.0),
                fill: ctx.style().visuals.panel_fill,
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                let bar_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(bar_w, 28.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(10.0);

                        // Mounted chip
                        if is_mounted {
                            let chip_color = egui::Color32::from_rgb(31, 31, 35);
                            let dot_color = egui::Color32::from_rgb(74, 222, 128); // green
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(76.0, 18.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                rect,
                                egui::Rounding::same(3.0),
                                chip_color,
                            );
                            ui.painter().rect_stroke(
                                rect,
                                egui::Rounding::same(3.0),
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(42, 42, 46)),
                            );
                            // dot
                            let dot_center = egui::pos2(rect.min.x + 10.0, rect.center().y);
                            ui.painter().circle_filled(dot_center, 3.0, dot_color);
                            // label
                            ui.painter().text(
                                egui::pos2(rect.min.x + 19.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "Mounted",
                                egui::FontId::monospace(10.0),
                                egui::Color32::from_rgb(228, 228, 231),
                            );
                            ui.add_space(6.0);
                        }

                        // Status message / loading
                        if !status_msg.is_empty() {
                            if is_mounted {
                                ui.label(
                                    egui::RichText::new("\u{00B7}")
                                        .monospace()
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(82, 82, 91)),
                                );
                                ui.add_space(2.0);
                            }
                            ui.label(
                                egui::RichText::new(status_msg)
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(168, 168, 176)),
                            );
                        }
                        if is_loading {
                            ui.spinner();
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(82, 82, 91)),
                            );
                            ui.label(
                                egui::RichText::new("\u{00B7}")
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(82, 82, 91)),
                            );
                            ui.label(
                                egui::RichText::new(format!("Patch {}", poe_version))
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(82, 82, 91)),
                            );
                            ui.label(
                                egui::RichText::new("\u{00B7}")
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(82, 82, 91)),
                            );
                            ui.label(
                                egui::RichText::new(format!("Schema {}", schema_date))
                                    .monospace()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(82, 82, 91)),
                            );
                        });
                    },
                );
            });
    }
}
