//! The editor's own terminal: a panel at the foot of the editor pane with
//! one or more shells, started in the open project's folder.
//!
//! The shell is PowerShell 7 where it exists, and Settings → Editor →
//! Terminal picks another. Owner's requirement, 2026-10-17.
//!
//! It is a second home for `hs-term` rather than a second implementation:
//! the session, the grid and the key handling are the terminal tool's, and
//! only the panel, its tabs and the project's folder belong here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hs_core::Services;
use hs_term::links::LinkTarget;
use hs_term::session::Session;
use hs_term::shells::ShellProfile;
use hs_term::widget::{TermTheme, TerminalView};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

/// Which shell a new editor terminal uses.
///
/// `configured` is Settings → Editor → Terminal, `fallback` the Terminal
/// tool's own default. With neither set this is **PowerShell 7**, which is
/// what the owner asked for; on a machine without it the first detected
/// shell is used, so the panel always opens on something.
pub fn pick_profile<'a>(
    profiles: &'a [ShellProfile],
    configured: &str,
    fallback: &str,
) -> Option<&'a ShellProfile> {
    let by_id = |id: &str| {
        if id.is_empty() {
            None
        } else {
            profiles.iter().find(|p| p.id == id)
        }
    };
    by_id(configured)
        .or_else(|| by_id("pwsh"))
        .or_else(|| by_id(fallback))
        .or_else(|| profiles.first())
}

/// One shell in the panel.
struct Shell {
    session: Session,
    view: TerminalView,
    title: String,
    /// Typed as soon as the shell is up (a build or run command).
    pending: Option<String>,
    /// This shell is running a web application: its output is read until
    /// it names an address, which is then opened in the Browser once.
    /// Owner's requirement, 2026-10-17.
    watch_web: bool,
}

/// What a frame of the panel produced for the editor to act on.
#[derive(Default)]
pub struct PanelOut {
    /// A Ctrl+clicked link.
    pub link: Option<LinkTarget>,
    /// The address a web application just reported, to open once.
    pub url: Option<String>,
}

#[derive(Default)]
pub struct TerminalPanel {
    pub open: bool,
    shells: Vec<Shell>,
    active: usize,
    /// Why no shell could be started; cleared when one is asked for again.
    error: Option<String>,
    /// Where the shells of this panel were started, so a project change
    /// does not silently move a running shell.
    cwd: Option<PathBuf>,
}

impl TerminalPanel {
    /// Show the panel, starting the first shell if there is none yet.
    pub fn show(&mut self, s: &Services, root: Option<&Path>) {
        self.open = true;
        if self.shells.is_empty() {
            self.spawn(s, root, None);
        }
    }

    /// Open the panel and run `command` in a shell of its own. With
    /// `web`, the first address the program prints is handed back from
    /// [`TerminalPanel::panel_ui`] for the Browser to open.
    pub fn run(&mut self, s: &Services, root: Option<&Path>, command: &str, web: bool) {
        self.run_with_env(s, root, command, web, &[]);
    }

    /// `run`, with variables set in the shell's environment first - so
    /// they reach the program whatever the shell is, without a
    /// `$env:` / `set` / `export` prefix that only one of them reads.
    pub fn run_with_env(
        &mut self,
        s: &Services,
        root: Option<&Path>,
        command: &str,
        web: bool,
        env: &[(String, String)],
    ) {
        self.open = true;
        self.spawn_with_env(s, root, Some(command.to_string()), env);
        if let Some(t) = self.shells.last_mut() {
            // The command names the tab, so a build and a run are told
            // apart at a glance.
            t.title = command
                .split_whitespace()
                .next()
                .unwrap_or("run")
                .to_string();
            t.watch_web = web;
        }
    }

    /// Open the panel with a shell in `dir`, whatever the project's own
    /// folder is: "Open Terminal Here" on a folder in the tree.
    pub fn show_in(&mut self, s: &Services, dir: &Path) {
        self.open = true;
        self.spawn(s, Some(dir), None);
    }

    pub fn toggle(&mut self, s: &Services, root: Option<&Path>) {
        if self.open {
            self.open = false;
        } else {
            self.show(s, root);
        }
    }

    /// Start a shell. `pending` is typed once it answers.
    fn spawn(&mut self, s: &Services, root: Option<&Path>, pending: Option<String>) {
        self.spawn_with_env(s, root, pending, &[]);
    }

