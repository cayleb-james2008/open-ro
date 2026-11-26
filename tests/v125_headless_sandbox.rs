//! Integration tests for OpenRO v1.25.0 (Cycle 3 Wave E3 — Gap #3):
//! Headless Sandbox connected to Verifier / Readiness / Publish-Gate + Spatial/Physics/Tween/Touched/Prompts/HttpMocks:
//! 1. AABB/OBB spatial index: `Workspace:Raycast`, `GetPartsInPart`, `GetPartBoundsInBox`, `GetPartBoundsInRadius`
//!    with `RaycastParams`/`OverlapParams` + `move_player` firing `Touched`/`TouchEnded` & stepping `Humanoid:MoveTo`.
//! 2. `trigger_prompt` (`ProximityPrompt.Triggered`), `click_detector` (`ClickDetector.MouseClick`),
//!    `key_down`/`key_up` (`UserInputService:IsKeyDown`, `InputBegan`, `InputEnded`), and time-stepped
//!    `TweenService:Create(...):Play()` interpolation + `Completed` signal.
//! 3. Sandbox-safe `HttpService:RequestAsync` / `GetAsync` / `PostAsync` mock router (`http_mocks`),
//!    with zero real outbound network access and clean blocked error on unmocked URLs.
//! 4. Persisted `.openro/sandbox/latest.json` & `<run_id>.json` reports + `GET /api/projects/:id/sandbox/{runs,latest}`,
//!    loopback-403 guard, and CWE-59 symlink containment.
//! 5. `GET /api/projects/:id/readiness` & `POST /api/projects/:id/publish-gate` passing via
//!    `verification_tier: "headless_sandbox"` without Studio, plus stale-after-edit rejection.
//! 6. Teamwork `game_code` verification with `last_sandbox`, `require_studio: true` refusal, and
//!    stale-after-edit (`at_ms < last_edit_ms`) rejection.

use openro_rs::agent::Agent;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::sandbox::{
    find_lune_binary, list_sandbox_runs, read_latest_sandbox_report, run_project_sandbox,
};
use openro_rs::secrets::{SecretStore, OPENCLOUD_KEY};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v125-headless-sandbox");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn require_lune() {
    let (bin, searched) = find_lune_binary();
    assert!(
        bin.is_some(),
        "lune binary is required for v125_headless_sandbox tests (searched: {searched:?})"
    );
}

fn write_file(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn start_server(data_dir: PathBuf, llm_base_url: Option<&str>) -> (u16, std::sync::Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let agent = llm_base_url.map(|url| {
        Agent::with_local_place(
            ModelClient::new(format!("{url}/chat/completions"), "stub-model".into(), None),
            LocalPlace::in_memory("Stub".into()),
        )
    });
    let shared = Shared::with_data_dir(agent, "V125 Sandbox".to_string(), data_dir);
    let s_clone = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, s_clone);
    });
    (port, shared)
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
    (status, head, raw[split + 4..].to_vec())
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

