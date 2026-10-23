//! The self-hosted image backends against mock servers that answer with the
//! documented shapes (AUTOMATIC1111 `/sdapi/v1`, ComfyUI, SwarmUI, InvokeAI,
//! Fooocus-API). These prove what this client sends and how it reads the
//! answers; they are not the real servers, none of which is installed on
//! the build machine (docs/tools/graphics.md).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use hs_core::ai::{
    AiConfig, Credentials, ImageJob, ImagePart, Provider, UpscaleMode, generate_image, http_client,
    list_models,
};
use serde_json::{Value, json};

/// One request as the mock saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

type Handler = Arc<dyn Fn(&Seen) -> (u16, &'static str, Vec<u8>) + Send + Sync>;

/// A blocking HTTP/1.1 server on a std thread: enough for JSON, multipart
/// and binary answers, which is all the backends need.
struct Mock {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn serve(handler: Handler) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let handler = handler.clone();
            let log = log.clone();
            thread::spawn(move || handle(stream, handler, log));
        }
    });
    Mock { base, seen }
}

fn handle(mut stream: TcpStream, handler: Handler, log: Arc<Mutex<Vec<Seen>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();
        let mut headers = HashMap::new();
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        let len: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        if len > 0 {
            reader.read_exact(&mut body).unwrap();
        }
        let seen = Seen {
            method,
            path,
            headers,
            body,
        };
        let (status, ctype, out) = handler(&seen);
        log.lock().unwrap().push(seen);
        let head = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            out.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&out).unwrap();
        stream.flush().unwrap();
    }
}

fn ok_json(v: Value) -> (u16, &'static str, Vec<u8>) {
    (200, "application/json", v.to_string().into_bytes())
}

/// A tiny valid PNG (1x1), what every backend hands back.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn cfg(provider: Provider, base: &str) -> AiConfig {
    let mut c = AiConfig {
        provider,
        base_url: base.to_string(),
        ..Default::default()
    };
    c.image.steps = 12;
    c.image.cfg_scale = 6.5;
    c.image.negative_prompt = "blurry".into();
    c.image.upscale_factor = 2;
    // Enhancing and upscaling are separate jobs with separate settings:
    // a refiner pass at this strength, and a 2x super-resolution model.
    c.image.enhance_denoise = 0.3;
    c.image.enhance_face_restore = true;
    c
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

fn source() -> ImagePart {
    ImagePart::png(PNG.to_vec())
}

#[test]
fn webui_txt2img_img2img_and_upscale_carry_the_documented_fields() {
    let mock = serve(Arc::new(|r: &Seen| match r.path.as_str() {
        "/sdapi/v1/txt2img" | "/sdapi/v1/img2img" => {
            ok_json(json!({ "images": [b64(PNG)], "parameters": {}, "info": "" }))
        }
        "/sdapi/v1/extra-single-image" => ok_json(json!({ "image": b64(PNG), "html_info": "" })),
        "/sdapi/v1/sd-models" => ok_json(
            json!([{ "title": "sd_xl_base_1.0.safetensors [abc]", "model_name": "sd_xl_base_1.0" }]),
        ),
        _ => (404, "text/plain", b"nope".to_vec()),
    }));
    let cfg = cfg(Provider::SdWebUi, &mock.base);
    let creds = Credentials::new("me:pw");
    run(async {
        let http = http_client();
        let out = generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Generate,
            "sd_xl_base_1.0.safetensors",
            "a cat",
            (640, 480),
            None,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(out, PNG);
        let src = source();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "",
            "winter",
            (640, 480),
            Some(&src),
            0.7,
        )
        .await
        .unwrap();
        // Enhance: img2img at the refiner's strength, the source's own
        // size, with the face restorer on. It must not resize.
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Enhance,
            "",
            "",
            (640, 480),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        // Upscale: the Extras route, by a ratio.
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (640, 480),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        // The same job asked for as a target size instead.
        let mut sized = cfg.clone();
        sized.image.upscale_mode = UpscaleMode::Dimensions;
        sized.image.upscale_width = 1920;
        sized.image.upscale_height = 1440;
        generate_image(
            &http,
            &sized,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (640, 480),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        let models = list_models(&http, &cfg, &creds).await.unwrap();
        assert_eq!(models, vec!["sd_xl_base_1.0.safetensors [abc]"]);
    });
    let seen = mock.seen.lock().unwrap();
    let t2i = seen.iter().find(|s| s.path == "/sdapi/v1/txt2img").unwrap();
    let body = t2i.json();
    assert_eq!(body["prompt"], "a cat");
    assert_eq!(body["negative_prompt"], "blurry");
    assert_eq!(body["steps"], 12);
    assert_eq!(body["cfg_scale"], 6.5);
    assert_eq!(body["width"], 640);
    assert_eq!(body["height"], 480);
    assert_eq!(
        body["override_settings"]["sd_model_checkpoint"],
        "sd_xl_base_1.0.safetensors"
    );
    // --api-auth user:password is HTTP Basic.
    assert!(t2i.headers["authorization"].starts_with("Basic "));
    let i2i: Vec<Value> = seen
        .iter()
        .filter(|s| s.path == "/sdapi/v1/img2img")
        .map(Seen::json)
        .collect();
    assert_eq!(i2i[0]["init_images"][0], b64(PNG));
    assert_eq!(i2i[0]["denoising_strength"], 0.7);
    assert_eq!(i2i[0]["restore_faces"], false);
    // The enhancement: the refiner's own strength, the restorer on, and
    // the source's size untouched.
    assert_eq!(i2i[1]["denoising_strength"], 0.3);
    assert_eq!(i2i[1]["restore_faces"], true);
    assert_eq!(i2i[1]["width"], 640);
    assert_eq!(i2i[1]["height"], 480);
    let up: Vec<Value> = seen
        .iter()
        .filter(|s| s.path == "/sdapi/v1/extra-single-image")
        .map(Seen::json)
        .collect();
    assert_eq!(up[0]["image"], b64(PNG));
    assert_eq!(up[0]["resize_mode"], 0);
    assert_eq!(up[0]["upscaling_resize"], 2.0);
    assert_eq!(up[0]["upscaler_1"], "R-ESRGAN 4x+");
    // Asked for as a size, the Extras route takes mode 1 and the pair.
    assert_eq!(up[1]["resize_mode"], 1);
    assert_eq!(up[1]["upscaling_resize_w"], 1920);
    assert_eq!(up[1]["upscaling_resize_h"], 1440);
}

