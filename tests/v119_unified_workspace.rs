//! Integration tests for OpenRO Cycle 2 Wave E1 (v1.19.0 GAP #1):
//! - `GET /api/projects/:id/workspace` (scripts, instances, meshes, lighting, diagnostics)
//! - `POST /api/projects/:id/scene/mutate` (`upsert`, `duplicate`, `delete`, `set_lighting`, `undo`, `redo`, `default.project.json` auto-wiring)
//! - `GET /api/projects/:id/mesh-asset?path=...` (valid .obj/.png, CWE-22 traversal & CWE-59 symlink jails, 10 MB cap)
//! - `projects::builtin_rbxlx` native `CFrame`, `Enum` tokens, `Content` URLs (`MeshId`/`TextureID`/`SoundId`), `Color3uint8`, and `Lighting` serialization
//! - `LocalPlace` preservation when no Rojo project is opened
//! - `project_scene_get` & `project_scene_mutate` agent/teamwork tool bindings

use openro_rs::agent::{Agent, AskOptions};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::projects;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v119-unified-workspace");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn start_server(agent: Option<Agent>, data_dir: PathBuf) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "Test Game".to_string(), data_dir);
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn http_req(
    port: u16,
    method: &str,
    path: &str,
    host: &str,
    body: Option<&str>,
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let payload = body.unwrap_or("");
    let ct = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{ct}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(req.as_bytes()).expect("write");
    stream.flush().ok();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http header separator");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body_bytes = raw[split + 4..].to_vec();
    (status, head, body_bytes)
}

fn api_json(port: u16, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let body_str = body.map(|v| v.to_string());
    let (code, _, bytes) = http_req(
        port,
        method,
        path,
        &format!("127.0.0.1:{port}"),
        body_str.as_deref(),
    );
    let parsed = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "non-JSON response ({code}) for {method} {path}: {e}\n{}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (code, parsed)
}

fn create_test_project(port: u16, ws: &Path, name: &str) -> (String, PathBuf) {
    let (code, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let (code, res) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": name, "parent": ws.to_string_lossy() })),
    );
    assert_eq!(code, 201);
    let id = res["project"]["id"].as_str().unwrap().to_string();
    let dir = PathBuf::from(res["project"]["path"].as_str().unwrap());
    (id, dir)
}

