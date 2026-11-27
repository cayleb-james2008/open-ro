//! Free CC0-1.0 SFX Pack Library, Zero-Dependency Safe ZIP Extractor,
//! 44.1 kHz PCM16 Mono WAV Mastering, and Local Neural Audio Runner Installer
//! for OpenRO v1.33.0 (Gap #88).

use crate::audio_synth::{audio_assets_dir, sanitize_audio_filename, SAMPLE_RATE};
use crate::market_taste::validate_public_url;
use flate2::read::DeflateDecoder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_ZIP_ENTRIES: usize = 1_000;
pub const MAX_ZIP_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
pub const MAX_ZIP_SINGLE_ENTRY_BYTES: usize = 32 * 1024 * 1024; // 32 MiB
pub const MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES: usize = 150 * 1024 * 1024; // 150 MiB
const MAX_LOCAL_WALK_DEPTH: usize = 16;
const MAX_LAYER_CLIPS: usize = 8;

/// Static catalog entry for a verified 100% free CC0-1.0 SFX pack.
#[derive(Clone, Debug)]
pub struct Cc0PackSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub author: &'static str,
    pub license: &'static str,
    pub url: &'static str,
    pub description: &'static str,
    pub tags: &'static [&'static str],
    pub estimated_clips: usize,
}

static CC0_PACK_CATALOG: [Cc0PackSpec; 7] = [
    Cc0PackSpec {
        id: "kenney-ui-sfx",
        name: "Kenney UI Sound Effects (51 clips)",
        author: "Kenney",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/UI_SFX_Set.zip",
        description: "Crisp menu clicks, hovers, switches, toggles, modal swooshes, and interface feedback cues.",
        tags: &["ui", "click", "hover", "button", "menu", "switch", "interface"],
        estimated_clips: 51,
    },
    Cc0PackSpec {
        id: "kenney-casino-cards",
        name: "Kenney Card & Casino Audio (54 clips)",
        author: "Kenney",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/kenney_casino-audio.zip",
        description: "Card flips, shuffles, slides, deals, chip clinks, and tabletop reward clicks for card & pack games.",
        tags: &["card", "cards", "flip", "shuffle", "deal", "chips", "pack", "gacha", "casino"],
        estimated_clips: 54,
    },
    Cc0PackSpec {
        id: "kenney-digital-sfx",
        name: "Kenney Digital & Sci-Fi SFX (63 clips)",
        author: "Kenney",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/Digital_SFX_Set.zip",
        description: "Digital alerts, laser zaps, shield hums, power-ups, phase shifts, and arcade sci-fi cues.",
        tags: &["digital", "scifi", "laser", "powerup", "zap", "phaser", "arcade", "alert"],
        estimated_clips: 63,
    },
    Cc0PackSpec {
        id: "rpg-combat-pack",
        name: "RPG Combat & Foley Pack",
        author: "Artisticdude",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/rpg_sound_pack.zip",
        description: "Sword swings, heavy hits, armor clanks, footsteps, potions, inventory rustles, and combat foley.",
        tags: &["rpg", "combat", "sword", "hit", "foley", "potion", "armor", "inventory"],
        estimated_clips: 50,
    },
    Cc0PackSpec {
        id: "rpg-fantasy-80",
        name: "80 CC0 RPG Fantasy SFX",
        author: "Rubberduck",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/80-CC0-RPG-SFX_0.zip",
        description: "Spells, magical chimes, elemental casts, treasure chests, doors, crits, and fantasy adventure foley.",
        tags: &["rpg", "fantasy", "magic", "spell", "chest", "chime", "crit", "door"],
        estimated_clips: 80,
    },
    Cc0PackSpec {
        id: "general-game-100",
        name: "100 CC0 Universal Game SFX",
        author: "Rubberduck",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/100-CC0-SFX_0.zip",
        description: "Broad universal game sound effects covering impacts, pickups, jumps, explosions, swooshes, and notifications.",
        tags: &["universal", "game", "impact", "pickup", "jump", "explosion", "whoosh", "sfx"],
        estimated_clips: 100,
    },
    Cc0PackSpec {
        id: "arcade-fanfare-pack",
        name: "Level Up, Power Up & Coin Fanfares (13 clips)",
        author: "Wobbleboxx",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/SoundPack01.zip",
        description: "Punchy level-up jingles, coin pickups, victory fanfares, and game-over stingers.",
        tags: &["arcade", "fanfare", "levelup", "powerup", "coin", "victory", "jingle"],
        estimated_clips: 13,
    },
];

/// Verified 100% Free CC0-1.0 SFX pack catalog (7 packs).
pub fn cc0_pack_catalog() -> &'static [Cc0PackSpec] {
    &CC0_PACK_CATALOG
}

pub fn find_catalog_pack(id: &str) -> Option<&'static Cc0PackSpec> {
    let trimmed = id.trim();
    CC0_PACK_CATALOG.iter().find(|p| p.id == trimmed)
}

pub fn sfx_library_root(data_dir: &Path) -> PathBuf {
    data_dir.join("library").join("cc0-sfx")
}

pub fn local_audio_runner_path(data_dir: &Path) -> PathBuf {
    data_dir.join("bin").join("openro-audio-gen")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn allow_loopback_packs() -> bool {
    std::env::var("OPENRO_SFX_PACK_ALLOW_LOOPBACK")
        .ok()
        .as_deref()
        == Some("1")
}

/// Strictly validate a pack identifier (`1..=64` ASCII lowercase alphanumeric, `-`, `_`).
pub fn validate_pack_id(raw: &str) -> Result<String, String> {
    let id = raw.trim().to_ascii_lowercase();
    let ok = !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(id)
    } else {
        Err(format!(
            "invalid sfx pack id '{raw}' (use 1-64 lowercase letters, digits, '-' or '_')"
        ))
    }
}

fn is_allowed_audio_ext(ext: &str) -> bool {
    matches!(ext, "wav" | "ogg" | "mp3" | "flac" | "aiff")
}

fn is_allowed_license_ext(ext: &str) -> bool {
    matches!(ext, "txt" | "md")
}

fn audio_mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        "mp3" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        "aiff" => Some("audio/aiff"),
        _ => None,
    }
}

fn file_ext_lower(path_str: &str) -> Option<String> {
    let fname = path_str.rsplit('/').next()?;
    let dot = fname.rfind('.')?;
    if dot == 0 || dot + 1 >= fname.len() {
        return None;
    }
    Some(fname[dot + 1..].to_ascii_lowercase())
}

/// Strictly validate a relative archive/clip path against traversal, backslashes,
/// drive letters, NUL bytes, leading slashes, and control characters.
pub fn validate_rel_archive_path(raw: &str, allow_trailing_slash: bool) -> Result<String, String> {
    if raw.is_empty() {
        return Err("empty entry path is forbidden".to_string());
    }
    if raw.contains('\0') {
        return Err("entry path contains null byte".to_string());
    }
    if raw.contains('\\') {
        return Err(format!("entry path '{raw}' contains backslash"));
    }
    if raw.starts_with('/') {
        return Err(format!("entry path '{raw}' must not start with '/'"));
    }
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(format!("entry path '{raw}' contains control characters"));
    }
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(format!(
            "entry path '{raw}' contains Windows drive letter prefix"
        ));
    }
    let trimmed = if allow_trailing_slash {
        raw.strip_suffix('/').unwrap_or(raw)
    } else {
        raw
    };
    if trimmed.is_empty() {
        return Err("entry path resolves to empty".to_string());
    }
    let mut clean_parts = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(format!("entry path '{raw}' contains forbidden segment '{seg}'"));
        }
        if seg.contains(':') {
            return Err(format!("entry path '{raw}' contains forbidden ':' character"));
        }
        clean_parts.push(seg);
    }
    Ok(clean_parts.join("/"))
}

fn is_macos_metadata_entry(clean_rel: &str) -> bool {
    if clean_rel == "__MACOSX" || clean_rel.starts_with("__MACOSX/") {
        return true;
    }
    clean_rel
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("._") || name == ".DS_Store")
}

/// Metadata for one extracted file from `extract_safe_zip`.
#[derive(Clone, Debug)]
pub struct ExtractedZipFile {
    pub rel_path: String,
    pub filename: String,
    pub ext: String,
    pub bytes: usize,
    pub is_audio: bool,
}

