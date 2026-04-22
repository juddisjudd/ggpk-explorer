use eframe::egui;

pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
    .fill(egui::Color32::from_rgb(16, 16, 18))
    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(32, 32, 36)))
    .inner_margin(egui::Margin::same(10.0))
    .rounding(egui::Rounding::same(6.0))
        .show(ui, |ui| add_contents(ui))
        .inner
}