fn start_stub_model<F>(handler: F) -> String
where
    F: Fn(&str, usize) -> Value + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    let counter = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let h = Arc::clone(&handler);
            let c = Arc::clone(&counter);
            std::thread::spawn(move || {
                let mut s = stream;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut header_end = None;
                let mut content_len = 0usize;
                while let Ok(n) = s.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let head = String::from_utf8_lossy(&buf[..pos]);
                            for line in head.lines() {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.trim().eq_ignore_ascii_case("content-length") {
                                        content_len = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(he) = header_end {
                        if buf.len() >= he + content_len {
                            break;
                        }
                    }
                }
                let body = header_end
                    .map(|he| {
                        String::from_utf8_lossy(&buf[he..buf.len().min(he + content_len)])
                            .to_string()
                    })
                    .unwrap_or_default();
                let seq = c.fetch_add(1, Ordering::SeqCst) + 1;
                let reply = h(&body, seq).to_string();
                let payload = json!({
                    "id": format!("chatcmpl-{seq}"),
                    "object": "chat.completion",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": reply },
                        "finish_reason": "stop"
                    }]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}/v1/chat/completions")
}

#[test]
fn workspace_bundle_returns_scripts_meshes_lighting_and_diagnostics() {
    let root = temp_dir("bundle");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Unified Arena");

    // Add a script with a syntax error so diagnostics surface with file + service_path
    std::fs::write(
        proj_dir.join("src/server/Broken.server.luau"),
        "local x = 1\nif x == 1 then\n  print(x)\n",
    )
    .unwrap();

    // Add a mesh + texture + manifest under assets/meshes/
    let meshes_dir = proj_dir.join("assets/meshes");
    std::fs::create_dir_all(&meshes_dir).unwrap();
    let obj_src = "v 0 0 0\nv 4 0 0\nv 4 6 0\nv 0 6 2\nf 1 2 3\nf 1 3 4\n";
    std::fs::write(meshes_dir.join("altar.obj"), obj_src).unwrap();
    std::fs::write(meshes_dir.join("altar.png"), b"\x89PNG\r\n\x1a\nMOCKPNG").unwrap();
    std::fs::write(
        meshes_dir.join("manifest.json"),
        json!({
            "altar": {
                "mesh": "altar.obj",
                "texture": "altar.png",
                "size": [4.0, 6.0, 2.0],
                "solid_tris": 2
            }
        })
        .to_string(),
    )
    .unwrap();

    // Non-loopback host refused
    let (code, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/workspace"),
        "evil.example.com",
        None,
    );
    assert_eq!(code, 403);

    // Fetch workspace bundle
    let (code, bundle) = api_json(port, "GET", &format!("/api/projects/{id}/workspace"), None);
    assert_eq!(code, 200);
    assert_eq!(bundle["ok"], json!(true));
    assert_eq!(bundle["project_id"], json!(id));
    assert_eq!(bundle["mode"], json!("rojo"));
    assert_eq!(bundle["scene_revision"], json!(0));
    assert_eq!(bundle["can_undo"], json!(false));

    // Scripts keyed by relative path with service_path, class_name, sha256, lines
    let main_server = &bundle["scripts"]["src/server/Main.server.luau"];
    assert_eq!(
        main_server["service_path"],
        json!("ServerScriptService.Server.Main")
    );
    assert_eq!(main_server["class_name"], json!("Script"));
    assert_eq!(main_server["lines"], json!(1));
    assert!(main_server["sha256"].as_str().unwrap().len() == 64);

    let main_client = &bundle["scripts"]["src/client/Main.client.luau"];
    assert_eq!(main_client["class_name"], json!("LocalScript"));
    let shared_cfg = &bundle["scripts"]["src/shared/Config/init.luau"];
    assert_eq!(shared_cfg["class_name"], json!("ModuleScript"));
    assert_eq!(
        shared_cfg["service_path"],
        json!("ReplicatedStorage.Shared.Config")
    );

    // Meshes discovered with mesh-asset URLs
    let meshes = bundle["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0]["name"], json!("altar"));
    assert_eq!(meshes[0]["tris"], json!(2));
    assert_eq!(meshes[0]["size"], json!([4.0, 6.0, 2.0]));
    let obj_url = meshes[0]["obj_url"].as_str().unwrap();
    let tex_url = meshes[0]["texture_url"].as_str().unwrap();
    assert!(obj_url.contains(&format!("/api/projects/{id}/mesh-asset?path=")));

    // Fetch mesh & texture bytes through the returned URLs
    let (code, head, bytes) = http_req(port, "GET", obj_url, &format!("127.0.0.1:{port}"), None);
    assert_eq!(code, 200);
    assert!(head.contains("text/plain"));
    assert_eq!(String::from_utf8_lossy(&bytes), obj_src);

    let (code, head, bytes) = http_req(port, "GET", tex_url, &format!("127.0.0.1:{port}"), None);
    assert_eq!(code, 200);
    assert!(head.contains("image/png"));
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    // Default lighting present
    assert_eq!(bundle["lighting"]["clock_time"], json!(14.0));
    assert_eq!(bundle["lighting"]["brightness"], json!(2.0));
    assert!(bundle["lighting"]["ambient"].as_str().unwrap().starts_with('#'));

    // Diagnostics capture Broken.server.luau with matching file path
    let diags = bundle["diagnostics"].as_array().unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d["file"] == json!("src/server/Broken.server.luau")
                && d["severity"] == json!("error")),
        "expected diagnostic for Broken.server.luau, got {diags:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scene_mutate_autowires_rojo_supports_crud_lighting_and_undo_redo() {
    let root = temp_dir("scene-mutate");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Scene Builder");

    // Before mutation: default.project.json has no Workspace or Lighting, and scene/workspace.model.json does not exist
    assert!(!proj_dir.join("scene/workspace.model.json").exists());
    let dp_before: Value = serde_json::from_str(
        &std::fs::read_to_string(proj_dir.join("default.project.json")).unwrap(),
    )
    .unwrap();
    assert!(dp_before["tree"].get("Workspace").is_none());
    assert!(dp_before["tree"].get("Lighting").is_none());

    // 1. Upsert SpawnPad + nested Altar MeshPart + set_lighting
    let (code, res1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [
                {
                    "op": "upsert",
                    "path": "Workspace.SpawnPad",
                    "class_name": "SpawnLocation",
                    "properties": {
                        "Anchored": true,
                        "Position": [0, 1, 0],
                        "Size": [8, 1, 8],
                        "Color": "#4F7CF0",
                        "Material": "SmoothPlastic"
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Hub.RollAltar",
                    "class_name": "MeshPart",
                    "properties": {
                        "Anchored": true,
                        "Position": [12, 3, -6],
                        "Rotation": [0, 90, 0],
                        "Size": [4, 6, 4],
                        "Color": "#F59E0B",
                        "Material": "Neon",
                        "MeshId": "assets/meshes/roll_altar.obj",
                        "TextureID": "assets/meshes/roll_altar.png"
                    }
                },
                {
                    "op": "set_lighting",
                    "path": "Lighting",
                    "properties": {
                        "ClockTime": 20.5,
                        "Brightness": 1.4,
                        "Ambient": "#1E293B",
                        "FogEnd": 600,
                        "Technology": "Future",
                        "Atmosphere": {
                            "Density": 0.42,
                            "Offset": 0.15,
                            "Color": "#93C5FD"
                        }
                    }
                }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res1["ok"], json!(true));
    assert_eq!(res1["scene_revision"], json!(1));
    assert_eq!(res1["can_undo"], json!(true));
    assert_eq!(res1["can_redo"], json!(false));
    let modified = res1["modified_files"].as_array().unwrap();
    assert!(modified.contains(&json!("scene/workspace.model.json")));
    assert!(modified.contains(&json!("default.project.json")));

    // Verify default.project.json auto-wiring
    let dp_after: Value = serde_json::from_str(
        &std::fs::read_to_string(proj_dir.join("default.project.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        dp_after["tree"]["Workspace"]["$path"],
        json!("scene/workspace.model.json")
    );
    assert_eq!(dp_after["tree"]["Lighting"]["$className"], json!("Lighting"));
    assert_eq!(
        dp_after["tree"]["Lighting"]["$properties"]["ClockTime"],
        json!(20.5)
    );
    assert_eq!(
        dp_after["tree"]["Lighting"]["Atmosphere"]["$properties"]["Density"],
        json!(0.42)
    );

    // Verify instances & lighting in response
    let spawn = &res1["instances"]["Workspace.SpawnPad"];
    assert_eq!(spawn["class_name"], json!("SpawnLocation"));
    assert_eq!(spawn["source_file"], json!("scene/workspace.model.json"));
    assert_eq!(spawn["properties"]["Position"], json!([0.0, 1.0, 0.0]));
    assert_eq!(spawn["properties"]["Color"], json!("#4F7CF0"));

    let altar = &res1["instances"]["Workspace.Hub.RollAltar"];
    assert_eq!(altar["class_name"], json!("MeshPart"));
    assert_eq!(altar["properties"]["Position"], json!([12.0, 3.0, -6.0]));
    assert_eq!(altar["properties"]["Rotation"], json!([0.0, 90.0, 0.0]));
    assert_eq!(
        altar["properties"]["MeshId"],
        json!("assets/meshes/roll_altar.obj")
    );

    assert_eq!(res1["lighting"]["clock_time"], json!(20.5));
    assert_eq!(res1["lighting"]["brightness"], json!(1.4));
    assert_eq!(res1["lighting"]["ambient"], json!("#1E293B"));
    assert_eq!(res1["lighting"]["fog_end"], json!(600.0));
    assert_eq!(res1["lighting"]["technology"], json!("Future"));
    assert_eq!(res1["lighting"]["atmosphere"]["density"], json!(0.42));

    // 2. Duplicate SpawnPad with offset + delete RollAltar
    let (code, res2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [
                {
                    "op": "duplicate",
                    "path": "Workspace.SpawnPad",
                    "name": "SpawnPadTwo",
                    "offset": [10, 0, -4]
                },
                {
                    "op": "delete",
                    "path": "Workspace.Hub.RollAltar"
                }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res2["scene_revision"], json!(2));
    assert!(res2["instances"].get("Workspace.Hub.RollAltar").is_none());
    assert_eq!(
        res2["instances"]["Workspace.SpawnPadTwo"]["properties"]["Position"],
        json!([10.0, 1.0, -4.0])
    );

    // 3. Undo step 2 -> RollAltar is back, SpawnPadTwo is gone
    let (code, undo1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "ops": [{ "op": "undo" }] })),
    );
    assert_eq!(code, 200);
    assert_eq!(undo1["undone"], json!(true));
    assert_eq!(undo1["can_undo"], json!(true));
    assert_eq!(undo1["can_redo"], json!(true));
    assert!(undo1["instances"].get("Workspace.Hub.RollAltar").is_some());
    assert!(undo1["instances"].get("Workspace.SpawnPadTwo").is_none());

    // 4. Redo step 2 -> SpawnPadTwo is back
    let (code, redo1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "redo": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(redo1["redone"], json!(true));
    assert!(redo1["instances"].get("Workspace.SpawnPadTwo").is_some());

    // 5. Undo twice back to pristine initial state (removes scene/workspace.model.json)
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "undo": true })),
    );
    assert_eq!(code, 200);
    let (code, undo2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "undo": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(undo2["can_undo"], json!(false));
    assert!(!proj_dir.join("scene/workspace.model.json").exists());

    // Undo when stack empty fails cleanly
    let (code, err_undo) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "undo": true })),
    );
    assert_eq!(code, 400);
    assert_eq!(err_undo["ok"], json!(false));

    // Invalid Workspace path rejected
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "ops": [{ "op": "upsert", "path": "ServerScriptService.Bad" }] })),
    );
    assert_eq!(code, 400);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mesh_asset_enforces_cwe22_cwe59_and_size_cap() {
    let root = temp_dir("mesh-security");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Mesh Security");

    let meshes = proj_dir.join("assets/meshes");
    std::fs::create_dir_all(&meshes).unwrap();
    std::fs::write(meshes.join("cube.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();

    // Outside secret file + oversized > 10 MB file
    let outside_obj = root.join("secret.obj");
    std::fs::write(&outside_obj, "v 9 9 9\n").unwrap();
    let big_bytes = vec![b' '; 10 * 1024 * 1024 + 64];
    std::fs::write(meshes.join("huge.obj"), &big_bytes).unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside_obj, meshes.join("escape_link.obj")).unwrap();
        std::os::unix::fs::symlink(meshes.join("cube.obj"), meshes.join("local_link.obj")).unwrap();
    }

    let host = format!("127.0.0.1:{port}");
    // Valid file succeeds
    let (code, _, body) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/mesh-asset?path=assets/meshes/cube.obj"),
        &host,
        None,
    );
    assert_eq!(code, 200);
    assert!(String::from_utf8_lossy(&body).contains("f 1 2 3"));

    // CWE-22 traversal & disallowed extensions
    for bad in [
        "",
        "../secret.obj",
        "assets/../../secret.obj",
        "/etc/passwd",
        "src/server/Main.server.luau",
        "openro.project.json",
        ".git/config",
        ".openro/scene-undo.json",
    ] {
        let (code, _, _) = http_req(
            port,
            "GET",
            &format!("/api/projects/{id}/mesh-asset?path={bad}"),
            &host,
            None,
        );
        assert_eq!(code, 400, "expected 400 for path '{bad}', got {code}");
    }

    // CWE-59 symlinks refused
    #[cfg(unix)]
    for link in ["assets/meshes/escape_link.obj", "assets/meshes/local_link.obj"] {
        let (code, _, _) = http_req(
            port,
            "GET",
            &format!("/api/projects/{id}/mesh-asset?path={link}"),
            &host,
            None,
        );
        assert_eq!(code, 400, "expected 400 for symlink '{link}', got {code}");
    }

    // > 10 MB file refused
    let (code, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/mesh-asset?path=assets/meshes/huge.obj"),
        &host,
        None,
    );
    assert_eq!(code, 400);

    // Missing valid .obj -> 404
    let (code, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/mesh-asset?path=assets/meshes/missing.obj"),
        &host,
        None,
    );
    assert_eq!(code, 404);

    // Non-loopback refused -> 403
    let (code, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/mesh-asset?path=assets/meshes/cube.obj"),
        "evil.example.com",
        None,
    );
    assert_eq!(code, 403);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn builtin_rbxlx_serializes_cframe_enum_content_color3uint8_and_lighting() {
    let root = temp_dir("builtin-xml");
    let proj = root.join("proj");
    std::fs::create_dir_all(proj.join("src/server")).unwrap();
    std::fs::create_dir_all(proj.join("scene")).unwrap();

    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "Rich XML Test",
            "tree": {
                "$className": "DataModel",
                "Workspace": { "$path": "scene/workspace.model.json" },
                "Lighting": {
                    "$className": "Lighting",
                    "$properties": {
                        "ClockTime": 18.5,
                        "Brightness": 2.5,
                        "Ambient": [0.2, 0.3, 0.4],
                        "FogEnd": 900.0,
                        "Technology": "Future"
                    },
                    "Atmosphere": {
                        "$className": "Atmosphere",
                        "$properties": {
                            "Density": 0.35,
                            "Color": [0.8, 0.85, 0.9]
                        }
                    }
                },
                "SoundService": {
                    "$className": "SoundService",
                    "ClickSfx": {
                        "$className": "Sound",
                        "$properties": {
                            "SoundId": "rbxassetid://998877"
                        }
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    std::fs::write(
        proj.join("scene/workspace.model.json"),
        json!({
            "className": "Workspace",
            "properties": { "Gravity": 196.2 },
            "children": [
                {
                    "name": "AltarMesh",
                    "className": "MeshPart",
                    "properties": {
                        "Anchored": true,
                        "Size": [4, 6, 4],
                        "CFrame": {
                            "CFrame": {
                                "position": [10, 3, -5],
                                "orientation": [[0, 0, -1], [0, 1, 0], [1, 0, 0]]
                            }
                        },
                        "Color": [1.0, 0.5, 0.0],
                        "Material": "Neon",
                        "MeshId": "rbxassetid://112233",
                        "TextureID": "rbxassetid://445566"
                    }
                },
                {
                    "name": "Pillar",
                    "className": "Part",
                    "properties": {
                        "Anchored": true,
                        "Position": [0, 5, 0],
                        "Rotation": [0, 0, 90],
                        "Size": [10, 2, 2],
                        "Shape": "Cylinder",
                        "Color3uint8": [255, 128, 0],
                        "Material": { "Enum": "Enum.Material.Marble" }
                    }
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let tree = projects::load_tree(&proj, "default.project.json").unwrap();
    let mut warnings = Vec::new();
    let xml = projects::builtin_rbxlx(&tree, &mut warnings);
    assert!(
        warnings.is_empty(),
        "expected zero builtin_rbxlx warnings, got: {warnings:?}"
    );

    assert!(xml.contains("<CoordinateFrame name=\"CFrame\"><X>10</X><Y>3</Y><Z>-5</Z>"));
    assert!(xml.contains("<R02>-1</R02>"));
    assert!(xml.contains("<token name=\"Material\">288</token>")); // Neon
    assert!(xml.contains("<token name=\"Material\">784</token>")); // Marble
    assert!(xml.contains("<token name=\"shape\">2</token>")); // Cylinder
    assert!(xml.contains("<Content name=\"MeshId\"><url>rbxassetid://112233</url></Content>"));
    assert!(xml.contains("<Content name=\"TextureID\"><url>rbxassetid://445566</url></Content>"));
    assert!(xml.contains("<Content name=\"SoundId\"><url>rbxassetid://998877</url></Content>"));
    assert!(xml.contains("<Color3uint8 name=\"Color3uint8\">4294934528</Color3uint8>")); // 0xFFFF8000
    assert!(xml.contains("<Item class=\"Lighting\""));
    assert!(xml.contains("<float name=\"ClockTime\">18.5</float>"));
    assert!(xml.contains("<float name=\"Brightness\">2.5</float>"));
    assert!(xml.contains("<Color3 name=\"Ambient\"><R>0.2</R><G>0.3</G><B>0.4</B></Color3>"));
    assert!(xml.contains("<float name=\"FogEnd\">900</float>"));
    assert!(xml.contains("<token name=\"Technology\">4</token>")); // Future
    assert!(xml.contains("<Item class=\"Atmosphere\""));
    assert!(xml.contains("<float name=\"Density\">0.35</float>"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn local_place_behavior_preserved_without_rojo_project() {
    let root = temp_dir("local-place");
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let place = LocalPlace::open(data.clone(), "Legacy Local Game".into());
    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:0".into(), "test".into(), None),
        place,
    );
    let port = start_server(Some(agent), data);
    let (code, tree) = api_json(port, "GET", "/api/place-tree", None);
    assert_eq!(code, 200);
    assert!(tree["scripts"].is_object());
    assert!(tree["instances"].is_object());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn agent_and_teamwork_project_scene_tools_mutate_and_read() {
    let root = temp_dir("agent-scene-tools");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    // 1. Test Agent tools (`project_scene_mutate` -> `project_scene_get` -> done)
    let stub_url = start_stub_model(|_body, seq| match seq {
        1 => json!({
            "tool": "project_scene_mutate",
            "args": {
                "ops": [{
                    "op": "upsert",
                    "path": "Workspace.AgentBeacon",
                    "class_name": "Part",
                    "properties": { "Position": [5, 2, 5], "Size": [2, 4, 2], "Color": "#22C55E", "Material": "Neon", "Anchored": true }
                }]
            }
        }),
        2 => json!({ "tool": "project_scene_get", "args": {} }),
        _ => json!({ "done": true, "summary": "created beacon" }),
    });

    let mut store = projects::ProjectStore::load(Some(&data));
    store.add_root(&ws.to_string_lossy()).unwrap();
    let pref = store.create("Agent Scene Game", None, None, None).unwrap();

    let mut agent = Agent::with_local_place(
        ModelClient::new(stub_url.clone(), "test".into(), None),
        LocalPlace::open(data.clone(), "Agent Scene Game".into()),
    );
    agent.set_project(Some(pref.dir.clone()), Some(pref.id.clone()));

    let out = agent
        .run_ask_with_options("chat-1", "Add AgentBeacon to Workspace", AskOptions::default())
        .expect("agent run");
    assert!(out.summary.contains("created beacon"));
    assert!(pref.dir.join("scene/workspace.model.json").is_file());
    let bundle = projects::load_workspace_bundle(&pref.dir, &pref.id).unwrap();
    assert!(bundle["instances"].get("Workspace.AgentBeacon").is_some());

    // 2. Test Teamwork tools via HTTP API (`POST /api/teamwork`)
    let tw_stub = start_stub_model(|body, _seq| {
        if body.contains("[Role: coordinator]") {
            if !body.contains("tool_result (create_task)") && !body.contains("\"t1\"") {
                json!({
                    "tool": "create_task",
                    "args": {
                        "title": "Add Hub Stage",
                        "owner": "builder",
                        "verifier": "verifier",
                        "kind": "docs",
                        "files": ["scene/workspace.model.json"]
                    }
                })
            } else if body.contains("\"done\"") || body.contains("HubStage verified") {
                json!({ "tool": "finish_run", "args": { "summary": "scene ready" } })
            } else {
                json!({ "done": true, "summary": "waiting" })
            }
        } else if body.contains("[Role: verifier]") {
            if !body.contains("tool_result (project_scene_get)") {
                json!({ "tool": "project_scene_get", "args": {} })
            } else {
                json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "HubStage verified" } })
            }
        } else if !body.contains("tool_result (project_scene_mutate)") {
            json!({
                "tool": "project_scene_mutate",
                "args": {
                    "task_id": "t1",
                    "ops": [{
                        "op": "upsert",
                        "path": "Workspace.HubStage",
                        "class_name": "Part",
                        "properties": { "Position": [0, 0.5, 0], "Size": [24, 1, 24], "Anchored": true }
                    }]
                }
            })
        } else {
            json!({ "tool": "submit_task", "args": { "task_id": "t1", "summary": "HubStage added" } })
        }
    });

    let tw_base = tw_stub.trim_end_matches("/chat/completions").to_string();
    let server_agent = Agent::with_local_place(
        ModelClient::new(tw_stub, "stub-model".into(), None),
        LocalPlace::open(data.clone(), "Agent Scene Game".into()),
    );
    let port = start_server(Some(server_agent), data);
    let (cfg_code, cfg_res) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({ "base_url": tw_base, "model_id": "stub-model" })),
    );
    assert_eq!(cfg_code, 200, "config failed: {cfg_res}");

    let (code, started) = api_json(
        port,
        "POST",
        "/api/teamwork",
        Some(json!({
            "goal": "Add HubStage",
            "project_id": pref.id,
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert_eq!(code, 201, "teamwork start failed: {started}");
    let run_id = started["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut final_status = String::new();
    while Instant::now() < deadline {
        let (_, st) = api_json(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        final_status = st["run"]["status"].as_str().unwrap_or("").to_string();
        if matches!(final_status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    assert_eq!(final_status, "completed");
    let after = projects::load_workspace_bundle(&pref.dir, &pref.id).unwrap();
    assert!(after["instances"].get("Workspace.HubStage").is_some());

    let _ = std::fs::remove_dir_all(&root);
}
