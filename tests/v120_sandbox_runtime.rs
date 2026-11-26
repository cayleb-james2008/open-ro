//! Integration tests for OpenRO Cycle 2 Wave E2 (Gap #2: Headless Local Roblox Luau Runtime Sandbox):
//! - Real `lune` execution of multi-script Rojo project (`RemoteEvent`, `RemoteFunction`,
//!   `DataStoreService` + `OrderedDataStore` save -> `BindToClose` -> `rejoin`, `ProcessReceipt`
//!   duplicate `PurchaseId` idempotency, `click_gui`, `PlayerGui` DOM snapshot, `assert_luau`)
//! - Default auto-journey when `journey` is omitted
//! - Non-idempotent `ProcessReceipt` double-grant failure detection
//! - Real Luau runtime error & stack-trace capture (including `task.spawn` errors)
//! - Hard timeout termination on infinite loops
//! - Missing `lune` binary structured diagnostics (no panic)
//! - HTTP integration (`POST /api/projects/:id/sandbox/run` + `POST /api/studio/sandbox/run`) + Host/CSRF guard
//! - Agent `sandbox_playtest` tool binding

use openro_rs::agent::{Agent, AskMode, AskOptions};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::sandbox::{
    find_lune_binary, run_local_place_sandbox_with_lune, run_project_sandbox,
    run_project_sandbox_with_lune,
};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v120-sandbox-runtime");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn require_lune() {
    let (bin, searched) = find_lune_binary();
    assert!(
        bin.is_some(),
        "lune binary is required for v120_sandbox_runtime tests (searched: {searched:?})"
    );
}

