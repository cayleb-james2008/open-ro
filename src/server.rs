//! Hand-rolled minimal HTTP over `std::net::TcpListener` serving the
//! embedded web UI (plain HTML/CSS/JS string constants — no framework,
//! no build step) plus the JSON API the UI and tests use.
//!
//! Chats are multiple and persistent: when the server is built with a
//! data dir (`Shared::with_data_dir`), every chat and its thread lines
//! are saved to `chats.json` there on every change and reloaded at
//! start, so a restart loses nothing.

use crate::agent::{Agent, AskMode, AskOptions};
use crate::config::RuntimeConfig;
use crate::mcp::McpClient;
use crate::model::{probe_local_gateways, ModelClient, AGY_BASE_URL, AGY_DEFAULT_MODEL, DEFAULT_BASE_URL, DEFAULT_MODEL_ID};
use crate::place::{LocalPlace, PlaceBackend};
use crate::snapshots::{
    compute_diff, compute_file_diffs, merge_selected_tree, parse_rbxlx_xml, ProposalStore,
    ScriptFileDiff, SnapshotStore,
};
use crate::sse::BuildEventHub;
use crate::studio_plugin::StudioPluginBridge;
use crate::studio_stream::{StudioActionInput, StudioActionKind, StudioStreamHub};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

mod ui_chunks {
    include!(concat!(env!("OUT_DIR"), "/ui_chunks.rs"));
}

#[derive(Clone)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
}

static CHAT_COUNTER: AtomicU64 = AtomicU64::new(0);
type CachedPlaceBytes = Mutex<Option<(Option<u64>, Arc<Vec<u8>>)>>;

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
    /// Chats whose undone change-set stack is non-empty (Redo has something to do).
    pub redoable: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Persistent Model + Studio settings (`data_dir/config.json`).
    pub config: Mutex<RuntimeConfig>,
    /// Versioned place snapshots & build diffs (`data_dir/snapshots.json`).
    pub snapshots: Mutex<SnapshotStore>,
    /// Live SSE build-progress event hub (`/api/build/stream`).
    pub events: Arc<BuildEventHub>,
    /// Live SSE realtime Studio 3D action & MCP frame hub (`/api/studio/stream`).
    pub studio_stream: Arc<StudioStreamHub>,
    /// Cached lightweight status so concurrent polling never stalls behind an active build.
    status_cache: Mutex<StatusInfo>,
    /// Cached latest place tree bytes keyed by `local_place_revision` so `GET /api/place-tree`
    /// never clones a `Value` DOM or stalls behind an active build (#31).
    cached_place_tree_bytes: CachedPlaceBytes,
    /// Cached serialized bytes of `GET /api/luau/check` keyed by `local_place_revision`.
    cached_luau_report: Mutex<Option<(u64, Arc<Vec<u8>>)>>,
    /// Game art assets metadata & in-memory blob fallback (`data_dir/game-assets/`).
    pub game_assets: Mutex<GameAssetsIndex>,
    /// Directly embedded Roblox Studio host lifecycle manager (`src/studio_embed.rs`).
    pub studio_embed: Mutex<crate::studio_embed::StudioEmbedManager>,
    /// Bounded 80-entry Studio CUA activity log + `cua_paused` human-takeover lock.
    pub cua_log: Arc<crate::studio_embed::StudioCuaLog>,
    /// Built-in Developer Skill & Connection Library manager (`data_dir/library.json`).
    pub library: Mutex<crate::library::LibraryManager>,
    /// Live Roblox Market Taste & 8-Dimension Craft Dossier store (`data_dir/market_taste_cache.json`).
    pub market_taste: Mutex<crate::market_taste::MarketTasteStore>,
    /// Optional 8-dimension Taste Dossier markdown from the latest `/api/market-taste/research` run.
    last_researched_dossier_prompt: Mutex<String>,
    /// Root directory for library/mesh/audio assets (equals `data_dir` or an ephemeral dir).
    pub assets_root: PathBuf,
    /// Workspace project list (`data_dir/projects.json`).
    pub projects: Mutex<crate::projects::ProjectStore>,
    /// Short-TTL cache for host Studio detection (`flatpak ps` / PATH scan) so
    /// concurrent `GET /api/studio/embed/status` polls never fork subprocess storms.
    cached_embed_detect: Mutex<Option<(std::time::Instant, crate::studio_embed::HostStudioDetection)>>,
    /// Short-TTL cache for `GET /api/library` (`blender --version` / host detection)
    /// invalidated immediately on any pack mutation.
    cached_library_list: Mutex<Option<(std::time::Instant, Value)>>,
    /// Staged RoPilot diff proposals (`data_dir/proposals.json`).
    pub proposals: Mutex<ProposalStore>,
    /// Lightweight Companion Roblox Studio Plugin long-poll bridge (`OpenRoCompanion.lua`).
    pub studio_plugin: Arc<StudioPluginBridge>,
    /// Multi-agent Teamwork coordinator, board, scheduler, and Studio lease.
    pub teamwork: Arc<crate::teamwork::TeamworkManager>,
    /// Active loopback listen port recorded when `serve()` binds.
    pub bound_port: AtomicU16,
    /// Persistent FIFO Run Queue + Plan->Execute checkpoints (`data_dir/runs/`).
    pub runs: Arc<crate::agent::runs::RunStore>,
    runs_worker_started: std::sync::atomic::AtomicBool,
    /// Studio / LocalPlace ScreenGui Designer store (`data_dir/ui/*.screen.json` or in-memory).
    pub gui_store: Mutex<crate::gui_designer::LocalGuiStore>,
}

#[derive(Clone, Default)]
pub struct GameAssetsIndex {
    pub mem_assets: Vec<crate::assets::AssetMetadata>,
    pub mem_blobs: std::collections::HashMap<String, Vec<u8>>,
}

static EPHEMERAL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn make_ephemeral_assets_root() -> PathBuf {
    let seq = EPHEMERAL_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("openro-mem-{}-{seq}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

impl Shared {
    /// In-memory server (tests, ephemeral runs): chats are not saved.
    pub fn new(agent: Option<Agent>, game_name: String) -> Arc<Shared> {
        let mut agent = agent;
        let updater = crate::update::Updater::from_env("OPENRO", "openro-rs", env!("CARGO_PKG_VERSION"), "cayleb-james2008/open-ro", true);
        let undoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let redoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let events = Arc::new(BuildEventHub::new());
        let initial_mode = agent.as_ref().map(|a| a.studio_mode_tag()).unwrap_or("local_place");
        let studio_stream = Arc::new(StudioStreamHub::new(initial_mode));
        let assets_root = make_ephemeral_assets_root();
        let library = crate::library::LibraryManager::open(&assets_root);
        let market_taste = crate::market_taste::MarketTasteStore::open(&assets_root);
        let studio_plugin = StudioPluginBridge::open(None);
        let lib_prompt = library.active_skill_prompt();
        if let Some(a) = agent.as_mut() {
            a.attach_undo_sink(Arc::clone(&undoable));
            a.attach_redo_sink(Arc::clone(&redoable));
            a.attach_events(Arc::clone(&events));
            a.attach_studio_stream(Arc::clone(&studio_stream));
            a.set_library_prompt(lib_prompt);
            a.set_assets_root(assets_root.clone());
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
            redoable,
            updater,
            busy: AtomicUsize::new(0),
            config: Mutex::new(RuntimeConfig::default()),
            snapshots: Mutex::new(SnapshotStore::in_memory()),
            events,
            studio_stream: Arc::clone(&studio_stream),
            status_cache: Mutex::new(initial_status),
            cached_place_tree_bytes: Mutex::new(None),
            cached_luau_report: Mutex::new(None),
            game_assets: Mutex::new(GameAssetsIndex::default()),
            studio_embed: Mutex::new(crate::studio_embed::StudioEmbedManager::new()),
            cua_log: Arc::new(crate::studio_embed::StudioCuaLog::new(Some(Arc::clone(&studio_stream)))),
            library: Mutex::new(library),
            market_taste: Mutex::new(market_taste),
            last_researched_dossier_prompt: Mutex::new(String::new()),
            assets_root,
            projects: Mutex::new(crate::projects::ProjectStore::load(None)),
            cached_embed_detect: Mutex::new(None),
            cached_library_list: Mutex::new(None),
            proposals: Mutex::new(ProposalStore::in_memory()),
            studio_plugin,
            teamwork: crate::teamwork::TeamworkManager::in_memory(),
            bound_port: AtomicU16::new(4040),
            runs: crate::agent::runs::RunStore::in_memory(),
            runs_worker_started: std::sync::atomic::AtomicBool::new(false),
            gui_store: Mutex::new(crate::gui_designer::LocalGuiStore::in_memory()),
        })
    }

    /// Persistent server: chats load from (and save to) `chats.json`
    /// inside `data_dir`.
    pub fn with_data_dir(agent: Option<Agent>, game_name: String, data_dir: PathBuf) -> Arc<Shared> {
        let _ = std::fs::create_dir_all(&data_dir);
        let updater = crate::update::Updater::from_env("OPENRO", "openro-rs", env!("CARGO_PKG_VERSION"), "cayleb-james2008/open-ro", true);
        let mut agent = agent;
        let undoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let redoable = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let events = Arc::new(BuildEventHub::new());
        let initial_mode = agent.as_ref().map(|a| a.studio_mode_tag()).unwrap_or("local_place");
        let studio_stream = Arc::new(StudioStreamHub::new(initial_mode));
        let library = crate::library::LibraryManager::open(&data_dir);
        let market_taste = crate::market_taste::MarketTasteStore::open(&data_dir);
        let studio_plugin = StudioPluginBridge::open(Some(&data_dir));
        let lib_prompt = library.active_skill_prompt();
        if let Some(a) = agent.as_mut() {
            a.attach_undo_sink(Arc::clone(&undoable));
            a.attach_redo_sink(Arc::clone(&redoable));
            a.attach_events(Arc::clone(&events));
            a.attach_studio_stream(Arc::clone(&studio_stream));
            a.set_library_prompt(lib_prompt);
            a.set_assets_root(data_dir.clone());
        }
        let initial_status = extract_status_from_agent(agent.as_mut());
        let mut recovered = agent.as_ref().map(|a| a.recovered.clone()).unwrap_or_default();
        let (chats, active) = load_chats(&data_dir, &mut recovered).unwrap_or_else(|| {
            let chat = default_chat();
            let active = chat.id.clone();
            (vec![chat], active)
        });
        let ideas = load_ideas(&data_dir, &mut recovered);
        let mut cfg = RuntimeConfig::load(&data_dir);
        migrate_config_secrets(&data_dir, &mut cfg);
        let mut snap_store = SnapshotStore::open(&data_dir);
        recovered.append(&mut snap_store.recovered);
        let mut proposal_store = ProposalStore::open(&data_dir);
        recovered.append(&mut proposal_store.recovered);
        // Ensure persistent game-assets directory is initialized on startup.
        let _ = crate::assets::AssetManager::open(&data_dir);
        let runs = crate::agent::runs::RunStore::open(&data_dir);
        let assets_root = data_dir.clone();
        let projects = Mutex::new(crate::projects::ProjectStore::load(Some(&data_dir)));
        let teamwork = crate::teamwork::TeamworkManager::open(data_dir.clone());
        let gui_store = Mutex::new(crate::gui_designer::LocalGuiStore::open(Some(&data_dir)));
        let shared = Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(chats),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: Some(data_dir),
            ideas: Mutex::new(ideas),
            store_recovered: recovered,
            active_handlers: AtomicUsize::new(0),
            undoable,
            redoable,
            updater,
            busy: AtomicUsize::new(0),
            config: Mutex::new(cfg),
            snapshots: Mutex::new(snap_store),
            events,
            studio_stream: Arc::clone(&studio_stream),
            status_cache: Mutex::new(initial_status),
            cached_place_tree_bytes: Mutex::new(None),
            cached_luau_report: Mutex::new(None),
            game_assets: Mutex::new(GameAssetsIndex::default()),
            studio_embed: Mutex::new(crate::studio_embed::StudioEmbedManager::new()),
            cua_log: Arc::new(crate::studio_embed::StudioCuaLog::new(Some(Arc::clone(&studio_stream)))),
            library: Mutex::new(library),
            market_taste: Mutex::new(market_taste),
            last_researched_dossier_prompt: Mutex::new(String::new()),
            assets_root,
            projects,
            cached_embed_detect: Mutex::new(None),
            cached_library_list: Mutex::new(None),
            proposals: Mutex::new(proposal_store),
            studio_plugin,
            teamwork,
            bound_port: AtomicU16::new(4040),
            runs,
            runs_worker_started: std::sync::atomic::AtomicBool::new(false),
            gui_store,
        });
        shared.teamwork.resume_pending_runs(&shared);
        shared
    }

    pub fn active_project_dir(&self) -> Option<PathBuf> {
        let store = self.projects.lock().unwrap();
        let active = store.active.as_deref()?;
        store.find(active).map(|r| r.dir)
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
            .map(|i| {
                json!({
                    "id": i.id,
                    "title": i.title,
                    "pitch": i.pitch,
                    "skills": i.skills,
                    "niche_id": i.niche_id,
                    "feel_profile": i.feel_profile.as_ref().map(crate::ideas::FeelProfile::to_json),
                })
            })
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

const POOL_WORKERS_BOOT: usize = 2;
const POOL_WORKERS: usize = 32;

struct WorkerJob {
    stream: TcpStream,
    slot: HandlerSlot,
}

fn spawn_pool_worker(
    idx: usize,
    q: Arc<(Mutex<VecDeque<WorkerJob>>, Condvar)>,
    sh: Arc<Shared>,
    al: Arc<Vec<String>>,
    bw: Arc<AtomicUsize>,
) {
    let _ = std::thread::Builder::new()
        .name(format!("openro-http-{idx}"))
        .stack_size(512 * 1024)
        .spawn(move || loop {
            let job = {
                let (lock, cvar) = &*q;
                let mut guard = lock.lock().unwrap();
                while guard.is_empty() {
                    guard = cvar.wait(guard).unwrap();
                }
                guard.pop_front()
            };
            if let Some(job) = job {
                bw.fetch_add(1, Ordering::Relaxed);
                let _ = handle(job.stream, &sh, &al, job.slot);
                bw.fetch_sub(1, Ordering::Relaxed);
            }
        });
}

pub fn serve(listener: TcpListener, shared: Arc<Shared>) -> std::io::Result<()> {
    if let Ok(addr) = listener.local_addr() {
        shared.bound_port.store(addr.port(), Ordering::Relaxed);
    }
    // Extra host names this server answers to, beyond this machine's own.
    let allowed: Arc<Vec<String>> = Arc::new(
        std::env::var("OPENRO_ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect(),
    );
    let queue: Arc<(Mutex<VecDeque<WorkerJob>>, Condvar)> =
        Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
    let busy_workers = Arc::new(AtomicUsize::new(0));
    let mut spawned_workers = 0usize;
    for idx in 0..POOL_WORKERS_BOOT {
        spawn_pool_worker(
            idx,
            Arc::clone(&queue),
            Arc::clone(&shared),
            Arc::clone(&allowed),
            Arc::clone(&busy_workers),
        );
        spawned_workers += 1;
    }
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        if shared.active_handlers.load(Ordering::SeqCst) >= MAX_HANDLERS {
            // Answered off the accept loop: a silent client must not
            // stall the connections queued behind it.
            std::thread::spawn(move || {
                respond_err(&mut stream, "503 Service Unavailable", "OpenRO is busy with other requests. Try again in a moment.");
                let _ = stream.read(&mut [0u8; 8192]);
            });
            continue;
        }
        shared.active_handlers.fetch_add(1, Ordering::SeqCst);
        let slot = HandlerSlot(Arc::clone(&shared));
        // Reuse persistent pool workers and grow lazily on demand up to POOL_WORKERS
        // so cold boot avoids a 40-thread spawn storm while concurrent bursts stay saturated.
        let busy = busy_workers.load(Ordering::Relaxed);
        if busy >= POOL_WORKERS {
            let sh = Arc::clone(&shared);
            let al = Arc::clone(&allowed);
            std::thread::spawn(move || {
                let _ = handle(stream, &sh, &al, slot);
            });
        } else {
            let queued = {
                let (lock, cvar) = &*queue;
                let mut q = lock.lock().unwrap();
                q.push_back(WorkerJob { stream, slot });
                let len = q.len();
                cvar.notify_one();
                len
            };
            if spawned_workers < POOL_WORKERS && busy + queued > spawned_workers {
                spawn_pool_worker(
                    spawned_workers,
                    Arc::clone(&queue),
                    Arc::clone(&shared),
                    Arc::clone(&allowed),
                    Arc::clone(&busy_workers),
                );
                spawned_workers += 1;
            }
        }
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
    accept_encoding: String,
    if_none_match: String,
    content_length: usize,
    last_event_id: u64,
    plugin_token: String,
    duplicate_host: bool,
    bad_content_length: bool,
    chunked_transfer: bool,
    keep_alive: bool,
    has_pipelined: bool,
}

thread_local! {
    static TL_KEEP_ALIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn contains_ascii_icase(haystack: &str, needle: &[u8]) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut chunk = [0u8; 4096];
    let mut total = stream.read(&mut chunk).ok()?;
    if total == 0 {
        return None;
    }
    if let Some(pos) = find_subslice(&chunk[..total], b"\r\n\r\n") {
        return parse_buffered_request(stream, &chunk[..total], pos + 4);
    }
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    buf.extend_from_slice(&chunk[..total]);
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        total = buf.len();
        if let Some(pos) = find_subslice(&buf[..total], b"\r\n\r\n") {
            return parse_buffered_request(stream, &buf[..total], pos + 4);
        }
        if total > 64 * 1024 {
            return None;
        }
    }
}

fn parse_buffered_request(
    stream: &mut TcpStream,
    buf: &[u8],
    header_end: usize,
) -> Option<Request> {
    let head_cow = match std::str::from_utf8(&buf[..header_end]) {
        Ok(s) => std::borrow::Cow::Borrowed(s),
        Err(_) => String::from_utf8_lossy(&buf[..header_end]),
    };
    let mut lines = head_cow.lines();
    let first = lines.next()?;
    let is_http11 = first.ends_with("HTTP/1.1");
    let mut parts = first.split_whitespace();
    let mut req = Request {
        method: parts.next()?.to_string(),
        path: parts.next()?.to_string(),
        body: Vec::new(),
        host: String::new(),
        origin: String::new(),
        sec_fetch_site: String::new(),
        content_type: String::new(),
        accept_encoding: String::new(),
        if_none_match: String::new(),
        content_length: 0,
        last_event_id: 0,
        plugin_token: String::new(),
        duplicate_host: false,
        bad_content_length: false,
        chunked_transfer: false,
        keep_alive: false,
        has_pipelined: false,
    };
    let mut seen_host = false;
    let mut seen_cl: Option<usize> = None;
    let mut want_keep_alive = false;
    let mut force_close = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            let v = v.trim();
            if k.eq_ignore_ascii_case("content-length") {
                match v.parse::<usize>() {
                    Ok(n) => {
                        if seen_cl.is_some_and(|prev| prev != n) {
                            req.bad_content_length = true;
                        }
                        seen_cl = Some(n);
                        req.content_length = n;
                    }
                    Err(_) => req.bad_content_length = true,
                }
            } else if k.eq_ignore_ascii_case("transfer-encoding") {
                if contains_ascii_icase(v, b"chunked") {
                    req.chunked_transfer = true;
                }
            } else if k.eq_ignore_ascii_case("connection") {
                if contains_ascii_icase(v, b"close") {
                    force_close = true;
                } else if contains_ascii_icase(v, b"keep-alive") {
                    want_keep_alive = true;
                }
            } else if k.eq_ignore_ascii_case("host") {
                if seen_host {
                    req.duplicate_host = true;
                }
                seen_host = true;
                req.host = v.to_string();
            } else if k.eq_ignore_ascii_case("origin") {
                req.origin = v.to_string();
            } else if k.eq_ignore_ascii_case("sec-fetch-site") {
                req.sec_fetch_site = v.to_ascii_lowercase();
            } else if k.eq_ignore_ascii_case("content-type") {
                req.content_type = v.to_ascii_lowercase();
            } else if k.eq_ignore_ascii_case("accept-encoding") {
                req.accept_encoding = v.to_ascii_lowercase();
            } else if k.eq_ignore_ascii_case("if-none-match") {
                req.if_none_match = v.to_string();
            } else if k.eq_ignore_ascii_case("last-event-id") {
                req.last_event_id = v.parse().unwrap_or(0);
            } else if k.eq_ignore_ascii_case("x-openro-plugin-token") {
                req.plugin_token = v.to_string();
            }
        }
    }
    req.keep_alive = is_http11 && want_keep_alive && !force_close;
    if req.duplicate_host || req.bad_content_length || req.chunked_transfer {
        req.keep_alive = false;
        return Some(req);
    }
    let body_cap = max_body_for_path(&req.path);
    // An oversized body is not read at all: handle answers 413.
    if req.content_length > body_cap {
        req.keep_alive = false;
        return Some(req);
    }
    let leftover = &buf[header_end..];
    if req.content_length == 0 {
        if !leftover.is_empty() {
            req.has_pipelined = true;
            req.keep_alive = false;
        }
        return Some(req);
    }
    let mut body = Vec::with_capacity(req.content_length.min(1024 * 1024));
    body.extend_from_slice(leftover);
    if body.len() < req.content_length {
        let mut body_chunk = [0u8; 4096];
        while body.len() < req.content_length {
            let n = stream.read(&mut body_chunk).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&body_chunk[..n]);
        }
    }
    if body.len() > req.content_length {
        req.has_pipelined = true;
        req.keep_alive = false;
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

#[inline]
fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let name = host_name(host);
    ["localhost", "127.0.0.1", "[::1]"].contains(&name.as_str())
        || allowed.iter().any(|a| host_name(a) == name)
}

fn requires_loopback_host(method: &str, clean_path: &str) -> bool {
    clean_path == "/api/opencloud"
        || clean_path.starts_with("/api/opencloud/")
        || clean_path == "/api/creator-hub"
        || clean_path.starts_with("/api/creator-hub/")
        || clean_path == "/api/studio/mcp"
        || clean_path.starts_with("/api/studio/mcp/")
        || clean_path == "/api/studio/cua"
        || clean_path.starts_with("/api/studio/cua/")
        || clean_path == "/api/projects"
        || clean_path.starts_with("/api/projects/")
        || clean_path == "/api/art"
        || clean_path.starts_with("/api/art/")
        || clean_path == "/api/library/sfx-packs"
        || clean_path.starts_with("/api/library/sfx-packs/")
        || clean_path == "/api/library/audio-runner/install"
        || clean_path == "/api/library/music"
        || clean_path.starts_with("/api/library/music/")
        || clean_path == "/api/library/music-runner/install"
        || clean_path == "/api/tools/image/process"
        || clean_path == "/api/monetization/pack"
        || clean_path == "/api/studio/monetization-audit"
        || clean_path == "/api/studio/economy-sim"
        || clean_path == "/api/studio/gui"
        || clean_path.starts_with("/api/studio/gui/")
        || clean_path == "/api/studio/ui-kit/generate"
        || clean_path == "/api/assets/upload"
        || clean_path == "/api/assets/generate"
        || (method == "DELETE" && clean_path.starts_with("/api/assets/"))
        || (matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
            && (clean_path.starts_with("/api/studio/embed/")
                || clean_path.starts_with("/api/studio/input/")
                || clean_path.starts_with("/api/studio/tools/")
                || clean_path == "/api/studio/playtest"
                || clean_path == "/api/studio/playtest/fix-all"
                || clean_path == "/api/studio/sandbox/run"
                || clean_path == "/api/studio/economy/simulate"
                || clean_path == "/api/studio/dependencies"
                || clean_path == "/api/studio/dependencies/prune"
                || clean_path == "/api/studio/snapshots/hook-integrity"
                || clean_path == "/api/studio/sync-conflicts"
                || clean_path == "/api/studio/sync-conflicts/resolve"
                || clean_path == "/api/studio/publish-guard"
                || clean_path == "/api/studio/publish-guard/autofix"
                || clean_path == "/api/studio/monetization-audit"
                || (clean_path.starts_with("/api/snapshots/") && clean_path.ends_with("/restore-hooks"))
                || clean_path == "/api/studio/capture"
                || clean_path == "/api/studio/launch-and-continue"))
}

