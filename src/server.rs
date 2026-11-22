//! Hand-rolled minimal HTTP over `std::net::TcpListener` serving the
//! embedded web UI (plain HTML/CSS/JS string constants — no framework,
//! no build step) plus the JSON API the UI and tests use.
//!
//! Chats are multiple and persistent: when the server is built with a
//! data dir (`Shared::with_data_dir`), every chat and its thread lines
//! are saved to `chats.json` there on every change and reloaded at
//! start, so a restart loses nothing.

use crate::agent::Agent;
use crate::config::RuntimeConfig;
use crate::mcp::McpClient;
use crate::model::{probe_local_gateways, ModelClient, DEFAULT_BASE_URL, DEFAULT_MODEL_ID};
use crate::place::{LocalPlace, PlaceBackend};
use crate::snapshots::{compute_diff, parse_rbxlx_xml, SnapshotStore};
use crate::sse::BuildEventHub;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
}

static CHAT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_chat_id() -> String {
    let n = CHAT_COUNTER.fetch_add(1, Ordering::SeqCst);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("chat-{millis}-{n}")
}

fn default_chat() -> Chat {
    Chat {
        id: new_chat_id(),
        title: "New chat".to_string(),
        lines: Vec::new(),
    }
}

/// Per-chat creativity state: what the game already has (skills that
/// have generated for this chat) and which idea set was last pitched
/// into it, so a set is pitched once per context — never repeated.
#[derive(Default)]
pub struct ChatIdeas {
    pub built: Vec<String>,
    pub pitched: Vec<String>,
}

pub struct Shared {
    pub agent: Mutex<Option<Agent>>,
    pub chats: Mutex<Vec<Chat>>,
    pub active_chat: Mutex<String>,
    pub game_name: String,
    pub data_dir: Option<PathBuf>,
    pub ideas: Mutex<std::collections::HashMap<String, ChatIdeas>>,
    /// Backup file names of saved state that was corrupt at startup and
    /// was set aside (empty normally).
    pub store_recovered: Vec<String>,
    /// Connection handler threads running now (capped per server).
    pub active_handlers: AtomicUsize,
    /// The self-updater (checks GitHub only when asked, or from `main`).
    pub updater: Arc<crate::update::Updater>,
    /// Asks, idea builds and reverts in flight: a restart waits for them.
    pub busy: AtomicUsize,
    /// Chats whose change-set stack is non-empty (Undo has something to do).
    /// The agent keeps this current, so polling never waits on a running ask.
    pub undoable: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Persistent Model + Studio settings (`data_dir/config.json`).
    pub config: Mutex<RuntimeConfig>,
    /// Versioned place snapshots & build diffs (`data_dir/snapshots.json`).
    pub snapshots: Mutex<SnapshotStore>,
    /// Live SSE build-progress event hub (`/api/build/stream`).
    pub events: Arc<BuildEventHub>,
    /// Cached lightweight status so concurrent polling never stalls behind an active build.
    status_cache: Mutex<StatusInfo>,
    /// Game art assets metadata & in-memory blob fallback (`data_dir/game-assets/`).
    pub game_assets: Mutex<GameAssetsIndex>,
}

#[derive(Clone, Default)]
pub struct GameAssetsIndex {
    pub items: Vec<Value>,
    pub mem_blobs: std::collections::HashMap<String, Vec<u8>>,
}