#[test]
fn comfyui_uploads_queues_polls_and_downloads() {
    let mock = serve(Arc::new(|r: &Seen| match r.path.as_str() {
        "/upload/image" => {
            ok_json(json!({ "name": "hyperstack.png", "subfolder": "", "type": "input" }))
        }
        "/prompt" => ok_json(json!({ "prompt_id": "p-1", "number": 1, "node_errors": {} })),
        "/history/p-1" => ok_json(
            json!({ "p-1": { "status": { "completed": true, "status_str": "success" }, "outputs": { "9": { "images": [{ "filename": "HyperStack_00001_.png", "subfolder": "", "type": "output" }] } } } }),
        ),
        p if p.starts_with("/view?") => (200, "image/png", PNG.to_vec()),
        "/object_info/CheckpointLoaderSimple" => ok_json(
            json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": [["v1-5-pruned.safetensors", "sdxl.safetensors"]] } } } }),
        ),
        // The V3 schema a recent ComfyUI answers with: the names live in
        // the settings object, not in the first element. Reading only the
        // old shape reported "no upscale models" against a server that had
        // one (owner's report, 2026-10-23).
        "/object_info/UpscaleModelLoader" => ok_json(json!({
            "UpscaleModelLoader": { "input": { "required": {
                "model_name": ["COMBO", { "options": ["RealESRGAN_x4plus.pth"], "tooltip": "the model" }]
            } } }
        })),
        _ => (404, "text/plain", b"nope".to_vec()),
    }));
    let cfg = cfg(Provider::ComfyUi, &mock.base);
    let creds = Credentials::default();
    run(async {
        let http = http_client();
        // No checkpoint typed: the server's first choice is used.
        let out = generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Generate,
            "",
            "a dog",
            (512, 512),
            None,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(out, PNG);
        let src = source();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "sdxl.safetensors",
            "sunset",
            (512, 512),
            Some(&src),
            0.4,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Enhance,
            "sdxl.safetensors",
            "",
            (512, 512),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (512, 512),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        let models = list_models(&http, &cfg, &creds).await.unwrap();
        assert_eq!(models, vec!["v1-5-pruned.safetensors", "sdxl.safetensors"]);
    });
    let seen = mock.seen.lock().unwrap();
    let prompts: Vec<Value> = seen
        .iter()
        .filter(|s| s.path == "/prompt")
        .map(Seen::json)
        .collect();
    assert_eq!(prompts.len(), 4);
    let made = &prompts[0]["prompt"];
    assert_eq!(made["4"]["inputs"]["ckpt_name"], "v1-5-pruned.safetensors");
    assert_eq!(made["6"]["inputs"]["text"], "a dog");
    assert_eq!(made["7"]["inputs"]["text"], "blurry");
    assert_eq!(made["3"]["inputs"]["steps"], 12);
    assert_eq!(made["3"]["inputs"]["cfg"], 6.5);
    assert_eq!(made["5"]["inputs"]["width"], 512);
    assert!(
        prompts[0]["client_id"]
            .as_str()
            .unwrap()
            .starts_with("hyperstack-")
    );
    let edit = &prompts[1]["prompt"];
    assert_eq!(edit["4"]["inputs"]["ckpt_name"], "sdxl.safetensors");
    assert_eq!(edit["10"]["inputs"]["image"], "hyperstack.png");
    assert_eq!(edit["3"]["inputs"]["denoise"], 0.4);
    // Enhancement re-samples through the checkpoint at the refiner's
    // strength: a VAEEncode of the source, no upscale model anywhere.
    let enh = &prompts[2]["prompt"];
    assert_eq!(enh["4"]["inputs"]["ckpt_name"], "sdxl.safetensors");
    assert_eq!(enh["11"]["class_type"], "VAEEncode");
    assert_eq!(enh["10"]["inputs"]["image"], "hyperstack.png");
    assert_eq!(enh["3"]["inputs"]["denoise"], 0.3);
    assert!(enh.get("12").is_none(), "an enhancement loads no upscaler");
    // Upscaling runs the super-resolution model and scales to the target.
    let up = &prompts[3]["prompt"];
    assert_eq!(up["12"]["inputs"]["model_name"], "RealESRGAN_x4plus.pth");
    assert_eq!(up["14"]["inputs"]["width"], 1024);
    assert_eq!(up["14"]["inputs"]["height"], 1024);
    // The upload is a multipart with an `image` part.
    let upload = seen.iter().find(|s| s.path == "/upload/image").unwrap();
    assert!(upload.headers["content-type"].starts_with("multipart/form-data; boundary="));
    assert!(
        String::from_utf8_lossy(&upload.body)
            .contains("name=\"image\"; filename=\"hyperstack.png\"")
    );
    let view = seen.iter().find(|s| s.path.starts_with("/view?")).unwrap();
    assert!(view.path.contains("filename=HyperStack_00001_.png"));
    assert!(view.path.contains("type=output"));
}

