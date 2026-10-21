use icarus_engine::devices;
use icarus_model::{Command, Connection, Node, OUTPUT_NODE, Project};

/// Applies one command to `project`. Routing cycles are caught later, when the whole
/// transaction is compiled.
pub fn apply_command(project: &mut Project, cmd: &Command) -> Result<(), String> {
    match cmd {
        Command::AddNode { id, kind, params } => {
            if id.is_empty() || id.contains('/') {
                return Err(format!("invalid node id `{id}`"));
            }
            if project.nodes.contains_key(id) {
                return Err(format!("node `{id}` already exists"));
            }
            let info = devices::info(kind).ok_or_else(|| format!("unknown device kind `{kind}`"))?;
            let mut node = Node::new(kind);
            for (param, value) in params {
                let spec = info
                    .params
                    .iter()
                    .find(|p| p.id == param)
                    .ok_or_else(|| format!("{kind} has no parameter `{param}`"))?;
                node.params.insert(param.clone(), spec.clamp(*value));
            }
            project.nodes.insert(id.clone(), node);
        }
        Command::RemoveNode { id } => {
            if id == OUTPUT_NODE {
                return Err(format!("`{OUTPUT_NODE}` cannot be removed"));
            }
            project.nodes.remove(id).ok_or_else(|| format!("no node `{id}`"))?;
            project.connections.retain(|c| &c.from != id && &c.to != id);
        }
        Command::Connect { from, to } => {
            for id in [from, to] {
                if !project.nodes.contains_key(id) {
                    return Err(format!("no node `{id}`"));
                }
            }
            if from == to {
                return Err("a node cannot feed itself".into());
            }
            let conn = Connection {
                from: from.clone(),
                to: to.clone(),
            };
            if project.connections.contains(&conn) {
                return Err(format!("{from} -> {to} already connected"));
            }
            project.connections.push(conn);
        }
        Command::Disconnect { from, to } => {
            let before = project.connections.len();
            project.connections.retain(|c| !(&c.from == from && &c.to == to));
            if project.connections.len() == before {
                return Err(format!("{from} -> {to} is not connected"));
            }
        }
        Command::SetParam { node, param, value } => {
            let spec = spec_for(project, node, param)?;
            let value = spec.clamp(*value);
            project
                .nodes
                .get_mut(node)
                .expect("checked")
                .params
                .insert(param.clone(), value);
        }
        Command::SetAutomation { node, param, points } => {
            let spec = spec_for(project, node, param)?;
            let mut points: Vec<_> = points
                .iter()
                .map(|p| icarus_model::AutoPoint {
                    time_s: p.time_s.max(0.0),
                    value: spec.clamp(p.value),
                })
                .collect();
            points.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
            project
                .nodes
                .get_mut(node)
                .expect("checked")
                .automation
                .insert(param.clone(), points);
        }
        Command::ClearAutomation { node, param } => {
            spec_for(project, node, param)?;
            project.nodes.get_mut(node).expect("checked").automation.remove(param);
        }
        Command::SetLength { length_s } => {
            if !(*length_s > 0.0 && *length_s <= 3_600.0) {
                return Err("length must be between 0 and 3600 seconds".into());
            }
            project.length_s = *length_s;
        }
        Command::SetTempo { bpm } => {
            if !(20.0..=999.0).contains(bpm) {
                return Err("tempo must be between 20 and 999 bpm".into());
            }
            project.tempo_bpm = *bpm;
        }
        Command::SetSeed { seed } => project.seed = *seed,
        Command::SetUi { path, value } => set_pointer(&mut project.ui, path, value.clone())?,
    }
    Ok(())
}

/// Writes `value` at a `/`-separated path inside a JSON document, creating objects on the way.
/// Arrays are indexed by number, and an index equal to the length appends. `null` removes.
pub fn set_pointer(root: &mut serde_json::Value, path: &str, value: serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let Some((last, parents)) = segs.split_last() else {
        if !value.is_object() {
            return Err("/ui must stay an object".into());
        }
        *root = value;
        return Ok(());
    };
    let mut cur = root;
    for seg in parents {
        cur = match cur {
            Value::Object(map) => map
                .entry(seg.to_string())
                .or_insert_with(|| Value::Object(Default::default())),
            Value::Array(items) => {
                let i: usize = seg.parse().map_err(|_| format!("`{seg}` is not an array index"))?;
                items.get_mut(i).ok_or_else(|| format!("index {i} is out of range"))?
            }
            _ => return Err(format!("cannot descend into `{seg}`: not an object or array")),
        };
    }
    match cur {
        Value::Object(map) if value.is_null() => {
            map.remove(*last);
        }
        Value::Object(map) => {
            map.insert(last.to_string(), value);
        }
        Value::Array(items) => {
            let i: usize = last.parse().map_err(|_| format!("`{last}` is not an array index"))?;
            match (i.cmp(&items.len()), value.is_null()) {
                (std::cmp::Ordering::Less, true) => {
                    items.remove(i);
                }
                (std::cmp::Ordering::Less, false) => items[i] = value,
                (std::cmp::Ordering::Equal, false) => items.push(value),
                _ => return Err(format!("index {i} is out of range")),
            }
        }
        _ => return Err(format!("cannot set `{last}`: parent is not an object or array")),
    }
    Ok(())
}

fn spec_for(project: &Project, node: &str, param: &str) -> Result<&'static icarus_engine::ParamSpec, String> {
    let n = project.nodes.get(node).ok_or_else(|| format!("no node `{node}`"))?;
    devices::info(&n.kind)
        .and_then(|d| d.params.iter().find(|p| p.id == param))
        .ok_or_else(|| format!("{} has no parameter `{param}`", n.kind))
}