impl Shared {
    /// In-memory server (tests, ephemeral runs): chats are not saved.
    pub fn new(agent: Option<Agent>, game_name: String) -> Arc<Shared> {
        let mut agent = agent;
        let updater = crate::update::Updater::from_env("OPENRO", "openro-rs", env!("CARGO_PKG_VERSION"), "cayleb-james2008/open-ro", true);
        let undoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let events = Arc::new(BuildEventHub::new());
        if let Some(a) = agent.as_mut() {
            a.attach_undo_sink(Arc::clone(&undoable));
            a.attach_events(Arc::clone(&events));
        }
        let initial_status = extract_status_from_agent(agent.as_mut());
        let chat = default_chat();
        let active = chat.id.clone();
        Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(vec![chat]),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: None,
            ideas: Mutex::new(std::collections::HashMap::new()),
            store_recovered: Vec::new(),
            active_handlers: AtomicUsize::new(0),
            undoable,
            updater,
            busy: AtomicUsize::new(0),
            config: Mutex::new(RuntimeConfig::default()),
            snapshots: Mutex::new(SnapshotStore::in_memory()),
            events,
            status_cache: Mutex::new(initial_status),
            game_assets: Mutex::new(GameAssetsIndex::default()),
        })
    }

    /// Persistent server: chats load from (and save to) `chats.json`
    /// inside `data_dir`.
    pub fn with_data_dir(agent: Option<Agent>, game_name: String, data_dir: PathBuf) -> Arc<Shared> {
        let _ = std::fs::create_dir_all(&data_dir);
        let updater = crate::update::Updater::from_env("OPENRO", "openro-rs", env!("CARGO_PKG_VERSION"), "cayleb-james2008/open-ro", true);
        let mut agent = agent;
        let undoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let events = Arc::new(BuildEventHub::new());
        if let Some(a) = agent.as_mut() {
            a.attach_undo_sink(Arc::clone(&undoable));
            a.attach_events(Arc::clone(&events));
        }
        let initial_status = extract_status_from_agent(agent.as_mut());
        let mut recovered = agent.as_ref().map(|a| a.recovered.clone()).unwrap_or_default();
        let (chats, active) = load_chats(&data_dir, &mut recovered).unwrap_or_else(|| {
            let chat = default_chat();
            let active = chat.id.clone();
            (vec![chat], active)
        });
        let ideas = load_ideas(&data_dir, &mut recovered);
        let cfg = RuntimeConfig::load(&data_dir);
        let mut snap_store = SnapshotStore::open(&data_dir);
        recovered.append(&mut snap_store.recovered);
        let loaded_assets = load_game_assets(&data_dir);
        Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(chats),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: Some(data_dir),
            ideas: Mutex::new(ideas),
            store_recovered: recovered,
            active_handlers: AtomicUsize::new(0),
            undoable,
            updater,
            busy: AtomicUsize::new(0),
            config: Mutex::new(cfg),
            snapshots: Mutex::new(snap_store),
            events,
            status_cache: Mutex::new(initial_status),
            game_assets: Mutex::new(loaded_assets),
        })
    }

    fn save(&self) {
        let Some(dir) = &self.data_dir else { return };
        let chats = self.chats.lock().unwrap();
        let active = self.active_chat.lock().unwrap();
        let v = json!({
            "chats": chats.iter().map(|c| json!({
                "id": c.id, "title": c.title, "lines": c.lines,
            })).collect::<Vec<_>>(),
            "active": *active,
        });
        let path = dir.join("chats.json");
        let tmp = dir.join("chats.json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// The active chat's thread lines.
    pub fn thread_lines(&self) -> Vec<String> {
        let chats = self.chats.lock().unwrap();
        let active = self.active_chat.lock().unwrap();
        chats
            .iter()
            .find(|c| c.id == *active)
            .map(|c| c.lines.clone())
            .unwrap_or_default()
    }

    /// Append lines to the active chat; the first "You: ..." line of a
    /// fresh chat becomes its title.
    pub fn append_lines(&self, lines: &[String]) {
        let active = self.active_chat.lock().unwrap().clone();
        self.append_lines_to(&active, lines);
    }

    /// Append lines to a SPECIFIC chat. An ask's outcome lines belong
    /// to the chat the ask ran in — appending to whichever chat is
    /// active when a long ask finishes files the build narration (and
    /// the failure) into the wrong chat while the change set sits in
    /// the right one.
    pub fn append_lines_to(&self, chat_id: &str, lines: &[String]) {
        {
            let mut chats = self.chats.lock().unwrap();
            if let Some(chat) = chats.iter_mut().find(|c| c.id == chat_id) {
                if chat.title == "New chat" {
                    if let Some(first) = lines.iter().find(|l| l.starts_with("You: ")) {
                        let ask = first.trim_start_matches("You: ");
                        chat.title = ask.chars().take(48).collect();
                    }
                }
                chat.lines.extend(lines.iter().cloned());
            }
        }
        self.save();
    }

    pub fn create_chat(&self) -> Chat {
        let chat = default_chat();
        {
            let mut chats = self.chats.lock().unwrap();
            chats.push(chat.clone());
            let mut active = self.active_chat.lock().unwrap();
            *active = chat.id.clone();
        }
        self.save();
        chat
    }

    pub fn select_chat(&self, id: &str) -> bool {
        let exists = self.chats.lock().unwrap().iter().any(|c| c.id == id);
        if exists {
            *self.active_chat.lock().unwrap() = id.to_string();
            self.save();
        }
        exists
    }

    pub fn chats_json(&self) -> Value {
        let chats = self.chats.lock().unwrap();
        let active = self.active_chat.lock().unwrap();
        json!({
            "chats": chats.iter().map(|c| json!({ "id": c.id, "title": c.title })).collect::<Vec<_>>(),
            "active": *active,
        })
    }

    /// A revert took back the skills of the set it undid: they are no
    /// longer in the place, so the prerequisite gate stops counting
    /// them. A skill another remaining set still holds stays counted.
    pub fn forget_built(&self, chat_id: &str, reverted: &[String], kept: &[String]) {
        let mut state = self.ideas.lock().unwrap();
        let Some(entry) = state.get_mut(chat_id) else { return };
        let before = entry.built.len();
        entry.built.retain(|s| !reverted.contains(s) || kept.contains(s));
        if entry.built.len() != before {
            self.persist_ideas(&state);
        }
    }

    /// Record skills a finished run used, so the creativity engine
    /// knows what this chat's game already has.
    pub fn note_built(&self, chat_id: &str, skills: &[String]) {
        let mut state = self.ideas.lock().unwrap();
        let entry = state.entry(chat_id.to_string()).or_default();
        let mut changed = false;
        for s in skills {
            if !entry.built.contains(s) {
                entry.built.push(s.clone());
                changed = true;
            }
        }
        if changed {
            self.persist_ideas(&state);
        }
    }

    /// Titles of the ideas this chat has already built (every skill built): the page says "Already in your game" for them.
    pub fn built_titles_for(&self, chat_id: &str) -> Vec<String> {
        let state = self.ideas.lock().unwrap();
        let built = state.get(chat_id).map(|e| e.built.clone()).unwrap_or_default();
        crate::ideas::catalogue()
            .into_iter()
            .filter(|i| !i.skills.is_empty() && i.skills.iter().all(|s| built.iter().any(|b| b == s)))
            .map(|i| i.title.to_string())
            .collect()
    }

    /// The ideas to pitch for a chat right now, and the thread lines
    /// for the pitch — non-empty only when this set is NEW for the
    /// chat (the context changed since the last pitch). A pitch is
    /// spoken once per context, never repeated into the record.
    pub fn pitch_for(&self, chat_id: &str, intro: &str) -> (Vec<crate::ideas::Idea>, Vec<String>) {
        let mut state = self.ideas.lock().unwrap();
        let entry = state.entry(chat_id.to_string()).or_default();
        let ideas = crate::ideas::ideas_for(&entry.built);
        let ids: Vec<String> = ideas.iter().map(|i| i.id.to_string()).collect();
        if ids == entry.pitched {
            return (ideas, Vec::new());
        }
        entry.pitched = ids;
        self.persist_ideas(&state);
        let lines = crate::ideas::pitch_lines(&ideas, intro);
        (ideas, lines)
    }

    /// Save the creativity state next to the chats (same directory,
    /// same tmp-then-rename discipline). Without this, a restart
    /// forgot what each game had built and re-pitched the opening
    /// games into chats that had long since built them.
    fn persist_ideas(&self, state: &std::collections::HashMap<String, ChatIdeas>) {
        let Some(dir) = &self.data_dir else { return };
        let v = json!({
            "chats": state.iter().map(|(id, s)| json!({
                "id": id, "built": s.built, "pitched": s.pitched,
            })).collect::<Vec<_>>(),
        });
        let path = dir.join("ideas.json");
        let tmp = dir.join("ideas.json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Load the persisted creativity state. Strict, like the chats and
/// changes loaders: any malformed entry makes the whole file corrupt
/// — backed up, never partially read (a half-remembered context
/// would pitch games the place already has, or skip ones it needs).
fn load_ideas(dir: &std::path::Path, recovered: &mut Vec<String>) -> std::collections::HashMap<String, ChatIdeas> {
    let path = dir.join("ideas.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return std::collections::HashMap::new(),
    };
    let strict = (|| -> Option<std::collections::HashMap<String, ChatIdeas>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut out = std::collections::HashMap::new();
        for item in v.get("chats")?.as_array()? {
            let id = item.get("id")?.as_str()?.to_string();
            let strings = |key: &str| -> Option<Vec<String>> {
                item.get(key)?
                    .as_array()?
                    .iter()
                    .map(|s| s.as_str().map(|x| x.to_string()))
                    .collect()
            };
            out.insert(
                id,
                ChatIdeas {
                    built: strings("built")?,
                    pitched: strings("pitched")?,
                },
            );
        }
        Some(out)
    })();
    match strict {
        Some(map) => map,
        None => {
            set_aside(&path, recovered);
            std::collections::HashMap::new()
        }
    }
}

fn ideas_json(ideas: &[crate::ideas::Idea]) -> Value {
    Value::Array(
        ideas
            .iter()
            .map(|i| json!({ "id": i.id, "title": i.title, "pitch": i.pitch, "skills": i.skills }))
            .collect(),
    )
}

/// Back up a corrupt state file and remember its backup name, so the
/// page can tell the operator what was set aside.
fn set_aside(path: &std::path::Path, recovered: &mut Vec<String>) {
    if let Some(bak) = backup_corrupt(path) {
        recovered.push(bak.file_name().unwrap_or_default().to_string_lossy().into_owned());
    }
}

/// Move a corrupt state file aside to the first free backup name so
/// its bytes survive for inspection instead of being overwritten by
/// the next save.
pub fn backup_corrupt(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut candidate = path.with_extension("json.bak");
    let mut n = 2;
    while candidate.exists() {
        candidate = path.with_extension(format!("json.bak{n}"));
        n += 1;
    }
    std::fs::rename(path, &candidate).ok()?;
    eprintln!(
        "openro: {} was corrupt; backed up to {} and starting fresh",
        path.display(),
        candidate.display()
    );
    Some(candidate)
}

fn load_chats(dir: &std::path::Path, recovered: &mut Vec<String>) -> Option<(Vec<Chat>, String)> {
    let path = dir.join("chats.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed = serde_json::from_str::<Value>(&text);
    let shape_ok = parsed
        .as_ref()
        .map(|v| v.get("chats").map(|c| c.is_array()).unwrap_or(false))
        .unwrap_or(false);
    if !shape_ok {
        // Corrupt (unparseable or wrong shape): preserve the bytes.
        set_aside(&path, recovered);
        return None;
    }
    let v = parsed.ok()?;
    // Entry-level strictness, the same standard changes.json gets:
    // an entry without a string id, or with lines that are not an
    // array of strings, makes the whole file corrupt — backed up,
    // never silently dropped or coerced (a lenient load once let a
    // malformed entry start the server fresh, and the first save
    // destroyed the original chats unrecoverably).
    let strict = (|| -> Option<Vec<Chat>> {
        let mut chats = Vec::new();
        for item in v["chats"].as_array()? {
            let id = item["id"].as_str()?.to_string();
            let title = match item.get("title") {
                None => "New chat".to_string(),
                Some(t) => t.as_str()?.to_string(),
            };
            let lines_arr = item["lines"].as_array()?;
            let mut lines = Vec::new();
            for l in lines_arr {
                lines.push(l.as_str()?.to_string());
            }
            chats.push(Chat { id, title, lines });
        }
        Some(chats)
    })();
    let chats = match strict {
        Some(c) => c,
        None => {
            set_aside(&path, recovered);
            return None;
        }
    };
    if chats.is_empty() {
        return None;
    }
    let active = v["active"]
        .as_str()
        .filter(|a| chats.iter().any(|c| &c.id == a))
        .unwrap_or(&chats[0].id)
        .to_string();
    Some((chats, active))
}

/// Handler threads allowed at once. Asks hold their thread for minutes,
/// so the cap is generous; beyond it a connection gets a plain 503.
const MAX_HANDLERS: usize = 64;
/// How long one read or write on a connection may stall before it is dropped.
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Largest request body the API accepts (chat asks are small JSON).
const MAX_BODY_BYTES: usize = 1_048_576;
/// Generous cap specifically for `.rbxlx` place imports (500+ instances & 50+ scripts).
const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;

fn max_body_for_path(path: &str) -> usize {
    let clean = path.split('?').next().unwrap_or(path);
    if clean == "/api/import/rbxlx" || clean == "/api/assets/upload" {
        MAX_IMPORT_BYTES
    } else {
        MAX_BODY_BYTES
    }
}

/// Counts one ask, idea build or revert in flight for as long as it lives.
struct BusyGuard<'a>(&'a AtomicUsize);

impl<'a> BusyGuard<'a> {
    fn new(counter: &'a AtomicUsize) -> BusyGuard<'a> {
        counter.fetch_add(1, Ordering::SeqCst);
        BusyGuard(counter)
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Holds one handler slot for the life of its thread.
struct HandlerSlot(Arc<Shared>);

impl Drop for HandlerSlot {
    fn drop(&mut self) {
        self.0.active_handlers.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn serve(listener: TcpListener, shared: Arc<Shared>) -> std::io::Result<()> {
    // Extra host names this server answers to, beyond this machine's own.
    let allowed: Arc<Vec<String>> = Arc::new(
        std::env::var("OPENRO_ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect(),
    );
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        if shared.active_handlers.load(Ordering::SeqCst) >= MAX_HANDLERS {
            // Answered off the accept loop: a silent client must not
            // stall the connections queued behind it.
            std::thread::spawn(move || {
                respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": "OpenRO is busy with other requests. Try again in a moment." }),
                );
                let _ = stream.read(&mut [0u8; 8192]);
            });
            continue;
        }
        shared.active_handlers.fetch_add(1, Ordering::SeqCst);
        let slot = HandlerSlot(Arc::clone(&shared));
        let shared = Arc::clone(&shared);
        let allowed = Arc::clone(&allowed);
        std::thread::spawn(move || {
            let _ = handle(stream, &shared, &allowed, slot);
        });
    }
    Ok(())
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
    host: String,
    origin: String,
    sec_fetch_site: String,
    content_type: String,
    content_length: usize,
    last_event_id: u64,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let mut req = Request {
        method: parts.next()?.to_string(),
        path: parts.next()?.to_string(),
        body: Vec::new(),
        host: String::new(),
        origin: String::new(),
        sec_fetch_site: String::new(),
        content_type: String::new(),
        content_length: 0,
        last_event_id: 0,
    };
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => req.content_length = v.parse().unwrap_or(0),
                "host" => req.host = v.to_string(),
                "origin" => req.origin = v.to_string(),
                "sec-fetch-site" => req.sec_fetch_site = v.to_ascii_lowercase(),
                "content-type" => req.content_type = v.to_ascii_lowercase(),
                "last-event-id" => req.last_event_id = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let body_cap = max_body_for_path(&req.path);
    // An oversized body is not read at all: handle answers 413.
    if req.content_length > body_cap {
        return Some(req);
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < req.content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(req.content_length);
    req.body = body;
    Some(req)
}

/// Lower-cased host name of a Host or Origin authority, without port.
fn host_name(authority: &str) -> String {
    let a = authority.trim().to_ascii_lowercase();
    if a.starts_with('[') {
        return a.split(']').next().map(|h| format!("{h}]")).unwrap_or_default();
    }
    a.split(':').next().unwrap_or("").to_string()
}

fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let name = host_name(host);
    ["localhost", "127.0.0.1", "[::1]"].contains(&name.as_str())
        || allowed.iter().any(|a| host_name(a) == name)
}

/// Why a request is refused before it reaches a handler, as (status,
/// plain-words message); None when it may proceed. A foreign Host is
/// refused outright (a rebinding page cannot read the API); a
/// state-changing request is refused when it is cross-site, when its
/// Origin is not this server's own authority, or when its body is not
/// JSON (a plain form post from another page cannot send one).
fn refusal(req: &Request, allowed: &[String]) -> Option<(&'static str, &'static str)> {
    if !host_allowed(&req.host, allowed) {
        return Some(("403 Forbidden", "This OpenRO server only answers requests addressed to this machine. Open it at http://127.0.0.1 instead."));
    }
    let clean_path = req.path.split('?').next().unwrap_or(&req.path);
    let body_cap = max_body_for_path(clean_path);
    if req.content_length > body_cap {
        return Some(("413 Payload Too Large", "That message is too large (over 1 MB). Send a shorter one."));
    }
    let changes_state = matches!(req.method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE");
    if !changes_state {
        return None;
    }
    let xml_import_ok = clean_path == "/api/import/rbxlx"
        && (req.content_type.starts_with("application/xml")
            || req.content_type.starts_with("text/xml"));
    let image_upload_ok = clean_path == "/api/assets/upload"
        && (req.content_type.starts_with("image/png")
            || req.content_type.starts_with("image/jpeg")
            || req.content_type.starts_with("image/webp"));
    if req.content_length > 0
        && !req.content_type.starts_with("application/json")
        && !xml_import_ok
        && !image_upload_ok
    {
        return Some(("415 Unsupported Media Type", "OpenRO takes JSON requests only. The page sends them; a plain form post is refused."));
    }
    let origin_authority = req
        .origin
        .split("://")
        .nth(1)
        .unwrap_or(req.origin.as_str())
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !req.origin.is_empty() && origin_authority != req.host.trim().to_ascii_lowercase() {
        return Some(("403 Forbidden", "cross-origin requests are refused"));
    }
    if req.sec_fetch_site == "cross-site" {
        return Some(("403 Forbidden", "cross-origin requests are refused"));
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn respond_bytes(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    cache_control: &str,
    body: &[u8],
) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: {cache_control}\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn respond_download(stream: &mut TcpStream, content_type: &str, filename: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn slugify_filename(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "openro-place".to_string()
    } else {
        trimmed
    }
}

fn respond_json(stream: &mut TcpStream, status: &str, value: &Value) {
    respond(stream, status, "application/json", &value.to_string());
}

fn handle(
    mut stream: TcpStream,
    shared: &Shared,
    allowed: &[String],
    slot: HandlerSlot,
) -> std::io::Result<()> {
    let req = match read_request(&mut stream) {
        Some(r) => r,
        None => return Ok(()),
    };
    if let Some((status, message)) = refusal(&req, allowed) {
        respond_json(&mut stream, status, &json!({ "ok": false, "error": message }));
        let body_cap = max_body_for_path(&req.path);
        if req.content_length > body_cap {
            // The body was never read: take it (bounded by its declared
            // length) so closing does not reset the 413 before it arrives.
            drain_unread(&mut stream, req.content_length.min(4 * body_cap));
        }
        return Ok(());
    }
    let (path_raw, query) = req.path.split_once('?').unwrap_or((req.path.as_str(), ""));
    let path = path_raw.to_string();

    // True SSE stream: drop the short-lived HandlerSlot so long-lived SSE
    // subscribers never starve normal concurrent HTTP traffic (SSE tracks
    // its own MAX_SSE_CLIENTS cap inside BuildEventHub).
    if req.method == "GET" && path == "/api/build/stream" {
        drop(slot);
        if shared.events.active_subscribers() >= crate::sse::MAX_SSE_CLIENTS {
            respond_json(
                &mut stream,
                "503 Service Unavailable",
                &json!({ "ok": false, "error": "Too many live build streams are open right now. Close an extra tab and retry." }),
            );
            return Ok(());
        }
        let active_chat = shared.active_chat.lock().unwrap().clone();
        let busy = shared.busy.load(Ordering::SeqCst) > 0;
        let last_id = if req.last_event_id > 0 {
            req.last_event_id
        } else {
            query_param(query, "since")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        };
        if let Err(msg) = shared.events.serve_stream(stream, &active_chat, busy, last_id) {
            eprintln!("openro: SSE stream refused: {msg}");
        }
        return Ok(());
    }

    let _slot = slot;

    // Parameterized snapshot routes: POST /api/snapshots/:id/restore & GET /api/snapshots/:id/rbxlx
    if let Some(rest) = path.strip_prefix("/api/snapshots/") {
        if let Some(snap_id) = rest.strip_suffix("/restore") {
            if req.method == "POST" {
                handle_snapshot_restore(&mut stream, shared, snap_id);
                return Ok(());
            }
        }
        if let Some(snap_id) = rest.strip_suffix("/rbxlx") {
            if req.method == "GET" {
                let xml = shared
                    .snapshots
                    .lock()
                    .unwrap()
                    .export_rbxlx(snap_id, &shared.game_name);
                match xml {
                    Some(body) => {
                        let fname = format!(
                            "{}-{}.rbxlx",
                            slugify_filename(&shared.game_name),
                            slugify_filename(snap_id)
                        );
                        respond_download(&mut stream, "application/xml; charset=utf-8", &fname, &body);
                    }
                    None => respond_json(
                        &mut stream,
                        "404 Not Found",
                        &json!({ "ok": false, "error": format!("snapshot not found: {snap_id}") }),
                    ),
                }
                return Ok(());
            }
        }
    }

    // Parameterized game art asset route: GET /api/assets/:file
    if req.method == "GET" {
        if let Some(raw_file) = path.strip_prefix("/api/assets/") {
            if !raw_file.is_empty() {
                handle_asset_file(&mut stream, shared, raw_file);
                return Ok(());
            }
        }
    }

    match (req.method.as_str(), path.as_str()) {
        ("GET", "/api/art-knowledge") => {
            respond_json(&mut stream, "200 OK", &art_knowledge_json());
        }
        ("GET", "/api/assets") => {
            handle_assets_list(&mut stream, shared);
        }
        ("POST", "/api/assets/upload") => {
            handle_assets_upload(&mut stream, shared, &req, query);
        }
        ("POST", "/api/assets/generate") => {
            handle_assets_generate(&mut stream, shared, &req.body);
        }
        ("GET", "/api/build/events") => {
            let since = query_param(query, "since")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let mut body = shared.events.recent_json(since);
            body["ok"] = json!(true);
            body["busy"] = json!(shared.busy.load(Ordering::SeqCst) > 0);
            respond_json(&mut stream, "200 OK", &body);
        }
        ("GET", "/api/snapshots") => {
            let include_tree = query_param(query, "include_tree")
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
            let store = shared.snapshots.lock().unwrap();
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "snapshots": store.list(include_tree),
                    "latest_diff": store.latest_diff(),
                }),
            );
        }
        ("POST", "/api/snapshots") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let label = body
                .get("label")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Manual snapshot")
                .to_string();
            let tree = {
                let mut guard = shared.agent.lock().unwrap();
                match guard.as_mut() {
                    Some(agent) => agent.place_tree(),
                    None => Err("Studio not connected".to_string()),
                }
            };
            match tree {
                Ok(t) => {
                    let chat_id = shared.active_chat.lock().unwrap().clone();
                    let snap = shared.snapshots.lock().unwrap().record(
                        label,
                        "manual",
                        Some(chat_id),
                        t,
                        None,
                    );
                    respond_json(
                        &mut stream,
                        "201 Created",
                        &json!({ "ok": true, "snapshot": snap.summary_json() }),
                    );
                }
                Err(e) => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": e }),
                ),
            }
        }
        ("GET", "/api/place/diff") => {
            let mut latest = shared.snapshots.lock().unwrap().latest_diff();
            latest["ok"] = json!(true);
            respond_json(&mut stream, "200 OK", &latest);
        }
        ("POST", "/api/import/rbxlx") => {
            handle_import_rbxlx(&mut stream, shared, &req);
        }
        ("POST", "/api/place/instance") => {
            handle_place_instance(&mut stream, shared, &req.body);
        }
        ("GET", "/api/skills") => {
            let catalog = crate::skills::catalog_json();
            let count = catalog.as_array().map(|a| a.len()).unwrap_or(0);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "count": count,
                    "skills": catalog,
                }),
            );
        }
        ("POST", "/api/skills/build") => {
            handle_skill_build(&mut stream, shared, &req.body);
        }
        ("GET", "/api/luau/check") | ("POST", "/api/luau/check") => {
            handle_luau_check(&mut stream, shared, req.method == "POST", &req.body);
        }
        ("GET", "/") => {
            let info = status_info(shared);
            let html = render_page_full(
                &shared.game_name,
                info.studio_connected,
                info.model_configured,
                &shared.store_recovered,
                info.workspace_mode,
            );
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", &html);
        }
        ("GET", "/api/version") => respond_json(
            &mut stream,
            "200 OK",
            &json!({ "name": "openro-rs", "version": env!("CARGO_PKG_VERSION") }),
        ),
        ("GET", "/api/export/rbxlx") => {
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.export_rbxlx(&shared.game_name) {
                    Ok(xml) => {
                        let fname = format!("{}.rbxlx", slugify_filename(&shared.game_name));
                        respond_download(
                            &mut stream,
                            "application/xml; charset=utf-8",
                            &fname,
                            &xml,
                        );
                    }
                    Err(e) => respond_json(
                        &mut stream,
                        "502 Bad Gateway",
                        &json!({ "ok": false, "error": e }),
                    ),
                },
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": "Studio not connected" }),
                ),
            }
        }
        ("GET", "/api/config") => {
            if !host_allowed(&req.host, &[]) {
                respond_json(
                    &mut stream,
                    "403 Forbidden",
                    &json!({ "ok": false, "error": "settings are only accessible from this machine (loopback)" }),
                );
                return Ok(());
            }
            respond_json(&mut stream, "200 OK", &config_snapshot(shared));
        }
        ("POST", "/api/config") => {
            if !host_allowed(&req.host, &[]) {
                respond_json(
                    &mut stream,
                    "403 Forbidden",
                    &json!({ "ok": false, "error": "changing settings is only allowed from this machine (loopback)" }),
                );
                return Ok(());
            }
            if shared.busy.load(Ordering::SeqCst) > 0 {
                respond_json(
                    &mut stream,
                    "409 Conflict",
                    &json!({ "ok": false, "error": "an ask is still running; change settings after it finishes" }),
                );
                return Ok(());
            }
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            match apply_config(shared, &body) {
                Ok(snap) => respond_json(&mut stream, "200 OK", &snap),
                Err(e) => respond_json(
                    &mut stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": e }),
                ),
            }
        }
        ("GET", "/api/update") => respond_json(&mut stream, "200 OK", &shared.updater.snapshot()),
        ("POST", "/api/update/check") => respond_json(&mut stream, "200 OK", &shared.updater.check_now()),
        ("POST", "/api/update/apply") | ("POST", "/api/update/restart") => {
            // Replacing the program is for whoever sits at this machine: a
            // loopback Host only, whatever OPENRO_ALLOWED_HOSTS names.
            if !host_allowed(&req.host, &[]) {
                respond_json(
                    &mut stream,
                    "403 Forbidden",
                    &json!({ "ok": false, "error": "updating is only allowed from this machine, not through another address" }),
                );
                return Ok(());
            }
            if path == "/api/update/apply" {
                match shared.updater.apply() {
                    Ok(v) => {
                        let mut snap = shared.updater.snapshot();
                        snap["applied"] = json!(v);
                        respond_json(&mut stream, "200 OK", &snap);
                    }
                    Err(e) => respond_json(&mut stream, "409 Conflict", &json!({ "ok": false, "error": e })),
                }
            } else if !shared.updater.snapshot()["staged"].is_string() {
                respond_json(
                    &mut stream,
                    "409 Conflict",
                    &json!({ "ok": false, "error": "no update is waiting for a restart" }),
                );
            } else if shared.busy.load(Ordering::SeqCst) > 0 {
                respond_json(
                    &mut stream,
                    "409 Conflict",
                    &json!({ "ok": false, "error": "an ask is still running; restart when it has finished" }),
                );
            } else {
                respond_json(&mut stream, "200 OK", &json!({ "restarting": true }));
                let up = Arc::clone(&shared.updater);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if let Err(e) = up.restart() {
                        eprintln!("openro-rs: restart failed: {e}");
                    }
                });
            }
        }
        ("GET", "/api/status") => {
            let info = status_info(shared);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "studio_connected": info.studio_connected,
                    "model_configured": info.model_configured,
                    "workspace_mode": info.workspace_mode,
                    "model_id": info.model_id,
                    "model_base_url": info.model_base_url,
                    "is_loopback_model": info.is_loopback_model,
                    "game_name": shared.game_name,
                    "version": env!("CARGO_PKG_VERSION"),
                    "sse_build_stream": true,
                    "skills_count": crate::skills::registry().len(),
                    "store_recovered": shared.store_recovered,
                }),
            );
        }
        ("GET", "/api/thread") => {
            respond_json(
                &mut stream,
                "200 OK",
                &json!({ "thread": shared.thread_lines() }),
            );
        }
        ("GET", "/api/chats") => {
            // can_undo: the active chat has a change set Undo can take back.
            let mut body = shared.chats_json();
            let active = shared.active_chat.lock().unwrap().clone();
            body["can_undo"] = json!(shared.undoable.lock().unwrap().contains(&active));
            respond_json(&mut stream, "200 OK", &body);
        }
        ("POST", "/api/chats") => {
            let chat = shared.create_chat();
            respond_json(
                &mut stream,
                "201 Created",
                &json!({ "id": chat.id, "title": chat.title }),
            );
        }
        ("POST", "/api/chats/select") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let id = body.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if shared.select_chat(id) {
                respond_json(&mut stream, "200 OK", &json!({ "ok": true }));
            } else {
                respond_json(
                    &mut stream,
                    "404 Not Found",
                    &json!({ "ok": false, "error": "chat not found" }),
                );
            }
        }
        ("GET", "/api/place-tree") => {
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.place_tree() {
                    Ok(tree) => respond_json(&mut stream, "200 OK", &tree),
                    Err(e) => respond_json(
                        &mut stream,
                        "502 Bad Gateway",
                        &json!({ "ok": false, "error": e }),
                    ),
                },
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": "Studio not connected" }),
                ),
            }
        }
        ("POST", "/api/chat") => {
            let _busy = BusyGuard::new(&shared.busy);
            let body: Value =
                serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let message = body
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            if message.trim().is_empty() {
                respond_json(
                    &mut stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": "empty message" }),
                );
                return Ok(());
            }
            // The chat this ask belongs to is fixed NOW, before the
            // ask runs: every line it produces — the user's words,
            // the narration, the failure — is filed into THIS chat,
            // even if the user switches chats while it runs.
            let chat_id = shared.active_chat.lock().unwrap().clone();
            // The user's words are recorded FIRST, before the ask runs:
            // a refused or failed ask must never silently lose them
            // (the no-key path used to record nothing at all, so the
            // exchange vanished on reload).
            shared.append_lines_to(&chat_id, &[format!("You: {message}")]);
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                None => {
                    let error = "no model key configured: set OPENRO_MODEL_API_KEY to enable chat";
                    shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
                    respond_json(
                        &mut stream,
                        "503 Service Unavailable",
                        &json!({ "ok": false, "error": error }),
                    );
                }
                Some(agent) => {
                    let before_tree = agent.place_tree().ok();
                    match agent.run_ask(&chat_id, &message) {
                    Ok(outcome) if outcome.refused => {
                        // Refused, not built: the plain reason is the
                        // answer, the model's success claim is not shown,
                        // and no pitch follows.
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        respond_json(
                            &mut stream,
                            "502 Bad Gateway",
                            &json!({ "ok": false, "error": outcome.summary }),
                        );
                    }
                    Ok(outcome) => {
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        let mut snapshot_id = Value::Null;
                        let mut diff_val = Value::Null;
                        // Only an ask that really changed the game records
                        // what it built and pitches the follow-up set (once
                        // per context). A zero-change ask gets no pitch.
                        if outcome.changed {
                            shared.note_built(&chat_id, &outcome.skills_used);
                            if let Ok(after_tree) = agent.place_tree() {
                                let explicit = before_tree
                                    .as_ref()
                                    .map(|b| compute_diff(b, &after_tree));
                                let label: String = format!(
                                    "Built: {}",
                                    message.chars().take(48).collect::<String>()
                                );
                                let snap = shared.snapshots.lock().unwrap().record(
                                    label,
                                    "build",
                                    Some(chat_id.clone()),
                                    after_tree,
                                    explicit,
                                );
                                snapshot_id = json!(snap.id);
                                diff_val = snap.diff.to_json();
                            }
                            let (_, pitch) = shared.pitch_for(
                                &chat_id,
                                "OpenRO: That landed. Next, I could add:",
                            );
                            if !pitch.is_empty() {
                                shared.append_lines_to(&chat_id, &pitch);
                            }
                        }
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({
                                "ok": true,
                                "summary": outcome.summary,
                                "thread": outcome.thread,
                                "transcript": outcome.transcript,
                                "snapshot_id": snapshot_id,
                                "diff": diff_val,
                            }),
                        );
                    }
                    Err(f) => {
                        // The ask's own lines and its failure both join
                        // the persisted thread — the chat tells the
                        // truth about what happened, and the change set
                        // stays revertable in this chat. Skills it did
                        // land still count as built for the pitch
                        // engine: the scripts are in the place even
                        // though the ask failed.
                        shared.append_lines_to(&chat_id, &f.thread);
                        shared.append_lines_to(&chat_id, &[format!("OpenRO: {}", f.message)]);
                        shared.note_built(&chat_id, &f.skills_used);
                        let status = if f.message.contains("no model key configured")
                            || f.message.contains("Studio not connected")
                            || f.message.contains("does not offer")
                        {
                            "503 Service Unavailable"
                        } else {
                            "502 Bad Gateway"
                        };
                        respond_json(
                            &mut stream,
                            status,
                            &json!({ "ok": false, "error": f.message }),
                        );
                    }
                }
                }
            }
        }
        ("POST", "/api/ideas") => {
            // The proactive pitch: which ideas fit this chat's game
            // right now. A new set is also spoken into the thread (the
            // agent talking first is part of the chat's record); a set
            // already pitched is returned silently for the buttons.
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let intro = if shared.thread_lines().is_empty() {
                "OpenRO: While you think — here are games I can build for you right now, in one go:"
            } else {
                "OpenRO: Next, I could add:"
            };
            let (ideas, lines) = shared.pitch_for(&chat_id, intro);
            if !lines.is_empty() {
                shared.append_lines_to(&chat_id, &lines);
            }
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "ideas": ideas_json(&ideas),
                    "built_titles": shared.built_titles_for(&chat_id),
                    "pitched_new": !lines.is_empty(),
                }),
            );
        }
        ("POST", "/api/ideas/build") => {
            let _busy = BusyGuard::new(&shared.busy);
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let id = body
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let idea = match crate::ideas::find(&id) {
                Some(i) => i,
                None => {
                    respond_json(
                        &mut stream,
                        "404 Not Found",
                        &json!({ "ok": false, "error": format!("unknown idea: {id}") }),
                    );
                    return Ok(());
                }
            };
            let chat_id = shared.active_chat.lock().unwrap().clone();
            // The tap is the user's words for this build — recorded
            // first, like a typed ask, so a refused build never loses it.
            shared.append_lines_to(&chat_id, &[format!("You: Build this — {}", idea.title)]);
            // Prerequisites are enforced HERE, not just at pitch time:
            // an idea whose `needs` are not in the game yet would build
            // something dead on arrival (a shop with no currency to
            // charge), so the build refuses honestly and names what is
            // missing — a direct API call gets the same answer the
            // pitch engine would have given by never offering it.
            let missing: Vec<&str> = {
                let state = shared.ideas.lock().unwrap();
                let built: &[String] = state
                    .get(&chat_id)
                    .map(|s| s.built.as_slice())
                    .unwrap_or(&[]);
                idea.needs
                    .iter()
                    .filter(|n| !built.iter().any(|b| b == *n))
                    .cloned()
                    .collect()
            };
            if !missing.is_empty() {
                let error = crate::agent::prerequisite_sentence(idea.title, missing[0]);
                shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
                respond_json(
                    &mut stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": error }),
                );
                return Ok(());
            }
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                None => {
                    let error =
                        "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server";
                    shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
                    respond_json(
                        &mut stream,
                        "503 Service Unavailable",
                        &json!({ "ok": false, "error": error }),
                    );
                }
                Some(agent) => {
                    let before_tree = agent.place_tree().ok();
                    match agent.build_idea(&chat_id, &id) {
                    Ok(outcome) => {
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        shared.note_built(&chat_id, &outcome.skills_used);
                        let mut snapshot_id = Value::Null;
                        let mut diff_val = Value::Null;
                        if let Ok(after_tree) = agent.place_tree() {
                            let explicit = before_tree
                                .as_ref()
                                .map(|b| compute_diff(b, &after_tree));
                            let snap = shared.snapshots.lock().unwrap().record(
                                format!("Blueprint: {}", idea.title),
                                "idea",
                                Some(chat_id.clone()),
                                after_tree,
                                explicit,
                            );
                            snapshot_id = json!(snap.id);
                            diff_val = snap.diff.to_json();
                        }
                        let (next, pitch) = shared.pitch_for(
                            &chat_id,
                            "OpenRO: That landed. Next, I could add:",
                        );
                        if !pitch.is_empty() {
                            shared.append_lines_to(&chat_id, &pitch);
                        }
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({
                                "ok": true,
                                "summary": outcome.summary,
                                "thread": outcome.thread,
                                "transcript": outcome.transcript,
                                "ideas": ideas_json(&next),
                                "snapshot_id": snapshot_id,
                                "diff": diff_val,
                            }),
                        );
                    }
                    Err(f) => {
                        shared.append_lines_to(&chat_id, &f.thread);
                        shared.append_lines_to(&chat_id, &[format!("OpenRO: {}", f.message)]);
                        shared.note_built(&chat_id, &f.skills_used);
                        let status = if f.message.contains("Studio not connected")
                            || f.message.contains("does not offer")
                        {
                            "503 Service Unavailable"
                        } else {
                            "502 Bad Gateway"
                        };
                        respond_json(
                            &mut stream,
                            status,
                            &json!({ "ok": false, "error": f.message }),
                        );
                    }
                }
                }
            }
        }
        ("POST", "/api/revert") => {
            let _busy = BusyGuard::new(&shared.busy);
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let mut guard = shared.agent.lock().unwrap();
            let reverted = guard.as_ref().map(|a| a.top_skills(&chat_id)).unwrap_or_default();
            match guard.as_mut() {
                Some(agent) => {
                    let before_tree = agent.place_tree().ok();
                    match agent.revert_chat(&chat_id) {
                    Ok(restored) => {
                        shared.forget_built(&chat_id, &reverted, &agent.chat_skills(&chat_id));
                        let mut snapshot_id = Value::Null;
                        let mut diff_val = Value::Null;
                        if let Ok(after_tree) = agent.place_tree() {
                            let explicit = before_tree
                                .as_ref()
                                .map(|b| compute_diff(b, &after_tree));
                            let snap = shared.snapshots.lock().unwrap().record(
                                format!("Undo ({} change(s))", restored.len()),
                                "revert",
                                Some(chat_id.clone()),
                                after_tree,
                                explicit,
                            );
                            snapshot_id = json!(snap.id);
                            diff_val = snap.diff.to_json();
                        }
                        shared.events.publish(
                            "revert",
                            chat_id.clone(),
                            "reverted",
                            "Undo complete",
                            format!("Reverted {} change(s)", restored.len()),
                            None,
                            None,
                            100,
                        );
                        shared.append_lines_to(&chat_id, &[format!(
                            "OpenRO: Undid the last build: {} change(s) put back.",
                            restored.len()
                        )]);
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({
                                "ok": true,
                                "restored": restored,
                                "snapshot_id": snapshot_id,
                                "diff": diff_val,
                            }),
                        );
                    }
                    Err(e) if e == crate::agent::NOTHING_TO_REVERT => {
                        // A normal state, stated plainly — never the
                        // fabricated "Reverted the last change set."
                        shared.append_lines_to(&chat_id, &[
                            "OpenRO: Nothing to revert — no change set is waiting in this chat."
                                .to_string(),
                        ]);
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({ "ok": false, "error": e }),
                        );
                    }
                    Err(e) => {
                        // A failed revert is part of the chat's record
                        // too — never a silent API-only error. The set
                        // is kept, and the line says so.
                        shared.append_lines_to(&chat_id, &[format!(
                            "OpenRO: Revert did not finish: {e}"
                        )]);
                        respond_json(
                            &mut stream,
                            "502 Bad Gateway",
                            &json!({ "ok": false, "error": e }),
                        );
                    }
                }
                }
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": "Studio not connected" }),
                ),
            }
        }
        _ => respond_json(
            &mut stream,
            "404 Not Found",
            &json!({ "ok": false, "error": "not found" }),
        ),
    }
    Ok(())
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

