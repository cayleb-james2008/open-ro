//! Universal Anything-to-API / Anything-to-CLI Tool Adapter Registry (#52).
//!
//! Turns any CLI binary/script or HTTP API into a typed OpenRO capability
//! (`image_gen`, `reference_search`, `mesh_gen`, `audio_gen`, `custom`) with
//! ordered priority fallback, strict argv templating (zero shell interpolation),
//! SSRF-guarded HTTP + async polling, and `SecretStore` integration (`0600`,
//! never persisted to `adapters.json`, always scrubbed from errors/logs).

use crate::market_taste::validate_public_url;
use crate::secrets::{self, SecretStore};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const CAPABILITIES: [&str; 7] = [
    "image_gen",
    "reference_search",
    "mesh_gen",
    "audio_gen",
    "luau_tooling",
    "asset_sync",
    "custom",
];

const MAX_HTTP_BYTES: usize = 25 * 1024 * 1024;
const MAX_CLI_STDOUT_BYTES: u64 = 4 * 1024 * 1024;

/// Resolve `$HOME` (or `%USERPROFILE%` on Windows) dynamically — never hardcode
/// a personal home directory in source code.
pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Expand a leading `~/` against `$HOME` (or return the path unchanged).
pub fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        if let Some(home) = user_home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

/// Resolve a CLI executable path: expand `~/`, check explicit paths directly, or
/// search `PATH` (including `.exe`/`.cmd` on Windows).
pub fn resolve_cli_binary(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_tilde(trimmed);
    if expanded.components().count() > 1 {
        return expanded.is_file().then_some(expanded);
    }
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{trimmed}.exe"),
            format!("{trimmed}.cmd"),
            trimmed.to_string(),
        ]
    } else {
        vec![trimmed.to_string()]
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in &names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    if let Some(home) = user_home_dir() {
        for name in &names {
            let local = home.join(".local").join("bin").join(name);
            if local.is_file() {
                return Some(local);
            }
        }
    }
    None
}

/// Dynamically locate `chatgpt-camoufox-imagegen` via env override, `$HOME`, or `PATH`.
pub fn resolve_chatgpt_imagegen_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OPENRO_CHATGPT_IMAGEGEN_BIN") {
        let t = p.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    if let Some(home) = user_home_dir() {
        let skill_bin = home
            .join(".agents")
            .join("skills")
            .join("chatgpt-image-gen")
            .join("chatgpt-camoufox-imagegen");
        if skill_bin.is_file() {
            return Some(skill_bin);
        }
    }
    resolve_cli_binary("chatgpt-camoufox-imagegen")
}

/// Dynamically locate the shared-browser Python runtime via env override or `$HOME`.
pub fn resolve_shared_browser_python() -> Option<PathBuf> {
    for env_key in [
        "OPENRO_SHARED_BROWSER_PY",
        "OPENRO_SHARED_BROWSER_PYTHON",
        "OPENRO_REFERENCE_SEARCH_PYTHON",
    ] {
        if let Ok(p) = std::env::var(env_key) {
            let t = p.trim();
            if !t.is_empty() {
                return Some(PathBuf::from(t));
            }
        }
    }
    if let Some(home) = user_home_dir() {
        for rel in [
            ["Work", "services", "shared-browser", ".venv", "bin", "python"].as_slice(),
            ["Work", "services", "shared-browser", ".venv", "Scripts", "python.exe"].as_slice(),
        ] {
            let mut venv = home.clone();
            for seg in rel {
                venv.push(seg);
            }
            if venv.is_file() {
                return Some(venv);
            }
        }
    }
    None
}

fn resolve_env_bin(var: &str) -> Option<PathBuf> {
    let raw = std::env::var(var).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    resolve_cli_binary(trimmed)
}