/// Why a request is refused before it reaches a handler, as (status,
/// plain-words message); None when it may proceed. A foreign Host is
/// refused outright (a rebinding page cannot read the API); a
/// state-changing request is refused when it is cross-site, when its
/// Origin is not this server's own authority, or when its body is not
/// JSON (a plain form post from another page cannot send one).
fn refusal(req: &Request, allowed: &[String]) -> Option<(&'static str, &'static str)> {
    if req.duplicate_host {
        return Some(("400 Bad Request", "Duplicate Host header is refused."));
    }
    if req.bad_content_length {
        return Some(("400 Bad Request", "Invalid Content-Length header."));
    }
    if req.chunked_transfer {
        return Some(("501 Not Implemented", "Chunked transfer encoding is not supported."));
    }
    if !host_allowed(&req.host, allowed) {
        return Some(("403 Forbidden", "This OpenRO server only answers requests addressed to this machine. Open it at http://127.0.0.1 instead."));
    }
    let clean_path = req.path.split('?').next().unwrap_or(&req.path);
    if requires_loopback_host(&req.method, clean_path) && !host_allowed(&req.host, &[]) {
        return Some(("403 Forbidden", "This action is only available from this machine (loopback)."));
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

fn conn_header_line(status: &str) -> &'static str {
    if TL_KEEP_ALIVE.get() && (status.starts_with("200") || status.starts_with("304")) {
        "Connection: keep-alive\r\nKeep-Alive: timeout=2, max=1000\r\n\r\n"
    } else {
        TL_KEEP_ALIVE.set(false);
        "Connection: close\r\n\r\n"
    }
}

#[inline]
fn write_all_vectored(stream: &mut TcpStream, header: &[u8], body: &[u8]) {
    if body.is_empty() {
        let _ = stream.write_all(header);
        return;
    }
    let mut slices = [std::io::IoSlice::new(header), std::io::IoSlice::new(body)];
    let mut idx = 0usize;
    let mut off = 0usize;
    while idx < slices.len() {
        let slice_slice = &mut slices[idx..];
        match stream.write_vectored(slice_slice) {
            Ok(0) => break,
            Ok(mut n) => {
                while idx < slices.len() {
                    let rem = slices[idx].len() - off;
                    if n < rem {
                        off += n;
                        let new_slice = if idx == 0 {
                            std::io::IoSlice::new(&header[off..])
                        } else {
                            std::io::IoSlice::new(&body[off..])
                        };
                        slices[idx] = new_slice;
                        break;
                    }
                    n -= rem;
                    idx += 1;
                    off = 0;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                let _ = stream.write_all(header);
                let _ = stream.write_all(body);
                break;
            }
        }
    }
}

#[inline]
fn push_usize(hdr: &mut String, mut n: usize) {
    let mut digits = [0u8; 20];
    let mut i = 20;
    if n == 0 {
        hdr.push('0');
        return;
    }
    while n > 0 {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = std::str::from_utf8(&digits[i..]) {
        hdr.push_str(s);
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let conn = conn_header_line(status);
    let mut hdr = String::with_capacity(224);
    hdr.push_str("HTTP/1.1 ");
    hdr.push_str(status);
    hdr.push_str("\r\nContent-Type: ");
    hdr.push_str(content_type);
    hdr.push_str("\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Length: ");
    push_usize(&mut hdr, body.len());
    hdr.push_str("\r\n");
    hdr.push_str(conn);
    write_all_vectored(stream, hdr.as_bytes(), body.as_bytes());
}

fn respond_bytes(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    cache_control: &str,
    body: &[u8],
) {
    respond_bytes_with_headers(stream, status, content_type, cache_control, "", body);
}

fn respond_bytes_with_headers(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    cache_control: &str,
    extra_headers: &str,
    body: &[u8],
) {
    let conn = conn_header_line(status);
    let mut hdr = String::with_capacity(256 + extra_headers.len());
    hdr.push_str("HTTP/1.1 ");
    hdr.push_str(status);
    hdr.push_str("\r\nContent-Type: ");
    hdr.push_str(content_type);
    hdr.push_str("\r\nCache-Control: ");
    hdr.push_str(cache_control);
    hdr.push_str("\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n");
    hdr.push_str(extra_headers);
    hdr.push_str("Content-Length: ");
    push_usize(&mut hdr, body.len());
    hdr.push_str("\r\n");
    hdr.push_str(conn);
    write_all_vectored(stream, hdr.as_bytes(), body);
}

fn respond_download(stream: &mut TcpStream, content_type: &str, filename: &str, body: &str) {
    respond_download_bytes(stream, content_type, filename, body.as_bytes());
}

fn respond_download_bytes(stream: &mut TcpStream, content_type: &str, filename: &str, body: &[u8]) {
    TL_KEEP_ALIVE.set(false);
    let mut hdr = String::with_capacity(256 + filename.len());
    hdr.push_str("HTTP/1.1 200 OK\r\nContent-Type: ");
    hdr.push_str(content_type);
    hdr.push_str("\r\nContent-Disposition: attachment; filename=\"");
    hdr.push_str(filename);
    hdr.push_str("\"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Length: ");
    push_usize(&mut hdr, body.len());
    hdr.push_str("\r\nConnection: close\r\n\r\n");
    write_all_vectored(stream, hdr.as_bytes(), body);
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
    if let Ok(bytes) = serde_json::to_vec(value) {
        let conn = conn_header_line(status);
        let mut hdr = String::with_capacity(224);
        hdr.push_str("HTTP/1.1 ");
        hdr.push_str(status);
        hdr.push_str("\r\nContent-Type: application/json\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Length: ");
        push_usize(&mut hdr, bytes.len());
        hdr.push_str("\r\n");
        hdr.push_str(conn);
        write_all_vectored(stream, hdr.as_bytes(), &bytes);
    } else {
        respond(stream, status, "application/json", &value.to_string());
    }
}

fn respond_err(stream: &mut TcpStream, status: &str, error: &str) {
    respond_json(stream, status, &json!({ "ok": false, "error": error }));
}

/// Resolve `(project_root, project_id)` from explicit request fields or `shared.projects`.
fn resolve_project_for_agent(
    shared: &Shared,
    req_project_id: Option<&str>,
    req_project_root: Option<&str>,
) -> (Option<PathBuf>, Option<String>) {
    if let Some(pr) = req_project_root.filter(|s| !s.trim().is_empty()) {
        return (Some(PathBuf::from(pr.trim())), req_project_id.map(String::from));
    }
    let store = shared.projects.lock().unwrap();
    if let Some(id) = req_project_id.filter(|s| !s.trim().is_empty()) {
        if let Some(r) = store.find(id.trim()) {
            return (Some(r.dir), Some(r.id));
        }
    }
    if let Some(active) = store.active.as_deref() {
        if let Some(r) = store.find(active) {
            return (Some(r.dir), Some(r.id));
        }
    }
    (None, None)
}

fn invoke_opencloud_tool(
    shared: &Shared,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<Value, String> {
    let (clean_path, query) = path.split_once('?').unwrap_or((path, ""));
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let resolver = |id: &str| resolve_project_dir(shared, id);
    let ctx = crate::opencloud::Ctx {
        data_dir,
        store,
        resolve: &resolver,
        active_project: shared.active_project_dir(),
    };
    let reply = crate::opencloud::route(&ctx, method, clean_path, query, body)
        .ok_or_else(|| format!("asset endpoint '{method} {clean_path}' not available"))?;
    match reply {
        crate::opencloud::Reply::Json(code, val) => {
            let is_action_required =
                val.get("status").and_then(|v| v.as_str()) == Some("action_required");
            if code >= 400
                || (val.get("ok").and_then(|v| v.as_bool()) == Some(false) && !is_action_required)
            {
                let err = val
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("HTTP {code}: {val}"));
                Err(err)
            } else {
                Ok(val)
            }
        }
        crate::opencloud::Reply::Bytes(mime, bytes) => {
            Ok(json!({ "ok": true, "mime": mime, "bytes": bytes.len() }))
        }
    }
}

fn resolve_agent_project_id(shared: &Shared, args: &Value, tool: &str) -> Result<String, String> {
    args.get("project_id")
        .or_else(|| args.get("project"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| shared.projects.lock().unwrap().active.clone())
        .ok_or_else(|| format!("{tool} needs an active project or args.project_id"))
}

/// Bind the active project + real `openro-core` & `openro-assets` tool hooks onto `agent`.
fn sync_agent_project(
    shared: &Arc<Shared>,
    agent: &mut Agent,
    req_project_id: Option<&str>,
    req_project_root: Option<&str>,
) {
    let (root, id) = resolve_project_for_agent(shared, req_project_id, req_project_root);
    if root.is_some() {
        agent.set_project(root, id);
    }
    ensure_agent_hooks(shared, agent);
}

/// Wire all `openro-core`, `openro-assets`, and `studio_cua_*` hooks onto `agent`.
pub fn ensure_agent_hooks(shared: &Arc<Shared>, agent: &mut Agent) {
    if agent.hooks.project_list.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.project_list = Some(Arc::new(move |_args| {
            let store = srv.projects.lock().unwrap();
            let list: Vec<Value> = store.list().iter().map(crate::projects::summary).collect();
            Ok(json!({ "ok": true, "active": store.active, "projects": list }))
        }));
    }
    if agent.hooks.reference_search.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.reference_search = Some(Arc::new(move |args| {
            invoke_opencloud_tool(&srv, "POST", "/api/art/references/search", args)
        }));
    }
    if agent.hooks.art_generate.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.art_generate = Some(Arc::new(move |args| {
            invoke_opencloud_tool(&srv, "POST", "/api/art/generate", args)
        }));
    }
    if agent.hooks.image_process.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.image_process = Some(Arc::new(move |args| {
            invoke_opencloud_tool(&srv, "POST", "/api/art/process", args)
        }));
    }
    if agent.hooks.art_pack_card.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.art_pack_card = Some(Arc::new(move |args| {
            invoke_opencloud_tool(&srv, "POST", "/api/art/pack-card", args)
        }));
    }
    if agent.hooks.ui_kit_presets.is_none() {
        agent.hooks.ui_kit_presets = Some(Arc::new(|_args| Ok(crate::ui_kit::presets_json())));
    }
    if agent.hooks.ui_kit_generate.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.ui_kit_generate = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "ui_kit_generate")
                .unwrap_or_else(|_| String::new());
            if pid.is_empty() {
                let mut body = args.clone();
                let data_dir = srv
                    .data_dir
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("openro-data"));
                if let Some(obj) = body.as_object_mut() {
                    obj.entry("data_dir".to_string())
                        .or_insert_with(|| json!(data_dir.to_string_lossy()));
                }
                let studio_root = data_dir.join("ui-kit-studio");
                let _ = std::fs::create_dir_all(studio_root.join("art"));
                let store = crate::secrets::SecretStore::for_data_dir(data_dir);
                crate::ui_kit::generate_ui_kit(&studio_root, &store, None, &body).map_err(|oc| oc.msg)
            } else {
                invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/ui-kit/generate"), args)
            }
        }));
    }
    if agent.hooks.assets_upload.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.assets_upload = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "assets_upload")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/assets/upload"), args)
        }));
    }
    if agent.hooks.opencloud_status.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.opencloud_status = Some(Arc::new(move |args| {
            invoke_opencloud_tool(&srv, "GET", "/api/opencloud", args)
        }));
    }
    if agent.hooks.creator_hub_status.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.creator_hub_status = Some(Arc::new(move |args| {
            let q = crate::agent::creator_hub_status_query(args);
            let path = if q.is_empty() {
                "/api/creator-hub/status".to_string()
            } else {
                format!("/api/creator-hub/status?{q}")
            };
            invoke_opencloud_tool(&srv, "GET", &path, args)
        }));
    }
    if agent.hooks.creator_hub_setup.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.creator_hub_setup = Some(Arc::new(move |args| {
            let path = match resolve_agent_project_id(&srv, args, "creator_hub_setup") {
                Ok(pid) => format!("/api/projects/{pid}/creator-hub/setup"),
                Err(_) => "/api/creator-hub/setup".to_string(),
            };
            invoke_opencloud_tool(&srv, "POST", &path, args)
        }));
    }
    if agent.hooks.creator_hub_configure.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.creator_hub_configure = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "creator_hub_configure")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/creator-hub/configure"), args)
        }));
    }
    if agent.hooks.assets_authorise.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.assets_authorise = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "assets_authorise")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/assets/authorise"), args)
        }));
    }
    if agent.hooks.assets_codegen.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.assets_codegen = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "assets_codegen")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/assets/codegen"), args)
        }));
    }
    if agent.hooks.products_sync.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.products_sync = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "products_sync")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/products/sync"), args)
        }));
    }
    if agent.hooks.products_codegen.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.products_codegen = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "products_codegen")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/products/codegen"), args)
        }));
    }
    if agent.hooks.project_publish.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.project_publish = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "project_publish")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/publish"), args)
        }));
    }
    if agent.hooks.universe_get.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.universe_get = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "universe_get")?;
            invoke_opencloud_tool(&srv, "GET", &format!("/api/projects/{pid}/universe"), args)
        }));
    }
    if agent.hooks.universe_set.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.universe_set = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "universe_set")?;
            invoke_opencloud_tool(&srv, "POST", &format!("/api/projects/{pid}/universe"), args)
        }));
    }
    if agent.hooks.mesh_generate.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.mesh_generate = Some(Arc::new(move |args| {
            let proj = args
                .get("project")
                .and_then(|v| v.as_str())
                .and_then(|id| resolve_project_dir(&srv, id))
                .or_else(|| srv.active_project_dir());
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            let actx = crate::tool_adapters::InvokeCtx {
                data_dir,
                assets_root: &srv.assets_root,
                store: &store,
                project_dir: proj.as_deref(),
            };
            let explicit = args.get("adapter_id").and_then(|v| v.as_str());
            if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "mesh_gen", explicit, args) {
                return res;
            }
            if args.get("parts").is_some() || args.get("script").is_some() {
                crate::mesh::generate_for_project(&srv.assets_root, proj.as_deref(), args, None)
            } else {
                crate::library::generate_blender_mesh(
                    proj.as_deref().unwrap_or(&srv.assets_root),
                    args,
                    None,
                )
            }
        }));
    }
    if agent.hooks.audio_generate.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.audio_generate = Some(Arc::new(move |args| {
            run_audio_generate(&srv, args)
        }));
    }
    if agent.hooks.sfx_pack_list.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.sfx_pack_list = Some(Arc::new(move |_args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            Ok(crate::audio_library::list_sfx_packs(data_dir))
        }));
    }
    if agent.hooks.sfx_pack_install.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.sfx_pack_install = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let pack_id = args
                .get("pack_id")
                .or_else(|| args.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let url = args.get("url").and_then(|v| v.as_str());
            let res = crate::audio_library::install_sfx_pack(data_dir, pack_id, url)?;
            invalidate_library_cache(&srv);
            Ok(res)
        }));
    }
    if agent.hooks.sfx_pack_search.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.sfx_pack_search = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let q = args
                .get("query")
                .or_else(|| args.get("q"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pack_id = args.get("pack_id").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(40) as usize;
            Ok(crate::audio_library::search_sfx_packs(
                data_dir, q, pack_id, limit,
            ))
        }));
    }
    if agent.hooks.sfx_pack_use.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.sfx_pack_use = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let proj = args
                .get("project")
                .or_else(|| args.get("project_id"))
                .and_then(|v| v.as_str())
                .and_then(|id| resolve_project_dir(&srv, id))
                .or_else(|| srv.active_project_dir());
            crate::audio_library::use_sfx_clip(data_dir, proj.as_deref(), args)
        }));
    }
    if agent.hooks.music_search.is_none() {
        agent.hooks.music_search = Some(Arc::new(move |args| {
            let genre = args.get("genre").and_then(|v| v.as_str());
            let query = args
                .get("query")
                .or_else(|| args.get("q"))
                .and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            crate::audio_library::search_youtube_music(genre, query, limit)
        }));
    }
    if agent.hooks.music_import.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.music_import = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let proj = args
                .get("project")
                .or_else(|| args.get("project_id"))
                .and_then(|v| v.as_str())
                .and_then(|id| resolve_project_dir(&srv, id))
                .or_else(|| srv.active_project_dir());
            crate::audio_library::import_youtube_music(data_dir, proj.as_deref(), args)
        }));
    }
    if agent.hooks.adapter_list.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.adapter_list = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            let cap = args.get("capability").and_then(|v| v.as_str());
            Ok(crate::tool_adapters::AdapterRegistry::load(data_dir).list_json(&store, cap))
        }));
    }
    if agent.hooks.adapter_invoke.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.adapter_invoke = Some(Arc::new(move |args| {
            let data_dir = srv.data_dir.as_deref().unwrap_or(&srv.assets_root);
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            let proj = args
                .get("project")
                .or_else(|| args.get("project_id"))
                .and_then(|v| v.as_str())
                .and_then(|id| resolve_project_dir(&srv, id))
                .or_else(|| srv.active_project_dir());
            let cap = args.get("capability").and_then(|v| v.as_str());
            let id = args.get("adapter_id").and_then(|v| v.as_str());
            let params = args.get("params").unwrap_or(args);
            let ctx = crate::tool_adapters::InvokeCtx {
                data_dir,
                assets_root: &srv.assets_root,
                store: &store,
                project_dir: proj.as_deref(),
            };
            crate::tool_adapters::invoke(&ctx, cap, id, params)
        }));
    }
    if agent.hooks.studio_cua_screenshot.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_cua_screenshot = Some(Arc::new(move |args| {
            cua_exec_screenshot(&srv, "agent", args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.studio_cua_mouse.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_cua_mouse = Some(Arc::new(move |args| {
            cua_exec_mouse(&srv, "agent", args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.studio_cua_keyboard.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_cua_keyboard = Some(Arc::new(move |args| {
            cua_exec_keyboard(&srv, "agent", args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.studio_cua_window.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_cua_window = Some(Arc::new(move |args| {
            cua_exec_window(&srv, "agent", args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.studio_stage_content.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_stage_content = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "studio_stage_content")?;
            let r = srv
                .projects
                .lock()
                .unwrap()
                .find(&pid)
                .ok_or_else(|| format!("no project '{pid}'"))?;
            let slug = args
                .get("slug")
                .or_else(|| args.get("studio_project_slug"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(crate::studio_embed::sanitize_studio_content_slug)
                .unwrap_or_else(|| crate::studio_embed::default_studio_project_slug(&r.dir));
            crate::studio_embed::stage_project_studio_content(&r.dir, &slug)
        }));
    }
    if agent.hooks.studio_verify_device.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_verify_device = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "studio_verify_device")?;
            let r = srv
                .projects
                .lock()
                .unwrap()
                .find(&pid)
                .ok_or_else(|| format!("no project '{pid}'"))?;
            crate::playtest::verify_studio_device(&srv, &r, args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.studio_verify_audio.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.studio_verify_audio = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "studio_verify_audio")?;
            let r = srv
                .projects
                .lock()
                .unwrap()
                .find(&pid)
                .ok_or_else(|| format!("no project '{pid}'"))?;
            crate::playtest::verify_studio_audio(&srv, &r, args).map_err(|(_, e)| e)
        }));
    }
    if agent.hooks.assets_moderation_poll.is_none() {
        let srv = Arc::clone(shared);
        agent.hooks.assets_moderation_poll = Some(Arc::new(move |args| {
            let pid = resolve_agent_project_id(&srv, args, "assets_moderation_poll")?;
            invoke_opencloud_tool(
                &srv,
                "POST",
                &format!("/api/projects/{pid}/assets/moderation"),
                args,
            )
        }));
    }
}

fn ensure_run_worker(shared: &Arc<Shared>) {
    if shared
        .runs_worker_started
        .compare_exchange(
            false,
            true,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    let srv = Arc::clone(shared);
    std::thread::spawn(move || loop {
        let (run_id, cancel) = srv.runs.pop_next();
        let _busy = BusyGuard::new(&srv.busy);
        let mut guard = srv.agent.lock().unwrap();
        match guard.as_mut() {
            Some(agent) => {
                sync_agent_project(&srv, agent, None, None);
                crate::agent::runs::RunStore::execute_run(&srv.runs, &run_id, cancel, agent);
            }
            None => {
                srv.runs.update(&run_id, |r| {
                    r.status = crate::agent::runs::RunStatus::Failed;
                    r.error = Some("No model configured on server.".to_string());
                });
                srv.runs.finish_active(&run_id);
            }
        }
        drop(guard);
        if let Some(done) = srv.runs.get(&run_id) {
            if let Some(summary) = done.summary {
                srv.append_lines_to(&done.chat_id, &[format!("OpenRO: {summary}")]);
            } else if let Some(err) = done.error {
                srv.append_lines_to(&done.chat_id, &[format!("OpenRO: {err}")]);
            }
        }
    });
}

fn handle(
    mut stream: TcpStream,
    shared: &Arc<Shared>,
    allowed: &[String],
    slot: HandlerSlot,
) -> std::io::Result<()> {
    let mut slot_opt = Some(slot);
    for req_idx in 0..1000usize {
        if req_idx > 0 {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        }
        let req = match read_request(&mut stream) {
            Some(r) => r,
            None => return Ok(()),
        };
        if req_idx > 0 {
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        }
        if let Some((status, message)) = refusal(&req, allowed) {
            TL_KEEP_ALIVE.set(false);
            respond_err(&mut stream, status, message);
            // Flush and close write half promptly so raw-TCP clients keeping their write
            // half open receive the 413/403 + EOF immediately without waiting on IO_TIMEOUT.
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let body_cap = max_body_for_path(&req.path);
            if req.content_length > body_cap {
                drain_unread_promptly(&mut stream, req.content_length.min(4 * body_cap));
            }
            return Ok(());
        }
        let (path, query) = req.path.split_once('?').unwrap_or((req.path.as_str(), ""));

        // True SSE stream: drop the short-lived HandlerSlot so long-lived SSE
        // subscribers never starve normal concurrent HTTP traffic (SSE tracks
        // its own MAX_SSE_CLIENTS cap inside BuildEventHub).
        if req.method == "GET" && path == "/api/build/stream" {
            TL_KEEP_ALIVE.set(false);
            drop(slot_opt.take());
            if shared.events.active_subscribers() >= crate::sse::MAX_SSE_CLIENTS {
                respond_err(&mut stream, "503 Service Unavailable", "Too many live build streams are open right now. Close an extra tab and retry.");
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

        // Live realtime Studio 3D action & MCP viewport frame SSE stream:
        // drop short-lived HandlerSlot first so SSE never starves concurrent HTTP requests.
        if req.method == "GET" && path == "/api/studio/stream" {
            TL_KEEP_ALIVE.set(false);
            drop(slot_opt.take());
            if shared.studio_stream.active_subscribers() >= crate::studio_stream::MAX_STUDIO_SSE_CLIENTS {
                respond_err(&mut stream, "503 Service Unavailable", "Too many live Studio viewport streams are open right now. Close an extra tab and retry.");
                return Ok(());
            }
            let last_id = if req.last_event_id > 0 {
                req.last_event_id
            } else {
                query_param(query, "since")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            if let Err(msg) = shared.studio_stream.serve_stream(stream, last_id) {
                eprintln!("openro: Studio SSE stream refused: {msg}");
            }
            return Ok(());
        }

        // Live Embedded Roblox Studio multipart MJPEG/MPNG viewport stream (`GET /api/studio/stream.mjpg`):
        // drops HandlerSlot first so active <img> streams never starve concurrent HTTP requests.
        if req.method == "GET" && path == "/api/studio/stream.mjpg" {
            TL_KEEP_ALIVE.set(false);
            drop(slot_opt.take());
            if shared.studio_stream.active_subscribers() >= crate::studio_stream::MAX_STUDIO_SSE_CLIENTS {
                respond_err(&mut stream, "503 Service Unavailable", "Too many live Studio viewport streams are open right now. Close an extra tab and retry.");
                return Ok(());
            }
            let max_frames = if query_param(query, "once").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
                Some(1)
            } else {
                query_param(query, "max_frames").and_then(|s| s.parse::<usize>().ok()).filter(|&n| n > 0)
            };
            if let Err(msg) = shared.studio_stream.serve_mjpeg(stream, max_frames) {
                eprintln!("openro: Studio MJPEG stream refused: {msg}");
            }
            return Ok(());
        }

        // Long-poll Companion Studio Plugin endpoint: drop short-lived HandlerSlot so
        // an idle Studio plugin poll never consumes normal HTTP concurrency slots.
        if req.method == "GET" && path == "/api/studio/plugin/poll" {
            TL_KEEP_ALIVE.set(false);
            drop(slot_opt.take());
            handle_studio_plugin_poll(&mut stream, shared, &req, query);
            return Ok(());
        }

        if req.method == "GET" && crate::teamwork::is_sse_path(path) {
            TL_KEEP_ALIVE.set(false);
            drop(slot_opt.take());
            crate::teamwork::try_serve_sse(
                stream,
                shared,
                path,
                query,
                req.last_event_id,
                host_allowed(&req.host, allowed),
            );
            return Ok(());
        }

        TL_KEEP_ALIVE.set(
            req.keep_alive
                && !req.has_pipelined
                && path != "/api/update/restart"
                && shared.active_handlers.load(Ordering::Relaxed) < POOL_WORKERS,
        );

        'req_done: {

    if let Some(rep) = crate::teamwork::route(
        shared,
        &req.method,
        &req.path,
        &req.body,
        host_allowed(&req.host, allowed),
    ) {
        respond_bytes(&mut stream, rep.status, rep.content_type, "no-store", &rep.body);
        break 'req_done;
    }

    // Parameterized staged diff proposal routes:
    // POST /api/proposals/:id/apply & POST /api/proposals/:id/reject
    if let Some(rest) = path.strip_prefix("/api/proposals/") {
        if let Some(prop_id) = rest.strip_suffix("/apply") {
            if req.method == "POST" && !prop_id.is_empty() {
                handle_proposal_apply(&mut stream, shared, &req, prop_id);
                break 'req_done;
            }
        }
        if let Some(prop_id) = rest.strip_suffix("/reject") {
            if req.method == "POST" && !prop_id.is_empty() {
                handle_proposal_reject(&mut stream, shared, prop_id);
                break 'req_done;
            }
        }
    }

    // Parameterized snapshot routes: POST /api/snapshots/:id/restore, POST /api/snapshots/:id/restore-hooks & GET /api/snapshots/:id/rbxlx
    if let Some(rest) = path.strip_prefix("/api/snapshots/") {
        if let Some(snap_id) = rest.strip_suffix("/restore-hooks") {
            if req.method == "POST" {
                handle_snapshot_restore_hooks(&mut stream, shared, snap_id);
                break 'req_done;
            }
        }
        if let Some(snap_id) = rest.strip_suffix("/restore") {
            if req.method == "POST" {
                handle_snapshot_restore(&mut stream, shared, snap_id);
                break 'req_done;
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
                break 'req_done;
            }
        }
    }

    // Parameterized game art, Blender mesh, and audio asset routes:
    // GET /api/assets/manifest, GET /api/assets/meshes/:file, GET /api/assets/audio/:file, GET|DELETE /api/assets/:file
    if req.method == "GET" {
        if let Some(chunk_name) = path.strip_prefix("/chunks/") {
            if serve_chunk_asset(&mut stream, &req, chunk_name) {
                break 'req_done;
            }
        }
    }

    if let Some(raw_file) = path.strip_prefix("/api/assets/") {
        if !raw_file.is_empty() {
            if req.method == "GET" && raw_file == "manifest" {
                handle_assets_manifest(&mut stream, shared);
                break 'req_done;
            }
            if let Some(mesh_file) = raw_file.strip_prefix("meshes/") {
                if req.method == "GET" && !mesh_file.is_empty() {
                    handle_mesh_asset_file(&mut stream, shared, mesh_file);
                    break 'req_done;
                }
            }
            if let Some(audio_file) = raw_file.strip_prefix("audio/") {
                if req.method == "GET" && !audio_file.is_empty() {
                    handle_audio_asset_file(&mut stream, shared, audio_file);
                    break 'req_done;
                }
            }
            if req.method == "GET" {
                handle_asset_file(&mut stream, shared, raw_file);
                break 'req_done;
            }
            if req.method == "DELETE" {
                handle_asset_delete(&mut stream, shared, raw_file);
                break 'req_done;
            }
        }
    }

    // Parameterized official Roblox Studio MCP tool route: POST /api/studio/tools/:tool
    if let Some(tool_name) = path.strip_prefix("/api/studio/tools/") {
        if !tool_name.is_empty() && req.method == "POST" {
            handle_studio_tool(&mut stream, shared, &req, tool_name);
            break 'req_done;
        }
    }

    // v0.11 Projects, project files, git, one-way Studio sync, and Playtest Runner routes.
    if let Some(reply) = crate::project_api::route(
        shared,
        &req.method,
        &req.path,
        &req.body,
        host_allowed(&req.host, &[]),
    ) {
        respond_bytes(&mut stream, reply.status, reply.content_type, "no-store", &reply.body);
        break 'req_done;
    }

    // Gap #84/#85 project-scoped Studio bridge & live verification routes.
    if try_route_project_studio(&mut stream, shared, &req, path) {
        break 'req_done;
    }

    // Open Cloud, shared secrets, reference search, art generation & image processing routes.
    if try_route_opencloud_assets(&mut stream, shared, &req, path, query) {
        break 'req_done;
    }

    // Universal Anything-to-API / Anything-to-CLI tool adapter registry routes (#52).
    if try_route_tool_adapters(&mut stream, shared, &req, path, query) {
        break 'req_done;
    }

    // Studio Computer Use (CUA) routes (Gap #81, strictly loopback-guarded).
    if try_route_studio_cua(&mut stream, shared, &req, path, query) {
        break 'req_done;
    }

    // Free CC0-1.0 SFX Pack Library & Local Neural Audio Runner routes (v1.33.0, Gap #88).
    if try_route_audio_library(&mut stream, shared, &req, path, query) {
        break 'req_done;
    }

    match (req.method.as_str(), path) {
        ("GET", "/api/art-knowledge") => {
            respond_json(&mut stream, "200 OK", &crate::art_knowledge::summary_json());
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
        ("GET", "/api/studio/actions") => {
            let since = query_param(query, "since")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let mut snap = shared.studio_stream.snapshot_json(since);
            if let Some(obj) = snap.as_object_mut() {
                obj.insert("ok".to_string(), json!(true));
            }
            respond_json(&mut stream, "200 OK", &snap);
        }
        ("GET", "/api/studio/frame") => {
            match shared.studio_stream.latest_frame() {
                Some(frame) => {
                    let extra = format!("X-Frame-Seq: {}\r\n", frame.seq);
                    respond_bytes_with_headers(
                        &mut stream,
                        "200 OK",
                        &frame.mime,
                        "no-store",
                        &extra,
                        &frame.bytes,
                    );
                }
                None => respond_json(
                    &mut stream,
                    "404 Not Found",
                    &json!({
                        "ok": false,
                        "has_live_frame": false,
                        "studio_mode": shared.studio_stream.studio_mode(),
                        "error": "No live Studio frame captured yet. Connect Roblox Studio MCP or click Capture Frame.",
                    }),
                ),
            }
        }
        ("POST", "/api/studio/capture") => {
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.capture_studio_frame() {
                    Ok(frame) => respond_json(
                        &mut stream,
                        "200 OK",
                        &json!({
                            "ok": true,
                            "frame_seq": frame.seq,
                            "mime": frame.mime,
                            "width": frame.width,
                            "height": frame.height,
                            "url": format!("/api/studio/frame?seq={}", frame.seq),
                            "frame": frame.meta_json(),
                        }),
                    ),
                    Err(e) => respond_json(
                        &mut stream,
                        "503 Service Unavailable",
                        &json!({
                            "ok": false,
                            "studio_mode": shared.studio_stream.studio_mode(),
                            "error": e,
                        }),
                    ),
                },
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({
                        "ok": false,
                        "studio_mode": shared.studio_stream.studio_mode(),
                        "error": "Studio not connected",
                    }),
                ),
            }
        }
        ("POST", "/api/studio/playtest") => {
            let _busy = BusyGuard::new(&shared.busy);
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.run_studio_playtest() {
                    Ok(val) => respond_json(&mut stream, "200 OK", &val),
                    Err(e) => respond_json(
                        &mut stream,
                        "502 Bad Gateway",
                        &json!({ "ok": false, "passed": false, "error": e }),
                    ),
                },
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "passed": false, "error": "Studio not connected" }),
                ),
            }
        }
        ("POST", "/api/studio/sandbox/run") => {
            let body: Value = if req.body.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => {
                        respond_json(
                            &mut stream,
                            "400 Bad Request",
                            &json!({ "ok": false, "passed": false, "error": "the request body is not valid JSON" }),
                        );
                        return Ok(());
                    }
                }
            };
            let _busy = BusyGuard::new(&shared.busy);
            enum SandboxSource {
                Local(String, std::collections::BTreeMap<String, String>, std::collections::BTreeMap<String, Value>),
                Rojo(PathBuf),
            }
            let explicit_project = body
                .get("project_id")
                .and_then(Value::as_str)
                .and_then(|pid| shared.projects.lock().unwrap().find(pid).map(|r| r.dir));
            let source = if let Some(dir) = explicit_project {
                Some(SandboxSource::Rojo(dir))
            } else {
                let agent_src = {
                    let guard = shared.agent.lock().unwrap();
                    guard.as_ref().and_then(|agent| match &agent.studio {
                        Some(PlaceBackend::Local(local)) => Some(SandboxSource::Local(
                            local.game_name().to_string(),
                            local.scripts().clone(),
                            local.instances().clone(),
                        )),
                        _ => agent.project_root.clone().map(SandboxSource::Rojo),
                    })
                };
                agent_src
                    .or_else(|| {
                        let store = shared.projects.lock().unwrap();
                        store
                            .active
                            .as_deref()
                            .and_then(|id| store.find(id))
                            .map(|r| SandboxSource::Rojo(r.dir))
                    })
                    .or_else(|| {
                        shared.data_dir.as_ref().and_then(|dir| {
                            dir.join("place.json").is_file().then(|| {
                                let lp = LocalPlace::open(dir.clone(), shared.game_name.clone());
                                SandboxSource::Local(
                                    lp.game_name().to_string(),
                                    lp.scripts().clone(),
                                    lp.instances().clone(),
                                )
                            })
                        })
                    })
            };
            let Some(src) = source else {
                respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({
                        "ok": false,
                        "engine": "lune_roblox_shim",
                        "passed": false,
                        "error": "No LocalPlace or active Rojo project is loaded"
                    }),
                );
                return Ok(());
            };
            let run_res = match src {
                SandboxSource::Local(name, scripts, instances) => {
                    crate::sandbox::run_local_place_sandbox(&name, &scripts, &instances, &body)
                }
                SandboxSource::Rojo(dir) => crate::sandbox::run_project_sandbox(&dir, &body),
            };
            match run_res {
                Ok(val) => {
                    let status = if val.pointer("/diagnostics/lune_found").and_then(Value::as_bool)
                        == Some(false)
                    {
                        "503 Service Unavailable"
                    } else {
                        "200 OK"
                    };
                    respond_json(&mut stream, status, &val);
                }
                Err(e) => respond_json(
                    &mut stream,
                    "400 Bad Request",
                    &json!({
                        "ok": false,
                        "engine": "lune_roblox_shim",
                        "passed": false,
                        "error": e
                    }),
                ),
            }
        }
        ("GET", "/api/studio/gui") => {
            let pid = query_param(query, "project_id");
            let proj_dir = resolve_studio_gui_project_dir(shared, pid);
            if let Some(slug) = query_param(query, "slug") {
                match crate::gui_designer::validate_slug(slug) {
                    Err(e) => respond_err(&mut stream, "400 Bad Request", &e),
                    Ok(clean) => {
                        let res = if let Some(dir) = proj_dir {
                            crate::gui_designer::get_project_screen(&dir, &clean)
                        } else {
                            let store = shared.gui_store.lock().unwrap();
                            match store.get(&clean) {
                                Ok(Some(doc)) => {
                                    let analysis = crate::gui_designer::analyze_safe_areas(&doc);
                                    Ok(json!({
                                        "slug": clean,
                                        "doc": doc.to_json(),
                                        "analysis": analysis.to_json(),
                                        "safe_area_warnings": analysis.warnings.iter().map(crate::gui_designer::SafeAreaWarning::to_json).collect::<Vec<_>>(),
                                    }))
                                }
                                Ok(None) => Err(format!("screen '{clean}' not found")),
                                Err(e) => Err(e),
                            }
                        };
                        match res {
                            Ok(mut v) => {
                                if let Some(obj) = v.as_object_mut() {
                                    obj.insert("ok".to_string(), json!(true));
                                }
                                respond_json(&mut stream, "200 OK", &v);
                            }
                            Err(e) => {
                                let st = if e.contains("not found") { "404 Not Found" } else { "400 Bad Request" };
                                respond_err(&mut stream, st, &e);
                            }
                        }
                    }
                }
            } else {
                let res = if let Some(dir) = proj_dir {
                    crate::gui_designer::list_project_screens(&dir)
                } else {
                    let store = shared.gui_store.lock().unwrap();
                    let screens: Vec<Value> = store
                        .list()
                        .into_iter()
                        .map(|(slug, doc)| {
                            let analysis = crate::gui_designer::analyze_safe_areas(&doc);
                            json!({
                                "slug": slug,
                                "name": doc.name,
                                "version": doc.version,
                                "warning_count": analysis.warning_count,
                                "doc": doc.to_json(),
                            })
                        })
                        .collect();
                    Ok(json!({
                        "screens": screens,
                        "presets": crate::gui_designer::list_presets_metadata(),
                    }))
                };
                match res {
                    Ok(mut v) => {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("ok".to_string(), json!(true));
                        }
                        respond_json(&mut stream, "200 OK", &v);
                    }
                    Err(e) => respond_err(&mut stream, "400 Bad Request", &e),
                }
            }
        }
        ("POST", "/api/studio/gui") => {
            let body: Value = if req.body.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => {
                        respond_err(&mut stream, "400 Bad Request", "the request body is not valid JSON");
                        break 'req_done;
                    }
                }
            };
            let clean = match extract_studio_gui_slug(&body) {
                Ok(s) => s,
                Err(e) => {
                    respond_err(&mut stream, "400 Bad Request", &e);
                    break 'req_done;
                }
            };
            let pid = body.get("project_id").and_then(Value::as_str);
            let res = if let Some(dir) = resolve_studio_gui_project_dir(shared, pid) {
                crate::gui_designer::save_project_screen(&dir, &clean, &body)
            } else {
                crate::gui_designer::doc_from_save_body(&clean, &body).and_then(|doc| {
                    let (saved_slug, validated, path) = shared.gui_store.lock().unwrap().save(&clean, doc)?;
                    let analysis = crate::gui_designer::analyze_safe_areas(&validated);
                    Ok(json!({
                        "ok": true,
                        "slug": saved_slug,
                        "path": path,
                        "doc": validated.to_json(),
                        "analysis": analysis.to_json(),
                        "safe_area_warnings": analysis.warnings.iter().map(crate::gui_designer::SafeAreaWarning::to_json).collect::<Vec<_>>(),
                    }))
                })
            };
            match res {
                Ok(v) => respond_json(&mut stream, "200 OK", &v),
                Err(e) => respond_err(&mut stream, "400 Bad Request", &e),
            }
        }
        ("POST", "/api/studio/gui/compile") => {
            let body: Value = if req.body.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => {
                        respond_err(&mut stream, "400 Bad Request", "the request body is not valid JSON");
                        break 'req_done;
                    }
                }
            };
            let clean = match extract_studio_gui_slug(&body) {
                Ok(s) => s,
                Err(e) => {
                    respond_err(&mut stream, "400 Bad Request", &e);
                    break 'req_done;
                }
            };
            let pid = body.get("project_id").and_then(Value::as_str);
            let res = if let Some(dir) = resolve_studio_gui_project_dir(shared, pid) {
                crate::gui_designer::compile_project_screen(&dir, &clean, &body)
            } else {
                let doc_res = if body.get("doc").is_some()
                    || (body.get("root").is_some() && body.get("version").is_some())
                    || body.get("preset").is_some()
                {
                    crate::gui_designer::doc_from_save_body(&clean, &body)
                } else {
                    shared
                        .gui_store
                        .lock()
                        .unwrap()
                        .get(&clean)
                        .and_then(|opt| opt.ok_or_else(|| format!("screen '{clean}' not found")))
                };
                doc_res.and_then(|doc| crate::gui_designer::compile_screen_doc(&clean, &doc, None))
            };
            match res {
                Ok(v) => respond_json(&mut stream, "200 OK", &v),
                Err(e) => {
                    let st = if e.contains("not found") { "404 Not Found" } else { "400 Bad Request" };
                    respond_err(&mut stream, st, &e);
                }
            }
        }
        ("GET", "/api/ui-kit/presets") => {
            respond_json(&mut stream, "200 OK", &crate::ui_kit::presets_json());
        }
        ("POST", "/api/studio/ui-kit/generate") => {
            let mut body: Value = if req.body.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => {
                        respond_err(&mut stream, "400 Bad Request", "the request body is not valid JSON");
                        break 'req_done;
                    }
                }
            };
            let data_dir = shared
                .data_dir
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new("openro-data"));
            if let Some(obj) = body.as_object_mut() {
                obj.entry("data_dir".to_string())
                    .or_insert_with(|| json!(data_dir.to_string_lossy()));
            }
            let pid = body.get("project_id").and_then(Value::as_str).map(str::to_string);
            let proj_dir = resolve_studio_gui_project_dir(shared, pid.as_deref());
            let (root_dir, eff_pid) = match proj_dir {
                Some(d) => (d, pid.as_deref()),
                None => {
                    let studio_root = data_dir.join("ui-kit-studio");
                    let _ = std::fs::create_dir_all(studio_root.join("art"));
                    (studio_root, None)
                }
            };
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            match crate::ui_kit::generate_ui_kit(&root_dir, &store, eff_pid, &body) {
                Ok(v) => respond_json(&mut stream, "200 OK", &v),
                Err(oc) => {
                    let st = match oc.status {
                        404 => "404 Not Found",
                        429 => "429 Too Many Requests",
                        500 => "500 Internal Server Error",
                        502 => "502 Bad Gateway",
                        503 => "503 Service Unavailable",
                        504 => "504 Gateway Timeout",
                        _ => "400 Bad Request",
                    };
                    respond_err(&mut stream, st, &oc.msg);
                }
            }
        }
        ("POST", "/api/studio/playtest/fix-all") => {
            let _busy = BusyGuard::new(&shared.busy);
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => {
                    let scripts = crate::opencloud::monetization_pack_luau_scripts_with_preset(
                        Some("starter_obby"),
                        "Coins",
                        0,
                        0,
                        100,
                    );
                    let mut applied = Vec::new();
                    for (place_path, _rel, _cls, src) in &scripts {
                        if let Ok(upd) = agent.update_script_source(&chat_id, place_path, src) {
                            applied.push(upd.path);
                        }
                    }
                    match agent.run_studio_playtest() {
                        Ok(pt) => {
                            let passed = pt["passed"].as_bool().unwrap_or(false);
                            respond_json(
                                &mut stream,
                                "200 OK",
                                &json!({
                                    "ok": true,
                                    "applied_scripts": applied,
                                    "export_ready": passed,
                                    "export_urls": {
                                        "rbxlx": "/api/export/rbxlx",
                                        "ship_pack": "/api/export/bundle"
                                    },
                                    "playtest": pt
                                }),
                            );
                        }
                        Err(e) => respond_json(
                            &mut stream,
                            "502 Bad Gateway",
                            &json!({ "ok": false, "export_ready": false, "error": e }),
                        ),
                    }
                }
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "export_ready": false, "error": "Studio not connected" }),
                ),
            }
        }
        ("POST", "/api/studio/mcp/connect") => {
            handle_studio_mcp_connect(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/mcp/disconnect") => {
            handle_studio_mcp_disconnect(&mut stream, shared, &req);
        }
        ("GET", "/api/studio/embed/status") => {
            handle_studio_embed_status(&mut stream, shared);
        }
        ("POST", "/api/studio/embed/launch") => {
            handle_studio_embed_launch(&mut stream, shared, &req, false);
        }
        ("POST", "/api/studio/embed/attach") => {
            handle_studio_embed_launch(&mut stream, shared, &req, true);
        }
        ("POST", "/api/studio/embed/transfer") => {
            handle_studio_embed_transfer(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/launch-and-continue") => {
            handle_studio_launch_and_continue(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/embed/stop") => {
            handle_studio_embed_stop(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/input/mouse") => {
            handle_studio_input_mouse(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/input/keyboard") => {
            handle_studio_input_keyboard(&mut stream, shared, &req);
        }
        ("GET", "/api/proposals") => {
            handle_proposals_list(&mut stream, shared, query);
        }
        ("GET", "/api/studio/plugin/status") => {
            handle_studio_plugin_status(&mut stream, shared, &req);
        }
        ("GET", "/api/studio/plugin/download") => {
            handle_studio_plugin_download(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/plugin/install") => {
            handle_studio_plugin_install(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/plugin/handshake") => {
            handle_studio_plugin_handshake(&mut stream, shared, &req);
        }
        ("POST", "/api/studio/plugin/result") => {
            handle_studio_plugin_result(&mut stream, shared, &req);
        }
        ("GET", "/api/library") => {
            let body = cached_library_list_json(shared);
            respond_json(&mut stream, "200 OK", &body);
        }
        ("POST", "/api/library/install") => {
            handle_library_install(&mut stream, shared, &req);
        }
        ("POST", "/api/library/uninstall") => {
            handle_library_uninstall(&mut stream, shared, &req);
        }
        ("POST", "/api/library/configure") => {
            handle_library_configure(&mut stream, shared, &req);
        }
        ("POST", "/api/library/test") => {
            handle_library_test(&mut stream, shared, &req);
        }
        ("GET", "/api/market-taste") => {
            handle_market_taste_get(&mut stream, shared, query);
        }
        ("POST", "/api/market-taste/niche") => {
            handle_market_taste_niche(&mut stream, shared, &req);
        }
        ("POST", "/api/market-taste/refresh") => {
            handle_market_taste_refresh(&mut stream, shared, &req);
        }
        ("POST", "/api/market-taste/research") => {
            handle_market_taste_research(&mut stream, shared, &req);
        }
        ("GET", "/api/tools/blender/status") => {
            handle_tools_blender_status(&mut stream);
        }
        ("POST", "/api/tools/blender/generate") => {
            handle_tools_blender_generate(&mut stream, shared, &req);
        }
        ("POST", "/api/tools/audio/generate") => {
            handle_tools_audio_generate(&mut stream, shared, &req);
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
            if include_tree {
                let store = shared.snapshots.lock().unwrap();
                let payload = json!({
                    "ok": true,
                    "snapshots": store.list(true),
                    "latest_diff": store.latest_diff(),
                });
                drop(store);
                respond_json(&mut stream, "200 OK", &payload);
            } else {
                let bytes = shared.snapshots.lock().unwrap().summary_response_bytes();
                respond_bytes(&mut stream, "200 OK", "application/json", "no-store", &bytes);
            }
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
                Err(e) => respond_err(&mut stream, "503 Service Unavailable", &e),
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
        ("POST", "/api/place/create") => {
            handle_place_create(&mut stream, shared, &req.body);
        }
        ("POST", "/api/place/duplicate") => {
            handle_place_duplicate(&mut stream, shared, &req.body);
        }
        ("POST", "/api/place/delete") => {
            handle_place_delete(&mut stream, shared, &req.body);
        }
        ("POST", "/api/place/script") => {
            handle_place_script(&mut stream, shared, &req.body);
        }
        ("POST", "/api/redo") => {
            handle_redo(&mut stream, shared);
        }
        ("GET", "/api/export/bundle") => {
            handle_export_bundle(&mut stream, shared);
        }
        ("GET", "/api/skills") => {
            static SKILLS_BYTES: OnceLock<Vec<u8>> = OnceLock::new();
            let bytes = SKILLS_BYTES.get_or_init(|| {
                let catalog = crate::skills::catalog_json();
                let count = catalog.as_array().map(|a| a.len()).unwrap_or(0);
                json!({
                    "ok": true,
                    "count": count,
                    "skills": catalog,
                })
                .to_string()
                .into_bytes()
            });
            respond_bytes(&mut stream, "200 OK", "application/json", "no-store", bytes);
        }
        ("POST", "/api/skills/build") => {
            handle_skill_build(&mut stream, shared, &req.body);
        }
        ("POST", "/api/monetization/pack") => {
            handle_monetization_pack(&mut stream, shared, &req.body);
        }
        ("POST", "/api/studio/economy/simulate") => {
            handle_studio_economy_simulate(&mut stream, shared, &req.body);
        }
        ("GET", "/api/studio/publish-guard") => {
            let current_tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
            let report = crate::playtest::studio_publish_guard_from_tree(&current_tree);
            respond_json(&mut stream, "200 OK", &report);
        }
        ("POST", "/api/studio/publish-guard/autofix") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let action_filter = body
                .get("fix_action")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let mut guard = shared.agent.lock().unwrap();
            let Some(agent) = guard.as_mut() else {
                respond_err(&mut stream, "503 Service Unavailable", "Studio not connected");
                break 'req_done;
            };
            let current_tree = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let chat_id = shared.active_chat.lock().unwrap().clone();
            if action_filter.is_none() || action_filter == Some("prune_dependencies") {
                let _ = crate::playtest::prune_or_fix_studio_place(agent, &chat_id);
            }
            let refreshed = agent.place_tree().unwrap_or(current_tree);
            let (next_tree, modified_files, fixed_count) =
                crate::playtest::studio_autofix_publish_guard_tree(&refreshed, action_filter);
            if fixed_count > 0 {
                let _ = agent.apply_place_tree(&next_tree);
                let _ = shared.snapshots.lock().unwrap().record(
                    "Pre-publish-guard autofix checkpoint".to_string(),
                    "publish-guard",
                    Some(chat_id),
                    next_tree.clone(),
                    None,
                );
            }
            let final_tree = agent.place_tree().unwrap_or(next_tree);
            drop(guard);
            let report = crate::playtest::studio_publish_guard_from_tree(&final_tree);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "fixed_count": fixed_count,
                    "modified_files": modified_files,
                    "report": report,
                }),
            );
        }
        ("GET", "/api/studio/monetization-audit") => {
            let current_tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
            let mut report = crate::playtest::studio_monetization_audit_from_tree(&current_tree);
            report["ok"] = json!(true);
            respond_json(&mut stream, "200 OK", &report);
        }
        ("POST", "/api/studio/monetization-audit") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let action_filter = body
                .get("fix_action")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let mut guard = shared.agent.lock().unwrap();
            let Some(agent) = guard.as_mut() else {
                respond_err(&mut stream, "503 Service Unavailable", "Studio not connected");
                break 'req_done;
            };
            let current_tree = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let (next_tree, modified_files, fixed_count) =
                crate::playtest::studio_repair_monetization_audit_tree(&current_tree, action_filter);
            if fixed_count > 0 {
                let _ = shared.snapshots.lock().unwrap().record(
                    "Pre-monetization-audit repair checkpoint".to_string(),
                    "monetization-audit",
                    Some(chat_id),
                    current_tree,
                    None,
                );
                let _ = agent.apply_place_tree(&next_tree);
            }
            let final_tree = agent.place_tree().unwrap_or(next_tree);
            drop(guard);
            let report = crate::playtest::studio_monetization_audit_from_tree(&final_tree);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "schema": "openro.monetization_audit.v1",
                    "fixed_count": fixed_count,
                    "modified_files": modified_files,
                    "report": report,
                }),
            );
        }
        ("GET", "/api/studio/economy-sim") => {
            let current_tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
            let mut report = crate::playtest::studio_economy_sim_from_tree(&current_tree, &json!({}));
            report["ok"] = json!(true);
            respond_json(&mut stream, "200 OK", &report);
        }
        ("POST", "/api/studio/economy-sim") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let apply = body.get("apply_suggestions").and_then(Value::as_bool) == Some(true);
            let mut guard = shared.agent.lock().unwrap();
            let Some(agent) = guard.as_mut() else {
                respond_err(&mut stream, "503 Service Unavailable", "Studio not connected");
                break 'req_done;
            };
            let current_tree = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            if !apply {
                drop(guard);
                let report = crate::playtest::studio_economy_sim_from_tree(&current_tree, &body);
                respond_json(
                    &mut stream,
                    "200 OK",
                    &json!({
                        "ok": true,
                        "schema": "openro.economy_sim.v1",
                        "applied": false,
                        "fixed_count": 0,
                        "modified_files": [],
                        "report": report,
                    }),
                );
                break 'req_done;
            }
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let (next_tree, modified_files, fixed_count) =
                crate::playtest::studio_apply_economy_sim_tree(&current_tree, &body);
            if fixed_count > 0 {
                let _ = shared.snapshots.lock().unwrap().record(
                    "Pre-economy-sim balance checkpoint".to_string(),
                    "economy-sim",
                    Some(chat_id),
                    current_tree,
                    None,
                );
                let _ = agent.apply_place_tree(&next_tree);
            }
            let final_tree = agent.place_tree().unwrap_or(next_tree);
            drop(guard);
            let report = crate::playtest::studio_economy_sim_from_tree(&final_tree, &json!({}));
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "schema": "openro.economy_sim.v1",
                    "applied": fixed_count > 0,
                    "fixed_count": fixed_count,
                    "modified_files": modified_files,
                    "report": report,
                }),
            );
        }
        ("GET", "/api/studio/sync-conflicts") => {
            let latest_snap = shared.snapshots.lock().unwrap().latest().cloned();
            let current_tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
            let mut place_scripts = std::collections::BTreeMap::new();
            let mut local_scripts = std::collections::BTreeMap::new();
            if let Some(obj) = current_tree.get("scripts").and_then(Value::as_object) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        local_scripts.insert(k.clone(), s.to_string());
                    }
                }
            }
            let (has_base, bid) = if let Some(s) = &latest_snap {
                if let Some(obj) = s.tree.get("scripts").and_then(Value::as_object) {
                    for (k, v) in obj {
                        if let Some(src) = v.as_str() {
                            place_scripts.insert(k.clone(), src.to_string());
                        }
                    }
                }
                (true, Some(s.id.as_str()))
            } else {
                (false, None)
            };
            let report = crate::playtest::build_sync_conflicts_report(
                &place_scripts,
                &local_scripts,
                has_base,
                bid,
            );
            respond_json(&mut stream, "200 OK", &report);
        }
        ("POST", "/api/studio/sync-conflicts/resolve") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let action = body.get("action").and_then(Value::as_str).unwrap_or("").trim();
            let path_key = body.get("path").and_then(Value::as_str).unwrap_or("").trim();
            if !matches!(action, "accept_local" | "keep_place" | "backup_snapshot")
                || path_key.is_empty()
                || path_key.contains("..")
                || path_key.contains('/')
                || path_key.contains('\\')
                || path_key.contains('\0')
            {
                respond_err(&mut stream, "400 Bad Request", "invalid sync conflict resolve payload");
                break 'req_done;
            }
            let latest_snap = shared.snapshots.lock().unwrap().latest().cloned();
            let mut guard = shared.agent.lock().unwrap();
            let Some(agent) = guard.as_mut() else {
                respond_err(&mut stream, "503 Service Unavailable", "Studio not connected");
                break 'req_done;
            };
            let current_tree = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let backup_snap = shared.snapshots.lock().unwrap().record(
                format!("Sync backup ({action}: {path_key})"),
                "sync-resolve",
                Some(chat_id.clone()),
                current_tree.clone(),
                None,
            );
            if action == "keep_place" {
                if let Some(src) = latest_snap
                    .as_ref()
                    .and_then(|s| s.tree.get("scripts"))
                    .and_then(|m| m.get(path_key))
                    .and_then(Value::as_str)
                {
                    let mut next_tree = current_tree.clone();
                    if let Some(m) = next_tree.get_mut("scripts").and_then(Value::as_object_mut) {
                        m.insert(path_key.to_string(), Value::String(src.to_string()));
                    }
                    let _ = agent.apply_place_tree(&next_tree);
                }
            }
            let after_tree = agent.place_tree().unwrap_or(current_tree);
            if action == "accept_local" || action == "keep_place" {
                let _ = shared.snapshots.lock().unwrap().record(
                    format!("Sync resolved ({action}: {path_key})"),
                    "sync-resolve",
                    Some(chat_id),
                    after_tree.clone(),
                    None,
                );
            }
            drop(guard);
            let mut local_scripts = std::collections::BTreeMap::new();
            if let Some(obj) = after_tree.get("scripts").and_then(Value::as_object) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        local_scripts.insert(k.clone(), s.to_string());
                    }
                }
            }
            let place_scripts = if action == "accept_local" {
                local_scripts.clone()
            } else if let Some(s) = &latest_snap {
                let mut m = std::collections::BTreeMap::new();
                if let Some(obj) = s.tree.get("scripts").and_then(Value::as_object) {
                    for (k, v) in obj {
                        if let Some(src) = v.as_str() {
                            m.insert(k.clone(), src.to_string());
                        }
                    }
                }
                m
            } else {
                local_scripts.clone()
            };
            let report = crate::playtest::build_sync_conflicts_report(
                &place_scripts,
                &local_scripts,
                latest_snap.is_some(),
                latest_snap.as_ref().map(|s| s.id.as_str()),
            );
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "action": action,
                    "path": path_key,
                    "backup_snapshot_id": backup_snap.id,
                    "in_sync": report["in_sync"],
                    "conflict_count": report["conflict_count"],
                    "conflicts": report["conflicts"],
                    "report": report,
                }),
            );
        }
        ("GET", "/api/studio/snapshots/hook-integrity") => {
            let latest_snap = shared.snapshots.lock().unwrap().latest().cloned();
            let current_tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
            let report = crate::playtest::studio_hook_integrity_from_trees(
                latest_snap.as_ref().map(|s| s.id.as_str()),
                latest_snap.as_ref().map(|s| s.label.as_str()),
                latest_snap.as_ref().map(|s| &s.tree),
                &current_tree,
            );
            respond_json(&mut stream, "200 OK", &report);
        }
        ("GET", "/api/studio/dependencies") => {
            let tree = shared
                .agent
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|a| a.place_tree().ok())
                .unwrap_or_else(|| json!({ "scripts": {} }));
            let mut scripts_map = std::collections::BTreeMap::new();
            if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        scripts_map.insert(k.clone(), s.to_string());
                    }
                }
            }
            let report = crate::playtest::studio_dependencies_report(&scripts_map);
            respond_json(&mut stream, "200 OK", &report);
        }
        ("POST", "/api/studio/dependencies/prune") => {
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match crate::playtest::prune_or_fix_studio_place(agent, &chat_id) {
                    Ok(res) => respond_json(&mut stream, "200 OK", &res),
                    Err(e) => respond_err(&mut stream, "502 Bad Gateway", &e),
                },
                None => respond_err(&mut stream, "503 Service Unavailable", "Studio not connected"),
            }
        }
        ("GET", "/api/luau/check") | ("POST", "/api/luau/check") => {
            handle_luau_check(&mut stream, shared, req.method == "POST", &req.body);
        }
        ("GET", "/") | ("GET", "/play") => {
            let info = status_info(shared);
            serve_index_page(
                &mut stream,
                &req,
                &shared.game_name,
                info.studio_connected,
                info.model_configured,
                &shared.store_recovered,
                info.workspace_mode,
            );
        }
        ("GET", "/api/version") => {
            static VERSION_BYTES: OnceLock<Vec<u8>> = OnceLock::new();
            let bytes = VERSION_BYTES.get_or_init(|| {
                json!({ "name": "openro-rs", "version": env!("CARGO_PKG_VERSION") })
                    .to_string()
                    .into_bytes()
            });
            respond_bytes(&mut stream, "200 OK", "application/json", "no-store", bytes);
        }
        ("GET", "/api/health") => {
            static HEALTH_BYTES: OnceLock<Vec<u8>> = OnceLock::new();
            let bytes = HEALTH_BYTES.get_or_init(|| {
                json!({
                    "ok": true,
                    "status": "ok",
                    "name": "openro-rs",
                    "version": env!("CARGO_PKG_VERSION"),
                    "cc0_sfx_library": true,
                    "audio_presets_tiered": true,
                    "youtube_music_adapter": true,
                })
                .to_string()
                .into_bytes()
            });
            respond_bytes(&mut stream, "200 OK", "application/json", "no-store", bytes);
        }
        ("GET", "/api/capabilities") => {
            static CAPS_BYTES: OnceLock<Vec<u8>> = OnceLock::new();
            let bytes = CAPS_BYTES.get_or_init(|| {
                json!({
                    "ok": true,
                    "name": "openro-rs",
                    "version": env!("CARGO_PKG_VERSION"),
                    "library_api": true,
                    "adapters_api": true,
                    "audio_synth": true,
                    "cc0_sfx_library": true,
                    "audio_presets_tiered": true,
                    "youtube_music_adapter": true,
                    "studio_embed": true,
                    "market_taste_api": true,
                })
                .to_string()
                .into_bytes()
            });
            respond_bytes(&mut stream, "200 OK", "application/json", "no-store", bytes);
        }
        ("GET", "/api/export/rbxlx") => {
            let stored = all_stored_assets(shared);
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => {
                    if !stored.is_empty() {
                        agent.bind_assets(&stored);
                    }
                    match agent.export_rbxlx(&shared.game_name) {
                        Ok(xml) => {
                            let fname = format!("{}.rbxlx", slugify_filename(&shared.game_name));
                            respond_download(
                                &mut stream,
                                "application/xml; charset=utf-8",
                                &fname,
                                &xml,
                            );
                        }
                        Err(e) => respond_err(&mut stream, "502 Bad Gateway", &e),
                    }
                }
                None => respond_err(&mut stream, "503 Service Unavailable", "Studio not connected"),
            }
        }
        ("GET", "/api/runs") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "runs are only accessible from this machine (loopback)");
                break 'req_done;
            }
            respond_json(&mut stream, "200 OK", &shared.runs.list_json());
        }
        ("POST", "/api/runs") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "runs are only accessible from this machine (loopback)");
                break 'req_done;
            }
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let message = body.get("message").or_else(|| body.get("ask")).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if message.is_empty() {
                respond_err(&mut stream, "400 Bad Request", "empty message");
                break 'req_done;
            }
            let chat_id = body
                .get("chat_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| shared.active_chat.lock().unwrap().clone());
            let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("plan_execute").to_string();
            let project_id = body.get("project_id").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let explicit_root = body.get("project_root").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let (resolved_dir, resolved_id) = resolve_project_for_agent(shared, project_id.as_deref(), explicit_root.as_deref());
            let project_id = resolved_id.or(project_id);
            let project_root = resolved_dir.map(|d| d.to_string_lossy().to_string());
            let max_steps = body.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(40) as usize;
            shared.append_lines_to(&chat_id, &[format!("You: {message}")]);
            let run = shared.runs.enqueue(crate::agent::runs::EnqueueRun {
                chat_id,
                ask: message,
                mode,
                project_id,
                project_root,
                max_steps,
            });
            ensure_run_worker(shared);
            respond_json(&mut stream, "202 Accepted", &json!({ "ok": true, "run_id": run.run_id, "run": run.detail_json() }));
        }
        ("GET", p) if p.starts_with("/api/runs/") && !p["/api/runs/".len()..].contains('/') => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "runs are only accessible from this machine (loopback)");
                break 'req_done;
            }
            let id = &p["/api/runs/".len()..];
            match shared.runs.get(id) {
                Some(run) => respond_json(&mut stream, "200 OK", &json!({ "ok": true, "run": run.detail_json() })),
                None => respond_json(&mut stream, "404 Not Found", &json!({ "ok": false, "error": format!("unknown run '{id}'") })),
            }
        }
        ("POST", p) if p.starts_with("/api/runs/") && p.ends_with("/resume") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "runs are only accessible from this machine (loopback)");
                break 'req_done;
            }
            let id = &p["/api/runs/".len()..p.len() - "/resume".len()];
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let hint = body.get("hint").and_then(|v| v.as_str()).map(String::from);
            match shared.runs.resume(id, hint) {
                Ok(run) => {
                    ensure_run_worker(shared);
                    respond_json(&mut stream, "202 Accepted", &json!({ "ok": true, "run_id": run.run_id, "run": run.detail_json() }));
                }
                Err(e) => {
                    let st = if e.contains("unknown run") { "404 Not Found" } else { "409 Conflict" };
                    respond_err(&mut stream, st, &e);
                }
            }
        }
        ("POST", p) if p.starts_with("/api/runs/") && p.ends_with("/cancel") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "runs are only accessible from this machine (loopback)");
                break 'req_done;
            }
            let id = &p["/api/runs/".len()..p.len() - "/cancel".len()];
            match shared.runs.cancel(id) {
                Ok(run) => respond_json(&mut stream, "200 OK", &json!({ "ok": true, "run_id": run.run_id, "run": run.detail_json() })),
                Err(e) => respond_err(&mut stream, "404 Not Found", &e),
            }
        }
        ("GET", "/api/models") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "settings are only accessible from this machine (loopback)");
                break 'req_done;
            }
            crate::model::agy_models(query_param(query, "refresh").is_some_and(|v| v == "1"));
            let snap = config_snapshot(shared);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "provider": snap["provider"],
                    "model_id": snap["model_id"],
                    "default": AGY_DEFAULT_MODEL,
                    "available": snap["antigravity_available"],
                    "models": snap["antigravity_models"],
                    "issue": snap["antigravity_issue"],
                }),
            );
        }
        ("GET", "/api/config") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "settings are only accessible from this machine (loopback)");
                break 'req_done;
            }
            respond_json(&mut stream, "200 OK", &config_snapshot(shared));
        }
        ("POST", "/api/config") => {
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "changing settings is only allowed from this machine (loopback)");
                break 'req_done;
            }
            if shared.busy.load(Ordering::SeqCst) > 0 {
                respond_err(&mut stream, "409 Conflict", "an ask is still running; change settings after it finishes");
                break 'req_done;
            }
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            match apply_config(shared, &body) {
                Ok(snap) => respond_json(&mut stream, "200 OK", &snap),
                Err(e) => respond_err(&mut stream, "400 Bad Request", &e),
            }
        }
        ("GET", "/api/update") => respond_json(&mut stream, "200 OK", &shared.updater.snapshot()),
        ("POST", "/api/update/check") => respond_json(&mut stream, "200 OK", &shared.updater.check_now()),
        ("POST", "/api/update/apply") | ("POST", "/api/update/restart") => {
            // Replacing the program is for whoever sits at this machine: a
            // loopback Host only, whatever OPENRO_ALLOWED_HOSTS names.
            if !host_allowed(&req.host, &[]) {
                respond_err(&mut stream, "403 Forbidden", "updating is only allowed from this machine, not through another address");
                break 'req_done;
            }
            if path == "/api/update/apply" {
                match shared.updater.apply() {
                    Ok(v) => {
                        let mut snap = shared.updater.snapshot();
                        snap["applied"] = json!(v);
                        respond_json(&mut stream, "200 OK", &snap);
                    }
                    Err(e) => respond_err(&mut stream, "409 Conflict", &e),
                }
            } else if !shared.updater.snapshot()["staged"].is_string() {
                respond_err(&mut stream, "409 Conflict", "no update is waiting for a restart");
            } else if shared.busy.load(Ordering::SeqCst) > 0 {
                respond_err(&mut stream, "409 Conflict", "an ask is still running; restart when it has finished");
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
            let active = shared.active_chat.lock().unwrap().clone();
            let can_undo = shared.undoable.lock().unwrap().contains(&active);
            let can_redo = shared.redoable.lock().unwrap().contains(&active);
            let (embed_running, embed_studio_id) = match shared.studio_embed.try_lock() {
                Ok(guard) => {
                    let st = guard.state();
                    (st.embedded, st.studio_id)
                }
                Err(_) => (false, None),
            };
            let active_lib_tools = shared.library.lock().unwrap().active_tools();
            let active_niche = shared.market_taste.lock().unwrap().active_niche().to_string();
            let plugin_connected = shared.studio_plugin.is_connected();
            let pending_proposals = shared.proposals.lock().unwrap().pending_count();
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "studio_connected": info.studio_connected,
                    "model_configured": info.model_configured,
                    "workspace_mode": info.workspace_mode,
                    "model_id": info.model_id,
                    "model_base_url": info.model_base_url,
                    "model_provider": if crate::model::is_agy_url(&info.model_base_url) { "antigravity" } else { "openai" },
                    "antigravity_issue": crate::model::agy_last_issue().map(|i| {
                        json!({ "kind": i.kind, "message": i.message, "resets_in": i.resets_in, "at": i.at_unix })
                    }),
                    "is_loopback_model": info.is_loopback_model,
                    "game_name": shared.game_name,
                    "version": env!("CARGO_PKG_VERSION"),
                    "ropilot_v0100": true,
                    "proposals_api": true,
                    "plugin_bridge": true,
                    "plugin_connected": plugin_connected,
                    "pending_proposals": pending_proposals,
                    "sse_build_stream": true,
                    "assets_api": true,
                    "studio_stream": true,
                    "studio_embed": true,
                    "studio_mjpeg": true,
                    "library_api": true,
                    "cc0_sfx_library": true,
                    "audio_presets_tiered": true,
                    "youtube_music_adapter": true,
                    "market_taste_api": true,
                    "active_niche": active_niche,
                    "embedded_running": embed_running,
                    "studio_id": embed_studio_id,
                    "studio_tools": crate::agent::OFFICIAL_STUDIO_MCP_TOOLS,
                    "active_library_tools": active_lib_tools,
                    "ide_api": true,
                    "ide_v080": true,
                    "rojo_export": true,
                    "can_undo": can_undo,
                    "can_redo": can_redo,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "skills_count": crate::skills::count(),
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
            // can_undo / can_redo: active chat's undo & redo stacks.
            let mut body = shared.chats_json();
            let active = shared.active_chat.lock().unwrap().clone();
            body["can_undo"] = json!(shared.undoable.lock().unwrap().contains(&active));
            body["can_redo"] = json!(shared.redoable.lock().unwrap().contains(&active));
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
                respond_err(&mut stream, "404 Not Found", "chat not found");
            }
        }
        ("GET", "/api/place-tree") => {
            match shared.agent.try_lock() {
                Ok(mut guard) => match guard.as_mut() {
                    Some(agent) => {
                        let rev = agent.local_place_revision();
                        if let Some(r) = rev {
                            if let Some((Some(cached_rev), bytes)) =
                                shared.cached_place_tree_bytes.lock().unwrap().as_ref()
                            {
                                if *cached_rev == r {
                                    let bytes = Arc::clone(bytes);
                                    drop(guard);
                                    respond_bytes(
                                        &mut stream,
                                        "200 OK",
                                        "application/json",
                                        "no-store",
                                        &bytes,
                                    );
                                    break 'req_done;
                                }
                            }
                        }
                        match agent.place_tree_bytes() {
                            Ok(raw) => {
                                let bytes = Arc::new(raw);
                                *shared.cached_place_tree_bytes.lock().unwrap() =
                                    Some((rev, Arc::clone(&bytes)));
                                drop(guard);
                                respond_bytes(
                                    &mut stream,
                                    "200 OK",
                                    "application/json",
                                    "no-store",
                                    &bytes,
                                );
                            }
                            Err(e) => respond_err(&mut stream, "502 Bad Gateway", &e),
                        }
                    }
                    None => respond_err(&mut stream, "503 Service Unavailable", "Studio not connected"),
                },
                Err(_) => {
                    // Agent mutex is held by an active build/chat turn (#31): return cached
                    // or single-flight on-disk LocalPlace bytes immediately without thundering-herd loads.
                    let cached = {
                        let mut cache = shared.cached_place_tree_bytes.lock().unwrap();
                        if cache.is_none() {
                            if let Some(dir) = shared.data_dir.as_ref().filter(|d| d.join("place.json").is_file()) {
                                let bytes = Arc::new(LocalPlace::open(dir.clone(), shared.game_name.clone()).tree_bytes());
                                *cache = Some((None, bytes));
                            }
                        }
                        cache.as_ref().map(|(_, b)| Arc::clone(b))
                    };
                    if let Some(bytes) = cached {
                        let out = if bytes.len() > 2 && bytes.ends_with(b"}") {
                            let mut buf = Vec::with_capacity(bytes.len() + 28);
                            buf.extend_from_slice(&bytes[..bytes.len() - 1]);
                            buf.extend_from_slice(br#","cached":true,"busy":true}"#);
                            buf
                        } else {
                            br#"{"scripts":{},"instances":{},"cached":true,"busy":true}"#.to_vec()
                        };
                        respond_bytes(&mut stream, "200 OK", "application/json", "no-store", &out);
                    } else {
                        respond_bytes(
                            &mut stream,
                            "200 OK",
                            "application/json",
                            "no-store",
                            br#"{"scripts":{},"instances":{},"cached":true,"busy":true}"#,
                        );
                    }
                }
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
                respond_err(&mut stream, "400 Bad Request", "empty message");
                break 'req_done;
            }
            let mode = body
                .get("mode")
                .and_then(|v| v.as_str())
                .map(AskMode::parse)
                .unwrap_or(AskMode::Agent);
            let reasoning = body
                .get("reasoning")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let context_paths: Vec<String> = body
                .get("context_paths")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .take(32)
                        .collect()
                })
                .unwrap_or_default();
            let review_diffs = body
                .get("review_diffs")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let opts = AskOptions {
                mode,
                reasoning,
                context_paths,
                review_diffs,
            };

            // The chat this ask belongs to is fixed NOW, before the
            // ask runs: every line it produces — the user's words,
            // the narration, the failure — is filed into THIS chat,
            // even if the user switches chats while it runs.
            let chat_id = shared.active_chat.lock().unwrap().clone();
            shared.append_lines_to(&chat_id, &[format!("You: {message}")]);
            let req_project_root = body
                .get("project_root")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let req_project_id = body
                .get("project_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if body.get("async").and_then(|v| v.as_bool()).unwrap_or(false) {
                let run_mode = body
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("plan_execute")
                    .to_string();
                let max_steps = body.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(40) as usize;
                let (resolved_dir, resolved_id) = resolve_project_for_agent(
                    shared,
                    req_project_id.as_deref(),
                    req_project_root.as_deref(),
                );
                let run = shared.runs.enqueue(crate::agent::runs::EnqueueRun {
                    chat_id,
                    ask: message,
                    mode: run_mode,
                    project_id: resolved_id.or(req_project_id),
                    project_root: resolved_dir.map(|d| d.to_string_lossy().to_string()),
                    max_steps,
                });
                ensure_run_worker(shared);
                respond_json(
                    &mut stream,
                    "202 Accepted",
                    &json!({ "ok": true, "async": true, "run_id": run.run_id, "run": run.detail_json() }),
                );
                break 'req_done;
            }
            let mut guard = shared.agent.lock().unwrap();
            if let Some(agent) = guard.as_mut() {
                sync_agent_project(shared, agent, req_project_id.as_deref(), req_project_root.as_deref());
            }
            match guard.as_mut() {
                None => {
                    let error = "no model key configured: set OPENRO_MODEL_API_KEY to enable chat";
                    shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
                    respond_err(&mut stream, "503 Service Unavailable", error);
                }
                Some(agent) if mode == AskMode::Ask => {
                    match agent.run_ask_with_options(&chat_id, &message, opts) {
                        Ok(outcome) => {
                            shared.append_lines_to(&chat_id, &outcome.thread);
                            respond_json(
                                &mut stream,
                                "200 OK",
                                &json!({
                                    "ok": true,
                                    "mode": "ask",
                                    "summary": outcome.summary,
                                    "thread": outcome.thread,
                                    "transcript": outcome.transcript,
                                    "file_diffs": [],
                                }),
                            );
                        }
                        Err(f) => {
                            shared.append_lines_to(&chat_id, &f.thread);
                            shared.append_lines_to(&chat_id, &[format!("OpenRO: {}", f.message)]);
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
                                &json!({ "ok": false, "mode": "ask", "error": f.message }),
                            );
                        }
                    }
                }
                Some(agent) if mode == AskMode::Plan || review_diffs => {
                    match agent.run_ask_staged(&chat_id, &message, opts) {
                        Ok(staged) if staged.outcome.refused => {
                            shared.append_lines_to(&chat_id, &staged.outcome.thread);
                            respond_json(
                                &mut stream,
                                "502 Bad Gateway",
                                &json!({ "ok": false, "mode": mode.as_str(), "error": staged.outcome.summary }),
                            );
                        }
                        Ok(staged) => {
                            shared.append_lines_to(&chat_id, &staged.outcome.thread);
                            let proposal = shared.proposals.lock().unwrap().create(
                                chat_id.clone(),
                                message.clone(),
                                staged.outcome.summary.clone(),
                                mode.as_str().to_string(),
                                staged.before_tree,
                                staged.after_tree,
                                staged.diagnostics,
                                staged.outcome.skills_used.clone(),
                            );
                            let file_diffs_json: Vec<Value> = proposal
                                .file_diffs
                                .iter()
                                .map(ScriptFileDiff::to_json)
                                .collect();
                            respond_json(
                                &mut stream,
                                "200 OK",
                                &json!({
                                    "ok": true,
                                    "mode": mode.as_str(),
                                    "staged": true,
                                    "proposal": proposal.to_json(),
                                    "file_diffs": file_diffs_json,
                                    "diff": proposal.diff.to_json(),
                                    "diagnostics": proposal.diagnostics,
                                    "summary": staged.outcome.summary,
                                    "thread": staged.outcome.thread,
                                    "transcript": staged.outcome.transcript,
                                }),
                            );
                        }
                        Err(f) => {
                            shared.append_lines_to(&chat_id, &f.thread);
                            shared.append_lines_to(&chat_id, &[format!("OpenRO: {}", f.message)]);
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
                                &json!({ "ok": false, "mode": mode.as_str(), "error": f.message }),
                            );
                        }
                    }
                }
                Some(agent) => {
                    let before_tree = agent.place_tree().ok();
                    match agent.run_ask_with_options(&chat_id, &message, opts) {
                        Ok(outcome) if outcome.refused => {
                            // Refused, not built: the plain reason is the
                            // answer, the model's success claim is not shown,
                            // and no pitch follows.
                            shared.append_lines_to(&chat_id, &outcome.thread);
                            respond_err(&mut stream, "502 Bad Gateway", &outcome.summary);
                        }
                        Ok(outcome) => {
                            shared.append_lines_to(&chat_id, &outcome.thread);
                            let mut snapshot_id = Value::Null;
                            let mut diff_val = Value::Null;
                            let mut file_diffs_json: Vec<Value> = Vec::new();
                            // Only an ask that really changed the game records
                            // what it built and pitches the follow-up set (once
                            // per context). A zero-change ask gets no pitch.
                            if outcome.changed {
                                shared.note_built(&chat_id, &outcome.skills_used);
                                if let Ok(after_tree) = agent.place_tree() {
                                    let explicit = before_tree
                                        .as_ref()
                                        .map(|b| compute_diff(b, &after_tree));
                                    if let Some(b) = before_tree.as_ref() {
                                        file_diffs_json = compute_file_diffs(b, &after_tree)
                                            .iter()
                                            .map(ScriptFileDiff::to_json)
                                            .collect();
                                    }
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
                                    "mode": "agent",
                                    "summary": outcome.summary,
                                    "thread": outcome.thread,
                                    "transcript": outcome.transcript,
                                    "snapshot_id": snapshot_id,
                                    "diff": diff_val,
                                    "file_diffs": file_diffs_json,
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
                            respond_err(&mut stream, status, &f.message);
                        }
                    }
                }
            }
        }
        ("POST", "/api/ideas") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let has_custom_prompt = body
                .get("prompt")
                .and_then(|v| v.as_str())
                .is_some_and(|p| !p.trim().is_empty());
            let has_custom_project = body
                .get("project_id")
                .and_then(|v| v.as_str())
                .is_some_and(|p| !p.trim().is_empty())
                || shared.active_project_dir().is_some()
                || shared
                    .agent
                    .try_lock()
                    .ok()
                    .and_then(|g| g.as_ref().and_then(|a| a.effective_project_root()))
                    .is_some();
            if has_custom_prompt || has_custom_project {
                respond_json(
                    &mut stream,
                    "200 OK",
                    &json!({
                        "ok": true,
                        "ideas": [],
                        "built_titles": shared.built_titles_for(&chat_id),
                        "pitched_new": false,
                        "custom_project": true,
                    }),
                );
                break 'req_done;
            }
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
                    break 'req_done;
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
                respond_err(&mut stream, "400 Bad Request", &error);
                break 'req_done;
            }
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                None => {
                    let error =
                        "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server";
                    shared.append_lines_to(&chat_id, &[format!("OpenRO: {error}")]);
                    respond_err(&mut stream, "503 Service Unavailable", error);
                }
                Some(agent) => {
                    let before_tree = agent.place_tree().ok();
                    match agent.build_idea(&chat_id, &id) {
                    Ok(outcome) => {
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        shared.note_built(&chat_id, &outcome.skills_used);
                        let stored_assets = all_stored_assets(shared);
                        if !stored_assets.is_empty() {
                            agent.bind_assets(&stored_assets);
                        }
                        let mut snapshot_id = Value::Null;
                        let mut diff_val = Value::Null;
                        let mut built_tree: Option<Value> = None;
                        if let Ok(after_tree) = agent.place_tree() {
                            let explicit = before_tree
                                .as_ref()
                                .map(|b| compute_diff(b, &after_tree));
                            let snap = shared.snapshots.lock().unwrap().record(
                                format!("Blueprint: {}", idea.title),
                                "idea",
                                Some(chat_id.clone()),
                                after_tree.clone(),
                                explicit,
                            );
                            snapshot_id = json!(snap.id);
                            diff_val = snap.diff.to_json();
                            built_tree = Some(after_tree);
                        }
                        let template_proof = if let Some(ref fp) = idea.feel_profile {
                            let (mut scripts_map, mut instances_map) = match &agent.studio {
                                Some(PlaceBackend::Local(local)) => (
                                    local.scripts().clone(),
                                    local.instances().clone(),
                                ),
                                _ => (
                                    std::collections::BTreeMap::new(),
                                    std::collections::BTreeMap::new(),
                                ),
                            };
                            if scripts_map.is_empty() {
                                if let Some(ref tree) = built_tree {
                                    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
                                        for (k, v) in obj {
                                            if let Some(src) = v.as_str() {
                                                scripts_map.insert(k.clone(), src.to_string());
                                            }
                                        }
                                    }
                                    if let Some(obj) = tree.get("instances").and_then(Value::as_object) {
                                        for (k, v) in obj {
                                            instances_map.insert(k.clone(), v.clone());
                                        }
                                    }
                                }
                            }
                            let gui_safe_area = match crate::gui_designer::template_screen_doc(idea.id) {
                                Ok(doc) => {
                                    let _ = shared.gui_store.lock().unwrap().save(fp.gui_screen_slug, doc.clone());
                                    crate::gui_designer::analyze_safe_areas(&doc).to_json()
                                }
                                Err(e) => json!({ "passed": false, "warning_count": 1, "error": e }),
                            };
                            let economy_sim = crate::playtest::evaluate_economy_sim(&scripts_map, &json!({}));
                            let mut sandbox_rep = match crate::sandbox::run_local_place_sandbox(
                                idea.title,
                                &scripts_map,
                                &instances_map,
                                &json!({}),
                            ) {
                                Ok(r) => r,
                                Err(e) => json!({ "ok": false, "passed": false, "error_count": 1, "error": e }),
                            };
                            let proof_root = shared
                                .active_project_dir()
                                .or_else(|| shared.data_dir.clone())
                                .unwrap_or_else(|| std::env::temp_dir().join("openro-sandbox-proofs"));
                            if let Ok(proof_path) =
                                crate::sandbox::persist_sandbox_proof(&proof_root, idea.id, &sandbox_rep)
                            {
                                if let Some(obj) = sandbox_rep.as_object_mut() {
                                    obj.insert(
                                        "proof_path".to_string(),
                                        json!(proof_path.to_string_lossy().into_owned()),
                                    );
                                }
                            }
                            Some(json!({
                                "sandbox": sandbox_rep,
                                "economy_sim": economy_sim,
                                "gui_safe_area": gui_safe_area,
                            }))
                        } else {
                            None
                        };
                        let (next, pitch) = shared.pitch_for(
                            &chat_id,
                            "OpenRO: That landed. Next, I could add:",
                        );
                        if !pitch.is_empty() {
                            shared.append_lines_to(&chat_id, &pitch);
                        }
                        let mut resp = json!({
                            "ok": true,
                            "summary": outcome.summary,
                            "thread": outcome.thread,
                            "transcript": outcome.transcript,
                            "ideas": ideas_json(&next),
                            "snapshot_id": snapshot_id,
                            "diff": diff_val,
                        });
                        if let Some(proof) = template_proof {
                            resp["template_proof"] = proof;
                        }
                        respond_json(&mut stream, "200 OK", &resp);
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
                        respond_err(&mut stream, status, &f.message);
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
                                "can_undo": agent.can_undo_chat(&chat_id),
                                "can_redo": agent.can_redo_chat(&chat_id),
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
                        respond_err(&mut stream, "200 OK", &e);
                    }
                    Err(e) => {
                        // A failed revert is part of the chat's record
                        // too — never a silent API-only error. The set
                        // is kept, and the line says so.
                        shared.append_lines_to(&chat_id, &[format!(
                            "OpenRO: Revert did not finish: {e}"
                        )]);
                        respond_err(&mut stream, "502 Bad Gateway", &e);
                    }
                }
                }
                None => respond_err(&mut stream, "503 Service Unavailable", "Studio not connected"),
            }
        }
        _ => respond_err(&mut stream, "404 Not Found", "not found"),
    }
        } // end 'req_done
        if !TL_KEEP_ALIVE.get() {
            break;
        }
    }
    TL_KEEP_ALIVE.set(false);
    let _ = slot_opt;
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

