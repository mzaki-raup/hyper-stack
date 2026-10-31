//! The HTTP plumbing the REST-based activities share (Automation,
//! Tracker; ADR-029): one client, the credential put on each request the
//! way the service wants it, and the server's refusals said in words.

use std::time::Duration;

use reqwest::{Method, RequestBuilder, Response};
use serde_json::Value;

use crate::service_auth::{Auth, TokenAs};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    pub base: String,
    auth: Auth,
    token_as: TokenAs,
    /// Headers every request carries (GitHub's Accept and API version).
    headers: Vec<(&'static str, &'static str)>,
}

/// A refusal in words. The body is the server's own explanation where it
/// gave one, trimmed.
pub fn status_error(status: reqwest::StatusCode, body: &str) -> String {
    let said = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            ["message", "detail", "error", "reason"]
                .iter()
                .find_map(|k| v.get(*k).and_then(Value::as_str).map(String::from))
        })
        .unwrap_or_else(|| {
            let t = body.trim();
            // An HTML error page says nothing useful in a toast.
            if t.starts_with('<') {
                String::new()
            } else {
                t.chars().take(200).collect()
            }
        });
    let what = match status.as_u16() {
        401 => "the server refused the credentials (401)".to_string(),
        403 => "not allowed (403)".to_string(),
        404 => "not found (404) - the server may not offer this".to_string(),
        code => format!("the server answered {code}"),
    };
    if said.is_empty() {
        what
    } else {
        format!("{what}: {said}")
    }
}

/// A request that never got an answer, in words: reqwest says "error
/// sending request for url", which does not say whether anything is
/// listening.
pub fn transport_error(e: &reqwest::Error) -> String {
    let at = e
        .url()
        .map(|u| {
            let host = u.host_str().unwrap_or_default();
            match u.port() {
                Some(p) => format!("{host}:{p}"),
                None => host.to_string(),
            }
        })
        .unwrap_or_default();
    if e.is_timeout() {
        format!("{at} did not answer in time")
    } else if e.is_connect() {
        format!("nothing answered at {at}: is the server running, and is the URL right?")
    } else {
        e.to_string()
    }
}

impl Http {
    pub fn new(base: &str, auth: Auth, token_as: TokenAs, timeout: Duration) -> Result<Http> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            // GitHub refuses a request without one.
            .user_agent(concat!("HyperStack/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Http {
            client,
            base: base.trim_end_matches('/').to_string(),
            auth,
            token_as,
            headers: Vec::new(),
        })
    }

    pub fn with_header(mut self, k: &'static str, v: &'static str) -> Self {
        self.headers.push((k, v));
        self
    }

    /// A request to `path` (joined to the base) or to a full URL.
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{path}", self.base)
        };
        let mut rb = self.client.request(method, url);
        for (k, v) in &self.headers {
            rb = rb.header(*k, *v);
        }
        self.auth.apply(rb, self.token_as)
    }

    /// Send, and turn a refusal into words.
    pub async fn send(&self, rb: RequestBuilder) -> Result<Response> {
        let resp = rb.send().await.map_err(|e| transport_error(&e))?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(status_error(status, &body))
    }

    pub async fn json(&self, rb: RequestBuilder) -> Result<Value> {
        let text = self
            .send(rb)
            .await?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| format!("the server's answer was not JSON: {e}"))
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        self.json(self.request(Method::GET, path)).await
    }

    pub async fn get_text(&self, path: &str) -> Result<String> {
        self.send(self.request(Method::GET, path))
            .await?
            .text()
            .await
            .map_err(|e| e.to_string())
    }
}

/// A string field, or "".
pub fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// A path segment, percent-encoded.
pub fn seg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_what_the_server_said() {
        let e = status_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"message":"Resource not accessible by integration"}"#,
        );
        assert_eq!(
            e,
            "not allowed (403): Resource not accessible by integration"
        );
        // An HTML error page is not quoted.
        assert_eq!(
            status_error(reqwest::StatusCode::UNAUTHORIZED, "<html>…</html>"),
            "the server refused the credentials (401)"
        );
    }

    #[test]
    fn a_server_that_is_not_there_is_named() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        // Port 9 (discard) is closed on any machine that runs these tests.
        let e = rt
            .block_on(reqwest::Client::new().get("http://127.0.0.1:9/").send())
            .unwrap_err();
        assert_eq!(
            transport_error(&e),
            "nothing answered at 127.0.0.1:9: is the server running, and is the URL right?"
        );
    }

    #[test]
    fn a_path_segment_is_encoded() {
        assert_eq!(seg("group/app"), "group%2Fapp");
        assert_eq!(seg("deploy staging"), "deploy%20staging");
    }
}
