//! Free CC0-1.0 SFX Pack Library, Zero-Dependency Safe ZIP Extractor,
//! 44.1 kHz PCM16 Mono WAV Mastering, and Local Neural Audio Runner Installer
//! for OpenRO v1.33.0 (Gap #88), plus trending Modern UI / Motion-Graphics SFX
//! packs and the copyright-free YouTube music sourcing adapter (v1.34.0, Gap #90).

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

/// Static catalog entry for a verified free (CC0-1.0 or CC-BY) SFX pack.
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

static CC0_PACK_CATALOG: [Cc0PackSpec; 9] = [
    Cc0PackSpec {
        id: "motion-ui-swishes",
        name: "Motion Graphics Swishes & Whooshes (CC0)",
        author: "artisticdude",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/swishes.zip",
        description: "Snappy swishes and whooshes for motion-graphics transitions, card slides, panel reveals, and modern UI motion.",
        tags: &["motion-graphics", "whoosh", "swish", "transition", "card", "ui"],
        estimated_clips: 13,
    },
    Cc0PackSpec {
        id: "modern-interface-sfx",
        name: "Modern UI Interface Starter Pack (Clicks, Pops & Taps)",
        author: "p0ss",
        license: "CC0-1.0",
        url: "https://opengameart.org/sites/default/files/interface.zip",
        description: "Clean modern interface clicks, pops, taps, toggles, and menu confirmations for polished app-style HUDs.",
        tags: &["ui", "motion-graphics", "click", "pop", "tap", "menu", "modern"],
        estimated_clips: 30,
    },
    Cc0PackSpec {
        id: "modern-ui-pack-1",
        name: "UI Sound Effects Pack Vol. 1 (Menus, Chimes & Alerts)",
        author: "junggle",
        license: "CC-BY-3.0",
        url: "https://opengameart.org/sites/default/files/UI%20pack%201.zip",
        description: "Modern menu blips, button chimes, alerts, and notification cues (CC-BY 3.0: credit junggle).",
        tags: &["ui", "motion-graphics", "chime", "alert", "button", "modern"],
        estimated_clips: 20,
    },
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
];

/// Verified free SFX pack catalog (9 packs: 3 trending Modern UI / Motion-Graphics packs first,
/// then 6 CC0-1.0 game SFX packs).
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

/// List all 9 catalog SFX packs plus any locally imported packs, with
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

pub(crate) fn find_ffmpeg_binary() -> Option<PathBuf> {
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

// ---------------------------------------------------------------------------
// Copyright-Free YouTube Music Sourcing (v1.34.0, Gap #90)
// ---------------------------------------------------------------------------

/// Shortest track kept by `search_youtube_music` (filters out 3-second shorts).
pub const YT_MUSIC_MIN_DURATION_S: f64 = 45.0;
/// Longest track kept by `search_youtube_music` (filters out 1-hour mixes).
pub const YT_MUSIC_MAX_DURATION_S: f64 = 420.0;
/// Default mastered loop length in seconds.
pub const YT_MUSIC_DEFAULT_CLIP_S: u64 = 90;
/// Hard clamp for mastered loop length (keeps 44.1 kHz mono PCM16 well under Roblox's 20 MB cap).
pub const YT_MUSIC_MIN_CLIP_S: u64 = 10;
pub const YT_MUSIC_MAX_CLIP_S: u64 = 110;
pub const YT_MUSIC_SEARCH_TIMEOUT_S: u64 = 35;
const YT_MUSIC_DOWNLOAD_TIMEOUT_S: u64 = 240;
const YT_MUSIC_FFMPEG_TIMEOUT_S: u64 = 180;
const YT_MUSIC_MAX_LIMIT: usize = 25;
const YT_MUSIC_MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

/// Terms that are always filtered out (title / description / tags, case-insensitive).
pub const ANTI_DUBSTEP_TERMS: [&str; 4] = ["dubstep", "brostep", "riddim", "wobble bass"];

/// Curated Roblox-trending copyright-free music genre.
#[derive(Clone, Debug)]
pub struct MusicGenre {
    pub id: &'static str,
    pub name: &'static str,
    pub query: &'static str,
    pub description: &'static str,
}

static MUSIC_GENRES: [MusicGenre; 7] = [
    MusicGenre {
        id: "jazz-rap",
        name: "Jazz Rap / Boom Bap",
        query: "no copyright jazz rap boom bap instrumental beat free for profit",
        description: "Dusty jazz chords over boom-bap drums for lobbies, shops, and hangout hubs.",
    },
    MusicGenre {
        id: "lofi",
        name: "Lo-Fi Chill / Study Beats",
        query: "StreamBeats chill lofi hip hop jazz copyright free",
        description: "Mellow lo-fi loops for menus, idle tycoons, and cozy simulators.",
    },
    MusicGenre {
        id: "trap",
        name: "Trap Instrumental",
        query: "100% royalty free no copyright trap beat instrumental free for profit",
        description: "Hard 808 trap instrumentals for battles, races, and obbies.",
    },
    MusicGenre {
        id: "carti-type-beat",
        name: "Carti / Melodic Rage Trap",
        query: "100% royalty free no copyright playboi carti type beat rage synth",
        description: "Rage synth leads and distorted 808s for high-energy PvP and gacha reveals.",
    },
    MusicGenre {
        id: "pluggnb",
        name: "PluggnB / Chill Trap",
        query: "free for profit no copyright pluggnb chill melodic trap type beat",
        description: "Airy melodic pluggnb for social hubs, dress-up, and showcase games.",
    },
    MusicGenre {
        id: "anime-hiphop",
        name: "Anime Hip-Hop / Chillhop",
        query: "no copyright anime lofi hip hop chill beat instrumental",
        description: "Anime-flavored chillhop for anime fighters, card collectors, and RPG towns.",
    },
    MusicGenre {
        id: "synthwave",
        name: "Synthwave / Cyber",
        query: "StreamBeats synthwave retrowave copyright free",
        description: "Retro synthwave for neon cities, racing, and sci-fi hubs.",
    },
];

/// Curated Roblox-trending copyright-free music genres (7 genres, zero dubstep).
pub fn music_genres() -> &'static [MusicGenre] {
    &MUSIC_GENRES
}

