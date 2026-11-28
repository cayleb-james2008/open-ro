//! Integration tests for OpenRO v1.35.0 (Gap #91): Image-Gen-First UI Craft workflow —
//! 9 UI Craft Rules, the `panels_sheet` recipe (prompt + procedural fallback + slicing),
//! the upgraded `game_gui` skill (sliced chrome + UIStroke), and live/offline
//! `studio_verify_ui_craft` (`POST /api/projects/:id/studio/verify-ui-craft`).

use openro_rs::agent::{project_system_prompt, system_prompt, Agent};
use openro_rs::model::ModelClient;
use openro_rs::opencloud::assets_codegen;
use openro_rs::place::LocalPlace;
use openro_rs::playtest::{
    offline_ui_craft_audit, ui_craft_gate, PANELS_SHEET_ITEMS, UI_CRAFT_REPORT_REL,
};
use openro_rs::projects::ProjectStore;
use openro_rs::secrets::SecretStore;
use openro_rs::server::{ensure_agent_hooks, serve, Shared};
use openro_rs::teamwork::teamwork_role_prompt_preview;
use openro_rs::ui_kit::{
    build_ui_sheet_prompt, find_sheet_recipe, generate_ui_kit, presets_json, prompt_guidance,
    sheet_recipes, slice_items_for_recipe, ui_craft_rules, visual_skins,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

const RESULT_KEYS: [&str; 10] = [
    "ok",
    "mode",
    "passed",
    "ui_images_bound",
    "slice_chrome_active",
    "opaque_parent_violations",
    "image_backed_buttons",
    "bare_buttons",
    "panel_coverage_pct",
    "report_path",
];

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("v135-ui-craft-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn boot_server(dir: &Path) -> (u16, String, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::create_dir_all(proj.join("art")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "UiCraftTest", "slug": "ui-craft-test" }).to_string(),
    )
    .unwrap();
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "UiCraftTest".into()));
    let shared = Shared::with_data_dir(Some(agent), "UiCraftTest".into(), data.clone());
    let proj_id = {
        let mut store: std::sync::MutexGuard<'_, ProjectStore> = shared.projects.lock().unwrap();
        let entry = store.open(proj.to_str().unwrap()).unwrap();
        let id = entry.id.clone();
        store.set_active(&id);
        id
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, proj_id, proj, shared)
}

fn http_raw(port: u16, host: &str, method: &str, path: &str, body: &[u8]) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, resp_body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, serde_json::from_str(resp_body).unwrap_or(Value::Null))
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    http_raw(port, &format!("127.0.0.1:{port}"), method, path, b.as_bytes())
}

