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
const CREATOR_SECRET: &str = "opencloud_creator";
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
type R<T> = Result<T, Oc>;

fn bad<T>(msg: impl Into<String>) -> R<T> {
    Err(Oc { status: 400, msg: msg.into() })
}
fn err<T>(status: u16, msg: impl Into<String>) -> R<T> {
    Err(Oc { status, msg: msg.into() })
}
fn ok_json(v: Value) -> Reply {
    Reply::Json(200, v)
}
fn fail(e: Oc) -> Reply {
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
    std::env::var("OPENRO_OPENCLOUD_POLL_MS").ok().and_then(|s| s.trim().parse().ok())
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

struct Client {
    base: String,
    key: String,
    creator: Option<Creator>,
    http: ureq::Agent,
}

impl Client {
    fn new(ctx: &Ctx) -> R<Client> {
        let Some(key) = ctx.store.get(OPENCLOUD_KEY) else {
            return err(503, "Add your Open Cloud key in Settings first.");
        };
        let http = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(120))
            .redirects(0) // never forward the key to another host
            .build();
        Ok(Client { base: base_url(), key, creator: read_creator(&ctx.store), http })
    }

    /// One HTTP exchange. Any HTTP status comes back as `(status, body)`; only a
    /// transport failure is an `Err`. A 429 is retried a few times with a short wait.
    fn call(&self, method: &str, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> R<(u16, Vec<u8>)> {
        let url = format!("{}{}", self.base, path);
        for attempt in 0..4u32 {
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
            if status == 429 && attempt < 3 {
                let wait = resp.header("retry-after").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(1);
                let cap = poll_ms().map(|p| p * 2).unwrap_or(5_000);
                std::thread::sleep(Duration::from_millis((wait * 1000).min(cap)));
                continue;
            }
            let mut buf = Vec::new();
            let _ = std::io::Read::take(resp.into_reader(), 16 * 1024 * 1024).read_to_end(&mut buf);
            return Ok((status, buf));
        }
        err(502, "Roblox kept answering 'too many requests'. Wait a minute and try again.")
    }

    fn scrub(&self, s: &str) -> String {
        secrets::scrub(s, &[&self.key])
    }

    /// Call and require a 2xx JSON (or empty) answer.
    fn json(&self, method: &str, path: &str, content_type: Option<&str>, body: Option<&[u8]>) -> R<Value> {
        let (status, bytes) = self.call(method, path, content_type, body)?;
        self.check(status, &bytes)?;
        if bytes.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&bytes).or_else(|_| err(502, "Roblox answered with something that is not JSON."))
    }

    fn check(&self, status: u16, bytes: &[u8]) -> R<()> {
        if (200..300).contains(&status) {
            return Ok(());
        }
        err(502, self.upstream_msg(status, bytes))
    }

    fn upstream_msg(&self, status: u16, bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes);
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(String::from)
                    .or_else(|| v.pointer("/errors/0/message").and_then(|m| m.as_str()).map(String::from))
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        let head = match status {
            401 => "Roblox did not accept the Open Cloud key (401). Check the key and its scopes in Settings.".to_string(),
            403 => "Roblox says this key has no permission for that (403): add the scope or the experience to the key.".to_string(),
            429 => "Roblox rate limit reached (429). Try again shortly.".to_string(),
            s => format!("Roblox answered {s}."),
        };
        self.scrub(&if detail.trim().is_empty() { head } else { format!("{head} {detail}") })
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

fn read_manifest(root: &Path) -> Value {
    std::fs::read_to_string(root.join(MANIFEST))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn write_json_atomic(path: &Path, v: &Value) -> R<()> {
    let text = serde_json::to_string_pretty(v).map_err(|e| Oc { status: 500, msg: format!("write {}: {e}", path.display()) })?;
    let tmp = path.with_extension("tmp-openro");
    std::fs::write(&tmp, format!("{text}\n"))
        .and_then(|_| std::fs::rename(&tmp, path))
        .or_else(|e| err(500, format!("could not write {}: {e}", path.display())))
}

/// Merge `f` into a freshly read manifest (unknown fields stay) and save it.
fn update_manifest(root: &Path, f: impl FnOnce(&mut Map<String, Value>)) -> R<()> {
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
    // a symlink that leaves the project is refused
    let existing = if joined.exists() { joined.clone() } else { joined.parent().map(Path::to_path_buf).unwrap_or(root.to_path_buf()) };
    if let (Ok(r), Ok(e)) = (std::fs::canonicalize(root), std::fs::canonicalize(&existing)) {
        if !e.starts_with(&r) {
            return bad(format!("'{rel}' leaves the project folder."));
        }
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

fn id_of(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn universe_id(manifest: &Value) -> R<String> {
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
        "fbx" | "gltf" | "glb" | "rbxm" | "rbxmx" => "Model",
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
    state: String,
    operation: String,
    asset_type: String,
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

    fn upload_asset(&self, rel: &str, kind: &str, bytes: &[u8]) -> R<Uploaded> {
        let stem = rel.rsplit('/').next().unwrap_or(rel);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
        let name: String = stem.chars().take(50).collect();
        let filename = rel.rsplit('/').next().unwrap_or(rel);
        let mime = mime_for(rel);
        let mut used = kind.to_string();
        let (mut status, mut resp) = self.create_asset(kind, &name, filename, mime, bytes)?;
        if status == 400 && kind == "Image" {
            // Some accounts only accept images as Decals.
            used = "Decal".to_string();
            (status, resp) = self.create_asset("Decal", &name, filename, mime, bytes)?;
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
        let Some(asset_id) = id_of(done.get("response").unwrap_or(&Value::Null), "assetId") else {
            return err(502, format!("Roblox finished the upload of {rel} without an asset id."));
        };
        let state = norm_state(done.pointer("/response/moderationResult/moderationState").and_then(Value::as_str));
        Ok(Uploaded { asset_id, state, operation: op_id, asset_type: used })
    }

    fn poll_operation(&self, id: &str) -> R<Value> {
        let first = poll_ms().unwrap_or(500);
        let (max_delay, cap) = match poll_ms() {
            Some(p) => (p * 4, Duration::from_millis(p * 40)),
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
            delay = (delay * 2).min(max_delay);
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
                    lock_dirty = true;
                }
                results.push(json!({ "path": rel, "state": "reused", "assetId": t.get("assetId"), "assetType": kind, "sha256": sha }));
                continue;
            }
        }
        let Some(client) = &client else {
            results.push(json!({ "path": rel, "state": "would_upload", "assetType": kind, "sha256": sha }));
            continue;
        };
        match client.upload_asset(&rel, &kind, &data) {
            Ok(u) if u.state == "rejected" => {
                // Never keep a rejected asset id in assets.lock.json.
                if let Some(map) = lock.get_mut("assets").and_then(Value::as_object_mut) {
                    if map.remove(&rel).is_some() {
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
                lock["assets"][&rel] = json!({
                    "sha256": sha, "assetId": u.asset_id, "assetType": u.asset_type,
                    "state": u.state, "uploadedAt": now_iso(),
                });
                lock_dirty = true;
                eprintln!("opencloud: uploaded {rel} as asset {}", u.asset_id);
                results.push(json!({
                    "path": rel, "state": "uploaded", "assetId": u.asset_id, "assetType": u.asset_type,
                    "sha256": sha, "operation": u.operation, "moderation": u.state,
                }));
            }
            Err(e) => results.push(json!({ "path": rel, "state": "failed", "assetType": kind, "sha256": sha, "error": e.msg })),
        }
        // a stopped key or missing owner would fail every file the same way: say so once
        if let Some(last) = results.last() {
            if last["state"] == "failed" && matches!(last["error"].as_str(), Some(m) if m.contains("(401)") || m.contains("Set who owns")) {
                break;
            }
        }
    }
    if lock_dirty {
        lock["version"] = json!(1);
        write_json_atomic(&lock_file, &lock)?;
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
    if everything {
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
    let mut skipped_rejected = 0usize;
    if let Some(map) = lock.get("assets").and_then(Value::as_object) {
        for (rel, entry) in map {
            let state = entry.get("state").and_then(Value::as_str).unwrap_or("unknown");
            if state == "rejected" {
                skipped_rejected += 1;
                continue;
            }
            let Some(id) = id_of(entry, "assetId") else { continue };
            let uri = format!("rbxassetid://{id}");
            by_path.push((rel.clone(), uri.clone()));
            let key = short_asset_key(rel);
            by_key.entry(key).or_insert(uri);
        }
    }
    by_path.sort_by(|a, b| a.0.cmp(&b.0));

    // Optional mesh metadata from assets/meshes/manifest.json (stud bbox & tris for pre-upload proxies).
    let mesh_manifest = std::fs::read_to_string(root.join("assets/meshes/manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut meshes: Vec<(String, [f64; 3], u64)> = Vec::new();
    if let Some(entries) = mesh_manifest.as_ref().and_then(|v| v.get("meshes").and_then(Value::as_object).or_else(|| v.as_object())) {
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
                    let tris = v.get("tris").or_else(|| v.get("triangles")).and_then(Value::as_u64).unwrap_or(0);
                    meshes.push((k.clone(), sz, tris));
                }
            }
        }
        meshes.sort_by(|a, b| a.0.cmp(&b.0));
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
    if meshes.is_empty() {
        out.push_str("AssetIds.meshes = {}\n\n");
    } else {
        out.push_str("AssetIds.meshes = {\n");
        for (k, sz, tris) in &meshes {
            out.push_str(&format!(
                "\t[{}] = {{ size = Vector3.new({:.3}, {:.3}, {:.3}), tris = {} }},\n",
                luau_str(k),
                sz[0],
                sz[1],
                sz[2],
                tris
            ));
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
        "mesh_count": meshes.len(),
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
    let only = str_list(body.get("keys"));
    let products = manifest.get("products").and_then(Value::as_array).cloned().unwrap_or_default();
    let client = Client::new(ctx)?;
    let mut remote_dp: Option<Vec<Remote>> = None;
    let mut remote_gp: Option<Vec<Remote>> = None;
    let mut results = Vec::new();
    let mut new_ids: Vec<(String, String)> = Vec::new();

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
        let current_id = id_of(p, "id");
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
            for_sale: p.get("forSale").and_then(Value::as_bool).unwrap_or(false),
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
                if adopted {
                    new_ids.push((key.clone(), r.id.clone()));
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
                            new_ids.push((key.clone(), id.clone()));
                            results.push(res("created", Some(&id), None));
                        }
                        None => results.push(res("failed", None, Some("Roblox created it but sent no id back; list the products to find it.".into()))),
                    },
                    Err(e) => results.push(res("failed", None, Some(e.msg))),
                }
            }
        }
    }
    if !new_ids.is_empty() && !dry_run {
        update_manifest(root, |m| {
            if let Some(arr) = m.get_mut("products").and_then(Value::as_array_mut) {
                for item in arr {
                    let k = item.get("key").and_then(Value::as_str).unwrap_or("").to_string();
                    if let Some((_, id)) = new_ids.iter().find(|(nk, _)| *nk == k) {
                        item["id"] = id.parse::<u64>().map(|n| json!(n)).unwrap_or_else(|_| json!(id));
                    }
                }
            }
        })?;
    }
    Ok(json!({ "ok": true, "results": results, "dry_run": dry_run }))
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

fn query_param(query: &str, key: &str) -> Option<String> {
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

/// `None` = not an Open Cloud / art / reference / image-ops route (the caller keeps matching).
pub fn route(ctx: &Ctx, method: &str, path: &str, query: &str, body: &Value) -> Option<Reply> {
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
        (_, ["projects", id, tail @ ..]) if matches!(tail, ["assets"] | ["assets", _] | ["publish"] | ["products"] | ["products", "sync"] | ["universe"]) => {
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
                ("GET", ["universe"]) => universe_get(ctx, &root).map(ok_json),
                ("POST", ["universe"]) => universe_set(ctx, &root, body).map(ok_json),
                _ => err(405, "That method is not available here."),
            }
        }
        _ => return None,
    };
    Some(out.unwrap_or_else(fail))
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

        let _ = std::fs::remove_dir_all(&base);
    }
}