pub fn find_music_genre(id: &str) -> Option<&'static MusicGenre> {
    let wanted = id.trim().to_ascii_lowercase();
    MUSIC_GENRES.iter().find(|g| g.id == wanted)
}

pub fn yt_music_runner_path(data_dir: &Path) -> PathBuf {
    data_dir.join("bin").join("openro-yt-music")
}

fn find_binary_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    for dir in std::env::split_paths(&path_var) {
        for c in &candidates {
            let cand = dir.join(c);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

pub(crate) fn find_yt_dlp_binary() -> Option<PathBuf> {
    find_binary_on_path("yt-dlp")
}

/// Genre catalog + tool availability snapshot for `GET /api/library/music/genres`.
pub fn music_genres_json(data_dir: Option<&Path>) -> Value {
    let genres: Vec<Value> = MUSIC_GENRES
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "name": g.name,
                "query": g.query,
                "description": g.description,
            })
        })
        .collect();
    json!({
        "ok": true,
        "source": "youtube_nocopyright_music",
        "genres": genres,
        "default_genre": "lofi",
        "anti_dubstep_terms": ANTI_DUBSTEP_TERMS,
        "duration_window_s": [YT_MUSIC_MIN_DURATION_S, YT_MUSIC_MAX_DURATION_S],
        "default_clip_s": YT_MUSIC_DEFAULT_CLIP_S,
        "max_clip_s": YT_MUSIC_MAX_CLIP_S,
        "yt_dlp_available": find_yt_dlp_binary().is_some(),
        "ffmpeg_available": find_ffmpeg_binary().is_some(),
        "runner_installed": data_dir.map(|d| yt_music_runner_path(d).is_file()).unwrap_or(false),
    })
}

fn normalize_match_text(text: &str) -> String {
    text.to_ascii_lowercase()
        .chars()
        .map(|c| if c == '-' || c == '_' || c == '#' { ' ' } else { c })
        .collect()
}

/// `true` when text mentions dubstep / brostep / riddim / wobble bass (case-insensitive).
pub fn is_dubstep_like(text: &str) -> bool {
    let norm = normalize_match_text(text);
    let squashed: String = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    ANTI_DUBSTEP_TERMS.iter().any(|t| squashed.contains(t))
}

fn clean_display_text(raw: &str, max_chars: usize) -> String {
    raw.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

/// Classify the reuse license of a copyright-free track from its title, description, and channel.
///
/// Returns `(license_status, credit_line)` where `license_status` is one of
/// `royalty_free_verified`, `free_for_profit`, `streambeats_dmca_free`, `non_profit_only`,
/// or `verify_description`.
pub fn classify_music_license(title: &str, description: &str, channel: &str) -> (&'static str, String) {
    let hay = normalize_match_text(&format!("{title}\n{description}\n{channel}"));
    let hay: String = hay.split_whitespace().collect::<Vec<_>>().join(" ");
    let chan_lower = channel.to_ascii_lowercase();
    let streambeats = hay.contains("streambeats")
        || hay.contains("stream beats")
        || chan_lower.contains("harris heller");
    let has = |needles: &[&str]| needles.iter().any(|n| hay.contains(n));

    let status = if streambeats {
        "streambeats_dmca_free"
    } else if has(&[
        "non profit",
        "nonprofit",
        "not for profit",
        "non commercial",
        "noncommercial",
    ]) {
        "non_profit_only"
    } else if has(&["royalty free", "royaltyfree"])
        && has(&["no copyright", "copyright free", "100%"])
    {
        "royalty_free_verified"
    } else if has(&[
        "free for profit",
        "free to use for profit",
        "free for commercial",
        "free commercial use",
    ]) {
        "free_for_profit"
    } else {
        "verify_description"
    };

    let credit_line = if streambeats {
        "StreamBeats by Harris Heller".to_string()
    } else {
        let chan = clean_display_text(channel, 80);
        if chan.is_empty() {
            "Prod. Unknown Producer".to_string()
        } else {
            format!("Prod. {chan}")
        }
    };
    (status, credit_line)
}

fn is_valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Strictly validate a YouTube URL and return `(video_id, normalized_url)`.
///
/// Accepts ONLY `https://www.youtube.com/watch?v=<id>` or `https://youtu.be/<id>` where `<id>` is
/// exactly 11 `[A-Za-z0-9_-]` characters. Plain tracking params (`&t=30`, `?si=abc`) are allowed
/// and discarded. The returned URL is always rebuilt as `https://www.youtube.com/watch?v=<id>`,
/// so nothing user-controlled other than the id ever reaches `yt-dlp` (no SSRF, no flag injection).
pub fn validate_youtube_url(raw: &str) -> Result<(String, String), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("missing YouTube URL".to_string());
    }
    if s.len() > 200 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("invalid YouTube URL (too long or contains whitespace/control characters)".to_string());
    }
    let rest = if let Some(r) = s.strip_prefix("https://www.youtube.com/watch?v=") {
        r
    } else if let Some(r) = s.strip_prefix("https://youtu.be/") {
        r
    } else {
        return Err(
            "only https://www.youtube.com/watch?v=<id> or https://youtu.be/<id> URLs are allowed".to_string(),
        );
    };
    if rest.len() < 11 || !rest.is_char_boundary(11) {
        return Err("invalid YouTube video id (expected 11 characters of [A-Za-z0-9_-])".to_string());
    }
    let (id, tail) = rest.split_at(11);
    if !is_valid_youtube_id(id) {
        return Err("invalid YouTube video id (expected 11 characters of [A-Za-z0-9_-])".to_string());
    }
    if !tail.is_empty() {
        let params_ok = (tail.starts_with('&') || tail.starts_with('?'))
            && tail[1..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'=' | b'&' | b'.'));
        if !params_ok {
            return Err("invalid YouTube URL suffix after video id".to_string());
        }
    }
    let normalized = format!("https://www.youtube.com/watch?v={id}");
    Ok((id.to_string(), normalized))
}

