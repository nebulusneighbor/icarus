//! OSC over UDP, for control surfaces, VR rigs and patching environments.
//!
//! ```text
//! /icarus/<path> <number>           set; e.g. /icarus/nodes/filt/params/cutoff/normalized 0.5
//! /icarus/<path> <string>           set, the string parsed as JSON
//! /icarus/<path>                    get; the reply goes back to the sender on the same address
//! /icarus/touch/<node>/<param> 1|0  begin/end a gesture; sets of that param in between are moves
//! /icarus/request <json>            any protocol request; reply on /icarus/reply
//! ```

use crate::Shared;
use icarus_model::{Gesture, Request};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::thread::JoinHandle;

const PREFIX: &str = "/icarus";

pub fn spawn(shared: Shared, port: u16) -> anyhow::Result<JoinHandle<()>> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    eprintln!("icarus: OSC on udp port {port}");
    Ok(std::thread::spawn(move || {
        let mut touches: HashMap<SocketAddr, BTreeSet<(String, String)>> = HashMap::new();
        let mut buf = [0u8; rosc::decoder::MTU];
        loop {
            let Ok((len, from)) = socket.recv_from(&mut buf) else {
                continue;
            };
            let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) else {
                continue;
            };
            let held = touches.entry(from).or_default();
            for msg in flatten(packet) {
                if let Some(reply) = handle(&shared, from, held, &msg)
                    && let Ok(bytes) = rosc::encoder::encode(&OscPacket::Message(reply))
                {
                    let _ = socket.send_to(&bytes, from);
                }
            }
        }
    }))
}

fn flatten(packet: OscPacket) -> Vec<OscMessage> {
    match packet {
        OscPacket::Message(m) => vec![m],
        OscPacket::Bundle(b) => b.content.into_iter().flat_map(flatten).collect(),
    }
}

fn arg_value(arg: &OscType) -> Option<Value> {
    Some(match arg {
        OscType::Float(f) => json!(f),
        OscType::Double(d) => json!(d),
        OscType::Int(i) => json!(i),
        OscType::Long(l) => json!(l),
        OscType::Bool(b) => json!(b),
        OscType::Nil => Value::Null,
        OscType::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!(s)),
        _ => return None,
    })
}

fn to_osc(v: &Value) -> OscType {
    match v {
        Value::Number(n) => OscType::Float(n.as_f64().unwrap_or(0.0) as f32),
        Value::String(s) => OscType::String(s.clone()),
        other => OscType::String(other.to_string()),
    }
}

/// Handles one message; returns a reply for gets, errors and raw requests.
fn handle(
    shared: &Shared,
    from: SocketAddr,
    held: &mut BTreeSet<(String, String)>,
    msg: &OscMessage,
) -> Option<OscMessage> {
    let path = msg.addr.strip_prefix(PREFIX)?;
    let client = format!("osc:{from}");
    let mut session = shared.lock().expect("session lock");
    let reply = |addr: &str, v: Value| {
        Some(OscMessage {
            addr: addr.to_string(),
            args: vec![to_osc(&v)],
        })
    };
    let error = |e: String| reply("/icarus/error", json!(e));

    if path == "/request" {
        let request = msg.args.first().and_then(arg_value)?;
        let result = serde_json::from_value::<Request>(request)
            .map_err(|e| e.to_string())
            .and_then(|r| session.handle(&client, r).map_err(|e| e.to_string()));
        return match result {
            Ok(v) => reply("/icarus/reply", v),
            Err(e) => error(e),
        };
    }

    if let Some(rest) = path.strip_prefix("/touch/") {
        let (node, param) = rest.split_once('/')?;
        let key = (node.to_string(), param.to_string());
        let down = msg
            .args
            .first()
            .and_then(arg_value)
            .is_none_or(|v| v.as_f64().unwrap_or(1.0) > 0.0 || v == true);
        let gesture = if down {
            Gesture::Begin {
                node: key.0.clone(),
                param: key.1.clone(),
                record: false,
            }
        } else {
            Gesture::End {
                node: key.0.clone(),
                param: key.1.clone(),
            }
        };
        return match session.handle(&client, Request::Gesture { gesture }) {
            Ok(_) => {
                if down {
                    held.insert(key)
                } else {
                    held.remove(&key)
                };
                None
            }
            Err(e) => error(e.to_string()),
        };
    }

    let Some(value) = msg.args.first().and_then(arg_value) else {
        return match session.handle(&client, Request::Get { path: path.to_string() }) {
            Ok(v) => reply(&msg.addr, v),
            Err(e) => error(e.to_string()),
        };
    };

    // While a param is touched, its sets are gesture moves (one undo step, like a real knob).
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let request = match segs.as_slice() {
        ["nodes", node, "params", param] if held.contains(&(node.to_string(), param.to_string())) => {
            value.as_f64().map(|v| Request::Gesture {
                gesture: Gesture::Move {
                    node: node.to_string(),
                    param: param.to_string(),
                    value: v,
                    at_s: None,
                },
            })
        }
        ["nodes", node, "params", param, "normalized"] if held.contains(&(node.to_string(), param.to_string())) => {
            let plain = session
                .project()
                .nodes
                .get(*node)
                .and_then(|n| icarus_engine::devices::info(&n.kind))
                .and_then(|d| d.params.iter().find(|p| p.id == *param))
                .zip(value.as_f64())
                .map(|(spec, n)| spec.from_normalized(n));
            plain.map(|v| Request::Gesture {
                gesture: Gesture::Move {
                    node: node.to_string(),
                    param: param.to_string(),
                    value: v,
                    at_s: None,
                },
            })
        }
        _ => None,
    }
    .unwrap_or(Request::Set {
        path: path.to_string(),
        value,
    });

    match session.handle(&client, request) {
        Ok(_) => None,
        Err(e) => error(e.to_string()),
    }
}
