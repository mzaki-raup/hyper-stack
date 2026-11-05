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
    // The product logos are drawn by URI from every list, tab and header,
    // so their bytes are registered once here rather than carried around.
    for (uri, bytes) in PRODUCT_LOGOS {
        ctx.include_bytes(*uri, *bytes);
    }
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
            // ADR-030's engines (CR-0024): dev-clickhouse, dev-duckdb;
            // dev-microsoftsqlserver (U+E82E) is under Phosphor, so SQL
            // Server takes dev-azuresqldatabase, the same family's mark.
            // dev-oracle (U+E859) is the ORACLE wordmark, unreadable at
            // icon size, so Oracle keeps the database glyph, as Db2 does
            // (it has no glyph anywhere).
            "clickhouse" => "\u{e8fe}",
            "duckdb" => "\u{e908}",
            "mssql" | "sqlserver" => "\u{e75b}",
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
        // The brokers. Redis Streams is still a Redis server, so it keeps
        // Redis' mark.
        "redis-streams" => "\u{e76d}",
        // Product marks where the bundled Nerd Font has one that Phosphor
        // does not shadow (CR-0024): md-rabbit (the devicon RabbitMQ at
        // U+E882 is under Phosphor's EXCLUDE), md-apache_kafka and
        // dev-nats. MQTT has no mark anywhere and keeps the broadcast.
        "rabbitmq" => "\u{f0907}",
        "kafka" => "\u{f100f}",
        "nats" => "\u{e8f0}",
        "mqtt" => ph::BROADCAST,
        // The build servers (CR-0024): dev-jenkins, dev-githubactions,
        // dev-gitlab, dev-forgejo, md-ansible. TeamCity has no mark that
        // shows (dev-jetbrains sits under Phosphor's BATTERY_PLUS).
        "jenkins" => "\u{e767}",
        "teamcity" => ph::GEAR_SIX,
        "github-actions" => "\u{e7e9}",
        "gitlab-ci" => "\u{e7eb}",
        "forgejo-actions" => "\u{e910}",
        "awx" => "\u{f109a}",
        // The Puppet mark comes from the logo; the glyph is the fallback.
        "puppet" => ph::PUZZLE_PIECE,
        // The trackers (CR-0024): md-jira (the devicon Jira at U+E75C
        // is under Phosphor), dev-confluence, the GitHub mark,
        // dev-gitlab, dev-forgejo. Redmine has no mark anywhere and takes
        // the bug.
        "jira" => "\u{f0303}",
        "confluence" => "\u{e799}",
        "github-issues" => ph::GITHUB_LOGO,
        "gitlab-issues" => "\u{e7eb}",
        "forgejo-issues" => "\u{e910}",
        "redmine" => ph::BUG,
        _ => container_kind_icon(kind),
    }
}

