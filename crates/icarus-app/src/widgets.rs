//! Controls that move parameters like a hand does: press to touch (gesture begin), drag to move,
//! release to let go. A drag is one undo step, and every client sees the touch while it lasts.

use crate::app::{Drag, Studio};
use crate::theme::client_color;
use eframe::egui::{
    self, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use icarus_model::{Command, Gesture};
use std::f32::consts::PI;

const ARC_START: f32 = 0.75 * PI;
const ARC_SWEEP: f32 = 1.5 * PI;

fn arc(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = ((to - from).abs() / 0.08).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            center + radius * Vec2::angled(a)
        })
        .collect()
}

/// Turns press/drag/release on `resp` into a gesture on `node.param`.
fn drive(s: &mut Studio, ui: &Ui, resp: &Response, node: &str, param: &str, pixels: f32, vertical: bool) {
    let key = format!("{node}/{param}");
    let Some(spec) = s.m.spec(node, param).cloned() else {
        return;
    };

    if resp.drag_started() {
        let begin = Gesture::Begin {
            node: node.into(),
            param: param.into(),
            record: false,
        };
        match s.link.gesture(begin) {
            Ok(_) => {
                s.drag = Some(Drag {
                    key: key.clone(),
                    start: spec.normalize(s.m.value(node, param)),
                    travel: 0.0,
                });
            }
            Err(e) => s.link.toast(e),
        }
    }
    if resp.dragged() {
        let delta = if vertical {
            -resp.drag_delta().y
        } else {
            resp.drag_delta().x
        };
        let fine = if ui.input(|i| i.modifiers.shift) { 6.0 } else { 1.0 };
        let moved = match &mut s.drag {
            Some(d) if d.key == key && delta != 0.0 => {
                d.travel += delta / fine;
                Some((d.start + f64::from(d.travel / pixels)).clamp(0.0, 1.0))
            }
            _ => None,
        };
        if let Some(n) = moved {
            // Optimistic: show the move now; the session's event confirms it.
            s.m.set_value(node, param, spec.denormalize(n));
            let gesture = Gesture::Move {
                node: node.into(),
                param: param.into(),
                value: None,
                normalized: Some(n),
                at_s: None,
            };
            if let Err(e) = s.link.gesture(gesture) {
                s.link.toast(e);
            }
        }
    }
    if resp.drag_stopped() && s.drag.as_ref().is_some_and(|d| d.key == key) {
        s.drag = None;
        let _ = s.link.gesture(Gesture::End {
            node: node.into(),
            param: param.into(),
        });
    }
    if resp.double_clicked() {
        s.link.tx(
            format!("reset {node}.{param}"),
            vec![Command::SetParam {
                node: node.into(),
                param: param.into(),
                value: spec.default,
            }],
        );
    }
}

/// Exposes the control to assistive technology (and tests) as a slider named "node param".
fn describe(resp: &Response, s: &Studio, node: &str, param: &str) {
    let value = s.m.value(node, param);
    let name =
        s.m.spec(node, param)
            .map(|sp| sp.name.clone())
            .unwrap_or_else(|| param.to_string());
    let label = format!("{node} {name}");
    resp.widget_info(|| egui::WidgetInfo::slider(true, value, &label));
}

fn hover_text(s: &Studio, node: &str, param: &str) -> String {
    let description =
        s.m.spec(node, param)
            .map(|sp| sp.description.clone())
            .unwrap_or_default();
    match s.m.held_by(node, param) {
        Some(who) if who != s.link.name => format!("{who} is holding this\n{description}"),
        _ => format!("{node}.{param}: {description}\nDrag to change, shift for fine, double-click to reset"),
    }
}

/// Outline in the colour of whoever else holds the control.
fn holder_ring(s: &Studio, ui: &Ui, rect: Rect, node: &str, param: &str) {
    if let Some(who) = s.m.held_by(node, param).filter(|w| *w != s.link.name) {
        ui.painter()
            .rect_stroke(rect, 4.0, Stroke::new(2.0, client_color(who)), StrokeKind::Inside);
    }
}

pub fn knob(ui: &mut Ui, s: &mut Studio, node: &str, param: &str, label: Option<&str>) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(58.0, 70.0), Sense::hover());
    let id = ui.make_persistent_id(("knob", node, param));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    drive(s, ui, &resp, node, param, 180.0, true);
    describe(&resp, s, node, param);

    let p = s.pal;
    let painter = ui.painter_at(rect);
    let Some(spec) = s.m.spec(node, param) else { return resp };
    let value = s.m.value(node, param);
    let n = spec.normalize(value) as f32;
    if resp.dragged() || resp.hovered() {
        painter.rect_filled(rect, 4.0, p.panel3);
    }
    painter.text(
        pos2(rect.center().x, rect.top() + 7.0),
        egui::Align2::CENTER_CENTER,
        label.unwrap_or(&spec.name),
        FontId::proportional(10.0),
        p.muted,
    );
    let center = pos2(rect.center().x, rect.top() + 32.0);
    painter.add(Shape::line(
        arc(center, 15.0, ARC_START, ARC_START + ARC_SWEEP),
        Stroke::new(4.0, p.panel3),
    ));
    if n > 0.002 {
        let fill = if s.m.automated(node, param) {
            p.accent2
        } else {
            p.accent
        };
        painter.add(Shape::line(
            arc(center, 15.0, ARC_START, ARC_START + ARC_SWEEP * n),
            Stroke::new(4.0, fill),
        ));
    }
    painter.circle(center, 9.0, p.panel, Stroke::new(1.0, p.border));
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        egui::Align2::CENTER_CENTER,
        spec.format(value),
        FontId::monospace(10.0),
        p.text,
    );
    holder_ring(s, ui, rect, node, param);
    resp.on_hover_text(hover_text(s, node, param))
}

