//! The window: builds itself every frame from the /ui document, exactly like the browser GUI.

use crate::layout;
use crate::link::Link;
use crate::mirror::Mirror;
use crate::panels;
use crate::theme::Palette;
use eframe::egui::{self, Align, Align2, Color32, Event, Layout, RichText, Sense, Stroke, Ui, UiBuilder, vec2};
use icarus_model::{Command, Request};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// How often the playhead is refreshed while playing.
const POLL: Duration = Duration::from_millis(60);

/// A knob or fader currently being dragged by this app.
pub struct Drag {
    pub key: String,
    pub start: f64,
    pub travel: f32,
}

#[derive(Default, Clone)]
pub struct Selection {
    pub track: Option<usize>,
    pub node: Option<String>,
}

/// An open editor for any path, as JSON (or raw text for multi-line strings such as code).
pub struct JsonEditor {
    pub path: String,
    pub text: String,
    pub raw: bool,
    pub open: bool,
}

pub enum PromptAction {
    Save,
    Load,
    RenameTrack(usize),
}

pub struct Prompt {
    pub title: String,
    pub text: String,
    pub action: PromptAction,
}

#[derive(Default)]
pub struct InspectorState {
    pub open: BTreeSet<String>,
    pub children: HashMap<String, Option<Vec<String>>>,
    pub values: HashMap<String, Option<Value>>,
    pub edits: HashMap<String, String>,
}

/// Everything panels can reach.
pub struct Studio {
    pub link: Link,
    pub m: Mirror,
    pub pal: Palette,
    pub sel: Selection,
    pub drag: Option<Drag>,
    pub editing: bool,
    pub tabs: HashMap<String, String>,
    pub editors: Vec<JsonEditor>,
    pub prompt: Option<Prompt>,
    pub inspector: InspectorState,
    pub log_filter: String,
    pub log_open: BTreeSet<u64>,
    pub graph_drag: Option<panels::graph::GraphDrag>,
    pub lane: Option<String>,
    pub lane_drag: Option<usize>,
    /// Where the browser GUI for this session lives, if it is being served.
    pub web_url: Option<String>,
    last_poll: Instant,
    applied_theme: Option<Palette>,
}

impl Studio {
    pub fn new(mut link: Link, web_url: Option<String>) -> Result<Self, String> {
        let m = Mirror::load(&mut link)?;
        Ok(Self {
            pal: Palette::from_ui(&m.project.ui),
            link,
            m,
            sel: Selection::default(),
            drag: None,
            editing: false,
            tabs: HashMap::new(),
            editors: Vec::new(),
            prompt: None,
            inspector: InspectorState::default(),
            log_filter: String::new(),
            log_open: BTreeSet::new(),
            graph_drag: None,
            lane: None,
            lane_drag: None,
            web_url,
            last_poll: Instant::now(),
            applied_theme: None,
        })
    }

    pub fn frame(&mut self, ui: &mut Ui) {
        self.sync(ui.ctx());
        self.keys(ui);

        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, self.pal.border);
        let areas = layout::areas(&self.m.project.ui["layout"], full);
        let mut by_area: HashMap<String, Vec<(String, Value)>> = HashMap::new();
        if let Some(panels) = self.m.project.ui["panels"].as_object() {
            for (id, panel) in panels {
                let area = panel["area"].as_str().unwrap_or("").to_string();
                by_area.entry(area).or_default().push((id.clone(), panel.clone()));
            }
        }
        for list in by_area.values_mut() {
            list.sort_by(|a, b| {
                a.1["order"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&b.1["order"].as_f64().unwrap_or(0.0))
            });
        }
        for (name, rect) in areas {
            let panels = by_area.remove(&name).unwrap_or_default();
            self.area(ui, &name, rect, panels);
        }

