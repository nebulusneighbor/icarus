//! Browser: add tracks and devices, see custom panels, save and load.

use super::section;
use crate::app::{Prompt, PromptAction, Studio};
use crate::tracks;
use eframe::egui::{self, RichText, Ui};
use icarus_model::Command;
use std::collections::BTreeMap;

fn add_track(s: &mut Studio, instrument: Option<&str>) {
    let (label, cmds) = tracks::add_track(&s.m, instrument);
    let index = s.m.tracks().len();
    if s.link.tx(label, cmds).is_some() {
        s.select(Some(index), None);
    }
}

fn add_device(s: &mut Studio, kind: &str) {
    if let Some(t) = s.sel.track {
        if let Some(cmds) = tracks::add_device(&s.m, t, kind) {
            s.link.tx(format!("add {kind}"), cmds);
        }
    } else if s.m.tracks().is_empty() {
        add_track(s, Some(kind));
    } else {
        // Nothing selected: a free-standing node, to patch by hand in the graph.
        let id = tracks::unique_id(&s.m, kind);
        let cmd = Command::AddNode {
            id: id.clone(),
            kind: kind.into(),
            params: BTreeMap::new(),
        };
        if s.link.tx(format!("add {id}"), vec![cmd]).is_some() {
            s.select(None, Some(id));
        }
    }
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    section(ui, "Tracks", p.muted);
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Synth track").clicked() {
            add_track(s, Some("osc"));
        }
        if ui.button("+ Noise track").clicked() {
            add_track(s, Some("noise"));
        }
        if ui.button("+ Empty").clicked() {
            add_track(s, None);
        }
    });

    let target = match (s.sel.track, &s.sel.node) {
        (Some(t), _) => {
            s.m.tracks()
                .get(t)
                .map(|t| format!("to {}", t.name))
                .unwrap_or_default()
        }
        (None, Some(_)) => "as a free node".into(),
        _ => String::new(),
    };
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("DEVICES").size(10.0).color(p.muted));
        ui.label(RichText::new(target).size(10.0).color(p.accent));
    });
    let kinds: Vec<(String, String)> =
        s.m.devices
            .values()
            .filter(|d| d.kind != "output")
            .map(|d| (d.kind.clone(), d.description.clone()))
            .collect();
    for (kind, description) in kinds {
        let r = ui
            .add(egui::Button::new(RichText::new(&kind).strong()).frame(false))
            .on_hover_text(&description);
        ui.label(RichText::new(&description).size(11.0).color(p.muted));
        if r.clicked() {
            add_device(s, &kind);
        }
    }

    section(ui, "Custom panels", p.muted);
    let components: Vec<(String, String)> = s.m.project.ui["components"]
        .as_object()
        .map(|c| {
            c.iter()
                .map(|(k, v)| (k.clone(), v["description"].as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    if components.is_empty() {
        ui.label(RichText::new("None yet").color(p.muted));
    }
    for (name, description) in components {
        if ui
            .add(egui::Button::new(RichText::new(&name).strong()).frame(false))
            .on_hover_text("Edit its source")
            .clicked()
        {
            s.edit_json(&format!("/ui/components/{name}/source"));
        }
        ui.label(RichText::new(description).size(11.0).color(p.muted));
    }

    section(ui, "Project", p.muted);
    ui.horizontal(|ui| {
        if ui.button("Save…").clicked() {
            s.prompt = Some(Prompt {
                title: "Save project to".into(),
                text: "song.icarus.json".into(),
                action: PromptAction::Save,
            });
        }
        if ui.button("Load…").clicked() {
            s.prompt = Some(Prompt {
                title: "Load project from".into(),
                text: "song.icarus.json".into(),
                action: PromptAction::Load,
            });
        }
    });
}
