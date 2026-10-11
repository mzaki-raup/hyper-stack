//! The optional AI assistant (ADR-019, docs/tools/editor.md §10): a streaming
//! chat client for 1min.AI and any OpenAI-compatible service, the prompt
//! that asks a model for reviewable edits, and the parser that turns its
//! answer into edits the editor can apply.
//!
//! Nothing here touches the UI or the vault: the caller resolves the API
//! key and runs [`chat`] on the runtime, receiving text as it streams.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use edits::{EditBlock, LocateError, Segment, apply_block, locate, segments};

/// Default endpoint of each provider (the settings field may override it).
pub const ONEMIN_BASE: &str = "https://api.1min.ai";
pub const OPENAI_BASE: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// 1min.AI's own API (`/api/chat-with-ai`, `API-KEY` header).
    #[default]
    #[serde(rename = "1min")]
    OneMin,
    /// `POST {base}/chat/completions` with a bearer token: OpenAI, OpenRouter,
    /// Groq, Ollama, LM Studio, vLLM and the like.
    OpenAi,
}

impl Provider {
    pub fn all() -> [Provider; 2] {
        [Provider::OneMin, Provider::OpenAi]
    }
    pub fn default_base(self) -> &'static str {
        match self {
            Provider::OneMin => ONEMIN_BASE,
            Provider::OpenAi => OPENAI_BASE,
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::OneMin => "gpt-4o-mini",
            Provider::OpenAi => "gpt-4o-mini",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiConfig {
    pub provider: Provider,
    /// Empty = the provider's default.
    pub base_url: String,
    pub model: String,
}

impl AiConfig {
    fn base(&self) -> String {
        let b = self.base_url.trim();
        let b = if b.is_empty() {
            self.provider.default_base()
        } else {
            b
        };
        b.trim_end_matches('/').to_string()
    }
    fn model(&self) -> &str {
        let m = self.model.trim();
        if m.is_empty() {
            self.provider.default_model()
        } else {
            m
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("the service answered {status}: {message}")]
    Http { status: u16, message: String },
    #[error("could not reach the service: {0}")]
    Network(String),
    #[error("unexpected answer from the service: {0}")]
    Format(String),
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // A long answer streams for minutes; only a silent connection fails.
        .read_timeout(Duration::from_secs(120))
        .user_agent(concat!("HyperStack/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// Send the conversation and stream the answer: `on_text` gets each piece
/// as it arrives; the whole answer is returned at the end.
pub async fn chat(
    http: &reqwest::Client,
    cfg: &AiConfig,
    api_key: &str,
    messages: &[Message],
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<String, AiError> {
    let req = match cfg.provider {
        Provider::OpenAi => {
            let body = json!({
                "model": cfg.model(),
                "stream": true,
                "messages": messages
                    .iter()
                    .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
                    .collect::<Vec<_>>(),
            });
            let mut r = http
                .post(format!("{}/chat/completions", cfg.base()))
                .json(&body);
            if !api_key.is_empty() {
                r = r.bearer_auth(api_key);
            }
            r
        }
        Provider::OneMin => {
            let body = json!({
                "type": "UNIFY_CHAT_WITH_AI",
                "model": cfg.model(),
                "promptObject": { "prompt": flatten(messages) },
            });
            http.post(format!("{}/api/chat-with-ai?isStreaming=true", cfg.base()))
                .header("API-KEY", api_key)
                .json(&body)
        }
    };
    let resp = req
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AiError::Http {
            status: status.as_u16(),
            message: error_message(&text),
        });
    }
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("event-stream"));
    if !is_sse {
        // A service that ignores the streaming request answers in one go.
        let v: Value = resp
            .json()
            .await
            .map_err(|e| AiError::Format(e.to_string()))?;
        let text = whole_answer(cfg.provider, &v)
            .ok_or_else(|| AiError::Format(truncate(&v.to_string(), 300)))?;
        on_text(&text);
        return Ok(text);
    }
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut sse = SseParser::default();
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AiError::Network(e.to_string()))?;
        for ev in sse.push(&chunk) {
            match interpret(cfg.provider, &ev, out.is_empty())? {
                Step::Text(t) => {
                    on_text(&t);
                    out.push_str(&t);
                }
                Step::Done => return Ok(out),
                Step::Skip => {}
            }
        }
    }
    Ok(out)
}

/// Model ids the service offers (`GET {base}/models`, OpenAI-compatible
/// services only; LM Studio with JIT loading lists every downloaded model,
/// Ollama its pulled ones). 1min.AI has no such endpoint: `Ok(vec![])`.
pub async fn list_models(
    http: &reqwest::Client,
    cfg: &AiConfig,
    api_key: &str,
) -> Result<Vec<String>, AiError> {
    if cfg.provider == Provider::OneMin {
        return Ok(Vec::new());
    }
    let mut r = http.get(format!("{}/models", cfg.base()));
    if !api_key.is_empty() {
        r = r.bearer_auth(api_key);
    }
    let resp = r
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AiError::Http {
            status: status.as_u16(),
            message: error_message(&text),
        });
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| AiError::Format(e.to_string()))?;
    Ok(model_ids(&v))
}

