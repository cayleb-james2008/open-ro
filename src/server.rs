//! Hand-rolled minimal HTTP over `std::net::TcpListener` serving the
//! embedded web UI (plain HTML/CSS/JS string constants — no framework,
//! no build step) plus the JSON API the UI and tests use.

use crate::agent::Agent;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

pub struct Shared {
    pub agent: Mutex<Option<Agent>>,
    pub thread: Mutex<Vec<String>>,
    pub game_name: String,
}

impl Shared {
    pub fn new(agent: Option<Agent>, game_name: String) -> Arc<Shared> {
        Arc::new(Shared {
            agent: Mutex::new(agent),
            thread: Mutex::new(Vec::new()),
            game_name,
        })
    }
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
            let thread = shared.thread.lock().unwrap().clone();
            respond_json(&mut stream, "200 OK", &json!({ "thread": thread }));
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
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                None => respond_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &json!({ "ok": false, "error": "no model key configured: set OPENRO_MODEL_API_KEY to enable chat" }),
                ),
                Some(agent) => match agent.run_ask(&message) {
                    Ok(outcome) => {
                        shared
                            .thread
                            .lock()
                            .unwrap()
                            .extend(outcome.thread.iter().cloned());
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
                    Err(e) => {
                        let status = if e.contains("no model key configured") {
                            "403 Forbidden"
                        } else {
                            "502 Bad Gateway"
                        };
                        respond_json(
                            &mut stream,
                            status,
                            &json!({ "ok": false, "error": e }),
                        );
                    }
                },
            }
        }
        ("POST", "/api/revert") => {
            let mut guard = shared.agent.lock().unwrap();
            match guard.as_mut() {
                Some(agent) => match agent.revert_last() {
                    Ok(restored) => {
                        shared
                            .thread
                            .lock()
                            .unwrap()
                            .push("OpenRO: Reverted the last change set.".to_string());
                        respond_json(
                            &mut stream,
                            "200 OK",
                            &json!({ "ok": true, "restored": restored }),
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
        ("POST", "/api/new-chat") => {
            shared.thread.lock().unwrap().clear();
            respond_json(&mut stream, "200 OK", &json!({ "ok": true }));
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

/// The single-page shell: left sidebar (chat list + New chat + the game's
/// name), one scrolling thread, one composer row pinned at the bottom, and
/// ONE status chip reflecting the real bridge state.
pub fn render_page(game_name: &str, studio_connected: bool, model_configured: bool) -> String {
    let chip = if studio_connected {
        "Studio connected"
    } else {
        "Studio not connected"
    };
    let chip_class = if studio_connected { "chip on" } else { "chip off" };
    let key_banner = if model_configured {
        String::new()
    } else {
        "<div class=\"banner\" id=\"no-key-banner\">no model key configured — set OPENRO_MODEL_API_KEY to enable chat</div>"
            .to_string()
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>OpenRO — {game_name}</title>
<style>
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; font-family: system-ui, sans-serif; background: #101014; color: #eee; display: flex; height: 100vh; }}
  #sidebar {{ width: 260px; flex: 0 0 260px; background: #17171d; border-right: 1px solid #2a2a33; display: flex; flex-direction: column; padding: 16px; gap: 12px; }}
  #sidebar h1 {{ font-size: 18px; margin: 0; }}
  #game-name {{ color: #9a9aad; font-size: 13px; }}
  #new-chat {{ background: #2f6fed; color: #fff; border: 0; border-radius: 8px; padding: 9px; cursor: pointer; }}
  #chat-list {{ list-style: none; margin: 0; padding: 0; font-size: 14px; }}
  #chat-list li {{ padding: 7px 9px; border-radius: 6px; background: #20202a; }}
  .chip {{ margin-top: auto; font-size: 12px; padding: 6px 10px; border-radius: 999px; text-align: center; }}
  .chip.on {{ background: #173d24; color: #7ee2a0; }}
  .chip.off {{ background: #3d1717; color: #f09393; }}
  main {{ flex: 1; display: flex; flex-direction: column; min-width: 0; }}
  .banner {{ background: #3a2f10; color: #f2d38a; padding: 9px 14px; font-size: 13px; }}
  #thread {{ flex: 1; overflow-y: auto; padding: 18px; display: flex; flex-direction: column; gap: 10px; }}
  .msg {{ max-width: 75ch; line-height: 1.45; white-space: pre-wrap; }}
  #composer {{ display: flex; gap: 8px; padding: 12px; border-top: 1px solid #2a2a33; }}
  #composer input {{ flex: 1; background: #1b1b22; color: #eee; border: 1px solid #33333f; border-radius: 8px; padding: 10px; }}
  #composer button {{ background: #2f6fed; color: #fff; border: 0; border-radius: 8px; padding: 10px 16px; cursor: pointer; }}
  #revert {{ background: #33333f !important; }}
</style>
</head>
<body>
  <aside id="sidebar">
    <h1>OpenRO</h1>
    <div id="game-name">{game_name}</div>
    <button id="new-chat" type="button">New chat</button>
    <ul id="chat-list"><li>Current chat</li></ul>
    <div id="status-chip" class="{chip_class}">{chip}</div>
  </aside>
  <main>
    {key_banner}
    <div id="thread" aria-live="polite"></div>
    <form id="composer">
      <input id="composer-input" type="text" placeholder="Describe the game…" autocomplete="off">
      <button id="revert" type="button">Revert</button>
      <button id="send" type="submit">Send</button>
    </form>
  </main>
<script>
  const thread = document.getElementById('thread');
  function addLine(text) {{
    const div = document.createElement('div');
    div.className = 'msg';
    div.textContent = text;
    thread.appendChild(div);
    thread.scrollTop = thread.scrollHeight;
  }}
  async function refreshThread() {{
    const r = await fetch('/api/thread');
    const data = await r.json();
    thread.innerHTML = '';
    data.thread.forEach(addLine);
  }}
  document.getElementById('composer').addEventListener('submit', async (e) => {{
    e.preventDefault();
    const input = document.getElementById('composer-input');
    const message = input.value.trim();
    if (!message) return;
    input.value = '';
    addLine('You: ' + message);
    const r = await fetch('/api/chat', {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify({{ message }}) }});
    const data = await r.json();
    if (!data.ok) {{ addLine('OpenRO: ' + data.error); return; }}
    await refreshThread();
  }});
  document.getElementById('revert').addEventListener('click', async () => {{
    const r = await fetch('/api/revert', {{ method: 'POST' }});
    const data = await r.json();
    addLine(data.ok ? 'OpenRO: Reverted the last change set.' : 'OpenRO: ' + data.error);
  }});
  document.getElementById('new-chat').addEventListener('click', async () => {{
    await fetch('/api/new-chat', {{ method: 'POST' }});
    thread.innerHTML = '';
  }});
  refreshThread();
</script>
</body>
</html>"#
    )
}
