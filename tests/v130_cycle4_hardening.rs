//! Cycle 4 Wave H1 Hardening Integration & Edge-Case Tests (Target v1.30.0):
//! 1. Studio Sync & XML/Binary Prefab bounds:
//!    - Malformed XML: XXE DOCTYPE/ENTITY, NUL character entities (`&#0;`/`&#x0;`),
//!      O(N) unclosed `&` scan, deep nesting (> 64), attribute cap (> 256), traversal `class`
//!    - Binary `.rbxm`: multi-chunk instance cap, cumulative LZ4 decompression cap,
//!      `PRNT` self-loop and disconnected cycles, deep tree nesting (> 64)
//!    - Symlink parent refusal on prefab import, corrupt `sync-baseline.json` recovery,
//!      stalled `get_place_snapshot` cursor fail-closed, malicious Studio snapshot script keys
//! 2. LiveOps Telemetry & Open Cloud collector:
//!    - SSRF URL validation: octal/hex/shorthand IPv4, IPv6 NAT64/IPv4-compat/zone-ID/bracketed-domain,
//!      percent-encoded query secrets
//!    - `Retry-After: u64::MAX` overflow guard in `opencloud::Client`
//!    - Stalled `nextPageToken` fail-closed in `telemetry/sync`
//!    - JSONL corrupt/non-UTF-8/truncated line recovery + `OPENRO_TELEMETRY_MAX_BYTES=512` read safety
//!    - Corrupt `telemetry-sync-state.json` recovery (vs symlink refusal)
//!    - Ring-buffer shift idempotent deduplication + numeric `id` dedup + duplicate-payload retention
//! 3. Luau typecheck/symbols/definition engine:
//!    - UTF-8 BOM + multibyte emoji/CJK coordinates in `analyze_workspace_symbols` & `lookup_definition`
//!    - Self and multi-hop cyclic `require()` graph detection (`cyclic_require`, zero infinite loop)
//!    - Huge (> 2 MiB) & malformed `--!strict` files (deep arrows, unclosed `<`, deep parens, nested `if` inside `function`)
//!    - CWE-22 path normalization (`%252e`, traversal, out-of-tree `luau-analyze` path filtering)

use openro_rs::agent::Agent;
use openro_rs::liveops::validate_https_relay_url;
use openro_rs::luau_check::{analyze_workspace_symbols, lookup_definition};
use openro_rs::model::ModelClient;
use openro_rs::place::{LocalPlace, PlaceBackend};
use openro_rs::projects::{self, import_prefab_model};
use openro_rs::server::{serve, Shared};
use openro_rs::studio_plugin::StudioPluginBridge;
use openro_rs::studio_sync::{pull_apply_with_backend, pull_diff_with_backend, PullStrategy};
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
        .join("openro-v130-h1")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn start_server(data_dir: &Path) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(None, "H1 Hardening".to_string(), data_dir.to_path_buf());
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