fn handle_snapshot_restore(stream: &mut TcpStream, shared: &Shared, snap_id: &str) {
    let _busy = BusyGuard::new(&shared.busy);
    let snap = {
        let store = shared.snapshots.lock().unwrap();
        store.get(snap_id).cloned()
    };
    let Some(target) = snap else {
        respond_json(
            stream,
            "404 Not Found",
            &json!({ "ok": false, "error": format!("snapshot not found: {snap_id}") }),
        );
        return;
    };
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({ "ok": false, "error": "Studio not connected" }),
        );
        return;
    };
    match agent.apply_place_tree(&target.tree) {
        Ok(diff) => {
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let restored_tree = agent.place_tree().unwrap_or_else(|_| target.tree.clone());
            let new_snap = shared.snapshots.lock().unwrap().record(
                format!("Restored: {}", target.label),
                "restore",
                Some(chat_id.clone()),
                restored_tree,
                Some(diff.clone()),
            );
            shared.events.publish(
                "restore",
                chat_id,
                "restored",
                "Snapshot restored",
                format!("Restored '{}' ({})", target.label, diff.summary()),
                None,
                None,
                100,
            );
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "restored_from": target.id,
                    "snapshot_id": new_snap.id,
                    "instance_count": new_snap.instance_count,
                    "script_count": new_snap.script_count,
                    "diff": diff.to_json(),
                }),
            );
        }
        Err(e) => respond_json(
            stream,
            "502 Bad Gateway",
            &json!({ "ok": false, "error": e }),
        ),
    }
}