fn resolve_mise_shim(name: &str) -> Option<PathBuf> {
    let home = user_home_dir()?;
    let shims = home.join(".local").join("share").join("mise").join("shims");
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    for cand in candidates {
        let p = shims.join(cand);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Dynamically locate Playwright / Chromium CDP CLI via `OPENRO_PLAYWRIGHT_CDP_BIN`,
/// `PATH`, `$HOME/.local/bin`, `$HOME/.local/share/mise/shims`, or `$HOME/.agents/skills`.
pub fn resolve_playwright_cdp_bin() -> Option<PathBuf> {
    if let Some(p) = resolve_env_bin("OPENRO_PLAYWRIGHT_CDP_BIN") {
        return Some(p);
    }
    if let Some(p) = resolve_cli_binary("playwright-cdp")
        .or_else(|| resolve_cli_binary("playwright"))
        .or_else(|| resolve_mise_shim("playwright-cdp"))
        .or_else(|| resolve_mise_shim("playwright"))
    {
        return Some(p);
    }
    if let Some(home) = user_home_dir() {
        let skill = home
            .join(".agents")
            .join("skills")
            .join("playwright-cdp")
            .join("playwright-cdp");
        if skill.is_file() {
            return Some(skill);
        }
    }
    None
}

/// Dynamically locate Craft Anything CLI / ledger helper via `OPENRO_CRAFT_ANYTHING_BIN`,
/// `PATH`, `$HOME/.local/bin`, or `$HOME/.agents/skills/*/scripts/craft_ledger.py`.
pub fn resolve_craft_anything_bin() -> Option<PathBuf> {
    if let Some(p) = resolve_env_bin("OPENRO_CRAFT_ANYTHING_BIN") {
        return Some(p);
    }
    if let Some(p) = resolve_cli_binary("craft-anything")
        .or_else(|| resolve_mise_shim("craft-anything"))
    {
        return Some(p);
    }
    if let Some(home) = user_home_dir() {
        for skill_dir in ["reverse-engineer-anything", "craft-anything"] {
            let script = home
                .join(".agents")
                .join("skills")
                .join(skill_dir)
                .join("scripts")
                .join("craft_ledger.py");
            if script.is_file() {
                return Some(script);
            }
        }
    }
    None
}

/// Dynamically locate Reverse Engineer Anything (REA) CLI via `OPENRO_REA_BIN`,
/// `PATH`, `$HOME/.local/bin`, or `$HOME/.local/share/mise/shims`.
pub fn resolve_rea_bin() -> Option<PathBuf> {
    if let Some(p) = resolve_env_bin("OPENRO_REA_BIN") {
        return Some(p);
    }
    resolve_cli_binary("rea")
        .or_else(|| resolve_cli_binary("rea-agents"))
        .or_else(|| resolve_mise_shim("rea"))
        .or_else(|| resolve_mise_shim("rea-agents"))
}

/// Resolve a CLI executable path inside an optional `<data_dir>/bin/<command>`
/// (or `$OPENRO_DATA_DIR/bin/<command>`) before checking dynamic preset resolvers and `PATH`.
pub fn resolve_adapter_cli_binary_in(
    data_dir_opt: Option<&Path>,
    adapter_id: &str,
    command: &str,
) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Check `<data_dir>/bin/<command>` and `$OPENRO_DATA_DIR/bin/<command>` for bare command names.
    if Path::new(trimmed).components().count() == 1 {
        let names: Vec<String> = if cfg!(windows) {
            vec![
                format!("{trimmed}.exe"),
                format!("{trimmed}.cmd"),
                trimmed.to_string(),
            ]
        } else {
            vec![trimmed.to_string()]
        };
        let mut bin_dirs: Vec<PathBuf> = Vec::new();
        if let Some(dd) = data_dir_opt {
            bin_dirs.push(dd.join("bin"));
        }
        if let Ok(env_dd) = std::env::var("OPENRO_DATA_DIR") {
            let t = env_dd.trim();
            if !t.is_empty() {
                bin_dirs.push(PathBuf::from(t).join("bin"));
            }
        }
        for dir in bin_dirs {
            for name in &names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    match (adapter_id, trimmed) {
        ("playwright-cdp-cli", "playwright" | "playwright-cdp") => resolve_playwright_cdp_bin(),
        ("craft-anything-cli", "craft-anything" | "craft_ledger.py") => resolve_craft_anything_bin(),
        ("rea-cli", "rea" | "rea-agents") => resolve_rea_bin(),
        _ => resolve_cli_binary(trimmed),
    }
}

/// Resolve the executable path for a CLI adapter, checking `$OPENRO_DATA_DIR/bin`,
/// dynamic preset resolvers, and `PATH`.
pub fn resolve_adapter_cli_binary(adapter_id: &str, command: &str) -> Option<PathBuf> {
    resolve_adapter_cli_binary_in(None, adapter_id, command)
}

#[derive(Clone, Debug)]
pub struct CliConfig {
    pub command: String,
    pub argv: Vec<String>,
    pub env_secrets: BTreeMap<String, String>,
    pub cwd: String,
    pub timeout_secs: u64,
    pub output_mode: String,
    pub result_json_path: Option<String>,
    pub exit_code_map: BTreeMap<i32, String>,
    pub health_argv: Vec<String>,
}

impl CliConfig {
    pub fn to_json(&self) -> Value {
        let exit_map: serde_json::Map<String, Value> = self
            .exit_code_map
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        json!({
            "command": self.command,
            "argv": self.argv,
            "env_secrets": self.env_secrets,
            "cwd": self.cwd,
            "timeout_secs": self.timeout_secs,
            "output_mode": self.output_mode,
            "result_json_path": self.result_json_path,
            "exit_code_map": exit_map,
            "health_argv": self.health_argv,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let mut argv = v
            .get("argv")
            .and_then(Value::as_array)
            .ok_or("cli.argv must be an array of argument strings")?
            .iter()
            .map(|x| {
                x.as_str()
                    .map(String::from)
                    .ok_or_else(|| "every entry in cli.argv must be a string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let command = match v
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(c) => c.to_string(),
            None => {
                if argv.is_empty() {
                    return Err("cli.command or a non-empty cli.argv[0] is required".to_string());
                }
                let first = argv.remove(0).trim().to_string();
                if first.is_empty() {
                    return Err("cli.argv[0] binary name cannot be empty".to_string());
                }
                first
            }
        };
        if command.contains(['\0', '\n', '\r', ';', '|', '&', '`', '$']) {
            return Err("cli.command must be a binary name or path, never a shell command string".to_string());
        }
        let mut env_secrets = BTreeMap::new();
        if let Some(obj) = v.get("env_secrets").and_then(Value::as_object) {
            for (env_k, sec_v) in obj {
                let Some(sec_name) = sec_v.as_str() else {
                    return Err(format!("cli.env_secrets.{env_k} must be a secret key name string"));
                };
                let norm = normalize_secret_key_name(sec_name)?;
                env_secrets.insert(env_k.trim().to_string(), norm);
            }
        }
        let cwd = v
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("project")
            .trim()
            .to_string();
        if !matches!(cwd.as_str(), "project" | "data_dir" | "sandbox") {
            return Err("cli.cwd must be 'project', 'data_dir', or 'sandbox'".to_string());
        }
        let timeout_secs = v
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(120)
            .clamp(1, 600);
        let output_mode = v
            .get("output_mode")
            .and_then(Value::as_str)
            .unwrap_or("stdout_json")
            .trim()
            .to_string();
        if !matches!(output_mode.as_str(), "file" | "stdout_json" | "stdout_path") {
            return Err("cli.output_mode must be 'file', 'stdout_json', or 'stdout_path'".to_string());
        }
        let result_json_path = v
            .get("result_json_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let mut exit_code_map = BTreeMap::new();
        if let Some(obj) = v.get("exit_code_map").and_then(Value::as_object) {
            for (k, val) in obj {
                let code = k
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| format!("cli.exit_code_map key '{k}' must be an integer exit code"))?;
                let status = val
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("cli.exit_code_map.{k} must be a non-empty string"))?;
                exit_code_map.insert(code, status.to_string());
            }
        }
        let health_argv = v
            .get("health_argv")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Ok(CliConfig {
            command,
            argv,
            env_secrets,
            cwd,
            timeout_secs,
            output_mode,
            result_json_path,
            exit_code_map,
            health_argv,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PollSpec {
    pub url_template: String,
    pub headers_template: Option<BTreeMap<String, String>>,
    pub status_path: String,
    pub done_values: Vec<String>,
    pub fail_values: Vec<String>,
    pub result_url_path: Option<String>,
    pub result_path: Option<String>,
    pub interval_ms: u64,
    pub max_attempts: usize,
}

impl PollSpec {
    pub fn to_json(&self) -> Value {
        json!({
            "url_template": self.url_template,
            "headers_template": self.headers_template,
            "status_path": self.status_path,
            "done_values": self.done_values,
            "fail_values": self.fail_values,
            "result_url_path": self.result_url_path,
            "result_path": self.result_path,
            "interval_ms": self.interval_ms,
            "max_attempts": self.max_attempts,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let url_template = v
            .get("url_template")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("http.poll_spec.url_template is required")?
            .to_string();
        let headers_template = v.get("headers_template").and_then(Value::as_object).map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        });
        let status_path = v
            .get("status_path")
            .and_then(Value::as_str)
            .unwrap_or("status")
            .trim()
            .to_string();
        let done_values = str_array_or(v.get("done_values"), &["COMPLETED", "SUCCEEDED", "done", "succeeded"]);
        let fail_values = str_array_or(v.get("fail_values"), &["FAILED", "ERROR", "failed", "error", "CANCELED"]);
        let result_url_path = v
            .get("result_url_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let result_path = v
            .get("result_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let interval_ms = v
            .get("interval_ms")
            .and_then(Value::as_u64)
            .unwrap_or(1000)
            .clamp(20, 10_000);
        let max_attempts = v
            .get("max_attempts")
            .and_then(Value::as_u64)
            .unwrap_or(60)
            .clamp(1, 300) as usize;
        Ok(PollSpec {
            url_template,
            headers_template,
            status_path,
            done_values,
            fail_values,
            result_url_path,
            result_path,
            interval_ms,
            max_attempts,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ResponseExtract {
    pub mode: String,
    pub path: Option<String>,
}

impl ResponseExtract {
    pub fn to_json(&self) -> Value {
        json!({ "mode": self.mode, "path": self.path })
    }
    pub fn from_json(v: Option<&Value>) -> Result<Self, String> {
        let Some(v) = v else {
            return Ok(ResponseExtract {
                mode: "json".to_string(),
                path: None,
            });
        };
        let (mode, path) = if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                ("json".to_string(), None)
            } else if let Some((m, p)) = trimmed.split_once(':') {
                let p = p.trim();
                (m.trim().to_string(), (!p.is_empty()).then(|| p.to_string()))
            } else {
                (trimmed.to_string(), None)
            }
        } else {
            let mode = v
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("json")
                .trim()
                .to_string();
            let path = v
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            (mode, path)
        };
        if !matches!(
            mode.as_str(),
            "json"
                | "image_b64_path"
                | "image_url_path"
                | "audio_b64_path"
                | "file_bytes"
                | "items_path"
        ) {
            return Err(
                "http.response_extract.mode must be json, image_b64_path, image_url_path, audio_b64_path, file_bytes, or items_path"
                    .to_string(),
            );
        }
        Ok(ResponseExtract { mode, path })
    }
}

#[derive(Clone, Debug)]
pub struct HttpConfig {
    pub method: String,
    pub url_template: String,
    pub allow_loopback: bool,
    pub headers_template: BTreeMap<String, String>,
    pub body_mode: String,
    pub body_template: Option<Value>,
    pub timeout_secs: u64,
    pub poll_spec: Option<PollSpec>,
    pub response_extract: ResponseExtract,
    pub health_url: Option<String>,
}

impl HttpConfig {
    pub fn to_json(&self) -> Value {
        json!({
            "method": self.method,
            "url_template": self.url_template,
            "allow_loopback": self.allow_loopback,
            "headers_template": self.headers_template,
            "body_mode": self.body_mode,
            "body_template": self.body_template,
            "timeout_secs": self.timeout_secs,
            "poll_spec": self.poll_spec.as_ref().map(PollSpec::to_json),
            "response_extract": self.response_extract.to_json(),
            "health_url": self.health_url,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let method = v
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("POST")
            .trim()
            .to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH") {
            return Err("http.method must be GET, POST, PUT, or PATCH".to_string());
        }
        let url_template = v
            .get("url_template")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("http.url_template is required")?
            .to_string();
        if !(url_template.starts_with("http://") || url_template.starts_with("https://")) {
            return Err("http.url_template must start with http:// or https://".to_string());
        }
        let allow_loopback = v
            .get("allow_loopback")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut headers_template = BTreeMap::new();
        if let Some(obj) = v.get("headers_template").and_then(Value::as_object) {
            for (k, val) in obj {
                let Some(s) = val.as_str() else {
                    return Err(format!("http.headers_template.{k} must be a string"));
                };
                headers_template.insert(k.clone(), s.to_string());
            }
        }
        let body_mode = v
            .get("body_mode")
            .and_then(Value::as_str)
            .unwrap_or(if method == "GET" { "none" } else { "json" })
            .trim()
            .to_string();
        if !matches!(body_mode.as_str(), "json" | "multipart" | "none") {
            return Err("http.body_mode must be json, multipart, or none".to_string());
        }
        let body_template = v.get("body_template").filter(|x| !x.is_null()).cloned();
        let timeout_secs = v
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(120)
            .clamp(1, 600);
        let poll_spec = match v.get("poll_spec").filter(|x| !x.is_null()) {
            Some(ps) => Some(PollSpec::from_json(ps)?),
            None => None,
        };
        let response_extract = ResponseExtract::from_json(v.get("response_extract"))?;
        let health_url = v
            .get("health_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        Ok(HttpConfig {
            method,
            url_template,
            allow_loopback,
            headers_template,
            body_mode,
            body_template,
            timeout_secs,
            poll_spec,
            response_extract,
            health_url,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ToolAdapter {
    pub id: String,
    pub name: String,
    pub capability: String,
    pub description: String,
    pub transport: String,
    pub enabled: bool,
    pub priority: i32,
    pub preset: bool,
    pub required_secrets: Vec<String>,
    pub cli: Option<CliConfig>,
    pub http: Option<HttpConfig>,
    pub hardware_tier: Option<String>,
    pub model_size: Option<String>,
    pub install_hint: Option<String>,
}

impl ToolAdapter {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "capability": self.capability,
            "description": self.description,
            "transport": self.transport,
            "enabled": self.enabled,
            "priority": self.priority,
            "preset": self.preset,
            "required_secrets": self.required_secrets,
            "cli": self.cli.as_ref().map(CliConfig::to_json),
            "http": self.http.as_ref().map(HttpConfig::to_json),
            "hardware_tier": self.hardware_tier,
            "model_size": self.model_size,
            "install_hint": self.install_hint,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let id = v
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or("adapter id is required")?
            .to_ascii_lowercase();
        validate_adapter_id(&id)?;
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string();
        let capability = v
            .get("capability")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("custom")
            .to_ascii_lowercase();
        if !CAPABILITIES.contains(&capability.as_str()) {
            return Err(format!(
                "invalid capability '{capability}' (expected {})",
                CAPABILITIES.join(", ")
            ));
        }
        let description = v
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let transport = v
            .get("transport")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or("adapter transport is required ('cli', 'http', or 'builtin')")?
            .to_ascii_lowercase();
        if !matches!(transport.as_str(), "builtin" | "cli" | "http") {
            return Err(format!("invalid transport '{transport}' (expected cli, http, or builtin)"));
        }
        let enabled = v.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let priority = v.get("priority").and_then(Value::as_i64).unwrap_or(100) as i32;
        let preset = v.get("preset").and_then(Value::as_bool).unwrap_or(false);
        let mut required_secrets: Vec<String> = Vec::new();
        if let Some(arr) = v.get("required_secrets").and_then(Value::as_array) {
            for x in arr {
                if let Some(s) = x.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    let norm = normalize_secret_key_name(s)?;
                    if !required_secrets.contains(&norm) {
                        required_secrets.push(norm);
                    }
                }
            }
        }

        let cli = if transport == "cli" {
            let cfg = CliConfig::from_json(v.get("cli").ok_or("cli config required for transport='cli'")?)?;
            for sec in cfg.env_secrets.values() {
                if !required_secrets.contains(sec) {
                    required_secrets.push(sec.clone());
                }
            }
            for arg in &cfg.argv {
                collect_secret_refs(arg, &mut required_secrets)?;
            }
            Some(cfg)
        } else {
            None
        };

        let http = if transport == "http" {
            let cfg = HttpConfig::from_json(v.get("http").ok_or("http config required for transport='http'")?)?;
            collect_secret_refs(&cfg.url_template, &mut required_secrets)?;
            for hv in cfg.headers_template.values() {
                collect_secret_refs(hv, &mut required_secrets)?;
            }
            if let Some(bt) = &cfg.body_template {
                collect_secret_refs_json(bt, &mut required_secrets)?;
            }
            if let Some(ps) = &cfg.poll_spec {
                collect_secret_refs(&ps.url_template, &mut required_secrets)?;
                if let Some(hdrs) = &ps.headers_template {
                    for hv in hdrs.values() {
                        collect_secret_refs(hv, &mut required_secrets)?;
                    }
                }
            }
            Some(cfg)
        } else {
            None
        };

        let opt_meta = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let hardware_tier = opt_meta("hardware_tier");
        let model_size = opt_meta("model_size");
        let install_hint = opt_meta("install_hint");

        let adapter = ToolAdapter {
            id,
            name,
            capability,
            description,
            transport,
            enabled,
            priority,
            preset,
            required_secrets,
            cli,
            http,
            hardware_tier,
            model_size,
            install_hint,
        };
        check_prohibited_provider(&adapter)?;
        Ok(adapter)
    }

    pub fn view_json(&self, store: &SecretStore) -> Value {
        self.view_json_in(None, store)
    }

    pub fn view_json_in(&self, data_dir_opt: Option<&Path>, store: &SecretStore) -> Value {
        let mut missing = Vec::new();
        let mut secrets_status = serde_json::Map::new();
        for name in &self.required_secrets {
            let val = store.get(name);
            if val.is_none() {
                missing.push(name.clone());
            }
            secrets_status.insert(
                name.clone(),
                json!({
                    "has_key": val.is_some(),
                    "masked": val.as_deref().map(secrets::mask),
                }),
            );
        }
        let binary_ok = adapter_endpoint_available_in(data_dir_opt, self);
        let available = binary_ok && missing.is_empty();
        let mut base = self.to_json();
        if let Some(o) = base.as_object_mut() {
            o.insert("available".into(), json!(available));
            o.insert("missing_secrets".into(), json!(missing));
            o.insert("secrets_status".into(), Value::Object(secrets_status));
        }
        base
    }
}

fn check_prohibited_provider(a: &ToolAdapter) -> Result<(), String> {
    let mut haystack = format!("{} {} {}", a.id, a.name, a.description);
    if let Some(c) = &a.cli {
        haystack.push(' ');
        haystack.push_str(&c.command);
    }
    if let Some(h) = &a.http {
        haystack.push(' ');
        haystack.push_str(&h.url_template);
    }
    let lower = haystack.to_ascii_lowercase();
    if lower.contains("higgsfield") || lower.contains("soul cinema") || lower.contains("soul_cinema") {
        return Err("Higgsfield / Soul Cinema adapters are prohibited.".to_string());
    }
    Ok(())
}

fn validate_adapter_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid adapter id '{id}' (use 1-64 lowercase letters, digits, '-' or '_')"
        ))
    }
}

fn normalize_secret_key_name(name: &str) -> Result<String, String> {
    let norm = name.trim().to_ascii_lowercase().replace('-', "_");
    let ok = !norm.is_empty()
        && norm.len() <= 64
        && norm
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(norm)
    } else {
        Err(format!(
            "invalid secret name '{name}' (use letters, digits, and '_')"
        ))
    }
}

fn str_array_or(v: Option<&Value>, default: &[&str]) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.iter().map(|s| s.to_string()).collect())
}

fn collect_secret_refs(template: &str, out: &mut Vec<String>) -> Result<(), String> {
    let mut rest = template;
    while let Some(pos) = rest.find("{{secret:") {
        let after = &rest[pos + "{{secret:".len()..];
        let Some(end) = after.find("}}") else {
            return Err("unclosed {{secret:...}} placeholder".to_string());
        };
        let key = normalize_secret_key_name(&after[..end])?;
        if !out.iter().any(|k| k == &key) {
            out.push(key);
        }
        rest = &after[end + 2..];
    }
    Ok(())
}

fn collect_secret_refs_json(val: &Value, out: &mut Vec<String>) -> Result<(), String> {
    match val {
        Value::String(s) => collect_secret_refs(s, out),
        Value::Array(arr) => {
            for item in arr {
                collect_secret_refs_json(item, out)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_secret_refs_json(v, out)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Built-in default adapters installed automatically on every fresh OpenRO instance.
pub fn default_installed_adapters() -> Vec<ToolAdapter> {
    vec![
        ToolAdapter {
            id: "chatgpt-browser-cli".into(),
            name: "ChatGPT (Shared Browser)".into(),
            capability: "image_gen".into(),
            description: "UI asset sheets (icons, 9-slice chrome, rarity frames) and reference-guided character & card art via shared Camoufox ChatGPT CLI (auto-detected).".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 10,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "gemini-nano-banana-http".into(),
            name: "Gemini Image (Nano Banana)".into(),
            capability: "image_gen".into(),
            description: "Google Gemini image generation API (optional fallback).".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 20,
            preset: true,
            required_secrets: vec![secrets::GEMINI_KEY.into()],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "openai-images-http".into(),
            name: "OpenAI Images (gpt-image-1)".into(),
            capability: "image_gen".into(),
            description: "OpenAI Images API with native transparent PNG support (background=\"transparent\", optional fallback).".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 30,
            preset: true,
            required_secrets: vec![secrets::OPENAI_IMAGE_KEY.into()],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "camoufox-browser-cli".into(),
            name: "Shared Browser Reference Search".into(),
            capability: "reference_search".into(),
            description: "Dynamic action-shot reference search via shared Camoufox browser (:9230).".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 10,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "builtin-http-search".into(),
            name: "Built-in HTTP Image Search".into(),
            capability: "reference_search".into(),
            description: "Read-only direct HTTP reference image search fallback.".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 20,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "blender-cycles-cli".into(),
            name: "Blender UV + Cycles AO Mesh Pipeline".into(),
            capability: "mesh_gen".into(),
            description: "Local headless Blender smart-UV + Cycles baked mesh generator.".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 10,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "rust-synth-builtin".into(),
            name: "Built-in 44.1 kHz WAV Synthesizer".into(),
            capability: "audio_gen".into(),
            description: "Zero-dependency Rust multi-layer SFX & fanfare synthesizer.".into(),
            transport: "builtin".into(),
            enabled: true,
            priority: 10,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: None,
            hardware_tier: Some("builtin-cpu".into()),
            model_size: Some("Built-in Pure Rust DSP".into()),
            install_hint: None,
        },
    ]
}

/// Optional 1-click installable adapter presets (ComfyUI, fal.ai Flux, Meshy, YouTube copyright-free music,
/// Hardware-Tiered SFX generators & ElevenLabs/Lyria).
pub fn optional_preset_adapters() -> Vec<ToolAdapter> {
    vec![
        ToolAdapter {
            id: "comfyui-local-http".into(),
            name: "ComfyUI (Local HTTP)".into(),
            capability: "image_gen".into(),
            description: "Local ComfyUI workflow API on 127.0.0.1:8188 with async history polling.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 40,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "http://127.0.0.1:8188/prompt".into(),
                allow_loopback: true,
                headers_template: BTreeMap::new(),
                body_mode: "json".into(),
                body_template: Some(json!({ "prompt": { "text": "{{prompt}}" } })),
                timeout_secs: 180,
                poll_spec: Some(PollSpec {
                    url_template: "http://127.0.0.1:8188/history/{{resp:prompt_id}}".into(),
                    headers_template: None,
                    status_path: "status.status_str".into(),
                    done_values: vec!["success".into(), "COMPLETED".into()],
                    fail_values: vec!["error".into(), "FAILED".into()],
                    result_url_path: None,
                    result_path: None,
                    interval_ms: 1000,
                    max_attempts: 90,
                }),
                response_extract: ResponseExtract {
                    mode: "image_url_path".into(),
                    path: Some("image_url".into()),
                },
                health_url: Some("http://127.0.0.1:8188/system_stats".into()),
            }),
            hardware_tier: Some("local-http".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "fal-flux-http".into(),
            name: "fal.ai Flux Schnell".into(),
            capability: "image_gen".into(),
            description: "Fast cloud Flux image generation via fal.ai Queue API.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 50,
            preset: true,
            required_secrets: vec!["fal_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://queue.fal.run/fal-ai/flux/schnell".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "Authorization".into(),
                    "Key {{secret:fal_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "prompt": "{{prompt}}",
                    "image_size": "portrait_4_3"
                })),
                timeout_secs: 120,
                poll_spec: Some(PollSpec {
                    url_template: "{{resp:status_url}}".into(),
                    headers_template: None,
                    status_path: "status".into(),
                    done_values: vec!["COMPLETED".into()],
                    fail_values: vec!["FAILED".into(), "ERROR".into()],
                    result_url_path: Some("response_url".into()),
                    result_path: None,
                    interval_ms: 1500,
                    max_attempts: 60,
                }),
                response_extract: ResponseExtract {
                    mode: "image_url_path".into(),
                    path: Some("images.0.url".into()),
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "recraft-v3-ui".into(),
            name: "Recraft V3 UI".into(),
            capability: "image_gen".into(),
            description: "Recraft V3 crisp vector-style game UI sprite sheets, icons, and 9-slice chrome via HTTP API.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 45,
            preset: true,
            required_secrets: vec!["recraft_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://external.api.recraft.ai/v1/images/generations".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "Authorization".into(),
                    "Bearer {{secret:recraft_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "prompt": "{{prompt}}",
                    "model": "recraftv3",
                    "style": "digital_illustration",
                    "size": "1024x1024"
                })),
                timeout_secs: 120,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "image_url_path".into(),
                    path: Some("data.0.url".into()),
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "layer-ai-ui".into(),
            name: "Layer.ai Game UI".into(),
            capability: "image_gen".into(),
            description: "Layer.ai transparent game UI panels, badges, and cohesive sprite sheets via HTTP API.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 46,
            preset: true,
            required_secrets: vec!["layer_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://api.app.layer.ai/v1/generate".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "Authorization".into(),
                    "Bearer {{secret:layer_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "prompt": "{{prompt}}",
                    "transparent": true,
                    "format": "png"
                })),
                timeout_secs: 120,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "image_url_path".into(),
                    path: Some("images.0.url".into()),
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "scenario-game-ui".into(),
            name: "Scenario Game UI".into(),
            capability: "image_gen".into(),
            description: "Scenario.com style-consistent game UI asset sheets, rarity frames, and icon packs via HTTP API.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 47,
            preset: true,
            required_secrets: vec!["scenario_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://api.cloud.scenario.com/v1/generate".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "Authorization".into(),
                    "Basic {{secret:scenario_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "prompt": "{{prompt}}",
                    "modelId": "flux-ui-game",
                    "numSamples": 1
                })),
                timeout_secs: 120,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "image_url_path".into(),
                    path: Some("inference.images.0.url".into()),
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "meshy-http".into(),
            name: "Meshy 3D API".into(),
            capability: "mesh_gen".into(),
            description: "Cloud text-to-3D mesh generation via Meshy v2 API with async polling.".into(),
            transport: "http".into(),
            enabled: true,
            priority: 20,
            preset: true,
            required_secrets: vec!["meshy_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://api.meshy.ai/v2/text-to-3d".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "Authorization".into(),
                    "Bearer {{secret:meshy_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "mode": "preview",
                    "prompt": "{{prompt}}",
                    "art_style": "stylized"
                })),
                timeout_secs: 240,
                poll_spec: Some(PollSpec {
                    url_template: "https://api.meshy.ai/v2/text-to-3d/{{resp:result}}".into(),
                    headers_template: None,
                    status_path: "status".into(),
                    done_values: vec!["SUCCEEDED".into()],
                    fail_values: vec!["FAILED".into(), "EXPIRED".into()],
                    result_url_path: None,
                    result_path: None,
                    interval_ms: 2000,
                    max_attempts: 90,
                }),
                response_extract: ResponseExtract {
                    mode: "json".into(),
                    path: None,
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "youtube-nocopyright-music-cli".into(),
            name: "YouTube Copyright-Free Music (yt-dlp)".into(),
            capability: "audio_gen".into(),
            description: "Background music sourcing: imports copyright-free / royalty-free YouTube tracks in Roblox-trending genres (jazz-rap, lofi, trap, carti-type-beat, pluggnb, anime-hiphop, synthwave; zero dubstep) and masters loop-ready 44.1 kHz PCM16 mono WAV with loudnorm + fades and a license/credit receipt.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 5,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-yt-music".into(),
                argv: vec![
                    "--download".into(),
                    "{{arg:youtube_url}}".into(),
                    "--query".into(),
                    "{{?prompt}}".into(),
                    "--genre".into(),
                    "{{?arg:genre}}".into(),
                    "--max-duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 300,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("youtube-nocopyright".into()),
            model_size: Some("YouTube Copyright-Free · yt-dlp".into()),
            install_hint: Some("Requires yt-dlp + ffmpeg; click Install Music Runner".into()),
        },
        ToolAdapter {
            id: "elevenlabs-sfx-http".into(),
            name: "ElevenLabs Sound Effects API".into(),
            capability: "audio_gen".into(),
            description: "SFX generator: prompt-to-SFX one-shots via ElevenLabs Sound Generation API (use youtube-nocopyright-music-cli for background music).".into(),
            transport: "http".into(),
            enabled: true,
            priority: 20,
            preset: true,
            required_secrets: vec!["elevenlabs_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://api.elevenlabs.io/v1/sound-generation".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "xi-api-key".into(),
                    "{{secret:elevenlabs_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({ "text": "{{prompt}}" })),
                timeout_secs: 60,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "file_bytes".into(),
                    path: None,
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: Some("ElevenLabs Cloud API".into()),
            install_hint: Some("Add ELEVENLABS_API_KEY in Settings -> Secrets".into()),
        },
        ToolAdapter {
            id: "audioldm-s-local-cli".into(),
            name: "AudioLDM-S Local (Tiny CPU)".into(),
            capability: "audio_gen".into(),
            description: "Lightweight local text-to-SFX model (~185M) via openro-audio-gen, runs on CPU with 4 GB RAM.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 21,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-audio-gen".into(),
                argv: vec![
                    "--backend".into(),
                    "audioldm_s".into(),
                    "--prompt".into(),
                    "{{prompt}}".into(),
                    "--duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                    "--seed".into(),
                    "{{?seed}}".into(),
                    "--steps".into(),
                    "{{?steps}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 240,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("tiny-cpu".into()),
            model_size: Some("~185M · CPU / 4GB RAM".into()),
            install_hint: Some("Install Local Audio Runner, then: pip install torch diffusers transformers scipy soundfile".into()),
        },
        ToolAdapter {
            id: "stable-audio-small-cli".into(),
            name: "Stable Audio Open Small (CPU)".into(),
            capability: "audio_gen".into(),
            description: "Fast local open-weights SFX & loop generator (~341M) via openro-audio-gen for CPU / 8 GB RAM machines.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 22,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-audio-gen".into(),
                argv: vec![
                    "--backend".into(),
                    "stable_audio_small".into(),
                    "--prompt".into(),
                    "{{prompt}}".into(),
                    "--duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                    "--seed".into(),
                    "{{?seed}}".into(),
                    "--steps".into(),
                    "{{?steps}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 240,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("small-cpu".into()),
            model_size: Some("~341M · CPU / 8GB RAM".into()),
            install_hint: Some("Install Local Audio Runner, then: pip install torch diffusers transformers scipy soundfile".into()),
        },
        ToolAdapter {
            id: "tangoflux-sfx-cli".into(),
            name: "TangoFlux SFX (Medium GPU)".into(),
            capability: "audio_gen".into(),
            description: "High-fidelity 44.1 kHz text-to-SFX model (~515M) via openro-audio-gen for 8-12 GB GPUs.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 23,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-audio-gen".into(),
                argv: vec![
                    "--backend".into(),
                    "tangoflux".into(),
                    "--prompt".into(),
                    "{{prompt}}".into(),
                    "--duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                    "--seed".into(),
                    "{{?seed}}".into(),
                    "--steps".into(),
                    "{{?steps}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 240,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("medium-gpu".into()),
            model_size: Some("~515M · 8-12GB GPU".into()),
            install_hint: Some("Install Local Audio Runner, then: pip install torch tangoflux scipy soundfile".into()),
        },
        ToolAdapter {
            id: "audioldm2-sfx-cli".into(),
            name: "AudioLDM 2 SFX (Large GPU)".into(),
            capability: "audio_gen".into(),
            description: "Rich general audio & SFX diffusion model (~1.1B) via openro-audio-gen for 12-16 GB GPUs.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 24,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-audio-gen".into(),
                argv: vec![
                    "--backend".into(),
                    "audioldm2".into(),
                    "--prompt".into(),
                    "{{prompt}}".into(),
                    "--duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                    "--seed".into(),
                    "{{?seed}}".into(),
                    "--steps".into(),
                    "{{?steps}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 300,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("large-gpu".into()),
            model_size: Some("~1.1B · 12-16GB GPU".into()),
            install_hint: Some("Install Local Audio Runner, then: pip install torch diffusers transformers scipy soundfile".into()),
        },
        ToolAdapter {
            id: "stable-audio-open-cli".into(),
            name: "Stable Audio Open 1.0 (Large GPU)".into(),
            capability: "audio_gen".into(),
            description: "SFX generator: studio-grade 44.1 kHz SFX & short stingers (~1.05B) via openro-audio-gen for 16 GB+ GPUs.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 25,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "openro-audio-gen".into(),
                argv: vec![
                    "--backend".into(),
                    "stable_audio_open".into(),
                    "--prompt".into(),
                    "{{prompt}}".into(),
                    "--duration".into(),
                    "{{?duration_s}}".into(),
                    "--out".into(),
                    "{{out_path}}".into(),
                    "--seed".into(),
                    "{{?seed}}".into(),
                    "--steps".into(),
                    "{{?steps}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 300,
                output_mode: "file".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--check".into()],
            }),
            http: None,
            hardware_tier: Some("large-gpu".into()),
            model_size: Some("~1.05B · 16GB+ GPU".into()),
            install_hint: Some("Install Local Audio Runner, then: pip install torch diffusers transformers scipy soundfile".into()),
        },
        ToolAdapter {
            id: "ace-step-local-http".into(),
            name: "ACE-Step 1.5 (Local HTTP)".into(),
            capability: "audio_gen".into(),
            description: "SFX generator: local ACE-Step 1.5 HTTP server on 127.0.0.1:8001 for short stingers & jingles (use youtube-nocopyright-music-cli for background music).".into(),
            transport: "http".into(),
            enabled: true,
            priority: 26,
            preset: true,
            required_secrets: vec![],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "http://127.0.0.1:8001/release_task".into(),
                allow_loopback: true,
                headers_template: BTreeMap::new(),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "prompt": "{{prompt}}",
                    "audio_duration": "{{?duration_s}}",
                    "audio_format": "wav"
                })),
                timeout_secs: 180,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "json".into(),
                    path: None,
                },
                health_url: Some("http://127.0.0.1:8001/health".into()),
            }),
            hardware_tier: Some("local-http".into()),
            model_size: Some("ACE-Step 1.5 · Local HTTP".into()),
            install_hint: Some("Start local ACE-Step 1.5 HTTP server on http://127.0.0.1:8001".into()),
        },
        ToolAdapter {
            id: "google-gemini-lyria-http".into(),
            name: "Google Gemini Lyria Audio".into(),
            capability: "audio_gen".into(),
            description: "SFX generator: Google Gemini Lyria short audio clips & stingers via Interactions API (decoded & normalized to 44.1 kHz WAV).".into(),
            transport: "http".into(),
            enabled: true,
            priority: 27,
            preset: true,
            required_secrets: vec!["gemini_api_key".into()],
            cli: None,
            http: Some(HttpConfig {
                method: "POST".into(),
                url_template: "https://generativelanguage.googleapis.com/v1beta/interactions".into(),
                allow_loopback: false,
                headers_template: BTreeMap::from([(
                    "x-goog-api-key".into(),
                    "{{secret:gemini_api_key}}".into(),
                )]),
                body_mode: "json".into(),
                body_template: Some(json!({
                    "model": "lyria-3-clip-preview",
                    "input": "{{prompt}}",
                    "response_modalities": ["audio"]
                })),
                timeout_secs: 90,
                poll_spec: None,
                response_extract: ResponseExtract {
                    mode: "audio_b64_path".into(),
                    path: Some("outputs.0.data".into()),
                },
                health_url: None,
            }),
            hardware_tier: Some("cloud-api".into()),
            model_size: Some("Google Gemini Lyria API".into()),
            install_hint: Some("Add GEMINI_API_KEY in Settings -> Secrets".into()),
        },
        ToolAdapter {
            id: "playwright-cdp-cli".into(),
            name: "Playwright CDP (Headless Browser)".into(),
            capability: "reference_search".into(),
            description: "Headless Chromium/Playwright loopback CDP passive inspection and reference search (compatible with OPENRO_BROWSER_CDP_URL / http://127.0.0.1:9222 live browser CDP probe; modern replacement for Camoufox; installable under reference_search or custom).".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 5,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "playwright".into(),
                argv: vec![
                    "screenshot".into(),
                    "--browser".into(),
                    "chromium".into(),
                    "--viewport-size".into(),
                    "1280,720".into(),
                    "--query".into(),
                    "{{?query}}".into(),
                    "--cdp".into(),
                    "{{?arg:cdp_url}}".into(),
                    "{{?arg:url}}".into(),
                    "{{?out_path}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 90,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::from([
                    (2, "login_required".into()),
                    (6, "rate_limited".into()),
                ]),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "craft-anything-cli".into(),
            name: "Craft Anything Study & Check CLI".into(),
            capability: "custom".into(),
            description: "Deterministic Craft Anything study ledger, unknowns tracker, mechanics verifier, screenshot likeness checker, and KeptChecks fingerprinter.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 20,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "craft-anything".into(),
                argv: vec![
                    "{{arg:subcommand}}".into(),
                    "--mode".into(),
                    "{{?arg:mode}}".into(),
                    "{{?arg:target}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 120,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--help".into()],
            }),
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "rea-cli".into(),
            name: "Reverse Engineer Anything (REA) CLI".into(),
            capability: "custom".into(),
            description: "Shipped-artifact and passive runtime reverse-engineering CLI (REA) resolved dynamically via OPENRO_REA_BIN, $HOME, or PATH.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 30,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "rea".into(),
                argv: vec![
                    "{{arg:subcommand}}".into(),
                    "--target".into(),
                    "{{?arg:target}}".into(),
                    "--out".into(),
                    "{{?out_path}}".into(),
                    "--json".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 180,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: None,
            model_size: None,
            install_hint: None,
        },
        ToolAdapter {
            id: "blender-mcp-oss".into(),
            name: "Blender MCP (ahujasid/mcp-for-blender)".into(),
            capability: "mesh_gen".into(),
            description: "OSS MIT Blender MCP bridge (ahujasid/mcp-for-blender, 36k+ stars) complementing OpenRO's headless Cycles .mesh/.glb pipeline.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 40,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "blender-mcp".into(),
                argv: vec!["--help".into()],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 60,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--help".into()],
            }),
            http: None,
            hardware_tier: Some("local-oss".into()),
            model_size: Some("OSS MCP · MIT".into()),
            install_hint: Some("Install via: uv tool install blender-mcp".into()),
        },
        ToolAdapter {
            id: "luau-lsp-cli".into(),
            name: "Luau Language Server (JohnnyMorganz/luau-lsp)".into(),
            capability: "luau_tooling".into(),
            description: "OSS MIT static typecheck and Luau analyzer (JohnnyMorganz/luau-lsp).".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 41,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "luau-lsp".into(),
                argv: vec![
                    "analyze".into(),
                    "{{?arg:target}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 90,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: Some("local-oss".into()),
            model_size: Some("OSS CLI · MIT".into()),
            install_hint: Some("Download Linux/Win release from JohnnyMorganz/luau-lsp or install via rokit".into()),
        },
        ToolAdapter {
            id: "darklua-cli".into(),
            name: "Darklua Transformer (seaofvoices/darklua)".into(),
            capability: "luau_tooling".into(),
            description: "OSS MIT Luau AST transformer, path-require resolver, and minifier (seaofvoices/darklua).".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 42,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "darklua".into(),
                argv: vec![
                    "process".into(),
                    "{{arg:input}}".into(),
                    "{{arg:output}}".into(),
                ],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 90,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: Some("local-oss".into()),
            model_size: Some("OSS CLI · MIT".into()),
            install_hint: Some("Download release from seaofvoices/darklua or install via rokit".into()),
        },
        ToolAdapter {
            id: "asphalt-cli".into(),
            name: "Asphalt Asset Sync (jackTabsCode/asphalt)".into(),
            capability: "asset_sync".into(),
            description: "OSS MIT Roblox asset syncer & AssetIds Luau generator (jackTabsCode/asphalt).".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 43,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "asphalt".into(),
                argv: vec!["sync".into()],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 180,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: Some("local-oss".into()),
            model_size: Some("OSS CLI · MIT".into()),
            install_hint: Some("Download release from jackTabsCode/asphalt or install via rokit".into()),
        },
        ToolAdapter {
            id: "rokit-cli".into(),
            name: "Rokit Toolchain Manager (rojo-rbx/rokit)".into(),
            capability: "luau_tooling".into(),
            description: "OSS MIT Roblox toolchain manager (rojo-rbx/rokit) locking rojo, selene, stylua, wally, and darklua versions.".into(),
            transport: "cli".into(),
            enabled: true,
            priority: 44,
            preset: true,
            required_secrets: vec![],
            cli: Some(CliConfig {
                command: "rokit".into(),
                argv: vec!["list".into()],
                env_secrets: BTreeMap::new(),
                cwd: "project".into(),
                timeout_secs: 60,
                output_mode: "stdout_json".into(),
                result_json_path: None,
                exit_code_map: BTreeMap::new(),
                health_argv: vec!["--version".into()],
            }),
            http: None,
            hardware_tier: Some("local-oss".into()),
            model_size: Some("OSS CLI · MIT".into()),
            install_hint: Some("Download release from rojo-rbx/rokit".into()),
        },
    ]
}