fn yt_track_tags(v: &Value) -> Vec<String> {
    v.get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|t| clean_display_text(t, 40))
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Filter + classify `yt-dlp --dump-json` JSONL output into copyright-free music tracks.
///
/// Drops entries with invalid ids, durations outside `45..=420` seconds, and any dubstep-like
/// title / description / tags. Returns `(tracks, filtered_counts)`.
pub fn filter_youtube_music_jsonl(jsonl: &str, genre_id: &str, limit: usize) -> (Vec<Value>, Value) {
    let mut tracks: Vec<Value> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let (mut invalid, mut duration, mut dubstep, mut duplicate) = (0usize, 0usize, 0usize, 0usize);
    let limit = limit.clamp(1, YT_MUSIC_MAX_LIMIT);

    for line in jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            invalid += 1;
            continue;
        };
        let id = v.get("id").and_then(Value::as_str).unwrap_or("");
        if !is_valid_youtube_id(id) {
            invalid += 1;
            continue;
        }
        let title = v.get("title").and_then(Value::as_str).unwrap_or("");
        let description = v.get("description").and_then(Value::as_str).unwrap_or("");
        let channel = v
            .get("channel")
            .and_then(Value::as_str)
            .or_else(|| v.get("uploader").and_then(Value::as_str))
            .unwrap_or("");
        let tags = yt_track_tags(&v);
        let dur = v.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        if !dur.is_finite() || !(YT_MUSIC_MIN_DURATION_S..=YT_MUSIC_MAX_DURATION_S).contains(&dur) {
            duration += 1;
            continue;
        }
        if is_dubstep_like(title) || is_dubstep_like(description) || tags.iter().any(|t| is_dubstep_like(t)) {
            dubstep += 1;
            continue;
        }
        if !seen.insert(id.to_string()) {
            duplicate += 1;
            continue;
        }
        if tracks.len() >= limit {
            continue;
        }
        let (license_status, credit_line) = classify_music_license(title, description, channel);
        tracks.push(json!({
            "id": id,
            "title": clean_display_text(title, 160),
            "channel": clean_display_text(channel, 80),
            "duration_s": (dur * 10.0).round() / 10.0,
            "youtube_url": format!("https://www.youtube.com/watch?v={id}"),
            "thumbnail": format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
            "view_count": v.get("view_count").and_then(Value::as_u64),
            "license_status": license_status,
            "credit_line": credit_line,
            "genre": genre_id,
            "tags": tags.into_iter().take(8).collect::<Vec<_>>(),
        }));
    }
    let counts = json!({
        "invalid": invalid,
        "duration": duration,
        "dubstep": dubstep,
        "duplicate": duplicate,
    });
    (tracks, counts)
}

pub(crate) struct CapturedRun {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}

/// Run a binary with a strict argv (never a shell), capturing stdout/stderr with a hard timeout.
/// On timeout the child is killed and any stdout already produced is still returned.
pub(crate) fn run_argv_with_timeout(bin: &Path, args: &[String], timeout: Duration) -> Result<CapturedRun, String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn '{}': {e}", bin.display()))?;
    let mut stdout = child.stdout.take().ok_or_else(|| "missing child stdout".to_string())?;
    let mut stderr = child.stderr.take().ok_or_else(|| "missing child stderr".to_string())?;
    let out_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let out_buf_t = std::sync::Arc::clone(&out_buf);
    let out_t = std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut g = out_buf_t.lock().unwrap_or_else(|p| p.into_inner());
                    if g.len() + n <= YT_MUSIC_MAX_CAPTURE_BYTES {
                        g.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
    });
    let err_t = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = (&mut stderr).take(256 * 1024).read_to_end(&mut s);
        String::from_utf8_lossy(&s).to_string()
    });

    let deadline = std::time::Instant::now() + timeout;
    let mut timed_out = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("wait for '{}': {e}", bin.display()));
            }
        }
    };
    // Reader threads finish once the pipes close; don't block forever on stray grandchildren.
    let join_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !(out_t.is_finished() && err_t.is_finished()) && std::time::Instant::now() < join_deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stderr_s = if err_t.is_finished() {
        err_t.join().unwrap_or_default()
    } else {
        String::new()
    };
    if out_t.is_finished() {
        let _ = out_t.join();
    }
    let stdout_s = {
        let g = out_buf.lock().unwrap_or_else(|p| p.into_inner());
        String::from_utf8_lossy(&g).to_string()
    };
    Ok(CapturedRun {
        code,
        stdout: stdout_s,
        stderr: stderr_s,
        timed_out,
    })
}

fn sanitize_music_query(raw: Option<&str>) -> Option<String> {
    let q = clean_display_text(raw.unwrap_or(""), 160);
    // Drop leading dashes so a query can never look like a CLI flag (it is also prefixed by `ytsearchN:`).
    let q = q.trim_start_matches('-').trim().to_string();
    if q.is_empty() {
        None
    } else {
        Some(q)
    }
}

pub(crate) fn read_mock_jsonl(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        if let Ok(text) = std::fs::read_to_string(trimmed) {
            return text;
        }
    }
    raw.to_string()
}

