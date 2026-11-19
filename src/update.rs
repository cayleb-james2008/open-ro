//! Self-update from this project's GitHub releases.
//!
//! What it does, in order: ask `releases/latest` of the project's repository
//! for the newest published (not draft, not pre-release) version; if it is
//! newer than the running one and has an asset for this platform
//! (`<bin>-<target>[.exe]`), tell the user. Applying an update downloads that
//! asset, checks its SHA-256 against the release's `SHA256SUMS`, runs the new
//! file with `--version` to prove it is the expected build, and only then
//! replaces the running executable (atomically on Unix; by renaming the old
//! file aside on Windows). The new version runs after a restart.
//!
//! Safety rules: nothing is fetched when the mode is `off`; the API base must be
//! https (plain http is allowed for a loopback address only, so tests can use a
//! local server); a download is size-capped and must match its declared size;
//! a cargo-built (`target/`) binary is never replaced unless told to; a failed
//! step leaves the installed executable untouched.
//!
//! Environment (prefix is the app's name in capitals, here `OPENRO`):
//!   `<PREFIX>_UPDATE_MODE`  `notify` (default: check and tell), `auto` (also
//!                           download and replace, restart still needed), `off`
//!   `<PREFIX>_UPDATE_REPO`  `owner/name` of the GitHub repository
//!   `<PREFIX>_UPDATE_API`   API base, default `https://api.github.com`
//!   `<PREFIX>_UPDATE_DEV`   `1` allows replacing a cargo-built binary (tests)

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_ASSET_BYTES: u64 = 300 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Off,
    Notify,
    Auto,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "0" | "false" | "no" => Mode::Off,
            "auto" | "apply" => Mode::Auto,
            _ => Mode::Notify,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Notify => "notify",
            Mode::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Executable name, for example `openro-rs`; asset names start with it.
    pub bin: &'static str,
    /// Version of the running build (`CARGO_PKG_VERSION`).
    pub current: &'static str,
    /// `owner/name`.
    pub repo: String,
    /// API base, no trailing slash.
    pub api: String,
    pub mode: Mode,
    /// Path of the running executable, captured at start (it is unlinked from
    /// its inode when replaced, so it cannot be asked for later).
    pub exe: PathBuf,
    pub allow_dev: bool,
    /// Whether this process may replace its own executable at all (false when
    /// it is shared by several people behind a gate).
    pub may_apply: bool,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: Option<u64>,
    pub sums_url: Option<String>,
    pub digest: Option<String>,
}

#[derive(Default, Debug)]
struct State {
    checked_at: Option<u64>,
    latest: Option<Release>,
    error: Option<String>,
    /// Version staged on disk, waiting for a restart.
    applied: Option<String>,
}

pub struct Updater {
    pub cfg: Config,
    state: Mutex<State>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `v1.2.3` or `1.2.3`. Anything else (pre-release suffixes included) is not
/// a release this updater will move to.
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let t = tag.trim().trim_start_matches('v');
    let mut it = t.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The Rust target this binary was built for, as release assets are named.
pub fn target() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else {
        "unsupported"
    }
}

pub fn asset_name(bin: &str) -> String {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    format!("{bin}-{}{ext}", target())
}

/// A URL this updater may fetch: https, or http to a loopback address.
pub fn url_allowed(url: &str) -> bool {
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        let host = r.split(['/', ':']).next().unwrap_or("");
        return matches!(host, "127.0.0.1" | "localhost" | "[::1]");
    } else {
        return false;
    };
    !rest.is_empty() && !rest.starts_with('/')
}

