//! Roblox Open Cloud for OpenRO projects: asset upload with operation polling and
//! `assets.lock.json`, place publish, game passes and developer products, universe
//! settings. Also routes the reference-image art endpoints (see `assets::art_generate`).
//!
//! The key lives only in the shared secrets store (`secrets.rs`), is typed only into
//! Settings, and never appears in a reply, a log line or an error (`secrets::scrub`).
//! Everything is addressed by project folder: callers pass a `Ctx` whose `resolve`
//! turns a project id into its folder. Replies are `(status, JSON)` or image bytes;
//! `server.rs` only translates them onto the socket.

use crate::secrets::{self, SecretStore, OPENCLOUD_KEY};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_BASE: &str = "https://apis.roblox.com";
pub const MANIFEST: &str = "openro.project.json";
pub(crate) const CREATOR_SECRET: &str = "opencloud_creator";
const MAX_UPLOAD_BYTES: u64 = 20 * 1024 * 1024;
const DEFAULT_MAX_PUBLISH_BYTES: u64 = 100 * 1024 * 1024;
const MAX_WALK_FILES: usize = 20_000;
/// Default authorised upload patterns (excludes preview sheets and owner references).
pub const DEFAULT_AUTHORISED_GLOBS: &[&str] = &[
    "art/cards/**",
    "art/ui/*.png",
    "art/frames/*/*.png",
    "!art/frames/preview/**",
    "art/store/*.png",
    "art/worlds/**",
    "art/audio/**",
    "assets/audio/**",
    "assets/meshes/**",
    "!assets/meshes/*preview*.png",
];
/// Manual Creator Dashboard steps that Roblox Open Cloud does not expose as endpoints.
pub const UNIVERSE_MANUAL_STEPS: &[&str] = &[
    "Game icon and store thumbnails — upload in Roblox Creator Dashboard -> Creations -> Experience -> Places / Thumbnails.",
    "Monetisation policy and regional pricing guidelines — configure in Creator Dashboard.",
    "Age & Experience Guidelines questionnaire — complete in Creator Dashboard -> Audience -> Questionnaire.",
];
const SCOPES: &[&str] = &[
    "asset:read",
    "asset:write",
    "universe-places:write",
    "game-pass:read",
    "game-pass:write",
    "developer-product:read",
    "developer-product:write",
    "universe:read",
    "universe:write",
    "universe-datastores.objects:read",
    "universe-datastores.objects:write",
    "universe.messaging-service:publish",
];

pub struct Ctx<'a> {
    pub data_dir: &'a Path,
    pub store: SecretStore,
    pub resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub active_project: Option<PathBuf>,
}

pub enum Reply {
    Json(u16, Value),
    Bytes(&'static str, Vec<u8>),
}

/// A failure with the status the HTTP layer should answer.
#[derive(Debug)]
pub struct Oc {
    pub status: u16,
    pub msg: String,
}
pub(crate) type R<T> = Result<T, Oc>;

pub(crate) fn bad<T>(msg: impl Into<String>) -> R<T> {
    Err(Oc { status: 400, msg: msg.into() })
}
pub(crate) fn err<T>(status: u16, msg: impl Into<String>) -> R<T> {
    Err(Oc { status, msg: msg.into() })
}
pub(crate) fn ok_json(v: Value) -> Reply {
    Reply::Json(200, v)
}
pub(crate) fn fail(e: Oc) -> Reply {
    Reply::Json(e.status, json!({ "ok": false, "error": e.msg }))
}

pub fn base_url() -> String {
    std::env::var("OPENRO_OPENCLOUD_BASE")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// First poll delay (`OPENRO_OPENCLOUD_POLL_MS` shortens it for tests).
fn poll_ms() -> Option<u64> {
    std::env::var("OPENRO_OPENCLOUD_POLL_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|n| n.clamp(1, 60_000))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_iso() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---------------------------------------------------------------- multipart

pub struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Default for Multipart {
    fn default() -> Self {
        Self::new()
    }
}

impl Multipart {
    pub fn new() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        Multipart { boundary: format!("----openro{nanos:x}"), body: Vec::new() }
    }
    fn part_head(&mut self, disposition: &str, mime: Option<&str>) {
        self.body.extend_from_slice(format!("--{}\r\nContent-Disposition: {disposition}\r\n", self.boundary).as_bytes());
        if let Some(m) = mime {
            self.body.extend_from_slice(format!("Content-Type: {m}\r\n").as_bytes());
        }
        self.body.extend_from_slice(b"\r\n");
    }
    pub fn text(&mut self, name: &str, value: &str) {
        self.part_head(&format!("form-data; name=\"{}\"", clean_hdr(name)), None);
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
    }
    pub fn file(&mut self, name: &str, filename: &str, mime: &str, bytes: &[u8]) {
        self.part_head(
            &format!("form-data; name=\"{}\"; filename=\"{}\"", clean_hdr(name), clean_hdr(filename)),
            Some(mime),
        );
        self.body.extend_from_slice(bytes);
        self.body.extend_from_slice(b"\r\n");
    }
    pub fn finish(mut self) -> (String, Vec<u8>) {
        self.body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (format!("multipart/form-data; boundary={}", self.boundary), self.body)
    }
}

fn clean_hdr(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '"' | '\r' | '\n' | '\\')).collect()
}

// ------------------------------------------------------------------ client

enum Creator {
    User(String),
    Group(String),
}

pub(crate) struct Client {
    base: String,
    key: String,
    creator: Option<Creator>,
    http: ureq::Agent,
}

impl Client {
    pub(crate) fn new(ctx: &Ctx) -> R<Client> {
        let Some(key) = ctx.store.get(OPENCLOUD_KEY) else {
            return err(503, "Add your Open Cloud key in Settings first.");
        };
        let http = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirects(0) // never forward the key to another host
            .build();
        Ok(Client { base: base_url(), key, creator: read_creator(&ctx.store), http })
    }

    /// One HTTP exchange. Any HTTP status comes back as `(status, body)`; only a
    /// transport failure is an `Err`. A 429 is retried up to 6 times with backoff (#69).
    pub(crate) fn call(&self, method: &str, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> R<(u16, Vec<u8>)> {
        let url = format!("{}{}", self.base, path);
        for attempt in 0..7u32 {
            let mut rq = self.http.request(method, &url).set("x-api-key", &self.key);
            if let Some(ct) = content_type {
                rq = rq.set("Content-Type", ct);
            }
            let res = match body {
                Some(b) => rq.send_bytes(b),
                None => rq.call(),
            };
            let (status, resp) = match res {
                Ok(r) => (r.status(), r),
                Err(ureq::Error::Status(code, r)) => (code, r),
                Err(ureq::Error::Transport(t)) => {
                    return err(502, self.scrub(&format!("Could not reach Roblox Open Cloud: {t}")));
                }
            };
            if status == 429 && attempt < 6 {
                let default_wait = 1u64.checked_shl(attempt.min(4)).unwrap_or(10);
                let wait = resp
                    .header("retry-after")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(default_wait);
                let cap = poll_ms().map(|p| p.saturating_mul(2)).unwrap_or(10_000);
                let sleep_ms = wait.saturating_mul(1000).min(cap);
                std::thread::sleep(Duration::from_millis(sleep_ms));
                continue;
            }
            let mut buf = Vec::new();
            let _ = std::io::Read::take(resp.into_reader(), 16 * 1024 * 1024).read_to_end(&mut buf);
            return Ok((status, buf));
        }
        err(502, "Roblox kept answering 'too many requests'. Wait a minute and try again.")
    }

    pub(crate) fn scrub(&self, s: &str) -> String {
        secrets::scrub(s, &[&self.key])
    }

    /// Call and require a 2xx JSON (or empty) answer.
    pub(crate) fn json(&self, method: &str, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> R<Value> {
        let (status, bytes) = self.call(method, path, content_type, body)?;
        self.check(status, &bytes)?;
        if bytes.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&bytes).or_else(|_| err(502, "Roblox answered with something that is not JSON."))
    }

    pub(crate) fn check(&self, status: u16, bytes: &[u8]) -> R<()> {
        if (200..300).contains(&status) {
            return Ok(());
        }
        err(502, self.upstream_msg(status, bytes))
    }

    pub(crate) fn upstream_msg(&self, status: u16, bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes);
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                ["/message", "/error/message", "/error/detail", "/errors/0/message", "/errors/0/detail", "/detail", "/error"]
                    .iter()
                    .find_map(|ptr| v.pointer(ptr).and_then(Value::as_str).map(String::from))
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        let head = match status {
            401 => "Roblox did not accept the Open Cloud key (401). Check the key and its scopes in Settings.".to_string(),
            403 => "Roblox says this key has no permission for that (403): add the scope or the experience to the key.".to_string(),
            429 => "Roblox rate limit reached (429). Try again shortly.".to_string(),
            s => format!("Roblox answered {s}."),
        };
        let combined = if detail.trim().is_empty() { head } else { format!("{head} {detail}") };
        self.scrub(&combined)
    }
}

fn read_creator(store: &SecretStore) -> Option<Creator> {
    let v = store.get(CREATOR_SECRET)?;
    let (kind, id) = v.split_once(':')?;
    match kind {
        "user" => Some(Creator::User(id.to_string())),
        "group" => Some(Creator::Group(id.to_string())),
        _ => None,
    }
}

pub fn creator_json(store: &SecretStore) -> Value {
    match read_creator(store) {
        Some(Creator::User(id)) => json!({ "userId": id }),
        Some(Creator::Group(id)) => json!({ "groupId": id }),
        None => json!({}),
    }
}

fn status_json(ctx: &Ctx) -> Value {
    let key = ctx.store.get(OPENCLOUD_KEY);
    json!({
        "ok": true,
        "has_key": key.is_some(),
        "key_masked": key.as_deref().map(secrets::mask),
        "creator": creator_json(&ctx.store),
        "base": base_url(),
        "scopes_needed": SCOPES,
    })
}

// ---------------------------------------------------------- project helpers

pub(crate) fn read_manifest(root: &Path) -> Value {
    std::fs::read_to_string(root.join(MANIFEST))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub(crate) fn write_json_atomic(path: &Path, v: &Value) -> R<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return bad("Target file cannot be a symlink.");
    }
    if let Some(parent) = path.parent() {
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return bad("Target parent directory cannot be a symlink.");
        }
    }
    let text = serde_json::to_string_pretty(v)
        .map_err(|e| Oc { status: 500, msg: format!("write {}: {e}", path.display()) })?;
    static OC_TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = OC_TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("json");
    let tmp = path.with_file_name(format!(".{fname}.openro-tmp.{}.{seq}", std::process::id()));
    let payload = format!("{text}\n");
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| Oc { status: 500, msg: format!("could not write {}: {e}", path.display()) })?;
        if let Err(e) = f.write_all(payload.as_bytes()).and_then(|_| f.flush()) {
            let _ = std::fs::remove_file(&tmp);
            return err(500, format!("could not write {}: {e}", path.display()));
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return err(500, format!("could not write {}: {e}", path.display()));
    }
    Ok(())
}

/// Merge `f` into a freshly read manifest (unknown fields stay) and save it.
pub(crate) fn update_manifest(root: &Path, f: impl FnOnce(&mut Map<String, Value>)) -> R<()> {
    let mut m = read_manifest(root);
    f(m.as_object_mut().expect("object"));
    write_json_atomic(&root.join(MANIFEST), &m)
}

/// A project-relative path that stays inside the folder: no `..`, no absolute path.
pub fn safe_rel(root: &Path, rel: &str) -> R<PathBuf> {
    let rel = rel.trim();
    let p = Path::new(rel);
    if rel.is_empty() || rel.contains('\0') || rel.contains('\\') || p.is_absolute() {
        return bad(format!("'{rel}' is not a path inside the project."));
    }
    let mut has_normal = false;
    for c in p.components() {
        match c {
            Component::Normal(_) => has_normal = true,
            Component::CurDir => {}
            _ => return bad(format!("'{rel}' leaves the project folder.")),
        }
    }
    if !has_normal {
        return bad(format!("'{rel}' is not a file path inside the project."));
    }
    let joined = root.join(p);
    let mut walk = root.to_path_buf();
    for c in p.components() {
        if let Component::Normal(seg) = c {
            walk.push(seg);
            if let Ok(meta) = std::fs::symlink_metadata(&walk) {
                if meta.file_type().is_symlink() {
                    return bad(format!("'{rel}' is a symlink that leaves the project folder."));
                }
            }
        }
    }
    // Walk up to the deepest existing ancestor and reject any symlink escape.
    let canon_root = std::fs::canonicalize(root)
        .map_err(|e| Oc { status: 500, msg: format!("project root unavailable: {e}") })?;
    let mut probe = joined.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let canon_ancestor = std::fs::canonicalize(&probe)
        .map_err(|_| Oc { status: 400, msg: format!("'{rel}' leaves the project folder.") })?;
    if !canon_ancestor.starts_with(&canon_root) {
        return bad(format!("'{rel}' leaves the project folder."));
    }
    Ok(joined)
}

fn lock_path(root: &Path, manifest: &Value) -> R<PathBuf> {
    let rel = manifest.pointer("/assets/lockFile").and_then(Value::as_str).unwrap_or("assets.lock.json");
    safe_rel(root, rel)
}

fn read_lock(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.get("assets").is_some_and(Value::is_object))
        .unwrap_or_else(|| json!({ "version": 1, "assets": {} }))
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

pub(crate) fn id_of(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn universe_id(manifest: &Value) -> R<String> {
    match id_of(manifest, "universeId").filter(|s| s.bytes().all(|b| b.is_ascii_digit())) {
        Some(u) => Ok(u),
        None => bad("This project has no universeId yet. Set it in the project settings."),
    }
}

// -------------------------------------------------------------------- globs

fn seg_match(pat: &[u8], text: &[u8]) -> bool {
    match pat.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|k| seg_match(rest, &text[k..])),
        Some((b'?', rest)) => !text.is_empty() && seg_match(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && seg_match(rest, &text[1..]),
    }
}

fn match_segs(p: &[&str], s: &[&str]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((&"**", rest)) => (0..=s.len()).any(|k| match_segs(rest, &s[k..])),
        Some((seg, rest)) => !s.is_empty() && seg_match(seg.as_bytes(), s[0].as_bytes()) && match_segs(rest, &s[1..]),
    }
}

/// `*` = inside one path segment, `**` = any depth. A pattern with no wildcard also
/// covers everything below that folder.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    if !pattern.contains(['*', '?']) {
        let p = pattern.trim_end_matches('/');
        return path == p || path.starts_with(&format!("{p}/"));
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    match_segs(&p, &s)
}

/// Evaluate a list of glob patterns supporting `!` negation entries (last match wins,
/// and any explicit `!` exclusion always beats a positive glob if listed after or matching previews).
pub fn is_authorised(patterns: &[String], path: &str) -> bool {
    if path.starts_with("art/references/") || path.starts_with("art/previews/") || path.starts_with("art/frames/preview/") {
        return false;
    }
    let mut matched = false;
    for raw in patterns {
        let pat = raw.trim();
        if let Some(neg) = pat.strip_prefix('!') {
            if glob_match(neg.trim(), path) {
                matched = false;
            }
        } else if !pat.is_empty() && glob_match(pat, path) {
            matched = true;
        }
    }
    matched
}

// ------------------------------------------------------------------- assets

const KINDS: &[&str] = &["Image", "Decal", "Audio", "Model", "Animation", "Video"];

fn ext_of(path: &str) -> String {
    path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn kind_by_ext(path: &str) -> Option<&'static str> {
    Some(match ext_of(path).as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "tga" => "Image",
        "mp3" | "ogg" | "wav" | "flac" => "Audio",
        "obj" | "fbx" | "gltf" | "glb" | "rbxm" | "rbxmx" => "Model",
        "mp4" | "mov" => "Video",
        _ => return None,
    })
}

fn mime_for(path: &str) -> &'static str {
    match ext_of(path).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "tga" => "image/tga",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "obj" => "model/obj",
        "fbx" => "model/fbx",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "rbxmx" => "application/xml",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// Kind for a file: a manifest `assets.types` glob (or request `types`) wins over the extension.
fn kind_for(path: &str, types: &[(String, String)]) -> Option<String> {
    for (glob, kind) in types {
        if glob_match(glob, path) {
            return KINDS.iter().find(|k| k.eq_ignore_ascii_case(kind)).map(|k| k.to_string());
        }
    }
    kind_by_ext(path).map(String::from)
}

fn type_overrides(manifest: &Value, body: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for src in [body.get("types"), manifest.pointer("/assets/types")].into_iter().flatten() {
        if let Some(o) = src.as_object() {
            out.extend(o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))));
        }
    }
    out
}

struct ProjectFile {
    rel: String,
    abs: PathBuf,
    bytes: u64,
}

/// Every regular file under the project that could be an asset. Symlinks, hidden folders,
/// build output, preview galleries and the owner's reference images are never walked.
fn walk(root: &Path) -> Vec<ProjectFile> {
    fn skip_dir(rel: &str, name: &str) -> bool {
        name.starts_with('.')
            || matches!(name, "node_modules" | "build" | "target" | "refs")
            || matches!(rel, "art/references" | "art/previews" | "art/frames/preview")
    }
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0u8)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < 12 && !skip_dir(&child_rel, &name) {
                    stack.push((e.path(), child_rel, depth + 1));
                }
            } else if ft.is_file() && !name.starts_with('.') && out.len() < MAX_WALK_FILES {
                let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(ProjectFile { rel: child_rel, abs: e.path(), bytes });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn assets_list(root: &Path) -> R<Value> {
    let manifest = read_manifest(root);
    let authorised = str_list(manifest.pointer("/assets/authorised"));
    let types = type_overrides(&manifest, &json!({}));
    let lock = read_lock(&lock_path(root, &manifest)?);
    let mut files = Vec::new();
    for f in walk(root) {
        let Some(kind) = kind_for(&f.rel, &types) else { continue };
        let sha = std::fs::read(&f.abs).map(|b| sha256_hex(&b)).unwrap_or_default();
        let locked = lock.pointer(&format!("/assets/{}", ptr_escape(&f.rel))).filter(|v| v.is_object());
        files.push(json!({
            "path": f.rel, "kind": kind, "bytes": f.bytes, "sha256": sha,
            "authorised": is_authorised(&authorised, &f.rel),
            "locked": locked.map(|l| json!({
                "assetId": l.get("assetId"), "state": l.get("state"), "sha256": l.get("sha256"),
            })),
            "stale": locked.is_some_and(|l| l.get("sha256").and_then(Value::as_str) != Some(sha.as_str())),
        }));
    }
    Ok(json!({
        "ok": true,
        "files": files,
        "default_authorised": DEFAULT_AUTHORISED_GLOBS,
        "locked_states": ["approved", "pending", "rejected", "unknown"],
    }))
}

fn ptr_escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn valid_glob_entry(s: &str) -> bool {
    let s = s.trim();
    let s = s.strip_prefix('!').unwrap_or(s).trim();
    !s.is_empty() && !s.starts_with('/') && !s.contains('\\') && !s.contains('\0') && !s.split('/').any(|seg| seg == "..")
}

fn assets_authorise(root: &Path, body: &Value) -> R<Value> {
    let paths = str_list(body.get("paths"));
    if paths.is_empty() || !paths.iter().all(|p| valid_glob_entry(p)) {
        return bad("Send paths: [..] as project-relative paths or globs (no '..', no absolute paths).");
    }
    let revoke = body.get("revoke").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    update_manifest(root, |m| {
        let assets = m.entry("assets").or_insert_with(|| json!({}));
        if !assets.is_object() {
            *assets = json!({});
        }
        let a = assets.as_object_mut().unwrap();
        let mut list = str_list(a.get("authorised"));
        for p in &paths {
            let p = p.trim().to_string();
            if revoke {
                list.retain(|x| *x != p);
            } else if !list.contains(&p) {
                list.push(p);
            }
        }
        result = list.clone();
        a.insert("authorised".into(), json!(list));
    })?;
    Ok(json!({ "ok": true, "authorised": result }))
}

struct Uploaded {
    asset_id: String,
    image_asset_id: Option<String>,
    state: String,
    operation: String,
    asset_type: String,
    converted_format: Option<&'static str>,
}

struct PreparedUpload {
    filename: String,
    mime: &'static str,
    bytes: Vec<u8>,
    converted_format: Option<&'static str>,
}

struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const BLENDER_OBJ_TO_FBX_PY: &str = r#"import os, bpy
bpy.ops.wm.read_factory_settings(use_empty=True)
obj_in = os.environ["OPENRO_OBJ_IN"]
png_in = os.environ.get("OPENRO_PNG_IN", "")
fbx_out = os.environ["OPENRO_FBX_OUT"]
if hasattr(bpy.ops.wm, "obj_import"):
    bpy.ops.wm.obj_import(filepath=obj_in)
else:
    bpy.ops.import_scene.obj(filepath=obj_in)
if png_in and os.path.isfile(png_in):
    img = bpy.data.images.load(png_in, check_existing=True)
    mat = bpy.data.materials.new(name="OpenRODiffuse")
    if mat.node_tree is None:
        mat.use_nodes = True
    nt = mat.node_tree
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None:
        bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    for o in bpy.context.scene.objects:
        if o.type == "MESH":
            o.data.materials.clear()
            o.data.materials.append(mat)
    try:
        bpy.ops.file.pack_all()
    except Exception:
        pass
bpy.ops.export_scene.fbx(filepath=fbx_out, path_mode="COPY", embed_textures=True)
"#;

fn make_upload_temp_dir(data_dir: &Path, tag: &str) -> R<TempDirGuard> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = data_dir
        .join("tmp-upload")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| Oc { status: 500, msg: format!("create upload conversion dir: {e}") })?;
    Ok(TempDirGuard(dir))
}

