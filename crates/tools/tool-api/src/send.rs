//! Sending a prepared request with `reqwest` and capturing the exchange:
//! status, headers, body bytes, timing. Multipart bodies are assembled
//! here (the workspace's reqwest has no `multipart` feature), and a curl
//! command can be produced for the same request.

use std::collections::HashMap;
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
    /// Authentication resolved at send time (OAuth 2.0 token, SigV4).
    pub auth: DynamicAuth,
}

/// Authentication that is computed at send time rather than written into
/// the headers beforehand: an OAuth 2.0 token fetched (and cached) from the
/// token endpoint, or an AWS SigV4 signature over the final request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DynamicAuth {
    #[default]
    None,
    OAuth2 {
        token_url: String,
        /// `client_credentials`, `password` or `refresh_token`.
        grant: String,
        client_id: String,
        client_secret: String,
        scope: String,
        audience: String,
        username: String,
        password: String,
        refresh_token: String,
    },
    SigV4 {
        access_key: String,
        secret_key: String,
        session_token: String,
        region: String,
        service: String,
    },
}

/// Tokens by (token URL, client id, grant, scope, username): fetched once
/// and reused until a minute before they expire. One cache for the
/// process, which is what "per environment" means with one send path.
static TOKENS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (String, Instant)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Forget every cached OAuth 2.0 token ("refresh now").
pub fn forget_tokens() {
    TOKENS.lock().unwrap().clear();
}

