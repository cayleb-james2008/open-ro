//! Built-In Developer Skill & Tool Connection Library for OpenRO v0.9.0.
//!
//! Ships with 3 pre-installed core workflow packs so every OpenRO agent starts
//! fluent in Roblox Luau/networking/DataStores/Studio MCP, Blender 3D UV/mesh
//! generation, and Anime Fan-Art & Creative Homage direction — plus 5 one-click
//! installable packs that dynamically unlock additional agent tools, `SKILL.md`
//! prompt guidance, and `0600`-persisted provider connections:
//!
//! Pre-installed defaults (always active):
//! - `roblox-studio-core`
//! - `blender-3d-pipeline` (auto-detects Blender 5.x on Linux/Windows + headless `.obj` generation)
//! - `anime-fan-art-knowledge`
//!
//! One-click installable packs:
//! - `image-gen-studio` (Gemini Nano Banana / OpenAI Image)
//! - `model-3d-gen` (Local Blender / procedural `.obj` + Meshy / Tripo)
//! - `video-gen-studio` (OpenMotion / Veo / Flow)
//! - `audio-sfx-gen` (Built-in pure-Rust `.wav` synth + ElevenLabs / Lyria)
//! - `rojo-open-cloud` (Rojo 7 + Roblox Open Cloud publishing).

use crate::art_knowledge;
use crate::audio_synth;
use crate::studio_embed::detect_studio_environment;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Canonical Blender 3D prop presets supported by `POST /api/tools/blender/generate`.
pub const BLENDER_PRESETS: &[&str] = &[
    "torii_gate",
    "card_pedestal",
    "colosseum_arch",
    "cauldron",
    "crystal_obelisk",
];

/// Error returned by `LibraryManager` carrying an HTTP-friendly status (`400` / `404` / `500`).
#[derive(Clone, Debug)]
pub struct LibraryError {
    pub status: u16,
    pub message: String,
}

impl LibraryError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        LibraryError {
            status: 400,
            message: msg.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        LibraryError {
            status: 404,
            message: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        LibraryError {
            status: 500,
            message: msg.into(),
        }
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Static definition of a built-in or installable OpenRO Library pack.
#[derive(Clone, Debug)]
pub struct PackSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub default_installed: bool,
    pub version: &'static str,
    pub default_provider: &'static str,
    pub tools: &'static [&'static str],
    pub capabilities: &'static [&'static str],
    pub env_vars: &'static [&'static str],
    pub skill_summary: &'static str,
    pub skill_markdown: &'static str,
}

/// User-configured settings for one library pack persisted atomically at `0600`.
#[derive(Clone, Debug, Default)]
pub struct PackUserConfig {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub binary_path: Option<String>,
    pub api_key: Option<String>,
}

/// Detected Blender installation metadata on the host machine.
#[derive(Clone, Debug, Default)]
pub struct BlenderDetection {
    pub detected: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// Complete catalog of all 8 OpenRO Library packs (3 pre-installed defaults + 5 installable).
pub fn all_pack_specs() -> &'static [PackSpec] {
    &PACK_SPECS
}

pub fn find_pack_spec(id: &str) -> Option<&'static PackSpec> {
    let trimmed = id.trim();
    PACK_SPECS.iter().find(|p| p.id == trimmed)
}

/// Mask a secret API key so raw credentials are NEVER echoed in HTTP JSON responses.
pub fn mask_secret(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

/// Pure Windows filesystem scanner for `%ProgramFiles%\Blender Foundation\Blender*\blender.exe`.
/// Unit-testable on both Linux and Windows.
pub fn find_windows_blender_exe(program_files: &Path) -> Option<PathBuf> {
    let bf_dir = program_files.join("Blender Foundation");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&bf_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let exe = p.join("blender.exe");
            if exe.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                found.push((name, exe));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().next().map(|(_, p)| p)
}

/// Passively detect a local Blender executable and read its version string.
pub fn detect_blender(override_path: Option<&str>) -> BlenderDetection {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(ov) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        candidates.push(PathBuf::from(ov));
    }

    #[cfg(unix)]
    {
        if let Some(p) = find_on_path("blender") {
            candidates.push(p);
        }
        for std_path in ["/usr/bin/blender", "/usr/local/bin/blender", "/snap/bin/blender"] {
            let p = PathBuf::from(std_path);
            if p.is_file() {
                candidates.push(p);
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(p) = find_on_path("blender.exe") {
            candidates.push(p);
        }
        if let Some(pf) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            if let Some(p) = find_windows_blender_exe(&pf) {
                candidates.push(p);
            }
        }
    }

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let version = Command::new(&candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .map(|s| s.trim().to_string())
            });
        return BlenderDetection {
            detected: true,
            path: Some(candidate.to_string_lossy().into_owned()),
            version,
        };
    }

    BlenderDetection::default()
}

/// Manages installed packs, `0600` provider configs, unlocked agent tools, and active `SKILL.md` prompts.
#[derive(Clone, Debug)]
pub struct LibraryManager {
    config_path: PathBuf,
    installed_extra: BTreeSet<String>,
    configs: BTreeMap<String, PackUserConfig>,
}

impl LibraryManager {
    /// Load (or initialize default) library state under `<data_dir>/library.json`.
    pub fn open(data_dir: &Path) -> Self {
        let config_path = data_dir.join("library.json");
        let mut mgr = LibraryManager {
            config_path,
            installed_extra: BTreeSet::new(),
            configs: BTreeMap::new(),
        };
        mgr.load();
        mgr
    }