#[test]
fn swarmui_opens_a_session_and_reads_the_image_path() {
    let mock = serve(Arc::new(|r: &Seen| match r.path.as_str() {
        "/API/GetNewSession" => ok_json(json!({ "session_id": "sess-9", "version": "0.9.6" })),
        "/API/GenerateText2Image" => {
            ok_json(json!({ "images": ["View/local/raw/2026-10-22/out.png"] }))
        }
        "/View/local/raw/2026-10-22/out.png" => (200, "image/png", PNG.to_vec()),
        "/API/ListModels" => ok_json(
            json!({ "files": [{ "name": "OfficialStableDiffusion/sd_xl_base_1.0" }], "folders": [] }),
        ),
        _ => (404, "text/plain", b"nope".to_vec()),
    }));
    let cfg = cfg(Provider::SwarmUi, &mock.base);
    let creds = Credentials::new("tok");
    run(async {
        let http = http_client();
        let out = generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Generate,
            "sd_xl_base_1.0",
            "a bird",
            (1024, 768),
            None,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(out, PNG);
        let src = source();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "",
            "greener",
            (1024, 768),
            Some(&src),
            0.6,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Enhance,
            "",
            "",
            (300, 200),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (300, 200),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        let models = list_models(&http, &cfg, &creds).await.unwrap();
        assert_eq!(models, vec!["OfficialStableDiffusion/sd_xl_base_1.0"]);
    });
    let seen = mock.seen.lock().unwrap();
    let gens: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.path == "/API/GenerateText2Image")
        .collect();
    assert_eq!(gens.len(), 4);
    let g = gens[0].json();
    assert_eq!(g["session_id"], "sess-9");
    assert_eq!(g["images"], 1);
    assert_eq!(g["prompt"], "a bird");
    assert_eq!(g["negativeprompt"], "blurry");
    assert_eq!(g["model"], "sd_xl_base_1.0");
    assert_eq!(g["cfgscale"], 6.5);
    assert_eq!(g["width"], 1024);
    // The account token travels as the swarm_token cookie.
    assert_eq!(gens[0].headers["cookie"], "swarm_token=tok");
    let e = gens[1].json();
    assert!(
        e["initimage"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(e["initimagecreativity"], 0.6);
    // An enhancement keeps the source's size and moves it a little.
    let enh = gens[2].json();
    assert_eq!(enh["width"], 300);
    assert_eq!(enh["height"], 200);
    assert_eq!(enh["initimagecreativity"], 0.3);
    // An upscale is the one that grows the picture.
    let u = gens[3].json();
    assert_eq!(u["width"], 600);
    assert_eq!(u["height"], 400);
    assert_eq!(u["initimagecreativity"], 0.25);
}

#[test]
fn invokeai_builds_a_graph_for_the_models_base_and_polls_the_queue() {
    let mock = serve(Arc::new(|r: &Seen| {
        match (r.method.as_str(), r.path.as_str()) {
            (_, p) if p.starts_with("/api/v2/models/") => ok_json(json!({ "models": [
            { "key": "k-sd15", "hash": "blake3:aaa", "name": "sd-1.5", "base": "sd-1", "type": "main" },
            { "key": "k-xl", "hash": "blake3:bbb", "name": "juggernaut-xl", "base": "sdxl", "type": "main" }
        ] })),
            (_, p) if p.starts_with("/api/v1/images/upload") => {
                ok_json(json!({ "image_name": "up-1.png" }))
            }
            ("POST", "/api/v1/queue/default/enqueue_batch") => {
                ok_json(json!({ "batch": { "batch_id": "b1" }, "item_ids": [7] }))
            }
            ("GET", "/api/v1/queue/default/i/7") => ok_json(
                json!({ "status": "completed", "session": { "results": { "l2i": { "type": "image_output", "image": { "image_name": "out-1.png" } }, "esrgan": { "type": "image_output", "image": { "image_name": "out-1.png" } } } } }),
            ),
            ("GET", "/api/v1/images/i/out-1.png/full") => (200, "image/png", PNG.to_vec()),
            _ => (404, "text/plain", b"nope".to_vec()),
        }
    }));
    let cfg = cfg(Provider::InvokeAi, &mock.base);
    let creds = Credentials::default();
    run(async {
        let http = http_client();
        let out = generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Generate,
            "sd-1.5",
            "a fox",
            (512, 768),
            None,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(out, PNG);
        let src = source();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "juggernaut-xl",
            "night",
            (512, 768),
            Some(&src),
            0.3,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Enhance,
            "sd-1.5",
            "",
            (512, 768),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (512, 768),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        let models = list_models(&http, &cfg, &creds).await.unwrap();
        assert_eq!(models, vec!["sd-1.5", "juggernaut-xl"]);
    });
    let seen = mock.seen.lock().unwrap();
    let batches: Vec<Value> = seen
        .iter()
        .filter(|s| s.path.ends_with("/enqueue_batch"))
        .map(Seen::json)
        .collect();
    assert_eq!(batches.len(), 4);
    let g = &batches[0]["batch"]["graph"];
    assert_eq!(g["nodes"]["loader"]["type"], "main_model_loader");
    assert_eq!(g["nodes"]["loader"]["model"]["key"], "k-sd15");
    assert_eq!(g["nodes"]["loader"]["model"]["hash"], "blake3:aaa");
    assert_eq!(g["nodes"]["pos"]["type"], "compel");
    assert_eq!(g["nodes"]["pos"]["prompt"], "a fox");
    assert_eq!(g["nodes"]["noise"]["width"], 512);
    assert_eq!(g["nodes"]["denoise"]["steps"], 12);
    assert_eq!(g["nodes"]["denoise"]["denoising_start"], 0.0);
    assert_eq!(g["nodes"]["l2i"]["is_intermediate"], false);
    let edges = g["edges"].as_array().unwrap();
    assert!(edges.iter().any(|e| e["source"]["node_id"] == "loader"
        && e["source"]["field"] == "unet"
        && e["destination"]["node_id"] == "denoise"));
    assert_eq!(batches[0]["batch"]["runs"], 1);
    // SDXL picks the XL loader and prompt nodes, an edit adds i2l.
    let e = &batches[1]["batch"]["graph"];
    assert_eq!(e["nodes"]["loader"]["type"], "sdxl_model_loader");
    assert_eq!(e["nodes"]["pos"]["type"], "sdxl_compel_prompt");
    assert_eq!(e["nodes"]["i2l"]["image"]["image_name"], "up-1.png");
    assert!((e["nodes"]["denoise"]["denoising_start"].as_f64().unwrap() - 0.7).abs() < 1e-6);
    assert!(
        e["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["source"]["field"] == "clip2")
    );
    // An enhancement is the img2img graph at the refiner's strength, not
    // the upscale node.
    let enh = &batches[2]["batch"]["graph"];
    assert_eq!(enh["nodes"]["i2l"]["image"]["image_name"], "up-1.png");
    assert!((enh["nodes"]["denoise"]["denoising_start"].as_f64().unwrap() - 0.7).abs() < 1e-6);
    assert!(enh["nodes"].get("esrgan").is_none());
    let u = &batches[3]["batch"]["graph"];
    assert_eq!(u["nodes"]["esrgan"]["type"], "esrgan");
    assert_eq!(u["nodes"]["esrgan"]["model_name"], "RealESRGAN_x4plus.pth");
    let upload = seen
        .iter()
        .find(|s| s.path.starts_with("/api/v1/images/upload"))
        .unwrap();
    assert!(upload.path.contains("image_category=user"));
    assert!(String::from_utf8_lossy(&upload.body).contains("name=\"file\""));
}

#[test]
fn fooocus_uses_preset_sizes_and_upscale_vary_methods() {
    let mock = serve(Arc::new(|r: &Seen| match r.path.as_str() {
        "/v1/generation/text-to-image" | "/v2/generation/image-upscale-vary" => ok_json(
            json!([{ "base64": b64(PNG), "url": "", "seed": "1", "finish_reason": "SUCCESS" }]),
        ),
        "/v1/engines/all-models" => ok_json(
            json!({ "model_filenames": ["juggernautXL_v8.safetensors"], "lora_filenames": [] }),
        ),
        _ => (404, "text/plain", b"nope".to_vec()),
    }));
    let cfg = cfg(Provider::Fooocus, &mock.base);
    let creds = Credentials::default();
    run(async {
        let http = http_client();
        let out = generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Generate,
            "juggernautXL_v8.safetensors",
            "a ship",
            (1920, 1080),
            None,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(out, PNG);
        let src = source();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "",
            "stormy",
            (1024, 1024),
            Some(&src),
            0.8,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Edit,
            "",
            "calm",
            (1024, 1024),
            Some(&src),
            0.2,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Enhance,
            "",
            "",
            (1024, 1024),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        generate_image(
            &http,
            &cfg,
            &creds,
            ImageJob::Upscale,
            "",
            "",
            (1024, 1024),
            Some(&src),
            0.0,
        )
        .await
        .unwrap();
        let models = list_models(&http, &cfg, &creds).await.unwrap();
        assert_eq!(models, vec!["juggernautXL_v8.safetensors"]);
    });
    let seen = mock.seen.lock().unwrap();
    let t2i = seen
        .iter()
        .find(|s| s.path == "/v1/generation/text-to-image")
        .unwrap()
        .json();
    assert_eq!(t2i["prompt"], "a ship");
    assert_eq!(t2i["aspect_ratios_selection"], "1344*768");
    assert_eq!(t2i["base_model_name"], "juggernautXL_v8.safetensors");
    assert_eq!(t2i["require_base64"], true);
    assert_eq!(t2i["async_process"], false);
    let uov: Vec<Value> = seen
        .iter()
        .filter(|s| s.path == "/v2/generation/image-upscale-vary")
        .map(Seen::json)
        .collect();
    assert_eq!(uov[0]["uov_method"], "Vary (Strong)");
    assert_eq!(uov[1]["uov_method"], "Vary (Subtle)");
    // Fooocus has no restorer, so an enhancement is its subtle vary and
    // an upscale is the one that asks for more pixels.
    assert_eq!(uov[2]["uov_method"], "Vary (Subtle)");
    assert_eq!(uov[3]["uov_method"], "Upscale (2x)");
    assert_eq!(uov[0]["input_image"], b64(PNG));
}

#[test]
fn extra_json_reaches_the_body_and_a_server_error_is_reported() {
    let mock = serve(Arc::new(|r: &Seen| match r.path.as_str() {
        "/sdapi/v1/txt2img" => {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            if body["restore_faces"] == true {
                ok_json(json!({ "images": [b64(PNG)] }))
            } else {
                (
                    500,
                    "application/json",
                    json!({ "error": "ValidationError", "detail": "no faces" })
                        .to_string()
                        .into_bytes(),
                )
            }
        }
        _ => (404, "text/plain", b"nope".to_vec()),
    }));
    let mut cfg = cfg(Provider::SdWebUi, &mock.base);
    run(async {
        let http = http_client();
        let err = generate_image(
            &http,
            &cfg,
            &Credentials::default(),
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
        assert!(err.contains("500"), "{err}");
        cfg.image.extra = r#"{"restore_faces": true}"#.into();
        generate_image(
            &http,
            &cfg,
            &Credentials::default(),
            ImageJob::Generate,
            "",
            "x",
            (64, 64),
            None,
            0.5,
        )
        .await
        .unwrap();
    });
}