pub fn starter_templates() -> Vec<Value> {
    vec![
        json!({
            "id": "custom-cli",
            "name": "Custom CLI Tool",
            "capability": "custom",
            "description": "Wrap any local binary or script with strict argv templates (zero shell injection).",
            "transport": "cli",
            "enabled": true,
            "priority": 50,
            "required_secrets": [],
            "cli": {
                "command": "my-tool",
                "argv": ["--prompt", "{{prompt}}", "--out", "{{out_path}}"],
                "env_secrets": {},
                "cwd": "project",
                "timeout_secs": 120,
                "output_mode": "stdout_json",
                "result_json_path": null,
                "exit_code_map": {},
                "health_argv": ["--version"]
            }
        }),
        json!({
            "id": "custom-http",
            "name": "Custom HTTP API",
            "capability": "custom",
            "description": "Connect any REST / async-polling HTTP API with SecretStore auth headers.",
            "transport": "http",
            "enabled": true,
            "priority": 50,
            "required_secrets": [],
            "http": {
                "method": "POST",
                "url_template": "https://api.example.com/v1/generate",
                "allow_loopback": false,
                "headers_template": { "Authorization": "Bearer {{secret:my_api_key}}" },
                "body_mode": "json",
                "body_template": { "prompt": "{{prompt}}" },
                "timeout_secs": 120,
                "poll_spec": null,
                "response_extract": { "mode": "json", "path": null },
                "health_url": null
            }
        }),
    ]
}

