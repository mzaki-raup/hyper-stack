//! Font families: three bundled monospace Nerd Fonts (see `assets/fonts/`)
//! plus any font installed on the machine, chosen separately for the
//! interface, the terminal and the editor (docs/04 §9, Appearance).
//!
//! Font ids stored in settings: `""` (egui default), a bundled id
//! (`jetbrains-mono-nf`, `fira-code-nf`, `bitstrom-wera-nf`) or
//! `system:<family name>` for an installed font. Installed fonts are scanned
//! once on a background thread; until then only the bundled ones are offered.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

pub const JETBRAINS_MONO: &str = "jetbrains-mono-nf";
pub const FIRA_CODE: &str = "fira-code-nf";
pub const BITSTROM_WERA: &str = "bitstrom-wera-nf";
/// Default for the terminal and the editor.
pub const DEFAULT_MONO: &str = JETBRAINS_MONO;
pub const SYSTEM_PREFIX: &str = "system:";

/// (id, label, bytes) for every bundled font.
pub const EMBEDDED: &[(&str, &str, &[u8])] = &[
    (
        JETBRAINS_MONO,
        "JetBrainsMono Nerd Font Mono",
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf"),
    ),
    (
        FIRA_CODE,
        "Fira Code Nerd Font Mono",
        include_bytes!("../../../assets/fonts/FiraCodeNerdFontMono-Regular.ttf"),
    ),
    (
        BITSTROM_WERA,
        "BitstromWera Nerd Font Mono",
        include_bytes!("../../../assets/fonts/BitstromWeraNerdFontMono-Regular.ttf"),
    ),
];

const TERMINAL_FAMILY: &str = "hs-terminal";
const EDITOR_FAMILY: &str = "hs-editor";

/// The family the terminal draws with (`FontId::new(size, terminal_family())`).
pub fn terminal_family() -> FontFamily {
    FontFamily::Name(TERMINAL_FAMILY.into())
}

/// The family the editor draws with.
pub fn editor_family() -> FontFamily {
    FontFamily::Name(EDITOR_FAMILY.into())
}

/// Which font each surface uses (settings values).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontChoice {
    pub ui: String,
    pub terminal: String,
    pub editor: String,
}

#[derive(Debug, Clone)]
pub struct SystemFont {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
    pub monospace: bool,
}

static SYSTEM: OnceLock<Vec<SystemFont>> = OnceLock::new();
static SCAN_STARTED: OnceLock<()> = OnceLock::new();

/// Start scanning installed fonts on a background thread (idempotent).
pub fn prefetch_system_fonts() {
    if SCAN_STARTED.set(()).is_ok() {
        std::thread::Builder::new()
            .name("hs-font-scan".into())
            .spawn(|| {
                let found = scan_system_fonts();
                let _ = SYSTEM.set(found);
            })
            .ok();
    }
}

/// Installed fonts, or `None` while the scan is still running.
pub fn system_fonts() -> Option<&'static [SystemFont]> {
    SYSTEM.get().map(|v| v.as_slice())
}

/// Human label for a font id.
pub fn label(id: &str, default_label: &str) -> String {
    if id.is_empty() {
        return default_label.to_string();
    }
    if let Some(name) = id.strip_prefix(SYSTEM_PREFIX) {
        return name.to_string();
    }
    EMBEDDED
        .iter()
        .find(|(i, _, _)| *i == id)
        .map(|(_, l, _)| (*l).to_string())
        .unwrap_or_else(|| id.to_string())
}

