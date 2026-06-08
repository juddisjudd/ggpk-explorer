pub mod badge;
pub mod button;
pub mod card;
pub mod input;

pub use badge::badge;
pub use card::card;

pub fn modal_section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(6.0);
    let color = if ui.visuals().dark_mode {
        eframe::egui::Color32::from_rgb(113, 113, 122)
    } else {
        eframe::egui::Color32::from_rgb(80, 80, 90)
    };
    ui.label(
        eframe::egui::RichText::new(label)
            .size(10.5)
            .monospace()
            .color(color),
    );
    ui.add_space(1.0);
}
