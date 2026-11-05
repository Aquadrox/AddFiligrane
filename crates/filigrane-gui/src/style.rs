//! Apparence : thème clair, textes un peu plus grands, accent bleu.

use eframe::egui::{self, Color32, FontId, TextStyle};

const ACCENT: Color32 = Color32::from_rgb(0x2F, 0x6F, 0xE4);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(22.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Monospace, FontId::monospace(13.5)),
            (TextStyle::Small, FontId::proportional(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.slider_width = 110.0;

        let visuals = &mut style.visuals;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke.color = Color32::WHITE;
        visuals.hyperlink_color = ACCENT;
        visuals.widgets.hovered.bg_stroke.color = ACCENT;
        visuals.panel_fill = Color32::from_rgb(0xF7, 0xF8, 0xFA);
        visuals.extreme_bg_color = Color32::from_rgb(0xE9, 0xEC, 0xF1);
    });
}
