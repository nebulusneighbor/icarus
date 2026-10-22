//! Colours from /ui/theme, the same CSS variables the browser GUI uses.

use eframe::egui::{self, Color32, CornerRadius, Stroke};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub panel3: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent2: Color32,
    pub danger: Color32,
}

fn color(theme: &Value, key: &str, fallback: &str) -> Color32 {
    let hex = theme[key].as_str().unwrap_or(fallback);
    Color32::from_hex(hex.trim()).unwrap_or_else(|_| Color32::from_hex(fallback).unwrap_or(Color32::GRAY))
}

impl Palette {
    pub fn from_ui(ui: &Value) -> Self {
        let t = &ui["theme"];
        Self {
            bg: color(t, "--bg", "#14161a"),
            panel: color(t, "--panel", "#1d2026"),
            panel2: color(t, "--panel-2", "#262a32"),
            panel3: color(t, "--panel-3", "#30353f"),
            border: color(t, "--border", "#363b46"),
            text: color(t, "--text", "#d9dce3"),
            muted: color(t, "--muted", "#8b91a1"),
            accent: color(t, "--accent", "#f0a236"),
            accent2: color(t, "--accent-2", "#4fb3d9"),
            danger: color(t, "--danger", "#e5534b"),
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(egui::Theme::Dark);
        let p = *self;
        ctx.all_styles_mut(move |style| {
            let v = &mut style.visuals;
            v.dark_mode = true;
            v.panel_fill = p.panel;
            v.window_fill = p.panel;
            v.extreme_bg_color = p.bg;
            v.faint_bg_color = p.panel2;
            v.override_text_color = Some(p.text);
            v.hyperlink_color = p.accent2;
            v.selection.bg_fill = p.accent.linear_multiply(0.45);
            v.selection.stroke = Stroke::new(1.0, p.text);
            v.window_stroke = Stroke::new(1.0, p.border);
            let r = CornerRadius::same(4);
            for (w, fill) in [
                (&mut v.widgets.noninteractive, p.panel),
                (&mut v.widgets.inactive, p.panel2),
                (&mut v.widgets.hovered, p.panel3),
                (&mut v.widgets.active, p.panel3),
                (&mut v.widgets.open, p.panel3),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.bg_stroke = Stroke::new(1.0, p.border);
                w.fg_stroke = Stroke::new(1.0, p.text);
                w.corner_radius = r;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.muted);
            v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
            style.spacing.item_spacing = egui::vec2(6.0, 5.0);
            style.spacing.button_padding = egui::vec2(8.0, 3.0);
        });
    }
}

/// A stable colour per client name, matching the browser GUI's.
pub fn client_color(name: &str) -> Color32 {
    let hash = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let hue = (hash % 360) as f32 / 360.0;
    let hsva = egui::ecolor::Hsva::from(egui::ecolor::HsvaGamma {
        h: hue,
        s: 0.6,
        v: 0.95,
        a: 1.0,
    });
    Color32::from(hsva)
}

pub fn track_color(hex: &str, fallback: Color32) -> Color32 {
    Color32::from_hex(hex).unwrap_or(fallback)
}
