//! Server-Sent Events (SSE) hub for live OpenRO build progress.
//!
//! Emits structured phase transitions (`planning` -> `compiling_rune` ->
//! `writing` -> `static_luau_check` -> `playtest_proof` -> `auto_repair` ->
//! `done` / `failed` / `reverted` / `imported` / `restored`) to any connected
//! `GET /api/build/stream` clients using a bounded ring buffer and `Condvar`
//! wakeups (zero busy polling).

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Maximum concurrent SSE subscribers (tracked separately from short-lived HTTP handlers).
pub const MAX_SSE_CLIENTS: usize = 32;
/// Ring buffer capacity for recent build events.
const MAX_RING_EVENTS: usize = 64;
/// Heartbeat interval so proxies/browsers keep the SSE connection warm.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct BuildEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub build_id: String,
    pub chat_id: String,
    pub phase: String,
    pub label: String,
    pub detail: String,
    pub skill: Option<String>,
    pub path: Option<String>,
    pub progress: u8,
}

impl BuildEvent {
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "ts_ms": self.ts_ms,
            "build_id": self.build_id,
            "chat_id": self.chat_id,
            "phase": self.phase,
            "label": self.label,
            "detail": self.detail,
            "skill": self.skill,
            "path": self.path,
            "progress": self.progress,
        })
    }
}

struct HubState {
    events: VecDeque<BuildEvent>,
    latest_seq: u64,
    current_phase: String,
    closed: bool,
}

pub struct BuildEventHub {
    state: Mutex<HubState>,
    notify: Condvar,
    next_seq: AtomicU64,
    active_clients: AtomicUsize,
}

struct SseClientGuard<'a>(&'a AtomicUsize);

impl Drop for SseClientGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for BuildEventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildEventHub {
    pub fn new() -> Self {
        BuildEventHub {
            state: Mutex::new(HubState {
                events: VecDeque::with_capacity(MAX_RING_EVENTS),
                latest_seq: 0,
                current_phase: "idle".to_string(),
                closed: false,
            }),
            notify: Condvar::new(),
            next_seq: AtomicU64::new(1),
            active_clients: AtomicUsize::new(0),
        }
    }

    pub fn active_subscribers(&self) -> usize {
        self.active_clients.load(Ordering::SeqCst)
    }

