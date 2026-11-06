//! Themes (docs/04 §8). The brand palette comes from the app icon: purple
//! accent, blue secondary, gold reserved for warnings and the beta badge.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::Color32;
use hs_core::settings::Appearance;

/// Whether the operating system currently prefers a dark colour scheme, as
/// last reported by the window (`sync_system`). Dark until told otherwise.
static SYSTEM_DARK: AtomicBool = AtomicBool::new(true);

pub fn system_dark() -> bool {
    SYSTEM_DARK.load(Ordering::Relaxed)
}

/// Record the OS preference from the frame's input. Returns `true` when it
/// changed, so the shell can re-apply a theme in System mode.
pub fn sync_system(ctx: &egui::Context) -> bool {
    let Some(theme) = ctx.system_theme() else {
        return false;
    };
    let dark = theme == egui::Theme::Dark;
    SYSTEM_DARK.swap(dark, Ordering::Relaxed) != dark
}

/// The scheme id the Appearance settings select right now: the dark scheme
/// in Dark mode, the light one in Light mode, and in System mode whichever
/// the OS prefers. An empty mode is the pre-mode behaviour (`theme` as is).
pub fn scheme_id(app: &Appearance) -> &str {
    match app.mode.as_str() {
        "light" => &app.light_theme,
        "system" => {
            if system_dark() {
                &app.theme
            } else {
                &app.light_theme
            }
        }
        "dark" => &app.theme,
        _ => &app.theme,
    }
}

/// Push the selected scheme(s) into egui. Explicit modes pin egui's theme
/// preference and fill both of its visuals slots, so a Windows 10 machine
/// whose system theme is reported late (or reported light while the user
/// chose dark) can never swap our palette for egui's default one. System
/// mode gives egui both palettes and lets it follow the OS.
pub fn install(ctx: &egui::Context, app: &Appearance) {
    match app.mode.as_str() {
        "system" => {
            let dark = Theme::by_name(&app.theme);
            let light = Theme::by_name(&app.light_theme);
            ctx.set_visuals_of(egui::Theme::Dark, dark.visuals());
            ctx.set_visuals_of(egui::Theme::Light, light.visuals());
            ctx.set_theme(egui::ThemePreference::System);
        }
        _ => {
            let t = Theme::from_settings(app);
            let v = t.visuals();
            ctx.set_visuals_of(egui::Theme::Dark, v.clone());
            ctx.set_visuals_of(egui::Theme::Light, v);
            ctx.set_theme(if t.dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
        }
    }
}

pub const BRAND_PURPLE: Color32 = Color32::from_rgb(0x72, 0x00, 0xCA);
pub const BRAND_BLUE: Color32 = Color32::from_rgb(0x00, 0x75, 0xD3);
pub const BRAND_GOLD: Color32 = Color32::from_rgb(0xA7, 0x87, 0x00);

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub secondary: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub tab_active: Color32,
    pub tab_inactive: Color32,
    pub env_dev: Color32,
    pub env_test: Color32,
    pub env_staging: Color32,
    pub env_prod: Color32,
    /// The pin mark: a pinned Scratchpad note wears it in the sidebar
    /// and on its tab. Chosen in Settings -> Appearance -> Scratchpad,
    /// so it defaults to the scheme's red and follows the setting when
    /// the theme is built from one (CR-0041).
    pub pin: Color32,
}

