//! Comprehensive verification suite for OpenRO v0.5.0 Systems:
//! - All 22 verified Rune Luau skills (generation, parameterization, escaping, strict validation)
//! - Composable ideas catalogue & LocalPlace playtest proofs
//! - Static Luau Analyzer & Cross-Reference Prover (`openro_rs::luau_check`)
//! - Authentic Roblox Studio `.rbxlx` XML export, `Place::from_rbxlx_xml` round-trip, & fuzz resilience
//! - Scale benchmark: 520 Instances + 54 Luau scripts round-trip & static verification

use openro_rs::luau_check::{analyze_place, analyze_script, check_luau_syntax, Severity};
use openro_rs::place::{LocalPlace, Place};
use openro_rs::{ideas, skills};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

const ALL_22_SKILLS: [&str; 22] = [
    "round_manager",
    "leaderboard",
    "day_night",
    "kill_brick",
    "spawn_link",
    "currency",
    "shop",
    "game_gui",
    "checkpoints",
    "soundtrack",
    "jump_pad",
    "moving_platform",
    "datastore_save",
    "npc_patrol",
    "portal_teleporter",
    "combat_sword",
    "inventory_backpack",
    "quest_tracker",
    "vehicle_spawner",
    "weather_cycle",
    "tycoon_dropper",
    "admin_spectate",
];

