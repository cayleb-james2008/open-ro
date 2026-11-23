//! v0.11 Universal Anything-to-API / Anything-to-CLI Tool Adapter Registry tests (#52).
//!
//! Uses only local mock CLI scripts and loopback mock HTTP servers (zero live external calls).

use openro_rs::agent::Agent;
use openro_rs::assets::encode_base64;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("adapter-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn boot_server(dir: &Path) -> (u16, PathBuf, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(proj.join("art/cards")).unwrap();
    std::fs::create_dir_all(proj.join("assets/audio")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "AdapterTest", "slug": "adapter-test" }).to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({ "name": "AdapterTest", "tree": { "$className": "DataModel" } }).to_string(),
    )
    .unwrap();

    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "AdapterTest".into()));
    let shared = Shared::with_data_dir(Some(agent), "AdapterTest".into(), data.clone());
    {
        let mut store: std::sync::MutexGuard<'_, ProjectStore> = shared.projects.lock().unwrap();
        let entry = store.open(proj.to_str().unwrap()).unwrap();
        store.set_active(&entry.id);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, data, proj, shared)
}

fn http_req(
    port: u16,
    method: &str,
    path: &str,
    host_override: Option<&str>,
    body: Option<&Value>,
) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    let host = host_override
        .map(String::from)
        .unwrap_or_else(|| format!("127.0.0.1:{port}"));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, resp_body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, serde_json::from_str(resp_body).unwrap_or(Value::Null))
}

fn minimal_png() -> Vec<u8> {
    // Valid 1x1 PNG signature + header + minimal payload bytes for asset tests.
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I',
        b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn spawn_mock_http_server() -> (u16, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hit_count = Arc::new(AtomicUsize::new(0));
    let poll_count = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&hit_count);
    let polls = Arc::clone(&poll_count);

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut raw = Vec::new();
            let mut tmp = [0u8; 4096];
            let mut header_end = None;
            let mut content_len = 0usize;
            while let Ok(n) = stream.read(&mut tmp) {
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let head = String::from_utf8_lossy(&raw[..pos]);
                        for line in head.lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.eq_ignore_ascii_case("content-length") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                if let Some(he) = header_end {
                    if raw.len() >= he + content_len {
                        break;
                    }
                }
            }
            let Some(he) = header_end else { continue };
            let head = String::from_utf8_lossy(&raw[..he]).to_string();
            let req_body = raw[he..].to_vec();
            let first = head.lines().next().unwrap_or("");
            let parts: Vec<&str> = first.split_whitespace().collect();
            let method = *parts.first().unwrap_or(&"GET");
            let path = *parts.get(1).unwrap_or(&"/");
            hits.fetch_add(1, Ordering::SeqCst);

            let (status, ct, body_bytes): (&str, &str, Vec<u8>) = match (method, path) {
                ("GET", "/health") => (
                    "200 OK",
                    "application/json",
                    json!({ "status": "ok" }).to_string().into_bytes(),
                ),
                ("POST", "/queue") => {
                    let has_key = head.contains("Key fal-secret-tok-12345");
                    if !has_key {
                        (
                            "401 Unauthorized",
                            "application/json",
                            json!({ "error": "missing auth" }).to_string().into_bytes(),
                        )
                    } else {
                        (
                            "200 OK",
                            "application/json",
                            json!({
                                "request_id": "job1",
                                "status_url": format!("http://127.0.0.1:{port}/status/job1")
                            })
                            .to_string()
                            .into_bytes(),
                        )
                    }
                }
                ("GET", "/status/job1") => {
                    let n = polls.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        (
                            "200 OK",
                            "application/json",
                            json!({ "status": "IN_PROGRESS" }).to_string().into_bytes(),
                        )
                    } else {
                        (
                            "200 OK",
                            "application/json",
                            json!({
                                "status": "COMPLETED",
                                "response_url": format!("http://127.0.0.1:{port}/result/job1")
                            })
                            .to_string()
                            .into_bytes(),
                        )
                    }
                }
                ("GET", "/result/job1") => (
                    "200 OK",
                    "application/json",
                    json!({
                        "images": [{ "url": format!("http://127.0.0.1:{port}/img/final.png") }]
                    })
                    .to_string()
                    .into_bytes(),
                ),
                ("GET", "/img/final.png") => ("200 OK", "image/png", minimal_png()),
                ("POST", "/b64") => (
                    "200 OK",
                    "application/json",
                    json!({
                        "data": [{ "b64_json": encode_base64(&minimal_png()) }]
                    })
                    .to_string()
                    .into_bytes(),
                ),
                ("POST", "/bytes") => ("200 OK", "audio/wav", b"RIFFmockwavdata".to_vec()),
                ("POST", "/multipart") => {
                    let body_str = String::from_utf8_lossy(&req_body);
                    let ok = head.to_ascii_lowercase().contains("multipart/form-data")
                        && body_str.contains("name=\"prompt\"")
                        && body_str.contains("hero card")
                        && body_str.contains("REF_PNG_PAYLOAD");
                    if ok {
                        (
                            "200 OK",
                            "application/json",
                            json!({ "uploaded": true }).to_string().into_bytes(),
                        )
                    } else {
                        (
                            "400 Bad Request",
                            "application/json",
                            json!({ "error": "multipart mismatch" }).to_string().into_bytes(),
                        )
                    }
                }
                ("POST", "/fail-leak") => (
                    "500 Internal Server Error",
                    "text/plain",
                    b"upstream crash echoing key=super-secret-leak-99999 in trace".to_vec(),
                ),
                ("POST", "/always-500") => (
                    "500 Internal Server Error",
                    "application/json",
                    json!({ "error": "primary provider down" }).to_string().into_bytes(),
                ),
                _ => (
                    "404 Not Found",
                    "application/json",
                    json!({ "error": "not found" }).to_string().into_bytes(),
                ),
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body_bytes.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&body_bytes);
        }
    });
    (port, hit_count, poll_count)
}

