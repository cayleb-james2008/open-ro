//! Model client: OpenAI-shaped `POST {base}/chat/completions`.
//!
//! Loopback OpenAI-compatible gateways (`127.0.0.1`, `localhost`, `[::1]`) work
//! without a bearer API key. When `OPENRO_MODEL_BASE_URL` and
//! `OPENRO_MODEL_API_KEY` are both unset (and `OPENRO_NO_AUTO_MODEL` is not
//! `"1"`), `ModelClient::from_env` probes local gateways running on this
//! machine (`8790`, `8789`, `11434`). Remote hosts still require an API key
//! and fail closed when none is configured.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Base URL that selects the Antigravity CLI provider (`agy -p ...`) instead
/// of an OpenAI-compatible HTTP endpoint.
pub const AGY_BASE_URL: &str = "agy://antigravity";
pub const AGY_DEFAULT_MODEL: &str = "gemini-4-hydrogen-high";
const AGY_MISE_REL: &str = ".local/share/mise/installs/antigravity-cli/latest";
/// Linux caps one argv string at 128 KiB; stay safely under it.
const AGY_MAX_PROMPT_BYTES: usize = 96_000;
/// Byte budget reserved for retaining recent `user` messages (e.g. multi-turn
/// `project_read_file` `tool_result` outputs) intact before compacting older turns (#60).
const AGY_USER_RETAIN_BUDGET_BYTES: usize = 60_000;
/// Maximum number of recent `user` turns retained uncompacted inside the byte budget.
const AGY_USER_RETAIN_MAX_TURNS: usize = 8;
/// agy must act as a pure text model: every tool class is denied, so a model
/// that tries to write a file, run a command or open a URL is refused by agy
/// itself, whatever the prompt says.
const AGY_DENY_SETTINGS: &str = r#"{"permissions":{"deny":["write_file(*)","read_file(*)","command(*)","mcp(*)","read_url(*)","execute_url(*)","download_url(*)"]}}"#;
const AGY_TEXT_ONLY: &str = "[Text-only mode]\nYou are the language model inside the OpenRO game builder. You have no tools: never read or write files, never run commands, never browse. The host application performs every action for you. Reply only with the plain text (or JSON) the instructions below ask for.\n\n";

pub const DEFAULT_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
pub const DEFAULT_MODEL_ID: &str = "deepseek-v4.1-flash";
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 OpenRO/0.4";

#[derive(Clone, Debug)]
pub struct DetectedGateway {
    pub base_url: String,
    pub model_id: String,
    pub label: String,
}

const LOCAL_GATEWAYS: [(&str, &str, &str); 3] = [
    ("http://127.0.0.1:11434/v1", "qwen2.5:3b", "Ollama :11434 (Qwen 2.5 3B)"),
    ("http://127.0.0.1:8789/v1", "gemini-3.8-flash-high", "Local Gateway :8789 (Gemini 3.8 Flash)"),
    ("http://127.0.0.1:8790/v1", "deepseek-v4.1-flash-free", "Local Gateway :8790 (DeepSeek V4.1 Flash)"),
];

/// True when `url` targets loopback (`127.0.0.1`, `localhost`, or `[::1]`)
/// with a real authority boundary (`:`, `/`, or end of string), never a
/// look-alike host such as `http://localhost.evil.com`.
pub fn is_loopback_url(url: &str) -> bool {
    let trimmed = url.trim();
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// Probe local OpenAI-compatible gateways on this machine (`8790`, `8789`,
/// `11434`) with a short timeout and return every live gateway with its
/// preferred available model ID.
pub fn probe_local_gateways() -> Vec<DetectedGateway> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(600))
        .timeout_read(Duration::from_millis(900))
        .timeout(Duration::from_millis(1500))
        .build();
    let mut found = Vec::new();
    for (base, preferred_model, label) in LOCAL_GATEWAYS {
        let url = format!("{base}/models");
        let Ok(resp) = agent.get(&url).call() else {
            continue;
        };
        let Ok(val) = resp.into_json::<Value>() else {
            continue;
        };
        let ids: Vec<String> = val
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            continue;
        }
        let chosen = if ids.iter().any(|id| id == preferred_model) {
            preferred_model.to_string()
        } else {
            ids[0].clone()
        };
        found.push(DetectedGateway {
            base_url: base.to_string(),
            model_id: chosen,
            label: label.to_string(),
        });
    }
    found
}

