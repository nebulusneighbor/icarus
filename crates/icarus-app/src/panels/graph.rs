//! The raw signal graph: every node and connection, whether or not a track claims it.
//! Drag from a node's right port to another node's left port to connect; click a cable to
//! disconnect; drag a node to move it. Positions are shared with the browser GUI at
//! /ui/graph/positions.

use crate::app::Studio;
use crate::theme::{client_color, track_color};
use crate::tracks::track_of;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use icarus_model::Command;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const W: f32 = 150.0;
const H: f32 = 54.0;

pub enum GraphDrag {
    Node { id: String, grab: egui::Vec2 },
    Cable { from: String },
}

fn auto_layout(s: &Studio) -> BTreeMap<String, Pos2> {
    let nodes: Vec<&String> = s.m.project.nodes.keys().collect();
    let mut inputs: BTreeMap<&str, Vec<&str>> = nodes.iter().map(|id| (id.as_str(), Vec::new())).collect();
    for c in &s.m.project.connections {
        inputs.entry(c.to.as_str()).or_default().push(c.from.as_str());
    }
    fn depth<'a>(
        id: &'a str,
        inputs: &BTreeMap<&'a str, Vec<&'a str>>,
        memo: &mut BTreeMap<&'a str, usize>,
        seen: &mut BTreeSet<&'a str>,
    ) -> usize {
        if let Some(d) = memo.get(id) {
            return *d;
        }
        if !seen.insert(id) {
            return 0;
        }
        let d = inputs
            .get(id)
            .map(|ins| ins.iter().map(|i| depth(i, inputs, memo, seen) + 1).max().unwrap_or(0))
            .unwrap_or(0);
        memo.insert(id, d);
        d
    }
    let mut memo = BTreeMap::new();
    for id in &nodes {
        depth(id, &inputs, &mut memo, &mut BTreeSet::new());
    }
    let max = memo.values().copied().max().unwrap_or(0);
    if inputs.get("master").is_some_and(|i| i.is_empty()) {
        memo.insert("master", max + 1);
    }
    let mut rows: BTreeMap<usize, usize> = BTreeMap::new();
    nodes
        .iter()
        .map(|id| {
            let d = memo[id.as_str()];
            let row = rows.entry(d).or_default();
            *row += 1;
            (
                id.to_string(),
                pos2(24.0 + d as f32 * (W + 60.0), 20.0 + (*row - 1) as f32 * (H + 30.0)),
            )
        })
        .collect()
}

fn bezier(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() / 2.0).max(40.0);
    let (c1, c2) = (pos2(a.x + dx, a.y), pos2(b.x - dx, b.y));
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let p = u * u * u * a.to_vec2()
                + 3.0 * u * u * t * c1.to_vec2()
                + 3.0 * u * t * t * c2.to_vec2()
                + t * t * t * b.to_vec2();
            p.to_pos2()
        })
        .collect()
}

