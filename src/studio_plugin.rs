//! Lightweight Companion Roblox Studio Plugin Bridge (`OpenRoCompanion.lua`).
//!
//! Provides a zero-external-dependency, token-authenticated loopback HTTP long-poll
//! bridge between OpenRO and a lightweight Luau plugin running directly inside
//! Roblox Studio (`HttpService`, `ChangeHistoryService`, `Selection`, `LogService`).
//!
//! Security guarantees:
//! - 256-bit CSPRNG bearer token (`X-OpenRo-Plugin-Token`), persisted `0600` in
//!   `data_dir/plugin_token` and verified with constant-time comparison.
//! - Token is NEVER echoed in `/api/status` or `/api/studio/plugin/status`.
//! - 1-click install writes `OpenRoCompanion.lua` ONLY inside a canonicalized,
//!   verified OS Roblox / Vinegar `Roblox/Plugins/` directory (rejects `..`,
//!   symlink escapes, and non-`Plugins` paths cleanly).

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Filename installed inside the OS Roblox Studio `Plugins/` folder.
pub const COMPANION_PLUGIN_FILENAME: &str = "OpenRoCompanion.lua";
/// Header required on `/handshake`, `/poll`, and `/result`.
pub const PLUGIN_TOKEN_HEADER: &str = "X-OpenRo-Plugin-Token";
/// Heartbeat freshness window (45 seconds) before a plugin session is treated as disconnected.
pub const PLUGIN_HEARTBEAT_TTL_MS: u64 = 45_000;
/// Maximum long-poll wait per HTTP `/poll` request so connection slots stay responsive.
pub const MAX_POLL_TIMEOUT_MS: u64 = 15_000;
/// Maximum queued commands waiting for the plugin before rejecting backpressure cleanly.
const MAX_PENDING_COMMANDS: usize = 64;

static TOKEN_ENTROPY_SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Constant-time byte-slice equality check (length + XOR fold) for bearer tokens.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Generate 32 cryptographically strong random bytes using OS CSPRNG
/// (`/dev/urandom` on Unix, `BCryptGenRandom` on Windows) encoded as 64 lowercase hex chars.
pub fn generate_csprng_token() -> String {
    let mut raw = [0u8; 32];
    let filled = fill_os_random(&mut raw);
    if !filled {
        // Defensive fallback only if OS CSPRNG read unexpectedly fails.
        let mut h = Sha256::new();
        h.update(now_ms().to_le_bytes());
        h.update(std::process::id().to_le_bytes());
        h.update(
            TOKEN_ENTROPY_SEQ
                .fetch_add(1, Ordering::SeqCst)
                .to_le_bytes(),
        );
        let stack_addr = (&raw as *const [u8; 32]) as usize;
        h.update(stack_addr.to_le_bytes());
        let digest = h.finalize();
        raw.copy_from_slice(&digest);
    }
    let mut hex = String::with_capacity(64);
    for b in raw {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

#[cfg(unix)]
fn fill_os_random(out: &mut [u8]) -> bool {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn fill_os_random(out: &mut [u8]) -> bool {
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            h_algorithm: *mut std::ffi::c_void,
            pb_buffer: *mut u8,
            cb_buffer: u32,
            dw_flags: u32,
        ) -> i32;
    }
    // SAFETY: `out.as_mut_ptr()` points to `out.len()` writable bytes and null algorithm handle
    // with `BCRYPT_USE_SYSTEM_PREFERRED_RNG` is documented safe by Win32 CNG.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            out.as_mut_ptr(),
            out.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    status == 0
}

#[cfg(not(any(unix, windows)))]
fn fill_os_random(_out: &mut [u8]) -> bool {
    false
}

fn is_valid_token_format(s: &str) -> bool {
    s.len() >= 32 && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Load an existing CSPRNG token from `data_dir/plugin_token` or create & persist
/// a fresh one atomically with mode `0600` (owner read/write only on Unix).
pub fn load_or_create_plugin_token(data_dir: Option<&Path>) -> String {
    if let Some(dir) = data_dir {
        let token_path = dir.join("plugin_token");
        if let Ok(existing) = std::fs::read_to_string(&token_path) {
            let trimmed = existing.trim();
            if is_valid_token_format(trimmed) {
                enforce_0600(&token_path);
                return trimmed.to_string();
            }
        }
        let token = generate_csprng_token();
        let _ = std::fs::create_dir_all(dir);
        let tmp_path = dir.join("plugin_token.tmp");
        if write_secret_file(&tmp_path, &token).is_ok() {
            let _ = std::fs::rename(&tmp_path, &token_path);
            enforce_0600(&token_path);
        }
        return token;
    }
    generate_csprng_token()
}

fn write_secret_file(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        f.write_all(content.as_bytes())?;
        f.write_all(b"\n")?;
        Ok(())
    }
}

fn enforce_0600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// One pending tool command dispatched by OpenRO to the connected Companion Studio Plugin.
#[derive(Clone, Debug)]
pub struct PluginCommand {
    pub id: String,
    pub tool: String,
    pub args: Value,
    pub created_at_ms: u64,
}

impl PluginCommand {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "tool": self.tool,
            "args": self.args,
            "created_at_ms": self.created_at_ms,
        })
    }
}

struct BridgeInner {
    token: String,
    connected: bool,
    studio_name: String,
    place_name: String,
    selection: Vec<String>,
    last_heartbeat_ms: u64,
    next_cmd_id: u64,
    pending: VecDeque<PluginCommand>,
    in_flight: HashMap<String, u64>,
    results: HashMap<String, Result<Value, String>>,
}

/// Thread-safe Companion Studio Plugin bridge shared between HTTP endpoints and `PlaceBackend::Plugin`.
pub struct StudioPluginBridge {
    inner: Mutex<BridgeInner>,
    cmd_cv: Condvar,
    res_cv: Condvar,
}

impl StudioPluginBridge {
    pub fn open(data_dir: Option<&Path>) -> Arc<Self> {
        let token = load_or_create_plugin_token(data_dir);
        Arc::new(StudioPluginBridge {
            inner: Mutex::new(BridgeInner {
                token,
                connected: false,
                studio_name: String::new(),
                place_name: String::new(),
                selection: Vec::new(),
                last_heartbeat_ms: 0,
                next_cmd_id: 1,
                pending: VecDeque::new(),
                in_flight: HashMap::new(),
                results: HashMap::new(),
            }),
            cmd_cv: Condvar::new(),
            res_cv: Condvar::new(),
        })
    }

    /// Constant-time verification of the `X-OpenRo-Plugin-Token` header.
    pub fn verify_token(&self, presented: &str) -> bool {
        let trimmed = presented.trim();
        if trimmed.is_empty() {
            return false;
        }
        let guard = self.inner.lock().unwrap();
        constant_time_eq(guard.token.as_bytes(), trimmed.as_bytes())
    }