fn read_u16_le(data: &[u8], off: usize) -> Result<u16, String> {
    let slice = data
        .get(off..off + 2)
        .ok_or_else(|| "unexpected EOF in zip header".to_string())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(data: &[u8], off: usize) -> Result<u32, String> {
    let slice = data
        .get(off..off + 4)
        .ok_or_else(|| "unexpected EOF in zip header".to_string())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn crc32_ieee(data: &[u8]) -> u32 {
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

/// Check that no existing component from `base` through `rel` is a symlink.
fn ensure_no_symlink_components(base: &Path, rel: &str) -> Result<(), String> {
    let mut cur = base.to_path_buf();
    for seg in rel.split('/') {
        cur.push(seg);
        if let Ok(meta) = std::fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return Err(format!("path component '{}' is a symlink", cur.display()));
            }
        }
    }
    Ok(())
}

/// Zero-dependency Safe ZIP Extractor using `flate2::read::DeflateDecoder`.
///
/// Parses EOCD + Central Directory (supports Stored `0` and Deflate `8`),
/// enforces CWE-22 / CWE-59 / ZIP-slip / symlink / zip-bomb caps, and extracts
/// ONLY audio files (`.wav`, `.ogg`, `.mp3`, `.flac`, `.aiff`) plus `.txt`/`.md`
/// license files into `dest_root`.
pub fn extract_safe_zip(zip_bytes: &[u8], dest_root: &Path) -> Result<Vec<ExtractedZipFile>, String> {
    if zip_bytes.len() < 22 {
        return Err("invalid zip archive: smaller than 22 bytes".to_string());
    }
    if zip_bytes.len() > MAX_ZIP_DOWNLOAD_BYTES {
        return Err(format!(
            "zip archive exceeds maximum compressed size ({} MiB)",
            MAX_ZIP_DOWNLOAD_BYTES / (1024 * 1024)
        ));
    }

    // 1. Locate End of Central Directory (EOCD: 0x06054b50).
    let search_start = zip_bytes.len().saturating_sub(22 + 65_535);
    let mut eocd_pos: Option<usize> = None;
    let mut idx = zip_bytes.len() - 22;
    loop {
        if zip_bytes[idx..idx + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment_len = read_u16_le(zip_bytes, idx + 20)? as usize;
            if idx + 22 + comment_len == zip_bytes.len() {
                eocd_pos = Some(idx);
                break;
            }
        }
        if idx == search_start {
            break;
        }
        idx -= 1;
    }
    let eocd_pos = eocd_pos.ok_or_else(|| "invalid zip archive: missing EOCD record".to_string())?;

    let disk_no = read_u16_le(zip_bytes, eocd_pos + 4)?;
    let cd_disk = read_u16_le(zip_bytes, eocd_pos + 6)?;
    if disk_no != 0 || cd_disk != 0 {
        return Err("multi-disk zip archives are not supported".to_string());
    }
    let total_entries = read_u16_le(zip_bytes, eocd_pos + 10)?;
    let cd_size = read_u32_le(zip_bytes, eocd_pos + 12)?;
    let cd_offset = read_u32_le(zip_bytes, eocd_pos + 16)?;
    if total_entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        return Err("ZIP64 archives are not supported".to_string());
    }
    if (total_entries as usize) > MAX_ZIP_ENTRIES {
        return Err(format!(
            "zip archive has {total_entries} entries, exceeding maximum of {MAX_ZIP_ENTRIES}"
        ));
    }
    let cd_start = cd_offset as usize;
    let cd_end = cd_start
        .checked_add(cd_size as usize)
        .ok_or_else(|| "zip central directory offset overflow".to_string())?;
    if cd_end > eocd_pos {
        return Err("zip central directory extends past EOCD".to_string());
    }

    std::fs::create_dir_all(dest_root)
        .map_err(|e| format!("create extraction root '{}': {e}", dest_root.display()))?;
    let canon_root = std::fs::canonicalize(dest_root)
        .map_err(|e| format!("canonicalize extraction root: {e}"))?;

    let mut pos = cd_start;
    let mut entry_count: usize = 0;
    let mut total_uncompressed: usize = 0;
    let mut seen_paths: BTreeSet<String> = BTreeSet::new();
    let mut extracted = Vec::new();

    while pos < cd_end {
        entry_count += 1;
        if entry_count > MAX_ZIP_ENTRIES {
            return Err(format!(
                "zip central directory exceeds maximum of {MAX_ZIP_ENTRIES} entries"
            ));
        }
        if pos + 46 > cd_end {
            return Err("truncated zip central directory header".to_string());
        }
        if zip_bytes[pos..pos + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err("invalid zip central directory signature".to_string());
        }

        let flags = read_u16_le(zip_bytes, pos + 8)?;
        let method = read_u16_le(zip_bytes, pos + 10)?;
        let expected_crc = read_u32_le(zip_bytes, pos + 16)?;
        let comp_size = read_u32_le(zip_bytes, pos + 20)? as usize;
        let uncomp_size = read_u32_le(zip_bytes, pos + 24)? as usize;
        let name_len = read_u16_le(zip_bytes, pos + 28)? as usize;
        let extra_len = read_u16_le(zip_bytes, pos + 30)? as usize;
        let comment_len = read_u16_le(zip_bytes, pos + 32)? as usize;
        let external_attr = read_u32_le(zip_bytes, pos + 38)?;
        let local_header_offset = read_u32_le(zip_bytes, pos + 42)? as usize;

        let name_start = pos + 46;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| "zip entry name overflow".to_string())?;
        let next_pos = name_end
            .checked_add(extra_len)
            .and_then(|v| v.checked_add(comment_len))
            .ok_or_else(|| "zip central directory entry overflow".to_string())?;
        if next_pos > cd_end {
            return Err("zip central directory entry extends past central directory".to_string());
        }
        let raw_name = std::str::from_utf8(&zip_bytes[name_start..name_end])
            .map_err(|_| "zip entry name is not valid UTF-8".to_string())?;
        pos = next_pos;

        // Security check 1: reject POSIX symlinks & special file modes unconditionally.
        let unix_mode_type = (external_attr >> 16) & 0o170000;
        if unix_mode_type == 0o120000 {
            return Err(format!(
                "zip entry '{raw_name}' is a POSIX symlink (rejected)"
            ));
        }
        if unix_mode_type != 0 && unix_mode_type != 0o100000 && unix_mode_type != 0o040000 {
            return Err(format!(
                "zip entry '{raw_name}' has unsupported special file mode 0o{:o}",
                unix_mode_type
            ));
        }

        // Security check 2: validate path against traversal, drive letters, backslashes, NULs.
        let is_dir = raw_name.ends_with('/') || unix_mode_type == 0o040000;
        let clean_rel = validate_rel_archive_path(raw_name, is_dir)?;

        // Security check 3: declared single-entry uncompressed size cap (even before ext filter).
        if uncomp_size > MAX_ZIP_SINGLE_ENTRY_BYTES {
            return Err(format!(
                "zip entry '{clean_rel}' uncompressed size ({uncomp_size} bytes) exceeds 32 MiB limit"
            ));
        }

        if is_dir || is_macos_metadata_entry(&clean_rel) {
            continue;
        }

        // Extension allowlist: audio clips + .txt/.md license files only.
        let Some(ext) = file_ext_lower(&clean_rel) else {
            continue;
        };
        let is_audio = is_allowed_audio_ext(&ext);
        if !is_audio && !is_allowed_license_ext(&ext) {
            continue;
        }

        if (flags & 0x0001) != 0 {
            return Err(format!("zip entry '{clean_rel}' is encrypted (not supported)"));
        }
        if method != 0 && method != 8 {
            return Err(format!(
                "zip entry '{clean_rel}' uses unsupported compression method {method} (only Stored=0 and Deflate=8 supported)"
            ));
        }
        if !seen_paths.insert(clean_rel.clone()) {
            continue;
        }

        // Parse Local File Header (0x04034b50).
        if local_header_offset + 30 > cd_start {
            return Err(format!(
                "zip entry '{clean_rel}' local header offset out of bounds"
            ));
        }
        if zip_bytes[local_header_offset..local_header_offset + 4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(format!(
                "zip entry '{clean_rel}' has invalid local file header signature"
            ));
        }
        let local_name_len = read_u16_le(zip_bytes, local_header_offset + 26)? as usize;
        let local_extra_len = read_u16_le(zip_bytes, local_header_offset + 28)? as usize;
        let data_start = local_header_offset
            .checked_add(30)
            .and_then(|v| v.checked_add(local_name_len))
            .and_then(|v| v.checked_add(local_extra_len))
            .ok_or_else(|| "zip local header size overflow".to_string())?;
        let data_end = data_start
            .checked_add(comp_size)
            .ok_or_else(|| "zip entry compressed size overflow".to_string())?;
        if data_end > cd_start {
            return Err(format!(
                "zip entry '{clean_rel}' compressed data extends past central directory"
            ));
        }
        let compressed = &zip_bytes[data_start..data_end];

        let remaining_total = MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES.saturating_sub(total_uncompressed);
        let effective_cap = MAX_ZIP_SINGLE_ENTRY_BYTES.min(remaining_total);

        let data: Vec<u8> = match method {
            0 => {
                if comp_size != uncomp_size {
                    return Err(format!(
                        "zip stored entry '{clean_rel}' compressed/uncompressed length mismatch"
                    ));
                }
                if comp_size > MAX_ZIP_SINGLE_ENTRY_BYTES {
                    return Err(format!(
                        "zip entry '{clean_rel}' exceeds 32 MiB single-entry limit"
                    ));
                }
                if comp_size > remaining_total {
                    return Err(format!(
                        "zip archive exceeds 150 MiB total uncompressed limit at '{clean_rel}'"
                    ));
                }
                compressed.to_vec()
            }
            8 => {
                let mut decoder = DeflateDecoder::new(compressed).take((effective_cap as u64) + 1);
                let mut out = Vec::with_capacity(uncomp_size.min(effective_cap).min(256 * 1024));
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| format!("deflate error on zip entry '{clean_rel}': {e}"))?;
                if out.len() > MAX_ZIP_SINGLE_ENTRY_BYTES {
                    return Err(format!(
                        "zip entry '{clean_rel}' decompressed size exceeds 32 MiB single-entry limit"
                    ));
                }
                if out.len() > remaining_total {
                    return Err(format!(
                        "zip archive total uncompressed size exceeds 150 MiB limit at '{clean_rel}'"
                    ));
                }
                out
            }
            _ => unreachable!(),
        };

        if expected_crc != 0 && crc32_ieee(&data) != expected_crc {
            return Err(format!(
                "zip entry '{clean_rel}' CRC-32 mismatch (corrupt archive)"
            ));
        }

        total_uncompressed = total_uncompressed.saturating_add(data.len());
        if total_uncompressed > MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES {
            return Err("zip archive exceeds 150 MiB total uncompressed limit".to_string());
        }

        // Write safely inside canon_root with symlink + canonical containment checks.
        ensure_no_symlink_components(&canon_root, &clean_rel)?;
        let target_path = canon_root.join(&clean_rel);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create parent dir for '{clean_rel}': {e}"))?;
            let canon_parent = std::fs::canonicalize(parent)
                .map_err(|e| format!("canonicalize parent dir for '{clean_rel}': {e}"))?;
            if !canon_parent.starts_with(&canon_root) {
                return Err(format!(
                    "zip entry '{clean_rel}' parent escapes extraction root"
                ));
            }
        }
        std::fs::write(&target_path, &data)
            .map_err(|e| format!("write extracted file '{clean_rel}': {e}"))?;
        let canon_target = std::fs::canonicalize(&target_path)
            .map_err(|e| format!("canonicalize extracted file '{clean_rel}': {e}"))?;
        if !canon_target.starts_with(&canon_root) {
            let _ = std::fs::remove_file(&target_path);
            return Err(format!(
                "zip entry '{clean_rel}' escapes extraction root"
            ));
        }

        let filename = clean_rel
            .rsplit('/')
            .next()
            .unwrap_or(&clean_rel)
            .to_string();
        extracted.push(ExtractedZipFile {
            rel_path: clean_rel,
            filename,
            ext,
            bytes: data.len(),
            is_audio,
        });
    }

    Ok(extracted)
}

