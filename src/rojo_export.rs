//! Zero-dependency Roblox Studio & Rojo Ship Pack `.zip` exporter for OpenRO.
//!
//! Emits a genuine PKWARE ZIP archive (readable by Python `zipfile`, Info-ZIP
//! `unzip`, macOS Finder, Windows Explorer, and Rojo 7) containing:
//! - `game.rbxlx` — authentic Roblox Studio `<roblox version="4">` XML place
//! - `default.project.json` — Rojo 7 project manifest wired to `src/{server,client,shared}`
//! - `src/server/*.server.luau`, `src/client/*.client.luau`, `src/shared/*.luau`
//! - `asset-manifest.json` & uploaded `assets/*.{png,webp,jpg}` files
//! - `luau-audit-report.json` — live Static Luau Prover diagnostics
//! - `STUDIO-UPLOAD-CHECKLIST.md` — plain-English Studio & Rojo publishing checklist.

use crate::assets::{build_asset_manifest_json, sanitize_filename, AssetManager, AssetMetadata};
use crate::luau_check::AnalysisReport;
use crate::place::LocalPlace;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic DOS timestamp (`2026-11-22 12:00:00`) for reproducible archives.
const DOS_MOD_TIME: u16 = 0x6000;
const DOS_MOD_DATE: u16 = 0x5D76;

/// One image asset packaged into `assets/<filename>` inside the Ship Pack.
#[derive(Clone, Debug)]
pub struct ShipPackAsset {
    pub metadata: AssetMetadata,
    pub bytes: Vec<u8>,
}

/// Complete Studio & Rojo Ship Pack archive plus summary metadata.
#[derive(Clone, Debug)]
pub struct ShipPackBundle {
    pub zip_bytes: Vec<u8>,
    pub file_list: Vec<String>,
    pub game_name: String,
    pub script_count: usize,
    pub instance_count: usize,
    pub asset_count: usize,
    pub audit_error_count: usize,
    pub audit_warning_count: usize,
}

impl ShipPackBundle {
    pub fn filename_slug(&self) -> String {
        let mut slug = String::new();
        let mut prev_dash = false;
        for ch in self.game_name.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash && !slug.is_empty() {
                slug.push('-');
                prev_dash = true;
            }
        }
        let trimmed = slug.trim_matches('-');
        if trimmed.is_empty() {
            "openro-ship-pack.zip".to_string()
        } else {
            format!("{trimmed}-ship-pack.zip")
        }
    }
}

struct ZipEntryMeta {
    name: String,
    crc32: u32,
    size: u32,
    local_header_offset: u32,
}

/// Pure-Rust PKWARE ZIP archive writer (Stored method 0, IEEE CRC-32, UTF-8 names).
pub struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<ZipEntryMeta>,
    seen: BTreeSet<String>,
}

impl Default for ZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipWriter {
    pub fn new() -> Self {
        ZipWriter {
            buf: Vec::with_capacity(64 * 1024),
            entries: Vec::new(),
            seen: BTreeSet::new(),
        }
    }

    pub fn add_file(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let clean = name.trim();
        if clean.is_empty()
            || clean.starts_with('/')
            || clean.contains('\\')
            || clean.contains(':')
            || clean.bytes().any(|b| b < 0x20 || b == 0x7f)
            || clean
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(format!("invalid archive entry path '{clean}'"));
        }
        if !self.seen.insert(clean.to_string()) {
            return Err(format!("duplicate archive entry '{clean}'"));
        }
        let name_bytes = clean.as_bytes();
        let name_len = u16::try_from(name_bytes.len())
            .map_err(|_| format!("archive entry path too long: {clean}"))?;
        let size = u32::try_from(data.len())
            .map_err(|_| format!("archive entry exceeds 4 GiB ZIP limit: {clean}"))?;
        let offset = u32::try_from(self.buf.len())
            .map_err(|_| "archive offset exceeds 4 GiB ZIP limit".to_string())?;
        let crc = crc32(data);

