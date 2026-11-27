//! Failing-first integration tests for OpenRO M2 PR A (Gaps #75-#78).

use openro_rs::gui_designer::{
    analyze_safe_areas, parse_and_validate_screen_str, preset_screen_doc,
};
use openro_rs::luau_check::{self, Severity};
use openro_rs::playtest::evaluate_economy_sim;
use openro_rs::projects;
use openro_rs::sandbox::{find_lune_binary, run_project_sandbox};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("m2-gap-fixes");
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
        "lune binary required for m2_gap_fixes sandbox tests (searched: {searched:?})"
    );
}

#[test]
fn gap_75_gui_designer_wcag_contrast_check_and_presets() {
    // 1. All 4 built-in presets must pass with 0 warnings including 0 contrast warnings
    for preset in ["rpg", "obby", "tycoon", "gacha"] {
        let doc = preset_screen_doc(preset, None).unwrap();
        let analysis = analyze_safe_areas(&doc);
        let summary = analysis.to_json();
        assert_eq!(
            summary["contrast_warnings"],
            json!(0),
            "preset '{preset}' had contrast warnings: {summary}"
        );
        assert!(
            analysis.passed && analysis.warnings.is_empty(),
            "preset '{preset}' had unexpected warnings: {:?}",
            analysis.warnings
        );
    }

    // 2. Low-contrast TextLabel & TextButton emit low_contrast with ratio in message;
    //    transparent TextLabel inherits nearest opaque ancestor BackgroundColor3 or canvas dark #171719;
    //    hidden or empty-text nodes are skipped.
    let doc = parse_and_validate_screen_str(
        &json!({
            "version": 1,
            "name": "ContrastCheck",
            "ignore_gui_inset": true,
            "root": {
                "name": "Root",
                "class_name": "Frame",
                "props": { "Size": [[1, 0], [1, 0]], "BackgroundTransparency": 1 },
                "children": [
                    {
                        "name": "LowDirectLabel",
                        "class_name": "TextLabel",
                        "props": {
                            "Position": [[0, 24], [0, 80]],
                            "Size": [[0, 180], [0, 48]],
                            "BackgroundColor3": "#1F2937",
                            "BackgroundTransparency": 0.1,
                            "Text": "Dim Label",
                            "TextColor3": "#4B5563"
                        }
                    },
                    {
                        "name": "LightPanel",
                        "class_name": "Frame",
                        "props": {
                            "Position": [[0, 24], [0, 140]],
                            "Size": [[0, 220], [0, 64]],
                            "BackgroundColor3": "#F8FAFC",
                            "BackgroundTransparency": 0.0
                        },
                        "children": [
                            {
                                "name": "InheritedLowBtn",
                                "class_name": "TextButton",
                                "props": {
                                    "Position": [[0, 8], [0, 8]],
                                    "Size": [[0, 140], [0, 48]],
                                    "BackgroundTransparency": 1.0,
                                    "Text": "Pale on Light",
                                    "TextColor3": "#CBD5E1"
                                }
                            }
                        ]
                    },
                    {
                        "name": "CanvasFallbackLabel",
                        "class_name": "TextLabel",
                        "props": {
                            "Position": [[0, 260], [0, 80]],
                            "Size": [[0, 180], [0, 48]],
                            "BackgroundTransparency": 1.0,
                            "Text": "Dark on Dark Canvas",
                            "TextColor3": "#27272A"
                        }
                    },
                    {
                        "name": "HiddenLowLabel",
                        "class_name": "TextLabel",
                        "props": {
                            "Position": [[0, 260], [0, 140]],
                            "Size": [[0, 180], [0, 48]],
                            "Visible": false,
                            "BackgroundColor3": "#111111",
                            "Text": "Hidden",
                            "TextColor3": "#222222"
                        }
                    },
                    {
                        "name": "EmptyTextLabel",
                        "class_name": "TextLabel",
                        "props": {
                            "Position": [[0, 260], [0, 200]],
                            "Size": [[0, 180], [0, 48]],
                            "BackgroundColor3": "#111111",
                            "Text": "",
                            "TextColor3": "#222222"
                        }
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let analysis = analyze_safe_areas(&doc);
    assert!(!analysis.passed);
    let summary = analysis.to_json();
    let contrast_count = summary["contrast_warnings"].as_u64().unwrap_or(0);
    assert!(contrast_count >= 9, "expected >=9 contrast warnings (3 nodes x 3 devices), got {summary}");

    let low_warnings: Vec<_> = analysis
        .warnings
        .iter()
        .filter(|w| w.code == "low_contrast")
        .collect();
    assert!(low_warnings.iter().all(|w| w.severity == "warning"));
    assert!(low_warnings.iter().all(|w| w.message.contains(":1")));
    assert!(low_warnings.iter().any(|w| w.node_name == "LowDirectLabel"));
    assert!(low_warnings.iter().any(|w| w.node_name == "InheritedLowBtn"));
    assert!(low_warnings.iter().any(|w| w.node_name == "CanvasFallbackLabel"));
    assert!(!low_warnings.iter().any(|w| w.node_name == "HiddenLowLabel"));
    assert!(!low_warnings.iter().any(|w| w.node_name == "EmptyTextLabel"));
}

#[test]
fn gap_76_economy_sim_real_luau_configs_and_zero_paid_luck() {
    // 1. Free-to-play card battler Config.luau + non-economy ReleaseConfig.luau
    let mut card_scripts = BTreeMap::new();
    card_scripts.insert(
        "src/server/ReleaseConfig.luau".to_string(),
        "return {\n\tuniverseId = 0,\n\tplaceId = 0,\n\tproductionScope = \"UACG_PROD_v1\",\n\tsupporterPassId = 0,\n}\n".to_string(),
    );
    card_scripts.insert(
        "src/shared/progression/Config.luau".to_string(),
        r#"local Config = {}
Config.ROLL_COOLDOWN_S = 6
Config.EARNED_PACK_COST = 100
Config.REWARD_WIN_XP = 20
Config.REWARD_WIN_CURRENCY = 10
Config.REWARD_BOSS_XP = 100
Config.REWARD_BOSS_CURRENCY = 50
return Config
"#
        .to_string(),
    );
    let card_rep = evaluate_economy_sim(&card_scripts, &json!({}));
    assert_eq!(
        card_rep["constants"]["gamepass_mult"],
        json!(1.0),
        "zero-paid-luck game must default gamepass_mult to 1.0: {card_rep}"
    );
    assert_eq!(
        card_rep["constants"]["min_price"],
        json!(100.0),
        "expected EARNED_PACK_COST = 100 as min_price: {card_rep}"
    );
    let card_earn = card_rep["constants"]["earn_per_min"].as_f64().unwrap_or(0.0);
    assert!(
        card_earn >= 40.0 && card_earn <= 150.0,
        "expected realistic derived card earn_per_min from ROLL_COOLDOWN_S & REWARD_WIN_CURRENCY, got {card_earn}: {card_rep}"
    );
    for issue in card_rep["issues"].as_array().unwrap() {
        assert_ne!(
            issue["file"],
            json!("src/server/ReleaseConfig.luau"),
            "must not blame ReleaseConfig.luau when Config.luau exists: {card_rep}"
        );
    }

    // 2. Tycoon / Currency / Tower constants extraction
    let mut tycoon_scripts = BTreeMap::new();
    tycoon_scripts.insert(
        "src/server/Dropper.server.luau".to_string(),
        "local DROP_INTERVAL = 2\nlocal DROP_VALUE = 10\nlocal UPGRADE_COST = 150\nlocal REBIRTH_COST = 1800\n"
            .to_string(),
    );
    let tycoon_rep = evaluate_economy_sim(&tycoon_scripts, &json!({}));
    assert_eq!(tycoon_rep["constants"]["gamepass_mult"], json!(1.0));
    assert_eq!(tycoon_rep["constants"]["earn_per_min"], json!(300.0));
    assert_eq!(tycoon_rep["constants"]["rebirth_cost"], json!(1800.0));

    let mut currency_scripts = BTreeMap::new();
    currency_scripts.insert(
        "src/server/Currency.server.luau".to_string(),
        "local EARN_AMOUNT = 15\nlocal EARN_SECONDS = 5\nlocal BASE_REWARD = 25\nlocal REBIRTH_FLOOR = 10\n"
            .to_string(),
    );
    let cur_rep = evaluate_economy_sim(&currency_scripts, &json!({}));
    assert_eq!(cur_rep["constants"]["gamepass_mult"], json!(1.0));
    assert_eq!(cur_rep["constants"]["earn_per_min"], json!(180.0));
    assert!(
        cur_rep["constants"]["rebirth_cost"].as_f64().unwrap_or(0.0) > 100.0,
        "REBIRTH_FLOOR=10 must scale to realistic rebirth_cost, not 10 coins: {cur_rep}"
    );
}

#[test]
fn gap_77_honest_headless_sandbox_grading_and_assertions() {
    require_lune();

    // 1. M0 false-pass: empty DataStores with require_saved_data=true + missing remote + init warning
    let dir_bad = temp_dir("sandbox-honest-fail");
    write_file(
        &dir_bad.join("default.project.json"),
        r#"{
  "name": "UnwiredGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } },
    "StarterPlayer": { "$className": "StarterPlayer", "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } } }
  }
}"#,
    );
    write_file(
        &dir_bad.join("src/server/Boot.server.luau"),
        "warn('[UACG] init failed at production: release target is not configured')\n",
    );
    write_file(
        &dir_bad.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "BattleHUD"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "StatusLine"
lbl.Text = "no-transport: UACG_Economy missing"
lbl.Parent = sg
"#,
    );

    let rep_require_ds = run_project_sandbox(&dir_bad, &json!({ "require_saved_data": true }))
        .expect("sandbox run with require_saved_data");
    assert_eq!(rep_require_ds["passed"], json!(false), "empty DS must fail when require_saved_data=true: {rep_require_ds}");
    let assertions = rep_require_ds["assertions"].as_array().unwrap();
    let rp = assertions
        .iter()
        .find(|a| a["name"] == "rejoin_persistence")
        .expect("rejoin_persistence assertion present");
    assert_eq!(rp["passed"], json!(false), "rejoin_persistence must fail when DS empty and require_saved_data=true: {rp}");

    // Journey with assert_remote, assert_hud (forbid_text), assert_datastore, assert_clean_init must fail
    let rep_journey_bad = run_project_sandbox(
        &dir_bad,
        &json!({
            "journey": [
                { "op": "assert_remote", "remote": "UACG_Economy", "kind": "RemoteFunction" },
                { "op": "assert_hud", "screen_gui": "BattleHUD", "forbid_text": "missing" },
                { "op": "assert_datastore" },
                { "op": "assert_clean_init" }
            ]
        }),
    )
    .expect("sandbox journey fail");
    assert_eq!(rep_journey_bad["passed"], json!(false), "expected journey failure: {rep_journey_bad}");
    for idx in 0..4 {
        assert_eq!(
            rep_journey_bad["steps"][idx]["passed"],
            json!(false),
            "step {idx} should fail: {}",
            rep_journey_bad["steps"][idx]
        );
    }

    // 2. Honest passing journey with assert_remote, assert_hud, assert_datastore
    let dir_good = temp_dir("sandbox-honest-pass");
    write_file(
        &dir_good.join("default.project.json"),
        r#"{
  "name": "HonestLoopGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } },
    "StarterPlayer": { "$className": "StarterPlayer", "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } } }
  }
}"#,
    );
    write_file(
        &dir_good.join("src/server/Main.server.luau"),
        r#"local Players = game:GetService("Players")
local RS = game:GetService("ReplicatedStorage")
local DSS = game:GetService("DataStoreService")
local store = DSS:GetDataStore("PlayerData_v1")

local rf = Instance.new("RemoteFunction")
rf.Name = "Game_Economy"
rf.Parent = RS
rf.OnServerInvoke = function(player, cmd)
	if cmd == "Ping" then
		store:SetAsync("u_" .. tostring(player.UserId), { coins = 100 })
		return { ok = true, coins = 100 }
	end
	return { ok = false }
end

game:BindToClose(function() end)
"#,
    );
    write_file(
        &dir_good.join("src/client/Hud.client.luau"),
        r#"local Players = game:GetService("Players")
local pg = Players.LocalPlayer:WaitForChild("PlayerGui")
local sg = Instance.new("ScreenGui")
sg.Name = "MainHUD"
sg.Parent = pg
local lbl = Instance.new("TextLabel")
lbl.Name = "CoinsLabel"
lbl.Text = "Coins: 100"
lbl.Parent = sg
"#,
    );

    let rep_good = run_project_sandbox(
        &dir_good,
        &json!({
            "require_saved_data": true,
            "journey": [
                { "op": "assert_clean_init" },
                { "op": "assert_remote", "remote": "Game_Economy", "kind": "RemoteFunction", "require_handler": true },
                { "op": "invoke_server", "remote": "Game_Economy", "player_id": 1001, "args": ["Ping"] },
                { "op": "rejoin", "player_id": 1001 },
                { "op": "assert_datastore", "store": "PlayerData_v1" },
                { "op": "assert_hud", "screen_gui": "MainHUD", "text_contains": "Coins", "forbid_text": "missing" }
            ]
        }),
    )
    .expect("sandbox good journey");
    assert_eq!(rep_good["passed"], json!(true), "good journey should pass: {rep_good}");

    let _ = std::fs::remove_dir_all(&dir_bad);
    let _ = std::fs::remove_dir_all(&dir_good);
}

#[test]
fn gap_78_project_check_selene_roblox_std_and_dynamic_runtime_remotes() {
    // 1. Project without selene.toml must still use Roblox std when running selene
    let dir = temp_dir("selene-no-toml");
    write_file(
        &dir.join("openro.project.json"),
        &json!({
            "name": "NoSeleneToml",
            "rojoProject": "default.project.json"
        })
        .to_string(),
    );
    write_file(
        &dir.join("default.project.json"),
        r#"{
  "name": "NoSeleneToml",
  "tree": {
    "$className": "DataModel",
    "ReplicatedStorage": { "$className": "ReplicatedStorage", "Shared": { "$path": "src/shared" } },
    "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } },
    "StarterPlayer": { "$className": "StarterPlayer", "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } } }
  }
}"#,
    );
    write_file(
        &dir.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nindent_type = \"Tabs\"\nindent_width = 4\n",
    );
    write_file(
        &dir.join("src/server/Remotes.luau"),
        "local Remotes = {}\nRemotes.REMOTE_NAME = \"UACG_Economy\"\nreturn Remotes\n",
    );
    write_file(
        &dir.join("src/server/Main.server.luau"),
        r#"local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Remotes = require(script.Parent:WaitForChild("Remotes"))

local rf = Instance.new("RemoteFunction")
rf.Name = Remotes.REMOTE_NAME
rf.Parent = ReplicatedStorage

local ev = Instance.new("RemoteEvent")
ev.Name = "UACG_Action"
ev.Parent = ReplicatedStorage

local folder = Instance.new("Folder")
folder.Name = "UACGAudio"
folder.Parent = workspace

local be = Instance.new("BindableEvent")
be.Name = "UACG_Bus"
be.Parent = ReplicatedStorage

local sv = Instance.new("StringValue")
sv.Name = "UACG_BuildTag"
sv.Value = "v1"
sv.Parent = ReplicatedStorage

Players.PlayerAdded:Connect(function(_player)
	task.wait(0.05)
end)

game:BindToClose(function() end)
"#,
    );
    write_file(
        &dir.join("src/shared/Checks.luau"),
        r#"local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Checks = {}
function Checks.verify(): boolean
	local rf = ReplicatedStorage:FindFirstChild("UACG_Economy")
	local ev = ReplicatedStorage:FindFirstChild("UACG_Action")
	local au = workspace:FindFirstChild("UACGAudio")
	local be = ReplicatedStorage:FindFirstChild("UACG_Bus")
	local sv = ReplicatedStorage:FindFirstChild("UACG_BuildTag")
	return rf ~= nil and ev ~= nil and au ~= nil and be ~= nil and sv ~= nil
end
return Checks
"#,
    );
    write_file(
        &dir.join("src/client/Boot.client.luau"),
        r#"local ReplicatedStorage = game:GetService("ReplicatedStorage")
local rf = ReplicatedStorage:FindFirstChild("UACG_Economy")
print(rf)
"#,
    );

    assert!(!dir.join("selene.toml").exists());
    let chk = projects::check_project(&dir).expect("check_project");
    if chk["selene"]["available"] == json!(true) {
        assert_eq!(
            chk["selene"]["passed"],
            json!(true),
            "selene without selene.toml must use roblox std: {chk}"
        );
    }

    let luau_diags = chk["luau"]["diagnostics"].as_array().unwrap();
    let unresolved: Vec<&Value> = luau_diags
        .iter()
        .filter(|d| d["code"] == json!("UNRESOLVED_FIND_FIRST_CHILD"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "runtime-created remotes/folders/values must not emit UNRESOLVED_FIND_FIRST_CHILD: {unresolved:?}"
    );

    // 2. Truly non-existent FindFirstChild must still warn
    let mut scripts = BTreeMap::new();
    scripts.insert(
        "ServerScriptService.Main".to_string(),
        "local RS = game:GetService(\"ReplicatedStorage\")\nlocal x = RS:FindFirstChild(\"NeverCreatedAnywhere\")\nprint(x)\n".to_string(),
    );
    let rep = luau_check::analyze_place(&scripts, &BTreeMap::new());
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "UNRESOLVED_FIND_FIRST_CHILD" && d.severity == Severity::Warning && d.message.contains("NeverCreatedAnywhere")),
        "missing FindFirstChild should still warn: {:?}",
        rep.diagnostics
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gap_83_economy_sim_earned_cost_classified_as_price() {
    let mut scripts = BTreeMap::new();
    scripts.insert(
        "src/shared/progression/Config.luau".to_string(),
        r#"local Config = {}
Config.ROLL_COOLDOWN_S = 3.5
Config.EARNED_PACK_COST = 100
Config.REWARD_WIN_XP = 25
Config.REWARD_WIN_CURRENCY = 10
Config.REWARD_BOSS_XP = 100
Config.REWARD_BOSS_CURRENCY = 50
return Config
"#
        .to_string(),
    );
    scripts.insert(
        "src/shared/monetization/OfferConfig.luau".to_string(),
        r#"local OfferConfig = {}
OfferConfig.SUPPORTER_PASS_ID = 0
OfferConfig.STARTER_BUNDLE_ROBUX = 199
return OfferConfig
"#
        .to_string(),
    );

    let rep = evaluate_economy_sim(&scripts, &json!({}));
    assert_eq!(
        rep["constants"]["gamepass_mult"],
        json!(1.0),
        "zero paid multiplier must stay 1.0: {rep}"
    );
    assert_eq!(
        rep["constants"]["min_price"],
        json!(100.0),
        "EARNED_PACK_COST=100 must classify as min_price=100: {rep}"
    );
    assert_eq!(
        rep["constants"]["max_price"],
        json!(800.0),
        "EARNED_PACK_COST=100 in price_hits must expand [100, 250, 500, 800], not synthetic 2200: {rep}"
    );
    assert_eq!(
        rep["constants"]["rebirth_cost"],
        json!(1200.0),
        "rebirth_cost should be 1200.0 rather than 2970.0 synthetic fallback: {rep}"
    );
    let earn = rep["constants"]["earn_per_min"].as_f64().unwrap_or(0.0);
    assert!(
        (earn - 80.0).abs() < 1e-6,
        "earn_per_min must derive from REWARD_WIN_CURRENCY=10 and ROLL_COOLDOWN_S=3.5 (80.0/min), not EARNED_PACK_COST=100: {rep}"
    );
    assert_eq!(
        rep["balanced"],
        json!(true),
        "card economy with EARNED_PACK_COST=100 and REWARD_WIN_CURRENCY=10 should be balanced: {rep}"
    );
    assert_eq!(
        rep["blocker_count"],
        json!(0),
        "card economy should have 0 blockers and not false-fail paywall_dead_end: {rep}"
    );
}
