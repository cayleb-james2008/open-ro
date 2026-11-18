//! Hand-rolled minimal HTTP over `std::net::TcpListener` serving the
//! embedded web UI (plain HTML/CSS/JS string constants — no framework,
//! no build step) plus the JSON API the UI and tests use.
//!
//! Chats are multiple and persistent: when the server is built with a
//! data dir (`Shared::with_data_dir`), every chat and its thread lines
//! are saved to `chats.json` there on every change and reloaded at
//! start, so a restart loses nothing.

use crate::agent::Agent;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
}

impl Shared {
    /// In-memory server (tests, ephemeral runs): chats are not saved.
    pub fn new(agent: Option<Agent>, game_name: String) -> Arc<Shared> {
        let chat = default_chat();
        let active = chat.id.clone();
        Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(vec![chat]),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: None,
            ideas: Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Persistent server: chats load from (and save to) `chats.json`
    /// inside `data_dir`.
    pub fn with_data_dir(agent: Option<Agent>, game_name: String, data_dir: PathBuf) -> Arc<Shared> {
        let _ = std::fs::create_dir_all(&data_dir);
        let (chats, active) = load_chats(&data_dir).unwrap_or_else(|| {
            let chat = default_chat();
            let active = chat.id.clone();
            (vec![chat], active)
        });
        let ideas = load_ideas(&data_dir);
        Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(chats),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: Some(data_dir),
            ideas: Mutex::new(ideas),
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
fn load_ideas(dir: &PathBuf) -> std::collections::HashMap<String, ChatIdeas> {
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
            let _ = backup_corrupt(&path);
            std::collections::HashMap::new()
        }
    }
}

fn ideas_json(ideas: &[crate::ideas::Idea]) -> Value {
    Value::Array(
        ideas
            .iter()
            .map(|i| json!({ "id": i.id, "title": i.title, "pitch": i.pitch }))
            .collect(),
    )
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

fn load_chats(dir: &PathBuf) -> Option<(Vec<Chat>, String)> {
    let path = dir.join("chats.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed = serde_json::from_str::<Value>(&text);
    let shape_ok = parsed
        .as_ref()
        .map(|v| v.get("chats").map(|c| c.is_array()).unwrap_or(false))
        .unwrap_or(false);
    if !shape_ok {
        // Corrupt (unparseable or wrong shape): preserve the bytes.
        let _ = backup_corrupt(&path);
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
            let _ = backup_corrupt(&path);
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

pub fn serve(listener: TcpListener, shared: Arc<Shared>) -> std::io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || {
                    let _ = handle(stream, &shared);
                });
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
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
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    // Bodies are small JSON; a request claiming a huge one is dropped
    // rather than waited on forever.
    if content_length > 1_048_576 {
        return None;
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Some(Request { method, path, body })
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

fn respond_json(stream: &mut TcpStream, status: &str, value: &Value) {
    respond(stream, status, "application/json", &value.to_string());
}

fn handle(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    let req = match read_request(&mut stream) {
        Some(r) => r,
        None => return Ok(()),
    };
    let path = req.path.split('?').next().unwrap_or("/").to_string();

    match (req.method.as_str(), path.as_str()) {
        ("GET", "/") => {
            let (studio_connected, model_configured) = status_flags(shared);
            let html = render_page(&shared.game_name, studio_connected, model_configured);
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", &html);
        }
        ("GET", "/api/status") => {
            let (studio_connected, model_configured) = status_flags(shared);
            respond_json(
                &mut stream,
                "200 OK",
                &json!({
                    "studio_connected": studio_connected,
                    "model_configured": model_configured,
                    "game_name": shared.game_name,
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
            respond_json(&mut stream, "200 OK", &shared.chats_json());
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
                Some(agent) => match agent.run_ask(&chat_id, &message) {
                    Ok(outcome) => {
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        // The agent speaks next, unprompted: record
                        // what the game now has and pitch the follow-up
                        // set for the new context (once per context).
                        shared.note_built(&chat_id, &outcome.skills_used);
                        let (_, pitch) = shared.pitch_for(
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
                },
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
                    "pitched_new": !lines.is_empty(),
                }),
            );
        }
        ("POST", "/api/ideas/build") => {
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
                let error = format!(
                    "“{}” needs {} first — I won't build it until that's in the game, or it would arrive dead.",
                    idea.title,
                    missing.join(", ")
                );
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
                Some(agent) => match agent.build_idea(&chat_id, &id) {
                    Ok(outcome) => {
                        shared.append_lines_to(&chat_id, &outcome.thread);
                        shared.note_built(&chat_id, &outcome.skills_used);
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
                },
            }
        }
        ("POST", "/api/revert") => {
            let chat_id = shared.active_chat.lock().unwrap().clone();
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.revert_chat(&chat_id) {
                    Ok(restored) => {
                        shared.append_lines_to(&chat_id, &[format!(
                            "OpenRO: Reverted {} change(s) in this chat.",
                            restored.len()
                        )]);
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({ "ok": true, "restored": restored }),
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
                },
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

fn status_flags(shared: &Shared) -> (bool, bool) {
    let mut guard = shared.agent.lock().unwrap();
    match guard.as_mut() {
        Some(agent) => (agent.studio_connected(), agent.model.is_configured()),
        None => (false, false),
    }
}

/// The single-page UI (assets/index.html); values are substituted in render_page.
const INDEX_HTML: &str = include_str!("../assets/index.html");

/// The single-page shell: left sidebar (the game's chats + New chat +
/// the game's name + settings), one scrolling thread, one composer row
/// pinned at the bottom, and ONE status chip reflecting the real bridge
/// state. Light + dark themes; dark is the default.
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
    let chip = if studio_connected {
        "Studio connected"
    } else {
        "Studio not connected"
    };
    let chip_class = if studio_connected { "chip on" } else { "chip off" };
    // Both blockers get a banner of the same weight, each naming its
    // fix — the no-Studio state used to be a lone 12px chip.
    let mut banners = String::new();
    if !model_configured {
        banners.push_str("<div class=\"banner\" id=\"no-key-banner\">no model key configured — set OPENRO_MODEL_API_KEY to enable chat</div>");
    }
    if !studio_connected {
        banners.push_str("<div class=\"banner\" id=\"no-studio-banner\">Studio not connected — set OPENRO_MCP_COMMAND to a Studio MCP server; OpenRO cannot build until one is connected</div>");
    }
    let can_build = model_configured && studio_connected;
    // The game name is the only operator-supplied value: it is escaped,
    // and substituted last so no other token can ever match inside it.
    INDEX_HTML
        .replace("__KEY_BANNER__", &banners)
        .replace("__CHIP_CLASS__", chip_class)
        .replace("__CHIP__", chip)
        .replace("__CAN_BUILD__", &can_build.to_string())
        .replace("__STUDIO_CONNECTED__", &studio_connected.to_string())
        .replace("__GAME_NAME__", &esc_html(game_name))
}

/* hallmark-rune override log — OpenRO elevation (18 Nov 2026)
   override · R9 · guard: the chat list is the primary navigation between a game's build sessions and is used constantly; the phone face collapses the rail to a top bar with a horizontal chat row in this same pass · reason: in OpenRO the rail is the session list, not chrome — the BAR's DIRECTION v1 fixes this shell and familiarity outranks novelty
   override · R1 (borderline) · guard: thread step lines render the build ledger — data returned by the API (skills run, scripts written, verdicts) · reason: mono carries the ledger because the ledger is data; UI labels, pitches and prose stay in the body face
*/
