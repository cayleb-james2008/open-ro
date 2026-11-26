//! Antigravity-style Teamwork mode for OpenRO:
//! - Goal -> coordinator split -> named role workers -> verifier gate -> git auto-commit
//! - Append-only board persistence under `<project>/.openro/teamwork/<run>/`
//! - Atomic file-ownership claims (refusing conflicting claims or edits)
//! - Internal `StudioLease` serializing sync and playtest operations
//! - Quota-aware concurrency scheduler (`max_concurrency` default 2, agy quota backoff + reset display)
//! - Human posts, stop/resume, per-agent kill/restart, per-agent trace, and SSE board events

use crate::agent::parse_step;
use crate::board::{now_ms, Board, BoardKind, BoardMessage};
use crate::model::{model_from_config, ModelClient, AGY_DEFAULT_MODEL};
use crate::projects::{self, ProjectRef};
use crate::server::Shared;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub const MAX_TEAMWORK_SSE_CLIENTS: usize = 32;
const MAX_SSE_RING: usize = 256;
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const DEFAULT_MAX_CONCURRENCY: usize = 2;
const MAX_AGENT_STEPS_PER_WAKE: usize = 24;
const MAX_TRANSIENT_MODEL_RETRIES: usize = 2;
const TEAMWORK_READ_FILE_MAX_BYTES: usize = 8 * 1024;
const MAX_TURN_TOOL_RESULT_BYTES: usize = 28 * 1024;
const READ_ONLY_SPIN_THRESHOLD: usize = 3;
const DUPLICATE_READ_WINDOW_TURNS: u64 = 6;
const HISTORY_TOOL_RESULT_BUDGET_BYTES: usize = 56 * 1024;
const HISTORY_RETAIN_TURNS: usize = 8;
const HISTORY_TOTAL_PROMPT_BUDGET_BYTES: usize = 76 * 1024;
const MAX_FILE_BYTES: usize = 1_048_576;

const DEFAULT_ROLES: [&str; 4] = ["researcher", "builder", "art", "verifier"];

// ---------------------------------------------------------------------------
// Core domain enums & records
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Completed,
    Stopped,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Stopped => "stopped",
            RunStatus::Failed => "failed",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "completed" => RunStatus::Completed,
            "stopped" => RunStatus::Stopped,
            "failed" => RunStatus::Failed,
            _ => RunStatus::Running,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentWorkState {
    Working,
    Waiting,
    Stopped,
}

