//! What the preview column can show for an object (docs/tools/s3.md):
//! text, an image or rendered Markdown, decided by the key's extension
//! and capped in size so a click never pulls a large object.

/// The most a preview fetches; a larger object says so instead.
pub const PREVIEW_LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Markdown,
    Image,
}

/// What a click on an object leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Fetch(Kind),
    TooLarge,
    Unsupported,
}

const TEXT: &[&str] = &[
    "txt",
    "log",
    "json",
    "jsonl",
    "ndjson",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "env",
    "csv",
    "tsv",
    "xml",
    "html",
    "htm",
    "css",
    "js",
    "mjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "py",
    "go",
    "java",
    "kt",
    "c",
    "h",
    "cpp",
    "hpp",
    "cs",
    "rb",
    "php",
    "sh",
    "bash",
    "ps1",
    "bat",
    "sql",
    "properties",
    "gradle",
    "tf",
    "hcl",
    "dockerfile",
    "gitignore",
    "lua",
    "swift",
    "scala",
    "r",
    "pl",
    "proto",
    "graphql",
    "vue",
    "svelte",
];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp"];
const MARKDOWN: &[&str] = &["md", "markdown", "mdown"];

/// The key's last path segment's extension, lower-cased; a dot-file
/// (`.env`, `Dockerfile`) is read by its whole name.
fn extension(key: &str) -> String {
    let name = key.rsplit('/').next().unwrap_or(key).to_ascii_lowercase();
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_string(),
        Some((_, ext)) => ext.to_string(),
        None => name,
    }
}

pub fn kind_for(key: &str) -> Option<Kind> {
    let ext = extension(key);
    let ext = ext.as_str();
    if MARKDOWN.contains(&ext) {
        Some(Kind::Markdown)
    } else if IMAGE.contains(&ext) {
        Some(Kind::Image)
    } else if TEXT.contains(&ext) {
        Some(Kind::Text)
    } else {
        None
    }
}

pub fn plan(key: &str, size: u64) -> Plan {
    match kind_for(key) {
        None => Plan::Unsupported,
        Some(_) if size > PREVIEW_LIMIT => Plan::TooLarge,
        Some(k) => Plan::Fetch(k),
    }
}

/// The cache identity of one object's preview: the same key with a new
/// ETag is a new object and is fetched again.
pub fn cache_id(bucket: &str, key: &str, etag: &str) -> String {
    format!("{bucket}/{key}#{etag}")
}

/// The uri an image preview is loaded under; the extension tells egui's
/// loaders the format.
pub fn image_uri(id: &str, key: &str) -> String {
    format!(
        "bytes://s3-preview/{}.{}",
        hs_core::storage::slug(id),
        extension(key)
    )
}

/// Each line's byte range in `text`, without its line break (`\n` or
/// `\r\n`), found once so a text preview draws only the lines in view.
pub fn line_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::with_capacity(text.len() / 40 + 1);
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            let end = if i > start && text.as_bytes()[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            out.push(start..end);
            start = i + 1;
        }
    }
    if start < text.len() || out.is_empty() {
        out.push(start..text.len());
    }
    out
}

/// Whether `bytes` start like an image egui can draw (PNG, JPEG, GIF,
/// WebP, BMP, ICO or SVG). An object named `.png` that is not one used to
/// show egui's bare red triangle; now it says so in words (2026-11-07).
pub fn looks_like_image(bytes: &[u8]) -> bool {
    let starts = |sig: &[u8]| bytes.starts_with(sig);
    if starts(b"\x89PNG\r\n\x1a\n")
        || starts(&[0xFF, 0xD8, 0xFF])
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || starts(b"BM")
        || starts(&[0, 0, 1, 0])
        || (bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
    {
        return true;
    }
    // SVG is text: an XML prolog or comments may come first.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    head.trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
        && head.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_preview_is_split_into_lines_once() {
        let text = "one\r\ntwo\n\nlast";
        let lines: Vec<&str> = line_ranges(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(lines, ["one", "two", "", "last"]);
        assert_eq!(line_ranges("").len(), 1);
        assert_eq!(line_ranges("a\n").len(), 1);
    }

    #[test]
    fn picks_the_preview_by_extension() {
        assert_eq!(kind_for("logs/app.LOG"), Some(Kind::Text));
        assert_eq!(kind_for("a/b/config.yaml"), Some(Kind::Text));
        assert_eq!(kind_for("README.md"), Some(Kind::Markdown));
        assert_eq!(kind_for("img/logo.SVG"), Some(Kind::Image));
        assert_eq!(kind_for("photo.webp"), Some(Kind::Image));
        assert_eq!(kind_for(".env"), Some(Kind::Text));
        assert_eq!(kind_for("backup.tar.gz"), None);
        assert_eq!(kind_for("noext"), None);
    }

    #[test]
    fn a_large_object_is_not_fetched() {
        assert_eq!(plan("a.json", PREVIEW_LIMIT), Plan::Fetch(Kind::Text));
        assert_eq!(plan("a.json", PREVIEW_LIMIT + 1), Plan::TooLarge);
        assert_eq!(plan("a.bin", 10), Plan::Unsupported);
    }

    #[test]
    fn a_new_etag_is_a_new_preview() {
        assert_ne!(cache_id("b", "k", "1"), cache_id("b", "k", "2"));
        assert!(image_uri("b/k#1", "x/pic.png").ends_with(".png"));
    }

    #[test]
    fn an_image_is_told_by_its_first_bytes_not_its_name() {
        assert!(looks_like_image(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"));
        assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(looks_like_image(b"GIF89a...."));
        assert!(looks_like_image(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(looks_like_image(
            b"<?xml version=\"1.0\"?>\n<svg xmlns=\"x\"/>"
        ));
        assert!(!looks_like_image(b"hello from images/logo.png"));
        assert!(!looks_like_image(b""));
        assert!(!looks_like_image(b"<html><body>not an svg</body></html>"));
    }
}