/// Search copyright-free music on YouTube via `yt-dlp ytsearch<N>:<query>` (strict argv, 35 s timeout).
///
/// `genre_opt` picks a curated Roblox-trending genre (defaults to `lofi` when neither genre nor
/// query is given). When `OPENRO_YT_MUSIC_MOCK_JSON` is set (JSONL text or a path to a JSONL file)
/// it is used instead of running `yt-dlp`, for deterministic offline tests.
pub fn search_youtube_music(
    genre_opt: Option<&str>,
    query_opt: Option<&str>,
    limit: usize,
) -> Result<Value, String> {
    let genre = match genre_opt.map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => Some(find_music_genre(g).ok_or_else(|| {
            let ids: Vec<&str> = MUSIC_GENRES.iter().map(|g| g.id).collect();
            format!("unknown music genre '{g}' (use one of: {})", ids.join(", "))
        })?),
        None => None,
    };
    let query = sanitize_music_query(query_opt);
    if let Some(q) = &query {
        if is_dubstep_like(q) {
            return Err("dubstep / brostep / riddim / wobble bass searches are blocked (anti-dubstep filter)".to_string());
        }
    }
    let genre = if genre.is_none() && query.is_none() {
        find_music_genre("lofi")
    } else {
        genre
    };
    let genre_id = genre.map(|g| g.id).unwrap_or("custom");
    let search_query = match (&query, genre) {
        (Some(q), Some(g)) => format!("{q} {}", g.query),
        (Some(q), None) => {
            let lower = q.to_ascii_lowercase();
            if lower.contains("copyright") || lower.contains("royalty") || lower.contains("streambeats") {
                q.clone()
            } else {
                format!("{q} no copyright free for profit instrumental")
            }
        }
        (None, Some(g)) => g.query.to_string(),
        (None, None) => unreachable!("genre defaults to lofi"),
    };
    let limit = limit.clamp(1, YT_MUSIC_MAX_LIMIT);
    let fetch_n = (limit + limit / 2 + 2).min(36);

    let mock = std::env::var("OPENRO_YT_MUSIC_MOCK_JSON")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let (jsonl, source, timed_out) = if let Some(mock_raw) = mock {
        (read_mock_jsonl(&mock_raw), "mock", false)
    } else {
        let bin = find_yt_dlp_binary().ok_or_else(|| {
            "yt-dlp not found on PATH (install yt-dlp + ffmpeg, then click Install Music Runner)".to_string()
        })?;
        let argv = vec![
            "--dump-json".to_string(),
            "--no-warnings".to_string(),
            "--skip-download".to_string(),
            "--ignore-errors".to_string(),
            "--".to_string(),
            format!("ytsearch{fetch_n}:{search_query}"),
        ];
        let run = run_argv_with_timeout(&bin, &argv, Duration::from_secs(YT_MUSIC_SEARCH_TIMEOUT_S))?;
        if run.stdout.trim().is_empty() {
            if run.timed_out {
                return Err(format!(
                    "yt-dlp search timed out after {YT_MUSIC_SEARCH_TIMEOUT_S}s with no results"
                ));
            }
            if run.code != Some(0) {
                let err: String = run.stderr.trim().chars().take(400).collect();
                return Err(format!("yt-dlp search failed: {err}"));
            }
        }
        (run.stdout, "yt-dlp", run.timed_out)
    };

    let (tracks, filtered) = filter_youtube_music_jsonl(&jsonl, genre_id, limit);
    Ok(json!({
        "ok": true,
        "source": source,
        "genre": genre_id,
        "genre_name": genre.map(|g| g.name),
        "query": query,
        "search_query": search_query,
        "limit": limit,
        "count": tracks.len(),
        "tracks": tracks,
        "filtered_out": filtered,
        "anti_dubstep": true,
        "partial": timed_out,
        "duration_window_s": [YT_MUSIC_MIN_DURATION_S, YT_MUSIC_MAX_DURATION_S],
    }))
}

fn slug_music_name(raw: Option<&str>, fallback: &str) -> String {
    let base = raw.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(fallback);
    let base = base
        .strip_suffix(".wav")
        .or_else(|| base.strip_suffix(".WAV"))
        .unwrap_or(base);
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep && !slug.is_empty() {
            slug.push('_');
            prev_sep = true;
        }
        if slug.len() >= 60 {
            break;
        }
    }
    let clean = slug.trim_matches('_').to_string();
    if clean.is_empty() {
        "music_loop".to_string()
    } else {
        clean
    }
}

/// RMS level (dBFS) of a PCM16 WAV buffer.
pub fn pcm16_rms_dbfs(bytes: &[u8]) -> f64 {
    let samples = pcm16_samples(bytes);
    if samples.is_empty() {
        return -96.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&s| {
            let f = s as f64 / 32_768.0;
            f * f
        })
        .sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms > 1e-9 {
        (20.0 * rms.log10() * 100.0).round() / 100.0
    } else {
        -96.0
    }
}

fn pcm16_samples(bytes: &[u8]) -> Vec<i16> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Vec::new();
    }
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let size = u32::from_le_bytes([bytes[off + 4], bytes[off + 5], bytes[off + 6], bytes[off + 7]]) as usize;
        let start = off + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"data" {
            return bytes[start..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        let Some(next) = start.checked_add(size + (size & 1)) else {
            break;
        };
        if next <= off {
            break;
        }
        off = next;
    }
    Vec::new()
}

/// Master any source audio file to 44.1 kHz 16-bit PCM mono WAV with ffmpeg:
/// trim to `max_s`, `loudnorm=I=-16:TP=-1.8:LRA=8`, 0.15 s fade-in and 0.35 s fade-out.
fn master_music_with_ffmpeg(ffmpeg: &Path, input: &Path, output: &Path, max_s: u64) -> Result<(), String> {
    let chain = format!(
        "atrim=0:{max_s},asetpts=PTS-STARTPTS,loudnorm=I=-16:TP=-1.8:LRA=8,aresample=44100,\
         afade=t=in:st=0:d=0.15,areverse,afade=t=in:st=0:d=0.35,areverse"
    );
    let args: Vec<String> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().to_string(),
        "-vn".into(),
        "-map_metadata".into(),
        "-1".into(),
        "-bitexact".into(),
        "-af".into(),
        chain,
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "1".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "wav".into(),
        output.to_string_lossy().to_string(),
    ];
    let run = run_argv_with_timeout(ffmpeg, &args, Duration::from_secs(YT_MUSIC_FFMPEG_TIMEOUT_S))?;
    if run.timed_out {
        return Err(format!("ffmpeg mastering timed out after {YT_MUSIC_FFMPEG_TIMEOUT_S}s"));
    }
    if run.code != Some(0) {
        let err: String = run.stderr.trim().chars().take(400).collect();
        return Err(format!("ffmpeg music mastering failed: {err}"));
    }
    Ok(())
}

