//! fal.ai behind `ai::generate_image` and `ai::describe_image`: a hosted
//! image provider whose models are chosen **per purpose** - one for a new
//! picture, one for an edit, one for an enhancement, one for an upscale
//! and one that describes a picture (`FalModels`), because fal.ai is a
//! catalogue of endpoints rather than one model with modes.
//!
//! Everything here follows fal.ai's own documentation as read on
//! 2026-11-05 (docs/tools/graphics.md, ADR-031):
//!
//! - **Auth**: `Authorization: Key <FAL_KEY>` on every request.
//! - **Queue API** on `https://queue.fal.run`: `POST /{endpoint_id}`
//!   submits and answers `request_id`, `status_url`, `response_url` and
//!   `cancel_url`; `GET …/requests/{id}/status` answers `IN_QUEUE`
//!   (`queue_position`), `IN_PROGRESS` or `COMPLETED`; `GET
//!   …/requests/{id}` is the output; `PUT …/requests/{id}/cancel` drops a
//!   queued request. The status, result and cancel URLs use only the
//!   endpoint's owner and app (`fal-ai/flux`, not `fal-ai/flux/dev`) -
//!   the full id answers 405 - so the URLs the submit returns are used
//!   as they are, and only built by hand when the answer lacks them.
//! - **Model list** on `https://api.fal.ai/v1/models` with `category`,
//!   `q`, `status`, `limit` and `cursor`; the key is optional there but
//!   lifts the rate limit, so it is sent when there is one.
//! - **Inputs**: a picture goes in as `image_url`, which the docs say may
//!   be a base64 data URI (fal's CDN upload is only documented for its
//!   SDKs). The Graphics tab's attachment is at most 1024 px on a side,
//!   so the body stays a few MB under the 64 MiB limit.
//! - **Outputs**: `images[0].url` (generation and most editors) or
//!   `image.url` (upscalers); a describing model answers `results`,
//!   `output`, `caption`, `text` or `description`. The URL is downloaded
//!   at once, because fal's media expires.

use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::ai::{AiConfig, AiError, Credentials, ImageJob, ImagePart, aspect_ratio, truncate};
use crate::ai_local::{tidy, urlencode};

/// Where jobs are submitted.
pub const FAL_QUEUE_BASE: &str = "https://queue.fal.run";
/// Where the model catalogue is read.
pub const FAL_API_BASE: &str = "https://api.fal.ai/v1";

/// A hosted job may still queue for a while behind other people's.
const JOB_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const POLL_EVERY: Duration = Duration::from_millis(1000);
/// The catalogue's page size, and how many pages a purpose may read.
const PAGE: usize = 100;
const MAX_PAGES: usize = 3;

/// What a fal.ai model is chosen for. The first four are the Graphics
/// activity's `ImageJob`s; the fifth turns a picture into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FalPurpose {
    Generate,
    Edit,
    Enhance,
    Upscale,
    Describe,
}