fn resolve_redirect(current: &str, loc: &str) -> Result<String, String> {
    let l = loc.trim();
    if l.starts_with("http://") || l.starts_with("https://") {
        return Ok(l.to_string());
    }
    let (scheme, rest) = current.split_once("://").ok_or("invalid current URL")?;
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

fn download_zip_checked(raw_url: &str, allow_loopback: bool) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .timeout(Duration::from_secs(120))
        .build();

    let mut current = validate_public_url(raw_url, allow_loopback)?;
    for _ in 0..5 {
        let resp = match agent
            .get(&current)
            .set("Accept", "application/zip,application/octet-stream,*/*;q=0.5")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) if (300..400).contains(&code) => {
                let loc = r
                    .header("Location")
                    .ok_or_else(|| format!("redirect HTTP {code} missing Location header"))?;
                let next = resolve_redirect(&current, loc)?;
                current = validate_public_url(&next, allow_loopback)?;
                continue;
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(format!("SFX pack download returned HTTP {code} for {current}"));
            }
            Err(e) => return Err(format!("SFX pack download failed for {current}: {e}")),
        };
        let status = resp.status();
        if (300..400).contains(&status) {
            let loc = resp
                .header("Location")
                .ok_or_else(|| format!("redirect HTTP {status} missing Location header"))?;
            let next = resolve_redirect(&current, loc)?;
            current = validate_public_url(&next, allow_loopback)?;
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(format!("SFX pack download returned HTTP {status} for {current}"));
        }
        let mut buf = Vec::new();
        resp.into_reader()
            .take((MAX_ZIP_DOWNLOAD_BYTES as u64) + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read SFX pack download body: {e}"))?;
        if buf.len() > MAX_ZIP_DOWNLOAD_BYTES {
            return Err(format!(
                "SFX pack download exceeded {} MiB limit",
                MAX_ZIP_DOWNLOAD_BYTES / (1024 * 1024)
            ));
        }
        return Ok(buf);
    }
    Err("too many HTTP redirects while downloading SFX pack".to_string())
}

fn tokenize_clip_tags(rel_path: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    let without_ext = match rel_path.rfind('.') {
        Some(idx) => &rel_path[..idx],
        None => rel_path,
    };
    for part in without_ext.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let low = part.to_ascii_lowercase();
        set.insert(low.clone());
        // Split camelCase and alpha/digit boundaries (e.g. `powerUp3` -> `power`, `up`, `3`, `click1` -> `click`)
        let mut cur = String::new();
        let mut prev_is_lower = false;
        let mut prev_is_digit = false;
        for ch in part.chars() {
            let is_upper = ch.is_ascii_uppercase();
            let is_digit = ch.is_ascii_digit();
            if !cur.is_empty() && ((is_upper && prev_is_lower) || (is_digit != prev_is_digit)) {
                if cur.len() >= 2 {
                    set.insert(cur.to_ascii_lowercase());
                }
                cur.clear();
            }
            cur.push(ch);
            prev_is_lower = ch.is_ascii_lowercase();
            prev_is_digit = is_digit;
        }
        if cur.len() >= 2 {
            set.insert(cur.to_ascii_lowercase());
        }
    }
    set.into_iter().collect()
}