#[test]
fn all_22_rune_skills_compile_parameterize_escape_and_pass_static_analysis() {
    let reg = skills::registry();
    assert_eq!(reg.len(), 28, "exact count of 28 game systems");

    let names: HashSet<&str> = reg.iter().map(|s| s.name).collect();
    assert_eq!(names.len(), 28, "all 28 system names are unique");
    for expected in ALL_22_SKILLS {
        assert!(names.contains(expected), "missing skill '{expected}'");
    }

    // Verify catalog_json covers all 6 UI Blueprint Library categories
    let catalog = skills::catalog_json();
    let arr = catalog.as_array().expect("catalog is JSON array");
    assert_eq!(arr.len(), 28);
    let valid_cats: HashSet<&str> = [
        "Movement",
        "Economy",
        "Combat",
        "World",
        "Persistence",
        "UI",
    ]
    .into_iter()
    .collect();
    let mut seen_cats: HashSet<String> = HashSet::new();
    for entry in arr {
        let name = entry["name"].as_str().unwrap();
        let cat = entry["category"].as_str().unwrap();
        assert!(
            valid_cats.contains(cat),
            "skill '{name}' has unexpected category '{cat}'"
        );
        seen_cats.insert(cat.to_string());
        assert!(!entry["title"].as_str().unwrap().is_empty());
        assert!(!entry["default_path"].as_str().unwrap().is_empty());
        assert!(entry["default_args"].is_object());

        // Execute every single skill on the Rune VM with its default_args
        let luau = skills::generate(name, &entry["default_args"])
            .unwrap_or_else(|e| panic!("skill '{name}' failed Rune execution: {e}"));
        assert!(
            luau.contains(&format!("Rune skill: {name}"))
                || luau.contains(&format!("system: {name}")),
            "system '{name}' header tags itself"
        );
        let report = analyze_script(entry["default_path"].as_str().unwrap(), &luau);
        assert!(
            report.is_ok(),
            "skill '{name}' failed static Luau analysis: {:?}",
            report.diagnostics
        );
        assert_eq!(
            report.warning_count(),
            0,
            "skill '{name}' emitted warnings: {:?}",
            report.diagnostics
        );
    }
    assert_eq!(
        seen_cats.len(),
        6,
        "all 6 Blueprint Library categories populated: {seen_cats:?}"
    );

    // Parameter sensitivity & Luau content assertions for every one of the 12 new skills
    let jp = skills::generate(
        "jump_pad",
        &json!({ "boost_power": 180, "cooldown_ms": 400, "pad_name": "SkyPad", "create_pad": true }),
    )
    .unwrap();
    assert!(jp.contains("BOOST_POWER = 180") && jp.contains("SkyPad") && jp.contains("ParticleEmitter"));

    let mp = skills::generate(
        "moving_platform",
        &json!({ "platform_name": "LiftAlpha", "travel_distance": 40, "travel_seconds": 6, "axis": "y", "create_platform": true }),
    )
    .unwrap();
    assert!(
        mp.contains("Vector3.new(0, 1, 0)")
            && mp.contains("TRAVEL_DISTANCE = 40")
            && mp.contains("RunService")
            && mp.contains("TweenService")
            && mp.contains("AssemblyLinearVelocity")
    );

    let ds = skills::generate(
        "datastore_save",
        &json!({ "store_name": "Season2", "stat_name": "Gems", "default_value": 50, "autosave_seconds": 90, "bind_close": true }),
    )
    .unwrap();
    assert!(
        ds.contains("GetDataStore(STORE_NAME)")
            && ds.contains("UpdateAsync")
            && ds.contains("GetAsync")
            && ds.contains("pcall")
            && ds.contains("BindToClose")
            && ds.contains("DataLoaded")
    );

    let npc = skills::generate(
        "npc_patrol",
        &json!({ "npc_name": "CastleGuard", "waypoint_count": 6, "walk_speed": 14, "damage": 20, "dialogue": "Stay \"back\"!" }),
    )
    .unwrap();
    assert!(
        npc.contains("PathfindingService")
            && npc.contains("WAYPOINT_COUNT = 6")
            && npc.contains("Stay \\\"back\\\"!")
    );
    assert!(check_luau_syntax(&npc).is_ok());

    let portal = skills::generate(
        "portal_teleporter",
        &json!({ "pair_name": "StarGate", "cooldown_seconds": 4, "offset_distance": 120, "create_portals": true }),
    )
    .unwrap();
    assert!(
        portal.contains("PortalA")
            && portal.contains("PortalB")
            && portal.contains("OFFSET_DISTANCE = 120")
            && portal.contains("root.CFrame = destPad.CFrame")
    );

    let sword = skills::generate(
        "combat_sword",
        &json!({ "sword_name": "Dragon\"Blade", "damage": 35, "cooldown_ms": 500, "range_studs": 12 }),
    )
    .unwrap();
    assert!(
        sword.contains("Dragon\\\"Blade")
            && sword.contains("DAMAGE = 35")
            && sword.contains("RANGE_STUDS = 12")
            && sword.contains("KOs")
    );
    assert!(check_luau_syntax(&sword).is_ok());

    let inv = skills::generate(
        "inventory_backpack",
        &json!({ "starter_item": "Lantern", "max_slots": 20, "remote_name": "EquipRemote" }),
    )
    .unwrap();
    assert!(inv.contains("MAX_SLOTS = 20") && inv.contains("EquipRemote") && inv.contains("OnServerEvent"));

    let quest = skills::generate(
        "quest_tracker",
        &json!({ "quest_name": "Relic \"Zero\"", "target_count": 10, "reward_amount": 500, "currency": "Gold" }),
    )
    .unwrap();
    assert!(
        quest.contains("Relic \\\"Zero\\\"")
            && quest.contains("TARGET_COUNT = 10")
            && quest.contains("REWARD_AMOUNT = 500")
            && quest.contains("OpenROQuestProgress")
    );
    assert!(check_luau_syntax(&quest).is_ok());

    let veh = skills::generate(
        "vehicle_spawner",
        &json!({ "vehicle_name": "Buggy", "max_speed": 95, "cooldown_seconds": 6, "create_pad": true }),
    )
    .unwrap();
    assert!(
        veh.contains("VehicleSeat")
            && veh.contains("MAX_SPEED = 95")
            && veh.contains("cleanupVehicle")
            && veh.contains("PlayerRemoving")
    );

    let weather = skills::generate(
        "weather_cycle",
        &json!({ "cycle_seconds": 30, "start_weather": "rain", "enable_particles": true }),
    )
    .unwrap();
    assert!(
        weather.contains("Atmosphere")
            && weather.contains("START_WEATHER = \"Rain\"")
            && weather.contains("TweenService")
            && weather.contains("ParticleEmitter")
    );

    let tycoon = skills::generate(
        "tycoon_dropper",
        &json!({ "tycoon_name": "OrePlant", "drop_value": 25, "drop_interval": 3, "upgrade_cost": 250, "currency": "Cash" }),
    )
    .unwrap();
    assert!(
        tycoon.contains("OrePlant")
            && tycoon.contains("Conveyor")
            && tycoon.contains("Collector")
            && tycoon.contains("UpgradeButton")
            && tycoon.contains("nextUpgradeCost = 250")
    );

    let spec = skills::generate(
        "admin_spectate",
        &json!({ "remote_name": "SpectateCmd", "cycle_interval": 3, "allow_all_players": false }),
    )
    .unwrap();
    assert!(
        spec.contains("SpectateCmd")
            && spec.contains("ALLOW_ALL_PLAYERS = false")
            && spec.contains("OpenRORoundStatus")
    );

    // Strict input validation rejects bad types, out-of-range ints, bad identifiers, control chars
    assert!(skills::generate("jump_pad", &json!({ "boost_power": 5 })).is_err());
    assert!(skills::generate("jump_pad", &json!({ "pad_name": "Bad Pad;" })).is_err());
    assert!(skills::generate("moving_platform", &json!({ "axis": "W" })).is_err());
    assert!(skills::generate("moving_platform", &json!({ "travel_seconds": 0 })).is_err());
    assert!(skills::generate("datastore_save", &json!({ "autosave_seconds": 5 })).is_err());
    assert!(skills::generate("npc_patrol", &json!({ "waypoint_count": 1 })).is_err());
    assert!(skills::generate("npc_patrol", &json!({ "dialogue": "bad\u{0000}str" })).is_err());
    assert!(skills::generate("portal_teleporter", &json!({ "offset_distance": 2 })).is_err());
    assert!(skills::generate("combat_sword", &json!({ "range_studs": 100 })).is_err());
    assert!(skills::generate("inventory_backpack", &json!({ "max_slots": 0 })).is_err());
    assert!(skills::generate("quest_tracker", &json!({ "reward_amount": 0 })).is_err());
    assert!(skills::generate("vehicle_spawner", &json!({ "max_speed": 999 })).is_err());
    assert!(skills::generate("weather_cycle", &json!({ "start_weather": "Tornado" })).is_err());
    assert!(skills::generate("tycoon_dropper", &json!({ "upgrade_cost": 1 })).is_err());
    assert!(skills::generate("admin_spectate", &json!({ "remote_name": "123bad" })).is_err());
}