fn model_ids(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = v
        .get("data")
        .or(v.get("models"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    m.get("id")
                        .or(m.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

/// 1min.AI takes one prompt, not a message list: fold the conversation into
/// a transcript (the instructions first, then the turns in order).
pub fn flatten(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        match m.role {
            Role::System => out.push_str(&m.content),
            Role::User => {
                out.push_str("### User\n");
                out.push_str(&m.content);
            }
            Role::Assistant => {
                out.push_str("### Assistant\n");
                out.push_str(&m.content);
            }
        }
    }
    if messages.last().is_some_and(|m| m.role == Role::User) {
        out.push_str("\n\n### Assistant\n");
    }
    out
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// The readable part of an error body (`{"error":{"message":…}}`,
/// `{"message":…}`), else the body itself, shortened.
fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        for p in ["/error/message", "/message", "/error"] {
            if let Some(s) = v.pointer(p).and_then(Value::as_str) {
                return truncate(s, 300);
            }
        }
    }
    let b = body.trim();
    if b.is_empty() {
        "no details".into()
    } else {
        truncate(b, 300)
    }
}

/// The answer text of a non-streamed reply.
fn whole_answer(provider: Provider, v: &Value) -> Option<String> {
    match provider {
        Provider::OpenAi => v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string),
        Provider::OneMin => {
            let r = v.pointer("/aiRecord/aiRecordDetail/resultObject")?;
            Some(match r {
                Value::Array(items) => items
                    .iter()
                    .map(|i| i.as_str().map(str::to_string).unwrap_or(i.to_string()))
                    .collect::<Vec<_>>()
                    .join("\n"),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Server-sent events from a byte stream that may split anywhere (even
/// inside a UTF-8 sequence).
#[derive(Debug, Default)]
pub struct SseParser {
    pending: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(nl) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() || self.event.is_some() {
                    out.push(SseEvent {
                        event: self.event.take(),
                        data: std::mem::take(&mut self.data).join("\n"),
                    });
                }
            } else if let Some(v) = line.strip_prefix("event:") {
                self.event = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("data:") {
                self.data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
            }
            // `id:`, `retry:` and `:` comments are ignored.
        }
        out
    }
}

enum Step {
    Text(String),
    Done,
    Skip,
}

fn interpret(provider: Provider, ev: &SseEvent, nothing_yet: bool) -> Result<Step, AiError> {
    let data = ev.data.trim();
    match provider {
        Provider::OpenAi => {
            if data == "[DONE]" {
                return Ok(Step::Done);
            }
            if data.is_empty() {
                return Ok(Step::Skip);
            }
            let v: Value =
                serde_json::from_str(data).map_err(|e| AiError::Format(e.to_string()))?;
            if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
                return Err(AiError::Format(m.to_string()));
            }
            Ok(v.pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| Step::Text(s.to_string()))
                .unwrap_or(Step::Skip))
        }
        Provider::OneMin => match ev.event.as_deref() {
            Some("done") => Ok(Step::Done),
            Some("error") => {
                let msg = serde_json::from_str::<Value>(data)
                    .ok()
                    .and_then(|v| {
                        v.get("message")
                            .or(v.get("error"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| truncate(data, 300));
                Err(AiError::Format(msg))
            }
            // The final record repeats the answer: use it only when no
            // pieces streamed.
            Some("result") if nothing_yet => Ok(serde_json::from_str::<Value>(data)
                .ok()
                .and_then(|v| whole_answer(Provider::OneMin, &v))
                .map(Step::Text)
                .unwrap_or(Step::Skip)),
            Some("content") | None => Ok(serde_json::from_str::<Value>(data)
                .ok()
                .and_then(|v| v.get("content").and_then(Value::as_str).map(str::to_string))
                .filter(|s| !s.is_empty())
                .map(Step::Text)
                .unwrap_or(Step::Skip)),
            _ => Ok(Step::Skip),
        },
    }
}

/// Instructions for the model: review and explain in Markdown, and express
/// every change to the open file as SEARCH/REPLACE blocks the editor can
/// show as a diff and apply.
pub fn code_system_prompt() -> String {
    "You are a coding assistant inside the HyperStack code editor. The user \
     shares the open file (or a selected fragment of it) and asks questions, \
     reviews or changes. Answer concisely in Markdown.\n\n\
     When you change code in the open file, do not repeat the whole file. \
     Write each change as an edit block, exactly in this form:\n\n\
     <<<<<<< SEARCH\n\
     (the exact existing lines to replace, copied character for character, \
     with enough surrounding lines to be unique in the file)\n\
     =======\n\
     (the new lines)\n\
     >>>>>>> REPLACE\n\n\
     Use several small blocks rather than one large one. To add code at the \
     end of the file, leave the SEARCH part empty. Explain each change in \
     one sentence before its block. When the user asks you to continue \
     after edits were applied, look at the updated file; if nothing more is \
     needed, answer DONE and nothing else."
        .to_string()
}

/// The user message that carries the editor's context.
pub fn context_message(
    file_label: &str,
    language: &str,
    text: &str,
    selection: Option<(usize, usize, &str)>,
    request: &str,
) -> String {
    let fence = if text.contains("```") { "~~~~" } else { "```" };
    let language = language.to_lowercase();
    let mut m = String::new();
    match selection {
        Some((first, last, sel)) => {
            m.push_str(&format!(
                "Selected lines {first}-{last} of `{file_label}`:\n{fence}{language}\n{sel}\n{fence}\n\n"
            ));
            m.push_str(&format!(
                "Whole file for reference:\n{fence}{language}\n{text}\n{fence}\n\n"
            ));
        }
        None => {
            m.push_str(&format!(
                "Open file `{file_label}`:\n{fence}{language}\n{text}\n{fence}\n\n"
            ));
        }
    }
    m.push_str(request);
    m
}

pub mod edits {
    //! SEARCH/REPLACE edit blocks in a model's answer, and where each one
    //! lands in the file.

    const SEARCH: &str = "<<<<<<< SEARCH";
    const DIVIDER: &str = "=======";
    const REPLACE: &str = ">>>>>>> REPLACE";

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EditBlock {
        pub search: String,
        pub replace: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Segment {
        Text(String),
        Edit(EditBlock),
    }

    /// Split an answer into prose and edit blocks, in order. A block still
    /// being streamed (no closing marker yet) stays text until it closes.
    pub fn segments(answer: &str) -> Vec<Segment> {
        let mut out = Vec::new();
        let mut text = String::new();
        let lines: Vec<&str> = answer.split_inclusive('\n').collect();
        let mut i = 0;
        let marker = |l: &str, m: &str| l.trim_end().trim_start() == m;
        'outer: while i < lines.len() {
            if marker(lines[i], SEARCH) {
                let mut search = String::new();
                let mut replace = String::new();
                let mut j = i + 1;
                let mut in_replace = false;
                while j < lines.len() {
                    let l = lines[j];
                    if !in_replace && marker(l, DIVIDER) {
                        in_replace = true;
                    } else if in_replace && marker(l, REPLACE) {
                        // A fence opened just before the block belongs to it.
                        trim_open_fence(&mut text);
                        if !text.trim().is_empty() {
                            out.push(Segment::Text(std::mem::take(&mut text)));
                        }
                        text.clear();
                        out.push(Segment::Edit(EditBlock {
                            search: strip_last_newline(search),
                            replace: strip_last_newline(replace),
                        }));
                        i = j + 1;
                        // …and so does the fence that closes it.
                        if i < lines.len() && lines[i].trim().starts_with("```") {
                            i += 1;
                        }
                        continue 'outer;
                    } else if in_replace {
                        replace.push_str(l);
                    } else {
                        search.push_str(l);
                    }
                    j += 1;
                }
            }
            text.push_str(lines[i]);
            i += 1;
        }
        if !text.trim().is_empty() {
            out.push(Segment::Text(text));
        }
        out
    }

    fn strip_last_newline(mut s: String) -> String {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    }

    fn trim_open_fence(text: &mut String) {
        let body = text.trim_end_matches(['\n', '\r']);
        let start = body.rfind('\n').map(|i| i + 1).unwrap_or(0);
        if body[start..].trim().starts_with("```") {
            text.truncate(start);
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LocateError {
        NotFound,
        Ambiguous,
    }

    /// Byte range of `search` in `text`: an exact, unique match first, then
    /// a line-by-line match that ignores trailing whitespace and the
    /// indentation difference models often introduce. An empty search means
    /// the end of the file.
    pub fn locate(text: &str, search: &str) -> Result<(usize, usize), LocateError> {
        if search.trim().is_empty() {
            return Ok((text.len(), text.len()));
        }
        let mut hits = text.match_indices(search);
        if let Some((at, _)) = hits.next() {
            return if hits.next().is_some() {
                Err(LocateError::Ambiguous)
            } else {
                Ok((at, at + search.len()))
            };
        }
        // Fuzzy: compare trimmed lines.
        let want: Vec<&str> = search.lines().map(str::trim).collect();
        let want: Vec<&str> = {
            // Leading/trailing blank lines in the block do not have to match.
            let s = want.iter().position(|l| !l.is_empty()).unwrap_or(0);
            let e = want
                .iter()
                .rposition(|l| !l.is_empty())
                .map_or(0, |e| e + 1);
            want[s..e].to_vec()
        };
        if want.is_empty() {
            return Err(LocateError::NotFound);
        }
        let mut starts = Vec::new();
        let mut pos = 0;
        for l in text.split_inclusive('\n') {
            starts.push((pos, l));
            pos += l.len();
        }
        let mut found = None;
        for i in 0..starts.len() {
            if i + want.len() > starts.len() {
                break;
            }
            if (0..want.len()).all(|k| starts[i + k].1.trim() == want[k]) {
                if found.is_some() {
                    return Err(LocateError::Ambiguous);
                }
                let (s, _) = starts[i];
                let (ls, l) = starts[i + want.len() - 1];
                let end = ls + l.trim_end_matches(['\n', '\r']).len();
                found = Some((s, end));
            }
        }
        found.ok_or(LocateError::NotFound)
    }

    /// The text with one block applied (for tests and previews).
    pub fn apply_block(text: &str, block: &EditBlock) -> Result<String, LocateError> {
        let (s, e) = locate(text, &block.search)?;
        let mut out = String::with_capacity(text.len() + block.replace.len());
        out.push_str(&text[..s]);
        if block.search.trim().is_empty() && !text.is_empty() && !text.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&block.replace);
        out.push_str(&text[e..]);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_survives_splits_inside_lines_and_utf8() {
        let raw = "event: content\ndata: {\"content\":\"héllo\"}\n\nevent: done\ndata: {}\n\n";
        let bytes = raw.as_bytes();
        let mut p = SseParser::default();
        let mut evs = Vec::new();
        // Feed one byte at a time: splits land everywhere, including inside é.
        for b in bytes {
            evs.extend(p.push(std::slice::from_ref(b)));
        }
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("content"));
        assert_eq!(evs[0].data, "{\"content\":\"héllo\"}");
        assert_eq!(evs[1].event.as_deref(), Some("done"));
    }

    #[test]
    fn interprets_both_providers() {
        let ev = |e: Option<&str>, d: &str| SseEvent {
            event: e.map(str::to_string),
            data: d.into(),
        };
        let t = |s: Step| match s {
            Step::Text(t) => t,
            Step::Done => "<done>".into(),
            Step::Skip => "<skip>".into(),
        };
        let oa = Provider::OpenAi;
        assert_eq!(
            t(interpret(
                oa,
                &ev(None, r#"{"choices":[{"delta":{"content":"Hi"}}]}"#),
                true
            )
            .unwrap()),
            "Hi"
        );
        assert_eq!(
            t(interpret(oa, &ev(None, "[DONE]"), false).unwrap()),
            "<done>"
        );
        let om = Provider::OneMin;
        assert_eq!(
            t(interpret(om, &ev(Some("content"), r#"{"content":"a"}"#), true).unwrap()),
            "a"
        );
        let record = r#"{"aiRecord":{"aiRecordDetail":{"resultObject":["x","y"]}}}"#;
        assert_eq!(
            t(interpret(om, &ev(Some("result"), record), true).unwrap()),
            "x\ny"
        );
        assert_eq!(
            t(interpret(om, &ev(Some("result"), record), false).unwrap()),
            "<skip>"
        );
        assert!(interpret(om, &ev(Some("error"), r#"{"message":"no credit"}"#), true).is_err());
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
    }

    #[test]
    fn model_ids_from_openai_and_ollama_shapes() {
        let v: Value = serde_json::from_str(
            r#"{"object":"list","data":[{"id":"qwen2.5-coder-7b"},{"id":"llama-3.1-8b"}]}"#,
        )
        .unwrap();
        assert_eq!(model_ids(&v), vec!["llama-3.1-8b", "qwen2.5-coder-7b"]);
        let v: Value = serde_json::from_str(r#"{"models":[{"name":"codellama"}]}"#).unwrap();
        assert_eq!(model_ids(&v), vec!["codellama"]);
    }

    #[test]
    fn flatten_ends_with_the_assistant_cue() {
        let m = vec![
            Message::new(Role::System, "Be brief."),
            Message::new(Role::User, "hi"),
            Message::new(Role::Assistant, "hello"),
            Message::new(Role::User, "and?"),
        ];
        let f = flatten(&m);
        assert!(f.starts_with("Be brief.\n\n### User\nhi"));
        assert!(f.ends_with("### User\nand?\n\n### Assistant\n"));
    }

    #[test]
    fn segments_split_prose_and_blocks_and_drop_their_fences() {
        let a = "Rename it.\n```rust\n<<<<<<< SEARCH\nfn a() {}\n=======\nfn b() {}\n>>>>>>> REPLACE\n```\nDone.\n";
        let s = segments(a);
        assert_eq!(
            s,
            vec![
                Segment::Text("Rename it.\n".into()),
                Segment::Edit(EditBlock {
                    search: "fn a() {}".into(),
                    replace: "fn b() {}".into()
                }),
                Segment::Text("Done.\n".into()),
            ]
        );
        // Still streaming: the open block is shown as text, not an edit.
        let partial = "x\n<<<<<<< SEARCH\nfn a() {}\n=====";
        assert!(
            segments(partial)
                .iter()
                .all(|s| matches!(s, Segment::Text(_)))
        );
    }

    #[test]
    fn locate_exact_fuzzy_ambiguous_and_append() {
        let text = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        assert_eq!(locate(text, "let y = 2;"), Ok((31, 41)));
        // Indentation and trailing spaces differ.
        let r = apply_block(
            text,
            &EditBlock {
                search: "let x = 1;  \nlet y = 2;".into(),
                replace: "    let x = 10;\n    let y = 20;".into(),
            },
        )
        .unwrap();
        assert_eq!(r, "fn main() {\n    let x = 10;\n    let y = 20;\n}\n");
        assert_eq!(locate("a\na\n", "a"), Err(LocateError::Ambiguous));
        assert_eq!(locate(text, "nope"), Err(LocateError::NotFound));
        let r = apply_block(
            "a",
            &EditBlock {
                search: String::new(),
                replace: "b\n".into(),
            },
        )
        .unwrap();
        assert_eq!(r, "a\nb\n");
    }
}