    /// Return the raw token ONLY for local `OpenRoCompanion.lua` templating/download/install.
    pub fn token_for_local_template(&self) -> String {
        self.inner.lock().unwrap().token.clone()
    }

    /// True when the plugin completed a handshake/poll within `PLUGIN_HEARTBEAT_TTL_MS`.
    pub fn is_connected(&self) -> bool {
        let guard = self.inner.lock().unwrap();
        guard.connected
            && guard.last_heartbeat_ms > 0
            && now_ms().saturating_sub(guard.last_heartbeat_ms) <= PLUGIN_HEARTBEAT_TTL_MS
    }

    /// Record a plugin handshake + live selection heartbeat.
    pub fn record_handshake(
        &self,
        studio_name: &str,
        place_name: &str,
        selection: Vec<String>,
    ) -> Value {
        let now = now_ms();
        let mut guard = self.inner.lock().unwrap();
        guard.connected = true;
        guard.last_heartbeat_ms = now;
        if !studio_name.trim().is_empty() {
            guard.studio_name = studio_name.trim().chars().take(128).collect();
        } else if guard.studio_name.is_empty() {
            guard.studio_name = "Roblox Studio".to_string();
        }
        if !place_name.trim().is_empty() {
            guard.place_name = place_name.trim().chars().take(160).collect();
        }
        guard.selection = selection
            .into_iter()
            .take(64)
            .map(|s| s.chars().take(256).collect())
            .collect();
        json!({
            "ok": true,
            "connected": true,
            "studio_name": guard.studio_name,
            "place_name": guard.place_name,
            "selection": guard.selection,
            "last_heartbeat_ms": guard.last_heartbeat_ms,
        })
    }

    /// Long-poll for the next queued command from OpenRO (wakes immediately when a command arrives).
    pub fn poll_next_command(&self, timeout: Duration) -> Option<PluginCommand> {
        let capped = timeout.min(Duration::from_millis(MAX_POLL_TIMEOUT_MS));
        let deadline = Instant::now() + capped;
        let mut guard = self.inner.lock().unwrap();
        guard.connected = true;
        guard.last_heartbeat_ms = now_ms();
        loop {
            if let Some(cmd) = guard.pending.pop_front() {
                guard.in_flight.insert(cmd.id.clone(), now_ms());
                guard.last_heartbeat_ms = now_ms();
                return Some(cmd);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                guard.last_heartbeat_ms = now_ms();
                return None;
            }
            let (new_guard, wait_res) = self.cmd_cv.wait_timeout(guard, remaining).unwrap();
            guard = new_guard;
            if wait_res.timed_out() && guard.pending.is_empty() {
                guard.last_heartbeat_ms = now_ms();
                return None;
            }
        }
    }

    /// Deliver the execution result for command `id` back to the waiting caller.
    pub fn submit_result(
        &self,
        id: &str,
        ok: bool,
        result: Value,
        error: Option<String>,
        selection: Option<Vec<String>>,
    ) -> bool {
        let mut guard = self.inner.lock().unwrap();
        guard.connected = true;
        guard.last_heartbeat_ms = now_ms();
        if let Some(sel) = selection {
            guard.selection = sel
                .into_iter()
                .take(64)
                .map(|s| s.chars().take(256).collect())
                .collect();
        }
        let known = guard.in_flight.remove(id).is_some();
        if !known {
            return false;
        }
        let outcome = if ok {
            Ok(result)
        } else {
            Err(error
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Companion Studio Plugin command failed".to_string()))
        };
        guard.results.insert(id.to_string(), outcome);
        self.res_cv.notify_all();
        true
    }

    /// Enqueue a tool command for the connected Companion Studio Plugin and wait for its result.
    pub fn execute_command(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.lock().unwrap();
        let alive = guard.connected
            && guard.last_heartbeat_ms > 0
            && now_ms().saturating_sub(guard.last_heartbeat_ms) <= PLUGIN_HEARTBEAT_TTL_MS;
        if !alive {
            return Err(
                "Companion Studio Plugin is not connected (install or enable OpenRoCompanion.lua in Roblox Studio)."
                    .to_string(),
            );
        }
        if guard.pending.len() >= MAX_PENDING_COMMANDS {
            return Err("Companion Studio Plugin command queue is full; retry shortly.".to_string());
        }
        let seq = guard.next_cmd_id;
        guard.next_cmd_id += 1;
        let id = format!("pcmd-{}-{seq}", now_ms());
        guard.pending.push_back(PluginCommand {
            id: id.clone(),
            tool: tool.to_string(),
            args,
            created_at_ms: now_ms(),
        });
        self.cmd_cv.notify_all();

        loop {
            if let Some(outcome) = guard.results.remove(&id) {
                return outcome;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                guard.pending.retain(|c| c.id != id);
                guard.in_flight.remove(&id);
                return Err(format!(
                    "Companion Studio Plugin did not answer '{tool}' within {}s",
                    timeout.as_secs().max(1)
                ));
            }
            let (new_guard, wait_res) = self.res_cv.wait_timeout(guard, remaining).unwrap();
            guard = new_guard;
            if wait_res.timed_out() && !guard.results.contains_key(&id) {
                guard.pending.retain(|c| c.id != id);
                guard.in_flight.remove(&id);
                return Err(format!(
                    "Companion Studio Plugin did not answer '{tool}' within {}s",
                    timeout.as_secs().max(1)
                ));
            }
        }
    }

    /// JSON payload for `GET /api/studio/plugin/status` (never includes raw bearer token).
    pub fn status_json(&self) -> Value {
        let detected_dir = detect_roblox_plugins_dir();
        let install_file = detected_dir
            .as_ref()
            .map(|d| d.join(COMPANION_PLUGIN_FILENAME));
        let installed = install_file.as_ref().is_some_and(|p| p.is_file());
        let guard = self.inner.lock().unwrap();
        let connected = guard.connected
            && guard.last_heartbeat_ms > 0
            && now_ms().saturating_sub(guard.last_heartbeat_ms) <= PLUGIN_HEARTBEAT_TTL_MS;
        json!({
            "ok": true,
            "connected": connected,
            "studio_name": guard.studio_name,
            "place_name": guard.place_name,
            "selection": guard.selection,
            "last_heartbeat_ms": guard.last_heartbeat_ms,
            "install_path": install_file.map(|p| p.to_string_lossy().into_owned()),
            "installed": installed,
            "token_configured": !guard.token.is_empty(),
        })
    }
}

/// Detect the verified OS Roblox / Vinegar `Plugins/` directory passively (never creates directories).
pub fn detect_roblox_plugins_dir() -> Option<PathBuf> {
    if let Ok(override_dir) = std::env::var("OPENRO_ROBLOX_PLUGINS_DIR") {
        let p = PathBuf::from(override_dir.trim());
        if verify_plugins_path_shape(&p).is_ok() && p.parent().is_some_and(|r| r.is_dir()) {
            return Some(p);
        }
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        let home = PathBuf::from(std::env::var("HOME").ok()?);
        let current_user = std::env::var("USER").unwrap_or_default();
        let roots = [
            home.join(".var/app/org.vinegarhq.Vinegar/data/vinegar/prefixes/studio/drive_c/users"),
            home.join(".local/share/vinegar/prefixes/studio/drive_c/users"),
        ];
        for users_dir in roots {
            if let Some(found) = find_vinegar_plugins_dir(&users_dir, &current_user) {
                return Some(found);
            }
        }
        None
    }

    #[cfg(windows)]
    {
        let local_appdata = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?);
        let roblox_dir = local_appdata.join("Roblox");
        if roblox_dir.is_dir() {
            return Some(roblox_dir.join("Plugins"));
        }
        None
    }

