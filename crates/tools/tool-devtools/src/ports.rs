//! What is listening on this machine, and which process owns it
//! (docs/tools/devtools.md §2 Network, "Local processes and ports").
//!
//! Read by asking the platform's own tool - `netstat -ano` plus
//! `tasklist` on Windows, `ss -ltnp` elsewhere - through
//! `hs_core::paths::background_command`, never a bare `Command`. The
//! parsing lives here so it can be tested against captured output.

use std::sync::Arc;

use hs_core::Services;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use parking_lot::Mutex;

/// One listening socket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listener {
    pub proto: String,
    pub address: String,
    pub port: u16,
    pub pid: u32,
    pub process: String,
}

type Slot = Arc<Mutex<Option<Result<Vec<Listener>, String>>>>;

#[derive(Default)]
pub struct State {
    pub filter: String,
    pub rows: Vec<Listener>,
    pub error: Option<String>,
    pub busy: bool,
    pub table: TableState,
    slot: Option<Slot>,
    /// A kill waiting for its yes.
    pub confirm: Option<Listener>,
}

/// `netstat -ano` lines that are LISTENING, with the pid at the end.
pub fn parse_netstat(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Proto Local Foreign State PID
        if f.len() < 5 || !f[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let Some((addr, port)) = f[1].rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        out.push(Listener {
            proto: f[0].to_string(),
            address: addr.trim_matches(['[', ']']).to_string(),
            port,
            pid: f[4].parse().unwrap_or(0),
            process: String::new(),
        });
    }
    out.sort_by_key(|l| (l.port, l.pid));
    out.dedup_by_key(|l| (l.port, l.pid, l.address.clone()));
    out
}

/// `tasklist /fo csv /nh`: pid → image name.
pub fn parse_tasklist(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split("\",\"").collect();
            if cells.len() < 2 {
                return None;
            }
            let name = cells[0].trim_matches('"').to_string();
            let pid = cells[1].trim_matches('"').parse().ok()?;
            Some((pid, name))
        })
        .collect()
}

/// `ss -ltnp` (or `netstat -ltnp`) lines.
pub fn parse_ss(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        // State Recv-Q Send-Q Local Peer [users:(("name",pid=1,fd=2))]
        let local = f[3];
        let Some((addr, port)) = local.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let mut pid = 0;
        let mut process = String::new();
        if let Some(users) = line.split_once("users:((").map(|(_, r)| r) {
            if let Some(name) = users.split('"').nth(1) {
                process = name.to_string();
            }
            if let Some(p) = users.split("pid=").nth(1) {
                pid = p
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
            }
        }
        out.push(Listener {
            proto: "tcp".into(),
            address: addr.trim_matches(['[', ']']).to_string(),
            port,
            pid,
            process,
        });
    }
    out.sort_by_key(|l| (l.port, l.pid));
    out
}

fn read_listeners() -> Result<Vec<Listener>, String> {
    #[cfg(windows)]
    {
        let out = hs_core::paths::background_command("netstat")
            .args(["-ano", "-p", "TCP"])
            .output()
            .map_err(|e| e.to_string())?;
        let mut rows = parse_netstat(&String::from_utf8_lossy(&out.stdout));
        if let Ok(tasks) = hs_core::paths::background_command("tasklist")
            .args(["/fo", "csv", "/nh"])
            .output()
        {
            let names = parse_tasklist(&String::from_utf8_lossy(&tasks.stdout));
            for r in &mut rows {
                if let Some((_, n)) = names.iter().find(|(pid, _)| *pid == r.pid) {
                    r.process = n.clone();
                }
            }
        }
        Ok(rows)
    }
    #[cfg(not(windows))]
    {
        let out = hs_core::paths::background_command("ss")
            .args(["-ltnp"])
            .output()
            .map_err(|e| e.to_string())?;
        Ok(parse_ss(&String::from_utf8_lossy(&out.stdout)))
    }
}

pub fn refresh(state: &mut State, services: &Services) {
    let slot: Slot = Arc::new(Mutex::new(None));
    state.slot = Some(slot.clone());
    state.busy = true;
    let waker = services.runtime.waker();
    services.runtime.spawn(async move {
        let r = tokio::task::spawn_blocking(read_listeners)
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        *slot.lock() = Some(r);
        waker.request_repaint();
    });
}