pub fn is_agy_url(url: &str) -> bool {
    url.trim().starts_with("agy://")
}

/// Path of the agy binary: `OPENRO_AGY_BIN`, then `$HOME/.local/share/mise/.../agy`,
/// then `agy` (`agy.exe`/`agy.cmd` on Windows) on `PATH`.
pub fn agy_bin() -> PathBuf {
    if let Some(p) = std::env::var("OPENRO_AGY_BIN").ok().filter(|s| !s.trim().is_empty()) {
        return PathBuf::from(p.trim());
    }
    let names: &[&str] = if cfg!(windows) {
        &["agy.exe", "agy.cmd", "agy"]
    } else {
        &["agy"]
    };
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let mise_dir = PathBuf::from(home).join(AGY_MISE_REL);
        for name in names {
            let candidate = mise_dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from(names[0])
}

/// True when the agy binary can be started (cheap check, no model call).
pub fn agy_available() -> bool {
    let bin = agy_bin();
    if bin.components().count() > 1 {
        bin.is_file()
    } else {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|d| d.join(&bin).is_file()))
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgyModel {
    pub id: String,
    pub label: String,
}

/// Parse `agy models` output (`id<TAB>label` lines after a header line).
pub fn parse_agy_models(text: &str) -> Vec<AgyModel> {
    text.lines()
        .filter_map(|l| {
            let (id, label) = l.split_once('\t')?;
            let id = id.trim();
            if id.is_empty() || id.contains(' ') {
                return None;
            }
            Some(AgyModel { id: id.to_string(), label: label.trim().to_string() })
        })
        .collect()
}

pub fn agy_models_fallback() -> Vec<AgyModel> {
    fallback_agy_models()
}

fn fallback_agy_models() -> Vec<AgyModel> {
    [
        ("gemini-4-hydrogen-high", "Gemini 4 Hydrogen (High)"),
        ("gemini-4-hydrogen-xhigh", "Gemini 4 Hydrogen (XHigh)"),
        ("gemini-4-hydrogen-medium", "Gemini 4 Hydrogen (Medium)"),
        ("gemini-4-hydrogen-low", "Gemini 4 Hydrogen (Low)"),
    ]
    .iter()
    .map(|(i, l)| AgyModel { id: i.to_string(), label: l.to_string() })
    .collect()
}

type ModelCache = Mutex<Option<(Instant, PathBuf, Vec<AgyModel>)>>;

/// Models agy offers, cached for ten minutes. Falls back to the Hydrogen set
/// when agy cannot list (offline, not logged in). `refresh` bypasses the cache.
pub fn agy_models(refresh: bool) -> Vec<AgyModel> {
    let cache = agy_cache();
    let bin = agy_bin();
    if !refresh {
        if let Some((at, b, list)) = cache.lock().unwrap().as_ref() {
            if b == &bin && at.elapsed() < Duration::from_secs(600) {
                return list.clone();
            }
        }
    }
    let listed = run_agy_models(&bin);
    let list = if listed.is_empty() { fallback_agy_models() } else { listed };
    *cache.lock().unwrap() = Some((Instant::now(), bin, list.clone()));
    list
}

/// Cached model list without ever running agy (None until a refresh happened).
pub fn agy_models_cached() -> Option<Vec<AgyModel>> {
    // Same cache as `agy_models`; a zero-cost peek for status snapshots.
    agy_cache().lock().unwrap().as_ref().map(|(_, _, l)| l.clone())
}

fn agy_cache() -> &'static ModelCache {
    static CACHE: OnceLock<ModelCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Last agy failure (quota, sign-in, outage); cleared by the next good answer.
#[derive(Clone, Debug, PartialEq)]
pub struct AgyIssue {
    pub kind: &'static str,
    pub message: String,
    pub resets_in: Option<String>,
    pub at_unix: u64,
}

fn agy_issue_slot() -> &'static Mutex<Option<AgyIssue>> {
    static ISSUE: OnceLock<Mutex<Option<AgyIssue>>> = OnceLock::new();
    ISSUE.get_or_init(|| Mutex::new(None))
}

pub fn agy_last_issue() -> Option<AgyIssue> {
    agy_issue_slot().lock().unwrap().clone()
}

