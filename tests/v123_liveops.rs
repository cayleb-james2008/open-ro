//! v1.23.0 Live-Ops Control Plane & Real Player Telemetry integration tests:
//! - Open Cloud v2 DataStores (standard & ordered): list, get, optimistic concurrency,
//!   confirm: true gate, automatic pre-mutation backup, delete, and secret scrubbing
//! - MessagingService live topic publish (confirm: true gate + 1024-byte cap)
//! - Remote Config validation (<= 64 KiB, valid Luau identifier keys), deterministic
//!   `src/shared/LiveConfig.luau` codegen, and optional `push_to_datastore`
//! - Telemetry JSONL ingest + summary (DAU sample, funnel drop-off, ARPDAU Robux, sinks, alerts)
//!   and `AnalyticsService` real event capture in headless Lune sandbox
//! - Agent & Teamwork `liveops_*` tools (confirm: true owner gate on live mutations)
//! - Loopback-403 & CWE-22 / CWE-59 symlink guards.

use openro_rs::agent::Agent;
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
        .join("openro-v123-liveops")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start_server(data_dir: &Path, agent: Option<Agent>) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "LiveOps Demo".to_string(), data_dir.to_path_buf());
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
        proj_dir.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();
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
            "universeId": 99887766u64,
            "placeId": 11223344u64,
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

