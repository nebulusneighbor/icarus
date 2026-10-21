//! Line-delimited JSON over TCP: the lowest-level way in, for scripts, game engines and agents
//! that do not speak MCP.
//!
//! Each line is a [`Request`](icarus_model::Request), or `{"client": "...", "request": {...}}` to
//! name the caller. Each reply is `{"ok": true, "result": ...}` or `{"ok": false, "error": "..."}`.

use crate::Shared;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

pub fn spawn(shared: Shared, addr: &str) -> anyhow::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("icarus: JSON socket on {}", listener.local_addr()?);
    Ok(std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = shared.clone();
            std::thread::spawn(move || {
                if let Err(e) = serve(&shared, stream) {
                    eprintln!("icarus: socket client error: {e}");
                }
            });
        }
    }))
}

fn serve(shared: &Shared, stream: TcpStream) -> std::io::Result<()> {
    let default_client = format!("socket:{}", stream.peer_addr()?);
    let mut writer = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = handle_line(shared, &default_client, &line);
        writeln!(writer, "{reply}")?;
    }
    Ok(())
}

pub fn handle_line(shared: &Shared, default_client: &str, line: &str) -> Value {
    let parsed: Result<Value, _> = serde_json::from_str(line);
    let result = parsed.map_err(|e| e.to_string()).and_then(|v| {
        let (client, request) = match v.get("request") {
            Some(r) => (
                v.get("client")
                    .and_then(Value::as_str)
                    .unwrap_or(default_client)
                    .to_string(),
                r.clone(),
            ),
            None => (default_client.to_string(), v),
        };
        let request = serde_json::from_value(request).map_err(|e| e.to_string())?;
        shared
            .lock()
            .expect("session lock")
            .handle(&client, request)
            .map_err(|e| e.to_string())
    });
    match result {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}
