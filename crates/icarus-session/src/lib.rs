//! A live project and everything clients can do to it.
//!
//! Every client (GUI, agent, OSC surface, VR rig) calls [`Session::handle`] with its own name
//! and a [`Request`]. There is no private API: whatever one client can do, any client can.
//! Every change is broadcast as an [`Event`] to the feeds subscribed to the paths it touched.

mod apply;
mod events;
mod paths;

use icarus_analysis::analyze;
use icarus_engine::{Graph, devices};
use icarus_model::{Command, Gesture, OUTPUT_NODE, Project, Request};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub use apply::apply_command;
pub use events::{Change, Event, FeedId};

const HISTORY_LIMIT: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Compile(#[from] icarus_engine::CompileError),
    #[error(transparent)]
    Render(#[from] icarus_render::RenderError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Wav(#[from] hound::Error),
}

pub type Result<T> = std::result::Result<T, SessionError>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(SessionError::Invalid(msg.into()))
}

/// Receives changes so a running engine can follow the project.
pub trait EngineSink: Send {
    /// Only a base parameter value changed; cheap and glitch-free.
    fn param_changed(&mut self, node: &str, param: &str, value: f64);
    /// Anything else changed; the sink should recompile.
    fn project_changed(&mut self, project: &Project);
    /// Start or stop playback. Stopping rewinds to the start.
    fn set_playing(&mut self, _playing: bool) {}
    /// Current playback position in samples.
    fn position(&self) -> Option<u64> {
        None
    }
}

/// One undoable step: the project as it was before the change, and who made the change.
#[derive(Clone)]
struct Step {
    client: String,
    label: String,
    before: Project,
}

struct Touch {
    client: String,
    record: bool,
}

#[derive(Default)]
pub struct Session {
    project: Project,
    undo: Vec<Step>,
    redo: Vec<Step>,
    touches: BTreeMap<(String, String), Touch>,
    sink: Option<Box<dyn EngineSink>>,
    feeds: events::Feeds,
    playing: bool,
}

