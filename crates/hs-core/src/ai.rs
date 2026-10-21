//! The optional AI assistant (ADR-019, docs/tools/editor.md §10): a streaming
//! chat client for 1min.AI and any OpenAI-compatible service, the prompt
//! that asks a model for reviewable edits, and the parser that turns its
//! answer into edits the editor can apply.
//!
//! Nothing here touches the UI or the vault: the caller resolves the API
//! key and runs [`chat`] on the runtime, receiving text as it streams.

use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use edits::{EditBlock, LocateError, Segment, apply_block, locate, segments};

/// Default endpoint of each provider (the settings field may override it).
pub const ONEMIN_BASE: &str = "https://api.1min.ai";
pub const OPENAI_BASE: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
pub const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Azure OpenAI has no shared host: the resource's own one is required.
pub const AZURE_BASE: &str = "";
/// Bedrock's host carries the region, so the base is built from it.
pub const BEDROCK_BASE: &str = "";
/// The `anthropic-version` every Messages API request must carry.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The Azure OpenAI API version used when a provider does not set one.
pub const AZURE_API_VERSION: &str = "2024-10-21";
/// What Anthropic and Bedrock need as an explicit answer cap.
const MAX_TOKENS: u32 = 8192;

/// The models 1min.AI's chat endpoint (`/api/chat-with-ai`) accepts, by
/// vendor, as `(id, display name)`: its "Available Models" list as of
/// 2026-10-12 (the owner pasted it from the 1min.AI documentation). The
/// endpoint cannot list them itself, so this is the picker's list; any
/// other id can still be typed. (1min.AI's separate OpenAI-compatible
/// adapter, `{base}/openai/v1`, has its own list, read live through an
/// OpenAI-compatible provider.)
pub const ONEMIN_MODEL_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Alibaba",
        &[
            ("qwen3.7-plus", "Qwen 3.7 Plus"),
            ("qwen3.7-max", "Qwen 3.7 Max"),
            ("qwen3.7-flash", "Qwen 3.7 Flash"),
            ("qwen3.6-plus", "Qwen 3.6 Plus"),
            ("qwen3.6-flash", "Qwen 3.6 Flash"),
            ("qwen-plus", "Qwen Plus"),
            ("qwen-max", "Qwen Max"),
            ("qwen-flash", "Qwen Flash"),
        ],
    ),
    (
        "Anthropic (AWS Bedrock)",
        &[
            ("us.anthropic.claude-sonnet-5-5", "Claude 5.5 Sonnet"),
            ("us.anthropic.claude-sonnet-5", "Claude 5 Sonnet"),
            ("us.anthropic.claude-sonnet-4-6", "Claude 4.6 Sonnet"),
            (
                "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
                "Claude 4.5 Sonnet",
            ),
            ("us.anthropic.claude-opus-5-5", "Claude 5.5 Opus"),
            ("us.anthropic.claude-opus-5", "Claude 5 Opus"),
            ("us.anthropic.claude-opus-4-8", "Claude 4.8 Opus"),
            ("us.anthropic.claude-opus-4-7", "Claude 4.7 Opus"),
            ("us.anthropic.claude-opus-4-6-v1", "Claude 4.6 Opus"),
            (
                "us.anthropic.claude-opus-4-5-20251101-v1:0",
                "Claude 4.5 Opus",
            ),
            ("us.anthropic.claude-haiku-5-5", "Claude 5.5 Haiku"),
            (
                "us.anthropic.claude-haiku-4-5-20251001-v1:0",
                "Claude 4.5 Haiku",
            ),
            ("us.anthropic.claude-fable-5-1", "Claude 5.1 Fable"),
            ("us.anthropic.claude-fable-5", "Claude 5 Fable"),
        ],
    ),
    ("Cohere", &[("command-r-08-2024", "Command R")]),
    (
        "DeepSeek",
        &[
            ("deepseek-v4-pro", "DeepSeek V4 Pro"),
            ("deepseek-flash", "DeepSeek V4.1 Flash"),
        ],
    ),
    (
        "Google",
        &[
            ("gemini-3.8-flash", "Gemini 3.8 Flash"),
            ("gemini-3.7-flash", "Gemini 3.7 Flash"),
            ("gemini-3.6-flash", "Gemini 3.6 Flash"),
            ("gemini-3.5-flash", "Gemini 3.5 Flash"),
            ("gemini-3.1-pro-preview", "Gemini 3.1 Pro"),
            ("gemini-3.1-flash-lite-preview", "Gemini 3.1 Flash Lite"),
            ("gemini-3-flash-preview", "Gemini 3 Flash"),
        ],
    ),
    (
        "Mistral",
        &[
            ("magistral-small-latest", "Magistral Small 1.2"),
            ("magistral-medium-latest", "Magistral Medium 1.2"),
            ("ministral-14b-latest", "Ministral 14B Latest"),
            ("open-mistral-nemo", "Mistral Open Nemo"),
            ("mistral-small-latest", "Mistral Small"),
            ("mistral-small-2603", "Mistral Small 4"),
            ("mistral-medium-latest", "Mistral Medium 3.1"),
            ("mistral-medium-3-5", "Mistral Medium 3.5"),
            ("mistral-large-latest", "Mistral Large 2"),
            ("mistral-large-2512", "Mistral Large 3"),
        ],
    ),
    (
        "OpenAI",
        &[
            ("gpt-5.3-codex", "GPT-5.3 Codex"),
            ("o3-mini", "GPT-o3 Mini"),
            ("gpt-6.1-sol", "GPT-6.1 Sol"),
            ("gpt-6-sol", "GPT-6 Sol"),
            ("gpt-6-luna", "GPT-6 Luna"),
            ("gpt-6-astra", "GPT-6 Astra"),
            ("gpt-5.6-terra", "GPT-5.6 Terra"),
            ("gpt-5.6-sol", "GPT-5.6 Sol"),
            ("gpt-5.6-luna", "GPT-5.6 Luna"),
            ("gpt-5.5-pro", "GPT-5.5 Pro"),
            ("gpt-5.5", "GPT-5.5"),
            ("gpt-5.4-pro", "GPT-5.4 Pro"),
            ("gpt-5.4-nano", "GPT-5.4 Nano"),
            ("gpt-5.4-mini", "GPT-5.4 Mini"),
            ("gpt-5.4", "GPT-5.4"),
            ("gpt-5.2-pro", "GPT-5.2 Pro"),
            ("gpt-5.2", "GPT-5.2"),
            ("gpt-5.1", "GPT-5.1"),
            ("gpt-5-nano", "GPT-5 Nano"),
            ("gpt-5-mini", "GPT-5 Mini"),
            ("gpt-5", "GPT-5"),
            ("gpt-4o-mini", "GPT-4o Mini"),
            ("gpt-4o", "GPT-4o"),
            ("gpt-4.1-nano", "GPT-4.1 nano"),
            ("gpt-4.1-mini", "GPT-4.1 mini"),
            ("gpt-4.1", "GPT-4.1"),
            ("gpt-4-turbo", "GPT-4 Turbo"),
            ("gpt-3.5-turbo", "GPT-3.5"),
            ("o3-pro", "o3 Pro"),
            ("o3", "o3"),
        ],
    ),
    (
        "OpenRouter",
        &[
            ("moonshotai/kimi-k3", "Kimi K3"),
            ("moonshotai/kimi-k2.7-code", "Kimi K2.7 Code"),
            ("moonshotai/kimi-k2.6", "Kimi K2.6"),
            ("meta/muse-spark-1.3", "Muse Spark 1.3"),
            ("meta/muse-spark-1.2", "Muse Spark 1.2"),
            ("meta/muse-spark-1.1", "Muse Spark 1.1"),
        ],
    ),
    (
        "Perplexity",
        &[
            ("sonar-reasoning-pro", "Perplexity Reasoning Pro"),
            ("sonar-pro", "Perplexity Pro"),
            ("sonar-deep-research", "Perplexity Deep Research"),
            ("sonar", "Perplexity"),
        ],
    ),
    (
        "xAI",
        &[
            ("grok-4.7", "Grok 4.7"),
            ("grok-4.6", "Grok 4.6"),
            ("grok-4.5", "Grok 4.5"),
            ("grok-4.3", "Grok 4.3"),
        ],
    ),
    (
        "Z.AI",
        &[
            ("glm-5.3", "GLM-5.3"),
            ("glm-5.2", "GLM-5.2"),
            ("glm-5.1", "GLM-5.1"),
            ("glm-5", "GLM-5"),
        ],
    ),
    (
        "Other",
        &[
            ("meta/meta-llama-3-70b-instruct", "LLaMA 3 70b"),
            ("meta/llama-4-scout-instruct", "LLaMA 4 Scout Instruct"),
            (
                "meta/llama-4-maverick-instruct",
                "LLaMA 4 Maverick Instruct",
            ),
            ("openai/gpt-oss-20b", "GPT OSS 20b"),
            ("openai/gpt-oss-120b", "GPT OSS 120b"),
        ],
    ),
];