    fn load(&mut self) {
        let Ok(raw) = std::fs::read_to_string(&self.config_path) else {
            return;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            return;
        };
        if let Some(arr) = val.get("installed").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(id) = item.as_str() {
                    if let Some(spec) = find_pack_spec(id) {
                        if !spec.default_installed {
                            self.installed_extra.insert(spec.id.to_string());
                        }
                    }
                }
            }
        }
        if let Some(map) = val.get("configs").and_then(|v| v.as_object()) {
            for (id, entry) in map {
                if find_pack_spec(id).is_none() {
                    continue;
                }
                let s = |k: &str| {
                    entry
                        .get(k)
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(ToString::to_string)
                };
                self.configs.insert(
                    id.clone(),
                    PackUserConfig {
                        provider: s("provider"),
                        base_url: s("base_url"),
                        binary_path: s("binary_path"),
                        api_key: s("api_key"),
                    },
                );
            }
        }
    }

    fn save(&self) -> Result<(), LibraryError> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| LibraryError::internal(format!("create library dir: {e}")))?;
        }
        let mut cfg_obj = serde_json::Map::new();
        for (id, cfg) in &self.configs {
            cfg_obj.insert(
                id.clone(),
                json!({
                    "provider": cfg.provider,
                    "base_url": cfg.base_url,
                    "binary_path": cfg.binary_path,
                    "api_key": cfg.api_key,
                }),
            );
        }
        let payload = json!({
            "version": 1,
            "installed": self.installed_extra.iter().cloned().collect::<Vec<_>>(),
            "configs": Value::Object(cfg_obj),
        });
        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| LibraryError::internal(format!("serialize library.json: {e}")))?;

        let tmp = self.config_path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .map_err(|e| LibraryError::internal(format!("write library.json.tmp: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.config_path)
            .map_err(|e| LibraryError::internal(format!("commit library.json: {e}")))?;
        Ok(())
    }

    pub fn is_installed(&self, id: &str) -> bool {
        match find_pack_spec(id) {
            Some(spec) => spec.default_installed || self.installed_extra.contains(spec.id),
            None => false,
        }
    }

    /// Return all unique tool names currently unlocked by installed packs.
    pub fn active_tools(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for spec in PACK_SPECS.iter() {
            if self.is_installed(spec.id) {
                for tool in spec.tools {
                    set.insert((*tool).to_string());
                }
            }
        }
        set.into_iter().collect()
    }

    /// Compose the live developer skill system prompt from all currently installed packs.
    pub fn active_skill_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str("## Active OpenRO Developer Skills & Tool Packs\n");
        for spec in PACK_SPECS.iter() {
            if !self.is_installed(spec.id) {
                continue;
            }
            out.push_str(&format!(
                "\n### {} (`{}`) — Tools: {}\n",
                spec.name,
                spec.id,
                spec.tools.join(", ")
            ));
            out.push_str(spec.skill_markdown);
            out.push('\n');
        }
        out
    }

    fn blender_detection(&self) -> BlenderDetection {
        let ov = self
            .configs
            .get("blender-3d-pipeline")
            .and_then(|c| c.binary_path.as_deref())
            .or_else(|| {
                self.configs
                    .get("model-3d-gen")
                    .and_then(|c| c.binary_path.as_deref())
            });
        detect_blender(ov)
    }

    fn pack_json(&self, spec: &PackSpec, blender: &BlenderDetection) -> Value {
        let installed = self.is_installed(spec.id);
        let user_cfg = self.configs.get(spec.id);
        let provider = user_cfg
            .and_then(|c| c.provider.as_deref())
            .unwrap_or(spec.default_provider);
        let base_url = user_cfg.and_then(|c| c.base_url.as_deref());
        let saved_key = user_cfg.and_then(|c| c.api_key.as_deref());
        let env_key_present = spec
            .env_vars
            .iter()
            .any(|k| std::env::var(k).ok().is_some_and(|v| !v.trim().is_empty()));
        let masked_key = saved_key.map(mask_secret).filter(|s| !s.is_empty());

        let detected_binary: Option<String> = match spec.id {
            "blender-3d-pipeline" | "model-3d-gen" => blender.path.clone(),
            "roblox-studio-core" => detect_studio_environment().mcp_command,
            "audio-sfx-gen" => Some("built-in:openro-audio-synth".to_string()),
            "rojo-open-cloud" => find_on_path("rojo")
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| Some("built-in:rojo-ship-pack".to_string())),
            _ => user_cfg.and_then(|c| c.binary_path.clone()),
        };

        let configured = match spec.id {
            "roblox-studio-core" | "anime-fan-art-knowledge" | "audio-sfx-gen" => true,
            "blender-3d-pipeline" | "model-3d-gen" => {
                blender.detected || saved_key.is_some() || env_key_present
            }
            "video-gen-studio" => {
                provider == "openmotion-local" || saved_key.is_some() || env_key_present
            }
            "rojo-open-cloud" => {
                provider == "rojo-local" || saved_key.is_some() || env_key_present
            }
            _ => saved_key.is_some() || env_key_present,
        };

        json!({
            "id": spec.id,
            "name": spec.name,
            "category": spec.category,
            "description": spec.description,
            "default_installed": spec.default_installed,
            "installed": installed,
            "configured": configured,
            "version": spec.version,
            "tools": spec.tools,
            "capabilities": spec.capabilities,
            "env_vars": spec.env_vars,
            "provider": provider,
            "base_url": base_url,
            "masked_key": masked_key,
            "detected_binary": detected_binary,
            "skill_summary": spec.skill_summary,
        })
    }

    /// Full `GET /api/library` response matching the exact `#332` wire contract.
    pub fn list_json(&self) -> Value {
        let blender = self.blender_detection();
        let packs: Vec<Value> = PACK_SPECS
            .iter()
            .map(|spec| self.pack_json(spec, &blender))
            .collect();
        let active_tools = self.active_tools();
        let prompt_skills_count = PACK_SPECS
            .iter()
            .filter(|s| self.is_installed(s.id))
            .count();

        json!({
            "ok": true,
            "packs": packs,
            "active_tools": active_tools,
            "blender_detected": blender.detected,
            "blender_path": blender.path,
            "blender_version": blender.version,
            "prompt_skills_count": prompt_skills_count,
        })
    }

    /// Install an optional pack by ID (`POST /api/library/install`).
    pub fn install(&mut self, id: &str) -> Result<Value, LibraryError> {
        let Some(spec) = find_pack_spec(id) else {
            return Err(LibraryError::not_found(format!(
                "unknown library pack '{id}'"
            )));
        };
        if !spec.default_installed {
            self.installed_extra.insert(spec.id.to_string());
            self.save()?;
        }
        let blender = self.blender_detection();
        let pack = self.pack_json(spec, &blender);
        let list = self.list_json();
        Ok(json!({
            "ok": true,
            "pack": pack,
            "active_tools": list["active_tools"],
            "packs": list["packs"],
        }))
    }

    /// Uninstall an optional pack by ID (`POST /api/library/uninstall`).
    /// Refuses uninstalling pre-installed default core packs with status `400`.
    pub fn uninstall(&mut self, id: &str) -> Result<Value, LibraryError> {
        let Some(spec) = find_pack_spec(id) else {
            return Err(LibraryError::not_found(format!(
                "unknown library pack '{id}'"
            )));
        };
        if spec.default_installed {
            return Err(LibraryError::bad_request(format!(
                "default core pack '{}' cannot be uninstalled",
                spec.id
            )));
        }
        self.installed_extra.remove(spec.id);
        self.save()?;
        let list = self.list_json();
        Ok(json!({
            "ok": true,
            "id": spec.id,
            "active_tools": list["active_tools"],
            "packs": list["packs"],
        }))
    }

    /// Persist provider settings / credentials (`0600`) for one pack (`POST /api/library/configure`).
    /// Never echoes raw `api_key` back in the response.
    pub fn configure(&mut self, id: &str, patch: &Value) -> Result<Value, LibraryError> {
        let Some(spec) = find_pack_spec(id) else {
            return Err(LibraryError::not_found(format!(
                "unknown library pack '{id}'"
            )));
        };
        let entry = self.configs.entry(spec.id.to_string()).or_default();

        if let Some(prov) = patch.get("provider").and_then(|v| v.as_str()) {
            let trimmed = prov.trim();
            if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.len() > 64 {
                return Err(LibraryError::bad_request("invalid provider identifier"));
            }
            if trimmed.is_empty() {
                entry.provider = None;
            } else {
                entry.provider = Some(trimmed.to_string());
            }
        }
        if let Some(url) = patch.get("base_url").and_then(|v| v.as_str()) {
            let trimmed = url.trim();
            if trimmed.is_empty() {
                entry.base_url = None;
            } else if !(trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                || trimmed.contains('\0')
                || trimmed.contains('\n')
            {
                return Err(LibraryError::bad_request(
                    "base_url must be a valid http:// or https:// URL",
                ));
            } else {
                entry.base_url = Some(trimmed.to_string());
            }
        }
        if let Some(bin) = patch.get("binary_path").and_then(|v| v.as_str()) {
            let trimmed = bin.trim();
            if trimmed.contains('\0') || trimmed.contains('\n') {
                return Err(LibraryError::bad_request(
                    "binary_path must not contain control characters",
                ));
            }
            if trimmed.is_empty() {
                entry.binary_path = None;
            } else {
                entry.binary_path = Some(trimmed.to_string());
            }
        }
        if patch
            .get("clear_api_key")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            entry.api_key = None;
        } else if let Some(key) = patch.get("api_key").and_then(|v| v.as_str()) {
            let trimmed = key.trim();
            if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.len() > 512 {
                return Err(LibraryError::bad_request("invalid api_key value"));
            }
            if !trimmed.is_empty() {
                entry.api_key = Some(trimmed.to_string());
            }
        }

        self.save()?;
        let blender = self.blender_detection();
        let pack = self.pack_json(spec, &blender);
        let list = self.list_json();
        Ok(json!({
            "ok": true,
            "pack": pack,
            "packs": list["packs"],
        }))
    }

    /// Run an honest local or credential readiness check for one pack (`POST /api/library/test`).
    pub fn test_pack(&self, id: &str) -> Result<Value, LibraryError> {
        let Some(spec) = find_pack_spec(id) else {
            return Err(LibraryError::not_found(format!(
                "unknown library pack '{id}'"
            )));
        };
        let t0 = Instant::now();
        let user_cfg = self.configs.get(spec.id);
        let has_key = user_cfg
            .and_then(|c| c.api_key.as_deref())
            .is_some_and(|k| !k.is_empty())
            || spec
                .env_vars
                .iter()
                .any(|k| std::env::var(k).ok().is_some_and(|v| !v.trim().is_empty()));

        let (passed, detail) = match spec.id {
            "roblox-studio-core" => {
                let det = detect_studio_environment();
                (
                    true,
                    format!(
                        "Core Roblox Luau, Networking, DataStore & Studio MCP skills loaded. {}",
                        det.guidance
                    ),
                )
            }
            "blender-3d-pipeline" | "model-3d-gen" => {
                let b = self.blender_detection();
                if b.detected {
                    (
                        true,
                        format!(
                            "Blender ready at {} ({})",
                            b.path.as_deref().unwrap_or("blender"),
                            b.version.as_deref().unwrap_or("detected")
                        ),
                    )
                } else {
                    (
                        true,
                        "Blender binary not found on PATH; built-in procedural .obj mesh generator is ready as fallback."
                            .to_string(),
                    )
                }
            }
            "anime-fan-art-knowledge" => {
                (
                    true,
                    format!(
                        "Fan-Art & Creative Homage Knowledge Point loaded ({} policy principles, {} anime archetype guides).",
                        art_knowledge::policy_principles().len(),
                        art_knowledge::anime_archetypes().len()
                    ),
                )
            }
            "audio-sfx-gen" => match audio_synth::synthesize_wav("ui_click", Some(60)) {
                Ok(w) => (
                    true,
                    format!(
                        "Built-in 44.1 kHz PCM16 .wav synthesizer verified ({} bytes test cue){}.",
                        w.wav_bytes.len(),
                        if has_key {
                            " + cloud audio API key configured"
                        } else {
                            ""
                        }
                    ),
                ),
                Err(e) => (false, format!("local audio synthesizer error: {e}")),
            },
            "image-gen-studio" => {
                if has_key {
                    (
                        true,
                        "Image generation provider API key configured (Gemini Nano Banana / OpenAI Image)."
                            .to_string(),
                    )
                } else {
                    (
                        false,
                        "No GEMINI_API_KEY or OPENAI_API_KEY configured yet. Add an API key above or upload local artwork."
                            .to_string(),
                    )
                }
            }
            "video-gen-studio" => {
                let prov = user_cfg
                    .and_then(|c| c.provider.as_deref())
                    .unwrap_or(spec.default_provider);
                if prov == "openmotion-local" {
                    (
                        true,
                        "Local OpenMotion / HyperFrames storyboard & trailer workflow ready."
                            .to_string(),
                    )
                } else if has_key {
                    (
                        true,
                        format!("Video provider '{prov}' API key configured."),
                    )
                } else {
                    (
                        false,
                        format!(
                            "Provider '{prov}' requires an API key (GEMINI_API_KEY / VEO_API_KEY), or switch to 'openmotion-local'."
                        ),
                    )
                }
            }
            "rojo-open-cloud" => {
                if has_key {
                    (
                        true,
                        "Rojo Ship Pack exporter + Roblox Open Cloud API key configured."
                            .to_string(),
                    )
                } else {
                    (
                        true,
                        "Local Rojo 7 Ship Pack (.zip) exporter ready; add ROBLOX_OPEN_CLOUD_API_KEY to enable direct cloud publishing."
                            .to_string(),
                    )
                }
            }
            _ => (false, "unsupported pack".to_string()),
        };

        let latency_ms = t0.elapsed().as_millis() as u64;
        Ok(json!({
            "ok": true,
            "id": spec.id,
            "passed": passed,
            "detail": detail,
            "latency_ms": latency_ms,
        }))
    }
}

