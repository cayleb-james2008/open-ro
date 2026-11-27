//! OpenRO Live-Ops Control Plane & Real Player Telemetry (Gap #1).
//!
//! Covers:
//! - Open Cloud v2 DataStores (`standard-data-stores` and `ordered-data-stores`):
//!   list, get, optimistic-concurrency set (`confirm: true` + `expected_revision`
//!   + automatic pre-mutation backup in `.openro/liveops/ds-backups/`), delete.
//! - MessagingService live topic publish (`/cloud/v2/universes/{u}:publishMessage`,
//!   `confirm: true`).
//! - Remote Config control (`liveops/remote-config.json` <= 64 KiB, valid Luau
//!   identifier keys, deterministic `src/shared/LiveConfig.luau` codegen, optional
//!   `push_to_datastore`).
//! - Player Funnel & Economy Telemetry (`POST telemetry/ingest` appending to
//!   `.openro/liveops/telemetry.jsonl` and `GET telemetry/summary`).

use crate::opencloud::{
    bad, err, id_of, now_iso, ok_json, query_param, read_manifest, safe_rel, sha256_hex,
    universe_id, write_json_atomic, Client, Ctx, Oc, Reply, R,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_REMOTE_CONFIG_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_BYTES: usize = 1024;
const MAX_INGEST_BATCH: usize = 1000;
const MAX_TELEMETRY_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DS_BACKUPS: usize = 50;
const DEFAULT_REMOTE_CONFIG_STORE: &str = "OpenRO_RemoteConfig";
const DEFAULT_REMOTE_CONFIG_KEY: &str = "config";
const DEFAULT_TELEMETRY_STORE: &str = "OpenRO_LiveTelemetry";
const MIN_FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_FLUSH_INTERVAL_SECS: u64 = 3600;
const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_SYNC_DAYS: u64 = 90;
const MAX_SYNC_PAGES: usize = 50;
const MAX_SEEN_EVENT_HASHES: usize = 5000;
const TELEMETRY_SYNC_STATE_REL: &str = ".openro/liveops/telemetry-sync-state.json";

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(1);

const LUAU_HARD_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// Route `/api/projects/:id/liveops/*` requests delegated from `opencloud::route`.
pub fn route(
    ctx: &Ctx,
    method: &str,
    root: &Path,
    tail: &[&str],
    query: &str,
    body: &Value,
) -> Option<R<Reply>> {
    match (method, tail) {
        ("GET", ["datastores"]) => Some(datastores_list(ctx, root, query).map(ok_json)),
        ("GET", ["datastores", "entry"]) => Some(datastore_entry_get(ctx, root, query).map(ok_json)),
        ("POST", ["datastores", "entry"]) => Some(datastore_entry_set(ctx, root, body).map(ok_json)),
        ("DELETE", ["datastores", "entry"]) => {
            Some(datastore_entry_delete(ctx, root, query, body).map(ok_json))
        }
        ("POST", ["messaging", "publish"]) => {
            Some(messaging_publish(ctx, root, body).map(ok_json))
        }
        ("GET", ["remote-config"]) => Some(remote_config_get(root).map(ok_json)),
        ("POST", ["remote-config"]) => Some(remote_config_set(ctx, root, body).map(ok_json)),
        ("POST", ["telemetry", "ingest"]) => Some(telemetry_ingest(root, body).map(ok_json)),
        ("POST", ["telemetry", "codegen"]) => Some(telemetry_codegen(root, body).map(ok_json)),
        ("POST", ["telemetry", "sync"]) => Some(telemetry_sync(ctx, root, body).map(ok_json)),
        ("GET", ["telemetry", "summary"]) => Some(telemetry_summary(root, query).map(ok_json)),
        ("GET", ["errors"]) => Some(liveops_errors(root, query).map(ok_json)),
        _ => None,
    }
}

// ------------------------------------------------------------------- helpers

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn ds_kind_segment(kind: Option<&str>) -> R<(&'static str, &'static str)> {
    match kind.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("standard") {
        "standard" | "standard-data-stores" => Ok(("standard", "standard-data-stores")),
        "ordered" | "ordered-data-stores" => Ok(("ordered", "ordered-data-stores")),
        other => bad(format!("Invalid DataStore kind '{other}'. Expected 'standard' or 'ordered'.")),
    }
}

fn validate_ds_token(name: &str, label: &str, max_len: usize) -> R<String> {
    let t = name.trim();
    if t.is_empty() {
        return bad(format!("{label} is required."));
    }
    if t.len() > max_len {
        return bad(format!("{label} exceeds {max_len} characters."));
    }
    if t.chars().any(|c| c.is_control()) || t.contains("..") {
        return bad(format!("{label} contains invalid characters."));
    }
    Ok(t.to_string())
}

fn validate_scope(raw: Option<&str>) -> R<String> {
    let s = raw.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("global");
    validate_ds_token(s, "scope", 64)
}

fn body_opt_str<'a>(body: &'a Value, key: &str, label: &str) -> R<Option<&'a str>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => bad(format!("{label} must be a string.")),
    }
}

fn parse_expected_revision(body: &Value) -> R<Option<String>> {
    match body.get("expected_revision") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return bad("expected_revision cannot be empty when provided.");
            }
            if t.len() > 256 || t.chars().any(|c| c.is_control()) {
                return bad("expected_revision contains invalid characters or exceeds 256 characters.");
            }
            Ok(Some(t.to_string()))
        }
        Some(_) => bad("expected_revision must be a string."),
    }
}

fn str_or_q<'a>(body: &'a Value, key: &str, query: &'a str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| query_param(query, key))
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn extract_revision(v: &Value) -> String {
    v.get("revisionId")
        .or_else(|| v.get("etag"))
        .or_else(|| v.get("revision"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

fn extract_updated_at(v: &Value) -> Option<String> {
    v.get("revisionCreateTime")
        .or_else(|| v.get("updateTime"))
        .or_else(|| v.get("createTime"))
        .or_else(|| v.get("updated_at"))
        .and_then(Value::as_str)
        .map(String::from)
}

fn normalize_ds_entry(store: &str, scope: &str, fallback_key: Option<&str>, item: &Value) -> Value {
    let key = id_of(item, "id")
        .or_else(|| id_of(item, "key"))
        .or_else(|| {
            item.get("path")
                .and_then(Value::as_str)
                .map(|p| last_path_segment(p).to_string())
        })
        .or_else(|| fallback_key.map(String::from))
        .unwrap_or_default();
    let revision = extract_revision(item);
    let updated_at = extract_updated_at(item);
    let value = item.get("value").cloned().unwrap_or(Value::Null);
    json!({
        "store": store,
        "scope": scope,
        "key": key,
        "value": value,
        "revision": revision,
        "updated_at": updated_at,
    })
}

struct DsTarget<'a> {
    universe: &'a str,
    kind: &'a str,
    store: &'a str,
    scope: &'a str,
    key: &'a str,
}

fn parse_backup_order_key(fname: &str) -> (u128, u64) {
    let stem = fname.strip_suffix(".json").unwrap_or(fname);
    if let Some((ms_s, seq_s)) = stem.split_once('-') {
        let ms = ms_s.parse::<u128>().unwrap_or(0);
        let seq = seq_s.parse::<u64>().unwrap_or(0);
        return (ms, seq);
    }
    (0, 0)
}

fn prune_ds_backups(dir: &Path, max_keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(u128, u64, String, PathBuf)> = Vec::new();
    for entry in rd.flatten() {
        let fname = entry.file_name().to_string_lossy().into_owned();
        if !fname.ends_with(".json") || fname.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let (ms, seq) = parse_backup_order_key(&fname);
        files.push((ms, seq, fname, path));
    }
    if files.len() <= max_keep {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
    let drop_n = files.len() - max_keep;
    for (_, _, _, path) in files.into_iter().take(drop_n) {
        let _ = std::fs::remove_file(path);
    }
}

fn write_ds_backup(
    root: &Path,
    action: &str,
    target: &DsTarget<'_>,
    expected_revision: Option<&str>,
    previous: Option<&Value>,
) -> R<String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
    let rel = format!(".openro/liveops/ds-backups/{ms}-{seq}.json");
    let path = safe_rel(root, &rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create backup dir: {e}") })?;
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return bad("ds-backups directory cannot be a symlink.");
        }
    }
    let payload = json!({
        "ts": now_iso(),
        "action": action,
        "universe_id": target.universe,
        "kind": target.kind,
        "store": target.store,
        "scope": target.scope,
        "key": target.key,
        "expected_revision": expected_revision,
        "previous": previous.cloned().unwrap_or(Value::Null),
    });
    write_json_atomic(&path, &payload)?;
    if let Some(parent) = path.parent() {
        prune_ds_backups(parent, MAX_DS_BACKUPS);
    }
    Ok(rel)
}

fn fetch_existing_ds_entry(
    client: &Client,
    entry_path: &str,
    store: &str,
    scope: &str,
    key: &str,
) -> R<Option<(Value, String)>> {
    let (status, bytes) = client.call("GET", entry_path, None, None)?;
    if status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return err(502, client.upstream_msg(status, &bytes));
    }
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Oc { status: 502, msg: "Roblox answered with invalid JSON for DataStore entry.".into() })?;
    let norm = normalize_ds_entry(store, scope, Some(key), &raw);
    let rev = norm["revision"].as_str().unwrap_or("").to_string();
    Ok(Some((norm, rev)))
}

// --------------------------------------------------------------- datastores

