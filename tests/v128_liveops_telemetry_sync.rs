//! v1.28.0 Live-Ops Telemetry Collector Codegen, Open Cloud Shard Sync & Live Error Aggregation tests:
//! - `POST /api/projects/:id/liveops/telemetry/codegen` (`opencloud_datastore` & `webhook_relay`),
//!   `--!strict` Luau static verification, HTTPS-only + SSRF `relay_url` rejection, interval bounds
//! - `POST /api/projects/:id/liveops/telemetry/sync` against loopback mock Open Cloud v2 DataStores
//!   & Creator Analytics, idempotent revision + per-event cursor in `telemetry-sync-state.json`,
//!   `source: "live_opencloud"` vs preserved `source: "sandbox"`
//! - `GET /api/projects/:id/liveops/errors` aggregation (`LiveErrorAggregate`) + `?source=` filtering
//! - Loopback-403 & CWE-59 symlink guards
//! - Agent `liveops_telemetry_codegen` and `liveops_telemetry_sync` tool execution.

use openro_rs::agent::Agent;
use openro_rs::luau_check::{analyze_place, analyze_script};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::sandbox::{find_lune_binary, run_project_sandbox};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("openro-v128-telemetry")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start_server(data_dir: &Path, agent: Option<Agent>) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "LiveOps Telemetry Demo".to_string(), data_dir.to_path_buf());
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
) -> (u16, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let raw_body = if sep + 4 <= buf.len() {
        &buf[sep + 4..]
    } else {
        &[]
    };
    let parsed = serde_json::from_slice(raw_body)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(raw_body).to_string() }));
    (status, parsed)
}