#[derive(Default)]
struct MockOpenCloudState {
    entries: BTreeMap<String, (Value, String)>,
    writes: usize,
    published: Vec<(String, String)>,
    secret_to_leak: Option<String>,
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

fn spawn_mock_opencloud() -> (String, Arc<Mutex<MockOpenCloudState>>) {
    let state = Arc::new(Mutex::new(MockOpenCloudState::default()));
    {
        let mut st = state.lock().unwrap();
        st.entries.insert(
            "standard:PlayerData:global:p_1001".to_string(),
            (json!({ "coins": 250, "level": 4 }), "rev-1".to_string()),
        );
        st.entries.insert(
            "standard:PlayerData:global:p_1002".to_string(),
            (json!({ "coins": 900, "level": 9 }), "rev-7".to_string()),
        );
        st.entries.insert(
            "ordered:TopWins:global:p_1001".to_string(),
            (json!(12), "ord-1".to_string()),
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let state_srv = Arc::clone(&state);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (head, body_bytes) = read_http_request(&mut stream);
            let body_bytes = body_bytes.as_slice();
            let first = head.lines().next().unwrap_or("");
            let parts: Vec<&str> = first.split_whitespace().collect();
            let method = *parts.first().unwrap_or(&"GET");
            let full_path = *parts.get(1).unwrap_or(&"/");
            let (path, query) = full_path.split_once('?').unwrap_or((full_path, ""));

            let (code, resp_json) = {
                let mut st = state_srv.lock().unwrap();
                if path.ends_with(":publishMessage") && method == "POST" {
                    let v: Value = serde_json::from_slice(body_bytes).unwrap_or(json!({}));
                    let topic = v["topic"].as_str().unwrap_or("").to_string();
                    let msg = v["message"].as_str().unwrap_or("").to_string();
                    st.published.push((topic, msg));
                    ("200 OK", json!({}))
                } else if path.contains("/standard-data-stores") && !path.contains("/scopes/") {
                    ("200 OK", json!({ "standardDataStores": [{ "id": "PlayerData" }, { "id": "OpenRO_RemoteConfig" }] }))
                } else if path.contains("/ordered-data-stores") && !path.contains("/scopes/") {
                    ("200 OK", json!({ "orderedDataStores": [{ "id": "TopWins" }] }))
                } else if path.ends_with("/entries") && method == "GET" {
                    let is_ordered = path.contains("/ordered-data-stores/");
                    let kind_prefix = if is_ordered { "ordered:" } else { "standard:" };
                    let segs: Vec<&str> = path.split('/').collect();
                    let store = segs.get(6).copied().unwrap_or("");
                    let scope = segs.get(8).copied().unwrap_or("global");
                    let match_prefix = format!("{kind_prefix}{store}:{scope}:");
                    let prefix_filter = query
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("prefix="))
                        .unwrap_or("");
                    let mut list = Vec::new();
                    for (k, (val, rev)) in &st.entries {
                        if let Some(entry_key) = k.strip_prefix(&match_prefix) {
                            if prefix_filter.is_empty() || entry_key.starts_with(prefix_filter) {
                                list.push(json!({
                                    "id": entry_key,
                                    "value": val,
                                    "revisionId": rev,
                                    "revisionCreateTime": "2026-11-26T12:00:00Z",
                                }));
                            }
                        }
                    }
                    let key_name = if is_ordered { "orderedDataStoreEntries" } else { "standardDataStoreEntries" };
                    ("200 OK", json!({ key_name: list }))
                } else if path.contains("/entries/") {
                    let is_ordered = path.contains("/ordered-data-stores/");
                    let kind_prefix = if is_ordered { "ordered:" } else { "standard:" };
                    let segs: Vec<&str> = path.split('/').collect();
                    let store = segs.get(6).copied().unwrap_or("");
                    let scope = segs.get(8).copied().unwrap_or("global");
                    let entry_key = segs.get(10).copied().unwrap_or("");
                    let full_key = format!("{kind_prefix}{store}:{scope}:{entry_key}");

                    if entry_key == "secret_leak_test" {
                        let leak = st.secret_to_leak.clone().unwrap_or_default();
                        ("403 Forbidden", json!({ "message": format!("Permission denied for key {leak}") }))
                    } else if method == "GET" {
                        match st.entries.get(&full_key) {
                            Some((val, rev)) => (
                                "200 OK",
                                json!({
                                    "id": entry_key,
                                    "value": val,
                                    "revisionId": rev,
                                    "revisionCreateTime": "2026-11-26T12:00:00Z",
                                }),
                            ),
                            None => ("404 Not Found", json!({ "message": "Entry not found" })),
                        }
                    } else if method == "PATCH" {
                        let v: Value = serde_json::from_slice(body_bytes).unwrap_or(json!({}));
                        let new_val = v.get("value").cloned().unwrap_or(Value::Null);
                        st.writes += 1;
                        let next_rev = format!("rev-{}", st.writes + 1);
                        st.entries.insert(full_key, (new_val.clone(), next_rev.clone()));
                        (
                            "200 OK",
                            json!({
                                "id": entry_key,
                                "value": new_val,
                                "revisionId": next_rev,
                                "revisionCreateTime": "2026-11-26T12:05:00Z",
                            }),
                        )
                    } else if method == "DELETE" {
                        st.writes += 1;
                        if st.entries.remove(&full_key).is_some() {
                            ("200 OK", json!({}))
                        } else {
                            ("404 Not Found", json!({ "message": "Entry not found" }))
                        }
                    } else {
                        ("405 Method Not Allowed", json!({ "message": "bad method" }))
                    }
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
fn liveops_datastores_optimistic_concurrency_backup_confirm_and_secret_scrub() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("datastores");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let (mock_base, mock_state) = spawn_mock_opencloud();
    let api_secret = "SECRET_LIVEOPS_KEY_998877";
    mock_state.lock().unwrap().secret_to_leak = Some(api_secret.to_string());

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
        ("OPENRO_OPENCLOUD_POLL_MS", "1"),
    ]);
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "ds-game");

    let (st_k, _) = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": api_secret })),
    );
    assert_eq!(st_k, 200);