fn datastores_list(ctx: &Ctx, root: &Path, query: &str) -> R<Value> {
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let (kind, seg) = ds_kind_segment(query_param(query, "kind").as_deref())?;
    let store = query_param(query, "store").unwrap_or_default();
    let scope = validate_scope(query_param(query, "scope").as_deref())?;
    let prefix = query_param(query, "prefix").unwrap_or_default();
    let cursor = query_param(query, "cursor").unwrap_or_default();
    let limit = query_param(query, "limit")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 100);

    if store.trim().is_empty() {
        let mut path = format!("/cloud/v2/universes/{u}/{seg}?maxPageSize={limit}");
        if !cursor.trim().is_empty() {
            path.push_str(&format!("&pageToken={}", percent_encode(cursor.trim())));
        }
        if !prefix.trim().is_empty() {
            path.push_str(&format!("&prefix={}", percent_encode(prefix.trim())));
        }
        let v = client.json("GET", &path, None, None)?;
        let raw_list = v
            .get("standardDataStores")
            .or_else(|| v.get("orderedDataStores"))
            .or_else(|| v.get("stores"))
            .or_else(|| v.get("datastores"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let stores: Vec<String> = raw_list
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(String::from)
                    .or_else(|| id_of(item, "id"))
                    .or_else(|| id_of(item, "name"))
                    .or_else(|| {
                        item.get("path")
                            .and_then(Value::as_str)
                            .map(|p| last_path_segment(p).to_string())
                    })
            })
            .collect();
        let next_cursor = v.get("nextPageToken").and_then(Value::as_str).filter(|s| !s.is_empty());
        return Ok(json!({
            "ok": true,
            "kind": kind,
            "stores": stores,
            "entries": [],
            "next_cursor": next_cursor,
            "source": "opencloud",
        }));
    }

    let store_name = validate_ds_token(&store, "store", 128)?;
    let mut path = format!(
        "/cloud/v2/universes/{u}/{seg}/{}/scopes/{}/entries?maxPageSize={limit}",
        percent_encode(&store_name),
        percent_encode(&scope)
    );
    if !cursor.trim().is_empty() {
        path.push_str(&format!("&pageToken={}", percent_encode(cursor.trim())));
    }
    if !prefix.trim().is_empty() {
        path.push_str(&format!("&prefix={}", percent_encode(prefix.trim())));
    }
    let v = client.json("GET", &path, None, None)?;
    let raw_entries = v
        .get("standardDataStoreEntries")
        .or_else(|| v.get("orderedDataStoreEntries"))
        .or_else(|| v.get("entries"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let entries: Vec<Value> = raw_entries
        .iter()
        .map(|item| normalize_ds_entry(&store_name, &scope, None, item))
        .filter(|e| {
            let p = prefix.trim();
            p.is_empty()
                || e["key"]
                    .as_str()
                    .is_some_and(|k| k.starts_with(p))
        })
        .collect();
    let next_cursor = v.get("nextPageToken").and_then(Value::as_str).filter(|s| !s.is_empty());
    Ok(json!({
        "ok": true,
        "kind": kind,
        "stores": [store_name],
        "entries": entries,
        "next_cursor": next_cursor,
        "source": "opencloud",
    }))
}

fn datastore_entry_get(ctx: &Ctx, root: &Path, query: &str) -> R<Value> {
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let (kind, seg) = ds_kind_segment(query_param(query, "kind").as_deref())?;
    let store = validate_ds_token(&query_param(query, "store").unwrap_or_default(), "store", 128)?;
    let scope = validate_scope(query_param(query, "scope").as_deref())?;
    let key = validate_ds_token(&query_param(query, "key").unwrap_or_default(), "key", 256)?;
    let path = format!(
        "/cloud/v2/universes/{u}/{seg}/{}/scopes/{}/entries/{}",
        percent_encode(&store),
        percent_encode(&scope),
        percent_encode(&key)
    );
    let Some((entry, _)) = fetch_existing_ds_entry(&client, &path, &store, &scope, &key)? else {
        return err(404, format!("DataStore entry '{key}' was not found in '{store}'."));
    };
    Ok(json!({
        "ok": true,
        "kind": kind,
        "entry": entry,
        "source": "opencloud",
    }))
}

fn datastore_entry_set(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return bad("Live DataStore writes require confirm: true.");
    }
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let kind_raw = body_opt_str(body, "kind", "kind")?;
    let (kind, seg) = ds_kind_segment(kind_raw)?;
    let store = validate_ds_token(
        body_opt_str(body, "store", "store")?.unwrap_or(""),
        "store",
        128,
    )?;
    let scope = validate_scope(body_opt_str(body, "scope", "scope")?)?;
    let key = validate_ds_token(
        body_opt_str(body, "key", "key")?.unwrap_or(""),
        "key",
        256,
    )?;
    let value = body
        .get("value")
        .filter(|v| !v.is_null())
        .ok_or_else(|| Oc { status: 400, msg: "value is required (and cannot be null).".into() })?;
    if kind == "ordered" && value.as_i64().is_none() {
        return bad("OrderedDataStore value must be an integer.");
    }

    let expected_rev = parse_expected_revision(body)?;

    let entry_path = format!(
        "/cloud/v2/universes/{u}/{seg}/{}/scopes/{}/entries/{}",
        percent_encode(&store),
        percent_encode(&scope),
        percent_encode(&key)
    );
    let existing = fetch_existing_ds_entry(&client, &entry_path, &store, &scope, &key)?;

    match (&existing, expected_rev.as_deref()) {
        (None, Some(exp)) => {
            return err(
                409,
                format!("Optimistic concurrency conflict: entry '{key}' does not exist (expected revision '{exp}')."),
            );
        }
        (Some((_, cur_rev)), None) if !cur_rev.is_empty() => {
            return err(
                409,
                format!("Optimistic concurrency conflict: entry '{key}' exists at revision '{cur_rev}'. Provide expected_revision to overwrite."),
            );
        }
        (Some((_, cur_rev)), Some(exp)) if cur_rev != exp => {
            return err(
                409,
                format!("Optimistic concurrency conflict: expected revision '{exp}', current revision is '{cur_rev}'."),
            );
        }
        _ => {}
    }

    let backup_path = write_ds_backup(
        root,
        "set",
        &DsTarget {
            universe: &u,
            kind,
            store: &store,
            scope: &scope,
            key: &key,
        },
        expected_rev.as_deref(),
        existing.as_ref().map(|(e, _)| e),
    )?;

    let payload = json!({ "value": value }).to_string();
    let patch_path = format!("{entry_path}?allowMissing=true");
    let (status, bytes) = client.call(
        "PATCH",
        &patch_path,
        Some("application/json"),
        Some(payload.as_bytes()),
    )?;
    if status == 409 || status == 412 {
        return err(
            409,
            client.scrub("Optimistic concurrency conflict while updating DataStore entry."),
        );
    }
    client.check(status, &bytes)?;
    let updated: Value = if bytes.is_empty() {
        json!({ "value": value })
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "value": value }))
    };
    let entry = normalize_ds_entry(&store, &scope, Some(&key), &updated);
    Ok(json!({
        "ok": true,
        "kind": kind,
        "entry": entry,
        "backup_path": backup_path,
        "source": "opencloud",
    }))
}

fn datastore_entry_delete(ctx: &Ctx, root: &Path, query: &str, body: &Value) -> R<Value> {
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return bad("Live DataStore deletes require confirm: true.");
    }
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let _ = body_opt_str(body, "kind", "kind")?;
    let _ = body_opt_str(body, "store", "store")?;
    let _ = body_opt_str(body, "scope", "scope")?;
    let _ = body_opt_str(body, "key", "key")?;
    let (kind, seg) = ds_kind_segment(str_or_q(body, "kind", query).as_deref())?;
    let store = validate_ds_token(
        &str_or_q(body, "store", query).unwrap_or_default(),
        "store",
        128,
    )?;
    let scope = validate_scope(str_or_q(body, "scope", query).as_deref())?;
    let key = validate_ds_token(
        &str_or_q(body, "key", query).unwrap_or_default(),
        "key",
        256,
    )?;
    let expected_rev = parse_expected_revision(body)?;

    let entry_path = format!(
        "/cloud/v2/universes/{u}/{seg}/{}/scopes/{}/entries/{}",
        percent_encode(&store),
        percent_encode(&scope),
        percent_encode(&key)
    );
    let Some((prev_entry, cur_rev)) =
        fetch_existing_ds_entry(&client, &entry_path, &store, &scope, &key)?
    else {
        return err(404, format!("DataStore entry '{key}' was not found in '{store}'."));
    };
    if let Some(exp) = expected_rev.as_deref() {
        if cur_rev != exp {
            return err(
                409,
                format!("Optimistic concurrency conflict: expected revision '{exp}', current revision is '{cur_rev}'."),
            );
        }
    }

    let backup_path = write_ds_backup(
        root,
        "delete",
        &DsTarget {
            universe: &u,
            kind,
            store: &store,
            scope: &scope,
            key: &key,
        },
        expected_rev.as_deref(),
        Some(&prev_entry),
    )?;

    let (status, bytes) = client.call("DELETE", &entry_path, None, None)?;
    if status == 404 {
        return err(404, format!("DataStore entry '{key}' was not found in '{store}'."));
    }
    client.check(status, &bytes)?;
    Ok(json!({
        "ok": true,
        "deleted": true,
        "kind": kind,
        "store": store,
        "scope": scope,
        "key": key,
        "backup_path": backup_path,
        "source": "opencloud",
    }))
}

// ---------------------------------------------------------- messagingservice

fn messaging_publish(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return bad("MessagingService broadcasts require confirm: true.");
    }
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let topic = body
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc { status: 400, msg: "topic is required.".into() })?;
    if topic.len() > 80 || topic.chars().any(|c| c.is_control()) {
        return bad("topic must be 1..=80 non-control characters.");
    }
    let message_str = match body.get("message") {
        Some(Value::String(s)) => s.clone(),
        Some(v) if !v.is_null() => serde_json::to_string(v)
            .map_err(|e| Oc { status: 400, msg: format!("invalid message payload: {e}") })?,
        _ => return bad("message is required."),
    };
    if message_str.len() > MAX_MESSAGE_BYTES {
        return bad(format!(
            "MessagingService message exceeds {MAX_MESSAGE_BYTES} bytes ({} bytes).",
            message_str.len()
        ));
    }

    let payload = json!({
        "topic": topic,
        "message": message_str,
    })
    .to_string();
    let path = format!("/cloud/v2/universes/{u}:publishMessage");
    let _ = client.json("POST", &path, Some("application/json"), Some(payload.as_bytes()))?;
    Ok(json!({
        "ok": true,
        "topic": topic,
        "published": true,
        "published_at": now_iso(),
    }))
}

// ------------------------------------------------------------- remote config

pub fn is_valid_luau_identifier(key: &str) -> bool {
    if key.is_empty() || key.len() > 128 {
        return false;
    }
    let bytes = key.as_bytes();
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return false;
    }
    if !bytes[1..]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return false;
    }
    !LUAU_HARD_KEYWORDS.contains(&key)
}

