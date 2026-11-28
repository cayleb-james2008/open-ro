//! Headless Blender UV-textured stylized mesh pipeline for Roblox (`POST /api/tools/blender/generate`).
//!
//! Accepts either:
//! - a structured description `{ name, parts: [{ shape, size, pos, rot, color, glow? }], ... }`, or
//! - an agent-written Python `bpy` `script` (or both).
//!
//! Pipeline inside headless Blender:
//! build -> smart UV unwrap + island pack -> Cycles bake of diffuse colour x ambient occlusion
//! (warm shadow tint) to PNG -> export Y-up OBJ in studs (+ separate `<name>_glow` OBJ/PNG when
//! glow parts exist) -> `<name>.manifest.json` + merged `manifest.json` -> 2-angle preview PNGs.
//! Meshes exceeding 20,000 triangles are hard-refused before unwrapping/baking.

use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::library::{detect_blender, mesh_assets_dir};

/// Hard Roblox mesh triangle limit enforced by this pipeline.
pub const MAX_TRIS: u64 = 20_000;

/// Maximum execution time (seconds) for headless Blender runs and agent scripts.
pub const SCRIPT_TIMEOUT_SECS: u64 = 90;

/// Maximum byte length of an agent-supplied `bpy` script.
const MAX_SCRIPT_BYTES: usize = 256 * 1024;

/// Maximum number of structured parts accepted in a single request.
const MAX_PARTS: usize = 500;

/// Supported primitive shapes in structured `parts` descriptions.
pub const SUPPORTED_SHAPES: &[&str] = &["box", "cylinder", "cone", "sphere", "wedge", "torus"];

/// Best-effort denylist of tokens refused in agent-supplied `bpy` scripts.
/// Documented as best-effort defense-in-depth alongside the 90 s timeout and isolated scratch cwd.
pub const SCRIPT_DENYLIST: &[&str] = &[
    "os.system",
    "subprocess",
    "socket",
    "urllib",
    "shutil.rmtree",
    "__import__",
    "os.popen",
    "os.exec",
    "os.spawn",
    "importlib",
    "ctypes",
    "http.client",
];

const MESH_LIB_PY: &str = include_str!("mesh_lib.py");
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

/// RAII cleanup guard for an isolated scratch directory.
struct ScratchGuard(PathBuf);

impl Drop for ScratchGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Return Blender availability and mesh pipeline capabilities for `GET /api/tools/blender/status`.
pub fn blender_status(blender_override: Option<&str>) -> Value {
    let det = detect_blender(blender_override);
    json!({
        "ok": true,
        "available": det.detected,
        "blender_detected": det.detected,
        "blender_path": det.path,
        "blender_version": det.version,
        "max_tris": MAX_TRIS,
        "script_timeout_secs": SCRIPT_TIMEOUT_SECS,
        "supported_shapes": SUPPORTED_SHAPES,
        "modes": ["parts", "script", "preset"],
        "units": "studs",
        "up_axis": "Y",
    })
}

/// Validate an agent-supplied `bpy` script against the best-effort safety denylist and size cap.
pub fn validate_script(script: &str) -> Result<(), String> {
    if script.trim().is_empty() {
        return Err("script must not be empty".to_string());
    }
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(format!(
            "script is too large ({} bytes; maximum is {} bytes)",
            script.len(),
            MAX_SCRIPT_BYTES
        ));
    }
    for banned in SCRIPT_DENYLIST {
        if script.contains(banned) {
            return Err(format!(
                "script refused: '{banned}' is not allowed in Blender mesh scripts"
            ));
        }
    }
    Ok(())
}

/// Normalize a mesh name into a safe lowercase slug preserving underscores and hyphens.
pub fn slug_for_mesh_name(raw: Option<&str>) -> String {
    let base = raw.unwrap_or("mesh").trim();
    let stripped = base
        .strip_suffix(".obj")
        .or_else(|| base.strip_suffix(".OBJ"))
        .unwrap_or(base);
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if (ch == '_' || ch == '-' || ch.is_whitespace()) && !prev_sep && !slug.is_empty() {
            slug.push(if ch == '-' { '-' } else { '_' });
            prev_sep = true;
        }
        if slug.len() >= 64 {
            break;
        }
    }
    let clean = slug.trim_matches(|c| c == '_' || c == '-');
    if clean.is_empty() {
        "mesh".to_string()
    } else {
        clean.to_string()
    }
}

/// Validate a hex colour string (`#RGB`, `RGB`, `#RRGGBB`, or `RRGGBB`) and normalize to `#rrggbb`.
pub fn validate_hex_color(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    let expanded = if trimmed.len() == 3 {
        let mut s = String::with_capacity(6);
        for c in trimmed.chars() {
            s.push(c);
            s.push(c);
        }
        s
    } else if trimmed.len() == 6 {
        trimmed.to_string()
    } else {
        return Err(format!(
            "colour '{raw}' is not a valid hex colour (expected #RRGGBB or #RGB)"
        ));
    };
    if !expanded.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "colour '{raw}' is not a valid hex colour (expected #RRGGBB or #RGB)"
        ));
    }
    Ok(format!("#{}", expanded.to_ascii_lowercase()))
}

fn parse_vec3(val: &Value, field: &str, positive_only: bool) -> Result<[f64; 3], String> {
    let arr = val
        .as_array()
        .ok_or_else(|| format!("'{field}' must be an array of 3 numbers"))?;
    if arr.len() != 3 {
        return Err(format!(
            "'{field}' must have exactly 3 numbers, got {}",
            arr.len()
        ));
    }
    let mut out = [0.0_f64; 3];
    for (i, item) in arr.iter().enumerate() {
        let n = item
            .as_f64()
            .ok_or_else(|| format!("'{field}[{i}]' must be a number"))?;
        if !n.is_finite() {
            return Err(format!("'{field}[{i}]' must be a finite number"));
        }
        if positive_only {
            if n <= 0.0 || n > 10_000.0 {
                return Err(format!(
                    "'{field}[{i}]' must be greater than 0 and at most 10000 studs"
                ));
            }
        } else if !(-100_000.0..=100_000.0).contains(&n) {
            return Err(format!(
                "'{field}[{i}]' is out of range (-100000..=100000)"
            ));
        }
        out[i] = n;
    }
    Ok(out)
}

