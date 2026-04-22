use eframe::egui;

#[derive(Clone, Debug)]
pub struct CommandPaletteItem {
    pub label: String,
    pub hash: u64,
}

pub struct CommandPalette {
    open: bool,
    query: String,
    selected_index: usize,
    focus_input: bool,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected_index: 0,
            focus_input: false,
        }
    }
}

impl CommandPalette {
    pub fn open(&mut self) {
        self.open = true;
        self.selected_index = 0;
        self.focus_input = true;
    }

    pub fn handle_shortcut(&mut self, ctx: &egui::Context) -> bool {
        let pressed = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K));
        if pressed {
            self.open = !self.open;
            self.selected_index = 0;
            self.focus_input = self.open;
            if !self.open {
                self.query.clear();
            }
            return true;
        }
        false
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self, ctx: &egui::Context, items: &[CommandPaletteItem]) -> Option<u64> {
        if !self.open {
            return None;
        }

        let query_lower = self.query.to_lowercase();
        let mut filtered_indices = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            if query_lower.is_empty() || item.label.to_lowercase().contains(&query_lower) {
                filtered_indices.push(idx);
            }
            if filtered_indices.len() >= 300 {
                break;
            }
        }

        if self.selected_index >= filtered_indices.len() && !filtered_indices.is_empty() {
            self.selected_index = filtered_indices.len() - 1;
        }

        let move_down = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
        let move_up = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let submit = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if close {
            self.open = false;
            self.query.clear();
            return None;
        }

        if !filtered_indices.is_empty() {
            if move_down {
                self.selected_index = (self.selected_index + 1).min(filtered_indices.len() - 1);
            }
            if move_up {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
        }

        let mut picked: Option<u64> = None;

        egui::Area::new(egui::Id::new("command_palette_overlay"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 18, 20))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(42, 42, 46)))
                    .rounding(egui::Rounding::same(12.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_width(780.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Command Palette")
                                    .size(15.0)
                                    .strong(),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                crate::ui::components::badge(ui, "Ctrl+K");
                            });
                        });
                        ui.separator();

                        let input_id = ui.make_persistent_id("command_palette_input");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id(input_id)
                                .hint_text("Type a file path or name...")
                                .desired_width(f32::INFINITY),
                        );

                        if self.focus_input {
                            response.request_focus();
                            self.focus_input = false;
                        }

                        ui.add_space(8.0);
                        if items.is_empty() {
                            ui.label("No indexed bundle files available yet.");
                            return;
                        }

                        if filtered_indices.is_empty() {
                            ui.label("No matches.");
                            return;
                        }

                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            for (row, idx) in filtered_indices.iter().enumerate() {
                                let item = &items[*idx];
                                let selected = row == self.selected_index;
                                let fill = if selected {
                                    egui::Color32::from_rgb(34, 34, 38)
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                let stroke = if selected {
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(66, 66, 74))
                                } else {
                                    egui::Stroke::NONE
                                };

                                let res = egui::Frame::none()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .rounding(egui::Rounding::same(8.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(&item.label)
                                                    .monospace()
                                                    .size(12.0),
                                            );
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(
                                                    egui::RichText::new(format!("{:016x}", item.hash))
                                                        .monospace()
                                                        .size(10.0)
                                                        .color(egui::Color32::from_rgb(122, 122, 130)),
                                                );
                                            });
                                        });
                                    })
                                    .response;
                                if res.hovered() {
                                    self.selected_index = row;
                                }
                                if res.clicked() {
                                    picked = Some(item.hash);
                                }
                            }
                        });
                    });
            });

        if submit && !filtered_indices.is_empty() {
            let idx = filtered_indices[self.selected_index];
            picked = Some(items[idx].hash);
        }

        if picked.is_some() {
            self.open = false;
            self.query.clear();
        }

        picked
    }
}
