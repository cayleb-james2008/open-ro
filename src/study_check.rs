//! Canonical Study & Evidence Ledger plus Contract-Driven Verification (#62).
//!
//! Brings Craft Anything's evidence-backed investigation and verification discipline
//! directly into OpenRO projects under `<project>/.openro/investigation/`:
//! - `evidence.jsonl`: content-addressed (`ev_<16hex>` from canonical JSON SHA-256)
//!   observations with `confidence` (`observed` | `derived` | `inferred`),
//!   `authority`, `coverage`, `limitations`, and `clean_room` provenance.
//! - `unknowns.json`: optimistic-concurrency (`revision`) ledger of open questions
//!   (`unk_<16hex>`) that fails closed if marking `resolved` without both a
//!   non-empty `resolution` and at least one valid `evidence_for` reference.
//! - `mechanics.json` provenance checker (`docs/mechanics.json` or `mechanics.json`),
//!   counting `sourced` / `estimated` / `unsourced` entries and warning whenever a
//!   literal config value is cited without verified runtime behavior.
//! - Playtest core-loop claim evaluator (`move`, `act`, `hud` + suite steps),
//!   reporting `"not tested"` when absent rather than assuming success.
//! - Zero-dependency pure-Rust PNG decoder + blank/single-color rejection +
//!   normalized `640x360` perceptual likeness scoring against `refs/<universeId>/`.
//! - `KeptChecks` SHA-256 project fingerprinting (`kept_checks.json`) that reuses
//!   passing `check` / `playtest` rounds when inputs and mode are unchanged,
//!   NEVER caches failures, and supports `force: true`.

use crate::projects;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const INVESTIGATION_REL: &str = ".openro/investigation";
const EVIDENCE_FILE: &str = "evidence.jsonl";
const UNKNOWNS_FILE: &str = "unknowns.json";
const KEPT_CHECKS_FILE: &str = "kept_checks.json";

const MAX_FINGERPRINT_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_KEPT_ENTRIES: usize = 64;
const MAX_PNG_DIM: u32 = 8192;
const MAX_PNG_PIXELS: u64 = 33_554_432;
pub const LIKENESS_NORM_W: u32 = 640;
pub const LIKENESS_NORM_H: u32 = 360;

/// Structured error returned by study/investigation operations so HTTP routes can
/// map `400 Bad Request`, `404 Not Found`, `409 Conflict`, and `500` accurately.
#[derive(Clone, Debug)]
pub struct StudyError {
    pub status: u16,
    pub message: String,
    pub details: Option<Value>,
}

impl StudyError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self { status: 400, message: msg.into(), details: None }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self { status: 404, message: msg.into(), details: None }
    }
    pub fn conflict(msg: impl Into<String>, details: Option<Value>) -> Self {
        Self { status: 409, message: msg.into(), details }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self { status: 500, message: msg.into(), details: None }
    }
    pub fn http_status(&self) -> &'static str {
        match self.status {
            400 => "400 Bad Request",
            403 => "403 Forbidden",
            404 => "404 Not Found",
            409 => "409 Conflict",
            _ => "500 Internal Server Error",
        }
    }
    pub fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("ok".into(), json!(false));
        obj.insert("error".into(), json!(self.message));
        if let Some(Value::Object(extra)) = &self.details {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
        Value::Object(obj)
    }
}

impl std::fmt::Display for StudyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn ledger_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn investigation_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(INVESTIGATION_REL)
}

fn ensure_investigation_dir(project_dir: &Path) -> Result<PathBuf, StudyError> {
    let dir = investigation_dir(project_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| StudyError::internal(format!("create investigation dir: {e}")))?;
    Ok(dir)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StudyError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| StudyError::internal(format!("create parent dir: {e}")))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", projects::unique_id("s")));
    std::fs::write(&tmp, bytes)
        .map_err(|e| StudyError::internal(format!("write {}: {e}", path.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| StudyError::internal(format!("rename {}: {e}", path.display())))?;
    Ok(())
}

// --------------------------------------------------------- canonical JSON & IDs

/// Produce a deterministic canonical JSON representation (recursively sorted keys,
/// compact encoding) suitable for stable content-addressed SHA-256 identifiers.
pub fn canonical_json(v: &Value) -> String {
    fn normalize(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
                for (k, val) in map {
                    sorted.insert(k.clone(), normalize(val));
                }
                let mut out = Map::new();
                for (k, val) in sorted {
                    out.insert(k, val);
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(normalize).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&normalize(v)).unwrap_or_else(|_| "null".to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Content-addressed evidence ID: `ev_<16 lowercase hex chars>` from canonical JSON SHA-256.
pub fn evidence_id_for(canonical_core: &Value) -> String {
    let hex = sha256_hex(canonical_json(canonical_core).as_bytes());
    format!("ev_{}", &hex[..16])
}

/// Content-addressed unknown ID: `unk_<16 lowercase hex chars>` from canonical JSON SHA-256.
pub fn unknown_id_for(domain: &str, title: &str, question: &str) -> String {
    let core = json!({
        "domain": domain.trim().to_ascii_lowercase(),
        "title": title.trim(),
        "question": question.trim(),
    });
    let hex = sha256_hex(canonical_json(&core).as_bytes());
    format!("unk_{}", &hex[..16])
}

/// Read the project's `clean_room` setting from `openro.project.json` (defaults to `true`).
pub fn project_clean_room(project_dir: &Path) -> bool {
    projects::manifest(project_dir)
        .ok()
        .and_then(|m| m.get("clean_room").and_then(Value::as_bool))
        .unwrap_or(true)
}

fn parse_string_list(v: Option<&Value>, field: &str) -> Result<Vec<String>, StudyError> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![t.to_string()])
            }
        }
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| StudyError::bad(format!("{field} must be an array of strings")))?;
                let t = s.trim();
                if !t.is_empty() && !out.iter().any(|e| e == t) {
                    out.push(t.to_string());
                }
            }
            Ok(out)
        }
        Some(_) => Err(StudyError::bad(format!("{field} must be an array of strings"))),
    }
}

// ------------------------------------------------------------- evidence ledger

#[derive(Clone, Debug, Default)]
pub struct EvidenceFilter {
    pub confidence: Option<String>,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub limit: Option<usize>,
}

fn read_all_evidence(project_dir: &Path) -> Result<Vec<Value>, StudyError> {
    let path = investigation_dir(project_dir).join(EVIDENCE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(StudyError::internal(format!("read evidence.jsonl: {e}"))),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            out.push(v);
        }
    }
    Ok(out)
}

/// Append a content-addressed (`ev_<16hex>`) evidence record to
/// `<project>/.openro/investigation/evidence.jsonl`.
/// Identical core observations deduplicate cleanly to the existing `ev_<16hex>` record.
pub fn record_evidence(project_dir: &Path, input: &Value) -> Result<Value, StudyError> {
    let obj = input
        .as_object()
        .ok_or_else(|| StudyError::bad("evidence payload must be a JSON object"))?;
    let summary = obj
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| StudyError::bad("evidence 'summary' is required and must be non-empty"))?;
    if summary.len() > 4000 {
        return Err(StudyError::bad("evidence 'summary' exceeds 4000 characters"));
    }

    let confidence = match obj.get("confidence") {
        None | Some(Value::Null) => "observed",
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "observed" => "observed",
            "derived" => "derived",
            "inferred" => "inferred",
            other => {
                return Err(StudyError::bad(format!(
                    "invalid confidence '{other}': must be one of observed, derived, inferred"
                )))
            }
        },
        Some(_) => {
            return Err(StudyError::bad(
                "confidence must be a string (observed, derived, or inferred)",
            ))
        }
    };

    let clean_room = match obj.get("clean_room") {
        None | Some(Value::Null) => project_clean_room(project_dir),
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(StudyError::bad("clean_room must be a boolean")),
    };

    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("observation")
        .chars()
        .take(80)
        .collect::<String>();

    let source = obj
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("manual")
        .chars()
        .take(200)
        .collect::<String>();

    let default_authority = if clean_room {
        "external_surface"
    } else {
        "deep_inspection"
    };
    let authority = obj
        .get("authority")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_authority)
        .chars()
        .take(120)
        .collect::<String>();

    let coverage = obj
        .get("coverage")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();

    let limitations = parse_string_list(obj.get("limitations"), "limitations")?;
    let refs = parse_string_list(obj.get("refs"), "refs")?;
    let data = obj.get("data").cloned().unwrap_or_else(|| json!({}));

    let core = json!({
        "kind": kind,
        "source": source,
        "summary": summary,
        "confidence": confidence,
        "authority": authority,
        "coverage": coverage,
        "limitations": limitations,
        "clean_room": clean_room,
        "refs": refs,
        "data": data,
    });
    let id = evidence_id_for(&core);

    let _guard = ledger_lock().lock().unwrap();
    let existing = read_all_evidence(project_dir)?;
    if let Some(prev) = existing.into_iter().find(|e| e["id"].as_str() == Some(&id)) {
        return Ok(json!({
            "deduplicated": true,
            "evidence": prev,
        }));
    }

    let mut record = core;
    if let Some(m) = record.as_object_mut() {
        m.insert("id".into(), json!(id));
        m.insert("recorded_at_ms".into(), json!(projects::now_ms()));
    }

    let dir = ensure_investigation_dir(project_dir)?;
    let path = dir.join(EVIDENCE_FILE);
    let mut line = canonical_json(&record);
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| StudyError::internal(format!("open evidence.jsonl: {e}")))?;
    file.write_all(line.as_bytes())
        .map_err(|e| StudyError::internal(format!("append evidence.jsonl: {e}")))?;

    Ok(json!({
        "deduplicated": false,
        "evidence": record,
    }))
}