fn percent_encode_path_segments(rel_path: &str) -> String {
    use std::fmt::Write;
    rel_path
        .split('/')
        .map(|seg| {
            let mut out = String::with_capacity(seg.len());
            for b in seg.bytes() {
                if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                    out.push(b as char);
                } else {
                    let _ = write!(&mut out, "%{b:02X}");
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_decode_strict(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("malformed percent-encoding in clip path".to_string());
            }
            let hi = (bytes[i + 1] as char)
                .to_digit(16)
                .ok_or_else(|| "invalid hex in percent-encoding".to_string())?;
            let lo = (bytes[i + 2] as char)
                .to_digit(16)
                .ok_or_else(|| "invalid hex in percent-encoding".to_string())?;
            let val = ((hi << 4) | lo) as u8;
            if val == 0 {
                return Err("null byte in percent-encoded clip path".to_string());
            }
            if val == b'%' {
                return Err("double-encoded '%25' in clip path is forbidden".to_string());
            }
            out.push(val);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "clip path is not valid UTF-8".to_string())
}

fn write_pack_index(
    pack_dir: &Path,
    pack_id: &str,
    name: &str,
    author: &str,
    license: &str,
    url: Option<&str>,
    description: &str,
    custom: bool,
    extracted: &[ExtractedZipFile],
) -> Result<Value, String> {
    let mut clips = Vec::new();
    let mut license_files = Vec::new();
    for item in extracted {
        if item.is_audio {
            clips.push(json!({
                "rel_path": item.rel_path,
                "filename": item.filename,
                "ext": item.ext,
                "bytes": item.bytes,
                "tags": tokenize_clip_tags(&item.rel_path),
            }));
        } else {
            license_files.push(item.rel_path.clone());
        }
    }
    clips.sort_by(|a, b| {
        a["rel_path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["rel_path"].as_str().unwrap_or(""))
    });
    license_files.sort();

    let installed_at_ms = now_ms();
    let index = json!({
        "version": 1,
        "id": pack_id,
        "name": name,
        "author": author,
        "license": license,
        "url": url,
        "description": description,
        "custom": custom,
        "installed_at_ms": installed_at_ms,
        "clip_count": clips.len(),
        "license_files": license_files,
        "clips": clips,
    });
    let text = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("serialize sfx pack index.json: {e}"))?;
    let idx_path = pack_dir.join("index.json");
    let tmp = pack_dir.join(".index.json.tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("write sfx index.json: {e}"))?;
    std::fs::rename(&tmp, &idx_path).map_err(|e| format!("commit sfx index.json: {e}"))?;
    Ok(index)
}

fn read_pack_index(pack_dir: &Path) -> Option<Value> {
    let idx_path = pack_dir.join("index.json");
    if let Ok(meta) = std::fs::symlink_metadata(&idx_path) {
        if meta.file_type().is_symlink() {
            return None;
        }
    }
    let raw = std::fs::read_to_string(&idx_path).ok()?;
    serde_json::from_str::<Value>(&raw).ok()
}

fn catalog_pack_view(spec: &Cc0PackSpec, idx_opt: Option<&Value>) -> Value {
    let installed = idx_opt.is_some();
    let installed_clips = idx_opt
        .and_then(|v| v.get("clips").and_then(Value::as_array))
        .map(Vec::len)
        .unwrap_or(0);
    let installed_at_ms = idx_opt.and_then(|v| v.get("installed_at_ms").and_then(Value::as_u64));
    let clip_count = if installed {
        installed_clips
    } else {
        spec.estimated_clips
    };
    json!({
        "id": spec.id,
        "name": spec.name,
        "author": spec.author,
        "license": spec.license,
        "url": spec.url,
        "description": spec.description,
        "tags": spec.tags,
        "clip_count": clip_count,
        "installed": installed,
        "installed_clips": installed_clips,
        "installed_at_ms": installed_at_ms,
        "custom": false,
    })
}

fn custom_pack_view(pack_id: &str, idx: &Value) -> Value {
    let installed_clips = idx
        .get("clips")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    json!({
        "id": pack_id,
        "name": idx.get("name").and_then(Value::as_str).unwrap_or(pack_id),
        "author": idx.get("author").and_then(Value::as_str).unwrap_or("Local Import"),
        "license": idx.get("license").and_then(Value::as_str).unwrap_or("CC0-1.0"),
        "url": idx.get("url").and_then(Value::as_str),
        "description": idx.get("description").and_then(Value::as_str).unwrap_or("Locally imported SFX pack."),
        "tags": ["local", "custom"],
        "clip_count": installed_clips,
        "installed": true,
        "installed_clips": installed_clips,
        "installed_at_ms": idx.get("installed_at_ms").and_then(Value::as_u64),
        "custom": true,
    })
}

/// List all 7 catalog CC0-1.0 SFX packs plus any locally imported packs, with
/// install counts and local audio runner status.
pub fn list_sfx_packs(data_dir: &Path) -> Value {
    let root = sfx_library_root(data_dir);
    let mut packs = Vec::new();
    let mut seen = BTreeSet::new();
    let mut installed_packs_count = 0usize;
    let mut installed_clips_total = 0usize;

    for spec in CC0_PACK_CATALOG.iter() {
        seen.insert(spec.id.to_string());
        let pdir = root.join(spec.id);
        let idx = read_pack_index(&pdir);
        if let Some(ref iv) = idx {
            installed_packs_count += 1;
            installed_clips_total += iv
                .get("clips")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
        }
        packs.push(catalog_pack_view(spec, idx.as_ref()));
    }

    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut custom_ids = Vec::new();
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || seen.contains(&fname) {
                continue;
            }
            if validate_pack_id(&fname).is_err() {
                continue;
            }
            let p = entry.path();
            if p.symlink_metadata()
                .ok()
                .is_some_and(|m| m.file_type().is_dir())
            {
                custom_ids.push(fname);
            }
        }
        custom_ids.sort();
        for cid in custom_ids {
            if let Some(idx) = read_pack_index(&root.join(&cid)) {
                installed_packs_count += 1;
                installed_clips_total += idx
                    .get("clips")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                packs.push(custom_pack_view(&cid, &idx));
            }
        }
    }

    let runner = local_audio_runner_path(data_dir);
    let runner_installed = runner.is_file();

    json!({
        "ok": true,
        "packs": packs,
        "installed_packs_count": installed_packs_count,
        "installed_clips_total": installed_clips_total,
        "runner_installed": runner_installed,
        "runner_path": if runner_installed { Some(runner.to_string_lossy().to_string()) } else { None },
    })
}

fn swap_staged_pack(root: &Path, pack_id: &str, staging_dir: &Path) -> Result<PathBuf, String> {
    let final_dir = root.join(pack_id);
    if final_dir.exists() {
        if final_dir
            .symlink_metadata()
            .ok()
            .is_some_and(|m| m.file_type().is_symlink())
        {
            return Err(format!("destination pack path '{pack_id}' is a symlink"));
        }
        std::fs::remove_dir_all(&final_dir)
            .map_err(|e| format!("replace existing pack '{pack_id}': {e}"))?;
    }
    std::fs::rename(staging_dir, &final_dir)
        .map_err(|e| format!("commit staged pack '{pack_id}': {e}"))?;
    Ok(final_dir)
}

/// Download (with SSRF guard & 64 MiB cap), safely extract, and index a CC0 SFX pack.
pub fn install_sfx_pack(
    data_dir: &Path,
    raw_pack_id: &str,
    custom_url_opt: Option<&str>,
) -> Result<Value, String> {
    let pack_id = validate_pack_id(raw_pack_id)?;
    let catalog = find_catalog_pack(&pack_id);
    let custom_url = custom_url_opt.map(str::trim).filter(|s| !s.is_empty());

    let (download_url, name, author, license, description, is_custom) = match (catalog, custom_url) {
        (Some(spec), Some(override_url)) => (
            override_url.to_string(),
            spec.name.to_string(),
            spec.author.to_string(),
            spec.license.to_string(),
            spec.description.to_string(),
            false,
        ),
        (Some(spec), None) => (
            spec.url.to_string(),
            spec.name.to_string(),
            spec.author.to_string(),
            spec.license.to_string(),
            spec.description.to_string(),
            false,
        ),
        (None, Some(u)) => (
            u.to_string(),
            pack_id.clone(),
            "Custom Pack".to_string(),
            "CC0-1.0".to_string(),
            "Custom downloaded SFX pack.".to_string(),
            true,
        ),
        (None, None) => {
            return Err(format!(
                "unknown CC0 SFX pack '{pack_id}' (available: {})",
                CC0_PACK_CATALOG
                    .iter()
                    .map(|p| p.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };

    let allow_loopback = allow_loopback_packs();
    let zip_bytes = download_zip_checked(&download_url, allow_loopback)?;

    let root = sfx_library_root(data_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("create cc0-sfx library root: {e}"))?;
    let staging = root.join(format!(".staging-{pack_id}-{}-{}", std::process::id(), now_ms()));
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }

    let extracted = match extract_safe_zip(&zip_bytes, &staging) {
        Ok(files) => files,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let audio_count = extracted.iter().filter(|f| f.is_audio).count();
    if audio_count == 0 {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "SFX pack '{pack_id}' archive contained zero supported audio files (.wav/.ogg/.mp3/.flac/.aiff)"
        ));
    }

    let idx = match write_pack_index(
        &staging,
        &pack_id,
        &name,
        &author,
        &license,
        Some(&download_url),
        &description,
        is_custom,
        &extracted,
    ) {
        Ok(i) => i,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    if let Err(e) = swap_staged_pack(&root, &pack_id, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let list = list_sfx_packs(data_dir);
    let pack_view = match catalog {
        Some(spec) => catalog_pack_view(spec, Some(&idx)),
        None => custom_pack_view(&pack_id, &idx),
    };

    Ok(json!({
        "ok": true,
        "pack": pack_view,
        "pack_id": pack_id,
        "installed_clips": audio_count,
        "packs": list["packs"],
        "installed_clips_total": list["installed_clips_total"],
    }))
}

/// Remove an installed SFX pack directory safely (rejecting symlinks & traversal).
pub fn remove_sfx_pack(data_dir: &Path, raw_pack_id: &str) -> Result<Value, String> {
    let pack_id = validate_pack_id(raw_pack_id)?;
    let root = sfx_library_root(data_dir);
    if !root.exists() {
        return Err(format!("SFX pack '{pack_id}' is not installed"));
    }
    let canon_root =
        std::fs::canonicalize(&root).map_err(|e| format!("canonicalize sfx library root: {e}"))?;
    let target = root.join(&pack_id);
    if !target.exists() {
        return Err(format!("SFX pack '{pack_id}' is not installed"));
    }
    if target
        .symlink_metadata()
        .map_err(|e| format!("stat sfx pack '{pack_id}': {e}"))?
        .file_type()
        .is_symlink()
    {
        return Err(format!("refusing to remove symlinked pack '{pack_id}'"));
    }
    let canon_target =
        std::fs::canonicalize(&target).map_err(|e| format!("canonicalize sfx pack '{pack_id}': {e}"))?;
    if !canon_target.starts_with(&canon_root) || canon_target == canon_root {
        return Err(format!("pack path '{pack_id}' escapes sfx library root"));
    }
    std::fs::remove_dir_all(&canon_target)
        .map_err(|e| format!("remove sfx pack '{pack_id}': {e}"))?;

    let list = list_sfx_packs(data_dir);
    Ok(json!({
        "ok": true,
        "pack_id": pack_id,
        "removed": true,
        "packs": list["packs"],
        "installed_clips_total": list["installed_clips_total"],
    }))
}

/// Import a local directory of audio clips as an indexed SFX pack (rejecting symlinks and traversal).
pub fn import_local_sfx_pack(
    data_dir: &Path,
    raw_pack_id: &str,
    name: &str,
    license: &str,
    source_dir: &Path,
) -> Result<Value, String> {
    let pack_id = validate_pack_id(raw_pack_id)?;
    let clean_name = name.trim();
    let clean_name = if clean_name.is_empty() {
        pack_id.as_str()
    } else {
        clean_name
    };
    let clean_license = license.trim();
    let clean_license = if clean_license.is_empty() {
        "CC0-1.0"
    } else {
        clean_license
    };

    // Reject path traversal components in source_dir itself and reject symlinked source_dir.
    if source_dir
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("source_dir must not contain '..' path traversal".to_string());
    }
    let src_meta = std::fs::symlink_metadata(source_dir)
        .map_err(|e| format!("source_dir '{}' not accessible: {e}", source_dir.display()))?;
    if src_meta.file_type().is_symlink() {
        return Err("source_dir must not be a symlink".to_string());
    }
    if !src_meta.file_type().is_dir() {
        return Err(format!("source_dir '{}' is not a directory", source_dir.display()));
    }
    let canon_src = std::fs::canonicalize(source_dir)
        .map_err(|e| format!("canonicalize source_dir: {e}"))?;

    let root = sfx_library_root(data_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("create cc0-sfx library root: {e}"))?;
    let staging = root.join(format!(
        ".staging-local-{pack_id}-{}-{}",
        std::process::id(),
        now_ms()
    ));
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(&staging).map_err(|e| format!("create staging dir: {e}"))?;
    let canon_staging = std::fs::canonicalize(&staging)
        .map_err(|e| format!("canonicalize staging dir: {e}"))?;

    let mut extracted = Vec::new();
    let mut total_bytes: usize = 0;
    let mut visited_entries: usize = 0;

    let walk_res = walk_local_import_dir(
        &canon_src,
        &canon_src,
        &canon_staging,
        0,
        &mut visited_entries,
        &mut total_bytes,
        &mut extracted,
    );
    if let Err(e) = walk_res {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let audio_count = extracted.iter().filter(|f| f.is_audio).count();
    if audio_count == 0 {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("source_dir contained zero supported audio files (.wav/.ogg/.mp3/.flac/.aiff)".to_string());
    }

    let idx = match write_pack_index(
        &staging,
        &pack_id,
        clean_name,
        "Local Import",
        clean_license,
        None,
        "Locally imported SFX audio pack.",
        true,
        &extracted,
    ) {
        Ok(i) => i,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    if let Err(e) = swap_staged_pack(&root, &pack_id, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let list = list_sfx_packs(data_dir);
    Ok(json!({
        "ok": true,
        "pack": custom_pack_view(&pack_id, &idx),
        "pack_id": pack_id,
        "installed_clips": audio_count,
        "packs": list["packs"],
        "installed_clips_total": list["installed_clips_total"],
    }))
}

fn walk_local_import_dir(
    canon_src_root: &Path,
    cur_dir: &Path,
    canon_dest_root: &Path,
    depth: usize,
    visited_entries: &mut usize,
    total_bytes: &mut usize,
    out: &mut Vec<ExtractedZipFile>,
) -> Result<(), String> {
    if depth > MAX_LOCAL_WALK_DEPTH {
        return Err("source_dir exceeds maximum nesting depth".to_string());
    }
    let entries = std::fs::read_dir(cur_dir)
        .map_err(|e| format!("read directory '{}': {e}", cur_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
        *visited_entries += 1;
        if *visited_entries > MAX_ZIP_ENTRIES {
            return Err(format!(
                "source_dir exceeds maximum of {MAX_ZIP_ENTRIES} entries"
            ));
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("stat '{}': {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "symlinks are forbidden in local SFX pack import: '{}'",
                path.display()
            ));
        }
        let canon_path =
            std::fs::canonicalize(&path).map_err(|e| format!("canonicalize '{}': {e}", path.display()))?;
        if !canon_path.starts_with(canon_src_root) {
            return Err(format!("path '{}' escapes source_dir", path.display()));
        }
        if meta.file_type().is_dir() {
            walk_local_import_dir(
                canon_src_root,
                &canon_path,
                canon_dest_root,
                depth + 1,
                visited_entries,
                total_bytes,
                out,
            )?;
            continue;
        }
        if !meta.file_type().is_file() {
            continue;
        }
        let rel = canon_path
            .strip_prefix(canon_src_root)
            .map_err(|_| "failed to compute relative path".to_string())?;
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let clean_rel = validate_rel_archive_path(&rel_str, false)?;
        if is_macos_metadata_entry(&clean_rel) {
            continue;
        }
        let Some(ext) = file_ext_lower(&clean_rel) else {
            continue;
        };
        let is_audio = is_allowed_audio_ext(&ext);
        if !is_audio && !is_allowed_license_ext(&ext) {
            continue;
        }
        let len = meta.len() as usize;
        if len > MAX_ZIP_SINGLE_ENTRY_BYTES {
            return Err(format!(
                "file '{clean_rel}' ({len} bytes) exceeds 32 MiB single-file cap"
            ));
        }
        *total_bytes = total_bytes.saturating_add(len);
        if *total_bytes > MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES {
            return Err("local SFX import exceeds 150 MiB total size cap".to_string());
        }
        let target = canon_dest_root.join(&clean_rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create parent dir for '{clean_rel}': {e}"))?;
        }
        std::fs::copy(&canon_path, &target)
            .map_err(|e| format!("copy '{clean_rel}': {e}"))?;
        let canon_target = std::fs::canonicalize(&target)
            .map_err(|e| format!("canonicalize target '{clean_rel}': {e}"))?;
        if !canon_target.starts_with(canon_dest_root) {
            return Err(format!("imported file '{clean_rel}' escapes destination root"));
        }
        let filename = clean_rel
            .rsplit('/')
            .next()
            .unwrap_or(&clean_rel)
            .to_string();
        out.push(ExtractedZipFile {
            rel_path: clean_rel,
            filename,
            ext,
            bytes: len,
            is_audio,
        });
    }
    Ok(())
}

/// Search across installed CC0/local SFX packs by filename, path, tags, or pack name.
pub fn search_sfx_packs(
    data_dir: &Path,
    query: &str,
    pack_id_opt: Option<&str>,
    limit: usize,
) -> Value {
    let root = sfx_library_root(data_dir);
    let capped_limit = limit.clamp(1, 200);
    let filter_pack = pack_id_opt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let terms: Vec<String> = query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let mut pack_dirs: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let pid = entry.file_name().to_string_lossy().to_string();
            if pid.starts_with('.') || validate_pack_id(&pid).is_err() {
                continue;
            }
            if let Some(ref fp) = filter_pack {
                if &pid != fp {
                    continue;
                }
            }
            let p = entry.path();
            if p.is_dir() {
                pack_dirs.push((pid, p));
            }
        }
    }
    pack_dirs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut scored: Vec<(i32, String, Value)> = Vec::new();
    for (pack_id, pack_dir) in pack_dirs {
        let Some(idx) = read_pack_index(&pack_dir) else {
            continue;
        };
        let pack_name = idx
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&pack_id)
            .to_string();
        let pack_name_low = pack_name.to_ascii_lowercase();
        let license = idx
            .get("license")
            .and_then(Value::as_str)
            .unwrap_or("CC0-1.0")
            .to_string();

        let Some(clips) = idx.get("clips").and_then(Value::as_array) else {
            continue;
        };
        for clip in clips {
            let Some(rel_path) = clip.get("rel_path").and_then(Value::as_str) else {
                continue;
            };
            let filename = clip
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or(rel_path);
            let ext = clip.get("ext").and_then(Value::as_str).unwrap_or("wav");
            let bytes = clip.get("bytes").and_then(Value::as_u64).unwrap_or(0);
            let clip_tags: Vec<String> = clip
                .get("tags")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_ascii_lowercase))
                        .collect()
                })
                .unwrap_or_else(|| tokenize_clip_tags(rel_path));

            let score = if terms.is_empty() {
                1
            } else {
                let fname_low = filename.to_ascii_lowercase();
                let rel_low = rel_path.to_ascii_lowercase();
                let mut total_score = 0i32;
                let mut all_matched = true;
                for term in &terms {
                    let mut term_score = 0i32;
                    if fname_low == *term || fname_low.starts_with(&format!("{term}.")) {
                        term_score = 100;
                    } else if clip_tags.iter().any(|t| t == term) {
                        term_score = 60;
                    } else if fname_low.contains(term.as_str()) {
                        term_score = 45;
                    } else if rel_low.contains(term.as_str()) {
                        term_score = 30;
                    } else if pack_name_low.contains(term.as_str()) || pack_id.contains(term.as_str())
                    {
                        term_score = 15;
                    }
                    if term_score == 0 {
                        all_matched = false;
                        break;
                    }
                    total_score += term_score;
                }
                if all_matched {
                    total_score
                } else {
                    0
                }
            };

            if score > 0 {
                let encoded_rel = percent_encode_path_segments(rel_path);
                let item = json!({
                    "pack_id": pack_id,
                    "pack_name": pack_name,
                    "license": license,
                    "rel_path": rel_path,
                    "filename": filename,
                    "ext": ext,
                    "bytes": bytes,
                    "tags": clip_tags,
                    "preview_url": format!("/api/library/sfx-packs/preview/{pack_id}/{encoded_rel}"),
                });
                scored.push((score, format!("{pack_id}/{rel_path}"), item));
            }
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let total = scored.len();
    let clips: Vec<Value> = scored
        .into_iter()
        .take(capped_limit)
        .map(|(_, _, v)| v)
        .collect();

    json!({
        "ok": true,
        "query": query,
        "total": total,
        "clips": clips,
    })
}

/// Resolve an installed clip path for previewing or mastering with strict
/// canonical containment and symlink rejection.
pub fn resolve_preview_clip_path(
    data_dir: &Path,
    raw_pack_id: &str,
    raw_rel_path: &str,
) -> Result<(PathBuf, &'static str), String> {
    let pack_id = validate_pack_id(raw_pack_id)?;
    let decoded = percent_decode_strict(raw_rel_path)?;
    let clean_rel = validate_rel_archive_path(&decoded, false)?;
    let ext = file_ext_lower(&clean_rel)
        .ok_or_else(|| format!("clip '{clean_rel}' missing audio extension"))?;
    let mime = audio_mime_for_ext(&ext)
        .ok_or_else(|| format!("unsupported preview audio extension '.{ext}'"))?;

    let root = sfx_library_root(data_dir);
    let pack_dir = root.join(&pack_id);
    if !pack_dir.exists() {
        return Err(format!("SFX pack '{pack_id}' is not installed"));
    }
    let canon_pack = std::fs::canonicalize(&pack_dir)
        .map_err(|_| format!("SFX pack '{pack_id}' is not accessible"))?;
    ensure_no_symlink_components(&canon_pack, &clean_rel)?;
    let candidate = canon_pack.join(&clean_rel);
    let meta = std::fs::symlink_metadata(&candidate)
        .map_err(|_| format!("SFX clip '{clean_rel}' not found in pack '{pack_id}'"))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(format!("SFX clip '{clean_rel}' is not a regular file"));
    }
    let canon_file = std::fs::canonicalize(&candidate)
        .map_err(|_| format!("SFX clip '{clean_rel}' not found in pack '{pack_id}'"))?;
    if !canon_file.starts_with(&canon_pack) {
        return Err("SFX clip path escapes pack directory".to_string());
    }
    Ok((canon_file, mime))
}

// ---------------------------------------------------------------------------
// WAV Analysis, ffmpeg Conversion/Layering, and Pure-Rust WAV Fallback
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct WavStats {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub duration_ms: u32,
    pub peak_dbfs: f64,
}

/// Inspect a RIFF/WAVE PCM buffer and compute duration, format metadata, and peak dBFS.
pub fn analyze_pcm16_wav(bytes: &[u8]) -> Result<WavStats, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("buffer is not a valid RIFF/WAVE file".to_string());
    }
    let mut off = 12usize;
    let mut audio_format: Option<u16> = None;
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut data_slice: Option<&[u8]> = None;

    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let size = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let chunk_start = off + 8;
        let chunk_end = chunk_start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && chunk_end >= chunk_start + 16 {
            audio_format = Some(u16::from_le_bytes([bytes[chunk_start], bytes[chunk_start + 1]]));
            channels = Some(u16::from_le_bytes([bytes[chunk_start + 2], bytes[chunk_start + 3]]));
            sample_rate = Some(u32::from_le_bytes([
                bytes[chunk_start + 4],
                bytes[chunk_start + 5],
                bytes[chunk_start + 6],
                bytes[chunk_start + 7],
            ]));
            bits_per_sample = Some(u16::from_le_bytes([
                bytes[chunk_start + 14],
                bytes[chunk_start + 15],
            ]));
        } else if id == b"data" {
            data_slice = Some(&bytes[chunk_start..chunk_end]);
        }
        // Chunks are word-aligned (padded to even size).
        let padded = size + (size & 1);
        let Some(next) = chunk_start.checked_add(padded) else {
            break;
        };
        if next <= off {
            break;
        }
        off = next;
    }

    let fmt = audio_format.ok_or_else(|| "WAV missing fmt chunk".to_string())?;
    if fmt != 1 {
        return Err(format!("WAV format code {fmt} is not uncompressed PCM (1)"));
    }
    let ch = channels.unwrap_or(1);
    let sr = sample_rate.unwrap_or(SAMPLE_RATE);
    let bps = bits_per_sample.unwrap_or(16);
    if ch == 0 || sr == 0 || bps == 0 {
        return Err("invalid WAV fmt parameters".to_string());
    }
    let data = data_slice.ok_or_else(|| "WAV missing data chunk".to_string())?;
    let bytes_per_frame = (ch as usize) * ((bps as usize) / 8).max(1);
    let frames = data.len() / bytes_per_frame.max(1);
    let duration_ms = ((frames as u64) * 1000 / (sr as u64)).max(1) as u32;

    let mut peak = 0.0f64;
    if bps == 16 {
        for chunk in data.chunks_exact(2) {
            let s = i16::from_le_bytes([chunk[0], chunk[1]]);
            let mag = (s as f64).abs() / 32_768.0;
            if mag > peak {
                peak = mag;
            }
        }
    } else if bps == 8 {
        for &b in data {
            let mag = ((b as f64) - 128.0).abs() / 128.0;
            if mag > peak {
                peak = mag;
            }
        }
    }
    let peak_dbfs = if peak > 1e-9 {
        (20.0 * peak.log10() * 100.0).round() / 100.0
    } else {
        -96.0
    };

    Ok(WavStats {
        sample_rate: sr,
        channels: ch,
        bits_per_sample: bps,
        duration_ms,
        peak_dbfs,
    })
}

