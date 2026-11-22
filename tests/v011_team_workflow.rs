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