impl Theme {
    /// What an icon's outline is drawn in: light on a dark theme, dark
    /// on a light one, so the ring always separates the glyph from the
    /// panel behind it rather than blending into it. Slightly
    /// translucent, so it reads as an edge and not as a second icon.
    pub fn icon_outline(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 205)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 190)
        }
    }

    pub fn dark() -> Self {
        Self {
            name: "HyperStack Dark".into(),
            dark: true,
            window_bg: Color32::from_rgb(0x15, 0x15, 0x1A),
            panel_bg: Color32::from_rgb(0x1C, 0x1C, 0x23),
            surface: Color32::from_rgb(0x24, 0x24, 0x2D),
            surface_hover: Color32::from_rgb(0x2E, 0x2E, 0x39),
            border: Color32::from_rgb(0x34, 0x34, 0x40),
            text: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            text_muted: Color32::from_rgb(0x9A, 0x9A, 0xA6),
            accent: Color32::from_rgb(0x9A, 0x4D, 0xF0),
            secondary: Color32::from_rgb(0x3F, 0x9C, 0xEE),
            success: Color32::from_rgb(0x3C, 0xB3, 0x71),
            warning: Color32::from_rgb(0xD9, 0xB0, 0x2A),
            error: Color32::from_rgb(0xE5, 0x4D, 0x4D),
            tab_active: Color32::from_rgb(0x2A, 0x2A, 0x35),
            tab_inactive: Color32::from_rgb(0x1C, 0x1C, 0x23),
            env_dev: Color32::from_rgb(0x3F, 0x9C, 0xEE),
            env_test: Color32::from_rgb(0x3C, 0xB3, 0x71),
            env_staging: Color32::from_rgb(0xD9, 0xB0, 0x2A),
            env_prod: Color32::from_rgb(0xE5, 0x4D, 0x4D),
            pin: Color32::from_rgb(0xE5, 0x4D, 0x4D),
        }
    }

    pub fn light() -> Self {
        Self {
            name: "HyperStack Light".into(),
            dark: false,
            window_bg: Color32::from_rgb(0xF6, 0xF6, 0xF8),
            panel_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            surface: Color32::from_rgb(0xEE, 0xEE, 0xF2),
            surface_hover: Color32::from_rgb(0xE2, 0xE2, 0xE8),
            border: Color32::from_rgb(0xD4, 0xD4, 0xDC),
            text: Color32::from_rgb(0x1E, 0x1E, 0x24),
            text_muted: Color32::from_rgb(0x6A, 0x6A, 0x76),
            accent: BRAND_PURPLE,
            secondary: BRAND_BLUE,
            success: Color32::from_rgb(0x1F, 0x8A, 0x4C),
            warning: BRAND_GOLD,
            error: Color32::from_rgb(0xC6, 0x2A, 0x2A),
            tab_active: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            tab_inactive: Color32::from_rgb(0xEA, 0xEA, 0xEF),
            env_dev: BRAND_BLUE,
            env_test: Color32::from_rgb(0x1F, 0x8A, 0x4C),
            env_staging: BRAND_GOLD,
            env_prod: Color32::from_rgb(0xC6, 0x2A, 0x2A),
            pin: Color32::from_rgb(0xC6, 0x2A, 0x2A),
        }
    }

    /// The theme the Appearance settings select right now (see `scheme_id`).
    pub fn from_settings(app: &Appearance) -> Self {
        let mut t = Self::by_name(scheme_id(app));
        if let Some(c) = parse_hex(hs_core::settings::pin_colour_hex(&app.pin_colour)) {
            t.pin = c;
        }
        t
    }

    pub fn by_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "light" => Self::light(),
            "dracula" => Self::dracula(),
            "nord" => Self::nord(),
            "one-dark" => Self::one_dark(),
            "monokai" => Self::monokai(),
            "gruvbox-dark" => Self::gruvbox_dark(),
            "catppuccin-mocha" => Self::catppuccin_mocha(),
            "solarized-dark" => Self::solarized_dark(),
            "solarized-light" => Self::solarized_light(),
            "github-light" => Self::github_light(),
            "tokyo-night" => Self::tokyo_night(),
            "rose-pine" => Self::rose_pine(),
            "kanagawa" => Self::kanagawa(),
            "everforest-dark" => Self::everforest_dark(),
            "ayu-dark" => Self::ayu_dark(),
            "night-owl" => Self::night_owl(),
            "palenight" => Self::palenight(),
            "github-dark" => Self::github_dark(),
            "catppuccin-macchiato" => Self::catppuccin_macchiato(),
            "one-light" => Self::one_light(),
            "gruvbox-light" => Self::gruvbox_light(),
            "catppuccin-latte" => Self::catppuccin_latte(),
            "rose-pine-dawn" => Self::rose_pine_dawn(),
            "ayu-light" => Self::ayu_light(),
            "everforest-light" => Self::everforest_light(),
            "tokyo-night-day" => Self::tokyo_night_day(),
            "tomorrow" => Self::tomorrow(),
            "light-owl" => Self::light_owl(),
            _ => Self::dark(),
        }
    }

    /// Every bundled colour scheme: (settings id, display name, dark).
    pub fn schemes() -> &'static [(&'static str, &'static str, bool)] {
        &[
            ("dark", "HyperStack Dark", true),
            ("light", "HyperStack Light", false),
            ("ayu-dark", "Ayu Dark", true),
            ("catppuccin-macchiato", "Catppuccin Macchiato", true),
            ("catppuccin-mocha", "Catppuccin Mocha", true),
            ("dracula", "Dracula", true),
            ("everforest-dark", "Everforest Dark", true),
            ("github-dark", "GitHub Dark", true),
            ("gruvbox-dark", "Gruvbox Dark", true),
            ("kanagawa", "Kanagawa", true),
            ("palenight", "Material Palenight", true),
            ("monokai", "Monokai", true),
            ("night-owl", "Night Owl", true),
            ("nord", "Nord", true),
            ("one-dark", "One Dark", true),
            ("rose-pine", "Rosé Pine", true),
            ("solarized-dark", "Solarized Dark", true),
            ("tokyo-night", "Tokyo Night", true),
            ("ayu-light", "Ayu Light", false),
            ("catppuccin-latte", "Catppuccin Latte", false),
            ("everforest-light", "Everforest Light", false),
            ("github-light", "GitHub Light", false),
            ("gruvbox-light", "Gruvbox Light", false),
            ("light-owl", "Light Owl", false),
            ("one-light", "One Light", false),
            ("rose-pine-dawn", "Rosé Pine Dawn", false),
            ("solarized-light", "Solarized Light", false),
            ("tokyo-night-day", "Tokyo Night Day", false),
            ("tomorrow", "Tomorrow", false),
        ]
    }

    /// A dark scheme from six base colours plus accents.
    #[allow(clippy::too_many_arguments)]
    fn scheme(
        name: &str,
        dark: bool,
        bg: Color32,
        panel: Color32,
        surface: Color32,
        hover: Color32,
        border: Color32,
        text: Color32,
        muted: Color32,
        accent: Color32,
        secondary: Color32,
        success: Color32,
        warning: Color32,
        error: Color32,
    ) -> Self {
        Self {
            name: name.into(),
            dark,
            window_bg: bg,
            panel_bg: panel,
            surface,
            surface_hover: hover,
            border,
            text,
            text_muted: muted,
            accent,
            secondary,
            success,
            warning,
            error,
            tab_active: surface,
            tab_inactive: panel,
            env_dev: secondary,
            env_test: success,
            env_staging: warning,
            env_prod: error,
            pin: error,
        }
    }

    fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    pub fn dracula() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Dracula",
            true,
            c(0x1E1F29),
            c(0x282A36),
            c(0x343746),
            c(0x44475A),
            c(0x44475A),
            c(0xF8F8F2),
            c(0x9A9CB0),
            c(0xBD93F9),
            c(0x8BE9FD),
            c(0x50FA7B),
            c(0xF1FA8C),
            c(0xFF5555),
        )
    }
    pub fn nord() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Nord",
            true,
            c(0x242933),
            c(0x2E3440),
            c(0x3B4252),
            c(0x434C5E),
            c(0x4C566A),
            c(0xECEFF4),
            c(0xA3ACBD),
            c(0x88C0D0),
            c(0x81A1C1),
            c(0xA3BE8C),
            c(0xEBCB8B),
            c(0xBF616A),
        )
    }
    pub fn one_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "One Dark",
            true,
            c(0x21252B),
            c(0x282C34),
            c(0x2C313A),
            c(0x3A3F4B),
            c(0x3E4451),
            c(0xABB2BF),
            c(0x7F848E),
            c(0xC678DD),
            c(0x61AFEF),
            c(0x98C379),
            c(0xE5C07B),
            c(0xE06C75),
        )
    }
    pub fn monokai() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Monokai",
            true,
            c(0x1E1F1C),
            c(0x272822),
            c(0x33342C),
            c(0x3E3D32),
            c(0x49483E),
            c(0xF8F8F2),
            c(0x90908A),
            c(0xAE81FF),
            c(0x66D9EF),
            c(0xA6E22E),
            c(0xE6DB74),
            c(0xF92672),
        )
    }
    pub fn gruvbox_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Gruvbox Dark",
            true,
            c(0x1D2021),
            c(0x282828),
            c(0x3C3836),
            c(0x504945),
            c(0x504945),
            c(0xEBDBB2),
            c(0xA89984),
            c(0xD3869B),
            c(0x83A598),
            c(0xB8BB26),
            c(0xFABD2F),
            c(0xFB4934),
        )
    }
    pub fn catppuccin_mocha() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Catppuccin Mocha",
            true,
            c(0x181825),
            c(0x1E1E2E),
            c(0x313244),
            c(0x45475A),
            c(0x45475A),
            c(0xCDD6F4),
            c(0xA6ADC8),
            c(0xCBA6F7),
            c(0x89B4FA),
            c(0xA6E3A1),
            c(0xF9E2AF),
            c(0xF38BA8),
        )
    }
    pub fn solarized_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Solarized Dark",
            true,
            c(0x00212B),
            c(0x002B36),
            c(0x073642),
            c(0x0B4453),
            c(0x0B4453),
            c(0xEEE8D5),
            c(0x93A1A1),
            c(0x6C71C4),
            c(0x268BD2),
            c(0x859900),
            c(0xB58900),
            c(0xDC322F),
        )
    }
    pub fn solarized_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Solarized Light",
            false,
            c(0xEEE8D5),
            c(0xFDF6E3),
            c(0xF3EDD9),
            c(0xE9E2CB),
            c(0xD9D2BB),
            c(0x073642),
            c(0x657B83),
            c(0x6C71C4),
            c(0x268BD2),
            c(0x859900),
            c(0xB58900),
            c(0xDC322F),
        )
    }
    pub fn github_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "GitHub Light",
            false,
            c(0xF6F8FA),
            c(0xFFFFFF),
            c(0xF0F3F6),
            c(0xE6EAEF),
            c(0xD0D7DE),
            c(0x1F2328),
            c(0x656D76),
            c(0x8250DF),
            c(0x0969DA),
            c(0x1A7F37),
            c(0x9A6700),
            c(0xCF222E),
        )
    }
    pub fn tokyo_night() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Tokyo Night",
            true,
            c(0x16161E),
            c(0x1A1B26),
            c(0x24283B),
            c(0x292E42),
            c(0x3B4261),
            c(0xC0CAF5),
            c(0x9AA5CE),
            c(0xBB9AF7),
            c(0x7AA2F7),
            c(0x9ECE6A),
            c(0xE0AF68),
            c(0xF7768E),
        )
    }
    pub fn rose_pine() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Rosé Pine",
            true,
            c(0x16141F),
            c(0x191724),
            c(0x1F1D2E),
            c(0x26233A),
            c(0x403D52),
            c(0xE0DEF4),
            c(0x908CAA),
            c(0xC4A7E7),
            c(0xEBBCBA),
            c(0x9CCFD8),
            c(0xF6C177),
            c(0xEB6F92),
        )
    }
    pub fn kanagawa() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Kanagawa",
            true,
            c(0x16161D),
            c(0x1F1F28),
            c(0x2A2A37),
            c(0x363646),
            c(0x54546D),
            c(0xDCD7BA),
            c(0x727169),
            c(0x957FB8),
            c(0x7E9CD8),
            c(0x98BB6C),
            c(0xE6C384),
            c(0xFF5D62),
        )
    }
    pub fn everforest_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Everforest Dark",
            true,
            c(0x232A2E),
            c(0x2D353B),
            c(0x343F44),
            c(0x3D484D),
            c(0x475258),
            c(0xD3C6AA),
            c(0x9DA9A0),
            c(0xA7C080),
            c(0x7FBBB3),
            c(0x83C092),
            c(0xDBBC7F),
            c(0xE67E80),
        )
    }
    pub fn ayu_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Ayu Dark",
            true,
            c(0x0B0E14),
            c(0x0D1017),
            c(0x131721),
            c(0x1A1F29),
            c(0x1E232B),
            c(0xBFBDB6),
            c(0x707A8C),
            c(0xE6B450),
            c(0x59C2FF),
            c(0xAAD94C),
            c(0xFFB454),
            c(0xD95757),
        )
    }
    pub fn night_owl() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Night Owl",
            true,
            c(0x010E1A),
            c(0x011627),
            c(0x0B253A),
            c(0x1D3B53),
            c(0x1D3B53),
            c(0xD6DEEB),
            c(0x637777),
            c(0xC792EA),
            c(0x82AAFF),
            c(0xADDB67),
            c(0xECC48D),
            c(0xEF5350),
        )
    }
    pub fn palenight() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Material Palenight",
            true,
            c(0x202331),
            c(0x292D3E),
            c(0x34324A),
            c(0x444267),
            c(0x4E5579),
            c(0xA6ACCD),
            c(0x676E95),
            c(0xC792EA),
            c(0x82AAFF),
            c(0xC3E88D),
            c(0xFFCB6B),
            c(0xF07178),
        )
    }
    pub fn github_dark() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "GitHub Dark",
            true,
            c(0x010409),
            c(0x0D1117),
            c(0x161B22),
            c(0x21262D),
            c(0x30363D),
            c(0xE6EDF3),
            c(0x7D8590),
            c(0xA371F7),
            c(0x2F81F7),
            c(0x3FB950),
            c(0xD29922),
            c(0xF85149),
        )
    }
    pub fn catppuccin_macchiato() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Catppuccin Macchiato",
            true,
            c(0x1E2030),
            c(0x24273A),
            c(0x363A4F),
            c(0x494D64),
            c(0x494D64),
            c(0xCAD3F5),
            c(0xA5ADCB),
            c(0xC6A0F6),
            c(0x8AADF4),
            c(0xA6DA95),
            c(0xEED49F),
            c(0xED8796),
        )
    }
    pub fn one_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "One Light",
            false,
            c(0xF0F0F1),
            c(0xFAFAFA),
            c(0xEAEAEB),
            c(0xDBDBDC),
            c(0xDBDBDC),
            c(0x383A42),
            c(0x696C77),
            c(0xA626A4),
            c(0x4078F2),
            c(0x50A14F),
            c(0xC18401),
            c(0xE45649),
        )
    }
    pub fn gruvbox_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Gruvbox Light",
            false,
            c(0xF2E5BC),
            c(0xFBF1C7),
            c(0xEBDBB2),
            c(0xD5C4A1),
            c(0xD5C4A1),
            c(0x3C3836),
            c(0x7C6F64),
            c(0x8F3F71),
            c(0x076678),
            c(0x79740E),
            c(0xB57614),
            c(0x9D0006),
        )
    }
    pub fn catppuccin_latte() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Catppuccin Latte",
            false,
            c(0xE6E9EF),
            c(0xEFF1F5),
            c(0xDCE0E8),
            c(0xCCD0DA),
            c(0xBCC0CC),
            c(0x4C4F69),
            c(0x6C6F85),
            c(0x8839EF),
            c(0x1E66F5),
            c(0x40A02B),
            c(0xDF8E1D),
            c(0xD20F39),
        )
    }
    pub fn rose_pine_dawn() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Rosé Pine Dawn",
            false,
            c(0xFAF4ED),
            c(0xFFFAF3),
            c(0xF2E9E1),
            c(0xDFDAD9),
            c(0xCECACD),
            c(0x575279),
            c(0x797593),
            c(0x907AA9),
            c(0x286983),
            c(0x56949F),
            c(0xEA9D34),
            c(0xB4637A),
        )
    }
    pub fn ayu_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Ayu Light",
            false,
            c(0xF8F9FA),
            c(0xFCFCFC),
            c(0xF3F4F5),
            c(0xE7EAED),
            c(0xE7EAED),
            c(0x5C6166),
            c(0x8A9199),
            // Contrast: the published keyword orange #FA8D3E, darkened for 3:1 on the window.
            c(0xD27634),
            c(0x399EE6),
            c(0x86B300),
            c(0xF2AE49),
            c(0xE65050),
        )
    }
    pub fn everforest_light() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Everforest Light",
            false,
            c(0xEFEBD4),
            c(0xFDF6E3),
            c(0xF4F0D9),
            c(0xE6E2CC),
            c(0xE0DCC7),
            c(0x5C6A72),
            // Contrast: the published grey2 #829181, darkened for 3:1 on the window.
            c(0x7A8879),
            // Contrast: the published green #8DA101, darkened for 3:1 on the window.
            c(0x7C8E01),
            c(0x3A94C5),
            c(0x35A77C),
            c(0xDFA000),
            c(0xF85552),
        )
    }
    pub fn tokyo_night_day() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Tokyo Night Day",
            false,
            c(0xD0D5E3),
            c(0xE1E2E7),
            c(0xE9E9EC),
            c(0xC4C8DA),
            c(0xC4C8DA),
            // Contrast: the published fg #3760BF, darkened for 4.5:1 on the window.
            c(0x3257AE),
            c(0x6172B0),
            // Contrast: the published magenta #9854F1, darkened for 3:1 on the window.
            c(0x9351EA),
            c(0x2E7DE9),
            c(0x587539),
            c(0x8C6C3E),
            c(0xF52A65),
        )
    }
    pub fn tomorrow() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Tomorrow",
            false,
            c(0xF5F5F5),
            c(0xFFFFFF),
            c(0xEFEFEF),
            c(0xD6D6D6),
            c(0xD6D6D6),
            c(0x4D4D4C),
            // Contrast: the published comment #8E908C, darkened for 3:1 on the window.
            c(0x8B8D89),
            c(0x8959A8),
            c(0x4271AE),
            c(0x718C00),
            c(0xEAB700),
            c(0xC82829),
        )
    }
    pub fn light_owl() -> Self {
        let c = Self::rgb;
        Self::scheme(
            "Light Owl",
            false,
            c(0xF0F0F0),
            c(0xFBFBFB),
            c(0xF6F6F6),
            c(0xE0E0E0),
            c(0xD9D9D9),
            c(0x403F53),
            // Contrast: the published comment #989FB1, darkened for 3:1 on the window.
            c(0x838998),
            c(0x994CC3),
            c(0x4876D6),
            c(0x08916A),
            c(0xE0AF02),
            c(0xDE3D3B),
        )
    }

    /// Push the palette into egui's visuals (both theme slots, preference
    /// pinned to this palette's darkness). Prefer `install` when the
    /// Appearance settings are at hand.
    pub fn apply(&self, ctx: &egui::Context) {
        let v = self.visuals();
        ctx.set_visuals_of(egui::Theme::Dark, v.clone());
        ctx.set_visuals_of(egui::Theme::Light, v);
        ctx.set_theme(if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
    }

    /// The background of anything the user types into or picks from: a
    /// text field, a dropdown, a checkbox, a radio, a slider's rail.
    ///
    /// These sit **inside** cards painted in `surface`, and the visuals
    /// used to give them `surface` as well - so on a dark theme a
    /// dropdown was a word floating on the card with no box around it,
    /// and a checkbox was a tick with no checkbox (owner's report with a
    /// screenshot, 2026-10-26). A field is a well: darker than the card
    /// on a dark theme, and a shade down from it on a light one, where
    /// going darker still would read as disabled.
    pub fn field_bg(&self) -> Color32 {
        if self.dark {
            mix(self.surface, Color32::BLACK, 0.45)
        } else {
            mix(self.surface, Color32::BLACK, 0.06)
        }
    }

    /// egui visuals for this palette.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.window_fill = self.panel_bg;
        v.panel_fill = self.panel_bg;
        // What a `TextEdit` and a code block are painted on.
        v.extreme_bg_color = self.field_bg();
        v.faint_bg_color = self.surface;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.secondary;
        v.selection.bg_fill = self.accent.linear_multiply(0.35);
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.noninteractive.bg_fill = self.panel_bg;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        // Fields are a well in the card, not the card itself, and they
        // keep an outline so the shape is visible even where the two
        // colours are close - which is most of the light themes.
        v.widgets.inactive.bg_fill = self.field_bg();
        v.widgets.inactive.weak_bg_fill = self.field_bg();
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.border);
        v.widgets.hovered.bg_fill = self.surface_hover;
        v.widgets.hovered.weak_bg_fill = self.surface_hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.active.bg_fill = self.accent.linear_multiply(0.5);
        v.widgets.active.weak_bg_fill = self.accent.linear_multiply(0.5);
        // egui grows a hovered or pressed widget by a pixel on every side
        // (`expansion`), which reads as rows and buttons magnifying under
        // the pointer. A highlight says "hovered" on its own (CR-0050).
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.expansion = 0.0;
        v.widgets.open.expansion = 0.0;
        v.widgets.open.bg_fill = self.surface_hover;
        v.window_stroke = egui::Stroke::new(1.0, self.border);
        v.window_corner_radius = egui::CornerRadius::same(6);
        v.menu_corner_radius = egui::CornerRadius::same(6);
        v.warn_fg_color = self.warning;
        v.error_fg_color = self.error;
        v
    }

    /// Colour for an environment: explicit hex if given, else by name, else the accent.
    pub fn env_colour(&self, name: &str, hex: Option<&str>) -> Color32 {
        if let Some(c) = hex.and_then(parse_hex) {
            return c;
        }
        let n = name.to_ascii_lowercase();
        if n.starts_with("prod") || n == "live" {
            self.env_prod
        } else if n.starts_with("stag") || n.starts_with("uat") || n.starts_with("pre") {
            self.env_staging
        } else if n.starts_with("test") || n.starts_with("qa") {
            self.env_test
        } else if n.starts_with("dev") || n.starts_with("local") {
            self.env_dev
        } else {
            self.accent
        }
    }
}

