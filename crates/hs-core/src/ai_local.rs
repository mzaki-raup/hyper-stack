//! Self-hosted image servers behind `ai::generate_image`: the Stable
//! Diffusion WebUI API (AUTOMATIC1111, SD.Next, Forge, Stability Matrix's
//! packages of them), ComfyUI (and SwarmUI's ComfyUI backend), SwarmUI's
//! own API, InvokeAI and Fooocus-API. Each takes the four jobs - a new
//! picture, an edit of the current one, an enhancement (same size,
//! better picture) and an upscale (more pixels, same picture) - and the
//! `ImageWorkflow` the provider carries (checkpoint, sampling, upscaler,
//! ComfyUI workflows, extra fields).
//!
//! Every request shape here follows the projects' own API documentation
//! as read on 2026-10-22; none has been exercised against a live server
//! from this machine (see `docs/tools/graphics.md`).

use std::time::Duration;

use base64::Engine as _;
use serde_json::{Map, Value, json};

use crate::ai::{
    AiConfig, AiError, Credentials, ImageJob, ImagePart, Provider, error_message, multipart_file,
    sdxl_size, truncate,
};

/// How long a picture may take: a CPU-only server needs minutes.
const JOB_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const POLL_EVERY: Duration = Duration::from_millis(900);

/// A client that waits as long as a slow server needs; `ai::http_client`
/// gives up after two silent minutes, which a CPU render exceeds.
fn long_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(JOB_TIMEOUT)
        .user_agent(concat!("HyperStack/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// The server's credential, as each one takes it: the WebUI's
/// `--api-auth user:password` is HTTP Basic, SwarmUI's account token is a
/// cookie, the rest accept a bearer token (a reverse proxy, usually) and
/// run open otherwise.
fn auth(
    r: reqwest::RequestBuilder,
    cfg: &AiConfig,
    creds: &Credentials,
) -> reqwest::RequestBuilder {
    let key = creds.key.trim();
    if key.is_empty() {
        return r;
    }
    match cfg.provider {
        Provider::SdWebUi => match key.split_once(':') {
            Some((u, p)) => r.basic_auth(u, Some(p)),
            None => r.bearer_auth(key),
        },
        Provider::SwarmUi => r.header(reqwest::header::COOKIE, format!("swarm_token={key}")),
        _ => r.bearer_auth(key),
    }
}

async fn send_json(r: reqwest::RequestBuilder) -> Result<Value, AiError> {
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
    serde_json::from_str(&text).map_err(|_| AiError::Format(truncate(&text, 300)))
}

async fn fetch_bytes(r: reqwest::RequestBuilder) -> Result<Vec<u8>, AiError> {
    let resp = r
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
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AiError::Network(e.to_string()))
}

/// Percent-encode a query value (the workspace's reqwest has no `query`
/// feature).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode a base64 picture, with or without a `data:` prefix.
fn unb64(text: &str) -> Result<Vec<u8>, AiError> {
    let data = text.split_once(";base64,").map(|(_, d)| d).unwrap_or(text);
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| AiError::Format(format!("picture is not base64: {e}")))
}

/// The provider's `extra` JSON, merged over a request body.
fn apply_extra(body: &mut Value, cfg: &AiConfig) -> Result<(), AiError> {
    let extra = cfg.image.extra.trim();
    if extra.is_empty() {
        return Ok(());
    }
    let v: Value = serde_json::from_str(extra)
        .map_err(|e| AiError::Format(format!("the provider's extra JSON does not parse: {e}")))?;
    let Value::Object(map) = v else {
        return Err(AiError::Format(
            "the provider's extra JSON must be an object".into(),
        ));
    };
    if let Value::Object(b) = body {
        for (k, v) in map {
            b.insert(k, v);
        }
    }
    Ok(())
}

/// The checkpoint to use: the one asked for, else the provider's.
fn checkpoint<'a>(cfg: &'a AiConfig, model: &'a str) -> &'a str {
    let m = model.trim();
    if m.is_empty() {
        cfg.image.checkpoint.trim()
    } else {
        m
    }
}

/// Upscaler for Enhance, with the server's conventional default.
fn upscaler<'a>(cfg: &'a AiConfig, default: &'a str) -> &'a str {
    let u = cfg.image.upscaler.trim();
    if u.is_empty() { default } else { u }
}

/// An `f32` setting as the number a server reads: four decimals, so 0.7
/// arrives as `0.7` and not `0.699999988079071`.
pub(crate) fn tidy(x: f32) -> f64 {
    (x as f64 * 10_000.0).round() / 10_000.0
}

/// How far an Enhance may move the picture. Enhancement is a refiner
/// pass, so this is small by nature; 0 would be a no-op, which no server
/// enjoys being asked for.
fn enhance_denoise(cfg: &AiConfig) -> f32 {
    cfg.image.enhance_denoise.clamp(0.02, 1.0)
}

/// Steps for the refiner pass: its own setting, else the provider's.
fn enhance_steps(cfg: &AiConfig) -> u32 {
    if cfg.image.enhance_steps > 0 {
        cfg.image.enhance_steps
    } else {
        cfg.image.steps.max(1)
    }
}

