//! Integration tests for OpenRO v0.9.0 Embedded Roblox Studio backend:
//! - All 13 official Roblox Studio MCP tools via live `mock_studio` stdio process
//! - Interactive `screen_capture` PNG verification across mouse click/drag/wheel,
//!   keyboard press/text, `multi_edit`, `execute_luau`, and `start_stop_play`
//! - `StudioEmbedManager` lifecycle, strict non-auto-launch, X11 validation,
//!   Windows `%LOCALAPPDATA%\Roblox\Versions\version-*` scanner, and MJPEG framing
//! - Cross-platform chromeless desktop window launcher (`src/desktop.rs`).

use openro_rs::desktop::{
    build_chromium_app_plan, launch_desktop_window_with_config, should_auto_open,
    windows_browser_candidates, DesktopLaunchConfig,
};
use openro_rs::mcp::{
    is_allowed_studio_tool, official_studio_tool_catalog, split_command_line,
    OFFICIAL_STUDIO_TOOLS,
};
use openro_rs::studio_embed::{
    find_windows_roblox_paths, format_mjpeg_part, forward_keyboard, forward_mouse,
    launch_embedded_studio, normalize_roblox_keycode, stop_embedded_studio,
    translate_keyboard_payload, translate_mouse_payload, validate_x11_display, EmbedConfig,
    StudioEmbedManager,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_test_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "openro-v090-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn all_thirteen_official_mcp_tools_and_interactive_png_updates() {
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let mut manager = StudioEmbedManager::new();
    assert_eq!(manager.state().phase, "idle");
    assert!(!manager.state().embedded);
    assert_eq!(manager.state().xvfb_pid, None);
    assert_eq!(manager.state().studio_pid, None);

    let cfg = EmbedConfig::from_json(&json!({
        "mcp_command": mock_bin,
        "launch_studio_process": false,
        "width": 1280,
        "height": 720,
    }))
    .expect("parse EmbedConfig");
    assert!(!cfg.launch_studio_process);

    let (state, mut client) =
        launch_embedded_studio(&mut manager, cfg).expect("attach mock_studio via StudioEmbedManager");
    assert_eq!(state.phase, "connected");
    assert!(state.embedded);
    assert!(state.mcp_connected);
    assert_eq!(state.studio_id.as_deref(), Some("mock-studio-01"));
    assert_eq!(state.datamodel_type, "Edit");
    assert_eq!(state.official_tools_count, OFFICIAL_STUDIO_TOOLS.len());
    assert_eq!(client.official_tools_available().len(), 13);
    assert_eq!(official_studio_tool_catalog().len(), 13);

    // 1. list_roblox_studios
    let studios = client
        .list_roblox_studios()
        .expect("list_roblox_studios");
    assert_eq!(studios["count"], 1);
    assert_eq!(studios["studios"][0]["studio_id"], "mock-studio-01");

    // 2. get_studio_state
    let st0 = client.get_studio_state().expect("get_studio_state");
    assert_eq!(st0["playing"], false);
    assert_eq!(st0["active_datamodel"], "Edit");

    // 3. execute_luau -> creates Workspace.ArenaCore + prints to Output console
    let exec_res = client
        .execute_luau(
            r#"local p = Instance.new("Part")
p.Name = "ArenaCore"
p.BrickColor = BrickColor.new("Bright green")
print("ArenaCore Ready!")"#,
        )
        .expect("execute_luau");
    assert_eq!(exec_res["ok"], true);
    assert_eq!(exec_res["created_instances"][0], "Workspace.ArenaCore");
    assert_eq!(exec_res["output"], "ArenaCore Ready!");

    // 4. search_game_tree -> finds ArenaCore under Workspace with BasePart IsA filter
    let tree = client
        .search_game_tree(Some("Workspace"), Some("arena"), Some("BasePart"), Some(3), Some(50))
        .expect("search_game_tree");
    assert_eq!(tree["count"], 1);
    assert_eq!(tree["results"][0]["path"], "Workspace.ArenaCore");

    // 5. inspect_instance -> case-insensitive lookup with `game.` prefix
    let inspected = client
        .inspect_instance("game.workspace.arenacore")
        .expect("inspect_instance");
    assert_eq!(inspected["path"], "Workspace.ArenaCore");
    assert_eq!(inspected["class_name"], "Part");
    assert_eq!(inspected["properties"]["Color"], "#22C55E");

    // 6. multi_edit -> create script, then apply targeted multi-hunk edit
    let created_sc = client
        .multi_edit(
            "game.ServerScriptService.BattleLoop",
            vec![json!({
                "old_string": "",
                "new_string": "local ROUND_SECS = 60\nlocal BONUS = 10\nprint(\"BattleLoop v1\")\n"
            })],
            Some("Script"),
        )
        .expect("multi_edit create");
    assert_eq!(created_sc["ok"], true);

    let edited_sc = client
        .multi_edit(
            "ServerScriptService.BattleLoop",
            vec![
                json!({ "old_string": "ROUND_SECS = 60", "new_string": "ROUND_SECS = 90" }),
                json!({ "old_string": "BattleLoop v1", "new_string": "BattleLoop v2" }),
            ],
            None,
        )
        .expect("multi_edit update");
    assert_eq!(edited_sc["edits_applied"], 2);

    // 7. script_read -> full file & line range
    let full_read = client
        .script_read("game.ServerScriptService.BattleLoop", None, None)
        .expect("script_read full");
    assert_eq!(full_read["total_lines"], 3);
    assert!(full_read["source"]
        .as_str()
        .unwrap_or("")
        .contains("ROUND_SECS = 90"));

    let range_read = client
        .script_read("ServerScriptService.BattleLoop", Some(2), Some(3))
        .expect("script_read range");
    assert_eq!(range_read["start_line"], 2);
    assert_eq!(range_read["end_line"], 3);
    assert!(range_read["source"]
        .as_str()
        .unwrap_or("")
        .contains("BattleLoop v2"));

    // 8. script_grep
    let grep = client
        .script_grep("BattleLoop v2")
        .expect("script_grep");
    assert_eq!(grep["match_count"], 1);
    assert_eq!(grep["matches"][0]["path"], "ServerScriptService.BattleLoop");
    assert_eq!(grep["matches"][0]["line"], 3);

    // 9. get_console_output
    let console = client
        .get_console_output()
        .expect("get_console_output");
    assert!(
        console.iter().any(|l| l.contains("ArenaCore Ready!")),
        "expected print output in console: {console:?}"
    );
    assert!(console.iter().any(|l| l.contains("[MultiEdit]")));

    // 10. screen_capture -> baseline PNG frame
    let frame1 = client.screen_capture().expect("screen_capture 1");
    assert_eq!(frame1.mime, "image/png");
    assert_eq!((frame1.width, frame1.height), (320, 200));
    assert!(frame1.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // Verify MJPEG multipart part formatting
    let part = format_mjpeg_part(&frame1, "openroframe");
    let part_head = String::from_utf8_lossy(&part[..120]);
    assert!(part_head.contains("--openroframe\r\n"));
    assert!(part_head.contains("Content-Type: image/png\r\n"));
    assert!(part_head.contains(&format!("Content-Length: {}\r\n", frame1.bytes.len())));
    assert!(part_head.contains("X-Studio-Frame-Seq: "));

    // 11. forward_mouse (`user_mouse_input`): click -> drag -> wheel -> verify distinct PNGs
    let click_res = forward_mouse(
        &mut client,
        &json!({ "event": "click", "x": 160, "y": 115, "button": "left" }),
    )
    .expect("forward_mouse click");
    assert_eq!(click_res["ok"], true);
    assert_eq!(click_res["selected_instance"], "Workspace.ArenaCore");
    let frame2 = client.screen_capture().expect("screen_capture 2");
    assert_ne!(
        frame1.bytes, frame2.bytes,
        "mouse click + selection highlight must update rendered PNG frame"
    );

    let drag_res = forward_mouse(
        &mut client,
        &json!({ "event": "drag", "x": 160, "y": 115, "end_x": 240, "end_y": 95, "button": "right" }),
    )
    .expect("forward_mouse drag");
    assert!(drag_res["camera"]["yaw"].as_f64().unwrap_or(0.0).abs() > 0.1);
    let wheel_res = forward_mouse(
        &mut client,
        &json!({ "event": "wheel", "x": 160, "y": 115, "delta_y": -120 }),
    )
    .expect("forward_mouse wheel");
    assert!(wheel_res["camera"]["zoom"].as_f64().unwrap_or(1.0) > 1.0);
    let frame3 = client.screen_capture().expect("screen_capture 3");
    assert_ne!(
        frame2.bytes, frame3.bytes,
        "camera orbit drag + zoom must update rendered PNG frame"
    );

    // 12. forward_keyboard (`user_keyboard_input`): WASD pan + text input -> verify distinct PNG
    let key_res = forward_keyboard(
        &mut client,
        &json!({ "event": "keydown", "key": "w" }),
    )
    .expect("forward_keyboard W");
    assert_eq!(key_res["last_key"], "W");
    let txt_res = forward_keyboard(
        &mut client,
        &json!({ "event": "text", "text": "ShadowBlade" }),
    )
    .expect("forward_keyboard text");
    assert_eq!(txt_res["typed_text"], "ShadowBlade");
    let frame4 = client.screen_capture().expect("screen_capture 4");
    assert_ne!(
        frame3.bytes, frame4.bytes,
        "keyboard input indicator + pan must update rendered PNG frame"
    );

    // 13. start_stop_play -> start_play, run_server, stop
    let play_res = client
        .start_stop_play("start_play")
        .expect("start_stop_play start_play");
    assert_eq!(play_res["playing"], true);
    assert_eq!(client.datamodel_type(), "Client");
    let frame5 = client.screen_capture().expect("screen_capture 5");
    assert_ne!(
        frame4.bytes, frame5.bytes,
        "play mode green indicator ribbon must update rendered PNG frame"
    );

    let srv_res = client
        .start_stop_play("run_server")
        .expect("start_stop_play run_server");
    assert_eq!(srv_res["playing"], true);
    assert_eq!(client.datamodel_type(), "Server");

    let stop_res = client.start_stop_play("stop").expect("start_stop_play stop");
    assert_eq!(stop_res["playing"], false);
    assert_eq!(client.datamodel_type(), "Edit");

    // Unified official tool dispatcher + strict allowlist
    let via_dispatch = client
        .call_official_tool("get_studio_state", json!({}))
        .expect("call_official_tool get_studio_state");
    assert_eq!(via_dispatch["studio_id"], "mock-studio-01");
    assert!(!is_allowed_studio_tool("shell_exec"));
    assert!(client
        .call_official_tool("shell_exec", json!({}))
        .is_err());

    // Optional PNG dump for external Python PIL/zlib verification
    if let Ok(dir) = std::env::var("OPENRO_DUMP_EMBED_PNG_DIR") {
        if !dir.trim().is_empty() {
            let p = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&p);
            for (i, f) in [&frame1, &frame2, &frame3, &frame4, &frame5]
                .iter()
                .enumerate()
            {
                std::fs::write(p.join(format!("frame-{}.png", i + 1)), &f.bytes)
                    .expect("write dumped png");
            }
        }
    }

    // Clean explicit teardown
    let stopped = stop_embedded_studio(&mut manager);
    assert_eq!(stopped.phase, "stopped");
    assert!(!stopped.embedded);
    assert!(!stopped.mcp_connected);
    assert_eq!(stopped.xvfb_pid, None);
    assert_eq!(stopped.studio_pid, None);
}

