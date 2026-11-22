//! Game Art Asset Manager for OpenRO.
//!
//! Stores verified PNG, JPEG, and WebP artwork under `data_dir/game-assets/`
//! with strict filename sanitization, magic-byte validation, symlink-escape
//! containment, atomic JSON manifest persistence, optional live Google Gemini
//! (Nano Banana) and OpenAI Image API generation (failing honestly when no API
//! key is configured), `.rbxlx` `Decal`/`ImageLabel` binding, and
//! `asset-manifest.json` export.

use crate::art_knowledge;
use crate::place::LocalPlace;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;
const MAX_FILENAME_LEN: usize = 96;
const DEFAULT_GEMINI_IMAGE_MODEL: &str = "gemini-2.5-flash-image";
const DEFAULT_OPENAI_IMAGE_MODEL: &str = "gpt-image-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetMetadata {
    pub card_id: String,
    pub title: String,
    pub rarity: String,
    pub element: String,
    pub atk: i64,
    pub hp: i64,
    pub speed: i64,
    pub odds: String,
    pub passive: String,
    pub ultimate: String,
    pub prompt: String,
    /// `"nano-banana"` | `"google-nano-banana"` | `"gemini-api"` | `"openai-image"` | `"upload"`
    pub source: String,
    pub filename: String,
    pub mime_type: String,
    pub bytes: usize,
    pub sha256: String,
    pub created_at_ms: u64,
    pub roblox_asset_id: Option<String>,
}