/// Every id in [`ONEMIN_MODEL_GROUPS`], in table order.
pub fn onemin_models() -> Vec<String> {
    ONEMIN_MODEL_GROUPS
        .iter()
        .flat_map(|(_, ids)| ids.iter().map(|(id, _)| id.to_string()))
        .collect()
}

/// 1min.AI's display name for a model id ("GPT-4o Mini").
pub fn onemin_model_name(id: &str) -> Option<&'static str> {
    ONEMIN_MODEL_GROUPS
        .iter()
        .flat_map(|(_, ids)| ids.iter())
        .find(|(m, _)| *m == id)
        .map(|(_, name)| *name)
}

/// Models arranged for a picker: for 1min.AI under its vendor headings
/// (ids it does not know last, under an empty heading), otherwise one
/// group with no heading.
pub fn group_models(provider: Provider, models: &[String]) -> Vec<(&'static str, Vec<String>)> {
    if provider != Provider::OneMin {
        return vec![("", models.to_vec())];
    }
    let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (heading, ids) in ONEMIN_MODEL_GROUPS {
        let have: Vec<String> = ids
            .iter()
            .filter(|(id, _)| models.iter().any(|m| m == id))
            .map(|(id, _)| id.to_string())
            .collect();
        if !have.is_empty() {
            out.push((heading, have));
        }
    }
    let rest: Vec<String> = models
        .iter()
        .filter(|m| onemin_model_name(m).is_none())
        .cloned()
        .collect();
    if !rest.is_empty() {
        out.push(("", rest));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// 1min.AI's own API (`/api/chat-with-ai`, `API-KEY` header).
    #[default]
    #[serde(rename = "1min")]
    OneMin,
    /// `POST {base}/chat/completions` with a bearer token: OpenAI, OpenRouter,
    /// Groq, Mistral, Moonshot (Kimi), Tencent Hunyuan, Ollama, LM Studio,
    /// vLLM and every other service that copies OpenAI's shape.
    OpenAi,
    /// Anthropic's Messages API: `POST {base}/v1/messages`, `x-api-key`,
    /// `anthropic-version`, the system prompt in its own field.
    Anthropic,
    /// Google Gemini: `POST {base}/models/{model}:streamGenerateContent`
    /// with `x-goog-api-key` and `contents`/`parts`.
    Gemini,
    /// Azure OpenAI: OpenAI's body on the resource's own host, the model
    /// as a deployment name, `api-key` and an `api-version`.
    Azure,
    /// AWS Bedrock's Converse API, signed with SigV4. The answer arrives
    /// in one piece: the streaming form is a binary event stream, which
    /// this client does not speak.
    Bedrock,
}

impl Provider {
    pub fn all() -> [Provider; 6] {
        [
            Provider::OpenAi,
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Azure,
            Provider::Bedrock,
            Provider::OneMin,
        ]
    }
    pub fn default_base(self) -> &'static str {
        match self {
            Provider::OneMin => ONEMIN_BASE,
            Provider::OpenAi => OPENAI_BASE,
            Provider::Anthropic => ANTHROPIC_BASE,
            Provider::Gemini => GEMINI_BASE,
            Provider::Azure => AZURE_BASE,
            Provider::Bedrock => BEDROCK_BASE,
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::OneMin | Provider::OpenAi | Provider::Azure => "gpt-4o-mini",
            Provider::Anthropic => "claude-sonnet-4-5",
            Provider::Gemini => "gemini-2.5-flash",
            Provider::Bedrock => "anthropic.claude-3-5-sonnet-20241022-v2:0",
        }
    }
    /// The name shown in the settings page.
    pub fn label(self) -> &'static str {
        match self {
            Provider::OneMin => "1min.AI",
            Provider::OpenAi => "OpenAI-compatible",
            Provider::Anthropic => "Anthropic",
            Provider::Gemini => "Google Gemini",
            Provider::Azure => "Azure OpenAI",
            Provider::Bedrock => "AWS Bedrock",
        }
    }
    /// Bedrock signs with an access key *and* a secret.
    pub fn needs_secret(self) -> bool {
        self == Provider::Bedrock
    }
    /// Bedrock's host and signature carry the region.
    pub fn needs_region(self) -> bool {
        self == Provider::Bedrock
    }
    /// Azure pins an API version per deployment.
    pub fn needs_api_version(self) -> bool {
        self == Provider::Azure
    }
    /// Azure's "model" is the deployment name, and its base URL is the
    /// resource's own host, so neither has a useful default.
    pub fn needs_base(self) -> bool {
        matches!(self, Provider::Azure)
    }
    /// Whether the service can list its models.
    pub fn lists_models(self) -> bool {
        !matches!(self, Provider::Bedrock)
    }
    /// The answer arrives in one piece rather than token by token.
    pub fn whole_answer_only(self) -> bool {
        self == Provider::Bedrock
    }
    /// The chat request may carry pictures (a vision-capable model is
    /// still needed on the other end).
    pub fn accepts_images(self) -> bool {
        true
    }
    /// The service can make a picture from a prompt (`generate_image`).
    pub fn generates_images(self) -> bool {
        self == Provider::OneMin
    }
}