fn adapter_endpoint_available_in(data_dir_opt: Option<&Path>, a: &ToolAdapter) -> bool {
    match a.transport.as_str() {
        "builtin" => match a.id.as_str() {
            "chatgpt-browser-cli" => resolve_chatgpt_imagegen_bin().is_some_and(|p| p.is_file()),
            "camoufox-browser-cli" => {
                std::env::var("OPENRO_REFERENCE_SEARCH_BIN")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .is_some()
                    || resolve_shared_browser_python().is_some_and(|p| p.is_file())
            }
            "blender-cycles-cli" => crate::mesh::blender_status(None)["available"]
                .as_bool()
                .unwrap_or(false),
            _ => true,
        },
        "cli" => a
            .cli
            .as_ref()
            .and_then(|c| resolve_adapter_cli_binary_in(data_dir_opt, &a.id, &c.command))
            .is_some(),
        "http" => true,
        _ => false,
    }
}

/// Persistent adapter registry backed by `<data_dir>/adapters.json`.
#[derive(Clone, Debug)]
pub struct AdapterRegistry {
    path: PathBuf,
    adapters: Vec<ToolAdapter>,
}

impl AdapterRegistry {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("adapters.json");
        let mut adapters = default_installed_adapters();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                if let Some(arr) = val.get("adapters").and_then(Value::as_array) {
                    let mut loaded = Vec::new();
                    for item in arr {
                        if let Ok(a) = ToolAdapter::from_json(item) {
                            loaded.push(a);
                        }
                    }
                    if !loaded.is_empty() {
                        // Ensure any newly introduced default builtins are present unless explicitly removed.
                        let removed: Vec<String> = val
                            .get("removed_defaults")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        for def in default_installed_adapters() {
                            if !loaded.iter().any(|x| x.id == def.id)
                                && !removed.iter().any(|r| r == &def.id)
                            {
                                loaded.push(def);
                            }
                        }
                        adapters = loaded;
                    }
                }
            }
        }
        adapters.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        AdapterRegistry { path, adapters }
    }

    fn save(&self, removed_defaults: &[String]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create adapters dir: {e}"))?;
        }
        let payload = json!({
            "version": 1,
            "adapters": self.adapters.iter().map(ToolAdapter::to_json).collect::<Vec<_>>(),
            "removed_defaults": removed_defaults,
        });
        let body = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("serialize adapters.json: {e}"))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body.as_bytes()).map_err(|e| format!("write adapters.json: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("commit adapters.json: {e}"))?;
        Ok(())
    }

    fn read_removed_defaults(&self) -> Vec<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("removed_defaults")?.as_array().cloned())
            .map(|a| a.into_iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn list(&self, capability: Option<&str>) -> Vec<ToolAdapter> {
        self.adapters
            .iter()
            .filter(|a| capability.is_none_or(|c| a.capability == c))
            .cloned()
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<ToolAdapter> {
        self.adapters.iter().find(|a| a.id == id).cloned()
    }

    pub fn list_json(&self, store: &SecretStore, capability: Option<&str>) -> Value {
        let data_dir_opt = self.path.parent();
        let adapters: Vec<Value> = self
            .adapters
            .iter()
            .filter(|a| capability.is_none_or(|c| a.capability == c))
            .map(|a| a.view_json_in(data_dir_opt, store))
            .collect();
        let all_presets: Vec<ToolAdapter> = default_installed_adapters()
            .into_iter()
            .chain(optional_preset_adapters())
            .filter(|p| {
                capability.is_none_or(|c| {
                    p.capability == c || (p.id == "playwright-cdp-cli" && c == "custom")
                })
            })
            .collect();
        let presets: Vec<Value> = all_presets
            .iter()
            .map(|p| {
                let mut v = p.view_json_in(data_dir_opt, store);
                let installed = self.adapters.iter().any(|x| x.id == p.id);
                if let Some(o) = v.as_object_mut() {
                    o.insert("installed".into(), json!(installed));
                }
                v
            })
            .collect();
        json!({
            "ok": true,
            "capabilities": CAPABILITIES,
            "adapters": adapters,
            "presets": presets,
            "templates": starter_templates(),
        })
    }

    /// Install a preset (`preset_id`) or create/update a custom adapter, writing
    /// any `secret_values` straight into `SecretStore` (`0600`) and never into
    /// `adapters.json`.
    pub fn upsert(&mut self, store: &SecretStore, body: &Value) -> Result<ToolAdapter, String> {
        if let Some(sec_obj) = body.get("secret_values").and_then(Value::as_object) {
            for (k, v) in sec_obj {
                let norm = normalize_secret_key_name(k)?;
                if let Some(raw) = v.as_str() {
                    let trimmed = raw.trim();
                    if !trimmed.is_empty() {
                        store.set(&norm, trimmed)?;
                    }
                }
            }
        }

        let adapter = if let Some(pid) = body.get("preset_id").and_then(Value::as_str) {
            let pid = pid.trim().to_ascii_lowercase();
            let mut base = default_installed_adapters()
                .into_iter()
                .chain(optional_preset_adapters())
                .find(|p| p.id == pid)
                .ok_or_else(|| format!("unknown preset_id '{pid}'"))?;
            if let Some(en) = body.get("enabled").and_then(Value::as_bool) {
                base.enabled = en;
            }
            if let Some(pr) = body.get("priority").and_then(Value::as_i64) {
                base.priority = pr as i32;
            }
            if let Some(name) = body.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                base.name = name.trim().to_string();
            }
            if let Some(cap) = body.get("capability").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
                if !CAPABILITIES.contains(&cap) {
                    return Err(format!("invalid capability '{cap}'"));
                }
                base.capability = cap.to_string();
            }
            base
        } else {
            // If updating an existing adapter and omitting `cli`/`http` (e.g. toggling
            // `enabled`, bumping `priority`, or updating `secret_values` from the UI),
            // merge onto the existing adapter instead of requiring full transport spec.
            let target_id = body
                .get("id")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_ascii_lowercase());
            let existing = target_id
                .as_deref()
                .and_then(|id| self.adapters.iter().find(|a| a.id == id).cloned());
            if let Some(mut cur) = existing {
                let same_transport = body
                    .get("transport")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .is_none_or(|t| t == cur.transport);
                let is_partial = same_transport
                    && body.get("cli").is_none()
                    && body.get("http").is_none();
                if is_partial {
                    if let Some(en) = body.get("enabled").and_then(Value::as_bool) {
                        cur.enabled = en;
                    }
                    if let Some(pr) = body.get("priority").and_then(Value::as_i64) {
                        cur.priority = pr as i32;
                    }
                    if let Some(name) = body.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                        cur.name = name.trim().to_string();
                    }
                    if let Some(desc) = body.get("description").and_then(Value::as_str) {
                        cur.description = desc.trim().to_string();
                    }
                    if let Some(tier) = body.get("hardware_tier") {
                        cur.hardware_tier = tier
                            .as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                    }
                    if let Some(ms) = body.get("model_size") {
                        cur.model_size = ms
                            .as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                    }
                    if let Some(ih) = body.get("install_hint") {
                        cur.install_hint = ih
                            .as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                    }
                    if let Some(arr) = body.get("required_secrets").and_then(Value::as_array) {
                        let mut reqs = Vec::new();
                        for x in arr {
                            if let Some(s) = x.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                                let norm = normalize_secret_key_name(s)?;
                                if !reqs.contains(&norm) {
                                    reqs.push(norm);
                                }
                            }
                        }
                        cur.required_secrets = reqs;
                    }
                    check_prohibited_provider(&cur)?;
                    cur
                } else {
                    ToolAdapter::from_json(body)?
                }
            } else {
                ToolAdapter::from_json(body)?
            }
        };

        let mut removed = self.read_removed_defaults();
        removed.retain(|r| r != &adapter.id);
        if let Some(existing) = self.adapters.iter_mut().find(|a| a.id == adapter.id) {
            *existing = adapter.clone();
        } else {
            self.adapters.push(adapter.clone());
        }
        self.adapters
            .sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        self.save(&removed)?;
        Ok(adapter)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.adapters.len();
        self.adapters.retain(|a| a.id != id);
        if self.adapters.len() == before {
            return Err(format!("unknown adapter '{id}'"));
        }
        let mut removed = self.read_removed_defaults();
        if default_installed_adapters().iter().any(|d| d.id == id) && !removed.iter().any(|r| r == id) {
            removed.push(id.to_string());
        }
        self.save(&removed)?;
        Ok(())
    }

    /// True if a non-builtin enabled adapter exists for `capability` with higher
    /// priority than all builtin defaults (or matching a specific adapter id).
    pub fn find_custom_override(&self, capability: &str, explicit_provider: Option<&str>) -> Option<ToolAdapter> {
        let explicit = explicit_provider
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"));
        if let Some(p) = explicit {
            if let Some(a) = self
                .adapters
                .iter()
                .find(|a| a.id == p && a.capability == capability && a.enabled && a.transport != "builtin")
            {
                return Some(a.clone());
            }
            return None;
        }
        self.adapters
            .iter()
            .filter(|a| a.capability == capability && a.enabled)
            .min_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)))
            .filter(|a| a.transport != "builtin")
            .cloned()
    }
}