impl AssetMetadata {
    pub fn content_uri(&self) -> String {
        if let Some(id) = self.roblox_asset_id.as_deref() {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                if trimmed.starts_with("rbxassetid://") || trimmed.starts_with("rbxasset://") {
                    return trimmed.to_string();
                }
                return format!("rbxassetid://{trimmed}");
            }
        }
        format!("rbxasset://openro-assets/{}", self.filename)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "card_id": self.card_id,
            "title": self.title,
            "rarity": self.rarity,
            "element": self.element,
            "atk": self.atk,
            "hp": self.hp,
            "speed": self.speed,
            "odds": self.odds,
            "passive": self.passive,
            "ultimate": self.ultimate,
            "prompt": self.prompt,
            "source": self.source,
            "file": self.filename,
            "filename": self.filename,
            "mime": self.mime_type,
            "mime_type": self.mime_type,
            "bytes": self.bytes,
            "sha256": self.sha256,
            "created_at_ms": self.created_at_ms,
            "roblox_asset_id": self.roblox_asset_id,
            "content_uri": self.content_uri(),
            "url": format!("/api/assets/{}", self.filename),
            "image_url": format!("/api/assets/{}", self.filename),
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let filename = obj.get("filename")?.as_str()?.to_string();
        Some(AssetMetadata {
            card_id: obj
                .get("card_id")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            title: obj
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            rarity: obj
                .get("rarity")
                .and_then(|x| x.as_str())
                .unwrap_or("Rare")
                .to_string(),
            element: obj
                .get("element")
                .and_then(|x| x.as_str())
                .unwrap_or("Flame")
                .to_string(),
            atk: obj.get("atk").and_then(|x| x.as_i64()).unwrap_or(250),
            hp: obj.get("hp").and_then(|x| x.as_i64()).unwrap_or(1000),
            speed: obj.get("speed").and_then(|x| x.as_i64()).unwrap_or(85),
            odds: obj
                .get("odds")
                .and_then(|x| x.as_str())
                .unwrap_or("1 in 32")
                .to_string(),
            passive: obj
                .get("passive")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            ultimate: obj
                .get("ultimate")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            prompt: obj
                .get("prompt")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            source: obj
                .get("source")
                .and_then(|x| x.as_str())
                .unwrap_or("upload")
                .to_string(),
            filename,
            mime_type: obj
                .get("mime_type")
                .and_then(|x| x.as_str())
                .unwrap_or("image/png")
                .to_string(),
            bytes: obj.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            sha256: obj
                .get("sha256")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            created_at_ms: obj
                .get("created_at_ms")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            roblox_asset_id: obj
                .get("roblox_asset_id")
                .and_then(|x| x.as_str())
                .map(ToString::to_string),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct AssetMetadataInput {
    pub card_id: Option<String>,
    pub title: Option<String>,
    pub rarity: Option<String>,
    pub element: Option<String>,
    pub atk: Option<i64>,
    pub hp: Option<i64>,
    pub speed: Option<i64>,
    pub odds: Option<String>,
    pub passive: Option<String>,
    pub ultimate: Option<String>,
    pub prompt: Option<String>,
    pub source: Option<String>,
    pub roblox_asset_id: Option<String>,
}

impl AssetMetadataInput {
    pub fn from_json(v: &Value) -> Self {
        let s = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
        };
        let n = |k: &str| v.get(k).and_then(|x| x.as_i64());
        AssetMetadataInput {
            card_id: s("card_id"),
            title: s("title"),
            rarity: s("rarity"),
            element: s("element"),
            atk: n("atk"),
            hp: n("hp"),
            speed: n("speed"),
            odds: s("odds"),
            passive: s("passive"),
            ultimate: s("ultimate"),
            prompt: s("prompt"),
            source: s("source"),
            roblox_asset_id: s("roblox_asset_id"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenerateImageRequest {
    pub provider: String,
    pub card_id: String,
    pub title: String,
    pub rarity: String,
    pub element: String,
    pub atk: i64,
    pub hp: i64,
    pub speed: i64,
    pub passive: String,
    pub ultimate: String,
    pub prompt: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

/// Strict filename sanitization blocking traversal, path separators, NULs, hidden files,
/// control characters, and non-image extensions.
pub fn sanitize_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("asset filename must not be empty".to_string());
    }
    if trimmed.len() > MAX_FILENAME_LEN {
        return Err(format!(
            "asset filename exceeds maximum length ({MAX_FILENAME_LEN} chars)"
        ));
    }
    if trimmed.contains('\0')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.starts_with('.')
    {
        return Err(format!(
            "invalid asset filename '{trimmed}': path traversal or separator characters are forbidden"
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!(
            "invalid asset filename '{trimmed}': only ASCII letters, digits, '.', '-', and '_' are allowed"
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp"))
    {
        return Err(format!(
            "unsupported asset extension in '{trimmed}': expected .png, .jpg, .jpeg, or .webp"
        ));
    }
    Ok(trimmed.to_string())
}

/// Normalize a slug/card_id into lowercase ASCII kebab-case safe for filenames and Roblox Instance names.
pub fn sanitize_card_id(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "card-art".to_string()
    } else if trimmed.len() > 64 {
        trimmed[..64].trim_matches('-').to_string()
    } else {
        trimmed
    }
}

/// Inspect image magic bytes to ensure uploaded files are genuinely PNG, JPEG, or WebP.
pub fn detect_image_mime(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    if bytes.is_empty() {
        return Err("image payload is empty".to_string());
    }
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(format!(
            "image payload exceeds maximum size ({MAX_ASSET_BYTES} bytes)"
        ));
    }
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        return Ok(("image/png", "png"));
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Ok(("image/jpeg", "jpg"));
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(("image/webp", "webp"));
    }
    Err("invalid image binary: expected PNG, JPEG, or WebP magic bytes".to_string())
}

pub fn mime_for_filename(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn normalize_source(raw: Option<&str>) -> String {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("nano-banana") | Some("google-nano-banana") => "nano-banana".to_string(),
        Some("gemini-api") | Some("gemini") => "gemini-api".to_string(),
        Some("openai-image") | Some("openai") => "openai-image".to_string(),
        _ => "upload".to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Zero-dependency RFC 4648 base64 decoder accepting optional `data:<mime>;base64,` prefix.
pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    let payload = if let Some(idx) = trimmed.find(";base64,") {
        &trimmed[idx + 8..]
    } else {
        trimmed
    };
    let mut out = Vec::with_capacity((payload.len() * 3) / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for b in payload.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val: u32 = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err(format!("invalid base64 byte: 0x{b:02x}")),
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    if out.is_empty() && !payload.trim().is_empty() {
        return Err("base64 payload decoded to zero bytes".to_string());
    }
    Ok(out)
}

/// Zero-dependency RFC 4648 base64 encoder.
pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn rarity_hex_color(rarity: &str) -> &'static str {
    match rarity.to_ascii_lowercase().as_str() {
        "secret" => "#FFC43C",
        "celestial" => "#A855F7",
        "mythic" => "#EB532A",
        "legendary" => "#38BDF8",
        "epic" => "#C026D3",
        "rare" => "#34D399",
        _ => "#94A3B8",
    }
}

/// Persistent manager for `data_dir/game-assets/`.
#[derive(Clone, Debug)]
pub struct AssetManager {
    assets_dir: PathBuf,
}

impl AssetManager {
    pub fn open(data_dir: &Path) -> Self {
        let assets_dir = data_dir.join("game-assets");
        let _ = std::fs::create_dir_all(&assets_dir);
        AssetManager { assets_dir }
    }

    pub fn assets_dir(&self) -> &Path {
        &self.assets_dir
    }

    fn manifest_path(&self) -> PathBuf {
        self.assets_dir.join("manifest.json")
    }

    fn load_manifest_map(&self) -> BTreeMap<String, AssetMetadata> {
        let mut map = BTreeMap::new();
        let Ok(raw) = std::fs::read_to_string(self.manifest_path()) else {
            return map;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            return map;
        };
        if let Some(arr) = val.get("assets").and_then(|v| v.as_array()) {
            for entry in arr {
                if let Some(meta) = AssetMetadata::from_json(entry) {
                    map.insert(meta.filename.clone(), meta);
                }
            }
        }
        map
    }

    fn save_manifest_map(&self, map: &BTreeMap<String, AssetMetadata>) -> Result<(), String> {
        std::fs::create_dir_all(&self.assets_dir)
            .map_err(|e| format!("create game-assets dir: {e}"))?;
        let items: Vec<Value> = map.values().map(AssetMetadata::to_json).collect();
        let doc = json!({
            "version": 1,
            "count": items.len(),
            "assets": items,
        });
        let text = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("serialize asset manifest: {e}"))?;
        let tmp = self.assets_dir.join("manifest.json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("write manifest tmp: {e}"))?;
        std::fs::rename(&tmp, self.manifest_path())
            .map_err(|e| format!("commit asset manifest: {e}"))?;
        Ok(())
    }

    pub fn list(&self) -> Vec<AssetMetadata> {
        self.load_manifest_map()
            .into_values()
            .filter(|m| self.assets_dir.join(&m.filename).is_file())
            .collect()
    }

    pub fn list_json(&self) -> Value {
        let items: Vec<Value> = self.list().iter().map(AssetMetadata::to_json).collect();
        json!({
            "count": items.len(),
            "assets": items,
        })
    }

    /// Save validated raw image bytes under `data_dir/game-assets/<filename>` atomically.
    pub fn save_bytes(
        &self,
        raw_filename: &str,
        bytes: &[u8],
        input: AssetMetadataInput,
    ) -> Result<AssetMetadata, String> {
        let filename = sanitize_filename(raw_filename)?;
        let (mime_type, _) = detect_image_mime(bytes)?;
        std::fs::create_dir_all(&self.assets_dir)
            .map_err(|e| format!("create game-assets dir: {e}"))?;

        let target = self.assets_dir.join(&filename);
        let tmp = self.assets_dir.join(format!(".{filename}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| format!("write asset tmp: {e}"))?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("commit asset file: {e}"))?;

        let stem = filename
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(&filename);
        let card_id = input
            .card_id
            .as_deref()
            .map(sanitize_card_id)
            .unwrap_or_else(|| sanitize_card_id(stem));

        // Enrich defaults from the built-in Anime Archetype catalog if card_id matches
        let guide = art_knowledge::anime_archetypes()
            .iter()
            .find(|a| a.card_id == card_id);

        let meta = AssetMetadata {
            card_id: card_id.clone(),
            title: input.title.unwrap_or_else(|| {
                guide
                    .map(|g| g.homage_title.to_string())
                    .unwrap_or_else(|| stem.replace(['-', '_'], " "))
            }),
            rarity: input
                .rarity
                .unwrap_or_else(|| guide.map(|g| g.rarity).unwrap_or("Rare").to_string()),
            element: input
                .element
                .unwrap_or_else(|| guide.map(|g| g.element).unwrap_or("Flame").to_string()),
            atk: input.atk.unwrap_or_else(|| guide.map(|g| g.atk).unwrap_or(280)),
            hp: input.hp.unwrap_or_else(|| guide.map(|g| g.hp).unwrap_or(1050)),
            speed: input
                .speed
                .unwrap_or_else(|| guide.map(|g| g.speed).unwrap_or(85)),
            odds: input
                .odds
                .unwrap_or_else(|| guide.map(|g| g.odds).unwrap_or("1 in 32").to_string()),
            passive: input.passive.unwrap_or_else(|| {
                guide
                    .map(|g| g.passive.to_string())
                    .unwrap_or_else(|| "Signature passive aura".to_string())
            }),
            ultimate: input.ultimate.unwrap_or_else(|| {
                guide
                    .map(|g| g.ultimate.to_string())
                    .unwrap_or_else(|| "Apex Burst Strike".to_string())
            }),
            prompt: input.prompt.unwrap_or_else(|| {
                guide
                    .map(|g| g.art_prompt.to_string())
                    .unwrap_or_default()
            }),
            source: normalize_source(input.source.as_deref()),
            filename: filename.clone(),
            mime_type: mime_type.to_string(),
            bytes: bytes.len(),
            sha256: sha256_hex(bytes),
            created_at_ms: now_ms(),
            roblox_asset_id: input.roblox_asset_id,
        };

        let mut map = self.load_manifest_map();
        map.insert(filename, meta.clone());
        self.save_manifest_map(&map)?;
        Ok(meta)
    }

    /// Save a base64-encoded image (plain or `data:image/...;base64,...`) under `data_dir/game-assets/`.
    pub fn save_base64(
        &self,
        raw_filename: &str,
        b64: &str,
        input: AssetMetadataInput,
    ) -> Result<AssetMetadata, String> {
        let bytes = decode_base64(b64)?;
        self.save_bytes(raw_filename, &bytes, input)
    }

    /// Read raw image bytes with strict filename validation and canonical directory jail check.
    pub fn read_bytes(&self, raw_filename: &str) -> Result<(Vec<u8>, &'static str), String> {
        let filename = sanitize_filename(raw_filename)?;
        let root = std::fs::canonicalize(&self.assets_dir)
            .map_err(|_| "asset directory unavailable".to_string())?;
        let candidate = self.assets_dir.join(&filename);
        let canonical =
            std::fs::canonicalize(&candidate).map_err(|_| format!("asset not found: {filename}"))?;
        if !canonical.starts_with(&root) {
            return Err("asset path escapes game-assets root".to_string());
        }
        let meta = std::fs::metadata(&canonical)
            .map_err(|_| format!("asset not found: {filename}"))?;
        if !meta.is_file() || meta.len() as usize > MAX_ASSET_BYTES {
            return Err(format!("invalid asset file: {filename}"));
        }
        let bytes = std::fs::read(&canonical).map_err(|e| format!("read asset: {e}"))?;
        let mime = detect_image_mime(&bytes)
            .map(|(m, _)| m)
            .unwrap_or_else(|_| mime_for_filename(&filename));
        Ok((bytes, mime))
    }

    /// Delete an asset and remove its manifest entry.
    pub fn delete(&self, raw_filename: &str) -> Result<bool, String> {
        let filename = sanitize_filename(raw_filename)?;
        let path = self.assets_dir.join(&filename);
        let existed = path.is_file();
        if existed {
            std::fs::remove_file(&path).map_err(|e| format!("remove asset: {e}"))?;
        }
        let mut map = self.load_manifest_map();
        let removed_meta = map.remove(&filename).is_some();
        if removed_meta {
            self.save_manifest_map(&map)?;
        }
        Ok(existed || removed_meta)
    }

    /// Generate live card art via Google Gemini Image (Nano Banana) or OpenAI Image API.
    /// Fails honestly with clear setup guidance when no API key is configured — never fakes pixels.
    pub fn generate_image(&self, req: &GenerateImageRequest) -> Result<AssetMetadata, String> {
        let provider = req.provider.trim().to_ascii_lowercase();
        let effective_prompt = if req.prompt.trim().is_empty() {
            art_knowledge::build_homage_prompt(&req.title, &req.element, &req.rarity, "")
        } else {
            req.prompt.trim().to_string()
        };
        let card_id = sanitize_card_id(if req.card_id.trim().is_empty() {
            &req.title
        } else {
            &req.card_id
        });

        match provider.as_str() {
            "" | "gemini" | "gemini-api" | "nano-banana" | "google-nano-banana" => {
                let api_key = req
                    .api_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        std::env::var("GEMINI_API_KEY")
                            .ok()
                            .map(|k| k.trim().to_string())
                            .filter(|k| !k.is_empty())
                    })
                    .ok_or_else(|| {
                        "Google Gemini / Nano Banana image generation requires GEMINI_API_KEY (set it in Settings or environment), or upload PNG/WebP card art directly via POST /api/assets/upload.".to_string()
                    })?;

                let model = req
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        std::env::var("OPENRO_GEMINI_IMAGE_MODEL")
                            .ok()
                            .map(|m| m.trim().to_string())
                            .filter(|m| !m.is_empty())
                    })
                    .unwrap_or_else(|| DEFAULT_GEMINI_IMAGE_MODEL.to_string());

                let base = req
                    .base_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .unwrap_or("https://generativelanguage.googleapis.com");
                let url = format!(
                    "{}/v1beta/models/{model}:generateContent?key={api_key}",
                    base.trim_end_matches('/')
                );

                let body = json!({
                    "contents": [{
                        "parts": [{ "text": effective_prompt }]
                    }],
                    "generationConfig": {
                        "responseModalities": ["IMAGE"]
                    }
                });
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(90))
                    .build();
                let resp = agent
                    .post(&url)
                    .set("Content-Type", "application/json")
                    .send_json(body)
                    .map_err(|e| format!("Gemini image request failed: {e}"))?;
                let payload: Value = resp
                    .into_json()
                    .map_err(|e| format!("Gemini image response JSON parse failed: {e}"))?;

                let (b64, ext) = extract_gemini_inline_image(&payload).ok_or_else(|| {
                    "Gemini image API returned no inline image data".to_string()
                })?;
                let filename = format!("{card_id}.{ext}");
                self.save_base64(
                    &filename,
                    &b64,
                    AssetMetadataInput {
                        card_id: Some(card_id),
                        title: Some(req.title.clone()),
                        rarity: Some(req.rarity.clone()),
                        element: Some(req.element.clone()),
                        atk: Some(req.atk),
                        hp: Some(req.hp),
                        speed: Some(req.speed),
                        odds: None,
                        passive: Some(req.passive.clone()),
                        ultimate: Some(req.ultimate.clone()),
                        prompt: Some(effective_prompt),
                        source: Some("gemini-api".to_string()),
                        roblox_asset_id: None,
                    },
                )
            }
            "openai" | "openai-image" => {
                let api_key = req
                    .api_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        std::env::var("OPENAI_API_KEY")
                            .ok()
                            .map(|k| k.trim().to_string())
                            .filter(|k| !k.is_empty())
                    })
                    .ok_or_else(|| {
                        "OpenAI image generation requires OPENAI_API_KEY (set it in Settings or environment), or upload PNG/WebP card art directly via POST /api/assets/upload.".to_string()
                    })?;

                let model = req
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .unwrap_or(DEFAULT_OPENAI_IMAGE_MODEL);
                let base = req
                    .base_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .unwrap_or("https://api.openai.com/v1");
                let url = format!("{}/images/generations", base.trim_end_matches('/'));
                let body = json!({
                    "model": model,
                    "prompt": effective_prompt,
                    "size": "1024x1536",
                    "response_format": "b64_json"
                });
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(90))
                    .build();
                let resp = agent
                    .post(&url)
                    .set("Authorization", &format!("Bearer {api_key}"))
                    .set("Content-Type", "application/json")
                    .send_json(body)
                    .map_err(|e| format!("OpenAI image request failed: {e}"))?;
                let payload: Value = resp
                    .into_json()
                    .map_err(|e| format!("OpenAI image response JSON parse failed: {e}"))?;
                let b64 = payload
                    .pointer("/data/0/b64_json")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "OpenAI image API returned no b64_json image data".to_string())?;
                let filename = format!("{card_id}.png");
                self.save_base64(
                    &filename,
                    b64,
                    AssetMetadataInput {
                        card_id: Some(card_id),
                        title: Some(req.title.clone()),
                        rarity: Some(req.rarity.clone()),
                        element: Some(req.element.clone()),
                        atk: Some(req.atk),
                        hp: Some(req.hp),
                        speed: Some(req.speed),
                        odds: None,
                        passive: Some(req.passive.clone()),
                        ultimate: Some(req.ultimate.clone()),
                        prompt: Some(effective_prompt),
                        source: Some("openai-image".to_string()),
                        roblox_asset_id: None,
                    },
                )
            }
            other => Err(format!(
                "unsupported image generation provider '{other}': expected 'gemini' / 'nano-banana' or 'openai'"
            )),
        }
    }