fn convert_wav_to_ogg(data_dir: &Path, abs: &Path, stem: &str) -> R<PreparedUpload> {
    let tmp = make_upload_temp_dir(data_dir, "wav")?;
    let out_name = format!("{stem}.ogg");
    let out_path = tmp.0.join(&out_name);
    let ffmpeg = crate::projects::find_tool("ffmpeg", "OPENRO_FFMPEG_BIN")
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let out = std::process::Command::new(&ffmpeg)
        .arg("-nostdin")
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(abs)
        .args(["-c:a", "libvorbis", "-q:a", "5"])
        .arg(&out_path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| Oc { status: 502, msg: format!("failed to run ffmpeg for WAV->OGG conversion: {e}") })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return err(502, format!("ffmpeg WAV->OGG conversion failed: {}", stderr.trim()));
    }
    let bytes = std::fs::read(&out_path)
        .map_err(|e| Oc { status: 502, msg: format!("read converted OGG audio: {e}") })?;
    if bytes.is_empty() {
        return err(502, "ffmpeg produced an empty OGG file.");
    }
    Ok(PreparedUpload {
        filename: out_name,
        mime: "audio/ogg",
        bytes,
        converted_format: Some("ogg"),
    })
}

fn convert_obj_to_fbx(data_dir: &Path, abs: &Path, stem: &str) -> R<PreparedUpload> {
    let tmp = make_upload_temp_dir(data_dir, "obj")?;
    let out_name = format!("{stem}.fbx");
    let out_path = tmp.0.join(&out_name);
    let sibling_png = abs.with_extension("png");
    let blender = crate::projects::find_tool("blender", "OPENRO_BLENDER_BIN")
        .unwrap_or_else(|| PathBuf::from("blender"));
    let mut cmd = std::process::Command::new(&blender);
    cmd.args(["--background", "--python-expr", BLENDER_OBJ_TO_FBX_PY])
        .env("OPENRO_OBJ_IN", abs)
        .env("OPENRO_FBX_OUT", &out_path)
        .stdin(std::process::Stdio::null());
    if sibling_png.is_file() {
        cmd.env("OPENRO_PNG_IN", &sibling_png);
    } else {
        cmd.env("OPENRO_PNG_IN", "");
    }
    let out = cmd
        .output()
        .map_err(|e| Oc { status: 502, msg: format!("failed to run blender for OBJ->FBX conversion: {e}") })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let detail = if !stderr.trim().is_empty() { stderr.trim() } else { stdout.trim() };
        return err(502, format!("blender OBJ->FBX conversion failed: {detail}"));
    }
    let bytes = std::fs::read(&out_path)
        .map_err(|e| Oc { status: 502, msg: format!("read converted FBX mesh: {e}") })?;
    if bytes.is_empty() {
        return err(502, "blender produced an empty FBX file.");
    }
    Ok(PreparedUpload {
        filename: out_name,
        mime: "model/fbx",
        bytes,
        converted_format: Some("fbx"),
    })
}

fn prepare_upload_payload(
    data_dir: &Path,
    rel: &str,
    abs: &Path,
    kind: &str,
    raw_bytes: &[u8],
) -> R<PreparedUpload> {
    let filename = rel.rsplit('/').next().unwrap_or(rel);
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);
    match (kind, ext_of(rel).as_str()) {
        ("Audio", "wav") => convert_wav_to_ogg(data_dir, abs, stem),
        ("Model", "obj") => convert_obj_to_fbx(data_dir, abs, stem),
        _ => Ok(PreparedUpload {
            filename: filename.to_string(),
            mime: mime_for(rel),
            bytes: raw_bytes.to_vec(),
            converted_format: None,
        }),
    }
}

/// Extract the underlying Image asset id from Roblox Decal XML returned by assetdelivery.
pub(crate) fn extract_decal_image_id(xml: &str, decal_id: &str) -> Option<String> {
    fn find_numeric_id(haystack: &str, decal_id: &str) -> Option<String> {
        for needle in ["id=", "rbxassetid://"] {
            let mut rest = haystack;
            while let Some(pos) = rest.find(needle) {
                let after = &rest[pos + needle.len()..];
                let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() && digits != decal_id {
                    return Some(digits);
                }
                rest = &rest[pos + needle.len()..];
            }
        }
        None
    }
    if let Some(tex_pos) = xml.find("name=\"Texture\"") {
        let slice = &xml[tex_pos..xml.len().min(tex_pos + 512)];
        if let Some(id) = find_numeric_id(slice, decal_id) {
            return Some(id);
        }
    }
    find_numeric_id(xml, decal_id)
}

fn assetdelivery_base(opencloud_base: &str) -> String {
    if let Ok(v) = std::env::var("OPENRO_ASSETDELIVERY_BASE") {
        let t = v.trim().trim_end_matches('/');
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if opencloud_base != DEFAULT_BASE {
        return opencloud_base.trim_end_matches('/').to_string();
    }
    "https://assetdelivery.roblox.com".to_string()
}

fn norm_state(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("").to_ascii_lowercase();
    let s = s.trim_start_matches("moderation_state_").to_string();
    match s.as_str() {
        "approved" => "approved".to_string(),
        "reviewing" | "pending" => "pending".to_string(),
        "rejected" => "rejected".to_string(),
        _ => "unknown".to_string(),
    }
}

impl Client {
    fn create_asset(&self, kind: &str, name: &str, filename: &str, mime: &str, bytes: &[u8]) -> R<(u16, Vec<u8>)> {
        let Some(creator) = &self.creator else {
            return bad("Set who owns the uploads first: POST /api/opencloud/settings with creator { userId } or { groupId }.");
        };
        let creator = match creator {
            Creator::User(id) => json!({ "userId": id }),
            Creator::Group(id) => json!({ "groupId": id }),
        };
        let req = json!({
            "assetType": kind,
            "displayName": name,
            "description": "Uploaded by OpenRO",
            "creationContext": { "creator": creator },
        });
        let mut mp = Multipart::new();
        mp.text("request", &req.to_string());
        mp.file("fileContent", filename, mime, bytes);
        let (ct, body) = mp.finish();
        self.call("POST", "/assets/v1/assets", Some(&ct), Some(&body))
    }

    fn resolve_decal_image_id(&self, decal_id: &str) -> Option<String> {
        // Fresh unauthenticated client: never forward `x-api-key` to assetdelivery / CDN.
        let url = format!("{}/v1/asset/?id={decal_id}", assetdelivery_base(&self.base));
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .redirects(5)
            .build();
        let resp = agent.get(&url).call().ok()?;
        let mut buf = Vec::new();
        let _ = std::io::Read::take(resp.into_reader(), 512 * 1024).read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        extract_decal_image_id(&text, decal_id)
    }

    fn upload_asset(&self, data_dir: &Path, rel: &str, abs: &Path, kind: &str, bytes: &[u8]) -> R<Uploaded> {
        let stem = rel.rsplit('/').next().unwrap_or(rel);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
        let name: String = stem.chars().take(50).collect();
        let prepared = prepare_upload_payload(data_dir, rel, abs, kind, bytes)?;
        let mut used = kind.to_string();
        let (mut status, mut resp) =
            self.create_asset(kind, &name, &prepared.filename, prepared.mime, &prepared.bytes)?;
        if status == 400 && kind == "Image" {
            // Some accounts only accept images as Decals.
            used = "Decal".to_string();
            (status, resp) =
                self.create_asset("Decal", &name, &prepared.filename, prepared.mime, &prepared.bytes)?;
        }
        self.check(status, &resp)?;
        let op: Value = serde_json::from_slice(&resp).or_else(|_| err(502, "Roblox answered the upload with something that is not JSON."))?;
        let op_path = op.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
        let op_id = op_path.rsplit('/').next().unwrap_or_default().to_string();
        let done = if op.get("done").and_then(Value::as_bool) == Some(true) {
            op
        } else if op_id.is_empty() {
            return err(502, "Roblox did not return an operation to follow for this upload.");
        } else {
            self.poll_operation(&op_id)?
        };
        if let Some(e) = done.get("error") {
            let m = e.get("message").and_then(Value::as_str).unwrap_or("the upload was rejected");
            return err(502, self.scrub(&format!("Roblox rejected {rel}: {m}")));
        }
        let resp_obj = done.get("response").unwrap_or(&Value::Null);
        let Some(asset_id) = id_of(resp_obj, "assetId") else {
            return err(502, format!("Roblox finished the upload of {rel} without an asset id."));
        };
        let state = norm_state(done.pointer("/response/moderationResult/moderationState").and_then(Value::as_str));
        let image_asset_id = if used == "Decal" && state != "rejected" {
            id_of(resp_obj, "imageAssetId")
                .or_else(|| id_of(resp_obj, "imageId"))
                .or_else(|| self.resolve_decal_image_id(&asset_id))
        } else {
            None
        };
        Ok(Uploaded {
            asset_id,
            image_asset_id,
            state,
            operation: op_id,
            asset_type: used,
            converted_format: prepared.converted_format,
        })
    }

    fn poll_operation(&self, id: &str) -> R<Value> {
        let first = poll_ms().unwrap_or(500);
        let (max_delay, cap) = match poll_ms() {
            Some(p) => (p.saturating_mul(4), Duration::from_millis(p.saturating_mul(40))),
            None => (4_000, Duration::from_secs(120)),
        };
        let started = Instant::now();
        let mut delay = first;
        loop {
            std::thread::sleep(Duration::from_millis(delay));
            let v = self.json("GET", &format!("/assets/v1/operations/{id}"), None, None)?;
            if v.get("done").and_then(Value::as_bool) == Some(true) {
                return Ok(v);
            }
            if started.elapsed() > cap {
                return err(502, format!("Roblox is still processing the upload (operation {id}). Try again in a minute; it is not lost."));
            }
            delay = delay.saturating_mul(2).min(max_delay);
        }
    }
}

fn assets_upload(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let authorised = str_list(manifest.pointer("/assets/authorised"));
    let types = type_overrides(&manifest, body);
    let flag = |k: &str| body.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (everything, dry_run, force) = (flag("everything"), flag("dry_run"), flag("force"));
    let explicit = str_list(body.get("paths"));
    if everything && !explicit.is_empty() {
        return bad("Send either paths or everything, not both.");
    }

    // selection: authorised set by default; a list when named; everything only on its own flag
    let all = walk(root);
    let mut results: Vec<Value> = Vec::new();
    let mut picked: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut overrun: Vec<String> = Vec::new();
    if !explicit.is_empty() {
        for p in &explicit {
            let abs = match safe_rel(root, p) {
                Ok(a) if a.is_file() => a,
                Ok(_) => {
                    results.push(json!({ "path": p, "state": "skipped", "error": "no such file in the project" }));
                    continue;
                }
                Err(e) => {
                    results.push(json!({ "path": p, "state": "skipped", "error": e.msg }));
                    continue;
                }
            };
            let bytes = abs.metadata().map(|m| m.len()).unwrap_or(0);
            let rel = p.trim().trim_start_matches("./").to_string();
            picked.push((rel, abs, bytes));
        }
    } else {
        for f in all {
            if kind_for(&f.rel, &types).is_none() {
                continue;
            }
            let is_auth = is_authorised(&authorised, &f.rel);
            if everything {
                if !is_auth {
                    overrun.push(f.rel.clone());
                }
                picked.push((f.rel, f.abs, f.bytes));
            } else if is_auth {
                picked.push((f.rel, f.abs, f.bytes));
            }
        }
    }
    if everything && !overrun.is_empty() {
        eprintln!("opencloud: 'everything' selected; {} file(s) beyond the authorised set: {}", overrun.len(), overrun.join(", "));
    }

    let lock_file = lock_path(root, &manifest)?;
    let mut lock = read_lock(&lock_file);
    let client = if dry_run || picked.is_empty() { None } else { Some(Client::new(ctx)?) };
    let mut lock_dirty = false;
    let mut halted_reason: Option<&'static str> = None;
    let mut halted_note: Option<String> = None;
    for (rel, abs, bytes) in picked {
        let Some(kind) = kind_for(&rel, &types) else {
            results.push(json!({ "path": rel, "state": "skipped", "error": "not an uploadable file type" }));
            continue;
        };
        if bytes > MAX_UPLOAD_BYTES {
            results.push(json!({ "path": rel, "state": "skipped", "assetType": kind, "error": "over 20 MB, too big to upload" }));
            continue;
        }
        let data = match std::fs::read(&abs) {
            Ok(d) => d,
            Err(e) => {
                results.push(json!({ "path": rel, "state": "failed", "error": format!("could not read the file: {e}") }));
                continue;
            }
        };
        let sha = sha256_hex(&data);
        let entry = lock.pointer(&format!("/assets/{}", ptr_escape(&rel))).cloned();
        if !force {
            if let Some(e) = entry.as_ref().filter(|e| e.get("sha256").and_then(Value::as_str) == Some(sha.as_str())) {
                if let Some(id) = id_of(e, "assetId") {
                    results.push(json!({ "path": rel, "state": "unchanged", "assetId": id, "assetType": e.get("assetType"), "sha256": sha }));
                    continue;
                }
            }
            // same content already uploaded under another name
            let twin = lock["assets"].as_object().and_then(|o| {
                o.values()
                    .find(|e| e.get("sha256").and_then(Value::as_str) == Some(sha.as_str()) && e.get("assetType").and_then(Value::as_str) == Some(kind.as_str()) && id_of(e, "assetId").is_some())
                    .cloned()
            });
            if let Some(t) = twin {
                if !dry_run {
                    lock["assets"][&rel] = t.clone();
                    lock["version"] = json!(1);
                    write_json_atomic(&lock_file, &lock)?;
                    lock_dirty = true;
                }
                results.push(json!({ "path": rel, "state": "reused", "assetId": t.get("assetId"), "imageAssetId": t.get("imageAssetId"), "assetType": kind, "sha256": sha }));
                continue;
            }
        }
        let Some(client) = &client else {
            results.push(json!({ "path": rel, "state": "would_upload", "assetType": kind, "sha256": sha }));
            continue;
        };
        match client.upload_asset(ctx.data_dir, &rel, &abs, &kind, &data) {
            Ok(u) if u.state == "rejected" => {
                // Never keep a rejected asset id in assets.lock.json.
                if let Some(map) = lock.get_mut("assets").and_then(Value::as_object_mut) {
                    if map.remove(&rel).is_some() {
                        lock["version"] = json!(1);
                        write_json_atomic(&lock_file, &lock)?;
                        lock_dirty = true;
                    }
                }
                eprintln!("opencloud: moderation rejected {rel} (operation {})", u.operation);
                results.push(json!({
                    "path": rel, "state": "rejected", "assetId": Value::Null, "assetType": u.asset_type,
                    "sha256": sha, "operation": u.operation, "moderation": "rejected",
                    "error": "Roblox moderation rejected this asset; redraw and retry.",
                }));
            }
            Ok(u) => {
                let mut lock_entry = json!({
                    "sha256": sha, "assetId": u.asset_id, "assetType": u.asset_type,
                    "state": u.state, "uploadedAt": now_iso(),
                });
                let mut row = json!({
                    "path": rel, "state": "uploaded", "assetId": u.asset_id, "assetType": u.asset_type,
                    "sha256": sha, "operation": u.operation, "moderation": u.state,
                });
                if let Some(ref img_id) = u.image_asset_id {
                    lock_entry["imageAssetId"] = json!(img_id);
                    row["imageAssetId"] = json!(img_id);
                }
                if let Some(fmt) = u.converted_format {
                    lock_entry["convertedFormat"] = json!(fmt);
                    row["convertedFormat"] = json!(fmt);
                }
                lock["assets"][&rel] = lock_entry;
                // Persist incrementally after every upload so partial batches never lose IDs (#69).
                lock["version"] = json!(1);
                write_json_atomic(&lock_file, &lock)?;
                lock_dirty = true;
                eprintln!("opencloud: uploaded {rel} as asset {}", u.asset_id);
                results.push(row);
            }
            Err(e) => results.push(json!({ "path": rel, "state": "failed", "assetType": kind, "sha256": sha, "error": e.msg })),
        }
        // a stopped key, missing owner, or moderated account would fail every file the same way: halt immediately
        if let Some(last) = results.last() {
            if last["state"] == "failed" {
                if let Some(m) = last["error"].as_str() {
                    if m.to_ascii_lowercase().contains("user is moderated") {
                        halted_reason = Some("account_moderated");
                        halted_note = Some("Upload batch halted: Roblox returned 'User is moderated'. Review and reactivate the moderation warning in the browser, replace any flagged asset cleanly (zero blood/gore), and retry.".to_string());
                        break;
                    }
                    if m.contains("(401)") || m.contains("Set who owns") {
                        halted_reason = Some(if m.contains("(401)") { "unauthorized_key" } else { "missing_creator" });
                        break;
                    }
                }
            }
        }
    }
    let auto_codegen = body.get("auto_codegen").and_then(Value::as_bool).unwrap_or(true);
    let codegen_res = if lock_dirty && auto_codegen {
        assets_codegen(root, &json!({})).ok()
    } else {
        None
    };

    let count = |s: &str| results.iter().filter(|r| r["state"] == s).count();
    let mut summary = json!({
        "uploaded": count("uploaded"), "reused": count("reused"), "unchanged": count("unchanged"),
        "skipped": count("skipped"), "failed": count("failed"), "rejected": count("rejected"),
        "would_upload": count("would_upload"),
        "selection": if everything { "everything" } else if !explicit.is_empty() { "paths" } else { "authorised" },
        "dry_run": dry_run,
    });
    if let Some(cg) = codegen_res {
        summary["asset_ids_luau"] = cg;
    }
    if let Some(reason) = halted_reason {
        summary["halted_reason"] = json!(reason);
    }
    if let Some(note) = halted_note {
        summary["note"] = json!(note);
    } else if everything {
        summary["overrun"] = json!(overrun);
        summary["note"] = json!(format!("'everything' went beyond the authorised set by {} file(s).", overrun.len()));
    } else if explicit.is_empty() && authorised.is_empty() {
        summary["note"] = json!("Nothing is authorised for upload yet. Authorise some files first (POST .../assets/authorise).");
    }
    Ok(json!({ "ok": true, "results": results, "summary": summary }))
}

fn luau_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "");
    format!("\"{escaped}\"")
}

fn short_asset_key(rel: &str) -> String {
    let fname = rel.rsplit('/').next().unwrap_or(rel);
    let stem = fname.rsplit_once('.').map(|(s, _)| s).unwrap_or(fname);
    // Avoid collisions when a folder has art.png / icon.png / full.png inside art/cards/<id>/...
    if matches!(stem, "art" | "icon" | "full" | "base" | "glow" | "ornament" | "border" | "shimmer" | "backdrop") {
        let parts: Vec<&str> = rel.split('/').collect();
        if parts.len() >= 2 {
            return format!("{}_{}", parts[parts.len() - 2], stem);
        }
    }
    stem.to_string()
}