/// Multi-coloured product logos (Devicon v2.18.0, MIT; see
/// `assets/icons/devicon/README.md`), by the URI they are drawn from.
/// [`install_image_loaders`] registers them (CR-0036).
pub static PRODUCT_LOGOS: &[(&str, &[u8])] = &[
    (
        "bytes://product/docker.svg",
        include_bytes!("../../../assets/icons/devicon/docker.svg"),
    ),
    (
        "bytes://product/podman.svg",
        include_bytes!("../../../assets/icons/devicon/podman.svg"),
    ),
    (
        "bytes://product/kubernetes.svg",
        include_bytes!("../../../assets/icons/devicon/kubernetes.svg"),
    ),
    (
        "bytes://product/windows11.svg",
        include_bytes!("../../../assets/icons/devicon/windows11.svg"),
    ),
    (
        "bytes://product/redis.svg",
        include_bytes!("../../../assets/icons/devicon/redis.svg"),
    ),
    (
        "bytes://product/mongodb.svg",
        include_bytes!("../../../assets/icons/devicon/mongodb.svg"),
    ),
    (
        "bytes://product/couchdb.svg",
        include_bytes!("../../../assets/icons/devicon/couchdb.svg"),
    ),
    (
        "bytes://product/postgresql.svg",
        include_bytes!("../../../assets/icons/devicon/postgresql.svg"),
    ),
    (
        "bytes://product/sqlite.svg",
        include_bytes!("../../../assets/icons/devicon/sqlite.svg"),
    ),
    (
        "bytes://product/clickhouse.svg",
        include_bytes!("../../../assets/icons/devicon/clickhouse.svg"),
    ),
    (
        "bytes://product/duckdb.svg",
        include_bytes!("../../../assets/icons/devicon/duckdb.svg"),
    ),
    (
        "bytes://product/microsoftsqlserver.svg",
        include_bytes!("../../../assets/icons/devicon/microsoftsqlserver.svg"),
    ),
    (
        "bytes://product/oracle.svg",
        include_bytes!("../../../assets/icons/devicon/oracle.svg"),
    ),
    (
        "bytes://product/elasticsearch.svg",
        include_bytes!("../../../assets/icons/devicon/elasticsearch.svg"),
    ),
    (
        "bytes://product/cassandra.svg",
        include_bytes!("../../../assets/icons/devicon/cassandra.svg"),
    ),
    (
        "bytes://product/dynamodb.svg",
        include_bytes!("../../../assets/icons/devicon/dynamodb.svg"),
    ),
    (
        "bytes://product/rabbitmq.svg",
        include_bytes!("../../../assets/icons/devicon/rabbitmq.svg"),
    ),
    (
        "bytes://product/nats.svg",
        include_bytes!("../../../assets/icons/devicon/nats.svg"),
    ),
    (
        "bytes://product/jenkins.svg",
        include_bytes!("../../../assets/icons/devicon/jenkins.svg"),
    ),
    (
        "bytes://product/teamcity.svg",
        include_bytes!("../../../assets/icons/devicon/teamcity.svg"),
    ),
    (
        "bytes://product/githubactions.svg",
        include_bytes!("../../../assets/icons/devicon/githubactions.svg"),
    ),
    (
        "bytes://product/gitlab.svg",
        include_bytes!("../../../assets/icons/devicon/gitlab.svg"),
    ),
    (
        "bytes://product/forgejo.svg",
        include_bytes!("../../../assets/icons/devicon/forgejo.svg"),
    ),
    (
        "bytes://product/ansible.svg",
        include_bytes!("../../../assets/icons/devicon/ansible.svg"),
    ),
    (
        "bytes://product/mysql.svg",
        include_bytes!("../../../assets/icons/devicon/mysql.svg"),
    ),
    (
        "bytes://product/mariadb.svg",
        include_bytes!("../../../assets/icons/devicon/mariadb.svg"),
    ),
    (
        "bytes://product/openai.svg",
        include_bytes!("../../../assets/icons/simple/openai.svg"),
    ),
    (
        "bytes://product/anthropic.svg",
        include_bytes!("../../../assets/icons/simple/anthropic.svg"),
    ),
    (
        "bytes://product/googlegemini.svg",
        include_bytes!("../../../assets/icons/simple/googlegemini.svg"),
    ),
    (
        "bytes://product/amazonwebservices.svg",
        include_bytes!("../../../assets/icons/simple/amazonwebservices.svg"),
    ),
    (
        "bytes://product/mistralai.svg",
        include_bytes!("../../../assets/icons/simple/mistralai.svg"),
    ),
    (
        "bytes://product/deepseek.svg",
        include_bytes!("../../../assets/icons/simple/deepseek.svg"),
    ),
    (
        "bytes://product/ollama.svg",
        include_bytes!("../../../assets/icons/simple/ollama.svg"),
    ),
    (
        "bytes://product/openrouter.svg",
        include_bytes!("../../../assets/icons/simple/openrouter.svg"),
    ),
    (
        "bytes://product/moonshotai.svg",
        include_bytes!("../../../assets/icons/simple/moonshotai.svg"),
    ),
    (
        "bytes://product/tencentqq.svg",
        include_bytes!("../../../assets/icons/simple/tencentqq.svg"),
    ),
    (
        "bytes://product/vllm.svg",
        include_bytes!("../../../assets/icons/simple/vllm.svg"),
    ),
    (
        "bytes://product/lmstudio.svg",
        include_bytes!("../../../assets/icons/simple/lmstudio.svg"),
    ),
    (
        "bytes://product/windsurf.svg",
        include_bytes!("../../../assets/icons/simple/windsurf.svg"),
    ),
    (
        "bytes://product/comfyui.svg",
        include_bytes!("../../../assets/icons/simple/comfyui.svg"),
    ),
    (
        "bytes://product/sdwebui.svg",
        include_bytes!("../../../assets/icons/simple/sdwebui.svg"),
    ),
    (
        "bytes://product/swarmui.svg",
        include_bytes!("../../../assets/icons/simple/swarmui.svg"),
    ),
    (
        "bytes://product/onemin.svg",
        include_bytes!("../../../assets/icons/simple/onemin.svg"),
    ),
    (
        "bytes://product/fal.svg",
        include_bytes!("../../../assets/icons/simple/fal.svg"),
    ),
    (
        "bytes://product/invokeai.svg",
        include_bytes!("../../../assets/icons/simple/invokeai.svg"),
    ),
    (
        "bytes://product/fooocus.svg",
        include_bytes!("../../../assets/icons/simple/fooocus.svg"),
    ),
    (
        "bytes://product/puppet.svg",
        include_bytes!("../../../assets/icons/simple/puppet.svg"),
    ),
    (
        "bytes://product/mqtt.svg",
        include_bytes!("../../../assets/icons/simple/mqtt.svg"),
    ),
    (
        "bytes://product/redmine.svg",
        include_bytes!("../../../assets/icons/simple/redmine.svg"),
    ),
    (
        "bytes://product/amazons3.svg",
        include_bytes!("../../../assets/icons/simple/amazons3.svg"),
    ),
    (
        "bytes://product/qdrant.svg",
        include_bytes!("../../../assets/icons/simple/qdrant.svg"),
    ),
    (
        "bytes://product/jira.svg",
        include_bytes!("../../../assets/icons/devicon/jira.svg"),
    ),
    (
        "bytes://product/confluence.svg",
        include_bytes!("../../../assets/icons/devicon/confluence.svg"),
    ),
    (
        "bytes://product/powershell.svg",
        include_bytes!("../../../assets/icons/devicon/powershell.svg"),
    ),
    (
        "bytes://product/bash.svg",
        include_bytes!("../../../assets/icons/devicon/bash.svg"),
    ),
    (
        "bytes://product/zsh.svg",
        include_bytes!("../../../assets/icons/devicon/zsh.svg"),
    ),
    (
        "bytes://product/fish.svg",
        include_bytes!("../../../assets/icons/devicon/fish.svg"),
    ),
    (
        "bytes://product/git.svg",
        include_bytes!("../../../assets/icons/devicon/git.svg"),
    ),
    (
        "bytes://product/ubuntu.svg",
        include_bytes!("../../../assets/icons/devicon/ubuntu.svg"),
    ),
    (
        "bytes://product/debian.svg",
        include_bytes!("../../../assets/icons/devicon/debian.svg"),
    ),
    (
        "bytes://product/fedora.svg",
        include_bytes!("../../../assets/icons/devicon/fedora.svg"),
    ),
    (
        "bytes://product/redhat.svg",
        include_bytes!("../../../assets/icons/devicon/redhat.svg"),
    ),
    (
        "bytes://product/centos.svg",
        include_bytes!("../../../assets/icons/devicon/centos.svg"),
    ),
    (
        "bytes://product/archlinux.svg",
        include_bytes!("../../../assets/icons/devicon/archlinux.svg"),
    ),
    (
        "bytes://product/opensuse.svg",
        include_bytes!("../../../assets/icons/devicon/opensuse.svg"),
    ),
    (
        "bytes://product/linux.svg",
        include_bytes!("../../../assets/icons/devicon/linux.svg"),
    ),
    (
        "bytes://product/apple-rainbow.svg",
        include_bytes!("../../../assets/icons/devicon/apple-rainbow.svg"),
    ),
    (
        "bytes://product/azure.svg",
        include_bytes!("../../../assets/icons/devicon/azure.svg"),
    ),
    (
        "bytes://product/googlecloud.svg",
        include_bytes!("../../../assets/icons/devicon/googlecloud.svg"),
    ),
    (
        "bytes://product/nodejs.svg",
        include_bytes!("../../../assets/icons/devicon/nodejs.svg"),
    ),
    (
        "bytes://product/python.svg",
        include_bytes!("../../../assets/icons/devicon/python.svg"),
    ),
    (
        "bytes://product/terraform.svg",
        include_bytes!("../../../assets/icons/devicon/terraform.svg"),
    ),
    (
        "bytes://product/claude.svg",
        include_bytes!("../../../assets/icons/devicon/claude.svg"),
    ),
    (
        "bytes://product/dotnetcore.svg",
        include_bytes!("../../../assets/icons/devicon/dotnetcore.svg"),
    ),
    (
        "bytes://product/go.svg",
        include_bytes!("../../../assets/icons/devicon/go.svg"),
    ),
    (
        "bytes://product/c.svg",
        include_bytes!("../../../assets/icons/devicon/c.svg"),
    ),
    (
        "bytes://product/cplusplus.svg",
        include_bytes!("../../../assets/icons/devicon/cplusplus.svg"),
    ),
    (
        "bytes://product/csharp.svg",
        include_bytes!("../../../assets/icons/devicon/csharp.svg"),
    ),
    (
        "bytes://product/java.svg",
        include_bytes!("../../../assets/icons/devicon/java.svg"),
    ),
    (
        "bytes://product/kotlin.svg",
        include_bytes!("../../../assets/icons/devicon/kotlin.svg"),
    ),
    (
        "bytes://product/scala.svg",
        include_bytes!("../../../assets/icons/devicon/scala.svg"),
    ),
    (
        "bytes://product/swift.svg",
        include_bytes!("../../../assets/icons/devicon/swift.svg"),
    ),
    (
        "bytes://product/zig.svg",
        include_bytes!("../../../assets/icons/devicon/zig.svg"),
    ),
    (
        "bytes://product/haskell.svg",
        include_bytes!("../../../assets/icons/devicon/haskell.svg"),
    ),
    (
        "bytes://product/elixir.svg",
        include_bytes!("../../../assets/icons/devicon/elixir.svg"),
    ),
    (
        "bytes://product/dart.svg",
        include_bytes!("../../../assets/icons/devicon/dart.svg"),
    ),
    (
        "bytes://product/ruby.svg",
        include_bytes!("../../../assets/icons/devicon/ruby.svg"),
    ),
    (
        "bytes://product/php.svg",
        include_bytes!("../../../assets/icons/devicon/php.svg"),
    ),
    (
        "bytes://product/perl.svg",
        include_bytes!("../../../assets/icons/devicon/perl.svg"),
    ),
    (
        "bytes://product/javascript.svg",
        include_bytes!("../../../assets/icons/devicon/javascript.svg"),
    ),
    (
        "bytes://product/typescript.svg",
        include_bytes!("../../../assets/icons/devicon/typescript.svg"),
    ),
    (
        "bytes://product/html5.svg",
        include_bytes!("../../../assets/icons/devicon/html5.svg"),
    ),
    (
        "bytes://product/css3.svg",
        include_bytes!("../../../assets/icons/devicon/css3.svg"),
    ),
    // Devicon's Rust mark is black; the file tree's orange one reads on both
    // themes (Material Icon Theme, MIT).
    (
        "bytes://product/rust.svg",
        include_bytes!("../../../assets/icons/material/rust.svg"),
    ),
    // Devicon's Lua is navy and its Markdown black, and the Nerd Font
    // glyph for Lua draws "TeX": the file tree's coloured ones instead.
    (
        "bytes://product/lua.svg",
        include_bytes!("../../../assets/icons/material/lua.svg"),
    ),
    (
        "bytes://product/markdown.svg",
        include_bytes!("../../../assets/icons/material/markdown.svg"),
    ),
];

