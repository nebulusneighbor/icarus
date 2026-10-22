//! Inspector: the whole session as its path tree, read through `list` and `get` exactly like
//! an agent or OSC surface would. Edit any value as JSON and press Enter to write it.

use crate::app::Studio;
use eframe::egui::{self, RichText, Ui};
use icarus_model::Request;
use serde_json::Value;

fn children(s: &mut Studio, path: &str) -> Option<Vec<String>> {
    if let Some(cached) = s.inspector.children.get(path) {
        return cached.clone();
    }
    let listed = s
        .link
        .request(Request::List { path: path.into() })
        .ok()
        .and_then(|v| serde_json::from_value(v).ok());
    s.inspector.children.insert(path.into(), listed.clone());
    listed
}

fn value(s: &mut Studio, path: &str) -> Option<Value> {
    if let Some(cached) = s.inspector.values.get(path) {
        return cached.clone();
    }
    let v = s.link.get(path).ok();
    s.inspector.values.insert(path.into(), v.clone());
    v
}

/// An editable value, named for assistive technology by the row's label `label`.
fn editable(ui: &mut Ui, s: &mut Studio, path: &str, v: &Value, label: egui::Id) {
    let shown = match v {
        Value::String(t) if t.contains('\n') => "(multi-line: click edit)".to_string(),
        other => other.to_string(),
    };
    let mut text = s.inspector.edits.get(path).cloned().unwrap_or(shown);
    let r = ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .font(egui::TextStyle::Monospace)
                .desired_width(ui.available_width() - 30.0),
        )
        .labelled_by(label);
    if r.has_focus() {
        s.inspector.edits.insert(path.into(), text.clone());
    }
    if r.lost_focus() {
        s.inspector.edits.remove(path);
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let next = serde_json::from_str(&text).unwrap_or(Value::String(text));
            s.link.set(path, next);
        }
    }
    if ui.small_button("edit").on_hover_text("Open in editor").clicked() {
        s.edit_json(path);
    }
}

fn row(ui: &mut Ui, s: &mut Studio, path: &str, depth: usize) {
    let name = if path == "/" {
        "/".to_string()
    } else {
        path.rsplit('/').next().unwrap_or("").to_string()
    };
    let p = s.pal;
    match children(s, path) {
        Some(kids) => {
            let open = s.inspector.open.contains(path);
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.0);
                let label = format!("{} {name}", if open { "▾" } else { "▸" });
                let toggle = ui.add(egui::Button::new(RichText::new(label).monospace().color(p.accent2)).frame(false));
                if toggle.clicked() {
                    if open {
                        s.inspector.open.remove(path);
                    } else {
                        s.inspector.open.insert(path.into());
                    }
                }
                ui.label(RichText::new(kids.len().to_string()).color(p.muted).size(10.0));
                // Some branches also hold a value, e.g. a param has normalized/spec children.
                if path != "/"
                    && let Some(v) = value(s, path).filter(|v| !v.is_object() && !v.is_array())
                {
                    editable(ui, s, path, &v, toggle.id);
                }
            });
            if open {
                for kid in kids {
                    row(ui, s, &kid, depth + 1);
                }
            }
        }
        None => {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.0 + 12.0);
                let label = ui.label(RichText::new(&name).monospace().color(p.accent2));
                match value(s, path) {
                    Some(v) => editable(ui, s, path, &v, label.id),
                    None => {
                        ui.label(RichText::new("unreadable").color(p.danger));
                    }
                }
            });
        }
    }
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    if s.inspector.open.is_empty() {
        s.inspector.open.insert("/".into());
    }
    row(ui, s, "/", 0);
}
