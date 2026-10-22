//! Serves the stock GUI over plain HTTP on the WebSocket port.
//!
//! The files are compiled in, so `icarus gui` works anywhere. With `--gui-dir` they are read
//! from disk on every request instead, so the GUI can be edited live and reloaded.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

static FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../gui/index.html")),
    ("style.css", include_str!("../gui/style.css")),
    ("app.js", include_str!("../gui/app.js")),
    ("client.js", include_str!("../gui/client.js")),
    ("widgets.js", include_str!("../gui/widgets.js")),
    ("tracks.js", include_str!("../gui/tracks.js")),
    ("sandbox.js", include_str!("../gui/sandbox.js")),
    ("panels/transport.js", include_str!("../gui/panels/transport.js")),
    ("panels/browser.js", include_str!("../gui/panels/browser.js")),
    ("panels/mixer.js", include_str!("../gui/panels/mixer.js")),
    ("panels/devices.js", include_str!("../gui/panels/devices.js")),
    ("panels/graph.js", include_str!("../gui/panels/graph.js")),
    ("panels/automation.js", include_str!("../gui/panels/automation.js")),
    ("panels/inspector.js", include_str!("../gui/panels/inspector.js")),
    ("panels/log.js", include_str!("../gui/panels/log.js")),
    ("panels/controls.js", include_str!("../gui/panels/controls.js")),
];

fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Reads the request line and answers it; the connection is closed afterwards.
pub fn serve_http(mut stream: TcpStream, gui_dir: Option<&PathBuf>) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let target = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = target.split(['?', '#']).next().unwrap_or("/").trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    let body: Option<Vec<u8>> = if path.split('/').any(|seg| seg == "..") {
        None
    } else if let Some(dir) = gui_dir {
        std::fs::read(dir.join(path)).ok()
    } else {
        FILES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, body)| body.as_bytes().to_vec())
    };

    let (status, ctype, body) = match body {
        Some(b) => ("200 OK", content_type(path), b),
        None => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

/// True when the waiting request asks to upgrade to a WebSocket. Peeks, so nothing is consumed.
pub fn is_websocket(stream: &TcpStream) -> std::io::Result<bool> {
    let mut buf = [0u8; 4096];
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    for _ in 0..50 {
        let n = stream.peek(&mut buf)?;
        let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
        if head.contains("\r\n\r\n") || n == buf.len() {
            return Ok(head.contains("upgrade: websocket"));
        }
        if n == 0 {
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(false)
}
