//! Track operations, matching the browser GUI's tracks.js. A track in /ui/tracks names a chain
//! `devices[0] -> ... -> mix -> master`; every operation changes the graph and /ui/tracks in
//! one transaction, so it undoes in one step.

use crate::mirror::{Mirror, Track};
use icarus_model::{Command, OUTPUT_NODE};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

const COLORS: [&str; 8] = [
    "#f0a236", "#4fb3d9", "#9bd45a", "#e5699a", "#b48cf2", "#58d1b2", "#e5534b", "#d9c14f",
];

pub fn unique_id(m: &Mirror, base: &str) -> String {
    if !m.project.nodes.contains_key(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}{i}"))
        .find(|id| !m.project.nodes.contains_key(id))
        .expect("unbounded")
}

fn edges(chain: &[String]) -> Vec<(String, String)> {
    chain.windows(2).map(|w| (w[0].clone(), w[1].clone())).collect()
}

fn chain(devices: &[String], mix: &str) -> Vec<String> {
    devices.iter().cloned().chain([mix.to_string()]).collect()
}

/// Commands that rewire a track from its current device list to `devices`.
fn rewire(m: &Mirror, track: &Track, devices: &[String]) -> Vec<Command> {
    let before = edges(&chain(&track.devices, &track.mix));
    let after = edges(&chain(devices, &track.mix));
    let existing: HashSet<(String, String)> = m
        .project
        .connections
        .iter()
        .map(|c| (c.from.clone(), c.to.clone()))
        .collect();
    let mut cmds: Vec<Command> = before
        .iter()
        .filter(|e| !after.contains(e) && existing.contains(*e))
        .map(|(from, to)| Command::Disconnect {
            from: from.clone(),
            to: to.clone(),
        })
        .collect();
    cmds.extend(
        after
            .iter()
            .filter(|e| !existing.contains(*e))
            .map(|(from, to)| Command::Connect {
                from: from.clone(),
                to: to.clone(),
            }),
    );
    cmds
}

pub fn add_track(m: &Mirror, instrument: Option<&str>) -> (String, Vec<Command>) {
    let n = m.tracks().len() + 1;
    let id = unique_id(m, &format!("t{n}"));
    let mix = unique_id(m, &format!("{id}_mix"));
    let name = format!("Track {n}");
    let mut cmds = vec![
        Command::AddNode {
            id: mix.clone(),
            kind: "gain".into(),
            params: BTreeMap::from([("gain".into(), -6.0)]),
        },
        Command::Connect {
            from: mix.clone(),
            to: OUTPUT_NODE.into(),
        },
    ];
    let mut devices = Vec::new();
    if let Some(kind) = instrument {
        let inst = unique_id(m, &format!("{id}_{kind}"));
        cmds.push(Command::AddNode {
            id: inst.clone(),
            kind: kind.into(),
            params: BTreeMap::new(),
        });
        cmds.push(Command::Connect {
            from: inst.clone(),
            to: mix.clone(),
        });
        devices.push(inst);
    }
    cmds.push(Command::SetUi {
        path: format!("/tracks/{}", n - 1),
        value: json!({ "id": id, "name": name, "color": COLORS[(n - 1) % COLORS.len()], "devices": devices, "mix": mix }),
    });
    (format!("add {name}"), cmds)
}

pub fn add_device(m: &Mirror, index: usize, kind: &str) -> Option<Vec<Command>> {
    let track = m.tracks().get(index)?.clone();
    let id = unique_id(m, &format!("{}_{kind}", track.id));
    let mut devices = track.devices.clone();
    devices.push(id.clone());
    let mut cmds = vec![Command::AddNode {
        id,
        kind: kind.into(),
        params: BTreeMap::new(),
    }];
    cmds.extend(rewire(m, &track, &devices));
    cmds.push(Command::SetUi {
        path: format!("/tracks/{index}/devices"),
        value: json!(devices),
    });
    Some(cmds)
}

pub fn remove_device(m: &Mirror, index: usize, id: &str) -> Option<Vec<Command>> {
    let track = m.tracks().get(index)?.clone();
    let devices: Vec<String> = track.devices.iter().filter(|d| *d != id).cloned().collect();
    let existing: HashSet<(String, String)> = m
        .project
        .connections
        .iter()
        .filter(|c| c.from != id && c.to != id)
        .map(|c| (c.from.clone(), c.to.clone()))
        .collect();
    let mut cmds = vec![Command::RemoveNode { id: id.into() }];
    cmds.extend(
        edges(&chain(&devices, &track.mix))
            .into_iter()
            .filter(|e| !existing.contains(e))
            .map(|(from, to)| Command::Connect { from, to }),
    );
    cmds.push(Command::SetUi {
        path: format!("/tracks/{index}/devices"),
        value: json!(devices),
    });
    Some(cmds)
}

pub fn move_device(m: &Mirror, index: usize, id: &str, delta: isize) -> Option<Vec<Command>> {
    let track = m.tracks().get(index)?.clone();
    let mut devices = track.devices.clone();
    let i = devices.iter().position(|d| d == id)?;
    let j = i.checked_add_signed(delta).filter(|j| *j < devices.len())?;
    devices.swap(i, j);
    let mut cmds = rewire(m, &track, &devices);
    cmds.push(Command::SetUi {
        path: format!("/tracks/{index}/devices"),
        value: json!(devices),
    });
    Some(cmds)
}

pub fn delete_track(m: &Mirror, index: usize) -> Option<Vec<Command>> {
    let track = m.tracks().get(index)?.clone();
    let mut cmds: Vec<Command> = chain(&track.devices, &track.mix)
        .into_iter()
        .filter(|id| m.project.nodes.contains_key(id))
        .map(|id| Command::RemoveNode { id })
        .collect();
    cmds.push(Command::SetUi {
        path: format!("/tracks/{index}"),
        value: serde_json::Value::Null,
    });
    Some(cmds)
}

pub fn track_of(m: &Mirror, node: &str) -> Option<usize> {
    m.tracks()
        .iter()
        .position(|t| t.mix == node || t.devices.iter().any(|d| d == node))
}
