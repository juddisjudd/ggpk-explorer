use crate::settings::Game;
use eframe::egui;

/// What the title bar's game switch shows.
pub struct GameSwitch {
    /// The game being read, or the one being opened.
    pub game: Game,
    pub loaded: bool,
    pub busy: bool,
    /// Whether an install is saved for PoE 1 and for PoE 2, in that order.
    pub saved: [bool; 2],
}

pub struct ChromeActions {
    pub switch_game: Option<Game>,
    pub open_ggpk: bool,
    pub open_steam: bool,
    pub open_settings: bool,
    pub open_home: bool,
    pub open_search: bool,
    pub open_patch: bool,
    pub open_diff: bool,
    pub export_data: bool,
    pub export_pob: bool,
    pub export_tables: bool,
    pub open_about: bool,
    pub open_command_palette: bool,
    pub toggle_inspector: bool,
}

impl ChromeActions {
    fn new() -> Self {
        Self {
            switch_game: None,
            open_ggpk: false,
            open_steam: false,
            open_settings: false,
            open_home: false,
            open_search: false,
            open_patch: false,
            open_diff: false,
            export_data: false,
            export_pob: false,
            export_tables: false,
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

    /// Both games side by side with the one being read lit, so the other is one click away on every page.
    fn game_switch(ui: &mut egui::Ui, games: &GameSwitch) -> Option<Game> {
        let dark_mode = ui.visuals().dark_mode;
        let shade = |dark: u8, light: u8| egui::Color32::from_gray(if dark_mode { dark } else { light });
        let mut asked = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for game in [Game::Poe1, Game::Poe2] {
                let lit = games.loaded && games.game == game;
                let saved = games.saved[usize::from(game.is_poe2())];
                let text = match (lit, saved) {
                    (true, _) => shade(244, 20),
                    (false, true) => shade(161, 75),
                    (false, false) => shade(96, 160),
                };
                let hint = match (lit, saved) {
                    (true, _) => format!("Reading {}", game.label()),
                    (false, true) => format!("Switch to {}", game.label()),
                    (false, false) => format!("No {} install is saved. Click to set its path.", game.label()),
                };
                let button = egui::Button::new(egui::RichText::new(game.label()).size(12.5).color(text))
                    .fill(if lit { shade(46, 222) } else { egui::Color32::TRANSPARENT })
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::same(4.0))
                    .min_size(egui::vec2(48.0, 24.0));
                let behind = ui.painter().add(egui::Shape::Noop);
                let response = ui.add_enabled(!games.busy, button).on_hover_text(hint);
                if response.hovered() && !lit {
                    ui.painter().set(behind, egui::Shape::rect_filled(response.rect, 4.0, shade(23, 232)));
                }
                if response.clicked() && !lit {
                    asked = Some(game);
                }
            }
        });
        asked
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
        let stroke = egui::Stroke::new(1.5_f32, color);
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
        ui.painter().rect_stroke(icon_rect, egui::Rounding::ZERO, egui::Stroke::new(1.5_f32, color));
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
            egui::Stroke::new(1.5_f32, color),
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
        games: &GameSwitch,
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
                        actions.switch_game = Self::game_switch(ui, games);
                        ui.add_space(6.0);

                        let mut open_ggpk = false;
                        let mut open_steam = false;
                        let mut export_data = false;
                        let mut export_pob = false;
                        let mut export_tables = false;
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
                            if ui
                                .button("Export Game Data...")
                                .on_hover_text(
                                    "Write the semantic JSON dumps: mods, skills, base items, stat translations",
                                )
                                .clicked()
                            {
                                export_data = true;
                                ui.close_menu();
                            }
                            if ui
                                .button("Export PoB Data...")
                                .on_hover_text(
                                    "Write Path of Building's data files (bases, skills, gems, mods, minions, stat descriptions) as pob-data JSON",
                                )
                                .clicked()
                            {
                                export_pob = true;
                                ui.close_menu();
                            }
                            if ui
                                .button("Export DAT Tables...")
                                .on_hover_text("Write every DAT table whole, as JSON and/or CSV, with foreign keys named by their target row's key")
                                .clicked()
                            {
                                export_tables = true;
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
                        if export_data {
                            actions.export_data = true;
                        }
                        if export_pob {
                            actions.export_pob = true;
                        }
                        if export_tables {
                            actions.export_tables = true;
                        }
                        if toggle_inspector {
                            actions.toggle_inspector = true;
                        }

                        if Self::nav_button(ui, "Home").clicked() {
                            actions.open_home = true;
                        }

                        if Self::nav_button(ui, "Search").on_hover_text("Find text inside the game's files (Ctrl+Shift+F)").clicked() {
                            actions.open_search = true;
                        }

                        if Self::nav_button(ui, "Patch").clicked() {
                            actions.open_patch = true;
                        }

                        if Self::nav_button(ui, "Diff").clicked() {
                            actions.open_diff = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the switch, clicks the game labelled `label`, and returns what the switch asked for.
    fn clicked(games: &GameSwitch, label: &str) -> Option<Game> {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.visual_bounding_rect().center()),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        let (mut at, mut asked) = (egui::Pos2::ZERO, None);
        for step in 0..4 {
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            let events = match step {
                0 => vec![],
                1 => vec![egui::Event::PointerMoved(at)],
                2 => vec![button(true)],
                _ => vec![button(false)],
            };
            let output = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| asked = asked.or(AppChrome::game_switch(ui, games)));
            });
            at = output.shapes.iter().find_map(|s| find(&s.shape, label)).expect("the switch did not draw this game");
        }
        asked
    }

    #[test]
    fn the_switch_asks_for_the_game_that_is_not_being_read() {
        let reading_poe1 = GameSwitch { game: Game::Poe1, loaded: true, busy: false, saved: [true, true] };
        assert_eq!(clicked(&reading_poe1, "PoE 2"), Some(Game::Poe2));
        assert_eq!(clicked(&reading_poe1, "PoE 1"), None);
    }

    #[test]
    fn a_game_with_no_install_can_still_be_asked_for() {
        let only_poe2 = GameSwitch { game: Game::Poe2, loaded: true, busy: false, saved: [false, true] };
        assert_eq!(clicked(&only_poe2, "PoE 1"), Some(Game::Poe1));
        let nothing_open = GameSwitch { game: Game::Poe2, loaded: false, busy: false, saved: [false, true] };
        assert_eq!(clicked(&nothing_open, "PoE 2"), Some(Game::Poe2));
    }

    #[test]
    fn the_switch_waits_while_a_load_or_export_runs() {
        let busy = GameSwitch { game: Game::Poe1, loaded: true, busy: true, saved: [true, true] };
        assert_eq!(clicked(&busy, "PoE 2"), None);
    }
}