/// If a custom (non-builtin) enabled adapter outranks built-in defaults for `capability`
/// (or matches `explicit_id`), run it (or the priority chain starting from it) and return
/// `Some(result)`. Otherwise return `None` so callers execute their built-in path directly
/// with zero recursion.
pub fn maybe_invoke_override(
    ctx: &InvokeCtx<'_>,
    capability: &str,
    explicit_id: Option<&str>,
    params: &Value,
) -> Option<Result<Value, String>> {
    let reg = AdapterRegistry::load(ctx.data_dir);
    let explicit = explicit_id
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"));
    let target = reg.find_custom_override(capability, explicit)?;
    let pinned = explicit.map(|_| target.id.as_str());
    let mut res = match invoke(ctx, Some(capability), pinned, params) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    // Lift convenience fields (`path`, `bytes`, `sha256`, `mime`, `items`, `name`,
    // `project_path`) from `result` to the top level so existing callers expecting
    // native endpoint shapes work alongside `adapter_id`, `attempts`, and `result`.
    let lifted: Vec<(String, Value)> = res
        .get("result")
        .and_then(Value::as_object)
        .map(|obj| {
            ["path", "bytes", "sha256", "mime", "items", "name", "project_path", "meta_path"]
                .iter()
                .filter_map(|k| obj.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect()
        })
        .unwrap_or_default();
    let adapter_id = res
        .get("adapter_id")
        .and_then(Value::as_str)
        .map(String::from);
    if let Some(map) = res.as_object_mut() {
        for (k, v) in lifted {
            map.entry(k).or_insert(v);
        }
        if let Some(aid) = adapter_id {
            map.entry("provider".to_string()).or_insert(json!(aid));
        }
    }
    Some(Ok(res))
}

/// Test an adapter's availability, secret readiness, and optional health check.
pub fn test_adapter(
    data_dir: &Path,
    store: &SecretStore,
    id: &str,
) -> Result<Value, String> {
    let reg = AdapterRegistry::load(data_dir);
    let adapter = reg
        .get(id)
        .ok_or_else(|| format!("unknown adapter '{id}'"))?;
    let started = Instant::now();
    let mut missing = Vec::new();
    let mut secret_vals = Vec::new();
    for s in &adapter.required_secrets {
        match store.get(s) {
            Some(v) => secret_vals.push(v),
            None => missing.push(s.clone()),
        }
    }
    if !missing.is_empty() {
        return Ok(json!({
            "ok": true,
            "id": adapter.id,
            "available": false,
            "status": "missing_secrets",
            "missing_secrets": missing,
            "details": format!("Add required secret(s) in Settings: {}", missing.join(", ")),
            "latency_ms": started.elapsed().as_millis() as u64,
        }));
    }

    match adapter.transport.as_str() {
        "builtin" => {
            let avail = adapter_endpoint_available_in(Some(data_dir), &adapter);
            Ok(json!({
                "ok": true,
                "id": adapter.id,
                "available": avail,
                "status": if avail { "ready" } else { "unavailable" },
                "missing_secrets": [],
                "details": if avail { "Built-in adapter ready." } else { "Required local tool was not detected on this machine." },
                "latency_ms": started.elapsed().as_millis() as u64,
            }))
        }
        "cli" => {
            let cfg = adapter.cli.as_ref().unwrap();
            let Some(bin) = resolve_adapter_cli_binary_in(Some(data_dir), &adapter.id, &cfg.command) else {
                return Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": false,
                    "status": "binary_not_found",
                    "missing_secrets": [],
                    "details": format!("Binary '{}' was not found on PATH or at that path.", cfg.command),
                    "latency_ms": started.elapsed().as_millis() as u64,
                }));
            };
            if cfg.health_argv.is_empty() {
                return Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": true,
                    "status": "ready",
                    "missing_secrets": [],
                    "details": format!("Found binary at {}", bin.display()),
                    "latency_ms": started.elapsed().as_millis() as u64,
                }));
            }
            let sandbox = data_dir.join("adapter-sandbox");
            let _ = std::fs::create_dir_all(&sandbox);
            match run_cli_raw(&bin, &cfg.health_argv, &BTreeMap::new(), &sandbox, Duration::from_secs(15)) {
                Ok((code, stdout, stderr)) => {
                    let raw = format!("{stdout} {stderr}");
                    let clean = scrub_secrets(raw.trim(), &secret_vals);
                    let preview: String = clean.chars().take(240).collect();
                    Ok(json!({
                        "ok": true,
                        "id": adapter.id,
                        "available": code == 0,
                        "status": if code == 0 { "ready" } else { "health_failed" },
                        "exit_code": code,
                        "missing_secrets": [],
                        "details": preview,
                        "latency_ms": started.elapsed().as_millis() as u64,
                    }))
                }
                Err(e) => Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": false,
                    "status": "health_error",
                    "missing_secrets": [],
                    "details": scrub_secrets(&e, &secret_vals),
                    "latency_ms": started.elapsed().as_millis() as u64,
                })),
            }
        }
        "http" => {
            let cfg = adapter.http.as_ref().unwrap();
            let check_url_tpl = cfg.health_url.as_deref().unwrap_or(&cfg.url_template);
            let dummy = HashMap::new();
            let url = match interpolate_str(check_url_tpl, &dummy, store, &mut secret_vals) {
                Ok(u) => u,
                Err(e) => {
                    return Ok(json!({
                        "ok": true,
                        "id": adapter.id,
                        "available": false,
                        "status": "config_error",
                        "missing_secrets": [],
                        "details": scrub_secrets(&e, &secret_vals),
                        "latency_ms": started.elapsed().as_millis() as u64,
                    }))
                }
            };
            if let Err(e) = validate_public_url(&url, cfg.allow_loopback) {
                return Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": false,
                    "status": "blocked_ssrf",
                    "missing_secrets": [],
                    "details": scrub_secrets(&e, &secret_vals),
                    "latency_ms": started.elapsed().as_millis() as u64,
                }));
            }
            if cfg.health_url.is_none() {
                return Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": true,
                    "status": "ready",
                    "missing_secrets": [],
                    "details": "Endpoint URL validated and required secrets present.",
                    "latency_ms": started.elapsed().as_millis() as u64,
                }));
            }
            let headers = match interpolate_headers(&cfg.headers_template, &dummy, store, &mut secret_vals) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(json!({
                        "ok": true,
                        "id": adapter.id,
                        "available": false,
                        "status": "missing_secrets",
                        "missing_secrets": [],
                        "details": scrub_secrets(&e, &secret_vals),
                        "latency_ms": started.elapsed().as_millis() as u64,
                    }))
                }
            };
            match http_request_checked("GET", &url, cfg.allow_loopback, &headers, None, None, Duration::from_secs(10)) {
                Ok((code, _, _)) => Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": (200..300).contains(&code),
                    "status": if (200..300).contains(&code) { "ready" } else { "health_failed" },
                    "http_status": code,
                    "missing_secrets": [],
                    "details": format!("Health check returned HTTP {code}"),
                    "latency_ms": started.elapsed().as_millis() as u64,
                })),
                Err(e) => Ok(json!({
                    "ok": true,
                    "id": adapter.id,
                    "available": false,
                    "status": "unreachable",
                    "missing_secrets": [],
                    "details": scrub_secrets(&e, &secret_vals),
                    "latency_ms": started.elapsed().as_millis() as u64,
                })),
            }
        }
        other => Err(format!("unsupported transport '{other}'")),
    }
}

/// Invocation context for executing one adapter or a capability fallback chain.
pub struct InvokeCtx<'a> {
    pub data_dir: &'a Path,
    pub assets_root: &'a Path,
    pub store: &'a SecretStore,
    pub project_dir: Option<&'a Path>,
}

