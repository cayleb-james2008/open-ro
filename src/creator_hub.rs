//! Browser-backed Roblox Creator Hub automation over local loopback CDP (#68).
//!
//! Connects ONLY to a local loopback Chrome/Chromium DevTools endpoint
//! (`http://127.0.0.1:<port>` or `http://localhost:<port>`), opens a dedicated
//! Creator Hub tab (never hijacking existing user tabs), never reads
//! `.ROBLOSECURITY` cookies, never prints/returns raw Open Cloud API keys, and
//! never attempts to bypass login, 2FA, OTP, passkeys, or CAPTCHA challenges.

use crate::opencloud::{self, Ctx, Oc, Reply, CREATOR_SECRET};
use crate::secrets::{self, OPENCLOUD_KEY};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_CDP_URL: &str = "http://127.0.0.1:9222";
const CDP_SCRIPT: &str = include_str!("creator_hub_cdp.py");

fn bad<T>(msg: impl Into<String>) -> Result<T, Oc> {
    Err(Oc { status: 400, msg: msg.into() })
}

fn err<T>(status: u16, msg: impl Into<String>) -> Result<T, Oc> {
    Err(Oc { status, msg: msg.into() })
}

/// Resolve default loopback CDP URL:
/// `OPENRO_BROWSER_CDP_URL` -> local Chrome/Chromium `DevToolsActivePort` -> `http://127.0.0.1:9222`.
pub fn default_cdp_url() -> String {
    if let Ok(v) = std::env::var("OPENRO_BROWSER_CDP_URL") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(port) = discover_devtools_active_port() {
        return format!("http://127.0.0.1:{port}");
    }
    DEFAULT_CDP_URL.to_string()
}

fn discover_devtools_active_port() -> Option<u16> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        candidates.push(h.join(".config/google-chrome/DevToolsActivePort"));
        candidates.push(h.join(".config/chromium/DevToolsActivePort"));
        candidates.push(h.join(".config/BraveSoftware/Brave-Browser/DevToolsActivePort"));
        candidates.push(h.join("Library/Application Support/Google/Chrome/DevToolsActivePort"));
        candidates.push(h.join("Library/Application Support/Chromium/DevToolsActivePort"));
    }
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(local_app);
        candidates.push(base.join("Google/Chrome/User Data/DevToolsActivePort"));
        candidates.push(base.join("Chromium/User Data/DevToolsActivePort"));
    }
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(port) = text
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u16>().ok())
            .filter(|p| *p > 0)
        {
            return Some(port);
        }
    }
    None
}

/// Strictly validate a CDP endpoint URL: loopback `http://` ONLY
/// (`127.0.0.1:<port>`, `localhost:<port>`, or `[::1]:<port>`).
pub fn validate_loopback_cdp_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
        || trimmed.contains(['@', '?', '#'])
    {
        return Err("CDP URL must be a plain loopback URL like http://127.0.0.1:9222.".to_string());
    }
    let Some(rest) = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("HTTP://"))
    else {
        return Err("CDP URL must use http:// on loopback (e.g. http://127.0.0.1:9222).".to_string());
    };
    let authority = rest.trim_end_matches('/');
    if authority.is_empty() || authority.contains('/') {
        return Err("CDP URL must not contain a path (expected http://127.0.0.1:<port>).".to_string());
    }
    let (host, port_str) = if let Some(after) = authority.strip_prefix("[::1]:") {
        ("[::1]", after)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h, p)
    } else {
        return Err("CDP URL must include an explicit port (e.g. http://127.0.0.1:9222).".to_string());
    };
    let host_low = host.to_ascii_lowercase();
    if !matches!(host_low.as_str(), "127.0.0.1" | "localhost" | "[::1]") {
        return Err(format!(
            "CDP URL host '{host}' is not loopback; only 127.0.0.1, localhost, or [::1] are allowed."
        ));
    }
    let port: u16 = port_str
        .parse()
        .ok()
        .filter(|p: &u16| *p > 0)
        .ok_or_else(|| format!("CDP URL port '{port_str}' is invalid (expected 1..65535)."))?;
    Ok(format!("http://{host_low}:{port}"))
}

