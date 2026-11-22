//! Realtime Roblox Studio action & frame stream hub for OpenRO.
//!
//! Records every granular agent Studio interaction (`PartCreated`, `PartUpdated`,
//! `ScriptWritten`, `DecalBound`, `LuauExecuted`, `PlaytestStarted`, `PlaytestLog`,
//! `PlaytestPassed`, `PlaytestFailed`, `StudioFrame`, `CameraFocus`, `Reverted`,
//! `Imported`, `Restored`) in a bounded 500-event ring buffer, stores the latest
//! captured live Roblox Studio viewport frame (`StudioFrame`), and fans out
//! non-blocking SSE updates (`event: studio_action`, `event: studio_frame`) to
//! up to 32 concurrent subscribers with `Last-Event-ID` / `?since` replay.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Bounded ring buffer capacity for recorded Studio actions.
pub const MAX_STUDIO_RING_ACTIONS: usize = 500;
/// Bounded ring buffer capacity for unified SSE stream notifications.
const MAX_STREAM_LOG_ITEMS: usize = 600;
/// Maximum concurrent Studio SSE subscribers.
pub const MAX_STUDIO_SSE_CLIENTS: usize = 32;
/// SSE heartbeat interval to keep proxies and browser EventSources alive.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioActionKind {
    PartCreated,
    PartUpdated,
    ScriptWritten,
    DecalBound,
    LuauExecuted,
    PlaytestStarted,
    PlaytestLog,
    PlaytestPassed,
    PlaytestFailed,
    StudioFrame,
    CameraFocus,
    Reverted,
    Imported,
    Restored,
}

impl StudioActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StudioActionKind::PartCreated => "part_created",
            StudioActionKind::PartUpdated => "part_updated",
            StudioActionKind::ScriptWritten => "script_written",
            StudioActionKind::DecalBound => "decal_bound",
            StudioActionKind::LuauExecuted => "luau_executed",
            StudioActionKind::PlaytestStarted => "playtest_started",
            StudioActionKind::PlaytestLog => "playtest_log",
            StudioActionKind::PlaytestPassed => "playtest_passed",
            StudioActionKind::PlaytestFailed => "playtest_failed",
            StudioActionKind::StudioFrame => "studio_frame",
            StudioActionKind::CameraFocus => "camera_focus",
            StudioActionKind::Reverted => "reverted",
            StudioActionKind::Imported => "imported",
            StudioActionKind::Restored => "restored",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StudioActionKind::PartCreated => "Part Created",
            StudioActionKind::PartUpdated => "Part Updated",
            StudioActionKind::ScriptWritten => "Script Written",
            StudioActionKind::DecalBound => "Artwork Bound",
            StudioActionKind::LuauExecuted => "Luau Executed",
            StudioActionKind::PlaytestStarted => "Playtest Started",
            StudioActionKind::PlaytestLog => "Console Output",
            StudioActionKind::PlaytestPassed => "Playtest Passed",
            StudioActionKind::PlaytestFailed => "Playtest Failed",
            StudioActionKind::StudioFrame => "Studio Frame Captured",
            StudioActionKind::CameraFocus => "Camera Focus",
            StudioActionKind::Reverted => "Build Reverted",
            StudioActionKind::Imported => "Place Imported",
            StudioActionKind::Restored => "Snapshot Restored",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudioActionEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub kind: StudioActionKind,
    /// `"live_mcp"` or `"local_place"`
    pub studio_mode: String,
    pub path: String,
    pub class_name: String,
    pub position: Option<[f64; 3]>,
    pub size: Option<[f64; 3]>,
    pub color: Option<String>,
    pub summary: String,
    pub code_snippet: Option<String>,
    pub console_line: Option<String>,
    pub frame_seq: Option<u64>,
    pub instance_count: usize,
    pub script_count: usize,
}

