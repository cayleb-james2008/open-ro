//! v1.27.0 Cycle 4 Wave E1 integration tests:
//! - Companion plugin cursor-paginated `get_place_snapshot` (< 64 KiB chunks, 12-component CFrame,
//!   MeshPart.MeshId, SurfaceAppearance, lights, ParticleEmitter, ScreenGui trees, Attributes)
//! - Bidirectional 3-way Studio pull (`POST /api/projects/:id/sync/pull-diff` & `/sync/pull`)
//!   with baseline persistence at `.openro/sync-baseline.json`, strategies (`merge_non_conflicting`,
//!   `take_studio`, `keep_local`), `confirm_overwrite` 409 gate, CWE-22 path checks, and loopback 403
//! - Prefab model import (`POST /api/projects/:id/import-model`) for `.rbxmx`, `.rbxlx`, `.rbxm`
//!   (script extraction, `scene/models/<slug>.model.json`, `default.project.json` wiring,
//!   XXE/DOCTYPE rejection, CWE-22 slug sanitization, 16 MiB cap, corrupt `.rbxm` 400 rejection)
//! - `project_sync_pull` tool dispatch in `Agent`

use openro_rs::agent::Agent;
use openro_rs::model::ModelClient;
use openro_rs::place::PlaceBackend;
use openro_rs::projects;
use openro_rs::server::{serve, Shared};
use openro_rs::studio_plugin::{render_companion_lua, StudioPluginBridge};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_dir(tag: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/cayleb/Work/scratch/swarm-evolve/target-open-ro"));
    let dir = base.join(format!("v127-{tag}-{}-{ts}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn start_plugin_server(data_dir: PathBuf) -> (u16, Arc<StudioPluginBridge>, Arc<Shared>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bridge = StudioPluginBridge::open(Some(&data_dir));
    bridge.record_handshake(
        "Roblox Studio (Edit)",
        "E1Arena",
        vec!["Workspace.SpawnPad".to_string()],
    );
    let model = ModelClient::new("http://127.0.0.1:1".into(), "stub".into(), None);
    let agent = Agent::with_backend(model, Some(PlaceBackend::Plugin(Arc::clone(&bridge))));
    let shared = Shared::with_data_dir(
        Some(agent),
        "E1Arena".to_string(),
        data_dir,
    );
    let shared_srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_srv);
    });
    (port, bridge, shared)
}

fn send_http(port: u16, method: &str, path: &str, body: Option<&Value>, host: &str) -> (u16, Value) {
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
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(req.as_bytes()).unwrap();
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
    let raw_body = if sep + 4 <= buf.len() {
        &buf[sep + 4..]
    } else {
        &[]
    };
    let parsed = serde_json::from_slice(raw_body).unwrap_or_else(|_| json!({}));
    (status, parsed)
}