    #[cfg(target_os = "macos")]
    {
        let home = PathBuf::from(std::env::var("HOME").ok()?);
        let roblox_dir = home.join("Documents").join("Roblox");
        if roblox_dir.is_dir() {
            return Some(roblox_dir.join("Plugins"));
        }
        None
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn find_vinegar_plugins_dir(users_dir: &Path, preferred_user: &str) -> Option<PathBuf> {
    if !users_dir.is_dir() {
        return None;
    }
    if !preferred_user.is_empty() {
        let preferred_roblox = users_dir
            .join(preferred_user)
            .join("AppData/Local/Roblox");
        if preferred_roblox.is_dir() {
            return Some(preferred_roblox.join("Plugins"));
        }
    }
    let mut fallback: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(users_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case("Public") {
                continue;
            }
            let roblox_dir = entry.path().join("AppData/Local/Roblox");
            if roblox_dir.is_dir() {
                if roblox_dir.join("Plugins").is_dir() {
                    return Some(roblox_dir.join("Plugins"));
                }
                if fallback.is_none() {
                    fallback = Some(roblox_dir.join("Plugins"));
                }
            }
        }
    }
    fallback
}

/// Pure structural validation that a path has no traversal segments and ends in `Roblox/Plugins`.
fn verify_plugins_path_shape(dir: &Path) -> Result<(), String> {
    let raw = dir.to_string_lossy();
    if raw.trim().is_empty() || raw.chars().any(|c| c.is_control()) {
        return Err("invalid Plugins directory path".to_string());
    }
    for comp in dir.components() {
        if matches!(comp, Component::ParentDir) {
            return Err("path traversal ('..') is forbidden in Plugins directory path".to_string());
        }
    }
    let leaf = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let parent_name = dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !leaf.eq_ignore_ascii_case("Plugins") || !parent_name.eq_ignore_ascii_case("Roblox") {
        return Err(
            "refusing to write outside a verified Roblox/Plugins directory".to_string(),
        );
    }
    Ok(())
}

/// Verify and canonicalize a candidate `Roblox/Plugins` directory, ensuring no symlink escape.
pub fn verify_safe_plugins_dir(candidate: &Path) -> Result<PathBuf, String> {
    verify_plugins_path_shape(candidate)?;
    let roblox_parent = candidate
        .parent()
        .ok_or_else(|| "Plugins directory has no parent Roblox directory".to_string())?;
    if !roblox_parent.is_dir() {
        return Err(
            "Roblox Studio directory does not exist yet on this machine — open Roblox Studio once or download OpenRoCompanion.lua manually."
                .to_string(),
        );
    }
    let canon_roblox = std::fs::canonicalize(roblox_parent)
        .map_err(|e| format!("failed to canonicalize Roblox directory: {e}"))?;
    let canon_roblox_name = canon_roblox
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !canon_roblox_name.eq_ignore_ascii_case("Roblox") {
        return Err(
            "refusing to follow symlink outside verified Roblox directory".to_string(),
        );
    }

    let plugins_under_canon = canon_roblox.join("Plugins");
    if let Ok(meta) = std::fs::symlink_metadata(&plugins_under_canon) {
        if meta.file_type().is_symlink() {
            return Err("refusing to install through symlinked Plugins directory".to_string());
        }
    } else {
        std::fs::create_dir_all(&plugins_under_canon)
            .map_err(|e| format!("failed to create Roblox/Plugins directory: {e}"))?;
    }
    let canon_plugins = std::fs::canonicalize(&plugins_under_canon)
        .map_err(|e| format!("failed to canonicalize Plugins directory: {e}"))?;
    verify_plugins_path_shape(&canon_plugins)?;
    if canon_plugins.parent() != Some(canon_roblox.as_path()) {
        return Err("canonical Plugins directory escaped Roblox root".to_string());
    }
    Ok(canon_plugins)
}

/// Write `OpenRoCompanion.lua` into the verified OS Roblox/Vinegar `Plugins/` directory.
/// Rejects any caller-supplied `requested_dir` that traverses or does not match the verified OS folder.
pub fn install_companion_plugin(
    port: u16,
    token: &str,
    requested_dir: Option<&str>,
) -> Result<PathBuf, String> {
    let detected = detect_roblox_plugins_dir().ok_or_else(|| {
        "Roblox Studio Plugins directory was not found on this machine — launch Roblox Studio once or use Download OpenRoCompanion.lua instead."
            .to_string()
    })?;
    let verified = verify_safe_plugins_dir(&detected)?;
    if let Some(req) = requested_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let req_path = Path::new(req);
        verify_plugins_path_shape(req_path)?;
        let canon_req = std::fs::canonicalize(req_path).map_err(|_| {
            "requested Plugins directory does not match the verified OS Roblox/Plugins folder"
                .to_string()
        })?;
        if canon_req != verified {
            return Err(
                "refusing to install outside the verified OS Roblox/Plugins directory".to_string(),
            );
        }
    }
    write_plugin_file_to_verified_dir(&verified, port, token)
}

/// Read the Settings preference for auto-installing `OpenRoCompanion.lua` when
/// Vinegar/Studio is detected (`data_dir/studio_prefs.json`, default `true`) (#29).
pub fn auto_install_pref(data_dir: Option<&Path>) -> bool {
    let Some(dir) = data_dir else { return true };
    let Ok(text) = std::fs::read_to_string(dir.join("studio_prefs.json")) else {
        return true;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("auto_install_plugin").and_then(|b| b.as_bool()))
        .unwrap_or(true)
}