#[test]
fn no_hardcoded_personal_home_paths_in_src() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("/home/cayleb"),
                "found hardcoded personal home path in {}",
                path.display()
            );
        }
    }
}

#[test]
fn registry_crud_presets_secrets_loopback_and_prohibited_providers() {
    let dir = scratch("crud");
    let (port, data, _proj, _shared) = boot_server(&dir);

    // Foreign Host rejected with 403.
    let (st, _) = http_req(port, "GET", "/api/adapters", Some("evil.example:80"), None);
    assert_eq!(st, 403);
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        Some("evil.example:80"),
        Some(&json!({ "capability": "audio_gen", "params": {} })),
    );
    assert_eq!(st, 403);

    // List all adapters + presets + templates.
    let (st, list) = http_req(port, "GET", "/api/adapters", None, None);
    assert_eq!(st, 200, "{list}");
    assert_eq!(list["ok"], true);
    let ids: Vec<&str> = list["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    for expected in [
        "chatgpt-browser-cli",
        "gemini-nano-banana-http",
        "openai-images-http",
        "camoufox-browser-cli",
        "builtin-http-search",
        "blender-cycles-cli",
        "rust-synth-builtin",
    ] {
        assert!(ids.contains(&expected), "missing default adapter {expected}: {ids:?}");
    }
    let preset_ids: Vec<&str> = list["presets"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    for expected in ["comfyui-local-http", "fal-flux-http", "meshy-http", "elevenlabs-sfx-http"] {
        assert!(preset_ids.contains(&expected), "missing preset {expected}: {preset_ids:?}");
    }
    assert_eq!(list["templates"].as_array().unwrap().len(), 2);

    // Capability filter works.
    let (st, filtered) = http_req(port, "GET", "/api/adapters?capability=audio_gen", None, None);
    assert_eq!(st, 200);
    assert!(filtered["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["capability"] == "audio_gen"));

    // 1-click preset install with secret_values -> secret goes ONLY to SecretStore, never adapters.json or API response.
    let raw_secret = "fal-live-secret-DO-NOT-PERSIST-8888";
    let (st, created) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "preset_id": "fal-flux-http",
            "priority": 15,
            "secret_values": { "fal_api_key": raw_secret }
        })),
    );
    assert_eq!(st, 200, "{created}");
    assert_eq!(created["adapter"]["id"], "fal-flux-http");
    assert_eq!(created["adapter"]["priority"], 15);
    assert_eq!(created["adapter"]["secrets_status"]["fal_api_key"]["has_key"], true);
    assert!(!created.to_string().contains(raw_secret), "API response leaked secret: {created}");

    let adapters_disk = std::fs::read_to_string(data.join("adapters.json")).unwrap();
    assert!(!adapters_disk.contains(raw_secret), "adapters.json leaked secret!");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let sec_dir = openro_rs::secrets::SecretStore::for_data_dir(&data).dir().to_path_buf();
        let dir_mode = std::fs::metadata(&sec_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "secrets dir must be 0700, got {dir_mode:o}");
        let sec_path = sec_dir.join("fal_api_key");
        let mode = std::fs::metadata(&sec_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "secret file must be 0600, got {mode:o}");
    }

    // Higgsfield / Soul Cinema prohibited.
    let (st, rej) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "higgsfield-gen",
            "name": "Higgsfield Soul Cinema",
            "capability": "image_gen",
            "transport": "http",
            "http": {
                "method": "POST",
                "url_template": "https://api.higgsfield.ai/v1/soul-cinema",
                "response_extract": { "mode": "json" }
            }
        })),
    );
    assert_eq!(st, 400, "{rej}");
    assert!(rej["error"].as_str().unwrap_or("").to_ascii_lowercase().contains("prohibited"));

    // Partial update from UI (toggle enabled, bump priority, save secret) preserves existing http config.
    let (st, toggled) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "fal-flux-http",
            "name": "fal.ai Flux",
            "capability": "image_gen",
            "transport": "http",
            "enabled": false,
            "priority": 12
        })),
    );
    assert_eq!(st, 200, "{toggled}");
    assert_eq!(toggled["adapter"]["enabled"], false);
    assert_eq!(toggled["adapter"]["priority"], 12);
    assert!(toggled["adapter"]["http"]["url_template"].as_str().unwrap_or("").contains("fal.run"));

    // UI form shorthands: CLI argv[0] without separate command, HTTP string response_extract, uppercase secret key.
    let (st, ui_cli) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "ui-cli-shorthand",
            "name": "UI CLI Shorthand",
            "capability": "custom",
            "transport": "cli",
            "cli": { "argv": ["echo", "--prompt", "{{prompt}}"], "output_mode": "stdout_json" }
        })),
    );
    assert_eq!(st, 200, "{ui_cli}");
    assert_eq!(ui_cli["adapter"]["cli"]["command"], "echo");
    assert_eq!(ui_cli["adapter"]["cli"]["argv"], json!(["--prompt", "{{prompt}}"]));

    let (st, ui_http) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "ui-http-shorthand",
            "name": "UI HTTP Shorthand",
            "capability": "image_gen",
            "transport": "http",
            "required_secrets": ["CUSTOM_API_KEY"],
            "secret_values": { "CUSTOM_API_KEY": "upper-key-val-1234" },
            "http": {
                "method": "POST",
                "url_template": "https://api.example.com/v1/gen",
                "headers_template": { "Authorization": "Bearer {{secret:CUSTOM_API_KEY}}" },
                "response_extract": "image_url_path:data.0.url"
            }
        })),
    );
    assert_eq!(st, 200, "{ui_http}");
    assert_eq!(ui_http["adapter"]["http"]["response_extract"]["mode"], "image_url_path");
    assert_eq!(ui_http["adapter"]["http"]["response_extract"]["path"], "data.0.url");
    assert_eq!(ui_http["adapter"]["secrets_status"]["custom_api_key"]["has_key"], true);

    // Delete adapter.
    let (st, del) = http_req(port, "DELETE", "/api/adapters/fal-flux-http", None, None);
    assert_eq!(st, 200, "{del}");
    assert_eq!(del["deleted"], true);
    let (st, list2) = http_req(port, "GET", "/api/adapters", None, None);
    assert_eq!(st, 200);
    assert!(!list2["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"] == "fal-flux-http"));
}

