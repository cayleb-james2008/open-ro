//! Versioned Place Snapshots, Instance/Script Build Diffing, and Defensive
//! Roblox Studio `.rbxlx` XML Import Parser.
//!
//! Snapshots persist atomically to `data_dir/snapshots.json` (bounded to
//! `MAX_SNAPSHOTS` entries) so creators can inspect instance-level diffs,
//! restore any earlier build cleanly, or export any historical snapshot as
//! genuine `<roblox version="4">` XML.

use crate::place::serialize_rbxlx;
use crate::server::backup_corrupt;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cap persisted snapshot history so memory and disk stay bounded at scale.
pub const MAX_SNAPSHOTS: usize = 64;
/// Cap persisted staged diff proposals so memory and disk stay bounded at scale.
pub const MAX_PROPOSALS: usize = 48;

static SNAP_COUNTER: AtomicU64 = AtomicU64::new(0);
static PROPOSAL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_snapshot_id() -> String {
    let n = SNAP_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("snap-{}-{n}", now_ms())
}

fn new_proposal_id() -> String {
    let n = PROPOSAL_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("prop-{}-{n}", now_ms())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceDiff {
    pub added_instances: Vec<String>,
    pub modified_instances: Vec<String>,
    pub removed_instances: Vec<String>,
    pub added_scripts: Vec<String>,
    pub modified_scripts: Vec<String>,
    pub removed_scripts: Vec<String>,
}

impl PlaceDiff {
    pub fn total_changes(&self) -> usize {
        self.added_instances.len()
            + self.modified_instances.len()
            + self.removed_instances.len()
            + self.added_scripts.len()
            + self.modified_scripts.len()
            + self.removed_scripts.len()
    }

    pub fn summary(&self) -> String {
        let added = self.added_instances.len() + self.added_scripts.len();
        let modified = self.modified_instances.len() + self.modified_scripts.len();
        let removed = self.removed_instances.len() + self.removed_scripts.len();
        if added == 0 && modified == 0 && removed == 0 {
            return "No changes".to_string();
        }
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("+{added} added"));
        }
        if modified > 0 {
            parts.push(format!("~{modified} modified"));
        }
        if removed > 0 {
            parts.push(format!("-{removed} removed"));
        }
        parts.join(", ")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "added_instances": self.added_instances,
            "modified_instances": self.modified_instances,
            "removed_instances": self.removed_instances,
            "added_scripts": self.added_scripts,
            "modified_scripts": self.modified_scripts,
            "removed_scripts": self.removed_scripts,
            "total_changes": self.total_changes(),
            "summary": self.summary(),
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let list = |k: &str| -> Option<Vec<String>> {
            v.get(k)?
                .as_array()?
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect()
        };
        Some(PlaceDiff {
            added_instances: list("added_instances")?,
            modified_instances: list("modified_instances")?,
            removed_instances: list("removed_instances")?,
            added_scripts: list("added_scripts")?,
            modified_scripts: list("modified_scripts")?,
            removed_scripts: list("removed_scripts")?,
        })
    }
}

/// Compute an exact sorted instance- and script-level diff between two place trees.
pub fn compute_diff(before: &Value, after: &Value) -> PlaceDiff {
    let mut diff = PlaceDiff::default();

    let empty = Map::new();
    let b_inst = before
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_inst = after
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    for (k, av) in a_inst {
        match b_inst.get(k) {
            None => diff.added_instances.push(k.clone()),
            Some(bv) if bv != av => diff.modified_instances.push(k.clone()),
            Some(_) => {}
        }
    }
    for k in b_inst.keys() {
        if !a_inst.contains_key(k) {
            diff.removed_instances.push(k.clone());
        }
    }

    let b_sc = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_sc = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    for (k, av) in a_sc {
        match b_sc.get(k) {
            None => diff.added_scripts.push(k.clone()),
            Some(bv) if bv != av => diff.modified_scripts.push(k.clone()),
            Some(_) => {}
        }
    }
    for k in b_sc.keys() {
        if !a_sc.contains_key(k) {
            diff.removed_scripts.push(k.clone());
        }
    }

    diff.added_instances.sort();
    diff.modified_instances.sort();
    diff.removed_instances.sort();
    diff.added_scripts.sort();
    diff.modified_scripts.sort();
    diff.removed_scripts.sort();
    diff
}