    /// Publish one build progress event to the ring buffer and wake all SSE clients.
    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        &self,
        build_id: impl Into<String>,
        chat_id: impl Into<String>,
        phase: impl Into<String>,
        label: impl Into<String>,
        detail: impl Into<String>,
        skill: Option<String>,
        path: Option<String>,
        progress: u8,
    ) -> BuildEvent {
        let phase_s = phase.into();
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let ev = BuildEvent {
            seq,
            ts_ms: now_ms(),
            build_id: build_id.into(),
            chat_id: chat_id.into(),
            phase: phase_s.clone(),
            label: label.into(),
            detail: detail.into(),
            skill,
            path,
            progress: progress.min(100),
        };
        {
            let mut st = self.state.lock().unwrap();
            if st.events.len() >= MAX_RING_EVENTS {
                st.events.pop_front();
            }
            st.events.push_back(ev.clone());
            st.latest_seq = seq;
            st.current_phase = match phase_s.as_str() {
                "done" | "failed" | "reverted" | "imported" | "restored" => "idle".to_string(),
                _ => phase_s,
            };
        }
        self.notify.notify_all();
        ev
    }

    /// Snapshot of recent events (used for initial SSE status frame and JSON polling fallback).
    pub fn recent_json(&self, since_seq: u64) -> Value {
        let st = self.state.lock().unwrap();
        let events: Vec<Value> = st
            .events
            .iter()
            .filter(|e| e.seq > since_seq)
            .map(|e| e.to_json())
            .collect();
        json!({
            "phase": st.current_phase,
            "seq": st.latest_seq,
            "subscribers": self.active_subscribers(),
            "events": events,
        })
    }

    /// Close all active SSE streams cleanly (used in tests or shutdown).
    pub fn shutdown(&self) {
        {
            let mut st = self.state.lock().unwrap();
            st.closed = true;
        }
        self.notify.notify_all();
    }

    /// Try to claim an SSE subscriber slot and stream events until client disconnect.
    /// Returns `Err` with a plain message if `MAX_SSE_CLIENTS` is already reached.
    pub fn serve_stream(
        self: &Arc<Self>,
        mut stream: TcpStream,
        active_chat: &str,
        busy: bool,
        last_event_id: u64,
    ) -> Result<(), String> {
        let prev = self.active_clients.fetch_add(1, Ordering::SeqCst);
        if prev >= MAX_SSE_CLIENTS {
            self.active_clients.fetch_sub(1, Ordering::SeqCst);
            return Err("Too many live build streams are open right now. Close an extra tab and retry.".to_string());
        }
        let _guard = SseClientGuard(&self.active_clients);

        // Keep writes bounded so a dead/stalled peer exits promptly.
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_nodelay(true);

        let headers = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream; charset=utf-8\r\n",
            "Cache-Control: no-cache, no-transform\r\n",
            "Connection: keep-alive\r\n",
            "X-Accel-Buffering: no\r\n\r\n"
        );

        // Send initial status snapshot + any missed events since Last-Event-ID.
        let (mut cursor, init_payload, missed) = {
            let st = self.state.lock().unwrap();
            let recent: Vec<Value> = st.events.iter().rev().take(12).rev().map(|e| e.to_json()).collect();
            let missed: Vec<BuildEvent> = if last_event_id > 0 {
                st.events
                    .iter()
                    .filter(|e| e.seq > last_event_id)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            let init = json!({
                "phase": if busy { st.current_phase.as_str() } else { "idle" },
                "seq": st.latest_seq,
                "busy": busy,
                "active_chat": active_chat,
                "recent": recent,
            });
            (st.latest_seq, init, missed)
        };

        let init_packet = format!("{headers}event: status\ndata: {init_payload}\n\n");
        if stream.write_all(init_packet.as_bytes()).is_err() || stream.flush().is_err() {
            return Ok(());
        }
        for ev in missed {
            if write_event_frame(&mut stream, &ev).is_err() {
                return Ok(());
            }
            if ev.seq > cursor {
                cursor = ev.seq;
            }
        }

        loop {
            let batch: Vec<BuildEvent> = {
                let mut st = self.state.lock().unwrap();
                if st.closed {
                    return Ok(());
                }
                if st.latest_seq <= cursor {
                    let (guard, _) = self.notify.wait_timeout(st, KEEPALIVE_INTERVAL).unwrap();
                    st = guard;
                }
                if st.closed {
                    return Ok(());
                }
                st.events
                    .iter()
                    .filter(|e| e.seq > cursor)
                    .cloned()
                    .collect()
            };

            if batch.is_empty() {
                if stream.write_all(b": keepalive\n\n").is_err() || stream.flush().is_err() {
                    return Ok(());
                }
                continue;
            }
            for ev in batch {
                if write_event_frame(&mut stream, &ev).is_err() {
                    return Ok(());
                }
                cursor = ev.seq;
            }
        }
    }
}

fn write_event_frame(stream: &mut TcpStream, ev: &BuildEvent) -> std::io::Result<()> {
    let frame = format!("id: {}\nevent: build\ndata: {}\n\n", ev.seq, ev.to_json());
    stream.write_all(frame.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_bounds_and_filters_by_seq() {
        let hub = BuildEventHub::new();
        for i in 0..(MAX_RING_EVENTS + 10) {
            hub.publish(
                "b1",
                "chat-1",
                "compiling_rune",
                format!("Step {i}"),
                "ok",
                Some("leaderboard".to_string()),
                None,
                50,
            );
        }
        let all = hub.recent_json(0);
        let arr = all["events"].as_array().unwrap();
        assert_eq!(arr.len(), MAX_RING_EVENTS);
        let latest = all["seq"].as_u64().unwrap();
        assert_eq!(latest, (MAX_RING_EVENTS + 10) as u64);
        let tail = hub.recent_json(latest - 3);
        assert_eq!(tail["events"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn phase_returns_to_idle_on_terminal_events() {
        let hub = BuildEventHub::new();
        hub.publish("b1", "c1", "planning", "Planning", "", None, None, 10);
        assert_eq!(hub.recent_json(0)["phase"], json!("planning"));
        hub.publish("b1", "c1", "done", "Done", "Built", None, None, 100);
        assert_eq!(hub.recent_json(0)["phase"], json!("idle"));
    }
}
