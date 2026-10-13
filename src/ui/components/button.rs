use super::modal::shade;
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
}

/// A dialog button drawn by hand, so the main action stands out and hover, press and disabled each show.
pub fn action(ui: &mut egui::Ui, label: &str, kind: Kind, enabled: bool) -> egui::Response {
    let dark_mode = ui.visuals().dark_mode;
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), egui::Color32::PLACEHOLDER);
    let size = egui::vec2((galley.size().x + 28.0).max(76.0), 30.0);
    let (rect, response) = ui.allocate_exact_size(size, if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let (hovered, pressed) = (response.hovered(), response.is_pointer_button_down_on());
        let (fill, ink, edge) = match (kind, enabled) {
            (_, false) => (shade(dark_mode, [30, 30, 34], [236, 236, 240]), shade(dark_mode, [104, 104, 112], [150, 150, 160]), None),
            (Kind::Primary, true) => {
                let fill = match (pressed, hovered) {
                    (true, _) => shade(dark_mode, [205, 205, 212], [52, 52, 60]),
                    (false, true) => shade(dark_mode, [255, 255, 255], [40, 40, 47]),
                    (false, false) => shade(dark_mode, [236, 236, 240], [24, 24, 28]),
                };
                (fill, shade(dark_mode, [12, 12, 14], [250, 250, 252]), None)
            }
            (Kind::Secondary, true) => {
                let fill = match (pressed, hovered) {
                    (true, _) => shade(dark_mode, [40, 40, 46], [226, 226, 232]),
                    (false, true) => shade(dark_mode, [30, 30, 34], [240, 240, 244]),
                    (false, false) => egui::Color32::TRANSPARENT,
                };
                (fill, shade(dark_mode, [236, 236, 240], [24, 24, 28]), Some(shade(dark_mode, [52, 52, 59], [200, 200, 208])))
            }
        };
        let stroke = edge.map_or(egui::Stroke::NONE, |colour| egui::Stroke::new(1.0_f32, colour));
        ui.painter().rect(rect, 6.0, fill, stroke);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
    match enabled {
        true => response.on_hover_cursor(egui::CursorIcon::PointingHand),
        false => response,
    }
}