impl StudioActionEvent {
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "ts_ms": self.ts_ms,
            "kind": self.kind.as_str(),
            "kind_label": self.kind.label(),
            "studio_mode": self.studio_mode,
            "path": self.path,
            "class_name": self.class_name,
            "position": self.position,
            "size": self.size,
            "color": self.color,
            "summary": self.summary,
            "code_snippet": self.code_snippet,
            "console_line": self.console_line,
            "frame_seq": self.frame_seq,
            "instance_count": self.instance_count,
            "script_count": self.script_count,
        })
    }
}

/// Ergonomic builder input for `StudioStreamHub::record_action`.
#[derive(Clone, Debug)]
pub struct StudioActionInput {
    pub kind: StudioActionKind,
    pub studio_mode: Option<String>,
    pub path: String,
    pub class_name: String,
    pub position: Option<[f64; 3]>,
    pub size: Option<[f64; 3]>,
    pub color: Option<String>,
    pub summary: String,
    pub code_snippet: Option<String>,
    pub console_line: Option<String>,
    pub frame_seq: Option<u64>,
    pub instance_count: usize,
    pub script_count: usize,
}

impl StudioActionInput {
    pub fn new(kind: StudioActionKind, summary: impl Into<String>) -> Self {
        StudioActionInput {
            kind,
            studio_mode: None,
            path: String::new(),
            class_name: String::new(),
            position: None,
            size: None,
            color: None,
            summary: summary.into(),
            code_snippet: None,
            console_line: None,
            frame_seq: None,
            instance_count: 0,
            script_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudioFrame {
    pub seq: u64,
    pub ts_ms: u64,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub source: String,
}

impl StudioFrame {
    pub fn meta_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "ts_ms": self.ts_ms,
            "mime": self.mime,
            "bytes": self.bytes.len(),
            "width": self.width,
            "height": self.height,
            "source": self.source,
            "url": format!("/api/studio/frame?seq={}", self.seq),
        })
    }
}

/// One ordered SSE notification delivered to subscribers (`event: studio_action` or `event: studio_frame`).
#[derive(Clone, Debug)]
pub struct StudioStreamMessage {
    pub stream_seq: u64,
    pub event_name: &'static str,
    pub data: Value,
}

impl StudioStreamMessage {
    pub fn to_sse_frame(&self) -> String {
        format!(
            "id: {}\nevent: {}\ndata: {}\n\n",
            self.stream_seq, self.event_name, self.data
        )
    }
}

struct HubState {
    studio_mode: String,
    actions: VecDeque<StudioActionEvent>,
    stream_log: VecDeque<StudioStreamMessage>,
    latest_action_seq: u64,
    latest_stream_seq: u64,
    latest_frame: Option<StudioFrame>,
    closed: bool,
}

pub struct StudioStreamHub {
    state: Mutex<HubState>,
    notify: Condvar,
    next_action_seq: AtomicU64,
    next_frame_seq: AtomicU64,
    next_stream_seq: AtomicU64,
    active_clients: AtomicUsize,
}

/// Subscription handle holding one of the `MAX_STUDIO_SSE_CLIENTS` slots (released on `Drop`).
pub struct StudioSubscription {
    hub: Arc<StudioStreamHub>,
    cursor: u64,
}

impl StudioSubscription {
    /// Current stream cursor sequence number.
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// Non-blocking drain of all queued messages after `cursor`.
    pub fn drain_pending(&mut self) -> Vec<StudioStreamMessage> {
        let st = self.hub.state.lock().unwrap();
        let batch: Vec<StudioStreamMessage> = st
            .stream_log
            .iter()
            .filter(|m| m.stream_seq > self.cursor)
            .cloned()
            .collect();
        if let Some(last) = batch.last() {
            self.cursor = last.stream_seq;
        }
        batch
    }

    /// Wait up to `timeout` for new stream messages. Returns `None` if hub was shut down.
    pub fn recv_batch(&mut self, timeout: Duration) -> Option<Vec<StudioStreamMessage>> {
        let mut st = self.hub.state.lock().unwrap();
        if st.closed {
            return None;
        }
        if st.latest_stream_seq <= self.cursor {
            let (guard, _) = self.hub.notify.wait_timeout(st, timeout).unwrap();
            st = guard;
        }
        if st.closed {
            return None;
        }
        let batch: Vec<StudioStreamMessage> = st
            .stream_log
            .iter()
            .filter(|m| m.stream_seq > self.cursor)
            .cloned()
            .collect();
        if let Some(last) = batch.last() {
            self.cursor = last.stream_seq;
        }
        Some(batch)
    }
}