fn handle_import_rbxlx(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    let _busy = BusyGuard::new(&shared.busy);
    let (xml_str, label) = if req.content_type.starts_with("application/xml")
        || req.content_type.starts_with("text/xml")
    {
        match std::str::from_utf8(&req.body) {
            Ok(s) => (s.to_string(), "Imported .rbxlx".to_string()),
            Err(_) => {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": "invalid UTF-8 in .rbxlx XML payload" }),
                );
                return;
            }
        }
    } else {
        let body: Value = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(_) => {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": "invalid JSON body; expected {\"xml\": \"<roblox ...>\"}" }),
                );
                return;
            }
        };
        let Some(xml) = body.get("xml").and_then(|v| v.as_str()) else {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "missing string field 'xml' in import payload" }),
            );
            return;
        };
        let lbl = body
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Imported .rbxlx")
            .to_string();
        (xml.to_string(), lbl)
    };

    let parsed_tree = match parse_rbxlx_xml(&xml_str) {
        Ok(t) => t,
        Err(e) => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": e }),
            );
            return;
        }
    };

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({ "ok": false, "error": "Studio not connected" }),
        );
        return;
    };
    match agent.apply_place_tree(&parsed_tree) {
        Ok(diff) => {
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let final_tree = agent.place_tree().unwrap_or(parsed_tree);
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "import",
                Some(chat_id.clone()),
                final_tree.clone(),
                Some(diff.clone()),
            );
            shared.events.publish(
                "import",
                chat_id,
                "imported",
                "Imported .rbxlx",
                format!(
                    "Imported {} instance(s) and {} script(s)",
                    snap.instance_count, snap.script_count
                ),
                None,
                None,
                100,
            );
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "snapshot_id": snap.id,
                    "instance_count": snap.instance_count,
                    "script_count": snap.script_count,
                    "diff": diff.to_json(),
                    "tree": final_tree,
                }),
            );
        }
        Err(e) => respond_json(
            stream,
            "502 Bad Gateway",
            &json!({ "ok": false, "error": e }),
        ),
    }
}

