//! OSC over UDP, for control surfaces, VR rigs and patching environments.
//!
//! ```text
//! /icarus/<path> <number>                set; e.g. /icarus/nodes/filt/params/cutoff/normalized 0.5
//! /icarus/<path> <string>                set, the string parsed as JSON
//! /icarus/<path>                         get; the reply goes back to the sender on the same address
//! /icarus/touch/<node>/<param> 1|0       begin/end a gesture; sets of that param in between are moves
//! /icarus/subscribe <path> [reply_port]  push changes under <path> back as /icarus/<changed path> <value>
//!                                        (params also as .../normalized), to reply_port if given
//! /icarus/unsubscribe [path]             stop one prefix, or all
//! /icarus/request <json>                 any protocol request; reply on /icarus/reply
//! ```

use crate::Shared;
use icarus_model::{Gesture, Request};
use icarus_session::{Event, FeedId};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const PREFIX: &str = "/icarus";

/// What the adapter remembers about one sender.
#[derive(Default)]
struct Peer {
    held: BTreeSet<(String, String)>,
    feed: Option<FeedId>,
    /// Where this peer's events go; shared with its forwarding thread.
    reply_to: Arc<Mutex<Option<SocketAddr>>>,
}

pub fn spawn(shared: Shared, port: u16) -> anyhow::Result<JoinHandle<()>> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    eprintln!("icarus: OSC on udp port {port}");
    Ok(std::thread::spawn(move || {
        let mut peers: HashMap<SocketAddr, Peer> = HashMap::new();
        let mut buf = [0u8; rosc::decoder::MTU];
        loop {
            let Ok((len, from)) = socket.recv_from(&mut buf) else {
                continue;
            };
            let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) else {
                continue;
            };
            let peer = peers.entry(from).or_default();
            for msg in flatten(packet) {
                if let Some(reply) = handle(&shared, &socket, from, peer, &msg) {
                    send(&socket, from, reply);
                }
            }
        }
    }))
}

fn send(socket: &UdpSocket, to: SocketAddr, msg: OscMessage) {
    if let Ok(bytes) = rosc::encoder::encode(&OscPacket::Message(msg)) {
        let _ = socket.send_to(&bytes, to);
    }
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
        Value::Bool(b) => OscType::Bool(*b),
        Value::Null => OscType::Nil,
        other => OscType::String(other.to_string()),
    }
}

/// Starts a thread that turns this peer's events into OSC messages.
fn forward_events(
    socket: &UdpSocket,
    events: std::sync::mpsc::Receiver<Event>,
    reply_to: Arc<Mutex<Option<SocketAddr>>>,
) {
    let Ok(socket) = socket.try_clone() else { return };
    std::thread::spawn(move || {
        for event in events {
            let Some(to) = *reply_to.lock().expect("reply lock") else {
                continue;
            };
            for change in event.changes {
                let addr = format!("{PREFIX}{}", change.path);
                if let Some(n) = change.normalized {
                    send(
                        &socket,
                        to,
                        OscMessage {
                            addr: format!("{addr}/normalized"),
                            args: vec![OscType::Float(n as f32)],
                        },
                    );
                }
                send(
                    &socket,
                    to,
                    OscMessage {
                        addr,
                        args: vec![to_osc(&change.value)],
                    },
                );
            }
        }
    });
}

/// Handles one message; returns a reply for gets, errors and raw requests.
fn handle(
    shared: &Shared,
    socket: &UdpSocket,
    from: SocketAddr,
    peer: &mut Peer,
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

    if path == "/subscribe" || path == "/unsubscribe" {
        let prefix = msg
            .args
            .first()
            .and_then(arg_value)
            .and_then(|v| v.as_str().map(str::to_string));
        let request = if path == "/subscribe" {
            let port = msg.args.get(1).and_then(arg_value).and_then(|v| v.as_u64());
            *peer.reply_to.lock().expect("reply lock") =
                Some(port.map_or(from, |p| SocketAddr::new(from.ip(), p as u16)));
            if peer.feed.is_none() {
                let (feed, events) = session.open_feed();
                forward_events(socket, events, peer.reply_to.clone());
                peer.feed = Some(feed);
            }
            Request::Subscribe {
                paths: vec![prefix.unwrap_or_else(|| "/".into())],
            }
        } else {
            Request::Unsubscribe {
                paths: prefix.into_iter().collect(),
            }
        };
        return match session.handle_with_feed(&client, peer.feed, request) {
            Ok(_) => None,
            Err(e) => error(e.to_string()),
        };
    }

    if path == "/request" {
        let request = msg.args.first().and_then(arg_value)?;
        let result = serde_json::from_value::<Request>(request)
            .map_err(|e| e.to_string())
            .and_then(|r| {
                session
                    .handle_with_feed(&client, peer.feed, r)
                    .map_err(|e| e.to_string())
            });
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
                    peer.held.insert(key)
                } else {
                    peer.held.remove(&key)
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
        ["nodes", node, "params", param] if peer.held.contains(&(node.to_string(), param.to_string())) => {
            value.as_f64().map(|v| Request::Gesture {
                gesture: Gesture::Move {
                    node: node.to_string(),
                    param: param.to_string(),
                    value: v,
                    at_s: None,
                },
            })
        }
        ["nodes", node, "params", param, "normalized"]
            if peer.held.contains(&(node.to_string(), param.to_string())) =>
        {
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