fn write_file(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn write_sample_rojo_project(dir: &Path) {
    write_file(
        &dir.join("default.project.json"),
        r#"{
  "name": "SandboxTestGame",
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
  "name": "SandboxTestGame",
  "rojoProject": "default.project.json",
  "products": [
    { "key": "coins_pack", "kind": "dev_product", "robloxId": 3736007337 }
  ]
}"#,
    );
    write_file(
        &dir.join("src/shared/ProductIds.luau"),
        r#"return {
	DevProducts = {
		coins_pack = { robloxId = 3736007337, coins = 500 },
	},
}
"#,
    );
    write_file(
        &dir.join("src/shared/EconomyConfig.luau"),
        r#"return {
	RollReward = 50,
}
"#,
    );
    write_file(
        &dir.join("src/server/GameServer.server.luau"),
        r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local DataStoreService = game:GetService("DataStoreService")
local MarketplaceService = game:GetService("MarketplaceService")
local CollectionService = game:GetService("CollectionService")
local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = require(Shared:WaitForChild("EconomyConfig"))
local ProductIds = require(Shared:WaitForChild("ProductIds"))

local Net = Instance.new("Folder")
Net.Name = "Net"
Net.Parent = ReplicatedStorage

local RollRemote = Instance.new("RemoteEvent")
RollRemote.Name = "Roll"
RollRemote.Parent = Net

local GetCoinsRemote = Instance.new("RemoteFunction")
GetCoinsRemote.Name = "GetCoins"
GetCoinsRemote.Parent = Net

local PlayerDataStore = DataStoreService:GetDataStore("PlayerData_v1")
local TopCoinsStore = DataStoreService:GetOrderedDataStore("TopCoins_v1")

local profiles = {}

local function saveProfile(player)
	local prof = profiles[player.UserId]
	if not prof then
		return
	end
	local encoded = HttpService:JSONEncode(prof)
	assert(type(encoded) == "string" and #encoded > 0)
	PlayerDataStore:SetAsync("u_" .. tostring(player.UserId), prof)
	TopCoinsStore:SetAsync("u_" .. tostring(player.UserId), prof.coins)
end

Players.PlayerAdded:Connect(function(player)
	local saved = PlayerDataStore:GetAsync("u_" .. tostring(player.UserId))
	local prof = if type(saved) == "table"
		then { coins = saved.coins or 0, processedReceipts = saved.processedReceipts or {} }
		else { coins = 0, processedReceipts = {} }
	profiles[player.UserId] = prof

	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = player

	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = prof.coins
	coins.Parent = ls

	CollectionService:AddTag(player, "ActivePlayer")
end)

Players.PlayerRemoving:Connect(function(player)
	saveProfile(player)
	profiles[player.UserId] = nil
end)

game:BindToClose(function()
	for _, p in ipairs(Players:GetPlayers()) do
		saveProfile(p)
	end
end)

RollRemote.OnServerEvent:Connect(function(player)
	local prof = profiles[player.UserId]
	if not prof then
		return
	end
	prof.coins += Config.RollReward
	local ls = player:FindFirstChild("leaderstats")
	if ls and ls:FindFirstChild("Coins") then
		ls.Coins.Value = prof.coins
	end
	RollRemote:FireClient(player, prof.coins)
end)

GetCoinsRemote.OnServerInvoke = function(player)
	local prof = profiles[player.UserId]
	return if prof then prof.coins else 0
end

MarketplaceService.ProcessReceipt = function(receiptInfo)
	local player = Players:GetPlayerByUserId(receiptInfo.PlayerId)
	if not player then
		return Enum.ProductPurchaseDecision.NotProcessedYet
	end
	local prof = profiles[player.UserId]
	if not prof then
		return Enum.ProductPurchaseDecision.NotProcessedYet
	end
	local key = tostring(receiptInfo.PurchaseId)
	if prof.processedReceipts[key] then
		return Enum.ProductPurchaseDecision.PurchaseGranted
	end
	for _, entry in pairs(ProductIds.DevProducts) do
		if entry.robloxId == receiptInfo.ProductId then
			prof.processedReceipts[key] = true
			prof.coins += entry.coins
			local ls = player:FindFirstChild("leaderstats")
			if ls and ls:FindFirstChild("Coins") then
				ls.Coins.Value = prof.coins
			end
			saveProfile(player)
			return Enum.ProductPurchaseDecision.PurchaseGranted
		end
	end
	return Enum.ProductPurchaseDecision.NotProcessedYet
end

local heartbeatTicks = 0
RunService.Heartbeat:Connect(function()
	heartbeatTicks += 1
end)
"#,
    );
    write_file(
        &dir.join("src/client/HudClient.client.luau"),
        r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local player = Players.LocalPlayer
local playerGui = player:WaitForChild("PlayerGui")

local gui = Instance.new("ScreenGui")
gui.Name = "MainHud"
gui.ResetOnSpawn = false
gui.Parent = playerGui

local root = Instance.new("Frame")
root.Name = "HudRoot"
root.Size = UDim2.fromScale(1, 1)
root.Parent = gui

local btn = Instance.new("TextButton")
btn.Name = "RollButton"
btn.Text = "Roll"
btn.Size = UDim2.fromOffset(180, 48)
btn.Parent = root

local Net = ReplicatedStorage:WaitForChild("Net")
local RollRemote = Net:WaitForChild("Roll")

btn.Activated:Connect(function()
	RollRemote:FireServer()
	TweenService:Create(btn, TweenInfo.new(0.1), { Text = "Rolling..." }):Play()
end)

RollRemote.OnClientEvent:Connect(function(newCoins)
	btn.Text = "Coins:" .. tostring(newCoins)
end)
"#,
    );
}

fn start_server(agent: Option<Agent>, data_dir: PathBuf) -> (u16, std::sync::Arc<Shared>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "Sandbox Place".to_string(), data_dir);
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