impl AgentWorkState {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentWorkState::Working => "working",
            AgentWorkState::Waiting => "waiting",
            AgentWorkState::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    GameCode,
    Art,
    Mesh,
    Docs,
    Research,
    Other,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::GameCode => "game_code",
            TaskKind::Art => "art",
            TaskKind::Mesh => "mesh",
            TaskKind::Docs => "docs",
            TaskKind::Research => "research",
            TaskKind::Other => "other",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "game_code" | "code" | "luau" | "systems" | "ui" | "world" => TaskKind::GameCode,
            "art" => TaskKind::Art,
            "mesh" | "3d" => TaskKind::Mesh,
            "docs" | "design" => TaskKind::Docs,
            "research" | "researcher" => TaskKind::Research,
            _ => TaskKind::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    AwaitingVerification,
    ChangesRequested,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::AwaitingVerification => "awaiting_verification",
            TaskStatus::ChangesRequested => "changes_requested",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "in_progress" => TaskStatus::InProgress,
            "awaiting_verification" => TaskStatus::AwaitingVerification,
            "changes_requested" => TaskStatus::ChangesRequested,
            "done" => TaskStatus::Done,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            _ => TaskStatus::Pending,
        }
    }

    pub fn holds_claims(self) -> bool {
        !matches!(self, TaskStatus::Done | TaskStatus::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct VerifyReport {
    pub passed: bool,
    pub verifier: String,
    pub playtest_passed: Option<bool>,
    pub console_errors: usize,
    pub notes: String,
    pub at_ms: u64,
}

impl VerifyReport {
    fn to_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "verifier": self.verifier,
            "playtest_passed": self.playtest_passed,
            "console_errors": self.console_errors,
            "notes": self.notes,
            "at_ms": self.at_ms,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(VerifyReport {
            passed: v.get("passed")?.as_bool()?,
            verifier: v.get("verifier")?.as_str()?.to_string(),
            playtest_passed: v.get("playtest_passed").and_then(|x| x.as_bool()),
            console_errors: v.get("console_errors").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            notes: v.get("notes").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            at_ms: v.get("at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct PlaytestCheckRecord {
    pub passed: bool,
    pub console_errors: usize,
    pub run_id: Option<String>,
    pub summary: String,
    pub at_ms: u64,
}

impl PlaytestCheckRecord {
    fn to_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "console_errors": self.console_errors,
            "run_id": self.run_id,
            "summary": self.summary,
            "at_ms": self.at_ms,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(PlaytestCheckRecord {
            passed: v.get("passed")?.as_bool()?,
            console_errors: v.get("console_errors").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            run_id: v.get("run_id").and_then(|x| x.as_str()).map(String::from),
            summary: v.get("summary").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            at_ms: v.get("at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct LuneCheckRecord {
    pub passed: bool,
    pub script: String,
    pub at_ms: u64,
}

impl LuneCheckRecord {
    fn to_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "script": self.script,
            "at_ms": self.at_ms,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(LuneCheckRecord {
            passed: v.get("passed")?.as_bool()?,
            script: v.get("script").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            at_ms: v.get("at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: TaskKind,
    pub owner: String,
    pub verifier: String,
    pub status: TaskStatus,
    pub claimed_files: Vec<String>,
    pub written_files: Vec<String>,
    pub depends_on: Vec<String>,
    pub result_summary: Option<String>,
    pub verify_report: Option<VerifyReport>,
    pub last_playtest: Option<PlaytestCheckRecord>,
    pub last_lune: Option<LuneCheckRecord>,
    pub commit_sha: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_edit_ms: u64,
}

impl Task {
    pub fn is_game_code(&self) -> bool {
        let has_files = !self.claimed_files.is_empty() || !self.written_files.is_empty();
        let touches_runtime_code = self
            .claimed_files
            .iter()
            .chain(self.written_files.iter())
            .any(|p| {
                (p.starts_with("src/") || p.starts_with("ui/"))
                    && !matches!(
                        p.as_str(),
                        "src/shared/AssetIds.luau" | "src/shared/ProductIds.luau"
                    )
            });
        touches_runtime_code || (self.kind == TaskKind::GameCode && !has_files)
    }

    pub fn is_lune_tool(&self) -> bool {
        !self.is_game_code()
            && self
                .claimed_files
                .iter()
                .chain(self.written_files.iter())
                .any(|p| {
                    p.starts_with("tools/")
                        && (p.ends_with(".luau") || p.ends_with(".lua"))
                })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "kind": self.kind.as_str(),
            "owner": self.owner,
            "verifier": self.verifier,
            "status": self.status.as_str(),
            "claimed_files": self.claimed_files,
            "written_files": self.written_files,
            "depends_on": self.depends_on,
            "result_summary": self.result_summary,
            "verify_report": self.verify_report.as_ref().map(VerifyReport::to_json),
            "commit_sha": self.commit_sha,
            "created_at_ms": self.created_at_ms,
            "updated_at_ms": self.updated_at_ms,
        })
    }

    fn to_persisted_json(&self) -> Value {
        let mut v = self.to_json();
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "last_playtest".into(),
                self.last_playtest
                    .as_ref()
                    .map(PlaytestCheckRecord::to_json)
                    .unwrap_or(Value::Null),
            );
            o.insert(
                "last_lune".into(),
                self.last_lune
                    .as_ref()
                    .map(LuneCheckRecord::to_json)
                    .unwrap_or(Value::Null),
            );
            o.insert("last_edit_ms".into(), json!(self.last_edit_ms));
        }
        v
    }

    fn from_json(v: &Value) -> Option<Self> {
        let str_arr = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|i| i.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        Some(Task {
            id: v.get("id")?.as_str()?.to_string(),
            title: v.get("title")?.as_str()?.to_string(),
            description: v.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            kind: v
                .get("kind")
                .and_then(|x| x.as_str())
                .map(TaskKind::parse)
                .unwrap_or(TaskKind::Other),
            owner: v.get("owner")?.as_str()?.to_string(),
            verifier: v
                .get("verifier")
                .and_then(|x| x.as_str())
                .unwrap_or("verifier")
                .to_string(),
            status: v
                .get("status")
                .and_then(|x| x.as_str())
                .map(TaskStatus::parse)
                .unwrap_or(TaskStatus::Pending),
            claimed_files: str_arr("claimed_files"),
            written_files: str_arr("written_files"),
            depends_on: str_arr("depends_on"),
            result_summary: v.get("result_summary").and_then(|x| x.as_str()).map(String::from),
            verify_report: v.get("verify_report").and_then(VerifyReport::from_json),
            last_playtest: v.get("last_playtest").and_then(PlaytestCheckRecord::from_json),
            last_lune: v.get("last_lune").and_then(LuneCheckRecord::from_json),
            commit_sha: v.get("commit_sha").and_then(|x| x.as_str()).map(String::from),
            created_at_ms: v.get("created_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            updated_at_ms: v.get("updated_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            last_edit_ms: v.get("last_edit_ms").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct FileClaim {
    pub path: String,
    pub task_id: String,
    pub owner: String,
    pub claimed_at_ms: u64,
}

impl FileClaim {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "task_id": self.task_id,
            "owner": self.owner,
            "claimed_at_ms": self.claimed_at_ms,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(FileClaim {
            path: v.get("path")?.as_str()?.to_string(),
            task_id: v.get("task_id")?.as_str()?.to_string(),
            owner: v.get("owner")?.as_str()?.to_string(),
            claimed_at_ms: v.get("claimed_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct AgentRecord {
    pub name: String,
    pub role: String,
    pub brief: Option<String>,
    pub model_id: String,
    pub state: AgentWorkState,
    pub killed: bool,
    pub chat_id: String,
    pub cursor_seq: u64,
    pub turns: u64,
    pub trace_count: u64,
    pub last_post_seq: Option<u64>,
    pub current_task_id: Option<String>,
    pub last_error: Option<String>,
    pub updated_at_ms: u64,
    pub generation: u64,
    pub parked_at_seq: Option<u64>,
    pub messages: Vec<Value>,
}

impl AgentRecord {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "role": self.role,
            "state": self.state.as_str(),
            "killed": self.killed,
            "chat_id": self.chat_id,
            "model_id": self.model_id,
            "current_task_id": self.current_task_id,
            "cursor_seq": self.cursor_seq,
            "turns": self.turns,
            "trace_count": self.trace_count,
            "last_post_seq": self.last_post_seq,
            "last_error": self.last_error,
            "updated_at_ms": self.updated_at_ms,
        })
    }

    fn to_persisted_json(&self) -> Value {
        json!({
            "name": self.name,
            "role": self.role,
            "brief": self.brief,
            "model_id": self.model_id,
            "killed": self.killed,
            "chat_id": self.chat_id,
            "cursor_seq": self.cursor_seq,
            "turns": self.turns,
            "trace_count": self.trace_count,
            "last_post_seq": self.last_post_seq,
            "current_task_id": self.current_task_id,
            "last_error": self.last_error,
            "updated_at_ms": self.updated_at_ms,
            "messages": self.messages,
        })
    }

    fn from_persisted_json(v: &Value) -> Option<Self> {
        let mut messages = v
            .get("messages")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        compact_agent_history(&mut messages);
        Some(AgentRecord {
            name: v.get("name")?.as_str()?.to_string(),
            role: v.get("role")?.as_str()?.to_string(),
            brief: v.get("brief").and_then(|x| x.as_str()).map(String::from),
            model_id: v
                .get("model_id")
                .and_then(|x| x.as_str())
                .unwrap_or(AGY_DEFAULT_MODEL)
                .to_string(),
            state: AgentWorkState::Stopped,
            killed: v.get("killed").and_then(|x| x.as_bool()).unwrap_or(false),
            chat_id: v.get("chat_id")?.as_str()?.to_string(),
            cursor_seq: v.get("cursor_seq").and_then(|x| x.as_u64()).unwrap_or(0),
            turns: v.get("turns").and_then(|x| x.as_u64()).unwrap_or(0),
            trace_count: v.get("trace_count").and_then(|x| x.as_u64()).unwrap_or(0),
            last_post_seq: v.get("last_post_seq").and_then(|x| x.as_u64()),
            current_task_id: v.get("current_task_id").and_then(|x| x.as_str()).map(String::from),
            last_error: v.get("last_error").and_then(|x| x.as_str()).map(String::from),
            updated_at_ms: v.get("updated_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            generation: 1,
            parked_at_seq: None,
            messages,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TraceEntry {
    pub seq: u64,
    pub ts_ms: u64,
    pub turn: u64,
    pub kind: String,
    pub summary: String,
    pub data: Value,
}

impl TraceEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "ts_ms": self.ts_ms,
            "turn": self.turn,
            "kind": self.kind,
            "summary": self.summary,
            "data": self.data,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct QuotaState {
    pub active: bool,
    pub reset_in: Option<String>,
    pub resume_at_ms: Option<u64>,
    pub message: Option<String>,
    pub retries: u32,
}

impl QuotaState {
    pub fn to_json(&self) -> Value {
        json!({
            "active": self.active,
            "reset_in": self.reset_in,
            "resume_at_ms": self.resume_at_ms,
            "message": self.message,
            "retries": self.retries,
        })
    }
}

#[derive(Clone, Debug)]
pub struct StudioLeaseInfo {
    pub holder_agent: String,
    pub task_id: Option<String>,
    pub operation: String,
    pub acquired_at_ms: u64,
}

impl StudioLeaseInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "holder_agent": self.holder_agent,
            "task_id": self.task_id,
            "operation": self.operation,
            "acquired_at_ms": self.acquired_at_ms,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TeamEvent {
    pub event_seq: u64,
    pub ts_ms: u64,
    pub event_kind: String,
    pub payload: Value,
}

// ---------------------------------------------------------------------------
// Internal Studio lease (one sync or playtest at a time across the server)
// ---------------------------------------------------------------------------

pub struct StudioLeaseLock {
    holder: Mutex<Option<StudioLeaseInfo>>,
    cv: Condvar,
}

impl Default for StudioLeaseLock {
    fn default() -> Self {
        Self::new()
    }
}

impl StudioLeaseLock {
    pub fn new() -> Self {
        StudioLeaseLock {
            holder: Mutex::new(None),
            cv: Condvar::new(),
        }
    }

    pub fn current(&self) -> Option<StudioLeaseInfo> {
        self.holder.lock().unwrap().clone()
    }

    pub fn acquire<F>(
        self: &Arc<Self>,
        agent: &str,
        task_id: Option<String>,
        operation: &str,
        cancelled: F,
    ) -> Result<StudioLeaseGuard, String>
    where
        F: Fn() -> bool,
    {
        loop {
            if cancelled() {
                return Err("cancelled while waiting for internal Studio lease".to_string());
            }
            let guard = self.holder.lock().unwrap();
            if guard.is_none() {
                let mut g = guard;
                let info = StudioLeaseInfo {
                    holder_agent: agent.to_string(),
                    task_id,
                    operation: operation.to_string(),
                    acquired_at_ms: now_ms(),
                };
                *g = Some(info);
                return Ok(StudioLeaseGuard { lock: Arc::clone(self) });
            }
            let (g, _) = self.cv.wait_timeout(guard, Duration::from_millis(80)).unwrap();
            drop(g);
        }
    }
}

pub struct StudioLeaseGuard {
    lock: Arc<StudioLeaseLock>,
}

impl Drop for StudioLeaseGuard {
    fn drop(&mut self) {
        *self.lock.holder.lock().unwrap() = None;
        self.lock.cv.notify_all();
    }
}

// ---------------------------------------------------------------------------
// Quota-aware concurrency scheduler
// ---------------------------------------------------------------------------

struct SchedulerInner {
    max_concurrency: usize,
    active_calls: usize,
    peak_concurrency: usize,
    quota: QuotaState,
    stopped: bool,
}

pub struct QuotaScheduler {
    inner: Mutex<SchedulerInner>,
    cv: Condvar,
}

impl QuotaScheduler {
    pub fn new(max_concurrency: usize) -> Self {
        QuotaScheduler {
            inner: Mutex::new(SchedulerInner {
                max_concurrency: max_concurrency.clamp(1, 16),
                active_calls: 0,
                peak_concurrency: 0,
                quota: QuotaState::default(),
                stopped: false,
            }),
            cv: Condvar::new(),
        }
    }

    pub fn snapshot(&self) -> (usize, usize, usize, QuotaState) {
        let st = self.inner.lock().unwrap();
        (
            st.max_concurrency,
            st.active_calls,
            st.peak_concurrency,
            st.quota.clone(),
        )
    }

    pub fn set_stopped(&self, stopped: bool) {
        {
            let mut st = self.inner.lock().unwrap();
            st.stopped = stopped;
            if !stopped {
                st.quota.active = false;
                st.quota.resume_at_ms = None;
            }
        }
        self.cv.notify_all();
    }

    pub fn clear_quota_on_success(&self) -> bool {
        let mut st = self.inner.lock().unwrap();
        let was_active = st.quota.active || st.quota.retries > 0;
        if was_active {
            st.quota = QuotaState::default();
            self.cv.notify_all();
        }
        was_active
    }

    pub fn record_quota_error(&self, reset_in: Option<String>, backoff_ms: u64, message: String) -> QuotaState {
        let mut st = self.inner.lock().unwrap();
        let retries = st.quota.retries.saturating_add(1);
        let resume_at = now_ms().saturating_add(backoff_ms.max(10));
        st.quota = QuotaState {
            active: true,
            reset_in,
            resume_at_ms: Some(resume_at),
            message: Some(message),
            retries,
        };
        let snap = st.quota.clone();
        self.cv.notify_all();
        snap
    }

    pub fn acquire<F>(self: &Arc<Self>, cancelled: F) -> Result<ModelCallPermit, String>
    where
        F: Fn() -> bool,
    {
        loop {
            if cancelled() {
                return Err("run or agent stopped".to_string());
            }
            let mut st = self.inner.lock().unwrap();
            if st.stopped {
                return Err("run or agent stopped".to_string());
            }
            if st.quota.active {
                let now = now_ms();
                if let Some(until) = st.quota.resume_at_ms {
                    if now < until {
                        let wait = Duration::from_millis((until - now).min(150));
                        let (g, _) = self.cv.wait_timeout(st, wait).unwrap();
                        drop(g);
                        continue;
                    }
                }
                // Backoff window elapsed: release gate for the retry probe.
                st.quota.active = false;
                st.quota.resume_at_ms = None;
            }
            if st.active_calls < st.max_concurrency {
                st.active_calls += 1;
                if st.active_calls > st.peak_concurrency {
                    st.peak_concurrency = st.active_calls;
                }
                return Ok(ModelCallPermit {
                    sched: Arc::clone(self),
                });
            }
            let (g, _) = self
                .cv
                .wait_timeout(st, Duration::from_millis(100))
                .unwrap();
            drop(g);
        }
    }
}

pub struct ModelCallPermit {
    sched: Arc<QuotaScheduler>,
}

impl Drop for ModelCallPermit {
    fn drop(&mut self) {
        let mut st = self.sched.inner.lock().unwrap();
        st.active_calls = st.active_calls.saturating_sub(1);
        self.sched.cv.notify_all();
    }
}

/// Parse an agy/OpenAI quota or 429 error into `(reset_phrase, backoff_ms)`.
pub fn parse_quota_error(err: &str) -> Option<(Option<String>, u64)> {
    let low = err.to_ascii_lowercase();
    let is_quota = low.contains("out of quota")
        || low.contains("quota reached")
        || low.contains("quota exceeded")
        || low.contains("rate limit")
        || low.contains("rate_limit")
        || has_http_status_token(&low, "429");
    if !is_quota {
        return None;
    }
    let phrase = extract_reset_phrase(err);
    let mut backoff_ms = phrase
        .as_deref()
        .and_then(parse_duration_ms)
        .unwrap_or(15_000);
    if let Ok(cap) = std::env::var("OPENRO_TEAMWORK_BACKOFF_CAP_MS") {
        if let Ok(cap_ms) = cap.trim().parse::<u64>() {
            backoff_ms = backoff_ms.min(cap_ms.max(10));
        }
    }
    Some((phrase, backoff_ms))
}

fn has_http_status_token(low: &str, code: &str) -> bool {
    let bytes = low.as_bytes();
    for (idx, _) in low.match_indices(code) {
        let before_ok = idx == 0
            || (!bytes[idx - 1].is_ascii_digit()
                && bytes[idx - 1] != b':'
                && bytes[idx - 1] != b'.');
        let after_idx = idx + code.len();
        let after_ok = after_idx >= bytes.len()
            || (!bytes[after_idx].is_ascii_digit() && bytes[after_idx] != b'.');
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// True when a non-quota model/CLI/HTTP failure looks transient and worth auto-retrying
/// (`empty conversation id`, `timed out`, `connection reset`, `502`/`503`).
pub fn is_transient_model_error(err: &str) -> bool {
    if parse_quota_error(err).is_some() {
        return false;
    }
    let low = err.to_ascii_lowercase();
    low.contains("empty conversation id")
        || low.contains("timed out")
        || low.contains("took longer than")
        || low.contains("connection reset")
        || low.contains("econnreset")
        || low.contains("etimedout")
        || low.contains("broken pipe")
        || low.contains("unexpected eof")
        || low.contains("bad gateway")
        || low.contains("service unavailable")
        || low.contains("gateway timeout")
        || has_http_status_token(&low, "502")
        || has_http_status_token(&low, "503")
        || has_http_status_token(&low, "504")
}

fn transient_retry_backoff_ms_with_cap(retry_idx: usize, cap_ms: Option<u64>) -> u64 {
    let mut ms = match retry_idx {
        0 => 5_000,
        _ => 15_000,
    };
    if let Some(c) = cap_ms {
        ms = ms.min(c.max(10));
    }
    ms
}

fn transient_retry_backoff_ms(retry_idx: usize) -> u64 {
    let cap = std::env::var("OPENRO_TEAMWORK_BACKOFF_CAP_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    transient_retry_backoff_ms_with_cap(retry_idx, cap)
}

fn wait_transient_backoff(
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
    backoff_ms: u64,
) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(backoff_ms);
    let mut st = handle.state.lock().unwrap();
    loop {
        if st.stop_requested || st.status != RunStatus::Running {
            return true;
        }
        let Some(idx) = st.agent_index(agent_name) else {
            return true;
        };
        if st.roster[idx].killed || st.roster[idx].generation != gen {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        let (guard, _) = handle
            .wake_cv
            .wait_timeout(st, deadline.saturating_duration_since(now))
            .unwrap();
        st = guard;
    }
}

fn extract_reset_phrase(raw: &str) -> Option<String> {
    let low = raw.to_ascii_lowercase();
    let idx = low.find("resets in")?;
    let after = raw[idx + "resets in".len()..].trim_start();
    let mut end = 0;
    for (i, ch) in after.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == ' ' {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    let tok = after[..end].trim();
    if tok.is_empty() {
        None
    } else {
        Some(tok.to_string())
    }
}

fn parse_duration_ms(phrase: &str) -> Option<u64> {
    let mut total_ms: u64 = 0;
    let mut matched = false;
    let bytes = phrase.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let num: u64 = phrase[start..i].parse().ok()?;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let unit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit = phrase[unit_start..i].to_ascii_lowercase();
        let mult = match unit.as_str() {
            "ms" | "msec" | "msecs" => 1,
            "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
            "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000,
            "d" | "day" | "days" => 86_400_000,
            _ => continue,
        };
        total_ms = total_ms.saturating_add(num.saturating_mul(mult));
        matched = true;
    }
    if matched { Some(total_ms) } else { None }
}

// ---------------------------------------------------------------------------
// Path jailing & normalization
// ---------------------------------------------------------------------------

pub fn normalize_rel_path(project_dir: &Path, raw: &str, for_write: bool) -> Result<String, String> {
    let replaced = raw.trim().replace('\\', "/");
    let trimmed = replaced.trim_start_matches("./");
    if trimmed.is_empty() || trimmed == "." {
        return Err("path must not be empty".to_string());
    }
    if trimmed.contains('\0') {
        return Err("path must not contain NUL bytes".to_string());
    }
    let p = Path::new(trimmed);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("path '{raw}' must stay inside the project folder"));
    }
    let segs: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.is_empty() {
        return Err("path must not be empty".to_string());
    }
    let norm = segs.join("/");
    if segs[0] == ".git" || segs.contains(&".git") {
        return Err("the .git folder is off limits".to_string());
    }
    if segs[0] == ".openro" {
        return Err("the .openro folder is managed by OpenRO".to_string());
    }
    if for_write {
        if segs[0] == "build" {
            return Err("build/ is OpenRO's output folder; it cannot be edited as source".to_string());
        }
        if norm == "openro.project.json" || norm.ends_with(".lock.json") {
            return Err(format!("{norm} is managed by OpenRO and cannot be overwritten directly"));
        }
    }
    // Check symlink containment on the deepest existing ancestor.
    let root_canon = project_dir
        .canonicalize()
        .map_err(|e| format!("project folder {}: {e}", project_dir.display()))?;
    let joined = project_dir.join(&norm);
    let mut probe = joined.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let canon = probe
        .canonicalize()
        .map_err(|e| format!("path '{norm}': {e}"))?;
    if !canon.starts_with(&root_canon) {
        return Err(format!("path '{raw}' escapes the project folder"));
    }
    Ok(norm)
}

fn sanitize_agent_name(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty()
        || s.len() > 32
        || s == "all"
        || s == "human"
        || s == "system"
        || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        || !s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return Err(format!("invalid agent name '{raw}'"));
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// Per-run state + persistence
// ---------------------------------------------------------------------------

pub struct RunState {
    pub id: String,
    pub goal: String,
    pub project_id: Option<String>,
    pub project_dir: PathBuf,
    pub storage_dir: PathBuf,
    pub status: RunStatus,
    pub model_id: String,
    pub provider: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub board: Board,
    pub roster: Vec<AgentRecord>,
    pub tasks: Vec<Task>,
    pub claims: BTreeMap<String, FileClaim>,
    pub next_task_num: u64,
    pub events: VecDeque<TeamEvent>,
    pub next_event_seq: u64,
    pub stop_requested: bool,
}

impl RunState {
    fn agent_index(&self, name: &str) -> Option<usize> {
        self.roster
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
    }

    fn task_index(&self, id: &str) -> Option<usize> {
        self.tasks.iter().position(|t| t.id == id)
    }

    fn emit_event(&mut self, kind: &str, payload: Value) {
        let seq = self.next_event_seq;
        self.next_event_seq += 1;
        if self.events.len() >= MAX_SSE_RING {
            self.events.pop_front();
        }
        let mut p = payload;
        if let Some(o) = p.as_object_mut() {
            o.insert("event_seq".into(), json!(seq));
            o.insert("run_id".into(), json!(self.id));
        }
        self.events.push_back(TeamEvent {
            event_seq: seq,
            ts_ms: now_ms(),
            event_kind: kind.to_string(),
            payload: p,
        });
    }

    fn append_board(
        &mut self,
        author: &str,
        kind: BoardKind,
        text: &str,
        task_id: Option<String>,
    ) -> Result<BoardMessage, String> {
        let msg = self.board.append(author, kind, text, task_id)?;
        if let Some(idx) = self.agent_index(author) {
            self.roster[idx].last_post_seq = Some(msg.seq);
        }
        self.updated_at_ms = msg.ts_ms;
        self.emit_event("board", json!({ "ts_ms": msg.ts_ms, "message": msg.to_json() }));
        Ok(msg)
    }

    fn append_trace(&mut self, agent_name: &str, turn: u64, kind: &str, summary: &str, data: Value) {
        let Some(idx) = self.agent_index(agent_name) else { return };
        let seq = self.roster[idx].trace_count + 1;
        self.roster[idx].trace_count = seq;
        let entry = TraceEntry {
            seq,
            ts_ms: now_ms(),
            turn,
            kind: kind.to_string(),
            summary: summary.to_string(),
            data,
        };
        let trace_path = self.storage_dir.join("traces").join(format!("{}.jsonl", self.roster[idx].name));
        if let Some(parent) = trace_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&trace_path) {
            let _ = writeln!(f, "{}", entry.to_json());
            let _ = f.flush();
        }
        self.emit_event(
            "trace",
            json!({
                "ts_ms": entry.ts_ms,
                "agent": self.roster[idx].name,
                "trace": entry.to_json(),
            }),
        );
    }

    fn save_agent_file(&self, agent_name: &str) {
        let Some(idx) = self.agent_index(agent_name) else { return };
        let a = &self.roster[idx];
        let path = self.storage_dir.join("agents").join(format!("{}.json", a.name));
        let _ = write_json_atomic(&path, &a.to_persisted_json());
    }

    fn save_tasks_file(&self) {
        let path = self.storage_dir.join("tasks.json");
        let val = json!({
            "next_task_num": self.next_task_num,
            "tasks": self.tasks.iter().map(Task::to_persisted_json).collect::<Vec<_>>(),
            "claims": self.claims.values().map(FileClaim::to_json).collect::<Vec<_>>(),
        });
        let _ = write_json_atomic(&path, &val);
    }

    fn save_run_file(&self, max_concurrency: usize, quota: &QuotaState) {
        let path = self.storage_dir.join("run.json");
        let val = json!({
            "id": self.id,
            "goal": self.goal,
            "project_id": self.project_id,
            "project_dir": self.project_dir.to_string_lossy(),
            "storage_dir": self.storage_dir.to_string_lossy(),
            "status": self.status.as_str(),
            "model_id": self.model_id,
            "provider": self.provider,
            "max_concurrency": max_concurrency,
            "quota": quota.to_json(),
            "roster_order": self.roster.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            "created_at_ms": self.created_at_ms,
            "updated_at_ms": self.updated_at_ms,
        });
        let _ = write_json_atomic(&path, &val);
    }

    fn set_agent_state(&mut self, name: &str, new_state: AgentWorkState) {
        let Some(idx) = self.agent_index(name) else { return };
        if self.roster[idx].state != new_state {
            self.roster[idx].state = new_state;
            self.roster[idx].updated_at_ms = now_ms();
            let snap = self.roster[idx].to_json();
            self.emit_event("roster", json!({ "ts_ms": now_ms(), "agent": snap }));
        }
    }

    fn deps_satisfied(&self, task: &Task) -> bool {
        task.depends_on.iter().all(|dep_id| {
            self.tasks
                .iter()
                .find(|t| t.id == *dep_id)
                .is_some_and(|t| t.status == TaskStatus::Done)
        })
    }

    fn agent_has_work(&self, idx: usize) -> bool {
        let a = &self.roster[idx];
        if a.killed || self.status != RunStatus::Running {
            return false;
        }
        if self.board.mentions_for_since(&a.name, a.cursor_seq) {
            return true;
        }
        if a.parked_at_seq.is_some_and(|p| p >= self.board.latest_seq()) {
            return false;
        }
        if a.role == "verifier" {
            return self.tasks.iter().any(|t| {
                t.status == TaskStatus::AwaitingVerification
                    && t.verifier.eq_ignore_ascii_case(&a.name)
            });
        }
        if a.role == "coordinator" {
            let all_done = !self.tasks.is_empty()
                && self
                    .tasks
                    .iter()
                    .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled));
            return all_done;
        }
        self.tasks.iter().any(|t| {
            t.owner.eq_ignore_ascii_case(&a.name)
                && matches!(
                    t.status,
                    TaskStatus::InProgress | TaskStatus::ChangesRequested | TaskStatus::Pending
                )
                && self.deps_satisfied(t)
        })
    }

    fn summary_json(
        &self,
        max_concurrency: usize,
        active_calls: usize,
        quota: &QuotaState,
    ) -> Value {
        let tasks_done = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        json!({
            "id": self.id,
            "goal": self.goal,
            "project_id": self.project_id,
            "project_dir": self.project_dir.to_string_lossy(),
            "storage_dir": self.storage_dir.to_string_lossy(),
            "status": self.status.as_str(),
            "model_id": self.model_id,
            "provider": self.provider,
            "max_concurrency": max_concurrency,
            "active_model_calls": active_calls,
            "board_seq": self.board.latest_seq(),
            "tasks_total": self.tasks.len(),
            "tasks_done": tasks_done,
            "agents": self.roster.iter().map(|a| json!({
                "name": a.name,
                "role": a.role,
                "state": a.state.as_str(),
                "killed": a.killed,
            })).collect::<Vec<_>>(),
            "quota": quota.to_json(),
            "created_at_ms": self.created_at_ms,
            "updated_at_ms": self.updated_at_ms,
        })
    }

    fn detail_json(
        &self,
        max_concurrency: usize,
        active_calls: usize,
        peak_concurrency: usize,
        quota: &QuotaState,
        studio_lease: Option<&StudioLeaseInfo>,
        since_board_seq: u64,
    ) -> Value {
        let board_items: Vec<Value> = if since_board_seq > 0 {
            self.board
                .since(since_board_seq)
                .into_iter()
                .map(|m| m.to_json())
                .collect()
        } else {
            self.board.all().iter().map(BoardMessage::to_json).collect()
        };
        json!({
            "run": {
                "id": self.id,
                "goal": self.goal,
                "project_id": self.project_id,
                "project_dir": self.project_dir.to_string_lossy(),
                "storage_dir": self.storage_dir.to_string_lossy(),
                "status": self.status.as_str(),
                "model_id": self.model_id,
                "provider": self.provider,
                "max_concurrency": max_concurrency,
                "active_model_calls": active_calls,
                "peak_concurrency": peak_concurrency,
                "board_seq": self.board.latest_seq(),
                "studio_lease": studio_lease.map(StudioLeaseInfo::to_json),
                "quota": quota.to_json(),
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            },
            "roster": self.roster.iter().map(AgentRecord::to_json).collect::<Vec<_>>(),
            "tasks": self.tasks.iter().map(Task::to_json).collect::<Vec<_>>(),
            "claims": self.claims.values().map(FileClaim::to_json).collect::<Vec<_>>(),
            "board": board_items,
        })
    }
}

pub struct RunHandle {
    pub state: Mutex<RunState>,
    pub wake_cv: Condvar,
    pub sse_cv: Condvar,
    pub scheduler: Arc<QuotaScheduler>,
    pub sse_clients: AtomicUsize,
}

impl RunHandle {
    fn notify_all(&self) {
        self.wake_cv.notify_all();
        self.sse_cv.notify_all();
    }
}

// ---------------------------------------------------------------------------
// TeamworkManager (owned by Shared)
// ---------------------------------------------------------------------------

pub struct TeamworkManager {
    data_dir: Option<PathBuf>,
    runs: Mutex<BTreeMap<String, Arc<RunHandle>>>,
    studio_lease: Arc<StudioLeaseLock>,
    next_run_seq: std::sync::atomic::AtomicU64,
}

impl TeamworkManager {
    pub fn in_memory() -> Arc<Self> {
        Arc::new(TeamworkManager {
            data_dir: None,
            runs: Mutex::new(BTreeMap::new()),
            studio_lease: Arc::new(StudioLeaseLock::new()),
            next_run_seq: std::sync::atomic::AtomicU64::new(1),
        })
    }

    pub fn open(data_dir: PathBuf) -> Arc<Self> {
        let mgr = Arc::new(TeamworkManager {
            data_dir: Some(data_dir),
            runs: Mutex::new(BTreeMap::new()),
            studio_lease: Arc::new(StudioLeaseLock::new()),
            next_run_seq: std::sync::atomic::AtomicU64::new(1),
        });
        mgr.load_existing();
        mgr
    }

    pub fn studio_lease(&self) -> Option<StudioLeaseInfo> {
        self.studio_lease.current()
    }

    pub fn shutdown(&self) {
        let handles: Vec<Arc<RunHandle>> = self.runs.lock().unwrap().values().cloned().collect();
        for h in handles {
            {
                let mut st = h.state.lock().unwrap();
                st.stop_requested = true;
            }
            h.scheduler.set_stopped(true);
            h.notify_all();
        }
    }

    fn load_existing(&self) {
        let Some(data_dir) = &self.data_dir else { return };
        let mut storage_dirs = BTreeSet::<PathBuf>::new();
        // 1. Persisted run index
        let idx_path = data_dir.join("teamwork-runs.json");
        if let Ok(text) = std::fs::read_to_string(&idx_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(arr) = v.get("runs").and_then(|x| x.as_array()) {
                    for item in arr {
                        if let Some(p) = item.as_str() {
                            storage_dirs.insert(PathBuf::from(p));
                        }
                    }
                }
            }
        }
        // 2. Scan default project storage and known workspace roots
        let mut scan_roots = vec![data_dir.join("teamwork-default")];
        if let Ok(text) = std::fs::read_to_string(data_dir.join("projects.json")) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                for key in ["opened", "roots"] {
                    if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
                        for p in arr.iter().filter_map(|x| x.as_str()) {
                            let pb = PathBuf::from(p);
                            scan_roots.push(pb.clone());
                            if let Ok(entries) = std::fs::read_dir(&pb) {
                                for e in entries.flatten() {
                                    if e.path().is_dir() {
                                        scan_roots.push(e.path());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Ok(ws) = std::env::var("OPENRO_WORKSPACE") {
            for root in ws.split(':').filter(|s| !s.trim().is_empty()) {
                let pb = PathBuf::from(root);
                scan_roots.push(pb.clone());
                if let Ok(entries) = std::fs::read_dir(&pb) {
                    for e in entries.flatten() {
                        if e.path().is_dir() {
                            scan_roots.push(e.path());
                        }
                    }
                }
            }
        }
        for root in scan_roots {
            let tw_root = root.join(".openro/teamwork");
            if let Ok(entries) = std::fs::read_dir(&tw_root) {
                for e in entries.flatten() {
                    if e.path().is_dir() && e.path().join("run.json").is_file() {
                        storage_dirs.insert(e.path());
                    }
                }
            }
        }

        let mut loaded = self.runs.lock().unwrap();
        for sdir in storage_dirs {
            if !sdir.join("run.json").is_file() {
                continue;
            }
            if let Ok((handle, _was_running)) = load_run_from_dir(&sdir) {
                let id = handle.state.lock().unwrap().id.clone();
                loaded.insert(id, handle);
            }
        }
    }

    /// Resume any runs loaded from disk whose status was `running` before restart.
    pub fn resume_pending_runs(self: &Arc<Self>, shared: &Arc<Shared>) {
        if std::env::var("OPENRO_TEAMWORK_NO_AUTORESUME")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            return;
        }
        let handles: Vec<Arc<RunHandle>> = self.runs.lock().unwrap().values().cloned().collect();
        for h in handles {
            let should_resume = {
                let mut st = h.state.lock().unwrap();
                if st.status == RunStatus::Running {
                    st.stop_requested = false;
                    let _ = st.append_board(
                        "system",
                        BoardKind::System,
                        "Resumed teamwork run after OpenRO restart.",
                        None,
                    );
                    let max_c = h.scheduler.snapshot().0;
                    let q = h.scheduler.snapshot().3;
                    st.save_run_file(max_c, &q);
                    true
                } else {
                    false
                }
            };
            if should_resume {
                h.scheduler.set_stopped(false);
                spawn_all_workers(shared, self, &h);
            }
        }
    }

    fn save_index(&self) {
        let Some(data_dir) = &self.data_dir else { return };
        let runs = self.runs.lock().unwrap();
        let dirs: Vec<String> = runs
            .values()
            .map(|h| h.state.lock().unwrap().storage_dir.to_string_lossy().into_owned())
            .collect();
        let _ = write_json_atomic(&data_dir.join("teamwork-runs.json"), &json!({ "runs": dirs }));
    }

    pub fn get(&self, id: &str) -> Option<Arc<RunHandle>> {
        self.runs.lock().unwrap().get(id).cloned()
    }
}

fn load_run_from_dir(storage_dir: &Path) -> Result<(Arc<RunHandle>, bool), String> {
    let run_text = std::fs::read_to_string(storage_dir.join("run.json"))
        .map_err(|e| format!("read run.json: {e}"))?;
    let rv: Value = serde_json::from_str(&run_text).map_err(|e| format!("parse run.json: {e}"))?;
    let id = rv.get("id").and_then(|v| v.as_str()).ok_or("run.json missing id")?.to_string();
    let goal = rv.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let project_id = rv.get("project_id").and_then(|v| v.as_str()).map(String::from);
    let project_dir = rv
        .get("project_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or("run.json missing project_dir")?;
    let status = rv
        .get("status")
        .and_then(|v| v.as_str())
        .map(RunStatus::parse)
        .unwrap_or(RunStatus::Stopped);
    let model_id = rv
        .get("model_id")
        .and_then(|v| v.as_str())
        .unwrap_or(AGY_DEFAULT_MODEL)
        .to_string();
    let provider = rv
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("antigravity")
        .to_string();
    let max_concurrency = rv
        .get("max_concurrency")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_CONCURRENCY);
    let created_at_ms = rv.get("created_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let updated_at_ms = rv.get("updated_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);

    let board = Board::open(storage_dir.join("board.jsonl"))?;

    let (tasks, claims, next_task_num) = if let Ok(tt) = std::fs::read_to_string(storage_dir.join("tasks.json")) {
        if let Ok(tv) = serde_json::from_str::<Value>(&tt) {
            let tasks: Vec<Task> = tv
                .get("tasks")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(Task::from_json).collect())
                .unwrap_or_default();
            let mut claims = BTreeMap::new();
            if let Some(arr) = tv.get("claims").and_then(|x| x.as_array()) {
                for c in arr.iter().filter_map(FileClaim::from_json) {
                    claims.insert(c.path.clone(), c);
                }
            }
            let next_num = tv.get("next_task_num").and_then(|x| x.as_u64()).unwrap_or(tasks.len() as u64 + 1);
            (tasks, claims, next_num)
        } else {
            (Vec::new(), BTreeMap::new(), 1)
        }
    } else {
        (Vec::new(), BTreeMap::new(), 1)
    };

    let mut roster = Vec::new();
    let order: Vec<String> = rv
        .get("roster_order")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let agents_dir = storage_dir.join("agents");
    let mut loaded_map: HashMap<String, AgentRecord> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(txt) = std::fs::read_to_string(e.path()) {
                    if let Ok(av) = serde_json::from_str::<Value>(&txt) {
                        if let Some(rec) = AgentRecord::from_persisted_json(&av) {
                            loaded_map.insert(rec.name.clone(), rec);
                        }
                    }
                }
            }
        }
    }
    for name in order {
        if let Some(rec) = loaded_map.remove(&name) {
            roster.push(rec);
        }
    }
    let mut rest: Vec<AgentRecord> = loaded_map.into_values().collect();
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    roster.extend(rest);

    let scheduler = Arc::new(QuotaScheduler::new(max_concurrency));
    let was_running = status == RunStatus::Running;
    let handle = Arc::new(RunHandle {
        state: Mutex::new(RunState {
            id,
            goal,
            project_id,
            project_dir,
            storage_dir: storage_dir.to_path_buf(),
            status,
            model_id,
            provider,
            created_at_ms,
            updated_at_ms,
            board,
            roster,
            tasks,
            claims,
            next_task_num,
            events: VecDeque::with_capacity(MAX_SSE_RING),
            next_event_seq: 1,
            stop_requested: false,
        }),
        wake_cv: Condvar::new(),
        sse_cv: Condvar::new(),
        scheduler,
        sse_clients: AtomicUsize::new(0),
    });
    Ok((handle, was_running))
}

fn write_json_atomic(path: &Path, val: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("openro-tmp");
    let text = serde_json::to_string_pretty(val).map_err(|e| format!("encode json: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// HTTP Routing & SSE
// ---------------------------------------------------------------------------

pub struct TeamworkReply {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl TeamworkReply {
    fn json(status: &'static str, mut v: Value) -> Self {
        if let Some(o) = v.as_object_mut() {
            o.entry("ok").or_insert(json!(true));
        }
        TeamworkReply {
            status,
            content_type: "application/json",
            body: v.to_string().into_bytes(),
        }
    }

    fn err(status: &'static str, msg: impl Into<String>) -> Self {
        TeamworkReply {
            status,
            content_type: "application/json",
            body: json!({ "ok": false, "error": msg.into() })
                .to_string()
                .into_bytes(),
        }
    }
}

fn parse_query_u64(query: &str, key: &str) -> Option<u64> {
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse::<u64>().ok())
}

fn parse_query_str(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
}

/// True when `path` matches `GET /api/teamwork/:id/stream`.
pub fn is_sse_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/teamwork/") else {
        return false;
    };
    let Some(run_id) = rest.strip_suffix("/stream") else {
        return false;
    };
    !run_id.is_empty() && !run_id.contains('/')
}

/// Serve `GET /api/teamwork/:id/stream` (SSE). Returns `true` if handled.
pub fn try_serve_sse(
    stream: TcpStream,
    shared: &Arc<Shared>,
    path: &str,
    query: &str,
    last_event_id: u64,
    loopback: bool,
) -> bool {
    let Some(rest) = path.strip_prefix("/api/teamwork/") else {
        return false;
    };
    let Some(run_id) = rest.strip_suffix("/stream") else {
        return false;
    };
    if run_id.is_empty() || run_id.contains('/') {
        return false;
    }
    let mut stream = stream;
    if !loopback {
        let msg = json!({ "ok": false, "error": "teamwork streams are loopback only" }).to_string();
        let resp = format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
            msg.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        return true;
    }
    let Some(handle) = shared.teamwork.get(run_id) else {
        let msg = json!({ "ok": false, "error": format!("unknown teamwork run '{run_id}'") }).to_string();
        let resp = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
            msg.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        return true;
    };
    let since = if last_event_id > 0 {
        last_event_id
    } else {
        parse_query_u64(query, "since").unwrap_or(0)
    };
    let _ = serve_sse_stream(&shared.teamwork, &handle, stream, since);
    true
}

struct SseClientGuard<'a>(&'a AtomicUsize);
impl Drop for SseClientGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve_sse_stream(
    mgr: &TeamworkManager,
    handle: &Arc<RunHandle>,
    mut stream: TcpStream,
    since_event: u64,
) -> Result<(), String> {
    let prev = handle.sse_clients.fetch_add(1, Ordering::SeqCst);
    if prev >= MAX_TEAMWORK_SSE_CLIENTS {
        handle.sse_clients.fetch_sub(1, Ordering::SeqCst);
        let body = json!({ "ok": false, "error": "Too many teamwork SSE streams open." }).to_string();
        let hdr = format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(hdr.as_bytes());
        let _ = stream.flush();
        return Err("too many SSE clients".to_string());
    }
    let _guard = SseClientGuard(&handle.sse_clients);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_nodelay(true);

    let (mut cursor, init_json, missed) = {
        let st = handle.state.lock().unwrap();
        let (max_c, active_c, _, quota) = handle.scheduler.snapshot();
        let lease = mgr.studio_lease();
        let recent: Vec<Value> = st
            .events
            .iter()
            .rev()
            .take(20)
            .rev()
            .map(|e| json!({ "event_seq": e.event_seq, "event": e.event_kind, "data": e.payload }))
            .collect();
        let missed: Vec<TeamEvent> = if since_event > 0 {
            st.events
                .iter()
                .filter(|e| e.event_seq > since_event)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let head = st.events.back().map(|e| e.event_seq).unwrap_or(0);
        let init = json!({
            "run_id": st.id,
            "status": st.status.as_str(),
            "event_seq": head,
            "board_seq": st.board.latest_seq(),
            "max_concurrency": max_c,
            "active_model_calls": active_c,
            "roster": st.roster.iter().map(AgentRecord::to_json).collect::<Vec<_>>(),
            "tasks": st.tasks.iter().map(Task::to_json).collect::<Vec<_>>(),
            "quota": quota.to_json(),
            "studio_lease": lease.as_ref().map(StudioLeaseInfo::to_json),
            "recent": recent,
        });
        (head, init, missed)
    };

    let headers = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream; charset=utf-8\r\n",
        "Cache-Control: no-cache, no-transform\r\n",
        "Connection: keep-alive\r\n",
        "X-Accel-Buffering: no\r\n\r\n"
    );
    let init_frame = format!("{headers}event: status\ndata: {init_json}\n\n");
    if stream.write_all(init_frame.as_bytes()).is_err() || stream.flush().is_err() {
        return Ok(());
    }
    for ev in missed {
        if write_team_sse_frame(&mut stream, &ev).is_err() {
            return Ok(());
        }
        if ev.event_seq > cursor {
            cursor = ev.event_seq;
        }
    }

    loop {
        let batch: Vec<TeamEvent> = {
            let mut st = handle.state.lock().unwrap();
            let head = st.events.back().map(|e| e.event_seq).unwrap_or(0);
            if head <= cursor && !st.stop_requested {
                let (g, _) = handle.sse_cv.wait_timeout(st, KEEPALIVE_INTERVAL).unwrap();
                st = g;
            }
            let list: Vec<TeamEvent> = st
                .events
                .iter()
                .filter(|e| e.event_seq > cursor)
                .cloned()
                .collect();
            if list.is_empty() && st.stop_requested {
                return Ok(());
            }
            list
        };
        if batch.is_empty() {
            if stream.write_all(b": keepalive\n\n").is_err() || stream.flush().is_err() {
                return Ok(());
            }
            continue;
        }
        for ev in batch {
            if write_team_sse_frame(&mut stream, &ev).is_err() {
                return Ok(());
            }
            cursor = ev.event_seq;
        }
    }
}

fn write_team_sse_frame(stream: &mut TcpStream, ev: &TeamEvent) -> std::io::Result<()> {
    let frame = format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        ev.event_seq, ev.event_kind, ev.payload
    );
    stream.write_all(frame.as_bytes())?;
    stream.flush()
}

/// JSON API router for `/api/teamwork*`. Returns `None` for other paths.
pub fn route(
    shared: &Arc<Shared>,
    method: &str,
    full_path: &str,
    body: &[u8],
    loopback: bool,
) -> Option<TeamworkReply> {
    let (path, query) = full_path.split_once('?').unwrap_or((full_path, ""));
    let rest = path.strip_prefix("/api/teamwork")?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let owned = matches!(
        (method, segs.as_slice()),
        ("POST", [])
            | ("GET", [])
            | ("GET", [_])
            | ("POST", [_, "post" | "stop" | "resume"])
            | ("POST", [_, "agents", _, "kill" | "restart"])
            | ("GET", [_, "agents", _, "trace"])
    );
    if !owned {
        return None;
    }
    if !loopback {
        return Some(TeamworkReply::err(
            "403 Forbidden",
            "teamwork endpoints are loopback only",
        ));
    }
    let parsed_body = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(body) {
            Ok(v) => v,
            Err(_) => {
                return Some(TeamworkReply::err(
                    "400 Bad Request",
                    "request body is not valid JSON",
                ))
            }
        }
    };
    Some(dispatch(shared, method, &segs, query, &parsed_body))
}

fn dispatch(
    shared: &Arc<Shared>,
    method: &str,
    segs: &[&str],
    query: &str,
    body: &Value,
) -> TeamworkReply {
    match (method, segs) {
        ("POST", []) => create_run(shared, body),
        ("GET", []) => list_runs(shared, query),
        ("GET", [id]) => get_run(shared, id, parse_query_u64(query, "since").unwrap_or(0)),
        ("POST", [id, "post"]) => human_post(shared, id, body),
        ("POST", [id, "stop"]) => stop_run(shared, id),
        ("POST", [id, "resume"]) => resume_run(shared, id),
        ("POST", [id, "agents", name, "kill"]) => kill_agent(shared, id, name, body),
        ("POST", [id, "agents", name, "restart"]) => restart_agent(shared, id, name, body),
        ("GET", [id, "agents", name, "trace"]) => {
            get_agent_trace(shared, id, name, parse_query_u64(query, "since").unwrap_or(0))
        }
        _ => TeamworkReply::err("404 Not Found", "not found"),
    }
}

fn create_run(shared: &Arc<Shared>, body: &Value) -> TeamworkReply {
    let goal = body
        .get("goal")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(goal) = goal else {
        return TeamworkReply::err("400 Bad Request", "teamwork run requires a non-empty goal");
    };
    let max_concurrency = body
        .get("max_concurrency")
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1, 16))
        .unwrap_or(DEFAULT_MAX_CONCURRENCY);

    let (project_id, project_dir) = match resolve_project_target(shared, body.get("project_id").and_then(|v| v.as_str())) {
        Ok(p) => p,
        Err(e) => return TeamworkReply::err("400 Bad Request", e),
    };

    let model_client = resolve_model_client(shared, body.get("model_id").and_then(|v| v.as_str()));
    if !model_client.is_configured() {
        return TeamworkReply::err(
            "503 Service Unavailable",
            "No model is configured for Teamwork. Set Gemini 4 Hydrogen (agy) or an API key in Settings.",
        );
    }

    let seq = shared
        .teamwork
        .next_run_seq
        .fetch_add(1, Ordering::SeqCst);
    let ts = now_ms();
    let run_id = format!("team-{ts}-{seq}");
    let storage_dir = project_dir.join(".openro/teamwork").join(&run_id);
    if let Err(e) = std::fs::create_dir_all(storage_dir.join("agents"))
        .and_then(|_| std::fs::create_dir_all(storage_dir.join("traces")))
    {
        return TeamworkReply::err("500 Internal Server Error", format!("create teamwork dir: {e}"));
    }

    let roster = match build_initial_roster(&run_id, body.get("roles"), model_client.model_id()) {
        Ok(r) => r,
        Err(e) => return TeamworkReply::err("400 Bad Request", e),
    };

    let mut board = match Board::open(storage_dir.join("board.jsonl")) {
        Ok(b) => b,
        Err(e) => return TeamworkReply::err("500 Internal Server Error", e),
    };
    if let Err(e) = board.append(
        "human",
        BoardKind::Goal,
        &format!("@coordinator Goal: {goal}"),
        None,
    ) {
        return TeamworkReply::err("500 Internal Server Error", e);
    }

    let scheduler = Arc::new(QuotaScheduler::new(max_concurrency));
    let mut state = RunState {
        id: run_id.clone(),
        goal: goal.to_string(),
        project_id,
        project_dir,
        storage_dir,
        status: RunStatus::Running,
        model_id: model_client.model_id().to_string(),
        provider: model_client.provider().to_string(),
        created_at_ms: ts,
        updated_at_ms: ts,
        board,
        roster,
        tasks: Vec::new(),
        claims: BTreeMap::new(),
        next_task_num: 1,
        events: VecDeque::with_capacity(MAX_SSE_RING),
        next_event_seq: 1,
        stop_requested: false,
    };
    let q_snap = QuotaState::default();
    state.save_tasks_file();
    for a in &state.roster {
        let p = state.storage_dir.join("agents").join(format!("{}.json", a.name));
        let _ = write_json_atomic(&p, &a.to_persisted_json());
    }
    state.save_run_file(max_concurrency, &q_snap);
    let summary = state.summary_json(max_concurrency, 0, &q_snap);
    state.emit_event("run", json!({ "ts_ms": ts, "status": "running", "run": summary }));

    let handle = Arc::new(RunHandle {
        state: Mutex::new(state),
        wake_cv: Condvar::new(),
        sse_cv: Condvar::new(),
        scheduler,
        sse_clients: AtomicUsize::new(0),
    });
    shared
        .teamwork
        .runs
        .lock()
        .unwrap()
        .insert(run_id, Arc::clone(&handle));
    shared.teamwork.save_index();

    spawn_all_workers(shared, &shared.teamwork, &handle);

    TeamworkReply::json("201 Created", json!({ "run": summary }))
}

fn build_initial_roster(
    run_id: &str,
    roles_val: Option<&Value>,
    default_model: &str,
) -> Result<Vec<AgentRecord>, String> {
    let ts = now_ms();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    {
        let mut push_agent = |name: String, role: String, brief: Option<String>, model: String| {
            if !seen.insert(name.clone()) {
                return;
            }
            out.push(AgentRecord {
                name: name.clone(),
                role,
                brief,
                model_id: model,
                state: AgentWorkState::Waiting,
                killed: false,
                chat_id: format!("team-{run_id}-{name}"),
                cursor_seq: 0,
                turns: 0,
                trace_count: 0,
                last_post_seq: None,
                current_task_id: None,
                last_error: None,
                updated_at_ms: ts,
                generation: 1,
                parked_at_seq: None,
                messages: Vec::new(),
            });
        };

        push_agent(
            "coordinator".into(),
            "coordinator".into(),
            None,
            default_model.to_string(),
        );

        if let Some(arr) = roles_val.and_then(|v| v.as_array()) {
            for item in arr {
                match item {
                    Value::String(s) => {
                        let n = sanitize_agent_name(s)?;
                        if n != "coordinator" {
                            push_agent(n.clone(), n, None, default_model.to_string());
                        }
                    }
                    Value::Object(obj) => {
                        let raw_name = obj
                            .get("name")
                            .or_else(|| obj.get("role"))
                            .and_then(|v| v.as_str())
                            .ok_or("role object requires name or role")?;
                        let n = sanitize_agent_name(raw_name)?;
                        let role = obj
                            .get("role")
                            .and_then(|v| v.as_str())
                            .map(|r| r.trim().to_ascii_lowercase())
                            .filter(|r| !r.is_empty())
                            .unwrap_or_else(|| n.clone());
                        let brief = obj.get("brief").and_then(|v| v.as_str()).map(String::from);
                        let model = obj
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .unwrap_or(default_model)
                            .to_string();
                        if n != "coordinator" {
                            push_agent(n, role, brief, model);
                        }
                    }
                    _ => return Err("roles entries must be strings or objects".to_string()),
                }
            }
        } else {
            for r in DEFAULT_ROLES {
                push_agent(r.into(), r.into(), None, default_model.to_string());
            }
        }
    }
    if !out.iter().any(|a| a.role == "verifier") {
        let name = "verifier".to_string();
        out.push(AgentRecord {
            name: name.clone(),
            role: "verifier".into(),
            brief: None,
            model_id: default_model.to_string(),
            state: AgentWorkState::Waiting,
            killed: false,
            chat_id: format!("team-{run_id}-{name}"),
            cursor_seq: 0,
            turns: 0,
            trace_count: 0,
            last_post_seq: None,
            current_task_id: None,
            last_error: None,
            updated_at_ms: ts,
            generation: 1,
            parked_at_seq: None,
            messages: Vec::new(),
        });
    }
    Ok(out)
}

fn list_runs(shared: &Arc<Shared>, query: &str) -> TeamworkReply {
    let filter_proj = parse_query_str(query, "project_id");
    let handles: Vec<Arc<RunHandle>> = shared.teamwork.runs.lock().unwrap().values().cloned().collect();
    let mut items: Vec<(u64, Value)> = Vec::new();
    for h in handles {
        let st = h.state.lock().unwrap();
        if let Some(pid) = &filter_proj {
            if st.project_id.as_deref() != Some(pid.as_str()) {
                continue;
            }
        }
        let (max_c, active_c, _, quota) = h.scheduler.snapshot();
        items.push((st.created_at_ms, st.summary_json(max_c, active_c, &quota)));
    }
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let runs: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();
    TeamworkReply::json("200 OK", json!({ "runs": runs }))
}

fn get_run(shared: &Arc<Shared>, id: &str, since_seq: u64) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let (max_c, active_c, peak_c, quota) = h.scheduler.snapshot();
    let lease = shared.teamwork.studio_lease();
    let st = h.state.lock().unwrap();
    TeamworkReply::json(
        "200 OK",
        st.detail_json(max_c, active_c, peak_c, &quota, lease.as_ref(), since_seq),
    )
}

fn human_post(shared: &Arc<Shared>, id: &str, body: &Value) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let text = body
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(text) = text else {
        return TeamworkReply::err("400 Bad Request", "post requires non-empty text");
    };
    let author = body
        .get("author")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("human");

    let mut need_spawn = false;
    let (msg, woken) = {
        let mut st = h.state.lock().unwrap();
        let msg = match st.append_board(author, BoardKind::Human, text, None) {
            Ok(m) => m,
            Err(e) => return TeamworkReply::err("500 Internal Server Error", e),
        };
        if st.status == RunStatus::Completed && !msg.mentions.is_empty() {
            st.status = RunStatus::Running;
            st.stop_requested = false;
            need_spawn = true;
            let (max_c, _, _, q) = h.scheduler.snapshot();
            st.save_run_file(max_c, &q);
        }
        let woken: Vec<String> = if st.status == RunStatus::Running {
            st.roster
                .iter()
                .filter(|a| {
                    !a.killed
                        && (msg.mentions.iter().any(|m| m == &a.name || m == "all"))
                })
                .map(|a| a.name.clone())
                .collect()
        } else {
            Vec::new()
        };
        (msg, woken)
    };
    if need_spawn {
        h.scheduler.set_stopped(false);
        spawn_all_workers(shared, &shared.teamwork, &h);
    }
    h.notify_all();
    TeamworkReply::json(
        "201 Created",
        json!({ "message": msg.to_json(), "woken": woken }),
    )
}

fn stop_run(shared: &Arc<Shared>, id: &str) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let summary = {
        let mut st = h.state.lock().unwrap();
        let ts = now_ms();
        st.stop_requested = true;
        st.status = RunStatus::Stopped;
        st.updated_at_ms = ts;
        for a in st.roster.iter_mut() {
            a.generation += 1;
            a.state = AgentWorkState::Stopped;
            a.updated_at_ms = ts;
        }
        let names: Vec<String> = st.roster.iter().map(|a| a.name.clone()).collect();
        for n in names {
            st.save_agent_file(&n);
        }
        let _ = st.append_board("system", BoardKind::System, "Run stopped by owner.", None);
        let (max_c, active_c, _, q) = h.scheduler.snapshot();
        st.save_run_file(max_c, &q);
        let sum = st.summary_json(max_c, active_c, &q);
        st.emit_event("run", json!({ "ts_ms": ts, "status": "stopped", "run": sum.clone() }));
        sum
    };
    h.scheduler.set_stopped(true);
    h.notify_all();
    TeamworkReply::json("200 OK", json!({ "run": summary }))
}

fn resume_run(shared: &Arc<Shared>, id: &str) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let (summary, woken) = {
        let mut st = h.state.lock().unwrap();
        let ts = now_ms();
        st.stop_requested = false;
        st.status = RunStatus::Running;
        st.updated_at_ms = ts;
        let mut woken = Vec::new();
        for a in st.roster.iter_mut() {
            if !a.killed {
                a.generation += 1;
                a.state = AgentWorkState::Waiting;
                a.last_error = None;
                a.updated_at_ms = ts;
                woken.push(a.name.clone());
            }
        }
        let _ = st.append_board("system", BoardKind::System, "Run resumed.", None);
        let (max_c, active_c, _, q) = h.scheduler.snapshot();
        st.save_run_file(max_c, &q);
        let sum = st.summary_json(max_c, active_c, &q);
        st.emit_event("run", json!({ "ts_ms": ts, "status": "running", "run": sum.clone() }));
        (sum, woken)
    };
    h.scheduler.set_stopped(false);
    spawn_all_workers(shared, &shared.teamwork, &h);
    h.notify_all();
    TeamworkReply::json("200 OK", json!({ "run": summary, "woken": woken }))
}

fn kill_agent(shared: &Arc<Shared>, id: &str, raw_name: &str, body: &Value) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("killed by owner");

    let agent_json = {
        let mut st = h.state.lock().unwrap();
        let Some(idx) = st.agent_index(raw_name) else {
            return TeamworkReply::err("404 Not Found", format!("unknown agent '{raw_name}'"));
        };
        let agent_name = st.roster[idx].name.clone();
        st.roster[idx].killed = true;
        st.roster[idx].state = AgentWorkState::Stopped;
        st.roster[idx].generation += 1;
        st.roster[idx].last_error = Some(reason.to_string());
        st.roster[idx].updated_at_ms = now_ms();

        // Release active file claims owned by this killed agent's unfinished tasks so work is never wedged.
        let mut released = Vec::new();
        for (p, c) in &st.claims {
            if c.owner.eq_ignore_ascii_case(&agent_name) {
                released.push(p.clone());
            }
        }
        for p in &released {
            st.claims.remove(p);
        }
        for t in st.tasks.iter_mut() {
            if t.owner.eq_ignore_ascii_case(&agent_name) && t.status.holds_claims() {
                t.claimed_files.retain(|f| !released.contains(f));
            }
        }
        st.save_tasks_file();
        st.save_agent_file(&agent_name);
        let turns = st.roster[idx].turns;
        st.append_trace(
            &agent_name,
            turns,
            "state",
            "Agent killed",
            json!({ "reason": reason, "released_claims": released }),
        );
        let _ = st.append_board(
            "system",
            BoardKind::System,
            &format!("Agent @{agent_name} stopped ({reason})."),
            None,
        );
        let snap = st.roster[idx].to_json();
        st.emit_event("roster", json!({ "ts_ms": now_ms(), "agent": snap.clone() }));
        snap
    };
    h.notify_all();
    TeamworkReply::json("200 OK", json!({ "agent": agent_json }))
}

fn restart_agent(shared: &Arc<Shared>, id: &str, raw_name: &str, body: &Value) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let reset = body.get("reset").and_then(|v| v.as_bool()).unwrap_or(false);
    let brief = body.get("brief").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());

    let (agent_json, gen, agent_name, should_spawn) = {
        let mut st = h.state.lock().unwrap();
        let Some(idx) = st.agent_index(raw_name) else {
            return TeamworkReply::err("404 Not Found", format!("unknown agent '{raw_name}'"));
        };
        let name = st.roster[idx].name.clone();
        st.roster[idx].killed = false;
        st.roster[idx].last_error = None;
        st.roster[idx].generation += 1;
        let gen = st.roster[idx].generation;
        if reset {
            st.roster[idx].messages.clear();
            st.roster[idx].turns = 0;
        }
        if st.status == RunStatus::Stopped {
            st.status = RunStatus::Running;
            st.stop_requested = false;
        }
        st.roster[idx].state = AgentWorkState::Waiting;
        st.roster[idx].updated_at_ms = now_ms();
        if let Some(b) = brief {
            let _ = st.append_board("human", BoardKind::Human, &format!("@{name} {b}"), None);
        } else {
            let _ = st.append_board("system", BoardKind::System, &format!("Agent @{name} restarted."), None);
        }
        st.save_agent_file(&name);
        let (max_c, _, _, q) = h.scheduler.snapshot();
        st.save_run_file(max_c, &q);
        let turns = st.roster[idx].turns;
        st.append_trace(&name, turns, "state", "Agent restarted", json!({ "reset": reset }));
        let snap = st.roster[idx].to_json();
        st.emit_event("roster", json!({ "ts_ms": now_ms(), "agent": snap.clone() }));
        (snap, gen, name, st.status == RunStatus::Running)
    };
    h.scheduler.set_stopped(false);
    if should_spawn {
        spawn_worker(shared, &shared.teamwork, &h, agent_name, gen);
    }
    h.notify_all();
    TeamworkReply::json("200 OK", json!({ "agent": agent_json }))
}

fn get_agent_trace(shared: &Arc<Shared>, id: &str, raw_name: &str, since: u64) -> TeamworkReply {
    let Some(h) = shared.teamwork.get(id) else {
        return TeamworkReply::err("404 Not Found", format!("unknown teamwork run '{id}'"));
    };
    let (agent_json, trace_path, messages) = {
        let st = h.state.lock().unwrap();
        let Some(idx) = st.agent_index(raw_name) else {
            return TeamworkReply::err("404 Not Found", format!("unknown agent '{raw_name}'"));
        };
        let a = &st.roster[idx];
        (
            a.to_json(),
            st.storage_dir.join("traces").join(format!("{}.jsonl", a.name)),
            a.messages.clone(),
        )
    };
    let mut trace = Vec::new();
    if let Ok(file) = File::open(&trace_path) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(t) {
                let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
                if seq > since {
                    trace.push(v);
                }
            }
        }
    }
    TeamworkReply::json(
        "200 OK",
        json!({ "agent": agent_json, "trace": trace, "messages": messages }),
    )
}

// ---------------------------------------------------------------------------
// Project & Model resolution (delegates to shared.projects + model_from_config)
// ---------------------------------------------------------------------------

fn resolve_project_target(
    shared: &Shared,
    explicit_id: Option<&str>,
) -> Result<(Option<String>, PathBuf), String> {
    let want = explicit_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    {
        let store = shared.projects.lock().unwrap();
        let target_id = want.as_deref().or(store.active.as_deref());
        if let Some(id) = target_id {
            if let Some(r) = store.find(id) {
                return Ok((Some(r.id), r.dir));
            }
            for p in &store.opened {
                let cname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if projects::slug(cname) == id && p.is_dir() {
                    return Ok((Some(id.to_string()), p.clone()));
                }
            }
            if want.is_some() {
                return Err(format!("unknown project '{id}'"));
            }
        }
        if let Some(r) = store.list().into_iter().next() {
            return Ok((Some(r.id), r.dir));
        }
        if let Some(p) = store.opened.iter().find(|p| p.is_dir()) {
            let cname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let id = projects::slug(cname);
            return Ok((if id.is_empty() { None } else { Some(id) }, p.clone()));
        }
    }
    let fallback = shared
        .data_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("openro-team-{}", std::process::id())))
        .join("teamwork-default");
    let _ = std::fs::create_dir_all(&fallback);
    Ok((None, fallback))
}

