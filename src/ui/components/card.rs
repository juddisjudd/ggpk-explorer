use eframe::egui;

pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
    .fill(ui.visuals().widgets.noninteractive.bg_fill)
    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
    .inner_margin(egui::Margin::same(10.0))
    .rounding(egui::Rounding::same(6.0))
        .show(ui, |ui| add_contents(ui))
        .inner
}