/// Turns the `releases/latest` JSON into a `Release` for this platform.
pub fn parse_release(v: &Value, bin: &str) -> Result<Release, String> {
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Err("the newest release is a draft or pre-release".to_string());
    }
    let tag = v["tag_name"].as_str().ok_or("release has no tag_name")?;
    parse_version(tag).ok_or_else(|| format!("release tag '{tag}' is not a plain version"))?;
    let version = tag.trim_start_matches('v').to_string();
    let want = asset_name(bin);
    let assets = v["assets"].as_array().ok_or("release has no assets")?;
    let find = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name));
    let asset = find(&want).ok_or_else(|| format!("release {version} has no build for this platform ({want})"))?;
    let asset_url = asset["browser_download_url"]
        .as_str()
        .ok_or("asset has no download url")?
        .to_string();
    let sums_url = find("SHA256SUMS").and_then(|a| a["browser_download_url"].as_str()).map(|s| s.to_string());
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|d| d.to_ascii_lowercase());
    Ok(Release {
        version,
        url: v["html_url"].as_str().unwrap_or("").to_string(),
        notes: v["body"].as_str().unwrap_or("").chars().take(4000).collect(),
        asset_name: want,
        asset_url,
        asset_size: asset["size"].as_u64(),
        sums_url,
        digest,
    })
}

/// The expected hash of `asset` from a `SHA256SUMS` file (`<hex>  <name>`).
pub fn hash_from_sums(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let h = it.next()?;
        let n = it.next()?.trim_start_matches('*');
        (n == asset && h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- network

/// GET with a size cap. Redirects are followed (release assets live on a
/// CDN); the caller verifies content by hash, not by where it came from.
fn http_get(url: &str, max: u64) -> Result<Vec<u8>, String> {
    if !url_allowed(url) {
        return Err(format!("refusing to fetch {url}: only https is allowed"));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .redirects(5)
        .user_agent("openro-updater")
        .build();
    let resp = agent
        .get(url)
        .set("Accept", "application/vnd.github+json, application/octet-stream")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("{url} answered HTTP {code}"),
            other => format!("could not reach {url}: {other}"),
        })?;
    let mut out = Vec::new();
    resp.into_reader()
        .take(max + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("download failed: {e}"))?;
    if out.len() as u64 > max {
        return Err(format!("download is larger than the {max} byte limit"));
    }
    Ok(out)
}

// --------------------------------------------------------------- updater

impl Updater {
    /// Builds the updater from the environment. `prefix` is the app's name in
    /// capitals; `may_apply` is false when the process is shared (gated).
    pub fn from_env(prefix: &str, bin: &'static str, current: &'static str, default_repo: &str, may_apply: bool) -> Arc<Updater> {
        let var = |k: &str| std::env::var(format!("{prefix}_UPDATE_{k}")).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let cfg = Config {
            bin,
            current,
            repo: var("REPO").unwrap_or_else(|| default_repo.to_string()),
            api: var("API").unwrap_or_else(|| "https://api.github.com".to_string()).trim_end_matches('/').to_string(),
            mode: var("MODE").map(|m| Mode::parse(&m)).unwrap_or(Mode::Notify),
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from(bin)),
            allow_dev: var("DEV").as_deref() == Some("1"),
            may_apply,
        };
        Arc::new(Updater::new(cfg))
    }

    pub fn new(cfg: Config) -> Updater {
        cleanup_leftovers(&cfg.exe);
        Updater { cfg, state: Mutex::new(State::default()) }
    }

    /// Fetches the newest release and records whether it is newer.
    pub fn check_now(&self) -> Value {
        if self.cfg.mode == Mode::Off {
            return self.snapshot();
        }
        let result = self.fetch_latest();
        let mut s = self.state.lock().unwrap();
        s.checked_at = Some(now_secs());
        match result {
            Ok(rel) => {
                s.error = None;
                s.latest = Some(rel);
            }
            Err(e) => s.error = Some(e),
        }
        drop(s);
        self.snapshot()
    }

    fn fetch_latest(&self) -> Result<Release, String> {
        let url = format!("{}/repos/{}/releases/latest", self.cfg.api, self.cfg.repo);
        let body = http_get(&url, MAX_JSON_BYTES)?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| format!("release answer was not JSON: {e}"))?;
        parse_release(&v, self.cfg.bin)
    }

    /// What the page shows.
    pub fn snapshot(&self) -> Value {
        let s = self.state.lock().unwrap();
        let available = s.latest.as_ref().map(|r| is_newer(&r.version, self.cfg.current)).unwrap_or(false);
        json!({
            "name": self.cfg.bin,
            "current": self.cfg.current,
            "mode": self.cfg.mode.label(),
            "repo": self.cfg.repo,
            "platform": target(),
            "checked_at": s.checked_at,
            "available": available,
            "latest": s.latest.as_ref().map(|r| r.version.clone()),
            "notes": s.latest.as_ref().filter(|_| available).map(|r| r.notes.clone()),
            "release_url": s.latest.as_ref().filter(|_| available).map(|r| r.url.clone()),
            "error": s.error,
            "staged": s.applied,
            "can_apply": self.cfg.may_apply && self.cfg.mode != Mode::Off && available && s.applied.is_none() && !self.is_dev_build(),
            "dev_build": self.is_dev_build(),
        })
    }

    fn is_dev_build(&self) -> bool {
        !self.cfg.allow_dev && self.cfg.exe.components().any(|c| c.as_os_str() == "target")
    }

    /// Downloads, verifies and installs the newest release. Returns the new
    /// version; the running process keeps running the old build until restart.
    pub fn apply(&self) -> Result<String, String> {
        if !self.cfg.may_apply {
            return Err("this installation is shared, so only its operator can update it".to_string());
        }
        if self.cfg.mode == Mode::Off {
            return Err("updates are turned off".to_string());
        }
        if self.is_dev_build() {
            return Err("this is a cargo-built binary; update it with git pull and cargo build".to_string());
        }
        if self.state.lock().unwrap().latest.is_none() {
            self.check_now();
        }
        let rel = {
            let s = self.state.lock().unwrap();
            match &s.latest {
                Some(r) if is_newer(&r.version, self.cfg.current) => r.clone(),
                Some(_) => return Err("already on the newest version".to_string()),
                None => return Err(s.error.clone().unwrap_or_else(|| "could not look up the newest version".to_string())),
            }
        };
        if self.state.lock().unwrap().applied.as_deref() == Some(rel.version.as_str()) {
            return Ok(rel.version);
        }
        install(&self.cfg, &rel)?;
        self.state.lock().unwrap().applied = Some(rel.version.clone());
        Ok(rel.version)
    }

    /// Starts the new build in place of this process. Does not return on success.
    pub fn restart(&self) -> Result<(), String> {
        if !self.cfg.may_apply {
            return Err("this installation is shared, so only its operator can restart it".to_string());
        }
        if self.state.lock().unwrap().applied.is_none() {
            return Err("no update is waiting for a restart".to_string());
        }
        let args: Vec<String> = std::env::args().skip(1).collect();
        relaunch(&self.cfg.exe, &args)
    }

    /// Checks soon after start and then every few hours. In `auto` mode a
    /// newer release is also installed (a restart is still the user's call).
    pub fn start_background(self: &Arc<Self>) {
        if self.cfg.mode == Mode::Off {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                let snap = me.check_now();
                if me.cfg.mode == Mode::Auto && snap["available"] == json!(true) {
                    if let Err(e) = me.apply() {
                        me.state.lock().unwrap().error = Some(e);
                    }
                }
                std::thread::sleep(CHECK_EVERY);
            }
        });
    }
}