/// List evidence entries from `<project>/.openro/investigation/evidence.jsonl`.
pub fn list_evidence(project_dir: &Path, filter: &EvidenceFilter) -> Result<Value, StudyError> {
    if let Some(ref c) = filter.confidence {
        let low = c.trim().to_ascii_lowercase();
        if !matches!(low.as_str(), "observed" | "derived" | "inferred") {
            return Err(StudyError::bad(format!(
                "invalid confidence filter '{c}': must be observed, derived, or inferred"
            )));
        }
    }
    let all = read_all_evidence(project_dir)?;
    let mut observed = 0usize;
    let mut derived = 0usize;
    let mut inferred = 0usize;
    for e in &all {
        match e["confidence"].as_str().unwrap_or("") {
            "observed" => observed += 1,
            "derived" => derived += 1,
            "inferred" => inferred += 1,
            _ => {}
        }
    }

    let mut filtered: Vec<Value> = all
        .into_iter()
        .filter(|e| {
            if let Some(ref c) = filter.confidence {
                if !e["confidence"]
                    .as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(c.trim())
                {
                    return false;
                }
            }
            if let Some(ref k) = filter.kind {
                if !e["kind"]
                    .as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(k.trim())
                {
                    return false;
                }
            }
            if let Some(ref s) = filter.source {
                if !e["source"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains(&s.trim().to_ascii_lowercase())
                {
                    return false;
                }
            }
            true
        })
        .collect();

    let total_matched = filtered.len();
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    if filtered.len() > limit {
        let keep_from = filtered.len() - limit;
        filtered = filtered.split_off(keep_from);
    }

    Ok(json!({
        "total": observed + derived + inferred,
        "matched": total_matched,
        "counts_by_confidence": {
            "observed": observed,
            "derived": derived,
            "inferred": inferred,
        },
        "evidence": filtered,
    }))
}

// ------------------------------------------------------------- unknowns ledger

fn read_unknowns_doc(project_dir: &Path) -> Result<(u64, Vec<Value>), StudyError> {
    let path = investigation_dir(project_dir).join(UNKNOWNS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, Vec::new())),
        Err(e) => return Err(StudyError::internal(format!("read unknowns.json: {e}"))),
    };
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| StudyError::internal(format!("parse unknowns.json: {e}")))?;
    let rev = doc.get("revision").and_then(Value::as_u64).unwrap_or(0);
    let items = doc
        .get("unknowns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((rev, items))
}

fn write_unknowns_doc(project_dir: &Path, revision: u64, items: &[Value]) -> Result<(), StudyError> {
    let dir = ensure_investigation_dir(project_dir)?;
    let doc = json!({
        "revision": revision,
        "unknowns": items,
    });
    let bytes = serde_json::to_string_pretty(&doc)
        .map_err(|e| StudyError::internal(format!("serialize unknowns.json: {e}")))?
        + "\n";
    write_atomic(&dir.join(UNKNOWNS_FILE), bytes.as_bytes())
}

fn validate_severity(raw: Option<&Value>, default_val: &str) -> Result<String, StudyError> {
    match raw {
        None | Some(Value::Null) => Ok(default_val.to_string()),
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "critical" => Ok("critical".into()),
            "high" => Ok("high".into()),
            "medium" => Ok("medium".into()),
            "low" => Ok("low".into()),
            other => Err(StudyError::bad(format!(
                "invalid severity '{other}': must be critical, high, medium, or low"
            ))),
        },
        Some(_) => Err(StudyError::bad("severity must be a string")),
    }
}

fn validate_status(raw: Option<&Value>, default_val: &str) -> Result<String, StudyError> {
    match raw {
        None | Some(Value::Null) => Ok(default_val.to_string()),
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok("open".into()),
            "investigating" => Ok("investigating".into()),
            "resolved" => Ok("resolved".into()),
            "wont_fix" => Ok("wont_fix".into()),
            other => Err(StudyError::bad(format!(
                "invalid status '{other}': must be open, investigating, resolved, or wont_fix"
            ))),
        },
        Some(_) => Err(StudyError::bad("status must be a string")),
    }
}

fn verify_evidence_ids(
    project_dir: &Path,
    ids: &[String],
    field: &str,
) -> Result<(), StudyError> {
    if ids.is_empty() {
        return Ok(());
    }
    let known: BTreeSet<String> = read_all_evidence(project_dir)?
        .into_iter()
        .filter_map(|e| e["id"].as_str().map(String::from))
        .collect();
    for id in ids {
        if id.starts_with("ev_") && !known.contains(id) {
            return Err(StudyError::bad(format!(
                "{field} references unknown evidence id '{id}' — record evidence first"
            )));
        }
    }
    Ok(())
}

fn validate_resolution_gate(
    project_dir: &Path,
    status: &str,
    resolution: Option<&str>,
    evidence_for: &[String],
    evidence_against: &[String],
) -> Result<(), StudyError> {
    verify_evidence_ids(project_dir, evidence_for, "evidence_for")?;
    verify_evidence_ids(project_dir, evidence_against, "evidence_against")?;
    if status == "resolved" {
        let res_ok = resolution.map(|s| !s.trim().is_empty()).unwrap_or(false);
        if !res_ok {
            return Err(StudyError::bad(
                "cannot mark unknown resolved without a non-empty 'resolution'",
            ));
        }
        if evidence_for.is_empty() {
            return Err(StudyError::bad(
                "cannot mark unknown resolved without at least one 'evidence_for' entry",
            ));
        }
    }
    Ok(())
}

