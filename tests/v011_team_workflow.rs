//! v0.11 Teamwork scheduler, claims, Studio lease, restart resume, control API,
//! and SSE integration tests using per-test local HTTP stub models.

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use openro_rs::teamwork::StudioLeaseLock;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scratch_dir(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("team-workflow-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

struct StubHandler {
    base_url: String,
    peak_seen: Arc<AtomicUsize>,
}

fn start_dynamic_stub<F>(handler: F) -> StubHandler
where
    F: Fn(&str, usize) -> Value + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    let active_now = Arc::new(AtomicUsize::new(0));
    let peak_seen = Arc::new(AtomicUsize::new(0));
    let call_counter = Arc::new(AtomicUsize::new(0));
    let active_c = Arc::clone(&active_now);
    let peak_c = Arc::clone(&peak_seen);

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let h = Arc::clone(&handler);
            let active = Arc::clone(&active_c);
            let peak = Arc::clone(&peak_c);
            let counter = Arc::clone(&call_counter);
            std::thread::spawn(move || {
                let mut s = stream;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut header_end = None;
                let mut content_len = 0usize;
                while let Ok(n) = s.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let head = String::from_utf8_lossy(&buf[..pos]);
                            for line in head.lines() {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.trim().eq_ignore_ascii_case("content-length") {
                                        content_len = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(he) = header_end {
                        if buf.len() >= he + content_len {
                            break;
                        }
                    }
                }
                let body = header_end
                    .map(|he| String::from_utf8_lossy(&buf[he..buf.len().min(he + content_len)]).to_string())
                    .unwrap_or_default();

                let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
                loop {
                    let prev = peak.load(Ordering::SeqCst);
                    if cur <= prev
                        || peak
                            .compare_exchange(prev, cur, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                    {
                        break;
                    }
                }
                let seq = counter.fetch_add(1, Ordering::SeqCst) + 1;
                let reply_val = h(&body, seq);
                active.fetch_sub(1, Ordering::SeqCst);

                let content = reply_val.to_string();
                let payload = json!({
                    "id": format!("chatcmpl-{seq}"),
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
            });
        }
    });

    StubHandler {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        peak_seen,
    }
}

fn boot_server(data_dir: PathBuf, mcp_bin: Option<&str>) -> (u16, Arc<Shared>) {
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "stub".into(), None);
    let agent = if let Some(bin) = mcp_bin {
        Agent::new(model, Some(McpClient::spawn(bin).expect("mock_studio")))
    } else {
        Agent::with_local_place(model, LocalPlace::open(data_dir.clone(), "TeamTest".into()))
    };
    let shared = Shared::with_data_dir(Some(agent), "TeamTest".into(), data_dir);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, shared)
}

fn http_host(port: u16, host: &str, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
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

fn http(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    http_host(port, &format!("127.0.0.1:{port}"), method, path, body)
}

#[test]
fn scheduler_caps_concurrency_and_claims_refuse_conflicts() {
    let dir = scratch_dir("conc-claims");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&proj_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _seq| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: w1]") {
            "w1"
        } else if sys.contains("[Agent: w2]") {
            "w2"
        } else if sys.contains("[Agent: w3]") {
            "w3"
        } else if sys.contains("[Agent: w4]") {
            "w4"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({ "tool": "create_task", "args": { "id": "t1", "owner": "w1", "title": "Task 1", "kind": "docs", "files": ["docs/a.md"] } }),
                2 => json!({ "tool": "create_task", "args": { "id": "t2", "owner": "w2", "title": "Task 2", "kind": "docs", "files": ["docs/b.md"] } }),
                3 => json!({ "tool": "create_task", "args": { "id": "t3", "owner": "w3", "title": "Task 3", "kind": "docs", "files": ["docs/c.md"] } }),
                4 => json!({ "tool": "create_task", "args": { "id": "t4", "owner": "w4", "title": "Task 4", "kind": "docs", "files": ["docs/d.md"] } }),
                5 => json!({ "done": true, "summary": "Dispatched 4 parallel workers." }),
                _ => {
                    let full = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if full.contains("Verified task t1")
                        && full.contains("Verified task t2")
                        && full.contains("Verified task t3")
                        && full.contains("Verified task t4")
                    {
                        json!({ "tool": "finish_run", "args": { "summary": "All 4 tasks done" } })
                    } else {
                        json!({ "done": true, "summary": "Waiting on remaining tasks." })
                    }
                }
            };
        }
        if let Some(n) = role.strip_prefix('w') {
            std::thread::sleep(Duration::from_millis(120));
            let own_file = format!("docs/{}.md", (b'a' + (n.as_bytes()[0] - b'1')) as char);
            return match step {
                1 => {
                    if role == "w2" {
                        // w2 tries to edit w1's claimed file docs/a.md -> must be refused!
                        json!({ "tool": "project_write_file", "args": { "path": "./docs/a.md", "content": "hijack" } })
                    } else {
                        json!({ "tool": "project_write_file", "args": { "path": own_file, "content": format!("ok {role}") } })
                    }
                }
                2 => {
                    if role == "w2" {
                        // Now w2 writes its own claimed file
                        json!({ "tool": "project_write_file", "args": { "path": "docs/b.md", "content": "ok w2" } })
                    } else {
                        json!({ "tool": "submit_task", "args": { "task_id": format!("t{n}"), "summary": format!("{role} done") } })
                    }
                }
                3 => {
                    if role == "w2" {
                        json!({ "tool": "submit_task", "args": { "task_id": "t2", "summary": "w2 done" } })
                    } else {
                        json!({ "done": true, "summary": "submitted" })
                    }
                }
                _ => json!({ "done": true, "summary": "submitted" }),
            };
        }
        // Verifier checks awaiting tasks
        let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
        for tid in ["t1", "t2", "t3", "t4"] {
            if all_text.contains(&format!("Task {tid} ("))
                && !all_text.contains(&format!("Verified task {tid}"))
                && !all_text.contains(&format!("status\":\"done\",\"title\":\"Task {}", &tid[1..]))
            {
                // Avoid re-verifying already verified tasks by checking our own assistant messages
                let already = msgs.iter().any(|m| {
                    m["role"] == "assistant"
                        && m["content"].as_str().unwrap_or("").contains(&format!("\"{tid}\""))
                });
                if !already {
                    return json!({ "tool": "verify_task", "args": { "task_id": tid, "passed": true, "notes": "ok" } });
                }
            }
        }
        json!({ "done": true, "summary": "Verified batch." })
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    // Non-loopback host is refused
    let (bad_st, _) = http_host(port, "evil.example.com:8080", "GET", "/api/teamwork", None);
    assert_eq!(bad_st, 403);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({
            "goal": "Write 4 docs concurrently with max_concurrency=2",
            "roles": ["w1", "w2", "w3", "w4", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");
    assert_eq!(stub.peak_seen.load(Ordering::SeqCst), 2, "must cap at max_concurrency=2");
    assert_eq!(detail["run"]["peak_concurrency"], 2);
    assert_eq!(std::fs::read_to_string(proj_dir.join("docs/a.md")).unwrap(), "ok w1");

    // Verify w2's trace shows the file claim conflict refusal
    let (_, tr) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/w2/trace"), None);
    assert!(tr["trace"].as_array().unwrap().iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["error"]
                .as_str()
                .is_some_and(|msg| msg.contains("file claim conflict"))
    }));

    shared.teamwork.shutdown();
}