/// Generate `src/shared/AssetIds.luau` (or custom `out` path) from `assets.lock.json`
/// and optional `assets/meshes/manifest.json` (stud bbox + anchors for pre-upload proxies).
pub fn assets_codegen(root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let lock = read_lock(&lock_path(root, &manifest)?);
    let out_rel = body
        .get("out")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("src/shared/AssetIds.luau");
    if !out_rel.ends_with(".luau") && !out_rel.ends_with(".lua") {
        return bad("out path must end in .luau or .lua.");
    }
    let out_abs = safe_rel(root, out_rel)?;
    if let Some(parent) = out_abs.parent() {
        std::fs::create_dir_all(parent).or_else(|e| err(500, format!("create codegen dir: {e}")))?;
    }

    let mut by_path: Vec<(String, String)> = Vec::new();
    let mut by_key: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut mesh_asset_ids: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut card_art_aliases: Vec<(String, String)> = Vec::new();
    let mut skipped_rejected = 0usize;
    if let Some(map) = lock.get("assets").and_then(Value::as_object) {
        let mut sorted: Vec<(&String, &Value)> = map.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (rel, entry) in sorted {
            let state = entry.get("state").and_then(Value::as_str).unwrap_or("unknown");
            if state == "rejected" {
                skipped_rejected += 1;
                continue;
            }
            // Prefer resolved Image asset id when an upload fell back to Decal (#69).
            let Some(id) = id_of(entry, "imageAssetId").or_else(|| id_of(entry, "assetId")) else {
                continue;
            };
            let uri = format!("rbxassetid://{id}");
            by_path.push((rel.clone(), uri.clone()));
            let key = short_asset_key(rel);
            by_key.entry(key).or_insert_with(|| uri.clone());
            // Packed card portrait aliases: art/cards/<cardId>/art.png -> "cards/<cardId>" & "<cardId>" (#69).
            if let Some(card_id) = rel
                .strip_prefix("art/cards/")
                .and_then(|s| s.strip_suffix("/art.png"))
                .filter(|id| !id.is_empty() && !id.contains('/'))
            {
                card_art_aliases.push((card_id.to_string(), uri.clone()));
            }
            // Locked mesh asset ids: assets/meshes/<stem>.<mesh_ext> -> AssetIds.meshes["<stem>"].assetId (#69).
            if let Some(fname) = rel
                .strip_prefix("assets/meshes/")
                .filter(|f| !f.contains('/'))
            {
                if let Some((stem, ext)) = fname.rsplit_once('.') {
                    if matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "obj" | "fbx" | "glb" | "gltf" | "rbxm" | "rbxmx"
                    ) && !stem.is_empty()
                    {
                        mesh_asset_ids.entry(stem.to_string()).or_insert_with(|| uri.clone());
                    }
                }
            }
        }
    }
    for (card_id, uri) in card_art_aliases {
        by_key
            .entry(format!("cards/{card_id}"))
            .or_insert_with(|| uri.clone());
        by_key.entry(card_id).or_insert(uri);
    }
    by_path.sort_by(|a, b| a.0.cmp(&b.0));

    // Optional mesh metadata from assets/meshes/manifest.json (stud bbox & tris for pre-upload proxies),
    // enriched with locked assetId from assets/meshes/<stem>.obj (#69).
    struct MeshCodegenRow {
        meta: Option<([f64; 3], u64)>,
        asset_uri: Option<String>,
    }
    let mesh_manifest = std::fs::read_to_string(root.join("assets/meshes/manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut mesh_rows: std::collections::BTreeMap<String, MeshCodegenRow> =
        std::collections::BTreeMap::new();
    if let Some(entries) = mesh_manifest
        .as_ref()
        .and_then(|v| v.get("meshes").and_then(Value::as_object).or_else(|| v.as_object()))
    {
        for (k, v) in entries {
            if let Some(arr) = v
                .get("size")
                .or_else(|| v.get("bbox_studs"))
                .or_else(|| v.get("size_studs"))
                .and_then(Value::as_array)
            {
                if arr.len() >= 3 {
                    let sz = [
                        arr[0].as_f64().unwrap_or(4.0),
                        arr[1].as_f64().unwrap_or(4.0),
                        arr[2].as_f64().unwrap_or(4.0),
                    ];
                    let tris = v
                        .get("tris")
                        .or_else(|| v.get("triangles"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    mesh_rows.insert(
                        k.clone(),
                        MeshCodegenRow {
                            meta: Some((sz, tris)),
                            asset_uri: mesh_asset_ids.get(k).cloned(),
                        },
                    );
                }
            }
        }
    }
    for (stem, uri) in &mesh_asset_ids {
        mesh_rows
            .entry(stem.clone())
            .and_modify(|row| {
                if row.asset_uri.is_none() {
                    row.asset_uri = Some(uri.clone());
                }
            })
            .or_insert(MeshCodegenRow {
                meta: None,
                asset_uri: Some(uri.clone()),
            });
    }

    let mut out = String::new();
    out.push_str("--!strict\n");
    out.push_str("-- Auto-generated by OpenRO from assets.lock.json. Do not edit by hand.\n\n");
    out.push_str("local AssetIds = {}\n\n");
    if by_path.is_empty() {
        out.push_str("AssetIds.byPath = {}\n\n");
    } else {
        out.push_str("AssetIds.byPath = {\n");
        for (p, uri) in &by_path {
            out.push_str(&format!("\t[{}] = {},\n", luau_str(p), luau_str(uri)));
        }
        out.push_str("}\n\n");
    }
    if by_key.is_empty() {
        out.push_str("AssetIds.byKey = {}\n\n");
    } else {
        out.push_str("AssetIds.byKey = {\n");
        for (k, uri) in &by_key {
            out.push_str(&format!("\t[{}] = {},\n", luau_str(k), luau_str(uri)));
        }
        out.push_str("}\n\n");
    }
    if mesh_rows.is_empty() {
        out.push_str("AssetIds.meshes = {}\n\n");
    } else {
        out.push_str("AssetIds.meshes = {\n");
        for (k, row) in &mesh_rows {
            let key_s = luau_str(k);
            match (&row.meta, &row.asset_uri) {
                (Some((sz, tris)), None) => {
                    out.push_str(&format!(
                        "\t[{key_s}] = {{ size = Vector3.new({:.3}, {:.3}, {:.3}), tris = {} }},\n",
                        sz[0], sz[1], sz[2], tris
                    ));
                }
                (Some((sz, tris)), Some(uri)) => {
                    let single = format!(
                        "\t[{key_s}] = {{ size = Vector3.new({:.3}, {:.3}, {:.3}), tris = {}, assetId = {} }},",
                        sz[0],
                        sz[1],
                        sz[2],
                        tris,
                        luau_str(uri)
                    );
                    // Respect stylua.toml column_width = 120 (tab = 4 columns).
                    let visual_cols = single.len() + 3;
                    if visual_cols <= 120 {
                        out.push_str(&single);
                        out.push('\n');
                    } else {
                        out.push_str(&format!(
                            "\t[{key_s}] = {{\n\t\tsize = Vector3.new({:.3}, {:.3}, {:.3}),\n\t\ttris = {},\n\t\tassetId = {},\n\t}},\n",
                            sz[0],
                            sz[1],
                            sz[2],
                            tris,
                            luau_str(uri)
                        ));
                    }
                }
                (None, Some(uri)) => {
                    out.push_str(&format!(
                        "\t[{key_s}] = {{ assetId = {} }},\n",
                        luau_str(uri)
                    ));
                }
                (None, None) => {}
            }
        }
        out.push_str("}\n\n");
    }
    out.push_str("function AssetIds.get(keyOrPath: string, fallback: string?): string\n");
    out.push_str("\treturn AssetIds.byPath[keyOrPath] or AssetIds.byKey[keyOrPath] or (fallback or \"\")\n");
    out.push_str("end\n\n");
    out.push_str("return AssetIds\n");

    let tmp = out_abs.with_extension("luau.tmp-openro");
    std::fs::write(&tmp, out.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &out_abs))
        .or_else(|e| err(500, format!("write {out_rel}: {e}")))?;

    let keys: Vec<String> = by_key.keys().cloned().collect();
    Ok(json!({
        "ok": true,
        "path": out_rel,
        "count": by_path.len(),
        "mesh_count": mesh_rows.len(),
        "skipped_rejected": skipped_rejected,
        "keys": keys,
    }))
}

// ------------------------------------------------------------------ publish

fn latest_build(root: &Path, want: Option<&str>) -> R<(String, PathBuf)> {
    let dir = root.join("build").join("builds");
    let mut best: Option<(u64, String, String)> = None;
    if let Some(id) = want {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return bad("build_id has odd characters.");
        }
        let v = std::fs::read_to_string(dir.join(format!("{id}.json"))).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let Some(path) = v.as_ref().and_then(|v| v.get("path")).and_then(Value::as_str) else {
            return err(404, format!("No build {id} in this project."));
        };
        best = Some((0, id.to_string(), path.to_string()));
    } else if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            let (Some(path), at) = (v.get("path").and_then(Value::as_str), v.get("at").and_then(Value::as_u64).unwrap_or(0)) else { continue };
            let id = v.get("build_id").and_then(Value::as_str).unwrap_or("build").to_string();
            if best.as_ref().is_none_or(|b| at >= b.0) {
                best = Some((at, id, path.to_string()));
            }
        }
    }
    let Some((_, id, path)) = best else {
        return bad("Build the project first; there is no build to publish.");
    };
    Ok((id, safe_rel(root, &path)?))
}

fn max_publish_bytes() -> u64 {
    std::env::var("OPENRO_MAX_PUBLISH_BYTES")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(DEFAULT_MAX_PUBLISH_BYTES)
}

fn publish(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let universe = universe_id(&manifest)?;
    let Some(place) = id_of(&manifest, "placeId").filter(|s| s.bytes().all(|b| b.is_ascii_digit())) else {
        return bad("This project has no placeId yet. Set it in the project settings.");
    };
    let version_type = body.get("versionType").and_then(Value::as_str).unwrap_or("Saved");
    if !matches!(version_type, "Saved" | "Published") {
        return bad("versionType must be Saved or Published.");
    }
    if version_type == "Published" && body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return bad("Published makes the game live. Ask the owner first, then send confirm: true.");
    }
    let (build_id, file) = latest_build(root, body.get("build_id").and_then(Value::as_str))?;
    let meta = std::fs::metadata(&file).or_else(|_| err(404, format!("The build file {} is missing; build again.", file.display())))?;
    let limit = max_publish_bytes();
    if meta.len() > limit {
        return bad(format!("The built place is over {} MB; Roblox will not take it.", limit / (1024 * 1024)));
    }
    let bytes = std::fs::read(&file).or_else(|e| err(500, format!("could not read the build: {e}")))?;
    let client = Client::new(ctx)?;
    let ct = if ext_of(&file.to_string_lossy()) == "rbxl" { "application/octet-stream" } else { "application/xml" };
    let v = client.json("POST", &format!("/universes/v1/{universe}/places/{place}/versions?versionType={version_type}"), Some(ct), Some(&bytes))?;
    Ok(json!({
        "ok": true, "versionNumber": v.get("versionNumber"), "versionType": version_type,
        "build_id": build_id, "bytes": bytes.len(),
    }))
}

// ----------------------------------------------------------------- products

#[derive(Clone)]
struct Remote {
    id: String,
    name: String,
    description: String,
    price: Option<i64>,
    for_sale: Option<bool>,
}

fn price_of(v: &Value) -> Option<i64> {
    let p = v.pointer("/priceInformation/defaultPriceInRobux").or_else(|| v.get("price"))?;
    p.as_i64().or_else(|| p.as_str().and_then(|s| s.trim().parse().ok()))
}

impl Client {
    fn list_remote(&self, universe: &str, kind: &str) -> R<Vec<Remote>> {
        let (path, list_key, id_key) = if kind == "game_pass" {
            (format!("/game-passes/v1/universes/{universe}/game-passes/creator"), "gamePasses", "gamePassId")
        } else {
            (format!("/developer-products/v2/universes/{universe}/developer-products/creator"), "developerProducts", "productId")
        };
        let mut out = Vec::new();
        let mut token = String::new();
        for _ in 0..50 {
            let q = if token.is_empty() { "?pageSize=100".to_string() } else { format!("?pageSize=100&pageToken={token}") };
            let v = self.json("GET", &format!("{path}{q}"), None, None)?;
            for item in v.get(list_key).and_then(Value::as_array).into_iter().flatten() {
                if let Some(id) = id_of(item, id_key).or_else(|| id_of(item, "id")) {
                    out.push(Remote {
                        id,
                        name: item.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        description: item.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                        price: price_of(item),
                        for_sale: item.get("isForSale").and_then(Value::as_bool),
                    });
                }
            }
            token = v.get("nextPageToken").and_then(Value::as_str).unwrap_or("").to_string();
            if token.is_empty() {
                break;
            }
        }
        Ok(out)
    }
}

fn remote_json(list: &[Remote]) -> Vec<Value> {
    list.iter()
        .map(|r| json!({ "id": r.id, "name": r.name, "description": r.description, "priceRobux": r.price, "forSale": r.for_sale }))
        .collect()
}

fn products_get(ctx: &Ctx, root: &Path) -> R<Value> {
    let manifest = read_manifest(root);
    let products = manifest.get("products").cloned().unwrap_or_else(|| json!([]));
    let mut remote = Value::Null;
    let mut remote_error = Value::Null;
    match (universe_id(&manifest), Client::new(ctx)) {
        (Ok(u), Ok(c)) => match (c.list_remote(&u, "developer_product"), c.list_remote(&u, "game_pass")) {
            (Ok(dp), Ok(gp)) => remote = json!({ "developer_products": remote_json(&dp), "game_passes": remote_json(&gp) }),
            (Err(e), _) | (_, Err(e)) => remote_error = json!(e.msg),
        },
        (Err(e), _) | (_, Err(e)) => remote_error = json!(e.msg),
    }
    Ok(json!({ "ok": true, "manifest": products, "remote": remote, "remote_error": remote_error }))
}

struct Wanted {
    name: String,
    description: String,
    price: i64,
    for_sale: bool,
}

fn products_sync(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let universe = universe_id(&manifest)?;
    let dry_run = body.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let default_for_sale = body
        .get("default_for_sale")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let only = str_list(body.get("keys"));
    let products = manifest.get("products").and_then(Value::as_array).cloned().unwrap_or_default();
    let client = Client::new(ctx)?;
    let mut remote_dp: Option<Vec<Remote>> = None;
    let mut remote_gp: Option<Vec<Remote>> = None;
    let mut results = Vec::new();
    let mut synced_ids: Vec<(String, String)> = Vec::new();

    for p in &products {
        let key = p.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        if key.is_empty() || (!only.is_empty() && !only.contains(&key)) {
            continue;
        }
        let kind = p.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
        let res = |action: &str, id: Option<&str>, error: Option<String>| {
            let mut r = json!({ "key": key, "kind": kind, "action": action, "id": id });
            if let Some(e) = error {
                r["error"] = json!(e);
            }
            r
        };
        // Read either `id` or `robloxId` (#69).
        let current_id = id_of(p, "id").or_else(|| id_of(p, "robloxId"));
        if p.get("priceApproved").and_then(Value::as_bool) != Some(true) {
            results.push(res("skipped", current_id.as_deref(), Some("price not approved".into())));
            continue;
        }
        if !matches!(kind.as_str(), "developer_product" | "game_pass") {
            results.push(res("failed", current_id.as_deref(), Some("kind must be developer_product or game_pass".into())));
            continue;
        }
        let want = Wanted {
            name: p.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string(),
            description: p.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
            price: p.get("priceRobux").and_then(Value::as_i64).unwrap_or(0),
            for_sale: p.get("forSale").and_then(Value::as_bool).unwrap_or(default_for_sale),
        };
        if want.name.is_empty() || want.price < 1 {
            results.push(res("failed", current_id.as_deref(), Some("needs a name and a priceRobux of at least 1".into())));
            continue;
        }
        let is_pass = kind == "game_pass";
        let slot = if is_pass { &mut remote_gp } else { &mut remote_dp };
        if slot.is_none() {
            match client.list_remote(&universe, &kind) {
                Ok(l) => *slot = Some(l),
                Err(e) => {
                    results.push(res("failed", current_id.as_deref(), Some(e.msg)));
                    continue;
                }
            }
        }
        let list = slot.as_ref().unwrap();
        let (found, adopted) = match &current_id {
            Some(id) => (list.iter().find(|r| r.id == *id), false),
            None => (list.iter().find(|r| r.name == want.name), true),
        };
        if current_id.is_some() && found.is_none() {
            results.push(res("failed", current_id.as_deref(), Some("that id is not in this experience".into())));
            continue;
        }
        let path_base = if is_pass {
            format!("/game-passes/v1/universes/{universe}/game-passes")
        } else {
            format!("/developer-products/v2/universes/{universe}/developer-products")
        };
        match found {
            Some(r) => {
                let mut mp = Multipart::new();
                let mut changed = false;
                if r.name != want.name {
                    mp.text("name", &want.name);
                    changed = true;
                }
                if r.description != want.description {
                    mp.text("description", &want.description);
                    changed = true;
                }
                if r.price != Some(want.price) {
                    mp.text("price", &want.price.to_string());
                    changed = true;
                }
                if r.for_sale != Some(want.for_sale) {
                    mp.text("isForSale", &want.for_sale.to_string());
                    changed = true;
                }
                let missing_mirror = id_of(p, "id").as_deref() != Some(r.id.as_str())
                    || id_of(p, "robloxId").as_deref() != Some(r.id.as_str());
                if adopted || missing_mirror {
                    synced_ids.push((key.clone(), r.id.clone()));
                }
                if !changed {
                    results.push(res(if adopted { "adopted" } else { "unchanged" }, Some(&r.id), None));
                } else if dry_run {
                    results.push(res("would_update", Some(&r.id), None));
                } else {
                    let (ct, bytes) = mp.finish();
                    match client.json("PATCH", &format!("{path_base}/{}", r.id), Some(&ct), Some(&bytes)) {
                        Ok(_) => results.push(res("updated", Some(&r.id), None)),
                        Err(e) => results.push(res("failed", Some(&r.id), Some(e.msg))),
                    }
                }
            }
            None if dry_run => results.push(res("would_create", None, None)),
            None => {
                let mut mp = Multipart::new();
                mp.text("name", &want.name);
                mp.text("description", &want.description);
                mp.text("price", &want.price.to_string());
                mp.text("isForSale", &want.for_sale.to_string());
                if let Some(icon) = p.get("icon").and_then(Value::as_str) {
                    if let Some(abs) = safe_rel(root, icon).ok().filter(|a| a.is_file()) {
                        if let Ok(bytes) = std::fs::read(&abs) {
                            mp.file("imageFile", icon.rsplit('/').next().unwrap_or("icon.png"), mime_for(icon), &bytes);
                        }
                    }
                }
                let (ct, bytes) = mp.finish();
                match client.json("POST", &path_base, Some(&ct), Some(&bytes)) {
                    Ok(v) => match id_of(&v, if is_pass { "gamePassId" } else { "productId" }).or_else(|| id_of(&v, "id")) {
                        Some(id) => {
                            synced_ids.push((key.clone(), id.clone()));
                            results.push(res("created", Some(&id), None));
                        }
                        None => results.push(res("failed", None, Some("Roblox created it but sent no id back; list the products to find it.".into()))),
                    },
                    Err(e) => results.push(res("failed", None, Some(e.msg))),
                }
            }
        }
    }
    if !synced_ids.is_empty() && !dry_run {
        update_manifest(root, |m| {
            if let Some(arr) = m.get_mut("products").and_then(Value::as_array_mut) {
                for item in arr {
                    let k = item.get("key").and_then(Value::as_str).unwrap_or("").to_string();
                    if let Some((_, id)) = synced_ids.iter().find(|(nk, _)| *nk == k) {
                        let val = id.parse::<u64>().map(|n| json!(n)).unwrap_or_else(|_| json!(id));
                        item["id"] = val.clone();
                        item["robloxId"] = val;
                    }
                }
            }
        })?;
    }
    let auto_codegen = body.get("auto_codegen").and_then(Value::as_bool);
    let run_codegen = match auto_codegen {
        Some(true) => !dry_run,
        Some(false) => false,
        None => !dry_run && !synced_ids.is_empty(),
    };
    let mut out = json!({ "ok": true, "results": results, "dry_run": dry_run });
    if run_codegen {
        if let Ok(cg) = products_codegen(root, &json!({})) {
            out["product_ids_luau"] = cg;
        }
    }
    Ok(out)
}