#[cfg(unix)]
#[test]
fn cli_adapter_strict_argv_no_shell_injection_exit_codes_and_output_modes() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch("cli");
    let (port, _data, proj, _shared) = boot_server(&dir);
    let script_path = dir.join("mock_tool.sh");
    let argv_dump = dir.join("captured_argv.txt");
    let pwned_marker = dir.join("SHOULD_NOT_EXIST");

    std::fs::write(
        &script_path,
        format!(
            r#"#!/bin/bash
if [ "$1" = "--version" ]; then
  echo "mock-tool 1.2.3"
  exit 0
fi
printf '%s\n' "$@" > "{}"
MODE="$1"
shift
case "$MODE" in
  file)
    OUT="$1"
    printf '\x89PNG\r\n\x1a\nMOCK_CLI_IMG' > "$OUT"
    exit 0
    ;;
  json)
    echo '{{"meta":{{"ok":true}},"result":{{"items":["a","b","c"]}}}}'
    exit 0
    ;;
  pending)
    echo '{{"review":"angle_check","score":0.42}}'
    exit 9
    ;;
  refused)
    echo "blocked by moderation policy" >&2
    exit 7
    ;;
  leak)
    echo "failure with secret=$TEST_SECRET_ENV" >&2
    exit 1
    ;;
  *)
    echo "unknown mode $MODE" >&2
    exit 2
    ;;