/// Download, verify, prove and swap. Any failure leaves the installed file alone.
fn install(cfg: &Config, rel: &Release) -> Result<(), String> {
    if let Some(n) = rel.asset_size {
        if n > MAX_ASSET_BYTES {
            return Err(format!("the release file is {n} bytes, over the {MAX_ASSET_BYTES} byte limit"));
        }
    }
    let expected = match (&rel.digest, &rel.sums_url) {
        (_, Some(u)) => {
            let sums = String::from_utf8_lossy(&http_get(u, MAX_JSON_BYTES)?).to_string();
            let h = hash_from_sums(&sums, &rel.asset_name).ok_or_else(|| format!("SHA256SUMS has no entry for {}", rel.asset_name))?;
            if let Some(d) = &rel.digest {
                if *d != h {
                    return Err("the release's two checksums disagree".to_string());
                }
            }
            h
        }
        (Some(d), None) => d.clone(),
        (None, None) => return Err("the release has no checksum, so it will not be installed".to_string()),
    };
    let bytes = http_get(&rel.asset_url, MAX_ASSET_BYTES)?;
    if let Some(n) = rel.asset_size {
        if bytes.len() as u64 != n {
            return Err(format!("downloaded {} bytes, the release says {n}", bytes.len()));
        }
    }
    let got = sha256_hex(&bytes);
    if got != expected {
        return Err("the download does not match its checksum, so it was discarded".to_string());
    }
    let dir = cfg.exe.parent().ok_or("cannot find the install folder")?;
    let staged = dir.join(format!("{}.new", file_name(&cfg.exe)));
    std::fs::write(&staged, &bytes).map_err(|e| format!("cannot write {}: {e}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg.exe).map(|m| m.permissions().mode()).unwrap_or(0o755) | 0o755;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode & 0o7777));
    }
    if let Err(e) = prove_version(&staged, &rel.version) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    if let Err(e) = swap(&cfg.exe, &staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    Ok(())
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "app".to_string())
}

/// Runs the downloaded file with `--version` and requires the expected version
/// in its output, so a wrong or damaged build is never swapped in.
fn prove_version(staged: &Path, version: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(staged)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("the downloaded file does not run on this machine: {e}"))?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(10) => {
                let _ = child.kill();
                return Err("the downloaded file did not answer --version".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("could not check the downloaded file: {e}")),
        }
    }
    let mut out = String::new();
    if let Some(so) = child.stdout.take() {
        let _ = so.take(4096).read_to_string(&mut out);
    }
    if out.split_whitespace().any(|w| w.trim_start_matches('v') == version) {
        Ok(())
    } else {
        Err(format!("the downloaded file reports '{}' instead of version {version}", out.trim()))
    }
}

