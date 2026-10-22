//! Transport bar: play/stop, position, tempo, loop, undo/redo, who's here, and edit mode.

use crate::app::{Studio, chip};
use crate::theme::client_color;
use eframe::egui::{self, Align, Button, Layout, RichText, Ui, vec2};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(
                    RichText::new("ICARUS")
                        .strong()
                        .color(p.accent)
                        .extra_letter_spacing(3.0),
                );
                ui.add_space(6.0);

                let playing = s.m.transport.playing;
                let play = Button::new(
                    RichText::new(if playing { "■" } else { "▶" })
                        .size(14.0)
                        .color(if playing { p.bg } else { p.text }),
                )
                .fill(if playing { p.accent } else { p.panel2 })
                .min_size(vec2(34.0, 24.0));
                let tip = if s.m.transport.live {
                    "Play / stop (space)"
                } else {
                    "No audio output (started with --no-live)"
                };
                if ui.add(play).on_hover_text(tip).clicked() {
                    s.action("transport.toggle");
                }

                let seconds = s.m.transport.position_s;
                let beats = seconds * s.m.project.tempo_bpm / 60.0;
                let pos = format!(
                    "{}.{}  ·  {seconds:5.2} s",
                    (beats / 4.0).floor() as i64 + 1,
                    (beats % 4.0).floor() as i64 + 1
                );
                egui::Frame::new()
                    .fill(p.bg)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(RichText::new(pos).monospace());
                    });

                number(ui, s, "BPM", "/project/tempo_bpm", 20.0..=999.0, 0.5);
                number(ui, s, "Loop s", "/project/length_s", 0.5..=3600.0, 0.1);
                if ui.button("Undo").on_hover_text("Ctrl+Z").clicked() {
                    s.action("undo");
                }
                if ui.button("Redo").on_hover_text("Ctrl+Shift+Z").clicked() {
                    s.action("redo");
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if s.editing { "Done" } else { "Edit UI" };
                    let edit = Button::new(RichText::new(label).color(if s.editing { p.bg } else { p.text }))
                        .fill(if s.editing { p.accent } else { p.panel2 });
                    if ui
                        .add(edit)
                        .on_hover_text("Rearrange and rewrite the interface (Ctrl+E)")
                        .clicked()
                    {
                        s.action("ui.edit");
                    }
                    ui.label(RichText::new(&s.link.name).color(p.muted))
                        .on_hover_text("This app's name in history and presence");

                    // Presence: whoever holds a control, plus anyone active in the last 20 seconds.
                    let mut present: BTreeMap<String, Option<String>> = BTreeMap::new();
                    for e in s.m.log.iter().filter(|e| e.at.elapsed() < Duration::from_secs(20)) {
                        present
                            .entry(e.client.split('/').next().unwrap_or("").to_string())
                            .or_default();
                    }
                    for (control, who) in &s.m.touches {
                        present.insert(who.split('/').next().unwrap_or("").to_string(), Some(control.clone()));
                    }
                    for (who, holding) in present.iter().rev() {
                        let text = if holding.is_some() {
                            format!("{who} •")
                        } else {
                            who.clone()
                        };
                        let tip = match holding {
                            Some(c) => format!("{who} is holding {c}"),
                            None => format!("{who} was active recently"),
                        };
                        chip(ui, &text, client_color(who)).on_hover_text(tip);
                    }
                    if !present.is_empty() {
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    }

                    if s.editing {
                        ui.separator();
                        for (label, path) in [
                            ("All UI", "/ui"),
                            ("Keys", "/ui/keys"),
                            ("Theme", "/ui/theme"),
                            ("Layout", "/ui/layout"),
                        ] {
                            if ui.button(label).clicked() {
                                s.edit_json(path);
                            }
                        }
                        if ui.button("+ Panel").clicked() {
                            add_panel(s);
                        }
                    }
                });
            });
        });
}

/// A number field that writes its path when an edit finishes, not on every drag step.
fn number(ui: &mut Ui, s: &mut Studio, label: &str, path: &str, range: std::ops::RangeInclusive<f64>, speed: f64) {
    let field = path.rsplit('/').next().unwrap_or("");
    let current = match field {
        "tempo_bpm" => &mut s.m.project.tempo_bpm,
        _ => &mut s.m.project.length_s,
    };
    let name = ui.label(RichText::new(label).color(s.pal.muted));
    let r = ui
        .add(egui::DragValue::new(current).range(range).speed(speed).max_decimals(2))
        .labelled_by(name.id);
    if r.drag_stopped() || r.lost_focus() {
        let value = *current;
        s.link.set(path, json!(value));
    }
}

fn add_panel(s: &mut Studio) {
    let panels = s.m.project.ui["panels"].as_object().cloned().unwrap_or_default();
    let n = (1..).find(|n| !panels.contains_key(&format!("panel{n}"))).unwrap_or(1);
    let path = format!("/ui/panels/panel{n}");
    let value = json!({ "type": "controls", "area": "bottom", "title": format!("Panel {n}"), "order": 9, "props": { "widgets": [] } });
    if s.link.set(&path, value).is_some() {
        s.edit_json(&path);
    }
}
