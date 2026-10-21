use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type NodeId = String;

/// The node every signal ends up in. It always exists and cannot be removed.
pub const OUTPUT_NODE: &str = "master";

/// The whole project as plain data. Rendering a given `Project` always produces the same audio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub sample_rate: u32,
    /// Project-wide seed. Every random source derives from it, so renders are reproducible.
    pub seed: u64,
    pub tempo_bpm: f64,
    /// Length of the loop the live engine plays, in seconds.
    pub length_s: f64,
    pub nodes: BTreeMap<NodeId, Node>,
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Device kind, e.g. `"osc"` or `"filter"`.
    pub kind: String,
    /// Base parameter values in plain units (Hz, dB, ...). Missing params use the device default.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    /// Automation lanes; a lane overrides the base value of its parameter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub automation: BTreeMap<String, Vec<AutoPoint>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    pub time_s: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub from: NodeId,
    pub to: NodeId,
}

impl Default for Project {
    fn default() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(OUTPUT_NODE.to_string(), Node::new("output"));
        Self {
            sample_rate: 48_000,
            seed: 0,
            tempo_bpm: 120.0,
            length_s: 8.0,
            nodes,
            connections: Vec::new(),
        }
    }
}

impl Node {
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            params: BTreeMap::new(),
            automation: BTreeMap::new(),
        }
    }
}
