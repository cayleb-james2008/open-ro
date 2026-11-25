//! Game Art Asset Manager for OpenRO.
//!
//! Stores verified PNG, JPEG, and WebP artwork under `data_dir/game-assets/`
//! with strict filename sanitization, magic-byte validation, symlink-escape
//! containment, atomic JSON manifest persistence, optional live Google Gemini
//! (Nano Banana) and OpenAI Image API generation (failing honestly when no API
//! key is configured), `.rbxlx` `Decal`/`ImageLabel` binding, and
//! `asset-manifest.json` export.

use crate::art_knowledge;
use crate::opencloud::{now_iso, safe_rel, Multipart, Oc};
use crate::place::LocalPlace;
use crate::secrets::{self, SecretStore, GEMINI_KEY, OPENAI_IMAGE_KEY};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Normalize a v0.11 art/card slug preserving both `_` and `-` (lowercase ASCII alphanumeric).
pub fn sanitize_art_slug(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' {
            out.push(ch);
        } else if ch.is_whitespace() && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches(['_', '-']).to_string();
    if trimmed.is_empty() {
        "artwork".to_string()
    } else if trimmed.len() > 64 {
        trimmed[..64].trim_matches(['_', '-']).to_string()
    } else {
        trimmed
    }
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
        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.file_type().is_symlink() {
                return Err(format!("refusing to overwrite symlink asset '{filename}'"));
            }
        }
        let tmp = self.assets_dir.join(format!(".{filename}.{}.tmp", std::process::id()));
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
        let candidate = self.assets_dir.join(&filename);
        if let Ok(meta) = std::fs::symlink_metadata(&candidate) {
            if meta.file_type().is_symlink() {
                return Err(format!("refusing to read symlink asset '{filename}'"));
            }
        }
        let root = std::fs::canonicalize(&self.assets_dir)
            .map_err(|_| "asset directory unavailable".to_string())?;
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
        let existed = match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!("refusing to delete symlink asset '{filename}'"));
                }
                meta.file_type().is_file()
            }
            Err(_) => false,
        };
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

// =========================================================================
// v0.11 Project Art Pipeline: ChatGPT (shared browser) default + Gemini + OpenAI
// =========================================================================

static CHATGPT_SERIAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

type ArtRes<T> = Result<T, Oc>;
fn art_bad<T>(msg: impl Into<String>) -> ArtRes<T> {
    Err(Oc { status: 400, msg: msg.into() })
}
fn art_err<T>(status: u16, msg: impl Into<String>) -> ArtRes<T> {
    Err(Oc { status, msg: msg.into() })
}

const LOW_ANGLE_PHRASES: &[&str] = &[
    "worm's-eye",
    "worms-eye",
    "worm's eye",
    "low-angle",
    "low angle",
    "upward-looking",
    "looking up",
    "from-below",
    "from below",
    "ground-up",
    "camera below the subject",
    "upward perspective",
    "floor-up",
    "upshot",
    "camera at ground height",
    "camera near the floor",
    "view from ground",
];

const NEUTRAL_CAMERA_PHRASES: &[&str] = &[
    "eye-level",
    "eye level",
    "neutral three-quarter",
    "chest-height",
    "shoulder-height",
    "straight-on",
    "level camera",
];

/// Collapse all line breaks and runs of whitespace into single spaces (required by ChatGPT composer).
pub fn collapse_one_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Validate and normalize a character-art prompt before acquiring the shared-browser lease:
/// rejects low-angle / worm's-eye camera wording and appends neutral eye-level framing if absent.
pub fn prepare_character_prompt(raw: &str) -> Result<String, String> {
    let one = collapse_one_line(raw);
    if one.is_empty() {
        return Err("prompt cannot be empty.".to_string());
    }
    let lower = one.to_ascii_lowercase();
    for bad in LOW_ANGLE_PHRASES {
        if lower.contains(bad) {
            return Err(format!(
                "Character art prompt contains disallowed low-angle phrasing '{bad}'. Use eye-level or neutral three-quarter camera framing instead."
            ));
        }
    }
    if NEUTRAL_CAMERA_PHRASES.iter().any(|p| lower.contains(p)) {
        Ok(one)
    } else {
        Ok(format!("{one}, eye-level camera, neutral three-quarter view"))
    }
}

/// Append a concise Roblox-safe zero-blood/zero-gore/zero-tobacco directive to art prompts unless already stated (#72).
pub fn ensure_roblox_safe_prompt(prompt: &str) -> String {
    let one = collapse_one_line(prompt);
    let lower = one.to_ascii_lowercase();
    if lower.contains("zero blood") || lower.contains("no blood") || lower.contains("roblox-safe") {
        one
    } else {
        format!(
            "{one}, Roblox-safe anime style: strictly zero blood, zero gore, zero open wounds, zero crimson splatters on skin, clothes, or weapons, and strictly zero tobacco, cigarettes, or smoking; use stylized glowing energy or chakra effects instead"
        )
    }
}

/// Normalize a weapon/prop sheet prompt for clean orthographic design sheets (#73).
pub fn ensure_weapon_sheet_prompt(prompt: &str) -> String {
    let one = collapse_one_line(prompt);
    let lower = one.to_ascii_lowercase();
    let with_sheet = if lower.contains("orthographic") || lower.contains("reference sheet") {
        one
    } else {
        format!(
            "{one}, clean orthographic prop reference sheet on solid neutral studio backdrop showing full side profile and blade/guard/grip detail views, standalone weapon only, zero characters or hands, zero text or watermarks"
        )
    };
    ensure_roblox_safe_prompt(&with_sheet)
}

/// Extract `(width, height)` from PNG/JPEG/WebP header bytes without external dependencies.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG: 8-byte signature + 4-byte len + "IHDR" + 4-byte w + 4-byte h
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((w, h));
    }
    // JPEG: scan SOF0/SOF1/SOF2 markers
    if bytes.len() > 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if matches!(marker, 0xC0..=0xC2) && i + 8 < bytes.len() {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((w, h));
            }
            if seg_len < 2 {
                break;
            }
            i += 2 + seg_len;
        }
    }
    // WebP VP8X / VP8L / VP8
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        if &bytes[12..16] == b"VP8X" {
            let w = 1 + (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16));
            let h = 1 + (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16));
            return Some((w, h));
        }
        if &bytes[12..16] == b"VP8L" && bytes.len() >= 25 {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            return Some((w, h));
        }
    }
    None
}

