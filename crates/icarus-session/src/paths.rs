//! The project as a tree of paths.
//!
//! ```text
//! /project                                  sample_rate, seed, tempo_bpm, length_s
//! /project/{seed,tempo_bpm,length_s}        settable
//! /nodes                                    node ids
//! /nodes/<id>                               description; set {"kind", "params"} to add, null to remove
//! /nodes/<id>/kind
//! /nodes/<id>/params                        every param's current value
//! /nodes/<id>/params/<param>                plain value (Hz, dB, ...), settable
//! /nodes/<id>/params/<param>/normalized     0..1 on the param's curve, settable (for faders, VR)
//! /nodes/<id>/params/<param>/spec
//! /nodes/<id>/automation/<param>            [{time_s, value}], settable; null clears
//! /connections                              [{from, to}], settable as a whole
//! /devices, /devices/<kind>                 device catalogue
//! ```
//!
//! Every write becomes ordinary [`Command`]s, so it is validated, undoable and attributed.

use crate::{Result, SessionError, invalid};
use icarus_engine::{ParamSpec, devices};
use icarus_model::{AutoPoint, Command, Connection, Project};
use serde_json::{Value, json};

fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

pub fn param_spec(project: &Project, node: &str, param: &str) -> Result<&'static ParamSpec> {
    let Some(n) = project.nodes.get(node) else {
        return invalid(format!("no node `{node}`"));
    };
    devices::info(&n.kind)
        .and_then(|d| d.params.iter().find(|p| p.id == param))
        .ok_or_else(|| SessionError::Invalid(format!("node `{node}` ({}) has no parameter `{param}`", n.kind)))
}

fn current_value(project: &Project, node: &str, spec: &ParamSpec) -> f64 {
    project.nodes[node].params.get(spec.id).copied().unwrap_or(spec.default)
}

pub fn describe_node(project: &Project, id: &str) -> Result<Value> {
    let Some(node) = project.nodes.get(id) else {
        return invalid(format!("no node `{id}`"));
    };
    let info = devices::info(&node.kind);
    let params: Vec<Value> = info
        .map(|d| d.params)
        .unwrap_or_default()
        .iter()
        .map(|spec| {
            let value = current_value(project, id, spec);
            json!({
                "spec": spec,
                "value": value,
                "normalized": spec.to_normalized(value),
                "automated": node.automation.get(spec.id).is_some_and(|l| !l.is_empty()),
            })
        })
        .collect();
    Ok(json!({
        "id": id,
        "kind": node.kind,
        "description": info.map(|d| d.description),
        "params": params,
        "inputs": project.connections.iter().filter(|c| c.to == id).map(|c| &c.from).collect::<Vec<_>>(),
        "outputs": project.connections.iter().filter(|c| c.from == id).map(|c| &c.to).collect::<Vec<_>>(),
    }))
}

pub fn get(project: &Project, path: &str) -> Result<Value> {
    let seg = segments(path);
    Ok(match seg.as_slice() {
        [] => json!(["project", "nodes", "connections", "devices"]),
        ["project"] => json!({
            "sample_rate": project.sample_rate,
            "seed": project.seed,
            "tempo_bpm": project.tempo_bpm,
            "length_s": project.length_s,
        }),
        ["project", field] => get(project, "/project")?
            .get(*field)
            .cloned()
            .ok_or_else(|| SessionError::Invalid(format!("no project field `{field}`")))?,
        ["nodes"] => json!(project.nodes.keys().collect::<Vec<_>>()),
        ["nodes", id] => describe_node(project, id)?,
        ["nodes", id, "kind"] => json!(node(project, id)?.kind),
        ["nodes", id, "params"] => {
            let kind = &node(project, id)?.kind;
            let params = devices::info(kind).map(|d| d.params).unwrap_or_default();
            Value::Object(
                params
                    .iter()
                    .map(|s| (s.id.to_string(), json!(current_value(project, id, s))))
                    .collect(),
            )
        }
        ["nodes", id, "params", param] => json!(current_value(project, id, param_spec(project, id, param)?)),
        ["nodes", id, "params", param, "normalized"] => {
            let spec = param_spec(project, id, param)?;
            json!(spec.to_normalized(current_value(project, id, spec)))
        }
        ["nodes", id, "params", param, "spec"] => json!(param_spec(project, id, param)?),
        ["nodes", id, "automation"] => json!(node(project, id)?.automation),
        ["nodes", id, "automation", param] => {
            param_spec(project, id, param)?;
            json!(node(project, id)?.automation.get(*param).cloned().unwrap_or_default())
        }
        ["connections"] => json!(project.connections),
        ["devices"] => json!(devices::DEVICES),
        ["devices", kind] => {
            json!(devices::info(kind).ok_or_else(|| SessionError::Invalid(format!("no device `{kind}`")))?)
        }
        _ => return invalid(format!("no path `{path}`")),
    })
}