/// How a connection kind's glyph is coloured when it has no logo to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnTint {
    /// The colour the surrounding control gives icons (muted, accent).
    Default,
    /// The product's own colour, for a product with no logo here.
    Brand(egui::Color32),
    /// The theme's text colour: a product whose logo is black, which would
    /// vanish on the dark theme (GitHub, Kafka, MariaDB).
    Text,
}

/// The icon of a connection kind, in one place for every list, tab,
/// dashboard and work-pane header (CR-0036): the product's multi-coloured
/// logo where there is one, otherwise the glyph and how to colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnIcon {
    pub glyph: &'static str,
    /// A `bytes://product/…` URI from [`PRODUCT_LOGOS`].
    pub image: Option<&'static str>,
    pub tint: ConnTint,
}

impl ConnIcon {
    /// A plain glyph, coloured as the control colours its icons.
    pub fn glyph(glyph: &'static str) -> Self {
        Self {
            glyph,
            image: None,
            tint: ConnTint::Default,
        }
    }

    /// The logo's URI, for an API that takes an owned one.
    pub fn uri(&self) -> Option<String> {
        self.image.map(str::to_string)
    }

    /// The brand colour, when the glyph carries one.
    pub fn brand(&self) -> Option<egui::Color32> {
        match self.tint {
            ConnTint::Brand(c) => Some(c),
            _ => None,
        }
    }

