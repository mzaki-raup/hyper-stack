//! `file:` URIs without the `url` crate: enough for local paths on
//! Windows and Unix, percent-encoded the way servers expect.

use std::path::{Path, PathBuf};

/// `D:\proj\src\main.rs` → `file:///D:/proj/src/main.rs`;
/// `/home/u/a b.rs` → `file:///home/u/a%20b.rs`.
pub fn from_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The path of a `file:` URI, `None` for any other scheme.
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://host/share` is a UNC path; `file:///C:/x` a local one.
    let (host, path) = match rest.find('/') {
        Some(0) => ("", rest),
        Some(i) => (&rest[..i], &rest[i..]),
        None => ("", rest),
    };
    let decoded = decode(path);
    let mut text = if host.is_empty() || host == "localhost" {
        decoded
    } else {
        format!("//{host}{decoded}")
    };
    // `/C:/x` → `C:/x` on Windows.
    if cfg!(windows) {
        let bytes = text.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[2] == b':'
            && bytes[1].is_ascii_alphabetic()
        {
            text.remove(0);
        }
        text = text.replace('/', "\\");
    }
    Some(PathBuf::from(text))
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_round_trip() {
        let p = Path::new(r"D:\proj\src\a b.rs");
        let u = from_path(p);
        assert_eq!(u, "file:///D:/proj/src/a%20b.rs");
        if cfg!(windows) {
            assert_eq!(to_path(&u).unwrap(), PathBuf::from(r"D:\proj\src\a b.rs"));
            assert_eq!(
                to_path("file:///c%3A/Users/x.rs").unwrap(),
                PathBuf::from(r"c:\Users\x.rs")
            );
        }
    }

    #[test]
    fn unix_paths_round_trip() {
        let u = from_path(Path::new("/home/u/a b.rs"));
        assert_eq!(u, "file:///home/u/a%20b.rs");
        if !cfg!(windows) {
            assert_eq!(to_path(&u).unwrap(), PathBuf::from("/home/u/a b.rs"));
        }
        assert!(to_path("https://x").is_none());
    }
}