impl FalPurpose {
    pub const ALL: [FalPurpose; 5] = [
        FalPurpose::Generate,
        FalPurpose::Edit,
        FalPurpose::Enhance,
        FalPurpose::Upscale,
        FalPurpose::Describe,
    ];
    pub fn of(job: ImageJob) -> Self {
        match job {
            ImageJob::Generate => FalPurpose::Generate,
            ImageJob::Edit => FalPurpose::Edit,
            ImageJob::Enhance => FalPurpose::Enhance,
            ImageJob::Upscale => FalPurpose::Upscale,
        }
    }
    /// A Fluent-free name for logs and errors.
    pub fn name(self) -> &'static str {
        match self {
            FalPurpose::Generate => "generate",
            FalPurpose::Edit => "edit",
            FalPurpose::Enhance => "enhance",
            FalPurpose::Upscale => "upscale",
            FalPurpose::Describe => "describe",
        }
    }
    /// The catalogue queries that list this purpose's candidates: a
    /// category alone, or a category narrowed by search stems (the
    /// search is a prefix match, so `upscal` finds "upscaler" and
    /// "upscaling"). Enhance has no category of its own on fal.ai; it is
    /// the image-to-image models that restore, enhance, denoise, sharpen,
    /// deblur or retouch.
    fn queries(self) -> Vec<Vec<(&'static str, &'static str)>> {
        match self {
            FalPurpose::Generate => vec![vec![("category", "text-to-image")]],
            FalPurpose::Edit => vec![vec![("category", "image-to-image")]],
            FalPurpose::Enhance => ["restor", "enhanc", "denois", "sharpen", "deblur", "retouch"]
                .into_iter()
                .map(|q| vec![("category", "image-to-image"), ("q", q)])
                .collect(),
            FalPurpose::Upscale => ["upscal", "resolution"]
                .into_iter()
                .map(|q| vec![("category", "image-to-image"), ("q", q)])
                .collect(),
            FalPurpose::Describe => vec![
                vec![("category", "vision")],
                vec![("category", "image-to-text")],
            ],
        }
    }
}

/// The endpoint id chosen for each purpose, saved on the provider. Empty
/// means none was chosen yet, which the Graphics form says rather than
/// guessing one: a fal.ai model is billed per call, so the choice is the
/// user's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FalModels {
    pub generate: String,
    pub edit: String,
    pub enhance: String,
    pub upscale: String,
    pub describe: String,
}

impl FalModels {
    pub fn get(&self, p: FalPurpose) -> &str {
        match p {
            FalPurpose::Generate => &self.generate,
            FalPurpose::Edit => &self.edit,
            FalPurpose::Enhance => &self.enhance,
            FalPurpose::Upscale => &self.upscale,
            FalPurpose::Describe => &self.describe,
        }
    }
    pub fn get_mut(&mut self, p: FalPurpose) -> &mut String {
        match p {
            FalPurpose::Generate => &mut self.generate,
            FalPurpose::Edit => &mut self.edit,
            FalPurpose::Enhance => &mut self.enhance,
            FalPurpose::Upscale => &mut self.upscale,
            FalPurpose::Describe => &mut self.describe,
        }
    }
}

/// How a long job reports back while it runs: a token that stops it (the
/// queued request is cancelled on the server, not only forgotten) and a
/// line of status for the Jobs panel.
#[derive(Default)]
pub struct Watch {
    pub cancel: Option<CancellationToken>,
    pub on_status: Option<StatusSink>,
}

/// Where a job's status lines go.
pub type StatusSink = Box<dyn FnMut(&str) + Send>;

impl Watch {
    fn say(&mut self, text: &str) {
        if let Some(f) = self.on_status.as_mut() {
            f(text);
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.is_cancelled())
    }
}

/// The catalogue host: fal's own unless a base URL was given, in which
/// case that one serves both the queue and the list (a proxy, or a mock).
fn api_base(cfg: &AiConfig) -> String {
    if cfg.base_url.trim().is_empty() {
        FAL_API_BASE.to_string()
    } else {
        cfg.base()
    }
}

fn authed(req: reqwest::RequestBuilder, creds: &Credentials) -> reqwest::RequestBuilder {
    let key = creds.key.trim();
    if key.is_empty() {
        req
    } else {
        req.header(reqwest::header::AUTHORIZATION, format!("Key {key}"))
    }
}