#[test]
fn studio_lease_serializes_concurrent_acquires() {
    let lock = Arc::new(StudioLeaseLock::new());
    let in_cs = Arc::new(AtomicUsize::new(0));
    let max_cs = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for i in 0..4 {
        let l = Arc::clone(&lock);
        let cur = Arc::clone(&in_cs);
        let max = Arc::clone(&max_cs);
        handles.push(std::thread::spawn(move || {
            let guard = l
                .acquire(&format!("agent-{i}"), Some(format!("t{i}")), "playtest", || false)
                .unwrap();
            let now = cur.fetch_add(1, Ordering::SeqCst) + 1;
            let prev = max.load(Ordering::SeqCst);
            if now > prev {
                max.store(now, Ordering::SeqCst);
            }
            assert!(l.current().is_some());
            std::thread::sleep(Duration::from_millis(25));
            cur.fetch_sub(1, Ordering::SeqCst);
            drop(guard);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(max_cs.load(Ordering::SeqCst), 1, "StudioLease must allow strictly 1 holder");
    assert!(lock.current().is_none());
}

#[test]
fn resume_after_restart_preserves_seq_and_finishes_remaining_work() {
    let dir = scratch_dir("restart");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&proj_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate_c = Arc::clone(&gate);
    let t1_verified = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let t1_v_c = Arc::clone(&t1_verified);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);

    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: worker]") {
            "worker"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({ "tool": "create_task", "args": { "id": "t1", "owner": "worker", "title": "First doc", "kind": "docs", "files": ["docs/one.md"] } }),
                2 => json!({ "tool": "create_task", "args": { "id": "t2", "owner": "worker", "title": "Second doc", "kind": "docs", "depends_on": ["t1"], "files": ["docs/two.md"] } }),
                3 => json!({ "done": true, "summary": "Created t1 and t2." }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t2") {
                        json!({ "tool": "finish_run", "args": { "summary": "Done across restart" } })
                    } else {
                        json!({ "done": true, "summary": "Waiting" })
                    }
                }
            };
        }
        if role == "worker" {
            let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
            let low_text = all_text.to_ascii_lowercase();
            if !low_text.contains("task t1") {
                return json!({ "done": true, "summary": "waiting" });
            }
            if !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("docs/one.md")) {
                return json!({ "tool": "project_write_file", "args": { "path": "docs/one.md", "content": "one" } });
            }
            if !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("\"t1\"")) {
                return json!({ "tool": "submit_task", "args": { "task_id": "t1", "summary": "t1 ready" } });
            }
            // Only handle t2 once t1 has verified (or dependency unblocked t2)
            if !low_text.contains("dependencies complete") && !low_text.contains("resumed teamwork run") {
                return json!({ "done": true, "summary": "parked after t1" });
            }
            // Block t2 until after server restart opens the gate!
            while !gate_c.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(40));
            }
            if !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("docs/two.md")) {
                return json!({ "tool": "project_write_file", "args": { "path": "docs/two.md", "content": "two" } });
            }
            if !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("\"t2\"")) {
                return json!({ "tool": "submit_task", "args": { "task_id": "t2", "summary": "t2 ready" } });
            }
            return json!({ "done": true, "summary": "t2 submitted" });
        }
        // Verifier
        for tid in ["t1", "t2"] {
            let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
            let submitted = all_text.contains(&format!("Task {tid} ("));
            let verified = msgs.iter().any(|m| {
                m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains(&format!("\"{tid}\""))
            });
            if submitted && !verified {
                if tid == "t1" {
                    t1_v_c.store(true, Ordering::SeqCst);
                }
                return json!({ "tool": "verify_task", "args": { "task_id": tid, "passed": true, "notes": "ok" } });
            }
        }
        json!({ "done": true, "summary": "verified" })
    });

    // Server 1
    let (port1, shared1) = boot_server(data_dir.clone(), None);
    let (st, _) = http(
        port1,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port1,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Two-step restart test", "roles": ["worker", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201);
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    // Wait until t1 is verified done (while t2 is parked at gate)
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seq_before_restart = 0u64;
    let mut last_d = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port1, "GET", &format!("/api/teamwork/{run_id}"), None);
        last_d = d.clone();
        if t1_verified.load(Ordering::SeqCst) && d["tasks"][0]["status"] == "done" {
            seq_before_restart = d["run"]["board_seq"].as_u64().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(seq_before_restart >= 4, "t1 must verify before restart: seq={seq_before_restart} t1_verified={} detail={last_d}", t1_verified.load(Ordering::SeqCst));

    // Simulate OpenRO restart: shut down server 1, open gate, boot server 2 on same data_dir
    shared1.teamwork.shutdown();
    drop(shared1);
    std::thread::sleep(Duration::from_millis(120));
    gate.store(true, Ordering::SeqCst);

    let (port2, shared2) = boot_server(data_dir, None);
    let deadline2 = Instant::now() + Duration::from_secs(20);
    let mut detail2 = Value::Null;
    while Instant::now() < deadline2 {
        let (c, d) = http(port2, "GET", &format!("/api/teamwork/{run_id}"), None);
        assert_eq!(c, 200);
        detail2 = d;
        if detail2["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    assert_eq!(detail2["run"]["status"], "completed", "{detail2}");
    let seq_after = detail2["run"]["board_seq"].as_u64().unwrap();
    assert!(seq_after > seq_before_restart, "seq must continue monotonically ({seq_before_restart} -> {seq_after})");
    assert_eq!(detail2["tasks"][0]["status"], "done");
    assert_eq!(detail2["tasks"][1]["status"], "done");
    assert_eq!(std::fs::read_to_string(proj_dir.join("docs/two.md")).unwrap(), "two");

    shared2.teamwork.shutdown();
}

#[test]
fn human_post_stop_kill_restart_and_sse_stream() {
    let dir = scratch_dir("controls-sse");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&proj_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let stub = start_dynamic_stub(move |body, _seq| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let all = msgs
            .iter()
            .map(|m| m["content"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        if msgs[0]["content"].as_str().unwrap_or("").contains("[Agent: scout]")
            && all.contains("ping scout")
            && !all.contains("scout ack")
        {
            return json!({ "tool": "board_post", "args": { "text": "@coordinator scout ack" } });
        }
        json!({ "done": true, "summary": "parked" })
    });

    let (port, shared) = boot_server(data_dir, None);
    let _ = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Control & SSE test", "roles": ["scout", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201);
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    // Connect SSE stream
    let mut sse_stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    sse_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        sse_stream,
        "GET /api/teamwork/{run_id}/stream HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\n\r\n"
    )
    .unwrap();
    let sse_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sse_c = Arc::clone(&sse_lines);
    std::thread::spawn(move || {
        let reader = BufReader::new(sse_stream);
        for line in reader.lines().map_while(Result::ok) {
            let mut v = sse_c.lock().unwrap();
            v.push(line);
            if v.len() > 80 {
                break;
            }
        }
    });

    // Wait for coordinator initial turn to settle in waiting
    std::thread::sleep(Duration::from_millis(200));

    // Human post waking @scout
    let (st, post_res) = http(
        port,
        "POST",
        &format!("/api/teamwork/{run_id}/post"),
        Some(&json!({ "text": "@scout ping scout" })),
    );
    assert_eq!(st, 201, "{post_res}");
    assert_eq!(post_res["woken"], json!(["scout"]));

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        if d["board"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "@coordinator scout ack")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }

    // Kill scout -> state stopped, killed true
    let (st, k_res) = http(
        port,
        "POST",
        &format!("/api/teamwork/{run_id}/agents/scout/kill"),
        Some(&json!({ "reason": "test kill" })),
    );
    assert_eq!(st, 200, "{k_res}");
    assert_eq!(k_res["agent"]["state"], "stopped");
    assert_eq!(k_res["agent"]["killed"], true);

    // Restart scout -> clears killed
    let (st, r_res) = http(
        port,
        "POST",
        &format!("/api/teamwork/{run_id}/agents/scout/restart"),
        Some(&json!({ "reset": false })),
    );
    assert_eq!(st, 200, "{r_res}");
    assert_eq!(r_res["agent"]["killed"], false);

    // Stop run -> status stopped, all agents stopped
    let (st, stop_res) = http(port, "POST", &format!("/api/teamwork/{run_id}/stop"), Some(&json!({})));
    assert_eq!(st, 200, "{stop_res}");
    assert_eq!(stop_res["run"]["status"], "stopped");

    let (_, detail) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
    for a in detail["roster"].as_array().unwrap() {
        assert_eq!(a["state"], "stopped", "{a}");
    }

    // List runs includes our run
    let (st, list_res) = http(port, "GET", "/api/teamwork", None);
    assert_eq!(st, 200);
    assert!(list_res["runs"].as_array().unwrap().iter().any(|r| r["id"] == run_id));

    // Verify SSE stream received status + board events
    let captured = sse_lines.lock().unwrap().join("\n");
    assert!(captured.contains("event: status"), "{captured}");
    assert!(captured.contains("event: board"), "{captured}");

    shared.teamwork.shutdown();
}

#[test]
fn batched_steps_abort_on_error_and_manifest_forces_price_unapproved() {
    let dir = scratch_dir("steps-manifest");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": "StepsTest",
            "tree": {
                "$className": "DataModel",
                "ServerScriptService": { "$className": "ServerScriptService", "$path": "src/server" }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: builder]") {
            "builder"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "tool": "create_task",
                    "args": { "id": "t1", "owner": "builder", "title": "Write config & draft manifest", "kind": "docs", "files": ["docs/spec.md", "docs/second.md"] }
                }),
                2 => json!({ "done": true, "summary": "created t1" }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1") {
                        json!({ "tool": "finish_run", "args": { "summary": "batched steps ok" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "builder" {
            return match step {
                // Step 1 uses batched `steps`: writes docs/spec.md, hits prohibited Higgsfield refusal (must abort before third!)
                1 => json!({
                    "steps": [
                        { "tool": "project_write_file", "args": { "path": "docs/spec.md", "content": "spec v1" } },
                        { "tool": "art_generate", "args": { "provider": "higgsfield", "prompt": "card art" } },
                        { "tool": "project_write_file", "args": { "path": "docs/second.md", "content": "should not run" } }
                    ]
                }),
                // Step 2 batches manifest update (attempting priceApproved:true) + project audio_generate + assets_codegen + valid file + submit_task + trailing done
                2 => json!({
                    "steps": [
                        {
                            "tool": "project_update_manifest",
                            "args": { "patch": { "products": [{ "key": "vip", "kind": "game_pass", "name": "VIP", "priceRobux": 299, "forSale": true, "priceApproved": true }] } }
                        },
                        { "tool": "audio_generate", "args": { "preset": "summon_roll", "name": "roll_cue" } },
                        { "tool": "assets_codegen", "args": {} },
                        { "tool": "project_write_file", "args": { "path": "docs/second.md", "content": "clean after abort" } },
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "batched done" } }
                    ],
                    "done": true,
                    "summary": "submitted t1 via batch"
                }),
                _ => json!({ "done": true, "summary": "builder parked" }),
            };
        }
        let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
        if all_text.contains("Task t1 (")
            && !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("\"t1\""))
        {
            return json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "ok" } });
        }
        json!({ "done": true, "summary": "verified" })
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify batched steps & manifest gate", "roles": ["builder", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");
    assert_eq!(std::fs::read_to_string(proj_dir.join("docs/spec.md")).unwrap(), "spec v1");
    assert_eq!(std::fs::read_to_string(proj_dir.join("docs/second.md")).unwrap(), "clean after abort");

    // Verify abort-on-error stopped step 3 of batch 1 before executing it
    let (_, tr) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/builder/trace"), None);
    let trace = tr["trace"].as_array().unwrap();
    let aborted_msg = tr["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .find(|s| s.contains("aborted on first error"))
        .expect("abort notice recorded");
    assert!(!aborted_msg.contains("should not run"), "{trace:?}");
    assert!(aborted_msg.contains("prohibited"), "{aborted_msg}");

    // Verify project audio_generate wrote into <project>/assets/audio/ + updated AssetIds.luau
    assert!(proj_dir.join("assets/audio/roll-cue.wav").is_file());
    assert!(proj_dir.join("src/shared/AssetIds.luau").is_file());

    // Verify agent-submitted product had priceApproved stripped to false
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(proj_dir.join("openro.project.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["products"][0]["priceApproved"], false, "{manifest}");

    shared.teamwork.shutdown();
}

#[test]
fn lune_tool_task_bypasses_studio_playtest_and_verifies_via_run_lune() {
    let dir = scratch_dir("lune-gate");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": "LuneGateTest",
            "tree": {
                "$className": "DataModel",
                "ServerScriptService": { "$className": "ServerScriptService", "$path": "src/server" }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: builder]") {
            "builder"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "steps": [{
                        "tool": "create_task",
                        "args": { "id": "t1", "owner": "builder", "title": "Add headless econ sim", "files": ["tools/econ-sim/sim.luau"] }
                    }],
                    "done": true,
                    "summary": "dispatched lune sim task"
                }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1") {
                        json!({ "tool": "finish_run", "args": { "summary": "lune gate verified" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "builder" {
            return match step {
                1 => json!({
                    "steps": [
                        { "tool": "project_write_file", "args": { "path": "tools/econ-sim/sim.luau", "content": "local total = 0\nfor i = 1, 3 do total += i end\nprint('sim ok', total)\n" } },
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "wrote sim.luau" } }
                    ],
                    "done": true,
                    "summary": "submitted sim"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        // Verifier tries verify_task BEFORE project_run_lune (must be refused), then runs project_run_lune, then passes!
        match step {
            1 => json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "shortcut" } }),
            2 => json!({ "tool": "project_run_lune", "args": { "script": "tools/econ-sim/sim.luau", "task_id": "t1" } }),
            3 => json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "lune sim ok" } }),
            _ => json!({ "done": true, "summary": "verified lune task" }),
        }
    });

    // Note: boot_server(data_dir, None) has LocalPlace only (NO Studio MCP) -> proves Studio playtest is not required!
    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify Lune tool task without Studio", "roles": ["builder", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");
    assert_ne!(detail["tasks"][0]["kind"], "game_code", "tools/*.luau must not infer game_code");
    assert_eq!(detail["tasks"][0]["status"], "done");

    let (_, tr) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/verifier/trace"), None);
    let trace = tr["trace"].as_array().unwrap();
    assert!(trace.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["error"]
                .as_str()
                .is_some_and(|m| m.contains("requires a passing project_run_lune"))
    }), "{trace:?}");

    shared.teamwork.shutdown();
}