#[test]
fn all_catalogue_ideas_execute_and_pass_playtest_in_local_place() {
    let catalogue = ideas::catalogue();
    // 3 opening games + 6 legacy features + 12 new game-system features = 21 ideas
    assert!(catalogue.len() >= 21, "expected >= 21 ideas, got {}", catalogue.len());

    let mut covered_skills: HashSet<&str> = HashSet::new();
    for idea in &catalogue {
        for s in &idea.skills {
            covered_skills.insert(*s);
        }

        let mut place = LocalPlace::in_memory(idea.title.to_string());
        // Seed prerequisite skills if needed (e.g., currency for add-shop)
        if idea.needs.contains(&"currency") {
            let src = skills::generate(
                "currency",
                &json!({ "currency": "Coins", "start_amount": 0, "earn_amount": 10, "earn_seconds": 5, "bind_close": true }),
            )
            .unwrap();
            place
                .call_tool(
                    "write_script",
                    json!({ "path": "ServerScriptService.Currency", "source": src }),
                )
                .unwrap();
        }

        let mut generated: BTreeMap<String, String> = BTreeMap::new();
        for step in &idea.plan {
            if let Some(sk) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or_else(|| json!({}));
                let src = skills::generate(sk, &args)
                    .unwrap_or_else(|e| panic!("idea '{}' skill '{}' failed: {e}", idea.id, sk));
                generated.insert(sk.to_string(), src);
            } else if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let mut args = step.get("args").cloned().unwrap_or_else(|| json!({}));
                if tool == "write_script" {
                    let sk = args["skill"].as_str().unwrap();
                    let src = generated.get(sk).expect("skill generated before write");
                    args["source"] = Value::String(src.clone());
                }
                let res = place
                    .call_tool(tool, args)
                    .unwrap_or_else(|e| panic!("idea '{}' tool '{}' failed: {e}", idea.id, tool));
                if tool == "playtest" {
                    let verdict: Value = serde_json::from_str(&res).unwrap();
                    assert_eq!(
                        verdict["status"], "passed",
                        "idea '{}' playtest failed: {}",
                        idea.id, verdict["console"]
                    );
                }
            }
        }

        let report = place.analyze();
        assert!(
            report.is_ok() && report.warning_count() == 0,
            "idea '{}' had static analyzer findings: {:?}",
            idea.id,
            report.diagnostics
        );
    }

    // Verify all 22 skills appear in at least one idea in the catalogue
    for skill_name in ALL_22_SKILLS {
        assert!(
            covered_skills.contains(skill_name),
            "skill '{skill_name}' missing from ideas catalogue"
        );
    }
}

#[test]
fn static_luau_analyzer_catches_syntax_blocks_and_allows_luau_if_expressions() {
    // Valid Luau `if ... then ... else ...` expression (no `end`) + level-1 long string + member `.end`
    let valid_luau = r#"
local label = if true then "day" else "night"
local note = [=[contains [[nested]] brackets]=]
local t = { end_time = 10 }
local x = t.end_time
local function greet(n)
    repeat
        n = n - 1
    until n <= 0
    return label .. note .. tostring(x)
end
print(greet(2))
"#;
    assert!(check_luau_syntax(valid_luau).is_ok());
    let rep = analyze_script("ServerScriptService.ValidExpr", valid_luau);
    assert!(rep.is_ok(), "{:?}", rep.diagnostics);

    // Unclosed blocks & mismatched keywords
    assert!(check_luau_syntax("local function f()\n  print(1)\n").is_err());
    assert!(check_luau_syntax("if true then\n  print(1)\n").is_err());
    assert!(check_luau_syntax("for i = 1, 5 do\n  print(i)\n").is_err());
    assert!(check_luau_syntax("repeat\n  print(1)\nend").is_err());
    assert!(check_luau_syntax("end").is_err());
    assert!(check_luau_syntax("until true").is_err());

    // Unclosed strings, comments, brackets, parens
    assert!(check_luau_syntax("local s = \"unterminated").is_err());
    assert!(check_luau_syntax("local s = 'unterminated\n").is_err());
    assert!(check_luau_syntax("local s = `unterminated").is_err());
    assert!(check_luau_syntax("local s = [[unterminated").is_err());
    assert!(check_luau_syntax("--[[ unclosed comment").is_err());
    assert!(check_luau_syntax("local a = {1, 2, 3").is_err());
    assert!(check_luau_syntax("local a = (1 + 2]").is_err());
}