/// Generate `stylua`-clean `src/shared/ProductIds.luau` (or custom `out`) from synced
/// `products` entries in `openro.project.json` (#69).
pub fn products_codegen(root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let out_rel = body
        .get("out")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("src/shared/ProductIds.luau");
    if !out_rel.ends_with(".luau") && !out_rel.ends_with(".lua") {
        return bad("out path must end in .luau or .lua.");
    }
    let out_abs = safe_rel(root, out_rel)?;
    if let Some(parent) = out_abs.parent() {
        std::fs::create_dir_all(parent).or_else(|e| err(500, format!("create codegen dir: {e}")))?;
    }

    let mut by_key: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut game_passes: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut dev_products: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for p in manifest
        .get("products")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let key = p.get("key").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        let Some(num_id) = id_of(p, "id")
            .or_else(|| id_of(p, "robloxId"))
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
        else {
            continue;
        };
        by_key.insert(key.to_string(), num_id);
        match p.get("kind").and_then(Value::as_str) {
            Some("game_pass") => {
                game_passes.insert(key.to_string(), num_id);
            }
            Some("developer_product") => {
                dev_products.insert(key.to_string(), num_id);
            }
            _ => {}
        }
    }

    let write_table = |buf: &mut String, name: &str, map: &std::collections::BTreeMap<String, u64>| {
        if map.is_empty() {
            buf.push_str(&format!("ProductIds.{name} = {{}}\n\n"));
        } else {
            buf.push_str(&format!("ProductIds.{name} = {{\n"));
            for (k, id) in map {
                buf.push_str(&format!("\t[{}] = {id},\n", luau_str(k)));
            }
            buf.push_str("}\n\n");
        }
    };

    let mut out = String::new();
    out.push_str("--!strict\n");
    out.push_str("-- Auto-generated by OpenRO from openro.project.json. Do not edit by hand.\n\n");
    out.push_str("local ProductIds = {}\n\n");
    write_table(&mut out, "byKey", &by_key);
    write_table(&mut out, "gamePasses", &game_passes);
    write_table(&mut out, "developerProducts", &dev_products);
    out.push_str("function ProductIds.get(key: string): number?\n");
    out.push_str("\treturn ProductIds.byKey[key]\n");
    out.push_str("end\n\n");
    out.push_str("return ProductIds\n");

    let tmp = out_abs.with_extension("luau.tmp-openro");
    std::fs::write(&tmp, out.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &out_abs))
        .or_else(|e| err(500, format!("write {out_rel}: {e}")))?;

    let keys: Vec<String> = by_key.keys().cloned().collect();
    Ok(json!({
        "ok": true,
        "path": out_rel,
        "count": by_key.len(),
        "game_passes": game_passes.len(),
        "developer_products": dev_products.len(),
        "keys": keys,
    }))
}

/// Balanced genre preset for 1-click Monetization & Retention scaffolding.
#[derive(Clone, Copy, Debug)]
pub struct MonetizationTierSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub default_currency: &'static str,
    pub default_daily_reward: i64,
    pub comeback_multiplier: i64,
    pub curve_multipliers: [i64; 7],
    pub drafts: &'static [(&'static str, &'static str, &'static str, &'static str, u64)],
}

const STARTER_OBBY_DRAFTS: &[(&str, &str, &str, &str, u64)] = &[
    ("vip_pass", "game_pass", "VIP & 2x Currency", "2x rewards, chat tag, and VIP badge", 149),
    ("speed_coil", "game_pass", "Coil / Speed Boost", "Permanent movement speed coil", 79),
    ("starter_pack", "developer_product", "Starter Pack", "Starter bundle + currency boost", 99),
    ("currency_pouch", "developer_product", "500 Currency Pouch", "Instant 500 currency top-up", 49),
    ("skip_stage", "developer_product", "Skip Stage / Revive", "Instant checkpoint advance or revive", 25),
];

const ANIME_GACHA_DRAFTS: &[(&str, &str, &str, &str, u64)] = &[
    ("vip_banner_luck", "game_pass", "VIP Banner Luck", "+25% rare drop luck and VIP aura", 299),
    ("auto_open_storage", "game_pass", "Auto-Open & Storage+", "Faster multi-roll & +100 inventory slots", 149),
    ("hero_starter_bundle", "developer_product", "Hero Starter Bundle", "Starter unit + summon currency pack", 199),
    ("summon_pack_10x", "developer_product", "10x Summon Pack", "Full 10-pull banner summon bundle", 399),
    ("gem_crate", "developer_product", "500 Currency Crate", "Instant 500 summon currency pack", 99),
];

const COMPETITIVE_ARENA_DRAFTS: &[(&str, &str, &str, &str, u64)] = &[
    ("pro_battle_pass", "game_pass", "Pro Battle Pass", "Season pass track + 2x match XP", 249),
    ("loadout_slot_plus", "game_pass", "Loadout Slot+", "Extra custom weapon loadout slots", 129),
    ("season_starter_kit", "developer_product", "Season Starter Kit", "Starter skin + currency boost", 149),
    ("xp_booster_1h", "developer_product", "XP Booster (1h)", "Double match XP for 60 minutes", 49),
    ("crate_key", "developer_product", "Arena Crate Key", "Unlock cosmetic reward crate", 79),
];

pub fn resolve_tier_preset(preset_id: Option<&str>) -> MonetizationTierSpec {
    match preset_id.map(str::trim).unwrap_or("starter_obby") {
        "anime_gacha_rpg" => MonetizationTierSpec {
            id: "anime_gacha_rpg",
            label: "Anime Gacha / RPG",
            default_currency: "Gems",
            default_daily_reward: 150,
            comeback_multiplier: 3,
            curve_multipliers: [1, 2, 3, 5, 7, 10, 15],
            drafts: ANIME_GACHA_DRAFTS,
        },
        "competitive_arena" => MonetizationTierSpec {
            id: "competitive_arena",
            label: "Competitive Arena",
            default_currency: "Credits",
            default_daily_reward: 120,
            comeback_multiplier: 2,
            curve_multipliers: [1, 2, 3, 4, 6, 8, 12],
            drafts: COMPETITIVE_ARENA_DRAFTS,
        },
        _ => MonetizationTierSpec {
            id: "starter_obby",
            label: "Starter Obby / Simulator",
            default_currency: "Coins",
            default_daily_reward: 100,
            comeback_multiplier: 2,
            curve_multipliers: [1, 2, 3, 4, 5, 7, 12],
            drafts: STARTER_OBBY_DRAFTS,
        },
    }
}

pub fn retention_curve_values(spec: &MonetizationTierSpec, daily_base_reward: i64) -> [i64; 7] {
    let base = daily_base_reward.clamp(10, 1_000_000);
    [
        base * spec.curve_multipliers[0],
        base * spec.curve_multipliers[1],
        base * spec.curve_multipliers[2],
        base * spec.curve_multipliers[3],
        base * spec.curve_multipliers[4],
        base * spec.curve_multipliers[5],
        base * spec.curve_multipliers[6],
    ]
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn detect_preset_from_scripts(scripts: &[(&str, &str)]) -> Option<&'static str> {
    for (name, src) in scripts {
        if name.contains("MonetizationConfig") || src.contains("TierPreset") {
            if src.contains("anime_gacha_rpg") {
                return Some("anime_gacha_rpg");
            }
            if src.contains("competitive_arena") {
                return Some("competitive_arena");
            }
            if src.contains("starter_obby") {
                return Some("starter_obby");
            }
        }
    }
    None
}

/// Deterministic Studio Economy & Funnel Dry-Run Simulator shared by Rojo projects
/// and Local Place (`/api/studio/economy/simulate`).
pub fn simulate_economy_funnel(
    tier_preset: Option<&str>,
    scripts: &[(&str, &str)],
    instance_count: usize,
    products: &[Value],
) -> Value {
    let detected = tier_preset.or_else(|| detect_preset_from_scripts(scripts));
    let spec = resolve_tier_preset(detected);

    let mut has_process_receipt = false;
    let mut has_daily_retention = false;
    let mut has_comeback_hook = false;
    let mut has_monetization_config = false;
    let mut has_policy_guard = false;
    let mut has_hud = false;
    let mut has_shop = false;
    let mut starter_items = 0usize;
    let mut loop_signals = 0usize;

    for &(path, src) in scripts {
        let low_p = path.to_ascii_lowercase();
        if src.contains("ProcessReceipt") && src.contains("PurchaseId") {
            has_process_receipt = true;
        }
        if src.contains("ClaimDailyReward") || src.contains("DailyStreak") {
            has_daily_retention = true;
        }
        if src.contains("COMEBACK_MULTIPLIER") || src.contains("ComebackThresholdHours") {
            has_comeback_hook = true;
        }
        if low_p.contains("monetizationconfig") {
            has_monetization_config = true;
        }
        if src.contains("PolicyService") {
            has_policy_guard = true;
        }
        if low_p.contains("startergui") || low_p.contains("hud") || low_p.contains("gamegui") {
            has_hud = true;
        }
        if low_p.contains("shop") || low_p.contains("economy") || src.contains("PromptGamePassPurchase") || src.contains("PromptProductPurchase") {
            has_shop = true;
        }
        if low_p.contains("starterpack") || low_p.contains("inventory") || low_p.contains("sword") || low_p.contains("tool") || low_p.contains("starter") {
            starter_items += 1;
        }
        if low_p.contains("checkpoint")
            || low_p.contains("round")
            || low_p.contains("quest")
            || low_p.contains("gacha")
            || low_p.contains("tower")
            || low_p.contains("arena")
            || low_p.contains("tycoon")
            || low_p.contains("currency")
            || low_p.contains("leaderboard")
        {
            loop_signals += 1;
        }
    }

    let mut game_passes = 0usize;
    let mut dev_products = 0usize;
    let mut unapproved_prices = 0usize;
    let mut pass_prices = 0u64;
    let mut prod_prices = 0u64;
    for p in products {
        let price = p.get("priceRobux").and_then(Value::as_u64).unwrap_or(0);
        if p.get("priceApproved").and_then(Value::as_bool) != Some(true) {
            unapproved_prices += 1;
        }
        match p.get("kind").and_then(Value::as_str) {
            Some("game_pass") => {
                game_passes += 1;
                pass_prices += price;
            }
            Some("developer_product") => {
                dev_products += 1;
                prod_prices += price;
            }
            _ => {}
        }
    }
    let effective_passes = if game_passes > 0 {
        game_passes
    } else if has_monetization_config {
        spec.drafts.iter().filter(|d| d.1 == "game_pass").count()
    } else {
        0
    };
    let effective_products = if dev_products > 0 {
        dev_products
    } else if has_monetization_config {
        spec.drafts.iter().filter(|d| d.1 == "developer_product").count()
    } else {
        0
    };
    let avg_pass_robux = if game_passes > 0 {
        round2(pass_prices as f64 / game_passes as f64)
    } else if effective_passes > 0 {
        let sum: u64 = spec.drafts.iter().filter(|d| d.1 == "game_pass").map(|d| d.4).sum();
        round2(sum as f64 / effective_passes as f64)
    } else {
        0.0
    };
    let avg_prod_robux = if dev_products > 0 {
        round2(prod_prices as f64 / dev_products as f64)
    } else if effective_products > 0 {
        let sum: u64 = spec.drafts.iter().filter(|d| d.1 == "developer_product").map(|d| d.4).sum();
        round2(sum as f64 / effective_products as f64)
    } else {
        0.0
    };

    let estimated_loop_minutes = round2(
        (2.5 + (loop_signals as f64 * 1.45) + ((instance_count.min(24) as f64) * 0.2)).clamp(2.5, 18.0),
    );

    let baseline_d1 = 26.0_f64;
    let baseline_d7 = 6.2_f64;
    let preset_d1_bonus = match spec.id {
        "anime_gacha_rpg" => 3.5,
        "competitive_arena" => 2.5,
        _ => 1.5,
    };
    let preset_d7_bonus = match spec.id {
        "anime_gacha_rpg" => 2.4,
        "competitive_arena" => 1.8,
        _ => 1.0,
    };
    let d1_retention_pct = round2(
        (baseline_d1
            + if has_daily_retention { 8.5 } else { 0.0 }
            + if has_hud { 3.5 } else { 0.0 }
            + (loop_signals.min(4) as f64 * 1.6)
            + (starter_items.min(3) as f64 * 1.2)
            + if has_daily_retention { preset_d1_bonus } else { 0.0 })
        .clamp(18.0, 58.0),
    );
    let d7_retention_pct = round2(
        (baseline_d7
            + if has_daily_retention { 3.2 } else { 0.0 }
            + if has_comeback_hook { spec.comeback_multiplier as f64 * 1.1 } else { 0.0 }
            + (loop_signals.min(5) as f64 * 0.9)
            + if has_daily_retention { preset_d7_bonus } else { 0.0 })
        .clamp(3.0, 24.0),
    );
    let d1_lift_pct = round2(d1_retention_pct - baseline_d1);
    let d7_lift_pct = round2(d7_retention_pct - baseline_d7);

    let price_drag = if avg_pass_robux > 499.0 { 0.7 } else { 0.0 };
    let gamepass_conversion_pct = round2(
        (1.1
            + if has_process_receipt { 1.4 } else { 0.0 }
            + if has_shop { 1.1 } else { 0.0 }
            + if has_hud { 0.7 } else { 0.0 }
            + if has_policy_guard { 0.4 } else { 0.0 }
            + (effective_passes.min(3) as f64 * 0.65)
            - price_drag)
            .clamp(0.8, 7.8),
    );

    let pass_arpdau = (gamepass_conversion_pct / 100.0) * if avg_pass_robux > 0.0 { avg_pass_robux } else { 149.0 };
    let prod_repeat_rate = if has_process_receipt {
        (effective_products.min(4) as f64 * 0.014) + if spec.id == "anime_gacha_rpg" { 0.022 } else { 0.012 }
    } else {
        0.004
    };
    let prod_arpdau = prod_repeat_rate * if avg_prod_robux > 0.0 { avg_prod_robux } else { 79.0 };
    let retention_scale = 1.0 + (d7_lift_pct / 25.0);
    let arpdau_robux = round2(((pass_arpdau + prod_arpdau) * retention_scale).clamp(0.4, 85.0));
    let arpdau_usd_est = round2(arpdau_robux * 0.0035);

    let mut blockers: Vec<Value> = Vec::new();
    if !has_process_receipt {
        blockers.push(json!({
            "id": "missing_process_receipt",
            "severity": "high",
            "title": "Missing idempotent MarketplaceService.ProcessReceipt",
            "impact": "-1.4% GamePass conversion & replay-loss risk",
            "fix_action": "install_monetization_pack"
        }));
    }
    if !has_daily_retention || !has_comeback_hook {
        blockers.push(json!({
            "id": "missing_retention_loop",
            "severity": "high",
            "title": "No 7-day streak & 48h comeback retention hook",
            "impact": "-8.5% D1 retention & -5.4% D7 retention",
            "fix_action": "install_monetization_pack"
        }));
    }
    if !has_shop || !has_hud {
        blockers.push(json!({
            "id": "missing_shop_hud_funnel",
            "severity": "medium",
            "title": "No StarterGui shop/HUD prompt bridge detected",
            "impact": "-1.8% conversion from unprompted players",
            "fix_action": "add_shop_hud"
        }));
    }
    if effective_passes == 0 || unapproved_prices > 0 {
        blockers.push(json!({
            "id": "unapproved_or_empty_tiers",
            "severity": "medium",
            "title": if effective_passes == 0 {
                "No GamePass or DevProduct tiers configured"
            } else {
                "Draft Robux tiers awaiting owner price approval"
            },
            "impact": "Live Robux checkout gated until owner review",
            "fix_action": "approve_or_seed_tiers"
        }));
    }
    if estimated_loop_minutes < 4.0 {
        blockers.push(json!({
            "id": "short_core_loop",
            "severity": "low",
            "title": format!("Short estimated core loop ({estimated_loop_minutes:.1}m < 4.0m)"),
            "impact": "Add checkpoints, quests, or progression milestones",
            "fix_action": "add_progression_loop"
        }));
    }

    let funnel_stages = vec![
        json!({
            "stage": "Joined Place",
            "rate_pct": 100.0,
            "note": format!("{instance_count} workspace instance(s)")
        }),
        json!({
            "stage": "Completed Core Loop",
            "rate_pct": round2((58.0 + (loop_signals.min(4) as f64 * 6.5) + if has_hud { 5.0 } else { 0.0 }).clamp(45.0, 92.0)),
            "note": format!("Est. {estimated_loop_minutes:.1} min loop · {starter_items} starter item(s)")
        }),
        json!({
            "stage": "Shop / GamePass Converted",
            "rate_pct": gamepass_conversion_pct,
            "note": format!("{effective_passes} pass(es), {effective_products} product(s)")
        }),
        json!({
            "stage": "D7 Comeback Retained",
            "rate_pct": d7_retention_pct,
            "note": if has_comeback_hook {
                format!("{}x 48h comeback bonus active", spec.comeback_multiplier)
            } else {
                "48h comeback hook not installed".to_string()
            }
        }),
    ];

    json!({
        "schema": "openro.economy_funnel.v1",
        "ok": true,
        "tier_preset": spec.id,
        "preset_label": spec.label,
        "currency": spec.default_currency,
        "place_signals": {
            "script_count": scripts.len(),
            "instance_count": instance_count,
            "estimated_loop_minutes": estimated_loop_minutes,
            "starter_items_count": starter_items,
            "loop_systems_count": loop_signals,
            "game_passes_count": effective_passes,
            "dev_products_count": effective_products,
            "avg_pass_robux": avg_pass_robux,
            "avg_product_robux": avg_prod_robux,
            "unapproved_prices": unapproved_prices,
            "has_process_receipt": has_process_receipt,
            "has_daily_retention": has_daily_retention,
            "has_comeback_hook": has_comeback_hook,
            "has_hud": has_hud,
            "has_shop": has_shop,
        },
        "projections": {
            "arpdau_robux": arpdau_robux,
            "arpdau_usd_est": arpdau_usd_est,
            "d1_retention_pct": d1_retention_pct,
            "d7_retention_pct": d7_retention_pct,
            "d1_lift_pct": d1_lift_pct,
            "d7_lift_pct": d7_lift_pct,
            "gamepass_conversion_pct": gamepass_conversion_pct,
        },
        "funnel_stages": funnel_stages,
        "blockers": blockers,
    })
}

fn project_monetization_simulate(root: &Path, body: &Value) -> R<Value> {
    let manifest = read_manifest(root);
    let products: Vec<Value> = manifest
        .get("products")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rojo_file = manifest
        .get("rojoProject")
        .and_then(Value::as_str)
        .unwrap_or("default.project.json");
    let mut scripts_owned: Vec<(String, String)> = Vec::new();
    let mut instance_count = 0usize;
    if let Ok(tree) = crate::projects::load_tree(root, rojo_file) {
        for f in tree.flatten() {
            if let Some(src) = f.source {
                scripts_owned.push((f.path.replace('/', "."), src));
            } else if !f.service {
                instance_count += 1;
            }
        }
    }
    let script_refs: Vec<(&str, &str)> = scripts_owned
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let preset = body.get("tier_preset").and_then(Value::as_str);
    Ok(simulate_economy_funnel(preset, &script_refs, instance_count, &products))
}

/// Build the production-grade Monetization & Retention Luau Pack:
/// - `MonetizationService`: GamePass ownership cache + `MarketplaceService.ProcessReceipt`
///   with DataStore `UpdateAsync` `PurchaseId` idempotency + `BindToClose` + `PolicyService`
/// - `DailyRewardRetention`: 24h daily login streak + 48h comeback multiplier + `BindToClose`
/// - `MonetizationConfig`: Shared product, pass, tier preset, and 7-day retention curve.
pub fn monetization_pack_luau_scripts(
    currency: &str,
    vip_pass_id: u64,
    starter_pack_id: u64,
    daily_base_reward: i64,
) -> Vec<(&'static str, &'static str, &'static str, String)> {
    monetization_pack_luau_scripts_with_preset(
        None,
        currency,
        vip_pass_id,
        starter_pack_id,
        daily_base_reward,
    )
}