    // Scopes check
    let (st_oc, oc) = http_req(port, &host, "GET", "/api/opencloud", None);
    assert_eq!(st_oc, 200);
    let scopes: Vec<&str> = oc["scopes_needed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(scopes.contains(&"universe-datastores.objects:read"));
    assert!(scopes.contains(&"universe-datastores.objects:write"));
    assert!(scopes.contains(&"universe.messaging-service:publish"));

    // List stores & entries
    let (st_stores, stores_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/datastores"),
        None,
    );
    assert_eq!(st_stores, 200, "{stores_res}");
    assert!(stores_res["stores"].as_array().unwrap().contains(&json!("PlayerData")));

    let (st_entries, entries_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/datastores?store=PlayerData&scope=global&prefix=p_1001"),
        None,
    );
    assert_eq!(st_entries, 200, "{entries_res}");
    assert_eq!(entries_res["entries"].as_array().unwrap().len(), 1);
    assert_eq!(entries_res["entries"][0]["revision"], "rev-1");

    // Get entry
    let (st_get, get_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry?store=PlayerData&scope=global&key=p_1001"),
        None,
    );
    assert_eq!(st_get, 200, "{get_res}");
    assert_eq!(get_res["entry"]["value"]["coins"], 250);
    assert_eq!(get_res["entry"]["revision"], "rev-1");

    // Confirm: true required on POST
    let (st_noc, noc_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "store": "PlayerData",
            "scope": "global",
            "key": "p_1001",
            "value": { "coins": 500, "level": 4 },
            "expected_revision": "rev-1"
        })),
    );
    assert_eq!(st_noc, 400, "{noc_res}");
    assert_eq!(mock_state.lock().unwrap().writes, 0);

    // Optimistic concurrency: stale revision -> 409 Conflict
    let (st_stale, stale_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "store": "PlayerData",
            "scope": "global",
            "key": "p_1001",
            "value": { "coins": 500, "level": 4 },
            "expected_revision": "rev-0",
            "confirm": true
        })),
    );
    assert_eq!(st_stale, 409, "{stale_res}");
    assert_eq!(mock_state.lock().unwrap().writes, 0);

    // Matching revision + confirm: true -> 200 + backup saved
    let (st_set, set_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "store": "PlayerData",
            "scope": "global",
            "key": "p_1001",
            "value": { "coins": 500, "level": 5 },
            "expected_revision": "rev-1",
            "confirm": true
        })),
    );
    assert_eq!(st_set, 200, "{set_res}");
    assert_eq!(set_res["entry"]["value"]["coins"], 500);
    let new_rev = set_res["entry"]["revision"].as_str().unwrap().to_string();
    let backup_rel = set_res["backup_path"].as_str().unwrap();
    assert!(backup_rel.starts_with(".openro/liveops/ds-backups/"));
    let backup_val: Value =
        serde_json::from_str(&std::fs::read_to_string(proj_dir.join(backup_rel)).unwrap()).unwrap();
    assert_eq!(backup_val["action"], "set");
    assert_eq!(backup_val["previous"]["value"]["coins"], 250);
    assert_eq!(backup_val["previous"]["revision"], "rev-1");

    // Ordered DataStore enforces integer value
    let (st_ord_bad, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "kind": "ordered",
            "store": "TopWins",
            "key": "p_1001",
            "value": { "not": "an int" },
            "expected_revision": "ord-1",
            "confirm": true
        })),
    );
    assert_eq!(st_ord_bad, 400);

    let (st_ord_ok, ord_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "kind": "ordered",
            "store": "TopWins",
            "key": "p_1001",
            "value": 15,
            "expected_revision": "ord-1",
            "confirm": true
        })),
    );
    assert_eq!(st_ord_ok, 200, "{ord_res}");
    assert_eq!(ord_res["entry"]["value"], 15);

    // Delete requires confirm: true + writes pre-delete backup
    let (st_del_no, _) = http_req(
        port,
        &host,
        "DELETE",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({ "store": "PlayerData", "key": "p_1001" })),
    );
    assert_eq!(st_del_no, 400);

    let (st_del_ok, del_res) = http_req(
        port,
        &host,
        "DELETE",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry"),
        Some(&json!({
            "store": "PlayerData",
            "scope": "global",
            "key": "p_1001",
            "expected_revision": new_rev,
            "confirm": true
        })),
    );
    assert_eq!(st_del_ok, 200, "{del_res}");
    assert_eq!(del_res["deleted"], true);
    let del_backup_rel = del_res["backup_path"].as_str().unwrap();
    let del_backup: Value =
        serde_json::from_str(&std::fs::read_to_string(proj_dir.join(del_backup_rel)).unwrap())
            .unwrap();
    assert_eq!(del_backup["action"], "delete");
    assert_eq!(del_backup["previous"]["value"]["coins"], 500);

    // Secrets scrub check when Open Cloud leaks API key in error response
    let (st_leak, leak_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/datastores/entry?store=PlayerData&key=secret_leak_test"),
        None,
    );
    assert_eq!(st_leak, 502);
    let leak_str = leak_res.to_string();
    assert!(!leak_str.contains(api_secret), "raw secret leaked: {leak_str}");
    assert!(leak_str.contains("••••"), "expected scrubbed dots: {leak_str}");
}

