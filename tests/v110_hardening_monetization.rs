//! v1.1.0 security hardening and Monetization & Retention Luau Pack tests.

use openro_rs::agent::Agent;
use openro_rs::luau_check::analyze_script;
use openro_rs::model::ModelClient;
use openro_rs::opencloud::{monetization_pack_luau_scripts, safe_rel};
use openro_rs::place::{parse_rbxlx_xml, LocalPlace};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn start_local_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let place = LocalPlace::in_memory("Monetization Obby".to_string());
    let model = ModelClient::new("http://127.0.0.1:1".into(), "stub".into(), None);
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::new(Some(agent), "Monetization Obby".to_string());
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn send_raw(port: u16, raw: &[u8]) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(raw).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = serde_json::from_slice(&buf[sep.saturating_add(4).min(buf.len())..])
        .unwrap_or_else(|_| json!({}));
    (status, body)
}

#[test]
fn http_parser_rejects_duplicate_host_bad_content_length_and_chunked() {
    let port = start_local_server();
    let (s_dup, _) = send_raw(
        port,
        b"GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(s_dup, 400);

    let (s_cl, _) = send_raw(
        port,
        b"POST /api/chats/new HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: abc\r\nConnection: close\r\n\r\n{}",
    );
    assert_eq!(s_cl, 400);

    let (s_chunk, _) = send_raw(
        port,
        b"POST /api/chats/new HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n",
    );
    assert_eq!(s_chunk, 501);
}

#[test]
fn sensitive_routes_enforce_loopback_host() {
    let port = start_local_server();
    for (method, path) in [
        ("GET", "/api/opencloud"),
        ("GET", "/api/creator-hub/status"),
        ("POST", "/api/monetization/pack"),
        ("POST", "/api/studio/playtest"),
    ] {
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: lan-desk.local:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        let (st, _) = send_raw(port, req.as_bytes());
        assert_eq!(st, 403, "{method} {path} must reject non-loopback Host");
    }
}

#[test]
fn xml_parser_blocks_mixed_case_dtd_and_element_xxe() {
    for xml in [
        "<!DocType roblox [ <!Entity x SYSTEM \"file:///etc/passwd\"> ]><roblox version=\"4\"/>",
        "<!ELEMENT roblox ANY><roblox version=\"4\"/>",
    ] {
        let err = parse_rbxlx_xml(xml).unwrap_err();
        assert!(err.contains("XXE"), "{err}");
    }
}

#[cfg(unix)]
#[test]
fn safe_rel_blocks_ancestor_symlink_for_uncreated_nested_target() {
    let base = std::env::temp_dir().join(format!("openro-v110-sym-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let proj = base.join("proj");
    let outside = base.join("outside");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, proj.join("art")).unwrap();

    let err = safe_rel(&proj, "art/nested/newfile.png").unwrap_err();
    assert!(err.msg.contains("leaves the project folder"), "{}", err.msg);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn luau_check_flags_unsafe_process_receipt_and_passes_monetization_pack() {
    let unsafe_src = r#"
local MarketplaceService = game:GetService("MarketplaceService")
MarketplaceService.ProcessReceipt = function(info)
    return Enum.ProductPurchaseDecision.PurchaseGranted
end
"#;
    let bad_rep = analyze_script("ServerScriptService.BadPay", unsafe_src);
    assert!(bad_rep.has_code("UNSAFE_PROCESS_RECEIPT"));

    let pack = monetization_pack_luau_scripts("Coins", 111, 222, 100);
    assert_eq!(pack.len(), 3);
    for (place_path, _rel, _cls, src) in &pack {
        let rep = analyze_script(place_path, src);
        assert!(
            rep.is_ok() && rep.warning_count() == 0,
            "{place_path} diagnostics: {:?}",
            rep.diagnostics
        );
    }

    let port = start_local_server();
    let payload = br#"{"currency":"Gems","vip_pass_id":12345,"starter_pack_id":67890,"daily_base_reward":150}"#;
    let req = format!(
        "POST /api/monetization/pack HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let mut full = req.into_bytes();
    full.extend_from_slice(payload);
    let (st, resp) = send_raw(port, &full);
    assert_eq!(st, 200, "{resp}");
    assert_eq!(resp["ok"], json!(true));
    assert_eq!(resp["audit"]["ok"], json!(true));
    assert_eq!(resp["scripts"].as_array().unwrap().len(), 3);
}

#[cfg(unix)]
#[test]
fn symlink_asset_delete_and_get_return_403_forbidden() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!(
        "openro-v110-symlink-delete-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let data_dir = base.join("data");
    let assets_dir = data_dir.join("game-assets");
    std::fs::create_dir_all(&assets_dir).unwrap();
    let outside = base.join("outside.png");
    std::fs::write(&outside, b"\x89PNG\r\n\x1a\n").unwrap();
    symlink(&outside, assets_dir.join("escape_writable.png")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let place = LocalPlace::in_memory("Symlink Guard".to_string());
    let model = ModelClient::new("http://127.0.0.1:1".into(), "stub".into(), None);
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::with_data_dir(Some(agent), "Symlink Guard".to_string(), data_dir);
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });

    let get_req = format!("GET /api/assets/escape_writable.png HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let (st_get, _) = send_raw(port, get_req.as_bytes());
    assert_eq!(st_get, 403, "GET symlink asset should return 403");

    let del_req = format!("DELETE /api/assets/escape_writable.png HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let (st_del, body_del) = send_raw(port, del_req.as_bytes());
    assert_eq!(st_del, 403, "DELETE symlink asset should return 403, got {body_del}");
    assert!(outside.exists(), "outside target must not be deleted");

    let _ = std::fs::remove_dir_all(&base);
}
