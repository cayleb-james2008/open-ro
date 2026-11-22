//! v0.11 UV-textured stylized Blender mesh pipeline integration tests:
//! - `GET /api/tools/blender/status` + legacy preset compatibility on `POST /api/tools/blender/generate`
//! - Real headless Blender run for a structured multi-shape prop (with glow part, UVs, Cycles AO bake,
//!   manifest bbox/anchors, 2-angle preview PNGs, served via `GET /api/assets/meshes/:file`)
//! - Agent `script` mode + `out_dir` under `<data_dir>/meshes-out` + traversal refusal
//! - Hard 400 refusal for >20,000 triangle script (with zero leaked output files)
//! - Hard 400 refusal for denylisted script tokens (`os.system`, `subprocess`, `socket`, `urllib`,
//!   `shutil.rmtree`, `__import__`)

use openro_rs::agent::Agent;
use openro_rs::library::detect_blender;
use openro_rs::mesh::{validate_script, MAX_TRIS};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let root = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
            .join("v011-mesh-tests");
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(format!("{label}-{ts}-{seq}"));
        std::fs::create_dir_all(&dir).expect("create test data dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start_local_server(data_dir: PathBuf) -> u16 {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = LocalPlace::open(data_dir.clone(), "MeshPipelineTest".to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::with_data_dir(Some(agent), "MeshPipelineTest".to_string(), data_dir);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral loopback port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn http_raw(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: &[u8],
    timeout_secs: u64,
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let header = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(header.as_bytes()).unwrap();
    if !body.is_empty() {
        stream.write_all(body).unwrap();
    }
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).unwrap();
    let split = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("valid HTTP response");
    let head = String::from_utf8_lossy(&resp[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .expect("HTTP status code");
    (status, head, resp[split + 4..].to_vec())
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>, timeout_secs: u64) -> (u16, Value) {
    let payload = body
        .map(|v| serde_json::to_vec(v).unwrap())
        .unwrap_or_default();
    let (status, _head, bytes) = http_raw(
        port,
        &format!("127.0.0.1:{port}"),
        method,
        path,
        &payload,
        timeout_secs,
    );
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "invalid JSON response (status {status}): {e}: {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, parsed)
}

#[test]
fn blender_status_and_legacy_preset_route_work() {
    let td = TestDir::new("status-legacy");
    let port = start_local_server(td.path().to_path_buf());

    let (st, status_json) = http_json(port, "GET", "/api/tools/blender/status", None, 15);
    assert_eq!(st, 200, "status failed: {status_json}");
    assert_eq!(status_json["ok"], json!(true));
    assert_eq!(status_json["max_tris"], json!(MAX_TRIS));
    assert_eq!(status_json["script_timeout_secs"], json!(90));
    let shapes: Vec<&str> = status_json["supported_shapes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        shapes,
        vec!["box", "cylinder", "cone", "sphere", "wedge", "torus"]
    );

    // Old preset body still routes to library::generate_blender_mesh.
    let (st_old, legacy) = http_json(
        port,
        "POST",
        "/api/tools/blender/generate",
        Some(&json!({
            "preset": "card_pedestal",
            "name": "LegacyStand",
            "use_fallback": true,
            "render_preview": true
        })),
        15,
    );
    assert_eq!(st_old, 200, "legacy preset failed: {legacy}");
    assert_eq!(legacy["ok"], json!(true));
    assert_eq!(legacy["preset"], json!("card_pedestal"));
    assert!(legacy["obj_url"].as_str().unwrap().starts_with("/api/assets/meshes/"));
}

#[test]
fn structured_mesh_real_blender_produces_uv_baked_png_glow_manifest_and_previews() {
    let det = detect_blender(None);
    if !det.detected {
        eprintln!(
            "SKIPPED structured_mesh_real_blender_produces_uv_baked_png_glow_manifest_and_previews: Blender binary not installed on PATH or standard paths"
        );
        return;
    }

    let td = TestDir::new("structured-real");
    let port = start_local_server(td.path().to_path_buf());

    // Prop with exact combined Y-up bbox: X in [-3, 3] (6), Y in [0, 6] (6), Z in [-2, 2] (4).
    // Exercises default bevel (0.03), all 6 supported shapes, a glow part, and a custom anchor.
    let body = json!({
        "name": "shrine_lantern",
        "texture_size": 256,
        "preview_size": 256,
        "anchors": [
            { "name": "orb", "pos": [0.0, 5.5, 0.0] }
        ],
        "parts": [
            { "shape": "box",      "size": [6.0, 1.0, 4.0], "pos": [0.0, 0.5, 0.0],  "rot": [0, 0, 0], "color": "#8b8378" },
            { "shape": "cylinder", "size": [1.4, 3.0, 1.4], "pos": [0.0, 2.5, 0.0],  "rot": [0, 0, 0], "color": "#c91f28", "segments": 16 },
            { "shape": "wedge",    "size": [1.2, 1.0, 1.2], "pos": [-1.8, 1.5, 0.0], "rot": [0, 0, 0], "color": "#524c58" },
            { "shape": "cone",     "size": [2.6, 1.0, 2.6], "pos": [0.0, 4.5, 0.0],  "rot": [0, 0, 0], "color": "#2f416e", "segments": 12 },
            { "shape": "torus",    "size": [2.2, 0.35, 2.2],"pos": [0.0, 4.0, 0.0],  "rot": [0, 0, 0], "color": "#f5b82e", "segments": 20 },
            { "shape": "sphere",   "size": [1.0, 1.0, 1.0], "pos": [0.0, 5.5, 0.0],  "rot": [0, 0, 0], "color": "#55eeff", "glow": true, "segments": 12 }
        ]
    });

    let (st, res) = http_json(port, "POST", "/api/tools/blender/generate", Some(&body), 120);
    assert_eq!(st, 200, "structured mesh generate failed: {res}");
    assert_eq!(res["ok"], json!(true));
    assert_eq!(res["mode"], json!("structured"));
    assert_eq!(res["blender_used"], json!(true));
    assert_eq!(res["name"], json!("shrine_lantern"));

    let tris = res["tris"].as_u64().unwrap();
    assert!(tris > 0 && tris <= MAX_TRIS, "unexpected tri count: {tris}");
    assert!(res["solid_tris"].as_u64().unwrap() > 0);
    assert!(res["glow_tris"].as_u64().unwrap() > 0);

    // Fetch OBJ and assert UVs (vt lines) are present.
    let obj_url = res["obj_url"].as_str().unwrap();
    let (st_obj, _, obj_bytes) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", obj_url, b"", 15);
    assert_eq!(st_obj, 200);
    let obj_text = String::from_utf8_lossy(&obj_bytes);
    assert!(obj_text.lines().any(|l| l.starts_with("v ")), "OBJ missing vertices");
    assert!(obj_text.lines().any(|l| l.starts_with("vt ")), "OBJ missing UV vt lines");
    assert!(obj_text.lines().any(|l| l.starts_with("f ")), "OBJ missing faces");

    // Fetch separate glow OBJ and assert UVs present.
    let glow_url = res["glow_obj_url"].as_str().unwrap();
    assert!(glow_url.ends_with("shrine_lantern_glow.obj"), "glow_url={glow_url}");
    let (st_glow, _, glow_bytes) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", glow_url, b"", 15);
    assert_eq!(st_glow, 200);
    let glow_text = String::from_utf8_lossy(&glow_bytes);
    assert!(glow_text.lines().any(|l| l.starts_with("vt ")), "glow OBJ missing UVs");

    // Fetch baked texture PNG and verify it is a valid non-blank PNG.
    let tex_url = res["texture_url"].as_str().unwrap();
    let (st_tex, head_tex, tex_bytes) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", tex_url, b"", 15);
    assert_eq!(st_tex, 200);
    assert!(head_tex.to_ascii_lowercase().contains("content-type: image/png"));
    assert!(tex_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(tex_bytes.len() > 1024, "baked PNG unexpectedly tiny: {} bytes", tex_bytes.len());

    // Fetch manifest JSON via GET /api/assets/meshes/:file and assert bbox + stats + anchors.
    let manifest_url = res["manifest_url"].as_str().unwrap();
    let (st_man, head_man, man_bytes) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", manifest_url, b"", 15);
    assert_eq!(st_man, 200);
    assert!(head_man.to_ascii_lowercase().contains("content-type: application/json"));
    let manifest: Value = serde_json::from_slice(&man_bytes).expect("parse manifest JSON");

    // Non-blank baked texture stats (non-zero colour mean and variation across shaded faces).
    let mean = manifest["texture_stats"]["solid"]["mean"].as_array().unwrap();
    let mean_sum: f64 = mean.iter().filter_map(|v| v.as_f64()).sum();
    assert!(mean_sum > 0.15, "baked texture appears black/blank: mean={mean:?}");
    let std_dev = manifest["texture_stats"]["solid"]["std"].as_f64().unwrap();
    assert!(std_dev > 0.01, "baked texture appears flat/blank: std={std_dev}");

    // Manifest bbox matches expected [6.0, 6.0, 4.0] studs within tight tolerance.
    let sz = manifest["size"].as_array().unwrap();
    let expected = [6.0_f64, 6.0_f64, 4.0_f64];
    for (i, exp) in expected.iter().enumerate() {
        let got = sz[i].as_f64().unwrap();
        assert!(
            (got - exp).abs() < 0.05,
            "manifest size[{i}] mismatch: expected {exp}, got {got} (full size={sz:?})"
        );
    }
    let bmin = manifest["bbox"]["min"].as_array().unwrap();
    let bmax = manifest["bbox"]["max"].as_array().unwrap();
    assert!((bmin[0].as_f64().unwrap() - (-3.0)).abs() < 0.05);
    assert!((bmin[1].as_f64().unwrap() - 0.0).abs() < 0.05);
    assert!((bmin[2].as_f64().unwrap() - (-2.0)).abs() < 0.05);
    assert!((bmax[0].as_f64().unwrap() - 3.0).abs() < 0.05);
    assert!((bmax[1].as_f64().unwrap() - 6.0).abs() < 0.05);
    assert!((bmax[2].as_f64().unwrap() - 2.0).abs() < 0.05);

    let anchor_names: Vec<&str> = manifest["anchors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["name"].as_str())
        .collect();
    assert!(anchor_names.contains(&"orb"), "missing custom orb anchor: {anchor_names:?}");
    assert!(anchor_names.contains(&"base-centre"));

    // Directory-level manifest.json also has the entry.
    let (st_dm, _, dm_bytes) = http_raw(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        res["dir_manifest_url"].as_str().unwrap(),
        b"",
        15,
    );
    assert_eq!(st_dm, 200);
    let dir_manifest: Value = serde_json::from_slice(&dm_bytes).unwrap();
    assert_eq!(dir_manifest["shrine_lantern"]["tris"], json!(tris));

    // Both 2-angle preview PNGs exist, are valid PNGs, and differ from each other.
    let previews = res["previews"].as_array().unwrap();
    assert_eq!(previews.len(), 2, "expected 2 preview angles: {previews:?}");
    let mut prev_blobs = Vec::new();
    for p in previews {
        let purl = p["url"].as_str().unwrap();
        let (stp, _, pbytes) = http_raw(port, &format!("127.0.0.1:{port}"), "GET", purl, b"", 15);
        assert_eq!(stp, 200, "preview fetch failed for {purl}");
        assert!(pbytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(pbytes.len() > 2048, "preview PNG too small: {}", pbytes.len());
        prev_blobs.push(pbytes);
    }
    assert_ne!(prev_blobs[0], prev_blobs[1], "two preview angles should differ");
}

#[test]
fn script_mode_with_out_dir_works_and_over_20k_tris_is_refused() {
    let det = detect_blender(None);
    if !det.detected {
        eprintln!(
            "SKIPPED script_mode_with_out_dir_works_and_over_20k_tris_is_refused: Blender binary not installed"
        );
        return;
    }

    let td = TestDir::new("script-and-limit");
    let port = start_local_server(td.path().to_path_buf());

    // 1) >20,000-triangle script must hard-fail with 400 plain words and write no mesh files.
    let high_poly_script = r#"
bpy.ops.mesh.primitive_uv_sphere_add(segments=128, ring_count=96, radius=2.0, location=(0, 0, 2))
"#;
    let (st_hi, hi_res) = http_json(
        port,
        "POST",
        "/api/tools/blender/generate",
        Some(&json!({
            "name": "too_dense_orb",
            "script": high_poly_script,
            "preview": false
        })),
        90,
    );
    assert_eq!(st_hi, 400, "expected 400 for >20k tris mesh, got {st_hi}: {hi_res}");
    assert_eq!(hi_res["ok"], json!(false));
    let err = hi_res["error"].as_str().unwrap_or("");
    assert!(
        err.contains("20000") && err.to_ascii_lowercase().contains("triangle"),
        "expected plain-words triangle limit error, got: {err}"
    );
    assert!(
        !td.path()
            .join("game-assets")
            .join("meshes")
            .join("too_dense_orb.obj")
            .exists(),
        "failed >20k run must not leave an OBJ behind"
    );

    // 2) Valid bpy script using optional out_dir relative to <data_dir>/meshes-out.
    let valid_script = r##"
bpy.ops.mesh.primitive_cube_add(size=2.0, location=(0, 0, 1.0))
ob = bpy.context.active_object
ob.name = "pillar"
ob.data.materials.append(material_for("#cc4422"))
"##;
    let (st_ok, ok_res) = http_json(
        port,
        "POST",
        "/api/tools/blender/generate",
        Some(&json!({
            "name": "script_pillar",
            "script": valid_script,
            "out_dir": "hub/props",
            "texture_size": 128,
            "preview": false
        })),
        90,
    );
    assert_eq!(st_ok, 200, "valid script failed: {ok_res}");
    assert_eq!(ok_res["mode"], json!("script"));
    assert_eq!(ok_res["out_dir_rel"], json!("hub/props"));
    assert!(
        td.path()
            .join("meshes-out")
            .join("hub")
            .join("props")
            .join("script_pillar.obj")
            .is_file()
    );
    let obj_url = ok_res["obj_url"].as_str().unwrap();
    assert_eq!(obj_url, "/api/assets/meshes/out/hub/props/script_pillar.obj");
    let (st_fetch, _, obj_bytes) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", obj_url, b"", 15);
    assert_eq!(st_fetch, 200);
    assert!(String::from_utf8_lossy(&obj_bytes).contains("vt "));
}

#[test]
fn denylisted_scripts_and_unsafe_inputs_are_refused() {
    let td = TestDir::new("safety-refusals");
    let port = start_local_server(td.path().to_path_buf());

    let bad_scripts = [
        ("os.system", "import os\nos.system('id')"),
        ("subprocess", "import subprocess\nsubprocess.run(['id'])"),
        ("socket", "import socket\ns = socket.socket()"),
        ("urllib", "import urllib.request\nurllib.request.urlopen('http://127.0.0.1')"),
        ("shutil.rmtree", "import shutil\nshutil.rmtree('/tmp/nope')"),
        ("__import__", "m = __import__('os')"),
    ];

    for (token, src) in bad_scripts {
        assert!(validate_script(src).is_err(), "validate_script should reject {token}");
        let (st, res) = http_json(
            port,
            "POST",
            "/api/tools/blender/generate",
            Some(&json!({
                "name": "evil_mesh",
                "script": src
            })),
            15,
        );
        assert_eq!(st, 400, "expected 400 for denylisted token {token}, got {res}");
        assert_eq!(res["ok"], json!(false));
        let msg = res["error"].as_str().unwrap_or("");
        assert!(
            msg.contains(token),
            "error should name blocked token '{token}', got: {msg}"
        );
    }

    // Bad structured inputs & traversal out_dir -> 400
    for payload in [
        json!({ "name": "empty", "parts": [] }),
        json!({ "name": "bad_shape", "parts": [{ "shape": "teapot" }] }),
        json!({ "name": "bad_hex", "parts": [{ "shape": "box", "color": "not-hex" }] }),
        json!({ "name": "bad_torus", "parts": [{ "shape": "torus", "size": [2, 2, 2] }] }),
        json!({ "name": "escape_dir", "out_dir": "../escape", "parts": [{ "shape": "box" }] }),
        json!({ "name": "abs_dir", "out_dir": "/etc/passwd", "parts": [{ "shape": "box" }] }),
    ] {
        let (st, res) = http_json(
            port,
            "POST",
            "/api/tools/blender/generate",
            Some(&payload),
            15,
        );
        assert_eq!(st, 400, "expected 400 for payload {payload}, got {res}");
        assert_eq!(res["ok"], json!(false));
    }

    // Asset route traversal refusal
    for bad_path in [
        "/api/assets/meshes/../secret.json",
        "/api/assets/meshes/out/../../etc/passwd",
        "/api/assets/meshes/sub/file.obj",
        "/api/assets/meshes/script.py",
    ] {
        let (st, _, _) = http_raw(port, &format!("127.0.0.1:{port}"), "GET", bad_path, b"", 10);
        assert!(st == 400 || st == 403, "expected 400/403 for {bad_path}, got {st}");
    }

    // Non-loopback host refused with 403
    let body = serde_json::to_vec(&json!({ "name": "remote", "parts": [{ "shape": "box" }] })).unwrap();
    let (st_host, _, _) = http_raw(port, "evil.example.com", "POST", "/api/tools/blender/generate", &body, 10);
    assert_eq!(st_host, 403);
}