/// The checkpoints a server offers, by its own listing route.
pub(crate) async fn list_checkpoints(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<Vec<String>, AiError> {
    let base = cfg.base();
    let names: Vec<String> = match cfg.provider {
        Provider::SdWebUi => {
            let v = send_json(auth(
                http.get(format!("{base}/sdapi/v1/sd-models")),
                cfg,
                creds,
            ))
            .await?;
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|m| {
                            m.get("title")
                                .or(m.get("model_name"))
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        Provider::ComfyUi => {
            comfy_choices(http, cfg, creds, "CheckpointLoaderSimple", "ckpt_name").await?
        }
        Provider::SwarmUi => {
            let session = swarm_session(http, cfg, creds).await?;
            let v = send_json(auth(
                http.post(format!("{base}/API/ListModels")).json(&json!({
                    "session_id": session,
                    "path": "",
                    "depth": 3,
                    "subtype": "Stable-Diffusion",
                })),
                cfg,
                creds,
            ))
            .await?;
            v.get("files")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        }
        Provider::InvokeAi => invoke_models(http, cfg, creds)
            .await?
            .into_iter()
            .filter_map(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
            .collect(),
        Provider::Fooocus => {
            let v = send_json(auth(
                http.get(format!("{base}/v1/engines/all-models")),
                cfg,
                creds,
            ))
            .await?;
            v.get("model_filenames")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    Ok(names)
}

/// A reachability check for Settings → Test Connection: the server's
/// version or name, from the cheapest route each one has.
pub async fn ping(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<String, AiError> {
    let base = cfg.base();
    let text = match cfg.provider {
        Provider::SdWebUi => {
            let v = send_json(auth(
                http.get(format!("{base}/sdapi/v1/options")),
                cfg,
                creds,
            ))
            .await?;
            let ckpt = v
                .get("sd_model_checkpoint")
                .and_then(Value::as_str)
                .unwrap_or("(no checkpoint loaded)");
            format!("Stable Diffusion WebUI, checkpoint {ckpt}")
        }
        Provider::ComfyUi => {
            let v = send_json(auth(http.get(format!("{base}/system_stats")), cfg, creds)).await?;
            let ver = v
                .pointer("/system/comfyui_version")
                .and_then(Value::as_str)
                .unwrap_or("(version not reported)");
            format!("ComfyUI {ver}")
        }
        Provider::SwarmUi => {
            let v = send_json(auth(
                http.post(format!("{base}/API/GetNewSession"))
                    .json(&json!({})),
                cfg,
                creds,
            ))
            .await?;
            let ver = v.get("version").and_then(Value::as_str).unwrap_or("");
            format!("SwarmUI {ver}").trim().to_string()
        }
        Provider::InvokeAi => {
            let v = send_json(auth(
                http.get(format!("{base}/api/v1/app/version")),
                cfg,
                creds,
            ))
            .await?;
            let ver = v.get("version").and_then(Value::as_str).unwrap_or("");
            format!("InvokeAI {ver}").trim().to_string()
        }
        Provider::Fooocus => {
            let bytes = fetch_bytes(auth(http.get(format!("{base}/ping")), cfg, creds)).await?;
            format!("Fooocus-API: {}", String::from_utf8_lossy(&bytes).trim())
        }
        other => {
            return Err(AiError::Format(format!(
                "{} is not an image server",
                other.label()
            )));
        }
    };
    Ok(text)
}

/// Make, edit or enhance a picture on a self-hosted server. `size` is the
/// picture to make (Generate) or the source's size (Edit / Enhance).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn generate_image(
    _http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let http = long_client();
    let source = match (job, source) {
        (ImageJob::Generate, _) => None,
        (_, Some(img)) => Some(img),
        (_, None) => return Err(AiError::Format("no picture to work on".into())),
    };
    let strength = strength.clamp(0.0, 1.0);
    match cfg.provider {
        Provider::SdWebUi => {
            webui(
                &http, cfg, creds, job, model, prompt, size, source, strength,
            )
            .await
        }
        Provider::ComfyUi => {
            comfy(
                &http, cfg, creds, job, model, prompt, size, source, strength,
            )
            .await
        }
        Provider::SwarmUi => {
            swarm(
                &http, cfg, creds, job, model, prompt, size, source, strength,
            )
            .await
        }
        Provider::InvokeAi => {
            invoke(
                &http, cfg, creds, job, model, prompt, size, source, strength,
            )
            .await
        }
        Provider::Fooocus => {
            fooocus(
                &http, cfg, creds, job, model, prompt, size, source, strength,
            )
            .await
        }
        other => Err(AiError::Format(format!(
            "{} is not an image server",
            other.label()
        ))),
    }
}

// ---------------------------------------------------------------------------
// Stable Diffusion WebUI API (AUTOMATIC1111, SD.Next, Forge)

#[allow(clippy::too_many_arguments)]
async fn webui(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let base = cfg.base();
    let w = &cfg.image;
    if job == ImageJob::Upscale {
        // The Extras route takes either shape: mode 0 scales by a ratio,
        // mode 1 to a width and a height (owner's report: some servers
        // only do one of the two).
        let img = source.expect("checked");
        let mut body = json!({
            "image": b64(&img.data),
            "upscaler_1": upscaler(cfg, "R-ESRGAN 4x+"),
        });
        if w.upscale_mode == crate::ai::UpscaleMode::Ratio {
            body["resize_mode"] = json!(0);
            body["upscaling_resize"] = json!(tidy(w.upscale_ratio(size)));
        } else {
            let (tw, th) = w.upscale_target(size);
            body["resize_mode"] = json!(1);
            body["upscaling_resize_w"] = json!(tw);
            body["upscaling_resize_h"] = json!(th);
            body["upscaling_crop"] = json!(false);
        }
        apply_extra(&mut body, cfg)?;
        let v = send_json(auth(
            http.post(format!("{base}/sdapi/v1/extra-single-image"))
                .json(&body),
            cfg,
            creds,
        ))
        .await?;
        let text = v
            .get("image")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::Format("the server returned no picture".into()))?;
        return unb64(text);
    }
    if job == ImageJob::Enhance && w.enhance_denoise <= 0.0 {
        // Restoration only, at the source's own size: Extras with a
        // resize of 1 runs GFPGAN/CodeFormer and nothing else.
        let img = source.expect("checked");
        let v = w.enhance_face_strength.clamp(0.0, 1.0);
        let mut body = json!({
            "image": b64(&img.data),
            "resize_mode": 0,
            "upscaling_resize": 1,
            "gfpgan_visibility": tidy(if w.enhance_face_restore { v } else { 0.0 }),
            "codeformer_visibility": tidy(if w.enhance_face_restore { v } else { 0.0 }),
            "codeformer_weight": tidy(1.0 - v),
        });
        apply_extra(&mut body, cfg)?;
        let r = send_json(auth(
            http.post(format!("{base}/sdapi/v1/extra-single-image"))
                .json(&body),
            cfg,
            creds,
        ))
        .await?;
        let text = r
            .get("image")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::Format("the server returned no picture".into()))?;
        return unb64(text);
    }
    // An Enhance is img2img at a low denoise and the *same* size, with the
    // restorer on when asked for; an Edit is the same route at the user's
    // strength. Neither resizes: that is Upscale's job.
    let enhancing = job == ImageJob::Enhance;
    let mut body = json!({
        "prompt": prompt,
        "negative_prompt": w.negative_prompt,
        "steps": if enhancing { enhance_steps(cfg) } else { w.steps.max(1) },
        "cfg_scale": tidy(w.cfg_scale),
        "width": size.0,
        "height": size.1,
        "seed": w.seed,
        "restore_faces": enhancing && w.enhance_face_restore,
        "batch_size": 1,
        "n_iter": 1,
        "override_settings_restore_afterwards": false,
    });
    if !w.sampler.trim().is_empty() {
        body["sampler_name"] = Value::String(w.sampler.trim().into());
    }
    if !w.scheduler.trim().is_empty() {
        body["scheduler"] = Value::String(w.scheduler.trim().into());
    }
    let ckpt = checkpoint(cfg, model);
    if !ckpt.is_empty() {
        body["override_settings"] = json!({ "sd_model_checkpoint": ckpt });
    }
    let route = match job {
        ImageJob::Generate => "txt2img",
        _ => {
            // An edit moves as far as the user asked; an enhancement only
            // as far as the refiner setting allows.
            let img = source.expect("checked");
            let d = if enhancing {
                enhance_denoise(cfg)
            } else {
                strength
            };
            body["init_images"] = json!([b64(&img.data)]);
            body["denoising_strength"] = json!(tidy(d));
            "img2img"
        }
    };
    apply_extra(&mut body, cfg)?;
    let v = send_json(auth(
        http.post(format!("{base}/sdapi/v1/{route}")).json(&body),
        cfg,
        creds,
    ))
    .await?;
    let text = v
        .pointer("/images/0")
        .and_then(Value::as_str)
        .ok_or_else(|| AiError::Format("the server returned no picture".into()))?;
    unb64(text)
}

// ---------------------------------------------------------------------------
// ComfyUI

/// The built-in workflows, in ComfyUI's API JSON form. Placeholders in
/// quotes stand for numbers too: `"{{steps}}"` becomes `20`.
pub const COMFY_GENERATE: &str = r#"{
  "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "{{checkpoint}}"}},
  "5": {"class_type": "EmptyLatentImage", "inputs": {"width": "{{width}}", "height": "{{height}}", "batch_size": 1}},
  "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{prompt}}", "clip": ["4", 1]}},
  "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{negative}}", "clip": ["4", 1]}},
  "3": {"class_type": "KSampler", "inputs": {"seed": "{{seed}}", "steps": "{{steps}}", "cfg": "{{cfg}}", "sampler_name": "{{sampler}}", "scheduler": "{{scheduler}}", "denoise": 1.0, "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0], "latent_image": ["5", 0]}},
  "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
  "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": "HyperStack", "images": ["8", 0]}}
}"#;