fn find_ffmpeg_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let cand = dir.join(bin_name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// Convert arbitrary audio bytes (MP3/OGG/FLAC/WAV) into 44.1 kHz 16-bit PCM mono WAV
/// using `ffmpeg` when available, or validating/returning cleanly when already valid RIFF PCM WAV.
pub fn normalize_audio_bytes_to_wav(raw_bytes: &[u8], hint_ext: Option<&str>) -> Result<Vec<u8>, String> {
    if raw_bytes.is_empty() {
        return Err("audio payload is empty".to_string());
    }
    if let Ok(stats) = analyze_pcm16_wav(raw_bytes) {
        if stats.sample_rate == SAMPLE_RATE && stats.channels == 1 && stats.bits_per_sample == 16 {
            return Ok(raw_bytes.to_vec());
        }
    }
    if let Some(ffmpeg) = find_ffmpeg_binary() {
        let ext = hint_ext
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()) && !e.is_empty())
            .unwrap_or_else(|| {
                if raw_bytes.len() >= 12 && &raw_bytes[0..4] == b"RIFF" && &raw_bytes[8..12] == b"WAVE" {
                    "wav".to_string()
                } else if raw_bytes.starts_with(b"OggS") {
                    "ogg".to_string()
                } else if raw_bytes.starts_with(b"fLaC") {
                    "flac".to_string()
                } else {
                    "mp3".to_string()
                }
            });
        let nonce = format!("{}-{}", std::process::id(), now_ms());
        let tmp_dir = std::env::temp_dir().join(format!("openro-audio-conv-{nonce}"));
        std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("create audio conv temp dir: {e}"))?;
        let in_path = tmp_dir.join(format!("in.{ext}"));
        let out_path = tmp_dir.join("out.wav");
        let res = (|| -> Result<Vec<u8>, String> {
            std::fs::write(&in_path, raw_bytes).map_err(|e| format!("write temp audio input: {e}"))?;
            let output = Command::new(&ffmpeg)
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(&in_path)
                .args([
                    "-ar",
                    "44100",
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                    "-f",
                    "wav",
                ])
                .arg(&out_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("spawn ffmpeg: {e}"))?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("ffmpeg conversion failed: {}", err.trim()));
            }
            std::fs::read(&out_path).map_err(|e| format!("read converted wav: {e}"))
        })();
        let _ = std::fs::remove_dir_all(&tmp_dir);
        if let Ok(wav) = res {
            if analyze_pcm16_wav(&wav).is_ok() {
                return Ok(wav);
            }
        }
    }

    // Fallback when ffmpeg is unavailable or failed: accept already-valid RIFF PCM WAV.
    let stats = analyze_pcm16_wav(raw_bytes).map_err(|e| {
        format!("cannot normalize non-WAV audio without ffmpeg on PATH ({e})")
    })?;
    if stats.sample_rate == SAMPLE_RATE && stats.channels == 1 && stats.bits_per_sample == 16 {
        Ok(raw_bytes.to_vec())
    } else if stats.bits_per_sample == 16 {
        // Pure-Rust stereo->mono & linear resample fallback for PCM16 WAV.
        resample_pcm16_wav_to_44100_mono(raw_bytes, &stats)
    } else {
        Ok(raw_bytes.to_vec())
    }
}