#[test]
fn sandbox_full_journey_remotes_datastore_rejoin_and_idempotent_receipt() {
    require_lune();
    let dir = temp_dir("full-journey");
    write_sample_rojo_project(&dir);

    let req = json!({
        "timeout_secs": 15,
        "players": [{ "user_id": 1001, "name": "Alice", "owns_passes": [777] }],
        "journey": [
            { "op": "click_gui", "player_id": 1001, "gui_path": "MainHud.HudRoot.RollButton" },
            { "op": "invoke_server", "remote": "ReplicatedStorage/Net/GetCoins", "player_id": 1001, "args": [] },
            { "op": "fire_server", "remote": "ReplicatedStorage/Net/Roll", "player_id": 1001, "args": [] },
            { "op": "purchase_receipt", "player_id": 1001, "product_id": 3736007337u64, "purchase_id": "rcpt_unique_1", "replay_duplicate": true },
            { "op": "rejoin", "player_id": 1001 },
            {
                "op": "assert_luau",
                "name": "coins_persisted_after_rejoin",
                "luau": "local Players = game:GetService('Players'); local p = Players:GetPlayerByUserId(1001); return p.leaderstats.Coins.Value",
                "expect": { "equals": 600 }
            },
            {
                "op": "assert_luau",
                "name": "ordered_datastore_leaderboard",
                "luau": "local ods = game:GetService('DataStoreService'):GetOrderedDataStore('TopCoins_v1'); local page = ods:GetSortedAsync(false, 10):GetCurrentPage(); return page[1].value",
                "expect": { "equals": 600 }
            }
        ]
    });

    let rep = run_project_sandbox(&dir, &req).expect("sandbox run");
    assert_eq!(rep["ok"], json!(true), "rep={rep}");
    assert_eq!(rep["engine"], json!("lune_roblox_shim"));
    assert_eq!(rep["passed"], json!(true), "rep={rep}");
    assert!(
        rep["console"]["errors"].as_array().unwrap().is_empty(),
        "unexpected console errors: {:?}",
        rep["console"]["errors"]
    );
    assert_eq!(rep["steps"].as_array().unwrap().len(), 7);
    assert_eq!(rep["steps"][1]["result"], json!(50));
    assert_eq!(rep["state_snapshot"]["leaderstats"]["1001"]["Coins"], json!(600));
    assert!(rep["state_snapshot"]["remotes_fired"].as_u64().unwrap() >= 3);
    assert_eq!(rep["state_snapshot"]["receipts_processed"].as_array().unwrap().len(), 2);
    assert_eq!(rep["state_snapshot"]["receipts_processed"][1]["idempotent"], json!(true));

    let gui_tree = rep["state_snapshot"]["player_gui_tree"].as_array().unwrap();
    assert!(!gui_tree.is_empty());
    assert_eq!(gui_tree[0]["children"][0]["name"], json!("MainHud"));
    assert_eq!(rep["core_loop"]["act"]["passed"], json!(true));
    assert_eq!(rep["core_loop"]["hud"]["passed"], json!(true));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandbox_default_auto_journey_discovers_receipt_and_verifies_rejoin() {
    require_lune();
    let dir = temp_dir("auto-journey");
    write_sample_rojo_project(&dir);

    let rep = run_project_sandbox(&dir, &json!({})).expect("auto journey");
    assert_eq!(rep["ok"], json!(true), "rep={rep}");
    assert_eq!(rep["passed"], json!(true), "rep={rep}");
    let assertions = rep["assertions"].as_array().unwrap();
    assert!(assertions.iter().any(|a| a["name"] == "process_receipt_idempotency" && a["passed"] == json!(true)));
    assert!(assertions.iter().any(|a| a["name"] == "rejoin_persistence" && a["passed"] == json!(true)));
    assert_eq!(rep["state_snapshot"]["leaderstats"]["1001"]["Coins"], json!(500));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandbox_catches_non_idempotent_process_receipt_and_runtime_errors() {
    require_lune();
    // 1. Non-idempotent ProcessReceipt
    let dir_dup = temp_dir("non-idempotent");
    write_file(
        &dir_dup.join("default.project.json"),
        r#"{
  "name": "BuggyMonetization",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } }
  }
}"#,
    );
    write_file(
        &dir_dup.join("src/server/BuggyReceipt.server.luau"),
        r#"local Players = game:GetService("Players")
local MarketplaceService = game:GetService("MarketplaceService")

Players.PlayerAdded:Connect(function(p)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = p
	local gems = Instance.new("IntValue")
	gems.Name = "Gems"
	gems.Value = 0
	gems.Parent = ls
end)

MarketplaceService.ProcessReceipt = function(info)
	local p = Players:GetPlayerByUserId(info.PlayerId)
	-- BUG: never checks info.PurchaseId!
	p.leaderstats.Gems.Value += 100
	return Enum.ProductPurchaseDecision.PurchaseGranted
end
"#,
    );

    let dup_rep = run_project_sandbox(
        &dir_dup,
        &json!({
            "journey": [
                { "op": "purchase_receipt", "player_id": 1001, "product_id": 999, "purchase_id": "dup_1", "replay_duplicate": true }
            ]
        }),
    )
    .expect("buggy receipt sandbox");
    assert_eq!(dup_rep["ok"], json!(true));
    assert_eq!(dup_rep["passed"], json!(false), "must fail on double-grant: {dup_rep}");
    assert_eq!(dup_rep["state_snapshot"]["leaderstats"]["1001"]["Gems"], json!(200));
    assert!(dup_rep["steps"][0]["error"]
        .as_str()
        .unwrap_or("")
        .contains("idempotency"));

    // 2. Deliberate nil-index Luau runtime error + stack trace capture
    let dir_err = temp_dir("runtime-error");
    write_file(
        &dir_err.join("default.project.json"),
        r#"{
  "name": "BrokenScriptGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } }
  }
}"#,
    );
    write_file(
        &dir_err.join("src/server/Broken.server.luau"),
        r#"local function triggerNilBug()
	local target = nil
	return target.MissingField
end
task.spawn(function()
	triggerNilBug()
end)
"#,
    );

    let err_rep = run_project_sandbox(&dir_err, &json!({})).expect("runtime error sandbox");
    assert_eq!(err_rep["ok"], json!(true));
    assert_eq!(err_rep["passed"], json!(false), "must fail when Luau throws");
    let errs = err_rep["console"]["errors"].as_array().unwrap();
    assert!(!errs.is_empty(), "expected Luau stack trace in console.errors");
    let joined = errs
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("Broken.server.luau") && joined.contains("nil"),
        "expected Broken.server.luau nil-index stack trace, got: {joined}"
    );

    let _ = std::fs::remove_dir_all(&dir_dup);
    let _ = std::fs::remove_dir_all(&dir_err);
}