fn handle_place_instance(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "invalid JSON body" }),
            );
            return;
        }
    };
    let Some(path) = body.get("path").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
        respond_json(
            stream,
            "400 Bad Request",
            &json!({ "ok": false, "error": "missing string 'path' (e.g. Workspace.JumpPad)" }),
        );
        return;
    };
    let delete = body.get("delete").and_then(|v| v.as_bool()).unwrap_or(false);
    let class_name = body.get("class_name").and_then(|v| v.as_str());
    let properties = body.get("properties").cloned().unwrap_or_else(|| json!({}));

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({ "ok": false, "error": "Studio not connected" }),
        );
        return;
    };
    let before = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));

    let op_res = if delete {
        agent.delete_instance_at(path).map(|()| json!({ "deleted": true, "path": path }))
    } else {
        agent.set_instance_properties(path, class_name, &properties)
    };
    match op_res {
        Ok(instance_info) => {
            let after = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let diff = compute_diff(&before, &after);
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let label = if delete {
                format!("Deleted {path}")
            } else {
                format!("Edited {path}")
            };
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "edit",
                Some(chat_id.clone()),
                after,
                Some(diff.clone()),
            );
            shared.events.publish(
                "edit",
                chat_id,
                "done",
                label,
                diff.summary(),
                None,
                Some(path.to_string()),
                100,
            );
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "path": path,
                    "instance": instance_info,
                    "snapshot_id": snap.id,
                    "diff": diff.to_json(),
                }),
            );
        }
        Err(e) => respond_json(
            stream,
            "400 Bad Request",
            &json!({ "ok": false, "error": e }),
        ),
    }
}

fn handle_skill_build(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "invalid JSON body" }),
            );
            return;
        }
    };
    let name = body
        .get("skill")
        .or_else(|| body.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let Some(def) = crate::skills::find(name) else {
        respond_json(
            stream,
            "404 Not Found",
            &json!({ "ok": false, "error": format!("unknown skill: {name}") }),
        );
        return;
    };
    let skill_args = body
        .get("args")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| serde_json::from_str(def.default_args).unwrap_or_else(|_| json!({})));
    let script_path = body
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(def.default_path);
    let chat_id = shared.active_chat.lock().unwrap().clone();
    shared.append_lines_to(&chat_id, &[format!("You: Add system — {}", def.title)]);

    let steps = vec![
        json!({ "skill": def.name, "args": skill_args }),
        json!({ "tool": "write_script", "args": { "path": script_path, "skill": def.name } }),
        json!({ "tool": "playtest", "args": {} }),
    ];

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        let error = "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server";
        shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({ "ok": false, "error": error }),
        );
        return;
    };
    let before_tree = agent.place_tree().ok();
    match agent.run_plan(&chat_id, def.title, &steps) {
        Ok(outcome) => {
            shared.append_lines_to(&chat_id, &outcome.thread);
            shared.note_built(&chat_id, &outcome.skills_used);
            let mut snapshot_id = Value::Null;
            let mut diff_val = Value::Null;
            if let Ok(after_tree) = agent.place_tree() {
                let explicit = before_tree
                    .as_ref()
                    .map(|b| compute_diff(b, &after_tree));
                let snap = shared.snapshots.lock().unwrap().record(
                    format!("System: {}", def.title),
                    "skill",
                    Some(chat_id.clone()),
                    after_tree,
                    explicit,
                );
                snapshot_id = json!(snap.id);
                diff_val = snap.diff.to_json();
            }
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "skill": def.name,
                    "title": def.title,
                    "path": script_path,
                    "summary": outcome.summary,
                    "thread": outcome.thread,
                    "snapshot_id": snapshot_id,
                    "diff": diff_val,
                }),
            );
        }
        Err(f) => {
            shared.append_lines_to(&chat_id, &f.thread);
            shared.append_lines_to(&chat_id, &[format!("OpenRO: {}", f.message)]);
            shared.note_built(&chat_id, &f.skills_used);
            respond_json(
                stream,
                "502 Bad Gateway",
                &json!({ "ok": false, "error": f.message }),
            );
        }
    }
}

