//! Cross-platform chromeless standalone desktop window launcher for OpenRO.
//!
//! Launches the OpenRO Studio Workbench in an isolated chromeless app window:
//! - **Linux:** `chromium` / `chromium-browser` / `google-chrome` / `brave` / `microsoft-edge`
//!   with `--app=<url>`, isolated `--user-data-dir`, and `--class=OpenRO` (falling back to
//!   `xdg-open` if no Chromium-family browser is present).
//! - **Windows:** `msedge.exe` / `chrome.exe` / `brave.exe` (detected on `PATH` and standard
//!   `%ProgramFiles%` / `%ProgramFiles(x86)%` / `%LOCALAPPDATA%` paths) with `--app=<url>`
//!   and isolated `--user-data-dir` (falling back to `cmd /C start "" <url>`).
//! - **macOS:** Chrome/Edge/Brave `--app=<url>` or `open <url>`.
//!
//! Strictly respects `--no-open`, `--headless`, `OPENRO_NO_OPEN=1`, `OPENRO_HEADLESS=1`,
//! `NO_BROWSER=1`, and `CI=1`/`true`, and never spawns a GUI window on a headless Linux box
//! lacking both `DISPLAY` and `WAYLAND_DISPLAY`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Configuration for launching the chromeless desktop app window.
#[derive(Clone, Debug)]
pub struct DesktopLaunchConfig {
    pub url: String,
    pub data_dir: PathBuf,
    pub window_width: u32,
    pub window_height: u32,
    pub app_class: String,
    pub no_open: bool,
}

impl DesktopLaunchConfig {
    pub fn new(url: impl Into<String>, data_dir: PathBuf) -> Self {
        DesktopLaunchConfig {
            url: url.into(),
            data_dir,
            window_width: 1440,
            window_height: 900,
            app_class: "OpenRO".to_string(),
            no_open: false,
        }
    }
}

/// Resolved command-line plan before spawning (pure and unit-testable).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub chromeless: bool,
    pub platform: &'static str,
}

/// Outcome of attempting to open the desktop window.
#[derive(Clone, Debug)]
pub struct DesktopLaunchOutcome {
    pub launched: bool,
    pub skipped: bool,
    pub chromeless: bool,
    pub program: Option<String>,
    pub reason: String,
}

