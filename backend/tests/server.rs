//! Real-HTTP tests against the release/debug backend binary.
//!
//! The API contract is exercised over real sockets (the binary is spawned
//! on a free port per test with hand-rolled `TcpStream` HTTP), plus static
//! WebUI guards. Stdlib only plus `serde_json` (already a main dependency).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn binary_path() -> String {
    if let Ok(v) = std::env::var("AIF_BACKEND_BIN") {
        if !v.is_empty() {
            return v;
        }
    }
    option_env!("CARGO_BIN_EXE_aif-backend")
        .or(option_env!("CARGO_BIN_EXE_aif_backend"))
        .unwrap_or(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/target/debug/aif-backend"
        ))
        .to_string()
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct TestServer {
    child: Child,
    port: u16,
    env: Vec<(String, String)>,
}

impl TestServer {
    fn new(env_extra: &[(&str, &str)]) -> TestServer {
        let port = free_port();
        let env: Vec<(String, String)> = env_extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let child = spawn_server(port, &env);
        let server = TestServer { child, port, env };
        server.wait_until_ready();
        server
    }

    /// Restart the same server (same port, same environment/data dir) to
    /// prove database state survives restarts.
    fn restart(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = spawn_server(self.port, &self.env);
        self.wait_until_ready();
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

/// Spawn the backend on `port` with a clean, caller-supplied environment.
fn spawn_server(port: u16, env: &[(String, String)]) -> Child {
    let mut cmd = Command::new(binary_path());
    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string()]);
    for key in [
        "AIF_AI_BACKEND",
        "AIF_AI_MODEL",
        "AIF_OLLAMA_URL",
        "AIF_CACHE_TTL",
        "OLLAMA_API_KEY",
        "AIF_REQUIRE_AUTH",
        "AIF_PRODUCTION",
        "AIF_DATA_DIR",
        "AIF_PUBLIC_ORIGIN",
        "AIF_DAILY_LIMIT",
        "AIF_OPERATOR_EMAIL",
        "AIF_INFERENCE_PAUSED",
        "AIF_COOKIE_SECURE",
        "AIF_INVITE_DAYS",
        "AIF_SESSION_DAYS",
        "AIF_TRUSTED_PROXIES",
        "AIF_MAX_INFERENCE",
    ] {
        cmd.env_remove(key);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().expect("backend binary failed to spawn")
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique throwaway data dir for one test's server (parallel-safe).
fn unique_data_dir(prefix: &str) -> String {
    let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("aif-it-{}-{}-{}", std::process::id(), prefix, n));
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_str().unwrap().to_string()
}

/// Auth-mode server: sessions required, isolated database.
fn setup_auth(extra: &[(&str, &str)], prefix: &str) -> (TestServer, String) {
    let data_dir = unique_data_dir(prefix);
    let mut env: Vec<(&str, &str)> = vec![
        ("AIF_AI_BACKEND", "none"),
        ("AIF_REQUIRE_AUTH", "1"),
        ("AIF_DATA_DIR", &data_dir),
    ];
    env.extend_from_slice(extra);
    (TestServer::new(&env), data_dir)
}

/// Run an operator CLI command against `data_dir`; returns (exit, stdout).
fn admin_cli(data_dir: &str, extra: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new(binary_path());
    cmd.args(args);
    for key in [
        "AIF_AI_BACKEND",
        "AIF_AI_MODEL",
        "AIF_OLLAMA_URL",
        "AIF_CACHE_TTL",
        "OLLAMA_API_KEY",
        "AIF_REQUIRE_AUTH",
        "AIF_PRODUCTION",
        "AIF_PUBLIC_ORIGIN",
        "AIF_DAILY_LIMIT",
        "AIF_OPERATOR_EMAIL",
        "AIF_INFERENCE_PAUSED",
        "AIF_COOKIE_SECURE",
        "AIF_INVITE_DAYS",
        "AIF_SESSION_DAYS",
        "AIF_TRUSTED_PROXIES",
        "AIF_MAX_INFERENCE",
    ] {
        cmd.env_remove(key);
    }
    cmd.env("AIF_DATA_DIR", data_dir);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let out = cmd.output().expect("admin CLI failed to run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

/// Mint an invite through the operator CLI and return its raw token.
fn cli_invite(data_dir: &str, email: &str, role: &str) -> String {
    let (code, stdout) = admin_cli(data_dir, &[], &["--create-invite", email, "--role", role]);
    assert_eq!(code, 0, "create-invite failed: {}", stdout);
    token_from_stdout(&stdout)
}

/// Extract the `#invite=` / `#recovery=` token from CLI printed URLs.
fn token_from_stdout(stdout: &str) -> String {
    for line in stdout.lines().rev() {
        if let Some(i) = line.find("#invite=") {
            return line[i + "#invite=".len()..].trim().to_string();
        }
        if let Some(i) = line.find("#recovery=") {
            return line[i + "#recovery=".len()..].trim().to_string();
        }
    }
    panic!("no token URL in CLI output: {}", stdout);
}

struct Session {
    cookie: String,
    csrf: String,
    user_id: String,
    email: String,
}

/// Accept an invite over HTTP; returns the logged-in session.
fn accept_invite(
    server: &TestServer,
    token: &str,
    password: &str,
    name: &str,
) -> (u16, Headers, serde_json::Value, Option<Session>) {
    let payload = format!(
        r#"{{"token": "{}", "password": "{}", "display_name": "{}"}}"#,
        token, password, name
    );
    let (status, headers, raw) = http_request(
        "POST",
        "/api/auth/accept-invite",
        server.port,
        &[json_header()],
        payload.as_bytes(),
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&raw).unwrap_or(serde_json::Value::Null);
    let session = (status == 200)
        .then(|| session_from(&headers, &body))
        .flatten();
    (status, headers, body, session)
}

fn session_from(headers: &Headers, body: &serde_json::Value) -> Option<Session> {
    let set_cookie = headers.get("set-cookie")?;
    let first = set_cookie.split(';').next()?.trim();
    let token = first.strip_prefix("aif_session=")?;
    if token.is_empty() {
        return None;
    }
    Some(Session {
        cookie: format!("aif_session={}", token),
        csrf: body["user"]["csrf_token"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        user_id: body["user"]["id"].as_str().unwrap_or("").to_string(),
        email: body["user"]["email"].as_str().unwrap_or("").to_string(),
    })
}

/// Log in over HTTP; panics unless login succeeds.
fn login(server: &TestServer, email: &str, password: &str) -> Session {
    let payload = format!(r#"{{"email": "{}", "password": "{}"}}"#, email, password);
    let (status, headers, raw) = http_request(
        "POST",
        "/api/auth/login",
        server.port,
        &[json_header()],
        payload.as_bytes(),
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&raw).unwrap_or(serde_json::Value::Null);
    assert_eq!(status, 200, "login failed: {}", body);
    session_from(&headers, &body).expect("login set no session cookie")
}

/// Authenticated JSON request (CSRF header included on writes).
fn authed(
    server: &TestServer,
    method: &str,
    path: &str,
    session: Option<&Session>,
    payload: Option<&str>,
) -> (u16, Headers, serde_json::Value) {
    let mut headers = vec![json_header()];
    if let Some(s) = session {
        headers.push(("Cookie".to_string(), s.cookie.clone()));
        if method != "GET" && method != "OPTIONS" && !s.csrf.is_empty() {
            headers.push(("X-CSRF-Token".to_string(), s.csrf.clone()));
        }
    }
    let body = payload.unwrap_or("").as_bytes();
    let (status, headers, raw) = http_request(method, path, server.port, &headers, body).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&raw).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

fn authed_raw(
    server: &TestServer,
    method: &str,
    path: &str,
    session: Option<&Session>,
    payload: Option<&str>,
) -> (u16, Headers, Vec<u8>) {
    let mut headers = vec![json_header()];
    if let Some(s) = session {
        headers.push(("Cookie".to_string(), s.cookie.clone()));
        if method != "GET" && method != "OPTIONS" && !s.csrf.is_empty() {
            headers.push(("X-CSRF-Token".to_string(), s.csrf.clone()));
        }
    }
    let body = payload.unwrap_or("").as_bytes();
    http_request(method, path, server.port, &headers, body).unwrap()
}

type Headers = HashMap<String, String>;

/// One raw HTTP exchange. `body` is sent verbatim with an explicit
/// `Content-Length` unless the caller already supplied one.
fn http_request(
    method: &str,
    path: &str,
    port: u16,
    extra_headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<(u16, Headers, Vec<u8>)> {
    // Under parallel load a spawned server can stall past the read timeout;
    // retry the whole exchange on timeouts rather than failing the test.
    let mut last_err = None;
    for _ in 0..3 {
        match http_request_once(method, path, port, extra_headers, body) {
            Ok(ok) => return Ok(ok),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap())
}

fn http_request_once(
    method: &str,
    path: &str,
    port: u16,
    extra_headers: &[(String, String)],
    body: &[u8],
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
    let (status, headers, body) = http_request(
        "POST",
        "/estimate-weight",
        server.port,
        &[json_header()],
        payload.as_bytes(),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

fn post_json_to(
    server: &TestServer,
    path: &str,
    payload: &str,
) -> (u16, Headers, serde_json::Value) {
    let (status, headers, body) = http_request(
        "POST",
        path,
        server.port,
        &[json_header()],
        payload.as_bytes(),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

// --- batch estimates + animal profiles ---

#[test]
fn estimate_batch_returns_per_item_results() {
    let server = setup_none();
    let payload = format!(
        r#"{{"items": [{{"image_base64": "{}"}}, {{"image_base64": "{}"}}]}}"#,
        png_b64(),
        png_b64()
    );
    let (status, headers, body) = post_json_to(&server, "/estimate-batch", &payload);
    assert_eq!(status, 200);
    assert!(headers.contains_key("x-request-id"));
    let parent = body["request_id"].as_str().unwrap().to_string();
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for (index, entry) in results.iter().enumerate() {
        assert_eq!(entry["status"], 200);
        assert_eq!(entry["body"]["source"], "local_fallback");
        assert_eq!(
            entry["body"]["request_id"].as_str().unwrap(),
            format!("{}-{}", parent, index)
        );
    }
}

#[test]
fn estimate_batch_isolates_item_failures() {
    // The `none` backend hashes without image validation, so the failing
    // items use shape errors (missing image, non-object) instead.
    let server = setup_none();
    let payload = format!(
        r#"{{"items": [{{"prompt": "no image here"}}, {{"image_base64": "{}"}}, 42]}}"#,
        png_b64()
    );
    let (status, _, body) = post_json_to(&server, "/estimate-batch", &payload);
    assert_eq!(status, 200);
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["status"], 400);
    assert_eq!(results[0]["body"]["code"], "missing_image");
    assert_eq!(results[1]["status"], 200);
    assert_eq!(results[1]["body"]["source"], "local_fallback");
    assert_eq!(results[2]["status"], 400);
    assert_eq!(results[2]["body"]["code"], "invalid_options");
}

#[test]
fn estimate_batch_rejects_bad_shapes() {
    let server = setup_none();
    for payload in [r#"{}"#.to_string(), r#"{"items": []}"#.to_string()] {
        let (status, _, body) = post_json_to(&server, "/estimate-batch", &payload);
        assert_eq!(status, 400);
        assert_eq!(body["code"], "invalid_options");
    }
    let many: Vec<String> = (0..21)
        .map(|_| format!(r#"{{"image_base64": "{}"}}"#, png_b64()))
        .collect();
    let payload = format!(r#"{{"items": [{}]}}"#, many.join(","));
    let (status, _, body) = post_json_to(&server, "/estimate-batch", &payload);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
}

#[test]
fn animal_profile_echoed_and_sharpens_prompt() {
    let server = setup_none();
    let (status, _, body) = post_json(
        &server,
        &format!(
            r#"{{"image_base64": "{}", "animal_breed": "Angus", "animal_sex": "cow", "animal_age_years": 4.5}}"#,
            png_b64()
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(body["animal_breed"], "Angus");
    assert_eq!(body["animal_sex"], "cow");
    assert_eq!(body["animal_age_years"], 4.5);
    assert!(body["prompt_used"]
        .as_str()
        .is_some_and(|p| p.contains("breed Angus")));
}

#[test]
fn animal_profile_rejects_bad_hints() {
    let server = setup_none();
    for payload in [
        format!(
            r#"{{"image_base64": "{}", "animal_sex": "dinosaur"}}"#,
            png_b64()
        ),
        format!(
            r#"{{"image_base64": "{}", "animal_age_years": 99}}"#,
            png_b64()
        ),
        format!(
            r#"{{"image_base64": "{}", "animal_breed": "!!!"}}"#,
            png_b64()
        ),
    ] {
        let (status, _, body) = post_json(&server, &payload);
        assert_eq!(status, 400);
        assert_eq!(body["code"], "invalid_options");
    }
}

#[test]
fn animal_profile_omitted_keeps_old_shape() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    assert!(body.get("animal_breed").is_none());
    assert!(body.get("animal_sex").is_none());
    assert!(body.get("animal_age_years").is_none());
}

#[test]
fn tape_only_echoes_animal_profile() {
    let server = setup_none();
    let (status, _, body) = post_json(
        &server,
        r#"{"heart_girth_cm": 180, "body_length_cm": 150, "animal_breed": "Hereford"}"#,
    );
    assert_eq!(status, 200);
    assert_eq!(body["source"], "tape_measure");
    assert_eq!(body["animal_breed"], "Hereford");
}

#[test]
fn info_lists_batch_endpoint() {
    let server = setup_none();
    let (status, _, body) = get_json(&server, "/info");
    assert_eq!(status, 200);
    let endpoints = serde_json::to_string(&body["endpoints"]).unwrap();
    assert!(endpoints.contains("POST /estimate-batch"));
}

#[test]
fn index_has_profile_and_history_upgrades() {
    let html = web_file("index.html");
    for needle in [
        r#"id="breed-input""#,
        r#"id="sex-select""#,
        r#"id="age-input""#,
        r#"id="profile-status""#,
        r#"id="history-export""#,
        r#"id="history-chart""#,
        "cross-check",
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn js_has_downscale_profile_and_history_upgrades() {
    let js = web_file("app.js");
    for needle in [
        "createImageBitmap",
        "1600",
        "animal_breed",
        "animal_sex",
        "animal_age_years",
        "history-export",
        "history-chart",
        "text/csv",
        "cross-check",
        "disagree by over 20%",
    ] {
        assert!(js.contains(needle), "app.js missing {}", needle);
    }
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
}

fn get_json(server: &TestServer, path: &str) -> (u16, Headers, serde_json::Value) {
    let (status, headers, body) = http_request("GET", path, server.port, &[], b"").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
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
    assert!(body["prompt_used"]
        .as_str()
        .is_some_and(|p| p.contains("weight_kg")));
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
    let (status, _, body) = http_request(
        "POST",
        "/estimate-weight",
        server.port,
        &[json_header()],
        b"",
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, 400);
    assert_eq!(body["code"], "missing_body");
}

#[test]
fn invalid_json_returns_bad_request() {
    let server = setup_none();
    let (status, _, raw) = http_request(
        "POST",
        "/estimate-weight",
        server.port,
        &[json_header()],
        b"not json",
    )
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
    let server = TestServer::new(&[("AIF_AI_BACKEND", "ollama"), ("OLLAMA_API_KEY", "test-key")]);
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
    let payload = format!(
        r#"{{"image_base64": "{}", "backend": "unknown"}}"#,
        png_b64()
    );
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
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .write_all(
            b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{\"",
        )
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "got: {}",
        &text[..text.len().min(60)]
    );
    let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body: serde_json::Value = serde_json::from_slice(&raw[body_start..]).unwrap();
    assert!(body.get("request_id").is_some());
}

fn raw_post_status_and_body(port: u16, head: &[u8]) -> (String, serde_json::Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(head).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body: serde_json::Value = serde_json::from_slice(&raw[body_start..]).unwrap();
    (text, body)
}

#[test]
fn chunked_transfer_encoding_is_rejected() {
    let server = setup_none();
    let (text, body) = raw_post_status_and_body(
        server.port,
        b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    );
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "got: {}",
        &text[..text.len().min(60)]
    );
    assert_eq!(body["code"], "bad_request");
    assert!(body.get("request_id").is_some());
}

#[test]
fn invalid_content_length_is_rejected() {
    let server = setup_none();
    let (text, body) = raw_post_status_and_body(
        server.port,
        b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: abc\r\nConnection: close\r\n\r\n{}",
    );
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "got: {}",
        &text[..text.len().min(60)]
    );
    assert_eq!(body["code"], "bad_request");
}

#[test]
fn duplicate_content_length_is_rejected() {
    let server = setup_none();
    let (text, body) = raw_post_status_and_body(
        server.port,
        b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "got: {}",
        &text[..text.len().min(60)]
    );
    assert_eq!(body["code"], "bad_request");
}

#[test]
fn oversize_body_is_rejected_before_reading() {
    // A declared length over the 20 MiB cap is refused from the headers
    // alone, so no 20 MB payload needs to be sent.
    let server = setup_none();
    let (text, body) = raw_post_status_and_body(
        server.port,
        b"POST /estimate-weight HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 20971521\r\nConnection: close\r\n\r\n",
    );
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "got: {}",
        &text[..text.len().min(60)]
    );
    assert_eq!(body["code"], "invalid_json");
}

#[test]
fn server_api_key_is_forwarded_to_configured_host() {
    // A fake Ollama endpoint records the Authorization header it sees. The
    // server is configured (via environment) with both the fake URL and a
    // key, so the key must be forwarded to that configured host.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let fake_port = listener.local_addr().unwrap().port();
    let seen_auth = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let seen_auth_server = std::sync::Arc::clone(&seen_auth);
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap() == 0 {
                break;
            }
            head.push(byte[0]);
            if head.len() > 65536 {
                break;
            }
        }
        let head_text = String::from_utf8_lossy(&head).to_string();
        let mut auth = None;
        let mut content_length = 0usize;
        for line in head_text.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("authorization") {
                    auth = Some(value.trim().to_string());
                }
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
        }
        let mut body = vec![0u8; content_length];
        stream.read_exact(&mut body).ok();
        *seen_auth_server.lock().unwrap() = auth;
        let reply = r#"{"response": "{\"weight_kg\": 600, \"confidence\": 0.8}"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            reply.len(),
            reply
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    let server_secret = "server-side-secret-key";
    let server = TestServer::new(&[
        ("AIF_AI_BACKEND", "ollama"),
        ("OLLAMA_API_KEY", server_secret),
        (
            "AIF_OLLAMA_URL",
            &format!("http://127.0.0.1:{}/api/generate", fake_port),
        ),
    ]);
    let payload = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    let (status, _, body) = post_json(&server, &payload);
    handle.join().ok();
    assert_eq!(status, 200);
    assert_eq!(body["estimated_weight_kg"], 600.0);
    let auth = seen_auth.lock().unwrap().clone();
    assert_eq!(auth.as_deref(), Some("Bearer server-side-secret-key"));
}

#[test]
fn private_runtime_ollama_url_is_rejected() {
    // Per-request `ollama_url` overrides refuse private/local targets with
    // 400 invalid_options (same SSRF policy as `image_url`). Point the
    // server itself at a local Ollama via configuration instead.
    let server = setup_none();
    for host in [
        "127.0.0.1:11434",
        "10.0.0.5",
        "192.168.1.20",
        "169.254.169.254",
        "localhost:11434",
    ] {
        let payload = format!(
            r#"{{"image_base64": "{}", "backend": "ollama", "ollama_url": "http://{}/api/generate"}}"#,
            png_b64(),
            host
        );
        let (status, _, body) = post_json(&server, &payload);
        assert_eq!(status, 400, "expected block for {}", host);
        assert_eq!(body["code"], "invalid_options");
    }
}

/// Scripted fake Ollama endpoint: serves exactly `script.len()` connections
/// with the scripted `(status, body)` pairs in order, counting inbound hits.
/// Exits on exhaustion or a 20 s deadline, so a client that over/under-calls
/// fails the test loudly instead of hanging it.
fn fake_ollama(
    script: Vec<(u16, String)>,
    hits: Arc<Mutex<usize>>,
) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut served = 0usize;
        while served < script.len() && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match stream.read(&mut byte) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => head.push(byte[0]),
                        }
                        if head.len() > 65536 {
                            break;
                        }
                    }
                    let head_text = String::from_utf8_lossy(&head).to_string();
                    let mut content_length = 0usize;
                    for line in head_text.lines().skip(1) {
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                content_length = value.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                    let mut body = vec![0u8; content_length];
                    stream.read_exact(&mut body).ok();
                    *hits.lock().unwrap() += 1;
                    let (status, reply) = script[served].clone();
                    served += 1;
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status, reason, reply.len(), reply
                    );
                    if stream.write_all(response.as_bytes()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    (port, handle)
}

fn ollama_success_reply() -> String {
    r#"{"response": "{\"weight_kg\": 600, \"confidence\": 0.8}"}"#.to_string()
}

fn estimate_via_fake(server: &TestServer, extra: &str) -> (u16, serde_json::Value) {
    // The server under test points at the fake via its configured
    // `AIF_OLLAMA_URL` (per-request private overrides are blocked).
    let payload = format!(r#"{{"image_base64": "{}"{}}}"#, png_b64(), extra);
    let (status, _, body) = post_json(server, &payload);
    (status, body)
}

fn ollama_test_server(fake_port: u16) -> TestServer {
    TestServer::new(&[
        ("AIF_AI_BACKEND", "ollama"),
        (
            "AIF_OLLAMA_URL",
            &format!("http://127.0.0.1:{}/api/generate", fake_port),
        ),
    ])
}

#[test]
fn ollama_retries_transient_500_once() {
    let hits = Arc::new(Mutex::new(0usize));
    let (port, handle) = fake_ollama(
        vec![
            (500, r#"{"error": "busy"}"#.to_string()),
            (200, ollama_success_reply()),
        ],
        Arc::clone(&hits),
    );
    let server = ollama_test_server(port);
    let (status, body) = estimate_via_fake(&server, "");
    handle.join().ok();
    assert_eq!(status, 200);
    assert_eq!(body["estimated_weight_kg"], 600.0);
    assert_eq!(*hits.lock().unwrap(), 2);
}

#[test]
fn ollama_does_not_retry_client_errors() {
    let hits = Arc::new(Mutex::new(0usize));
    let (port, handle) = fake_ollama(
        vec![(400, r#"{"error": "bad key"}"#.to_string())],
        Arc::clone(&hits),
    );
    let server = ollama_test_server(port);
    let (status, body) = estimate_via_fake(&server, "");
    handle.join().ok();
    assert_eq!(status, 502);
    assert_eq!(body["code"], "estimation_failed");
    assert_eq!(*hits.lock().unwrap(), 1);
}

#[test]
fn ollama_cache_isolates_request_configuration() {
    let hits = Arc::new(Mutex::new(0usize));
    let (port, handle) = fake_ollama(
        vec![(200, ollama_success_reply()), (200, ollama_success_reply())],
        Arc::clone(&hits),
    );
    let server = ollama_test_server(port);
    // Same triple twice: second call is a cache hit, no new upstream call.
    let (first, _) = estimate_via_fake(&server, "");
    assert_eq!(first, 200);
    let (second, _) = estimate_via_fake(&server, "");
    assert_eq!(second, 200);
    // Different prompt: different cache key, upstream is called again.
    let (third, _) = estimate_via_fake(&server, r#", "prompt": "other prompt""#);
    assert_eq!(third, 200);
    handle.join().ok();
    assert_eq!(*hits.lock().unwrap(), 2);
}

#[test]
fn over_limit_image_url_rejected() {
    // Loopback image URLs are refused by the SSRF guard before any fetch,
    // so the fake origin below may never see a connection: accept with a
    // deadline instead of blocking forever. Either path (SSRF block or
    // size check) surfaces as 400 invalid_image. The byte-limit logic
    // itself is covered by `validate::image` unit tests.
    let blob_len = 20 * 1024 * 1024 + 1;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let img_port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
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
                        if stream.write_all(&zeros[..n]).is_err() {
                            break;
                        }
                        remaining -= n;
                    }
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    });
    let server = setup_none();
    let payload = format!(
        r#"{{"image_base64": null, "image_url": "http://127.0.0.1:{}/big.png", "backend": "ollama", "ollama_url": "https://example.com/api/generate"}}"#,
        img_port
    );
    let (status, _, body) = post_json(&server, &payload);
    handle.join().ok();
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_image");
}

#[test]
fn private_image_url_is_blocked_before_fetch() {
    // SSRF guard: loopback / private / metadata hosts are refused with
    // 400 invalid_image and never fetched (no fake origin needed).
    let server = setup_none();
    for host in [
        "127.0.0.1:9",
        "10.0.0.5",
        "192.168.1.20",
        "169.254.169.254",
        "localhost:9",
    ] {
        let payload = format!(
            r#"{{"image_url": "http://{}/cow.jpg", "backend": "ollama", "ollama_url": "https://example.com/api/generate"}}"#,
            host
        );
        let (status, _, body) = post_json(&server, &payload);
        assert_eq!(status, 400, "expected block for {}", host);
        assert_eq!(body["code"], "invalid_image");
        let text = serde_json::to_string(&body).unwrap();
        assert!(text.contains("blocked"), "got: {}", text);
    }
}

#[test]
fn metrics_endpoint_reports_counters() {
    let server = setup_none();
    let (status, headers, body) = get_json(&server, "/metrics");
    assert_eq!(status, 200);
    assert_eq!(headers["access-control-allow-origin"], "*");
    assert!(body.get("request_id").is_some());
    assert!(body.get("version").is_some());
    assert_eq!(body["backend"], "none");
    for field in [
        "uptime_secs",
        "total_requests",
        "active_connections",
        "rejected_connections",
        "cache_entries",
    ] {
        assert!(
            body.get(field).is_some(),
            "metrics missing {}: {}",
            field,
            body
        );
    }
    assert!(!serde_json::to_string(&body)
        .unwrap()
        .contains("OLLAMA_API_KEY"));
}

#[test]
fn health_reports_uptime_and_connections() {
    let server = setup_none();
    let (status, _, body) = get_json(&server, "/health");
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert!(body.get("uptime_secs").is_some());
    assert!(body.get("active_connections").is_some());
}

#[test]
fn concurrent_requests_all_succeed() {
    let server = setup_none();
    let mut handles = Vec::new();
    for i in 0..8u32 {
        let port = server.port;
        handles.push(std::thread::spawn(move || {
            let payload = format!(r#"{{"image_url": "https://example.com/cow-{}.jpg"}}"#, i);
            http_request(
                "POST",
                "/estimate-weight",
                port,
                &[json_header()],
                payload.as_bytes(),
            )
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
    let (status, _, _) =
        http_request("GET", "/%2e%2e/%2e%2e/Cargo.toml", server.port, &[], b"").unwrap();
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

// --- static WebUI guards (served-asset contract: ids, strings, headers) ---

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
        assert!(
            !html.contains(gone),
            "index.html should not contain {}",
            gone
        );
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

#[test]
fn index_has_demo_picker_capture_and_disclaimer() {
    let html = web_file("index.html");
    for needle in [
        r#"id="demo-select""#,
        r#"id="demo-button""#,
        r#"capture="environment""#,
        "not a scale or vet advice",
        "local_fallback",
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn js_demo_picker_uses_image_url_and_matches_20mb_limit() {
    let js = web_file("app.js");
    assert!(js.contains("demo-select"));
    assert!(js.contains("/demo-cows"));
    assert!(js.contains("image_url"));
    assert!(js.contains("20 * 1024 * 1024"));
    assert!(!js.contains("15 * 1024 * 1024"));
}

#[test]
fn css_has_touch_targets_and_print_rules() {
    let css = web_file("styles.css");
    assert!(css.contains("min-height: 44px"));
    assert!(css.contains("@media print"));
}

#[test]
fn accuracy_trust_helpers_render_extras_and_warnings() {
    let html = web_file("index.html");
    for needle in [
        r#"id="unit-toggle""#,
        r#"id="photo-heading""#,
        "Good photo tips",
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
    let js = web_file("app.js");
    for needle in [
        "unit-toggle",
        "confidence",
        "body_condition_score",
        "Outside the typical",
        "Low confidence",
        "Offline placeholder",
    ] {
        assert!(js.contains(needle), "app.js missing {}", needle);
    }
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
}

#[test]
fn index_has_health_badge_and_batch_controls() {
    let html = web_file("index.html");
    for needle in [
        r#"id="health-pill""#,
        r#"id="backend-label""#,
        r#"id="file-list""#,
        r#"id="batch-list""#,
        r#"id="progress""#,
        r#"role="progressbar""#,
        r#"id="cancel-button""#,
        r#"id="limits-hint""#,
        r#"id="demo-preview""#,
        r#"id="demo-retry""#,
        r#"id="demo-status""#,
        r#"id="history-clear""#,
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn js_has_batch_health_and_history_controls() {
    let js = web_file("app.js");
    for needle in [
        "health-pill",
        "backend-label",
        "file-list",
        "batch-list",
        "cancel-button",
        "history-clear",
        "demo-preview",
        "demo-retry",
        "requestId",
        "aria-valuenow",
        "revokeObjectURL",
    ] {
        assert!(js.contains(needle), "app.js missing {}", needle);
    }
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
}

#[test]
fn static_assets_send_cache_headers() {
    let server = setup_none();
    let (status, headers, _) = get_raw(&server, "/styles.css");
    assert_eq!(status, 200);
    assert!(headers
        .get("cache-control")
        .is_some_and(|v| v.contains("immutable")));
    assert!(headers.contains_key("etag"));
    let (status, headers, _) = get_raw(&server, "/demo-cows/1");
    assert_eq!(status, 200);
    assert!(headers
        .get("cache-control")
        .is_some_and(|v| v.contains("max-age")));
    assert!(headers.contains_key("etag"));
}

#[test]
fn fallback_schema_has_nullable_extras() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    assert_eq!(body["source"], "local_fallback");
    assert!(body.get("model").is_some());
    assert!(body.get("confidence").is_some());
    assert!(body.get("breed").is_some());
    assert!(body.get("body_condition_score").is_some());
}

#[test]
fn photo_estimates_include_range_and_disclaimer() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, &format!(r#"{{"image_base64": "{}"}}"#, png_b64()));
    assert_eq!(status, 200);
    let kg = body["estimated_weight_kg"].as_f64().unwrap();
    let min = body["weight_min_kg"].as_f64().unwrap();
    let max = body["weight_max_kg"].as_f64().unwrap();
    assert!(min < kg && kg < max);
    assert!((max - min - kg * 0.2).abs() < 0.2);
    assert!(body["disclaimer"].as_str().unwrap().contains("Do not dose"));
}

#[test]
fn tape_only_estimate_needs_no_image() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#);
    assert_eq!(status, 200);
    assert_eq!(body["source"], "tape_measure");
    assert_eq!(body["method"], "schaeffer_tape");
    assert_eq!(body["estimated_weight_kg"], 448.4);
    assert_eq!(body["heart_girth_cm"], 180.0);
    assert!(body["disclaimer"].as_str().unwrap().contains("Do not dose"));
    let min = body["weight_min_kg"].as_f64().unwrap();
    let max = body["weight_max_kg"].as_f64().unwrap();
    assert!((min - 426.0).abs() < 0.2 && (max - 470.8).abs() < 0.2);
}

#[test]
fn tape_partial_and_out_of_range_are_invalid_options() {
    let server = setup_none();
    let (status, _, body) = post_json(&server, r#"{"heart_girth_cm": 180}"#);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
    let (status, _, body) = post_json(&server, r#"{"heart_girth_cm": 20, "body_length_cm": 150}"#);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
    let (status, _, body) = post_json(
        &server,
        r#"{"heart_girth_cm": "big", "body_length_cm": 150}"#,
    );
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
}

#[test]
fn photo_plus_tape_returns_cross_check() {
    let server = setup_none();
    let (status, _, body) = post_json(
        &server,
        &format!(
            r#"{{"image_base64": "{}", "heart_girth_cm": 180, "body_length_cm": 150}}"#,
            png_b64()
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(body["source"], "local_fallback");
    assert_eq!(body["tape_weight_kg"], 448.4);
    assert_eq!(body["heart_girth_cm"], 180.0);
}

#[test]
fn index_has_tape_inputs_and_dosing_warning() {
    let html = web_file("index.html");
    for needle in [
        r#"id="tape-girth""#,
        r#"id="tape-length""#,
        r#"id="tape-button""#,
        r#"id="tape-status""#,
        "Tape measure",
        "Do not dose",
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn js_renders_range_tape_and_disclaimer_safely() {
    let js = web_file("app.js");
    for needle in [
        "tape-button",
        "heart_girth_cm",
        "body_length_cm",
        "weight_min_kg",
        "tape_weight_kg",
        "Do not dose",
        "range ",
    ] {
        assert!(js.contains(needle), "app.js missing {}", needle);
    }
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
}

#[test]
fn index_has_clear_button_and_drop_hint() {
    let html = web_file("index.html");
    for needle in [r#"id="clear-button""#, "drag and drop", "paste"] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

#[test]
fn js_uses_batch_endpoint_with_retry_and_file_helpers() {
    let js = web_file("app.js");
    for needle in [
        "estimate-batch",
        "estimate-weight",
        "server_busy",
        "batch-retry",
        "retryOne",
        "DataTransfer",
        "clear-button",
        "setInterval",
        "online",
        "offline",
        "drop",
        "paste",
        "BATCH_CHUNK_BYTES",
    ] {
        assert!(js.contains(needle), "app.js missing {}", needle);
    }
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
}

#[test]
fn batch_cancel_explains_active_inference_can_finish() {
    let js = web_file("account.js");
    assert!(js.contains(
        "Cancelling removes pending photos. Active inference may finish and its result can still be saved."
    ));
}

#[test]
fn css_has_clear_and_retry_styles() {
    let css = web_file("styles.css");
    assert!(css.contains("#clear-button"));
    assert!(css.contains(".batch-retry"));
}

// --- invite-only auth, isolation, durable history (two-user service) ---

#[test]
fn auth_me_reports_mode_flags() {
    let (server, _dir) = setup_auth(&[], "meflags");
    let (_, _, body) = authed(&server, "GET", "/api/me", None, None);
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_required"], true);
    let open = setup_none();
    let (_, _, body) = authed(&open, "GET", "/api/me", None, None);
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_required"], false);
}

#[test]
fn full_invite_login_logout_journey() {
    let (server, dir) = setup_auth(&[], "journey");
    let token = cli_invite(&dir, "op@example.com", "operator");
    let (status, headers, body, session) =
        accept_invite(&server, &token, "correct horse battery staple", "Op");
    assert_eq!(status, 200);
    assert_eq!(body["user"]["email"], "op@example.com");
    assert_eq!(body["user"]["role"], "operator");
    assert!(headers
        .get("set-cookie")
        .is_some_and(|v| v.contains("HttpOnly")));
    let session = session.expect("accept-invite must log in");
    // Session works.
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["user"]["email"], "op@example.com");
    assert_eq!(me["user"]["id"], session.user_id);
    // Logout revokes it.
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/auth/logout",
        Some(&session),
        Some("{}"),
    );
    assert_eq!(status, 200);
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["authenticated"], false);
    // Login failures are generic (no enumeration oracle).
    let payload = r#"{"email": "nobody@example.com", "password": "wrong password here"}"#;
    let (status, _, bad_email) = post_json_to(&server, "/api/auth/login", payload);
    assert_eq!(status, 401);
    assert_eq!(bad_email["code"], "invalid_credentials");
    let payload = r#"{"email": "op@example.com", "password": "wrong password here"}"#;
    let (status, _, bad_pw) = post_json_to(&server, "/api/auth/login", payload);
    assert_eq!(status, 401);
    assert_eq!(bad_pw["code"], "invalid_credentials");
    assert_eq!(bad_email["error"], bad_pw["error"]);
    // Correct login works again.
    let again = login(&server, "op@example.com", "correct horse battery staple");
    assert_eq!(again.email, "op@example.com");
}

#[test]
fn invite_reuse_fails_safely() {
    let (server, dir) = setup_auth(&[], "reuse");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (status, _, _, _) = accept_invite(&server, &token, "long enough password", "A");
    assert_eq!(status, 200);
    let (status, _, body, _) = accept_invite(&server, &token, "another long password", "A2");
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invite_used");
}

#[test]
fn invite_revocation_blocks_acceptance() {
    let (server, dir) = setup_auth(&[], "revoke");
    let op_token = cli_invite(&dir, "op@example.com", "operator");
    let (_, _, _, op) = accept_invite(&server, &op_token, "operator password 1", "Op");
    assert!(op.is_some());
    let op = op.unwrap();
    // Operator mints an invite over the API.
    let (status, _, created) = authed(
        &server,
        "POST",
        "/api/operator/invites",
        Some(&op),
        Some(r#"{"email": "b@example.com", "role": "user"}"#),
    );
    assert_eq!(status, 200);
    let invite_id = created["id"].as_str().unwrap().to_string();
    assert!(created["invite_url"].as_str().unwrap().contains("#invite="));
    assert!(!serde_json::to_string(&created)
        .unwrap()
        .contains("token_hash"));
    // Revoke it; the raw token (unknown to us) is now useless — but the
    // revoked status is visible, and reuse of a *different* minted invite
    // proves the list endpoint works.
    let (status, _, listed) = authed(&server, "GET", "/api/operator/invites", Some(&op), None);
    assert_eq!(status, 200);
    assert!(listed["invites"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == invite_id));
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/operator/invites/{}/revoke", invite_id),
        Some(&op),
        Some("{}"),
    );
    assert_eq!(status, 200);
    let (status, _, listed) = authed(&server, "GET", "/api/operator/invites", Some(&op), None);
    assert_eq!(status, 200);
    let entry = listed["invites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == invite_id)
        .unwrap();
    assert_eq!(entry["status"], "revoked");
    // A revoked invite id cannot be re-revoked into success.
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/operator/invites/{}/revoke", invite_id),
        Some(&op),
        Some("{}"),
    );
    assert_eq!(status, 404);
}

#[test]
fn third_account_is_refused() {
    let (server, dir) = setup_auth(&[], "third");
    for (email, role) in [("a@example.com", "operator"), ("b@example.com", "user")] {
        let token = cli_invite(&dir, email, role);
        let (status, _, _, _) = accept_invite(&server, &token, "long enough password", "N");
        assert_eq!(status, 200);
    }
    let token = cli_invite(&dir, "c@example.com", "user");
    let (status, _, body, _) = accept_invite(&server, &token, "long enough password", "C");
    assert_eq!(status, 403);
    assert_eq!(body["code"], "user_limit");
}

#[test]
fn recovery_flow_resets_password_and_revokes_sessions() {
    let (server, dir) = setup_auth(&[], "recovery");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "original password 1", "A");
    let session = session.unwrap();
    // Unknown emails get the same generic success (no enumeration).
    let (status, _, _) = post_json_to(
        &server,
        "/api/auth/recovery/request",
        r#"{"email": "ghost@example.com"}"#,
    );
    assert_eq!(status, 200);
    // Real account: request is generic too; operator mints the link via CLI.
    let (status, _, _) = post_json_to(
        &server,
        "/api/auth/recovery/request",
        r#"{"email": "a@example.com"}"#,
    );
    assert_eq!(status, 200);
    let (code, stdout) = admin_cli(&dir, &[], &["--create-recovery", "a@example.com"]);
    assert_eq!(code, 0, "{}", stdout);
    let reset = token_from_stdout(&stdout);
    // Bad token fails safely.
    let (status, _, body) = post_json_to(
        &server,
        "/api/auth/recovery/complete",
        r#"{"token": "deadbeef", "new_password": "brand new password 1"}"#,
    );
    assert_eq!(status, 400);
    assert_eq!(body["code"], "recovery_invalid");
    // Good token works once.
    let payload = format!(
        r#"{{"token": "{}", "new_password": "brand new password 1"}}"#,
        reset
    );
    let (status, _, _) = post_json_to(&server, "/api/auth/recovery/complete", &payload);
    assert_eq!(status, 200);
    let (status, _, body) = post_json_to(&server, "/api/auth/recovery/complete", &payload);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "recovery_invalid");
    // Old session died with recovery; new password logs in.
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["authenticated"], false);
    let fresh = login(&server, "a@example.com", "brand new password 1");
    assert_eq!(fresh.email, "a@example.com");
}

#[test]
fn csrf_is_rejected_without_token() {
    let (server, dir) = setup_auth(&[], "csrf");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    // State-changing call with the cookie but no CSRF header fails.
    let (status, _, body) = http_request(
        "POST",
        "/api/auth/profile",
        server.port,
        &[
            json_header(),
            ("Cookie".to_string(), session.cookie.clone()),
        ],
        br#"{"display_name": "X"}"#,
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, 403);
    assert_eq!(body["code"], "csrf_invalid");
    // With the token it succeeds.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/api/auth/profile",
        Some(&session),
        Some(r#"{"display_name": "Alice"}"#),
    );
    assert_eq!(status, 200, "{}", body);
    assert_eq!(body["display_name"], "Alice");
}

#[test]
fn auth_enforcement_blocks_anonymous_estimation_and_metrics() {
    let (server, _dir) = setup_auth(&[], "enforce");
    let (status, _, body) = post_json(&server, r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#);
    assert_eq!(status, 401);
    assert_eq!(body["code"], "unauthorized");
    let (status, _, _) = http_request(
        "POST",
        "/estimate-batch",
        server.port,
        &[json_header()],
        br#"{"items": []}"#,
    )
    .unwrap();
    assert_eq!(status, 401);
    let (status, _, body) = authed(&server, "GET", "/metrics", None, None);
    assert_eq!(status, 401);
    assert_eq!(body["code"], "unauthorized");
}

#[test]
fn estimate_saved_and_history_persists_with_pagination() {
    let (server, dir) = setup_auth(&[], "history");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    // Tape estimates save with version stamps and no placeholder flag.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    assert_eq!(body["saved"], true);
    let history_id = body["history_id"].as_str().unwrap().to_string();
    // Anonymous estimates are never saved (open-mode server).
    let open = setup_none();
    let (status, _, anon) = post_json(&open, r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#);
    assert_eq!(status, 200);
    assert!(anon.get("history_id").is_none());
    // Two more rows for pagination.
    for _ in 0..2 {
        let (status, _, _) = authed(
            &server,
            "POST",
            "/estimate-weight",
            Some(&session),
            Some(r#"{"heart_girth_cm": 170, "body_length_cm": 140}"#),
        );
        assert_eq!(status, 200);
    }
    let (status, _, page1) = authed(
        &server,
        "GET",
        "/api/history?page=1&per_page=2",
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(page1["total"], 3);
    assert_eq!(page1["items"].as_array().unwrap().len(), 2);
    let item = &page1["items"][0];
    assert!(item.get("estimator_version").is_some());
    assert!(item.get("prompt_version").is_some());
    assert_eq!(item["placeholder"], false);
    let (status, _, page2) = authed(
        &server,
        "GET",
        "/api/history?page=2&per_page=2",
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(page2["items"].as_array().unwrap().len(), 1);
    // Detail + deletion.
    let (status, _, detail) = authed(
        &server,
        "GET",
        &format!("/api/history/{}", history_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(detail["weight_kg"], 448.4);
    let (status, _, _) = authed(
        &server,
        "DELETE",
        &format!("/api/history/{}", history_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/history/{}", history_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 404);
}

#[test]
fn two_users_are_fully_isolated() {
    let (server, dir) = setup_auth(&[], "isolate");
    let token_a = cli_invite(&dir, "a@example.com", "operator");
    let (_, _, _, session_a) = accept_invite(&server, &token_a, "password for alice", "Alice");
    let token_b = cli_invite(&dir, "b@example.com", "user");
    let (_, _, _, session_b) = accept_invite(&server, &token_b, "password for bobby", "Bobby");
    let (a, b) = (session_a.unwrap(), session_b.unwrap());
    // A creates an animal and a linked estimate.
    let (status, _, animal) = authed(
        &server,
        "POST",
        "/api/animals",
        Some(&a),
        Some(r#"{"name": "Bessie"}"#),
    );
    assert_eq!(status, 200);
    let animal_id = animal["id"].as_str().unwrap().to_string();
    let payload = format!(
        r#"{{"heart_girth_cm": 180, "body_length_cm": 150, "animal_id": "{}"}}"#,
        animal_id
    );
    let (status, _, est) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&a),
        Some(&payload),
    );
    assert_eq!(status, 200);
    let est_id = est["history_id"].as_str().unwrap().to_string();
    // B sees none of it: detail, delete, animal, and filtered lists.
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/history/{}", est_id),
        Some(&b),
        None,
    );
    assert_eq!(status, 404);
    let (status, _, _) = authed(
        &server,
        "DELETE",
        &format!("/api/history/{}", est_id),
        Some(&b),
        None,
    );
    assert_eq!(status, 404);
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/animals/{}", animal_id),
        Some(&b),
        None,
    );
    assert_eq!(status, 404);
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&b), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 0);
    let (status, _, animals) = authed(&server, "GET", "/api/animals", Some(&b), None);
    assert_eq!(status, 200);
    assert_eq!(animals["animals"].as_array().unwrap().len(), 0);
    // B cannot link to A's animal (identifier swap fails closed).
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&b),
        Some(&payload),
    );
    assert_eq!(status, 404);
    assert_eq!(body["code"], "not_found");
    // B cannot export A's rows: CSV has a header and no data lines.
    let (status, headers, raw) = authed_raw(
        &server,
        "GET",
        "/api/history/export?format=csv",
        Some(&b),
        None,
    );
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("text/csv"));
    let text = String::from_utf8_lossy(&raw);
    assert_eq!(text.lines().count(), 1);
    // Ordinary users cannot touch operator routes.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/api/operator/invites",
        Some(&b),
        Some(r#"{"email": "x@example.com"}"#),
    );
    assert_eq!(status, 403);
    assert_eq!(body["code"], "forbidden");
    let (status, _, _) = authed(&server, "GET", "/api/operator/usage", Some(&b), None);
    assert_eq!(status, 403);
}

#[test]
fn quota_counts_photo_estimates_not_tape() {
    let (server, dir) = setup_auth(&[("AIF_DAILY_LIMIT", "2")], "quota");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let photo = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    for _ in 0..2 {
        let (status, _, _) = authed(
            &server,
            "POST",
            "/estimate-weight",
            Some(&session),
            Some(&photo),
        );
        assert_eq!(status, 200);
    }
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo),
    );
    assert_eq!(status, 429);
    assert_eq!(body["code"], "quota_exceeded");
    // Tape-only estimates stay free under quota exhaustion.
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
}

#[test]
fn idempotent_replay_returns_stored_result_without_duplicates() {
    let (server, dir) = setup_auth(&[], "idem");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let payload = r#"{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "tape-1"}"#;
    let (status, headers, first) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(payload),
    );
    assert_eq!(status, 200);
    assert!(!headers.contains_key("x-idempotent-replayed"));
    let history_id = first["history_id"].as_str().unwrap().to_string();
    // Replay with a different request body shape but the same key.
    let (status, headers, second) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 170, "body_length_cm": 140, "idempotency_key": "tape-1"}"#),
    );
    assert_eq!(status, 200);
    assert_eq!(second["replayed"], true);
    assert_eq!(second["history_id"], history_id);
    assert_eq!(second["estimated_weight_kg"], first["estimated_weight_kg"]);
    assert_eq!(
        headers.get("x-idempotent-replayed").map(|s| s.as_str()),
        Some("true")
    );
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 1);
}

#[test]
fn inference_pause_blocks_provider_but_not_tape() {
    // The ollama backend without a key: unpaused photo fails 502 (needs a
    // key), paused photo fails 503 without touching the provider.
    let data_dir = unique_data_dir("pause-ollama");
    let ollama = TestServer::new(&[
        ("AIF_AI_BACKEND", "ollama"),
        ("AIF_REQUIRE_AUTH", "1"),
        ("AIF_DATA_DIR", &data_dir),
    ]);
    let token = cli_invite(&data_dir, "op@example.com", "operator");
    let (_, _, _, op) = accept_invite(&ollama, &token, "operator password 1", "Op");
    let op = op.unwrap();
    let photo = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    let (status, _, _) = authed(&ollama, "POST", "/estimate-weight", Some(&op), Some(&photo));
    assert_eq!(status, 502);
    let (status, _, _) = authed(
        &ollama,
        "POST",
        "/api/operator/pause",
        Some(&op),
        Some(r#"{"paused": true}"#),
    );
    assert_eq!(status, 200);
    let (status, _, body) = authed(&ollama, "POST", "/estimate-weight", Some(&op), Some(&photo));
    assert_eq!(status, 503);
    assert_eq!(body["code"], "inference_paused");
    let (status, _, _) = authed(
        &ollama,
        "POST",
        "/estimate-weight",
        Some(&op),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    let (status, _, _) = authed(
        &ollama,
        "POST",
        "/api/operator/pause",
        Some(&op),
        Some(r#"{"paused": false}"#),
    );
    assert_eq!(status, 200);
}

#[test]
fn history_survives_server_restart_with_live_sessions() {
    let (mut server, dir) = setup_auth(&[], "restart");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    server.restart();
    // The session cookie still works after restart (durable sessions).
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["authenticated"], true);
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["weight_kg"], 448.4);
}

#[test]
fn production_rejects_overrides_and_hides_internals() {
    let data_dir = unique_data_dir("prod");
    let server = TestServer::new(&[
        ("AIF_AI_BACKEND", "none"),
        ("AIF_REQUIRE_AUTH", "1"),
        ("AIF_PRODUCTION", "1"),
        ("AIF_PUBLIC_ORIGIN", "https://cows.example.com"),
        ("AIF_DATA_DIR", &data_dir),
    ]);
    let token = cli_invite(&data_dir, "op@example.com", "operator");
    let (_, _, _, op) = accept_invite(&server, &token, "operator password 1", "Op");
    let op = op.unwrap();
    // Provider overrides are rejected even for the operator in production.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&op),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150, "model": "evil"}"#),
    );
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
    // Info/health are reduced; metrics need the operator.
    let (status, headers, info) = authed(&server, "GET", "/info", None, None);
    assert_eq!(status, 200);
    assert!(info.get("model").is_none());
    assert!(info.get("backend").is_none());
    assert!(info.get("default_prompt").is_none());
    assert!(headers.contains_key("strict-transport-security"));
    assert!(!headers.contains_key("access-control-allow-origin"));
    let (status, _, health) = authed(&server, "GET", "/health", None, None);
    assert_eq!(status, 200);
    assert_eq!(health["status"], "ok");
    assert!(health.get("model").is_none());
    let (status, _, _) = authed(&server, "GET", "/metrics", Some(&op), None);
    assert_eq!(status, 200);
}

#[test]
fn static_account_assets_are_served_and_unknown_routes_404() {
    let server = setup_none();
    let (status, headers, _) = get_raw(&server, "/account.js");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("javascript"));
    let (status, headers, _) = get_raw(&server, "/account.css");
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("text/css"));
    let (status, _, _) = get_raw(&server, "/evil.js");
    assert_eq!(status, 404);
    let (status, _, body) = authed(&server, "GET", "/api/nope", None, None);
    assert_eq!(status, 404);
    assert_eq!(body["code"], "not_found");
}

#[test]
fn application_pages_are_explicit_and_detail_ids_are_validated() {
    let server = setup_none();
    for path in [
        "/login",
        "/invite",
        "/recover",
        "/reset-password",
        "/dashboard",
        "/estimate/photo",
        "/estimate/batch",
        "/estimate/tape",
        "/uploads",
        "/history",
        "/animals",
        "/photos",
        "/settings/profile",
        "/settings/security",
        "/settings/privacy",
        "/operator",
        "/operator/invites",
        "/operator/users",
        "/operator/activity",
        "/help",
        "/privacy",
        "/animals/animal-123",
        "/history/history_123",
        "/uploads/job-123",
    ] {
        let (status, headers, body) = get_raw(&server, path);
        assert_eq!(status, 200, "route {path}");
        assert!(headers["content-type"].contains("text/html"));
        assert!(String::from_utf8_lossy(&body).contains("page-content"));
    }
    for path in ["/router.js", "/pages.css"] {
        let (status, _, _) = get_raw(&server, path);
        assert_eq!(status, 200, "asset {path}");
    }
    for path in ["/animals/%2e%2e%2fsecret", "/unlisted/page"] {
        let (status, _, _) = get_raw(&server, path);
        assert_eq!(status, 404, "unknown route {path}");
    }
}

#[test]
fn upload_batches_keep_partial_jobs_private_and_clear_terminal_payloads() {
    let (server, dir) = setup_auth(&[], "upload-batches");
    let token1 = cli_invite(&dir, "a@example.com", "operator");
    let (_, _, _, first) = accept_invite(&server, &token1, "long enough password", "A");
    let first = first.unwrap();
    let token2 = cli_invite(&dir, "b@example.com", "user");
    let (_, _, _, second) = accept_invite(&server, &token2, "another long password", "B");
    let second = second.unwrap();

    let (status, _, _) = authed(&server, "GET", "/api/upload-batches", None, None);
    assert_eq!(status, 401);
    let (status, _, batch) = authed(
        &server,
        "POST",
        "/api/upload-batches",
        Some(&first),
        Some(r#"{"item_count":2}"#),
    );
    assert_eq!(status, 201, "{batch}");
    let batch_id = batch["id"].as_str().unwrap();
    let job_body = format!(
        r#"{{"heart_girth_cm":180,"body_length_cm":150,"idempotency_key":"batch-item-key","batch_id":"{}","batch_index":0,"filename":"cow-one.jpg"}}"#,
        batch_id
    );
    let (status, _, job) = authed(&server, "POST", "/api/jobs", Some(&first), Some(&job_body));
    assert_eq!(status, 202, "{job}");
    assert_eq!(job["filename"], "cow-one.jpg");
    assert_eq!(job["batch_index"], 0);
    let (status, _, replay) = authed(&server, "POST", "/api/jobs", Some(&first), Some(&job_body));
    assert_eq!(status, 202);
    assert_eq!(replay["replayed"], true);
    let path = format!("/api/upload-batches/{batch_id}");
    let (status, _, _) = authed(&server, "GET", &path, Some(&second), None);
    assert_eq!(status, 404);
    let cancel_path = format!("{path}/cancel");
    let (status, _, cancelled) = authed(&server, "POST", &cancel_path, Some(&first), Some("{}"));
    assert_eq!(status, 200, "{cancelled}");
    assert_eq!(cancelled["cancelled_count"], 1);
    assert_eq!(cancelled["items"][0]["status"], "cancelled");
    let job_id = job["id"].as_str().unwrap();
    let job_path = format!("/api/jobs/{job_id}");
    let (status, _, after_cancel) = authed(&server, "GET", &job_path, Some(&first), None);
    assert_eq!(status, 200);
    assert_eq!(after_cancel["status"], "cancelled");
    let retry_path = format!("{path}/items/0/retry");
    let retry_body =
        r#"{"heart_girth_cm":180,"body_length_cm":150,"idempotency_key":"batch-item-key"}"#;
    let (status, _, _) = authed(
        &server,
        "POST",
        &retry_path,
        Some(&second),
        Some(retry_body),
    );
    assert_eq!(status, 404, "another user cannot retry this batch item");
    let (status, _, wrong_key) = authed(
        &server,
        "POST",
        &retry_path,
        Some(&first),
        Some(r#"{"heart_girth_cm":180,"body_length_cm":150,"idempotency_key":"different-key"}"#),
    );
    assert_eq!(
        status, 400,
        "retry must reuse its original idempotency key: {wrong_key}"
    );
    let (status, _, retried) = authed(&server, "POST", &retry_path, Some(&first), Some(retry_body));
    assert_eq!(status, 202, "retry should be accepted: {retried}");
    assert_eq!(
        retried["submitted_count"], 1,
        "retry reuses the original item row"
    );
    assert_ne!(retried["items"][0]["status"], "cancelled");
    let (status, _, jobs) = authed(&server, "GET", "/api/jobs", Some(&first), None);
    assert_eq!(status, 200);
    assert_eq!(
        jobs["jobs"].as_array().unwrap().len(),
        1,
        "retry must not duplicate saved jobs"
    );
}

#[test]
fn account_security_headers_and_deletion() {
    let (server, dir) = setup_auth(&[], "headers");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, headers, _) = authed_raw(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    assert_eq!(
        headers.get("x-frame-options").map(|s| s.as_str()),
        Some("DENY")
    );
    assert!(headers
        .get("content-security-policy")
        .is_some_and(|v| v.contains("default-src 'self'")));
    assert!(headers.contains_key("x-request-id"));
    // Wrong password cannot delete; right password wipes everything.
    let (status, _, _) = authed(
        &server,
        "DELETE",
        "/api/account",
        Some(&session),
        Some(r#"{"password": "wrong password here"}"#),
    );
    assert_eq!(status, 401);
    let (status, headers, _) = authed(
        &server,
        "DELETE",
        "/api/account",
        Some(&session),
        Some(r#"{"password": "long enough password"}"#),
    );
    assert_eq!(status, 200);
    assert!(headers
        .get("set-cookie")
        .is_some_and(|v| v.contains("Max-Age=0")));
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["authenticated"], false);
    // The freed slot admits a new account.
    let token2 = cli_invite(&dir, "fresh@example.com", "user");
    let (status, _, _, _) = accept_invite(&server, &token2, "fresh password 123", "Fresh");
    assert_eq!(status, 200);
}

#[test]
fn password_change_needs_reauth_and_logs_out() {
    let (server, dir) = setup_auth(&[], "pwchange");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "original password 1", "A");
    let session = session.unwrap();
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/auth/change-password",
        Some(&session),
        Some(
            r#"{"current_password": "wrong password here", "new_password": "brand new password 1"}"#,
        ),
    );
    assert_eq!(status, 401);
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/auth/change-password",
        Some(&session),
        Some(
            r#"{"current_password": "original password 1", "new_password": "brand new password 1"}"#,
        ),
    );
    assert_eq!(status, 200);
    // All sessions (including this one) are revoked: fresh login required.
    let (status, _, me) = authed(&server, "GET", "/api/me", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(me["authenticated"], false);
    let fresh = login(&server, "a@example.com", "brand new password 1");
    assert_eq!(fresh.email, "a@example.com");
}

#[test]
fn csv_export_is_shaped_and_numeric() {
    let (server, dir) = setup_auth(&[], "csv");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    let (status, headers, raw) = authed_raw(
        &server,
        "GET",
        "/api/history/export?format=csv",
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("text/csv"));
    assert!(headers
        .get("content-disposition")
        .is_some_and(|v| v.contains("attachment")));
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    let header = lines.next().unwrap();
    assert!(
        header.contains("weight_kg")
            && header.contains("measured_at")
            && header.contains("placeholder")
    );
    let row = lines.next().unwrap();
    // Numeric measurement columns stay unquoted numbers.
    assert!(row.contains(",448.4,"), "row: {}", row);
    assert!(row.contains("tape_measure"));
}

#[test]
fn login_rate_limit_blocks_brute_force() {
    let (server, _dir) = setup_auth(&[], "ratelimit");
    // 10 failures are plain 401s; the 11th trips the per-IP limiter.
    for i in 0..11 {
        let payload = format!(
            r#"{{"email": "ghost{}@example.com", "password": "wrong password here"}}"#,
            i
        );
        let (status, _, _) = post_json_to(&server, "/api/auth/login", &payload);
        if i < 10 {
            assert_eq!(status, 401, "attempt {}", i);
        } else {
            assert_eq!(status, 429, "attempt {}", i);
        }
    }
}

#[test]
fn forwarded_ip_is_honored_only_from_trusted_proxy() {
    let (server, _dir) = setup_auth(&[], "fwd");
    // 127.0.0.1 is a trusted proxy by default, so a forged client IP gets
    // its own rate-limit bucket: exhaust it, then prove the real bucket
    // (no header) is untouched.
    for i in 0..11 {
        let payload = format!(
            r#"{{"email": "forged{}@example.com", "password": "wrong password here"}}"#,
            i
        );
        let (status, _, _) = http_request(
            "POST",
            "/api/auth/login",
            server.port,
            &[
                json_header(),
                ("X-Forwarded-For".to_string(), "9.9.9.9".to_string()),
            ],
            payload.as_bytes(),
        )
        .unwrap();
        if i < 10 {
            assert_eq!(status, 401, "attempt {}", i);
        } else {
            assert_eq!(status, 429, "attempt {}", i);
        }
    }
    let (status, _, _) = post_json_to(
        &server,
        "/api/auth/login",
        r#"{"email": "direct@example.com", "password": "wrong password here"}"#,
    );
    assert_eq!(status, 401);
}

#[test]
fn animal_scale_flow_keeps_trends_separate() {
    let (server, dir) = setup_auth(&[], "animals");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    // Validation first.
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/animals",
        Some(&session),
        Some(r#"{"name": ""}"#),
    );
    assert_eq!(status, 400);
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/animals",
        Some(&session),
        Some(r#"{"name": "Bessie", "sex": "dinosaur"}"#),
    );
    assert_eq!(status, 400);
    let (status, _, animal) = authed(
        &server,
        "POST",
        "/api/animals",
        Some(&session),
        Some(r#"{"name": "Bessie", "breed": "Angus", "sex": "cow"}"#),
    );
    assert_eq!(status, 200);
    let animal_id = animal["id"].as_str().unwrap().to_string();
    // Editing works (PUT carries a JSON body).
    let (status, _, updated) = authed(
        &server,
        "PUT",
        &format!("/api/animals/{}", animal_id),
        Some(&session),
        Some(r#"{"name": "Bessie II", "breed": "Angus", "sex": "cow"}"#),
    );
    assert_eq!(status, 200);
    assert_eq!(updated["name"], "Bessie II");
    // Bad scale readings are rejected.
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/animals/{}/measurements", animal_id),
        Some(&session),
        Some(r#"{"scale_weight_kg": 5}"#),
    );
    assert_eq!(status, 400);
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/animals/{}/measurements", animal_id),
        Some(&session),
        Some(r#"{"scale_weight_kg": 500, "measured_at": "2999-01-01T00:00:00Z"}"#),
    );
    assert_eq!(status, 400);
    // A real scale reading lands on the animal with an exact range.
    let (status, _, scale) = authed(
        &server,
        "POST",
        &format!("/api/animals/{}/measurements", animal_id),
        Some(&session),
        Some(r#"{"scale_weight_kg": 512.5, "measured_at": "2026-09-01T08:00:00Z"}"#),
    );
    assert_eq!(status, 200);
    assert_eq!(scale["source"], "scale");
    assert_eq!(scale["weight_min_kg"], scale["weight_max_kg"]);
    // A photo estimate linked to the same animal joins its trend.
    let payload = format!(
        r#"{{"heart_girth_cm": 180, "body_length_cm": 150, "animal_id": "{}"}}"#,
        animal_id
    );
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&payload),
    );
    assert_eq!(status, 200);
    let (status, _, detail) = authed(
        &server,
        "GET",
        &format!("/api/animals/{}", animal_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(detail["estimates_total"], 2);
    let sources: Vec<String> = detail["estimates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["source"].as_str().unwrap().to_string())
        .collect();
    assert!(sources.contains(&"scale".to_string()));
    // Deleting the animal keeps the estimates, unlinked.
    let (status, _, _) = authed(
        &server,
        "DELETE",
        &format!("/api/animals/{}", animal_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 2);
    assert!(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["animal_id"].is_null()));
}

#[test]
fn batch_items_save_individually_and_count_quota() {
    let (server, dir) = setup_auth(&[("AIF_DAILY_LIMIT", "3")], "batchq");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let payload = format!(
        r#"{{"items": [{{"image_base64": "{}"}}, {{"heart_girth_cm": 180, "body_length_cm": 150}}, {{"prompt": "no image"}}]}}"#,
        png_b64()
    );
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-batch",
        Some(&session),
        Some(&payload),
    );
    assert_eq!(status, 200);
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["status"], 200);
    assert!(results[0]["body"].get("history_id").is_some());
    assert_eq!(results[1]["status"], 200);
    assert_eq!(results[2]["status"], 400);
    // One photo estimate spent; two more are allowed, the third is not.
    let photo = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    for _ in 0..2 {
        let (status, _, _) = authed(
            &server,
            "POST",
            "/estimate-weight",
            Some(&session),
            Some(&photo),
        );
        assert_eq!(status, 200);
    }
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo),
    );
    assert_eq!(status, 429);
    assert_eq!(body["code"], "quota_exceeded");
}

#[test]
fn fallback_rows_are_flagged_placeholders_not_ai() {
    let (server, dir) = setup_auth(&[], "placeholder");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let payload = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&payload),
    );
    assert_eq!(status, 200);
    assert_eq!(body["source"], "local_fallback");
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["items"][0]["source"], "local_fallback");
    assert_eq!(list["items"][0]["placeholder"], true);
}

#[test]
fn account_export_contains_everything_owned() {
    let (server, dir) = setup_auth(&[], "export");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/animals",
        Some(&session),
        Some(r#"{"name": "Bessie"}"#),
    );
    assert_eq!(status, 200);
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 200);
    let (status, _, export) = authed(&server, "GET", "/api/account/export", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(export["user"]["email"], "a@example.com");
    assert!(export.get("password_hash").is_none());
    assert_eq!(export["animals"].as_array().unwrap().len(), 1);
    assert_eq!(export["estimates"].as_array().unwrap().len(), 1);
    let text = serde_json::to_string(&export).unwrap();
    assert!(!text.contains("aif_session"));
}

#[test]
fn js_guards_cover_account_assets() {
    let js = web_file("account.js");
    assert!(js.contains("textContent"));
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("localStorage"));
    assert!(!js.contains("sessionStorage"));
    for needle in [
        "accept-invite",
        "X-CSRF-Token",
        "animal-select",
        "history/export",
        "operator/pause",
        "aif-estimate-saved",
    ] {
        assert!(js.contains(needle), "account.js missing {}", needle);
    }
    let router = web_file("router.js");
    for needle in [
        "popstate",
        "session_probe_failed",
        "aria-current",
        "Show password",
    ] {
        assert!(router.contains(needle), "router.js missing {}", needle);
    }
    assert!(!router.contains("innerHTML"));
    assert!(!router.contains("localStorage"));
    let html = web_file("index.html");
    for needle in [
        r#"id="auth-bar""#,
        r#"id="login-form""#,
        r#"id="invite-form""#,
        r#"id="recovery-form""#,
        r#"id="animal-select""#,
        r#"id="server-history-list""#,
        r#"id="operator-panel""#,
        "/account.js",
        "/account.css",
        "/router.js",
        "/pages.css",
        "no public registration",
        "HEIC",
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}

// --- photo retention, durable jobs, operator alerts (items 8-10) ---

fn setup_retention(extra: &[(&str, &str)], prefix: &str) -> (TestServer, String) {
    let data_dir = unique_data_dir(prefix);
    let mut env: Vec<(&str, &str)> = vec![
        ("AIF_AI_BACKEND", "none"),
        ("AIF_REQUIRE_AUTH", "1"),
        ("AIF_DATA_DIR", &data_dir),
        ("AIF_RETAIN_PHOTOS", "1"),
    ];
    env.extend_from_slice(extra);
    (TestServer::new(&env), data_dir)
}

fn photo_payload() -> String {
    format!(
        r#"{{"image_base64": "{}", "retain_photo": true}}"#,
        png_b64()
    )
}

#[test]
fn photo_retention_lifecycle_owner_only() {
    let (server, dir) = setup_retention(&[], "photoret");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    // Retain a photo with the estimate.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo_payload()),
    );
    assert_eq!(status, 200);
    let photo_id = body["photo_id"].as_str().expect("photo_id").to_string();
    assert_eq!(photo_id.len(), 32);
    assert!(body.get("photo_error").is_none());
    // Metadata listing + private owner-only download.
    let (status, _, list) = authed(&server, "GET", "/api/photos", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["photos"].as_array().unwrap().len(), 1);
    assert_eq!(list["photos"][0]["mime"], "image/png");
    let (status, headers, raw) = authed_raw(
        &server,
        "GET",
        &format!("/api/photos/{}", photo_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert!(headers["content-type"].contains("image/png"));
    assert_eq!(
        headers.get("cache-control").map(|s| s.as_str()),
        Some("private, max-age=86400")
    );
    assert!(!headers.contains_key("access-control-allow-origin"));
    assert_eq!(raw, png_bytes());
    // File actually lives under an opaque id in the photo dir.
    assert!(std::path::Path::new(&dir)
        .join("photos")
        .join(&photo_id)
        .is_file());
    // Another user gets 404 (no existence oracle); anonymous gets 401.
    let token_b = cli_invite(&dir, "b@example.com", "user");
    let (_, _, _, session_b) = accept_invite(&server, &token_b, "password for bobby", "Bobby");
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/photos/{}", photo_id),
        Some(&session_b.unwrap()),
        None,
    );
    assert_eq!(status, 404);
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/photos/{}", photo_id),
        None,
        None,
    );
    assert_eq!(status, 401);
    let (status, _, _) = authed(
        &server,
        "GET",
        "/api/photos/../../../../etc/passwdxxxxxxxxxxxxxxxx",
        Some(&session),
        None,
    );
    assert_eq!(status, 404);
    // Owner deletion removes the row and the file.
    let (status, _, _) = authed(
        &server,
        "DELETE",
        &format!("/api/photos/{}", photo_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    assert!(!std::path::Path::new(&dir)
        .join("photos")
        .join(&photo_id)
        .exists());
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/photos/{}", photo_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 404);
}

#[test]
fn photo_history_delete_removes_linked_photo() {
    let (server, dir) = setup_retention(&[], "photohist");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo_payload()),
    );
    assert_eq!(status, 200);
    let photo_id = body["photo_id"].as_str().unwrap().to_string();
    let history_id = body["history_id"].as_str().unwrap().to_string();
    let (status, _, _) = authed(
        &server,
        "DELETE",
        &format!("/api/history/{}", history_id),
        Some(&session),
        None,
    );
    assert_eq!(status, 200);
    // Row cascaded via FK and the file is gone.
    let (status, _, list) = authed(&server, "GET", "/api/photos", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["photos"].as_array().unwrap().len(), 0);
    assert!(!std::path::Path::new(&dir)
        .join("photos")
        .join(&photo_id)
        .exists());
}

#[test]
fn photo_retention_off_by_default() {
    let (server, dir) = setup_auth(&[], "photodefault");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    // Retention quietly stays off: the estimate succeeds, nothing is stored.
    let (status, _, body) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo_payload()),
    );
    assert_eq!(status, 200);
    assert!(body.get("photo_id").is_none());
    let (status, _, list) = authed(&server, "GET", "/api/photos", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["photos"].as_array().unwrap().len(), 0);
    let (status, _, summary) = authed(&server, "GET", "/api/account", Some(&session), None);
    assert_eq!(status, 200);
    assert!(summary["photo_policy"]
        .as_str()
        .unwrap()
        .contains("no permanent photo retention"));
}

#[test]
fn photo_account_wipe_clears_files() {
    let (server, dir) = setup_retention(&[], "photowipe");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, _) = authed(
        &server,
        "POST",
        "/estimate-weight",
        Some(&session),
        Some(&photo_payload()),
    );
    assert_eq!(status, 200);
    let photo_dir = std::path::Path::new(&dir).join("photos");
    assert_eq!(std::fs::read_dir(&photo_dir).unwrap().count(), 1);
    let (status, _, _) = authed(
        &server,
        "DELETE",
        "/api/account",
        Some(&session),
        Some(r#"{"password": "long enough password"}"#),
    );
    assert_eq!(status, 200);
    let leftovers: usize = std::fs::read_dir(&photo_dir)
        .map(|e| e.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
}

fn poll_job(server: &TestServer, session: &Session, id: &str, secs: u64) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let (status, _, job) = authed(
            server,
            "GET",
            &format!("/api/jobs/{}", id),
            Some(session),
            None,
        );
        assert_eq!(status, 200);
        let terminal = matches!(
            job["status"].as_str(),
            Some("success") | Some("failed") | Some("cancelled") | Some("expired")
        );
        if terminal || Instant::now() >= deadline {
            return job;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn job_lifecycle_saves_exactly_once() {
    let (server, dir) = setup_auth(&[], "joblife");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, created) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "job-1"}"#),
    );
    assert_eq!(status, 202);
    let job_id = created["id"].as_str().unwrap().to_string();
    let job = poll_job(&server, &session, &job_id, 15);
    assert_eq!(job["status"], "success");
    assert_eq!(job["result"]["estimated_weight_kg"], 448.4);
    assert!(job["history_id"].as_str().is_some());
    // Exactly one history row: no duplicates from the background path.
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&session), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 1);
    // Same idempotency key returns the same job, not a new row.
    let (status, _, again) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&session),
        Some(r#"{"heart_girth_cm": 170, "body_length_cm": 140, "idempotency_key": "job-1"}"#),
    );
    assert_eq!(status, 202);
    assert_eq!(again["id"], job_id);
    assert_eq!(again["replayed"], true);
}

#[test]
fn job_failed_payload_is_permanent() {
    let (server, dir) = setup_auth(&[], "jobfail");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    let (status, _, created) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180}"#),
    );
    assert_eq!(status, 202);
    let job = poll_job(&server, &session, created["id"].as_str().unwrap(), 15);
    assert_eq!(job["status"], "failed");
    assert_eq!(job["error_code"], "invalid_options");
    assert_eq!(job["attempts"], 1);
}

#[test]
fn job_cancel_and_isolation() {
    let (server, dir) = setup_auth(&[("AIF_JOB_WORKER", "0")], "jobcancel");
    let token_a = cli_invite(&dir, "a@example.com", "operator");
    let (_, _, _, session_a) = accept_invite(&server, &token_a, "password for alice", "Alice");
    let token_b = cli_invite(&dir, "b@example.com", "user");
    let (_, _, _, session_b) = accept_invite(&server, &token_b, "password for bobby", "Bobby");
    let (a, b) = (session_a.unwrap(), session_b.unwrap());
    let (status, _, created) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&a),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#),
    );
    assert_eq!(status, 202);
    let job_id = created["id"].as_str().unwrap().to_string();
    // B cannot see or cancel A's job.
    let (status, _, _) = authed(
        &server,
        "GET",
        &format!("/api/jobs/{}", job_id),
        Some(&b),
        None,
    );
    assert_eq!(status, 404);
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/jobs/{}/cancel", job_id),
        Some(&b),
        None,
    );
    assert_eq!(status, 404);
    // A cancels the still-queued job (worker disabled).
    let (status, _, _) = authed(
        &server,
        "POST",
        &format!("/api/jobs/{}/cancel", job_id),
        Some(&a),
        Some("{}"),
    );
    assert_eq!(status, 200);
    let (status, _, job) = authed(
        &server,
        "GET",
        &format!("/api/jobs/{}", job_id),
        Some(&a),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(job["status"], "cancelled");
    // Cancelling twice fails loudly (already terminal).
    let (status, _, body) = authed(
        &server,
        "POST",
        &format!("/api/jobs/{}/cancel", job_id),
        Some(&a),
        Some("{}"),
    );
    assert_eq!(status, 400);
    assert_eq!(body["code"], "invalid_options");
}

#[test]
fn job_queue_depth_is_bounded() {
    let (server, dir) = setup_auth(&[("AIF_JOB_WORKER", "0")], "jobqueue");
    let token = cli_invite(&dir, "a@example.com", "user");
    let (_, _, _, session) = accept_invite(&server, &token, "long enough password", "A");
    let session = session.unwrap();
    for i in 0..20 {
        let payload = format!(
            r#"{{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "q-{}"}}"#,
            i
        );
        let (status, _, _) = authed(&server, "POST", "/api/jobs", Some(&session), Some(&payload));
        assert_eq!(status, 202, "job {}", i);
    }
    let (status, _, body) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "q-full"}"#),
    );
    assert_eq!(status, 429);
    assert_eq!(body["code"], "rate_limited");
    let (status, _, replay) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&session),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "q-0"}"#),
    );
    assert_eq!(status, 202, "exact replay must work at capacity: {replay}");
    assert_eq!(replay["replayed"], true);
}

