//! The app's connection to the session. Requests go out synchronously; events arrive on a
//! background thread, which wakes the UI so it repaints only when something changed.

use icarus_client::{Connection, Events};
use icarus_model::{Command, Gesture, Request};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TOAST_FOR: Duration = Duration::from_secs(4);

pub struct Link {
    conn: Box<dyn Connection>,
    /// Who this app is in history and presence, e.g. `app:human`.
    pub name: String,
    inbox: Arc<Mutex<Vec<Value>>>,
    pub toasts: Vec<(String, Instant)>,
}

impl Link {
    pub fn new(conn: Box<dyn Connection>, events: Events, name: String, ctx: eframe::egui::Context) -> Self {
        let inbox: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = inbox.clone();
        std::thread::spawn(move || {
            while let Some(event) = events.recv() {
                sink.lock().expect("inbox").push(event);
                ctx.request_repaint();
            }
        });
        Self {
            conn,
            name,
            inbox,
            toasts: Vec::new(),
        }
    }

    pub fn request(&mut self, request: Request) -> Result<Value, String> {
        self.conn.request(&self.name, request)
    }

    /// Like [`Link::request`], but shows failures as a toast instead of returning them.
    pub fn run(&mut self, request: Request) -> Option<Value> {
        match self.request(request) {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(e);
                None
            }
        }
    }

    pub fn get(&mut self, path: &str) -> Result<Value, String> {
        self.request(Request::Get { path: path.into() })
    }

    pub fn set(&mut self, path: &str, value: Value) -> Option<Value> {
        self.run(Request::Set {
            path: path.into(),
            value,
        })
    }

    pub fn tx(&mut self, label: impl Into<String>, commands: Vec<Command>) -> Option<Value> {
        self.run(Request::Transaction {
            label: Some(label.into()),
            commands,
        })
    }

    pub fn gesture(&mut self, gesture: Gesture) -> Result<Value, String> {
        self.request(Request::Gesture { gesture })
    }

    pub fn drain(&self) -> Vec<Value> {
        std::mem::take(&mut *self.inbox.lock().expect("inbox"))
    }

    pub fn toast(&mut self, message: impl Into<String>) {
        self.toasts.push((message.into(), Instant::now()));
    }

    pub fn live_toasts(&mut self) -> &[(String, Instant)] {
        self.toasts.retain(|(_, at)| at.elapsed() < TOAST_FOR);
        &self.toasts
    }
}