#[test]
fn companion_plugin_snapshot_pagination_under_64kib_and_rich_properties() {
    let lua = render_companion_lua(18420, "0123456789abcdef0123456789abcdef");
    assert!(lua.contains("get_place_snapshot"));
    assert!(lua.contains("GetComponents"));
    assert!(lua.contains("MeshId"));
    assert!(lua.contains("SurfaceAppearance"));
    assert!(lua.contains("ParticleEmitter"));
    assert!(lua.contains("ScreenGui"));
    assert!(lua.contains("GetAttributes"));

    let bridge = StudioPluginBridge::open(None);
    bridge.record_handshake("Roblox Studio (Edit)", "SnapshotPlace", vec![]);

    let worker = Arc::clone(&bridge);
    let handle = std::thread::spawn(move || {
        // Page 1: script part 1 + MeshPart + SurfaceAppearance + Lighting
        let cmd1 = worker.poll_next_command(Duration::from_secs(2)).unwrap();
        assert_eq!(cmd1.tool, "get_place_snapshot");
        let page1 = json!({
            "snapshot_id": "snap-e1-1",
            "cursor": "0",
            "next_cursor": "2",
            "done": false,
            "scripts": {},
            "script_parts": {
                "ServerScriptService.Server.Huge": { "index": 1, "total": 2, "data": "-- part 1\n" }
            },
            "script_meta": {
                "ServerScriptService.Server.Huge": { "class_name": "Script" }
            },
            "instances": {
                "Workspace.Tower": {
                    "class_name": "MeshPart",
                    "properties": {
                        "CFrame": {
                            "position": [10.0, 4.0, -6.0],
                            "orientation": [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]
                        },
                        "Size": [4.0, 12.0, 4.0],
                        "MeshId": "rbxassetid://99887766",
                        "Attributes": { "Zone": "North", "Level": 3 }
                    }
                },
                "Workspace.Tower.PBR": {
                    "class_name": "SurfaceAppearance",
                    "properties": {
                        "ColorMap": "rbxassetid://1111",
                        "NormalMap": "rbxassetid://2222",
                        "RoughnessMap": "rbxassetid://3333",
                        "AlphaMode": "Overlay"
                    }
                }
            },
            "lighting": { "ClockTime": 18.5, "Brightness": 3.0 }
        });
        assert!(page1.to_string().len() < 65536);
        assert!(worker.submit_result(&cmd1.id, true, page1, None, None));

        // Page 2: script part 2 + PointLight + ParticleEmitter + ScreenGui tree
        let cmd2 = worker.poll_next_command(Duration::from_secs(2)).unwrap();
        assert_eq!(cmd2.tool, "get_place_snapshot");
        assert_eq!(cmd2.args["cursor"], json!("2"));
        assert_eq!(cmd2.args["snapshot_id"], json!("snap-e1-1"));
        let page2 = json!({
            "snapshot_id": "snap-e1-1",
            "cursor": "2",
            "next_cursor": Value::Null,
            "done": true,
            "scripts": {
                "ServerScriptService.Server.Main": "print(\"hello\")\n"
            },
            "script_parts": {
                "ServerScriptService.Server.Huge": { "index": 2, "total": 2, "data": "print(\"part2\")\n" }
            },
            "instances": {
                "Workspace.Tower.Lamp": {
                    "class_name": "PointLight",
                    "properties": { "Brightness": 2.5, "Range": 24.0, "Shadows": true }
                },
                "Workspace.Tower.Sparks": {
                    "class_name": "ParticleEmitter",
                    "properties": { "Rate": 30, "Enabled": true }
                },
                "StarterGui.Hud": {
                    "class_name": "ScreenGui",
                    "properties": { "Enabled": true, "ResetOnSpawn": false }
                },
                "StarterGui.Hud.Title": {
                    "class_name": "TextLabel",
                    "properties": { "Text": "Arena", "Visible": true }
                }
            }
        });
        assert!(page2.to_string().len() < 65536);
        assert!(worker.submit_result(&cmd2.id, true, page2, None, None));
    });

    let mut backend = PlaceBackend::Plugin(Arc::clone(&bridge));
    let snap = backend.get_place_snapshot().expect("paginated snapshot");
    handle.join().unwrap();

    assert_eq!(snap["snapshot_id"], json!("snap-e1-1"));
    assert_eq!(
        snap["scripts"]["ServerScriptService.Server.Huge"],
        json!("-- part 1\nprint(\"part2\")\n")
    );
    assert_eq!(
        snap["scripts"]["ServerScriptService.Server.Main"],
        json!("print(\"hello\")\n")
    );
    assert_eq!(
        snap["instances"]["Workspace.Tower"]["properties"]["MeshId"],
        json!("rbxassetid://99887766")
    );
    assert_eq!(
        snap["instances"]["Workspace.Tower"]["properties"]["Attributes"]["Zone"],
        json!("North")
    );
    assert!(snap["instances"]["Workspace.Tower.PBR"].is_object());
    assert!(snap["instances"]["Workspace.Tower.Lamp"].is_object());
    assert!(snap["instances"]["Workspace.Tower.Sparks"].is_object());
    assert!(snap["instances"]["StarterGui.Hud.Title"].is_object());
    assert_eq!(snap["lighting"]["ClockTime"], json!(18.5));
}

