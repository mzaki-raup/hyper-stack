//! Phosphor icon font installation and the glyphs the shell uses.

pub use egui_phosphor::regular as ph;

/// Install the UI fonts with the default choice (egui defaults, Phosphor
/// icons, bundled monospace fonts). The app calls `fonts::install` with the
/// user's choice instead.
pub fn install_fonts(ctx: &egui::Context) {
    crate::fonts::install(ctx, &crate::fonts::FontChoice::default());
}

/// Install egui's image loaders (PNG and SVG) so `egui::Image` and the tree's
/// Material icons render. Call once at startup, next to `install_fonts`.
pub fn install_image_loaders(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

pub const TAB_CLOSE: &str = ph::X;
pub const TAB_NEW: &str = ph::PLUS;
pub const PIN: &str = ph::PUSH_PIN;
pub const SETTINGS: &str = ph::GEAR;
pub const BELL: &str = ph::BELL;
pub const SIDEBAR: &str = ph::SIDEBAR;
pub const DOCK: &str = ph::ARROWS_IN_LINE_VERTICAL;
pub const SEARCH: &str = ph::MAGNIFYING_GLASS;
pub const LOCK: &str = ph::LOCK;
pub const UNLOCK: &str = ph::LOCK_OPEN;
pub const WORKSPACE: &str = ph::FOLDERS;
pub const ENVIRONMENT: &str = ph::GLOBE_HEMISPHERE_WEST;
pub const JOBS: &str = ph::SPINNER_GAP;
pub const CHECK: &str = ph::CHECK;
pub const WARNING: &str = ph::WARNING;
pub const ERROR: &str = ph::X_CIRCLE;
pub const INFO: &str = ph::INFO;
pub const NOTE: &str = ph::NOTE_PENCIL;
pub const HAND_WAVE: &str = ph::HAND_WAVING;
pub const CARET_RIGHT: &str = ph::CARET_RIGHT;
pub const CARET_DOWN: &str = ph::CARET_DOWN;
pub const TRASH: &str = ph::TRASH;
pub const COPY: &str = ph::COPY;
pub const EDIT: &str = ph::PENCIL_SIMPLE;
pub const ARROW_UP: &str = ph::ARROW_CIRCLE_UP;
pub const EYE: &str = ph::EYE;
pub const EYE_SLASH: &str = ph::EYE_SLASH;

/// Paint a glyph with a contrasting outline around it, and return the
/// rectangle it covered.
///
/// epaint has no stroke for text - `TextShape` carries an `underline`
/// and nothing else - so the ring is the glyph itself, drawn eight
/// times at small offsets under the real one. `width` is that offset in
/// points: about 1 reads as a hairline, 1.5 to 2 as a drawn edge. The
/// diagonals are pulled in by √½ so the ring is round rather than
/// square.
///
/// Nine text shapes per icon is nothing for a rail of sixteen and would
/// be worth thinking about for a tree of hundreds, which is why this is
/// a helper the caller opts into rather than something `painter.text`
/// does everywhere.
#[allow(clippy::too_many_arguments)]
pub fn outlined_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    anchor: egui::Align2,
    text: impl ToString,
    font: egui::FontId,
    fill: egui::Color32,
    outline: egui::Color32,
    width: f32,
) -> egui::Rect {
    let text = text.to_string();
    if width > 0.0 && outline.a() > 0 {
        let d = width * std::f32::consts::FRAC_1_SQRT_2;
        for (dx, dy) in [
            (-width, 0.0),
            (width, 0.0),
            (0.0, -width),
            (0.0, width),
            (-d, -d),
            (d, -d),
            (-d, d),
            (d, d),
        ] {
            painter.text(
                pos + egui::vec2(dx, dy),
                anchor,
                &text,
                font.clone(),
                outline,
            );
        }
    }
    painter.text(pos, anchor, &text, font, fill)
}