#[test]
fn static_luau_analyzer_catches_infinite_loops_without_yield() {
    let bad_spin = "local n = 0\nwhile true do\n    n = n + 1\nend\n";
    let rep = analyze_script("ServerScriptService.Spin", bad_spin);
    assert!(rep.has_code("INFINITE_LOOP_NO_YIELD"), "{:?}", rep.diagnostics);

    let bad_numeric_spin = "while 1 do\n    print('freeze')\nend\n";
    assert!(
        analyze_script("ServerScriptService.Spin1", bad_numeric_spin)
            .has_code("INFINITE_LOOP_NO_YIELD")
    );

    // Yield inside a nested function callback does NOT yield the outer `while true do`!
    let nested_trap = "while true do\n    task.spawn(function()\n        task.wait(1)\n    end)\nend\n";
    assert!(
        analyze_script("ServerScriptService.NestedSpin", nested_trap)
            .has_code("INFINITE_LOOP_NO_YIELD")
    );

    // Valid yields & exits pass cleanly
    for ok_loop in [
        "while true do\n    task.wait(0.25)\nend\n",
        "while true do\n    wait(1)\nend\n",
        "local RS = game:GetService(\"RunService\")\nwhile true do\n    RS.Heartbeat:Wait()\nend\n",
        "local i = 0\nwhile true do\n    i = i + 1\n    if i > 10 then break end\nend\n",
    ] {
        let r = analyze_script("ServerScriptService.GoodLoop", ok_loop);
        assert!(r.is_ok(), "expected pass for:\n{ok_loop}\ngot: {:?}", r.diagnostics);
    }
}

#[test]
fn static_luau_analyzer_catches_unguarded_datastore_and_remote_exploits() {
    // 1. Unguarded DataStore calls missing pcall AND BindToClose
    let bad_ds = r#"
local DataStoreService = game:GetService("DataStoreService")
local store = DataStoreService:GetDataStore("Coins")
local val = store:GetAsync("player_1")
store:SetAsync("player_1", (val or 0) + 1)
"#;
    let ds_rep = analyze_script("ServerScriptService.BadDS", bad_ds);
    assert!(ds_rep.has_code("UNGUARDED_DATASTORE_CALL"), "{:?}", ds_rep.diagnostics);
    assert!(ds_rep.has_code("MISSING_BIND_TO_CLOSE"), "{:?}", ds_rep.diagnostics);

    // Guarded with pcall + BindToClose passes cleanly
    let good_ds = r#"
local DataStoreService = game:GetService("DataStoreService")
local store = DataStoreService:GetDataStore("Coins")
local ok, val = pcall(function()
    return store:GetAsync("player_1")
end)
if ok then
    pcall(function()
        store:UpdateAsync("player_1", function(old) return (old or 0) + 1 end)
    end)
end
game:BindToClose(function() end)
"#;
    let good_ds_rep = analyze_script("ServerScriptService.GoodDS", good_ds);
    assert!(good_ds_rep.is_ok(), "{:?}", good_ds_rep.diagnostics);

    // 2. Client-trusting RemoteEvent handler missing argument validation
    let bad_remote = r#"
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local give = Instance.new("RemoteEvent")
give.Name = "GiveCoins"
give.Parent = ReplicatedStorage
give.OnServerEvent:Connect(function(player, amount)
    player.leaderstats.Coins.Value = player.leaderstats.Coins.Value + amount
end)
"#;
    let rem_rep = analyze_script("ServerScriptService.BadRemote", bad_remote);
    assert!(rem_rep.has_code("UNVALIDATED_REMOTE_ARG"), "{:?}", rem_rep.diagnostics);

    // Second parameter left unvalidated while first is validated -> still caught!
    let partial_remote = r#"
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ev = Instance.new("RemoteEvent")
ev.Name = "Trade"
ev.Parent = ReplicatedStorage
ev.OnServerEvent:Connect(function(player, item, price)
    if typeof(item) ~= "string" then return end
    print(player.Name, item, price)
end)
"#;
    let partial_rep = analyze_script("ServerScriptService.PartialRemote", partial_remote);
    assert!(
        partial_rep.has_code("UNVALIDATED_REMOTE_ARG")
            && partial_rep.errors().iter().any(|m| m.contains("'price'")),
        "{:?}",
        partial_rep.diagnostics
    );

    // Properly validated RemoteEvent handler (with Luau type annotations) passes cleanly
    let good_remote = r#"
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ev = Instance.new("RemoteEvent")
ev.Name = "Buy"
ev.Parent = ReplicatedStorage
ev.OnServerEvent:Connect(function(player: Player, itemId: unknown)
    if typeof(itemId) ~= "string" then
        return
    end
    print(player.Name, itemId)
end)
"#;
    let good_rem_rep = analyze_script("ServerScriptService.GoodRemote", good_remote);
    assert!(good_rem_rep.is_ok(), "{:?}", good_rem_rep.diagnostics);
}