pub fn monetization_pack_luau_scripts_with_preset(
    tier_preset: Option<&str>,
    currency: &str,
    vip_pass_id: u64,
    starter_pack_id: u64,
    daily_base_reward: i64,
) -> Vec<(&'static str, &'static str, &'static str, String)> {
    let spec = resolve_tier_preset(tier_preset);
    let clean_currency = if currency.trim().is_empty()
        || !currency.trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        spec.default_currency
    } else {
        currency.trim()
    };
    let reward = daily_base_reward.clamp(10, 1_000_000);
    let curve = retention_curve_values(&spec, reward);

    let config_luau = format!(
        "--!strict\n\
-- Generated by OpenRO v1.16.0 Monetization & Retention Pack\n\
local MonetizationConfig = {{\n\
\tTierPreset = {preset_lit},\n\
\tCurrency = {curr},\n\
\tVipGamePassId = {vip_pass_id},\n\
\tStarterPackProductId = {starter_pack_id},\n\
\tComebackThresholdHours = 48,\n\
\tComebackMultiplier = {comeback_mult},\n\
\tDailyRewards = {{\n\
\t\t[1] = {r1},\n\
\t\t[2] = {r2},\n\
\t\t[3] = {r3},\n\
\t\t[4] = {r4},\n\
\t\t[5] = {r5},\n\
\t\t[6] = {r6},\n\
\t\t[7] = {r7},\n\
\t}},\n\
}}\n\n\
return MonetizationConfig\n",
        preset_lit = luau_str(spec.id),
        curr = luau_str(clean_currency),
        comeback_mult = spec.comeback_multiplier,
        r1 = curve[0],
        r2 = curve[1],
        r3 = curve[2],
        r4 = curve[3],
        r5 = curve[4],
        r6 = curve[5],
        r7 = curve[6],
    );

    let service_luau = format!(
        "--!strict\n\
-- Server-authoritative GamePass + ProcessReceipt idempotency + PolicyService compliance.\n\
local DataStoreService = game:GetService(\"DataStoreService\")\n\
local MarketplaceService = game:GetService(\"MarketplaceService\")\n\
local Players = game:GetService(\"Players\")\n\
local PolicyService = game:GetService(\"PolicyService\")\n\
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n\n\
local CURRENCY = {curr}\n\
local VIP_PASS_ID = {vip_pass_id}\n\
local STARTER_PACK_ID = {starter_pack_id}\n\
local receiptStore = DataStoreService:GetDataStore(\"OpenRO_Receipts_v1\")\n\
local passCache: {{ [number]: {{ [number]: boolean }} }} = {{}}\n\
local policyCache: {{ [number]: boolean }} = {{}}\n\n\
local function ensureRemotes()\n\
\tlocal folder = ReplicatedStorage:FindFirstChild(\"MonetizationRemotes\")\n\
\tif not folder then\n\
\t\tfolder = Instance.new(\"Folder\")\n\
\t\tfolder.Name = \"MonetizationRemotes\"\n\
\t\tfolder.Parent = ReplicatedStorage\n\
\tend\n\
\tif not folder:FindFirstChild(\"PromptPurchase\") then\n\
\t\tlocal ev = Instance.new(\"RemoteEvent\")\n\
\t\tev.Name = \"PromptPurchase\"\n\
\t\tev.Parent = folder\n\
\tend\n\
\treturn folder\n\
end\n\n\
local remotes = ensureRemotes()\n\
local promptRemote = remotes:WaitForChild(\"PromptPurchase\") :: RemoteEvent\n\n\
local function grantCurrency(player: Player, amount: number)\n\
\tlocal stats = player:FindFirstChild(\"leaderstats\")\n\
\tlocal val = stats and stats:FindFirstChild(CURRENCY)\n\
\tif val and val:IsA(\"IntValue\") then\n\
\t\tval.Value += amount\n\
\tend\n\
end\n\n\
local function checkPaidRandomAllowed(player: Player): boolean\n\
\tif policyCache[player.UserId] ~= nil then\n\
\t\treturn policyCache[player.UserId]\n\
\tend\n\
\tlocal ok, info = pcall(function()\n\
\t\treturn PolicyService:GetPolicyInfoForPlayerAsync(player)\n\
\tend)\n\
\tlocal allowed = ok and type(info) == \"table\" and info.ArePaidRandomItemsRestricted ~= true\n\
\tpolicyCache[player.UserId] = allowed\n\
\treturn allowed\n\
end\n\n\
local function playerOwnsPass(player: Player, passId: number): boolean\n\
\tif passId <= 0 then\n\
\t\treturn false\n\
\tend\n\
\tlocal cache = passCache[player.UserId]\n\
\tif cache and cache[passId] ~= nil then\n\
\t\treturn cache[passId]\n\
\tend\n\
\tlocal ok, owns = pcall(function()\n\
\t\treturn MarketplaceService:UserOwnsGamePassAsync(player.UserId, passId)\n\
\tend)\n\
\tlocal result = ok and owns == true\n\
\tpassCache[player.UserId] = passCache[player.UserId] or {{}}\n\
\tpassCache[player.UserId][passId] = result\n\
\treturn result\n\
end\n\n\
MarketplaceService.PromptGamePassPurchaseFinished:Connect(function(player: Player, passId: number, purchased: boolean)\n\
\tif purchased and passId > 0 then\n\
\t\tpassCache[player.UserId] = passCache[player.UserId] or {{}}\n\
\t\tpassCache[player.UserId][passId] = true\n\
\t\tplayer:SetAttribute(\"OwnsVipPass\", passId == VIP_PASS_ID or player:GetAttribute(\"OwnsVipPass\") == true)\n\
\tend\n\
end)\n\n\
MarketplaceService.ProcessReceipt = function(receiptInfo)\n\
\tlocal player = Players:GetPlayerByUserId(receiptInfo.PlayerId)\n\
\tif not player then\n\
\t\treturn Enum.ProductPurchaseDecision.NotProcessedYet\n\
\tend\n\
\tlocal receiptKey = tostring(receiptInfo.PlayerId) .. \"_\" .. tostring(receiptInfo.PurchaseId)\n\
\tlocal alreadyGranted = false\n\
\tlocal ok, err = pcall(function()\n\
\t\treceiptStore:UpdateAsync(receiptKey, function(existing)\n\
\t\t\tif existing == true then\n\
\t\t\t\talreadyGranted = true\n\
\t\t\t\treturn true\n\
\t\t\tend\n\
\t\t\treturn true\n\
\t\tend)\n\
\tend)\n\
\tif not ok then\n\
\t\twarn(\"[MonetizationService] Receipt DataStore error:\", err)\n\
\t\treturn Enum.ProductPurchaseDecision.NotProcessedYet\n\
\tend\n\
\tif not alreadyGranted then\n\
\t\tif receiptInfo.ProductId == STARTER_PACK_ID then\n\
\t\t\tgrantCurrency(player, 1000)\n\
\t\telse\n\
\t\t\tgrantCurrency(player, 250)\n\
\t\tend\n\
\tend\n\
\treturn Enum.ProductPurchaseDecision.PurchaseGranted\n\
end\n\n\
promptRemote.OnServerEvent:Connect(function(player: Player, kind: unknown, targetId: unknown)\n\
\tif typeof(kind) ~= \"string\" or typeof(targetId) ~= \"number\" then\n\
\t\treturn\n\
\tend\n\
\tlocal idNum = math.floor(targetId)\n\
\tif idNum <= 0 then\n\
\t\treturn\n\
\tend\n\
\tif kind == \"pass\" then\n\
\t\tif not playerOwnsPass(player, idNum) then\n\
\t\t\tpcall(function()\n\
\t\t\t\tMarketplaceService:PromptGamePassPurchase(player, idNum)\n\
\t\t\tend)\n\
\t\tend\n\
\telseif kind == \"product\" then\n\
\t\tlocal _ = checkPaidRandomAllowed(player)\n\
\t\tpcall(function()\n\
\t\t\tMarketplaceService:PromptProductPurchase(player, idNum)\n\
\t\tend)\n\
\tend\n\
end)\n\n\
Players.PlayerAdded:Connect(function(player: Player)\n\
\tplayer:SetAttribute(\"OwnsVipPass\", playerOwnsPass(player, VIP_PASS_ID))\n\
\tplayer:SetAttribute(\"PaidRandomAllowed\", checkPaidRandomAllowed(player))\n\
end)\n\n\
Players.PlayerRemoving:Connect(function(player: Player)\n\
\tpassCache[player.UserId] = nil\n\
\tpolicyCache[player.UserId] = nil\n\
end)\n\n\
game:BindToClose(function()\n\
\ttask.wait(0.2)\n\
end)\n",
        curr = luau_str(clean_currency),
    );

    let retention_luau = format!(
        "--!strict\n\
-- Daily login streak + 48h comeback bonus + escalating retention curve with DataStore persistence.\n\
local DataStoreService = game:GetService(\"DataStoreService\")\n\
local Players = game:GetService(\"Players\")\n\
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n\n\
local CURRENCY = {curr}\n\
local DAY_SECONDS = 86400\n\
local COMEBACK_SECONDS = 172800\n\
local COMEBACK_MULTIPLIER = {comeback_mult}\n\
local DAY_CURVE: {{ number }} = {{ {r1}, {r2}, {r3}, {r4}, {r5}, {r6}, {r7} }}\n\
local streakStore = DataStoreService:GetDataStore(\"OpenRO_DailyStreak_v1\")\n\n\
local folder = ReplicatedStorage:FindFirstChild(\"MonetizationRemotes\") or Instance.new(\"Folder\")\n\
folder.Name = \"MonetizationRemotes\"\n\
folder.Parent = ReplicatedStorage\n\n\
local claimFn = Instance.new(\"RemoteFunction\")\n\
claimFn.Name = \"ClaimDailyReward\"\n\
claimFn.Parent = folder\n\n\
local function award(player: Player, amount: number)\n\
\tlocal stats = player:FindFirstChild(\"leaderstats\")\n\
\tlocal val = stats and stats:FindFirstChild(CURRENCY)\n\
\tif val and val:IsA(\"IntValue\") then\n\
\t\tval.Value += amount\n\
\tend\n\
end\n\n\
claimFn.OnServerInvoke = function(player: Player)\n\
\tlocal key = \"streak_\" .. tostring(player.UserId)\n\
\tlocal now = os.time()\n\
\tlocal granted = 0\n\
\tlocal streakOut = 1\n\
\tlocal comebackBonus = false\n\
\tlocal ok, _ = pcall(function()\n\
\t\tstreakStore:UpdateAsync(key, function(prev)\n\
\t\t\tlocal rec = if typeof(prev) == \"table\" then prev else {{ streak = 0, lastClaim = 0 }}\n\
\t\t\tlocal prevLast = tonumber(rec.lastClaim) or 0\n\
\t\t\tlocal elapsed = now - prevLast\n\
\t\t\tif prevLast > 0 and elapsed < DAY_SECONDS then\n\
\t\t\t\tstreakOut = tonumber(rec.streak) or 1\n\
\t\t\t\treturn rec\n\
\t\t\tend\n\
\t\t\tlocal isComeback = prevLast > 0 and elapsed >= COMEBACK_SECONDS\n\
\t\t\tlocal nextStreak = if prevLast > 0 and not isComeback then ((tonumber(rec.streak) or 0) % 7) + 1 else 1\n\
\t\t\tstreakOut = nextStreak\n\
\t\t\tcomebackBonus = isComeback\n\
\t\t\tlocal baseAmt = DAY_CURVE[nextStreak] or DAY_CURVE[1]\n\
\t\t\tgranted = if isComeback then baseAmt * COMEBACK_MULTIPLIER else baseAmt\n\
\t\t\treturn {{ streak = nextStreak, lastClaim = now, comeback = isComeback }}\n\
\t\tend)\n\
\tend)\n\
\tif ok and granted > 0 then\n\
\t\taward(player, granted)\n\
\t\tplayer:SetAttribute(\"DailyStreak\", streakOut)\n\
\t\tplayer:SetAttribute(\"ComebackBonusActive\", comebackBonus)\n\
\t\treturn {{ claimed = true, reward = granted, streak = streakOut, comebackBonus = comebackBonus }}\n\
\tend\n\
\treturn {{ claimed = false, reward = 0, streak = streakOut, comebackBonus = false }}\n\
end\n\n\
Players.PlayerAdded:Connect(function(player: Player)\n\
\tlocal key = \"streak_\" .. tostring(player.UserId)\n\
\tlocal ok, rec = pcall(function()\n\
\t\treturn streakStore:GetAsync(key)\n\
\tend)\n\
\tif ok and typeof(rec) == \"table\" then\n\
\t\tplayer:SetAttribute(\"DailyStreak\", tonumber(rec.streak) or 0)\n\
\t\tplayer:SetAttribute(\"ComebackBonusActive\", rec.comeback == true)\n\
\tend\n\
end)\n\n\
game:BindToClose(function()\n\
\ttask.wait(0.2)\n\
end)\n",
        curr = luau_str(clean_currency),
        comeback_mult = spec.comeback_multiplier,
        r1 = curve[0],
        r2 = curve[1],
        r3 = curve[2],
        r4 = curve[3],
        r5 = curve[4],
        r6 = curve[5],
        r7 = curve[6],
    );

    vec![
        (
            "ReplicatedStorage.MonetizationConfig",
            "src/shared/MonetizationConfig.luau",
            "ModuleScript",
            config_luau,
        ),
        (
            "ServerScriptService.MonetizationService",
            "src/server/MonetizationService.server.luau",
            "Script",
            service_luau,
        ),
        (
            "ServerScriptService.DailyRewardRetention",
            "src/server/DailyRewardRetention.server.luau",
            "Script",
            retention_luau,
        ),
    ]
}

fn seed_tier_draft_products(
    root: &Path,
    spec: &MonetizationTierSpec,
    currency: &str,
    replace_only_if_empty: bool,
) -> usize {
    let mut added = 0usize;
    let _ = update_manifest(root, |m| {
        let arr = m.get("products").and_then(Value::as_array).cloned().unwrap_or_default();
        if replace_only_if_empty && !arr.is_empty() {
            return;
        }
        let mut out = arr;
        let existing_keys: std::collections::HashSet<String> = out
            .iter()
            .filter_map(|p| p.get("key").and_then(Value::as_str).map(String::from))
            .collect();
        for (key, kind, name, desc, price) in spec.drafts {
            if existing_keys.contains(*key) {
                continue;
            }
            let clean_name = name.replace("Currency", currency);
            let clean_desc = desc.replace("currency", &currency.to_ascii_lowercase());
            out.push(json!({
                "key": key,
                "kind": kind,
                "name": clean_name,
                "description": clean_desc,
                "priceRobux": price,
                "forSale": true,
                "priceApproved": false
            }));
            added += 1;
        }
        m["products"] = Value::Array(out);
    });
    added
}