/// Pure-Rust fallback mastering (when ffmpeg is unavailable): convert PCM WAV to 44.1 kHz mono,
/// trim to `max_s`, apply 0.15 s / 0.35 s linear fades, and peak-normalize to -1.8 dBFS.
fn master_music_rust_fallback(raw: &[u8], hint_ext: Option<&str>, max_s: u64) -> Result<Vec<u8>, String> {
    let wav = normalize_audio_bytes_to_wav(raw, hint_ext)?;
    let mut samples: Vec<f64> = pcm16_samples(&wav).iter().map(|&s| s as f64 / 32_768.0).collect();
    if samples.is_empty() {
        return Err("source audio contains zero samples".to_string());
    }
    let max_len = (max_s as usize) * (SAMPLE_RATE as usize);
    samples.truncate(max_len);
    let n = samples.len();
    let fade_in = ((0.15 * SAMPLE_RATE as f64) as usize).min(n);
    let fade_out = ((0.35 * SAMPLE_RATE as f64) as usize).min(n);
    for (i, s) in samples.iter_mut().enumerate().take(fade_in) {
        *s *= i as f64 / fade_in.max(1) as f64;
    }
    for k in 0..fade_out {
        let idx = n - 1 - k;
        samples[idx] *= k as f64 / fade_out.max(1) as f64;
    }
    let peak = samples.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    let target = 10f64.powf(-1.8 / 20.0);
    let gain = if peak > 1e-9 { target / peak } else { 1.0 };
    let pcm: Vec<i16> = samples
        .iter()
        .map(|s| ((s * gain).clamp(-target, target) * 32_767.0) as i16)
        .collect();
    Ok(crate::audio_synth::encode_pcm16_wav(&pcm, SAMPLE_RATE))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn atomic_write(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir '{}': {e}", parent.display()))?;
    }
    let fname = target
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".to_string());
    let tmp = target.with_file_name(format!(".{fname}.{}.{}.tmp", std::process::id(), now_ms()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write '{}': {e}", tmp.display()))?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("commit '{}': {e}", target.display())
    })
}

fn ensure_source_receipts_gitignored(project_root: &Path) {
    let gi = project_root.join(".gitignore");
    let line = "assets/audio/*.source.json";
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) {
        return;
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    let _ = std::fs::write(&gi, next);
}

