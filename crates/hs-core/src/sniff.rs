//! Content sniffing for binary values: what a blob, a decoded Base64 string
//! or a downloaded object most likely is, from its leading bytes. Used by
//! the database cell viewer, the Base64 utility and the S3 browser so a
//! picture is shown as a picture and a PDF is offered as a file instead of
//! a hex dump.

/// Extensions of pictures the graphics tool opens; the editor hands these
/// over instead of showing bytes.
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico"];

/// Whether a path has a picture extension (see [`IMAGE_EXTS`]).
pub fn is_image_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A raster image egui can decode (PNG, JPEG, GIF, BMP, WEBP, ICO).
    Image,
    Svg,
    Pdf,
    /// ZIP container: also DOCX/XLSX/PPTX/JAR/APK.
    Zip,
    Gzip,
    Rtf,
    /// Legacy OLE2 Office documents (DOC/XLS/PPT/MSG).
    Ole,
    /// Valid UTF-8 with no control characters: text, JSON, XML, CSV …
    Text,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sniffed {
    pub kind: Kind,
    pub mime: &'static str,
    /// Extension without the dot, for Save As.
    pub ext: &'static str,
    /// Human label ("PNG image", "PDF document", "UTF-8 text").
    pub label: &'static str,
}

fn is(bytes: &[u8], magic: &[u8]) -> bool {
    bytes.len() >= magic.len() && &bytes[..magic.len()] == magic
}

pub fn sniff(bytes: &[u8]) -> Sniffed {
    let s = |kind, mime, ext, label| Sniffed {
        kind,
        mime,
        ext,
        label,
    };
    if is(bytes, b"\x89PNG\r\n\x1a\n") {
        return s(Kind::Image, "image/png", "png", "PNG image");
    }
    if is(bytes, b"\xFF\xD8\xFF") {
        return s(Kind::Image, "image/jpeg", "jpg", "JPEG image");
    }
    if is(bytes, b"GIF87a") || is(bytes, b"GIF89a") {
        return s(Kind::Image, "image/gif", "gif", "GIF image");
    }
    if is(bytes, b"BM") && bytes.len() > 14 {
        return s(Kind::Image, "image/bmp", "bmp", "BMP image");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return s(Kind::Image, "image/webp", "webp", "WebP image");
    }
    if is(bytes, b"\x00\x00\x01\x00") {
        return s(Kind::Image, "image/x-icon", "ico", "ICO icon");
    }
    if is(bytes, b"%PDF") {
        return s(Kind::Pdf, "application/pdf", "pdf", "PDF document");
    }
    if is(bytes, b"PK\x03\x04") {
        return s(
            Kind::Zip,
            "application/zip",
            "zip",
            "ZIP archive (or Office document)",
        );
    }
    if is(bytes, b"\x1F\x8B") {
        return s(Kind::Gzip, "application/gzip", "gz", "gzip archive");
    }
    if is(bytes, b"{\\rtf") {
        return s(Kind::Rtf, "application/rtf", "rtf", "RTF document");
    }
    if is(bytes, b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return s(
            Kind::Ole,
            "application/msword",
            "doc",
            "Office document (OLE)",
        );
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let head = text.trim_start();
        if head.starts_with("<svg") || (head.starts_with("<?xml") && text.contains("<svg")) {
            return s(Kind::Svg, "image/svg+xml", "svg", "SVG image");
        }
        if !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            let (mime, ext, label) = if head.starts_with('{') || head.starts_with('[') {
                ("application/json", "json", "JSON text")
            } else if head.starts_with('<') {
                ("application/xml", "xml", "XML text")
            } else {
                ("text/plain", "txt", "UTF-8 text")
            };
            return s(Kind::Text, mime, ext, label);
        }
    }
    s(
        Kind::Binary,
        "application/octet-stream",
        "bin",
        "binary data",
    )
}

/// `offset  hex bytes  ascii` lines, at most `max_rows` rows of 16 bytes.
pub fn hex_dump(bytes: &[u8], max_rows: usize) -> String {
    let mut out = String::new();
    for (n, chunk) in bytes.chunks(16).take(max_rows).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|x| format!("{x:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&x| {
                if (32..127).contains(&x) {
                    x as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{:08x}  {:<48}  {ascii}\n", n * 16, hex.join(" ")));
    }
    if bytes.len() > max_rows * 16 {
        out.push_str(&format!("… {} more bytes\n", bytes.len() - max_rows * 16));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_numbers_are_recognised() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n....").kind, Kind::Image);
        assert_eq!(sniff(b"%PDF-1.7").ext, "pdf");
        assert_eq!(sniff(b"PK\x03\x04rest").kind, Kind::Zip);
        assert_eq!(sniff(b"{\"a\":1}").ext, "json");
        assert_eq!(sniff(b"<svg xmlns='x'/>").kind, Kind::Svg);
        assert_eq!(sniff(b"hello\n").label, "UTF-8 text");
        assert_eq!(sniff(&[0, 1, 2, 3, 0xFF]).kind, Kind::Binary);
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 ").mime, "image/webp");
    }

    #[test]
    fn hex_dump_truncates() {
        let d = hex_dump(&[0u8; 40], 2);
        assert!(d.starts_with("00000000  00 00"));
        assert!(d.contains("… 8 more bytes"));
    }
}