#[test]
fn liveops_messaging_publish_enforces_confirm_and_payload_limits() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("messaging");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let (mock_base, mock_state) = spawn_mock_opencloud();

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
    ]);
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, _) = create_test_project(port, &root, "msg-game");
    let _ = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "valid_msg_key_12345" })),
    );

    // Missing confirm -> 400
    let (st_no, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/messaging/publish"),
        Some(&json!({ "topic": "DoubleXP", "message": { "mult": 2 } })),
    );
    assert_eq!(st_no, 400);
    assert!(mock_state.lock().unwrap().published.is_empty());

    // Oversized message (> 1024 bytes) -> 400
    let big = "x".repeat(1200);
    let (st_big, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/messaging/publish"),
        Some(&json!({ "topic": "DoubleXP", "message": big, "confirm": true })),
    );
    assert_eq!(st_big, 400);

    // Valid confirmed broadcast -> 200
    let (st_ok, ok_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/messaging/publish"),
        Some(&json!({
            "topic": "DoubleXP",
            "message": { "multiplier": 2, "durationSec": 1800 },
            "confirm": true
        })),
    );
    assert_eq!(st_ok, 200, "{ok_res}");
    assert_eq!(ok_res["published"], true);
    assert_eq!(ok_res["topic"], "DoubleXP");
    assert_eq!(mock_state.lock().unwrap().published.len(), 1);
}

