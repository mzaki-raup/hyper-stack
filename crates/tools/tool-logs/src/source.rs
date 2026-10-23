//! Where a log comes from, and the saved sources in the workspace
//! (`<workspace>/logs/<slug>.toml`, docs/tools/logs.md §12).

use std::path::{Path, PathBuf};

use hs_core::storage;
use serde::{Deserialize, Serialize};

use crate::parse::Format;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSpec {
    /// A file on this machine, followed like `tail -F`.
    LocalFile { path: String },
    /// A file on an SSH host (`tail -n <lines> -F <path>` on an exec
    /// channel), through a saved `ssh` connection profile.
    SshFile { conn_id: String, path: String },
    /// Any command's stdout, local or on an SSH host.
    Command {
        #[serde(default)]
        conn_id: Option<String>,
        command: String,
    },
    /// Text pasted from the clipboard, kept with the tab.
    Text { title: String },
}

impl SourceSpec {
    /// What the tab and the sidebar call it.
    pub fn label(&self) -> String {
        match self {
            SourceSpec::LocalFile { path } => Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone()),
            SourceSpec::SshFile { path, .. } => Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone()),
            SourceSpec::Command { command, .. } => command
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" "),
            SourceSpec::Text { title } => title.clone(),
        }
    }

    /// The whole location, for tooltips and the header.
    pub fn describe(&self) -> String {
        match self {
            SourceSpec::LocalFile { path } => path.clone(),
            SourceSpec::SshFile { path, .. } => path.clone(),
            SourceSpec::Command { command, .. } => command.clone(),
            SourceSpec::Text { title } => title.clone(),
        }
    }

    /// The glyph the sidebar and the dashboard draw for this kind of
    /// source, so the two never disagree about what a command looks like.
    pub fn icon(&self) -> &'static str {
        use hs_ui::icons::ph;
        match self {
            SourceSpec::LocalFile { .. } => ph::FILE_TEXT,
            SourceSpec::SshFile { .. } => ph::HARD_DRIVES,
            SourceSpec::Command { .. } => ph::TERMINAL,
            SourceSpec::Text { .. } => ph::CLIPBOARD_TEXT,
        }
    }

    pub fn conn_id(&self) -> Option<&str> {
        match self {
            SourceSpec::SshFile { conn_id, .. } => Some(conn_id),
            SourceSpec::Command { conn_id, .. } => conn_id.as_deref(),
            _ => None,
        }
    }

    /// A stable id for `current_uri`, so opening the same source twice
    /// focuses the tab that has it.
    pub fn uri(&self) -> String {
        match self {
            SourceSpec::LocalFile { path } => format!("logs://file/{path}"),
            SourceSpec::SshFile { conn_id, path } => format!("logs://ssh/{conn_id}{path}"),
            SourceSpec::Command { conn_id, command } => {
                format!(
                    "logs://cmd/{}/{command}",
                    conn_id.as_deref().unwrap_or("local")
                )
            }
            SourceSpec::Text { title } => format!("logs://text/{title}"),
        }
    }
}

/// A source kept in the workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSource {
    pub schema: u32,
    pub name: String,
    pub spec: SourceSpec,
    /// Parser override; `Auto` detects.
    pub format: Format,
    /// Lines of history a tail starts with (SSH and command sources).
    pub tail_lines: u32,
    /// Where this sits in the sidebar. The list is the user's to arrange,
    /// so it is dragged rather than sorted by name (owner's request,
    /// 2026-10-23); sources saved before this all read 0 and keep their
    /// alphabetical order until one is moved.
    #[serde(default)]
    pub order: u32,
}

impl Default for SavedSource {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            name: String::new(),
            spec: SourceSpec::Text {
                title: String::new(),
            },
            format: Format::Auto,
            tail_lines: 2000,
            order: 0,
        }
    }
}

impl SavedSource {
    pub fn file_for(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.toml", storage::slug(&self.name)))
    }

    pub fn load_all(dir: &Path) -> Vec<SavedSource> {
        let mut out = Vec::new();
        if let Ok(files) = std::fs::read_dir(dir) {
            for f in files.flatten() {
                let p = f.path();
                if p.extension().is_some_and(|e| e == "toml") {
                    match storage::read_toml::<SavedSource>(&p) {
                        Ok(s) if s.schema <= SCHEMA => out.push(s),
                        Ok(_) => {
                            tracing::warn!("skipping log source {}: newer schema", p.display())
                        }
                        Err(e) => tracing::warn!("skipping log source {}: {e}", p.display()),
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn save(&self, dir: &Path) -> storage::Result<()> {
        let _ = std::fs::create_dir_all(dir);
        storage::write_toml(&self.file_for(dir), self)
    }

    pub fn delete(&self, dir: &Path) -> std::io::Result<()> {
        let p = self.file_for(dir);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_sources_round_trip_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let s = SavedSource {
            name: "web-01 app.log".into(),
            spec: SourceSpec::SshFile {
                conn_id: "c1".into(),
                path: "/var/log/app/app.log".into(),
            },
            format: Format::Json,
            tail_lines: 500,
            ..Default::default()
        };
        s.save(dir.path()).unwrap();
        let all = SavedSource::load_all(dir.path());
        assert_eq!(all, vec![s.clone()]);
        assert_eq!(s.spec.label(), "app.log");
        assert_eq!(s.spec.uri(), "logs://ssh/c1/var/log/app/app.log");
        s.delete(dir.path()).unwrap();
        assert!(SavedSource::load_all(dir.path()).is_empty());
    }
}