fn resolve_model_client(shared: &Shared, override_model: Option<&str>) -> ModelClient {
    let mut cfg = shared.config.lock().unwrap().clone();
    if let Some(m) = override_model.map(str::trim).filter(|s| !s.is_empty()) {
        cfg.model_id = Some(m.to_string());
    }
    model_from_config(&cfg, shared.data_dir.as_deref())
}

// ---------------------------------------------------------------------------
// Worker threads & agent turn loop
// ---------------------------------------------------------------------------

fn spawn_all_workers(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
) {
    let to_spawn: Vec<(String, u64)> = {
        let st = handle.state.lock().unwrap();
        st.roster
            .iter()
            .filter(|a| !a.killed)
            .map(|a| (a.name.clone(), a.generation))
            .collect()
    };
    for (name, gen) in to_spawn {
        spawn_worker(shared, mgr, handle, name, gen);
    }
}

fn spawn_worker(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: String,
    gen: u64,
) {
    let shared = Arc::clone(shared);
    let mgr = Arc::clone(mgr);
    let handle = Arc::clone(handle);
    std::thread::spawn(move || {
        worker_loop(&shared, &mgr, &handle, &agent_name, gen);
    });
}

fn is_cancelled(handle: &Arc<RunHandle>, agent_name: &str, gen: u64) -> bool {
    let st = handle.state.lock().unwrap();
    if st.stop_requested || st.status != RunStatus::Running {
        return true;
    }
    let Some(idx) = st.agent_index(agent_name) else {
        return true;
    };
    let a = &st.roster[idx];
    a.killed || a.generation != gen
}

fn worker_loop(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
) {
    loop {
        let should_work = {
            let mut st = handle.state.lock().unwrap();
            loop {
                if st.stop_requested || st.status != RunStatus::Running {
                    if let Some(idx) = st.agent_index(agent_name) {
                        if st.roster[idx].generation == gen {
                            st.set_agent_state(agent_name, AgentWorkState::Stopped);
                        }
                    }
                    return;
                }
                let Some(idx) = st.agent_index(agent_name) else { return };
                if st.roster[idx].killed || st.roster[idx].generation != gen {
                    if st.roster[idx].generation == gen {
                        st.set_agent_state(agent_name, AgentWorkState::Stopped);
                    }
                    return;
                }
                if st.agent_has_work(idx) {
                    st.set_agent_state(agent_name, AgentWorkState::Working);
                    break true;
                }
                st.set_agent_state(agent_name, AgentWorkState::Waiting);
                let (g, _) = handle
                    .wake_cv
                    .wait_timeout(st, Duration::from_secs(2))
                    .unwrap();
                st = g;
            }
        };
        if !should_work {
            return;
        }

        if let Err(err_msg) = run_agent_wake_cycle(shared, mgr, handle, agent_name, gen) {
            let mut st = handle.state.lock().unwrap();
            if let Some(idx) = st.agent_index(agent_name) {
                if st.roster[idx].generation == gen && !st.stop_requested {
                    st.roster[idx].last_error = Some(err_msg.clone());
                    let turns = st.roster[idx].turns;
                    st.append_trace(agent_name, turns, "error", &err_msg, json!({ "error": err_msg }));
                    st.set_agent_state(agent_name, AgentWorkState::Stopped);
                    st.save_agent_file(agent_name);
                    let _ = st.append_board(
                        "system",
                        BoardKind::System,
                        &format!("@{agent_name} stopped on error: {err_msg}"),
                        None,
                    );
                }
            }
            handle.notify_all();
            return;
        }
    }
}

fn rollback_unsent_board_turn(st: &mut RunState, agent_name: &str, pushed_board_msg: bool) {
    if pushed_board_msg {
        if let Some(idx) = st.agent_index(agent_name) {
            if st.roster[idx]
                .messages
                .last()
                .and_then(|m| m.get("role").and_then(|r| r.as_str()))
                == Some("user")
            {
                st.roster[idx].messages.pop();
            }
        }
    }
}

fn is_write_or_submit_tool(tool: &str) -> bool {
    matches!(
        tool,
        "project_write_file"
            | "project_edit_file"
            | "project_delete_file"
            | "project_format"
            | "project_update_manifest"
            | "project_scene_mutate"
            | "submit_task"
            | "mesh_generate"
            | "blender_generate"
            | "audio_generate"
            | "art_generate"
            | "image_process"
            | "art_pack_card"
            | "assets_authorise"
            | "assets_codegen"
            | "assets_upload"
            | "creator_hub_setup"
            | "creator_hub_configure"
            | "products_sync"
            | "products_codegen"
            | "project_publish"
            | "universe_set"
            | "adapter_invoke"
            | "create_task"
            | "verify_task"
            | "finish_run"
            | "evidence_record"
            | "unknown_add"
            | "unknown_update"
    )
}

fn is_action_required_value(val: &Value) -> bool {
    val.get("status").and_then(Value::as_str) == Some("action_required")
}

fn utf8_prefix_max_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn cap_turn_tool_results(results_lines: &[String], batch_aborted: bool, batched: bool) -> String {
    let abort_footer = if batch_aborted && batched {
        "\n(batched steps aborted on first error)"
    } else {
        ""
    };
    let overflow_footer = "\n[tool_result output capped at 28 KiB — read fewer files per turn or use start_line/end_line / project_grep]";
    let raw_total: usize = results_lines
        .iter()
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + abort_footer.len();
    if raw_total <= MAX_TURN_TOOL_RESULT_BYTES {
        let mut s = results_lines.join("\n");
        s.push_str(abort_footer);
        return s;
    }

    // Reserve room for footers + optional read-only nudge so total stays <= 28 KiB.
    let reserve = overflow_footer.len() + abort_footer.len() + 320;
    let body_budget = MAX_TURN_TOOL_RESULT_BYTES.saturating_sub(reserve);
    let mut kept: Vec<String> = Vec::with_capacity(results_lines.len());
    let mut used = 0usize;
    let mut overflowed = false;
    let mut omitted_count = 0usize;

    for (idx, line) in results_lines.iter().enumerate() {
        let sep = if kept.is_empty() { 0 } else { 1 };
        if !overflowed && used + sep + line.len() <= body_budget {
            used += sep + line.len();
            kept.push(line.clone());
            continue;
        }
        overflowed = true;
        if idx == 0 && body_budget > 512 {
            let prefix = utf8_prefix_max_bytes(line, body_budget.saturating_sub(64));
            let clipped = format!("{prefix}...[truncated from {} bytes]", line.len());
            used += clipped.len();
            kept.push(clipped);
            continue;
        }
        let summary = format!("[omitted overflow: {}]", summarize_tool_result_line(line));
        if used + 1 + summary.len() <= body_budget {
            used += 1 + summary.len();
            kept.push(summary);
        } else {
            omitted_count += results_lines.len() - idx;
            break;
        }
    }

    let mut out = kept.join("\n");
    if omitted_count > 0 {
        out.push_str(&format!("\n[...{omitted_count} additional tool result(s) omitted]"));
    }
    out.push_str(overflow_footer);
    out.push_str(abort_footer);
    out
}