impl Drop for StudioSubscription {
    fn drop(&mut self) {
        self.hub.active_clients.fetch_sub(1, Ordering::SeqCst);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn normalize_studio_mode(mode: &str) -> String {
    match mode.trim() {
        "live_mcp" | "studio_mcp" => "live_mcp".to_string(),
        _ => "local_place".to_string(),
    }
}

impl Default for StudioStreamHub {
    fn default() -> Self {
        Self::new("local_place")
    }
}

impl StudioStreamHub {
    pub fn new(studio_mode: impl Into<String>) -> Self {
        let raw_mode = studio_mode.into();
        StudioStreamHub {
            state: Mutex::new(HubState {
                studio_mode: normalize_studio_mode(&raw_mode),
                actions: VecDeque::with_capacity(MAX_STUDIO_RING_ACTIONS),
                stream_log: VecDeque::with_capacity(MAX_STREAM_LOG_ITEMS),
                latest_action_seq: 0,
                latest_stream_seq: 0,
                latest_frame: None,
                closed: false,
            }),
            notify: Condvar::new(),
            next_action_seq: AtomicU64::new(1),
            next_frame_seq: AtomicU64::new(1),
            next_stream_seq: AtomicU64::new(1),
            active_clients: AtomicUsize::new(0),
        }
    }

    pub fn set_studio_mode(&self, mode: impl Into<String>) {
        let normalized = normalize_studio_mode(&mode.into());
        let mut st = self.state.lock().unwrap();
        st.studio_mode = normalized;
    }

    pub fn studio_mode(&self) -> String {
        self.state.lock().unwrap().studio_mode.clone()
    }

    pub fn active_subscribers(&self) -> usize {
        self.active_clients.load(Ordering::SeqCst)
    }

    pub fn latest_action_seq(&self) -> u64 {
        self.state.lock().unwrap().latest_action_seq
    }

    pub fn latest_frame_seq(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .latest_frame
            .as_ref()
            .map(|f| f.seq)
            .unwrap_or(0)
    }

    /// Record a Studio action into the bounded 500-event ring buffer and broadcast
    /// `event: studio_action` to all live subscribers.
    pub fn record_action(&self, input: StudioActionInput) -> StudioActionEvent {
        let seq = self.next_action_seq.fetch_add(1, Ordering::SeqCst);
        let stream_seq = self.next_stream_seq.fetch_add(1, Ordering::SeqCst);
        let ts_ms = now_ms();

        let ev = {
            let mut st = self.state.lock().unwrap();
            let studio_mode = input
                .studio_mode
                .as_deref()
                .map(normalize_studio_mode)
                .unwrap_or_else(|| st.studio_mode.clone());
            let ev = StudioActionEvent {
                seq,
                ts_ms,
                kind: input.kind,
                studio_mode,
                path: input.path,
                class_name: input.class_name,
                position: input.position,
                size: input.size,
                color: input.color,
                summary: input.summary,
                code_snippet: input.code_snippet,
                console_line: input.console_line,
                frame_seq: input.frame_seq,
                instance_count: input.instance_count,
                script_count: input.script_count,
            };
            if st.actions.len() >= MAX_STUDIO_RING_ACTIONS {
                st.actions.pop_front();
            }
            st.actions.push_back(ev.clone());
            st.latest_action_seq = seq;

            if st.stream_log.len() >= MAX_STREAM_LOG_ITEMS {
                st.stream_log.pop_front();
            }
            st.stream_log.push_back(StudioStreamMessage {
                stream_seq,
                event_name: "studio_action",
                data: ev.to_json(),
            });
            st.latest_stream_seq = stream_seq;
            ev
        };

        self.notify.notify_all();
        ev
    }

    /// Return all recorded actions strictly after `since_seq` (oldest first).
    pub fn actions_since(&self, since_seq: u64) -> Vec<StudioActionEvent> {
        let st = self.state.lock().unwrap();
        st.actions
            .iter()
            .filter(|a| a.seq > since_seq)
            .cloned()
            .collect()
    }

    /// Return a full snapshot of the bounded ring buffer (up to 500 actions, oldest first).
    pub fn snapshot_actions(&self) -> Vec<StudioActionEvent> {
        let st = self.state.lock().unwrap();
        st.actions.iter().cloned().collect()
    }

    /// Store a newly captured live Roblox Studio viewport frame, broadcast
    /// `event: studio_frame`, and record a matching `StudioActionKind::StudioFrame` action.
    pub fn store_frame(&self, mut frame: StudioFrame) -> StudioFrame {
        let frame_seq = self.next_frame_seq.fetch_add(1, Ordering::SeqCst);
        let action_seq = self.next_action_seq.fetch_add(1, Ordering::SeqCst);
        let frame_stream_seq = self.next_stream_seq.fetch_add(1, Ordering::SeqCst);
        let action_stream_seq = self.next_stream_seq.fetch_add(1, Ordering::SeqCst);
        let ts_ms = if frame.ts_ms > 0 {
            frame.ts_ms
        } else {
            now_ms()
        };
        frame.seq = frame_seq;
        frame.ts_ms = ts_ms;

        {
            let mut st = self.state.lock().unwrap();
            let frame_meta = frame.meta_json();
            let summary = format!(
                "Captured live Studio viewport frame #{frame_seq} ({}x{}, {})",
                frame.width, frame.height, frame.mime
            );
            let studio_mode = st.studio_mode.clone();
            let last_counts = st
                .actions
                .back()
                .map(|a| (a.instance_count, a.script_count))
                .unwrap_or((0, 0));

            let action_ev = StudioActionEvent {
                seq: action_seq,
                ts_ms,
                kind: StudioActionKind::StudioFrame,
                studio_mode,
                path: "Workspace.CurrentCamera".to_string(),
                class_name: "Camera".to_string(),
                position: None,
                size: None,
                color: None,
                summary,
                code_snippet: None,
                console_line: None,
                frame_seq: Some(frame_seq),
                instance_count: last_counts.0,
                script_count: last_counts.1,
            };

            st.latest_frame = Some(frame.clone());
            if st.actions.len() >= MAX_STUDIO_RING_ACTIONS {
                st.actions.pop_front();
            }
            st.actions.push_back(action_ev.clone());
            st.latest_action_seq = action_seq;

            if st.stream_log.len() >= MAX_STREAM_LOG_ITEMS {
                st.stream_log.pop_front();
            }
            st.stream_log.push_back(StudioStreamMessage {
                stream_seq: frame_stream_seq,
                event_name: "studio_frame",
                data: frame_meta,
            });
            if st.stream_log.len() >= MAX_STREAM_LOG_ITEMS {
                st.stream_log.pop_front();
            }
            st.stream_log.push_back(StudioStreamMessage {
                stream_seq: action_stream_seq,
                event_name: "studio_action",
                data: action_ev.to_json(),
            });
            st.latest_stream_seq = action_stream_seq;
        }

        self.notify.notify_all();
        frame
    }

    /// Return the most recently captured `StudioFrame`, if any.
    pub fn latest_frame(&self) -> Option<StudioFrame> {
        self.state.lock().unwrap().latest_frame.clone()
    }

    /// JSON response payload for `GET /api/studio/actions` (and initial SSE `status` event).
    pub fn snapshot_json(&self, since_seq: u64) -> Value {
        let st = self.state.lock().unwrap();
        let actions: Vec<Value> = st
            .actions
            .iter()
            .filter(|a| a.seq > since_seq)
            .map(StudioActionEvent::to_json)
            .collect();
        let frame_meta = st.latest_frame.as_ref().map(StudioFrame::meta_json);
        let frame_seq = st.latest_frame.as_ref().map(|f| f.seq).unwrap_or(0);
        json!({
            "studio_mode": st.studio_mode,
            "has_live_frame": st.latest_frame.is_some(),
            "frame_seq": frame_seq,
            "latest_seq": st.latest_action_seq,
            "stream_seq": st.latest_stream_seq,
            "subscribers": self.active_subscribers(),
            "frame": frame_meta,
            "actions": actions,
        })
    }

    /// Claim one of the `MAX_STUDIO_SSE_CLIENTS` slots and return a `StudioSubscription`
    /// positioned at `since_seq` (for replay via `Last-Event-ID` / `?since`).
    pub fn subscribe(self: &Arc<Self>, since_seq: u64) -> Result<StudioSubscription, String> {
        let prev = self.active_clients.fetch_add(1, Ordering::SeqCst);
        if prev >= MAX_STUDIO_SSE_CLIENTS {
            self.active_clients.fetch_sub(1, Ordering::SeqCst);
            return Err(
                "Too many live Studio viewport streams are open right now. Close an extra tab and retry."
                    .to_string(),
            );
        }
        Ok(StudioSubscription {
            hub: Arc::clone(self),
            cursor: since_seq,
        })
    }

    /// Shut down all active SSE subscribers cleanly (for graceful exit / tests).
    pub fn shutdown(&self) {
        {
            let mut st = self.state.lock().unwrap();
            st.closed = true;
        }
        self.notify.notify_all();
    }

    /// Serve a live HTTP Server-Sent Events stream (`GET /api/studio/stream`) with:
    /// - Initial `event: status` snapshot
    /// - Replay of missed `studio_action` and `studio_frame` events after `last_event_id`
    /// - Non-blocking live delivery & 15s keepalive heartbeats
    /// - Strict 32-client concurrency cap.
    pub fn serve_stream(
        self: &Arc<Self>,
        mut stream: TcpStream,
        last_event_id: u64,
    ) -> Result<(), String> {
        let mut sub = self.subscribe(last_event_id)?;

        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_nodelay(true);

        let headers = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream; charset=utf-8\r\n",
            "Cache-Control: no-cache, no-transform\r\n",
            "Connection: keep-alive\r\n",
            "X-Accel-Buffering: no\r\n\r\n"
        );

        let (init_payload, replay) = {
            let st = self.state.lock().unwrap();
            let recent: Vec<Value> = st
                .actions
                .iter()
                .rev()
                .take(40)
                .rev()
                .map(StudioActionEvent::to_json)
                .collect();
            let frame_meta = st.latest_frame.as_ref().map(StudioFrame::meta_json);
            let frame_seq = st.latest_frame.as_ref().map(|f| f.seq).unwrap_or(0);
            let init = json!({
                "studio_mode": st.studio_mode,
                "has_live_frame": st.latest_frame.is_some(),
                "frame_seq": frame_seq,
                "latest_seq": st.latest_action_seq,
                "stream_seq": st.latest_stream_seq,
                "frame": frame_meta,
                "recent": recent,
            });
            let missed: Vec<StudioStreamMessage> = if last_event_id > 0 {
                st.stream_log
                    .iter()
                    .filter(|m| m.stream_seq > last_event_id)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            sub.cursor = st.latest_stream_seq;
            (init, missed)
        };

        let init_packet = format!("{headers}event: status\ndata: {init_payload}\n\n");
        if stream.write_all(init_packet.as_bytes()).is_err() || stream.flush().is_err() {
            return Ok(());
        }
        for msg in replay {
            let frame = msg.to_sse_frame();
            if stream.write_all(frame.as_bytes()).is_err() || stream.flush().is_err() {
                return Ok(());
            }
        }

        loop {
            let Some(batch) = sub.recv_batch(KEEPALIVE_INTERVAL) else {
                return Ok(());
            };
            if batch.is_empty() {
                if stream.write_all(b": keepalive\n\n").is_err() || stream.flush().is_err() {
                    return Ok(());
                }
                continue;
            }
            for msg in batch {
                let frame = msg.to_sse_frame();
                if stream.write_all(frame.as_bytes()).is_err() || stream.flush().is_err() {
                    return Ok(());
                }
            }
        }
    }
}
