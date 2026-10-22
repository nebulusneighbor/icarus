//! Native panels. Each mirrors the browser GUI panel of the same type and reads the same
//! /ui document, so a layout written by an agent works in both.

pub mod automation;
pub mod browser;
pub mod controls;
pub mod devices;
pub mod graph;
pub mod inspector;
pub mod log;
pub mod mixer;
pub mod transport;
pub mod web;

use eframe::egui::{RichText, Ui};

pub fn section(ui: &mut Ui, title: &str, muted: eframe::egui::Color32) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).size(10.0).color(muted));
}