fn escape_luau_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_to_luau_literal(v: &Value, indent: usize) -> String {
    match v {
        Value::Null => "nil".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => escape_luau_string(s),
        Value::Array(arr) => {
            if arr.is_empty() {
                return "{}".to_string();
            }
            let pad = "\t".repeat(indent + 1);
            let close_pad = "\t".repeat(indent);
            let mut lines = Vec::with_capacity(arr.len());
            for item in arr {
                lines.push(format!("{pad}{},", json_to_luau_literal(item, indent + 1)));
            }
            format!("{{\n{}\n{close_pad}}}", lines.join("\n"))
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                return "{}".to_string();
            }
            let sorted: BTreeMap<&String, &Value> = obj.iter().collect();
            let pad = "\t".repeat(indent + 1);
            let close_pad = "\t".repeat(indent);
            let mut lines = Vec::with_capacity(sorted.len());
            for (k, val) in sorted {
                if val.is_null() {
                    continue;
                }
                let key_expr = if is_valid_luau_identifier(k) {
                    k.to_string()
                } else {
                    format!("[{}]", escape_luau_string(k))
                };
                lines.push(format!(
                    "{pad}{key_expr} = {},",
                    json_to_luau_literal(val, indent + 1)
                ));
            }
            if lines.is_empty() {
                "{}".to_string()
            } else {
                format!("{{\n{}\n{close_pad}}}", lines.join("\n"))
            }
        }
    }
}

/// Deterministic `--!strict` Luau module generator for `src/shared/LiveConfig.luau`.
pub fn generate_live_config_luau(sorted: &BTreeMap<String, Value>) -> String {
    let mut defaults_obj = Map::new();
    for (k, v) in sorted {
        defaults_obj.insert(k.clone(), v.clone());
    }
    let defaults_literal = json_to_luau_literal(&Value::Object(defaults_obj), 0);
    format!(
        "--!strict
-- Generated deterministically by OpenRO Live-Ops Remote Config. Do not edit by hand.

local DataStoreService = game:GetService(\"DataStoreService\")
local MessagingService = game:GetService(\"MessagingService\")
local RunService = game:GetService(\"RunService\")

local STORE_NAME = \"OpenRO_RemoteConfig\"
local CONFIG_KEY = \"config\"
local TOPIC_NAME = \"LiveConfigUpdated\"

local DEFAULTS: {{ [string]: any }} = {defaults_literal}

local current: {{ [string]: any }} = table.clone(DEFAULTS)
local listeners: {{ (string, any) -> () }} = {{}}

local LiveConfig = {{}}
LiveConfig.DEFAULTS = DEFAULTS

local function applyPatch(patch: any)
\tif type(patch) ~= \"table\" then
\t\treturn
\tend
\tfor key, val in pairs(patch) do
\t\tif type(key) == \"string\" and current[key] ~= val then
\t\t\tcurrent[key] = val
\t\t\tfor _, cb in ipairs(listeners) do
\t\t\t\ttask.spawn(cb, key, val)
\t\t\tend
\t\tend
\tend
end

function LiveConfig.Get(key: string): any
\tlocal val = current[key]
\tif val ~= nil then
\t\treturn val
\tend
\treturn DEFAULTS[key]
end

function LiveConfig.GetAll(): {{ [string]: any }}
\treturn table.clone(current)
end

function LiveConfig.OnChanged(callback: (string, any) -> ()): () -> ()
\ttable.insert(listeners, callback)
\treturn function()
\t\tlocal idx = table.find(listeners, callback)
\t\tif idx then
\t\t\ttable.remove(listeners, idx)
\t\tend
\tend
end

function LiveConfig.Refresh(): boolean
\tif not RunService:IsServer() then
\t\treturn false
\tend
\tlocal ok, result = pcall(function()
\t\tlocal store = DataStoreService:GetDataStore(STORE_NAME)
\t\treturn store:GetAsync(CONFIG_KEY)
\tend)
\tif ok and type(result) == \"table\" then
\t\tapplyPatch(result)
\t\treturn true
\tend
\treturn false
end

function LiveConfig.StartPolling(intervalSeconds: number?)
\tlocal interval = math.max(10, intervalSeconds or 60)
\tif RunService:IsServer() then
\t\tpcall(function()
\t\t\tgame:BindToClose(function() end)
\t\tend)
\tend
\tpcall(function()
\t\tMessagingService:SubscribeAsync(TOPIC_NAME, function(msg)
\t\t\tif type(msg) == \"table\" and type(msg.Data) == \"table\" then
\t\t\t\tapplyPatch(msg.Data)
\t\t\telse
\t\t\t\tLiveConfig.Refresh()
\t\t\tend
\t\tend)
\tend)
\ttask.spawn(function()
\t\twhile true do
\t\t\tLiveConfig.Refresh()
\t\t\ttask.wait(interval)
\t\tend
\tend)
end

return LiveConfig
"
    )
}

fn validate_and_sort_remote_config(raw: &Value) -> R<(BTreeMap<String, Value>, Value, usize)> {
    let Some(obj) = raw.as_object() else {
        return bad("Remote Config must be a JSON object.");
    };
    let mut sorted = BTreeMap::new();
    for (k, v) in obj {
        if !is_valid_luau_identifier(k) {
            return bad(format!(
                "Invalid Remote Config key '{k}': keys must be valid Luau identifiers ([A-Za-z_][A-Za-z0-9_]*, non-keyword)."
            ));
        }
        sorted.insert(k.clone(), v.clone());
    }
    let mut out_map = Map::new();
    for (k, v) in &sorted {
        out_map.insert(k.clone(), v.clone());
    }
    let out_val = Value::Object(out_map);
    let compact_len = serde_json::to_string(&out_val)
        .map(|s| s.len())
        .unwrap_or(usize::MAX);
    if compact_len > MAX_REMOTE_CONFIG_BYTES {
        return bad(format!(
            "Remote Config exceeds the 64 KiB ({MAX_REMOTE_CONFIG_BYTES} bytes) cap ({compact_len} bytes)."
        ));
    }
    Ok((sorted, out_val, compact_len))
}

fn remote_config_get(root: &Path) -> R<Value> {
    let cfg_path = safe_rel(root, "liveops/remote-config.json")?;
    let luau_path = safe_rel(root, "src/shared/LiveConfig.luau")?;
    let config = if cfg_path.is_file() {
        let text = std::fs::read_to_string(&cfg_path)
            .map_err(|e| Oc { status: 500, msg: format!("read remote-config.json: {e}") })?;
        serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}))
    } else {
        json!({})
    };
    let bytes = serde_json::to_string(&config).map(|s| s.len()).unwrap_or(2);
    Ok(json!({
        "ok": true,
        "config": config,
        "entries": config,
        "path": "liveops/remote-config.json",
        "luau_path": "src/shared/LiveConfig.luau",
        "luau_exists": luau_path.is_file(),
        "bytes": bytes,
    }))
}

fn remote_config_set(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let raw_cfg = body
        .get("config")
        .or_else(|| body.get("entries"))
        .ok_or_else(|| Oc {
            status: 400,
            msg: "Send config: { ... } (or entries: { ... }) as a JSON object.".into(),
        })?;
    let (sorted, out_val, bytes) = validate_and_sort_remote_config(raw_cfg)?;

    let cfg_rel = "liveops/remote-config.json";
    let cfg_path = safe_rel(root, cfg_rel)?;
    if let Some(parent) = cfg_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create liveops dir: {e}") })?;
    }
    write_json_atomic(&cfg_path, &out_val)?;

    let generate_luau = body
        .get("generate_luau")
        .or_else(|| body.get("codegen"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let luau_rel = "src/shared/LiveConfig.luau";
    if generate_luau {
        let luau_path = safe_rel(root, luau_rel)?;
        if let Some(parent) = luau_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Oc { status: 500, msg: format!("create src/shared dir: {e}") })?;
        }
        let code = generate_live_config_luau(&sorted);
        let tmp = luau_path.with_extension("tmp-openro");
        std::fs::write(&tmp, code)
            .and_then(|_| std::fs::rename(&tmp, &luau_path))
            .map_err(|e| Oc { status: 500, msg: format!("write LiveConfig.luau: {e}") })?;
    }

    let push_ds = body
        .get("push_to_datastore")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut datastore_pushed = false;
    let mut backup_path: Option<String> = None;
    if push_ds {
        let u = universe_id(&read_manifest(root))?;
        let client = Client::new(ctx)?;
        let store = validate_ds_token(
            body.get("datastore")
                .or_else(|| body.get("datastore_name"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_REMOTE_CONFIG_STORE),
            "datastore",
            128,
        )?;
        let scope = validate_scope(body.get("scope").and_then(Value::as_str))?;
        let key = validate_ds_token(
            body.get("key")
                .or_else(|| body.get("datastore_key"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_REMOTE_CONFIG_KEY),
            "key",
            256,
        )?;
        let entry_path = format!(
            "/cloud/v2/universes/{u}/standard-data-stores/{}/scopes/{}/entries/{}",
            percent_encode(&store),
            percent_encode(&scope),
            percent_encode(&key)
        );
        let existing = fetch_existing_ds_entry(&client, &entry_path, &store, &scope, &key)?;
        backup_path = Some(write_ds_backup(
            root,
            "remote_config_push",
            &DsTarget {
                universe: &u,
                kind: "standard",
                store: &store,
                scope: &scope,
                key: &key,
            },
            None,
            existing.as_ref().map(|(e, _)| e),
        )?);
        let payload = json!({ "value": out_val }).to_string();
        let _ = client.json(
            "PATCH",
            &format!("{entry_path}?allowMissing=true"),
            Some("application/json"),
            Some(payload.as_bytes()),
        )?;
        datastore_pushed = true;
    }

    Ok(json!({
        "ok": true,
        "config": out_val,
        "entries": out_val,
        "path": cfg_rel,
        "luau_path": luau_rel,
        "luau_written": generate_luau,
        "datastore_pushed": datastore_pushed,
        "backup_path": backup_path,
        "bytes": bytes,
    }))
}

// ----------------------------------------------------------------- telemetry

fn telemetry_max_bytes() -> u64 {
    std::env::var("OPENRO_TELEMETRY_MAX_BYTES")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|n| n.clamp(512, MAX_TELEMETRY_FILE_BYTES))
        .unwrap_or(MAX_TELEMETRY_FILE_BYTES)
}

fn telemetry_path(root: &Path) -> R<PathBuf> {
    safe_rel(root, ".openro/liveops/telemetry.jsonl")
}

fn telemetry_sync_state_path(root: &Path) -> R<PathBuf> {
    safe_rel(root, TELEMETRY_SYNC_STATE_REL)
}

