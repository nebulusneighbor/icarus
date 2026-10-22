//! Automation: draw a parameter's lane over the loop. Click to add a point, drag to move it,
//! right-click to delete. Each edit writes the whole lane at /nodes/<id>/automation/<param>.

use crate::app::Studio;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use icarus_model::AutoPoint;
use serde_json::{Value, json};

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    let mut options: Vec<(String, bool)> = Vec::new();
    for (id, node) in &s.m.project.nodes {
        for spec in
            s.m.devices
                .get(&node.kind)
                .map(|d| d.params.as_slice())
                .unwrap_or_default()
        {
            options.push((format!("{id}.{}", spec.id), s.m.automated(id, &spec.id)));
        }
    }
    if !s.lane.as_ref().is_some_and(|l| options.iter().any(|(k, _)| k == l)) {
        let selected = s
            .sel
            .node
            .as_ref()
            .and_then(|n| options.iter().find(|(k, _)| k.starts_with(&format!("{n}."))));
        s.lane = options
            .iter()
            .find(|(_, a)| *a)
            .or(selected)
            .or(options.first())
            .map(|(k, _)| k.clone());
    }
    let Some(lane) = s.lane.clone() else {
        ui.label(RichText::new("No parameters yet. Add a track first.").color(p.muted));
        return;
    };
    let (node, param) = lane
        .split_once('.')
        .map(|(n, p)| (n.to_string(), p.to_string()))
        .unwrap_or_default();
    let Some(spec) = s.m.spec(&node, &param).cloned() else {
        return;
    };

    let mut points: Vec<AutoPoint> = s.m.project.nodes[&node]
        .automation
        .get(&param)
        .cloned()
        .unwrap_or_default();
    let path = format!("/nodes/{node}/automation/{param}");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("lane")
            .selected_text(&lane)
            .width(220.0)
            .show_ui(ui, |ui| {
                for (key, automated) in &options {
                    let text = if *automated { format!("● {key}") } else { key.clone() };
                    if ui.selectable_label(*key == lane, text).clicked() {
                        s.lane = Some(key.clone());
                    }
                }
            });
        let status = if points.is_empty() {
            format!("not automated · {}", spec.format(s.m.value(&node, &param)))
        } else {
            format!("{} points", points.len())
        };
        ui.label(RichText::new(status).color(p.muted));
        ui.label(RichText::new("click: add · drag: move · right-click: delete").color(p.muted));
        if ui.button("Clear lane").clicked() {
            s.link.set(&path, Value::Null);
        }
    });

    let length = s.m.project.length_s.max(0.001);
    let size = vec2(ui.available_width(), ui.available_height().max(120.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let inner = rect.shrink(10.0);
    let to_screen = |t: f64, v: f64| {
        pos2(
            inner.left() + (t / length) as f32 * inner.width(),
            inner.bottom() - spec.normalize(v) as f32 * inner.height(),
        )
    };
    let from_screen = |q: Pos2| AutoPoint {
        time_s: (((q.x - inner.left()) / inner.width()) as f64 * length).clamp(0.0, length),
        value: spec.denormalize((1.0 - (q.y - inner.top()) / inner.height()) as f64),
    };

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.bg);
    let beat = 60.0 / s.m.project.tempo_bpm;
    let mut t = 0.0;
    let mut i = 0;
    while t <= length {
        let x = to_screen(t, spec.min).x;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, if i % 4 == 0 { p.panel3 } else { p.panel2 }),
        );
        t += beat;
        i += 1;
    }

    let hit = |q: Pos2, pts: &[AutoPoint]| {
        pts.iter()
            .position(|pt| to_screen(pt.time_s, pt.value).distance(q) < 8.0)
    };
    if resp.drag_started()
        && let Some(q) = resp.interact_pointer_pos()
    {
        s.lane_drag = Some(hit(q, &points).unwrap_or_else(|| {
            points.push(from_screen(q));
            points.len() - 1
        }));
    }
    if let (Some(i), Some(q)) = (s.lane_drag, resp.interact_pointer_pos())
        && resp.dragged()
        && i < points.len()
    {
        points[i] = from_screen(q);
        // Shown immediately; written to the session on release.
        if let Some(n) = s.m.project.nodes.get_mut(&node) {
            n.automation.insert(param.clone(), points.clone());
        }
    }
    let mut write = None;
    if resp.drag_stopped() && s.lane_drag.take().is_some() {
        write = Some(points.clone());
    } else if resp.clicked()
        && let Some(q) = resp.interact_pointer_pos()
        && hit(q, &points).is_none()
    {
        points.push(from_screen(q));
        write = Some(points.clone());
    } else if resp.secondary_clicked()
        && let Some(i) = resp.interact_pointer_pos().and_then(|q| hit(q, &points))
    {
        points.remove(i);
        write = Some(points.clone());
    }

    let line = if points.is_empty() {
        let v = s.m.value(&node, &param);
        vec![to_screen(0.0, v), to_screen(length, v)]
    } else {
        let mut sorted = points.clone();
        sorted.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        let mut l = vec![to_screen(0.0, sorted[0].value)];
        l.extend(sorted.iter().map(|pt| to_screen(pt.time_s, pt.value)));
        l.push(to_screen(length, sorted.last().expect("non-empty").value));
        l
    };
    if points.is_empty() {
        painter.add(Shape::dashed_line(&line, Stroke::new(2.0, p.accent), 6.0, 4.0));
    } else {
        painter.add(Shape::line(line, Stroke::new(2.0, p.accent)));
        for pt in &points {
            painter.circle_filled(to_screen(pt.time_s, pt.value), 4.0, p.accent);
        }
    }
    if s.m.transport.playing {
        let x = to_screen(s.m.transport.position_s % length, spec.min).x;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, p.accent2),
        );
    }
    painter.text(
        Rect::from_min_max(rect.min, rect.max).right_top() + vec2(-8.0, 6.0),
        Align2::RIGHT_TOP,
        spec.format(spec.max),
        FontId::monospace(10.0),
        p.muted,
    );

    if let Some(mut pts) = write {
        pts.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        s.link.set(&path, json!(pts));
    }
}
