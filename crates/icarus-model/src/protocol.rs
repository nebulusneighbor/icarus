use crate::{AutoPoint, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One discrete, validated edit. Commands are always applied inside a transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    AddNode {
        id: NodeId,
        kind: String,
        #[serde(default)]
        params: BTreeMap<String, f64>,
    },
    RemoveNode {
        id: NodeId,
    },
    Connect {
        from: NodeId,
        to: NodeId,
    },
    Disconnect {
        from: NodeId,
        to: NodeId,
    },
    SetParam {
        node: NodeId,
        param: String,
        value: f64,
    },
    SetAutomation {
        node: NodeId,
        param: String,
        points: Vec<AutoPoint>,
    },
    ClearAutomation {
        node: NodeId,
        param: String,
    },
    SetLength {
        length_s: f64,
    },
    SetTempo {
        bpm: f64,
    },
    SetSeed {
        seed: u64,
    },
}

/// A continuous action over time: a finger on a knob.
///
/// Moves are accepted only between `begin` and `end`, and the whole gesture is one undo step.
/// With `record`, each move that carries `at_s` is written into the parameter's automation lane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Gesture {
    Begin {
        node: NodeId,
        param: String,
        #[serde(default)]
        record: bool,
    },
    Move {
        node: NodeId,
        param: String,
        value: f64,
        #[serde(default)]
        at_s: Option<f64>,
    },
    End {
        node: NodeId,
        param: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderSpec {
    pub start_s: f64,
    pub end_s: f64,
    #[serde(default = "default_block_size")]
    pub block_size: usize,
    /// Nodes whose output is captured alongside the master.
    #[serde(default)]
    pub taps: Vec<NodeId>,
}

fn default_block_size() -> usize {
    256
}

/// One parameter to vary in a probe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sweep {
    pub node: NodeId,
    pub param: String,
    pub values: Vec<f64>,
}

/// Render every combination of the sweeps on a forked copy of the project and measure the result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub start_s: f64,
    pub end_s: f64,
    /// Node to measure; defaults to the master.
    #[serde(default)]
    pub listen: Option<NodeId>,
    pub sweeps: Vec<Sweep>,
}

/// Everything a client can ask of a session.
///
/// `get`, `set` and `list` expose the whole project as a tree of paths (see the session crate),
/// which is what OSC, VR and other raw controllers use. The other requests are the structured API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Transaction {
        #[serde(default)]
        label: Option<String>,
        commands: Vec<Command>,
    },
    Gesture {
        gesture: Gesture,
    },
    Undo,
    Redo,
    /// Undo history with the client that made each change.
    History,
    Snapshot,
    Get {
        path: String,
    },
    Set {
        path: String,
        value: serde_json::Value,
    },
    /// Push change events for these path prefixes (`/` for everything) to the caller's feed.
    /// Only adapters with a push channel (socket, WebSocket, OSC, MCP polling) support it.
    Subscribe {
        paths: Vec<String>,
    },
    /// Stop events for these prefixes, or for all of them when `paths` is empty.
    Unsubscribe {
        #[serde(default)]
        paths: Vec<String>,
    },
    /// Child paths under `path`.
    List {
        #[serde(default)]
        path: String,
    },
    ListDevices,
    Describe {
        node: NodeId,
    },
    Render {
        spec: RenderSpec,
        #[serde(default)]
        wav_path: Option<String>,
    },
    Probe {
        spec: ProbeSpec,
    },
    Save {
        path: String,
    },
    Load {
        path: String,
    },
}