/// Append one JSONL record to `<root>/art/ART-LOG.jsonl`.
pub fn append_art_log(root: &Path, entry: &Value) -> Result<(), String> {
    let art_dir = root.join("art");
    std::fs::create_dir_all(&art_dir).map_err(|e| format!("create art dir: {e}"))?;
    let log_path = art_dir.join("ART-LOG.jsonl");
    let line = serde_json::to_string(entry).map_err(|e| format!("encode art log: {e}"))?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("open ART-LOG.jsonl: {e}"))?;
    writeln!(f, "{line}").map_err(|e| format!("write ART-LOG.jsonl: {e}"))
}

fn write_meta_atomic(path: &Path, meta: &Value) -> ArtRes<()> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| Oc { status: 500, msg: format!("encode metadata: {e}") })?;
    let tmp = path.with_extension("meta.tmp-openro");
    std::fs::write(&tmp, format!("{text}\n"))
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| Oc { status: 500, msg: format!("write metadata: {e}") })
}

fn meta_path_for(rel: &str) -> String {
    let stem = rel.rsplit_once('.').map(|(s, _)| s).unwrap_or(rel);
    format!("{stem}.meta.json")
}

fn art_image_url(project: &str, rel: &str) -> String {
    if project.is_empty() {
        format!("/api/art/image?path={rel}")
    } else {
        format!("/api/art/image?project={project}&path={rel}")
    }
}

fn default_out_path(body: &Value) -> String {
    if let Some(p) = body
        .get("out_path")
        .or_else(|| body.get("output"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return p.to_string();
    }
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("card").to_ascii_lowercase();
    let slug = sanitize_art_slug(
        body.get("card_id")
            .or_else(|| body.get("name"))
            .or_else(|| body.get("title"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("artwork"),
    );
    match kind.as_str() {
        "icon" | "ui" => format!("art/ui/{slug}.png"),
        "frame" => format!("art/frames/{slug}.png"),
        "store" | "thumbnail" => format!("art/store/{slug}.png"),
        "world" | "backdrop" => format!("art/worlds/{slug}.png"),
        "weapon" | "weapon_sheet" | "prop_sheet" => format!("art/weapons/{slug}.png"),
        _ => format!("art/cards/{slug}.png"),
    }
}

type ResolvedRefImage = (String, PathBuf, Vec<u8>, &'static str);

pub fn resolve_reference_images_for_test(root: &Path, body: &Value) -> ArtRes<Vec<String>> {
    resolve_reference_images(root, body).map(|v| v.into_iter().map(|r| r.0).collect())
}

pub fn default_out_path_for_test(body: &Value) -> String {
    default_out_path(body)
}

fn resolve_reference_images(root: &Path, body: &Value) -> ArtRes<Vec<ResolvedRefImage>> {
    let mut raw_paths: Vec<String> = Vec::new();
    let mut push_unique = |s: &str| {
        let t = s.trim();
        if !t.is_empty() && !raw_paths.iter().any(|p| p == t) {
            raw_paths.push(t.to_string());
        }
    };
    for arr_key in ["reference_images", "ref_images", "references", "weapon_sheets"] {
        if let Some(arr) = body.get(arr_key).and_then(Value::as_array) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    push_unique(s);
                }
            }
        }
    }
    for single_key in ["reference_image", "ref_image", "weapon_sheet"] {
        if let Some(single) = body.get(single_key).and_then(Value::as_str) {
            push_unique(single);
        }
    }
    if raw_paths.len() > 8 {
        return art_bad("At most 8 reference images are supported per generation.");
    }
    let mut out = Vec::with_capacity(raw_paths.len());
    for rel in raw_paths {
        let abs = safe_rel(root, &rel)?;
        if !abs.is_file() {
            return art_err(404, format!("Reference image '{rel}' not found in project."));
        }
        let bytes = std::fs::read(&abs)
            .map_err(|e| Oc { status: 500, msg: format!("read reference '{rel}': {e}") })?;
        let (mime, _) = detect_image_mime(&bytes).map_err(|e| Oc { status: 400, msg: format!("reference '{rel}': {e}") })?;
        out.push((rel, abs, bytes, mime));
    }
    Ok(out)
}

const ALLOWED_CHATGPT_SUBMIT_METHODS: [&str; 6] = [
    "keyboard-button",
    "button",
    "enter",
    "auto",
    "ctrl-enter",
    "meta-enter",
];

fn resolve_chatgpt_submit_method(body: &Value) -> ArtRes<String> {
    let raw = body
        .get("submit_method")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            std::env::var("OPENRO_CHATGPT_SUBMIT_METHOD")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "keyboard-button".to_string());
    let norm = raw.to_ascii_lowercase();
    if ALLOWED_CHATGPT_SUBMIT_METHODS.contains(&norm.as_str()) {
        Ok(norm)
    } else {
        art_bad(format!(
            "Invalid submit_method '{raw}'. Allowed values: {}.",
            ALLOWED_CHATGPT_SUBMIT_METHODS.join(", ")
        ))
    }
}

