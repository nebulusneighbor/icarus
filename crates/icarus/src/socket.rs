//! Line-delimited JSON over TCP: the lowest-level way in, for scripts, game engines and agents
//! that do not speak MCP.
//!
//! Each line is a [`Request`](icarus_model::Request), or an envelope
//! `{"id": any, "client": "...", "request": {...}}` to name the caller and tag the reply.
//! Replies are `{"id", "ok": true, "result"}` or `{"id", "ok": false, "error"}`. After a
//! `{"type": "subscribe", "paths": [...]}`, change events arrive on the same connection as
//! `{"event": {...}}` lines. Events caused by a request are written before its reply.

use crate::Shared;
use icarus_session::{Event, FeedId};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
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
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let (feed, events) = shared.lock().expect("session lock").open_feed();
    // Shared so the request path can flush its own events before replying. Lock order is
    // always events, then writer.
    let events = Arc::new(Mutex::new(events));

    let (idle_events, idle_writer) = (events.clone(), writer.clone());
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(IDLE_POLL);
            let rx = idle_events.lock().expect("events lock");
            let mut w = idle_writer.lock().expect("writer lock");
            if write_pending(&rx, &mut *w).is_err() {
                break;
            }
        }
    });

    let result = (|| {
        for line in BufReader::new(stream).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = handle_message(shared, feed, &default_client, &line);
            let rx = events.lock().expect("events lock");
            let mut w = writer.lock().expect("writer lock");
            write_pending(&rx, &mut *w)?;
            writeln!(w, "{reply}")?;
        }
        Ok(())
    })();
    // Closing the feed ends the event thread.
    shared.lock().expect("session lock").close_feed(feed);
    result
}

/// How often queued events are pushed while a connection is idle.
const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(5);

/// Writes every queued event. Fails when the connection or the feed has gone away.
fn write_pending(events: &Receiver<Event>, w: &mut impl Write) -> std::io::Result<()> {
    loop {
        match events.try_recv() {
            Ok(event) => writeln!(w, "{}", json!({ "event": event }))?,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => return Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }
}

/// Handles one JSON message (bare request or envelope) and builds the reply.
pub fn handle_message(shared: &Shared, feed: FeedId, default_client: &str, text: &str) -> Value {
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return json!({ "id": null, "ok": false, "error": e.to_string() }),
    };
    let id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let (client, request) = match parsed.get("request") {
        Some(r) => (
            parsed
                .get("client")
                .and_then(Value::as_str)
                .unwrap_or(default_client)
                .to_string(),
            r.clone(),
        ),
        None => (default_client.to_string(), parsed),
    };
    let result = serde_json::from_value(request)
        .map_err(|e| e.to_string())
        .and_then(|request| {
            shared
                .lock()
                .expect("session lock")
                .handle_with_feed(&client, Some(feed), request)
                .map_err(|e| e.to_string())
        });
    match result {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    }
}