fn extract_studio_gui_slug(body: &Value) -> Result<String, String> {
    if let Some(slug_val) = body.get("slug") {
        if !slug_val.is_null() && !slug_val.is_string() {
            return Err("gui slug must be a string".to_string());
        }
    }
    let raw = body
        .get("slug")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            body.pointer("/doc/name")
                .or_else(|| body.get("name"))
                .and_then(Value::as_str)
                .map(crate::projects::slug)
        })
        .or_else(|| {
            body.get("preset")
                .and_then(Value::as_str)
                .map(crate::projects::slug)
        })
        .unwrap_or_default();
    crate::gui_designer::validate_slug(&raw)
}

fn resolve_studio_gui_project_dir(shared: &Arc<Shared>, project_id: Option<&str>) -> Option<PathBuf> {
    if let Some(pid) = project_id.map(str::trim).filter(|s| !s.is_empty()) {
        return shared.projects.lock().unwrap().find(pid).map(|r| r.dir);
    }
    let from_agent = {
        let guard = shared.agent.lock().unwrap();
        guard.as_ref().and_then(|a| match &a.studio {
            Some(PlaceBackend::Local(_)) => None,
            _ => a.project_root.clone(),
        })
    };
    from_agent.or_else(|| shared.active_project_dir())
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
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
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
            let mut st_ev = StudioActionInput::new(
                StudioActionKind::Restored,
                format!("Restored snapshot '{}' ({})", target.label, diff.summary()),
            );
            st_ev.path = "Workspace".to_string();
            st_ev.class_name = "DataModel".to_string();
            st_ev.instance_count = new_snap.instance_count;
            st_ev.script_count = new_snap.script_count;
            shared.studio_stream.record_action(st_ev);
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
        Err(e) => respond_err(stream, "502 Bad Gateway", &e),
    }
}

