//! The native Icarus app.
//!
//! It is a protocol client like any other: it never touches the engine, only sends requests and
//! applies change events. Its layout, panels, tracks, theme and key bindings come from the
//! session's /ui document, the same one the browser GUI reads, so an agent rearranging /ui
//! rearranges both. Agents connect to the session, not to this window, so nothing about agent
//! access depends on which GUI is open.

mod app;
mod fonts;
mod layout;
mod link;
mod mirror;
mod panels;
mod theme;
mod tracks;
mod widgets;

use eframe::egui;
use icarus_client::{Connection, Events};

pub struct Options {
    /// Name this app's changes are attributed to, e.g. `app:human`.
    pub client_name: String,
    /// Address of the browser GUI for the same session, if one is being served.
    pub web_url: Option<String>,
}

/// Opens the window and runs until it is closed.
pub fn run(connection: Box<dyn Connection>, events: Events, options: Options) -> anyhow::Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Icarus")
            .with_app_id("icarus")
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Icarus",
        native,
        Box::new(move |cc| {
            fonts::install(&cc.egui_ctx);
            let link = link::Link::new(connection, events, options.client_name, cc.egui_ctx.clone());
            let studio = app::Studio::new(link, options.web_url)?;
            Ok(Box::new(app::IcarusApp { studio }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod tests;