fn run_agent_wake_cycle(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
) -> Result<(), String> {
    let mut parse_failures = 0usize;
    let mut transient_retries = 0usize;
    let mut consecutive_read_only_turns = 0usize;
    let mut recent_reads: VecDeque<(u64, String, u64, u64)> = VecDeque::new();
    for _step in 0..MAX_AGENT_STEPS_PER_WAKE {
        if is_cancelled(handle, agent_name, gen) {
            return Ok(());
        }
        let (model_client, messages_snapshot, pending_cursor, turn_num, pushed_board_msg) = {
            let mut st = handle.state.lock().unwrap();
            let Some(idx) = st.agent_index(agent_name) else { return Ok(()) };
            if st.roster[idx].killed || st.roster[idx].generation != gen {
                return Ok(());
            }
            st.roster[idx].parked_at_seq = None;
            let head_seq = st.board.latest_seq();
            if st.roster[idx].messages.is_empty() {
                let sys = build_role_system_prompt(&st, idx);
                st.roster[idx].messages.push(json!({ "role": "system", "content": sys }));
            }
            compact_agent_history(&mut st.roster[idx].messages);

            let board_update = build_board_wake_update(&st, idx);
            let pushed_board_msg = if let Some(msg) = board_update {
                st.roster[idx].messages.push(json!({ "role": "user", "content": msg }));
                true
            } else {
                false
            };

            let live_tasks = build_live_tasks_block(&st, idx);
            let mut msgs = st.roster[idx].messages.clone();
            if let Some(tasks_text) = live_tasks {
                if msgs.last().and_then(|m| m.get("role").and_then(|r| r.as_str())) == Some("user") {
                    if let Some(last) = msgs.last_mut() {
                        let prev = last.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        let combined = if prev.is_empty() {
                            tasks_text
                        } else {
                            format!("{prev}\n\n{tasks_text}")
                        };
                        last["content"] = Value::String(combined);
                    }
                } else {
                    msgs.push(json!({
                        "role": "user",
                        "content": format!("{tasks_text}\n\nContinue your assigned work and reply with one JSON step.")
                    }));
                }
            } else if msgs.last().and_then(|m| m.get("role").and_then(|r| r.as_str())) != Some("user") {
                msgs.push(json!({
                    "role": "user",
                    "content": "Continue your assigned work and reply with one JSON step."
                }));
            }

            let agent_model = st.roster[idx].model_id.clone();
            let sandbox_base = shared
                .data_dir
                .clone()
                .unwrap_or_else(|| shared.assets_root.clone())
                .join("agy-sandbox");
            let sandbox = sandbox_base.join(format!("{}-{}", st.id, agent_name));
            let client = resolve_model_client(shared, Some(&agent_model)).with_agy_dir(sandbox);
            let turn = st.roster[idx].turns + 1;
            st.append_trace(
                agent_name,
                turn,
                "prompt",
                "Sending model turn",
                json!({ "cursor_seq": head_seq, "message_count": msgs.len() }),
            );
            (client, msgs, head_seq, turn, pushed_board_msg)
        };

        let permit = handle
            .scheduler
            .acquire(|| is_cancelled(handle, agent_name, gen))
            .map_err(|_| String::new())?;
        let reply_res = model_client.chat(&messages_snapshot);
        drop(permit);

        if is_cancelled(handle, agent_name, gen) {
            return Ok(());
        }

        let reply = match reply_res {
            Ok(text) => {
                transient_retries = 0;
                if handle.scheduler.clear_quota_on_success() {
                    let mut st = handle.state.lock().unwrap();
                    let (max_c, _, _, q) = handle.scheduler.snapshot();
                    st.save_run_file(max_c, &q);
                    st.emit_event("quota", json!({ "ts_ms": now_ms(), "quota": q.to_json() }));
                    handle.notify_all();
                }
                text
            }
            Err(err) => {
                if let Some((reset_in, backoff_ms)) = parse_quota_error(&err) {
                    let q = handle.scheduler.record_quota_error(reset_in.clone(), backoff_ms, err.clone());
                    {
                        let mut st = handle.state.lock().unwrap();
                        let reset_label = reset_in.as_deref().unwrap_or("shortly");
                        let notice = format!(
                            "Model quota reached for {agent_name}; resets in {reset_label}. Backing off."
                        );
                        let _ = st.append_board("system", BoardKind::Quota, &notice, None);
                        st.append_trace(
                            agent_name,
                            turn_num,
                            "quota_backoff",
                            &notice,
                            json!({ "reset_in": reset_in, "backoff_ms": backoff_ms }),
                        );
                        let (max_c, _, _, _) = handle.scheduler.snapshot();
                        st.save_run_file(max_c, &q);
                        st.emit_event("quota", json!({ "ts_ms": now_ms(), "quota": q.to_json() }));
                        rollback_unsent_board_turn(&mut st, agent_name, pushed_board_msg);
                    }
                    handle.notify_all();
                    continue;
                }
                if is_transient_model_error(&err) && transient_retries < MAX_TRANSIENT_MODEL_RETRIES {
                    let backoff_ms = transient_retry_backoff_ms(transient_retries);
                    transient_retries += 1;
                    let notice = format!(
                        "Transient model error for {agent_name}; retry {transient_retries}/{MAX_TRANSIENT_MODEL_RETRIES} in {backoff_ms}ms: {err}"
                    );
                    {
                        let mut st = handle.state.lock().unwrap();
                        st.append_trace(
                            agent_name,
                            turn_num,
                            "transient_retry",
                            &notice,
                            json!({
                                "attempt": transient_retries,
                                "max_retries": MAX_TRANSIENT_MODEL_RETRIES,
                                "backoff_ms": backoff_ms,
                                "error": err,
                            }),
                        );
                        rollback_unsent_board_turn(&mut st, agent_name, pushed_board_msg);
                    }
                    if wait_transient_backoff(handle, agent_name, gen, backoff_ms) {
                        return Ok(());
                    }
                    continue;
                }
                return Err(err);
            }
        };

        {
            let mut st = handle.state.lock().unwrap();
            if let Some(idx) = st.agent_index(agent_name) {
                if pending_cursor > st.roster[idx].cursor_seq {
                    st.roster[idx].cursor_seq = pending_cursor;
                }
                st.roster[idx].turns = turn_num;
                st.roster[idx].last_error = None;
                st.roster[idx].updated_at_ms = now_ms();
                st.roster[idx].messages.push(json!({ "role": "assistant", "content": reply }));
                compact_agent_history(&mut st.roster[idx].messages);
                st.append_trace(
                    agent_name,
                    turn_num,
                    "model_reply",
                    "Received model step",
                    json!({ "bytes": reply.len(), "preview": reply.chars().take(200).collect::<String>() }),
                );
                st.save_agent_file(agent_name);
            }
        }

        let step = match parse_step(&reply) {
            Ok(s) => {
                parse_failures = 0;
                s
            }
            Err(e) => {
                parse_failures += 1;
                if parse_failures >= 4 {
                    return Err(format!("agent produced unparseable replies: {e}"));
                }
                let mut st = handle.state.lock().unwrap();
                if let Some(idx) = st.agent_index(agent_name) {
                    st.roster[idx].messages.push(json!({
                        "role": "user",
                        "content": format!("Step parse error: {e}. Reply with ONE JSON object: {{\"tool\":\"...\",\"args\":{{...}}}} or {{\"done\":true,\"summary\":\"...\"}}.")
                    }));
                }
                continue;
            }
        };

        let mut batch: Vec<(String, Value)> = Vec::new();
        if let Some(arr) = step.get("steps").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(t) = item.get("tool").and_then(|v| v.as_str()) {
                    batch.push((
                        t.to_string(),
                        item.get("args").cloned().unwrap_or_else(|| json!({})),
                    ));
                }
            }
        } else if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
            batch.push((
                tool.to_string(),
                step.get("args").cloned().unwrap_or_else(|| json!({})),
            ));
        }

        let mut batch_ran = false;
        let mut batch_aborted = false;
        let mut spin_park_head: Option<u64> = None;
        if !batch.is_empty() {
            batch_ran = true;
            let batched = step.get("steps").is_some();
            let only_board_read = batch.iter().all(|(t, _)| t == "board_read");
            let mut last_board_read_head: Option<u64> = None;
            let mut did_write_or_submit = false;
            let mut dup_read_in_turn: Option<(String, u64, u64)> = None;
            let mut results_lines = Vec::with_capacity(batch.len());
            for (step_idx, (tool, args)) in batch.into_iter().enumerate() {
                if is_cancelled(handle, agent_name, gen) {
                    return Ok(());
                }
                {
                    let mut st = handle.state.lock().unwrap();
                    st.append_trace(
                        agent_name,
                        turn_num,
                        "tool_call",
                        &format!("Calling {tool}"),
                        json!({ "tool": tool, "args": args, "step": step_idx + 1 }),
                    );
                }
                let tool_out =
                    execute_agent_tool(shared, mgr, handle, agent_name, gen, &tool, &args);
                let ok = tool_out.is_ok();
                let action_required = tool_out.as_ref().is_ok_and(is_action_required_value);
                if ok && !action_required && is_write_or_submit_tool(&tool) {
                    did_write_or_submit = true;
                }
                if ok && !action_required && (is_write_or_submit_tool(&tool) || tool == "board_post") {
                    recent_reads.clear();
                    dup_read_in_turn = None;
                }
                if ok && tool == "project_read_file" {
                    if let Ok(v) = &tool_out {
                        let path = v
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let start_line = v.get("start_line").and_then(Value::as_u64).unwrap_or(1);
                        let end_line = v.get("end_line").and_then(Value::as_u64).unwrap_or(0);
                        while recent_reads
                            .front()
                            .is_some_and(|(t, _, _, _)| turn_num.saturating_sub(*t) > DUPLICATE_READ_WINDOW_TURNS)
                        {
                            recent_reads.pop_front();
                        }
                        if recent_reads
                            .iter()
                            .any(|(_, p, s, e)| p == &path && *s == start_line && *e == end_line)
                        {
                            dup_read_in_turn = Some((path.clone(), start_line, end_line));
                        }
                        recent_reads.push_back((turn_num, path, start_line, end_line));
                    }
                }
                if ok && tool == "board_read" {
                    last_board_read_head = tool_out
                        .as_ref()
                        .ok()
                        .and_then(|v| v.get("head"))
                        .and_then(|h| h.as_u64());
                }
                results_lines.push(match &tool_out {
                    Ok(v) => format!("tool_result ({tool}): {v}"),
                    Err(e) => format!("tool_result ({tool}) error: {e}"),
                });
                let mut st = handle.state.lock().unwrap();
                st.append_trace(
                    agent_name,
                    turn_num,
                    "tool_result",
                    &format!("{tool} -> {}", if ok { "ok" } else { "error" }),
                    json!({ "tool": tool, "ok": ok, "result": tool_out.as_ref().ok(), "error": tool_out.as_ref().err() }),
                );
                if !ok || action_required {
                    batch_aborted = true;
                    break;
                }
            }
            if only_board_read && !batch_aborted && last_board_read_head == Some(pending_cursor) {
                spin_park_head = last_board_read_head;
            }
            if did_write_or_submit {
                consecutive_read_only_turns = 0;
            } else {
                consecutive_read_only_turns += 1;
            }
            let mut combined = cap_turn_tool_results(&results_lines, batch_aborted, batched);
            let mut parked_spin = false;
            {
                let mut st = handle.state.lock().unwrap();
                if let Some(idx) = st.agent_index(agent_name) {
                    let role = st.roster[idx].role.as_str();
                    let is_worker = !matches!(role, "coordinator" | "verifier");
                    let has_worker_task = is_worker
                        && st.tasks.iter().any(|t| {
                            t.owner.eq_ignore_ascii_case(agent_name)
                                && matches!(t.status, TaskStatus::InProgress | TaskStatus::ChangesRequested)
                        });
                    let has_verifier_task = role == "verifier"
                        && st.tasks.iter().any(|t| {
                            t.verifier.eq_ignore_ascii_case(agent_name)
                                && t.status == TaskStatus::AwaitingVerification
                        });
                    if has_worker_task || has_verifier_task {
                        let mut parts: Vec<String> = Vec::new();
                        if let Some((ref d_path, d_start, d_end)) = dup_read_in_turn {
                            parts.push(format!(
                                "Stop re-reading; write/edit/verify/submit now — `{d_path}` lines {d_start}..={d_end} was already read within the last {DUPLICATE_READ_WINDOW_TURNS} turns and remains in history."
                            ));
                        }
                        if consecutive_read_only_turns >= READ_ONLY_SPIN_THRESHOLD {
                            if has_worker_task {
                                parts.push(format!(
                                    "Stop reading, write your files now! You have run {consecutive_read_only_turns} consecutive turns with 0 writes or submissions; emit project_write_file/project_edit_file or submit_task on this turn."
                                ));
                            } else if has_verifier_task && dup_read_in_turn.is_none() {
                                parts.push(format!(
                                    "Stop re-reading; write/edit/verify/submit now — you have run {consecutive_read_only_turns} consecutive read-only turns; run project_playtest/project_run_lune and verify_task on this turn."
                                ));
                            }
                        }
                        if !parts.is_empty() {
                            let nudge = parts.join(" ");
                            combined.push_str("\n\n");
                            combined.push_str(&nudge);
                            st.append_trace(
                                agent_name,
                                turn_num,
                                "read_spin_nudge",
                                &nudge,
                                json!({
                                    "consecutive_read_only_turns": consecutive_read_only_turns,
                                    "duplicate_read": dup_read_in_turn.as_ref().map(|(p, s, e)| json!({
                                        "path": p,
                                        "start_line": s,
                                        "end_line": e,
                                    })),
                                }),
                            );
                        }
                    }
                    st.roster[idx]
                        .messages
                        .push(json!({ "role": "user", "content": combined }));
                    compact_agent_history(&mut st.roster[idx].messages);
                    if let Some(head) = spin_park_head {
                        let all_done = st.roster[idx].role == "coordinator"
                            && !st.tasks.is_empty()
                            && st
                                .tasks
                                .iter()
                                .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled));
                        if !all_done && st.board.latest_seq() == head {
                            st.roster[idx].parked_at_seq = Some(head);
                            st.append_trace(
                                agent_name,
                                turn_num,
                                "auto_park",
                                &format!("Auto-parked after board_read poll with unchanged head {head}"),
                                json!({ "head": head }),
                            );
                            parked_spin = true;
                        }
                    }
                    st.save_agent_file(agent_name);
                }
            }
            handle.notify_all();
            if parked_spin {
                return Ok(());
            }
            if batch_aborted || step.get("done").and_then(|v| v.as_bool()) != Some(true) {
                continue;
            }
        }

        if !batch_aborted && step.get("done").and_then(|v| v.as_bool()) == Some(true) {
            let summary = step
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Done.")
                .to_string();
            let post_text = step.get("post").and_then(|v| v.as_str()).map(String::from);
            let mut st = handle.state.lock().unwrap();
            if let Some(text) = post_text {
                let _ = st.append_board(agent_name, BoardKind::Post, &text, None);
            }
            if let Some(idx) = st.agent_index(agent_name) {
                let role = st.roster[idx].role.clone();
                if role == "coordinator"
                    && !st.tasks.is_empty()
                    && st
                        .tasks
                        .iter()
                        .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
                {
                    let _ = finalize_completed_run(&mut st, handle, agent_name, &summary);
                } else {
                    st.roster[idx].parked_at_seq = Some(st.board.latest_seq());
                    st.append_trace(agent_name, turn_num, "done", &summary, json!({ "summary": summary }));
                    st.save_agent_file(agent_name);
                }
            }
            handle.notify_all();
            return Ok(());
        }

        if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
            let args = step.get("args").cloned().unwrap_or_else(|| json!({}));
            let res = crate::skills::generate(skill, &args);
            let msg = match res {
                Ok(src) => format!("skill_result: {skill} generated {} bytes:\n{src}", src.len()),
                Err(e) => format!("skill_result: error: {e}"),
            };
            let mut st = handle.state.lock().unwrap();
            st.append_trace(agent_name, turn_num, "tool_result", &format!("skill {skill}"), json!({ "skill": skill }));
            if let Some(idx) = st.agent_index(agent_name) {
                st.roster[idx].messages.push(json!({ "role": "user", "content": msg }));
                st.save_agent_file(agent_name);
            }
            continue;
        }
        if batch_ran {
            continue;
        }

        // Unrecognized object
        let mut st = handle.state.lock().unwrap();
        if let Some(idx) = st.agent_index(agent_name) {
            st.roster[idx].messages.push(json!({
                "role": "user",
                "content": "Unrecognized step. Reply with {\"tool\":\"...\",\"args\":{...}}, {\"steps\":[...]}, or {\"done\":true,\"summary\":\"...\"}."
            }));
        }
    }
    Ok(())
}

fn build_role_system_prompt(st: &RunState, idx: usize) -> String {
    let a = &st.roster[idx];
    let roster_list: Vec<String> = st
        .roster
        .iter()
        .map(|m| format!("{} ({})", m.name, m.role))
        .collect();
    let role_rules = match a.role.as_str() {
        "coordinator" => concat!(
            "You are the Teamwork coordinator.\n",
            "Split the goal into concrete non-overlapping tasks with `create_task` (specify owner, verifier, kind, and non-overlapping `files`); spread tasks across all available workers in parallel instead of stacking everything onto one worker.\n",
            "Follow Study -> Build -> Check -> Refine: check `study_status` so gaps are investigated with `evidence_record`/`unknown_add` before coding and checked with `mechanics_check`/`project_check`.\n",
            "To wait for workers, return {\"done\":true,\"summary\":\"waiting\"} — NEVER poll board_read.\n",
            "Call `finish_run` once all tasks are `done`.\n",
            "Tools: create_task, board_post, board_read, project_files, project_read_file, project_grep, project_scene_get, study_status, unknowns_list, evidence_list, opencloud_status, creator_hub_status, creator_hub_setup, creator_hub_configure, assets_authorise, assets_upload, assets_codegen, products_sync, products_codegen, project_publish, universe_get, universe_set, finish_run.\n",
            "Cloud/Publish rules: check `opencloud_status` / `creator_hub_status` before publishing; `creator_hub_*` uses loopback CDP and NEVER bypasses login/2FA/CAPTCHA (if `status:\"action_required\"`, stop and report `blocked_reason`/`url` to owner); `project_publish` with `versionType:\"Published\"` REQUIRES explicit owner approval and `{\"versionType\":\"Published\",\"confirm\":true}` (default `versionType` is `\"Saved\"`); never self-approve product prices."
        ),
        "verifier" => concat!(
            "You are the Teamwork verifier.\n",
            "Follow Study -> Build -> Check -> Refine: inspect submitted work (`project_read_file`, `project_grep`, `project_scene_get`, `luau_check`, `project_check`, `project_run_lune`, `project_build`, `evidence_list`, `unknowns_list`, `mechanics_check`, `study_status`, `opencloud_status`, `creator_hub_status`, `universe_get`).\n",
            "For game_code tasks (or `src/` / `ui/` changes), you MUST run `project_playtest` with `{\"task_id\":\"<id>\"}` and confirm 0 console errors BEFORE calling `verify_task` with `passed: true`.\n",
            "For `tools/**/*.luau` tasks (not touching `src/` or `ui/`), run `project_run_lune` (confirm `passed: true`) instead of `project_playtest` BEFORE calling `verify_task(passed=true)`.\n",
            "If checks, contracts (`mechanics_check`), or critical unknowns fail, call `verify_task` with `passed: false` and clear failure notes."
        ),
        "researcher" => concat!(
            "You are the Teamwork researcher/designer.\n",
            "Follow Study -> Build -> Check -> Refine: use `research_roblox_game` (`target` = Roblox URL or place/universe ID -> saves dossier & thumbs in `refs/<universeId>/`), `web_search`, `web_fetch`, `project_files`, `project_read_file`, `project_grep`, `project_write_file`, `project_edit_file`, `project_update_manifest`, `evidence_record`, `evidence_list`, `unknown_add`, `unknown_update`, `unknowns_list`, `mechanics_check`, `study_status`.\n",
            "Record observed/derived/inferred findings with `evidence_record`, track open gaps with `unknown_add`/`unknown_update` (resolve only with `resolution` + `evidence_for`), and validate `docs/mechanics.json` provenance via `mechanics_check`.\n",
            "Budget <=3 research turns before writing deliverables (`web_fetch` is capped cleanly).\n",
            "Claim every file you write (`project_write_file` auto-claims onto your active task), then call `submit_task` to hand off to `@verifier`."
        ),
        _ => concat!(
            "You are a Teamwork specialist worker.\n",
            "Claim every file you edit with `claim_files` (two active tasks may never claim or edit the same file).\n",
            "Follow Study -> Build -> Check -> Refine: check `study_status`/`unknowns_list`, record verified behavior with `evidence_record`/`unknown_add`/`unknown_update` (capping guesswork), cite `ev_*` IDs in `docs/mechanics.json` (`mechanics_check`), build, check (`project_check`/`project_playtest`, pass `\"force\":true` to bypass `KeptChecks`), and refine.\n",
            "Use `project_files`, `project_read_file`, `project_grep`, `project_write_file`, `project_edit_file`, `project_delete_file`, `project_update_manifest`, `project_scene_get`, `project_scene_mutate`, `luau_check`, `project_format`, `project_check`, `project_run_lune`, `project_build`, `project_sync`, `project_playtest`, `research_roblox_game`, `web_search`, `web_fetch`, `evidence_record`, `evidence_list`, `unknown_add`, `unknown_update`, `unknowns_list`, `mechanics_check`, `study_status`, `reference_search`, `reference_select`, `art_generate`, `art_approve`, `image_process`, `art_pack_card`, `mesh_generate`, `blender_generate`, `audio_generate`, `adapter_list`, `adapter_invoke`, `opencloud_status`, `creator_hub_status`, `creator_hub_setup`, `creator_hub_configure`, `assets_authorise`, `assets_codegen`, `assets_upload`, `products_sync`, `products_codegen`, `project_publish`, `universe_get`, `universe_set`.\n",
            "Formatting: `project_check` with `{\"format\":true}` or `project_format` auto-formats your claimed `.luau` files with `stylua` instead of hand-editing whitespace.\n",
            "Asset arg signatures:\n",
            "- reference_search: {\"character\":\"...\",\"show\":\"...\",\"card_id\":\"...\"}\n",
            "- art_generate: {\"prompt\":\"single-line eye-level three-quarter prompt\",\"out_path\":\"art/cards/x/art.png\",\"references\":[\"art/references/...\"]}\n",
            "- image_process: {\"output\":\"art/ui/x.png\",\"input\":\"...?\",\"ops\":[{\"op\":\"canvas\",\"width\":256,\"height\":256,\"color\":\"#00000000\"},{\"op\":\"draw\",\"shapes\":[...]}]} (or \"script\")\n",
            "- mesh_generate: {\"name\":\"PropName\",\"parts\":[{\"shape\":\"box|cylinder|sphere|cone|torus|wedge\",\"size\":[w,d,h],\"pos\":[x,y,z],\"color\":\"#hex\"}],\"script\":\"...?\"}\n",
            "- audio_generate: {\"name\":\"sfx_name\",\"preset\":\"summon_roll|mythic_fanfare|battle_crit|potion_drink|ui_click|level_up\",\"duration_ms\":1200,\"out_path\":\"assets/audio/sfx.wav\"}\n",
            "Cloud & Creator Hub rules & signatures:\n",
            "- creator_hub_status {\"cdp_url\":\"http://127.0.0.1:9222\",\"check_login\":true,\"save_creator\":true} / creator_hub_setup / creator_hub_configure: loopback CDP only; NEVER bypass login/2FA/CAPTCHA — if `status:\"action_required\"`, stop and report `blocked_reason`/`url` to owner; keys stay in 0600 SecretStore (never logged)\n",
            "- assets_authorise {\"paths\":[\"art/**\"]} -> assets_upload {\"paths\":[...]} -> assets_codegen {}\n",
            "- products_sync {\"dry_run\":false,\"default_for_sale\":false,\"auto_codegen\":true} -> products_codegen {} (emits src/shared/ProductIds.luau; never self-approve product prices)\n",
            "- project_publish {\"versionType\":\"Saved\"|\"Published\",\"confirm\":true?} (`versionType:\"Published\"` REQUIRES explicit owner approval + `confirm:true`); universe_get {} / universe_set {\"settings\":{...}}\n",
            "When your task is ready for verification, call `submit_task` to hand it to `@verifier`."
        ),
    };
    let custom_brief = a
        .brief
        .as_deref()
        .map(|b| format!("\nRole brief: {b}\n"))
        .unwrap_or_default();
    format!(
        "[Role: {}] [Agent: {}]\n\
         Goal: {}\n\
         Project dir: {}\n\
         Roster: {}\n\
         {custom_brief}\n\
         {role_rules}\n\
         Note: `project_write_file` / `project_edit_file` automatically claim unclaimed files onto your active task.\n\
         Prefer batching actions + completion in ONE JSON object per turn: {{\"steps\":[{{\"tool\":\"...\",\"args\":{{...}}}},...],\"done\":true,\"summary\":\"...\"}}, or omit \"done\" to inspect tool outputs next turn; single {{\"tool\":\"<name>\",\"args\":{{...}}}} or {{\"done\":true,\"summary\":\"...\"}} also work.",
        a.role,
        a.name,
        st.goal,
        st.project_dir.display(),
        roster_list.join(", ")
    )
}

fn build_board_wake_update(st: &RunState, idx: usize) -> Option<String> {
    let unread = st.board.since(st.roster[idx].cursor_seq);
    if unread.is_empty() {
        return None;
    }
    let lines: Vec<String> = unread
        .iter()
        .map(|m| format!("#{} {}: {}", m.seq, m.author, m.text))
        .collect();
    Some(format!("New board messages:\n{}", lines.join("\n")))
}

fn build_live_tasks_block(st: &RunState, idx: usize) -> Option<String> {
    let a = &st.roster[idx];
    let relevant_tasks: Vec<String> = st
        .tasks
        .iter()
        .filter(|t| {
            a.role == "coordinator"
                || t.owner.eq_ignore_ascii_case(&a.name)
                || (a.role == "verifier" && t.verifier.eq_ignore_ascii_case(&a.name))
        })
        .map(|t| {
            format!(
                "- {} [{}] ({}) owner={} files={:?}: {}",
                t.id,
                t.status.as_str(),
                t.kind.as_str(),
                t.owner,
                t.claimed_files,
                t.title
            )
        })
        .collect();
    if relevant_tasks.is_empty() {
        None
    } else {
        Some(format!("Tasks:\n{}", relevant_tasks.join("\n")))
    }
}

fn single_line_truncate(s: &str, max_chars: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
        .collect();
    let trimmed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.chars().count() <= max_chars {
        trimmed
    } else {
        let head: String = trimmed.chars().take(max_chars).collect();
        format!("{head}...")
    }
}

fn summarize_tool_result_line(line: &str) -> String {
    let Some(rest) = line.strip_prefix("tool_result (") else {
        return single_line_truncate(line, 100);
    };
    let Some((tool, after)) = rest.split_once(')') else {
        return single_line_truncate(line, 100);
    };
    if let Some(err) = after.strip_prefix(" error:") {
        return format!("{tool}: err({})", single_line_truncate(err.trim(), 90));
    }
    let payload = after.strip_prefix(':').unwrap_or(after).trim();
    if let Ok(val) = serde_json::from_str::<Value>(payload) {
        let mut meta = Vec::new();
        for k in ["path", "url", "target", "task_id", "id"] {
            if let Some(s) = val.get(k).and_then(|v| v.as_str()) {
                meta.push(format!("{k}={}", single_line_truncate(s, 60)));
            }
        }
        if let Some(n) = val.get("count").and_then(|v| v.as_u64()) {
            meta.push(format!("count={n}"));
        }
        if let Some(p) = val.get("passed").and_then(|v| v.as_bool()) {
            meta.push(format!("passed={p}"));
        }
        if let Some(b) = val.get("bytes").and_then(|v| v.as_u64()) {
            meta.push(format!("bytes={b}"));
        }
        if !meta.is_empty() {
            return format!("{tool}: ok ({})", meta.join(", "));
        }
    }
    format!("{tool}: ok ({})", single_line_truncate(payload, 80))
}

fn compact_step_args_obj(step: &mut serde_json::Map<String, Value>) -> bool {
    let tool = step
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let Some(args) = step.get_mut("args").and_then(|v| v.as_object_mut()) else {
        return false;
    };
    let path_hint = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("<file>")
        .to_string();
    let mut changed = false;
    for key in ["content", "source", "code", "script"] {
        if let Some(s) = args.get(key).and_then(|v| v.as_str()) {
            if s.len() > 200 {
                args.insert(
                    key.to_string(),
                    json!(format!(
                        "[compacted {tool} {key} ({} bytes) for {path_hint}]",
                        s.len()
                    )),
                );
                changed = true;
            }
        }
    }
    if let Some(edits) = args.get("edits").and_then(|v| v.as_array()) {
        let bytes = serde_json::to_string(edits).map(|s| s.len()).unwrap_or(0);
        if bytes > 240 {
            args.insert(
                "edits".to_string(),
                json!(format!(
                    "[compacted {} edit(s) ({bytes} bytes) on {path_hint}]",
                    edits.len()
                )),
            );
            changed = true;
        }
    }
    changed
}

fn compact_user_message_entry(raw: &str, compact_tool_entries: bool) -> Option<String> {
    if raw.starts_with("[compacted ") {
        return None;
    }
    let content = raw.split("\n\nTasks:\n").next().unwrap_or(raw);
    if content.starts_with("Tasks:\n") {
        return Some("[compacted task snapshot]".to_string());
    }
    if content.starts_with("tool_result (") {
        if !compact_tool_entries {
            return (content.len() != raw.len()).then(|| content.to_string());
        }
        let summaries: Vec<String> = content
            .lines()
            .filter(|l| l.starts_with("tool_result ("))
            .map(summarize_tool_result_line)
            .collect();
        let line = if summaries.is_empty() {
            format!(
                "[compacted tool_result ({} bytes): {}]",
                content.len(),
                single_line_truncate(content, 160)
            )
        } else {
            format!(
                "[compacted tool_result ({} bytes): {}]",
                content.len(),
                single_line_truncate(&summaries.join("; "), 260)
            )
        };
        return Some(line);
    }
    if content.starts_with("skill_result:") {
        if !compact_tool_entries {
            return (content.len() != raw.len()).then(|| content.to_string());
        }
        return Some(format!(
            "[compacted skill_result ({} bytes): {}]",
            content.len(),
            single_line_truncate(content, 140)
        ));
    }
    if content.starts_with("Step parse error:") && content.len() > 220 {
        return Some(format!(
            "[compacted parse error: {}]",
            single_line_truncate(content, 140)
        ));
    }
    if content.len() > 1_200 {
        return Some(format!(
            "[compacted update ({} bytes): {}]",
            content.len(),
            single_line_truncate(content, 200)
        ));
    }
    (content.len() != raw.len()).then(|| content.to_string())
}