esac
"#,
            argv_dump.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Register CLI file adapter with optional {{?ref_image}} and health_argv.
    let (st, res) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-cli-file",
            "name": "Mock CLI Image",
            "capability": "custom",
            "transport": "cli",
            "priority": 10,
            "required_secrets": ["cli_test_token"],
            "secret_values": { "cli_test_token": "cli-secret-val-7777" },
            "cli": {
                "command": script_path.to_string_lossy(),
                "argv": ["file", "{{out_path}}", "--prompt", "{{prompt}}", "--ref", "{{?ref_image}}"],
                "env_secrets": { "TEST_SECRET_ENV": "cli_test_token" },
                "cwd": "project",
                "timeout_secs": 30,
                "output_mode": "file",
                "exit_code_map": { "2": "login_required", "7": "refused", "9": "pending_review" },
                "health_argv": ["--version"]
            }
        })),
    );
    assert_eq!(st, 200, "{res}");

    // Health check passes.
    let (st, tested) = http_req(port, "POST", "/api/adapters/mock-cli-file/test", None, None);
    assert_eq!(st, 200, "{tested}");
    assert_eq!(tested["available"], true);
    assert_eq!(tested["status"], "ready");

    // Invoke with shell metacharacters in prompt & NO ref_image:
    // - Shell command MUST NOT execute (pwned_marker must NOT exist).
    // - Optional `--ref` + `{{?ref_image}}` MUST be omitted from argv.
    let evil_prompt = format!("$(touch {}) ; echo hacked | cat", pwned_marker.display());
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "adapter_id": "mock-cli-file",
            "params": {
                "prompt": evil_prompt,
                "out_path": "art/cards/cli_out.png"
            }
        })),
    );
    assert_eq!(st, 200, "{inv}");
    assert!(!pwned_marker.exists(), "shell injection executed!");
    assert_eq!(inv["result"]["path"], "art/cards/cli_out.png");
    assert!(proj.join("art/cards/cli_out.png").is_file());
    let captured = std::fs::read_to_string(&argv_dump).unwrap();
    let lines: Vec<&str> = captured.lines().collect();
    assert_eq!(lines.len(), 4, "expected `--ref` omitted when ref_image empty: {lines:?}");
    assert_eq!(lines[3], evil_prompt, "prompt must be passed literally as one argv token");

    // Path traversal & protected art/references/ rejected.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "adapter_id": "mock-cli-file",
            "params": { "prompt": "x", "out_path": "../escape.png" }
        })),
    );
    assert_ne!(st, 200);
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "adapter_id": "mock-cli-file",
            "params": { "prompt": "x", "out_path": "art/references/blocked.png" }
        })),
    );
    assert_ne!(st, 200);

    // stdout_json + result_json_path extraction.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-cli-json",
            "name": "Mock CLI JSON",
            "capability": "custom",
            "transport": "cli",
            "cli": {
                "command": script_path.to_string_lossy(),
                "argv": ["json"],
                "output_mode": "stdout_json",
                "result_json_path": "result.items"
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "mock-cli-json", "params": {} })),
    );
    assert_eq!(st, 200, "{inv}");
    assert_eq!(inv["result"]["data"], json!(["a", "b", "c"]));

    // Exit code 9 -> structured pending_review (non-error).
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-cli-pending",
            "name": "Mock CLI Pending",
            "capability": "custom",
            "transport": "cli",
            "cli": {
                "command": script_path.to_string_lossy(),
                "argv": ["pending"],
                "exit_code_map": { "9": "pending_review" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "mock-cli-pending", "params": {} })),
    );
    assert_eq!(st, 200, "{inv}");
    assert_eq!(inv["result"]["status"], "pending_review");
    assert_eq!(inv["result"]["data"]["review"], "angle_check");

    // Secret scrubbing on CLI stderr error.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-cli-leak",
            "name": "Mock CLI Leak",
            "capability": "custom",
            "transport": "cli",
            "required_secrets": ["cli_test_token"],
            "cli": {
                "command": script_path.to_string_lossy(),
                "argv": ["leak"],
                "env_secrets": { "TEST_SECRET_ENV": "cli_test_token" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, err) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "mock-cli-leak", "params": {} })),
    );
    assert_eq!(st, 502, "{err}");
    let err_s = err.to_string();
    assert!(err_s.contains("[REDACTED]"), "expected [REDACTED] in {err_s}");
    assert!(!err_s.contains("cli-secret-val-7777"), "raw secret leaked in {err_s}");

    // Terminal `refused` stops fallback chain (never tries priority 20).
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "chain-refuse-1",
            "name": "Refuser",
            "capability": "custom",
            "transport": "cli",
            "priority": 1,
            "cli": {
                "command": script_path.to_string_lossy(),
                "argv": ["refused"],
                "exit_code_map": { "7": "refused" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, err) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "capability": "custom", "params": {} })),
    );
    assert_eq!(st, 502, "{err}");
    assert!(err["error"].as_str().unwrap_or("").contains("refused"), "{err}");
}