fn handle_snapshot_restore_hooks(stream: &mut TcpStream, shared: &Shared, snap_id: &str) {
    let _busy = BusyGuard::new(&shared.busy);
    if !snap_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) || snap_id.contains("..") {
        respond_err(stream, "400 Bad Request", "invalid snapshot id");
        return;
    }
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
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let current_tree = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    let (next_tree, restored_files, modified_files, hooks_restored) =
        crate::playtest::studio_restore_hooks_tree(&target.tree, &current_tree);
    match agent.apply_place_tree(&next_tree) {
        Ok(diff) => {
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let after_tree = agent.place_tree().unwrap_or(next_tree);
            let _ = shared.snapshots.lock().unwrap().record(
                format!("Restored hooks from {}", target.label),
                "restore-hooks",
                Some(chat_id),
                after_tree.clone(),
                Some(diff),
            );
            let after = crate::playtest::studio_hook_integrity_from_trees(
                Some(target.id.as_str()),
                Some(target.label.as_str()),
                Some(&target.tree),
                &after_tree,
            );
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "restored_id": target.id,
                    "label": target.label,
                    "hooks_restored": hooks_restored,
                    "restored_files": restored_files,
                    "modified_files": modified_files,
                    "preserved_new_scripts": after["preserved_new_scripts"],
                    "after": after,
                }),
            );
        }
        Err(e) => respond_err(stream, "502 Bad Gateway", &e),
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
                respond_err(stream, "400 Bad Request", "invalid UTF-8 in .rbxlx XML payload");
                return;
            }
        }
    } else {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
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
            respond_err(stream, "400 Bad Request", "missing string field 'xml' in import payload");
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
            respond_err(stream, "400 Bad Request", &e);
            return;
        }
    };

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
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
            let mut st_ev = StudioActionInput::new(
                StudioActionKind::Imported,
                format!(
                    "Imported {} instance(s) and {} script(s) from .rbxlx",
                    snap.instance_count, snap.script_count
                ),
            );
            st_ev.path = "Workspace".to_string();
            st_ev.class_name = "DataModel".to_string();
            st_ev.instance_count = snap.instance_count;
            st_ev.script_count = snap.script_count;
            shared.studio_stream.record_action(st_ev);
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
        Err(e) => respond_err(stream, "502 Bad Gateway", &e),
    }
}

fn handle_place_instance(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let Some(path) = body.get("path").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
        respond_err(stream, "400 Bad Request", "missing string 'path' (e.g. Workspace.JumpPad)");
        return;
    };
    let delete = body.get("delete").and_then(|v| v.as_bool()).unwrap_or(false);
    let class_name = body.get("class_name").and_then(|v| v.as_str());
    let properties = body.get("properties").cloned().unwrap_or_else(|| json!({}));

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
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
            agent.record_direct_edit(&chat_id, &before, &after);
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
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
                    "can_undo": can_undo,
                    "can_redo": can_redo,
                    "snapshot_id": snap.id,
                    "diff": diff.to_json(),
                }),
            );
        }
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn parse_opt_vec3(val: Option<&Value>) -> Option<[f64; 3]> {
    let arr = val?.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    let x = arr[0].as_f64()?;
    let y = arr[1].as_f64()?;
    let z = arr[2].as_f64()?;
    if x.is_finite() && y.is_finite() && z.is_finite() {
        Some([x, y, z])
    } else {
        None
    }
}

fn handle_place_create(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let preset = body
        .get("preset")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("block");
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let props = body.get("properties");
    let position = parse_opt_vec3(body.get("position"))
        .or_else(|| parse_opt_vec3(props.and_then(|p| p.get("Position"))));
    let size = parse_opt_vec3(body.get("size"))
        .or_else(|| parse_opt_vec3(props.and_then(|p| p.get("Size"))));
    let color = body
        .get("color")
        .and_then(|v| v.as_str())
        .or_else(|| props.and_then(|p| p.get("Color")).and_then(|v| v.as_str()));

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before = agent
        .place_tree()
        .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    match agent.create_part_preset(&chat_id, preset, name, position, size, color) {
        Ok(created) => {
            let after = agent
                .place_tree()
                .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let diff = compute_diff(&before, &after);
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
            let label = format!("Added {} ({})", created.path, created.preset);
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "edit",
                Some(chat_id.clone()),
                after,
                Some(diff.clone()),
            );
            shared.events.publish(
                "create",
                chat_id,
                "done",
                label,
                diff.summary(),
                None,
                Some(created.path.clone()),
                100,
            );
            let mut out = created.to_json();
            if let Some(obj) = out.as_object_mut() {
                obj.insert("can_undo".to_string(), json!(can_undo));
                obj.insert("can_redo".to_string(), json!(can_redo));
                obj.insert("snapshot_id".to_string(), json!(snap.id));
                obj.insert("diff".to_string(), diff.to_json());
            }
            respond_json(stream, "200 OK", &out);
        }
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn handle_place_duplicate(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let Some(path) = body
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        respond_err(stream, "400 Bad Request", "missing string 'path' to duplicate");
        return;
    };

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before = agent
        .place_tree()
        .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    match agent.duplicate_instance(&chat_id, path) {
        Ok(dup) => {
            let after = agent
                .place_tree()
                .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let diff = compute_diff(&before, &after);
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
            let label = format!("Duplicated {} -> {}", dup.source_path, dup.new_path);
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "edit",
                Some(chat_id.clone()),
                after,
                Some(diff.clone()),
            );
            shared.events.publish(
                "duplicate",
                chat_id,
                "done",
                label,
                diff.summary(),
                None,
                Some(dup.new_path.clone()),
                100,
            );
            let mut out = dup.to_json();
            if let Some(obj) = out.as_object_mut() {
                obj.insert("can_undo".to_string(), json!(can_undo));
                obj.insert("can_redo".to_string(), json!(can_redo));
                obj.insert("snapshot_id".to_string(), json!(snap.id));
                obj.insert("diff".to_string(), diff.to_json());
            }
            respond_json(stream, "200 OK", &out);
        }
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn handle_place_delete(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let Some(path) = body
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        respond_err(stream, "400 Bad Request", "missing string 'path' to delete");
        return;
    };

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before = agent
        .place_tree()
        .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    match agent.delete_at_path(&chat_id, path) {
        Ok(deleted) => {
            let after = agent
                .place_tree()
                .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let diff = compute_diff(&before, &after);
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
            let label = format!("Deleted {}", deleted.path);
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "edit",
                Some(chat_id.clone()),
                after,
                Some(diff.clone()),
            );
            shared.events.publish(
                "delete",
                chat_id,
                "done",
                label,
                diff.summary(),
                None,
                Some(deleted.path.clone()),
                100,
            );
            let mut out = deleted.to_json();
            if let Some(obj) = out.as_object_mut() {
                obj.insert("can_undo".to_string(), json!(can_undo));
                obj.insert("can_redo".to_string(), json!(can_redo));
                obj.insert("snapshot_id".to_string(), json!(snap.id));
                obj.insert("diff".to_string(), diff.to_json());
            }
            respond_json(stream, "200 OK", &out);
        }
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn handle_place_script(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let Some(path) = body
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        respond_err(stream, "400 Bad Request", "missing string 'path' (e.g. ServerScriptService.Leaderboard)");
        return;
    };
    let Some(source) = body.get("source").and_then(|v| v.as_str()) else {
        respond_err(stream, "400 Bad Request", "missing string 'source' for Luau script");
        return;
    };

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before = agent
        .place_tree()
        .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    match agent.update_script_source(&chat_id, path, source) {
        Ok(updated) => {
            let after = agent
                .place_tree()
                .unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
            let diff = compute_diff(&before, &after);
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
            let label = format!("Edited script {}", updated.path);
            let snap = shared.snapshots.lock().unwrap().record(
                label.clone(),
                "edit",
                Some(chat_id.clone()),
                after,
                Some(diff.clone()),
            );
            shared.events.publish(
                "script_edit",
                chat_id,
                "done",
                label,
                diff.summary(),
                None,
                Some(updated.path.clone()),
                100,
            );
            let place_json = updated.place_report.to_json();
            let diags = place_json
                .get("diagnostics")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let mut out = updated.to_json();
            if let Some(obj) = out.as_object_mut() {
                obj.insert("errors".to_string(), json!(updated.place_report.error_count()));
                obj.insert(
                    "warnings".to_string(),
                    json!(updated.place_report.warning_count()),
                );
                obj.insert("passed".to_string(), json!(updated.place_report.is_ok()));
                obj.insert("diagnostics".to_string(), diags);
                obj.insert("can_undo".to_string(), json!(can_undo));
                obj.insert("can_redo".to_string(), json!(can_redo));
                obj.insert("snapshot_id".to_string(), json!(snap.id));
                obj.insert("diff".to_string(), diff.to_json());
            }
            respond_json(stream, "200 OK", &out);
        }
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn handle_redo(stream: &mut TcpStream, shared: &Shared) {
    let _busy = BusyGuard::new(&shared.busy);
    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before_tree = agent.place_tree().ok();
    match agent.redo_chat(&chat_id) {
        Ok((restored, skills)) => {
            if !skills.is_empty() {
                shared.note_built(&chat_id, &skills);
            }
            let can_undo = agent.can_undo_chat(&chat_id);
            let can_redo = agent.can_redo_chat(&chat_id);
            let mut snapshot_id = Value::Null;
            let mut diff_val = Value::Null;
            let mut instance_count = 0usize;
            let mut script_count = 0usize;
            if let Ok(after_tree) = agent.place_tree() {
                instance_count = after_tree
                    .get("instances")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                script_count = after_tree
                    .get("scripts")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let explicit = before_tree
                    .as_ref()
                    .map(|b| compute_diff(b, &after_tree));
                let snap = shared.snapshots.lock().unwrap().record(
                    format!("Redo ({} change(s))", restored.len()),
                    "redo",
                    Some(chat_id.clone()),
                    after_tree,
                    explicit,
                );
                snapshot_id = json!(snap.id);
                diff_val = snap.diff.to_json();
            }
            let label = format!("{} change(s) re-applied", restored.len());
            shared.events.publish(
                "redo",
                chat_id.clone(),
                "restored",
                "Redo complete",
                label.clone(),
                None,
                None,
                100,
            );
            shared.append_lines_to(
                &chat_id,
                &[format!("OpenRO: Redid the undone step: {label}.")],
            );
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "restored": restored,
                    "label": label,
                    "instance_count": instance_count,
                    "script_count": script_count,
                    "can_undo": can_undo,
                    "can_redo": can_redo,
                    "snapshot_id": snapshot_id,
                    "diff": diff_val,
                }),
            );
        }
        Err(e) if e == crate::agent::NOTHING_TO_REDO => respond_json(
            stream,
            "200 OK",
            &json!({
                "ok": false,
                "error": e,
                "can_undo": agent.can_undo_chat(&chat_id),
                "can_redo": false,
            }),
        ),
        Err(e) => respond_err(stream, "502 Bad Gateway", &e),
    }
}

fn handle_export_bundle(stream: &mut TcpStream, shared: &Shared) {
    let stored_meta = all_stored_assets(shared);
    let pack_assets = all_ship_pack_assets(shared);
    let (extra_files, extra_manifest) = collect_generated_media_for_bundle(shared);
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    if !stored_meta.is_empty() {
        agent.bind_assets(&stored_meta);
    }
    let bundle_res = match agent.studio.as_mut() {
        Some(PlaceBackend::Local(local)) => {
            if local.game_name().trim().is_empty() && !shared.game_name.trim().is_empty() {
                local.set_game_name(shared.game_name.clone());
            }
            crate::rojo_export::build_rojo_ship_pack_with_extras(
                local,
                &pack_assets,
                &extra_files,
                &extra_manifest,
            )
        }
        Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => match agent.place_tree() {
            Ok(tree) => {
                let mut tmp = LocalPlace::in_memory(shared.game_name.clone());
                tmp.replace_tree(&tree);
                crate::rojo_export::build_rojo_ship_pack_with_extras(
                    &tmp,
                    &pack_assets,
                    &extra_files,
                    &extra_manifest,
                )
            }
            Err(e) => Err(e),
        },
        None => Err("Studio not connected".to_string()),
    };
    match bundle_res {
        Ok(bundle) => {
            let fname = bundle.filename_slug();
            respond_download_bytes(stream, "application/zip", &fname, &bundle.zip_bytes);
        }
        Err(e) => respond_err(stream, "502 Bad Gateway", &e),
    }
}

fn handle_skill_build(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
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
        respond_err(stream, "503 Service Unavailable", error);
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
            respond_err(stream, "502 Bad Gateway", &f.message);
        }
    }
}

fn handle_luau_check(stream: &mut TcpStream, shared: &Shared, is_post: bool, raw_body: &[u8]) {
    let tree = if is_post && !raw_body.is_empty() {
        let body: Value = match serde_json::from_slice(raw_body) {
            Ok(v) => v,
            Err(_) => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
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
            Some(agent) => {
                if let Some(rev) = agent.local_place_revision() {
                    if let Some((cached_rev, bytes)) = shared.cached_luau_report.lock().unwrap().as_ref() {
                        if *cached_rev == rev {
                            let bytes = Arc::clone(bytes);
                            drop(guard);
                            respond_bytes(stream, "200 OK", "application/json", "no-store", &bytes);
                            return;
                        }
                    }
                    if let Some(report) = agent.analyze_local_luau() {
                        let scripts_checked = agent
                            .place_tree()
                            .ok()
                            .and_then(|t| t.get("scripts").and_then(|s| s.as_object()).map(|m| m.len()))
                            .unwrap_or(0);
                        let mut out = report.to_json();
                        out["passed"] = json!(report.is_ok());
                        out["ok"] = json!(true);
                        out["scripts_checked"] = json!(scripts_checked);
                        let bytes = Arc::new(out.to_string().into_bytes());
                        *shared.cached_luau_report.lock().unwrap() = Some((rev, Arc::clone(&bytes)));
                        drop(guard);
                        respond_bytes(stream, "200 OK", "application/json", "no-store", &bytes);
                        return;
                    }
                }
                match agent.place_tree() {
                    Ok(t) => t,
                    Err(e) => {
                        respond_err(stream, "502 Bad Gateway", &e);
                        return;
                    }
                }
            }
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

/// Read and discard immediately buffered bytes on an oversized request without
/// stalling if the client keeps its write half open.
fn drain_unread_promptly(stream: &mut TcpStream, mut budget: usize) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(20)));
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

pub fn refresh_status_cache(shared: &Shared) {
    let _ = status_info(shared);
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
    let (base_url, model_id, has_key, masked, configured, is_loopback, studio_connected, active_backend, rbxlx_path) =
        match shared.agent.try_lock() {
            Ok(mut guard) => match guard.as_mut() {
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
            },
            Err(_) => {
                // Agent busy (#31): return cached status + persisted config immediately.
                let st = shared.status_cache.lock().unwrap().clone();
                let has = cfg.api_key.as_ref().is_some_and(|k| !k.trim().is_empty());
                (
                    st.model_base_url,
                    st.model_id,
                    has,
                    cfg.api_key.as_deref().map(crate::secrets::mask),
                    st.model_configured,
                    st.is_loopback_model,
                    st.studio_connected,
                    st.workspace_mode,
                    shared.data_dir.as_ref().map(|d| d.join("game.rbxlx").display().to_string()),
                )
            }
        };
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
    let agy_active = crate::model::is_agy_url(&base_url);
    let agy_models: Vec<Value> = agy_models_json();
    let agy_issue = crate::model::agy_last_issue().map(|i| {
        json!({ "kind": i.kind, "message": i.message, "resets_in": i.resets_in, "at": i.at_unix })
    });
    let store = shared
        .data_dir
        .as_deref()
        .map(crate::secrets::SecretStore::for_data_dir);
    let gemini_key = store
        .as_ref()
        .and_then(|s| s.get(crate::secrets::GEMINI_KEY))
        .or_else(|| cfg.gemini_api_key.clone())
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    let openai_img_key = store
        .as_ref()
        .and_then(|s| s.get(crate::secrets::OPENAI_IMAGE_KEY))
        .or_else(|| cfg.openai_image_api_key.clone())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    let opencloud_key = store
        .as_ref()
        .and_then(|s| s.get(crate::secrets::OPENCLOUD_KEY));
    let opencloud_creator = store
        .as_ref()
        .map(crate::opencloud::creator_json)
        .unwrap_or_else(|| json!({}));
    let secrets_dir_shared = std::env::var("OPENRO_SECRETS_DIR")
        .ok()
        .is_some_and(|s| !s.trim().is_empty());
    json!({
        "ok": true,
        "base_url": base_url,
        "model_id": model_id,
        "provider": if agy_active { "antigravity" } else { "openai" },
        "antigravity_available": crate::model::agy_available(),
        "antigravity_default_model": AGY_DEFAULT_MODEL,
        "antigravity_models": agy_models,
        "antigravity_issue": agy_issue,
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
        "auto_install_plugin": crate::studio_plugin::auto_install_pref(shared.data_dir.as_deref()),
        "rbxlx_path": rbxlx_path,
        "detected_gateways": gateways,
        "image_provider": cfg.image_provider.unwrap_or_else(|| "chatgpt".to_string()),
        "has_gemini_api_key": gemini_key.is_some(),
        "gemini_api_key_masked": gemini_key.as_deref().map(crate::secrets::mask),
        "has_openai_image_api_key": openai_img_key.is_some(),
        "openai_image_api_key_masked": openai_img_key.as_deref().map(crate::secrets::mask),
        "has_opencloud_key": opencloud_key.is_some(),
        "opencloud_key_masked": opencloud_key.as_deref().map(crate::secrets::mask),
        "opencloud_creator": opencloud_creator,
        "secrets_dir_shared": secrets_dir_shared,
    })
}

/// Model list for the picker: the cached `agy models` result, else the
/// Hydrogen set. Never runs agy (that is `GET /api/models`).
fn agy_models_json() -> Vec<Value> {
    let list = crate::model::agy_models_cached().unwrap_or_else(crate::model::agy_models_fallback);
    list.into_iter()
        .map(|m| {
            let (family, effort) = match m.label.split_once(" (") {
                Some((f, e)) => (f.to_string(), e.trim_end_matches(')').to_string()),
                None => (m.label.clone(), String::new()),
            };
            json!({ "id": m.id, "label": m.label, "family": family, "effort": effort })
        })
        .collect()
}

const VALID_CONFIG_KEYS: &[&str] = &[
    "provider",
    "base_url",
    "model_id",
    "api_key",
    "clear_api_key",
    "mcp_command",
    "workspace_mode",
    "active_backend",
    "gemini_api_key",
    "clear_gemini_api_key",
    "openai_image_api_key",
    "clear_openai_image_api_key",
    "image_provider",
    "auto_install_plugin",
    "opencloud_api_key",
    "clear_opencloud_api_key",
    "opencloud_creator",
    "opencloud_user_id",
    "opencloud_group_id",
];

fn apply_config(shared: &Shared, body: &Value) -> Result<Value, String> {
    if let Some(obj) = body.as_object() {
        for k in obj.keys() {
            if !VALID_CONFIG_KEYS.contains(&k.as_str()) {
                return Err(format!(
                    "unknown setting key '{k}' (use workspace_mode, provider, model_id, base_url, mcp_command, image_provider, or auto_install_plugin)"
                ));
            }
        }
    }
    let mut cfg = shared.config.lock().unwrap().clone();
    let (active_base, active_model) = {
        let guard = shared.agent.lock().unwrap();
        match guard.as_ref() {
            Some(a) => (Some(a.model.base_url().to_string()), Some(a.model.model_id().to_string())),
            None => (None, None),
        }
    };

    // `provider: "antigravity"` selects the Antigravity CLI (Gemini models);
    // `provider: "openai"` goes back to an OpenAI-compatible endpoint.
    let current_base = cfg.base_url.as_deref().or(active_base.as_deref()).unwrap_or("");
    let was_agy = cfg.provider.as_deref() == Some("antigravity") || crate::model::is_agy_url(current_base);
    let mut explicit_openai = false;
    match body.get("provider").and_then(|v| v.as_str()) {
        Some("antigravity") => {
            cfg.provider = Some("antigravity".to_string());
            cfg.base_url = Some(AGY_BASE_URL.to_string());
            if !was_agy && body.get("model_id").and_then(|v| v.as_str()).is_none_or(|m| m.trim().is_empty()) {
                cfg.model_id = Some(AGY_DEFAULT_MODEL.to_string());
            }
        }
        Some("openai") => {
            explicit_openai = true;
            cfg.provider = Some("openai".to_string());
            if was_agy {
                cfg.base_url = None;
                cfg.model_id = None;
            }
        }
        Some(other) => return Err(format!("invalid provider '{other}' (expected antigravity or openai)")),
        None => {}
    }
    let base_url = match body.get("base_url").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => {
            let trimmed = s.trim();
            if !(trimmed.starts_with("http://") || trimmed.starts_with("https://") || crate::model::is_agy_url(trimmed)) {
                return Err("base_url must start with http://, https:// or agy://".to_string());
            }
            cfg.base_url = Some(trimmed.to_string());
            cfg.provider = Some(if crate::model::is_agy_url(trimmed) { "antigravity" } else { "openai" }.to_string());
            trimmed.to_string()
        }
        _ => cfg
            .base_url
            .clone()
            .or_else(|| if explicit_openai { None } else { active_base.clone() })
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
    };
    if crate::model::is_agy_url(&base_url) {
        cfg.provider = Some("antigravity".to_string());
        cfg.base_url = Some(base_url.clone());
    }

    let model_id = match body.get("model_id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => {
            cfg.model_id = Some(s.trim().to_string());
            s.trim().to_string()
        }
        _ => cfg
            .model_id
            .clone()
            .or(if explicit_openai { None } else { active_model })
            .unwrap_or_else(|| {
                if crate::model::is_agy_url(&base_url) {
                    AGY_DEFAULT_MODEL.to_string()
                } else {
                    DEFAULT_MODEL_ID.to_string()
                }
            }),
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

    if let Some(mode) = body
        .get("workspace_mode")
        .or_else(|| body.get("active_backend"))
        .and_then(|v| v.as_str())
    {
        match mode {
            "auto" | "local_place" | "studio_mcp" => cfg.workspace_mode = Some(mode.to_string()),
            "live_mcp" => cfg.workspace_mode = Some("studio_mcp".to_string()),
            other => return Err(format!("invalid workspace_mode '{other}' (expected auto, local_place, or studio_mcp)")),
        }
    }

    let secret_store = shared
        .data_dir
        .as_deref()
        .map(crate::secrets::SecretStore::for_data_dir);
    if body
        .get("clear_gemini_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        cfg.gemini_api_key = None;
        if let Some(s) = &secret_store {
            s.clear(crate::secrets::GEMINI_KEY)?;
        }
    } else if let Some(k) = body.get("gemini_api_key").and_then(|v| v.as_str()) {
        let t = k.trim();
        if !t.is_empty() {
            if let Some(s) = &secret_store {
                s.set(crate::secrets::GEMINI_KEY, t)?;
                cfg.gemini_api_key = None;
            } else {
                cfg.gemini_api_key = Some(t.to_string());
            }
        }
    }
    if body
        .get("clear_openai_image_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        cfg.openai_image_api_key = None;
        if let Some(s) = &secret_store {
            s.clear(crate::secrets::OPENAI_IMAGE_KEY)?;
        }
    } else if let Some(k) = body.get("openai_image_api_key").and_then(|v| v.as_str()) {
        let t = k.trim();
        if !t.is_empty() {
            if let Some(s) = &secret_store {
                s.set(crate::secrets::OPENAI_IMAGE_KEY, t)?;
                cfg.openai_image_api_key = None;
            } else {
                cfg.openai_image_api_key = Some(t.to_string());
            }
        }
    }
    if body
        .get("clear_opencloud_api_key")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(s) = &secret_store {
            s.clear(crate::secrets::OPENCLOUD_KEY)?;
        }
    } else if let Some(k) = body.get("opencloud_api_key").and_then(|v| v.as_str()) {
        let t = k.trim();
        if !t.is_empty() {
            let Some(s) = &secret_store else {
                return Err("saving Open Cloud keys requires a persistent data directory".to_string());
            };
            s.set(crate::secrets::OPENCLOUD_KEY, t)?;
        }
    }
    if let Some(prov) = body.get("image_provider").and_then(|v| v.as_str()) {
        match prov {
            "chatgpt" | "auto" | "gemini" | "openai" => cfg.image_provider = Some(prov.to_string()),
            other => return Err(format!("invalid image_provider '{other}' (expected chatgpt, auto, gemini, or openai)")),
        }
    }

    if let Some(auto_plugin) = body.get("auto_install_plugin").and_then(|v| v.as_bool()) {
        crate::studio_plugin::set_auto_install_pref(shared.data_dir.as_deref(), auto_plugin)?;
    }

    let backend_changed = body.get("workspace_mode").is_some()
        || body.get("active_backend").is_some()
        || body.get("mcp_command").is_some();
    let has_existing_backend = shared
        .agent
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|a| a.studio.is_some());

    let desired_mode = cfg.workspace_mode.as_deref().unwrap_or("auto");
    let new_backend: Option<Option<PlaceBackend>> = if !backend_changed && has_existing_backend {
        // Keep the existing connected Studio / LocalPlace backend untouched when only
        // model/provider/API keys/Open Cloud settings changed (prevents 60s MCP re-spawns).
        None
    } else {
        Some(match desired_mode {
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
                let mut client = McpClient::spawn(&cmd)?;
                if !client.has_live_studio() {
                    return Err(
                        "Roblox Studio is not signed in or has no open place yet. Sign in and open a place in Roblox Studio, then try again."
                            .to_string(),
                    );
                }
                Some(PlaceBackend::Mcp(client))
            }
            _ => {
                // "auto": explicit mcp_command -> running Vinegar with live open place -> Local Place Engine (#24)
                let mcp_try = cfg
                    .mcp_command
                    .clone()
                    .or_else(McpClient::detect_vinegar_studio_mcp);
                if let Some(cmd) = mcp_try {
                    match McpClient::spawn_with_timeout(&cmd, std::time::Duration::from_secs(4)) {
                        Ok(mut client) => {
                            if client.has_live_studio() {
                                client.reset_default_timeout();
                                Some(PlaceBackend::Mcp(client))
                            } else {
                                let place = match &shared.data_dir {
                                    Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
                                    None => LocalPlace::in_memory(shared.game_name.clone()),
                                };
                                Some(PlaceBackend::Local(place))
                            }
                        }
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
        })
    };

    let mut new_model = ModelClient::new(base_url, model_id, cfg.api_key.clone());
    if let Some(dir) = &shared.data_dir {
        new_model = new_model.with_agy_dir(dir.join("agy-sandbox"));
    }

    if let Some(dir) = &shared.data_dir {
        cfg.save(dir)?;
    }
    *shared.config.lock().unwrap() = cfg;

    {
        let mut guard = shared.agent.lock().unwrap();
        match guard.as_mut() {
            Some(agent) => {
                agent.model = new_model;
                if let Some(backend) = new_backend {
                    agent.studio = backend;
                }
                agent.attach_events(Arc::clone(&shared.events));
                agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
            }
            None => {
                let mut agent = Agent::with_backend(new_model, new_backend.flatten());
                if let Some(dir) = &shared.data_dir {
                    agent.set_changes_file(dir.join("changes.json"));
                }
                agent.attach_undo_sink(Arc::clone(&shared.undoable));
                agent.attach_redo_sink(Arc::clone(&shared.redoable));
                agent.attach_events(Arc::clone(&shared.events));
                agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
                *guard = Some(agent);
            }
        }
        *shared.status_cache.lock().unwrap() = extract_status_from_agent(guard.as_mut());
    }

    Ok(config_snapshot(shared))
}

fn migrate_config_secrets(data_dir: &std::path::Path, cfg: &mut RuntimeConfig) {
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let moved = store.migrate(&[
        (crate::secrets::GEMINI_KEY, cfg.gemini_api_key.as_deref()),
        (crate::secrets::OPENAI_IMAGE_KEY, cfg.openai_image_api_key.as_deref()),
    ]);
    if !moved.is_empty() {
        if moved.iter().any(|k| k == crate::secrets::GEMINI_KEY) {
            cfg.gemini_api_key = None;
        }
        if moved.iter().any(|k| k == crate::secrets::OPENAI_IMAGE_KEY) {
            cfg.openai_image_api_key = None;
        }
        let _ = cfg.save(data_dir);
    }
}

fn is_valid_project_slug(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Resolve a project slug to its directory via `ProjectStore` first, falling back to
/// `<data_dir>/projects/<id>`, `OPENRO_PROJECT_ROOT`, or `projects/<id>` under cwd.
fn resolve_project_dir(shared: &Shared, id: &str) -> Option<PathBuf> {
    if !is_valid_project_slug(id) {
        return None;
    }
    if let Some(found) = shared.projects.lock().unwrap().find(id) {
        if found.dir.is_dir() {
            return Some(found.dir);
        }
    }
    if let Some(dir) = &shared.data_dir {
        let in_data = dir.join("projects").join(id);
        if in_data.is_dir() {
            return Some(in_data);
        }
    }
    if let Ok(root) = std::env::var("OPENRO_PROJECT_ROOT") {
        let pb = PathBuf::from(root.trim());
        let child = pb.join(id);
        if child.is_dir() {
            return Some(child);
        }
        if pb.file_name().and_then(|n| n.to_str()) == Some(id) && pb.is_dir() {
            return Some(pb);
        }
    }
    let cwd_proj = PathBuf::from("projects").join(id);
    if cwd_proj.is_dir() {
        return Some(cwd_proj);
    }
    None
}

fn status_line_for_code(code: u16) -> &'static str {
    match code {
        200 => "200 OK",
        201 => "201 Created",
        400 => "400 Bad Request",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        405 => "405 Method Not Allowed",
        409 => "409 Conflict",
        423 => "423 Locked",
        429 => "429 Too Many Requests",
        502 => "502 Bad Gateway",
        503 => "503 Service Unavailable",
        504 => "504 Gateway Timeout",
        _ => "500 Internal Server Error",
    }
}

fn try_route_opencloud_assets(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &Request,
    path: &str,
    query: &str,
) -> bool {
    let is_candidate = path == "/api/opencloud"
        || path.starts_with("/api/opencloud/")
        || path == "/api/creator-hub"
        || path.starts_with("/api/creator-hub/")
        || path == "/api/art"
        || path.starts_with("/api/art/")
        || path == "/api/tools/image/process"
        || (path.starts_with("/api/projects/")
            && (path.contains("/assets")
                || path.ends_with("/publish")
                || path.ends_with("/products")
                || path.ends_with("/products/sync")
                || path.ends_with("/products/codegen")
                || path.ends_with("/monetization/pack")
                || path.ends_with("/monetization/codegen")
                || path.ends_with("/monetization/simulate")
                || path.ends_with("/publish-gate")
                || path.ends_with("/readiness")
                || path.ends_with("/autopilot")
                || path.ends_with("/universe")
                || path.contains("/liveops/")
                || path.ends_with("/liveops")
                || path.ends_with("/creator-hub/setup")
                || path.ends_with("/creator-hub/configure")));
    if !is_candidate {
        return false;
    }
    if (path.contains("/liveops/") || path.ends_with("/liveops")) && !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "live-ops control plane endpoints are only allowed from this machine (loopback)");
        return true;
    }
    let mut body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        }
    };
    if path == "/api/art/generate" && body.get("provider").is_none() {
        if let Some(prov) = shared.config.lock().unwrap().image_provider.clone() {
            if !prov.is_empty() {
                body["provider"] = json!(prov);
            }
        }
    }
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let resolver = |id: &str| resolve_project_dir(shared, id);
    let ctx = crate::opencloud::Ctx {
        data_dir,
        store,
        resolve: &resolver,
        active_project: shared.active_project_dir(),
    };
    let Some(reply) = crate::opencloud::route(&ctx, &req.method, path, query, &body) else {
        return false;
    };
    match reply {
        crate::opencloud::Reply::Json(code, val) => {
            respond_json(stream, status_line_for_code(code), &val);
        }
        crate::opencloud::Reply::Bytes(mime, bytes) => {
            respond_bytes(stream, "200 OK", mime, "no-store", &bytes);
        }
    }
    true
}