/// What a provider authenticates with. Bedrock needs a pair; everything
/// else uses `key` alone (empty for a local server).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    pub key: String,
    /// AWS secret access key; empty for every other provider.
    pub secret: String,
}

impl Credentials {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            secret: String::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.key.trim().is_empty()
    }
}

impl From<&str> for Credentials {
    fn from(key: &str) -> Self {
        Credentials::new(key)
    }
}

impl From<&String> for Credentials {
    fn from(key: &String) -> Self {
        Credentials::new(key.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiConfig {
    pub provider: Provider,
    /// Empty = the provider's default.
    pub base_url: String,
    pub model: String,
    /// AWS region (Bedrock); empty elsewhere.
    pub region: String,
    /// Azure OpenAI API version; empty uses [`AZURE_API_VERSION`].
    pub api_version: String,
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
    fn region(&self) -> &str {
        let r = self.region.trim();
        if r.is_empty() { "us-east-1" } else { r }
    }
    fn api_version(&self) -> &str {
        let v = self.api_version.trim();
        if v.is_empty() { AZURE_API_VERSION } else { v }
    }
    /// Bedrock's host is built from the region unless one was given.
    fn bedrock_host(&self) -> String {
        let b = self.base_url.trim().trim_end_matches('/');
        if b.is_empty() {
            format!("bedrock-runtime.{}.amazonaws.com", self.region())
        } else {
            b.trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string()
        }
    }
}

/// The conversation as the providers that keep the system prompt apart
/// need it: the system text, then the alternating turns.
fn split_system(messages: &[Message]) -> (String, Vec<&Message>) {
    let mut system = String::new();
    let mut turns = Vec::new();
    for m in messages {
        if m.role == Role::System {
            if !system.is_empty() {
                system.push_str("\n\n");
            }
            system.push_str(&m.content);
        } else {
            turns.push(m);
        }
    }
    (system, turns)
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

/// A picture attached to a message: the encoded bytes and their MIME type
/// (`image/png`, `image/jpeg`, `image/webp`). Sent base64-encoded to the
/// providers that take image content; uploaded as an asset to 1min.AI.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePart {
    pub mime: String,
    pub data: Vec<u8>,
}

impl ImagePart {
    pub fn png(data: Vec<u8>) -> Self {
        Self {
            mime: "image/png".into(),
            data,
        }
    }
    fn base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
    fn data_uri(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.base64())
    }
    /// Bedrock names the format rather than the MIME type.
    fn bedrock_format(&self) -> &str {
        match self.mime.as_str() {
            "image/jpeg" | "image/jpg" => "jpeg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Pictures that go with the text (a user turn asking about an image).
    pub images: Vec<ImagePart>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            images: Vec::new(),
        }
    }

    pub fn with_images(mut self, images: Vec<ImagePart>) -> Self {
        self.images = images;
        self
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
    creds: &Credentials,
    messages: &[Message],
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<String, AiError> {
    let api_key = creds.key.as_str();
    let req = match cfg.provider {
        Provider::OpenAi => {
            let body = openai_body(cfg, messages, true);
            let mut r = http
                .post(format!("{}/chat/completions", cfg.base()))
                .json(&body);
            if !api_key.is_empty() {
                r = r.bearer_auth(api_key);
            }
            r
        }
        Provider::Azure => {
            // The model is a deployment on the resource's own host.
            let body = openai_body(cfg, messages, true);
            http.post(format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                cfg.base(),
                cfg.model(),
                cfg.api_version()
            ))
            .header("api-key", api_key)
            .json(&body)
        }
        Provider::Anthropic => {
            let (system, turns) = split_system(messages);
            let mut body = json!({
                "model": cfg.model(),
                "max_tokens": MAX_TOKENS,
                "stream": true,
                "messages": anthropic_turns(&turns),
            });
            if !system.is_empty() {
                body["system"] = Value::String(system);
            }
            http.post(format!("{}/v1/messages", cfg.base()))
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body)
        }
        Provider::Gemini => {
            let (system, turns) = split_system(messages);
            let mut body = json!({
                "contents": turns
                    .iter()
                    .map(|m| {
                        let mut parts = vec![json!({"text": m.content})];
                        for img in &m.images {
                            parts.push(json!({
                                "inline_data": {"mime_type": img.mime, "data": img.base64()}
                            }));
                        }
                        json!({
                            "role": if m.role == Role::Assistant { "model" } else { "user" },
                            "parts": parts,
                        })
                    })
                    .collect::<Vec<_>>(),
            });
            if !system.is_empty() {
                body["systemInstruction"] = json!({"parts": [{"text": system}]});
            }
            http.post(format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                cfg.base(),
                cfg.model()
            ))
            .header("x-goog-api-key", api_key)
            .json(&body)
        }
        Provider::Bedrock => {
            // Converse, not ConverseStream: the streaming form is a binary
            // event stream this client does not speak, and a whole answer
            // is better than a half-parsed one.
            let (system, turns) = split_system(messages);
            let mut body = json!({
                "messages": turns
                    .iter()
                    .map(|m| {
                        let mut content = vec![json!({"text": m.content})];
                        for img in &m.images {
                            content.push(json!({
                                "image": {
                                    "format": img.bedrock_format(),
                                    "source": {"bytes": img.base64()}
                                }
                            }));
                        }
                        json!({
                            "role": if m.role == Role::Assistant { "assistant" } else { "user" },
                            "content": content,
                        })
                    })
                    .collect::<Vec<_>>(),
                "inferenceConfig": {"maxTokens": MAX_TOKENS},
            });
            if !system.is_empty() {
                body["system"] = json!([{"text": system}]);
            }
            return bedrock_converse(http, cfg, creds, &body, on_text).await;
        }
        Provider::OneMin => {
            // Pictures go up as assets first; the chat then names them.
            let mut keys = Vec::new();
            for img in messages.iter().flat_map(|m| m.images.iter()) {
                keys.push(onemin_upload(http, cfg, api_key, img).await?);
            }
            let mut prompt_object = json!({ "prompt": flatten(messages) });
            if !keys.is_empty() {
                prompt_object["attachments"] = json!({ "images": keys });
            }
            let body = json!({
                "type": "UNIFY_CHAT_WITH_AI",
                "model": cfg.model(),
                "promptObject": prompt_object,
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

/// The OpenAI (and Azure) request body. A message with pictures becomes
/// a content array of a text part and `image_url` data URIs.
fn openai_body(cfg: &AiConfig, messages: &[Message], stream: bool) -> Value {
    json!({
        "model": cfg.model(),
        "stream": stream,
        "messages": messages
            .iter()
            .map(|m| {
                if m.images.is_empty() {
                    json!({"role": m.role.as_str(), "content": m.content})
                } else {
                    let mut parts = vec![json!({"type": "text", "text": m.content})];
                    for img in &m.images {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": {"url": img.data_uri()}
                        }));
                    }
                    json!({"role": m.role.as_str(), "content": parts})
                }
            })
            .collect::<Vec<_>>(),
    })
}

/// Anthropic needs turns that alternate and begin with the user, so
/// consecutive messages of one role are joined; their pictures are kept.
fn anthropic_turns(turns: &[&Message]) -> Vec<Value> {
    let mut out: Vec<(Role, String, Vec<ImagePart>)> = Vec::new();
    for m in turns {
        match out.last_mut() {
            Some((role, text, images)) if *role == m.role => {
                text.push_str("\n\n");
                text.push_str(&m.content);
                images.extend(m.images.iter().cloned());
            }
            _ => out.push((m.role, m.content.clone(), m.images.clone())),
        }
    }
    if out.first().is_some_and(|(r, ..)| *r == Role::Assistant) {
        out.insert(0, (Role::User, "(continue)".to_string(), Vec::new()));
    }
    out.into_iter()
        .map(|(role, content, images)| {
            let role = if role == Role::Assistant {
                "assistant"
            } else {
                "user"
            };
            if images.is_empty() {
                json!({ "role": role, "content": content })
            } else {
                let mut blocks: Vec<Value> = images
                    .iter()
                    .map(|img| {
                        json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": img.mime,
                                "data": img.base64()
                            }
                        })
                    })
                    .collect();
                blocks.push(json!({"type": "text", "text": content}));
                json!({ "role": role, "content": blocks })
            }
        })
        .collect()
}