#[test]
fn research_and_grep_tools_enforce_ssrf_and_execute_cleanly() {
    let dir = scratch_dir("research-grep");
    let data_dir = dir.join("data");
    let proj_dir = dir.join("proj");
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::write(
        proj_dir.join("src/shared/Rates.luau"),
        "return { MythicRate = 0.001 }\n",
    )
    .unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj_dir);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: researcher]") {
            assert!(
                sys.contains("research_roblox_game")
                    && sys.contains("web_search")
                    && sys.contains("web_fetch")
                    && sys.contains("project_grep"),
                "researcher prompt missing research tools: {sys}"
            );
            "researcher"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "steps": [{
                        "tool": "create_task",
                        "args": { "id": "t1", "owner": "researcher", "title": "Audit rates & refs", "kind": "research", "files": ["docs/research.md"] }
                    }],
                    "done": true,
                    "summary": "dispatched research"
                }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1") {
                        json!({ "tool": "finish_run", "args": { "summary": "research tools verified" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "researcher" {
            return match step {
                // Step 1: project_grep succeeds, then web_fetch SSRF error aborts batch
                1 => json!({
                    "steps": [
                        { "tool": "project_grep", "args": { "query": "MythicRate", "dir": "src" } },
                        { "tool": "web_fetch", "args": { "url": "http://127.0.0.1:8080/secret" } }
                    ]
                }),
                // Step 2: verify empty web_search rejects cleanly
                2 => json!({ "tool": "web_search", "args": { "query": "   " } }),
                // Step 3: verify research_roblox_game rejects missing target cleanly
                3 => json!({ "tool": "research_roblox_game", "args": {} }),
                // Step 4: write findings and submit
                4 => json!({
                    "steps": [
                        { "tool": "project_write_file", "args": { "path": "docs/research.md", "content": "Found MythicRate = 0.001 in src/shared/Rates.luau\n" } },
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "wrote research dossier" } }
                    ],
                    "done": true,
                    "summary": "research submitted"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
        if all_text.contains("Task t1 (")
            && !msgs.iter().any(|m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("\"t1\""))
        {
            return json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "verified research doc" } });
        }
        json!({ "done": true, "summary": "verified" })
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify research_roblox_game, web_search, web_fetch, project_grep", "roles": ["researcher", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");

    let (_, tr) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/researcher/trace"), None);
    let trace = tr["trace"].as_array().unwrap();
    assert!(trace.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "project_grep"
            && e["data"]["ok"] == true
            && e["data"]["result"]["count"] == 1
    }), "{trace:?}");
    assert!(trace.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "web_fetch"
            && e["data"]["ok"] == false
    }), "{trace:?}");
    assert!(trace.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "web_search"
            && e["data"]["ok"] == false
    }), "{trace:?}");
    assert!(trace.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "research_roblox_game"
            && e["data"]["ok"] == false
    }), "{trace:?}");

    shared.teamwork.shutdown();
}