fn try_route_tool_adapters(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &Request,
    path: &str,
    query: &str,
) -> bool {
    if path != "/api/adapters" && !path.starts_with("/api/adapters/") {
        return false;
    }
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "tool adapters are only allowed from this machine (loopback)");
        return true;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        }
    };
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let resolver = |id: &str| resolve_project_dir(shared, id);
    let rctx = crate::tool_adapters::RouteCtx {
        data_dir,
        assets_root: &shared.assets_root,
        store: &store,
        project_resolver: &resolver,
        active_project: shared.active_project_dir(),
    };
    let Some((code, val)) = crate::tool_adapters::route(
        &rctx,
        &req.method,
        path,
        query,
        &body,
    ) else {
        return false;
    };
    respond_json(stream, status_line_for_code(code), &val);
    true
}

fn handle_studio_mcp_connect(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "connecting a live Studio MCP bridge is only allowed from this machine (loopback)");
        return;
    }
    if shared.busy.load(Ordering::SeqCst) > 0 {
        respond_err(stream, "409 Conflict", "a build or playtest is currently running; try connecting once it finishes");
        return;
    }
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
    let explicit_cmd = body
        .get("command")
        .or_else(|| body.get("mcp_command"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let cfg_cmd = shared.config.lock().unwrap().mcp_command.clone();
    let env_cmd = std::env::var("OPENRO_MCP_COMMAND")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let Some(cmd) = explicit_cmd
        .or(cfg_cmd)
        .or(env_cmd)
        .or_else(McpClient::detect_vinegar_studio_mcp)
    else {
        respond_json(
            stream,
            "400 Bad Request",
            &json!({
                "ok": false,
                "connected": false,
                "studio_mode": shared.studio_stream.studio_mode(),
                "error": "No Studio MCP bridge command provided and no running Roblox Studio MCP session was detected. Provide a command (e.g. ~/.local/bin/roblox-studio-mcp) or set OPENRO_MCP_COMMAND.",
            }),
        );
        return;
    };

    if cmd.contains('\0') || cmd.contains('\n') || cmd.contains('\r') {
        respond_json(
            stream,
            "400 Bad Request",
            &json!({ "ok": false, "connected": false, "error": "invalid MCP bridge command" }),
        );
        return;
    }

    let mut mcp = match McpClient::spawn(&cmd) {
        Ok(c) => c,
        Err(e) => {
            respond_json(
                stream,
                "502 Bad Gateway",
                &json!({
                    "ok": false,
                    "connected": false,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "error": format!("Could not connect to Studio MCP bridge '{cmd}': {e}"),
                }),
            );
            return;
        }
    };
    if !mcp.has_live_studio() {
        respond_json(
            stream,
            "502 Bad Gateway",
            &json!({
                "ok": false,
                "connected": false,
                "studio_mode": shared.studio_stream.studio_mode(),
                "error": "Studio MCP started, but no open Roblox Studio place was found. Sign in and open a place in Roblox Studio, then try again.",
            }),
        );
        return;
    }
    shared
        .studio_embed
        .lock()
        .unwrap()
        .sync_from_client(&mcp, Some(cmd.clone()));

    let (frame_seq, frame_meta) = {
        let mut guard = shared.agent.lock().unwrap();
        match guard.as_mut() {
            Some(agent) => {
                agent.studio = Some(PlaceBackend::Mcp(mcp));
                agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
                let mut ev = StudioActionInput::new(
                    StudioActionKind::CameraFocus,
                    format!("Connected live Roblox Studio MCP bridge ({cmd})"),
                );
                ev.path = "Workspace.CurrentCamera".to_string();
                ev.class_name = "Camera".to_string();
                shared.studio_stream.record_action(ev);
                let captured = agent.capture_studio_frame().ok();
                *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
                (
                    captured.as_ref().map(|f| f.seq).unwrap_or(0),
                    captured.as_ref().map(|f| f.meta_json()),
                )
            }
            None => {
                let model = ModelClient::new(
                    DEFAULT_BASE_URL.to_string(),
                    DEFAULT_MODEL_ID.to_string(),
                    None,
                );
                let mut agent = Agent::with_backend(model, Some(PlaceBackend::Mcp(mcp)));
                if let Some(dir) = &shared.data_dir {
                    agent.set_changes_file(dir.join("changes.json"));
                }
                agent.attach_undo_sink(Arc::clone(&shared.undoable));
                agent.attach_redo_sink(Arc::clone(&shared.redoable));
                agent.attach_events(Arc::clone(&shared.events));
                agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
                let captured = agent.capture_studio_frame().ok();
                *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(&mut agent));
                *guard = Some(agent);
                (
                    captured.as_ref().map(|f| f.seq).unwrap_or(0),
                    captured.as_ref().map(|f| f.meta_json()),
                )
            }
        }
    };

    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "connected": true,
            "studio_connected": true,
            "workspace_mode": "studio_mcp",
            "studio_mode": "live_mcp",
            "command": cmd,
            "has_live_frame": frame_seq > 0,
            "frame_seq": frame_seq,
            "frame": frame_meta,
        }),
    );
}

fn handle_studio_mcp_disconnect(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "disconnecting a live Studio MCP bridge is only allowed from this machine (loopback)");
        return;
    }
    if shared.busy.load(Ordering::SeqCst) > 0 {
        respond_err(stream, "409 Conflict", "a build or playtest is currently running; disconnect once it finishes");
        return;
    }

    {
        let mut mgr = shared.studio_embed.lock().unwrap();
        let _ = crate::studio_embed::stop_embedded_studio(&mut mgr);
    }
    invalidate_embed_detect_cache(shared);

    let local_place = match &shared.data_dir {
        Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
        None => LocalPlace::in_memory(shared.game_name.clone()),
    };

    {
        let mut guard = shared.agent.lock().unwrap();
        if let Some(agent) = guard.as_mut() {
            agent.studio = Some(PlaceBackend::Local(local_place));
            agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
            *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
        }
    }

    let mut ev = StudioActionInput::new(
        StudioActionKind::CameraFocus,
        "Disconnected live Studio MCP bridge — returned to Local Studio 3D",
    );
    ev.path = "Workspace.CurrentCamera".to_string();
    ev.class_name = "Camera".to_string();
    shared.studio_stream.record_action(ev);

    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "connected": false,
            "studio_connected": true,
            "workspace_mode": "local_place",
            "studio_mode": "local_place",
        }),
    );
}

fn handle_studio_input_mouse(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "forwarding Studio mouse input is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => v,
        _ => {
            respond_err(stream, "400 Bad Request", "invalid JSON body for mouse input");
            return;
        }
    };
    let action = body
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("click")
        .to_string();
    const VALID_ACTIONS: [&str; 8] = [
        "click",
        "move",
        "down",
        "up",
        "drag",
        "wheel",
        "double_click",
        "right_click",
    ];
    if !VALID_ACTIONS.contains(&action.as_str()) {
        respond_json(
            stream,
            "400 Bad Request",
            &json!({ "ok": false, "error": format!("unsupported mouse action '{action}'") }),
        );
        return;
    }
    let x = body.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = body.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if !x.is_finite() || !y.is_finite() {
        respond_err(stream, "400 Bad Request", "mouse coordinates x and y must be finite numbers");
        return;
    }

    // While Embedded Studio is streaming X11 (before MCP attaches or on Start Page),
    // forward clicks/moves directly to the X11 display so users can interact inside OpenRO (#35, #51).
    let x11_info = shared.studio_embed.try_lock().ok().and_then(|g| {
        let st = g.state();
        if st.x11grab_active {
            st.display.map(|d| (d, st.resolution, !st.mcp_connected || !st.place_open))
        } else {
            None
        }
    });
    let button = body
        .get("button")
        .or_else(|| body.get("mouse_button"))
        .and_then(|v| v.as_str())
        .unwrap_or("left")
        .to_string();
    let delta_y = body
        .get("delta_y")
        .or_else(|| body.get("deltaY"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if let Some((ref disp, res, true)) = x11_info {
        match crate::studio_embed::send_x11_mouse(disp, res, &action, x, y, None, &button, delta_y) {
            Ok(result) => {
                if let Ok(mut mgr) = shared.studio_embed.try_lock() {
                    mgr.record_input(format!("x11 mouse {action} ({x:.2}, {y:.2})"));
                }
                let fseq = shared.studio_stream.latest_frame_seq();
                respond_json(
                    stream,
                    "200 OK",
                    &json!({
                        "ok": true,
                        "action": action,
                        "transport": "x11",
                        "studio_mode": "awaiting_signin",
                        "frame_seq": fseq,
                        "has_live_frame": fseq > 0,
                        "result": result,
                    }),
                );
            }
            Err(e) => {
                respond_json(
                    stream,
                    "502 Bad Gateway",
                    &json!({ "ok": false, "transport": "x11", "error": e }),
                );
            }
        }
        return;
    }

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    match agent.invoke_studio_tool(&chat_id, "user_mouse_input", body) {
        Ok((result, frame_seq)) => {
            let fseq = frame_seq.unwrap_or_else(|| shared.studio_stream.latest_frame_seq());
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "action": action,
                    "studio_mode": agent.studio_mode_tag(),
                    "frame_seq": fseq,
                    "has_live_frame": fseq > 0,
                    "result": result,
                }),
            );
        }
        Err(e) => {
            // When embedded x11grab is active and MCP user_mouse_input isn't available in Edit mode,
            // fall back transparently to X11 input on :99 so embedded viewport clicks always work (#51).
            if let Some((disp, res, _)) = x11_info {
                drop(guard);
                if let Ok(result) =
                    crate::studio_embed::send_x11_mouse(&disp, res, &action, x, y, None, &button, delta_y)
                {
                    if let Ok(mut mgr) = shared.studio_embed.try_lock() {
                        mgr.record_input(format!("x11 mouse {action} ({x:.2}, {y:.2})"));
                    }
                    let fseq = shared.studio_stream.latest_frame_seq();
                    respond_json(
                        stream,
                        "200 OK",
                        &json!({
                            "ok": true,
                            "action": action,
                            "transport": "x11",
                            "studio_mode": shared.studio_stream.studio_mode(),
                            "frame_seq": fseq,
                            "has_live_frame": fseq > 0,
                            "result": result,
                        }),
                    );
                    return;
                }
            }
            let status = if e.contains("requires a connected live Roblox Studio")
                || e.contains("Studio not connected")
            {
                "503 Service Unavailable"
            } else {
                "502 Bad Gateway"
            };
            respond_json(
                stream,
                status,
                &json!({
                    "ok": false,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "error": e,
                }),
            );
        }
    }
}

fn handle_studio_input_keyboard(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "forwarding Studio keyboard input is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => v,
        _ => {
            respond_err(stream, "400 Bad Request", "invalid JSON body for keyboard input");
            return;
        }
    };
    let has_key = body
        .get("key")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let has_text = body
        .get("text")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    if !has_key && !has_text {
        respond_err(stream, "400 Bad Request", "keyboard input requires non-empty 'key' or 'text'");
        return;
    }
    let action = body
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(if has_text { "type" } else { "press" })
        .to_string();

    // While Embedded Studio is streaming X11 (before MCP attaches or on Start Page),
    // forward keystrokes/text directly into the X11 display so users can interact inside OpenRO (#35, #51).
    let x11_info = shared.studio_embed.try_lock().ok().and_then(|g| {
        let st = g.state();
        if st.x11grab_active {
            st.display.map(|d| (d, !st.mcp_connected || !st.place_open))
        } else {
            None
        }
    });
    let key_str = body
        .get("key")
        .or_else(|| body.get("key_code"))
        .or_else(|| body.get("code"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let text_str = body
        .get("text")
        .or_else(|| body.get("text_inputs"))
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some((ref disp, true)) = x11_info {
        match crate::studio_embed::send_x11_keyboard(
            disp,
            &action,
            key_str.as_deref(),
            text_str.as_deref(),
            None,
        ) {
            Ok(result) => {
                if let Ok(mut mgr) = shared.studio_embed.try_lock() {
                    mgr.record_input(format!("x11 keyboard {action}"));
                }
                let fseq = shared.studio_stream.latest_frame_seq();
                respond_json(
                    stream,
                    "200 OK",
                    &json!({
                        "ok": true,
                        "action": action,
                        "transport": "x11",
                        "studio_mode": "awaiting_signin",
                        "frame_seq": fseq,
                        "has_live_frame": fseq > 0,
                        "result": result,
                    }),
                );
            }
            Err(e) => {
                respond_json(
                    stream,
                    "502 Bad Gateway",
                    &json!({ "ok": false, "transport": "x11", "error": e }),
                );
            }
        }
        return;
    }

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    match agent.invoke_studio_tool(&chat_id, "user_keyboard_input", body) {
        Ok((result, frame_seq)) => {
            let fseq = frame_seq.unwrap_or_else(|| shared.studio_stream.latest_frame_seq());
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "action": action,
                    "studio_mode": agent.studio_mode_tag(),
                    "frame_seq": fseq,
                    "has_live_frame": fseq > 0,
                    "result": result,
                }),
            );
        }
        Err(e) => {
            if let Some((disp, _)) = x11_info {
                drop(guard);
                if let Ok(result) = crate::studio_embed::send_x11_keyboard(
                    &disp,
                    &action,
                    key_str.as_deref(),
                    text_str.as_deref(),
                    None,
                ) {
                    if let Ok(mut mgr) = shared.studio_embed.try_lock() {
                        mgr.record_input(format!("x11 keyboard {action}"));
                    }
                    let fseq = shared.studio_stream.latest_frame_seq();
                    respond_json(
                        stream,
                        "200 OK",
                        &json!({
                            "ok": true,
                            "action": action,
                            "transport": "x11",
                            "studio_mode": shared.studio_stream.studio_mode(),
                            "frame_seq": fseq,
                            "has_live_frame": fseq > 0,
                            "result": result,
                        }),
                    );
                    return;
                }
            }
            let status = if e.contains("requires a connected live Roblox Studio")
                || e.contains("Studio not connected")
            {
                "503 Service Unavailable"
            } else {
                "502 Bad Gateway"
            };
            respond_json(
                stream,
                status,
                &json!({
                    "ok": false,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "error": e,
                }),
            );
        }
    }
}

fn handle_studio_tool(stream: &mut TcpStream, shared: &Shared, req: &Request, tool: &str) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "calling Studio MCP tools directly is only allowed from this machine (loopback)");
        return;
    }
    if !crate::agent::is_official_studio_tool(tool) {
        respond_json(
            stream,
            "404 Not Found",
            &json!({
                "ok": false,
                "error": format!("unknown or disallowed Studio tool '{tool}'"),
                "allowed_tools": crate::agent::OFFICIAL_STUDIO_MCP_TOOLS,
            }),
        );
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON object body for Studio tool arguments");
                return;
            }
        }
    };

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    match agent.invoke_studio_tool(&chat_id, tool, body) {
        Ok((result, frame_seq)) => {
            let fseq = frame_seq.unwrap_or_else(|| shared.studio_stream.latest_frame_seq());
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "ok": true,
                    "tool": tool,
                    "studio_mode": agent.studio_mode_tag(),
                    "frame_seq": fseq,
                    "has_live_frame": fseq > 0,
                    "result": result,
                }),
            );
        }
        Err(e) => {
            let status = if e.contains("requires a connected live Roblox Studio")
                || e.contains("Studio not connected")
            {
                "503 Service Unavailable"
            } else if e.contains("requires ") || e.contains("no script at") || e.contains("no instance") {
                "400 Bad Request"
            } else {
                "502 Bad Gateway"
            };
            respond_json(
                stream,
                status,
                &json!({
                    "ok": false,
                    "tool": tool,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "error": e,
                }),
            );
        }
    }
}

const HOST_DETECT_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

fn cached_embed_detection(shared: &Shared) -> crate::studio_embed::HostStudioDetection {
    {
        let cache = shared.cached_embed_detect.lock().unwrap();
        if let Some((at, ref det)) = *cache {
            if at.elapsed() < HOST_DETECT_CACHE_TTL {
                return det.clone();
            }
        }
    }
    let fresh = crate::studio_embed::detect_studio_environment();
    *shared.cached_embed_detect.lock().unwrap() = Some((std::time::Instant::now(), fresh.clone()));
    fresh
}

pub(crate) fn invalidate_embed_detect_cache(shared: &Shared) {
    *shared.cached_embed_detect.lock().unwrap() = None;
}

fn cached_library_list_json(shared: &Shared) -> Value {
    {
        let cache = shared.cached_library_list.lock().unwrap();
        if let Some((at, ref val)) = *cache {
            if at.elapsed() < HOST_DETECT_CACHE_TTL {
                return val.clone();
            }
        }
    }
    let fresh = shared.library.lock().unwrap().list_json();
    *shared.cached_library_list.lock().unwrap() = Some((std::time::Instant::now(), fresh.clone()));
    fresh
}

fn invalidate_library_cache(shared: &Shared) {
    *shared.cached_library_list.lock().unwrap() = None;
}

fn build_embed_status_payload(shared: &Shared) -> Value {
    let info = status_info(shared);
    let frame_seq = shared.studio_stream.latest_frame_seq();
    let studio_mode = shared.studio_stream.studio_mode();
    let Some(state) = shared.studio_embed.try_lock().ok().map(|g| g.state()) else {
        let detect = crate::studio_embed::detect_studio_environment_fast();
        return json!({
            "ok": true,
            "running": false,
            "embedded": false,
            "state": "launching",
            "phase": "starting",
            "awaiting_signin": false,
            "studio_mode": studio_mode,
            "studio_connected": info.studio_connected,
            "studio_running": detect.studio_running,
            "studio_display": detect.studio_display,
            "already_logged_in": detect.already_logged_in,
            "place_open": detect.place_open,
            "transferred": false,
            "can_transfer": detect.studio_installed,
            "guidance": detect.guidance,
            "running_studios": detect.running_studios,
            "vinegar_installed": detect.studio_installed,
            "xvfb_installed": detect.xvfb_available,
            "windows_studio_path": detect.studio_exe,
            "has_live_frame": frame_seq > 0,
            "frame_seq": frame_seq,
            "stream_url": "/api/studio/stream.mjpg",
            "frame_url": "/api/studio/frame",
            "available_tools": crate::agent::OFFICIAL_STUDIO_MCP_TOOLS,
        });
    };
    let is_custom_mcp = state.mcp_command.as_deref().is_some_and(|cmd| {
        let host_mcp = crate::studio_embed::detect_host_mcp_command();
        host_mcp.as_deref() != Some(cmd)
    });
    let detect = if (state.phase == "connected" && (state.place_open || is_custom_mcp))
        || state.phase == "awaiting_signin"
    {
        // Active open-place session, custom MCP bridge, or background auto-attacher already
        // tracks MCP state; avoid spawning redundant/unrelated host MCP probes on status poll.
        crate::studio_embed::detect_studio_environment_fast()
    } else {
        cached_embed_detection(shared)
    };
    let available_tools = match shared.agent.try_lock() {
        Ok(guard) => guard
            .as_ref()
            .map(Agent::available_studio_tools)
            .unwrap_or_default(),
        Err(_) => state.tools.clone(),
    };
    let running = state.embedded;
    let awaiting_signin = state.phase == "awaiting_signin";
    let simple_state = if awaiting_signin {
        "awaiting_signin"
    } else if running {
        "running"
    } else if state.last_error.is_some() {
        "error"
    } else {
        "idle"
    };
    let mut running_studios = detect.running_studios.clone();
    let place_open = if state.mcp_connected {
        state.place_open || detect.place_open
    } else {
        detect.place_open
    };
    if state.mcp_connected && running_studios.is_empty() {
        if let Some(ref sid) = state.studio_id {
            running_studios.push(json!({
                "studio_id": sid,
                "place_name": Value::Null,
                "datamodel_type": if place_open { state.datamodel_type.as_str() } else { "Unavailable" },
                "place_open": place_open,
                "studio_state": if place_open { Value::Null } else { json!("Place is not open") },
            }));
        }
    }
    let already_logged_in = detect.already_logged_in || (state.mcp_connected && state.studio_id.is_some());
    let studio_running = detect.studio_running || state.embedded;
    let studio_display = detect.studio_display.clone().or_else(|| state.display.clone());
    let guidance = if state.phase == "connected" && place_open {
        "Roblox Studio is connected and ready to sync and playtest.".to_string()
    } else if already_logged_in && !place_open {
        "Studio is on the Start Page — open a blank Baseplate to sync/playtest.".to_string()
    } else {
        detect.guidance.clone()
    };

    let mut obj = state.to_json_with_detection(&detect);
    if let Some(map) = obj.as_object_mut() {
        map.insert("ok".to_string(), json!(true));
        map.insert("running".to_string(), json!(running));
        map.insert("state".to_string(), json!(simple_state));
        map.insert("awaiting_signin".to_string(), json!(awaiting_signin));
        map.insert("studio_mode".to_string(), json!(studio_mode));
        map.insert("studio_connected".to_string(), json!(info.studio_connected));
        map.insert("studio_running".to_string(), json!(studio_running));
        map.insert("studio_display".to_string(), json!(studio_display));
        map.insert("already_logged_in".to_string(), json!(already_logged_in));
        map.insert("place_open".to_string(), json!(place_open));
        map.insert("transferred".to_string(), json!(state.transferred));
        map.insert("can_transfer".to_string(), json!(detect.studio_installed));
        map.insert("guidance".to_string(), json!(guidance));
        map.insert("running_studios".to_string(), json!(running_studios));
        map.insert("vinegar_installed".to_string(), json!(detect.studio_installed));
        map.insert("xvfb_installed".to_string(), json!(detect.xvfb_available));
        map.insert("windows_studio_path".to_string(), json!(detect.studio_exe));
        map.insert("has_live_frame".to_string(), json!(frame_seq > 0));
        map.insert("frame_seq".to_string(), json!(frame_seq));
        map.insert("stream_url".to_string(), json!("/api/studio/stream.mjpg"));
        map.insert("frame_url".to_string(), json!("/api/studio/frame"));
        map.insert("available_tools".to_string(), json!(available_tools));
    }
    obj
}

fn handle_studio_embed_status(stream: &mut TcpStream, shared: &Shared) {
    respond_json(stream, "200 OK", &build_embed_status_payload(shared));
}

fn attach_embed_mcp_client(shared: &Shared, mut mcp: McpClient) -> (u64, Option<Value>, Vec<String>) {
    mcp.reset_default_timeout();
    let mut guard = shared.agent.lock().unwrap();
    match guard.as_mut() {
        Some(agent) => {
            agent.studio = Some(PlaceBackend::Mcp(mcp));
            agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
            let mut ev = StudioActionInput::new(
                StudioActionKind::CameraFocus,
                "Embedded Roblox Studio live session attached",
            );
            ev.path = "Workspace.CurrentCamera".to_string();
            ev.class_name = "Camera".to_string();
            shared.studio_stream.record_action(ev);
            let captured = agent.capture_studio_frame().ok();
            let tools = agent.available_studio_tools();
            *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
            (
                captured.as_ref().map(|f| f.seq).unwrap_or(0),
                captured.as_ref().map(|f| f.meta_json()),
                tools,
            )
        }
        None => {
            let model = ModelClient::new(
                DEFAULT_BASE_URL.to_string(),
                DEFAULT_MODEL_ID.to_string(),
                None,
            );
            let mut agent = Agent::with_backend(model, Some(PlaceBackend::Mcp(mcp)));
            if let Some(dir) = &shared.data_dir {
                agent.set_changes_file(dir.join("changes.json"));
            }
            agent.attach_undo_sink(Arc::clone(&shared.undoable));
            agent.attach_redo_sink(Arc::clone(&shared.redoable));
            agent.attach_events(Arc::clone(&shared.events));
            agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
            let lib_prompt = shared.library.lock().unwrap().active_skill_prompt();
            agent.set_library_prompt(lib_prompt);
            let captured = agent.capture_studio_frame().ok();
            let tools = agent.available_studio_tools();
            *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(&mut agent));
            *guard = Some(agent);
            (
                captured.as_ref().map(|f| f.seq).unwrap_or(0),
                captured.as_ref().map(|f| f.meta_json()),
                tools,
            )
        }
    }
}

/// On non-X11 platforms (Windows, macOS, Wayland) where `ffmpeg x11grab` is not active,
/// capture live MCP `screen_capture` frames into `StudioStreamHub` whenever a browser
/// client is subscribed to `/api/studio/stream.mjpg` (#35). Uses `try_lock()` so it
/// never blocks builds or playtests.
fn spawn_mcp_screen_capture_pump(shared: Arc<Shared>, stop_flag: Arc<AtomicBool>) {
    if std::env::var("OPENRO_DISABLE_MCP_FRAME_PUMP")
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
    {
        return;
    }
    std::thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            for _ in 0..7 {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if shared.studio_stream.active_subscribers() == 0
                || shared.busy.load(Ordering::SeqCst) > 0
            {
                continue;
            }
            let Ok(mut guard) = shared.agent.try_lock() else {
                continue;
            };
            let Some(agent) = guard.as_mut() else {
                break;
            };
            if !matches!(agent.studio, Some(PlaceBackend::Mcp(_))) {
                break;
            }
            let _ = agent.capture_studio_frame();
        }
    });
}