fn png_alpha(path: &Path) -> (u32, u32, u8, u8) {
    let py = r#"
import sys, json
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
w, h = im.size
px = im.load()
print(json.dumps({"w": w, "h": h, "corner": px[0, 0][3], "center": px[w // 2, h // 2][3]}))
"#;
    let out = Command::new("python3").arg("-c").arg(py).arg(path).output().unwrap();
    assert!(out.status.success(), "PIL inspect failed: {}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    (
        v["w"].as_u64().unwrap() as u32,
        v["h"].as_u64().unwrap() as u32,
        v["corner"].as_u64().unwrap() as u8,
        v["center"].as_u64().unwrap() as u8,
    )
}

fn assert_result_schema(v: &Value) {
    for k in RESULT_KEYS {
        assert!(v.get(k).is_some(), "missing result key '{k}': {v}");
    }
    assert!(v.get("details").is_some_and(Value::is_object), "missing details: {v}");
    assert_eq!(v["report_path"], UI_CRAFT_REPORT_REL);
    assert!(v["panel_coverage_pct"].is_number());
}

#[test]
fn nine_craft_rules_include_image_gen_coverage_and_dense_master_detail() {
    let rules = ui_craft_rules();
    assert_eq!(rules.len(), 9);
    let numbers: Vec<u8> = rules.iter().map(|r| r.number).collect();
    assert_eq!(numbers, (1..=9).collect::<Vec<u8>>());
    let mut ids: Vec<&str> = rules.iter().map(|r| r.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 9, "rule ids must be unique");

    let r8 = rules.iter().find(|r| r.id == "image-gen-panel-coverage").expect("rule #8");
    assert_eq!(r8.number, 8);
    assert!(r8.rule.contains(">=70%"));
    assert!(r8.rule.contains("BackgroundTransparency = 1"));
    assert!(r8.rule.contains("panels_sheet"));
    assert!(r8.roblox_binding.contains("ScaleType.Slice"));
    assert!(r8.roblox_binding.contains("studio_verify_ui_craft"));

    let r9 = rules.iter().find(|r| r.id == "dense-master-detail-taste").expect("rule #9");
    assert_eq!(r9.number, 9);
    assert!(r9.rule.contains("Master-Detail"));
    assert!(r9.rule.contains("tile grid"));
    assert!(r9.anti_pattern.contains("single-card voids"));
    assert!(r9.anti_pattern.contains("outline-only stacks"));

    let presets = presets_json();
    assert_eq!(presets["craft_rules"].as_array().unwrap().len(), 9);
    for r in &rules {
        let text = format!("{} {} {} {}", r.title, r.rule, r.roblox_binding, r.anti_pattern);
        let low = text.to_ascii_lowercase();
        assert!(!low.contains("rune") && !text.contains("RHL") && !low.contains("soul cinema"), "{text}");
    }
}

#[test]
fn panels_sheet_recipe_slicing_and_prompts() {
    let recipes = sheet_recipes();
    assert_eq!(recipes.len(), 5);
    let ids: Vec<&str> = recipes.iter().map(|r| r.id).collect();
    for expected in ["chrome_sheet", "icons_sheet", "rarity_frames_sheet", "card_borders_sheet", "panels_sheet"] {
        assert!(ids.contains(&expected), "missing {expected}");
    }

    let panels = find_sheet_recipe("panels_sheet").unwrap();
    assert_eq!((panels.cols, panels.rows), (4, 2));
    assert_eq!(panels.items.len(), 8);
    let expected: [(&str, [u32; 2]); 8] = [
        ("modal_glass", [512, 320]),
        ("title_ribbon", [512, 128]),
        ("tab_active", [256, 96]),
        ("tab_inactive", [256, 96]),
        ("btn_emerald", [320, 112]),
        ("btn_gold", [320, 112]),
        ("dock_tile", [160, 160]),
        ("inspector_card", [440, 320]),
    ];
    let mut cells = std::collections::HashSet::new();
    for (item, (name, size)) in panels.items.iter().zip(expected.iter()) {
        assert_eq!(item.name, *name);
        assert_eq!(item.target_size, *size, "{name} target size");
        assert!(item.is_9slice, "{name} must be 9-slice");
        assert!(!item.cutout_interior, "{name} is a solid plate");
        assert!(item.row < panels.rows && item.col < panels.cols);
        assert!(cells.insert((item.row, item.col)), "duplicate grid cell for {name}");
    }
    assert_eq!(PANELS_SHEET_ITEMS.to_vec(), expected.iter().map(|(n, _)| *n).collect::<Vec<_>>());

    let slice_items = slice_items_for_recipe("panels_sheet").unwrap();
    assert_eq!(slice_items.len(), 8);
    for it in &slice_items {
        assert_eq!(it["rows"], 2);
        assert_eq!(it["cols"], 4);
        assert_eq!(it["is_9slice"], true);
        assert!(it.get("pad_px").is_none(), "pad_px is card_borders only: {it}");
    }
    assert!(slice_items_for_recipe("PANELS_SHEET").is_ok(), "recipe ids are case-insensitive");
    assert!(slice_items_for_recipe("nope_sheet").is_err());

    let presets = presets_json();
    let sheet = presets["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "panels_sheet")
        .expect("panels_sheet preset");
    assert_eq!(sheet["item_count"], 8);

    for skin in visual_skins() {
        let prompt = build_ui_sheet_prompt(skin.id, "panels_sheet").unwrap();
        assert!(!prompt.contains('\n'), "single-line prompt");
        assert!(prompt.contains("4x2"), "{prompt}");
        assert!(prompt.contains(skin.chroma_key), "skin chroma key {}: {prompt}", skin.chroma_key);
        assert!(prompt.contains("jet-black obsidian translucent modal panel"));
        assert!(prompt.contains("diagonal angular slash-crest header banner"));
        assert!(prompt.contains("glossy emerald-green beveled primary"));
        assert!(prompt.contains("hotkey dock tile"));
        let low = prompt.to_ascii_lowercase();
        assert!(low.contains("zero text"));
        assert!(low.contains("zero trademarked emblems"));
        for banned in ["anrx", "anime nexus", "rune", "soul cinema"] {
            assert!(!low.contains(banned), "prompt leaked '{banned}': {prompt}");
        }
        assert!(!prompt.contains("RHL"));
    }
    assert!(build_ui_sheet_prompt("obsidian-prism", "missing_sheet").is_err());

    let guidance = prompt_guidance();
    for needle in [
        "9 Rules",
        "chrome_sheet",
        "panels_sheet",
        "icons_sheet",
        "card_borders_sheet",
        "chatgpt-browser-cli",
        "\"procedural_fallback\":false",
        "BackgroundTransparency = 1",
        "Master-Detail",
        "studio_verify_ui_craft",
        // Back-compat needles from v1.32 guidance.
        "UDim2.fromScale(1, 1)",
        "ScaleType.Crop",
        "card-borders/",
        "hotbar",
    ] {
        assert!(guidance.contains(needle), "guidance missing {needle}");
    }
    assert!(!guidance.to_ascii_lowercase().contains("rune"));
}

#[test]
fn panels_sheet_procedural_fallback_slices_transparent_9slice_pngs() {
    let dir = scratch("panels-procedural");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("art")).unwrap();
    let store = SecretStore::for_data_dir(&dir);

    for skin in ["obsidian-prism", "cyber-forge"] {
        let res = generate_ui_kit(
            &proj,
            &store,
            None,
            &json!({ "skin": skin, "sheet": "panels_sheet", "procedural_fallback": true }),
        )
        .unwrap();
        assert_eq!(res["ok"], true, "{res}");
        assert_eq!(res["sheet"], "panels_sheet");
        assert_eq!(res["generation"]["source"], "procedural_fallback", "Pillow path expected: {res}");
        let sliced = res["sliced"].as_array().unwrap();
        assert_eq!(sliced.len(), 8, "{res}");
        assert_eq!(res["nine_slice_count"], 8);
        for name in PANELS_SHEET_ITEMS {
            let abs = proj.join(format!("art/ui/{skin}/{name}.png"));
            assert!(abs.is_file(), "missing {}", abs.display());
            let (w, h, corner, center) = png_alpha(&abs);
            assert!(w >= 64 && h >= 64, "{name} {w}x{h}");
            assert_eq!(corner, 0, "{skin}/{name} corner must be transparent");
            assert!(center > 128, "{skin}/{name} plate body must be opaque (center alpha {center})");
        }
        // The rendered master sheet keeps the skin chroma key for the auto-slicer.
        assert!(proj.join(format!("art/ui/sheets/{skin}_panels_sheet.png")).is_file());
    }

    let slices: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("art/ui/slices.json")).unwrap()).unwrap();
    for name in PANELS_SHEET_ITEMS {
        let entry = &slices[format!("obsidian-prism/{name}")];
        let rect = entry["slice"].as_array().unwrap_or_else(|| panic!("no slice for {name}: {slices}"));
        let size = entry["size"].as_array().unwrap();
        let (w, h) = (size[0].as_i64().unwrap(), size[1].as_i64().unwrap());
        let r: Vec<i64> = rect.iter().map(|v| v.as_i64().unwrap()).collect();
        assert!(0 < r[0] && r[0] < r[2] && r[2] < w, "{name} SliceCenter X {r:?} in {w}");
        assert!(0 < r[1] && r[1] < r[3] && r[3] < h, "{name} SliceCenter Y {r:?} in {h}");
    }

    // AssetIds codegen exposes the panels for the upgraded game_gui skill.
    let cg = assets_codegen(&proj, &json!({})).unwrap();
    assert_eq!(cg["ok"], true);
    let luau = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    for name in ["modal_glass", "btn_emerald", "title_ribbon"] {
        assert!(luau.contains(&format!("obsidian-prism/{name}")), "AssetIds missing {name}: {luau}");
    }

    let audit = offline_ui_craft_audit(&proj);
    assert_eq!(audit["details"]["panels_sheet_complete"], true, "{audit}");
    assert_eq!(audit["details"]["slices_missing_png"], 0, "{audit}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn game_gui_skill_binds_sliced_chrome_and_stays_compatible() {
    let gui = openro_rs::skills::generate(
        "game_gui",
        &json!({ "title": "Coin Tycoon", "button_label": "Buy", "currency": "Coins" }),
    )
    .unwrap();
    // v0.5 compatibility surface.
    assert!(gui.contains("ScreenGui") && gui.contains("Coin Tycoon"));
    assert!(gui.contains("gui.Name = \"GameGui\""));
    assert!(gui.contains("title.Name = \"Title\""));
    assert!(gui.contains("balance.Name = \"Balance\""));
    assert!(gui.contains("button.Name = \"ShopButton\""));
    assert!(gui.contains("WaitForChild(\"OpenROShopBuy\"):FireServer()"));
    assert!(gui.contains("stats:WaitForChild(\"Coins\")"));
    assert!(gui.contains("player:WaitForChild(\"PlayerGui\")"));
    // Gap #91 sliced chrome binding.
    for needle in [
        "{ \"Shared\", \"Content\", \"AssetIds\" }",
        "{ \"Shared\", \"AssetIds\" }",
        "findPath(ReplicatedStorage, names)",
        "node:FindFirstChild(name)",
        "mod:IsA(\"ModuleScript\")",
        "pcall(require, mod)",
        "local SKIN = \"obsidian-prism\"",
        "\"title_ribbon\", \"TitleRibbon\"",
        "\"modal_glass\", \"PanelChrome\"",
        "\"btn_emerald\", \"SliceChrome\"",
        "Instance.new(\"ImageLabel\")",
        "chrome.ScaleType = Enum.ScaleType.Slice",
        "chrome.SliceCenter = entry.slice",
        "host.BackgroundTransparency = 1",
        "Instance.new(\"UIStroke\")",
        "Enum.ZIndexBehavior.Global",
    ] {
        assert!(gui.contains(needle), "game_gui missing `{needle}`:\n{gui}");
    }
    assert!(!gui.contains("Rune"), "#155: no user-facing product name leak");
    openro_rs::luau_check::check_luau_syntax(&gui).expect("generated GameGui must parse");

    // Escaping discipline still holds.
    let evil = openro_rs::skills::generate("game_gui", &json!({ "title": "a\"b\\c" })).unwrap();
    assert!(evil.contains("title.Text = \"a\\\"b\\\\c\""), "{evil}");
    assert!(openro_rs::skills::generate("game_gui", &json!({ "title": "bad\u{0007}title" })).is_err());
}

#[test]
fn ui_craft_gate_thresholds() {
    assert!(ui_craft_gate(6, 4, 0, 65.0));
    assert!(ui_craft_gate(20, 12, 0, 100.0));
    assert!(!ui_craft_gate(5, 4, 0, 90.0), "needs >= 6 images");
    assert!(!ui_craft_gate(6, 3, 0, 90.0), "needs >= 4 slice chrome");
    assert!(!ui_craft_gate(6, 4, 1, 90.0), "zero opaque-parent violations");
    assert!(!ui_craft_gate(6, 4, 0, 64.9), "needs >= 65% coverage");
}

#[test]
fn offline_audit_counts_bindings_and_flags_opaque_chrome_parents() {
    let dir = scratch("offline-audit");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::create_dir_all(proj.join("art/ui/obsidian-prism")).unwrap();
    std::fs::write(proj.join("art/ui/obsidian-prism/modal_glass.png"), b"png").unwrap();
    std::fs::write(
        proj.join("art/ui/slices.json"),
        json!({
            "obsidian-prism/modal_glass": { "slice": [20, 20, 492, 300], "size": [512, 320], "path": "art/ui/obsidian-prism/modal_glass.png" },
            "obsidian-prism/btn_emerald": { "slice": [30, 30, 290, 82], "size": [320, 112], "path": "art/ui/obsidian-prism/btn_emerald.png" },
            "obsidian-prism/bad_rect": { "slice": [50, 50, 10, 10], "size": [64, 64], "path": "art/ui/obsidian-prism/bad_rect.png" }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join("src/client/Hud.client.luau"),
        r#"local panel = Instance.new("Frame")
panel.BackgroundTransparency = 0.2
local chrome = Instance.new("ImageLabel")
chrome.Name = "PanelChrome"
chrome.Image = "rbxassetid://1"
chrome.ScaleType = Enum.ScaleType.Slice
chrome.Parent = panel
local okPanel = Instance.new("Frame")
okPanel.BackgroundTransparency = 1
local ribbon = Instance.new("ImageLabel")
ribbon.Name = "TitleRibbon"
ribbon.Image = "rbxassetid://2"
ribbon.ScaleType = Enum.ScaleType.Slice
ribbon.Parent = okPanel
local buy = Instance.new("ImageButton")
buy.Image = "rbxassetid://3"
buy.ScaleType = Enum.ScaleType.Slice
local bare = Instance.new("TextButton")
-- chrome.Image = "commented out"
"#,
    )
    .unwrap();

    let audit = offline_ui_craft_audit(&proj);
    assert_eq!(audit["ui_images_bound"], 3, "{audit}");
    assert_eq!(audit["slice_chrome_active"], 3, "{audit}");
    assert_eq!(audit["opaque_parent_violations"], 1, "{audit}");
    assert_eq!(audit["image_backed_buttons"], 1, "{audit}");
    assert_eq!(audit["static_gate_passed"], false);
    let v = &audit["details"]["violations"][0];
    assert_eq!(v["chrome_var"], "chrome");
    assert_eq!(v["parent_var"], "panel");
    assert_eq!(audit["details"]["slices_json_entries"], 3);
    assert_eq!(audit["details"]["valid_slice_rects"], 2);
    assert_eq!(audit["details"]["slices_missing_png"], 2);
    assert_eq!(audit["details"]["luau_files_scanned"], 1);
    let pct = audit["panel_coverage_pct"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&pct));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_ui_craft_http_route_offline_schema_and_errors() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("http-offline");
    let (port, pid, proj, _shared) = boot_server(&dir);
    let route = format!("/api/projects/{pid}/studio/verify-ui-craft");

    // 1. Studio disconnected -> offline_fallback static audit, never a live pass.
    let (st, res) = http_json(port, "POST", &route, Some(&json!({})));
    assert_eq!(st, 200, "{res}");
    assert_result_schema(&res);
    assert_eq!(res["ok"], true);
    assert_eq!(res["mode"], "offline_fallback");
    assert_eq!(res["live_studio_connected"], false);
    assert_eq!(res["passed"], false);
    assert!(res["error"].as_str().unwrap().contains("not connected"));
    let receipt = proj.join(UI_CRAFT_REPORT_REL);
    assert!(receipt.is_file(), "missing receipt");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_eq!(doc["mode"], "offline_fallback");

    // 2. Rich project + explicit offline flag -> counters reflect sources; static gate may pass but passed stays false.
    let mut src = String::new();
    for i in 0..6 {
        src.push_str(&format!(
            "local host{i} = Instance.new(\"Frame\")\nhost{i}.BackgroundTransparency = 1\nlocal chrome{i} = Instance.new(\"ImageLabel\")\nchrome{i}.Name = \"SliceChrome\"\nchrome{i}.Image = \"rbxassetid://{i}\"\nchrome{i}.ScaleType = Enum.ScaleType.Slice\nchrome{i}.Parent = host{i}\n"
        ));
    }
    std::fs::write(proj.join("src/client/Panels.client.luau"), src).unwrap();
    let (st2, res2) = http_json(port, "POST", &route, Some(&json!({ "offline": true })));
    assert_eq!(st2, 200, "{res2}");
    assert_result_schema(&res2);
    assert_eq!(res2["mode"], "offline_fallback");
    assert_eq!(res2["passed"], false);
    assert_eq!(res2["ui_images_bound"], 6);
    assert_eq!(res2["slice_chrome_active"], 6);
    assert_eq!(res2["opaque_parent_violations"], 0);
    assert_eq!(res2["panel_coverage_pct"], 100.0);
    assert_eq!(res2["static_gate_passed"], true);
    assert!(res2["error"].as_str().unwrap().contains("offline"));

    // 3. Schema / error cases.
    let (st_bad_type, _) = http_json(port, "POST", &route, Some(&json!({ "offline": "yes" })));
    assert_eq!(st_bad_type, 400);
    let (st_bad_settle, _) = http_json(port, "POST", &route, Some(&json!({ "settle_ms": -5 })));
    assert_eq!(st_bad_settle, 400);
    let (st_bad_json, _) = http_raw(port, &format!("127.0.0.1:{port}"), "POST", &route, b"{not json");
    assert_eq!(st_bad_json, 400);
    let (st_array, _) = http_raw(port, &format!("127.0.0.1:{port}"), "POST", &route, b"[1,2]");
    assert_eq!(st_array, 400);
    let (st_get, _) = http_json(port, "GET", &route, None);
    assert_eq!(st_get, 405);
    let (st_missing, _) = http_json(port, "POST", "/api/projects/no-such-proj/studio/verify-ui-craft", Some(&json!({})));
    assert_eq!(st_missing, 404);
    let (st_foreign, _) = http_raw(port, "evil.example", "POST", &route, b"{}");
    assert_eq!(st_foreign, 403, "non-loopback Host must be refused");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_hook_and_prompts_expose_studio_verify_ui_craft() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("hooks");
    let (_port, _pid, proj, shared) = boot_server(&dir);

    let sp = system_prompt();
    let psp = project_system_prompt();
    let coord = teamwork_role_prompt_preview("coordinator", "ship", &proj);
    let ver = teamwork_role_prompt_preview("verifier", "ship", &proj);
    let work = teamwork_role_prompt_preview("worker", "ship", &proj);
    for (label, p) in [("system", &sp), ("project", &psp), ("coordinator", &coord), ("verifier", &ver), ("worker", &work)] {
        assert!(p.contains("studio_verify_ui_craft"), "{label} prompt missing studio_verify_ui_craft");
    }
    for (label, p) in [("system", &sp), ("project", &psp), ("worker", &work)] {
        assert!(p.contains("panels_sheet"), "{label} prompt missing panels_sheet");
        assert!(p.contains("9 Rules"), "{label} prompt missing 9 UI Craft Rules");
    }
    assert!(sp.contains("offline_fallback"));
    assert!(!work.contains("UI Craft (7 Rules"), "stale 7-rule guidance");
    assert!(!sp.contains("UI Craft (7 Rules"), "stale 7-rule guidance");

    let mut agent = Agent::new(ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None), None);
    ensure_agent_hooks(&shared, &mut agent);
    let hook = agent.hooks.studio_verify_ui_craft.as_ref().expect("studio_verify_ui_craft hook");
    let res = hook(&json!({})).expect("hook ok");
    assert_result_schema(&res);
    assert_eq!(res["mode"], "offline_fallback");
    assert_eq!(res["passed"], false);
    assert!(hook(&json!({ "offline": 3 })).is_err(), "bad offline type is rejected");
    assert!(proj.join(UI_CRAFT_REPORT_REL).is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn write_mock_ui_craft_mcp(dir: &Path, state_file: &Path, log_file: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script_path = dir.join("mock_ui_craft_mcp.py");
    let script = format!(
        r#"#!/usr/bin/env python3
import json, sys
STATE_FILE = {state:?}
LOG_FILE = {log:?}

def cfg():
    try:
        with open(STATE_FILE) as f:
            return json.load(f)
    except Exception:
        return {{}}

def text(t):
    return {{"content": [{{"type": "text", "text": t}}], "isError": False}}

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    rid = req.get("id")
    method = req.get("method", "")
    if method == "initialize":
        res = {{"protocolVersion": "2024-11-05", "capabilities": {{"tools": {{}}}}, "serverInfo": {{"name": "mock-ui-craft", "version": "1.0.0"}}}}
    elif method == "tools/list":
        res = {{"tools": [{{"name": n, "description": n}} for n in ["list_roblox_studios", "get_studio_state", "start_stop_play", "execute_luau", "get_console_output"]]}}
    elif method == "tools/call":
        name = req.get("params", {{}}).get("name", "")
        args = req.get("params", {{}}).get("arguments", {{}})
        with open(LOG_FILE, "a") as f:
            f.write(json.dumps({{"tool": name, "args": args}}) + "\n")
        c = cfg()
        if name == "list_roblox_studios":
            res = text(json.dumps({{"studios": [{{"studio_id": "s-ui-1", "place_name": "UiPlace", "datamodel_type": "Edit"}}]}}))
        elif name == "get_studio_state":
            res = text(json.dumps({{"active_datamodel": "Edit", "mode": "edit", "place_open": c.get("place_open", True)}}))
        elif name == "start_stop_play":
            res = text(json.dumps({{"ok": True, "mode": args.get("mode")}}))
        elif name == "get_console_output":
            res = text("[]")
        elif name == "execute_luau":
            code = args.get("code", "")
            if "-- openro:v135:verify_ui_craft" in code:
                payload = c.get("ui_result", {{
                    "ok": True,
                    "screen_guis": ["GameGui", "ShopModal"],
                    "ui_images_bound": 9,
                    "slice_chrome_active": 7,
                    "opaque_parent_violations": 0,
                    "violations": [],
                    "image_backed_buttons": 5,
                    "bare_buttons": 1,
                    "bare_button_paths": ["PlayerGui.ShopModal.Close"],
                    "panel_candidates": 10,
                    "panel_covered": 8
                }})
                res = text("OPENRO_V011:" + json.dumps(payload))
            else:
                res = text("OPENRO_V011:" + json.dumps({{"ok": True}}))
        else:
            res = {{"content": [{{"type": "text", "text": "unknown tool"}}], "isError": True}}
    else:
        if rid is None:
            continue
        res = None
    if rid is not None:
        sys.stdout.write(json.dumps({{"jsonrpc": "2.0", "id": rid, "result": res}}) + "\n")
        sys.stdout.flush()
"#,
        state = state_file.to_string_lossy(),
        log = log_file.to_string_lossy(),
    );
    std::fs::write(&script_path, script).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();
    script_path
}

#[cfg(unix)]
#[test]
fn verify_ui_craft_live_studio_probe_pass_fail_and_offline_override() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("http-live");
    let (port, pid, proj, _shared) = boot_server(&dir);
    let route = format!("/api/projects/{pid}/studio/verify-ui-craft");
    let state_file = dir.join("mcp_state.json");
    let log_file = dir.join("mcp_calls.jsonl");
    std::fs::write(&state_file, "{}").unwrap();
    let script = write_mock_ui_craft_mcp(&dir, &state_file, &log_file);
    // Never stage into the machine's real Studio content dir during tests.
    std::env::set_var("OPENRO_STUDIO_CONTENT_DIR", dir.join("studio-content"));

    let (st_conn, conn) = http_json(
        port,
        "POST",
        "/api/studio/embed/launch",
        Some(&json!({ "command": script.to_str().unwrap() })),
    );
    assert_eq!(st_conn, 200, "mock MCP launch failed: {conn}");
    assert_eq!(conn["connected"], true);

    // 1. Live pass: 9 images, 7 slice chrome, 0 violations, 8/10 = 80% coverage.
    let (st, res) = http_json(port, "POST", &route, Some(&json!({ "settle_ms": 0 })));
    assert_eq!(st, 200, "{res}");
    assert_result_schema(&res);
    assert_eq!(res["mode"], "live_studio");
    assert_eq!(res["live_studio_connected"], true);
    assert_eq!(res["ui_images_bound"], 9);
    assert_eq!(res["slice_chrome_active"], 7);
    assert_eq!(res["opaque_parent_violations"], 0);
    assert_eq!(res["image_backed_buttons"], 5);
    assert_eq!(res["bare_buttons"], 1);
    assert_eq!(res["panel_coverage_pct"], 80.0);
    assert_eq!(res["passed"], true, "{res}");
    assert_eq!(res["details"]["screen_guis"], json!(["GameGui", "ShopModal"]));
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join(UI_CRAFT_REPORT_REL)).unwrap()).unwrap();
    assert_eq!(doc["mode"], "live_studio");
    assert_eq!(doc["passed"], true);

    // The probe runs on the Client over PlayerGui and skips Roblox core GUIs.
    let calls = std::fs::read_to_string(&log_file).unwrap();
    assert!(calls.contains("-- openro:v135:verify_ui_craft"), "{calls}");
    for needle in ["BubbleChat", "Freecam", "PlayerGui", "BackgroundTransparency < 0.85", "SliceChrome", "RarityBorder"] {
        assert!(calls.contains(needle), "probe missing {needle}");
    }
    assert!(calls.contains("\"start_play\"") && calls.contains("\"stop\""), "{calls}");

    // 2. Live fail: an opaque parent behind PanelChrome + low coverage.
    std::fs::write(
        &state_file,
        json!({
            "ui_result": {
                "ok": true,
                "ui_images_bound": 8,
                "slice_chrome_active": 5,
                "opaque_parent_violations": 1,
                "violations": [{ "path": "PlayerGui.Hud.Panel.PanelChrome", "parent": "PlayerGui.Hud.Panel", "parent_background_transparency": 0.2 }],
                "image_backed_buttons": 2,
                "bare_buttons": 4,
                "panel_candidates": 10,
                "panel_covered": 4
            }
        })
        .to_string(),
    )
    .unwrap();
    let (st_f, res_f) = http_json(port, "POST", &route, Some(&json!({ "settle_ms": 0 })));
    assert_eq!(st_f, 200, "{res_f}");
    assert_eq!(res_f["mode"], "live_studio");
    assert_eq!(res_f["passed"], false);
    assert_eq!(res_f["opaque_parent_violations"], 1);
    assert_eq!(res_f["panel_coverage_pct"], 40.0);
    assert_eq!(res_f["details"]["violations"][0]["parent_background_transparency"], 0.2);

    // 3. Probe error -> live mode, not passed.
    std::fs::write(&state_file, json!({ "ui_result": { "ok": false, "error": "LocalPlayer has no PlayerGui" } }).to_string()).unwrap();
    let (_, res_e) = http_json(port, "POST", &route, Some(&json!({ "settle_ms": 0 })));
    assert_eq!(res_e["mode"], "live_studio");
    assert_eq!(res_e["passed"], false);
    assert_eq!(res_e["error"], "LocalPlayer has no PlayerGui");

    // 4. Explicit offline flag skips the live probe even while connected.
    let before = std::fs::read_to_string(&log_file).unwrap().matches("verify_ui_craft").count();
    let (_, res_o) = http_json(port, "POST", &route, Some(&json!({ "offline": true })));
    assert_eq!(res_o["mode"], "offline_fallback");
    assert_eq!(res_o["passed"], false);
    let after = std::fs::read_to_string(&log_file).unwrap().matches("verify_ui_craft").count();
    assert_eq!(before, after, "offline mode must not call execute_luau");

    // 5. Connected MCP but no open place -> offline fallback.
    std::fs::write(&state_file, json!({ "place_open": false }).to_string()).unwrap();
    let (_, res_np) = http_json(port, "POST", &route, Some(&json!({ "settle_ms": 0 })));
    assert_eq!(res_np["mode"], "offline_fallback", "{res_np}");
    let _ = std::fs::remove_dir_all(&dir);
}