#[test]
fn http_adapter_ssrf_guard_async_poll_extractors_and_secret_scrubbing() {
    let dir = scratch("http");
    let (port, _data, proj, _shared) = boot_server(&dir);
    let (mock_port, hits, polls) = spawn_mock_http_server();

    // 1. SSRF guard blocks loopback by default (allow_loopback: false) WITHOUT connecting.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "ssrf-blocked",
            "name": "SSRF Test",
            "capability": "custom",
            "transport": "http",
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/queue"),
                "allow_loopback": false,
                "health_url": format!("http://127.0.0.1:{mock_port}/health"),
                "response_extract": { "mode": "json" }
            }
        })),
    );
    assert_eq!(st, 200);
    let before_hits = hits.load(Ordering::SeqCst);
    let (st, tested) = http_req(port, "POST", "/api/adapters/ssrf-blocked/test", None, None);
    assert_eq!(st, 200, "{tested}");
    assert_eq!(tested["available"], false);
    assert_eq!(tested["status"], "blocked_ssrf");
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "ssrf-blocked", "params": { "prompt": "hi" } })),
    );
    assert_eq!(st, 400, "{inv}");
    assert!(inv["error"].as_str().unwrap_or("").contains("blocked_ssrf"));
    assert_eq!(hits.load(Ordering::SeqCst), before_hits, "SSRF-blocked request must never touch loopback server");

    // 2. Async polling HTTP adapter (allow_loopback: true) + secret header + image_url_path download.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-fal-async",
            "name": "Mock Async Poll Image",
            "capability": "custom",
            "transport": "http",
            "required_secrets": ["mock_fal_key"],
            "secret_values": { "mock_fal_key": "fal-secret-tok-12345" },
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/queue"),
                "allow_loopback": true,
                "headers_template": { "Authorization": "Key {{secret:mock_fal_key}}" },
                "body_mode": "json",
                "body_template": { "prompt": "{{prompt}}" },
                "timeout_secs": 15,
                "poll_spec": {
                    "url_template": "{{resp:status_url}}",
                    "status_path": "status",
                    "done_values": ["COMPLETED"],
                    "fail_values": ["FAILED"],
                    "result_url_path": "response_url",
                    "interval_ms": 20,
                    "max_attempts": 10
                },
                "response_extract": { "mode": "image_url_path", "path": "images.0.url" },
                "health_url": format!("http://127.0.0.1:{mock_port}/health")
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, tested) = http_req(port, "POST", "/api/adapters/mock-fal-async/test", None, None);
    assert_eq!(st, 200, "{tested}");
    assert_eq!(tested["available"], true);
    assert_eq!(tested["status"], "ready");

    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "adapter_id": "mock-fal-async",
            "params": { "prompt": "flame swordsman", "out_path": "art/cards/polled.png" }
        })),
    );
    assert_eq!(st, 200, "{inv}");
    assert!(polls.load(Ordering::SeqCst) >= 2, "expected at least 2 poll requests");
    assert_eq!(inv["result"]["path"], "art/cards/polled.png");
    assert_eq!(std::fs::read(proj.join("art/cards/polled.png")).unwrap(), minimal_png());

    // 3. image_b64_path + file_bytes + multipart.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-b64",
            "name": "Mock B64",
            "capability": "custom",
            "transport": "http",
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/b64"),
                "allow_loopback": true,
                "response_extract": { "mode": "image_b64_path", "path": "data.0.b64_json" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "mock-b64", "params": { "out_path": "art/cards/from_b64.png" } })),
    );
    assert_eq!(st, 200, "{inv}");
    assert_eq!(std::fs::read(proj.join("art/cards/from_b64.png")).unwrap(), minimal_png());

    // Multipart with in-project ref_image.
    std::fs::write(proj.join("art/cards/ref.png"), b"REF_PNG_PAYLOAD").unwrap();
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-mp",
            "name": "Mock Multipart",
            "capability": "custom",
            "transport": "http",
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/multipart"),
                "allow_loopback": true,
                "body_mode": "multipart",
                "body_template": {
                    "fields": { "prompt": "{{prompt}}" },
                    "file_field": "image",
                    "file_path": "{{ref_image}}"
                },
                "response_extract": { "mode": "json" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "adapter_id": "mock-mp",
            "params": { "prompt": "hero card", "ref_image": "art/cards/ref.png" }
        })),
    );
    assert_eq!(st, 200, "{inv}");
    assert_eq!(inv["result"]["data"]["uploaded"], true);

    // 4. Secret scrubbing on HTTP 500 error body.
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "mock-leak",
            "name": "Mock Leak",
            "capability": "custom",
            "transport": "http",
            "required_secrets": ["leak_tok"],
            "secret_values": { "leak_tok": "super-secret-leak-99999" },
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/fail-leak"),
                "allow_loopback": true,
                "headers_template": { "Authorization": "Bearer {{secret:leak_tok}}" },
                "response_extract": { "mode": "json" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, err) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({ "adapter_id": "mock-leak", "params": {} })),
    );
    assert_eq!(st, 502, "{err}");
    let err_s = err.to_string();
    assert!(err_s.contains("[REDACTED]"), "expected [REDACTED] in {err_s}");
    assert!(!err_s.contains("super-secret-leak-99999"), "HTTP error leaked secret: {err_s}");
}