fn record_agy_issue(raw: &str, message: &str) {
    let lower = raw.to_ascii_lowercase();
    let kind = if lower.contains("quota") {
        "quota"
    } else if lower.contains("not signed in") || lower.contains("unauthenticated") || lower.contains("log in") {
        "auth"
    } else {
        "error"
    };
    let resets_in = reset_phrase(raw).map(|r| r.trim_start_matches("resets in").trim().to_string());
    let at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    *agy_issue_slot().lock().unwrap() = Some(AgyIssue {
        kind,
        message: message.to_string(),
        resets_in,
        at_unix,
    });
}

fn run_agy_models(bin: &PathBuf) -> Vec<AgyModel> {
    let Ok(mut child) = Command::new(bin)
        .arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let mut out = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(text) => {
            let _ = child.wait();
            parse_agy_models(&text)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Vec::new()
        }
    }
}

/// Compact an older turn's large script/file body so multi-file builds stay
/// well below Linux's 128 KiB single-argv limit without losing recent turns.
fn compact_older_message(role: &str, content: &str) -> String {
    if content.len() <= 2_200 {
        return content.to_string();
    }
    if role == "assistant" {
        if let Ok(mut v) = serde_json::from_str::<Value>(content.trim()) {
            let mut trimmed = false;
            let trim_obj = |obj: &mut serde_json::Map<String, Value>, flag: &mut bool| {
                for key in ["content", "source", "code", "script"] {
                    if let Some(s) = obj.get(key).and_then(|x| x.as_str()) {
                        if s.len() > 600 {
                            let head: String = s.chars().take(240).collect();
                            obj.insert(
                                key.to_string(),
                                json!(format!("{head}... [omitted {} bytes from earlier turn]", s.len())),
                            );
                            *flag = true;
                        }
                    }
                }
            };
            if let Some(args) = v.get_mut("args").and_then(|x| x.as_object_mut()) {
                trim_obj(args, &mut trimmed);
            }
            if let Some(steps) = v.get_mut("steps").and_then(|x| x.as_array_mut()) {
                for st in steps {
                    if let Some(args) = st.get_mut("args").and_then(|x| x.as_object_mut()) {
                        trim_obj(args, &mut trimmed);
                    }
                }
            }
            if trimmed {
                return v.to_string();
            }
        }
    }
    let head: String = content.chars().take(1_200).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(400)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}
[...omitted {} bytes from earlier turn; re-read file if needed...]
{tail}", content.len())
}

