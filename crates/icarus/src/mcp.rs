//! MCP over stdio, so any MCP-capable agent can drive the session.
//!
//! The tools are thin: each one builds protocol [`Request`]s, and `request` accepts any request
//! verbatim, so an agent is never limited to what the curated tools cover.

use crate::Shared;
use icarus_model::{Gesture, ProbeSpec, RenderSpec, Request};
use icarus_session::{Event, FeedId};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::sync::mpsc::Receiver;

fn tools() -> Value {
    let path = json!({"type": "string", "description": "e.g. /nodes/filt/params/cutoff"});
    json!([
        {
            "name": "list",
            "description": "List child paths. Everything in the project is a path: start at / and walk down. \
                /nodes/<id>/params/<p> is a plain value, .../normalized is 0..1, .../spec explains the control.",
            "inputSchema": {"type": "object", "properties": {"path": path}}
        },
        {
            "name": "get",
            "description": "Read any path, e.g. / , /project, /nodes, /nodes/<id>, /nodes/<id>/params/<p>/spec, /devices.",
            "inputSchema": {"type": "object", "properties": {"path": path}, "required": ["path"]}
        },
        {
            "name": "set",
            "description": "Write any settable path as one undoable step. Examples: /nodes/filt/params/cutoff = 800; \
                /nodes/filt/params/cutoff/normalized = 0.4; /nodes/amp = {\"kind\":\"gain\"} adds a node, null removes it; \
                /nodes/filt/automation/cutoff = [{\"time_s\":0,\"value\":200}, ...]; /connections = [{\"from\",\"to\"}, ...].",
            "inputSchema": {"type": "object", "properties": {"path": path, "value": {}}, "required": ["path", "value"]}
        },
        {
            "name": "transaction",
            "description": "Apply several commands atomically as one undo step. Ops: add_node{id,kind,params}, remove_node{id}, \
                connect{from,to}, disconnect{from,to}, set_param{node,param,value}, set_automation{node,param,points}, \
                clear_automation{node,param}, set_length{length_s}, set_tempo{bpm}, set_seed{seed}.",
            "inputSchema": {"type": "object", "properties": {
                "label": {"type": "string", "description": "Shown in undo history"},
                "commands": {"type": "array", "items": {"type": "object", "properties": {"op": {"type": "string"}}, "required": ["op"]}}
            }, "required": ["commands"]}
        },
        {
            "name": "gesture",
            "description": "Move a control like a finger on a knob. Send events in order: {phase:begin,node,param,record?}, \
                {phase:move,node,param,value,at_s?}..., {phase:end,node,param}. The whole gesture is one undo step. \
                With record:true every move needs at_s and is written to automation. Live playback hears each move.",
            "inputSchema": {"type": "object", "properties": {
                "events": {"type": "array", "items": {"type": "object", "properties": {"phase": {"enum": ["begin", "move", "end"]}}, "required": ["phase"]}}
            }, "required": ["events"]}
        },
        {
            "name": "render",
            "description": "Render a region offline (deterministic, faster than realtime) and measure it: rms/peak/crest dB, \
                spectral centroid, low/mid/high energy share, stereo width. taps measures any node mid-chain. \
                wav_path also writes the master to a file.",
            "inputSchema": {"type": "object", "properties": {
                "start_s": {"type": "number"}, "end_s": {"type": "number"},
                "taps": {"type": "array", "items": {"type": "string"}},
                "wav_path": {"type": "string"}
            }, "required": ["start_s", "end_s"]}
        },
        {
            "name": "probe",
            "description": "Experiment without changing the project: render every combination of parameter sweeps on forked \
                copies in parallel, measure the listen node, and rank which parameter moves which feature most \
                (change_over_full_range is per normalized 0..1 of the param). Use before committing a change.",
            "inputSchema": {"type": "object", "properties": {
                "start_s": {"type": "number"}, "end_s": {"type": "number"},
                "listen": {"type": "string", "description": "Node to measure (default master)"},
                "sweeps": {"type": "array", "items": {"type": "object", "properties": {
                    "node": {"type": "string"}, "param": {"type": "string"},
                    "values": {"type": "array", "items": {"type": "number"}}
                }, "required": ["node", "param", "values"]}}
            }, "required": ["start_s", "end_s", "sweeps"]}
        },
        {"name": "undo", "description": "Undo the last step, whoever made it.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "redo", "description": "Redo the last undone step.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "history", "description": "Undo/redo history with which client made each change.", "inputSchema": {"type": "object", "properties": {}}},
        {
            "name": "subscribe",
            "description": "Watch path prefixes (e.g. /nodes/filt, /touches, or / for everything) for changes made by \
                anyone: people on OSC/VR, other agents, undo. Then call `events` to read what happened.",
            "inputSchema": {"type": "object", "properties": {"paths": {"type": "array", "items": path}}, "required": ["paths"]}
        },
        {
            "name": "unsubscribe",
            "description": "Stop watching these prefixes, or all of them when paths is empty.",
            "inputSchema": {"type": "object", "properties": {"paths": {"type": "array", "items": path}}}
        },
        {
            "name": "events",
            "description": "Changes on watched paths since the last call, oldest first: who changed what (client), the \
                step label, and new values (params include their 0..1 normalized position; null means removed). \
                /touches/<node>/<param> shows who is holding a control. A gap in seq means events were dropped.",
            "inputSchema": {"type": "object", "properties": {"max": {"type": "integer", "description": "Default 100"}}}
        },
        {
            "name": "request",
            "description": "Send any raw protocol request, e.g. {\"type\":\"save\",\"path\":\"song.json\"}. \
                Types: transaction, gesture, undo, redo, history, snapshot, get, set, list, list_devices, describe, render, probe, save, load.",
            "inputSchema": {"type": "object", "properties": {"request": {"type": "object"}}, "required": ["request"]}
        }
    ])
}

fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("`{key}`: {e}"))
}

/// This connection's event feed.
struct Feed {
    id: FeedId,
    events: Receiver<Event>,
}

fn call(shared: &Shared, feed: &Feed, client: &str, name: &str, args: &Value) -> Result<Value, String> {
    if name == "events" {
        let max = arg::<Option<usize>>(args, "max")?.unwrap_or(100);
        let events: Vec<Event> = feed.events.try_iter().take(max).collect();
        return Ok(json!({ "events": events }));
    }
    let mut session = shared.lock().expect("session lock");
    let mut run = |r: Request| {
        session
            .handle_with_feed(client, Some(feed.id), r)
            .map_err(|e| e.to_string())
    };
    match name {
        "list" => run(Request::List {
            path: arg::<Option<String>>(args, "path")?.unwrap_or_default(),
        }),
        "get" => run(Request::Get {
            path: arg(args, "path")?,
        }),
        "set" => run(Request::Set {
            path: arg(args, "path")?,
            value: args.get("value").cloned().unwrap_or(Value::Null),
        }),
        "transaction" => run(Request::Transaction {
            label: arg(args, "label")?,
            commands: arg(args, "commands")?,
        }),
        "gesture" => {
            let events: Vec<Gesture> = arg(args, "events")?;
            let mut results = Vec::with_capacity(events.len());
            for (i, gesture) in events.into_iter().enumerate() {
                results.push(run(Request::Gesture { gesture }).map_err(|e| format!("event {i}: {e}"))?);
            }
            Ok(json!(results))
        }
        "render" => run(Request::Render {
            spec: RenderSpec {
                start_s: arg(args, "start_s")?,
                end_s: arg(args, "end_s")?,
                block_size: 512,
                taps: arg::<Option<Vec<String>>>(args, "taps")?.unwrap_or_default(),
            },
            wav_path: arg(args, "wav_path")?,
        }),
        "probe" => run(Request::Probe {
            spec: ProbeSpec {
                start_s: arg(args, "start_s")?,
                end_s: arg(args, "end_s")?,
                listen: arg(args, "listen")?,
                sweeps: arg(args, "sweeps")?,
            },
        }),
        "undo" => run(Request::Undo),
        "redo" => run(Request::Redo),
        "history" => run(Request::History),
        "subscribe" => run(Request::Subscribe {
            paths: arg(args, "paths")?,
        }),
        "unsubscribe" => run(Request::Unsubscribe {
            paths: arg::<Option<Vec<String>>>(args, "paths")?.unwrap_or_default(),
        }),
        "request" => run(arg(args, "request")?),
        _ => Err(format!("unknown tool `{name}`")),
    }
}

pub fn run(shared: Shared) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut client = "mcp".to_string();
    let (id, events) = shared.lock().expect("session lock").open_feed();
    let feed = Feed { id, events };
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications need no reply
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match msg.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" => {
                if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                    client = format!("mcp:{name}");
                }
                Ok(json!({
                    "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "icarus", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Icarus is a DAW. Everything is a path: start with list('/'). Probe before changing \
                        sound, render to verify after. Every change is undoable and attributed to you."
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let (text, is_error) = match call(&shared, &feed, &client, name, &args) {
                    Ok(v) => (serde_json::to_string_pretty(&v)?, false),
                    Err(e) => (e, true),
                };
                Ok(json!({ "content": [{"type": "text", "text": text}], "isError": is_error }))
            }
            other => Err(json!({"code": -32601, "message": format!("method not found: {other}")})),
        };
        let reply = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        writeln!(stdout, "{reply}")?;
        stdout.flush()?;
    }
    Ok(())
}
