use eframe::egui;

/// A new release, as a chip that opens its page. Only drawn when there is one,
/// so the bar stays as it was otherwise.
fn update_chip(ui: &mut egui::Ui, tag: &str, url: &str) {
    let dark_mode = ui.visuals().dark_mode;
    let (fill, border, text_color, dot_color) = match dark_mode {
        true => (
            egui::Color32::from_rgb(31, 31, 35),
            egui::Color32::from_rgb(42, 42, 46),
            egui::Color32::from_rgb(228, 228, 231),
            egui::Color32::from_rgb(96, 165, 250),
        ),
        false => (
            egui::Color32::from_rgb(222, 232, 250),
            egui::Color32::from_rgb(180, 196, 225),
            egui::Color32::from_rgb(25, 55, 120),
            egui::Color32::from_rgb(37, 99, 235),
        ),
    };

    let label = format!("{} available", tag);
    let font = egui::FontId::monospace(10.0);
    let galley = ui.painter().layout_no_wrap(label, font, text_color);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(galley.size().x + 26.0, 18.0), egui::Sense::click());

    let border = match response.hovered() {
        true => dot_color,
        false => border,
    };
    ui.painter().rect_filled(rect, egui::Rounding::same(3.0), fill);
    ui.painter().rect_stroke(rect, egui::Rounding::same(3.0), egui::Stroke::new(1.0_f32, border));
    ui.painter().circle_filled(egui::pos2(rect.min.x + 10.0, rect.center().y), 3.0, dot_color);
    ui.painter().galley(egui::pos2(rect.min.x + 19.0, rect.center().y - galley.size().y / 2.0), galley, text_color);

    if response.clicked() {
        let _ = open::that(url);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("A newer release is out — open it on GitHub");
}

/// How the open patch stands, as a chip that opens the patch page.
fn patch_chip(ui: &mut egui::Ui, health: &crate::ui::patch_page::Health) -> bool {
    let dark_mode = ui.visuals().dark_mode;
    let (fill, border, text_color) = match dark_mode {
        true => (egui::Color32::from_rgb(31, 31, 35), egui::Color32::from_rgb(42, 42, 46), egui::Color32::from_rgb(228, 228, 231)),
        false => (egui::Color32::from_rgb(236, 236, 240), egui::Color32::from_rgb(200, 200, 210), egui::Color32::from_rgb(40, 40, 50)),
    };
    let dot_color = health.tone.color(dark_mode);
    let galley = ui.painter().layout_no_wrap(health.label.clone(), egui::FontId::monospace(10.0), text_color);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(galley.size().x + 26.0, 18.0), egui::Sense::click());
    let border = if response.hovered() { dot_color } else { border };
    ui.painter().rect_filled(rect, egui::Rounding::same(3.0), fill);
    ui.painter().rect_stroke(rect, egui::Rounding::same(3.0), egui::Stroke::new(1.0_f32, border));
    ui.painter().circle_filled(egui::pos2(rect.min.x + 10.0, rect.center().y), 3.0, dot_color);
    ui.painter().galley(egui::pos2(rect.min.x + 19.0, rect.center().y - galley.size().y / 2.0), galley, text_color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open the patch report").clicked()
}

pub struct StatusBar;

impl StatusBar {
    pub fn show(
        ctx: &egui::Context,
        status_msg: &str,
        is_loading: bool,
        is_mounted: bool,
        game: &str,
        poe_version: &str,
        schema_date: &str,
        update: Option<(&str, &str)>,
        patch: Option<&crate::ui::patch_page::Health>,
    ) -> bool {
        let mut open_patch = false;
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

                        let dark_mode = ui.visuals().dark_mode;

                        // Mounted chip
                        if is_mounted {
                            let chip_color = if dark_mode {
                                egui::Color32::from_rgb(31, 31, 35)
                            } else {
                                egui::Color32::from_rgb(220, 235, 225)
                            };
                            let chip_border = if dark_mode {
                                egui::Color32::from_rgb(42, 42, 46)
                            } else {
                                egui::Color32::from_rgb(180, 200, 185)
                            };
                            let chip_text_color = if dark_mode {
                                egui::Color32::from_rgb(228, 228, 231)
                            } else {
                                egui::Color32::from_rgb(20, 80, 40)
                            };
                            let dot_color = if dark_mode {
                                egui::Color32::from_rgb(74, 222, 128)
                            } else {
                                egui::Color32::from_rgb(34, 197, 94)
                            };

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
                                egui::Stroke::new(1.0_f32, chip_border),
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
                                chip_text_color,
                            );
                            ui.add_space(6.0);
                        }
                        if let Some(health) = patch {
                            open_patch = patch_chip(ui, health);
                            ui.add_space(6.0);
                        }

