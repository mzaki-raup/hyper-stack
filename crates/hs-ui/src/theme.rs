//! Themes (docs/04 §8). The brand palette comes from the app icon: purple
//! accent, blue secondary, gold reserved for warnings and the beta badge.

use egui::Color32;

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
            name: "Dark".into(),
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
            name: "Light".into(),
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

    pub fn by_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("light") {
            Self::light()
        } else {
            Self::dark()
        }
    }

    /// Push the palette into egui's visuals.
    pub fn apply(&self, ctx: &egui::Context) {
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
        ctx.set_visuals(v);
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