        // Local File Header (30 bytes + filename + uncompressed payload)
        self.buf.extend_from_slice(&0x0403_4B50u32.to_le_bytes()); // signature
        self.buf.extend_from_slice(&20u16.to_le_bytes()); // version needed (2.0)
        self.buf.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 filename flag
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // method 0 = Stored
        self.buf.extend_from_slice(&DOS_MOD_TIME.to_le_bytes());
        self.buf.extend_from_slice(&DOS_MOD_DATE.to_le_bytes());
        self.buf.extend_from_slice(&crc.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes()); // compressed size
        self.buf.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        self.buf.extend_from_slice(&name_len.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
        self.buf.extend_from_slice(name_bytes);
        self.buf.extend_from_slice(data);

        self.entries.push(ZipEntryMeta {
            name: clean.to_string(),
            crc32: crc,
            size,
            local_header_offset: offset,
        });
        Ok(())
    }

    pub fn finish(mut self) -> Result<(Vec<u8>, Vec<String>), String> {
        let cd_offset = u32::try_from(self.buf.len())
            .map_err(|_| "central directory offset exceeds ZIP limit".to_string())?;
        let entry_count = u16::try_from(self.entries.len())
            .map_err(|_| "too many entries for standard ZIP".to_string())?;

        let mut names = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            let name_len = name_bytes.len() as u16;
            self.buf.extend_from_slice(&0x0201_4B50u32.to_le_bytes()); // central header sig
            self.buf.extend_from_slice(&0x0314u16.to_le_bytes()); // version made by: Unix, 2.0
            self.buf.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
            self.buf.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 flag
            self.buf.extend_from_slice(&0u16.to_le_bytes()); // method: Stored
            self.buf.extend_from_slice(&DOS_MOD_TIME.to_le_bytes());
            self.buf.extend_from_slice(&DOS_MOD_DATE.to_le_bytes());
            self.buf.extend_from_slice(&entry.crc32.to_le_bytes());
            self.buf.extend_from_slice(&entry.size.to_le_bytes());
            self.buf.extend_from_slice(&entry.size.to_le_bytes());
            self.buf.extend_from_slice(&name_len.to_le_bytes());
            self.buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
            self.buf.extend_from_slice(&0u16.to_le_bytes()); // comment len
            self.buf.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            self.buf.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            self.buf
                .extend_from_slice(&(0o100644u32 << 16).to_le_bytes()); // Unix rw-r--r--
            self.buf
                .extend_from_slice(&entry.local_header_offset.to_le_bytes());
            self.buf.extend_from_slice(name_bytes);
            names.push(entry.name.clone());
        }

        let cd_size = u32::try_from(self.buf.len() - (cd_offset as usize))
            .map_err(|_| "central directory size exceeds ZIP limit".to_string())?;

        // End of Central Directory Record (22 bytes)
        self.buf.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // disk number
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // disk with central dir
        self.buf.extend_from_slice(&entry_count.to_le_bytes());
        self.buf.extend_from_slice(&entry_count.to_le_bytes());
        self.buf.extend_from_slice(&cd_size.to_le_bytes());
        self.buf.extend_from_slice(&cd_offset.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // archive comment len

        Ok((self.buf, names))
    }
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

fn sanitize_script_stem(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else if ch == '.' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "Script".to_string()
    } else {
        out
    }
}

#[derive(Clone, Debug)]
struct MappedScript {
    place_path: String,
    zip_path: String,
    service: String,
    leaf_name: String,
    source: String,
}