#[derive(Clone, Debug)]
pub struct PlaceSnapshot {
    pub id: String,
    pub created_at_ms: u64,
    pub label: String,
    pub source: String,
    pub chat_id: Option<String>,
    pub instance_count: usize,
    pub script_count: usize,
    pub tree: Value,
    pub diff: PlaceDiff,
}

impl PlaceSnapshot {
    pub fn summary_json(&self) -> Value {
        json!({
            "id": self.id,
            "created_at_ms": self.created_at_ms,
            "label": self.label,
            "source": self.source,
            "chat_id": self.chat_id,
            "instance_count": self.instance_count,
            "script_count": self.script_count,
            "diff": self.diff.to_json(),
        })
    }

    pub fn full_json(&self) -> Value {
        let mut obj = self.summary_json();
        obj["tree"] = self.tree.clone();
        obj
    }

    fn from_json(v: &Value) -> Option<Self> {
        let id = v.get("id")?.as_str()?.to_string();
        let created_at_ms = v.get("created_at_ms")?.as_u64()?;
        let label = v.get("label")?.as_str()?.to_string();
        let source = v.get("source")?.as_str()?.to_string();
        let chat_id = match v.get("chat_id") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Null) | None => None,
            _ => return None,
        };
        let tree = v.get("tree")?.clone();
        if !tree.is_object() {
            return None;
        }
        let instance_count = tree
            .get("instances")
            .and_then(|x| x.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let script_count = tree
            .get("scripts")
            .and_then(|x| x.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let diff = v
            .get("diff")
            .and_then(PlaceDiff::from_json)
            .unwrap_or_default();
        Some(PlaceSnapshot {
            id,
            created_at_ms,
            label,
            source,
            chat_id,
            instance_count,
            script_count,
            tree,
            diff,
        })
    }
}

pub struct SnapshotStore {
    snapshots: Vec<PlaceSnapshot>,
    path: Option<PathBuf>,
    pub recovered: Vec<String>,
    cached_summary_bytes: Option<std::sync::Arc<Vec<u8>>>,
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl SnapshotStore {
    pub fn in_memory() -> Self {
        SnapshotStore {
            snapshots: Vec::new(),
            path: None,
            recovered: Vec::new(),
            cached_summary_bytes: None,
        }
    }

    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("snapshots.json");
        let mut recovered = Vec::new();
        let snapshots = load_snapshots(&path, &mut recovered);
        SnapshotStore {
            snapshots,
            path: Some(path),
            recovered,
            cached_summary_bytes: None,
        }
    }