fn normalize_shape(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "box" | "cube" => Ok("box"),
        "cylinder" | "cyl" => Ok("cylinder"),
        "cone" => Ok("cone"),
        "sphere" => Ok("sphere"),
        "wedge" => Ok("wedge"),
        "torus" => Ok("torus"),
        other => Err(format!(
            "unknown part shape '{other}' (expected one of: box, cylinder, cone, sphere, wedge, torus)"
        )),
    }
}

fn parse_parts(val: &Value) -> Result<Vec<Value>, String> {
    let arr = val
        .as_array()
        .ok_or_else(|| "'parts' must be an array of part objects".to_string())?;
    if arr.len() > MAX_PARTS {
        return Err(format!(
            "too many parts ({}; maximum is {MAX_PARTS})",
            arr.len()
        ));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("parts[{idx}] must be an object"))?;
        let shape_raw = obj
            .get("shape")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("parts[{idx}] is missing a 'shape' string"))?;
        let shape = normalize_shape(shape_raw)?;
        let size = match obj.get("size") {
            Some(v) => parse_vec3(v, &format!("parts[{idx}].size"), true)?,
            None => [1.0, 1.0, 1.0],
        };
        let pos = match obj.get("pos").or_else(|| obj.get("position")) {
            Some(v) => parse_vec3(v, &format!("parts[{idx}].pos"), false)?,
            None => [0.0, 0.0, 0.0],
        };
        let rot = match obj.get("rot").or_else(|| obj.get("rotation")) {
            Some(v) => parse_vec3(v, &format!("parts[{idx}].rot"), false)?,
            None => [0.0, 0.0, 0.0],
        };
        let color = match obj.get("color").or_else(|| obj.get("colour")) {
            Some(v) => {
                let s = v
                    .as_str()
                    .ok_or_else(|| format!("parts[{idx}].color must be a hex colour string"))?;
                validate_hex_color(s)?
            }
            None => "#b8b8c0".to_string(),
        };
        let glow = match obj.get("glow") {
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("parts[{idx}].glow must be a boolean"))?,
            None => false,
        };
        let segments = match obj.get("segments") {
            Some(v) if !v.is_null() => {
                let n = v
                    .as_u64()
                    .ok_or_else(|| format!("parts[{idx}].segments must be a positive integer"))?;
                if !(3..=64).contains(&n) {
                    return Err(format!("parts[{idx}].segments must be between 3 and 64"));
                }
                Some(n)
            }
            _ => None,
        };
        let tube = match obj.get("tube") {
            Some(v) if !v.is_null() => {
                let t = v
                    .as_f64()
                    .ok_or_else(|| format!("parts[{idx}].tube must be a positive number"))?;
                if !t.is_finite() || t <= 0.0 {
                    return Err(format!("parts[{idx}].tube must be greater than 0"));
                }
                Some(t)
            }
            _ => None,
        };
        if shape == "torus" {
            let tube_d = tube.unwrap_or(size[1]);
            if size[0].min(size[2]) <= tube_d {
                return Err(format!(
                    "parts[{idx}] torus size [{}, {}, {}] is invalid: outer X and Z diameters must exceed tube thickness ({tube_d})",
                    size[0], size[1], size[2]
                ));
            }
        }
        out.push(json!({
            "shape": shape,
            "size": size,
            "pos": pos,
            "rot": rot,
            "color": color,
            "glow": glow,
            "segments": segments,
            "tube": tube,
        }));
    }
    Ok(out)
}

fn parse_anchors(val: Option<&Value>) -> Result<Vec<Value>, String> {
    let Some(v) = val else {
        return Ok(Vec::new());
    };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let arr = v
        .as_array()
        .ok_or_else(|| "'anchors' must be an array of { name, pos } objects".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("anchors[{idx}] must be an object"))?;
        let name = obj
            .get("name")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or_else(|| format!("anchors[{idx}].name must be a non-empty string (<=64 chars)"))?;
        let pos_v = obj
            .get("pos")
            .ok_or_else(|| format!("anchors[{idx}] is missing 'pos'"))?;
        let pos = parse_vec3(pos_v, &format!("anchors[{idx}].pos"), false)?;
        out.push(json!({ "name": name, "pos": pos }));
    }
    Ok(out)
}

fn validate_rel_segment(seg: &str) -> Result<(), String> {
    if seg.is_empty() || seg.len() > 64 {
        return Err("path segment must be 1..=64 characters".to_string());
    }
    if seg == "." || seg == ".." || seg.starts_with('.') {
        return Err(format!("invalid path segment '{seg}'"));
    }
    if !seg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!(
            "invalid path segment '{seg}': only ASCII letters, digits, '.', '-', '_' allowed"
        ));
    }
    Ok(())
}

/// Resolve an optional relative `out_dir` under `<data_dir>/meshes-out` (or the default
/// `<data_dir>/game-assets/meshes` when omitted).
fn resolve_out_dir(
    data_dir: &Path,
    out_dir_opt: Option<&str>,
) -> Result<(PathBuf, Option<String>), String> {
    let Some(raw) = out_dir_opt.map(str::trim).filter(|s| !s.is_empty()) else {
        let dir = mesh_assets_dir(data_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("create mesh assets dir: {e}"))?;
        let canon = std::fs::canonicalize(&dir)
            .map_err(|e| format!("canonicalize mesh assets dir: {e}"))?;
        return Ok((canon, None));
    };
    if raw.len() > 160
        || raw.contains('\0')
        || raw.contains('\\')
        || raw.contains(':')
        || raw.starts_with('/')
    {
        return Err(format!(
            "invalid out_dir '{raw}': must be a relative path under meshes-out without traversal"
        ));
    }
    let mut clean_segs: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        if seg.is_empty() {
            continue;
        }
        validate_rel_segment(seg).map_err(|e| format!("invalid out_dir '{raw}': {e}"))?;
        clean_segs.push(seg);
    }
    if clean_segs.is_empty() || clean_segs.len() > 8 {
        return Err(format!(
            "invalid out_dir '{raw}': expected 1..=8 relative path segments"
        ));
    }
    let rel_norm = clean_segs.join("/");
    let base = data_dir.join("meshes-out");
    std::fs::create_dir_all(&base).map_err(|e| format!("create meshes-out dir: {e}"))?;
    let canon_base =
        std::fs::canonicalize(&base).map_err(|e| format!("canonicalize meshes-out dir: {e}"))?;
    let mut target = canon_base.clone();
    for seg in &clean_segs {
        target.push(seg);
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("create out_dir '{rel_norm}': {e}"))?;
    let canon_target =
        std::fs::canonicalize(&target).map_err(|e| format!("canonicalize out_dir: {e}"))?;
    if !canon_target.starts_with(&canon_base) {
        return Err("out_dir escapes meshes-out directory".to_string());
    }
    Ok((canon_target, Some(rel_norm)))
}