fn validate_cidr(raw: &str) -> Result<String, Oc> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || matches!(b, b'.' | b':' | b'/'))
    {
        return bad("cidr must be a valid IPv4/IPv6 CIDR like 0.0.0.0/0.");
    }
    Ok(trimmed.to_string())
}

/// Atomic `0600` temp file inside `SecretStore::dir()` for safe secret handoff from Python -> Rust.
struct TempKeyFile {
    path: PathBuf,
}

impl TempKeyFile {
    fn create_in(dir: &Path) -> Result<Self, Oc> {
        std::fs::create_dir_all(dir)
            .map_err(|e| Oc { status: 500, msg: format!("create secrets dir: {e}") })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        for attempt in 0..16u32 {
            let path = dir.join(format!(".creator-hub-key-{pid}-{nanos}-{attempt}.tmp"));
            #[cfg(unix)]
            let opened = {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
            };
            #[cfg(not(unix))]
            let opened = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path);
            match opened {
                Ok(_) => return Ok(TempKeyFile { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return err(500, format!("create private key handoff file: {e}"));
                }
            }
        }
        err(500, "could not allocate unique private key handoff file")
    }

    fn read_trimmed(&self) -> Option<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

impl Drop for TempKeyFile {
    fn drop(&mut self) {
        if let Ok(meta) = std::fs::metadata(&self.path) {
            let zeros = vec![0u8; meta.len().min(4096) as usize];
            let _ = std::fs::write(&self.path, &zeros);
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn ensure_cdp_script(data_dir: &Path) -> Result<PathBuf, Oc> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| Oc { status: 500, msg: format!("create data dir: {e}") })?;
    let path = data_dir.join("creator_hub_cdp.py");
    let up_to_date = std::fs::read_to_string(&path).ok().as_deref() == Some(CDP_SCRIPT);
    if !up_to_date {
        let tmp = data_dir.join(format!(".creator_hub_cdp.{}.tmp", std::process::id()));
        std::fs::write(&tmp, CDP_SCRIPT.as_bytes())
            .map_err(|e| Oc { status: 500, msg: format!("write creator_hub_cdp.py: {e}") })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700));
        }
        std::fs::rename(&tmp, &path)
            .map_err(|e| Oc { status: 500, msg: format!("install creator_hub_cdp.py: {e}") })?;
    }
    Ok(path)
}

fn probe_cdp_version(cdp_url: &str) -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .redirects(0)
        .build();
    let resp = agent.get(&format!("{cdp_url}/json/version")).call().ok()?;
    let mut buf = Vec::new();
    let _ = resp.into_reader().take(256 * 1024).read_to_end(&mut buf);
    let v: Value = serde_json::from_slice(&buf).ok()?;
    Some(
        v.get("Browser")
            .and_then(Value::as_str)
            .unwrap_or("Chromium")
            .to_string(),
    )
}

fn mock_cmd_override() -> Option<String> {
    std::env::var("OPENRO_CREATOR_HUB_CMD")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn python_bin() -> String {
    std::env::var("OPENRO_CREATOR_HUB_PYTHON")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "python".to_string()
            } else {
                "python3".to_string()
            }
        })
}