fn write_text_atomic(path: &Path, content: &str) -> R<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create parent dir: {e}") })?;
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return bad("Target parent directory cannot be a symlink.");
        }
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return bad("Target file cannot be a symlink.");
    }
    let tmp = path.with_extension("tmp-openro");
    std::fs::write(&tmp, content)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| Oc { status: 500, msg: format!("write {}: {e}", path.display()) })
}

fn normalize_user_id(v: Option<&Value>) -> R<String> {
    match v {
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() || t.len() > 128 || t.chars().any(|c| c.is_control()) {
                return bad("Each telemetry event requires a valid non-empty user_id.");
            }
            Ok(t.to_string())
        }
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => bad("Each telemetry event requires a non-empty user_id."),
    }
}

fn normalize_optional_user_id(v: Option<&Value>, allow_server_default: bool) -> R<String> {
    match v {
        None | Some(Value::Null) if allow_server_default => Ok("server".to_string()),
        _ => normalize_user_id(v),
    }
}

fn opt_clean_str(obj: &Map<String, Value>, key: &str, max_len: usize) -> R<Option<String>> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            if t.chars().any(|c| c.is_control()) {
                return bad(format!("{key} contains invalid control characters."));
            }
            Ok(Some(t.chars().take(max_len).collect()))
        }
        Some(_) => bad(format!("{key} must be a string.")),
    }
}

fn opt_clean_version(obj: &Map<String, Value>) -> R<Option<u64>> {
    match obj
        .get("version_number")
        .or_else(|| obj.get("place_version"))
        .or_else(|| obj.get("version"))
    {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u))
            } else {
                bad("version_number must be a non-negative integer.")
            }
        }
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<u64>()
                .map(Some)
                .map_err(|_| Oc { status: 400, msg: "version_number must be a non-negative integer.".into() })
        }
        Some(_) => bad("version_number must be a non-negative integer."),
    }
}

fn normalize_ts(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() && !s.chars().any(|c| c.is_control()) => {
            s.trim().to_string()
        }
        Some(Value::Number(n)) => n.to_string(),
        _ => now_iso(),
    }
}

fn normalize_telemetry_event(
    obj: &Map<String, Value>,
    default_source: &str,
    force_source: Option<&str>,
    idx_label: Option<usize>,
) -> R<Value> {
    let prefix = idx_label
        .map(|i| format!("events[{i}]."))
        .unwrap_or_default();
    let error_msg = opt_clean_str(obj, "error", 512)?
        .or_else(|| opt_clean_str(obj, "error_message", 512).ok().flatten())
        .or_else(|| {
            if obj.get("kind").and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case("error")) {
                opt_clean_str(obj, "message", 512).ok().flatten()
            } else {
                None
            }
        });
    let raw_kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| {
            if error_msg.is_some() {
                "error".to_string()
            } else {
                String::new()
            }
        });
    if !matches!(raw_kind.as_str(), "onboarding" | "funnel" | "economy" | "custom" | "error") {
        return bad(format!(
            "{prefix}kind must be one of: onboarding, funnel, economy, custom, error."
        ));
    }
    let user_id = normalize_optional_user_id(obj.get("user_id"), raw_kind == "error")?;
    let step = opt_clean_str(obj, "step", 128)?;
    let currency = opt_clean_str(obj, "currency", 64)?;
    let amount = match obj.get("amount") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Some(n),
            _ => return bad(format!("{prefix}amount must be a finite number.")),
        },
    };
    let flow_type = opt_clean_str(obj, "flow_type", 64)?.map(|s| s.to_ascii_lowercase());
    let transaction_type = opt_clean_str(obj, "transaction_type", 128)?;
    let item_sku = opt_clean_str(obj, "item_sku", 128)?;
    let script_path = opt_clean_str(obj, "script_path", 512)?
        .or_else(|| opt_clean_str(obj, "script", 512).ok().flatten());
    let version_number = opt_clean_version(obj)?;
    let source = if let Some(forced) = force_source {
        forced.to_string()
    } else {
        match opt_clean_str(obj, "source", 64)? {
            None => default_source.to_string(),
            Some(s) => {
                let low = s.to_ascii_lowercase();
                if !matches!(low.as_str(), "sandbox" | "live_opencloud") {
                    return bad(format!("{prefix}source must be 'sandbox' or 'live_opencloud'."));
                }
                low
            }
        }
    };
    let ts = normalize_ts(obj.get("ts").or_else(|| obj.get("timestamp")));

    let mut out = Map::new();
    out.insert("kind".to_string(), json!(raw_kind));
    out.insert("user_id".to_string(), json!(user_id));
    if let Some(s) = step {
        out.insert("step".to_string(), json!(s));
    }
    if let Some(c) = currency {
        out.insert("currency".to_string(), json!(c));
    }
    if let Some(a) = amount {
        out.insert("amount".to_string(), json!(a));
    }
    if let Some(ft) = flow_type {
        out.insert("flow_type".to_string(), json!(ft));
    }
    if let Some(tt) = transaction_type {
        out.insert("transaction_type".to_string(), json!(tt));
    }
    if let Some(sku) = item_sku {
        out.insert("item_sku".to_string(), json!(sku));
    }
    if let Some(err_s) = error_msg {
        out.insert("error".to_string(), json!(err_s));
    }
    if let Some(sp) = script_path {
        out.insert("script_path".to_string(), json!(sp));
    }
    if let Some(ver) = version_number {
        out.insert("version_number".to_string(), json!(ver));
    }
    out.insert("source".to_string(), json!(source));
    out.insert("ts".to_string(), json!(ts));
    Ok(Value::Object(out))
}

fn append_telemetry_events(root: &Path, items: &[Value]) -> R<usize> {
    let path = telemetry_path(root)?;
    if items.is_empty() {
        return Ok(count_telemetry_lines(&path));
    }
    let mut serialized_lines = Vec::with_capacity(items.len());
    let mut incoming_bytes: u64 = 0;
    for item in items {
        let line = serde_json::to_string(item)
            .map_err(|e| Oc { status: 500, msg: format!("serialize telemetry event: {e}") })?;
        incoming_bytes = incoming_bytes.saturating_add(line.len() as u64 + 1);
        serialized_lines.push(line);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create telemetry dir: {e}") })?;
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return bad(".openro/liveops cannot be a symlink.");
        }
    }
    rotate_telemetry_if_needed(&path, incoming_bytes, telemetry_max_bytes())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| Oc { status: 500, msg: format!("open telemetry.jsonl: {e}") })?;
    for line in &serialized_lines {
        writeln!(file, "{line}")
            .map_err(|e| Oc { status: 500, msg: format!("write telemetry.jsonl: {e}") })?;
    }
    file.flush()
        .map_err(|e| Oc { status: 500, msg: format!("flush telemetry.jsonl: {e}") })?;
    Ok(count_telemetry_lines(&path))
}

fn rotate_telemetry_if_needed(path: &Path, incoming_bytes: u64, max_bytes: u64) -> R<()> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        return bad("telemetry.jsonl cannot be a symlink.");
    }
    if meta.len().saturating_add(incoming_bytes) <= max_bytes {
        return Ok(());
    }
    let archive = path.with_file_name("telemetry.jsonl.1");
    if let Ok(arch_meta) = std::fs::symlink_metadata(&archive) {
        if arch_meta.file_type().is_symlink() {
            return bad("telemetry.jsonl.1 cannot be a symlink.");
        }
        let _ = std::fs::remove_file(&archive);
    }
    std::fs::rename(path, &archive)
        .map_err(|e| Oc { status: 500, msg: format!("rotate telemetry.jsonl: {e}") })?;
    Ok(())
}

fn telemetry_ingest(root: &Path, body: &Value) -> R<Value> {
    let Some(events) = body.get("events").and_then(Value::as_array) else {
        return bad("Send events: [...] as a JSON array.");
    };
    if events.is_empty() {
        return bad("events array cannot be empty.");
    }
    if events.len() > MAX_INGEST_BATCH {
        return bad(format!("events batch exceeds max {MAX_INGEST_BATCH} entries."));
    }

    let mut normalized = Vec::with_capacity(events.len());
    for (idx, ev) in events.iter().enumerate() {
        let Some(obj) = ev.as_object() else {
            return bad(format!("events[{idx}] must be an object."));
        };
        normalized.push(normalize_telemetry_event(obj, "sandbox", None, Some(idx))?);
    }

    let total_events = append_telemetry_events(root, &normalized)?;
    Ok(json!({
        "ok": true,
        "ingested": normalized.len(),
        "total_events": total_events,
    }))
}

// ------------------------------------------------ telemetry codegen & SSRF

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || o[0] == 0
        }
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00
                || (seg0 & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| is_forbidden_ip(IpAddr::V4(v4)))
        }
    }
}

/// Validate an HTTPS-only external webhook relay URL against SSRF and secret leakage.
pub fn validate_https_relay_url(raw: &str) -> R<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return bad("relay_url is required for webhook_relay mode.");
    }
    if trimmed.len() > 2048 {
        return bad("relay_url exceeds 2048 characters.");
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\\' | '<' | '>'))
    {
        return bad("relay_url contains invalid characters.");
    }
    let rest = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| Oc { status: 400, msg: "relay_url must use https:// (plaintext http and non-HTTPS schemes are rejected).".into() })?;

    let (authority, tail) = match rest.find(['/', '?', '#']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return bad("relay_url must include a host.");
    }
    if authority.contains('@') {
        return bad("relay_url must not contain embedded credentials (@).");
    }

    let (host, port_opt) = if let Some(after_open) = authority.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            return bad("relay_url has an unclosed IPv6 host literal.");
        };
        let ip_str = &after_open[..close_idx];
        let rem = &after_open[close_idx + 1..];
        let port = if rem.is_empty() {
            None
        } else if let Some(p_str) = rem.strip_prefix(':') {
            Some(p_str)
        } else {
            return bad("relay_url has an invalid IPv6 authority.");
        };
        (ip_str, port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h, Some(p))
    } else {
        (authority, None)
    };

    if host.is_empty() {
        return bad("relay_url must include a non-empty host.");
    }
    if let Some(p_str) = port_opt {
        let port: u16 = p_str
            .parse()
            .map_err(|_| Oc { status: 400, msg: "relay_url has an invalid port.".into() })?;
        if port == 0 {
            return bad("relay_url port cannot be 0.");
        }
    }

    let lower_host = host.trim_end_matches('.').to_ascii_lowercase();
    if lower_host.is_empty()
        || lower_host == "localhost"
        || lower_host.ends_with(".localhost")
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
        || lower_host.ends_with(".arpa")
    {
        return bad("relay_url cannot target localhost or internal network hosts (SSRF blocked).");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_forbidden_ip(ip) {
            return bad("relay_url cannot target loopback, private, link-local, or reserved IP addresses (SSRF blocked).");
        }
    } else {
        if !lower_host.contains('.') {
            return bad("relay_url host must be a fully-qualified public domain name.");
        }
        if !lower_host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        {
            return bad("relay_url host contains invalid domain characters.");
        }
    }

    if let Some(q_idx) = tail.find('?') {
        let query = tail[q_idx + 1..].split('#').next().unwrap_or("");
        for pair in query.split('&') {
            let key = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
            if matches!(
                key.as_str(),
                "token" | "api_key" | "apikey" | "secret" | "password" | "x-api-key" | "key"
            ) {
                return bad("relay_url must not embed secrets or API keys in query parameters.");
            }
        }
    }

    Ok(trimmed.to_string())
}

