//! Everything here goes through the same protocol an agent uses.

use icarus_model::{Project, Request};
use icarus_session::Session;
use serde_json::{Value, json};
use std::path::Path;

fn send(session: &mut Session, request: Value) -> anyhow::Result<Value> {
    Ok(session.handle("demo", serde_json::from_value::<Request>(request)?)?)
}

pub fn run(out: &Path) -> anyhow::Result<()> {
    let mut s = Session::new(Project::default())?;

    println!("1. Build a patch: saw -> filter -> gain -> master");
    send(
        &mut s,
        json!({"type": "transaction", "label": "build patch", "commands": [
            {"op": "add_node", "id": "osc", "kind": "osc", "params": {"freq": 55.0, "shape": 1.0, "level": 0.6}},
            {"op": "add_node", "id": "filt", "kind": "filter", "params": {"resonance": 0.6}},
            {"op": "add_node", "id": "amp", "kind": "gain", "params": {"gain": -6.0}},
            {"op": "connect", "from": "osc", "to": "filt"},
            {"op": "connect", "from": "filt", "to": "amp"},
            {"op": "connect", "from": "amp", "to": "master"},
            {"op": "set_length", "length_s": 4.0},
        ]}),
    )?;

    println!("2. Sweep the cutoff with a recorded gesture (a finger on the knob)");
    let gesture = |g: Value| json!({"type": "gesture", "gesture": g});
    send(
        &mut s,
        gesture(json!({"phase": "begin", "node": "filt", "param": "cutoff", "record": true})),
    )?;
    for i in 0..=40 {
        let t = f64::from(i) / 10.0;
        let hz = 150.0 * (6_000.0_f64 / 150.0).powf(t / 4.0);
        send(
            &mut s,
            gesture(json!({"phase": "move", "node": "filt", "param": "cutoff", "value": hz, "at_s": t})),
        )?;
    }
    send(
        &mut s,
        gesture(json!({"phase": "end", "node": "filt", "param": "cutoff"})),
    )?;

    println!("3. Render offline and measure, second by second");
    let path = out.to_string_lossy();
    send(
        &mut s,
        json!({"type": "render", "spec": {"start_s": 0.0, "end_s": 4.0}, "wav_path": path}),
    )?;
    for sec in 0..4 {
        let r = send(
            &mut s,
            json!({"type": "render", "spec": {"start_s": sec, "end_s": sec + 1, "taps": ["osc"]}}),
        )?;
        println!(
            "   {sec}-{}s  master centroid {:>6.0} Hz  rms {:>6.1} dB   (osc before filter: {:>6.0} Hz)",
            sec + 1,
            r["master"]["centroid_hz"].as_f64().unwrap_or(0.0),
            r["master"]["rms_db"].as_f64().unwrap_or(0.0),
            r["taps"]["osc"]["centroid_hz"].as_f64().unwrap_or(0.0),
        );
    }
    println!("   wrote {path}");

    println!("4. Determinism: render with block sizes 1, 64 and 1024 and compare bits");
    let project = s.project().clone();
    let renders: Vec<_> = [1, 64, 1024]
        .into_iter()
        .map(|block_size| {
            let spec = icarus_model::RenderSpec {
                start_s: 0.0,
                end_s: 4.0,
                block_size,
                taps: vec![],
            };
            icarus_render::render(&project, &spec).map(|r| r.master)
        })
        .collect::<Result<_, _>>()?;
    let identical = renders.windows(2).all(|w| w[0] == w[1]);
    println!("   bit-identical: {identical}");
    anyhow::ensure!(identical, "renders differ");

    println!("5. Probe: 5 cutoffs x 3 resonances on forked copies, rendered in parallel");
    let started = std::time::Instant::now();
    let probe = send(
        &mut s,
        json!({"type": "probe", "spec": {
            "start_s": 1.0, "end_s": 2.0, "listen": "filt",
            "sweeps": [
                {"node": "filt", "param": "cutoff", "values": [200, 500, 1000, 2500, 6000]},
                {"node": "filt", "param": "resonance", "values": [0.0, 0.5, 0.9]},
            ]
        }}),
    )?;
    println!(
        "   {} variants in {:.0} ms. Strongest effects:",
        probe["rows"].as_array().map_or(0, Vec::len),
        started.elapsed().as_secs_f64() * 1_000.0
    );
    for e in probe["sensitivity"].as_array().into_iter().flatten().take(5) {
        println!(
            "   {:<16} -> {:<12} {:>+9.1} over full range",
            e["param"].as_str().unwrap_or(""),
            e["feature"].as_str().unwrap_or(""),
            e["change_over_full_range"].as_f64().unwrap_or(0.0)
        );
    }

    println!("6. History");
    for step in send(&mut s, json!({"type": "history"}))?["undo"]
        .as_array()
        .into_iter()
        .flatten()
    {
        println!(
            "   {} by {}",
            step["label"].as_str().unwrap_or(""),
            step["client"].as_str().unwrap_or("")
        );
    }
    Ok(())
}
