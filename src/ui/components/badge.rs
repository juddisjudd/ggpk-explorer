use eframe::egui;

pub fn badge(ui: &mut egui::Ui, text: &str) {
    let fill = ui.visuals().widgets.noninteractive.bg_fill;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;

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
                    .color(ui.visuals().weak_text_color()),
            );
        });
}