#[test]
fn sandbox_enforces_timeout_and_missing_lune_diagnostics() {
    require_lune();
    let dir = temp_dir("timeout-and-missing");
    write_file(
        &dir.join("default.project.json"),
        r#"{
  "name": "TimeoutGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } }
  }
}"#,
    );
    write_file(
        &dir.join("src/server/Hang.server.luau"),
        r#"local x = 0
while true do
	x += 1
end
"#,
    );

    // 1. Missing lune binary returns structured diagnostics without panicking
    let missing = run_project_sandbox_with_lune(&dir, &json!({}), Some(None))
        .expect("missing lune diagnostics");
    assert_eq!(missing["ok"], json!(false));
    assert_eq!(missing["passed"], json!(false));
    assert_eq!(missing["engine"], json!("lune_roblox_shim"));
    assert_eq!(missing["diagnostics"]["lune_found"], json!(false));

    // 2. Infinite loop hits 1s timeout cleanly
    let timed_out = run_project_sandbox(&dir, &json!({ "timeout_secs": 1 })).expect("timeout");
    assert_eq!(timed_out["ok"], json!(false));
    assert_eq!(timed_out["passed"], json!(false));
    assert_eq!(timed_out["timed_out"], json!(true));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandbox_http_routes_and_local_place_and_agent_tool_work() {
    require_lune();
    let ws = temp_dir("http-ws");
    let data_dir = temp_dir("http-data");

    // LocalPlace with server + client GUI + module script
    let mut lp = LocalPlace::in_memory("LocalSandboxPlace".to_string());
    lp.update_script_source(
        "ReplicatedStorage.SharedConfig",
        "return { Bonus = 25 }\n",
    )
    .unwrap();
    lp.update_script_source(
        "ServerScriptService.Main",
        r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local cfg = require(ReplicatedStorage:WaitForChild("SharedConfig"))
Players.PlayerAdded:Connect(function(p)
	local ls = Instance.new("Folder")
	ls.Name = "leaderstats"
	ls.Parent = p
	local score = Instance.new("IntValue")
	score.Name = "Score"
	score.Value = cfg.Bonus
	score.Parent = ls
end)
"#,
    )
    .unwrap();
    lp.update_script_source(
        "StarterGui.Hud",
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "LocalHud"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "ScoreLabel"
lbl.Text = "Ready"
lbl.Parent = sg
"#,
    )
    .unwrap();

    // Direct LocalPlace helper smoke check + missing lune check
    let lp_missing = run_local_place_sandbox_with_lune(
        lp.game_name(),
        lp.scripts(),
        lp.instances(),
        &json!({}),
        Some(None),
    )
    .unwrap();
    assert_eq!(lp_missing["diagnostics"]["lune_found"], json!(false));

    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:9".into(), "m".into(), None),
        lp,
    );
    let (port, shared) = start_server(Some(agent), data_dir.clone());

    // 1. POST /api/studio/sandbox/run executes LocalPlace
    let (code, lp_res) = api_json(port, "POST", "/api/studio/sandbox/run", Some(json!({})));
    assert_eq!(code, 200, "studio sandbox: {lp_res}");
    assert_eq!(lp_res["ok"], json!(true));
    assert_eq!(lp_res["passed"], json!(true));
    assert_eq!(lp_res["engine"], json!("lune_roblox_shim"));
    assert_eq!(lp_res["state_snapshot"]["leaderstats"]["1001"]["Score"], json!(25));
    assert!(
        lp_res["state_snapshot"]["player_gui_tree"][0]["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == json!("LocalHud")),
        "expected LocalHud in player_gui_tree: {}",
        lp_res["state_snapshot"]["player_gui_tree"]
    );

    // 2. Host/CSRF security check on /api/studio/sandbox/run
    let (evil_code, _, _) = http_req(
        port,
        "POST",
        "/api/studio/sandbox/run",
        "evil.example:80",
        Some("{}"),
    );
    assert_eq!(evil_code, 403);

    // 3. Create Rojo project & run POST /api/projects/:id/sandbox/run
    let (code, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "http-sandbox", "parent": ws.to_string_lossy() })),
    );
    assert_eq!(code, 201);
    let pid = created["project"]["id"].as_str().unwrap().to_string();
    let pdir = PathBuf::from(created["project"]["path"].as_str().unwrap());
    write_sample_rojo_project(&pdir);

    let (code, proj_res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/sandbox/run"),
        Some(json!({})),
    );
    assert_eq!(code, 200, "project sandbox: {proj_res}");
    assert_eq!(proj_res["ok"], json!(true));
    assert_eq!(proj_res["passed"], json!(true));
    assert_eq!(proj_res["state_snapshot"]["leaderstats"]["1001"]["Coins"], json!(500));

    // Host/CSRF guard on /api/projects/:id/sandbox/run
    let (evil_proj_code, _, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/sandbox/run"),
        "evil.example:80",
        Some("{}"),
    );
    assert_eq!(evil_proj_code, 403);

    // 4. Verify Agent tool `sandbox_playtest` runs against the Rojo project via mock LLM
    let llm = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let llm_port = llm.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for turn in 0..2 {
            let Ok((mut s, _)) = llm.accept() else { break };
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let content = if turn == 0 {
                r#"{"tool":"sandbox_playtest","args":{"timeout_secs":15}}"#
            } else {
                r#"{"done":true,"summary":"Sandbox verified cleanly."}"#
            };
            let body = json!({
                "choices": [{ "message": { "content": content } }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    let mut proj_agent = Agent::with_local_place(
        ModelClient::new(
            format!("http://127.0.0.1:{llm_port}/v1/chat/completions"),
            "m".into(),
            None,
        ),
        LocalPlace::in_memory("Unused".into()),
    );
    proj_agent.set_project(Some(pdir.clone()), Some(pid));
    let reply = proj_agent
        .run_ask_with_options(
            "c1",
            "Run sandbox_playtest on this project",
            AskOptions {
                mode: AskMode::Ask,
                ..Default::default()
            },
        )
        .expect("agent sandbox_playtest ask");
    assert!(
        reply
            .transcript
            .iter()
            .any(|t| t.contains("tool sandbox_playtest -> passed=true errors=0")),
        "expected sandbox_playtest transcript entry, got: {:?}",
        reply.transcript
    );

    drop(shared);
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&data_dir);
    let _ = BTreeMap::<String, String>::new();
}

#[test]
fn sandbox_teamwork_tool_and_flagship_project_execute() {
    require_lune();
    // 1. Verify Teamwork verifier can invoke `sandbox_playtest`
    let root = temp_dir("teamwork-sandbox");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let pdir = ws.join("tw-game");
    write_sample_rojo_project(&pdir);
    write_file(&pdir.join("docs/DESIGN.md"), "# Design\n");

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
                                "title": "Verify runtime",
                                "owner": "builder",
                                "verifier": "verifier",
                                "kind": "docs",
                                "files": ["docs/DESIGN.md"]
                            }
                        })
                    } else if body_str.contains("\"done\"") || body_str.contains("Sandbox ok") {
                        json!({ "tool": "finish_run", "args": { "summary": "verified" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                } else if body_str.contains("[Role: verifier]") {
                    if !body_str.contains("tool_result (sandbox_playtest)") {
                        json!({ "tool": "sandbox_playtest", "args": { "timeout_secs": 15 } })
                    } else if !body_str.contains("tool_result (verify_task)") {
                        json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "Sandbox ok" } })
                    } else {
                        json!({ "done": true, "summary": "verified" })
                    }
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

    let (port, shared) = start_server(
        Some(Agent::with_local_place(
            ModelClient::new(
                format!("http://127.0.0.1:{llm_port}/v1/chat/completions"),
                "stub-model".into(),
                None,
            ),
            LocalPlace::open(data.clone(), "TW Sandbox".into()),
        )),
        data.clone(),
    );
    let (cfg_code, cfg_res) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({
            "base_url": format!("http://127.0.0.1:{llm_port}/v1"),
            "model_id": "stub-model"
        })),
    );
    assert_eq!(cfg_code, 200, "config failed: {cfg_res}");
    let (code, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let (code, opened) = api_json(
        port,
        "POST",
        "/api/projects/open",
        Some(json!({ "path": pdir.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let pid = opened["project"]["id"].as_str().unwrap().to_string();

    let (code, started) = api_json(
        port,
        "POST",
        "/api/teamwork",
        Some(json!({
            "goal": "Run sandbox_playtest in teamwork",
            "project_id": pid,
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert!(code == 200 || code == 201, "teamwork start ({code}): {started}");
    let run_id = started["run"]["id"].as_str().unwrap().to_string();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut final_state = Value::Null;
    while std::time::Instant::now() < deadline {
        let (_, st) = api_json(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        let status = st["run"]["status"].as_str().unwrap_or("").to_string();
        final_state = st;
        if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    assert_eq!(
        final_state["run"]["status"],
        json!("completed"),
        "teamwork run did not complete: {final_state}"
    );

    // 2. Smoke check against flagship projects/untitled-anime-card-game if present
    let flagship = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("projects/untitled-anime-card-game");
    if flagship.join("default.project.json").is_file() {
        let rep = run_project_sandbox(&flagship, &json!({ "timeout_secs": 15 }))
            .expect("flagship sandbox");
        assert_eq!(rep["ok"], json!(true), "flagship ok=false: {rep}");
        assert_eq!(rep["engine"], json!("lune_roblox_shim"));
        assert!(
            rep["console"]["errors"].as_array().unwrap().is_empty(),
            "flagship runtime errors: {:?}",
            rep["console"]["errors"]
        );
    }

    drop(shared);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sandbox_ui_preset_assertions_pass_on_compliant_project() {
    require_lune();
    let dir = temp_dir("ui-preset-asserts");
    write_sample_rojo_project(&dir);

    let rcpt_rep = run_project_sandbox(
        &dir,
        &json!({
            "players": [{ "user_id": 1001, "name": "TestPlayer", "owns_passes": [] }],
            "journey": [
                {
                    "op": "purchase_receipt",
                    "player_id": 1001,
                    "product_id": 3736007337_u64,
                    "purchase_id": "rcpt_idempotency_1",
                    "replay_duplicate": true
                },
                {
                    "op": "assert_luau",
                    "name": "Single receipt recorded",
                    "luau": "return #receipts_processed == 1",
                    "expect": { "equals": true }
                }
            ]
        }),
    )
    .expect("receipt_idempotency preset");
    assert_eq!(rcpt_rep["ok"], json!(true), "rcpt_rep={rcpt_rep}");
    assert_eq!(rcpt_rep["passed"], json!(true), "rcpt_rep={rcpt_rep}");

    let rejoin_rep = run_project_sandbox(
        &dir,
        &json!({
            "players": [{ "user_id": 1001, "name": "TestPlayer", "owns_passes": [] }],
            "journey": [
                { "op": "rejoin", "player_id": 1001 },
                {
                    "op": "assert_luau",
                    "name": "DataStore state restored",
                    "luau": "return leaderstats ~= nil",
                    "expect": { "truthy": true }
                }
            ]
        }),
    )
    .expect("save_rejoin preset");
    assert_eq!(rejoin_rep["ok"], json!(true), "rejoin_rep={rejoin_rep}");
    assert_eq!(rejoin_rep["passed"], json!(true), "rejoin_rep={rejoin_rep}");

    let _ = std::fs::remove_dir_all(&dir);
}
