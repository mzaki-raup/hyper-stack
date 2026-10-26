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

/// The glyph for a saved connection: its engine's own mark where one
/// exists, and the kind's otherwise.
///
/// `engine` is the `db` kinds' sub-type (`postgres`, `mysql`, …) and is
/// ignored for every other kind; pass `""` when there is none. Brands
/// come from the bundled Nerd Font's devicon set, which is where
/// PostgreSQL's elephant and MySQL's dolphin live. A store with no logo
/// anywhere - MinIO, Qdrant, WebDAV, Nexus - takes the nearest honest
/// generic rather than somebody else's brand.
///
/// One table, so the list in Settings, each activity's sidebar and the
/// tabs cannot disagree about what a connection looks like (owner's
/// requirement, 2026-10-26).
///
/// **Phosphor is installed before the Nerd Font and wins where the two
/// overlap**, and both live in the Private Use Area. A codepoint taken
/// from a Nerd Fonts cheat sheet may therefore draw something else
/// entirely - `U+E76E` is PostgreSQL there and a *safe* in Phosphor -
/// so every glyph below was checked on screen, and the shadowed ones
/// were replaced rather than shipped.
pub fn connection_icon(kind: &str, engine: &str) -> &'static str {
    match kind {
        "db" => match engine {
            // The devicon brands for these three sit at codepoints
            // Phosphor already uses (see the note above), so these are
            // the mascots from the Material range instead: the elephant
            // and the dolphin are what PostgreSQL and MySQL put on their
            // own pages, and SQLite is a database that is one file.
            "postgres" | "postgresql" => "\u{f07c6}",
            "mysql" | "mariadb" => "\u{f18b4}",
            "sqlite" => "\u{f0b1d}",
            _ => ph::DATABASE,
        },
        // nf-dev-redis, a real brand and not shadowed. Valkey is Redis'
        // fork and has no mark of its own.
        "redis" | "valkey" => "\u{e76d}",
        // MongoDB's own logo is a leaf; the devicon one is shadowed.
        "mongodb" => "\u{f032a}",
        // nf-dev-couchdb, a real brand and not shadowed.
        "couchdb" => "\u{e77c}",
        // No Qdrant logo anywhere in Nerd Fonts: a vector store is still
        // a database.
        "qdrant" => ph::DATABASE,
        "elasticsearch" => ph::MAGNIFYING_GLASS,
        "dynamodb" | "cassandra" => ph::DATABASE,
        // S3 and the other object stores keep the cloud and the folder:
        // MinIO, WebDAV and Nexus have no glyph to use.
        "s3" => ph::CLOUD,
        "webdav" => ph::FOLDER_SIMPLE,
        "nexus" => ph::PACKAGE,
        "ssh" => ph::COMPUTER_TOWER,
        "api" => ph::PAPER_PLANE_TILT,
        _ => container_kind_icon(kind),
    }
}

/// The glyph for a programming language, for the Editor's project list.
///
/// The brands come from the bundled Nerd Font's devicon and Material
/// ranges. **Phosphor is installed first and wins where the two overlap**
/// (see the note on [`connection_icon`]), and ten of the obvious choices
/// here were shadowed - Rust, Python, JavaScript, TypeScript, C#, C,
/// Kotlin, Lua, Node and Markdown all had to move to a second codepoint.
/// `scratchpad/langcheck.py` is what found that; run it before adding a
/// language, and then still look at the result, because being unshadowed
/// does not make a glyph the right picture: `U+EBCA` is listed as a bash
/// prompt and draws a **cube**, which is why the shell takes a Phosphor
/// terminal instead of its "own" mark.
///
/// An unknown or undetected language returns the folder, which is the
/// honest answer for "we did not work it out" and draws the same as the
/// other rows.
pub fn language_icon(id: &str) -> &'static str {
    match id {
        "rust" => "\u{e68b}",
        "java" => "\u{e738}",
        "kotlin" => "\u{e81b}",
        "scala" => "\u{e737}",
        "python" => "\u{e235}",
        "go" => "\u{e627}",
        "javascript" => "\u{e781}",
        "typescript" => "\u{e69d}",
        "node" => "\u{e76f}",
        "csharp" => "\u{f031b}",
        "cpp" => "\u{e61d}",
        "c" => "\u{f0671}",
        "php" => "\u{e73d}",
        "ruby" => "\u{e739}",
        "dart" => "\u{e798}",
        "swift" => "\u{e755}",
        "elixir" => "\u{e62d}",
        "haskell" => "\u{e777}",
        "perl" => "\u{e769}",
        "zig" => "\u{e6a9}",
        "lua" => "\u{e69b}",
        "css" => "\u{e749}",
        "markdown" => "\u{e609}",
        // No usable glyph: HTML's two candidates are both shadowed, and
        // a shell prompt draws a cube in this font.
        "shell" => ph::TERMINAL,
        "html" => ph::BRACKETS_ANGLE,
        "terraform" => ph::STACK,
        _ => ph::FOLDER,
    }
}

/// The glyph for a saved connection **profile**, reading whatever the
/// profile itself knows.
///
/// [`connection_icon`] answers from the kind alone, which is all most
/// kinds have. Two kinds know more about themselves and keep it on the
/// profile:
///
/// * `db` carries the engine, so PostgreSQL gets the elephant.
/// * `ssh` carries the operating system found by the probe on connect,
///   so a host wears its distribution's logo.
///
/// Every list that shows profiles goes through this, because the SSH
/// sidebar read `os_id` and Settings -> Connections did not: the same
/// host had two different icons depending on where you looked at it
/// (owner's report, 2026-10-26).
pub fn profile_icon(profile: &hs_core::connections::ConnectionProfile) -> &'static str {
    let field = |section: &str, key: &str| -> &str {
        profile
            .fields
            .get(section)
            .and_then(|v| v.as_table())
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
    };
    if profile.kind == "ssh" {
        let os = field("ssh", "os_id");
        if !os.is_empty() {
            return os_icon(os);
        }
    }
    connection_icon(&profile.kind, field("db", "engine"))
}