fn map_scripts_for_rojo(scripts: &BTreeMap<String, String>) -> Vec<MappedScript> {
    let mut mapped = Vec::with_capacity(scripts.len());
    let mut used_zip_paths: BTreeSet<String> = BTreeSet::new();
    // Reserve the generated metadata module path so user scripts named PlaceMeta never collide.
    used_zip_paths.insert("src/shared/PlaceMeta.luau".to_string());

    for (place_path, source) in scripts {
        let segs: Vec<&str> = place_path.split('.').filter(|s| !s.is_empty()).collect();
        let service = segs.first().copied().unwrap_or("ServerScriptService");
        let leaf = segs.last().copied().unwrap_or("Script");
        let stem = if segs.len() > 2 && service == "Workspace" {
            sanitize_script_stem(&segs[1..].join("_"))
        } else if segs.len() > 2
            && service == "StarterPlayer"
            && segs[1] == "StarterPlayerScripts"
        {
            sanitize_script_stem(&segs[2..].join("_"))
        } else if segs.len() > 2 {
            sanitize_script_stem(&segs[1..].join("_"))
        } else {
            sanitize_script_stem(leaf)
        };

        let (dir, ext) = match service {
            "StarterGui" | "StarterPlayer" => ("src/client", "client.luau"),
            "ReplicatedStorage" => ("src/shared", "luau"),
            _ => ("src/server", "server.luau"),
        };

        let mut candidate = format!("{dir}/{stem}.{ext}");
        let mut idx = 2usize;
        while !used_zip_paths.insert(candidate.clone()) {
            candidate = format!("{dir}/{stem}_{idx}.{ext}");
            idx += 1;
        }

        mapped.push(MappedScript {
            place_path: place_path.clone(),
            zip_path: candidate,
            service: service.to_string(),
            leaf_name: leaf.to_string(),
            source: source.clone(),
        });
    }
    mapped
}

/// Generate a Rojo 7 `default.project.json` mapping `src/{server,client,shared}` cleanly.
pub fn build_rojo_project_json(game_name: &str, scripts: &BTreeMap<String, String>) -> Value {
    let mapped = map_scripts_for_rojo(scripts);

    let mut starter_gui_tree = Map::new();
    starter_gui_tree.insert("$className".to_string(), json!("StarterGui"));

    let mut starter_player_scripts_tree = Map::new();
    starter_player_scripts_tree.insert("$className".to_string(), json!("StarterPlayerScripts"));

    let mut server_tree = Map::new();
    server_tree.insert("$className".to_string(), json!("ServerScriptService"));

    let mut replicated_tree = Map::new();
    replicated_tree.insert("$className".to_string(), json!("ReplicatedStorage"));
    replicated_tree.insert(
        "PlaceMeta".to_string(),
        json!({ "$path": "src/shared/PlaceMeta.luau" }),
    );

    let mut workspace_tree = Map::new();
    workspace_tree.insert("$className".to_string(), json!("Workspace"));
    workspace_tree.insert("$properties".to_string(), json!({ "Gravity": 196.2 }));

    let insert_unique = |tree: &mut Map<String, Value>, base_key: &str, path: &str| {
        let mut key = base_key.to_string();
        let mut idx = 2usize;
        while tree.contains_key(&key) {
            key = format!("{base_key}_{idx}");
            idx += 1;
        }
        tree.insert(key, json!({ "$path": path }));
    };

    for item in &mapped {
        match item.service.as_str() {
            "StarterGui" => {
                insert_unique(&mut starter_gui_tree, &item.leaf_name, &item.zip_path);
            }
            "StarterPlayer" => {
                insert_unique(
                    &mut starter_player_scripts_tree,
                    &item.leaf_name,
                    &item.zip_path,
                );
            }
            "ReplicatedStorage" => {
                insert_unique(&mut replicated_tree, &item.leaf_name, &item.zip_path);
            }
            "Workspace" => {
                let key = sanitize_script_stem(
                    item.place_path
                        .strip_prefix("Workspace.")
                        .unwrap_or(&item.leaf_name),
                );
                insert_unique(&mut workspace_tree, &key, &item.zip_path);
            }
            _ => {
                insert_unique(&mut server_tree, &item.leaf_name, &item.zip_path);
            }
        }
    }

    json!({
        "name": game_name,
        "tree": {
            "$className": "DataModel",
            "Workspace": Value::Object(workspace_tree),
            "ServerScriptService": Value::Object(server_tree),
            "ReplicatedStorage": Value::Object(replicated_tree),
            "StarterGui": Value::Object(starter_gui_tree),
            "StarterPlayer": {
                "$className": "StarterPlayer",
                "StarterPlayerScripts": Value::Object(starter_player_scripts_tree)
            },
            "Lighting": {
                "$className": "Lighting"
            },
            "SoundService": {
                "$className": "SoundService"
            }
        }
    })
}