fn run_cdp_helper(
    data_dir: &Path,
    action: &str,
    cdp_url: &str,
    key_out: Option<&Path>,
    check_login: bool,
    stdin_spec: &Value,
    timeout: Duration,
) -> Result<Value, Oc> {
    let mut cmd = if let Some(override_cmd) = mock_cmd_override() {
        let mut c = Command::new(override_cmd);
        c.args(["--action", action, "--cdp-url", cdp_url]);
        c
    } else {
        let script = ensure_cdp_script(data_dir)?;
        let mut c = Command::new(python_bin());
        c.arg(&script);
        c.args(["--action", action, "--cdp-url", cdp_url]);
        c
    };
    if let Some(ko) = key_out {
        cmd.arg("--key-out").arg(ko);
    }
    if check_login {
        cmd.arg("--check-login");
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| Oc { status: 502, msg: format!("could not launch Creator Hub browser helper: {e}") })?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = stdin_spec.to_string();
        let _ = stdin.write_all(payload.as_bytes());
    }

    let stdout_handle = child.stdout.take().map(|r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.take(4 * 1024 * 1024).read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.take(1024 * 1024).read_to_end(&mut buf);
            buf
        })
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return err(
                        504,
                        format!("Creator Hub browser automation timed out after {}s.", timeout.as_secs()),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return err(502, format!("Creator Hub helper wait failed: {e}")),
        }
    };

    let stdout_bytes = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr_bytes = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stdout_text = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes).trim().to_string();

    let parsed = serde_json::from_str::<Value>(&stdout_text).ok().or_else(|| {
        stdout_text
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
    });

    if let Some(val) = parsed {
        return Ok(val);
    }
    if !status.success() {
        let detail = if !stderr_text.is_empty() {
            stderr_text.chars().take(400).collect::<String>()
        } else {
            stdout_text.chars().take(400).collect::<String>()
        };
        return err(
            502,
            format!("Creator Hub helper exited with {status}: {detail}"),
        );
    }
    err(502, "Creator Hub helper returned non-JSON output.")
}

fn scrub_value(val: &mut Value, secrets_to_scrub: &[&str]) {
    if secrets_to_scrub.is_empty() {
        return;
    }
    match val {
        Value::String(s) => {
            *s = secrets::scrub(s, secrets_to_scrub);
        }
        Value::Array(arr) => {
            for item in arr {
                scrub_value(item, secrets_to_scrub);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                scrub_value(v, secrets_to_scrub);
            }
        }
        _ => {}
    }
}

fn save_creator_from_user(ctx: &Ctx, user: Option<&Value>) {
    let Some(uid) = user
        .and_then(|u| opencloud::id_of(u, "id"))
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
    else {
        return;
    };
    let _ = ctx.store.set(CREATOR_SECRET, &format!("user:{uid}"));
}

fn json_id_value(s: &str) -> Value {
    s.parse::<u64>()
        .map(|n| json!(n))
        .unwrap_or_else(|_| json!(s))
}

/// Probe local browser CDP availability, logged-in Creator Hub user, and existing experiences.
pub fn status(
    ctx: &Ctx,
    cdp_url_opt: Option<&str>,
    check_login: bool,
    save_creator: bool,
) -> Result<Value, Oc> {
    let raw_url = cdp_url_opt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(default_cdp_url);
    let cdp_url = validate_loopback_cdp_url(&raw_url).map_err(|msg| Oc { status: 400, msg })?;

    let mut result = if mock_cmd_override().is_some() {
        run_cdp_helper(
            ctx.data_dir,
            "status",
            &cdp_url,
            None,
            check_login,
            &json!({ "check_login": check_login, "save_creator": save_creator }),
            Duration::from_secs(30),
        )?
    } else {
        let Some(browser_version) = probe_cdp_version(&cdp_url) else {
            return Ok(json!({
                "ok": true,
                "cdp_url": cdp_url,
                "cdp_reachable": false,
                "browser_version": Value::Null,
                "logged_in": false,
                "error": format!("No browser with remote debugging found at {cdp_url}. Start Chrome/Chromium with --remote-debugging-port and sign in to create.roblox.com."),
            }));
        };
        if !check_login {
            return Ok(json!({
                "ok": true,
                "cdp_url": cdp_url,
                "cdp_reachable": true,
                "browser_version": browser_version,
                "creator": opencloud::creator_json(&ctx.store),
            }));
        }
        let mut r = run_cdp_helper(
            ctx.data_dir,
            "status",
            &cdp_url,
            None,
            true,
            &json!({ "check_login": true }),
            Duration::from_secs(45),
        )?;
        if r.get("browser_version").and_then(Value::as_str).unwrap_or("").is_empty() {
            r["browser_version"] = json!(browser_version);
        }
        r
    };

    if save_creator {
        save_creator_from_user(ctx, result.get("user"));
    }
    if result.is_object() {
        result["cdp_url"] = json!(cdp_url);
        if result.get("cdp_reachable").is_none() {
            result["cdp_reachable"] = json!(true);
        }
        result["creator"] = opencloud::creator_json(&ctx.store);
    }
    Ok(result)
}

