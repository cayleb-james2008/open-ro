//! Integration tests for OpenRO v1.36.0 (Gap #92 + Pop-Anime UI + OSS Roblox Tooling):
//! - Craft Rule #10 (`modern-roblox-pop-gradients`), `anime-pop-dark` skin (WCAG >= 5.25:1),
//!   and `cartoon_icons_sheet` (12 bold-ink icons + procedural slicing).
//! - Native Roblox `.mesh` v2.00 + `.glb` support, `assets_codegen` `meshId`/`textureId`/`glowMeshId`/`glowTextureId`,
//!   `POST /api/projects/:id/blender/build-map`, `blender_build_map` agent hook/prompts,
//!   and 5 OSS Roblox/Blender adapters (`blender-mcp-oss`, `luau-lsp-cli`, `darklua-cli`, `asphalt-cli`, `rokit-cli`).

use openro_rs::agent::{project_system_prompt, system_prompt, Agent};
use openro_rs::mesh::build_map_kit;
use openro_rs::model::ModelClient;
use openro_rs::opencloud::assets_codegen;
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::secrets::SecretStore;
use openro_rs::server::{ensure_agent_hooks, serve, Shared};
use openro_rs::teamwork::teamwork_role_prompt_preview;
use openro_rs::tool_adapters::optional_preset_adapters;
use openro_rs::ui_kit::{
    build_ui_sheet_prompt, find_sheet_recipe, find_skin, generate_ui_kit, presets_json,
    ui_craft_rules, visual_skins, wcag_contrast_ratio,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("openro-v136-tests");
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = base.join(format!("{name}-{pid}-{ts}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn boot_server(dir: &Path) -> (u16, String, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join("project");
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "RouteSanctum", "slug": "route-sanctum" }).to_string(),
    )
    .unwrap();
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "RouteSanctum".into()));
    let shared = Shared::with_data_dir(Some(agent), "RouteSanctum".into(), data);
    let proj_id = {
        let mut store = shared.projects.lock().unwrap();
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

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    let (head, body_str) = raw.split_once("\r\n\r\n").unwrap_or((&raw, "{}"));
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    let parsed = serde_json::from_str(body_str).unwrap_or_else(|_| json!({ "raw": body_str }));
    (code, parsed)
}

fn minimal_png() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 10, b'I',
        b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4, 0, 0,
        0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn minimal_roblox_mesh_v2() -> Vec<u8> {
    let mut out = b"version 2.00\n".to_vec();
    out.extend_from_slice(&12u16.to_le_bytes());
    out.push(36);
    out.push(12);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.resize(25 + 3 * 36 + 12, 0);
    out
}

#[test]
fn pop_anime_skin_craft_rule_and_cartoon_icons_sheet() {
    let rules = ui_craft_rules();
    assert_eq!(rules.len(), 10);
    let r10 = rules
        .iter()
        .find(|r| r.id == "modern-roblox-pop-gradients")
        .expect("rule #10");
    assert_eq!(r10.number, 10);
    assert!(r10.roblox_binding.contains("anime-pop-dark"));
    assert!(r10.roblox_binding.contains("blender_build_map"));

    let skins = visual_skins();
    assert_eq!(skins.len(), 7);
    let pop = find_skin("anime-pop-dark").expect("anime-pop-dark");
    for (mode, tok) in [("dark", &pop.dark), ("light", &pop.light)] {
        assert!(
            wcag_contrast_ratio(tok.text, tok.surface) >= 5.25,
            "anime-pop-dark {mode} text/surface contrast"
        );
        assert!(
            wcag_contrast_ratio(tok.accent_fg, tok.accent) >= 5.25,
            "anime-pop-dark {mode} accent contrast"
        );
    }

    let recipe = find_sheet_recipe("cartoon_icons_sheet").expect("cartoon_icons_sheet");
    assert_eq!((recipe.cols, recipe.rows), (4, 3));
    assert_eq!(recipe.items.len(), 12);

    let icon_prompt = build_ui_sheet_prompt("anime-pop-dark", "cartoon_icons_sheet").unwrap();
    assert!(!icon_prompt.contains('\n'));
    assert!(icon_prompt.contains("ZERO medieval bevel shields"));
    assert!(icon_prompt.to_ascii_lowercase().contains("zero text"));

    let dir = scratch("cartoon-icons");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let store = SecretStore::for_data_dir(&dir);
    let gen = generate_ui_kit(
        &proj,
        &store,
        None,
        &json!({
            "skin": "anime-pop-dark",
            "sheet": "cartoon_icons_sheet",
            "procedural_fallback": true
        }),
    )
    .expect("procedural cartoon_icons_sheet");
    assert_eq!(gen["ok"], true);
    assert_eq!(gen["sliced_count"], 12);
    for item in recipe.items {
        assert!(
            proj.join(format!("art/ui/anime-pop-dark/{}.png", item.name)).is_file(),
            "missing sliced icon {}",
            item.name
        );
    }
    let presets = presets_json();
    assert_eq!(presets["craft_rules"].as_array().unwrap().len(), 10);
    assert_eq!(presets["skins"].as_array().unwrap().len(), 7);
    assert_eq!(presets["sheets"].as_array().unwrap().len(), 6);
}

