//! egui bundles a small font without symbols like ▶ ■ ↶ ✕ →. Add whichever common system
//! fonts exist as fallbacks so those draw everywhere, and draw a box rather than fail if not.

use eframe::egui::{self, FontData, FontFamily};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily, MissingGlyphPolicy};

const FALLBACKS: &[&str] = &[
    // Arch and Fedora
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    // Debian and Ubuntu
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
];

pub fn install(ctx: &egui::Context) {
    ctx.set_missing_glyph_policy(MissingGlyphPolicy::Tofu);
    add_fallbacks(ctx);
}

/// Adds the fallback fonts that exist on this machine; returns how many.
pub fn add_fallbacks(ctx: &egui::Context) -> usize {
    let mut added = 0;
    for path in FALLBACKS {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let families = [FontFamily::Proportional, FontFamily::Monospace]
            .into_iter()
            .map(|family| InsertFontFamily {
                family,
                priority: FontPriority::Lowest,
            })
            .collect();
        ctx.add_font(FontInsert::new(path, FontData::from_owned(bytes), families));
        added += 1;
    }
    added
}