/// Auto-link experience IDs (`universeId`/`placeId`) into `openro.project.json` and provision
/// an Open Cloud API key directly into the `0600` `SecretStore` without ever exposing the key.
pub fn setup(ctx: &Ctx, project_root: Option<&Path>, body: &Value) -> Result<Value, Oc> {
    let raw_url = body
        .get("cdp_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(default_cdp_url);
    let cdp_url = validate_loopback_cdp_url(&raw_url).map_err(|msg| Oc { status: 400, msg })?;

    let cidr = match body.get("cidr").and_then(Value::as_str) {
        Some(c) if !c.trim().is_empty() => validate_cidr(c)?,
        _ => "0.0.0.0/0".to_string(),
    };
    let key_name = body
        .get("key_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("OpenRO Auto Key")
        .chars()
        .take(64)
        .collect::<String>();
    let description = body
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Provisioned by OpenRO for local project publishing")
        .chars()
        .take(200)
        .collect::<String>();
    let ensure_experience = body
        .get("ensure_experience")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let force_new_key = body
        .get("force_new_key")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reuse_universe_id = opencloud::id_of(body, "reuse_universe_id");
    let manifest = project_root.map(opencloud::read_manifest);
    let existing_universe_id = manifest
        .as_ref()
        .and_then(|m| opencloud::id_of(m, "universeId"));
    let existing_place_id = manifest
        .as_ref()
        .and_then(|m| opencloud::id_of(m, "placeId"));
    let experience_name = body
        .get("experience_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        });
    let scopes: Option<Vec<String>> = body.get("scopes").and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let existing_key = ctx.store.get(OPENCLOUD_KEY);
    let need_key = force_new_key || existing_key.is_none();

    if mock_cmd_override().is_none() && probe_cdp_version(&cdp_url).is_none() {
        return err(
            503,
            format!("No browser with remote debugging found at {cdp_url}. Start Chrome/Chromium with --remote-debugging-port and sign in to create.roblox.com."),
        );
    }

    let key_handoff = TempKeyFile::create_in(ctx.store.dir())?;
    let mut spec = json!({
        "need_key": need_key,
        "ensure_experience": ensure_experience,
        "key_name": key_name,
        "description": description,
        "cidr": cidr,
    });
    if let Some(uid) = &reuse_universe_id {
        spec["reuse_universe_id"] = json!(uid);
    }
    if let Some(uid) = &existing_universe_id {
        spec["existing_universe_id"] = json!(uid);
    }
    if let Some(name) = &experience_name {
        spec["experience_name"] = json!(name);
    }
    if let Some(sc) = &scopes {
        spec["scopes"] = json!(sc);
    }

    let raw_res = run_cdp_helper(
        ctx.data_dir,
        "setup",
        &cdp_url,
        Some(&key_handoff.path),
        false,
        &spec,
        Duration::from_secs(90),
    );

    // Read handoff key immediately and drop TempKeyFile so the temp secret file is wiped now.
    let new_key = key_handoff.read_trimmed();
    drop(key_handoff);

    let mut scrub_list: Vec<String> = Vec::new();
    if let Some(k) = &new_key {
        scrub_list.push(k.clone());
    }
    if let Some(k) = &existing_key {
        scrub_list.push(k.clone());
    }
    let scrub_refs: Vec<&str> = scrub_list.iter().map(String::as_str).collect();

    let mut helper_val = match raw_res {
        Ok(v) => v,
        Err(e) => return err(e.status, secrets::scrub(&e.msg, &scrub_refs)),
    };
    scrub_value(&mut helper_val, &scrub_refs);

    save_creator_from_user(ctx, helper_val.get("user"));

    let mut key_provisioned = false;
    if let Some(ref key) = new_key {
        if key.len() < 12 || key.chars().any(char::is_whitespace) {
            return err(
                502,
                "Creator Hub returned an invalid API key format; refusing to save.",
            );
        }
        ctx.store
            .set(OPENCLOUD_KEY, key)
            .map_err(|e| Oc { status: 500, msg: secrets::scrub(&e, &scrub_refs) })?;
        key_provisioned = true;
    }

    let resolved_uid = opencloud::id_of(&helper_val, "universeId")
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
        .or(existing_universe_id);
    let resolved_pid = opencloud::id_of(&helper_val, "placeId")
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
        .or(existing_place_id);

    if let Some(root) = project_root {
        if resolved_uid.is_some() || resolved_pid.is_some() {
            opencloud::update_manifest(root, |m| {
                if let Some(ref uid) = resolved_uid {
                    m.insert("universeId".into(), json_id_value(uid));
                }
                if let Some(ref pid) = resolved_pid {
                    m.insert("placeId".into(), json_id_value(pid));
                }
            })?;
        }
    }

    let current_key = ctx.store.get(OPENCLOUD_KEY);
    let has_key = current_key.is_some();
    let key_masked = current_key.as_deref().map(secrets::mask);
    let status_str = helper_val
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_action_required = status_str == "action_required"
        || helper_val
            .get("blocked_reason")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());

    if is_action_required {
        let mut out = json!({
            "ok": false,
            "status": "action_required",
            "blocked_reason": helper_val.get("blocked_reason").cloned().unwrap_or(json!("action_required")),
            "url": helper_val.get("url").cloned().unwrap_or(Value::Null),
            "message": helper_val.get("message").cloned().unwrap_or(json!("Complete the prompt in the open Creator Hub tab, then retry.")),
            "has_key": has_key,
            "key_masked": key_masked,
            "creator": opencloud::creator_json(&ctx.store),
            "universeId": resolved_uid,
            "placeId": resolved_pid,
            "experience_name": helper_val.get("experience_name").cloned().unwrap_or(Value::Null),
        });
        scrub_value(&mut out, &scrub_refs);
        return Ok(out);
    }

    if helper_val.get("ok").and_then(Value::as_bool) == Some(false) {
        let msg = helper_val
            .get("error")
            .or_else(|| helper_val.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Creator Hub setup failed.");
        return err(502, secrets::scrub(msg, &scrub_refs));
    }

    if need_key && !has_key {
        return err(
            502,
            "Creator Hub setup completed without delivering an Open Cloud API key.",
        );
    }

    let mut out = json!({
        "ok": true,
        "status": "ready",
        "has_key": has_key,
        "key_masked": key_masked,
        "key_provisioned": key_provisioned,
        "creator": opencloud::creator_json(&ctx.store),
        "user": helper_val.get("user").cloned().unwrap_or(Value::Null),
        "universeId": resolved_uid,
        "placeId": resolved_pid,
        "experience_name": helper_val.get("experience_name").cloned().unwrap_or(Value::Null),
        "experiences": helper_val.get("experiences").cloned().unwrap_or_else(|| json!([])),
    });
    scrub_value(&mut out, &scrub_refs);
    Ok(out)
}

/// Automate Creator Hub dashboard-only configuration (Private Servers, Game Icon, Thumbnails,
/// optional visibility/name/description, and Content Rating questionnaire check) for a project.
pub fn configure(ctx: &Ctx, project_root: &Path, body: &Value) -> Result<Value, Oc> {
    let raw_url = body
        .get("cdp_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(default_cdp_url);
    let cdp_url = validate_loopback_cdp_url(&raw_url).map_err(|msg| Oc { status: 400, msg })?;

    let manifest = opencloud::read_manifest(project_root);
    let universe_id = opencloud::id_of(body, "universeId")
        .or_else(|| opencloud::id_of(body, "universe_id"))
        .or_else(|| opencloud::id_of(&manifest, "universeId"))
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| Oc {
            status: 400,
            msg: "This project has no numeric universeId yet. Run Creator Hub setup or set universeId first.".into(),
        })?;
    let place_id = opencloud::id_of(body, "placeId")
        .or_else(|| opencloud::id_of(body, "place_id"))
        .or_else(|| opencloud::id_of(&manifest, "placeId"))
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()));

    let icon_abs = if let Some(rel) = body
        .get("icon_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if place_id.is_none() {
            return bad("icon_path requires a numeric placeId in openro.project.json or request.");
        }
        let abs = opencloud::safe_rel(project_root, rel)?;
        if !abs.is_file() {
            return bad(format!("icon_path '{rel}' does not exist in the project."));
        }
        Some(abs.to_string_lossy().to_string())
    } else {
        None
    };

    let mut thumb_abs_list: Vec<String> = Vec::new();
    if let Some(arr) = body.get("thumbnail_paths").and_then(Value::as_array) {
        if !arr.is_empty() && place_id.is_none() {
            return bad("thumbnail_paths requires a numeric placeId in openro.project.json or request.");
        }
        for item in arr {
            let Some(rel) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                return bad("thumbnail_paths entries must be non-empty relative path strings.");
            };
            let abs = opencloud::safe_rel(project_root, rel)?;
            if !abs.is_file() {
                return bad(format!("thumbnail path '{rel}' does not exist in the project."));
            }
            thumb_abs_list.push(abs.to_string_lossy().to_string());
        }
    }

    let visibility = if let Some(vis) = body
        .get("visibility")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let up = vis.to_ascii_uppercase();
        if up != "PUBLIC" && up != "PRIVATE" {
            return bad("visibility must be PUBLIC or PRIVATE.");
        }
        Some(up)
    } else {
        None
    };

    if mock_cmd_override().is_none() && probe_cdp_version(&cdp_url).is_none() {
        return err(
            503,
            format!("No browser with remote debugging found at {cdp_url}. Start Chrome/Chromium with --remote-debugging-port and sign in to create.roblox.com."),
        );
    }

    let check_questionnaire = body
        .get("check_questionnaire")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut spec = json!({
        "universeId": universe_id,
        "placeId": place_id,
        "check_questionnaire": check_questionnaire,
    });
    if let Some(ps) = body.get("private_servers").filter(|v| v.is_object()) {
        spec["private_servers"] = ps.clone();
    }
    if let Some(ip) = icon_abs {
        spec["icon_abs_path"] = json!(ip);
    }
    if !thumb_abs_list.is_empty() {
        spec["thumbnail_abs_paths"] = json!(thumb_abs_list);
    }
    if let Some(vis) = visibility {
        spec["visibility"] = json!(vis);
    }
    if let Some(name) = body.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        spec["name"] = json!(name);
    }
    if let Some(desc) = body.get("description").and_then(Value::as_str) {
        spec["description"] = json!(desc);
    }

    let existing_key = ctx.store.get(OPENCLOUD_KEY);
    let scrub_refs: Vec<&str> = existing_key.iter().map(String::as_str).collect();

    let mut helper_val = match run_cdp_helper(
        ctx.data_dir,
        "configure",
        &cdp_url,
        None,
        false,
        &spec,
        Duration::from_secs(90),
    ) {
        Ok(v) => v,
        Err(e) => return err(e.status, secrets::scrub(&e.msg, &scrub_refs)),
    };
    scrub_value(&mut helper_val, &scrub_refs);

    let is_action_required = helper_val.get("status").and_then(Value::as_str) == Some("action_required")
        || helper_val
            .get("blocked_reason")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());

    if is_action_required {
        return Ok(json!({
            "ok": false,
            "status": "action_required",
            "blocked_reason": helper_val.get("blocked_reason").cloned().unwrap_or(json!("action_required")),
            "url": helper_val.get("url").cloned().unwrap_or(Value::Null),
            "message": helper_val.get("message").cloned().unwrap_or(json!("Complete the prompt in the open Creator Hub tab, then retry.")),
            "universeId": universe_id,
            "placeId": place_id,
            "configured": helper_val.get("configured").cloned().unwrap_or_else(|| json!([])),
            "manual_steps_remaining": helper_val.get("manual_steps_remaining").cloned().unwrap_or_else(|| json!([])),
        }));
    }

    if helper_val.get("ok").and_then(Value::as_bool) == Some(false) {
        let msg = helper_val
            .get("error")
            .or_else(|| helper_val.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Creator Hub configure failed.");
        return err(502, secrets::scrub(msg, &scrub_refs));
    }

    Ok(json!({
        "ok": true,
        "status": "ready",
        "universeId": universe_id,
        "placeId": place_id,
        "configured": helper_val.get("configured").cloned().unwrap_or_else(|| json!([])),
        "manual_steps_remaining": helper_val.get("manual_steps_remaining").cloned().unwrap_or_else(|| json!([])),
    }))
}

