//! Cycle 3 Wave H1 Hardening Integration Tests (OpenRO v1.26.0):
//! 1. E1 LiveOps: bounded ds-backups retention (<= 50), telemetry.jsonl bounded rotation +
//!    malformed-line skipping, expected_revision / empty-key / control-char validation (400),
//!    and secrets::scrub coverage across URL-encoded / JSON-escaped / nested upstream errors.
//! 2. E2 Terrain, Scene & Mesh Asset: NaN/Inf/inverted bounds_min >= bounds_max (400),
//!    circular reparent cycles (400), and URL-encoded %2e%2e / %252e%252e / null-byte /
//!    backslash / intermediate symlink escape guards on GET /api/projects/:id/mesh-asset.
//! 3. E3 Headless Sandbox & Gates: bounded .openro/sandbox/ history retention (<= 50)
//!    preserving latest.json, corrupt/truncated latest.json graceful recovery (no 500s),
//!    sandbox_runtime.luau edge cases (zero-length/NaN Raycast, zero-size GetPartBoundsInBox /
//!    GetPartBoundsInRadius, negative/NaN tween durations, malformed http_mocks with 100% zero
//!    outbound network), project_system_prompt < 7 KB, teamwork last_sandbox staleness across
//!    edits, and project_fingerprint invalidation across file/scene/script edits.

use openro_rs::agent::project_system_prompt;
use openro_rs::sandbox::{
    find_lune_binary, list_sandbox_runs, read_latest_sandbox_report, run_project_sandbox,
};
use openro_rs::secrets::{scrub, scrub_value, SecretStore, OPENCLOUD_KEY};
use openro_rs::server::{serve, Shared};
use openro_rs::study_check::{project_fingerprint, study_status};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

struct EnvGuard(Vec<(String, Option<String>)>);
impl EnvGuard {
    fn set(pairs: &[(&str, &str)]) -> Self {
        let mut prev = Vec::new();
        for (k, v) in pairs {
            prev.push(((*k).to_string(), std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        EnvGuard(prev)
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in self.0.drain(..).rev() {
            match old {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}

struct DirCleanup(PathBuf);
impl Drop for DirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v126-cycle3-hardening");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn start_server(data_dir: &Path) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(None, "Hardening v1.26".to_string(), data_dir.to_path_buf());
    let shared_srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_srv);
    });
    (port, shared)
}

fn start_server_with_llm(data_dir: &Path, llm_url: &str) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let agent = openro_rs::agent::Agent::with_local_place(
        openro_rs::model::ModelClient::new(format!("{llm_url}/chat/completions"), "stub".into(), None),
        openro_rs::place::LocalPlace::in_memory("Stub".into()),
    );
    let shared = Shared::with_data_dir(Some(agent), "Hardening Team".to_string(), data_dir.to_path_buf());
    let shared_srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_srv);
    });
    (port, shared)
}

fn http_req(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> (u16, String, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().ok();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let raw_body = if sep + 4 <= buf.len() { &buf[sep + 4..] } else { &[] };
    let parsed = serde_json::from_slice(raw_body)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(raw_body).to_string() }));
    (status, head, parsed)
}

fn create_test_project(port: u16, ws: &Path, name: &str) -> (String, PathBuf) {
    let host = format!("127.0.0.1:{port}");
    let (c1, _, _) = http_req(
        port,
        &host,
        "POST",
        "/api/projects/workspace",
        Some(&json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(c1, 200);
    let (c2, _, res) = http_req(
        port,
        &host,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": name, "universeId": 55667788u64, "placeId": 88776655u64 })),
    );
    assert_eq!(c2, 201, "create failed: {res}");
    let id = res["project"]["id"].as_str().unwrap().to_string();
    let dir = PathBuf::from(res["project"]["path"].as_str().unwrap());
    (id, dir)
}

fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut raw = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_end = None;
    let mut content_len = 0usize;
    loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&tmp[..n]);
        if header_end.is_none() {
            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                let hdr = String::from_utf8_lossy(&raw[..pos]);
                for line in hdr.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
            }
        }
        if let Some(end) = header_end {
            if raw.len() >= end + content_len {
                break;
            }
        }
    }
    let end = header_end.unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..end]).to_string();
    let body_bytes = if end + content_len <= raw.len() {
        raw[end..end + content_len].to_vec()
    } else {
        raw[end..].to_vec()
    };
    (head, body_bytes)
}