/// Background poller while Embedded Studio is in `phase: "awaiting_signin"` (#35, #118):
/// automatically promotes the session to `phase: "connected"` and attaches `McpClient`
/// as soon as the user finishes signing in / opens a place inside the live viewport.
fn spawn_embed_mcp_auto_attacher(
    shared: Arc<Shared>,
    mcp_cmd: String,
    stop_flag: Arc<AtomicBool>,
) {
    let poll_ms: u64 = std::env::var("OPENRO_EMBED_POLL_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|ms| ms.clamp(50, 10_000))
        .unwrap_or(2_000);

    std::thread::spawn(move || {
        let step_ms = 50u64;
        while !stop_flag.load(Ordering::Relaxed) {
            let mut waited = 0u64;
            while waited < poll_ms {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                let slice = step_ms.min(poll_ms - waited);
                std::thread::sleep(std::time::Duration::from_millis(slice));
                waited += slice;
            }
            if stop_flag.load(Ordering::Relaxed) {
                return;
            }
            if shared.busy.load(Ordering::SeqCst) > 0 {
                continue;
            }
            let Ok(mut client) =
                McpClient::spawn_with_timeout(&mcp_cmd, std::time::Duration::from_secs(3))
            else {
                continue;
            };
            if !client.has_live_studio() {
                continue;
            }
            if stop_flag.load(Ordering::Relaxed) {
                return;
            }
            let place_open = crate::studio_embed::probe_client_place_open(&mut client);
            client.reset_default_timeout();
            let studio_id = client.studio_id().map(ToString::to_string);
            let datamodel_type = if place_open {
                client.datamodel_type().to_string()
            } else {
                "Unavailable".to_string()
            };
            let tools = client.tools().to_vec();
            let official_count = client.official_tools_available().len();
            // Attach PlaceBackend::Mcp + capture initial live frame BEFORE exposing
            // phase="connected" so status readers never observe connected before MCP/frame are live.
            let _ = attach_embed_mcp_client(&shared, client);
            let x11grab_active = {
                let mut mgr = shared.studio_embed.lock().unwrap();
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                mgr.sync_connected_meta(
                    studio_id,
                    datamodel_type,
                    tools,
                    official_count,
                    Some(mcp_cmd.clone()),
                );
                mgr.set_place_open(place_open);
                mgr.state().x11grab_active
            };
            invalidate_embed_detect_cache(&shared);
            if !x11grab_active {
                spawn_mcp_screen_capture_pump(Arc::clone(&shared), Arc::clone(&stop_flag));
            }
            return;
        }
    });
}

fn stage_active_project_studio_content_best_effort(
    shared: &Shared,
    project_id_override: Option<&str>,
) {
    let proj_dir = {
        let store = shared.projects.lock().unwrap();
        project_id_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|pid| store.find(pid))
            .or_else(|| store.active.as_deref().and_then(|aid| store.find(aid)))
            .map(|r| r.dir)
    };
    if let Some(dir) = proj_dir {
        let slug = crate::studio_embed::default_studio_project_slug(&dir);
        let _ = crate::studio_embed::stage_project_studio_content(&dir, &slug);
    }
}

fn resolve_active_project_latest_rbxlx(
    shared: &Shared,
    project_id_override: Option<&str>,
) -> Option<PathBuf> {
    let proj_dir = {
        let store = shared.projects.lock().unwrap();
        project_id_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|pid| store.find(pid))
            .or_else(|| store.active.as_deref().and_then(|aid| store.find(aid)))
            .map(|r| r.dir)
    }?;
    if let Some((_, p)) = crate::opencloud::latest_build_file(&proj_dir) {
        if p.is_file() {
            return Some(p);
        }
    }
    crate::projects::builds(&proj_dir)
        .into_iter()
        .find_map(|rec| {
            let rel = rec.get("path")?.as_str()?;
            let full = crate::projects::safe_join(&proj_dir, rel).ok()?;
            full.is_file().then_some(full)
        })
}

fn try_route_project_studio(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &Request,
    path: &str,
) -> bool {
    let Some(rest) = path.strip_prefix("/api/projects/") else {
        return false;
    };
    let segs: Vec<&str> = rest.split('/').collect();
    if segs.len() != 3 || segs[1] != "studio" {
        return false;
    }
    let id = segs[0];
    let action = segs[2];
    if !matches!(action, "stage-content" | "verify-device" | "verify-audio") {
        return false;
    }
    if req.method != "POST" {
        respond_err(stream, "405 Method Not Allowed", "use POST for this endpoint");
        return true;
    }
    if !host_allowed(&req.host, &[]) {
        respond_err(
            stream,
            "403 Forbidden",
            "Studio project bridge & verification endpoints are only allowed from this machine (loopback)",
        );
        return true;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        }
    };
    let Some(proj_ref) = shared.projects.lock().unwrap().find(id) else {
        respond_err(stream, "404 Not Found", &format!("no project '{id}'"));
        return true;
    };
    match action {
        "stage-content" => {
            let slug = body
                .get("slug")
                .or_else(|| body.get("studio_project_slug"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(crate::studio_embed::sanitize_studio_content_slug)
                .unwrap_or_else(|| crate::studio_embed::default_studio_project_slug(&proj_ref.dir));
            match crate::studio_embed::stage_project_studio_content(&proj_ref.dir, &slug) {
                Ok(v) => respond_json(stream, "200 OK", &v),
                Err(e) => {
                    if crate::studio_embed::find_studio_content_dir().is_none() {
                        respond_json(
                            stream,
                            "200 OK",
                            &json!({
                                "ok": false,
                                "studio_found": false,
                                "staged_count": 0,
                                "skipped_count": 0,
                                "error": e,
                            }),
                        )
                    } else {
                        respond_err(stream, "400 Bad Request", &e)
                    }
                }
            }
        }
        "verify-device" => match crate::playtest::verify_studio_device(shared, &proj_ref, &body) {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err((code, msg)) => respond_err(stream, status_line_for_code(code), &msg),
        },
        "verify-audio" => match crate::playtest::verify_studio_audio(shared, &proj_ref, &body) {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err((code, msg)) => respond_err(stream, status_line_for_code(code), &msg),
        },
        _ => return false,
    }
    true
}

fn perform_studio_embed_launch(
    shared: &Arc<Shared>,
    body: &Value,
    attach_only: bool,
) -> Result<Value, (&'static str, String)> {
    stage_active_project_studio_content_best_effort(
        shared,
        body.get("project_id").and_then(Value::as_str),
    );
    let mut cfg = crate::studio_embed::EmbedConfig::from_json(body)
        .map_err(|e| ("400 Bad Request", e))?;
    if attach_only {
        cfg.launch_studio_process = false;
        if cfg.mcp_command.is_none() {
            cfg.mcp_command = shared
                .config
                .lock()
                .unwrap()
                .mcp_command
                .clone()
                .or_else(|| {
                    std::env::var("OPENRO_MCP_COMMAND")
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });
        }
    } else if body.get("transfer").and_then(|v| v.as_bool()) == Some(true)
        || (cfg.place_file.is_some() && cfg.mcp_command.is_none())
    {
        let opts = crate::studio_embed::TransferOptions {
            display: cfg.display,
            width: cfg.width,
            height: cfg.height,
            place_file: cfg.place_file,
            force_transfer: body.get("transfer").and_then(|v| v.as_bool()).unwrap_or(false),
            wait_secs: (cfg.startup_timeout_ms / 1000).clamp(3, 90),
            mcp_command: cfg.mcp_command,
        };
        return perform_studio_embed_transfer(shared, opts);
    }

    let (launch_res, stop_flag) = {
        let mut mgr = shared.studio_embed.lock().unwrap();
        let res = crate::studio_embed::launch_or_attach_live(
            &mut mgr,
            cfg,
            Some(Arc::clone(&shared.studio_stream)),
        );
        let flag = mgr.stop_flag();
        (res, flag)
    };
    invalidate_embed_detect_cache(shared);

    match launch_res {
        Ok(crate::studio_embed::EmbedLaunchOutcome::Connected(embed_state, mcp)) => {
            let (frame_seq, frame_meta, available_tools) = attach_embed_mcp_client(shared, *mcp);
            if !embed_state.x11grab_active {
                spawn_mcp_screen_capture_pump(Arc::clone(shared), stop_flag);
            }
            let status_obj = build_embed_status_payload(shared);
            Ok(json!({
                "ok": true,
                "running": true,
                "connected": true,
                "awaiting_signin": false,
                "place_open": embed_state.place_open,
                "phase": "connected",
                "studio_connected": true,
                "workspace_mode": "studio_mcp",
                "studio_mode": "live_mcp",
                "x11grab_active": embed_state.x11grab_active,
                "has_live_frame": frame_seq > 0,
                "frame_seq": frame_seq,
                "frame": frame_meta,
                "stream_url": "/api/studio/stream.mjpg",
                "frame_url": "/api/studio/frame",
                "available_tools": available_tools,
                "status": status_obj,
            }))
        }
        Ok(crate::studio_embed::EmbedLaunchOutcome::StreamingAwaitingAuth { state, mcp_cmd }) => {
            spawn_embed_mcp_auto_attacher(Arc::clone(shared), mcp_cmd, stop_flag);
            if state.x11grab_active && shared.studio_stream.latest_frame_seq() == 0 {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
                while shared.studio_stream.latest_frame_seq() == 0
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            let frame_seq = shared.studio_stream.latest_frame_seq();
            let status_obj = build_embed_status_payload(shared);
            Ok(json!({
                "ok": true,
                "running": true,
                "connected": false,
                "awaiting_signin": true,
                "phase": "awaiting_signin",
                "studio_connected": false,
                "studio_mode": "awaiting_signin",
                "x11grab_active": state.x11grab_active,
                "has_live_frame": frame_seq > 0,
                "frame_seq": frame_seq,
                "stream_url": "/api/studio/stream.mjpg",
                "frame_url": "/api/studio/frame",
                "message": "Studio viewport is streaming live — sign in or open a place to attach MCP automatically.",
                "status": status_obj,
            }))
        }
        Err(e) => Err(("502 Bad Gateway", e)),
    }
}

fn handle_studio_embed_launch(
    stream: &mut TcpStream,
    shared: &Arc<Shared>,
    req: &Request,
    attach_only: bool,
) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "launching Embedded Roblox Studio is only allowed from this machine (loopback)");
        return;
    }
    if shared.busy.load(Ordering::SeqCst) > 0 {
        respond_err(stream, "409 Conflict", "a build or playtest is currently running; launch Studio once it finishes");
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body for Studio embed launch");
                return;
            }
        }
    };
    match perform_studio_embed_launch(shared, &body, attach_only) {
        Ok(payload) => respond_json(stream, "200 OK", &payload),
        Err((status, msg)) => {
            if status == "400 Bad Request" {
                respond_err(stream, status, &msg);
                return;
            }
            let status_obj = build_embed_status_payload(shared);
            respond_json(
                stream,
                status,
                &json!({
                    "ok": false,
                    "running": false,
                    "connected": false,
                    "awaiting_signin": false,
                    "studio_mode": shared.studio_stream.studio_mode(),
                    "error": msg,
                    "status": status_obj,
                }),
            );
        }
    }
}

/// Shared execution core for `POST /api/studio/embed/transfer`, `POST /api/projects/{id}/open-in-studio`,
/// and `auto_open` in `sync`/`playtest` (#51).
pub(crate) fn perform_studio_embed_transfer(
    shared: &Arc<Shared>,
    opts: crate::studio_embed::TransferOptions,
) -> Result<Value, (&'static str, String)> {
    stage_active_project_studio_content_best_effort(shared, None);
    let (res, stop_flag) = {
        let mut mgr = shared.studio_embed.lock().unwrap();
        let res = crate::studio_embed::transfer_to_embedded_studio(
            &mut mgr,
            opts,
            Some(Arc::clone(&shared.studio_stream)),
        );
        let flag = mgr.stop_flag();
        (res, flag)
    };
    invalidate_embed_detect_cache(shared);

    let tr = res.map_err(|e| ("502 Bad Gateway", e))?;
    match tr.outcome {
        crate::studio_embed::EmbedLaunchOutcome::Connected(embed_state, mcp) => {
            let (mcp_frame_seq, frame_meta, available_tools) = attach_embed_mcp_client(shared, *mcp);
            if !embed_state.x11grab_active {
                spawn_mcp_screen_capture_pump(Arc::clone(shared), stop_flag);
            } else if shared.studio_stream.latest_frame_seq() == 0 {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
                while shared.studio_stream.latest_frame_seq() == 0
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            let frame_seq = mcp_frame_seq.max(shared.studio_stream.latest_frame_seq());
            let status_obj = build_embed_status_payload(shared);
            let msg = if tr.transferred {
                "Roblox Studio transferred into OpenRO's embedded viewport with your place open."
            } else if tr.attached_existing {
                "Attached to your running Roblox Studio place."
            } else {
                "Roblox Studio is open and connected."
            };
            Ok(json!({
                "ok": true,
                "running": true,
                "connected": true,
                "transferred": tr.transferred,
                "attached_existing": tr.attached_existing,
                "already_running": tr.attached_existing,
                "launched": !tr.attached_existing,
                "dry_run": false,
                "awaiting_signin": false,
                "place_open": embed_state.place_open,
                "phase": "connected",
                "studio_connected": true,
                "studio_id": embed_state.studio_id,
                "display": embed_state.display,
                "workspace_mode": "studio_mcp",
                "studio_mode": "live_mcp",
                "x11grab_active": embed_state.x11grab_active,
                "has_live_frame": frame_seq > 0,
                "frame_seq": frame_seq,
                "frame": frame_meta,
                "rbxlx_path": tr.rbxlx_path,
                "staged_path": tr.staged_path,
                "windows_path": tr.windows_path,
                "command": tr.command,
                "stream_url": "/api/studio/stream.mjpg",
                "frame_url": "/api/studio/frame",
                "available_tools": available_tools,
                "message": msg,
                "status": status_obj,
            }))
        }
        crate::studio_embed::EmbedLaunchOutcome::StreamingAwaitingAuth { state, mcp_cmd } => {
            spawn_embed_mcp_auto_attacher(Arc::clone(shared), mcp_cmd, stop_flag);
            if state.x11grab_active && shared.studio_stream.latest_frame_seq() == 0 {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
                while shared.studio_stream.latest_frame_seq() == 0
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            let frame_seq = shared.studio_stream.latest_frame_seq();
            let status_obj = build_embed_status_payload(shared);
            Ok(json!({
                "ok": true,
                "running": true,
                "connected": false,
                "transferred": tr.transferred,
                "attached_existing": false,
                "already_running": false,
                "launched": true,
                "dry_run": false,
                "awaiting_signin": true,
                "place_open": false,
                "phase": "awaiting_signin",
                "studio_connected": false,
                "studio_id": Value::Null,
                "display": state.display,
                "studio_mode": "awaiting_signin",
                "x11grab_active": state.x11grab_active,
                "has_live_frame": frame_seq > 0,
                "frame_seq": frame_seq,
                "rbxlx_path": tr.rbxlx_path,
                "staged_path": tr.staged_path,
                "windows_path": tr.windows_path,
                "command": tr.command,
                "stream_url": "/api/studio/stream.mjpg",
                "frame_url": "/api/studio/frame",
                "message": "Studio viewport is streaming live on the embedded display — sign in or wait for the place to finish loading; OpenRO attaches MCP automatically.",
                "status": status_obj,
            }))
        }
    }
}

fn handle_studio_embed_transfer(stream: &mut TcpStream, shared: &Arc<Shared>, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "transferring Embedded Roblox Studio is only allowed from this machine (loopback)");
        return;
    }
    if shared.busy.load(Ordering::SeqCst) > 0 {
        respond_err(stream, "409 Conflict", "a build or playtest is currently running; transfer Studio once it finishes");
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body for Studio embed transfer");
                return;
            }
        }
    };
    let display = match body.get("display").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => match crate::studio_embed::validate_x11_display(d) {
            Ok(valid) => valid,
            Err(e) => {
                respond_err(stream, "400 Bad Request", &e);
                return;
            }
        },
        None => crate::studio_embed::DEFAULT_EMBED_DISPLAY.to_string(),
    };
    let width = body
        .get("width")
        .and_then(|v| v.as_u64())
        .map(|w| (w as u32).clamp(320, 3840))
        .unwrap_or(crate::studio_embed::DEFAULT_EMBED_WIDTH);
    let height = body
        .get("height")
        .and_then(|v| v.as_u64())
        .map(|h| (h as u32).clamp(200, 2160))
        .unwrap_or(crate::studio_embed::DEFAULT_EMBED_HEIGHT);
    let force_transfer = body.get("transfer").and_then(|v| v.as_bool()).unwrap_or(true);
    let rebuild = body.get("rebuild").and_then(|v| v.as_bool()).unwrap_or(true);
    let wait_secs = body
        .get("wait_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(45)
        .clamp(1, 90);
    let mcp_command = body
        .get("mcp_command")
        .or_else(|| body.get("command"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Resolve place_file: explicit place_file -> specified/active project build -> synthesized blank Baseplate.
    let mut place_file: Option<PathBuf> = None;
    if let Some(raw_pf) = body
        .get("place_file")
        .or_else(|| body.get("rbxlx_path"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if raw_pf.contains('\0') || raw_pf.contains('\n') || raw_pf.contains('\r') {
            respond_err(stream, "400 Bad Request", "place_file must not contain control characters");
            return;
        }
        let p = PathBuf::from(raw_pf);
        if p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            respond_err(stream, "400 Bad Request", "place_file must not contain '..' traversal segments");
            return;
        }
        place_file = Some(p);
    } else {
        let proj_ref = {
            let store = shared.projects.lock().unwrap();
            if let Some(pid) = body.get("project_id").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                store.find(pid)
            } else {
                store.active.as_deref().and_then(|aid| store.find(aid))
            }
        };
        if let Some(r) = proj_ref {
            let existing = if !rebuild {
                crate::projects::builds(&r.dir).into_iter().find_map(|rec| {
                    let rel = rec.get("path")?.as_str()?;
                    let full = crate::projects::safe_join(&r.dir, rel).ok()?;
                    full.is_file().then_some(full)
                })
            } else {
                None
            };
            place_file = existing.or_else(|| {
                crate::projects::build(&r.dir, "auto", None)
                    .ok()
                    .and_then(|rec| {
                        let rel = rec.get("path")?.as_str()?;
                        crate::projects::safe_join(&r.dir, rel).ok()
                    })
            });
        }
        if place_file.is_none() {
            let base_dir = shared
                .data_dir
                .clone()
                .unwrap_or_else(|| std::env::temp_dir().join("openro-baseplates"));
            if let Ok(bp) = crate::studio_embed::ensure_blank_baseplate_rbxlx(&base_dir, &shared.game_name) {
                place_file = Some(bp);
            }
        }
    }

    stage_active_project_studio_content_best_effort(
        shared,
        body.get("project_id").and_then(Value::as_str),
    );
    let opts = crate::studio_embed::TransferOptions {
        display,
        width,
        height,
        place_file,
        force_transfer,
        wait_secs,
        mcp_command,
    };
    match perform_studio_embed_transfer(shared, opts) {
        Ok(payload) => respond_json(stream, "200 OK", &payload),
        Err((status, msg)) => {
            let status_obj = build_embed_status_payload(shared);
            respond_json(
                stream,
                status,
                &json!({
                    "ok": false,
                    "running": false,
                    "connected": false,
                    "awaiting_signin": false,
                    "error": msg,
                    "status": status_obj,
                }),
            );
        }
    }
}

fn stop_studio_embed_runtime(shared: &Shared) -> Value {
    {
        let mut mgr = shared.studio_embed.lock().unwrap();
        let _ = crate::studio_embed::stop_embedded_studio(&mut mgr);
    }
    invalidate_embed_detect_cache(shared);

    let local_place = match &shared.data_dir {
        Some(dir) => LocalPlace::open(dir.clone(), shared.game_name.clone()),
        None => LocalPlace::in_memory(shared.game_name.clone()),
    };
    {
        let mut guard = shared.agent.lock().unwrap();
        if let Some(agent) = guard.as_mut() {
            agent.studio = Some(PlaceBackend::Local(local_place));
            agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
            *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
        }
    }

    let mut ev = StudioActionInput::new(
        StudioActionKind::CameraFocus,
        "Stopped Embedded Roblox Studio — returned to Local Studio 3D",
    );
    ev.path = "Workspace.CurrentCamera".to_string();
    ev.class_name = "Camera".to_string();
    shared.studio_stream.record_action(ev);

    let status_obj = build_embed_status_payload(shared);
    json!({
        "ok": true,
        "running": false,
        "connected": false,
        "stopped": true,
        "studio_connected": true,
        "workspace_mode": "local_place",
        "studio_mode": "local_place",
        "status": status_obj,
    })
}

fn handle_studio_embed_stop(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "stopping Embedded Roblox Studio is only allowed from this machine (loopback)");
        return;
    }
    if shared.busy.load(Ordering::SeqCst) > 0 {
        respond_err(stream, "409 Conflict", "a build or playtest is currently running; stop Studio once it finishes");
        return;
    }
    let payload = stop_studio_embed_runtime(shared);
    respond_json(stream, "200 OK", &payload);
}

/// Capture the current live/embedded Studio frame to `<data_dir>/cua-frames/<id>.png`
/// (CWE-22/59 safe, pruned to 40 frames).
pub fn capture_studio_frame_png(
    shared: &Shared,
    data_dir: &std::path::Path,
    label: Option<&str>,
) -> Result<Value, String> {
    if shared.studio_stream.latest_frame().is_none() {
        if let Ok(mut guard) = shared.agent.try_lock() {
            if let Some(agent) = guard.as_mut() {
                if matches!(agent.studio, Some(PlaceBackend::Mcp(_))) {
                    let _ = agent.capture_studio_frame();
                }
            }
        }
    }
    let state = shared
        .studio_embed
        .try_lock()
        .map(|g| g.state())
        .unwrap_or_else(|_| crate::studio_embed::StudioEmbedState::idle());
    crate::studio_embed::capture_studio_frame_png_from_hub(
        &shared.studio_stream,
        &state,
        data_dir,
        label,
    )
}

fn cua_data_dir(shared: &Shared) -> &std::path::Path {
    shared.data_dir.as_deref().unwrap_or(&shared.assets_root)
}

fn cua_embed_resolution(shared: &Shared) -> (Option<String>, [u32; 2], bool) {
    shared
        .studio_embed
        .try_lock()
        .ok()
        .map(|g| {
            let st = g.state();
            (st.display, st.resolution, st.x11grab_active)
        })
        .unwrap_or((
            None,
            [
                crate::studio_embed::DEFAULT_EMBED_WIDTH,
                crate::studio_embed::DEFAULT_EMBED_HEIGHT,
            ],
            false,
        ))
}

pub fn cua_status_json(shared: &Shared) -> Value {
    let embed = build_embed_status_payload(shared);
    let (_, res, _) = cua_embed_resolution(shared);
    let recent: Vec<Value> = shared
        .cua_log
        .recent(24)
        .into_iter()
        .map(|e| e.to_json())
        .collect();
    json!({
        "ok": true,
        "cua_paused": shared.cua_log.is_paused(),
        "running": embed["running"],
        "embedded": embed["embedded"],
        "state": embed["state"],
        "phase": embed["phase"],
        "platform": embed["platform"],
        "display": embed["display"],
        "resolution": res,
        "width": res[0],
        "height": res[1],
        "place_open": embed["place_open"],
        "mcp_connected": embed["mcp_connected"],
        "has_live_frame": embed["has_live_frame"],
        "frame_seq": embed["frame_seq"],
        "stream_url": embed["stream_url"],
        "recent_events": recent,
    })
}

fn cua_exec_screenshot(shared: &Shared, actor: &str, body: &Value) -> Result<Value, (u16, String)> {
    let label = body
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut shot = capture_studio_frame_png(shared, cua_data_dir(shared), label)
        .map_err(|e| (503, e))?;
    let frame_url = shot.get("url").and_then(Value::as_str).map(String::from);
    let ev = shared
        .cua_log
        .record(crate::studio_embed::StudioCuaEventInput {
            actor: actor.to_string(),
            kind: "screenshot".to_string(),
            summary: format!(
                "Captured Studio frame ({}x{})",
                shot["width"], shot["height"]
            ),
            norm_x: None,
            norm_y: None,
            to_norm_x: None,
            to_norm_y: None,
            key: None,
            hotkey: None,
            frame_url,
            ok: true,
        });
    if let Some(obj) = shot.as_object_mut() {
        obj.insert("event".to_string(), ev.to_json());
        obj.insert("cua_paused".to_string(), json!(shared.cua_log.is_paused()));
    }
    Ok(shot)
}

fn cua_exec_mouse(shared: &Shared, actor: &str, body: &Value) -> Result<Value, (u16, String)> {
    if shared.cua_log.is_paused() {
        return Err((
            409,
            "Agent CUA is paused by human takeover. Resume CUA before sending mouse input."
                .to_string(),
        ));
    }
    let (disp_opt, res, x11_active) = cua_embed_resolution(shared);
    let cmd = crate::studio_embed::validate_cua_mouse_payload(body, res)
        .map_err(|e| (400, e))?;

    let to_coords = cmd
        .to_norm_x
        .zip(cmd.to_norm_y);
    let x11_disp = disp_opt.filter(|d| {
        x11_active
            || std::path::Path::new(&format!("/tmp/.X11-unix/X{}", d.trim_start_matches(':'))).exists()
    });

    let result = if let Some(ref disp) = x11_disp {
        crate::studio_embed::send_x11_mouse(
            disp,
            res,
            &cmd.action,
            cmd.norm_x,
            cmd.norm_y,
            to_coords,
            &cmd.button,
            cmd.delta_y,
        )
        .map_err(|e| (502, e))?
    } else {
        let mut mcp_payload = json!({
            "action": cmd.action,
            "x": cmd.norm_x,
            "y": cmd.norm_y,
            "button": cmd.button,
            "delta_y": cmd.delta_y,
        });
        if let (Some(tx), Some(ty)) = (cmd.to_norm_x, cmd.to_norm_y) {
            mcp_payload["to_x"] = json!(tx);
            mcp_payload["to_y"] = json!(ty);
        }
        let mut guard = shared.agent.lock().unwrap();
        let Some(agent) = guard.as_mut() else {
            return Err((
                503,
                "No live Roblox Studio window or X11 display active. Launch or attach Studio first."
                    .to_string(),
            ));
        };
        if !matches!(agent.studio, Some(PlaceBackend::Mcp(_))) {
            return Err((
                503,
                "No live Roblox Studio window or X11 display active. Launch or attach Studio first."
                    .to_string(),
            ));
        }
        let chat_id = shared.active_chat.lock().unwrap().clone();
        agent
            .invoke_studio_tool(&chat_id, "user_mouse_input", mcp_payload)
            .map(|(v, _)| v)
            .map_err(|e| (502, e))?
    };

    if let Ok(mut mgr) = shared.studio_embed.try_lock() {
        mgr.record_input(format!(
            "cua {} ({:.2}, {:.2})",
            cmd.action, cmd.norm_x, cmd.norm_y
        ));
    }
    let summary = if cmd.action == "drag" {
        format!(
            "{actor} · drag ({:.2}, {:.2}) -> ({:.2}, {:.2})",
            cmd.norm_x,
            cmd.norm_y,
            cmd.to_norm_x.unwrap_or(cmd.norm_x),
            cmd.to_norm_y.unwrap_or(cmd.norm_y)
        )
    } else {
        format!("{actor} · {} ({:.2}, {:.2})", cmd.action, cmd.norm_x, cmd.norm_y)
    };
    let ev = shared
        .cua_log
        .record(crate::studio_embed::StudioCuaEventInput {
            actor: actor.to_string(),
            kind: cmd.action.clone(),
            summary,
            norm_x: Some(cmd.norm_x),
            norm_y: Some(cmd.norm_y),
            to_norm_x: cmd.to_norm_x,
            to_norm_y: cmd.to_norm_y,
            key: None,
            hotkey: None,
            frame_url: None,
            ok: true,
        });
    Ok(json!({
        "ok": true,
        "cua_paused": false,
        "event": ev.to_json(),
        "result": result,
    }))
}

fn cua_exec_keyboard(shared: &Shared, actor: &str, body: &Value) -> Result<Value, (u16, String)> {
    if shared.cua_log.is_paused() {
        return Err((
            409,
            "Agent CUA is paused by human takeover. Resume CUA before sending keyboard input."
                .to_string(),
        ));
    }
    let cmd = crate::studio_embed::validate_cua_keyboard_payload(body).map_err(|e| (400, e))?;
    let (disp_opt, _, x11_active) = cua_embed_resolution(shared);
    let x11_disp = disp_opt.filter(|d| {
        x11_active
            || std::path::Path::new(&format!("/tmp/.X11-unix/X{}", d.trim_start_matches(':'))).exists()
    });

    let result = if let Some(ref disp) = x11_disp {
        crate::studio_embed::send_x11_keyboard(
            disp,
            &cmd.action,
            cmd.key.as_deref(),
            cmd.text.as_deref(),
            cmd.hotkey.as_deref(),
        )
        .map_err(|e| (502, e))?
    } else {
        let mut mcp_payload = json!({ "action": cmd.action });
        if let Some(ref hk) = cmd.hotkey {
            mcp_payload["hotkey"] = json!(hk);
        }
        if let Some(ref k) = cmd.key {
            mcp_payload["key"] = json!(k);
        }
        if let Some(ref t) = cmd.text {
            mcp_payload["text"] = json!(t);
        }
        let mut guard = shared.agent.lock().unwrap();
        let Some(agent) = guard.as_mut() else {
            return Err((
                503,
                "No live Roblox Studio window or X11 display active. Launch or attach Studio first."
                    .to_string(),
            ));
        };
        if !matches!(agent.studio, Some(PlaceBackend::Mcp(_))) {
            return Err((
                503,
                "No live Roblox Studio window or X11 display active. Launch or attach Studio first."
                    .to_string(),
            ));
        }
        let chat_id = shared.active_chat.lock().unwrap().clone();
        agent
            .invoke_studio_tool(&chat_id, "user_keyboard_input", mcp_payload)
            .map(|(v, _)| v)
            .map_err(|e| (502, e))?
    };

    let summary = if let Some(ref hk) = cmd.hotkey {
        format!("{actor} · {}", hk.join("+"))
    } else if let Some(ref t) = cmd.text {
        let preview: String = t.chars().take(24).collect();
        format!("{actor} · type \"{preview}\"")
    } else {
        format!("{actor} · key {}", cmd.key.as_deref().unwrap_or(""))
    };
    let ev = shared
        .cua_log
        .record(crate::studio_embed::StudioCuaEventInput {
            actor: actor.to_string(),
            kind: cmd.action.clone(),
            summary,
            norm_x: None,
            norm_y: None,
            to_norm_x: None,
            to_norm_y: None,
            key: cmd.key.clone(),
            hotkey: cmd.hotkey.clone(),
            frame_url: None,
            ok: true,
        });
    Ok(json!({
        "ok": true,
        "cua_paused": false,
        "event": ev.to_json(),
        "result": result,
    }))
}

fn prepare_cua_window_body(shared: &Shared, body: &Value, is_transfer: bool) -> Result<Value, (u16, String)> {
    let mut out = if body.is_object() {
        body.clone()
    } else {
        json!({})
    };
    let project_id = out
        .get("project_id")
        .or_else(|| out.get("project"))
        .and_then(Value::as_str)
        .map(String::from);
    let explicit_pf = out
        .get("place_file")
        .or_else(|| out.get("rbxlx_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    if let Some(raw_pf) = explicit_pf {
        if raw_pf.contains('\0') || raw_pf.contains('\n') || raw_pf.contains('\r') {
            return Err((400, "place_file must not contain control characters".to_string()));
        }
        let p = PathBuf::from(&raw_pf);
        if p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err((
                400,
                "place_file must not contain '..' traversal segments".to_string(),
            ));
        }
        if !p.is_absolute() {
            let proj_dir = {
                let store = shared.projects.lock().unwrap();
                project_id
                    .as_deref()
                    .and_then(|pid| store.find(pid))
                    .or_else(|| store.active.as_deref().and_then(|aid| store.find(aid)))
                    .map(|r| r.dir)
            };
            if let Some(dir) = proj_dir {
                let resolved = crate::projects::safe_join(&dir, &raw_pf).map_err(|e| (400, e))?;
                if let Some(obj) = out.as_object_mut() {
                    obj.insert(
                        "place_file".to_string(),
                        json!(resolved.to_string_lossy().into_owned()),
                    );
                }
            }
        }
    } else if let Some(latest_rbxlx) =
        resolve_active_project_latest_rbxlx(shared, project_id.as_deref())
    {
        if let Some(obj) = out.as_object_mut() {
            obj.insert(
                "place_file".to_string(),
                json!(latest_rbxlx.to_string_lossy().into_owned()),
            );
        }
    }

    if is_transfer {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("transfer".to_string(), json!(true));
        }
    }
    Ok(out)
}