impl DesktopLaunchOutcome {
    pub fn to_json(&self) -> Value {
        json!({
            "launched": self.launched,
            "skipped": self.skipped,
            "chromeless": self.chromeless,
            "program": self.program,
            "reason": self.reason,
        })
    }
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key).ok().is_some_and(|raw| {
        let v = raw.trim();
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

/// Returns `true` when OpenRO should open its standalone desktop window on startup.
///
/// Honors CLI flags (`--no-open`, `--headless`) and environment guards
/// (`OPENRO_NO_OPEN=1`, `OPENRO_HEADLESS=1`, `NO_BROWSER=1`, `CI=true`/`1`).
/// On Linux, also requires `DISPLAY` or `WAYLAND_DISPLAY` to be present.
pub fn should_auto_open(cli_args: &[String]) -> bool {
    if cli_args
        .iter()
        .any(|a| matches!(a.as_str(), "--no-open" | "--headless" | "--server-only"))
    {
        return false;
    }
    if env_truthy("OPENRO_NO_OPEN")
        || env_truthy("OPENRO_HEADLESS")
        || env_truthy("NO_BROWSER")
        || env_truthy("CI")
    {
        return false;
    }
    has_graphical_session()
}

fn has_graphical_session() -> bool {
    #[cfg(target_os = "linux")]
    {
        let display = std::env::var("DISPLAY")
            .ok()
            .is_some_and(|v| !v.trim().is_empty());
        let wayland = std::env::var("WAYLAND_DISPLAY")
            .ok()
            .is_some_and(|v| !v.trim().is_empty());
        display || wayland
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Build the isolated browser profile path used for OpenRO's chromeless desktop window.
pub fn desktop_profile_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("desktop-profile")
}

/// Construct a chromeless Chromium-family launch plan (`--app=<url>`).
pub fn build_chromium_app_plan(
    program: impl Into<String>,
    cfg: &DesktopLaunchConfig,
    platform: &'static str,
) -> DesktopLaunchPlan {
    let profile = desktop_profile_dir(&cfg.data_dir);
    let mut args = vec![
        format!("--app={}", cfg.url),
        format!("--user-data-dir={}", profile.display()),
        format!("--window-size={},{}", cfg.window_width, cfg.window_height),
        "--new-window".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
    ];
    if platform == "linux" && !cfg.app_class.trim().is_empty() {
        args.push(format!("--class={}", cfg.app_class.trim()));
    }
    DesktopLaunchPlan {
        program: program.into(),
        args,
        chromeless: true,
        platform,
    }
}

/// Candidate Windows browser paths given standard Windows root folders.
/// Pure function so Windows resolution logic is unit-tested on Linux as well.
pub fn windows_browser_candidates(
    program_files: Option<&Path>,
    program_files_x86: Option<&Path>,
    local_appdata: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let rels: &[(&str, bool)] = &[
        ("Microsoft\\Edge\\Application\\msedge.exe", false),
        ("Google\\Chrome\\Application\\chrome.exe", false),
        ("BraveSoftware\\Brave-Browser\\Application\\brave.exe", false),
    ];
    for (rel, _) in rels {
        // Use forward-slash join segments so PathBuf works consistently on Linux tests & Windows.
        let parts: Vec<&str> = rel.split('\\').collect();
        for base in [program_files_x86, program_files, local_appdata]
            .into_iter()
            .flatten()
        {
            let mut p = base.to_path_buf();
            for seg in &parts {
                p.push(seg);
            }
            out.push(p);
        }
    }
    out
}

/// Resolve the best available standalone desktop browser command for the current OS.
pub fn resolve_desktop_command(cfg: &DesktopLaunchConfig) -> Option<DesktopLaunchPlan> {
    if let Ok(explicit) = std::env::var("OPENRO_DESKTOP_BROWSER") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Some(build_chromium_app_plan(
                trimmed.to_string(),
                cfg,
                current_platform(),
            ));
        }
    }

    #[cfg(target_os = "linux")]
    {
        for bin in [
            "chromium",
            "chromium-browser",
            "google-chrome",
            "google-chrome-stable",
            "brave",
            "brave-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
        ] {
            if let Some(found) = find_on_path(bin) {
                return Some(build_chromium_app_plan(
                    found.to_string_lossy().into_owned(),
                    cfg,
                    "linux",
                ));
            }
        }
        if let Some(xdg) = find_on_path("xdg-open") {
            return Some(DesktopLaunchPlan {
                program: xdg.to_string_lossy().into_owned(),
                args: vec![cfg.url.clone()],
                chromeless: false,
                platform: "linux",
            });
        }
        None
    }

    #[cfg(target_os = "windows")]
    {
        for bin in ["msedge.exe", "chrome.exe", "brave.exe"] {
            if let Some(found) = find_on_path(bin) {
                return Some(build_chromium_app_plan(
                    found.to_string_lossy().into_owned(),
                    cfg,
                    "windows",
                ));
            }
        }
        let pf = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        for candidate in
            windows_browser_candidates(pf.as_deref(), pf86.as_deref(), local.as_deref())
        {
            if candidate.is_file() {
                return Some(build_chromium_app_plan(
                    candidate.to_string_lossy().into_owned(),
                    cfg,
                    "windows",
                ));
            }
        }
        Some(DesktopLaunchPlan {
            program: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                "start".to_string(),
                "".to_string(),
                cfg.url.clone(),
            ],
            chromeless: false,
            platform: "windows",
        })
    }

    #[cfg(target_os = "macos")]
    {
        for app_bin in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ] {
            if Path::new(app_bin).is_file() {
                return Some(build_chromium_app_plan(app_bin, cfg, "macos"));
            }
        }
        Some(DesktopLaunchPlan {
            program: "open".to_string(),
            args: vec![cfg.url.clone()],
            chromeless: false,
            platform: "macos",
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = cfg;
        None
    }
}

/// Launch OpenRO in a chromeless standalone desktop app window if allowed by CLI/env.
pub fn launch_desktop_window(
    url: &str,
    data_dir: &Path,
    cli_args: &[String],
) -> DesktopLaunchOutcome {
    let mut cfg = DesktopLaunchConfig::new(url, data_dir.to_path_buf());
    cfg.no_open = !should_auto_open(cli_args);
    launch_desktop_window_with_config(&cfg)
}

/// Launch using an explicit `DesktopLaunchConfig`.
pub fn launch_desktop_window_with_config(cfg: &DesktopLaunchConfig) -> DesktopLaunchOutcome {
    if cfg.no_open {
        return DesktopLaunchOutcome {
            launched: false,
            skipped: true,
            chromeless: false,
            program: None,
            reason: "desktop window auto-open disabled (--no-open / OPENRO_NO_OPEN / headless)"
                .to_string(),
        };
    }
    if !(cfg.url.starts_with("http://127.0.0.1:")
        || cfg.url.starts_with("http://localhost:")
        || cfg.url.starts_with("https://"))
    {
        return DesktopLaunchOutcome {
            launched: false,
            skipped: true,
            chromeless: false,
            program: None,
            reason: format!("refused to open non-HTTP(S) URL '{}'", cfg.url),
        };
    }

    let Some(plan) = resolve_desktop_command(cfg) else {
        return DesktopLaunchOutcome {
            launched: false,
            skipped: true,
            chromeless: false,
            program: None,
            reason: "no compatible desktop browser found on PATH".to_string(),
        };
    };

    if plan.chromeless {
        let _ = std::fs::create_dir_all(desktop_profile_dir(&cfg.data_dir));
    }

    match Command::new(&plan.program)
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => DesktopLaunchOutcome {
            launched: true,
            skipped: false,
            chromeless: plan.chromeless,
            program: Some(plan.program),
            reason: if plan.chromeless {
                "launched chromeless desktop app window".to_string()
            } else {
                "launched system browser fallback".to_string()
            },
        },
        Err(e) => DesktopLaunchOutcome {
            launched: false,
            skipped: false,
            chromeless: plan.chromeless,
            program: Some(plan.program),
            reason: format!("failed to launch desktop browser: {e}"),
        },
    }
}

pub fn current_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "unknown"
    }
}

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