fn mime_for_mesh_ext(filename: &str) -> Result<&'static str, String> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".obj") {
        Ok("text/plain; charset=utf-8")
    } else if lower.ends_with(".mesh") {
        Ok("application/octet-stream")
    } else if lower.ends_with(".glb") {
        Ok("model/gltf-binary")
    } else if lower.ends_with(".png") {
        Ok("image/png")
    } else if lower.ends_with(".json") {
        Ok("application/json; charset=utf-8")
    } else {
        Err(format!(
            "mesh asset '{filename}' must end with .mesh, .obj, .glb, .png, or .json"
        ))
    }
}

/// Safely read a generated mesh asset (`.obj`, `.png`, or `.json`) for `GET /api/assets/meshes/:file`.
/// Supports flat files in `<data_dir>/game-assets/meshes/` as well as `out/<rel>/<file>` in
/// `<data_dir>/meshes-out/`.
pub fn read_mesh_asset(
    data_dir: &Path,
    raw_file: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    let trimmed = raw_file.trim();
    if trimmed.is_empty() || trimmed.len() > 220 {
        return Err("mesh filename must be 1..=220 characters".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err(format!(
            "invalid mesh filename '{trimmed}': path traversal is forbidden"
        ));
    }

    let (base_dir, rel_path, leaf_name) = if let Some(rest) = trimmed.strip_prefix("out/") {
        let segs: Vec<&str> = rest.split('/').collect();
        if segs.is_empty() || segs.len() > 9 {
            return Err(format!("invalid mesh asset path '{trimmed}'"));
        }
        let mut rel = PathBuf::new();
        for seg in &segs {
            validate_rel_segment(seg)?;
            rel.push(seg);
        }
        let leaf = segs[segs.len() - 1].to_string();
        (data_dir.join("meshes-out"), rel, leaf)
    } else {
        if trimmed.contains('/') || trimmed.starts_with('.') {
            return Err(format!(
                "invalid mesh filename '{trimmed}': path separators or hidden files are forbidden"
            ));
        }
        if trimmed.len() > 96 {
            return Err("mesh filename must be 1..=96 characters".to_string());
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(format!(
                "invalid mesh filename '{trimmed}': only ASCII letters, digits, '.', '-', '_' allowed"
            ));
        }
        (
            mesh_assets_dir(data_dir),
            PathBuf::from(trimmed),
            trimmed.to_string(),
        )
    };

    let mime = mime_for_mesh_ext(&leaf_name)?;
    let mut walk = base_dir.clone();
    for seg in rel_path.components() {
        walk.push(seg);
        if let Ok(meta) = std::fs::symlink_metadata(&walk) {
            if meta.file_type().is_symlink() {
                return Err(format!("mesh asset path '{trimmed}' is a symlink"));
            }
        }
    }
    let canon_dir = std::fs::canonicalize(&base_dir)
        .map_err(|_| format!("mesh asset '{trimmed}' not found"))?;
    let candidate = base_dir.join(&rel_path);
    let canon_file = std::fs::canonicalize(&candidate)
        .map_err(|_| format!("mesh asset '{trimmed}' not found"))?;
    if !canon_file.starts_with(&canon_dir) {
        return Err("mesh asset path escapes meshes directory".to_string());
    }
    if !canon_file.is_file() {
        return Err(format!("mesh asset '{trimmed}' not found"));
    }
    let bytes = std::fs::read(&canon_file).map_err(|e| format!("read mesh asset: {e}"))?;
    Ok((bytes, mime))
}

fn read_capped<R: Read>(mut reader: R, limit: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0_u8; 4096];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        if out.len() < limit {
            let take = n.min(limit - out.len());
            out.extend_from_slice(&buf[..take]);
        }
    }
    out
}

fn run_blender_with_timeout(
    blender_exe: &str,
    script_path: &Path,
    spec_path: &Path,
    stage_dir: &Path,
    work_dir: &Path,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, String, String), String> {
    let mut child = Command::new(blender_exe)
        .args([
            "--background",
            "--factory-startup",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script_path)
        .arg("--")
        .arg(spec_path)
        .arg(stage_dir)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start Blender ({blender_exe}): {e}"))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        stdout_pipe
            .map(|r| read_capped(r, 256 * 1024))
            .unwrap_or_default()
    });
    let err_handle = std::thread::spawn(move || {
        stderr_pipe
            .map(|r| read_capped(r, 256 * 1024))
            .unwrap_or_default()
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break child
                        .wait()
                        .map_err(|e| format!("waiting for timed-out Blender process: {e}"))?;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("monitoring Blender process: {e}"));
            }
        }
    };

    let stdout = String::from_utf8_lossy(&out_handle.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_handle.join().unwrap_or_default()).into_owned();

    if timed_out {
        return Err(format!(
            "Blender timed out after {} seconds and was stopped",
            timeout.as_secs()
        ));
    }
    Ok((status, stdout, stderr))
}

fn move_file_overwrite(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        let _ = std::fs::remove_file(dst);
    }
    if std::fs::rename(src, dst).is_err() {
        std::fs::copy(src, dst).map_err(|e| format!("copy {} to {}: {e}", src.display(), dst.display()))?;
        let _ = std::fs::remove_file(src);
    }
    Ok(())
}

fn merge_dir_manifest(out_dir: &Path, slug: &str, entry: &Value) -> Result<PathBuf, String> {
    let _guard = MANIFEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let manifest_path = out_dir.join("manifest.json");
    let mut root: Map<String, Value> = if manifest_path.is_file() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    } else {
        Map::new()
    };
    root.insert(slug.to_string(), entry.clone());
    let serialized = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("serialize manifest.json: {e}"))?;
    let seq = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = out_dir.join(format!(".manifest.json.tmp.{seq}"));
    std::fs::write(&tmp, serialized).map_err(|e| format!("write temp manifest.json: {e}"))?;
    move_file_overwrite(&tmp, &manifest_path)?;
    Ok(manifest_path)
}