fn near_curve(points: &[Pos2], p: Pos2) -> bool {
    points.windows(2).any(|w| {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        (a + ab * t).distance(p) < 6.0
    })
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(
                "Drag ● → ● to connect · click a cable to disconnect · Delete removes the selected node",
            )
            .color(p.muted),
        );
        if ui.button("Auto layout").clicked() {
            s.link.set("/ui/graph/positions", json!({}));
        }
    });

    let auto = auto_layout(s);
    let saved = &s.m.project.ui["graph"]["positions"];
    let mut pos: BTreeMap<String, Pos2> = auto
        .into_iter()
        .map(|(id, a)| {
            let p = saved[&id]
                .as_array()
                .and_then(|xy| Some(pos2(xy.first()?.as_f64()? as f32, xy.get(1)?.as_f64()? as f32)));
            (id, p.unwrap_or(a))
        })
        .collect();

    let extent = pos.values().fold(vec2(600.0, 300.0), |acc, p| {
        acc.max(vec2(p.x + W + 60.0, p.y + H + 40.0))
    });
    let size = extent.max(ui.available_size());
    let (canvas, bg) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let origin = canvas.min.to_vec2();
    let pointer = ui.input(|i| i.pointer.interact_pos());

    // Moving a node follows the pointer locally and saves on release.
    if let (Some(GraphDrag::Node { id, grab }), Some(ptr)) = (&s.graph_drag, pointer)
        && ui.input(|i| i.pointer.primary_down())
    {
        pos.insert(id.clone(), (ptr - *grab - origin).max(Pos2::ZERO));
    }

    let painter = ui.painter_at(canvas);
    let out_port = |id: &str, pos: &BTreeMap<String, Pos2>| pos[id] + origin + vec2(W, H / 2.0);
    let in_port = |id: &str, pos: &BTreeMap<String, Pos2>| pos[id] + origin + vec2(0.0, H / 2.0);
    let tracks = s.m.tracks();

    let mut disconnect = None;
    for c in &s.m.project.connections {
        if !pos.contains_key(&c.from) || !pos.contains_key(&c.to) {
            continue;
        }
        let curve = bezier(out_port(&c.from, &pos), in_port(&c.to, &pos));
        let hovered = pointer.is_some_and(|ptr| near_curve(&curve, ptr)) && s.graph_drag.is_none();
        let color = track_of(&s.m, &c.from)
            .map(|t| track_color(&tracks[t].color, p.muted))
            .unwrap_or(p.muted);
        painter.add(Shape::line(
            curve,
            Stroke::new(if hovered { 4.0 } else { 2.5 }, if hovered { p.danger } else { color }),
        ));
        if hovered && bg.clicked() {
            disconnect = Some((c.from.clone(), c.to.clone()));
        }
    }

    let mut select = None;
    for (id, node) in &s.m.project.nodes {
        let rect = Rect::from_min_size(pos[id] + origin, vec2(W, H));
        let resp = ui.interact(rect, ui.make_persistent_id(("node", id)), Sense::click_and_drag());
        let t = track_of(&s.m, id);
        let color = t.map(|t| track_color(&tracks[t].color, p.border)).unwrap_or(p.border);
        let holders: Vec<&String> =
            s.m.touches
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("{id}/")))
                .map(|(_, v)| v)
                .collect();
        painter.rect_filled(rect, 4.0, p.panel2);
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(4.0, H)), 2.0, color);
        let outline = if s.sel.node.as_deref() == Some(id) {
            p.accent
        } else {
            p.border
        };
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, outline), StrokeKind::Inside);
        if let Some(who) = holders.first() {
            painter.rect_stroke(
                rect.expand(2.0),
                5.0,
                Stroke::new(2.0, client_color(who)),
                StrokeKind::Outside,
            );
        }
        painter.text(
            rect.min + vec2(12.0, 8.0),
            Align2::LEFT_TOP,
            id,
            FontId::monospace(12.0),
            p.text,
        );
        let sub = match t {
            Some(t) => format!("{} · {}", node.kind, tracks[t].name),
            None => node.kind.clone(),
        };
        painter.text(
            rect.min + vec2(12.0, 28.0),
            Align2::LEFT_TOP,
            sub,
            FontId::proportional(11.0),
            p.muted,
        );

        let generator = matches!(node.kind.as_str(), "osc" | "noise");
        if id != "master" {
            let port = out_port(id, &pos);
            painter.circle(port, 6.0, p.bg, Stroke::new(2.0, p.muted));
            let r = ui.interact(
                Rect::from_center_size(port, vec2(16.0, 16.0)),
                ui.make_persistent_id(("out", id)),
                Sense::drag(),
            );
            if r.drag_started() {
                s.graph_drag = Some(GraphDrag::Cable { from: id.clone() });
            }
        }
        if !generator {
            painter.circle(in_port(id, &pos), 6.0, p.bg, Stroke::new(2.0, p.muted));
        }

        if resp.drag_started()
            && let Some(ptr) = pointer
        {
            s.graph_drag = Some(GraphDrag::Node {
                id: id.clone(),
                grab: ptr - (pos[id] + origin),
            });
        }
        if resp.clicked() {
            select = Some((t, id.clone()));
        }
    }

    if let (Some(GraphDrag::Cable { from }), Some(ptr)) = (&s.graph_drag, pointer) {
        painter.add(Shape::dashed_line(
            &bezier(out_port(from, &pos), ptr),
            Stroke::new(2.0, p.accent),
            6.0,
            4.0,
        ));
    }

    // Finish drags on release.
    if s.graph_drag.is_some() && ui.input(|i| i.pointer.any_released()) {
        match s.graph_drag.take() {
            Some(GraphDrag::Node { id, .. }) => {
                let at = pos[&id];
                s.link.set(
                    &format!("/ui/graph/positions/{id}"),
                    json!([at.x.round(), at.y.round()]),
                );
            }
            Some(GraphDrag::Cable { from }) => {
                let target = pointer.and_then(|ptr| {
                    s.m.project
                        .nodes
                        .keys()
                        .find(|id| {
                            Rect::from_min_size(pos[*id] + origin, vec2(W, H))
                                .expand(8.0)
                                .contains(ptr)
                        })
                        .cloned()
                });
                if let Some(to) = target.filter(|to| *to != from) {
                    s.link
                        .tx(format!("connect {from} → {to}"), vec![Command::Connect { from, to }]);
                }
            }
            None => {}
        }
    }
    if s.graph_drag.is_some() {
        ui.ctx().request_repaint();
    }

    if let Some((from, to)) = disconnect {
        s.link.tx(
            format!("disconnect {from} → {to}"),
            vec![Command::Disconnect { from, to }],
        );
    }
    if let Some((track, id)) = select {
        s.select(track, Some(id));
    }
}