fn handle_luau_check(stream: &mut TcpStream, shared: &Shared, is_post: bool, raw_body: &[u8]) {
    let tree = if is_post && !raw_body.is_empty() {
        let body: Value = match serde_json::from_slice(raw_body) {
            Ok(v) => v,
            Err(_) => {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": "invalid JSON body" }),
                );
                return;
            }
        };
        if let Some(src) = body.get("source").and_then(|v| v.as_str()) {
            let path = body
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("ServerScriptService.Check");
            json!({ "instances": {}, "scripts": { path: src } })
        } else {
            body
        }
    } else {
        let mut guard = shared.agent.lock().unwrap();
        match guard.as_mut() {
            Some(agent) => match agent.place_tree() {
                Ok(t) => t,
                Err(e) => {
                    respond_json(
                        stream,
                        "502 Bad Gateway",
                        &json!({ "ok": false, "error": e }),
                    );
                    return;
                }
            },
            None => json!({ "instances": {}, "scripts": {} }),
        }
    };

    let mut scripts_map = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts_map.insert(k.clone(), s.to_string());
            }
        }
    }
    let mut instances_map = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("instances").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            instances_map.insert(k.clone(), v.clone());
        }
    }

    let report = crate::luau_check::analyze_place(&scripts_map, &instances_map);
    let mut out = report.to_json();
    out["passed"] = json!(report.is_ok());
    out["ok"] = json!(true);
    out["scripts_checked"] = json!(scripts_map.len());
    respond_json(stream, "200 OK", &out);
}

/// Read and discard up to `budget` bytes the client is still sending.
fn drain_unread(stream: &mut TcpStream, mut budget: usize) {
    let mut chunk = [0u8; 8192];
    while budget > 0 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => budget = budget.saturating_sub(n),
        }
    }
}

#[derive(Clone)]
struct StatusInfo {
    studio_connected: bool,
    model_configured: bool,
    workspace_mode: &'static str,
    model_id: String,
    model_base_url: String,
    is_loopback_model: bool,
}

fn extract_status_from_agent(agent: Option<&mut Agent>) -> StatusInfo {
    match agent {
        Some(agent) => StatusInfo {
            studio_connected: agent.studio_connected(),
            model_configured: agent.model.is_configured(),
            workspace_mode: agent.backend_mode(),
            model_id: agent.model.model_id().to_string(),
            model_base_url: agent.model.base_url().to_string(),
            is_loopback_model: agent.model.is_loopback(),
        },
        None => StatusInfo {
            studio_connected: false,
            model_configured: false,
            workspace_mode: "disconnected",
            model_id: DEFAULT_MODEL_ID.to_string(),
            model_base_url: DEFAULT_BASE_URL.to_string(),
            is_loopback_model: false,
        },
    }
}

fn status_info(shared: &Shared) -> StatusInfo {
    match shared.agent.try_lock() {
        Ok(mut guard) => {
            let info = extract_status_from_agent(guard.as_mut());
            *shared.status_cache.lock().unwrap() = info.clone();
            info
        }
        Err(_) => shared.status_cache.lock().unwrap().clone(),
    }
}

fn config_snapshot(shared: &Shared) -> Value {
    let cfg = shared.config.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let (base_url, model_id, has_key, masked, configured, is_loopback, studio_connected, active_backend, rbxlx_path) =
        match guard.as_mut() {
            Some(agent) => (
                agent.model.base_url().to_string(),
                agent.model.model_id().to_string(),
                agent.model.has_api_key(),
                agent.model.api_key_masked(),
                agent.model.is_configured(),
                agent.model.is_loopback(),
                agent.studio_connected(),
                agent.backend_mode(),
                agent.rbxlx_path().map(|p| p.display().to_string()),
            ),
            None => (
                cfg.base_url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
                cfg.model_id.clone().unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
                cfg.api_key.as_ref().is_some_and(|k| !k.trim().is_empty()),
                None,
                false,
                false,
                false,
                "disconnected",
                None,
            ),
        };
    drop(guard);
    let vinegar = McpClient::detect_vinegar_studio_mcp();
    let gateways: Vec<Value> = probe_local_gateways()
        .into_iter()
        .map(|g| {
            json!({
                "base_url": g.base_url,
                "model_id": g.model_id,
                "label": g.label,
            })
        })
        .collect();
    let gemini_key = cfg
        .gemini_api_key
        .clone()
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    let openai_img_key = cfg
        .openai_image_api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    json!({
        "ok": true,
        "base_url": base_url,
        "model_id": model_id,
        "has_api_key": has_key,
        "api_key_masked": masked,
        "model_configured": configured,
        "is_loopback_model": is_loopback,
        "studio_connected": studio_connected,
        "workspace_mode": cfg.workspace_mode.unwrap_or_else(|| "auto".to_string()),
        "active_backend": active_backend,
        "mcp_command": cfg.mcp_command.unwrap_or_default(),
        "vinegar_detected": vinegar.is_some(),
        "vinegar_mcp_path": vinegar,
        "rbxlx_path": rbxlx_path,
        "detected_gateways": gateways,
        "image_provider": cfg.image_provider.unwrap_or_else(|| "auto".to_string()),
        "has_gemini_api_key": gemini_key.is_some(),
        "gemini_api_key_masked": gemini_key.as_deref().map(mask_secret),
        "has_openai_image_api_key": openai_img_key.is_some(),
        "openai_image_api_key_masked": openai_img_key.as_deref().map(mask_secret),
    })
}

fn apply_config(shared: &Shared, body: &Value) -> Result<Value, String> {
    let mut cfg = shared.config.lock().unwrap().clone();

    let base_url = match body.get("base_url").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => {
            let trimmed = s.trim();
            if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
                return Err("base_url must start with http:// or https://".to_string());
            }
            cfg.base_url = Some(trimmed.to_string());
            trimmed.to_string()
        }
        _ => cfg
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
    };

    let model_id = match body.get("model_id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => {
            cfg.model_id = Some(s.trim().to_string());
            s.trim().to_string()
        }
        _ => cfg
            .model_id
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
    };

    let clear_key = body
        .get("clear_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if clear_key {
        cfg.api_key = None;
    } else if let Some(raw_key) = body.get("api_key").and_then(|v| v.as_str()) {
        let k = raw_key.trim();
        if !k.is_empty() {
            cfg.api_key = Some(k.to_string());
        }
    }

    if let Some(mcp) = body.get("mcp_command").and_then(|v| v.as_str()) {
        let trimmed = mcp.trim();
        cfg.mcp_command = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }

    if let Some(mode) = body.get("workspace_mode").and_then(|v| v.as_str()) {
        match mode {
            "auto" | "local_place" | "studio_mcp" => cfg.workspace_mode = Some(mode.to_string()),
            other => return Err(format!("invalid workspace_mode '{other}' (expected auto, local_place, or studio_mcp)")),
        }
    }

    if body
        .get("clear_gemini_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        cfg.gemini_api_key = None;
    } else if let Some(k) = body.get("gemini_api_key").and_then(|v| v.as_str()) {
        let t = k.trim();
        if !t.is_empty() {
            cfg.gemini_api_key = Some(t.to_string());
        }
    }
    if body
        .get("clear_openai_image_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        cfg.openai_image_api_key = None;
    } else if let Some(k) = body.get("openai_image_api_key").and_then(|v| v.as_str()) {
        let t = k.trim();
        if !t.is_empty() {
            cfg.openai_image_api_key = Some(t.to_string());
        }
    }
    if let Some(prov) = body.get("image_provider").and_then(|v| v.as_str()) {
        match prov {
            "auto" | "gemini" | "openai" => cfg.image_provider = Some(prov.to_string()),
            other => return Err(format!("invalid image_provider '{other}' (expected auto, gemini, or openai)")),
        }
    }

    let desired_mode = cfg.workspace_mode.as_deref().unwrap_or("auto");
    let new_backend: Option<PlaceBackend> = match desired_mode {
        "local_place" => {
            let place = match &shared.data_dir {
                Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
                None => LocalPlace::in_memory(shared.game_name.clone()),
            };
            Some(PlaceBackend::Local(place))
        }
        "studio_mcp" => {
            let cmd = cfg
                .mcp_command
                .clone()
                .or_else(McpClient::detect_vinegar_studio_mcp)
                .ok_or_else(|| {
                    "No Studio MCP command set and Roblox Studio (Vinegar) is not running.".to_string()
                })?;
            Some(PlaceBackend::Mcp(McpClient::spawn(&cmd)?))
        }
        _ => {
            // "auto": explicit mcp_command -> running Vinegar -> Local Place Engine
            let mcp_try = cfg
                .mcp_command
                .clone()
                .or_else(McpClient::detect_vinegar_studio_mcp);
            if let Some(cmd) = mcp_try {
                match McpClient::spawn(&cmd) {
                    Ok(client) => Some(PlaceBackend::Mcp(client)),
                    Err(_) => {
                        let place = match &shared.data_dir {
                            Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
                            None => LocalPlace::in_memory(shared.game_name.clone()),
                        };
                        Some(PlaceBackend::Local(place))
                    }
                }
            } else {
                let place = match &shared.data_dir {
                    Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
                    None => LocalPlace::in_memory(shared.game_name.clone()),
                };
                Some(PlaceBackend::Local(place))
            }
        }
    };

    let new_model = ModelClient::new(base_url, model_id, cfg.api_key.clone());

    if let Some(dir) = &shared.data_dir {
        cfg.save(dir)?;
    }
    *shared.config.lock().unwrap() = cfg;

    {
        let mut guard = shared.agent.lock().unwrap();
        match guard.as_mut() {
            Some(agent) => {
                agent.model = new_model;
                agent.studio = new_backend;
                agent.attach_events(Arc::clone(&shared.events));
            }
            None => {
                let mut agent = Agent::with_backend(new_model, new_backend);
                if let Some(dir) = &shared.data_dir {
                    agent.set_changes_file(dir.join("changes.json"));
                }
                agent.attach_undo_sink(Arc::clone(&shared.undoable));
                agent.attach_events(Arc::clone(&shared.events));
                *guard = Some(agent);
            }
        }
        *shared.status_cache.lock().unwrap() = extract_status_from_agent(guard.as_mut());
    }

    Ok(config_snapshot(shared))
}