#[test]
fn liveops_remote_config_validation_codegen_and_optional_datastore_push() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("remote-config");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let (mock_base, mock_state) = spawn_mock_opencloud();

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
    ]);
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "rc-game");
    let _ = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "valid_rc_key_123456" })),
    );

    // Reject invalid Luau identifiers
    for bad_key in ["2fast", "drop-rate", "local", "return"] {
        let (st_bad, bad_res) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/remote-config"),
            Some(&json!({ "config": { (bad_key): 1 } })),
        );
        assert_eq!(st_bad, 400, "key {bad_key} should fail: {bad_res}");
    }

    // Reject > 64 KiB payload
    let huge = "a".repeat(66 * 1024);
    let (st_huge, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/remote-config"),
        Some(&json!({ "config": { "HugeText": huge } })),
    );
    assert_eq!(st_huge, 400);

    // Valid save + deterministic Luau codegen + push_to_datastore
    let body = json!({
        "config": {
            "ZetaDropRate": 0.25,
            "AlphaBanner": "Summer Event",
            "MaxLobbyPlayers": 16,
            "DoubleXpActive": true
        },
        "generate_luau": true,
        "push_to_datastore": true
    });
    let (st_save1, res1) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/remote-config"),
        Some(&body),
    );
    assert_eq!(st_save1, 200, "{res1}");
    assert_eq!(res1["luau_written"], true);
    assert_eq!(res1["datastore_pushed"], true);
    let luau_1 = std::fs::read_to_string(proj_dir.join("src/shared/LiveConfig.luau")).unwrap();
    assert!(luau_1.contains("--!strict"));
    assert!(luau_1.contains("DataStoreService:GetDataStore"));
    assert!(luau_1.contains("MessagingService:SubscribeAsync"));

    // Second save produces byte-for-byte identical LiveConfig.luau
    let (st_save2, _) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/remote-config"),
        Some(&json!({
            "config": {
                "DoubleXpActive": true,
                "MaxLobbyPlayers": 16,
                "ZetaDropRate": 0.25,
                "AlphaBanner": "Summer Event"
            },
            "generate_luau": true,
            "push_to_datastore": false
        })),
    );
    assert_eq!(st_save2, 200);
    let luau_2 = std::fs::read_to_string(proj_dir.join("src/shared/LiveConfig.luau")).unwrap();
    assert_eq!(luau_1, luau_2, "LiveConfig.luau codegen must be deterministic");

    if let Some(stylua) = openro_rs::projects::find_tool("stylua", "OPENRO_STYLUA") {
        let out = std::process::Command::new(&stylua)
            .current_dir(&proj_dir)
            .args(["--check", "--", "src/shared/LiveConfig.luau"])
            .output()
            .expect("run stylua");
        assert!(
            out.status.success(),
            "stylua failed on LiveConfig.luau:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let (st_get, get_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/remote-config"),
        None,
    );
    assert_eq!(st_get, 200, "{get_res}");
    assert_eq!(get_res["luau_exists"], true);
    assert_eq!(get_res["config"]["AlphaBanner"], "Summer Event");
    assert!(
        mock_state
            .lock()
            .unwrap()
            .entries
            .contains_key("standard:OpenRO_RemoteConfig:global:config")
    );
}

