//! Real-HTTP tests against the release/debug backend binary.
//!
//! Ports `tests/test_server.py` (API contract over real sockets) and
//! `tests/test_webui.py` (static WebUI guards). Stdlib only plus
//! `serde_json` (already a main dependency): the binary is spawned on a
//! free port per test and exercised with hand-rolled `TcpStream` HTTP.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn binary_path() -> String {
    if let Ok(v) = std::env::var("AIF_BACKEND_BIN") {
        if !v.is_empty() {
            return v;
        }
    }
    option_env!("CARGO_BIN_EXE_aif-backend")
        .or(option_env!("CARGO_BIN_EXE_aif_backend"))
        .unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/debug/aif-backend"))
        .to_string()
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct TestServer {
    child: Child,
    port: u16,
}

impl TestServer {
    fn new(env_extra: &[(&str, &str)]) -> TestServer {
        let port = free_port();
        let mut cmd = Command::new(binary_path());
        cmd.args(["--host", "127.0.0.1", "--port", &port.to_string()]);
        cmd.env_remove("AIF_AI_BACKEND");
        cmd.env_remove("OLLAMA_API_KEY");
        for (k, v) in env_extra {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().expect("backend binary failed to spawn");
        let server = TestServer { child, port };
        server.wait_until_ready();
        server
    }

    fn wait_until_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok((status, _, _)) = http_request("GET", "/health", self.port, &[], b"") {
                if status == 200 {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("backend did not become ready in time");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type Headers = HashMap<String, String>;

/// One raw HTTP exchange. `body` is sent verbatim; pass `Some(0)` length
/// handling via `content_len` when the body is empty but headers are needed.
fn http_request(
    method: &str,
    path: &str,
    port: u16,
    extra_headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<(u16, Headers, Vec<u8>)> {
    http_request_inner(method, path, port, extra_headers, Some(body))
}

fn http_request_inner(
    method: &str,
    path: &str,
    port: u16,
    extra_headers: &[(String, String)],
    body: Option<&[u8]>,
) -> std::io::Result<(u16, Headers, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n",
        method, path
    );
    let mut has_len = false;
    for (k, v) in extra_headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_len = true;
        }
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    let body = body.unwrap_or(b"");
    if (method == "POST" || !body.is_empty()) && !has_len {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(raw.len());
    let (head_bytes, body_bytes) = raw.split_at(sep.min(raw.len()));
    let head_str = String::from_utf8_lossy(head_bytes);
    let mut lines = head_str.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut headers = Headers::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok((status, headers, body_bytes.to_vec()))
}

fn json_header() -> (String, String) {
    ("Content-Type".to_string(), "application/json".to_string())
}

fn post_json(server: &TestServer, payload: &str) -> (u16, Headers, serde_json::Value) {
    let (status, headers, body) =
        http_request("POST", "/estimate-weight", server.port, &[json_header()], payload.as_bytes())
            .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

fn get_json(server: &TestServer, path: &str) -> (u16, Headers, serde_json::Value) {
    let (status, headers, body) = http_request("GET", path, server.port, &[], b"").unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

fn get_raw(server: &TestServer, path: &str) -> (u16, Headers, Vec<u8>) {
    http_request("GET", path, server.port, &[], b"").unwrap()
}

fn png_bytes() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82".to_vec()
}

fn b64_encode(bytes: &[u8]) -> String {
    const ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut chunks = bytes.chunks_exact(3);
    for c in &mut chunks {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32;
        out.push(ALPH[(n >> 18) as usize & 63] as char);
        out.push(ALPH[(n >> 12) as usize & 63] as char);
        out.push(ALPH[(n >> 6) as usize & 63] as char);
        out.push(ALPH[n as usize & 63] as char);
    }
    match chunks.remainder() {
        [a] => {
            let n = (*a as u32) << 16;
            out.push(ALPH[(n >> 18) as usize & 63] as char);
            out.push(ALPH[(n >> 12) as usize & 63] as char);
            out.push_str("==");
        }
        [a, b] => {
            let n = ((*a as u32) << 16) | ((*b as u32) << 8);
            out.push(ALPH[(n >> 18) as usize & 63] as char);
            out.push(ALPH[(n >> 12) as usize & 63] as char);
            out.push(ALPH[(n >> 6) as usize & 63] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

fn png_b64() -> String {
    b64_encode(&png_bytes())
}

fn setup_none() -> TestServer {
    TestServer::new(&[("AIF_AI_BACKEND", "none")])
}

// --- estimate API ---

#[test]
fn estimate_weight_with_image_url() {
    let server = setup_none();
    let (status, _, body) = post_json(
        &server,
        r#"{"image_url": "https://example.com/cow.jpg", "prompt": "Estimate in kg"}"#,
    );
    assert_eq!(status, 200);
    assert!(body.get("estimated_weight_kg").is_some());
    assert_eq!(body["source"], "local_fallback");
    assert_eq!(body["prompt_used"], "Estimate in kg");
}

#[test]
fn estimate_weight_uses_default_prompt() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    assert!(body["prompt_used"].as_str().is_some_and(|p| p.contains("weight_kg")));
}

#[test]
fn response_includes_lbs() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    let kg = body["estimated_weight_kg"].as_f64().unwrap();
    let lbs = body["estimated_weight_lbs"].as_f64().unwrap();
    assert!((lbs - kg * 2.20462).abs() < 0.06);
}

#[test]
fn response_has_request_id_header_and_body() {
    let server = setup_none();
    let (status, headers, body) =
        post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    let rid = headers.get("x-request-id").expect("x-request-id header");
    assert_eq!(rid.len(), 8);
    assert_eq!(body["request_id"].as_str().unwrap(), rid);
}

#[test]
fn error_response_has_request_id() {
    let server = setup_none();
    let (status, headers, body) = post_json(&server, r#"{"prompt": "Estimate in kg"}"#);
    assert_eq!(status, 400);
    assert_eq!(
        headers.get("x-request-id").unwrap(),
        body["request_id"].as_str().unwrap()
    );
}

#[test]
fn missing_image_returns_bad_request() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, r#"{"prompt": "Estimate in kg"}"#);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "missing_image");
}

#[test]
fn missing_body_returns_bad_request() {
    let server = setup_none();
    let (status, _, body) =
        http_request("POST", "/estimate-weight", server.port, &[json_header()], b"").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, 400);
    assert_eq!(body["code"], "missing_body");
}

#[test]
fn invalid_json_returns_bad_request() {
    let server = setup_none();
    let (status, _, raw) =
        http_request("POST", "/estimate-weight", server.port, &[json_header()], b"not json")
            .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_json");
}

#[test]
fn estimation_failure_returns_bad_gateway() {
    // ollama backend with no API key -> estimation error -> 502.
    let server = TestServer::new(&[("AIF_AI_BACKEND", "ollama")]);
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 502);
    assert_eq!(body["code"], "estimation_failed");
}

#[test]
fn invalid_image_returns_bad_request() {
    // Reach image validation (ollama + key) with non-image bytes.
    let server = TestServer::new(&[
        ("AIF_AI_BACKEND", "ollama"),
        ("OLLAMA_API_KEY", "test-key"),
    ]);
    let (status, _, body) = post_json(&server, r#"{"image_base64": "QUJD"}"#);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_image");
    assert!(!serde_json::to_string(&body).unwrap().contains("test-key"));
}

#[test]
fn request_level_overrides_are_optional() {
    let server = setup_none();
    let secret = "request-only-secret";
    let payload = format!(
        r#"{{"image_base64": "{}", "prompt": "Return a short JSON estimate.", "backend": "none", "model": "runtime-model", "ollama_url": "https://example.com/api/generate", "ollama_api_key": "{}"}}"#,
        png_b64(),
        secret
    );
    let (status, _, body) = post_json(&server, &payload);
    assert_eq!(status, 200);
    assert_eq!(body["source"], "local_fallback");
    assert_eq!(body["prompt_used"], "Return a short JSON estimate.");
    assert!(!serde_json::to_string(&body).unwrap().contains(secret));
}

#[test]
fn unsupported_runtime_backend_is_rejected() {
    let server = setup_none();
    let payload = format!(r#"{{"image_base64": "{}", "backend": "unknown"}}"#, png_b64());
    let (status, _, body) = post_json(&server, &payload);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
}

#[test]
fn invalid_runtime_ollama_url_is_rejected() {
    let server = setup_none();
    let payload = format!(
        r#"{{"image_base64": "{}", "backend": "none", "ollama_url": "file:///secret"}}"#,
        png_b64()
    );
    let (status, _, body) = post_json(&server, &payload);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
}

#[test]
fn truncated_body_returns_400_json() {
    let server = setup_none();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream
        .write_all(
            b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{\"",
        )
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 400"), "got: {}", &text[..text.len().min(60)]);
    let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body: serde_json::Value = serde_json::from_slice(&raw[body_start..]).unwrap();
    assert!(body.get("request_id").is_some());
}

#[test]
fn over_limit_image_url_rejected() {
    // Serve a >20 MiB blob locally; route to the ollama path so the fetch
    // size check runs (backend=none never fetches URLs). The dummy
    // ollama_url is never contacted: validation runs first, host != ollama.com.
    let blob_len = 20 * 1024 * 1024 + 1;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let img_port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = [0u8; 4096];
        let _ = stream.read(&mut head);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            blob_len
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        let zeros = vec![0u8; 64 * 1024];
        let mut remaining = blob_len - 8;
        while remaining > 0 {
            let n = remaining.min(zeros.len());
            stream.write_all(&zeros[..n]).unwrap();
            remaining -= n;
        }
    });
    let server = setup_none();
    let payload = format!(
        r#"{{"image_base64": null, "image_url": "http://127.0.0.1:{}/big.png", "backend": "ollama", "ollama_url": "http://127.0.0.1:9/api/generate"}}"#,
        img_port
    );
    let (status, _, body) = post_json(&server, &payload);
    handle.join().ok();
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_image");
}

#[test]
fn concurrent_requests_all_succeed() {
    let server = setup_none();
    let mut handles = Vec::new();
    for i in 0..8u32 {
        let port = server.port;
        handles.push(std::thread::spawn(move || {
            let payload = format!(r#"{{"image_url": "https://example.com/cow-{}.jpg"}}"#, i);
            http_request("POST", "/estimate-weight", port, &[json_header()], payload.as_bytes())
        }));
    }
    for h in handles {
        let (status, _, raw) = h.join().unwrap().unwrap();
        assert_eq!(status, 200);
        let body: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(body["source"], "local_fallback");
    }
}

// --- info / health / static / demo ---

#[test]
fn root_serves_web_ui_html() {
    let server = setup_none();
    let (status, headers, body) = get_raw(&server, "/");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("text/html"));
    assert!(String::from_utf8_lossy(&body).contains("Cow Weight Estimator"));
}

#[test]
fn static_assets_have_content_types() {
    let server = setup_none();
    let (status, headers, css) = get_raw(&server, "/styles.css");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("text/css"));
    assert!(String::from_utf8_lossy(&css).contains("prefers-color-scheme"));

    let (status, headers, js) = get_raw(&server, "/app.js");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("javascript"));
    assert!(String::from_utf8_lossy(&js).contains("estimate-weight"));
}

#[test]
fn health_endpoint() {
    let server = setup_none();
    let (status, _, body) = get_json(&server, "/health");
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["backend"], "none");
    assert!(body.get("model").is_some());
    assert!(body.get("request_id").is_some());
}

#[test]
fn info_endpoint_remains_json_and_has_safe_defaults() {
    let server = setup_none();
    let (status, headers, body) = get_json(&server, "/info");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("application/json"));
    assert_eq!(body["name"], "Cow Weight Estimator");
    assert!(body.get("version").is_some());
    let endpoints = serde_json::to_string(&body["endpoints"]).unwrap();
    assert!(endpoints.contains("POST /estimate-weight"));
    assert!(body.get("default_prompt").is_some());
    assert!(body.get("ollama_url").is_some());
    assert!(body.get("ollama_api_key").is_none());
}

#[test]
fn info_never_exposes_api_key() {
    let secret = "super-secret-test-key";
    let server = TestServer::new(&[("AIF_AI_BACKEND", "none"), ("OLLAMA_API_KEY", secret)]);
    let (status, _, _) = get_json(&server, "/health");
    assert_eq!(status, 200);
    let (_, _, info) = get_json(&server, "/info");
    assert_eq!(info["ollama_configured"], true);
    assert!(!serde_json::to_string(&info).unwrap().contains(secret));
}

#[test]
fn unknown_get_returns_404() {
    let server = setup_none();
    let (status, _, body) = get_json(&server, "/nope");
    assert_eq!(status, 404);
    assert_eq!(body["code"], "not_found");
}

#[test]
fn path_traversal_attempt_is_not_served() {
    let server = setup_none();
    let (status, _, _) = http_request("GET", "/%2e%2e/%2e%2e/Cargo.toml", server.port, &[], b"")
        .unwrap();
    assert_eq!(status, 404);
}

#[test]
fn demo_routes_are_controlled() {
    let server = setup_none();
    let (status, _, body) = get_json(&server, "/demo-cows");
    assert_eq!(status, 200);
    assert_eq!(body["demos"].as_array().unwrap().len(), 3);
    let (status, headers, img) = get_raw(&server, "/demo-cows/1");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("image/webp"));
    assert!(img.starts_with(b"RIFF"));
}

#[test]
fn options_preflight_returns_204() {
    let server = setup_none();
    let (status, headers, _) =
        http_request("OPTIONS", "/estimate-weight", server.port, &[], b"").unwrap();
    assert_eq!(status, 204);
    assert_eq!(headers["access-control-allow-origin"], "*");
    assert!(headers["access-control-allow-methods"].contains("POST"));
}

#[test]
fn cors_header_on_success() {
    let server = setup_none();
    let (status, headers, _) =
        post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    assert_eq!(headers["access-control-allow-origin"], "*");
}

// --- static WebUI guards (ports tests/test_webui.py) ---

fn web_file(name: &str) -> String {
    let path = format!("{}/../web/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing web/{}", name))
}

#[test]
fn index_has_batch_upload_result_and_history() {
    let html = web_file("index.html");
    for needle in [
        "Cow Weight Estimator",
        r#"id="image-input""#,
        "multiple",
        r#"id="estimate-button""#,
        r#"id="status""#,
        r#"id="result-area""#,
        r#"id="history-list""#,
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn index_stays_simple_without_settings_clutter() {
    let html = web_file("index.html");
    for gone in [
        "advanced-settings",
        "api-key-input",
        "prompt-input",
        "backend-select",
        "model-input",
        "ollama-url-input",
    ] {
        assert!(!html.contains(gone), "index.html should not contain {}", gone);
    }
}

#[test]
fn js_posts_estimates_and_renders_safely() {
    let js = web_file("app.js");
    assert!(js.contains("estimate-weight"));
    assert!(js.contains("history-list"));
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
}

#[test]
fn css_stays_small_and_supports_dark_mode() {
    let css = web_file("styles.css");
    assert!(css.contains("prefers-color-scheme"));
    let lines = css.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(lines <= 200, "stylesheet grew to {} lines", lines);
}
