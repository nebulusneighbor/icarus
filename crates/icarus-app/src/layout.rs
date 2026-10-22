//! Lays out /ui/layout the way CSS grid would: `columns` and `rows` are track lists such as
//! `"210px 1fr 320px"`, and `areas` names each cell. An area covers every cell with its name.

use eframe::egui::{Pos2, Rect, pos2};
use serde_json::Value;

pub const GAP: f32 = 1.0;

enum Size {
    Px(f32),
    Fr(f32),
    Pct(f32),
}

fn parse(token: &str) -> Size {
    let t = token.trim();
    if let Some(n) = t.strip_suffix("px").and_then(|n| n.parse().ok()) {
        Size::Px(n)
    } else if let Some(n) = t.strip_suffix("fr").and_then(|n| n.parse().ok()) {
        Size::Fr(n)
    } else if let Some(n) = t.strip_suffix('%').and_then(|n| n.parse().ok()) {
        Size::Pct(n)
    } else if let Ok(n) = t.parse::<f32>() {
        Size::Px(n)
    } else {
        // `auto`, `minmax(...)` and anything else share leftover space.
        Size::Fr(1.0)
    }
}

/// Start offsets and sizes of each track along one axis.
fn tracks(spec: &str, count: usize, total: f32) -> Vec<(f32, f32)> {
    let mut sizes: Vec<Size> = spec.split_whitespace().map(parse).collect();
    sizes.resize_with(count.max(sizes.len()), || Size::Fr(1.0));
    let gaps = GAP * sizes.len().saturating_sub(1) as f32;
    let fixed: f32 = sizes
        .iter()
        .map(|s| match s {
            Size::Px(p) => *p,
            Size::Pct(p) => total * p / 100.0,
            Size::Fr(_) => 0.0,
        })
        .sum();
    let fr_total: f32 = sizes.iter().map(|s| if let Size::Fr(f) = s { *f } else { 0.0 }).sum();
    let per_fr = if fr_total > 0.0 {
        ((total - gaps - fixed) / fr_total).max(0.0)
    } else {
        0.0
    };
    let mut at = 0.0;
    sizes
        .iter()
        .map(|s| {
            let len = match s {
                Size::Px(p) => *p,
                Size::Pct(p) => total * p / 100.0,
                Size::Fr(f) => f * per_fr,
            };
            let out = (at, len);
            at += len + GAP;
            out
        })
        .collect()
}

/// Each named area and its rectangle, in the order areas first appear.
pub fn areas(layout: &Value, rect: Rect) -> Vec<(String, Rect)> {
    let grid: Vec<Vec<String>> =
        serde_json::from_value(layout["areas"].clone()).unwrap_or_else(|_| vec![vec!["main".into()]]);
    let rows = grid.len();
    let cols = grid.iter().map(Vec::len).max().unwrap_or(1);
    let col = tracks(layout["columns"].as_str().unwrap_or("1fr"), cols, rect.width());
    let row = tracks(layout["rows"].as_str().unwrap_or("1fr"), rows, rect.height());

    let mut out: Vec<(String, Rect)> = Vec::new();
    for (r, line) in grid.iter().enumerate() {
        for (c, name) in line.iter().enumerate() {
            if name == "." {
                continue;
            }
            let min = Pos2::new(rect.left() + col[c].0, rect.top() + row[r].0);
            let cell = Rect::from_min_max(min, pos2(min.x + col[c].1, min.y + row[r].1));
            match out.iter_mut().find(|(n, _)| n == name) {
                Some((_, area)) => *area = area.union(cell),
                None => out.push((name.clone(), cell)),
            }
        }
    }
    out.retain(|(_, r)| r.width() > 0.5 && r.height() > 0.5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grid_matches_css() {
        let layout = json!({
            "columns": "200px 1fr 100px",
            "rows": "50px 1fr",
            "areas": [["top", "top", "top"], ["left", "main", "right"]]
        });
        let a = areas(
            &layout,
            Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(1002.0, 551.0)),
        );
        let get = |n: &str| a.iter().find(|(name, _)| name == n).unwrap().1;
        assert_eq!(get("top"), Rect::from_min_max(pos2(0.0, 0.0), pos2(1002.0, 50.0)));
        assert_eq!(get("left").width(), 200.0);
        assert_eq!(get("main").width(), 700.0);
        assert_eq!(get("right").left(), 902.0);
        assert_eq!(get("main").height(), 500.0);
    }
}