#[test]
fn liveops_telemetry_ingest_summary_and_sandbox_analytics_capture() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("telemetry");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "tel-game");

    // Ingest realistic cohort events
    let (st_in, in_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/ingest"),
        Some(&json!({
            "events": [
                { "kind": "onboarding", "user_id": "u1", "step": "Spawned" },
                { "kind": "onboarding", "user_id": "u2", "step": "Spawned" },
                { "kind": "onboarding", "user_id": "u3", "step": "Spawned" },
                { "kind": "onboarding", "user_id": "u4", "step": "Spawned" },
                { "kind": "funnel", "user_id": "u1", "step": "FirstQuest" },
                { "kind": "economy", "user_id": "u1", "step": "StarterBundle", "currency": "Robux", "amount": 200, "flow_type": "sink" },
                { "kind": "economy", "user_id": "u2", "step": "SwordUpgrade", "currency": "Coins", "amount": 500, "flow_type": "sink" },
                { "kind": "custom", "user_id": "u1", "step": "BossDefeated" }
            ]
        })),
    );
    assert_eq!(st_in, 200, "{in_res}");
    assert_eq!(in_res["ingested"], 8);
    assert!(proj_dir.join(".openro/liveops/telemetry.jsonl").is_file());

    let (st_sum, sum_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/telemetry/summary"),
        None,
    );
    assert_eq!(st_sum, 200, "{sum_res}");
    assert_eq!(sum_res["dau_sample"], 4);
    assert_eq!(sum_res["arpdau_robux"], 50.0); // 200 Robux / 4 DAU
    let steps = sum_res["funnel_steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["step"], "Spawned");
    assert_eq!(steps[0]["users"], 4);
    assert_eq!(steps[1]["step"], "FirstQuest");
    assert_eq!(steps[1]["users"], 1);
    assert_eq!(steps[1]["drop_off_pct"], 75.0);
    assert!(!sum_res["alerts"].as_array().unwrap().is_empty());
    assert_eq!(sum_res["top_sinks"][0]["label"], "SwordUpgrade");
    assert_eq!(sum_res["top_sinks"][0]["amount"], 500.0);

    // Also verify AnalyticsService + LiveConfig in headless Lune sandbox if lune is available
    if find_lune_binary().0.is_some() {
        let _ = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/remote-config"),
            Some(&json!({
                "config": { "DropMultiplier": 3 },
                "generate_luau": true,
                "push_to_datastore": false
            })),
        );
        std::fs::write(
            proj_dir.join("src/server/AnalyticsTest.server.luau"),
            r#"
local AnalyticsService = game:GetService("AnalyticsService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local LiveConfig = require(ReplicatedStorage.Shared.LiveConfig)

assert(LiveConfig.Get("DropMultiplier") == 3, "expected LiveConfig default 3")

Players.PlayerAdded:Connect(function(player)
    AnalyticsService:LogOnboardingFunnelStepEvent(player, 1, "TutorialStart")
    AnalyticsService:LogFunnelStepEvent(player, "Shop", "sess_1", 2, "OpenedShop")
    AnalyticsService:LogEconomyEvent(player, Enum.AnalyticsEconomyFlowType.Sink, "Robux", 99, 100, "IAP", "VipPass")
    AnalyticsService:LogCustomEvent(player, "WaveCleared", 5)
end)
"#,
        )
        .unwrap();

        let report = run_project_sandbox(&proj_dir, &json!({ "timeout_secs": 15 })).unwrap();
        assert_eq!(report["passed"], true, "sandbox report failed: {report}");
        let captured = report["telemetry_events"].as_array().unwrap();
        assert!(
            captured.len() >= 4,
            "expected >=4 AnalyticsService telemetry events, got: {report}"
        );
        assert!(captured.iter().any(|e| e["kind"] == "onboarding" && e["step"] == "TutorialStart"));
        assert!(captured.iter().any(|e| e["kind"] == "funnel" && e["step"] == "OpenedShop"));
        assert!(captured.iter().any(|e| e["kind"] == "economy" && e["currency"] == "Robux" && e["amount"] == 99));
        assert!(captured.iter().any(|e| e["kind"] == "custom" && e["event_name"] == "WaveCleared"));

        // Ingest captured sandbox events directly into liveops telemetry
        let (st_cap, _) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/ingest"),
            Some(&json!({ "events": captured })),
        );
        assert_eq!(st_cap, 200);
    }
}

#[test]
fn liveops_cwe22_traversal_and_loopback403_guards() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("security");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_test_project(port, &root, "sec-game");

    // 1. Non-loopback Host must return 403 Forbidden on every liveops endpoint
    let evil_host = format!("attacker.example:{port}");
    for (method, suffix) in [
        ("GET", "datastores"),
        ("GET", "datastores/entry?store=A&key=B"),
        ("POST", "datastores/entry"),
        ("DELETE", "datastores/entry"),
        ("POST", "messaging/publish"),
        ("GET", "remote-config"),
        ("POST", "remote-config"),
        ("POST", "telemetry/ingest"),
        ("GET", "telemetry/summary"),
    ] {
        let body = if method == "GET" { None } else { Some(json!({})) };
        let (st, _) = http_req(
            port,
            &evil_host,
            method,
            &format!("/api/projects/{proj_id}/liveops/{suffix}"),
            body.as_ref(),
        );
        assert_eq!(st, 403, "{method} liveops/{suffix} must enforce loopback-403");
    }

    // 2. CWE-59 symlink escape guard on remote-config.json and telemetry.jsonl
    #[cfg(unix)]
    {
        let outside = root.join("outside.txt");
        std::fs::write(&outside, "DO_NOT_TOUCH").unwrap();
        std::fs::create_dir_all(proj_dir.join("liveops")).unwrap();
        std::os::unix::fs::symlink(&outside, proj_dir.join("liveops/remote-config.json")).unwrap();
        let (st_sym_rc, _) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/remote-config"),
            Some(&json!({ "config": { "TestFlag": true } })),
        );
        assert_eq!(st_sym_rc, 400);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "DO_NOT_TOUCH");

        std::fs::create_dir_all(proj_dir.join(".openro/liveops")).unwrap();
        std::os::unix::fs::symlink(&outside, proj_dir.join(".openro/liveops/telemetry.jsonl")).unwrap();
        let (st_sym_tel, _) = http_req(
            port,
            &host,
            "POST",
            &format!("/api/projects/{proj_id}/liveops/telemetry/ingest"),
            Some(&json!({ "events": [{ "kind": "custom", "user_id": "1" }] })),
        );
        assert_eq!(st_sym_tel, 400);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "DO_NOT_TOUCH");
    }
}

