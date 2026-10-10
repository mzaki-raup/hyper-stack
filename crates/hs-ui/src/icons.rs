//! Phosphor icon font installation and the glyphs the shell uses.

pub use egui_phosphor::regular as ph;

/// Install the UI fonts: egui defaults plus Phosphor regular and fill.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Fill);
    ctx.set_fonts(fonts);
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