fn check_png_file(path: &Path, label: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {label}: {e}"))?;
    if bytes.len() < 64 || !bytes.starts_with(PNG_MAGIC) {
        return Err(format!("Blender produced an invalid PNG for {label}"));
    }
    Ok(())
}

fn check_obj_has_uvs(path: &Path, label: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {label}: {e}"))?;
    let has_v = text.lines().any(|l| l.starts_with("v "));
    let has_vt = text.lines().any(|l| l.starts_with("vt "));
    if !has_v || !has_vt {
        return Err(format!(
            "exported OBJ for {label} is missing vertices or UV coordinates"
        ));
    }
    Ok(())
}

fn check_roblox_mesh_v2(path: &Path, label: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {label}: {e}"))?;
    if bytes.len() < 25 || !bytes.starts_with(b"version 2.00\n") {
        return Err(format!(
            "exported Roblox binary .mesh for {label} is invalid (missing version 2.00 header)"
        ));
    }
    Ok(())
}

fn check_glb_file(path: &Path, label: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {label}: {e}"))?;
    if bytes.len() < 12 || !bytes.starts_with(b"glTF") {
        return Err(format!(
            "exported GLB for {label} is invalid (missing glTF magic header)"
        ));
    }
    Ok(())
}

/// Generate a UV-textured, Cycles-baked stylized mesh (plus optional glow mesh, manifest, and
/// 2-angle preview PNGs) via headless Blender.
pub fn generate(
    data_dir: &Path,
    req: &Value,
    blender_override: Option<&str>,
) -> Result<Value, String> {
    let started = Instant::now();
    let req_obj = req
        .as_object()
        .ok_or_else(|| "request body must be a JSON object".to_string())?;

    let parts_opt = match req_obj.get("parts") {
        Some(v) if !v.is_null() => Some(parse_parts(v)?),
        _ => None,
    };
    let script_opt = match req_obj.get("script") {
        Some(v) if !v.is_null() => {
            let s = v
                .as_str()
                .ok_or_else(|| "'script' must be a string".to_string())?;
            validate_script(s)?;
            Some(s.to_string())
        }
        _ => None,
    };

    let has_parts = parts_opt.as_ref().is_some_and(|p| !p.is_empty());
    let has_script = script_opt.is_some();
    if !has_parts && !has_script {
        return Err(
            "provide at least one item in 'parts', or a non-empty 'script' to build a mesh"
                .to_string(),
        );
    }
    let mode = match (has_parts, has_script) {
        (true, true) => "hybrid",
        (true, false) => "structured",
        (false, true) => "script",
        (false, false) => unreachable!(),
    };

    let slug = slug_for_mesh_name(req_obj.get("name").and_then(|v| v.as_str()));
    let anchors = parse_anchors(req_obj.get("anchors"))?;

    let bevel = match req_obj.get("bevel") {
        Some(v) if !v.is_null() => {
            let b = v
                .as_f64()
                .ok_or_else(|| "'bevel' must be a number".to_string())?;
            if !b.is_finite() || !(0.0..=2.0).contains(&b) {
                return Err("'bevel' must be between 0 and 2 studs".to_string());
            }
            b
        }
        _ => 0.03,
    };

    let texture_size = match req_obj.get("texture_size") {
        Some(v) if !v.is_null() => {
            let n = v
                .as_u64()
                .ok_or_else(|| "'texture_size' must be an integer".to_string())?;
            if !(64..=2048).contains(&n) {
                return Err("'texture_size' must be between 64 and 2048 pixels".to_string());
            }
            n
        }
        _ => 1024,
    };

    let ao_strength = match req_obj.get("ao_strength") {
        Some(v) if !v.is_null() => {
            let a = v
                .as_f64()
                .ok_or_else(|| "'ao_strength' must be a number".to_string())?;
            if !a.is_finite() || !(0.0..=1.0).contains(&a) {
                return Err("'ao_strength' must be between 0 and 1".to_string());
            }
            a
        }
        _ => 0.75,
    };

    let render_preview = req_obj
        .get("preview")
        .or_else(|| req_obj.get("render_preview"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let preview_size = match req_obj.get("preview_size") {
        Some(v) if !v.is_null() => {
            let n = v
                .as_u64()
                .ok_or_else(|| "'preview_size' must be an integer".to_string())?;
            if !(64..=1024).contains(&n) {
                return Err("'preview_size' must be between 64 and 1024 pixels".to_string());
            }
            n
        }
        _ => 480,
    };

    let timeout_secs = match req_obj.get("timeout_secs") {
        Some(v) if !v.is_null() => {
            let n = v
                .as_u64()
                .ok_or_else(|| "'timeout_secs' must be an integer".to_string())?;
            if !(1..=SCRIPT_TIMEOUT_SECS).contains(&n) {
                return Err(format!(
                    "'timeout_secs' must be between 1 and {SCRIPT_TIMEOUT_SECS} seconds"
                ));
            }
            n
        }
        _ => SCRIPT_TIMEOUT_SECS,
    };

    let out_dir_str = match req_obj.get("out_dir") {
        Some(v) if !v.is_null() => Some(
            v.as_str()
                .ok_or_else(|| "'out_dir' must be a relative path string".to_string())?,
        ),
        _ => None,
    };
    let (out_dir, out_dir_rel) = resolve_out_dir(data_dir, out_dir_str)?;

    let det = detect_blender(blender_override);
    let blender_exe = det.path.as_deref().ok_or_else(|| {
        "Blender is not installed or could not be found; install Blender to build UV-textured meshes"
            .to_string()
    })?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let scratch_root = data_dir
        .join("meshes-scratch")
        .join(format!("run-{now_ms}-{seq}"));
    let stage_dir = scratch_root.join("stage");
    let work_dir = scratch_root.join("work");
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("create scratch stage dir: {e}"))?;
    std::fs::create_dir_all(&work_dir).map_err(|e| format!("create scratch work dir: {e}"))?;
    let _guard = ScratchGuard(scratch_root.clone());

    let script_path = scratch_root.join("mesh_lib.py");
    let spec_path = scratch_root.join("spec.json");
    std::fs::write(&script_path, MESH_LIB_PY)
        .map_err(|e| format!("write Blender mesh helper: {e}"))?;

    let spec = json!({
        "slug": slug,
        "parts": parts_opt.unwrap_or_default(),
        "script": script_opt,
        "anchors": anchors,
        "bevel": bevel,
        "texture_size": texture_size,
        "ao_strength": ao_strength,
        "preview": render_preview,
        "preview_size": preview_size,
        "script_timeout_secs": timeout_secs,
    });
    let spec_bytes =
        serde_json::to_vec_pretty(&spec).map_err(|e| format!("serialize mesh spec: {e}"))?;
    std::fs::write(&spec_path, spec_bytes).map_err(|e| format!("write mesh spec: {e}"))?;

    let (status, stdout, stderr) = run_blender_with_timeout(
        blender_exe,
        &script_path,
        &spec_path,
        &stage_dir,
        &work_dir,
        Duration::from_secs(timeout_secs),
    )?;

    let error_file = stage_dir.join("error.txt");
    if error_file.is_file() {
        let msg = std::fs::read_to_string(&error_file)
            .unwrap_or_default()
            .trim()
            .to_string();
        if !msg.is_empty() {
            return Err(msg);
        }
    }

    if !status.success() {
        let detail = stderr
            .lines()
            .rev()
            .find(|l| {
                let t = l.trim();
                !t.is_empty() && !t.contains("DeprecationWarning")
            })
            .or_else(|| stdout.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("Blender exited with an error");
        return Err(format!("Blender mesh pipeline failed: {detail}"));
    }

    let stage_manifest_path = stage_dir.join(format!("{slug}.manifest.json"));
    let manifest_raw = std::fs::read_to_string(&stage_manifest_path)
        .map_err(|_| "Blender finished without writing manifest.json".to_string())?;
    let manifest: Value = serde_json::from_str(&manifest_raw)
        .map_err(|e| format!("parse generated manifest: {e}"))?;

    let tris = manifest
        .get("tris")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "generated manifest is missing triangle count".to_string())?;
    if tris > MAX_TRIS {
        return Err(format!(
            "this mesh has {tris} triangles, over the {MAX_TRIS} triangle limit Roblox allows"
        ));
    }

    // Collect and verify every expected staged output file before moving into out_dir.
    let mut files_to_commit: Vec<String> = vec![format!("{slug}.manifest.json")];

    let obj_file = manifest
        .get("mesh")
        .and_then(|v| v.as_str())
        .map(String::from);
    let roblox_mesh_file = manifest
        .get("roblox_mesh")
        .and_then(|v| v.as_str())
        .map(String::from);
    let glb_file = manifest
        .get("glb")
        .and_then(|v| v.as_str())
        .map(String::from);
    let texture_file = manifest
        .get("texture")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some(ref f) = obj_file {
        check_obj_has_uvs(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = roblox_mesh_file {
        check_roblox_mesh_v2(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = glb_file {
        check_glb_file(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = texture_file {
        check_png_file(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }

    let glow_obj_file = manifest
        .get("glow_mesh")
        .and_then(|v| v.as_str())
        .map(String::from);
    let glow_roblox_mesh_file = manifest
        .get("glow_roblox_mesh")
        .and_then(|v| v.as_str())
        .map(String::from);
    let glow_glb_file = manifest
        .get("glow_glb")
        .and_then(|v| v.as_str())
        .map(String::from);
    let glow_texture_file = manifest
        .get("glow_texture")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some(ref f) = glow_obj_file {
        check_obj_has_uvs(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = glow_roblox_mesh_file {
        check_roblox_mesh_v2(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = glow_glb_file {
        check_glb_file(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }
    if let Some(ref f) = glow_texture_file {
        check_png_file(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }

    let preview_files: Vec<String> = manifest
        .get("previews")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for f in &preview_files {
        check_png_file(&stage_dir.join(f), f)?;
        files_to_commit.push(f.clone());
    }

    for f in &files_to_commit {
        move_file_overwrite(&stage_dir.join(f), &out_dir.join(f))?;
    }

    let dir_manifest_path = merge_dir_manifest(&out_dir, &slug, &manifest)?;

    let url_prefix = match &out_dir_rel {
        Some(rel) => format!("/api/assets/meshes/out/{rel}"),
        None => "/api/assets/meshes".to_string(),
    };
    let mk_url = |opt: &Option<String>| opt.as_ref().map(|f| format!("{url_prefix}/{f}"));
    let mk_path = |opt: &Option<String>| {
        opt.as_ref()
            .map(|f| out_dir.join(f).to_string_lossy().into_owned())
    };

    let previews_json: Vec<Value> = preview_files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            json!({
                "angle": i,
                "file": f,
                "path": out_dir.join(f).to_string_lossy().into_owned(),
                "url": format!("{url_prefix}/{f}"),
            })
        })
        .collect();
    let first_preview_url = preview_files.first().map(|f| format!("{url_prefix}/{f}"));
    let manifest_file = format!("{slug}.manifest.json");

    let mut out = json!({
        "ok": true,
        "mode": mode,
        "name": slug,
        "blender_used": true,
        "blender_version": det.version,
        "units": "studs",
        "up_axis": "Y",
        "out_dir": out_dir.to_string_lossy().into_owned(),
        "out_dir_rel": out_dir_rel,
        "tris": tris,
        "solid_tris": manifest.get("solid_tris").and_then(|v| v.as_u64()).unwrap_or(0),
        "glow_tris": manifest.get("glow_tris").and_then(|v| v.as_u64()).unwrap_or(0),
        "max_tris": MAX_TRIS,
        "size": manifest.get("size").cloned().unwrap_or(Value::Null),
        "bbox": manifest.get("bbox").cloned().unwrap_or(Value::Null),
        "anchors": manifest.get("anchors").cloned().unwrap_or(json!([])),
        "obj_file": obj_file,
        "obj_path": mk_path(&obj_file),
        "obj_url": mk_url(&obj_file),
        "texture_file": texture_file,
        "texture_path": mk_path(&texture_file),
        "texture_url": mk_url(&texture_file),
        "glow_obj_file": glow_obj_file,
        "glow_obj_path": mk_path(&glow_obj_file),
        "glow_obj_url": mk_url(&glow_obj_file),
        "glow_texture_file": glow_texture_file,
        "glow_texture_path": mk_path(&glow_texture_file),
        "glow_texture_url": mk_url(&glow_texture_file),
        "manifest_file": manifest_file,
        "manifest_path": out_dir.join(&manifest_file).to_string_lossy().into_owned(),
        "manifest_url": format!("{url_prefix}/{manifest_file}"),
        "dir_manifest_file": "manifest.json",
        "dir_manifest_path": dir_manifest_path.to_string_lossy().into_owned(),
        "dir_manifest_url": format!("{url_prefix}/manifest.json"),
        "previews": previews_json,
        "preview_url": first_preview_url,
        "manifest": manifest,
        "elapsed_ms": started.elapsed().as_millis() as u64,
    });
    if let Some(m) = out.as_object_mut() {
        m.insert("roblox_mesh_file".into(), json!(roblox_mesh_file));
        m.insert("roblox_mesh_path".into(), json!(mk_path(&roblox_mesh_file)));
        m.insert("roblox_mesh_url".into(), json!(mk_url(&roblox_mesh_file)));
        m.insert("glb_file".into(), json!(glb_file));
        m.insert("glb_path".into(), json!(mk_path(&glb_file)));
        m.insert("glb_url".into(), json!(mk_url(&glb_file)));
        m.insert("glow_roblox_mesh_file".into(), json!(glow_roblox_mesh_file));
        m.insert("glow_roblox_mesh_path".into(), json!(mk_path(&glow_roblox_mesh_file)));
        m.insert("glow_roblox_mesh_url".into(), json!(mk_url(&glow_roblox_mesh_file)));
        m.insert("glow_glb_file".into(), json!(glow_glb_file));
        m.insert("glow_glb_path".into(), json!(mk_path(&glow_glb_file)));
        m.insert("glow_glb_url".into(), json!(mk_url(&glow_glb_file)));
    }
    Ok(out)
}

/// Generate a Blender mesh via `generate(...)` and, when `project_root` is provided,
/// also copy outputs into `<project_root>/assets/meshes/`, update its merged `manifest.json`,
/// and regenerate `<project_root>/src/shared/AssetIds.luau` with stud bbox metadata.
pub fn generate_for_project(
    data_dir: &Path,
    project_root: Option<&Path>,
    req: &Value,
    blender_override: Option<&str>,
) -> Result<Value, String> {
    let mut res = generate(data_dir, req, blender_override)?;
    let Some(root) = project_root else {
        return Ok(res);
    };
    let proj_meshes = root.join("assets").join("meshes");
    std::fs::create_dir_all(&proj_meshes)
        .map_err(|e| format!("create project meshes dir: {e}"))?;
    let mut copied = Vec::new();
    for key in [
        "obj_path",
        "roblox_mesh_path",
        "glb_path",
        "texture_path",
        "glow_obj_path",
        "glow_roblox_mesh_path",
        "glow_glb_path",
        "glow_texture_path",
        "manifest_path",
    ] {
        if let Some(src_str) = res.get(key).and_then(Value::as_str) {
            let src = Path::new(src_str);
            if let Some(fname) = src.file_name() {
                if src.is_file() {
                    let dst = proj_meshes.join(fname);
                    std::fs::copy(src, &dst)
                        .map_err(|e| format!("copy mesh asset to project: {e}"))?;
                    copied.push(format!("assets/meshes/{}", fname.to_string_lossy()));
                }
            }
        }
    }
    if let (Some(slug), Some(manifest)) = (res.get("name").and_then(Value::as_str), res.get("manifest")) {
        let _ = merge_dir_manifest(&proj_meshes, slug, manifest);
    }
    let cg_args = if root.join("src/shared/content/AssetIds.luau").is_file() {
        json!({ "out": "src/shared/content/AssetIds.luau" })
    } else {
        json!({})
    };
    let codegen = crate::opencloud::assets_codegen(root, &cg_args).ok();
    if let Some(obj) = res.as_object_mut() {
        obj.insert("project_files".into(), json!(copied));
        obj.insert("project_manifest".into(), json!("assets/meshes/manifest.json"));
        if let Some(cg) = codegen {
            obj.insert("asset_ids_luau".into(), cg);
        }
    }
    Ok(res)
}

const ARENA_FLOOR_ARCANE_PY: &str = r##"import math, sys
from PIL import Image, ImageDraw

out_path = sys.argv[1]
size = 1024
img = Image.new("RGBA", (size, size), (14, 24, 68, 255))
d = ImageDraw.Draw(img)

tile = 128
for row in range(size // tile):
    for col in range(size // tile):
        alt = (row + col) % 2 == 0
        c = (18, 33, 92, 255) if alt else (11, 21, 64, 255)
        x0, y0 = col * tile, row * tile
        d.rectangle((x0, y0, x0 + tile - 1, y0 + tile - 1), fill=c)
        d.rectangle((x0 + 4, y0 + 4, x0 + tile - 5, y0 + tile - 5), outline=(34, 64, 148, 120), width=2)

# Geometric sapphire-cyan arcane compass & concentric rings (original geometric motifs, zero text)
cx = cy = size // 2
for r, col, w in [(470, (44, 118, 232, 200), 6), (446, (56, 189, 248, 220), 4), (310, (56, 189, 248, 235), 5), (170, (99, 230, 255, 240), 4)]:
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=col, width=w)

for idx in range(8):
    ang = idx * math.pi / 4.0
    x1 = cx + int(math.cos(ang) * 446)
    y1 = cy + int(math.sin(ang) * 446)
    x2 = cx + int(math.cos(ang) * 170)
    y2 = cy + int(math.sin(ang) * 170)
    d.line([(x2, y2), (x1, y1)], fill=(56, 189, 248, 165), width=3)

# Corner diamond studs on tile intersections
for k in range(0, size + 1, tile):
    for m in range(0, size + 1, tile):
        pts = [(k, m - 6), (k + 6, m), (k, m + 6), (k - 6, m)]
        d.polygon(pts, fill=(94, 214, 255, 190))

img.save(out_path, "PNG", optimize=True)
"##;

fn map_kit_prop_specs(preset: &str) -> Result<Vec<(&'static str, &'static str, &'static str)>, String> {
    let norm = preset.trim().to_ascii_lowercase().replace('_', "-");
    if !matches!(norm.as_str(), "arcane-dungeon-sanctum" | "dungeon-sanctum" | "arcane-arena" | "default") {
        return Err(format!(
            "unknown Blender map kit preset '{preset}' (supported: arcane-dungeon-sanctum)"
        ));
    }
    Ok(vec![
        (
            "dungeon_column",
            "Sculpted octagonal sapphire-slate dungeon column with stepped plinth, beveled stone rings, ornate capital, and glowing cyan sigil collar",
            r##"
stone_mat = make_stone_material("stone", "#1F2E56", "#0D1530")
gold_mat = make_metallic_material("gold", "#D4A43C", "#3A2A0D", roughness=0.28)
glow_mat = make_glow_material("glow", "#38BDF8", 4.5)

bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=3.2, depth=1.6, location=(0, 0, 0.8))
base1 = bpy.context.active_object; bevel_obj(base1, 0.08, 2); assign_material(base1, stone_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=2.65, depth=1.2, location=(0, 0, 2.2))
base2 = bpy.context.active_object; bevel_obj(base2, 0.07, 2); assign_material(base2, stone_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=12, radius=1.95, depth=12.4, location=(0, 0, 9.0))
shaft = bpy.context.active_object; bevel_obj(shaft, 0.05, 1); assign_material(shaft, stone_mat)
for z in (3.2, 8.8, 14.2):
    bpy.ops.mesh.primitive_cylinder_add(vertices=12, radius=2.22, depth=0.65, location=(0, 0, z))
    ring = bpy.context.active_object; bevel_obj(ring, 0.04, 1); assign_material(ring, gold_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=3.15, depth=1.8, location=(0, 0, 16.1))
cap = bpy.context.active_object; bevel_obj(cap, 0.08, 2); assign_material(cap, stone_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=12, radius=2.12, depth=0.75, location=(0, 0, 14.95))
gcol = bpy.context.active_object; gcol['glow'] = True; assign_material(gcol, glow_mat)
"##,
        ),
        (
            "soul_brazier",
            "Carved octagonal stone torch brazier with gilded bronze basin rim and glowing crystalline arcane flame core",
            r##"
stone_mat = make_stone_material("stone", "#1F2E56", "#0D1530")
gold_mat = make_metallic_material("gold", "#D4A43C", "#3A2A0D", roughness=0.25)
glow_mat = make_glow_material("glow", "#38BDF8", 5.0)

bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=2.1, depth=1.2, location=(0, 0, 0.6))
ped = bpy.context.active_object; bevel_obj(ped, 0.06, 2); assign_material(ped, stone_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=1.55, depth=2.0, location=(0, 0, 2.2))
stem = bpy.context.active_object; bevel_obj(stem, 0.05, 1); assign_material(stem, stone_mat)
bpy.ops.mesh.primitive_cone_add(vertices=8, radius1=1.35, radius2=2.25, depth=1.5, location=(0, 0, 3.85))
bowl = bpy.context.active_object; bevel_obj(bowl, 0.06, 2); assign_material(bowl, stone_mat)
bpy.ops.mesh.primitive_torus_add(major_segments=16, minor_segments=8, major_radius=2.15, minor_radius=0.24, location=(0, 0, 4.55))
rim = bpy.context.active_object; assign_material(rim, gold_mat)
bpy.ops.mesh.primitive_cone_add(vertices=8, radius1=1.05, radius2=0.08, depth=2.3, location=(0, 0, 5.4))
flame = bpy.context.active_object; flame['glow'] = True; assign_material(flame, glow_mat)
"##,
        ),
        (
            "sanctum_arch_gate",
            "Monumental dungeon sanctum stone arch gate with stepped buttress piers, gilded keystone crest, and glowing portal arc",
            r##"
stone_mat = make_stone_material("stone", "#1B284C", "#0C1329")
gold_mat = make_metallic_material("gold", "#D4A43C", "#3A2A0D", roughness=0.27)
glow_mat = make_glow_material("glow", "#38BDF8", 4.5)

for sx in (-8.5, 8.5):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(sx, 0, 1.0)); b = bpy.context.active_object
    b.scale = (5.2, 5.2, 2.0); bpy.ops.object.transform_apply(scale=True); bevel_obj(b, 0.08, 2); assign_material(b, stone_mat)
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(sx, 0, 9.0)); p = bpy.context.active_object
    p.scale = (3.8, 3.8, 14.0); bpy.ops.object.transform_apply(scale=True); bevel_obj(p, 0.07, 2); assign_material(p, stone_mat)
bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 17.0)); lintel = bpy.context.active_object
lintel.scale = (23.0, 4.6, 3.2); bpy.ops.object.transform_apply(scale=True); bevel_obj(lintel, 0.09, 2); assign_material(lintel, stone_mat)
bpy.ops.mesh.primitive_cone_add(vertices=6, radius1=2.2, radius2=0.5, depth=3.2, location=(0, 0, 19.6))
crest = bpy.context.active_object; bevel_obj(crest, 0.05, 1); assign_material(crest, gold_mat)
bpy.ops.mesh.primitive_torus_add(major_segments=20, minor_segments=8, major_radius=5.8, minor_radius=0.35, location=(0, 0, 11.2), rotation=(math.pi / 2, 0, 0))
portal = bpy.context.active_object; portal['glow'] = True; assign_material(portal, glow_mat)
"##,
        ),
        (
            "summon_altar",
            "Stepped octagonal summon dais with bronze sigil ring and luminous arcane core pad",
            r##"
stone_mat = make_stone_material("stone", "#1F2E56", "#0D1530")
gold_mat = make_metallic_material("gold", "#D4A43C", "#3A2A0D", roughness=0.28)
glow_mat = make_glow_material("glow", "#38BDF8", 4.6)

bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=6.4, depth=0.9, location=(0, 0, 0.45))
d1 = bpy.context.active_object; bevel_obj(d1, 0.07, 2); assign_material(d1, stone_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=4.8, depth=0.9, location=(0, 0, 1.35))
d2 = bpy.context.active_object; bevel_obj(d2, 0.06, 2); assign_material(d2, stone_mat)
bpy.ops.mesh.primitive_torus_add(major_segments=24, minor_segments=8, major_radius=4.1, minor_radius=0.25, location=(0, 0, 1.85))
ring = bpy.context.active_object; assign_material(ring, gold_mat)
bpy.ops.mesh.primitive_cylinder_add(vertices=16, radius=3.4, depth=0.35, location=(0, 0, 1.95))
core = bpy.context.active_object; core['glow'] = True; assign_material(core, glow_mat)
"##,
        ),
        (
            "relief_wall_panel",
            "Architectural fortress relief stone wall bay with twin buttress ribs, beveled cornice, and vertical arcane light conduits",
            r##"
stone_mat = make_stone_material("stone", "#192547", "#0B1226")
gold_mat = make_metallic_material("gold", "#D4A43C", "#3A2A0D", roughness=0.30)
glow_mat = make_glow_material("glow", "#38BDF8", 4.2)

bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 7.0)); slab = bpy.context.active_object
slab.scale = (18.0, 2.2, 14.0); bpy.ops.object.transform_apply(scale=True); bevel_obj(slab, 0.07, 2); assign_material(slab, stone_mat)
bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 14.4)); corn = bpy.context.active_object
corn.scale = (19.0, 3.0, 1.4); bpy.ops.object.transform_apply(scale=True); bevel_obj(corn, 0.07, 2); assign_material(corn, stone_mat)
for rx in (-6.5, 6.5):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(rx, 0.55, 7.0)); rib = bpy.context.active_object
    rib.scale = (1.6, 1.8, 13.0); bpy.ops.object.transform_apply(scale=True); bevel_obj(rib, 0.05, 1); assign_material(rib, gold_mat)
for gx in (-2.4, 2.4):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(gx, 1.05, 7.0)); strip = bpy.context.active_object
    strip.scale = (0.45, 0.35, 10.6); bpy.ops.object.transform_apply(scale=True); strip['glow'] = True; assign_material(strip, glow_mat)
"##,
        ),
    ])
}