#[derive(Default)]
struct MockOcState {
    rev: usize,
    value: Value,
    nested_leak_secret: Option<String>,
}

fn spawn_mock_opencloud(secret: &str) -> (String, Arc<Mutex<MockOcState>>) {
    let state = Arc::new(Mutex::new(MockOcState {
        rev: 1,
        value: json!({ "coins": 100 }),
        nested_leak_secret: Some(secret.to_string()),
    }));
    let srv_state = Arc::clone(&state);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (head, body_bytes) = read_http_request(&mut stream);
            let first = head.lines().next().unwrap_or("");
            let parts: Vec<&str> = first.split_whitespace().collect();
            let method = *parts.first().unwrap_or(&"GET");
            let full_path = *parts.get(1).unwrap_or(&"/");
            let (path, _) = full_path.split_once('?').unwrap_or((full_path, ""));

            let (code, resp_json) = {
                let mut st = srv_state.lock().unwrap();
                if path.contains("/entries/nested_secret_err") {
                    let s = st.nested_leak_secret.clone().unwrap_or_default();
                    let enc = s.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D");
                    (
                        "500 Internal Server Error",
                        json!({
                            "error": {
                                "message": format!("Upstream error raw={s} url={enc}")
                            }
                        }),
                    )
                } else if path.contains("/entries/nested_array_err") {
                    let s = st.nested_leak_secret.clone().unwrap_or_default();
                    (
                        "403 Forbidden",
                        json!({
                            "errors": [{ "detail": format!("Forbidden for token {s}") }]
                        }),
                    )
                } else if method == "GET" && path.contains("/entries/") {
                    let rev_s = format!("rev-{}", st.rev);
                    (
                        "200 OK",
                        json!({
                            "id": "p_1",
                            "value": st.value,
                            "revisionId": rev_s,
                            "revisionCreateTime": "2026-11-26T12:00:00Z"
                        }),
                    )
                } else if method == "PATCH" && path.contains("/entries/") {
                    let v: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));
                    st.value = v.get("value").cloned().unwrap_or(Value::Null);
                    st.rev += 1;
                    let rev_s = format!("rev-{}", st.rev);
                    (
                        "200 OK",
                        json!({
                            "id": "p_1",
                            "value": st.value,
                            "revisionId": rev_s,
                            "revisionCreateTime": "2026-11-26T12:01:00Z"
                        }),
                    )
                } else {
                    ("404 Not Found", json!({ "message": "not found" }))
                }
            };
            let body = resp_json.to_string();
            let resp = format!(
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}"), state)
}