#[test]
fn liveops_agent_tools_enforce_confirm_gate_and_execute_cleanly() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("agent-tools");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let (mock_base, mock_state) = spawn_mock_opencloud();

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
                        { "tool": "liveops_datastore_list", "args": { "store": "PlayerData", "scope": "global" } },
                        { "tool": "liveops_datastore_get", "args": { "store": "PlayerData", "scope": "global", "key": "p_1001" } },
                        { "tool": "liveops_telemetry_summary", "args": {} }
                    ]
                })
                .to_string(),
                1 => json!({
                    "tool": "liveops_datastore_set",
                    "args": { "store": "PlayerData", "key": "p_1001", "value": { "coins": 999 }, "expected_revision": "rev-1" }
                })
                .to_string(),
                2 => json!({
                    "tool": "liveops_messaging_publish",
                    "args": { "topic": "EventStart", "message": { "active": true } }
                })
                .to_string(),
                3 => json!({
                    "tool": "liveops_remote_config_push",
                    "args": { "config": { "XpRate": 2 }, "push_to_datastore": true }
                })
                .to_string(),
                4 => json!({
                    "steps": [
                        { "tool": "liveops_datastore_set", "args": { "store": "PlayerData", "key": "p_1001", "value": { "coins": 777 }, "expected_revision": "rev-1", "confirm": true } },
                        { "tool": "liveops_messaging_publish", "args": { "topic": "EventStart", "message": { "active": true }, "confirm": true } },
                        { "tool": "liveops_remote_config_push", "args": { "config": { "XpRate": 2 }, "generate_luau": true, "push_to_datastore": false } }
                    ],
                    "done": true,
                    "summary": "Live-ops workflow complete."
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
    let agent = Agent::with_local_place(model, LocalPlace::open(data_dir.clone(), "LiveAgent".into()));
    let (port, shared) = start_server(&data_dir, Some(agent));
    let host = format!("127.0.0.1:{port}");
    let (_proj_id, proj_dir) = create_test_project(port, &root, "agent-liveops");
    {
        let mut store = shared.projects.lock().unwrap();
        let entry = store.open(proj_dir.to_str().unwrap()).unwrap();
        store.set_active(&entry.id);
    }
    let _ = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "agent_liveops_key_1234" })),
    );

    let (st, chat) = http_req(
        port,
        &host,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "Inspect and update live-ops config." })),
    );
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    let transcript = chat["transcript"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    assert!(transcript.contains("tool liveops_datastore_list ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_datastore_get ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_telemetry_summary ->"), "{transcript}");
    assert!(transcript.contains("confirm: true"), "unconfirmed liveops writes must block: {transcript}");
    assert!(transcript.contains("tool liveops_datastore_set ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_messaging_publish ->"), "{transcript}");
    assert!(transcript.contains("tool liveops_remote_config_push ->"), "{transcript}");
    assert!(proj_dir.join("liveops/remote-config.json").is_file());
    assert!(proj_dir.join("src/shared/LiveConfig.luau").is_file());
    assert_eq!(mock_state.lock().unwrap().writes, 1);
    assert_eq!(mock_state.lock().unwrap().published.len(), 1);
}