/// Generate a concise, plain-English Roblox Studio & Rojo upload checklist.
pub fn build_upload_checklist(
    game_name: &str,
    place: &LocalPlace,
    assets: &[ShipPackAsset],
    audit: &AnalysisReport,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {game_name} — Studio & Rojo Ship Pack Checklist\n\n"));
    out.push_str("## Package Summary\n");
    out.push_str(&format!(
        "- **Place File:** `game.rbxlx` (`{}` instances, `{}` Luau scripts)\n",
        place.instances().len(),
        place.scripts().len()
    ));
    out.push_str(&format!(
        "- **Static Luau Audit:** {} (`{}` error(s), `{}` warning(s) — see `luau-audit-report.json`)\n",
        if audit.is_ok() { "PASSED" } else { "NEEDS REVIEW" },
        audit.error_count(),
        audit.warning_count()
    ));
    out.push_str(&format!(
        "- **Bundled Art Assets:** `{}` file(s) in `assets/` (see `asset-manifest.json`)\n\n",
        assets.len()
    ));

    out.push_str("## Option A — Open Directly in Roblox Studio\n");
    out.push_str("1. Unzip this archive to a local folder.\n");
    out.push_str("2. Open `game.rbxlx` in Roblox Studio (`File -> Open from File...`).\n");
    out.push_str("3. Enable **Studio Access to API Services** (`Game Settings -> Security`) if your place uses `DataStoreService`.\n");
    out.push_str("4. Press **Play (F5)** to verify all scripts and 3D parts in Studio Output.\n\n");

    out.push_str("## Option B — Sync with Rojo 7\n");
    out.push_str("1. Install Rojo (`aftman` or `cargo install rojo`).\n");
    out.push_str("2. Run `rojo serve default.project.json` in the unzipped folder and connect the Roblox Studio Rojo plugin, or build with `rojo build default.project.json -o game.rbxl`.\n");
    out.push_str("3. Source scripts live in `src/server/`, `src/client/`, and `src/shared/`.\n\n");

    if !assets.is_empty() {
        out.push_str("## Art Asset Upload Steps (`assets/`)\n");
        out.push_str("1. Open **Asset Manager -> Bulk Import** in Roblox Studio and select the files inside `assets/`.\n");
        out.push_str("2. Copy each generated `rbxassetid://<id>` into its matching `Decal.Texture` and `ImageLabel.Image` listed in `asset-manifest.json`:\n");
        for a in assets {
            out.push_str(&format!(
                "   - `assets/{}` -> `{}` (`{}`, `{}`)\n",
                a.metadata.filename, a.metadata.title, a.metadata.rarity, a.metadata.element
            ));
        }
        out.push('\n');
    }

    out.push_str("## Pre-Publish Verification\n");
    out.push_str("- Confirm `luau-audit-report.json` shows `\"error_count\": 0`.\n");
    out.push_str("- Test with **Server & Clients** in Studio to confirm server-authoritative remotes and leaderstats.\n");
    out.push_str("- Publish to Roblox via `File -> Publish to Roblox...`.\n");
    out
}