pub const COMFY_EDIT: &str = r#"{
  "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "{{checkpoint}}"}},
  "10": {"class_type": "LoadImage", "inputs": {"image": "{{image}}"}},
  "11": {"class_type": "VAEEncode", "inputs": {"pixels": ["10", 0], "vae": ["4", 2]}},
  "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{prompt}}", "clip": ["4", 1]}},
  "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{negative}}", "clip": ["4", 1]}},
  "3": {"class_type": "KSampler", "inputs": {"seed": "{{seed}}", "steps": "{{steps}}", "cfg": "{{cfg}}", "sampler_name": "{{sampler}}", "scheduler": "{{scheduler}}", "denoise": "{{denoise}}", "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0], "latent_image": ["11", 0]}},
  "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
  "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": "HyperStack", "images": ["8", 0]}}
}"#;

/// Enhancement: the picture re-sampled at a low denoise, at its own
/// size. More detail, same subject, same pixels wide. Not a resize.
pub const COMFY_ENHANCE: &str = r#"{
  "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "{{checkpoint}}"}},
  "10": {"class_type": "LoadImage", "inputs": {"image": "{{image}}"}},
  "11": {"class_type": "VAEEncode", "inputs": {"pixels": ["10", 0], "vae": ["4", 2]}},
  "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{prompt}}", "clip": ["4", 1]}},
  "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{negative}}", "clip": ["4", 1]}},
  "3": {"class_type": "KSampler", "inputs": {"seed": "{{seed}}", "steps": "{{steps}}", "cfg": "{{cfg}}", "sampler_name": "{{sampler}}", "scheduler": "{{scheduler}}", "denoise": "{{denoise}}", "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0], "latent_image": ["11", 0]}},
  "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
  "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": "HyperStack", "images": ["8", 0]}}
}"#;

