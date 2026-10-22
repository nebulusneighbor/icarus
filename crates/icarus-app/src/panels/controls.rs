//! Declarative panels: an agent (or person) composes a panel out of native widgets by writing
//! data, no code needed. The browser GUI renders the same `controls` panels.
//!
//! ```json
//! {"type": "controls", "area": "bottom", "title": "Macros", "props": {"widgets": [
//!   {"knob": "t1_osc.freq", "label": "Pitch"},
//!   {"fader": "t1_mix.gain"},
//!   {"xy": ["t1_filter.cutoff", "t1_filter.resonance"]},
//!   {"label": "Section B"},
//!   {"button": "Drop", "request": {"type": "set", "path": "/nodes/t1_filter/params/cutoff", "value": 200}},
//!   {"row": [ ...widgets ]}, {"column": [ ...widgets ]}
//! ]}}
//! ```

use crate::app::Studio;
use crate::widgets::{fader, knob, xy};
use eframe::egui::{RichText, Ui};
use icarus_model::Request;
use serde_json::Value;

fn split(target: &str) -> Option<(String, String)> {
    target.split_once('.').map(|(n, p)| (n.to_string(), p.to_string()))
}

fn widget(ui: &mut Ui, s: &mut Studio, w: &Value) {
    let p = s.pal;
    if let Some((node, param)) = w["knob"].as_str().and_then(split) {
        if s.m.spec(&node, &param).is_some() {
            knob(ui, s, &node, &param, w["label"].as_str());
        } else {
            ui.label(RichText::new(format!("no {node}.{param}")).color(p.danger));
        }
    } else if let Some((node, param)) = w["fader"].as_str().and_then(split) {
        if s.m.spec(&node, &param).is_some() {
            fader(
                ui,
                s,
                &node,
                &param,
                w["height"].as_f64().unwrap_or(160.0) as f32,
                p.accent,
            );
        } else {
            ui.label(RichText::new(format!("no {node}.{param}")).color(p.danger));
        }
    } else if let Some(axes) = w["xy"].as_array() {
        let x = axes.first().and_then(Value::as_str).unwrap_or("");
        let y = axes.get(1).and_then(Value::as_str).unwrap_or("");
        xy(ui, s, x, y, w["size"].as_f64().unwrap_or(180.0) as f32);
    } else if let Some(text) = w["label"].as_str() {
        ui.label(RichText::new(text).strong());
    } else if let Some(text) = w["button"].as_str() {
        if ui.button(text).clicked() {
            match serde_json::from_value::<Request>(w["request"].clone()) {
                Ok(request) => {
                    s.link.run(request);
                }
                Err(e) => s.link.toast(format!("button `{text}`: {e}")),
            }
        }
    } else if let Some(items) = w["row"].as_array() {
        ui.horizontal_top(|ui| items.iter().for_each(|i| widget(ui, s, i)));
    } else if let Some(items) = w["column"].as_array() {
        ui.vertical(|ui| items.iter().for_each(|i| widget(ui, s, i)));
    } else {
        ui.label(RichText::new(format!("unknown widget {w}")).color(p.danger));
    }
}

pub fn show(ui: &mut Ui, s: &mut Studio, id: &str, panel: &Value) {
    let widgets = panel["props"]["widgets"].as_array().cloned().unwrap_or_default();
    if widgets.is_empty() {
        ui.label(
            RichText::new(
                "An empty controls panel. Fill props.widgets with knobs, faders, XY pads, labels and buttons:",
            )
            .color(s.pal.muted),
        );
        ui.label(RichText::new(r#"[{"knob": "node.param"}, {"fader": "node.param"}, {"xy": ["a.p", "b.q"]}, {"button": "Go", "request": {...}}]"#).monospace().size(11.0));
        if ui.button("Edit this panel").clicked() {
            s.edit_json(&format!("/ui/panels/{id}"));
        }
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for w in &widgets {
            widget(ui, s, w);
        }
    });
}