impl Session {
    pub fn new(project: Project) -> Result<Self> {
        Graph::compile(&project)?;
        Ok(Self {
            project,
            ..Self::default()
        })
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn set_sink(&mut self, sink: Box<dyn EngineSink>) {
        self.sink = Some(sink);
    }

    /// Opens an event feed. Subscribe it with [`Session::handle_with_feed`]; drop the receiver
    /// or call [`Session::close_feed`] when the client goes away.
    pub fn open_feed(&mut self) -> (FeedId, std::sync::mpsc::Receiver<Event>) {
        self.feeds.open()
    }

    pub fn close_feed(&mut self, feed: FeedId) {
        self.feeds.close(feed);
    }

    /// Handles one request from `client`, returning a JSON result.
    pub fn handle(&mut self, client: &str, request: Request) -> Result<Value> {
        self.handle_with_feed(client, None, request)
    }

    /// Like [`Session::handle`], for clients that own a feed and may subscribe it.
    pub fn handle_with_feed(&mut self, client: &str, feed: Option<FeedId>, request: Request) -> Result<Value> {
        match request {
            Request::Transaction { label, commands } => self.transaction(client, label, commands),
            Request::Gesture { gesture } => self.gesture(client, gesture),
            Request::Undo => self.undo(),
            Request::Redo => self.redo(),
            Request::History => Ok(json!({
                "undo": self.undo.iter().rev().map(|s| json!({"client": s.client, "label": s.label})).collect::<Vec<_>>(),
                "redo": self.redo.iter().rev().map(|s| json!({"client": s.client, "label": s.label})).collect::<Vec<_>>(),
            })),
            Request::Snapshot => Ok(serde_json::to_value(&self.project)?),
            Request::Get { path } if events::normalize(&path).starts_with("/touches") => Ok(self.touches_value()),
            Request::Get { path } if events::normalize(&path).starts_with("/transport") => {
                let transport = self.transport_value();
                match events::normalize(&path).strip_prefix("/transport/") {
                    None => Ok(transport),
                    Some(field) => transport
                        .get(field)
                        .cloned()
                        .ok_or_else(|| SessionError::Invalid(format!("no path `{path}`"))),
                }
            }
            Request::Set { path, value } if events::normalize(&path) == "/transport/playing" => {
                let Some(playing) = value.as_bool() else {
                    return invalid("/transport/playing must be true or false");
                };
                self.set_playing(client, playing)
            }
            Request::Get { path } => paths::get(&self.project, &path),
            Request::Set { path, value } => {
                let commands = paths::set_commands(&self.project, &path, value)?;
                self.transaction(client, Some(format!("set {path}")), commands)
            }
            Request::Subscribe { paths } => {
                let Some(feed) = feed else {
                    return invalid(
                        "this connection cannot receive events; use the socket, WebSocket, OSC or MCP adapter",
                    );
                };
                let paths = self
                    .feeds
                    .subscribe(feed, &paths)
                    .ok_or_else(|| SessionError::Invalid("feed is closed".into()))?;
                Ok(json!({ "feed": feed, "subscribed": paths }))
            }
            Request::Unsubscribe { paths } => {
                let Some(feed) = feed else {
                    return invalid("this connection has no event feed");
                };
                let paths = self
                    .feeds
                    .unsubscribe(feed, &paths)
                    .ok_or_else(|| SessionError::Invalid("feed is closed".into()))?;
                Ok(json!({ "feed": feed, "subscribed": paths }))
            }
            Request::List { path } if events::normalize(&path) == "/transport" => Ok(json!([
                "/transport/playing",
                "/transport/live",
                "/transport/position_s"
            ])),
            Request::List { path } => paths::list(&self.project, &path),
            Request::ListDevices => Ok(serde_json::to_value(devices::DEVICES)?),
            Request::Describe { node } => paths::describe_node(&self.project, &node),
            Request::Render { spec, wav_path } => self.render(&spec, wav_path),
            Request::Probe { spec } => Ok(serde_json::to_value(icarus_render::probe(&self.project, &spec)?)?),
            Request::Save { path } => {
                std::fs::write(&path, serde_json::to_string_pretty(&self.project)?)?;
                Ok(json!({ "saved": path }))
            }
            Request::Load { path } => {
                let loaded: Project = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                Graph::compile(&loaded)?;
                self.commit(client, format!("load {path}"), loaded, false);
                Ok(json!({ "loaded": path }))
            }
        }
    }

    /// Applies all commands or none, as one undo step.
    fn transaction(&mut self, client: &str, label: Option<String>, commands: Vec<Command>) -> Result<Value> {
        if commands.is_empty() {
            return invalid("transaction has no commands");
        }
        let mut next = self.project.clone();
        for (i, cmd) in commands.iter().enumerate() {
            apply_command(&mut next, cmd).map_err(|e| SessionError::Invalid(format!("command {i} ({cmd:?}): {e}")))?;
        }
        Graph::compile(&next)?;
        let label = label.unwrap_or_else(|| format!("{} command(s)", commands.len()));
        // A change to nothing is not a step: it would make undo appear to do nothing.
        if next == self.project {
            return Ok(json!({ "ok": true, "label": label, "commands": commands.len(), "unchanged": true }));
        }
        let params_only = commands.iter().all(|c| matches!(c, Command::SetParam { .. }));
        self.commit(client, label.clone(), next, params_only);
        Ok(json!({ "ok": true, "label": label, "commands": commands.len() }))
    }

    fn commit(&mut self, client: &str, label: String, next: Project, params_only: bool) {
        let before = std::mem::replace(&mut self.project, next);
        self.feeds.emit(client, &label, events::diff(&before, &self.project));
        self.push_undo(Step {
            client: client.to_string(),
            label,
            before,
        });
        self.redo.clear();
        self.notify(params_only);
    }

    fn push_undo(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }

    /// Tells the sink about the latest change: just the changed base values when `params_only`,
    /// a full recompile otherwise.
    fn notify(&mut self, params_only: bool) {
        let Some(sink) = &mut self.sink else { return };
        let Some(step) = self.undo.last() else { return };
        if params_only {
            for (id, node) in &self.project.nodes {
                for (param, value) in &node.params {
                    if step.before.nodes.get(id).and_then(|n| n.params.get(param)) != Some(value) {
                        sink.param_changed(id, param, *value);
                    }
                }
            }
        } else {
            sink.project_changed(&self.project);
        }
    }

    fn undo(&mut self) -> Result<Value> {
        let Some(step) = self.undo.pop() else {
            return invalid("nothing to undo");
        };
        let Step { client, label, before } = step;
        let after = std::mem::replace(&mut self.project, before);
        let mut changes = events::diff(&after, &self.project);
        changes.extend(
            std::mem::take(&mut self.touches)
                .into_keys()
                .map(|(n, p)| touch_change(&n, &p, None)),
        );
        self.feeds.emit(&client, &format!("undo {label}"), changes);
        self.redo.push(Step {
            client: client.clone(),
            label: label.clone(),
            before: after,
        });
        if let Some(sink) = &mut self.sink {
            sink.project_changed(&self.project);
        }
        Ok(json!({ "undone": label, "by": client }))
    }

    fn redo(&mut self) -> Result<Value> {
        let Some(step) = self.redo.pop() else {
            return invalid("nothing to redo");
        };
        let Step { client, label, before } = step;
        let before = std::mem::replace(&mut self.project, before);
        self.feeds
            .emit(&client, &format!("redo {label}"), events::diff(&before, &self.project));
        self.push_undo(Step {
            client: client.clone(),
            label: label.clone(),
            before,
        });
        if let Some(sink) = &mut self.sink {
            sink.project_changed(&self.project);
        }
        Ok(json!({ "redone": label, "by": client }))
    }

    fn gesture(&mut self, client: &str, gesture: Gesture) -> Result<Value> {
        match gesture {
            Gesture::Begin { node, param, record } => {
                paths::param_spec(&self.project, &node, &param)?;
                let key = (node.clone(), param.clone());
                if let Some(t) = self.touches.get(&key) {
                    return invalid(format!("{node}.{param} is already being touched by {}", t.client));
                }
                self.push_undo(Step {
                    client: client.to_string(),
                    label: format!("gesture {node}.{param}"),
                    before: self.project.clone(),
                });
                self.redo.clear();
                self.touches.insert(
                    key,
                    Touch {
                        client: client.to_string(),
                        record,
                    },
                );
                let label = format!("touch {node}.{param}");
                self.feeds
                    .emit(client, &label, vec![touch_change(&node, &param, Some(client))]);
                Ok(json!({ "touching": format!("{node}.{param}"), "record": record }))
            }
            Gesture::Move {
                node,
                param,
                value,
                normalized,
                at_s,
            } => {
                let record = match self.touches.get(&(node.clone(), param.clone())) {
                    Some(t) if t.client == client => t.record,
                    Some(t) => return invalid(format!("{node}.{param} is held by {}", t.client)),
                    None => return invalid(format!("begin a gesture on {node}.{param} before moving it")),
                };
                let spec = paths::param_spec(&self.project, &node, &param)?;
                let value = match (value, normalized) {
                    (Some(v), _) => spec.clamp(v),
                    (None, Some(n)) => spec.from_normalized(n),
                    (None, None) => return invalid("a move needs `value` or `normalized`"),
                };
                let n = self.project.nodes.get_mut(&node).expect("validated by param_spec");
                n.params.insert(param.clone(), value);
                if record {
                    let Some(at_s) = at_s else {
                        return invalid("a recording gesture needs `at_s` on every move");
                    };
                    let lane = n.automation.entry(param.clone()).or_default();
                    lane.retain(|p| (p.time_s - at_s).abs() > 1e-9);
                    lane.push(icarus_model::AutoPoint { time_s: at_s, value });
                    lane.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
                }
                if let Some(sink) = &mut self.sink {
                    sink.param_changed(&node, &param, value);
                }
                let mut changes = vec![events::param_change(&self.project, &node, &param).expect("validated")];
                if record {
                    changes.push(Change {
                        path: format!("/nodes/{node}/automation/{param}"),
                        value: json!(self.project.nodes[&node].automation[&param]),
                        normalized: None,
                    });
                }
                self.feeds.emit(client, &format!("move {node}.{param}"), changes);
                Ok(json!({ "value": value }))
            }
            Gesture::End { node, param } => {
                let key = (node.clone(), param.clone());
                match self.touches.get(&key) {
                    Some(t) if t.client == client => {}
                    Some(t) => return invalid(format!("{node}.{param} is held by {}", t.client)),
                    None => return invalid(format!("{node}.{param} is not being touched")),
                }
                let touch = self.touches.remove(&key).expect("checked above");
                self.feeds.emit(
                    client,
                    &format!("release {node}.{param}"),
                    vec![touch_change(&node, &param, None)],
                );
                if touch.record
                    && let Some(sink) = &mut self.sink
                {
                    sink.project_changed(&self.project);
                }
                Ok(json!({ "released": format!("{node}.{param}") }))
            }
        }
    }

    fn set_playing(&mut self, client: &str, playing: bool) -> Result<Value> {
        self.playing = playing;
        if let Some(sink) = &mut self.sink {
            sink.set_playing(playing);
        }
        let label = if playing { "play" } else { "stop" };
        self.feeds.emit(
            client,
            label,
            vec![Change {
                path: "/transport/playing".into(),
                value: json!(playing),
                normalized: None,
            }],
        );
        Ok(json!({ "playing": playing }))
    }

    fn transport_value(&self) -> Value {
        let position = self.sink.as_ref().and_then(|s| s.position()).unwrap_or(0);
        json!({
            "playing": self.playing,
            "live": self.sink.is_some(),
            "position_s": position as f64 / f64::from(self.project.sample_rate),
        })
    }

    fn touches_value(&self) -> Value {
        Value::Object(
            self.touches
                .iter()
                .map(|((node, param), t)| (format!("{node}/{param}"), json!(t.client)))
                .collect(),
        )
    }

    fn render(&self, spec: &icarus_model::RenderSpec, wav_path: Option<String>) -> Result<Value> {
        let out = icarus_render::render(&self.project, spec)?;
        if let Some(path) = &wav_path {
            let mut wav = hound::WavWriter::create(
                path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: out.sample_rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )?;
            for (l, r) in out.master.l.iter().zip(&out.master.r) {
                wav.write_sample(*l)?;
                wav.write_sample(*r)?;
            }
            wav.finalize()?;
        }
        let taps: BTreeMap<&String, _> = out
            .taps
            .iter()
            .map(|(id, s)| (id, analyze(&s.l, &s.r, out.sample_rate)))
            .collect();
        Ok(json!({
            OUTPUT_NODE: analyze(&out.master.l, &out.master.r, out.sample_rate),
            "taps": taps,
            "wav_path": wav_path,
        }))
    }
}

/// Presence: who is holding a control, or `None` when it is released.
fn touch_change(node: &str, param: &str, client: Option<&str>) -> Change {
    Change {
        path: format!("/touches/{node}/{param}"),
        value: json!(client),
        normalized: None,
    }
}

#[cfg(test)]
mod tests;