/// Invoke either a specific `adapter_id` or the ordered fallback chain for `capability`.
pub fn invoke(
    ctx: &InvokeCtx,
    capability: Option<&str>,
    adapter_id: Option<&str>,
    params: &Value,
) -> Result<Value, String> {
    let reg = AdapterRegistry::load(ctx.data_dir);
    let chain: Vec<ToolAdapter> = if let Some(id) = adapter_id.map(str::trim).filter(|s| !s.is_empty()) {
        let a = reg
            .get(id)
            .ok_or_else(|| format!("unknown adapter '{id}'"))?;
        check_prohibited_provider(&a)?;
        if !a.enabled {
            return Err(format!("adapter '{id}' is disabled"));
        }
        vec![a]
    } else {
        let cap = capability
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("invoke requires either capability or adapter_id")?;
        let mut list: Vec<ToolAdapter> = reg
            .list(Some(cap))
            .into_iter()
            .filter(|a| a.enabled)
            .collect();
        list.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        if list.is_empty() {
            return Err(format!("no enabled adapters registered for capability '{cap}'"));
        }
        list
    };

    let mut attempts = Vec::new();
    for adapter in &chain {
        match invoke_single(ctx, adapter, params) {
            Ok(result) => {
                let status = result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("ok")
                    .to_string();
                attempts.push(json!({
                    "adapter_id": adapter.id,
                    "ok": true,
                    "status": status,
                }));
                return Ok(json!({
                    "ok": true,
                    "capability": adapter.capability,
                    "adapter_id": adapter.id,
                    "attempts": attempts,
                    "result": result,
                }));
            }
            Err(InvokeErr::Terminal(status, msg)) => {
                attempts.push(json!({
                    "adapter_id": adapter.id,
                    "ok": false,
                    "status": status,
                    "error": msg,
                }));
                return Err(format!("{} ({status}): {msg}", adapter.id));
            }
            Err(InvokeErr::Fallback(status, msg)) => {
                attempts.push(json!({
                    "adapter_id": adapter.id,
                    "ok": false,
                    "status": status,
                    "error": msg,
                }));
            }
        }
    }

    let summary: Vec<String> = attempts
        .iter()
        .map(|a| {
            format!(
                "{}: {}",
                a["adapter_id"].as_str().unwrap_or("?"),
                a["error"].as_str().unwrap_or("failed")
            )
        })
        .collect();
    Err(format!(
        "all {} adapter(s) failed: {}",
        attempts.len(),
        summary.join(" | ")
    ))
}

enum InvokeErr {
    /// Do not fall through (e.g. explicit content moderation refusal).
    Terminal(String, String),
    /// Try next adapter in priority order.
    Fallback(String, String),
}

fn invoke_single(ctx: &InvokeCtx, adapter: &ToolAdapter, params: &Value) -> Result<Value, InvokeErr> {
    // Check required secrets first.
    let mut missing = Vec::new();
    for s in &adapter.required_secrets {
        if !ctx.store.has(s) {
            missing.push(s.clone());
        }
    }
    if !missing.is_empty() {
        return Err(InvokeErr::Fallback(
            "missing_secrets".into(),
            format!("missing required secret(s): {}", missing.join(", ")),
        ));
    }

    match adapter.transport.as_str() {
        "builtin" => invoke_builtin(ctx, adapter, params),
        "cli" => invoke_cli(ctx, adapter, params),
        "http" => invoke_http(ctx, adapter, params),
        other => Err(InvokeErr::Fallback("unsupported".into(), format!("unsupported transport '{other}'"))),
    }
}

fn invoke_builtin(ctx: &InvokeCtx, adapter: &ToolAdapter, params: &Value) -> Result<Value, InvokeErr> {
    let proj = ctx.project_dir;
    match adapter.id.as_str() {
        "chatgpt-browser-cli" | "gemini-nano-banana-http" | "openai-images-http" => {
            let root = proj.ok_or_else(|| {
                InvokeErr::Fallback("no_project".into(), "image_gen requires an active project".into())
            })?;
            let mut body = params.clone();
            if let Some(o) = body.as_object_mut() {
                let provider = match adapter.id.as_str() {
                    "chatgpt-browser-cli" => "chatgpt",
                    "gemini-nano-banana-http" => "gemini",
                    _ => "openai",
                };
                o.insert("provider".into(), json!(provider));
            }
            match crate::assets::art_generate(ctx.store, root, &body) {
                Ok(v) => Ok(v),
                Err(oc) => {
                    if oc.msg.to_ascii_lowercase().contains("refused")
                        || oc.msg.to_ascii_lowercase().contains("higgsfield")
                    {
                        Err(InvokeErr::Terminal("refused".into(), oc.msg))
                    } else {
                        Err(InvokeErr::Fallback(format!("http_{}", oc.status), oc.msg))
                    }
                }
            }
        }
        "camoufox-browser-cli" | "builtin-http-search" => {
            let root = proj.ok_or_else(|| {
                InvokeErr::Fallback("no_project".into(), "reference_search requires an active project".into())
            })?;
            let mut body = params.clone();
            if let Some(o) = body.as_object_mut() {
                o.insert(
                    "use_http_fallback".into(),
                    json!(adapter.id == "builtin-http-search"),
                );
            }
            crate::reference_search::search(root, &body)
                .map_err(|oc| InvokeErr::Fallback(format!("http_{}", oc.status), oc.msg))
        }
        "blender-cycles-cli" => {
            if params.get("parts").is_some() || params.get("script").is_some() {
                crate::mesh::generate_for_project(ctx.assets_root, proj, params, None)
                    .map_err(|e| InvokeErr::Fallback("mesh_error".into(), e))
            } else {
                crate::library::generate_blender_mesh(proj.unwrap_or(ctx.assets_root), params, None)
                    .map_err(|e| InvokeErr::Fallback("mesh_error".into(), e))
            }
        }
        "rust-synth-builtin" => {
            crate::audio_synth::generate_audio_asset_for_project(ctx.assets_root, proj, params)
                .map_err(|e| InvokeErr::Fallback("audio_error".into(), e))
        }
        other => Err(InvokeErr::Fallback("unknown_builtin".into(), format!("unknown builtin '{other}'"))),
    }
}

struct BuiltVars {
    vars: HashMap<String, String>,
    out_target: Option<(String, PathBuf)>,
}

fn build_vars(ctx: &InvokeCtx, params: &Value) -> Result<BuiltVars, InvokeErr> {
    let mut vars = HashMap::new();
    let project_str = ctx
        .project_dir
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ctx.data_dir.to_string_lossy().to_string());
    vars.insert("project_dir".into(), project_str);

    // Resolve and strictly confine `out_path` if supplied.
    let mut out_target: Option<(String, PathBuf)> = None;
    if let Some(raw_out) = params.get("out_path").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        let base = ctx.project_dir.unwrap_or(ctx.data_dir);
        let safe = crate::projects::safe_join(base, raw_out)
            .map_err(|e| InvokeErr::Terminal("invalid_path".into(), e))?;
        let rel = crate::projects::rel_display(base, &safe);
        if rel.starts_with(".git/")
            || rel.starts_with(".openro/")
            || rel.starts_with("art/references/")
        {
            return Err(InvokeErr::Terminal(
                "forbidden_path".into(),
                format!("cannot write adapter output to protected path '{rel}'"),
            ));
        }
        if let Some(parent) = safe.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        vars.insert("out_path".into(), safe.to_string_lossy().to_string());
        out_target = Some((rel, safe));
    }

    // Resolve `ref_image` safely inside project when supplied.
    if let Some(raw_ref) = params
        .get("ref_image")
        .or_else(|| params.get("references").and_then(|v| v.as_array()).and_then(|a| a.first()))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        let base = ctx.project_dir.unwrap_or(ctx.data_dir);
        let safe = crate::projects::safe_join(base, raw_ref)
            .map_err(|e| InvokeErr::Terminal("invalid_ref_path".into(), e))?;
        vars.insert("ref_image".into(), safe.to_string_lossy().to_string());
    }

    for key in [
        "prompt",
        "query",
        "width",
        "height",
        "name",
        "card_id",
        "character",
        "show",
        "duration",
        "duration_s",
        "seed",
        "steps",
        "preset",
    ] {
        if let Some(v) = params.get(key) {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            vars.insert(key.to_string(), s);
        }
    }
    // Derive `duration_s` / `duration` automatically from `duration_ms` if not explicitly passed.
    if !vars.contains_key("duration_s") {
        if let Some(d) = vars.get("duration").cloned() {
            vars.insert("duration_s".into(), d);
        } else if let Some(ms) = params.get("duration_ms").and_then(Value::as_f64) {
            if ms.is_finite() && ms > 0.0 {
                let secs = (ms / 1000.0).clamp(0.1, 60.0);
                let formatted = format!("{secs:.2}");
                vars.insert("duration_s".into(), formatted.clone());
                vars.entry("duration".into()).or_insert(formatted);
            }
        }
    } else if !vars.contains_key("duration") {
        if let Some(ds) = vars.get("duration_s").cloned() {
            vars.insert("duration".into(), ds);
        }
    }
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            vars.insert(format!("arg:{k}"), s);
        }
    }
    Ok(BuiltVars { vars, out_target })
}

fn invoke_cli(ctx: &InvokeCtx, adapter: &ToolAdapter, params: &Value) -> Result<Value, InvokeErr> {
    let cfg = adapter.cli.as_ref().unwrap();
    let bin = resolve_adapter_cli_binary_in(Some(ctx.data_dir), &adapter.id, &cfg.command)
        .ok_or_else(|| {
            InvokeErr::Fallback(
                "binary_not_found".into(),
                format!("CLI binary '{}' not found", cfg.command),
            )
        })?;
    let BuiltVars { vars, out_target } = build_vars(ctx, params)?;
    let mut secrets_used = Vec::new();

    // Render strict argv array — NEVER invoke a shell.
    let mut rendered_argv: Vec<String> = Vec::with_capacity(cfg.argv.len());
    for token in &cfg.argv {
        if let Some(opt_key) = token
            .strip_prefix("{{?")
            .and_then(|s| s.strip_suffix("}}"))
        {
            let val = vars.get(opt_key.trim()).cloned().unwrap_or_default();
            if val.is_empty() {
                if rendered_argv.last().is_some_and(|prev| prev.starts_with('-')) {
                    rendered_argv.pop();
                }
                continue;
            }
            rendered_argv.push(val);
            continue;
        }
        let s = interpolate_str(token, &vars, ctx.store, &mut secrets_used)
            .map_err(|e| InvokeErr::Fallback("template_error".into(), scrub_secrets(&e, &secrets_used)))?;
        rendered_argv.push(s);
    }

    let mut env_vars = BTreeMap::new();
    for (env_k, sec_name) in &cfg.env_secrets {
        let Some(val) = ctx.store.get(sec_name) else {
            return Err(InvokeErr::Fallback(
                "missing_secrets".into(),
                format!("missing secret '{sec_name}'"),
            ));
        };
        secrets_used.push(val.clone());
        env_vars.insert(env_k.clone(), val);
    }

    let cwd = match cfg.cwd.as_str() {
        "project" => ctx
            .project_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.data_dir.join("adapter-sandbox")),
        "data_dir" => ctx.data_dir.to_path_buf(),
        _ => ctx.data_dir.join("adapter-sandbox"),
    };
    let _ = std::fs::create_dir_all(&cwd);

    let (code, stdout, stderr) = run_cli_raw(
        &bin,
        &rendered_argv,
        &env_vars,
        &cwd,
        Duration::from_secs(cfg.timeout_secs),
    )
    .map_err(|e| InvokeErr::Fallback("exec_error".into(), scrub_secrets(&e, &secrets_used)))?;

    let clean_out = scrub_secrets(&stdout, &secrets_used);
    let clean_err = scrub_secrets(&stderr, &secrets_used);

    if code != 0 {
        let mapped = cfg
            .exit_code_map
            .get(&code)
            .cloned()
            .unwrap_or_else(|| format!("exit_{code}"));
        // `pending_review` is a structured non-error workflow state (e.g. exit 9 angle review).
        if mapped == "pending_review" {
            let parsed = serde_json::from_str::<Value>(clean_out.trim()).unwrap_or_else(|_| json!({ "stdout": clean_out.trim() }));
            return Ok(json!({
                "status": "pending_review",
                "exit_code": code,
                "data": parsed,
            }));
        }
        let detail = if !clean_err.trim().is_empty() {
            clean_err.trim().chars().take(400).collect::<String>()
        } else {
            clean_out.trim().chars().take(400).collect::<String>()
        };
        if mapped == "refused" {
            return Err(InvokeErr::Terminal(mapped, detail));
        }
        return Err(InvokeErr::Fallback(
            mapped,
            format!("CLI exited with code {code}: {detail}"),
        ));
    }

    match cfg.output_mode.as_str() {
        "file" => {
            let (rel, abs) = out_target.ok_or_else(|| {
                InvokeErr::Fallback("missing_out_path".into(), "output_mode='file' requires params.out_path".into())
            })?;
            let bytes = std::fs::read(&abs).map_err(|e| {
                InvokeErr::Fallback("missing_output_file".into(), format!("CLI succeeded but '{rel}' was not written: {e}"))
            })?;
            Ok(json!({
                "status": "ok",
                "path": rel,
                "bytes": bytes.len(),
                "sha256": crate::projects::sha256_hex(&bytes),
            }))
        }
        "stdout_path" => {
            let reported = clean_out.trim().lines().last().unwrap_or("").trim().to_string();
            if reported.is_empty() {
                return Err(InvokeErr::Fallback("empty_stdout".into(), "CLI printed no output path".into()));
            }
            Ok(json!({ "status": "ok", "path": reported }))
        }
        _ => {
            let parsed: Value = serde_json::from_str(clean_out.trim()).map_err(|e| {
                InvokeErr::Fallback(
                    "invalid_json".into(),
                    format!("CLI stdout was not valid JSON ({e}): {}", clean_out.trim().chars().take(240).collect::<String>()),
                )
            })?;
            let extracted = match &cfg.result_json_path {
                Some(p) => extract_json_path(&parsed, p).cloned().unwrap_or(Value::Null),
                None => parsed,
            };
            Ok(json!({ "status": "ok", "data": extracted }))
        }
    }
}

