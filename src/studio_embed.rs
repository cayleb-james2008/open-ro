//! Cross-platform Embedded Roblox Studio host manager for OpenRO v0.9.0.
//!
//! Manages an embedded Roblox Studio session strictly on explicit user action
//! (NEVER auto-launched on server boot or during automated tests):
//! - **Linux:** attaches to an existing display or spawns an isolated
//!   `Xvfb :99 -screen 0 1440x900x24 -ac`, launches Flatpak
//!   `org.vinegarhq.Vinegar` with `DISPLAY=:99`, and attaches `~/.local/bin/roblox-studio-mcp`.
//! - **Windows:** passively locates `%LOCALAPPDATA%\Roblox\Versions\version-*\RobloxStudioBeta.exe`
//!   plus `StudioMCP.exe` / `mcp.bat` without any Unix-only APIs, launches Studio when requested,
//!   and attaches `McpClient`.
//! - **Explicit child teardown:** `stop_embedded_studio` and `Drop` terminate ONLY the child
//!   process handles spawned by this manager (`child.kill()` + `child.wait()`) — never `pkill -f`,
//!   and never touching a pre-existing host `Xvfb`.

use crate::desktop::current_platform;
use crate::mcp::{official_studio_tool_catalog, McpClient, OFFICIAL_STUDIO_TOOLS};
use crate::studio_stream::{StudioFrame, StudioStreamHub};
use serde_json::{json, Value};
use std::collections::VecDeque;
#[cfg(unix)]
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default isolated X11 display used for embedded Studio on Linux.
pub const DEFAULT_EMBED_DISPLAY: &str = ":99";
/// Default embedded Studio viewport resolution.
pub const DEFAULT_EMBED_WIDTH: u32 = 1440;
pub const DEFAULT_EMBED_HEIGHT: u32 = 900;
pub const DEFAULT_EMBED_DEPTH: u8 = 24;
/// Default startup wait timeout when launching a real Studio host process (60s for Wine/Vinegar init).
pub const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 60_000;
/// Maximum JPEG frame buffer cap for the x11grab MJPEG pipe parser (8 MiB).
const MAX_MJPEG_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// User-supplied configuration for launching or attaching an embedded Roblox Studio session.
#[derive(Clone, Debug)]
pub struct EmbedConfig {
    /// Isolated Linux X11 display (e.g. `":99"`). Ignored on Windows/macOS.
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    /// Optional explicit MCP bridge command (e.g. `mock_studio`, `~/.local/bin/roblox-studio-mcp`,
    /// or `C:\...\StudioMCP.exe`). When set alongside `launch_studio_process: false`, attaches
    /// directly without launching Vinegar or `RobloxStudioBeta.exe`.
    pub mcp_command: Option<String>,
    /// Optional `.rbxlx` / `.rbxl` place file to open directly in embedded Roblox Studio.
    pub place_file: Option<PathBuf>,
    /// When `true` and no direct custom MCP bridge overrides it, spawn the host Studio runtime
    /// (`Xvfb` + Flatpak Vinegar on Linux; `RobloxStudioBeta.exe` on Windows).
    pub launch_studio_process: bool,
    /// Maximum time to wait for Studio + MCP readiness in milliseconds.
    pub startup_timeout_ms: u64,
}

impl Default for EmbedConfig {
    fn default() -> Self {
        EmbedConfig {
            display: DEFAULT_EMBED_DISPLAY.to_string(),
            width: DEFAULT_EMBED_WIDTH,
            height: DEFAULT_EMBED_HEIGHT,
            depth: DEFAULT_EMBED_DEPTH,
            mcp_command: None,
            place_file: None,
            launch_studio_process: true,
            startup_timeout_ms: DEFAULT_STARTUP_TIMEOUT_MS,
        }
    }
}

impl EmbedConfig {
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let mut cfg = Self::default();
        if let Some(d) = v.get("display").and_then(|x| x.as_str()) {
            let trimmed = d.trim();
            if !trimmed.is_empty() {
                cfg.display = validate_x11_display(trimmed)?;
            }
        }
        if let Some(w) = v.get("width").and_then(|x| x.as_u64()) {
            cfg.width = (w as u32).clamp(320, 3840);
        }
        if let Some(h) = v.get("height").and_then(|x| x.as_u64()) {
            cfg.height = (h as u32).clamp(200, 2160);
        }
        if let Some(depth) = v.get("depth").and_then(|x| x.as_u64()) {
            cfg.depth = match depth {
                16 => 16,
                32 => 32,
                _ => 24,
            };
        }
        if let Some(cmd) = v
            .get("mcp_command")
            .or_else(|| v.get("command"))
            .and_then(|x| x.as_str())
        {
            let trimmed = cmd.trim();
            if !trimmed.is_empty() {
                if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
                    return Err("mcp_command must not contain control characters".to_string());
                }
                cfg.mcp_command = Some(trimmed.to_string());
                // Explicit custom MCP command defaults to direct attach unless caller forces launch.
                if v.get("launch_studio_process").is_none() {
                    cfg.launch_studio_process = false;
                }
            }
        }
        if let Some(pf) = v
            .get("place_file")
            .or_else(|| v.get("rbxlx_path"))
            .and_then(|x| x.as_str())
        {
            let trimmed = pf.trim();
            if !trimmed.is_empty() {
                if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
                    return Err("place_file must not contain control characters".to_string());
                }
                let p = PathBuf::from(trimmed);
                if p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("place_file must not contain '..' traversal segments".to_string());
                }
                cfg.place_file = Some(p);
            }
        }
        if let Some(launch) = v.get("launch_studio_process").and_then(|x| x.as_bool()) {
            cfg.launch_studio_process = launch;
        }
        if let Some(ms) = v.get("startup_timeout_ms").and_then(|x| x.as_u64()) {
            cfg.startup_timeout_ms = ms.clamp(500, 180_000);
        }
        Ok(cfg)
    }
}

/// Discovered Windows Roblox Studio binaries under `%LOCALAPPDATA%\Roblox`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowsRobloxPaths {
    pub version_dir: Option<PathBuf>,
    pub studio_exe: Option<PathBuf>,
    pub studio_mcp_exe: Option<PathBuf>,
    pub mcp_bat: Option<PathBuf>,
}

/// Pure filesystem scanner for `%LOCALAPPDATA%\Roblox\Versions\version-*\RobloxStudioBeta.exe`,
/// `StudioMCP.exe`, and `%LOCALAPPDATA%\Roblox\mcp.bat`. Unit-testable on Linux and Windows.
pub fn find_windows_roblox_paths(local_appdata: &Path) -> WindowsRobloxPaths {
    let roblox_root = local_appdata.join("Roblox");
    let mcp_bat_candidate = roblox_root.join("mcp.bat");
    let mcp_bat = if mcp_bat_candidate.is_file() {
        Some(mcp_bat_candidate)
    } else {
        None
    };

    let versions_dir = roblox_root.join("Versions");
    let mut candidates: Vec<(SystemTime, String, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().starts_with("version-") {
                continue;
            }
            let has_studio = path.join("RobloxStudioBeta.exe").is_file();
            let has_mcp = path.join("StudioMCP.exe").is_file();
            if has_studio || has_mcp {
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH);
                candidates.push((modified, name, path));
            }
        }
    }
    // Prefer newest version directory (tie-break deterministically by directory name).
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    if let Some((_, _, best_dir)) = candidates.into_iter().next() {
        let studio = best_dir.join("RobloxStudioBeta.exe");
        let mcp = best_dir.join("StudioMCP.exe");
        WindowsRobloxPaths {
            studio_exe: if studio.is_file() { Some(studio) } else { None },
            studio_mcp_exe: if mcp.is_file() { Some(mcp) } else { None },
            version_dir: Some(best_dir),
            mcp_bat,
        }
    } else {
        WindowsRobloxPaths {
            version_dir: None,
            studio_exe: None,
            studio_mcp_exe: None,
            mcp_bat,
        }
    }
}

/// Passive host-environment probe (NEVER spawns Vinegar or Roblox Studio).
#[derive(Clone, Debug)]
pub struct HostStudioDetection {
    pub platform: &'static str,
    pub studio_installed: bool,
    pub studio_running: bool,
    pub studio_display: Option<String>,
    pub already_logged_in: bool,
    pub place_open: bool,
    pub running_studios: Vec<Value>,
    pub xvfb_available: bool,
    pub display_ready: bool,
    pub studio_exe: Option<String>,
    pub mcp_command: Option<String>,
    pub guidance: String,
}

impl HostStudioDetection {
    pub fn to_json(&self) -> Value {
        json!({
            "platform": self.platform,
            "studio_installed": self.studio_installed,
            "studio_running": self.studio_running,
            "studio_display": self.studio_display,
            "already_logged_in": self.already_logged_in,
            "place_open": self.place_open,
            "running_studios": self.running_studios,
            "xvfb_available": self.xvfb_available,
            "display_ready": self.display_ready,
            "studio_exe": self.studio_exe,
            "mcp_command": self.mcp_command,
            "guidance": self.guidance,
        })
    }
}

/// Normalize an X11 `DISPLAY` string (e.g. `:99.0` -> `:99`) if valid.
pub fn normalize_x11_display_env(raw: &str) -> Option<String> {
    let base = raw.trim().split('.').next().unwrap_or("").trim();
    validate_x11_display(base).ok()
}

/// Scan `/proc/<pid>/{cmdline,environ}` on Linux to detect an already-running
/// `RobloxStudioBeta.exe` / Vinegar process and its actual `DISPLAY` (e.g. `:0`, `:1`, `:99`) (#118).
#[cfg(target_os = "linux")]
pub fn detect_linux_running_studio_display() -> (bool, Option<String>) {
    let mut found_running = false;
    let mut found_display: Option<String> = None;
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let pid_str = name.to_string_lossy();
            if !pid_str.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let pdir = entry.path();
            let Ok(cmd_bytes) = std::fs::read(pdir.join("cmdline")) else {
                continue;
            };
            if cmd_bytes.is_empty() {
                continue;
            }
            let cmd_text = String::from_utf8_lossy(&cmd_bytes);
            let is_studio = (cmd_text.contains("RobloxStudioBeta.exe")
                && !cmd_text.contains("msedgewebview2.exe"))
                || (cmd_text.contains("vinegar") && cmd_text.contains("studio"));
            if !is_studio {
                continue;
            }
            found_running = true;
            if let Ok(env_bytes) = std::fs::read(pdir.join("environ")) {
                for item in env_bytes.split(|b| *b == 0) {
                    if let Some(val) = item.strip_prefix(b"DISPLAY=") {
                        if let Some(disp) = normalize_x11_display_env(&String::from_utf8_lossy(val)) {
                            found_display = Some(disp);
                            break;
                        }
                    }
                }
            }
            if found_display.is_some() {
                break;
            }
        }
    }
    if !found_running && is_vinegar_flatpak_running() {
        found_running = true;
    }
    if found_running && found_display.is_none() && x11_socket_exists(DEFAULT_EMBED_DISPLAY) {
        found_display = Some(DEFAULT_EMBED_DISPLAY.to_string());
    }
    (found_running, found_display)
}

#[cfg(target_os = "macos")]
fn detect_macos_studio_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "RobloxStudio"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn detect_windows_studio_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq RobloxStudioBeta.exe", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .to_ascii_lowercase()
                    .contains("robloxstudiobeta.exe")
        })
        .unwrap_or(false)
}