pub fn list(project: &Project, path: &str) -> Result<Value> {
    let seg = segments(path);
    let children: Vec<String> = match seg.as_slice() {
        [] => vec!["project".into(), "nodes".into(), "connections".into(), "devices".into()],
        ["project"] => vec![
            "sample_rate".into(),
            "seed".into(),
            "tempo_bpm".into(),
            "length_s".into(),
        ],
        ["nodes"] => project.nodes.keys().cloned().collect(),
        ["nodes", id] => {
            node(project, id)?;
            vec!["kind".into(), "params".into(), "automation".into()]
        }
        ["nodes", id, "params" | "automation"] => devices::info(&node(project, id)?.kind)
            .map(|d| d.params.iter().map(|p| p.id.to_string()).collect())
            .unwrap_or_default(),
        ["nodes", id, "params", param] => {
            param_spec(project, id, param)?;
            vec!["normalized".into(), "spec".into()]
        }
        ["devices"] => devices::DEVICES.iter().map(|d| d.kind.to_string()).collect(),
        _ => return invalid(format!("`{path}` has no children")),
    };
    let base = format!("/{}", seg.join("/"));
    let base = base.trim_end_matches('/');
    Ok(json!(
        children.iter().map(|c| format!("{base}/{c}")).collect::<Vec<_>>()
    ))
}

/// Translates a path write into commands.
pub fn set_commands(project: &Project, path: &str, value: Value) -> Result<Vec<Command>> {
    let seg = segments(path);
    let number = |v: &Value| {
        v.as_f64()
            .ok_or_else(|| SessionError::Invalid(format!("`{path}` needs a number")))
    };
    Ok(match seg.as_slice() {
        ["project", "seed"] => vec![Command::SetSeed {
            seed: value
                .as_u64()
                .ok_or_else(|| SessionError::Invalid("seed must be a non-negative integer".into()))?,
        }],
        ["project", "tempo_bpm"] => vec![Command::SetTempo { bpm: number(&value)? }],
        ["project", "length_s"] => vec![Command::SetLength {
            length_s: number(&value)?,
        }],
        ["nodes", id] if value.is_null() => vec![Command::RemoveNode { id: id.to_string() }],
        ["nodes", id] => {
            #[derive(serde::Deserialize)]
            struct NewNode {
                kind: String,
                #[serde(default)]
                params: std::collections::BTreeMap<String, f64>,
            }
            let n: NewNode = serde_json::from_value(value)?;
            vec![Command::AddNode {
                id: id.to_string(),
                kind: n.kind,
                params: n.params,
            }]
        }
        ["nodes", id, "params", param] => {
            vec![Command::SetParam {
                node: id.to_string(),
                param: param.to_string(),
                value: number(&value)?,
            }]
        }
        ["nodes", id, "params", param, "normalized"] => {
            let spec = param_spec(project, id, param)?;
            vec![Command::SetParam {
                node: id.to_string(),
                param: param.to_string(),
                value: spec.from_normalized(number(&value)?),
            }]
        }
        ["nodes", id, "automation", param] if value.is_null() => {
            vec![Command::ClearAutomation {
                node: id.to_string(),
                param: param.to_string(),
            }]
        }
        ["nodes", id, "automation", param] => vec![Command::SetAutomation {
            node: id.to_string(),
            param: param.to_string(),
            points: serde_json::from_value::<Vec<AutoPoint>>(value)?,
        }],
        ["connections"] => {
            let wanted: Vec<Connection> = serde_json::from_value(value)?;
            let mut cmds: Vec<Command> = project
                .connections
                .iter()
                .map(|c| Command::Disconnect {
                    from: c.from.clone(),
                    to: c.to.clone(),
                })
                .collect();
            cmds.extend(wanted.into_iter().map(|c| Command::Connect { from: c.from, to: c.to }));
            cmds
        }
        _ => return invalid(format!("`{path}` is not settable")),
    })
}

fn node<'a>(project: &'a Project, id: &str) -> Result<&'a icarus_model::Node> {
    project
        .nodes
        .get(id)
        .ok_or_else(|| SessionError::Invalid(format!("no node `{id}`")))
}
