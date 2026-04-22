use eframe::egui;

#[allow(dead_code)]
pub fn search_input(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(220.0),
    )
}
