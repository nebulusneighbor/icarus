//! WebSocket, for browsers, WebXR and game engines. Plain HTTP requests on the same port get
//! the stock GUI (see `gui.rs`), which connects back here as an ordinary client.
//!
//! Same messages as the JSON socket (see `socket.rs`), one per text frame: requests or
//! envelopes in, `{"id", "ok", ...}` replies and `{"event": ...}` pushes out. Events caused by a
//! request are sent before its reply, so a client's state is current when the reply arrives.
//!
//! There is no authentication or origin check yet: anything that can reach the port controls
//! the session, so bind to localhost unless the network is trusted.

use crate::Shared;
use crate::gui;
use crate::socket::handle_message;
use serde_json::json;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::{Error, Message};

/// How long a read waits before the connection checks for events to push.
const POLL: Duration = Duration::from_millis(5);

pub fn spawn(shared: Shared, addr: &str, gui_dir: Option<PathBuf>) -> anyhow::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    eprintln!("icarus: GUI on http://{local}  (WebSocket on ws://{local})");
    Ok(std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = shared.clone();
            let gui_dir = gui_dir.clone();
            std::thread::spawn(move || {
                let result = match gui::is_websocket(&stream) {
                    Ok(true) => serve(&shared, stream),
                    Ok(false) => gui::serve_http(stream, gui_dir.as_ref()).map_err(Into::into),
                    Err(e) => Err(e.into()),
                };
                if let Err(e) = result {
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
                if let Err(e) = push_events(&mut ws, &events) {
                    break Err(e);
                }
                if let Err(e) = ws.send(Message::text(reply.to_string())) {
                    break Err(e);
                }
            }
            Ok(Message::Close(_)) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => break Ok(()),
            Ok(_) => {}
            Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => break Err(e),
        }
        match push_events(&mut ws, &events) {
            Ok(true) => {
                if let Err(e) = ws.flush() {
                    break Err(e);
                }
            }
            Ok(false) => {}
            Err(e) => break Err(e),
        }
    };
    shared.lock().expect("session lock").close_feed(feed);
    match result {
        Ok(()) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Queues every pending event on the socket; returns whether there were any. The caller flushes.
fn push_events(
    ws: &mut tungstenite::WebSocket<TcpStream>,
    events: &std::sync::mpsc::Receiver<icarus_session::Event>,
) -> Result<bool, Error> {
    let mut any = false;
    for event in events.try_iter() {
        ws.write(Message::text(json!({ "event": event }).to_string()))?;
        any = true;
    }
    Ok(any)
}