/// Upscaling: a super-resolution model, then a scale to the asked-for
/// size. `{{scale_by}}` is there for a workflow that would rather use
/// `ImageScaleBy` (a ratio) than `ImageScale` (a width and a height).
pub const COMFY_UPSCALE: &str = r#"{
  "10": {"class_type": "LoadImage", "inputs": {"image": "{{image}}"}},
  "12": {"class_type": "UpscaleModelLoader", "inputs": {"model_name": "{{upscaler}}"}},
  "13": {"class_type": "ImageUpscaleWithModel", "inputs": {"upscale_model": ["12", 0], "image": ["10", 0]}},
  "14": {"class_type": "ImageScale", "inputs": {"upscale_method": "lanczos", "width": "{{target_width}}", "height": "{{target_height}}", "crop": "disabled", "image": ["13", 0]}},
  "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": "HyperStack", "images": ["14", 0]}}
}"#;

/// The placeholders a ComfyUI workflow may carry.
pub const COMFY_PLACEHOLDERS: &[&str] = &[
    "prompt",
    "negative",
    "checkpoint",
    "sampler",
    "scheduler",
    "upscaler",
    "image",
    "seed",
    "steps",
    "cfg",
    "width",
    "height",
    "denoise",
    "target_width",
    "target_height",
    "scale_by",
];

/// Fill a workflow's placeholders. Numbers replace the *quoted* token so
/// the template stays valid JSON; strings are JSON-escaped in place.
pub fn fill_workflow(template: &str, strings: &[(&str, &str)], numbers: &[(&str, f64)]) -> String {
    let mut out = template.to_string();
    for (k, v) in numbers {
        let text = if v.fract() == 0.0 && v.abs() < 1e15 {
            format!("{}", *v as i64)
        } else {
            format!("{v}")
        };
        out = out.replace(&format!("\"{{{{{k}}}}}\""), &text);
        out = out.replace(&format!("{{{{{k}}}}}"), &text);
    }
    for (k, v) in strings {
        let escaped = serde_json::to_string(v).unwrap_or_default();
        let inner = &escaped[1..escaped.len() - 1];
        out = out.replace(&format!("{{{{{k}}}}}"), inner);
    }
    out
}

