//! A lettered tile (CR-0082): one or two initials in white on a rounded
//! square in the name's colour from [`crate::palette8`]. What a workspace
//! without a logo wears, the same in every place it is drawn.

use crate::palette8;

/// The one or two letters a tile shows for `name`. The name splits into
/// words on whitespace, `-`, `_`, `.` and a lower-to-upper case change;
/// two or more words give the first letters of the first two, one word its
/// first letter, nothing at all `?`.
pub fn initials(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_whitespace() || matches!(c, '-' | '_' | '.') {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        prev_lower = c.is_lowercase();
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current);
    }
    let letters: String = words
        .iter()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "?".to_owned()
    } else {
        letters
    }
}

/// Paints `name`'s tile filling `rect`.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, name: &str) {
    let size = rect.width().min(rect.height());
    painter.rect_filled(rect, size * 0.22, palette8::by_name(name));
    let letters = initials(name);
    let scale = if letters.chars().count() > 1 {
        0.46
    } else {
        0.58
    };
    let font = egui::FontId::proportional(size * scale);
    // No bold face is bundled: the letters are drawn twice, a hair apart.
    let nudge = egui::vec2((size / 40.0).max(0.5), 0.0);
    for at in [rect.center() - nudge / 2.0, rect.center() + nudge / 2.0] {
        painter.text(
            at,
            egui::Align2::CENTER_CENTER,
            &letters,
            font.clone(),
            egui::Color32::WHITE,
        );
    }
}

/// A `size` square tile for `name`, laid out in `ui`.
pub fn tile(ui: &mut egui::Ui, name: &str, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, name);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_two_words() {
        assert_eq!(initials("default"), "D");
        assert_eq!(initials("hyper-stack"), "HS");
        assert_eq!(initials("My Work Space"), "MW");
        assert_eq!(initials("HyperStack"), "HS");
        assert_eq!(initials("x"), "X");
        assert_eq!(initials("snake_case.name"), "SC");
        assert_eq!(initials(""), "?");
        assert_eq!(initials(" - _ "), "?");
    }
}
