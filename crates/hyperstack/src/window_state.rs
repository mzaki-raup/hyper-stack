//! Window geometry persistence (docs/04 §1): size, position and maximised
//! state are saved to `HyperStackData/window.toml` while the app runs and
//! restored on the next start. Sidebar width and dock height live in the
//! settings (Appearance) and are captured by the shell the same way.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WindowState {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl WindowState {
    pub const DEFAULT_SIZE: [f32; 2] = [1360.0, 860.0];
    pub const MIN_SIZE: [f32; 2] = [800.0, 500.0];

    pub fn load(path: &Path) -> Self {
        let st: Self = hs_core::storage::read_toml(path).unwrap_or_default();
        st.sanitised()
    }

    pub fn save(&self, path: &Path) {
        if let Err(e) = hs_core::storage::write_toml(path, self) {
            tracing::warn!("window state not saved: {e}");
        }
    }

    /// Ignore nonsense from a corrupt file or a monitor that is gone.
    fn sanitised(mut self) -> Self {
        if !(self.width >= Self::MIN_SIZE[0] && self.width <= 10_000.0) {
            self.width = Self::DEFAULT_SIZE[0];
        }
        if !(self.height >= Self::MIN_SIZE[1] && self.height <= 10_000.0) {
            self.height = Self::DEFAULT_SIZE[1];
        }
        if let (Some(x), Some(y)) = (self.x, self.y)
            && !(x > -10_000.0 && x < 20_000.0 && y > -10_000.0 && y < 20_000.0)
        {
            self.x = None;
            self.y = None;
        }
        self
    }

    /// Build the startup viewport from the saved state.
    pub fn viewport(&self) -> egui::ViewportBuilder {
        let mut v = egui::ViewportBuilder::default()
            .with_inner_size([self.width, self.height])
            .with_min_inner_size(Self::MIN_SIZE)
            .with_maximized(self.maximized);
        if let (Some(x), Some(y)) = (self.x, self.y) {
            v = v.with_position([x, y]);
        }
        v
    }

    /// Read the live geometry from egui's viewport info (points).
    pub fn from_viewport(info: &egui::ViewportInfo) -> Option<Self> {
        let inner = info.inner_rect?;
        let maximized = info.maximized.unwrap_or(false);
        let (x, y) = info
            .outer_rect
            .map(|r| (Some(r.min.x), Some(r.min.y)))
            .unwrap_or((None, None));
        Some(Self {
            x,
            y,
            width: inner.width(),
            height: inner.height(),
            maximized,
        })
    }
}

/// A window size the user can pick in Settings → General → Startup, as a
/// share of the screen the window is on (owner's request, 2026-10-23).
/// **Full** maximises rather than sizing, so the taskbar is left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePreset {
    Full,
    ThreeQuarters,
    TwoThirds,
    Half,
    Default,
}

impl SizePreset {
    pub const ALL: [Self; 5] = [
        Self::Full,
        Self::ThreeQuarters,
        Self::TwoThirds,
        Self::Half,
        Self::Default,
    ];

    /// The Fluent key for this preset's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Full => "settings-window-full",
            Self::ThreeQuarters => "settings-window-three-quarters",
            Self::TwoThirds => "settings-window-two-thirds",
            Self::Half => "settings-window-half",
            Self::Default => "settings-window-default",
        }
    }

    /// The share of each screen axis this preset asks for, if any.
    fn share(self) -> Option<f32> {
        match self {
            Self::Full => Some(1.0),
            Self::ThreeQuarters => Some(0.75),
            Self::TwoThirds => Some(2.0 / 3.0),
            Self::Half => Some(0.5),
            Self::Default => None,
        }
    }

    /// The inner size this preset means on a screen of `monitor`, never
    /// below the minimum the window enforces.
    pub fn size(self, monitor: egui::Vec2) -> egui::Vec2 {
        let want = match self.share() {
            Some(f) => egui::vec2(monitor.x * f, monitor.y * f),
            None => egui::Vec2::from(WindowState::DEFAULT_SIZE),
        };
        egui::vec2(
            want.x.max(WindowState::MIN_SIZE[0]),
            want.y.max(WindowState::MIN_SIZE[1]),
        )
    }

    /// Which preset `size` is, so the picker can show it rather than
    /// Custom. A maximised window is always Full.
    pub fn matching(size: egui::Vec2, monitor: egui::Vec2, maximized: bool) -> Option<Self> {
        if maximized {
            return Some(Self::Full);
        }
        Self::ALL.into_iter().skip(1).find(|p| {
            let want = p.size(monitor);
            (want.x - size.x).abs() <= 2.0 && (want.y - size.y).abs() <= 2.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let st = WindowState {
            x: Some(-50_000.0),
            y: Some(10.0),
            width: 10.0,
            height: 5.0,
            maximized: false,
        }
        .sanitised();
        assert_eq!(st.width, WindowState::DEFAULT_SIZE[0]);
        assert_eq!(st.height, WindowState::DEFAULT_SIZE[1]);
        assert!(st.x.is_none() && st.y.is_none());
    }

    #[test]
    fn a_preset_is_a_share_of_the_screen_and_never_below_the_minimum() {
        let monitor = egui::vec2(1920.0, 1080.0);
        assert_eq!(SizePreset::Half.size(monitor), egui::vec2(960.0, 540.0));
        assert_eq!(
            SizePreset::ThreeQuarters.size(monitor),
            egui::vec2(1440.0, 810.0)
        );
        // Half of a small screen would be under the window's minimum.
        let small = egui::vec2(1280.0, 720.0);
        assert_eq!(
            SizePreset::Half.size(small),
            egui::Vec2::from(WindowState::MIN_SIZE)
        );
    }

    #[test]
    fn the_picker_names_the_size_the_window_already_has() {
        let monitor = egui::vec2(1920.0, 1080.0);
        assert_eq!(
            SizePreset::matching(egui::vec2(960.0, 540.0), monitor, false),
            Some(SizePreset::Half)
        );
        // Maximised is Full however large the inner rectangle reads.
        assert_eq!(
            SizePreset::matching(egui::vec2(1234.0, 567.0), monitor, true),
            Some(SizePreset::Full)
        );
        // Anything else is the user's own size.
        assert_eq!(
            SizePreset::matching(egui::vec2(1111.0, 777.0), monitor, false),
            None
        );
    }
}
