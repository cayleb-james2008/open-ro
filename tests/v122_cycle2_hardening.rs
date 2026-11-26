//! Cycle 2 Wave H1 Hardening, Security & Concurrency Integration Tests (v1.22.0):
//! - CWE-22/59 symlink & path traversal containment on `mesh-asset`, `scene/mutate`, and `gui/:slug`
//! - Deeply nested `ScreenDoc`, excessive node/prop counts, `NaN`/Inf rejection, and `UDim2`/color clamping
//! - Malformed JSON, oversized payloads, undo/redo stack cap (50), corrupt state recovery, and concurrent mutation resilience
//! - Sandbox timeout/runaway loop, malformed Luau (`Script`, `ModuleScript`, `assert_luau`), and duplicate `ProcessReceipt` idempotency edge cases

use openro_rs::gui_designer::{
    analyze_safe_areas, compile_project_screen, compile_screen_doc, list_project_screens,
    load_project_screen, parse_and_validate_screen_str, preset_screen_doc, save_project_screen,
    LocalGuiStore, MAX_GUI_DEPTH, MAX_GUI_NODES,
};
use openro_rs::luau_check;
use openro_rs::projects;
use openro_rs::sandbox::{
    find_lune_binary, run_local_place_sandbox, run_project_sandbox,
};
use openro_rs::scene::{mutate_scene, scan_project_meshes, undo_status};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v122-cycle2-hardening");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn start_server(data_dir: PathBuf) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(None, "Hardening Game".to_string(), data_dir);
    let sh_clone = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, sh_clone);
    });
    (port, shared)
}

fn http_raw(port: u16, raw: &[u8]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(raw).expect("write");
    stream.flush().ok();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = if sep + 4 <= buf.len() {
        buf[sep + 4..].to_vec()
    } else {
        Vec::new()
    };
    (status, head, body)
}

fn http_req(
    port: u16,
    method: &str,
    path: &str,
    host: &str,
    body: Option<&str>,
) -> (u16, Value) {
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
    let (code, _, bytes) = http_raw(port, req.as_bytes());
    let parsed = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    (code, parsed)
}

fn create_test_project(port: u16, ws: &Path, name: &str) -> (String, PathBuf) {
    let host = format!("127.0.0.1:{port}");
    let (c1, _) = http_req(
        port,
        "POST",
        "/api/projects/workspace",
        &host,
        Some(&json!({ "path": ws.to_string_lossy() }).to_string()),
    );
    assert_eq!(c1, 200);
    let (c2, res) = http_req(
        port,
        "POST",
        "/api/projects/create",
        &host,
        Some(&json!({ "name": name, "parent": ws.to_string_lossy() }).to_string()),
    );
    assert_eq!(c2, 201);
    let id = res["project"]["id"].as_str().unwrap().to_string();
    let dir = PathBuf::from(res["project"]["path"].as_str().unwrap());
    (id, dir)
}

