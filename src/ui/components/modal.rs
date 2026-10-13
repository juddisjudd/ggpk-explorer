//! The shell every dialog shares: the app dimmed behind it, a title row with a close button, padded content and a row of buttons.

use eframe::egui;

const PAD: f32 = 18.0;

pub(crate) fn shade(dark_mode: bool, dark: [u8; 3], light: [u8; 3]) -> egui::Color32 {
    let [r, g, b] = if dark_mode { dark } else { light };
    egui::Color32::from_rgb(r, g, b)
}

/// The frame of a dialog or tool window: a step lighter than the page, with a soft shadow under it.
pub fn frame(dark_mode: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(shade(dark_mode, [18, 18, 21], [255, 255, 255]))
        .stroke(egui::Stroke::new(1.0_f32, shade(dark_mode, [44, 44, 50], [208, 208, 216])))
        .rounding(egui::Rounding::same(10.0))
        .shadow(egui::Shadow {
            offset: egui::vec2(0.0, 14.0),
            blur: 40.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(if dark_mode { 150 } else { 45 }),
        })
}

/// Dims the app behind a dialog and takes the clicks that land there. Returns whether it was clicked.
pub fn scrim(ctx: &egui::Context, id: egui::Id) -> bool {
    let screen = ctx.screen_rect();
    let dark_mode = ctx.style().visuals.dark_mode;
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(if dark_mode { 150 } else { 90 }));
            response.clicked()
        })
        .inner
}

/// Draws a dialog in the middle of the window. Returns true when the user asked to close it: the close button, Escape, or a click outside.
pub fn show(ctx: &egui::Context, id: &str, title: &str, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) -> bool {
    let id = egui::Id::new(("dialog", id));
    let mut close = scrim(ctx, id.with("scrim")) || ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let screen = ctx.screen_rect();
    egui::Area::new(id).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
        frame(ui.visuals().dark_mode).show(ui, |ui| {
            ui.set_width(width.min(screen.width() - 48.0));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 7.0);
            close |= header(ui, title);
            add_contents(ui);
        });
    });
    close
}

/// A dialog's title row. Returns whether its close button was clicked.
pub fn header(ui: &mut egui::Ui, title: &str) -> bool {
    let dark_mode = ui.visuals().dark_mode;
    let mut close = false;
    egui::Frame::none().inner_margin(egui::Margin { left: PAD, right: 10.0, top: 11.0, bottom: 4.0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(15.0).color(shade(dark_mode, [244, 244, 246], [20, 20, 24])));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = close_button(ui).clicked());
        });
    });
    divider(ui);
    close
}

fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let dark_mode = ui.visuals().dark_mode;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    if response.hovered() {
        ui.painter().rect_filled(rect, 6.0, shade(dark_mode, [36, 36, 41], [232, 232, 238]));
    }
    let ink = match response.hovered() {
        true => shade(dark_mode, [244, 244, 246], [20, 20, 24]),
        false => shade(dark_mode, [150, 150, 158], [90, 90, 100]),
    };
    let (centre, reach) = (rect.center(), 4.5);
    for corner in [egui::vec2(reach, reach), egui::vec2(reach, -reach)] {
        ui.painter().line_segment([centre - corner, centre + corner], egui::Stroke::new(1.3_f32, ink));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn divider(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let line = egui::Stroke::new(1.0_f32, shade(ui.visuals().dark_mode, [34, 34, 39], [226, 226, 232]));
    ui.painter().hline(rect.x_range(), rect.top(), line);
    ui.add_space(1.0);
}

/// The sentence under a dialog's title that says what it does.
pub fn lead(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.5).color(shade(ui.visuals().dark_mode, [150, 150, 158], [90, 90, 100])));
}

/// A dialog's content, padded. It scrolls only when the window is too short to show it all.
pub fn body(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let room = (ui.ctx().screen_rect().height() - 220.0).max(160.0);
    egui::ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| {
        egui::Frame::none().inner_margin(egui::Margin { left: PAD, right: PAD, top: 6.0, bottom: 14.0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
    });
}

/// The buttons at the foot of a dialog, laid out from the right, so the main action is added first.
pub fn footer(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    divider(ui);
    egui::Frame::none().inner_margin(egui::Margin { left: PAD, right: PAD, top: 5.0, bottom: 12.0 }).show(ui, |ui| {
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            add_buttons(ui);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::button::{action, Kind};

    /// Runs a dialog for a few frames with a click where `label` is drawn; returns (asked to close, the main button was clicked).
    fn clicked(label: &str) -> (bool, bool) {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.visual_bounding_rect().center()),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        let (mut at, mut close, mut confirmed) = (None, false, false);
        // A new area spends its first frame measuring itself, so the dialog's text is on screen from the second.
        for step in 0..5 {
            let pos = at.unwrap_or(egui::Pos2::ZERO);
            let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            let events = match step {
                0 | 1 => vec![],
                2 => vec![egui::Event::PointerMoved(pos)],
                3 => vec![button(true)],
                _ => vec![button(false)],
            };
            let output = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| ui.label("the page behind"));
                close |= show(ctx, "test", "A dialog", 320.0, |ui| {
                    body(ui, |ui| lead(ui, "What it does"));
                    footer(ui, |ui| confirmed |= action(ui, "Do it", Kind::Primary, true).clicked());
                });
            });
            at = output.shapes.iter().find_map(|s| find(&s.shape, label)).or(at);
        }
        assert!(at.is_some(), "nothing drew the text {:?}", label);
        (close, confirmed)
    }

    #[test]
    fn a_dialog_runs_its_main_button_and_stays_open() {
        assert_eq!(clicked("Do it"), (false, true));
    }

    #[test]
    fn a_click_on_the_page_behind_closes_the_dialog_and_reaches_nothing() {
        assert_eq!(clicked("the page behind"), (true, false));
    }

    #[test]
    fn a_click_inside_the_dialog_does_not_close_it() {
        assert_eq!(clicked("What it does"), (false, false));
    }
}
