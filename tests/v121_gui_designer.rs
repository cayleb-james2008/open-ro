//! Integration tests for OpenRO Cycle 2 Wave E3 (Gap #3: Visual ScreenGui / HUD Designer,
//! Multi-Device Safe-Area Simulator, and Deterministic `--!strict` Luau Compiler).

use openro_rs::agent::{Agent, AgentHooks, AskMode, AskOptions};
use openro_rs::gui_designer::{
    analyze_safe_areas, compile_project_screen, compile_screen_doc, parse_and_validate_screen_str,
    preset_screen_doc, save_project_screen, validate_slug, ALLOWED_CLASSES,
};
use openro_rs::luau_check;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::projects;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
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
        .join("v121-gui-designer");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn send_raw_bytes(port: u16, raw: &[u8]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(raw).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = if sep + 4 <= buf.len() {
        buf[sep + 4..].to_vec()
    } else {
        Vec::new()
    };
    (status, head, body)
}

fn send_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
    };
    let (st, _head, raw_body) = send_raw_bytes(port, req.as_bytes());
    let parsed = serde_json::from_slice(&raw_body).unwrap_or_else(|_| json!({}));
    (st, parsed)
}

fn start_server(data_dir: Option<PathBuf>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let place = LocalPlace::in_memory("GuiTestPlace".to_string());
    let model = ModelClient::new("http://127.0.0.1:1".into(), "stub".into(), None);
    let agent = Agent::with_local_place(model, place);
    let shared = match data_dir {
        Some(dir) => Shared::with_data_dir(Some(agent), "GuiTestPlace".to_string(), dir),
        None => Shared::new(Some(agent), "GuiTestPlace".to_string()),
    };
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn create_rojo_project(root: &Path, name: &str) -> (String, PathBuf) {
    let data_dir = root.join("data");
    let ws = root.join("workspace");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    let mut store = projects::ProjectStore::load(Some(&data_dir));
    store.add_root(ws.to_str().unwrap()).unwrap();
    let r = store
        .create(name, Some(ws.to_str().unwrap()), None, None)
        .unwrap();
    (r.id, r.dir)
}

#[test]
fn schema_allowlists_reject_disallowed_classes_props_and_versions() {
    // All 12 allowed classes validate in a single ScreenDoc v1
    let mut children = Vec::new();
    for (i, cls) in ALLOWED_CLASSES.iter().enumerate() {
        let node = match *cls {
            "UICorner" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "CornerRadius": [0, 8] }
            }),
            "UIStroke" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "Color": "#3B82F6", "Thickness": 1.5 }
            }),
            "UIListLayout" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "FillDirection": "Horizontal", "Padding": [0, 8] }
            }),
            "UIGridLayout" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "CellSize": [[0, 64], [0, 64]], "CellPadding": [[0, 6], [0, 6]] }
            }),
            "UIAspectRatioConstraint" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "AspectRatio": 1.5 }
            }),
            "UIPadding" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": { "PaddingTop": [0, 8], "PaddingLeft": [0, 8] }
            }),
            "TextButton" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": {
                    "Position": [[0, 40], [0, 80]],
                    "Size": [[0, 120], [0, 48]],
                    "Text": "Click",
                    "Callback": "onClick",
                    "ProductKey": "vip_pass"
                }
            }),
            "ImageButton" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": {
                    "Position": [[0, 40], [0, 140]],
                    "Size": [[0, 48], [0, 48]],
                    "AssetKey": "btn_icon",
                    "SliceCenter": [4, 4, 28, 28],
                    "Callback": "onImgClick"
                }
            }),
            "ImageLabel" => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": {
                    "Position": [[0, 40], [0, 200]],
                    "Size": [[0, 48], [0, 48]],
                    "AssetPath": "art/ui/badge.png"
                }
            }),
            _ => json!({
                "name": format!("Node{i}"),
                "class_name": cls,
                "props": {
                    "Position": [[0, 40], [0, 80]],
                    "Size": [[0, 100], [0, 50]]
                }
            }),
        };
        children.push(node);
    }

    let valid_doc = json!({
        "version": 1,
        "name": "AllClassesHud",
        "ignore_gui_inset": true,
        "reset_on_spawn": false,
        "theme": { "accent": "#3B82F6", "surface": "#0F172A", "text": "#F8FAFC" },
        "root": {
            "name": "Root",
            "class_name": "Frame",
            "props": { "Size": [[1, 0], [1, 0]], "BackgroundTransparency": 1 },
            "children": children
        }
    });
    assert!(
        parse_and_validate_screen_str(&valid_doc.to_string()).is_ok(),
        "valid ScreenDoc v1 with all 12 classes should pass"
    );

    // Reject disallowed class (Script)
    let bad_class = json!({
        "version": 1,
        "name": "BadClass",
        "root": {
            "name": "Hack",
            "class_name": "Script",
            "props": {}
        }
    });
    assert!(parse_and_validate_screen_str(&bad_class.to_string()).is_err());

    // Reject disallowed prop on Frame (Text / Source)
    let bad_prop_frame = json!({
        "version": 1,
        "name": "BadProp",
        "root": {
            "name": "Box",
            "class_name": "Frame",
            "props": { "Text": "NotAllowedOnFrame" }
        }
    });
    assert!(parse_and_validate_screen_str(&bad_prop_frame.to_string()).is_err());

    // Reject Callback on non-button (TextLabel)
    let bad_callback = json!({
        "version": 1,
        "name": "BadCb",
        "root": {
            "name": "Lbl",
            "class_name": "TextLabel",
            "props": { "Callback": "hack" }
        }
    });
    assert!(parse_and_validate_screen_str(&bad_callback.to_string()).is_err());

    // Reject unsupported version
    let bad_ver = json!({
        "version": 2,
        "name": "BadVer",
        "root": { "name": "Root", "class_name": "Frame", "props": {} }
    });
    assert!(parse_and_validate_screen_str(&bad_ver.to_string()).is_err());

    // Reject invalid callback identifier (Luau injection attempt)
    let bad_ident = json!({
        "version": 1,
        "name": "BadIdent",
        "root": {
            "name": "Btn",
            "class_name": "TextButton",
            "props": { "Callback": "foo(); evil()" }
        }
    });
    assert!(parse_and_validate_screen_str(&bad_ident.to_string()).is_err());
}

