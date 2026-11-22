//! v0.11 Antigravity CLI provider: a fake `agy` script stands in for the real
//! one (no live calls). Checks argv (no tool permissions, deny rules, stdin
//! ignored, empty sandbox cwd), stream-json parsing, quota errors with the
//! reset time, the timeout, and the model list / provider switch over HTTP.

use openro_rs::agent::Agent;
use openro_rs::model::{
    agy_last_issue, flatten_messages, parse_agy_models, parse_agy_stream, AgyOutcome, ModelClient,
    AGY_BASE_URL, AGY_DEFAULT_MODEL,
};
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base.join("agy-tests").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_agy(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("agy");
    std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

const OK_SCRIPT: &str = r#"
if [ "$1" = "models" ]; then
  echo "Fetching available models..."
  printf 'gemini-4-hydrogen-high\tGemini 4 Hydrogen (High)\n'
  printf 'gemini-4-hydrogen-low\tGemini 4 Hydrogen (Low)\n'
  exit 0
fi
cat >/dev/null            # returns at once only when stdin is closed/null
printf '%s\n' "$@" > "$(dirname "$0")/argv.txt"
pwd > "$(dirname "$0")/cwd.txt"
echo '{"event":"init","init":{"model":"x"}}'
echo '{"event":"step_update","step_update":{"step_type":"agent_response","state":"ACTIVE","text_delta":"{\"tool\":"}}'
echo '{"event":"step_update","step_update":{"step_type":"agent_response","state":"DONE","text_delta":"\"none\"}"}}'
echo '{"event":"result","result":{"status":"SUCCESS","response":"{\"tool\":\"none\"}"}}'
"#;

fn msgs() -> Vec<Value> {
    vec![
        json!({"role":"system","content":"Answer with JSON steps only."}),
        json!({"role":"user","content":"build a coin"}),
    ]
}

#[test]
fn agy_turn_returns_text_with_no_tool_permissions() {
    let dir = scratch("ok");
    let bin = fake_agy(&dir, OK_SCRIPT);
    let sandbox = dir.join("sandbox");
    let model = ModelClient::antigravity(None)
        .with_agy_bin(bin)
        .with_agy_dir(sandbox.clone());
    assert!(model.is_antigravity() && model.is_configured());
    assert_eq!(model.model_id(), AGY_DEFAULT_MODEL);
    assert_eq!(model.base_url(), AGY_BASE_URL);

    let text = model.chat(&msgs()).expect("agy turn");
    assert_eq!(text, "{\"tool\":\"none\"}");

    let argv = std::fs::read_to_string(dir.join("argv.txt")).unwrap();
    let args: Vec<&str> = argv.lines().collect();
    assert_eq!(args[0], "-p");
    assert!(argv.contains("[Text-only mode]") && argv.contains("build a coin"));
    assert!(argv.contains("--model\ngemini-4-hydrogen-high"));
    assert!(argv.contains("--output-format\nstream-json"));
    assert!(!argv.contains("--dangerously-skip-permissions"), "{argv}");
    let settings = args.iter().position(|a| *a == "--settings").map(|i| args[i + 1]).unwrap();
    let deny: Value = serde_json::from_str(settings).unwrap();
    let deny: Vec<&str> = deny["permissions"]["deny"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    for rule in ["write_file(*)", "read_file(*)", "command(*)", "mcp(*)", "read_url(*)"] {
        assert!(deny.contains(&rule), "missing deny {rule}: {deny:?}");
    }
    let cwd = std::fs::read_to_string(dir.join("cwd.txt")).unwrap();
    assert!(Path::new(cwd.trim()).ends_with("sandbox"), "{cwd}");
}

#[test]
fn quota_refusal_becomes_a_clear_error_with_reset_time() {
    let dir = scratch("quota");
    let bin = fake_agy(
        &dir,
        r#"cat >/dev/null
echo '{"event":"result","result":{"status":"ERROR","error":"429 Individual quota reached for this model. Quota resets in 2h13m. Upgrade for more."}}'"#,
    );
    let model = ModelClient::antigravity(None).with_agy_bin(bin).with_agy_dir(dir.join("sb"));
    let err = model.chat(&msgs()).unwrap_err();
    assert!(err.contains("out of quota"), "{err}");
    assert!(err.contains("resets in 2h13m"), "{err}");
    let issue = agy_last_issue().expect("issue recorded");
    assert_eq!(issue.kind, "quota");
    assert_eq!(issue.resets_in.as_deref(), Some("2h13m"));
}

#[test]
fn empty_answer_and_silent_exit_are_errors_never_empty_text() {
    let dir = scratch("empty");
    let bin = fake_agy(&dir, r#"cat >/dev/null
echo '{"event":"result","result":{"status":"SUCCESS","response":""}}'"#);
    let model = ModelClient::antigravity(None).with_agy_bin(bin).with_agy_dir(dir.join("sb"));
    assert!(model.chat(&msgs()).unwrap_err().contains("empty answer"));

    let bin = fake_agy(&dir, "cat >/dev/null\necho boom >&2\nexit 3");
    let model = ModelClient::antigravity(None).with_agy_bin(bin).with_agy_dir(dir.join("sb"));
    let err = model.chat(&msgs()).unwrap_err();
    assert!(err.contains("boom"), "{err}");
}

#[test]
fn a_hung_agy_is_killed_at_the_timeout() {
    let dir = scratch("hang");
    let bin = fake_agy(&dir, "exec sleep 30");
    let model = ModelClient::antigravity(None)
        .with_agy_bin(bin)
        .with_agy_dir(dir.join("sb"))
        .with_agy_timeout(Duration::from_millis(800));
    let t = std::time::Instant::now();
    let err = model.chat(&msgs()).unwrap_err();
    assert!(err.contains("longer than"), "{err}");
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn stream_parsing_and_prompt_flattening() {
    let lines: Vec<String> = [
        r#"{"event":"step_update","step_update":{"text_delta":"Hel"}}"#,
        r#"{"event":"step_update","step_update":{"text_delta":"lo"}}"#,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(parse_agy_stream(&lines), AgyOutcome::Text("Hello".into()));
    assert_eq!(
        parse_agy_models("Fetching available models...\ngemini-4-hydrogen-high\tGemini 4 Hydrogen (High)\n"),
        vec![openro_rs::model::AgyModel {
            id: "gemini-4-hydrogen-high".into(),
            label: "Gemini 4 Hydrogen (High)".into()
        }]
    );
    let p = flatten_messages(&msgs());
    assert!(p.contains("[System Instructions]\nAnswer with JSON steps only."));
    assert!(p.contains("User: build a coin") && p.ends_with("Assistant:"));
    // An oversized conversation is cut in the middle, never past the argv limit.
    let big = vec![json!({"role":"user","content":"x".repeat(300_000)})];
    assert!(flatten_messages(&big).len() <= 111_000);
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
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
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

#[test]
fn model_picker_endpoints_and_persisted_selection() {
    let dir = scratch("http");
    let bin = fake_agy(&dir, OK_SCRIPT);
    std::env::set_var("OPENRO_AGY_BIN", &bin);
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "G".into()));
    let shared = Shared::with_data_dir(Some(agent), "G".into(), data.clone());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });

    let (st, cfg) = http(port, "GET", "/api/config", None);
    assert_eq!(st, 200);
    assert_eq!(cfg["provider"], "openai");

    let (st, cfg) = http(port, "POST", "/api/config", Some(r#"{"provider":"antigravity"}"#));
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["provider"], "antigravity");
    assert_eq!(cfg["model_id"], AGY_DEFAULT_MODEL);
    assert_eq!(cfg["base_url"], AGY_BASE_URL);
    assert_eq!(cfg["model_configured"], true);

    let (st, models) = http(port, "GET", "/api/models?refresh=1", None);
    assert_eq!(st, 200);
    assert_eq!(models["default"], AGY_DEFAULT_MODEL);
    let list = models["models"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{models}");
    assert_eq!(list[0]["id"], "gemini-4-hydrogen-high");
    assert_eq!(list[0]["family"], "Gemini 4 Hydrogen");
    assert_eq!(list[0]["effort"], "High");

    let (st, cfg) = http(port, "POST", "/api/config", Some(r#"{"model_id":"gemini-4-hydrogen-low"}"#));
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["model_id"], "gemini-4-hydrogen-low");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(data.join("config.json")).unwrap()).unwrap();
    assert_eq!(saved["base_url"], AGY_BASE_URL);
    assert_eq!(saved["model_id"], "gemini-4-hydrogen-low");

    // Switching back to OpenAI-compatible provider preserves compatibility.
    let (st, cfg) = http(port, "POST", "/api/config", Some(r#"{"provider":"openai","base_url":"http://127.0.0.1:11434/v1","model_id":"qwen2.5:3b"}"#));
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["provider"], "openai");
    assert_eq!(cfg["model_id"], "qwen2.5:3b");

    // Partial update when agent started directly on Antigravity (empty config.json) stays on Antigravity.
    let (st, cfg) = http(port, "POST", "/api/config", Some(r#"{"provider":"antigravity"}"#));
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["provider"], "antigravity");
    let (st, status) = http(port, "GET", "/api/status", None);
    assert_eq!(st, 200);
    assert_eq!(status["model_provider"], "antigravity");

    let (st, cfg) = http(port, "POST", "/api/config", Some(r#"{"provider":"bogus"}"#));
    assert_eq!(st, 400, "{cfg}");
}

#[test]
fn model_from_config_honours_provider_and_parse_step_is_public() {
    let dir = scratch("cfg");
    let bin = fake_agy(&dir, OK_SCRIPT);
    std::env::set_var("OPENRO_AGY_BIN", &bin);

    let cfg = openro_rs::config::RuntimeConfig {
        provider: Some("antigravity".into()),
        model_id: Some("gemini-4-hydrogen-xhigh".into()),
        ..Default::default()
    };
    let m = openro_rs::model::model_from_config(&cfg, Some(&dir));
    assert!(m.is_antigravity());
    assert_eq!(m.model_id(), "gemini-4-hydrogen-xhigh");

    let step = openro_rs::agent::parse_step(r#"{"tool":"none","args":{}}"#).unwrap();
    assert_eq!(step["tool"], "none");
}