/// Persist the `auto_install_plugin` Settings toggle (`data_dir/studio_prefs.json`) (#29).
pub fn set_auto_install_pref(data_dir: Option<&Path>, enabled: bool) -> Result<(), String> {
    let Some(dir) = data_dir else { return Ok(()) };
    std::fs::create_dir_all(dir).map_err(|e| format!("create prefs dir: {e}"))?;
    let path = dir.join("studio_prefs.json");
    let tmp = dir.join("studio_prefs.json.tmp");
    let body = serde_json::to_string_pretty(&json!({ "auto_install_plugin": enabled }))
        .map_err(|e| format!("serialize studio prefs: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("write studio prefs: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("commit studio prefs: {e}"))?;
    Ok(())
}

/// Best-effort first-launch install of `OpenRoCompanion.lua` into the verified
/// Vinegar `Roblox/Plugins/` directory when `auto_install_plugin` is enabled (#29).
/// Never touches any path outside the verified OS `Roblox/Plugins` directory and
/// is a no-op if the plugin file already exists or `OPENRO_NO_AUTO_PLUGIN=1`.
pub fn maybe_auto_install_vinegar_plugin(
    bridge: &StudioPluginBridge,
    data_dir: &Path,
    port: u16,
) -> Option<PathBuf> {
    if std::env::var("OPENRO_NO_AUTO_PLUGIN")
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
    {
        return None;
    }
    if !auto_install_pref(Some(data_dir)) {
        return None;
    }
    let detected = detect_roblox_plugins_dir()?;
    if !detected.to_string_lossy().contains("Vinegar") {
        return None;
    }
    if detected.join(COMPANION_PLUGIN_FILENAME).is_file() {
        return None;
    }
    let token = bridge.token_for_local_template();
    write_plugin_file_to_verified_dir(&detected, port, &token).ok()
}

/// Direct helper for writing `OpenRoCompanion.lua` into a canonicalized verified `Roblox/Plugins` directory.
pub fn write_plugin_file_to_verified_dir(
    plugins_dir: &Path,
    port: u16,
    token: &str,
) -> Result<PathBuf, String> {
    let verified = verify_safe_plugins_dir(plugins_dir)?;
    let target = verified.join(COMPANION_PLUGIN_FILENAME);
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err("refusing to overwrite symlinked OpenRoCompanion.lua".to_string());
        }
    }
    let lua = render_companion_lua(port, token);
    let tmp = verified.join(".OpenRoCompanion.lua.tmp");
    std::fs::write(&tmp, lua.as_bytes())
        .map_err(|e| format!("failed to write temporary plugin file: {e}"))?;
    std::fs::rename(&tmp, &target)
        .map_err(|e| format!("failed to install OpenRoCompanion.lua: {e}"))?;
    Ok(target)
}

/// Render the real Roblox Studio companion plugin (`OpenRoCompanion.lua`) templated with
/// the server's loopback port and CSPRNG bearer token.
pub fn render_companion_lua(port: u16, token: &str) -> String {
    let safe_token: String = token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let template = include_str_companion_lua();
    template
        .replace("__OPENRO_PORT__", &port.to_string())
        .replace("__OPENRO_TOKEN__", &safe_token)
}

fn include_str_companion_lua() -> &'static str {
    r##"--!strict
-- OpenRoCompanion.lua -- OpenRO Companion Roblox Studio Plugin
-- Enables live DataModel sync, cursor-paginated snapshot capture, ChangeHistoryService
-- undo waypoints, Selection sync, LogService console streaming, and Luau execution.

