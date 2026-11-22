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
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
            let is_studio = cmd_text.contains("RobloxStudioBeta.exe")
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

/// Extract normalized `[{ studio_id, place_name, datamodel_type }]` records from
/// a `list_roblox_studios` MCP JSON response.
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
            .and_then(|v| v.as_str())
            .map(String::from);
        let dm = item
            .get("active_datamodel")
            .or_else(|| item.get("datamodel_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("Edit")
            .to_string();
        out.push(json!({
            "studio_id": id,
            "place_name": place_name,
            "datamodel_type": dm,
        }));
    }
    out
}

/// Passively query an MCP command for live open Studio sessions without spawning Studio (#118).
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
    parse_running_studios(&val)
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

        let guidance = if already_logged_in {
            "Found your logged-in Roblox Studio — click 'Grab & Connect' to attach immediately."
                .to_string()
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
        let guidance = if already_logged_in {
            "Found your logged-in Roblox Studio — click 'Grab & Connect' to attach immediately."
                .to_string()
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
        json!({
            "phase": self.phase,
            "platform": self.platform,
            "embedded": self.embedded,
            "mcp_connected": self.mcp_connected,
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
            "detection": detection.to_json(),
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
        if self.studio_child.is_none() && self.xvfb_child.is_none() && self.ffmpeg_child.is_none() {
            self.state.embedded = false;
            self.state.phase = "idle".to_string();
        }
        self.state.studio_id = None;
        self.state.tools.clear();
        self.state.official_tools_count = 0;
        self.state.playing = false;
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

/// Forward a mouse event directly into an X11 display (`DISPLAY=:99`) via `libX11` + `libXtst`
/// so users can click buttons or complete Roblox Studio sign-in inside the embedded viewport
/// even before Studio MCP attaches (#35).
#[cfg(unix)]
pub fn send_x11_mouse(
    display: &str,
    resolution: [u32; 2],
    action: &str,
    x: f64,
    y: f64,
    button: &str,
    delta_y: f64,
) -> Result<Value, String> {
    let valid = validate_x11_display(display)?;
    let w = resolution[0].clamp(320, 3840) as f64;
    let h = resolution[1].clamp(200, 2160) as f64;
    let (px, py) = if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) && (x > 0.0 || y > 0.0) {
        ((x * w).round() as i32, (y * h).round() as i32)
    } else {
        (x.round() as i32, y.round() as i32)
    };
    let px = px.clamp(0, (w as i32) - 1);
    let py = py.clamp(0, (h as i32) - 1);
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
elif action == "down":
    xtst.XTestFakeButtonEvent(dpy, btn, 1, 0)
elif action == "up":
    xtst.XTestFakeButtonEvent(dpy, btn, 0, 0)
elif action == "wheel":
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
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("X11 mouse pass-through failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("X11 mouse pass-through on {valid} failed: {err}"));
    }
    Ok(json!({ "ok": true, "transport": "x11", "display": valid, "x": px, "y": py, "action": action }))
}

#[cfg(not(unix))]
pub fn send_x11_mouse(
    _display: &str,
    _resolution: [u32; 2],
    _action: &str,
    _x: f64,
    _y: f64,
    _button: &str,
    _delta_y: f64,
) -> Result<Value, String> {
    Err("X11 mouse pass-through is only available on Linux".to_string())
}

/// Forward a keyboard key press or typed text directly into an X11 display (`DISPLAY=:99`)
/// via `libX11` + `libXtst` while Studio is awaiting auth/sign-in (#35).
#[cfg(unix)]
pub fn send_x11_keyboard(
    display: &str,
    action: &str,
    key: Option<&str>,
    text: Option<&str>,
) -> Result<Value, String> {
    let valid = validate_x11_display(display)?;
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
action, key, text = sys.argv[2], sys.argv[3], sys.argv[4]
KEY_MAP = {
    "enter": "Return", "return": "Return", "tab": "Tab", "escape": "Escape", "esc": "Escape",
    "backspace": "BackSpace", "delete": "Delete", "space": "space", " ": "space",
    "arrowup": "Up", "up": "Up", "arrowdown": "Down", "down": "Down",
    "arrowleft": "Left", "left": "Left", "arrowright": "Right", "right": "Right",
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
def press_sym(sym_name, need_shift=False, mode="press"):
    ks = x11.XStringToKeysym(sym_name.encode())
    if not ks:
        return
    kc = x11.XKeysymToKeycode(dpy, ks)
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
if text:
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
                manager.sync_from_client(&client, Some(explicit_cmd));
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
            cmd.arg("run");
            if let Some(pf) = config.place_file.as_ref() {
                if let Some(parent) = pf.parent() {
                    cmd.arg(format!("--filesystem={}", parent.display()));
                }
                cmd.arg("--file-forwarding");
                cmd.arg("org.vinegarhq.Vinegar");
                cmd.arg("@@");
                cmd.arg(pf);
                cmd.arg("@@");
            } else {
                cmd.arg("org.vinegarhq.Vinegar");
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
                    manager.state.started_at_ms = Some(now_ms());
                    manager.sync_from_client(&client, Some(mcp_cmd));
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
            Ok(EmbedLaunchOutcome::Connected(state, Box::new(client)))
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
            cmd.arg("run");
            if let Some(pf) = config.place_file.as_ref() {
                if let Some(parent) = pf.parent() {
                    cmd.arg(format!("--filesystem={}", parent.display()));
                }
                cmd.arg("--file-forwarding");
                cmd.arg("org.vinegarhq.Vinegar");
                cmd.arg("@@");
                cmd.arg(pf);
                cmd.arg("@@");
            } else {
                cmd.arg("org.vinegarhq.Vinegar");
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
                    cmd.arg(pf);
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
        let _ = child.kill();
        let _ = child.wait();
    }
    manager.state.phase = "stopped".to_string();
    manager.state.embedded = false;
    manager.state.mcp_connected = false;
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

    let x = payload.get("x").and_then(|v| v.as_f64());
    let y = payload.get("y").and_then(|v| v.as_f64());
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
                .get("end_x")
                .or_else(|| payload.get("x2"))
                .and_then(|v| v.as_f64())
                .or(x);
            let end_y = payload
                .get("end_y")
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

#[cfg(unix)]
fn x11_socket_exists(display: &str) -> bool {
    if let Some(num) = display.strip_prefix(':') {
        Path::new(&format!("/tmp/.X11-unix/X{num}")).exists()
    } else {
        false
    }
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