/// Build the complete Studio & Rojo Ship Pack `.zip` archive from a `LocalPlace` and asset list.
pub fn build_rojo_ship_pack(
    place: &LocalPlace,
    assets: &[ShipPackAsset],
) -> Result<ShipPackBundle, String> {
    let game_name = place.game_name().to_string();
    let audit = place.analyze();
    let rbxlx_xml = place.to_rbxlx_xml();
    let project_json = build_rojo_project_json(&game_name, place.scripts());
    let mapped_scripts = map_scripts_for_rojo(place.scripts());

    let meta_list: Vec<AssetMetadata> = assets.iter().map(|a| a.metadata.clone()).collect();
    let asset_manifest = build_asset_manifest_json(&game_name, &meta_list);
    let checklist = build_upload_checklist(&game_name, place, assets, &audit);

    let mut zip = ZipWriter::new();

    // 1. Authentic Roblox Studio place file
    zip.add_file("game.rbxlx", rbxlx_xml.as_bytes())?;

    // 2. Rojo 7 project manifest
    let project_text = serde_json::to_string_pretty(&project_json)
        .map_err(|e| format!("serialize default.project.json: {e}"))?;
    zip.add_file("default.project.json", project_text.as_bytes())?;

    // 3. Shared PlaceMeta Luau module (always ensures `src/shared` is non-empty and Rojo-valid)
    let escaped_game_name = game_name.replace('\\', "\\\\").replace('"', "\\\"");
    let place_meta_luau = format!(
        "-- PlaceMeta -- generated by OpenRO Studio & Rojo Ship Pack\n\
         return {{\n\
         \x20   GameName = \"{escaped_game_name}\",\n\
         \x20   ScriptCount = {},\n\
         \x20   InstanceCount = {},\n\
         \x20   AssetCount = {},\n\
         }}\n",
        place.scripts().len(),
        place.instances().len(),
        assets.len()
    );
    zip.add_file("src/shared/PlaceMeta.luau", place_meta_luau.as_bytes())?;

    // 4. All place Luau scripts under src/{server,client,shared}/*.luau
    for item in &mapped_scripts {
        zip.add_file(&item.zip_path, item.source.as_bytes())?;
    }

    // 5. Asset manifest + all uploaded artwork files
    let manifest_text = serde_json::to_string_pretty(&asset_manifest)
        .map_err(|e| format!("serialize asset-manifest.json: {e}"))?;
    zip.add_file("asset-manifest.json", manifest_text.as_bytes())?;

    let mut included_assets = 0usize;
    for asset in assets {
        let safe_name = sanitize_filename(&asset.metadata.filename)?;
        zip.add_file(&format!("assets/{safe_name}"), &asset.bytes)?;
        included_assets += 1;
    }

    // 6. Live Static Luau Audit Report
    let audit_text = serde_json::to_string_pretty(&audit.to_json())
        .map_err(|e| format!("serialize luau-audit-report.json: {e}"))?;
    zip.add_file("luau-audit-report.json", audit_text.as_bytes())?;

    // 7. Plain-English Studio Upload Checklist
    zip.add_file("STUDIO-UPLOAD-CHECKLIST.md", checklist.as_bytes())?;

    let (zip_bytes, file_list) = zip.finish()?;
    Ok(ShipPackBundle {
        zip_bytes,
        file_list,
        game_name,
        script_count: place.scripts().len(),
        instance_count: place.instances().len(),
        asset_count: included_assets,
        audit_error_count: audit.error_count(),
        audit_warning_count: audit.warning_count(),
    })
}

/// Convenience helper loading all stored image assets from an `AssetManager` (when present)
/// and packaging them with the Place into a Studio & Rojo Ship Pack `.zip`.
pub fn build_rojo_ship_pack_with_manager(
    place: &LocalPlace,
    asset_mgr: Option<&AssetManager>,
) -> Result<ShipPackBundle, String> {
    let mut assets = Vec::new();
    if let Some(mgr) = asset_mgr {
        for meta in mgr.list() {
            if let Ok((bytes, _)) = mgr.read_bytes(&meta.filename) {
                assets.push(ShipPackAsset {
                    metadata: meta,
                    bytes,
                });
            }
        }
    }
    build_rojo_ship_pack(place, &assets)
}