/// Determine whether a `get_studio_state` JSON value or error string indicates that
/// an actual Roblox place is open vs. Studio sitting on the Start Page (#47).
pub fn infer_place_open_from_state(state_res: Result<&Value, &str>) -> (bool, Option<String>) {
    match state_res {
        Err(err) => {
            let low = err.to_ascii_lowercase();
            if low.contains("place is not open")
                || low.contains("no place")
                || low.contains("unavailable")
                || low.contains("start page")
            {
                (false, Some("Place is not open".to_string()))
            } else {
                (true, None)
            }
        }
        Ok(val) => {
            if val.get("place_open").and_then(|v| v.as_bool()) == Some(false) {
                let lbl = val
                    .get("state")
                    .or_else(|| val.get("mode"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Place is not open")
                    .to_string();
                return (false, Some(lbl));
            }
            let mode = val
                .get("mode")
                .or_else(|| val.get("studio_mode"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(m) = mode {
                if m.eq_ignore_ascii_case("unavailable")
                    || m.eq_ignore_ascii_case("startpage")
                    || m.eq_ignore_ascii_case("start_page")
                    || m.eq_ignore_ascii_case("none")
                {
                    let lbl = val
                        .get("state")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("Place is not open")
                        .to_string();
                    return (false, Some(lbl));
                }
            }
            let active_dm = val
                .get("active_datamodel")
                .or_else(|| val.get("datamodel_type"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if active_dm.is_some_and(|dm| dm.eq_ignore_ascii_case("unavailable")) {
                return (false, Some("Place is not open".to_string()));
            }
            let serialized_low = val.to_string().to_ascii_lowercase();
            if serialized_low.contains("place is not open")
                || serialized_low.contains("no place open")
                || serialized_low.contains("start page")
            {
                return (false, Some("Place is not open".to_string()));
            }
            if let Some(dms) = val.get("datamodel_types").and_then(|v| v.as_array()) {
                if dms.is_empty() && active_dm.is_none() {
                    return (false, Some("Place is not open".to_string()));
                }
            }
            let lbl = mode.or(active_dm).map(String::from);
            (true, lbl)
        }
    }
}

/// Query an active `McpClient` for whether a Roblox place is open (vs. Start Page) (#47).
pub fn probe_client_place_open(client: &mut McpClient) -> bool {
    if !client.has_exact_tool("get_studio_state") {
        return client.studio_id().is_some();
    }
    match client.get_studio_state() {
        Ok(val) => infer_place_open_from_state(Ok(&val)).0,
        Err(err) => infer_place_open_from_state(Err(err.as_str())).0,
    }
}

/// Extract normalized `[{ studio_id, place_name, datamodel_type, place_open, studio_state }]`
/// records from a `list_roblox_studios` MCP JSON response (#47, #118).
pub fn parse_running_studios(val: &Value) -> Vec<Value> {
    let arr = val
        .get("studios")
        .and_then(|v| v.as_array())
        .or_else(|| val.as_array());
    let Some(items) = arr else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(id) = item
            .get("studio_id")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let place_name = item
            .get("place_name")
            .or_else(|| item.get("name"))
            .or_else(|| item.get("file"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let (inferred_open, inferred_state) = infer_place_open_from_state(Ok(item));
        let dm = if !inferred_open {
            "Unavailable".to_string()
        } else {
            item.get("active_datamodel")
                .or_else(|| item.get("datamodel_type"))
                .or_else(|| item.get("mode"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Edit")
                .to_string()
        };
        out.push(json!({
            "studio_id": id,
            "place_name": place_name,
            "datamodel_type": dm,
            "place_open": inferred_open,
            "studio_state": inferred_state,
        }));
    }
    out
}

/// Passively query an MCP command for live Studio sessions and whether a place is open
/// (`get_studio_state`) without spawning Studio (#47, #118).
pub fn probe_running_studios(mcp_cmd: &str) -> Vec<Value> {
    if std::env::var("OPENRO_NO_STATUS_MCP_PROBE")
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
    {
        return Vec::new();
    }
    let Ok(mut client) = McpClient::spawn_with_timeout(mcp_cmd, Duration::from_secs(2)) else {
        return Vec::new();
    };
    let Ok(val) = client.list_roblox_studios() else {
        return Vec::new();
    };
    let mut studios = parse_running_studios(&val);
    if !studios.is_empty() && client.has_exact_tool("get_studio_state") {
        let state_res = client.get_studio_state();
        let (open, state_label) = match &state_res {
            Ok(v) => infer_place_open_from_state(Ok(v)),
            Err(e) => infer_place_open_from_state(Err(e.as_str())),
        };
        let active_dm = state_res
            .as_ref()
            .ok()
            .and_then(|v| v.get("active_datamodel"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());
        for s in &mut studios {
            if let Some(map) = s.as_object_mut() {
                let item_open = map
                    .get("place_open")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let final_open = item_open && open;
                map.insert("place_open".to_string(), json!(final_open));
                if let Some(ref lbl) = state_label {
                    map.insert("studio_state".to_string(), json!(lbl));
                }
                if !final_open {
                    map.insert("datamodel_type".to_string(), json!("Unavailable"));
                } else if let Some(dm) = active_dm {
                    map.insert("datamodel_type".to_string(), json!(dm));
                }
            }
        }
    }
    studios
}

/// Passively detect Roblox Studio & MCP availability across Linux, macOS, and Windows
/// without launching anything (#118). When `probe_mcp` is `true` and Studio is running,
/// performs a short passive `list_roblox_studios` probe (respecting `OPENRO_NO_STATUS_MCP_PROBE=1`).
pub fn detect_studio_environment() -> HostStudioDetection {
    detect_studio_environment_with_probe(true)
}

/// Fast passive detection without spawning any MCP probe subprocess (used during server startup).
pub fn detect_studio_environment_fast() -> HostStudioDetection {
    detect_studio_environment_with_probe(false)
}

fn detect_studio_environment_with_probe(probe_mcp: bool) -> HostStudioDetection {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let mcp_wrapper = PathBuf::from(&home).join(".local/bin/roblox-studio-mcp");
        let vinegar_prefix = PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar");
        let mcp_present = mcp_wrapper.is_file();
        let xvfb_available = find_on_path("Xvfb").is_some();
        let (studio_running, studio_display) = detect_linux_running_studio_display();
        let display_ready = studio_display
            .as_deref()
            .is_some_and(x11_socket_exists)
            || x11_socket_exists(DEFAULT_EMBED_DISPLAY);
        let flatpak_installed = find_on_path("flatpak").is_some() && vinegar_prefix.exists();
        let mcp_command = if mcp_present {
            Some(mcp_wrapper.to_string_lossy().into_owned())
        } else {
            None
        };
        let running_studios = if probe_mcp && studio_running {
            mcp_command
                .as_deref()
                .map(probe_running_studios)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let already_logged_in = !running_studios.is_empty();
        let place_open = running_studios
            .iter()
            .any(|s| s.get("place_open").and_then(|v| v.as_bool()).unwrap_or(false));

        let guidance = if already_logged_in && place_open {
            "Found your logged-in Roblox Studio — click 'Grab & Connect' to attach immediately."
                .to_string()
        } else if already_logged_in {
            "Studio is on the Start Page — open a blank Baseplate to sync/playtest.".to_string()
        } else if studio_running && mcp_present {
            "Roblox Studio is running (awaiting sign-in or open place) — click 'Launch & Embed Studio' to stream and sign in live."
                .to_string()
        } else if flatpak_installed && mcp_present {
            "Click 'Open My Roblox Studio' to launch using your saved login and attach live MCP."
                .to_string()
        } else {
            "Install Flatpak org.vinegarhq.Vinegar + ~/.local/bin/roblox-studio-mcp, or connect a custom Studio MCP command."
                .to_string()
        };

        HostStudioDetection {
            platform: current_platform(),
            studio_installed: flatpak_installed || studio_running,
            studio_running,
            studio_display,
            already_logged_in,
            place_open,
            running_studios,
            xvfb_available,
            display_ready,
            studio_exe: if flatpak_installed {
                Some("flatpak:org.vinegarhq.Vinegar".to_string())
            } else {
                None
            },
            mcp_command,
            guidance,
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = probe_mcp;
        let studio_app = PathBuf::from("/Applications/RobloxStudio.app");
        let studio_running = detect_macos_studio_running();
        let studio_installed = studio_app.exists() || studio_running;
        HostStudioDetection {
            platform: current_platform(),
            studio_installed,
            studio_running,
            studio_display: None,
            already_logged_in: false,
            place_open: false,
            running_studios: Vec::new(),
            xvfb_available: false,
            display_ready: true,
            studio_exe: studio_app.exists().then(|| studio_app.to_string_lossy().into_owned()),
            mcp_command: None,
            guidance: if studio_running {
                "Roblox Studio is running on macOS — connect your Studio MCP command to attach."
                    .to_string()
            } else {
                "Open Roblox Studio to attach live MCP.".to_string()
            },
        }
    }

    #[cfg(windows)]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let paths = local_appdata
            .as_deref()
            .map(find_windows_roblox_paths)
            .unwrap_or_default();
        let studio_running = detect_windows_studio_running();
        let studio_installed = paths.studio_exe.is_some() || studio_running;
        let mcp_command = paths
            .studio_mcp_exe
            .as_ref()
            .map(|p| format!("\"{}\"", p.display()))
            .or_else(|| {
                paths
                    .mcp_bat
                    .as_ref()
                    .map(|p| format!("cmd /C \"{}\"", p.display()))
            });
        let running_studios = if probe_mcp && studio_running {
            mcp_command
                .as_deref()
                .map(probe_running_studios)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let already_logged_in = !running_studios.is_empty();
        let place_open = running_studios
            .iter()
            .any(|s| s.get("place_open").and_then(|v| v.as_bool()).unwrap_or(false));
        let guidance = if already_logged_in && place_open {
            "Found your logged-in Roblox Studio — click 'Grab & Connect' to attach immediately."
                .to_string()
        } else if already_logged_in {
            "Studio is on the Start Page — open a blank Baseplate to sync/playtest.".to_string()
        } else if studio_running && mcp_command.is_some() {
            "Roblox Studio is running — sign in or open a place to attach MCP.".to_string()
        } else if studio_installed && mcp_command.is_some() {
            "Native Windows Roblox Studio & StudioMCP.exe detected — click 'Open My Roblox Studio' to connect."
                .to_string()
        } else if studio_installed {
            "Roblox Studio detected in %LOCALAPPDATA%\\Roblox\\Versions; enable Studio MCP in Beta Features."
                .to_string()
        } else {
            "Install Roblox Studio from create.roblox.com to enable live embedded Studio sync."
                .to_string()
        };

        HostStudioDetection {
            platform: current_platform(),
            studio_installed,
            studio_running,
            studio_display: None,
            already_logged_in,
            place_open,
            running_studios,
            xvfb_available: false,
            display_ready: true,
            studio_exe: paths.studio_exe.map(|p| p.to_string_lossy().into_owned()),
            mcp_command,
            guidance,
        }
    }
}

/// Cross-platform passive MCP command discovery (never spawns Studio).
pub fn detect_host_mcp_command() -> Option<String> {
    #[cfg(unix)]
    {
        McpClient::detect_vinegar_studio_mcp()
    }
    #[cfg(windows)]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let paths = find_windows_roblox_paths(&local_appdata);
        if let Some(mcp) = paths.studio_mcp_exe {
            return Some(format!("\"{}\"", mcp.display()));
        }
        if let Some(bat) = paths.mcp_bat {
            return Some(format!("cmd /C \"{}\"", bat.display()));
        }
        None
    }
}

/// Snapshot of the embedded Roblox Studio runtime state returned by status/launch/stop endpoints.
#[derive(Clone, Debug)]
pub struct StudioEmbedState {
    pub phase: String,
    pub platform: &'static str,
    pub embedded: bool,
    pub mcp_connected: bool,
    pub place_open: bool,
    pub transferred: bool,
    pub display: Option<String>,
    pub resolution: [u32; 2],
    pub studio_id: Option<String>,
    pub datamodel_type: String,
    pub playing: bool,
    pub xvfb_pid: Option<u32>,
    pub studio_pid: Option<u32>,
    pub ffmpeg_pid: Option<u32>,
    pub x11grab_active: bool,
    pub place_file: Option<String>,
    pub mcp_command: Option<String>,
    pub detected_studio_exe: Option<String>,
    pub detected_mcp_exe: Option<String>,
    pub tools: Vec<String>,
    pub official_tools_count: usize,
    pub last_error: Option<String>,
    pub last_input: Option<String>,
    pub started_at_ms: Option<u64>,
}

impl StudioEmbedState {
    pub fn idle() -> Self {
        let probe = detect_studio_environment_fast();
        StudioEmbedState {
            phase: "idle".to_string(),
            platform: probe.platform,
            embedded: false,
            mcp_connected: false,
            place_open: false,
            transferred: false,
            display: if cfg!(unix) {
                probe
                    .studio_display
                    .or_else(|| Some(DEFAULT_EMBED_DISPLAY.to_string()))
            } else {
                None
            },
            resolution: [DEFAULT_EMBED_WIDTH, DEFAULT_EMBED_HEIGHT],
            studio_id: None,
            datamodel_type: "Edit".to_string(),
            playing: false,
            xvfb_pid: None,
            studio_pid: None,
            ffmpeg_pid: None,
            x11grab_active: false,
            place_file: None,
            mcp_command: None,
            detected_studio_exe: probe.studio_exe,
            detected_mcp_exe: probe.mcp_command,
            tools: Vec::new(),
            official_tools_count: 0,
            last_error: None,
            last_input: None,
            started_at_ms: None,
        }
    }

    pub fn to_json(&self) -> Value {
        let detection = detect_studio_environment();
        self.to_json_with_detection(&detection)
    }

    pub fn to_json_with_detection(&self, detection: &HostStudioDetection) -> Value {
        let mut det_json = detection.to_json();
        if self.phase == "connected" && self.place_open {
            if let Some(m) = det_json.as_object_mut() {
                m.insert("already_logged_in".to_string(), json!(true));
                m.insert("place_open".to_string(), json!(true));
                m.insert(
                    "guidance".to_string(),
                    json!("Roblox Studio is connected and ready to sync and playtest."),
                );
            }
        }
        json!({
            "phase": self.phase,
            "platform": self.platform,
            "embedded": self.embedded,
            "mcp_connected": self.mcp_connected,
            "place_open": self.place_open,
            "transferred": self.transferred,
            "display": self.display,
            "resolution": self.resolution,
            "studio_id": self.studio_id,
            "datamodel_type": self.datamodel_type,
            "playing": self.playing,
            "xvfb_pid": self.xvfb_pid,
            "studio_pid": self.studio_pid,
            "ffmpeg_pid": self.ffmpeg_pid,
            "x11grab_active": self.x11grab_active,
            "place_file": self.place_file,
            "mcp_command": self.mcp_command,
            "detected_studio_exe": self.detected_studio_exe,
            "detected_mcp_exe": self.detected_mcp_exe,
            "tools": self.tools,
            "official_tools_count": self.official_tools_count,
            "official_tools_total": OFFICIAL_STUDIO_TOOLS.len(),
            "tool_catalog": official_studio_tool_catalog(),
            "detection": det_json,
            "last_error": self.last_error,
            "last_input": self.last_input,
            "started_at_ms": self.started_at_ms,
        })
    }
}

/// Owns any child processes (`Xvfb`, Vinegar / `RobloxStudioBeta.exe`, `ffmpeg` x11grab pump)
/// spawned by OpenRO for an embedded Studio session, guaranteeing explicit child-only cleanup
/// on `stop` and `Drop`.
pub struct StudioEmbedManager {
    xvfb_child: Option<Child>,
    studio_child: Option<Child>,
    ffmpeg_child: Option<Child>,
    x11grab_stop: Arc<AtomicBool>,
    state: StudioEmbedState,
}

impl Default for StudioEmbedManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StudioEmbedManager {
    /// Creates an idle manager. NEVER spawns Xvfb, Vinegar, ffmpeg, or Roblox Studio automatically.
    pub fn new() -> Self {
        StudioEmbedManager {
            xvfb_child: None,
            studio_child: None,
            ffmpeg_child: None,
            x11grab_stop: Arc::new(AtomicBool::new(false)),
            state: StudioEmbedState::idle(),
        }
    }

    pub fn state(&self) -> StudioEmbedState {
        self.state.clone()
    }

    /// Synchronize live metadata from an active `McpClient` (e.g. after connect/tool call).
    pub fn sync_from_client(&mut self, client: &McpClient, mcp_command: Option<String>) {
        self.sync_connected_meta(
            client.studio_id().map(ToString::to_string),
            client.datamodel_type().to_string(),
            client.tools().to_vec(),
            client.official_tools_available().len(),
            mcp_command,
        );
    }

    /// Atomically mark the embedded session `connected` using pre-extracted MCP metadata
    /// (called after `PlaceBackend::Mcp` and its initial frame are already attached).
    pub fn sync_connected_meta(
        &mut self,
        studio_id: Option<String>,
        datamodel_type: String,
        tools: Vec<String>,
        official_tools_count: usize,
        mcp_command: Option<String>,
    ) {
        self.state.mcp_connected = true;
        self.state.embedded = true;
        self.state.phase = "connected".to_string();
        self.state.studio_id = studio_id;
        self.state.datamodel_type = datamodel_type;
        self.state.tools = tools;
        self.state.official_tools_count = official_tools_count;
        if let Some(cmd) = mcp_command {
            self.state.mcp_command = Some(cmd);
        }
        self.state.last_error = None;
    }

    pub fn mark_disconnected(&mut self) {
        self.state.mcp_connected = false;
        self.state.place_open = false;
        if self.studio_child.is_none() && self.xvfb_child.is_none() && self.ffmpeg_child.is_none() {
            self.state.embedded = false;
            self.state.phase = "idle".to_string();
        }
        self.state.studio_id = None;
        self.state.tools.clear();
        self.state.official_tools_count = 0;
        self.state.playing = false;
    }

    pub fn set_place_open(&mut self, open: bool) {
        self.state.place_open = open;
    }

    pub fn set_transferred(&mut self, transferred: bool) {
        self.state.transferred = transferred;
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.state.playing = playing;
    }

    pub fn record_input(&mut self, summary: impl Into<String>) {
        self.state.last_input = Some(summary.into());
    }

    pub fn launch(&mut self, config: EmbedConfig) -> Result<(StudioEmbedState, McpClient), String> {
        launch_embedded_studio(self, config)
    }

    pub fn launch_with_hub(
        &mut self,
        config: EmbedConfig,
        hub: Option<Arc<StudioStreamHub>>,
    ) -> Result<(StudioEmbedState, McpClient), String> {
        launch_embedded_studio_with_hub(self, config, hub)
    }

    pub fn stop(&mut self) -> StudioEmbedState {
        stop_embedded_studio(self)
    }

    /// Stop flag toggled to `true` whenever `stop()` or `Drop` runs — shared by
    /// the x11grab frame pump and any background MCP auto-attacher thread (#35).
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.x11grab_stop)
    }
}

impl Drop for StudioEmbedManager {
    fn drop(&mut self) {
        let _ = stop_embedded_studio(self);
    }
}

/// Extract all complete JPEG frames (`0xFF 0xD8` .. `0xFF 0xD9`) from a streaming byte buffer
/// (such as `ffmpeg -f mpjpeg pipe:1`), preserving any partial trailing frame for the next read.
pub fn extract_jpeg_frames(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut scan_pos = 0usize;

    loop {
        let Some(rel_soi) = buf[scan_pos..].windows(2).position(|w| w == [0xFF, 0xD8]) else {
            // No SOI in remainder: discard prefix bytes, keeping a trailing 0xFF if split across chunks.
            let keep_last = buf.last() == Some(&0xFF) && buf.len() > scan_pos;
            let drain_to = if keep_last {
                buf.len() - 1
            } else {
                buf.len()
            };
            if drain_to > 0 {
                buf.drain(..drain_to);
            }
            return frames;
        };
        let soi = scan_pos + rel_soi;
        let search_eoi_start = soi + 2;
        let Some(rel_eoi) = buf
            .get(search_eoi_start..)
            .and_then(|tail| tail.windows(2).position(|w| w == [0xFF, 0xD9]))
        else {
            // Incomplete frame: discard leading garbage before SOI and enforce max frame cap.
            if soi > 0 {
                buf.drain(..soi);
            }
            if buf.len() > MAX_MJPEG_FRAME_BYTES {
                buf.clear();
            }
            return frames;
        };
        let eoi_end = search_eoi_start + rel_eoi + 2;
        frames.push(buf[soi..eoi_end].to_vec());
        scan_pos = eoi_end;
        if scan_pos >= buf.len() {
            buf.clear();
            return frames;
        }
    }
}

/// Spawn a real-time 12 FPS Linux X11 MJPEG stream pump (`ffmpeg -f x11grab ... -f mpjpeg pipe:1`)
/// that parses `0xFFD8..0xFFD9` frames directly into `hub.push_frame(...)` -> `/api/studio/stream.mjpg`.
///
/// Child handle is stored in `manager.ffmpeg_child` and torn down cleanly in `stop_embedded_studio` / `Drop`.
pub fn spawn_x11grab_pump(
    manager: &mut StudioEmbedManager,
    hub: Arc<StudioStreamHub>,
    display: &str,
    width: u32,
    height: u32,
) -> Result<u32, String> {
    let valid_display = validate_x11_display(display)?;
    let w = width.clamp(320, 3840);
    let h = height.clamp(200, 2160);

    // Stop any previous ffmpeg pump owned by this manager first.
    manager.x11grab_stop.store(true, Ordering::SeqCst);
    if let Some(mut prev) = manager.ffmpeg_child.take() {
        let _ = prev.kill();
        let _ = prev.wait();
    }
    manager.state.ffmpeg_pid = None;
    manager.state.x11grab_active = false;

    #[cfg(unix)]
    {
        let video_size = format!("{w}x{h}");
        let mut child = Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-f",
                "x11grab",
                "-framerate",
                "12",
                "-video_size",
                &video_size,
                "-i",
                &valid_display,
                "-c:v",
                "mjpeg",
                "-q:v",
                "6",
                "-f",
                "mpjpeg",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn ffmpeg x11grab on {valid_display}: {e}"))?;

        let pid = child.id();
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("ffmpeg x11grab stdout was not piped".to_string());
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        manager.x11grab_stop = Arc::clone(&stop_flag);
        manager.ffmpeg_child = Some(child);
        manager.state.ffmpeg_pid = Some(pid);
        manager.state.x11grab_active = true;

        std::thread::spawn(move || {
            let mut read_buf = [0u8; 32 * 1024];
            let mut acc: Vec<u8> = Vec::with_capacity(128 * 1024);
            while !stop_flag.load(Ordering::Relaxed) {
                match stdout.read(&mut read_buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        acc.extend_from_slice(&read_buf[..n]);
                        for jpeg in extract_jpeg_frames(&mut acc) {
                            let frame = StudioFrame {
                                seq: 0,
                                ts_ms: now_ms(),
                                mime: "image/jpeg".to_string(),
                                bytes: jpeg,
                                width: w,
                                height: h,
                                source: "x11grab_mjpeg".to_string(),
                            };
                            hub.push_frame(frame);
                        }
                    }
                }
            }
        });

        Ok(pid)
    }

    #[cfg(not(unix))]
    {
        let _ = (hub, valid_display, w, h);
        Err("x11grab MJPEG pump is only supported on Linux/Unix (use MCP screen_capture on Windows)"
            .to_string())
    }
}

/// Outcome of `launch_or_attach_live` (#35): either live MCP is already connected, or
/// the X11 viewport stream (`x11grab` MJPEG) is live immediately while waiting for the
/// user to sign into Roblox Studio / open a place (with background MCP auto-attach).
pub enum EmbedLaunchOutcome {
    Connected(StudioEmbedState, Box<McpClient>),
    StreamingAwaitingAuth {
        state: StudioEmbedState,
        mcp_cmd: String,
    },
}

/// Probe the actual width/height of an existing Linux X11 display via `libX11.so.6`
/// so `x11grab` captures the full Xvfb desktop (e.g. `1440x900`) instead of cropping.
#[cfg(unix)]
pub fn probe_x11_resolution(display: &str) -> Option<[u32; 2]> {
    let valid = validate_x11_display(display).ok()?;
    let script = r#"
import ctypes, sys
x11 = ctypes.CDLL("libX11.so.6")
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XDefaultScreen.argtypes = [ctypes.c_void_p]
x11.XDefaultScreen.restype = ctypes.c_int
x11.XDisplayWidth.argtypes = [ctypes.c_void_p, ctypes.c_int]
x11.XDisplayWidth.restype = ctypes.c_int
x11.XDisplayHeight.argtypes = [ctypes.c_void_p, ctypes.c_int]
x11.XDisplayHeight.restype = ctypes.c_int
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
dpy = x11.XOpenDisplay(sys.argv[1].encode())
if not dpy:
    sys.exit(1)
scr = x11.XDefaultScreen(dpy)
w = x11.XDisplayWidth(dpy, scr)
h = x11.XDisplayHeight(dpy, scr)
x11.XCloseDisplay(dpy)
print(f"{w} {h}")
"#;
    let out = Command::new("python3")
        .args(["-c", script, &valid])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let w: u32 = parts.next()?.parse().ok()?;
    let h: u32 = parts.next()?.parse().ok()?;
    if (320..=3840).contains(&w) && (200..=2160).contains(&h) {
        Some([w, h])
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn probe_x11_resolution(_display: &str) -> Option<[u32; 2]> {
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedMouseCmd {
    pub action: String,
    pub x: i32,
    pub y: i32,
    pub norm_x: f64,
    pub norm_y: f64,
    pub to_x: Option<i32>,
    pub to_y: Option<i32>,
    pub to_norm_x: Option<f64>,
    pub to_norm_y: Option<f64>,
    pub button: String,
    pub delta_y: f64,
}

fn resolve_coord_pair(
    rx: f64,
    ry: f64,
    normalized_hint: Option<bool>,
    w: f64,
    h: f64,
    label: &str,
) -> Result<(i32, i32, f64, f64), String> {
    if !rx.is_finite() || !ry.is_finite() {
        return Err(format!("{label} coordinates must be finite numbers"));
    }
    if rx < 0.0 || ry < 0.0 || rx > 8192.0 || ry > 8192.0 {
        return Err(format!("{label} coordinates out of range: ({rx}, {ry})"));
    }
    let is_norm = match normalized_hint {
        Some(true) => {
            if rx > 1.0 || ry > 1.0 {
                return Err(format!(
                    "normalized {label} coordinates must be within 0.0..=1.0: ({rx}, {ry})"
                ));
            }
            true
        }
        Some(false) => false,
        None => rx <= 1.0 && ry <= 1.0,
    };
    let (px, py, nx, ny) = if is_norm {
        let px = (rx * w).round() as i32;
        let py = (ry * h).round() as i32;
        (
            px.clamp(0, (w as i32) - 1),
            py.clamp(0, (h as i32) - 1),
            (rx * 10_000.0).round() / 10_000.0,
            (ry * 10_000.0).round() / 10_000.0,
        )
    } else {
        let px = (rx.round() as i32).clamp(0, (w as i32) - 1);
        let py = (ry.round() as i32).clamp(0, (h as i32) - 1);
        (
            px,
            py,
            (((px as f64) / w.max(1.0)) * 10_000.0).round() / 10_000.0,
            (((py as f64) / h.max(1.0)) * 10_000.0).round() / 10_000.0,
        )
    };
    Ok((px, py, nx, ny))
}

/// Validate and normalize a CUA mouse request (`click`, `double_click`, `right_click`, `move`, `drag`, `scroll`, `down`, `up`).
pub fn validate_cua_mouse_payload(
    payload: &Value,
    resolution: [u32; 2],
) -> Result<ValidatedMouseCmd, String> {
    let raw_action = payload
        .get("action")
        .or_else(|| payload.get("event"))
        .and_then(|v| v.as_str())
        .unwrap_or("click")
        .trim()
        .to_ascii_lowercase();
    let action = match raw_action.as_str() {
        "click" | "double_click" | "right_click" | "move" | "drag" | "down" | "up" => raw_action,
        "scroll" | "wheel" => "scroll".to_string(),
        other => return Err(format!("unsupported mouse action '{other}'")),
    };
    let w = resolution[0].clamp(320, 3840) as f64;
    let h = resolution[1].clamp(200, 2160) as f64;
    let norm_hint = payload.get("normalized").and_then(|v| v.as_bool());
    let rx = payload
        .get("from_x")
        .or_else(|| payload.get("x"))
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "mouse input requires numeric x/from_x coordinate".to_string())?;
    let ry = payload
        .get("from_y")
        .or_else(|| payload.get("y"))
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "mouse input requires numeric y/from_y coordinate".to_string())?;
    let (x, y, norm_x, norm_y) = resolve_coord_pair(rx, ry, norm_hint, w, h, "start")?;

    let (to_x, to_y, to_norm_x, to_norm_y) = if action == "drag" {
        let tx = payload
            .get("to_x")
            .or_else(|| payload.get("end_x"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "drag action requires numeric to_x/end_x".to_string())?;
        let ty = payload
            .get("to_y")
            .or_else(|| payload.get("end_y"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "drag action requires numeric to_y/end_y".to_string())?;
        let (px2, py2, nx2, ny2) = resolve_coord_pair(tx, ty, norm_hint, w, h, "target")?;
        (Some(px2), Some(py2), Some(nx2), Some(ny2))
    } else {
        (None, None, None, None)
    };

    let button = match payload
        .get("button")
        .or_else(|| payload.get("mouse_button"))
        .and_then(|v| v.as_str())
        .unwrap_or(if action == "right_click" { "right" } else { "left" })
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "right" | "2" => "right".to_string(),
        "middle" => "middle".to_string(),
        _ => "left".to_string(),
    };
    let delta_y = payload
        .get("delta_y")
        .or_else(|| payload.get("deltaY"))
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| {
            if payload.get("direction").and_then(|v| v.as_str()) == Some("up") {
                -120.0
            } else {
                120.0
            }
        });
    if !delta_y.is_finite() {
        return Err("scroll delta_y must be finite".to_string());
    }

    Ok(ValidatedMouseCmd {
        action,
        x,
        y,
        norm_x,
        norm_y,
        to_x,
        to_y,
        to_norm_x,
        to_norm_y,
        button,
        delta_y,
    })
}

fn normalize_hotkey_token(raw: &str) -> Result<String, String> {
    let t = raw.trim().to_ascii_lowercase();
    if t.is_empty() || t.contains('\0') || t.chars().any(|c| c.is_control()) {
        return Err("hotkey token must be non-empty without control characters".to_string());
    }
    let norm = match t.as_str() {
        "control" | "control_l" | "control_r" | "leftcontrol" | "rightcontrol" => "ctrl",
        "shift_l" | "shift_r" | "leftshift" | "rightshift" => "shift",
        "alt_l" | "alt_r" | "leftalt" | "rightalt" | "option" => "alt",
        "cmd" | "command" | "win" | "windows" | "meta" | "meta_l" | "meta_r" | "super"
        | "super_l" | "super_r" | "hyper" | "hyper_l" | "hyper_r" => "super",
        "esc" => "escape",
        "del" => "delete",
        "ret" | "return" => "enter",
        other => other,
    };
    Ok(norm.to_string())
}

/// Normalize a list of hotkey tokens (e.g. `["ctrl", "shift", "p"]`).
pub fn normalize_hotkey_tokens(keys: &[String]) -> Result<Vec<String>, String> {
    if keys.is_empty() || keys.len() > 5 {
        return Err("hotkey must contain between 1 and 5 keys".to_string());
    }
    let mut out = Vec::with_capacity(keys.len());
    for k in keys {
        out.push(normalize_hotkey_token(k)?);
    }
    Ok(out)
}

/// Strict OS-escape safety denylist for Studio CUA keyboard/hotkey execution.
/// Blocks `super`/`meta`, `alt+f4`, `ctrl+alt+delete`, `ctrl+alt+f*`, and `ctrl+alt+backspace`.
pub fn check_hotkey_safety(keys: &[String]) -> Result<(), String> {
    let norm = normalize_hotkey_tokens(keys)?;
    let has = |target: &str| norm.iter().any(|k| k == target);
    if has("super") {
        return Err("blocked OS-escape shortcut: super/meta key is not permitted".to_string());
    }
    if has("alt") && has("f4") {
        return Err("blocked OS-escape shortcut: alt+f4 is not permitted".to_string());
    }
    if has("ctrl") && has("alt") && (has("delete") || has("backspace")) {
        return Err("blocked OS-escape shortcut: ctrl+alt+delete/backspace is not permitted".to_string());
    }
    if has("ctrl")
        && has("alt")
        && norm.iter().any(|k| {
            k.strip_prefix('f')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n))
        })
    {
        return Err("blocked OS-escape shortcut: ctrl+alt+f* VT switch is not permitted".to_string());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedKeyboardCmd {
    pub action: String,
    pub key: Option<String>,
    pub text: Option<String>,
    pub hotkey: Option<Vec<String>>,
}

/// Validate a CUA keyboard request (`hotkey` chord, `key` + optional `modifiers`, or `text`).
pub fn validate_cua_keyboard_payload(payload: &Value) -> Result<ValidatedKeyboardCmd, String> {
    let mut hotkey_vec: Option<Vec<String>> = None;
    if let Some(hk) = payload.get("hotkey") {
        let raw_tokens: Vec<String> = if let Some(arr) = hk.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = hk.as_str() {
            s.split('+')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        } else {
            return Err("hotkey must be an array of key strings or '+'-separated string".to_string());
        };
        let norm = normalize_hotkey_tokens(&raw_tokens)?;
        check_hotkey_safety(&norm)?;
        hotkey_vec = Some(norm);
    }

    let key = payload
        .get("key")
        .or_else(|| payload.get("key_code"))
        .or_else(|| payload.get("code"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let text = payload
        .get("text")
        .or_else(|| payload.get("text_inputs"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    if let Some(ref t) = text {
        if t.contains('\0') || t.len() > 2048 {
            return Err("keyboard text must be 1..=2048 bytes without NUL".to_string());
        }
    }

    if hotkey_vec.is_none() {
        if let Some(ref k) = key {
            let mut chord = Vec::new();
            if let Some(mods) = payload.get("modifiers").and_then(|v| v.as_object()) {
                if mods.get("ctrl").and_then(|v| v.as_bool()) == Some(true) {
                    chord.push("ctrl".to_string());
                }
                if mods.get("shift").and_then(|v| v.as_bool()) == Some(true) {
                    chord.push("shift".to_string());
                }
                if mods.get("alt").and_then(|v| v.as_bool()) == Some(true) {
                    chord.push("alt".to_string());
                }
                if mods.get("meta").and_then(|v| v.as_bool()) == Some(true)
                    || mods.get("super").and_then(|v| v.as_bool()) == Some(true)
                {
                    chord.push("super".to_string());
                }
            }
            let norm_key = normalize_hotkey_token(k)?;
            let has_mods = !chord.is_empty();
            chord.push(norm_key);
            check_hotkey_safety(&chord)?;
            if has_mods {
                hotkey_vec = Some(chord);
            }
        }
    }

    if hotkey_vec.is_none() && key.is_none() && text.is_none() {
        return Err("keyboard input requires non-empty 'hotkey', 'key', or 'text'".to_string());
    }

    let action = if hotkey_vec.is_some() {
        "hotkey".to_string()
    } else if text.is_some() && key.is_none() {
        "type".to_string()
    } else {
        payload
            .get("action")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("press")
            .to_ascii_lowercase()
    };

    Ok(ValidatedKeyboardCmd {
        action,
        key,
        text,
        hotkey: hotkey_vec,
    })
}

/// Forward a mouse event directly into an X11 display (`DISPLAY=:99`) via `libX11` + `libXtst`
/// so users and agents can click, drag, move, or scroll inside the embedded Studio viewport (#35, #81).
#[cfg(unix)]
pub fn send_x11_mouse(
    display: &str,
    resolution: [u32; 2],
    action: &str,
    x: f64,
    y: f64,
    to: Option<(f64, f64)>,
    button: &str,
    delta_y: f64,
) -> Result<Value, String> {
    let valid = validate_x11_display(display)?;
    let w = resolution[0].clamp(320, 3840) as f64;
    let h = resolution[1].clamp(200, 2160) as f64;
    let to_px_pair = |vx: f64, vy: f64| -> (i32, i32) {
        let (px, py) = if (0.0..=1.0).contains(&vx) && (0.0..=1.0).contains(&vy) && (vx > 0.0 || vy > 0.0) {
            ((vx * w).round() as i32, (vy * h).round() as i32)
        } else {
            (vx.round() as i32, vy.round() as i32)
        };
        (px.clamp(0, (w as i32) - 1), py.clamp(0, (h as i32) - 1))
    };
    let (px, py) = to_px_pair(x, y);
    let (tpx, tpy, has_drag_to) = match to {
        Some((tx, ty)) => {
            let (dx, dy) = to_px_pair(tx, ty);
            (dx, dy, 1)
        }
        None => (px, py, 0),
    };
    let btn: u32 = match button {
        "right" | "2" => 3,
        "middle" => 2,
        _ => 1,
    };
    let script = r#"
import ctypes, sys, time
x11 = ctypes.CDLL("libX11.so.6")
xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
dpy = x11.XOpenDisplay(sys.argv[1].encode())
if not dpy:
    sys.stderr.write("cannot open X11 display")
    sys.exit(1)
action, px, py, btn, dy = sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), float(sys.argv[6])
tpx, tpy, has_to = int(sys.argv[7]), int(sys.argv[8]), int(sys.argv[9])
xtst.XTestFakeMotionEvent(dpy, -1, px, py, 0)
if action == "click":
    xtst.XTestFakeButtonEvent(dpy, btn, 1, 0)
    xtst.XTestFakeButtonEvent(dpy, btn, 0, 0)
elif action == "double_click":
    for _ in range(2):
        xtst.XTestFakeButtonEvent(dpy, btn, 1, 0)
        xtst.XTestFakeButtonEvent(dpy, btn, 0, 0)
        x11.XFlush(dpy)
        time.sleep(0.05)
elif action == "right_click":
    xtst.XTestFakeButtonEvent(dpy, 3, 1, 0)
    xtst.XTestFakeButtonEvent(dpy, 3, 0, 0)
elif action == "drag" and has_to:
    xtst.XTestFakeButtonEvent(dpy, btn, 1, 0)
    x11.XFlush(dpy)
    steps = 12
    for step in range(1, steps + 1):
        ix = round(px + (tpx - px) * (step / steps))
        iy = round(py + (tpy - py) * (step / steps))
        xtst.XTestFakeMotionEvent(dpy, -1, ix, iy, 0)
        x11.XFlush(dpy)
        time.sleep(0.012)
    xtst.XTestFakeButtonEvent(dpy, btn, 0, 0)
elif action == "down":
    xtst.XTestFakeButtonEvent(dpy, btn, 1, 0)
elif action == "up":
    xtst.XTestFakeButtonEvent(dpy, btn, 0, 0)
elif action in ("wheel", "scroll"):
    wb = 4 if dy < 0 else 5
    steps = max(1, min(6, int(abs(dy) / 80) or 1))
    for _ in range(steps):
        xtst.XTestFakeButtonEvent(dpy, wb, 1, 0)
        xtst.XTestFakeButtonEvent(dpy, wb, 0, 0)
x11.XFlush(dpy)
x11.XCloseDisplay(dpy)
"#;
    let out = Command::new("python3")
        .args([
            "-c",
            script,
            &valid,
            action,
            &px.to_string(),
            &py.to_string(),
            &btn.to_string(),
            &delta_y.to_string(),
            &tpx.to_string(),
            &tpy.to_string(),
            &has_drag_to.to_string(),
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("X11 mouse pass-through failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("X11 mouse pass-through on {valid} failed: {err}"));
    }
    Ok(json!({
        "ok": true,
        "transport": "x11",
        "display": valid,
        "x": px,
        "y": py,
        "to_x": if has_drag_to == 1 { Some(tpx) } else { None },
        "to_y": if has_drag_to == 1 { Some(tpy) } else { None },
        "action": action,
    }))
}

#[cfg(not(unix))]
pub fn send_x11_mouse(
    _display: &str,
    _resolution: [u32; 2],
    _action: &str,
    _x: f64,
    _y: f64,
    _to: Option<(f64, f64)>,
    _button: &str,
    _delta_y: f64,
) -> Result<Value, String> {
    Err("X11 mouse pass-through is only available on Linux".to_string())
}

/// Forward a keyboard key press, typed text, or validated hotkey chord into an X11 display
/// (`DISPLAY=:99`) via `libX11` + `libXtst` (#35, #81).
#[cfg(unix)]
pub fn send_x11_keyboard(
    display: &str,
    action: &str,
    key: Option<&str>,
    text: Option<&str>,
    hotkey: Option<&[String]>,
) -> Result<Value, String> {
    let valid = validate_x11_display(display)?;
    let hotkey_joined = if let Some(hk) = hotkey {
        check_hotkey_safety(hk)?;
        normalize_hotkey_tokens(hk)?.join("+")
    } else {
        if let Some(k) = key {
            check_hotkey_safety(&[k.to_string()])?;
        }
        String::new()
    };
    let script = r#"
import ctypes, sys
x11 = ctypes.CDLL("libX11.so.6")
xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
x11.XStringToKeysym.restype = ctypes.c_ulong
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XKeysymToKeycode.restype = ctypes.c_uint
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
dpy = x11.XOpenDisplay(sys.argv[1].encode())
if not dpy:
    sys.stderr.write("cannot open X11 display")
    sys.exit(1)
action, key, text, hotkey = sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]
KEY_MAP = {
    "enter": "Return", "return": "Return", "tab": "Tab", "escape": "Escape", "esc": "Escape",
    "backspace": "BackSpace", "delete": "Delete", "space": "space", " ": "space",
    "arrowup": "Up", "up": "Up", "arrowdown": "Down", "down": "Down",
    "arrowleft": "Left", "left": "Left", "arrowright": "Right", "right": "Right",
    "ctrl": "Control_L", "shift": "Shift_L", "alt": "Alt_L",
    **{f"f{i}": f"F{i}" for i in range(1, 13)},
}
SHIFTED = set('~!@#$%^&*()_+{}|:"<>?')
CHAR_MAP = {
    ' ': 'space', '!': 'exclam', '"': 'quotedbl', '#': 'numbersign', '$': 'dollar', '%': 'percent',
    '&': 'ampersand', "'": 'apostrophe', '(': 'parenleft', ')': 'parenright', '*': 'asterisk',
    '+': 'plus', ',': 'comma', '-': 'minus', '.': 'period', '/': 'slash', ':': 'colon',
    ';': 'semicolon', '<': 'less', '=': 'equal', '>': 'greater', '?': 'question', '@': 'at',
    '[': 'bracketleft', '\\': 'backslash', ']': 'bracketright', '^': 'asciicircum', '_': 'underscore',
    '`': 'grave', '{': 'braceleft', '|': 'bar', '}': 'braceright', '~': 'asciitilde',
}
shift_kc = x11.XKeysymToKeycode(dpy, x11.XStringToKeysym(b"Shift_L"))
def keycode_for(sym_name):
    ks = x11.XStringToKeysym(sym_name.encode())
    if not ks:
        return 0
    return x11.XKeysymToKeycode(dpy, ks)
def press_sym(sym_name, need_shift=False, mode="press"):
    kc = keycode_for(sym_name)
    if not kc:
        return
    if need_shift and shift_kc:
        xtst.XTestFakeKeyEvent(dpy, shift_kc, 1, 0)
    if mode in ("press", "down"):
        xtst.XTestFakeKeyEvent(dpy, kc, 1, 0)
    if mode in ("press", "up"):
        xtst.XTestFakeKeyEvent(dpy, kc, 0, 0)
    if need_shift and shift_kc:
        xtst.XTestFakeKeyEvent(dpy, shift_kc, 0, 0)
if hotkey:
    mods = []
    mains = []
    for tok in hotkey.split("+"):
        if tok in ("ctrl", "shift", "alt"):
            kc = keycode_for(KEY_MAP[tok])
            if kc:
                mods.append(kc)
        else:
            sym = KEY_MAP.get(tok, CHAR_MAP.get(tok, tok))
            mains.append(sym)
    for mkc in mods:
        xtst.XTestFakeKeyEvent(dpy, mkc, 1, 0)
    for sym in mains:
        press_sym(sym, False, "press")
    for mkc in reversed(mods):
        xtst.XTestFakeKeyEvent(dpy, mkc, 0, 0)
elif text:
    for ch in text[:256]:
        if ch == '\n':
            press_sym("Return")
        elif ch in CHAR_MAP:
            press_sym(CHAR_MAP[ch], ch in SHIFTED)
        elif ch.isupper():
            press_sym(ch, True)
        elif ch.isascii() and ch.isprintable():
            press_sym(ch, False)
elif key:
    low = key.strip().lower()
    sym = KEY_MAP.get(low, CHAR_MAP.get(key, key))
    need_shift = len(key) == 1 and (key.isupper() or key in SHIFTED)
    press_sym(sym, need_shift, action if action in ("down", "up") else "press")
x11.XFlush(dpy)
x11.XCloseDisplay(dpy)
"#;
    let out = Command::new("python3")
        .args([
            "-c",
            script,
            &valid,
            action,
            key.unwrap_or(""),
            text.unwrap_or(""),
            &hotkey_joined,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("X11 keyboard pass-through failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("X11 keyboard pass-through on {valid} failed: {err}"));
    }
    Ok(json!({ "ok": true, "transport": "x11", "display": valid, "action": action }))
}

#[cfg(not(unix))]
pub fn send_x11_keyboard(
    _display: &str,
    _action: &str,
    _key: Option<&str>,
    _text: Option<&str>,
    _hotkey: Option<&[String]>,
) -> Result<Value, String> {
    Err("X11 keyboard pass-through is only available on Linux".to_string())
}

/// Launch or attach to Roblox Studio and start streaming the viewport **immediately**
/// even while Studio is on the Sign-In / QR screen (before MCP attaches) (#35, #118).
/// Never spawns a duplicate Studio when one is already running; detects its real X11
/// `DISPLAY` automatically.
pub fn launch_or_attach_live(
    manager: &mut StudioEmbedManager,
    mut config: EmbedConfig,
    hub: Option<Arc<StudioStreamHub>>,
) -> Result<EmbedLaunchOutcome, String> {
    // Direct non-launch MCP command mode (e.g. mock_studio in E2E/CI tests or custom bridge)
    if let Some(explicit_cmd) = config.mcp_command.clone() {
        if !config.launch_studio_process {
            manager.x11grab_stop.store(true, Ordering::SeqCst);
            if let Some(mut prev) = manager.ffmpeg_child.take() {
                let _ = prev.kill();
                let _ = prev.wait();
            }
            manager.x11grab_stop = Arc::new(AtomicBool::new(false));
            manager.state.ffmpeg_pid = None;
            manager.state.x11grab_active = false;
            let mut client = McpClient::spawn(&explicit_cmd).inspect_err(|e| {
                manager.state.phase = "error".to_string();
                manager.state.last_error = Some(e.clone());
            })?;
            manager.state.started_at_ms = Some(now_ms());
            if client.has_live_studio() {
                let open = probe_client_place_open(&mut client);
                manager.sync_from_client(&client, Some(explicit_cmd));
                manager.set_place_open(open);
                return Ok(EmbedLaunchOutcome::Connected(manager.state(), Box::new(client)));
            }
            // Running at Sign-In / no open place yet -> stay alive in awaiting_signin (#35, #118)
            manager.state.embedded = true;
            manager.state.mcp_connected = false;
            manager.state.phase = "awaiting_signin".to_string();
            manager.state.studio_id = None;
            manager.state.mcp_command = Some(explicit_cmd.clone());
            manager.state.last_error = None;
            return Ok(EmbedLaunchOutcome::StreamingAwaitingAuth {
                state: manager.state(),
                mcp_cmd: explicit_cmd,
            });
        }
    }

    #[cfg(unix)]
    {
        #[cfg(target_os = "linux")]
        let (already_running, detected_display) = detect_linux_running_studio_display();
        #[cfg(not(target_os = "linux"))]
        let (already_running, detected_display) = (is_vinegar_flatpak_running(), None::<String>);

        if !config.launch_studio_process
            && !already_running
            && !is_vinegar_flatpak_running()
            && config.mcp_command.is_none()
        {
            return Err(
                "No running Roblox Studio detected. Open Roblox Studio first or click Launch."
                    .to_string(),
            );
        }

        // Prefer the running Studio process's actual X11 DISPLAY (e.g. :0, :1, :99)
        // unless the caller explicitly passed a non-default display.
        if config.display == DEFAULT_EMBED_DISPLAY {
            if let Some(ref d) = detected_display {
                config.display = d.clone();
            }
        }
        let display = validate_x11_display(&config.display)?;
        let home = std::env::var("HOME")
            .map_err(|_| "HOME environment variable is not set".to_string())?;
        let mcp_cmd = config.mcp_command.clone().unwrap_or_else(|| {
            PathBuf::from(&home)
                .join(".local/bin/roblox-studio-mcp")
                .to_string_lossy()
                .into_owned()
        });

        // Stop any prior managed x11grab pump & poller before re-attaching (preserve pre-existing external Xvfb/Studio).
        manager.x11grab_stop.store(true, Ordering::SeqCst);
        if let Some(mut prev) = manager.ffmpeg_child.take() {
            let _ = prev.kill();
            let _ = prev.wait();
        }
        manager.x11grab_stop = Arc::new(AtomicBool::new(false));
        manager.state.ffmpeg_pid = None;
        manager.state.x11grab_active = false;
        manager.state.phase = "starting".to_string();
        manager.state.last_error = None;
        manager.state.place_file = config
            .place_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        manager.state.display = Some(display.clone());

        // 1. Attach to existing X11 display (probing its true resolution) or spawn Xvfb.
        if x11_socket_exists(&display) {
            if let Some([rw, rh]) = probe_x11_resolution(&display) {
                config.width = rw;
                config.height = rh;
            }
        } else if config.launch_studio_process && !already_running {
            cleanup_stale_x11_lock(&display);
            let screen_spec = format!("{}x{}x{}", config.width, config.height, config.depth);
            let child = Command::new("Xvfb")
                .args([&display, "-screen", "0", &screen_spec, "-ac"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("failed to spawn Xvfb {display}: {e}"))?;
            manager.state.xvfb_pid = Some(child.id());
            manager.xvfb_child = Some(child);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !x11_socket_exists(&display) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        } else if !already_running {
            return Err(format!("No running Roblox Studio or X11 display {display} found; launch Studio first."));
        }
        manager.state.resolution = [config.width, config.height];

        // 2. Spawn Vinegar on DISPLAY=<display> ONLY if requested and Studio is NOT already running (#118).
        if config.launch_studio_process && !already_running && !is_vinegar_flatpak_running() {
            let mut cmd = Command::new("flatpak");
            cmd.args(["run", "--command=vinegar", "org.vinegarhq.Vinegar"]);
            if let Some(pf) = config.place_file.as_ref() {
                let vinegar_root = PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar");
                if let Ok((_, win_path)) = stage_vinegar_place_file(pf, &vinegar_root) {
                    cmd.arg(win_path);
                }
            }
            cmd.env("DISPLAY", &display)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
                if let Some(uid) = current_unix_uid() {
                    cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
                }
            }
            let child = cmd.spawn().map_err(|e| format!("failed to launch Flatpak Vinegar: {e}"))?;
            manager.state.studio_pid = Some(child.id());
            manager.studio_child = Some(child);
        }

        // 3. Start real-time 12 FPS x11grab MJPEG stream IMMEDIATELY (even while on Sign-In / QR screen!),
        //    except when targeting the host Wayland desktop's XWayland display (e.g. `:0` under Wayland),
        //    where x11grab captures an empty/root buffer and MCP `screen_capture` should drive `/stream.mjpg`.
        if let Some(stream_hub) = hub {
            if !is_wayland_desktop_display(&display)
                && x11_socket_exists(&display)
                && find_on_path("ffmpeg").is_some()
            {
                let _ = spawn_x11grab_pump(
                    manager,
                    stream_hub,
                    &display,
                    config.width,
                    config.height,
                );
            }
        }

        // 4. Short probe for an already-signed-in Studio place; NEVER kill x11grab if awaiting sign-in!
        if (already_running || is_vinegar_flatpak_running()) && Path::new(&mcp_cmd).is_file() {
            if let Ok(mut client) = McpClient::spawn_with_timeout(&mcp_cmd, Duration::from_secs(3)) {
                if client.has_live_studio() {
                    let open = probe_client_place_open(&mut client);
                    client.reset_default_timeout();
                    manager.state.started_at_ms = Some(now_ms());
                    manager.sync_from_client(&client, Some(mcp_cmd));
                    manager.set_place_open(open);
                    return Ok(EmbedLaunchOutcome::Connected(manager.state(), Box::new(client)));
                }
            }
        }

        if !manager.state.x11grab_active && !already_running && !is_vinegar_flatpak_running() {
            return Err(
                "Could not start X11 viewport capture or detect a running Roblox Studio session."
                    .to_string(),
            );
        }

        manager.state.embedded = true;
        manager.state.mcp_connected = false;
        manager.state.phase = "awaiting_signin".to_string();
        manager.state.started_at_ms = Some(now_ms());
        manager.state.mcp_command = Some(mcp_cmd.clone());
        manager.state.last_error = None;
        Ok(EmbedLaunchOutcome::StreamingAwaitingAuth {
            state: manager.state(),
            mcp_cmd,
        })
    }

    #[cfg(not(unix))]
    {
        manager.x11grab_stop.store(true, Ordering::SeqCst);
        manager.x11grab_stop = Arc::new(AtomicBool::new(false));
        #[cfg(windows)]
        {
            let running = detect_windows_studio_running();
            if running {
                // Studio is already running on Windows -> never spawn a duplicate (#118).
                config.launch_studio_process = false;
            } else if !config.launch_studio_process && config.mcp_command.is_none() {
                return Err(
                    "No running Roblox Studio detected on Windows. Open Roblox Studio first or click Launch."
                        .to_string(),
                );
            }
        }
        let mcp_cmd_fallback = config
            .mcp_command
            .clone()
            .or_else(detect_host_mcp_command)
            .unwrap_or_default();
        let (state, mut client) = launch_embedded_studio_with_hub(manager, config, hub)?;
        if client.has_live_studio() {
            let open = probe_client_place_open(&mut client);
            manager.set_place_open(open);
            Ok(EmbedLaunchOutcome::Connected(manager.state(), Box::new(client)))
        } else {
            let cmd = state.mcp_command.clone().unwrap_or(mcp_cmd_fallback);
            manager.state.embedded = true;
            manager.state.mcp_connected = false;
            manager.state.phase = "awaiting_signin".to_string();
            manager.state.studio_id = None;
            manager.state.last_error = None;
            Ok(EmbedLaunchOutcome::StreamingAwaitingAuth {
                state: manager.state(),
                mcp_cmd: cmd,
            })
        }
    }
}

/// Explicitly launch (or attach to) an embedded Roblox Studio session and return the
/// connected `McpClient` alongside updated `StudioEmbedState`.
///
/// Strictly user-triggered — NEVER call from startup or tests unless explicitly requested.
pub fn launch_embedded_studio(
    manager: &mut StudioEmbedManager,
    config: EmbedConfig,
) -> Result<(StudioEmbedState, McpClient), String> {
    launch_embedded_studio_with_hub(manager, config, None)
}

/// Same as `launch_embedded_studio`, plus automatically attaches the real-time 12 FPS Linux
/// `x11grab` MJPEG pump directly into `hub` whenever real Linux Studio runs on `Xvfb`.
pub fn launch_embedded_studio_with_hub(
    manager: &mut StudioEmbedManager,
    config: EmbedConfig,
    hub: Option<Arc<StudioStreamHub>>,
) -> Result<(StudioEmbedState, McpClient), String> {
    let display = validate_x11_display(&config.display)?;
    // Clean up any prior managed child processes before starting a fresh session.
    if manager.studio_child.is_some()
        || manager.xvfb_child.is_some()
        || manager.ffmpeg_child.is_some()
    {
        stop_embedded_studio(manager);
    }

    manager.state.phase = "starting".to_string();
    manager.state.last_error = None;
    manager.state.resolution = [config.width, config.height];
    manager.state.place_file = config
        .place_file
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    manager.state.display = if cfg!(unix) {
        Some(display.clone())
    } else {
        None
    };

    // Direct MCP command mode (e.g. mock_studio in E2E/CI or custom external MCP bridge)
    if let Some(explicit_cmd) = config.mcp_command.clone() {
        if !config.launch_studio_process {
            let client = match McpClient::spawn(&explicit_cmd) {
                Ok(c) => c,
                Err(e) => {
                    manager.state.phase = "error".to_string();
                    manager.state.last_error = Some(e.clone());
                    return Err(e);
                }
            };
            manager.state.started_at_ms = Some(now_ms());
            manager.sync_from_client(&client, Some(explicit_cmd));
            return Ok((manager.state(), client));
        }
    }

    #[cfg(unix)]
    {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME environment variable is not set".to_string())?;
        let mcp_cmd = config.mcp_command.clone().unwrap_or_else(|| {
            PathBuf::from(&home)
                .join(".local/bin/roblox-studio-mcp")
                .to_string_lossy()
                .into_owned()
        });
        if !Path::new(&mcp_cmd).is_file() {
            let msg = format!(
                "Roblox Studio MCP wrapper not found at '{mcp_cmd}'. Install ~/.local/bin/roblox-studio-mcp or pass an explicit mcp_command."
            );
            manager.state.phase = "error".to_string();
            manager.state.last_error = Some(msg.clone());
            return Err(msg);
        }

        // 1. Attach to existing Xvfb display or spawn an isolated Xvfb owned by this manager.
        if !x11_socket_exists(&display) {
            cleanup_stale_x11_lock(&display);
            let screen_spec = format!("{}x{}x{}", config.width, config.height, config.depth);
            let child = Command::new("Xvfb")
                .args([&display, "-screen", "0", &screen_spec, "-ac"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| {
                    let msg = format!("failed to spawn Xvfb {display}: {e}");
                    manager.state.phase = "error".to_string();
                    manager.state.last_error = Some(msg.clone());
                    msg
                })?;
            manager.state.xvfb_pid = Some(child.id());
            manager.xvfb_child = Some(child);

            let deadline = Instant::now() + Duration::from_secs(5);
            while !x11_socket_exists(&display) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // 2. Spawn Flatpak Vinegar on DISPLAY=<display> (opening optional .rbxlx place_file) if not already running.
        if config.launch_studio_process && !is_vinegar_flatpak_running() {
            let mut cmd = Command::new("flatpak");
            cmd.args(["run", "--command=vinegar", "org.vinegarhq.Vinegar"]);
            if let Some(pf) = config.place_file.as_ref() {
                let vinegar_root = PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar");
                if let Ok((_, win_path)) = stage_vinegar_place_file(pf, &vinegar_root) {
                    cmd.arg(win_path);
                }
            }
            cmd.env("DISPLAY", &display)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
                if let Some(uid) = current_unix_uid() {
                    cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
                }
            }
            match cmd.spawn() {
                Ok(child) => {
                    manager.state.studio_pid = Some(child.id());
                    manager.studio_child = Some(child);
                }
                Err(e) => {
                    let msg = format!("failed to launch Flatpak org.vinegarhq.Vinegar: {e}");
                    stop_embedded_studio(manager);
                    manager.state.phase = "error".to_string();
                    manager.state.last_error = Some(msg.clone());
                    return Err(msg);
                }
            }
        }

        // 3. Start real-time 12 FPS x11grab MJPEG frame pump into StudioStreamHub when hub is provided!
        if let Some(stream_hub) = hub {
            if x11_socket_exists(&display) && find_on_path("ffmpeg").is_some() {
                let _ = spawn_x11grab_pump(
                    manager,
                    stream_hub,
                    &display,
                    config.width,
                    config.height,
                );
            }
        }

        // 4. Poll until Vinegar Flatpak is visible and StudioMCP answers initialize.
        let deadline = Instant::now() + Duration::from_millis(config.startup_timeout_ms);
        let mut last_err = "timed out waiting for Roblox Studio MCP bridge".to_string();
        loop {
            if is_vinegar_flatpak_running() {
                match McpClient::spawn(&mcp_cmd) {
                    Ok(client) => {
                        manager.state.started_at_ms = Some(now_ms());
                        manager.sync_from_client(&client, Some(mcp_cmd));
                        return Ok((manager.state(), client));
                    }
                    Err(e) => last_err = e,
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        stop_embedded_studio(manager);
        manager.state.phase = "error".to_string();
        manager.state.last_error = Some(last_err.clone());
        Err(last_err)
    }

    #[cfg(windows)]
    {
        let _ = hub;
        let local_appdata = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA environment variable is not set".to_string())?;
        let paths = find_windows_roblox_paths(&local_appdata);
        let mcp_cmd = config
            .mcp_command
            .clone()
            .or_else(|| {
                paths
                    .studio_mcp_exe
                    .as_ref()
                    .map(|p| format!("\"{}\"", p.display()))
            })
            .or_else(|| {
                paths
                    .mcp_bat
                    .as_ref()
                    .map(|p| format!("cmd /C \"{}\"", p.display()))
            })
            .ok_or_else(|| {
                let msg = "StudioMCP.exe / mcp.bat not found under %LOCALAPPDATA%\\Roblox\\Versions."
                    .to_string();
                manager.state.phase = "error".to_string();
                manager.state.last_error = Some(msg.clone());
                msg
            })?;

        if config.launch_studio_process {
            if let Some(studio_exe) = paths.studio_exe.as_ref() {
                let mut cmd = Command::new(studio_exe);
                if let Some(pf) = config.place_file.as_ref() {
                    cmd.args(["-task", "EditFile", "-localPlaceFile"]).arg(pf);
                }
                match cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        manager.state.studio_pid = Some(child.id());
                        manager.studio_child = Some(child);
                    }
                    Err(e) => {
                        let msg = format!(
                            "failed to launch RobloxStudioBeta.exe ({}): {e}",
                            studio_exe.display()
                        );
                        manager.state.phase = "error".to_string();
                        manager.state.last_error = Some(msg.clone());
                        return Err(msg);
                    }
                }
            }
        }

        let deadline = Instant::now() + Duration::from_millis(config.startup_timeout_ms);
        let last_err = loop {
            match McpClient::spawn(&mcp_cmd) {
                Ok(client) => {
                    manager.state.started_at_ms = Some(now_ms());
                    manager.sync_from_client(&client, Some(mcp_cmd));
                    return Ok((manager.state(), client));
                }
                Err(e) => {
                    if Instant::now() >= deadline {
                        break e;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        };

        stop_embedded_studio(manager);
        manager.state.phase = "error".to_string();
        manager.state.last_error = Some(last_err.clone());
        Err(last_err)
    }
}

/// Stop ONLY child processes explicitly spawned by `manager`
/// (`ffmpeg_child`, `studio_child`, and `xvfb_child`).
/// Never runs `pkill -f` and never terminates an external pre-existing display server.
pub fn stop_embedded_studio(manager: &mut StudioEmbedManager) -> StudioEmbedState {
    let prev_display = manager.state.display.clone();
    manager.x11grab_stop.store(true, Ordering::SeqCst);
    if let Some(mut child) = manager.ffmpeg_child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(mut child) = manager.studio_child.take() {
        let _ = child.kill();
        let _ = child.wait();
        #[cfg(unix)]
        if find_on_path("flatpak").is_some() {
            // `flatpak run` spawns a bubblewrap sandbox that can outlive the wrapper PID;
            // only kill the Vinegar sandbox we launched (never touching Sober or host apps).
            let _ = Command::new("flatpak")
                .args(["kill", "org.vinegarhq.Vinegar"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    if let Some(mut child) = manager.xvfb_child.take() {
        #[cfg(unix)]
        let killed_pid = child.id();
        let _ = child.kill();
        let _ = child.wait();
        #[cfg(unix)]
        if let Some(ref disp) = prev_display {
            let _ = cleanup_stale_x11_lock_in(
                Path::new("/tmp"),
                Path::new("/proc"),
                disp,
                Some(killed_pid),
            );
        }
    }
    #[cfg(not(unix))]
    let _ = prev_display;
    manager.state.phase = "stopped".to_string();
    manager.state.embedded = false;
    manager.state.mcp_connected = false;
    manager.state.place_open = false;
    manager.state.transferred = false;
    manager.state.playing = false;
    manager.state.studio_id = None;
    manager.state.studio_pid = None;
    manager.state.xvfb_pid = None;
    manager.state.ffmpeg_pid = None;
    manager.state.x11grab_active = false;
    manager.state.tools.clear();
    manager.state.official_tools_count = 0;
    manager.state()
}

/// Sanitize a `.rbxlx`/`.rbxl` file name for staging inside Vinegar's Wine `C:\openro-places\`
/// directory so there are never spaces, path separators, or leading flag dashes (#50).
pub fn sanitize_vinegar_place_filename(rbxlx_path: &Path) -> String {
    let raw = rbxlx_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("place.rbxlx");
    let mut clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while clean.starts_with('.') || clean.starts_with('-') {
        clean.remove(0);
    }
    if clean.is_empty() {
        clean = "place.rbxlx".to_string();
    }
    let low = clean.to_ascii_lowercase();
    if !low.ends_with(".rbxlx") && !low.ends_with(".rbxl") {
        clean.push_str(".rbxlx");
    }
    clean
}

/// Detect the active Wine username inside Vinegar's `drive_c/users/` prefix (#51).
/// Prefers `$USER` if present, else the first non-`Public` directory, else `"Public"`.
pub fn detect_vinegar_wine_user(vinegar_root: &Path) -> String {
    let users_dir = vinegar_root.join("data/vinegar/prefixes/studio/drive_c/users");
    if let Ok(env_user) = std::env::var("USER") {
        let u = env_user.trim();
        if !u.is_empty()
            && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            && users_dir.join(u).is_dir()
        {
            return u.to_string();
        }
    }
    if let Ok(entries) = std::fs::read_dir(&users_dir) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| !n.eq_ignore_ascii_case("Public") && !n.starts_with('.'))
            .collect();
        names.sort();
        if let Some(first) = names.into_iter().next() {
            return first;
        }
    }
    if let Ok(env_user) = std::env::var("USER") {
        let u = env_user.trim();
        if !u.is_empty()
            && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return u.to_string();
        }
    }
    "Public".to_string()
}

/// Compute the host staging path under Vinegar's writable `drive_c/users/<user>/Documents/OpenRO/`
/// (always mounted R/W inside Vinegar's Flatpak sandbox) and its corresponding Wine Windows path
/// `C:\users\<user>\Documents\OpenRO\<file>.rbxlx` (#50, #51).
pub fn vinegar_stage_paths(rbxlx_path: &Path, vinegar_root: &Path) -> (PathBuf, String) {
    let fname = sanitize_vinegar_place_filename(rbxlx_path);
    let win_user = detect_vinegar_wine_user(vinegar_root);
    let host_path = vinegar_root
        .join("data/vinegar/prefixes/studio/drive_c/users")
        .join(&win_user)
        .join("Documents/OpenRO")
        .join(&fname);
    let win_path = format!("C:\\users\\{win_user}\\Documents\\OpenRO\\{fname}");
    (host_path, win_path)
}

/// Copy a built `.rbxlx`/`.rbxl` place into Vinegar's `drive_c/users/<user>/Documents/OpenRO/`
/// so Wine `RobloxStudioBeta.exe` loads it directly into Edit mode (`place_open: true`) without
/// needing extra Flatpak `--filesystem=` overrides (#50, #51).
pub fn stage_vinegar_place_file(
    rbxlx_path: &Path,
    vinegar_root: &Path,
) -> Result<(PathBuf, String), String> {
    if !rbxlx_path.is_file() {
        return Err(format!("place file '{}' does not exist", rbxlx_path.display()));
    }
    let (host_path, win_path) = vinegar_stage_paths(rbxlx_path, vinegar_root);
    if let Some(parent) = host_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create Vinegar staging dir '{}': {e}", parent.display()))?;
    }
    std::fs::copy(rbxlx_path, &host_path).map_err(|e| {
        format!(
            "stage '{}' to '{}': {e}",
            rbxlx_path.display(),
            host_path.display()
        )
    })?;
    Ok((host_path, win_path))
}

/// Synthesize a minimal, valid blank Baseplate `.rbxlx` file when no project build exists yet (#51).
pub fn ensure_blank_baseplate_rbxlx(dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create baseplate dir '{}': {e}", dir.display()))?;
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let clean = slug.trim_matches('-');
    let fname = if clean.is_empty() {
        "openro-baseplate.rbxlx".to_string()
    } else {
        format!("{clean}-baseplate.rbxlx")
    };
    let out = dir.join(fname);
    let xml = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
  <External>null</External>
  <External>nil</External>
  <Item class="Workspace" referent="RBX0">
    <Properties><string name="Name">Workspace</string></Properties>
    <Item class="Part" referent="RBX1">
      <Properties>
        <string name="Name">Baseplate</string>
        <bool name="Anchored">true</bool>
        <bool name="Locked">true</bool>
        <Vector3 name="size"><X>512</X><Y>16</Y><Z>512</Z></Vector3>
        <CoordinateFrame name="CFrame"><X>0</X><Y>-8</Y><Z>0</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
      </Properties>
    </Item>
    <Item class="SpawnLocation" referent="RBX2">
      <Properties>
        <string name="Name">SpawnLocation</string>
        <bool name="Anchored">true</bool>
        <Vector3 name="size"><X>12</X><Y>1</Y><Z>12</Z></Vector3>
        <CoordinateFrame name="CFrame"><X>0</X><Y>0.5</Y><Z>0</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
      </Properties>
    </Item>
  </Item>
  <Item class="Lighting" referent="RBX3">
    <Properties><string name="Name">Lighting</string></Properties>
  </Item>
  <Item class="ReplicatedStorage" referent="RBX4">
    <Properties><string name="Name">ReplicatedStorage</string></Properties>
  </Item>
  <Item class="ServerScriptService" referent="RBX5">
    <Properties><string name="Name">ServerScriptService</string></Properties>
  </Item>
  <Item class="ServerStorage" referent="RBX6">
    <Properties><string name="Name">ServerStorage</string></Properties>
  </Item>
  <Item class="StarterGui" referent="RBX7">
    <Properties><string name="Name">StarterGui</string></Properties>
  </Item>
  <Item class="StarterPlayer" referent="RBX8">
    <Properties><string name="Name">StarterPlayer</string></Properties>
    <Item class="StarterPlayerScripts" referent="RBX9">
      <Properties><string name="Name">StarterPlayerScripts</string></Properties>
    </Item>
  </Item>
</roblox>
"#;
    std::fs::write(&out, xml).map_err(|e| format!("write blank baseplate '{}': {e}", out.display()))?;
    Ok(out)
}

/// Options for seamless transfer / zero-manual-step open in Embedded Roblox Studio (#51).
#[derive(Clone, Debug)]
pub struct TransferOptions {
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub place_file: Option<PathBuf>,
    pub force_transfer: bool,
    pub wait_secs: u64,
    pub mcp_command: Option<String>,
}

impl Default for TransferOptions {
    fn default() -> Self {
        TransferOptions {
            display: DEFAULT_EMBED_DISPLAY.to_string(),
            width: DEFAULT_EMBED_WIDTH,
            height: DEFAULT_EMBED_HEIGHT,
            place_file: None,
            force_transfer: false,
            wait_secs: 45,
            mcp_command: None,
        }
    }
}

/// Result of `transfer_to_embedded_studio` (#51).
pub struct TransferResult {
    pub outcome: EmbedLaunchOutcome,
    pub transferred: bool,
    pub attached_existing: bool,
    pub staged_path: Option<String>,
    pub windows_path: Option<String>,
    pub rbxlx_path: Option<String>,
    pub command: Vec<String>,
}

/// Seamlessly transfer or open a `.rbxlx` place directly inside OpenRO's Embedded Studio viewport (#51):
/// - If Studio is not running, OR sitting on Start Page (`place_open == false`), OR `force_transfer == true`:
///   cleanly stops Start-Page Vinegar (never `pkill -f`, never touching `Xvfb :99`), stages `.rbxlx` into
///   Vinegar's signed-in prefix (`C:\users\<user>\Documents\OpenRO\<slug>.rbxlx`), launches on `DISPLAY=:99`,
///   starts 12 FPS `x11grab` MJPEG + X11 input pass-through immediately, and polls `StudioMCP` until
///   `place_open == true` (staying live in `awaiting_signin` if auth is still required).
/// - If desktop Studio already has an open place (`place_open == true`) and `!force_transfer`:
///   preserves the open desktop window and attaches live MCP (`screen_capture` pump on non-X11 / Wayland).
pub fn transfer_to_embedded_studio(
    manager: &mut StudioEmbedManager,
    #[allow(unused_mut)] mut opts: TransferOptions,
    hub: Option<Arc<StudioStreamHub>>,
) -> Result<TransferResult, String> {
    let rbxlx_path = opts
        .place_file
        .as_ref()
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
    let rbxlx_str = rbxlx_path.as_ref().map(|p| p.to_string_lossy().into_owned());

    // Explicit custom MCP bridge (e.g. mock_studio in automated tests): never spawn/kill Vinegar.
    if let Some(explicit_cmd) = opts.mcp_command.clone() {
        let cfg = EmbedConfig {
            display: opts.display.clone(),
            width: opts.width,
            height: opts.height,
            depth: DEFAULT_EMBED_DEPTH,
            mcp_command: Some(explicit_cmd.clone()),
            place_file: rbxlx_path.clone(),
            launch_studio_process: false,
            startup_timeout_ms: opts.wait_secs.clamp(1, 90) * 1000,
        };
        let outcome = launch_or_attach_live(manager, cfg, hub)?;
        manager.set_transferred(opts.force_transfer);
        return Ok(TransferResult {
            outcome,
            transferred: opts.force_transfer,
            attached_existing: !opts.force_transfer,
            staged_path: None,
            windows_path: None,
            rbxlx_path: rbxlx_str,
            command: vec![explicit_cmd],
        });
    }

    #[cfg(target_os = "linux")]
    {
        let display = validate_x11_display(&opts.display)?;
        let home = std::env::var("HOME")
            .map_err(|_| "HOME environment variable is not set".to_string())?;
        let vinegar_root = std::env::var("OPENRO_VINEGAR_ROOT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar"));
        let mcp_cmd = PathBuf::from(&home)
            .join(".local/bin/roblox-studio-mcp")
            .to_string_lossy()
            .into_owned();
        if find_on_path("flatpak").is_none() || !vinegar_root.exists() {
            return Err(
                "Flatpak org.vinegarhq.Vinegar is not installed; install Vinegar or pass a custom mcp_command."
                    .to_string(),
            );
        }

        let (staged_path, windows_path) = if let Some(ref pf) = rbxlx_path {
            let (st, win) = stage_vinegar_place_file(pf, &vinegar_root)?;
            (Some(st.to_string_lossy().into_owned()), Some(win))
        } else {
            (None, None)
        };

        let det = detect_studio_environment();
        let already_running = det.studio_running || is_vinegar_flatpak_running();

        // Case B: Studio already has an open place (`place_open == true`) AND either:
        //  - caller did not force transfer (preserve desktop Studio), OR
        //  - Studio is ALREADY open on the target embedded display (e.g. `:99`) — avoid needless restart!
        let already_on_target_display = det.studio_display.as_deref() == Some(display.as_str());
        if already_running && det.place_open && (!opts.force_transfer || already_on_target_display) {
            if let Ok(mut client) = McpClient::spawn_with_timeout(&mcp_cmd, Duration::from_secs(5)) {
                if client.has_live_studio() {
                    let open = probe_client_place_open(&mut client);
                    client.reset_default_timeout();
                    manager.x11grab_stop.store(true, Ordering::SeqCst);
                    if let Some(mut prev) = manager.ffmpeg_child.take() {
                        let _ = prev.kill();
                        let _ = prev.wait();
                    }
                    manager.x11grab_stop = Arc::new(AtomicBool::new(false));
                    manager.state.ffmpeg_pid = None;
                    manager.state.x11grab_active = false;
                    let target_disp = det.studio_display.clone().unwrap_or_else(|| display.clone());
                    manager.state.display = Some(target_disp.clone());
                    if let Some(stream_hub) = hub {
                        if !is_wayland_desktop_display(&target_disp)
                            && x11_socket_exists(&target_disp)
                            && find_on_path("ffmpeg").is_some()
                        {
                            if let Some([rw, rh]) = probe_x11_resolution(&target_disp) {
                                opts.width = rw;
                                opts.height = rh;
                            }
                            manager.state.resolution = [opts.width, opts.height];
                            let _ = spawn_x11grab_pump(
                                manager,
                                stream_hub,
                                &target_disp,
                                opts.width,
                                opts.height,
                            );
                        }
                    }
                    manager.state.started_at_ms = Some(now_ms());
                    manager.state.place_file = rbxlx_str.clone();
                    manager.sync_from_client(&client, Some(mcp_cmd.clone()));
                    manager.set_place_open(open);
                    manager.set_transferred(false);
                    return Ok(TransferResult {
                        outcome: EmbedLaunchOutcome::Connected(manager.state(), Box::new(client)),
                        transferred: false,
                        attached_existing: true,
                        staged_path,
                        windows_path,
                        rbxlx_path: rbxlx_str,
                        command: vec![mcp_cmd],
                    });
                }
            }
        }

        // Case A: Transfer / cold-launch onto isolated Xvfb :99 with staged .rbxlx (#51).
        // Cleanly stop any existing Vinegar instance first (never pkill -f, never touch Xvfb :99).
        manager.x11grab_stop.store(true, Ordering::SeqCst);
        if let Some(mut prev) = manager.ffmpeg_child.take() {
            let _ = prev.kill();
            let _ = prev.wait();
        }
        if let Some(mut prev_studio) = manager.studio_child.take() {
            let _ = prev_studio.kill();
            let _ = prev_studio.wait();
        }
        manager.x11grab_stop = Arc::new(AtomicBool::new(false));
        manager.state.ffmpeg_pid = None;
        manager.state.x11grab_active = false;
        manager.state.studio_pid = None;

        if already_running || is_vinegar_flatpak_running() {
            stop_vinegar_gracefully();
        }

        // Ensure isolated Xvfb display exists (reusing existing Xvfb :99 cleanly).
        if x11_socket_exists(&display) {
            if let Some([rw, rh]) = probe_x11_resolution(&display) {
                opts.width = rw;
                opts.height = rh;
            }
        } else {
            cleanup_stale_x11_lock(&display);
            let screen_spec = format!("{}x{}x{}", opts.width, opts.height, DEFAULT_EMBED_DEPTH);
            let child = Command::new("Xvfb")
                .args([&display, "-screen", "0", &screen_spec, "-ac"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("failed to spawn Xvfb {display}: {e}"))?;
            manager.state.xvfb_pid = Some(child.id());
            manager.xvfb_child = Some(child);
            let xvfb_deadline = Instant::now() + Duration::from_secs(5);
            while !x11_socket_exists(&display) && Instant::now() < xvfb_deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        manager.state.display = Some(display.clone());
        manager.state.resolution = [opts.width, opts.height];
        manager.state.place_file = rbxlx_str.clone();
        manager.state.phase = "starting".to_string();
        manager.state.last_error = None;

        let mut args = vec![
            "run".to_string(),
            "--nosocket=wayland".to_string(),
            "--command=vinegar".to_string(),
            "org.vinegarhq.Vinegar".to_string(),
        ];
        if let Some(ref wp) = windows_path {
            args.push(wp.clone());
        }
        let mut command_vec = vec!["flatpak".to_string()];
        command_vec.extend(args.iter().cloned());

        let mut cmd = Command::new("flatpak");
        cmd.args(&args)
            .env_remove("WAYLAND_DISPLAY")
            .env("GDK_BACKEND", "x11")
            .env("DISPLAY", &display)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            if let Some(uid) = current_unix_uid() {
                cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
            }
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to launch Flatpak Vinegar on {display}: {e}"))?;
        manager.state.studio_pid = Some(child.id());
        manager.studio_child = Some(child);

        // Start 12 FPS x11grab MJPEG stream IMMEDIATELY on :99 so the UI sees Studio boot live!
        if let Some(stream_hub) = hub {
            if !is_wayland_desktop_display(&display)
                && x11_socket_exists(&display)
                && find_on_path("ffmpeg").is_some()
            {
                let _ = spawn_x11grab_pump(manager, stream_hub, &display, opts.width, opts.height);
            }
        }

        // Wait until RobloxStudioBeta.exe has actually spawned inside Vinegar before invoking
        // `roblox-studio-mcp` (`flatpak enter ... wine StudioMCP.exe`), so MCP never collides
        // with Vinegar's initial `Setting up Wine` / `wineserver` boot phase (#51).
        let wait_secs = opts.wait_secs.clamp(3, 90);
        let deadline = Instant::now() + Duration::from_secs(wait_secs);
        let boot_deadline = Instant::now() + Duration::from_secs(12).min(Duration::from_secs(wait_secs));
        while !detect_linux_running_studio_display().0 && Instant::now() < boot_deadline {
            std::thread::sleep(Duration::from_millis(500));
        }
        if detect_linux_running_studio_display().0 {
            std::thread::sleep(Duration::from_secs(2));
        }

        // Poll StudioMCP until a place is actually open (`place_open == true`), up to `wait_secs`.
        // Note: on Vinegar cold start, `list_roblox_studios` can briefly flicker while the place
        // initializes before settling in `mode: "Edit"`. Never kill x11grab/Studio on timeout.
        if Path::new(&mcp_cmd).is_file() {
            while Instant::now() < deadline {
                if detect_linux_running_studio_display().0 {
                    if let Ok(mut client) =
                        McpClient::spawn_with_timeout(&mcp_cmd, Duration::from_secs(5))
                    {
                        if client.has_live_studio() && probe_client_place_open(&mut client) {
                            client.reset_default_timeout();
                            manager.state.started_at_ms = Some(now_ms());
                            manager.sync_from_client(&client, Some(mcp_cmd.clone()));
                            manager.set_place_open(true);
                            manager.set_transferred(true);
                            return Ok(TransferResult {
                                outcome: EmbedLaunchOutcome::Connected(
                                    manager.state(),
                                    Box::new(client),
                                ),
                                transferred: true,
                                attached_existing: false,
                                staged_path,
                                windows_path,
                                rbxlx_path: rbxlx_str,
                                command: command_vec,
                            });
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        }

        manager.state.embedded = true;
        manager.state.mcp_connected = false;
        manager.state.place_open = false;
        manager.state.transferred = true;
        manager.state.phase = "awaiting_signin".to_string();
        manager.state.started_at_ms = Some(now_ms());
        manager.state.mcp_command = Some(mcp_cmd.clone());
        manager.state.last_error = None;
        Ok(TransferResult {
            outcome: EmbedLaunchOutcome::StreamingAwaitingAuth {
                state: manager.state(),
                mcp_cmd,
            },
            transferred: true,
            attached_existing: false,
            staged_path,
            windows_path,
            rbxlx_path: rbxlx_str,
            command: command_vec,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        let det = detect_studio_environment();
        let command_vec = if let Some(ref pf) = rbxlx_path {
            let p_str = pf.to_string_lossy().into_owned();
            #[cfg(windows)]
            {
                vec![
                    det.studio_exe
                        .clone()
                        .unwrap_or_else(|| "RobloxStudioBeta.exe".to_string()),
                    "-task".to_string(),
                    "EditFile".to_string(),
                    "-localPlaceFile".to_string(),
                    p_str,
                ]
            }
            #[cfg(target_os = "macos")]
            {
                vec![
                    "open".to_string(),
                    "-a".to_string(),
                    "/Applications/RobloxStudio.app".to_string(),
                    p_str,
                ]
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                vec![p_str]
            }
        } else {
            Vec::new()
        };
        let cfg = EmbedConfig {
            display: opts.display,
            width: opts.width,
            height: opts.height,
            depth: DEFAULT_EMBED_DEPTH,
            mcp_command: det.mcp_command,
            place_file: rbxlx_path.clone(),
            launch_studio_process: !det.studio_running || opts.force_transfer || !det.place_open,
            startup_timeout_ms: opts.wait_secs.clamp(3, 90) * 1000,
        };
        let outcome = launch_or_attach_live(manager, cfg, hub)?;
        let transferred = !det.place_open || opts.force_transfer;
        manager.set_transferred(transferred);
        Ok(TransferResult {
            outcome,
            transferred,
            attached_existing: det.studio_running && !transferred,
            staged_path: None,
            windows_path: rbxlx_str.clone(),
            rbxlx_path: rbxlx_str,
            command: command_vec,
        })
    }
}

/// Gracefully ask any running `RobloxStudioBeta.exe` processes inside Vinegar to close
/// (`SIGTERM`) before falling back to `flatpak kill org.vinegarhq.Vinegar`, so Studio
/// can flush its auth/session state cleanly instead of losing cookies on SIGKILL (#51).
#[cfg(target_os = "linux")]
fn stop_vinegar_gracefully() {
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let pid_str = entry.file_name().to_string_lossy().to_string();
            if !pid_str.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(cmd_bytes) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let cmd_text = String::from_utf8_lossy(&cmd_bytes);
            if cmd_text.contains("RobloxStudioBeta.exe") && !cmd_text.contains("msedgewebview2.exe") {
                let _ = Command::new("kill")
                    .args(["-TERM", &pid_str])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
    let graceful_deadline = Instant::now() + Duration::from_secs(4);
    while (is_vinegar_flatpak_running() || detect_linux_running_studio_display().0)
        && Instant::now() < graceful_deadline
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    if is_vinegar_flatpak_running() || detect_linux_running_studio_display().0 {
        let _ = Command::new("flatpak")
            .args(["kill", "org.vinegarhq.Vinegar"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let kill_deadline = Instant::now() + Duration::from_secs(5);
        while (is_vinegar_flatpak_running() || detect_linux_running_studio_display().0)
            && Instant::now() < kill_deadline
        {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    // Brief settle so wineserver cleanly releases its prefix socket before relaunch.
    std::thread::sleep(Duration::from_millis(600));
}

/// Build `flatpak` arguments that pass a Wine Windows `C:\...` path cleanly to Vinegar (#50, #51).
pub fn build_vinegar_open_args(windows_path: &str) -> Vec<String> {
    vec![
        "run".to_string(),
        "--command=vinegar".to_string(),
        "org.vinegarhq.Vinegar".to_string(),
        windows_path.to_string(),
    ]
}

/// Open a built `.rbxlx` / `.rbxl` place file directly in Roblox Studio so developers never
/// get stuck on Studio's Start Page (#47, #50). Avoids spawning duplicate Studio instances
/// when Studio is already running unless `force` is `true`.
pub fn open_place_in_studio(
    rbxlx_path: &Path,
    display_override: Option<&str>,
    dry_run: bool,
) -> Result<Value, String> {
    open_place_in_studio_with_options(rbxlx_path, display_override, dry_run, false)
}

/// Same as `open_place_in_studio`, with explicit `force` flag to allow launching even when
/// a Studio instance is already running (#50).
pub fn open_place_in_studio_with_options(
    rbxlx_path: &Path,
    display_override: Option<&str>,
    dry_run: bool,
    force: bool,
) -> Result<Value, String> {
    open_place_in_studio_inner(rbxlx_path, display_override, dry_run, force, None)
}

/// Testable implementation of `open_place_in_studio_with_options` accepting an optional
/// `(studio_running, place_open)` override for deterministic duplicate-guard unit tests (#50).
pub fn open_place_in_studio_inner(
    rbxlx_path: &Path,
    display_override: Option<&str>,
    dry_run: bool,
    force: bool,
    running_override: Option<(bool, bool)>,
) -> Result<Value, String> {
    if !rbxlx_path.is_file() {
        return Err(format!(
            "place file '{}' does not exist (build the project first)",
            rbxlx_path.display()
        ));
    }
    let abs = std::fs::canonicalize(rbxlx_path).unwrap_or_else(|_| rbxlx_path.to_path_buf());
    if !dry_run {
        if let Some(proj_dir) = infer_project_root_from_place_file(&abs) {
            let slug = default_studio_project_slug(&proj_dir);
            let _ = stage_project_studio_content(&proj_dir, &slug);
        }
    }
    let abs_str = abs.to_string_lossy().into_owned();
    let platform = current_platform();
    let fname = abs
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("place.rbxlx");

    let (studio_running, place_open) = match running_override {
        Some(pair) => pair,
        None => {
            let det = detect_studio_environment();
            (det.studio_running, det.place_open)
        }
    };

    // Test / custom launcher override (`OPENRO_OPEN_STUDIO_CMD`): passed `<abs_path>` as argv[1].
    if let Ok(override_cmd) = std::env::var("OPENRO_OPEN_STUDIO_CMD") {
        let trimmed = override_cmd.trim();
        if !trimmed.is_empty() {
            let argv = vec![trimmed.to_string(), abs_str.clone()];
            if dry_run {
                return Ok(json!({
                    "launched": false,
                    "dry_run": true,
                    "already_running": studio_running,
                    "place_open": place_open,
                    "staged_path": Value::Null,
                    "windows_path": Value::Null,
                    "pid": Value::Null,
                    "display": display_override,
                    "platform": platform,
                    "command": argv,
                    "message": format!("Dry run: ready to open {fname} in Roblox Studio."),
                }));
            }
            if studio_running && !force {
                let msg = if place_open {
                    "Roblox Studio already has an open place — use sync/playtest directly, or pass force: true to open another window.".to_string()
                } else {
                    format!("Roblox Studio is already running on the Start Page — open a blank Baseplate or {fname} in the running window (pass force: true to launch a new Studio window).")
                };
                return Ok(json!({
                    "launched": false,
                    "dry_run": false,
                    "already_running": true,
                    "place_open": place_open,
                    "staged_path": Value::Null,
                    "windows_path": Value::Null,
                    "pid": Value::Null,
                    "display": display_override,
                    "platform": platform,
                    "command": argv,
                    "message": msg,
                }));
            }
            let child = Command::new(trimmed)
                .arg(&abs)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("failed to spawn OPENRO_OPEN_STUDIO_CMD '{trimmed}': {e}"))?;
            return Ok(json!({
                "launched": true,
                "dry_run": false,
                "already_running": studio_running,
                "place_open": place_open,
                "staged_path": Value::Null,
                "windows_path": Value::Null,
                "pid": child.id(),
                "display": display_override,
                "platform": platform,
                "command": argv,
                "message": format!("Opened {fname} in Roblox Studio — wait a few seconds for the place to load, then sync or playtest."),
            }));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let display = match display_override {
            Some(d) => validate_x11_display(d)?,
            None => detect_linux_running_studio_display()
                .1
                .or_else(|| {
                    std::env::var("DISPLAY")
                        .ok()
                        .and_then(|d| normalize_x11_display_env(&d))
                })
                .unwrap_or_else(|| ":0".to_string()),
        };
        let home = std::env::var("HOME").unwrap_or_default();
        let vinegar_root = std::env::var("OPENRO_VINEGAR_ROOT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar"));
        let flatpak_ok = find_on_path("flatpak").is_some() && vinegar_root.exists();
        let (staged_host, windows_path) = vinegar_stage_paths(&abs, &vinegar_root);
        let args = build_vinegar_open_args(&windows_path);
        let mut command_vec = vec!["flatpak".to_string()];
        command_vec.extend(args.iter().cloned());

        if dry_run {
            return Ok(json!({
                "launched": false,
                "dry_run": true,
                "already_running": studio_running,
                "place_open": place_open,
                "staged_path": staged_host.to_string_lossy(),
                "windows_path": windows_path,
                "pid": Value::Null,
                "display": display,
                "platform": platform,
                "studio_installed": flatpak_ok,
                "command": command_vec,
                "message": format!("Dry run: ready to open {fname} ({windows_path}) in Roblox Studio via Vinegar."),
            }));
        }
        if !flatpak_ok {
            return Err(
                "Flatpak org.vinegarhq.Vinegar is not installed; open the built .rbxlx manually in Roblox Studio."
                    .to_string(),
            );
        }
        // Stage into Vinegar's persistent `drive_c/openro-places/` so Wine RobloxStudioBeta.exe
        // can always read `C:\openro-places\<file>.rbxlx` inside the Flatpak sandbox (#50).
        let (staged_path, win_path) = stage_vinegar_place_file(&abs, &vinegar_root)?;

        if studio_running && !force {
            let msg = if place_open {
                format!(
                    "Roblox Studio already has an open place — staged fresh copy at {win_path}; sync/playtest directly or pass force: true to open another window."
                )
            } else {
                format!(
                    "Roblox Studio is already running on the Start Page — staged {fname} at {win_path}; open a blank Baseplate or that file in the running window (or pass force: true to open a new Studio window)."
                )
            };
            return Ok(json!({
                "launched": false,
                "dry_run": false,
                "already_running": true,
                "place_open": place_open,
                "staged_path": staged_path.to_string_lossy(),
                "windows_path": win_path,
                "pid": Value::Null,
                "display": display,
                "platform": platform,
                "command": command_vec,
                "message": msg,
            }));
        }

        let mut cmd = Command::new("flatpak");
        cmd.args(&args)
            .env("DISPLAY", &display)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            if let Some(uid) = current_unix_uid() {
                cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
            }
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to open '{}' in Roblox Studio via Vinegar: {e}", abs.display()))?;
        Ok(json!({
            "launched": true,
            "dry_run": false,
            "already_running": studio_running,
            "place_open": place_open,
            "staged_path": staged_path.to_string_lossy(),
            "windows_path": win_path,
            "pid": child.id(),
            "display": display,
            "platform": platform,
            "command": command_vec,
            "message": format!("Opened {fname} ({win_path}) in Roblox Studio — wait a few seconds for the place to load, then sync or playtest."),
        }))
    }

    #[cfg(target_os = "macos")]
    {
        let _ = display_override;
        let studio_app = PathBuf::from("/Applications/RobloxStudio.app");
        let args = vec![
            "-a".to_string(),
            studio_app.to_string_lossy().into_owned(),
            abs_str.clone(),
        ];
        let mut command_vec = vec!["open".to_string()];
        command_vec.extend(args.iter().cloned());
        if dry_run {
            return Ok(json!({
                "launched": false,
                "dry_run": true,
                "already_running": studio_running,
                "place_open": place_open,
                "staged_path": Value::Null,
                "windows_path": Value::Null,
                "pid": Value::Null,
                "display": Value::Null,
                "platform": platform,
                "studio_installed": studio_app.exists(),
                "command": command_vec,
                "message": format!("Dry run: ready to open {fname} in Roblox Studio."),
            }));
        }
        if !studio_app.exists() {
            return Err(
                "RobloxStudio.app not found in /Applications; open the built .rbxlx manually."
                    .to_string(),
            );
        }
        if studio_running && !force {
            let msg = if place_open {
                "Roblox Studio already has an open place — use sync/playtest directly, or pass force: true to open another window.".to_string()
            } else {
                format!("Roblox Studio is already running on the Start Page — open a blank Baseplate or {fname} in the running window (pass force: true to open a new Studio window).")
            };
            return Ok(json!({
                "launched": false,
                "dry_run": false,
                "already_running": true,
                "place_open": place_open,
                "staged_path": Value::Null,
                "windows_path": Value::Null,
                "pid": Value::Null,
                "display": Value::Null,
                "platform": platform,
                "command": command_vec,
                "message": msg,
            }));
        }
        let child = Command::new("open")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to open '{}' in Roblox Studio: {e}", abs.display()))?;
        Ok(json!({
            "launched": true,
            "dry_run": false,
            "already_running": studio_running,
            "place_open": place_open,
            "staged_path": Value::Null,
            "windows_path": Value::Null,
            "pid": child.id(),
            "display": Value::Null,
            "platform": platform,
            "command": command_vec,
            "message": format!("Opened {fname} in Roblox Studio — wait a few seconds for the place to load, then sync or playtest."),
        }))
    }

    #[cfg(windows)]
    {
        let _ = display_override;
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let studio_exe = local_appdata
            .as_deref()
            .map(find_windows_roblox_paths)
            .and_then(|p| p.studio_exe);
        let exe_str = studio_exe
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "RobloxStudioBeta.exe".to_string());
        let args = vec![
            "-task".to_string(),
            "EditFile".to_string(),
            "-localPlaceFile".to_string(),
            abs_str.clone(),
        ];
        let mut command_vec = vec![exe_str];
        command_vec.extend(args.iter().cloned());
        if dry_run {
            return Ok(json!({
                "launched": false,
                "dry_run": true,
                "already_running": studio_running,
                "place_open": place_open,
                "staged_path": Value::Null,
                "windows_path": abs_str,
                "pid": Value::Null,
                "display": Value::Null,
                "platform": platform,
                "studio_installed": studio_exe.is_some(),
                "command": command_vec,
                "message": format!("Dry run: ready to open {fname} in Roblox Studio."),
            }));
        }
        let Some(exe) = studio_exe else {
            return Err(
                "RobloxStudioBeta.exe not found under %LOCALAPPDATA%\\Roblox\\Versions."
                    .to_string(),
            );
        };
        if studio_running && !force {
            let msg = if place_open {
                "Roblox Studio already has an open place — use sync/playtest directly, or pass force: true to open another window.".to_string()
            } else {
                format!("Roblox Studio is already running on the Start Page — open a blank Baseplate or {fname} in the running window (pass force: true to open a new Studio window).")
            };
            return Ok(json!({
                "launched": false,
                "dry_run": false,
                "already_running": true,
                "place_open": place_open,
                "staged_path": Value::Null,
                "windows_path": abs_str,
                "pid": Value::Null,
                "display": Value::Null,
                "platform": platform,
                "command": command_vec,
                "message": msg,
            }));
        }
        let child = Command::new(&exe)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to open '{}' in RobloxStudioBeta.exe: {e}", abs.display()))?;
        Ok(json!({
            "launched": true,
            "dry_run": false,
            "already_running": studio_running,
            "place_open": place_open,
            "staged_path": Value::Null,
            "windows_path": abs_str,
            "pid": child.id(),
            "display": Value::Null,
            "platform": platform,
            "command": command_vec,
            "message": format!("Opened {fname} in Roblox Studio — wait a few seconds for the place to load, then sync or playtest."),
        }))
    }
}

/// Translate browser mouse JSON or raw official MCP `actions` into validated
/// Roblox Studio `user_mouse_input` action records.
pub fn translate_mouse_payload(payload: &Value) -> Result<Vec<Value>, String> {
    if let Some(arr) = payload.get("actions").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            return Err("mouse input actions array must not be empty".to_string());
        }
        return Ok(arr.clone());
    }

    let event = payload
        .get("event")
        .or_else(|| payload.get("type"))
        .or_else(|| payload.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("click")
        .trim()
        .to_ascii_lowercase();

    let button = match payload
        .get("button")
        .or_else(|| payload.get("mouse_button"))
        .and_then(|v| v.as_str())
        .unwrap_or("left")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "right" | "2" => "right",
        _ => "left",
    };

    let x = payload.get("from_x").or_else(|| payload.get("x")).and_then(|v| v.as_f64());
    let y = payload.get("from_y").or_else(|| payload.get("y")).and_then(|v| v.as_f64());
    let instance_path = payload
        .get("instance_path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let pos_fields = |act: &mut serde_json::Map<String, Value>, px: Option<f64>, py: Option<f64>| {
        if let Some(ip) = instance_path {
            act.insert("instance_path".to_string(), json!(ip));
        } else if let (Some(xv), Some(yv)) = (px, py) {
            if xv.is_finite() && yv.is_finite() {
                act.insert("x".to_string(), json!(xv.clamp(0.0, 4096.0)));
                act.insert("y".to_string(), json!(yv.clamp(0.0, 4096.0)));
            }
        }
    };

    let mut actions = Vec::new();
    match event.as_str() {
        "move" | "mousemove" | "moveto" => {
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!("moveTo"));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        "down" | "mousedown" | "mousebuttondown" => {
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!("mouseButtonDown"));
            m.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        "up" | "mouseup" | "mousebuttonup" => {
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!("mouseButtonUp"));
            m.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        "click" | "mousebuttonclick" => {
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!("mouseButtonClick"));
            m.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        "right_click" | "rightclick" => {
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!("mouseButtonClick"));
            m.insert("mouse_button".to_string(), json!("right"));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        "double_click" | "doubleclick" => {
            let mut m1 = serde_json::Map::new();
            m1.insert("action".to_string(), json!("mouseButtonClick"));
            m1.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut m1, x, y);
            let mut m2 = serde_json::Map::new();
            m2.insert("action".to_string(), json!("mouseButtonClick"));
            m2.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut m2, x, y);
            actions.push(Value::Object(m1));
            actions.push(Value::Object(m2));
        }
        "drag" => {
            let end_x = payload
                .get("to_x")
                .or_else(|| payload.get("end_x"))
                .or_else(|| payload.get("x2"))
                .and_then(|v| v.as_f64())
                .or(x);
            let end_y = payload
                .get("to_y")
                .or_else(|| payload.get("end_y"))
                .or_else(|| payload.get("y2"))
                .and_then(|v| v.as_f64())
                .or(y);
            let mut down = serde_json::Map::new();
            down.insert("action".to_string(), json!("mouseButtonDown"));
            down.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut down, x, y);
            let mut mv = serde_json::Map::new();
            mv.insert("action".to_string(), json!("moveTo"));
            pos_fields(&mut mv, end_x, end_y);
            let mut up = serde_json::Map::new();
            up.insert("action".to_string(), json!("mouseButtonUp"));
            up.insert("mouse_button".to_string(), json!(button));
            pos_fields(&mut up, end_x, end_y);
            actions.push(Value::Object(down));
            actions.push(Value::Object(mv));
            actions.push(Value::Object(up));
        }
        "wheel" | "scroll" | "scrollup" | "scrolldown" => {
            let delta = payload
                .get("delta_y")
                .or_else(|| payload.get("deltaY"))
                .and_then(|v| v.as_f64())
                .unwrap_or(if event == "scrollup" { -120.0 } else { 120.0 });
            let act_name = if event == "scrollup" || delta < 0.0 {
                "scrollUp"
            } else {
                "scrollDown"
            };
            let mut m = serde_json::Map::new();
            m.insert("action".to_string(), json!(act_name));
            pos_fields(&mut m, x, y);
            actions.push(Value::Object(m));
        }
        other => {
            return Err(format!(
                "unsupported mouse event '{other}' (expected click, double_click, right_click, down, up, move, drag, or wheel)"
            ));
        }
    }

    Ok(actions)
}

/// Forward a mouse click, drag, move, or wheel event to Roblox Studio via `user_mouse_input`.
pub fn forward_mouse(client: &mut McpClient, payload: &Value) -> Result<Value, String> {
    let actions = translate_mouse_payload(payload)?;
    client.user_mouse_input(actions)
}

/// Map a browser DOM `KeyboardEvent.key` or `code` to a Roblox `Enum.KeyCode` string.
pub fn normalize_roblox_keycode(raw: &str) -> String {
    if raw == " " {
        return "Space".to_string();
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "None".to_string();
    }
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return ch.to_ascii_uppercase().to_string();
        }
        return match ch {
            '0' => "Zero",
            '1' => "One",
            '2' => "Two",
            '3' => "Three",
            '4' => "Four",
            '5' => "Five",
            '6' => "Six",
            '7' => "Seven",
            '8' => "Eight",
            '9' => "Nine",
            '-' => "Minus",
            '=' => "Equals",
            '/' => "Slash",
            '.' => "Period",
            ',' => "Comma",
            _ => trimmed,
        }
        .to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("Key") {
        if rest.len() == 1 {
            return rest.to_ascii_uppercase();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("Digit") {
        if rest.len() == 1 {
            return normalize_roblox_keycode(rest);
        }
    }
    match trimmed {
        "Enter" => "Return".to_string(),
        "Esc" | "Escape" => "Escape".to_string(),
        "ArrowUp" => "Up".to_string(),
        "ArrowDown" => "Down".to_string(),
        "ArrowLeft" => "Left".to_string(),
        "ArrowRight" => "Right".to_string(),
        "Shift" | "ShiftLeft" => "LeftShift".to_string(),
        "ShiftRight" => "RightShift".to_string(),
        "Control" | "ControlLeft" => "LeftControl".to_string(),
        "ControlRight" => "RightControl".to_string(),
        "Alt" | "AltLeft" => "LeftAlt".to_string(),
        "AltRight" => "RightAlt".to_string(),
        "Spacebar" | "Space" => "Space".to_string(),
        "Delete" => "Delete".to_string(),
        "Backspace" => "Backspace".to_string(),
        "Tab" => "Tab".to_string(),
        other => other.to_string(),
    }
}

/// Translate browser keyboard JSON or raw official MCP `actions` into validated
/// Roblox Studio `user_keyboard_input` action records.
pub fn translate_keyboard_payload(payload: &Value) -> Result<Vec<Value>, String> {
    if let Some(arr) = payload.get("actions").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            return Err("keyboard input actions array must not be empty".to_string());
        }
        return Ok(arr.clone());
    }

    let event = payload
        .get("event")
        .or_else(|| payload.get("type"))
        .or_else(|| payload.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("press")
        .trim()
        .to_ascii_lowercase();

    let instance_path = payload
        .get("instance_path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(hk) = payload.get("hotkey") {
        let raw_tokens: Vec<String> = if let Some(arr) = hk.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = hk.as_str() {
            s.split('+')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        let norm = normalize_hotkey_tokens(&raw_tokens)?;
        check_hotkey_safety(&norm)?;
        let mut mods = Vec::new();
        let mut mains = Vec::new();
        for tok in &norm {
            match tok.as_str() {
                "ctrl" => mods.push("LeftControl"),
                "shift" => mods.push("LeftShift"),
                "alt" => mods.push("LeftAlt"),
                other => mains.push(normalize_roblox_keycode(other)),
            }
        }
        let mut out = Vec::new();
        for m in &mods {
            out.push(json!({ "action": "keyDown", "key_code": m }));
        }
        for k in &mains {
            out.push(json!({ "action": "keyPress", "key_code": k }));
        }
        for m in mods.iter().rev() {
            out.push(json!({ "action": "keyUp", "key_code": m }));
        }
        if out.is_empty() {
            return Err("hotkey produced no key actions".to_string());
        }
        return Ok(out);
    }

    if matches!(event.as_str(), "text" | "textinput" | "type") {
        let text = payload
            .get("text")
            .or_else(|| payload.get("text_inputs"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "keyboard textInput requires a 'text' string".to_string())?;
        if text.is_empty() || text.len() > 2048 || text.contains('\0') {
            return Err("keyboard text must be 1..=2048 bytes without NUL".to_string());
        }
        let mut m = serde_json::Map::new();
        m.insert("action".to_string(), json!("textInput"));
        m.insert("text_inputs".to_string(), json!(text));
        if let Some(ip) = instance_path {
            m.insert("instance_path".to_string(), json!(ip));
        }
        return Ok(vec![Value::Object(m)]);
    }

    let raw_key = payload
        .get("key_code")
        .or_else(|| payload.get("key"))
        .or_else(|| payload.get("code"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "keyboard input requires 'key' or 'key_code'".to_string())?;
    let key_code = normalize_roblox_keycode(raw_key);

    let act = match event.as_str() {
        "down" | "keydown" => "keyDown",
        "up" | "keyup" => "keyUp",
        "press" | "keypress" => "keyPress",
        other => {
            return Err(format!(
                "unsupported keyboard event '{other}' (expected keydown, keyup, press, or text)"
            ));
        }
    };

    let mut m = serde_json::Map::new();
    m.insert("action".to_string(), json!(act));
    m.insert("key_code".to_string(), json!(key_code));
    if let Some(ip) = instance_path {
        m.insert("instance_path".to_string(), json!(ip));
    }
    Ok(vec![Value::Object(m)])
}

/// Forward a keyboard press, hold, release, or text input event to Roblox Studio
/// via `user_keyboard_input`.
pub fn forward_keyboard(client: &mut McpClient, payload: &Value) -> Result<Value, String> {
    let actions = translate_keyboard_payload(payload)?;
    client.user_keyboard_input(actions)
}

/// Format a single `multipart/x-mixed-replace` HTTP stream chunk for `/api/studio/stream.mjpg`.
pub fn format_mjpeg_part(frame: &StudioFrame, boundary: &str) -> Vec<u8> {
    let header = format!(
        "--{boundary}\r\nContent-Type: {}\r\nContent-Length: {}\r\nX-Studio-Frame-Seq: {}\r\n\r\n",
        frame.mime,
        frame.bytes.len(),
        frame.seq
    );
    let mut out = Vec::with_capacity(header.len() + frame.bytes.len() + 2);
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&frame.bytes);
    out.extend_from_slice(b"\r\n");
    out
}

/// Validate Linux X11 display strings (`:0` .. `:999`).
pub fn validate_x11_display(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let Some(num) = trimmed.strip_prefix(':') else {
        return Err(format!(
            "invalid X11 display '{trimmed}' (expected ':99' format)"
        ));
    };
    if num.is_empty() || num.len() > 3 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "invalid X11 display '{trimmed}' (expected ':0' through ':999')"
        ));
    }
    Ok(trimmed.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse an X11 display string like `":99"` or `":99.0"` into its numeric display ID (`"99"`).
pub fn parse_x11_display_num(display: &str) -> Option<&str> {
    let rest = display.trim().strip_prefix(':')?;
    let num = rest.split('.').next()?;
    if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
        Some(num)
    } else {
        None
    }
}

#[cfg(unix)]
fn read_x11_lock_pid(lock_path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(lock_path).ok()?;
    text.trim().parse::<u32>().ok().filter(|&p| p > 0)
}

#[cfg(unix)]
fn x11_pid_is_live(pid: u32, proc_root: &Path) -> Option<bool> {
    if proc_root.is_dir() {
        Some(proc_root.join(pid.to_string()).exists())
    } else {
        None
    }
}

/// Testable implementation of `x11_socket_exists` (Gap #89): returns `false` when
/// `/tmp/.X11-unix/X{num}` is missing OR `/tmp/.X{num}-lock` references a dead PID
/// whose `/proc/{pid}` directory no longer exists on Linux.
#[cfg(unix)]
pub fn x11_socket_exists_in(tmp_dir: &Path, proc_root: &Path, display: &str) -> bool {
    let Some(num) = parse_x11_display_num(display) else {
        return false;
    };
    let sock = tmp_dir.join(".X11-unix").join(format!("X{num}"));
    if !sock.exists() {
        return false;
    }
    let lock = tmp_dir.join(format!(".X{num}-lock"));
    if lock.exists() {
        if let Some(pid) = read_x11_lock_pid(&lock) {
            if x11_pid_is_live(pid, proc_root) == Some(false) {
                return false;
            }
        }
    }
    true
}

/// Unlink stale `/tmp/.X{num}-lock` and `/tmp/.X11-unix/X{num}` ONLY when no live
/// process owns `.X{num}-lock` (matching either a dead `/proc/{pid}` or `expected_dead_pid`
/// from our just-terminated `xvfb_child`). Never removes files owned by a live PID.
#[cfg(unix)]
pub fn cleanup_stale_x11_lock_in(
    tmp_dir: &Path,
    proc_root: &Path,
    display: &str,
    expected_dead_pid: Option<u32>,
) -> bool {
    let Some(num) = parse_x11_display_num(display) else {
        return false;
    };
    let lock = tmp_dir.join(format!(".X{num}-lock"));
    let sock = tmp_dir.join(".X11-unix").join(format!("X{num}"));

    if lock.exists() {
        let Some(pid) = read_x11_lock_pid(&lock) else {
            return false;
        };
        let is_live = x11_pid_is_live(pid, proc_root);
        if is_live == Some(true) {
            return false;
        }
        let confirmed_dead = is_live == Some(false) || expected_dead_pid == Some(pid);
        if !confirmed_dead {
            return false;
        }
        let mut removed = false;
        if std::fs::remove_file(&lock).is_ok() {
            removed = true;
        }
        if sock.exists() && std::fs::remove_file(&sock).is_ok() {
            removed = true;
        }
        return removed;
    }

    if sock.exists() && expected_dead_pid.is_some() {
        return std::fs::remove_file(&sock).is_ok();
    }
    false
}

#[cfg(unix)]
fn cleanup_stale_x11_lock(display: &str) {
    let _ = cleanup_stale_x11_lock_in(Path::new("/tmp"), Path::new("/proc"), display, None);
}

#[cfg(unix)]
fn x11_socket_exists(display: &str) -> bool {
    x11_socket_exists_in(Path::new("/tmp"), Path::new("/proc"), display)
}

/// Returns `true` when `display` is the desktop XWayland display under an active Wayland
/// compositor (e.g. `WAYLAND_DISPLAY=wayland-1` + `DISPLAY=:0`), as opposed to an isolated
/// headless `Xvfb` like `:99` where `ffmpeg x11grab` works cleanly.
#[cfg(unix)]
fn is_wayland_desktop_display(display: &str) -> bool {
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    if !has_wayland {
        return false;
    }
    let host_disp = std::env::var("DISPLAY")
        .ok()
        .and_then(|d| normalize_x11_display_env(&d));
    host_disp.as_deref() == Some(display)
}

#[cfg(unix)]
fn is_vinegar_flatpak_running() -> bool {
    let Ok(ps) = Command::new("flatpak")
        .args(["ps", "--columns=instance,application"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    if !ps.status.success() {
        return false;
    }
    String::from_utf8_lossy(&ps.stdout).lines().any(|line| {
        let mut parts = line.split_whitespace();
        let _inst = parts.next();
        parts.next() == Some("org.vinegarhq.Vinegar")
    })
}

#[cfg(unix)]
fn find_on_path(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn current_unix_uid() -> Option<String> {
    let out = Command::new("id")
        .arg("-u")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            return Some(s);
        }
    }
    None
}

/// Bounded capacity of `StudioCuaLog`.
pub const MAX_CUA_LOG_ENTRIES: usize = 80;
/// Maximum retained PNG frames under `<data_dir>/cua-frames/`.
pub const MAX_CUA_FRAMES: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct StudioCuaEvent {
    pub id: u64,
    pub at_ms: u64,
    pub actor: String,
    pub kind: String,
    pub summary: String,
    pub norm_x: Option<f64>,
    pub norm_y: Option<f64>,
    pub to_norm_x: Option<f64>,
    pub to_norm_y: Option<f64>,
    pub key: Option<String>,
    pub hotkey: Option<Vec<String>>,
    pub frame_url: Option<String>,
    pub ok: bool,
}

impl StudioCuaEvent {
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("id".to_string(), json!(self.id));
        map.insert("at_ms".to_string(), json!(self.at_ms));
        map.insert("actor".to_string(), json!(self.actor));
        map.insert("kind".to_string(), json!(self.kind));
        map.insert("summary".to_string(), json!(self.summary));
        if let Some(v) = self.norm_x {
            map.insert("norm_x".to_string(), json!(v));
        }
        if let Some(v) = self.norm_y {
            map.insert("norm_y".to_string(), json!(v));
        }
        if let Some(v) = self.to_norm_x {
            map.insert("to_norm_x".to_string(), json!(v));
        }
        if let Some(v) = self.to_norm_y {
            map.insert("to_norm_y".to_string(), json!(v));
        }
        if let Some(ref k) = self.key {
            map.insert("key".to_string(), json!(k));
        }
        if let Some(ref hk) = self.hotkey {
            map.insert("hotkey".to_string(), json!(hk));
        }
        if let Some(ref u) = self.frame_url {
            map.insert("frame_url".to_string(), json!(u));
        }
        map.insert("ok".to_string(), json!(self.ok));
        Value::Object(map)
    }
}

#[derive(Clone, Debug)]
pub struct StudioCuaEventInput {
    pub actor: String,
    pub kind: String,
    pub summary: String,
    pub norm_x: Option<f64>,
    pub norm_y: Option<f64>,
    pub to_norm_x: Option<f64>,
    pub to_norm_y: Option<f64>,
    pub key: Option<String>,
    pub hotkey: Option<Vec<String>>,
    pub frame_url: Option<String>,
    pub ok: bool,
}

/// Thread-safe bounded 80-entry CUA action log + `cua_paused` human-takeover lock
/// broadcasting live events over `StudioStreamHub` (`/api/studio/stream`).
pub struct StudioCuaLog {
    events: Mutex<VecDeque<StudioCuaEvent>>,
    pub cua_paused: AtomicBool,
    next_id: AtomicU64,
    stream_hub: Mutex<Option<Arc<StudioStreamHub>>>,
}

impl StudioCuaLog {
    pub fn new(stream_hub: Option<Arc<StudioStreamHub>>) -> Self {
        StudioCuaLog {
            events: Mutex::new(VecDeque::with_capacity(MAX_CUA_LOG_ENTRIES)),
            cua_paused: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            stream_hub: Mutex::new(stream_hub),
        }
    }

    pub fn is_paused(&self) -> bool {
        self.cua_paused.load(Ordering::SeqCst)
    }

    pub fn pause(&self, actor: &str) -> StudioCuaEvent {
        self.cua_paused.store(true, Ordering::SeqCst);
        self.record(StudioCuaEventInput {
            actor: actor.to_string(),
            kind: "pause".to_string(),
            summary: "Agent CUA paused by human takeover".to_string(),
            norm_x: None,
            norm_y: None,
            to_norm_x: None,
            to_norm_y: None,
            key: None,
            hotkey: None,
            frame_url: None,
            ok: true,
        })
    }

    pub fn resume(&self, actor: &str) -> StudioCuaEvent {
        self.cua_paused.store(false, Ordering::SeqCst);
        self.record(StudioCuaEventInput {
            actor: actor.to_string(),
            kind: "resume".to_string(),
            summary: "Agent CUA resumed".to_string(),
            norm_x: None,
            norm_y: None,
            to_norm_x: None,
            to_norm_y: None,
            key: None,
            hotkey: None,
            frame_url: None,
            ok: true,
        })
    }

    pub fn record(&self, input: StudioCuaEventInput) -> StudioCuaEvent {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let ev = StudioCuaEvent {
            id,
            at_ms: now_ms(),
            actor: input.actor,
            kind: input.kind,
            summary: input.summary,
            norm_x: input.norm_x,
            norm_y: input.norm_y,
            to_norm_x: input.to_norm_x,
            to_norm_y: input.to_norm_y,
            key: input.key,
            hotkey: input.hotkey,
            frame_url: input.frame_url,
            ok: input.ok,
        };
        {
            let mut ring = self.events.lock().unwrap();
            if ring.len() >= MAX_CUA_LOG_ENTRIES {
                ring.pop_front();
            }
            ring.push_back(ev.clone());
        }
        if let Some(hub) = self.stream_hub.lock().unwrap().as_ref() {
            hub.push_cua_event(ev.to_json());
        }
        ev
    }

    pub fn recent(&self, limit: usize) -> Vec<StudioCuaEvent> {
        let cap = limit.min(MAX_CUA_LOG_ENTRIES);
        let ring = self.events.lock().unwrap();
        let skip = ring.len().saturating_sub(cap);
        ring.iter().skip(skip).cloned().collect()
    }

    pub fn since(&self, since_id: u64, limit: usize) -> Vec<StudioCuaEvent> {
        let cap = limit.min(MAX_CUA_LOG_ENTRIES);
        let ring = self.events.lock().unwrap();
        let filtered: Vec<StudioCuaEvent> =
            ring.iter().filter(|e| e.id > since_id).cloned().collect();
        let skip = filtered.len().saturating_sub(cap);
        filtered.into_iter().skip(skip).collect()
    }
}

static CUA_FRAME_FILE_SEQ: AtomicU64 = AtomicU64::new(1);

fn sanitize_cua_label(label: Option<&str>) -> String {
    let raw = label.unwrap_or("frame");
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    let trimmed = clean.trim_matches('_');
    if trimmed.is_empty() {
        "frame".to_string()
    } else {
        trimmed.to_string()
    }
}

fn ensure_safe_cua_frames_dir(data_dir: &Path) -> Result<PathBuf, String> {
    let cua_dir = data_dir.join("cua-frames");
    if let Ok(meta) = std::fs::symlink_metadata(&cua_dir) {
        if meta.file_type().is_symlink() {
            return Err("cua-frames directory must not be a symlink (CWE-59)".to_string());
        }
    }
    std::fs::create_dir_all(&cua_dir)
        .map_err(|e| format!("failed to create cua-frames directory: {e}"))?;
    let canon_root = std::fs::canonicalize(data_dir)
        .map_err(|e| format!("failed to canonicalize data_dir: {e}"))?;
    let canon_cua = std::fs::canonicalize(&cua_dir)
        .map_err(|e| format!("failed to canonicalize cua-frames: {e}"))?;
    if !canon_cua.starts_with(&canon_root) {
        return Err("cua-frames escaped data_dir containment (CWE-22)".to_string());
    }
    Ok(canon_cua)
}

fn prune_cua_frames(cua_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cua_dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("png") {
                return None;
            }
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            let name = e.file_name().to_string_lossy().to_string();
            Some((mtime, name, p))
        })
        .collect();
    if files.len() <= MAX_CUA_FRAMES {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let to_remove = files.len() - MAX_CUA_FRAMES;
    for (_, _, p) in files.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(p);
    }
}

fn convert_jpeg_to_png(jpeg_bytes: &[u8]) -> Result<Vec<u8>, String> {
    if let Ok(mut child) = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "mjpeg",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(jpeg_bytes);
        }
        if let Ok(out) = child.wait_with_output() {
            if out.status.success() && out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Ok(out.stdout);
            }
        }
    }
    let mut py = Command::new("python3")
        .args([
            "-c",
            "import sys,io; from PIL import Image; im=Image.open(io.BytesIO(sys.stdin.buffer.read())); buf=io.BytesIO(); im.save(buf, format='PNG'); sys.stdout.buffer.write(buf.getvalue())",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn python3 PIL for JPEG->PNG: {e}"))?;
    if let Some(mut stdin) = py.stdin.take() {
        let _ = stdin.write_all(jpeg_bytes);
    }
    let out = py
        .wait_with_output()
        .map_err(|e| format!("JPEG->PNG conversion failed: {e}"))?;
    if !out.status.success() || !out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("JPEG->PNG conversion failed: {err}"));
    }
    Ok(out.stdout)
}

#[cfg(unix)]
fn capture_x11_single_png(display: &str, resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    let valid = validate_x11_display(display)?;
    if !x11_socket_exists(&valid) {
        return Err(format!("X11 display {valid} socket is not active"));
    }
    let size = format!(
        "{}x{}",
        resolution[0].clamp(320, 3840),
        resolution[1].clamp(200, 2160)
    );
    let out = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "x11grab",
            "-video_size",
            &size,
            "-i",
            &valid,
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg x11grab single-frame capture failed: {e}"))?;
    if !out.status.success() || !out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffmpeg x11grab PNG capture on {valid} failed: {err}"));
    }
    Ok(out.stdout)
}

#[cfg(not(unix))]
fn capture_x11_single_png(_display: &str, _resolution: [u32; 2]) -> Result<Vec<u8>, String> {
    Err("X11 single-frame capture is only supported on Linux/Unix".to_string())
}

/// Capture the current Studio viewport into `<data_dir>/cua-frames/<id>.png` (CWE-22/59 safe,
/// pruned to 40 frames) from `StudioStreamHub` or validated single-frame X11 `ffmpeg x11grab`.
pub fn capture_studio_frame_png_from_hub(
    hub: &StudioStreamHub,
    state: &StudioEmbedState,
    data_dir: &Path,
    label: Option<&str>,
) -> Result<Value, String> {
    let (png_bytes, width, height, seq) = if let Some(frame) = hub.latest_frame() {
        let png = if frame.bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            frame.bytes.clone()
        } else {
            convert_jpeg_to_png(&frame.bytes)?
        };
        (png, frame.width, frame.height, frame.seq)
    } else if let Some(ref disp) = state.display {
        let png = capture_x11_single_png(disp, state.resolution)?;
        let stored = hub.push_frame(StudioFrame {
            seq: 0,
            ts_ms: now_ms(),
            mime: "image/png".to_string(),
            bytes: png.clone(),
            width: state.resolution[0],
            height: state.resolution[1],
            source: "cua_x11_png".to_string(),
        });
        (png, stored.width, stored.height, stored.seq)
    } else {
        return Err("No live Studio frame or active X11 display available for CUA screenshot".to_string());
    };

    let cua_dir = ensure_safe_cua_frames_dir(data_dir)?;
    let seq_num = CUA_FRAME_FILE_SEQ.fetch_add(1, Ordering::SeqCst);
    let clean_label = sanitize_cua_label(label);
    let filename = format!("cua-{:013}-{:04}-{}.png", now_ms(), seq_num, clean_label);
    let dest = cua_dir.join(&filename);
    if let Ok(meta) = std::fs::symlink_metadata(&dest) {
        if meta.file_type().is_symlink() {
            return Err("refusing to overwrite symlink in cua-frames (CWE-59)".to_string());
        }
    }
    std::fs::write(&dest, &png_bytes)
        .map_err(|e| format!("failed to write CUA frame PNG: {e}"))?;
    let canon_dest = std::fs::canonicalize(&dest)
        .map_err(|e| format!("failed to canonicalize saved CUA frame: {e}"))?;
    if !canon_dest.starts_with(&cua_dir) {
        let _ = std::fs::remove_file(&dest);
        return Err("saved CUA frame escaped containment (CWE-22)".to_string());
    }
    prune_cua_frames(&cua_dir);

    Ok(json!({
        "ok": true,
        "path": canon_dest.to_string_lossy(),
        "file": filename,
        "url": format!("/api/studio/cua/frame/{filename}"),
        "width": width,
        "height": height,
        "seq": seq,
        "display": state.display,
        "phase": state.phase,
        "place_open": state.place_open,
    }))
}

/// CWE-22/59 safe resolver for `GET /api/studio/cua/frame/<file>`.
pub fn resolve_cua_frame_path(data_dir: &Path, file: &str) -> Result<PathBuf, (u16, String)> {
    let trimmed = file.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.contains('\0')
        || !trimmed.ends_with(".png")
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err((400, "invalid CUA frame filename".to_string()));
    }
    let cua_dir = data_dir.join("cua-frames");
    if let Ok(meta) = std::fs::symlink_metadata(&cua_dir) {
        if meta.file_type().is_symlink() {
            return Err((403, "cua-frames directory symlink refused (CWE-59)".to_string()));
        }
    }
    let target = cua_dir.join(trimmed);
    let meta = match std::fs::symlink_metadata(&target) {
        Ok(m) => m,
        Err(_) => return Err((404, "CUA frame not found".to_string())),
    };
    if meta.file_type().is_symlink() {
        return Err((403, "symlink CUA frame refused (CWE-59)".to_string()));
    }
    let canon_dir = std::fs::canonicalize(&cua_dir)
        .map_err(|_| (404, "cua-frames directory not found".to_string()))?;
    let canon_file = std::fs::canonicalize(&target)
        .map_err(|_| (404, "CUA frame not found".to_string()))?;
    if !canon_file.starts_with(&canon_dir) {
        return Err((403, "CUA frame path traversal refused (CWE-22)".to_string()));
    }
    Ok(canon_file)
}

// ---------------------------------------------------------------------------
// Gap #84: Instant Local Roblox Studio Content Bridge (`rbxasset://openro/<slug>/...`)
// ---------------------------------------------------------------------------

static STUDIO_CONTENT_STAGE_SEQ: AtomicU64 = AtomicU64::new(1);

/// Sanitize a project slug for `<content_dir>/openro/<slug>/` (`[a-zA-Z0-9_-]`).
pub fn sanitize_studio_content_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 64 {
            break;
        }
    }
    let cleaned = out.trim_matches(|c| c == '-' || c == '_').to_string();
    if cleaned.is_empty() {
        "project".to_string()
    } else {
        cleaned
    }
}

/// Derive the default `studio_project_slug` from a project directory's folder name.
pub fn default_studio_project_slug(project_dir: &Path) -> String {
    let raw = project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let slugged = crate::projects::slug(raw);
    sanitize_studio_content_slug(&slugged)
}

fn infer_project_root_from_place_file(rbxlx_path: &Path) -> Option<PathBuf> {
    let mut cur = rbxlx_path.parent()?;
    for _ in 0..4 {
        if cur.join("openro.project.json").is_file() || cur.join("default.project.json").is_file() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
    None
}

/// Pure filesystem scanner for Linux Vinegar Studio content directories:
/// `<vinegar_root>/data/vinegar/versions/version-*/content` (newest version first).
pub fn find_vinegar_studio_content_dir(vinegar_root: &Path) -> Option<PathBuf> {
    let versions_dir = vinegar_root.join("data").join("vinegar").join("versions");
    let entries = std::fs::read_dir(&versions_dir).ok()?;
    let mut candidates: Vec<(SystemTime, String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() || !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().starts_with("version-") {
            continue;
        }
        let content_dir = path.join("content");
        let Ok(cmeta) = std::fs::symlink_metadata(&content_dir) else {
            continue;
        };
        if cmeta.file_type().is_symlink() || !cmeta.is_dir() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        candidates.push((modified, name, content_dir));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    candidates.into_iter().next().map(|(_, _, p)| p)
}

/// Pure filesystem scanner for Windows Roblox Studio content directories:
/// `<local_appdata>\Roblox\Versions\version-*\content`.
pub fn find_windows_studio_content_dir(local_appdata: &Path) -> Option<PathBuf> {
    let paths = find_windows_roblox_paths(local_appdata);
    let content_dir = paths.version_dir?.join("content");
    let meta = std::fs::symlink_metadata(&content_dir).ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    Some(content_dir)
}

/// Locate the active Roblox Studio `content/` directory for instant local `rbxasset://` staging.
///
/// Precedence:
/// 1. `OPENRO_STUDIO_CONTENT_DIR` env override
/// 2. Linux: `$HOME/.var/app/org.vinegarhq.Vinegar/data/vinegar/versions/version-*/content`
///    (honoring `OPENRO_VINEGAR_ROOT` if set)
/// 3. Windows: `%LOCALAPPDATA%\Roblox\Versions\version-*\content`
/// 4. macOS: `/Applications/RobloxStudio.app/Contents/Resources/content`
pub fn find_studio_content_dir() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("OPENRO_STUDIO_CONTENT_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() && !trimmed.contains('\0') && !trimmed.contains('\n') && !trimmed.contains('\r') {
            return Some(PathBuf::from(trimmed));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let vinegar_root = std::env::var("OPENRO_VINEGAR_ROOT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".var/app/org.vinegarhq.Vinegar"));
        find_vinegar_studio_content_dir(&vinegar_root)
    }

    #[cfg(target_os = "windows")]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        find_windows_studio_content_dir(&local_appdata)
    }

    #[cfg(target_os = "macos")]
    {
        let p = PathBuf::from("/Applications/RobloxStudio.app/Contents/Resources/content");
        std::fs::symlink_metadata(&p)
            .ok()
            .filter(|m| !m.file_type().is_symlink() && m.is_dir())
            .map(|_| p)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

fn is_superseded_or_hidden_name(name: &str) -> bool {
    name.starts_with('.') || name.to_ascii_lowercase().contains("_superseded")
}

fn has_ext_ci(name: &str, exts: &[&str]) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    exts.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// Collect project-relative `(rel_path, abs_path)` pairs eligible for local Studio content
/// staging (`art/cards/*/*.png`, `art/ui/**/*.png`, `art/store/*.png`, `assets/images/*.png`,
/// `assets/images/replacements/*.png`, `assets/audio/*.{wav,ogg,mp3}`).
/// Strictly skips symlinks, hidden files, and `*_superseded*` files, and path-jails every entry.
pub fn collect_studio_content_files(project_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(canon_root) = std::fs::canonicalize(project_dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    let mut push_checked = |rel: String, abs: PathBuf| {
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return;
            }
        } else {
            return;
        }
        if let Ok(canon) = std::fs::canonicalize(&abs) {
            if canon.starts_with(&canon_root) {
                out.push((rel, abs));
            }
        }
    };

    // Helper: direct non-symlink files in `<project_dir>/<rel_dir>` matching extensions.
    let collect_flat = |rel_dir: &str, exts: &[&str], push: &mut dyn FnMut(String, PathBuf)| {
        let dir = project_dir.join(rel_dir);
        let Ok(dmeta) = std::fs::symlink_metadata(&dir) else {
            return;
        };
        if dmeta.file_type().is_symlink() || !dmeta.is_dir() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() || !ft.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if is_superseded_or_hidden_name(&name) || !has_ext_ci(&name, exts) {
                continue;
            }
            push(format!("{rel_dir}/{name}"), entry.path());
        }
    };

    // 1. art/cards/*/*.png (one level of card subdirs)
    {
        let cards_dir = project_dir.join("art/cards");
        if std::fs::symlink_metadata(&cards_dir)
            .ok()
            .is_some_and(|m| !m.file_type().is_symlink() && m.is_dir())
        {
            if let Ok(rd) = std::fs::read_dir(&cards_dir) {
                for entry in rd.flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    if ft.is_symlink() || !ft.is_dir() {
                        continue;
                    }
                    let card_name = entry.file_name().to_string_lossy().to_string();
                    if is_superseded_or_hidden_name(&card_name) {
                        continue;
                    }
                    collect_flat(
                        &format!("art/cards/{card_name}"),
                        &["png"],
                        &mut push_checked,
                    );
                }
            }
        }
    }

    // 2. art/ui/**/*.png (recursive, excluding symlinks and hidden dirs)
    {
        let ui_dir = project_dir.join("art/ui");
        if std::fs::symlink_metadata(&ui_dir)
            .ok()
            .is_some_and(|m| !m.file_type().is_symlink() && m.is_dir())
        {
            let mut stack: Vec<(PathBuf, String, u8)> = vec![(ui_dir, "art/ui".to_string(), 0)];
            while let Some((dir, rel_dir, depth)) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for entry in rd.flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    if ft.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if is_superseded_or_hidden_name(&name) {
                        continue;
                    }
                    let child_rel = format!("{rel_dir}/{name}");
                    if ft.is_dir() && depth < 8 {
                        stack.push((entry.path(), child_rel, depth + 1));
                    } else if ft.is_file() && has_ext_ci(&name, &["png"]) {
                        push_checked(child_rel, entry.path());
                    }
                }
            }
        }
    }

    // 3. art/store/*.png
    collect_flat("art/store", &["png"], &mut push_checked);
    // 4. assets/images/*.png
    collect_flat("assets/images", &["png"], &mut push_checked);
    // 5. assets/images/replacements/*.png
    collect_flat("assets/images/replacements", &["png"], &mut push_checked);
    // 6. assets/audio/*.{wav,ogg,mp3}
    collect_flat("assets/audio", &["wav", "ogg", "mp3"], &mut push_checked);
    // 7. assets/meshes/*.{mesh,png,glb} (excluding preview renders; #92)
    {
        let meshes_dir = project_dir.join("assets/meshes");
        if std::fs::symlink_metadata(&meshes_dir)
            .ok()
            .is_some_and(|m| !m.file_type().is_symlink() && m.is_dir())
        {
            if let Ok(rd) = std::fs::read_dir(&meshes_dir) {
                for entry in rd.flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    if ft.is_symlink() || !ft.is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    let lower = name.to_ascii_lowercase();
                    if is_superseded_or_hidden_name(&name) || lower.contains("_preview") {
                        continue;
                    }
                    if has_ext_ci(&name, &["mesh", "png", "glb"]) {
                        push_checked(format!("assets/meshes/{name}"), entry.path());
                    }
                }
            }
        }
    }

    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn ensure_safe_dir_no_symlink(base: &Path, rel_parts: &[&str]) -> Result<PathBuf, String> {
    let mut cur = base.to_path_buf();
    for part in rel_parts {
        cur.push(part);
        if let Ok(meta) = std::fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "refusing to traverse symlink directory '{}' (CWE-59)",
                    cur.display()
                ));
            }
            if !meta.is_dir() {
                return Err(format!(
                    "expected directory at '{}' but found non-directory",
                    cur.display()
                ));
            }
        } else {
            std::fs::create_dir(&cur)
                .map_err(|e| format!("failed to create directory '{}': {e}", cur.display()))?;
        }
    }
    Ok(cur)
}

/// Stage local project images and audio into `<studio_content_dir>/openro/<slug>/<rel_path>`
/// so Roblox Studio renders `rbxasset://openro/<slug>/<rel_path>` instantly without waiting
/// for Open Cloud moderation (Gap #84).
///
/// Path-jailed, skips symlinks, skips unchanged files by byte length + SHA-256 content hash
/// (unless `force` is true), and writes atomically via temp file + rename.
pub fn stage_project_studio_content(
    project_dir: &Path,
    project_slug: &str,
) -> Result<Value, String> {
    stage_project_studio_content_with_options(project_dir, project_slug, false)
}

/// Configurable variant of `stage_project_studio_content` supporting `force` overwrites (Gap #95).
pub fn stage_project_studio_content_with_options(
    project_dir: &Path,
    project_slug: &str,
    force: bool,
) -> Result<Value, String> {
    if !project_dir.is_dir() {
        return Err(format!(
            "project directory '{}' does not exist",
            project_dir.display()
        ));
    }
    let slug = sanitize_studio_content_slug(project_slug);
    let content_dir = find_studio_content_dir().ok_or_else(|| {
        "Roblox Studio content directory not found (set OPENRO_STUDIO_CONTENT_DIR or install Roblox Studio / Vinegar)".to_string()
    })?;

    if let Ok(meta) = std::fs::symlink_metadata(&content_dir) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing symlink Studio content directory '{}' (CWE-59)",
                content_dir.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!(
                "Studio content path '{}' is not a directory",
                content_dir.display()
            ));
        }
    } else {
        std::fs::create_dir_all(&content_dir).map_err(|e| {
            format!(
                "failed to create Studio content directory '{}': {e}",
                content_dir.display()
            )
        })?;
    }

    let canon_content = std::fs::canonicalize(&content_dir).map_err(|e| {
        format!(
            "failed to canonicalize Studio content directory '{}': {e}",
            content_dir.display()
        )
    })?;
    let dest_root = ensure_safe_dir_no_symlink(&canon_content, &["openro", &slug])?;
    let canon_dest_root = std::fs::canonicalize(&dest_root).map_err(|e| {
        format!(
            "failed to canonicalize staged content root '{}': {e}",
            dest_root.display()
        )
    })?;
    if !canon_dest_root.starts_with(&canon_content) {
        return Err("staged content root escaped Studio content directory (CWE-22)".to_string());
    }

    let candidates = collect_studio_content_files(project_dir);
    let mut staged_count = 0usize;
    let mut overwritten_count = 0usize;
    let mut skipped_count = 0usize;
    let mut files: Vec<String> = Vec::with_capacity(candidates.len());

    for (rel, src_abs) in candidates {
        let rel_path = Path::new(&rel);
        let mut normal_parts: Vec<&str> = Vec::new();
        let mut valid_rel = true;
        for comp in rel_path.components() {
            match comp {
                std::path::Component::Normal(seg) => {
                    if let Some(s) = seg.to_str() {
                        normal_parts.push(s);
                    } else {
                        valid_rel = false;
                        break;
                    }
                }
                _ => {
                    valid_rel = false;
                    break;
                }
            }
        }
        if !valid_rel || normal_parts.is_empty() {
            continue;
        }

        let Ok(src_meta) = std::fs::symlink_metadata(&src_abs) else {
            continue;
        };
        if src_meta.file_type().is_symlink() || !src_meta.is_file() {
            continue;
        }
        let src_len = src_meta.len();
        let src_mtime = src_meta.modified().ok();

        let (parent_parts, file_name) = normal_parts.split_at(normal_parts.len() - 1);
        let dst_parent = ensure_safe_dir_no_symlink(&canon_dest_root, parent_parts)?;
        let dst_abs = dst_parent.join(file_name[0]);

        let bytes = std::fs::read(&src_abs)
            .map_err(|e| format!("failed to read '{}': {e}", src_abs.display()))?;

        let mut was_existing = false;
        if let Ok(dst_meta) = std::fs::symlink_metadata(&dst_abs) {
            if dst_meta.file_type().is_symlink() {
                continue;
            }
            if dst_meta.is_file() {
                was_existing = true;
                if !force && dst_meta.len() == src_len {
                    if let Ok(dst_bytes) = std::fs::read(&dst_abs) {
                        if crate::opencloud::sha256_hex(&dst_bytes)
                            == crate::opencloud::sha256_hex(&bytes)
                        {
                            skipped_count += 1;
                            files.push(rel);
                            continue;
                        }
                    }
                }
            }
        }

        let seq = STUDIO_CONTENT_STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dst_parent.join(format!(
            ".openro-stage-{}-{}.tmp",
            std::process::id(),
            seq
        ));
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .map_err(|e| format!("failed to create staging temp file '{}': {e}", tmp_path.display()))?;
            if let Err(e) = f.write_all(&bytes).and_then(|_| f.flush()) {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("failed to write staged asset '{rel}': {e}"));
            }
            if let Some(sm) = src_mtime {
                let _ = f.set_modified(sm);
            }
        }
        if let Err(e) = std::fs::rename(&tmp_path, &dst_abs) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("failed to move staged asset '{rel}' into place: {e}"));
        }
        if let Ok(canon_dst) = std::fs::canonicalize(&dst_abs) {
            if !canon_dst.starts_with(&canon_dest_root) {
                let _ = std::fs::remove_file(&dst_abs);
                return Err(format!("staged asset '{rel}' escaped content root (CWE-22)"));
            }
        }
        staged_count += 1;
        if was_existing {
            overwritten_count += 1;
        }
        files.push(rel);
    }

    Ok(json!({
        "ok": true,
        "slug": slug,
        "content_dir": canon_content.to_string_lossy(),
        "target_dir": canon_dest_root.to_string_lossy(),
        "prefix": format!("rbxasset://openro/{slug}/"),
        "staged_count": staged_count,
        "overwritten_count": overwritten_count,
        "skipped_count": skipped_count,
        "force": force,
        "files": files,
    }))
}
