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

fn drain(rx: &std::sync::mpsc::Receiver<Event>) -> Vec<Event> {
    rx.try_iter().collect()
}

#[test]
fn subscribers_get_only_their_paths() {
    let mut s = session_with_chain();
    let (filt_feed, filt_rx) = s.open_feed();
    let (all_feed, all_rx) = s.open_feed();
    s.handle_with_feed(
        "vr",
        Some(filt_feed),
        Request::Subscribe {
            paths: vec!["/nodes/filt".into()],
        },
    )
    .unwrap();
    s.handle_with_feed(
        "agent",
        Some(all_feed),
        Request::Subscribe {
            paths: vec!["/".into()],
        },
    )
    .unwrap();

    s.handle(
        "human",
        req(json!({"type": "transaction", "commands": [
            {"op": "set_param", "node": "filt", "param": "cutoff", "value": 2_000.0},
            {"op": "set_param", "node": "osc", "param": "freq", "value": 220.0},
        ]})),
    )
    .unwrap();

    let filt_events = drain(&filt_rx);
    assert_eq!(filt_events.len(), 1);
    assert_eq!(filt_events[0].client, "human");
    assert_eq!(filt_events[0].changes.len(), 1);
    let change = &filt_events[0].changes[0];
    assert_eq!(change.path, "/nodes/filt/params/cutoff");
    assert_eq!(change.value, json!(2_000.0));
    assert!(change.normalized.unwrap() > 0.5);

    let all = drain(&all_rx);
    assert_eq!(all[0].changes.len(), 2);
    assert_eq!(all[0].seq, filt_events[0].seq);
}

#[test]
fn gestures_broadcast_presence_moves_and_release() {
    let mut s = session_with_chain();
    let (feed, rx) = s.open_feed();
    s.handle_with_feed(
        "vr",
        Some(feed),
        Request::Subscribe {
            paths: vec!["/touches".into(), "/nodes/filt/params".into()],
        },
    )
    .unwrap();
    let g = |phase: Value| req(json!({"type": "gesture", "gesture": phase}));
    s.handle("agent", g(json!({"phase": "begin", "node": "filt", "param": "cutoff"})))
        .unwrap();
    assert_eq!(
        s.handle(
            "x",
            Request::Get {
                path: "/touches".into()
            }
        )
        .unwrap(),
        json!({"filt/cutoff": "agent"})
    );
    s.handle(
        "agent",
        g(json!({"phase": "move", "node": "filt", "param": "cutoff", "value": 500.0})),
    )
    .unwrap();
    s.handle("agent", g(json!({"phase": "end", "node": "filt", "param": "cutoff"})))
        .unwrap();

    let paths: Vec<(String, Value)> = drain(&rx)
        .into_iter()
        .flat_map(|e| e.changes)
        .map(|c| (c.path, c.value))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("/touches/filt/cutoff".into(), json!("agent")),
            ("/nodes/filt/params/cutoff".into(), json!(500.0)),
            ("/touches/filt/cutoff".into(), Value::Null),
        ]
    );
}

#[test]
fn undo_and_structural_changes_emit_events() {
    let mut s = session_with_chain();
    let (feed, rx) = s.open_feed();
    s.handle_with_feed(
        "gui",
        Some(feed),
        Request::Subscribe {
            paths: vec!["/nodes/amp".into(), "/connections".into()],
        },
    )
    .unwrap();
    s.handle(
        "agent",
        Request::Set {
            path: "/nodes/amp".into(),
            value: json!({"kind": "gain"}),
        },
    )
    .unwrap();
    let added = drain(&rx);
    assert!(
        added[0]
            .changes
            .iter()
            .any(|c| c.path == "/nodes/amp/kind" && c.value == json!("gain"))
    );

    s.handle("human", Request::Undo).unwrap();
    let undone = drain(&rx);
    assert_eq!(undone[0].label, "undo set /nodes/amp");
    assert!(undone[0].changes.iter().all(|c| c.value.is_null()));
}

#[test]
fn closed_feeds_are_dropped_and_feedless_clients_cannot_subscribe() {
    let mut s = session_with_chain();
    assert!(
        s.handle(
            "mcp",
            Request::Subscribe {
                paths: vec!["/".into()]
            }
        )
        .is_err()
    );
    let (feed, rx) = s.open_feed();
    s.handle_with_feed(
        "ws",
        Some(feed),
        Request::Subscribe {
            paths: vec!["/".into()],
        },
    )
    .unwrap();
    drop(rx);
    s.handle(
        "x",
        Request::Set {
            path: "/project/tempo_bpm".into(),
            value: json!(140),
        },
    )
    .unwrap();
    assert!(
        s.handle_with_feed("ws", Some(feed), Request::Subscribe { paths: vec![] })
            .is_err()
    );
}