fn truthy_query(query: &str, key: &str) -> bool {
    matches!(
        opencloud::query_param(query, key)
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// HTTP route dispatcher for `/api/creator-hub/*` and `/api/projects/:id/creator-hub/*`.
pub fn route(ctx: &Ctx, method: &str, path: &str, query: &str, body: &Value) -> Option<Reply> {
    let rest = path.strip_prefix("/api/")?;
    let segs: Vec<&str> = rest.split('/').collect();
    let res: Result<Value, Oc> = match (method, segs.as_slice()) {
        ("GET", ["creator-hub", "status"]) => {
            let cdp_url = opencloud::query_param(query, "cdp_url");
            let check_login = truthy_query(query, "check_login");
            let save_creator = truthy_query(query, "save_creator");
            status(ctx, cdp_url.as_deref(), check_login, save_creator)
        }
        ("POST", ["creator-hub", "setup"]) => setup(ctx, ctx.active_project.as_deref(), body),
        ("POST", ["projects", id, "creator-hub", "setup"]) => {
            let Some(root) = (ctx.resolve)(id) else {
                return Some(Reply::Json(404, json!({ "ok": false, "error": "Unknown project." })));
            };
            setup(ctx, Some(&root), body)
        }
        ("POST", ["projects", id, "creator-hub", "configure"]) => {
            let Some(root) = (ctx.resolve)(id) else {
                return Some(Reply::Json(404, json!({ "ok": false, "error": "Unknown project." })));
            };
            configure(ctx, &root, body)
        }
        (_, ["creator-hub", "status" | "setup"])
        | (_, ["projects", _, "creator-hub", "setup" | "configure"]) => {
            Err(Oc { status: 405, msg: "That method is not available here.".into() })
        }
        _ => return None,
    };
    Some(match res {
        Ok(v) => Reply::Json(200, v),
        Err(e) => Reply::Json(e.status, json!({ "ok": false, "error": e.msg })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_cdp_url_validation_is_strict() {
        assert_eq!(
            validate_loopback_cdp_url("http://127.0.0.1:9222").unwrap(),
            "http://127.0.0.1:9222"
        );
        assert_eq!(
            validate_loopback_cdp_url("http://localhost:9223/").unwrap(),
            "http://localhost:9223"
        );
        assert_eq!(
            validate_loopback_cdp_url("http://[::1]:9222").unwrap(),
            "http://[::1]:9222"
        );
        for bad_url in [
            "",
            "https://127.0.0.1:9222",
            "ws://127.0.0.1:9222",
            "http://192.168.1.10:9222",
            "http://evil.example.com:9222",
            "http://127.0.0.1.evil.com:9222",
            "http://user:pass@127.0.0.1:9222",
            "http://127.0.0.1:9222/json",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:99999",
        ] {
            assert!(
                validate_loopback_cdp_url(bad_url).is_err(),
                "expected rejection for {bad_url:?}"
            );
        }
    }
}