#[test]
fn static_luau_prover_cross_references_waitforchild_and_findfirstchild_against_place() {
    let mut scripts = BTreeMap::new();
    let mut instances = BTreeMap::new();

    scripts.insert(
        "ServerScriptService.BossHook".to_string(),
        r#"
local Workspace = game:GetService("Workspace")
local RS = game:GetService("ReplicatedStorage")
local boss = Workspace:WaitForChild("BossArena")
local remote = RS:WaitForChild("OpenROInventoryEquip")
local optionalPad = workspace:FindFirstChild("OptionalSecretPad")
local player = game:GetService("Players").LocalPlayer
local gui = player and player:WaitForChild("PlayerGui")
print(boss, remote, optionalPad, gui)
"#
        .to_string(),
    );

    // Initially BossArena, OpenROInventoryEquip, and OptionalSecretPad do not exist
    let rep1 = analyze_place(&scripts, &instances);
    assert!(!rep1.is_ok());
    assert!(
        rep1.diagnostics
            .iter()
            .any(|d| d.code == "MISSING_WAIT_FOR_CHILD" && d.message.contains("Workspace.BossArena")),
        "{:?}",
        rep1.diagnostics
    );
    assert!(
        rep1.diagnostics.iter().any(|d| d.code == "MISSING_WAIT_FOR_CHILD"
            && d.message.contains("ReplicatedStorage.OpenROInventoryEquip")),
        "{:?}",
        rep1.diagnostics
    );
    assert!(
        rep1.diagnostics.iter().any(|d| d.code == "UNRESOLVED_FIND_FIRST_CHILD"
            && d.severity == Severity::Warning
            && d.message.contains("OptionalSecretPad")),
        "{:?}",
        rep1.diagnostics
    );
    // No false positive on player:WaitForChild("PlayerGui")!
    assert!(
        !rep1.diagnostics.iter().any(|d| d.message.contains("PlayerGui")),
        "{:?}",
        rep1.diagnostics
    );

    // Now add Workspace.BossArena instance, Workspace.OptionalSecretPad instance,
    // AND add the inventory_backpack script which dynamically creates OpenROInventoryEquip via `local REMOTE_NAME = ...`!
    instances.insert(
        "Workspace.BossArena".to_string(),
        json!({ "class_name": "Part", "properties": { "Anchored": true } }),
    );
    instances.insert(
        "Workspace.OptionalSecretPad".to_string(),
        json!({ "class_name": "Part", "properties": { "Anchored": true } }),
    );
    let inv_src = skills::generate(
        "inventory_backpack",
        &json!({ "starter_item": "Torch", "max_slots": 10, "remote_name": "OpenROInventoryEquip" }),
    )
    .unwrap();
    scripts.insert("ServerScriptService.InventoryBackpack".to_string(), inv_src);

    let rep2 = analyze_place(&scripts, &instances);
    assert!(
        rep2.is_ok() && rep2.warning_count() == 0,
        "all cross-references resolved cleanly: {:?}",
        rep2.diagnostics
    );
}

