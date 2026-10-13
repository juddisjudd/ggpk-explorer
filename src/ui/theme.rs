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
        visuals.window_fill = Color::from_rgb(18, 18, 21);
        visuals.extreme_bg_color = bg_tertiary;
        visuals.faint_bg_color = bg_secondary;
        visuals.override_text_color = Some(text_primary);
        visuals.window_rounding = egui::Rounding::same(10.0);
        visuals.menu_rounding = egui::Rounding::same(8.0);
        visuals.window_shadow = egui::Shadow {
            offset: egui::vec2(0.0, 14.0),
            blur: 40.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(150),
        };
        visuals.popup_shadow = egui::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(112),
        };
        visuals.window_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(44, 44, 50));

        visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.noninteractive.bg_fill = bg_secondary;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text_muted);

        visuals.widgets.inactive.rounding = egui::Rounding::same(5.0);
        visuals.widgets.inactive.weak_bg_fill = Color::from_rgb(26, 26, 30);
        visuals.widgets.inactive.bg_fill = bg_secondary;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(44, 44, 50));
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.hovered.rounding = egui::Rounding::same(5.0);
        visuals.widgets.hovered.weak_bg_fill = Color::from_rgb(36, 36, 41);
        visuals.widgets.hovered.bg_fill = bg_hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(68, 68, 76));
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.active.rounding = egui::Rounding::same(5.0);
        visuals.widgets.active.weak_bg_fill = Color::from_rgb(46, 46, 52);
        visuals.widgets.active.bg_fill = selection;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(68, 68, 76));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.open.rounding = egui::Rounding::same(5.0);
        visuals.widgets.open.weak_bg_fill = Color::from_rgb(36, 36, 41);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(68, 68, 76));

        visuals.selection.bg_fill = selection;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, border_strong);
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

pub struct PremiumLightTheme;

impl PremiumLightTheme {
    pub fn get_visuals() -> egui::Visuals {
        let mut visuals = egui::Visuals::light();

        let bg_primary = Color::from_rgb(250, 250, 252);
        let bg_secondary = Color::from_rgb(243, 243, 246);
        let bg_tertiary = Color::from_rgb(230, 230, 235);
        let bg_hover = Color::from_rgb(236, 236, 241);
        let border = Color::from_rgb(215, 215, 222);
        let border_strong = Color::from_rgb(180, 180, 190);
        let text_primary = Color::from_rgb(24, 24, 28);
        let text_muted = Color::from_rgb(70, 70, 80);
        let selection = Color::from_rgb(220, 225, 238);

        visuals.panel_fill = bg_primary;
        visuals.window_fill = Color::from_rgb(255, 255, 255);
        visuals.extreme_bg_color = bg_tertiary;
        visuals.faint_bg_color = bg_secondary;
        visuals.override_text_color = Some(text_primary);
        visuals.window_rounding = egui::Rounding::same(10.0);
        visuals.menu_rounding = egui::Rounding::same(8.0);
        visuals.window_shadow = egui::Shadow {
            offset: egui::vec2(0.0, 14.0),
            blur: 40.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(44),
        };
        visuals.popup_shadow = egui::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(33),
        };
        visuals.window_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(206, 206, 214));

        visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        visuals.widgets.noninteractive.bg_fill = bg_secondary;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text_muted);

        visuals.widgets.inactive.rounding = egui::Rounding::same(5.0);
        visuals.widgets.inactive.weak_bg_fill = Color::from_rgb(255, 255, 255);
        visuals.widgets.inactive.bg_fill = bg_secondary;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(206, 206, 214));
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.hovered.rounding = egui::Rounding::same(5.0);
        visuals.widgets.hovered.weak_bg_fill = Color::from_rgb(240, 240, 245);
        visuals.widgets.hovered.bg_fill = bg_hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(176, 176, 186));
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.active.rounding = egui::Rounding::same(5.0);
        visuals.widgets.active.weak_bg_fill = Color::from_rgb(228, 228, 236);
        visuals.widgets.active.bg_fill = selection;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(176, 176, 186));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, text_primary);

        visuals.widgets.open.rounding = egui::Rounding::same(5.0);
        visuals.widgets.open.weak_bg_fill = Color::from_rgb(240, 240, 245);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, Color::from_rgb(176, 176, 186));

        visuals.selection.bg_fill = selection;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, border_strong);
        visuals.hyperlink_color = Color::from_rgb(50, 70, 150);

        visuals
    }

    pub fn apply_to_style(style: &mut egui::Style) {
        style.visuals = Self::get_visuals();
        for (text_style, font_id) in PremiumDarkTheme::text_styles() {
            style.text_styles.insert(text_style, font_id);
        }
        style.spacing = PremiumDarkTheme::spacing();
    }
}

type Color = egui::Color32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_creation() {
        let visuals_dark = PremiumDarkTheme::get_visuals();
        assert_ne!(visuals_dark.panel_fill, egui::Color32::WHITE);
        
        let visuals_light = PremiumLightTheme::get_visuals();
        assert_eq!(visuals_light.panel_fill, egui::Color32::from_rgb(250, 250, 252));
        
        let text_styles = PremiumDarkTheme::text_styles();
        assert_eq!(text_styles.len(), 5);
        
        let spacing = PremiumDarkTheme::spacing();
        assert!(spacing.item_spacing.x > 0.0);
    }
}
