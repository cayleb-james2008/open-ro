//! Integration tests for OpenRO Cycle 3 Wave E2 (v1.24.0 GAP #2):
//! - `GET /api/projects/:id/workspace`: `terrain`, `lights`, `meshes[].texture_url` (`.png`/`.jpg`/`.mtl` + `SurfaceAppearance.ColorMap`)
//! - `POST /api/projects/:id/scene/mutate`: `set_terrain` (generators, `paint_box`, `clear`, Luau builder hook, undo/redo, 2048x512x2048 / 100-region bounds 400), `reparent`, and `upsert` for `PointLight`/`SpotLight`/`SurfaceLight`/`ParticleEmitter`/`Sound`/`SurfaceAppearance`
//! - `GET /api/projects/:id/mesh-asset?file=<rel>`: `.obj`/`.mtl`/`.png`/`.jpg` under `assets/meshes/**` and `art/**`, CWE-22/CWE-59 traversal + symlink confinement, strict extension allowlist, 10 MiB cap, exact Content-Type
//! - `project_scene_mutate` agent prompt schema & end-to-end `set_terrain` + `reparent` + lights execution

use openro_rs::agent::{project_system_prompt, system_prompt, Agent, AskOptions};
use openro_rs::luau_check;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::projects;
use openro_rs::scene;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v124-viewport-terrain");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn start_server(agent: Option<Agent>, data_dir: PathBuf) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "Viewport Test".to_string(), data_dir);
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
fn workspace_returns_terrain_lights_and_surface_appearance_texture_url() {
    let root = temp_dir("ws-terrain-lights");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Viewport World");

    // 1. Default workspace bundle has disabled terrain (cell_size = 4) and empty lights
    let (code, initial) = api_json(port, "GET", &format!("/api/projects/{id}/workspace"), None);
    assert_eq!(code, 200);
    assert_eq!(initial["terrain"]["enabled"], json!(false));
    assert_eq!(initial["terrain"]["cell_size"], json!(4));
    assert_eq!(initial["terrain"]["regions"], json!([]));
    assert_eq!(initial["lights"], json!([]));

    // 2. Meshes with .jpg, .mtl, and SurfaceAppearance ColorMap under art/
    let meshes_dir = proj_dir.join("assets/meshes");
    let art_dir = proj_dir.join("art/textures");
    std::fs::create_dir_all(&meshes_dir).unwrap();
    std::fs::create_dir_all(&art_dir).unwrap();

    let obj = "v 0 0 0\nv 2 0 0\nv 2 2 0\nf 1 2 3\n";
    std::fs::write(meshes_dir.join("statue.obj"), obj).unwrap();
    std::fs::write(meshes_dir.join("statue.jpg"), b"\xFF\xD8\xFF\xE0JPEG").unwrap();

    std::fs::write(meshes_dir.join("pillar.obj"), obj).unwrap();
    std::fs::write(meshes_dir.join("pillar.mtl"), "newmtl stone\nKd 0.6 0.6 0.6\n").unwrap();

    std::fs::write(meshes_dir.join("crate.obj"), obj).unwrap();
    std::fs::write(art_dir.join("crate_albedo.png"), b"\x89PNG\r\n\x1a\nCRATE").unwrap();

    // 3. Upsert MeshPart + SurfaceAppearance + Lamp parts with PointLight / SpotLight / SurfaceLight
    let (code, mutated) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [
                {
                    "op": "upsert",
                    "path": "Workspace.CrateProp",
                    "class_name": "MeshPart",
                    "properties": {
                        "Position": [6.0, 2.0, -4.0],
                        "Size": [4.0, 4.0, 4.0],
                        "MeshId": "assets/meshes/crate.obj",
                        "Anchored": true
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.CrateProp.PbrMat",
                    "class_name": "SurfaceAppearance",
                    "properties": {
                        "ColorMap": "art/textures/crate_albedo.png",
                        "AlphaMode": "Enum.AlphaMode.Overlay"
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Torch",
                    "class_name": "Part",
                    "properties": {
                        "Position": [10.0, 6.0, -4.0],
                        "Rotation": [0.0, 90.0, 0.0],
                        "Anchored": true
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Torch.FlameLight",
                    "class_name": "PointLight",
                    "properties": {
                        "Color": "#FF8833",
                        "Brightness": 2.5,
                        "Range": 20.0,
                        "Shadows": true,
                        "Enabled": true
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Torch.Beam",
                    "class_name": "SpotLight",
                    "properties": {
                        "Color": "#FFF0AA",
                        "Brightness": 3.0,
                        "Range": 32.0,
                        "Angle": 55.0,
                        "Face": "Enum.NormalId.Front",
                        "Shadows": true,
                        "Enabled": true
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Torch.Panel",
                    "class_name": "SurfaceLight",
                    "properties": {
                        "Color": "#88CCFF",
                        "Brightness": 1.25,
                        "Range": 12.0,
                        "Angle": 90.0,
                        "Face": "Top",
                        "Shadows": false,
                        "Enabled": false
                    }
                }
            ]
        })),
    );
    assert_eq!(code, 200, "mutate failed: {mutated}");

    let (code, bundle) = api_json(port, "GET", &format!("/api/projects/{id}/workspace"), None);
    assert_eq!(code, 200);

    // Verify meshes[].texture_url points to /api/projects/:id/mesh-asset?file=<rel> and serves 200
    let meshes = bundle["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 3);
    let find_mesh = |name: &str| meshes.iter().find(|m| m["name"] == json!(name)).unwrap();

    let statue_tex = find_mesh("statue")["texture_url"].as_str().unwrap();
    assert!(
        statue_tex.starts_with(&format!("/api/projects/{id}/mesh-asset?file=")),
        "expected ?file= on texture_url, got {statue_tex}"
    );
    let (sc, sh, _) = http_req(port, "GET", statue_tex, &format!("127.0.0.1:{port}"), None);
    assert_eq!(sc, 200);
    assert!(sh.contains("image/jpeg"), "head: {sh}");

    let pillar_tex = find_mesh("pillar")["texture_url"].as_str().unwrap();
    let (pc, ph, _) = http_req(port, "GET", pillar_tex, &format!("127.0.0.1:{port}"), None);
    assert_eq!(pc, 200);
    assert!(ph.contains("text/plain"), "head: {ph}");

    let crate_tex = find_mesh("crate")["texture_url"].as_str().unwrap();
    assert!(crate_tex.contains("art%2Ftextures%2Fcrate_albedo.png"));
    let (cc, ch, _) = http_req(port, "GET", crate_tex, &format!("127.0.0.1:{port}"), None);
    assert_eq!(cc, 200);
    assert!(ch.contains("image/png"), "head: {ch}");

    // Verify extracted lights
    let lights = bundle["lights"].as_array().unwrap();
    assert_eq!(lights.len(), 3, "lights: {lights:?}");

    let pt = lights
        .iter()
        .find(|l| l["path"] == json!("Workspace.Torch.FlameLight"))
        .unwrap();
    assert_eq!(pt["id"], json!("Workspace.Torch.FlameLight"));
    assert_eq!(pt["kind"], json!("PointLight"));
    assert_eq!(pt["parent_path"], json!("Workspace.Torch"));
    assert_eq!(pt["position"], json!([10.0, 6.0, -4.0]));
    assert_eq!(pt["color"], json!("#FF8833"));
    assert_eq!(pt["brightness"], json!(2.5));
    assert_eq!(pt["range"], json!(20.0));
    assert_eq!(pt["shadows"], json!(true));
    assert_eq!(pt["enabled"], json!(true));

    let spot = lights
        .iter()
        .find(|l| l["path"] == json!("Workspace.Torch.Beam"))
        .unwrap();
    assert_eq!(spot["kind"], json!("SpotLight"));
    assert_eq!(spot["angle"], json!(55.0));
    // Front ([0,0,-1]) rotated +90 deg around Y -> [-1, 0, 0]
    let dir = spot["direction"].as_array().unwrap();
    assert!((dir[0].as_f64().unwrap() - (-1.0)).abs() < 1e-3, "dir: {dir:?}");
    assert!(dir[2].as_f64().unwrap().abs() < 1e-3, "dir: {dir:?}");

    let surf = lights
        .iter()
        .find(|l| l["path"] == json!("Workspace.Torch.Panel"))
        .unwrap();
    assert_eq!(surf["kind"], json!("SurfaceLight"));
    assert_eq!(surf["enabled"], json!(false));
    assert_eq!(surf["direction"], json!([0.0, 1.0, 0.0]));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scene_mutate_set_terrain_generators_paint_box_luau_and_bounds_400() {
    let root = temp_dir("set-terrain");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Terrain Island");

    // 1. Test generator presets + paint_box
    for kind in ["flat", "hills", "island", "canyon"] {
        let (code, res) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/scene/mutate"),
            Some(json!({
                "ops": [{
                    "op": "set_terrain",
                    "enabled": true,
                    "generator": {
                        "kind": kind,
                        "seed": 42,
                        "amplitude": 28.0,
                        "water_level": 2.0
                    }
                }]
            })),
        );
        assert_eq!(code, 200, "generator {kind} failed: {res}");
        assert_eq!(res["terrain"]["enabled"], json!(true));
        assert_eq!(res["terrain"]["cell_size"], json!(4));
        assert_eq!(
            res["terrain"]["regions"][0]["generator"]["kind"],
            json!(kind)
        );
    }

    // Add a paint_box region
    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [{
                "op": "set_terrain",
                "paint_box": {
                    "id": "beach_sand",
                    "min": [-48.0, -4.0, -48.0],
                    "max": [48.0, 4.0, 48.0],
                    "material": "Sand"
                }
            }]
        })),
    );
    assert_eq!(code, 200, "paint_box failed: {res}");
    assert_eq!(res["terrain"]["regions"].as_array().unwrap().len(), 2);
    assert!(proj_dir.join("scene/terrain.json").is_file());
    assert!(proj_dir.join("src/server/OpenROTerrain.server.luau").is_file());

    // Emitted Luau script must pass static luau_check with 0 errors
    let luau_src =
        std::fs::read_to_string(proj_dir.join("src/server/OpenROTerrain.server.luau")).unwrap();
    let mut scripts = BTreeMap::new();
    scripts.insert("ServerScriptService.Server.OpenROTerrain".to_string(), luau_src.clone());
    let report = luau_check::analyze_place(&scripts, &BTreeMap::new());
    assert!(report.is_ok(), "OpenROTerrain.server.luau diagnostics: {:?}", report.diagnostics);
    assert!(luau_src.contains("Enum.Material"));
    assert!(luau_src.contains("FillBlock"));

    // Undo reverts paint_box cleanly
    let (code, undone) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "undo": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(undone["terrain"]["regions"].as_array().unwrap().len(), 1);

    // Redo restores paint_box cleanly
    let (code, redone) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "redo": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(redone["terrain"]["regions"].as_array().unwrap().len(), 2);

    // Clear removes regions and disabling cleans up OpenROTerrain.server.luau
    let (code, cleared) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "ops": [{ "op": "set_terrain", "clear": true }] })),
    );
    assert_eq!(code, 200);
    assert_eq!(cleared["terrain"]["enabled"], json!(false));
    assert_eq!(cleared["terrain"]["regions"], json!([]));
    assert!(!proj_dir.join("src/server/OpenROTerrain.server.luau").exists());

    // 2. Bounds enforcement -> HTTP 400
    for bad_op in [
        // Exceeds X > 2048
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [-1100.0, 0.0, -50.0], "max": [1100.0, 16.0, 50.0], "material": "Grass" }
        }),
        // Exceeds Y > 512
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, -300.0, 0.0], "max": [64.0, 300.0, 64.0], "material": "Rock" }
        }),
        // Exceeds Z > 2048
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [0.0, 0.0, 0.0], "max": [64.0, 16.0, 2049.0], "material": "Sand" }
        }),
        // Inverted bounds
        json!({
            "op": "set_terrain",
            "paint_box": { "min": [10.0, 0.0, 0.0], "max": [5.0, 8.0, 10.0], "material": "Grass" }
        }),
        // Invalid generator kind
        json!({
            "op": "set_terrain",
            "generator": { "kind": "volcano_invalid", "seed": 1, "amplitude": 10.0, "water_level": 0.0 }
        }),
    ] {
        let (code, err_res) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/scene/mutate"),
            Some(json!({ "ops": [bad_op] })),
        );
        assert_eq!(code, 400, "expected 400 for invalid terrain op, got {err_res}");
    }

    // > 100 regions -> HTTP 400
    let too_many: Vec<Value> = (0..101)
        .map(|i| {
            json!({
                "id": format!("r_{i}"),
                "bounds_min": [0.0, 0.0, 0.0],
                "bounds_max": [16.0, 8.0, 16.0],
                "material": "Grass"
            })
        })
        .collect();
    let (code, err_res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({ "ops": [{ "op": "set_terrain", "regions": too_many }] })),
    );
    assert_eq!(code, 400, "expected 400 for >100 regions, got {err_res}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scene_mutate_lights_pbr_fx_and_reparent() {
    let root = temp_dir("pbr-reparent");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "FX World");

    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [
                {
                    "op": "upsert",
                    "path": "Workspace.Props",
                    "class_name": "Folder"
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Stage",
                    "class_name": "Model"
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Props.Altar",
                    "class_name": "MeshPart",
                    "properties": { "Position": [0.0, 3.0, 0.0], "MeshId": "rbxassetid://101", "Anchored": true }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Props.Altar.Aura",
                    "class_name": "ParticleEmitter",
                    "properties": { "Rate": 24, "Texture": "rbxassetid://202", "Enabled": true }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Props.Altar.Chime",
                    "class_name": "Sound",
                    "properties": { "SoundId": "rbxassetid://303", "Volume": 0.6, "Looped": true }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Props.Altar.Surface",
                    "class_name": "SurfaceAppearance",
                    "properties": {
                        "ColorMap": "rbxassetid://404",
                        "NormalMap": "rbxassetid://405",
                        "RoughnessMap": "rbxassetid://406",
                        "MetalnessMap": "rbxassetid://407",
                        "AlphaMode": "Transparency"
                    }
                },
                {
                    "op": "upsert",
                    "path": "Workspace.Props.Altar.Glow",
                    "class_name": "PointLight",
                    "properties": { "Color": "#44DDFF", "Brightness": 1.8, "Range": 18.0 }
                },
                {
                    "op": "reparent",
                    "path": "Workspace.Props.Altar",
                    "new_parent": "Workspace.Stage"
                }
            ]
        })),
    );
    assert_eq!(code, 200, "mutate failed: {res}");
    assert!(res["instances"].get("Workspace.Props.Altar").is_none());
    assert!(res["instances"].get("Workspace.Stage.Altar").is_some());
    assert_eq!(
        res["instances"]["Workspace.Stage.Altar.Surface"]["class_name"],
        json!("SurfaceAppearance")
    );
    assert_eq!(
        res["instances"]["Workspace.Stage.Altar.Chime"]["class_name"],
        json!("Sound")
    );
    assert_eq!(
        res["instances"]["Workspace.Stage.Altar.Aura"]["class_name"],
        json!("ParticleEmitter")
    );
    assert_eq!(res["lights"].as_array().unwrap().len(), 1);
    assert_eq!(
        res["lights"][0]["path"],
        json!("Workspace.Stage.Altar.Glow")
    );

    // Reparent Altar directly under Workspace root
    let (code, res2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [{
                "op": "reparent",
                "path": "Workspace.Stage.Altar",
                "new_parent": "Workspace"
            }]
        })),
    );
    assert_eq!(code, 200);
    assert!(res2["instances"].get("Workspace.Altar").is_some());
    assert!(res2["instances"].get("Workspace.Stage.Altar").is_none());

    // Self/descendant reparent cycle -> 400
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/scene/mutate"),
        Some(json!({
            "ops": [{
                "op": "reparent",
                "path": "Workspace.Altar",
                "new_parent": "Workspace.Altar.Glow"
            }]
        })),
    );
    assert_eq!(code, 400);

    // Built-in rbxlx build succeeds cleanly with all classes
    let tree = projects::load_tree(&proj_dir, projects::DEFAULT_ROJO_FILE).unwrap();
    let mut warnings = Vec::new();
    let xml = projects::builtin_rbxlx(&tree, &mut warnings);
    assert!(xml.contains("class=\"SurfaceAppearance\""));
    assert!(xml.contains("class=\"ParticleEmitter\""));
    assert!(xml.contains("class=\"PointLight\""));
    assert!(xml.contains("<Content name=\"ColorMap\"><url>rbxassetid://404</url></Content>"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mesh_asset_file_query_security_and_content_types() {
    let root = temp_dir("mesh-asset-sec");
    let ws = root.join("workspace");
    let data = root.join("data");
    let outside = root.join("outside");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let port = start_server(None, data);
    let (id, proj_dir) = create_test_project(port, &ws, "Asset Jail");
    let host = format!("127.0.0.1:{port}");

    let meshes = proj_dir.join("assets/meshes/nested");
    let art = proj_dir.join("art/props");
    std::fs::create_dir_all(&meshes).unwrap();
    std::fs::create_dir_all(&art).unwrap();
    std::fs::create_dir_all(proj_dir.join("assets/other")).unwrap();

    std::fs::write(meshes.join("hero.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    std::fs::write(meshes.join("hero.mtl"), "newmtl hero\n").unwrap();
    std::fs::write(art.join("albedo.png"), b"\x89PNG\r\n\x1a\nPNGDATA").unwrap();
    std::fs::write(art.join("diffuse.jpg"), b"\xFF\xD8\xFF\xE0JPGDATA").unwrap();
    std::fs::write(art.join("bad.svg"), "<svg></svg>").unwrap();
    std::fs::write(art.join("bad.webp"), b"RIFFWEBP").unwrap();
    std::fs::write(proj_dir.join("assets/other/nope.png"), b"\x89PNG\r\n\x1a\nNOPE").unwrap();
    std::fs::write(meshes.join("oversized.obj"), vec![b'x'; 10 * 1024 * 1024 + 16]).unwrap();
    std::fs::write(outside.join("secret.png"), b"\x89PNG\r\n\x1a\nSECRET").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("secret.png"), meshes.join("esc.png")).unwrap();
        std::os::unix::fs::symlink(&outside, art.join("esc_dir")).unwrap();
        std::os::unix::fs::symlink(art.join("albedo.png"), meshes.join("local_sym.png")).unwrap();
    }

    // Valid files with ?file= return 200 and exact Content-Type
    for (file, expect_ct) in [
        ("assets/meshes/nested/hero.obj", "text/plain; charset=utf-8"),
        ("assets/meshes/nested/hero.mtl", "text/plain; charset=utf-8"),
        ("art/props/albedo.png", "image/png"),
        ("art/props/diffuse.jpg", "image/jpeg"),
    ] {
        let (code, head, _) = http_req(
            port,
            "GET",
            &format!("/api/projects/{id}/mesh-asset?file={file}"),
            &host,
            None,
        );
        assert_eq!(code, 200, "expected 200 for {file}");
        assert!(
            head.to_ascii_lowercase()
                .contains(&format!("content-type: {expect_ct}")),
            "expected Content-Type '{expect_ct}' for {file}, got:\n{head}"
        );
    }

    // CWE-22, outside allowed dirs, disallowed extensions, >10 MiB, symlinks -> 400
    for bad in [
        "",
        "../outside/secret.png",
        "assets/meshes/../../outside/secret.png",
        "assets/meshes\\nested\\hero.obj",
        "art/props/albedo.png%00.jpg",
        "src/server/Main.server.luau",
        "assets/other/nope.png",
        "art/props/bad.svg",
        "art/props/bad.webp",
        "openro.project.json",
        "assets/meshes/nested/oversized.obj",
        #[cfg(unix)]
        "assets/meshes/nested/esc.png",
        #[cfg(unix)]
        "art/props/esc_dir/secret.png",
        #[cfg(unix)]
        "assets/meshes/nested/local_sym.png",
    ] {
        let (code, _, _) = http_req(
            port,
            "GET",
            &format!("/api/projects/{id}/mesh-asset?file={bad}"),
            &host,
            None,
        );
        assert_eq!(code, 400, "expected 400 for mesh-asset ?file={bad}, got {code}");
    }

    // Non-loopback -> 403
    let (code, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/mesh-asset?file=art/props/albedo.png"),
        "evil.example.com",
        None,
    );
    assert_eq!(code, 403);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn agent_project_scene_mutate_schema_and_terrain_lights_execution() {
    for prompt in [system_prompt(), project_system_prompt()] {
        assert!(prompt.contains("project_scene_mutate"));
        assert!(prompt.contains("set_terrain"));
        assert!(prompt.contains("reparent"));
        assert!(prompt.contains("PointLight"));
        assert!(prompt.contains("SpotLight"));
        assert!(prompt.contains("SurfaceLight"));
        assert!(prompt.contains("ParticleEmitter"));
        assert!(prompt.contains("Sound"));
        assert!(prompt.contains("SurfaceAppearance"));
    }

    let root = temp_dir("agent-terrain-schema");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let stub_url = start_stub_model(|_body, seq| match seq {
        1 => json!({
            "tool": "project_scene_mutate",
            "args": {
                "ops": [
                    {
                        "op": "set_terrain",
                        "enabled": true,
                        "generator": {
                            "kind": "island",
                            "seed": 7,
                            "amplitude": 30.0,
                            "water_level": 1.5
                        }
                    },
                    {
                        "op": "upsert",
                        "path": "Workspace.LanternGroup",
                        "class_name": "Folder"
                    },
                    {
                        "op": "upsert",
                        "path": "Workspace.Lantern",
                        "class_name": "Part",
                        "properties": { "Position": [0.0, 5.0, 0.0], "Anchored": true }
                    },
                    {
                        "op": "upsert",
                        "path": "Workspace.Lantern.CoreLight",
                        "class_name": "PointLight",
                        "properties": { "Color": "#FFD27D", "Brightness": 2.0, "Range": 24.0 }
                    },
                    {
                        "op": "reparent",
                        "path": "Workspace.Lantern",
                        "new_parent": "Workspace.LanternGroup"
                    }
                ]
            }
        }),
        _ => json!({ "done": true, "summary": "configured terrain and lantern" }),
    });

    let mut store = projects::ProjectStore::load(Some(&data));
    store.add_root(&ws.to_string_lossy()).unwrap();
    let pref = store.create("Agent Terrain Game", None, None, None).unwrap();

    let mut agent = Agent::with_local_place(
        ModelClient::new(stub_url, "test".into(), None),
        LocalPlace::open(data.clone(), "Agent Terrain Game".into()),
    );
    agent.set_project(Some(pref.dir.clone()), Some(pref.id.clone()));

    let out = agent
        .run_ask_with_options("chat-terrain", "Build island terrain and lantern", AskOptions::default())
        .expect("agent run");
    assert!(out.summary.contains("terrain"));
    assert!(pref.dir.join(scene::TERRAIN_FILE_REL).is_file());
    assert!(pref.dir.join(scene::TERRAIN_LUAU_REL).is_file());

    let bundle = projects::load_workspace_bundle(&pref.dir, &pref.id).unwrap();
    assert_eq!(bundle["terrain"]["enabled"], json!(true));
    assert_eq!(
        bundle["terrain"]["regions"][0]["generator"]["kind"],
        json!("island")
    );
    assert_eq!(
        bundle["lights"][0]["path"],
        json!("Workspace.LanternGroup.Lantern.CoreLight")
    );

    // Also verify teamwork tool_project_scene_mutate with terrain_only claim on scene/terrain.json
    let pref_tw = store.create("Team Terrain Game", None, None, None).unwrap();
    let tw_stub = start_stub_model(|body, _seq| {
        if body.contains("[Role: coordinator]") {
            if !body.contains("tool_result (create_task)") && !body.contains("\"t1\"") {
                json!({
                    "tool": "create_task",
                    "args": {
                        "title": "Paint Arena Terrain",
                        "owner": "builder",
                        "verifier": "verifier",
                        "kind": "docs",
                        "files": ["scene/terrain.json"]
                    }
                })
            } else if body.contains("\"done\"") || body.contains("Terrain verified") {
                json!({ "tool": "finish_run", "args": { "summary": "terrain painted" } })
            } else {
                json!({ "done": true, "summary": "waiting" })
            }
        } else if body.contains("[Role: verifier]") {
            if !body.contains("tool_result (project_scene_get)") {
                json!({ "tool": "project_scene_get", "args": {} })
            } else {
                json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "Terrain verified" } })
            }
        } else if !body.contains("tool_result (project_scene_mutate)") {
            json!({
                "tool": "project_scene_mutate",
                "args": {
                    "task_id": "t1",
                    "ops": [{
                        "op": "set_terrain",
                        "enabled": false,
                        "paint_box": {
                            "id": "spawn_snow",
                            "min": [-24.0, 0.0, -24.0],
                            "max": [24.0, 6.0, 24.0],
                            "material": "Snow"
                        }
                    }]
                }
            })
        } else {
            json!({ "tool": "submit_task", "args": { "task_id": "t1", "summary": "painted snow pad" } })
        }
    });
    let tw_base = tw_stub.trim_end_matches("/chat/completions").to_string();
    let srv_agent = Agent::with_local_place(
        ModelClient::new(tw_stub, "stub-model".into(), None),
        LocalPlace::open(data.clone(), "Team Terrain Game".into()),
    );
    let port = start_server(Some(srv_agent), data);
    let (cfg_code, _) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({ "base_url": tw_base, "model_id": "stub-model" })),
    );
    assert_eq!(cfg_code, 200);
    let (code, started) = api_json(
        port,
        "POST",
        "/api/teamwork",
        Some(json!({
            "goal": "Paint snow pad",
            "project_id": pref_tw.id,
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert_eq!(code, 201, "teamwork start failed: {started}");
    let run_id = started["run"]["id"].as_str().unwrap().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut final_status = String::new();
    while std::time::Instant::now() < deadline {
        let (_, st) = api_json(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        final_status = st["run"]["status"].as_str().unwrap_or("").to_string();
        if matches!(final_status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    assert_eq!(final_status, "completed");
    let after = projects::load_workspace_bundle(&pref_tw.dir, &pref_tw.id).unwrap();
    assert!(
        after["terrain"]["regions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == json!("spawn_snow")),
        "expected spawn_snow region in terrain: {:?}",
        after["terrain"]
    );

    let _ = std::fs::remove_dir_all(&root);
}