fn create_test_project(port: u16, root: &Path, slug: &str) -> (String, PathBuf) {
    let proj_dir = root.join(slug);
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/client")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": slug,
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": {
                    "$className": "ReplicatedStorage",
                    "Shared": { "$path": "src/shared" }
                },
                "ServerScriptService": {
                    "$className": "ServerScriptService",
                    "Server": { "$path": "src/server" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("openro.project.json"),
        json!({
            "name": slug,
            "universeId": 77665544u64,
            "placeId": 33445566u64,
            "rojoProject": "default.project.json",
        })
        .to_string(),
    )
    .unwrap();
    let (st, opened) = http_req(
        port,
        &format!("127.0.0.1:{port}"),
        "POST",
        "/api/projects/open",
        Some(&json!({ "path": proj_dir.to_str().unwrap() })),
    );
    assert_eq!(st, 200, "open project failed: {opened}");
    let id = opened["project"]["id"].as_str().unwrap().to_string();
    (id, proj_dir)
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

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[derive(Default)]
struct MockTelemetryOpenCloud {
    shards: BTreeMap<String, (Value, String)>,
    creator_analytics_events: Vec<Value>,
    list_calls: usize,
    omit_values_in_list: bool,
}

fn spawn_mock_telemetry_opencloud(
    initial: MockTelemetryOpenCloud,
) -> (String, Arc<Mutex<MockTelemetryOpenCloud>>) {
    let state = Arc::new(Mutex::new(initial));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let state_srv = Arc::clone(&state);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (head, _body) = read_http_request(&mut stream);
            let first = head.lines().next().unwrap_or("");
            let parts: Vec<&str> = first.split_whitespace().collect();
            let method = *parts.first().unwrap_or(&"GET");
            let full_path = *parts.get(1).unwrap_or(&"/");
            let (path, _query) = full_path.split_once('?').unwrap_or((full_path, ""));

            let (code, resp) = {
                let mut st = state_srv.lock().unwrap();
                if method == "GET" && path.contains("/analytics/events") {
                    ("200 OK", json!({ "events": st.creator_analytics_events }))
                } else if method == "GET" && path.ends_with("/entries") {
                    st.list_calls += 1;
                    let segs: Vec<&str> = path.split('/').collect();
                    let store = percent_decode(segs.get(6).copied().unwrap_or(""));
                    let prefix = format!("{store}:");
                    let mut list = Vec::new();
                    for (k, (val, rev)) in &st.shards {
                        if let Some(entry_key) = k.strip_prefix(&prefix) {
                            if st.omit_values_in_list {
                                list.push(json!({
                                    "id": entry_key,
                                    "revisionId": rev,
                                }));
                            } else {
                                list.push(json!({
                                    "id": entry_key,
                                    "value": val,
                                    "revisionId": rev,
                                }));
                            }
                        }
                    }
                    if list.is_empty() && store == "MissingStore" {
                        ("404 Not Found", json!({ "message": "DataStore not found" }))
                    } else {
                        ("200 OK", json!({ "standardDataStoreEntries": list }))
                    }
                } else if method == "GET" && path.contains("/entries/") {
                    let segs: Vec<&str> = path.split('/').collect();
                    let store = percent_decode(segs.get(6).copied().unwrap_or(""));
                    let entry_key = percent_decode(segs.get(10).copied().unwrap_or(""));
                    let full_key = format!("{store}:{entry_key}");
                    match st.shards.get(&full_key) {
                        Some((val, rev)) => (
                            "200 OK",
                            json!({
                                "id": entry_key,
                                "value": val,
                                "revisionId": rev,
                            }),
                        ),
                        None => ("404 Not Found", json!({ "message": "Entry not found" })),
                    }
                } else {
                    ("404 Not Found", json!({ "message": "not found" }))
                }
            };

            let body_str = resp.to_string();
            let reply = format!(
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
                body_str.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}"), state)
}

#[test]
fn telemetry_codegen_validates_ssrf_bounds_and_emits_strict_luau() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("codegen");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "codegen-game");

    // 1. Default opencloud_datastore mode
    let (st, res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
        Some(&json!({
            "mode": "opencloud_datastore",
            "flush_interval_secs": 120
        })),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["ok"], true);
    assert_eq!(res["mode"], "opencloud_datastore");
    assert_eq!(res["store_name"], "OpenRO_LiveTelemetry");
    assert_eq!(res["flush_interval_secs"], 120);
    let files = res["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);

    let shared_path = proj_dir.join("src/shared/LiveTelemetry.luau");
    let flush_path = proj_dir.join("src/server/LiveTelemetryFlush.server.luau");
    assert!(shared_path.is_file());
    assert!(flush_path.is_file());

    let shared_src = std::fs::read_to_string(&shared_path).unwrap();
    let flush_src = std::fs::read_to_string(&flush_path).unwrap();
    assert!(shared_src.starts_with("--!strict"));
    assert!(shared_src.contains("MAX_RING_BUFFER"));
    assert!(flush_src.starts_with("--!strict"));
    assert!(flush_src.contains("BindToClose"));
    assert!(flush_src.contains("pcall("));
    assert!(flush_src.contains("bucket:%04d-%02d-%02d:%02d"));
    assert!(flush_src.contains("OpenRO_LiveTelemetry"));
    assert!(!flush_src.contains("x-api-key"));
    assert!(!flush_src.contains("Bearer "));

    let rep_shared = analyze_script("ReplicatedStorage.Shared.LiveTelemetry", &shared_src);
    assert!(rep_shared.diagnostics.is_empty(), "{:?}", rep_shared.diagnostics);
    let rep_flush = analyze_script("ServerScriptService.Server.LiveTelemetryFlush", &flush_src);
    assert!(rep_flush.diagnostics.is_empty(), "{:?}", rep_flush.diagnostics);
    let mut place_scripts = BTreeMap::new();
    place_scripts.insert("ReplicatedStorage.Shared.LiveTelemetry".to_string(), shared_src);
    place_scripts.insert("ServerScriptService.Server.LiveTelemetryFlush".to_string(), flush_src);
    let rep_proj = analyze_place(&place_scripts, &BTreeMap::new());
    assert!(rep_proj.diagnostics.is_empty(), "place luau_check diagnostics: {:?}", rep_proj.diagnostics);

    // 2. Webhook relay mode with valid HTTPS URL
    let (st_wh, res_wh) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
        Some(&json!({
            "mode": "webhook_relay",
            "flush_interval_secs": 3600,
            "relay_url": "https://telemetry.example.com/v1/events"
        })),
    );
    assert_eq!(st_wh, 200, "{res_wh}");
    assert_eq!(res_wh["mode"], "webhook_relay");
    let flush_wh_src = std::fs::read_to_string(&flush_path).unwrap();
    assert!(flush_wh_src.starts_with("--!strict"));
    assert!(flush_wh_src.contains("https://telemetry.example.com/v1/events"));
    assert!(flush_wh_src.contains("HttpService:RequestAsync"));
    let rep_wh = analyze_script("ServerScriptService.Server.LiveTelemetryFlush", &flush_wh_src);
    assert!(rep_wh.diagnostics.is_empty(), "{:?}", rep_wh.diagnostics);

    // 3. Reject non-HTTPS & SSRF relay_url targets (400)
    for bad_url in [
        "http://telemetry.example.com/events",
        "https://127.0.0.1/hook",
        "https://localhost:8443/hook",
        "https://169.254.169.254/latest/meta-data",
        "https://10.0.0.1/hook",
        "https://192.168.1.50/hook",
        "https://[::1]/hook",
        "https://user:secret@telemetry.example.com/hook",
        "https://telemetry.example.com/hook?token=leak",
    ] {
        let (code, err_res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
            Some(&json!({
                "mode": "webhook_relay",
                "relay_url": bad_url
            })),
        );
        assert_eq!(code, 400, "expected 400 for relay_url '{bad_url}', got {code}: {err_res}");
    }

    // Missing relay_url in webhook_relay mode -> 400
    let (st_missing, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
        Some(&json!({ "mode": "webhook_relay" })),
    );
    assert_eq!(st_missing, 400);

    // 4. Bounds checks on flush_interval_secs (60..=3600)
    for bad_interval in [json!(0), json!(59), json!(3601), json!(-10), json!(60.5), json!("fast")] {
        let (code, err_res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
            Some(&json!({
                "mode": "opencloud_datastore",
                "flush_interval_secs": bad_interval
            })),
        );
        assert_eq!(code, 400, "expected 400 for flush_interval_secs={bad_interval}: {err_res}");
    }

    // 5. Loopback-403 on new liveops telemetry routes
    let evil_host = format!("evil.example:{port}");
    for (method, suffix) in [
        ("POST", "telemetry/codegen"),
        ("POST", "telemetry/sync"),
        ("GET", "errors"),
    ] {
        let body = if method == "GET" { None } else { Some(json!({})) };
        let (code, _) = http_req(
            port,
            &evil_host,
            method,
            &format!("/api/projects/{proj_id}/liveops/{suffix}"),
            body.as_ref(),
        );
        assert_eq!(code, 403, "{method} {suffix} must enforce loopback-403");
    }

    // 6. CWE-59 symlink guard on codegen target
    #[cfg(unix)]
    {
        let outside = root.join("outside.luau");
        std::fs::write(&outside, "DO_NOT_OVERWRITE").unwrap();
        std::fs::remove_file(&shared_path).unwrap();
        std::os::unix::fs::symlink(&outside, &shared_path).unwrap();
        let (code, _) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
            Some(&json!({ "mode": "opencloud_datastore" })),
        );
        assert_eq!(code, 400);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "DO_NOT_OVERWRITE");
        std::fs::remove_file(&shared_path).unwrap();
    }

    // 7. Regenerate opencloud_datastore mode and verify in headless Lune sandbox if available
    if find_lune_binary().0.is_some() {
        let (st_regen, _) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/codegen"),
            Some(&json!({ "mode": "opencloud_datastore", "flush_interval_secs": 60 })),
        );
        assert_eq!(st_regen, 200);
        std::fs::write(
            proj_dir.join("src/server/TelemetrySandboxProbe.server.luau"),
            r#"
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local LiveTelemetry = require(ReplicatedStorage.Shared.LiveTelemetry)

for i = 1, 205 do
    LiveTelemetry.LogCustom(nil, "Tick_" .. tostring(i))
end
assert(LiveTelemetry.PeekCount() == 200, "ring buffer must cap at 200")
assert(LiveTelemetry.DroppedCount() == 5, "expected 5 dropped events")

Players.PlayerAdded:Connect(function(player)
    LiveTelemetry.LogOnboarding(player, "Spawned")
    LiveTelemetry.LogError("src/server/Shop.server.luau", "attempt to index nil", 7)
end)
"#,
        )
        .unwrap();
        let report = run_project_sandbox(&proj_dir, &json!({ "timeout_secs": 15 })).unwrap();
        assert_eq!(report["passed"], true, "sandbox failed: {report}");
        let ds_snap = report
            .pointer("/state_snapshot/datastores")
            .cloned()
            .unwrap_or(json!({}));
        let snap_text = ds_snap.to_string();
        assert!(snap_text.contains("OpenRO_LiveTelemetry"), "expected DataStore flush in sandbox: {report}");
        assert!(snap_text.contains("bucket:"), "expected hourly bucket key in sandbox: {report}");
    }
}

