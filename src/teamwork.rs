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
        if self.kind == TaskKind::GameCode {
            return true;
        }
        self.claimed_files
            .iter()
            .chain(self.written_files.iter())
            .any(|p| p.starts_with("src/") || p.starts_with("ui/"))
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
            messages: v
                .get("messages")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
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
        || low.contains("429");
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

fn run_agent_wake_cycle(
    shared: &Arc<Shared>,
    mgr: &Arc<TeamworkManager>,
    handle: &Arc<RunHandle>,
    agent_name: &str,
    gen: u64,
) -> Result<(), String> {
    let mut parse_failures = 0usize;
    for _step in 0..MAX_AGENT_STEPS_PER_WAKE {
        if is_cancelled(handle, agent_name, gen) {
            return Ok(());
        }
        let (model_client, messages_snapshot, pending_cursor, turn_num) = {
            let mut st = handle.state.lock().unwrap();
            let Some(idx) = st.agent_index(agent_name) else { return Ok(()) };
            if st.roster[idx].killed || st.roster[idx].generation != gen {
                return Ok(());
            }
            st.roster[idx].parked_at_seq = None;
            let head_seq = st.board.latest_seq();
            let prompt_block = build_wake_update(&st, idx);
            if st.roster[idx].messages.is_empty() {
                let sys = build_role_system_prompt(&st, idx);
                st.roster[idx].messages.push(json!({ "role": "system", "content": sys }));
            }
            if !prompt_block.is_empty() {
                st.roster[idx].messages.push(json!({ "role": "user", "content": prompt_block }));
            } else if st.roster[idx].messages.last().and_then(|m| m.get("role").and_then(|r| r.as_str())) != Some("user") {
                st.roster[idx].messages.push(json!({
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
            let msgs = st.roster[idx].messages.clone();
            let turn = st.roster[idx].turns + 1;
            st.append_trace(
                agent_name,
                turn,
                "prompt",
                "Sending model turn",
                json!({ "cursor_seq": head_seq, "message_count": msgs.len() }),
            );
            (client, msgs, head_seq, turn)
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
                        // Roll back the unsent prompt turn so retry re-uses clean history.
                        if let Some(idx) = st.agent_index(agent_name) {
                            if st.roster[idx].messages.last().and_then(|m| m.get("role").and_then(|r| r.as_str())) == Some("user") {
                                st.roster[idx].messages.pop();
                            }
                        }
                    }
                    handle.notify_all();
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
        if !batch.is_empty() {
            batch_ran = true;
            let batched = step.get("steps").is_some();
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
                if !ok {
                    batch_aborted = true;
                    break;
                }
            }
            let combined = if batch_aborted && batched {
                format!(
                    "{}\n(batched steps aborted on first error)",
                    results_lines.join("\n")
                )
            } else {
                results_lines.join("\n")
            };
            let mut st = handle.state.lock().unwrap();
            if let Some(idx) = st.agent_index(agent_name) {
                st.roster[idx]
                    .messages
                    .push(json!({ "role": "user", "content": combined }));
                st.save_agent_file(agent_name);
            }
            handle.notify_all();
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
                    let ts = now_ms();
                    st.status = RunStatus::Completed;
                    st.updated_at_ms = ts;
                    let _ = st.append_board(agent_name, BoardKind::Done, &format!("[done] {summary}"), None);
                    let (max_c, active_c, _, q) = handle.scheduler.snapshot();
                    st.save_run_file(max_c, &q);
                    let sum = st.summary_json(max_c, active_c, &q);
                    st.emit_event("run", json!({ "ts_ms": ts, "status": "completed", "run": sum }));
                } else {
                    st.roster[idx].parked_at_seq = Some(st.board.latest_seq());
                    st.append_trace(agent_name, turn_num, "done", &summary, json!({ "summary": summary }));
                }
                st.save_agent_file(agent_name);
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
            "Split the goal into concrete non-overlapping tasks with `create_task` (specify owner, verifier, kind, and non-overlapping `files`).\n",
            "Call `finish_run` once all tasks are `done`.\n",
            "Tools: create_task, board_post, board_read, project_files, project_read_file, finish_run."
        ),
        "verifier" => concat!(
            "You are the Teamwork verifier.\n",
            "Inspect submitted work (`project_read_file`, `luau_check`, `project_check`, `project_run_lune`, `project_build`).\n",
            "For game_code tasks (or `src/` / `ui/` changes), you MUST run `project_playtest` with `{\"task_id\":\"<id>\"}` and confirm 0 console errors BEFORE calling `verify_task` with `passed: true`.\n",
            "For `tools/**/*.luau` tasks (not touching `src/` or `ui/`), run `project_run_lune` (confirm `passed: true`) instead of `project_playtest` BEFORE calling `verify_task(passed=true)`.\n",
            "If anything fails, call `verify_task` with `passed: false` and clear failure notes."
        ),
        _ => concat!(
            "You are a Teamwork specialist worker.\n",
            "Claim every file you edit with `claim_files` (two active tasks may never claim or edit the same file).\n",
            "Use `project_files`, `project_read_file`, `project_write_file`, `project_edit_file`, `project_delete_file`, `project_update_manifest`, `luau_check`, `project_check`, `project_run_lune`, `project_build`, `project_sync`, `project_playtest`, `reference_search`, `reference_select`, `art_generate`, `art_approve`, `image_process`, `art_pack_card`, `mesh_generate`, `blender_generate`, `audio_generate`, `assets_codegen`, `assets_upload`.\n",
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

fn build_wake_update(st: &RunState, idx: usize) -> String {
    let a = &st.roster[idx];
    let unread = st.board.since(a.cursor_seq);
    let mut parts = Vec::new();
    if !unread.is_empty() {
        let lines: Vec<String> = unread
            .iter()
            .map(|m| format!("#{} {}: {}", m.seq, m.author, m.text))
            .collect();
        parts.push(format!("New board messages:\n{}", lines.join("\n")));
    }
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
    if !relevant_tasks.is_empty() {
        parts.push(format!("Tasks:\n{}", relevant_tasks.join("\n")));
    }
    parts.join("\n\n")
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
        "project_write_file" => tool_project_write_file(handle, agent_name, args),
        "project_edit_file" => tool_project_edit_file(handle, agent_name, args),
        "project_delete_file" => tool_project_delete_file(handle, agent_name, args),
        "luau_check" => tool_luau_check(handle, args),
        "project_check" => tool_project_check(handle),
        "project_run_lune" => tool_project_run_lune(handle, agent_name, args),
        "project_update_manifest" => tool_project_update_manifest(handle, args),
        "project_build" => tool_project_build(handle, args),
        "project_sync" => tool_project_sync(shared, mgr, handle, agent_name, gen, args),
        "project_playtest" => {
            tool_project_playtest(shared, mgr, handle, agent_name, gen, args)
        }
        "blender_generate" | "mesh_generate" => tool_blender_generate(shared, handle, args),
        "audio_generate" => tool_audio_generate(shared, handle, args),
        "reference_search" => run_opencloud_route(shared, handle, "POST", "/api/art/references/search", args),
        "reference_select" => run_opencloud_route(shared, handle, "POST", "/api/art/references/select", args),
        "art_generate" => run_opencloud_route(shared, handle, "POST", "/api/art/generate", args),
        "art_approve" => run_opencloud_route(shared, handle, "POST", "/api/art/approve", args),
        "image_process" => run_opencloud_route(shared, handle, "POST", "/api/art/process", args),
        "art_pack_card" => run_opencloud_route(shared, handle, "POST", "/api/art/pack-card", args),
        "assets_codegen" => run_opencloud_route(shared, handle, "POST", "/api/projects/active/assets/codegen", args),
        "assets_upload" => run_opencloud_route(shared, handle, "POST", "/api/projects/active/assets/upload", args),
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
    for f in &files_raw {
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
    let out = task.to_json();
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
    let ts = now_ms();
    st.status = RunStatus::Completed;
    st.updated_at_ms = ts;
    let _ = st.append_board(caller, BoardKind::Done, &format!("[done] {summary}"), None);
    let (max_c, active_c, _, q) = handle.scheduler.snapshot();
    st.save_run_file(max_c, &q);
    let sum = st.summary_json(max_c, active_c, &q);
    st.emit_event("run", json!({ "ts_ms": ts, "status": "completed", "run": sum.clone() }));
    Ok(sum)
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

fn tool_project_files(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let rel = args.get("dir").and_then(|v| v.as_str()).unwrap_or("");
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
    let content = String::from_utf8(bytes.clone()).map_err(|_| format!("'{norm}' is not UTF-8 text"))?;
    Ok(json!({ "path": norm, "content": content, "bytes": bytes.len() }))
}

fn ensure_write_claim(
    st: &mut RunState,
    caller: &str,
    norm_path: &str,
    explicit_task: Option<&str>,
) -> Result<String, String> {
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
    if let Some(existing) = st.claims.get(norm_path) {
        if existing.task_id != task_id {
            return Err(format!(
                "file claim conflict: '{}' is owned by task {} (owner: {})",
                norm_path, existing.task_id, existing.owner
            ));
        }
    } else {
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
    let edits = args
        .get("edits")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty())
        .ok_or("project_edit_file requires non-empty args.edits")?;
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
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("edit {}: old must be non-empty", i + 1))?;
        let new = e
            .get("new")
            .or_else(|| e.get("new_str"))
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

fn tool_project_check(handle: &Arc<RunHandle>) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    projects::check_project(&project_dir)
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

fn tool_project_update_manifest(handle: &Arc<RunHandle>, args: &Value) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let current = projects::manifest(&project_dir)?;
    let patch = args.get("patch").unwrap_or(args);
    // Agents can never self-approve product prices (`allow_price_approval = false`).
    let merged = projects::merge_manifest_for_actor(&current, patch, false)?;
    projects::write_manifest(&project_dir, &merged)?;
    Ok(json!({ "ok": true, "manifest": merged }))
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

    let mut pt_req = json!({
        "sync_first": args.get("sync_first").and_then(|v| v.as_bool()).unwrap_or(true),
    });
    for k in ["mode", "settle_ms", "checks", "screenshots", "allow_errors", "suite", "build_id"] {
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
    Ok(json!({
        "ok": true,
        "run_id": pt_id,
        "task_id": target_task_id,
        "passed": passed,
        "console": { "errors": errors },
        "report": val.get("report").cloned().unwrap_or(Value::Null),
        "summary": summary,
    }))
}

fn tool_blender_generate(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    crate::mesh::generate_for_project(data_dir, Some(&project_dir), args, None)
}

fn tool_audio_generate(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    crate::audio_synth::generate_audio_asset_for_project(data_dir, Some(&project_dir), args)
}

fn run_opencloud_route(
    shared: &Arc<Shared>,
    handle: &Arc<RunHandle>,
    method: &str,
    path: &str,
    args: &Value,
) -> Result<Value, String> {
    let project_dir = handle.state.lock().unwrap().project_dir.clone();
    let mut body = args.clone();
    if path == "/api/art/generate" && body.get("provider").is_none() {
        if let Some(prov) = shared.config.lock().unwrap().image_provider.clone() {
            if !prov.is_empty() {
                body["provider"] = json!(prov);
            }
        }
    }
    let data_dir = shared.data_dir.as_deref().unwrap_or(&shared.assets_root);
    let store = crate::secrets::SecretStore::for_data_dir(data_dir);
    let resolver = |_: &str| Some(project_dir.clone());
    let ctx = crate::opencloud::Ctx {
        data_dir,
        store,
        resolve: &resolver,
        active_project: Some(project_dir.clone()),
    };
    match crate::opencloud::route(&ctx, method, path, "", &body) {
        Some(crate::opencloud::Reply::Json(code, val)) => {
            if code < 400 && val.get("ok").and_then(|v| v.as_bool()) != Some(false) {
                Ok(val)
            } else {
                Err(val
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("request failed with HTTP {code}: {val}")))
            }
        }
        Some(crate::opencloud::Reply::Bytes(mime, bytes)) => {
            Ok(json!({ "ok": true, "mime": mime, "bytes": bytes.len() }))
        }
        None => Err(format!("opencloud route not available: {path}")),
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
}