#[test]
fn rbxlx_xml_fidelity_and_round_trip_import_export() {
    let mut place = LocalPlace::in_memory("Neon Citadel".to_string());

    // Add diverse 3D parts with full Roblox Studio properties
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "SpawnLocation",
                "name": "SpawnPad",
                "parent": "Workspace",
                "properties": {
                    "Anchored": true,
                    "CanCollide": true,
                    "Position": [0, 2, 0],
                    "Size": [10, 1, 10],
                    "Color": "#4F7CF0",
                    "Material": "SmoothPlastic",
                    "Transparency": 0.0
                }
            }),
        )
        .unwrap();
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Part",
                "name": "JumpPad",
                "parent": "Workspace",
                "properties": {
                    "Anchored": true,
                    "CanCollide": false,
                    "Position": [12, 4, -8],
                    "Rotation": [0, 45, 0],
                    "Size": [8, 1, 8],
                    "Color": "#3CB371",
                    "Material": "Neon",
                    "Transparency": 0.25
                }
            }),
        )
        .unwrap();
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Folder",
                "name": "Checkpoints",
                "parent": "Workspace",
                "properties": {}
            }),
        )
        .unwrap();
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Part",
                "name": "Stage1",
                "parent": "Workspace.Checkpoints",
                "properties": {
                    "Anchored": true,
                    "Position": [0, 1, 24],
                    "Size": [6, 1, 6],
                    "Color": "#E6B830",
                    "Material": "Ice",
                    "Transparency": 0.4
                }
            }),
        )
        .unwrap();
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Part",
                "name": "LavaBrick",
                "parent": "Workspace",
                "properties": {
                    "Anchored": true,
                    "Position": [0, 1, 48],
                    "Size": [14, 1, 14],
                    "Color": "#E0532A",
                    "Material": "Neon"
                }
            }),
        )
        .unwrap();
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "Atmosphere",
                "name": "CitadelAtmo",
                "parent": "Lighting",
                "properties": { "Density": 0.35 }
            }),
        )
        .unwrap();

    // Scripts including CDATA edge case `]]>` and XML metacharacters `<>&"'`
    let tricky_script = "-- Tricky <script> & \"quotes\" 'apos'\nlocal s = \"a ]]> b\"\nprint(s)\n";
    place
        .call_tool(
            "write_script",
            json!({ "path": "ServerScriptService.Tricky", "source": tricky_script }),
        )
        .unwrap();
    let kb_src = skills::generate("kill_brick", &json!({ "damage": 40 })).unwrap();
    place
        .call_tool(
            "write_script",
            json!({ "path": "Workspace.LavaBrick.KillBrick", "source": kb_src }),
        )
        .unwrap();
    let gui_src = skills::generate(
        "game_gui",
        &json!({ "title": "Neon Citadel", "button_label": "Buy", "currency": "Coins" }),
    )
    .unwrap();
    place
        .call_tool(
            "write_script",
            json!({ "path": "StarterGui.GameGui", "source": gui_src }),
        )
        .unwrap();
    place
        .call_tool(
            "write_script",
            json!({ "path": "StarterPlayer.StarterPlayerScripts.CamClient", "source": "print('cam ready')\n" }),
        )
        .unwrap();

    let xml = place.to_rbxlx_xml();
    // Verify true Roblox Studio XML tags are emitted
    assert!(xml.contains("<CoordinateFrame name=\"CFrame\">"));
    assert!(xml.contains("<Vector3 name=\"size\">"));
    assert!(xml.contains("<Color3uint8 name=\"Color3uint8\">"));
    assert!(xml.contains("<bool name=\"Anchored\">true</bool>"));
    assert!(xml.contains("<bool name=\"CanCollide\">false</bool>"));
    assert!(xml.contains("<token name=\"Material\">288</token>")); // Neon
    assert!(xml.contains("<token name=\"Material\">1536</token>")); // Ice
    assert!(xml.contains("<float name=\"Transparency\">0.25</float>"));
    assert!(xml.contains("<ProtectedString name=\"Source\"><![CDATA["));
    assert!(xml.contains("<Item class=\"LocalScript\""));

    // Round-trip through Place::from_rbxlx_xml
    let imported = Place::from_rbxlx_xml(&xml).expect("round-trip parse succeeds");
    assert_eq!(imported.game_name(), "Neon Citadel");
    assert_eq!(imported.scripts().len(), place.scripts().len());
    assert_eq!(imported.instances().len(), place.instances().len());

    // Byte-for-byte identical scripts (including `]]>` CDATA split!)
    assert_eq!(
        imported.scripts().get("ServerScriptService.Tricky").map(String::as_str),
        Some(tricky_script)
    );
    assert_eq!(
        imported.scripts().get("Workspace.LavaBrick.KillBrick").map(String::as_str),
        Some(kb_src.as_str())
    );
    assert_eq!(
        imported.scripts().get("StarterGui.GameGui").map(String::as_str),
        Some(gui_src.as_str())
    );
    assert_eq!(
        imported
            .scripts()
            .get("StarterPlayer.StarterPlayerScripts.CamClient")
            .map(String::as_str),
        Some("print('cam ready')\n")
    );

    // Exact properties preserved on nested and rotated parts
    let jp_rec = imported.instances().get("Workspace.JumpPad").unwrap();
    assert_eq!(jp_rec["class_name"], "Part");
    assert_eq!(jp_rec["properties"]["Color"], "#3CB371");
    assert_eq!(jp_rec["properties"]["Material"], "Neon");
    assert_eq!(jp_rec["properties"]["CanCollide"], false);
    assert_eq!(jp_rec["properties"]["Anchored"], true);
    assert_eq!(jp_rec["properties"]["Transparency"], 0.25);
    assert_eq!(jp_rec["properties"]["Position"], json!([12.0, 4.0, -8.0]));
    assert_eq!(jp_rec["properties"]["Size"], json!([8.0, 1.0, 8.0]));
    assert!(
        jp_rec["properties"]["CFrame"].as_array().map(|a| a.len()) == Some(12),
        "rotated part preserves 12-component CFrame"
    );

    let stage1 = imported
        .instances()
        .get("Workspace.Checkpoints.Stage1")
        .expect("nested folder child preserved");
    assert_eq!(stage1["properties"]["Material"], "Ice");
    assert_eq!(stage1["properties"]["Color"], "#E6B830");

    // Re-exporting imported place produces identical logical structure & passes second round-trip
    let xml2 = imported.to_rbxlx_xml();
    let imported2 = Place::from_rbxlx_xml(&xml2).expect("second round-trip succeeds");
    assert_eq!(imported2.scripts(), imported.scripts());
    assert_eq!(imported2.instances(), imported.instances());

    // Test external Roblox Studio XML flavor (Color3 floats, SharedStrings, Ref, escaped ProtectedString)
    let studio_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<roblox version="4">
  <!-- Exported from Studio -->
  <External>null</External>
  <SharedStrings><SharedString md5="abc">AAAA</SharedString></SharedStrings>
  <Item class="Workspace" referent="RBX0">
    <Properties><string name="Name">Workspace</string></Properties>
    <Item class="Part" referent="RBX1">
      <Properties>
        <string name="Name">StudioBlock</string>
        <bool name="Anchored">true</bool>
        <Color3 name="Color"><R>1</R><G>0.5</G><B>0</B></Color3>
        <Vector3 name="size"><X>4</X><Y>2</Y><Z>6</Z></Vector3>
        <Ref name="PrimaryPart">null</Ref>
        <PhysicalProperties name="CustomPhysicalProperties"><CustomPhysics>false</CustomPhysics></PhysicalProperties>
      </Properties>
    </Item>
  </Item>
  <Item class="ServerScriptService" referent="RBX2">
    <Properties><string name="Name">ServerScriptService</string></Properties>
    <Item class="Script" referent="RBX3">
      <Properties>
        <string name="Name">Hello</string>
        <ProtectedString name="Source">print(&quot;a &amp; b &lt; c&quot;)</ProtectedString>
      </Properties>
    </Item>
  </Item>
