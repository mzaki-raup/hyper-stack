//! Text measurement and wrapping shared by layout and export. The UI passes a
//! real font measurement callback; tests and the headless generator use
//! [`approx_measure`], which is deterministic.

use crate::model::Point;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
        }
    }

    pub fn inflate(&self, by: f32) -> Self {
        Self::new(
            self.x - by,
            self.y - by,
            self.w + by * 2.0,
            self.h + by * 2.0,
        )
    }

    pub fn union(&self, other: &Rect) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
}

pub type Measure<'a> = &'a dyn Fn(&str) -> Size;

/// A deterministic approximation: 7.4 px per character at 14 px, 18 px per
/// line. Good enough for tests and for the generator's first layout.
pub fn approx_measure(text: &str) -> Size {
    let mut w: f32 = 8.0;
    let mut lines = 0usize;
    for line in text.lines() {
        lines += 1;
        w = w.max(line.chars().count() as f32 * 7.4);
    }
    Size::new(w, lines.max(1) as f32 * 18.0)
}

/// Wrap `text` into lines no wider than `max_width`, preserving explicit
/// newlines. Long words are split by character so a label can never overflow
/// the node.
pub fn wrap(text: &str, max_width: f32, measure: Measure<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if measure(&candidate).w <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            if measure(word).w <= max_width {
                line = word.to_string();
                continue;
            }
            // A single word that does not fit: split it by character.
            let mut chunk = String::new();
            for ch in word.chars() {
                let next = format!("{chunk}{ch}");
                if measure(&next).w > max_width && !chunk.is_empty() {
                    out.push(std::mem::take(&mut chunk));
                }
                chunk.push(ch);
            }
            line = chunk;
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Wrap and return both the lines and their measured size.
pub fn label_lines(text: &str, max_width: f32, measure: Measure<'_>) -> (String, Size) {
    let lines = wrap(text, max_width, measure);
    let mut w: f32 = 0.0;
    for line in &lines {
        w = w.max(measure(line).w);
    }
    let h = lines.len() as f32 * 18.0;
    (lines.join("\n"), Size::new(w.max(8.0), h.max(18.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_measure_counts_lines() {
        assert_eq!(approx_measure("one\ntwo").h, 36.0);
        assert!(approx_measure("hello").w > 20.0);
    }

    #[test]
    fn wrap_splits_long_labels() {
        let lines = wrap(
            "a very long label that needs wrapping",
            80.0,
            &approx_measure,
        );
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(approx_measure(line).w <= 80.0, "{line:?}");
        }
    }

    #[test]
    fn wrap_keeps_explicit_lines() {
        let lines = wrap("one\n\ntwo", 200.0, &approx_measure);
        assert_eq!(lines, vec!["one", "", "two"]);
    }
}