fn invoke_http(ctx: &InvokeCtx, adapter: &ToolAdapter, params: &Value) -> Result<Value, InvokeErr> {
    let cfg = adapter.http.as_ref().unwrap();
    let BuiltVars { vars, out_target } = build_vars(ctx, params)?;
    let mut secrets_used = Vec::new();
    let timeout = Duration::from_secs(cfg.timeout_secs);

    let url = interpolate_str(&cfg.url_template, &vars, ctx.store, &mut secrets_used)
        .map_err(|e| InvokeErr::Fallback("template_error".into(), scrub_secrets(&e, &secrets_used)))?;
    validate_public_url(&url, cfg.allow_loopback)
        .map_err(|e| InvokeErr::Terminal("blocked_ssrf".into(), scrub_secrets(&e, &secrets_used)))?;
    let headers = interpolate_headers(&cfg.headers_template, &vars, ctx.store, &mut secrets_used)
        .map_err(|e| InvokeErr::Fallback("header_error".into(), scrub_secrets(&e, &secrets_used)))?;

    let (req_ct, req_body) = match cfg.body_mode.as_str() {
        "none" => (None, None),
        "json" => {
            let val = match &cfg.body_template {
                Some(tpl) => interpolate_json(tpl, &vars, ctx.store, &mut secrets_used)
                    .map_err(|e| InvokeErr::Fallback("body_error".into(), scrub_secrets(&e, &secrets_used)))?,
                None => params.clone(),
            };
            (Some("application/json".to_string()), Some(val.to_string().into_bytes()))
        }
        "multipart" => {
            let tpl = cfg.body_template.as_ref().cloned().unwrap_or_else(|| json!({}));
            let rendered = interpolate_json(&tpl, &vars, ctx.store, &mut secrets_used)
                .map_err(|e| InvokeErr::Fallback("body_error".into(), scrub_secrets(&e, &secrets_used)))?;
            let boundary = format!("----OpenROAdapter{:x}", crate::projects::now_ms());
            let mut mp = Vec::new();
            if let Some(fields) = rendered.get("fields").and_then(Value::as_object) {
                for (k, v) in fields {
                    let sv = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    mp.extend_from_slice(
                        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{sv}\r\n").as_bytes(),
                    );
                }
            }
            if let Some(fp) = rendered.get("file_path").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                let field = rendered
                    .get("file_field")
                    .and_then(Value::as_str)
                    .unwrap_or("file");
                let base = ctx.project_dir.unwrap_or(ctx.data_dir);
                let base_canon = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
                let safe_file = if Path::new(fp).is_absolute() {
                    let cand = std::fs::canonicalize(fp).map_err(|e| {
                        InvokeErr::Terminal("invalid_path".into(), format!("multipart file_path invalid: {e}"))
                    })?;
                    if !cand.starts_with(&base_canon) {
                        return Err(InvokeErr::Terminal(
                            "forbidden_path".into(),
                            "multipart file_path escapes active project root".into(),
                        ));
                    }
                    cand
                } else {
                    crate::projects::safe_join(base, fp)
                        .map_err(|e| InvokeErr::Terminal("invalid_path".into(), e))?
                };
                let bytes = std::fs::read(&safe_file).map_err(|e| {
                    InvokeErr::Fallback("ref_read_error".into(), format!("read multipart file '{fp}': {e}"))
                })?;
                let fname = safe_file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("upload.bin");
                mp.extend_from_slice(
                    format!(
                        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{fname}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                mp.extend_from_slice(&bytes);
                mp.extend_from_slice(b"\r\n");
            }
            mp.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            (Some(format!("multipart/form-data; boundary={boundary}")), Some(mp))
        }
        other => {
            return Err(InvokeErr::Fallback("body_mode".into(), format!("unsupported body_mode '{other}'")));
        }
    };

    let (status_code, content_type, mut resp_bytes) = http_request_checked(
        &cfg.method,
        &url,
        cfg.allow_loopback,
        &headers,
        req_ct.as_deref(),
        req_body.as_deref(),
        timeout,
    )
    .map_err(|e| InvokeErr::Fallback("http_error".into(), scrub_secrets(&e, &secrets_used)))?;

    if !(200..300).contains(&status_code) {
        let preview: String = scrub_secrets(&String::from_utf8_lossy(&resp_bytes), &secrets_used)
            .chars()
            .take(300)
            .collect();
        return Err(InvokeErr::Fallback(
            format!("http_{status_code}"),
            format!("HTTP {status_code}: {preview}"),
        ));
    }

    // Optional async polling loop
    let mut final_json: Option<Value> = if cfg.response_extract.mode == "file_bytes" && cfg.poll_spec.is_none() {
        None
    } else {
        serde_json::from_slice::<Value>(&resp_bytes).ok()
    };

    if let Some(poll) = &cfg.poll_spec {
        let initial = final_json.clone().ok_or_else(|| {
            InvokeErr::Fallback("poll_init_error".into(), "async adapter initial response was not JSON".into())
        })?;
        let poll_headers = match &poll.headers_template {
            Some(ht) => interpolate_headers(ht, &vars, ctx.store, &mut secrets_used)
                .map_err(|e| InvokeErr::Fallback("poll_header_error".into(), scrub_secrets(&e, &secrets_used)))?,
            None => headers.clone(),
        };
        let poll_url = interpolate_with_resp(&poll.url_template, &vars, &initial, ctx.store, &mut secrets_used)
            .map_err(|e| InvokeErr::Fallback("poll_url_error".into(), scrub_secrets(&e, &secrets_used)))?;
        validate_public_url(&poll_url, cfg.allow_loopback)
            .map_err(|e| InvokeErr::Terminal("blocked_ssrf".into(), scrub_secrets(&e, &secrets_used)))?;

        let mut polled_done: Option<Value> = None;
        for _ in 0..poll.max_attempts {
            let (pc, _, pbytes) = http_request_checked(
                "GET",
                &poll_url,
                cfg.allow_loopback,
                &poll_headers,
                None,
                None,
                timeout,
            )
            .map_err(|e| InvokeErr::Fallback("poll_http_error".into(), scrub_secrets(&e, &secrets_used)))?;
            if !(200..300).contains(&pc) {
                return Err(InvokeErr::Fallback(
                    format!("poll_http_{pc}"),
                    format!("poll returned HTTP {pc}"),
                ));
            }
            let pval: Value = serde_json::from_slice(&pbytes).map_err(|e| {
                InvokeErr::Fallback("poll_json_error".into(), format!("poll response not JSON: {e}"))
            })?;
            let st = extract_json_path(&pval, &poll.status_path)
                .and_then(Value::as_str)
                .unwrap_or("");
            if poll.done_values.iter().any(|d| d.eq_ignore_ascii_case(st)) {
                polled_done = Some(pval);
                break;
            }
            if poll.fail_values.iter().any(|f| f.eq_ignore_ascii_case(st)) {
                let err_txt = scrub_secrets(&pval.to_string(), &secrets_used);
                return Err(InvokeErr::Fallback(
                    "poll_failed".into(),
                    format!("async job failed ({st}): {}", err_txt.chars().take(300).collect::<String>()),
                ));
            }
            std::thread::sleep(Duration::from_millis(poll.interval_ms));
        }
        let mut done_val = polled_done.ok_or_else(|| {
            InvokeErr::Fallback("poll_timeout".into(), format!("async job did not complete after {} polls", poll.max_attempts))
        })?;
        // If `result_url_path` points to a separate result URL (e.g. fal.ai `response_url`), fetch it!
        if let Some(rup) = &poll.result_url_path {
            if let Some(res_url) = extract_json_path(&done_val, rup).and_then(Value::as_str) {
                validate_public_url(res_url, cfg.allow_loopback)
                    .map_err(|e| InvokeErr::Terminal("blocked_ssrf".into(), scrub_secrets(&e, &secrets_used)))?;
                let (rc, _, rbytes) = http_request_checked(
                    "GET",
                    res_url,
                    cfg.allow_loopback,
                    &poll_headers,
                    None,
                    None,
                    timeout,
                )
                .map_err(|e| InvokeErr::Fallback("result_fetch_error".into(), scrub_secrets(&e, &secrets_used)))?;
                if !(200..300).contains(&rc) {
                    return Err(InvokeErr::Fallback(format!("result_http_{rc}"), format!("result fetch HTTP {rc}")));
                }
                resp_bytes = rbytes;
                if let Ok(v) = serde_json::from_slice::<Value>(&resp_bytes) {
                    done_val = v;
                }
            }
        }
        if let Some(rp) = &poll.result_path {
            if let Some(sub) = extract_json_path(&done_val, rp) {
                done_val = sub.clone();
            }
        }
        final_json = Some(done_val);
    }

    // Extract response according to `response_extract`.
    match cfg.response_extract.mode.as_str() {
        "file_bytes" => {
            if adapter.capability == "audio_gen" {
                let (norm, norm_ct) =
                    maybe_normalize_audio_wav(resp_bytes, &out_target, Some(&content_type));
                write_or_encode_bytes(norm, out_target, norm_ct.as_deref())
            } else {
                write_or_encode_bytes(resp_bytes, out_target, Some(&content_type))
            }
        }
        "audio_b64_path" => {
            let v = final_json.as_ref().ok_or_else(|| {
                InvokeErr::Fallback("not_json".into(), "response was not JSON".into())
            })?;
            let p = cfg
                .response_extract
                .path
                .as_deref()
                .unwrap_or("outputs.0.data");
            let b64 = extract_json_path(v, p).and_then(Value::as_str).ok_or_else(|| {
                InvokeErr::Fallback(
                    "extract_missing".into(),
                    format!("path '{p}' missing in response"),
                )
            })?;
            let clean_b64 = b64.split_once(',').map(|(_, r)| r).unwrap_or(b64);
            let bytes = crate::assets::decode_base64(clean_b64)
                .map_err(|e| InvokeErr::Fallback("base64_error".into(), e))?;
            let (norm, norm_ct) = maybe_normalize_audio_wav(bytes, &out_target, Some("audio/wav"));
            write_or_encode_bytes(norm, out_target, norm_ct.as_deref())
        }
        "image_b64_path" => {
            let v = final_json.as_ref().ok_or_else(|| {
                InvokeErr::Fallback("not_json".into(), "response was not JSON".into())
            })?;
            let p = cfg.response_extract.path.as_deref().unwrap_or("data.0.b64_json");
            let b64 = extract_json_path(v, p)
                .and_then(Value::as_str)
                .ok_or_else(|| InvokeErr::Fallback("extract_missing".into(), format!("path '{p}' missing in response")))?;
            let clean_b64 = b64.split_once(',').map(|(_, r)| r).unwrap_or(b64);
            let bytes = crate::assets::decode_base64(clean_b64)
                .map_err(|e| InvokeErr::Fallback("base64_error".into(), e))?;
            write_or_encode_bytes(bytes, out_target, None)
        }
        "image_url_path" => {
            let v = final_json.as_ref().ok_or_else(|| {
                InvokeErr::Fallback("not_json".into(), "response was not JSON".into())
            })?;
            let p = cfg.response_extract.path.as_deref().unwrap_or("images.0.url");
            let img_url = extract_json_path(v, p)
                .and_then(Value::as_str)
                .ok_or_else(|| InvokeErr::Fallback("extract_missing".into(), format!("path '{p}' missing in response")))?;
            validate_public_url(img_url, cfg.allow_loopback)
                .map_err(|e| InvokeErr::Terminal("blocked_ssrf".into(), scrub_secrets(&e, &secrets_used)))?;
            let (ic, ict, ibytes) = http_request_checked(
                "GET",
                img_url,
                cfg.allow_loopback,
                &BTreeMap::new(),
                None,
                None,
                timeout,
            )
            .map_err(|e| InvokeErr::Fallback("download_error".into(), scrub_secrets(&e, &secrets_used)))?;
            if !(200..300).contains(&ic) {
                return Err(InvokeErr::Fallback(format!("download_http_{ic}"), format!("image download HTTP {ic}")));
            }
            let mut out = write_or_encode_bytes(ibytes, out_target, Some(&ict))?;
            if let Some(o) = out.as_object_mut() {
                o.insert("source_url".into(), json!(img_url));
            }
            Ok(out)
        }
        "items_path" => {
            let v = final_json.as_ref().ok_or_else(|| {
                InvokeErr::Fallback("not_json".into(), "response was not JSON".into())
            })?;
            let items = match cfg.response_extract.path.as_deref() {
                Some(p) => extract_json_path(v, p).cloned().unwrap_or_else(|| json!([])),
                None => v.clone(),
            };
            Ok(json!({ "status": "ok", "items": items }))
        }
        _ => {
            let v = final_json.unwrap_or_else(|| {
                json!({ "text": scrub_secrets(&String::from_utf8_lossy(&resp_bytes), &secrets_used) })
            });
            let data = match cfg.response_extract.path.as_deref() {
                Some(p) => extract_json_path(&v, p).cloned().unwrap_or(Value::Null),
                None => v,
            };
            Ok(json!({ "status": "ok", "data": data }))
        }
    }
}

fn maybe_normalize_audio_wav(
    bytes: Vec<u8>,
    out_target: &Option<(String, PathBuf)>,
    content_type: Option<&str>,
) -> (Vec<u8>, Option<String>) {
    let wants_wav = out_target
        .as_ref()
        .is_some_and(|(_, p)| p.to_string_lossy().to_ascii_lowercase().ends_with(".wav"));
    if !wants_wav {
        return (bytes, content_type.map(String::from));
    }
    let hint_ext = content_type.and_then(|ct| {
        let low = ct.to_ascii_lowercase();
        if low.contains("mpeg") || low.contains("mp3") {
            Some("mp3")
        } else if low.contains("ogg") {
            Some("ogg")
        } else if low.contains("flac") {
            Some("flac")
        } else if low.contains("wav") {
            Some("wav")
        } else {
            None
        }
    });
    match crate::audio_library::normalize_audio_bytes_to_wav(&bytes, hint_ext) {
        Ok(wav) => (wav, Some("audio/wav".to_string())),
        Err(_) => (bytes, content_type.map(String::from)),
    }
}

fn write_or_encode_bytes(
    bytes: Vec<u8>,
    out_target: Option<(String, PathBuf)>,
    content_type: Option<&str>,
) -> Result<Value, InvokeErr> {
    let sha = crate::projects::sha256_hex(&bytes);
    if let Some((rel, abs)) = out_target {
        let tmp = abs.with_extension("openro.tmp");
        std::fs::write(&tmp, &bytes)
            .and_then(|_| std::fs::rename(&tmp, &abs))
            .map_err(|e| InvokeErr::Fallback("write_error".into(), format!("write '{rel}': {e}")))?;
        Ok(json!({
            "status": "ok",
            "path": rel,
            "bytes": bytes.len(),
            "sha256": sha,
            "content_type": content_type,
        }))
    } else {
        Ok(json!({
            "status": "ok",
            "bytes": bytes.len(),
            "sha256": sha,
            "content_type": content_type,
            "data_base64": crate::assets::encode_base64(&bytes),
        }))
    }
}

fn run_cli_raw(
    bin: &Path,
    argv: &[String],
    env_vars: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
) -> Result<(i32, String, String), String> {
    let mut cmd = Command::new(bin);
    cmd.args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn '{}': {e}", bin.display()))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.take(MAX_CLI_STDOUT_BYTES).read_to_string(&mut s);
        s
    });
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.take(MAX_CLI_STDOUT_BYTES).read_to_string(&mut s);
        let _ = tx.send(s);
    });
    match rx.recv_timeout(timeout) {
        Ok(out) => {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
            let err = err_t.join().unwrap_or_default();
            Ok((code, out, err))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("CLI adapter timed out after {}s", timeout.as_secs()))
        }
    }
}