#[test]
fn ordered_priority_fallback_capability_routing_and_agent_tools() {
    let dir = scratch("fallback");
    let (port, _data, proj, _shared) = boot_server(&dir);
    let (mock_port, _hits, _polls) = spawn_mock_http_server();

    // Priority 1 failing image_gen adapter + Priority 5 succeeding image_gen adapter
    // (both higher priority than built-in chatgpt-browser-cli priority 10).
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "img-primary-down",
            "name": "Failing Primary",
            "capability": "image_gen",
            "transport": "http",
            "priority": 1,
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/always-500"),
                "allow_loopback": true,
                "response_extract": { "mode": "image_b64_path", "path": "data.0.b64_json" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "img-secondary-ok",
            "name": "Working Fallback",
            "capability": "image_gen",
            "transport": "http",
            "priority": 5,
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/b64"),
                "allow_loopback": true,
                "response_extract": { "mode": "image_b64_path", "path": "data.0.b64_json" }
            }
        })),
    );
    assert_eq!(st, 200);

    // 1. Direct capability invoke falls back from priority 1 -> priority 5 and records attempts.
    let (st, inv) = http_req(
        port,
        "POST",
        "/api/adapters/invoke",
        None,
        Some(&json!({
            "capability": "image_gen",
            "params": { "prompt": "phoenix", "out_path": "art/cards/fallback.png" }
        })),
    );
    assert_eq!(st, 200, "{inv}");
    assert_eq!(inv["adapter_id"], "img-secondary-ok");
    let attempts = inv["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{inv}");
    assert_eq!(attempts[0]["adapter_id"], "img-primary-down");
    assert_eq!(attempts[0]["ok"], false);
    assert_eq!(attempts[1]["adapter_id"], "img-secondary-ok");
    assert_eq!(attempts[1]["ok"], true);
    assert!(proj.join("art/cards/fallback.png").is_file());

    // 2. Standard `POST /api/art/generate` automatically routes through the active priority chain!
    let (st, art) = http_req(
        port,
        "POST",
        "/api/art/generate",
        None,
        Some(&json!({
            "prompt": "eye-level neutral three-quarter anime knight",
            "out_path": "art/cards/routed.png"
        })),
    );
    assert_eq!(st, 200, "{art}");
    assert_eq!(art["adapter_id"], "img-secondary-ok");
    assert_eq!(art["path"], "art/cards/routed.png");
    assert!(proj.join("art/cards/routed.png").is_file());

    // 3. Standard `POST /api/tools/audio/generate` routes through a custom audio_gen adapter!
    let (st, _) = http_req(
        port,
        "POST",
        "/api/adapters",
        None,
        Some(&json!({
            "id": "audio-custom-http",
            "name": "Mock Audio API",
            "capability": "audio_gen",
            "transport": "http",
            "priority": 1,
            "http": {
                "method": "POST",
                "url_template": format!("http://127.0.0.1:{mock_port}/bytes"),
                "allow_loopback": true,
                "response_extract": { "mode": "file_bytes" }
            }
        })),
    );
    assert_eq!(st, 200);
    let (st, aud) = http_req(
        port,
        "POST",
        "/api/tools/audio/generate",
        None,
        Some(&json!({
            "name": "whoosh",
            "prompt": "sword whoosh",
            "out_path": "assets/audio/whoosh.wav"
        })),
    );
    assert_eq!(st, 200, "{aud}");
    assert_eq!(aud["adapter_id"], "audio-custom-http");
    assert_eq!(std::fs::read(proj.join("assets/audio/whoosh.wav")).unwrap(), b"RIFFmockwavdata");

    // 4. End-to-end Agent turn executing `adapter_list` and `adapter_invoke` tools!
    let turn = Arc::new(AtomicUsize::new(0));
    let turn_c = Arc::clone(&turn);
    let llm = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in llm.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let n = turn_c.fetch_add(1, Ordering::SeqCst);
            let content = match n {
                0 => json!({
                    "steps": [
                        { "tool": "adapter_list", "args": { "capability": "audio_gen" } },
                        { "tool": "adapter_invoke", "args": { "capability": "audio_gen", "params": { "out_path": "assets/audio/from_agent.wav" } } }
                    ],
                    "done": true,
                    "summary": "Generated audio via custom adapter."
                })
                .to_string(),
                _ => json!({ "done": true, "summary": "done" }).to_string(),
            };
            let body = json!({
                "choices": [{ "message": { "role": "assistant", "content": content } }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    let (st, cfg) = http_req(
        port,
        "POST",
        "/api/config",
        None,
        Some(&json!({
            "provider": "openai",
            "base_url": format!("http://127.0.0.1:{llm_port}/v1"),
            "model_id": "mock-model"
        })),
    );
    assert_eq!(st, 200, "{cfg}");
    let (st, chat) = http_req(
        port,
        "POST",
        "/api/chat",
        None,
        Some(&json!({ "message": "Generate a sword whoosh sound via the custom audio adapter." })),
    );
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    assert_eq!(
        std::fs::read(proj.join("assets/audio/from_agent.wav")).unwrap(),
        b"RIFFmockwavdata",
        "Agent adapter_invoke must write assets/audio/from_agent.wav"
    );
    assert!(openro_rs::agent::project_system_prompt().contains("adapter_invoke"));
    assert!(openro_rs::agent::system_prompt().contains("adapter_list"));
}