#[test]
fn self_claim_routing_history_compaction_tools_kind_and_ergonomics() {
    let dir = scratch_dir("self-claim-history-compact");
    let data_dir = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/server")).unwrap();
    std::fs::create_dir_all(proj.join("docs")).unwrap();
    std::fs::create_dir_all(proj.join("tools/econ-sim")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "SelfClaimCompactTest",
            "tree": {
                "$className": "DataModel",
                "ServerScriptService": { "$className": "ServerScriptService", "$path": "src/server" }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(proj.join("docs/SEED.md"), "# Seed\n").unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj);

    let big_body_1 = format!("# Research\n{}\n", "A".repeat(1_800));
    let big_body_2 = format!("# GDD\n{}\n", "B".repeat(1_800));
    let prompt_checks: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let prompt_checks_srv = prompt_checks.clone();
    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);

    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().cloned().unwrap_or_default();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: researcher]") {
            "researcher"
        } else if sys.contains("[Agent: builder]") {
            "builder"
        } else {
            "verifier"
        };
        let last_user = msgs
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("");
        prompt_checks_srv
            .lock()
            .unwrap()
            .push((role.to_string(), last_user.contains("Tasks:\n")));

        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "steps": [
                        { "tool": "create_task", "args": { "id": "t1", "owner": "researcher", "title": "Write research", "files": ["docs/RESEARCH.md"] } },
                        { "tool": "create_task", "args": { "id": "t2", "owner": "researcher", "title": "Write GDD", "files": ["docs/GDD.md"] } },
                        { "tool": "create_task", "args": { "id": "t3", "owner": "builder", "title": "Economy sim", "kind": "game_code", "files": ["docs/ECONOMY.md", "tools/econ-sim/sim.luau", "openro.project.json"] } }
                    ],
                    "done": true,
                    "summary": "dispatched"
                }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1")
                        && all_text.contains("Verified task t2")
                        && all_text.contains("Verified task t3")
                    {
                        json!({ "tool": "finish_run", "args": { "summary": "all verified" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "researcher" {
            return match step {
                // Step 1: project_files using `path` alias (#45a)
                1 => json!({ "tool": "project_files", "args": { "path": "docs" } }),
                // Step 2: write t2's claimed file while current_task_id == t1 (#41 self-claim routing)
                2 => json!({
                    "tool": "project_write_file",
                    "args": { "path": "docs/GDD.md", "content": big_body_2.clone() }
                }),
                // Step 3: write t1's file
                3 => json!({
                    "tool": "project_write_file",
                    "args": { "path": "docs/RESEARCH.md", "content": big_body_1.clone() }
                }),
                // Step 4: submit both tasks
                4 => json!({
                    "steps": [
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "research ready" } },
                        { "tool": "submit_task", "args": { "task_id": "t2", "summary": "gdd ready" } }
                    ],
                    "done": true,
                    "summary": "researcher finished"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        if role == "builder" {
            return match step {
                // Step 1: cross-agent write to researcher's docs/GDD.md MUST STILL FAIL
                1 => json!({
                    "tool": "project_write_file",
                    "args": { "path": "docs/GDD.md", "content": "hijack\n" }
                }),
                // Step 2: write t3's econ doc + Lune tool & submit
                2 => json!({
                    "steps": [
                        { "tool": "project_write_file", "args": { "path": "docs/ECONOMY.md", "content": "# Economy\n" } },
                        { "tool": "project_write_file", "args": { "path": "tools/econ-sim/sim.luau", "content": "print('sim ok')\n" } },
                        { "tool": "submit_task", "args": { "task_id": "t3", "summary": "econ sim ready" } }
                    ],
                    "done": true,
                    "summary": "builder finished"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        // Verifier: wait until t1, t2, and t3 are all awaiting verification so builder's
        // cross-agent claim test on t2/docs/GDD.md always runs while t2 holds its claim.
        if last_user.contains("- t1 [awaiting_verification]")
            && last_user.contains("- t2 [awaiting_verification]")
            && last_user.contains("- t3 [awaiting_verification]")
        {
            return json!({
                "steps": [
                    { "tool": "project_run_lune", "args": { "script": "tools/econ-sim/sim.luau", "task_id": "t3" } },
                    { "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "t1 ok" } },
                    { "tool": "verify_task", "args": { "task_id": "t2", "passed": true, "notes": "t2 ok" } },
                    { "tool": "verify_task", "args": { "task_id": "t3", "passed": true, "notes": "t3 verified via lune despite game_code kind" } }
                ],
                "done": true,
                "summary": "all tasks verified"
            });
        }
        json!({ "done": true, "summary": "waiting for all 3 submissions" })
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify #41, #43, #44, #45", "roles": ["researcher", "builder", "verifier"], "max_concurrency": 3 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");

    // #45b: create_task stripped openro.project.json instead of failing, and returned a note
    let (_, coord_trace) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/coordinator/trace"), None);
    let t3_res = coord_trace["trace"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "tool_result" && e["data"]["tool"] == "create_task" && e["data"]["result"]["id"] == "t3")
        .expect("t3 create_task result");
    assert!(t3_res["data"]["result"]["note"].as_str().unwrap_or("").contains("project_update_manifest"));
    assert!(!t3_res["data"]["result"]["claimed_files"].as_array().unwrap().iter().any(|f| f == "openro.project.json"));

    // #45a & #41: researcher project_files(path="docs") listed SEED.md and wrote t2's docs/GDD.md while on t1
    let (_, res_trace) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/researcher/trace"), None);
    let res_events = res_trace["trace"].as_array().unwrap();
    assert!(res_events.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "project_files"
            && e["data"]["ok"] == true
            && e["data"]["result"]["entries"]
                .as_array()
                .is_some_and(|arr| arr.iter().any(|it| it["path"] == "docs/SEED.md"))
    }), "{res_events:?}");
    assert!(res_events.iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "project_write_file"
            && e["data"]["ok"] == true
            && e["data"]["result"]["path"] == "docs/GDD.md"
            && e["data"]["result"]["task_id"] == "t2"
    }), "{res_events:?}");

    // Cross-agent write conflict on docs/GDD.md still rejected for builder
    let (_, bld_trace) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/builder/trace"), None);
    assert!(bld_trace["trace"].as_array().unwrap().iter().any(|e| {
        e["kind"] == "tool_result"
            && e["data"]["tool"] == "project_write_file"
            && e["data"]["ok"] == false
            && e["data"]["error"].as_str().unwrap_or("").contains("file claim conflict")
    }));

    // #43: model saw live "Tasks:" transiently, while agents/researcher.json persisted ZERO "Tasks:" and compacted >2-turn-old entries
    let checks = prompt_checks.lock().unwrap();
    assert!(checks.iter().filter(|(r, _)| r == "researcher").all(|(_, has_tasks)| *has_tasks), "researcher prompts missing transient Tasks: {checks:?}");

    let agent_file = proj.join(".openro/teamwork").join(&run_id).join("agents/researcher.json");
    let raw_agent = std::fs::read_to_string(&agent_file).unwrap();
    let agent_json: Value = serde_json::from_str(&raw_agent).unwrap();
    let persisted_msgs = agent_json["messages"].as_array().unwrap();
    assert!(persisted_msgs.iter().all(|m| !m["content"].as_str().unwrap_or("").contains("Tasks:\n")), "{persisted_msgs:?}");
    assert!(persisted_msgs.iter().any(|m| {
        let c = m["content"].as_str().unwrap_or("");
        c.starts_with("[compacted tool_result") && !c.contains('\n')
    }), "{persisted_msgs:?}");
    assert!(persisted_msgs.iter().any(|m| {
        let c = m["content"].as_str().unwrap_or("");
        m["role"] == "assistant" && c.contains("[compacted project_write_file content") && !c.contains(&"B".repeat(500))
    }), "{persisted_msgs:?}");

    // #44: t3 passed via Lune without needing Studio playtest
    let tasks = detail["tasks"].as_array().unwrap();
    let t3 = tasks.iter().find(|t| t["id"] == "t3").unwrap();
    assert_eq!(t3["status"], "done");
    assert_eq!(t3["verify_report"]["playtest_passed"], Value::Null);

    shared.teamwork.shutdown();
}

#[test]
fn manifest_update_aliases_written_files_autocommit_and_flat_edit_alias() {
    let dir = scratch_dir("manifest-autocommit-aliases");
    let data_dir = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("docs")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "ManifestAliasTest",
            "tree": { "$className": "DataModel" }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(proj.join(".gitignore"), ".openro/\nbuild/\n").unwrap();
    std::fs::write(proj.join("openro.project.json"), "{}\n").unwrap();
    std::fs::write(proj.join("docs/MONETIZATION.md"), "# Draft\nPRICE_PLACEHOLDER\n").unwrap();
    for args in [
        &["init"][..],
        &["config", "user.name", "OpenRO Test"],
        &["config", "user.email", "test@openro.local"],
        &["add", "default.project.json", ".gitignore", "openro.project.json", "docs/MONETIZATION.md"],
        &["commit", "-m", "init"],
    ] {
        let out = std::process::Command::new("git")
            .current_dir(&proj)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: builder]") {
            "builder"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "tool": "create_task",
                    "args": { "id": "t1", "owner": "builder", "title": "Update monetization & manifest", "kind": "docs", "files": ["docs/MONETIZATION.md"] },
                    "done": true,
                    "summary": "created t1"
                }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1") {
                        json!({ "tool": "finish_run", "args": { "summary": "manifest & edit committed" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "builder" {
            return match step {
                1 => json!({
                    "steps": [
                        {
                            "tool": "project_edit_file",
                            "args": { "path": "docs/MONETIZATION.md", "old_string": "PRICE_PLACEHOLDER", "new_string": "VIP: 349 R$" }
                        },
                        {
                            "tool": "project_update_manifest",
                            "args": {
                                "products": [{ "id": "pass_vip", "kind": "game_pass", "name": "VIP", "priceRobux": 349, "priceApproved": true }]
                            }
                        },
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "updated doc & manifest" } }
                    ],
                    "done": true,
                    "summary": "submitted t1"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        let last_user = msgs.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("");
        if last_user.contains("- t1 [awaiting_verification]") {
            json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "doc & manifest ok" }, "done": true, "summary": "verified" })
        } else {
            json!({ "done": true, "summary": "waiting" })
        }
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify #46 manifest auto-commit and aliases", "roles": ["builder", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");
    let written = detail["tasks"][0]["written_files"].as_array().unwrap();
    assert!(written.iter().any(|f| f == "openro.project.json"), "{written:?}");

    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("openro.project.json")).unwrap()).unwrap();
    assert_eq!(manifest["products"][0]["key"], "pass_vip");
    assert_eq!(manifest["products"][0]["priceApproved"], false);
    assert_eq!(std::fs::read_to_string(proj.join("docs/MONETIZATION.md")).unwrap(), "# Draft\nVIP: 349 R$\n");

    // Auto-commit staged & committed openro.project.json with zero dirty files left over
    let show = std::process::Command::new("git")
        .current_dir(&proj)
        .args(["show", "--name-only", "--format=", "HEAD"])
        .output()
        .unwrap();
    let changed = String::from_utf8_lossy(&show.stdout);
    assert!(changed.lines().any(|l| l.trim() == "openro.project.json"), "HEAD files: {changed}");
    assert!(changed.lines().any(|l| l.trim() == "docs/MONETIZATION.md"), "HEAD files: {changed}");
    let status = std::process::Command::new("git")
        .current_dir(&proj)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&status.stdout).trim().is_empty(), "dirty git status: {}", String::from_utf8_lossy(&status.stdout));

    shared.teamwork.shutdown();
}

#[test]
fn board_read_spin_breaker_auto_parks_and_prompts_include_signatures() {
    let dir = scratch_dir("board-read-spin-breaker");
    let data_dir = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj);

    let coord_sys: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let art_sys: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let cs = Arc::clone(&coord_sys);
    let asys = Arc::clone(&art_sys);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);

    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            *cs.lock().unwrap() = sys.to_string();
            "coordinator"
        } else if sys.contains("[Agent: art]") {
            *asys.lock().unwrap() = sys.to_string();
            "art"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
            if all_text.contains("Verified task t1") {
                return json!({ "tool": "finish_run", "args": { "summary": "woke after auto-park and finished" } });
            }
            return match step {
                1 => json!({
                    "tool": "create_task",
                    "args": { "id": "t1", "owner": "art", "title": "Draft asset doc", "kind": "docs", "files": ["docs/ART.md"] }
                }),
                // Step 2 tries to spin by polling board_read without done:true; spin-breaker must auto-park after 1 call!
                _ => json!({ "tool": "board_read", "args": {} }),
            };
        }
        if role == "art" {
            // Brief pause so coordinator reliably hits Step 2 (board_read with unchanged head) before t1 completes.
            std::thread::sleep(Duration::from_millis(80));
            return json!({
                "steps": [
                    { "tool": "project_write_file", "args": { "path": "docs/ART.md", "content": "# Art\n" } },
                    { "tool": "submit_task", "args": { "task_id": "t1", "summary": "art doc done" } }
                ],
                "done": true,
                "summary": "submitted"
            });
        }
        let last_user = msgs.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("");
        if last_user.contains("- t1 [awaiting_verification]") {
            json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "ok" }, "done": true, "summary": "verified" })
        } else {
            json!({ "done": true, "summary": "waiting" })
        }
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify board_read spin-breaker and art tool signatures", "roles": ["art", "verifier"], "max_concurrency": 3 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");

    // Verify spin-breaker fired exactly once on coordinator's board_read poll instead of spinning 24 turns
    let (_, tr) = http(port, "GET", &format!("/api/teamwork/{run_id}/agents/coordinator/trace"), None);
    let trace = tr["trace"].as_array().unwrap();
    let board_reads = trace
        .iter()
        .filter(|e| e["kind"] == "tool_result" && e["data"]["tool"] == "board_read")
        .count();
    assert_eq!(board_reads, 1, "expected 1 auto-parked board_read, got {board_reads}: {trace:?}");
    assert!(trace.iter().any(|e| e["kind"] == "auto_park"), "missing auto_park trace: {trace:?}");

    // Verify #48 prompt additions
    let coord_prompt = coord_sys.lock().unwrap();
    assert!(coord_prompt.contains("NEVER poll board_read"), "{coord_prompt}");
    let art_prompt = art_sys.lock().unwrap();
    for needle in [
        "reference_search: {\"character\":",
        "art_generate: {\"prompt\":",
        "image_process: {\"output\":",
        "mesh_generate: {\"name\":",
        "audio_generate: {\"name\":",
    ] {
        assert!(art_prompt.contains(needle), "missing {needle} in art prompt: {art_prompt}");
    }

    shared.teamwork.shutdown();
}