// ---------------------------------------------------------------------------
// Headless Blender 5.x + Procedural `.obj` 3D Mesh Generator & Preview
// ---------------------------------------------------------------------------

pub fn mesh_assets_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("game-assets").join("meshes")
}

/// Normalize a Blender prop preset name into one of the 5 canonical `#332` presets.
pub fn normalize_blender_preset(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "torii_gate" | "torii" | "shrine_gate" => Ok("torii_gate"),
        "card_pedestal" | "pedestal" | "card_stand" => Ok("card_pedestal"),
        "colosseum_arch" | "arch" | "arena_arch" => Ok("colosseum_arch"),
        "cauldron" | "potion_cauldron" => Ok("cauldron"),
        "crystal_obelisk" | "obelisk" | "crystal" => Ok("crystal_obelisk"),
        other => Err(format!(
            "unknown Blender preset '{other}' (expected torii_gate, card_pedestal, colosseum_arch, cauldron, or crystal_obelisk)"
        )),
    }
}

/// Strictly validate mesh/preview filenames (`.obj` or `.png` only; no traversal).
pub fn sanitize_mesh_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 96 {
        return Err("mesh filename must be 1..=96 characters".to_string());
    }
    if trimmed.contains('\0')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.starts_with('.')
    {
        return Err(format!(
            "invalid mesh filename '{trimmed}': path separators or traversal are forbidden"
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!(
            "invalid mesh filename '{trimmed}': only ASCII letters, digits, '.', '-', '_' allowed"
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.ends_with(".obj") || lower.ends_with(".png")) {
        return Err(format!(
            "mesh filename '{trimmed}' must end with .obj or .png"
        ));
    }
    Ok(trimmed.to_string())
}

fn slug_for_mesh(raw: Option<&str>, preset: &str) -> String {
    let base = raw.unwrap_or(preset).trim();
    let stripped = base
        .strip_suffix(".obj")
        .or_else(|| base.strip_suffix(".OBJ"))
        .unwrap_or(base);
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let clean = slug.trim_matches('-');
    if clean.is_empty() {
        preset.replace('_', "-")
    } else {
        clean.to_string()
    }
}

/// Generate a 3D `.obj` mesh (using real headless Blender when installed, with clean
/// procedural `.obj` fallback) plus an isometric shaded PNG preview under
/// `<data_dir>/game-assets/meshes/`. Matches the exact `#332` `POST /api/tools/blender/generate` contract.
pub fn generate_blender_mesh(
    data_dir: &Path,
    req: &Value,
    blender_override: Option<&str>,
) -> Result<Value, String> {
    let preset_raw = req
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("torii_gate");
    let preset = normalize_blender_preset(preset_raw)?;
    let render_preview = req
        .get("render_preview")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let force_fallback = req
        .get("use_fallback")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let slug = slug_for_mesh(req.get("name").and_then(|v| v.as_str()), preset);
    let obj_filename = sanitize_mesh_filename(&format!("{slug}.obj"))?;
    let png_filename = sanitize_mesh_filename(&format!("{slug}.png"))?;

    let dir = mesh_assets_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create mesh assets dir: {e}"))?;
    let obj_path = dir.join(&obj_filename);
    let png_path = dir.join(&png_filename);

    let detection = if force_fallback {
        BlenderDetection::default()
    } else {
        detect_blender(blender_override)
    };

    let mut blender_used = false;
    let mut detail = String::new();

    if let Some(blender_exe) = detection.path.as_deref() {
        match run_headless_blender_obj(blender_exe, preset, &obj_path) {
            Ok(()) => {
                blender_used = true;
                detail = format!(
                    "Generated with {} ({}) via headless bpy.ops.wm.obj_export.",
                    detection.version.as_deref().unwrap_or("Blender"),
                    blender_exe
                );
            }
            Err(err) => {
                detail = format!(
                    "Blender invocation fallback ({err}); generated clean procedural .obj mesh."
                );
            }
        }
    }

    if !blender_used {
        let obj_text = build_procedural_obj(preset);
        let tmp = dir.join(format!(".{obj_filename}.tmp"));
        std::fs::write(&tmp, obj_text.as_bytes()).map_err(|e| format!("write obj tmp: {e}"))?;
        std::fs::rename(&tmp, &obj_path).map_err(|e| format!("commit obj file: {e}"))?;
        if detail.is_empty() {
            detail = "Generated with built-in procedural .obj mesh builder.".to_string();
        }
    }

    let obj_bytes = std::fs::read(&obj_path).map_err(|e| format!("read generated obj: {e}"))?;
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    let (verts, tris) = parse_obj_geometry(&obj_str);

    let preview_url = if render_preview {
        let png_bytes = render_obj_preview_png(&verts, &tris, preset);
        let tmp_png = dir.join(format!(".{png_filename}.tmp"));
        std::fs::write(&tmp_png, &png_bytes).map_err(|e| format!("write mesh preview png: {e}"))?;
        std::fs::rename(&tmp_png, &png_path).map_err(|e| format!("commit mesh preview png: {e}"))?;
        Some(format!("/api/assets/meshes/{png_filename}"))
    } else {
        None
    };

    Ok(json!({
        "ok": true,
        "preset": preset,
        "filename": obj_filename,
        "obj_url": format!("/api/assets/meshes/{obj_filename}"),
        "preview_filename": if render_preview { Some(&png_filename) } else { None },
        "preview_url": preview_url,
        "vertices": verts.len(),
        "triangles": tris.len(),
        "bytes": obj_bytes.len(),
        "blender_used": blender_used,
        "blender_version": detection.version,
        "detail": detail,
    }))
}

/// Safely read a `.obj` or `.png` file from `<data_dir>/game-assets/meshes/` with symlink containment.
pub fn read_mesh_asset(
    data_dir: &Path,
    raw_filename: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    let filename = sanitize_mesh_filename(raw_filename)?;
    let dir = mesh_assets_dir(data_dir);
    let canon_dir = std::fs::canonicalize(&dir)
        .map_err(|_| format!("mesh asset '{filename}' not found"))?;
    let candidate = dir.join(&filename);
    let canon_file = std::fs::canonicalize(&candidate)
        .map_err(|_| format!("mesh asset '{filename}' not found"))?;
    if !canon_file.starts_with(&canon_dir) {
        return Err("mesh asset path escapes meshes directory".to_string());
    }
    let bytes = std::fs::read(&canon_file).map_err(|e| format!("read mesh asset: {e}"))?;
    let mime = if filename.to_ascii_lowercase().ends_with(".png") {
        "image/png"
    } else {
        "text/plain; charset=utf-8"
    };
    Ok((bytes, mime))
}

fn run_headless_blender_obj(
    blender_exe: &str,
    preset: &str,
    out_obj: &Path,
) -> Result<(), String> {
    let script_path = out_obj.with_extension("gen.py");
    std::fs::write(&script_path, BLENDER_GEN_SCRIPT)
        .map_err(|e| format!("write blender script: {e}"))?;

    let status = Command::new(blender_exe)
        .args([
            "--background",
            "--factory-startup",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(&script_path)
        .env("OPENRO_BLENDER_PRESET", preset)
        .env("OPENRO_BLENDER_OUT_OBJ", out_obj)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    let _ = std::fs::remove_file(&script_path);

    let out = status.map_err(|e| format!("spawn blender: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("non-zero exit");
        return Err(format!("blender exited with {}: {tail}", out.status));
    }
    if !out_obj.is_file() {
        return Err("blender did not produce expected .obj file".to_string());
    }
    Ok(())
}

/// Self-contained Blender 4.x/5.x headless Python generator reading `OPENRO_BLENDER_PRESET`
/// and `OPENRO_BLENDER_OUT_OBJ` from the environment (zero string interpolation).
const BLENDER_GEN_SCRIPT: &str = r#"
import math, os, sys
import bpy

preset = os.environ.get("OPENRO_BLENDER_PRESET", "torii_gate")
out_obj = os.environ["OPENRO_BLENDER_OUT_OBJ"]

bpy.ops.wm.read_factory_settings(use_empty=True)

def add_box(name, loc, scale):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=loc)
    o = bpy.context.active_object
    o.name = name
    o.scale = scale

def add_cyl(name, loc, radius, depth, verts=16):
    bpy.ops.mesh.primitive_cylinder_add(vertices=verts, radius=radius, depth=depth, location=loc)
    o = bpy.context.active_object
    o.name = name

def add_cone(name, loc, r1, r2, depth, verts=8):
    bpy.ops.mesh.primitive_cone_add(vertices=verts, radius1=r1, radius2=r2, depth=depth, location=loc)
    o = bpy.context.active_object
    o.name = name

if preset == "torii_gate":
    add_cyl("LeftPillar", (-3.2, 0.0, 3.6), 0.42, 7.2, 16)
    add_cyl("RightPillar", (3.2, 0.0, 3.6), 0.42, 7.2, 16)
    add_box("LeftBase", (-3.2, 0.0, 0.45), (1.2, 1.2, 0.9))
    add_box("RightBase", (3.2, 0.0, 0.45), (1.2, 1.2, 0.9))
    add_box("NukiBeam", (0.0, 0.0, 5.6), (8.4, 0.45, 0.55))
    add_box("KasagiLintels", (0.0, 0.0, 7.2), (10.2, 0.95, 0.72))
    add_box("GakuzukaPlaque", (0.0, 0.0, 6.4), (1.3, 0.52, 1.15))
elif preset == "card_pedestal":
    add_cyl("PlinthBase", (0.0, 0.0, 0.35), 2.4, 0.7, 8)
    add_cyl("PlinthMid", (0.0, 0.0, 1.05), 1.7, 0.7, 8)
    add_box("CardFrame", (0.0, 0.0, 3.4), (2.8, 0.32, 3.9))
    add_box("FrameTop", (0.0, 0.0, 5.45), (3.1, 0.44, 0.25))
    add_box("FrameBottom", (0.0, 0.0, 1.45), (3.1, 0.44, 0.25))
elif preset == "colosseum_arch":
    add_box("LeftPier", (-3.0, 0.0, 2.8), (1.4, 1.6, 5.6))
    add_box("RightPier", (3.0, 0.0, 2.8), (1.4, 1.6, 5.6))
    for idx in range(7):
        ang = math.pi * idx / 6.0
        x = math.cos(ang) * 3.0
        z = 5.4 + math.sin(ang) * 2.4
        add_box(f"ArchStone_{idx}", (x, 0.0, z), (1.25, 1.5, 0.95))
    add_box("Cornice", (0.0, 0.0, 8.2), (8.2, 1.9, 0.65))
elif preset == "cauldron":
    bpy.ops.mesh.primitive_uv_sphere_add(segments=16, ring_count=10, radius=2.0, location=(0.0, 0.0, 2.0))
    add_cyl("Rim", (0.0, 0.0, 3.55), 1.65, 0.45, 16)
    for idx in range(3):
        ang = 2.0 * math.pi * idx / 3.0
        add_cyl(f"Leg_{idx}", (math.cos(ang) * 1.25, math.sin(ang) * 1.25, 0.45), 0.28, 0.9, 10)
else:  # crystal_obelisk
    add_cyl("StoneBase", (0.0, 0.0, 0.5), 2.1, 1.0, 6)
    add_cone("CrystalShaft", (0.0, 0.0, 3.6), 1.35, 0.85, 5.2, 6)
    add_cone("CrystalTip", (0.0, 0.0, 7.2), 0.85, 0.0, 2.0, 6)

bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
bpy.ops.wm.obj_export(
    filepath=out_obj,
    export_triangulated_mesh=True,
    export_uv=True,
    export_normals=True,
    export_materials=False,
    forward_axis='NEGATIVE_Z',
    up_axis='Y',
)
"#;

// ---------------------------------------------------------------------------
// Pure-Rust Procedural Wavefront `.obj` Fallback + Isometric PNG Preview
// ---------------------------------------------------------------------------

fn append_obj_box(out: &mut String, v_offset: &mut usize, center: [f64; 3], size: [f64; 3]) {
    let [cx, cy, cz] = center;
    let hx = size[0] * 0.5;
    let hy = size[1] * 0.5;
    let hz = size[2] * 0.5;
    let corners = [
        [cx - hx, cy - hy, cz - hz],
        [cx + hx, cy - hy, cz - hz],
        [cx + hx, cy + hy, cz - hz],
        [cx - hx, cy + hy, cz - hz],
        [cx - hx, cy - hy, cz + hz],
        [cx + hx, cy - hy, cz + hz],
        [cx + hx, cy + hy, cz + hz],
        [cx - hx, cy + hy, cz + hz],
    ];
    for c in corners {
        out.push_str(&format!("v {:.4} {:.4} {:.4}\n", c[0], c[1], c[2]));
    }
    let b = *v_offset;
    let tris = [
        (0, 2, 1),
        (0, 3, 2),
        (4, 5, 6),
        (4, 6, 7),
        (0, 1, 5),
        (0, 5, 4),
        (2, 3, 7),
        (2, 7, 6),
        (1, 2, 6),
        (1, 6, 5),
        (3, 0, 4),
        (3, 4, 7),
    ];
    for (i0, i1, i2) in tris {
        out.push_str(&format!("f {} {} {}\n", b + i0, b + i1, b + i2));
    }
    *v_offset += 8;
}

fn build_procedural_obj(preset: &str) -> String {
    let mut out = format!("# OpenRO Procedural Mesh Preset: {preset}\no {preset}\n");
    let mut v = 1usize;
    match preset {
        "torii_gate" => {
            append_obj_box(&mut out, &mut v, [-3.2, 3.6, 0.0], [0.85, 7.2, 0.85]);
            append_obj_box(&mut out, &mut v, [3.2, 3.6, 0.0], [0.85, 7.2, 0.85]);
            append_obj_box(&mut out, &mut v, [-3.2, 0.45, 0.0], [1.3, 0.9, 1.3]);
            append_obj_box(&mut out, &mut v, [3.2, 0.45, 0.0], [1.3, 0.9, 1.3]);
            append_obj_box(&mut out, &mut v, [0.0, 5.6, 0.0], [8.4, 0.55, 0.5]);
            append_obj_box(&mut out, &mut v, [0.0, 7.2, 0.0], [10.2, 0.72, 0.95]);
            append_obj_box(&mut out, &mut v, [0.0, 6.4, 0.0], [1.3, 1.15, 0.52]);
        }
        "card_pedestal" => {
            append_obj_box(&mut out, &mut v, [0.0, 0.35, 0.0], [4.4, 0.7, 4.4]);
            append_obj_box(&mut out, &mut v, [0.0, 1.05, 0.0], [3.2, 0.7, 3.2]);
            append_obj_box(&mut out, &mut v, [0.0, 3.4, 0.0], [2.8, 3.9, 0.35]);
            append_obj_box(&mut out, &mut v, [0.0, 5.45, 0.0], [3.1, 0.25, 0.45]);
        }
        "colosseum_arch" => {
            append_obj_box(&mut out, &mut v, [-3.0, 2.8, 0.0], [1.4, 5.6, 1.6]);
            append_obj_box(&mut out, &mut v, [3.0, 2.8, 0.0], [1.4, 5.6, 1.6]);
            append_obj_box(&mut out, &mut v, [-2.0, 6.3, 0.0], [1.5, 1.2, 1.5]);
            append_obj_box(&mut out, &mut v, [0.0, 7.2, 0.0], [2.6, 1.1, 1.5]);
            append_obj_box(&mut out, &mut v, [2.0, 6.3, 0.0], [1.5, 1.2, 1.5]);
            append_obj_box(&mut out, &mut v, [0.0, 8.1, 0.0], [8.2, 0.65, 1.9]);
        }
        "cauldron" => {
            append_obj_box(&mut out, &mut v, [0.0, 2.0, 0.0], [3.4, 2.6, 3.4]);
            append_obj_box(&mut out, &mut v, [0.0, 3.45, 0.0], [3.8, 0.45, 3.8]);
            append_obj_box(&mut out, &mut v, [-1.2, 0.4, -1.0], [0.55, 0.8, 0.55]);
            append_obj_box(&mut out, &mut v, [1.2, 0.4, -1.0], [0.55, 0.8, 0.55]);
            append_obj_box(&mut out, &mut v, [0.0, 0.4, 1.2], [0.55, 0.8, 0.55]);
        }
        _ => {
            // crystal_obelisk
            append_obj_box(&mut out, &mut v, [0.0, 0.5, 0.0], [3.8, 1.0, 3.8]);
            append_obj_box(&mut out, &mut v, [0.0, 3.2, 0.0], [2.0, 4.4, 2.0]);
            append_obj_box(&mut out, &mut v, [0.0, 6.2, 0.0], [1.1, 1.8, 1.1]);
            append_obj_box(&mut out, &mut v, [0.0, 7.4, 0.0], [0.5, 0.9, 0.5]);
        }
    }
    out
}

fn parse_obj_geometry(obj: &str) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for line in obj.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("v ") {
            let nums: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();
            if nums.len() >= 3 {
                verts.push([nums[0], nums[1], nums[2]]);
            }
        } else if let Some(rest) = trimmed.strip_prefix("f ") {
            let idx: Vec<usize> = rest
                .split_whitespace()
                .filter_map(|tok| tok.split('/').next()?.parse::<isize>().ok())
                .filter_map(|i| {
                    if i > 0 {
                        Some((i - 1) as usize)
                    } else if i < 0 {
                        verts.len().checked_sub(i.unsigned_abs())
                    } else {
                        None
                    }
                })
                .collect();
            if idx.len() >= 3 {
                for k in 1..(idx.len() - 1) {
                    tris.push([idx[0], idx[k], idx[k + 1]]);
                }
            }
        }
    }
    (verts, tris)
}

/// Render an isometric wireframe + flat-shaded `320x320` PNG preview directly from `.obj` geometry.
fn render_obj_preview_png(verts: &[[f64; 3]], tris: &[[usize; 3]], preset: &str) -> Vec<u8> {
    const W: usize = 320;
    const H: usize = 320;
    let mut rgb = vec![0u8; W * H * 3];

    // Studio dark slate backdrop with subtle horizon grid
    for y in 0..H {
        let t = y as f32 / H as f32;
        let bg = [
            (18.0 + 16.0 * t) as u8,
            (22.0 + 18.0 * t) as u8,
            (34.0 + 22.0 * t) as u8,
        ];
        for x in 0..W {
            let col = if y > 220 && (x % 24 == 0 || y % 20 == 0) {
                [46, 54, 72]
            } else {
                bg
            };
            let idx = (y * W + x) * 3;
            rgb[idx] = col[0];
            rgb[idx + 1] = col[1];
            rgb[idx + 2] = col[2];
        }
    }

    let accent: [u8; 3] = match preset {
        "torii_gate" => [228, 76, 60],
        "card_pedestal" => [245, 184, 52],
        "colosseum_arch" => [206, 194, 168],
        "cauldron" => [102, 224, 148],
        _ => [92, 200, 250],
    };

    if !verts.is_empty() {
        let mut min_v = [f64::INFINITY; 3];
        let mut max_v = [f64::NEG_INFINITY; 3];
        for v in verts {
            for axis in 0..3 {
                min_v[axis] = min_v[axis].min(v[axis]);
                max_v[axis] = max_v[axis].max(v[axis]);
            }
        }
        let center = [
            (min_v[0] + max_v[0]) * 0.5,
            (min_v[1] + max_v[1]) * 0.5,
            (min_v[2] + max_v[2]) * 0.5,
        ];
        let span = (max_v[0] - min_v[0])
            .max(max_v[1] - min_v[1])
            .max(max_v[2] - min_v[2])
            .max(1.0);
        let scale = 190.0 / span;

        let project = |p: [f64; 3]| -> (i32, i32) {
            let x = p[0] - center[0];
            let y = p[1] - center[1];
            let z = p[2] - center[2];
            let yaw = 0.62f64;
            let rx = x * yaw.cos() - z * yaw.sin();
            let rz = x * yaw.sin() + z * yaw.cos();
            let sx = (W as f64 * 0.5 + rx * scale) as i32;
            let sy = (H as f64 * 0.54 - (y * 0.88 - rz * 0.34) * scale) as i32;
            (sx, sy)
        };

        for tri in tris {
            if tri[0] < verts.len() && tri[1] < verts.len() && tri[2] < verts.len() {
                let p0 = project(verts[tri[0]]);
                let p1 = project(verts[tri[1]]);
                let p2 = project(verts[tri[2]]);
                draw_line(&mut rgb, W, H, p0, p1, accent);
                draw_line(&mut rgb, W, H, p1, p2, accent);
                draw_line(&mut rgb, W, H, p2, p0, accent);
            }
        }
    }

    encode_png_rgb(W as u32, H as u32, &rgb)
}

fn draw_line(
    rgb: &mut [u8],
    w: usize,
    h: usize,
    a: (i32, i32),
    b: (i32, i32),
    col: [u8; 3],
) {
    let mut x0 = a.0;
    let mut y0 = a.1;
    let x1 = b.0;
    let y1 = b.1;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    for _ in 0..800 {
        if x0 >= 0 && y0 >= 0 && (x0 as usize) < w && (y0 as usize) < h {
            let idx = ((y0 as usize) * w + (x0 as usize)) * 3;
            rgb[idx] = col[0];
            rgb[idx + 1] = col[1];
            rgb[idx + 2] = col[2];
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn encode_png_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let stride = (width as usize) * 3;
    let mut raw = Vec::with_capacity((stride + 1) * (height as usize));
    for y in 0..(height as usize) {
        raw.push(0u8);
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }
    let zlib_data = zlib_store(&raw);
    let mut out = Vec::with_capacity(zlib_data.len() + 64);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(2);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    push_png_chunk(&mut out, b"IHDR", &ihdr);
    push_png_chunk(&mut out, b"IDAT", &zlib_data);
    push_png_chunk(&mut out, b"IEND", &[]);
    out
}

fn push_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_in = Vec::with_capacity(4 + data.len());
    crc_in.extend_from_slice(chunk_type);
    crc_in.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 32);
    out.push(0x78);
    out.push(0x01);
    let mut off = 0usize;
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    } else {
        while off < data.len() {
            let chunk = (data.len() - off).min(65_535);
            out.push(if off + chunk >= data.len() { 0x01 } else { 0x00 });
            let l16 = chunk as u16;
            out.extend_from_slice(&l16.to_le_bytes());
            out.extend_from_slice(&(!l16).to_le_bytes());
            out.extend_from_slice(&data[off..off + chunk]);
            off += chunk;
        }
    }
    const MOD: u32 = 65_521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + b as u32) % MOD;
        s2 = (s2 + s1) % MOD;
    }
    out.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
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