fn mask_secret(k: &str) -> String {
    let n = k.chars().count();
    if n <= 8 {
        "••••••••".to_string()
    } else {
        let tail: String = k.chars().skip(n - 4).collect();
        format!("••••{tail}")
    }
}

fn art_knowledge_json() -> Value {
    json!({
        "ok": true,
        "title": "Anime Fan-Art & Creative Homage Knowledge Point",
        "never_refuse_anime_homage": true,
        "policy": "Never knee-jerk refuse anime-inspired Roblox games, card battlers, or stylized fan-art themes on copyright grounds. Translate requested anime character archetypes into original, DMCA-safe homage designs that preserve recognizable silhouette, cel-shading, color palette, elemental aura, and gameplay feel under original names and lore so the Roblox game ships safely.",
        "guidelines": [
            "Translate iconic anime archetypes into original titles, passives, and ultimates rather than refusing creative requests.",
            "Preserve high-impact visual readability: crisp cel-shaded line art, bold 3:4 trading-card framing, elemental particle auras, and high-contrast palettes.",
            "Keep card art watermark-free and text-free so Roblox SurfaceGui/ImageLabel overlays render rarity frames, stats, and foil effects crisply.",
            "Bind each generated asset through asset-manifest.json and Roblox Studio Decal/ImageLabel properties for instant studio import."
        ],
        "archetype_translations": [
            { "archetype": "Straw-Hat Pirate Brawler", "homage_title": "Crimson Sea Captain", "element": "Flame", "rarity": "Mythic", "palette": "Crimson, obsidian, sunlit gold" },
            { "archetype": "Limitless Void Mentor", "homage_title": "Infinity Void Sorcerer", "element": "Void", "rarity": "Celestial", "palette": "Electric violet, silver-white, abyssal indigo" },
            { "archetype": "Lightning Iaido Swordsman", "homage_title": "Thunder Flash Ronin", "element": "Storm", "rarity": "Legendary", "palette": "Storm gold, deep navy, arc cyan" },
            { "archetype": "Shadow Monarch Necromancer", "homage_title": "Shadow Sovereign", "element": "Shadow", "rarity": "Celestial", "palette": "Obsidian black, neon cobalt, phantom violet" },
            { "archetype": "Fox-Flame Ninja", "homage_title": "Nine-Tail Ember Shinobi", "element": "Flame", "rarity": "Mythic", "palette": "Solar orange, crimson embers, warm gold" },
            { "archetype": "Ascended Golden Martial Artist", "homage_title": "Supernova Ascended Striker", "element": "Flame", "rarity": "Secret", "palette": "Blazing gold, plasma cyan, starlight white" },
            { "archetype": "Gravity-Blade Ronin", "homage_title": "Blood-Moon Gravity Ronin", "element": "Void", "rarity": "Epic", "palette": "Blood crimson, dark slate, lunar silver" },
            { "archetype": "Transmutation Alchemist", "homage_title": "Eclipse Arcane Alchemist", "element": "Arcane", "rarity": "Rare", "palette": "Alchemical gold, crimson coat, arcane teal" }
        ],
        "prompt_template": "High-craft modern anime cel-shaded trading card illustration (3:4 portrait), dynamic heroic pose, bold inked contours, glowing {element} elemental aura, dramatic rim lighting, original stylized homage character '{title}' ({rarity} tier), no text, no watermark, no borders, clean studio quality."
    })
}

fn game_assets_dir(shared: &Shared) -> Option<PathBuf> {
    shared.data_dir.as_ref().map(|d| d.join("game-assets"))
}

fn load_game_assets(data_dir: &std::path::Path) -> GameAssetsIndex {
    let manifest_path = data_dir.join("game-assets").join("manifest.json");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return GameAssetsIndex::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return GameAssetsIndex::default();
    };
    let items = v
        .get("assets")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    GameAssetsIndex {
        items,
        mem_blobs: std::collections::HashMap::new(),
    }
}

fn persist_game_assets_manifest(dir: &std::path::Path, items: &[Value]) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("manifest.json");
    let tmp = dir.join("manifest.json.tmp");
    let body = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "assets": items,
    });
    if let Ok(text) = serde_json::to_string_pretty(&body) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Strictly validate an asset filename against path traversal, NUL bytes,
/// leading dots, directory separators, and unsupported extensions.
fn validate_asset_filename(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > 128 {
        return Err("asset filename must be 1..=128 characters".to_string());
    }
    if raw.contains('/')
        || raw.contains('\\')
        || raw.contains("..")
        || raw.contains('\0')
        || raw.contains('%')
        || raw.starts_with('.')
    {
        return Err("invalid asset filename (path traversal or special characters rejected)".to_string());
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("asset filename may only contain ASCII letters, digits, '-', '_', and '.'".to_string());
    }
    let lower = raw.to_ascii_lowercase();
    if !(lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".webp")) {
        return Err("asset filename must end with .png, .jpg, .jpeg, or .webp".to_string());
    }
    Ok(raw.to_string())
}

fn detect_image_mime(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some(("image/png", "png"));
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Some(("image/jpeg", "jpg"));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(("image/webp", "webp"));
    }
    None
}

fn mime_for_asset_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    let payload = if let Some((_, after)) = trimmed.split_once("base64,") {
        after
    } else {
        trimmed
    };
    let mut out = Vec::with_capacity((payload.len() * 3) / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for b in payload.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err("invalid base64 character in image payload".to_string()),
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    if out.is_empty() {
        return Err("empty image payload after base64 decoding".to_string());
    }
    Ok(out)
}

fn handle_assets_list(stream: &mut TcpStream, shared: &Shared) {
    let idx = shared.game_assets.lock().unwrap();
    let dir_str = game_assets_dir(shared).map(|p| p.display().to_string());
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "count": idx.items.len(),
            "assets": idx.items,
            "assets_dir": dir_str,
        }),
    );
}

fn handle_asset_file(stream: &mut TcpStream, shared: &Shared, raw_file: &str) {
    let safe_name = match validate_asset_filename(raw_file) {
        Ok(s) => s,
        Err(e) => {
            respond_json(stream, "400 Bad Request", &json!({ "ok": false, "error": e }));
            return;
        }
    };
    if let Some(dir) = game_assets_dir(shared) {
        let candidate = dir.join(&safe_name);
        if let (Ok(canon_dir), Ok(canon_file)) = (std::fs::canonicalize(&dir), std::fs::canonicalize(&candidate)) {
            if !canon_file.starts_with(&canon_dir) {
                respond_json(
                    stream,
                    "403 Forbidden",
                    &json!({ "ok": false, "error": "asset path escapes game-assets directory" }),
                );
                return;
            }
            if let Ok(bytes) = std::fs::read(&canon_file) {
                respond_bytes(
                    stream,
                    "200 OK",
                    mime_for_asset_name(&safe_name),
                    "public, max-age=3600",
                    &bytes,
                );
                return;
            }
        }
    }
    let idx = shared.game_assets.lock().unwrap();
    if let Some(bytes) = idx.mem_blobs.get(&safe_name) {
        respond_bytes(
            stream,
            "200 OK",
            mime_for_asset_name(&safe_name),
            "public, max-age=3600",
            bytes,
        );
        return;
    }
    respond_json(
        stream,
        "404 Not Found",
        &json!({ "ok": false, "error": format!("asset not found: {safe_name}") }),
    );
}

