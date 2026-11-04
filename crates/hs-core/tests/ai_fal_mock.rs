//! fal.ai against a mock that answers with the documented shapes (the
//! queue's submit / status / result / cancel routes, the catalogue's
//! `/models` page, the key probe). These prove what this client sends and
//! how it reads the answers; a live run with a real key is the owner's
//! check (docs/tools/graphics.md, FEAT-0099).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{PNG, Seen, ok_json, run, serve};
use hs_core::ai::{
    AiConfig, Credentials, ImageJob, ImagePart, Provider, describe_image, generate_image,
    generate_image_watched, http_client, list_models,
};
use hs_core::ai_fal::{self, FalPurpose, Watch};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn cfg(base: &str) -> AiConfig {
    let mut c = AiConfig {
        provider: Provider::Fal,
        base_url: base.to_string(),
        ..Default::default()
    };
    c.image.upscale_factor = 2;
    c.image.enhance_denoise = 0.3;
    c
}

fn key() -> Credentials {
    Credentials::new("test-key-123")
}

fn source() -> ImagePart {
    ImagePart::png(PNG.to_vec())
}

fn err(status: u16, body: Value) -> (u16, &'static str, Vec<u8>) {
    (status, "application/json", body.to_string().into_bytes())
}

/// Submit, two status polls (queued, then done), the result, the file.
/// The submit answer carries no URLs, so the client must build them from
/// the owner and app alone - never from the full endpoint id.
#[test]
fn a_picture_is_submitted_polled_fetched_and_downloaded() {
    let polls = Arc::new(AtomicUsize::new(0));
    let p = polls.clone();
    let mock = serve(Arc::new(move |r: &Seen| {
        match (r.method.as_str(), r.path.as_str()) {
            ("POST", "/fal-ai/flux/dev") => {
                ok_json(json!({ "request_id": "r1", "status": "IN_QUEUE" }))
            }
            ("GET", "/fal-ai/flux/requests/r1/status") => {
                if p.fetch_add(1, Ordering::SeqCst) == 0 {
                    (
                        202,
                        "application/json",
                        json!({ "status": "IN_QUEUE", "queue_position": 2 })
                            .to_string()
                            .into_bytes(),
                    )
                } else {
                    ok_json(json!({ "status": "COMPLETED", "logs": [] }))
                }
            }
            ("GET", "/fal-ai/flux/requests/r1") => ok_json(json!({
                "images": [{ "url": format!("http://{}/files/out.png", r.headers["host"]), "width": 1, "height": 1 }],
                "seed": 7,
            })),
            ("GET", "/files/out.png") => (200, "image/png", PNG.to_vec()),
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    let cfg = cfg(&mock.base);
    let statuses: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let heard = statuses.clone();
    let bytes = run(async {
        let http = http_client();
        generate_image_watched(
            &http,
            &cfg,
            &key(),
            ImageJob::Generate,
            "fal-ai/flux/dev",
            "a lighthouse",
            (1024, 768),
            None,
            0.5,
            Watch {
                cancel: None,
                on_status: Some(Box::new(move |m: &str| {
                    heard.lock().unwrap().push(m.to_string())
                })),
            },
        )
        .await
        .unwrap()
    });
    assert_eq!(bytes, PNG);
    let seen = mock.seen.lock().unwrap();
    let submit = seen
        .iter()
        .find(|r| r.method == "POST")
        .expect("one submit");
    assert_eq!(submit.headers["authorization"], "Key test-key-123");
    let body = submit.json();
    assert_eq!(body["prompt"], "a lighthouse");
    assert_eq!(body["image_size"]["width"], 1024);
    assert_eq!(body["aspect_ratio"], "4:3");
    assert_eq!(body["num_images"], 1);
    // Every queue call carries the key; the media download does not.
    for r in seen.iter().filter(|r| r.path.starts_with("/fal-ai/")) {
        assert_eq!(r.headers["authorization"], "Key test-key-123", "{}", r.path);
    }
    let file = seen.iter().find(|r| r.path == "/files/out.png").unwrap();
    assert!(!file.headers.contains_key("authorization"));
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    let statuses = statuses.lock().unwrap();
    assert!(
        statuses.iter().any(|s| s == "In queue, position 2"),
        "{statuses:?}"
    );
}

/// The submit answer's own URLs win when it has them (they may point at
/// another host), and an upscaler's `image.url` is read like `images`.
#[test]
fn the_answers_urls_are_followed_and_an_upscale_sends_the_ratio() {
    let base: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let b = base.clone();
    let mock = serve(Arc::new(move |r: &Seen| {
        let base = b.lock().unwrap().clone();
        match (r.method.as_str(), r.path.as_str()) {
            ("POST", "/fal-ai/clarity-upscaler") => ok_json(json!({
                "request_id": "u1",
                "status_url": format!("{base}/elsewhere/status"),
                "response_url": format!("{base}/elsewhere/result"),
                "cancel_url": format!("{base}/elsewhere/cancel"),
            })),
            ("GET", "/elsewhere/status") => ok_json(json!({ "status": "COMPLETED" })),
            ("GET", "/elsewhere/result") => ok_json(json!({
                "image": { "url": format!("{base}/files/up.png") },
            })),
            ("GET", "/files/up.png") => (200, "image/png", PNG.to_vec()),
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    *base.lock().unwrap() = mock.base.clone();
    let cfg = cfg(&mock.base);
    let src = source();
    run(async {
        let http = http_client();
        let bytes = generate_image(
            &http,
            &cfg,
            &key(),
            ImageJob::Upscale,
            "fal-ai/clarity-upscaler",
            "",
            (64, 64),
            Some(&src),
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(bytes, PNG);
        // An enhancement is the same route at a ratio of 1.
        let bytes = generate_image(
            &http,
            &cfg,
            &key(),
            ImageJob::Enhance,
            "fal-ai/clarity-upscaler",
            "",
            (64, 64),
            Some(&src),
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(bytes, PNG);
    });
    let seen = mock.seen.lock().unwrap();
    let submits: Vec<Value> = seen
        .iter()
        .filter(|r| r.method == "POST")
        .map(Seen::json)
        .collect();
    assert_eq!(submits.len(), 2);
    assert_eq!(submits[0]["upscale_factor"], 2.0);
    assert_eq!(submits[0]["scale"], 2.0);
    assert!(
        submits[0]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"),
        "the source goes in as a data URI"
    );
    assert_eq!(submits[1]["upscale_factor"], 1);
    assert_eq!(submits[1]["creativity"], 0.3);
    assert_eq!(submits[1]["resemblance"], 0.7);
    assert!(seen.iter().any(|r| r.path == "/elsewhere/status"));
    assert!(seen.iter().any(|r| r.path == "/elsewhere/result"));
}

#[test]
fn a_describing_model_answers_text() {
    let mock = serve(Arc::new(|r: &Seen| {
        match (r.method.as_str(), r.path.as_str()) {
            ("POST", "/fal-ai/florence-2-large/caption") => {
                ok_json(json!({ "request_id": "d1", "status": "IN_QUEUE" }))
            }
            ("GET", "/fal-ai/florence-2-large/requests/d1/status") => {
                ok_json(json!({ "status": "COMPLETED" }))
            }
            ("GET", "/fal-ai/florence-2-large/requests/d1") => {
                ok_json(json!({ "results": "A red car parked by a wall." }))
            }
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    let cfg = cfg(&mock.base);
    let src = source();
    let text = run(async {
        let http = http_client();
        describe_image(
            &http,
            &cfg,
            &key(),
            "fal-ai/florence-2-large/caption",
            "",
            &src,
            Watch::default(),
        )
        .await
        .unwrap()
    });
    assert_eq!(text, "A red car parked by a wall.");
    let seen = mock.seen.lock().unwrap();
    let submit = seen.iter().find(|r| r.method == "POST").unwrap().json();
    assert!(
        submit["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert!(
        submit.get("prompt").is_none(),
        "no question, no prompt field"
    );
    // The status route is the owner and app, not the full endpoint id
    // (the full id answers 405 on fal.ai).
    assert!(
        seen.iter()
            .any(|r| r.path == "/fal-ai/florence-2-large/requests/d1/status")
    );
}

/// Stop while queued: the token ends the wait and the request is
/// cancelled on the server, not only forgotten.
#[test]
fn cancelling_puts_the_cancel_route_and_stops_polling() {
    let mock = serve(Arc::new(|r: &Seen| {
        match (r.method.as_str(), r.path.as_str()) {
            ("POST", "/fal-ai/flux/dev") => ok_json(json!({ "request_id": "c1" })),
            ("GET", "/fal-ai/flux/requests/c1/status") => {
                ok_json(json!({ "status": "IN_QUEUE", "queue_position": 9 }))
            }
            ("PUT", "/fal-ai/flux/requests/c1/cancel") => ok_json(json!({ "status": "CANCELLED" })),
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    let cfg = cfg(&mock.base);
    let token = CancellationToken::new();
    let t = token.clone();
    let err = run(async {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            t.cancel();
        });
        let http = http_client();
        generate_image_watched(
            &http,
            &cfg,
            &key(),
            ImageJob::Generate,
            "fal-ai/flux/dev",
            "x",
            (64, 64),
            None,
            0.5,
            Watch {
                cancel: Some(token),
                on_status: None,
            },
        )
        .await
        .unwrap_err()
        .to_string()
    });
    assert!(err.contains("cancelled"), "{err}");
    let seen = mock.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|r| r.method == "PUT" && r.path == "/fal-ai/flux/requests/c1/cancel"),
        "the cancel route was not called: {:?}",
        seen.iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect::<Vec<_>>()
    );
    assert!(
        seen.iter().filter(|r| r.path.ends_with("/status")).count() <= 2,
        "polling went on after the cancel"
    );
}

/// The catalogue, per purpose: the category and search filters, the
/// `status=active` and `limit` parameters, the cursor followed to the
/// next page, the key sent along, and the ids sorted without repeats.
#[test]
fn the_catalogue_is_read_per_purpose_and_paged() {
    let mock = serve(Arc::new(|r: &Seen| {
        if !r.path.starts_with("/models?") {
            return (404, "text/plain", b"nope".to_vec());
        }
        let q = r.path.split_once('?').map(|x| x.1).unwrap_or("");
        let has = |kv: &str| q.split('&').any(|p| p == kv);
        if !has("status=active") || !has("limit=100") {
            return err(400, json!({ "detail": format!("bad query {q}") }));
        }
        if has("category=text-to-image") {
            if has("cursor=Mg") {
                ok_json(json!({
                    "models": [{ "endpoint_id": "fal-ai/flux/dev" }, { "endpoint_id": "fal-ai/aura" }],
                    "next_cursor": null, "has_more": false,
                }))
            } else {
                ok_json(json!({
                    "models": [
                        { "endpoint_id": "fal-ai/flux/schnell", "metadata": { "display_name": "FLUX.1 [schnell]", "category": "text-to-image" } },
                        { "endpoint_id": "fal-ai/flux/dev" },
                    ],
                    "next_cursor": "Mg", "has_more": true,
                }))
            }
        } else if has("category=image-to-image") && has("q=upscal") {
            ok_json(
                json!({ "models": [{ "endpoint_id": "fal-ai/clarity-upscaler" }], "next_cursor": null }),
            )
        } else if has("category=image-to-image") && has("q=resolution") {
            ok_json(
                json!({ "models": [{ "endpoint_id": "bria/increase-resolution" }, { "endpoint_id": "fal-ai/clarity-upscaler" }], "next_cursor": null }),
            )
        } else if has("category=vision") {
            ok_json(
                json!({ "models": [{ "endpoint_id": "fal-ai/florence-2-large/caption" }], "next_cursor": null }),
            )
        } else {
            // image-to-text, and every Enhance stem: nothing listed.
            ok_json(json!({ "models": [], "next_cursor": null }))
        }
    }));
    let cfg = cfg(&mock.base);
    run(async {
        let http = http_client();
        let generate = ai_fal::list_models(&http, &cfg, &key(), FalPurpose::Generate)
            .await
            .unwrap();
        assert_eq!(
            generate,
            vec!["fal-ai/aura", "fal-ai/flux/dev", "fal-ai/flux/schnell"]
        );
        let upscale = ai_fal::list_models(&http, &cfg, &key(), FalPurpose::Upscale)
            .await
            .unwrap();
        assert_eq!(
            upscale,
            vec!["bria/increase-resolution", "fal-ai/clarity-upscaler"]
        );
        let describe = ai_fal::list_models(&http, &cfg, &key(), FalPurpose::Describe)
            .await
            .unwrap();
        assert_eq!(describe, vec!["fal-ai/florence-2-large/caption"]);
        // `ai::list_models` for the kind is the generation catalogue.
        assert_eq!(list_models(&http, &cfg, &key()).await.unwrap(), generate);
        let enhance = ai_fal::list_models(&http, &cfg, &key(), FalPurpose::Enhance)
            .await
            .unwrap();
        assert!(enhance.is_empty());
    });
    let seen = mock.seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|r| r.headers["authorization"] == "Key test-key-123")
    );
    assert!(
        seen.iter()
            .any(|r| r.path.contains("category=text-to-image") && r.path.contains("cursor=Mg"))
    );
    // Enhance asks the image-to-image category with each of its stems.
    let stems = seen
        .iter()
        .filter(|r| r.path.contains("category=image-to-image") && r.path.contains("q=restor"))
        .count();
    assert_eq!(stems, 1);
}

/// Test Connection: a rejected key is 401 with fal's `detail`; an accepted
/// one answers 404 for the made-up request. A server error on submit is
/// read in its 422 shape.
#[test]
fn the_key_probe_and_the_error_shapes() {
    let mock = serve(Arc::new(|r: &Seen| {
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", p) if p.ends_with("/status") => {
                if r.headers.get("authorization").map(String::as_str) == Some("Key good") {
                    err(404, json!({ "status": "NOT_FOUND" }))
                } else {
                    err(401, json!({ "detail": "invalid key credentials" }))
                }
            }
            ("POST", "/fal-ai/flux/dev") => err(
                422,
                json!({ "detail": [{ "loc": ["body", "prompt"], "msg": "field required", "type": "missing" }] }),
            ),
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    let cfg = cfg(&mock.base);
    run(async {
        let http = http_client();
        let ok = ai_fal::ping(&http, &cfg, &Credentials::new("good"))
            .await
            .unwrap();
        assert!(ok.contains("accepts"), "{ok}");
        let bad = ai_fal::ping(&http, &cfg, &Credentials::new("bad"))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            bad.contains("401") && bad.contains("invalid key credentials"),
            "{bad}"
        );
        let none = ai_fal::ping(&http, &cfg, &Credentials::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(none.contains("needs an API key"), "{none}");
        let e = generate_image(
            &http,
            &cfg,
            &key(),
            ImageJob::Generate,
            "fal-ai/flux/dev",
            "x",
            (64, 64),
            None,
            0.5,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("422") && e.contains("body.prompt: field required"),
            "{e}"
        );
        // No model chosen for the purpose is said before anything is sent.
        let e = generate_image(
            &http,
            &cfg,
            &key(),
            ImageJob::Generate,
            "",
            "x",
            (64, 64),
            None,
            0.5,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(e.contains("no fal.ai model chosen"), "{e}");
    });
}