#[test]
fn assets_codegen_emits_roblox_mesh_v2_textures_glow_and_floor() {
    let dir = scratch("mesh-codegen");
    let proj = dir.join("game");
    let meshes_dir = proj.join("assets/meshes");
    std::fs::create_dir_all(&meshes_dir).unwrap();
    std::fs::create_dir_all(proj.join("src/shared/content")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({ "name": "test-sanctum", "tree": { "$className": "DataModel" } }).to_string(),
    )
    .unwrap();

    std::fs::write(meshes_dir.join("dungeon_column.mesh"), minimal_roblox_mesh_v2()).unwrap();
    std::fs::write(meshes_dir.join("dungeon_column.obj"), "v 0 0 0\nvt 0 0\n").unwrap();
    std::fs::write(meshes_dir.join("dungeon_column.png"), minimal_png()).unwrap();
    std::fs::write(meshes_dir.join("dungeon_column_glow.mesh"), minimal_roblox_mesh_v2()).unwrap();
    std::fs::write(meshes_dir.join("dungeon_column_glow.png"), minimal_png()).unwrap();
    std::fs::write(meshes_dir.join("arena_floor_arcane.png"), minimal_png()).unwrap();
    std::fs::write(
        meshes_dir.join("manifest.json"),
        json!({
            "dungeon_column": {
                "size": [6.4, 17.0, 6.4],
                "tris": 420
            }
        })
        .to_string(),
    )
    .unwrap();

    let cg = assets_codegen(
        &proj,
        &json!({
            "out": "src/shared/content/AssetIds.luau",
            "studio_local_fallback": true,
            "studio_project_slug": "test-sanctum"
        }),
    )
    .expect("assets_codegen");
    assert_eq!(cg["ok"], true);
    let luau = std::fs::read_to_string(proj.join("src/shared/content/AssetIds.luau")).unwrap();
    assert!(luau.contains("assets/meshes/dungeon_column.mesh"), "{luau}");
    assert!(
        luau.contains("textureId = \"rbxasset://openro/test-sanctum/assets/meshes/dungeon_column.png\""),
        "{luau}"
    );
    assert!(
        luau.contains("glowMeshId = \"rbxasset://openro/test-sanctum/assets/meshes/dungeon_column_glow.mesh\""),
        "{luau}"
    );
    assert!(
        luau.contains("glowTextureId = \"rbxasset://openro/test-sanctum/assets/meshes/dungeon_column_glow.png\""),
        "{luau}"
    );
    assert!(luau.contains("[\"arena_floor_arcane\"]"), "{luau}");
}

#[test]
fn oss_roblox_adapters_prompts_and_build_map_route() {
    let presets = optional_preset_adapters();
    for expected in [
        "blender-mcp-oss",
        "luau-lsp-cli",
        "darklua-cli",
        "asphalt-cli",
        "rokit-cli",
    ] {
        assert!(
            presets.iter().any(|a| a.id == expected),
            "missing OSS tool adapter preset {expected}"
        );
    }

    assert!(system_prompt().contains("blender_build_map"));
    assert!(project_system_prompt().contains("blender_build_map"));
    assert!(teamwork_role_prompt_preview("architect", "test", Path::new(".")).contains("blender_build_map"));

    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("map-route");
    let (port, pid, proj, shared) = boot_server(&dir);

    let bad = build_map_kit(&dir, Some(&proj), &json!({ "preset": "nonexistent-map" }), None);
    assert!(bad.is_err());

    let mut agent = Agent::new(ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None), None);
    ensure_agent_hooks(&shared, &mut agent);
    assert!(agent.hooks.blender_build_map.is_some());

    let (code, body) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/blender/build-map"),
        Some(&json!({ "preset": "unknown-preset" })),
    );
    assert_eq!(code, 400, "{body}");

    let (code_missing, _) = http_json(
        port,
        "POST",
        "/api/projects/no-such-project/blender/build-map",
        Some(&json!({})),
    );
    assert_eq!(code_missing, 404);
    let _ = ProjectStore::load(None);
}

#[test]
fn build_map_kit_runs_headless_blender_when_installed() {
    if !openro_rs::library::detect_blender(None).detected {
        return;
    }
    let dir = scratch("blender-map-live");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/shared/content")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({ "name": "live-sanctum", "tree": { "$className": "DataModel" } }).to_string(),
    )
    .unwrap();
    std::fs::write(proj.join("src/shared/content/AssetIds.luau"), "return {}\n").unwrap();

    let res = build_map_kit(
        &dir,
        Some(&proj),
        &json!({ "preset": "arcane-dungeon-sanctum", "props": ["soul_brazier"] }),
        None,
    )
    .expect("build_map_kit with live Blender");
    assert_eq!(res["ok"], true);
    assert_eq!(res["prop_count"], 1);
    let mesh_bytes = std::fs::read(proj.join("assets/meshes/soul_brazier.mesh")).unwrap();
    assert!(mesh_bytes.starts_with(b"version 2.00\n"));
    let glow_bytes = std::fs::read(proj.join("assets/meshes/soul_brazier_glow.mesh")).unwrap();
    assert!(glow_bytes.starts_with(b"version 2.00\n"));
    assert!(proj.join("assets/meshes/soul_brazier.png").is_file());
    assert!(proj.join("assets/meshes/arena_floor_arcane.png").is_file());
    let luau = std::fs::read_to_string(proj.join("src/shared/content/AssetIds.luau")).unwrap();
    assert!(luau.contains("soul_brazier.mesh"), "{luau}");
    assert!(luau.contains("arena_floor_arcane.png"), "{luau}");
    let _ = std::fs::remove_dir_all(&dir);
}
