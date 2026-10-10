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
}