#[test]
fn telemetry_sync_is_idempotent_preserves_sources_and_aggregates_live_errors() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("sync-errors");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");

    let mut initial = MockTelemetryOpenCloud {
        omit_values_in_list: true,
        ..Default::default()
    };
    initial.shards.insert(
        "OpenRO_LiveTelemetry:bucket:2026-11-26:17".to_string(),
        (
            json!({
                "bucket": "bucket:2026-11-26:17",
                "events": [
                    { "kind": "onboarding", "user_id": "live_u1", "step": "Spawned", "ts": "2026-11-26T17:01:00Z" },
                    { "kind": "economy", "user_id": "live_u1", "currency": "Robux", "amount": 150, "flow_type": "sink", "step": "VipPass", "ts": "2026-11-26T17:02:00Z" },
                    { "kind": "error", "error": "attempt to index nil with 'Coins'", "script_path": "src/server/Shop.server.luau", "version_number": 42, "ts": "2026-11-26T17:03:00Z" },
                    { "kind": "error", "error": "attempt to index nil with 'Coins'", "script_path": "src/server/Shop.server.luau", "version_number": 42, "ts": "2026-11-26T17:09:00Z" }
                ]
            }),
            "rev-shard-1".to_string(),
        ),
    );
    initial.shards.insert(
        "OpenRO_LiveTelemetry:bucket:2026-11-26:18".to_string(),
        (
            json!({
                "bucket": "bucket:2026-11-26:18",
                "events": [
                    { "kind": "error", "error": "Workspace.BossArena:WaitForChild timed out", "script_path": "src/client/Hud.client.luau", "version_number": 42, "ts": "2026-11-26T18:05:00Z" }
                ]
            }),
            "rev-shard-2".to_string(),
        ),
    );
    initial.creator_analytics_events = vec![
        json!({
            "kind": "custom",
            "user_id": "ca_u9",
            "step": "DailyBonusClaimed",
            "ts": "2026-11-26T18:10:00Z"
        }),
    ];

    let (mock_base, mock_state) = spawn_mock_telemetry_opencloud(initial);
    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
    ]);

    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "sync-game");

    let (st_k, _) = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "telemetry_test_key_1234" })),
    );
    assert_eq!(st_k, 200);

    // 1. Ingest local sandbox events first -> preserved as source: "sandbox"
    let (st_sb, sb_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/ingest"),
        Some(&json!({
            "events": [
                { "kind": "onboarding", "user_id": "sb_1", "step": "Spawned", "ts": "2026-11-26T16:00:00Z" },
                { "kind": "error", "error": "Sandbox local warning", "script_path": "src/server/Test.server.luau", "ts": "2026-11-26T16:01:00Z" }
            ]
        })),
    );
    assert_eq!(st_sb, 200, "{sb_res}");

    // 2. First sync from Open Cloud DataStore -> imports 5 live events
    let (st_sync1, sync1) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 7 })),
    );
    assert_eq!(st_sync1, 200, "{sync1}");
    assert_eq!(sync1["ok"], true);
    assert_eq!(sync1["imported_count"], 5);
    assert_eq!(sync1["sync_state"]["shards_tracked"], 2);
    assert_eq!(sync1["sync_state"]["total_imported_count"], 5);
    assert!(proj_dir.join(".openro/liveops/telemetry-sync-state.json").is_file());

    // Verify JSONL contains both source:"sandbox" and source:"live_opencloud"
    let jsonl = std::fs::read_to_string(proj_dir.join(".openro/liveops/telemetry.jsonl")).unwrap();
    let lines: Vec<Value> = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 7);
    assert_eq!(
        lines.iter().filter(|e| e["source"] == "sandbox").count(),
        2
    );
    assert_eq!(
        lines.iter().filter(|e| e["source"] == "live_opencloud").count(),
        5
    );

    // 3. Re-sync immediately with unchanged revisions -> imported_count == 0, no duplicates
    let (st_sync2, sync2) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 7 })),
    );
    assert_eq!(st_sync2, 200, "{sync2}");
    assert_eq!(sync2["imported_count"], 0);
    assert_eq!(sync2["sync_state"]["total_imported_count"], 5);

    // 4. Append 1 new error event to existing shard 18 & bump revision -> imports ONLY the 1 new event
    {
        let mut st = mock_state.lock().unwrap();
        st.shards.insert(
            "OpenRO_LiveTelemetry:bucket:2026-11-26:18".to_string(),
            (
                json!({
                    "bucket": "bucket:2026-11-26:18",
                    "events": [
                        { "kind": "error", "error": "Workspace.BossArena:WaitForChild timed out", "script_path": "src/client/Hud.client.luau", "version_number": 42, "ts": "2026-11-26T18:05:00Z" },
                        { "kind": "error", "error": "attempt to index nil with 'Coins'", "script_path": "src/server/Shop.server.luau", "version_number": 43, "ts": "2026-11-26T18:20:00Z" }
                    ]
                }),
                "rev-shard-2b".to_string(),
            ),
        );
    }
    let (st_sync3, sync3) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 7 })),
    );
    assert_eq!(st_sync3, 200, "{sync3}");
    assert_eq!(sync3["imported_count"], 1);
    assert_eq!(sync3["sync_state"]["total_imported_count"], 6);

    // 5. GET /liveops/errors aggregation & ?source= filtering
    let (st_err_live, err_live) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/errors?source=live_opencloud"),
        None,
    );
    assert_eq!(st_err_live, 200, "{err_live}");
    assert_eq!(err_live["source"], "live_opencloud");
    assert_eq!(err_live["total_error_events"], 4);
    let errs = err_live["errors"].as_array().unwrap();
    assert_eq!(errs.len(), 2);
    assert_eq!(errs[0]["script_path"], "src/server/Shop.server.luau");
    assert_eq!(errs[0]["count"], 3);
    assert_eq!(errs[0]["first_seen"], "2026-11-26T17:03:00Z");
    assert_eq!(errs[0]["last_seen"], "2026-11-26T18:20:00Z");
    assert!(errs[0]["fingerprint"].as_str().unwrap().starts_with("err_"));
    assert_eq!(errs[1]["script_path"], "src/client/Hud.client.luau");
    assert_eq!(errs[1]["count"], 1);

    let (st_err_sb, err_sb) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/errors?source=sandbox"),
        None,
    );
    assert_eq!(st_err_sb, 200, "{err_sb}");
    assert_eq!(err_sb["total_error_events"], 1);
    assert_eq!(err_sb["errors"][0]["message"], "Sandbox local warning");

    let (st_sum_live, sum_live) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/telemetry/summary?source=live_opencloud"),
        None,
    );
    assert_eq!(st_sum_live, 200, "{sum_live}");
    assert_eq!(sum_live["total_events"], 6);
    assert_eq!(sum_live["dau_sample"], 1);
    assert_eq!(sum_live["arpdau_robux"], 150.0);

    let (st_bad_src, _) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/errors?source=bogus"),
        None,
    );
    assert_eq!(st_bad_src, 400);

    // 6. Creator Analytics sync + days bounds
    let (st_ca, ca_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "creator_analytics", "days": 7 })),
    );
    assert_eq!(st_ca, 200, "{ca_res}");
    assert_eq!(ca_res["imported_count"], 1);

    let (st_bad_days, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 0 })),
    );
    assert_eq!(st_bad_days, 400);
}

