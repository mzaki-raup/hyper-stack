//! The in-app updater (docs/09 §5): check the forge, download and verify a
//! release (the zip when the manifest lists one, with the executable taken
//! out of it), install it by rename-and-relaunch, keep the previous
//! executable for a one-click revert. Pure parsing/verification lives in [`crate::update`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::jobs::{JobHandle, Jobs, Progress};
use crate::paths::DataDir;
use crate::runtime::AppRuntime;
use crate::update::{self, Candidate, Channel, Manifest, ManifestAsset};
use crate::version::VersionInfo;

/// What Settings → Updates shows.
// `Available` carries the manifest; the enum is cloned once per frame at most.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate {
        checked_at: String,
    },
    Available {
        candidate: Candidate,
        manifest: Manifest,
        /// What is downloaded: the release zip when the manifest lists one
        /// (`<platform>-zip`), else the bare executable.
        asset: ManifestAsset,
        /// The manifest's entry for the bare executable when `asset` is a
        /// zip: the extracted file is checked against it as well.
        exe: Option<ManifestAsset>,
    },
    Downloading {
        version: String,
    },
    /// Verified executable waiting for "Restart and Install".
    Ready {
        version: String,
        path: PathBuf,
    },
    /// A newer release exists but this copy cannot update in place (too old,
    /// read-only folder); the zip is the way.
    ManualOnly {
        version: String,
        reason: String,
        html_url: String,
    },
    Error(String),
}

/// `HyperStackData/update/state.toml`: the hand-over record between versions.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct InstallState {
    pub installed: String,
    pub previous: String,
    pub at: String,
    /// Set by the new version after its first successful frame.
    pub ok: bool,
}

#[derive(Clone)]
pub struct Updater {
    runtime: AppRuntime,
    paths: Arc<DataDir>,
    status: Arc<Mutex<UpdateStatus>>,
    client: reqwest::Client,
    current: VersionInfo,
}