/// A node class's choice list (`/object_info/<class>` →
/// `input.required.<field>[0]`): checkpoints, upscale models…
async fn comfy_choices(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    class: &str,
    field: &str,
) -> Result<Vec<String>, AiError> {
    let v = send_json(auth(
        http.get(format!("{}/object_info/{class}", cfg.base())),
        cfg,
        creds,
    ))
    .await?;
    Ok(v.pointer(&format!("/{class}/input/required/{field}/0"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
async fn comfy(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let base = cfg.base();
    let w = &cfg.image;
    // Empty settings take the server's first choice rather than a name it
    // may not have.
    // Upscaling runs a super-resolution model and needs no checkpoint;
    // every other job, enhancement included, re-samples through one.
    let mut ckpt = checkpoint(cfg, model).to_string();
    if ckpt.is_empty() && job != ImageJob::Upscale {
        ckpt = comfy_choices(http, cfg, creds, "CheckpointLoaderSimple", "ckpt_name")
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AiError::Format("ComfyUI has no checkpoints".into()))?;
    }
    let mut upscale = w.upscaler.trim().to_string();
    if upscale.is_empty() && job == ImageJob::Upscale {
        upscale = comfy_choices(http, cfg, creds, "UpscaleModelLoader", "model_name")
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AiError::Format("ComfyUI has no upscale models".into()))?;
    }
    let image_name = match source {
        Some(img) => {
            let (boundary, body) = multipart_file("image", "hyperstack.png", &img.mime, &img.data);
            let v = send_json(auth(
                http.post(format!("{base}/upload/image"))
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(body),
                cfg,
                creds,
            ))
            .await?;
            let name = v.get("name").and_then(Value::as_str).ok_or_else(|| {
                AiError::Format("ComfyUI did not name the uploaded picture".into())
            })?;
            match v.get("subfolder").and_then(Value::as_str) {
                Some(sub) if !sub.is_empty() => format!("{sub}/{name}"),
                _ => name.to_string(),
            }
        }
        None => String::new(),
    };
    let (template, custom) = match job {
        ImageJob::Generate => (COMFY_GENERATE, &w.comfy_generate),
        ImageJob::Edit => (COMFY_EDIT, &w.comfy_edit),
        ImageJob::Enhance => (COMFY_ENHANCE, &w.comfy_enhance),
        ImageJob::Upscale => (COMFY_UPSCALE, &w.comfy_upscale),
    };
    let template = if custom.trim().is_empty() {
        template
    } else {
        custom.as_str()
    };
    let sampler = if w.sampler.trim().is_empty() {
        "euler"
    } else {
        w.sampler.trim()
    };
    let scheduler = if w.scheduler.trim().is_empty() {
        "normal"
    } else {
        w.scheduler.trim()
    };
    let seed = if w.seed < 0 {
        random_seed()
    } else {
        w.seed as f64
    };
    let (target_w, target_h) = w.upscale_target(size);
    let denoise = match job {
        ImageJob::Enhance => enhance_denoise(cfg),
        _ => strength,
    };
    let steps = match job {
        ImageJob::Enhance => enhance_steps(cfg),
        _ => w.steps.max(1),
    };
    let filled = fill_workflow(
        template,
        &[
            ("prompt", prompt),
            ("negative", w.negative_prompt.as_str()),
            ("checkpoint", ckpt.as_str()),
            ("sampler", sampler),
            ("scheduler", scheduler),
            ("upscaler", upscale.as_str()),
            ("image", image_name.as_str()),
        ],
        &[
            ("seed", seed),
            ("steps", steps as f64),
            ("cfg", tidy(w.cfg_scale)),
            ("width", size.0 as f64),
            ("height", size.1 as f64),
            ("denoise", tidy(denoise)),
            ("target_width", target_w as f64),
            ("target_height", target_h as f64),
            ("scale_by", tidy(w.upscale_ratio(size))),
        ],
    );
    let workflow: Value = serde_json::from_str(&filled)
        .map_err(|e| AiError::Format(format!("the ComfyUI workflow does not parse: {e}")))?;
    let client_id = format!("hyperstack-{}", crate::new_id());
    let mut body = json!({ "prompt": workflow, "client_id": client_id });
    apply_extra(&mut body, cfg)?;
    let v = send_json(auth(
        http.post(format!("{base}/prompt")).json(&body),
        cfg,
        creds,
    ))
    .await?;
    if let Some(errs) = v
        .get("node_errors")
        .filter(|e| e.as_object().is_some_and(|o| !o.is_empty()))
    {
        return Err(AiError::Format(format!(
            "ComfyUI rejected the workflow: {}",
            truncate(&errs.to_string(), 300)
        )));
    }
    let prompt_id = v
        .get("prompt_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AiError::Format(truncate(&v.to_string(), 300)))?
        .to_string();
    let started = std::time::Instant::now();
    loop {
        tokio::time::sleep(POLL_EVERY).await;
        let h = send_json(auth(
            http.get(format!("{base}/history/{prompt_id}")),
            cfg,
            creds,
        ))
        .await?;
        if let Some(entry) = h.get(&prompt_id) {
            if entry.pointer("/status/status_str").and_then(Value::as_str) == Some("error") {
                return Err(AiError::Format(comfy_error(entry)));
            }
            let image = entry
                .get("outputs")
                .and_then(Value::as_object)
                .and_then(|outs| {
                    outs.values().find_map(|o| {
                        o.get("images")
                            .and_then(Value::as_array)
                            .and_then(|a| a.first())
                    })
                });
            if let Some(image) = image {
                let filename = image.get("filename").and_then(Value::as_str).unwrap_or("");
                let subfolder = image.get("subfolder").and_then(Value::as_str).unwrap_or("");
                let kind = image
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("output");
                return fetch_bytes(auth(
                    http.get(format!(
                        "{base}/view?filename={}&subfolder={}&type={}",
                        urlencode(filename),
                        urlencode(subfolder),
                        urlencode(kind)
                    )),
                    cfg,
                    creds,
                ))
                .await;
            }
            if entry.pointer("/status/completed").and_then(Value::as_bool) == Some(true) {
                return Err(AiError::Format(
                    "the workflow finished without a SaveImage output".into(),
                ));
            }
        }
        if started.elapsed() > JOB_TIMEOUT {
            return Err(AiError::Network("ComfyUI did not finish in time".into()));
        }
    }
}

/// A readable line from ComfyUI's status messages: the failing node and
/// its exception rather than the whole event list, plus the one hint that
/// explains the common case - a checkpoint with no text encoder or VAE of
/// its own (Z-Image, Flux, SD3 split files), which the built-in workflow's
/// CheckpointLoaderSimple cannot drive.
fn comfy_error(entry: &Value) -> String {
    let messages = entry
        .pointer("/status/messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let error = messages.iter().find_map(|m| {
        let kind = m.get(0).and_then(Value::as_str)?;
        (kind == "execution_error")
            .then(|| m.get(1).cloned())
            .flatten()
    });
    let Some(e) = error else {
        return format!(
            "ComfyUI: the workflow failed ({})",
            truncate(&Value::Array(messages).to_string(), 200)
        );
    };
    let node_type = e.get("node_type").and_then(Value::as_str).unwrap_or("node");
    let node_id = e.get("node_id").and_then(Value::as_str).unwrap_or("?");
    let message = e
        .get("exception_message")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    let mut out = format!(
        "ComfyUI: {node_type} (node {node_id}): {}",
        truncate(message, 300)
    );
    let inputs = e
        .get("current_inputs")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if inputs.contains("\"clip\":[null]")
        || inputs.contains("\"vae\":[null]")
        || message.contains("NoneType")
    {
        out.push_str(
            " - this checkpoint carries no text encoder or VAE of its own, so the \
             built-in workflow cannot drive it: in ComfyUI or SwarmUI, export the \
             workflow that loads it (Save (API Format)) and paste it under the \
             provider's ComfyUI Workflows in Settings",
        );
    }
    out
}

fn random_seed() -> f64 {
    // Enough entropy for a seed; no RNG dependency needed.
    let id = crate::new_id();
    let mut h: u64 = 1469598103934665603;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % 2_147_483_647) as f64
}

// ---------------------------------------------------------------------------
// SwarmUI

async fn swarm_session(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<String, AiError> {
    let v = send_json(auth(
        http.post(format!("{}/API/GetNewSession", cfg.base()))
            .json(&json!({})),
        cfg,
        creds,
    ))
    .await?;
    v.get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AiError::Format("SwarmUI gave no session".into()))
}

#[allow(clippy::too_many_arguments)]
async fn swarm(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let base = cfg.base();
    let w = &cfg.image;
    let session = swarm_session(http, cfg, creds).await?;
    // SwarmUI's own upscale lives in its refiner stage, which needs a
    // second model, so both jobs go through the init-image route: an
    // Upscale asks for the larger size, an Enhance for the same size at
    // a low creativity (detail, not content).
    let (width, height, creativity) = match job {
        ImageJob::Upscale => {
            let (tw, th) = w.upscale_target(size);
            (tw, th, 0.25)
        }
        ImageJob::Enhance => (size.0, size.1, enhance_denoise(cfg)),
        _ => (size.0, size.1, strength),
    };
    let mut body = json!({
        "session_id": session,
        "images": 1,
        "prompt": prompt,
        "negativeprompt": w.negative_prompt,
        "width": width,
        "height": height,
        "steps": if job == ImageJob::Enhance { enhance_steps(cfg) } else { w.steps.max(1) },
        "cfgscale": tidy(w.cfg_scale),
        "seed": w.seed,
    });
    let ckpt = checkpoint(cfg, model);
    if !ckpt.is_empty() {
        body["model"] = Value::String(ckpt.into());
    }
    if !w.sampler.trim().is_empty() {
        body["sampler"] = Value::String(w.sampler.trim().into());
    }
    if !w.scheduler.trim().is_empty() {
        body["scheduler"] = Value::String(w.scheduler.trim().into());
    }
    if let Some(img) = source {
        body["initimage"] = Value::String(img.data_uri());
        body["initimagecreativity"] = json!(tidy(creativity));
    }
    apply_extra(&mut body, cfg)?;
    let v = send_json(auth(
        http.post(format!("{base}/API/GenerateText2Image"))
            .json(&body),
        cfg,
        creds,
    ))
    .await?;
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Err(AiError::Format(format!("SwarmUI: {}", truncate(e, 300))));
    }
    let path = v
        .pointer("/images/0")
        .and_then(Value::as_str)
        .ok_or_else(|| AiError::Format(truncate(&v.to_string(), 300)))?;
    if path.starts_with("data:") {
        return unb64(path);
    }
    let url = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{base}/{}", path.trim_start_matches('/'))
    };
    fetch_bytes(auth(http.get(url), cfg, creds)).await
}