/// fal's error bodies: `{"detail": "…"}`, a 422's `detail` array of
/// `{loc, msg}`, or `{"error": "…"}`; anything else is shown as it came.
pub(crate) fn fal_error(status: u16, body: &str) -> AiError {
    let message = match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            if let Some(s) = v.get("detail").and_then(Value::as_str) {
                s.to_string()
            } else if let Some(items) = v.get("detail").and_then(Value::as_array) {
                items
                    .iter()
                    .map(|i| {
                        let loc = i
                            .get("loc")
                            .and_then(Value::as_array)
                            .map(|l| {
                                l.iter()
                                    .filter_map(|x| match x {
                                        Value::String(s) => Some(s.clone()),
                                        Value::Number(n) => Some(n.to_string()),
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join(".")
                            })
                            .unwrap_or_default();
                        let msg = i.get("msg").and_then(Value::as_str).unwrap_or("invalid");
                        if loc.is_empty() {
                            msg.to_string()
                        } else {
                            format!("{loc}: {msg}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            } else if let Some(s) = v
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| v.get("message").and_then(Value::as_str))
            {
                s.to_string()
            } else {
                body.trim().to_string()
            }
        }
        Err(_) => body.trim().to_string(),
    };
    let message = if message.is_empty() {
        "no details".to_string()
    } else {
        truncate(&message, 300)
    };
    AiError::Http { status, message }
}

async fn read(resp: reqwest::Response) -> Result<(u16, Value), AiError> {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) && status != 202 {
        return Err(fal_error(status, &text));
    }
    let v: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).map_err(|e| AiError::Format(e.to_string()))?
    };
    Ok((status, v))
}

async fn send(req: reqwest::RequestBuilder) -> Result<(u16, Value), AiError> {
    let resp = req
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    read(resp).await
}

/// A picture as fal.ai takes it inline.
pub fn data_uri(img: &ImagePart) -> String {
    format!(
        "data:{};base64,{}",
        img.mime,
        base64::engine::general_purpose::STANDARD.encode(&img.data)
    )
}

/// The request body for one job. Models differ in the fields they read,
/// and fal ignores the ones a model lacks, so the common spellings are
/// all sent: a size as `image_size` and as an `aspect_ratio`, the source
/// as `image_url` and as a one-item `image_urls`, an upscale's ratio as
/// `upscale_factor` and `scale`. An enhancement is the upscale family at
/// a ratio of 1 with the refiner's strength as `creativity` (the
/// Clarity-style upscalers' name for how far it may move) and its inverse
/// as `resemblance`.
pub(crate) fn input_for(
    job: ImageJob,
    cfg: &AiConfig,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Value, AiError> {
    let prompt = prompt.trim();
    let picture = |what: &str| -> Result<String, AiError> {
        source
            .map(data_uri)
            .ok_or_else(|| AiError::Format(format!("no picture to {what}")))
    };
    let mut body = match job {
        ImageJob::Generate => json!({
            "prompt": prompt,
            "image_size": { "width": size.0.max(1), "height": size.1.max(1) },
            "aspect_ratio": aspect_ratio(size),
            "num_images": 1,
            "output_format": "png",
        }),
        ImageJob::Edit => {
            let url = picture("edit")?;
            json!({
                "image_url": url,
                "image_urls": [url],
                "prompt": prompt,
                "strength": tidy(strength.clamp(0.01, 1.0)),
                "num_images": 1,
                "output_format": "png",
            })
        }
        ImageJob::Enhance => {
            let url = picture("enhance")?;
            let d = cfg.image.enhance_denoise.clamp(0.0, 1.0);
            json!({
                "image_url": url,
                "image_urls": [url],
                "upscale_factor": 1,
                "scale": 1,
                "creativity": tidy(d),
                "resemblance": tidy(1.0 - d),
                "strength": tidy(d.max(0.01)),
                "num_images": 1,
                "output_format": "png",
            })
        }
        ImageJob::Upscale => {
            let url = picture("upscale")?;
            let ratio = cfg.image.upscale_ratio(size);
            json!({
                "image_url": url,
                "image_urls": [url],
                "upscale_factor": tidy(ratio),
                "scale": tidy(ratio),
                "output_format": "png",
            })
        }
    };
    // Enhancing and upscaling need no prompt; one is passed on when given,
    // because some of those models read it.
    if job != ImageJob::Generate && job != ImageJob::Edit && !prompt.is_empty() {
        body["prompt"] = Value::String(prompt.to_string());
    }
    Ok(body)
}

/// The picture a job produced: `images[0].url` or `image.url`.
pub(crate) fn picture_url(output: &Value) -> Option<String> {
    output
        .pointer("/images/0/url")
        .or_else(|| output.pointer("/image/url"))
        .or_else(|| output.pointer("/output/url"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The text a describing model produced, under whichever name it uses.
pub(crate) fn description(output: &Value) -> Option<String> {
    for key in [
        "results",
        "output",
        "caption",
        "text",
        "description",
        "answer",
    ] {
        match output.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => return Some(s.trim().to_string()),
            Some(Value::Array(a)) => {
                let lines: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
                if !lines.is_empty() {
                    return Some(lines.join("\n"));
                }
            }
            _ => {}
        }
    }
    None
}

/// The status, result and cancel URLs: the submit answer's, else built
/// from the endpoint's owner and app alone, as the docs say.
pub(crate) fn queue_urls(base: &str, model: &str, submit: &Value) -> (String, String, String) {
    let id = submit
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let app: String = model
        .trim_matches('/')
        .split('/')
        .take(2)
        .collect::<Vec<_>>()
        .join("/");
    let built = |suffix: &str| format!("{base}/{app}/requests/{id}{suffix}");
    let pick = |key: &str, suffix: &str| {
        submit
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| built(suffix))
    };
    (
        pick("status_url", "/status"),
        pick("response_url", ""),
        pick("cancel_url", "/cancel"),
    )
}

/// Settings → Test Connection: does fal.ai accept this key? The status
/// of a request id that cannot exist costs nothing and answers 401 for a
/// bad key and 404 for a good one.
pub async fn ping(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<String, AiError> {
    if creds.key.trim().is_empty() {
        return Err(AiError::Format("fal.ai needs an API key".into()));
    }
    let url = format!(
        "{}/fal-ai/flux/requests/00000000-0000-0000-0000-000000000000/status",
        cfg.base()
    );
    let resp = authed(http.get(url), creds)
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    match status {
        401 | 403 => Err(fal_error(status, &text)),
        404 | 200 | 202 | 405 | 422 => Ok("fal.ai accepts this key".to_string()),
        _ => Err(fal_error(status, &text)),
    }
}

/// The endpoint ids that fit a purpose, from the catalogue, sorted and
/// without repeats.
pub async fn list_models(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    purpose: FalPurpose,
) -> Result<Vec<String>, AiError> {
    let base = api_base(cfg);
    let mut ids: Vec<String> = Vec::new();
    for query in purpose.queries() {
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut params: Vec<(&str, String)> =
                query.iter().map(|(k, v)| (*k, v.to_string())).collect();
            params.push(("status", "active".into()));
            params.push(("limit", PAGE.to_string()));
            if let Some(c) = &cursor {
                params.push(("cursor", c.clone()));
            }
            let query = params
                .iter()
                .map(|(k, v)| format!("{k}={}", urlencode(v)))
                .collect::<Vec<_>>()
                .join("&");
            let (_, v) = send(authed(http.get(format!("{base}/models?{query}")), creds)).await?;
            let page = v.get("models").and_then(Value::as_array);
            let Some(page) = page else {
                return Err(AiError::Format(truncate(&v.to_string(), 300)));
            };
            ids.extend(
                page.iter()
                    .filter_map(|m| m.get("endpoint_id").and_then(Value::as_str))
                    .map(str::to_string),
            );
            cursor = v
                .get("next_cursor")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Submit `input` to `model` on the queue, wait for it, and return its
/// output. `watch` may stop it (the request is cancelled on the server)
/// and hears the queue position and the running state.
pub async fn run(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    model: &str,
    input: Value,
    mut watch: Watch,
) -> Result<Value, AiError> {
    let model = model.trim().trim_matches('/');
    if model.is_empty() {
        return Err(AiError::Format(
            "no fal.ai model chosen for this purpose; pick one in Settings → AI Assistant".into(),
        ));
    }
    if creds.key.trim().is_empty() {
        return Err(AiError::Format("fal.ai needs an API key".into()));
    }
    let base = cfg.base();
    watch.say("Submitting");
    let (_, submitted) = send(authed(
        http.post(format!("{base}/{model}")).json(&input),
        creds,
    ))
    .await?;
    let (status_url, result_url, cancel_url) = queue_urls(&base, model, &submitted);
    let started = Instant::now();
    let mut first = true;
    loop {
        if watch.cancelled() {
            let _ = authed(http.put(&cancel_url), creds).send().await;
            return Err(AiError::Network("cancelled".into()));
        }
        if !first {
            match watch.cancel.clone() {
                Some(token) => {
                    tokio::select! {
                        _ = token.cancelled() => continue,
                        _ = tokio::time::sleep(POLL_EVERY) => {}
                    }
                }
                None => tokio::time::sleep(POLL_EVERY).await,
            }
        }
        first = false;
        if started.elapsed() > JOB_TIMEOUT {
            let _ = authed(http.put(&cancel_url), creds).send().await;
            return Err(AiError::Network(
                "fal.ai did not finish within twenty minutes".into(),
            ));
        }
        let (_, st) = send(authed(http.get(&status_url), creds)).await?;
        match st.get("status").and_then(Value::as_str).unwrap_or("") {
            "IN_QUEUE" => {
                let pos = st.get("queue_position").and_then(Value::as_u64);
                watch.say(&match pos {
                    Some(p) => format!("In queue, position {p}"),
                    None => "In queue".to_string(),
                });
            }
            "IN_PROGRESS" => watch.say("Running"),
            "COMPLETED" => break,
            other => {
                return Err(AiError::Format(format!(
                    "unexpected queue status {other:?}: {}",
                    truncate(&st.to_string(), 200)
                )));
            }
        }
    }
    watch.say("Fetching the result");
    let (_, output) = send(authed(http.get(&result_url), creds)).await?;
    if let Some(err) = output.get("error").and_then(Value::as_str) {
        return Err(AiError::Format(truncate(err, 300)));
    }
    Ok(output)
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, AiError> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AiError::Http {
            status: status.as_u16(),
            message: format!("downloading the picture from {url}"),
        });
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AiError::Network(e.to_string()))
}

/// Make, edit, enhance or upscale a picture through `model`; the result
/// is the picture's bytes.
#[allow(clippy::too_many_arguments)]
pub async fn generate_image(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
    watch: Watch,
) -> Result<Vec<u8>, AiError> {
    let input = input_for(job, cfg, prompt, size, source, strength)?;
    let output = run(http, cfg, creds, model, input, watch).await?;
    let url =
        picture_url(&output).ok_or_else(|| AiError::Format(truncate(&output.to_string(), 300)))?;
    download(http, &url).await
}

/// Describe a picture through `model`: a caption, or the answer to
/// `prompt` when the model takes a question.
pub async fn describe_image(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    model: &str,
    prompt: &str,
    source: &ImagePart,
    watch: Watch,
) -> Result<String, AiError> {
    let url = data_uri(source);
    let mut input = json!({ "image_url": url, "image_urls": [url] });
    if !prompt.trim().is_empty() {
        input["prompt"] = Value::String(prompt.trim().to_string());
    }
    let output = run(http, cfg, creds, model, input, watch).await?;
    description(&output).ok_or_else(|| AiError::Format(truncate(&output.to_string(), 300)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::UpscaleMode;

    fn png() -> ImagePart {
        ImagePart::png(vec![0x89, b'P', b'N', b'G'])
    }

    #[test]
    fn each_image_job_maps_to_its_purpose_and_the_fifth_stands_alone() {
        assert_eq!(FalPurpose::of(ImageJob::Generate), FalPurpose::Generate);
        assert_eq!(FalPurpose::of(ImageJob::Edit), FalPurpose::Edit);
        assert_eq!(FalPurpose::of(ImageJob::Enhance), FalPurpose::Enhance);
        assert_eq!(FalPurpose::of(ImageJob::Upscale), FalPurpose::Upscale);
        let mut m = FalModels::default();
        for p in FalPurpose::ALL {
            assert!(m.get(p).is_empty(), "{}", p.name());
            *m.get_mut(p) = p.name().to_string();
        }
        assert_eq!(m.describe, "describe");
        assert_eq!(m.get(FalPurpose::Upscale), "upscale");
    }

    /// A provider saved before fal.ai existed reads back with empty
    /// choices rather than failing to parse.
    #[test]
    fn an_older_provider_has_no_fal_models_yet() {
        let m: FalModels = toml::from_str("").unwrap();
        assert_eq!(m, FalModels::default());
        let m: FalModels = toml::from_str("generate = \"fal-ai/flux/dev\"").unwrap();
        assert_eq!(m.generate, "fal-ai/flux/dev");
        assert!(m.edit.is_empty());
    }

    #[test]
    fn the_source_travels_as_a_data_uri() {
        let uri = data_uri(&png());
        assert!(uri.starts_with("data:image/png;base64,"), "{uri}");
        assert!(uri.ends_with("iVBORw=="), "{uri}");
    }

    #[test]
    fn each_job_sends_the_documented_fields() {
        let mut cfg = AiConfig::default();
        cfg.image.enhance_denoise = 0.3;
        cfg.image.upscale_factor = 2;
        let src = png();
        let g = input_for(ImageJob::Generate, &cfg, " a cat ", (1024, 768), None, 0.5).unwrap();
        assert_eq!(g["prompt"], "a cat");
        assert_eq!(g["image_size"]["width"], 1024);
        assert_eq!(g["image_size"]["height"], 768);
        assert_eq!(g["aspect_ratio"], "4:3");
        assert_eq!(g["num_images"], 1);
        assert_eq!(g["output_format"], "png");
        assert!(g.get("image_url").is_none());

        let e = input_for(ImageJob::Edit, &cfg, "winter", (64, 64), Some(&src), 0.7).unwrap();
        assert_eq!(e["prompt"], "winter");
        assert_eq!(e["strength"], 0.7);
        assert!(
            e["image_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png")
        );
        assert_eq!(e["image_urls"][0], e["image_url"]);

        // An enhancement never resizes: the ratio is 1, and the refiner's
        // strength is the creativity.
        let n = input_for(ImageJob::Enhance, &cfg, "", (64, 64), Some(&src), 0.5).unwrap();
        assert_eq!(n["upscale_factor"], 1);
        assert_eq!(n["scale"], 1);
        assert_eq!(n["creativity"], 0.3);
        assert_eq!(n["resemblance"], 0.7);
        assert!(n.get("prompt").is_none(), "an empty prompt is not sent");

        let u = input_for(ImageJob::Upscale, &cfg, "crisp", (64, 64), Some(&src), 0.5).unwrap();
        assert_eq!(u["upscale_factor"], 2.0);
        assert_eq!(u["scale"], 2.0);
        assert_eq!(u["prompt"], "crisp");
        // A target size becomes the ratio the upscalers take.
        cfg.image.upscale_mode = UpscaleMode::Dimensions;
        cfg.image.upscale_width = 256;
        cfg.image.upscale_height = 0;
        let u = input_for(ImageJob::Upscale, &cfg, "", (64, 64), Some(&src), 0.5).unwrap();
        assert_eq!(u["upscale_factor"], 4.0);

        for job in [ImageJob::Edit, ImageJob::Enhance, ImageJob::Upscale] {
            let err = input_for(job, &cfg, "x", (64, 64), None, 0.5)
                .unwrap_err()
                .to_string();
            assert!(err.contains("no picture"), "{err}");
        }
    }

    #[test]
    fn the_output_is_read_under_either_shape() {
        let generated =
            json!({ "images": [{ "url": "https://v3.fal.media/a.png", "width": 1, "height": 1 }] });
        assert_eq!(
            picture_url(&generated).as_deref(),
            Some("https://v3.fal.media/a.png")
        );
        let upscaled = json!({ "image": { "url": "https://v3.fal.media/b.png" }, "seed": 1 });
        assert_eq!(
            picture_url(&upscaled).as_deref(),
            Some("https://v3.fal.media/b.png")
        );
        assert!(picture_url(&json!({ "seed": 1 })).is_none());

        assert_eq!(
            description(&json!({ "results": " A red car. " })).as_deref(),
            Some("A red car.")
        );
        assert_eq!(
            description(&json!({ "output": "Two cats" })).as_deref(),
            Some("Two cats")
        );
        assert_eq!(
            description(&json!({ "output": ["one", "two"] })).as_deref(),
            Some("one\ntwo")
        );
        assert!(description(&json!({ "results": "" })).is_none());
    }

    /// fal's 405 trap: the status URL takes the owner and app, never the
    /// full endpoint id. The submit answer's URLs win when present.
    #[test]
    fn queue_urls_come_from_the_answer_or_from_owner_and_app() {
        let answer = json!({
            "request_id": "abc",
            "status_url": "https://queue.fal.run/fal-ai/flux/requests/abc/status",
            "response_url": "https://queue.fal.run/fal-ai/flux/requests/abc",
            "cancel_url": "https://queue.fal.run/fal-ai/flux/requests/abc/cancel",
        });
        let (s, r, c) = queue_urls("http://mock", "fal-ai/flux/dev", &answer);
        assert_eq!(s, "https://queue.fal.run/fal-ai/flux/requests/abc/status");
        assert_eq!(r, "https://queue.fal.run/fal-ai/flux/requests/abc");
        assert_eq!(c, "https://queue.fal.run/fal-ai/flux/requests/abc/cancel");
        let bare = json!({ "request_id": "abc" });
        let (s, r, c) = queue_urls("http://mock", "fal-ai/flux/dev/image-to-image", &bare);
        assert_eq!(s, "http://mock/fal-ai/flux/requests/abc/status");
        assert_eq!(r, "http://mock/fal-ai/flux/requests/abc");
        assert_eq!(c, "http://mock/fal-ai/flux/requests/abc/cancel");
    }

    #[test]
    fn fal_errors_are_read_in_their_three_shapes() {
        let e = fal_error(401, r#"{"detail":"invalid key credentials"}"#).to_string();
        assert!(
            e.contains("401") && e.contains("invalid key credentials"),
            "{e}"
        );
        let e = fal_error(
            422,
            r#"{"detail":[{"loc":["body","prompt"],"msg":"field required","type":"missing"}]}"#,
        )
        .to_string();
        assert!(e.contains("body.prompt: field required"), "{e}");
        let e = fal_error(500, r#"{"error":"boom"}"#).to_string();
        assert!(e.contains("boom"), "{e}");
        let e = fal_error(502, "Bad Gateway").to_string();
        assert!(e.contains("Bad Gateway"), "{e}");
    }

    #[test]
    fn every_purpose_asks_the_catalogue_for_active_image_models() {
        for p in FalPurpose::ALL {
            let qs = p.queries();
            assert!(!qs.is_empty(), "{}", p.name());
            for q in qs {
                assert!(
                    q.iter().any(|(k, _)| *k == "category"),
                    "{} queries without a category",
                    p.name()
                );
            }
        }
        assert_eq!(
            FalPurpose::Generate.queries(),
            vec![vec![("category", "text-to-image")]]
        );
    }
}
