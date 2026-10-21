use super::*;
use icarus_model::RenderSpec;

fn req(json: Value) -> Request {
    serde_json::from_value(json).unwrap()
}

fn session_with_chain() -> Session {
    let mut s = Session::new(Project::default()).unwrap();
    s.handle(
        "test",
        req(json!({"type": "transaction", "label": "build", "commands": [
            {"op": "add_node", "id": "osc", "kind": "osc", "params": {"freq": 110.0}},
            {"op": "add_node", "id": "filt", "kind": "filter"},
            {"op": "connect", "from": "osc", "to": "filt"},
            {"op": "connect", "from": "filt", "to": "master"},
        ]})),
    )
    .unwrap();
    s
}

#[test]
fn transaction_is_all_or_nothing() {
    let mut s = session_with_chain();
    let before = s.project().clone();
    let err = s.handle(
        "agent",
        req(json!({"type": "transaction", "commands": [
            {"op": "set_param", "node": "filt", "param": "cutoff", "value": 300.0},
            {"op": "connect", "from": "master", "to": "osc"},
        ]})),
    );
    assert!(err.is_err(), "cycle must be rejected");
    assert_eq!(s.project(), &before);
}

#[test]
fn gesture_is_one_undo_step_and_records_automation() {
    let mut s = session_with_chain();
    let g = |phase: Value| req(json!({"type": "gesture", "gesture": phase}));
    s.handle(
        "agent",
        g(json!({"phase": "begin", "node": "filt", "param": "cutoff", "record": true})),
    )
    .unwrap();
    for (i, hz) in [200.0, 800.0, 3_200.0].iter().enumerate() {
        s.handle(
            "agent",
            g(json!({"phase": "move", "node": "filt", "param": "cutoff", "value": hz, "at_s": i as f64})),
        )
        .unwrap();
    }
    // Another client cannot grab a control that is being held.
    assert!(
        s.handle(
            "human",
            g(json!({"phase": "move", "node": "filt", "param": "cutoff", "value": 1.0}))
        )
        .is_err()
    );
    s.handle("agent", g(json!({"phase": "end", "node": "filt", "param": "cutoff"})))
        .unwrap();

    assert_eq!(s.project().nodes["filt"].automation["cutoff"].len(), 3);
    let history = s.handle("x", Request::History).unwrap();
    assert_eq!(history["undo"][0]["client"], "agent");
    s.handle("x", Request::Undo).unwrap();
    assert!(s.project().nodes["filt"].automation.is_empty());
}

#[test]
fn moves_need_a_touch() {
    let mut s = session_with_chain();
    let r = s.handle(
        "agent",
        req(
            json!({"type": "gesture", "gesture": {"phase": "move", "node": "filt", "param": "cutoff", "value": 500.0}}),
        ),
    );
    assert!(r.is_err());
}

#[test]
fn paths_read_write_and_list() {
    let mut s = session_with_chain();
    let get = |s: &mut Session, p: &str| s.handle("osc-surface", Request::Get { path: p.into() }).unwrap();
    assert_eq!(get(&mut s, "/nodes/osc/params/freq"), json!(110.0));

    s.handle(
        "osc-surface",
        Request::Set {
            path: "/nodes/filt/params/cutoff/normalized".into(),
            value: json!(1.0),
        },
    )
    .unwrap();
    assert_eq!(get(&mut s, "/nodes/filt/params/cutoff"), json!(20_000.0));

    s.handle(
        "vr",
        Request::Set {
            path: "/nodes/amp".into(),
            value: json!({"kind": "gain"}),
        },
    )
    .unwrap();
    assert_eq!(get(&mut s, "/nodes/amp/kind"), json!("gain"));
    let listed = s
        .handle(
            "vr",
            Request::List {
                path: "/nodes/filt/params".into(),
            },
        )
        .unwrap();
    assert!(listed.as_array().unwrap().contains(&json!("/nodes/filt/params/cutoff")));

    s.handle(
        "vr",
        Request::Set {
            path: "/nodes/amp".into(),
            value: Value::Null,
        },
    )
    .unwrap();
    assert!(
        s.handle(
            "vr",
            Request::Get {
                path: "/nodes/amp".into()
            }
        )
        .is_err()
    );
}

#[test]
fn render_reports_master_and_taps() {
    let mut s = session_with_chain();
    let out = s
        .handle(
            "agent",
            Request::Render {
                spec: RenderSpec {
                    start_s: 0.0,
                    end_s: 0.25,
                    block_size: 256,
                    taps: vec!["osc".into()],
                },
                wav_path: None,
            },
        )
        .unwrap();
    assert!(out["master"]["rms_db"].as_f64().unwrap() > -40.0);
    // The filter darkens the oscillator.
    assert!(out["taps"]["osc"]["centroid_hz"].as_f64() > out["master"]["centroid_hz"].as_f64());
}