fn resample_pcm16_wav_to_44100_mono(bytes: &[u8], stats: &WavStats) -> Result<Vec<u8>, String> {
    let mut off = 12usize;
    let mut data_slice: Option<&[u8]> = None;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let size = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let start = off + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"data" {
            data_slice = Some(&bytes[start..end]);
            break;
        }
        let padded = size + (size & 1);
        let Some(next) = start.checked_add(padded) else {
            break;
        };
        if next <= off {
            break;
        }
        off = next;
    }
    let data = data_slice.ok_or_else(|| "WAV missing data chunk".to_string())?;
    let ch = (stats.channels as usize).max(1);
    let mut mono = Vec::with_capacity(data.len() / (2 * ch));
    for frame in data.chunks_exact(2 * ch) {
        let mut sum = 0.0f64;
        for c in 0..ch {
            let s = i16::from_le_bytes([frame[c * 2], frame[c * 2 + 1]]) as f64;
            sum += s;
        }
        mono.push((sum / (ch as f64)) / 32_768.0);
    }
    if mono.is_empty() {
        return Err("WAV contains zero audio samples".to_string());
    }
    let out_len = (((mono.len() as u64) * (SAMPLE_RATE as u64)) / (stats.sample_rate as u64).max(1))
        .max(1) as usize;
    let mut out_i16 = Vec::with_capacity(out_len);
    let ratio = (stats.sample_rate as f64) / (SAMPLE_RATE as f64);
    for i in 0..out_len {
        let src_pos = (i as f64) * ratio;
        let idx = (src_pos.floor() as usize).min(mono.len() - 1);
        let next = (idx + 1).min(mono.len() - 1);
        let frac = src_pos - (idx as f64);
        let sample = mono[idx] * (1.0 - frac) + mono[next] * frac;
        out_i16.push((sample.clamp(-0.98, 0.98) * 32_767.0) as i16);
    }
    Ok(crate::audio_synth::encode_pcm16_wav(&out_i16, SAMPLE_RATE))
}

fn slug_wav_name(raw: Option<&str>, fallback_clip: &str) -> String {
    let base = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let fname = fallback_clip.rsplit('/').next().unwrap_or("sfx");
            match fname.rfind('.') {
                Some(dot) => &fname[..dot],
                None => fname,
            }
        });
    let stripped = base
        .strip_suffix(".wav")
        .or_else(|| base.strip_suffix(".WAV"))
        .unwrap_or(base);
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_sep && !slug.is_empty() {
            slug.push('_');
            prev_sep = true;
        }
    }
    let clean = slug.trim_matches(|c| c == '_' || c == '-');
    if clean.is_empty() {
        "sfx_clip.wav".to_string()
    } else {
        format!("{clean}.wav")
    }
}

struct LayerSpec {
    abs_path: PathBuf,
    delay_ms: u64,
    gain_db: f64,
    pitch_semitones: f64,
}

fn build_ffmpeg_chain_for_input(
    input_idx: usize,
    delay_ms: u64,
    gain_db: f64,
    pitch_semitones: f64,
    fx: Option<&Value>,
    out_label: &str,
) -> String {
    let mut filters: Vec<String> = Vec::new();
    filters.push("aresample=44100".to_string());
    filters.push("aformat=sample_fmts=fltp:channel_layouts=mono".to_string());

    let pitch = pitch_semitones.clamp(-24.0, 24.0);
    if pitch.abs() > 0.01 {
        let factor = 2.0f64.powf(pitch / 12.0);
        let rate = (44_100.0 * factor).round().clamp(8_000.0, 192_000.0) as u32;
        filters.push(format!("asetrate={rate}"));
        filters.push("aresample=44100".to_string());
    }

    if let Some(fx_obj) = fx {
        if let Some(hp) = fx_obj.get("highpass_hz").and_then(Value::as_f64) {
            if hp.is_finite() && hp >= 20.0 && hp <= 18_000.0 {
                filters.push(format!("highpass=f={hp:.1}"));
            }
        }
        if let Some(lp) = fx_obj.get("lowpass_hz").and_then(Value::as_f64) {
            if lp.is_finite() && lp >= 80.0 && lp <= 20_000.0 {
                filters.push(format!("lowpass=f={lp:.1}"));
            }
        }
        if let Some(trim_ms) = fx_obj.get("trim_ms").and_then(Value::as_u64) {
            if trim_ms >= 20 && trim_ms <= 60_000 {
                filters.push(format!("atrim=0:{:.3}", (trim_ms as f64) / 1000.0));
            }
        }
        if let Some(fade_in_ms) = fx_obj.get("fade_in_ms").and_then(Value::as_u64) {
            if fade_in_ms > 0 && fade_in_ms <= 10_000 {
                filters.push(format!("afade=t=in:st=0:d={:.3}", (fade_in_ms as f64) / 1000.0));
            }
        }
        if let Some(fade_out_ms) = fx_obj.get("fade_out_ms").and_then(Value::as_u64) {
            if fade_out_ms > 0 && fade_out_ms <= 10_000 {
                // Use areverse + fade in + areverse so fade-out works cleanly regardless of duration.
                filters.push(format!(
                    "areverse,afade=t=in:st=0:d={:.3},areverse",
                    (fade_out_ms as f64) / 1000.0
                ));
            }
        }
    }

    let gain = gain_db.clamp(-48.0, 24.0);
    if gain.abs() > 0.01 {
        filters.push(format!("volume={gain:.2}dB"));
    }
    if delay_ms > 0 {
        let d = delay_ms.min(30_000);
        filters.push(format!("adelay={d}|{d}"));
    }

    format!("[{input_idx}:a]{}[{out_label}]", filters.join(","))
}