fn kill(pid: u32, services: &Services) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = hs_core::paths::background_command("taskkill");
        c.args(["/PID", &pid.to_string(), "/F"]);
        c
    } else {
        let mut c = hs_core::paths::background_command("kill");
        c.args(["-9", &pid.to_string()]);
        c
    };
    let _ = services;
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn ui(state: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    if let Some(slot) = state.slot.clone()
        && let Some(done) = slot.lock().take()
    {
        match done {
            Ok(rows) => {
                state.rows = rows;
                state.error = None;
            }
            Err(e) => state.error = Some(e),
        }
        state.busy = false;
        state.slot = None;
    }
    // First open reads once; after that it is the button's job.
    if state.rows.is_empty() && state.error.is_none() && !state.busy && state.slot.is_none() {
        refresh(state, s);
    }
    form::section(ui, theme, &s.t("dt-ports"), |ui| {
        form::hint(ui, theme, &s.t("dt-ports-hint"));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.busy,
                    egui::Button::new(format!(
                        "{} {}",
                        ph::ARROWS_CLOCKWISE,
                        s.t("dt-ports-refresh")
                    )),
                )
                .clicked()
            {
                refresh(state, s);
            }
            if state.busy {
                ui.spinner();
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .hint_text(s.t("dt-ports-filter"))
                    .desired_width(200.0),
            );
            ui.label(
                egui::RichText::new(
                    s.i18n
                        .t_args("dt-ports-count", &[("n", &state.rows.len().to_string())]),
                )
                .small()
                .color(theme.text_muted),
            );
        });
        if let Some(e) = &state.error {
            form::note(ui, theme, ph::WARNING, e);
            return;
        }
        let needle = state.filter.trim().to_lowercase();
        let shown: Vec<&Listener> = state
            .rows
            .iter()
            .filter(|l| {
                needle.is_empty()
                    || l.port.to_string().contains(&needle)
                    || l.process.to_lowercase().contains(&needle)
                    || l.address.contains(&needle)
            })
            .collect();
        let cells: Vec<Vec<String>> = shown
            .iter()
            .map(|l| {
                vec![
                    l.port.to_string(),
                    l.address.clone(),
                    l.proto.clone(),
                    l.pid.to_string(),
                    l.process.clone(),
                ]
            })
            .collect();
        let columns = [
            ColumnSpec::new(s.t("dt-ports-port"), 80.0),
            ColumnSpec::new(s.t("dt-ports-address"), 140.0),
            ColumnSpec::new(s.t("dt-ports-proto"), 70.0),
            ColumnSpec::new(s.t("dt-ports-pid"), 80.0),
            ColumnSpec::new(s.t("dt-ports-process"), 220.0),
        ];
        let copy_label = s.t("table-copy-row");
        let r = ui
            .allocate_ui(egui::vec2(ui.available_width(), 320.0), |ui| {
                DataTable {
                    theme,
                    columns: &columns,
                    rows: &cells,
                    row_height: 22.0,
                    show_filter: false,
                    copy_row_label: &copy_label,
                    built_in_menu: true,
                    row_icons: None,
                }
                .show(ui, &mut state.table)
            })
            .inner;
        let _ = r;
        let selected = state
            .table
            .selected
            .and_then(|i| shown.get(i).map(|l| (*l).clone()));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Killing a process is destructive, so it asks first
            // (docs/06-security.md §3).
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(format!("{} {}", ph::SKULL, s.t("dt-ports-kill"))),
                )
                .clicked()
            {
                state.confirm = selected.clone();
            }
            if let Some(l) = &selected {
                ui.label(
                    egui::RichText::new(format!(":{} · {}", l.port, l.process))
                        .small()
                        .color(theme.text_muted),
                );
            }
        });
    });
    // The confirmation.
    if let Some(target) = state.confirm.clone() {
        let mut close = false;
        egui::Modal::new(egui::Id::new("dt-ports-kill")).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.heading(s.t("dt-ports-kill-title"));
            ui.add_space(6.0);
            ui.label(s.i18n.t_args(
                "dt-ports-kill-body",
                &[
                    ("process", &target.process),
                    ("pid", &target.pid.to_string()),
                    ("port", &target.port.to_string()),
                ],
            ));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(s.t("dt-ports-kill")).clicked() {
                    match kill(target.pid, s) {
                        Ok(()) => refresh(state, s),
                        Err(e) => state.error = Some(e),
                    }
                    close = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    close = true;
                }
            });
        });
        if close {
            state.confirm = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netstat_listening_lines_parse() {
        let text = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1120\r\n  TCP    127.0.0.1:6379         0.0.0.0:0              LISTENING       9876\r\n  TCP    192.168.1.5:50123      52.1.2.3:443           ESTABLISHED     4444\r\n  TCP    [::]:445               [::]:0                 LISTENING       4\r\n";
        let rows = parse_netstat(text);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(rows[0].port, 135);
        assert_eq!(rows[0].pid, 1120);
        assert_eq!(rows[1].port, 445);
        assert_eq!(rows[1].address, "::");
        assert_eq!(rows[2].port, 6379);
        assert!(rows.iter().all(|r| r.proto == "TCP"));
    }

    #[test]
    fn tasklist_csv_maps_pids_to_names() {
        let text = "\"svchost.exe\",\"1120\",\"Services\",\"0\",\"12,345 K\"\r\n\"redis-server.exe\",\"9876\",\"Console\",\"1\",\"5,000 K\"\r\n";
        let names = parse_tasklist(text);
        assert_eq!(names.len(), 2);
        assert_eq!(names[1], (9876, "redis-server.exe".to_string()));
        assert!(parse_tasklist("rubbish").is_empty());
    }

    #[test]
    fn ss_lines_parse_with_the_process() {
        let text = "State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\nLISTEN 0      4096   0.0.0.0:6379        0.0.0.0:*         users:((\"redis-server\",pid=812,fd=6))\nLISTEN 0      511    [::]:80             [::]:*            users:((\"nginx\",pid=99,fd=7))\n";
        let rows = parse_ss(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].port, 80);
        assert_eq!(rows[0].process, "nginx");
        assert_eq!(rows[0].pid, 99);
        assert_eq!(rows[1].port, 6379);
        assert_eq!(rows[1].process, "redis-server");
    }
}