/// `(scheme, host, path, query pairs)` of a URL, without a URL crate: enough
/// for a signature's canonical request.
pub fn split_url(url: &str) -> (String, String, String, Vec<(String, String)>) {
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let (authority, path_query) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (path, query) = path_query.split_once('?').unwrap_or((path_query, ""));
    let pairs = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (urldecode(k), urldecode(v))
        })
        .collect();
    (
        scheme.to_string(),
        authority.to_string(),
        if path.is_empty() {
            "/".into()
        } else {
            path.to_string()
        },
        pairs,
    )
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() - 1 + 1 => {
                match u8::from_str_radix(&s[i + 1..(i + 3).min(s.len())], 16) {
                    Ok(b) if i + 2 < bytes.len() => {
                        out.push(b);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The bytes a body puts on the wire and their content type (what a
/// signature must cover).
fn body_bytes(p: &Prepared) -> (Option<String>, Vec<u8>) {
    match &p.body {
        BodyData::None => (None, Vec::new()),
        BodyData::Text { content_type, text } => {
            (Some(content_type.clone()), text.clone().into_bytes())
        }
        BodyData::Form(fields) => (
            Some("application/x-www-form-urlencoded".into()),
            form_body(fields).into_bytes(),
        ),
        BodyData::Multipart(parts) => {
            let (boundary, body) = multipart(parts);
            (
                Some(format!("multipart/form-data; boundary={boundary}")),
                body,
            )
        }
        BodyData::Binary {
            content_type,
            bytes,
        } => (Some(content_type.clone()), bytes.clone()),
    }
}

/// The SigV4 headers for `p` with the given body: `host`, `x-amz-date`,
/// `x-amz-content-sha256`, the session token when there is one, and
/// `Authorization`. `now` is the signing time (`%Y%m%dT%H%M%SZ`).
// The signing inputs are what they are; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn sigv4_headers(
    p: &Prepared,
    body: &[u8],
    content_type: Option<&str>,
    access_key: &str,
    secret_key: &str,
    session_token: &str,
    region: &str,
    service: &str,
    now: &str,
) -> Vec<(String, String)> {
    use hs_core::objstore::sigv4;
    let (_, host, path, query) = split_url(&p.url);
    let payload_hash = sigv4::sha256_hex(body);
    let mut signed: Vec<(String, String)> = vec![
        ("host".into(), host),
        ("x-amz-date".into(), now.to_string()),
        ("x-amz-content-sha256".into(), payload_hash.clone()),
    ];
    if let Some(ct) = content_type {
        signed.push(("content-type".into(), ct.to_string()));
    }
    if !session_token.is_empty() {
        signed.push(("x-amz-security-token".into(), session_token.to_string()));
    }
    let canonical_path: String = path
        .split('/')
        .map(|seg| sigv4::uri_encode(seg, true))
        .collect::<Vec<_>>()
        .join("/");
    let creds = sigv4::Credentials {
        access_key: access_key.to_string(),
        secret_key: secret_key.to_string(),
        session_token: (!session_token.is_empty()).then(|| session_token.to_string()),
    };
    let req = sigv4::Request {
        method: &p.method,
        path: &canonical_path,
        query: &query,
        headers: &signed,
        payload_hash: &payload_hash,
    };
    let authorization = sigv4::authorization(&creds, region, service, now, &req);
    let mut out: Vec<(String, String)> = signed
        .into_iter()
        .filter(|(k, _)| k != "host" && k != "content-type")
        .collect();
    out.push(("Authorization".into(), authorization));
    out
}

/// Fetch an OAuth 2.0 token with the client-credentials, password or
/// refresh-token grant (RFC 6749 §4.3, §4.4, §6), cached until a minute
/// before `expires_in`.
#[allow(clippy::too_many_arguments)]
async fn oauth2_token(
    client: &reqwest::Client,
    token_url: &str,
    grant: &str,
    client_id: &str,
    client_secret: &str,
    scope: &str,
    audience: &str,
    username: &str,
    password: &str,
    refresh_token: &str,
) -> Result<String, String> {
    let key = format!("{token_url}|{client_id}|{grant}|{scope}|{username}");
    if let Some((tok, until)) = TOKENS.lock().unwrap().get(&key)
        && Instant::now() < *until
    {
        return Ok(tok.clone());
    }
    let grant_type = match grant {
        "password" => "password",
        "refresh_token" => "refresh_token",
        _ => "client_credentials",
    };
    let mut fields: Vec<(String, String)> = vec![("grant_type".into(), grant_type.into())];
    if grant_type == "password" {
        fields.push(("username".into(), username.into()));
        fields.push(("password".into(), password.into()));
    }
    if grant_type == "refresh_token" {
        fields.push(("refresh_token".into(), refresh_token.into()));
    }
    if !scope.trim().is_empty() {
        fields.push(("scope".into(), scope.trim().into()));
    }
    if !audience.trim().is_empty() {
        fields.push(("audience".into(), audience.trim().into()));
    }
    // The client authenticates with HTTP Basic (RFC 6749 §2.3.1), and the
    // id goes in the body too for the servers that read it there.
    fields.push(("client_id".into(), client_id.into()));
    let mut req = client
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form_body(&fields));
    if !client_secret.is_empty() {
        req = req.basic_auth(client_id, Some(client_secret));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("token request: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        let why = v
            .get("error_description")
            .or(v.get("error"))
            .and_then(|e| e.as_str())
            .unwrap_or(text.trim());
        return Err(format!(
            "token endpoint answered {}: {}",
            status.as_u16(),
            why
        ));
    }
    let token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "token endpoint returned no access_token".to_string())?
        .to_string();
    let expires = v
        .get("expires_in")
        .and_then(|e| e.as_u64())
        .unwrap_or(3600)
        .saturating_sub(60)
        .max(5);
    TOKENS.lock().unwrap().insert(
        key,
        (token.clone(), Instant::now() + Duration::from_secs(expires)),
    );
    Ok(token)
}

/// Resolve the dynamic authentication into concrete headers: the OAuth 2.0
/// bearer token, or the SigV4 signature over this exact body.
pub async fn dynamic_headers(
    client: &reqwest::Client,
    p: &Prepared,
) -> Result<Vec<(String, String)>, String> {
    match &p.auth {
        DynamicAuth::None => Ok(Vec::new()),
        DynamicAuth::OAuth2 {
            token_url,
            grant,
            client_id,
            client_secret,
            scope,
            audience,
            username,
            password,
            refresh_token,
        } => {
            let token = oauth2_token(
                client,
                token_url,
                grant,
                client_id,
                client_secret,
                scope,
                audience,
                username,
                password,
                refresh_token,
            )
            .await?;
            Ok(vec![("Authorization".into(), format!("Bearer {token}"))])
        }
        DynamicAuth::SigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
        } => {
            let (content_type, body) = body_bytes(p);
            // A Content-Type the user typed wins over the body's own.
            let ct = p
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone())
                .or(content_type);
            let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            Ok(sigv4_headers(
                p,
                &body,
                ct.as_deref(),
                access_key,
                secret_key,
                session_token,
                region,
                service,
                &now,
            ))
        }
    }
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
    for (k, v) in dynamic_headers(client, p).await? {
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
            auth: DynamicAuth::None,
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

    #[test]
    fn urls_split_into_host_path_and_query() {
        let (scheme, host, path, q) = split_url("https://api.example.com/v1/a%20b?x=1&y=two+words");
        assert_eq!(scheme, "https");
        assert_eq!(host, "api.example.com");
        assert_eq!(path, "/v1/a%20b");
        assert_eq!(
            q,
            vec![
                ("x".to_string(), "1".to_string()),
                ("y".to_string(), "two words".to_string())
            ]
        );
        let (_, host, path, q) = split_url("http://localhost:8080");
        assert_eq!((host.as_str(), path.as_str()), ("localhost:8080", "/"));
        assert!(q.is_empty());
    }

    #[test]
    fn sigv4_signs_the_exact_body_and_lists_the_headers() {
        let p = Prepared {
            method: "POST".into(),
            url: "https://abc123.execute-api.eu-west-1.amazonaws.com/prod/orders?limit=5".into(),
            headers: Vec::new(),
            body: BodyData::Text {
                content_type: "application/json".into(),
                text: "{\"a\":1}".into(),
            },
            timeout_secs: 30,
            auth: DynamicAuth::None,
        };
        let h = sigv4_headers(
            &p,
            b"{\"a\":1}",
            Some("application/json"),
            "AKIAEXAMPLE",
            "secret",
            "",
            "eu-west-1",
            "execute-api",
            "20261022T120000Z",
        );
        let get = |k: &str| {
            h.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("x-amz-date"), "20261022T120000Z");
        assert_eq!(
            get("x-amz-content-sha256"),
            hs_core::objstore::sigv4::sha256_hex(b"{\"a\":1}")
        );
        let auth = get("Authorization");
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIAEXAMPLE/20261022/eu-west-1/execute-api/aws4_request, "
        ));
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date,"));
        assert!(h.iter().all(|(k, _)| k != "x-amz-security-token"));
        // A different body changes the signature; a session token is signed in.
        let other = sigv4_headers(
            &p,
            b"{\"a\":2}",
            Some("application/json"),
            "AKIAEXAMPLE",
            "secret",
            "tok",
            "eu-west-1",
            "execute-api",
            "20261022T120000Z",
        );
        assert_ne!(other.last().unwrap().1, auth);
        assert!(
            other
                .iter()
                .any(|(k, v)| k == "x-amz-security-token" && v == "tok")
        );
        assert!(other.last().unwrap().1.contains("x-amz-security-token"));
    }

    #[test]
    fn oauth2_tokens_are_fetched_with_the_grant_and_cached() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let (h2, s2) = (hits.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut len = 0usize;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    let h = h.trim_end().to_string();
                    if h.is_empty() {
                        break;
                    }
                    if let Some(v) = h
                        .strip_prefix("content-length: ")
                        .or(h.strip_prefix("Content-Length: "))
                    {
                        len = v.parse().unwrap();
                    }
                    if let Some(v) = h
                        .strip_prefix("authorization: ")
                        .or(h.strip_prefix("Authorization: "))
                    {
                        auth = v.to_string();
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                h2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                s2.lock()
                    .unwrap()
                    .push((auth, String::from_utf8_lossy(&body).into_owned()));
                let out = br#"{"access_token":"tok-1","token_type":"Bearer","expires_in":3600}"#;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(out).unwrap();
            }
        });
        let p = Prepared {
            method: "GET".into(),
            url: "http://example.invalid/".into(),
            headers: Vec::new(),
            body: BodyData::None,
            timeout_secs: 30,
            auth: DynamicAuth::OAuth2 {
                token_url: format!("{base}/oauth/token"),
                grant: "client_credentials".into(),
                client_id: "cid".into(),
                client_secret: "sec".into(),
                scope: "read write".into(),
                audience: "https://api".into(),
                username: String::new(),
                password: String::new(),
                refresh_token: String::new(),
            },
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = reqwest::Client::new();
        forget_tokens();
        let h = rt.block_on(dynamic_headers(&client, &p)).unwrap();
        assert_eq!(
            h,
            vec![("Authorization".to_string(), "Bearer tok-1".to_string())]
        );
        let again = rt.block_on(dynamic_headers(&client, &p)).unwrap();
        assert_eq!(again, h);
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the second send reuses the token"
        );
        let (auth, body) = seen.lock().unwrap()[0].clone();
        assert!(auth.starts_with("Basic "));
        assert!(body.contains("grant_type=client_credentials"));
        assert!(body.contains("scope=read+write"));
        assert!(body.contains("audience=https%3A%2F%2Fapi"));
        assert!(body.contains("client_id=cid"));
        forget_tokens();
        rt.block_on(dynamic_headers(&client, &p)).unwrap();
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "forgetting the cache fetches again"
        );
    }
}
