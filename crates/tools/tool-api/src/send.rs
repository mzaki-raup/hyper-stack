//! Sending a prepared request with `reqwest` and capturing the exchange:
//! status, headers, body bytes, timing. Multipart bodies are assembled
//! here (the workspace's reqwest has no `multipart` feature), and a curl
//! command can be produced for the same request.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyData {
    None,
    Text {
        content_type: String,
        text: String,
    },
    Form(Vec<(String, String)>),
    /// Parts: `(name, filename or None, content type, bytes)`.
    Multipart(Vec<(String, Option<String>, String, Vec<u8>)>),
    Binary {
        content_type: String,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyData,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub status: u16,
    pub status_text: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub millis: u64,
    pub remote: String,
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn form_body(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn multipart(parts: &[(String, Option<String>, String, Vec<u8>)]) -> (String, Vec<u8>) {
    let boundary = format!("----HyperStack{}", hs_core::new_id());
    let mut body = Vec::new();
    for (name, filename, ct, bytes) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(f) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\nContent-Type: {ct}\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

pub async fn send(client: &reqwest::Client, p: &Prepared) -> Result<Exchange, String> {
    let method = reqwest::Method::from_bytes(p.method.as_bytes()).map_err(|e| e.to_string())?;
    let mut req = client
        .request(method, &p.url)
        .timeout(Duration::from_secs(p.timeout_secs.max(1)));
    for (k, v) in &p.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req = match &p.body {
        BodyData::None => req,
        BodyData::Text { content_type, text } => {
            if !p
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                req = req.header("Content-Type", content_type.as_str());
            }
            req.body(text.clone())
        }
        BodyData::Form(fields) => req
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_body(fields)),
        BodyData::Multipart(parts) => {
            let (boundary, body) = multipart(parts);
            req.header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
        }
        BodyData::Binary {
            content_type,
            bytes,
        } => {
            if !p
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                req = req.header("Content-Type", content_type.as_str());
            }
            req.body(bytes.clone())
        }
    };
    let start = Instant::now();
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let version = format!("{:?}", resp.version());
    let remote = resp
        .remote_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_string()))
        .collect();
    let body = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
    Ok(Exchange {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        version,
        headers,
        body,
        millis: start.elapsed().as_millis() as u64,
        remote,
    })
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The request as a curl command (secrets already resolved, so copy with care).
pub fn as_curl(p: &Prepared) -> String {
    let mut parts = vec!["curl".to_string()];
    if p.method != "GET" {
        parts.push(format!("-X {}", p.method));
    }
    parts.push(sh_quote(&p.url));
    for (k, v) in &p.headers {
        parts.push(format!("-H {}", sh_quote(&format!("{k}: {v}"))));
    }
    match &p.body {
        BodyData::None => {}
        BodyData::Text { content_type, text } => {
            if !p
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                parts.push(format!(
                    "-H {}",
                    sh_quote(&format!("Content-Type: {content_type}"))
                ));
            }
            parts.push(format!("--data-raw {}", sh_quote(text)));
        }
        BodyData::Form(fields) => {
            for (k, v) in fields {
                parts.push(format!(
                    "--data-urlencode {}",
                    sh_quote(&format!("{k}={v}"))
                ));
            }
        }
        BodyData::Multipart(items) => {
            for (name, filename, _, bytes) in items {
                match filename {
                    Some(f) => parts.push(format!("-F {}", sh_quote(&format!("{name}=@{f}")))),
                    None => parts.push(format!(
                        "-F {}",
                        sh_quote(&format!("{name}={}", String::from_utf8_lossy(bytes)))
                    )),
                }
            }
        }
        BodyData::Binary { .. } => parts.push("--data-binary @file".into()),
    }
    parts.join(" \\\n  ")
}

/// `(method, url, headers, body text)` parsed from a curl command.
pub type CurlParts = (String, String, Vec<(String, String)>, Option<String>);

/// A curl command line back into its parts.
pub fn parse_curl(text: &str) -> Option<CurlParts> {
    let joined = text.replace("\\\n", " ").replace("\\\r\n", " ");
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = joined.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                    }
                }
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                c => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    if tokens.first().map(String::as_str) != Some("curl") {
        return None;
    }
    let mut method = None;
    let mut url = None;
    let mut headers = Vec::new();
    let mut body = None;
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        let next = tokens.get(i + 1).cloned();
        match t {
            "-X" | "--request" => {
                method = next;
                i += 2;
            }
            "-H" | "--header" => {
                if let Some(h) = next
                    && let Some((k, v)) = h.split_once(':')
                {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
                i += 2;
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-urlencode" => {
                body = next;
                i += 2;
            }
            "-u" | "--user" => {
                if let Some(u) = next {
                    use base64::Engine;
                    headers.push((
                        "Authorization".into(),
                        format!(
                            "Basic {}",
                            base64::engine::general_purpose::STANDARD.encode(u.as_bytes())
                        ),
                    ));
                }
                i += 2;
            }
            "--url" => {
                url = next;
                i += 2;
            }
            "-L" | "--location" | "-k" | "--insecure" | "-s" | "--silent" | "-i" | "--include"
            | "--compressed" => {
                i += 1;
            }
            t if t.starts_with('-') => {
                i += 2;
            }
            t => {
                if url.is_none() {
                    url = Some(t.to_string());
                }
                i += 1;
            }
        }
    }
    let url = url?;
    let method = method.unwrap_or_else(|| {
        if body.is_some() {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    Some((method.to_uppercase(), url, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_round_trip() {
        let p = Prepared {
            method: "POST".into(),
            url: "https://api.example.com/v1/orders?x=1".into(),
            headers: vec![("Authorization".into(), "Bearer t".into())],
            body: BodyData::Text {
                content_type: "application/json".into(),
                text: "{\"a\":\"it's\"}".into(),
            },
            timeout_secs: 30,
        };
        let c = as_curl(&p);
        assert!(c.starts_with("curl"));
        assert!(c.contains("-X POST") && c.contains("'https://api.example.com/v1/orders?x=1'"));
        let (m, u, h, b) = parse_curl(&c).unwrap();
        assert_eq!(m, "POST");
        assert_eq!(u, p.url);
        assert_eq!(h[0], ("Authorization".into(), "Bearer t".into()));
        assert_eq!(h[1].0, "Content-Type");
        assert_eq!(b.as_deref(), Some("{\"a\":\"it's\"}"));
        let (m, u, _, _) = parse_curl("curl https://x.test/ping -H \"A: b\"").unwrap();
        assert_eq!((m.as_str(), u.as_str()), ("GET", "https://x.test/ping"));
        assert!(parse_curl("wget x").is_none());
    }

    #[test]
    fn form_and_multipart_bodies() {
        assert_eq!(form_body(&[("a b".into(), "c&d".into())]), "a+b=c%26d");
        let (boundary, body) = multipart(&[
            ("f".into(), None, "text/plain".into(), b"v".to_vec()),
            (
                "file".into(),
                Some("x.bin".into()),
                "application/octet-stream".into(),
                vec![1, 2],
            ),
        ]);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"f\"\r\n\r\nv\r\n"
        )));
        assert!(text.contains("filename=\"x.bin\""));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }
}
