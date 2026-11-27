//! Failing-first integration tests for OpenRO Gap #82 (`RunService:IsStudio()` in Headless Sandbox).

use openro_rs::sandbox::{find_lune_binary, run_local_place_sandbox, run_project_sandbox};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("sandbox-is-studio");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn require_lune() {
    let (bin, searched) = find_lune_binary();
    assert!(
        bin.is_some(),
        "lune binary required for sandbox_is_studio tests (searched: {searched:?})"
    );
}

fn write_fail_closed_studio_fixture(dir: &Path) {
    write_file(
        &dir.join("default.project.json"),
        r#"{
  "name": "FailClosedStudioFixture",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } },
    "StarterPlayer": { "$className": "StarterPlayer", "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } } }
  }
}"#,
    );

    write_file(
        &dir.join("src/server/Init.server.luau"),
        r#"local RunService = game:GetService("RunService")
local RS = game:GetService("ReplicatedStorage")
local DSS = game:GetService("DataStoreService")

if not RunService:IsStudio() then
	error("release target is not configured")
end

local store = DSS:GetDataStore("PlayerData_v1")
local rf = Instance.new("RemoteFunction")
rf.Name = "Game_State"
rf.Parent = RS
rf.OnServerInvoke = function(player, cmd)
	if cmd == "Save" then
		store:SetAsync("u_" .. tostring(player.UserId), { coins = 250, isStudio = RunService:IsStudio() })
		return { ok = true, isStudio = RunService:IsStudio(), coins = 250 }
	elseif cmd == "Load" then
		local data = store:GetAsync("u_" .. tostring(player.UserId))
		return { ok = true, isStudio = RunService:IsStudio(), data = data }
	end
	return { ok = false }
end

game:BindToClose(function() end)
"#,
    );

    write_file(
        &dir.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")

local sg = Instance.new("ScreenGui")
sg.Name = "StudioCheckHUD"
sg.Parent = pg

local lbl = Instance.new("TextLabel")
lbl.Name = "StudioMode"
lbl.Text = "studio:" .. tostring(RunService:IsStudio())
lbl.Parent = sg
"#,
    );
}

