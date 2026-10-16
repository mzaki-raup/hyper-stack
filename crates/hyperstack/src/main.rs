//! HyperStack entry point: command line, data folder, logging, then the window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(test)]
mod api_tests;
mod app;
mod bootstrap;
mod browser_host;
mod conn_test;
mod credentials;
mod dock;
#[cfg(test)]
mod editor_tests;
mod launcher;
mod self_test;
mod settings_ai;
mod settings_ui;
mod tabspace;
mod tools;
mod window_state;

use std::path::PathBuf;

use clap::Parser;

/// HyperStack — one workspace for everyone who ships software.
#[derive(Parser, Debug)]
#[command(name = "HyperStack", disable_version_flag = true)]
struct Cli {
    /// Print the version and exit.
    #[arg(long)]
    version: bool,

    /// Use this folder for all data instead of HyperStackData/ beside the executable.
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Exercise the core without opening a window, then exit 0 (used by CI and the updater).
    #[arg(long)]
    self_test: bool,

    /// Second half of an in-place update: wait for the old process, then start.
    #[arg(long, value_name = "VERSION")]
    finish_update: Option<String>,

    /// PID of the process that launched us for --finish-update.
    #[arg(long, value_name = "PID")]
    waiting_for: Option<u32>,

    /// Log filter (tracing syntax), default "info".
    #[arg(long, default_value = "info")]
    log: String,

    /// Headless: query the forge for a newer release, print the result, exit 0 when a
    /// signed, installable update exists, 3 when up to date, 1 on error.
    #[arg(long, hide = true)]
    check_updates: bool,

    /// Headless: like --check-updates, then download and verify the update into the
    /// data folder (size, sha256, signature, self-test) without installing it.
    #[arg(long, hide = true)]
    download_update: bool,

    /// Headless: like --download-update, then install it in place (rename-and-relaunch)
    /// and exit; the new version starts and opens its window.
    #[arg(long, hide = true)]
    install_update: bool,

    /// Headless: swap the kept previous executable back and relaunch it.
    #[arg(long, hide = true)]
    revert_update: bool,

    /// Files or folders to open in the editor at startup.
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    if cli.version {
        println!("HyperStack {}", hs_core::version::VersionInfo::current());
        return;
    }

    let paths = match hs_core::paths::DataDir::resolve(cli.data_dir.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("HyperStack cannot create its data folder: {e}");
            std::process::exit(2);
        }
    };

    if cli.self_test {
        let logging = hs_core::logging::init(None, &cli.log);
        std::process::exit(self_test::run(paths, logging));
    }
    if cli.check_updates || cli.download_update || cli.install_update {
        let _logging = hs_core::logging::init(None, &cli.log);
        std::process::exit(self_test::update_headless(
            paths,
            cli.download_update || cli.install_update,
            cli.install_update,
        ));
    }
    if cli.revert_update {
        let _logging = hs_core::logging::init(None, &cli.log);
        std::process::exit(self_test::revert_headless(paths));
    }

    let logging = hs_core::logging::init(Some(&paths.logs_dir()), &cli.log);
    tracing::info!(
        "HyperStack {} starting; data folder {} ({:?})",
        hs_core::version::VersionInfo::current(),
        paths.root().display(),
        paths.source
    );
    if let Some(v) = &cli.finish_update {
        // M1 implements the hand-over; Phase 0 only waits for the launcher to exit.
        tracing::info!("finishing update to {v}");
        if let Some(pid) = cli.waiting_for {
            wait_for_exit(pid);
        }
    }
    let _ = std::fs::write(paths.instance_lock(), std::process::id().to_string());

    let icon = load_icon();
    let window = window_state::WindowState::load(&paths.root().join("window.toml"));
    let viewport = window
        .viewport()
        .with_title("HyperStack")
        .with_icon(std::sync::Arc::new(icon));
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let open = cli
        .paths
        .iter()
        .map(|p| {
            // Absolute, without the verbatim `\\?\` prefix that canonicalize adds on Windows.
            let abs = std::path::absolute(p).unwrap_or_else(|_| p.clone());
            let s = abs.to_string_lossy();
            match s.strip_prefix(r"\\?\") {
                Some(rest) => PathBuf::from(rest),
                None => abs,
            }
        })
        .collect();
    let result = eframe::run_native(
        "HyperStack",
        options,
        Box::new(move |cc| Ok(Box::new(app::HyperStackApp::new(cc, paths, logging, open)))),
    );
    if let Err(e) = result {
        tracing::error!("window failed: {e}");
        std::process::exit(1);
    }
}

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icon/hyperstack-256.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn wait_for_exit(pid: u32) {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(30) {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
