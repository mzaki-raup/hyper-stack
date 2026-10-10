//! Embeds build metadata (git sha, build date, channel, target) so the app can
//! show `1.4.0 (a1b2c3d · 2026-10-10 · stable · windows-x64)`.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let date = chrono_free_utc_date();
    let channel = std::env::var("HS_CHANNEL").unwrap_or_else(|_| "dev".to_string());
    let target = std::env::var("TARGET").unwrap_or_default();

    println!("cargo:rustc-env=HS_GIT_SHA={sha}");
    println!("cargo:rustc-env=HS_BUILD_DATE={date}");
    println!("cargo:rustc-env=HS_CHANNEL={channel}");
    println!("cargo:rustc-env=HS_TARGET={target}");
    println!("cargo:rerun-if-env-changed=HS_CHANNEL");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
}

/// YYYY-MM-DD in UTC without pulling chrono into the build script.
fn chrono_free_utc_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
