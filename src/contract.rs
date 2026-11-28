//! Default-FAIL Contract Engine & Cold-Verifier-Only Grading (Gap #100):
//! Manages `<project>/.harness/contract.json` atomically with strict CWE-22/CWE-59
//! guards and enforces that only role `"verifier"` (with non-empty evidence) may
//! mark any feature `passed: true`.

use crate::opencloud::{now_iso, safe_rel, Oc};
use crate::projects::now_ms;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const CONTRACT_REL: &str = ".harness/contract.json";
const MAX_CONTRACT_BYTES: u64 = 2 * 1024 * 1024;
static CONTRACT_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn bad(msg: impl Into<String>) -> Oc {
    Oc {
        status: 400,
        msg: msg.into(),
    }
}

fn err(status: u16, msg: impl Into<String>) -> Oc {
    Oc {
        status,
        msg: msg.into(),
    }
}

fn write_atomic_json(path: &Path, value: &Value) -> Result<(), Oc> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(500, format!("create '{}': {e}", parent.display())))?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(bad(format!(
                "refusing to overwrite symlink '{}' (CWE-59)",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| err(500, "missing parent directory for contract"))?;
    let seq = CONTRACT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".contract.json.tmp-{}-{seq}", std::process::id()));
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| err(500, format!("serialize contract: {e}")))?;
    let write_res = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.flush()));
    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(500, format!("write contract tmp: {e}")));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(500, format!("commit contract: {e}")));
    }
    Ok(())
}

fn summarize_contract(doc: &Value) -> (bool, usize, usize, Vec<String>) {
    let active = doc.get("active").and_then(Value::as_bool).unwrap_or(true);
    let features = doc
        .get("features")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let total = features.len();
    let mut passed_count = 0usize;
    let mut failing_ids = Vec::new();
    for (idx, feat) in features.iter().enumerate() {
        let fid = feat
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("feature_{}", idx + 1));
        let passed = feat.get("passed").and_then(Value::as_bool).unwrap_or(false)
            && feat.get("passes").and_then(Value::as_bool).unwrap_or(false);
        if passed {
            passed_count += 1;
        } else {
            failing_ids.push(fid);
        }
    }
    (active, total, passed_count, failing_ids)
}

fn read_existing_contract(project_dir: &Path) -> Result<Option<Value>, Oc> {
    let abs = safe_rel(project_dir, CONTRACT_REL)?;
    match std::fs::symlink_metadata(&abs) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(bad("refusing symlinked .harness/contract.json (CWE-59)"));
            }
            if !meta.is_file() {
                return Err(bad(".harness/contract.json is not a regular file"));
            }
            if meta.len() > MAX_CONTRACT_BYTES {
                return Err(bad(".harness/contract.json exceeds 2 MiB safety limit"));
            }
            let raw = std::fs::read_to_string(&abs)
                .map_err(|e| err(500, format!("read contract: {e}")))?;
            let val: Value = serde_json::from_str(&raw)
                .map_err(|e| err(500, format!("corrupt .harness/contract.json: {e}")))?;
            Ok(Some(val))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(err(500, format!("stat contract: {e}"))),
    }
}

/// Initialize `<project>/.harness/contract.json` forcing every feature's
/// `"passed": false` and `"passes": false` (default-FAIL invariant).
pub fn contract_init(project_dir: &Path, body: &Value) -> Result<Value, Oc> {
    let raw_features = body
        .get("features")
        .or_else(|| body.get("checks"))
        .and_then(Value::as_array)
        .ok_or_else(|| bad("contract_init requires non-empty body.features array"))?;
    if raw_features.is_empty() {
        return Err(bad("contract_init requires at least 1 feature in body.features"));
    }

    let ts = now_ms();
    let mut seen_ids = BTreeSet::new();
    let mut normalized = Vec::with_capacity(raw_features.len());
    for (idx, item) in raw_features.iter().enumerate() {
        let mut obj: Map<String, Value> = match item {
            Value::String(s) => {
                let title = s.trim();
                if title.is_empty() {
                    return Err(bad(format!("feature[{idx}] must not be empty")));
                }
                let mut m = Map::new();
                m.insert("id".into(), json!(format!("f{}", idx + 1)));
                m.insert("title".into(), json!(title));
                m
            }
            Value::Object(m) => m.clone(),
            _ => {
                return Err(bad(format!(
                    "feature[{idx}] must be an object or non-empty string"
                )));
            }
        };
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("f{}", idx + 1));
        if !seen_ids.insert(id.clone()) {
            return Err(bad(format!("duplicate contract feature id '{id}'")));
        }
        obj.insert("id".into(), json!(id));
        if obj.get("title").and_then(Value::as_str).map(str::trim).unwrap_or("").is_empty() {
            let fallback_title = obj
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&id)
                .to_string();
            obj.insert("title".into(), json!(fallback_title));
        }
        // Default-FAIL invariant: never allow init to pre-pass any feature.
        obj.insert("passed".into(), json!(false));
        obj.insert("passes".into(), json!(false));
        obj.insert("evidence".into(), json!(""));
        obj.insert("verified_by".into(), Value::Null);
        obj.insert("updated_at_ms".into(), json!(ts));
        normalized.push(Value::Object(obj));
    }

    let active = body.get("active").and_then(Value::as_bool).unwrap_or(true);
    let goal = body
        .get("goal")
        .or_else(|| body.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let doc = json!({
        "schema": "openro.contract.v1",
        "active": active,
        "goal": goal,
        "created_at": now_iso(),
        "created_at_ms": ts,
        "updated_at_ms": ts,
        "features": normalized,
    });

    let abs = safe_rel(project_dir, CONTRACT_REL)?;
    write_atomic_json(&abs, &doc)?;

    let (active, total, passed_count, failing_ids) = summarize_contract(&doc);
    Ok(json!({
        "ok": true,
        "has_contract": true,
        "active": active,
        "path": CONTRACT_REL,
        "total": total,
        "passed_count": passed_count,
        "failing_ids": failing_ids,
        "contract": doc,
    }))
}