/// Build and install the egui font definitions for `choice`: egui defaults,
/// Phosphor icons, the bundled fonts and whichever installed fonts are
/// selected. Unknown or unreadable choices fall back silently.
pub fn install(ctx: &egui::Context, choice: &FontChoice) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Fill);
    for (id, _, bytes) in EMBEDDED {
        fonts
            .font_data
            .insert((*id).to_string(), Arc::new(FontData::from_static(bytes)));
    }
    let mut resolve = |id: &str, mono_default: bool| -> Option<String> {
        if id.is_empty() {
            return None;
        }
        if EMBEDDED.iter().any(|(i, _, _)| *i == id) {
            return Some(id.to_string());
        }
        if let Some(family) = id.strip_prefix(SYSTEM_PREFIX) {
            if fonts.font_data.contains_key(id) {
                return Some(id.to_string());
            }
            if let Some(font) =
                system_fonts().and_then(|list| list.iter().find(|f| f.family == family))
                && let Ok(bytes) = std::fs::read(&font.path)
            {
                let mut data = FontData::from_owned(bytes);
                data.index = font.index;
                fonts.font_data.insert(id.to_string(), Arc::new(data));
                return Some(id.to_string());
            }
        }
        mono_default.then(|| DEFAULT_MONO.to_string())
    };
    let ui_font = resolve(&choice.ui, false);
    let terminal_font = resolve(&choice.terminal, true).unwrap_or_else(|| DEFAULT_MONO.to_string());
    let editor_font = resolve(&choice.editor, true).unwrap_or_else(|| DEFAULT_MONO.to_string());

    if let Some(f) = ui_font
        && let Some(list) = fonts.families.get_mut(&FontFamily::Proportional)
    {
        list.insert(0, f);
    }
    // egui's default UI font has no arrows (→ ↑ ↓) or box drawing; the
    // bundled Nerd Font has them, so it is the last glyph fallback.
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional)
        && !list.iter().any(|f| f == DEFAULT_MONO)
    {
        list.push(DEFAULT_MONO.to_string());
    }
    let mono_fallback = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    // Monospace (markdown code, status rows) follows the editor font.
    if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
        list.insert(0, editor_font.clone());
    }
    let mut term_list = vec![terminal_font];
    term_list.extend(mono_fallback.iter().cloned());
    fonts.families.insert(terminal_family(), term_list);
    let mut editor_list = vec![editor_font];
    editor_list.extend(mono_fallback);
    fonts.families.insert(editor_family(), editor_list);
    ctx.set_fonts(fonts);
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(w) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(w).join("Fonts"));
        }
        if let Ok(l) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(l)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Ok(h) = std::env::var("HOME") {
            dirs.push(PathBuf::from(h).join("Library").join("Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(h) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&h).join(".fonts"));
            dirs.push(PathBuf::from(&h).join(".local").join("share").join("fonts"));
        }
    }
    dirs
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                walk(&p, out, depth + 1);
            }
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
        {
            out.push(p);
        }
    }
}

/// Scan the OS font folders; one entry per family, preferring the regular
/// upright face. Collections contribute their first face only.
pub fn scan_system_fonts() -> Vec<SystemFont> {
    let mut files = Vec::new();
    for d in font_dirs() {
        walk(&d, &mut files, 0);
    }
    files.sort();
    files.dedup();
    let mut by_family: std::collections::BTreeMap<String, (SystemFont, u8)> =
        std::collections::BTreeMap::new();
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(face) = ttf_parser::Face::parse(&bytes, 0) else {
            continue;
        };
        let names = face.names();
        let pick = |id: u16| -> Option<String> {
            names
                .into_iter()
                .filter(|n| n.name_id == id && n.is_unicode())
                .find_map(|n| n.to_string())
        };
        let Some(family) = pick(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| pick(ttf_parser::name_id::FAMILY))
            .filter(|f| !f.is_empty())
        else {
            continue;
        };
        // Rank: regular upright first, then other uprights, then italics.
        let regular = face.weight() == ttf_parser::Weight::Normal
            && !face.is_italic()
            && face.width() == ttf_parser::Width::Normal;
        let rank: u8 = if regular {
            0
        } else if !face.is_italic() {
            1
        } else {
            2
        };
        let font = SystemFont {
            family: family.clone(),
            path: path.clone(),
            index: 0,
            monospace: face.is_monospaced(),
        };
        match by_family.get(&family) {
            Some((_, r)) if *r <= rank => {}
            _ => {
                by_family.insert(family, (font, rank));
            }
        }
    }
    by_family.into_values().map(|(f, _)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_parse_and_are_monospace() {
        for (id, label, bytes) in EMBEDDED {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap_or_else(|_| panic!("{id} parses"));
            assert!(face.is_monospaced(), "{label} is monospace");
        }
        assert_eq!(label(JETBRAINS_MONO, "x"), "JetBrainsMono Nerd Font Mono");
        assert_eq!(label("system:Anthropic Sans", "x"), "Anthropic Sans");
        assert_eq!(label("", "Default"), "Default");
    }

    #[test]
    fn install_builds_named_families_with_fallbacks() {
        let ctx = egui::Context::default();
        let choice = FontChoice {
            ui: "system:Definitely Not Installed".into(),
            terminal: FIRA_CODE.into(),
            editor: "nonsense".into(),
        };
        install(&ctx, &choice);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.fonts_mut(|f| {
                assert!(f.row_height(&egui::FontId::new(13.0, terminal_family())) > 0.0);
                assert!(f.row_height(&egui::FontId::new(13.0, editor_family())) > 0.0);
            });
        });
    }
}