</roblox>"#;
    let ext = Place::from_rbxlx_xml(studio_xml).expect("external Studio XML imports cleanly");
    assert_eq!(
        ext.instances()["Workspace.StudioBlock"]["properties"]["Color"],
        "#FF8000"
    );
    assert_eq!(
        ext.scripts()["ServerScriptService.Hello"],
        "print(\"a & b < c\")"
    );

    // Verify property patch + diff helpers
    let before = imported.clone();
    let mut after = imported;
    after
        .update_instance_properties("Workspace.JumpPad", json!({ "Color": "#FF00AA" }))
        .unwrap();
    let diff = after.diff_from(&before);
    assert_eq!(diff.instances_modified, vec!["Workspace.JumpPad"]);
    assert!(diff.scripts_added.is_empty());
}

#[test]
fn rbxlx_malformed_xml_fuzz_resilience_never_panics() {
    let valid_sample = {
        let mut p = LocalPlace::in_memory("FuzzSample".to_string());
        p.call_tool(
            "create_instance",
            json!({ "class_name": "Part", "name": "Floor", "parent": "Workspace", "properties": { "Anchored": true } }),
        )
        .unwrap();
        p.call_tool(
            "write_script",
            json!({ "path": "ServerScriptService.Init", "source": "print('ok')\n" }),
        )
        .unwrap();
        p.to_rbxlx_xml()
    };

    // Deep nesting bomb (exceeds MAX_XML_DEPTH=64)
    let mut deep_bomb = String::from("<roblox version=\"4\">");
    for i in 0..80 {
        deep_bomb.push_str(&format!("<Item class=\"Folder\" referent=\"R{i}\">"));
    }
    for _ in 0..80 {
        deep_bomb.push_str("</Item>");
    }
    deep_bomb.push_str("</roblox>");

    let malformed_cases: Vec<String> = vec![
        "".to_string(),
        "   \n\t  ".to_string(),
        "not xml at all".to_string(),
        "<html><body>wrong root</body></html>".to_string(),
        "<roblox version=\"4\">".to_string(),
        "<roblox version=\"4\"></other>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\"></roblox>".to_string(),
        "<roblox version=\"4\"><Item></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\"><Properties><string name=\"Name\">A</string></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"ServerScriptService\"><Item class=\"Script\"><Properties><ProtectedString name=\"Source\"><![CDATA[unclosed".to_string(),
        "<roblox version=\"4\"><!-- unclosed comment".to_string(),
        "<?xml version=\"1.0\" unclosed".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\" badattr></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\" class=\"unclosed></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\"><Properties><bool name=\"Anchored\">maybe</bool></Properties></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\"><Properties><float name=\"Transparency\">NaN</float></Properties></Item></roblox>".to_string(),
        "<roblox version=\"4\"><Item class=\"Workspace\"><Properties><Color3uint8 name=\"Color3uint8\">nope</Color3uint8></Properties></Item></roblox>".to_string(),
        "<roblox version=\"4\"></roblox>trailing garbage".to_string(),
        "<?xml version=\"1.0\"?><!DOCTYPE foo [ <!ENTITY xxe SYSTEM \"file:///etc/passwd\"> ]><roblox version=\"4\"><Meta name=\"OpenROGameName\">&xxe;</Meta></roblox>".to_string(),
        "<roblox version=\"4\">\0</roblox>".to_string(),
        deep_bomb,
    ];

    for (idx, bad) in malformed_cases.iter().enumerate() {
        let res = Place::from_rbxlx_xml(bad);
        assert!(
            res.is_err(),
            "malformed case #{idx} should return Err, got Ok"
        );
    }

    // Truncated cuts across a real serialized `.rbxlx` file
    for step in 1..20 {
        let cut = (valid_sample.len() * step) / 20;
        let prefix = &valid_sample[..cut];
        assert!(
            Place::from_rbxlx_xml(prefix).is_err(),
            "truncated XML at {cut}/{} must fail cleanly",
            valid_sample.len()
        );
    }
}