fn store_uploaded_asset(
    shared: &Shared,
    safe_name: &str,
    bytes: &[u8],
    meta: Value,
) -> Result<Value, String> {
    let (mime, _) = detect_image_mime(bytes)
        .ok_or_else(|| "uploaded bytes are not a valid PNG, JPEG, or WebP image".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let stem = safe_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(safe_name);
    let card_id = meta
        .get("card_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(stem)
        .to_string();
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&card_id)
        .to_string();

    if let Some(dir) = game_assets_dir(shared) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("create game-assets dir: {e}"))?;
        let dest = dir.join(safe_name);
        let tmp = dir.join(format!("{safe_name}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| format!("write asset: {e}"))?;
        std::fs::rename(&tmp, &dest).map_err(|e| format!("commit asset: {e}"))?;
    }

    let record = json!({
        "filename": safe_name,
        "url": format!("/api/assets/{safe_name}"),
        "mime": mime,
        "bytes": bytes.len(),
        "card_id": card_id,
        "title": title,
        "rarity": meta.get("rarity").and_then(|v| v.as_str()).unwrap_or("Epic"),
        "odds": meta.get("odds").and_then(|v| v.as_str()).unwrap_or("1 in 32"),
        "element": meta.get("element").and_then(|v| v.as_str()).unwrap_or("Flame"),
        "atk": meta.get("atk").and_then(|v| v.as_u64()).unwrap_or(300),
        "hp": meta.get("hp").and_then(|v| v.as_u64()).unwrap_or(1100),
        "speed": meta.get("speed").and_then(|v| v.as_u64()).unwrap_or(85),
        "passive": meta.get("passive").and_then(|v| v.as_str()).unwrap_or(""),
        "ultimate": meta.get("ultimate").and_then(|v| v.as_str()).unwrap_or(""),
        "prompt": meta.get("prompt").and_then(|v| v.as_str()).unwrap_or(""),
        "source": meta.get("source").and_then(|v| v.as_str()).unwrap_or("upload"),
        "created_at_ms": now_ms,
    });

    {
        let mut idx = shared.game_assets.lock().unwrap();
        idx.items
            .retain(|it| it.get("filename").and_then(|f| f.as_str()) != Some(safe_name));
        idx.items.push(record.clone());
        if shared.data_dir.is_none() {
            idx.mem_blobs.insert(safe_name.to_string(), bytes.to_vec());
        } else if let Some(dir) = game_assets_dir(shared) {
            persist_game_assets_manifest(&dir, &idx.items);
        }
    }
    let chat_id = shared.active_chat.lock().unwrap().clone();
    shared.events.publish(
        "asset",
        chat_id,
        "done",
        "Card Art Uploaded",
        format!("Stored {safe_name} ({} bytes)", bytes.len()),
        None,
        Some(format!("/api/assets/{safe_name}")),
        100,
    );
    Ok(record)
}

fn handle_assets_upload(stream: &mut TcpStream, shared: &Shared, req: &Request, query: &str) {
    let is_raw_image = req.content_type.starts_with("image/png")
        || req.content_type.starts_with("image/jpeg")
        || req.content_type.starts_with("image/webp");
    let (raw_name, bytes, meta) = if is_raw_image {
        let name = query_param(query, "filename").unwrap_or("card-art.png").to_string();
        (name, req.body.clone(), json!({ "source": "upload" }))
    } else {
        let body: Value = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(_) => {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "ok": false, "error": "invalid JSON body for asset upload" }),
                );
                return;
            }
        };
        let raw_name = body
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let b64 = body
            .get("data_base64")
            .or_else(|| body.get("data_url"))
            .or_else(|| body.get("base64"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let decoded = match decode_base64(b64) {
            Ok(d) => d,
            Err(e) => {
                respond_json(stream, "400 Bad Request", &json!({ "ok": false, "error": e }));
                return;
            }
        };
        (raw_name, decoded, body)
    };

    let safe_name = match validate_asset_filename(&raw_name) {
        Ok(s) => s,
        Err(e) => {
            respond_json(stream, "400 Bad Request", &json!({ "ok": false, "error": e }));
            return;
        }
    };

    match store_uploaded_asset(shared, &safe_name, &bytes, meta) {
        Ok(record) => respond_json(
            stream,
            "201 Created",
            &json!({ "ok": true, "asset": record }),
        ),
        Err(e) => respond_json(
            stream,
            "400 Bad Request",
            &json!({ "ok": false, "error": e }),
        ),
    }
}

fn handle_assets_generate(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "invalid JSON body" }),
            );
            return;
        }
    };
    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Apex Vanguard")
        .to_string();
    let element = body
        .get("element")
        .and_then(|v| v.as_str())
        .unwrap_or("Flame");
    let rarity = body
        .get("rarity")
        .and_then(|v| v.as_str())
        .unwrap_or("Mythic");
    let prompt = body
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            format!(
                "High-craft modern anime cel-shaded trading card illustration (3:4 portrait), dynamic heroic pose, bold inked contours, glowing {element} elemental aura, dramatic rim lighting, original stylized homage character '{title}' ({rarity} tier), no text, no watermark, no borders."
            )
        });

    let cfg = shared.config.lock().unwrap().clone();
    let provider = body
        .get("provider")
        .and_then(|v| v.as_str())
        .or(cfg.image_provider.as_deref())
        .unwrap_or("auto");
    let gemini_key = cfg
        .gemini_api_key
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    let openai_key = cfg
        .openai_image_api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()));

    let use_gemini = matches!(provider, "gemini" | "auto") && gemini_key.is_some();
    let use_openai = !use_gemini && matches!(provider, "openai" | "auto") && openai_key.is_some();

    if !use_gemini && !use_openai {
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({
                "ok": false,
                "error": "No image generation API key configured. Add a Google Gemini API key (Nano Banana) or OpenAI API key in Settings (or set GEMINI_API_KEY / OPENAI_API_KEY), or upload card artwork directly.",
                "homage_prompt": prompt,
                "setup_hint": "Settings -> Image Generation Key (Gemini Nano Banana / OpenAI) or POST /api/assets/upload",
            }),
        );
        return;
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .build();
    let img_res: Result<(Vec<u8>, &'static str), String> = if use_gemini {
        let key = gemini_key.unwrap();
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent?key={key}"
        );
        let req_body = json!({
            "contents": [{ "parts": [{ "text": prompt }] }],
            "generationConfig": { "responseModalities": ["IMAGE"] }
        });
        match agent.post(&url).send_json(req_body) {
            Ok(resp) => match resp.into_json::<Value>() {
                Ok(val) => {
                    let b64 = val
                        .pointer("/candidates/0/content/parts")
                        .and_then(|p| p.as_array())
                        .and_then(|arr| {
                            arr.iter().find_map(|item| {
                                item.pointer("/inlineData/data")
                                    .or_else(|| item.pointer("/inline_data/data"))
                                    .and_then(|d| d.as_str())
                            })
                        });
                    match b64 {
                        Some(s) => decode_base64(s).map(|b| (b, "gemini-api")),
                        None => Err("Gemini Image API returned no inline image bytes".to_string()),
                    }
                }
                Err(e) => Err(format!("could not parse Gemini response: {e}")),
            },
            Err(e) => Err(format!("Gemini Image API request failed: {e}")),
        }
    } else {
        let key = openai_key.unwrap();
        let req_body = json!({
            "model": "gpt-image-1",
            "prompt": prompt,
            "size": "1024x1536",
            "response_format": "b64_json"
        });
        match agent
            .post("https://api.openai.com/v1/images/generations")
            .set("Authorization", &format!("Bearer {key}"))
            .send_json(req_body)
        {
            Ok(resp) => match resp.into_json::<Value>() {
                Ok(val) => match val.pointer("/data/0/b64_json").and_then(|d| d.as_str()) {
                    Some(s) => decode_base64(s).map(|b| (b, "openai-image")),
                    None => Err("OpenAI Images API returned no b64_json data".to_string()),
                },
                Err(e) => Err(format!("could not parse OpenAI Images response: {e}")),
            },
            Err(e) => Err(format!("OpenAI Images API request failed: {e}")),
        }
    };

    match img_res {
        Ok((bytes, source_tag)) => {
            let ext = detect_image_mime(&bytes).map(|(_, e)| e).unwrap_or("png");
            let slug = slugify_filename(&title);
            let safe_name = format!("{slug}.{ext}");
            let mut meta = body;
            meta["source"] = json!(source_tag);
            meta["prompt"] = json!(prompt);
            match store_uploaded_asset(shared, &safe_name, &bytes, meta) {
                Ok(record) => respond_json(
                    stream,
                    "201 Created",
                    &json!({ "ok": true, "asset": record }),
                ),
                Err(e) => respond_json(
                    stream,
                    "502 Bad Gateway",
                    &json!({ "ok": false, "error": e }),
                ),
            }
        }
        Err(e) => respond_json(
            stream,
            "502 Bad Gateway",
            &json!({ "ok": false, "error": e, "homage_prompt": prompt }),
        ),
    }
}

/// The single-page UI (assets/index.html); values are substituted in render_page.
const INDEX_HTML: &str = include_str!("../assets/index.html");

/// Escape a value interpolated into the page (the game name comes from
/// the operator's environment; unescaped, it was a straight HTML
/// injection into <title> and the sidebar).
fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_page(game_name: &str, studio_connected: bool, model_configured: bool) -> String {
    render_page_with(game_name, studio_connected, model_configured, &[])
}

/// The page, with the names of any saved files that were set aside at
/// startup (each gets a calm, dismissible banner).
pub fn render_page_with(
    game_name: &str,
    studio_connected: bool,
    model_configured: bool,
    recovered: &[String],
) -> String {
    let mode = if studio_connected { "studio_mcp" } else { "disconnected" };
    render_page_full(game_name, studio_connected, model_configured, recovered, mode)
}

pub fn render_page_full(
    game_name: &str,
    studio_connected: bool,
    model_configured: bool,
    recovered: &[String],
    workspace_mode: &str,
) -> String {
    let chip = match workspace_mode {
        "studio_mcp" if studio_connected => "Studio connected",
        "local_place" if studio_connected => "Local Place ready",
        _ => "Studio not connected",
    };
    let chip_class = if studio_connected { "on" } else { "off" };
    let mut banners = String::new();
    if !model_configured {
        banners.push_str("<div class=\"banner\" id=\"no-key-banner\"><span><strong>Chat is off: no model key configured.</strong> Open Settings to pick a local gateway or add an API key, or set OPENRO_MODEL_API_KEY.</span></div>");
    }
    if !studio_connected {
        banners.push_str("<div class=\"banner\" id=\"no-studio-banner\"><span><strong>Studio not connected.</strong> OpenRO cannot build until Roblox Studio or the Local Place Engine is connected. Open Settings or set OPENRO_MCP_COMMAND.</span></div>");
    }
    if !recovered.is_empty() {
        let names: Vec<String> = recovered.iter().map(|n| format!("<code>{}</code>", esc_html(n))).collect();
        banners.push_str(&format!(
            "<div class=\"banner\" id=\"recovered-banner\" role=\"status\"><span><strong>Some saved data could not be read.</strong> It was set aside as {} in your data folder, and OpenRO started fresh with it. The original file is kept there.</span><button class=\"btn btn-quiet\" type=\"button\" data-dismiss=\"recovered-banner\">Dismiss</button></div>",
            names.join(", ")
        ));
    }
    let can_build = model_configured && studio_connected;
    INDEX_HTML
        .replace("__KEY_BANNER__", &banners)
        .replace("__CHIP_CLASS__", chip_class)
        .replace("__CHIP__", chip)
        .replace("__CAN_BUILD__", &can_build.to_string())
        .replace("__STUDIO_CONNECTED__", &studio_connected.to_string())
        .replace("__WORKSPACE_MODE__", workspace_mode)
        .replace("__GAME_NAME__", &esc_html(game_name))
}

/* hallmark-rune override log — OpenRO elevation (18 Nov 2026)
   override · R9 · guard: the chat list is the primary navigation between a game's build sessions and is used constantly; the phone face collapses the rail to a top bar with a horizontal chat row in this same pass · reason: in OpenRO the rail is the session list, not chrome — the BAR's DIRECTION v1 fixes this shell and familiarity outranks novelty
   override · R1 (borderline) · guard: thread step lines render the build ledger — data returned by the API (skills run, scripts written, verdicts) · reason: mono carries the ledger because the ledger is data; UI labels, pitches and prose stay in the body face
*/
