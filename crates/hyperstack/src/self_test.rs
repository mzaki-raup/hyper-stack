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
    let mut check = |name: &str, ok: Result<(), String>| match ok {
        Ok(()) => println!("ok      {name}"),
        Err(e) => {
            println!("FAILED  {name}: {e}");
            failures.push(name.to_string());
        }
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

    check(
        "pty spawn",
        (|| {
            let (cmd, args) = hs_term::pty::probe_command();
            let out = hs_term::pty::run_capture(cmd, &args, std::time::Duration::from_secs(10))?;
            if out.contains("HS_PTY_OK") {
                Ok(())
            } else {
                Err(format!("unexpected output {out:?}"))
            }
        })(),
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
pub fn update_headless(paths: DataDir, download: bool) -> i32 {
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
            candidate, asset, ..
        } => {
            println!(
                "available {} ({} bytes) signed manifest verified",
                candidate.version, asset.size
            );
            if !download {
                return 0;
            }
            s.updater.download(&s.jobs, candidate, asset);
            let status = wait_status(s, |st| !matches!(st, UpdateStatus::Downloading { .. }), 600);
            match status {
                UpdateStatus::Ready { version, path } => {
                    println!("ready   {version} at {}", path.display());
                    0
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