fn run_chatgpt_cli(
    prompt: &str,
    refs: &[PathBuf],
    staged_out: &Path,
    character_art: bool,
    timeout_s: u64,
    submit_method: &str,
) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    let custom_bin = std::env::var("OPENRO_CHATGPT_IMAGEGEN_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let custom_py = std::env::var("OPENRO_SHARED_BROWSER_PY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut cmd = if let Some(bin) = custom_bin {
        if let Some(py) = custom_py {
            let mut c = Command::new(py);
            c.arg(bin);
            c
        } else {
            Command::new(bin)
        }
    } else {
        let py = custom_py
            .map(PathBuf::from)
            .or_else(crate::tool_adapters::resolve_shared_browser_python)
            .ok_or_else(|| {
                "Shared browser Python not found. Use 'gemini' or 'openai' provider, configure a custom image adapter, or install shared-browser.".to_string()
            })?;
        if !py.is_file() {
            return Err(format!(
                "Shared browser Python not found at {}. Use 'gemini' or 'openai' provider, or install shared-browser.",
                py.display()
            ));
        }
        let driver = crate::tool_adapters::resolve_chatgpt_imagegen_bin().ok_or_else(|| {
            "ChatGPT image driver (chatgpt-camoufox-imagegen) not found via OPENRO_CHATGPT_IMAGEGEN_BIN, $HOME/.agents/skills/chatgpt-image-gen, or PATH.".to_string()
        })?;
        if !driver.is_file() {
            return Err(format!(
                "ChatGPT image driver not found at {}.",
                driver.display()
            ));
        }
        let mut c = Command::new(py);
        c.arg(driver);
        c
    };

    cmd.arg(prompt)
        .arg("-o")
        .arg(staged_out)
        .arg("--lease")
        .arg("openro")
        .arg("--timeout")
        .arg(timeout_s.to_string())
        .arg("--submit-method")
        .arg(submit_method)
        .arg("--json");

    if !refs.is_empty() {
        cmd.arg("--mode").arg("edit");
        for r in refs {
            cmd.arg("--ref-image").arg(r);
        }
    } else {
        cmd.arg("--mode").arg("generate");
    }
    if character_art {
        cmd.arg("--character-art");
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("failed to launch chatgpt-camoufox-imagegen: {e}"))?;
    let started = Instant::now();
    let hard_cap = Duration::from_secs(timeout_s.saturating_add(60));
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = child.wait_with_output().map_err(|e| format!("read imagegen output: {e}"))?;
                return Ok((out.status.code().unwrap_or(-1), out.stdout, out.stderr));
            }
            Ok(None) => {
                if started.elapsed() >= hard_cap {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("ChatGPT image generation exceeded hard timeout ({}s).", hard_cap.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("wait imagegen: {e}"));
            }
        }
    }
}

