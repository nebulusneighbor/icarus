//! Panels the native app doesn't draw itself (custom panels written as web code) point to the
//! browser GUI for the same session, where they run.

use crate::app::Studio;
use eframe::egui::{RichText, Ui};

pub fn show(ui: &mut Ui, s: &mut Studio, kind: &str, why: &str) {
    ui.label(RichText::new(format!("\"{kind}\" {why}.")).strong());
    ui.label(
        RichText::new("It runs in the browser GUI for this same session. Anything it changes shows up here live. Native panels can be composed without code using the `controls` panel type.")
            .color(s.pal.muted),
    );
    ui.horizontal(|ui| {
        if ui.button("Open in browser").clicked() {
            s.open_web();
        }
        if ui.button("Edit source").clicked() {
            s.edit_json(&format!("/ui/components/{kind}/source"));
        }
    });
}