#[test]
fn bidirectional_three_way_pull_diff_and_strategies_e2e() {
    let dir = temp_dir("pull-e2e");
    let ws_dir = dir.join("workspace");
    std::fs::create_dir_all(&ws_dir).unwrap();
    let (port, bridge, _shared) = start_plugin_server(dir.join("data"));

    let (st_ws, _) = send_http(
        port,
        "POST",
        "/api/projects/workspace",
        Some(&json!({ "path": ws_dir.to_string_lossy() })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_ws, 200);

    let (st_cr, cr_json) = send_http(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Sync Arena" })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_cr, 201);
    let pid = cr_json["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(cr_json["project"]["path"].as_str().unwrap());

    // Write initial local scripts + baseline
    std::fs::write(
        proj_dir.join("src/server/Main.server.luau"),
        "print(\"v1\")\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/server/Conflict.server.luau"),
        "print(\"base\")\n",
    )
    .unwrap();
    openro_rs::studio_sync::update_baseline_from_project(
        &proj_dir,
        projects::DEFAULT_ROJO_FILE,
        Some("snap-base-1"),
    )
    .unwrap();
    assert!(proj_dir.join(".openro/sync-baseline.json").is_file());

    // Now create 4-way divergence:
    // 1. Main.server.luau: local unchanged ("v1"), Studio changed ("v2-studio") -> studio_modified
    // 2. Conflict.server.luau: local changed ("local-edit"), Studio changed ("studio-edit") -> conflict
    // 3. LocalOnly.server.luau: added locally only -> local_only
    // 4. StudioNew.server.luau + Workspace.ArenaGate (MeshPart) + Lighting: added in Studio -> studio_only
    std::fs::write(
        proj_dir.join("src/server/Conflict.server.luau"),
        "print(\"local-edit\")\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/server/LocalOnly.server.luau"),
        "print(\"local-only\")\n",
    )
    .unwrap();

    let studio_snap = json!({
        "snapshot_id": "snap-live-2",
        "done": true,
        "next_cursor": Value::Null,
        "scripts": {
            "ServerScriptService.Server.Main": "print(\"v2-studio\")\n",
            "ServerScriptService.Server.Conflict": "print(\"studio-edit\")\n",
            "ServerScriptService.Server.StudioNew": "print(\"from-studio\")\n",
            "StarterPlayer.StarterPlayerScripts.Client.Main": "print(\"client ready\")\n",
            "ReplicatedStorage.Shared.Config": "return {}\n"
        },
        "script_meta": {
            "ServerScriptService.Server.StudioNew": { "class_name": "Script" }
        },
        "instances": {
            "Workspace.ArenaGate": {
                "class_name": "MeshPart",
                "properties": {
                    "CFrame": {
                        "position": [0.0, 5.0, 12.0],
                        "orientation": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                    },
                    "Size": [10.0, 10.0, 2.0],
                    "MeshId": "rbxassetid://445566",
                    "Anchored": true,
                    "Attributes": { "GateId": "NorthGate" }
                }
            }
        },
        "lighting": {
            "ClockTime": 20.0,
            "Brightness": 1.5
        }
    });

    // Spawn a plugin responder thread serving studio_snap for each get_place_snapshot call
    let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop_flag);
    let worker = Arc::clone(&bridge);
    let snap_clone = studio_snap.clone();
    let responder = std::thread::spawn(move || {
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(cmd) = worker.poll_next_command(Duration::from_millis(200)) {
                if cmd.tool == "get_place_snapshot" {
                    let _ = worker.submit_result(&cmd.id, true, snap_clone.clone(), None, None);
                } else {
                    let _ = worker.submit_result(&cmd.id, false, json!({}), Some("unexpected".into()), None);
                }
            }
        }
    });

    // Loopback-403 check
    let (st_403, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull-diff"),
        Some(&json!({})),
        "evil.example:4040",
    );
    assert_eq!(st_403, 403);

    // 1. pull-diff
    let (st_diff, diff_json) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull-diff"),
        Some(&json!({})),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_diff, 200, "pull-diff failed: {diff_json}");
    assert_eq!(diff_json["ok"], json!(true));
    assert_eq!(diff_json["snapshot_id"], json!("snap-live-2"));
    assert_eq!(diff_json["conflict_count"], json!(1));

    let changes = diff_json["changes"].as_array().unwrap();
    let find_status = |p: &str| -> Option<String> {
        changes
            .iter()
            .find(|c| c["path"].as_str() == Some(p))
            .and_then(|c| c["status"].as_str())
            .map(String::from)
    };
    assert_eq!(find_status("src/server/Main.server.luau").as_deref(), Some("studio_modified"));
    assert_eq!(find_status("src/server/Conflict.server.luau").as_deref(), Some("conflict"));
    assert_eq!(find_status("src/server/LocalOnly.server.luau").as_deref(), Some("local_only"));
    assert_eq!(find_status("src/server/StudioNew.server.luau").as_deref(), Some("studio_only"));
    assert_eq!(find_status("Workspace.ArenaGate").as_deref(), Some("studio_only"));
    assert_eq!(find_status("Lighting").as_deref(), Some("studio_only"));

    // Verify path traversal rejection
    let (st_trav, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull"),
        Some(&json!({ "paths": ["../escape.luau"], "strategy": "merge_non_conflicting" })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_trav, 400);

    // 2. merge_non_conflicting: pulls studio_only & studio_modified, keeps conflict & local_only
    let (st_merge, merge_json) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull"),
        Some(&json!({ "strategy": "merge_non_conflicting" })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_merge, 200, "merge failed: {merge_json}");
    assert_eq!(merge_json["ok"], json!(true));
    assert!(merge_json["applied_count"].as_u64().unwrap() >= 4);
    assert_eq!(merge_json["conflict_count"], json!(1));
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/Main.server.luau")).unwrap(),
        "print(\"v2-studio\")\n"
    );
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/StudioNew.server.luau")).unwrap(),
        "print(\"from-studio\")\n"
    );
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/Conflict.server.luau")).unwrap(),
        "print(\"local-edit\")\n"
    );
    assert!(proj_dir.join("scene/workspace.model.json").is_file());
    let ws_model: Value = serde_json::from_str(
        &std::fs::read_to_string(proj_dir.join("scene/workspace.model.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ws_model["children"][0]["name"], json!("ArenaGate"));
    assert_eq!(
        ws_model["children"][0]["properties"]["MeshId"],
        json!("rbxassetid://445566")
    );

    // 3. take_studio without confirm_overwrite on remaining conflict -> 409 Conflict
    let (st_409, err_409) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull"),
        Some(&json!({
            "paths": ["src/server/Conflict.server.luau"],
            "strategy": "take_studio"
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_409, 409, "expected 409 confirm gate: {err_409}");

    // 4. take_studio WITH confirm_overwrite: true -> applies conflict & updates baseline
    let (st_take, take_json) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/sync/pull"),
        Some(&json!({
            "paths": ["src/server/Conflict.server.luau"],
            "strategy": "take_studio",
            "confirm_overwrite": true
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_take, 200, "take_studio failed: {take_json}");
    assert_eq!(take_json["applied_count"], json!(1));
    assert_eq!(take_json["conflict_count"], json!(0));
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/Conflict.server.luau")).unwrap(),
        "print(\"studio-edit\")\n"
    );

    stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    responder.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefab_import_rbxmx_rbxlx_rbxm_and_security_guards() {
    let dir = temp_dir("prefab-import");
    let ws_dir = dir.join("workspace");
    std::fs::create_dir_all(&ws_dir).unwrap();
    let (port, _bridge, _shared) = start_plugin_server(dir.join("data"));

    send_http(
        port,
        "POST",
        "/api/projects/workspace",
        Some(&json!({ "path": ws_dir.to_string_lossy() })),
        &format!("127.0.0.1:{port}"),
    );
    let (_, cr_json) = send_http(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Prefab Place" })),
        &format!("127.0.0.1:{port}"),
    );
    let pid = cr_json["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(cr_json["project"]["path"].as_str().unwrap());

    // 1. Valid .rbxmx with MeshPart + SurfaceAppearance + PointLight + Script + LocalScript
    let rbxmx = r#"<roblox version="4">
  <Item class="Model">
    <Properties>
      <string name="Name">LootChest</string>
    </Properties>
    <Item class="MeshPart">
      <Properties>
        <string name="Name">ChestBody</string>
        <Content name="MeshId"><url>rbxassetid://777111</url></Content>
        <Vector3 name="size"><X>4</X><Y>3</Y><Z>3</Z></Vector3>
        <CoordinateFrame name="CFrame"><X>1</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
      </Properties>
      <Item class="SurfaceAppearance">
        <Properties>
          <string name="Name">WoodPBR</string>
          <Content name="ColorMap"><url>rbxassetid://888222</url></Content>
        </Properties>
      </Item>
      <Item class="PointLight">
        <Properties>
          <string name="Name">Glow</string>
          <float name="Brightness">2</float>
          <float name="Range">16</float>
        </Properties>
      </Item>
    </Item>
    <Item class="Script">
      <Properties>
        <string name="Name">OpenChest</string>
        <ProtectedString name="Source"><![CDATA[print("open chest")]]></ProtectedString>
      </Properties>
    </Item>
  </Item>
</roblox>"#;

    let (st_imp, imp_json) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "LootChest.rbxmx",
            "format": "rbxmx",
            "bytes_base64": b64_encode(rbxmx.as_bytes()),
            "target_service": "Workspace"
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_imp, 200, "import rbxmx failed: {imp_json}");
    assert_eq!(imp_json["ok"], json!(true));
    assert_eq!(imp_json["model_path"], json!("scene/models/lootchest.model.json"));
    assert_eq!(imp_json["wired_service"], json!("Workspace"));
    let scripts = imp_json["extracted_scripts"].as_array().unwrap();
    assert_eq!(scripts.len(), 1);
    assert!(proj_dir.join("scene/models/lootchest.model.json").is_file());
    assert!(proj_dir.join(scripts[0].as_str().unwrap()).is_file());

    // Verify Rojo tree loads cleanly and builds to .rbxlx
    let tree = projects::load_tree(&proj_dir, projects::DEFAULT_ROJO_FILE).unwrap();
    let flat = tree.flatten();
    assert!(flat.iter().any(|f| f.path == "Workspace/LootChest/ChestBody"));
    assert!(flat.iter().any(|f| f.path == "Workspace/LootChest/OpenChest"));

    // 2. Valid minimal binary .rbxm (32-byte <roblox! header + END\0 chunk)
    let mut rbxm = Vec::new();
    rbxm.extend_from_slice(b"<roblox!\x89\xff\x0d\x0a\x1a\x0a");
    rbxm.extend_from_slice(&0u16.to_le_bytes()); // version
    rbxm.extend_from_slice(&0u32.to_le_bytes()); // class_count
    rbxm.extend_from_slice(&0u32.to_le_bytes()); // inst_count
    rbxm.extend_from_slice(&[0u8; 8]); // reserved
    rbxm.extend_from_slice(b"END\0");
    rbxm.extend_from_slice(&0u32.to_le_bytes());
    rbxm.extend_from_slice(&0u32.to_le_bytes());
    rbxm.extend_from_slice(&0u32.to_le_bytes());
    let (st_bin, bin_json) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "MinimalBin.rbxm",
            "format": "rbxm",
            "bytes_base64": b64_encode(&rbxm),
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_bin, 200, "minimal rbxm failed: {bin_json}");
    assert_eq!(bin_json["model_path"], json!("scene/models/minimalbin.model.json"));
    assert!(!bin_json["warnings"].as_array().unwrap().is_empty());

    // 3. Security rejections (400 Bad Request)
    // XXE DOCTYPE/ENTITY
    let xxe = r#"<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><roblox version="4"></roblox>"#;
    let (st_xxe, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "Evil.rbxmx",
            "format": "rbxmx",
            "bytes_base64": b64_encode(xxe.as_bytes()),
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_xxe, 400);

    // Path traversal file_name
    let (st_cwe22, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "../../escape.rbxmx",
            "format": "rbxmx",
            "bytes_base64": b64_encode(rbxmx.as_bytes()),
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_cwe22, 400);

    // Corrupt .rbxm bytes
    let (st_bad_bin, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "Corrupt.rbxm",
            "format": "rbxm",
            "bytes_base64": b64_encode(b"ROBLOX!corrupt_truncated_bytes"),
        })),
        &format!("127.0.0.1:{port}"),
    );
    assert_eq!(st_bad_bin, 400);

    // Loopback-403
    let (st_remote, _) = send_http(
        port,
        "POST",
        &format!("/api/projects/{pid}/import-model"),
        Some(&json!({
            "file_name": "LootChest.rbxmx",
            "format": "rbxmx",
            "bytes_base64": b64_encode(rbxmx.as_bytes()),
        })),
        "attacker.example:4040",
    );
    assert_eq!(st_remote, 403);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_project_sync_pull_tool_executes_against_plugin_bridge() {
    let dir = temp_dir("agent-pull");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        serde_json::to_string_pretty(&json!({
            "name": "AgentPull",
            "tree": {
                "$className": "DataModel",
                "ServerScriptService": {
                    "$ignoreUnknownInstances": true,
                    "Server": { "$path": "src/server" }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(proj_dir.join("src/server/Main.server.luau"), "print(1)\n").unwrap();

    let bridge = StudioPluginBridge::open(None);
    bridge.record_handshake("Roblox Studio (Edit)", "AgentPull", vec![]);
    let worker = Arc::clone(&bridge);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let responder = std::thread::spawn(move || {
        while !stop_c.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(cmd) = worker.poll_next_command(Duration::from_millis(150)) {
                if cmd.tool == "get_place_snapshot" {
                    let _ = worker.submit_result(
                        &cmd.id,
                        true,
                        json!({
                            "snapshot_id": "snap-agent-1",
                            "done": true,
                            "next_cursor": Value::Null,
                            "scripts": {
                                "ServerScriptService.Server.Main": "print(1)\n",
                                "ServerScriptService.Server.Pulled": "print(\"pulled\")\n"
                            },
                            "instances": {}
                        }),
                        None,
                        None,
                    );
                }
            }
        }
    });

    // Stub OpenAI-compatible model endpoint: turn 1 -> project_sync_pull, turn 2 -> done
    let model_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let model_port = model_listener.local_addr().unwrap().port();
    let stop_model = Arc::clone(&stop);
    let model_thread = std::thread::spawn(move || {
        let mut turn = 0usize;
        let _ = model_listener.set_nonblocking(true);
        while !stop_model.load(std::sync::atomic::Ordering::Relaxed) {
            match model_listener.accept() {
                Ok((mut s, _)) => {
                    let mut buf = [0u8; 8192];
                    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                    let _ = s.read(&mut buf);
                    turn += 1;
                    let content = if turn == 1 {
                        json!({
                            "tool": "project_sync_pull",
                            "args": { "strategy": "merge_non_conflicting" }
                        })
                        .to_string()
                    } else {
                        json!({
                            "done": true,
                            "summary": "Pulled changes from Studio."
                        })
                        .to_string()
                    };
                    let payload = json!({
                        "id": format!("chatcmpl-{turn}"),
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": content },
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
                    let _ = s.flush();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });

    let model = ModelClient::new(
        format!("http://127.0.0.1:{model_port}/v1"),
        "stub".into(),
        None,
    );
    let mut agent = Agent::with_backend(model, Some(PlaceBackend::Plugin(Arc::clone(&bridge))));
    agent.set_project(Some(proj_dir.clone()), Some("agent-pull".into()));

    let outcome = agent
        .run_ask("chat-pull-1", "Pull Studio changes into the project.")
        .expect("agent project_sync_pull succeeds");
    assert!(outcome.summary.contains("Pulled"));
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/Pulled.server.luau")).unwrap(),
        "print(\"pulled\")\n"
    );

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    responder.join().unwrap();
    model_thread.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pull_apply_workspace_part_with_nested_script_preserves_parent_and_loads_cleanly() {
    let dir = temp_dir("openro-v127-ws-overlay");
    let mut store = openro_rs::projects::ProjectStore::load(Some(&dir));
    let proj = store
        .create("WS Overlay", None, None, None)
        .expect("create project");

    let mut backend = PlaceBackend::Local(openro_rs::place::LocalPlace::in_memory(
        "Overlay Place".into(),
    ));
    backend
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Part",
                "name": "LavaBrick",
                "parent": "Workspace",
                "properties": { "Anchored": true }
            }),
        )
        .unwrap();
    backend
        .call_tool(
            "write_script",
            json!({
                "path": "Workspace.LavaBrick.KillBrick",
                "source": "print(script.Parent.ClassName)\n"
            }),
        )
        .unwrap();
    let applied = openro_rs::studio_sync::pull_apply_with_backend(
        &mut backend,
        &proj.dir,
        None,
        openro_rs::studio_sync::PullStrategy::MergeNonConflicting,
        false,
    )
    .expect("pull_apply succeeds");
    assert!(applied.applied_count >= 2);

    let tree = projects::load_tree(&proj.dir, projects::DEFAULT_ROJO_FILE)
        .expect("load_tree succeeds without duplicate LavaBrick error");
    let flat = tree.flatten();
    let lava_nodes: Vec<_> = flat.iter().filter(|f| f.path == "Workspace/LavaBrick").collect();
    assert_eq!(lava_nodes.len(), 1);
    assert_eq!(lava_nodes[0].class, "Part");
    let kill_nodes: Vec<_> = flat
        .iter()
        .filter(|f| f.path == "Workspace/LavaBrick/KillBrick")
        .collect();
    assert_eq!(kill_nodes.len(), 1);
    assert_eq!(kill_nodes[0].class, "Script");

    let xml = r#"<roblox version="4"><Item class="Model"><Properties><string name="Name">GateTower</string></Properties><Item class="Part"><Properties><string name="Name">TowerBase</string></Properties></Item></Item></roblox>"#;
    let imp = projects::import_prefab_model(
        &proj.dir,
        "GateTower.rbxmx",
        "rbxmx",
        &openro_rs::assets::encode_base64(xml.as_bytes()),
        Some("Workspace"),
    )
    .expect("import_prefab_model succeeds after pull_apply");
    assert_eq!(imp["ok"], json!(true));

    let _ = std::fs::remove_dir_all(&dir);
}