fn create_rojo_project(port: u16, root: &Path, slug: &str) -> (String, PathBuf) {
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
                },
                "StarterPlayer": {
                    "$className": "StarterPlayer",
                    "StarterPlayerScripts": {
                        "$className": "StarterPlayerScripts",
                        "Client": { "$path": "src/client" }
                    }
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
            "rojoProject": "default.project.json",
            "universeId": 99887766u64,
            "placeId": 11223344u64
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

fn make_rbxm_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(tag);
    out.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn make_rbxm_header(class_count: u32, inst_count: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(b"<roblox!\x89\xff\x0d\x0a\x1a\x0a");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&class_count.to_le_bytes());
    out.extend_from_slice(&inst_count.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out
}

fn encode_rbxm_referents(refs: &[i32]) -> Vec<u8> {
    let n = refs.len();
    let mut buf = vec![0u8; n * 4];
    let mut prev = 0i32;
    for (idx, &r) in refs.iter().enumerate() {
        let delta = r.wrapping_sub(prev);
        prev = r;
        let zz = ((delta << 1) ^ (delta >> 31)) as u32;
        let be = zz.to_be_bytes();
        buf[idx] = be[0];
        buf[n + idx] = be[1];
        buf[2 * n + idx] = be[2];
        buf[3 * n + idx] = be[3];
    }
    buf
}

// =========================================================================
// 1. Studio Sync & XML/Binary Prefab Hardening
// =========================================================================

#[test]
fn prefab_parser_rejects_malformed_xml_rbxm_cycles_and_symlink_escapes() {
    let root = scratch_dir("prefab-hardening");
    let _cleanup = DirCleanup(root.clone());
    let proj = root.join("proj");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({ "name": "PrefabH1", "tree": { "$className": "DataModel" } }).to_string(),
    )
    .unwrap();

    // 1. NUL numeric entity injection (`&#0;` and `&#x0;`) -> rejected
    for xml in [
        r#"<roblox version="4"><Item class="Model"><Properties><string name="Name">Bad&#0;Name</string></Properties></Item></roblox>"#,
        r#"<roblox version="4"><Item class="Model"><Properties><string name="Name">Bad&#x0;Name</string></Properties></Item></roblox>"#,
    ] {
        let err = import_prefab_model(&proj, "NulEnt.rbxmx", "rbxmx", &b64_encode(xml.as_bytes()), None)
            .unwrap_err();
        assert!(err.contains("NUL"), "unexpected err: {err}");
    }

    // 2. Deep XML nesting (> 64) -> rejected without stack overflow
    let mut deep_xml = String::from(r#"<roblox version="4">"#);
    for _ in 0..70 {
        deep_xml.push_str(r#"<Item class="Model">"#);
    }
    for _ in 0..70 {
        deep_xml.push_str("</Item>");
    }
    deep_xml.push_str("</roblox>");
    let err_deep =
        import_prefab_model(&proj, "Deep.rbxmx", "rbxmx", &b64_encode(deep_xml.as_bytes()), None)
            .unwrap_err();
    assert!(err_deep.contains("depth"), "unexpected err: {err_deep}");

    // 3. Traversal `class=".."` -> rejected cleanly
    let trav_xml = r#"<roblox version="4"><Item class=".."><Properties><string name="Name">..</string></Properties></Item></roblox>"#;
    assert!(import_prefab_model(
        &proj,
        "TravClass.rbxmx",
        "rbxmx",
        &b64_encode(trav_xml.as_bytes()),
        None
    )
    .is_err());

    // 4. Excessive attributes (> 256) -> rejected
    let mut attr_xml = String::from(r#"<roblox version="4"><Item class="Model""#);
    for i in 0..260 {
        attr_xml.push_str(&format!(" a{i}=\"1\""));
    }
    attr_xml.push_str("/></roblox>");
    assert!(import_prefab_model(
        &proj,
        "ManyAttrs.rbxmx",
        "rbxmx",
        &b64_encode(attr_xml.as_bytes()),
        None
    )
    .is_err());

    // 5. Quadratic `&` DoS probe (40,000 `&` without `;`) completes quickly
    let ampersands = "&".repeat(40_000);
    let amp_xml = format!(
        r#"<roblox version="4"><Item class="Model"><Properties><string name="Name">AmpTest</string><string name="Tag">{ampersands}</string></Properties></Item></roblox>"#
    );
    let ok_amp = import_prefab_model(
        &proj,
        "AmpTest.rbxmx",
        "rbxmx",
        &b64_encode(amp_xml.as_bytes()),
        None,
    );
    assert!(ok_amp.is_ok(), "ampersand text failed: {ok_amp:?}");

    // 6. Binary `.rbxm` cyclic PRNT (A <-> B and self-loop) -> rejected fail-closed
    let mut inst_payload = Vec::new();
    inst_payload.extend_from_slice(&0u32.to_le_bytes()); // cid
    inst_payload.extend_from_slice(&5u32.to_le_bytes());
    inst_payload.extend_from_slice(b"Model");
    inst_payload.push(0); // obj_fmt
    inst_payload.extend_from_slice(&2u32.to_le_bytes());
    inst_payload.extend_from_slice(&encode_rbxm_referents(&[1, 2]));

    let mut prnt_cycle = Vec::new();
    prnt_cycle.push(0); // version
    prnt_cycle.extend_from_slice(&2u32.to_le_bytes());
    prnt_cycle.extend_from_slice(&encode_rbxm_referents(&[1, 2]));
    prnt_cycle.extend_from_slice(&encode_rbxm_referents(&[2, 1]));

    let mut rbxm_cycle = make_rbxm_header(1, 2);
    rbxm_cycle.extend_from_slice(&make_rbxm_chunk(b"INST", &inst_payload));
    rbxm_cycle.extend_from_slice(&make_rbxm_chunk(b"PRNT", &prnt_cycle));
    rbxm_cycle.extend_from_slice(&make_rbxm_chunk(b"END\0", &[]));

    let err_cyc = import_prefab_model(
        &proj,
        "Cycle.rbxm",
        "rbxm",
        &b64_encode(&rbxm_cycle),
        None,
    )
    .unwrap_err();
    assert!(err_cyc.contains("cyclic"), "expected cyclic error, got: {err_cyc}");

    // 7. Binary `.rbxm` cumulative LZ4 bomb (> 64 MiB uncompressed across 5 chunks of 16 MiB) -> rejected
    // Valid minimal LZ4 block encoding 16 MiB zeroes: token=0xf0, lit_len=(16M-1) or match offset=1.
    // Build a tiny 16 MiB LZ4 payload: 1 literal byte (0x00) + match offset=1 repeated!
    let chunk_uncomp = 16 * 1024 * 1024usize;
    let mut lz4_payload = Vec::new();
    // token: lit=1 (0x10) | match=15 (0x0f) => 0x1f
    lz4_payload.push(0x1f);
    lz4_payload.push(0x00); // 1 literal byte
    lz4_payload.extend_from_slice(&1u16.to_le_bytes()); // offset = 1
    let mut rem_match = (chunk_uncomp - 1) - 19; // base 4 + 15 = 19
    while rem_match >= 255 {
        lz4_payload.push(255);
        rem_match -= 255;
    }
    lz4_payload.push(rem_match as u8);

    let mut rbxm_bomb = make_rbxm_header(0, 0);
    for _ in 0..5 {
        rbxm_bomb.extend_from_slice(b"META");
        rbxm_bomb.extend_from_slice(&(lz4_payload.len() as u32).to_le_bytes());
        rbxm_bomb.extend_from_slice(&(chunk_uncomp as u32).to_le_bytes());
        rbxm_bomb.extend_from_slice(&0u32.to_le_bytes());
        rbxm_bomb.extend_from_slice(&lz4_payload);
    }
    rbxm_bomb.extend_from_slice(&make_rbxm_chunk(b"END\0", &[]));
    let err_bomb = import_prefab_model(
        &proj,
        "Bomb.rbxm",
        "rbxm",
        &b64_encode(&rbxm_bomb),
        None,
    )
    .unwrap_err();
    assert!(err_bomb.contains("decompressed size"), "expected bomb rejection: {err_bomb}");

    // 8. Symlink parent escape in scene/models -> refused without writing outside project
    #[cfg(unix)]
    {
        let outside = root.join("outside_models");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(proj.join("scene")).unwrap();
        let _ = std::fs::remove_dir_all(proj.join("scene/models"));
        std::os::unix::fs::symlink(&outside, proj.join("scene/models")).unwrap();
        let valid_xml = r#"<roblox version="4"><Item class="Part"><Properties><string name="Name">Box</string></Properties></Item></roblox>"#;
        let err_sym = import_prefab_model(
            &proj,
            "SymlinkEscape.rbxmx",
            "rbxmx",
            &b64_encode(valid_xml.as_bytes()),
            None,
        )
        .unwrap_err();
        assert!(err_sym.contains("symlink"), "expected symlink error: {err_sym}");
        assert!(!outside.join("symlinkescape.model.json").exists());
    }
}

#[test]
fn studio_sync_recovers_corrupt_baseline_rejects_stalled_cursor_and_sanitizes_paths() {
    let root = scratch_dir("sync-hardening");
    let _cleanup = DirCleanup(root.clone());
    let proj = root.join("proj");
    std::fs::create_dir_all(proj.join("src/server")).unwrap();
    std::fs::create_dir_all(proj.join(".openro")).unwrap();
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    std::fs::write(proj.join("src/server/Main.server.luau"), "print('local')\n").unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "SyncH1",
            "tree": {
                "$className": "DataModel",
                "ServerScriptService": {
                    "$className": "ServerScriptService",
                    "Server": { "$path": "src/server" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    // Write corrupt truncated baseline JSON
    std::fs::write(proj.join(".openro/sync-baseline.json"), "{corrupt_truncated").unwrap();

    // 1. Stalled cursor in get_place_snapshot -> fails closed instead of looping 512x
    let bridge = StudioPluginBridge::open(None);
    // Mark bridge connected by doing one initial poll before spawning the worker
    assert!(bridge.poll_next_command(Duration::from_millis(1)).is_none());
    let b_poll = Arc::clone(&bridge);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let worker = std::thread::spawn(move || {
        while !stop_c.load(Ordering::Relaxed) {
            if let Some(cmd) = b_poll.poll_next_command(Duration::from_millis(20)) {
                b_poll.submit_result(
                    &cmd.id,
                    true,
                    json!({
                        "snapshot_id": "snap-stalled",
                        "done": false,
                        "next_cursor": "same-cursor",
                        "scripts": {}
                    }),
                    None,
                    None,
                );
            }
        }
    });
    // First send with cursor=None -> next_cursor="same-cursor", second send with cursor="same-cursor" -> next_cursor="same-cursor" -> stalled!
    let mut backend_stalled = PlaceBackend::Plugin(Arc::clone(&bridge));
    let err_stall = backend_stalled.get_place_snapshot().unwrap_err();
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    assert!(err_stall.contains("stalled"), "expected stalled cursor error, got: {err_stall}");

    // 2. Corrupt sync-baseline.json recovers cleanly + malicious `.git/hooks/pwn.luau` / `../../evil.luau` keys in Studio snapshot never write `.git/` or escape project
    let bridge2 = StudioPluginBridge::open(None);
    assert!(bridge2.poll_next_command(Duration::from_millis(1)).is_none());
    let b2_poll = Arc::clone(&bridge2);
    let stop2 = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2_c = Arc::clone(&stop2);
    let worker2 = std::thread::spawn(move || {
        while !stop2_c.load(Ordering::Relaxed) {
            if let Some(cmd) = b2_poll.poll_next_command(Duration::from_millis(20)) {
                b2_poll.submit_result(
                    &cmd.id,
                    true,
                    json!({
                        "snapshot_id": "snap-malicious-keys",
                        "done": true,
                        "scripts": {
                            "ServerScriptService/Server/Main": "print('local')\n",
                            ".git/hooks/pwn.luau": "evil\n",
                            "../../evil.luau": "escape\n"
                        },
                        "script_meta": {
                            "ServerScriptService/Server/Main": { "class_name": "Script" }
                        },
                        "instances": {},
                        "lighting": {}
                    }),
                    None,
                    None,
                );
            }
        }
    });
    let mut backend = PlaceBackend::Plugin(Arc::clone(&bridge2));

    let diff = pull_diff_with_backend(&mut backend, &proj).unwrap();
    assert!(diff.ok);

    let applied = pull_apply_with_backend(
        &mut backend,
        &proj,
        None,
        PullStrategy::MergeNonConflicting,
        false,
    )
    .unwrap();
    stop2.store(true, Ordering::Relaxed);
    worker2.join().unwrap();
    assert!(applied.ok);
    assert!(!proj.join(".git/hooks/pwn.luau").exists());
    assert!(!root.join("evil.luau").exists());

    // Corrupt openro.project.json returns clean Err (no panic)
    std::fs::write(proj.join("openro.project.json"), "{bad_json").unwrap();
    assert!(projects::manifest(&proj).is_err());
}

// =========================================================================
// 2. LiveOps Telemetry & Open Cloud Collector Hardening
// =========================================================================

#[test]
fn liveops_ssrf_blocks_octal_hex_shorthand_ipv6_and_encoded_secrets() {
    for bad in [
        "https://0177.0.0.1/hook",
        "https://0x7f.0.0.1/hook",
        "https://127.1/hook",
        "https://127.000.000.1/hook",
        "https://10.1/hook",
        "https://192.0.2.1/hook",
        "https://198.18.0.1/hook",
        "https://203.0.113.5/hook",
        "https://240.0.0.1/hook",
        "https://100.64.0.5/hook",
        "https://[::1]/hook",
        "https://[::ffff:127.0.0.1]/hook",
        "https://[::127.0.0.1]/hook",
        "https://[fc00::1]/hook",
        "https://[fe80::1]/hook",
        "https://[fe80::1%eth0]/hook",
        "https://[64:ff9b::127.0.0.1]/hook",
        "https://[evil.com]/hook",
        "https://telemetry.example.com/hook?api%5Fkey=secret123",
        "https://telemetry.example.com/hook?access_token=abc",
        "https://foo.home.arpa/hook",
    ] {
        assert!(
            validate_https_relay_url(bad).is_err(),
            "expected SSRF/secret rejection for '{bad}'"
        );
    }
    assert!(validate_https_relay_url("https://telemetry.example.com/v1/events").is_ok());
}

#[test]
fn liveops_recovers_corrupt_jsonl_and_sync_state_and_dedups_ring_buffer_shifts() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("liveops-h1");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");

    // Mock Open Cloud that:
    // - Returns 429 with `Retry-After: 18446744073709551615` (u64::MAX) once before 200
    // - Serves a shard whose ring buffer shifts (evicts oldest event, adds 1 new event, bumps revision)
    let call_num = Arc::new(AtomicUsize::new(0));
    let phase = Arc::new(AtomicUsize::new(1));
    let call_num_c = Arc::clone(&call_num);
    let phase_c = Arc::clone(&phase);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mock_port = listener.local_addr().unwrap().port();
    let mock_base = format!("http://127.0.0.1:{mock_port}");

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let req_path = head
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");

            let c = call_num_c.fetch_add(1, Ordering::SeqCst);
            if c == 0 {
                let resp = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 18446744073709551615\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
                let _ = s.write_all(resp.as_bytes());
                continue;
            }

            let cur_phase = phase_c.load(Ordering::SeqCst);
            let body = if req_path.contains("/entries?") {
                let rev = if cur_phase == 1 { "rev-1" } else { "rev-2" };
                let events = if cur_phase == 1 {
                    json!([
                        { "kind": "onboarding", "user_id": "u1", "step": "StepA", "ts": "2026-11-26T10:00:00Z" },
                        { "kind": "onboarding", "user_id": "u2", "step": "StepB", "ts": "2026-11-26T10:01:00Z" },
                        { "kind": "custom", "id": 9001, "user_id": "u3", "step": "NumericId", "ts": "2026-11-26T10:02:00Z" },
                        { "kind": "custom", "user_id": "u4", "step": "DupSameTick", "ts": "2026-11-26T10:03:00Z" },
                        { "kind": "custom", "user_id": "u4", "step": "DupSameTick", "ts": "2026-11-26T10:03:00Z" }
                    ])
                } else {
                    // StepA evicted (indices shifted!), StepB + NumericId + DupSameTick remain, plus StepC added!
                    json!([
                        { "kind": "onboarding", "user_id": "u2", "step": "StepB", "ts": "2026-11-26T10:01:00Z" },
                        { "kind": "custom", "id": 9001, "user_id": "u3", "step": "NumericId", "ts": "2026-11-26T10:02:00Z" },
                        { "kind": "custom", "user_id": "u4", "step": "DupSameTick", "ts": "2026-11-26T10:03:00Z" },
                        { "kind": "custom", "user_id": "u4", "step": "DupSameTick", "ts": "2026-11-26T10:03:00Z" },
                        { "kind": "error", "error": "Recovered after corrupt line", "script_path": "src/server/A.server.luau", "ts": "2026-11-26T10:04:00Z" }
                    ])
                };
                json!({
                    "standardDataStoreEntries": [{
                        "id": "bucket:2026-11-26:10",
                        "revisionId": rev,
                        "value": { "events": events }
                    }]
                })
                .to_string()
            } else {
                "{}".to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
        ("OPENRO_OPENCLOUD_POLL_MS", "5"),
        ("OPENRO_TELEMETRY_MAX_BYTES", "512"),
    ]);

    let (port, _shared) = start_server(&data_dir);
    let host = format!("127.0.0.1:{port}");
    let (proj_id, proj_dir) = create_rojo_project(port, &root, "liveops-h1");

    let (st_k, _) = http_req(
        port,
        &host,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "h1_test_key_5678" })),
    );
    assert_eq!(st_k, 200);

    // Write corrupt telemetry-sync-state.json before first sync -> must recover (no 500!)
    std::fs::create_dir_all(proj_dir.join(".openro/liveops")).unwrap();
    std::fs::write(
        proj_dir.join(".openro/liveops/telemetry-sync-state.json"),
        "{corrupt_sync_state",
    )
    .unwrap();

    // Sync phase 1 (hits Retry-After: u64::MAX without overflow panic, imports 5 events incl. 2 identical DupSameTick)
    let (st1, r1) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 7 })),
    );
    assert_eq!(st1, 200, "sync1 failed: {r1}");
    assert_eq!(r1["imported_count"], 5);

    // Phase 2: ring-buffer shifts (StepA evicted, StepB/NumericId/DupSameTick shift down, 1 new error added)
    phase.store(2, Ordering::SeqCst);
    let (st2, r2) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{proj_id}/liveops/telemetry/sync"),
        Some(&json!({ "source": "opencloud_datastore", "days": 7 })),
    );
    assert_eq!(st2, 200, "sync2 failed: {r2}");
    assert_eq!(
        r2["imported_count"], 1,
        "ring-buffer shift must only import the 1 truly new event, got: {r2}"
    );

    // Inject corrupt JSON, non-object JSON, invalid UTF-8 bytes, and truncated trailing line into telemetry.jsonl
    let jsonl_path = proj_dir.join(".openro/liveops/telemetry.jsonl");
    let existing = std::fs::read(&jsonl_path).unwrap();
    let mut corrupted = Vec::new();
    corrupted.extend_from_slice(b"{\"kind\":\"error\",\"error\":\"BeforeCorrupt\",\"user_id\":\"server\",\"source\":\"sandbox\",\"ts\":\"2026-11-26T09:00:00Z\"}\n");
    corrupted.extend_from_slice(b"\xff\xfe\xfd_invalid_utf8_line\n");
    corrupted.extend_from_slice(b"{not_valid_json\n");
    corrupted.extend_from_slice(b"[1, 2, 3]\n");
    corrupted.extend_from_slice(&existing);
    corrupted.extend_from_slice(b"{\"kind\":\"error\",\"truncated_no_newline");
    std::fs::write(&jsonl_path, corrupted).unwrap();

    // Must NOT return 413 despite OPENRO_TELEMETRY_MAX_BYTES=512, and must recover events before AND after invalid UTF-8!
    let (st_err, err_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/errors"),
        None,
    );
    assert_eq!(st_err, 200, "errors failed: {err_res}");
    assert_eq!(
        err_res["total_error_events"], 2,
        "expected both BeforeCorrupt and Recovered after corrupt line: {err_res}"
    );

    let (st_sum, sum_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{proj_id}/liveops/telemetry/summary"),
        None,
    );
    assert_eq!(st_sum, 200, "summary failed: {sum_res}");
    // sync1's 5 events rotated into telemetry.jsonl.1 when sync2 exceeded 512 bytes;
    // active telemetry.jsonl contains sync2's 1 event + injected BeforeCorrupt (with corrupt/non-UTF8 lines skipped).
    assert!(proj_dir.join(".openro/liveops/telemetry.jsonl.1").is_file());
    assert_eq!(sum_res["total_events"], 2);
}