/// Compact large assistant write payloads older than 2 turns and retain a
/// byte-budgeted window (~56 KiB / up to 8 recent turns) of `tool_result`
/// messages intact before compacting older turns (#59), keeping `agents/<name>.json`
/// and model prompts well under `model.rs`'s 96 KiB `-p` cap.
fn compact_agent_history(messages: &mut [Value]) {
    if messages.len() <= 1 {
        return;
    }
    let asst_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(i, m)| *i > 0 && m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
        .map(|(i, _)| i)
        .collect();
    let asst_cutoff = if asst_indices.len() > 2 {
        asst_indices[asst_indices.len() - 2]
    } else {
        1
    };

    for m in &mut messages[1..asst_cutoff] {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let Some(raw) = m.get("content").and_then(|v| v.as_str()).map(String::from) else {
            continue;
        };
        if raw.starts_with("[compacted ") {
            continue;
        }
        if role == "assistant" {
            if let Ok(mut val) = serde_json::from_str::<Value>(raw.trim()) {
                let mut changed = false;
                if let Some(obj) = val.as_object_mut() {
                    if compact_step_args_obj(obj) {
                        changed = true;
                    }
                    if let Some(steps) = obj.get_mut("steps").and_then(|v| v.as_array_mut()) {
                        for st in steps {
                            if let Some(so) = st.as_object_mut() {
                                if compact_step_args_obj(so) {
                                    changed = true;
                                }
                            }
                        }
                    }
                }
                let one_line = val.to_string();
                if changed || (raw.contains('\n') && one_line.len() <= raw.len()) {
                    m["content"] = Value::String(one_line);
                }
            } else if raw.len() > 300 || raw.contains('\n') {
                m["content"] = Value::String(format!(
                    "[compacted assistant ({} bytes): {}]",
                    raw.len(),
                    single_line_truncate(&raw, 160)
                ));
            }
        } else if role == "user" {
            let trimmed = raw.split("\n\nTasks:\n").next().unwrap_or(&raw);
            let is_tool_msg =
                trimmed.starts_with("tool_result (") || trimmed.starts_with("skill_result:");
            if !is_tool_msg {
                if let Some(next) = compact_user_message_entry(&raw, false) {
                    m["content"] = Value::String(next);
                }
            }
        }
    }

    let mut base_bytes = 0usize;
    for m in messages.iter() {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let raw = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let is_uncompacted_tool = role == "user" && !raw.starts_with("[compacted ") && {
            let trimmed = raw.split("\n\nTasks:\n").next().unwrap_or(raw);
            trimmed.starts_with("tool_result (") || trimmed.starts_with("skill_result:")
        };
        if !is_uncompacted_tool {
            base_bytes += raw.len();
        }
    }

    let mut kept_tool_turns = 0usize;
    let mut retained_tool_bytes = 0usize;
    let mut hit_cutoff = false;
    for m in messages[1..].iter_mut().rev() {
        if m.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let Some(raw) = m.get("content").and_then(|v| v.as_str()).map(String::from) else {
            continue;
        };
        if raw.starts_with("[compacted ") {
            continue;
        }
        let content_no_tasks = raw.split("\n\nTasks:\n").next().unwrap_or(&raw);
        let is_tool = content_no_tasks.starts_with("tool_result (")
            || content_no_tasks.starts_with("skill_result:");
        if !is_tool {
            continue;
        }
        let entry_len = content_no_tasks.len();
        let fits = !hit_cutoff
            && (kept_tool_turns == 0
                || (kept_tool_turns < HISTORY_RETAIN_TURNS
                    && retained_tool_bytes + entry_len <= HISTORY_TOOL_RESULT_BUDGET_BYTES
                    && base_bytes + retained_tool_bytes + entry_len
                        <= HISTORY_TOTAL_PROMPT_BUDGET_BYTES));
        if fits {
            kept_tool_turns += 1;
            retained_tool_bytes += entry_len;
            if content_no_tasks.len() != raw.len() {
                m["content"] = Value::String(content_no_tasks.to_string());
            }
        } else {
            hit_cutoff = true;
            if let Some(compacted) = compact_user_message_entry(&raw, true) {
                base_bytes += compacted.len();
                m["content"] = Value::String(compacted);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tool Execution (claims, verifier gate, Studio lease, project/asset tools, git commit)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn execute_agent_tool(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    match tool {
        "board_read" => {
            let since = args.get("since").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut st = handle.state.lock().unwrap();
            let msgs: Vec<Value> = st.board.since(since).into_iter().map(|m| m.to_json()).collect();
            let head = st.board.latest_seq();
            if let Some(idx) = st.agent_index(agent_name) {
                if head > st.roster[idx].cursor_seq {
                    st.roster[idx].cursor_seq = head;
                }
            }
            Ok(json!({ "head": head, "messages": msgs }))
        }
        "board_post" => {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("board_post requires non-empty args.text")?;
            let task_id = args.get("task_id").and_then(|v| v.as_str()).map(String::from);
            let mut st = handle.state.lock().unwrap();
            let kind = if crate::board::is_done_post(text) {
                BoardKind::Done
            } else {
                BoardKind::Post
            };
            let msg = st.append_board(agent_name, kind, text, task_id)?;
            Ok(msg.to_json())
        }
        "create_task" => tool_create_task(handle, agent_name, args),
        "claim_files" => tool_claim_files(handle, agent_name, args),
        "release_files" => tool_release_files(handle, agent_name, args),
        "submit_task" => tool_submit_task(handle, agent_name, args),
        "verify_task" => tool_verify_task(handle, agent_name, args),
        "finish_run" => tool_finish_run(handle, agent_name, args),
        "project_list" => Ok(tool_project_list(shared)),
        "project_files" => tool_project_files(handle, args),
        "project_read_file" => tool_project_read_file(handle, args),
        "project_grep" => tool_project_grep(handle, args),
        "project_write_file" => tool_project_write_file(handle, agent_name, args),
        "project_edit_file" => tool_project_edit_file(handle, agent_name, args),
        "project_delete_file" => tool_project_delete_file(handle, agent_name, args),
        "luau_check" => tool_luau_check(handle, args),
        "project_format" => tool_project_format(handle, agent_name, args),
        "project_check" => tool_project_check(handle, agent_name, args),
        "project_run_lune" => tool_project_run_lune(handle, agent_name, args),
        "project_update_manifest" => tool_project_update_manifest(handle, agent_name, args),
        "project_scene_get" => tool_project_scene_get(handle),
        "project_scene_mutate" => tool_project_scene_mutate(handle, agent_name, args),
        "project_build" => tool_project_build(handle, args),
        "project_sync" => tool_project_sync(shared, mgr, handle, agent_name, gen, args),
        "project_playtest" => {
            tool_project_playtest(shared, mgr, handle, agent_name, gen, args)
        }
        "research_roblox_game" => tool_research_roblox_game(handle, args),
        "web_fetch" => tool_web_fetch(args),
        "web_search" => tool_web_search(args),
        "evidence_record" => tool_evidence_record(handle, args),
        "evidence_list" => tool_evidence_list(handle, args),
        "unknown_add" => tool_unknown_add(handle, args),
        "unknown_update" => tool_unknown_update(handle, args),
        "unknowns_list" => tool_unknowns_list(handle, args),
        "mechanics_check" => tool_mechanics_check(handle, args),
        "study_status" => tool_study_status(handle),
        "blender_generate" | "mesh_generate" => {
            let res = tool_blender_generate(shared, handle, args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "audio_generate" => {
            let res = tool_audio_generate(shared, handle, args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "reference_search" => run_opencloud_route(shared, handle, "POST", "/api/art/references/search", args),
        "reference_select" => run_opencloud_route(shared, handle, "POST", "/api/art/references/select", args),
        "art_generate" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/art/generate", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "art_approve" => run_opencloud_route(shared, handle, "POST", "/api/art/approve", args),
        "image_process" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/art/process", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "art_pack_card" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/art/pack-card", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "opencloud_status" => run_opencloud_route(shared, handle, "GET", "/api/opencloud", args),
        "creator_hub_status" => {
            let path = creator_hub_status_path(args);
            run_opencloud_route(shared, handle, "GET", &path, &json!({}))
        }
        "creator_hub_setup" => {
            let route_path = if args.get("link_project").and_then(Value::as_bool) == Some(false) {
                "/api/creator-hub/setup"
            } else {
                "/api/projects/active/creator-hub/setup"
            };
            let res = run_opencloud_route(shared, handle, "POST", route_path, args)?;
            if !is_action_required_value(&res) {
                record_generated_asset_files(handle, agent_name, tool, args, &res);
            }
            Ok(res)
        }
        "creator_hub_configure" => {
            run_opencloud_route(shared, handle, "POST", "/api/projects/active/creator-hub/configure", args)
        }
        "assets_authorise" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/projects/active/assets/authorise", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "assets_codegen" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/projects/active/assets/codegen", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "assets_upload" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/projects/active/assets/upload", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "products_sync" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/projects/active/products/sync", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "products_codegen" => {
            let res = run_opencloud_route(shared, handle, "POST", "/api/projects/active/products/codegen", args)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        "project_publish" => {
            if args
                .get("versionType")
                .and_then(Value::as_str)
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("Published"))
                && args.get("confirm").and_then(Value::as_bool) != Some(true)
            {
                return Err(
                    "Published makes the game live. Ask the owner first, then send confirm: true."
                        .to_string(),
                );
            }
            run_opencloud_route(shared, handle, "POST", "/api/projects/active/publish", args)
        }
        "universe_get" => {
            run_opencloud_route(shared, handle, "GET", "/api/projects/active/universe", args)
        }
        "universe_set" => {
            let payload = normalize_universe_set_args(args);
            run_opencloud_route(shared, handle, "POST", "/api/projects/active/universe", &payload)
        }
        "adapter_list" => {
            let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            let cap = args.get("capability").and_then(Value::as_str);
            Ok(crate::tool_adapters::AdapterRegistry::load(data_dir).list_json(&store, cap))
        }
        "adapter_invoke" => {
            let project_dir = handle.state.lock().unwrap().project_dir.clone();
            let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
            let store = crate::secrets::SecretStore::for_data_dir(data_dir);
            let cap = args.get("capability").and_then(Value::as_str);
            let id = args.get("adapter_id").and_then(Value::as_str);
            let params = args.get("params").unwrap_or(args);
            let ctx = crate::tool_adapters::InvokeCtx {
                data_dir,
                assets_root: &shared.assets_root,
                store: &store,
                project_dir: Some(&project_dir),
            };
            let res = crate::tool_adapters::invoke(&ctx, cap, id, params)?;
            record_generated_asset_files(handle, agent_name, tool, args, &res);
            Ok(res)
        }
        other => Err(format!("unknown Teamwork tool '{other}'")),
    }
}

fn tool_create_task(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let mut st = handle.state.lock().unwrap();
    let Some(c_idx) = st.agent_index(caller) else {
        return Err("unknown caller".to_string());
    };
    if st.roster[c_idx].role != "coordinator" {
        return Err("only the coordinator can create tasks".to_string());
    }
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("create_task requires args.title")?
        .to_string();
    let owner_raw = args
        .get("owner")
        .and_then(|v| v.as_str())
        .ok_or("create_task requires args.owner")?;
    let Some(o_idx) = st.agent_index(owner_raw) else {
        return Err(format!("unknown task owner '{owner_raw}'"));
    };
    if st.roster[o_idx].killed {
        return Err(format!("owner '{owner_raw}' is killed"));
    }
    let owner = st.roster[o_idx].name.clone();
    let verifier = args
        .get("verifier")
        .and_then(|v| v.as_str())
        .and_then(|v| st.agent_index(v).map(|i| st.roster[i].name.clone()))
        .or_else(|| {
            st.roster
                .iter()
                .find(|a| a.role == "verifier" && !a.killed)
                .map(|a| a.name.clone())
        })
        .unwrap_or_else(|| "verifier".to_string());

    let id = match args.get("id").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
        Some(explicit) => {
            if st.task_index(explicit).is_some() {
                return Err(format!("task '{explicit}' already exists"));
            }
            explicit.to_string()
        }
        None => {
            let n = st.next_task_num;
            st.next_task_num += 1;
            format!("t{n}")
        }
    };

    let files_raw: Vec<String> = args
        .get("files")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut norm_files = Vec::new();
    let mut stripped_manifest = false;
    for f in &files_raw {
        let peek = normalize_rel_path(&st.project_dir, f, false)?;
        if peek == "openro.project.json" {
            stripped_manifest = true;
            continue;
        }
        let nf = normalize_rel_path(&st.project_dir, f, true)?;
        if let Some(conflict) = st.claims.get(&nf) {
            return Err(format!(
                "file claim conflict: '{}' is already claimed by task {} (owner: {})",
                nf, conflict.task_id, conflict.owner
            ));
        }
        if !norm_files.contains(&nf) {
            norm_files.push(nf);
        }
    }

    let depends_on: Vec<String> = args
        .get("depends_on")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let kind = args
        .get("kind")
        .and_then(|v| v.as_str())
        .map(TaskKind::parse)
        .unwrap_or_else(|| {
            if norm_files.iter().any(|p| p.starts_with("src/") || p.starts_with("ui/")) {
                TaskKind::GameCode
            } else if norm_files
                .iter()
                .any(|p| p.starts_with("tools/") && (p.ends_with(".luau") || p.ends_with(".lua")))
            {
                TaskKind::Other
            } else if !norm_files.is_empty()
                && norm_files.iter().all(|p| p.starts_with("docs/") || p.ends_with(".md"))
            {
                TaskKind::Docs
            } else if owner == "art" {
                TaskKind::Art
            } else if owner == "world" {
                TaskKind::Mesh
            } else if owner == "researcher" {
                TaskKind::Research
            } else {
                TaskKind::Other
            }
        });
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or(&title)
        .to_string();

    let ts = now_ms();
    let temp_task = Task {
        id: id.clone(),
        title: title.clone(),
        description,
        kind,
        owner: owner.clone(),
        verifier,
        status: TaskStatus::Pending,
        claimed_files: norm_files.clone(),
        written_files: Vec::new(),
        depends_on,
        result_summary: None,
        verify_report: None,
        last_playtest: None,
        last_lune: None,
        commit_sha: None,
        created_at_ms: ts,
        updated_at_ms: ts,
        last_edit_ms: 0,
    };
    let initial_status = if st.deps_satisfied(&temp_task) {
        TaskStatus::InProgress
    } else {
        TaskStatus::Pending
    };
    let mut task = temp_task;
    task.status = initial_status;

    for nf in &norm_files {
        st.claims.insert(
            nf.clone(),
            FileClaim {
                path: nf.clone(),
                task_id: id.clone(),
                owner: owner.clone(),
                claimed_at_ms: ts,
            },
        );
    }
    let mut out = task.to_json();
    if stripped_manifest {
        out["note"] = json!("stripped openro.project.json from files; use project_update_manifest instead");
    }
    st.tasks.push(task);
    if st.roster[o_idx].current_task_id.is_none() {
        st.roster[o_idx].current_task_id = Some(id.clone());
    }
    st.save_tasks_file();
    st.emit_event("task", json!({ "ts_ms": ts, "task": out.clone() }));
    let _ = st.append_board(
        caller,
        BoardKind::Brief,
        &format!("@{owner} Assigned task {id}: {title} (files: {norm_files:?})"),
        Some(id),
    );
    Ok(out)
}

fn tool_claim_files(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let mut st = handle.state.lock().unwrap();
    let task_id = resolve_caller_task_id(&st, caller, args.get("task_id").and_then(|v| v.as_str()))?;
    let t_idx = st
        .task_index(&task_id)
        .ok_or_else(|| format!("unknown task '{task_id}'"))?;
    let caller_is_coord = st
        .agent_index(caller)
        .is_some_and(|i| st.roster[i].role == "coordinator");
    if !st.tasks[t_idx].owner.eq_ignore_ascii_case(caller) && !caller_is_coord {
        return Err(format!(
            "task '{}' belongs to {}, not {}",
            task_id, st.tasks[t_idx].owner, caller
        ));
    }
    if !st.tasks[t_idx].status.holds_claims() {
        return Err(format!("task '{}' is already {}", task_id, st.tasks[t_idx].status.as_str()));
    }
    let files = args
        .get("files")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty())
        .ok_or("claim_files requires non-empty args.files array")?;

    let mut to_claim = Vec::new();
    for item in files {
        let raw = item.as_str().ok_or("files entries must be strings")?;
        let nf = normalize_rel_path(&st.project_dir, raw, true)?;
        if let Some(existing) = st.claims.get(&nf) {
            if existing.task_id != task_id {
                return Err(format!(
                    "file claim conflict: '{}' is already claimed by task {} (owner: {})",
                    nf, existing.task_id, existing.owner
                ));
            }
        }
        if !to_claim.contains(&nf) {
            to_claim.push(nf);
        }
    }

    let ts = now_ms();
    let owner = st.tasks[t_idx].owner.clone();
    for nf in &to_claim {
        st.claims.insert(
            nf.clone(),
            FileClaim {
                path: nf.clone(),
                task_id: task_id.clone(),
                owner: owner.clone(),
                claimed_at_ms: ts,
            },
        );
        if !st.tasks[t_idx].claimed_files.contains(nf) {
            st.tasks[t_idx].claimed_files.push(nf.clone());
        }
    }
    st.tasks[t_idx].updated_at_ms = ts;
    let out = st.tasks[t_idx].to_json();
    st.save_tasks_file();
    st.emit_event("task", json!({ "ts_ms": ts, "task": out.clone() }));
    Ok(json!({ "claimed": to_claim, "task": out }))
}

fn tool_release_files(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let mut st = handle.state.lock().unwrap();
    let task_id = resolve_caller_task_id(&st, caller, args.get("task_id").and_then(|v| v.as_str()))?;
    let t_idx = st
        .task_index(&task_id)
        .ok_or_else(|| format!("unknown task '{task_id}'"))?;
    let caller_is_coord = st
        .agent_index(caller)
        .is_some_and(|i| st.roster[i].role == "coordinator");
    if !st.tasks[t_idx].owner.eq_ignore_ascii_case(caller) && !caller_is_coord {
        return Err(format!("only {} or coordinator may release claims on {}", st.tasks[t_idx].owner, task_id));
    }
    let targets: Vec<String> = if let Some(arr) = args.get("files").and_then(|v| v.as_array()) {
        let mut v = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                v.push(normalize_rel_path(&st.project_dir, s, true)?);
            }
        }
        v
    } else {
        st.tasks[t_idx].claimed_files.clone()
    };
    for nf in &targets {
        if st.claims.get(nf).is_some_and(|c| c.task_id == task_id) {
            st.claims.remove(nf);
        }
        st.tasks[t_idx].claimed_files.retain(|f| f != nf);
    }
    st.tasks[t_idx].updated_at_ms = now_ms();
    let out = st.tasks[t_idx].to_json();
    st.save_tasks_file();
    st.emit_event("task", json!({ "ts_ms": now_ms(), "task": out.clone() }));
    Ok(json!({ "released": targets, "task": out }))
}

fn tool_submit_task(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let mut st = handle.state.lock().unwrap();
    let task_id = resolve_caller_task_id(&st, caller, args.get("task_id").and_then(|v| v.as_str()))?;
    let t_idx = st
        .task_index(&task_id)
        .ok_or_else(|| format!("unknown task '{task_id}'"))?;
    let caller_is_coord = st
        .agent_index(caller)
        .is_some_and(|i| st.roster[i].role == "coordinator");
    if !st.tasks[t_idx].owner.eq_ignore_ascii_case(caller) && !caller_is_coord {
        return Err(format!("only {} may submit task {}", st.tasks[t_idx].owner, task_id));
    }
    if !matches!(
        st.tasks[t_idx].status,
        TaskStatus::InProgress | TaskStatus::ChangesRequested | TaskStatus::Pending
    ) {
        return Err(format!("task '{}' cannot be submitted from state '{}'", task_id, st.tasks[t_idx].status.as_str()));
    }
    let summary = args
        .get("summary")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Ready for verification.")
        .to_string();
    let ts = now_ms();
    st.tasks[t_idx].status = TaskStatus::AwaitingVerification;
    st.tasks[t_idx].result_summary = Some(summary.clone());
    st.tasks[t_idx].updated_at_ms = ts;
    let verifier = st.tasks[t_idx].verifier.clone();
    let title = st.tasks[t_idx].title.clone();
    let owner = st.tasks[t_idx].owner.clone();
    if let Some(o_idx) = st.agent_index(&owner) {
        if st.roster[o_idx].current_task_id.as_deref() == Some(task_id.as_str()) {
            st.roster[o_idx].current_task_id = st
                .tasks
                .iter()
                .find(|t| {
                    t.owner.eq_ignore_ascii_case(&owner)
                        && matches!(t.status, TaskStatus::InProgress | TaskStatus::ChangesRequested)
                })
                .or_else(|| {
                    st.tasks.iter().find(|t| {
                        t.owner.eq_ignore_ascii_case(&owner)
                            && t.id != task_id
                            && t.status.holds_claims()
                    })
                })
                .map(|t| t.id.clone());
        }
    }
    let out = st.tasks[t_idx].to_json();
    st.save_tasks_file();
    st.emit_event("task", json!({ "ts_ms": ts, "task": out.clone() }));
    let _ = st.append_board(
        caller,
        BoardKind::Done,
        &format!("@{verifier} [done] Task {task_id} ({title}) ready for verification: {summary}"),
        Some(task_id),
    );
    Ok(out)
}

fn tool_verify_task(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let passed = args
        .get("passed")
        .and_then(|v| v.as_bool())
        .ok_or("verify_task requires boolean args.passed")?;
    let notes = args
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Validate verifier role, task state, and game-code playtest gate under lock.
    let commit_plan: Option<(PathBuf, Vec<String>, String, String, String)> = {
        let mut st = handle.state.lock().unwrap();
        let Some(c_idx) = st.agent_index(caller) else {
            return Err("unknown verifier".to_string());
        };
        if st.roster[c_idx].role != "verifier" {
            return Err("Verifier gate: only a verifier agent can call verify_task".to_string());
        }
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| {
                st.tasks
                    .iter()
                    .find(|t| t.status == TaskStatus::AwaitingVerification)
                    .map(|t| t.id.clone())
            })
            .ok_or("verify_task requires args.task_id")?;
        let t_idx = st
            .task_index(&task_id)
            .ok_or_else(|| format!("unknown task '{task_id}'"))?;
        if st.tasks[t_idx].owner.eq_ignore_ascii_case(caller) {
            return Err("Verifier gate: an agent cannot verify its own task".to_string());
        }
        if st.tasks[t_idx].status != TaskStatus::AwaitingVerification {
            return Err(format!(
                "task '{}' is '{}' (must be awaiting_verification)",
                task_id,
                st.tasks[t_idx].status.as_str()
            ));
        }

        let is_code = st.tasks[t_idx].is_game_code();
        let is_lune = st.tasks[t_idx].is_lune_tool();
        let last_pt = st.tasks[t_idx].last_playtest.clone();
        let last_lune = st.tasks[t_idx].last_lune.clone();
        let last_edit = st.tasks[t_idx].last_edit_ms;

        if passed && is_code {
            match &last_pt {
                None => {
                    return Err(format!(
                        "Verifier gate refused: game-code task '{task_id}' requires a Playtest Runner pass with 0 console errors before passing. Run project_playtest first."
                    ));
                }
                Some(pt) if pt.at_ms < last_edit => {
                    return Err(format!(
                        "Verifier gate refused: game-code task '{task_id}' was edited after the last playtest. Run project_playtest again."
                    ));
                }
                Some(pt) if !pt.passed || pt.console_errors > 0 => {
                    return Err(format!(
                        "Verifier gate refused: last playtest did not pass with 0 console errors ({} error(s)). Fail the task with passed:false instead.",
                        pt.console_errors
                    ));
                }
                _ => {}
            }
        } else if passed && is_lune {
            match &last_lune {
                None => {
                    return Err(format!(
                        "Verifier gate refused: Lune tool task '{task_id}' requires a passing project_run_lune run before passing. Run project_run_lune first."
                    ));
                }
                Some(lr) if lr.at_ms < last_edit => {
                    return Err(format!(
                        "Verifier gate refused: Lune tool task '{task_id}' was edited after the last project_run_lune check. Run project_run_lune again."
                    ));
                }
                Some(lr) if !lr.passed => {
                    return Err(format!(
                        "Verifier gate refused: last project_run_lune ({}) did not pass. Fail the task with passed:false instead.",
                        lr.script
                    ));
                }
                _ => {}
            }
        }

        let ts = now_ms();
        let report = VerifyReport {
            passed,
            verifier: caller.to_string(),
            playtest_passed: last_pt.as_ref().map(|p| p.passed),
            console_errors: last_pt.as_ref().map(|p| p.console_errors).unwrap_or(0),
            notes: notes.clone(),
            at_ms: ts,
        };
        st.tasks[t_idx].verify_report = Some(report);
        st.tasks[t_idx].updated_at_ms = ts;

        if !passed {
            st.tasks[t_idx].status = TaskStatus::ChangesRequested;
            let owner = st.tasks[t_idx].owner.clone();
            let title = st.tasks[t_idx].title.clone();
            let out = st.tasks[t_idx].to_json();
            st.save_tasks_file();
            st.emit_event("task", json!({ "ts_ms": ts, "task": out.clone() }));
            let _ = st.append_board(
                caller,
                BoardKind::Verdict,
                &format!("@{owner} Verification failed for {task_id} ({title}): {notes}"),
                Some(task_id),
            );
            return Ok(out);
        }

        st.tasks[t_idx].status = TaskStatus::Done;
        // Collect files to commit and release claims.
        let mut commit_files: Vec<String> = st.tasks[t_idx].written_files.clone();
        for f in &st.tasks[t_idx].claimed_files {
            if !commit_files.contains(f) {
                commit_files.push(f.clone());
            }
        }
        let claimed = st.tasks[t_idx].claimed_files.clone();
        for f in claimed {
            if st.claims.get(&f).is_some_and(|c| c.task_id == task_id) {
                st.claims.remove(&f);
            }
        }
        let owner = st.tasks[t_idx].owner.clone();
        let owner_role = st
            .agent_index(&owner)
            .map(|i| st.roster[i].role.clone())
            .unwrap_or_else(|| owner.clone());
        let title = st.tasks[t_idx].title.clone();
        let msg = format!("openro({owner_role}): {title} [team {} task {task_id}]", st.id);

        // Unblock pending dependent tasks.
        let mut newly_ready = Vec::new();
        for idx in 0..st.tasks.len() {
            if st.tasks[idx].status == TaskStatus::Pending && st.deps_satisfied(&st.tasks[idx]) {
                st.tasks[idx].status = TaskStatus::InProgress;
                st.tasks[idx].updated_at_ms = ts;
                newly_ready.push((st.tasks[idx].id.clone(), st.tasks[idx].owner.clone(), st.tasks[idx].title.clone()));
            }
        }
        for (nid, nowner, ntitle) in newly_ready {
            let _ = st.append_board(
                "coordinator",
                BoardKind::Brief,
                &format!("@{nowner} Dependencies complete; task {nid} ({ntitle}) is ready."),
                Some(nid),
            );
        }

        // Clear current_task_id on owner if it pointed to this completed task.
        if let Some(o_idx) = st.agent_index(&owner) {
            if st.roster[o_idx].current_task_id.as_deref() == Some(task_id.as_str()) {
                st.roster[o_idx].current_task_id = st
                    .tasks
                    .iter()
                    .find(|t| t.owner == owner && t.status.holds_claims())
                    .map(|t| t.id.clone());
            }
        }
        st.save_tasks_file();
        Some((st.project_dir.clone(), commit_files, msg, task_id, title))
    };

    // Run git auto-commit outside the state lock via @openro-core's project git hook.
    let (project_dir, commit_files, commit_msg, task_id, title) = commit_plan.unwrap();
    let commit_res = projects::git_commit(&project_dir, &commit_files, &commit_msg);

    let mut st = handle.state.lock().unwrap();
    let mut commit_note = String::new();
    if let Some(t_idx) = st.task_index(&task_id) {
        match commit_res {
            Ok(Some(sha)) => {
                st.tasks[t_idx].commit_sha = Some(sha.clone());
                commit_note = format!(" commit={sha}");
            }
            Ok(None) => {}
            Err(e) => {
                commit_note = format!(" (git commit skipped: {e})");
            }
        }
        let out = st.tasks[t_idx].to_json();
        st.save_tasks_file();
        st.emit_event("task", json!({ "ts_ms": now_ms(), "task": out.clone() }));
        let _ = st.append_board(
            caller,
            BoardKind::Verdict,
            &format!("@coordinator [done] Verified task {task_id} ({title}) passed.{commit_note} {notes}"),
            Some(task_id),
        );
        return Ok(out);
    }
    Err("task disappeared during verification".to_string())
}

fn tool_finish_run(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let mut st = handle.state.lock().unwrap();
    let Some(c_idx) = st.agent_index(caller) else {
        return Err("unknown caller".to_string());
    };
    if st.roster[c_idx].role != "coordinator" {
        return Err("only the coordinator may finish the run".to_string());
    }
    let incomplete: Vec<String> = st
        .tasks
        .iter()
        .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
        .map(|t| format!("{}({})", t.id, t.status.as_str()))
        .collect();
    if !incomplete.is_empty() {
        return Err(format!(
            "cannot finish run while tasks are incomplete: {}",
            incomplete.join(", ")
        ));
    }
    let summary = args
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("All tasks verified and completed.")
        .to_string();
    Ok(finalize_completed_run(&mut st, handle, caller, &summary))
}

fn finalize_completed_run(
    st: &mut RunState,
    handle: &Arc<RunHandle>,
    actor: &str,
    summary: &str,
) -> Value {
    let ts = now_ms();
    // Transition all roster agents to Stopped before exposing RunStatus::Completed so
    // any reader observing "completed" sees consistent final roster states immediately.
    let names: Vec<String> = st.roster.iter().map(|a| a.name.clone()).collect();
    for name in &names {
        st.set_agent_state(name, AgentWorkState::Stopped);
        st.save_agent_file(name);
    }
    st.status = RunStatus::Completed;
    st.updated_at_ms = ts;
    let _ = st.append_board(actor, BoardKind::Done, &format!("[done] {summary}"), None);
    let (max_c, active_c, _, q) = handle.scheduler.snapshot();
    st.save_run_file(max_c, &q);
    let sum = st.summary_json(max_c, active_c, &q);
    st.emit_event("run", json!({ "ts_ms": ts, "status": "completed", "run": sum.clone() }));
    handle.notify_all();
    sum
}

fn resolve_caller_task_id(
    st: &RunState,
    caller: &str,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(tid) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(tid.to_string());
    }
    if let Some(idx) = st.agent_index(caller) {
        if let Some(tid) = &st.roster[idx].current_task_id {
            if st.task_index(tid).is_some_and(|i| {
                matches!(
                    st.tasks[i].status,
                    TaskStatus::InProgress | TaskStatus::ChangesRequested
                )
            }) {
                return Ok(tid.clone());
            }
        }
    }
    if let Some(t) = st.tasks.iter().find(|t| {
        t.owner.eq_ignore_ascii_case(caller)
            && matches!(t.status, TaskStatus::InProgress | TaskStatus::ChangesRequested)
    }) {
        return Ok(t.id.clone());
    }
    if let Some(idx) = st.agent_index(caller) {
        if let Some(tid) = &st.roster[idx].current_task_id {
            if st.task_index(tid).is_some_and(|i| st.tasks[i].status.holds_claims()) {
                return Ok(tid.clone());
            }
        }
    }
    st.tasks
        .iter()
        .find(|t| t.owner.eq_ignore_ascii_case(caller) && t.status.holds_claims())
        .map(|t| t.id.clone())
        .ok_or_else(|| format!("agent '{caller}' has no active task; specify args.task_id"))
}

// ---------------------------------------------------------------------------
// Project file, build, sync, playtest & asset tools
// ---------------------------------------------------------------------------

fn tool_project_list(shared: &Shared) -> Value {
    let store = shared.projects.lock().unwrap();
    let list = store.list();
    json!({
        "active": store.active,
        "projects": list.iter().map(projects::summary).collect::<Vec<_>>(),
    })
}

fn tool_project_grep(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let query = args
        .get("query")
        .or_else(|| args.get("pattern"))
        .and_then(|v| v.as_str())
        .ok_or("project_grep requires args.query")?;
    let dir = args
        .get("dir")
        .or_else(|| args.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let max_matches = args
        .get("max_matches")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    crate::agent::project_grep_files(&project_dir, query, dir, max_matches)
}

fn tool_research_roblox_game(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let target = args
        .get("target")
        .or_else(|| args.get("url"))
        .or_else(|| args.get("place_id"))
        .or_else(|| args.get("universe_id"))
        .and_then(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .ok_or("research_roblox_game requires args.target (Roblox game URL, place ID, or universe ID)")?;
    let niche = args.get("niche").and_then(|v| v.as_str());
    crate::market_taste::research_roblox_game_to_project(&project_dir, &target, niche)
}

fn resolve_web_fetch_max_chars(args: &Value) -> usize {
    args.get("max_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(6_000)
        .clamp(512, 10_000) as usize
}

fn tool_web_fetch(args: &Value) -> Result<Value, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("web_fetch requires args.url")?;
    let max_chars = resolve_web_fetch_max_chars(args);
    crate::market_taste::web_fetch(url, max_chars)
}

fn tool_web_search(args: &Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("web_search requires args.query")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(6) as usize;
    crate::market_taste::web_search(query, limit)
}

fn tool_project_files(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let rel = args
        .get("dir")
        .or_else(|| args.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let target = if rel.trim().is_empty() || rel.trim() == "." {
        project_dir.clone()
    } else {
        let norm = normalize_rel_path(&project_dir, rel, false)?;
        project_dir.join(norm)
    };
    if !target.is_dir() {
        return Err("folder does not exist".to_string());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&target)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let items: Vec<Value> = entries
        .into_iter()
        .filter(|e| !matches!(e.file_name().to_str(), Some(".git" | ".openro")))
        .map(|e| {
            let p = e.path();
            let rel_disp = p
                .strip_prefix(&project_dir)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            json!({
                "path": rel_disp,
                "type": if p.is_dir() { "dir" } else { "file" },
                "bytes": e.metadata().map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    Ok(json!({ "entries": items }))
}

fn tool_project_read_file(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let raw = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("project_read_file requires args.path")?;
    let norm = normalize_rel_path(&project_dir, raw, false)?;
    let full = project_dir.join(&norm);
    let meta = std::fs::metadata(&full).map_err(|_| format!("file '{norm}' does not exist"))?;
    if !meta.is_file() {
        return Err(format!("'{norm}' is a folder"));
    }
    if meta.len() as usize > MAX_FILE_BYTES {
        return Err(format!("'{norm}' exceeds 1 MB text read limit"));
    }
    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
    let full_text = String::from_utf8(bytes.clone()).map_err(|_| format!("'{norm}' is not UTF-8 text"))?;
    let lines: Vec<&str> = full_text.lines().collect();
    let total_lines = lines.len();
    let start_arg = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    let end_arg = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

    let (start_line, end_line, sliced) = match (start_arg, end_arg) {
        (None, None) => (1usize, total_lines, full_text),
        _ => {
            let s = start_arg.unwrap_or(1).max(1);
            let e = end_arg.unwrap_or(total_lines).min(total_lines);
            let text = if s > e || total_lines == 0 {
                String::new()
            } else {
                lines[s - 1..e].join("\n")
            };
            (s, e, text)
        }
    };

    if sliced.len() <= TEAMWORK_READ_FILE_MAX_BYTES {
        return Ok(json!({
            "path": norm,
            "content": sliced,
            "bytes": bytes.len(),
            "start_line": start_line,
            "end_line": end_line,
            "total_lines": total_lines,
            "truncated": false,
        }));
    }

    // Cap individual read outputs to ~8 KiB and guide pagination via start_line/end_line.
    let slice_lines: Vec<&str> = sliced.lines().collect();
    let mut kept_lines = Vec::new();
    let mut used = 0usize;
    for l in &slice_lines {
        let sep = if kept_lines.is_empty() { 0 } else { 1 };
        if used + sep + l.len() <= TEAMWORK_READ_FILE_MAX_BYTES {
            used += sep + l.len();
            kept_lines.push(*l);
        } else {
            break;
        }
    }
    let (shown_end, truncated_text) = if kept_lines.is_empty() {
        let end_l = if total_lines == 0 { 0 } else { start_line.min(total_lines) };
        (
            end_l,
            utf8_prefix_max_bytes(&sliced, TEAMWORK_READ_FILE_MAX_BYTES).to_string(),
        )
    } else {
        (start_line + kept_lines.len() - 1, kept_lines.join("\n"))
    };
    let next_start = (shown_end + 1).min(total_lines.max(1));
    let next_end = (next_start + 150).min(total_lines.max(next_start));
    let hint = format!(
        "Truncated to {TEAMWORK_READ_FILE_MAX_BYTES} bytes (lines {start_line}..={shown_end} of {total_lines}); pass args.start_line={next_start} and args.end_line={next_end} or use project_grep."
    );
    Ok(json!({
        "path": norm,
        "content": truncated_text,
        "bytes": bytes.len(),
        "start_line": start_line,
        "end_line": shown_end,
        "total_lines": total_lines,
        "truncated": true,
        "hint": hint,
    }))
}

fn ensure_write_claim(
    st: &mut RunState,
    caller: &str,
    norm_path: &str,
    explicit_task: Option<&str>,
) -> Result<String, String> {
    // Route edits directly to the active task owned by the same agent holding the claim,
    // avoiding self-claim conflicts when one agent owns multiple active tasks (#41).
    if let Some(existing) = st.claims.get(norm_path).cloned() {
        if existing.owner.eq_ignore_ascii_case(caller) {
            let active = st
                .task_index(&existing.task_id)
                .is_some_and(|i| st.tasks[i].status.holds_claims());
            if active {
                return Ok(existing.task_id);
            }
        }
        return Err(format!(
            "file claim conflict: '{}' is owned by task {} (owner: {})",
            norm_path, existing.task_id, existing.owner
        ));
    }

    let task_id = resolve_caller_task_id(st, caller, explicit_task)?;
    let t_idx = st
        .task_index(&task_id)
        .ok_or_else(|| format!("unknown task '{task_id}'"))?;
    if !st.tasks[t_idx].owner.eq_ignore_ascii_case(caller) {
        return Err(format!(
            "file ownership error: task '{}' is owned by {}",
            task_id, st.tasks[t_idx].owner
        ));
    }
    if !st.tasks[t_idx].status.holds_claims() {
        return Err(format!("task '{}' is {}", task_id, st.tasks[t_idx].status.as_str()));
    }
    // Auto-claim onto the caller's active task when unclaimed.
    let ts = now_ms();
    let owner = st.tasks[t_idx].owner.clone();
    st.claims.insert(
        norm_path.to_string(),
        FileClaim {
            path: norm_path.to_string(),
            task_id: task_id.clone(),
            owner,
            claimed_at_ms: ts,
        },
    );
    if !st.tasks[t_idx].claimed_files.contains(&norm_path.to_string()) {
        st.tasks[t_idx].claimed_files.push(norm_path.to_string());
    }
    Ok(task_id)
}

fn record_task_file_mutation(st: &mut RunState, task_id: &str, norm_path: &str) {
    let ts = now_ms();
    if let Some(t_idx) = st.task_index(task_id) {
        if !st.tasks[t_idx].written_files.contains(&norm_path.to_string()) {
            st.tasks[t_idx].written_files.push(norm_path.to_string());
        }
        st.tasks[t_idx].last_edit_ms = ts;
        st.tasks[t_idx].updated_at_ms = ts;
        // Editing invalidates any prior playtest or Lune check.
        st.tasks[t_idx].last_playtest = None;
        st.tasks[t_idx].last_lune = None;
        st.save_tasks_file();
    }
}

fn tool_project_write_file(
    handle: &Arc<RunHandle>,
    caller: &str,
    args: &Value,
) -> Result<Value, String> {
    let raw_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("project_write_file requires args.path")?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or("project_write_file requires args.content")?;
    if content.len() > MAX_FILE_BYTES {
        return Err("content exceeds 1 MB".to_string());
    }
    let explicit_task = args.get("task_id").and_then(|v| v.as_str());

    let (project_dir, norm, task_id) = {
        let mut st = handle.state.lock().unwrap();
        let pdir = st.project_dir.clone();
        let norm = normalize_rel_path(&pdir, raw_path, true)?;
        let tid = ensure_write_claim(&mut st, caller, &norm, explicit_task)?;
        (pdir, norm, tid)
    };

    let full = project_dir.join(&norm);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent dir: {e}"))?;
    }
    let tmp = full.with_extension("openro-tmp");
    std::fs::write(&tmp, content.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &full).map_err(|e| e.to_string())?;

    let mut st = handle.state.lock().unwrap();
    record_task_file_mutation(&mut st, &task_id, &norm);
    Ok(json!({ "path": norm, "bytes": content.len(), "task_id": task_id }))
}

fn tool_project_edit_file(
    handle: &Arc<RunHandle>,
    caller: &str,
    args: &Value,
) -> Result<Value, String> {
    let raw_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("project_edit_file requires args.path")?;
    let edits: Vec<Value> = match args.get("edits").and_then(|v| v.as_array()) {
        Some(arr) if !arr.is_empty() => arr.clone(),
        Some(_) => return Err("project_edit_file requires non-empty args.edits".to_string()),
        None => {
            let old = args
                .get("old")
                .or_else(|| args.get("old_str"))
                .or_else(|| args.get("old_string"));
            let new = args
                .get("new")
                .or_else(|| args.get("new_str"))
                .or_else(|| args.get("new_string"));
            match (old, new) {
                (Some(o), Some(n)) => vec![json!({ "old": o, "new": n })],
                _ => {
                    return Err(
                        "project_edit_file requires non-empty args.edits (or old_string/new_string)"
                            .to_string(),
                    )
                }
            }
        }
    };
    let explicit_task = args.get("task_id").and_then(|v| v.as_str());

    let (project_dir, norm, task_id) = {
        let mut st = handle.state.lock().unwrap();
        let pdir = st.project_dir.clone();
        let norm = normalize_rel_path(&pdir, raw_path, true)?;
        let tid = ensure_write_claim(&mut st, caller, &norm, explicit_task)?;
        (pdir, norm, tid)
    };

    let full = project_dir.join(&norm);
    let mut text = std::fs::read_to_string(&full)
        .map_err(|_| format!("file '{norm}' does not exist or is not UTF-8"))?;
    for (i, e) in edits.iter().enumerate() {
        let old = e
            .get("old")
            .or_else(|| e.get("old_str"))
            .or_else(|| e.get("old_string"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("edit {}: old must be non-empty", i + 1))?;
        let new = e
            .get("new")
            .or_else(|| e.get("new_str"))
            .or_else(|| e.get("new_string"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("edit {}: new must be text", i + 1))?;
        match text.matches(old).count() {
            1 => text = text.replacen(old, new, 1),
            0 => return Err(format!("edit {}: target text not found in '{norm}'", i + 1)),
            n => {
                return Err(format!(
                    "edit {}: target text appears {n} times in '{norm}'",
                    i + 1
                ))
            }
        }
    }
    if text.len() > MAX_FILE_BYTES {
        return Err("edited file exceeds 1 MB".to_string());
    }
    let tmp = full.with_extension("openro-tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &full).map_err(|e| e.to_string())?;

    let mut st = handle.state.lock().unwrap();
    record_task_file_mutation(&mut st, &task_id, &norm);
    Ok(json!({ "path": norm, "edits": edits.len(), "bytes": text.len(), "task_id": task_id }))
}

fn tool_project_delete_file(
    handle: &Arc<RunHandle>,
    caller: &str,
    args: &Value,
) -> Result<Value, String> {
    let raw_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("project_delete_file requires args.path")?;
    let explicit_task = args.get("task_id").and_then(|v| v.as_str());

    let (project_dir, norm, task_id) = {
        let mut st = handle.state.lock().unwrap();
        let pdir = st.project_dir.clone();
        let norm = normalize_rel_path(&pdir, raw_path, true)?;
        let tid = ensure_write_claim(&mut st, caller, &norm, explicit_task)?;
        (pdir, norm, tid)
    };
    let full = project_dir.join(&norm);
    if !full.exists() {
        return Err(format!("'{norm}' does not exist"));
    }
    if full.is_dir() {
        std::fs::remove_dir_all(&full).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(&full).map_err(|e| e.to_string())?;
    }
    let mut st = handle.state.lock().unwrap();
    record_task_file_mutation(&mut st, &task_id, &norm);
    Ok(json!({ "path": norm, "deleted": true }))
}

fn tool_luau_check(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("luau_check requires args.path")?;
    let source = if let Some(s) = args.get("source").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        let norm = normalize_rel_path(&project_dir, path, false)?;
        std::fs::read_to_string(project_dir.join(&norm))
            .map_err(|e| format!("read '{norm}': {e}"))?
    };
    let rep = crate::luau_check::analyze_source(path, &source);
    Ok(rep.to_json())
}

fn project_ref_for_run(handle: &Arc<RunHandle>) -> ProjectRef {
    let st = handle.state.lock().unwrap();
    let id = st
        .project_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            st.project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(projects::slug)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "project".to_string())
        });
    ProjectRef {
        id,
        dir: st.project_dir.clone(),
    }
}

fn reply_to_tool_result(res: Result<crate::project_api::Reply, crate::project_api::Reply>) -> Result<Value, String> {
    match res {
        Ok(rep) => {
            let v: Value = serde_json::from_slice(&rep.body)
                .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&rep.body).into_owned() }));
            Ok(v)
        }
        Err(rep) => {
            let v: Value = serde_json::from_slice(&rep.body).unwrap_or(Value::Null);
            let msg = v
                .get("error")
                .and_then(|e| e.as_str())
                .map(String::from)
                .unwrap_or_else(|| String::from_utf8_lossy(&rep.body).trim().to_string());
            Err(msg)
        }
    }
}

fn tool_project_build(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let engine = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let out_rel = args.get("out").and_then(|v| v.as_str());
    projects::build(&project_dir, engine, out_rel)
}

fn tool_project_scene_get(handle: &Arc<RunHandle>) -> Result<Value, String> {
    let project_ref = project_ref_for_run(handle);
    projects::load_workspace_bundle(&project_ref.dir, &project_ref.id)
}

fn tool_project_scene_mutate(
    handle: &Arc<RunHandle>,
    caller: &str,
    args: &Value,
) -> Result<Value, String> {
    let explicit_task = args.get("task_id").and_then(|v| v.as_str());
    let project_ref = project_ref_for_run(handle);
    let task_id = {
        let mut st = handle.state.lock().unwrap();
        ensure_write_claim(&mut st, caller, crate::scene::SCENE_MODEL_REL, explicit_task)?
    };
    let mut res = crate::scene::mutate_scene(&project_ref.dir, &project_ref.id, args)?;
    let modified: Vec<String> = res
        .get("modified_files")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    {
        let mut st = handle.state.lock().unwrap();
        for f in &modified {
            record_task_file_mutation(&mut st, &task_id, f);
        }
    }
    if let Some(obj) = res.as_object_mut() {
        obj.insert("ok".into(), json!(true));
        obj.insert("task_id".into(), json!(task_id));
    }
    Ok(res)
}

fn parse_format_paths_arg(args: &Value) -> Result<Option<Vec<String>>, String> {
    match args.get("paths") {
        Some(Value::Array(arr)) => {
            if arr.is_empty() {
                return Ok(None);
            }
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item.as_str().ok_or("args.paths entries must be strings")?;
                out.push(s.to_string());
            }
            Ok(Some(out))
        }
        Some(Value::Null) | None => match args.get("path") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => {
                let t = s.trim();
                if t.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(vec![t.to_string()]))
                }
            }
            Some(_) => Err("args.path must be a string".to_string()),
        },
        Some(_) => Err("args.paths must be an array of strings".to_string()),
    }
}

