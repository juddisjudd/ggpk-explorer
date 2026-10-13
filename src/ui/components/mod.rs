pub mod badge;
pub mod button;
pub mod card;
pub mod input;
pub mod modal;

pub use badge::badge;
pub use card::card;

/// Hint text for an empty field, dim enough not to read as a value.
pub fn hint(ui: &eframe::egui::Ui, text: &str) -> eframe::egui::RichText {
    eframe::egui::RichText::new(text).color(modal::shade(ui.visuals().dark_mode, [104, 104, 112], [150, 150, 160]))
}

/// A count with a comma between each three digits.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let groups: Vec<&str> = digits.as_bytes().rchunks(3).rev().map(|group| std::str::from_utf8(group).unwrap_or_default()).collect();
    groups.join(",")
}

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