/// `#RRGGBB` or `#RRGGBBAA`.
/// Interface font size (Settings → Appearance → Interface Font Size): menus,
/// tabs, sidebars, settings and dialogs. The terminal and the editor have
/// their own sizes. egui's defaults are Body 12.5 / Heading 18 / Small 9 /
/// Monospace 12 for a `size` of 12.5; everything scales with it.
pub fn apply_font_size(ctx: &egui::Context, size: f32) {
    let size = size.clamp(9.0, 32.0);
    ctx.all_styles_mut(|style| {
        use egui::{FontFamily, FontId, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(size, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(size, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new((size * 0.78).round(), FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new((size * 1.45).round(), FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new((size * 0.96).round(), FontFamily::Monospace),
        );
    });
}

pub fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

/// Blend `a` towards `b` by `t` (0 = `a`, 1 = `b`), channel by channel.
///
/// `Color32::linear_multiply` is not this: it scales the alpha too, so
/// it dims a colour rather than darkening it (CLAUDE.md).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

#[cfg(test)]
mod tests {
    /// CR-0050: nothing grows under the pointer; a highlight is enough.
    #[test]
    fn the_applied_style_has_no_hover_or_press_expansion() {
        let ctx = egui::Context::default();
        super::Theme::dark().apply(&ctx);
        let v = ctx.global_style().visuals.clone();
        assert_eq!(v.widgets.hovered.expansion, 0.0);
        assert_eq!(v.widgets.active.expansion, 0.0);
        assert_eq!(v.widgets.open.expansion, 0.0);
    }

    /// A field has to be distinguishable from the card it sits in, in
    /// every bundled scheme - that is the whole point of `field_bg`, and
    /// a scheme added later must not quietly lose it.
    #[test]
    fn every_scheme_separates_a_field_from_its_card() {
        for (id, _, _) in Theme::schemes() {
            let t = Theme::by_name(id);
            let name = id;
            let f = t.field_bg();
            let d = (f.r() as i32 - t.surface.r() as i32).abs()
                + (f.g() as i32 - t.surface.g() as i32).abs()
                + (f.b() as i32 - t.surface.b() as i32).abs();
            assert!(
                d >= 12,
                "{name}: field {f:?} is too close to surface {:?}",
                t.surface
            );
        }
    }

    use super::*;

    #[test]
    fn hex_round_trips_and_env_names_map() {
        assert_eq!(parse_hex("#7200CA"), Some(BRAND_PURPLE));
        assert_eq!(to_hex(BRAND_BLUE), "#0075D3");
        assert_eq!(parse_hex("nope"), None);
        let t = Theme::dark();
        assert_eq!(t.env_colour("production", None), t.env_prod);
        assert_eq!(t.env_colour("staging", None), t.env_staging);
        assert_eq!(t.env_colour("anything", Some("#0075D3")), BRAND_BLUE);
    }

    /// FEAT-0105: every listed id resolves to its own scheme, not the
    /// HyperStack Dark fallback.
    #[test]
    fn every_scheme_id_resolves_to_itself() {
        for (id, name, dark) in Theme::schemes() {
            let t = Theme::by_name(id);
            assert_eq!(t.name, *name, "{id}");
            assert_eq!(t.dark, *dark, "{id}");
        }
    }

    /// WCAG 2 relative luminance.
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// FEAT-0105: text stays readable (WCAG AA) on every background of
    /// every scheme; muted text and the accent keep 3:1 on the window.
    #[test]
    fn every_scheme_keeps_readable_contrast() {
        for (id, _, _) in Theme::schemes() {
            let t = Theme::by_name(id);
            for (what, bg) in [
                ("window", t.window_bg),
                ("panel", t.panel_bg),
                ("surface", t.surface),
            ] {
                let r = contrast(t.text, bg);
                assert!(r >= 4.5, "{id}: text on {what} is {r:.2}:1");
            }
            let r = contrast(t.text_muted, t.window_bg);
            assert!(r >= 3.0, "{id}: muted text on window is {r:.2}:1");
            let r = contrast(t.accent, t.window_bg);
            assert!(r >= 3.0, "{id}: accent on window is {r:.2}:1");
        }
    }
}
