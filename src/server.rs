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

pub struct Shared {
    pub agent: Mutex<Option<Agent>>,
    pub chats: Mutex<Vec<Chat>>,
    pub active_chat: Mutex<String>,
    pub game_name: String,
    pub data_dir: Option<PathBuf>,
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
        Arc::new(Shared {
            agent: Mutex::new(agent),
            chats: Mutex::new(chats),
            active_chat: Mutex::new(active),
            game_name,
            data_dir: Some(data_dir),
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
    let mut chats = Vec::new();
    for item in v["chats"].as_array()? {
        let id = item["id"].as_str()?.to_string();
        let title = item["title"].as_str().unwrap_or("New chat").to_string();
        let lines: Vec<String> = item["lines"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        chats.push(Chat { id, title, lines });
    }
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
                        // stays revertable in this chat.
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
    let game_name = esc_html(game_name);
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
    let key_banner = banners;
    let can_build = model_configured && studio_connected;
    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>OpenRO — {game_name}</title>
<script>
  try {{
    var t = localStorage.getItem('openro-theme');
    if (t === 'light' || t === 'dark') document.documentElement.setAttribute('data-theme', t);
  }} catch (e) {{ /* dark stays the default */ }}
</script>
<style>
  :root {{
    color-scheme: dark;
    --bg: #101014; --panel: #1b1b22; --sidebar: #17171d; --ink: #eeeeee;
    --muted: #9a9aad; --faint: #8e8ea5; --rule: #2a2a33;
    --accent: #2f6fed; --accent-hover: #2a62d4;
    --user-bubble: #23232e; --field: #1b1b22;
    --ok-bg: #173d24; --ok-ink: #7ee2a0; --bad-bg: #3d1717; --bad-ink: #f09393;
    --warn-bg: #3a2f10; --warn-ink: #f2d38a;
  }}
  [data-theme="light"] {{
    color-scheme: light;
    --bg: #f6f6f4; --panel: #ffffff; --sidebar: #ececea; --ink: #1d1d22;
    --muted: #5d5d6b; --faint: #62627a; --rule: #d9d9d4;
    --accent: #2459c4; --accent-hover: #1e4dab;
    --user-bubble: #e6e6e1; --field: #ffffff;
    --ok-bg: #e2f2e6; --ok-ink: #1c6b34; --bad-bg: #fbe7e7; --bad-ink: #9c2222;
    --warn-bg: #fdf3e3; --warn-ink: #7a4b12;
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; font-family: system-ui, sans-serif; background: var(--bg); color: var(--ink); display: flex; height: 100vh; font-size: 15px; }}
  ::selection {{ background: var(--accent); color: #fff; }}
  :focus-visible {{ outline: 2px solid var(--accent); outline-offset: 2px; }}
  button {{ font: inherit; }}
  .sr-only {{ position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }}
  #sidebar {{ width: 264px; flex: 0 0 264px; background: var(--sidebar); border-right: 1px solid var(--rule); display: flex; flex-direction: column; padding: 16px; gap: 12px; min-height: 0; }}
  #sidebar h1 {{ font-size: 18px; margin: 0; font-weight: 600; }}
  #game-name {{ color: var(--muted); font-size: 13px; }}
  #new-chat {{ background: var(--accent); color: #fff; border: 0; border-radius: 8px; padding: 10px; cursor: pointer; font-weight: 600; }}
  #new-chat:hover {{ background: var(--accent-hover); }}
  #chat-list {{ list-style: none; margin: 0; padding: 0; font-size: 14px; overflow-y: auto; flex: 1; min-height: 48px; display: flex; flex-direction: column; gap: 4px; }}
  #chat-list li {{ margin: 0; }}
  .chat-btn {{ display: block; width: 100%; text-align: left; background: none; border: 0; padding: 8px 10px; border-radius: 6px; color: var(--muted); cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 14px; }}
  .chat-btn:hover {{ color: var(--ink); }}
  .chat-btn.active {{ background: var(--panel); color: var(--ink); }}
  .chip {{ font-size: 12px; padding: 6px 10px; border-radius: 999px; text-align: center; }}
  .chip.on {{ background: var(--ok-bg); color: var(--ok-ink); }}
  .chip.off {{ background: var(--bad-bg); color: var(--bad-ink); }}
  #settings {{ border-top: 1px solid var(--rule); padding-top: 12px; display: flex; flex-direction: column; gap: 8px; }}
  #settings h2 {{ font-size: 12px; font-weight: 600; color: var(--faint); margin: 0; text-transform: uppercase; letter-spacing: 0.08em; }}
  .set-row {{ display: flex; align-items: center; gap: 8px; }}
  .set-row .set-name {{ font-size: 13px; color: var(--muted); flex: 1; }}
  .theme-btn {{ background: none; border: 1px solid var(--rule); color: var(--muted); border-radius: 999px; padding: 4px 12px; font-size: 13px; cursor: pointer; }}
  .theme-btn[aria-pressed="true"] {{ background: var(--ink); color: var(--bg); border-color: var(--ink); }}
  main {{ flex: 1; display: flex; flex-direction: column; min-width: 0; }}
  .banner {{ background: var(--warn-bg); color: var(--warn-ink); padding: 10px 16px; font-size: 13px; }}
  #thread {{ flex: 1; overflow-y: auto; padding: 20px; display: flex; flex-direction: column; gap: 10px; }}
  .msg {{ max-width: 75ch; line-height: 1.5; white-space: pre-wrap; word-wrap: break-word; border-radius: 10px; padding: 10px 14px; }}
  .msg.user {{ background: var(--user-bubble); align-self: flex-end; }}
  .msg.assistant {{ background: var(--panel); border: 1px solid var(--rule); align-self: flex-start; }}
  .msg.step {{ background: transparent; color: var(--muted); font-size: 13px; align-self: flex-start; padding: 2px 14px; }}
  .msg.step.verdict-ok {{ color: var(--ok-ink); font-weight: 600; }}
  .msg.step.verdict-bad {{ color: var(--bad-ink); font-weight: 600; }}
  .empty {{ color: var(--faint); text-align: center; margin-top: 12vh; line-height: 1.6; }}
  #composer {{ display: flex; gap: 8px; padding: 12px; border-top: 1px solid var(--rule); }}
  #composer input {{ flex: 1; background: var(--field); color: var(--ink); border: 1px solid var(--muted); border-radius: 8px; padding: 10px; font-size: 15px; }}
  #composer button {{ background: var(--accent); color: #fff; border: 0; border-radius: 8px; padding: 10px 16px; cursor: pointer; font-weight: 600; }}
  #composer button:disabled {{ opacity: 0.45; cursor: default; }}
  #revert {{ background: var(--panel) !important; color: var(--ink) !important; border: 1px solid var(--rule) !important; }}
  @media (max-width: 760px) {{
    body {{ flex-direction: column; }}
    #sidebar {{ width: 100%; flex: none; max-height: 44vh; overflow-y: auto; border-right: 0; border-bottom: 1px solid var(--rule); }}
  }}
  @media (prefers-reduced-motion: reduce) {{
    * {{ scroll-behavior: auto !important; transition: none !important; }}
  }}
</style>
</head>
<body>
  <aside id="sidebar">
    <h1>OpenRO</h1>
    <div id="game-name">{game_name}</div>
    <button id="new-chat" type="button">New chat</button>
    <ul id="chat-list"></ul>
    <div id="status-chip" class="{chip_class}">{chip}</div>
    <div id="settings">
      <h2>Settings</h2>
      <div class="set-row">
        <span class="set-name">Appearance</span>
        <button class="theme-btn" type="button" data-set-theme="dark" aria-pressed="true">Dark</button>
        <button class="theme-btn" type="button" data-set-theme="light" aria-pressed="false">Light</button>
      </div>
    </div>
  </aside>
  <main>
    {key_banner}
    <div id="thread" aria-live="polite"></div>
    <form id="composer">
      <label class="sr-only" for="composer-input">Describe the game</label>
      <input id="composer-input" type="text" placeholder="Describe the game…" autocomplete="off">
      <button id="revert" type="button">Revert</button>
      <button id="send" type="submit">Send</button>
    </form>
  </main>
<script>
  const thread = document.getElementById('thread');
  const chatList = document.getElementById('chat-list');
  const rootEl = document.documentElement;
  const CAN_BUILD = {can_build};

  /* appearance — the settings toggle; the choice persists on this device */
  const themeBtns = document.querySelectorAll('.theme-btn');
  function syncThemeBtns() {{
    const cur = rootEl.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
    themeBtns.forEach((b) => b.setAttribute('aria-pressed', b.getAttribute('data-set-theme') === cur ? 'true' : 'false'));
  }}
  themeBtns.forEach((b) => b.addEventListener('click', () => {{
    const next = b.getAttribute('data-set-theme');
    rootEl.setAttribute('data-theme', next);
    try {{ localStorage.setItem('openro-theme', next); }} catch (e) {{}}
    syncThemeBtns();
  }}));
  syncThemeBtns();

  function lineClass(text) {{
    if (text.startsWith('You: ')) return 'msg user';
    if (text.startsWith('OpenRO: ')) return 'msg assistant';
    if (text === 'Playtest passed.') return 'msg step verdict-ok';
    if (text.startsWith('Playtest failed')) return 'msg step verdict-bad';
    return 'msg step';
  }}
  function addLine(text) {{
    const empty = thread.querySelector('.empty');
    if (empty) empty.remove();
    const div = document.createElement('div');
    div.className = lineClass(text);
    div.textContent = text;
    thread.appendChild(div);
    thread.scrollTop = thread.scrollHeight;
  }}
  function showEmpty() {{
    /* The empty state must not promise, in the present tense, work the
       current screen cannot do (no key / no Studio). */
    thread.innerHTML = CAN_BUILD
      ? '<div class="empty">Describe the game you want — OpenRO builds it in Studio and playtests it before calling it done.</div>'
      : '<div class="empty">Describe the game you want. To build it, OpenRO needs a model key and a Studio connection — see the notes above.</div>';
  }}
  async function refreshThread() {{
    try {{
      const r = await fetch('/api/thread');
      if (!r.ok) throw new Error('thread');
      const data = await r.json();
      thread.innerHTML = '';
      if (!data.thread.length) {{ showEmpty(); return; }}
      data.thread.forEach(addLine);
    }} catch (err) {{
      if (!thread.children.length) showEmpty();
    }}
  }}
  async function refreshChats() {{
    try {{
      const r = await fetch('/api/chats');
      if (!r.ok) throw new Error('chats');
      const data = await r.json();
      chatList.innerHTML = '';
      for (const c of data.chats) {{
        const li = document.createElement('li');
        /* A real <button>: chat switching is keyboard-reachable. */
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'chat-btn' + (c.id === data.active ? ' active' : '');
        btn.textContent = c.title || 'New chat';
        btn.addEventListener('click', async () => {{
          try {{
            const sr = await fetch('/api/chats/select', {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify({{ id: c.id }}) }});
            if (!sr.ok) throw new Error('select');
          }} catch (err) {{
            addLine('OpenRO: Could not switch chats — the server did not answer.');
            return;
          }}
          await refreshChats();
          await refreshThread();
        }});
        li.appendChild(btn);
        chatList.appendChild(li);
      }}
    }} catch (err) {{ /* the list keeps its last good state */ }}
  }}
  document.getElementById('composer').addEventListener('submit', async (e) => {{
    e.preventDefault();
    const input = document.getElementById('composer-input');
    const send = document.getElementById('send');
    const message = input.value.trim();
    if (!message) return;
    input.value = '';
    addLine('You: ' + message);
    send.disabled = true;
    send.textContent = 'Building…';
    try {{
      const r = await fetch('/api/chat', {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify({{ message }}) }});
      const data = await r.json();
      if (!data.ok) {{ addLine('OpenRO: ' + data.error); }}
      else {{ await refreshThread(); }}
    }} catch (err) {{
      addLine('OpenRO: Could not reach the OpenRO server.');
    }}
    send.disabled = false;
    send.textContent = 'Send';
    await refreshChats();
    input.focus();
  }});
  document.getElementById('revert').addEventListener('click', async () => {{
    try {{
      const r = await fetch('/api/revert', {{ method: 'POST' }});
      const data = await r.json();
      if (data.ok) addLine('OpenRO: Reverted ' + data.restored.length + ' change(s) in this chat.');
      else if (data.error === 'nothing to revert') addLine('OpenRO: Nothing to revert — no change set is waiting in this chat.');
      else addLine('OpenRO: ' + data.error);
    }} catch (err) {{
      addLine('OpenRO: Could not reach the OpenRO server.');
    }}
  }});
  document.getElementById('new-chat').addEventListener('click', async () => {{
    try {{
      const r = await fetch('/api/chats', {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: '{{}}' }});
      if (!r.ok) throw new Error('new chat');
    }} catch (err) {{
      addLine('OpenRO: Could not reach the OpenRO server.');
      return;
    }}
    await refreshChats();
    await refreshThread();
    document.getElementById('composer-input').focus();
  }});
  refreshChats();
  refreshThread();
</script>
</body>
</html>"##
    )
}
