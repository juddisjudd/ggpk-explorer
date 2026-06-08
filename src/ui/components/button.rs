use eframe::egui;

#[allow(dead_code)]
pub fn ghost_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = egui::Button::new(label)
        .fill(ui.visuals().widgets.inactive.bg_fill)
        .stroke(ui.visuals().widgets.inactive.bg_stroke)
        .rounding(egui::Rounding::same(4.0))
        .min_size(egui::vec2(0.0, 24.0));
    ui.add(button)
}
