//! Verification suite for OpenRO v0.7.0 Studio Backend:
//! - `StudioStreamHub`: 500-event bounded ring buffer, `actions_since` / `snapshot_actions`,
//!   `store_frame` / `latest_frame`, 32-subscriber cap + `Drop` release, live SSE `serve_stream`
//!   with `event: status`, `event: studio_action`, `event: studio_frame`, and `Last-Event-ID` replay.
//! - Live MCP `mock_studio` E2E: `screen_capture` dynamic valid PNG decoding (verifying pixels
//!   genuinely change when parts/scripts/play-mode mutate!), `get_console_output`, `start_stop_play`
//!   (`start_play`, `run_server`, `stop`, and failure path), plus `MOCK_DROP=screen_capture` honest refusal.

use openro_rs::assets::detect_image_mime;
use openro_rs::mcp::McpClient;
use openro_rs::studio_stream::{
    StudioActionInput, StudioActionKind, StudioFrame, StudioStreamHub, MAX_STUDIO_RING_ACTIONS,
    MAX_STUDIO_SSE_CLIENTS,
};
use serde_json::{json, Value};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

fn mock_studio_command() -> String {
    env!("CARGO_BIN_EXE_mock_studio").to_string()
}

#[test]
fn studio_stream_hub_bounds_500_ring_replays_since_and_caps_32_subscribers() {
    let hub = Arc::new(StudioStreamHub::new("local_place"));
    assert_eq!(hub.studio_mode(), "local_place");
    assert!(hub.latest_frame().is_none());

    // Push 560 actions to prove 500-event ring eviction & monotonic seq
    for i in 1..=(MAX_STUDIO_RING_ACTIONS + 60) {
        let mut input = StudioActionInput::new(
            if i % 2 == 0 {
                StudioActionKind::PartCreated
            } else {
                StudioActionKind::ScriptWritten
            },
            format!("Action #{i}"),
        );
        input.path = format!("Workspace.Tile{i}");
        input.class_name = "Part".to_string();
        input.position = Some([i as f64, 2.0, 0.0]);
        input.size = Some([6.0, 1.0, 6.0]);
        input.color = Some("#4F7CF0".to_string());
        input.instance_count = i;
        input.script_count = i / 2;
        let ev = hub.record_action(input);
        assert_eq!(ev.seq, i as u64);
    }

    let snap = hub.snapshot_actions();
    assert_eq!(snap.len(), MAX_STUDIO_RING_ACTIONS);
    assert_eq!(snap.first().unwrap().seq, 61);
    assert_eq!(snap.last().unwrap().seq, 560);

    // actions_since filters accurately
    let tail = hub.actions_since(555);
    assert_eq!(tail.len(), 5);
    assert_eq!(tail[0].seq, 556);
    assert_eq!(tail[4].seq, 560);

    // Verify all 14 StudioActionKind variants serialize with zero internal jargon
    for kind in [
        StudioActionKind::PartCreated,
        StudioActionKind::PartUpdated,
        StudioActionKind::ScriptWritten,
        StudioActionKind::DecalBound,
        StudioActionKind::LuauExecuted,
        StudioActionKind::PlaytestStarted,
        StudioActionKind::PlaytestLog,
        StudioActionKind::PlaytestPassed,
        StudioActionKind::PlaytestFailed,
        StudioActionKind::StudioFrame,
        StudioActionKind::CameraFocus,
        StudioActionKind::Reverted,
        StudioActionKind::Imported,
        StudioActionKind::Restored,
    ] {
        assert!(!kind.as_str().is_empty());
        assert!(!kind.label().to_ascii_lowercase().contains("rune"));
    }

    // Verify 32-subscriber cap + RAII Drop release
    let mut subs = Vec::new();
    for _ in 0..MAX_STUDIO_SSE_CLIENTS {
        subs.push(hub.subscribe(0).expect("within 32-client cap"));
    }
    assert_eq!(hub.active_subscribers(), MAX_STUDIO_SSE_CLIENTS);
    assert!(
        hub.subscribe(0).is_err(),
        "33rd subscriber must be refused cleanly"
    );
    subs.pop();
    assert_eq!(hub.active_subscribers(), MAX_STUDIO_SSE_CLIENTS - 1);
    assert!(hub.subscribe(0).is_ok());
}