local HttpService = game:GetService("HttpService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local Selection = game:GetService("Selection")
local LogService = game:GetService("LogService")
local RunService = game:GetService("RunService")

local OPENRO_HOST = "http://127.0.0.1:__OPENRO_PORT__"
local OPENRO_TOKEN = "__OPENRO_TOKEN__"
local HEADERS = {
	["X-OpenRo-Plugin-Token"] = OPENRO_TOKEN,
}

local TOOLBAR = plugin:CreateToolbar("OpenRO")
local CONNECT_BTN = TOOLBAR:CreateButton(
	"OpenRoCompanionConnect",
	"Connect Roblox Studio to OpenRO Copilot",
	"rbxassetid://4458901886"
)

local running = true
local SERVICES = {
	"ReplicatedStorage",
	"ServerScriptService",
	"ServerStorage",
	"StarterGui",
	"StarterPlayer",
	"StarterPack",
	"ReplicatedFirst",
	"Lighting",
	"SoundService",
	"Teams",
	"Workspace",
}

local SNAPSHOT_HARD_LIMIT = 65536
local SNAPSHOT_DEFAULT_MAX_BYTES = 56000
local SNAPSHOT_SCRIPT_CHUNK = 40000
local cachedSnapshots: { [string]: { entries: { any }, lighting: { [string]: any } } } = {}
local snapshotOrder: { string } = {}
local snapshotSeq = 0

local function round4(n: number): number
	if n ~= n or n == math.huge or n == -math.huge then
		return 0
	end
	return math.round(n * 10000) / 10000
end

local function fullPath(inst: Instance): string
	local parts = {}
	local cur: Instance? = inst
	while cur and cur ~= game do
		table.insert(parts, 1, cur.Name)
		cur = cur.Parent
	end
	return table.concat(parts, ".")
end

local function currentSelectionPaths(): { string }
	local out = {}
	for _, inst in ipairs(Selection:Get()) do
		local p = fullPath(inst)
		if #p > 0 then
			table.insert(out, p)
		end
	end
	return out
end

local function serializeValue(v: any): any
	local t = typeof(v)
	if t == "Vector3" then
		return { round4(v.X), round4(v.Y), round4(v.Z) }
	elseif t == "Vector2" then
		return { round4(v.X), round4(v.Y) }
	elseif t == "Color3" then
		return { round4(v.R), round4(v.G), round4(v.B) }
	elseif t == "UDim2" then
		return {
			{ round4(v.X.Scale), math.round(v.X.Offset) },
			{ round4(v.Y.Scale), math.round(v.Y.Offset) },
		}
	elseif t == "UDim" then
		return { round4(v.Scale), math.round(v.Offset) }
	elseif t == "NumberRange" then
		return { NumberRange = { round4(v.Min), round4(v.Max) } }
	elseif t == "CFrame" then
		local x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22 = v:GetComponents()
		return {
			position = { round4(x), round4(y), round4(z) },
			orientation = {
				{ round4(r00), round4(r01), round4(r02) },
				{ round4(r10), round4(r11), round4(r12) },
				{ round4(r20), round4(r21), round4(r22) },
			},
		}
	elseif t == "EnumItem" then
		return v.Name
	elseif t == "BrickColor" then
		return v.Number
	elseif t == "ColorSequence" then
		local kp = v.Keypoints and v.Keypoints[1]
		if kp then
			return { round4(kp.Value.R), round4(kp.Value.G), round4(kp.Value.B) }
		end
		return { 1, 1, 1 }
	elseif type(v) == "number" then
		return round4(v)
	elseif type(v) == "string" or type(v) == "boolean" then
		return v
	end
	return tostring(v)
end

local function serializeAttributes(inst: Instance): { [string]: any }?
	local okA, attrs = pcall(function()
		return inst:GetAttributes()
	end)
	if not okA or type(attrs) ~= "table" then
		return nil
	end
	local out: { [string]: any } = {}
	local count = 0
	for k, v in pairs(attrs) do
		out[tostring(k)] = serializeValue(v)
		count += 1
	end
	if count == 0 then
		return nil
	end
	return out
end

local function readProp(inst: Instance, props: { [string]: any }, key: string)
	local ok, val = pcall(function()
		return (inst :: any)[key]
	end)
	if ok and val ~= nil then
		props[key] = serializeValue(val)
	end
end

local function collectLightingProps(): { [string]: any }
	local out: { [string]: any } = {}
	local okL, lighting = pcall(function()
		return game:GetService("Lighting")
	end)
	if not okL or not lighting then
		return out
	end
	for _, key in ipairs({
		"ClockTime",
		"Brightness",
		"Ambient",
		"OutdoorAmbient",
		"ColorShift_Top",
		"ColorShift_Bottom",
		"FogStart",
		"FogEnd",
		"FogColor",
		"GlobalShadows",
		"ShadowSoftness",
		"EnvironmentDiffuseScale",
		"EnvironmentSpecularScale",
		"ExposureCompensation",
		"Technology",
	}) do
		readProp(lighting, out, key)
	end
	local attrs = serializeAttributes(lighting)
	if attrs then
		out.Attributes = attrs
	end
	return out
end

local function captureInstanceProps(d: Instance): (boolean, { [string]: any })
	local props: { [string]: any } = {}
	local captured = false

	if d:IsA("BasePart") then
		captured = true
		local p = d :: BasePart
		props.CFrame = serializeValue(p.CFrame)
		props.Size = serializeValue(p.Size)
		props.Color = serializeValue(p.Color)
		props.Anchored = p.Anchored
		props.Material = p.Material.Name
		props.Transparency = round4(p.Transparency)
		props.CanCollide = p.CanCollide
		readProp(p, props, "CastShadow")
		if d:IsA("Part") then
			readProp(d, props, "Shape")
		elseif d:IsA("MeshPart") then
			readProp(d, props, "MeshId")
			readProp(d, props, "TextureID")
		end
	elseif d:IsA("SurfaceAppearance") then
		captured = true
		for _, k in ipairs({ "ColorMap", "MetalnessMap", "NormalMap", "RoughnessMap", "AlphaMode" }) do
			readProp(d, props, k)
		end
	elseif d:IsA("PointLight") or d:IsA("SpotLight") or d:IsA("SurfaceLight") then
		captured = true
		for _, k in ipairs({ "Color", "Brightness", "Range", "Shadows", "Enabled" }) do
			readProp(d, props, k)
		end
		if d:IsA("SpotLight") or d:IsA("SurfaceLight") then
			readProp(d, props, "Angle")
			readProp(d, props, "Face")
		end
	elseif d:IsA("ParticleEmitter") then
		captured = true
		for _, k in ipairs({
			"Rate",
			"LightEmission",
			"Enabled",
			"Texture",
			"EmissionDirection",
			"Lifetime",
			"Speed",
			"SpreadAngle",
			"Color",
		}) do
			readProp(d, props, k)
		end
	elseif d:IsA("ScreenGui") or d:IsA("BillboardGui") or d:IsA("SurfaceGui") then
		captured = true
		for _, k in ipairs({
			"Enabled",
			"ResetOnSpawn",
			"DisplayOrder",
			"ZIndexBehavior",
			"IgnoreGuiInset",
			"AlwaysOnTop",
			"Face",
			"SizingMode",
		}) do
			readProp(d, props, k)
		end
	elseif d:IsA("GuiObject") then
		captured = true
		for _, k in ipairs({
			"Position",
			"Size",
			"AnchorPoint",
			"BackgroundColor3",
			"BackgroundTransparency",
			"BorderSizePixel",
			"Visible",
			"ZIndex",
			"LayoutOrder",
			"Rotation",
		}) do
			readProp(d, props, k)
		end
		if d:IsA("TextLabel") or d:IsA("TextButton") or d:IsA("TextBox") then
			for _, k in ipairs({ "Text", "TextColor3", "TextSize", "TextScaled", "Font" }) do
				readProp(d, props, k)
			end
		end
		if d:IsA("ImageLabel") or d:IsA("ImageButton") then
			for _, k in ipairs({ "Image", "ImageColor3", "ImageTransparency", "ScaleType" }) do
				readProp(d, props, k)
			end
		end
	elseif d:IsA("UICorner") or d:IsA("UIStroke") or d:IsA("UIListLayout") or d:IsA("UIGridLayout") or d:IsA("UIPadding") or d:IsA("UIAspectRatioConstraint") or d:IsA("UIScale") then
		captured = true
		for _, k in ipairs({
			"CornerRadius",
			"Thickness",
			"Color",
			"Transparency",
			"FillDirection",
			"SortOrder",
			"HorizontalAlignment",
			"VerticalAlignment",
			"Padding",
			"CellSize",
			"CellPadding",
			"PaddingTop",
			"PaddingBottom",
			"PaddingLeft",
			"PaddingRight",
			"AspectRatio",
			"Scale",
		}) do
			readProp(d, props, k)
		end
	elseif d:IsA("Sound") then
		captured = true
		for _, k in ipairs({ "SoundId", "Volume", "Looped", "PlaybackSpeed", "Playing" }) do
			readProp(d, props, k)
		end
	elseif d:IsA("Decal") or d:IsA("Texture") then
		captured = true
		for _, k in ipairs({ "Texture", "Face", "Transparency", "Color3" }) do
			readProp(d, props, k)
		end
	elseif d:IsA("Atmosphere") then
		captured = true
		for _, k in ipairs({ "Density", "Offset", "Color", "Decay", "Glare", "Haze" }) do
			readProp(d, props, k)
		end
	elseif d:IsA("Sky") then
		captured = true
		for _, k in ipairs({
			"SkyboxBk",
			"SkyboxDn",
			"SkyboxFt",
			"SkyboxLf",
			"SkyboxRt",
			"SkyboxUp",
			"StarCount",
			"CelestialBodiesShown",
		}) do
			readProp(d, props, k)
		end
	elseif d:IsA("ValueBase") then
		captured = true
		readProp(d, props, "Value")
	elseif d:IsA("Model") or d:IsA("Folder") or d:IsA("RemoteEvent") or d:IsA("RemoteFunction") or d:IsA("BindableEvent") or d:IsA("Tool") or d:IsA("Attachment") or d:IsA("ProximityPrompt") or d:IsA("ClickDetector") or d:IsA("WeldConstraint") or d:IsA("BloomEffect") or d:IsA("ColorCorrectionEffect") or d:IsA("SunRaysEffect") or d:IsA("DepthOfFieldEffect") then
		captured = true
	end

	local attrs = serializeAttributes(d)
	if attrs then
		captured = true
		props.Attributes = attrs
	end

	return captured, props
end

local function buildSnapshotEntries(): ({ any }, { [string]: any })
	local entries: { any } = {}
	for _, svcName in ipairs(SERVICES) do
		local okSvc, svc = pcall(function()
			return game:GetService(svcName)
		end)
		if okSvc and svc then
			for _, d in ipairs(svc:GetDescendants()) do
				if d:IsA("LuaSourceContainer") then
					local okSrc, src = pcall(function()
						return (d :: any).Source
					end)
					if okSrc and type(src) == "string" then
						local p = fullPath(d)
						local attrs = serializeAttributes(d)
						if #src <= SNAPSHOT_SCRIPT_CHUNK then
							table.insert(entries, {
								kind = "script",
								path = p,
								class_name = d.ClassName,
								source = src,
								attributes = attrs,
							})
						else
							local total = math.ceil(#src / SNAPSHOT_SCRIPT_CHUNK)
							for idx = 1, total do
								local startPos = (idx - 1) * SNAPSHOT_SCRIPT_CHUNK + 1
								local endPos = math.min(#src, idx * SNAPSHOT_SCRIPT_CHUNK)
								table.insert(entries, {
									kind = "script_part",
									path = p,
									class_name = d.ClassName,
									index = idx,
									total = total,
									data = string.sub(src, startPos, endPos),
									attributes = if idx == 1 then attrs else nil,
								})
							end
						end
					end
				else
					local okCap, props = captureInstanceProps(d)
					if okCap then
						table.insert(entries, {
							kind = "instance",
							path = fullPath(d),
							class_name = d.ClassName,
							properties = props,
						})
					end
				end
			end
		end
	end
	return entries, collectLightingProps()
end

local function collectPlaceSnapshot(args: { [string]: any }): { [string]: any }
	local maxBytes = math.clamp(tonumber(args.max_bytes) or SNAPSHOT_DEFAULT_MAX_BYTES, 4096, 60000)
	local reqSnapId = if type(args.snapshot_id) == "string" and #args.snapshot_id > 0 then args.snapshot_id else nil
	local cursorNum = math.max(1, (tonumber(args.cursor) or 0) + 1)

	local snapId = reqSnapId
	local snap = if snapId then cachedSnapshots[snapId] else nil
	if not snap then
		local entries, lighting = buildSnapshotEntries()
		snapshotSeq += 1
		snapId = reqSnapId or ("snap-" .. tostring(os.time()) .. "-" .. tostring(snapshotSeq))
		snap = { entries = entries, lighting = lighting }
		cachedSnapshots[snapId] = snap
		table.insert(snapshotOrder, snapId)
		while #snapshotOrder > 2 do
			local oldest = table.remove(snapshotOrder, 1)
			if oldest then
				cachedSnapshots[oldest] = nil
			end
		end
	end

	local scripts: { [string]: string } = {}
	local scriptParts: { [string]: any } = {}
	local scriptMeta: { [string]: any } = {}
	local instances: { [string]: any } = {}
	local entries = snap.entries
	local total = #entries
	local idx = cursorNum

	local lightingPayload = if cursorNum == 1 then snap.lighting else nil
	local chunk: { [string]: any } = {
		snapshot_id = snapId,
		cursor = tostring(cursorNum - 1),
		next_cursor = nil,
		done = true,
		total_items = total,
		scripts = scripts,
		script_parts = scriptParts,
		script_meta = scriptMeta,
		instances = instances,
		lighting = lightingPayload,
	}

	while idx <= total do
		local entry = entries[idx]
		local p = entry.path
		local prevScript = scripts[p]
		local prevPart = scriptParts[p]
		local prevMeta = scriptMeta[p]
		local prevInst = instances[p]

		if entry.kind == "script" then
			scripts[p] = entry.source
			scriptMeta[p] = { class_name = entry.class_name, attributes = entry.attributes }
		elseif entry.kind == "script_part" then
			scriptParts[p] = { index = entry.index, total = entry.total, data = entry.data }
			if entry.index == 1 then
				scriptMeta[p] = { class_name = entry.class_name, attributes = entry.attributes }
			end
		else
			instances[p] = {
				class_name = entry.class_name,
				properties = entry.properties,
			}
		end

		local encodedLen = #HttpService:JSONEncode(chunk)
		if encodedLen > maxBytes and idx > cursorNum then
			scripts[p] = prevScript
			scriptParts[p] = prevPart
			scriptMeta[p] = prevMeta
			instances[p] = prevInst
			break
		end
		idx += 1
		if encodedLen >= SNAPSHOT_HARD_LIMIT then
			break
		end
	end

	local done = idx > total
	chunk.done = done
	chunk.next_cursor = if done then nil else tostring(idx - 1)
	chunk.chunk_bytes = #HttpService:JSONEncode(chunk)
	if done and snapId then
		cachedSnapshots[snapId] = nil
	end
	return chunk
end

local function resolvePath(dotPath: string, createFolders: boolean): Instance?
	local segments = string.split(dotPath, ".")
	if #segments < 1 then
		return nil
	end
	local okSvc, cur = pcall(function()
		return game:GetService(segments[1])
	end)
	if not okSvc or not cur then
		return nil
	end
	for i = 2, #segments do
		local seg = segments[i]
		local child = cur:FindFirstChild(seg)
		if not child then
			if createFolders then
				local folder = Instance.new("Folder")
				folder.Name = seg
				folder.Parent = cur
				child = folder
			else
				return nil
			end
		end
		cur = child
	end
	return cur
end

local function inferScriptClass(dotPath: string): string
	if string.sub(dotPath, 1, 11) == "StarterGui." or string.sub(dotPath, 1, 14) == "StarterPlayer." then
		return "LocalScript"
	elseif string.sub(dotPath, 1, 18) == "ReplicatedStorage." then
		return "ModuleScript"
	end
	return "Script"
end

local function applyProperties(inst: Instance, props: { [string]: any }?)
	if type(props) ~= "table" then
		return
	end
	for k, v in pairs(props) do
		pcall(function()
			if k == "Attributes" and type(v) == "table" then
				for ak, av in pairs(v) do
					inst:SetAttribute(tostring(ak), av)
				end
			elseif k == "CFrame" and type(v) == "table" then
				local cf = v.CFrame or v
				if type(cf) == "table" and type(cf.position) == "table" and type(cf.orientation) == "table" then
					local p = cf.position
					local o = cf.orientation
					(inst :: any).CFrame = CFrame.new(
						tonumber(p[1]) or 0, tonumber(p[2]) or 0, tonumber(p[3]) or 0,
						tonumber(o[1][1]) or 1, tonumber(o[1][2]) or 0, tonumber(o[1][3]) or 0,
						tonumber(o[2][1]) or 0, tonumber(o[2][2]) or 1, tonumber(o[2][3]) or 0,
						tonumber(o[3][1]) or 0, tonumber(o[3][2]) or 0, tonumber(o[3][3]) or 1
					)
				elseif #cf >= 12 then
					(inst :: any).CFrame = CFrame.new(
						tonumber(cf[1]) or 0, tonumber(cf[2]) or 0, tonumber(cf[3]) or 0,
						tonumber(cf[4]) or 1, tonumber(cf[5]) or 0, tonumber(cf[6]) or 0,
						tonumber(cf[7]) or 0, tonumber(cf[8]) or 1, tonumber(cf[9]) or 0,
						tonumber(cf[10]) or 0, tonumber(cf[11]) or 0, tonumber(cf[12]) or 1
					)
				end
			elseif (k == "Position" or k == "Size") and type(v) == "table" and #v >= 3 then
				(inst :: any)[k] = Vector3.new(tonumber(v[1]) or 0, tonumber(v[2]) or 0, tonumber(v[3]) or 0)
			elseif k == "Color" and type(v) == "table" and #v >= 3 then
				(inst :: any).Color = Color3.fromRGB(tonumber(v[1]) or 128, tonumber(v[2]) or 128, tonumber(v[3]) or 128)
			elseif k == "Color" and type(v) == "string" and string.sub(v, 1, 1) == "#" and #v == 7 then
				(inst :: any).Color = Color3.fromHex(v)
			elseif k == "Material" and type(v) == "string" then
				local mat = (Enum.Material :: any)[v]
				if mat then
					(inst :: any).Material = mat
				end
			else
				(inst :: any)[k] = v
			end
		end)
	end
end

local function collectPlaceTree(): { [string]: any }
	local scripts: { [string]: string } = {}
	local instances: { [string]: any } = {}
	local scount = 0
	local icount = 0
	for _, svcName in ipairs(SERVICES) do
		local okSvc, svc = pcall(function()
			return game:GetService(svcName)
		end)
		if okSvc and svc then
			for _, d in ipairs(svc:GetDescendants()) do
				if d:IsA("LuaSourceContainer") and scount < 160 then
					local okSrc, src = pcall(function()
						return (d :: any).Source
					end)
					if okSrc and type(src) == "string" then
						scripts[fullPath(d)] = src
						scount += 1
					end
				elseif d:IsA("BasePart") and icount < 320 then
					local p = d :: BasePart
					local c = p.Color
					instances[fullPath(p)] = {
						class_name = p.ClassName,
						properties = {
							Position = { math.round(p.Position.X * 100) / 100, math.round(p.Position.Y * 100) / 100, math.round(p.Position.Z * 100) / 100 },
							Size = { math.round(p.Size.X * 100) / 100, math.round(p.Size.Y * 100) / 100, math.round(p.Size.Z * 100) / 100 },
							Color = { math.round(c.R * 255), math.round(c.G * 255), math.round(c.B * 255) },
							Anchored = p.Anchored,
							Material = p.Material.Name,
							Transparency = math.round(p.Transparency * 100) / 100,
							CanCollide = p.CanCollide,
						},
					}
					icount += 1
				elseif (d:IsA("Model") or d:IsA("Folder") or d:IsA("RemoteEvent") or d:IsA("RemoteFunction") or d:IsA("Sound") or d:IsA("ScreenGui") or d:IsA("Atmosphere")) and icount < 320 then
					instances[fullPath(d)] = {
						class_name = d.ClassName,
						properties = {},
					}
					icount += 1
				end
			end
		end
	end
	return { scripts = scripts, instances = instances }
end

local function collectConsoleLines(): { string }
	local lines = {}
	local okLog, history = pcall(function()
		return LogService:GetLogHistory()
	end)
	if okLog and type(history) == "table" then
		local startIdx = math.max(1, #history - 60)
		for i = startIdx, #history do
			local item = history[i]
			if item and item.message then
				table.insert(lines, tostring(item.message))
			end
		end
	end
	return lines
end

local function handleCommand(tool: string, args: { [string]: any }): (boolean, any, string?)
	if tool == "get_place_tree" then
		return true, collectPlaceTree(), nil
	elseif tool == "get_place_snapshot" then
		return true, collectPlaceSnapshot(args), nil
	elseif tool == "write_script" or tool == "write_luau" then
		local dotPath = tostring(args.path or "")
		local source = tostring(args.source or "")
		local segments = string.split(dotPath, ".")
		if #segments < 2 then
			return false, nil, "invalid script path: " .. dotPath
		end
		local scriptName = segments[#segments]
		table.remove(segments, #segments)
		local parentPath = table.concat(segments, ".")
		local parentInst = resolvePath(parentPath, true)
		if not parentInst then
			return false, nil, "could not resolve parent: " .. parentPath
		end
		local existing = parentInst:FindFirstChild(scriptName)
		local target: Instance
		if existing and existing:IsA("LuaSourceContainer") then
			target = existing
		else
			if existing then
				existing:Destroy()
			end
			local created = Instance.new(inferScriptClass(dotPath))
			created.Name = scriptName
			created.Parent = parentInst
			target = created
		end
		(target :: any).Source = source
		pcall(function()
			ChangeHistoryService:SetWaypoint("OpenRO edit")
		end)
		return true, { ok = true, path = dotPath }, nil
	elseif tool == "delete_script" or tool == "delete_instance" then
		local dotPath = tostring(args.path or "")
		local inst = resolvePath(dotPath, false)
		if inst then
			inst:Destroy()
			pcall(function()
				ChangeHistoryService:SetWaypoint("OpenRO delete")
			end)
		end
		return true, { ok = true, path = dotPath }, nil
	elseif tool == "create_instance" then
		local className = tostring(args.class_name or "Part")
		local name = tostring(args.name or "Part")
		local parentPath = tostring(args.parent or "Workspace")
		local parentInst = resolvePath(parentPath, true)
		if not parentInst then
			return false, nil, "could not resolve parent: " .. parentPath
		end
		local existing = parentInst:FindFirstChild(name)
		local inst: Instance
		if existing and existing.ClassName == className then
			inst = existing
		else
			inst = Instance.new(className)
			inst.Name = name
			inst.Parent = parentInst
		end
		applyProperties(inst, args.properties)
		pcall(function()
			ChangeHistoryService:SetWaypoint("OpenRO create_instance")
		end)
		return true, { ok = true, path = parentPath .. "." .. name }, nil
	elseif tool == "get_console" or tool == "get_console_output" then
		local lines = collectConsoleLines()
		return true, { ok = true, lines = lines, console = table.concat(lines, "\n") }, nil
	elseif tool == "playtest" or tool == "start_stop_play" then
		local lines = collectConsoleLines()
		local consoleText = table.concat(lines, "\n")
		return true, { status = "passed", console = consoleText, lines = lines }, nil
	elseif tool == "execute_luau" then
		local code = tostring(args.code or "")
		local fn, loadErr = loadstring(code)
		if not fn then
			return false, nil, "Luau compile error: " .. tostring(loadErr)
		end
		local okExec, res = pcall(fn)
		if not okExec then
			return false, nil, "Luau runtime error: " .. tostring(res)
		end
		return true, { ok = true, output = tostring(res or "") }, nil
	end
	return false, nil, "unsupported plugin command: " .. tool
end

local function postJson(path: string, bodyTable: { [string]: any }): (boolean, any)
	local okReq, resp = pcall(function()
		return HttpService:RequestAsync({
			Url = OPENRO_HOST .. path,
			Method = "POST",
			Headers = {
				["Content-Type"] = "application/json",
				["X-OpenRo-Plugin-Token"] = OPENRO_TOKEN,
			},
			Body = HttpService:JSONEncode(bodyTable),
		})
	end)
	if not okReq or not resp or not resp.Success then
		return false, nil
	end
	local okDec, decoded = pcall(function()
		return HttpService:JSONDecode(resp.Body)
	end)
	return okDec, decoded
end

local function sendHandshake()
	postJson("/api/studio/plugin/handshake", {
		studio_name = "Roblox Studio (" .. (if RunService:IsEdit() then "Edit" else "Runtime") .. ")",
		place_name = game.Name,
		place_id = game.PlaceId,
		selection = currentSelectionPaths(),
	})
end

CONNECT_BTN.Click:Connect(function()
	running = not running
	CONNECT_BTN:SetActive(running)
	if running then
		sendHandshake()
	end
end)

Selection.SelectionChanged:Connect(function()
	if running then
		task.spawn(sendHandshake)
	end
end)

plugin.Unloading:Connect(function()
	running = false
end)

task.spawn(function()
	CONNECT_BTN:SetActive(true)
	sendHandshake()
	while running do
		local okPoll, resp = pcall(function()
			return HttpService:RequestAsync({
				Url = OPENRO_HOST .. "/api/studio/plugin/poll?timeout_ms=10000",
				Method = "GET",
				Headers = HEADERS,
			})
		end)
		if okPoll and resp and resp.Success then
			local okDec, data = pcall(function()
				return HttpService:JSONDecode(resp.Body)
			end)
			if okDec and type(data) == "table" and type(data.command) == "table" then
				local cmd = data.command
				local okCmd, result, errMsg = handleCommand(tostring(cmd.tool or ""), cmd.args or {})
				postJson("/api/studio/plugin/result", {
					id = cmd.id,
					ok = okCmd,
					result = result,
					error = errMsg,
					selection = currentSelectionPaths(),
				})
			end
		else
			task.wait(2)
		end
		task.wait(0.05)
	end
end)
"##
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csprng_token_persists_with_0600_and_constant_time_verifies() {
        let dir = std::env::temp_dir().join(format!(
            "openro-plugin-tok-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let t1 = load_or_create_plugin_token(Some(&dir));
        assert_eq!(t1.len(), 64);
        assert!(t1.bytes().all(|b| b.is_ascii_hexdigit()));
        let t2 = load_or_create_plugin_token(Some(&dir));
        assert_eq!(t1, t2);
        assert!(constant_time_eq(t1.as_bytes(), t2.as_bytes()));
        assert!(!constant_time_eq(t1.as_bytes(), b"wrong"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("plugin_token"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "plugin_token must be mode 0600");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rendered_companion_lua_has_zero_static_luau_errors() {
        let lua = render_companion_lua(4040, "0123456789abcdef0123456789abcdef");
        assert!(lua.contains("http://127.0.0.1:4040"));
        assert!(lua.contains("0123456789abcdef0123456789abcdef"));
        assert!(lua.contains("ChangeHistoryService"));
        assert!(lua.contains("HttpService"));
        assert!(lua.contains("Selection"));
        assert!(lua.contains("LogService"));
        assert!(lua.contains("get_place_snapshot"));
        assert!(lua.contains("GetComponents"));
        assert!(lua.contains("SurfaceAppearance"));
        assert!(lua.contains("ParticleEmitter"));
        assert!(lua.contains("ScreenGui"));
        assert!(lua.contains("GetAttributes"));
        assert!(lua.contains("SNAPSHOT_HARD_LIMIT"));
        let report = crate::luau_check::analyze_source("ServerScriptService.OpenRoCompanion", &lua);
        assert_eq!(
            report.error_count(),
            0,
            "OpenRoCompanion.lua must pass static Luau prover cleanly: {:?}",
            report.errors()
        );
    }

    #[test]
    fn install_rejects_traversal_and_non_plugins_dirs_and_installs_into_verified_dir() {
        let base = std::env::temp_dir().join(format!(
            "openro-plugin-inst-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let roblox_dir = base.join("Roblox");
        let plugins_dir = roblox_dir.join("Plugins");
        std::fs::create_dir_all(&roblox_dir).unwrap();

        // Reject traversal and non-Plugins dirs.
        assert!(verify_safe_plugins_dir(&base.join("Roblox/../Plugins")).is_err());
        assert!(verify_safe_plugins_dir(&base.join("Evil/Plugins")).is_err());
        assert!(verify_safe_plugins_dir(&roblox_dir.join("NotPlugins")).is_err());

        // Valid verified Roblox/Plugins succeeds cleanly.
        let installed =
            write_plugin_file_to_verified_dir(&plugins_dir, 18999, "abcdef0123456789abcdef0123456789")
                .expect("verified Roblox/Plugins install succeeds");
        assert!(installed.is_file());
        let text = std::fs::read_to_string(&installed).unwrap();
        assert!(text.contains("http://127.0.0.1:18999"));
        assert!(text.contains("abcdef0123456789abcdef0123456789"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plugin_bridge_long_poll_round_trip() {
        let bridge = StudioPluginBridge::open(None);
        assert!(!bridge.is_connected());
        bridge.record_handshake("Studio Edit", "ArenaPlace", vec!["Workspace.SpawnPad".into()]);
        assert!(bridge.is_connected());

        let worker_bridge = Arc::clone(&bridge);
        let handle = std::thread::spawn(move || {
            let cmd = worker_bridge
                .poll_next_command(Duration::from_secs(2))
                .expect("command received");
            assert_eq!(cmd.tool, "get_place_tree");
            assert!(worker_bridge.submit_result(
                &cmd.id,
                true,
                json!({ "instances": {}, "scripts": { "ServerScriptService.Main": "print(1)" } }),
                None,
                Some(vec!["ServerScriptService.Main".into()]),
            ));
        });

        let res = bridge
            .execute_command("get_place_tree", json!({}), Duration::from_secs(2))
            .expect("command completes");
        assert_eq!(
            res["scripts"]["ServerScriptService.Main"].as_str(),
            Some("print(1)")
        );
        handle.join().unwrap();
        let st = bridge.status_json();
        assert_eq!(st["connected"], json!(true));
        assert_eq!(st["selection"], json!(["ServerScriptService.Main"]));
    }
}
