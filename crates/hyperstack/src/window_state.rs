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
    /// The window opens at its minimum, which is also the smallest layout
    /// every tool is designed for (owner's rule, 2026-10-23).
    pub const DEFAULT_SIZE: [f32; 2] = [1720.0, 860.0];
    /// Strict: the viewport enforces it, Settings cannot ask for less, and a
    /// saved geometry below it is replaced rather than clamped.
    pub const MIN_SIZE: [f32; 2] = [1720.0, 860.0];

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

/// What the Size picker in Settings → General → Startup offers (owner's
/// request, 2026-10-23, narrowed to these two the same day). **Full**
/// maximises rather than sizing to the monitor, so the taskbar is left
/// alone; **Custom** is the window at the width and height beside it, and
/// picking it restores a maximised window to that size. Shares of the
/// screen were offered briefly and taken out again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePreset {
    Full,
    Custom,
}

impl SizePreset {
    pub const ALL: [Self; 2] = [Self::Full, Self::Custom];

    /// The Fluent key for this entry's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Full => "settings-window-full",
            Self::Custom => "settings-window-custom",
        }
    }

    /// Which entry names the window as it is now.
    pub fn of(maximized: bool) -> Self {
        if maximized { Self::Full } else { Self::Custom }
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
    fn a_window_saved_under_the_minimum_opens_at_the_minimum() {
        // A geometry written before the minimum rose, or by hand.
        let st = WindowState {
            x: Some(20.0),
            y: Some(20.0),
            width: 1360.0,
            height: 700.0,
            maximized: false,
        }
        .sanitised();
        assert_eq!(st.width, 1720.0);
        assert_eq!(st.height, 860.0);
        // The viewport is told the same floor, so the user cannot drag under it.
        let v = st.viewport();
        assert_eq!(v.min_inner_size, Some(egui::vec2(1720.0, 860.0)));
    }

    #[test]
    fn the_picker_names_the_window_as_it_is() {
        assert_eq!(SizePreset::of(true), SizePreset::Full);
        assert_eq!(SizePreset::of(false), SizePreset::Custom);
        // Only those two are offered.
        assert_eq!(SizePreset::ALL.len(), 2);
    }
}
