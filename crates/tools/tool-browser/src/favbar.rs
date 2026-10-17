//! The favourites bar: a row of the workspace's favourites under the
//! address bar, the way Chrome's bookmarks bar sits under its omnibox.
//! Settings → Browser shows or hides it. Owner's requirement, 2026-10-18.
//!
//! Chrome's rules are followed because that is what people's hands know:
//! a click opens the site in the current page, a middle click or a
//! Ctrl+click opens it in a new page without leaving this one, a folder is
//! a menu (with sub-folders as sub-menus), what does not fit goes behind a
//! » at the right, and the right-click menu offers Open in New Page, Edit
//! and Remove.

use std::collections::HashMap;

use hs_core::Services;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

use crate::store::{FavItem, Favorites};

/// The widest a chip may be before its name is cut, as in Chrome.
const CHIP_MAX: f32 = 140.0;
/// Room kept for the overflow button.
const OVERFLOW_WIDTH: f32 = 28.0;

/// What the bar asked for this frame.
#[derive(Default)]
pub struct BarOut {
    /// Navigate the current page here.
    pub go: Option<String>,
    /// Open this URL in a new page, without switching to it.
    pub open_background: Option<String>,
    /// Edit this favourite in the sidebar.
    pub edit: Option<String>,
    /// Remove this favourite (a folder goes with its contents).
    pub remove: Option<String>,
}

/// Draw the bar. `icons` maps a page URL to its favicon URI.
pub fn show(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    favorites: &Favorites,
    icons: &HashMap<String, Option<String>>,
) -> BarOut {
    let mut out = BarOut::default();
    let items = &favorites.items;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            if items.is_empty() {
                ui.label(
                    egui::RichText::new(s.t("browser-favbar-empty"))
                        .small()
                        .color(theme.text_muted),
                );
                return;
            }
            ui.horizontal(|ui| {
                // Room between chips and inside them, so neighbours do not
                // read as one word. Owner's request, 2026-10-18.
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
                let total = ui.available_width();
                let mut shown = 0;
                for it in items.iter() {
                    // Stop before the chip that would push past the right
                    // edge; the rest go behind the overflow button.
                    let left = total - (ui.min_rect().width());
                    if left < chip_width(ui, it) + OVERFLOW_WIDTH && shown + 1 < items.len() {
                        break;
                    }
                    chip(ui, s, theme, it, icons, &mut out);
                    shown += 1;
                }
                if shown < items.len() {
                    ui.menu_button(ph::CARET_DOUBLE_RIGHT, |ui| {
                        for it in &items[shown..] {
                            menu_item(ui, s, theme, it, icons, &mut out);
                        }
                    })
                    .response
                    .tip(s.t("browser-favbar-more"));
                }
            });
        });
    out
}

/// How wide a chip will be, so the row knows whether it fits.
fn chip_width(ui: &egui::Ui, it: &FavItem) -> f32 {
    let text = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            it.name().to_string(),
            egui::FontId::proportional(12.5),
            egui::Color32::WHITE,
        )
        .size()
        .x
    });
    (text + 16.0 + 16.0 + 10.0).min(CHIP_MAX + 26.0)
}