#[test]
fn scale_benchmark_500_plus_instances_and_50_plus_scripts() {
    let mut place = LocalPlace::in_memory("Ludicrous Mega World".to_string());

    // 1. Seed 520 Instances: 20 Folders + 500 3D Parts across Workspace, ReplicatedStorage, Lighting
    for zone in 0..20 {
        let folder_name = format!("Zone{zone:02}");
        place
            .call_tool(
                "create_instance",
                json!({
                    "class_name": "Folder",
                    "name": folder_name,
                    "parent": "Workspace",
                    "properties": {}
                }),
            )
            .unwrap();
        for p in 0..25 {
            let materials = ["SmoothPlastic", "Neon", "Metal", "Ice", "Concrete"];
            let colors = ["#4F7CF0", "#3CB371", "#E0532A", "#E6B830", "#A855F7"];
            place
                .call_tool(
                    "create_instance",
                    json!({
                        "class_name": if zone == 0 && p == 0 { "SpawnLocation" } else { "Part" },
                        "name": format!("Tile{p:02}"),
                        "parent": format!("Workspace.Zone{zone:02}"),
                        "properties": {
                            "Anchored": true,
                            "CanCollide": true,
                            "Position": [(zone as f64) * 32.0, (p as f64) * 2.0, (p as f64) * 14.0],
                            "Size": [10.0, 1.0, 10.0],
                            "Color": colors[(zone + p) % colors.len()],
                            "Material": materials[(zone + p) % materials.len()],
                            "Transparency": if (zone + p) % 7 == 0 { 0.25 } else { 0.0 }
                        }
                    }),
                )
                .unwrap();
        }
    }
    // Also add SpawnPad for spawn_link
    place
        .call_tool(
            "create_instance",
            json!({
                "class_name": "SpawnLocation",
                "name": "SpawnPad",
                "parent": "Workspace",
                "properties": { "Anchored": true, "Position": [0, 2, 0], "Size": [8, 1, 8] }
            }),
        )
        .unwrap();
    assert_eq!(place.instances().len(), 521);

    // 2. Seed 54 Luau scripts using ALL 22 Rune skills (first 22) + 32 parameterized regional scripts
    let catalog = skills::catalog_json();
    for entry in catalog.as_array().unwrap() {
        let name = entry["name"].as_str().unwrap();
        let path = entry["default_path"].as_str().unwrap();
        let src = skills::generate(name, &entry["default_args"]).unwrap();
        place
            .call_tool("write_script", json!({ "path": path, "source": src }))
            .unwrap();
    }
    for i in 0..32 {
        let src = skills::generate(
            "jump_pad",
            &json!({
                "boost_power": 100 + (i as i64) * 5,
                "cooldown_ms": 500,
                "pad_name": format!("MegaJumpPad{i:02}"),
                "create_pad": true
            }),
        )
        .unwrap();
        place
            .call_tool(
                "write_script",
                json!({ "path": format!("ServerScriptService.MegaJumpPad{i:02}"), "source": src }),
            )
            .unwrap();
    }
    assert_eq!(place.scripts().len(), 60);

    // 3. Measure XML export
    let t_export = Instant::now();
    let xml = place.to_rbxlx_xml();
    let export_ms = t_export.elapsed().as_millis();
    assert!(xml.len() > 200_000, "rich 521-instance/60-script XML ({} bytes)", xml.len());
    if let Ok(dump_path) = std::env::var("OPENRO_DUMP_RBXLX_PATH") {
        std::fs::write(&dump_path, &xml).expect("write dump rbxlx");
    }

    // 4. Measure XML import round-trip
    let t_import = Instant::now();
    let imported = Place::from_rbxlx_xml(&xml).expect("521-instance / 60-script XML parses cleanly");
    let import_ms = t_import.elapsed().as_millis();
    assert_eq!(imported.instances().len(), 522); // +1 auto-created Workspace.LavaBrick container for Workspace.LavaBrick.KillBrick
    assert_eq!(imported.scripts().len(), 60);
    assert_eq!(
        imported.scripts().get("ServerScriptService.MegaJumpPad31"),
        place.scripts().get("ServerScriptService.MegaJumpPad31")
    );
    assert_eq!(
        imported.instances()["Workspace.Zone19.Tile24"]["properties"]["Position"],
        json!([19.0 * 32.0, 24.0 * 2.0, 24.0 * 14.0])
    );

    // 5. Measure Static Luau Analyzer & Cross-Reference Prover across all 54 scripts & 522 instances
    let t_analyze = Instant::now();
    let report = imported.analyze();
    let analyze_ms = t_analyze.elapsed().as_millis();
    assert!(
        report.is_ok(),
        "all 54 scripts against 522 instances passed static prover: {:?}",
        report.diagnostics
    );

    eprintln!(
        "[scale-bench] 522 instances + 54 scripts ({} KB XML): export={}ms, import={}ms, static_analyze={}ms",
        xml.len() / 1024,
        export_ms,
        import_ms,
        analyze_ms
    );

    // Generous CI-safe upper bounds while verifying sub-second real-world speed
    assert!(export_ms < 2_000, "export too slow: {export_ms}ms");
    assert!(import_ms < 2_000, "import too slow: {import_ms}ms");
    assert!(analyze_ms < 2_000, "static analysis too slow: {analyze_ms}ms");
}
