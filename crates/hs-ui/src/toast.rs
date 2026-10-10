//! Toasts with a history (docs/02 §1 "Notifications").

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, Vec2};
use hs_core::tool::ToastLevel;

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    pub at: Instant,
}

#[derive(Default)]
pub struct Toasts {
    live: Vec<Toast>,
    pub history: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, level: ToastLevel, text: impl Into<String>) {
        let t = Toast {
            level,
            text: text.into(),
            at: Instant::now(),
        };
        self.history.push(t.clone());
        if self.history.len() > 200 {
            self.history.remove(0);
        }
        self.live.push(t);
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        let ttl = |l: ToastLevel| match l {
            ToastLevel::Error => Duration::from_secs(8),
            ToastLevel::Warning => Duration::from_secs(6),
            _ => Duration::from_secs(4),
        };
        self.live.retain(|t| t.at.elapsed() < ttl(t.level));
        if self.live.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        egui::Area::new(egui::Id::new("hs-toasts"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -40.0))
            .interactable(false)
            .show(ctx, |ui| {
                for t in self.live.iter().rev() {
                    let (icon, colour) = match t.level {
                        ToastLevel::Info => (crate::icons::INFO, theme.secondary),
                        ToastLevel::Success => (crate::icons::CHECK, theme.success),
                        ToastLevel::Warning => (crate::icons::WARNING, theme.warning),
                        ToastLevel::Error => (crate::icons::ERROR, theme.error),
                    };
                    egui::Frame::new()
                        .fill(theme.surface)
                        .stroke(egui::Stroke::new(1.0, colour))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 2],
                            blur: 8,
                            spread: 0,
                            color: Color32::from_black_alpha(70),
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(colour, icon);
                                ui.label(&t.text);
                            });
                        });
                    ui.add_space(6.0);
                }
            });
    }
}