#[test]
fn cwe22_slug_validation_and_symlink_guards() {
    for bad in [
        "",
        ".",
        "..",
        "../evil",
        "..\\evil",
        "..%2Fevil",
        "ui/nested",
        "bad slug",
        "-leading-dash",
        "bad:slug",
        "null\0byte",
    ] {
        assert!(
            validate_slug(bad).is_err(),
            "slug '{bad}' must be rejected (CWE-22)"
        );
    }
    for good in ["main-hud", "rpg_hud_v1", "ShopScreen2"] {
        assert_eq!(validate_slug(good).unwrap(), good);
    }

    // CWE-59 symlink guard inside project/ui
    let dir = temp_dir("symlink-guard");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("ui")).unwrap();
    let outside = dir.join("outside.json");
    std::fs::write(&outside, "{}").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, proj.join("ui/escape.screen.json")).unwrap();
        let preset = preset_screen_doc("obby", None).unwrap();
        let err = save_project_screen(&proj, "escape", &json!({ "doc": preset.to_json() }))
            .expect_err("symlink target must be rejected");
        assert!(err.contains("symlink"), "unexpected err: {err}");
    }
}

#[test]
fn safe_area_analyzer_detects_all_collisions_and_passes_presets() {
    // 1. All 4 starter presets must pass with 0 safe-area warnings on Desktop, Tablet, Mobile Landscape
    for preset in ["rpg", "obby", "tycoon", "gacha"] {
        let doc = preset_screen_doc(preset, None).unwrap();
        let analysis = analyze_safe_areas(&doc);
        assert!(
            analysis.passed && analysis.warnings.is_empty(),
            "preset '{preset}' had unexpected safe-area warnings: {:?}",
            analysis.warnings
        );
        assert!(analysis.devices.contains_key("desktop"));
        assert!(analysis.devices.contains_key("tablet"));
        assert!(analysis.devices.contains_key("mobile_landscape"));
    }

    // 2. TopBar overlap when ignore_gui_inset = true and y < 58, cleared when ignore_gui_inset = false
    let mut topbar_doc = parse_and_validate_screen_str(
        &json!({
            "version": 1,
            "name": "TopOverlap",
            "ignore_gui_inset": true,
            "root": {
                "name": "Root",
                "class_name": "Frame",
                "props": { "Size": [[1, 0], [1, 0]], "BackgroundTransparency": 1 },
                "children": [{
                    "name": "TopBanner",
                    "class_name": "TextLabel",
                    "props": {
                        "Position": [[0, 100], [0, 12]],
                        "Size": [[0, 200], [0, 44]],
                        "Text": "Top"
                    }
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    let a1 = analyze_safe_areas(&topbar_doc);
    assert!(
        a1.warnings.iter().any(|w| w.code == "topbar_overlap"),
        "expected topbar_overlap warning: {:?}",
        a1.warnings
    );
    topbar_doc.ignore_gui_inset = false;
    let a2 = analyze_safe_areas(&topbar_doc);
    assert!(
        !a2.warnings.iter().any(|w| w.code == "topbar_overlap"),
        "ignore_gui_inset=false should not trigger topbar_overlap: {:?}",
        a2.warnings
    );

    // 3. Mobile thumbstick overlap, offscreen bounds, and tiny_tap_target (<44x44)
    let collision_doc = parse_and_validate_screen_str(
        &json!({
            "version": 1,
            "name": "Collisions",
            "ignore_gui_inset": true,
            "root": {
                "name": "Root",
                "class_name": "Frame",
                "props": { "Size": [[1, 0], [1, 0]], "BackgroundTransparency": 1 },
                "children": [
                    {
                        "name": "ThumbStickHit",
                        "class_name": "TextButton",
                        "props": {
                            "Position": [[0, 20], [1, -100]],
                            "Size": [[0, 80], [0, 60]],
                            "Text": "Move"
                        }
                    },
                    {
                        "name": "TinyBtn",
                        "class_name": "ImageButton",
                        "props": {
                            "Position": [[0.5, -16], [0.5, -16]],
                            "Size": [[0, 32], [0, 32]],
                            "Image": "rbxassetid://123"
                        }
                    },
                    {
                        "name": "OffscreenCard",
                        "class_name": "Frame",
                        "props": {
                            "Position": [[1, -40], [0, 80]],
                            "Size": [[0, 120], [0, 60]]
                        }
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();
    let col = analyze_safe_areas(&collision_doc);
    let codes: Vec<&str> = col.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(codes.contains(&"thumbstick_overlap"), "got: {codes:?}");
    assert!(codes.contains(&"tiny_tap_target"), "got: {codes:?}");
    assert!(codes.contains(&"offscreen"), "got: {codes:?}");
}

#[test]
fn deterministic_strict_luau_compiler_wires_assets_products_and_rojo_model() {
    let dir = temp_dir("compile");
    std::fs::write(
        dir.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();

    let doc = preset_screen_doc("gacha", Some("gacha-hud")).unwrap();
    let res1 = compile_screen_doc("gacha-hud", &doc, Some(&dir)).unwrap();
    let res2 = compile_screen_doc("gacha-hud", &doc, Some(&dir)).unwrap();

    let luau1 = res1["luau"].as_str().unwrap();
    let luau2 = res2["luau"].as_str().unwrap();
    assert_eq!(luau1, luau2, "compiled Luau must be deterministic");
    assert_eq!(
        res1["model_json"], res2["model_json"],
        "compiled Rojo .model.json must be deterministic"
    );

    assert!(luau1.starts_with("--!strict\n"), "missing --!strict");
    assert!(luau1.contains("function ScreenModule.create(parent: Instance?, callbacks: CallbackMap?): ScreenGui"));
    assert!(luau1.contains("loadOptionalShared(\"AssetIds\")"));
    assert!(luau1.contains("loadOptionalShared(\"ProductIds\")"));
    assert!(luau1.contains("resolveAssetId(assetIds, \"gem_icon\""));
    assert!(luau1.contains("resolveProductId(productIds, \"roll_ten_pack\")"));
    assert!(luau1.contains("callbacks[\"onRollTen\"]"));
    assert_eq!(res1["luau_check"]["passed"], json!(true));
    assert_eq!(res1["luau_check"]["error_count"], json!(0));
    if res1["stylua"]["available"] == json!(true) {
        assert_eq!(res1["stylua"]["passed"], json!(true), "stylua check failed: {}", res1["stylua"]);
    }

    // Verify Rojo .model.json structure and write-out
    let comp_written = compile_project_screen(
        &dir,
        "gacha-hud",
        &json!({ "doc": doc.to_json(), "emit_model_json": true }),
    )
    .unwrap();
    assert!(dir.join("src/client/ui/GachaHud.luau").is_file());
    assert!(dir.join("ui/gacha-hud.model.json").is_file());
    assert_eq!(comp_written["model_json"]["className"], json!("ScreenGui"));

    let check = luau_check::analyze_source("StarterPlayer.StarterPlayerScripts.Client.ui.GachaHud", luau1);
    assert!(check.is_ok(), "diagnostics: {:?}", check.diagnostics);
}

#[test]
fn http_routes_project_and_studio_local_place_with_cwe22_and_loopback403() {
    let root = temp_dir("http-routes");
    let (proj_id, proj_dir) = create_rojo_project(&root, "GuiHttpGame");
    let data_dir = root.join("data");
    let port = start_server(Some(data_dir));

    // 1. Project GUI routes
    let (st_list, list_res) = send_json(port, "GET", &format!("/api/projects/{proj_id}/gui"), None);
    assert_eq!(st_list, 200);
    assert_eq!(list_res["screens"].as_array().unwrap().len(), 0);
    assert_eq!(list_res["presets"].as_array().unwrap().len(), 4);

    let (st_missing, _) = send_json(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/gui/does-not-exist"),
        None,
    );
    assert_eq!(st_missing, 404);

    // Save preset via POST /api/projects/:id/gui/:slug
    let (st_save, save_res) = send_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/gui/main-hud"),
        Some(&json!({ "preset": "rpg" })),
    );
    assert_eq!(st_save, 200, "save failed: {save_res}");
    assert_eq!(save_res["slug"], json!("main-hud"));
    assert!(proj_dir.join("ui/main-hud.screen.json").is_file());

    // Fetch saved screen
    let (st_get, get_res) = send_json(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/gui/main-hud"),
        None,
    );
    assert_eq!(st_get, 200);
    assert_eq!(get_res["doc"]["name"], json!("MainHud"));
    assert_eq!(get_res["analysis"]["passed"], json!(true));

    // Compile via POST /api/projects/:id/gui/:slug/compile
    let (st_comp, comp_res) = send_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/gui/main-hud/compile"),
        Some(&json!({ "emit_model_json": true })),
    );
    assert_eq!(st_comp, 200, "compile failed: {comp_res}");
    assert!(proj_dir.join("src/client/ui/MainHud.luau").is_file());
    assert!(proj_dir.join("ui/main-hud.model.json").is_file());
    assert_eq!(comp_res["luau_check"]["passed"], json!(true));

    // CWE-22 traversal rejection on project routes
    let (st_trav1, _) = send_json(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/gui/..%2F..%2Fetc"),
        None,
    );
    assert_eq!(st_trav1, 400);
    let (st_trav2, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/gui/..%2Fevil"),
        Some(&json!({ "preset": "obby" })),
    );
    assert_eq!(st_trav2, 400);

    // 2. Studio LocalPlace routes (/api/studio/gui, /api/studio/gui/compile) on fresh in-memory server
    let studio_port = start_server(None);
    let (st_s_save, s_save) = send_json(
        studio_port,
        "POST",
        "/api/studio/gui",
        Some(&json!({ "slug": "studio-hud", "preset": "tycoon" })),
    );
    assert_eq!(st_s_save, 200, "studio save failed: {s_save}");
    assert_eq!(s_save["slug"], json!("studio-hud"));

    let (st_s_list, s_list) = send_json(studio_port, "GET", "/api/studio/gui", None);
    assert_eq!(st_s_list, 200);
    assert_eq!(s_list["screens"].as_array().unwrap().len(), 1);

    let (st_s_get, s_get) = send_json(studio_port, "GET", "/api/studio/gui?slug=studio-hud", None);
    assert_eq!(st_s_get, 200);
    assert_eq!(s_get["slug"], json!("studio-hud"));

    let (st_s_comp, s_comp) = send_json(
        studio_port,
        "POST",
        "/api/studio/gui/compile",
        Some(&json!({ "slug": "studio-hud" })),
    );
    assert_eq!(st_s_comp, 200, "studio compile failed: {s_comp}");
    assert!(s_comp["luau"].as_str().unwrap().contains("--!strict"));
    assert_eq!(s_comp["luau_check"]["passed"], json!(true));

    // CWE-22 on /api/studio/gui
    let (st_s_bad, _) = send_json(
        studio_port,
        "POST",
        "/api/studio/gui",
        Some(&json!({ "slug": "../bad", "preset": "obby" })),
    );
    assert_eq!(st_s_bad, 400);

    // Loopback-403 check on /api/studio/gui
    let bad_req = format!(
        "GET /api/studio/gui HTTP/1.1\r\nHost: evil.example:{studio_port}\r\nConnection: close\r\n\r\n"
    );
    let (st_403, _, _) = send_raw_bytes(studio_port, bad_req.as_bytes());
    assert_eq!(st_403, 403);
}

#[test]
fn agent_tools_gui_save_and_gui_compile_work_end_to_end() {
    let root = temp_dir("agent-tools");
    let (proj_id, proj_dir) = create_rojo_project(&root, "AgentGuiGame");

    let script = json!({
        "steps": [
            { "tool": "gui_save", "args": { "slug": "shop-hud", "preset": "obby" } },
            { "tool": "gui_compile", "args": { "slug": "shop-hud", "emit_model_json": true } },
            { "tool": "project_check", "args": {} }
        ],
        "done": true,
        "summary": "Saved, compiled, and checked ShopHud."
    })
    .to_string();

    let llm_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in llm_listener.incoming().flatten() {
            let mut buf = [0u8; 16384];
            let _ = stream.read(&mut buf);
            let body = json!({
                "choices": [{ "message": { "role": "assistant", "content": script } }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let model = ModelClient::new(format!("http://127.0.0.1:{llm_port}/v1"), "mock".into(), None);
    let mut agent = Agent::with_local_place(model, LocalPlace::in_memory("AgentGuiGame".into()));
    agent.set_project(Some(proj_dir.clone()), Some(proj_id));
    agent.set_hooks(AgentHooks {
        project_playtest: Some(Arc::new(|_args| {
            Ok(json!({ "ok": true, "passed": true, "console": { "errors": [] }, "checks": [] }))
        })),
        ..Default::default()
    });
    let res = agent
        .run_ask_with_options(
            "gui_chat",
            "Build shop HUD",
            AskOptions {
                mode: AskMode::Agent,
                ..Default::default()
            },
        )
        .expect("agent run succeeds");

    assert!(res.summary.contains("ShopHud"));
    assert!(proj_dir.join("ui/shop-hud.screen.json").is_file());
    assert!(proj_dir.join("src/client/ui/ShopHud.luau").is_file());
    assert!(proj_dir.join("ui/shop-hud.model.json").is_file());
}