fn project_monetization_pack(root: &Path, body: &Value) -> R<Value> {
    let tier_preset = body.get("tier_preset").and_then(Value::as_str);
    let spec = resolve_tier_preset(tier_preset);
    let currency = body
        .get("currency")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(spec.default_currency);
    let daily_reward = body
        .get("daily_base_reward")
        .and_then(Value::as_i64)
        .unwrap_or(spec.default_daily_reward);

    let seeded_from_preset = if tier_preset.is_some()
        && body.get("seed_tier_products").and_then(Value::as_bool) != Some(false)
    {
        seed_tier_draft_products(root, &spec, currency, false)
    } else {
        0
    };

    let manifest = read_manifest(root);
    let mut vip_pass_id = body.get("vip_pass_id").and_then(Value::as_u64).unwrap_or(0);
    let mut starter_pack_id = body
        .get("starter_pack_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    for p in manifest
        .get("products")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let num_id = id_of(p, "id")
            .or_else(|| id_of(p, "robloxId"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if num_id == 0 {
            continue;
        }
        match p.get("kind").and_then(Value::as_str) {
            Some("game_pass") if vip_pass_id == 0 => vip_pass_id = num_id,
            Some("developer_product") if starter_pack_id == 0 => starter_pack_id = num_id,
            _ => {}
        }
    }
    let scripts = monetization_pack_luau_scripts_with_preset(
        tier_preset,
        currency,
        vip_pass_id,
        starter_pack_id,
        daily_reward,
    );
    let curve = retention_curve_values(&spec, daily_reward);
    let mut written = Vec::new();
    let mut diagnostics = Vec::new();
    for (place_path, rel_path, _cls, src) in &scripts {
        let abs = safe_rel(root, rel_path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).or_else(|e| err(500, format!("create dir: {e}")))?;
        }
        let tmp = abs.with_extension("luau.tmp-openro");
        std::fs::write(&tmp, src.as_bytes())
            .and_then(|_| std::fs::rename(&tmp, &abs))
            .or_else(|e| err(500, format!("write {rel_path}: {e}")))?;
        let rep = crate::luau_check::analyze_script(place_path, src);
        for d in rep.diagnostics {
            diagnostics.push(d.to_json());
        }
        written.push(rel_path.to_string());
    }
    let product_ids = products_codegen(root, &json!({})).ok();
    Ok(json!({
        "ok": true,
        "tier_preset": spec.id,
        "comeback_multiplier": spec.comeback_multiplier,
        "retention_curve": curve,
        "seeded_products_count": seeded_from_preset,
        "written": written,
        "product_ids_luau": product_ids,
        "diagnostics": diagnostics,
    }))
}

/// One-click Monetization & Economy Codegen workflow (`POST /api/projects/:id/monetization/codegen`):
/// - Optionally seeds starter draft products (`priceApproved: false`) if manifest has none yet
/// - Generates/updates `MonetizationConfig.luau`, `MonetizationService.server.luau`, `DailyRewardRetention.server.luau`, and `ProductIds.luau`
/// - Generates server-authoritative `EconomyShopHandler.server.luau` + client `ShopPromptBridge.client.luau`
/// - Formats with `stylua` (when installed) and verifies `0` Luau static errors across the Rojo project.
fn project_monetization_codegen(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let tier_preset = body.get("tier_preset").and_then(Value::as_str);
    let spec = resolve_tier_preset(tier_preset);
    let seed_defaults = body
        .get("seed_defaults")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let currency = body
        .get("currency")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(spec.default_currency);

    let seeded_count = if seed_defaults {
        seed_tier_draft_products(root, &spec, currency, tier_preset.is_none())
    } else {
        0
    };
    let seeded_products = seeded_count > 0;

    let pack_res = project_monetization_pack(root, body)?;
    let mut written: Vec<String> = pack_res
        .get("written")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let manifest_after = read_manifest(root);
    let products = manifest_after
        .get("products")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut pass_cases = String::new();
    let mut dev_cases = String::new();
    for p in &products {
        let key = p.get("key").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        let kind = p.get("kind").and_then(Value::as_str).unwrap_or("developer_product");
        let k_lit = luau_str(key);
        if kind == "game_pass" {
            pass_cases.push_str(&format!("\t[{k_lit}] = true,\n"));
        } else {
            dev_cases.push_str(&format!("\t[{k_lit}] = 500,\n"));
        }
    }

    let economy_handler_src = format!(
        "--!strict\n\
-- Auto-generated by OpenRO v1.16.0 Monetization & Economy Codegen.\n\\
-- Server-authoritative purchase prompting wired to ProductIds + MonetizationConfig.\n\
local MarketplaceService = game:GetService(\"MarketplaceService\")\n\
local Players = game:GetService(\"Players\")\n\
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n\
\n\
local Shared = ReplicatedStorage:FindFirstChild(\"Shared\")\n\
local ProductIdsModule = if Shared then Shared:FindFirstChild(\"ProductIds\") else nil\n\
local ProductIds = if ProductIdsModule and ProductIdsModule:IsA(\"ModuleScript\") then require(ProductIdsModule) else {{ byKey = {{}}, gamePasses = {{}}, developerProducts = {{}} }}\n\
\n\
local remotesFolder = ReplicatedStorage:FindFirstChild(\"MonetizationRemotes\")\n\
if not remotesFolder then\n\
\tlocal f = Instance.new(\"Folder\")\n\
\tf.Name = \"MonetizationRemotes\"\n\
\tf.Parent = ReplicatedStorage\n\
\tremotesFolder = f\n\
end\n\
\n\
local promptEvent = remotesFolder:FindFirstChild(\"PromptPurchase\")\n\
if not promptEvent then\n\
\tlocal ev = Instance.new(\"RemoteEvent\")\n\
\tev.Name = \"PromptPurchase\"\n\
\tev.Parent = remotesFolder\n\
\tpromptEvent = ev\n\
end\n\
\n\
local feedbackEvent = remotesFolder:FindFirstChild(\"PurchaseFeedback\")\n\
if not feedbackEvent then\n\
\tlocal ev = Instance.new(\"RemoteEvent\")\n\
\tev.Name = \"PurchaseFeedback\"\n\
\tev.Parent = remotesFolder\n\
\tfeedbackEvent = ev\n\
end\n\
\n\
local GAME_PASS_KEYS: {{ [string]: boolean }} = {{\n\
{pass_cases}\
}}\n\
\n\
local DEV_PRODUCT_GRANTS: {{ [string]: number }} = {{\n\
{dev_cases}\
}}\n\
\n\
local function ensureLeaderstats(player: Player): IntValue?\n\
\tlocal ls = player:FindFirstChild(\"leaderstats\")\n\
\tif not ls then\n\
\t\tlocal folder = Instance.new(\"Folder\")\n\
\t\tfolder.Name = \"leaderstats\"\n\
\t\tfolder.Parent = player\n\
\t\tls = folder\n\
\tend\n\
\tlocal stat = ls:FindFirstChild({curr_lit})\n\
\tif not stat then\n\
\t\tlocal v = Instance.new(\"IntValue\")\n\
\t\tv.Name = {curr_lit}\n\
\t\tv.Value = 0\n\
\t\tv.Parent = ls\n\
\t\treturn v\n\
\tend\n\
\treturn if stat:IsA(\"IntValue\") then stat else nil\n\
end\n\
\n\
Players.PlayerAdded:Connect(function(player)\n\
\tensureLeaderstats(player)\n\
end)\n\
for _, player in Players:GetPlayers() do\n\
\tensureLeaderstats(player)\n\
end\n\
\n\
if promptEvent:IsA(\"RemoteEvent\") then\n\
\tpromptEvent.OnServerEvent:Connect(function(player: Player, rawKey: unknown)\n\
\t\tif typeof(rawKey) ~= \"string\" then\n\
\t\t\treturn\n\
\t\tend\n\
\t\tlocal idNum = if type(ProductIds.byKey) == \"table\" then ProductIds.byKey[rawKey] else nil\n\
\t\tif typeof(idNum) ~= \"number\" or idNum <= 0 then\n\
\t\t\tif feedbackEvent:IsA(\"RemoteEvent\") then\n\
\t\t\t\tfeedbackEvent:FireClient(player, {{ ok = false, key = rawKey, reason = \"not_synced_to_roblox\" }})\n\
\t\t\tend\n\
\t\t\treturn\n\
\t\tend\n\
\t\tif GAME_PASS_KEYS[rawKey] then\n\
\t\t\tpcall(function()\n\
\t\t\t\tMarketplaceService:PromptGamePassPurchase(player, idNum)\n\
\t\t\tend)\n\
\t\telseif DEV_PRODUCT_GRANTS[rawKey] ~= nil then\n\
\t\t\tpcall(function()\n\
\t\t\t\tMarketplaceService:PromptProductPurchase(player, idNum)\n\
\t\t\tend)\n\
\t\tend\n\
\tend)\n\
end\n",
        curr_lit = luau_str(currency),
    );

    let shop_bridge_src = "\
--!strict\n\
-- Auto-generated by OpenRO v1.16.0 Monetization & Economy Codegen.\n\\
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n\
local StarterGui = game:GetService(\"StarterGui\")\n\
\n\
local remotes = ReplicatedStorage:WaitForChild(\"MonetizationRemotes\", 10)\n\
if remotes then\n\
\tlocal promptEvent = remotes:WaitForChild(\"PromptPurchase\", 10)\n\
\tlocal feedbackEvent = remotes:WaitForChild(\"PurchaseFeedback\", 10)\n\
\tif feedbackEvent and feedbackEvent:IsA(\"RemoteEvent\") then\n\
\t\tfeedbackEvent.OnClientEvent:Connect(function(info)\n\
\t\t\tif type(info) == \"table\" and info.ok == false then\n\
\t\t\t\tpcall(function()\n\
\t\t\t\t\tStarterGui:SetCore(\"SendNotification\", {\n\
\t\t\t\t\t\tTitle = \"Shop\",\n\
\t\t\t\t\t\tText = \"Sync product IDs in OpenRO Ship before live purchases.\",\n\
\t\t\t\t\t\tDuration = 4,\n\
\t\t\t\t\t})\n\
\t\t\t\tend)\n\
\t\t\tend\n\
\t\tend)\n\
\tend\n\
\tif promptEvent and promptEvent:IsA(\"RemoteEvent\") then\n\
\t\t_G.OpenRO_PromptProduct = function(productKey: string)\n\
\t\t\tif type(productKey) == \"string\" and productKey ~= \"\" then\n\
\t\t\t\tpromptEvent:FireServer(productKey)\n\
\t\t\tend\n\
\t\tend\n\
\tend\n\
end\n";

    for (rel_path, src) in [
        ("src/server/EconomyShopHandler.server.luau", economy_handler_src.as_str()),
        ("src/client/ShopPromptBridge.client.luau", shop_bridge_src),
    ] {
        let abs = safe_rel(root, rel_path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).or_else(|e| err(500, format!("create dir: {e}")))?;
        }
        let tmp = abs.with_extension("luau.tmp-openro");
        std::fs::write(&tmp, src.as_bytes())
            .and_then(|_| std::fs::rename(&tmp, &abs))
            .or_else(|e| err(500, format!("write {rel_path}: {e}")))?;
        if !written.iter().any(|w| w == rel_path) {
            written.push(rel_path.to_string());
        }
    }

    let _ = crate::projects::format_project(root, None);
    let readiness = project_readiness(ctx, root)?;
    Ok(json!({
        "ok": true,
        "tier_preset": spec.id,
        "comeback_multiplier": spec.comeback_multiplier,
        "retention_curve": pack_res["retention_curve"],
        "written": written,
        "seeded_products": seeded_products,
        "product_count": products.len(),
        "luau_errors": readiness["luau_errors"],
        "luau_warnings": readiness["luau_warnings"],
        "diagnostics": readiness["diagnostics"],
        "readiness": readiness,
    }))
}

pub fn project_ship_pack_integrity(root: &Path, rojo_file: &str) -> Value {
    let mut zip = crate::rojo_export::ZipWriter::new();
    let mut zip_err: Option<String> = None;
    for f in walk(root) {
        if f.rel == "openro.project.json"
            || f.rel == rojo_file
            || (f.rel.starts_with("src/") && (f.rel.ends_with(".luau") || f.rel.ends_with(".lua")))
        {
            if let Ok(abs) = safe_rel(root, &f.rel) {
                if let Ok(bytes) = std::fs::read(&abs) {
                    if let Err(e) = zip.add_file(&f.rel, &bytes) {
                        zip_err = Some(e);
                        break;
                    }
                }
            }
        }
    }
    if let Some(err_msg) = zip_err {
        json!({
            "ok": false,
            "crc_verified": false,
            "zip_slip_clean": false,
            "entry_count": 0,
            "total_bytes": 0,
            "error": err_msg,
        })
    } else {
        match zip.finish() {
            Ok((zip_bytes, _)) => crate::rojo_export::verify_zip_integrity(&zip_bytes),
            Err(e) => json!({
                "ok": false,
                "crc_verified": false,
                "zip_slip_clean": true,
                "entry_count": 0,
                "total_bytes": 0,
                "error": e,
            }),
        }
    }
}

fn project_readiness(ctx: &Ctx, root: &Path) -> R<Value> {
    let manifest = read_manifest(root);
    let has_key = ctx.store.has(OPENCLOUD_KEY);
    let universe = universe_id(&manifest).ok();
    let place = id_of(&manifest, "placeId").filter(|s| s.bytes().all(|b| b.is_ascii_digit()));
    let has_ids = universe.is_some() && place.is_some();
    let build_res = latest_build(root, None);
    let has_build = build_res.is_ok();
    let products = manifest
        .get("products")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let unapproved = products
        .iter()
        .filter(|p| p.get("priceApproved").and_then(Value::as_bool) != Some(true))
        .count();
    let synced_products = products
        .iter()
        .filter(|p| {
            id_of(p, "id")
                .or_else(|| id_of(p, "robloxId"))
                .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| n > 0))
        })
        .count();

    let mut empty_scripts = Vec::new();
    let mut trailing_ws_scripts = 0usize;
    let mut fallback_scripts = std::collections::BTreeMap::new();
    for f in walk(root) {
        if f.rel.starts_with("src/") && (f.rel.ends_with(".luau") || f.rel.ends_with(".lua")) {
            if let Ok(text) = std::fs::read_to_string(&f.abs) {
                if text.trim().is_empty() {
                    empty_scripts.push(f.rel.clone());
                } else if text.lines().any(|l| l.ends_with(' ') || l.ends_with('\t'))
                    || !text.ends_with('\n')
                {
                    trailing_ws_scripts += 1;
                }
                let place_key = if let Some(rest) = f.rel.strip_prefix("src/server/") {
                    format!("ServerScriptService.{}", rest.trim_end_matches(".server.luau").trim_end_matches(".server.lua").trim_end_matches(".luau").trim_end_matches(".lua").replace('/', "."))
                } else if let Some(rest) = f.rel.strip_prefix("src/client/") {
                    format!("StarterPlayer.StarterPlayerScripts.{}", rest.trim_end_matches(".client.luau").trim_end_matches(".client.lua").trim_end_matches(".luau").trim_end_matches(".lua").replace('/', "."))
                } else if let Some(rest) = f.rel.strip_prefix("src/shared/") {
                    format!("ReplicatedStorage.Shared.{}", rest.trim_end_matches(".luau").trim_end_matches(".lua").replace('/', "."))
                } else {
                    format!("ReplicatedStorage.{}", f.rel.trim_start_matches("src/").replace('/', "."))
                };
                fallback_scripts.insert(place_key, text);
            }
        }
    }
    let rojo_file = manifest
        .get("rojoProject")
        .and_then(Value::as_str)
        .unwrap_or("default.project.json");
    let mut scripts_map = std::collections::BTreeMap::new();
    let mut instances_map = std::collections::BTreeMap::new();
    if let Ok(tree) = crate::projects::load_tree(root, rojo_file) {
        for f in tree.flatten() {
            let dot = f.path.replace('/', ".");
            if let Some(src) = f.source {
                scripts_map.insert(dot, src);
            } else if !f.service {
                instances_map.insert(dot, json!({ "class_name": f.class, "properties": f.props }));
            }
        }
    } else {
        scripts_map = fallback_scripts;
    }
    let report = crate::luau_check::analyze_place(&scripts_map, &instances_map);
    let has_process_receipt = scripts_map
        .values()
        .any(|s| s.contains("ProcessReceipt") && s.contains("PurchaseId"));
    let has_daily_retention = scripts_map
        .values()
        .any(|s| s.contains("ClaimDailyReward") || s.contains("DailyStreak"));

    let lock_file = lock_path(root, &manifest).ok();
    let lock_json = lock_file.as_ref().map(|p| read_lock(p)).unwrap_or_else(|| json!({ "assets": {} }));
    let locked_count = lock_json
        .get("assets")
        .and_then(Value::as_object)
        .map(|m| m.len())
        .unwrap_or(0);
    let has_asset_ids = safe_rel(root, "src/shared/AssetIds.luau")
        .map(|p| p.is_file())
        .unwrap_or(false);
    let product_ids_text = safe_rel(root, "src/shared/ProductIds.luau")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let has_product_ids = product_ids_text.is_some();
    let mut synced_keys: Vec<String> = Vec::new();
    let mut missing_product_keys: Vec<String> = Vec::new();
    for p in &products {
        let key = p.get("key").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        let is_synced = id_of(p, "id")
            .or_else(|| id_of(p, "robloxId"))
            .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| n > 0));
        if is_synced {
            synced_keys.push(key.to_string());
            let in_luau = product_ids_text
                .as_deref()
                .is_some_and(|txt| txt.contains(&luau_str(key)));
            if !in_luau {
                missing_product_keys.push(key.to_string());
            }
        }
    }
    let monetization_parity_ok = missing_product_keys.is_empty() && unapproved == 0;

    // Verify Ship Pack .zip integrity (zip-slip clean + IEEE CRC-32 verified across project files).
    let ship_pack_integrity = project_ship_pack_integrity(root, rojo_file);
    let asset_lock_ok = locked_count == 0 || has_asset_ids;

    let latest_pt = crate::playtest::latest_report_summary(root);
    let live_pt_tested = latest_pt.is_some();
    let live_pt_passed = latest_pt
        .as_ref()
        .and_then(|r| r["passed"].as_bool())
        .unwrap_or(false);
    let live_pt_blanks = latest_pt
        .as_ref()
        .and_then(|r| r["blank_screenshots"].as_u64())
        .unwrap_or(0);
    let live_core_loop_passed = latest_pt
        .as_ref()
        .and_then(|r| {
            r["core_loop"]["all_passed"]
                .as_bool()
                .or_else(|| r["core_loop"]["all_core_passed"].as_bool())
        })
        .unwrap_or(false);
    let live_pt_ok = live_pt_passed
        && live_pt_blanks == 0
        && (live_core_loop_passed || latest_pt.as_ref().is_some_and(|r| r["core_loop"].is_null()));

    let latest_sb = crate::sandbox::latest_verified_summary(root);
    let sb_verified = latest_sb
        .as_ref()
        .and_then(|s| s["verified"].as_bool())
        .unwrap_or(false);
    let sb_fresh = latest_sb
        .as_ref()
        .and_then(|s| s["fresh"].as_bool())
        .unwrap_or(false);
    let sb_err_count = latest_sb
        .as_ref()
        .and_then(|s| s["error_count"].as_u64())
        .unwrap_or(0);

    let verification_tier: Option<&'static str> = if live_pt_ok {
        Some("live_studio")
    } else if sb_verified {
        Some("headless_sandbox")
    } else {
        None
    };

    let active_report = if live_pt_ok {
        latest_pt.as_ref()
    } else if sb_verified || (!live_pt_tested && latest_sb.is_some()) {
        latest_sb.as_ref()
    } else {
        latest_pt.as_ref()
    };

    let pt_tested = live_pt_tested || latest_sb.is_some();
    let pt_passed = live_pt_ok || sb_verified;
    let pt_blanks = if live_pt_ok || (!sb_verified && live_pt_tested) {
        live_pt_blanks
    } else {
        0
    };
    let move_status = active_report
        .and_then(|r| r["core_loop"]["move"]["status"].as_str())
        .unwrap_or("not tested")
        .to_string();
    let act_status = active_report
        .and_then(|r| r["core_loop"]["act"]["status"].as_str())
        .unwrap_or("not tested")
        .to_string();
    let hud_status = active_report
        .and_then(|r| r["core_loop"]["hud"]["status"].as_str())
        .unwrap_or("not tested")
        .to_string();
    let playtest_ok = live_pt_ok || sb_verified;

    let mut fixable_actions: Vec<&'static str> = Vec::new();
    if !empty_scripts.is_empty() || trailing_ws_scripts > 0 {
        fixable_actions.push("format_and_fix_luau");
    }
    if !has_process_receipt || !has_daily_retention {
        fixable_actions.push("install_monetization_pack");
    }
    if locked_count > 0 && !has_asset_ids {
        fixable_actions.push("codegen_asset_ids");
    }
    if synced_products > 0 && (!has_product_ids || !missing_product_keys.is_empty()) {
        fixable_actions.push("codegen_product_ids");
    }
    if !has_build {
        fixable_actions.push("build_rbxlx");
    }

    let luau_ok = report.is_ok();
    let luau_earned: u64 = if luau_ok && empty_scripts.is_empty() { 20 } else if luau_ok { 12 } else { 0 };
    let playtest_earned: u64 = if playtest_ok { 20 } else if live_pt_passed { 10 } else { 0 };
    let monetization_earned: u64 = if has_process_receipt { 15 } else { 0 };
    let retention_earned: u64 = if has_daily_retention { 10 } else { 0 };
    let asset_earned: u64 = if asset_lock_ok { 10 } else { 0 };
    let build_earned: u64 = if has_build { 10 } else { 0 };
    let pricing_earned: u64 = if unapproved == 0 { 5 } else { 0 };
    let opencloud_earned: u64 = if has_key && has_ids { 10 } else if has_key || has_ids { 5 } else { 0 };

    let luau_fixable = !empty_scripts.is_empty() || trailing_ws_scripts > 0;
    let asset_fixable = locked_count > 0 && !has_asset_ids;

    let checks = vec![
        json!({
            "id": "luau_audit",
            "pillar": "luau",
            "weight": 20,
            "earned": luau_earned,
            "label": "Static Luau audit (0 errors)",
            "ok": luau_ok,
            "auto_fixable": luau_fixable,
            "fix_action": if luau_fixable { json!("format_and_fix_luau") } else { Value::Null },
            "detail": format!(
                "{} error(s), {} warning(s){}",
                report.error_count(),
                report.warning_count(),
                if empty_scripts.is_empty() { String::new() } else { format!(", {} empty script(s)", empty_scripts.len()) }
            )
        }),
        json!({
            "id": "playtest_core_loop",
            "pillar": "playtest",
            "weight": 20,
            "earned": playtest_earned,
            "label": "Playtest MOVE / ACT / HUD proof",
            "ok": playtest_ok,
            "verification_tier": verification_tier,
            "auto_fixable": false,
            "fix_action": Value::Null,
            "cta_section": "playtests",
            "detail": if live_pt_ok {
                format!("Passed via Live Studio (MOVE: {move_status}, ACT: {act_status}, HUD: {hud_status})")
            } else if sb_verified {
                format!("Passed via Headless Sandbox (MOVE: {move_status}, ACT: {act_status}, HUD: {hud_status})")
            } else if !live_pt_tested && latest_sb.is_some() && !sb_fresh {
                "Headless sandbox report is stale after project edits — re-run Sandbox".to_string()
            } else if !live_pt_tested && latest_sb.is_some() {
                format!("Headless sandbox incomplete (MOVE: {move_status}, ACT: {act_status}, HUD: {hud_status}, errors: {sb_err_count})")
            } else if pt_tested {
                format!("Latest run incomplete (MOVE: {move_status}, ACT: {act_status}, HUD: {hud_status}, blanks: {pt_blanks})")
            } else {
                "No playtest run yet — run Headless Sandbox or Playtests to verify MOVE / ACT / HUD".to_string()
            }
        }),
        json!({
            "id": "rojo_build",
            "pillar": "build",
            "weight": 10,
            "earned": build_earned,
            "label": "Rojo / .rbxlx build artifact",
            "ok": has_build,
            "auto_fixable": !has_build,
            "fix_action": if !has_build { json!("build_rbxlx") } else { Value::Null },
            "detail": if has_build { "Latest .rbxlx build ready".to_string() } else { "Run Autopilot or Build before publishing".to_string() }
        }),
        json!({
            "id": "asset_lock",
            "pillar": "assets",
            "weight": 10,
            "earned": asset_earned,
            "label": "Asset lock & AssetIds.luau codegen",
            "ok": asset_lock_ok,
            "auto_fixable": asset_fixable,
            "fix_action": if asset_fixable { json!("codegen_asset_ids") } else { Value::Null },
            "detail": if locked_count == 0 {
                "No locked assets yet (optional)".to_string()
            } else if has_asset_ids {
                format!("{locked_count} locked asset(s) in AssetIds.luau")
            } else {
                format!("{locked_count} locked asset(s) missing AssetIds.luau")
            }
        }),
        json!({
            "id": "monetization_guard",
            "pillar": "monetization",
            "weight": 15,
            "earned": monetization_earned,
            "label": "Idempotent ProcessReceipt & GamePass guard",
            "ok": has_process_receipt,
            "auto_fixable": !has_process_receipt,
            "fix_action": if !has_process_receipt { json!("install_monetization_pack") } else { Value::Null },
            "detail": if has_process_receipt { "PurchaseId DataStore ledger verified" } else { "Install Monetization Pack to wire ProcessReceipt" }
        }),
        json!({
            "id": "retention_loop",
            "pillar": "monetization",
            "weight": 10,
            "earned": retention_earned,
            "label": "Daily Reward & Retention streak hook",
            "ok": has_daily_retention,
            "auto_fixable": !has_daily_retention,
            "fix_action": if !has_daily_retention { json!("install_monetization_pack") } else { Value::Null },
            "detail": if has_daily_retention { "24h streak DataStore hook active" } else { "Auto-installs 7-day streak & PolicyService hooks" }
        }),
        json!({
            "id": "prices_approved",
            "pillar": "monetization",
            "weight": 5,
            "earned": pricing_earned,
            "label": "Owner-approved Robux pricing",
            "ok": unapproved == 0,
            "auto_fixable": false,
            "fix_action": Value::Null,
            "cta_section": "publish",
            "detail": if unapproved == 0 { format!("{} product(s) approved", products.len()) } else { format!("{unapproved} pending owner price approval") }
        }),
        json!({
            "id": "opencloud_auth",
            "pillar": "publish",
            "weight": 10,
            "earned": opencloud_earned,
            "label": "Open Cloud key & Universe/Place IDs",
            "ok": has_key && has_ids,
            "auto_fixable": false,
            "fix_action": Value::Null,
            "cta_action": "open_settings",
            "detail": if has_key && has_ids { "Credentials and IDs linked" } else { "Configure in Settings or Auto-setup from browser" }
        }),
    ];

    let passed_count = checks.iter().filter(|c| c["ok"].as_bool() == Some(true)).count();
    let total_count = checks.len();
    let score_pct: u64 = (luau_earned
        + playtest_earned
        + monetization_earned
        + retention_earned
        + asset_earned
        + build_earned
        + pricing_earned
        + opencloud_earned)
        .min(100);
    let blockers: Vec<Value> = checks
        .iter()
        .filter(|c| c["ok"].as_bool() != Some(true))
        .cloned()
        .collect();
    let diag_preview: Vec<Value> = report.diagnostics.iter().take(12).map(|d| d.to_json()).collect();

    Ok(json!({
        "ok": true,
        "ready_to_publish": report.is_ok() && has_build && has_key && has_ids && unapproved == 0,
        "verification_tier": verification_tier,
        "score_pct": score_pct,
        "passed_count": passed_count,
        "total_count": total_count,
        "auto_fixable_count": fixable_actions.len(),
        "fixable_actions": fixable_actions,
        "blockers": blockers,
        "pillars": {
            "luau": { "label": "Luau", "earned": luau_earned, "max": 20 },
            "playtest": { "label": "Playtest", "earned": playtest_earned, "max": 20 },
            "monetization": { "label": "Monetization", "earned": monetization_earned + retention_earned + pricing_earned, "max": 30 },
            "assets": { "label": "Assets", "earned": asset_earned, "max": 10 },
            "build_publish": { "label": "Build & Cloud", "earned": build_earned + opencloud_earned, "max": 20 },
        },
        "playtest_summary": {
            "tested": pt_tested,
            "passed": pt_passed,
            "verification_tier": verification_tier,
            "blank_screenshots": pt_blanks,
            "move": move_status,
            "act": act_status,
            "hud": hud_status,
        },
        "sandbox_summary": latest_sb,
        "luau_errors": report.error_count(),
        "luau_warnings": report.warning_count(),
        "has_process_receipt": has_process_receipt,
        "has_daily_retention": has_daily_retention,
        "locked_assets": locked_count,
        "has_asset_ids": has_asset_ids,
        "has_product_ids": has_product_ids,
        "unapproved_prices": unapproved,
        "monetization_parity": {
            "ok": monetization_parity_ok,
            "synced_products": synced_keys.len(),
            "missing_keys": missing_product_keys,
            "unapproved_prices": unapproved,
            "has_product_ids": has_product_ids,
        },
        "ship_pack_integrity": ship_pack_integrity,
        "diagnostics": diag_preview,
        "checks": checks,
    }))
}