/// Reference-image art generation (`POST /api/art/generate`).
///
/// Default provider: `"chatgpt"` (`ChatGPT (shared browser)` via `chatgpt-camoufox-imagegen`).
/// Optional API providers: `"gemini"` (Nano Banana, 1-N inline reference images) and `"openai"`.
/// Never uses Higgsfield / Soul Cinema.
pub fn art_generate(store: &SecretStore, root: &Path, body: &Value) -> ArtRes<Value> {
    let raw_provider = body
        .get("provider")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "chatgpt".to_string());

    if raw_provider.contains("higgsfield") || raw_provider.contains("soul") {
        return art_bad("Higgsfield / Soul Cinema is prohibited. Use ChatGPT (shared browser), Gemini, or OpenAI.");
    }

    let raw_prompt = body.get("prompt").and_then(Value::as_str).unwrap_or("").trim();
    if raw_prompt.is_empty() {
        return art_bad("Send a non-empty 'prompt' describing the pose, composition, and style.");
    }

    let out_rel = default_out_path(body);
    if !out_rel.starts_with("art/") {
        return art_bad("out_path must stay under the project's art/ folder (e.g. art/cards/my_card.png).");
    }
    if out_rel.starts_with("art/references/") {
        return art_bad("Cannot overwrite owner reference files under art/references/; write generated art to art/cards/ or art/ui/.");
    }
    let out_abs = safe_rel(root, &out_rel)?;
    if let Some(parent) = out_abs.parent() {
        std::fs::create_dir_all(parent).or_else(|e| art_err(500, format!("create art dir: {e}")))?;
    }

    let mut refs = resolve_reference_images(root, body)?;
    let project_param = body.get("project").and_then(Value::as_str).unwrap_or("");
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("card");
    let is_weapon_sheet = matches!(
        kind.to_ascii_lowercase().as_str(),
        "weapon" | "weapon_sheet" | "prop_sheet"
    );

    // For weapon/prop sheets, automatically source a canonical isolated prop reference
    // into gitignored `art/references/` when `auto_reference: true` or when no reference
    // was provided and `subject` (or `weapon`/`prop`) is supplied (#76).
    let mut prop_reference_auto_sourced = false;
    let subject_opt = body
        .get("subject")
        .or_else(|| body.get("weapon"))
        .or_else(|| body.get("prop"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let explicit_auto_ref = body.get("auto_reference").and_then(Value::as_bool);
    let should_auto_source_prop = is_weapon_sheet
        && refs.is_empty()
        && (explicit_auto_ref == Some(true)
            || (explicit_auto_ref != Some(false) && subject_opt.is_some()));
    if should_auto_source_prop {
        let search_subject = subject_opt
            .or_else(|| {
                body.get("card_id")
                    .or_else(|| body.get("slug"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| {
                raw_prompt
                    .split([',', '.', ';', '\n'])
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(raw_prompt)
            });
        let mut search_req = json!({
            "kind": "prop",
            "subject": search_subject,
            "auto_pick": true,
        });
        if let Some(show) = body
            .get("show")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            search_req["show"] = json!(show);
        }
        if let Some(q) = body
            .get("qualifiers")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            search_req["qualifiers"] = json!(q);
        }
        if let Some(raw_slug) = body
            .get("reference_slug")
            .or_else(|| body.get("slug"))
            .or_else(|| body.get("card_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let clean = sanitize_art_slug(raw_slug);
            let prefixed = if clean.starts_with("weapon_") {
                clean
            } else {
                format!("weapon_{clean}")
            };
            search_req["slug"] = json!(prefixed);
        }
        if !project_param.is_empty() {
            search_req["project"] = json!(project_param);
        }
        let search_res = crate::reference_search::search(root, &search_req)?;
        let Some(ref_rel) = search_res
            .get("reference_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return art_err(
                502,
                "Auto prop reference search completed without returning a reference_path.",
            );
        };
        let ref_abs = safe_rel(root, ref_rel)?;
        let bytes = std::fs::read(&ref_abs)
            .or_else(|e| art_err(500, format!("read auto-sourced prop reference '{ref_rel}': {e}")))?;
        let (mime, _) = detect_image_mime(&bytes).map_err(|e| Oc { status: 502, msg: e })?;
        refs.push((ref_rel.to_string(), ref_abs, bytes, mime));
        prop_reference_auto_sourced = true;
    }

    let ref_rels: Vec<String> = refs.iter().map(|r| r.0.clone()).collect();
    let character_art = body
        .get("character_art")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| !is_weapon_sheet && (kind.eq_ignore_ascii_case("card") || !refs.is_empty()));

    let finalized_non_character = |raw: &str| -> String {
        if is_weapon_sheet {
            ensure_weapon_sheet_prompt(raw)
        } else {
            ensure_roblox_safe_prompt(&collapse_one_line(raw))
        }
    };

    match raw_provider.as_str() {
        "chatgpt" | "chatgpt-shared-browser" | "chatgpt_shared_browser" | "auto" => {
            let prompt = if character_art {
                let p = prepare_character_prompt(raw_prompt).map_err(|e| Oc { status: 400, msg: e })?;
                ensure_roblox_safe_prompt(&p)
            } else {
                finalized_non_character(raw_prompt)
            };
            let timeout_s = body
                .get("timeout_s")
                .and_then(Value::as_u64)
                .unwrap_or(420)
                .clamp(15, 600);
            let submit_method = resolve_chatgpt_submit_method(body)?;

            // Serialize shared-browser invocations inside OpenRO and allocate a fresh unused -o path.
            let lock = CHATGPT_SERIAL_LOCK.get_or_init(|| Mutex::new(()));
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

            let parent = out_abs.parent().unwrap_or(root);
            let stem = out_abs
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("art");
            let staged_out = parent.join(format!(".openro_gen_{stem}_{}.png", now_ms()));
            let ref_paths: Vec<PathBuf> = refs.iter().map(|r| r.1.clone()).collect();

            let (code, stdout, stderr) = run_chatgpt_cli(
                &prompt,
                &ref_paths,
                &staged_out,
                character_art,
                timeout_s,
                &submit_method,
            )
            .map_err(|m| Oc { status: 503, msg: m })?;
            let cli_json: Value = serde_json::from_slice(&stdout).unwrap_or_else(|_| {
                let err_txt = String::from_utf8_lossy(&stderr);
                json!({
                    "ok": false,
                    "exit_code": code,
                    "error_kind": "DRIVER",
                    "error": err_txt.trim(),
                })
            });

            let err_detail = cli_json
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();

            match code {
                0 => {
                    if !staged_out.is_file() {
                        return art_err(502, "ChatGPT driver reported success but did not write the PNG file.");
                    }
                    let bytes = std::fs::read(&staged_out)
                        .or_else(|e| art_err(500, format!("read generated PNG: {e}")))?;
                    let _ = detect_image_mime(&bytes).map_err(|e| Oc { status: 502, msg: e })?;
                    std::fs::rename(&staged_out, &out_abs)
                        .or_else(|e| art_err(500, format!("save generated PNG: {e}")))?;
                    let (w, h) = image_dimensions(&bytes).unwrap_or((
                        cli_json.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                        cli_json.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    ));
                    let sha = sha256_hex(&bytes);
                    let meta_rel = meta_path_for(&out_rel);
                    let mut meta = json!({
                        "version": 1,
                        "status": "generated",
                        "provider": "chatgpt",
                        "model": cli_json.get("model_ui").cloned().unwrap_or_else(|| json!("chatgpt-shared-browser")),
                        "kind": kind,
                        "prompt": prompt,
                        "reference_images": ref_rels,
                        "path": out_rel,
                        "sha256": sha,
                        "bytes": bytes.len(),
                        "width": w,
                        "height": h,
                        "created_at": now_iso(),
                    });
                    if prop_reference_auto_sourced {
                        meta["prop_reference_auto_sourced"] = json!(true);
                    }
                    write_meta_atomic(&safe_rel(root, &meta_rel)?, &meta)?;
                    let _ = append_art_log(root, &json!({
                        "at": now_iso(),
                        "action": "generate",
                        "status": "generated",
                        "provider": "chatgpt",
                        "path": out_rel,
                        "sha256": sha,
                        "width": w,
                        "height": h,
                        "bytes": bytes.len(),
                        "reference_images": ref_rels,
                        "prop_reference_auto_sourced": prop_reference_auto_sourced,
                    }));
                    let mut reply = json!({
                        "ok": true,
                        "status": "generated",
                        "pending_review": false,
                        "refused": false,
                        "path": out_rel,
                        "url": art_image_url(project_param, &out_rel),
                        "meta_path": meta_rel,
                        "provider": "chatgpt",
                        "model": meta["model"],
                        "sha256": sha,
                        "width": w,
                        "height": h,
                        "bytes": bytes.len(),
                        "reference_images": ref_rels,
                        "metadata": meta,
                    });
                    if prop_reference_auto_sourced {
                        reply["prop_reference_auto_sourced"] = json!(true);
                    }
                    Ok(reply)
                }
                9 => {
                    // Exit 9 = ANGLE_REVIEW_REQUIRED: real verified PNG staged pending independent pixel review.
                    let _ = std::fs::remove_file(&staged_out);
                    let staged_cand = cli_json
                        .pointer("/angle_review/candidate_path")
                        .and_then(Value::as_str)
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(format!("{}.pending-angle.png", staged_out.display())));
                    if !staged_cand.is_file() {
                        return art_err(502, "ChatGPT driver exited 9 (ANGLE_REVIEW_REQUIRED) but staged candidate PNG is missing.");
                    }
                    let bytes = std::fs::read(&staged_cand)
                        .or_else(|e| art_err(500, format!("read staged candidate: {e}")))?;
                    let _ = detect_image_mime(&bytes).map_err(|e| Oc { status: 502, msg: e })?;
                    let sha = sha256_hex(&bytes);
                    let short_sha = &sha[..12.min(sha.len())];
                    let out_stem = out_rel.rsplit_once('.').map(|(s, _)| s).unwrap_or(&out_rel);
                    let cand_rel = format!("{out_stem}.pending-angle.{short_sha}.png");
                    let cand_abs = safe_rel(root, &cand_rel)?;
                    if staged_cand != cand_abs {
                        std::fs::rename(&staged_cand, &cand_abs)
                            .or_else(|e| art_err(500, format!("move staged candidate: {e}")))?;
                    }
                    let (w, h) = image_dimensions(&bytes).unwrap_or((
                        cli_json.pointer("/angle_review/width").and_then(Value::as_u64).unwrap_or(0) as u32,
                        cli_json.pointer("/angle_review/height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    ));
                    let cand_meta_rel = meta_path_for(&cand_rel);
                    let meta = json!({
                        "version": 1,
                        "status": "pending_review",
                        "provider": "chatgpt",
                        "model": cli_json.get("model_ui").cloned().unwrap_or_else(|| json!("chatgpt-shared-browser")),
                        "kind": kind,
                        "prompt": prompt,
                        "reference_images": ref_rels,
                        "requested_path": out_rel,
                        "candidate_path": cand_rel,
                        "candidate_sha256": sha,
                        "bytes": bytes.len(),
                        "width": w,
                        "height": h,
                        "angle_review": {
                            "status": "PENDING_VISION",
                            "candidate_path": cand_rel,
                            "candidate_sha256": sha,
                            "width": w,
                            "height": h,
                        },
                        "created_at": now_iso(),
                    });
                    write_meta_atomic(&safe_rel(root, &cand_meta_rel)?, &meta)?;
                    let _ = append_art_log(root, &json!({
                        "at": now_iso(),
                        "action": "generate",
                        "status": "pending_review",
                        "provider": "chatgpt",
                        "candidate_path": cand_rel,
                        "requested_path": out_rel,
                        "candidate_sha256": sha,
                        "width": w,
                        "height": h,
                        "bytes": bytes.len(),
                        "reference_images": ref_rels,
                    }));
                    let ref_urls: Vec<String> = ref_rels.iter().map(|r| art_image_url(project_param, r)).collect();
                    Ok(json!({
                        "ok": true,
                        "status": "pending_review",
                        "pending_review": true,
                        "refused": false,
                        "candidate_path": cand_rel,
                        "candidate_url": art_image_url(project_param, &cand_rel),
                        "candidate_sha256": sha,
                        "requested_path": out_rel,
                        "meta_path": cand_meta_rel,
                        "provider": "chatgpt",
                        "model": meta["model"],
                        "width": w,
                        "height": h,
                        "bytes": bytes.len(),
                        "reference_images": ref_rels,
                        "reference_urls": ref_urls,
                        "angle_review": meta["angle_review"],
                        "note": "Verified character candidate staged for pixel review; inspect candidate vs reference and call POST /api/art/approve to promote.",
                    }))
                }
                7 => {
                    let _ = std::fs::remove_file(&staged_out);
                    let reason = if err_detail.is_empty() {
                        "ChatGPT refused the prompt or returned no image.".to_string()
                    } else {
                        err_detail
                    };
                    let _ = append_art_log(root, &json!({
                        "at": now_iso(),
                        "action": "generate",
                        "status": "refused",
                        "provider": "chatgpt",
                        "requested_path": out_rel,
                        "prompt": prompt,
                        "moderation_reason": reason,
                    }));
                    Ok(json!({
                        "ok": false,
                        "refused": true,
                        "status": "refused",
                        "error_kind": "REFUSED",
                        "provider": "chatgpt",
                        "requested_path": out_rel,
                        "moderation_reason": reason,
                        "error": format!("ChatGPT did not produce an image: {reason}"),
                    }))
                }
                2 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(503, "ChatGPT is logged out in the shared Camoufox browser. Log into chatgpt.com once in the visible window, then retry.")
                }
                3 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_bad(if err_detail.is_empty() {
                        "Invalid image generation arguments or camera angle preflight failed.".to_string()
                    } else {
                        err_detail
                    })
                }
                4 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(502, format!("ChatGPT page layout changed (UI_DRIFT) or non-image model tier selected: {err_detail}"))
                }
                5 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(504, format!("ChatGPT timed out before returning an image: {err_detail}"))
                }
                6 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(429, format!("ChatGPT rate limit reached. Wait a moment and retry. {err_detail}"))
                }
                8 => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(503, format!("Shared Camoufox browser is busy or unreachable on port 9230: {err_detail}"))
                }
                other => {
                    let _ = std::fs::remove_file(&staged_out);
                    art_err(502, format!("ChatGPT image driver failed (exit {other}): {err_detail}"))
                }
            }
        }
        "gemini" | "gemini-api" | "nano-banana" | "google-nano-banana" => {
            let safe_prompt = finalized_non_character(raw_prompt);
            art_generate_gemini(
                store,
                root,
                project_param,
                kind,
                &safe_prompt,
                &out_rel,
                &out_abs,
                &refs,
                body,
                prop_reference_auto_sourced,
            )
        }
        "openai" | "openai-image" => {
            let safe_prompt = finalized_non_character(raw_prompt);
            art_generate_openai(
                store,
                root,
                project_param,
                kind,
                &safe_prompt,
                &out_rel,
                &out_abs,
                &refs,
                body,
                prop_reference_auto_sourced,
            )
        }
        other => art_bad(format!(
            "Unsupported image provider '{other}': expected 'chatgpt' (default), 'gemini', or 'openai'."
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn art_generate_gemini(
    store: &SecretStore,
    root: &Path,
    project_param: &str,
    kind: &str,
    prompt: &str,
    out_rel: &str,
    out_abs: &Path,
    refs: &[ResolvedRefImage],
    body: &Value,
    prop_reference_auto_sourced: bool,
) -> ArtRes<Value> {
    let api_key = store
        .get(GEMINI_KEY)
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .ok_or_else(|| Oc {
            status: 503,
            msg: "Add your Gemini API key in Settings first (or use the default ChatGPT shared browser provider).".into(),
        })?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty() && !m.starts_with("gemini-nano-banana"))
        .map(String::from)
        .or_else(|| std::env::var("OPENRO_GEMINI_IMAGE_MODEL").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| DEFAULT_GEMINI_IMAGE_MODEL.to_string());
    let base = body
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| std::env::var("OPENRO_GEMINI_IMAGE_BASE").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());

    let mut parts: Vec<Value> = Vec::with_capacity(refs.len() + 1);
    for (_, _, bytes, mime) in refs {
        parts.push(json!({
            "inlineData": {
                "mimeType": mime,
                "data": encode_base64(bytes),
            }
        }));
    }
    parts.push(json!({ "text": prompt }));

    let mut gen_cfg = json!({ "responseModalities": ["IMAGE"] });
    if let Some(ar) = body.get("aspect_ratio").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        gen_cfg["imageConfig"] = json!({ "aspectRatio": ar.trim() });
    }
    let req_body = json!({
        "contents": [{ "parts": parts }],
        "generationConfig": gen_cfg,
    });
    let url = format!("{}/v1beta/models/{model}:generateContent?key={api_key}", base.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(90)).build();
    let res = agent.post(&url).set("Content-Type", "application/json").send_json(req_body);
    let (status, resp_bytes) = match res {
        Ok(r) => {
            let st = r.status();
            let mut b = Vec::new();
            let _ = std::io::Read::take(r.into_reader(), 32 * 1024 * 1024).read_to_end(&mut b);
            (st, b)
        }
        Err(ureq::Error::Status(code, r)) => {
            let mut b = Vec::new();
            let _ = std::io::Read::take(r.into_reader(), 1024 * 1024).read_to_end(&mut b);
            (code, b)
        }
        Err(ureq::Error::Transport(t)) => {
            return art_err(502, secrets::scrub(&format!("Gemini image request failed: {t}"), &[&api_key]));
        }
    };
    if !(200..300).contains(&status) {
        let txt = String::from_utf8_lossy(&resp_bytes);
        return art_err(502, secrets::scrub(&format!("Gemini image API answered {status}: {}", txt.chars().take(240).collect::<String>()), &[&api_key]));
    }
    let payload: Value = serde_json::from_slice(&resp_bytes)
        .or_else(|_| art_err(502, "Gemini image API returned invalid JSON."))?;

    let ref_rels: Vec<String> = refs.iter().map(|r| r.0.clone()).collect();
    // Structured moderation / safety refusal check
    let block_reason = payload
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/candidates/0/finishReason")
                .and_then(Value::as_str)
                .filter(|r| matches!(*r, "SAFETY" | "IMAGE_SAFETY" | "PROHIBITED_CONTENT" | "RECITATION" | "BLOCKLIST"))
        });
    if let Some(reason) = block_reason {
        let _ = append_art_log(root, &json!({
            "at": now_iso(),
            "action": "generate",
            "status": "refused",
            "provider": "gemini",
            "model": model,
            "requested_path": out_rel,
            "moderation_reason": reason,
        }));
        return Ok(json!({
            "ok": false,
            "refused": true,
            "status": "refused",
            "error_kind": "REFUSED",
            "provider": "gemini",
            "requested_path": out_rel,
            "moderation_reason": reason,
            "error": format!("Gemini blocked the image generation ({reason}). Adjust the prompt and retry."),
        }));
    }

    let Some((b64, _ext)) = extract_gemini_inline_image(&payload) else {
        let text_reply = payload
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            .unwrap_or("no inline image returned");
        let _ = append_art_log(root, &json!({
            "at": now_iso(),
            "action": "generate",
            "status": "refused",
            "provider": "gemini",
            "requested_path": out_rel,
            "moderation_reason": text_reply,
        }));
        return Ok(json!({
            "ok": false,
            "refused": true,
            "status": "refused",
            "error_kind": "REFUSED",
            "provider": "gemini",
            "requested_path": out_rel,
            "moderation_reason": text_reply,
            "error": format!("Gemini returned no image: {text_reply}"),
        }));
    };

    let bytes = decode_base64(&b64).map_err(|e| Oc { status: 502, msg: e })?;
    let _ = detect_image_mime(&bytes).map_err(|e| Oc { status: 502, msg: e })?;
    let tmp = out_abs.with_extension("img.tmp-openro");
    std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::rename(&tmp, out_abs))
        .or_else(|e| art_err(500, format!("save image: {e}")))?;

    let (w, h) = image_dimensions(&bytes).unwrap_or((0, 0));
    let sha = sha256_hex(&bytes);
    let meta_rel = meta_path_for(out_rel);
    let mut meta = json!({
        "version": 1,
        "status": "generated",
        "provider": "gemini",
        "model": model,
        "kind": kind,
        "prompt": prompt,
        "reference_images": ref_rels,
        "path": out_rel,
        "sha256": sha,
        "bytes": bytes.len(),
        "width": w,
        "height": h,
        "created_at": now_iso(),
    });
    if prop_reference_auto_sourced {
        meta["prop_reference_auto_sourced"] = json!(true);
    }
    write_meta_atomic(&safe_rel(root, &meta_rel)?, &meta)?;
    let _ = append_art_log(root, &json!({
        "at": now_iso(),
        "action": "generate",
        "status": "generated",
        "provider": "gemini",
        "model": model,
        "path": out_rel,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reference_images": ref_rels,
        "prop_reference_auto_sourced": prop_reference_auto_sourced,
    }));

    let mut reply = json!({
        "ok": true,
        "status": "generated",
        "pending_review": false,
        "refused": false,
        "path": out_rel,
        "url": art_image_url(project_param, out_rel),
        "meta_path": meta_rel,
        "provider": "gemini",
        "model": model,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reference_images": ref_rels,
        "metadata": meta,
    });
    if prop_reference_auto_sourced {
        reply["prop_reference_auto_sourced"] = json!(true);
    }
    Ok(reply)
}