#[cfg(unix)]
fn swap(exe: &Path, staged: &Path) -> Result<(), String> {
    std::fs::rename(staged, exe).map_err(|e| format!("cannot replace {}: {e}", exe.display()))
}

#[cfg(windows)]
fn swap(exe: &Path, staged: &Path) -> Result<(), String> {
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("cannot move {} aside: {e}", exe.display()))?;
    if let Err(e) = std::fs::rename(staged, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("cannot put the new build in place: {e}"));
    }
    Ok(())
}

/// Removes files a previous update left next to the executable.
fn cleanup_leftovers(exe: &Path) {
    let _ = std::fs::remove_file(exe.with_extension("old"));
    if let Some(dir) = exe.parent() {
        let _ = std::fs::remove_file(dir.join(format!("{}.new", file_name(exe))));
    }
}

#[cfg(unix)]
fn relaunch(exe: &Path, args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(exe).args(args).exec();
    Err(format!("could not start the new build: {err}"))
}

#[cfg(windows)]
fn relaunch(exe: &Path, args: &[String]) -> Result<(), String> {
    std::process::Command::new(exe).args(args).spawn().map_err(|e| format!("could not start the new build: {e}"))?;
    std::process::exit(0);
}

// ----------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn versions_compare_and_odd_tags_are_ignored() {
        assert!(is_newer("v0.2.1", "0.2.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.3.0-rc1", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
        assert!(parse_version("1.2").is_none() && parse_version("1.2.3.4").is_none());
    }

    #[test]
    fn urls_must_be_https_or_loopback_http() {
        assert!(url_allowed("https://api.github.com/x"));
        assert!(url_allowed("http://127.0.0.1:9000/x"));
        assert!(url_allowed("http://localhost/x"));
        assert!(!url_allowed("http://example.com/x"));
        assert!(!url_allowed("ftp://x"));
        assert!(!url_allowed("file:///etc/passwd"));
        assert!(!url_allowed("https://"));
    }

    #[test]
    fn sums_parse_and_hashes_are_sha256() {
        let sums = format!("{}  other\n{}  *openro-rs-x\n", "a".repeat(64), "B".repeat(64));
        assert_eq!(hash_from_sums(&sums, "openro-rs-x").unwrap(), "b".repeat(64));
        assert!(hash_from_sums(&sums, "missing").is_none());
        assert!(hash_from_sums("short  openro-rs-x", "openro-rs-x").is_none());
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn release_json_needs_a_platform_asset_and_ignores_prereleases() {
        let name = asset_name("openro-rs");
        let ok = json!({"tag_name": "v0.3.0", "body": "notes", "html_url": "u", "assets": [
            {"name": name, "browser_download_url": "https://x/a", "size": 3, "digest": "sha256:ABCDEF"},
            {"name": "SHA256SUMS", "browser_download_url": "https://x/s"}]});
        let r = parse_release(&ok, "openro-rs").unwrap();
        assert_eq!((r.version.as_str(), r.asset_size, r.digest.as_deref()), ("0.3.0", Some(3), Some("abcdef")));
        assert!(r.sums_url.is_some());
        assert!(parse_release(&json!({"tag_name": "v0.3.0", "prerelease": true, "assets": []}), "openro-rs").is_err());
        assert!(parse_release(&json!({"tag_name": "v0.3.0", "assets": [{"name": "other"}]}), "openro-rs").unwrap_err().contains("no build for this platform"));
        assert!(parse_release(&json!({"tag_name": "weekly", "assets": []}), "openro-rs").is_err());
    }

    // ---- a mock GitHub: releases/latest, SHA256SUMS and the binary asset

    struct Mock {
        port: u16,
    }

    fn mock(version: &str, asset: Vec<u8>, sums_hash: Option<String>, declared_size: Option<usize>) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let name = asset_name("openro-rs");
        let sums = sums_hash.map(|h| format!("{h}  {name}\n"));
        let version = version.to_string();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { continue };
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let (ctype, body): (&str, Vec<u8>) = if path.ends_with("/releases/latest") {
                    let mut assets = vec![json!({"name": name, "browser_download_url": format!("http://127.0.0.1:{port}/dl/asset"), "size": declared_size.unwrap_or(asset.len())})];
                    if sums.is_some() {
                        assets.push(json!({"name": "SHA256SUMS", "browser_download_url": format!("http://127.0.0.1:{port}/dl/sums")}));
                    }
                    ("application/json", json!({"tag_name": format!("v{version}"), "body": "What is new", "html_url": "http://x/r", "assets": assets}).to_string().into_bytes())
                } else if path == "/dl/asset" {
                    ("application/octet-stream", asset.clone())
                } else if path == "/dl/sums" {
                    ("text/plain", sums.clone().unwrap_or_default().into_bytes())
                } else {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        Mock { port }
    }

    fn fake_exe(dir: &Path, version: &str) -> PathBuf {
        let p = dir.join("openro-rs");
        std::fs::write(&p, format!("#!/bin/sh\necho 'openro-rs {version}'\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    fn updater(m: &Mock, exe: &Path, mode: Mode, may_apply: bool) -> Updater {
        Updater::new(Config {
            bin: "openro-rs",
            current: "0.2.0",
            repo: "o/r".to_string(),
            api: format!("http://127.0.0.1:{}", m.port),
            mode,
            exe: exe.to_path_buf(),
            allow_dev: false,
            may_apply,
        })
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("openro-rs-upd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn a_newer_release_is_reported_downloaded_verified_and_swapped_in() {
        let dir = tmp("ok");
        let exe = fake_exe(&dir, "0.2.0");
        let new = b"#!/bin/sh\necho 'openro-rs 0.3.0'\n".to_vec();
        let m = mock("0.3.0", new.clone(), Some(sha256_hex(&new)), None);
        let u = updater(&m, &exe, Mode::Notify, true);
        let snap = u.check_now();
        assert_eq!((snap["available"].clone(), snap["latest"].clone(), snap["current"].clone()), (json!(true), json!("0.3.0"), json!("0.2.0")));
        assert_eq!(snap["notes"], json!("What is new"));
        assert_eq!(u.apply().unwrap(), "0.3.0");
        assert_eq!(std::fs::read(&exe).unwrap(), new, "the installed file is now the release");
        assert!(!dir.join("openro-rs.new").exists(), "no staging file is left behind");
        assert_eq!(u.snapshot()["staged"], json!("0.3.0"));
        assert_eq!(u.snapshot()["can_apply"], json!(false), "already staged");
    }

    #[cfg(unix)]
    #[test]
    fn a_bad_checksum_leaves_the_installed_file_alone() {
        let dir = tmp("badsum");
        let exe = fake_exe(&dir, "0.2.0");
        let before = std::fs::read(&exe).unwrap();
        let new = b"#!/bin/sh\necho 'openro-rs 0.3.0'\n".to_vec();
        let m = mock("0.3.0", new, Some(sha256_hex(b"something else")), None);
        let u = updater(&m, &exe, Mode::Notify, true);
        u.check_now();
        let e = u.apply().unwrap_err();
        assert!(e.contains("checksum"), "{e}");
        assert_eq!(std::fs::read(&exe).unwrap(), before);
        assert!(!dir.join("openro-rs.new").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_build_that_reports_the_wrong_version_is_not_installed() {
        let dir = tmp("wrongver");
        let exe = fake_exe(&dir, "0.2.0");
        let before = std::fs::read(&exe).unwrap();
        let new = b"#!/bin/sh\necho 'openro-rs 9.9.9'\n".to_vec();
        let m = mock("0.3.0", new.clone(), Some(sha256_hex(&new)), None);
        let u = updater(&m, &exe, Mode::Notify, true);
        u.check_now();
        let e = u.apply().unwrap_err();
        assert!(e.contains("instead of version 0.3.0"), "{e}");
        assert_eq!(std::fs::read(&exe).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn a_release_without_checksums_a_size_mismatch_and_old_releases_are_refused() {
        let dir = tmp("refuse");
        let exe = fake_exe(&dir, "0.2.0");
        let new = b"#!/bin/sh\necho 'openro-rs 0.3.0'\n".to_vec();
        let no_sums = mock("0.3.0", new.clone(), None, None);
        let u = updater(&no_sums, &exe, Mode::Notify, true);
        u.check_now();
        assert!(u.apply().unwrap_err().contains("no checksum"));
        let wrong_size = mock("0.3.0", new.clone(), Some(sha256_hex(&new)), Some(new.len() + 5));
        let u = updater(&wrong_size, &exe, Mode::Notify, true);
        u.check_now();
        assert!(u.apply().unwrap_err().contains("downloaded"));
        let same = mock("0.2.0", new.clone(), Some(sha256_hex(&new)), None);
        let u = updater(&same, &exe, Mode::Notify, true);
        assert_eq!(u.check_now()["available"], json!(false));
        assert!(u.apply().unwrap_err().contains("newest"));
    }

    #[test]
    fn off_mode_never_calls_out_and_shared_or_dev_installs_never_apply() {
        let dir = tmp("modes");
        let exe = fake_exe(&dir, "0.2.0");
        // No server at all: any network call would error, and off mode records nothing.
        let off = Updater::new(Config { bin: "openro-rs", current: "0.2.0", repo: "o/r".into(), api: "http://127.0.0.1:1".into(), mode: Mode::Off, exe: exe.clone(), allow_dev: false, may_apply: true });
        let snap = off.check_now();
        assert_eq!((snap["checked_at"].clone(), snap["error"].clone()), (Value::Null, Value::Null));
        assert!(off.apply().unwrap_err().contains("turned off"));
        let shared = Updater::new(Config { may_apply: false, mode: Mode::Notify, ..off.cfg.clone() });
        assert!(shared.apply().unwrap_err().contains("operator"));
        assert!(shared.restart().unwrap_err().contains("operator"));
        let dev = Updater::new(Config { exe: dir.join("target").join("debug").join("openro-rs"), mode: Mode::Notify, ..off.cfg.clone() });
        assert_eq!(dev.snapshot()["dev_build"], json!(true));
        assert!(dev.apply().unwrap_err().contains("cargo-built"));
    }

    #[test]
    fn an_unreachable_or_insecure_api_reports_an_error_and_changes_nothing() {
        let dir = tmp("neterr");
        let exe = fake_exe(&dir, "0.2.0");
        let down = Updater::new(Config { bin: "openro-rs", current: "0.2.0", repo: "o/r".into(), api: "http://127.0.0.1:1".into(), mode: Mode::Notify, exe: exe.clone(), allow_dev: false, may_apply: true });
        let snap = down.check_now();
        assert!(snap["error"].as_str().unwrap().contains("could not reach"), "{snap}");
        assert_eq!(snap["available"], json!(false));
        let insecure = Updater::new(Config { api: "http://example.com".into(), ..down.cfg.clone() });
        assert!(insecure.check_now()["error"].as_str().unwrap().contains("only https"));
    }
}
