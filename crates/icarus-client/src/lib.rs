//! A connection to an Icarus session: send requests, receive change events.
//!
//! Every client that isn't a network adapter goes through this: the MCP server and the native
//! app. A [`Local`] connection shares a session in this process; a [`Remote`] one reaches a
//! session in another process through its JSON socket. Both speak the same protocol.

use icarus_model::Request;
use icarus_session::{Event, FeedId, Session};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// A session shared by everything in one process.
pub type Shared = Arc<Mutex<Session>>;

/// Change events for whatever a connection subscribed to, as JSON.
///
/// A local connection reads the session's feed directly, so an event is available as soon as
/// the request that caused it returns; a remote one reads them off the socket.
pub enum Events {
    Local(Receiver<Event>),
    Remote(Receiver<Value>),
}

impl Events {
    /// Waits for the next event; `None` once the connection is gone.
    pub fn recv(&self) -> Option<Value> {
        match self {
            Events::Local(rx) => rx.recv().ok().map(|e| json!(e)),
            Events::Remote(rx) => rx.recv().ok(),
        }
    }

    /// The events already queued, oldest first.
    pub fn drain(&self, max: usize) -> Vec<Value> {
        match self {
            Events::Local(rx) => rx.try_iter().take(max).map(|e| json!(e)).collect(),
            Events::Remote(rx) => rx.try_iter().take(max).collect(),
        }
    }
}

pub trait Connection: Send {
    /// Sends one request as `client` and waits for the result.
    fn request(&mut self, client: &str, request: Request) -> Result<Value, String>;
}

/// A session in this process.
pub struct Local {
    shared: Shared,
    feed: FeedId,
}

impl Local {
    /// Connects to `shared`. Events for whatever this connection subscribes to arrive on the
    /// returned [`Events`].
    pub fn connect(shared: Shared) -> (Self, Events) {
        let (feed, events) = shared.lock().expect("session lock").open_feed();
        (Self { shared, feed }, Events::Local(events))
    }
}

impl Connection for Local {
    fn request(&mut self, client: &str, request: Request) -> Result<Value, String> {
        self.shared
            .lock()
            .expect("session lock")
            .handle_with_feed(client, Some(self.feed), request)
            .map_err(|e| e.to_string())
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        if let Ok(mut session) = self.shared.lock() {
            session.close_feed(self.feed);
        }
    }
}

/// A session in another process, reached through its JSON socket.
pub struct Remote {
    writer: TcpStream,
    replies: Receiver<Value>,
    next_id: u64,
}

impl Remote {
    pub fn connect(addr: &str) -> anyhow::Result<(Self, Events)> {
        let stream = TcpStream::connect(addr)?;
        let reader = BufReader::new(stream.try_clone()?);
        let (reply_tx, replies) = channel();
        let (event_tx, events): (Sender<Value>, _) = channel();
        std::thread::spawn(move || {
            for line in reader.lines() {
                let Some(mut msg) = line.ok().and_then(|l| serde_json::from_str::<Value>(&l).ok()) else {
                    break;
                };
                let sent = match msg.get_mut("event") {
                    Some(event) => event_tx.send(event.take()).is_ok(),
                    None => reply_tx.send(msg).is_ok(),
                };
                if !sent {
                    break;
                }
            }
        });
        Ok((
            Self {
                writer: stream,
                replies,
                next_id: 0,
            },
            Events::Remote(events),
        ))
    }
}

impl Connection for Remote {
    fn request(&mut self, client: &str, request: Request) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({ "id": id, "client": client, "request": request });
        writeln!(self.writer, "{line}").map_err(|e| format!("session connection lost: {e}"))?;
        loop {
            let reply = self
                .replies
                .recv()
                .map_err(|_| "session connection closed".to_string())?;
            if reply["id"] != json!(id) {
                continue;
            }
            return if reply["ok"] == json!(true) {
                Ok(reply["result"].clone())
            } else {
                Err(reply["error"].as_str().unwrap_or("request failed").to_string())
            };
        }
    }
}
