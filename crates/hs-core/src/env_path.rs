//! The `PATH` to search for installed programs, re-readable at runtime.
//!
//! A process keeps the environment it was handed at start, so a language
//! server or debug adapter installed while HyperStack is open stays
//! invisible until it is restarted — which is exactly the moment a user
//! least expects to restart. On Windows the user and machine `Path` values
//! live in the registry and can be read again at any time; [`refresh`] does
//! that and [`current`] hands back the result. Elsewhere there is no such
//! store, so the inherited value stands and `refresh` says nothing changed.
//!
//! Nothing here writes to the process environment: `std::env::set_var` is
//! unsound while other threads are reading, and the only callers that need
//! the value are the lookups in `hs-lsp` and `hs-dap`.

use std::ffi::OsString;
use std::sync::RwLock;

static CURRENT: RwLock<Option<OsString>> = RwLock::new(None);

/// The `PATH` to search: the value from the last successful [`refresh`], or
/// the one this process started with.
pub fn current() -> OsString {
    if let Ok(guard) = CURRENT.read()
        && let Some(p) = guard.as_ref()
    {
        return p.clone();
    }
    inherited()
}

/// Resolve a command on [`current`]'s `PATH`. On Windows the name is tried
/// with each `PATHEXT` suffix (`mysql` finds `mysql.exe`), and a bare name is
/// never accepted: `CreateProcess` cannot start a file without one.
pub fn find(name: &str) -> Option<std::path::PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let path = current();
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    let has_extension = std::path::Path::new(name).extension().is_some();
    for dir in std::env::split_paths(&path) {
        if has_extension || !cfg!(windows) {
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
        }
        if cfg!(windows) {
            for ext in &exts {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Read `PATH` from the system again. Returns whether the value changed, so
/// a caller can say "nothing new" rather than claim the refresh did
/// something.
pub fn refresh() -> bool {
    let Some(fresh) = sys::read() else {
        return false;
    };
    // Whatever this process was given that the system store does not know
    // about (a folder a launching shell added) is kept, after the system's
    // own entries: a lookup that worked before a refresh must still work.
    let mut entries: Vec<OsString> = std::env::split_paths(&fresh)
        .map(|p| p.into_os_string())
        .filter(|e| !e.is_empty())
        .collect();
    let mut known: Vec<String> = entries.iter().map(fold).collect();
    for p in std::env::split_paths(&inherited()) {
        let e = p.into_os_string();
        if e.is_empty() {
            continue;
        }
        let key = fold(&e);
        if !known.contains(&key) {
            known.push(key);
            entries.push(e);
        }
    }
    let Ok(joined) = std::env::join_paths(&entries) else {
        return false;
    };
    let Ok(mut guard) = CURRENT.write() else {
        return false;
    };
    let changed = guard.as_ref() != Some(&joined);
    *guard = Some(joined);
    changed
}

fn inherited() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

/// Compare two entries the way the platform does.
fn fold(p: &OsString) -> String {
    let s = p.to_string_lossy();
    let s = s.trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s.to_string()
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, RegCloseKey,
        RegOpenKeyExW, RegQueryValueExW,
    };

    const MACHINE_ENV: &str = "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment";
    const USER_ENV: &str = "Environment";

    /// The machine's entries first, then the user's: the order Windows
    /// composes them in.
    pub fn read() -> Option<OsString> {
        let machine = value(HKEY_LOCAL_MACHINE, MACHINE_ENV);
        let user = value(HKEY_CURRENT_USER, USER_ENV);
        match (machine, user) {
            (None, None) => None,
            (Some(m), None) => Some(m),
            (None, Some(u)) => Some(u),
            (Some(mut m), Some(u)) => {
                if !m.is_empty() && !u.is_empty() {
                    m.push(";");
                }
                m.push(&u);
                Some(m)
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn value(root: HKEY, subkey: &str) -> Option<OsString> {
        let path = wide(subkey);
        let name = wide("Path");
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
                return None;
            }
            // Ask for the size first, then read into a buffer of it.
            let mut kind = 0u32;
            let mut len = 0u32;
            let sized = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                std::ptr::null_mut(),
                &mut len,
            ) == ERROR_SUCCESS;
            if !sized || len == 0 {
                RegCloseKey(key);
                return None;
            }
            let mut buf = vec![0u8; len as usize];
            let read = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                buf.as_mut_ptr(),
                &mut len,
            ) == ERROR_SUCCESS;
            RegCloseKey(key);
            if !read {
                return None;
            }
            let units: Vec<u16> = buf[..(len as usize).min(buf.len())]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .take_while(|u| *u != 0)
                .collect();
            let raw = OsString::from_wide(&units);
            // A REG_EXPAND_SZ carries %SystemRoot% and friends unexpanded.
            if kind == REG_EXPAND_SZ {
                Some(expand(&raw))
            } else {
                Some(raw)
            }
        }
    }

    fn expand(s: &OsString) -> OsString {
        let src: Vec<u16> = s.encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            let n = ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0);
            if n == 0 {
                return s.clone();
            }
            let mut buf = vec![0u16; n as usize];
            let written = ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), n);
            if written == 0 || written > n {
                return s.clone();
            }
            let end = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
            OsString::from_wide(&buf[..end])
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::OsString;

    /// No system store to re-read: the inherited value is all there is.
    pub fn read() -> Option<OsString> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True before any refresh (it *is* the inherited value) and after one
    /// (which only ever appends), so it does not race the test below for
    /// the static they share.
    #[test]
    fn current_always_covers_what_the_process_was_given() {
        let after: Vec<String> = std::env::split_paths(&current())
            .map(|p| fold(&p.into_os_string()))
            .collect();
        for entry in std::env::split_paths(&inherited())
            .map(|p| fold(&p.into_os_string()))
            .filter(|s| !s.is_empty())
        {
            assert!(after.contains(&entry), "{entry} is not in the search path");
        }
    }

    #[test]
    fn entries_are_folded_the_way_the_platform_compares_them() {
        let (a, b) = if cfg!(windows) {
            ("C:\\Tools\\", "c:\\tools")
        } else {
            ("/opt/tools/", "/opt/tools")
        };
        assert_eq!(fold(&OsString::from(a)), fold(&OsString::from(b)));
        assert_ne!(
            fold(&OsString::from("/opt/tools")),
            fold(&OsString::from("/opt/other"))
        );
    }

    #[test]
    fn a_refresh_keeps_every_inherited_entry() {
        let before: Vec<String> = std::env::split_paths(&inherited())
            .map(|p| fold(&p.into_os_string()))
            .filter(|s| !s.is_empty())
            .collect();
        refresh();
        let after: Vec<String> = std::env::split_paths(&current())
            .map(|p| fold(&p.into_os_string()))
            .collect();
        for entry in before {
            assert!(after.contains(&entry), "{entry} was dropped by the refresh");
        }
    }
}
