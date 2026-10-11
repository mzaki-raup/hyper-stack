//! Rebindable keyboard shortcuts. Stored as `shortcuts.toml`: `action = "Ctrl+Shift+P"`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl KeyCombo {
    /// Parse "Ctrl+Shift+P", "Alt+F4", "F1", "Ctrl+`".
    pub fn parse(s: &str) -> Option<Self> {
        let mut c = Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: egui::Key::Escape,
        };
        let mut key_set = false;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                other => {
                    c.key = key_from_name(other)?;
                    key_set = true;
                }
            }
        }
        key_set.then_some(c)
    }

    pub fn matches(&self, input: &egui::InputState) -> bool {
        let m = input.modifiers;
        m.command == self.ctrl
            && m.shift == self.shift
            && m.alt == self.alt
            && input.key_pressed(self.key)
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }
}

fn key_from_name(name: &str) -> Option<egui::Key> {
    use egui::Key::*;
    Some(match name {
        "a" => A,
        "b" => B,
        "c" => C,
        "d" => D,
        "e" => E,
        "f" => F,
        "g" => G,
        "h" => H,
        "i" => I,
        "j" => J,
        "k" => K,
        "l" => L,
        "m" => M,
        "n" => N,
        "o" => O,
        "p" => P,
        "q" => Q,
        "r" => R,
        "s" => S,
        "t" => T,
        "u" => U,
        "v" => V,
        "w" => W,
        "x" => X,
        "y" => Y,
        "z" => Z,
        "0" => Num0,
        "1" => Num1,
        "2" => Num2,
        "3" => Num3,
        "4" => Num4,
        "5" => Num5,
        "6" => Num6,
        "7" => Num7,
        "8" => Num8,
        "9" => Num9,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        "escape" | "esc" => Escape,
        "tab" => Tab,
        "enter" | "return" => Enter,
        "space" => Space,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "insert" => Insert,
        "home" => Home,
        "end" => End,
        "pageup" | "pgup" => PageUp,
        "pagedown" | "pgdn" => PageDown,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "`" | "backtick" | "grave" => Backtick,
        "," | "comma" => Comma,
        "." | "period" => Period,
        "/" | "slash" => Slash,
        "\\" | "backslash" => Backslash,
        "-" | "minus" => Minus,
        "=" | "equals" | "plus" => Equals,
        "[" => OpenBracket,
        "]" => CloseBracket,
        ";" | "semicolon" => Semicolon,
        "'" | "quote" => Quote,
        _ => return None,
    })
}

