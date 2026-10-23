//! Saved investigations (docs/tools/logs.md §15.7).
//!
//! A source and a filter preset persist separately, and neither is what
//! is worth reopening after an incident. What is worth reopening is the
//! whole set-up: the source, the chips that narrowed it, the highlight
//! rules, the pinned columns, the bookmarks *with their notes*, the time
//! zone and whether Follow was on.
//!
//! They live in `logs/views/<slug>.toml` beside the sources, and the
//! slug is derived from the name exactly as `SavedSource::file_for`
//! does - a fixture written under any other file name is duplicated the
//! first time anything saves it (see the connection-profile gotcha in
//! CLAUDE.md).

use std::path::{Path, PathBuf};

use hs_core::storage;
use serde::{Deserialize, Serialize};

use crate::filter::{Chip, Highlight};
use crate::parse::Format;
use crate::source::SourceSpec;

/// Bumped when a field's meaning changes, never for an addition: a
/// reader refuses a file from the future rather than guessing.
pub const SCHEMA: u32 = 1;

/// The folder investigations live in, under the workspace's `logs/`.
pub fn dir(logs_dir: &Path) -> PathBuf {
    logs_dir.join("views")
}

/// A whole Logs tab, saved so it can be reopened as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Investigation {
    pub schema: u32,
    pub name: String,
    /// What was being read. Kept by value rather than by reference to a
    /// saved source: an investigation has to survive that source being
    /// renamed or deleted, which is exactly what happens while someone
    /// tidies up after an incident.
    pub spec: SourceSpec,
    pub format: Format,
    pub tail_lines: u32,
    pub chips: Vec<Chip>,
    pub highlights: Vec<Highlight>,
    pub pinned: Vec<String>,
    /// Bookmarks as `(line number, note)`: the notes are the point of
    /// saving an investigation at all.
    pub bookmarks: Vec<(u64, String)>,
    pub local_time: bool,
    pub follow: bool,
    pub mask: bool,
    pub collapse: bool,
    pub chart_open: bool,
    /// Where this sits in its sidebar section.
    pub order: u32,
    /// A free note about the incident, shown as the row's subtitle.
    pub note: String,
}

impl Default for Investigation {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            name: String::new(),
            spec: SourceSpec::Text {
                title: String::new(),
            },
            format: Format::Auto,
            tail_lines: 2000,
            chips: Vec::new(),
            highlights: Vec::new(),
            pinned: Vec::new(),
            bookmarks: Vec::new(),
            local_time: true,
            // Deliberately off: an investigation is a frozen picture of
            // something that already happened, and reopening it should
            // not rush to the newest line and lose the place.
            follow: false,
            mask: true,
            collapse: false,
            chart_open: true,
            order: 0,
            note: String::new(),
        }
    }
}

impl Investigation {
    pub fn file_for(&self, logs_dir: &Path) -> PathBuf {
        dir(logs_dir).join(format!("{}.toml", storage::slug(&self.name)))
    }