    fn spawn_with_env(
        &mut self,
        s: &Services,
        root: Option<&Path>,
        pending: Option<String>,
        env: &[(String, String)],
    ) {
        self.error = None;
        let profiles = hs_term::shells::profiles();
        let (configured, fallback, scrollback, font, copy, right_click, links) = {
            let st = s.settings.read();
            (
                st.editor.terminal_profile.clone(),
                st.terminal.default_profile.clone(),
                st.terminal.scrollback,
                st.editor.font_size,
                st.terminal.copy_on_select,
                st.terminal.right_click_copy_paste,
                st.terminal.link_detection,
            )
        };
        let Some(mut profile) = pick_profile(&profiles, &configured, &fallback).cloned() else {
            self.error = Some(s.t("terminal-no-shell"));
            return;
        };
        profile.env.extend(env.iter().cloned());
        let cwd = root
            .map(|r| r.to_path_buf())
            .unwrap_or_else(|| s.workspace.read().root().to_path_buf());
        let waker = s.runtime.waker();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
        match Session::spawn(profile.clone(), Some(&cwd), 100, 12, scrollback, waker) {
            Ok(session) => {
                let mut view =
                    TerminalView::new(egui::Id::new(("hs-editor-term", hs_core::new_id())), font);
                view.copy_on_select = copy;
                view.right_click_copy_paste = right_click;
                view.link_detection = links;
                self.cwd = Some(cwd);
                self.shells.push(Shell {
                    session,
                    view,
                    title: profile.name.clone(),
                    pending,
                    watch_web: false,
                });
                self.active = self.shells.len() - 1;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn close(&mut self, i: usize) {
        if i < self.shells.len() {
            self.shells[i].session.kill();
            self.shells.remove(i);
            self.active = self.active.min(self.shells.len().saturating_sub(1));
        }
        if self.shells.is_empty() {
            self.open = false;
        }
    }

    /// Draw the panel.
    pub fn panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        root: Option<&Path>,
    ) -> PanelOut {
        let mut out = PanelOut::default();
        let mut link = None;
        let mut close_panel = false;
        let mut close_shell = None;
        let mut new_shell = false;
        ui.horizontal(|ui| {
            for (i, t) in self.shells.iter().enumerate() {
                let dead = t.session.exited().is_some();
                let label = if dead {
                    format!("{} ({})", t.title, s.t("editor-terminal-exited"))
                } else {
                    t.title.clone()
                };
                if ui
                    .add(egui::Button::selectable(self.active == i, label))
                    .clicked()
                {
                    self.active = i;
                }
            }
            if ui
                .small_button(ph::PLUS)
                .tip(s.t("editor-terminal-new"))
                .clicked()
            {
                new_shell = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(ph::X)
                    .tip(s.t("editor-terminal-hide"))
                    .clicked()
                {
                    close_panel = true;
                }
                if !self.shells.is_empty()
                    && ui
                        .small_button(ph::TRASH)
                        .tip(s.t("editor-terminal-close"))
                        .clicked()
                {
                    close_shell = Some(self.active);
                }
                if let Some(cwd) = &self.cwd {
                    ui.label(
                        egui::RichText::new(cwd.display().to_string())
                            .small()
                            .color(theme.text_muted),
                    );
                }
            });
        });
        ui.separator();
        if let Some(err) = self.error.clone() {
            ui.colored_label(theme.error, err);
        }
        let tt = TermTheme::for_theme(&Theme::from_settings(&s.settings.read().appearance));
        // Every shell is drained, not only the one on screen: a background
        // shell's event queue otherwise grows until it is selected.
        for (i, t) in self.shells.iter_mut().enumerate() {
            if i != self.active {
                let _ = t.session.drain_events();
            }
            if t.watch_web && t.session.exited().is_some() {
                t.watch_web = false;
            }
        }
        if let Some(t) = self.shells.get_mut(self.active) {
            // A command handed to a fresh shell is typed once it is up;
            // ConPTY swallows anything written before that.
            if let Some(cmd) = t.pending.take() {
                t.session.write(format!("{cmd}\r").as_bytes());
            }
            let _ = t.session.drain_events();
            if let Some(code) = t.session.exited() {
                ui.horizontal(|ui| {
                    ui.label(
                        s.i18n
                            .t_args("terminal-exited", &[("code", &code.to_string())]),
                    );
                    if ui.button(s.t("terminal-restart")).clicked() {
                        close_shell = Some(self.active);
                        new_shell = true;
                    }
                });
            } else {
                let r = t.view.show(ui, &mut t.session, &tt);
                if r.link.is_some() {
                    link = r.link;
                }
            }
        } else if self.error.is_none() {
            hs_ui::form::hint(ui, theme, &s.t("editor-terminal-empty"));
        }
        if let Some(i) = close_shell {
            self.close(i);
        }
        if new_shell {
            self.open = true;
            self.spawn(s, root, None);
        }
        if close_panel {
            self.open = false;
        }
        // A web application says where it is listening a second or two
        // after it starts, so every shell that is watching is read each
        // frame until one does. Reading the grid is enough: the line is on
        // screen when it is printed.
        for t in self.shells.iter_mut().filter(|t| t.watch_web) {
            let text = t.session.visible_text(200);
            if let Some(url) = crate::project::served_url(&text) {
                t.watch_web = false;
                out.url = Some(url);
                break;
            }
        }
        out.link = link;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> ShellProfile {
        ShellProfile::new(id, id, "", format!("{id}.exe"), &[])
    }

    #[test]
    fn powershell_7_is_the_default_and_settings_win() {
        let all = [p("cmd"), p("pwsh"), p("git-bash")];
        // Nothing configured: PowerShell 7, not the first detected shell.
        assert_eq!(pick_profile(&all, "", "").unwrap().id, "pwsh");
        // Settings → Editor wins over everything.
        assert_eq!(
            pick_profile(&all, "git-bash", "cmd").unwrap().id,
            "git-bash"
        );
        // A shell that is no longer installed falls back the same way.
        assert_eq!(pick_profile(&all, "wsl:Gone", "").unwrap().id, "pwsh");
    }

    #[test]
    fn without_powershell_7_the_terminal_default_then_the_first_is_used() {
        let all = [p("cmd"), p("git-bash")];
        assert_eq!(pick_profile(&all, "", "git-bash").unwrap().id, "git-bash");
        assert_eq!(pick_profile(&all, "", "").unwrap().id, "cmd");
        assert!(pick_profile(&[], "", "").is_none());
    }
}
