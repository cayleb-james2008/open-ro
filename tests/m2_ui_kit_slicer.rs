//! M2 PR C (Gap #80): UI Asset-Sheet Generator, Auto-Slicer / Alpha-Matting / 9-Slice Pipeline,
//! 7 AAA UI Craft Rules, 4 Mixable Layout Topologies, 5 Visual Skins, and External UI-Gen Adapter Presets.

use openro_rs::agent::Agent;
use openro_rs::gui_designer::{
    analyze_safe_areas, compile_screen_doc, compile_screen_to_luau, ui_kit_screen_doc,
};
use openro_rs::image_ops::slice_ui_sheet;
use openro_rs::luau_check;
use openro_rs::model::ModelClient;
use openro_rs::opencloud::assets_codegen;
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::secrets::SecretStore;
use openro_rs::server::{serve, Shared};
use openro_rs::tool_adapters::{default_installed_adapters, optional_preset_adapters};
use openro_rs::ui_kit::{
    build_ui_sheet_prompt, generate_ui_kit, layout_topologies, sheet_recipes, ui_craft_rules,
    visual_skins, wcag_contrast_ratio,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("m2-ui-kit-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn boot_server(dir: &Path) -> (u16, String, PathBuf, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::create_dir_all(proj.join("art")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "UiKitTest", "slug": "ui-kit-test" }).to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "UiKitTest",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": {
                    "$className": "ReplicatedStorage",
                    "Shared": { "$path": "src/shared" }
                },
                "StarterPlayer": {
                    "$className": "StarterPlayer",
                    "StarterPlayerScripts": { "$path": "src/client" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();

    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "UiKitTest".into()));
    let shared = Shared::with_data_dir(Some(agent), "UiKitTest".into(), data.clone());
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
    (port, proj_id, data, proj, shared)
}

fn http_req(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    );
    s.write_all(req.as_bytes()).unwrap();
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

fn inspect_png_alpha(path: &Path) -> (u32, u32, u8, u8) {
    let py = r#"
import sys, json
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
w, h = im.size
px = im.load()
corner = px[0, 0][3]
center = px[w // 2, h // 2][3]
print(json.dumps({"w": w, "h": h, "corner_alpha": corner, "center_alpha": center}))
"#;
    let out = Command::new("python3")
        .arg("-c")
        .arg(py)
        .arg(path)
        .output()
        .expect("python3 PIL inspect");
    assert!(out.status.success(), "PIL inspect failed: {}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    (
        v["w"].as_u64().unwrap() as u32,
        v["h"].as_u64().unwrap() as u32,
        v["corner_alpha"].as_u64().unwrap() as u8,
        v["center_alpha"].as_u64().unwrap() as u8,
    )
}

fn check_stylua(dir: &Path, src: &str) {
    let Ok(mut child) = Command::new("stylua")
        .arg("--check")
        .arg("-")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(src.as_bytes());
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stylua --check failed:\n{}\n---\n{}",
        String::from_utf8_lossy(&out.stderr),
        src
    );
}

#[test]
fn ui_craft_rules_skins_topologies_and_originality_prompts() {
    let rules = ui_craft_rules();
    assert_eq!(rules.len(), 7, "must encode all 7 AAA UI craft rules");

    let layouts = layout_topologies();
    let layout_ids: Vec<&str> = layouts.iter().map(|l| l.id).collect();
    for expected in [
        "skybridge-crest",
        "perimeter-command",
        "workshop-ribbon",
        "compact-mobile-split",
    ] {
        assert!(layout_ids.contains(&expected), "missing layout {expected}");
    }

    let skins = visual_skins();
    let skin_ids: Vec<&str> = skins.iter().map(|s| s.id).collect();
    for expected in [
        "obsidian-prism",
        "skybridge-gilded",
        "sakura-lacquer",
        "cyber-forge",
        "clean-monochrome",
    ] {
        assert!(skin_ids.contains(&expected), "missing skin {expected}");
    }

    // Verify every skin satisfies >= 5.25:1 WCAG contrast on both Dark and Light tokens.
    for skin in &skins {
        for (mode_name, tok) in [("dark", &skin.dark), ("light", &skin.light)] {
            let text_ratio = wcag_contrast_ratio(&tok.text, &tok.surface);
            assert!(
                text_ratio >= 5.25,
                "skin {} {mode_name} text/surface contrast {text_ratio:.2} < 5.25",
                skin.id
            );
            let muted_ratio = wcag_contrast_ratio(&tok.text_muted, &tok.surface);
            assert!(
                muted_ratio >= 5.25,
                "skin {} {mode_name} text_muted/surface contrast {muted_ratio:.2} < 5.25",
                skin.id
            );
            let btn_ratio = wcag_contrast_ratio(&tok.accent_fg, &tok.accent);
            assert!(
                btn_ratio >= 5.25,
                "skin {} {mode_name} accent_fg/accent contrast {btn_ratio:.2} < 5.25",
                skin.id
            );
            let danger_ratio = wcag_contrast_ratio(&tok.danger_fg, &tok.danger);
            assert!(
                danger_ratio >= 5.25,
                "skin {} {mode_name} danger_fg/danger contrast {danger_ratio:.2} < 5.25",
                skin.id
            );
        }
    }

    let recipes = sheet_recipes();
    let recipe_ids: Vec<&str> = recipes.iter().map(|r| r.id).collect();
    for expected in ["chrome_sheet", "icons_sheet", "rarity_frames_sheet"] {
        assert!(recipe_ids.contains(&expected), "missing sheet recipe {expected}");
    }
    let icons = recipes.iter().find(|r| r.id == "icons_sheet").unwrap();
    assert_eq!(icons.items.len(), 12, "icons_sheet must contain 12 icons");
    let frames = recipes.iter().find(|r| r.id == "rarity_frames_sheet").unwrap();
    assert_eq!(
        frames.items.len(),
        7,
        "rarity_frames_sheet must contain 6 escalating tiers + Legacy (7 frames)"
    );

    // Prompts must be single-line, zero baked-in text, chroma-key bg, zero franchise/ANRX shapes.
    for skin_id in &skin_ids {
        for sheet_id in &recipe_ids {
            let prompt = build_ui_sheet_prompt(skin_id, sheet_id).unwrap();
            assert!(!prompt.contains('\n'), "prompt must be single-line");
            let lower = prompt.to_ascii_lowercase();
            assert!(
                lower.contains("zero text") || lower.contains("no text"),
                "prompt must forbid baked-in text: {prompt}"
            );
            assert!(
                !lower.contains("anrx") && !lower.contains("anime nexus"),
                "prompt must not reference ANRX"
            );
        }
    }
}

#[test]
fn procedural_sheet_slicer_alpha_cutout_slice_center_and_codegen() {
    let dir = scratch("slicer");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("art")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "SlicerProj", "slug": "slicer-proj" }).to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();

    let store = SecretStore::for_data_dir(&dir);

    // 1. Empty AssetIds.slices is stylua-clean `{}`
    let empty_cg = assets_codegen(&proj, &json!({})).unwrap();
    assert_eq!(empty_cg["ok"], true);
    let empty_luau = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(empty_luau.contains("AssetIds.slices = {}"), "{empty_luau}");
    check_stylua(&proj, &empty_luau);

    // 2. Generate chrome_sheet procedurally + auto-slice
    let chrome_res = generate_ui_kit(
        &proj,
        &store,
        None,
        &json!({
            "skin": "obsidian-prism",
            "sheet": "chrome_sheet",
            "procedural_fallback": true,
        }),
    )
    .unwrap();
    assert_eq!(chrome_res["ok"], true);
    let sliced = chrome_res["sliced"].as_array().unwrap();
    assert!(sliced.len() >= 8, "expected >=8 sliced chrome items, got {}", sliced.len());

    let modal_panel_rel = "art/ui/obsidian-prism/modal_panel.png";
    let modal_panel_abs = proj.join(modal_panel_rel);
    assert!(modal_panel_abs.is_file(), "missing {modal_panel_rel}");
    let (w, h, corner_alpha, center_alpha) = inspect_png_alpha(&modal_panel_abs);
    assert!(w >= 64 && h >= 64);
    assert_eq!(corner_alpha, 0, "corner must have transparent alpha");
    assert!(center_alpha > 128, "modal panel body must be visible");

    // Verify SliceCenter math in art/ui/slices.json
    let slices_raw: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("art/ui/slices.json")).unwrap())
            .unwrap();
    let mp_slice = slices_raw["obsidian-prism/modal_panel"]["slice"]
        .as_array()
        .unwrap();
    let l = mp_slice[0].as_i64().unwrap();
    let t = mp_slice[1].as_i64().unwrap();
    let r = mp_slice[2].as_i64().unwrap();
    let b = mp_slice[3].as_i64().unwrap();
    assert!(0 < l && l < r && r < w as i64, "invalid SliceCenter X: {mp_slice:?}");
    assert!(0 < t && t < b && b < h as i64, "invalid SliceCenter Y: {mp_slice:?}");

    // 3. Generate rarity_frames_sheet procedurally + verify transparent center cutout!
    let frames_res = generate_ui_kit(
        &proj,
        &store,
        None,
        &json!({
            "skin": "skybridge-gilded",
            "sheet": "rarity_frames_sheet",
            "procedural_fallback": true,
        }),
    )
    .unwrap();
    assert_eq!(frames_res["ok"], true);
    assert_eq!(frames_res["sliced"].as_array().unwrap().len(), 7);
    let mythic_abs = proj.join("art/ui/skybridge-gilded/frame_mythic.png");
    let (_, _, f_corner_a, f_center_a) = inspect_png_alpha(&mythic_abs);
    assert_eq!(f_corner_a, 0, "rarity frame corner must be transparent");
    assert_eq!(f_center_a, 0, "rarity frame center cutout MUST be transparent");

    // 4. Generate icons_sheet procedurally -> 12 transparent icons
    let icons_res = generate_ui_kit(
        &proj,
        &store,
        None,
        &json!({
            "skin": "cyber-forge",
            "sheet": "icons_sheet",
            "procedural_fallback": true,
        }),
    )
    .unwrap();
    assert_eq!(icons_res["ok"], true);
    assert_eq!(icons_res["sliced"].as_array().unwrap().len(), 12);

    // 5. Verify AssetIds.slices codegen with locked assetId + Rect.new + stylua-clean
    std::fs::write(
        proj.join("assets.lock.json"),
        json!({
            "assets": {
                "art/ui/obsidian-prism/modal_panel.png": {
                    "state": "locked",
                    "assetId": "900112233",
                    "imageAssetId": "900112233"
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let cg = assets_codegen(&proj, &json!({})).unwrap();
    assert_eq!(cg["ok"], true);
    let luau = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(luau.contains("AssetIds.slices = {"), "{luau}");
    assert!(luau.contains("obsidian-prism/modal_panel"), "{luau}");
    assert!(luau.contains("rbxassetid://900112233"), "{luau}");
    assert!(luau.contains("Rect.new("), "{luau}");
    check_stylua(&proj, &luau);

    // 6. Verify ART-LOG.jsonl recorded slice operations
    let log_txt = std::fs::read_to_string(proj.join("art/ART-LOG.jsonl")).unwrap();
    assert!(log_txt.contains("slice_ui_sheet"), "{log_txt}");

    // 7. Direct slice_ui_sheet call on existing master sheet PNG
    let reslice = slice_ui_sheet(
        &proj,
        &json!({
            "input": "art/ui/sheets/obsidian-prism_chrome_sheet.png",
            "skin": "obsidian-prism",
            "sheet": "chrome_sheet",
        }),
    )
    .unwrap();
    assert_eq!(reslice["ok"], true);
}

#[test]
fn mixable_layouts_pass_three_viewports_and_compile_luau_slice_blur_stroke() {
    let dir = scratch("layouts");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::write(
        proj.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();

    for layout in [
        "skybridge-crest",
        "perimeter-command",
        "workshop-ribbon",
        "compact-mobile-split",
    ] {
        for skin in [
            "obsidian-prism",
            "skybridge-gilded",
            "sakura-lacquer",
            "cyber-forge",
            "clean-monochrome",
        ] {
            for mode in ["dark", "light"] {
                let doc = ui_kit_screen_doc(layout, skin, Some(mode), None).unwrap();
                let analysis = analyze_safe_areas(&doc);
                assert_eq!(
                    analysis.warning_count, 0,
                    "layout={layout} skin={skin} mode={mode} had warnings: {:?}",
                    analysis.warnings
                );
            }
        }
    }

    // Verify ScaleType=Slice, SliceCenter, UIStroke, and modal_blur Luau compilation
    let mut modal_doc = ui_kit_screen_doc("skybridge-crest", "obsidian-prism", Some("dark"), None).unwrap();
    modal_doc.modal_blur = true;
    let luau = compile_screen_to_luau("skybridge-modal", &modal_doc).unwrap();
    assert!(luau.contains("Enum.ScaleType.Slice"), "{luau}");
    assert!(
        luau.contains(".SliceCenter = ") && luau.contains("Rect.new("),
        "{luau}"
    );
    assert!(luau.contains("Instance.new(\"UIStroke\")"), "{luau}");
    assert!(luau.contains("BlurEffect"), "{luau}");
    assert!(luau.contains("Lighting"), "{luau}");

    let compiled = compile_screen_doc("skybridge-modal", &modal_doc, Some(&proj)).unwrap();
    assert_eq!(compiled["ok"], true);
    let formatted_luau = compiled["luau"].as_str().unwrap();
    let check = luau_check::analyze_source(
        "StarterPlayer.StarterPlayerScripts.Client.ui.SkybridgeModal",
        formatted_luau,
    );
    assert_eq!(check.errors().len(), 0, "luau errors: {:?}", check.diagnostics);
    assert_eq!(compiled["stylua"]["passed"], true, "stylua failed: {:?}", compiled["stylua"]);
    check_stylua(&proj, formatted_luau);
}

#[test]
fn external_ui_gen_adapters_and_http_routes() {
    let defaults = default_installed_adapters();
    let chatgpt = defaults
        .iter()
        .find(|a| a.id == "chatgpt-browser-cli")
        .expect("chatgpt-browser-cli");
    assert_eq!(chatgpt.priority, 10);
    let chatgpt_desc = chatgpt.description.to_ascii_lowercase();
    assert!(
        chatgpt_desc.contains("ui") && chatgpt_desc.contains("9-slice") && chatgpt_desc.contains("icon"),
        "chatgpt description must mention UI sheets, icons, and 9-slice: {}",
        chatgpt.description
    );

    let openai = defaults
        .iter()
        .find(|a| a.id == "openai-images-http")
        .expect("openai-images-http");
    assert!(
        openai.description.to_ascii_lowercase().contains("transparent"),
        "openai description must mention native transparent PNG support: {}",
        openai.description
    );

    let optionals = optional_preset_adapters();
    let opt_ids: Vec<&str> = optionals.iter().map(|a| a.id.as_str()).collect();
    for expected in ["recraft-v3-ui", "layer-ai-ui", "scenario-game-ui"] {
        let preset = optionals
            .iter()
            .find(|a| a.id == expected)
            .unwrap_or_else(|| panic!("missing adapter preset {expected} in {opt_ids:?}"));
        assert_eq!(preset.capability, "image_gen");
        assert_eq!(preset.transport, "http");
    }

    let dir = scratch("http");
    let (port, proj_id, _data, proj, _shared) = boot_server(&dir);

    let (st_presets, presets) = http_req(port, "GET", "/api/ui-kit/presets", None);
    assert_eq!(st_presets, 200, "presets failed: {presets}");
    assert_eq!(presets["ok"], true);
    assert_eq!(presets["craft_rules"].as_array().unwrap().len(), 7);
    assert_eq!(presets["layouts"].as_array().unwrap().len(), 4);
    assert_eq!(presets["skins"].as_array().unwrap().len(), 5);
    assert_eq!(presets["sheets"].as_array().unwrap().len(), 3);

    let (st_gen, gen_res) = http_req(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/ui-kit/generate"),
        Some(&json!({
            "layout": "perimeter-command",
            "skin": "sakura-lacquer",
            "sheet": "chrome_sheet",
            "procedural_fallback": true,
        })),
    );
    assert_eq!(st_gen, 200, "project ui-kit generate failed: {gen_res}");
    assert_eq!(gen_res["ok"], true);
    assert!(proj.join("art/ui/sakura-lacquer/modal_panel.png").is_file());

    let (st_studio, studio_res) = http_req(
        port,
        "POST",
        "/api/studio/ui-kit/generate",
        Some(&json!({
            "layout": "compact-mobile-split",
            "skin": "clean-monochrome",
            "sheet": "icons_sheet",
            "procedural_fallback": true,
        })),
    );
    assert_eq!(st_studio, 200, "studio ui-kit generate failed: {studio_res}");
    assert_eq!(studio_res["ok"], true);
    assert_eq!(studio_res["sliced"].as_array().unwrap().len(), 12);
}