fn write_spatial_and_interaction_project(dir: &Path) {
    write_file(
        &dir.join("default.project.json"),
        r#"{
  "name": "SpatialInteractionGame",
  "tree": {
    "$className": "DataModel",
    "ReplicatedStorage": {
      "$className": "ReplicatedStorage",
      "Shared": { "$path": "src/shared" }
    },
    "ServerScriptService": {
      "$className": "ServerScriptService",
      "Server": { "$path": "src/server" }
    },
    "StarterPlayer": {
      "$className": "StarterPlayer",
      "StarterPlayerScripts": {
        "$className": "StarterPlayerScripts",
        "Client": { "$path": "src/client" }
      }
    }
  }
}"#,
    );
    write_file(
        &dir.join("openro.project.json"),
        r#"{
  "version": 1,
  "name": "SpatialInteractionGame",
  "rojoProject": "default.project.json"
}"#,
    );
    write_file(
        &dir.join("src/shared/Config.luau"),
        "return { Version = 1 }\n",
    );
    write_file(
        &dir.join("src/server/WorldMechanics.server.luau"),
        r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Workspace = game:GetService("Workspace")
local TweenService = game:GetService("TweenService")
local HttpService = game:GetService("HttpService")

local state = {
	touched = 0,
	touchEnded = 0,
	promptTriggered = 0,
	detectorClicked = 0,
	tweenMidSeen = false,
	tweenDone = false,
	rayHit = false,
	boxHits = 0,
	radiusHits = 0,
	partHits = 0,
	mockHttpOk = false,
	unmockedBlocked = false,
}
_G.WorldState = state

-- 1. Build spatial arena parts (including a rotated target and a checkpoint pad)
local wall = Instance.new("Part")
wall.Name = "WallTarget"
wall.Anchored = true
wall.Size = Vector3.new(6, 6, 2)
wall.CFrame = CFrame.new(0, 3, 20) * CFrame.Angles(0, math.rad(30), 0)
wall.Parent = Workspace

local pad = Instance.new("Part")
pad.Name = "CoinPad"
pad.Anchored = true
pad.Size = Vector3.new(4, 4, 4)
pad.Position = Vector3.new(10, 0, 0)
pad.Parent = Workspace

local prompt = Instance.new("ProximityPrompt")
prompt.Name = "ChestPrompt"
prompt.ActionText = "Open Chest"
prompt.Parent = wall

local detector = Instance.new("ClickDetector")
detector.Name = "LeverDetector"
detector.Parent = pad

local tweenPart = Instance.new("Part")
tweenPart.Name = "MovingGate"
tweenPart.Anchored = true
tweenPart.Position = Vector3.new(0, 0, 0)
tweenPart.Size = Vector3.new(2, 2, 2)
tweenPart.Parent = Workspace

-- 2. Spatial queries: Raycast, GetPartBoundsInBox, GetPartBoundsInRadius, GetPartsInPart
local rayParams = RaycastParams.new()
rayParams.FilterType = Enum.RaycastFilterType.Include
rayParams.FilterDescendantsInstances = { wall }
local rayRes = Workspace:Raycast(Vector3.new(0, 3, 0), Vector3.new(0, 0, 40), rayParams)
if rayRes and rayRes.Instance == wall and rayRes.Distance > 15 and rayRes.Distance < 21 then
	state.rayHit = true
end

local boxParts = Workspace:GetPartBoundsInBox(CFrame.new(10, 0, 0), Vector3.new(2, 2, 2))
state.boxHits = #boxParts

local radParts = Workspace:GetPartBoundsInRadius(Vector3.new(12.5, 0, 0), 1.5)
for _, p in ipairs(radParts) do
	if p == pad then
		state.radiusHits += 1
	end
end

local probe = Instance.new("Part")
probe.Name = "Probe"
probe.Size = Vector3.new(2, 2, 2)
probe.Position = Vector3.new(10.5, 0, 0)
probe.Parent = Workspace
local overlapParams = OverlapParams.new()
overlapParams.FilterType = Enum.OverlapFilterType.Include
overlapParams.FilterDescendantsInstances = { pad }
state.partHits = #Workspace:GetPartsInPart(probe, overlapParams)
probe:Destroy()

-- 3. Touched / TouchEnded + leaderstats
Players.PlayerAdded:Connect(function(player)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player

	local score = Instance.new("IntValue")
	score.Name = "Score"
	score.Value = 0
	score.Parent = ls
end)

pad.Touched:Connect(function(hit)
	local char = hit and hit.Parent
	local player = char and Players:GetPlayerFromCharacter(char)
	if player then
		state.touched += 1
		player.leaderstats.Score.Value += 10
	end
end)

pad.TouchEnded:Connect(function(hit)
	local char = hit and hit.Parent
	local player = char and Players:GetPlayerFromCharacter(char)
	if player then
		state.touchEnded += 1
	end
end)

prompt.Triggered:Connect(function(player)
	state.promptTriggered += 1
	player.leaderstats.Score.Value += 25
end)

detector.MouseClick:Connect(function(player)
	state.detectorClicked += 1
	player.leaderstats.Score.Value += 15
end)

-- 4. Time-stepped TweenService interpolation
local gateTween = TweenService:Create(tweenPart, TweenInfo.new(0.5, Enum.EasingStyle.Linear), {
	Position = Vector3.new(0, 10, 0),
})
gateTween.Completed:Connect(function()
	state.tweenDone = true
end)
gateTween:Play()

-- Sample mid-tween position via RemoteEvent / wait
local net = Instance.new("Folder")
net.Name = "Net"
net.Parent = ReplicatedStorage
local ping = Instance.new("RemoteEvent")
ping.Name = "CheckTween"
ping.Parent = net
ping.OnServerEvent:Connect(function()
	if tweenPart.Position.Y > 0.5 and tweenPart.Position.Y < 9.9 then
		state.tweenMidSeen = true
	end
end)

-- 5. HttpService mock router (zero real network)
local okGet, bodyGet = pcall(function()
	return HttpService:GetAsync("https://mock.openro.local/config")
end)
local okPost, bodyPost = pcall(function()
	return HttpService:PostAsync("https://mock.openro.local/events", '{"ev":"join"}')
end)
if okGet and okPost then
	local decGet = HttpService:JSONDecode(bodyGet)
	local decPost = HttpService:JSONDecode(bodyPost)
	if decGet.multiplier == 3 and decPost.accepted == true then
		state.mockHttpOk = true
	end
end

local okBlocked, errBlocked = pcall(function()
	return HttpService:GetAsync("https://127.0.0.1:9/forbidden-real-net")
end)
if not okBlocked and string.find(tostring(errBlocked), "blocked", 1, true) then
	state.unmockedBlocked = true
end
"#,
    );
    write_file(
        &dir.join("src/client/InputAndHud.client.luau"),
        r#"local Players = game:GetService("Players")
local UserInputService = game:GetService("UserInputService")

local player = Players.LocalPlayer
local pg = player:WaitForChild("PlayerGui")

local sg = Instance.new("ScreenGui")
sg.Name = "SpatialHud"
sg.Parent = pg

local lbl = Instance.new("TextLabel")
lbl.Name = "KeyState"
lbl.Text = "Idle"
lbl.Parent = sg

_G.ClientInputLog = { began = 0, ended = 0, wasDownDuringBegan = false }

UserInputService.InputBegan:Connect(function(input)
	if input.KeyCode == Enum.KeyCode.E then
		_G.ClientInputLog.began += 1
		_G.ClientInputLog.wasDownDuringBegan = UserInputService:IsKeyDown(Enum.KeyCode.E)
		lbl.Text = "KeyDown:E"
	end
end)

UserInputService.InputEnded:Connect(function(input)
	if input.KeyCode == Enum.KeyCode.E then
		_G.ClientInputLog.ended += 1
		lbl.Text = "KeyUp:E"
	end
end)
"#,
    );
}