/// Import a copyright-free YouTube music track, master it to a loop-ready 44.1 kHz 16-bit PCM
/// mono WAV, and atomically write `<project>/assets/audio/<out_name>.wav` (or
/// `<data_dir>/game-assets/audio/<out_name>.wav` with no project) plus a
/// `<out_name>.source.json` provenance receipt.
///
/// `req`: `{ url | youtube_url, genre?, out_name?, max_duration_s? (10..=110, default 90),
/// title?, channel?, description? }`. When `OPENRO_YT_MUSIC_MOCK_AUDIO_FILE` points at a local
/// audio file it replaces the `yt-dlp` download (offline tests); URL validation still applies.
pub fn import_youtube_music(
    data_dir: &Path,
    project_root: Option<&Path>,
    req: &Value,
) -> Result<Value, String> {
    let raw_url = req
        .get("url")
        .or_else(|| req.get("youtube_url"))
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required field 'url' (YouTube watch URL)".to_string())?;
    let (video_id, youtube_url) = validate_youtube_url(raw_url)?;

    let genre_id = match req.get("genre").and_then(Value::as_str).map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => find_music_genre(g)
            .map(|g| g.id.to_string())
            .ok_or_else(|| format!("unknown music genre '{g}'"))?,
        None => "custom".to_string(),
    };
    let max_s = req
        .get("max_duration_s")
        .or_else(|| req.get("duration_s"))
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64)))
        .unwrap_or(YT_MUSIC_DEFAULT_CLIP_S)
        .clamp(YT_MUSIC_MIN_CLIP_S, YT_MUSIC_MAX_CLIP_S);

    let mut title = req.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let mut channel = req.get("channel").and_then(Value::as_str).unwrap_or("").to_string();
    let mut description = req.get("description").and_then(Value::as_str).unwrap_or("").to_string();
    if is_dubstep_like(&title) || is_dubstep_like(&description) {
        return Err("track rejected by anti-dubstep filter (dubstep / brostep / riddim / wobble bass)".to_string());
    }

    let scratch = data_dir
        .join("tmp")
        .join(format!("yt-music-{}-{}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("create music scratch dir: {e}"))?;

    let result = (|| -> Result<Value, String> {
        let mock_audio = std::env::var("OPENRO_YT_MUSIC_MOCK_AUDIO_FILE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let (source_path, source_mode) = if let Some(mock) = mock_audio {
            let p = PathBuf::from(&mock);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !p.is_file() || !(is_allowed_audio_ext(&ext) || matches!(ext.as_str(), "m4a" | "webm" | "opus")) {
                return Err("OPENRO_YT_MUSIC_MOCK_AUDIO_FILE must point to a local audio file".to_string());
            }
            (p, "mock")
        } else {
            let bin = find_yt_dlp_binary().ok_or_else(|| {
                "yt-dlp not found on PATH (install yt-dlp + ffmpeg, then click Install Music Runner)".to_string()
            })?;
            let out_tpl = scratch.join("source.%(ext)s");
            let argv = vec![
                "--no-playlist".to_string(),
                "--max-filesize".to_string(),
                "40M".to_string(),
                "-f".to_string(),
                "bestaudio".to_string(),
                "--no-warnings".to_string(),
                "--no-progress".to_string(),
                "--dump-json".to_string(),
                "--no-simulate".to_string(),
                "-o".to_string(),
                out_tpl.to_string_lossy().to_string(),
                "--".to_string(),
                youtube_url.clone(),
            ];
            let run = run_argv_with_timeout(&bin, &argv, Duration::from_secs(YT_MUSIC_DOWNLOAD_TIMEOUT_S))?;
            if run.timed_out {
                return Err(format!("yt-dlp download timed out after {YT_MUSIC_DOWNLOAD_TIMEOUT_S}s"));
            }
            if run.code != Some(0) {
                let err: String = run.stderr.trim().chars().take(400).collect();
                return Err(format!("yt-dlp download failed: {err}"));
            }
            if let Some(meta) = run
                .stdout
                .lines()
                .rev()
                .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            {
                let get = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                if title.is_empty() {
                    title = get("title");
                }
                if channel.is_empty() {
                    channel = meta
                        .get("channel")
                        .and_then(Value::as_str)
                        .or_else(|| meta.get("uploader").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string();
                }
                if description.is_empty() {
                    description = get("description");
                }
                let tags = yt_track_tags(&meta);
                if is_dubstep_like(&title) || is_dubstep_like(&description) || tags.iter().any(|t| is_dubstep_like(t)) {
                    return Err("track rejected by anti-dubstep filter (dubstep / brostep / riddim / wobble bass)".to_string());
                }
            }
            let downloaded = std::fs::read_dir(&scratch)
                .map_err(|e| format!("read music scratch dir: {e}"))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| {
                    p.is_file()
                        && p.file_name()
                            .map(|f| {
                                let f = f.to_string_lossy();
                                f.starts_with("source.") && !f.ends_with(".part")
                            })
                            .unwrap_or(false)
                })
                .ok_or_else(|| "yt-dlp finished but produced no audio file (file may exceed 40 MB)".to_string())?;
            (downloaded, "yt-dlp")
        };

        let mastered_path = scratch.join("mastered.wav");
        let (wav_bytes, mastering) = match find_ffmpeg_binary() {
            Some(ffmpeg) => {
                master_music_with_ffmpeg(&ffmpeg, &source_path, &mastered_path, max_s)?;
                let bytes = std::fs::read(&mastered_path).map_err(|e| format!("read mastered wav: {e}"))?;
                (bytes, "ffmpeg-loudnorm")
            }
            None => {
                let raw = std::fs::read(&source_path).map_err(|e| format!("read source audio: {e}"))?;
                let ext = source_path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
                (master_music_rust_fallback(&raw, ext.as_deref(), max_s)?, "rust-fallback")
            }
        };
        let stats = analyze_pcm16_wav(&wav_bytes)?;
        if stats.sample_rate != SAMPLE_RATE || stats.channels != 1 || stats.bits_per_sample != 16 {
            return Err(format!(
                "mastered music is not 44.1 kHz 16-bit mono (got {} Hz, {} ch, {} bit)",
                stats.sample_rate, stats.channels, stats.bits_per_sample
            ));
        }
        let rms_dbfs = pcm16_rms_dbfs(&wav_bytes);
        let sha256 = sha256_hex(&wav_bytes);

        let (license_status, credit_line) = classify_music_license(&title, &description, &channel);
        let fallback_name = if title.trim().is_empty() {
            format!("music_{genre_id}_{video_id}")
        } else {
            title.clone()
        };
        let raw_out = req.get("out_name").or_else(|| req.get("name")).and_then(Value::as_str);
        let stem = slug_music_name(raw_out, &fallback_name);
        let filename = sanitize_audio_filename(&format!("{stem}.wav"))?;
        let stem = filename.trim_end_matches(".wav").to_string();

        // Preview copy for `/api/assets/audio/<filename>` playback.
        let preview_dir = audio_assets_dir(data_dir);
        atomic_write(&preview_dir.join(&filename), &wav_bytes)?;

        let (out_rel, project_path, receipt_abs) = if let Some(root) = project_root {
            let rel = format!("assets/audio/{filename}");
            let abs = crate::opencloud::safe_rel(root, &rel).map_err(|e| e.msg)?;
            atomic_write(&abs, &wav_bytes)?;
            let receipt_rel = format!("assets/audio/{stem}.source.json");
            let receipt_abs = crate::opencloud::safe_rel(root, &receipt_rel).map_err(|e| e.msg)?;
            ensure_source_receipts_gitignored(root);
            (rel.clone(), Some(rel), receipt_abs)
        } else {
            (
                format!("game-assets/audio/{filename}"),
                None,
                preview_dir.join(format!("{stem}.source.json")),
            )
        };

        let duration_s = (stats.duration_ms as f64 / 100.0).round() / 10.0;
        let receipt = json!({
            "source": "youtube_nocopyright_music",
            "youtube_url": youtube_url,
            "video_id": video_id,
            "title": clean_display_text(&title, 160),
            "channel": clean_display_text(&channel, 80),
            "genre": genre_id,
            "license_status": license_status,
            "credit_line": credit_line,
            "sha256": sha256,
            "bytes": wav_bytes.len(),
            "duration_s": duration_s,
            "max_duration_s": max_s,
            "mastering": mastering,
            "imported_at_ms": now_ms(),
            "openro_version": env!("CARGO_PKG_VERSION"),
        });
        let receipt_bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| format!("encode receipt: {e}"))?;
        atomic_write(&receipt_abs, &receipt_bytes)?;

        let url = format!("/api/assets/audio/{filename}");
        let license_warning = match license_status {
            "non_profit_only" => Some("Track is marked non-profit only; do not use it in a monetized Roblox experience."),
            "verify_description" => Some("License not confirmed from metadata; read the YouTube description before publishing."),
            _ => None,
        };
        Ok(json!({
            "ok": true,
            "path": out_rel,
            "project_path": project_path,
            "filename": filename,
            "url": url,
            "wav_url": url,
            "receipt_path": receipt_abs.to_string_lossy().to_string(),
            "bytes": wav_bytes.len(),
            "sha256": sha256,
            "duration_ms": stats.duration_ms,
            "duration_s": duration_s,
            "max_duration_s": max_s,
            "sample_rate": stats.sample_rate,
            "channels": stats.channels,
            "bits_per_sample": stats.bits_per_sample,
            "peak_dbfs": stats.peak_dbfs,
            "rms_dbfs": rms_dbfs,
            "source": "youtube_nocopyright_music",
            "source_mode": source_mode,
            "mastering": mastering,
            "genre": genre_id,
            "title": clean_display_text(&title, 160),
            "channel": clean_display_text(&channel, 80),
            "youtube_url": youtube_url,
            "video_id": video_id,
            "license_status": license_status,
            "license_warning": license_warning,
            "credit_line": credit_line,
        }))
    })();

    let _ = std::fs::remove_dir_all(&scratch);
    result
}