#[test]
fn asset_tools_record_written_files_and_autocommit_skips_unmatched_globs() {
    let dir = scratch_dir("asset-written-autocommit");
    let data_dir = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({
            "name": "AssetWrittenTest",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": { "Shared": { "$path": "src/shared" } }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj.join(".gitignore"),
        ".openro/\nbuild/\nart/references/\nart/previews/\nrefs/\n*.rbxl\n*.rbxlx\n",
    )
    .unwrap();
    std::fs::write(proj.join("openro.project.json"), "{}\n").unwrap();
    for args in [
        &["init"][..],
        &["config", "user.name", "OpenRO Test"],
        &["config", "user.email", "test@openro.local"],
        &["add", "default.project.json", ".gitignore", "openro.project.json"],
        &["commit", "-m", "init"],
    ] {
        let out = std::process::Command::new("git")
            .current_dir(&proj)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    std::fs::create_dir_all(&data_dir).unwrap();
    write_projects_index(&data_dir, &proj);

    let per_role: Arc<Mutex<std::collections::HashMap<String, usize>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let pr = Arc::clone(&per_role);
    let stub = start_dynamic_stub(move |body, _| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let role = if sys.contains("[Agent: coordinator]") {
            "coordinator"
        } else if sys.contains("[Agent: art]") {
            "art"
        } else {
            "verifier"
        };
        let step = {
            let mut m = pr.lock().unwrap();
            let e = m.entry(role.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        if role == "coordinator" {
            return match step {
                1 => json!({
                    "tool": "create_task",
                    // Deliberately include an unmatched glob `assets/models/**` alongside real outputs in `assets/meshes/` + `art/`
                    "args": { "id": "t1", "owner": "art", "title": "Build M1 assets & codegen", "kind": "art", "files": ["assets/models/**"] },
                    "done": true,
                    "summary": "created t1"
                }),
                _ => {
                    let all_text = msgs.iter().map(|m| m["content"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n");
                    if all_text.contains("Verified task t1") {
                        json!({ "tool": "finish_run", "args": { "summary": "all generated assets committed" } })
                    } else {
                        json!({ "done": true, "summary": "waiting" })
                    }
                }
            };
        }
        if role == "art" {
            return match step {
                1 => json!({
                    "steps": [
                        {
                            "tool": "mesh_generate",
                            "args": { "name": "mini_cube", "parts": [{ "shape": "box", "size": [2, 2, 2], "color": "#44aa88" }] }
                        },
                        {
                            "tool": "audio_generate",
                            "args": { "preset": "ui_click", "name": "click_sfx" }
                        },
                        {
                            "tool": "image_process",
                            "args": {
                                "output": "art/ui/frame_r1.png",
                                "ops": [
                                    { "op": "canvas", "width": 64, "height": 64, "color": "#00000000" },
                                    { "op": "draw", "shapes": [{ "kind": "rect", "box": [0, 0, 64, 64], "fill": "#ff4466ff" }] }
                                ],
                                "slice": [8, 8, 56, 56]
                            }
                        },
                        {
                            "tool": "art_pack_card",
                            "args": { "card_id": "hero_r1", "source_image": "art/ui/frame_r1.png", "art_size": [64, 64], "icon_size": [32, 32] }
                        },
                        { "tool": "assets_codegen", "args": {} },
                        { "tool": "submit_task", "args": { "task_id": "t1", "summary": "generated meshes, sfx, ui, card pack, AssetIds" } }
                    ],
                    "done": true,
                    "summary": "assets done"
                }),
                _ => json!({ "done": true, "summary": "parked" }),
            };
        }
        let last_user = msgs.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("");
        if last_user.contains("- t1 [awaiting_verification]") {
            json!({ "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "assets ok without playtest" }, "done": true, "summary": "verified" })
        } else {
            json!({ "done": true, "summary": "waiting" })
        }
    });

    let (port, shared) = boot_server(data_dir, None);
    let (st, _) = http(
        port,
        "POST",
        "/api/config",
        Some(&json!({ "base_url": stub.base_url, "model_id": "stub-model" })),
    );
    assert_eq!(st, 200);

    let (st, created) = http(
        port,
        "POST",
        "/api/teamwork",
        Some(&json!({ "goal": "Verify #49 written_files tracking & glob-safe git_commit", "roles": ["art", "verifier"], "max_concurrency": 2 })),
    );
    assert_eq!(st, 201, "{created}");
    let run_id = created["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut detail = Value::Null;
    while Instant::now() < deadline {
        let (_, d) = http(port, "GET", &format!("/api/teamwork/{run_id}"), None);
        detail = d;
        if detail["run"]["status"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(detail["run"]["status"], "completed", "{detail}");
    assert_eq!(detail["tasks"][0]["status"], "done");
    assert_eq!(detail["tasks"][0]["verify_report"]["playtest_passed"], Value::Null);
    assert!(detail["tasks"][0]["commit_sha"].is_string(), "{detail}");

    let written: Vec<&str> = detail["tasks"][0]["written_files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for required in [
        "assets/meshes/mini_cube.obj",
        "assets/meshes/manifest.json",
        "assets/audio/click-sfx.wav",
        "art/ui/frame_r1.png",
        "art/ui/slices.json",
        "art/cards/hero_r1/art.png",
        "art/cards/hero_r1/icon.png",
        "art/cards/hero_r1/full.png",
        "art/ART-LOG.jsonl",
        "src/shared/AssetIds.luau",
    ] {
        assert!(written.contains(&required), "missing {required} in written_files: {written:?}");
    }
    assert!(!written.iter().any(|p| p.starts_with("art/previews/")), "gitignored contact sheet leaked into written_files: {written:?}");

    let show = std::process::Command::new("git")
        .current_dir(&proj)
        .args(["show", "--name-only", "--format=", "HEAD"])
        .output()
        .unwrap();
    let changed: Vec<String> = String::from_utf8_lossy(&show.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect();
    for committed in [
        "assets/meshes/mini_cube.obj",
        "assets/meshes/manifest.json",
        "assets/audio/click-sfx.wav",
        "art/ui/frame_r1.png",
        "art/ui/slices.json",
        "art/cards/hero_r1/full.png",
        "src/shared/AssetIds.luau",
    ] {
        assert!(changed.iter().any(|c| c == committed), "missing {committed} in HEAD: {changed:?}");
    }
    let status = std::process::Command::new("git")
        .current_dir(&proj)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "dirty leftover git files: {}",
        String::from_utf8_lossy(&status.stdout)
    );

    shared.teamwork.shutdown();
}