#[allow(clippy::too_many_arguments)]
fn art_generate_openai(
    store: &SecretStore,
    root: &Path,
    project_param: &str,
    kind: &str,
    prompt: &str,
    out_rel: &str,
    out_abs: &Path,
    refs: &[ResolvedRefImage],
    body: &Value,
    prop_reference_auto_sourced: bool,
) -> ArtRes<Value> {
    let api_key = store
        .get(OPENAI_IMAGE_KEY)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .ok_or_else(|| Oc {
            status: 503,
            msg: "Add your OpenAI image key in Settings first (or use the default ChatGPT shared browser provider).".into(),
        })?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_OPENAI_IMAGE_MODEL);
    let size = body.get("size").and_then(Value::as_str).unwrap_or("1024x1536");
    let base = body
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| std::env::var("OPENRO_OPENAI_IMAGE_BASE").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(90)).build();
    let res = if refs.is_empty() {
        let url = format!("{}/images/generations", base.trim_end_matches('/'));
        agent
            .post(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_json(json!({
                "model": model,
                "prompt": prompt,
                "size": size,
                "response_format": "b64_json",
            }))
    } else {
        let url = format!("{}/images/edits", base.trim_end_matches('/'));
        let mut mp = Multipart::new();
        mp.text("model", model);
        mp.text("prompt", prompt);
        mp.text("size", size);
        mp.text("response_format", "b64_json");
        for (rel, _, bytes, mime) in refs {
            let fname = rel.rsplit('/').next().unwrap_or("ref.png");
            mp.file("image[]", fname, mime, bytes);
        }
        let (ct, body_bytes) = mp.finish();
        agent
            .post(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", &ct)
            .send_bytes(&body_bytes)
    };

    let (status, resp_bytes) = match res {
        Ok(r) => {
            let st = r.status();
            let mut b = Vec::new();
            let _ = std::io::Read::take(r.into_reader(), 32 * 1024 * 1024).read_to_end(&mut b);
            (st, b)
        }
        Err(ureq::Error::Status(code, r)) => {
            let mut b = Vec::new();
            let _ = std::io::Read::take(r.into_reader(), 1024 * 1024).read_to_end(&mut b);
            (code, b)
        }
        Err(ureq::Error::Transport(t)) => {
            return art_err(502, secrets::scrub(&format!("OpenAI image request failed: {t}"), &[&api_key]));
        }
    };
    let ref_rels: Vec<String> = refs.iter().map(|r| r.0.clone()).collect();
    if !(200..300).contains(&status) {
        let txt = String::from_utf8_lossy(&resp_bytes);
        if txt.to_ascii_lowercase().contains("content_policy") || txt.to_ascii_lowercase().contains("moderation") || txt.to_ascii_lowercase().contains("safety") {
            let reason = secrets::scrub(&txt.chars().take(240).collect::<String>(), &[&api_key]);
            let _ = append_art_log(root, &json!({
                "at": now_iso(),
                "action": "generate",
                "status": "refused",
                "provider": "openai",
                "requested_path": out_rel,
                "moderation_reason": reason,
            }));
            return Ok(json!({
                "ok": false,
                "refused": true,
                "status": "refused",
                "error_kind": "REFUSED",
                "provider": "openai",
                "requested_path": out_rel,
                "moderation_reason": reason,
                "error": format!("OpenAI blocked the image prompt: {reason}"),
            }));
        }
        return art_err(502, secrets::scrub(&format!("OpenAI image API answered {status}: {}", txt.chars().take(240).collect::<String>()), &[&api_key]));
    }
    let payload: Value = serde_json::from_slice(&resp_bytes)
        .or_else(|_| art_err(502, "OpenAI image API returned invalid JSON."))?;
    let Some(b64) = payload.pointer("/data/0/b64_json").and_then(Value::as_str) else {
        return art_err(502, "OpenAI image API returned no b64_json image data.");
    };
    let bytes = decode_base64(b64).map_err(|e| Oc { status: 502, msg: e })?;
    let _ = detect_image_mime(&bytes).map_err(|e| Oc { status: 502, msg: e })?;
    let tmp = out_abs.with_extension("img.tmp-openro");
    std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::rename(&tmp, out_abs))
        .or_else(|e| art_err(500, format!("save image: {e}")))?;

    let (w, h) = image_dimensions(&bytes).unwrap_or((0, 0));
    let sha = sha256_hex(&bytes);
    let meta_rel = meta_path_for(out_rel);
    let mut meta = json!({
        "version": 1,
        "status": "generated",
        "provider": "openai",
        "model": model,
        "kind": kind,
        "prompt": prompt,
        "reference_images": ref_rels,
        "path": out_rel,
        "sha256": sha,
        "bytes": bytes.len(),
        "width": w,
        "height": h,
        "created_at": now_iso(),
    });
    if prop_reference_auto_sourced {
        meta["prop_reference_auto_sourced"] = json!(true);
    }
    write_meta_atomic(&safe_rel(root, &meta_rel)?, &meta)?;
    let _ = append_art_log(root, &json!({
        "at": now_iso(),
        "action": "generate",
        "status": "generated",
        "provider": "openai",
        "model": model,
        "path": out_rel,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reference_images": ref_rels,
        "prop_reference_auto_sourced": prop_reference_auto_sourced,
    }));

    let mut reply = json!({
        "ok": true,
        "status": "generated",
        "pending_review": false,
        "refused": false,
        "path": out_rel,
        "url": art_image_url(project_param, out_rel),
        "meta_path": meta_rel,
        "provider": "openai",
        "model": model,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reference_images": ref_rels,
        "metadata": meta,
    });
    if prop_reference_auto_sourced {
        reply["prop_reference_auto_sourced"] = json!(true);
    }
    Ok(reply)
}

