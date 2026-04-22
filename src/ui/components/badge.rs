use eframe::egui;

pub fn badge(ui: &mut egui::Ui, text: &str) {
    let fill = egui::Color32::from_rgb(20, 20, 22);
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 38, 42));

    egui::Frame::none()
        .fill(fill)
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .rounding(egui::Rounding::same(4.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(11.0)
                    .monospace()
                    .color(egui::Color32::from_rgb(168, 168, 176)),
            );
        });
}
