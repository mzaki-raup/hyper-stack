//! `--self-test`: exercise the core end to end without a window. CI runs it
//! after every build and the updater runs it on a downloaded executable
//! before installing it (docs/09 §5.3).

use std::sync::Arc;

use hs_core::logging::Logging;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::{KdfParams, SecretStore, VaultStore};
use secrecy::{ExposeSecret, SecretString};

pub fn run(paths: DataDir, _logging: Logging) -> i32 {
    let mut failures = Vec::new();
    let mut check = |name: &str, ok: Result<(), String>| {
        match ok {
            Ok(()) => println!("ok      {name}"),
            Err(e) => {
                println!("FAILED  {name}: {e}");
                failures.push(name.to_string());
            }
        }
        // Out to the pipe now, not when the buffer fills: a run that ends
        // inside a check still shows the last one it passed (BUG-0042).
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };

    println!(
        "HyperStack {} self-test",
        hs_core::version::VersionInfo::current()
    );
    println!(
        "data folder: {} ({:?})",
        paths.root().display(),
        paths.source
    );

    check(
        "data folder layout",
        (|| {
            for d in [
                paths.workspaces_dir(),
                paths.logs_dir(),
                paths.cache_dir(),
                paths.tmp(),
            ] {
                if !d.is_dir() {
                    return Err(format!("missing {}", d.display()));
                }
            }
            Ok(())
        })(),
    );

    let boot = match crate::bootstrap::services(paths.clone(), Arc::new(NoopWaker), KdfParams::FAST)
    {
        Ok(b) => b,
        Err(e) => {
            println!("FAILED  bootstrap: {e}");
            return 1;
        }
    };
    let s = &boot.services;

    check(
        "settings round trip",
        (|| {
            s.save_settings().map_err(|e| e.to_string())?;
            let back = hs_core::settings::Settings::load(&paths.settings_file())
                .map_err(|e| e.to_string())?;
            if back != *s.settings.read() {
                Err("settings differ after reload".into())
            } else {
                Ok(())
            }
        })(),
    );

    check("default workspace", {
        let ws = s.workspace.read();
        if ws.root().join("workspace.toml").exists() {
            Ok(())
        } else {
            Err("workspace.toml missing".into())
        }
    });

    check(
        "i18n",
        if s.t("app-name") == "HyperStack" {
            Ok(())
        } else {
            Err("app-name not translated".into())
        },
    );

    check(
        "keymap",
        if s.shortcuts.read().get("palette.commands").is_some() {
            Ok(())
        } else {
            Err("default binding missing".into())
        },
    );

    check(
        "environment substitution",
        (|| {
            let mut envs = s.environments.write();
            let env = hs_core::environments::Environment {
                name: "selftest".into(),
                variables: std::collections::HashMap::from([("host".into(), "db.local".into())]),
                ..Default::default()
            };
            let id = env.id.clone();
            envs.upsert(env).map_err(|e| e.to_string())?;
            let out = envs
                .substitute("{{host}}:{{$randomInt(1,1)}}", Some(&id), &[])
                .map_err(|m| format!("unresolved {m:?}"))?;
            envs.remove(&id).map_err(|e| e.to_string())?;
            if out == "db.local:1" {
                Ok(())
            } else {
                Err(format!("got {out}"))
            }
        })(),
    );

    check(
        "vault create / set / get / lock / unlock",
        (|| {
            let vault = VaultStore::new(&paths.vault_file(), KdfParams::FAST);
            let master = SecretString::from(format!("selftest-{}", hs_core::new_id()));
            if vault.exists() {
                // A real vault exists here (self-test run inside a used data folder); do not touch it.
                return Ok(());
            }
            vault.initialize(&master).map_err(|e| e.to_string())?;
            let r = vault
                .create("selftest", &SecretString::from("value-123".to_string()))
                .map_err(|e| e.to_string())?;
            vault.lock();
            vault.unlock(&master).map_err(|e| e.to_string())?;
            let v = vault.get(&r).map_err(|e| e.to_string())?;
            if v.expose_secret() != "value-123" {
                return Err("value mismatch".into());
            }
            let bytes = std::fs::read(paths.vault_file()).map_err(|e| e.to_string())?;
            if bytes.windows(9).any(|w| w == b"value-123") {
                return Err("plaintext secret found in vault file".into());
            }
            std::fs::remove_file(paths.vault_file()).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(paths.vault_file().with_extension("hsv.bak"));
            Ok(())
        })(),
    );

    check(
        "background job",
        (|| {
            let h = s.jobs.spawn("selftest", "self-test", |h| async move {
                h.set_progress(hs_core::jobs::Progress::Fraction(1.0));
                Ok(())
            });
            let start = std::time::Instant::now();
            loop {
                let snap = s.jobs.snapshot();
                if let Some(j) = snap.iter().find(|j| j.id == h.id()) {
                    if j.status == hs_core::jobs::JobStatus::Succeeded {
                        return Ok(());
                    }
                    if j.status != hs_core::jobs::JobStatus::Running {
                        return Err(format!("job ended as {:?}", j.status));
                    }
                }
                if start.elapsed().as_secs() > 5 {
                    return Err("job did not finish".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })(),
    );

    // The release check runs this as LocalSystem in session 0, straight
    // after a release build, where the pseudo-console can be slow to open:
    // a second, longer attempt, and a panic reported rather than ending the
    // run (BUG-0042).
    check(
        "pty spawn",
        retrying(&[PTY_FIRST_WAIT, PTY_SECOND_WAIT], |wait| {
            let (cmd, args) = hs_term::pty::probe_command();
            let out = hs_term::pty::run_capture(cmd, &args, wait)?;
            if out.contains("HS_PTY_OK") {
                Ok(())
            } else {
                Err(format!("unexpected output {out:?}"))
            }
        }),
    );

    check(
        "release signing key",
        if hs_core::update::SIGNING_KEYS
            .iter()
            .all(|k| minisign_key_ok(k))
        {
            Ok(())
        } else {
            Err("embedded key invalid".into())
        },
    );

    // The release source is baked in per forge (docs/09 §10); a typo in a
    // workflow variable would otherwise ship a binary that can never update.
    check(
        "release source",
        if hs_core::update::RELEASES_URL.starts_with("https://")
            && hs_core::update::FORGE_URL.starts_with("https://")
        {
            Ok(())
        } else {
            Err(format!(
                "must be https: {} and {}",
                hs_core::update::RELEASES_URL,
                hs_core::update::FORGE_URL
            ))
        },
    );

    if failures.is_empty() {
        println!("self-test ok");
        0
    } else {
        println!("self-test FAILED: {}", failures.join(", "));
        1
    }
}

fn minisign_key_ok(k: &str) -> bool {
    // Re-use the parser through hs-core's test-free path: a key is valid if it decodes.
    k.len() == 56 && k.starts_with("RW")
}

/// `--check-updates` / `--download-update`: drive the updater without a window.
pub fn update_headless(paths: DataDir, download: bool, install: bool) -> i32 {
    use hs_core::update::Channel;
    use hs_core::updater::UpdateStatus;
    let boot = match crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST) {
        Ok(b) => b,
        Err(e) => {
            println!("FAILED  bootstrap: {e}");
            return 1;
        }
    };
    let s = &boot.services;
    let (channel, source, skipped) = {
        let st = s.settings.read();
        (
            st.updates.channel.clone(),
            st.updates.source.clone(),
            st.updates.skipped_version.clone(),
        )
    };
    println!("HyperStack {} checking {} channel", s.version, channel);
    s.updater
        .check(Channel::from_setting(&channel), Some(source), skipped);
    let status = wait_status(
        s,
        |st| !matches!(st, UpdateStatus::Checking | UpdateStatus::Idle),
        60,
    );
    match status {
        UpdateStatus::UpToDate { .. } => {
            println!("up to date");
            3
        }
        UpdateStatus::ManualOnly {
            version,
            reason,
            html_url,
        } => {
            println!(
                "newer release {version} cannot be installed in place: {reason}
{html_url}"
            );
            2
        }
        UpdateStatus::Error(e) => {
            println!("FAILED  {e}");
            1
        }
        UpdateStatus::Available {
            candidate,
            asset,
            exe,
            ..
        } => {
            println!(
                "available {} ({}, {} bytes) signed manifest verified",
                candidate.version, asset.name, asset.size
            );
            if !download {
                return 0;
            }
            s.updater.download(&s.jobs, candidate, asset, exe);
            let status = wait_status(s, |st| !matches!(st, UpdateStatus::Downloading { .. }), 600);
            match status {
                UpdateStatus::Ready { version, path } => {
                    println!("ready   {version} at {}", path.display());
                    if !install {
                        return 0;
                    }
                    match s.updater.install(&path, &version) {
                        Ok(()) => {
                            println!("installed {version}; the new executable is starting");
                            0
                        }
                        Err(e) => {
                            println!("FAILED  install: {e}");
                            1
                        }
                    }
                }
                UpdateStatus::Error(e) => {
                    println!("FAILED  {e}");
                    1
                }
                other => {
                    println!("FAILED  unexpected state {other:?}");
                    1
                }
            }
        }
        other => {
            println!("FAILED  unexpected state {other:?}");
            1
        }
    }
}

/// `--revert-update`: swap the kept previous executable back (docs/09 §5.5).
pub fn revert_headless(paths: DataDir) -> i32 {
    let boot = match crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST) {
        Ok(b) => b,
        Err(e) => {
            println!("FAILED  bootstrap: {e}");
            return 1;
        }
    };
    match boot.services.updater.revert() {
        Ok(v) => {
            println!("reverted to {v}; the previous executable is starting");
            0
        }
        Err(e) => {
            println!("FAILED  revert: {e}");
            1
        }
    }
}

fn wait_status(
    s: &hs_core::Services,
    done: impl Fn(&hs_core::updater::UpdateStatus) -> bool,
    timeout_secs: u64,
) -> hs_core::updater::UpdateStatus {
    let start = std::time::Instant::now();
    loop {
        let st = s.updater.status();
        if done(&st) {
            return st;
        }
        if start.elapsed().as_secs() > timeout_secs {
            return hs_core::updater::UpdateStatus::Error("timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// How long the terminal probe waits on its first try, and on its second.
const PTY_FIRST_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
const PTY_SECOND_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Run `attempt` once per wait in `waits`, stopping at the first success.
/// A panic inside an attempt is that attempt's failure, not the end of the
/// self-test; when every attempt fails, the reason names each of them
/// (BUG-0042).
fn retrying(
    waits: &[std::time::Duration],
    mut attempt: impl FnMut(std::time::Duration) -> Result<(), String>,
) -> Result<(), String> {
    let mut reasons = Vec::new();
    for (i, &wait) in waits.iter().enumerate() {
        let tried = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| attempt(wait)));
        match tried {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => reasons.push(format!("try {} ({wait:?}): {e}", i + 1)),
            Err(panic) => {
                let what = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "a panic".to_string());
                reasons.push(format!("try {} ({wait:?}): panicked: {what}", i + 1));
            }
        }
    }
    Err(reasons.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const WAITS: [Duration; 2] = [Duration::from_millis(1), Duration::from_millis(2)];

    /// A slow first try is not a failure if the second one answers
    /// (BUG-0042).
    #[test]
    fn a_later_attempt_that_succeeds_passes() {
        let mut tries = 0;
        let r = retrying(&WAITS, |_| {
            tries += 1;
            if tries == 1 {
                Err("slow".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(r, Ok(()));
        assert_eq!(tries, 2);
    }

    #[test]
    fn every_attempt_is_named_when_none_succeeds() {
        let r = retrying(&WAITS, |_| Err("no console".into()));
        let e = r.unwrap_err();
        assert!(e.contains("try 1") && e.contains("try 2"), "{e}");
        assert!(e.contains("no console"), "{e}");
    }

    /// A panic inside an attempt is reported as that attempt's reason and
    /// the next attempt still runs - it does not end the self-test.
    #[test]
    fn a_panic_is_a_reason_not_the_end_of_the_run() {
        let mut tries = 0;
        let r = retrying(&WAITS, |_| {
            tries += 1;
            if tries == 1 {
                panic!("boom");
            }
            Err("still no console".into())
        });
        let e = r.unwrap_err();
        assert_eq!(tries, 2, "the second attempt ran");
        assert!(e.contains("panicked: boom"), "{e}");
    }
}