impl Updater {
    pub fn new(runtime: AppRuntime, paths: Arc<DataDir>, current: VersionInfo) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(format!("HyperStack/{}", current.version))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            runtime,
            paths,
            status: Arc::new(Mutex::new(UpdateStatus::Idle)),
            client,
            current,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().clone()
    }

    pub fn set_status(&self, s: UpdateStatus) {
        *self.status.lock() = s;
    }

    fn state_file(&self) -> PathBuf {
        self.paths.update_dir().join("state.toml")
    }

    pub fn install_state(&self) -> Option<InstallState> {
        crate::storage::read_toml(&self.state_file()).ok()
    }

    fn write_state(&self, st: &InstallState) -> Result<(), String> {
        crate::storage::write_toml(&self.state_file(), st).map_err(|e| e.to_string())
    }

    /// The executable kept by the last install, if any.
    pub fn previous_exe(&self) -> Option<(PathBuf, String)> {
        let p = self.paths.update_dir().join("previous").join(exe_name());
        let st = self.install_state()?;
        p.exists().then_some((p, st.previous))
    }

    /// Called by the running version after its first frame: records that the
    /// installed version started fine. Returns true when this call finished a
    /// pending install (so the UI can say "Updated to …").
    pub fn mark_started_ok(&self) -> Option<InstallState> {
        let mut st = self.install_state()?;
        if st.ok || st.installed != self.current.version.to_string() {
            return None;
        }
        st.ok = true;
        self.write_state(&st).ok()?;
        Some(st)
    }

    /// A pending install whose new version never reported `ok` (crash before
    /// first frame) — offer a revert on the next start of any copy.
    pub fn failed_install(&self) -> Option<InstallState> {
        let st = self.install_state()?;
        (!st.ok
            && st.installed != self.current.version.to_string()
            && self.previous_exe().is_some())
        .then_some(st)
    }

    /// Where the running executable's folder is, and whether we can write there.
    pub fn exe_dir_writable(&self) -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe
            .parent()
            .ok_or("executable has no parent folder")?
            .to_path_buf();
        let probe = dir.join(format!(".hs-update-probe-{}", std::process::id()));
        std::fs::write(&probe, b"x")
            .map_err(|e| format!("{} is not writable: {e}", dir.display()))?;
        let _ = std::fs::remove_file(&probe);
        Ok(dir)
    }

    /// Start a check. The result lands in [`status`](Self::status); the caller
    /// polls it each frame.
    pub fn check(&self, channel: Channel, source: Option<String>, skipped: Option<String>) {
        self.set_status(UpdateStatus::Checking);
        let this = self.clone();
        self.runtime.spawn(async move {
            let result = this.check_inner(channel, source, skipped).await;
            this.set_status(match result {
                Ok(s) => s,
                Err(e) => UpdateStatus::Error(e),
            });
        });
    }

    async fn check_inner(
        &self,
        channel: Channel,
        source: Option<String>,
        skipped: Option<String>,
    ) -> Result<UpdateStatus, String> {
        let url = source
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| update::RELEASES_URL.to_string());
        let json = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let Some(candidate) = update::newer_than(&json, &self.current.version, channel)? else {
            return Ok(UpdateStatus::UpToDate { checked_at: now });
        };
        if skipped.as_deref() == Some(candidate.tag.as_str()) {
            return Ok(UpdateStatus::UpToDate { checked_at: now });
        }
        let manifest_bytes = self
            .client
            .get(&candidate.manifest_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let signature = self
            .client
            .get(&candidate.signature_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let manifest = update::verify_manifest(&manifest_bytes, &signature, &candidate.tag)
            .map_err(|e| e.to_string())?;
        if let Err(e) = update::check_supported(&manifest, &self.current) {
            return Ok(UpdateStatus::ManualOnly {
                version: candidate.version.to_string(),
                reason: e.to_string(),
                html_url: candidate.html_url,
            });
        }
        // The zip first: the bare executable has outgrown what the forge's
        // edge serves in one response (100 MB behind Cloudflare), while the
        // compressed archive stays well under it. A release without a zip
        // entry still installs from the bare file.
        let key = self.current.platform_key();
        let exe = manifest.assets.get(&key).cloned();
        let Some(asset) = manifest
            .assets
            .get(&format!("{key}-zip"))
            .cloned()
            .or_else(|| exe.clone())
        else {
            return Ok(UpdateStatus::ManualOnly {
                version: candidate.version.to_string(),
                reason: format!("the release has no in-place asset for {key}"),
                html_url: candidate.html_url,
            });
        };
        let exe = exe.filter(|_| asset.name.ends_with(".zip"));
        if self.exe_dir_writable().is_err() {
            return Ok(UpdateStatus::ManualOnly {
                version: candidate.version.to_string(),
                reason: "the executable's folder is read-only".into(),
                html_url: candidate.html_url,
            });
        }
        Ok(UpdateStatus::Available {
            candidate,
            manifest,
            asset,
            exe,
        })
    }

    /// Download the asset as a Job, verify size + sha256 + `--self-test`, and
    /// leave it as `update/<version>/HyperStack.exe.ready`.
    pub fn download(
        &self,
        jobs: &Jobs,
        candidate: Candidate,
        asset: ManifestAsset,
        exe: Option<ManifestAsset>,
    ) -> JobHandle {
        let version = candidate.version.to_string();
        self.set_status(UpdateStatus::Downloading {
            version: version.clone(),
        });
        let this = self.clone();
        jobs.spawn(
            format!("Download HyperStack {version}"),
            "updater",
            move |h| async move {
                match this
                    .download_inner(&h, &version, &asset, exe.as_ref())
                    .await
                {
                    Ok(path) => {
                        this.set_status(UpdateStatus::Ready { version, path });
                        Ok(())
                    }
                    Err(e) => {
                        this.set_status(UpdateStatus::Error(e.clone()));
                        Err(e)
                    }
                }
            },
        )
    }

    async fn download_inner(
        &self,
        h: &JobHandle,
        version: &str,
        asset: &ManifestAsset,
        exe: Option<&ManifestAsset>,
    ) -> Result<PathBuf, String> {
        use futures_util::StreamExt;
        let dir = self.paths.update_dir().join(version);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let part = dir.join(format!("{}.part", exe_name()));
        let ready = dir.join(format!("{}.ready", exe_name()));
        let resp = self
            .client
            .get(&asset.url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let total = resp.content_length().or(Some(asset.size));
        let mut stream = resp.bytes_stream();
        let mut bytes: Vec<u8> = Vec::with_capacity(asset.size as usize);
        while let Some(chunk) = stream.next().await {
            if h.is_cancelled() {
                return Err("cancelled".into());
            }
            let chunk = chunk.map_err(|e| e.to_string())?;
            bytes.extend_from_slice(&chunk);
            h.set_progress(Progress::Bytes {
                done: bytes.len() as u64,
                total,
            });
        }
        h.set_message("verifying");
        update::verify_asset(asset, &bytes).map_err(|e| e.to_string())?;
        // A zip is verified whole against the signed manifest, then the
        // executable is taken out of it and, when the manifest also lists
        // the bare file, checked against that entry too.
        let bytes = if asset.name.ends_with(".zip") {
            h.set_message("extracting");
            let exe_entry = exe.cloned();
            self.runtime
                .spawn_blocking(move || {
                    let out = extract_exe(&bytes)?;
                    if let Some(entry) = exe_entry {
                        update::verify_asset(&entry, &out).map_err(|e| e.to_string())?;
                    }
                    Ok::<Vec<u8>, String>(out)
                })
                .await
                .map_err(|e| e.to_string())??
        } else {
            bytes
        };
        std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
        h.set_message("running self-test");
        let part2 = part.clone();
        let tmp = self.paths.tmp().join(format!("selftest-{version}"));
        let out = self
            .runtime
            .spawn_blocking(move || run_self_test(&part2, &tmp))
            .await
            .map_err(|e| e.to_string())??;
        if !out.contains("self-test ok") {
            return Err(format!(
                "downloaded executable failed its self-test: {}",
                out.lines().last().unwrap_or("")
            ));
        }
        if ready.exists() {
            std::fs::remove_file(&ready).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&part, &ready).map_err(|e| e.to_string())?;
        h.set_progress(Progress::Fraction(1.0));
        Ok(ready)
    }

    /// Rename-and-relaunch (docs/09 §5.4). On success the caller must exit the
    /// process: the new executable is already starting and waiting for us.
    pub fn install(&self, ready: &Path, new_version: &str) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = self.exe_dir_writable()?;
        let target = dir.join(exe_name());
        let previous_dir = self.paths.update_dir().join("previous");
        std::fs::create_dir_all(&previous_dir).map_err(|e| e.to_string())?;
        let previous = previous_dir.join(exe_name());
        if previous.exists() {
            std::fs::remove_file(&previous)
                .map_err(|e| format!("cannot replace the kept previous executable: {e}"))?;
        }
        // 1. move the running exe aside (allowed on Windows: rename, not overwrite)
        std::fs::rename(&exe, &previous)
            .map_err(|e| format!("cannot move the running executable aside: {e}"))?;
        // 2. put the new one in place; undo step 1 if that fails
        if let Err(e) = move_file(ready, &target) {
            let _ = std::fs::rename(&previous, &exe);
            return Err(format!("cannot place the new executable: {e}"));
        }
        self.write_state(&InstallState {
            installed: new_version.to_string(),
            previous: self.current.version.to_string(),
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ok: false,
        })?;
        // 3. start the new version; it waits for our pid, then opens its window
        std::process::Command::new(&target)
            .args([
                "--finish-update",
                new_version,
                "--waiting-for",
                &std::process::id().to_string(),
            ])
            .spawn()
            .map_err(|e| format!("the new executable did not start: {e}"))?;
        Ok(())
    }

    /// Swap the kept previous executable back. The caller exits afterwards.
    pub fn revert(&self) -> Result<String, String> {
        let (previous, prev_version) = self
            .previous_exe()
            .ok_or("no previous executable is kept")?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = self.exe_dir_writable()?;
        let target = dir.join(exe_name());
        let parked = self
            .paths
            .update_dir()
            .join(format!("reverted-{}.exe", self.current.version));
        if parked.exists() {
            let _ = std::fs::remove_file(&parked);
        }
        std::fs::rename(&exe, &parked)
            .map_err(|e| format!("cannot move the running executable aside: {e}"))?;
        if let Err(e) = move_file(&previous, &target) {
            let _ = std::fs::rename(&parked, &exe);
            return Err(format!("cannot restore the previous executable: {e}"));
        }
        self.write_state(&InstallState {
            installed: prev_version.clone(),
            previous: self.current.version.to_string(),
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ok: true,
        })?;
        std::process::Command::new(&target)
            .args([
                "--finish-update",
                &prev_version,
                "--waiting-for",
                &std::process::id().to_string(),
            ])
            .spawn()
            .map_err(|e| format!("the previous executable did not start: {e}"))?;
        Ok(prev_version)
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "HyperStack.exe"
    } else {
        "hyperstack"
    }
}

