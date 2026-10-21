//! WebSocket, for browsers, WebXR and game engines.
//!
//! Same messages as the JSON socket (see `socket.rs`), one per text frame: requests or
//! envelopes in, `{"id", "ok", ...}` replies and `{"event": ...}` pushes out.
//!
//! There is no authentication or origin check yet: anything that can reach the port controls
//! the session, so bind to localhost unless the network is trusted.

use crate::Shared;
use crate::socket::handle_message;
use serde_json::json;
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::{Error, Message};

/// How long a read waits before the connection checks for events to push.
const POLL: Duration = Duration::from_millis(5);

pub fn spawn(shared: Shared, addr: &str) -> anyhow::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("icarus: WebSocket on ws://{}", listener.local_addr()?);
    Ok(std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = shared.clone();
            std::thread::spawn(move || {
                if let Err(e) = serve(&shared, stream) {
                    eprintln!("icarus: websocket client error: {e}");
                }
            });
        }
    }))
}

fn serve(shared: &Shared, stream: TcpStream) -> anyhow::Result<()> {
    let default_client = format!("ws:{}", stream.peer_addr()?);
    let mut ws = tungstenite::accept(stream)?;
    // Reads time out so one thread can both answer requests and push events.
    ws.get_ref().set_read_timeout(Some(POLL))?;
    let (feed, events) = shared.lock().expect("session lock").open_feed();

    let result = loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let reply = handle_message(shared, feed, &default_client, &text);
                if let Err(e) = ws.send(Message::text(reply.to_string())) {
                    break Err(e);
                }
            }
            Ok(Message::Close(_)) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => break Ok(()),
            Ok(_) => {}
            Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => break Err(e),
        }
        let mut pushed = false;
        let mut failed = None;
        for event in events.try_iter() {
            if let Err(e) = ws.write(Message::text(json!({ "event": event }).to_string())) {
                failed = Some(e);
                break;
            }
            pushed = true;
        }
        if let Some(e) = failed {
            break Err(e);
        }
        if pushed && let Err(e) = ws.flush() {
            break Err(e);
        }
    };
    shared.lock().expect("session lock").close_feed(feed);
    match result {
        Ok(()) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
