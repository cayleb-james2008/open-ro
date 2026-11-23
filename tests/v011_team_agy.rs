//! v0.11 Teamwork end-to-end tests driven by a fake `OPENRO_AGY_BIN` script
//! and real `mock_studio` MCP + real `git` repository (zero live model calls).

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn scratch_dir(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("team-agy-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn init_git_rojo_project(proj_dir: &Path) {
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": "TestGame",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": { "$className": "ReplicatedStorage", "Shared": { "$path": "src/shared" } },
                "ServerScriptService": { "$className": "ServerScriptService", "$path": "src/server" }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(proj_dir.join(".gitignore"), ".openro/\nbuild/\n").unwrap();
    std::fs::write(proj_dir.join("README.md"), "init\n").unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(proj_dir)
        .args(["init", "-q"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(proj_dir)
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(proj_dir)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(proj_dir)
        .args(["add", "default.project.json", ".gitignore", "README.md"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(proj_dir)
        .args(["commit", "-q", "-m", "chore: init test project"])
        .status()
        .unwrap()
        .success());
}

fn write_projects_index(data_dir: &Path, proj_dir: &Path) {
    let slug = proj_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase();
    std::fs::write(
        data_dir.join("projects.json"),
        json!({
            "roots": [],
            "opened": [proj_dir.to_string_lossy()],
            "active": slug,
        })
        .to_string(),
    )
    .unwrap();
}

fn http(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, raw_body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    (status, serde_json::from_str(raw_body).unwrap_or(Value::Null))
}

/// Fake `agy` script using state files in `$FAKE_AGY_STATE` to script realistic
/// multi-agent Teamwork flows including quota backoff, verifier-gate refusal,
/// playtest failure -> repair -> playtest pass, and final run completion.
const FAKE_AGY_SCRIPT: &str = r#"#!/usr/bin/env python3
import json, os, sys

# Verify stdin is closed/null (read returns immediately)
_ = sys.stdin.read()

args = sys.argv[1:]
prompt = ""
if "-p" in args:
    prompt = args[args.index("-p") + 1]

state_dir = os.environ["FAKE_AGY_STATE"]
os.makedirs(state_dir, exist_ok=True)

# Record argv & cwd atomically across concurrent fake-agy workers
argv_tmp = os.path.join(state_dir, f"last_argv.{os.getpid()}.tmp")
with open(argv_tmp, "w") as f:
    json.dump({"args": args, "cwd": os.getcwd()}, f)
os.replace(argv_tmp, os.path.join(state_dir, "last_argv.json"))

def emit_text(obj):
    txt = json.dumps(obj)
    print(json.dumps({"event": "init", "init": {"model": "gemini-4-hydrogen-high"}}))
    print(json.dumps({"event": "step_update", "step_update": {"text_delta": txt}}))
    print(json.dumps({"event": "result", "result": {"status": "SUCCESS", "response": txt}}))
    sys.exit(0)

def emit_quota(msg):
    print(json.dumps({"event": "result", "result": {"status": "ERROR", "error": msg}}))
    sys.exit(0)

def count_file(name):
    p = os.path.join(state_dir, name)
    lock_p = p + ".lock"
    # Simple step counter per agent
    c = 0
    if os.path.exists(p):
        with open(p) as f:
            c = int(f.read().strip() or "0")
    c += 1
    tmp_p = f"{p}.{os.getpid()}.tmp"
    with open(tmp_p, "w") as f:
        f.write(str(c))
    os.replace(tmp_p, p)
    return c

if "[Role: coordinator]" in prompt:
    step = count_file("coordinator.step")
    if step == 1:
        emit_text({
            "tool": "create_task",
            "args": {
                "id": "t1",
                "title": "Build SaveService",
                "description": "Write Save.server.luau with DataStore + BindToClose",
                "owner": "systems",
                "verifier": "verifier",
                "kind": "game_code",
                "files": ["src/server/Save.server.luau"]
            }
        })
    elif step == 2:
        # Attempt conflicting claim on t1's file -> must be refused by Rust
        emit_text({
            "tool": "create_task",
            "args": {
                "id": "t_conflict",
                "title": "Conflicting task",
                "owner": "researcher",
                "kind": "game_code",
                "files": ["src/server/Save.server.luau"]
            }
        })
    elif step == 3:
        if "file claim conflict" not in prompt:
            emit_text({"done": True, "summary": "unexpectedly missed conflict"})
        emit_text({"done": True, "summary": "Created t1 for systems; waiting for verification."})
    else:
        if "[done] Verified task t1" in prompt:
            emit_text({"tool": "finish_run", "args": {"summary": "All tasks verified and committed."}})
        else:
            emit_text({"done": True, "summary": "Waiting for t1."})

elif "[Role: systems]" in prompt:
    # Trigger one transient agy conversation glitch before step counting (#53)
    transient_flag = os.path.join(state_dir, "transient_once.done")
    if not os.path.exists(transient_flag):
        open(transient_flag, "w").write("1")
        emit_quota("failed to start conversation: backend returned an empty conversation id")
    step = count_file("systems.step")
    # Trigger one quota backoff on first counted systems call!
    quota_flag = os.path.join(state_dir, "quota_once.done")
    if not os.path.exists(quota_flag):
        open(quota_flag, "w").write("1")
        emit_quota("429 Individual quota reached for gemini-4-hydrogen-high. Quota resets in 200ms.")
    if step == 2:
        # Intentionally omit BindToClose so mock_studio playtest fails first!
        emit_text({
            "tool": "project_write_file",
            "args": {
                "path": "src/server/Save.server.luau",
                "content": "local DSS = game:GetService('DataStoreService')\nlocal store = DSS:GetDataStore('SaveV1')\nprint('init')\n"
            }
        })
    elif step == 3:
        emit_text({
            "tool": "submit_task",
            "args": {"task_id": "t1", "summary": "Initial SaveService written"}
        })
    elif step == 4:
        emit_text({"done": True, "summary": "Submitted t1 for verification."})
    elif step == 5:
        # Woken by verifier failure! Repair BindToClose.
        emit_text({
            "tool": "project_edit_file",
            "args": {
                "path": "src/server/Save.server.luau",
                "edits": [{
                    "old": "print('init')",
                    "new": "game:BindToClose(function() end)\nprint('init ok')"
                }]
            }
        })
    elif step == 6:
        emit_text({
            "tool": "submit_task",
            "args": {"task_id": "t1", "summary": "Added BindToClose handler"}
        })
    else:
        emit_text({"done": True, "summary": "Resubmitted t1 after BindToClose fix."})

elif "[Role: verifier]" in prompt:
    step = count_file("verifier.step")
    if step == 1:
        # Try verifying BEFORE playtesting -> Verifier Gate must refuse!
        emit_text({
            "tool": "verify_task",
            "args": {"task_id": "t1", "passed": True, "notes": "shortcut"}
        })
    elif step == 2:
        # Confirm refusal occurred, then run Playtest Runner
        emit_text({"tool": "project_playtest", "args": {"task_id": "t1"}})
    elif step == 3:
        # Playtest failed with BindToClose console error -> reject task back to systems!
        emit_text({
            "tool": "verify_task",
            "args": {"task_id": "t1", "passed": False, "notes": "Playtest console error: BindToClose missing"}
        })
    elif step == 4:
        emit_text({"done": True, "summary": "Sent t1 back to systems for BindToClose fix."})
    elif step == 5:
        # Re-submitted after fix -> playtest again
        emit_text({"tool": "project_playtest", "args": {"task_id": "t1"}})
    elif step == 6:
        # Now playtest passed with 0 console errors -> pass verification!
        emit_text({
            "tool": "verify_task",
            "args": {"task_id": "t1", "passed": True, "notes": "Playtest passed with 0 console errors"}
        })
    else:
        emit_text({"done": True, "summary": "Verified t1."})

else:
    emit_text({"done": True, "summary": "Idle role done."})
"#;

#[test]
fn agy_fake_bin_drives_split_claims_quota_verifier_gate_and_autocommit() {
    let dir = scratch_dir("e2e");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("game-proj");
    let agy_state = dir.join("agy-state");
    std::fs::create_dir_all(&data_dir).unwrap();
    init_git_rojo_project(&proj_dir);
    write_projects_index(&data_dir, &proj_dir);

    let agy_bin = dir.join("fake-agy");
    std::fs::write(&agy_bin, FAKE_AGY_SCRIPT).unwrap();
    std::fs::set_permissions(&agy_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    std::env::set_var("OPENRO_AGY_BIN", &agy_bin);
    std::env::set_var("FAKE_AGY_STATE", &agy_state);
    std::env::set_var("OPENRO_TEAMWORK_BACKOFF_CAP_MS", "50");
    std::env::remove_var("OPENRO_MODEL_BASE_URL");
    std::env::remove_var("OPENRO_NO_AGY");

    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let mcp = McpClient::spawn(mock_bin).expect("spawn mock_studio");
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "unused".into(), None);
    let agent = Agent::new(model, Some(mcp));
    let shared = Shared::with_data_dir(Some(agent), "TestGame".into(), data_dir.clone());

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({
            "goal": "Implement SaveService with DataStore and BindToClose",
            "roles": ["systems", "researcher", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["run"]["provider"], "antigravity");

    // Poll until completed (up to 30s)
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (code, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        assert_eq!(code, 200);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        assert_ne!(detail["run"]["status"], "failed", "{detail}");
        std::thread::sleep(Duration::from_millis(80));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");

    // Verify task t1 passed verification and auto-committed with required format
    let tasks = detail["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    assert_eq!(tasks[0]["id"], "t1");
    assert_eq!(tasks[0]["status"], "done");
    let sha = tasks[0]["commit_sha"].as_str().expect("commit_sha set");
    assert!(!sha.is_empty());
    assert_eq!(tasks[0]["verify_report"]["passed"], true);
    assert_eq!(tasks[0]["verify_report"]["console_errors"], 0);
    assert!(detail["claims"].as_array().unwrap().is_empty(), "claims released on done");

    // Verify git commit message format in project repo
    let git_log = Command::new("git")
        .arg("-C")
        .arg(&proj_dir)
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    let commit_msg = String::from_utf8_lossy(&git_log.stdout).trim().to_string();
    assert_eq!(
        commit_msg,
        format!("openro(systems): Build SaveService [team {run_id} task t1]")
    );

    // Verify mentions-only wakeups: unmentioned `researcher` never burned a turn
    let roster = detail["roster"].as_array().unwrap();
    let researcher = roster.iter().find(|a| a["name"] == "researcher").unwrap();
    assert_eq!(researcher["turns"], 0, "unmentioned researcher must stay idle: {researcher}");
    assert_eq!(researcher["state"], "stopped");

    // Verify board.jsonl persisted under <project>/.openro/teamwork/<run_id>/board.jsonl
    let board_path = proj_dir.join(format!(".openro/teamwork/{run_id}/board.jsonl"));
    assert!(board_path.is_file(), "missing {}", board_path.display());
    let board_arr = detail["board"].as_array().unwrap();
    assert!(board_arr.iter().any(|m| m["kind"] == "quota" && m["text"].as_str().unwrap().contains("200ms")));
    assert!(board_arr.iter().any(|m| m["is_done"] == true));

    // Verify monotonic board seq
    let seqs: Vec<u64> = board_arr.iter().filter_map(|m| m["seq"].as_u64()).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");

    // Verify fake agy invocation obeyed pure-text security rules
    let argv_json: Value =
        serde_json::from_str(&std::fs::read_to_string(agy_state.join("last_argv.json")).unwrap())
            .unwrap();
    let args_arr: Vec<&str> = argv_json["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(!args_arr.contains(&"--dangerously-skip-permissions"));
    assert!(args_arr.contains(&"--output-format") && args_arr.contains(&"stream-json"));
    assert!(argv_json["cwd"].as_str().unwrap().contains("agy-sandbox"));

    // Verify verifier trace recorded refusal of premature verify_task
    let (tr_st, tr) = http(
        port,
        "GET",
        &format!("/api/teamwork/{run_id}/agents/verifier/trace"),
        None,
    );
    assert_eq!(tr_st, 200);
    let trace_items = tr["trace"].as_array().unwrap();
    assert!(trace_items.iter().any(|t| {
        t["kind"] == "tool_result"
            && t["data"]["error"]
                .as_str()
                .is_some_and(|e| e.contains("Verifier gate refused"))
    }));

    // Verify systems trace recorded transient agy auto-retry (#53) before succeeding
    let (sys_st, sys_tr) = http(
        port,
        "GET",
        &format!("/api/teamwork/{run_id}/agents/systems/trace"),
        None,
    );
    assert_eq!(sys_st, 200);
    assert!(sys_tr["trace"].as_array().unwrap().iter().any(|t| {
        t["kind"] == "transient_retry"
            && t["data"]["attempt"] == 1
            && t["data"]["error"]
                .as_str()
                .is_some_and(|e| e.contains("empty conversation id"))
    }));

    shared.teamwork.shutdown();
}