#[test]
fn e1_liveops_backup_rotation_telemetry_rotation_validation_and_nested_scrub() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("e1-liveops");
    let _cleanup = DirCleanup(root.clone());
    let ws = root.join("ws");
    let data = root.join("data");
    let secrets_dir = root.join("secrets");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let tricky_secret = "rbk+sec/key=998877";
    let (mock_base, _) = spawn_mock_opencloud(tricky_secret);
    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
        ("OPENRO_OPENCLOUD_POLL_MS", "1"),
        ("OPENRO_TELEMETRY_MAX_BYTES", "768"),
    ]);

    let (port, _shared) = start_server(&data);
    let host = format!("127.0.0.1:{port}");
    let (pid, pdir) = create_test_project(port, &ws, "LiveOps Hardening");

    let (st_k, _, _) = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": tricky_secret })),
    );
    assert_eq!(st_k, 200);

    // 1. Validation: empty key, control chars, invalid/empty/non-string expected_revision -> 400
    for bad_body in [
        json!({ "store": "PlayerData", "key": "", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "PlayerData", "key": "   ", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "", "key": "p_1", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "Player\x07Data", "key": "p_1", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "PlayerData", "key": "p_1\tbad", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "PlayerData", "scope": "bad\x1bscope", "key": "p_1", "value": { "a": 1 }, "confirm": true }),
        json!({ "store": "PlayerData", "key": "p_1", "value": { "a": 1 }, "expected_revision": "", "confirm": true }),
        json!({ "store": "PlayerData", "key": "p_1", "value": { "a": 1 }, "expected_revision": "   ", "confirm": true }),
        json!({ "store": "PlayerData", "key": "p_1", "value": { "a": 1 }, "expected_revision": 42, "confirm": true }),
        json!({ "store": "PlayerData", "key": "p_1", "value": { "a": 1 }, "expected_revision": "rev\n1", "confirm": true }),
    ] {
        let (code, _, res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{pid}/liveops/datastores/entry"),
            Some(&bad_body),
        );
        assert_eq!(code, 400, "expected 400 for {bad_body}, got {res}");
    }

    // 2. Nested upstream error payloads & URL-encoded secrets are scrubbed cleanly
    for key in ["nested_secret_err", "nested_array_err"] {
        let (code, _, res) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/liveops/datastores/entry?store=PlayerData&key={key}"),
            None,
        );
        assert_eq!(code, 502, "expected 502 for {key}, got {res}");
        let dumped = res.to_string();
        assert!(!dumped.contains(tricky_secret), "raw secret leaked: {dumped}");
        assert!(!dumped.contains("%2B"), "URL-encoded secret leaked: {dumped}");
        assert!(dumped.contains("••••"), "expected scrub dots: {dumped}");
    }
    let scrubbed = scrub("err=rbk%2bsec%2fkey%3d998877", &[tricky_secret]);
    assert_eq!(scrubbed, "err=••••");
    let mut nested = json!({ "outer": { "inner": [format!("leak {tricky_secret}")] } });
    scrub_value(&mut nested, &[tricky_secret]);
    assert!(!nested.to_string().contains(tricky_secret));

    // 3. Bounded ds-backups retention: 55 writes keep <= 50 newest backups
    let mut current_rev = "rev-1".to_string();
    let mut first_backup = String::new();
    let mut last_backup = String::new();
    for i in 0..55 {
        let (code, _, res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{pid}/liveops/datastores/entry"),
            Some(&json!({
                "store": "PlayerData",
                "scope": "global",
                "key": "p_1",
                "value": { "coins": i },
                "expected_revision": current_rev,
                "confirm": true
            })),
        );
        assert_eq!(code, 200, "write {i} failed: {res}");
        current_rev = res["entry"]["revision"].as_str().unwrap().to_string();
        let bpath = res["backup_path"].as_str().unwrap().to_string();
        if i == 0 {
            first_backup = bpath.clone();
        }
        last_backup = bpath;
    }
    let backup_dir = pdir.join(".openro/liveops/ds-backups");
    let backup_count = std::fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .count();
    assert_eq!(backup_count, 50, "expected 50 retained backups, got {backup_count}");
    assert!(!pdir.join(&first_backup).exists(), "oldest backup should be pruned");
    assert!(pdir.join(&last_backup).is_file(), "newest backup must be preserved");

    // 4. Telemetry validation (control chars rejected), bounded rotation, and corrupt-line skipping
    let (st_bad_tel, _, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/liveops/telemetry/ingest"),
        Some(&json!({ "events": [{ "kind": "custom", "user_id": "u\n1" }] })),
    );
    assert_eq!(st_bad_tel, 400);

    for i in 0..10 {
        let (code, _, res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{pid}/liveops/telemetry/ingest"),
            Some(&json!({
                "events": [
                    { "kind": "onboarding", "user_id": format!("usr_{i}"), "step": "Spawned" },
                    { "kind": "economy", "user_id": format!("usr_{i}"), "currency": "Robux", "amount": 25, "flow_type": "sink", "step": "Sword" }
                ]
            })),
        );
        assert_eq!(code, 200, "telemetry ingest {i} failed: {res}");
    }
    let tel_path = pdir.join(".openro/liveops/telemetry.jsonl");
    let arch_path = pdir.join(".openro/liveops/telemetry.jsonl.1");
    assert!(tel_path.is_file());
    assert!(arch_path.is_file(), "expected telemetry.jsonl.1 rotation archive");
    assert!(
        std::fs::metadata(&tel_path).unwrap().len() <= 768,
        "active telemetry.jsonl must stay within cap after rotation"
    );

    // Append corrupt JSON, non-object JSON, and partial lines -> summary still succeeds (200)
    {
        let mut f = std::fs::OpenOptions::new().append(true).open(&tel_path).unwrap();
        writeln!(f, "{{not-valid-json").unwrap();
        writeln!(f, "12345").unwrap();
        writeln!(f, "\"bare-string\"").unwrap();
        writeln!(f, "[1,2,3]").unwrap();
        writeln!(f, "   ").unwrap();
    }
    let (st_sum, _, sum) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/liveops/telemetry/summary"),
        None,
    );
    assert_eq!(st_sum, 200, "summary should skip corrupt lines cleanly: {sum}");
    assert!(sum["total_events"].as_u64().unwrap() >= 2);
}