fn key_name(key: egui::Key) -> String {
    use egui::Key::*;
    match key {
        Backtick => "`".into(),
        Comma => ",".into(),
        Period => ".".into(),
        Slash => "/".into(),
        Backslash => "\\".into(),
        Minus => "-".into(),
        Equals => "=".into(),
        OpenBracket => "[".into(),
        CloseBracket => "]".into(),
        Semicolon => ";".into(),
        Quote => "'".into(),
        ArrowUp => "Up".into(),
        ArrowDown => "Down".into(),
        ArrowLeft => "Left".into(),
        ArrowRight => "Right".into(),
        other => {
            let s = format!("{other:?}");
            s.strip_prefix("Num").map(str::to_string).unwrap_or(s)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeymapFile {
    pub schema: u32,
    pub bindings: BTreeMap<String, String>,
}

/// Action id → combo, with defaults (docs/04 §7) overridden by the user file.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: BTreeMap<String, KeyCombo>,
    overrides: BTreeMap<String, String>,
}

pub const DEFAULTS: &[(&str, &str)] = &[
    ("tab.new", "Ctrl+T"),
    ("tab.close", "Ctrl+W"),
    ("tab.reopen", "Ctrl+Shift+T"),
    ("tab.next", "Ctrl+PageDown"),
    ("tab.prev", "Ctrl+PageUp"),
    ("tab.split_right", "Ctrl+\\"),
    ("tab.split_down", "Ctrl+Shift+\\"),
    ("palette.commands", "Ctrl+Shift+P"),
    ("palette.quick_open", "Ctrl+P"),
    ("view.sidebar", "Ctrl+B"),
    ("view.dock", "Ctrl+J"),
    ("view.terminal", "Ctrl+`"),
    ("terminal.new", "Ctrl+Shift+`"),
    ("terminal.find", "Ctrl+Shift+F"),
    ("env.switch", "Ctrl+Shift+E"),
    ("app.settings", "Ctrl+,"),
    ("app.shortcuts", "F1"),
    ("file.save", "Ctrl+S"),
    ("file.save_as", "Ctrl+Shift+S"),
    ("file.open", "Ctrl+O"),
    ("file.new", "Ctrl+N"),
    ("editor.preview", "Ctrl+Shift+M"),
    ("run", "Ctrl+Enter"),
    ("run.alt", "F5"),
    ("vault.lock", "Ctrl+Shift+L"),
    ("zoom.in", "Ctrl+="),
    ("zoom.out", "Ctrl+-"),
];

impl Keymap {
    pub fn load(path: &Path) -> Self {
        let mut km = Self::default();
        for (action, combo) in DEFAULTS {
            if let Some(c) = KeyCombo::parse(combo) {
                km.bindings.insert((*action).to_string(), c);
            }
        }
        if path.exists()
            && let Ok(file) = storage::read_toml::<KeymapFile>(path)
        {
            for (action, combo) in file.bindings {
                if let Some(c) = KeyCombo::parse(&combo) {
                    km.bindings.insert(action.clone(), c);
                    km.overrides.insert(action, combo);
                }
            }
        }
        km
    }

    pub fn save(&self, path: &Path) -> storage::Result<()> {
        storage::write_toml(
            path,
            &KeymapFile {
                schema: 1,
                bindings: self.overrides.clone(),
            },
        )
    }

    pub fn get(&self, action: &str) -> Option<KeyCombo> {
        self.bindings.get(action).copied()
    }

    pub fn set(&mut self, action: &str, combo: &str) -> bool {
        match KeyCombo::parse(combo) {
            Some(c) => {
                self.bindings.insert(action.to_string(), c);
                self.overrides.insert(action.to_string(), combo.to_string());
                true
            }
            None => false,
        }
    }

    pub fn all(&self) -> impl Iterator<Item = (&String, &KeyCombo)> {
        self.bindings.iter()
    }

    /// Actions bound to the same combo (shown on the shortcuts page).
    pub fn conflicts(&self) -> Vec<(KeyCombo, Vec<String>)> {
        let mut by_combo: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (a, c) in &self.bindings {
            by_combo.entry(c.display()).or_default().push(a.clone());
        }
        by_combo
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|v| (self.bindings[&v[0]], v))
            .collect()
    }

    /// Which action (if any) the current frame's input triggers.
    pub fn triggered(&self, input: &egui::InputState) -> Option<&str> {
        self.bindings
            .iter()
            .find(|(_, c)| c.matches(input))
            .map(|(a, _)| a.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_parse_and_display() {
        let c = KeyCombo::parse("Ctrl+Shift+P").unwrap();
        assert!(c.ctrl && c.shift && !c.alt);
        assert_eq!(c.key, egui::Key::P);
        assert_eq!(c.display(), "Ctrl+Shift+P");
        assert_eq!(KeyCombo::parse("Ctrl+`").unwrap().display(), "Ctrl+`");
        assert_eq!(KeyCombo::parse("F1").unwrap().display(), "F1");
        assert_eq!(KeyCombo::parse("Ctrl+1").unwrap().display(), "Ctrl+1");
        assert!(KeyCombo::parse("Ctrl+").is_none());
        assert!(KeyCombo::parse("Ctrl+Bogus").is_none());
    }

    #[test]
    fn user_overrides_persist_and_conflicts_are_found() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("shortcuts.toml");
        let mut km = Keymap::load(&p);
        assert_eq!(
            km.get("palette.commands").unwrap().display(),
            "Ctrl+Shift+P"
        );
        assert!(km.set("palette.commands", "Ctrl+K"));
        assert!(km.set("tab.new", "Ctrl+K"));
        km.save(&p).unwrap();
        let km2 = Keymap::load(&p);
        assert_eq!(km2.get("palette.commands").unwrap().display(), "Ctrl+K");
        let conflicts = km2.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1.len(), 2);
    }
}