/// A `multipart/form-data` body with one file field, built by hand: the
/// workspace's reqwest has no `multipart` feature.
fn multipart_file(field: &str, filename: &str, mime: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("----HyperStack{}", crate::new_id());
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// Upload a picture to 1min.AI's asset store and return the key later
/// requests refer to it by (`promptObject.imageUrl`, `attachments.images`).
async fn onemin_upload(
    http: &reqwest::Client,
    cfg: &AiConfig,
    api_key: &str,
    img: &ImagePart,
) -> Result<String, AiError> {
    let ext = match img.mime.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    };
    let (boundary, body) =
        multipart_file("asset", &format!("hyperstack.{ext}"), &img.mime, &img.data);
    let resp = http
        .post(format!("{}/api/assets", cfg.base()))
        .header("API-KEY", api_key)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
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
    v.pointer("/asset/key")
        .or_else(|| v.pointer("/fileContent/path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AiError::Format(truncate(&text, 300)))
}

/// Ask a service for a picture: from `prompt` alone, or from `source` with
/// the prompt as the variation instruction. Only 1min.AI does this today
/// (`Provider::generates_images`); the result is the picture's bytes.
///
/// `model` is the image model (`black-forest-labs/flux-schnell`,
/// `gpt-image-2`, `clipdrop` for variations…); `size` is advisory, sent as
/// an aspect ratio where the model takes one.
pub async fn generate_image(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
) -> Result<Vec<u8>, AiError> {
    if cfg.provider != Provider::OneMin {
        return Err(AiError::Format(format!(
            "{} does not generate pictures; add a 1min.AI provider for that",
            cfg.provider.label()
        )));
    }
    let api_key = creds.key.as_str();
    let aspect = aspect_ratio(size);
    let body = match source {
        None => json!({
            "type": "IMAGE_GENERATOR",
            "model": model,
            "promptObject": {
                "prompt": prompt,
                "n": 1,
                "num_outputs": 1,
                "aspect_ratio": aspect,
                "size": format!("{}x{}", size.0, size.1),
            },
        }),
        Some(img) => {
            let key = onemin_upload(http, cfg, api_key, img).await?;
            json!({
                "type": "IMAGE_VARIATOR",
                "model": model,
                "promptObject": {
                    "imageUrl": key,
                    "prompt": prompt,
                    "n": 1,
                },
            })
        }
    };
    let resp = http
        .post(format!("{}/api/features", cfg.base()))
        .header("API-KEY", api_key)
        .json(&body)
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
    // The record carries a signed, time-limited download URL; the
    // resultObject keys are storage paths, not downloadable.
    let url = v
        .pointer("/aiRecord/temporaryUrl")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            v.pointer("/aiRecord/aiRecordDetail/resultObject/0")
                .and_then(Value::as_str)
                .filter(|s| s.starts_with("http"))
                .map(str::to_string)
        })
        .ok_or_else(|| AiError::Format(truncate(&text, 300)))?;
    let bytes = http
        .get(&url)
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?
        .bytes()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    Ok(bytes.to_vec())
}