// ---------------------------------------------------------------------------
// Static Pack Catalog (3 Pre-Installed Defaults + 5 One-Click Installable)
// ---------------------------------------------------------------------------

static PACK_SPECS: [PackSpec; 8] = [
    // --- 3 Pre-Installed Default Packs ---
    PackSpec {
        id: "roblox-studio-core",
        name: "Roblox Studio Core",
        category: "Core Engine",
        description: "Pre-installed Roblox Luau, server-authoritative networking, DataStore persistence, and all 13 live Roblox Studio MCP bridge workflows.",
        default_installed: true,
        version: "0.9.0",
        default_provider: "roblox-studio-mcp",
        tools: &[
            "list_roblox_studios",
            "get_studio_state",
            "search_game_tree",
            "inspect_instance",
            "script_read",
            "script_grep",
            "multi_edit",
            "execute_luau",
            "get_console_output",
            "screen_capture",
            "start_stop_play",
            "user_mouse_input",
            "user_keyboard_input",
        ],
        capabilities: &[
            "luau-scripting",
            "server-authoritative-remotes",
            "datastore-pcall-bindtoclose",
            "live-studio-mcp",
        ],
        env_vars: &["OPENRO_MCP_COMMAND"],
        skill_summary: "Authoritative Roblox Luau services, RemoteEvent validation & rate limits, pcall+UpdateAsync+BindToClose DataStores, and live Studio MCP tool orchestration.",
        skill_markdown: "- Always fetch services with `game:GetService(...)` and set `Instance.Parent` last after configuring properties.\n\
- Never trust client RemoteEvent arguments: validate types, finiteness, allowlisted IDs, ownership, and per-player cooldowns strictly on the server.\n\
- Guard every `DataStoreService` call in `pcall`, prefer `UpdateAsync`, load on `PlayerAdded`, and save on `PlayerRemoving` + `game:BindToClose`.\n\
- When Studio MCP is attached, inspect the live tree with `search_game_tree`/`inspect_instance`, edit via `multi_edit`, verify with `start_stop_play` + `get_console_output`, and capture proof frames with `screen_capture`.",
    },
    PackSpec {
        id: "blender-3d-pipeline",
        name: "Blender 3D & UV Pipeline",
        category: "3D Pipeline",
        description: "Pre-installed Blender 5.x headless 3D prop generation (.obj + preview PNG), clean UV unwrapping, PBR color-space rules, and Roblox stud-scale alignment.",
        default_installed: true,
        version: "0.9.0",
        default_provider: "blender-local",
        tools: &[
            "generate_blender_mesh",
            "uv_atlas_guide",
            "bake_mesh_preview",
        ],
        capabilities: &[
            "headless-blender-obj",
            "uv-unwrapping",
            "pbr-colorspace-rules",
            "procedural-obj-fallback",
        ],
        env_vars: &["OPENRO_BLENDER_PATH"],
        skill_summary: "Headless Blender 3D mesh generation, sRGB albedo vs Non-Color roughness/normal maps, Principled BSDF export, and Roblox scale conventions.",
        skill_markdown: "- Use `POST /api/tools/blender/generate` (`torii_gate`, `card_pedestal`, `colosseum_arch`, `cauldron`, `crystal_obelisk`) for clean triangulated `.obj` props and preview PNGs.\n\
- Keep basecolor/albedo textures in `sRGB` and roughness/metallic/normal/AO maps in `Non-Color`.\n\
- Map texture atlases by normalized UV rectangle rather than stretching a full atlas across one face, and key alpha decals with `BLEND` or `CLIP`.",
    },
    PackSpec {
        id: "anime-fan-art-knowledge",
        name: "Fan-Art & Creative Homage Knowledge Point",
        category: "Art Direction",
        description: "Pre-installed anime character card craft pillars, transformation checklist, rarity VFX specs, and Nano Banana prompt architecture.",
        default_installed: true,
        version: "0.9.0",
        default_provider: "built-in-knowledge",
        tools: &[
            "fan_art_knowledge_point",
            "anime_card_prompt_guide",
            "rarity_vfx_spec",
        ],
        capabilities: &[
            "anime-homage-direction",
            "card-rarity-tiers",
            "decal-surfacegui-binding",
        ],
        env_vars: &[],
        skill_summary: "Original anime homage craft pillars, archetype silhouettes, elemental VFX layering, and Roblox Decal/SurfaceGui card art binding.",
        skill_markdown: "- Design bold original anime-inspired silhouettes, distinct elemental color keys, and readable card stats (`ATK`, `HP`, `SPD`, passive + ultimate).\n\
- Pair every generated portrait with both a Workspace `Decal` and `SurfaceGui > ImageLabel` frame so cards read clearly in 3D and UI.",
    },
    // --- 5 One-Click Installable Library Packs ---
    PackSpec {
        id: "image-gen-studio",
        name: "Image Gen Studio",
        category: "2D Art & Textures",
        description: "Unlock AI character card portraits, skybox textures, and prop decals via Google Gemini (Nano Banana) or OpenAI Image.",
        default_installed: false,
        version: "0.9.0",
        default_provider: "gemini-nano-banana",
        tools: &[
            "generate_card_art",
            "generate_texture_pack",
            "edit_reference_art",
        ],
        capabilities: &[
            "gemini-nano-banana",
            "openai-image",
            "card-portrait-art",
        ],
        env_vars: &["GEMINI_API_KEY", "OPENAI_API_KEY"],
        skill_summary: "Generate anime card portraits, UI icons, and PBR decals with Gemini Nano Banana or OpenAI Image and bind directly to place assets.",
        skill_markdown: "- Use `gemini-2.5-flash-image` (Nano Banana) or `gpt-image-1` for 3:4 portrait card art and seamless arena decals.\n\
- Save generated assets through `AssetManager` so SHA-256 manifests and Roblox `Decal`/`ImageLabel` bindings stay synchronized.",
    },
    PackSpec {
        id: "model-3d-gen",
        name: "3D Model Generator",
        category: "3D Modeling",
        description: "Unlock one-click 3D prop generation powered by local Blender 5.x (with procedural .obj fallback) plus optional Meshy or Tripo cloud APIs.",
        default_installed: false,
        version: "0.9.0",
        default_provider: "blender-local",
        tools: &[
            "generate_3d_mesh",
            "generate_torii_prop",
            "export_obj_mesh",
        ],
        capabilities: &[
            "blender-procedural-props",
            "meshy-3d",
            "tripo-3d",
        ],
        env_vars: &["MESHY_API_KEY", "TRIPO_API_KEY"],
        skill_summary: "Generate Roblox-ready triangulated .obj hero props locally with Blender or remotely with Meshy/Tripo.",
        skill_markdown: "- Prefer local Blender headless prop presets for instant deterministic Torii gates, pedestals, cauldrons, arches, and crystal obelisks.\n\
- Keep Roblox prop triangle counts lean (<10k tris per hero prop) and export with normals and UVs enabled.",
    },
    PackSpec {
        id: "video-gen-studio",
        name: "Video & Trailer Studio",
        category: "Trailers & Motion",
        description: "Unlock launch trailer storyboards and motion cuts powered by local OpenMotion / HyperFrames plus Google Veo and Flow.",
        default_installed: false,
        version: "0.9.0",
        default_provider: "openmotion-local",
        tools: &[
            "generate_trailer_storyboard",
            "render_openmotion_cut",
            "veo_motion_prompt",
        ],
        capabilities: &[
            "openmotion-walkthrough",
            "google-veo",
            "google-flow",
        ],
        env_vars: &["GEMINI_API_KEY", "VEO_API_KEY"],
        skill_summary: "Compose deterministic OpenMotion launch trailers and Veo/Flow cinematics for Roblox game showcases.",
        skill_markdown: "- Structure Roblox launch trailers as 4 concise gameplay chapters (Arena reveal, Core loop, Live combat/VFX, Publish/Play CTA) rendered in H.264 1280x720 30fps.",
    },
    PackSpec {
        id: "audio-sfx-gen",
        name: "Audio & SFX Studio",
        category: "Audio & SFX",
        description: "Unlock instant offline 44.1 kHz PCM16 .wav sound synthesis (summon rolls, mythic fanfare, battle crits, potions) plus ElevenLabs & Google Lyria connections.",
        default_installed: false,
        version: "0.9.0",
        default_provider: "openro-synth-local",
        tools: &[
            "generate_audio_sfx",
            "synthesize_wav_cue",
            "audio_pack_manifest",
        ],
        capabilities: &[
            "offline-wav-synth",
            "elevenlabs-sfx",
            "google-lyria",
        ],
        env_vars: &["ELEVENLABS_API_KEY", "GEMINI_API_KEY"],
        skill_summary: "Synthesize royalty-free 44.1 kHz PCM16 .wav SFX cues locally or connect ElevenLabs/Lyria for custom soundtracks.",
        skill_markdown: "- Use `POST /api/tools/audio/generate` (`summon_roll`, `mythic_fanfare`, `battle_crit`, `potion_drink`, `ui_click`, `level_up`) to generate ready-to-upload `.wav` cues immediately offline.\n\
- Register Sound instances under `SoundService` or part attachments with normalized Volume (`0.5..0.85`).",
    },
    PackSpec {
        id: "rojo-open-cloud",
        name: "Rojo & Roblox Open Cloud",
        category: "Publishing & Sync",
        description: "Unlock Rojo 7 project sync packaging and Roblox Open Cloud place/asset publishing configuration.",
        default_installed: false,
        version: "0.9.0",
        default_provider: "rojo-local",
        tools: &[
            "export_rojo_ship_pack",
            "open_cloud_publish_place",
            "open_cloud_upload_asset",
        ],
        capabilities: &[
            "rojo-7-sync",
            "roblox-open-cloud",
            "ship-pack-zip",
        ],
        env_vars: &["ROBLOX_OPEN_CLOUD_API_KEY", "ROBLOX_UNIVERSE_ID"],
        skill_summary: "Package Rojo 7 `default.project.json` + `.rbxlx` Ship Packs and configure Roblox Open Cloud publishing credentials.",
        skill_markdown: "- Export a verified Rojo Ship Pack (`.zip`) with `game.rbxlx`, `default.project.json`, `src/{server,client,shared}/*.luau`, and `luau-audit-report.json` before publishing via Studio or Open Cloud.",
    },
];