    /// The glyph's colour on `theme`, with `fallback` for [`ConnTint::Default`].
    pub fn glyph_colour(
        &self,
        theme: &crate::theme::Theme,
        fallback: egui::Color32,
    ) -> egui::Color32 {
        match self.tint {
            ConnTint::Brand(c) => c,
            ConnTint::Text => theme.text,
            ConnTint::Default => fallback,
        }
    }

    /// The glyph's colour when it has one of its own, on `theme`: the
    /// product's, or the text colour for a black logo; `None` when the
    /// control's usual icon colour applies.
    pub fn fixed_colour(&self, theme: &crate::theme::Theme) -> Option<egui::Color32> {
        match self.tint {
            ConnTint::Brand(c) => Some(c),
            ConnTint::Text => Some(theme.text),
            ConnTint::Default => None,
        }
    }

    /// A work-pane header band led by this icon.
    pub fn header(self, name: impl Into<String>) -> hs_core::tool::ToolHeader {
        let h = hs_core::tool::ToolHeader::new(self.glyph, name);
        let h = match self.image {
            Some(uri) => h.icon_image(uri),
            None => h,
        };
        match self.tint {
            ConnTint::Brand(c) => h.icon_hex(format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())),
            ConnTint::Text => h.icon_plain(),
            ConnTint::Default => h,
        }
    }
}

/// The logo file for a connection kind (and a database's engine), when
/// Devicon has one that reads on both themes.
fn product_logo(kind: &str, engine: &str) -> Option<&'static str> {
    let name = match kind {
        "db" => match engine {
            "postgres" | "postgresql" => "postgresql",
            "sqlite" => "sqlite",
            "clickhouse" => "clickhouse",
            "duckdb" => "duckdb",
            "mssql" | "sqlserver" => "microsoftsqlserver",
            "oracle" => "oracle",
            "mysql" => "mysql",
            "mariadb" => "mariadb",
            _ => return None,
        },
        "docker" => "docker",
        "podman" => "podman",
        "k8s" => "kubernetes",
        "wincontainer" => "windows11",
        "redis" | "redis-streams" => "redis",
        "mongodb" => "mongodb",
        "couchdb" => "couchdb",
        "elasticsearch" => "elasticsearch",
        "cassandra" => "cassandra",
        "dynamodb" => "dynamodb",
        "rabbitmq" => "rabbitmq",
        "nats" => "nats",
        "jenkins" => "jenkins",
        "teamcity" => "teamcity",
        "github-actions" => "githubactions",
        "gitlab-ci" | "gitlab-issues" => "gitlab",
        "forgejo-actions" | "forgejo-issues" => "forgejo",
        "awx" => "ansible",
        "puppet" => "puppet",
        // Simple Icons marks filled with the brand colour (CR-0052).
        "qdrant" => "qdrant",
        "mqtt" => "mqtt",
        "redmine" => "redmine",
        "s3" => "amazons3",
        "jira" => "jira",
        "confluence" => "confluence",
        _ => return None,
    };
    logo(name)
}

/// The URI of a vendored logo by its file name, without `.svg`.
/// The AI Assistant's service marks (CR-0055): Simple Icons where a mark
/// is published, a drawn look-alike where none is (`comfyui`, `sdwebui`,
/// `swarmui`, `onemin`, `fal`, `invokeai`, `fooocus`). Settings → AI
/// Assistant names them by slug through [`logo_for`].
pub const PROVIDER_LOGOS: &[&str] = &[
    "azure",
    "openai",
    "anthropic",
    "googlegemini",
    "amazonwebservices",
    "mistralai",
    "deepseek",
    "ollama",
    "openrouter",
    "moonshotai",
    "tencentqq",
    "vllm",
    "lmstudio",
    "windsurf",
    "comfyui",
    "sdwebui",
    "swarmui",
    "onemin",
    "fal",
    "invokeai",
    "fooocus",
];