pub fn fader(ui: &mut Ui, s: &mut Studio, node: &str, param: &str, height: f32, color: Color32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, height.max(60.0)), Sense::hover());
    let id = ui.make_persistent_id(("fader", node, param));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    drive(s, ui, &resp, node, param, (rect.height() - 24.0).max(40.0), true);
    describe(&resp, s, node, param);

    let p = s.pal;
    let painter = ui.painter_at(rect);
    let Some(spec) = s.m.spec(node, param) else { return resp };
    let value = s.m.value(node, param);
    let n = spec.normalize(value) as f32;
    let slot = Rect::from_min_max(
        pos2(rect.center().x - 4.0, rect.top() + 6.0),
        pos2(rect.center().x + 4.0, rect.bottom() - 20.0),
    );
    painter.rect_filled(slot, 4.0, p.bg);
    let level = slot.bottom() - slot.height() * n;
    painter.rect_filled(
        Rect::from_min_max(pos2(slot.left(), level), slot.max),
        4.0,
        color.linear_multiply(0.85),
    );
    painter.rect_filled(
        Rect::from_center_size(pos2(slot.center().x, level), vec2(26.0, 8.0)),
        2.0,
        p.text,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        egui::Align2::CENTER_CENTER,
        spec.format(value),
        FontId::monospace(10.0),
        p.text,
    );
    holder_ring(s, ui, rect, node, param);
    resp.on_hover_text(hover_text(s, node, param))
}

/// Two parameters on one pad. Each axis is "node.param"; either may be empty.
pub fn xy(ui: &mut Ui, s: &mut Studio, x: &str, y: &str, size: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let id = ui.make_persistent_id(("xy", x, y));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let axes: Vec<(String, String)> = [x, y]
        .iter()
        .map(|a| {
            a.split_once('.')
                .map(|(n, p)| (n.to_string(), p.to_string()))
                .unwrap_or_default()
        })
        .collect();

    if resp.drag_started() {
        for (node, param) in axes.iter().filter(|(n, _)| !n.is_empty()) {
            let begin = Gesture::Begin {
                node: node.clone(),
                param: param.clone(),
                record: false,
            };
            if let Err(e) = s.link.gesture(begin) {
                s.link.toast(e);
            }
        }
    }
    if (resp.dragged() || resp.drag_started())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        let ny = (1.0 - (pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0) as f64;
        for ((node, param), n) in axes.iter().zip([nx, ny]) {
            if node.is_empty() {
                continue;
            }
            if let Some(spec) = s.m.spec(node, param).cloned() {
                s.m.set_value(node, param, spec.denormalize(n));
                let gesture = Gesture::Move {
                    node: node.clone(),
                    param: param.clone(),
                    value: None,
                    normalized: Some(n),
                    at_s: None,
                };
                let _ = s.link.gesture(gesture);
            }
        }
    }
    if resp.drag_stopped() {
        for (node, param) in axes.iter().filter(|(n, _)| !n.is_empty()) {
            let _ = s.link.gesture(Gesture::End {
                node: node.clone(),
                param: param.clone(),
            });
        }
    }

    let p = s.pal;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.panel2);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.line_segment(
            [
                pos2(rect.left() + rect.width() * t, rect.top()),
                pos2(rect.left() + rect.width() * t, rect.bottom()),
            ],
            Stroke::new(1.0, p.border),
        );
        painter.line_segment(
            [
                pos2(rect.left(), rect.top() + rect.height() * t),
                pos2(rect.right(), rect.top() + rect.height() * t),
            ],
            Stroke::new(1.0, p.border),
        );
    }
    let norm = |(node, param): &(String, String)| {
        s.m.spec(node, param)
            .map(|sp| sp.normalize(s.m.value(node, param)) as f32)
            .unwrap_or(0.5)
    };
    let dot = pos2(
        rect.left() + rect.width() * norm(&axes[0]),
        rect.bottom() - rect.height() * norm(&axes[1]),
    );
    painter.circle_filled(dot, 9.0, p.accent.linear_multiply(0.3));
    painter.circle_filled(dot, 6.0, p.accent);
    painter.text(
        rect.left_bottom() + vec2(6.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        format!("x {x}  y {y}"),
        FontId::proportional(10.0),
        p.muted,
    );
    resp
}
