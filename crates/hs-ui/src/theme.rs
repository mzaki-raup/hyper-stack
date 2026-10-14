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
}

impl Theme {
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
        }
    }

    /// The theme the Appearance settings select right now (see `scheme_id`).
    pub fn from_settings(app: &Appearance) -> Self {
        Self::by_name(scheme_id(app))
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
            _ => Self::dark(),
        }
    }

    /// Every bundled colour scheme: (settings id, display name, dark).
    pub fn schemes() -> &'static [(&'static str, &'static str, bool)] {
        &[
            ("dark", "HyperStack Dark", true),
            ("light", "HyperStack Light", false),
            ("dracula", "Dracula", true),
            ("nord", "Nord", true),
            ("one-dark", "One Dark", true),
            ("monokai", "Monokai", true),
            ("gruvbox-dark", "Gruvbox Dark", true),
            ("catppuccin-mocha", "Catppuccin Mocha", true),
            ("solarized-dark", "Solarized Dark", true),
            ("solarized-light", "Solarized Light", false),
            ("github-light", "GitHub Light", false),
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

    /// egui visuals for this palette.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.window_fill = self.panel_bg;
        v.panel_fill = self.panel_bg;
        v.extreme_bg_color = self.window_bg;
        v.faint_bg_color = self.surface;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.secondary;
        v.selection.bg_fill = self.accent.linear_multiply(0.35);
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.noninteractive.bg_fill = self.panel_bg;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        v.widgets.inactive.bg_fill = self.surface;
        v.widgets.inactive.weak_bg_fill = self.surface;
        v.widgets.hovered.bg_fill = self.surface_hover;
        v.widgets.hovered.weak_bg_fill = self.surface_hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.active.bg_fill = self.accent.linear_multiply(0.5);
        v.widgets.active.weak_bg_fill = self.accent.linear_multiply(0.5);
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

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

#[cfg(test)]
mod tests {
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
}