    pub fn load_all(logs_dir: &Path) -> Vec<Investigation> {
        let mut out = Vec::new();
        let Ok(files) = std::fs::read_dir(dir(logs_dir)) else {
            return out;
        };
        for f in files.flatten() {
            let p = f.path();
            if !p.extension().is_some_and(|e| e == "toml") {
                continue;
            }
            match storage::read_toml::<Investigation>(&p) {
                Ok(v) if v.schema <= SCHEMA => out.push(v),
                Ok(_) => tracing::warn!("skipping investigation {}: newer schema", p.display()),
                Err(e) => tracing::warn!("skipping investigation {}: {e}", p.display()),
            }
        }
        out.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn save(&self, logs_dir: &Path) -> storage::Result<()> {
        let _ = std::fs::create_dir_all(dir(logs_dir));
        storage::write_toml(&self.file_for(logs_dir), self)
    }

    pub fn delete(&self, logs_dir: &Path) -> std::io::Result<()> {
        let p = self.file_for(logs_dir);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }

    /// What the row says under the name: the note if there is one, else
    /// what was being read and how much was narrowed away.
    pub fn describe(&self) -> String {
        if !self.note.trim().is_empty() {
            return self.note.trim().to_string();
        }
        let mut parts = vec![self.spec.label()];
        if !self.chips.is_empty() {
            parts.push(format!("{} filters", self.chips.len()));
        }
        if !self.bookmarks.is_empty() {
            parts.push(format!("{} bookmarks", self.bookmarks.len()));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::ChipKind;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    fn sample() -> Investigation {
        Investigation {
            name: "Payment Outage".into(),
            spec: SourceSpec::LocalFile {
                path: "/var/log/app/app.log".into(),
            },
            chips: vec![Chip::new(ChipKind::Contains("timeout".into()))],
            bookmarks: vec![(42, "first failure".into()), (99, "recovered".into())],
            pinned: vec!["service".into()],
            local_time: false,
            note: "Deploy 14:02, recovered 14:31".into(),
            ..Default::default()
        }
    }

    /// The whole set-up has to come back, notes included: the notes are
    /// why an investigation is saved at all.
    #[test]
    fn an_investigation_round_trips_through_its_file() {
        let dir = tmp();
        let v = sample();
        v.save(dir.path()).expect("saved");
        let back = Investigation::load_all(dir.path());
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], v);
        assert_eq!(
            back[0].bookmarks,
            vec![
                (42, "first failure".to_string()),
                (99, "recovered".to_string())
            ]
        );
        assert!(!back[0].local_time, "the time zone comes back too");
    }

    /// The file name is derived from the name, as every other store here
    /// does it: a file under any other name would be duplicated the
    /// first time it was saved.
    #[test]
    fn the_file_name_is_the_slug_of_the_name() {
        let dir = tmp();
        let v = sample();
        assert_eq!(
            v.file_for(dir.path()).file_name().unwrap(),
            "payment-outage.toml"
        );
        // Saving twice is one file, not two.
        v.save(dir.path()).unwrap();
        v.save(dir.path()).unwrap();
        assert_eq!(Investigation::load_all(dir.path()).len(), 1);
    }

    /// Reopening an investigation must not rush to the newest line: it
    /// is a frozen picture of something that already happened, and
    /// following would lose the place.
    #[test]
    fn follow_is_off_by_default() {
        assert!(!Investigation::default().follow);
        assert!(Investigation::default().mask, "and masking stays on");
    }

    #[test]
    fn a_file_from_the_future_is_skipped_not_guessed_at() {
        let dir = tmp();
        let mut v = sample();
        v.schema = SCHEMA + 1;
        v.save(dir.path()).unwrap();
        assert!(
            Investigation::load_all(dir.path()).is_empty(),
            "a newer schema is refused"
        );
    }

    #[test]
    fn the_row_prefers_the_note_and_falls_back_to_the_setup() {
        let v = sample();
        assert_eq!(v.describe(), "Deploy 14:02, recovered 14:31");
        let bare = Investigation {
            note: "  ".into(),
            ..sample()
        };
        let text = bare.describe();
        assert!(text.contains("app.log"), "{text}");
        assert!(text.contains("1 filters"), "{text}");
        assert!(text.contains("2 bookmarks"), "{text}");
    }

    #[test]
    fn the_list_is_ordered_by_the_users_arrangement_then_by_name() {
        let dir = tmp();
        for (name, order) in [("Zeta", 0u32), ("Alpha", 1), ("Beta", 0)] {
            Investigation {
                name: name.into(),
                order,
                ..Default::default()
            }
            .save(dir.path())
            .unwrap();
        }
        let names: Vec<String> = Investigation::load_all(dir.path())
            .into_iter()
            .map(|v| v.name)
            .collect();
        assert_eq!(names, ["Beta", "Zeta", "Alpha"]);
    }

    /// Nothing saved yet must be an empty list, not an error: the folder
    /// does not exist until the first save.
    #[test]
    fn no_folder_means_no_investigations() {
        let dir = tmp();
        assert!(Investigation::load_all(dir.path()).is_empty());
    }
}