#[test]
fn studio_stream_hub_sse_delivers_live_studio_action_studio_frame_and_replays() {
    let hub = Arc::new(StudioStreamHub::new("live_mcp"));

    // Record an initial action before the SSE client connects
    let mut a1 = StudioActionInput::new(
        StudioActionKind::PartCreated,
        "Created RollPedestal at (0, 1.5, -14)",
    );
    a1.path = "Workspace.RollPedestal".to_string();
    a1.class_name = "Part".to_string();
    a1.position = Some([0.0, 1.5, -14.0]);
    a1.size = Some([10.0, 2.0, 10.0]);
    a1.color = Some("#4F7CF0".to_string());
    a1.instance_count = 1;
    hub.record_action(a1);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hub_srv = Arc::clone(&hub);
    let server_thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = hub_srv.serve_stream(stream, 0);
    });

    let mut client = TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    // Emit a live script write AND a live StudioFrame while subscriber is attached
    std::thread::sleep(Duration::from_millis(60));
    let mut a2 = StudioActionInput::new(
        StudioActionKind::ScriptWritten,
        "Wrote ServerScriptService.CardGachaRoller",
    );
    a2.path = "ServerScriptService.CardGachaRoller".to_string();
    a2.class_name = "Script".to_string();
    a2.code_snippet = Some("local Players = game:GetService(\"Players\")".to_string());
    a2.instance_count = 1;
    a2.script_count = 1;
    hub.record_action(a2);

    let stored = hub.store_frame(StudioFrame {
        seq: 0,
        ts_ms: 0,
        mime: "image/png".to_string(),
        bytes: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        width: 320,
        height: 200,
        source: "studio_mcp".to_string(),
    });
    assert_eq!(stored.seq, 1);
    assert_eq!(hub.latest_frame_seq(), 1);
    assert_eq!(hub.latest_frame().unwrap().width, 320);

    // Shut down hub so serve_stream closes cleanly after flushing
    std::thread::sleep(Duration::from_millis(60));
    hub.shutdown();
    server_thread.join().unwrap();

    let mut body = String::new();
    let _ = client.read_to_string(&mut body);
    assert!(body.contains("HTTP/1.1 200 OK"), "{body}");
    assert!(body.contains("event: status"), "{body}");
    assert!(body.contains("event: studio_action"), "{body}");
    assert!(body.contains("event: studio_frame"), "{body}");
    assert!(body.contains("ServerScriptService.CardGachaRoller"), "{body}");
    assert!(body.contains("/api/studio/frame?seq=1"), "{body}");

    // Snapshot JSON matches GET /api/studio/actions contract
    let snap = hub.snapshot_json(0);
    assert_eq!(snap["studio_mode"], json!("live_mcp"));
    assert_eq!(snap["has_live_frame"], json!(true));
    assert_eq!(snap["frame_seq"], json!(1));
    assert_eq!(snap["actions"].as_array().unwrap().len(), 3);

    // Last-Event-ID replay test on a fresh hub
    let hub2 = Arc::new(StudioStreamHub::new("local_place"));
    for k in 1..=4 {
        hub2.record_action(StudioActionInput::new(
            StudioActionKind::PlaytestLog,
            format!("Log line {k}"),
        ));
    }
    let mut sub = hub2.subscribe(2).unwrap();
    let replayed = sub.drain_pending();
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].stream_seq, 3);
    assert_eq!(replayed[1].stream_seq, 4);
}