fn http_request_checked(
    method: &str,
    raw_url: &str,
    allow_loopback: bool,
    headers: &BTreeMap<String, String>,
    content_type: Option<&str>,
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<(u16, String, Vec<u8>), String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(timeout.min(Duration::from_secs(10)))
        .timeout_read(timeout)
        .timeout(timeout)
        .build();
    let mut current = validate_public_url(raw_url, allow_loopback)?;
    let initial_origin = url_origin(&current);
    for _ in 0..5 {
        let same_origin = url_origin(&current) == initial_origin;
        let mut req = agent
            .request(method, &current)
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")));
        if let Some(ct) = content_type {
            req = req.set("Content-Type", ct);
        }
        for (k, v) in headers {
            if !same_origin && is_auth_header(k) {
                continue;
            }
            req = req.set(k, v);
        }
        let result = match body {
            Some(bytes) if method != "GET" => req.send_bytes(bytes),
            _ => req.call(),
        };
        let resp = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) if (300..400).contains(&code) => {
                let loc = r
                    .header("Location")
                    .ok_or_else(|| format!("redirect HTTP {code} missing Location"))?;
                let next = resolve_redirect(&current, loc)?;
                current = validate_public_url(&next, allow_loopback)?;
                continue;
            }
            Err(ureq::Error::Status(code, r)) => {
                let ct = r.header("Content-Type").unwrap_or("").to_string();
                let mut buf = Vec::new();
                let _ = r.into_reader().take(256 * 1024).read_to_end(&mut buf);
                return Ok((code, ct, buf));
            }
            Err(e) => return Err(format!("HTTP {method} failed: {e}")),
        };
        let code = resp.status();
        if (300..400).contains(&code) {
            let loc = resp
                .header("Location")
                .ok_or_else(|| format!("redirect HTTP {code} missing Location"))?;
            let next = resolve_redirect(&current, loc)?;
            current = validate_public_url(&next, allow_loopback)?;
            continue;
        }
        let ct = resp
            .header("Content-Type")
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut buf = Vec::new();
        resp.into_reader()
            .take((MAX_HTTP_BYTES as u64) + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read HTTP body: {e}"))?;
        if buf.len() > MAX_HTTP_BYTES {
            return Err(format!("HTTP response exceeded {} MB cap", MAX_HTTP_BYTES / (1024 * 1024)));
        }
        return Ok((code, ct, buf));
    }
    Err("too many HTTP redirects".to_string())
}

fn resolve_redirect(current: &str, loc: &str) -> Result<String, String> {
    let l = loc.trim();
    if l.starts_with("http://") || l.starts_with("https://") {
        return Ok(l.to_string());
    }
    let (scheme, rest) = current
        .split_once("://")
        .ok_or("invalid current URL")?;
    let origin = match rest.find('/') {
        Some(i) => format!("{scheme}://{}", &rest[..i]),
        None => current.to_string(),
    };
    if l.starts_with("//") {
        Ok(format!("{scheme}:{l}"))
    } else if l.starts_with('/') {
        Ok(format!("{origin}{l}"))
    } else {
        Ok(format!("{origin}/{l}"))
    }
}

fn interpolate_str(
    template: &str,
    vars: &HashMap<String, String>,
    store: &SecretStore,
    secrets_used: &mut Vec<String>,
) -> Result<String, String> {
    interpolate_with_resp(template, vars, &Value::Null, store, secrets_used)
}

fn interpolate_with_resp(
    template: &str,
    vars: &HashMap<String, String>,
    resp: &Value,
    store: &SecretStore,
    secrets_used: &mut Vec<String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err("unclosed template placeholder '{{'".to_string());
        };
        let key = after[..end].trim();
        if let Some(sec_name) = key.strip_prefix("secret:") {
            let sec_name = normalize_secret_key_name(sec_name)?;
            let val = store
                .get(&sec_name)
                .ok_or_else(|| format!("missing required secret '{sec_name}'"))?;
            secrets_used.push(val.clone());
            out.push_str(&val);
        } else if let Some(resp_path) = key.strip_prefix("resp:") {
            let val = extract_json_path(resp, resp_path.trim())
                .ok_or_else(|| format!("poll template placeholder '{{{{resp:{resp_path}}}}}' not found in response"))?;
            match val {
                Value::String(s) => out.push_str(s),
                Value::Number(n) => out.push_str(&n.to_string()),
                Value::Bool(b) => out.push_str(&b.to_string()),
                other => out.push_str(&other.to_string()),
            }
        } else {
            let lookup = key.strip_prefix('?').unwrap_or(key);
            if let Some(val) = vars.get(lookup) {
                out.push_str(val);
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn interpolate_headers(
    headers: &BTreeMap<String, String>,
    vars: &HashMap<String, String>,
    store: &SecretStore,
    secrets_used: &mut Vec<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in headers {
        out.insert(k.clone(), interpolate_str(v, vars, store, secrets_used)?);
    }
    Ok(out)
}

fn interpolate_json(
    val: &Value,
    vars: &HashMap<String, String>,
    store: &SecretStore,
    secrets_used: &mut Vec<String>,
) -> Result<Value, String> {
    match val {
        Value::String(s) => Ok(Value::String(interpolate_str(s, vars, store, secrets_used)?)),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(interpolate_json(item, vars, store, secrets_used)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), interpolate_json(v, vars, store, secrets_used)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// Extract a value via either JSON pointer (`/images/0/url`) or dotted path (`images.0.url`).
pub fn extract_json_path<'a>(val: &'a Value, path: &str) -> Option<&'a Value> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Some(val);
    }
    if trimmed.starts_with('/') {
        return val.pointer(trimmed);
    }
    let mut cur = val;
    for seg in trimmed.split('.') {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => {
                let idx = seg.parse::<usize>().ok()?;
                a.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

pub fn scrub_secrets(text: &str, secret_values: &[String]) -> String {
    let mut out = text.to_string();
    for sec in secret_values {
        if !sec.is_empty() {
            out = out.replace(sec, "[REDACTED]");
        }
    }
    out
}

fn url_origin(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        let auth = rest.split('/').next().unwrap_or("");
        return format!("{}://{}", scheme.to_ascii_lowercase(), auth.to_ascii_lowercase());
    }
    String::new()
}

fn is_auth_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "xi-api-key"
            | "x-goog-api-key"
            | "api-key"
            | "cookie"
    )
}

/// Request routing context for `/api/adapters*`.
pub struct RouteCtx<'a> {
    pub data_dir: &'a Path,
    pub assets_root: &'a Path,
    pub store: &'a SecretStore,
    pub project_resolver: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub active_project: Option<PathBuf>,
}

/// Server-side HTTP router for `/api/adapters*` (loopback-only).
pub fn route(
    ctx: &RouteCtx<'_>,
    method: &str,
    path: &str,
    query: &str,
    body: &Value,
) -> Option<(u16, Value)> {
    if path == "/api/adapters" && method == "GET" {
        let cap = crate::project_api::query_param(query, "capability");
        let reg = AdapterRegistry::load(ctx.data_dir);
        return Some((200, reg.list_json(ctx.store, cap.as_deref())));
    }
    if path == "/api/adapters" && method == "POST" {
        let mut reg = AdapterRegistry::load(ctx.data_dir);
        return Some(match reg.upsert(ctx.store, body) {
            Ok(a) => (
                200,
                json!({ "ok": true, "adapter": a.view_json_in(Some(ctx.data_dir), ctx.store) }),
            ),
            Err(e) => (400, json!({ "ok": false, "error": e })),
        });
    }
    if path == "/api/adapters/invoke" && method == "POST" {
        let cap = body.get("capability").and_then(Value::as_str);
        let id = body.get("adapter_id").and_then(Value::as_str);
        let params = body.get("params").unwrap_or(body);
        let proj = body
            .get("project")
            .or_else(|| body.get("project_id"))
            .and_then(Value::as_str)
            .and_then(ctx.project_resolver)
            .or_else(|| ctx.active_project.clone());
        let ictx = InvokeCtx {
            data_dir: ctx.data_dir,
            assets_root: ctx.assets_root,
            store: ctx.store,
            project_dir: proj.as_deref(),
        };
        return Some(match invoke(&ictx, cap, id, params) {
            Ok(v) => (200, v),
            Err(e) => {
                let code = if e.contains("unknown adapter") {
                    404
                } else if e.contains("blocked_ssrf") || e.contains("prohibited") || e.contains("invalid") {
                    400
                } else {
                    502
                };
                (code, json!({ "ok": false, "error": e }))
            }
        });
    }
    if let Some(rest) = path.strip_prefix("/api/adapters/") {
        if let Some(id) = rest.strip_suffix("/test") {
            if method == "POST" && !id.contains('/') {
                return Some(match test_adapter(ctx.data_dir, ctx.store, id) {
                    Ok(v) => (200, v),
                    Err(e) => (404, json!({ "ok": false, "error": e })),
                });
            }
        } else if !rest.contains('/') && method == "DELETE" {
            let mut reg = AdapterRegistry::load(ctx.data_dir);
            return Some(match reg.delete(rest) {
                Ok(()) => (200, json!({ "ok": true, "id": rest, "deleted": true })),
                Err(e) => (404, json!({ "ok": false, "error": e })),
            });
        }
    }
    None
}