#[test]
fn telemetry_agent_tools_execute_codegen_and_sync_cleanly() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("agent-telemetry");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");

    let mut initial = MockTelemetryOpenCloud::default();
    initial.shards.insert(
        "OpenRO_LiveTelemetry:bucket:2026-11-26:19".to_string(),
        (
            json!({
                "events": [
                    { "kind": "onboarding", "user_id": "ag_1", "step": "TutorialDone", "ts": "2026-11-26T19:00:00Z" },
                    { "kind": "error", "error": "DataStore queue full", "script_path": "src/server/Save.server.luau", "version_number": 5, "ts": "2026-11-26T19:01:00Z" }
                ]
            }),
            "rev-agent-1".to_string(),
        ),
    );
    let (mock_base, _mock_state) = spawn_mock_telemetry_opencloud(initial);

    let turn = Arc::new(AtomicUsize::new(0));
    let turn_c = Arc::clone(&turn);
    let llm_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in llm_listener.incoming().flatten() {
            let _ = read_http_request(&mut stream);
            let n = turn_c.fetch_add(1, Ordering::SeqCst);
            let content = match n {
                0 => json!({
                    "steps": [
                        {
                            "tool": "liveops_telemetry_codegen",
                            "args": { "mode": "opencloud_datastore", "store_name": "OpenRO_LiveTelemetry", "flush_interval_secs": 90 }
                        },
                        {
                            "tool": "liveops_telemetry_sync",
                            "args": { "source": "opencloud_datastore", "store_name": "OpenRO_LiveTelemetry", "days": 7 }
                        },
                        {
                            "tool": "liveops_telemetry_summary",
                            "args": { "source": "live_opencloud" }
                        }
                    ],
                    "done": true,
                    "summary": "Installed LiveTelemetry collector and synced live errors."
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
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
    ]);

    let model = ModelClient::new(format!("http://127.0.0.1:{llm_port}/v1"), "mock".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data_dir.clone(), "TelAgent".into()));
    let (port, shared) = start_server(&data_dir, Some(agent));
    let host = format!("127.0.0.1:{port}");
    let (_proj_id, proj_dir) = create_test_project(port, &root, "agent-tel");
    {
        let mut store = shared.projects.lock().unwrap();
        let entry = store.open(proj_dir.to_str().unwrap()).unwrap();
        store.set_active(&entry.id);
    }
    let (st_k, _) = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "agent_telemetry_key_99" })),
    );
    assert_eq!(st_k, 200);

    let (st, chat) = http_req(
        port,
        &host,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "Generate telemetry collector and sync production shards." })),
    );
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    let transcript = chat["transcript"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    assert!(transcript.contains("tool liveops_telemetry_codegen ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_telemetry_sync ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_telemetry_summary ->"), "{transcript}");
    assert!(proj_dir.join("src/shared/LiveTelemetry.luau").is_file());
    assert!(proj_dir.join("src/server/LiveTelemetryFlush.server.luau").is_file());
    assert!(proj_dir.join(".openro/liveops/telemetry-sync-state.json").is_file());
}