/// A vendored logo's URI by its file name, for a caller that keeps its own
/// name → mark table (the AI services).
pub fn logo_for(name: &str) -> Option<&'static str> {
    logo(name)
}

fn logo(name: &str) -> Option<&'static str> {
    PRODUCT_LOGOS.iter().map(|(uri, _)| *uri).find(|uri| {
        uri.strip_prefix("bytes://product/")
            .and_then(|f| f.strip_suffix(".svg"))
            == Some(name)
    })
}

/// A product by its logo's name, or `glyph` when there is no such logo.
pub fn product_icon(name: &str, glyph: &'static str) -> ConnIcon {
    match logo(name) {
        Some(uri) => ConnIcon {
            glyph,
            image: Some(uri),
            tint: ConnTint::Default,
        },
        None => ConnIcon::glyph(glyph),
    }
}

/// `glyph` in a product's own colour, for a product with no logo here.
pub fn branded(glyph: &'static str, rgb: [u8; 3]) -> ConnIcon {
    ConnIcon {
        glyph,
        image: None,
        tint: ConnTint::Brand(egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
    }
}

/// `glyph` in the theme's text colour: a product whose logo is black.
pub fn plain_mark(glyph: &'static str) -> ConnIcon {
    ConnIcon {
        glyph,
        image: None,
        tint: ConnTint::Text,
    }
}

/// An operating system's logo, for an SSH host and a WSL shell
/// (CR-0036). macOS takes the striped Apple: the black one is a hole on
/// the dark theme.
pub fn os_conn_icon(id: &str) -> ConnIcon {
    let glyph = os_icon(id);
    match id {
        "ubuntu" => product_icon("ubuntu", glyph),
        "debian" => product_icon("debian", glyph),
        "fedora" => product_icon("fedora", glyph),
        "rhel" => product_icon("redhat", glyph),
        "centos" => product_icon("centos", glyph),
        "arch" => product_icon("archlinux", glyph),
        "suse" => product_icon("opensuse", glyph),
        "linux" => product_icon("linux", glyph),
        "macos" => product_icon("apple-rainbow", glyph),
        "windows" => product_icon("windows11", glyph),
        "alpine" => branded(glyph, [0x0d, 0x59, 0x7f]),
        "freebsd" => branded(glyph, [0xab, 0x2b, 0x28]),
        _ => ConnIcon::glyph(glyph),
    }
}

/// A language's logo, for the Editor's projects (CR-0036).
pub fn language_conn_icon(id: &str) -> ConnIcon {
    let glyph = language_icon(id);
    let name = match id {
        "rust" => "rust",
        "go" => "go",
        "c" => "c",
        "cpp" => "cplusplus",
        "csharp" => "csharp",
        "java" => "java",
        "kotlin" => "kotlin",
        "scala" => "scala",
        "swift" => "swift",
        "zig" => "zig",
        "haskell" => "haskell",
        "elixir" => "elixir",
        "dart" => "dart",
        "python" => "python",
        "ruby" => "ruby",
        "php" => "php",
        "perl" => "perl",
        "javascript" => "javascript",
        "typescript" => "typescript",
        "node" => "nodejs",
        "shell" => "bash",
        "terraform" => "terraform",
        "html" => "html5",
        "css" => "css3",
        "lua" => "lua",
        "markdown" => "markdown",
        _ => return ConnIcon::glyph(glyph),
    };
    product_icon(name, glyph)
}

/// A CLI tool's product, from the command it runs, or `glyph` - the
/// category the user picked - when the command is not one we know
/// (CR-0036).
pub fn cli_conn_icon(command: &str, glyph: &'static str) -> ConnIcon {
    let exe = std::path::Path::new(command.trim())
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let name = match exe.as_str() {
        "psql" | "pg_dump" => "postgresql",
        "mysql" | "mysqlsh" | "mariadb" => return plain_mark(connection_icon("db", "mysql")),
        "sqlite3" | "sqlite" => "sqlite",
        "redis-cli" | "valkey-cli" => "redis",
        "mongosh" | "mongo" => "mongodb",
        "clickhouse" | "clickhouse-client" => "clickhouse",
        "duckdb" => "duckdb",
        "sqlcmd" => "microsoftsqlserver",
        "sqlplus" => "oracle",
        "kubectl" | "k9s" => "kubernetes",
        "docker" => "docker",
        "podman" => "podman",
        "git" => "git",
        "az" => "azure",
        "gcloud" => "googlecloud",
        "terraform" | "tofu" => "terraform",
        "ansible" | "ansible-playbook" => "ansible",
        "node" | "npm" | "npx" => "nodejs",
        "python" | "python3" | "py" | "ipython" => "python",
        "dotnet" => "dotnetcore",
        "claude" => "claude",
        "gh" => return plain_mark(ph::GITHUB_LOGO),
        "aws" => return branded(ph::CLOUD, [0xff, 0x99, 0x00]),
        _ => return ConnIcon::glyph(glyph),
    };
    product_icon(name, glyph)
}

/// A connection kind's icon (CR-0036). See [`ConnIcon`].
pub fn conn_icon(kind: &str, engine: &str) -> ConnIcon {
    let glyph = connection_icon(kind, engine);
    if let Some(uri) = product_logo(kind, engine) {
        return ConnIcon {
            glyph,
            image: Some(uri),
            tint: ConnTint::Default,
        };
    }
    let tint = match (kind, engine) {
        // Black logos: the mark in the theme's text colour.
        // MySQL's blue dolphin is a thin outline that all but vanishes
        // on the dark theme; MariaDB shares it here (the database tool
        // runs MariaDB through its MySQL engine).
        ("github-issues", _) => ConnTint::Text,
        // MySQL's teal blue; MariaDB's sea green (CR-0052).
        ("db", "mysql") => ConnTint::Brand(egui::Color32::from_rgb(0x00, 0x75, 0x8f)),
        ("db", "mariadb") => ConnTint::Brand(egui::Color32::from_rgb(0x1f, 0x8a, 0x70)),
        // No logo here: the glyph in the product's colour.
        ("s3", _) => ConnTint::Brand(egui::Color32::from_rgb(0xff, 0x99, 0x00)),
        ("qdrant", _) => ConnTint::Brand(egui::Color32::from_rgb(0xdc, 0x24, 0x4c)),
        // MQTT's own #660066 is lost on the dark theme: a lighter step of
        // the same purple.
        ("mqtt", _) => ConnTint::Brand(egui::Color32::from_rgb(0x9c, 0x4f, 0x9c)),
        // Kafka's mark is black on white, which reads as unset beside the
        // other brands: the teal its ecosystem uses (CR-0051).
        ("kafka", _) => ConnTint::Brand(egui::Color32::from_rgb(0x23, 0x8a, 0xd1)),
        ("redmine", _) => ConnTint::Brand(egui::Color32::from_rgb(0xb3, 0x20, 0x24)),
        // Nexus has no vendored logo: its package glyph in Sonatype's
        // green. WebDAV is a folder on a server: the folder glyph in the
        // folder blue of the file icons (CR-0046).
        ("nexus", _) => ConnTint::Brand(egui::Color32::from_rgb(0x2e, 0xa4, 0x4f)),
        ("webdav", _) => ConnTint::Brand(egui::Color32::from_rgb(0x4a, 0x90, 0xe2)),
        ("puppet", _) => ConnTint::Brand(egui::Color32::from_rgb(0xff, 0xae, 0x1a)),
        _ => ConnTint::Default,
    };
    ConnIcon {
        glyph,
        image: None,
        tint,
    }
}

/// [`conn_icon`] for a saved profile: a database by its engine, an SSH
/// host by its operating system ([`os_conn_icon`]).
pub fn profile_conn_icon(profile: &hs_core::connections::ConnectionProfile) -> ConnIcon {
    if profile.kind == "ssh" {
        let os = profile
            .fields
            .get("ssh")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("os_id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        return if os.is_empty() {
            ConnIcon::glyph(profile_icon(profile))
        } else {
            os_conn_icon(os)
        };
    }
    let engine = profile
        .fields
        .get("db")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("engine"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    conn_icon(&profile.kind, engine)
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

/// The glyphs a saved repository may wear, by a stable key the list file
/// stores (`hs_core::git_repos::RepoEntry::icon`). A language id works
/// too, through [`language_icon`]; this is the hand-picked rest.
pub const REPO_ICONS: &[(&str, &str)] = &[
    ("git", ph::GIT_BRANCH),
    ("fork", ph::GIT_FORK),
    ("github", ph::GITHUB_LOGO),
    ("gitlab", ph::GITLAB_LOGO),
    ("folder", ph::FOLDER),
    ("package", ph::PACKAGE),
    ("cube", ph::CUBE),
    ("rocket", ph::ROCKET),
    ("database", ph::DATABASE),
    ("globe", ph::GLOBE),
    ("cloud", ph::CLOUD),
    ("terminal", ph::TERMINAL_WINDOW),
    ("book", ph::BOOK),
    ("bug", ph::BUG),
    ("flask", ph::FLASK),
    ("shield", ph::SHIELD),
    ("star", ph::STAR),
    ("heart", ph::HEART),
    ("lightning", ph::LIGHTNING),
    ("gear", ph::GEAR),
    ("wrench", ph::WRENCH),
    ("puzzle", ph::PUZZLE_PIECE),
    ("code", ph::CODE),
    ("atom", ph::ATOM),
    ("robot", ph::ROBOT),
    ("archive", ph::ARCHIVE),
    ("stack", ph::STACK),
    ("tag", ph::TAG),
];

/// An image file on disk as an egui image URI: the bytes are read once
/// per (path, modification stamp) and registered with the context under
/// a `bytes://` name, so a replaced file is shown anew and an unchanged
/// one costs a `stat` per call. `None` when the file cannot be read.
pub fn image_uri(ctx: &egui::Context, path: &std::path::Path) -> Option<String> {
    use std::collections::HashSet;
    use std::sync::{LazyLock, Mutex};
    static KNOWN: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    let stamp = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let uri = format!(
        "bytes://hs-image/{}/{stamp}.{ext}",
        path.display().to_string().replace(['\\', ':'], "/")
    );
    let mut known = KNOWN.lock().ok()?;
    if !known.contains(&uri) {
        let bytes = std::fs::read(path).ok()?;
        ctx.include_bytes(uri.clone(), bytes);
        known.insert(uri.clone());
    }
    Some(uri)
}

/// The glyph for a stored icon key: one of [`REPO_ICONS`], or a language
/// id. `None` for an empty or unknown key, so the caller can fall back to
/// what it knows about the thing (its language, its kind).
pub fn named(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        return None;
    }
    if let Some((_, g)) = REPO_ICONS.iter().find(|(k, _)| *k == key) {
        return Some(g);
    }
    hs_core::projects::LANGUAGES
        .iter()
        .any(|(id, _)| *id == key)
        .then(|| language_icon(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CR-0046: the object-store kinds with no logo wear a colour of
    /// their own, so the S3 sidebar and its tabs never show a plain glyph
    /// (S3 itself gained a mark in CR-0052 and is checked with the logos).
    #[test]
    fn nexus_and_webdav_resolve_to_a_tinted_glyph() {
        for k in ["nexus", "webdav"] {
            let icon = conn_icon(k, "");
            assert!(icon.image.is_none(), "{k} has no vendored logo");
            assert!(icon.brand().is_some(), "{k} is tinted");
        }
        assert_eq!(connection_icon("webdav", ""), ph::FOLDER_SIMPLE);
        assert_eq!(connection_icon("nexus", ""), ph::PACKAGE);
    }

    /// CR-0036: every kind a user can add has its product's logo, or a
    /// glyph coloured on purpose, and the only plain glyphs left are the
    /// ones with no product to show.
    #[test]
    fn every_connection_kind_resolves_to_its_product_icon() {
        let plain = ["ssh", "api", "valkey"];
        for k in hs_core::connections::KINDS {
            let icon = conn_icon(k, "");
            if *k == "db" {
                continue; // by engine, below
            }
            match (icon.image, icon.tint) {
                (Some(uri), _) => assert!(
                    PRODUCT_LOGOS.iter().any(|(u, _)| *u == uri),
                    "{k}'s logo {uri} is not registered"
                ),
                (None, ConnTint::Default) => {
                    assert!(plain.contains(k), "{k} has neither a logo nor a colour")
                }
                (None, _) => {}
            }
        }
        for (engine, logo) in [
            ("postgres", Some("postgresql")),
            ("mysql", Some("mysql")),
            ("sqlite", Some("sqlite")),
            ("clickhouse", Some("clickhouse")),
            ("duckdb", Some("duckdb")),
            ("mssql", Some("microsoftsqlserver")),
            ("oracle", Some("oracle")),
            ("mariadb", Some("mariadb")),
        ] {
            let icon = conn_icon("db", engine);
            assert_eq!(
                icon.image,
                logo.map(|l| format!("bytes://product/{l}.svg"))
                    .and_then(|u| PRODUCT_LOGOS.iter().map(|(p, _)| *p).find(|p| *p == u)),
                "{engine}"
            );
        }
        // GitHub's black mark keeps a glyph in the text colour, both
        // themes; the kinds that gained a brand tint are checked here too.
        assert_eq!(conn_icon("github-issues", "").tint, ConnTint::Text);
        assert!(matches!(conn_icon("kafka", "").tint, ConnTint::Brand(_)));
        // The Simple Icons marks are registered under their slugs.
        for (k, slug) in [
            ("puppet", "puppet"),
            ("s3", "amazons3"),
            ("qdrant", "qdrant"),
            ("mqtt", "mqtt"),
            ("redmine", "redmine"),
        ] {
            assert_eq!(
                conn_icon(k, "").image.map(str::to_owned),
                Some(format!("bytes://product/{slug}.svg")),
                "{k}"
            );
        }
    }

    /// An SSH host, a language and a CLI resolve to their products' logos;
    /// macOS takes the striped Apple, and the black marks a glyph.
    #[test]
    fn hosts_languages_and_clis_wear_their_products() {
        assert_eq!(
            os_conn_icon("macos").image,
            Some("bytes://product/apple-rainbow.svg")
        );
        assert_eq!(
            os_conn_icon("ubuntu").image,
            Some("bytes://product/ubuntu.svg")
        );
        assert!(os_conn_icon("alpine").brand().is_some());
        for (id, _) in hs_core::projects::LANGUAGES {
            let icon = language_conn_icon(id);
            assert!(
                icon.image.is_some() || icon.tint == ConnTint::Text,
                "{id} has neither a logo nor a deliberate glyph"
            );
        }
        assert_eq!(
            language_conn_icon("rust").image,
            Some("bytes://product/rust.svg")
        );
        assert_eq!(
            cli_conn_icon("C:/Program Files/PostgreSQL/17/bin/psql.exe", ph::DATABASE).image,
            Some("bytes://product/postgresql.svg")
        );
        assert_eq!(cli_conn_icon("gh", ph::TERMINAL).tint, ConnTint::Text);
        // An unknown command keeps the category the user picked.
        assert_eq!(
            cli_conn_icon("mytool", ph::DATABASE),
            ConnIcon::glyph(ph::DATABASE)
        );
    }

    /// The sidebar row, the tab and the work-pane header all draw the same
    /// picture for a kind: the header carries `conn_icon`'s URI and colour,
    /// which is what the tab hooks and the list rows take too.
    #[test]
    fn the_header_carries_the_same_icon_as_the_list_and_the_tab() {
        for k in ["docker", "redis", "rabbitmq", "jira", "mqtt", "kafka"] {
            let icon = conn_icon(k, "");
            let header = icon.header("x");
            assert_eq!(header.icon_uri, icon.uri(), "{k}");
            assert_eq!(header.icon, icon.glyph, "{k}");
            assert_eq!(
                header.icon_hex.is_some(),
                icon.brand().is_some(),
                "{k}'s colour"
            );
            assert_eq!(header.icon_plain, icon.tint == ConnTint::Text, "{k}");
        }
    }

    /// Every vendored logo is an SVG and is drawn for at least one kind.
    #[test]
    fn every_vendored_logo_is_an_svg_some_kind_uses() {
        let mut used = std::collections::HashSet::new();
        for k in hs_core::connections::KINDS {
            used.extend(conn_icon(k, "").image);
        }
        for e in [
            "postgres",
            "mysql",
            "sqlite",
            "clickhouse",
            "duckdb",
            "mssql",
            "oracle",
            "mariadb",
        ] {
            used.extend(conn_icon("db", e).image);
        }
        for os in [
            "ubuntu", "debian", "fedora", "rhel", "centos", "arch", "suse", "linux", "macos",
            "windows",
        ] {
            used.extend(os_conn_icon(os).image);
        }
        for (id, _) in hs_core::projects::LANGUAGES {
            used.extend(language_conn_icon(id).image);
        }
        for cmd in [
            "az", "gcloud", "git", "dotnet", "claude", "pwsh", "bash", "zsh", "fish",
        ] {
            used.extend(cli_conn_icon(cmd, ph::TERMINAL).image);
        }
        // The shells' own marks (tool-terminal maps them by shell id).
        for name in ["powershell", "bash", "zsh", "fish", "git"] {
            used.extend(logo(name));
        }
        for name in PROVIDER_LOGOS {
            assert!(logo(name).is_some(), "{name} is named but not vendored");
            used.extend(logo(name));
        }
        for (uri, bytes) in PRODUCT_LOGOS {
            let text = std::str::from_utf8(bytes).expect("utf-8");
            assert!(text.trim_start().starts_with("<svg"), "{uri}");
            assert!(
                used.contains(uri),
                "{uri} is vendored but drawn for no kind"
            );
        }
    }

    /// CR-0024: the activities' connections wear their products' marks,
    /// so no kind may fall through to the container fallback.
    #[test]
    fn every_queue_automation_and_tracker_kind_has_a_mark_of_its_own() {
        use hs_core::connections::{AUTOMATION_KINDS, QUEUE_KINDS, TRACKER_KINDS};
        let fallback = connection_icon("no-such-kind", "");
        for k in QUEUE_KINDS
            .iter()
            .chain(AUTOMATION_KINDS)
            .chain(TRACKER_KINDS)
        {
            assert_ne!(connection_icon(k, ""), fallback, "{k}");
        }
    }
}

/// The open workspace's logo as an image uri a tab can draw, or `None`
/// when the workspace has none.
///
/// Asked for on every frame the tab strip is drawn, so the file is read
/// only when the workspace changes - a root that differs from the cached
/// one - or after [`reload_workspace_logo`] says the logo itself changed.
/// Every other frame costs a lock and a path compare, no disk (owner's
/// rule, CR-0043: load on switch, not per frame).
pub fn workspace_logo_uri(ctx: &egui::Context, s: &hs_core::Services) -> Option<String> {
    let root = s.workspace.read().root().to_path_buf();
    let cached: Option<LogoCache> = ctx.data(|d| d.get_temp(logo_cache_id()));
    if let Some(c) = cached
        && c.root == root
    {
        return c.uri;
    }
    let uri = hs_core::workspace::workspace_icon_file(&root).and_then(|f| {
        let bytes = std::fs::read(&f).ok()?;
        // A new name for a new picture: egui keeps an image by its uri,
        // so a replaced logo needs one the old picture was never under.
        let uri = format!("bytes://workspace-logo/{}.png", crate_stamp(&f, &bytes));
        ctx.include_bytes(uri.clone(), bytes);
        Some(uri)
    });
    ctx.data_mut(|d| {
        d.insert_temp(
            logo_cache_id(),
            LogoCache {
                root,
                uri: uri.clone(),
            },
        )
    });
    uri
}

/// Forget the loaded logo, so the next [`workspace_logo_uri`] reads the
/// file again. The shell calls it when Settings -> Workspace sets or
/// removes the logo; a workspace switch needs no call, since the root no
/// longer matches.
pub fn reload_workspace_logo(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<LogoCache>(logo_cache_id()));
}

#[derive(Clone)]
struct LogoCache {
    root: std::path::PathBuf,
    uri: Option<String>,
}

fn logo_cache_id() -> egui::Id {
    egui::Id::new("hs-workspace-logo")
}

/// What makes one logo's uri differ from the last: its time on disk and
/// its length, read once, when the file is.
fn crate_stamp(file: &std::path::Path, bytes: &[u8]) -> String {
    let at = std::fs::metadata(file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{at}-{}", bytes.len())
}