/// Deterministic `--!strict` Luau telemetry collector module (`src/shared/LiveTelemetry.luau`).
pub fn generate_live_telemetry_luau() -> String {
    "--!strict
-- Generated deterministically by OpenRO Live-Ops Telemetry Codegen. Do not edit by hand.

local RunService = game:GetService(\"RunService\")

export type TelemetryEvent = {
	kind: string,
	user_id: string,
	step: string?,
	currency: string?,
	amount: number?,
	flow_type: string?,
	item_sku: string?,
	error: string?,
	script_path: string?,
	version_number: number?,
	ts: string,
}

local MAX_RING_BUFFER = 200
local buffer: { TelemetryEvent } = {}
local droppedCount = 0
local errorHooked = false

local LiveTelemetry = {}
LiveTelemetry.MAX_BUFFER_SIZE = MAX_RING_BUFFER

local function nowIso(): string
	local ok, iso = pcall(function()
		return DateTime.now():ToIsoDate()
	end)
	if ok and type(iso) == \"string\" then
		return iso
	end
	return tostring(os.time())
end

local function playerId(player: Player?): string
	if player ~= nil and typeof(player) == \"Instance\" and player:IsA(\"Player\") then
		return tostring(player.UserId)
	end
	return \"server\"
end

local function pushEvent(ev: TelemetryEvent)
	if #buffer >= MAX_RING_BUFFER then
		table.remove(buffer, 1)
		droppedCount += 1
	end
	table.insert(buffer, ev)
end

function LiveTelemetry.LogOnboarding(player: Player?, step: string)
	if type(step) ~= \"string\" or #step == 0 then
		return
	end
	pushEvent({
		kind = \"onboarding\",
		user_id = playerId(player),
		step = string.sub(step, 1, 128),
		ts = nowIso(),
	})
end

function LiveTelemetry.LogFunnel(player: Player?, step: string)
	if type(step) ~= \"string\" or #step == 0 then
		return
	end
	pushEvent({
		kind = \"funnel\",
		user_id = playerId(player),
		step = string.sub(step, 1, 128),
		ts = nowIso(),
	})
end

function LiveTelemetry.LogEconomy(
	player: Player?,
	currency: string,
	amount: number,
	flowType: string?,
	itemSku: string?
)
	if type(currency) ~= \"string\" or #currency == 0 or type(amount) ~= \"number\" then
		return
	end
	local sku = if type(itemSku) == \"string\" then string.sub(itemSku, 1, 128) else nil
	pushEvent({
		kind = \"economy\",
		user_id = playerId(player),
		currency = string.sub(currency, 1, 64),
		amount = amount,
		flow_type = if type(flowType) == \"string\" then string.lower(string.sub(flowType, 1, 32)) else \"sink\",
		item_sku = sku,
		step = sku,
		ts = nowIso(),
	})
end

function LiveTelemetry.LogCustom(player: Player?, step: string)
	if type(step) ~= \"string\" or #step == 0 then
		return
	end
	pushEvent({
		kind = \"custom\",
		user_id = playerId(player),
		step = string.sub(step, 1, 128),
		ts = nowIso(),
	})
end

local function currentPlaceVersion(): number
	local ok, ver = pcall(function()
		return (game :: any).PlaceVersion
	end)
	if ok and type(ver) == \"number\" then
		return ver
	end
	return 0
end

function LiveTelemetry.LogError(scriptPath: string?, message: string, versionNumber: number?)
	if type(message) ~= \"string\" or #message == 0 then
		return
	end
	pushEvent({
		kind = \"error\",
		user_id = \"server\",
		error = string.sub(message, 1, 512),
		script_path = if type(scriptPath) == \"string\" and #scriptPath > 0
			then string.sub(scriptPath, 1, 256)
			else nil,
		version_number = if type(versionNumber) == \"number\" then versionNumber else currentPlaceVersion(),
		ts = nowIso(),
	})
end

function LiveTelemetry.HookScriptErrors()
	if errorHooked or not RunService:IsServer() then
		return
	end
	errorHooked = true
	pcall(function()
		local ScriptContext = game:GetService(\"ScriptContext\")
		ScriptContext.Error:Connect(function(message: string, _trace: string, scriptInstance: Instance?)
			local path = if scriptInstance ~= nil then scriptInstance:GetFullName() else nil
			LiveTelemetry.LogError(path, message, currentPlaceVersion())
		end)
	end)
end

function LiveTelemetry.PeekCount(): number
	return #buffer
end

function LiveTelemetry.DroppedCount(): number
	return droppedCount
end

function LiveTelemetry.Drain(): { TelemetryEvent }
	if #buffer == 0 then
		return {}
	end
	local drained = table.clone(buffer)
	table.clear(buffer)
	return drained
end

return LiveTelemetry
"
    .to_string()
}

/// Deterministic `--!strict` Luau server flush loop (`src/server/LiveTelemetryFlush.server.luau`).
pub fn generate_live_telemetry_flush_luau(
    mode: &str,
    store_name: &str,
    flush_interval_secs: u64,
    relay_url: Option<&str>,
) -> String {
    let interval = flush_interval_secs.clamp(MIN_FLUSH_INTERVAL_SECS, MAX_FLUSH_INTERVAL_SECS);
    if mode == "webhook_relay" {
        let relay_lit = escape_luau_string(relay_url.unwrap_or(""));
        return format!(
            "--!strict
-- Generated deterministically by OpenRO Live-Ops Telemetry Codegen. Do not edit by hand.
-- Mode: webhook_relay (HTTPS-only endpoint; zero hardcoded secrets)

local HttpService = game:GetService(\"HttpService\")
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")
local RunService = game:GetService(\"RunService\")

local RELAY_URL = {relay_lit}
local FLUSH_INTERVAL_SECS = {interval}

local sharedFolder = (ReplicatedStorage :: any):FindFirstChild(\"Shared\")
local telemetryModule = if sharedFolder ~= nil
	then sharedFolder:FindFirstChild(\"LiveTelemetry\")
	else (ReplicatedStorage :: any):FindFirstChild(\"LiveTelemetry\")
if telemetryModule == nil or not telemetryModule:IsA(\"ModuleScript\") then
	return
end

local LiveTelemetry = require(telemetryModule) :: any
LiveTelemetry.HookScriptErrors()

local flushing = false

local function currentBucketKey(): string
	local ok, utc = pcall(function()
		return os.date(\"!*t\")
	end)
	if ok and type(utc) == \"table\" and utc.year ~= nil then
		return string.format(\"bucket:%04d-%02d-%02d:%02d\", utc.year, utc.month, utc.day, utc.hour)
	end
	local iso = DateTime.now():ToIsoDate()
	return string.format(\"bucket:%s:%s\", string.sub(iso, 1, 10), string.sub(iso, 12, 13))
end

local function flushPending(): boolean
	if flushing or not RunService:IsServer() then
		return false
	end
	local batch = LiveTelemetry.Drain()
	if #batch == 0 then
		return true
	end
	flushing = true
	local bucketKey = currentBucketKey()
	local ok, errMsg = pcall(function()
		local body = HttpService:JSONEncode({{
			bucket = bucketKey,
			events = batch,
			sent_at = DateTime.now():ToIsoDate(),
		}})
		HttpService:RequestAsync({{
			Url = RELAY_URL,
			Method = \"POST\",
			Headers = {{
				[\"Content-Type\"] = \"application/json\",
			}},
			Body = body,
		}})
	end)
	flushing = false
	if not ok then
		warn(\"[LiveTelemetryFlush] Webhook relay flush failed: \" .. tostring(errMsg))
		return false
	end
	return true
end

game:BindToClose(function()
	flushPending()
end)

task.spawn(function()
	while true do
		task.wait(FLUSH_INTERVAL_SECS)
		flushPending()
	end
end)
"
        );
    }

    let store_lit = escape_luau_string(store_name);
    format!(
        "--!strict
-- Generated deterministically by OpenRO Live-Ops Telemetry Codegen. Do not edit by hand.
-- Mode: opencloud_datastore (zero hardcoded secrets; uses Roblox DataStoreService)

local DataStoreService = game:GetService(\"DataStoreService\")
local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")
local RunService = game:GetService(\"RunService\")

local STORE_NAME = {store_lit}
local FLUSH_INTERVAL_SECS = {interval}
local MAX_BUCKET_EVENTS = 500

local sharedFolder = (ReplicatedStorage :: any):FindFirstChild(\"Shared\")
local telemetryModule = if sharedFolder ~= nil
	then sharedFolder:FindFirstChild(\"LiveTelemetry\")
	else (ReplicatedStorage :: any):FindFirstChild(\"LiveTelemetry\")
if telemetryModule == nil or not telemetryModule:IsA(\"ModuleScript\") then
	return
end

local LiveTelemetry = require(telemetryModule) :: any
LiveTelemetry.HookScriptErrors()

local telemetryStore = DataStoreService:GetDataStore(STORE_NAME)
local flushing = false

local function currentBucketKey(): string
	local ok, utc = pcall(function()
		return os.date(\"!*t\")
	end)
	if ok and type(utc) == \"table\" and utc.year ~= nil then
		return string.format(\"bucket:%04d-%02d-%02d:%02d\", utc.year, utc.month, utc.day, utc.hour)
	end
	local iso = DateTime.now():ToIsoDate()
	return string.format(\"bucket:%s:%s\", string.sub(iso, 1, 10), string.sub(iso, 12, 13))
end

local function flushPending(): boolean
	if flushing or not RunService:IsServer() then
		return false
	end
	local batch = LiveTelemetry.Drain()
	if #batch == 0 then
		return true
	end
	flushing = true
	local bucketKey = currentBucketKey()
	local ok, errMsg = pcall(function()
		telemetryStore:UpdateAsync(bucketKey, function(existing: any)
			local record: {{ [string]: any }} = if type(existing) == \"table\" then existing else {{}}
			local list: {{ any }} = if type(record.events) == \"table\" then record.events else {{}}
			for _, ev in ipairs(batch) do
				if #list >= MAX_BUCKET_EVENTS then
					table.remove(list, 1)
				end
				table.insert(list, ev)
			end
			record.bucket = bucketKey
			record.events = list
			record.updated_at = DateTime.now():ToIsoDate()
			return record
		end)
	end)
	flushing = false
	if not ok then
		warn(\"[LiveTelemetryFlush] DataStore flush failed: \" .. tostring(errMsg))
		return false
	end
	return true
end

game:BindToClose(function()
	flushPending()
end)

task.spawn(function()
	while true do
		task.wait(FLUSH_INTERVAL_SECS)
		flushPending()
	end
end)
"
    )
}

fn parse_flush_interval(body: &Value) -> R<u64> {
    match body.get("flush_interval_secs") {
        None | Some(Value::Null) => Ok(DEFAULT_FLUSH_INTERVAL_SECS),
        Some(v) => {
            let secs = if let Some(u) = v.as_u64() {
                u
            } else if let Some(f) = v.as_f64() {
                if !f.is_finite() || f < 0.0 || f.fract() != 0.0 {
                    return bad("flush_interval_secs must be an integer between 60 and 3600.");
                }
                f as u64
            } else {
                return bad("flush_interval_secs must be an integer between 60 and 3600.");
            };
            if !(MIN_FLUSH_INTERVAL_SECS..=MAX_FLUSH_INTERVAL_SECS).contains(&secs) {
                return bad(format!(
                    "flush_interval_secs must be between {MIN_FLUSH_INTERVAL_SECS} and {MAX_FLUSH_INTERVAL_SECS} seconds."
                ));
            }
            Ok(secs)
        }
    }
}

fn telemetry_codegen(root: &Path, body: &Value) -> R<Value> {
    let mode = match body_opt_str(body, "mode", "mode")? {
        None => "opencloud_datastore".to_string(),
        Some(s) => {
            let low = s.trim().to_ascii_lowercase();
            if !matches!(low.as_str(), "opencloud_datastore" | "webhook_relay") {
                return bad("mode must be 'opencloud_datastore' or 'webhook_relay'.");
            }
            low
        }
    };

    let store_name = match body_opt_str(body, "store_name", "store_name")? {
        None => DEFAULT_TELEMETRY_STORE.to_string(),
        Some(s) => validate_ds_token(s, "store_name", 128)?,
    };
    let flush_interval_secs = parse_flush_interval(body)?;

    let relay_url = match body_opt_str(body, "relay_url", "relay_url")? {
        Some(raw) if !raw.trim().is_empty() => Some(validate_https_relay_url(raw)?),
        _ => None,
    };
    if mode == "webhook_relay" && relay_url.is_none() {
        return bad("relay_url is required when mode is 'webhook_relay'.");
    }

    let shared_rel = "src/shared/LiveTelemetry.luau";
    let flush_rel = "src/server/LiveTelemetryFlush.server.luau";
    let shared_path = safe_rel(root, shared_rel)?;
    let flush_path = safe_rel(root, flush_rel)?;

    let shared_code = generate_live_telemetry_luau();
    let flush_code = generate_live_telemetry_flush_luau(
        &mode,
        &store_name,
        flush_interval_secs,
        relay_url.as_deref(),
    );

    write_text_atomic(&shared_path, &shared_code)?;
    write_text_atomic(&flush_path, &flush_code)?;

    let warnings: Vec<String> = Vec::new();
    Ok(json!({
        "ok": true,
        "mode": mode,
        "store_name": store_name,
        "flush_interval_secs": flush_interval_secs,
        "relay_url": relay_url,
        "files": [shared_rel, flush_rel],
        "warnings": warnings,
    }))
}

// ------------------------------------------------ telemetry sync & errors

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TelemetrySourceFilter {
    All,
    LiveOpenCloud,
    Sandbox,
}

impl TelemetrySourceFilter {
    fn as_str(self) -> &'static str {
        match self {
            TelemetrySourceFilter::All => "all",
            TelemetrySourceFilter::LiveOpenCloud => "live_opencloud",
            TelemetrySourceFilter::Sandbox => "sandbox",
        }
    }

    fn matches(self, ev: &Value) -> bool {
        match self {
            TelemetrySourceFilter::All => true,
            TelemetrySourceFilter::LiveOpenCloud => {
                ev.get("source").and_then(Value::as_str) == Some("live_opencloud")
            }
            TelemetrySourceFilter::Sandbox => {
                ev.get("source").and_then(Value::as_str).unwrap_or("sandbox") == "sandbox"
            }
        }
    }
}

fn parse_source_filter(query: &str) -> R<TelemetrySourceFilter> {
    match query_param(query, "source")
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        None => Ok(TelemetrySourceFilter::All),
        Some(s) if s == "all" => Ok(TelemetrySourceFilter::All),
        Some(s) if s == "live_opencloud" => Ok(TelemetrySourceFilter::LiveOpenCloud),
        Some(s) if s == "sandbox" => Ok(TelemetrySourceFilter::Sandbox),
        Some(other) => bad(format!(
            "Invalid source filter '{other}'. Expected 'all', 'live_opencloud', or 'sandbox'."
        )),
    }
}

struct SyncStateStore {
    store_name: String,
    source: String,
    last_sync_at: Option<String>,
    last_imported_count: usize,
    total_imported_count: usize,
    shard_revisions: BTreeMap<String, String>,
    seen_event_hashes: Vec<String>,
}

impl SyncStateStore {
    fn empty(store_name: &str, source: &str) -> Self {
        SyncStateStore {
            store_name: store_name.to_string(),
            source: source.to_string(),
            last_sync_at: None,
            last_imported_count: 0,
            total_imported_count: 0,
            shard_revisions: BTreeMap::new(),
            seen_event_hashes: Vec::new(),
        }
    }

    fn from_value(v: &Value, default_store: &str, default_source: &str) -> Self {
        let store_name = v
            .get("store_name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default_store)
            .to_string();
        let source = v
            .get("source")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default_source)
            .to_string();
        let last_sync_at = v.get("last_sync_at").and_then(Value::as_str).map(String::from);
        let last_imported_count = v
            .get("last_imported_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let total_imported_count = v
            .get("total_imported_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let mut shard_revisions = BTreeMap::new();
        if let Some(obj) = v.get("shard_revisions").and_then(Value::as_object) {
            for (k, rv) in obj {
                if let Some(s) = rv.as_str() {
                    shard_revisions.insert(k.clone(), s.to_string());
                }
            }
        }
        let seen_event_hashes = v
            .get("seen_event_hashes")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        SyncStateStore {
            store_name,
            source,
            last_sync_at,
            last_imported_count,
            total_imported_count,
            shard_revisions,
            seen_event_hashes,
        }
    }

    fn to_public_json(&self) -> Value {
        json!({
            "store_name": self.store_name,
            "source": self.source,
            "last_sync_at": self.last_sync_at,
            "last_imported_count": self.last_imported_count,
            "total_imported_count": self.total_imported_count,
            "shards_tracked": self.shard_revisions.len(),
            "shard_revisions": self.shard_revisions,
        })
    }

    fn to_disk_json(&self) -> Value {
        json!({
            "store_name": self.store_name,
            "source": self.source,
            "last_sync_at": self.last_sync_at,
            "last_imported_count": self.last_imported_count,
            "total_imported_count": self.total_imported_count,
            "shards_tracked": self.shard_revisions.len(),
            "shard_revisions": self.shard_revisions,
            "seen_event_hashes": self.seen_event_hashes,
        })
    }
}

fn load_sync_state(root: &Path, default_store: &str, default_source: &str) -> R<Option<SyncStateStore>> {
    let path = telemetry_sync_state_path(root)?;
    if !path.exists() {
        return Ok(None);
    }
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return bad("telemetry-sync-state.json cannot be a symlink.");
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Oc { status: 500, msg: format!("read telemetry-sync-state.json: {e}") })?;
    let val: Value = serde_json::from_str(&text)
        .map_err(|e| Oc { status: 500, msg: format!("parse telemetry-sync-state.json: {e}") })?;
    Ok(Some(SyncStateStore::from_value(&val, default_store, default_source)))
}

fn save_sync_state(root: &Path, state: &SyncStateStore) -> R<()> {
    let path = telemetry_sync_state_path(root)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create telemetry state dir: {e}") })?;
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return bad(".openro/liveops cannot be a symlink.");
        }
    }
    write_json_atomic(&path, &state.to_disk_json())
}

fn parse_sync_days(body: &Value) -> R<u64> {
    match body.get("days") {
        None | Some(Value::Null) => Ok(7),
        Some(v) => {
            let days = if let Some(u) = v.as_u64() {
                u
            } else if let Some(f) = v.as_f64() {
                if !f.is_finite() || f < 0.0 || f.fract() != 0.0 {
                    return bad("days must be an integer between 1 and 90.");
                }
                f as u64
            } else {
                return bad("days must be an integer between 1 and 90.");
            };
            if !(1..=MAX_SYNC_DAYS).contains(&days) {
                return bad(format!("days must be between 1 and {MAX_SYNC_DAYS}."));
            }
            Ok(days)
        }
    }
}

fn extract_events_from_shard_value(val: &Value) -> Vec<Map<String, Value>> {
    let mut out = Vec::new();
    let arr_opt = val
        .as_array()
        .or_else(|| val.get("events").and_then(Value::as_array))
        .or_else(|| val.get("items").and_then(Value::as_array))
        .or_else(|| val.get("records").and_then(Value::as_array));
    if let Some(arr) = arr_opt {
        for item in arr {
            if let Some(obj) = item.as_object() {
                out.push(obj.clone());
            }
        }
        return out;
    }
    if let Some(obj) = val.as_object() {
        if obj.contains_key("kind") || obj.contains_key("error") || obj.contains_key("message") {
            out.push(obj.clone());
        }
    }
    out
}

fn event_dedupe_hash(shard_key: &str, idx: usize, raw_obj: &Map<String, Value>) -> String {
    if let Some(eid) = raw_obj
        .get("event_id")
        .or_else(|| raw_obj.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return sha256_hex(format!("id|{shard_key}|{eid}").as_bytes());
    }
    let sorted: BTreeMap<&String, &Value> = raw_obj.iter().collect();
    let encoded = serde_json::to_string(&sorted).unwrap_or_default();
    sha256_hex(format!("ev|{shard_key}|{idx}|{encoded}").as_bytes())
}

fn push_seen_hash(seen_vec: &mut Vec<String>, seen_set: &mut HashSet<String>, hash: String) {
    if seen_set.insert(hash.clone()) {
        seen_vec.push(hash);
        if seen_vec.len() > MAX_SEEN_EVENT_HASHES {
            let overflow = seen_vec.len() - MAX_SEEN_EVENT_HASHES;
            for old in seen_vec.drain(..overflow) {
                seen_set.remove(&old);
            }
        }
    }
}

fn telemetry_sync(ctx: &Ctx, root: &Path, body: &Value) -> R<Value> {
    let u = universe_id(&read_manifest(root))?;
    let client = Client::new(ctx)?;
    let source = match body_opt_str(body, "source", "source")? {
        None => "opencloud_datastore".to_string(),
        Some(s) => {
            let low = s.trim().to_ascii_lowercase();
            if !matches!(low.as_str(), "opencloud_datastore" | "creator_analytics") {
                return bad("source must be 'opencloud_datastore' or 'creator_analytics'.");
            }
            low
        }
    };
    let store_name = match body_opt_str(body, "store_name", "store_name")? {
        None => DEFAULT_TELEMETRY_STORE.to_string(),
        Some(s) => validate_ds_token(s, "store_name", 128)?,
    };
    let days = parse_sync_days(body)?;

    let mut state = load_sync_state(root, &store_name, &source)?
        .unwrap_or_else(|| SyncStateStore::empty(&store_name, &source));
    state.store_name = store_name.clone();
    state.source = source.clone();

    let mut seen_set: HashSet<String> = state.seen_event_hashes.iter().cloned().collect();
    let mut new_events: Vec<Value> = Vec::new();

    if source == "opencloud_datastore" {
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_SYNC_PAGES {
            let mut path = format!(
                "/cloud/v2/universes/{u}/standard-data-stores/{}/scopes/global/entries?maxPageSize=100",
                percent_encode(&store_name)
            );
            if let Some(tok) = page_token.as_deref().filter(|s| !s.is_empty()) {
                path.push_str(&format!("&pageToken={}", percent_encode(tok)));
            }
            let (status, bytes) = client.call("GET", &path, None, None)?;
            if status == 404 {
                break;
            }
            client.check(status, &bytes)?;
            let v: Value = if bytes.is_empty() {
                json!({})
            } else {
                serde_json::from_slice(&bytes)
                    .map_err(|_| Oc { status: 502, msg: "Roblox answered with invalid JSON for DataStore entries.".into() })?
            };
            let raw_entries = v
                .get("standardDataStoreEntries")
                .or_else(|| v.get("entries"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for item in &raw_entries {
                let key = id_of(item, "id")
                    .or_else(|| id_of(item, "key"))
                    .or_else(|| {
                        item.get("path")
                            .and_then(Value::as_str)
                            .map(|p| last_path_segment(p).to_string())
                    })
                    .unwrap_or_default();
                if key.is_empty() {
                    continue;
                }
                let mut rev = extract_revision(item);
                if !rev.is_empty() && state.shard_revisions.get(&key) == Some(&rev) {
                    continue;
                }
                let shard_value = if item.get("value").is_some_and(|val| !val.is_null()) && !rev.is_empty() {
                    item.get("value").cloned().unwrap_or(Value::Null)
                } else {
                    let entry_path = format!(
                        "/cloud/v2/universes/{u}/standard-data-stores/{}/scopes/global/entries/{}",
                        percent_encode(&store_name),
                        percent_encode(&key)
                    );
                    let Some((norm, fetched_rev)) =
                        fetch_existing_ds_entry(&client, &entry_path, &store_name, "global", &key)?
                    else {
                        continue;
                    };
                    rev = fetched_rev;
                    if !rev.is_empty() && state.shard_revisions.get(&key) == Some(&rev) {
                        continue;
                    }
                    norm.get("value").cloned().unwrap_or(Value::Null)
                };

                let raw_events = extract_events_from_shard_value(&shard_value);
                for (idx, raw_obj) in raw_events.iter().enumerate() {
                    let canonical =
                        normalize_telemetry_event(raw_obj, "live_opencloud", Some("live_opencloud"), None)?;
                    let h = event_dedupe_hash(&key, idx, raw_obj);
                    if seen_set.contains(&h) {
                        continue;
                    }
                    push_seen_hash(&mut state.seen_event_hashes, &mut seen_set, h);
                    new_events.push(canonical);
                }
                if !rev.is_empty() {
                    state.shard_revisions.insert(key, rev);
                }
            }

            let next = v
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            if next.is_none() {
                break;
            }
            page_token = next;
        }
    } else {
        let path = format!("/cloud/v2/universes/{u}/analytics/events?days={days}");
        let (status, bytes) = client.call("GET", &path, None, None)?;
        if status == 404 || status == 501 {
            return err(
                502,
                "Creator Analytics telemetry export is unavailable for this universe; use source='opencloud_datastore' with the OpenRO telemetry collector.",
            );
        }
        client.check(status, &bytes)?;
        let v: Value = if bytes.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|_| Oc { status: 502, msg: "Roblox answered with invalid JSON for Creator Analytics.".into() })?
        };
        let raw_events = extract_events_from_shard_value(&v);
        for (idx, raw_obj) in raw_events.iter().enumerate() {
            let canonical =
                normalize_telemetry_event(raw_obj, "live_opencloud", Some("live_opencloud"), None)?;
            let h = event_dedupe_hash("creator_analytics", idx, raw_obj);
            if seen_set.contains(&h) {
                continue;
            }
            push_seen_hash(&mut state.seen_event_hashes, &mut seen_set, h);
            new_events.push(canonical);
        }
    }

    let imported_count = new_events.len();
    if !new_events.is_empty() {
        let _ = append_telemetry_events(root, &new_events)?;
    }

    state.last_sync_at = Some(now_iso());
    state.last_imported_count = imported_count;
    state.total_imported_count = state.total_imported_count.saturating_add(imported_count);
    save_sync_state(root, &state)?;

    let public_state = state.to_public_json();
    let all_events = read_telemetry_events(root, TelemetrySourceFilter::All)?;
    let (all_errors, _) = aggregate_live_errors(&all_events);
    let top_errors: Vec<Value> = all_errors.into_iter().take(10).collect();
    let summary = compute_telemetry_summary_from_events(
        &all_events,
        TelemetrySourceFilter::All,
        Some(&public_state),
    );

    Ok(json!({
        "ok": true,
        "imported_count": imported_count,
        "sync_state": public_state,
        "summary": summary,
        "telemetry_summary": summary,
        "top_errors": top_errors,
    }))
}

fn read_telemetry_events(root: &Path, filter: TelemetrySourceFilter) -> R<Vec<Value>> {
    let path = telemetry_path(root)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return bad("telemetry.jsonl cannot be a symlink.");
        }
        if meta.len() > telemetry_max_bytes() {
            return err(413, "telemetry.jsonl exceeds maximum size.");
        }
    }
    let file = std::fs::File::open(&path)
        .map_err(|e| Oc { status: 500, msg: format!("read telemetry.jsonl: {e}") })?;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if !ev.is_object() || !filter.matches(&ev) {
            continue;
        }
        events.push(ev);
    }
    Ok(events)
}

fn aggregate_live_errors(events: &[Value]) -> (Vec<Value>, usize) {
    struct ErrAgg {
        fingerprint: String,
        script_path: Option<String>,
        message: String,
        count: usize,
        first_seen: String,
        last_seen: String,
    }

    let mut groups: BTreeMap<String, ErrAgg> = BTreeMap::new();
    let mut total_error_events: usize = 0;

    for ev in events {
        let kind = ev.get("kind").and_then(Value::as_str).unwrap_or("");
        let err_msg = ev
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| {
                if kind == "error" {
                    ev.get("message").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(raw_msg) = err_msg else {
            continue;
        };
        total_error_events += 1;
        let norm_msg = raw_msg.split_whitespace().collect::<Vec<_>>().join(" ");
        let script_path = ev
            .get("script_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let fp = ev
            .get("fingerprint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| {
                let sp = script_path.as_deref().unwrap_or("");
                let key = format!("{sp}|{norm_msg}");
                format!("err_{}", &sha256_hex(key.as_bytes())[..12])
            });
        let ts = ev
            .get("ts")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();

        groups
            .entry(fp.clone())
            .and_modify(|agg| {
                agg.count += 1;
                if !ts.is_empty() {
                    if agg.first_seen.is_empty() || ts < agg.first_seen {
                        agg.first_seen = ts.clone();
                    }
                    if agg.last_seen.is_empty() || ts > agg.last_seen {
                        agg.last_seen = ts.clone();
                    }
                }
            })
            .or_insert_with(|| ErrAgg {
                fingerprint: fp,
                script_path,
                message: norm_msg,
                count: 1,
                first_seen: ts.clone(),
                last_seen: ts,
            });
    }

    let mut list: Vec<ErrAgg> = groups.into_values().collect();
    list.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });

    let serialized: Vec<Value> = list
        .into_iter()
        .take(100)
        .map(|agg| {
            let mut obj = Map::new();
            obj.insert("fingerprint".to_string(), json!(agg.fingerprint));
            if let Some(sp) = agg.script_path {
                obj.insert("script_path".to_string(), json!(sp));
            }
            obj.insert("message".to_string(), json!(agg.message));
            obj.insert("count".to_string(), json!(agg.count));
            obj.insert("first_seen".to_string(), json!(agg.first_seen));
            obj.insert("last_seen".to_string(), json!(agg.last_seen));
            Value::Object(obj)
        })
        .collect();
    (serialized, total_error_events)
}

fn liveops_errors(root: &Path, query: &str) -> R<Value> {
    let filter = parse_source_filter(query)?;
    let events = read_telemetry_events(root, filter)?;
    let (errors, total_error_events) = aggregate_live_errors(&events);
    let sync_state = load_sync_state(root, DEFAULT_TELEMETRY_STORE, "opencloud_datastore")?
        .map(|s| s.to_public_json());
    Ok(json!({
        "ok": true,
        "source": filter.as_str(),
        "total_error_events": total_error_events,
        "errors": errors,
        "sync_state": sync_state,
    }))
}

fn count_telemetry_lines(path: &Path) -> usize {
    let Ok(f) = std::fs::File::open(path) else {
        return 0;
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && serde_json::from_str::<Value>(t).is_ok_and(|v| v.is_object())
        })
        .count()
}

fn round1(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 10.0).round() / 10.0
}

fn round2(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 100.0).round() / 100.0
}

fn compute_telemetry_summary_from_events(
    events: &[Value],
    filter: TelemetrySourceFilter,
    sync_state: Option<&Value>,
) -> Value {
    if events.is_empty() {
        return json!({
            "ok": true,
            "source": filter.as_str(),
            "dau_sample": 0,
            "total_events": 0,
            "error_count": 0,
            "top_errors": [],
            "funnel_steps": [],
            "arpdau_robux": 0.0,
            "top_sinks": [],
            "sync_state": sync_state,
            "alerts": [{
                "severity": "info",
                "title": "No player telemetry yet",
                "detail": "Run a headless sandbox test or ingest events to populate funnel and economy telemetry."
            }],
        });
    }

    let mut dau_users: BTreeSet<String> = BTreeSet::new();
    let mut step_order: Vec<String> = Vec::new();
    let mut step_users: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sink_totals: BTreeMap<String, (f64, u64)> = BTreeMap::new();
    let mut robux_revenue: f64 = 0.0;
    let total_events: usize = events.len();

    for ev in events {
        let user_id = ev
            .get("user_id")
            .and_then(|u| u.as_str().map(String::from).or_else(|| u.as_u64().map(|n| n.to_string())))
            .unwrap_or_default();
        let kind = ev.get("kind").and_then(Value::as_str).unwrap_or("");
        if !user_id.is_empty() && kind != "error" {
            dau_users.insert(user_id.clone());
        }
        if matches!(kind, "onboarding" | "funnel") {
            if let Some(step) = ev.get("step").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
                let entry = step_users.entry(step.to_string()).or_insert_with(|| {
                    step_order.push(step.to_string());
                    BTreeSet::new()
                });
                if !user_id.is_empty() {
                    entry.insert(user_id.clone());
                }
            }
        } else if kind == "economy" {
            let currency = ev
                .get("currency")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let amount = ev
                .get("amount")
                .and_then(Value::as_f64)
                .filter(|a| a.is_finite())
                .unwrap_or(0.0);
            let flow = ev.get("flow_type").and_then(Value::as_str).unwrap_or("");
            if currency.eq_ignore_ascii_case("robux") || currency.eq_ignore_ascii_case("rbx") {
                robux_revenue += amount.abs();
            }
            if flow != "source" {
                let label = ev
                    .get("step")
                    .or_else(|| ev.get("item_sku"))
                    .or_else(|| ev.get("transaction_type"))
                    .or_else(|| ev.get("currency"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("economy_sink")
                    .to_string();
                let rec = sink_totals.entry(label).or_insert((0.0, 0));
                rec.0 += amount.abs();
                rec.1 += 1;
            }
        }
    }

    let (all_errors, error_count) = aggregate_live_errors(events);
    let top_errors: Vec<Value> = all_errors.into_iter().take(10).collect();

    let dau_sample = dau_users.len();
    let first_users = step_order
        .first()
        .and_then(|s| step_users.get(s))
        .map(BTreeSet::len)
        .unwrap_or(0);

    let mut funnel_steps = Vec::with_capacity(step_order.len());
    let mut alerts: Vec<Value> = Vec::new();
    let mut prev_users: Option<usize> = None;

    for step in &step_order {
        let users = step_users.get(step).map(BTreeSet::len).unwrap_or(0);
        let pct = if first_users > 0 {
            round1((users as f64 / first_users as f64) * 100.0)
        } else {
            0.0
        };
        let drop_off_pct = match prev_users {
            Some(prev) if prev > 0 && prev >= users => {
                round1(((prev - users) as f64 / prev as f64) * 100.0)
            }
            _ => 0.0,
        };
        if drop_off_pct >= 50.0 {
            alerts.push(json!({
                "severity": "warning",
                "title": format!("High funnel drop-off at '{step}'"),
                "detail": format!("{drop_off_pct:.1}% of players dropped before reaching '{step}'."),
            }));
        }
        funnel_steps.push(json!({
            "step": step,
            "users": users,
            "pct": pct,
            "drop_off_pct": drop_off_pct,
        }));
        prev_users = Some(users);
    }

    let mut sinks_vec: Vec<(String, f64, u64)> = sink_totals
        .into_iter()
        .map(|(label, (amt, count))| (label, amt, count))
        .collect();
    sinks_vec.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let top_sinks: Vec<Value> = sinks_vec
        .into_iter()
        .take(10)
        .map(|(label, amount, count)| {
            json!({
                "label": label,
                "amount": round2(amount),
                "events": count,
            })
        })
        .collect();

    let arpdau_robux = if dau_sample > 0 {
        round2(robux_revenue / dau_sample as f64)
    } else {
        0.0
    };

    if dau_sample > 0 && funnel_steps.is_empty() {
        alerts.push(json!({
            "severity": "info",
            "title": "No onboarding or funnel steps recorded",
            "detail": "Instrument AnalyticsService:LogOnboardingFunnelStepEvent to track tutorial drop-off.",
        }));
    }

    json!({
        "ok": true,
        "source": filter.as_str(),
        "dau_sample": dau_sample,
        "total_events": total_events,
        "error_count": error_count,
        "top_errors": top_errors,
        "funnel_steps": funnel_steps,
        "arpdau_robux": arpdau_robux,
        "top_sinks": top_sinks,
        "sync_state": sync_state,
        "alerts": alerts,
    })
}

fn telemetry_summary(root: &Path, query: &str) -> R<Value> {
    let filter = parse_source_filter(query)?;
    let events = read_telemetry_events(root, filter)?;
    let sync_state = load_sync_state(root, DEFAULT_TELEMETRY_STORE, "opencloud_datastore")?
        .map(|s| s.to_public_json());
    Ok(compute_telemetry_summary_from_events(
        &events,
        filter,
        sync_state.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_identifier_validation_rejects_keywords_and_bad_chars() {
        assert!(is_valid_luau_identifier("DropRateMultiplier"));
        assert!(is_valid_luau_identifier("_v2_flag"));
        assert!(!is_valid_luau_identifier(""));
        assert!(!is_valid_luau_identifier("2fast"));
        assert!(!is_valid_luau_identifier("drop-rate"));
        assert!(!is_valid_luau_identifier("local"));
        assert!(!is_valid_luau_identifier("function"));
        assert!(!is_valid_luau_identifier("a b"));
    }

    #[test]
    fn remote_config_codegen_is_deterministic() {
        let raw = json!({
            "ZetaFlag": true,
            "AlphaMultiplier": 1.5,
            "BannerText": "Double XP\nWeekend"
        });
        let (sorted, _, _) = validate_and_sort_remote_config(&raw).unwrap();
        let a = generate_live_config_luau(&sorted);
        let b = generate_live_config_luau(&sorted);
        assert_eq!(a, b);
        let idx_alpha = a.find("AlphaMultiplier").unwrap();
        let idx_banner = a.find("BannerText").unwrap();
        let idx_zeta = a.find("ZetaFlag").unwrap();
        assert!(idx_alpha < idx_banner && idx_banner < idx_zeta);
        assert!(a.contains("--!strict"));
        assert!(a.contains("DataStoreService:GetDataStore"));
        assert!(a.contains("MessagingService:SubscribeAsync"));
        let rep = crate::luau_check::analyze_script("ReplicatedStorage.Shared.LiveConfig", &a);
        assert!(
            rep.diagnostics.is_empty(),
            "LiveConfig.luau diagnostics: {:?}",
            rep.diagnostics
        );
    }

    #[test]
    fn telemetry_codegen_passes_strict_luau_checks_and_ssrf_url_validation() {
        assert!(validate_https_relay_url("https://telemetry.example.com/v1/ingest").is_ok());
        assert!(validate_https_relay_url("http://telemetry.example.com/ingest").is_err());
        assert!(validate_https_relay_url("https://127.0.0.1:8443/hook").is_err());
        assert!(validate_https_relay_url("https://localhost/hook").is_err());
        assert!(validate_https_relay_url("https://169.254.169.254/latest/meta-data").is_err());
        assert!(validate_https_relay_url("https://10.0.0.5/hook").is_err());
        assert!(validate_https_relay_url("https://[::1]/hook").is_err());
        assert!(validate_https_relay_url("https://user:pass@telemetry.example.com/hook").is_err());
        assert!(validate_https_relay_url("https://telemetry.example.com/hook?api_key=abc").is_err());

        let shared = generate_live_telemetry_luau();
        assert!(shared.starts_with("--!strict"));
        assert!(shared.contains("MAX_RING_BUFFER"));
        let rep_shared = crate::luau_check::analyze_script("ReplicatedStorage.Shared.LiveTelemetry", &shared);
        assert!(rep_shared.diagnostics.is_empty(), "LiveTelemetry diagnostics: {:?}", rep_shared.diagnostics);

        let flush_ds = generate_live_telemetry_flush_luau("opencloud_datastore", "OpenRO_LiveTelemetry", 120, None);
        assert!(flush_ds.starts_with("--!strict"));
        assert!(flush_ds.contains("BindToClose"));
        assert!(flush_ds.contains("bucket:%04d-%02d-%02d:%02d"));
        let rep_ds = crate::luau_check::analyze_script("ServerScriptService.Server.LiveTelemetryFlush", &flush_ds);
        assert!(rep_ds.diagnostics.is_empty(), "LiveTelemetryFlush DS diagnostics: {:?}", rep_ds.diagnostics);

        let flush_wh = generate_live_telemetry_flush_luau(
            "webhook_relay",
            "OpenRO_LiveTelemetry",
            90,
            Some("https://telemetry.example.com/ingest"),
        );
        assert!(flush_wh.starts_with("--!strict"));
        assert!(flush_wh.contains("BindToClose"));
        assert!(flush_wh.contains("https://telemetry.example.com/ingest"));
        let rep_wh = crate::luau_check::analyze_script("ServerScriptService.Server.LiveTelemetryFlush", &flush_wh);
        assert!(rep_wh.diagnostics.is_empty(), "LiveTelemetryFlush WH diagnostics: {:?}", rep_wh.diagnostics);
    }
}