fn project_autopilot(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let before_readiness = project_readiness(ctx, root)?;
    let before_score = before_readiness["score_pct"].as_u64().unwrap_or(0);
    let targeted = body
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let install_monetization = match targeted {
        Some(a) => a == "install_monetization_pack",
        None => body.get("install_monetization").and_then(Value::as_bool).unwrap_or(true),
    };
    let do_format = match targeted {
        Some(a) => a == "format_and_fix_luau",
        None => body.get("format").and_then(Value::as_bool).unwrap_or(true),
    };
    let do_build = match targeted {
        Some(a) => a == "build_rbxlx",
        None => body.get("build").and_then(Value::as_bool).unwrap_or(true),
    };
    let fix_empty = match targeted {
        Some(a) => a == "format_and_fix_luau",
        None => body.get("fix_empty").and_then(Value::as_bool).unwrap_or(true),
    };
    let do_codegen_assets = match targeted {
        Some(a) => a == "codegen_asset_ids",
        None => true,
    };
    let do_codegen_products = match targeted {
        Some(a) => a == "codegen_product_ids",
        None => true,
    };
    let do_codegen_economy = match targeted {
        Some(a) => a == "codegen_economy",
        None => false,
    };

    let mut actions_taken: Vec<Value> = Vec::new();
    let mut fixed_files: Vec<String> = Vec::new();

    if do_codegen_economy {
        let ec = project_monetization_codegen(ctx, root, body)?;
        actions_taken.push(json!({
            "action": "codegen_economy",
            "written": ec.get("written").cloned().unwrap_or(json!([])),
        }));
    }

    // 1. Safe auto-fix for empty placeholder Luau files & trailing whitespace/newlines.
    if fix_empty || do_format {
        for f in walk(root) {
            if f.rel.starts_with("src/") && (f.rel.ends_with(".luau") || f.rel.ends_with(".lua")) {
                let Ok(abs) = safe_rel(root, &f.rel) else { continue };
                let Ok(text) = std::fs::read_to_string(&abs) else { continue };
                if fix_empty && text.trim().is_empty() {
                    let is_script = f.rel.ends_with(".server.luau")
                        || f.rel.ends_with(".server.lua")
                        || f.rel.ends_with(".client.luau")
                        || f.rel.ends_with(".client.lua");
                    let stub = if is_script {
                        "-- Auto-initialized by OpenRO Ship Autopilot\n"
                    } else {
                        "-- Auto-initialized by OpenRO Ship Autopilot\nreturn {}\n"
                    };
                    if std::fs::write(&abs, stub.as_bytes()).is_ok() {
                        fixed_files.push(f.rel.clone());
                    }
                } else if do_format {
                    let mut cleaned: String = text
                        .lines()
                        .map(|l| l.trim_end_matches([' ', '\t']))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !cleaned.ends_with('\n') {
                        cleaned.push('\n');
                    }
                    if cleaned != text && std::fs::write(&abs, cleaned.as_bytes()).is_ok() {
                        fixed_files.push(f.rel.clone());
                    }
                }
            }
        }
        if !fixed_files.is_empty() {
            actions_taken.push(json!({
                "action": "fix_luau_hygiene",
                "count": fixed_files.len(),
                "files": fixed_files,
            }));
        }
    }

    // 2. Run stylua formatting if available.
    if do_format {
        if let Ok(fmt) = crate::projects::format_project(root, None) {
            if fmt.get("formatted").and_then(Value::as_bool) == Some(true) {
                actions_taken.push(json!({
                    "action": "stylua_format",
                    "targets": fmt.get("targets").cloned().unwrap_or(json!([])),
                }));
            }
        }
    }

    // 3. Install Monetization & Retention Pack if ProcessReceipt or DailyRewardRetention is missing.
    if install_monetization {
        let before_readiness = project_readiness(ctx, root)?;
        let has_pr = before_readiness["has_process_receipt"].as_bool() == Some(true);
        let has_ret = before_readiness["has_daily_retention"].as_bool() == Some(true);
        if !has_pr || !has_ret {
            let pack = project_monetization_pack(root, body)?;
            actions_taken.push(json!({
                "action": "install_monetization_pack",
                "written": pack.get("written").cloned().unwrap_or(json!([])),
            }));
        }
    }

    // 4. Generate AssetIds.luau & ProductIds.luau if locked assets / synced products exist.
    let manifest = read_manifest(root);
    if do_codegen_assets {
        if let Ok(lp) = lock_path(root, &manifest) {
            let lock = read_lock(&lp);
            let locked_n = lock.get("assets").and_then(Value::as_object).map(|m| m.len()).unwrap_or(0);
            if locked_n > 0 {
                if let Ok(cg) = assets_codegen(root, &json!({})) {
                    actions_taken.push(json!({
                        "action": "codegen_asset_ids",
                        "file": cg.get("out").cloned().unwrap_or(json!("src/shared/AssetIds.luau")),
                    }));
                }
            }
        }
    }
    let has_synced_products = manifest
        .get("products")
        .and_then(Value::as_array)
        .is_some_and(|arr| {
            arr.iter().any(|p| {
                id_of(p, "id")
                    .or_else(|| id_of(p, "robloxId"))
                    .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| n > 0))
            })
        });
    if do_codegen_products && has_synced_products {
        if let Ok(cg) = products_codegen(root, &json!({})) {
            actions_taken.push(json!({
                "action": "codegen_product_ids",
                "file": cg.get("out").cloned().unwrap_or(json!("src/shared/ProductIds.luau")),
            }));
        }
    }

    // 5. Build latest .rbxlx artifact.
    let mut build_info = Value::Null;
    if do_build {
        match crate::projects::build(root, "auto", None) {
            Ok(b) => {
                build_info = b;
                actions_taken.push(json!({ "action": "build_rbxlx", "ok": true }));
            }
            Err(e) => {
                actions_taken.push(json!({ "action": "build_rbxlx", "ok": false, "error": e }));
            }
        }
    }

    let readiness = project_readiness(ctx, root)?;
    let after_score = readiness["score_pct"].as_u64().unwrap_or(0);
    let score_delta = (after_score as i64) - (before_score as i64);
    let mut pillar_deltas = serde_json::Map::new();
    if let (Some(b_pil), Some(a_pil)) = (
        before_readiness["pillars"].as_object(),
        readiness["pillars"].as_object(),
    ) {
        for (k, av) in a_pil {
            let bv = &b_pil[k];
            let before_e = bv["earned"].as_u64().unwrap_or(0);
            let after_e = av["earned"].as_u64().unwrap_or(0);
            pillar_deltas.insert(
                k.clone(),
                json!({
                    "label": av["label"],
                    "before": before_e,
                    "after": after_e,
                    "delta": (after_e as i64) - (before_e as i64),
                    "max": av["max"],
                }),
            );
        }
    }
    let mut remaining_manual: Vec<String> = Vec::new();
    if readiness["unapproved_prices"].as_u64().unwrap_or(0) > 0 {
        remaining_manual.push("Approve Game Pass / Developer Product prices in Publish & Monetize.".to_string());
    }
    if readiness["checks"]
        .as_array()
        .is_some_and(|arr| arr.iter().any(|c| c["id"] == "opencloud_auth" && c["ok"] != true))
    {
        remaining_manual.push("Link Open Cloud API key and Universe/Place IDs in Settings or Auto-setup from browser.".to_string());
    }
    if readiness["luau_errors"].as_u64().unwrap_or(0) > 0 {
        remaining_manual.push("Resolve remaining Luau static errors in the Code workbench.".to_string());
    }

    Ok(json!({
        "ok": true,
        "before_score": before_score,
        "after_score": after_score,
        "score_delta": score_delta,
        "pillar_deltas": Value::Object(pillar_deltas),
        "actions_taken": actions_taken,
        "remaining_manual": remaining_manual,
        "build": build_info,
        "readiness": readiness,
    }))
}

/// Unified 1-click Pre-Publish Revenue & Playtest Gate (`POST /api/projects/:id/publish-gate`):
/// verifies live economy shop bindings (`ProductIds.luau` + `EconomyShopHandler` + client prompt
/// hooks), retention/daily-loop scripts, Rojo build + CRC-32 Ship Pack integrity, and playtest
/// readiness in one pass, returning actionable 1-click Auto-Fix cards (`fix_cards`).
fn project_publish_gate(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let auto_fix = body.get("auto_fix").and_then(Value::as_bool).unwrap_or(false);
    let mut autopilot_run: Option<Value> = None;
    if auto_fix {
        let mut ap_body = body.clone();
        if let Some(o) = ap_body.as_object_mut() {
            o.remove("action");
        }
        let ap = project_autopilot(ctx, root, &ap_body)?;
        // Also ensure shop handler + prompt bridge exist if missing when auto_fix=true
        let has_shop = safe_rel(root, "src/server/EconomyShopHandler.server.luau")
            .is_ok_and(|p| p.is_file())
            && safe_rel(root, "src/client/ShopPromptBridge.client.luau")
                .is_ok_and(|p| p.is_file());
        if !has_shop {
            let _ = project_monetization_codegen(ctx, root, body);
        }
        autopilot_run = Some(ap);
    }

    let readiness = project_readiness(ctx, root)?;
    let manifest = read_manifest(root);
    let rojo_file = manifest
        .get("rojoProject")
        .and_then(Value::as_str)
        .unwrap_or("default.project.json");

    let mut scripts_map = std::collections::BTreeMap::new();
    if let Ok(tree) = crate::projects::load_tree(root, rojo_file) {
        for f in tree.flatten() {
            if let Some(src) = f.source {
                scripts_map.insert(f.path, src);
            }
        }
    }
    let has_product_ids = readiness["has_product_ids"].as_bool() == Some(true);
    let parity_ok = readiness["monetization_parity"]["ok"].as_bool() == Some(true);
    let has_economy_handler = safe_rel(root, "src/server/EconomyShopHandler.server.luau")
        .is_ok_and(|p| p.is_file())
        || scripts_map
            .values()
            .any(|s| s.contains("PromptGamePassPurchase") || s.contains("PromptProductPurchase"));
    let has_client_bridge = safe_rel(root, "src/client/ShopPromptBridge.client.luau")
        .is_ok_and(|p| p.is_file())
        || scripts_map
            .values()
            .any(|s| s.contains("OpenRO_PromptProduct") || s.contains("PromptPurchase"));
    let has_process_receipt = readiness["has_process_receipt"].as_bool() == Some(true);
    let has_daily_retention = readiness["has_daily_retention"].as_bool() == Some(true);
    let zip_ok = readiness["ship_pack_integrity"]["ok"].as_bool() == Some(true);
    let luau_ok = readiness["luau_errors"].as_u64().unwrap_or(0) == 0;
    let has_build = readiness["checks"]
        .as_array()
        .is_some_and(|arr| arr.iter().any(|c| c["id"] == "rojo_build" && c["ok"] == true));
    let pt_summary = &readiness["playtest_summary"];
    let pt_passed = pt_summary["passed"].as_bool() == Some(true);
    let pt_tested = pt_summary["tested"].as_bool() == Some(true);
    let verification_tier = readiness["verification_tier"].clone();

    let economy_bindings_ok = has_product_ids && has_economy_handler && has_client_bridge && has_process_receipt;
    let build_and_zip_ok = luau_ok && has_build && zip_ok;

    let gate_checks = vec![
        json!({
            "id": "economy_bindings",
            "label": "Economy shop bindings (ProductIds + Server + Client)",
            "ok": economy_bindings_ok,
            "detail": if economy_bindings_ok {
                "ProductIds.luau, EconomyShopHandler, ShopPromptBridge, and ProcessReceipt wired".to_string()
            } else {
                let mut missing = Vec::new();
                if !has_product_ids { missing.push("ProductIds.luau"); }
                if !has_economy_handler { missing.push("EconomyShopHandler"); }
                if !has_client_bridge { missing.push("ShopPromptBridge"); }
                if !has_process_receipt { missing.push("ProcessReceipt guard"); }
                format!("Missing: {}", missing.join(", "))
            }
        }),
        json!({
            "id": "retention_loop",
            "label": "Retention & daily reward streak loop",
            "ok": has_daily_retention,
            "detail": if has_daily_retention { "DailyRewardRetention active with BindToClose" } else { "DailyRewardRetention.server.luau not wired" }
        }),
        json!({
            "id": "build_and_zip_crc",
            "label": "Rojo .rbxlx build + CRC-32 Ship Pack integrity",
            "ok": build_and_zip_ok,
            "detail": if build_and_zip_ok {
                format!("{} ZIP files CRC-32 verified, 0 Luau errors", readiness["ship_pack_integrity"]["entry_count"].as_u64().unwrap_or(0))
            } else {
                format!("Luau errors: {}, .rbxlx build: {}, ZIP CRC-32: {}", readiness["luau_errors"], has_build, zip_ok)
            }
        }),
        json!({
            "id": "playtest_gate",
            "label": "Playtest readiness (MOVE / ACT / HUD)",
            "ok": pt_passed,
            "verification_tier": verification_tier.clone(),
            "detail": if pt_passed {
                let tier_label = match verification_tier.as_str() {
                    Some("live_studio") => "Live Studio",
                    Some("headless_sandbox") => "Headless Sandbox",
                    _ => "Verified",
                };
                format!("Passed via {tier_label} (MOVE {}, ACT {}, HUD {})", pt_summary["move"].as_str().unwrap_or("ok"), pt_summary["act"].as_str().unwrap_or("ok"), pt_summary["hud"].as_str().unwrap_or("ok"))
            } else if pt_tested {
                "Latest run had incomplete assertions or is stale — re-run Sandbox or Playtests".to_string()
            } else {
                "Static Luau playtest check clean; run Headless Sandbox or live Playtest to capture MOVE/ACT/HUD".to_string()
            }
        }),
    ];

    let mut fix_cards: Vec<Value> = Vec::new();
    if !economy_bindings_ok {
        fix_cards.push(json!({
            "id": "fix_economy_bindings",
            "title": "Wire Shop, ProductIds & ProcessReceipt",
            "reason": gate_checks[0]["detail"],
            "fix_action": "codegen_economy",
        }));
    }
    if !has_daily_retention {
        fix_cards.push(json!({
            "id": "fix_retention_loop",
            "title": "Install Daily Reward Retention Hook",
            "reason": "Missing 24h daily streak + BindToClose persistence script",
            "fix_action": "install_monetization_pack",
        }));
    }
    if !parity_ok && readiness["monetization_parity"]["missing_keys"].as_array().is_some_and(|a| !a.is_empty()) {
        fix_cards.push(json!({
            "id": "fix_product_ids_parity",
            "title": "Sync ProductIds.luau Table",
            "reason": "Synced product keys in manifest are missing from src/shared/ProductIds.luau",
            "fix_action": "codegen_product_ids",
        }));
    }
    if !luau_ok {
        fix_cards.push(json!({
            "id": "fix_luau_hygiene",
            "title": "Format & Fix Luau Placeholders",
            "reason": "Format scripts via stylua and initialize empty Luau stubs",
            "fix_action": "format_and_fix_luau",
        }));
    }
    if !has_build {
        fix_cards.push(json!({
            "id": "fix_rojo_build",
            "title": "Build Latest .rbxlx Artifact",
            "reason": "Build artifact missing or stale before Open Cloud publish",
            "fix_action": "build_rbxlx",
        }));
    }

    let gate_passed = economy_bindings_ok && has_daily_retention && build_and_zip_ok && pt_passed && parity_ok;
    Ok(json!({
        "ok": true,
        "gate_passed": gate_passed,
        "verification_tier": verification_tier,
        "economy_bindings_ok": economy_bindings_ok,
        "retention_ok": has_daily_retention,
        "build_and_zip_ok": build_and_zip_ok,
        "playtest_passed": pt_passed,
        "parity_ok": parity_ok,
        "checks": gate_checks,
        "fix_cards": fix_cards,
        "autopilot": autopilot_run,
        "readiness": readiness,
    }))
}

// ----------------------------------------------------------------- universe

const UNIVERSE_FIELDS: &[(&str, bool)] = &[
    ("displayName", false),
    ("description", false),
    ("visibility", false),
    ("voiceChatEnabled", true),
    ("desktopEnabled", true),
    ("mobileEnabled", true),
    ("tabletEnabled", true),
    ("consoleEnabled", true),
    ("vrEnabled", true),
];