    pub fn record(
        &mut self,
        label: impl Into<String>,
        source: impl Into<String>,
        chat_id: Option<String>,
        tree: Value,
        explicit_diff: Option<PlaceDiff>,
    ) -> PlaceSnapshot {
        let normalized = normalize_tree(&tree);
        let diff = explicit_diff.unwrap_or_else(|| {
            let empty_tree = json!({ "instances": {}, "scripts": {} });
            let prev = self
                .snapshots
                .last()
                .map(|s| &s.tree)
                .unwrap_or(&empty_tree);
            compute_diff(prev, &normalized)
        });
        let instance_count = normalized["instances"]
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0);
        let script_count = normalized["scripts"]
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0);
        let snap = PlaceSnapshot {
            id: new_snapshot_id(),
            created_at_ms: now_ms(),
            label: label.into(),
            source: source.into(),
            chat_id,
            instance_count,
            script_count,
            tree: normalized,
            diff,
        };
        self.snapshots.push(snap.clone());
        if self.snapshots.len() > MAX_SNAPSHOTS {
            let drop_n = self.snapshots.len() - MAX_SNAPSHOTS;
            self.snapshots.drain(0..drop_n);
        }
        self.cached_summary_bytes = None;
        self.persist();
        snap
    }

    pub fn summary_response_bytes(&mut self) -> std::sync::Arc<Vec<u8>> {
        if let Some(cached) = &self.cached_summary_bytes {
            return std::sync::Arc::clone(cached);
        }
        let bytes = std::sync::Arc::new(
            json!({
                "ok": true,
                "snapshots": self.list(false),
                "latest_diff": self.latest_diff(),
            })
            .to_string()
            .into_bytes(),
        );
        self.cached_summary_bytes = Some(std::sync::Arc::clone(&bytes));
        bytes
    }

    pub fn list(&self, include_tree: bool) -> Vec<Value> {
        self.snapshots
            .iter()
            .rev()
            .map(|s| {
                if include_tree {
                    s.full_json()
                } else {
                    s.summary_json()
                }
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&PlaceSnapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    pub fn latest_diff(&self) -> Value {
        match self.snapshots.last() {
            Some(s) => json!({
                "snapshot_id": s.id,
                "label": s.label,
                "created_at_ms": s.created_at_ms,
                "diff": s.diff.to_json(),
            }),
            None => json!({
                "snapshot_id": Value::Null,
                "label": "No builds yet",
                "created_at_ms": 0,
                "diff": PlaceDiff::default().to_json(),
            }),
        }
    }

    pub fn export_rbxlx(&self, id: &str, game_name: &str) -> Option<String> {
        let snap = self.get(id)?;
        Some(serialize_rbxlx(game_name, &snap.tree))
    }

    fn persist(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = json!({
            "snapshots": self.snapshots.iter().map(|s| s.full_json()).collect::<Vec<_>>(),
        });
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn load_snapshots(path: &Path, recovered: &mut Vec<String>) -> Vec<PlaceSnapshot> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let parsed = (|| -> Option<Vec<PlaceSnapshot>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let arr = v.get("snapshots")?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            out.push(PlaceSnapshot::from_json(item)?);
        }
        Some(out)
    })();
    match parsed {
        Some(list) => list,
        None => {
            if let Some(bak) = backup_corrupt(path) {
                recovered.push(
                    bak.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Vec::new()
        }
    }
}

pub fn normalize_tree(tree: &Value) -> Value {
    let instances = tree
        .get("instances")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let scripts = tree
        .get("scripts")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    json!({
        "instances": Value::Object(instances),
        "scripts": Value::Object(scripts),
    })
}

/// Parse a Roblox Studio `.rbxlx` XML document into a normalized place tree
/// using the canonical fuzz-hardened parser in `crate::place`.
pub fn parse_rbxlx_xml(xml: &str) -> Result<Value, String> {
    crate::place::parse_rbxlx_xml(xml).map(|p| p.tree_json())
}

// ---------------------------------------------------------------------------
// Unified Line Diff Engine & Staged Diff Proposals (RoPilot v0.10.0)
// ---------------------------------------------------------------------------

/// One line inside a unified diff hunk (`"context"`, `"add"`, or `"remove"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: String,
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

impl DiffLine {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind,
            "text": self.text,
            "old_line": self.old_line,
            "new_line": self.new_line,
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let kind = v.get("kind")?.as_str()?.to_string();
        if !matches!(kind.as_str(), "context" | "add" | "remove") {
            return None;
        }
        let text = v.get("text")?.as_str()?.to_string();
        let old_line = v
            .get("old_line")
            .and_then(|x| x.as_u64())
            .map(|n| n as usize);
        let new_line = v
            .get("new_line")
            .and_then(|x| x.as_u64())
            .map(|n| n as usize);
        Some(DiffLine {
            kind,
            text,
            old_line,
            new_line,
        })
    }
}

/// One unified diff hunk with 1-indexed start positions and line operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

impl DiffHunk {
    pub fn to_json(&self) -> Value {
        json!({
            "old_start": self.old_start,
            "old_count": self.old_count,
            "new_start": self.new_start,
            "new_count": self.new_count,
            "lines": self.lines.iter().map(DiffLine::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let old_start = v.get("old_start")?.as_u64()? as usize;
        let old_count = v
            .get("old_count")
            .and_then(|x| x.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let new_start = v.get("new_start")?.as_u64()? as usize;
        let new_count = v
            .get("new_count")
            .and_then(|x| x.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let lines = v
            .get("lines")?
            .as_array()?
            .iter()
            .map(DiffLine::from_json)
            .collect::<Option<Vec<_>>>()?;
        Some(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        })
    }
}

/// Reviewable unified diff for a single Luau script (`added`, `modified`, or `removed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFileDiff {
    pub path: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    pub before_source: String,
    pub after_source: String,
    pub hunks: Vec<DiffHunk>,
}

impl ScriptFileDiff {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "status": self.status,
            "additions": self.additions,
            "deletions": self.deletions,
            "before_source": self.before_source,
            "after_source": self.after_source,
            "hunks": self.hunks.iter().map(DiffHunk::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let path = v.get("path")?.as_str()?.to_string();
        let status = v.get("status")?.as_str()?.to_string();
        let additions = v.get("additions")?.as_u64()? as usize;
        let deletions = v.get("deletions")?.as_u64()? as usize;
        let before_source = v.get("before_source")?.as_str()?.to_string();
        let after_source = v.get("after_source")?.as_str()?.to_string();
        let hunks = v
            .get("hunks")?
            .as_array()?
            .iter()
            .map(DiffHunk::from_json)
            .collect::<Option<Vec<_>>>()?;
        Some(ScriptFileDiff {
            path,
            status,
            additions,
            deletions,
            before_source,
            after_source,
            hunks,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RawOp {
    Context(usize, usize), // (old_idx_0, new_idx_0)
    Remove(usize),         // old_idx_0
    Add(usize),            // new_idx_0
}

fn split_lines_owned(src: &str) -> Vec<&str> {
    if src.is_empty() {
        Vec::new()
    } else {
        src.split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect()
    }
}

/// Compute unified diff hunks between `before_source` and `after_source` with 3 context lines.
pub fn compute_script_diff(
    path: &str,
    status: &str,
    before_source: &str,
    after_source: &str,
) -> ScriptFileDiff {
    let old_lines = split_lines_owned(before_source);
    let new_lines = split_lines_owned(after_source);

    // Fast path for pure additions and pure removals.
    if old_lines.is_empty() && !new_lines.is_empty() {
        let lines: Vec<DiffLine> = new_lines
            .iter()
            .enumerate()
            .map(|(i, text)| DiffLine {
                kind: "add".to_string(),
                text: (*text).to_string(),
                old_line: None,
                new_line: Some(i + 1),
            })
            .collect();
        let additions = lines.len();
        return ScriptFileDiff {
            path: path.to_string(),
            status: status.to_string(),
            additions,
            deletions: 0,
            before_source: before_source.to_string(),
            after_source: after_source.to_string(),
            hunks: vec![DiffHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: additions,
                lines,
            }],
        };
    }
    if new_lines.is_empty() && !old_lines.is_empty() {
        let lines: Vec<DiffLine> = old_lines
            .iter()
            .enumerate()
            .map(|(i, text)| DiffLine {
                kind: "remove".to_string(),
                text: (*text).to_string(),
                old_line: Some(i + 1),
                new_line: None,
            })
            .collect();
        let deletions = lines.len();
        return ScriptFileDiff {
            path: path.to_string(),
            status: status.to_string(),
            additions: 0,
            deletions,
            before_source: before_source.to_string(),
            after_source: after_source.to_string(),
            hunks: vec![DiffHunk {
                old_start: 1,
                old_count: deletions,
                new_start: 0,
                new_count: 0,
                lines,
            }],
        };
    }
    if old_lines == new_lines {
        return ScriptFileDiff {
            path: path.to_string(),
            status: status.to_string(),
            additions: 0,
            deletions: 0,
            before_source: before_source.to_string(),
            after_source: after_source.to_string(),
            hunks: Vec::new(),
        };
    }

    // Trim shared prefix and suffix for O(N) performance on typical edits.
    let mut prefix = 0usize;
    let min_len = old_lines.len().min(new_lines.len());
    while prefix < min_len && old_lines[prefix] == new_lines[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < (old_lines.len() - prefix).min(new_lines.len() - prefix)
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid_end = old_lines.len() - suffix;
    let new_mid_end = new_lines.len() - suffix;
    let m = old_mid_end - prefix;
    let n = new_mid_end - prefix;

    let mut ops: Vec<RawOp> = Vec::with_capacity(old_lines.len() + new_lines.len());
    for i in 0..prefix {
        ops.push(RawOp::Context(i, i));
    }

    // Exact LCS DP on the changed window when bounded; fallback to replace hunk for massive rewrites.
    if m.saturating_mul(n) <= 250_000 {
        let cols = n + 1;
        let mut dp = vec![0u32; (m + 1) * cols];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                dp[i * cols + j] = if old_lines[prefix + i] == new_lines[prefix + j] {
                    dp[(i + 1) * cols + (j + 1)] + 1
                } else {
                    dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                };
            }
        }
        let mut i = 0usize;
        let mut j = 0usize;
        while i < m && j < n {
            if old_lines[prefix + i] == new_lines[prefix + j] {
                ops.push(RawOp::Context(prefix + i, prefix + j));
                i += 1;
                j += 1;
            } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                ops.push(RawOp::Remove(prefix + i));
                i += 1;
            } else {
                ops.push(RawOp::Add(prefix + j));
                j += 1;
            }
        }
        while i < m {
            ops.push(RawOp::Remove(prefix + i));
            i += 1;
        }
        while j < n {
            ops.push(RawOp::Add(prefix + j));
            j += 1;
        }
    } else {
        for i in prefix..old_mid_end {
            ops.push(RawOp::Remove(i));
        }
        for j in prefix..new_mid_end {
            ops.push(RawOp::Add(j));
        }
    }

    for k in 0..suffix {
        ops.push(RawOp::Context(old_mid_end + k, new_mid_end + k));
    }

    // Group ops into unified hunks with 3 lines of surrounding context.
    const CONTEXT: usize = 3;
    let change_indices: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter_map(|(idx, op)| match op {
            RawOp::Context(_, _) => None,
            RawOp::Remove(_) | RawOp::Add(_) => Some(idx),
        })
        .collect();

    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut hunks = Vec::new();
    let mut c_idx = 0usize;
    while c_idx < change_indices.len() {
        let start = change_indices[c_idx].saturating_sub(CONTEXT);
        let mut end = (change_indices[c_idx] + CONTEXT + 1).min(ops.len());
        c_idx += 1;
        while c_idx < change_indices.len() && change_indices[c_idx] <= end + CONTEXT {
            end = (change_indices[c_idx] + CONTEXT + 1).min(ops.len());
            c_idx += 1;
        }

        let mut hunk_lines = Vec::with_capacity(end - start);
        let mut first_old: Option<usize> = None;
        let mut first_new: Option<usize> = None;
        let mut old_count = 0usize;
        let mut new_count = 0usize;

        for op in &ops[start..end] {
            match *op {
                RawOp::Context(oi, ni) => {
                    if first_old.is_none() {
                        first_old = Some(oi + 1);
                    }
                    if first_new.is_none() {
                        first_new = Some(ni + 1);
                    }
                    old_count += 1;
                    new_count += 1;
                    hunk_lines.push(DiffLine {
                        kind: "context".to_string(),
                        text: old_lines[oi].to_string(),
                        old_line: Some(oi + 1),
                        new_line: Some(ni + 1),
                    });
                }
                RawOp::Remove(oi) => {
                    if first_old.is_none() {
                        first_old = Some(oi + 1);
                    }
                    old_count += 1;
                    deletions += 1;
                    hunk_lines.push(DiffLine {
                        kind: "remove".to_string(),
                        text: old_lines[oi].to_string(),
                        old_line: Some(oi + 1),
                        new_line: None,
                    });
                }
                RawOp::Add(ni) => {
                    if first_new.is_none() {
                        first_new = Some(ni + 1);
                    }
                    new_count += 1;
                    additions += 1;
                    hunk_lines.push(DiffLine {
                        kind: "add".to_string(),
                        text: new_lines[ni].to_string(),
                        old_line: None,
                        new_line: Some(ni + 1),
                    });
                }
            }
        }

        hunks.push(DiffHunk {
            old_start: first_old.unwrap_or(1),
            old_count,
            new_start: first_new.unwrap_or(1),
            new_count,
            lines: hunk_lines,
        });
    }

    ScriptFileDiff {
        path: path.to_string(),
        status: status.to_string(),
        additions,
        deletions,
        before_source: before_source.to_string(),
        after_source: after_source.to_string(),
        hunks,
    }
}

/// Compute sorted reviewable unified diffs for every added, modified, or removed script
/// between `before` and `after` place trees.
pub fn compute_file_diffs(before: &Value, after: &Value) -> Vec<ScriptFileDiff> {
    let empty = Map::new();
    let b_sc = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_sc = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    let mut keys: Vec<&String> = b_sc.keys().chain(a_sc.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut out = Vec::new();
    for path in keys {
        let old_src = b_sc.get(path).and_then(|v| v.as_str());
        let new_src = a_sc.get(path).and_then(|v| v.as_str());
        match (old_src, new_src) {
            (None, Some(after_s)) => {
                out.push(compute_script_diff(path, "added", "", after_s));
            }
            (Some(before_s), None) => {
                out.push(compute_script_diff(path, "removed", before_s, ""));
            }
            (Some(before_s), Some(after_s)) if before_s != after_s => {
                out.push(compute_script_diff(path, "modified", before_s, after_s));
            }
            _ => {}
        }
    }
    out
}

/// Build a selective merged target tree by applying either all changes (`selected_paths == None`)
/// or only the caller-selected paths (`selected_paths == Some(...)`) from `before -> after`
/// onto `live_base`. Returns `(merged_tree, applied_paths)`.
pub fn merge_selected_tree(
    live_base: &Value,
    before: &Value,
    after: &Value,
    selected_paths: Option<&[String]>,
) -> (Value, Vec<String>) {
    let mut merged_inst = live_base
        .get("instances")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let mut merged_scripts = live_base
        .get("scripts")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let empty = Map::new();
    let b_inst = before
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_inst = after
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let b_sc = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_sc = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    let full_diff = compute_diff(before, after);
    let mut candidate_inst: Vec<String> = Vec::new();
    candidate_inst.extend(full_diff.added_instances.iter().cloned());
    candidate_inst.extend(full_diff.modified_instances.iter().cloned());
    candidate_inst.extend(full_diff.removed_instances.iter().cloned());

    let mut candidate_scripts: Vec<String> = Vec::new();
    candidate_scripts.extend(full_diff.added_scripts.iter().cloned());
    candidate_scripts.extend(full_diff.modified_scripts.iter().cloned());
    candidate_scripts.extend(full_diff.removed_scripts.iter().cloned());

    let mut applied = Vec::new();
    let is_selected = |p: &str| -> bool {
        match selected_paths {
            None => true,
            Some(list) => list.iter().any(|s| s == p),
        }
    };

    for path in &candidate_inst {
        if !is_selected(path) {
            continue;
        }
        if let Some(val) = a_inst.get(path) {
            merged_inst.insert(path.clone(), val.clone());
            applied.push(path.clone());
        } else if b_inst.contains_key(path) {
            merged_inst.remove(path);
            applied.push(path.clone());
        }
    }

    for path in &candidate_scripts {
        if !is_selected(path) {
            continue;
        }
        if let Some(val) = a_sc.get(path) {
            merged_scripts.insert(path.clone(), val.clone());
            applied.push(path.clone());
        } else if b_sc.contains_key(path) {
            merged_scripts.remove(path);
            applied.push(path.clone());
        }
    }

    applied.sort();
    applied.dedup();
    (
        json!({
            "instances": Value::Object(merged_inst),
            "scripts": Value::Object(merged_scripts),
        }),
        applied,
    )
}

/// A staged RoPilot change proposal generated in `plan` mode or with `review_diffs: true`.
#[derive(Clone, Debug)]
pub struct PendingProposal {
    pub id: String,
    pub chat_id: String,
    pub message: String,
    pub summary: String,
    pub mode: String,
    pub before_tree: Value,
    pub after_tree: Value,
    pub file_diffs: Vec<ScriptFileDiff>,
    pub diff: PlaceDiff,
    pub diagnostics: Value,
    pub skills_used: Vec<String>,
    pub status: String,
    pub created_at_ms: u64,
}

impl PendingProposal {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "chat_id": self.chat_id,
            "message": self.message,
            "summary": self.summary,
            "mode": self.mode,
            "before_tree": self.before_tree,
            "after_tree": self.after_tree,
            "file_diffs": self.file_diffs.iter().map(ScriptFileDiff::to_json).collect::<Vec<_>>(),
            "diff": self.diff.to_json(),
            "diagnostics": self.diagnostics,
            "skills_used": self.skills_used,
            "status": self.status,
            "created_at_ms": self.created_at_ms,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let id = v.get("id")?.as_str()?.to_string();
        let chat_id = v.get("chat_id")?.as_str()?.to_string();
        let message = v.get("message")?.as_str()?.to_string();
        let summary = v.get("summary")?.as_str()?.to_string();
        let mode = v
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("plan")
            .to_string();
        let before_tree = v.get("before_tree")?.clone();
        let after_tree = v.get("after_tree")?.clone();
        let file_diffs = v
            .get("file_diffs")?
            .as_array()?
            .iter()
            .map(ScriptFileDiff::from_json)
            .collect::<Option<Vec<_>>>()?;
        let diff = v
            .get("diff")
            .and_then(PlaceDiff::from_json)
            .unwrap_or_default();
        let diagnostics = v
            .get("diagnostics")
            .cloned()
            .unwrap_or_else(|| json!({ "ok": true, "error_count": 0, "warning_count": 0, "diagnostics": [] }));
        let skills_used = v
            .get("skills_used")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let status = v.get("status")?.as_str()?.to_string();
        let created_at_ms = v.get("created_at_ms")?.as_u64()?;
        Some(PendingProposal {
            id,
            chat_id,
            message,
            summary,
            mode,
            before_tree,
            after_tree,
            file_diffs,
            diff,
            diagnostics,
            skills_used,
            status,
            created_at_ms,
        })
    }
}

/// Bounded persistent store of staged diff proposals (`data_dir/proposals.json`).
pub struct ProposalStore {
    proposals: Vec<PendingProposal>,
    path: Option<PathBuf>,
    pub recovered: Vec<String>,
}

impl Default for ProposalStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl ProposalStore {
    pub fn in_memory() -> Self {
        ProposalStore {
            proposals: Vec::new(),
            path: None,
            recovered: Vec::new(),
        }
    }

    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("proposals.json");
        let mut recovered = Vec::new();
        let proposals = load_proposals(&path, &mut recovered);
        ProposalStore {
            proposals,
            path: Some(path),
            recovered,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        chat_id: String,
        message: String,
        summary: String,
        mode: String,
        before_tree: Value,
        after_tree: Value,
        diagnostics: Value,
        skills_used: Vec<String>,
    ) -> PendingProposal {
        let before = normalize_tree(&before_tree);
        let after = normalize_tree(&after_tree);
        let diff = compute_diff(&before, &after);
        let file_diffs = compute_file_diffs(&before, &after);
        let prop = PendingProposal {
            id: new_proposal_id(),
            chat_id,
            message,
            summary,
            mode,
            before_tree: before,
            after_tree: after,
            file_diffs,
            diff,
            diagnostics,
            skills_used,
            status: "pending".to_string(),
            created_at_ms: now_ms(),
        };
        self.proposals.push(prop.clone());
        if self.proposals.len() > MAX_PROPOSALS {
            let drop_n = self.proposals.len() - MAX_PROPOSALS;
            self.proposals.drain(0..drop_n);
        }
        self.persist();
        prop
    }

    pub fn list(&self, status_filter: Option<&str>, chat_id_filter: Option<&str>) -> Vec<Value> {
        self.proposals
            .iter()
            .rev()
            .filter(|p| status_filter.is_none_or(|s| p.status == s))
            .filter(|p| chat_id_filter.is_none_or(|c| p.chat_id == c))
            .map(PendingProposal::to_json)
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.proposals
            .iter()
            .filter(|p| p.status == "pending")
            .count()
    }

    pub fn get(&self, id: &str) -> Option<&PendingProposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    pub fn mark_status(&mut self, id: &str, status: &str) -> Option<PendingProposal> {
        let found = self.proposals.iter_mut().find(|p| p.id == id)?;
        found.status = status.to_string();
        let cloned = found.clone();
        self.persist();
        Some(cloned)
    }

    fn persist(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = json!({
            "proposals": self.proposals.iter().map(PendingProposal::to_json).collect::<Vec<_>>(),
        });
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn load_proposals(path: &Path, recovered: &mut Vec<String>) -> Vec<PendingProposal> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let parsed = (|| -> Option<Vec<PendingProposal>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let arr = v.get("proposals")?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            out.push(PendingProposal::from_json(item)?);
        }
        Some(out)
    })();
    match parsed {
        Some(list) => list,
        None => {
            if let Some(bak) = backup_corrupt(path) {
                recovered.push(
                    bak.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_diff_tracks_added_modified_removed() {
        let before = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": true } },
                "Workspace.OldPart": { "class_name": "Part", "properties": {} }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "print(1)",
                "ServerScriptService.OldScript": "print(0)"
            }
        });
        let after = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": false } },
                "Workspace.JumpPad": { "class_name": "Part", "properties": { "Anchored": true } }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "print(2)",
                "ServerScriptService.DayNight": "print(3)"
            }
        });
        let d = compute_diff(&before, &after);
        assert_eq!(d.added_instances, vec!["Workspace.JumpPad"]);
        assert_eq!(d.modified_instances, vec!["Workspace.SpawnPad"]);
        assert_eq!(d.removed_instances, vec!["Workspace.OldPart"]);
        assert_eq!(d.added_scripts, vec!["ServerScriptService.DayNight"]);
        assert_eq!(d.modified_scripts, vec!["ServerScriptService.Leaderboard"]);
        assert_eq!(d.removed_scripts, vec!["ServerScriptService.OldScript"]);
        assert_eq!(d.total_changes(), 6);
    }

    #[test]
    fn rbxlx_round_trip_and_malformed_rejection() {
        let tree = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": true, "Position": [0, 3, 0] } },
                "Workspace.LavaBrick": { "class_name": "Part", "properties": { "Anchored": true, "Color": [220, 60, 40] } }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "local Players = game:GetService(\"Players\")\nprint(\"a < b & c\")\n",
                "StarterGui.GameGui": "local gui = Instance.new(\"ScreenGui\")\n"
            }
        });
        let xml = serialize_rbxlx("Skyline Obby", &tree);
        let parsed = parse_rbxlx_xml(&xml).expect("valid exported .rbxlx parses back");
        assert!(parsed["instances"]["Workspace.SpawnPad"].is_object());
        assert!(parsed["instances"]["Workspace.LavaBrick"].is_object());
        assert!(parsed["scripts"]["ServerScriptService.Leaderboard"]
            .as_str()
            .unwrap()
            .contains("a < b & c"));
        assert!(parsed["scripts"]["StarterGui.GameGui"]
            .as_str()
            .unwrap()
            .contains("ScreenGui"));

        // Malformed / XXE XML rejected cleanly without panic.
        assert!(parse_rbxlx_xml("").is_err());
        assert!(parse_rbxlx_xml("<!DOCTYPE foo [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><roblox></roblox>").is_err());
        assert!(parse_rbxlx_xml("<roblox><Item class=\"Part\"></roblox>").is_err());
        assert!(parse_rbxlx_xml("<notroblox></notroblox>").is_err());
    }

    #[test]
    fn snapshot_store_persists_and_bounds_history() {
        let dir = std::env::temp_dir().join(format!("openro-snap-test-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = SnapshotStore::open(&dir);
        for i in 0..(MAX_SNAPSHOTS + 5) {
            store.record(
                format!("Build {i}"),
                "build",
                Some("chat-1".to_string()),
                json!({ "instances": {}, "scripts": { "ServerScriptService.S": format!("-- {i}") } }),
                None,
            );
        }
        assert_eq!(store.list(false).len(), MAX_SNAPSHOTS);
        let reloaded = SnapshotStore::open(&dir);
        assert_eq!(reloaded.list(false).len(), MAX_SNAPSHOTS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compute_file_diffs_produces_unified_hunks_and_selective_merge() {
        let before = json!({
            "instances": {},
            "scripts": {
                "ServerScriptService.Keep": "local x = 1\nlocal y = 2\nprint(x + y)",
                "ServerScriptService.Old": "print('bye')"
            }
        });
        let after = json!({
            "instances": {
                "Workspace.SprintPad": { "class_name": "Part", "properties": { "Anchored": true } }
            },
            "scripts": {
                "ServerScriptService.Keep": "local x = 1\nlocal y = 42\nprint(x + y)\nprint('done')",
                "ServerScriptService.Sprint": "local Players = game:GetService('Players')\nprint(Players)"
            }
        });
        let diffs = compute_file_diffs(&before, &after);
        assert_eq!(diffs.len(), 3);
        let keep = diffs.iter().find(|d| d.path == "ServerScriptService.Keep").unwrap();
        assert_eq!(keep.status, "modified");
        assert_eq!(keep.additions, 2);
        assert_eq!(keep.deletions, 1);
        assert_eq!(keep.hunks.len(), 1);
        assert!(keep.hunks[0].lines.iter().any(|l| l.kind == "remove" && l.text == "local y = 2"));
        assert!(keep.hunks[0].lines.iter().any(|l| l.kind == "add" && l.text == "local y = 42"));

        let added = diffs.iter().find(|d| d.path == "ServerScriptService.Sprint").unwrap();
        assert_eq!(added.status, "added");
        assert_eq!(added.additions, 2);
        assert_eq!(added.deletions, 0);

        let removed = diffs.iter().find(|d| d.path == "ServerScriptService.Old").unwrap();
        assert_eq!(removed.status, "removed");
        assert_eq!(removed.additions, 0);
        assert_eq!(removed.deletions, 1);

        // Selective merge applies ONLY selected path.
        let (merged, applied) = merge_selected_tree(
            &before,
            &before,
            &after,
            Some(&["ServerScriptService.Sprint".to_string()]),
        );
        assert_eq!(applied, vec!["ServerScriptService.Sprint"]);
        assert!(merged["scripts"]["ServerScriptService.Sprint"].is_string());
        assert_eq!(
            merged["scripts"]["ServerScriptService.Keep"].as_str().unwrap(),
            "local x = 1\nlocal y = 2\nprint(x + y)"
        );
        assert!(merged["scripts"]["ServerScriptService.Old"].is_string());
        assert!(merged["instances"]["Workspace.SprintPad"].is_null());
    }
}