fn is_luau_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("luau") || e.eq_ignore_ascii_case("lua"))
}

fn collect_dir_luau_files(
    root_canon: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read dir {}: {e}", dir.display()))?;
    let mut children: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    children.sort();
    for path in children {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if matches!(name, ".git" | ".openro" | "build") {
            continue;
        }
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if !canon.starts_with(root_canon) {
            continue;
        }
        if canon.is_dir() {
            collect_dir_luau_files(root_canon, &canon, out)?;
        } else if canon.is_file() && is_luau_file_path(&canon) {
            if let Ok(rel) = canon.strip_prefix(root_canon) {
                let rel_s = rel.to_string_lossy().replace('\\', "/");
                if !rel_s.is_empty() && !out.contains(&rel_s) {
                    out.push(rel_s);
                }
            }
        }
    }
    Ok(())
}

fn find_conflicting_subclaim(st: &RunState, caller: &str, dir_rel: &str) -> Option<String> {
    let prefix = format!("{dir_rel}/");
    st.claims
        .values()
        .find(|oc| !oc.owner.eq_ignore_ascii_case(caller) && oc.path.starts_with(&prefix))
        .map(|oc| {
            format!(
                "file claim conflict: directory '{dir_rel}' contains '{}' owned by task {} (owner: {})",
                oc.path, oc.task_id, oc.owner
            )
        })
}

fn caller_owns_format_path(st: &RunState, caller: &str, norm: &str) -> Result<String, String> {
    let is_dir = st.project_dir.join(norm).is_dir();
    if let Some(c) = st.claims.get(norm) {
        if !c.owner.eq_ignore_ascii_case(caller) {
            return Err(format!(
                "file claim conflict: '{norm}' is owned by task {} (owner: {})",
                c.task_id, c.owner
            ));
        }
        if st
            .task_index(&c.task_id)
            .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims())
        {
            if is_dir {
                if let Some(err) = find_conflicting_subclaim(st, caller, norm) {
                    return Err(err);
                }
            }
            return Ok(c.task_id.clone());
        }
    }

    for c in st.claims.values() {
        if norm.starts_with(&format!("{}/", c.path)) {
            if !c.owner.eq_ignore_ascii_case(caller) {
                return Err(format!(
                    "file claim conflict: '{norm}' is under '{}' owned by task {} (owner: {})",
                    c.path, c.task_id, c.owner
                ));
            }
            if st
                .task_index(&c.task_id)
                .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims())
            {
                if is_dir {
                    if let Some(err) = find_conflicting_subclaim(st, caller, norm) {
                        return Err(err);
                    }
                }
                return Ok(c.task_id.clone());
            }
        }
    }

    for t in &st.tasks {
        if t.owner.eq_ignore_ascii_case(caller)
            && t.status.holds_claims()
            && t.claimed_files.iter().any(|f| f == norm)
        {
            if is_dir {
                if let Some(err) = find_conflicting_subclaim(st, caller, norm) {
                    return Err(err);
                }
            }
            return Ok(t.id.clone());
        }
    }

    Err(format!(
        "file ownership error: '{norm}' is not claimed by any active task owned by {caller}; claim it first with claim_files"
    ))
}

struct ScopedFormatPlan {
    project_dir: PathBuf,
    targets: Vec<String>,
    owned_files: Vec<(String, String)>,
}

fn push_scoped_owned_file(
    targets: &mut Vec<String>,
    owned_files: &mut Vec<(String, String)>,
    task_id: &str,
    rel: String,
    also_add_target: bool,
) {
    if !owned_files.iter().any(|(_, p)| p == &rel) {
        owned_files.push((task_id.to_string(), rel.clone()));
    }
    if also_add_target && !targets.contains(&rel) {
        targets.push(rel);
    }
}

fn prepare_scoped_format_plan(
    handle: &Arc<RunHandle>,
    caller: &str,
    requested: Option<&[String]>,
) -> Result<ScopedFormatPlan, String> {
    let st = handle.state.lock().unwrap();
    let project_dir = st.project_dir.clone();
    let root_canon = project_dir
        .canonicalize()
        .map_err(|e| format!("project folder {}: {e}", project_dir.display()))?;
    let mut targets: Vec<String> = Vec::new();
    let mut owned_files: Vec<(String, String)> = Vec::new();

    if let Some(list) = requested.filter(|l| !l.is_empty()) {
        for raw in list {
            if raw.chars().any(|c| c.is_control()) {
                return Err("format path cannot contain control characters".to_string());
            }
            let trimmed = raw.trim();
            if trimmed.starts_with('-') {
                return Err(format!("invalid format path '{trimmed}'"));
            }
            let norm = normalize_rel_path(&project_dir, raw, true)?;
            if norm.starts_with('-') {
                return Err(format!("invalid format path '{norm}'"));
            }
            let task_id = caller_owns_format_path(&st, caller, &norm)?;
            let full = project_dir.join(&norm);
            if !full.exists() {
                return Err(format!("format target '{norm}' does not exist"));
            }
            let canon = full
                .canonicalize()
                .map_err(|e| format!("format target '{norm}': {e}"))?;
            if !canon.starts_with(&root_canon) {
                return Err(format!("path '{norm}' must stay inside the project folder"));
            }
            if canon.is_dir() {
                if !targets.contains(&norm) {
                    targets.push(norm);
                }
                let mut dir_files = Vec::new();
                collect_dir_luau_files(&root_canon, &canon, &mut dir_files)?;
                for f in dir_files {
                    if st
                        .claims
                        .get(&f)
                        .is_some_and(|c| !c.owner.eq_ignore_ascii_case(caller))
                    {
                        continue;
                    }
                    push_scoped_owned_file(&mut targets, &mut owned_files, &task_id, f, false);
                }
            } else {
                if !is_luau_file_path(&canon) {
                    return Err(format!(
                        "format target '{norm}' must be a directory or a .luau/.lua file"
                    ));
                }
                push_scoped_owned_file(&mut targets, &mut owned_files, &task_id, norm, true);
            }
        }
        return Ok(ScopedFormatPlan {
            project_dir,
            targets,
            owned_files,
        });
    }

    // Default when args.paths is omitted: format only existing .luau/.lua files owned by caller.
    let mut candidate_claims: Vec<(String, String)> = Vec::new();
    for t in &st.tasks {
        if t.owner.eq_ignore_ascii_case(caller) && t.status.holds_claims() {
            for f in &t.claimed_files {
                if !candidate_claims.iter().any(|(_, p)| p == f) {
                    candidate_claims.push((t.id.clone(), f.clone()));
                }
            }
        }
    }
    for c in st.claims.values() {
        if c.owner.eq_ignore_ascii_case(caller)
            && st
                .task_index(&c.task_id)
                .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims())
            && !candidate_claims.iter().any(|(_, p)| p == &c.path)
        {
            candidate_claims.push((c.task_id.clone(), c.path.clone()));
        }
    }

    let mut saw_claimed_luau = false;
    for (tid, cp) in candidate_claims {
        if cp.starts_with('-') || cp == "openro.project.json" {
            continue;
        }
        if st
            .claims
            .get(&cp)
            .is_some_and(|c| !c.owner.eq_ignore_ascii_case(caller))
        {
            continue;
        }
        let Ok(norm) = normalize_rel_path(&project_dir, &cp, true) else {
            continue;
        };
        let full = project_dir.join(&norm);
        if !full.exists() {
            if is_luau_file_path(Path::new(&norm)) {
                saw_claimed_luau = true;
            }
            continue;
        }
        let Ok(canon) = full.canonicalize() else {
            continue;
        };
        if !canon.starts_with(&root_canon) {
            continue;
        }
        if canon.is_file() {
            if is_luau_file_path(&canon) {
                saw_claimed_luau = true;
                push_scoped_owned_file(&mut targets, &mut owned_files, &tid, norm, true);
            }
        } else if canon.is_dir() && find_conflicting_subclaim(&st, caller, &norm).is_none() {
            let mut dir_files = Vec::new();
            if collect_dir_luau_files(&root_canon, &canon, &mut dir_files).is_ok() {
                for f in dir_files {
                    if st
                        .claims
                        .get(&f)
                        .is_some_and(|c| !c.owner.eq_ignore_ascii_case(caller))
                    {
                        continue;
                    }
                    saw_claimed_luau = true;
                    push_scoped_owned_file(&mut targets, &mut owned_files, &tid, f, true);
                }
            }
        }
    }

    if targets.is_empty() {
        if saw_claimed_luau {
            return Err(format!(
                "claimed .luau/.lua files for '{caller}' do not exist on disk yet"
            ));
        }
        return Err(format!(
            "no claimed .luau/.lua files owned by '{caller}'; write or claim a .luau/.lua file first"
        ));
    }

    Ok(ScopedFormatPlan {
        project_dir,
        targets,
        owned_files,
    })
}