// ---------------------------------------------------------------------------
// InvokeAI

async fn invoke_models(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
) -> Result<Vec<Value>, AiError> {
    let v = send_json(auth(
        http.get(format!("{}/api/v2/models/?model_type=main", cfg.base())),
        cfg,
        creds,
    ))
    .await?;
    Ok(v.get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
async fn invoke(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let base = cfg.base();
    let w = &cfg.image;
    let image_name = match source {
        Some(img) => {
            let (boundary, body) = multipart_file("file", "hyperstack.png", &img.mime, &img.data);
            let v = send_json(auth(
                http.post(format!(
                    "{base}/api/v1/images/upload?image_category=user&is_intermediate=false"
                ))
                .header(
                    reqwest::header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body),
                cfg,
                creds,
            ))
            .await?;
            v.get("image_name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    AiError::Format("InvokeAI did not name the uploaded picture".into())
                })?
        }
        None => String::new(),
    };
    let mut nodes = Map::new();
    let mut edges = Vec::new();
    let mut edge = |from: &str, out: &str, to: &str, input: &str| {
        edges.push(json!({
            "source": { "node_id": from, "field": out },
            "destination": { "node_id": to, "field": input },
        }));
    };
    let output_node;
    if job == ImageJob::Upscale {
        output_node = "esrgan";
        nodes.insert(
            "esrgan".into(),
            json!({
                "id": "esrgan",
                "type": "esrgan",
                "image": { "image_name": image_name },
                "model_name": upscaler(cfg, "RealESRGAN_x4plus.pth"),
                "is_intermediate": false,
                "use_cache": false,
            }),
        );
    } else {
        // The loader needs the model record, not only its name.
        let models = invoke_models(http, cfg, creds).await?;
        let wanted = checkpoint(cfg, model);
        let record = models
            .iter()
            .find(|m| {
                let name = m.get("name").and_then(Value::as_str).unwrap_or("");
                wanted.is_empty()
                    || name == wanted
                    || m.get("key").and_then(Value::as_str) == Some(wanted)
            })
            .cloned()
            .ok_or_else(|| {
                AiError::Format(if wanted.is_empty() {
                    "InvokeAI has no main models installed".into()
                } else {
                    format!("InvokeAI has no model named {wanted}")
                })
            })?;
        let base_kind = record.get("base").and_then(Value::as_str).unwrap_or("sd-1");
        let sdxl = base_kind.starts_with("sdxl");
        let ident = json!({
            "key": record.get("key").cloned().unwrap_or(Value::Null),
            "hash": record.get("hash").cloned().unwrap_or(Value::Null),
            "name": record.get("name").cloned().unwrap_or(Value::Null),
            "base": record.get("base").cloned().unwrap_or(Value::Null),
            "type": record.get("type").cloned().unwrap_or(Value::String("main".into())),
        });
        nodes.insert(
            "loader".into(),
            json!({
                "id": "loader",
                "type": if sdxl { "sdxl_model_loader" } else { "main_model_loader" },
                "model": ident,
                "is_intermediate": true,
            }),
        );
        for (id, text) in [("pos", prompt), ("neg", w.negative_prompt.as_str())] {
            let node = if sdxl {
                json!({ "id": id, "type": "sdxl_compel_prompt", "prompt": text, "style": text, "is_intermediate": true })
            } else {
                json!({ "id": id, "type": "compel", "prompt": text, "is_intermediate": true })
            };
            nodes.insert(id.into(), node);
            edge("loader", "clip", id, "clip");
            if sdxl {
                edge("loader", "clip2", id, "clip2");
            }
        }
        let seed = if w.seed < 0 {
            random_seed() as i64
        } else {
            w.seed
        };
        nodes.insert(
            "noise".into(),
            json!({
                "id": "noise",
                "type": "noise",
                "seed": seed,
                "width": size.0,
                "height": size.1,
                "use_cpu": true,
                "is_intermediate": true,
            }),
        );
        let scheduler = if w.scheduler.trim().is_empty() {
            "euler"
        } else {
            w.scheduler.trim()
        };
        // Where the sampler starts: a new picture from pure noise (0), an
        // edit from the source at the user's strength, an enhancement
        // from the source at its own small denoise.
        let denoising_start = match job {
            ImageJob::Edit => tidy(1.0 - strength),
            ImageJob::Enhance => tidy(1.0 - enhance_denoise(cfg)),
            _ => 0.0,
        };
        nodes.insert(
            "denoise".into(),
            json!({
                "id": "denoise",
                "type": "denoise_latents",
                "steps": if job == ImageJob::Enhance { enhance_steps(cfg) } else { w.steps.max(1) },
                "cfg_scale": tidy(w.cfg_scale),
                "scheduler": scheduler,
                "denoising_start": denoising_start,
                "denoising_end": 1.0,
                "is_intermediate": true,
            }),
        );
        edge("loader", "unet", "denoise", "unet");
        edge("pos", "conditioning", "denoise", "positive_conditioning");
        edge("neg", "conditioning", "denoise", "negative_conditioning");
        edge("noise", "noise", "denoise", "noise");
        if job == ImageJob::Edit || job == ImageJob::Enhance {
            nodes.insert(
                "i2l".into(),
                json!({
                    "id": "i2l",
                    "type": "i2l",
                    "image": { "image_name": image_name },
                    "fp32": false,
                    "is_intermediate": true,
                }),
            );
            edge("loader", "vae", "i2l", "vae");
            edge("i2l", "latents", "denoise", "latents");
        }
        nodes.insert(
            "l2i".into(),
            json!({
                "id": "l2i",
                "type": "l2i",
                "fp32": false,
                "is_intermediate": false,
                "use_cache": false,
            }),
        );
        edge("denoise", "latents", "l2i", "latents");
        edge("loader", "vae", "l2i", "vae");
        output_node = "l2i";
    }
    let mut body = json!({
        "prepend": false,
        "batch": {
            "graph": { "id": "hyperstack", "nodes": nodes, "edges": edges },
            "runs": 1,
        },
    });
    apply_extra(&mut body, cfg)?;
    let v = send_json(auth(
        http.post(format!("{base}/api/v1/queue/default/enqueue_batch"))
            .json(&body),
        cfg,
        creds,
    ))
    .await?;
    let item_id = v
        .pointer("/item_ids/0")
        .and_then(Value::as_i64)
        .ok_or_else(|| AiError::Format(truncate(&v.to_string(), 300)))?;
    let started = std::time::Instant::now();
    loop {
        tokio::time::sleep(POLL_EVERY).await;
        let item = send_json(auth(
            http.get(format!("{base}/api/v1/queue/default/i/{item_id}")),
            cfg,
            creds,
        ))
        .await?;
        match item.get("status").and_then(Value::as_str).unwrap_or("") {
            "completed" => {
                let results = item.pointer("/session/results").and_then(Value::as_object);
                let name = results
                    .and_then(|r| {
                        r.get(output_node)
                            .or_else(|| r.values().find(|v| v.get("image").is_some()))
                    })
                    .and_then(|r| r.pointer("/image/image_name"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| AiError::Format("InvokeAI finished without an image".into()))?;
                return fetch_bytes(auth(
                    http.get(format!("{base}/api/v1/images/i/{name}/full")),
                    cfg,
                    creds,
                ))
                .await;
            }
            "failed" | "canceled" => {
                let msg = item
                    .get("error_message")
                    .or(item.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("the graph failed");
                return Err(AiError::Format(format!("InvokeAI: {}", truncate(msg, 300))));
            }
            _ => {}
        }
        if started.elapsed() > JOB_TIMEOUT {
            return Err(AiError::Network("InvokeAI did not finish in time".into()));
        }
    }
}

// ---------------------------------------------------------------------------
// Fooocus-API

#[allow(clippy::too_many_arguments)]
async fn fooocus(
    http: &reqwest::Client,
    cfg: &AiConfig,
    creds: &Credentials,
    job: ImageJob,
    model: &str,
    prompt: &str,
    size: (u32, u32),
    source: Option<&ImagePart>,
    strength: f32,
) -> Result<Vec<u8>, AiError> {
    let base = cfg.base();
    let w = &cfg.image;
    let mut body = json!({
        "prompt": prompt,
        "negative_prompt": w.negative_prompt,
        "image_number": 1,
        "image_seed": w.seed,
        "guidance_scale": tidy(w.cfg_scale),
        "require_base64": true,
        "async_process": false,
    });
    let ckpt = checkpoint(cfg, model);
    if !ckpt.is_empty() {
        body["base_model_name"] = Value::String(ckpt.into());
    }
    let route = match job {
        ImageJob::Generate => {
            // Fooocus takes one of SDXL's preset sizes, written `W*H`.
            body["aspect_ratios_selection"] = Value::String(sdxl_size(size).replace('x', "*"));
            "v1/generation/text-to-image"
        }
        ImageJob::Edit => {
            let img = source.expect("checked");
            body["input_image"] = Value::String(b64(&img.data));
            body["uov_method"] = Value::String(
                if strength >= 0.5 {
                    "Vary (Strong)"
                } else {
                    "Vary (Subtle)"
                }
                .into(),
            );
            "v2/generation/image-upscale-vary"
        }
        ImageJob::Enhance => {
            // Fooocus has no restorer route; Vary (Subtle) is its
            // same-size detail pass, which is what an enhancement is.
            let img = source.expect("checked");
            body["input_image"] = Value::String(b64(&img.data));
            body["uov_method"] = Value::String("Vary (Subtle)".into());
            "v2/generation/image-upscale-vary"
        }
        ImageJob::Upscale => {
            let img = source.expect("checked");
            body["input_image"] = Value::String(b64(&img.data));
            // Fooocus only takes its own three ratios, so a target size
            // is answered with the nearest one it has.
            body["uov_method"] = Value::String(
                if cfg.image.upscale_ratio(size) >= 2.0 {
                    "Upscale (2x)"
                } else {
                    "Upscale (1.5x)"
                }
                .into(),
            );
            "v2/generation/image-upscale-vary"
        }
    };
    apply_extra(&mut body, cfg)?;
    let v = send_json(auth(
        http.post(format!("{base}/{route}")).json(&body),
        cfg,
        creds,
    ))
    .await?;
    let first = v.pointer("/0").unwrap_or(&v);
    if let Some(text) = first
        .get("base64")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return unb64(text);
    }
    if let Some(url) = first
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return fetch_bytes(auth(http.get(url), cfg, creds)).await;
    }
    let reason = first
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("no picture returned");
    Err(AiError::Format(format!("Fooocus: {reason}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflows_fill_into_valid_json() {
        let strings = [
            ("prompt", "a \"quoted\" cat\nline two"),
            ("negative", ""),
            ("checkpoint", "sd_xl_base_1.0.safetensors"),
            ("sampler", "euler"),
            ("scheduler", "normal"),
            ("upscaler", "RealESRGAN_x4plus.pth"),
            ("image", "in.png"),
        ];
        let numbers = [
            ("seed", 42.0),
            ("steps", 20.0),
            ("cfg", 7.5),
            ("width", 1024.0),
            ("height", 768.0),
            ("denoise", 0.6),
            ("target_width", 2048.0),
            ("target_height", 1536.0),
        ];
        for t in [COMFY_GENERATE, COMFY_EDIT, COMFY_ENHANCE] {
            let filled = fill_workflow(t, &strings, &numbers);
            let v: Value = serde_json::from_str(&filled).expect("filled workflow parses");
            assert!(!filled.contains("{{"), "placeholder left: {filled}");
            if t == COMFY_GENERATE {
                assert_eq!(v.pointer("/3/inputs/steps"), Some(&json!(20)));
                assert_eq!(v.pointer("/3/inputs/cfg"), Some(&json!(7.5)));
                assert_eq!(
                    v.pointer("/6/inputs/text").and_then(Value::as_str),
                    Some("a \"quoted\" cat\nline two")
                );
            }
        }
    }

    #[test]
    fn comfy_errors_name_the_node_and_hint_at_a_missing_clip() {
        let entry: Value = serde_json::from_str(
            r#"{"status":{"status_str":"error","messages":[
                ["execution_start",{"prompt_id":"p"}],
                ["execution_error",{"node_id":"6","node_type":"CLIPTextEncode","exception_message":"'NoneType' object has no attribute 'tokenize'","current_inputs":{"clip":[null],"text":["A city"]}}]
            ]}}"#,
        )
        .unwrap();
        let msg = comfy_error(&entry);
        assert!(
            msg.starts_with("ComfyUI: CLIPTextEncode (node 6): 'NoneType'"),
            "{msg}"
        );
        assert!(msg.contains("no text encoder"), "{msg}");
        let plain: Value =
            serde_json::from_str(r#"{"status":{"status_str":"error","messages":[]}}"#).unwrap();
        assert!(comfy_error(&plain).contains("the workflow failed"));
    }

    #[test]
    fn base64_with_or_without_prefix() {
        assert_eq!(unb64("aGk=").unwrap(), b"hi");
        assert_eq!(unb64("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert!(unb64("***").is_err());
    }

    #[test]
    fn extra_json_merges_over_the_body() {
        let mut cfg = AiConfig {
            provider: Provider::SdWebUi,
            ..Default::default()
        };
        cfg.image.extra = r#"{"restore_faces": true, "steps": 9}"#.into();
        let mut body = json!({ "steps": 20, "prompt": "x" });
        apply_extra(&mut body, &cfg).unwrap();
        assert_eq!(body["steps"], json!(9));
        assert_eq!(body["restore_faces"], json!(true));
        cfg.image.extra = "[1]".into();
        assert!(apply_extra(&mut body, &cfg).is_err());
    }

    #[test]
    fn webui_credential_forms() {
        let http = reqwest::Client::new();
        let cfg = AiConfig {
            provider: Provider::SdWebUi,
            ..Default::default()
        };
        let r = auth(http.get("http://x/"), &cfg, &Credentials::new("me:pw"))
            .build()
            .unwrap();
        assert!(
            r.headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("Basic ")
        );
        let cfg = AiConfig {
            provider: Provider::SwarmUi,
            ..Default::default()
        };
        let r = auth(http.get("http://x/"), &cfg, &Credentials::new("tok"))
            .build()
            .unwrap();
        assert_eq!(r.headers().get("cookie").unwrap(), "swarm_token=tok");
    }
}
