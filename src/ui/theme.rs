use eframe::egui;
use std::collections::BTreeMap;

pub struct PremiumDarkTheme;

impl PremiumDarkTheme {
    pub fn get_visuals() -> egui::Visuals {
        let mut visuals = egui::Visuals::dark();

        let bg_primary = Color::from_rgb(12, 12, 14);
        let bg_secondary = Color::from_rgb(17, 17, 19);
        let bg_tertiary = Color::from_rgb(24, 24, 27);
        let bg_hover = Color::from_rgb(26, 26, 29);
        let border = Color::from_rgb(31, 31, 35);
        let border_strong = Color::from_rgb(42, 42, 47);
        let text_primary = Color::from_rgb(236, 236, 240);
        let text_muted = Color::from_rgb(150, 150, 158);
        let selection = Color::from_rgb(34, 34, 39);

        visuals.panel_fill = bg_primary;
        visuals.window_fill = bg_primary;
        visuals.extreme_bg_color = bg_tertiary;
        visuals.faint_bg_color = bg_secondary;
        visuals.override_text_color = Some(text_primary);
        visuals.window_rounding = egui::Rounding::same(6.0);
        visuals.window_shadow = egui::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 18.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(72),
        };
        visuals.window_stroke = egui::Stroke::new(1.0, border);

        visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.noninteractive.bg_fill = bg_secondary;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_muted);

        visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.inactive.bg_fill = bg_secondary;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text_primary);

        visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        visuals.widgets.hovered.bg_fill = bg_hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border_strong);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text_primary);

        visuals.widgets.active.rounding = egui::Rounding::same(4.0);
        visuals.widgets.active.bg_fill = selection;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, border_strong);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, text_primary);

        visuals.selection.bg_fill = selection;
        visuals.selection.stroke = egui::Stroke::new(1.0, border_strong);
        visuals.hyperlink_color = Color::from_rgb(186, 186, 196);

        visuals
    }

    pub fn text_styles() -> BTreeMap<egui::TextStyle, egui::FontId> {
        let mut styles = BTreeMap::new();
        styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.0, egui::FontFamily::Proportional),
        );
        styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(11.5, egui::FontFamily::Monospace),
        );
        styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.5, egui::FontFamily::Proportional),
        );
        styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(22.0, egui::FontFamily::Proportional),
        );
        styles
    }

    pub fn spacing() -> egui::style::Spacing {
        let mut spacing = egui::style::Spacing::default();

        spacing.item_spacing = egui::vec2(6.0, 6.0);
        spacing.window_margin = egui::Margin::same(12.0);
        spacing.button_padding = egui::vec2(8.0, 4.0);
        spacing.indent = 14.0;
        spacing.interact_size = egui::vec2(30.0, 22.0);
        spacing.combo_width = 96.0;
        spacing.text_edit_width = 220.0;

        spacing
    }

    pub fn apply_to_style(style: &mut egui::Style) {
        style.visuals = Self::get_visuals();
        for (text_style, font_id) in Self::text_styles() {
            style.text_styles.insert(text_style, font_id);
        }
        style.spacing = Self::spacing();
    }
}

type Color = egui::Color32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_creation() {
        let visuals = PremiumDarkTheme::get_visuals();
        assert_ne!(visuals.panel_fill, egui::Color32::WHITE);
        
        let text_styles = PremiumDarkTheme::text_styles();
        assert_eq!(text_styles.len(), 5); // Small, Body, Monospace, Button, Heading
        
        let spacing = PremiumDarkTheme::spacing();
        assert!(spacing.item_spacing.x > 0.0);
    }
}
