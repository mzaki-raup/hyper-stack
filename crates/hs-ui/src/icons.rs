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

/// The logo for an operating system id (`hs_ssh::os::HostOs::id`), from
/// the bundled Nerd Font's Linux set - `fonts::install` adds it as the
/// last fallback, so these render wherever the interface font has no
/// glyph of its own. An id nothing recognises keeps the generic host
/// glyph rather than drawing a box.
pub fn os_icon(id: &str) -> &'static str {
    match id {
        "ubuntu" => "\u{f31b}",
        "debian" => "\u{f306}",
        "fedora" => "\u{f30a}",
        "rhel" => "\u{f316}",
        "centos" => "\u{f304}",
        "alpine" => "\u{f300}",
        "arch" => "\u{f303}",
        "suse" => "\u{f314}",
        "linux" => "\u{f17c}",
        "macos" => "\u{f302}",
        "windows" => "\u{f17a}",
        "freebsd" => "\u{f30c}",
        _ => ph::COMPUTER_TOWER,
    }
}

/// The glyph for a container engine or cluster kind.
///
/// Brands where a brand exists, from the bundled Nerd Font: Docker's
/// whale and Kubernetes' helm. Podman has no logo in Nerd Fonts at all,
/// so it takes a neutral package rather than a borrowed one, and
/// Windows containers take the Windows logo. The **activity's** own
/// icon stays the shipping container - that is the category, not an
/// engine (owner's requirement, 2026-10-26).
///
/// One table, used by the activity and by Settings alike: two copies of
/// this drifted apart once already.
pub fn container_kind_icon(kind: &str) -> &'static str {
    match kind {
        // nf-md-docker: the filled whale. The devicon one (U+E7B0) is
        // drawn too fine to read at 14 px.
        "docker" => "\u{f0868}",
        // nf-dev-kubernetes: the helm. The Material one (U+F10FE) comes
        // out as an indistinct dot at this size.
        "k8s" => "\u{e81d}",
        // nf-cod-package: no podman logo exists to use.
        "podman" => "\u{eb29}",
        "wincontainer" => ph::WINDOWS_LOGO,
        _ => ph::SHIPPING_CONTAINER,
    }
}