/// The executable inside a release zip (`HyperStack.exe` at the archive
/// root or under one folder). Anything else in the archive (README,
/// LICENSE, notes) is ignored.
fn extract_exe(zip_bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("not a zip: {e}"))?;
    let want = exe_name();
    let index = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .ok()
            .and_then(|f| f.name().ok().map(|n| n.to_string()))
            .is_some_and(|n| n.rsplit(['/', '\\']).next() == Some(want))
    });
    let Some(i) = index else {
        return Err(format!("the archive holds no {want}"));
    };
    let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Rename when on the same volume, copy + delete otherwise.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        std::fs::remove_file(to)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

/// Run `<exe> --self-test --data-dir <tmp>` with a 30 s timeout; returns its stdout.
fn run_self_test(exe: &Path, tmp: &Path) -> Result<String, String> {
    use std::io::Read;
    let _ = std::fs::remove_dir_all(tmp);
    let mut child = std::process::Command::new(exe)
        .args(["--self-test", "--data-dir", &tmp.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run the downloaded executable: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                let _ = std::fs::remove_dir_all(tmp);
                return if status.success() {
                    Ok(out)
                } else {
                    Err(format!(
                        "self-test exited with {status}: {}",
                        out.lines().last().unwrap_or("")
                    ))
                };
            }
            Ok(None) if start.elapsed() > Duration::from_secs(30) => {
                let _ = child.kill();
                return Err("self-test timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_state_round_trips_and_mark_ok_only_for_the_installed_version() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Arc::new(DataDir::resolve(Some(tmp.path())).unwrap());
        let rt = AppRuntime::new(Arc::new(crate::runtime::NoopWaker)).unwrap();
        let current = VersionInfo::current();
        let u = Updater::new(rt, paths.clone(), current.clone());
        assert!(u.install_state().is_none());
        assert!(u.mark_started_ok().is_none());

        // A pending install of exactly this version → marked ok once.
        u.write_state(&InstallState {
            installed: current.version.to_string(),
            previous: "0.0.1".into(),
            at: "now".into(),
            ok: false,
        })
        .unwrap();
        let st = u.mark_started_ok().unwrap();
        assert!(st.ok);
        assert!(u.mark_started_ok().is_none());

        // A pending install of another version with a kept previous exe → failed install.
        std::fs::create_dir_all(paths.update_dir().join("previous")).unwrap();
        std::fs::write(paths.update_dir().join("previous").join(exe_name()), b"old").unwrap();
        u.write_state(&InstallState {
            installed: "9.9.9".into(),
            previous: "0.0.1".into(),
            at: "now".into(),
            ok: false,
        })
        .unwrap();
        assert!(u.failed_install().is_some());
        assert_eq!(u.previous_exe().unwrap().1, "0.0.1");
    }

    #[test]
    fn the_executable_is_taken_out_of_the_release_zip() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("README.txt", opts).unwrap();
            w.write_all(b"portable").unwrap();
            w.start_file(format!("HyperStack-1.2.3/{}", exe_name()), opts)
                .unwrap();
            w.write_all(b"MZ-not-really").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(extract_exe(buf.get_ref()).unwrap(), b"MZ-not-really");
        assert!(extract_exe(b"nope").is_err());
        let mut empty = std::io::Cursor::new(Vec::new());
        zip::ZipWriter::new(&mut empty).finish().unwrap();
        assert!(
            extract_exe(empty.get_ref())
                .unwrap_err()
                .contains("holds no")
        );
    }

    #[test]
    fn move_file_replaces_target() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::write(&a, b"new").unwrap();
        std::fs::write(&b, b"old").unwrap();
        move_file(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"new");
        assert!(!a.exists());
    }
}