        let ctx = ui.ctx().clone();
        self.editors_ui(&ctx);
        self.prompt_ui(&ctx);
        self.toasts_ui(&ctx);
    }

    /// Applies queued events, reloads when needed, polls the playhead, follows the theme.
    fn sync(&mut self, ctx: &egui::Context) {
        let mut structural = false;
        for event in self.link.drain() {
            let applied = self.m.apply(&event);
            structural |= applied.structural;
            for path in applied.paths {
                self.inspector.values.remove(&path);
                self.inspector.values.remove(&format!("{path}/normalized"));
                for prefix in ["/touches", "/transport"] {
                    if path.starts_with(prefix) {
                        self.inspector.values.remove(prefix);
                    }
                }
            }
        }
        if structural {
            self.m.reload_project(&mut self.link);
            self.inspector.children.clear();
            self.inspector.values.clear();
        }
        if self.m.transport.playing {
            if self.last_poll.elapsed() >= POLL {
                self.m.refresh_transport(&mut self.link);
                self.last_poll = Instant::now();
            }
            ctx.request_repaint_after(POLL);
        }

        // Selections are not cleared when they point at nothing: right after an edit, the
        // event describing it may not have arrived yet. Panels show a hint until it does.

        self.pal = Palette::from_ui(&self.m.project.ui);
        if self.applied_theme != Some(self.pal) {
            self.pal.apply(ctx);
            self.applied_theme = Some(self.pal);
        }
    }

    /// Key bindings from /ui/keys, e.g. {"ctrl+z": "undo"}, shared with the browser GUI.
    fn keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let name = match key.name() {
                "Space" => " ".to_string(),
                other => other.to_lowercase(),
            };
            let combo: Vec<&str> = [
                (modifiers.command || modifiers.ctrl).then_some("ctrl"),
                modifiers.shift.then_some("shift"),
                modifiers.alt.then_some("alt"),
                Some(name.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect();
            if let Some(action) = self.m.project.ui["keys"][combo.join("+")].as_str().map(str::to_string) {
                self.action(&action);
            }
        }
    }

    pub fn action(&mut self, action: &str) {
        match action {
            "transport.toggle" => {
                let playing = !self.m.transport.playing;
                if self.link.set("/transport/playing", Value::Bool(playing)).is_some() {
                    self.m.transport.playing = playing;
                }
            }
            "undo" => {
                self.link.run(Request::Undo);
            }
            "redo" => {
                self.link.run(Request::Redo);
            }
            "ui.edit" => self.editing = !self.editing,
            "selection.delete" => {
                if let Some(id) = self.sel.node.clone().filter(|id| id != "master") {
                    self.link.tx(format!("remove {id}"), vec![Command::RemoveNode { id }]);
                }
            }
            other => self.link.toast(format!("unknown action `{other}`")),
        }
    }

    pub fn select(&mut self, track: Option<usize>, node: Option<String>) {
        self.sel = Selection { track, node };
    }

    fn area(&mut self, ui: &mut Ui, name: &str, rect: egui::Rect, panels: Vec<(String, Value)>) {
        ui.painter().rect_filled(rect, 0.0, self.pal.panel);
        if self.editing {
            ui.painter().rect_stroke(
                rect.shrink(0.5),
                0.0,
                Stroke::new(1.0, self.pal.accent.linear_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .id_salt(("area", name))
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(rect);

        let mut active = self
            .tabs
            .get(name)
            .cloned()
            .filter(|id| panels.iter().any(|(p, _)| p == id));
        if active.is_none() {
            active = panels.first().map(|(id, _)| id.clone());
        }
        let bare = !self.editing && (panels.is_empty() || (panels.len() == 1 && panels[0].1["type"] == "transport"));
        if !bare {
            let bar = egui::Frame::new().fill(self.pal.bg).inner_margin(egui::Margin {
                left: 4,
                right: 4,
                top: 3,
                bottom: 0,
            });
            bar.show(&mut child, |ui| {
                ui.set_width(rect.width() - 8.0);
                ui.horizontal(|ui| {
                    for (id, panel) in &panels {
                        let title = panel["title"].as_str().unwrap_or(id);
                        if ui.selectable_label(active.as_deref() == Some(id), title).clicked() {
                            self.tabs.insert(name.to_string(), id.clone());
                            active = Some(id.clone());
                        }
                    }
                    if self.editing {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(id) = active.clone() {
                                if ui.small_button("✕").on_hover_text("Remove this panel").clicked() {
                                    self.link.set(&format!("/ui/panels/{id}"), Value::Null);
                                }
                                if ui.small_button("⚙").on_hover_text("Edit this panel's JSON").clicked() {
                                    self.edit_json(&format!("/ui/panels/{id}"));
                                }
                            }
                            ui.label(RichText::new(name).monospace().color(self.pal.accent));
                        });
                    }
                });
            });
        }

        let Some((id, panel)) = active.and_then(|a| panels.into_iter().find(|(id, _)| *id == a)) else {
            if self.editing {
                child.label(RichText::new(format!("Empty area \"{name}\". Add a panel to it.")).color(self.pal.muted));
            }
            return;
        };
        let kind = panel["type"].as_str().unwrap_or("").to_string();
        if kind == "transport" {
            panels::transport::show(&mut child, self);
            return;
        }
        egui::ScrollArea::both()
            .id_salt(("scroll", &id))
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                egui::Frame::new()
                    .inner_margin(8)
                    .show(ui, |ui| self.panel(ui, &id, &kind, &panel));
            });
    }

    fn panel(&mut self, ui: &mut Ui, id: &str, kind: &str, panel: &Value) {
        let has_source = self.m.project.ui["components"][kind]["source"].is_string();
        match kind {
            _ if has_source => panels::web::show(ui, self, kind, "is a custom panel written as web code"),
            "browser" => panels::browser::show(ui, self),
            "mixer" => panels::mixer::show(ui, self),
            "devices" => panels::devices::show(ui, self),
            "graph" => panels::graph::show(ui, self),
            "automation" => panels::automation::show(ui, self),
            "inspector" => panels::inspector::show(ui, self),
            "log" => panels::log::show(ui, self),
            "controls" => panels::controls::show(ui, self, id, panel),
            other => {
                ui.label(format!(
                    "Unknown panel type \"{other}\". Define it at /ui/components/{other}."
                ));
            }
        }
    }

    pub fn edit_json(&mut self, path: &str) {
        if self.editors.iter().any(|e| e.path == path) {
            return;
        }
        match self.link.get(path) {
            Ok(value) => {
                let raw = value.as_str().is_some_and(|s| s.contains('\n'));
                let text = match &value {
                    Value::String(s) if raw => s.clone(),
                    v => serde_json::to_string_pretty(v).unwrap_or_default(),
                };
                self.editors.push(JsonEditor {
                    path: path.into(),
                    text,
                    raw,
                    open: true,
                });
            }
            Err(e) => self.link.toast(e),
        }
    }

    fn editors_ui(&mut self, ctx: &egui::Context) {
        let mut saves = Vec::new();
        for (i, editor) in self.editors.iter_mut().enumerate() {
            let mut open = editor.open;
            egui::Window::new(&editor.path)
                .id(egui::Id::unique(("editor", &editor.path)))
                .open(&mut open)
                .default_size([640.0, 480.0])
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked()
                            || ui.input(|inp| inp.modifiers.command && inp.key_pressed(egui::Key::Enter))
                        {
                            saves.push(i);
                        }
                        ui.weak("Ctrl+Enter saves. Writes go through the protocol, so they're undoable.");
                    });
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut editor.text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(24),
                        );
                    });
                });
            editor.open = open;
        }
        for i in saves {
            let editor = &self.editors[i];
            let value = if editor.raw {
                Ok(Value::String(editor.text.clone()))
            } else {
                serde_json::from_str(&editor.text).map_err(|e| format!("invalid JSON: {e}"))
            };
            match value {
                Ok(v) => {
                    let path = editor.path.clone();
                    if self.link.set(&path, v).is_some() {
                        self.editors[i].open = false;
                    }
                }
                Err(e) => self.link.toast(e),
            }
        }
        self.editors.retain(|e| e.open);
    }

    fn prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.prompt else { return };
        let mut done = None;
        egui::Window::new(&prompt.title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut prompt.text).desired_width(360.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(false);
                    }
                });
            });
        let Some(ok) = done else { return };
        let prompt = self.prompt.take().expect("prompt is open");
        if !ok || prompt.text.trim().is_empty() {
            return;
        }
        let text = prompt.text.trim().to_string();
        match prompt.action {
            PromptAction::Save => {
                if self.link.run(Request::Save { path: text.clone() }).is_some() {
                    self.link.toast(format!("Saved {text}"));
                }
            }
            PromptAction::Load => {
                self.link.run(Request::Load { path: text });
            }
            PromptAction::RenameTrack(i) => {
                self.link.set(&format!("/ui/tracks/{i}/name"), Value::String(text));
            }
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let toasts: Vec<String> = self.link.live_toasts().iter().map(|(t, _)| t.clone()).collect();
        if toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .show(ctx, |ui| {
                for t in toasts {
                    egui::Frame::new()
                        .fill(pal.panel3)
                        .stroke(Stroke::new(1.0, pal.danger))
                        .corner_radius(4)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_max_width(420.0);
                            ui.label(t);
                        });
                }
            });
    }

    /// Opens the browser GUI for this session, for panels the native app doesn't draw yet.
    pub fn open_web(&mut self) {
        match &self.web_url {
            Some(url) => {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
            None => self
                .link
                .toast("This app isn't serving the browser GUI (started with --connect or --no-web)."),
        }
    }
}

/// A small coloured pill with a client's name.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(14.0, 4.0), Sense::hover());
    ui.painter()
        .rect_stroke(rect, 9.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    ui.painter().galley(rect.min + vec2(7.0, 2.0), galley, color);
    resp
}

pub struct IcarusApp {
    pub studio: Studio,
}

impl eframe::App for IcarusApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.studio.frame(ui);
    }
}