#[test]
fn windows_roblox_path_scanner_and_x11_display_validation() {
    // X11 display sanitizer rejects shell metacharacters & bogus values
    assert_eq!(validate_x11_display(":99").unwrap(), ":99");
    assert_eq!(validate_x11_display(":0").unwrap(), ":0");
    for bad in ["", "99", ":abc", ":9999", ":99; rm -rf /", "../:99"] {
        assert!(
            validate_x11_display(bad).is_err(),
            "display '{bad}' must be rejected"
        );
    }

    // EmbedConfig rejects control characters in mcp_command
    assert!(EmbedConfig::from_json(&json!({ "mcp_command": "foo\nbar" })).is_err());
    assert!(EmbedConfig::from_json(&json!({ "mcp_command": "foo\0bar" })).is_err());

    // Synthetic Windows %LOCALAPPDATA%\Roblox\Versions\version-* tree
    let tmp = temp_test_dir("win-roblox");
    let v_old = tmp.join("Roblox").join("Versions").join("version-aaaa1111");
    let v_new = tmp.join("Roblox").join("Versions").join("version-zzzz9999");
    std::fs::create_dir_all(&v_old).unwrap();
    std::fs::create_dir_all(&v_new).unwrap();
    std::fs::write(v_old.join("RobloxStudioBeta.exe"), b"old").unwrap();
    std::fs::write(v_new.join("RobloxStudioBeta.exe"), b"new").unwrap();
    std::fs::write(v_new.join("StudioMCP.exe"), b"mcp").unwrap();
    std::fs::write(tmp.join("Roblox").join("mcp.bat"), b"@echo off").unwrap();

    let found = find_windows_roblox_paths(&tmp);
    assert_eq!(found.studio_exe, Some(v_new.join("RobloxStudioBeta.exe")));
    assert_eq!(found.studio_mcp_exe, Some(v_new.join("StudioMCP.exe")));
    assert_eq!(found.mcp_bat, Some(tmp.join("Roblox").join("mcp.bat")));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn mouse_and_keyboard_input_translators_normalize_events() {
    let drag = translate_mouse_payload(&json!({
        "event": "drag",
        "x": 100.0,
        "y": 80.0,
        "end_x": 220.0,
        "end_y": 140.0,
        "button": "right"
    }))
    .expect("translate drag");
    assert_eq!(drag.len(), 3);
    assert_eq!(drag[0]["action"], "mouseButtonDown");
    assert_eq!(drag[0]["mouse_button"], "right");
    assert_eq!(drag[1]["action"], "moveTo");
    assert_eq!(drag[2]["action"], "mouseButtonUp");

    let wheel_up = translate_mouse_payload(&json!({ "event": "wheel", "delta_y": -100 }))
        .expect("translate wheel up");
    assert_eq!(wheel_up[0]["action"], "scrollUp");
    assert!(translate_mouse_payload(&json!({ "event": "explode" })).is_err());

    assert_eq!(normalize_roblox_keycode("Enter"), "Return");
    assert_eq!(normalize_roblox_keycode(" "), "Space");
    assert_eq!(normalize_roblox_keycode("ArrowUp"), "Up");
    assert_eq!(normalize_roblox_keycode("KeyW"), "W");
    assert_eq!(normalize_roblox_keycode("Digit7"), "Seven");
    assert_eq!(normalize_roblox_keycode("ShiftLeft"), "LeftShift");

    let k = translate_keyboard_payload(&json!({ "event": "keydown", "key": "Enter" }))
        .expect("translate Enter");
    assert_eq!(k[0]["action"], "keyDown");
    assert_eq!(k[0]["key_code"], "Return");

    let t = translate_keyboard_payload(&json!({ "event": "text", "text": "hello" }))
        .expect("translate text");
    assert_eq!(t[0]["action"], "textInput");
    assert_eq!(t[0]["text_inputs"], "hello");
    assert!(translate_keyboard_payload(&json!({ "event": "text", "text": "bad\0" })).is_err());
}

#[test]
fn desktop_launcher_honors_guards_and_builds_cross_platform_plans() {
    assert!(!should_auto_open(&["openro-rs".to_string(), "--no-open".to_string()]));
    assert!(!should_auto_open(&["openro-rs".to_string(), "--headless".to_string()]));

    let mut cfg = DesktopLaunchConfig::new("http://127.0.0.1:8077", PathBuf::from("/tmp/openro-test"));
    cfg.no_open = true;
    let skipped = launch_desktop_window_with_config(&cfg);
    assert!(skipped.skipped);
    assert!(!skipped.launched);

    // Non-HTTP URLs must be refused even if no_open is false
    let mut bad_url_cfg = DesktopLaunchConfig::new("file:///etc/passwd", PathBuf::from("/tmp/openro-test"));
    bad_url_cfg.no_open = false;
    let refused = launch_desktop_window_with_config(&bad_url_cfg);
    assert!(refused.skipped);
    assert!(!refused.launched);

    // Linux chromeless app plan
    let linux_plan = build_chromium_app_plan("chromium", &cfg, "linux");
    assert!(linux_plan.chromeless);
    assert!(linux_plan.args.contains(&"--app=http://127.0.0.1:8077".to_string()));
    assert!(linux_plan.args.contains(&"--class=OpenRO".to_string()));
    assert!(linux_plan
        .args
        .iter()
        .any(|a| a.starts_with("--user-data-dir=") && a.ends_with("desktop-profile")));

    // Windows chromeless app plan + browser search order
    let win_plan = build_chromium_app_plan("msedge.exe", &cfg, "windows");
    assert!(win_plan.chromeless);
    assert!(win_plan.args.contains(&"--app=http://127.0.0.1:8077".to_string()));
    assert!(!win_plan.args.iter().any(|a| a.starts_with("--class=")));

    let cands = windows_browser_candidates(
        Some(Path::new("C:/Program Files")),
        Some(Path::new("C:/Program Files (x86)")),
        Some(Path::new("C:/Users/Cayleb/AppData/Local")),
    );
    assert!(cands[0].to_string_lossy().contains("msedge.exe"));
    assert!(cands.iter().any(|p| p.to_string_lossy().contains("chrome.exe")));
    assert!(cands.iter().any(|p| p.to_string_lossy().contains("brave.exe")));

    // Quoted Windows MCP command-line splitting
    let parts = split_command_line(r#""C:\Program Files\Roblox\StudioMCP.exe" --stdio --flag"#);
    assert_eq!(
        parts,
        vec![
            r"C:\Program Files\Roblox\StudioMCP.exe".to_string(),
            "--stdio".to_string(),
            "--flag".to_string()
        ]
    );
}