#[test]
fn job_survives_pause_and_restart_exactly_once() {
    let (mut server, dir) = setup_auth(&[], "jobrestart");
    let token = cli_invite(&dir, "op@example.com", "operator");
    let (_, _, _, op) = accept_invite(&server, &token, "operator password 1", "Op");
    let op = op.unwrap();
    // Pause: the worker leaves jobs queued without failing them.
    let (status, _, _) = authed(
        &server,
        "POST",
        "/api/operator/pause",
        Some(&op),
        Some(r#"{"paused": true}"#),
    );
    assert_eq!(status, 200);
    let (status, _, created) = authed(
        &server,
        "POST",
        "/api/jobs",
        Some(&op),
        Some(r#"{"heart_girth_cm": 180, "body_length_cm": 150, "idempotency_key": "restart-1"}"#),
    );
    assert_eq!(status, 202);
    let job_id = created["id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(1200));
    let (status, _, still) = authed(
        &server,
        "GET",
        &format!("/api/jobs/{}", job_id),
        Some(&op),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(still["status"], "queued");
    // Restart clears the runtime pause; the worker picks the job up and the
    // idempotency key guarantees a single saved result.
    server.restart();
    let fresh = login(&server, "op@example.com", "operator password 1");
    let job = poll_job(&server, &fresh, &job_id, 15);
    assert_eq!(job["status"], "success");
    let (status, _, list) = authed(&server, "GET", "/api/history", Some(&fresh), None);
    assert_eq!(status, 200);
    assert_eq!(list["total"], 1);
}

#[test]
fn operator_status_carries_spending_alerts() {
    let (server, dir) = setup_auth(&[("AIF_DAILY_LIMIT", "2")], "alerts");
    let token = cli_invite(&dir, "op@example.com", "operator");
    let (_, _, _, op) = accept_invite(&server, &token, "operator password 1", "Op");
    let op = op.unwrap();
    let (status, _, quiet) = authed(&server, "GET", "/api/operator/status", Some(&op), None);
    assert_eq!(status, 200);
    assert!(quiet["alerts"].as_array().unwrap().is_empty());
    let photo = format!(r#"{{"image_base64": "{}"}}"#, png_b64());
    for _ in 0..2 {
        let (status, _, _) = authed(&server, "POST", "/estimate-weight", Some(&op), Some(&photo));
        assert_eq!(status, 200);
    }
    let (status, _, loud) = authed(&server, "GET", "/api/operator/status", Some(&op), None);
    assert_eq!(status, 200);
    let alerts = loud["alerts"].as_array().unwrap();
    assert!(alerts
        .iter()
        .any(|a| a["kind"] == "usage_high"
            && a["message"].as_str().unwrap().contains("op@example.com")));
    assert!(loud.get("data_dir_mb").is_some());
    assert!(loud.get("photo_retention").is_some());
    assert!(loud.get("jobs_enabled").is_some());
}

#[test]
fn js_guards_cover_jobs_photos_and_background() {
    let js = web_file("account.js");
    for needle in [
        "api/jobs",
        "api/photos",
        "job-list",
        "photo-list",
        "aif-jobs-changed",
    ] {
        assert!(js.contains(needle), "account.js missing {}", needle);
    }
    let app = web_file("app.js");
    for needle in [
        "background-button",
        "runBackground",
        "retain_photo",
        "idempotency_key",
        "aif-jobs-changed",
    ] {
        assert!(app.contains(needle), "app.js missing {}", needle);
    }
    assert!(app.contains("textContent"));
    assert!(!app.contains("innerHTML"));
    let html = web_file("index.html");
    for needle in [
        r#"id="background-button""#,
        r#"id="retain-photo""#,
        r#"id="job-list""#,
        r#"id="photo-list""#,
        r#"id="job-empty""#,
        r#"id="photo-empty""#,
    ] {
        assert!(html.contains(needle), "index.html missing {}", needle);
    }
}