fn record_formatted_owned_files(handle: &Arc<RunHandle>, caller: &str, plan: &ScopedFormatPlan) {
    let mut st = handle.state.lock().unwrap();
    for (task_id, rel_path) in &plan.owned_files {
        let can_record = st
            .task_index(task_id)
            .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims());
        if can_record {
            record_task_file_mutation(&mut st, task_id, rel_path);
        }
    }
}

fn tool_project_format(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let requested = parse_format_paths_arg(args)?;
    let plan = prepare_scoped_format_plan(handle, caller, requested.as_deref())?;
    let res = projects::format_project(&plan.project_dir, Some(&plan.targets))?;
    if res.get("formatted").and_then(Value::as_bool) == Some(true) {
        record_formatted_owned_files(handle, caller, &plan);
    }
    Ok(res)
}

fn tool_evidence_record(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    crate::study_check::record_evidence(&project_dir, args).map_err(|e| e.message)
}

fn tool_evidence_list(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let filter = crate::study_check::EvidenceFilter {
        confidence: args
            .get("confidence")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        kind: args
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        source: args
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        limit: args.get("limit").and_then(Value::as_u64).map(|n| n as usize),
    };
    crate::study_check::list_evidence(&project_dir, &filter).map_err(|e| e.message)
}

fn tool_unknown_add(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    crate::study_check::add_unknown(&project_dir, args).map_err(|e| e.message)
}

fn tool_unknown_update(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let id = args
        .get("id")
        .or_else(|| args.get("unknown_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    crate::study_check::update_unknown(&project_dir, id, args).map_err(|e| e.message)
}

fn tool_unknowns_list(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let opt = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    crate::study_check::list_unknowns(&project_dir, opt("status"), opt("severity"), opt("domain"))
        .map_err(|e| e.message)
}

fn tool_mechanics_check(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let path = args.get("path").and_then(Value::as_str);
    let strict = match args.get("strict") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("mechanics_check args.strict must be a boolean".to_string()),
    };
    crate::study_check::check_mechanics(&project_dir, path, strict).map_err(|e| e.message)
}

fn tool_study_status(handle: &Arc<RunHandle>) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    crate::study_check::study_status(&project_dir).map_err(|e| e.message)
}

fn tool_project_check(handle: &Arc<RunHandle>, caller: &str, args: &Value) -> Result<Value, String> {
    let format = match args.get("format") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("project_check args.format must be a boolean".to_string()),
    };
    let force = match args.get("force") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("project_check args.force must be a boolean".to_string()),
    };
    let requested = parse_format_paths_arg(args)?;
    if !format {
        let project_dir = handle.state.lock().unwrap().project_dir.clone();
        return projects::check_project_with_full_options(
            &project_dir,
            false,
            requested.as_deref(),
            force,
        );
    }
    let plan = prepare_scoped_format_plan(handle, caller, requested.as_deref())?;
    let res = projects::check_project_with_full_options(
        &plan.project_dir,
        true,
        Some(&plan.targets),
        force,
    )?;
    if res.get("formatted").and_then(Value::as_bool) == Some(true) {
        record_formatted_owned_files(handle, caller, &plan);
    }
    Ok(res)
}

fn tool_project_run_lune(
    handle: &Arc<RunHandle>,
    agent_name: &str,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let script = args
        .get("script")
        .or_else(|| args.get("path"))
        .and_then(|v| v.as_str())
        .ok_or("project_run_lune requires args.script")?;
    let run_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let timeout = args.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30);
    let res = projects::run_lune(&project_dir, script, &run_args, timeout)?;
    let passed = res.get("passed").and_then(|v| v.as_bool()).unwrap_or(false);
    let norm_script = res
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or(script)
        .to_string();
    let explicit_tid = args.get("task_id").and_then(|v| v.as_str()).map(String::from);
    {
        let mut st = handle.state.lock().unwrap();
        let target_tid = explicit_tid
            .or_else(|| {
                st.tasks
                    .iter()
                    .find(|t| {
                        t.status == TaskStatus::AwaitingVerification
                            && t.verifier.eq_ignore_ascii_case(agent_name)
                            && (t.claimed_files.iter().any(|f| f == &norm_script)
                                || t.written_files.iter().any(|f| f == &norm_script))
                    })
                    .map(|t| t.id.clone())
            })
            .or_else(|| {
                st.tasks
                    .iter()
                    .find(|t| {
                        t.status == TaskStatus::AwaitingVerification
                            && t.verifier.eq_ignore_ascii_case(agent_name)
                    })
                    .map(|t| t.id.clone())
            })
            .or_else(|| {
                st.agent_index(agent_name)
                    .and_then(|i| st.roster[i].current_task_id.clone())
            });
        if let Some(tid) = target_tid {
            if let Some(t_idx) = st.task_index(&tid) {
                st.tasks[t_idx].last_lune = Some(LuneCheckRecord {
                    passed,
                    script: norm_script,
                    at_ms: now_ms(),
                });
                st.save_tasks_file();
            }
        }
    }
    Ok(res)
}

fn normalize_manifest_patch_aliases(patch: &mut Value) {
    let Some(obj) = patch.as_object_mut() else {
        return;
    };
    obj.remove("task_id");
    if let Some(arr) = obj.get_mut("products").and_then(|v| v.as_array_mut()) {
        for item in arr.iter_mut() {
            let Some(p) = item.as_object_mut() else {
                continue;
            };
            let missing_key = p
                .get("key")
                .and_then(|k| k.as_str())
                .is_none_or(|s| s.trim().is_empty());
            if missing_key {
                if let Some(id_val) = p.remove("id") {
                    if let Some(id_str) = id_val.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                        p.insert("key".into(), json!(id_str));
                    }
                }
            }
        }
    }
}

fn tool_project_update_manifest(
    handle: &Arc<RunHandle>,
    caller: &str,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let current = projects::manifest(&project_dir)?;
    let mut patch = args.get("patch").unwrap_or(args).clone();
    normalize_manifest_patch_aliases(&mut patch);
    // Agents can never self-approve product prices (`allow_price_approval = false`).
    let merged = projects::merge_manifest_for_actor(&current, &patch, false)?;
    projects::write_manifest(&project_dir, &merged)?;

    let explicit_task = args.get("task_id").and_then(|v| v.as_str());
    let mut recorded_task_id: Option<String> = None;
    {
        let mut st = handle.state.lock().unwrap();
        if let Ok(tid) = resolve_caller_task_id(&st, caller, explicit_task) {
            let can_record = st
                .task_index(&tid)
                .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims());
            if can_record {
                record_task_file_mutation(&mut st, &tid, "openro.project.json");
                recorded_task_id = Some(tid);
            }
        }
    }
    Ok(json!({ "ok": true, "manifest": merged, "task_id": recorded_task_id }))
}

fn tool_project_sync(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
    args: &Value,
) -> Result<Value, String> {
    let project_ref = project_ref_for_run(handle);
    let task_id = args.get("task_id").and_then(|v| v.as_str()).map(String::from);
    let prune = args.get("prune").and_then(|v| v.as_bool()).unwrap_or(false);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let _lease = mgr
        .studio_lease
        .acquire(agent_name, task_id, "sync", || is_cancelled(handle, agent_name, gen))?;
    handle.notify_all();
    crate::project_api::ensure_studio_place_open_for_dir(
        shared,
        &project_ref.id,
        &project_ref.dir,
        args,
    );
    let res = reply_to_tool_result(crate::studio_sync::run(shared, &project_ref, prune, dry_run));
    drop(_lease);
    handle.notify_all();
    res
}

fn tool_project_playtest(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
    args: &Value,
) -> Result<Value, String> {
    let (project_ref, target_task_id) = {
        let st = handle.state.lock().unwrap();
        let tid = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| {
                st.tasks
                    .iter()
                    .find(|t| t.status == TaskStatus::AwaitingVerification && t.verifier.eq_ignore_ascii_case(agent_name))
                    .map(|t| t.id.clone())
            })
            .or_else(|| {
                st.agent_index(agent_name)
                    .and_then(|i| st.roster[i].current_task_id.clone())
            });
        let id = st
            .project_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                st.project_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(projects::slug)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "project".to_string())
            });
        (ProjectRef { id, dir: st.project_dir.clone() }, tid)
    };

    let _lease = mgr.studio_lease.acquire(
        agent_name,
        target_task_id.clone(),
        "playtest",
        || is_cancelled(handle, agent_name, gen),
    )?;
    handle.notify_all();
    crate::project_api::ensure_studio_place_open_for_dir(
        shared,
        &project_ref.id,
        &project_ref.dir,
        args,
    );

    let mut pt_req = json!({
        "sync_first": args.get("sync_first").and_then(|v| v.as_bool()).unwrap_or(true),
    });
    for k in [
        "mode",
        "settle_ms",
        "checks",
        "screenshots",
        "allow_errors",
        "suite",
        "build_id",
        "require_core_loop",
        "force",
    ] {
        if let Some(v) = args.get(k) {
            pt_req[k] = v.clone();
        }
    }
    let res = reply_to_tool_result(crate::playtest::run(shared, &project_ref, &pt_req));
    drop(_lease);
    handle.notify_all();

    let val = res?;
    let pt_id = val["run_id"].as_str().unwrap_or("").to_string();
    let mut errors: Vec<String> = val["console"]["errors"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if let Some(tail) = val["console"]["tail"].as_array() {
        for line in tail.iter().filter_map(|v| v.as_str()) {
            let low = line.to_ascii_lowercase();
            if !low.contains("0 errors")
                && !low.contains("no errors")
                && !low.contains("without errors")
                && (low.contains("save failed:") || low.contains("bindtoclose was never bound"))
                && !errors.iter().any(|e| e == line)
            {
                errors.push(line.to_string());
            }
        }
    }
    let passed = val["passed"].as_bool().unwrap_or(false) && errors.is_empty();
    let summary = if passed {
        "Playtest passed with 0 console errors".to_string()
    } else if !errors.is_empty() {
        format!("Playtest failed with {} console error(s): {}", errors.len(), errors.join("; "))
    } else {
        "Playtest checks failed".to_string()
    };

    let at_ms = now_ms();
    if let Some(tid) = &target_task_id {
        let mut st = handle.state.lock().unwrap();
        if let Some(t_idx) = st.task_index(tid) {
            st.tasks[t_idx].last_playtest = Some(PlaytestCheckRecord {
                passed,
                console_errors: errors.len(),
                run_id: Some(pt_id.clone()),
                summary: summary.clone(),
                at_ms,
            });
            st.save_tasks_file();
        }
    }
    let mut out = json!({
        "ok": true,
        "run_id": pt_id,
        "task_id": target_task_id,
        "passed": passed,
        "console": { "errors": errors },
        "report": val.get("report").cloned().unwrap_or(Value::Null),
        "summary": summary,
    });
    for extra in [
        "evidence_id",
        "kept",
        "cached",
        "core_loop",
        "blank_screenshots",
        "visual_likeness",
    ] {
        if let Some(v) = val.get(extra) {
            if !v.is_null() {
                out[extra] = v.clone();
            }
        }
    }
    Ok(out)
}

fn collect_generated_asset_paths(tool: &str, res: &Value) -> Vec<String> {
    let mut raw = Vec::new();
    let push_str = |vec: &mut Vec<String>, v: Option<&str>| {
        if let Some(s) = v.map(str::trim).filter(|s| !s.is_empty()) {
            vec.push(s.to_string());
        }
    };
    match tool {
        "mesh_generate" | "blender_generate" => {
            if let Some(arr) = res.get("project_files").and_then(Value::as_array) {
                for item in arr {
                    push_str(&mut raw, item.as_str());
                }
            }
            push_str(&mut raw, res.get("project_manifest").and_then(Value::as_str));
            push_str(
                &mut raw,
                res.pointer("/asset_ids_luau/path").and_then(Value::as_str),
            );
        }
        "audio_generate" => {
            push_str(&mut raw, res.get("project_path").and_then(Value::as_str));
        }
        "image_process" => {
            push_str(&mut raw, res.get("path").and_then(Value::as_str));
            push_str(&mut raw, res.get("slices_updated").and_then(Value::as_str));
            raw.push("art/ART-LOG.jsonl".to_string());
        }
        "art_pack_card" => {
            for ptr in ["/art/path", "/icon/path", "/full/path", "/contact_sheet/path"] {
                push_str(&mut raw, res.pointer(ptr).and_then(Value::as_str));
            }
            raw.push("art/ART-LOG.jsonl".to_string());
        }
        "assets_codegen" | "products_codegen" => {
            push_str(&mut raw, res.get("path").and_then(Value::as_str));
        }
        "assets_upload" => {
            if res.pointer("/summary/dry_run").and_then(Value::as_bool) != Some(true) {
                raw.push("assets.lock.json".to_string());
                push_str(
                    &mut raw,
                    res.pointer("/summary/asset_ids_luau/path")
                        .and_then(Value::as_str),
                );
            }
        }
        "products_sync" => {
            if res.get("dry_run").and_then(Value::as_bool) != Some(true) {
                raw.push("openro.project.json".to_string());
                push_str(
                    &mut raw,
                    res.pointer("/product_ids_luau/path")
                        .and_then(Value::as_str),
                );
            }
        }
        "assets_authorise" => {
            raw.push("openro.project.json".to_string());
        }
        "creator_hub_setup" => {
            if !is_action_required_value(res)
                && (res.get("universeId").is_some_and(|v| !v.is_null())
                    || res.get("placeId").is_some_and(|v| !v.is_null()))
            {
                raw.push("openro.project.json".to_string());
            }
        }
        "art_generate" => {
            push_str(&mut raw, res.get("path").and_then(Value::as_str));
            push_str(&mut raw, res.get("meta_path").and_then(Value::as_str));
            raw.push("art/ART-LOG.jsonl".to_string());
        }
        "adapter_invoke" => {
            push_str(&mut raw, res.pointer("/result/path").and_then(Value::as_str));
            push_str(&mut raw, res.get("path").and_then(Value::as_str));
        }
        _ => {}
    }
    raw
}

fn record_generated_asset_files(
    handle: &Arc<RunHandle>,
    caller: &str,
    tool: &str,
    args: &Value,
    res: &Value,
) {
    let raw_paths = collect_generated_asset_paths(tool, res);
    if raw_paths.is_empty() {
        return;
    }
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let keep = projects::filter_unignored_rel_paths(&project_dir, &raw_paths);
    if keep.is_empty() {
        return;
    }
    let explicit_task = args.get("task_id").and_then(Value::as_str);
    let mut st = handle.state.lock().unwrap();
    if let Ok(tid) = resolve_caller_task_id(&st, caller, explicit_task) {
        let can_record = st
            .task_index(&tid)
            .is_some_and(|i| st.tasks[i].owner.eq_ignore_ascii_case(caller) && st.tasks[i].status.holds_claims());
        if can_record {
            for rel in keep {
                record_task_file_mutation(&mut st, &tid, &rel);
            }
        }
    }
}

fn tool_blender_generate(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let actx = crate::tool_adapters::InvokeCtx {
        data_dir,
        assets_root: &shared.assets_root,
        store: &store,
        project_dir: Some(&project_dir),
    };
    let explicit = args.get("adapter_id").and_then(Value::as_str);
    if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "mesh_gen", explicit, args) {
        return res;
    }
    crate::mesh::generate_for_project(data_dir, Some(&project_dir), args, None)
}

fn tool_audio_generate(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let actx = crate::tool_adapters::InvokeCtx {
        data_dir,
        assets_root: &shared.assets_root,
        store: &store,
        project_dir: Some(&project_dir),
    };
    let explicit = args.get("adapter_id").and_then(Value::as_str);
    if let Some(res) = crate::tool_adapters::maybe_invoke_override(&actx, "audio_gen", explicit, args) {
        return res;
    }
    crate::audio_synth::generate_audio_asset_for_project(data_dir, Some(&project_dir), args)
}

fn percent_encode_query_value(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(&mut out, "%{b:02X}");
        }
    }
    out
}

fn parse_arg_flag(args: &Value, key: &str) -> Option<bool> {
    let v = args.get(key)?;
    if let Some(b) = v.as_bool() {
        return Some(b);
    }
    if let Some(n) = v.as_i64() {
        return Some(n != 0);
    }
    v.as_str().and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    })
}

fn creator_hub_status_path(args: &Value) -> String {
    let check_login = parse_arg_flag(args, "check_login").unwrap_or(true);
    let save_creator = parse_arg_flag(args, "save_creator").unwrap_or(check_login);
    let mut params = Vec::with_capacity(3);
    if let Some(cdp) = args
        .get("cdp_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        params.push(format!("cdp_url={}", percent_encode_query_value(cdp)));
    }
    params.push(format!("check_login={}", if check_login { "1" } else { "0" }));
    params.push(format!("save_creator={}", if save_creator { "1" } else { "0" }));
    format!("/api/creator-hub/status?{}", params.join("&"))
}

fn normalize_universe_set_args(args: &Value) -> Value {
    if args.get("settings").is_some() {
        return args.clone();
    }
    let Some(obj) = args.as_object() else {
        return args.clone();
    };
    const FIELDS: &[&str] = &[
        "displayName",
        "description",
        "visibility",
        "voiceChatEnabled",
        "desktopEnabled",
        "mobileEnabled",
        "tabletEnabled",
        "consoleEnabled",
        "vrEnabled",
    ];
    if !FIELDS.iter().any(|k| obj.contains_key(*k)) {
        return args.clone();
    }
    let mut settings = serde_json::Map::new();
    for (k, v) in obj {
        if k != "task_id" {
            settings.insert(k.clone(), v.clone());
        }
    }
    json!({ "settings": settings })
}