fn master_with_ffmpeg(
    ffmpeg: &Path,
    primary: &LayerSpec,
    layers: &[LayerSpec],
    fx: Option<&Value>,
) -> Result<Vec<u8>, String> {
    let nonce = format!("{}-{}", std::process::id(), now_ms());
    let tmp_dir = std::env::temp_dir().join(format!("openro-sfx-use-{nonce}"));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("create sfx master temp dir: {e}"))?;
    let out_wav = tmp_dir.join("mastered.wav");

    let run_res = (|| -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&primary.abs_path);
        for layer in layers {
            cmd.arg("-i").arg(&layer.abs_path);
        }

        let mut graph_parts = Vec::new();
        let mut mix_labels = Vec::new();
        graph_parts.push(build_ffmpeg_chain_for_input(
            0,
            primary.delay_ms,
            primary.gain_db,
            primary.pitch_semitones,
            fx,
            "s0",
        ));
        mix_labels.push("[s0]".to_string());

        for (i, layer) in layers.iter().enumerate() {
            let idx = i + 1;
            let lbl = format!("s{idx}");
            graph_parts.push(build_ffmpeg_chain_for_input(
                idx,
                layer.delay_ms,
                layer.gain_db,
                layer.pitch_semitones,
                None,
                &lbl,
            ));
            mix_labels.push(format!("[{lbl}]"));
        }

        let total_inputs = 1 + layers.len();
        if total_inputs > 1 {
            graph_parts.push(format!(
                "{}amix=inputs={total_inputs}:duration=longest:dropout_transition=0:normalize=0,aresample=44100,aformat=sample_fmts=s16:channel_layouts=mono,alimiter=limit=0.891:attack=5:release=50[out]",
                mix_labels.join("")
            ));
        } else {
            graph_parts.push(
                "[s0]aresample=44100,aformat=sample_fmts=s16:channel_layouts=mono,alimiter=limit=0.891:attack=5:release=50[out]"
                    .to_string(),
            );
        }

        cmd.args(["-filter_complex", &graph_parts.join(";")])
            .args([
                "-map",
                "[out]",
                "-ar",
                "44100",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(&out_wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let out = cmd.output().map_err(|e| format!("spawn ffmpeg: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("ffmpeg mastering failed: {}", err.trim()));
        }
        std::fs::read(&out_wav).map_err(|e| format!("read mastered wav: {e}"))
    })();

    let _ = std::fs::remove_dir_all(&tmp_dir);
    run_res
}

/// Copy, mix, and master an installed CC0 SFX clip (+ optional `layer_clips` and `fx`)
/// into 44.1 kHz 16-bit PCM mono WAV and atomically write to `<project>/assets/audio/<out_name>.wav`
/// (and `<data_dir>/game-assets/audio/<filename>` for instant preview).
pub fn use_sfx_clip(
    data_dir: &Path,
    project_root: Option<&Path>,
    req: &Value,
) -> Result<Value, String> {
    let pack_id_raw = req
        .get("pack_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required field 'pack_id'".to_string())?;
    let clip_raw = req
        .get("clip")
        .or_else(|| req.get("rel_path"))
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required field 'clip'".to_string())?;

    let pack_id = validate_pack_id(pack_id_raw)?;
    let (primary_abs, _) = resolve_preview_clip_path(data_dir, &pack_id, clip_raw)?;

    let fx = req.get("fx").filter(|v| v.is_object());
    let primary_gain = fx
        .and_then(|f| f.get("gain_db").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let primary_pitch = fx
        .and_then(|f| f.get("pitch_semitones").and_then(Value::as_f64))
        .unwrap_or(0.0);

    let primary = LayerSpec {
        abs_path: primary_abs.clone(),
        delay_ms: 0,
        gain_db: primary_gain,
        pitch_semitones: primary_pitch,
    };

    let mut layers: Vec<LayerSpec> = Vec::new();
    if let Some(arr) = req.get("layer_clips").and_then(Value::as_array) {
        if arr.len() > MAX_LAYER_CLIPS {
            return Err(format!("layer_clips cannot exceed {MAX_LAYER_CLIPS} layers"));
        }
        for (idx, item) in arr.iter().enumerate() {
            let l_pack = item
                .get("pack_id")
                .and_then(Value::as_str)
                .unwrap_or(&pack_id);
            let l_clip = item
                .get("clip")
                .or_else(|| item.get("rel_path"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("layer_clips[{idx}] missing 'clip'"))?;
            let (l_abs, _) = resolve_preview_clip_path(data_dir, l_pack, l_clip)?;
            layers.push(LayerSpec {
                abs_path: l_abs,
                delay_ms: item.get("delay_ms").and_then(Value::as_u64).unwrap_or(0),
                gain_db: item.get("gain_db").and_then(Value::as_f64).unwrap_or(0.0),
                pitch_semitones: item
                    .get("pitch_semitones")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            });
        }
    }

    let raw_out_name = req
        .get("out_name")
        .or_else(|| req.get("name"))
        .and_then(Value::as_str);
    let filename = sanitize_audio_filename(&slug_wav_name(raw_out_name, clip_raw))?;

    let mut ffmpeg_used = false;
    let wav_bytes = if let Some(ffmpeg) = find_ffmpeg_binary() {
        match master_with_ffmpeg(&ffmpeg, &primary, &layers, fx) {
            Ok(bytes) => {
                ffmpeg_used = true;
                bytes
            }
            Err(e) => {
                if !layers.is_empty() {
                    return Err(e);
                }
                let raw = std::fs::read(&primary_abs)
                    .map_err(|re| format!("read source audio clip: {re}"))?;
                normalize_audio_bytes_to_wav(&raw, file_ext_lower(clip_raw).as_deref())?
            }
        }
    } else {
        if !layers.is_empty() {
            return Err("ffmpeg is required on PATH to mix multiple layer_clips".to_string());
        }
        let raw =
            std::fs::read(&primary_abs).map_err(|e| format!("read source audio clip: {e}"))?;
        normalize_audio_bytes_to_wav(&raw, file_ext_lower(clip_raw).as_deref())?
    };

    let stats = analyze_pcm16_wav(&wav_bytes)?;
    let mut hasher = Sha256::new();
    hasher.update(&wav_bytes);
    let sha256: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();

    // Always write to <data_dir>/game-assets/audio/<filename> for instant browser playback.
    let preview_dir = audio_assets_dir(data_dir);
    std::fs::create_dir_all(&preview_dir)
        .map_err(|e| format!("create game-assets/audio dir: {e}"))?;
    let preview_target = preview_dir.join(&filename);
    let preview_tmp = preview_dir.join(format!(".{filename}.{}.tmp", std::process::id()));
    std::fs::write(&preview_tmp, &wav_bytes).map_err(|e| format!("write preview wav: {e}"))?;
    std::fs::rename(&preview_tmp, &preview_target)
        .map_err(|e| format!("commit preview wav: {e}"))?;

    // Write into active project <project_root>/assets/audio/<out_name>.wav when present.
    let mut out_rel = format!("assets/audio/{filename}");
    let mut project_path_out: Option<String> = None;
    if let Some(root) = project_root {
        let rel = req
            .get("out_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("assets/audio/{filename}"));
        if !rel.to_ascii_lowercase().ends_with(".wav") {
            return Err("out_path for audio must end with .wav".to_string());
        }
        let abs = crate::opencloud::safe_rel(root, &rel).map_err(|e| e.msg)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create project audio dir: {e}"))?;
        }
        let ptmp = abs.with_extension(format!("wav.{}.tmp-openro", std::process::id()));
        std::fs::write(&ptmp, &wav_bytes).map_err(|e| format!("write project wav: {e}"))?;
        std::fs::rename(&ptmp, &abs).map_err(|e| format!("commit project wav: {e}"))?;
        out_rel = rel.clone();
        project_path_out = Some(rel);
    }

    let idx = read_pack_index(&sfx_library_root(data_dir).join(&pack_id));
    let license = idx
        .as_ref()
        .and_then(|v| v.get("license").and_then(Value::as_str))
        .unwrap_or("CC0-1.0");
    let author = idx
        .as_ref()
        .and_then(|v| v.get("author").and_then(Value::as_str))
        .unwrap_or("Unknown");
    let attribution = if license.eq_ignore_ascii_case("CC0-1.0")
        || license.eq_ignore_ascii_case("CC0")
    {
        "None required (CC0 1.0 Public Domain)".to_string()
    } else {
        format!("{license} ({author})")
    };

    let url = format!("/api/assets/audio/{filename}");
    Ok(json!({
        "ok": true,
        "path": out_rel,
        "project_path": project_path_out,
        "filename": filename,
        "url": url,
        "wav_url": url,
        "bytes": wav_bytes.len(),
        "sha256": sha256,
        "duration_ms": stats.duration_ms,
        "sample_rate": stats.sample_rate,
        "channels": stats.channels,
        "bits_per_sample": stats.bits_per_sample,
        "peak_dbfs": stats.peak_dbfs,
        "pack_id": pack_id,
        "clip": clip_raw,
        "layers_used": 1 + layers.len(),
        "ffmpeg_used": ffmpeg_used,
        "attribution": attribution,
    }))
}

// ---------------------------------------------------------------------------
// Local Neural Audio Runner Installer (`<data_dir>/bin/openro-audio-gen`)
// ---------------------------------------------------------------------------

const LOCAL_AUDIO_RUNNER_SCRIPT: &str = r#"#!/usr/bin/env python3
"""OpenRO Local Neural Audio Runner (v1.33.0).

Supports hardware-tiered local open-weights SFX/audio models:
  - audioldm_s        (~185M · CPU / 4GB RAM)
  - stable_audio_small (~341M · CPU / 8GB RAM)
  - tangoflux         (~515M · 8-12GB GPU)
  - audioldm2         (~1.1B · 12-16GB GPU)
  - stable_audio_open (~1.05B · 16GB+ GPU)

Normalizes all generated output to 44.1 kHz 16-bit PCM mono WAV (peak <= -1.0 dBFS,
DC offset removed) and prints compact JSON to stdout.
"""

import argparse
import importlib.util
import json
import math
import os
import struct
import sys
import wave

VERSION = "1.33.0"
TARGET_SR = 44100
PEAK_LIMIT = 0.89125  # -1.0 dBFS

BACKEND_MODELS = {
    "audioldm_s": "cvssp/audioldm-s-full-v2",
    "stable_audio_small": "stabilityai/stable-audio-open-small",
    "tangoflux": "declare-lab/TangoFlux",
    "audioldm2": "cvssp/audioldm2",
    "stable_audio_open": "stabilityai/stable-audio-open-1.0",
}


def has_pkg(name: str) -> bool:
    return importlib.util.find_spec(name) is not None


def check_env() -> dict:
    pkgs = {
        p: has_pkg(p)
        for p in ("torch", "diffusers", "transformers", "scipy", "soundfile", "torchaudio", "numpy")
    }
    cuda = False
    if pkgs["torch"]:
        try:
            import torch  # type: ignore

            cuda = bool(torch.cuda.is_available())
        except Exception:
            cuda = False
    ready = []
    if pkgs["torch"] and pkgs["diffusers"] and pkgs["transformers"]:
        ready.extend(["audioldm_s", "stable_audio_small", "audioldm2", "stable_audio_open"])
    if pkgs["torch"] and has_pkg("tangoflux"):
        ready.append("tangoflux")
    return {
        "ok": True,
        "tool": "openro-audio-gen",
        "version": VERSION,
        "python": sys.version.split()[0],
        "cuda_available": cuda,
        "packages": pkgs,
        "ready_backends": ready,
        "backends": list(BACKEND_MODELS.keys()),
        "install_hint": "pip install torch diffusers transformers scipy soundfile",
    }


def to_mono_float(audio) -> list:
    # Convert torch tensor / numpy ndarray / nested list to flat 1-D Python float list
    if hasattr(audio, "detach"):
        audio = audio.detach().cpu().numpy()
    if hasattr(audio, "ndim"):
        import numpy as np  # type: ignore

        arr = np.asarray(audio, dtype=np.float64)
        while arr.ndim > 2:
            arr = arr[0]
        if arr.ndim == 2:
            # Handle (channels, samples) or (samples, channels)
            if arr.shape[0] <= 8 and arr.shape[1] > arr.shape[0]:
                arr = arr.mean(axis=0)
            else:
                arr = arr.mean(axis=1)
        return arr.reshape(-1).tolist()
    return [float(x) for x in audio]


def resample_linear(samples: list, src_sr: int, dst_sr: int) -> list:
    if src_sr == dst_sr or not samples:
        return samples
    if has_pkg("scipy") and has_pkg("numpy"):
        try:
            import numpy as np  # type: ignore
            from scipy.signal import resample_poly  # type: ignore

            g = math.gcd(src_sr, dst_sr)
            out = resample_poly(np.asarray(samples, dtype=np.float64), dst_sr // g, src_sr // g)
            return out.tolist()
        except Exception:
            pass
    out_len = max(1, int(round(len(samples) * dst_sr / float(src_sr))))
    ratio = src_sr / float(dst_sr)
    n_last = len(samples) - 1
    out = []
    for i in range(out_len):
        pos = i * ratio
        idx = min(int(pos), n_last)
        nxt = min(idx + 1, n_last)
        frac = pos - idx
        out.append(samples[idx] * (1.0 - frac) + samples[nxt] * frac)
    return out


def write_mastered_wav(out_path: str, samples: list, src_sr: int) -> dict:
    mono = resample_linear(samples, src_sr, TARGET_SR)
    if not mono:
        raise RuntimeError("model returned zero audio samples")
    mean_val = sum(mono) / float(len(mono))
    centered = [x - mean_val for x in mono]
    peak = max((abs(x) for x in centered), default=0.0)
    scale = (PEAK_LIMIT / peak) if peak > PEAK_LIMIT else 1.0
    final_peak = 0.0
    pcm = bytearray(len(centered) * 2)
    for i, x in enumerate(centered):
        v = max(-PEAK_LIMIT, min(PEAK_LIMIT, x * scale))
        av = abs(v)
        if av > final_peak:
            final_peak = av
        struct.pack_into("<h", pcm, i * 2, int(round(v * 32767.0)))

    parent = os.path.dirname(os.path.abspath(out_path))
    if parent:
        os.makedirs(parent, exist_ok=True)
    tmp_path = out_path + ".openro.tmp"
    with wave.open(tmp_path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(TARGET_SR)
        wf.writeframes(pcm)
    os.replace(tmp_path, out_path)

    peak_dbfs = round(20.0 * math.log10(final_peak), 2) if final_peak > 1e-9 else -96.0
    duration_ms = max(1, int(round(len(centered) * 1000.0 / TARGET_SR)))
    return {
        "ok": True,
        "out": out_path,
        "sample_rate": TARGET_SR,
        "channels": 1,
        "bits_per_sample": 16,
        "duration_ms": duration_ms,
        "peak_dbfs": peak_dbfs,
        "bytes": os.path.getsize(out_path),
    }


def generate(backend: str, prompt: str, duration: float, out_path: str, seed, steps) -> dict:
    if backend not in BACKEND_MODELS:
        raise RuntimeError(f"unsupported backend '{backend}'")
    if not has_pkg("torch"):
        raise RuntimeError(
            "PyTorch is not installed. Run: pip install torch diffusers transformers scipy soundfile"
        )
    import torch  # type: ignore

    device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.float16 if device == "cuda" else torch.float32
    gen = torch.Generator(device=device)
    if seed is not None:
        gen.manual_seed(int(seed))

    if backend == "tangoflux":
        if not has_pkg("tangoflux"):
            raise RuntimeError("tangoflux package not installed. Run: pip install tangoflux")
        from tangoflux import TangoFluxInference  # type: ignore

        model = TangoFluxInference(name=BACKEND_MODELS[backend], device=device)
        audio = model.generate(prompt, steps=int(steps or 25), duration=float(duration))
        samples = to_mono_float(audio)
        info = write_mastered_wav(out_path, samples, 44100)
        info["backend"] = backend
        return info

    if not has_pkg("diffusers"):
        raise RuntimeError(
            "diffusers is not installed. Run: pip install torch diffusers transformers scipy soundfile"
        )

    model_id = BACKEND_MODELS[backend]
    if backend == "audioldm_s":
        from diffusers import AudioLDMPipeline  # type: ignore

        pipe = AudioLDMPipeline.from_pretrained(model_id, torch_dtype=dtype).to(device)
        res = pipe(
            prompt,
            num_inference_steps=int(steps or 20),
            audio_length_in_s=float(duration),
            generator=gen,
        )
        samples = to_mono_float(res.audios[0])
        src_sr = 16000
    elif backend == "audioldm2":
        from diffusers import AudioLDM2Pipeline  # type: ignore

        pipe = AudioLDM2Pipeline.from_pretrained(model_id, torch_dtype=dtype).to(device)
        res = pipe(
            prompt,
            num_inference_steps=int(steps or 50),
            audio_length_in_s=float(duration),
            generator=gen,
        )
        samples = to_mono_float(res.audios[0])
        src_sr = 16000
    else:
        from diffusers import StableAudioPipeline  # type: ignore

        pipe = StableAudioPipeline.from_pretrained(model_id, torch_dtype=dtype).to(device)
        default_steps = 8 if backend == "stable_audio_small" else 50
        res = pipe(
            prompt,
            num_inference_steps=int(steps or default_steps),
            audio_end_in_s=float(duration),
            generator=gen,
        )
        samples = to_mono_float(res.audios[0])
        src_sr = int(getattr(pipe.vae, "sampling_rate", 44100))

    info = write_mastered_wav(out_path, samples, src_sr)
    info["backend"] = backend
    return info


def main() -> int:
    parser = argparse.ArgumentParser(description="OpenRO Local Neural Audio Runner")
    parser.add_argument("--version", action="store_true", help="Print runner version as JSON")
    parser.add_argument("--check", action="store_true", help="Check installed Python audio packages")
    parser.add_argument("--backend", choices=list(BACKEND_MODELS.keys()), default="audioldm_s")
    parser.add_argument("--prompt", default="")
    parser.add_argument("--duration", type=float, default=4.0)
    parser.add_argument("--out", default="")
    parser.add_argument("--seed", type=int, default=None)
    parser.add_argument("--steps", type=int, default=None)
    args = parser.parse_args()

    if args.version:
        print(json.dumps({"ok": True, "tool": "openro-audio-gen", "version": VERSION}))
        return 0
    if args.check:
        print(json.dumps(check_env()))
        return 0
    if not args.prompt.strip():
        print(json.dumps({"ok": False, "error": "--prompt is required"}), file=sys.stderr)
        return 2
    if not args.out.strip():
        print(json.dumps({"ok": False, "error": "--out is required"}), file=sys.stderr)
        return 2

    duration = max(0.25, min(30.0, float(args.duration)))
    try:
        res = generate(args.backend, args.prompt.strip(), duration, args.out.strip(), args.seed, args.steps)
        print(json.dumps(res))
        return 0
    except Exception as exc:
        print(
            json.dumps(
                {
                    "ok": False,
                    "backend": args.backend,
                    "error": str(exc),
                    "install_hint": "pip install torch diffusers transformers scipy soundfile",
                }
            ),
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
"#;

/// Install the Python 3 local neural audio runner script at `<data_dir>/bin/openro-audio-gen`
/// (`0755` on Unix) and run `--check` when `python3` is available.
pub fn install_local_audio_runner(data_dir: &Path) -> Result<Value, String> {
    let bin_dir = data_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("create runner bin dir '{}': {e}", bin_dir.display()))?;
    let target = local_audio_runner_path(data_dir);
    let tmp = bin_dir.join(format!(".openro-audio-gen.{}.tmp", std::process::id()));
    std::fs::write(&tmp, LOCAL_AUDIO_RUNNER_SCRIPT.as_bytes())
        .map_err(|e| format!("write local audio runner script: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod 0755 local audio runner: {e}"))?;
    }

    std::fs::rename(&tmp, &target)
        .map_err(|e| format!("commit local audio runner script: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
    }

    let python_bin = if cfg!(windows) { "python" } else { "python3" };
    let mut python_available = false;
    let mut check_json: Option<Value> = None;

    if let Ok(out) = Command::new(python_bin)
        .arg(&target)
        .arg("--check")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if out.status.success() {
            python_available = true;
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(parsed) = serde_json::from_str::<Value>(stdout.trim()) {
                check_json = Some(parsed);
            }
        }
    }

    Ok(json!({
        "ok": true,
        "path": target.to_string_lossy().to_string(),
        "command": "openro-audio-gen",
        "executable": true,
        "python_available": python_available,
        "check": check_json,
        "backends": [
            "audioldm_s",
            "stable_audio_small",
            "tangoflux",
            "audioldm2",
            "stable_audio_open"
        ],
        "install_hint": "pip install torch diffusers transformers scipy soundfile",
    }))
}