fn cua_exec_window(
    shared: &Arc<Shared>,
    actor: &str,
    body: &Value,
) -> Result<Value, (u16, String)> {
    let action = body
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status")
        .trim()
        .to_ascii_lowercase();
    match action.as_str() {
        "status" => Ok(cua_status_json(shared)),
        "pause" => {
            let ev = shared.cua_log.pause(actor);
            Ok(json!({
                "ok": true,
                "action": "pause",
                "cua_paused": true,
                "event": ev.to_json(),
            }))
        }
        "resume" => {
            let ev = shared.cua_log.resume(actor);
            Ok(json!({
                "ok": true,
                "action": "resume",
                "cua_paused": false,
                "event": ev.to_json(),
            }))
        }
        "stop" => {
            let mut out = stop_studio_embed_runtime(shared);
            let ev = shared
                .cua_log
                .record(crate::studio_embed::StudioCuaEventInput {
                    actor: actor.to_string(),
                    kind: "window_stop".to_string(),
                    summary: format!("{actor} · Studio window stop"),
                    norm_x: None,
                    norm_y: None,
                    to_norm_x: None,
                    to_norm_y: None,
                    key: None,
                    hotkey: None,
                    frame_url: None,
                    ok: true,
                });
            if let Some(obj) = out.as_object_mut() {
                obj.insert("action".to_string(), json!("stop"));
                obj.insert("event".to_string(), ev.to_json());
                obj.insert("cua_paused".to_string(), json!(shared.cua_log.is_paused()));
            }
            Ok(out)
        }
        "launch" | "attach" => {
            let launch_body = if action == "launch" {
                prepare_cua_window_body(shared, body, false)?
            } else {
                body.clone()
            };
            let mut out = perform_studio_embed_launch(shared, &launch_body, action == "attach")
                .map_err(|(st, msg)| {
                    let code = if st.starts_with("400") { 400 } else { 502 };
                    (code, msg)
                })?;
            let ev = shared
                .cua_log
                .record(crate::studio_embed::StudioCuaEventInput {
                    actor: actor.to_string(),
                    kind: format!("window_{action}"),
                    summary: format!("{actor} · Studio window {action}"),
                    norm_x: None,
                    norm_y: None,
                    to_norm_x: None,
                    to_norm_y: None,
                    key: None,
                    hotkey: None,
                    frame_url: None,
                    ok: true,
                });
            if let Some(obj) = out.as_object_mut() {
                obj.insert("event".to_string(), ev.to_json());
                obj.insert("cua_paused".to_string(), json!(shared.cua_log.is_paused()));
            }
            Ok(out)
        }
        "transfer" => {
            let transfer_body = prepare_cua_window_body(shared, body, true)?;
            let mut out = perform_studio_embed_launch(shared, &transfer_body, false)
                .map_err(|(st, msg)| {
                    let code = if st.starts_with("400") { 400 } else { 502 };
                    (code, msg)
                })?;
            let ev = shared
                .cua_log
                .record(crate::studio_embed::StudioCuaEventInput {
                    actor: actor.to_string(),
                    kind: "window_transfer".to_string(),
                    summary: format!("{actor} · Studio window transfer"),
                    norm_x: None,
                    norm_y: None,
                    to_norm_x: None,
                    to_norm_y: None,
                    key: None,
                    hotkey: None,
                    frame_url: None,
                    ok: true,
                });
            if let Some(obj) = out.as_object_mut() {
                obj.insert("event".to_string(), ev.to_json());
                obj.insert("cua_paused".to_string(), json!(shared.cua_log.is_paused()));
            }
            Ok(out)
        }
        other => Err((
            400,
            format!("unsupported CUA window action '{other}' (expected status, launch, attach, transfer, stop, pause, or resume)"),
        )),
    }
}

/// Shared dispatch entry point for `studio_cua_*` tools from both Agent hooks and Teamwork.
pub fn execute_cua_tool(
    shared: &Arc<Shared>,
    actor: &str,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    match tool {
        "studio_cua_screenshot" => cua_exec_screenshot(shared, actor, args).map_err(|(_, e)| e),
        "studio_cua_mouse" => cua_exec_mouse(shared, actor, args).map_err(|(_, e)| e),
        "studio_cua_keyboard" => cua_exec_keyboard(shared, actor, args).map_err(|(_, e)| e),
        "studio_cua_window" => cua_exec_window(shared, actor, args).map_err(|(_, e)| e),
        other => Err(format!("unknown Studio CUA tool '{other}'")),
    }
}

fn try_route_studio_cua(
    stream: &mut TcpStream,
    shared: &Arc<Shared>,
    req: &Request,
    path: &str,
    query: &str,
) -> bool {
    if path != "/api/studio/cua" && !path.starts_with("/api/studio/cua/") {
        return false;
    }
    if !host_allowed(&req.host, &[]) {
        respond_err(
            stream,
            "403 Forbidden",
            "Studio CUA endpoints are only allowed from this machine (loopback)",
        );
        return true;
    }

    if req.method == "GET" && path == "/api/studio/cua/status" {
        respond_json(stream, "200 OK", &cua_status_json(shared));
        return true;
    }
    if req.method == "GET" && path == "/api/studio/cua/events" {
        let since = query_param(query, "since")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let limit = query_param(query, "limit")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(crate::studio_embed::MAX_CUA_LOG_ENTRIES);
        let list = if since > 0 {
            shared.cua_log.since(since, limit)
        } else {
            shared.cua_log.recent(limit)
        };
        let events: Vec<Value> = list.into_iter().map(|e| e.to_json()).collect();
        respond_json(
            stream,
            "200 OK",
            &json!({
                "ok": true,
                "cua_paused": shared.cua_log.is_paused(),
                "events": events,
            }),
        );
        return true;
    }
    if req.method == "GET" && path.starts_with("/api/studio/cua/frame/") {
        let file = path.trim_start_matches("/api/studio/cua/frame/");
        match crate::studio_embed::resolve_cua_frame_path(cua_data_dir(shared), file) {
            Ok(resolved) => match std::fs::read(&resolved) {
                Ok(bytes) => {
                    respond_bytes(stream, "200 OK", "image/png", "no-store", &bytes);
                }
                Err(_) => respond_err(stream, "404 Not Found", "CUA frame not found"),
            },
            Err((code, msg)) => {
                respond_err(stream, status_line_for_code(code), &msg);
            }
        }
        return true;
    }

    if req.method != "POST" {
        return false;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON object body for CUA action");
                return true;
            }
        }
    };
    let actor = body
        .get("actor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("agent");

    let res = match path {
        "/api/studio/cua/screenshot" => cua_exec_screenshot(shared, actor, &body),
        "/api/studio/cua/mouse" => cua_exec_mouse(shared, actor, &body),
        "/api/studio/cua/keyboard" => cua_exec_keyboard(shared, actor, &body),
        "/api/studio/cua/window" => cua_exec_window(shared, actor, &body),
        _ => return false,
    };
    match res {
        Ok(val) => respond_json(stream, "200 OK", &val),
        Err((code, msg)) => respond_json(
            stream,
            status_line_for_code(code),
            &json!({
                "ok": false,
                "cua_paused": shared.cua_log.is_paused(),
                "error": msg,
            }),
        ),
    }
    true
}

fn respond_library_error(stream: &mut TcpStream, err: crate::library::LibraryError) {
    let status = match err.status {
        400 => "400 Bad Request",
        404 => "404 Not Found",
        _ => "500 Internal Server Error",
    };
    respond_err(stream, status, &err.message);
}

fn sync_agent_library_prompt(shared: &Shared) {
    invalidate_library_cache(shared);
    let prompt = shared.library.lock().unwrap().active_skill_prompt();
    let dossier_prompt = shared.last_researched_dossier_prompt.lock().unwrap().clone();
    if let Ok(mut guard) = shared.agent.lock() {
        if let Some(agent) = guard.as_mut() {
            agent.set_library_prompt(prompt);
            agent.set_market_dossier_prompt(dossier_prompt);
        }
    }
}

fn handle_library_install(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "installing library packs is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let res = shared.library.lock().unwrap().install(id);
    match res {
        Ok(v) => {
            sync_agent_library_prompt(shared);
            respond_json(stream, "200 OK", &v);
        }
        Err(e) => respond_library_error(stream, e),
    }
}

fn handle_library_uninstall(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "uninstalling library packs is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let res = shared.library.lock().unwrap().uninstall(id);
    match res {
        Ok(v) => {
            sync_agent_library_prompt(shared);
            respond_json(stream, "200 OK", &v);
        }
        Err(e) => respond_library_error(stream, e),
    }
}

fn handle_library_configure(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "configuring library packs is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let res = shared.library.lock().unwrap().configure(id, &body);
    match res {
        Ok(v) => {
            sync_agent_library_prompt(shared);
            respond_json(stream, "200 OK", &v);
        }
        Err(e) => respond_library_error(stream, e),
    }
}

fn handle_library_test(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "testing library packs is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let res = shared.library.lock().unwrap().test_pack(id);
    invalidate_library_cache(shared);
    match res {
        Ok(v) => respond_json(stream, "200 OK", &v),
        Err(e) => respond_library_error(stream, e),
    }
}

fn handle_market_taste_get(stream: &mut TcpStream, shared: &Shared, query: &str) {
    let niche_param = query_param(query, "niche");
    let canonical_niche = match niche_param {
        Some(raw) if !raw.trim().is_empty() => match crate::market_taste::normalize_niche_id(raw) {
            Ok(id) => Some(id),
            Err(msg) => {
                respond_err(stream, "400 Bad Request", &msg);
                return;
            }
        },
        _ => None,
    };
    let snap = shared.market_taste.lock().unwrap().snapshot_json(canonical_niche);
    respond_json(stream, "200 OK", &snap);
}

fn handle_market_taste_niche(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "changing active market niche is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => v,
        _ => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "invalid JSON body; expected {\"niche\": \"...\"}" }),
            );
            return;
        }
    };
    let niche = body
        .get("niche")
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let res = shared.market_taste.lock().unwrap().set_active_niche(niche);
    match res {
        Ok(snap) => {
            sync_agent_library_prompt(shared);
            respond_json(stream, "200 OK", &snap);
        }
        Err(msg) => respond_err(stream, "400 Bad Request", &msg),
    }
}

fn handle_market_taste_refresh(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "refreshing live Roblox charts is only allowed from this machine (loopback)");
        return;
    }
    let res = shared
        .market_taste
        .lock()
        .unwrap()
        .refresh_live(std::time::Duration::from_secs(12));
    match res {
        Ok(snap) => {
            sync_agent_library_prompt(shared);
            respond_json(stream, "200 OK", &snap);
        }
        Err(msg) => {
            let fallback = shared.market_taste.lock().unwrap().snapshot_json(None);
            respond_json(
                stream,
                "502 Bad Gateway",
                &json!({
                    "ok": false,
                    "error": msg,
                    "snapshot": fallback,
                }),
            );
        }
    }
}

fn handle_market_taste_research(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "researching Roblox games is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => v,
        _ => {
            respond_json(
                stream,
                "400 Bad Request",
                &json!({ "ok": false, "error": "invalid JSON body; expected {\"target\": \"<Roblox URL or Place/Universe ID>\"}" }),
            );
            return;
        }
    };
    let target = body
        .get("target")
        .or_else(|| body.get("url"))
        .or_else(|| body.get("place_or_universe_id"))
        .or_else(|| body.get("input"))
        .or_else(|| body.get("place_id"))
        .or_else(|| body.get("universe_id"))
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_default();
    if target.trim().is_empty() {
        respond_err(stream, "400 Bad Request", "missing Roblox game URL, Place ID, or Universe ID");
        return;
    }
    let niche_hint = body
        .get("niche_hint")
        .or_else(|| body.get("niche"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Run network research without holding `shared.market_taste` lock.
    let res = crate::market_taste::research_roblox_game(&shared.assets_root, &target, niche_hint);
    match res {
        Ok(val) => {
            // Also inject the synthesized reference game dossier into the Agent's system prompt
            // so the very next build inherits its craft bar & automatic twists.
            let mut dossier_block = String::new();
            if let Some(md) = val
                .get("markdown")
                .or_else(|| val.get("dossier_markdown"))
                .and_then(|v| v.as_str())
            {
                dossier_block = md.to_string();
            } else if let Some(title) = val.get("name").and_then(|v| v.as_str()) {
                let niche = val.get("niche").and_then(|v| v.as_str()).unwrap_or("anime-rng-cards");
                let craft = val.get("craft_bar").and_then(|v| v.as_str()).unwrap_or("");
                dossier_block = format!(
                    "## Researched Roblox Reference Game: {title} (niche `{niche}`)\nCraft Bar: {craft}"
                );
            }
            if !dossier_block.is_empty() {
                *shared.last_researched_dossier_prompt.lock().unwrap() = dossier_block;
                sync_agent_library_prompt(shared);
            }
            respond_json(stream, "200 OK", &val);
        }
        Err(msg) => {
            let low = msg.to_ascii_lowercase();
            let status = if low.contains("invalid")
                || low.contains("empty")
                || low.contains("expected")
                || low.contains("parse")
                || low.contains("provide a roblox")
            {
                "400 Bad Request"
            } else {
                "502 Bad Gateway"
            };
            respond_err(stream, status, &msg);
        }
    }
}

fn handle_proposals_list(stream: &mut TcpStream, shared: &Shared, query: &str) {
    let status_filter = query_param(query, "status").filter(|s| !s.is_empty());
    let chat_filter = query_param(query, "chat_id").filter(|s| !s.is_empty());
    let store = shared.proposals.lock().unwrap();
    let proposals = store.list(status_filter, chat_filter);
    let pending_count = store.pending_count();
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "proposals": proposals,
            "pending_count": pending_count,
        }),
    );
}

fn handle_proposal_apply(stream: &mut TcpStream, shared: &Shared, req: &Request, prop_id: &str) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };

    let selected_paths: Option<Vec<String>> = match body.get("selected_paths") {
        None | Some(Value::Null) => None,
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let Some(s) = item.as_str().map(str::trim) else {
                    respond_err(stream, "400 Bad Request", "selected_paths entries must be strings");
                    return;
                };
                if let Err(e) = crate::agent::check_path(s) {
                    respond_err(stream, "400 Bad Request", &e);
                    return;
                }
                out.push(s.to_string());
            }
            if out.is_empty() {
                respond_err(stream, "400 Bad Request", "selected_paths must contain at least one path");
                return;
            }
            Some(out)
        }
        Some(_) => {
            respond_err(stream, "400 Bad Request", "selected_paths must be an array of paths");
            return;
        }
    };

    let prop = {
        let store = shared.proposals.lock().unwrap();
        match store.get(prop_id) {
            Some(p) => p.clone(),
            None => {
                respond_json(
                    stream,
                    "404 Not Found",
                    &json!({ "ok": false, "error": format!("proposal not found: {prop_id}") }),
                );
                return;
            }
        }
    };
    if prop.status != "pending" {
        respond_json(
            stream,
            "409 Conflict",
            &json!({ "ok": false, "error": format!("proposal {prop_id} is already {}", prop.status) }),
        );
        return;
    }

    let (applied_paths, applied_diff, applied_file_diffs, snapshot_id, playtest) = {
        let mut guard = shared.agent.lock().unwrap();
        let Some(agent) = guard.as_mut() else {
            respond_err(stream, "503 Service Unavailable", "Studio not connected");
            return;
        };
        let live_before = match agent.place_tree() {
            Ok(t) => t,
            Err(e) => {
                respond_json(
                    stream,
                    "502 Bad Gateway",
                    &json!({ "ok": false, "error": format!("failed to read active place tree: {e}") }),
                );
                return;
            }
        };
        let (merged_tree, applied_paths) = merge_selected_tree(
            &live_before,
            &prop.before_tree,
            &prop.after_tree,
            selected_paths.as_deref(),
        );
        if let Err(e) = agent.apply_place_tree(&merged_tree) {
            respond_json(
                stream,
                "502 Bad Gateway",
                &json!({ "ok": false, "error": format!("failed to apply proposal to Studio: {e}") }),
            );
            return;
        }
        let live_after = agent.place_tree().unwrap_or(merged_tree);
        let applied_diff = compute_diff(&live_before, &live_after);
        let applied_file_diffs: Vec<Value> = compute_file_diffs(&live_before, &live_after)
            .iter()
            .map(ScriptFileDiff::to_json)
            .collect();
        agent.record_direct_edit(&prop.chat_id, &live_before, &live_after);
        if !prop.skills_used.is_empty() {
            shared.note_built(&prop.chat_id, &prop.skills_used);
        }
        let label = format!(
            "Applied: {}",
            prop.message.chars().take(48).collect::<String>()
        );
        let snap = shared.snapshots.lock().unwrap().record(
            label,
            "proposal_apply",
            Some(prop.chat_id.clone()),
            live_after,
            Some(applied_diff.clone()),
        );
        let playtest = agent.run_studio_playtest().unwrap_or_else(|e| {
            json!({
                "ok": false,
                "passed": false,
                "status": "failed",
                "console": e,
                "console_lines": [e],
            })
        });
        *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
        (
            applied_paths,
            applied_diff,
            applied_file_diffs,
            snap.id,
            playtest,
        )
    };

    let updated = shared
        .proposals
        .lock()
        .unwrap()
        .mark_status(prop_id, "applied")
        .unwrap_or(prop);
    let passed = playtest
        .get("passed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    shared.append_lines_to(
        &updated.chat_id,
        &[format!(
            "OpenRO: Applied {} change(s) from proposal {} (playtest {}).",
            applied_paths.len(),
            updated.id,
            if passed { "passed" } else { "reported errors" }
        )],
    );
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "proposal": updated.to_json(),
            "applied_paths": applied_paths,
            "playtest": playtest,
            "snapshot_id": snapshot_id,
            "diff": applied_diff.to_json(),
            "file_diffs": applied_file_diffs,
        }),
    );
}

fn handle_proposal_reject(stream: &mut TcpStream, shared: &Shared, prop_id: &str) {
    let mut store = shared.proposals.lock().unwrap();
    let Some(existing) = store.get(prop_id) else {
        respond_json(
            stream,
            "404 Not Found",
            &json!({ "ok": false, "error": format!("proposal not found: {prop_id}") }),
        );
        return;
    };
    if existing.status != "pending" {
        respond_json(
            stream,
            "409 Conflict",
            &json!({ "ok": false, "error": format!("proposal {prop_id} is already {}", existing.status) }),
        );
        return;
    }
    let updated = store.mark_status(prop_id, "rejected").unwrap();
    drop(store);
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "proposal": updated.to_json(),
        }),
    );
}

fn request_port(req: &Request, shared: &Shared) -> u16 {
    let host = req.host.trim();
    if let Some(after_bracket) = host.strip_prefix('[') {
        if let Some((_, rest)) = after_bracket.split_once("]:") {
            if let Ok(p) = rest.parse::<u16>() {
                if p > 0 {
                    return p;
                }
            }
        }
    } else if let Some((_, port_str)) = host.rsplit_once(':') {
        if let Ok(p) = port_str.parse::<u16>() {
            if p > 0 {
                return p;
            }
        }
    }
    shared.bound_port.load(Ordering::Relaxed)
}

fn handle_studio_plugin_status(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "Companion Studio Plugin status is loopback-only");
        return;
    }
    let mut st = shared.studio_plugin.status_json();
    if let Some(o) = st.as_object_mut() {
        o.insert(
            "auto_install_plugin".to_string(),
            json!(crate::studio_plugin::auto_install_pref(shared.data_dir.as_deref())),
        );
    }
    respond_json(stream, "200 OK", &st);
}

/// 1-click "Launch Studio & continue" endpoint (#29):
/// If live Studio MCP is already connected, returns `ready: true`. Otherwise tries to
/// attach an existing running Studio MCP session, or delegates to `handle_studio_embed_launch`.
fn handle_studio_launch_and_continue(stream: &mut TcpStream, shared: &Arc<Shared>, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "launching Studio is only allowed from this machine (loopback)");
        return;
    }
    {
        let mut guard = shared.agent.lock().unwrap();
        if let Some(agent) = guard.as_mut() {
            if let Some(PlaceBackend::Mcp(mcp)) = agent.studio.as_mut() {
                if mcp.has_live_studio() {
                    respond_json(
                        stream,
                        "200 OK",
                        &json!({
                            "ok": true,
                            "already_connected": true,
                            "studio_connected": true,
                            "ready": true,
                            "studio_id": mcp.studio_id(),
                        }),
                    );
                    return;
                }
            }
        }
    }
    if let Some(cmd) = shared
        .config
        .lock()
        .unwrap()
        .mcp_command
        .clone()
        .or_else(McpClient::detect_vinegar_studio_mcp)
    {
        if let Ok(mut mcp) = McpClient::spawn(&cmd) {
            if mcp.has_live_studio() {
                let sid = mcp.studio_id().map(String::from);
                shared.studio_embed.lock().unwrap().sync_from_client(&mcp, Some(cmd));
                let mut guard = shared.agent.lock().unwrap();
                if let Some(agent) = guard.as_mut() {
                    agent.studio = Some(PlaceBackend::Mcp(mcp));
                    agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
                    *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
                }
                respond_json(
                    stream,
                    "200 OK",
                    &json!({
                        "ok": true,
                        "attached_existing": true,
                        "studio_connected": true,
                        "ready": true,
                        "studio_id": sid,
                    }),
                );
                return;
            }
        }
    }
    handle_studio_embed_launch(stream, shared, req, false);
}

fn handle_studio_plugin_download(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "downloading OpenRoCompanion.lua is loopback-only");
        return;
    }
    let port = request_port(req, shared);
    let token = shared.studio_plugin.token_for_local_template();
    let lua = crate::studio_plugin::render_companion_lua(port, &token);
    respond_download(
        stream,
        "text/plain; charset=utf-8",
        crate::studio_plugin::COMPANION_PLUGIN_FILENAME,
        &lua,
    );
}

fn handle_studio_plugin_install(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "installing OpenRoCompanion.lua is loopback-only");
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    let requested_dir = body
        .get("target_dir")
        .or_else(|| body.get("path"))
        .and_then(|v| v.as_str());
    let port = request_port(req, shared);
    let token = shared.studio_plugin.token_for_local_template();
    match crate::studio_plugin::install_companion_plugin(port, &token, requested_dir) {
        Ok(installed_path) => respond_json(
            stream,
            "200 OK",
            &json!({
                "ok": true,
                "install_path": installed_path.to_string_lossy(),
                "installed": true,
                "port": port,
            }),
        ),
        Err(msg) => {
            let low = msg.to_ascii_lowercase();
            let status = if low.contains("was not found") || low.contains("does not exist") {
                "404 Not Found"
            } else if low.contains("traversal") || low.contains("refusing") || low.contains("symlink") {
                "403 Forbidden"
            } else {
                "400 Bad Request"
            };
            respond_err(stream, status, &msg);
        }
    }
}

fn require_plugin_auth(stream: &mut TcpStream, shared: &Shared, req: &Request) -> bool {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "Companion Studio Plugin endpoints are loopback-only");
        return false;
    }
    if !shared.studio_plugin.verify_token(&req.plugin_token) {
        respond_err(stream, "401 Unauthorized", "missing or invalid X-OpenRo-Plugin-Token");
        return false;
    }
    true
}

fn handle_studio_plugin_handshake(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !require_plugin_auth(stream, shared, req) {
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    let studio_name = body
        .get("studio_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Roblox Studio");
    let place_name = body
        .get("place_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let selection: Vec<String> = body
        .get("selection")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let payload = shared
        .studio_plugin
        .record_handshake(studio_name, place_name, selection);

    // Automatically attach PlaceBackend::Plugin when running on LocalPlace (never interrupts active MCP).
    if let Ok(mut guard) = shared.agent.try_lock() {
        if let Some(agent) = guard.as_mut() {
            let can_attach = matches!(agent.studio, None | Some(PlaceBackend::Local(_)));
            if can_attach {
                agent.studio = Some(PlaceBackend::Plugin(Arc::clone(&shared.studio_plugin)));
                agent.attach_studio_stream(Arc::clone(&shared.studio_stream));
                *shared.status_cache.lock().unwrap() = extract_status_from_agent(Some(agent));
            }
        }
    }

    respond_json(stream, "200 OK", &payload);
}

fn handle_studio_plugin_poll(stream: &mut TcpStream, shared: &Shared, req: &Request, query: &str) {
    if !require_plugin_auth(stream, shared, req) {
        return;
    }
    let timeout_ms = query_param(query, "timeout_ms")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10_000)
        .min(crate::studio_plugin::MAX_POLL_TIMEOUT_MS);
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(timeout_ms + 5_000)));
    let cmd = shared
        .studio_plugin
        .poll_next_command(std::time::Duration::from_millis(timeout_ms));
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "command": cmd.map(|c| c.to_json()),
        }),
    );
}

fn handle_studio_plugin_result(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !require_plugin_auth(stream, shared, req) {
        return;
    }
    let body: Value = match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => v,
        _ => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
            return;
        }
    };
    let Some(id) = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        respond_err(stream, "400 Bad Request", "missing command id");
        return;
    };
    let ok = body.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
    let result = body.get("result").cloned().unwrap_or_else(|| json!({ "ok": true }));
    let error = body
        .get("error")
        .and_then(|v| v.as_str())
        .map(String::from);
    let selection = body.get("selection").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    });
    if shared
        .studio_plugin
        .submit_result(id, ok, result, error, selection)
    {
        respond_json(stream, "200 OK", &json!({ "ok": true, "id": id }));
    } else {
        respond_json(
            stream,
            "404 Not Found",
            &json!({ "ok": false, "error": format!("unknown or expired command id: {id}") }),
        );
    }
}

fn handle_tools_blender_status(stream: &mut TcpStream) {
    respond_json(stream, "200 OK", &crate::mesh::blender_status(None));
}

fn handle_tools_blender_generate(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "generating Blender 3D meshes is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    let blender_override = body
        .get("binary_path")
        .and_then(|v| v.as_str())
        .map(String::from);
    let project_root = body
        .get("project")
        .and_then(|v| v.as_str())
        .and_then(|id| resolve_project_dir(shared, id))
        .or_else(|| shared.active_project_dir());
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let actx = crate::tool_adapters::InvokeCtx {
        data_dir,
        assets_root: &shared.assets_root,
        store: &store,
        project_dir: project_root.as_deref(),
    };
    let explicit = body.get("adapter_id").and_then(|v| v.as_str());
    let result = if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "mesh_gen", explicit, &body) {
        res
    } else if body.get("parts").is_some() || body.get("script").is_some() {
        crate::mesh::generate_for_project(
            &shared.assets_root,
            project_root.as_deref(),
            &body,
            blender_override.as_deref(),
        )
    } else {
        crate::library::generate_blender_mesh(
            &shared.assets_root,
            &body,
            blender_override.as_deref(),
        )
    };
    match result {
        Ok(v) => respond_json(stream, "200 OK", &v),
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn audio_generate_slug_filename(body: &Value) -> String {
    let raw = body
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| body.get("preset").and_then(Value::as_str))
        .or_else(|| body.get("prompt").and_then(Value::as_str))
        .unwrap_or("sfx");
    let stripped = raw
        .trim()
        .strip_suffix(".wav")
        .or_else(|| raw.trim().strip_suffix(".WAV"))
        .unwrap_or(raw.trim());
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in stripped.chars().take(64) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_sep && !slug.is_empty() {
            slug.push('-');
            prev_sep = true;
        }
    }
    let clean = slug.trim_matches('-');
    let candidate = if clean.is_empty() {
        "sfx.wav".to_string()
    } else {
        format!("{clean}.wav")
    };
    crate::audio_synth::sanitize_audio_filename(&candidate).unwrap_or_else(|_| "sfx.wav".to_string())
}

fn run_audio_generate(shared: &Shared, body: &Value) -> Result<Value, String> {
    let project_root = body
        .get("project")
        .or_else(|| body.get("project_id"))
        .and_then(|v| v.as_str())
        .and_then(|id| resolve_project_dir(shared, id))
        .or_else(|| shared.active_project_dir());
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let actx = crate::tool_adapters::InvokeCtx {
        data_dir,
        assets_root: &shared.assets_root,
        store: &store,
        project_dir: project_root.as_deref(),
    };
    let explicit = body.get("adapter_id").and_then(|v| v.as_str());

    let default_filename = audio_generate_slug_filename(body);
    let mut enriched_body = body.clone();
    if let Some(obj) = enriched_body.as_object_mut() {
        let has_out = obj
            .get("out_path")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_out {
            let default_out = if project_root.is_some() {
                format!("assets/audio/{default_filename}")
            } else {
                format!("game-assets/audio/{default_filename}")
            };
            obj.insert("out_path".into(), json!(default_out));
        }
        let has_prompt = obj
            .get("prompt")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !has_prompt {
            let fallback_prompt = obj
                .get("preset")
                .and_then(Value::as_str)
                .or_else(|| obj.get("name").and_then(Value::as_str))
                .unwrap_or("game sound effect")
                .to_string();
            obj.insert("prompt".into(), json!(fallback_prompt));
        }
    }

    if let Some(res) =
        crate::tool_adapters::maybe_invoke_override(&actx, "audio_gen", explicit, &enriched_body)
    {
        let mut v = res?;
        let rel_path = v
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| enriched_body.get("out_path").and_then(Value::as_str))
            .unwrap_or("assets/audio/sfx.wav")
            .to_string();
        let fname = std::path::Path::new(&rel_path)
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| crate::audio_synth::sanitize_audio_filename(s).ok())
            .unwrap_or_else(|| default_filename.clone());

        // Mirror project audio output into `<assets_root>/game-assets/audio/<fname>` so browser preview `url` always works.
        if let Some(ref root) = project_root {
            if let Ok(abs_src) = crate::projects::safe_join(root, &rel_path) {
                if let Ok(raw) = std::fs::read(&abs_src) {
                    let preview_dir = crate::audio_synth::audio_assets_dir(&shared.assets_root);
                    let _ = std::fs::create_dir_all(&preview_dir);
                    let dst = preview_dir.join(&fname);
                    let tmp = preview_dir.join(format!(".{fname}.{}.tmp", std::process::id()));
                    if std::fs::write(&tmp, &raw).is_ok() {
                        let _ = std::fs::rename(&tmp, &dst);
                    }
                }
            }
        }

        let url = format!("/api/assets/audio/{fname}");
        let adapter_id = v
            .get("adapter_id")
            .and_then(Value::as_str)
            .unwrap_or("custom-audio")
            .to_string();
        if let Some(map) = v.as_object_mut() {
            map.insert("ok".into(), json!(true));
            map.entry("path").or_insert_with(|| json!(rel_path));
            if project_root.is_some() {
                map.entry("project_path")
                    .or_insert_with(|| json!(rel_path));
            }
            map.insert("filename".into(), json!(fname));
            map.insert("url".into(), json!(url));
            map.insert("wav_url".into(), json!(url));
            map.insert("adapter_id".into(), json!(adapter_id));
            map.insert("source".into(), json!("adapter"));
        }
        return Ok(v);
    }

    let mut v = crate::audio_synth::generate_audio_asset_for_project(
        &shared.assets_root,
        project_root.as_deref(),
        body,
    )?;
    let fname = v
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or(&default_filename)
        .to_string();
    let wav_url = v
        .get("wav_url")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("/api/assets/audio/{fname}"));
    let path_str = v
        .get("project_path")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("game-assets/audio/{fname}"));
    if let Some(map) = v.as_object_mut() {
        map.insert("path".into(), json!(path_str));
        map.insert("url".into(), json!(wav_url));
        map.insert("adapter_id".into(), json!("rust-synth-builtin"));
        map.insert("source".into(), json!("builtin"));
    }
    Ok(v)
}