#[test]
fn cwe22_59_containment_across_mesh_scene_and_gui() {
    let root = temp_dir("cwe22-59");
    let ws = root.join("ws");
    let data = root.join("data");
    let outside = root.join("outside");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    write_file(&outside.join("secret.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    write_file(&outside.join("secret.json"), r#"{"secret":true}"#);
    write_file(&outside.join("screen.screen.json"), &preset_screen_doc("rpg", None).unwrap().to_json().to_string());
    write_file(&outside.join("evil_target.txt"), "UNTAMPERED");

    let (port, _shared) = start_server(data.clone());
    let host = format!("127.0.0.1:{port}");
    let (pid, pdir) = create_test_project(port, &ws, "JailGame");

    // 1. mesh-asset traversal, NUL, backslash, symlinked directory & file
    let meshes = pdir.join("assets/meshes");
    std::fs::create_dir_all(&meshes).unwrap();
    write_file(&meshes.join("valid.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, meshes.join("symdir")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.obj"), meshes.join("symfile.obj")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("secret.json"),
            meshes.join("manifest.json"),
        )
        .unwrap();
    }

    // scan_project_meshes must skip symlinks and only return valid.obj
    let scanned = scan_project_meshes(&pdir, &pid);
    assert_eq!(scanned.len(), 1, "expected only valid.obj, got: {scanned:?}");
    assert_eq!(scanned[0]["name"], json!("valid"));

    for bad_path in [
        "assets\\..\\..\\outside\\secret.obj",
        "assets/meshes/valid.obj%00.png",
        "%2e%2e%2foutside%2fsecret.obj",
        "assets/meshes/%2e%2e/%2e%2e/outside/secret.obj",
        "assets/meshes/symdir/secret.obj",
        "assets/meshes/symfile.obj",
        ".git/HEAD",
        ".openro/scene-undo.json",
    ] {
        let (code, _) = http_req(
            port,
            "GET",
            &format!("/api/projects/{pid}/mesh-asset?path={bad_path}"),
            &host,
            None,
        );
        assert_eq!(code, 400, "mesh-asset must reject '{bad_path}' with 400, got {code}");
    }

    // 2. scene/mutate CWE-59 symlinks (scene/workspace.model.json, .openro/scene-undo.json, pre-existing tmp symlink)
    #[cfg(unix)]
    {
        std::fs::create_dir_all(pdir.join("scene")).unwrap();
        std::fs::create_dir_all(pdir.join(".openro")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("secret.json"),
            pdir.join("scene/workspace.model.json"),
        )
        .unwrap();
        let err = mutate_scene(
            &pdir,
            &pid,
            &json!({ "ops": [{ "op": "upsert", "path": "Workspace.P1" }] }),
        )
        .expect_err("symlinked workspace.model.json must be rejected");
        assert!(err.contains("symlink"), "got: {err}");
        let _ = std::fs::remove_file(pdir.join("scene/workspace.model.json"));

        // Pre-existing legacy tmp symlink must never be written to
        std::os::unix::fs::symlink(
            outside.join("evil_target.txt"),
            pdir.join("scene/.workspace.model.json.openro-tmp"),
        )
        .unwrap();
        mutate_scene(
            &pdir,
            &pid,
            &json!({ "ops": [{ "op": "upsert", "path": "Workspace.P1" }] }),
        )
        .expect("unique atomic tmp write succeeds despite legacy tmp symlink");
        assert_eq!(
            std::fs::read_to_string(outside.join("evil_target.txt")).unwrap(),
            "UNTAMPERED"
        );
    }

    // 3. gui/:slug CWE-22/59 & out_path confinement
    let valid_doc = preset_screen_doc("obby", None).unwrap().to_json();
    for bad_slug in [
        "..",
        ".",
        "%2e%2e",
        "..%2Fsecret",
        "bad%5Cslug",
        "hud%00",
        "-leading-dash",
    ] {
        let (code, _) = http_req(
            port,
            "POST",
            &format!("/api/projects/{pid}/gui/{bad_slug}"),
            &host,
            Some(&json!({ "doc": valid_doc }).to_string()),
        );
        assert_eq!(code, 400, "gui/:slug must reject '{bad_slug}'");
    }

    // Non-string slug on /api/studio/gui must fail
    let (code_ns, _) = http_req(
        port,
        "POST",
        "/api/studio/gui",
        &host,
        Some(&json!({ "slug": 123, "preset": "obby" }).to_string()),
    );
    assert_eq!(code_ns, 400);

    // out_path confinement on gui compile
    for bad_out in [
        "../escape.luau",
        "src\\..\\escape.luau",
        ".git/hooks/post-checkout.luau",
        ".openro/inject.luau",
        "build/out.luau",
        "src/client/ui/Hud.txt",
        "src/client/ui/Hud\0.luau",
    ] {
        let err = compile_project_screen(
            &pdir,
            "safe-hud",
            &json!({ "doc": valid_doc, "out_path": bad_out }),
        )
        .expect_err("disallowed out_path must fail");
        assert!(!err.is_empty());
    }

    // Symlinked gui file skipped in list_project_screens & LocalGuiStore::open, rejected on load
    #[cfg(unix)]
    {
        std::fs::create_dir_all(pdir.join("ui")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("screen.screen.json"),
            pdir.join("ui/symlinked.screen.json"),
        )
        .unwrap();
        let listed = list_project_screens(&pdir).unwrap();
        assert!(
            listed["screens"].as_array().unwrap().is_empty(),
            "symlinked screen must be skipped in list_project_screens: {listed}"
        );
        let err = load_project_screen(&pdir, "symlinked")
            .expect_err("symlinked screen must be rejected on load");
        assert!(err.contains("symlink"), "got: {err}");

        let store = LocalGuiStore::open(Some(&pdir));
        assert!(store.list().is_empty());
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn gui_designer_bounds_depth_node_cap_nan_and_color_clamping() {
    // 1. Deeply nested ScreenDoc (> MAX_GUI_DEPTH) rejected cleanly without stack overflow
    let mut deep = json!({ "name": "Leaf", "class_name": "Frame", "props": {} });
    for i in 0..=(MAX_GUI_DEPTH + 5) {
        deep = json!({
            "name": format!("Layer{i}"),
            "class_name": "Frame",
            "props": {},
            "children": [deep]
        });
    }
    let deep_doc = json!({
        "version": 1,
        "name": "DeepHud",
        "root": deep
    });
    let err = parse_and_validate_screen_str(&deep_doc.to_string())
        .expect_err("over-deep ScreenDoc must be rejected");
    assert!(err.contains("depth"), "unexpected err: {err}");

    // 2. Excessive node count (> MAX_GUI_NODES) rejected cleanly
    let many_children: Vec<Value> = (0..=MAX_GUI_NODES)
        .map(|i| json!({ "name": format!("Child{i}"), "class_name": "Frame", "props": {} }))
        .collect();
    let wide_doc = json!({
        "version": 1,
        "name": "WideHud",
        "root": {
            "name": "Root",
            "class_name": "Frame",
            "props": {},
            "children": many_children
        }
    });
    let err = parse_and_validate_screen_str(&wide_doc.to_string())
        .expect_err("excessive node count must be rejected");
    assert!(err.contains("node count"), "unexpected err: {err}");

    // 3. Version u64 truncation bypass (4294967297 == (1<<32)+1) must be rejected
    let trunc_ver = json!({
        "version": 4_294_967_297_u64,
        "name": "TruncVer",
        "root": { "name": "Root", "class_name": "Frame", "props": {} }
    });
    assert!(parse_and_validate_screen_str(&trunc_ver.to_string()).is_err());

    // 4. NaN / Inf in UDim2 strings or objects must be rejected (no silent 0.0 fallback)
    for bad_udim in [
        json!("NaN, 0, 1, 0"),
        json!("inf, 0, 1, 0"),
        json!({ "xs": "NaN", "xo": 0, "ys": 1, "yo": 0 }),
        json!([[ "NaN", 0 ], [ 1, 0 ]]),
    ] {
        let bad = json!({
            "version": 1,
            "name": "BadUdim",
            "root": {
                "name": "Root",
                "class_name": "Frame",
                "props": { "Position": bad_udim }
            }
        });
        assert!(
            parse_and_validate_screen_str(&bad.to_string()).is_err(),
            "expected NaN/Inf UDim2 to fail: {bad_udim}"
        );
    }

    // 5. Out-of-range UDim2, numeric props, and colors are clamped safely & compile cleanly
    let extreme = json!({
        "version": 1,
        "name": "ExtremeClamped",
        "root": {
            "name": "Root",
            "class_name": "Frame",
            "props": {
                "Position": [[500.0, 999999.0], [-500.0, -999999.0]],
                "Size": [[1, 0], [1, 0]],
                "AnchorPoint": [50.0, -50.0],
                "BackgroundTransparency": 5.0,
                "BackgroundColor3": [300, -10, 500]
            },
            "children": [{
                "name": "ActionBtn",
                "class_name": "TextButton",
                "props": {
                    "Position": [[0, 20], [0, 80]],
                    "Size": [[0, 140], [0, 50]],
                    "BackgroundColor3": { "Color3": [2.0, -0.5, 0.5] },
                    "TextColor3": { "Color3uint8": [400, -25, 128] },
                    "Text": "Go",
                    "TextSize": 9999.0,
                    "TextTransparency": -3.0,
                    "Rotation": 50000.0
                }
            }]
        }
    });
    let doc = parse_and_validate_screen_str(&extreme.to_string()).expect("clamped doc validates");
    assert_eq!(
        doc.root.props.get("Position"),
        Some(&json!([[10.0, 16384.0], [-10.0, -16384.0]]))
    );
    assert_eq!(doc.root.props.get("AnchorPoint"), Some(&json!([10.0, -10.0])));
    assert_eq!(doc.root.props.get("BackgroundTransparency"), Some(&json!(1.0)));
    assert_eq!(doc.root.props.get("BackgroundColor3"), Some(&json!("#FF00FF")));

    let btn = &doc.root.children[0];
    assert_eq!(btn.props.get("BackgroundColor3"), Some(&json!("#FF0080")));
    assert_eq!(btn.props.get("TextColor3"), Some(&json!("#FF0080")));
    assert_eq!(btn.props.get("TextSize"), Some(&json!(256.0)));
    assert_eq!(btn.props.get("TextTransparency"), Some(&json!(0.0)));
    assert_eq!(btn.props.get("Rotation"), Some(&json!(3600.0)));

    let analysis = analyze_safe_areas(&doc);
    for dev in analysis.devices.values() {
        for n in &dev.nodes {
            assert!(n.bounds.x.is_finite() && n.bounds.y.is_finite());
            assert!(n.bounds.w.is_finite() && n.bounds.h.is_finite());
        }
    }

    let compiled = compile_screen_doc("extreme-clamped", &doc, None).unwrap();
    let luau = compiled["luau"].as_str().unwrap();
    assert!(luau.contains("UDim2.new(10, 16384, -10, -16384)"));
    assert!(luau.contains("Color3.fromRGB(255, 0, 128)"));
    let check = luau_check::analyze_source("StarterPlayer.StarterPlayerScripts.Client.ui.ExtremeClamped", luau);
    assert!(check.is_ok(), "diagnostics: {:?}", check.diagnostics);
}

#[test]
fn scene_malformed_oversized_undo_cap_and_concurrent_mutations() {
    let root = temp_dir("scene-hardening");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let (port, _shared) = start_server(data.clone());
    let host = format!("127.0.0.1:{port}");
    let (pid, pdir) = create_test_project(port, &ws, "SceneHardening");

    // 1. Malformed JSON -> 400
    let (c_bad_json, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        &host,
        Some("{not-valid-json"),
    );
    assert_eq!(c_bad_json, 400);

    // 2. Oversized payload (> 1 MB) -> 413 Payload Too Large
    let big_body = format!(r#"{{"ops":[{{"op":"upsert","path":"Workspace.Big","properties":{{"Note":"{}"}}}}]}}"#, "A".repeat(1_200_000));
    let (c_413, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/scene/mutate"),
        &host,
        Some(&big_body),
    );
    assert_eq!(c_413, 413);

    // 3. Invalid scene ops & ambiguous undo+redo rejected cleanly (no panic)
    for bad_body in [
        json!([]),
        json!({ "undo": true, "redo": true }),
        json!({ "ops": [] }),
        json!({ "ops": [{ "op": "upsert", "path": "ReplicatedStorage.Bad" }] }),
        json!({ "ops": [{ "op": "upsert", "path": "Workspace.Bad", "properties": "not-an-object" }] }),
        json!({ "ops": [{ "op": "upsert", "path": "Workspace.Bad", "class_name": "Bad\nClass" }] }),
        json!({ "ops": [{ "op": "duplicate", "path": "Workspace.Missing", "offset": "bad" }] }),
    ] {
        assert!(mutate_scene(&pdir, &pid, &bad_body).is_err(), "expected err for {bad_body}");
    }

    // Too many ops (>500) rejected
    let too_many_ops: Vec<Value> = (0..501)
        .map(|i| json!({ "op": "upsert", "path": format!("Workspace.P{i}") }))
        .collect();
    assert!(mutate_scene(&pdir, &pid, &json!({ "ops": too_many_ops })).is_err());

    // 4. Undo stack cap (MAX_UNDO_CHECKPOINTS = 50)
    for i in 0..65 {
        mutate_scene(
            &pdir,
            &pid,
            &json!({
                "ops": [{
                    "op": "upsert",
                    "path": format!("Workspace.Step{i}"),
                    "class_name": "Part",
                    "properties": { "Position": [i as f64, 1.0, 0.0] }
                }]
            }),
        )
        .unwrap();
    }
    let (rev, can_undo, can_redo) = undo_status(&pdir);
    assert_eq!(rev, 65);
    assert!(can_undo);
    assert!(!can_redo);

    for _ in 0..50 {
        mutate_scene(&pdir, &pid, &json!({ "undo": true })).unwrap();
    }
    let err_51 = mutate_scene(&pdir, &pid, &json!({ "undo": true }))
        .expect_err("51st undo must fail because undo stack is capped at 50");
    assert!(err_51.contains("nothing to undo"), "got: {err_51}");

    // Redo works up to 50 times
    for _ in 0..50 {
        mutate_scene(&pdir, &pid, &json!({ "redo": true })).unwrap();
    }
    assert!(mutate_scene(&pdir, &pid, &json!({ "redo": true })).is_err());

    // 5. Corrupt scene/workspace.model.json recovers with Err instead of panicking
    let orig_ws = std::fs::read_to_string(pdir.join("scene/workspace.model.json")).unwrap();
    std::fs::write(
        pdir.join("scene/workspace.model.json"),
        r#"{"className":"Workspace","children":{}}"#,
    )
    .unwrap();
    assert!(
        mutate_scene(
            &pdir,
            &pid,
            &json!({ "ops": [{ "op": "upsert", "path": "Workspace.Test" }] })
        )
        .is_err()
    );
    std::fs::write(pdir.join("scene/workspace.model.json"), orig_ws).unwrap();

    // 6. Concurrent scene mutations + concurrent GUI saves/compiles from 8 threads
    let mut handles = Vec::new();
    for t in 0..8 {
        let dir_c = pdir.clone();
        let pid_c = pid.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..4 {
                mutate_scene(
                    &dir_c,
                    &pid_c,
                    &json!({
                        "ops": [{
                            "op": "upsert",
                            "path": format!("Workspace.Concurrent.T{t}_I{i}"),
                            "class_name": "Part",
                            "properties": { "Position": [t as f64, i as f64, 0.0] }
                        }]
                    }),
                )
                .expect("concurrent scene mutate succeeds");
            }
            let doc = preset_screen_doc("obby", Some("shared-hud")).unwrap();
            save_project_screen(&dir_c, "shared-hud", &json!({ "doc": doc.to_json() }))
                .expect("concurrent gui save succeeds");
            compile_project_screen(&dir_c, "shared-hud", &json!({ "emit_model_json": true }))
                .expect("concurrent gui compile succeeds");
        }));
    }
    for h in handles {
        h.join().expect("worker thread joined cleanly");
    }

    let bundle = projects::load_workspace_bundle(&pdir, &pid).unwrap();
    for t in 0..8 {
        for i in 0..4 {
            let key = format!("Workspace.Concurrent.T{t}_I{i}");
            assert!(
                bundle["instances"].get(&key).is_some(),
                "missing concurrent instance {key}"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sandbox_timeout_malformed_luau_and_receipt_idempotency_edge_cases() {
    let (lune, searched) = find_lune_binary();
    assert!(
        lune.is_some(),
        "lune binary required for sandbox hardening tests (searched: {searched:?})"
    );

    // 1. Parameter validation (invalid timeout, oversized players/journey)
    let lp_scripts = BTreeMap::new();
    let lp_instances = BTreeMap::new();
    for bad_req in [
        json!({ "timeout_secs": -5 }),
        json!({ "timeout_secs": "fast" }),
        json!({ "players": (0..40).map(|i| json!({ "user_id": i })).collect::<Vec<_>>() }),
        json!({ "journey": (0..250).map(|_| json!({ "op": "wait", "ms": 1 })).collect::<Vec<_>>() }),
    ] {
        assert!(
            run_local_place_sandbox("BadReq", &lp_scripts, &lp_instances, &bad_req).is_err(),
            "expected invalid sandbox req to fail: {bad_req}"
        );
    }

    // 2. Runaway loop hits 1s timeout cleanly
    let mut runaway_scripts = BTreeMap::new();
    runaway_scripts.insert(
        "ServerScriptService.Spin.server".to_string(),
        "while true do end\n".to_string(),
    );
    let timed_out = run_local_place_sandbox(
        "Runaway",
        &runaway_scripts,
        &BTreeMap::new(),
        &json!({ "timeout_secs": 1 }),
    )
    .expect("timeout returns structured report");
    assert_eq!(timed_out["ok"], json!(false));
    assert_eq!(timed_out["passed"], json!(false));
    assert_eq!(timed_out["timed_out"], json!(true));

    // 3. Malformed Luau in Script, ModuleScript, and assert_luau
    let mut bad_luau_scripts = BTreeMap::new();
    bad_luau_scripts.insert(
        "ReplicatedStorage.BrokenMod.module".to_string(),
        "local x = (\n".to_string(),
    );
    bad_luau_scripts.insert(
        "ServerScriptService.Main.server".to_string(),
        r#"local ReplicatedStorage = game:GetService("ReplicatedStorage")
require(ReplicatedStorage:WaitForChild("BrokenMod"))
"#
        .to_string(),
    );
    bad_luau_scripts.insert(
        "ServerScriptService.SyntaxBad.server".to_string(),
        "function oops(\n".to_string(),
    );
    let syntax_rep = run_local_place_sandbox(
        "BadLuau",
        &bad_luau_scripts,
        &BTreeMap::new(),
        &json!({
            "journey": [
                { "op": "assert_luau", "name": "BadAssert", "luau": "return ((" }
            ]
        }),
    )
    .expect("malformed Luau produces structured report");
    assert_eq!(syntax_rep["ok"], json!(true));
    assert_eq!(syntax_rep["passed"], json!(false));
    let errs = syntax_rep["console"]["errors"].as_array().unwrap();
    assert!(errs.len() >= 3, "expected errors from script, module, and assert: {errs:?}");

    // 4. Duplicate ProcessReceipt idempotency edge cases:
    // (a) Non-adjacent duplicate PurchaseId in separate steps MUST fail if double-granted
    let dir_non_adj = temp_dir("rcpt-non-adj");
    write_file(
        &dir_non_adj.join("default.project.json"),
        r#"{
  "name": "NonAdjDoubleGrant",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } }
  }
}"#,
    );
    write_file(
        &dir_non_adj.join("src/server/Shop.server.luau"),
        r#"local MarketplaceService = game:GetService("MarketplaceService")
local DataStoreService = game:GetService("DataStoreService")
local store = DataStoreService:GetDataStore("Wallet")
MarketplaceService.ProcessReceipt = function(info)
	-- Mutates receiptInfo AND double-grants in multi-key DataStore table!
	info.PurchaseId = "tampered"
	store:UpdateAsync("u_" .. tostring(info.PlayerId), function(cur)
		local tbl = cur or { Coins = 0, Gems = 0, Purchases = 0 }
		tbl.Coins += 100
		tbl.Gems += 5
		tbl.Purchases += 1
		return tbl
	end)
	return Enum.ProductPurchaseDecision.PurchaseGranted
end
"#,
    );
    let non_adj_rep = run_project_sandbox(
        &dir_non_adj,
        &json!({
            "journey": [
                { "op": "purchase_receipt", "player_id": 1001, "product_id": 55, "purchase_id": "rcpt_dup_x", "replay_duplicate": false },
                { "op": "wait", "ms": 20 },
                { "op": "purchase_receipt", "player_id": 1001, "product_id": 55, "purchase_id": "rcpt_dup_x", "replay_duplicate": false }
            ]
        }),
    )
    .unwrap();
    assert_eq!(
        non_adj_rep["passed"],
        json!(false),
        "non-adjacent duplicate PurchaseId double-grant must fail: {non_adj_rep}"
    );

    // (b) Idempotent ProcessReceipt with multi-key DataStore table & receiptInfo mutation MUST pass
    let dir_idem = temp_dir("rcpt-idem-ok");
    write_file(
        &dir_idem.join("default.project.json"),
        r#"{
  "name": "IdemOk",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } }
  }
}"#,
    );
    write_file(
        &dir_idem.join("src/server/Shop.server.luau"),
        r#"local MarketplaceService = game:GetService("MarketplaceService")
local DataStoreService = game:GetService("DataStoreService")
local store = DataStoreService:GetDataStore("Wallet")
local seen = {}
MarketplaceService.ProcessReceipt = function(info)
	local pid = info.PurchaseId
	info.PurchaseId = "mutated_by_callback"
	if seen[pid] then
		return Enum.ProductPurchaseDecision.PurchaseGranted
	end
	seen[pid] = true
	store:UpdateAsync("u_" .. tostring(info.PlayerId), function(cur)
		local tbl = cur or { Coins = 0, Gems = 0, Rank = "Bronze", Tag = "VIP" }
		tbl.Coins += 100
		tbl.Gems += 10
		return tbl
	end)
	return Enum.ProductPurchaseDecision.PurchaseGranted
end
"#,
    );
    let idem_rep = run_project_sandbox(
        &dir_idem,
        &json!({
            "journey": [
                { "op": "purchase_receipt", "player_id": 1001, "product_id": 55, "purchase_id": "rcpt_ok_1", "replay_duplicate": true },
                { "op": "purchase_receipt", "player_id": 1001, "product_id": 55, "purchase_id": "rcpt_ok_1", "replay_duplicate": false },
                { "op": "assert_luau", "name": "GrantedOnce", "luau": "return state_snapshot.datastores.Wallet.u_1001.Coins == 100", "expect": { "equals": true } }
            ]
        }),
    )
    .unwrap();
    assert_eq!(
        idem_rep["passed"],
        json!(true),
        "idempotent multi-key DataStore ProcessReceipt must pass: {idem_rep}"
    );

    let _ = std::fs::remove_dir_all(&dir_non_adj);
    let _ = std::fs::remove_dir_all(&dir_idem);
}
