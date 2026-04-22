use eframe::egui;

#[allow(dead_code)]
pub fn ghost_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = egui::Button::new(label)
        .fill(egui::Color32::from_rgb(21, 21, 24))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 40, 44)))
        .rounding(egui::Rounding::same(4.0))
        .min_size(egui::vec2(0.0, 24.0));
    ui.add(button)
}