/// One chip: a site, or a folder that opens as a menu.
fn chip(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    it: &FavItem,
    icons: &HashMap<String, Option<String>>,
    out: &mut BarOut,
) {
    match it {
        FavItem::Folder(f) => {
            let r = ui
                .menu_button(
                    egui::RichText::new(format!("{} {}", ph::FOLDER, cut(&f.name))).size(12.5),
                    |ui| {
                        if f.items.is_empty() {
                            ui.label(
                                egui::RichText::new(s.t("browser-favbar-folder-empty"))
                                    .color(theme.text_muted),
                            );
                        }
                        for it in &f.items {
                            menu_item(ui, s, theme, it, icons, out);
                        }
                    },
                )
                .response;
            // Not `r.context_menu`: a menu button and a context menu on the
            // same response share one popup id (`response.id.with("popup")`),
            // so the context menu's "not open" state closed the folder's
            // menu the frame it opened. A separate id keeps them apart.
            egui::Popup::context_menu(&r)
                .id(r.id.with("favbar-folder-ctx"))
                .show(|ui| {
                    if ui.button(s.t("browser-rename")).clicked() {
                        out.edit = Some(f.id.clone());
                        ui.close();
                    }
                    if ui.button(s.t("browser-favbar-remove")).clicked() {
                        out.remove = Some(f.id.clone());
                        ui.close();
                    }
                });
        }
        FavItem::Link(l) => {
            let icon = crate::side::site_icon(&l.icon, &l.url, icons);
            let text = egui::RichText::new(cut(&l.name)).size(12.5);
            let button = match icon {
                Some(uri) => egui::Button::image_and_text(
                    egui::Image::from_uri(uri).fit_to_exact_size(egui::Vec2::splat(14.0)),
                    text,
                ),
                None => egui::Button::new(format!("{} {}", ph::GLOBE, cut(&l.name))),
            };
            let r = ui.add(button.frame(false)).tip(&l.url);
            let modifier = ui.input(|i| i.modifiers.command);
            if r.middle_clicked() || (r.clicked() && modifier) {
                out.open_background = Some(l.url.clone());
            } else if r.clicked() {
                out.go = Some(l.url.clone());
            }
            r.context_menu(|ui| {
                if ui.button(s.t("browser-favbar-open-new")).clicked() {
                    out.open_background = Some(l.url.clone());
                    ui.close();
                }
                if ui.button(s.t("browser-favbar-edit")).clicked() {
                    out.edit = Some(l.id.clone());
                    ui.close();
                }
                ui.separator();
                if ui.button(s.t("browser-favbar-remove")).clicked() {
                    out.remove = Some(l.id.clone());
                    ui.close();
                }
            });
        }
    }
}

/// An entry of a folder menu: a site, or a sub-folder as a sub-menu.
fn menu_item(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    it: &FavItem,
    icons: &HashMap<String, Option<String>>,
    out: &mut BarOut,
) {
    match it {
        FavItem::Folder(f) => {
            ui.menu_button(format!("{} {}", ph::FOLDER, f.name), |ui| {
                if f.items.is_empty() {
                    ui.label(
                        egui::RichText::new(s.t("browser-favbar-folder-empty"))
                            .color(theme.text_muted),
                    );
                }
                for it in &f.items {
                    menu_item(ui, s, theme, it, icons, out);
                }
            });
        }
        FavItem::Link(l) => {
            let icon = crate::side::site_icon(&l.icon, &l.url, icons);
            let button = match icon {
                Some(uri) => egui::Button::image_and_text(
                    egui::Image::from_uri(uri).fit_to_exact_size(egui::Vec2::splat(14.0)),
                    &l.name,
                ),
                None => egui::Button::new(format!("{} {}", ph::GLOBE, l.name)),
            };
            let r = ui.add(button).tip(&l.url);
            let modifier = ui.input(|i| i.modifiers.command);
            if r.middle_clicked() || (r.clicked() && modifier) {
                out.open_background = Some(l.url.clone());
                ui.close();
            } else if r.clicked() {
                out.go = Some(l.url.clone());
                ui.close();
            }
        }
    }
}

/// Chrome cuts long names rather than letting one chip take the bar.
fn cut(name: &str) -> String {
    const MAX: usize = 18;
    if name.chars().count() <= MAX {
        name.to_string()
    } else {
        let short: String = name.chars().take(MAX - 1).collect();
        format!("{short}…")
    }
}

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        assert_eq!(cut("GitHub"), "GitHub");
        assert_eq!(
            cut("A very long favourite name indeed"),
            "A very long favou…"
        );
        assert_eq!(
            cut("日本語の長いお気に入りの名前です本当に"),
            "日本語の長いお気に入りの名前です本…"
        );
    }
}