/// List unknowns from `<project>/.openro/investigation/unknowns.json`.
pub fn list_unknowns(
    project_dir: &Path,
    status_filter: Option<&str>,
    severity_filter: Option<&str>,
    domain_filter: Option<&str>,
) -> Result<Value, StudyError> {
    if let Some(s) = status_filter {
        let _ = validate_status(Some(&json!(s)), "open")?;
    }
    if let Some(s) = severity_filter {
        let _ = validate_severity(Some(&json!(s)), "medium")?;
    }
    let (revision, items) = read_unknowns_doc(project_dir)?;
    let mut by_status: BTreeMap<&str, usize> =
        BTreeMap::from([("open", 0), ("investigating", 0), ("resolved", 0), ("wont_fix", 0)]);
    let mut by_severity: BTreeMap<&str, usize> =
        BTreeMap::from([("critical", 0), ("high", 0), ("medium", 0), ("low", 0)]);
    let mut critical_open = 0usize;
    let mut high_open = 0usize;
    for u in &items {
        let st = u["status"].as_str().unwrap_or("open");
        let sv = u["severity"].as_str().unwrap_or("medium");
        if let Some(c) = by_status.get_mut(st) {
            *c += 1;
        }
        if let Some(c) = by_severity.get_mut(sv) {
            *c += 1;
        }
        if matches!(st, "open" | "investigating") {
            if sv == "critical" {
                critical_open += 1;
            } else if sv == "high" {
                high_open += 1;
            }
        }
    }
    let filtered: Vec<Value> = items
        .iter()
        .filter(|u| {
            if let Some(sf) = status_filter {
                if !u["status"].as_str().unwrap_or("").eq_ignore_ascii_case(sf.trim()) {
                    return false;
                }
            }
            if let Some(sv) = severity_filter {
                if !u["severity"].as_str().unwrap_or("").eq_ignore_ascii_case(sv.trim()) {
                    return false;
                }
            }
            if let Some(df) = domain_filter {
                if !u["domain"].as_str().unwrap_or("").eq_ignore_ascii_case(df.trim()) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    Ok(json!({
        "revision": revision,
        "total": items.len(),
        "open_count": by_status["open"] + by_status["investigating"],
        "critical_open": critical_open,
        "high_open": high_open,
        "counts_by_status": by_status,
        "counts_by_severity": by_severity,
        "unknowns": filtered,
    }))
}

/// Add a new unknown (`unk_<16hex>`) to `<project>/.openro/investigation/unknowns.json`.
pub fn add_unknown(project_dir: &Path, input: &Value) -> Result<Value, StudyError> {
    let obj = input
        .as_object()
        .ok_or_else(|| StudyError::bad("unknown payload must be a JSON object"))?;
    let title = obj
        .get("title")
        .or_else(|| obj.get("question"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| StudyError::bad("unknown requires a non-empty 'title' or 'question'"))?;
    let question = obj
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(title);
    let domain = obj
        .get("domain")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("general")
        .to_ascii_lowercase();
    let severity = validate_severity(obj.get("severity"), "medium")?;
    let status = validate_status(obj.get("status"), "open")?;
    let probes = parse_string_list(obj.get("probes"), "probes")?;
    let evidence_for = parse_string_list(obj.get("evidence_for"), "evidence_for")?;
    let evidence_against = parse_string_list(obj.get("evidence_against"), "evidence_against")?;
    let resolution = obj
        .get("resolution")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let expected_rev = obj
        .get("expected_revision")
        .or_else(|| obj.get("if_revision"))
        .and_then(Value::as_u64);

    let _guard = ledger_lock().lock().unwrap();
    validate_resolution_gate(
        project_dir,
        &status,
        resolution.as_deref(),
        &evidence_for,
        &evidence_against,
    )?;

    let (cur_rev, mut items) = read_unknowns_doc(project_dir)?;
    if let Some(want) = expected_rev {
        if want != cur_rev {
            return Err(StudyError::conflict(
                format!("unknowns ledger revision mismatch: expected {want}, current is {cur_rev}"),
                Some(json!({ "current_revision": cur_rev })),
            ));
        }
    }

    let id = unknown_id_for(&domain, title, question);
    if items.iter().any(|u| u["id"].as_str() == Some(&id)) {
        return Err(StudyError::conflict(
            format!("unknown '{id}' already exists — use PATCH to update it"),
            Some(json!({ "id": id, "current_revision": cur_rev })),
        ));
    }

    let now = projects::now_ms();
    let item = json!({
        "id": id,
        "revision": 1u64,
        "title": title,
        "question": question,
        "severity": severity,
        "domain": domain,
        "status": status,
        "probes": probes,
        "evidence_for": evidence_for,
        "evidence_against": evidence_against,
        "resolution": resolution,
        "created_at_ms": now,
        "updated_at_ms": now,
    });
    let new_rev = cur_rev + 1;
    items.push(item.clone());
    write_unknowns_doc(project_dir, new_rev, &items)?;

    Ok(json!({
        "revision": new_rev,
        "unknown": item,
    }))
}

/// Update an unknown with optimistic concurrency (`expected_revision`) and
/// fail-closed resolution validation (`resolution` + `evidence_for` required for `resolved`).
pub fn update_unknown(
    project_dir: &Path,
    id: &str,
    patch: &Value,
) -> Result<Value, StudyError> {
    let target_id = id.trim();
    if target_id.is_empty() {
        return Err(StudyError::bad("unknown 'id' is required"));
    }
    let obj = patch
        .as_object()
        .ok_or_else(|| StudyError::bad("update payload must be a JSON object"))?;

    let expected_item_rev = obj
        .get("expected_revision")
        .or_else(|| obj.get("if_revision"))
        .or_else(|| obj.get("revision"))
        .and_then(Value::as_u64);
    let expected_ledger_rev = obj.get("expected_ledger_revision").and_then(Value::as_u64);

    let _guard = ledger_lock().lock().unwrap();
    let (cur_ledger_rev, mut items) = read_unknowns_doc(project_dir)?;
    if let Some(want_l) = expected_ledger_rev {
        if want_l != cur_ledger_rev {
            return Err(StudyError::conflict(
                format!(
                    "unknowns ledger revision mismatch: expected {want_l}, current is {cur_ledger_rev}"
                ),
                Some(json!({ "current_ledger_revision": cur_ledger_rev })),
            ));
        }
    }

    let idx = items
        .iter()
        .position(|u| u["id"].as_str() == Some(target_id))
        .ok_or_else(|| StudyError::not_found(format!("unknown '{target_id}' not found")))?;

    let mut item = items[idx].clone();
    let cur_item_rev = item["revision"].as_u64().unwrap_or(1);
    if let Some(want_r) = expected_item_rev {
        if want_r != cur_item_rev {
            return Err(StudyError::conflict(
                format!(
                    "unknown '{target_id}' revision mismatch: expected {want_r}, current is {cur_item_rev}"
                ),
                Some(json!({
                    "id": target_id,
                    "current_revision": cur_item_rev,
                    "current_ledger_revision": cur_ledger_rev,
                })),
            ));
        }
    }

    let next_severity = if obj.contains_key("severity") {
        validate_severity(obj.get("severity"), "medium")?
    } else {
        item["severity"].as_str().unwrap_or("medium").to_string()
    };
    let next_status = if obj.contains_key("status") {
        validate_status(obj.get("status"), "open")?
    } else {
        item["status"].as_str().unwrap_or("open").to_string()
    };
    let next_domain = if let Some(d) = obj.get("domain").and_then(Value::as_str) {
        let clean = d.trim().to_ascii_lowercase();
        if clean.is_empty() {
            return Err(StudyError::bad("domain cannot be empty"));
        }
        clean
    } else {
        item["domain"].as_str().unwrap_or("general").to_string()
    };

    let merge_list = |existing: Option<&Value>, replace_key: &str, add_key: &str| -> Result<Vec<String>, StudyError> {
        let mut list = if obj.contains_key(replace_key) {
            parse_string_list(obj.get(replace_key), replace_key)?
        } else {
            parse_string_list(existing, replace_key)?
        };
        if obj.contains_key(add_key) {
            for extra in parse_string_list(obj.get(add_key), add_key)? {
                if !list.contains(&extra) {
                    list.push(extra);
                }
            }
        }
        Ok(list)
    };

    let next_probes = merge_list(item.get("probes"), "probes", "add_probes")?;
    let next_for = merge_list(item.get("evidence_for"), "evidence_for", "add_evidence_for")?;
    let next_against = merge_list(
        item.get("evidence_against"),
        "evidence_against",
        "add_evidence_against",
    )?;
    let next_resolution: Option<String> = if obj.contains_key("resolution") {
        match obj.get("resolution") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => {
                let t = s.trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            }
            Some(_) => return Err(StudyError::bad("resolution must be a string or null")),
        }
    } else {
        item.get("resolution")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };

    validate_resolution_gate(
        project_dir,
        &next_status,
        next_resolution.as_deref(),
        &next_for,
        &next_against,
    )?;

    let next_item_rev = cur_item_rev + 1;
    let next_ledger_rev = cur_ledger_rev + 1;
    let map = item.as_object_mut().unwrap();
    map.insert("revision".into(), json!(next_item_rev));
    map.insert("severity".into(), json!(next_severity));
    map.insert("domain".into(), json!(next_domain));
    map.insert("status".into(), json!(next_status));
    map.insert("probes".into(), json!(next_probes));
    map.insert("evidence_for".into(), json!(next_for));
    map.insert("evidence_against".into(), json!(next_against));
    map.insert("resolution".into(), json!(next_resolution));
    map.insert("updated_at_ms".into(), json!(projects::now_ms()));

    items[idx] = item.clone();
    write_unknowns_doc(project_dir, next_ledger_rev, &items)?;

    Ok(json!({
        "revision": next_ledger_rev,
        "unknown": item,
    }))
}

// -------------------------------------------------- mechanics.json provenance

/// Check `docs/mechanics.json` or `mechanics.json` (or an explicit jailed project-relative
/// path) for provenance coverage (`sourced` / `estimated` / `unsourced`) and warn whenever
/// literal config values are cited without verified runtime behavior.
pub fn check_mechanics(
    project_dir: &Path,
    rel_path: Option<&str>,
    strict: bool,
) -> Result<Value, StudyError> {
    let resolved: Option<(String, PathBuf)> = match rel_path.map(str::trim).filter(|s| !s.is_empty()) {
        Some(custom) => {
            let full = projects::safe_join(project_dir, custom).map_err(StudyError::bad)?;
            let disp = projects::rel_display(project_dir, &full);
            if projects::is_blocked_git_rel(&disp) {
                return Err(StudyError::bad(format!("refusing blocked path '{disp}'")));
            }
            if !full.is_file() {
                return Err(StudyError::not_found(format!(
                    "mechanics file '{disp}' does not exist"
                )));
            }
            Some((disp, full))
        }
        None => {
            let candidates = ["docs/mechanics.json", "mechanics.json"];
            candidates.iter().find_map(|rel| {
                let p = project_dir.join(rel);
                p.is_file().then(|| ((*rel).to_string(), p))
            })
        }
    };

    let Some((disp_path, full_path)) = resolved else {
        return Ok(json!({
            "found": false,
            "passed": false,
            "path": Value::Null,
            "counts": {
                "total": 0,
                "sourced": 0,
                "estimated": 0,
                "unsourced": 0,
                "runtime_verified": 0,
                "config_only_warnings": 0,
            },
            "warnings": ["No mechanics.json found (checked docs/mechanics.json and mechanics.json)"],
            "items": [],
        }));
    };

    let text = std::fs::read_to_string(&full_path)
        .map_err(|e| StudyError::internal(format!("read {disp_path}: {e}")))?;
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| StudyError::bad(format!("{disp_path} is not valid JSON: {e}")))?;

    let raw_items: Vec<Value> = if let Some(arr) = doc.get("mechanics").and_then(Value::as_array) {
        arr.clone()
    } else if let Some(arr) = doc.as_array() {
        arr.clone()
    } else {
        return Err(StudyError::bad(format!(
            "{disp_path} must be a JSON array or an object with a 'mechanics' array"
        )));
    };

    let known_evidence: BTreeSet<String> = read_all_evidence(project_dir)?
        .into_iter()
        .filter_map(|e| e["id"].as_str().map(String::from))
        .collect();

    let mut sourced = 0usize;
    let mut estimated = 0usize;
    let mut unsourced = 0usize;
    let mut runtime_verified = 0usize;
    let mut config_only_warnings = 0usize;
    let mut config_mismatches = 0usize;
    let mut warnings: Vec<String> = Vec::new();
    let mut audited_items: Vec<Value> = Vec::with_capacity(raw_items.len());

    for (idx, item) in raw_items.iter().enumerate() {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("mechanic_{}", idx + 1));
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string();
        let domain = item
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or("general")
            .to_string();

        let source_ref = item
            .get("evidence_id")
            .or_else(|| item.get("source"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        let raw_prov = item
            .get("provenance")
            .or_else(|| item.get("status"))
            .or_else(|| item.get("source_status"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_ascii_lowercase());

        let mut item_warnings: Vec<String> = Vec::new();
        let effective_prov = match raw_prov.as_deref() {
            Some("sourced") => {
                match source_ref.as_deref() {
                    None => {
                        item_warnings.push(format!(
                            "mechanic '{id}': claimed 'sourced' with no 'source' or 'evidence_id'; counted as unsourced"
                        ));
                        "unsourced"
                    }
                    Some(ev) if ev.starts_with("ev_") && !known_evidence.contains(ev) => {
                        item_warnings.push(format!(
                            "mechanic '{id}': cites missing evidence id '{ev}' not found in evidence.jsonl; counted as unsourced"
                        ));
                        "unsourced"
                    }
                    Some(_) => "sourced",
                }
            }
            Some("estimated") => "estimated",
            Some("unsourced") => "unsourced",
            Some(other) => {
                item_warnings.push(format!(
                    "mechanic '{id}': unrecognized provenance '{other}'; counted as unsourced"
                ));
                "unsourced"
            }
            None => match source_ref.as_deref() {
                Some(ev) if ev.starts_with("ev_") && !known_evidence.contains(ev) => {
                    item_warnings.push(format!(
                        "mechanic '{id}': cites missing evidence id '{ev}'; counted as unsourced"
                    ));
                    "unsourced"
                }
                Some(_) => "sourced",
                None => "unsourced",
            },
        };

        match effective_prov {
            "sourced" => sourced += 1,
            "estimated" => estimated += 1,
            _ => unsourced += 1,
        }

        let verified_rt = item
            .get("verified_runtime")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if verified_rt {
            runtime_verified += 1;
        }

        let has_config = item.get("config_path").is_some_and(|v| !v.is_null())
            || item.get("config_value").is_some_and(|v| !v.is_null())
            || item.get("config").is_some_and(|v| !v.is_null());
        if has_config {
            let cfg_val = item.get("config_value").or_else(|| item.get("config"));
            let rt_val = item.get("runtime_value");
            if let (Some(cv), Some(rv)) = (cfg_val, rt_val) {
                if cv != rv {
                    config_mismatches += 1;
                    item_warnings.push(format!(
                        "mechanic '{id}': literal config ({cv}) != verified runtime ({rv})"
                    ));
                }
            } else if !verified_rt {
                config_only_warnings += 1;
                let cfg_label = item
                    .get("config_path")
                    .and_then(Value::as_str)
                    .unwrap_or("config");
                item_warnings.push(format!(
                    "mechanic '{id}': cites literal config ({cfg_label}) without verified_runtime=true — literal config != verified runtime behavior"
                ));
            }
        }

        for w in &item_warnings {
            warnings.push(w.clone());
        }
        audited_items.push(json!({
            "id": id,
            "name": name,
            "domain": domain,
            "provenance": effective_prov,
            "source": source_ref,
            "verified_runtime": verified_rt,
            "warnings": item_warnings,
        }));
    }

    let total = raw_items.len();
    let passed = total > 0
        && unsourced == 0
        && config_mismatches == 0
        && (!strict || (estimated == 0 && config_only_warnings == 0));

    Ok(json!({
        "found": true,
        "passed": passed,
        "strict": strict,
        "path": disp_path,
        "counts": {
            "total": total,
            "sourced": sourced,
            "estimated": estimated,
            "unsourced": unsourced,
            "runtime_verified": runtime_verified,
            "config_only_warnings": config_only_warnings,
            "config_mismatches": config_mismatches,
        },
        "warnings": warnings,
        "items": audited_items,
    }))
}

// -------------------------------------------------- playtest core-loop claims

fn classify_core_claim(name: &str, explicit: Option<&str>) -> Option<&'static str> {
    if let Some(tag) = explicit.map(|s| s.trim().to_ascii_lowercase()) {
        match tag.as_str() {
            "move" | "movement" | "spawn" | "locomotion" => return Some("move"),
            "act" | "action" | "interact" | "combat" | "roll" | "battle" => return Some("act"),
            "hud" | "ui" | "gui" | "screen" => return Some("hud"),
            _ => {}
        }
    }
    let low = name.to_ascii_lowercase();
    let has_word = |words: &[&str]| -> bool {
        let tokens: Vec<&str> = low
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        words.iter().any(|w| tokens.contains(w))
    };
    if has_word(&["move", "movement", "walk", "spawn", "locomotion", "character", "teleport", "position"]) {
        Some("move")
    } else if has_word(&["act", "action", "attack", "roll", "gacha", "battle", "interact", "trigger", "fire", "equip", "buy", "claim", "craft"]) {
        Some("act")
    } else if has_word(&["hud", "gui", "ui", "screen", "banner", "topbar", "menu", "panel", "inventory", "label", "screengui"]) {
        Some("hud")
    } else {
        None
    }
}

/// Evaluate Craft Anything core-loop playtest claims (`move`, `act`, `hud`) plus
/// suite steps. Missing claims are reported honestly as `"not tested"`.
pub fn evaluate_core_loop(
    check_results: &[Value],
    spec_checks: &[Value],
    console_errors: &[String],
    require_core_loop: bool,
) -> Value {
    let mut move_checks: Vec<(String, bool)> = Vec::new();
    let mut act_checks: Vec<(String, bool)> = Vec::new();
    let mut hud_checks: Vec<(String, bool)> = Vec::new();
    let mut suite_steps: Vec<Value> = Vec::new();

    for (idx, res) in check_results.iter().enumerate() {
        let name = res["name"].as_str().unwrap_or("check").to_string();
        let passed = res["passed"].as_bool().unwrap_or(false);
        let spec = spec_checks.get(idx);
        let explicit = spec
            .and_then(|s| s.get("claim").or_else(|| s.get("category")).or_else(|| s.get("step")))
            .and_then(Value::as_str);
        let claim = classify_core_claim(&name, explicit);
        match claim {
            Some("move") => move_checks.push((name.clone(), passed)),
            Some("act") => act_checks.push((name.clone(), passed)),
            Some("hud") => hud_checks.push((name.clone(), passed)),
            _ => {}
        }
        suite_steps.push(json!({
            "name": name,
            "claim": claim,
            "status": if passed { "passed" } else { "failed" },
        }));
    }

    let no_console_errors = console_errors.is_empty();
    let summarize = |list: &[(String, bool)]| -> Value {
        if list.is_empty() {
            json!({
                "status": "not tested",
                "tested": false,
                "passed": false,
                "checks": [],
            })
        } else {
            let ok = no_console_errors && list.iter().all(|(_, p)| *p);
            json!({
                "status": if ok { "passed" } else { "failed" },
                "tested": true,
                "passed": ok,
                "checks": list.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            })
        }
    };

    let mv = summarize(&move_checks);
    let ac = summarize(&act_checks);
    let hd = summarize(&hud_checks);

    let all_tested = mv["tested"].as_bool().unwrap_or(false)
        && ac["tested"].as_bool().unwrap_or(false)
        && hd["tested"].as_bool().unwrap_or(false);
    let all_passed = mv["passed"].as_bool().unwrap_or(false)
        && ac["passed"].as_bool().unwrap_or(false)
        && hd["passed"].as_bool().unwrap_or(false);

    json!({
        "move": mv,
        "act": ac,
        "hud": hd,
        "all_core_tested": all_tested,
        "all_core_passed": all_passed,
        "require_core_loop": require_core_loop,
        "suite_steps": suite_steps,
    })
}

// ----------------------------------------- pure-Rust PNG decode & visual check

/// Decoded RGB image buffer (`width * height * 3` bytes, row-major R,G,B).
#[derive(Clone, Debug)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u64,
    bit_count: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_buf: 0, bit_count: 0 }
    }
    fn fill(&mut self, need: u32) -> Result<(), String> {
        while self.bit_count < need {
            if self.byte_pos >= self.data.len() {
                return Err("unexpected EOF in deflate stream".into());
            }
            self.bit_buf |= (self.data[self.byte_pos] as u64) << self.bit_count;
            self.byte_pos += 1;
            self.bit_count += 8;
        }
        Ok(())
    }
    fn read_bits(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n)?;
        let v = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.bit_buf >>= n;
        self.bit_count -= n;
        Ok(v)
    }
    fn align_byte(&mut self) {
        let drop_bits = self.bit_count % 8;
        self.bit_buf >>= drop_bits;
        self.bit_count -= drop_bits;
    }
    fn read_aligned_u16(&mut self) -> Result<u16, String> {
        let lo = self.read_bits(8)? as u16;
        let hi = self.read_bits(8)? as u16;
        Ok(lo | (hi << 8))
    }
    fn read_aligned_bytes(&mut self, len: usize, out: &mut Vec<u8>, max_out: usize) -> Result<(), String> {
        if out.len().saturating_add(len) > max_out {
            return Err("deflate output exceeded safety limit".into());
        }
        let mut rem = len;
        while rem > 0 && self.bit_count >= 8 {
            out.push((self.bit_buf & 0xff) as u8);
            self.bit_buf >>= 8;
            self.bit_count -= 8;
            rem -= 1;
        }
        if self.byte_pos.saturating_add(rem) > self.data.len() {
            return Err("unexpected EOF in uncompressed deflate block".into());
        }
        out.extend_from_slice(&self.data[self.byte_pos..self.byte_pos + rem]);
        self.byte_pos += rem;
        Ok(())
    }
}

