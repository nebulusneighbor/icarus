//! A local copy of the session, loaded once and kept current from change events.

use crate::link::Link;
use icarus_model::{Project, Request};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

const LOG_LIMIT: usize = 500;

/// A parameter's spec as the session describes it (see `ParamSpec` in icarus-engine).
#[derive(Clone, Debug, Deserialize)]
pub struct Spec {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub curve: String,
    #[serde(default)]
    pub description: String,
}

impl Spec {
    pub fn normalize(&self, v: f64) -> f64 {
        let v = v.clamp(self.min, self.max);
        if self.curve == "log" {
            (v / self.min).ln() / (self.max / self.min).ln()
        } else {
            (v - self.min) / (self.max - self.min)
        }
    }

    pub fn denormalize(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0);
        let v = if self.curve == "log" {
            self.min * (self.max / self.min).powf(n)
        } else {
            self.min + n * (self.max - self.min)
        };
        if self.curve == "stepped" { v.round() } else { v }
    }

    pub fn format(&self, v: f64) -> String {
        match self.unit.as_str() {
            "Hz" if v >= 10_000.0 => format!("{:.1} kHz", v / 1000.0),
            "Hz" if v >= 1_000.0 => format!("{:.2} kHz", v / 1000.0),
            "Hz" => format!("{v:.0} Hz"),
            "dB" => format!("{v:.1} dB"),
            _ if self.curve == "stepped" => format!("{v:.0}"),
            _ => format!("{v:.2}"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DeviceInfo {
    pub kind: String,
    pub description: String,
    pub params: Vec<Spec>,
}

/// A track as stored in /ui/tracks: a reading of the graph, not an engine object.
#[derive(Clone, Debug, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub devices: Vec<String>,
    pub mix: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct Transport {
    pub playing: bool,
    pub live: bool,
    pub position_s: f64,
}

pub struct LogEntry {
    pub at: Instant,
    pub seq: u64,
    pub client: String,
    pub label: String,
    pub changes: Vec<(String, Value)>,
}

pub struct Mirror {
    pub project: Project,
    pub devices: BTreeMap<String, DeviceInfo>,
    /// `"node/param"` to the client holding it.
    pub touches: BTreeMap<String, String>,
    pub transport: Transport,
    pub log: VecDeque<LogEntry>,
}

/// What an event changed, so the app knows how much to refresh.
#[derive(Default)]
pub struct Applied {
    pub structural: bool,
    /// Paths whose values changed in place.
    pub paths: Vec<String>,
}

fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

impl Mirror {
    pub fn load(link: &mut Link) -> Result<Self, String> {
        let project = decode(link.request(Request::Snapshot)?)?;
        let devices: Vec<DeviceInfo> = decode(link.request(Request::ListDevices)?)?;
        let touches = decode(link.get("/touches")?)?;
        let transport = decode(link.get("/transport")?)?;
        link.request(Request::Subscribe {
            paths: vec!["/".into()],
        })?;
        Ok(Self {
            project,
            devices: devices.into_iter().map(|d| (d.kind.clone(), d)).collect(),
            touches,
            transport,
            log: VecDeque::new(),
        })
    }

    pub fn reload_project(&mut self, link: &mut Link) {
        match link.request(Request::Snapshot).and_then(decode) {
            Ok(project) => self.project = project,
            Err(e) => link.toast(format!("reload failed: {e}")),
        }
    }

    pub fn refresh_transport(&mut self, link: &mut Link) {
        if let Ok(t) = link.get("/transport").and_then(decode) {
            self.transport = t;
        }
    }

    pub fn spec(&self, node: &str, param: &str) -> Option<&Spec> {
        let kind = &self.project.nodes.get(node)?.kind;
        self.devices.get(kind)?.params.iter().find(|p| p.id == param)
    }

    pub fn value(&self, node: &str, param: &str) -> f64 {
        let explicit = self.project.nodes.get(node).and_then(|n| n.params.get(param)).copied();
        explicit
            .or_else(|| self.spec(node, param).map(|s| s.default))
            .unwrap_or(0.0)
    }

    pub fn set_value(&mut self, node: &str, param: &str, value: f64) {
        if let Some(n) = self.project.nodes.get_mut(node) {
            n.params.insert(param.to_string(), value);
        }
    }

    pub fn automated(&self, node: &str, param: &str) -> bool {
        self.project
            .nodes
            .get(node)
            .and_then(|n| n.automation.get(param))
            .is_some_and(|l| !l.is_empty())
    }

    pub fn held_by(&self, node: &str, param: &str) -> Option<&str> {
        self.touches.get(&format!("{node}/{param}")).map(String::as_str)
    }

    pub fn tracks(&self) -> Vec<Track> {
        serde_json::from_value(self.project.ui["tracks"].clone()).unwrap_or_default()
    }

    /// Applies one change event. Values that change often (params, touches, transport) are
    /// patched in place; anything else marks the mirror for a reload.
    pub fn apply(&mut self, event: &Value) -> Applied {
        let mut applied = Applied::default();
        let changes: Vec<(String, Value)> = event["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (c["path"].as_str().unwrap_or("").to_string(), c["value"].clone()))
            .collect();
        for (path, value) in &changes {
            let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            match seg.as_slice() {
                ["nodes", id, "params", param] if self.project.nodes.contains_key(*id) => {
                    let node = self.project.nodes.get_mut(*id).expect("checked");
                    match value.as_f64() {
                        Some(v) => node.params.insert(param.to_string(), v),
                        None => node.params.remove(*param),
                    };
                }
                ["touches", node, param] => {
                    let key = format!("{node}/{param}");
                    match value.as_str() {
                        Some(who) => self.touches.insert(key, who.to_string()),
                        None => self.touches.remove(&key),
                    };
                }
                ["transport", "playing"] => self.transport.playing = value.as_bool().unwrap_or(false),
                _ => applied.structural = true,
            }
            applied.paths.push(path.clone());
        }
        self.log.push_back(LogEntry {
            at: Instant::now(),
            seq: event["seq"].as_u64().unwrap_or(0),
            client: event["client"].as_str().unwrap_or("?").to_string(),
            label: event["label"].as_str().unwrap_or("").to_string(),
            changes,
        });
        if self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
        applied
    }
}