                        // Status message / loading
                        if !status_msg.is_empty() {
                            let dot_color = if dark_mode {
                                egui::Color32::from_rgb(82, 82, 91)
                            } else {
                                egui::Color32::from_rgb(120, 120, 130)
                            };
                            let text_color = if dark_mode {
                                egui::Color32::from_rgb(168, 168, 176)
                            } else {
                                egui::Color32::from_rgb(70, 70, 80)
                            };

                            if is_mounted {
                                ui.label(
                                    egui::RichText::new("\u{00B7}")
                                        .monospace()
                                        .size(11.0)
                                        .color(dot_color),
                                );
                                ui.add_space(2.0);
                            }
                            // The message is cut short rather than drawn over what sits on the right of the bar.
                            let meta = format!("Schema {} · Patch {} · {} · v{}", schema_date, poe_version, game, env!("CARGO_PKG_VERSION"));
                            let meta_width = ui.painter().layout_no_wrap(meta, egui::FontId::monospace(10.5), text_color).size().x;
                            let reserved = meta_width + 56.0 + if update.is_some() { 150.0 } else { 0.0 };
                            let room = (ui.available_width() - reserved).max(60.0);
                            ui.allocate_ui_with_layout(egui::vec2(room, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let text = egui::RichText::new(status_msg).monospace().size(11.0).color(text_color);
                                ui.add(egui::Label::new(text).truncate()).on_hover_text(status_msg);
                            });
                        }
                        if is_loading {
                            ui.spinner();
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            if let Some((tag, url)) = update {
                                update_chip(ui, tag, url);
                                ui.add_space(8.0);
                            }
                            let meta_color = if dark_mode {
                                egui::Color32::from_rgb(82, 82, 91)
                            } else {
                                egui::Color32::from_rgb(100, 100, 110)
                            };

                            ui.label(
                                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new("\u{00B7}")
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new(game).monospace().size(10.5).color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new("\u{00B7}")
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new(format!("Patch {}", poe_version))
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new("\u{00B7}")
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                            ui.label(
                                egui::RichText::new(format!("Schema {}", schema_date))
                                    .monospace()
                                    .size(10.5)
                                    .color(meta_color),
                            );
                        });
                    },
                );
            });
        open_patch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every piece of text the bar paints, with where it sits and whether it was cut short.
    fn painted(update: Option<(&str, &str)>, message: &str, input: egui::RawInput) -> Vec<(String, egui::Rect, bool)> {
        let ctx = egui::Context::default();
        let output = ctx.run(input, |ctx| {
            let health = crate::ui::patch_page::Health { label: "2 tables re-fitted".to_string(), tone: crate::ui::patch_page::Tone::Warn };
            StatusBar::show(ctx, message, false, true, "PoE 2", "4.5.5.2", "2026-09-17", update, Some(&health));
        });
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect, bool)>) {
            match shape {
                egui::Shape::Text(text) => out.push((text.galley.text().to_string(), text.visual_bounding_rect(), text.galley.elided)),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        output.shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }

    fn painted_text(update: Option<(&str, &str)>) -> String {
        painted(update, "Ready", egui::RawInput::default()).into_iter().map(|(text, _, _)| text).collect()
    }

    /// The chip is the only thing a release adds; the rest of the bar is the same.
    #[test]
    fn the_update_chip_draws_only_when_there_is_a_release() {
        let quiet = painted_text(None);
        let announced = painted_text(Some(("v9.9.9", "https://example.invalid/releases/latest")));
        assert!(quiet.contains("Patch 4.5.5.2"), "the bar should still say what it always says");
        assert!(quiet.contains("2 tables re-fitted"), "the patch chip should say how the patch stands");
        assert!(!quiet.contains("available"));
        assert!(announced.contains("v9.9.9 available"));
    }

    #[test]
    fn a_long_message_is_cut_short_before_the_right_side_of_the_bar() {
        let message = "Patch 3.29.3.3: 849 tables checked, 1 moved, 1 still broken, 32 read differently by the client, and more besides";
        let narrow = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() };
        let texts = painted(None, message, narrow);
        let find = |start: &str| texts.iter().find(|(text, _, _)| text.starts_with(start)).unwrap_or_else(|| panic!("no text starting {:?}", start));
        let (_, shown, cut) = find("Patch 3.29.3.3: 849");
        let (_, schema, _) = find("Schema 2026-09-17");
        assert!(*cut, "the message should be cut short on a 900-pixel bar");
        assert!(shown.max.x <= schema.min.x, "the message ends at {} but the schema date starts at {}", shown.max.x, schema.min.x);
    }
}