struct HuffTable {
    max_bits: u32,
    counts: [u32; 16],
    symbols: Vec<u16>,
}

impl HuffTable {
    fn from_lengths(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u32; 16];
        let mut max_bits = 0u32;
        for &len in lengths {
            if len > 15 {
                return Err("huffman code length > 15".into());
            }
            if len > 0 {
                counts[len as usize] += 1;
                if (len as u32) > max_bits {
                    max_bits = len as u32;
                }
            }
        }
        let mut offsets = [0usize; 16];
        let mut total = 0usize;
        for b in 1..=15 {
            offsets[b] = total;
            total += counts[b] as usize;
        }
        let mut symbols = vec![0u16; total];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let idx = offsets[len as usize];
                symbols[idx] = sym as u16;
                offsets[len as usize] += 1;
            }
        }
        Ok(Self { max_bits, counts, symbols })
    }

    fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=self.max_bits as usize {
            code |= br.read_bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                let pos = (index + (code - first)) as usize;
                return self
                    .symbols
                    .get(pos)
                    .copied()
                    .ok_or_else(|| "invalid huffman symbol".into());
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("invalid huffman code".into())
    }
}

fn inflate_zlib(zlib: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    if zlib.len() < 6 {
        return Err("zlib stream too short".into());
    }
    let cmf = zlib[0] as u16;
    let flg = zlib[1] as u16;
    if (cmf & 0x0f) != 8 || !((cmf << 8) | flg).is_multiple_of(31) {
        return Err("invalid zlib header".into());
    }
    let start = if (flg & 0x20) != 0 { 6 } else { 2 };
    if start > zlib.len() {
        return Err("truncated zlib header".into());
    }
    let mut br = BitReader::new(&zlib[start..]);
    let mut out = Vec::new();

    const LEN_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
        115, 131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
        1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
        12, 13, 13,
    ];
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;
        match btype {
            0 => {
                br.align_byte();
                let len = br.read_aligned_u16()?;
                let nlen = br.read_aligned_u16()?;
                if len != !nlen {
                    return Err("corrupt uncompressed deflate block length".into());
                }
                br.read_aligned_bytes(len as usize, &mut out, max_out)?;
            }
            1 | 2 => {
                let (lit_table, dist_table) = if btype == 1 {
                    let mut lit_lens = vec![0u8; 288];
                    for l in &mut lit_lens[0..=143] {
                        *l = 8;
                    }
                    for l in &mut lit_lens[144..=255] {
                        *l = 9;
                    }
                    for l in &mut lit_lens[256..=279] {
                        *l = 7;
                    }
                    for l in &mut lit_lens[280..=287] {
                        *l = 8;
                    }
                    let dist_lens = vec![5u8; 32];
                    (HuffTable::from_lengths(&lit_lens)?, HuffTable::from_lengths(&dist_lens)?)
                } else {
                    let hlit = br.read_bits(5)? as usize + 257;
                    let hdist = br.read_bits(5)? as usize + 1;
                    let hclen = br.read_bits(4)? as usize + 4;
                    let mut cl_lens = [0u8; 19];
                    for i in 0..hclen {
                        cl_lens[CL_ORDER[i]] = br.read_bits(3)? as u8;
                    }
                    let cl_table = HuffTable::from_lengths(&cl_lens)?;
                    let total_codes = hlit + hdist;
                    let mut all_lens = Vec::with_capacity(total_codes);
                    while all_lens.len() < total_codes {
                        let sym = cl_table.decode(&mut br)?;
                        match sym {
                            0..=15 => all_lens.push(sym as u8),
                            16 => {
                                let prev = *all_lens
                                    .last()
                                    .ok_or_else(|| "repeat code 16 with no previous length".to_string())?;
                                let rep = br.read_bits(2)? as usize + 3;
                                for _ in 0..rep {
                                    if all_lens.len() < total_codes {
                                        all_lens.push(prev);
                                    }
                                }
                            }
                            17 => {
                                let rep = br.read_bits(3)? as usize + 3;
                                for _ in 0..rep {
                                    if all_lens.len() < total_codes {
                                        all_lens.push(0);
                                    }
                                }
                            }
                            18 => {
                                let rep = br.read_bits(7)? as usize + 11;
                                for _ in 0..rep {
                                    if all_lens.len() < total_codes {
                                        all_lens.push(0);
                                    }
                                }
                            }
                            _ => return Err("invalid code-length symbol".into()),
                        }
                    }
                    (
                        HuffTable::from_lengths(&all_lens[..hlit])?,
                        HuffTable::from_lengths(&all_lens[hlit..hlit + hdist])?,
                    )
                };

                loop {
                    let sym = lit_table.decode(&mut br)?;
                    match sym {
                        0..=255 => {
                            if out.len() >= max_out {
                                return Err("deflate output exceeded safety limit".into());
                            }
                            out.push(sym as u8);
                        }
                        256 => break,
                        257..=285 => {
                            let idx = (sym - 257) as usize;
                            let length = LEN_BASE[idx] + br.read_bits(LEN_EXTRA[idx])? as usize;
                            let dsym = dist_table.decode(&mut br)? as usize;
                            if dsym >= 30 {
                                return Err("invalid distance symbol".into());
                            }
                            let dist = DIST_BASE[dsym] + br.read_bits(DIST_EXTRA[dsym])? as usize;
                            if dist == 0 || dist > out.len() {
                                return Err("invalid LZ77 back-reference distance".into());
                            }
                            if out.len().saturating_add(length) > max_out {
                                return Err("deflate output exceeded safety limit".into());
                            }
                            for _ in 0..length {
                                let b = out[out.len() - dist];
                                out.push(b);
                            }
                        }
                        _ => return Err("invalid literal/length symbol".into()),
                    }
                }
            }
            _ => return Err("invalid deflate block type 3".into()),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let ai = a as i32;
    let bi = b as i32;
    let ci = c as i32;
    let p = ai + bi - ci;
    let pa = (p - ai).abs();
    let pb = (p - bi).abs();
    let pc = (p - ci).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Pure-Rust zero-dependency PNG decoder returning packed 8-bit RGB (`w * h * 3`).
pub fn decode_png_rgb(bytes: &[u8]) -> Result<RgbImage, String> {
    if bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not a valid PNG header".into());
    }
    let mut pos = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut saw_ihdr = false;
    let mut plte: Vec<[u8; 3]> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();

    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.saturating_add(len);
        if data_end + 4 > bytes.len() {
            return Err("truncated PNG chunk".into());
        }
        let chunk = &bytes[data_start..data_end];
        match kind {
            b"IHDR" => {
                if chunk.len() != 13 {
                    return Err("invalid IHDR chunk length".into());
                }
                width = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                height = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                bit_depth = chunk[8];
                color_type = chunk[9];
                interlace = chunk[12];
                saw_ihdr = true;
            }
            b"PLTE" => {
                for tri in chunk.chunks_exact(3) {
                    plte.push([tri[0], tri[1], tri[2]]);
                }
            }
            b"IDAT" => idat.extend_from_slice(chunk),
            b"IEND" => break,
            _ => {}
        }
        pos = data_end + 4;
    }

    if !saw_ihdr || width == 0 || height == 0 || width > MAX_PNG_DIM || height > MAX_PNG_DIM {
        return Err("invalid PNG dimensions".into());
    }
    if (width as u64) * (height as u64) > MAX_PNG_PIXELS {
        return Err("PNG pixel count exceeds safety limit".into());
    }
    if interlace != 0 {
        return Err("interlaced PNG not supported in fast decoder".into());
    }
    if bit_depth != 8 && bit_depth != 16 {
        return Err(format!("unsupported PNG bit depth {bit_depth}"));
    }
    let channels: usize = match color_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        other => return Err(format!("unsupported PNG color type {other}")),
    };
    let bytes_per_sample = (bit_depth as usize) / 8;
    let bpp = channels * bytes_per_sample;
    let row_bytes = (width as usize) * bpp;
    let expected_raw = (height as usize) * (row_bytes + 1);
    let raw = inflate_zlib(&idat, expected_raw + 64)?;
    if raw.len() < expected_raw {
        return Err(format!(
            "decompressed PNG data too short ({} < {})",
            raw.len(),
            expected_raw
        ));
    }

    let mut prev_row = vec![0u8; row_bytes];
    let mut cur_row = vec![0u8; row_bytes];
    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);

    for y in 0..height as usize {
        let row_off = y * (row_bytes + 1);
        let filter = raw[row_off];
        let src = &raw[row_off + 1..row_off + 1 + row_bytes];
        for i in 0..row_bytes {
            let x = src[i];
            let a = if i >= bpp { cur_row[i - bpp] } else { 0 };
            let b = prev_row[i];
            let c = if i >= bpp { prev_row[i - bpp] } else { 0 };
            cur_row[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add((((a as u16) + (b as u16)) >> 1) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                other => return Err(format!("unsupported PNG filter {other}")),
            };
        }
        match (color_type, bytes_per_sample) {
            (2, 1) => rgb.extend_from_slice(&cur_row),
            (6, 1) => {
                for px in cur_row.chunks_exact(4) {
                    rgb.extend_from_slice(&px[..3]);
                }
            }
            (0, 1) => {
                for &g in &cur_row {
                    rgb.extend_from_slice(&[g, g, g]);
                }
            }
            (4, 1) => {
                for px in cur_row.chunks_exact(2) {
                    rgb.extend_from_slice(&[px[0], px[0], px[0]]);
                }
            }
            (3, 1) => {
                for &idx in &cur_row {
                    let c = plte.get(idx as usize).copied().unwrap_or([0, 0, 0]);
                    rgb.extend_from_slice(&c);
                }
            }
            (2, 2) => {
                for px in cur_row.chunks_exact(6) {
                    rgb.extend_from_slice(&[px[0], px[2], px[4]]);
                }
            }
            (6, 2) => {
                for px in cur_row.chunks_exact(8) {
                    rgb.extend_from_slice(&[px[0], px[2], px[4]]);
                }
            }
            (0, 2) => {
                for px in cur_row.chunks_exact(2) {
                    rgb.extend_from_slice(&[px[0], px[0], px[0]]);
                }
            }
            (4, 2) => {
                for px in cur_row.chunks_exact(4) {
                    rgb.extend_from_slice(&[px[0], px[0], px[0]]);
                }
            }
            _ => return Err("unsupported PNG color/depth combination".into()),
        }
        std::mem::swap(&mut prev_row, &mut cur_row);
    }

    Ok(RgbImage { width, height, rgb })
}

