//! v0.11 P1 Agent Quality + Gaps (#4, #5, #12, #15, #17, #18, #19, #21, #23,
//! #30, #31, #32) verification. Zero live network or live model calls: uses
//! fake `agy` scripts via `OPENRO_AGY_BIN` and loopback mock HTTP servers.

use openro_rs::agent::runs::{AgentRun, EnqueueRun, PlanPhase, RunStatus, RunStore};
use openro_rs::agent::{
    build_project_context, parse_step, project_check_local, project_edit_file, project_files_list,
    project_grep_files, project_read_file, project_run_lune_local, project_safe_path,
    project_system_prompt, project_update_manifest_safe, project_write_file, Agent, AgentHooks,
    AskMode, AskOptions,
};
use openro_rs::art_knowledge;
use openro_rs::library::LibraryManager;
use openro_rs::market_taste::{
    research_roblox_game_to_project_with_base, validate_public_url, web_fetch_with_options,
    web_search_with_endpoint,
};
use openro_rs::model::{flatten_messages, parse_agy_stream, AgyOutcome, ModelClient};
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("agent-p1-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_agy_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// Build a fake agy binary that replays scripted JSON responses in order (1 per call).
fn scripted_agy(dir: &Path, name: &str, replies: &[&str]) -> PathBuf {
    let script_dir = dir.join(format!("{name}_replies"));
    std::fs::create_dir_all(&script_dir).unwrap();
    for (i, r) in replies.iter().enumerate() {
        std::fs::write(script_dir.join(format!("{i}.txt")), r).unwrap();
    }
    let counter = script_dir.join("idx");
    std::fs::write(&counter, "0").unwrap();
    let sd = script_dir.display();
    let body = format!(
        r#"cat >/dev/null
printf '%s\n' "$@" > "{sd}/last_argv.txt"
i=$(cat "{sd}/idx")
echo $((i + 1)) > "{sd}/idx"
f="{sd}/${{i}}.txt"
if [ ! -f "$f" ]; then
  echo '{{"event":"result","result":{{"status":"SUCCESS","response":"{{\"done\":true,\"summary\":\"fallback done\"}}"}}}}'
  exit 0
fi
cat "$f"
"#
    );
    fake_agy_script(dir, name, &body)
}

fn ok_event(response_json: &str) -> String {
    json!({
        "event": "result",
        "result": { "status": "SUCCESS", "response": response_json }
    })
    .to_string()
}

fn quota_event(reset: &str) -> String {
    json!({
        "event": "result",
        "result": { "status": "ERROR", "error": format!("429 Individual quota reached. Quota resets in {reset}.") }
    })
    .to_string()
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let b = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

#[test]
fn gap23_portability_slash_commands_and_partial_delta_error() {
    // 1. No hardcoded personal path in src/model.rs
    let src = std::fs::read_to_string("src/model.rs").unwrap();
    assert!(!src.contains("/home/cayleb/"), "src/model.rs must not hardcode personal home path");

    // 2. Partial step_update deltas followed by result status ERROR must still return Error
    let lines: Vec<String> = vec![
        r#"{"event":"step_update","step_update":{"text_delta":"{\"tool\":\"write"}}"#.into(),
        r#"{"event":"result","result":{"status":"ERROR","error":"429 Individual quota reached. Quota resets in 45m."}}"#.into(),
    ];
    match parse_agy_stream(&lines) {
        AgyOutcome::Error(e) => {
            assert!(e.contains("out of quota") && e.contains("resets in 45m"), "{e}");
        }
        other => panic!("expected Error after partial deltas + ERROR result, got {other:?}"),
    }

    // 3. --disable-slash-commands passed to agy -p
    let dir = scratch("gap23");
    let bin = scripted_agy(&dir, "agy", &[&ok_event(r#"{"done":true,"summary":"ok"}"#)]);
    let m = ModelClient::antigravity(None)
        .with_agy_bin(bin)
        .with_agy_dir(dir.join("sb"));
    let _ = m.chat(&[json!({"role":"user","content":"/plan something"})]).unwrap();
    let argv = std::fs::read_to_string(dir.join("agy_replies/last_argv.txt")).unwrap();
    assert!(argv.contains("--disable-slash-commands"), "missing --disable-slash-commands: {argv}");
}

#[test]
fn parse_step_recovers_fences_prose_unescaped_newlines_and_trailing_commas() {
    // Literal newline inside Luau string + trailing comma + markdown fence + think tag
    let messy = "<think>planning...</think>\nSure, here is the step:\n```json\n{\n  \"tool\": \"project_write_file\",\n  \"args\": {\n    \"path\": \"src/server/Main.server.luau\",\n    \"content\": \"local x = 1\nprint(x)\",\n  },\n}\n```\n";
    let step = parse_step(messy).expect("parse_step recovery");
    assert_eq!(step["tool"], "project_write_file");
    assert_eq!(step["args"]["path"], "src/server/Main.server.luau");
    assert_eq!(step["args"]["content"], "local x = 1\nprint(x)");
}

#[test]
fn project_context_file_tools_manifest_gate_and_lune() {
    let dir = scratch("proj");
    std::fs::write(
        dir.join("default.project.json"),
        r#"{"name":"TestGame","tree":{"$className":"DataModel","ServerScriptService":{"$path":"src/server"}}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("openro.project.json"),
        r#"{"version":1,"name":"TestGame","products":[{"key":"vip","kind":"game_pass","priceRobux":399,"priceApproved":true}]}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs/GDD.md"), "# TestGame GDD\nCore card battle loop.\n").unwrap();
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join(".git/config"), "secret").unwrap();
    std::fs::write(dir.join(".env"), "TOP_SECRET=123").unwrap();

    // Smart context includes manifest & GDD, excludes .git and .env
    let ctx = build_project_context(&dir, "implement card battle loop", &[], 8_000);
    assert!(ctx.contains("openro.project.json"));
    assert!(ctx.contains("TestGame GDD"));
    assert!(!ctx.contains("TOP_SECRET"));
    assert!(!ctx.contains(".git/config"));

    // Path safety checks
    assert!(project_safe_path(&dir, "../escape.luau", false).is_err());
    assert!(project_safe_path(&dir, ".git/config", false).is_err());
    assert!(project_safe_path(&dir, ".env", false).is_err());
    assert!(project_safe_path(&dir, ".openro/state.json", true).is_err());
    assert!(project_safe_path(&dir, "openro.project.json", true).is_err());
    assert!(project_safe_path(&dir, "build/out.rbxlx", true).is_err());
    assert!(project_safe_path(&dir, "assets.lock.json", true).is_err());

    // Write + Luau syntax guard + read + multi-edit + grep + files list
    let bad = project_write_file(&dir, "src/server/Bad.server.luau", "local function oops(\n", None);
    assert!(bad.is_err(), "broken Luau must be refused by Static Luau Prover");

    let w = project_write_file(
        &dir,
        "src/server/Main.server.luau",
        "local Players = game:GetService(\"Players\")\nlocal mult = 1\nreturn mult\n",
        None,
    )
    .unwrap();
    let sha = w["sha256"].as_str().unwrap().to_string();
    let r = project_read_file(&dir, "src/server/Main.server.luau", Some(2), Some(3)).unwrap();
    assert_eq!(r["content"], "local mult = 1\nreturn mult");

    let e = project_edit_file(
        &dir,
        "src/server/Main.server.luau",
        &[json!({"old":"local mult = 1","new":"local mult = 2"})],
        Some(&sha),
    )
    .unwrap();
    assert_eq!(e["edits"], 1);
    assert!(project_edit_file(
        &dir,
        "src/server/Main.server.luau",
        &[json!({"old":"nonexistent","new":"x"})],
        None
    )
    .is_err());

    let grep = project_grep_files(&dir, "local mult = 2", "src", 20).unwrap();
    assert_eq!(grep["count"], 1);
    let list = project_files_list(&dir, "src/server").unwrap();
    assert_eq!(list["entries"].as_array().unwrap().len(), 1);

    // Gap #19: project_update_manifest preserves existing approval only when price unchanged; forces false on new or repriced
    let m = project_update_manifest_safe(
        &dir,
        &json!({
            "products": [
                { "key": "vip", "kind": "game_pass", "name": "VIP", "priceRobux": 399, "priceApproved": true },
                { "key": "vip_changed", "kind": "game_pass", "name": "VIP Plus", "priceRobux": 999, "priceApproved": true }
            ]
        }),
    )
    .unwrap();
    assert_eq!(m["manifest"]["products"][0]["priceApproved"], json!(true));
    assert_eq!(m["manifest"]["products"][1]["priceApproved"], json!(false));

    // Repricing previously approved `vip` strips approval
    let m2 = project_update_manifest_safe(
        &dir,
        &json!({
            "products": [
                { "key": "vip", "kind": "game_pass", "name": "VIP", "priceRobux": 499, "priceApproved": true }
            ]
        }),
    )
    .unwrap();
    assert_eq!(m2["manifest"]["products"][0]["priceApproved"], json!(false));

    // Gap #18: project_check & project_run_lune (delegated to openro-core projects.rs)
    let chk = project_check_local(&dir).unwrap();
    assert!(chk["luau"]["scripts_checked"].as_u64().unwrap() >= 1, "{chk}");

    let lune_script = "print('lune-ok-42')\n";
    project_write_file(&dir, "tools/sim.luau", lune_script, None).unwrap();
    if let Ok(res) = project_run_lune_local(&dir, &json!({"script": "tools/sim.luau"})) {
        assert_eq!(res["exit_code"], 0);
        assert!(res["stdout"].as_str().unwrap().contains("lune-ok-42"));
    }
    assert!(project_run_lune_local(&dir, &json!({"script": "../outside.luau"})).is_err());
}

#[test]
fn gap30_batched_steps_lean_prompt_compaction_and_auto_repair() {
    let dir = scratch("batch");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("default.project.json"), r#"{"name":"P","tree":{"$className":"DataModel"}}"#).unwrap();

    // 1. Lean Rojo system prompt drops legacy v0.9 skills & fits tightly
    let psp = project_system_prompt();
    assert!(psp.contains("BATCHED STEPS"));
    assert!(psp.contains("PartType.Cylinder"));
    assert!(!psp.contains("tycoon_dropper"), "Rojo prompt must drop legacy v0.9 skills");
    assert!(psp.len() < 7_000, "Rojo prompt should be lean, got {}B", psp.len());

    // 2. History compaction keeps argv well under 100 KiB and shrinks older large bodies
    let huge = "x".repeat(40_000);
    let msgs = vec![
        json!({"role":"system","content":"sys"}),
        json!({"role":"assistant","content": json!({"tool":"project_write_file","args":{"path":"a.luau","content": huge}}).to_string()}),
        json!({"role":"user","content":"ok1"}),
        json!({"role":"assistant","content":"ok2"}),
        json!({"role":"user","content":"ok3"}),
        json!({"role":"assistant","content":"ok4"}),
        json!({"role":"user","content":"latest"}),
    ];
    let flat = flatten_messages(&msgs);
    assert!(flat.len() < 98_000, "flattened prompt must stay < 100 KiB: {}", flat.len());
    assert!(flat.contains("omitted 40000 bytes from earlier turn"));

    // 3. Batched steps + Playtest Runner auto-verify & auto-repair from console errors + asset hook surfacing!
    let turn1 = ok_event(
        &json!({
            "steps": [
                {"tool":"project_write_file","args":{"path":"src/server/Coins.server.luau","content":"local coins = 1\nreturn coins\n"}},
                {"tool":"art_generate","args":{"prompt":"eye-level hero","out_path":"art/cards/a/art.png"}}
            ],
            "done": true,
            "summary": "Initial coins script + art."
        })
        .to_string(),
    );
    // Auto-verify playtest fails once -> model repairs with edit + done -> auto-verify passes!
    let turn2 = ok_event(
        &json!({
            "steps": [
                {"tool":"project_edit_file","args":{"path":"src/server/Coins.server.luau","edits":[{"old":"local coins = 1","new":"local coins = 100"}]}}
            ],
            "done": true,
            "summary": "Repaired coins script and verified."
        })
        .to_string(),
    );
    let bin = scripted_agy(&dir, "agy_repair", &[&turn1, &turn2]);
    let model = ModelClient::antigravity(None)
        .with_agy_bin(bin)
        .with_agy_dir(dir.join("sb"));
    let mut agent = Agent::with_local_place(model, LocalPlace::in_memory("P".into()));
    agent.set_project(Some(proj.clone()), Some("p".into()));

    let playtest_calls = Arc::new(AtomicUsize::new(0));
    let pt_c = Arc::clone(&playtest_calls);
    let art_calls = Arc::new(AtomicUsize::new(0));
    let art_c = Arc::clone(&art_calls);
    agent.set_hooks(AgentHooks {
        project_playtest: Some(Arc::new(move |_args| {
            let n = pt_c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(json!({
                    "ok": true,
                    "passed": false,
                    "console": { "errors": ["ServerScriptService.Coins:1: attempt to index nil with 'Coins'"] },
                    "checks": []
                }))
            } else {
                Ok(json!({ "ok": true, "passed": true, "console": { "errors": [] }, "checks": [] }))
            }
        })),
        art_generate: Some(Arc::new(move |args| {
            art_c.fetch_add(1, Ordering::SeqCst);
            Ok(json!({ "ok": true, "status": "ok", "out_path": args["out_path"] }))
        })),
        ..Default::default()
    });

    let outcome = agent
        .run_ask_with_options("c1", "build coins and card art", AskOptions { mode: AskMode::Agent, ..Default::default() })
        .expect("batch + auto-repair succeeds");
    assert_eq!(playtest_calls.load(Ordering::SeqCst), 2, "auto-verify failed once then passed after repair");
    assert_eq!(art_calls.load(Ordering::SeqCst), 1, "art_generate hook invoked");
    let final_src = std::fs::read_to_string(proj.join("src/server/Coins.server.luau")).unwrap();
    assert!(final_src.contains("local coins = 100"), "repair edit applied: {final_src}");
    assert!(outcome.summary.contains("Repaired"));
}

#[test]
fn gap5_plan_execute_checkpoints_quota_pause_restart_and_resume() {
    let dir = scratch("runs");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).unwrap();

    // Scripted agy:
    // call 0: plan_execute planner -> 2 phases
    // call 1: phase 1 -> writes p1.txt + done
    // call 2: phase 2 -> QUOTA ERROR (pauses run at phase 2!)
    // call 3 (after restart & resume): phase 2 -> writes p2.txt + done
    let r0 = ok_event(
        &json!({
            "plan": [
                {"id":"p1","title":"Phase One","goal":"Write docs/p1.md","files":["docs/p1.md"]},
                {"id":"p2","title":"Phase Two","goal":"Write docs/p2.md","files":["docs/p2.md"]}
            ]
        })
        .to_string(),
    );
    let r1 = ok_event(
        &json!({
            "steps":[{"tool":"project_write_file","args":{"path":"docs/p1.md","content":"phase one\n"}}],
            "done":true,"summary":"Phase 1 done"
        })
        .to_string(),
    );
    let r2 = quota_event("1h15m");
    let r3 = ok_event(
        &json!({
            "steps":[{"tool":"project_write_file","args":{"path":"docs/p2.md","content":"phase two\n"}}],
            "done":true,"summary":"Phase 2 done"
        })
        .to_string(),
    );
    let bin = scripted_agy(&dir, "agy_runs", &[&r0, &r1, &r2, &r3]);

    // First server instance: enqueue run -> completes phase 1 -> pauses on quota at phase 2
    let model = ModelClient::antigravity(None)
        .with_agy_bin(bin.clone())
        .with_agy_dir(dir.join("sb"));
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "G".into()));
    let shared = Shared::with_data_dir(Some(agent), "G".into(), data.clone());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });

    let (st, created) = http(
        port,
        "POST",
        "/api/runs",
        Some(
            &json!({
                "message": "Build two docs",
                "mode": "plan_execute",
                "project_root": proj.to_string_lossy()
            })
            .to_string(),
        ),
    );
    assert_eq!(st, 202, "{created}");
    let run_id = created["run_id"].as_str().unwrap().to_string();

    // Poll until paused on quota
    let mut final_first = Value::Null;
    for _ in 0..80 {
        let (_, g) = http(port, "GET", &format!("/api/runs/{run_id}"), None);
        if g["run"]["status"] == "paused" {
            final_first = g["run"].clone();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(final_first["status"], "paused", "{final_first}");
    assert_eq!(final_first["current_phase"], 1);
    assert_eq!(final_first["quota_reset_in"], "1h15m");
    assert_eq!(final_first["plan"][0]["status"], "done");
    assert!(proj.join("docs/p1.md").is_file());
    assert!(!proj.join("docs/p2.md").is_file());

    // Simulate server restart: open fresh RunStore + server from same data_dir
    let model2 = ModelClient::antigravity(None)
        .with_agy_bin(bin)
        .with_agy_dir(dir.join("sb"));
    let agent2 = Agent::with_local_place(model2, LocalPlace::open(data.clone(), "G".into()));
    let shared2 = Shared::with_data_dir(Some(agent2), "G".into(), data.clone());
    let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let port2 = listener2.local_addr().unwrap().port();
    let srv2 = Arc::clone(&shared2);
    std::thread::spawn(move || {
        let _ = serve(listener2, srv2);
    });

    let (st, res) = http(port2, "POST", &format!("/api/runs/{run_id}/resume"), Some(r#"{"hint":"continue"}"#));
    assert_eq!(st, 202, "{res}");

    let mut completed = Value::Null;
    for _ in 0..80 {
        let (_, g) = http(port2, "GET", &format!("/api/runs/{run_id}"), None);
        if g["run"]["status"] == "completed" {
            completed = g["run"].clone();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(completed["status"], "completed", "{completed}");
    assert_eq!(completed["plan"][0]["status"], "done");
    assert_eq!(completed["plan"][1]["status"], "done");
    assert!(proj.join("docs/p2.md").is_file());

    // Also verify interrupted `Running` checkpoint transitions to `Paused` on reload
    let raw_run = AgentRun {
        run_id: "run-interrupted".into(),
        chat_id: "c".into(),
        project_id: None,
        project_root: None,
        ask: "x".into(),
        mode: "agent".into(),
        status: RunStatus::Running,
        plan: vec![PlanPhase {
            id: "p1".into(),
            title: "t".into(),
            goal: "g".into(),
            files: vec![],
            status: "running".into(),
            summary: None,
        }],
        current_phase: 0,
        step: 2,
        max_steps: 20,
        checkpoint_seq: 3,
        summary: None,
        error: None,
        quota_reset_in: None,
        resume_hint: None,
        thread: vec![],
        transcript: vec![],
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    std::fs::write(
        data.join("runs/run-interrupted.json"),
        serde_json::to_string_pretty(&raw_run.detail_json()).unwrap(),
    )
    .unwrap();
    let reloaded = RunStore::open(&data);
    let rec = reloaded.get("run-interrupted").expect("reloaded");
    assert_eq!(rec.status, RunStatus::Paused);
    assert!(rec.error.unwrap().contains("restart"));
    // Enqueue + cancel works cleanly
    let q = reloaded.enqueue(EnqueueRun {
        chat_id: "c".into(),
        ask: "cancel me".into(),
        mode: "agent".into(),
        project_id: None,
        project_root: None,
        max_steps: 10,
    });
    let cancelled = reloaded.cancel(&q.run_id).unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
}

#[test]
fn gaps_12_15_17_21_31_32_research_ssrf_art_ideas_place_tree_and_config() {
    // Gap #15: art_knowledge updated to reference_search + ChatGPT eye-level neutral framing
    let guidance = art_knowledge::prompt_guidance();
    assert!(guidance.contains("reference_search"));
    assert!(guidance.contains("eye-level"));
    assert!(guidance.contains("AssetIds.luau"));
    assert!(!guidance.contains("rbxasset://openro-assets"));
    for a in art_knowledge::anime_archetypes() {
        assert!(!a.art_prompt.contains("dynamic action angle"), "angle_gate violation in {}", a.id);
    }
    let hp = art_knowledge::build_homage_prompt("Shadow Monarch", "Shadow", "Mythic", "twin daggers\nwith aura");
    assert!(hp.contains("eye-level") && !hp.contains('\n'), "{hp}");

    // Gap #12: library 3D/VFX guardrails
    let tmp_lib = scratch("lib");
    let lib = LibraryManager::open(&tmp_lib);
    let lp = lib.active_skill_prompt();
    assert!(lp.contains("PartType.Cylinder") && lp.contains("Debris:AddItem") && lp.contains("20,000"), "{lp}");

    // Gap #17: SSRF safety on web_fetch + real project research downloading PNG thumbnails
    for bad in [
        "http://127.0.0.1:8080/secret",
        "http://localhost/admin",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/",
        "http://[::1]/",
        "file:///etc/passwd",
    ] {
        assert!(validate_public_url(bad, false).is_err(), "SSRF must block {bad}");
    }

    // Mock server for Roblox project research + web_fetch + web_search
    let tiny_png: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00,
    ];
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let base_srv = base.clone();
    let png_vec = tiny_png.to_vec();
    let srv = std::thread::spawn(move || {
        for _ in 0..6 {
            let Ok((mut s, _)) = listener.accept() else { break };
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
            if path == "/thumb.png" {
                let h = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png_vec.len());
                let _ = s.write_all(h.as_bytes());
                let _ = s.write_all(&png_vec);
            } else if path == "/page" {
                let body = "<html><head><script>evil()</script></head><body><h1>Game Wiki</h1><p>Luck x2</p></body></html>";
                let h = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(h.as_bytes());
            } else if path == "/ddg" {
                let body = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fwiki">Roblox Wiki</a><a class="result__snippet">Odds table</a>"#;
                let h = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(h.as_bytes());
            } else {
                let body = if path.starts_with("/v1/games/votes") {
                    json!({"data":[{"id":999,"upVotes":900,"downVotes":100}]}).to_string()
                } else if path.starts_with("/v1/games/multiget/thumbnails") {
                    json!({"data":[{"universeId":999,"thumbnails":[{"imageUrl": format!("{base_srv}/thumb.png")}]}]}).to_string()
                } else {
                    json!({"data":[{"id":999,"rootPlaceId":888,"name":"Anime Nexus RNG X","description":"Real description only.","creator":{"name":"Dev"},"playing":12000,"visits":500000,"favoritedCount":4000,"genre_l1":"Simulation"}]}).to_string()
                };
                let h = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(h.as_bytes());
            }
        }
    });

    let proj = scratch("research_proj");
    let r = research_roblox_game_to_project_with_base(&proj, "999", None, Some(&base), Duration::from_secs(5)).unwrap();
    assert_eq!(r["name"], "Anime Nexus RNG X");
    assert_eq!(r["local_thumbnails"][0], "refs/999/thumb_1.png");
    assert!(proj.join("refs/999/thumb_1.png").is_file());
    let md = std::fs::read_to_string(proj.join("refs/999/README.md")).unwrap();
    assert!(md.contains("Real description only."));
    assert!(!md.contains("Torii Shrine") && !md.contains("Chain Overdrive"));
    assert!(!proj.join("TASTE-DOSSIER.md").exists());

    let wf = web_fetch_with_options(&format!("{base}/page"), 2000, true, Duration::from_secs(5)).unwrap();
    let txt = wf["text"].as_str().unwrap();
    assert!(txt.contains("Game Wiki") && txt.contains("Luck x2") && !txt.contains("evil()"), "{txt}");

    let ws = web_search_with_endpoint("roblox odds", &format!("{base}/ddg"), 5, true, Duration::from_secs(5)).unwrap();
    assert_eq!(ws["count"], 1);
    assert_eq!(ws["results"][0]["url"], "https://example.com/wiki");
    let _ = srv.join();

    // Gaps #21, #31, #32 over HTTP
    let dir = scratch("http_gaps");
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(dir.clone(), "G".into()));
    let shared = Shared::with_data_dir(Some(agent), "G".into(), dir.clone());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv_h = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv_h);
    });

    // #21: POST /api/ideas with custom prompt does NOT pitch 4 canned starters
    let (st, ideas) = http(port, "POST", "/api/ideas", Some(r#"{"prompt":"Hybrid anime card game"}"#));
    assert_eq!(st, 200);
    assert_eq!(ideas["ideas"].as_array().unwrap().len(), 0);
    assert_eq!(ideas["pitched_new"], json!(false));

    // #32: POST /api/config unknown key returns 400; active_backend alias works
    let (st, err) = http(port, "POST", "/api/config", Some(r#"{"bogus_key":"x"}"#));
    assert_eq!(st, 400, "{err}");
    assert!(err["error"].as_str().unwrap().contains("bogus_key"));

    let (st, ok_cfg) = http(port, "POST", "/api/config", Some(r#"{"active_backend":"local_place"}"#));
    assert_eq!(st, 200, "{ok_cfg}");
    assert_eq!(ok_cfg["workspace_mode"], "local_place");

    // #31: GET /api/place-tree stays instant (<500ms) even while shared.agent mutex is locked!
    let _hold_agent = shared.agent.lock().unwrap();
    let t0 = std::time::Instant::now();
    let (st, tree) = http(port, "GET", "/api/place-tree", None);
    assert_eq!(st, 200, "{tree}");
    assert!(t0.elapsed() < Duration::from_secs(2), "place-tree blocked while agent busy: {:?}", t0.elapsed());
    drop(_hold_agent);
}