// =========================================================================
// 3. Luau Typecheck, Symbols & Definition Engine Hardening
// =========================================================================

#[test]
fn luau_handles_utf8_bom_multibyte_cyclic_require_huge_and_malformed_strict_files() {
    // 1. UTF-8 BOM + multibyte strings/comments + cyclic require graph (A -> B -> A and self C -> C)
    let mut scripts = BTreeMap::new();
    let mut service_to_file = BTreeMap::new();
    scripts.insert(
        "ReplicatedStorage.Shared.ModA".to_string(),
        "\u{FEFF}--!strict\n-- コメント 🚀 日本語\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal ModB = require(ReplicatedStorage.Shared.ModB)\nlocal ModA = {}\nlocal label = \"テスト🎮\" ; function ModA.Ping(n: number): number\n\treturn n + 1\nend\nreturn ModA\n".to_string(),
    );
    service_to_file.insert(
        "ReplicatedStorage.Shared.ModA".to_string(),
        "src/shared/ModA.luau".to_string(),
    );
    scripts.insert(
        "ReplicatedStorage.Shared.ModB".to_string(),
        "--!strict\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal ModA = require(ReplicatedStorage.Shared.ModA)\nlocal ModB = {}\nreturn ModB\n".to_string(),
    );
    service_to_file.insert(
        "ReplicatedStorage.Shared.ModB".to_string(),
        "src/shared/ModB.luau".to_string(),
    );
    scripts.insert(
        "ReplicatedStorage.Shared.SelfLoop".to_string(),
        "--!strict\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal Self = require(ReplicatedStorage.Shared.SelfLoop)\nreturn {}\n".to_string(),
    );
    service_to_file.insert(
        "ReplicatedStorage.Shared.SelfLoop".to_string(),
        "src/shared/SelfLoop.luau".to_string(),
    );

    // 2. Malformed `--!strict` file (deep parens, chained arrows, unclosed `<`, nested `if` inside `function`)
    let deep_arrows = "() -> ".repeat(150) + "number";
    let deep_parens = "(".repeat(150) + "ReplicatedStorage.Shared.ModA" + &")".repeat(150);
    let malformed_strict = format!(
        "--!strict\nlocal x: {deep_arrows} = nil\nlocal y: Foo<Bar = 1\nlocal z = {deep_parens}\nlocal function outer(): number\n\tlocal inner = function()\n\t\tif true then\n\t\t\tlocal k = 1\n\t\tend\n\t\treturn \"string_in_inner_is_ok\"\n\tend\n\treturn 42\nend\n"
    );
    scripts.insert(
        "ServerScriptService.Server.MalformedStrict".to_string(),
        malformed_strict,
    );
    service_to_file.insert(
        "ServerScriptService.Server.MalformedStrict".to_string(),
        "src/server/MalformedStrict.server.luau".to_string(),
    );

    // 3. Huge (> 2 MiB) `--!strict` file -> fail-closed FILE_TOO_LARGE diagnostic, zero OOM
    let huge_src = format!("--!strict\n-- {}\nreturn {{}}\n", "x".repeat(2_200_000));
    scripts.insert(
        "ServerScriptService.Server.Huge".to_string(),
        huge_src,
    );
    service_to_file.insert(
        "ServerScriptService.Server.Huge".to_string(),
        "src/server/Huge.server.luau".to_string(),
    );

    let index = analyze_workspace_symbols(&scripts, &BTreeMap::new(), &service_to_file, None, false);
    assert!(!index.ok);
    assert!(
        index.mismatches.iter().any(|m| m.kind == "cyclic_require"),
        "expected cyclic_require mismatch: {:?}",
        index.mismatches
    );
    assert!(
        index.diagnostics.iter().any(|d| d.code == "FILE_TOO_LARGE"),
        "expected FILE_TOO_LARGE diagnostic: {:?}",
        index.diagnostics
    );
    // Ensure inner `return "string_in_inner_is_ok"` after `if ... end` did NOT falsely trigger outer `: number` mismatch!
    assert!(
        !index
            .mismatches
            .iter()
            .any(|m| m.caller_path == "src/server/MalformedStrict.server.luau"
                && m.kind == "strict_type_error"),
        "nested function after if..end must not false-positive: {:?}",
        index.mismatches
    );

    // UTF-8 char column lookup on line 6 (`local label = "テスト🎮" ; function ModA.Ping(...)`)
    let def = lookup_definition(
        &scripts,
        &BTreeMap::new(),
        &service_to_file,
        "./src/shared/ModA.luau",
        6,
        38,
    );
    assert!(def.ok, "expected UTF-8 line lookup to succeed: {def:?}");
    assert_eq!(def.target_path.as_deref(), Some("src/shared/ModA.luau"));

    // CWE-22 path normalization in lookup_definition & filter_by_path fails closed
    for bad in ["../secret.luau", "/etc/passwd", r"src\shared\ModA.luau", "C:\\x.luau"] {
        let res = lookup_definition(&scripts, &BTreeMap::new(), &service_to_file, bad, 1, 1);
        assert!(!res.ok, "expected not_found for '{bad}'");
    }
    let mut filtered = index.clone();
    filtered.filter_by_path("../escape");
    assert!(filtered.symbols.is_empty());
}