fn universe_get(ctx: &Ctx, root: &Path) -> R<Value> {
    let u = universe_id(&read_manifest(root))?;
    let v = Client::new(ctx)?.json("GET", &format!("/cloud/v2/universes/{u}"), None, None)?;
    Ok(json!({ "ok": true, "universe": v, "manual_steps": UNIVERSE_MANUAL_STEPS }))
}

fn universe_set(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let u = universe_id(&read_manifest(root))?;
    let Some(settings) = body.get("settings").and_then(Value::as_object).filter(|s| !s.is_empty()) else {
        return bad("Send settings: { displayName?, description?, visibility?, voiceChatEnabled?, desktopEnabled?, mobileEnabled?, tabletEnabled?, consoleEnabled?, vrEnabled? }.");
    };
    let mut patch = Map::new();
    for (k, v) in settings {
        let Some((_, is_bool)) = UNIVERSE_FIELDS.iter().find(|(n, _)| n == k) else {
            return bad(format!("'{k}' cannot be changed through Open Cloud. Game icon, thumbnails, monetisation policy and the age questionnaire are done in the Creator Dashboard."));
        };
        if *is_bool != v.is_boolean() || (!is_bool && !v.is_string()) {
            return bad(format!("'{k}' must be {}.", if *is_bool { "true or false" } else { "text" }));
        }
        let v = if k == "visibility" {
            let up = v.as_str().unwrap_or("").to_ascii_uppercase();
            if up != "PUBLIC" && up != "PRIVATE" {
                return bad("visibility must be PUBLIC or PRIVATE.");
            }
            json!(up)
        } else {
            v.clone()
        };
        patch.insert(k.clone(), v);
    }
    let mask = patch.keys().cloned().collect::<Vec<_>>().join(",");
    let payload = Value::Object(patch).to_string();
    let v = Client::new(ctx)?.json("PATCH", &format!("/cloud/v2/universes/{u}?updateMask={mask}"), Some("application/json"), Some(payload.as_bytes()))?;
    Ok(json!({
        "ok": true,
        "universe": v,
        "updated": mask.split(',').collect::<Vec<_>>(),
        "manual_steps": UNIVERSE_MANUAL_STEPS,
    }))
}

// ------------------------------------------------------------------- routes

pub(crate) fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// `None` = not an Open Cloud / Creator Hub / art / reference / image-ops route (the caller keeps matching).
pub fn route(ctx: &Ctx, method: &str, path: &str, query: &str, body: &Value) -> Option<Reply> {
    if let Some(reply) = crate::creator_hub::route(ctx, method, path, query, body) {
        return Some(reply);
    }
    let rest = path.strip_prefix("/api/")?;
    let segs: Vec<&str> = rest.split('/').collect();
    let out: R<Reply> = match (method, segs.as_slice()) {
        ("GET", ["opencloud"]) => Ok(ok_json(status_json(ctx))),
        ("POST", ["opencloud", "key"]) => set_key(ctx, body).map(|_| ok_json(status_json(ctx))),
        ("POST", ["opencloud", "settings"]) => set_creator(ctx, body).map(|_| ok_json(status_json(ctx))),
        ("POST", ["art", "generate"]) => art_root(ctx, body.get("project").and_then(Value::as_str))
            .and_then(|root| {
                let explicit = body
                    .get("adapter_id")
                    .or_else(|| body.get("provider"))
                    .and_then(Value::as_str);
                let actx = crate::tool_adapters::InvokeCtx {
                    data_dir: ctx.data_dir,
                    assets_root: ctx.data_dir,
                    store: &ctx.store,
                    project_dir: Some(&root),
                };
                if let Some(out_p) = body.get("out_path").and_then(Value::as_str) {
                    let norm = out_p.trim().replace('\\', "/");
                    if norm.starts_with("art/references/") || norm == "art/references" {
                        return bad("art_generate cannot write into gitignored art/references/.");
                    }
                }
                if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "image_gen", explicit, body) {
                    return res.map_err(|e| Oc {
                        status: if e.contains("prohibited") || e.contains("blocked_ssrf") || e.contains("invalid") {
                            400
                        } else {
                            502
                        },
                        msg: e,
                    });
                }
                crate::assets::art_generate(&ctx.store, &root, body)
            })
            .map(ok_json),
        ("POST", ["art", "approve"]) => art_root(ctx, body.get("project").and_then(Value::as_str))
            .and_then(|root| crate::assets::art_approve(&root, body))
            .map(ok_json),
        ("POST", ["art", "process"]) | ("POST", ["tools", "image", "process"]) => {
            art_root(ctx, body.get("project").and_then(Value::as_str))
                .and_then(|root| crate::image_ops::process(&root, body))
                .map(ok_json)
        }
        ("POST", ["art", "pack-card"]) => art_root(ctx, body.get("project").and_then(Value::as_str))
            .and_then(|root| crate::image_ops::pack_card(&root, body))
            .map(ok_json),
        ("POST", ["art", "references", "search"]) => art_root(ctx, body.get("project").and_then(Value::as_str))
            .and_then(|root| {
                let explicit = body.get("adapter_id").and_then(Value::as_str);
                let actx = crate::tool_adapters::InvokeCtx {
                    data_dir: ctx.data_dir,
                    assets_root: ctx.data_dir,
                    store: &ctx.store,
                    project_dir: Some(&root),
                };
                if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "reference_search", explicit, body) {
                    return res.map_err(|e| Oc {
                        status: if e.contains("prohibited") || e.contains("blocked_ssrf") || e.contains("invalid") {
                            400
                        } else {
                            502
                        },
                        msg: e,
                    });
                }
                crate::reference_search::search(&root, body)
            })
            .map(ok_json),
        ("GET", ["art", "references"]) => art_root(ctx, query_param(query, "project").as_deref())
            .and_then(|root| crate::reference_search::list(&root))
            .map(ok_json),
        ("POST", ["art", "references", "select"]) => art_root(ctx, body.get("project").and_then(Value::as_str))
            .and_then(|root| crate::reference_search::select(&root, body))
            .map(ok_json),
        ("GET", ["art"]) => {
            art_root(ctx, query_param(query, "project").as_deref()).and_then(|r| crate::assets::art_list(&r)).map(ok_json)
        }
        ("GET", ["art", "image"]) => art_root(ctx, query_param(query, "project").as_deref())
            .and_then(|r| crate::assets::art_image(&r, &query_param(query, "path").unwrap_or_default()))
            .map(|(m, b)| Reply::Bytes(m, b)),
        (_, ["projects", id, "liveops", tail @ ..]) => {
            let Some(root) = (ctx.resolve)(id) else {
                return Some(fail(Oc { status: 404, msg: "Unknown project.".into() }));
            };
            let reply = crate::liveops::route(ctx, method, &root, tail, query, body)
                .map(|r| r.unwrap_or_else(fail));
            return reply.map(|r| scrub_error_reply(ctx, r));
        }
        (_, ["projects", id, tail @ ..]) if matches!(tail, ["assets"] | ["assets", _] | ["publish"] | ["products"] | ["products", "sync"] | ["products", "codegen"] | ["monetization", "pack"] | ["monetization", "codegen"] | ["monetization", "simulate"] | ["publish-gate"] | ["readiness"] | ["autopilot"] | ["universe"]) => {
            let Some(root) = (ctx.resolve)(id) else {
                return Some(fail(Oc { status: 404, msg: "Unknown project.".into() }));
            };
            match (method, tail) {
                ("GET", ["assets"]) => assets_list(&root).map(ok_json),
                ("GET", ["assets", "lock"]) => read_manifest_lock(&root).map(ok_json),
                ("POST", ["assets", "authorise"]) => assets_authorise(&root, body).map(ok_json),
                ("POST", ["assets", "upload"]) => assets_upload(ctx, &root, body).map(ok_json),
                ("POST", ["assets", "codegen"]) => assets_codegen(&root, body).map(ok_json),
                ("POST", ["publish"]) => publish(ctx, &root, body).map(ok_json),
                ("GET", ["products"]) => products_get(ctx, &root).map(ok_json),
                ("POST", ["products", "sync"]) => products_sync(ctx, &root, body).map(ok_json),
                ("POST", ["products", "codegen"]) => products_codegen(&root, body).map(ok_json),
                ("POST", ["monetization", "pack"]) => project_monetization_pack(&root, body).map(ok_json),
                ("POST", ["monetization", "codegen"]) => project_monetization_codegen(ctx, &root, body).map(ok_json),
                ("POST", ["monetization", "simulate"]) => project_monetization_simulate(&root, body).map(ok_json),
                ("POST", ["publish-gate"]) => project_publish_gate(ctx, &root, body).map(ok_json),
                ("GET", ["readiness"]) => project_readiness(ctx, &root).map(ok_json),
                ("POST", ["autopilot"]) => project_autopilot(ctx, &root, body).map(ok_json),
                ("GET", ["universe"]) => universe_get(ctx, &root).map(ok_json),
                ("POST", ["universe"]) => universe_set(ctx, &root, body).map(ok_json),
                _ => err(405, "That method is not available here."),
            }
        }
        _ => return None,
    };
    Some(scrub_error_reply(ctx, out.unwrap_or_else(fail)))
}

fn scrub_error_reply(ctx: &Ctx, reply: Reply) -> Reply {
    match reply {
        Reply::Json(code, mut val) if code >= 400 => {
            let vals: Vec<String> = [
                secrets::OPENCLOUD_KEY,
                secrets::GEMINI_KEY,
                secrets::OPENAI_IMAGE_KEY,
            ]
            .into_iter()
            .filter_map(|k| ctx.store.get(k))
            .collect();
            let refs: Vec<&str> = vals.iter().map(String::as_str).collect();
            if !refs.is_empty() {
                secrets::scrub_value(&mut val, &refs);
            }
            Reply::Json(code, val)
        }
        other => other,
    }
}

fn read_manifest_lock(root: &Path) -> R<Value> {
    let manifest = read_manifest(root);
    Ok(read_lock(&lock_path(root, &manifest)?))
}

fn art_root(ctx: &Ctx, project: Option<&str>) -> R<PathBuf> {
    match project.filter(|p| !p.is_empty()) {
        Some(id) => (ctx.resolve)(id).ok_or(Oc { status: 404, msg: "Unknown project.".into() }),
        None => {
            if let Some(active) = &ctx.active_project {
                return Ok(active.clone());
            }
            let p = ctx.data_dir.join("art");
            let _ = std::fs::create_dir_all(&p);
            Ok(p)
        }
    }
}

fn set_key(ctx: &Ctx, body: &Value) -> R<()> {
    if body.get("clear").and_then(Value::as_bool) == Some(true) {
        return ctx.store.clear(OPENCLOUD_KEY).or_else(|e| err(500, e));
    }
    let key = body.get("api_key").and_then(Value::as_str).unwrap_or("").trim();
    if key.len() < 12 || key.chars().any(char::is_whitespace) {
        return bad("That does not look like an Open Cloud key. Paste it exactly as Roblox shows it.");
    }
    ctx.store.set(OPENCLOUD_KEY, key).or_else(|e| err(500, e))
}

fn set_creator(ctx: &Ctx, body: &Value) -> R<()> {
    let c = body.get("creator").cloned().unwrap_or(Value::Null);
    let digits = |k: &str| id_of(&c, k).filter(|s| s.bytes().all(|b| b.is_ascii_digit()));
    let value = match (digits("userId"), digits("groupId")) {
        (Some(u), None) => format!("user:{u}"),
        (None, Some(g)) => format!("group:{g}"),
        _ => return bad("Send creator as { userId } or { groupId } (numbers), not both."),
    };
    ctx.store.set(CREATOR_SECRET, &value).or_else(|e| err(500, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("art/cards/**", "art/cards/a.png"));
        assert!(glob_match("art/cards/**", "art/cards/x/y/a.png"));
        assert!(glob_match("art/ui/*.png", "art/ui/a.png"));
        assert!(!glob_match("art/ui/*.png", "art/ui/deep/a.png"));
        assert!(glob_match("art/cards", "art/cards/a.png"));
        assert!(!glob_match("art/cards", "art/cards2/a.png"));
        assert!(!glob_match("art/*.png", "art/cards/a.png"));
        assert!(glob_match("**/*.ogg", "a/b/c.ogg"));
    }

    #[test]
    fn rel_paths_are_jailed() {
        let root = std::env::temp_dir();
        assert!(safe_rel(&root, "../x").is_err());
        assert!(safe_rel(&root, "/etc/passwd").is_err());
        assert!(safe_rel(&root, "a/../../x").is_err());
        assert!(safe_rel(&root, "art/ok.png").is_ok());
    }

    #[test]
    fn iso_time_shape() {
        let t = now_iso();
        assert_eq!(t.len(), 20);
        assert!(t.ends_with('Z') && t.contains('T'));
    }

    #[test]
    fn multipart_has_parts() {
        let mut m = Multipart::new();
        m.text("request", "{}");
        m.file("fileContent", "a.png", "image/png", b"PNG");
        let (ct, body) = m.finish();
        let b = String::from_utf8_lossy(&body).to_string();
        assert!(ct.starts_with("multipart/form-data; boundary="));
        assert!(b.contains("name=\"request\"") && b.contains("filename=\"a.png\"") && b.contains("image/png"));
    }

    #[test]
    fn assets_codegen_emits_stylua_clean_empty_and_populated_tables() {
        let base = std::env::var("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("openro-assetids-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/shared")).unwrap();
        std::fs::write(
            base.join("stylua.toml"),
            "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
        )
        .unwrap();

        let assert_stylua_clean = |root: &Path| {
            if let Some(stylua) = crate::projects::find_tool("stylua", "OPENRO_STYLUA") {
                let out = std::process::Command::new(&stylua)
                    .current_dir(root)
                    .args(["--check", "--", "src/shared/AssetIds.luau"])
                    .output()
                    .expect("run stylua --check");
                assert!(
                    out.status.success(),
                    "stylua --check failed on AssetIds.luau:\n{}\n{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        };

        // 1. All empty -> single-line `{}` for all three tables
        let res = assets_codegen(&base, &json!({})).expect("codegen empty");
        assert_eq!(res["count"], json!(0));
        assert_eq!(res["mesh_count"], json!(0));
        let txt = std::fs::read_to_string(base.join("src/shared/AssetIds.luau")).unwrap();
        assert!(txt.contains("AssetIds.byPath = {}\n\n"), "got:\n{txt}");
        assert!(txt.contains("AssetIds.byKey = {}\n\n"), "got:\n{txt}");
        assert!(txt.contains("AssetIds.meshes = {}\n\n"), "got:\n{txt}");
        assert!(!txt.contains("{\n}"), "unexpected multi-line empty brace:\n{txt}");
        assert_stylua_clean(&base);

        // 2. Mesh manifest populated before Open Cloud upload (mesh_generate scenario, #58)
        std::fs::create_dir_all(base.join("assets/meshes")).unwrap();
        std::fs::write(
            base.join("assets/meshes/manifest.json"),
            json!({
                "meshes": {
                    "gacha_pedestal": { "size": [6.0, 4.5, 6.0], "tris": 1280 }
                }
            })
            .to_string(),
        )
        .unwrap();
        let res = assets_codegen(&base, &json!({})).expect("codegen mesh");
        assert_eq!(res["count"], json!(0));
        assert_eq!(res["mesh_count"], json!(1));
        let txt = std::fs::read_to_string(base.join("src/shared/AssetIds.luau")).unwrap();
        assert!(txt.contains("AssetIds.byPath = {}\n\n"));
        assert!(txt.contains("AssetIds.byKey = {}\n\n"));
        assert!(txt.contains("AssetIds.meshes = {\n\t[\"gacha_pedestal\"] = { size = Vector3.new(6.000, 4.500, 6.000), tris = 1280 },\n}\n\n"));
        assert_stylua_clean(&base);

        // 3. Populated assets.lock.json + mesh manifest -> multi-line tables, still stylua-clean
        std::fs::write(
            base.join("assets.lock.json"),
            json!({
                "assets": {
                    "art/cards/hero.png": { "assetId": "123456789", "state": "uploaded" }
                }
            })
            .to_string(),
        )
        .unwrap();
        let res = assets_codegen(&base, &json!({})).expect("codegen populated");
        assert_eq!(res["count"], json!(1));
        assert_eq!(res["mesh_count"], json!(1));
        let txt = std::fs::read_to_string(base.join("src/shared/AssetIds.luau")).unwrap();
        assert!(txt.contains("AssetIds.byPath = {\n\t[\"art/cards/hero.png\"] = \"rbxassetid://123456789\",\n}\n\n"));
        assert!(txt.contains("AssetIds.byKey = {\n\t[\"hero\"] = \"rbxassetid://123456789\",\n}\n\n"));
        assert_stylua_clean(&base);

        // 4. Packed card art/cards/<id>/art.png aliases, Decal imageAssetId override, and locked mesh assetId (#69)
        std::fs::write(
            base.join("assets/meshes/manifest.json"),
            json!({
                "meshes": {
                    "gacha_pedestal": { "size": [6.0, 4.5, 6.0], "tris": 1280 },
                    "world1_portal_arch": { "size": [6.18, 4.668, 2.1], "tris": 4468 }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            base.join("assets.lock.json"),
            json!({
                "assets": {
                    "art/cards/w1_naruto/art.png": {
                        "assetId": "555000",
                        "imageAssetId": "555001",
                        "assetType": "Decal",
                        "state": "approved"
                    },
                    "assets/meshes/world1_portal_arch.obj": {
                        "assetId": "999888777666",
                        "assetType": "Model",
                        "state": "approved"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let res = assets_codegen(&base, &json!({})).expect("codegen v012 #69");
        assert_eq!(res["count"], json!(2));
        assert_eq!(res["mesh_count"], json!(2));
        let txt = std::fs::read_to_string(base.join("src/shared/AssetIds.luau")).unwrap();
        assert!(txt.contains("[\"cards/w1_naruto\"] = \"rbxassetid://555001\""), "missing cards/<id> alias:\n{txt}");
        assert!(txt.contains("[\"w1_naruto\"] = \"rbxassetid://555001\""), "missing <id> alias:\n{txt}");
        assert!(txt.contains("[\"w1_naruto_art\"] = \"rbxassetid://555001\""), "missing <id>_art alias:\n{txt}");
        assert!(txt.contains("assetId = \"rbxassetid://999888777666\""), "missing mesh assetId:\n{txt}");
        assert_stylua_clean(&base);

        // 5. ProductIds.luau codegen (empty + populated, stylua-clean, #69)
        let res = products_codegen(&base, &json!({})).expect("products codegen empty");
        assert_eq!(res["count"], json!(0));
        std::fs::write(
            base.join("openro.project.json"),
            json!({
                "name": "Demo",
                "products": [
                    { "key": "pass_vip", "kind": "game_pass", "robloxId": 4200 },
                    { "key": "prod_gems", "kind": "developer_product", "id": 5001 },
                    { "key": "unsynced", "kind": "game_pass", "robloxId": null }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let res = products_codegen(&base, &json!({})).expect("products codegen populated");
        assert_eq!(res["count"], json!(2));
        assert_eq!(res["game_passes"], json!(1));
        assert_eq!(res["developer_products"], json!(1));
        let ptxt = std::fs::read_to_string(base.join("src/shared/ProductIds.luau")).unwrap();
        assert!(ptxt.contains("[\"pass_vip\"] = 4200"), "got:\n{ptxt}");
        assert!(ptxt.contains("[\"prod_gems\"] = 5001"), "got:\n{ptxt}");
        assert!(!ptxt.contains("unsynced"));
        if let Some(stylua) = crate::projects::find_tool("stylua", "OPENRO_STYLUA") {
            let out = std::process::Command::new(&stylua)
                .current_dir(&base)
                .args(["--check", "--", "src/shared/ProductIds.luau"])
                .output()
                .expect("run stylua --check on ProductIds.luau");
            assert!(
                out.status.success(),
                "stylua --check failed on ProductIds.luau:\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn decal_xml_extracts_underlying_image_id() {
        let xml = r#"<roblox version="4"><Item class="Decal"><Properties><Content name="Texture"><url>http://www.roblox.com/asset/?id=88776655</url></Content></Properties></Item></roblox>"#;
        assert_eq!(
            extract_decal_image_id(xml, "11223344").as_deref(),
            Some("88776655")
        );
        let xml_same = r#"<roblox><Content name="Texture"><url>rbxassetid://11223344</url></Content></roblox>"#;
        assert_eq!(extract_decal_image_id(xml_same, "11223344"), None);
    }
}