/// Read `<project>/.harness/contract.json` + pass/fail summary.
/// Returns `has_contract: false` cleanly (`200 OK`) when absent.
pub fn contract_get(project_dir: &Path) -> Result<Value, Oc> {
    let Some(doc) = read_existing_contract(project_dir)? else {
        return Ok(json!({
            "ok": true,
            "has_contract": false,
            "active": false,
            "total": 0,
            "passed_count": 0,
            "failing_ids": [],
        }));
    };
    let (active, total, passed_count, failing_ids) = summarize_contract(&doc);
    Ok(json!({
        "ok": true,
        "has_contract": true,
        "active": active,
        "path": CONTRACT_REL,
        "total": total,
        "passed_count": passed_count,
        "failing_ids": failing_ids,
        "contract": doc,
    }))
}

/// Update a contract feature's pass state and verification evidence.
/// Strictly rejects any `caller_role` other than `"verifier"` when setting `passed: true`,
/// and requires non-empty `evidence` when setting `passed: true`.
pub fn contract_verify_update(
    project_dir: &Path,
    body: &Value,
    caller_role: &str,
) -> Result<Value, Oc> {
    let id = body
        .get("id")
        .or_else(|| body.get("feature_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad("contract_verify_update requires non-empty body.id"))?
        .to_string();
    let passed = body
        .get("passed")
        .or_else(|| body.get("passes"))
        .and_then(Value::as_bool)
        .ok_or_else(|| bad("contract_verify_update requires boolean body.passed"))?;
    let evidence = body
        .get("evidence")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let role_clean = caller_role.trim();
    if passed && !role_clean.eq_ignore_ascii_case("verifier") {
        return Err(err(
            403,
            format!(
                "only role 'verifier' may set contract feature '{id}' to passed: true (caller role: '{role_clean}')"
            ),
        ));
    }
    if passed && evidence.is_empty() {
        return Err(bad(format!(
            "contract_verify_update requires non-empty evidence when setting '{id}' passed: true"
        )));
    }

    let mut doc = read_existing_contract(project_dir)?
        .ok_or_else(|| err(404, "no .harness/contract.json exists for this project"))?;
    let ts = now_ms();
    let mut updated_feature: Option<Value> = None;
    {
        let features = doc
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| err(500, "contract features is not an array"))?;
        for feat in features.iter_mut() {
            if feat.get("id").and_then(Value::as_str).map(str::trim) == Some(id.as_str()) {
                if let Some(obj) = feat.as_object_mut() {
                    obj.insert("passed".into(), json!(passed));
                    obj.insert("passes".into(), json!(passed));
                    obj.insert("evidence".into(), json!(evidence));
                    obj.insert("verified_by".into(), json!(role_clean));
                    obj.insert("updated_at_ms".into(), json!(ts));
                }
                updated_feature = Some(feat.clone());
                break;
            }
        }
    }
    let Some(feature) = updated_feature else {
        return Err(err(404, format!("unknown contract feature id '{id}'")));
    };

    if let Some(obj) = doc.as_object_mut() {
        obj.insert("updated_at_ms".into(), json!(ts));
    }
    let abs = safe_rel(project_dir, CONTRACT_REL)?;
    write_atomic_json(&abs, &doc)?;

    let (active, total, passed_count, failing_ids) = summarize_contract(&doc);
    Ok(json!({
        "ok": true,
        "has_contract": true,
        "active": active,
        "path": CONTRACT_REL,
        "feature": feature,
        "total": total,
        "passed_count": passed_count,
        "failing_ids": failing_ids,
        "contract": doc,
    }))
}