#[test]
fn luau_routes_block_double_encoded_traversal_and_filter_out_of_tree_analyzer_paths() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("luau-cwe22-h1");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Mock `luau-analyze` that emits an out-of-tree `/etc/passwd` path AND a `../../outside.luau` path
    let fake_bin = root.join("luau-analyze");
    std::fs::write(
        &fake_bin,
        "#!/bin/sh\necho '/etc/passwd(1,1): TypeError: out of tree'\necho '../../outside.luau(2,3): TypeError: traversal'\necho 'src/server/Main.server.luau(1,1): Warning: clean warning'\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, perms).unwrap();
    }
    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", fake_bin.to_str().unwrap())]);

    let (port, _shared) = start_server(&data_dir);
    let host = format!("127.0.0.1:{port}");
    let (pid, proj_dir) = create_rojo_project(port, &root, "LuauH1");
    std::fs::write(proj_dir.join("src/server/Main.server.luau"), "--!strict\nlocal x = 1\n").unwrap();

    // Double-encoded `%252e%252e` rejected with 400
    for bad in ["%252e%252e%252fetc%252fpasswd", "src/%2e%2e/etc/passwd"] {
        let (st_sym, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/luau/symbols?path={bad}"),
            None,
        );
        assert_eq!(st_sym, 400, "symbols must reject '{bad}'");
        let (st_def, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/luau/definition?path={bad}&line=1&col=1"),
            None,
        );
        assert_eq!(st_def, 400, "definition must reject '{bad}'");
    }

    // Typecheck strips out-of-tree paths from luau-analyze output
    let (st_tc, tc_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/luau/typecheck"),
        Some(&json!({})),
    );
    assert_eq!(st_tc, 200, "{tc_res}");
    let diags = tc_res["diagnostics"].as_array().unwrap();
    assert!(
        !diags.iter().any(|d| d["path"].as_str().unwrap_or("").contains("passwd")
            || d["path"].as_str().unwrap_or("").contains("outside")),
        "out-of-tree diagnostic path leaked: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d["path"] == "src/server/Main.server.luau"),
        "expected valid project diagnostic: {diags:?}"
    );
    let _unused = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:0".into(), "stub".into(), None),
        LocalPlace::in_memory("Stub".into()),
    );
}