// ---------------------------------------------------------------------------
// YouTube Copyright-Free Music Runner Installer (`<data_dir>/bin/openro-yt-music`)
// ---------------------------------------------------------------------------

const YT_MUSIC_RUNNER_SCRIPT: &str = r#"#!/usr/bin/env python3
"""OpenRO Copyright-Free YouTube Music Runner (v__OPENRO_VERSION__).

  --check
  --search --genre <g> [--query <q>] [--limit <n>]
  --download <url> --out <wav_path> [--max-duration <secs>]

Searches curated Roblox-trending copyright-free genres (zero dubstep), and masters
downloads to loop-ready 44.1 kHz 16-bit PCM mono WAV with ffmpeg loudnorm + fades.
Never invokes a shell. Prints compact JSON to stdout.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

VERSION = "__OPENRO_VERSION__"
GENRES = __OPENRO_GENRES__
BANNED = ("dubstep", "brostep", "riddim", "wobble bass")
URL_RE = re.compile(r"^https://(?:www\.youtube\.com/watch\?v=|youtu\.be/)([A-Za-z0-9_-]{11})(?:[?&][A-Za-z0-9_=&.-]*)?$")


def norm(text):
    t = (text or "").lower()
    for ch in "-_#":
        t = t.replace(ch, " ")
    return " ".join(t.split())


def dubstep_like(text):
    t = norm(text)
    return any(b in t for b in BANNED)


def classify(title, desc, channel):
    hay = norm(title + "\n" + desc + "\n" + channel)
    sb = "streambeats" in hay or "stream beats" in hay or "harris heller" in (channel or "").lower()
    if sb:
        status = "streambeats_dmca_free"
    elif any(k in hay for k in ("non profit", "nonprofit", "not for profit", "non commercial", "noncommercial")):
        status = "non_profit_only"
    elif ("royalty free" in hay or "royaltyfree" in hay) and any(k in hay for k in ("no copyright", "copyright free", "100%")):
        status = "royalty_free_verified"
    elif any(k in hay for k in ("free for profit", "free to use for profit", "free for commercial", "free commercial use")):
        status = "free_for_profit"
    else:
        status = "verify_description"
    credit = "StreamBeats by Harris Heller" if sb else "Prod. " + ((channel or "").strip()[:80] or "Unknown Producer")
    return status, credit


def check():
    ytdlp = shutil.which("yt-dlp")
    ffmpeg = shutil.which("ffmpeg")
    return {
        "ok": True,
        "tool": "openro-yt-music",
        "version": VERSION,
        "python": sys.version.split()[0],
        "yt_dlp": bool(ytdlp),
        "ffmpeg": bool(ffmpeg),
        "ready": bool(ytdlp and ffmpeg),
        "genres": list(GENRES.keys()),
        "install_hint": "pip install yt-dlp  (and install ffmpeg)",
    }


def search(genre, query, limit):
    ytdlp = shutil.which("yt-dlp")
    if not ytdlp:
        raise RuntimeError("yt-dlp not found on PATH")
    if query and dubstep_like(query):
        raise RuntimeError("dubstep searches are blocked")
    if genre and genre not in GENRES:
        raise RuntimeError("unknown genre '%s'" % genre)
    if not genre and not query:
        genre = "lofi"
    q = (query or "").lstrip("-").strip()
    sq = (q + " " + GENRES[genre]).strip() if genre else q + " no copyright free for profit instrumental"
    limit = max(1, min(25, int(limit)))
    n = min(36, limit + limit // 2 + 2)
    try:
        proc = subprocess.run(
            [ytdlp, "--dump-json", "--no-warnings", "--skip-download", "--ignore-errors", "--", "ytsearch%d:%s" % (n, sq)],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=35,
        )
        out = proc.stdout
    except subprocess.TimeoutExpired as exc:
        out = exc.stdout.decode() if isinstance(exc.stdout, bytes) else (exc.stdout or "")
    tracks, seen = [], set()
    for line in out.splitlines():
        try:
            v = json.loads(line)
        except Exception:
            continue
        vid = v.get("id") or ""
        if not re.fullmatch(r"[A-Za-z0-9_-]{11}", vid) or vid in seen:
            continue
        dur = float(v.get("duration") or 0)
        if dur < 45 or dur > 420:
            continue
        title, desc = v.get("title") or "", v.get("description") or ""
        chan = v.get("channel") or v.get("uploader") or ""
        if dubstep_like(title) or dubstep_like(desc) or any(dubstep_like(t) for t in (v.get("tags") or [])):
            continue
        seen.add(vid)
        status, credit = classify(title, desc, chan)
        tracks.append({
            "id": vid, "title": title[:160], "channel": chan[:80], "duration_s": round(dur, 1),
            "youtube_url": "https://www.youtube.com/watch?v=" + vid,
            "license_status": status, "credit_line": credit, "genre": genre or "custom",
        })
        if len(tracks) >= limit:
            break
    return {"ok": True, "genre": genre or "custom", "search_query": sq, "count": len(tracks), "tracks": tracks}


def download(url, out_path, max_dur):
    m = URL_RE.match((url or "").strip())
    if not m:
        raise ValueError("only https://www.youtube.com/watch?v=<id> or https://youtu.be/<id> URLs are allowed")
    vid = m.group(1)
    clean = "https://www.youtube.com/watch?v=" + vid
    ytdlp, ffmpeg = shutil.which("yt-dlp"), shutil.which("ffmpeg")
    if not ytdlp or not ffmpeg:
        raise RuntimeError("yt-dlp and ffmpeg are required on PATH")
    max_dur = max(10, min(110, int(max_dur)))
    tmp = tempfile.mkdtemp(prefix="openro-yt-music-")
    try:
        proc = subprocess.run(
            [ytdlp, "--no-playlist", "--max-filesize", "40M", "-f", "bestaudio", "--no-warnings", "--no-progress",
             "--dump-json", "--no-simulate", "-o", os.path.join(tmp, "source.%(ext)s"), "--", clean],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=240,
        )
        if proc.returncode != 0:
            raise RuntimeError("yt-dlp download failed: " + proc.stderr.strip()[:400])
        meta = {}
        for line in reversed(proc.stdout.splitlines()):
            try:
                meta = json.loads(line)
                break
            except Exception:
                continue
        title, desc = meta.get("title") or "", meta.get("description") or ""
        chan = meta.get("channel") or meta.get("uploader") or ""
        if dubstep_like(title) or dubstep_like(desc):
            raise RuntimeError("track rejected by anti-dubstep filter")
        src = next((os.path.join(tmp, f) for f in os.listdir(tmp) if f.startswith("source.") and not f.endswith(".part")), None)
        if not src:
            raise RuntimeError("yt-dlp produced no audio file")
        parent = os.path.dirname(os.path.abspath(out_path))
        if parent:
            os.makedirs(parent, exist_ok=True)
        tmp_out = out_path + ".openro.tmp.wav"
        chain = ("atrim=0:%d,asetpts=PTS-STARTPTS,loudnorm=I=-16:TP=-1.8:LRA=8,aresample=44100,"
                 "afade=t=in:st=0:d=0.15,areverse,afade=t=in:st=0:d=0.35,areverse") % max_dur
        ff = subprocess.run(
            [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", src, "-vn", "-map_metadata", "-1",
             "-bitexact", "-af", chain, "-ar", "44100", "-ac", "1", "-c:a", "pcm_s16le", "-f", "wav", tmp_out],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=180,
        )
        if ff.returncode != 0:
            raise RuntimeError("ffmpeg mastering failed: " + ff.stderr.strip()[:400])
        os.replace(tmp_out, out_path)
        status, credit = classify(title, desc, chan)
        return {
            "ok": True, "out": out_path, "youtube_url": clean, "title": title[:160], "channel": chan[:80],
            "license_status": status, "credit_line": credit, "sample_rate": 44100, "channels": 1,
            "bits_per_sample": 16, "max_duration_s": max_dur, "bytes": os.path.getsize(out_path),
            "source": "youtube_nocopyright_music",
        }
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    p = argparse.ArgumentParser(description="OpenRO Copyright-Free YouTube Music Runner")
    p.add_argument("--check", action="store_true")
    p.add_argument("--version", action="store_true")
    p.add_argument("--search", action="store_true")
    p.add_argument("--genre", default="")
    p.add_argument("--query", default="")
    p.add_argument("--limit", type=int, default=10)
    p.add_argument("--download", default="")
    p.add_argument("--out", default="")
    p.add_argument("--max-duration", type=float, default=90)
    a = p.parse_args()
    try:
        if a.version:
            res = {"ok": True, "tool": "openro-yt-music", "version": VERSION}
        elif a.check:
            res = check()
        elif a.search:
            res = search(a.genre.strip().lower(), a.query.strip(), a.limit)
        elif a.download:
            if not a.out.strip():
                print(json.dumps({"ok": False, "error": "--out is required"}), file=sys.stderr)
                return 2
            res = download(a.download, a.out.strip(), a.max_duration)
        else:
            print(json.dumps({"ok": False, "error": "use --check, --search or --download"}), file=sys.stderr)
            return 2
        print(json.dumps(res))
        return 0
    except ValueError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}), file=sys.stderr)
        return 2
    except Exception as exc:
        print(json.dumps({"ok": False, "error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
"#;

/// Install the Python 3 copyright-free YouTube music runner at `<data_dir>/bin/openro-yt-music`
/// (`0755` on Unix) and run `--check` when `python3` is available.
pub fn install_yt_music_runner(data_dir: &Path) -> Result<Value, String> {
    let bin_dir = data_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("create runner bin dir '{}': {e}", bin_dir.display()))?;
    let target = yt_music_runner_path(data_dir);
    let genres_py = {
        let mut m = serde_json::Map::new();
        for g in MUSIC_GENRES.iter() {
            m.insert(g.id.to_string(), Value::String(g.query.to_string()));
        }
        Value::Object(m).to_string()
    };
    let script = YT_MUSIC_RUNNER_SCRIPT
        .replace("__OPENRO_VERSION__", env!("CARGO_PKG_VERSION"))
        .replace("__OPENRO_GENRES__", &genres_py);
    let tmp = bin_dir.join(format!(".openro-yt-music.{}.tmp", std::process::id()));
    std::fs::write(&tmp, script.as_bytes()).map_err(|e| format!("write yt music runner script: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod 0755 yt music runner: {e}"))?;
    }

    std::fs::rename(&tmp, &target).map_err(|e| format!("commit yt music runner script: {e}"))?;

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
            if let Ok(parsed) = serde_json::from_str::<Value>(String::from_utf8_lossy(&out.stdout).trim()) {
                check_json = Some(parsed);
            }
        }
    }

    Ok(json!({
        "ok": true,
        "path": target.to_string_lossy().to_string(),
        "command": "openro-yt-music",
        "adapter_id": "youtube-nocopyright-music-cli",
        "executable": true,
        "python_available": python_available,
        "yt_dlp_available": find_yt_dlp_binary().is_some(),
        "ffmpeg_available": find_ffmpeg_binary().is_some(),
        "check": check_json,
        "genres": MUSIC_GENRES.iter().map(|g| g.id).collect::<Vec<_>>(),
        "install_hint": "Requires yt-dlp + ffmpeg (pip install yt-dlp)",
    }))
}