/// The closest of the aspect ratios image models take.
fn aspect_ratio((w, h): (u32, u32)) -> &'static str {
    let r = w.max(1) as f32 / h.max(1) as f32;
    let choices: [(&str, f32); 7] = [
        ("1:1", 1.0),
        ("16:9", 16.0 / 9.0),
        ("9:16", 9.0 / 16.0),
        ("4:3", 4.0 / 3.0),
        ("3:4", 3.0 / 4.0),
        ("3:2", 1.5),
        ("2:3", 2.0 / 3.0),
    ];
    choices
        .iter()
        .min_by(|a, b| (a.1 - r).abs().total_cmp(&(b.1 - r).abs()))
        .map(|c| c.0)
        .unwrap_or("1:1")
}

/// Sign and send one Bedrock Converse request, and hand the whole answer
/// to `on_text` when it arrives.
async fn bedrock_converse(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    body: &Value,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<String, AiError> {
    use crate::objstore::sigv4;
    let host = cfg.bedrock_host();
    // The model id travels in the path, so its colons and slashes are encoded.
    let path = format!("/model/{}/converse", sigv4::uri_encode(cfg.model(), true));
    let payload = serde_json::to_vec(body).unwrap_or_default();
    let payload_hash = sigv4::sha256_hex(&payload);
    let date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let headers = vec![
        ("host".to_string(), host.clone()),
        ("content-type".to_string(), "application/json".to_string()),
        ("x-amz-date".to_string(), date.clone()),
    ];
    let aws = sigv4::Credentials {
        access_key: creds.key.clone(),
        secret_key: creds.secret.clone(),
        session_token: None,
    };
    let auth = sigv4::authorization(
        &aws,
        cfg.region(),
        "bedrock",
        &date,
        &sigv4::Request {
            method: "POST",
            path: &path,
            query: &[],
            headers: &headers,
            payload_hash: &payload_hash,
        },
    );
    let resp = http
        .post(format!("https://{host}{path}"))
        .header("content-type", "application/json")
        .header("x-amz-date", date)
        .header("authorization", auth)
        .body(payload)
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
    let answer = whole_answer(Provider::Bedrock, &v)
        .ok_or_else(|| AiError::Format(truncate(&v.to_string(), 300)))?;
    on_text(&answer);
    Ok(answer)
}

/// Model ids the service offers (`GET {base}/models`, OpenAI-compatible
/// services only; LM Studio with JIT loading lists every downloaded model,
/// Ollama its pulled ones). 1min.AI's chat endpoint has no such list, so
/// it answers with [`onemin_models`] without a request.
pub async fn list_models(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<Vec<String>, AiError> {
    let api_key = creds.key.as_str();
    if cfg.provider == Provider::OneMin {
        return Ok(onemin_models());
    }
    if cfg.provider == Provider::Bedrock {
        // Listing them is a different service (bedrock, not
        // bedrock-runtime) with its own permissions; the well-known ids
        // are more useful than an error.
        return Ok(BEDROCK_MODELS.iter().map(|m| m.to_string()).collect());
    }
    let mut r = match cfg.provider {
        Provider::Anthropic => http
            .get(format!("{}/v1/models", cfg.base()))
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        Provider::Gemini => http
            .get(format!("{}/models", cfg.base()))
            .header("x-goog-api-key", api_key),
        Provider::Azure => http
            .get(format!(
                "{}/openai/models?api-version={}",
                cfg.base(),
                cfg.api_version()
            ))
            .header("api-key", api_key),
        _ => http.get(format!("{}/models", cfg.base())),
    };
    if cfg.provider == Provider::OpenAi && !api_key.is_empty() {
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

/// The Bedrock model ids worth offering without a control-plane call.
pub const BEDROCK_MODELS: &[&str] = &[
    "anthropic.claude-3-5-haiku-20241022-v1:0",
    "anthropic.claude-3-5-sonnet-20241022-v2:0",
    "anthropic.claude-3-7-sonnet-20250219-v1:0",
    "amazon.nova-lite-v1:0",
    "amazon.nova-pro-v1:0",
    "meta.llama3-3-70b-instruct-v1:0",
    "mistral.mistral-large-2407-v1:0",
];

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
                        // Gemini returns `models/gemini-2.5-flash`.
                        .map(|id| id.strip_prefix("models/").unwrap_or(id).to_string())
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
        Provider::OpenAi | Provider::Azure => v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string),
        Provider::Anthropic => {
            // `content` is a list of blocks; the text ones make the answer.
            let blocks = v.get("content")?.as_array()?;
            let text: String = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
        Provider::Gemini => {
            let parts = v.pointer("/candidates/0/content/parts")?.as_array()?;
            let text: String = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
        Provider::Bedrock => {
            let blocks = v.pointer("/output/message/content")?.as_array()?;
            let text: String = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
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
        Provider::Anthropic => {
            if data.is_empty() {
                return Ok(Step::Skip);
            }
            let v: Value =
                serde_json::from_str(data).map_err(|e| AiError::Format(e.to_string()))?;
            let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
            match ev.event.as_deref().unwrap_or(kind) {
                "message_stop" => Ok(Step::Done),
                "error" => Err(AiError::Format(
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| truncate(data, 300)),
                )),
                "content_block_delta" => Ok(v
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| Step::Text(s.to_string()))
                    .unwrap_or(Step::Skip)),
                _ => Ok(Step::Skip),
            }
        }
        Provider::Gemini => {
            // Gemini has no end marker: the stream simply stops.
            if data.is_empty() || data == "[DONE]" {
                return Ok(Step::Skip);
            }
            let v: Value =
                serde_json::from_str(data).map_err(|e| AiError::Format(e.to_string()))?;
            if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
                return Err(AiError::Format(m.to_string()));
            }
            Ok(whole_answer(Provider::Gemini, &v)
                .filter(|s| !s.is_empty())
                .map(Step::Text)
                .unwrap_or(Step::Skip))
        }
        Provider::Bedrock => Ok(Step::Skip),
        Provider::OpenAi | Provider::Azure => {
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

/// System instructions for conversational help in tools that do not edit a
/// source buffer. Never ask the model to run commands or perform actions: the
/// host tool remains under the user's control.
pub fn assistant_system_prompt(tool: &str) -> String {
    format!(
        "You are HyperStack's {tool} assistant. Help explain, troubleshoot, and draft scripts, queries, or configuration using only context the user chose to share. Be concise, state assumptions, and point out risky or destructive operations before suggesting them. Never claim to have executed a command, query, request, or storage operation. Return suggestions for the user to review and apply manually."
    )
}

#[cfg(test)]
mod assistant_prompt_tests {
    use super::assistant_system_prompt;

    #[test]
    fn shared_assistant_is_review_only_and_does_not_claim_actions() {
        let prompt = assistant_system_prompt("SQL");
        assert!(prompt.contains("SQL assistant"));
        assert!(prompt.contains("Never claim to have executed"));
        assert!(prompt.contains("review and apply manually"));
    }
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

    fn cfg_for(provider: Provider) -> AiConfig {
        AiConfig {
            provider,
            base_url: String::new(),
            model: String::new(),
            region: String::new(),
            api_version: String::new(),
        }
    }

    #[test]
    fn every_provider_has_a_usable_default_and_a_label() {
        for p in Provider::all() {
            assert!(!p.default_model().is_empty(), "{p:?} has no default model");
            assert!(!p.label().is_empty(), "{p:?} has no label");
            // Azure and Bedrock build their host from the resource or the
            // region, so only those two may have an empty default base.
            if p.default_base().is_empty() {
                assert!(
                    matches!(p, Provider::Azure | Provider::Bedrock),
                    "{p:?} needs a default base URL"
                );
            } else {
                assert!(
                    p.default_base().starts_with("https://"),
                    "{p:?} default base is not https"
                );
            }
        }
        assert!(Provider::Bedrock.needs_secret() && Provider::Bedrock.needs_region());
        assert!(Provider::Azure.needs_api_version() && Provider::Azure.needs_base());
        assert!(!Provider::OpenAi.needs_secret());
        assert!(!Provider::Bedrock.lists_models());
        assert!(Provider::Bedrock.whole_answer_only());
        assert!(!Provider::Anthropic.whole_answer_only());
    }

    #[test]
    fn the_system_prompt_is_split_out_for_the_services_that_want_it() {
        let msgs = [
            Message::new(Role::System, "be terse"),
            Message::new(Role::System, "and polite"),
            Message::new(Role::User, "hello"),
            Message::new(Role::Assistant, "hi"),
            Message::new(Role::User, "again"),
        ];
        let (system, turns) = split_system(&msgs);
        assert_eq!(system, "be terse\n\nand polite");
        assert_eq!(turns.len(), 3);
        let anthropic = anthropic_turns(&turns);
        assert_eq!(anthropic.len(), 3);
        assert_eq!(anthropic[0]["role"], "user");
        assert_eq!(anthropic[1]["role"], "assistant");
    }

    #[test]
    fn anthropic_turns_alternate_and_start_with_the_user() {
        // Two user messages in a row are joined, and an answer-first
        // conversation gets an opening user turn.
        let msgs = [
            Message::new(Role::User, "one"),
            Message::new(Role::User, "two"),
        ];
        let turns: Vec<&Message> = msgs.iter().collect();
        let out = anthropic_turns(&turns);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["content"], "one\n\ntwo");
        let msgs = [Message::new(Role::Assistant, "already talking")];
        let turns: Vec<&Message> = msgs.iter().collect();
        let out = anthropic_turns(&turns);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[1]["role"], "assistant");
    }

    #[test]
    fn bedrock_builds_its_host_from_the_region() {
        let mut cfg = cfg_for(Provider::Bedrock);
        assert_eq!(
            cfg.bedrock_host(),
            "bedrock-runtime.us-east-1.amazonaws.com"
        );
        cfg.region = "eu-central-1".into();
        assert_eq!(
            cfg.bedrock_host(),
            "bedrock-runtime.eu-central-1.amazonaws.com"
        );
        // An explicit endpoint wins, with or without a scheme.
        cfg.base_url = "https://my-gateway.test/".into();
        assert_eq!(cfg.bedrock_host(), "my-gateway.test");
    }

    #[test]
    fn azure_falls_back_to_a_known_api_version() {
        let mut cfg = cfg_for(Provider::Azure);
        assert_eq!(cfg.api_version(), AZURE_API_VERSION);
        cfg.api_version = "2025-01-01-preview".into();
        assert_eq!(cfg.api_version(), "2025-01-01-preview");
    }

    #[test]
    fn each_provider_reads_its_own_stream_and_whole_answer() {
        let ev = |e: Option<&str>, d: &str| SseEvent {
            event: e.map(str::to_string),
            data: d.into(),
        };
        let text = |p: Provider, e: SseEvent| match interpret(p, &e, false) {
            Ok(Step::Text(t)) => t,
            Ok(Step::Done) => "<done>".into(),
            Ok(Step::Skip) => "<skip>".into(),
            Err(e) => format!("<err {e:?}>"),
        };
        // Anthropic: text deltas, then message_stop.
        assert_eq!(
            text(
                Provider::Anthropic,
                ev(
                    Some("content_block_delta"),
                    r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hel"}}"#
                )
            ),
            "Hel"
        );
        assert_eq!(
            text(
                Provider::Anthropic,
                ev(Some("message_stop"), r#"{"type":"message_stop"}"#)
            ),
            "<done>"
        );
        assert_eq!(
            text(
                Provider::Anthropic,
                ev(Some("message_start"), r#"{"type":"message_start"}"#)
            ),
            "<skip>"
        );
        // Gemini: candidates with parts, and no end marker at all.
        assert_eq!(
            text(
                Provider::Gemini,
                ev(
                    None,
                    r#"{"candidates":[{"content":{"parts":[{"text":"lo"}]}}]}"#
                )
            ),
            "lo"
        );
        // Azure streams exactly like OpenAI.
        assert_eq!(
            text(
                Provider::Azure,
                ev(None, r#"{"choices":[{"delta":{"content":"zu"}}]}"#)
            ),
            "zu"
        );
        assert_eq!(text(Provider::Azure, ev(None, "[DONE]")), "<done>");
        // Whole answers, for the non-streaming path.
        let anthropic: Value = serde_json::from_str(
            r#"{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(
            whole_answer(Provider::Anthropic, &anthropic).as_deref(),
            Some("ab")
        );
        let gemini: Value =
            serde_json::from_str(r#"{"candidates":[{"content":{"parts":[{"text":"g"}]}}]}"#)
                .unwrap();
        assert_eq!(
            whole_answer(Provider::Gemini, &gemini).as_deref(),
            Some("g")
        );
        let bedrock: Value =
            serde_json::from_str(r#"{"output":{"message":{"content":[{"text":"br"}]}}}"#).unwrap();
        assert_eq!(
            whole_answer(Provider::Bedrock, &bedrock).as_deref(),
            Some("br")
        );
        assert_eq!(whole_answer(Provider::Bedrock, &gemini), None);
    }

    #[test]
    fn gemini_model_ids_lose_their_prefix() {
        let v: Value =
            serde_json::from_str(r#"{"models":[{"name":"models/gemini-2.5-flash"}]}"#).unwrap();
        assert_eq!(model_ids(&v), vec!["gemini-2.5-flash"]);
    }

    #[test]
    fn one_min_lists_its_built_in_models_without_asking() {
        // An unroutable endpoint: a request would fail, the list must not.
        let cfg = AiConfig {
            provider: Provider::OneMin,
            base_url: "http://127.0.0.1:9".into(),
            model: String::new(),
            ..Default::default()
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let models = rt
            .block_on(list_models(&http_client(), &cfg, &Credentials::default()))
            .unwrap();
        assert_eq!(models.len(), 95);
        assert!(models.iter().any(|m| m == Provider::OneMin.default_model()));
        // Even with a key nothing is asked: the chat endpoint has no list.
        let again = rt
            .block_on(list_models(&http_client(), &cfg, &Credentials::new("k")))
            .unwrap();
        assert_eq!(again, models);
    }

    #[test]
    fn one_min_list_has_no_duplicates() {
        let mut all = onemin_models();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n);
    }

    #[test]
    fn one_min_models_group_under_their_vendors() {
        let models: Vec<String> = ["gpt-5", "glm-5", "new-model", "sonar"]
            .map(String::from)
            .to_vec();
        let groups = group_models(Provider::OneMin, &models);
        let headings: Vec<&str> = groups.iter().map(|g| g.0).collect();
        assert_eq!(headings, ["OpenAI", "Perplexity", "Z.AI", ""]);
        assert_eq!(groups[3].1, ["new-model"]);
        assert_eq!(onemin_model_name("gpt-4o-mini"), Some("GPT-4o Mini"));
        assert_eq!(onemin_model_name("new-model"), None);
        assert_eq!(group_models(Provider::OpenAi, &models).len(), 1);
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
