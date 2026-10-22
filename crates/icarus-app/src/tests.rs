//! Drives the real app, headless, against a real session while an "agent" works the same
//! session directly, and checks that each sees the other. Screenshots go to target/.

use crate::app::{IcarusApp, Studio};
use crate::link::Link;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use icarus_client::{Local, Shared};
use icarus_model::{Project, Request};
use icarus_session::Session;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn agent(shared: &Shared, request: Value) -> Value {
    let request: Request = serde_json::from_value(request).expect("valid request");
    shared.lock().unwrap().handle("agent", request).expect("agent request")
}

fn get(shared: &Shared, path: &str) -> Value {
    shared
        .lock()
        .unwrap()
        .handle("test", Request::Get { path: path.into() })
        .unwrap_or(Value::Null)
}

/// Steps the app until `done` holds; events arrive on a background thread.
fn settle(h: &mut Harness<'_, IcarusApp>, mut done: impl FnMut(&mut Harness<'_, IcarusApp>) -> bool) -> bool {
    for _ in 0..200 {
        h.step();
        if done(h) {
            h.step();
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn screenshot(h: &mut Harness<'_, IcarusApp>, name: &str) {
    // Rendering needs a GPU adapter; without one (e.g. bare CI) the checks still run.
    match h.render() {
        Ok(image) => {
            let path = format!("{}/../../target/{name}.png", env!("CARGO_MANIFEST_DIR"));
            image.save(&path).expect("save screenshot");
        }
        Err(e) => eprintln!("no screenshot ({e})"),
    }
}

fn drag(h: &mut Harness<'_, IcarusApp>, from: eframe::egui::Pos2, by: eframe::egui::Vec2) {
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=10 {
        h.hover_at(from + by * (i as f32 / 10.0));
        h.step();
    }
    h.drop_at(from + by);
    h.step();
}

#[test]
fn app_and_agent_share_one_session() {
    let shared: Shared = Arc::new(Mutex::new(Session::new(Project::default()).unwrap()));
    let (connection, events) = Local::connect(shared.clone());
    let mut h = Harness::builder()
        .with_size([1500.0, 950.0])
        .wgpu()
        .build_eframe(move |cc| {
            // Tests keep egui's panic on missing glyphs, so a symbol no font can draw fails loudly.
            assert!(
                crate::fonts::add_fallbacks(&cc.egui_ctx) > 0,
                "no fallback fonts found; install DejaVu or Noto"
            );
            let link = Link::new(Box::new(connection), events, "app:test".into(), cc.egui_ctx.clone());
            IcarusApp {
                studio: Studio::new(link, None).expect("load session"),
            }
        });
    h.run_steps(3);

    // A person adds a track with the browser button.
    h.get_by_label("+ Synth track").click();
    h.run_steps(2);
    let tracks = get(&shared, "/ui/tracks");
    assert_eq!(
        tracks.as_array().map(Vec::len),
        Some(1),
        "track added through the protocol"
    );
    let osc = tracks[0]["devices"][0].as_str().unwrap().to_string();
    let mix = tracks[0]["mix"].as_str().unwrap().to_string();
    assert!(
        settle(&mut h, |h| h.query_by_label(&format!("{osc} Frequency")).is_some()),
        "device knobs appear"
    );

    // The agent grabs the synth's pitch: the app sees whose hand is on it, then the new value.
    agent(
        &shared,
        json!({"type": "gesture", "gesture": {"phase": "begin", "node": osc, "param": "freq"}}),
    );
    assert!(
        settle(&mut h, |h| h.state().studio.m.held_by(&osc, "freq") == Some("agent")),
        "agent's touch reaches the app"
    );
    agent(
        &shared,
        json!({"type": "gesture", "gesture": {"phase": "move", "node": osc, "param": "freq", "value": 440}}),
    );
    assert!(
        settle(&mut h, |h| h.state().studio.m.value(&osc, "freq") == 440.0),
        "agent's move reaches the app"
    );
    screenshot(&mut h, "app-agent-holding");
    agent(
        &shared,
        json!({"type": "gesture", "gesture": {"phase": "end", "node": osc, "param": "freq"}}),
    );
    assert!(settle(&mut h, |h| h.state().studio.m.held_by(&osc, "freq").is_none()));

    // The person drags the track's fader up: one gesture, attributed to the app.
    let before = get(&shared, &format!("/nodes/{mix}/params/gain")).as_f64().unwrap();
    let fader = h.get_by_label(&format!("{mix} Gain")).rect();
    drag(&mut h, fader.center(), eframe::egui::vec2(0.0, -40.0));
    let after = get(&shared, &format!("/nodes/{mix}/params/gain")).as_f64().unwrap();
    assert!(after > before, "fader drag raised the gain ({before} -> {after})");
    let history = shared.lock().unwrap().handle("test", Request::History).unwrap();
    assert_eq!(history["undo"][0]["client"], "app:test");
    assert_eq!(history["undo"][0]["label"], format!("gesture {mix}.gain"));

    // Space comes from /ui/keys and starts the transport.
    h.key_press(eframe::egui::Key::Space);
    h.run_steps(2);
    assert_eq!(get(&shared, "/transport/playing"), json!(true));
    h.key_press(eframe::egui::Key::Space);
    h.run_steps(2);

    // The agent restyles the app and composes a native panel from data alone.
    agent(
        &shared,
        json!({"type": "set", "path": "/ui/theme/--accent", "value": "#ff2d95"}),
    );
    agent(
        &shared,
        json!({"type": "transaction", "label": "agent builds a macro panel", "commands": [
            {"op": "set_ui", "path": "/panels/macros", "value": {"type": "controls", "area": "bottom", "title": "Macros", "order": -1,
                "props": {"widgets": [{"label": "Agent macros"}, {"knob": format!("{osc}.freq"), "label": "Pitch"}, {"xy": [format!("{osc}.shape"), format!("{mix}.pan")], "size": 150}]}}},
        ]}),
    );
    assert!(
        settle(&mut h, |h| h.state().studio.pal.accent
            == eframe::egui::Color32::from_hex("#ff2d95").unwrap()),
        "theme from /ui reaches the app"
    );
    assert!(
        settle(&mut h, |h| h.query_by_label("Agent macros").is_some()),
        "agent-composed panel renders natively"
    );
    assert!(h.query_by_label(&format!("{osc} Frequency")).is_some());
    screenshot(&mut h, "app-agent-panel");

    // Ctrl+Z in the app undoes the agent's last change, whoever made it.
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::Z);
    h.run_steps(2);
    assert!(
        get(&shared, "/ui/panels/macros").is_null(),
        "undo from the app reverted the agent's panel"
    );
}
