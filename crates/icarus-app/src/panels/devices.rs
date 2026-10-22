//! Device chain of the selected track, or the selected node on its own.

use crate::app::Studio;
use crate::theme::track_color;
use crate::tracks;
use crate::widgets::knob;
use eframe::egui::{self, RichText, Stroke, Ui};
use icarus_model::Command;

enum Action {
    Move(String, isize),
    Remove(String),
    Inspect(String),
}

fn card(ui: &mut Ui, s: &mut Studio, id: &str, can_left: bool, can_right: bool, removable: bool) -> Option<Action> {
    let p = s.pal;
    let Some(node) = s.m.project.nodes.get(id) else {
        ui.label(RichText::new(format!("missing node {id}")).color(p.danger));
        return None;
    };
    let kind = node.kind.clone();
    let params: Vec<String> =
        s.m.devices
            .get(&kind)
            .map(|d| d.params.iter().map(|p| p.id.clone()).collect())
            .unwrap_or_default();
    let description =
        s.m.devices
            .get(&kind)
            .map(|d| d.description.clone())
            .unwrap_or_default();
    let selected = s.sel.node.as_deref() == Some(id);
    let mut action = None;
    egui::Frame::new()
        .fill(p.panel2)
        .stroke(Stroke::new(1.0, if selected { p.accent } else { p.border }))
        .corner_radius(4)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(capitalize(&kind)).strong())
                        .on_hover_text(description);
                    ui.label(RichText::new(id).monospace().size(10.0).color(p.muted));
                    if can_left && ui.small_button("◀").on_hover_text("Move earlier").clicked() {
                        action = Some(Action::Move(id.into(), -1));
                    }
                    if can_right && ui.small_button("▶").on_hover_text("Move later").clicked() {
                        action = Some(Action::Move(id.into(), 1));
                    }
                    if ui.small_button("{}").on_hover_text("Edit as raw paths").clicked() {
                        action = Some(Action::Inspect(id.into()));
                    }
                    if removable && ui.small_button("✕").on_hover_text("Remove").clicked() {
                        action = Some(Action::Remove(id.into()));
                    }
                });
                ui.horizontal(|ui| {
                    for param in &params {
                        knob(ui, s, id, param, None);
                    }
                });
            });
        });
    action
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    let track_index = s.sel.track;
    let track = track_index.and_then(|i| s.m.tracks().get(i).cloned());
    let mut action = None;

    ui.horizontal_top(|ui| {
        if let (Some(index), Some(track)) = (track_index, &track) {
            let n = track.devices.len();
            for (i, id) in track.devices.iter().enumerate() {
                if let Some(a) = card(ui, s, id, i > 0, i + 1 < n, true) {
                    action = Some(a);
                }
                ui.label(RichText::new("→").color(p.muted));
            }
            let mut choice = String::new();
            egui::ComboBox::from_id_salt(("add-device", index))
                .selected_text("+ Add device")
                .show_ui(ui, |ui| {
                    for kind in s.m.devices.keys().filter(|k| *k != "output") {
                        ui.selectable_value(&mut choice, kind.clone(), kind);
                    }
                });
            if !choice.is_empty()
                && let Some(cmds) = tracks::add_device(&s.m, index, &choice)
            {
                s.link.tx(format!("add {choice} to {}", track.name), cmds);
            }
            ui.label(RichText::new("→").color(p.muted));
            egui::Frame::new()
                .stroke(Stroke::new(1.0, track_color(&track.color, p.border)))
                .corner_radius(4)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.label(RichText::new(format!("{} mix", track.name)).color(p.muted))
                });
        } else if let Some(id) = s.sel.node.clone().filter(|id| s.m.project.nodes.contains_key(id)) {
            action = card(ui, s, &id, false, false, id != "master");
        } else {
            ui.label(
                RichText::new("Select a track in the Mixer or a node in the Graph to see its devices.").color(p.muted),
            );
        }
    });

    match action {
        Some(Action::Move(id, delta)) => {
            if let Some(cmds) = track_index.and_then(|i| tracks::move_device(&s.m, i, &id, delta)) {
                s.link.tx("reorder devices", cmds);
            }
        }
        Some(Action::Remove(id)) => {
            let cmds = match track_index {
                Some(i) => tracks::remove_device(&s.m, i, &id),
                None => Some(vec![Command::RemoveNode { id: id.clone() }]),
            };
            if let Some(cmds) = cmds {
                s.link.tx(format!("remove {id}"), cmds);
            }
        }
        Some(Action::Inspect(id)) => s.edit_json(&format!("/nodes/{id}")),
        None => {}
    }
}