#[test]
fn e2_terrain_bounds_reparent_cycles_and_mesh_asset_encoding_symlinks() {
    let root = scratch_dir("e2-scene-mesh");
    let _cleanup = DirCleanup(root.clone());
    let ws = root.join("ws");
    let data = root.join("data");
    let outside = root.join("outside");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    write_file(&outside.join("secret.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");

    let (port, _shared) = start_server(&data);
    let host = format!("127.0.0.1:{port}");
    let (pid, pdir) = create_test_project(port, &ws, "Scene Hardening");

    // 1. Terrain NaN/Inf/inverted/equal bounds_min >= bounds_max -> HTTP 400
    for bad_op in [
        json!({
            "op": "set_terrain",
            "paint_box": { "min": ["NaN", 0.0, 0.0], "max": [16.0, 8.0, 16.0], "material": "Grass" }
        }),
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, 0.0, 0.0], "max": ["Inf", 8.0, 16.0], "material": "Grass" }
        }),
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, 8.0, 0.0], "max": [16.0, 8.0, 16.0], "material": "Grass" }
        }),
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, 10.0, 0.0], "max": [16.0, 4.0, 16.0], "material": "Grass" }
        }),
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, 0.0, 10.0], "max": [16.0, 8.0, 10.0], "material": "Grass" }
        }),
        json!({
            "op": "set_terrain",
            "generator": { "kind": "hills", "amplitude": "NaN", "water_level": 0.0 }
        }),
        json!({
            "op": "set_terrain",
            "generator": { "kind": "hills" },
            "bounds_min": [10.0, 0.0, 0.0],
            "bounds_max": [10.0, 8.0, 16.0]
        }),
    ] {
        let (code, _, res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{pid}/scene/mutate"),
            Some(&json!({ "ops": [bad_op] })),
        );
        assert_eq!(code, 400, "expected 400 for bad terrain op {bad_op}: {res}");
    }

    // Corrupt/inverted terrain.json on disk -> scene/mutate returns 400 (not 500), workspace falls back cleanly
    write_file(
        &pdir.join("scene/terrain.json"),
        r#"{"enabled":true,"cell_size":4,"regions":[{"id":"bad","bounds_min":[10,0,0],"bounds_max":[5,8,16],"material":"Grass"}]}"#,
    );
    let (code_mut_bad, _, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        Some(&json!({ "ops": [{ "op": "upsert", "path": "Workspace.Box", "class_name": "Part" }] })),
    );
    assert_eq!(code_mut_bad, 400);
    let (code_ws_ok, _, ws_res) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/workspace"), None);
    assert_eq!(code_ws_ok, 200);
    assert_eq!(ws_res["terrain"]["enabled"], json!(false));
    let _ = std::fs::remove_file(pdir.join("scene/terrain.json"));

    // 2. Circular reparent cycles -> HTTP 400
    let (c_setup, _, setup_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        Some(&json!({
            "ops": [
                { "op": "upsert", "path": "Workspace.NodeA", "class_name": "Folder" },
                { "op": "upsert", "path": "Workspace.NodeA.NodeB", "class_name": "Folder" }
            ]
        })),
    );
    assert_eq!(c_setup, 200, "setup failed: {setup_res}");

    for (path, new_parent) in [
        ("Workspace.NodeA", "Workspace.NodeA"),
        ("Workspace.NodeA", "Workspace.NodeA.NodeB"),
        ("Workspace.NodeA.NodeB", "Workspace.NodeA.NodeB"),
    ] {
        let (code, _, res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{pid}/scene/mutate"),
            Some(&json!({ "ops": [{ "op": "reparent", "path": path, "new_parent": new_parent }] })),
        );
        assert_eq!(code, 400, "reparent {path} -> {new_parent} must return 400: {res}");
    }

    // Multi-op cycle in single request: A -> C, then C -> C.A -> HTTP 400 and atomic rollback
    let (c_batch, _, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        Some(&json!({
            "ops": [
                { "op": "upsert", "path": "Workspace.NodeC", "class_name": "Folder" },
                { "op": "reparent", "path": "Workspace.NodeA", "new_parent": "Workspace.NodeC" },
                { "op": "reparent", "path": "Workspace.NodeC", "new_parent": "Workspace.NodeC.NodeA" }
            ]
        })),
    );
    assert_eq!(c_batch, 400);

    // 3. mesh-asset traversal (%2e%2e, %252e%252e, %00, backslash, intermediate symlinks) -> 400
    let meshes_dir = pdir.join("assets/meshes");
    std::fs::create_dir_all(&meshes_dir).unwrap();
    write_file(&meshes_dir.join("ok.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, meshes_dir.join("mid_sym")).unwrap();
        std::os::unix::fs::symlink(&outside, pdir.join("art")).unwrap();
    }
    for bad in [
        "assets/meshes/%2e%2e/%2e%2e/outside/secret.obj",
        "assets/meshes/%252e%252e/%252e%252e/outside/secret.obj",
        "assets/meshes/ok.obj%00.png",
        "assets/meshes/ok%2500.obj",
        "assets/meshes%5c..%5c..%5coutside%5csecret.obj",
        "assets\\meshes\\ok.obj",
        #[cfg(unix)]
        "assets/meshes/mid_sym/secret.obj",
        #[cfg(unix)]
        "art/secret.obj",
    ] {
        let (code, _, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/mesh-asset?file={bad}"),
            None,
        );
        assert_eq!(code, 400, "mesh-asset ?file={bad} must return 400, got {code}");
    }
    let (code_ok, _, _) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/mesh-asset?file=assets/meshes/ok.obj"),
        None,
    );
    assert_eq!(code_ok, 200);
}

