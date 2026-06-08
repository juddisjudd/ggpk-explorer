use eframe::egui;

pub struct ChromeActions {
    pub open_ggpk: bool,
    pub open_steam: bool,
    pub open_settings: bool,
    pub open_about: bool,
    pub open_command_palette: bool,
    pub toggle_inspector: bool,
}

impl ChromeActions {
    fn new() -> Self {
        Self {
            open_ggpk: false,
            open_steam: false,
            open_settings: false,
            open_about: false,
            open_command_palette: false,
            toggle_inspector: false,
        }
    }
}

pub struct AppChrome;

impl AppChrome {
    fn nav_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
        let font_id = egui::FontId::proportional(13.0);
        let text_width = ui
            .fonts(|fonts| {
                fonts
                    .layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
            .max(1.0);
        let button_h = 24.0;
        let button_w = text_width + 16.0;
        let next_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(button_w, button_h));
        let is_hovered = ui
            .ctx()
            .pointer_latest_pos()
            .map(|p| next_rect.contains(p))
            .unwrap_or(false);

        let dark_mode = ui.visuals().dark_mode;
        let fill = if is_hovered {
            if dark_mode {
                egui::Color32::from_rgb(23, 23, 23)
            } else {
                egui::Color32::from_rgb(230, 230, 235)
            }
        } else {
            egui::Color32::TRANSPARENT
        };
        let text_color = if is_hovered {
            if dark_mode {
                egui::Color32::from_rgb(228, 228, 231)
            } else {
                egui::Color32::from_rgb(24, 24, 28)
            }
        } else {
            if dark_mode {
                egui::Color32::from_rgb(161, 161, 170)
            } else {
                egui::Color32::from_rgb(70, 70, 80)
            }
        };

        ui.add(
            egui::Button::new(egui::RichText::new(label).size(13.0).color(text_color))
                .fill(fill)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(4.0))
                .min_size(egui::vec2(button_w, button_h)),
        )
    }

    fn nav_button_menu(ui: &mut egui::Ui, label: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
        let prev_inactive = ui.visuals().widgets.inactive.clone();
        let prev_hovered = ui.visuals().widgets.hovered.clone();
        let prev_active = ui.visuals().widgets.active.clone();
        let prev_open = ui.visuals().widgets.open.clone();

        // Must zero BOTH bg_fill and weak_bg_fill — egui 0.29 Button uses weak_bg_fill
        // for its background rect, not bg_fill.
        let dark_mode = ui.visuals().dark_mode;
        let inactive_text = if dark_mode {
            egui::Color32::from_rgb(161, 161, 170)
        } else {
            egui::Color32::from_rgb(70, 70, 80)
        };
        let hover_bg = if dark_mode {
            egui::Color32::from_rgb(39, 39, 42)
        } else {
            egui::Color32::from_rgb(230, 230, 235)
        };
        let active_text = if dark_mode {
            egui::Color32::from_rgb(228, 228, 231)
        } else {
            egui::Color32::from_rgb(24, 24, 28)
        };

        ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.inactive.fg_stroke.color = inactive_text;

        ui.visuals_mut().widgets.hovered.bg_fill = hover_bg;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = hover_bg;
        ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.hovered.fg_stroke.color = active_text;

        ui.visuals_mut().widgets.active.bg_fill = hover_bg;
        ui.visuals_mut().widgets.active.weak_bg_fill = hover_bg;
        ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.active.fg_stroke.color = active_text;

        ui.visuals_mut().widgets.open.bg_fill = hover_bg;
        ui.visuals_mut().widgets.open.weak_bg_fill = hover_bg;
        ui.visuals_mut().widgets.open.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.open.fg_stroke.color = active_text;

        egui::menu::menu_button(ui, egui::RichText::new(label).size(13.0), add_contents);

        ui.visuals_mut().widgets.inactive = prev_inactive;
        ui.visuals_mut().widgets.hovered = prev_hovered;
        ui.visuals_mut().widgets.active = prev_active;
        ui.visuals_mut().widgets.open = prev_open;
    }

    fn win_button_close(ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(46.0, 36.0), egui::Sense::click());
        let color = if response.hovered() {
            egui::Color32::from_rgb(239, 68, 68)
        } else {
            egui::Color32::from_rgb(113, 113, 122)
        };
        let c = rect.center();
        let h = 5.0_f32;
        let stroke = egui::Stroke::new(1.5, color);
        ui.painter().line_segment([egui::pos2(c.x - h, c.y - h), egui::pos2(c.x + h, c.y + h)], stroke);
        ui.painter().line_segment([egui::pos2(c.x + h, c.y - h), egui::pos2(c.x - h, c.y + h)], stroke);
        response
    }

    fn win_button_maximize(ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(46.0, 36.0), egui::Sense::click());
        let color = if response.hovered() {
            egui::Color32::from_rgb(74, 222, 128)
        } else {
            egui::Color32::from_rgb(113, 113, 122)
        };
        let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0));
        ui.painter().rect_stroke(icon_rect, egui::Rounding::ZERO, egui::Stroke::new(1.5, color));
        response
    }

    fn win_button_minimize(ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(46.0, 36.0), egui::Sense::click());
        let color = if response.hovered() {
            egui::Color32::from_rgb(250, 204, 21)
        } else {
            egui::Color32::from_rgb(113, 113, 122)
        };
        let c = rect.center();
        ui.painter().line_segment(
            [egui::pos2(c.x - 5.0, c.y), egui::pos2(c.x + 5.0, c.y)],
            egui::Stroke::new(1.5, color),
        );
        response
    }

    fn show_location_breadcrumbs(ui: &mut egui::Ui, location: &str) {
        let parts: Vec<&str> = location.split('/').filter(|segment| !segment.is_empty()).collect();
        let dark_mode = ui.visuals().dark_mode;
        let root_color = if dark_mode {
            egui::Color32::from_rgb(228, 228, 231)
        } else {
            egui::Color32::from_rgb(24, 24, 28)
        };
        let separator_color = if dark_mode {
            egui::Color32::from_rgb(113, 113, 122)
        } else {
            egui::Color32::from_rgb(120, 120, 130)
        };
        let inactive_part = if dark_mode {
            egui::Color32::from_rgb(161, 161, 170)
        } else {
            egui::Color32::from_rgb(80, 80, 90)
        };
        let active_part = if dark_mode {
            egui::Color32::from_rgb(228, 228, 231)
        } else {
            egui::Color32::from_rgb(24, 24, 28)
        };

        if parts.len() <= 1 {
            ui.label(
                egui::RichText::new(location)
                    .size(11.5)
                    .monospace()
                    .color(root_color),
            );
            return;
        }

        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                ui.label(
                    egui::RichText::new("/")
                        .size(11.0)
                        .monospace()
                        .color(separator_color),
                );
            }
            let is_current = idx + 1 == parts.len();
            ui.label(
                egui::RichText::new(*part)
                    .size(11.5)
                    .monospace()
                    .color(if is_current {
                        active_part
                    } else {
                        inactive_part
                    }),
            );
        }
    }

    pub fn show(
        ctx: &egui::Context,
        location: &str,
        _status_msg: &str,
        _has_reader: bool,
        _is_loading: bool,
        _inspector_open: &mut bool,
    ) -> ChromeActions {
        let mut actions = ChromeActions::new();

        egui::TopBottomPanel::top("app_chrome")
            .resizable(false)
            .frame(egui::Frame {
                inner_margin: egui::Margin::same(0.0),
                fill: ctx.style().visuals.panel_fill,
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                // ── Titlebar row ─────────────────────────────────
                let titlebar_height = 36.0;
                let available_w = ui.available_width();
                let titlebar_rect = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(available_w, titlebar_height),
                );

                // Register drag sense first; buttons painted after take input priority
                let drag_resp = ui.interact(
                    titlebar_rect,
                    egui::Id::new("titlebar_drag"),
                    egui::Sense::click_and_drag(),
                );
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_resp.double_clicked() {
                    let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                }

                // Render titlebar content on top.
                // allocate_ui_with_layout gives the child UI an explicit (avail_w × 36)
                // rect so Align::Center has a real height to work with.
                ui.allocate_ui_with_layout(
                    egui::vec2(available_w, titlebar_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(12.0);
                        ui.add(
                            egui::Image::new(egui::include_image!("../../assets/icon-16x16.png"))
                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        );
                        ui.add_space(6.0);
                        let title_color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(161, 161, 170)
                        } else {
                            egui::Color32::from_rgb(70, 70, 80)
                        };
                        ui.label(
                            egui::RichText::new("GGPK Explorer")
                                .size(12.5)
                                .color(title_color),
                        );

                        ui.add_space(10.0);

                        let mut open_ggpk = false;
                        let mut open_steam = false;
                        let mut toggle_inspector = false;
                        Self::nav_button_menu(ui, "File", |ui| {
                            if ui.button("Open GGPK...").clicked() {
                                open_ggpk = true;
                                ui.close_menu();
                            }
                            if ui.button("Open Steam Folder...").clicked() {
                                open_steam = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Toggle Inspector (Ctrl+I)").clicked() {
                                toggle_inspector = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Exit").clicked() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                ui.close_menu();
                            }
                        });
                        if open_ggpk {
                            actions.open_ggpk = true;
                        }
                        if open_steam {
                            actions.open_steam = true;
                        }
                        if toggle_inspector {
                            actions.toggle_inspector = true;
                        }

                        if Self::nav_button(ui, "Settings").clicked() {
                            actions.open_settings = true;
                        }

                        if Self::nav_button(ui, "About").clicked() {
                            actions.open_about = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if Self::win_button_close(ui).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            if Self::win_button_maximize(ui).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                            }
                            if Self::win_button_minimize(ui).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                        });
                    },
                );

                ui.separator();

                // ── Path / breadcrumb bar ─────────────────────────
                let crumb_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(crumb_w, 30.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(12.0);
                        let path_label_color = if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(113, 113, 122)
                        } else {
                            egui::Color32::from_rgb(100, 100, 110)
                        };
                        ui.label(
                            egui::RichText::new("PATH")
                                .size(10.5)
                                .monospace()
                                .color(path_label_color),
                        );
                        ui.add_space(4.0);
                        Self::show_location_breadcrumbs(ui, location);
                    },
                );

                ui.separator();
            });

        actions
    }
}