/// Flatten an OpenAI-style conversation into one agy prompt.
///
/// Older `assistant` writes (>2 turns old) are compacted so large script bodies
/// don't consume argv space, while recent `user` messages (`tool_result` reads)
/// use a byte-budgeted retention window (`AGY_USER_RETAIN_BUDGET_BYTES` across
/// up to `AGY_USER_RETAIN_MAX_TURNS` turns, bounded strictly within
/// `AGY_MAX_PROMPT_BYTES`) so multi-turn reads are never prematurely dropped (#60).
pub fn flatten_messages(messages: &[Value]) -> String {
    let text_of = |m: &Value| match m.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    let keep_assistant_from = messages.len().saturating_sub(4);

    // Precompute non-user rendered text and compacted user baselines so we can
    // budget how many recent `user` turns stay uncompacted within AGY_MAX_PROMPT_BYTES.
    let mut roles: Vec<&str> = Vec::with_capacity(messages.len());
    let mut raws: Vec<String> = Vec::with_capacity(messages.len());
    let mut compacted_or_non_user: Vec<String> = Vec::with_capacity(messages.len());
    let mut baseline_bytes = AGY_TEXT_ONLY.len() + "Assistant:".len();

    for (idx, m) in messages.iter().enumerate() {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let raw = text_of(m);
        let rendered_base = if role == "system" || role == "developer" {
            baseline_bytes += "[System Instructions]\n".len() + raw.len() + 2;
            raw.clone()
        } else if role == "user" {
            let comp = compact_older_message(role, &raw);
            baseline_bytes += "User: ".len() + comp.len() + 2;
            comp
        } else {
            let body = if idx >= keep_assistant_from {
                raw.clone()
            } else {
                compact_older_message(role, &raw)
            };
            let prefix_len = if role == "assistant" {
                "Assistant: ".len()
            } else {
                role.len() + 2
            };
            baseline_bytes += prefix_len + body.len() + 2;
            body
        };
        roles.push(role);
        raws.push(raw);
        compacted_or_non_user.push(rendered_base);
    }

    // Decide which `user` messages stay intact walking backward from the newest turn.
    let mut keep_user_raw = vec![false; messages.len()];
    let prompt_headroom = AGY_MAX_PROMPT_BYTES.saturating_sub(baseline_bytes);
    let mut kept_user_turns = 0usize;
    let mut kept_user_bytes = 0usize;
    let mut extra_used = 0usize;

    for idx in (0..messages.len()).rev() {
        if roles[idx] != "user" {
            continue;
        }
        let raw_len = raws[idx].len();
        let comp_len = compacted_or_non_user[idx].len();
        let extra = raw_len.saturating_sub(comp_len);
        if idx >= keep_assistant_from {
            // Always preserve the newest 2 turns verbatim (fit_prompt still enforces cap).
            keep_user_raw[idx] = true;
            kept_user_turns += 1;
            kept_user_bytes = kept_user_bytes.saturating_add(raw_len);
            extra_used = extra_used.saturating_add(extra);
            continue;
        }
        if kept_user_turns >= AGY_USER_RETAIN_MAX_TURNS {
            break;
        }
        if kept_user_bytes.saturating_add(raw_len) > AGY_USER_RETAIN_BUDGET_BYTES {
            break;
        }
        if extra_used.saturating_add(extra) > prompt_headroom {
            break;
        }
        keep_user_raw[idx] = true;
        kept_user_turns += 1;
        kept_user_bytes = kept_user_bytes.saturating_add(raw_len);
        extra_used = extra_used.saturating_add(extra);
    }

    let mut out = String::from(AGY_TEXT_ONLY);
    for idx in 0..messages.len() {
        let role = roles[idx];
        let content = if role == "user" && keep_user_raw[idx] {
            &raws[idx]
        } else {
            &compacted_or_non_user[idx]
        };
        match role {
            "system" | "developer" => out.push_str(&format!("[System Instructions]\n{content}\n\n")),
            "user" => out.push_str(&format!("User: {content}\n\n")),
            "assistant" => out.push_str(&format!("Assistant: {content}\n\n")),
            other => out.push_str(&format!("{other}: {content}\n\n")),
        }
    }
    out.push_str("Assistant:");
    fit_prompt(out)
}

/// Keep the prompt under the argv limit by cutting the middle (the oldest
/// turns), keeping the opening instructions and the latest turns.
fn fit_prompt(prompt: String) -> String {
    if prompt.len() <= AGY_MAX_PROMPT_BYTES {
        return prompt;
    }
    let head_len = AGY_MAX_PROMPT_BYTES * 35 / 100;
    let tail_len = AGY_MAX_PROMPT_BYTES - head_len - 200;
    let mut h = head_len;
    while !prompt.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = prompt.len() - tail_len;
    while !prompt.is_char_boundary(t) {
        t += 1;
    }
    format!(
        "{}\n\n[... {} bytes of older conversation left out to fit ...]\n\n{}",
        &prompt[..h],
        t - h,
        &prompt[t..]
    )
}

/// Outcome of reading agy's `stream-json` output.
#[derive(Debug, PartialEq)]
pub enum AgyOutcome {
    Text(String),
    Error(String),
}

