//! Activity: every change from every client, newest first. Consecutive identical actions (a
//! gesture's stream of moves) collapse into one row. Click a row to see its paths.

use crate::app::{Studio, chip};
use crate::theme::client_color;
use eframe::egui::{self, RichText, Ui};

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let p = s.pal;
    ui.horizontal(|ui| {
        let name = ui.label(RichText::new("Filter").color(p.muted));
        ui.add(
            egui::TextEdit::singleline(&mut s.log_filter)
                .hint_text("client, label or path")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(name.id);
    });
    let filter = s.log_filter.to_lowercase();

    struct Row {
        seq: u64,
        secs: u64,
        client: String,
        label: String,
        count: usize,
        changes: Vec<(String, String)>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for e in &s.m.log {
        let matches = filter.is_empty()
            || e.client.to_lowercase().contains(&filter)
            || e.label.to_lowercase().contains(&filter)
            || e.changes.iter().any(|(path, _)| path.contains(&filter));
        if !matches {
            continue;
        }
        let changes = e
            .changes
            .iter()
            .map(|(path, v)| (path.clone(), v.to_string().chars().take(160).collect()))
            .collect();
        match rows.last_mut() {
            Some(last) if last.client == e.client && last.label == e.label => {
                last.count += 1;
                last.changes = changes;
                last.secs = e.at.elapsed().as_secs();
            }
            _ => rows.push(Row {
                seq: e.seq,
                secs: e.at.elapsed().as_secs(),
                client: e.client.clone(),
                label: e.label.clone(),
                count: 1,
                changes,
            }),
        }
    }

    for r in rows.iter().rev().take(200) {
        let resp = ui
            .horizontal(|ui| {
                ui.label(RichText::new(format!("{:>4}s", r.secs)).monospace().color(p.muted));
                chip(ui, &r.client, client_color(r.client.split('/').next().unwrap_or("")));
                ui.label(&r.label);
                if r.count > 1 {
                    ui.label(RichText::new(format!("×{}", r.count)).color(p.muted));
                }
                ui.label(
                    RichText::new(format!(
                        "{} change{}",
                        r.changes.len(),
                        if r.changes.len() == 1 { "" } else { "s" }
                    ))
                    .color(p.muted),
                );
            })
            .response
            .interact(egui::Sense::click());
        if resp.clicked() && !s.log_open.remove(&r.seq) {
            s.log_open.insert(r.seq);
        }
        if s.log_open.contains(&r.seq) {
            for (path, v) in &r.changes {
                ui.label(
                    RichText::new(format!("      {path} = {v}"))
                        .monospace()
                        .size(11.0)
                        .color(p.muted),
                );
            }
        }
    }
}