/// Decode a PNG (pure Rust) or fallback to Pillow (`python3`) for JPEG/WEBP/interlaced reference images.
fn decode_image_any(path: &Path) -> Result<RgbImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if let Ok(img) = decode_png_rgb(&bytes) {
            return Ok(img);
        }
    }
    decode_via_pillow(path)
}

fn decode_via_pillow(path: &Path) -> Result<RgbImage, String> {
    let py = std::env::var("OPENRO_PYTHON_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "python3".to_string());
    let script = "import sys; from PIL import Image; im=Image.open(sys.argv[1]).convert('RGB').resize((640,360)); sys.stdout.buffer.write(im.tobytes())";
    let out = std::process::Command::new(py)
        .args(["-c", script])
        .arg(path)
        .output()
        .map_err(|e| format!("pillow decode unavailable: {e}"))?;
    let expected = (LIKENESS_NORM_W as usize) * (LIKENESS_NORM_H as usize) * 3;
    if !out.status.success() || out.stdout.len() != expected {
        return Err("pillow failed to decode image".into());
    }
    Ok(RgbImage { width: LIKENESS_NORM_W, height: LIKENESS_NORM_H, rgb: out.stdout })
}

/// Resize an `RgbImage` to normalized `(target_w, target_h)` using bilinear interpolation.
pub fn resize_rgb_bilinear(img: &RgbImage, target_w: u32, target_h: u32) -> RgbImage {
    if img.width == target_w && img.height == target_h {
        return img.clone();
    }
    let sw = img.width as usize;
    let sh = img.height as usize;
    let tw = target_w as usize;
    let th = target_h as usize;
    let mut out = vec![0u8; tw * th * 3];
    let scale_x = (sw as f32) / (tw as f32);
    let scale_y = (sh as f32) / (th as f32);

    for ty in 0..th {
        let fy = ((ty as f32 + 0.5) * scale_y - 0.5).clamp(0.0, (sh.saturating_sub(1)) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(sh.saturating_sub(1));
        let wy = fy - (y0 as f32);
        let wy0 = 1.0 - wy;
        for tx in 0..tw {
            let fx = ((tx as f32 + 0.5) * scale_x - 0.5).clamp(0.0, (sw.saturating_sub(1)) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(sw.saturating_sub(1));
            let wx = fx - (x0 as f32);
            let wx0 = 1.0 - wx;

            let i00 = (y0 * sw + x0) * 3;
            let i10 = (y0 * sw + x1) * 3;
            let i01 = (y1 * sw + x0) * 3;
            let i11 = (y1 * sw + x1) * 3;
            let dst = (ty * tw + tx) * 3;
            for c in 0..3 {
                let top = img.rgb[i00 + c] as f32 * wx0 + img.rgb[i10 + c] as f32 * wx;
                let bot = img.rgb[i01 + c] as f32 * wx0 + img.rgb[i11 + c] as f32 * wx;
                out[dst + c] = (top * wy0 + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    RgbImage { width: target_w, height: target_h, rgb: out }
}

/// Inspect screenshot PNG/image bytes and reject corrupt, blank, or single-color frames.
pub fn inspect_screenshot_bytes(bytes: &[u8]) -> (Option<RgbImage>, Value) {
    let img = match decode_png_rgb(bytes) {
        Ok(i) => i,
        Err(e) => {
            return (
                None,
                json!({
                    "valid": false,
                    "blank": true,
                    "reason": format!("unreadable PNG: {e}"),
                }),
            );
        }
    };
    let n = (img.width as usize) * (img.height as usize);
    if n == 0 {
        return (
            None,
            json!({ "valid": false, "blank": true, "reason": "empty image" }),
        );
    }

    let mut min_rgb = [255u8; 3];
    let mut max_rgb = [0u8; 3];
    let mut sum_luma = 0.0f64;
    let mut sum_luma_sq = 0.0f64;
    let mut buckets = vec![0u32; 4096];
    let mut distinct = 0u32;

    for px in img.rgb.chunks_exact(3) {
        let (r, g, b) = (px[0], px[1], px[2]);
        min_rgb[0] = min_rgb[0].min(r);
        min_rgb[1] = min_rgb[1].min(g);
        min_rgb[2] = min_rgb[2].min(b);
        max_rgb[0] = max_rgb[0].max(r);
        max_rgb[1] = max_rgb[1].max(g);
        max_rgb[2] = max_rgb[2].max(b);
        let luma = 0.299 * (r as f64) + 0.587 * (g as f64) + 0.114 * (b as f64);
        sum_luma += luma;
        sum_luma_sq += luma * luma;
        let idx = (((r as usize) >> 4) << 8) | (((g as usize) >> 4) << 4) | ((b as usize) >> 4);
        if buckets[idx] == 0 {
            distinct += 1;
        }
        buckets[idx] += 1;
    }

    let nf = n as f64;
    let mean = sum_luma / nf;
    let var = (sum_luma_sq / nf - mean * mean).max(0.0);
    let stddev = var.sqrt();
    let max_spread = (max_rgb[0] - min_rgb[0])
        .max(max_rgb[1] - min_rgb[1])
        .max(max_rgb[2] - min_rgb[2]);
    let max_bucket = buckets.iter().copied().max().unwrap_or(0);
    let dominant_ratio = (max_bucket as f64) / nf;

    let (blank, reason): (bool, Option<String>) = if max_spread <= 3 {
        (
            true,
            Some(format!(
                "single-color / blank screenshot (channel spread {max_spread}, mean luma {mean:.1})"
            )),
        )
    } else if stddev < 1.5 && distinct <= 2 && dominant_ratio > 0.995 {
        (
            true,
            Some(format!(
                "near-blank screenshot (luma stddev {stddev:.2}, {distinct} color bucket(s))"
            )),
        )
    } else {
        (false, None)
    };

    let meta = json!({
        "valid": true,
        "blank": blank,
        "width": img.width,
        "height": img.height,
        "luma_mean": (mean * 10.0).round() / 10.0,
        "luma_stddev": (stddev * 100.0).round() / 100.0,
        "distinct_colors": distinct,
        "reason": reason,
    });
    (Some(img), meta)
}

/// Normalized `640x360` perceptual likeness score (`0.0..=1.0`) combining
/// block-level SSIM luminance similarity, RGB color distribution similarity, and
/// spatial edge-gradient similarity.
pub fn perceptual_likeness_640x360(a: &RgbImage, b: &RgbImage) -> f64 {
    let na = resize_rgb_bilinear(a, LIKENESS_NORM_W, LIKENESS_NORM_H);
    let nb = resize_rgb_bilinear(b, LIKENESS_NORM_W, LIKENESS_NORM_H);

    // Divide normalized 640x360 canvas into 32x18 grid of 20x20 cells.
    const GX: usize = 32;
    const GY: usize = 18;
    const CELL: usize = 20;
    const CELLS: usize = GX * GY;

    let cell_stats = |img: &RgbImage| -> (Vec<f64>, Vec<[f64; 3]>) {
        let mut luma = vec![0.0f64; CELLS];
        let mut rgb = vec![[0.0f64; 3]; CELLS];
        let w = img.width as usize;
        let inv = 1.0 / ((CELL * CELL) as f64);
        for cy in 0..GY {
            for cx in 0..GX {
                let mut sr = 0.0f64;
                let mut sg = 0.0f64;
                let mut sb = 0.0f64;
                for dy in 0..CELL {
                    let row = (cy * CELL + dy) * w;
                    for dx in 0..CELL {
                        let off = (row + cx * CELL + dx) * 3;
                        sr += img.rgb[off] as f64;
                        sg += img.rgb[off + 1] as f64;
                        sb += img.rgb[off + 2] as f64;
                    }
                }
                let mr = sr * inv;
                let mg = sg * inv;
                let mb = sb * inv;
                let idx = cy * GX + cx;
                rgb[idx] = [mr, mg, mb];
                luma[idx] = 0.299 * mr + 0.587 * mg + 0.114 * mb;
            }
        }
        (luma, rgb)
    };

    let (la, ra) = cell_stats(&na);
    let (lb, rb) = cell_stats(&nb);

    // 1. SSIM over the 32x18 luminance map
    let n = CELLS as f64;
    let mu_a: f64 = la.iter().sum::<f64>() / n;
    let mu_b: f64 = lb.iter().sum::<f64>() / n;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    let mut cov = 0.0;
    for i in 0..CELLS {
        let da = la[i] - mu_a;
        let db = lb[i] - mu_b;
        var_a += da * da;
        var_b += db * db;
        cov += da * db;
    }
    var_a /= n;
    var_b /= n;
    cov /= n;
    let c1 = (0.01 * 255.0_f64).powi(2);
    let c2 = (0.03 * 255.0_f64).powi(2);
    let ssim = (((2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2))
        / ((mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2)))
        .clamp(0.0, 1.0);

    // 2. Per-cell RGB L1 color similarity
    let mut color_diff_sum = 0.0;
    for i in 0..CELLS {
        color_diff_sum += (ra[i][0] - rb[i][0]).abs()
            + (ra[i][1] - rb[i][1]).abs()
            + (ra[i][2] - rb[i][2]).abs();
    }
    let color_sim = (1.0 - color_diff_sum / (n * 3.0 * 255.0)).clamp(0.0, 1.0);

    // 3. Spatial gradient cosine similarity (layout / horizon / silhouette structure)
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for cy in 0..GY {
        for cx in 0..GX {
            let idx = cy * GX + cx;
            if cx + 1 < GX {
                let ga = la[idx + 1] - la[idx];
                let gb = lb[idx + 1] - lb[idx];
                dot += ga * gb;
                norm_a += ga * ga;
                norm_b += gb * gb;
            }
            if cy + 1 < GY {
                let ga = la[idx + GX] - la[idx];
                let gb = lb[idx + GX] - lb[idx];
                dot += ga * gb;
                norm_a += ga * ga;
                norm_b += gb * gb;
            }
        }
    }
    let grad_sim = if norm_a > 1e-6 && norm_b > 1e-6 {
        ((dot / (norm_a.sqrt() * norm_b.sqrt()) + 1.0) * 0.5).clamp(0.0, 1.0)
    } else if norm_a <= 1e-6 && norm_b <= 1e-6 {
        1.0
    } else {
        0.0
    };

    (0.45 * ssim + 0.35 * color_sim + 0.20 * grad_sim).clamp(0.0, 1.0)
}

fn find_reference_thumbnails(project_dir: &Path) -> (Option<String>, Vec<(String, PathBuf)>) {
    let manifest_uid = projects::manifest(project_dir)
        .ok()
        .and_then(|m| m.get("universeId").and_then(Value::as_u64));
    let refs_root = project_dir.join("refs");
    if !refs_root.is_dir() {
        return (None, Vec::new());
    }

    let collect_dir = |dir: &Path| -> Vec<(String, PathBuf)> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp"))
            })
            .collect();
        files.sort();
        files
            .into_iter()
            .take(6)
            .map(|p| (projects::rel_display(project_dir, &p), p))
            .collect()
    };

    if let Some(uid) = manifest_uid {
        let preferred = refs_root.join(uid.to_string());
        if preferred.is_dir() {
            let items = collect_dir(&preferred);
            if !items.is_empty() {
                return (Some(format!("refs/{uid}")), items);
            }
        }
    }

    // Fallback: first refs/<universeId>/ directory that contains reference thumbnails.
    if let Ok(rd) = std::fs::read_dir(&refs_root) {
        let mut subdirs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            let items = collect_dir(&sub);
            if !items.is_empty() {
                return (Some(projects::rel_display(project_dir, &sub)), items);
            }
        }
    }

    (None, Vec::new())
}

/// Score captured non-blank screenshots against `refs/<universeId>/` thumbnails
/// normalized to `640x360`.
pub fn evaluate_visual_likeness(
    project_dir: &Path,
    shots: &[(String, RgbImage)],
) -> Value {
    let (ref_dir, ref_paths) = find_reference_thumbnails(project_dir);
    if shots.is_empty() || ref_paths.is_empty() {
        return json!({
            "normalized_size": [LIKENESS_NORM_W, LIKENESS_NORM_H],
            "reference_dir": ref_dir,
            "references_compared": 0,
            "best_score": Value::Null,
            "best_pct": Value::Null,
            "best_reference": Value::Null,
            "comparisons": [],
        });
    }

    let mut decoded_refs: Vec<(String, RgbImage)> = Vec::new();
    for (rel, path) in &ref_paths {
        if let Ok(img) = decode_image_any(path) {
            decoded_refs.push((rel.clone(), img));
        }
    }
    if decoded_refs.is_empty() {
        return json!({
            "normalized_size": [LIKENESS_NORM_W, LIKENESS_NORM_H],
            "reference_dir": ref_dir,
            "references_compared": 0,
            "best_score": Value::Null,
            "best_pct": Value::Null,
            "best_reference": Value::Null,
            "comparisons": [],
        });
    }

    let mut best_score = -1.0f64;
    let mut best_ref: Option<String> = None;
    let mut comparisons: Vec<Value> = Vec::new();
    for (shot_name, shot_img) in shots {
        let mut shot_best = -1.0f64;
        let mut shot_best_ref: Option<String> = None;
        for (ref_rel, ref_img) in &decoded_refs {
            let score = perceptual_likeness_640x360(shot_img, ref_img);
            if score > shot_best {
                shot_best = score;
                shot_best_ref = Some(ref_rel.clone());
            }
            if score > best_score {
                best_score = score;
                best_ref = Some(ref_rel.clone());
            }
        }
        if let Some(rr) = shot_best_ref {
            let rounded = (shot_best * 10000.0).round() / 10000.0;
            let pct = (shot_best * 100.0).round() as u32;
            comparisons.push(json!({
                "shot": shot_name,
                "reference": rr,
                "score": rounded,
                "pct": pct,
            }));
        }
    }

    let best_rounded = if best_score >= 0.0 {
        Some((best_score * 10000.0).round() / 10000.0)
    } else {
        None
    };
    let best_pct = if best_score >= 0.0 {
        Some((best_score * 100.0).round() as u32)
    } else {
        None
    };

    json!({
        "normalized_size": [LIKENESS_NORM_W, LIKENESS_NORM_H],
        "reference_dir": ref_dir,
        "references_compared": decoded_refs.len(),
        "best_score": best_rounded,
        "best_pct": best_pct,
        "best_reference": best_ref,
        "comparisons": comparisons,
    })
}

// -------------------------------------------- KeptChecks SHA-256 fingerprinting

fn is_ignored_for_fingerprint(rel: &str) -> bool {
    let clean = rel.trim_start_matches("./");
    clean.is_empty()
        || clean == ".git"
        || clean.starts_with(".git/")
        || clean == ".openro"
        || clean.starts_with(".openro/")
        || clean == "build"
        || clean.starts_with("build/")
        || clean == "node_modules"
        || clean.starts_with("node_modules/")
        || clean == "art/references"
        || clean.starts_with("art/references/")
        || clean == "art/previews"
        || clean.starts_with("art/previews/")
        || clean.ends_with(".rbxl")
        || clean.ends_with(".rbxlx")
}

/// Deterministic SHA-256 fingerprint across all project source, manifest, config,
/// mechanics, and reference inputs (excluding `.git/`, `.openro/`, `build/`, `*.rbxl*`).
pub fn project_fingerprint(project_dir: &Path) -> Result<String, String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let rel = projects::rel_display(root, &p);
            if is_ignored_for_fingerprint(&rel) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                walk(root, &p, out);
            } else if meta.is_file() {
                out.push((rel, p));
            }
        }
    }
    walk(project_dir, project_dir, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    hasher.update(b"openro-kept-checks-v1\n");
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        if let Ok(meta) = std::fs::metadata(&path) {
            let len = meta.len();
            hasher.update(len.to_le_bytes());
            if len <= MAX_FINGERPRINT_FILE_BYTES {
                if let Ok(bytes) = std::fs::read(&path) {
                    let fh = Sha256::digest(&bytes);
                    hasher.update(fh);
                }
            } else if let Ok(mt) = meta.modified() {
                let secs = mt
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                hasher.update(secs.to_le_bytes());
            }
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    Ok(out)
}

fn kept_cache_key(kind: &str, fingerprint: &str, mode_spec: &Value) -> String {
    let payload = json!({
        "kind": kind,
        "fingerprint": fingerprint,
        "mode": mode_spec,
    });
    sha256_hex(canonical_json(&payload).as_bytes())
}

fn read_kept_doc(project_dir: &Path) -> Map<String, Value> {
    let path = investigation_dir(project_dir).join(KEPT_CHECKS_FILE);
    let Ok(text) = std::fs::read_to_string(path) else {
        return Map::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Map::new();
    };
    v.get("entries")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn write_kept_doc(project_dir: &Path, mut entries: Map<String, Value>) {
    if entries.len() > MAX_KEPT_ENTRIES {
        let mut by_age: Vec<(String, u64)> = entries
            .iter()
            .map(|(k, v)| (k.clone(), v["cached_at_ms"].as_u64().unwrap_or(0)))
            .collect();
        by_age.sort_by_key(|(_, ts)| *ts);
        let drop_n = entries.len() - MAX_KEPT_ENTRIES;
        for (k, _) in by_age.into_iter().take(drop_n) {
            entries.remove(&k);
        }
    }
    let doc = json!({ "version": 1, "entries": entries });
    if let Ok(dir) = ensure_investigation_dir(project_dir) {
        if let Ok(s) = serde_json::to_string_pretty(&doc) {
            let _ = write_atomic(&dir.join(KEPT_CHECKS_FILE), (s + "\n").as_bytes());
        }
    }
}

/// Look up a previously passing cached `check` or `playtest` result for the
/// current project SHA-256 fingerprint and mode/spec. Returns `None` if `force`
/// is true or no passing entry exists.
pub fn lookup_kept_check(
    project_dir: &Path,
    kind: &str,
    fingerprint: &str,
    mode_spec: &Value,
    force: bool,
) -> Option<Value> {
    if force {
        return None;
    }
    let key = kept_cache_key(kind, fingerprint, mode_spec);
    let _guard = ledger_lock().lock().unwrap();
    let entries = read_kept_doc(project_dir);
    let entry = entries.get(&key)?;
    let mut result = entry.get("result")?.clone();
    // Safety invariant: NEVER return a non-passing entry from cache.
    if result["passed"].as_bool() != Some(true) {
        return None;
    }
    if let Some(o) = result.as_object_mut() {
        o.insert("kept".into(), json!(true));
        o.insert("cached".into(), json!(true));
        o.insert("fingerprint".into(), json!(fingerprint));
        o.insert("cached_at_ms".into(), entry["cached_at_ms"].clone());
    }
    Some(result)
}

/// Persist a passing `check` or `playtest` result under `(kind, fingerprint, mode_spec)`.
/// STRICT INVARIANT: Failures (`passed != true`) are NEVER cached and evict any stale entry.
pub fn store_kept_check(
    project_dir: &Path,
    kind: &str,
    fingerprint: &str,
    mode_spec: &Value,
    result: &Value,
) {
    let key = kept_cache_key(kind, fingerprint, mode_spec);
    let _guard = ledger_lock().lock().unwrap();
    let mut entries = read_kept_doc(project_dir);
    if result["passed"].as_bool() != Some(true) {
        if entries.remove(&key).is_some() {
            write_kept_doc(project_dir, entries);
        }
        return;
    }
    let mut stored = result.clone();
    if let Some(o) = stored.as_object_mut() {
        o.remove("ok");
        o.insert("fingerprint".into(), json!(fingerprint));
    }
    entries.insert(
        key,
        json!({
            "kind": kind,
            "fingerprint": fingerprint,
            "cached_at_ms": projects::now_ms(),
            "result": stored,
        }),
    );
    write_kept_doc(project_dir, entries);
}

// ---------------------------------------------------------------- study status

/// Aggregate project study progress (`Study -> Build -> Check -> Ready`),
/// `clean_room` state, evidence/unknown counts, mechanics status, kept-check
/// fingerprint status, and latest playtest summary for agents and UI (#62-#64).
pub fn study_status(project_dir: &Path) -> Result<Value, StudyError> {
    let clean_room = project_clean_room(project_dir);
    let fp = project_fingerprint(project_dir).unwrap_or_default();
    let ev = list_evidence(project_dir, &EvidenceFilter { limit: Some(1), ..Default::default() })?;
    let unk = list_unknowns(project_dir, None, None, None)?;
    let mech = check_mechanics(project_dir, None, false)?;

    let kept_entries = {
        let _guard = ledger_lock().lock().unwrap();
        read_kept_doc(project_dir)
    };
    let static_check_kept = kept_entries.values().any(|e| {
        e["kind"].as_str() == Some("check")
            && e["fingerprint"].as_str() == Some(fp.as_str())
            && e["result"]["passed"].as_bool() == Some(true)
    });
    let playtest_kept = kept_entries.values().any(|e| {
        e["kind"].as_str() == Some("playtest")
            && e["fingerprint"].as_str() == Some(fp.as_str())
            && e["result"]["passed"].as_bool() == Some(true)
    });

    // Read latest playtest or headless sandbox report if any exists.
    let latest_playtest = crate::playtest::latest_report_summary(project_dir);
    let last_pt_passed = latest_playtest
        .as_ref()
        .and_then(|r| r["passed"].as_bool())
        .unwrap_or(false);
    let last_pt_blanks = latest_playtest
        .as_ref()
        .and_then(|r| r["blank_screenshots"].as_u64())
        .unwrap_or(0);
    let latest_sandbox = crate::sandbox::latest_verified_summary(project_dir);
    let live_pt_ok = last_pt_passed && last_pt_blanks == 0;
    let sandbox_ok = latest_sandbox
        .as_ref()
        .and_then(|s| s["verified"].as_bool())
        .unwrap_or(false);
    let verification_tier = if live_pt_ok {
        Some("live_studio")
    } else if sandbox_ok {
        Some("headless_sandbox")
    } else {
        None
    };

    let ev_total = ev["total"].as_u64().unwrap_or(0);
    let mech_found = mech["found"].as_bool().unwrap_or(false);
    let mech_passed = mech["passed"].as_bool().unwrap_or(false);
    let open_unknowns = unk["open_count"].as_u64().unwrap_or(0);
    let has_build = !projects::builds(project_dir).is_empty();

    let study_done = ev_total > 0 || mech_found;
    let build_done = has_build;
    let check_done = static_check_kept && (live_pt_ok || sandbox_ok);
    let ready_done = check_done && open_unknowns == 0 && (!mech_found || mech_passed);

    let stage = if ready_done {
        "ready"
    } else if build_done || check_done {
        "check"
    } else if study_done {
        "build"
    } else {
        "study"
    };

    Ok(json!({
        "stage": stage,
        "stages": [
            { "id": "study", "label": "Study", "done": study_done },
            { "id": "build", "label": "Build", "done": build_done },
            { "id": "check", "label": "Check", "done": check_done },
            { "id": "ready", "label": "Ready", "done": ready_done },
        ],
        "clean_room": clean_room,
        "fingerprint": fp,
        "evidence": {
            "total": ev_total,
            "observed": ev["counts_by_confidence"]["observed"],
            "derived": ev["counts_by_confidence"]["derived"],
            "inferred": ev["counts_by_confidence"]["inferred"],
        },
        "unknowns": {
            "revision": unk["revision"],
            "total": unk["total"],
            "open": unk["counts_by_status"]["open"],
            "investigating": unk["counts_by_status"]["investigating"],
            "resolved": unk["counts_by_status"]["resolved"],
            "wont_fix": unk["counts_by_status"]["wont_fix"],
            "open_count": open_unknowns,
            "critical_open": unk["critical_open"],
            "high_open": unk["high_open"],
        },
        "mechanics": {
            "found": mech_found,
            "passed": mech_passed,
            "path": mech["path"],
            "counts": mech["counts"],
            "warnings": mech["warnings"].as_array().map(|a| a.len()).unwrap_or(0),
        },
        "checks": {
            "static_check_kept": static_check_kept,
            "playtest_kept": playtest_kept,
            "last_playtest": latest_playtest,
            "last_sandbox": latest_sandbox,
            "verification_tier": verification_tier,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_uncompressed_png(w: u32, h: u32, pixel_fn: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut raw = Vec::with_capacity((h as usize) * ((w as usize) * 3 + 1));
        for y in 0..h {
            raw.push(0u8);
            for x in 0..w {
                raw.extend_from_slice(&pixel_fn(x, y));
            }
        }
        // Zlib stored blocks
        let mut z = vec![0x78u8, 0x01u8];
        let mut pos = 0usize;
        while pos < raw.len() {
            let chunk = (raw.len() - pos).min(65535);
            let final_b = if pos + chunk >= raw.len() { 1u8 } else { 0u8 };
            z.push(final_b);
            let len = chunk as u16;
            let nlen = !len;
            z.extend_from_slice(&len.to_le_bytes());
            z.extend_from_slice(&nlen.to_le_bytes());
            z.extend_from_slice(&raw[pos..pos + chunk]);
            pos += chunk;
        }
        let mut s1 = 1u32;
        let mut s2 = 0u32;
        for &b in &raw {
            s1 = (s1 + b as u32) % 65521;
            s2 = (s2 + s1) % 65521;
        }
        z.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());

        fn crc32(data: &[u8]) -> u32 {
            let mut c = 0xffffffffu32;
            for &b in data {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if (c & 1) != 0 { (c >> 1) ^ 0xedb88320 } else { c >> 1 };
                }
            }
            !c
        }
        fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut buf = Vec::with_capacity(4 + data.len());
            buf.extend_from_slice(kind);
            buf.extend_from_slice(data);
            out.extend_from_slice(&crc32(&buf).to_be_bytes());
        }

        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        push_chunk(&mut png, b"IHDR", &ihdr);
        push_chunk(&mut png, b"IDAT", &z);
        push_chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn canonical_json_and_deterministic_ids() {
        let a = json!({ "b": 1, "a": { "y": true, "x": [3, 2, 1] } });
        let b = json!({ "a": { "x": [3, 2, 1], "y": true }, "b": 1 });
        assert_eq!(canonical_json(&a), canonical_json(&b));
        let id1 = evidence_id_for(&a);
        let id2 = evidence_id_for(&b);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ev_") && id1.len() == 19);
        let u = unknown_id_for("Economy", "Pity cap", "Is pity 80?");
        assert!(u.starts_with("unk_") && u.len() == 20);
    }

    #[test]
    fn png_blank_rejection_and_640x360_perceptual_likeness() {
        let blank_png = make_uncompressed_png(64, 36, |_, _| [24, 24, 24]);
        let (_, meta_blank) = inspect_screenshot_bytes(&blank_png);
        assert_eq!(meta_blank["valid"], json!(true));
        assert_eq!(meta_blank["blank"], json!(true));

        let scene_a = make_uncompressed_png(160, 90, |x, y| {
            if y < 45 {
                [80, 160, 240]
            } else if x < 80 {
                [220, 180, 90]
            } else {
                [60, 150, 70]
            }
        });
        let (img_a, meta_a) = inspect_screenshot_bytes(&scene_a);
        assert_eq!(meta_a["valid"], json!(true));
        assert_eq!(meta_a["blank"], json!(false));
        let img_a = img_a.unwrap();

        // Identical scene at different resolution -> high likeness (~1.0)
        let scene_a_hi = make_uncompressed_png(320, 180, |x, y| {
            if y < 90 {
                [80, 160, 240]
            } else if x < 160 {
                [220, 180, 90]
            } else {
                [60, 150, 70]
            }
        });
        let (img_hi, _) = inspect_screenshot_bytes(&scene_a_hi);
        let same_score = perceptual_likeness_640x360(&img_a, &img_hi.unwrap());
        assert!(same_score > 0.95, "expected identical scene > 0.95, got {same_score}");

        // Inverted / opposite scene -> clearly lower score
        let scene_opp = make_uncompressed_png(160, 90, |x, y| {
            if y < 45 {
                [20, 20, 25]
            } else if x < 80 {
                [25, 40, 220]
            } else {
                [240, 40, 190]
            }
        });
        let (img_opp, _) = inspect_screenshot_bytes(&scene_opp);
        let diff_score = perceptual_likeness_640x360(&img_a, &img_opp.unwrap());
        assert!(
            diff_score < same_score - 0.30,
            "expected opposite scene ({diff_score}) much lower than same ({same_score})"
        );
    }
}