#[test]
fn ui_document_is_paths_with_events_and_no_op_writes_are_not_steps() {
    let mut s = session_with_chain();
    let (feed, rx) = s.open_feed();
    s.handle_with_feed(
        "gui",
        Some(feed),
        Request::Subscribe {
            paths: vec!["/ui/panels".into()],
        },
    )
    .unwrap();

    s.handle(
        "agent",
        Request::Set {
            path: "/ui/panels/mixer/area".into(),
            value: json!("bottom"),
        },
    )
    .unwrap();
    assert_eq!(
        s.handle(
            "x",
            Request::Get {
                path: "/ui/panels/mixer/area".into()
            }
        )
        .unwrap(),
        json!("bottom")
    );
    let events = drain(&rx);
    assert_eq!(
        events[0].changes,
        vec![Change {
            path: "/ui/panels/mixer/area".into(),
            value: json!("bottom"),
            normalized: None
        }]
    );

    // Deep writes create objects; null removes.
    s.handle(
        "agent",
        Request::Set {
            path: "/ui/panels/new/props/x".into(),
            value: json!("osc.freq"),
        },
    )
    .unwrap();
    assert!(
        s.handle(
            "x",
            Request::List {
                path: "/ui/panels".into()
            }
        )
        .unwrap()
        .as_array()
        .unwrap()
        .contains(&json!("/ui/panels/new"))
    );
    s.handle(
        "agent",
        Request::Set {
            path: "/ui/panels/new".into(),
            value: Value::Null,
        },
    )
    .unwrap();
    assert!(
        s.handle(
            "x",
            Request::Get {
                path: "/ui/panels/new".into()
            }
        )
        .is_err()
    );

    // Arrays append at their length.
    s.handle(
        "agent",
        Request::Set {
            path: "/ui/tracks/0".into(),
            value: json!({"name": "A"}),
        },
    )
    .unwrap();
    assert_eq!(
        s.handle(
            "x",
            Request::Get {
                path: "/ui/tracks/0/name".into()
            }
        )
        .unwrap(),
        json!("A")
    );

    let steps = s.handle("x", Request::History).unwrap()["undo"]
        .as_array()
        .unwrap()
        .len();
    s.handle(
        "agent",
        Request::Set {
            path: "/ui/tracks/0/name".into(),
            value: json!("A"),
        },
    )
    .unwrap();
    assert_eq!(
        s.handle("x", Request::History).unwrap()["undo"]
            .as_array()
            .unwrap()
            .len(),
        steps
    );
}

#[test]
fn transport_is_session_state_with_events() {
    let mut s = session_with_chain();
    let (feed, rx) = s.open_feed();
    s.handle_with_feed(
        "gui",
        Some(feed),
        Request::Subscribe {
            paths: vec!["/transport".into()],
        },
    )
    .unwrap();
    s.handle(
        "agent",
        Request::Set {
            path: "/transport/playing".into(),
            value: json!(true),
        },
    )
    .unwrap();
    assert_eq!(
        s.handle(
            "x",
            Request::Get {
                path: "/transport/playing".into()
            }
        )
        .unwrap(),
        json!(true)
    );
    assert_eq!(
        s.handle(
            "x",
            Request::Get {
                path: "/transport/live".into()
            }
        )
        .unwrap(),
        json!(false)
    );
    assert_eq!(drain(&rx)[0].changes[0].path, "/transport/playing");
}

#[test]
fn moves_accept_normalized_positions() {
    let mut s = session_with_chain();
    let g = |phase: Value| req(json!({"type": "gesture", "gesture": phase}));
    s.handle("vr", g(json!({"phase": "begin", "node": "filt", "param": "cutoff"})))
        .unwrap();
    s.handle(
        "vr",
        g(json!({"phase": "move", "node": "filt", "param": "cutoff", "normalized": 1.0})),
    )
    .unwrap();
    assert!(
        s.handle("vr", g(json!({"phase": "move", "node": "filt", "param": "cutoff"})))
            .is_err()
    );
    assert_eq!(s.project().nodes["filt"].params["cutoff"], 20_000.0);
}