    /// Bind all stored card art assets into a Place's `Workspace.CardGallery`
    /// as 3D Showcase Parts with genuine `Decal` (`Texture`) and `SurfaceGui` ->
    /// `ImageLabel` (`Image`) instances ready for `.rbxlx` export!
    pub fn bind_to_place(&self, place: &mut LocalPlace) -> usize {
        bind_assets_to_place(place, &self.list())
    }

    /// Build a portable `asset-manifest.json` describing every card illustration and its
    /// Roblox Studio `Decal` / `ImageLabel` binding paths.
    pub fn asset_manifest_json(&self, game_name: &str) -> Value {
        build_asset_manifest_json(game_name, &self.list())
    }

    /// Write `asset-manifest.json` atomically to `dest_dir/asset-manifest.json`.
    pub fn write_asset_manifest(&self, dest_dir: &Path, game_name: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| format!("create manifest output dir: {e}"))?;
        let manifest = self.asset_manifest_json(game_name);
        let text = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("serialize asset-manifest.json: {e}"))?;
        let out_path = dest_dir.join("asset-manifest.json");
        let tmp_path = dest_dir.join("asset-manifest.json.tmp");
        std::fs::write(&tmp_path, text).map_err(|e| format!("write asset-manifest tmp: {e}"))?;
        std::fs::rename(&tmp_path, &out_path)
            .map_err(|e| format!("commit asset-manifest.json: {e}"))?;
        Ok(out_path)
    }
}