#[test]
fn gap_82_default_is_studio_true_and_explicit_false_override() {
    require_lune();

    let dir = temp_dir("is-studio-fixture");
    write_fail_closed_studio_fixture(&dir);

    // 1. Default run (is_studio omitted) must set RunService:IsStudio() == true on both server
    //    and client proxies, boot studioDev path cleanly, and persist DataStore writes across rejoin.
    let rep_default = run_project_sandbox(
        &dir,
        &json!({
            "require_saved_data": true,
            "journey": [
                { "op": "assert_clean_init" },
                { "op": "assert_remote", "remote": "Game_State", "kind": "RemoteFunction", "require_handler": true },
                { "op": "invoke_server", "remote": "Game_State", "player_id": 1001, "args": ["Save"] },
                { "op": "rejoin", "player_id": 1001 },
                { "op": "assert_datastore", "store": "PlayerData_v1" },
                { "op": "assert_hud", "screen_gui": "StudioCheckHUD", "text_contains": "studio:true", "forbid_text": "studio:false" }
            ]
        }),
    )
    .expect("default sandbox run");

    assert_eq!(
        rep_default["passed"],
        json!(true),
        "default sandbox run (is_studio omitted) must run with IsStudio() == true and persist DataStore: {rep_default}"
    );
    let ds_snap = &rep_default["state_snapshot"]["datastores"]["PlayerData_v1"];
    assert!(
        ds_snap.is_object() && !ds_snap.as_object().unwrap().is_empty(),
        "expected non-empty PlayerData_v1 snapshot after rejoin: {rep_default}"
    );
    let ds_assert = rep_default["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "assert_datastore:PlayerData_v1")
        .expect("assert_datastore:PlayerData_v1 assertion present");
    assert_eq!(
        ds_assert["passed"],
        json!(true),
        "assert_datastore must pass across rejoin: {ds_assert}"
    );
    let hud_assert = rep_default["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "assert_hud:StudioCheckHUD")
        .expect("assert_hud:StudioCheckHUD assertion present");
    assert_eq!(
        hud_assert["passed"],
        json!(true),
        "client runProxy.IsStudio() HUD must report studio:true: {hud_assert}"
    );

    // 2. Explicit `"is_studio": false` must flip RunService:IsStudio() to false on both server
    //    and client proxies so fail-closed production tests still work.
    let rep_prod = run_project_sandbox(
        &dir,
        &json!({
            "is_studio": false,
            "require_saved_data": true,
            "journey": [
                { "op": "assert_hud", "screen_gui": "StudioCheckHUD", "text_contains": "studio:false", "forbid_text": "studio:true" },
                { "op": "assert_remote", "remote": "Game_State", "kind": "RemoteFunction" }
            ]
        }),
    )
    .expect("explicit is_studio=false sandbox run");

    assert_eq!(
        rep_prod["passed"],
        json!(false),
        "is_studio=false must trigger fail-closed production error: {rep_prod}"
    );
    let console_err_text = serde_json::to_string(&rep_prod["console"]).unwrap();
    assert!(
        console_err_text.contains("release target is not configured"),
        "expected fail-closed server error in console: {console_err_text}"
    );
    assert_eq!(
        rep_prod["steps"][0]["passed"],
        json!(true),
        "client runProxy.IsStudio() must report studio:false when is_studio=false: {}",
        rep_prod["steps"][0]
    );
    assert_eq!(
        rep_prod["steps"][1]["passed"],
        json!(false),
        "Game_State remote must not register when server aborts fail-closed: {}",
        rep_prod["steps"][1]
    );

    // 3. Invalid non-boolean is_studio must return a clear error.
    let bad = run_project_sandbox(&dir, &json!({ "is_studio": "yes" }));
    assert!(bad.is_err(), "expected non-boolean is_studio to error");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gap_82_auto_journey_and_local_place_respect_is_studio_default_and_override() {
    require_lune();

    let mut scripts = BTreeMap::new();
    scripts.insert(
        "ServerScriptService.Boot".to_string(),
        r#"local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local DSS = game:GetService("DataStoreService")

if not RunService:IsStudio() then
	error("release target is not configured")
end

local store = DSS:GetDataStore("AutoJourneyData")
Players.PlayerAdded:Connect(function(player)
	store:SetAsync("u_" .. tostring(player.UserId), { studio = true, coins = 42 })
end)
game:BindToClose(function() end)
"#
        .to_string(),
    );
    scripts.insert(
        "StarterPlayer.StarterPlayerScripts.Hud".to_string(),
        r#"local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "AutoHud"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "Mode"
lbl.Text = "is_studio=" .. tostring(RunService:IsStudio())
lbl.Parent = sg
"#
        .to_string(),
    );

    let instances = BTreeMap::new();

    // Auto-journey with default is_studio (omitted -> true) persists DataStore across rejoin
    let rep_auto = run_local_place_sandbox(
        "LocalStudioTest",
        &scripts,
        &instances,
        &json!({ "require_saved_data": true }),
    )
    .expect("local place default sandbox");
    assert_eq!(
        rep_auto["passed"],
        json!(true),
        "auto_journey default is_studio=true must pass rejoin_persistence: {rep_auto}"
    );
    let rp = rep_auto["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "rejoin_persistence")
        .expect("rejoin_persistence assertion present");
    assert_eq!(rp["passed"], json!(true), "rejoin_persistence must pass: {rp}");
    let hud_text = serde_json::to_string(&rep_auto["state_snapshot"]["player_gui_tree"]).unwrap();
    assert!(
        hud_text.contains("is_studio=true"),
        "expected client HUD is_studio=true: {hud_text}"
    );

    // LocalPlace with explicit is_studio=false fails closed
    let rep_false = run_local_place_sandbox(
        "LocalStudioTest",
        &scripts,
        &instances,
        &json!({ "is_studio": false, "require_saved_data": true }),
    )
    .expect("local place is_studio=false sandbox");
    assert_eq!(
        rep_false["passed"],
        json!(false),
        "is_studio=false must fail closed: {rep_false}"
    );
    let hud_false = serde_json::to_string(&rep_false["state_snapshot"]["player_gui_tree"]).unwrap();
    assert!(
        hud_false.contains("is_studio=false"),
        "expected client HUD is_studio=false: {hud_false}"
    );
}