/// Turn agy `stream-json` lines into the answer text or a clear error.
/// A `result` event with status ERROR (quota, auth, outage) is never an empty
/// answer.
pub fn parse_agy_stream(lines: &[String]) -> AgyOutcome {
    let mut deltas = String::new();
    let mut response: Option<String> = None;
    let mut error: Option<String> = None;
    for line in lines {
        let Ok(evt) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match evt.get("event").and_then(|e| e.as_str()) {
            Some("step_update") => {
                if let Some(d) = evt.pointer("/step_update/text_delta").and_then(|d| d.as_str()) {
                    deltas.push_str(d);
                }
            }
            Some("result") => {
                let r = &evt["result"];
                let status = r.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if let Some(e) = r.get("error").and_then(|e| e.as_str()).filter(|e| !e.trim().is_empty()) {
                    error = Some(e.trim().to_string());
                } else if status.eq_ignore_ascii_case("ERROR") {
                    error = Some("the model returned no response".to_string());
                }
                if let Some(t) = r.get("response").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) {
                    response = Some(t.to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(e) = error {
        let message = explain_agy_error(&e);
        record_agy_issue(&e, &message);
        return AgyOutcome::Error(message);
    }
    let text = response.unwrap_or(deltas);
    if text.trim().is_empty() {
        AgyOutcome::Error(
            "The model sent back an empty answer, so nothing was built. Send it again. (Details: Antigravity returned no text)".to_string(),
        )
    } else {
        AgyOutcome::Text(text)
    }
}

/// `resets in 2h13m` out of "Individual quota reached ... resets in 2h13m."
fn reset_phrase(raw: &str) -> Option<String> {
    let i = raw.to_ascii_lowercase().find("resets in")?;
    let rest = &raw[i..];
    let end = rest.find(". ").unwrap_or(rest.len());
    Some(rest[..end].trim().trim_end_matches('.').to_string())
}

fn explain_agy_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("quota") {
        // "Individual quota reached ... resets in 2h13m"
        return match reset_phrase(raw) {
            Some(r) => format!(
                "The Gemini model is out of quota for now, so nothing was built. Its quota {r}. Wait until then, or pick another model in Settings. (Details: {raw})"
            ),
            None => format!(
                "The Gemini model is out of quota for now, so nothing was built. Wait a while, or pick another model in Settings. (Details: {raw})"
            ),
        };
    }
    if lower.contains("unauthenticated") || lower.contains("not logged in") || lower.contains("log in") || lower.contains("sign in") {
        return format!("Antigravity is not signed in, so nothing was built. Run `agy` once in a terminal and sign in. (Details: {raw})");
    }
    format!("The model did not answer, so nothing was built. Wait a minute and send it again. (Details: {raw})")
}

#[derive(Clone)]
pub struct ModelClient {
    base_url: String,
    model_id: String,
    api_key: Option<String>,
    session_id: String,
    agent: ureq::Agent,
    agy_bin: Option<PathBuf>,
    agy_dir: PathBuf,
    agy_timeout: Duration,
}

impl ModelClient {
    pub fn new(base_url: String, model_id: String, api_key: Option<String>) -> Self {
        let session_id = format!("openro-{}-{}", std::process::id(), session_nonce());
        // A hung model endpoint must not hang the ask forever.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(90))
            .build();
        ModelClient {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            model_id: model_id.trim().to_string(),
            api_key: api_key.and_then(|k| {
                let t = k.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            }),
            session_id,
            agent,
            agy_bin: None,
            agy_dir: default_agy_dir(),
            agy_timeout: Duration::from_secs(
                std::env::var("OPENRO_AGY_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .filter(|n: &u64| *n > 0)
                    .unwrap_or(600),
            ),
        }
    }

    /// The Antigravity CLI provider with the given model (default Hydrogen).
    pub fn antigravity(model_id: Option<String>) -> Self {
        ModelClient::new(
            AGY_BASE_URL.to_string(),
            model_id.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| AGY_DEFAULT_MODEL.to_string()),
            None,
        )
    }

    /// Use this agy binary instead of `OPENRO_AGY_BIN` / the default.
    pub fn with_agy_bin(mut self, bin: PathBuf) -> Self {
        self.agy_bin = Some(bin);
        self
    }

    /// Empty working directory agy runs in.
    pub fn with_agy_dir(mut self, dir: PathBuf) -> Self {
        self.agy_dir = dir;
        self
    }

    pub fn with_agy_timeout(mut self, t: Duration) -> Self {
        self.agy_timeout = t;
        self
    }

    pub fn is_antigravity(&self) -> bool {
        is_agy_url(&self.base_url)
    }

    /// `"antigravity"` or `"openai"`.
    pub fn provider(&self) -> &'static str {
        if self.is_antigravity() { "antigravity" } else { "openai" }
    }

    /// Build from the environment (`OPENRO_MODEL_BASE_URL`, `OPENRO_MODEL_ID`,
    /// `OPENRO_MODEL_API_KEY`). When both URL and key are unset and
    /// `OPENRO_NO_AUTO_MODEL` is not `"1"`, probes local machine gateways
    /// (`8790`, `8789`, `11434`) before falling back to the unconfigured remote
    /// default.
    pub fn from_env() -> Self {
        let env_base = std::env::var("OPENRO_MODEL_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let env_model = std::env::var("OPENRO_MODEL_ID")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let env_key = std::env::var("OPENRO_MODEL_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let no_auto = std::env::var("OPENRO_NO_AUTO_MODEL")
            .ok()
            .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"));

        let no_agy = std::env::var("OPENRO_NO_AGY")
            .ok()
            .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"));

        if env_base.is_none() && env_key.is_none() && !no_auto {
            if !no_agy && agy_available() {
                return ModelClient::antigravity(env_model);
            }
            if let Some(gw) = probe_local_gateways().into_iter().next() {
                return ModelClient::new(gw.base_url, env_model.unwrap_or(gw.model_id), None);
            }
        }

        let base = env_base.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = env_model.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string());
        ModelClient::new(base, model, env_key)
    }

    /// Loopback OpenAI-compatible endpoints work without an API key; remote
    /// endpoints require one.
    pub fn is_configured(&self) -> bool {
        if self.is_antigravity() {
            return self.agy_bin.as_ref().map(|b| b.exists()).unwrap_or_else(agy_available);
        }
        self.api_key.is_some() || is_loopback_url(&self.base_url)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn is_loopback(&self) -> bool {
        self.is_antigravity() || is_loopback_url(&self.base_url)
    }

    pub fn api_key_masked(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| {
            let n = k.chars().count();
            if n <= 8 {
                "••••••••".to_string()
            } else {
                let tail: String = k.chars().skip(n - 4).collect();
                format!("••••{tail}")
            }
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// One chat completion. Returns the assistant message content.
    pub fn chat(&self, messages: &[Value]) -> Result<String, String> {
        if self.is_antigravity() {
            return self.chat_agy(messages);
        }
        if !self.is_configured() {
            return Err(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
            );
        }
        let url = format!("{}/chat/completions", self.base_url);
        let body = if is_loopback_url(&self.base_url) {
            json!({ "model": self.model_id, "messages": messages, "temperature": 0.1 })
        } else {
            json!({ "model": self.model_id, "messages": messages })
        };
        let mut req = self
            .agent
            .post(&url)
            .set("x-opencode-session", &self.session_id)
            .set("User-Agent", USER_AGENT);
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        let response = req
            .send_json(body)
            .map_err(|e| format!("The model did not answer, so nothing was built. Wait a minute and send it again. (Details: {e})"))?;
        let value: Value = response
            .into_json()
            .map_err(|e| format!("The model sent back something OpenRO could not read, so nothing was built. Send it again. (Details: {e})"))?;
        value
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "The model sent back an empty answer, so nothing was built. Send it again. (Details: no message content)".to_string())
    }
}

impl ModelClient {
    /// One turn through `agy -p`: stdin ignored (agy hangs on an open stdin),
    /// all tools denied, run in an empty sandbox directory, hard timeout.
    fn chat_agy(&self, messages: &[Value]) -> Result<String, String> {
        if !self.is_configured() {
            return Err("Antigravity (agy) is not installed here, so nothing was built. Install it or pick another model in Settings.".to_string());
        }
        let bin = self.agy_bin.clone().unwrap_or_else(agy_bin);
        let _ = std::fs::create_dir_all(&self.agy_dir);
        let prompt = flatten_messages(messages);
        let mut child = Command::new(&bin)
            .arg("-p")
            .arg(&prompt)
            .arg("--model")
            .arg(&self.model_id)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--disable-slash-commands")
            .arg("--settings")
            .arg(AGY_DENY_SETTINGS)
            .current_dir(&self.agy_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Antigravity could not start, so nothing was built. (Details: {e})"))?;
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + self.agy_timeout;
        let mut lines = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(l) => lines.push(l),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "The model took longer than {} seconds, so nothing was built. Send it again.",
                        self.agy_timeout.as_secs()
                    ));
                }
            }
        }
        let status = child.wait().ok();
        let stderr_text = err_thread.join().unwrap_or_default();
        match parse_agy_stream(&lines) {
            AgyOutcome::Text(t) => {
                *agy_issue_slot().lock().unwrap() = None;
                Ok(t)
            }
            AgyOutcome::Error(e) => {
                let tail: String = stderr_text.trim().chars().take(300).collect();
                if lines.is_empty() && !tail.is_empty() {
                    Err(format!("The model did not answer, so nothing was built. Send it again. (Details: agy exited {:?}: {tail})", status.and_then(|s| s.code())))
                } else {
                    Err(e)
                }
            }
        }
    }
}