fn handle_tools_audio_generate(stream: &mut TcpStream, shared: &Shared, req: &Request) {
    if !host_allowed(&req.host, &[]) {
        respond_err(stream, "403 Forbidden", "generating audio assets is only allowed from this machine (loopback)");
        return;
    }
    let body: Value = if req.body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    match run_audio_generate(shared, &body) {
        Ok(v) => respond_json(stream, "200 OK", &v),
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn try_route_audio_library(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &Request,
    path: &str,
    query: &str,
) -> bool {
    let is_sfx = path == "/api/library/sfx-packs"
        || path.starts_with("/api/library/sfx-packs/")
        || path == "/api/library/audio-runner/install"
        || path == "/api/library/music"
        || path.starts_with("/api/library/music/")
        || path == "/api/library/music-runner/install";
    if !is_sfx {
        return false;
    }
    if !host_allowed(&req.host, &[]) {
        respond_err(
            stream,
            "403 Forbidden",
            "CC0 SFX library, music sourcing, and audio runner routes are only allowed from this machine (loopback)",
        );
        return true;
    }

    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);

    if req.method == "GET" && path == "/api/library/sfx-packs" {
        let body = crate::audio_library::list_sfx_packs(data_dir);
        respond_json(stream, "200 OK", &body);
        return true;
    }

    if req.method == "POST" && path == "/api/library/sfx-packs/install" {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        };
        let pack_id = body
            .get("pack_id")
            .or_else(|| body.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let custom_url = body.get("url").and_then(Value::as_str);
        match crate::audio_library::install_sfx_pack(data_dir, pack_id, custom_url) {
            Ok(v) => {
                invalidate_library_cache(shared);
                respond_json(stream, "200 OK", &v);
            }
            Err(e) => {
                let status = if e.contains("unknown CC0 SFX pack") {
                    "404 Not Found"
                } else if e.contains("download failed") || e.contains("download returned HTTP") {
                    "502 Bad Gateway"
                } else {
                    "400 Bad Request"
                };
                respond_err(stream, status, &e);
            }
        }
        return true;
    }

    if req.method == "GET" && path == "/api/library/sfx-packs/search" {
        let q = crate::project_api::query_param(query, "q")
            .or_else(|| crate::project_api::query_param(query, "query"))
            .unwrap_or_default();
        let pack_id = crate::project_api::query_param(query, "pack_id");
        let limit = crate::project_api::query_param(query, "limit")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50);
        let res = crate::audio_library::search_sfx_packs(data_dir, &q, pack_id.as_deref(), limit);
        respond_json(stream, "200 OK", &res);
        return true;
    }

    if req.method == "GET" {
        if let Some(rest) = path.strip_prefix("/api/library/sfx-packs/preview/") {
            let Some((pack_id, rel_path)) = rest.split_once('/') else {
                respond_err(stream, "400 Bad Request", "expected /api/library/sfx-packs/preview/:pack_id/:rel_path");
                return true;
            };
            match crate::audio_library::resolve_preview_clip_path(data_dir, pack_id, rel_path) {
                Ok((abs_path, mime)) => match std::fs::read(&abs_path) {
                    Ok(bytes) => {
                        respond_bytes(stream, "200 OK", mime, "no-store", &bytes);
                    }
                    Err(e) => respond_err(stream, "404 Not Found", &format!("read preview clip: {e}")),
                },
                Err(e) => {
                    let status = if e.contains("not found") || e.contains("not installed") {
                        "404 Not Found"
                    } else {
                        "400 Bad Request"
                    };
                    respond_err(stream, status, &e);
                }
            }
            return true;
        }
    }

    if req.method == "POST" && path == "/api/library/sfx-packs/use" {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        };
        let project_root = body
            .get("project")
            .or_else(|| body.get("project_id"))
            .and_then(Value::as_str)
            .and_then(|id| resolve_project_dir(shared, id))
            .or_else(|| shared.active_project_dir());
        match crate::audio_library::use_sfx_clip(data_dir, project_root.as_deref(), &body) {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err(e) => {
                let status = if e.contains("not found") || e.contains("not installed") {
                    "404 Not Found"
                } else {
                    "400 Bad Request"
                };
                respond_err(stream, status, &e);
            }
        }
        return true;
    }

    if req.method == "POST" && path == "/api/library/sfx-packs/import-local" {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        };
        let pack_id = body
            .get("pack_id")
            .or_else(|| body.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let name = body.get("name").and_then(Value::as_str).unwrap_or("");
        let license = body
            .get("license")
            .and_then(Value::as_str)
            .unwrap_or("CC0-1.0");
        let source_dir = body
            .get("source_dir")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if source_dir.is_empty() {
            respond_err(stream, "400 Bad Request", "source_dir is required");
            return true;
        }
        match crate::audio_library::import_local_sfx_pack(
            data_dir,
            pack_id,
            name,
            license,
            std::path::Path::new(source_dir),
        ) {
            Ok(v) => {
                invalidate_library_cache(shared);
                respond_json(stream, "200 OK", &v);
            }
            Err(e) => respond_err(stream, "400 Bad Request", &e),
        }
        return true;
    }

    if req.method == "DELETE" {
        if let Some(pack_id) = path.strip_prefix("/api/library/sfx-packs/") {
            if !pack_id.is_empty() && !pack_id.contains('/') {
                match crate::audio_library::remove_sfx_pack(data_dir, pack_id) {
                    Ok(v) => {
                        invalidate_library_cache(shared);
                        respond_json(stream, "200 OK", &v);
                    }
                    Err(e) => {
                        let status = if e.contains("not installed") {
                            "404 Not Found"
                        } else {
                            "400 Bad Request"
                        };
                        respond_err(stream, status, &e);
                    }
                }
                return true;
            }
        }
    }

    if req.method == "POST" && path == "/api/library/audio-runner/install" {
        match crate::audio_library::install_local_audio_runner(data_dir) {
            Ok(v) => {
                invalidate_library_cache(shared);
                respond_json(stream, "200 OK", &v);
            }
            Err(e) => respond_err(stream, "500 Internal Server Error", &e),
        }
        return true;
    }

    if req.method == "GET" && path == "/api/library/music/genres" {
        let body = crate::audio_library::music_genres_json(Some(data_dir));
        respond_json(stream, "200 OK", &body);
        return true;
    }

    if req.method == "POST" && path == "/api/library/music/search" {
        let body: Value = match parse_optional_json_object(&req.body) {
            Some(v) => v,
            None => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        };
        let genre = body.get("genre").and_then(Value::as_str);
        let query = body
            .get("query")
            .or_else(|| body.get("q"))
            .and_then(Value::as_str);
        let limit = body.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        match crate::audio_library::search_youtube_music(genre, query, limit) {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err(e) => respond_err(stream, music_error_status(&e), &e),
        }
        return true;
    }

    if req.method == "POST" && path == "/api/library/music/import" {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) if v.is_object() => v,
            _ => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return true;
            }
        };
        let _busy = BusyGuard::new(&shared.busy);
        let project_root = body
            .get("project")
            .or_else(|| body.get("project_id"))
            .and_then(Value::as_str)
            .and_then(|id| resolve_project_dir(shared, id))
            .or_else(|| shared.active_project_dir());
        match crate::audio_library::import_youtube_music(data_dir, project_root.as_deref(), &body) {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err(e) => respond_err(stream, music_error_status(&e), &e),
        }
        return true;
    }

    if req.method == "POST" && path == "/api/library/music-runner/install" {
        match crate::audio_library::install_yt_music_runner(data_dir) {
            Ok(v) => {
                invalidate_library_cache(shared);
                respond_json(stream, "200 OK", &v);
            }
            Err(e) => respond_err(stream, "500 Internal Server Error", &e),
        }
        return true;
    }

    false
}

fn parse_optional_json_object(raw: &[u8]) -> Option<Value> {
    if raw.iter().all(|b| b.is_ascii_whitespace()) {
        return Some(json!({}));
    }
    serde_json::from_slice::<Value>(raw).ok().filter(Value::is_object)
}

fn music_error_status(e: &str) -> &'static str {
    if e.contains("not found on PATH") {
        "503 Service Unavailable"
    } else if e.contains("timed out") {
        "504 Gateway Timeout"
    } else if e.contains("download failed") || e.contains("search failed") || e.contains("produced no audio") {
        "502 Bad Gateway"
    } else {
        "400 Bad Request"
    }
}

fn handle_monetization_pack(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let _busy = BusyGuard::new(&shared.busy);
    let body: Value = if raw_body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(raw_body) {
            Ok(v) => v,
            Err(_) => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    let tier_preset = body.get("tier_preset").and_then(|v| v.as_str());
    let spec = crate::opencloud::resolve_tier_preset(tier_preset);
    let currency = body
        .get("currency")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(spec.default_currency);
    let vip_pass_id = body.get("vip_pass_id").and_then(|v| v.as_u64()).unwrap_or(0);
    let starter_pack_id = body.get("starter_pack_id").and_then(|v| v.as_u64()).unwrap_or(0);
    let daily_reward = body
        .get("daily_base_reward")
        .and_then(|v| v.as_i64())
        .unwrap_or(spec.default_daily_reward);
    let curve = crate::opencloud::retention_curve_values(&spec, daily_reward);
    let scripts = crate::opencloud::monetization_pack_luau_scripts_with_preset(
        tier_preset,
        currency,
        vip_pass_id,
        starter_pack_id,
        daily_reward,
    );

    let chat_id = shared.active_chat.lock().unwrap().clone();
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        respond_err(stream, "503 Service Unavailable", "Studio not connected");
        return;
    };
    let before = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    let mut written_paths = Vec::new();
    let mut last_report = crate::luau_check::AnalysisReport::default();
    for (place_path, _rel, _cls, src) in &scripts {
        match agent.update_script_source(&chat_id, place_path, src) {
            Ok(updated) => {
                last_report = updated.place_report;
                written_paths.push(updated.path);
            }
            Err(e) => {
                respond_err(stream, "502 Bad Gateway", &e);
                return;
            }
        }
    }
    let after = agent.place_tree().unwrap_or_else(|_| json!({ "instances": {}, "scripts": {} }));
    let diff = compute_diff(&before, &after);
    let can_undo = agent.can_undo_chat(&chat_id);
    let can_redo = agent.can_redo_chat(&chat_id);
    let snap = shared.snapshots.lock().unwrap().record(
        "Monetization & Retention Pack".to_string(),
        "monetization",
        Some(chat_id.clone()),
        after,
        Some(diff.clone()),
    );
    shared.events.publish(
        "monetization",
        chat_id,
        "done",
        "Monetization & Retention Pack Installed",
        diff.summary(),
        Some("monetization_pack".to_string()),
        written_paths.first().cloned(),
        100,
    );
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "tier_preset": spec.id,
            "comeback_multiplier": spec.comeback_multiplier,
            "retention_curve": curve,
            "scripts": written_paths,
            "diff": diff.to_json(),
            "snapshot_id": snap.id,
            "can_undo": can_undo,
            "can_redo": can_redo,
            "audit": last_report.to_json(),
        }),
    );
}

fn handle_studio_economy_simulate(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let body: Value = if raw_body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(raw_body) {
            Ok(v) => v,
            Err(_) => {
                respond_err(stream, "400 Bad Request", "invalid JSON body");
                return;
            }
        }
    };
    let preset = body.get("tier_preset").and_then(|v| v.as_str());
    let tree = shared
        .agent
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|a| a.place_tree().ok())
        .unwrap_or_else(|| json!({ "instances": {}, "scripts": {} }));
    let mut owned: Vec<(String, String)> = Vec::new();
    if let Some(scripts_obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in scripts_obj {
            if let Some(s) = v.as_str() {
                owned.push((k.clone(), s.to_string()));
            }
        }
    }
    let inst_count = tree
        .get("instances")
        .and_then(Value::as_object)
        .map(|m| m.len())
        .unwrap_or(0);
    let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let report = crate::opencloud::simulate_economy_funnel(preset, &refs, inst_count, &[]);
    respond_json(stream, "200 OK", &report);
}

fn handle_mesh_asset_file(stream: &mut TcpStream, shared: &Shared, raw_file: &str) {
    match crate::mesh::read_mesh_asset(&shared.assets_root, raw_file) {
        Ok((bytes, mime)) => {
            respond_bytes(stream, "200 OK", mime, "public, max-age=3600", &bytes);
        }
        Err(e) => {
            let status = if e.contains("escapes") || e.contains("symlink") {
                "403 Forbidden"
            } else if e.contains("not found") {
                "404 Not Found"
            } else {
                "400 Bad Request"
            };
            respond_err(stream, status, &e);
        }
    }
}

fn handle_audio_asset_file(stream: &mut TcpStream, shared: &Shared, raw_file: &str) {
    match crate::audio_synth::read_audio_asset(&shared.assets_root, raw_file) {
        Ok(bytes) => {
            respond_bytes(stream, "200 OK", "audio/wav", "public, max-age=3600", &bytes);
        }
        Err(e) => {
            let status = if e.contains("escapes") || e.contains("symlink") {
                "403 Forbidden"
            } else if e.contains("not found") {
                "404 Not Found"
            } else {
                "400 Bad Request"
            };
            respond_err(stream, status, &e);
        }
    }
}

fn all_stored_assets(shared: &Shared) -> Vec<crate::assets::AssetMetadata> {
    if let Some(dir) = &shared.data_dir {
        crate::assets::AssetManager::open(dir).list()
    } else {
        shared.game_assets.lock().unwrap().mem_assets.clone()
    }
}

fn all_ship_pack_assets(shared: &Shared) -> Vec<crate::rojo_export::ShipPackAsset> {
    if let Some(dir) = &shared.data_dir {
        let mgr = crate::assets::AssetManager::open(dir);
        let mut out = Vec::new();
        for meta in mgr.list() {
            if let Ok((bytes, _)) = mgr.read_bytes(&meta.filename) {
                out.push(crate::rojo_export::ShipPackAsset {
                    metadata: meta,
                    bytes,
                });
            }
        }
        out
    } else {
        let idx = shared.game_assets.lock().unwrap();
        let mut out = Vec::new();
        for meta in &idx.mem_assets {
            if let Some(bytes) = idx.mem_blobs.get(&meta.filename) {
                out.push(crate::rojo_export::ShipPackAsset {
                    metadata: meta.clone(),
                    bytes: bytes.clone(),
                });
            }
        }
        out
    }
}

fn bind_stored_assets_to_agent(shared: &Shared) {
    let assets = all_stored_assets(shared);
    if assets.is_empty() {
        return;
    }
    if let Ok(mut guard) = shared.agent.lock() {
        if let Some(agent) = guard.as_mut() {
            agent.bind_assets(&assets);
        }
    }
}

struct MediaCollector {
    read_bytes: bool,
    seen: std::collections::HashSet<String>,
    rows: Vec<Value>,
    files: Vec<(String, Vec<u8>)>,
}

impl MediaCollector {
    fn scan(&mut self, dir: &std::path::Path, kind: &str, sub_prefix: &str, url_prefix: &str) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                std::fs::symlink_metadata(p)
                    .map(|m| !m.file_type().is_symlink() && m.file_type().is_file())
                    .unwrap_or(false)
            })
            .collect();
        entries.sort();
        for path in entries {
            let Some(fname) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if fname.starts_with('.') {
                continue;
            }
            let zip_rel = format!("assets/{sub_prefix}/{fname}");
            if !self.seen.insert(zip_rel.clone()) {
                continue;
            }
            let bytes_len = path.metadata().map(|m| m.len()).unwrap_or(0);
            if self.read_bytes {
                if let Ok(bytes) = std::fs::read(&path) {
                    self.files.push((zip_rel.clone(), bytes));
                }
            }
            self.rows.push(json!({
                "filename": format!("{sub_prefix}/{fname}"),
                "name": fname,
                "kind": kind,
                "bytes": bytes_len,
                "url": format!("{url_prefix}/{fname}"),
                "content_uri": "rbxassetid://0",
                "bundle_path": zip_rel,
            }));
        }
    }
}

fn collect_generated_media(shared: &Shared, read_bytes: bool) -> (Vec<(String, Vec<u8>)>, Vec<Value>) {
    let mut c = MediaCollector {
        read_bytes,
        seen: std::collections::HashSet::new(),
        rows: Vec::new(),
        files: Vec::new(),
    };
    c.scan(&shared.assets_root.join("game-assets/meshes"), "mesh", "meshes", "/api/assets/meshes");
    c.scan(&shared.assets_root.join("game-assets/audio"), "audio", "audio", "/api/assets/audio");
    if let Some(proj) = shared.active_project_dir() {
        c.scan(&proj.join("assets/meshes"), "mesh", "meshes", "/api/assets/meshes");
        c.scan(&proj.join("assets/audio"), "audio", "audio", "/api/assets/audio");
    }
    (c.files, c.rows)
}

fn collect_generated_media_for_bundle(shared: &Shared) -> (Vec<(String, Vec<u8>)>, Vec<Value>) {
    collect_generated_media(shared, true)
}

fn handle_assets_list(stream: &mut TcpStream, shared: &Shared) {
    let assets = all_stored_assets(shared);
    let mut items: Vec<Value> = assets.iter().map(crate::assets::AssetMetadata::to_json).collect();
    let (_, media_rows) = collect_generated_media(shared, false);
    items.extend(media_rows);
    let dir_str = shared
        .data_dir
        .as_ref()
        .map(|d| d.join("game-assets").display().to_string());
    respond_json(
        stream,
        "200 OK",
        &json!({
            "ok": true,
            "count": items.len(),
            "assets": items,
            "assets_dir": dir_str,
        }),
    );
}

fn handle_assets_manifest(stream: &mut TcpStream, shared: &Shared) {
    let assets = all_stored_assets(shared);
    let (_, media_rows) = collect_generated_media(shared, false);
    let mut manifest = crate::assets::build_asset_manifest_json(&shared.game_name, &assets);
    if !media_rows.is_empty() {
        if let Some(arr) = manifest.get_mut("assets").and_then(|v| v.as_array_mut()) {
            arr.extend(media_rows);
            let total = arr.len();
            if let Some(obj) = manifest.as_object_mut() {
                obj.insert("asset_count".to_string(), json!(total));
            }
        }
    }
    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("ok".to_string(), json!(true));
    }
    respond_json(stream, "200 OK", &manifest);
}

fn handle_asset_file(stream: &mut TcpStream, shared: &Shared, raw_file: &str) {
    let safe_name = match crate::assets::sanitize_filename(raw_file) {
        Ok(s) => s,
        Err(e) => {
            respond_err(stream, "400 Bad Request", &e);
            return;
        }
    };
    if let Some(dir) = &shared.data_dir {
        let mgr = crate::assets::AssetManager::open(dir);
        match mgr.read_bytes(&safe_name) {
            Ok((bytes, mime)) => {
                respond_bytes(stream, "200 OK", mime, "public, max-age=3600", &bytes);
                return;
            }
            Err(e) => {
                let status = if e.contains("escapes") || e.contains("symlink") {
                    "403 Forbidden"
                } else {
                    "404 Not Found"
                };
                respond_err(stream, status, &e);
                return;
            }
        }
    }
    let idx = shared.game_assets.lock().unwrap();
    if let Some(bytes) = idx.mem_blobs.get(&safe_name) {
        respond_bytes(
            stream,
            "200 OK",
            crate::assets::mime_for_filename(&safe_name),
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

fn handle_asset_delete(stream: &mut TcpStream, shared: &Shared, raw_file: &str) {
    let safe_name = match crate::assets::sanitize_filename(raw_file) {
        Ok(s) => s,
        Err(e) => {
            respond_err(stream, "400 Bad Request", &e);
            return;
        }
    };
    let deleted = if let Some(dir) = &shared.data_dir {
        match crate::assets::AssetManager::open(dir).delete(&safe_name) {
            Ok(d) => d,
            Err(e) => {
                let status = if e.contains("symlink") || e.contains("escape") {
                    "403 Forbidden"
                } else {
                    "500 Internal Server Error"
                };
                respond_err(stream, status, &e);
                return;
            }
        }
    } else {
        let mut idx = shared.game_assets.lock().unwrap();
        let before = idx.mem_assets.len();
        idx.mem_assets.retain(|a| a.filename != safe_name);
        idx.mem_blobs.remove(&safe_name).is_some() || idx.mem_assets.len() != before
    };
    respond_json(
        stream,
        if deleted { "200 OK" } else { "404 Not Found" },
        &json!({ "ok": deleted, "filename": safe_name }),
    );
}

fn save_asset_and_bind(
    shared: &Shared,
    raw_filename: &str,
    bytes: &[u8],
    input: crate::assets::AssetMetadataInput,
) -> Result<crate::assets::AssetMetadata, String> {
    let meta = if let Some(dir) = &shared.data_dir {
        crate::assets::AssetManager::open(dir).save_bytes(raw_filename, bytes, input)?
    } else {
        let filename = crate::assets::sanitize_filename(raw_filename)?;
        let (mime_type, _) = crate::assets::detect_image_mime(bytes)?;
        let stem = filename
            .rsplit_once('.')
            .map(|(a, _)| a)
            .unwrap_or(&filename);
        let card_id = input
            .card_id
            .as_deref()
            .map(crate::assets::sanitize_card_id)
            .unwrap_or_else(|| crate::assets::sanitize_card_id(stem));
        let guide = crate::art_knowledge::anime_archetypes()
            .iter()
            .find(|a| a.card_id == card_id);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let meta = crate::assets::AssetMetadata {
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
            passive: input
                .passive
                .unwrap_or_else(|| guide.map(|g| g.passive).unwrap_or("").to_string()),
            ultimate: input
                .ultimate
                .unwrap_or_else(|| guide.map(|g| g.ultimate).unwrap_or("").to_string()),
            prompt: input
                .prompt
                .unwrap_or_else(|| guide.map(|g| g.art_prompt).unwrap_or("").to_string()),
            source: input.source.unwrap_or_else(|| "upload".to_string()),
            filename: filename.clone(),
            mime_type: mime_type.to_string(),
            bytes: bytes.len(),
            sha256: String::new(),
            created_at_ms: now_ms,
            roblox_asset_id: input.roblox_asset_id,
        };
        let mut idx = shared.game_assets.lock().unwrap();
        idx.mem_assets.retain(|a| a.filename != filename);
        idx.mem_assets.push(meta.clone());
        idx.mem_blobs.insert(filename, bytes.to_vec());
        meta
    };

    bind_stored_assets_to_agent(shared);

    let chat_id = shared.active_chat.lock().unwrap().clone();
    shared.events.publish(
        "asset",
        chat_id,
        "done",
        "Card Art Uploaded",
        format!("Stored {} ({} bytes)", meta.filename, meta.bytes),
        None,
        Some(format!("/api/assets/{}", meta.filename)),
        100,
    );
    Ok(meta)
}

fn handle_assets_upload(stream: &mut TcpStream, shared: &Shared, req: &Request, query: &str) {
    let is_raw_image = req.content_type.starts_with("image/png")
        || req.content_type.starts_with("image/jpeg")
        || req.content_type.starts_with("image/webp");
    let (raw_name, bytes, input) = if is_raw_image {
        let name = query_param(query, "filename").unwrap_or("card-art.png").to_string();
        let meta = crate::assets::AssetMetadataInput {
            source: Some("upload".to_string()),
            ..Default::default()
        };
        (name, req.body.clone(), meta)
    } else {
        let body: Value = match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) => v,
            Err(_) => {
                respond_err(stream, "400 Bad Request", "invalid JSON body for asset upload");
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
        let decoded = match crate::assets::decode_base64(b64) {
            Ok(d) => d,
            Err(e) => {
                respond_err(stream, "400 Bad Request", &e);
                return;
            }
        };
        (raw_name, decoded, crate::assets::AssetMetadataInput::from_json(&body))
    };

    match save_asset_and_bind(shared, &raw_name, &bytes, input) {
        Ok(meta) => respond_json(
            stream,
            "201 Created",
            &json!({
                "ok": true,
                "file": meta.filename,
                "url": format!("/api/assets/{}", meta.filename),
                "asset": meta.to_json(),
            }),
        ),
        Err(e) => respond_err(stream, "400 Bad Request", &e),
    }
}

fn handle_assets_generate(stream: &mut TcpStream, shared: &Shared, raw_body: &[u8]) {
    let body: Value = match serde_json::from_slice(raw_body) {
        Ok(v) => v,
        Err(_) => {
            respond_err(stream, "400 Bad Request", "invalid JSON body");
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
        .unwrap_or("Flame")
        .to_string();
    let rarity = body
        .get("rarity")
        .and_then(|v| v.as_str())
        .unwrap_or("Mythic")
        .to_string();
    let prompt = body
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| crate::art_knowledge::build_homage_prompt(&title, &element, &rarity, ""));

    let cfg = shared.config.lock().unwrap().clone();
    let store = shared
        .data_dir
        .as_deref()
        .map(crate::secrets::SecretStore::for_data_dir);
    let requested_provider = body
        .get("provider")
        .and_then(|v| v.as_str())
        .or(cfg.image_provider.as_deref())
        .unwrap_or("auto");
    let gemini_key = store
        .as_ref()
        .and_then(|s| s.get(crate::secrets::GEMINI_KEY))
        .or(cfg.gemini_api_key)
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
    let openai_key = store
        .as_ref()
        .and_then(|s| s.get(crate::secrets::OPENAI_IMAGE_KEY))
        .or(cfg.openai_image_api_key)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()));

    let use_gemini = matches!(requested_provider, "gemini" | "nano-banana" | "auto") && gemini_key.is_some();
    let use_openai = !use_gemini && matches!(requested_provider, "openai" | "openai-image" | "auto") && openai_key.is_some();

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

    let raw_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let normalized_model = raw_model.and_then(|m| {
        if m.starts_with("gemini-nano-banana") {
            None
        } else {
            Some(m.to_string())
        }
    });

    let req = crate::assets::GenerateImageRequest {
        provider: if use_gemini { "gemini" } else { "openai" }.to_string(),
        card_id: body
            .get("card_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&title)
            .to_string(),
        title,
        rarity,
        element,
        atk: body.get("atk").and_then(|v| v.as_i64()).unwrap_or(420),
        hp: body.get("hp").and_then(|v| v.as_i64()).unwrap_or(1480),
        speed: body.get("speed").and_then(|v| v.as_i64()).unwrap_or(92),
        passive: body
            .get("passive")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        ultimate: body
            .get("ultimate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        prompt: prompt.clone(),
        model: normalized_model,
        api_key: if use_gemini { gemini_key } else { openai_key },
        base_url: None,
    };

    let Some(dir) = &shared.data_dir else {
        respond_json(
            stream,
            "503 Service Unavailable",
            &json!({
                "ok": false,
                "error": "image generation requires a persistent OpenRO data directory",
                "homage_prompt": prompt,
            }),
        );
        return;
    };

    let mgr = crate::assets::AssetManager::open(dir);
    match mgr.generate_image(&req) {
        Ok(meta) => {
            bind_stored_assets_to_agent(shared);
            let chat_id = shared.active_chat.lock().unwrap().clone();
            shared.events.publish(
                "asset",
                chat_id,
                "done",
                "Card Art Generated",
                format!("Generated {} ({} bytes)", meta.filename, meta.bytes),
                None,
                Some(format!("/api/assets/{}", meta.filename)),
                100,
            );
            respond_json(
                stream,
                "201 Created",
                &json!({
                    "ok": true,
                    "file": meta.filename,
                    "url": format!("/api/assets/{}", meta.filename),
                    "asset": meta.to_json(),
                }),
            );
        }
        Err(e) => respond_json(stream, "502 Bad Gateway", &json!({ "ok": false, "error": e, "homage_prompt": prompt })),
    }
}

/// The single-page UI (pre-gzipped assets/index.html.gz); values are substituted in render_page.
const INDEX_HTML_GZ: &[u8] = include_bytes!("../assets/index.html.gz");

fn index_html() -> String {
    use flate2::read::GzDecoder;
    let mut decoder = GzDecoder::new(INDEX_HTML_GZ);
    let mut html = String::with_capacity(INDEX_HTML_GZ.len() * 3);
    let _ = decoder.read_to_string(&mut html);
    html
}

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
    let can_build = (model_configured && studio_connected).to_string();
    let studio_str = studio_connected.to_string();
    let game_escaped = esc_html(game_name);
    let template = index_html();
    let mut out = String::with_capacity(template.len() + banners.len() + 64);
    let mut rest = template.as_str();
    while let Some(pos) = rest.find("__") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let matched = [
            ("__KEY_BANNER__", banners.as_str()),
            ("__CHIP_CLASS__", chip_class),
            ("__CHIP__", chip),
            ("__CAN_BUILD__", can_build.as_str()),
            ("__STUDIO_CONNECTED__", studio_str.as_str()),
            ("__WORKSPACE_MODE__", workspace_mode),
            ("__GAME_NAME__", game_escaped.as_str()),
        ]
        .into_iter()
        .find(|(tok, _)| after.starts_with(tok));
        if let Some((tok, val)) = matched {
            out.push_str(val);
            rest = &after[tok.len()..];
        } else {
            out.push_str("__");
            rest = &after[2..];
        }
    }
    out.push_str(rest);
    out
}

pub fn prewarm_index_cache(shared: &Shared) {
    let info = status_info(shared);
    let _ = cached_index(
        &shared.game_name,
        info.studio_connected,
        info.model_configured,
        &shared.store_recovered,
        info.workspace_mode,
    );
}

#[derive(Clone)]
struct CachedIndex {
    gzip: Vec<u8>,
    etag: String,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct IndexCacheKey {
    game_name: String,
    studio_connected: bool,
    model_configured: bool,
    recovered: Vec<String>,
    workspace_mode: &'static str,
}

fn cached_index(
    game_name: &str,
    studio_connected: bool,
    model_configured: bool,
    recovered: &[String],
    workspace_mode: &'static str,
) -> Arc<CachedIndex> {
    use flate2::{write::GzEncoder, Compression};
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;

    static CACHE: OnceLock<Mutex<HashMap<IndexCacheKey, Arc<CachedIndex>>>> = OnceLock::new();
    static BUILD_GATE: OnceLock<Mutex<()>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = IndexCacheKey {
        game_name: game_name.to_string(),
        studio_connected,
        model_configured,
        recovered: recovered.to_vec(),
        workspace_mode,
    };
    if let Some(hit) = cache.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let _build_guard = BUILD_GATE.get_or_init(|| Mutex::new(())).lock().unwrap();
    if let Some(hit) = cache.lock().unwrap().get(&key).cloned() {
        return hit;
    }

    let plain = render_page_full(
        game_name,
        studio_connected,
        model_configured,
        recovered,
        workspace_mode,
    )
    .into_bytes();

    let mut hasher = Sha256::new();
    hasher.update(&plain);
    let digest = hasher.finalize();
    let etag = format!(
        "\"{:016x}\"",
        u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
    );

    let mut enc = GzEncoder::new(Vec::with_capacity(plain.len() / 3), Compression::new(5));
    let gzip = if enc.write_all(&plain).is_ok() {
        enc.finish().unwrap_or_default()
    } else {
        Vec::new()
    };

    let entry = Arc::new(CachedIndex { gzip, etag });
    let mut map = cache.lock().unwrap();
    if map.len() >= 16 {
        map.clear();
    }
    map.insert(key, Arc::clone(&entry));
    entry
}

fn accepts_gzip(header: &str) -> bool {
    header.split(',').any(|part| {
        let mut pieces = part.trim().split(';');
        let enc = pieces.next().unwrap_or("").trim();
        if !enc.eq_ignore_ascii_case("gzip") && !enc.eq_ignore_ascii_case("x-gzip") {
            return false;
        }
        for param in pieces {
            if let Some((k, v)) = param.trim().split_once('=') {
                if k.trim().eq_ignore_ascii_case("q") {
                    if let Ok(q) = v.trim().parse::<f32>() {
                        return q > 0.0;
                    }
                }
            }
        }
        true
    })
}

fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let trimmed = if_none_match.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "*" {
        return true;
    }
    trimmed.split(',').any(|cand| {
        let c = cand.trim();
        c == etag || c.strip_prefix("W/") == Some(etag)
    })
}

fn serve_index_page(
    stream: &mut TcpStream,
    req: &Request,
    game_name: &str,
    studio_connected: bool,
    model_configured: bool,
    recovered: &[String],
    workspace_mode: &'static str,
) {
    let cached = cached_index(
        game_name,
        studio_connected,
        model_configured,
        recovered,
        workspace_mode,
    );
    if etag_matches(&req.if_none_match, &cached.etag) {
        let conn = conn_header_line("304 Not Modified");
        let resp = format!(
            "HTTP/1.1 304 Not Modified\r\nETag: {}\r\nCache-Control: no-cache\r\nVary: Accept-Encoding\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\n{conn}",
            cached.etag
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }
    if !cached.gzip.is_empty() && accepts_gzip(&req.accept_encoding) {
        let extra = format!(
            "ETag: {}\r\nVary: Accept-Encoding\r\nContent-Encoding: gzip\r\n",
            cached.etag
        );
        respond_bytes_with_headers(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            "no-cache",
            &extra,
            &cached.gzip,
        );
    } else {
        let extra = format!("ETag: {}\r\nVary: Accept-Encoding\r\n", cached.etag);
        let plain = render_page_full(
            game_name,
            studio_connected,
            model_configured,
            recovered,
            workspace_mode,
        )
        .into_bytes();
        respond_bytes_with_headers(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            "no-cache",
            &extra,
            &plain,
        );
    }
}

fn serve_chunk_asset(stream: &mut TcpStream, req: &Request, name: &str) -> bool {
    use flate2::{read::GzDecoder, write::GzEncoder, Compression};
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;

    let Some((blob, is_gz)) = ui_chunks::get_gz(name) else {
        return false;
    };
    static ETAG_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let etag_map = ETAG_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let etag = {
        let guard = etag_map.lock().unwrap();
        guard.get(name).cloned()
    }
    .unwrap_or_else(|| {
        let mut hasher = Sha256::new();
        hasher.update(blob);
        let digest = hasher.finalize();
        let tag = format!(
            "\"{:016x}\"",
            u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
        );
        let mut map = etag_map.lock().unwrap();
        if map.len() < 128 {
            map.insert(name.to_string(), tag.clone());
        }
        tag
    });

    if etag_matches(&req.if_none_match, &etag) {
        let conn = conn_header_line("304 Not Modified");
        let resp = format!(
            "HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nCache-Control: public, max-age=31536000, immutable\r\nVary: Accept-Encoding\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\n{conn}"
        );
        let _ = stream.write_all(resp.as_bytes());
        return true;
    }

    let content_type = if name.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if name.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/javascript; charset=utf-8"
    };
    if name.ends_with(".woff2") {
        let extra_plain = format!("ETag: {etag}\r\nVary: Accept-Encoding\r\n");
        respond_bytes_with_headers(
            stream,
            "200 OK",
            content_type,
            "public, max-age=31536000, immutable",
            &extra_plain,
            blob,
        );
        return true;
    }
    if accepts_gzip(&req.accept_encoding) {
        let extra = format!("ETag: {etag}\r\nVary: Accept-Encoding\r\nContent-Encoding: gzip\r\n");
        if is_gz {
            respond_bytes_with_headers(
                stream,
                "200 OK",
                content_type,
                "public, max-age=31536000, immutable",
                &extra,
                blob,
            );
        } else {
            let mut enc = GzEncoder::new(Vec::with_capacity(blob.len() / 3), Compression::default());
            if enc.write_all(blob).is_ok() {
                if let Ok(gz) = enc.finish() {
                    respond_bytes_with_headers(
                        stream,
                        "200 OK",
                        content_type,
                        "public, max-age=31536000, immutable",
                        &extra,
                        &gz,
                    );
                    return true;
                }
            }
            let extra_plain = format!("ETag: {etag}\r\nVary: Accept-Encoding\r\n");
            respond_bytes_with_headers(
                stream,
                "200 OK",
                content_type,
                "public, max-age=31536000, immutable",
                &extra_plain,
                blob,
            );
        }
    } else if is_gz {
        let mut dec = GzDecoder::new(blob);
        let mut plain = Vec::new();
        let _ = dec.read_to_end(&mut plain);
        let extra = format!("ETag: {etag}\r\nVary: Accept-Encoding\r\n");
        respond_bytes_with_headers(
            stream,
            "200 OK",
            content_type,
            "public, max-age=31536000, immutable",
            &extra,
            &plain,
        );
    } else {
        let extra = format!("ETag: {etag}\r\nVary: Accept-Encoding\r\n");
        respond_bytes_with_headers(
            stream,
            "200 OK",
            content_type,
            "public, max-age=31536000, immutable",
            &extra,
            blob,
        );
    }
    true
}

/* hallmark-rune override log — OpenRO elevation (18 Nov 2026)
   override · R9 · guard: the chat list is the primary navigation between a game's build sessions and is used constantly; the phone face collapses the rail to a top bar with a horizontal chat row in this same pass · reason: in OpenRO the rail is the session list, not chrome — the BAR's DIRECTION v1 fixes this shell and familiarity outranks novelty
   override · R1 (borderline) · guard: thread step lines render the build ledger — data returned by the API (skills run, scripts written, verdicts) · reason: mono carries the ledger because the ledger is data; UI labels, pitches and prose stay in the body face
*/