/// `POST /api/art/approve` — promote a reviewed `.pending-angle.*.png` candidate to its final path
/// (or record a `redo` verdict without promoting). Never skips pixel review provenance.
pub fn art_approve(root: &Path, body: &Value) -> ArtRes<Value> {
    let cand_rel = body
        .get("candidate_path")
        .or_else(|| body.get("pending_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc { status: 400, msg: "Send 'candidate_path' and 'reviewer'.".into() })?;
    let reviewer = body
        .get("reviewer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc { status: 400, msg: "Send 'reviewer' (who inspected the pixels, e.g. 'owner' or 'vision-critic').".into() })?;
    let verdict = body
        .get("verdict")
        .and_then(Value::as_str)
        .unwrap_or("pass")
        .to_ascii_lowercase();
    let notes = body.get("notes").and_then(Value::as_str).unwrap_or("");
    let project_param = body.get("project").and_then(Value::as_str).unwrap_or("");

    let cand_abs = safe_rel(root, cand_rel)?;
    if !cand_abs.is_file() {
        return art_err(404, format!("Candidate image '{cand_rel}' not found."));
    }
    let bytes = std::fs::read(&cand_abs)
        .or_else(|e| art_err(500, format!("read candidate: {e}")))?;
    let _ = detect_image_mime(&bytes).map_err(|e| Oc { status: 400, msg: e })?;
    let sha = sha256_hex(&bytes);

    if let Some(expected) = body.get("expected_sha256").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        if !expected.eq_ignore_ascii_case(&sha) {
            return art_err(409, format!("Candidate SHA-256 mismatch: expected {expected}, actual {sha}."));
        }
    }

    if verdict == "redo" || verdict == "reject" {
        let _ = append_art_log(root, &json!({
            "at": now_iso(),
            "action": "angle_review",
            "verdict": "redo",
            "candidate_path": cand_rel,
            "sha256": sha,
            "reviewer": reviewer,
            "notes": notes,
        }));
        return Ok(json!({
            "ok": true,
            "status": "redo",
            "candidate_path": cand_rel,
            "sha256": sha,
            "reviewer": reviewer,
        }));
    }

    let cand_meta_abs = safe_rel(root, &meta_path_for(cand_rel))?;
    let cand_meta: Value = std::fs::read_to_string(&cand_meta_abs)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));

    let target_rel = body
        .get("target_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| cand_meta.get("requested_path").and_then(Value::as_str).map(String::from))
        .ok_or_else(|| Oc { status: 400, msg: "Send 'target_path' (destination path under art/).".into() })?;
    if !target_rel.starts_with("art/") || target_rel.starts_with("art/references/") {
        return art_bad("target_path must be under art/ (and not under art/references/).");
    }
    let target_abs = safe_rel(root, &target_rel)?;
    if let Some(p) = target_abs.parent() {
        std::fs::create_dir_all(p).or_else(|e| art_err(500, format!("create target dir: {e}")))?;
    }
    let tmp = target_abs.with_extension("approved.tmp-openro");
    std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::rename(&tmp, &target_abs))
        .or_else(|e| art_err(500, format!("promote candidate: {e}")))?;

    let (w, h) = image_dimensions(&bytes).unwrap_or((0, 0));
    let mut final_meta = cand_meta;
    final_meta["status"] = json!("approved");
    final_meta["path"] = json!(target_rel);
    final_meta["sha256"] = json!(sha);
    final_meta["bytes"] = json!(bytes.len());
    final_meta["width"] = json!(w);
    final_meta["height"] = json!(h);
    final_meta["approved_by"] = json!(reviewer);
    final_meta["approved_at"] = json!(now_iso());
    final_meta["review_notes"] = json!(notes);
    final_meta["promoted_from"] = json!(cand_rel);
    let target_meta_rel = meta_path_for(&target_rel);
    write_meta_atomic(&safe_rel(root, &target_meta_rel)?, &final_meta)?;

    let _ = append_art_log(root, &json!({
        "at": now_iso(),
        "action": "angle_review",
        "verdict": "pass",
        "path": target_rel,
        "promoted_from": cand_rel,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reviewer": reviewer,
        "notes": notes,
    }));

    Ok(json!({
        "ok": true,
        "status": "approved",
        "path": target_rel,
        "url": art_image_url(project_param, &target_rel),
        "meta_path": target_meta_rel,
        "sha256": sha,
        "width": w,
        "height": h,
        "bytes": bytes.len(),
        "reviewer": reviewer,
    }))
}