/// Generate a complete Blender 3D Map & Architecture Kit (sculpted `.mesh`/`.obj`/`.glb` props,
/// baked AO PNGs, checkered arcane floor texture, `map_kit.json`, and regenerated `AssetIds.luau`).
pub fn build_map_kit(
    data_dir: &Path,
    project_root: Option<&Path>,
    req: &Value,
    blender_override: Option<&str>,
) -> Result<Value, String> {
    let started = Instant::now();
    let preset = req
        .get("preset")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("arcane-dungeon-sanctum");
    let specs = map_kit_prop_specs(preset)?;
    let filter_names: Vec<String> = req
        .get("props")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| slug_for_mesh_name(Some(s)))).collect())
        .unwrap_or_default();

    let mut props = Vec::new();
    let mut all_files = Vec::new();
    for (name, desc, script) in specs {
        if !filter_names.is_empty() && !filter_names.iter().any(|f| f == name) {
            continue;
        }
        let prop_req = json!({
            "name": name,
            "description": desc,
            "script": script,
            "bevel": 0.07,
            "texture_size": 512,
        });
        let res = generate_for_project(data_dir, project_root, &prop_req, blender_override)?;
        if let Some(arr) = res.get("project_files").and_then(Value::as_array) {
            for f in arr {
                if let Some(s) = f.as_str() {
                    all_files.push(s.to_string());
                }
            }
        }
        props.push(res);
    }

    let mut floor_texture: Option<String> = None;
    let floor_dir = project_root
        .map(|r| r.join("assets").join("meshes"))
        .unwrap_or_else(|| data_dir.join("meshes"));
    if let Ok(()) = std::fs::create_dir_all(&floor_dir) {
        let floor_abs = floor_dir.join("arena_floor_arcane.png");
        if let Some(py) = find_python() {
            let out = Command::new(&py)
                .arg("-c")
                .arg(ARENA_FLOOR_ARCANE_PY)
                .arg(&floor_abs)
                .output();
            if out.is_ok_and(|o| o.status.success()) && check_png_file(&floor_abs, "arena_floor_arcane.png").is_ok() {
                let rel = "assets/meshes/arena_floor_arcane.png".to_string();
                all_files.push(rel.clone());
                floor_texture = Some(rel);
            }
        }
    }

    let mut map_kit_manifest: Option<String> = None;
    let mut asset_ids_luau: Option<Value> = None;
    if let Some(root) = project_root {
        let proj_meshes = root.join("assets").join("meshes");
        std::fs::create_dir_all(&proj_meshes)
            .map_err(|e| format!("create project meshes dir: {e}"))?;
        let kit_json = json!({
            "schema": "openro.map_kit.v1",
            "preset": preset,
            "floor_texture": floor_texture,
            "props": props.iter().map(|p| json!({
                "name": p["name"],
                "tris": p["tris"],
                "size": p["size"],
                "roblox_mesh": p["roblox_mesh_file"],
                "obj": p["obj_file"],
                "glb": p["glb_file"],
                "texture": p["texture_file"],
                "glow_roblox_mesh": p["glow_roblox_mesh_file"],
                "glow_texture": p["glow_texture_file"],
            })).collect::<Vec<_>>(),
        });
        let kit_path = proj_meshes.join("map_kit.json");
        std::fs::write(
            &kit_path,
            serde_json::to_string_pretty(&kit_json).unwrap_or_else(|_| "{}".to_string()),
        )
        .map_err(|e| format!("write map_kit.json: {e}"))?;
        map_kit_manifest = Some("assets/meshes/map_kit.json".to_string());
        let cg_args = if root.join("src/shared/content/AssetIds.luau").is_file() {
            json!({ "out": "src/shared/content/AssetIds.luau" })
        } else {
            json!({})
        };
        asset_ids_luau = crate::opencloud::assets_codegen(root, &cg_args).ok();
    }

    Ok(json!({
        "ok": true,
        "schema": "openro.map_kit.v1",
        "preset": preset,
        "prop_count": props.len(),
        "props": props,
        "floor_texture": floor_texture,
        "map_kit_manifest": map_kit_manifest,
        "project_files": all_files,
        "asset_ids_luau": asset_ids_luau,
        "elapsed_ms": started.elapsed().as_millis() as u64,
    }))
}

fn find_python() -> Option<String> {
    for cand in ["python3", "python"] {
        if Command::new(cand)
            .arg("-c")
            .arg("from PIL import Image")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return Some(cand.to_string());
        }
    }
    None
}
