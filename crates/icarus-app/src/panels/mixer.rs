//! Mixer: one strip per track in /ui/tracks, with the track's mix gain as the fader.

use crate::app::{Prompt, PromptAction, Studio};
use crate::theme::track_color;
use crate::tracks;
use crate::widgets::{fader, knob};
use eframe::egui::{self, Align, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, vec2};

const STRIP: f32 = 100.0;

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    let height = (ui.available_height() - 4.0).max(260.0);
    ui.horizontal_top(|ui| {
        for (i, track) in s.m.tracks().into_iter().enumerate() {
            let color = track_color(&track.color, p.accent);
            let (rect, resp) = ui.allocate_exact_size(vec2(STRIP, height), Sense::click());
            let selected = s.sel.track == Some(i);
            ui.painter()
                .rect_filled(rect, 4.0, if selected { p.panel3 } else { p.panel2 });
            ui.painter()
                .rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)), 2.0, color);
            if selected {
                ui.painter()
                    .rect_stroke(rect, 4.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
            }
            if resp.clicked() {
                s.select(Some(i), None);
            }

            let mut strip = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect.shrink(6.0))
                    .id_salt(("strip", i))
                    .layout(Layout::top_down(Align::Center)),
            );
            strip.add_space(4.0);
            let name = strip.add(
                egui::Label::new(RichText::new(&track.name).strong())
                    .sense(Sense::click())
                    .truncate(),
            );
            if name.double_clicked() {
                s.prompt = Some(Prompt {
                    title: "Track name".into(),
                    text: track.name.clone(),
                    action: PromptAction::RenameTrack(i),
                });
            } else if name.clicked() {
                s.select(Some(i), None);
            }
            let chain: Vec<String> = track
                .devices
                .iter()
                .map(|d| s.m.project.nodes.get(d).map(|n| n.kind.clone()).unwrap_or("?".into()))
                .collect();
            strip.label(
                RichText::new(if chain.is_empty() {
                    "no devices".into()
                } else {
                    chain.join(" → ")
                })
                .size(10.0)
                .color(p.muted),
            );
            if s.m.project.nodes.contains_key(&track.mix) {
                knob(&mut strip, s, &track.mix, "pan", Some("Pan"));
                let remaining = strip.available_height() - 30.0;
                fader(&mut strip, s, &track.mix, "gain", remaining, color);
            } else {
                strip.label(RichText::new(format!("missing {}", track.mix)).color(p.danger));
            }
            if strip
                .small_button("✕")
                .on_hover_text("Delete track and its devices")
                .clicked()
                && let Some(cmds) = tracks::delete_track(&s.m, i)
            {
                s.link.tx(format!("delete {}", track.name), cmds);
            }
        }

        let (rect, _) = ui.allocate_exact_size(vec2(STRIP, height), Sense::hover());
        ui.painter().rect_filled(rect, 4.0, p.panel2);
        ui.painter()
            .rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)), 2.0, p.accent);
        let mut master = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(6.0))
                .layout(Layout::top_down(Align::Center)),
        );
        master.add_space(4.0);
        master.label(RichText::new("Master").strong());
        master.label(RichText::new("output").size(10.0).color(p.muted));

        let (rect, resp) = ui.allocate_exact_size(vec2(STRIP, height), Sense::click());
        ui.painter()
            .rect_stroke(rect, 4.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "+ Track",
            egui::FontId::proportional(12.0),
            p.muted,
        );
        if resp.clicked() {
            let (label, cmds) = tracks::add_track(&s.m, Some("osc"));
            let index = s.m.tracks().len();
            if s.link.tx(label, cmds).is_some() {
                s.select(Some(index), None);
            }
        }
    });
}