/// `GET /api/art` — list generated & pending art metadata under `<root>/art/` plus recent `ART-LOG.jsonl` rows.
pub fn art_list(root: &Path) -> ArtRes<Value> {
    let art_dir = root.join("art");
    let mut items = Vec::new();
    let mut pending = Vec::new();
    if art_dir.is_dir() {
        let mut stack = vec![(art_dir.clone(), "art".to_string(), 0u8)];
        while let Some((dir, rel, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().to_string();
                let child_rel = format!("{rel}/{name}");
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    if depth < 6 && name != "references" && !name.starts_with('.') {
                        stack.push((e.path(), child_rel, depth + 1));
                    }
                } else if ft.is_file() && name.ends_with(".meta.json") {
                    if let Ok(t) = std::fs::read_to_string(e.path()) {
                        if let Ok(v) = serde_json::from_str::<Value>(&t) {
                            if v.get("status").and_then(Value::as_str) == Some("pending_review") {
                                pending.push(v);
                            } else {
                                items.push(v);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut recent_log = Vec::new();
    if let Ok(text) = std::fs::read_to_string(art_dir.join("ART-LOG.jsonl")) {
        let lines: Vec<&str> = text.lines().collect();
        for l in lines.iter().rev().take(50).rev() {
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                recent_log.push(v);
            }
        }
    }
    Ok(json!({
        "ok": true,
        "count": items.len(),
        "items": items,
        "pending_review": pending,
        "log": recent_log,
    }))
}

/// `GET /api/art/image?path=<rel>` — safely read an image inside `<root>/art/`.
pub fn art_image(root: &Path, rel: &str) -> ArtRes<(&'static str, Vec<u8>)> {
    let rel = rel.trim();
    if !rel.starts_with("art/") {
        return art_bad("Image path must start with art/.");
    }
    let abs = safe_rel(root, rel)?;
    let meta = std::fs::metadata(&abs)
        .map_err(|_| Oc { status: 404, msg: format!("Image '{rel}' not found.") })?;
    if !meta.is_file() || meta.len() > 20 * 1024 * 1024 {
        return art_bad("Invalid image file.");
    }
    let bytes = std::fs::read(&abs).or_else(|e| art_err(500, format!("read image: {e}")))?;
    let (mime, _) = detect_image_mime(&bytes).map_err(|e| Oc { status: 400, msg: e })?;
    Ok((mime, bytes))
}