fn extract_gemini_inline_image(payload: &Value) -> Option<(String, &'static str)> {
    let candidates = payload.get("candidates")?.as_array()?;
    for cand in candidates {
        let parts = cand.pointer("/content/parts")?.as_array()?;
        for part in parts {
            let inline = part.get("inlineData").or_else(|| part.get("inline_data"));
            if let Some(obj) = inline.and_then(|v| v.as_object()) {
                if let Some(data) = obj.get("data").and_then(|v| v.as_str()) {
                    let mime = obj
                        .get("mimeType")
                        .or_else(|| obj.get("mime_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/png");
                    let ext = if mime.contains("webp") {
                        "webp"
                    } else if mime.contains("jpeg") || mime.contains("jpg") {
                        "jpg"
                    } else {
                        "png"
                    };
                    return Some((data.to_string(), ext));
                }
            }
        }
    }
    None
}

/// Bind a slice of `AssetMetadata` records into `Workspace.CardGallery` in `place`,
/// creating a 3D Showcase Part, front-facing `Decal` (`Texture`), and `SurfaceGui` ->
/// `ImageLabel` (`Image`) for each card.
pub fn bind_assets_to_place(place: &mut LocalPlace, assets: &[AssetMetadata]) -> usize {
    if assets.is_empty() {
        return 0;
    }
    let _ = place.call_tool(
        "create_instance",
        json!({
            "class_name": "Folder",
            "name": "CardGallery",
            "parent": "Workspace",
            "properties": {}
        }),
    );

    let count = assets.len();
    for (idx, asset) in assets.iter().enumerate() {
        let safe_id = asset.card_id.replace('-', "_");
        let stand_name = format!("Showcase_{safe_id}");
        let stand_path = format!("Workspace.CardGallery.{stand_name}");
        let x_pos = ((idx as f64) - ((count as f64 - 1.0) / 2.0)) * 9.0;
        let uri = asset.content_uri();

        let _ = place.call_tool(
            "create_instance",
            json!({
                "class_name": "Part",
                "name": stand_name,
                "parent": "Workspace.CardGallery",
                "properties": {
                    "Anchored": true,
                    "CanCollide": true,
                    "Position": [x_pos, 5.0, 28.0],
                    "Size": [6.0, 8.5, 0.8],
                    "Color": rarity_hex_color(&asset.rarity),
                    "Material": "SmoothPlastic",
                    "CardId": asset.card_id,
                    "CardTitle": asset.title,
                    "CardRarity": asset.rarity,
                    "CardElement": asset.element,
                    "OpenROAssetFile": asset.filename
                }
            }),
        );
        let _ = place.call_tool(
            "create_instance",
            json!({
                "class_name": "Decal",
                "name": "CardArtDecal",
                "parent": stand_path,
                "properties": {
                    "Texture": uri,
                    "Face": "Front",
                    "OpenROAssetFile": asset.filename,
                    "CardId": asset.card_id
                }
            }),
        );
        let gui_path = format!("{stand_path}.CardDisplayGui");
        let _ = place.call_tool(
            "create_instance",
            json!({
                "class_name": "SurfaceGui",
                "name": "CardDisplayGui",
                "parent": stand_path,
                "properties": {
                    "Face": "Front"
                }
            }),
        );
        let _ = place.call_tool(
            "create_instance",
            json!({
                "class_name": "ImageLabel",
                "name": "CardArtImage",
                "parent": gui_path,
                "properties": {
                    "Image": uri,
                    "BackgroundTransparency": 1.0,
                    "OpenROAssetFile": asset.filename,
                    "CardId": asset.card_id
                }
            }),
        );
    }
    count
}

/// Build an `asset-manifest.json` object from any list of `AssetMetadata`.
pub fn build_asset_manifest_json(game_name: &str, assets: &[AssetMetadata]) -> Value {
    let entries: Vec<Value> = assets
        .iter()
        .map(|a| {
            let safe_id = a.card_id.replace('-', "_");
            let stand_path = format!("Workspace.CardGallery.Showcase_{safe_id}");
            json!({
                "card_id": a.card_id,
                "title": a.title,
                "rarity": a.rarity,
                "element": a.element,
                "atk": a.atk,
                "hp": a.hp,
                "speed": a.speed,
                "odds": a.odds,
                "passive": a.passive,
                "ultimate": a.ultimate,
                "source": a.source,
                "filename": a.filename,
                "mime_type": a.mime_type,
                "bytes": a.bytes,
                "sha256": a.sha256,
                "roblox_content_uri": a.content_uri(),
                "place_bindings": {
                    "stand": stand_path,
                    "decal": format!("{stand_path}.CardArtDecal"),
                    "image_label": format!("{stand_path}.CardDisplayGui.CardArtImage"),
                }
            })
        })
        .collect();
    json!({
        "schema": "openro-asset-manifest-v1",
        "game": game_name,
        "asset_count": entries.len(),
        "assets": entries,
    })
}