/// Resolve the active `ModelClient` from env vars, saved `RuntimeConfig`,
/// Antigravity CLI availability, and local gateway probing (in that order).
/// Used both at server startup and by Teamwork workers so every agent honours
/// the live Settings selection and `OPENRO_AGY_BIN`.
pub fn model_from_config(
    cfg: &crate::config::RuntimeConfig,
    data_dir: Option<&std::path::Path>,
) -> ModelClient {
    let env_nonempty = |k: &str| {
        std::env::var(k)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let env_flag = |k: &str| {
        std::env::var(k)
            .ok()
            .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
    };
    let env_base = env_nonempty("OPENRO_MODEL_BASE_URL");
    let env_model = env_nonempty("OPENRO_MODEL_ID");
    let env_key = env_nonempty("OPENRO_MODEL_API_KEY");
    let no_auto = env_flag("OPENRO_NO_AUTO_MODEL");
    let no_agy = env_flag("OPENRO_NO_AGY");

    let saved_is_agy = cfg.provider.as_deref() == Some("antigravity")
        || cfg.base_url.as_deref().is_some_and(is_agy_url);
    let saved_is_openai = cfg.provider.as_deref() == Some("openai");

    let model = if env_base.is_some() || env_key.is_some() {
        let base = env_base.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let def_model = if is_agy_url(&base) { AGY_DEFAULT_MODEL } else { DEFAULT_MODEL_ID };
        ModelClient::new(base, env_model.unwrap_or_else(|| def_model.to_string()), env_key)
    } else if saved_is_agy {
        ModelClient::antigravity(env_model.or_else(|| cfg.model_id.clone()))
    } else if cfg.base_url.is_some() || cfg.api_key.is_some() {
        ModelClient::new(
            cfg.base_url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            env_model
                .or_else(|| cfg.model_id.clone())
                .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
            cfg.api_key.clone(),
        )
    } else if !saved_is_openai && !no_auto && !no_agy && agy_available() {
        ModelClient::antigravity(env_model.or_else(|| cfg.model_id.clone()))
    } else if !no_auto {
        if let Some(gw) = probe_local_gateways().into_iter().next() {
            ModelClient::new(gw.base_url, env_model.unwrap_or(gw.model_id), None)
        } else {
            ModelClient::new(
                DEFAULT_BASE_URL.to_string(),
                env_model.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
                None,
            )
        }
    } else {
        ModelClient::new(
            DEFAULT_BASE_URL.to_string(),
            env_model.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
            None,
        )
    };

    match data_dir {
        Some(dir) => model.with_agy_dir(dir.join("agy-sandbox")),
        None => model,
    }
}

fn default_agy_dir() -> PathBuf {
    std::env::var("OPENRO_DATA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("openro-data"))
        .join("agy-sandbox")
}

fn session_nonce() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 % 1_000_000_000)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_url_detection_is_strict() {
        assert!(is_loopback_url("http://127.0.0.1:8790/v1"));
        assert!(is_loopback_url("http://localhost:11434/v1"));
        assert!(is_loopback_url("http://[::1]:8789/v1"));
        assert!(!is_loopback_url("https://opencode.ai/zen/go/v1"));
        assert!(!is_loopback_url("http://localhost.evil.com/v1"));
        assert!(!is_loopback_url("http://127.0.0.1.evil.com/v1"));
    }

    #[test]
    fn loopback_works_without_api_key_while_remote_fails_closed() {
        let local = ModelClient::new(
            "http://127.0.0.1:8790/v1".to_string(),
            "deepseek-v4.1-flash-free".to_string(),
            None,
        );
        assert!(local.is_configured());
        assert!(local.is_loopback());
        assert!(!local.has_api_key());

        let remote = ModelClient::new(
            DEFAULT_BASE_URL.to_string(),
            DEFAULT_MODEL_ID.to_string(),
            None,
        );
        assert!(!remote.is_configured());
        let err = remote.chat(&[]).unwrap_err();
        assert!(err.contains("no model key configured"), "{err}");
    }

    #[test]
    fn flatten_messages_retains_multi_turn_user_reads_within_budget() {
        let mut msgs = vec![json!({"role": "system", "content": "sys instructions"})];
        // Older assistant turn (>2 turns ago) writing a huge 40 KiB script: must still compact!
        let huge_write = "w".repeat(40_000);
        msgs.push(json!({
            "role": "assistant",
            "content": json!({"tool": "project_write_file", "args": {"path": "old.luau", "content": huge_write}}).to_string()
        }));
        msgs.push(json!({"role": "user", "content": "tool_result: project_write_file: ok"}));

        // 6 subsequent turns of ~6.5 KiB `project_read_file` outputs (~39 KiB total <= 60 KiB budget).
        for turn in 1..=6 {
            msgs.push(json!({
                "role": "assistant",
                "content": format!(r#"{{"tool":"project_read_file","args":{{"path":"src/file{turn}.luau"}}}}"#)
            }));
            let pad = "a".repeat(3_000);
            let sentinel = format!("MIDDLE_SENTINEL_FILE_{turn}");
            msgs.push(json!({
                "role": "user",
                "content": format!("tool_result: project_read_file: {pad}{sentinel}{pad}")
            }));
        }

        let flat = flatten_messages(&msgs);
        assert!(
            flat.len() <= AGY_MAX_PROMPT_BYTES,
            "flattened prompt must stay <= {AGY_MAX_PROMPT_BYTES}, got {}",
            flat.len()
        );
        // Older assistant 40 KiB write is still compacted (preserving v011_agent_p1:289).
        assert!(
            flat.contains("omitted 40000 bytes from earlier turn"),
            "older assistant write must stay compacted"
        );
        // ALL 6 multi-turn user read outputs stay 100% intact with zero truncation!
        for turn in 1..=6 {
            let sentinel = format!("MIDDLE_SENTINEL_FILE_{turn}");
            assert!(
                flat.contains(&sentinel),
                "expected turn {turn} read sentinel to survive compaction"
            );
        }
        assert!(
            !flat.contains("re-read file if needed"),
            "multi-turn reads inside 60 KiB budget must not be truncated"
        );
    }

    #[test]
    fn flatten_messages_compacts_oldest_user_reads_beyond_budget_and_turn_cap() {
        // Case A: byte budget exceeded (10 turns x 9 KiB = 90 KiB > 60 KiB).
        let mut msgs = vec![json!({"role": "system", "content": "sys"})];
        for turn in 1..=10 {
            msgs.push(json!({"role": "assistant", "content": format!(r#"{{"tool":"project_read_file","args":{{"path":"f{turn:02}.luau"}}}}"#)}));
            let pad = "b".repeat(4_400);
            msgs.push(json!({
                "role": "user",
                "content": format!("tool_result: {pad}BUDGET_MARK_{turn:02}{pad}")
            }));
        }
        let flat = flatten_messages(&msgs);
        assert!(flat.len() <= AGY_MAX_PROMPT_BYTES, "got {}", flat.len());
        // Newest 6 turns (~54 KiB <= 60 KiB) stay verbatim intact.
        for turn in 5..=10 {
            assert!(flat.contains(&format!("BUDGET_MARK_{turn:02}")), "missing turn {turn}");
        }
        // Oldest turn exceeded budget and was compacted cleanly without fit_prompt chopping recent turns.
        assert!(!flat.contains("BUDGET_MARK_01"), "oldest turn should be compacted");
        assert!(flat.contains("re-read file if needed"));
        assert!(!flat.contains("of older conversation left out to fit"));

        // Case B: turn cap exceeded (10 turns x 3.5 KiB = 35 KiB <= 60 KiB, > 8 turns).
        let mut msgs_cap = vec![json!({"role": "system", "content": "sys"})];
        for turn in 1..=10 {
            msgs_cap.push(json!({"role": "assistant", "content": format!(r#"{{"tool":"project_read_file","args":{{"path":"c{turn:02}.luau"}}}}"#)}));
            let pad = "c".repeat(1_700);
            msgs_cap.push(json!({
                "role": "user",
                "content": format!("tool_result: {pad}CAP_MARK_{turn:02}{pad}")
            }));
        }
        let flat_cap = flatten_messages(&msgs_cap);
        assert!(flat_cap.len() <= AGY_MAX_PROMPT_BYTES);
        for turn in 3..=10 {
            assert!(flat_cap.contains(&format!("CAP_MARK_{turn:02}")), "missing turn {turn}");
        }
        assert!(!flat_cap.contains("CAP_MARK_01"), "turn 1 (>8 turns old) should compact");
        assert!(!flat_cap.contains("CAP_MARK_02"), "turn 2 (>8 turns old) should compact");
    }
}