#[test]
fn mock_studio_live_mcp_screen_capture_png_console_output_and_start_stop_play() {
    let mut mcp = McpClient::spawn(&mock_studio_command()).expect("spawn mock_studio");
    assert!(mcp.has_tool("screen_capture"));
    assert!(mcp.has_tool("get_console_output"));
    assert!(mcp.has_tool("start_stop_play"));

    // 1. Capture baseline frame (empty workspace) -> must decode as genuine 320x200 PNG
    let frame1 = mcp.screen_capture().expect("baseline screen_capture");
    assert_eq!(frame1.mime, "image/png");
    assert_eq!(frame1.width, 320);
    assert_eq!(frame1.height, 200);
    assert_eq!(frame1.source, "studio_mcp");
    assert_eq!(detect_image_mime(&frame1.bytes).unwrap(), ("image/png", "png"));

    // 2. Place colored 3D parts & write a script -> capture again and prove PNG bytes genuinely changed!
    mcp.call_tool(
        "create_instance",
        json!({
            "class_name": "SpawnLocation",
            "name": "SpawnPad",
            "parent": "Workspace",
            "properties": { "Anchored": true, "Position": [0, 2, 0], "Size": [10, 1, 10], "Color": "#4F7CF0" }
        }),
    )
    .unwrap();
    mcp.call_tool(
        "create_instance",
        json!({
            "class_name": "Part",
            "name": "BossRaidShrine",
            "parent": "Workspace",
            "properties": { "Anchored": true, "Position": [18, 4, -12], "Size": [12, 8, 8], "Color": "#E0532A" }
        }),
    )
    .unwrap();
    mcp.call_tool(
        "write_script",
        json!({
            "path": "ServerScriptService.Leaderboard",
            "source": "local Players = game:GetService(\"Players\")\nprint('ok')\n"
        }),
    )
    .unwrap();

    let frame2 = mcp.screen_capture().expect("post-build screen_capture");
    assert_eq!(frame2.width, 320);
    assert_eq!(frame2.height, 200);
    assert_ne!(
        frame1.bytes, frame2.bytes,
        "rendered Studio PNG must reflect placed parts & scripts"
    );

    if let Ok(dump_path) = std::env::var("OPENRO_DUMP_STUDIO_PNG") {
        std::fs::write(&dump_path, &frame2.bytes).expect("write studio PNG dump");
    }

    // 3. start_stop_play("start_play") -> passes, toggles play mode, updates console output, changes frame!
    let play_res = mcp
        .start_stop_play("start_play")
        .expect("start_play succeeds");
    assert_eq!(play_res["ok"], json!(true));
    assert_eq!(play_res["playing"], json!(true));
    assert_eq!(play_res["status"], json!("passed"));

    let frame3 = mcp.screen_capture().expect("play-mode screen_capture");
    assert_ne!(
        frame2.bytes, frame3.bytes,
        "play-mode capture should visibly show active playtest indicator"
    );

    // 4. get_console_output returns accumulated structured lines
    let lines = mcp
        .get_console_output()
        .expect("get_console_output succeeds");
    assert!(lines.len() >= 4, "expected accumulated lines: {lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("Workspace.BossRaidShrine")),
        "console includes part creation: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("Playtest OK")),
        "console includes playtest verdict: {lines:?}"
    );

    // 5. stop playtest & reject invalid mode cleanly
    let stop_res = mcp.start_stop_play("stop").expect("stop succeeds");
    assert_eq!(stop_res["playing"], json!(false));
    assert_eq!(stop_res["status"], json!("stopped"));
    assert!(mcp.start_stop_play("teleport_mars").is_err());

    // 6. Store captured live frame into StudioStreamHub end-to-end
    let hub = Arc::new(StudioStreamHub::new("live_mcp"));
    let stored = hub.store_frame(frame3);
    assert_eq!(stored.seq, 1);
    assert_eq!(hub.latest_frame().unwrap().width, 320);
    let snap: Value = hub.snapshot_json(0);
    assert_eq!(snap["has_live_frame"], json!(true));
    assert_eq!(snap["frame"]["width"], json!(320));
    assert_eq!(snap["frame"]["height"], json!(200));

    // 7. Bridge dropping screen_capture fails honestly without panicking
    let cmd_dropped = format!("env MOCK_DROP=screen_capture {}", mock_studio_command());
    let mut mcp_dropped = McpClient::spawn(&cmd_dropped).expect("spawn dropped mock");
    assert!(!mcp_dropped.has_tool("screen_capture"));
    let err = mcp_dropped
        .screen_capture()
        .expect_err("dropped screen_capture must fail honestly");
    assert!(err.contains("screen_capture"), "{err}");
}