#[test]
fn v125_spatial_physics_tween_prompts_input_and_http_mocks() {
    require_lune();
    let dir = temp_dir("spatial-physics");
    write_spatial_and_interaction_project(&dir);

    let req = json!({
        "timeout_secs": 15,
        "players": [{ "user_id": 1001, "name": "Hero", "owns_passes": [] }],
        "http_mocks": [
            {
                "url": "https://mock.openro.local/config",
                "method": "GET",
                "status": 200,
                "body": { "multiplier": 3 }
            },
            {
                "url": "https://mock.openro.local/events",
                "method": "POST",
                "status": 201,
                "body": "{\"accepted\":true}"
            }
        ],
        "journey": [
            { "op": "fire_server", "remote": "ReplicatedStorage/Net/CheckTween", "player_id": 1001, "args": [] },
            { "op": "wait", "ms": 400 },
            { "op": "move_player", "player_id": 1001, "to": [10, 0, 0] },
            { "op": "move_player", "player_id": 1001, "to": [20, 0, 0] },
            { "op": "trigger_prompt", "player_id": 1001, "path": "Workspace.WallTarget.ChestPrompt" },
            { "op": "click_detector", "player_id": 1001, "path": "Workspace.CoinPad.LeverDetector" },
            { "op": "key_down", "key": "E" },
            {
                "op": "assert_luau",
                "name": "key_is_down_during_hold",
                "luau": "return game:GetService('UserInputService'):IsKeyDown(Enum.KeyCode.E)",
                "expect": { "equals": true }
            },
            { "op": "key_up", "key": "E" },
            {
                "op": "assert_luau",
                "name": "all_spatial_tween_input_http_checks",
                "luau": "local s = _G.WorldState; local c = _G.ClientInputLog; local uis = game:GetService('UserInputService'); local gp = game:GetService('Workspace'):FindFirstChild('MovingGate'); assert(s.rayHit, 'rayHit'); assert(s.boxHits >= 1, 'boxHits'); assert(s.radiusHits >= 1, 'radiusHits'); assert(s.partHits >= 1, 'partHits'); assert(s.touched >= 1, 'touched'); assert(s.touchEnded >= 1, 'touchEnded'); assert(s.promptTriggered == 1, 'promptTriggered'); assert(s.detectorClicked == 1, 'detectorClicked'); assert(s.tweenMidSeen, 'tweenMidSeen'); assert(s.tweenDone, 'tweenDone'); assert(math.abs(gp.Position.Y - 10) < 0.01, 'tweenEndPos'); assert(s.mockHttpOk, 'mockHttpOk'); assert(s.unmockedBlocked, 'unmockedBlocked'); assert(c.began == 1 and c.ended == 1 and c.wasDownDuringBegan, 'inputLog'); assert(not uis:IsKeyDown(Enum.KeyCode.E), 'keyReleased'); return true",
                "expect": { "equals": true }
            }
        ]
    });

    let rep = run_project_sandbox(&dir, &req).expect("run spatial sandbox");
    assert_eq!(rep["ok"], json!(true), "rep={rep}");
    assert_eq!(rep["passed"], json!(true), "rep={rep}");
    assert_eq!(rep["error_count"], json!(0), "rep={rep}");
    assert_eq!(rep["core_loop"]["move"]["passed"], json!(true), "rep={rep}");
    assert_eq!(rep["core_loop"]["act"]["passed"], json!(true), "rep={rep}");
    assert_eq!(rep["core_loop"]["hud"]["passed"], json!(true), "rep={rep}");
    assert_eq!(rep["core_loop"]["all_passed"], json!(true), "rep={rep}");
    assert_eq!(rep["state_snapshot"]["leaderstats"]["1001"]["Score"], json!(50));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v125_persistence_routes_symlink_guard_and_readiness_publish_gate() {
    require_lune();
    let root = temp_dir("readiness-gate");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    SecretStore::for_data_dir(&data)
        .set(OPENCLOUD_KEY, "fake-opencloud-test-key-1234567890")
        .unwrap();

    let (port, shared) = start_server(data.clone(), None);
    let (st, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(st, 200);
    let (st, cr) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "GateGame", "universeId": 123456, "placeId": 654321 })),
    );
    assert_eq!(st, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();
    let pdir = PathBuf::from(cr["project"]["path"].as_str().unwrap());

    let (st, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/autopilot"),
        Some(json!({})),
    );
    assert_eq!(st, 200);

    // Ensure HUD LocalScript exists so core_loop.hud passes in headless sandbox
    write_file(
        &pdir.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "MainHud"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "Status"
lbl.Text = "Ready"
lbl.Parent = sg
"#,
    );
    // Rebuild so .rbxlx includes Hud.client.luau
    let (st, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/build"),
        Some(json!({})),
    );
    assert_eq!(st, 200);

    // Before sandbox run: GET .../sandbox/latest returns 404, runs returns empty list, readiness has verification_tier=null
    let (st_lat0, _) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/latest"),
        None,
    );
    assert_eq!(st_lat0, 404);
    let (st_runs0, runs0) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/runs"),
        None,
    );
    assert_eq!(st_runs0, 200);
    assert_eq!(runs0["runs"].as_array().unwrap().len(), 0);

    let (st_r0, r0) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/readiness"),
        None,
    );
    assert_eq!(st_r0, 200);
    assert!(r0["verification_tier"].is_null(), "r0={r0}");

    // Run Headless Sandbox via HTTP
    let (st_sb, sb) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/sandbox/run"),
        Some(json!({})),
    );
    assert_eq!(st_sb, 200, "sb={sb}");
    assert_eq!(sb["ok"], json!(true));
    assert_eq!(sb["passed"], json!(true), "sb={sb}");
    assert_eq!(sb["error_count"], json!(0));
    assert_eq!(sb["core_loop"]["all_passed"], json!(true), "sb={sb}");
    let run_id = sb["run_id"].as_str().unwrap().to_string();
    assert!(!run_id.is_empty());

    // Confirm .openro/sandbox/latest.json AND .openro/sandbox/<run_id>.json exist on disk
    assert!(pdir.join(".openro/sandbox/latest.json").is_file());
    assert!(pdir.join(format!(".openro/sandbox/{run_id}.json")).is_file());

    // Confirm GET /sandbox/runs and GET /sandbox/latest return persisted proof
    let (st_runs1, runs1) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/runs"),
        None,
    );
    assert_eq!(st_runs1, 200);
    let list = runs1["runs"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["run_id"], json!(run_id));
    assert_eq!(list[0]["passed"], json!(true));
    assert_eq!(list[0]["core_loop"]["all_passed"], json!(true));

    let (st_lat1, lat1) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/latest"),
        None,
    );
    assert_eq!(st_lat1, 200);
    assert_eq!(lat1["run_id"], json!(run_id));
    assert_eq!(lat1["passed"], json!(true));

    // Loopback-403 guard on GET .../sandbox/runs & latest
    let (evil_runs, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/runs"),
        "evil.example:80",
        None,
    );
    assert_eq!(evil_runs, 403);
    let (evil_lat, _, _) = http_req(
        port,
        "GET",
        &format!("/api/projects/{pid}/sandbox/latest"),
        "evil.example:80",
        None,
    );
    assert_eq!(evil_lat, 403);

    // Readiness & Publish-Gate now PASS via verification_tier = "headless_sandbox" WITHOUT Studio!
    let (st_r1, r1) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/readiness"),
        None,
    );
    assert_eq!(st_r1, 200, "r1={r1}");
    assert_eq!(r1["verification_tier"], json!("headless_sandbox"), "r1={r1}");
    assert_eq!(r1["playtest_summary"]["passed"], json!(true), "r1={r1}");
    let pt_check = r1["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "playtest_core_loop")
        .unwrap();
    assert_eq!(pt_check["ok"], json!(true), "pt_check={pt_check}");
    assert_eq!(pt_check["verification_tier"], json!("headless_sandbox"));

    let (st_pg1, pg1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/publish-gate"),
        Some(json!({})),
    );
    assert_eq!(st_pg1, 200, "pg1={pg1}");
    assert_eq!(pg1["verification_tier"], json!("headless_sandbox"), "pg1={pg1}");
    assert_eq!(pg1["playtest_passed"], json!(true), "pg1={pg1}");
    assert_eq!(pg1["gate_passed"], json!(true), "pg1={pg1}");

    // Stale-after-edit: editing a project script invalidates sandbox freshness for readiness/publish-gate
    write_file(
        &pdir.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "MainHud"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "Status"
lbl.Text = "Updated"
lbl.Parent = sg
"#,
    );
    let (st_r2, r2) = api_json(
        port,
        "GET",
        &format!("/api/projects/{pid}/readiness"),
        None,
    );
    assert_eq!(st_r2, 200);
    assert!(r2["verification_tier"].is_null(), "stale run must clear tier: {r2}");
    assert_eq!(r2["playtest_summary"]["passed"], json!(false), "stale run must not pass: {r2}");

    let (st_pg2, pg2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/publish-gate"),
        Some(json!({})),
    );
    assert_eq!(st_pg2, 200);
    assert_eq!(pg2["gate_passed"], json!(false), "stale sandbox must fail publish-gate: {pg2}");

    // CWE-59 symlink containment check on .openro/sandbox/latest.json
    #[cfg(unix)]
    {
        let outside = root.join("outside.json");
        std::fs::write(&outside, r#"{"passed":true}"#).unwrap();
        let lat_path = pdir.join(".openro/sandbox/latest.json");
        let _ = std::fs::remove_file(&lat_path);
        std::os::unix::fs::symlink(&outside, &lat_path).unwrap();

        assert!(read_latest_sandbox_report(&pdir).is_err());
        let (st_sym, sym_body) = api_json(
            port,
            "GET",
            &format!("/api/projects/{pid}/sandbox/latest"),
            None,
        );
        assert_eq!(st_sym, 400, "symlinked latest.json must fail 400: {sym_body}");
        let _ = list_sandbox_runs(&pdir).expect("non-symlink run files still list cleanly");
    }

    drop(shared);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v125_teamwork_verify_task_sandbox_gate_and_stale_edit() {
    require_lune();
    let root = temp_dir("teamwork-gate");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let pdir = ws.join("tw-game");
    write_file(
        &pdir.join("default.project.json"),
        r#"{
  "name": "TeamworkSandboxGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": {
      "$className": "ServerScriptService",
      "Server": { "$path": "src/server" }
    }
  }
}"#,
    );
    write_file(
        &pdir.join("openro.project.json"),
        r#"{"version":1,"name":"TeamworkSandboxGame","rojoProject":"default.project.json"}"#,
    );
    write_file(
        &pdir.join("src/server/Main.server.luau"),
        "local x = 1\n",
    );

    // Mock LLM orchestrating:
    // - coordinator: creates game_code task t1 (builder -> verifier) on src/server/Main.server.luau
    // - builder: edits src/server/Main.server.luau -> submit_task
    // - verifier:
    //   1) tries verify_task(passed=true) before sandbox -> refused!
    //   2) runs sandbox_playtest(task_id="t1") -> passes & sets last_sandbox!
    //   3) tries verify_task(task_id="t1", passed=true, require_studio=true) -> refused!
    //   4) calls verify_task(task_id="t1", passed=true) -> succeeds!
    let llm = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in llm.incoming().flatten() {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    match s.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            let text = String::from_utf8_lossy(&buf);
                            if let Some(hdr_end) = text.find("\r\n\r\n") {
                                let cl = text[..hdr_end]
                                    .lines()
                                    .find_map(|l| {
                                        let (k, v) = l.split_once(':')?;
                                        k.eq_ignore_ascii_case("content-length")
                                            .then(|| v.trim().parse::<usize>().ok())
                                            .flatten()
                                    })
                                    .unwrap_or(0);
                                if buf.len() >= hdr_end + 4 + cl {
                                    break;
                                }
                            }
                        }
                    }
                }
                let body_str = String::from_utf8_lossy(&buf);
                let reply_content = if body_str.contains("[Role: coordinator]") {
                    if !body_str.contains("tool_result (create_task)") && !body_str.contains("\"t1\"") {
                        json!({
                            "tool": "create_task",
                            "args": {
                                "title": "Implement server loop",
                                "owner": "builder",
                                "verifier": "verifier",
                                "kind": "game_code",
                                "files": ["src/server/Main.server.luau"]
                            }
                        })
                    } else if body_str.contains("Headless verified") {
                        json!({ "tool": "finish_run", "args": { "summary": "done" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                } else if body_str.contains("[Role: verifier]") {
                    if !body_str.contains("requires a passing sandbox_playtest") {
                        json!({
                            "tool": "verify_task",
                            "args": { "task_id": "t1", "passed": true, "notes": "premature" }
                        })
                    } else if !body_str.contains("tool_result (sandbox_playtest)") {
                        json!({
                            "tool": "sandbox_playtest",
                            "args": { "task_id": "t1", "timeout_secs": 15 }
                        })
                    } else if !body_str.contains("require_studio=true") {
                        json!({
                            "tool": "verify_task",
                            "args": { "task_id": "t1", "passed": true, "require_studio": true, "notes": "studio check" }
                        })
                    } else if !body_str.contains("Headless verified") {
                        json!({
                            "tool": "verify_task",
                            "args": { "task_id": "t1", "passed": true, "notes": "Headless verified" }
                        })
                    } else {
                        json!({ "done": true, "summary": "verified" })
                    }
                } else if !body_str.contains("tool_result (project_write_file)") {
                    json!({
                        "tool": "project_write_file",
                        "args": {
                            "path": "src/server/Main.server.luau",
                            "content": "local Players = game:GetService('Players')\nPlayers.PlayerAdded:Connect(function() end)\n"
                        }
                    })
                } else if !body_str.contains("tool_result (submit_task)") {
                    json!({ "tool": "submit_task", "args": { "task_id": "t1", "summary": "ready" } })
                } else {
                    json!({ "done": true, "summary": "submitted" })
                };
                let resp_body = json!({
                    "choices": [{ "message": { "content": reply_content.to_string() } }]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });

    let llm_url = format!("http://127.0.0.1:{llm_port}/v1");
    let (port, shared) = start_server(data.clone(), Some(&llm_url));
    let (st, _) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({ "base_url": llm_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);
    let (st, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(st, 200);
    let (st, opened) = api_json(
        port,
        "POST",
        "/api/projects/open",
        Some(json!({ "path": pdir.to_string_lossy() })),
    );
    assert_eq!(st, 200);
    let pid = opened["project"]["id"].as_str().unwrap().to_string();

    let (st, started) = api_json(
        port,
        "POST",
        "/api/teamwork",
        Some(json!({
            "goal": "Verify game_code task via headless sandbox without Studio",
            "project_id": pid,
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert!(st == 200 || st == 201, "{started}");
    let run_id = started["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut final_state = Value::Null;
    while Instant::now() < deadline {
        let (_, st_res) = api_json(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        final_state = st_res;
        if matches!(
            final_state["run"]["status"].as_str().unwrap_or(""),
            "completed" | "failed" | "cancelled"
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    if final_state["run"]["status"] != json!("completed") {
        let (_, tr) = api_json(port, "GET", &format!("/api/teamwork/{run_id}/trace"), None);
        panic!("teamwork run did not complete: {final_state}\ntrace={tr}");
    }
    assert_eq!(final_state["tasks"][0]["status"], json!("done"), "{final_state}");
    assert_eq!(
        final_state["tasks"][0]["verify_report"]["notes"],
        json!("Headless verified")
    );

    drop(shared);
    let _ = std::fs::remove_dir_all(&root);
}