#[test]
fn e3_sandbox_retention_corrupt_latest_luau_edge_cases_and_fingerprint_gates() {
    assert!(find_lune_binary().0.is_some(), "lune is required for E3 hardening tests");
    let psp = project_system_prompt();
    assert!(psp.len() < 7_000, "project_system_prompt exceeded 7 KB cap: {}", psp.len());

    let root = scratch_dir("e3-sandbox");
    let _cleanup = DirCleanup(root.clone());
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    SecretStore::for_data_dir(&data)
        .set(OPENCLOUD_KEY, "test-opencloud-key-v126-123456")
        .unwrap();

    let (port, _shared) = start_server(&data);
    let host = format!("127.0.0.1:{port}");
    let (pid, pdir) = create_test_project(port, &ws, "Sandbox Hardening");

    // 1. Luau runtime edge cases:
    // - zero-length & NaN Workspace:Raycast directions
    // - degenerate / zero-size / negative GetPartBoundsInBox & GetPartBoundsInRadius
    // - negative & NaN TweenInfo durations (no hang, no NaN position)
    // - malformed http_mocks (non-tables, empty url/prefix, "*", non-numeric status, bad headers)
    //   while still blocking 100% of unmocked outbound HTTP requests!
    write_file(
        &pdir.join("src/server/EdgeCases.server.luau"),
        r#"local Workspace = game:GetService("Workspace")
local TweenService = game:GetService("TweenService")
local HttpService = game:GetService("HttpService")

local p = Instance.new("Part")
p.Name = "TargetBox"
p.Size = Vector3.new(6, 6, 6)
p.Position = Vector3.new(0, 0, 0)
p.Anchored = true
p.Parent = Workspace

-- Zero-length and NaN Raycast directions return nil without throwing
assert(Workspace:Raycast(Vector3.new(0, 0, 0), Vector3.new(0, 0, 0)) == nil, "zero ray")
assert(Workspace:Raycast(Vector3.new(0, 0, 0), Vector3.new(0/0, 0, 0)) == nil, "nan ray")
assert(Workspace:Raycast(Vector3.new(0/0, 0, 0), Vector3.new(1, 0, 0)) == nil, "nan origin ray")

-- Degenerate/zero-size GetPartBoundsInBox & GetPartBoundsInRadius return empty table
assert(#Workspace:GetPartBoundsInBox(CFrame.new(0, 0, 0), Vector3.new(0, 0, 0)) == 0, "zero box")
assert(#Workspace:GetPartBoundsInBox(CFrame.new(0, 0, 0), Vector3.new(2, 0, 2)) == 0, "flat box")
assert(#Workspace:GetPartBoundsInBox(CFrame.new(0, 0, 0), Vector3.new(-2, 2, 2)) == 0, "neg box")
assert(#Workspace:GetPartBoundsInBox(CFrame.new(0, 0, 0), Vector3.new(0/0, 2, 2)) == 0, "nan box")
assert(#Workspace:GetPartBoundsInRadius(Vector3.new(0, 0, 0), 0) == 0, "zero radius")
assert(#Workspace:GetPartBoundsInRadius(Vector3.new(0, 0, 0), -5) == 0, "neg radius")
assert(#Workspace:GetPartBoundsInRadius(Vector3.new(0, 0, 0), 0/0) == 0, "nan radius")

-- Negative and NaN tween durations complete immediately and never write NaN
local negDone = false
local twNeg = TweenService:Create(p, TweenInfo.new(-3), { Position = Vector3.new(5, 0, 0) })
twNeg.Completed:Connect(function() negDone = true end)
twNeg:Play()

local nanDone = false
local twNan = TweenService:Create(p, TweenInfo.new(0/0, Enum.EasingStyle.Linear, Enum.EasingDirection.Out, -1, false, 0/0), { Position = Vector3.new(8, 2, 0) })
twNan.Completed:Connect(function() nanDone = true end)
twNan:Play()

task.wait(0.05)
assert(negDone and nanDone, "negative/NaN tweens must complete")
assert(p.Position.X == 8 and p.Position.Y == 2, "tween target must remain finite")

-- Valid mock still works while malformed mocks are ignored and unmocked outbound HTTP is blocked
local okMock, mockBody = pcall(function()
	return HttpService:GetAsync("https://mock.openro.local/safe")
end)
assert(okMock and mockBody == "ok", "valid mock failed")

for _, badUrl in ipairs({
	"https://example.com/unmocked",
	"http://127.0.0.1:9/evil",
	"https://evil.invalid/wildcard-check"
}) do
	local okReal, errReal = pcall(function()
		return HttpService:GetAsync(badUrl)
	end)
	assert(not okReal and string.find(tostring(errReal), "blocked", 1, true), "unmocked HTTP must stay blocked: " .. badUrl)
end
"#,
    );
    write_file(
        &pdir.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "MainHud"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "Lbl"
lbl.Text = "OK"
lbl.Parent = sg
"#,
    );

    let rep = run_project_sandbox(
        &pdir,
        &json!({
            "timeout_secs": 15,
            "http_mocks": [
                "not_an_object",
                123,
                { "url": "", "body": "should_not_match_empty" },
                { "url_prefix": "", "body": "should_not_match_empty_prefix" },
                { "url": "*", "body": "should_not_match_star" },
                { "url": "https://mock.openro.local/safe", "status": "bad_status", "headers": "bad_headers", "body": "ok" }
            ]
        }),
    )
    .expect("sandbox run with edge cases");
    assert_eq!(rep["passed"], json!(true), "rep={rep}");
    assert_eq!(rep["error_count"], json!(0), "rep={rep}");

    // 2. Bounded sandbox run history retention (> 50 runs pruned to 50, preserving latest.json)
    let sb_dir = pdir.join(".openro/sandbox");
    for i in 1..=54u64 {
        write_file(
            &sb_dir.join(format!("sb-{}-1.json", 10_000 + i)),
            &json!({
                "run_id": format!("sb-{}-1", 10_000 + i),
                "at_ms": 10_000 + i,
                "passed": true,
                "error_count": 0,
                "core_loop": { "all_passed": true }
            })
            .to_string(),
        );
    }
    // Run one more real sandbox -> triggers pruning to 50 <run_id>.json files + preserves latest.json
    let (st_run, _, sb_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/sandbox/run"),
        Some(&json!({})),
    );
    assert_eq!(st_run, 200, "sb_res={sb_res}");
    assert!(sb_dir.join("latest.json").is_file());
    let run_files_count = std::fs::read_dir(&sb_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.ends_with(".json") && n != "latest.json"
        })
        .count();
    assert_eq!(run_files_count, 50, "expected 50 retained sandbox runs, got {run_files_count}");
    assert!(!sb_dir.join("sb-10001-1.json").exists(), "oldest sandbox run must be pruned");
    assert_eq!(list_sandbox_runs(&pdir).unwrap().len(), 50);

    // 3. Fingerprint invalidation across script, scene, and file edits on readiness & publish-gate
    let _ = std::fs::remove_file(pdir.join("src/server/EdgeCases.server.luau"));
    let (_, _, _) = http_req(port, &host, "POST", &format!("/api/projects/{pid}/autopilot"), Some(&json!({})));
    let fp0 = project_fingerprint(&pdir).unwrap();
    let (st_sb2, _, sb2) = http_req(port, &host, "POST", &format!("/api/projects/{pid}/sandbox/run"), Some(&json!({})));
    assert_eq!(st_sb2, 200, "{sb2}");
    let (st_r1, _, r1) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/readiness"), None);
    assert_eq!(st_r1, 200);
    assert_eq!(r1["verification_tier"], json!("headless_sandbox"), "r1={r1}");

    // Edit scene via scene/mutate -> fingerprint changes -> readiness/publish-gate & study_status invalidate
    let (st_sc, _, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        Some(&json!({ "ops": [{ "op": "upsert", "path": "Workspace.Marker", "class_name": "Part" }] })),
    );
    assert_eq!(st_sc, 200);
    let fp1 = project_fingerprint(&pdir).unwrap();
    assert_ne!(fp0, fp1);
    let (_, _, r_after_scene) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/readiness"), None);
    assert!(r_after_scene["verification_tier"].is_null(), "scene edit must invalidate sandbox tier: {r_after_scene}");
    let (_, _, pg_after_scene) = http_req(port, &host, "POST", &format!("/api/projects/{pid}/publish-gate"), Some(&json!({})));
    assert_eq!(pg_after_scene["gate_passed"], json!(false));
    let st_study = study_status(&pdir).unwrap();
    assert!(st_study["checks"]["verification_tier"].is_null());

    // Re-verify, then edit openro.project.json -> fingerprint changes again
    let (_, _, _) = http_req(port, &host, "POST", &format!("/api/projects/{pid}/sandbox/run"), Some(&json!({})));
    let manifest_text = std::fs::read_to_string(pdir.join("openro.project.json")).unwrap();
    let mut manifest: Value = serde_json::from_str(&manifest_text).unwrap();
    manifest["description"] = json!("edited-manifest");
    write_file(&pdir.join("openro.project.json"), &manifest.to_string());
    let fp2 = project_fingerprint(&pdir).unwrap();
    assert_ne!(fp1, fp2);
    let (_, _, r_after_file) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/readiness"), None);
    assert!(r_after_file["verification_tier"].is_null(), "manifest edit must invalidate sandbox tier");

    // 4. Corrupt/truncated latest.json recovery (no 500s on any endpoint)
    for corrupt in ["{truncated", "", "null", "[1,2,3]"] {
        write_file(&sb_dir.join("latest.json"), corrupt);
        assert_eq!(read_latest_sandbox_report(&pdir).unwrap(), None);
        let (st_lat, _, _) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/sandbox/latest"), None);
        assert_eq!(st_lat, 404, "corrupt latest.json should return 404, never 500");
        let (st_runs, _, _) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/sandbox/runs"), None);
        assert_eq!(st_runs, 200);
        let (st_rd, _, rd) = http_req(port, &host, "GET", &format!("/api/projects/{pid}/readiness"), None);
        assert_eq!(st_rd, 200);
        assert!(rd["verification_tier"].is_null());
        let (st_pg, _, pg) = http_req(port, &host, "POST", &format!("/api/projects/{pid}/publish-gate"), Some(&json!({})));
        assert_eq!(st_pg, 200);
        assert_eq!(pg["gate_passed"], json!(false));
    }
}

#[test]
fn e3_teamwork_last_sandbox_invalidation_after_edit() {
    assert!(find_lune_binary().0.is_some(), "lune is required");
    let root = scratch_dir("e3-teamwork-stale");
    let _cleanup = DirCleanup(root.clone());
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let pdir = ws.join("stale-tw");
    write_file(
        &pdir.join("default.project.json"),
        r#"{"name":"StaleTw","tree":{"$className":"DataModel","ServerScriptService":{"$className":"ServerScriptService","Server":{"$path":"src/server"}}}}"#,
    );
    write_file(
        &pdir.join("openro.project.json"),
        r#"{"version":1,"name":"StaleTw","rojoProject":"default.project.json"}"#,
    );
    write_file(&pdir.join("src/server/Main.server.luau"), "local x = 1\n");

    // Sequence:
    // 1. coordinator creates game_code task t1
    // 2. builder writes Main.server.luau, runs sandbox_playtest (sets last_sandbox), then edits Main.server.luau (MUST invalidate last_sandbox!), then submits!
    // 3. verifier tries verify_task -> MUST be refused because of edit after sandbox!
    // 4. verifier runs sandbox_playtest -> verify_task succeeds!
    let saw_refusal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_refusal_srv = Arc::clone(&saw_refusal);
    let llm = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in llm.incoming().flatten() {
            let saw_ref = Arc::clone(&saw_refusal_srv);
            std::thread::spawn(move || {
                let (_, body_bytes) = read_http_request(&mut s);
                let body_str = String::from_utf8_lossy(&body_bytes);
                if body_str.contains("Verifier gate refused") {
                    saw_ref.store(true, Ordering::SeqCst);
                }
                let reply_content = if body_str.contains("[Role: coordinator]") {
                    if !body_str.contains("tool_result (create_task)") && !body_str.contains("\"t1\"") {
                        json!({
                            "tool": "create_task",
                            "args": {
                                "title": "Build and test server",
                                "owner": "builder",
                                "verifier": "verifier",
                                "kind": "game_code",
                                "files": ["src/server/Main.server.luau"]
                            }
                        })
                    } else if body_str.contains("Reverified after edit") {
                        json!({ "tool": "finish_run", "args": { "summary": "done" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                } else if body_str.contains("[Role: verifier]") {
                    if !body_str.contains("Verifier gate refused") {
                        json!({
                            "tool": "verify_task",
                            "args": { "task_id": "t1", "passed": true, "notes": "stale attempt" }
                        })
                    } else if !body_str.contains("tool_result (sandbox_playtest)") {
                        json!({
                            "tool": "sandbox_playtest",
                            "args": { "task_id": "t1", "timeout_secs": 15 }
                        })
                    } else if !body_str.contains("Reverified after edit") {
                        json!({
                            "tool": "verify_task",
                            "args": { "task_id": "t1", "passed": true, "notes": "Reverified after edit" }
                        })
                    } else {
                        json!({ "done": true, "summary": "ok" })
                    }
                } else if !body_str.contains("tool_result (project_write_file)") {
                    json!({
                        "tool": "project_write_file",
                        "args": { "path": "src/server/Main.server.luau", "content": "local v = 1\n" }
                    })
                } else if !body_str.contains("tool_result (sandbox_playtest)") {
                    json!({
                        "tool": "sandbox_playtest",
                        "args": { "task_id": "t1", "timeout_secs": 15 }
                    })
                } else if !body_str.contains("tool_result (project_edit_file)") {
                    json!({
                        "tool": "project_edit_file",
                        "args": { "path": "src/server/Main.server.luau", "old": "local v = 1", "new": "local v = 2" }
                    })
                } else if !body_str.contains("tool_result (submit_task)") {
                    json!({
                        "tool": "submit_task",
                        "args": { "task_id": "t1", "summary": "edited after sandbox" }
                    })
                } else {
                    json!({ "done": true, "summary": "submitted" })
                };
                let resp_body = json!({
                    "choices": [{ "message": { "content": reply_content.to_string() } }]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });

    let llm_url = format!("http://127.0.0.1:{llm_port}/v1");
    let (port, _shared) = start_server_with_llm(&data, &llm_url);
    let host = format!("127.0.0.1:{port}");
    let (st_cfg, _, _) = http_req(
        port,
        &host,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": llm_url, "model_id": "stub" })),
    );
    assert_eq!(st_cfg, 200);
    let _ = http_req(port, &host, "POST", "/api/projects/workspace", Some(&json!({ "path": ws.to_string_lossy() })));
    let (_, _, opened) = http_req(port, &host, "POST", "/api/projects/open", Some(&json!({ "path": pdir.to_string_lossy() })));
    let pid = opened["project"]["id"].as_str().unwrap().to_string();

    let (st_tw, _, started) = http_req(
        port,
        &host,
        "POST",
        "/api/teamwork",
        Some(&json!({
            "goal": "Ensure edit after sandbox invalidates last_sandbox",
            "project_id": pid,
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert!(st_tw == 200 || st_tw == 201, "{started}");
    let run_id = started["run"]["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut final_state = Value::Null;
    while Instant::now() < deadline {
        let (_, _, st_res) = http_req(port, &host, "GET", &format!("/api/teamwork/{run_id}"), None);
        final_state = st_res;
        if matches!(
            final_state["run"]["status"].as_str().unwrap_or(""),
            "completed" | "failed" | "cancelled"
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    assert_eq!(final_state["run"]["status"], json!("completed"), "{final_state}");
    assert_eq!(
        final_state["tasks"][0]["verify_report"]["notes"],
        json!("Reverified after edit")
    );
    assert!(
        saw_refusal.load(Ordering::SeqCst),
        "expected Verifier gate refusal after post-sandbox edit"
    );
}