fn run_opencloud_route(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    method: &str,
    path: &str,
    args: &Value,
) -> Result<Value, String> {
    let (route_path, route_query) = path.split_once('?').unwrap_or((path, ""));
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let mut body = args.clone();
    if route_path == "/api/art/generate" && body.get("provider").is_none() {
        if let Some(prov) = shared.config.lock().unwrap().image_provider.clone() {
            if !prov.is_empty() {
                body["provider"] = json!(prov);
            }
        }
    }
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let resolver = |_: &str| Some(project_dir.clone());
    let active_project = if route_path == "/api/creator-hub/setup" {
        None
    } else {
        Some(project_dir.clone())
    };
    let ctx = crate::opencloud::Ctx {
        data_dir,
        store,
        resolve: &resolver,
        active_project,
    };
    match crate::opencloud::route(&ctx, method, route_path, route_query, &body) {
        Some(crate::opencloud::Reply::Json(code, val)) => {
            if code < 400
                && (val.get("ok").and_then(|v| v.as_bool()) != Some(false)
                    || is_action_required_value(&val))
            {
                Ok(val)
            } else {
                Err(val
                    .get("error")
                    .or_else(|| val.get("message"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("request failed with HTTP {code}: {val}")))
            }
        }
        Some(crate::opencloud::Reply::Bytes(mime, bytes)) => {
            Ok(json!({ "ok": true, "mime": mime, "bytes": bytes.len() }))
        }
        None => Err(format!("opencloud route not available: {route_path}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_parser_extracts_reset_duration() {
        let err = "The Gemini model is out of quota for now, so nothing was built. Its quota resets in 2h13m. Wait until then.";
        let (phrase, ms) = parse_quota_error(err).expect("quota detected");
        assert_eq!(phrase.as_deref(), Some("2h13m"));
        assert_eq!(ms, (2 * 3600 + 13 * 60) * 1000);

        let (p2, ms2) = parse_quota_error("429 rate limit resets in 250ms").unwrap();
        assert_eq!(p2.as_deref(), Some("250ms"));
        assert_eq!(ms2, 250);

        assert!(parse_quota_error("connection refused").is_none());
        // Never mistake TCP port numbers containing "429" for HTTP 429 quota errors.
        assert!(parse_quota_error("http://127.0.0.1:42917/v1/chat/completions: status code 502").is_none());
        assert!(parse_quota_error("http://127.0.0.1:34298/v1/chat/completions: status code 503").is_none());
        assert!(parse_quota_error("http://127.0.0.1:42917/v1/chat/completions: status code 429").is_some());
    }

    #[test]
    fn rel_path_normalization_blocks_escapes_and_internal_dirs() {
        let dir = std::env::temp_dir().join(format!("openro-team-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            normalize_rel_path(&dir, "./src\\server//Main.server.luau", true).unwrap(),
            "src/server/Main.server.luau"
        );
        assert!(normalize_rel_path(&dir, "../escape.luau", true).is_err());
        assert!(normalize_rel_path(&dir, ".", true).is_err());
        assert!(normalize_rel_path(&dir, "./", true).is_err());
        assert!(normalize_rel_path(&dir, ".openro/teamwork/board.jsonl", true).is_err());
        assert!(normalize_rel_path(&dir, ".git/config", false).is_err());
        assert!(normalize_rel_path(&dir, "build/out.rbxlx", true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn game_code_and_lune_tool_classification() {
        let mk = |kind: TaskKind, files: &[&str]| Task {
            id: "t1".into(),
            title: "x".into(),
            description: "x".into(),
            kind,
            owner: "builder".into(),
            verifier: "verifier".into(),
            status: TaskStatus::AwaitingVerification,
            claimed_files: files.iter().map(|s| (*s).to_string()).collect(),
            written_files: Vec::new(),
            depends_on: Vec::new(),
            result_summary: None,
            verify_report: None,
            last_playtest: None,
            last_lune: None,
            commit_sha: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            last_edit_ms: 0,
        };
        let lune_only = mk(TaskKind::Other, &["tools/econ-sim/sim.luau"]);
        assert!(!lune_only.is_game_code());
        assert!(lune_only.is_lune_tool());

        // #44: explicit GameCode kind with non-empty docs/tools files still skips Studio playtest
        let lune_mislabeled = mk(TaskKind::GameCode, &["docs/ECONOMY.md", "tools/econ-sim/sim.luau"]);
        assert!(!lune_mislabeled.is_game_code());
        assert!(lune_mislabeled.is_lune_tool());

        let docs_mislabeled = mk(TaskKind::GameCode, &["docs/GDD.md"]);
        assert!(!docs_mislabeled.is_game_code());
        assert!(!docs_mislabeled.is_lune_tool());

        let empty_code = mk(TaskKind::GameCode, &[]);
        assert!(empty_code.is_game_code());

        let ui_code = mk(TaskKind::Other, &["ui/Shop.luau"]);
        assert!(ui_code.is_game_code());
        assert!(!ui_code.is_lune_tool());

        let src_code = mk(TaskKind::Docs, &["src/shared/Rates.luau"]);
        assert!(src_code.is_game_code());
        assert!(!src_code.is_lune_tool());

        let mixed = mk(TaskKind::Other, &["tools/sim.luau", "src/server/Main.server.luau"]);
        assert!(mixed.is_game_code());
        assert!(!mixed.is_lune_tool());
    }

    #[test]
    fn history_compaction_and_web_fetch_cap() {
        assert_eq!(resolve_web_fetch_max_chars(&json!({})), 6_000);
        assert_eq!(resolve_web_fetch_max_chars(&json!({ "max_chars": 50_000 })), 10_000);
        assert_eq!(resolve_web_fetch_max_chars(&json!({ "max_chars": 100 })), 512);
        assert_eq!(resolve_web_fetch_max_chars(&json!({ "max_chars": 4_000 })), 4_000);

        let big_doc = "X".repeat(2_000);
        let big_web = "W".repeat(8_000);
        let recent_doc = "R".repeat(2_000);
        let mut msgs = vec![
            json!({ "role": "system", "content": "sys" }),
            json!({ "role": "user", "content": "New board messages:\n#1 sys: go\n\nTasks:\n- t1" }),
            json!({
                "role": "assistant",
                "content": json!({
                    "tool": "project_write_file",
                    "args": { "path": "docs/OLD.md", "content": big_doc }
                }).to_string()
            }),
            json!({
                "role": "user",
                "content": format!("tool_result (web_fetch): {{\"ok\":true,\"url\":\"https://example.com\",\"text\":\"{big_web}\"}}")
            }),
            json!({
                "role": "assistant",
                "content": json!({
                    "tool": "project_write_file",
                    "args": { "path": "docs/MID.md", "content": recent_doc }
                }).to_string()
            }),
            json!({ "role": "user", "content": "tool_result (project_write_file): {\"path\":\"docs/MID.md\"}" }),
            json!({ "role": "assistant", "content": "{\"done\":true,\"summary\":\"finished\"}" }),
        ];

        compact_agent_history(&mut msgs);

        let compacted_board = msgs[1]["content"].as_str().unwrap();
        assert!(!compacted_board.contains("Tasks:\n"), "{compacted_board}");

        let old_asst = msgs[2]["content"].as_str().unwrap();
        assert!(!old_asst.contains(&"X".repeat(500)), "{old_asst}");
        assert!(old_asst.contains("compacted"), "{old_asst}");
        assert!(!old_asst.contains('\n'), "{old_asst}");

        // Within the 56 KiB / 8-turn retention window (#59), the 8 KB web_fetch result stays intact!
        let retained_web = msgs[3]["content"].as_str().unwrap();
        assert!(retained_web.starts_with("tool_result (web_fetch):"), "{retained_web}");
        assert!(retained_web.contains(&big_web));

        // Last 2 assistant turns remain uncompacted.
        assert!(msgs[4]["content"].as_str().unwrap().contains(&recent_doc));
        assert!(msgs[5]["content"].as_str().unwrap().starts_with("tool_result (project_write_file):"));

        // Pushing total tool_result bytes over the 56 KiB retention window compacts oldest turns
        // while keeping the newest turns intact and total prompt well below 96 KiB.
        let mut big_msgs = vec![json!({ "role": "system", "content": "sys" })];
        for i in 0..8 {
            big_msgs.push(json!({ "role": "assistant", "content": format!("{{\"tool\":\"project_read_file\",\"args\":{{\"path\":\"src/f{i}.luau\"}}}}") }));
            big_msgs.push(json!({
                "role": "user",
                "content": format!("tool_result (project_read_file): {{\"path\":\"src/f{i}.luau\",\"content\":\"M{i}_{}\"}}", "Z".repeat(10_000))
            }));
        }
        compact_agent_history(&mut big_msgs);
        assert!(big_msgs[2]["content"].as_str().unwrap().starts_with("[compacted tool_result"), "{}", big_msgs[2]);
        assert!(big_msgs[4]["content"].as_str().unwrap().starts_with("[compacted tool_result"), "{}", big_msgs[4]);
        let newest = big_msgs.last().unwrap()["content"].as_str().unwrap();
        assert!(newest.starts_with("tool_result (project_read_file):"), "{newest}");
        assert!(newest.contains("M7_"));
        let total_bytes: usize = big_msgs
            .iter()
            .filter_map(|m| m["content"].as_str())
            .map(str::len)
            .sum();
        assert!(total_bytes <= HISTORY_TOTAL_PROMPT_BUDGET_BYTES, "total={total_bytes}");

        // Exceeding 8 recent turns also compacts older turns even when each tool_result is small.
        let mut many_turns = vec![json!({ "role": "system", "content": "sys" })];
        for i in 0..11 {
            many_turns.push(json!({ "role": "assistant", "content": "{\"done\":false}" }));
            many_turns.push(json!({
                "role": "user",
                "content": format!("tool_result (project_read_file): {{\"path\":\"src/s{i}.luau\",\"content\":\"tiny_{i}\"}}")
            }));
        }
        compact_agent_history(&mut many_turns);
        // Turns 0, 1, 2 exceed the 8-turn retention cap -> compacted; turns 3..=10 (8 turns) kept!
        assert!(many_turns[2]["content"].as_str().unwrap().starts_with("[compacted tool_result"), "{}", many_turns[2]);
        assert!(many_turns[6]["content"].as_str().unwrap().starts_with("[compacted tool_result"), "{}", many_turns[6]);
        assert!(many_turns[8]["content"].as_str().unwrap().starts_with("tool_result (project_read_file):"), "{}", many_turns[8]);

        // Running compaction again is idempotent.
        let snapshot = msgs.clone();
        compact_agent_history(&mut msgs);
        assert_eq!(msgs, snapshot);
        let big_snapshot = big_msgs.clone();
        compact_agent_history(&mut big_msgs);
        assert_eq!(big_msgs, big_snapshot);
    }

    #[test]
    fn manifest_patch_aliases_products_id_to_key() {
        let mut patch = json!({
            "task_id": "t4",
            "products": [
                { "id": "pass_vip", "kind": "game_pass", "name": "VIP", "priceRobux": 349 },
                { "key": "prod_gems", "id": "ignored_id", "kind": "developer_product", "name": "Gems", "priceRobux": 99 }
            ]
        });
        normalize_manifest_patch_aliases(&mut patch);
        assert!(patch.get("task_id").is_none());
        assert_eq!(patch["products"][0]["key"], "pass_vip");
        assert!(patch["products"][0].get("id").is_none());
        assert_eq!(patch["products"][1]["key"], "prod_gems");
    }

    #[test]
    fn generated_asset_collection_and_asset_ids_non_runtime_classification() {
        let paths = collect_generated_asset_paths(
            "mesh_generate",
            &json!({
                "project_files": ["assets/meshes/altar.obj", "assets/meshes/altar.manifest.json"],
                "project_manifest": "assets/meshes/manifest.json",
                "asset_ids_luau": { "path": "src/shared/AssetIds.luau" }
            }),
        );
        assert_eq!(
            paths,
            vec![
                "assets/meshes/altar.obj",
                "assets/meshes/altar.manifest.json",
                "assets/meshes/manifest.json",
                "src/shared/AssetIds.luau",
            ]
        );

        let mesh_task = Task {
            id: "t1".into(),
            title: "Hub mesh".into(),
            description: "x".into(),
            kind: TaskKind::Mesh,
            owner: "art".into(),
            verifier: "verifier".into(),
            status: TaskStatus::AwaitingVerification,
            claimed_files: vec!["assets/models/**".into()],
            written_files: vec![
                "assets/meshes/altar.obj".into(),
                "assets/meshes/manifest.json".into(),
                "src/shared/AssetIds.luau".into(),
            ],
            depends_on: Vec::new(),
            result_summary: None,
            verify_report: None,
            last_playtest: None,
            last_lune: None,
            commit_sha: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            last_edit_ms: 0,
        };
        assert!(!mesh_task.is_game_code());
    }

    #[test]
    fn transient_error_classifier_and_backoff_schedule() {
        assert!(is_transient_model_error(
            "failed to start conversation: backend returned an empty conversation id"
        ));
        assert!(is_transient_model_error(
            "The model did not answer: http status 502 Bad Gateway"
        ));
        assert!(is_transient_model_error("503 Service Unavailable"));
        assert!(is_transient_model_error("stream timed out while reading"));
        assert!(is_transient_model_error("io error: connection reset by peer"));

        // Quota errors must stay handled by quota backoff, not generic transient retry.
        assert!(!is_transient_model_error("429 quota reached resets in 200ms"));
        // Do not confuse port numbers or non-transient errors with HTTP 502/503.
        assert!(!is_transient_model_error("dial tcp 127.0.0.1:50234 refused"));
        assert!(!is_transient_model_error("dial tcp 127.0.0.1:502 refused"));
        assert!(!is_transient_model_error("401 unauthorized invalid api key"));

        assert_eq!(transient_retry_backoff_ms_with_cap(0, None), 5_000);
        assert_eq!(transient_retry_backoff_ms_with_cap(1, None), 15_000);
        assert_eq!(transient_retry_backoff_ms_with_cap(0, Some(50)), 50);
    }

    #[test]
    fn turn_tool_result_cap_and_read_only_classifier() {
        assert!(!is_write_or_submit_tool("project_read_file"));
        assert!(!is_write_or_submit_tool("project_files"));
        assert!(!is_write_or_submit_tool("project_grep"));
        assert!(!is_write_or_submit_tool("luau_check"));
        assert!(!is_write_or_submit_tool("project_check"));
        assert!(!is_write_or_submit_tool("board_read"));
        assert!(!is_write_or_submit_tool("evidence_list"));
        assert!(!is_write_or_submit_tool("unknowns_list"));
        assert!(!is_write_or_submit_tool("mechanics_check"));
        assert!(!is_write_or_submit_tool("study_status"));
        assert!(is_write_or_submit_tool("project_write_file"));
        assert!(is_write_or_submit_tool("project_edit_file"));
        assert!(is_write_or_submit_tool("project_format"));
        assert!(is_write_or_submit_tool("evidence_record"));
        assert!(is_write_or_submit_tool("unknown_add"));
        assert!(is_write_or_submit_tool("unknown_update"));
        assert!(is_write_or_submit_tool("submit_task"));
        assert!(is_write_or_submit_tool("adapter_invoke"));

        // 19 x 6 KB reads (~114 KB) get capped <= 28 KiB while summarizing overflowed steps.
        let batch: Vec<String> = (0..19)
            .map(|i| {
                format!(
                    "tool_result (project_read_file): {{\"path\":\"src/f{i:02}.luau\",\"content\":\"{}\"}}",
                    "A".repeat(6_000)
                )
            })
            .collect();
        let capped = cap_turn_tool_results(&batch, false, true);
        assert!(capped.len() <= MAX_TURN_TOOL_RESULT_BYTES, "len={}", capped.len());
        assert!(capped.contains("capped at 28 KiB"));
        assert!(capped.contains("[omitted overflow:"));

        // Single huge 40 KB result also stays UTF-8 safe & <= 28 KiB.
        let huge = vec![format!("tool_result (project_files): {}", "é".repeat(20_000))];
        let capped_single = cap_turn_tool_results(&huge, false, false);
        assert!(capped_single.len() <= MAX_TURN_TOOL_RESULT_BYTES);
    }

    struct EnvRestore(Vec<(String, Option<String>)>);
    impl EnvRestore {
        fn set(pairs: &[(&str, &str)]) -> Self {
            let mut prev = Vec::new();
            for (k, v) in pairs {
                prev.push(((*k).to_string(), std::env::var(k).ok()));
                std::env::set_var(k, v);
            }
            EnvRestore(prev)
        }
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (k, old) in self.0.drain(..).rev() {
                match old {
                    Some(v) => std::env::set_var(&k, v),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }

    #[test]
    fn cloud_and_creator_hub_tools_end_to_end_gap_70() {
        use std::io::Read as _;
        use std::net::TcpListener;
        use std::sync::atomic::AtomicBool;
        use std::time::{SystemTime, UNIX_EPOCH};

        static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());
        let _env_guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_tmp = std::env::var("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let root = base_tmp.join(format!("openro-team-gap70-{}-{nanos}", std::process::id()));
        let data_dir = root.join("data");
        let secrets_dir = root.join("secrets");
        let project_dir = data_dir.join("projects").join("uacg-team");
        std::fs::create_dir_all(project_dir.join("art/store")).unwrap();
        std::fs::create_dir_all(project_dir.join("src/shared")).unwrap();
        std::fs::create_dir_all(project_dir.join("build/builds")).unwrap();

        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let png = crate::assets::decode_base64(png_b64).unwrap();
        std::fs::write(project_dir.join("art/store/icon.png"), &png).unwrap();
        std::fs::write(project_dir.join("build/game.rbxl"), b"MOCK_RBXL").unwrap();
        std::fs::write(
            project_dir.join("build/builds/b1.json"),
            json!({ "build_id": "b1", "path": "build/game.rbxl", "at": 1 }).to_string(),
        )
        .unwrap();
        std::fs::write(
            project_dir.join("openro.project.json"),
            json!({
                "version": 1,
                "name": "Untitled Anime Card Game",
                "universeId": null,
                "placeId": null,
                "products": [
                    { "key": "vip", "kind": "game_pass", "name": "VIP", "description": "VIP pass", "priceRobux": 349, "priceApproved": true }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let mock_cmd = root.join("mock_creator_hub.py");
        std::fs::write(
            &mock_cmd,
            r#"#!/usr/bin/env python3
import argparse, json, os, sys
p = argparse.ArgumentParser()
p.add_argument("--action", required=True)
p.add_argument("--cdp-url", required=True)
p.add_argument("--key-out", default=None)
p.add_argument("--check-login", action="store_true")
args = p.parse_args()
spec = json.loads(sys.stdin.read() or "{}")

if args.action == "status":
    print(json.dumps({
        "ok": True,
        "cdp_reachable": True,
        "browser_version": "Chrome/135.0-mock",
        "logged_in": args.check_login,
        "user": {"id": "1170918805", "name": "Chrono_H3X", "displayName": "Chrono"} if args.check_login else None,
        "experiences": [{"universeId": "10771116814", "rootPlaceId": "112916648108736", "name": "Untitled Anime Card Game"}] if args.check_login else []
    }))
    sys.exit(0)

if args.action == "setup":
    if spec.get("key_name") == "TRIGGER_2FA":
        print(json.dumps({
            "ok": False,
            "status": "action_required",
            "blocked_reason": "2fa_required",
            "url": "https://create.roblox.com/dashboard/credentials",
            "message": "Complete 2FA in the open browser tab."
        }))
        sys.exit(0)
    raw_secret = "TEAMWORK_OPENCLOUD_KEY_ABC9876"
    if args.key_out and spec.get("need_key", True):
        with open(args.key_out, "w", encoding="utf-8") as f:
            f.write(raw_secret)
    print(json.dumps({
        "ok": True,
        "status": "ready",
        "user": {"id": "1170918805", "name": "Chrono_H3X", "displayName": "Chrono"},
        "universeId": "10771116814",
        "placeId": "112916648108736",
        "experience_name": "Untitled Anime Card Game " + raw_secret,
        "experiences": [{"universeId": "10771116814", "rootPlaceId": "112916648108736", "name": "Untitled Anime Card Game"}]
    }))
    sys.exit(0)

if args.action == "configure":
    configured = []
    if spec.get("private_servers"):
        configured.append("private_servers")
    if spec.get("icon_abs_path"):
        configured.append("icon")
    print(json.dumps({
        "ok": True,
        "status": "ready",
        "configured": configured,
        "manual_steps_remaining": []
    }))
    sys.exit(0)
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock_cmd, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Local mock Open Cloud + OpenAI fake model HTTP endpoint.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let oc_port = listener.local_addr().unwrap().port();
        let stop_oc = Arc::new(AtomicBool::new(false));
        let stop_oc_thr = Arc::clone(&stop_oc);
        let model_turns = Arc::new(AtomicUsize::new(0));
        let model_turns_thr = Arc::clone(&model_turns);
        let oc_thread = std::thread::spawn(move || {
            while !stop_oc_thr.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let mut header_end = None;
                        let mut expected_len = 0usize;
                        loop {
                            let n = match stream.read(&mut chunk) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            buf.extend_from_slice(&chunk[..n]);
                            if header_end.is_none() {
                                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    header_end = Some(pos + 4);
                                    let head = String::from_utf8_lossy(&buf[..pos]);
                                    for line in head.lines() {
                                        if let Some((k, v)) = line.split_once(':') {
                                            if k.trim().eq_ignore_ascii_case("content-length") {
                                                expected_len = v.trim().parse().unwrap_or(0);
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(he) = header_end {
                                if buf.len() >= he + expected_len {
                                    break;
                                }
                            }
                        }
                        let req_line = String::from_utf8_lossy(&buf)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        let mut parts = req_line.split_whitespace();
                        let method = parts.next().unwrap_or("");
                        let path = parts.next().unwrap_or("");
                        let resp_body = if method == "POST" && path == "/v1/chat/completions" {
                            let step_i = model_turns_thr.fetch_add(1, Ordering::SeqCst);
                            let content = if step_i == 0 {
                                json!({
                                    "steps": [
                                        { "tool": "creator_hub_setup", "args": { "key_name": "TRIGGER_2FA" } },
                                        { "tool": "project_write_file", "args": { "path": "src/ShouldNotExist.luau", "content": "return 1" } }
                                    ]
                                })
                                .to_string()
                            } else {
                                json!({ "done": true, "summary": "Stopped on 2fa_required blocker" }).to_string()
                            };
                            json!({
                                "choices": [{
                                    "index": 0,
                                    "message": { "role": "assistant", "content": content },
                                    "finish_reason": "stop"
                                }]
                            })
                        } else if method == "POST" && path == "/assets/v1/assets" {
                            json!({
                                "done": true,
                                "path": "operations/op-1",
                                "response": {
                                    "assetId": "777001",
                                    "moderationResult": { "moderationState": "Approved" }
                                }
                            })
                        } else if method == "GET" && path.contains("/game-passes/creator") {
                            json!({ "gamePasses": [] })
                        } else if method == "POST" && path.ends_with("/game-passes") {
                            json!({ "gamePassId": "990011" })
                        } else if method == "POST" && path.contains("versionType=Saved") {
                            json!({ "versionNumber": 7 })
                        } else if method == "POST" && path.contains("versionType=Published") {
                            json!({ "versionNumber": 8 })
                        } else if method == "GET" && path.starts_with("/cloud/v2/universes/") {
                            json!({ "displayName": "Untitled Anime Card Game", "visibility": "PRIVATE" })
                        } else if method == "PATCH" && path.starts_with("/cloud/v2/universes/") {
                            json!({ "displayName": "Untitled Anime Card Game", "visibility": "PUBLIC" })
                        } else {
                            json!({ "ok": true })
                        };
                        let body_s = resp_body.to_string();
                        let raw = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body_s.len(),
                            body_s
                        );
                        let _ = stream.write_all(raw.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        let oc_base = format!("http://127.0.0.1:{oc_port}");
        let model_base = format!("http://127.0.0.1:{oc_port}/v1");
        let _env = EnvRestore::set(&[
            ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
            ("OPENRO_CREATOR_HUB_CMD", mock_cmd.to_str().unwrap()),
            ("OPENRO_OPENCLOUD_BASE", &oc_base),
        ]);

        let model = ModelClient::new(model_base.clone(), "test".into(), Some("mock-key".into()));
        let place = crate::place::LocalPlace::in_memory("TestGame".into());
        let agent = crate::agent::Agent::with_local_place(model, place);
        let shared = Arc::new(Shared::with_data_dir(
            Some(agent),
            "TestGame".into(),
            data_dir.clone(),
        ));
        {
            let mut cfg = shared.config.lock().unwrap();
            cfg.provider = Some("openai".into());
            cfg.base_url = Some(model_base);
            cfg.model_id = Some("test".into());
            cfg.api_key = Some("mock-key".into());
        }
        let mgr = Arc::clone(&shared.teamwork);

        let run_dir = project_dir.join(".openro/teamwork/team-test-70");
        std::fs::create_dir_all(run_dir.join("agents")).unwrap();
        std::fs::create_dir_all(run_dir.join("traces")).unwrap();
        let board = Board::open(run_dir.join("board.jsonl")).unwrap();

        let roster = vec![
            AgentRecord {
                name: "coordinator".into(),
                role: "coordinator".into(),
                brief: None,
                model_id: "test".into(),
                state: AgentWorkState::Waiting,
                killed: false,
                chat_id: "team-70-coord".into(),
                cursor_seq: 0,
                turns: 0,
                trace_count: 0,
                last_post_seq: None,
                current_task_id: None,
                last_error: None,
                updated_at_ms: 1,
                generation: 1,
                parked_at_seq: None,
                messages: Vec::new(),
            },
            AgentRecord {
                name: "publisher".into(),
                role: "publisher".into(),
                brief: None,
                model_id: "test".into(),
                state: AgentWorkState::Working,
                killed: false,
                chat_id: "team-70-pub".into(),
                cursor_seq: 0,
                turns: 0,
                trace_count: 0,
                last_post_seq: None,
                current_task_id: Some("t1".into()),
                last_error: None,
                updated_at_ms: 1,
                generation: 1,
                parked_at_seq: None,
                messages: Vec::new(),
            },
        ];
        let task = Task {
            id: "t1".into(),
            title: "Publish pipeline".into(),
            description: "Wire Creator Hub + Open Cloud".into(),
            kind: TaskKind::Other,
            owner: "publisher".into(),
            verifier: "verifier".into(),
            status: TaskStatus::InProgress,
            claimed_files: vec!["openro.project.json".into()],
            written_files: Vec::new(),
            depends_on: Vec::new(),
            result_summary: None,
            verify_report: None,
            last_playtest: None,
            last_lune: None,
            commit_sha: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            last_edit_ms: 0,
        };
        let state = RunState {
            id: "team-test-70".into(),
            goal: "Test Open Cloud & Creator Hub tools".into(),
            project_id: Some("uacg-team".into()),
            project_dir: project_dir.clone(),
            storage_dir: run_dir,
            status: RunStatus::Running,
            model_id: "test".into(),
            provider: "test".into(),
            created_at_ms: 1,
            updated_at_ms: 1,
            board,
            roster,
            tasks: vec![task],
            claims: BTreeMap::new(),
            next_task_num: 2,
            events: VecDeque::new(),
            next_event_seq: 1,
            stop_requested: false,
        };
        let handle = Arc::new(RunHandle {
            state: Mutex::new(state),
            wake_cv: Condvar::new(),
            sse_cv: Condvar::new(),
            scheduler: Arc::new(QuotaScheduler::new(2)),
            sse_clients: AtomicUsize::new(0),
        });

        // 1. Prompt documentation & tool classifiers
        {
            let st = handle.state.lock().unwrap();
            for idx in [0usize, 1usize] {
                let prompt = build_role_system_prompt(&st, idx);
                for tool in [
                    "opencloud_status",
                    "creator_hub_status",
                    "creator_hub_setup",
                    "creator_hub_configure",
                    "assets_authorise",
                    "assets_upload",
                    "assets_codegen",
                    "products_sync",
                    "products_codegen",
                    "project_publish",
                    "universe_get",
                    "universe_set",
                ] {
                    assert!(prompt.contains(tool), "role idx={idx} missing {tool}");
                }
                assert!(prompt.contains("confirm"));
                assert!(prompt.contains("action_required"));
            }
            let product_task = Task {
                written_files: vec!["src/shared/ProductIds.luau".into()],
                ..st.tasks[0].clone()
            };
            assert!(!product_task.is_game_code());
        }
        assert!(!is_write_or_submit_tool("opencloud_status"));
        assert!(!is_write_or_submit_tool("creator_hub_status"));
        assert!(!is_write_or_submit_tool("universe_get"));
        for w in [
            "creator_hub_setup",
            "creator_hub_configure",
            "assets_authorise",
            "assets_upload",
            "assets_codegen",
            "products_sync",
            "products_codegen",
            "project_publish",
            "universe_set",
        ] {
            assert!(is_write_or_submit_tool(w), "{w} must count as write tool");
        }

        // 2. Query string construction & non-loopback rejection
        assert_eq!(
            creator_hub_status_path(&json!({})),
            "/api/creator-hub/status?check_login=1&save_creator=1"
        );
        assert_eq!(
            creator_hub_status_path(&json!({
                "cdp_url": "http://127.0.0.1:9222",
                "check_login": false
            })),
            "/api/creator-hub/status?cdp_url=http%3A%2F%2F127.0.0.1%3A9222&check_login=0&save_creator=0"
        );
        let bad_cdp = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "creator_hub_status",
            &json!({ "cdp_url": "http://192.168.1.99:9222" }),
        );
        assert!(bad_cdp.is_err());

        // 3. Coordinator checks creator_hub_status (query check_login=1&save_creator=1) & opencloud_status
        let ch_status = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "creator_hub_status",
            &json!({}),
        )
        .expect("creator_hub_status");
        assert_eq!(ch_status["logged_in"], true);
        assert_eq!(ch_status["creator"]["userId"], "1170918805");
        let oc_before = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "opencloud_status",
            &json!({}),
        )
        .expect("opencloud_status");
        assert_eq!(oc_before["has_key"], false);
        assert_eq!(oc_before["creator"]["userId"], "1170918805");

        // 4. 2FA blocker propagates cleanly as action_required (no bypass, no saved key),
        //    and aborts remaining batched steps cleanly inside `run_agent_wake_cycle`.
        let blocker = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "creator_hub_setup",
            &json!({ "key_name": "TRIGGER_2FA" }),
        )
        .expect("action_required returned cleanly");
        assert_eq!(blocker["ok"], false);
        assert_eq!(blocker["status"], "action_required");
        assert_eq!(blocker["blocked_reason"], "2fa_required");
        assert_eq!(blocker["has_key"], false);

        run_agent_wake_cycle(&shared, &mgr, &handle, "coordinator", 1)
            .expect("wake cycle handles action_required");
        assert!(
            !project_dir.join("src/ShouldNotExist.luau").exists(),
            "action_required must abort remaining batched steps"
        );
        {
            let st = handle.state.lock().unwrap();
            let msgs = serde_json::to_string(&st.roster[0].messages).unwrap();
            assert!(msgs.contains("2fa_required"), "{msgs}");
        }

        // 4b. Unscoped setup (`link_project: false` -> POST /api/creator-hub/setup) leaves manifest null.
        let unscoped = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "creator_hub_setup",
            &json!({ "link_project": false, "reuse_universe_id": "10771116814" }),
        )
        .expect("unscoped creator_hub_setup");
        assert_eq!(unscoped["has_key"], true);
        let manifest_mid: Value = serde_json::from_str(
            &std::fs::read_to_string(project_dir.join("openro.project.json")).unwrap(),
        )
        .unwrap();
        assert!(manifest_mid["universeId"].is_null());

        // 5. Worker runs creator_hub_setup -> key stored in SecretStore, zero raw key echo, manifest linked
        let setup_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "creator_hub_setup",
            &json!({ "reuse_universe_id": "10771116814" }),
        )
        .expect("creator_hub_setup");
        assert_eq!(setup_res["has_key"], true);
        assert_eq!(setup_res["key_masked"], "••••9876");
        assert_eq!(setup_res["universeId"], "10771116814");
        assert_eq!(setup_res["placeId"], "112916648108736");
        assert!(
            !setup_res.to_string().contains("TEAMWORK_OPENCLOUD_KEY_ABC9876"),
            "raw secret must be scrubbed: {setup_res}"
        );

        let oc_after = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "opencloud_status",
            &json!({}),
        )
        .unwrap();
        assert_eq!(oc_after["has_key"], true);
        assert_eq!(oc_after["key_masked"], "••••9876");

        // 6. creator_hub_configure
        let cfg_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "creator_hub_configure",
            &json!({
                "private_servers": { "enabled": true, "price_robux": 50 },
                "icon_path": "art/store/icon.png"
            }),
        )
        .expect("creator_hub_configure");
        assert_eq!(cfg_res["configured"], json!(["private_servers", "icon"]));

        // 7. assets_authorise -> assets_upload -> assets_codegen
        let auth_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "assets_authorise",
            &json!({ "paths": ["art/**"] }),
        )
        .expect("assets_authorise");
        assert_eq!(auth_res["authorised"], json!(["art/**"]));
        let up_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "assets_upload",
            &json!({ "paths": ["art/store/icon.png"] }),
        )
        .expect("assets_upload");
        assert_eq!(up_res["summary"]["uploaded"], 1);
        let cg_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "assets_codegen",
            &json!({}),
        )
        .expect("assets_codegen");
        assert_eq!(cg_res["path"], "src/shared/AssetIds.luau");
        assert!(project_dir.join("src/shared/AssetIds.luau").is_file());

        // 8. products_sync + products_codegen
        let ps_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "products_sync",
            &json!({ "auto_codegen": true }),
        )
        .expect("products_sync");
        assert_eq!(ps_res["results"][0]["id"], "990011");
        let pc_res = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "products_codegen",
            &json!({}),
        )
        .expect("products_codegen");
        assert_eq!(pc_res["path"], "src/shared/ProductIds.luau");
        let product_ids_luau = std::fs::read_to_string(project_dir.join("src/shared/ProductIds.luau")).unwrap();
        assert!(product_ids_luau.contains("990011"));

        // 9. project_publish enforces confirm: true on Published
        let unconfirmed = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "project_publish",
            &json!({ "versionType": "Published" }),
        )
        .unwrap_err();
        assert!(unconfirmed.contains("confirm: true"), "{unconfirmed}");
        let unconfirmed_ci = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "project_publish",
            &json!({ "versionType": "published" }),
        )
        .unwrap_err();
        assert!(unconfirmed_ci.contains("confirm: true"), "{unconfirmed_ci}");

        let saved_pub = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "project_publish",
            &json!({ "versionType": "Saved" }),
        )
        .expect("Saved publish");
        assert_eq!(saved_pub["versionNumber"], 7);

        let live_pub = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "project_publish",
            &json!({ "versionType": "Published", "confirm": true }),
        )
        .expect("Confirmed Published");
        assert_eq!(live_pub["versionNumber"], 8);

        // 10. universe_get & universe_set (both nested settings and flat args)
        let ug = execute_agent_tool(&shared, &mgr, &handle, "publisher", 1, "universe_get", &json!({}))
            .expect("universe_get");
        assert_eq!(ug["universe"]["visibility"], "PRIVATE");
        let us1 = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "publisher",
            1,
            "universe_set",
            &json!({ "settings": { "visibility": "PUBLIC" } }),
        )
        .expect("universe_set nested");
        assert_eq!(us1["universe"]["visibility"], "PUBLIC");
        let us2 = execute_agent_tool(
            &shared,
            &mgr,
            &handle,
            "coordinator",
            1,
            "universe_set",
            &json!({ "visibility": "PUBLIC" }),
        )
        .expect("universe_set flat");
        assert_eq!(us2["updated"], json!(["visibility"]));

        // Worker task recorded generated/mutated files cleanly.
        {
            let st = handle.state.lock().unwrap();
            let written = &st.tasks[0].written_files;
            for f in [
                "openro.project.json",
                "assets.lock.json",
                "src/shared/AssetIds.luau",
                "src/shared/ProductIds.luau",
            ] {
                assert!(written.iter().any(|p| p == f), "missing {f} in {written:?}");
            }
        }

        stop_oc.store(true, Ordering::SeqCst);
        let _ = oc_thread.join();
        shared.teamwork.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }
}
