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
    bad, err, id_of, now_iso, ok_json, query_param, read_manifest, safe_rel, universe_id,
    write_json_atomic, Client, Ctx, Oc, Reply, R,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_REMOTE_CONFIG_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_BYTES: usize = 1024;
const MAX_INGEST_BATCH: usize = 1000;
const MAX_TELEMETRY_FILE_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_REMOTE_CONFIG_STORE: &str = "OpenRO_RemoteConfig";
const DEFAULT_REMOTE_CONFIG_KEY: &str = "config";

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
        ("GET", ["telemetry", "summary"]) => Some(telemetry_summary(root).map(ok_json)),
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
    if t.contains('\0') || t.contains('\r') || t.contains('\n') || t.contains("..") {
        return bad(format!("{label} contains invalid characters."));
    }
    Ok(t.to_string())
}

fn validate_scope(raw: Option<&str>) -> R<String> {
    let s = raw.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("global");
    validate_ds_token(s, "scope", 64)
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
    let (kind, seg) = ds_kind_segment(body.get("kind").and_then(Value::as_str))?;
    let store = validate_ds_token(
        body.get("store").and_then(Value::as_str).unwrap_or(""),
        "store",
        128,
    )?;
    let scope = validate_scope(body.get("scope").and_then(Value::as_str))?;
    let key = validate_ds_token(
        body.get("key").and_then(Value::as_str).unwrap_or(""),
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

    let expected_rev = body
        .get("expected_revision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let entry_path = format!(
        "/cloud/v2/universes/{u}/{seg}/{}/scopes/{}/entries/{}",
        percent_encode(&store),
        percent_encode(&scope),
        percent_encode(&key)
    );
    let existing = fetch_existing_ds_entry(&client, &entry_path, &store, &scope, &key)?;

    match (&existing, expected_rev) {
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
        expected_rev,
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
    let expected_rev = body
        .get("expected_revision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

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
    if let Some(exp) = expected_rev {
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
        expected_rev,
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

fn telemetry_path(root: &Path) -> R<PathBuf> {
    safe_rel(root, ".openro/liveops/telemetry.jsonl")
}

fn normalize_user_id(v: Option<&Value>) -> R<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() && s.trim().len() <= 128 => {
            Ok(s.trim().to_string())
        }
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => bad("Each telemetry event requires a non-empty user_id."),
    }
}

fn normalize_ts(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => now_iso(),
    }
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
        let kind = obj
            .get("kind")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(kind.as_str(), "onboarding" | "funnel" | "economy" | "custom") {
            return bad(format!(
                "events[{idx}].kind must be one of: onboarding, funnel, economy, custom."
            ));
        }
        let user_id = normalize_user_id(obj.get("user_id"))?;
        let step = obj
            .get("step")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(128).collect::<String>());
        let currency = obj
            .get("currency")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(64).collect::<String>());
        let amount = match obj.get("amount") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_f64() {
                Some(n) if n.is_finite() => Some(n),
                _ => return bad(format!("events[{idx}].amount must be a finite number.")),
            },
        };
        let flow_type = obj
            .get("flow_type")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let transaction_type = obj
            .get("transaction_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(128).collect::<String>());
        let item_sku = obj
            .get("item_sku")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(128).collect::<String>());
        let ts = normalize_ts(obj.get("ts"));

        normalized.push(json!({
            "kind": kind,
            "user_id": user_id,
            "step": step,
            "currency": currency,
            "amount": amount,
            "flow_type": flow_type,
            "transaction_type": transaction_type,
            "item_sku": item_sku,
            "ts": ts,
        }));
    }

    let path = telemetry_path(root)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Oc { status: 500, msg: format!("create telemetry dir: {e}") })?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return bad("telemetry.jsonl cannot be a symlink.");
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| Oc { status: 500, msg: format!("open telemetry.jsonl: {e}") })?;
    for item in &normalized {
        let line = serde_json::to_string(item)
            .map_err(|e| Oc { status: 500, msg: format!("serialize telemetry event: {e}") })?;
        writeln!(file, "{line}")
            .map_err(|e| Oc { status: 500, msg: format!("write telemetry.jsonl: {e}") })?;
    }
    file.flush()
        .map_err(|e| Oc { status: 500, msg: format!("flush telemetry.jsonl: {e}") })?;

    let total_events = count_telemetry_lines(&path);
    Ok(json!({
        "ok": true,
        "ingested": normalized.len(),
        "total_events": total_events,
    }))
}

fn count_telemetry_lines(path: &Path) -> usize {
    let Ok(f) = std::fs::File::open(path) else {
        return 0;
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .count()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn telemetry_summary(root: &Path) -> R<Value> {
    let path = telemetry_path(root)?;
    if !path.exists() {
        return Ok(json!({
            "ok": true,
            "dau_sample": 0,
            "total_events": 0,
            "funnel_steps": [],
            "arpdau_robux": 0.0,
            "top_sinks": [],
            "alerts": [{
                "severity": "info",
                "title": "No player telemetry yet",
                "detail": "Run a headless sandbox test or ingest events to populate funnel and economy telemetry."
            }],
        }));
    }
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return bad("telemetry.jsonl cannot be a symlink.");
        }
        if meta.len() > MAX_TELEMETRY_FILE_BYTES {
            return err(413, "telemetry.jsonl exceeds maximum size.");
        }
    }

    let file = std::fs::File::open(&path)
        .map_err(|e| Oc { status: 500, msg: format!("read telemetry.jsonl: {e}") })?;
    let mut dau_users: BTreeSet<String> = BTreeSet::new();
    let mut step_order: Vec<String> = Vec::new();
    let mut step_users: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sink_totals: BTreeMap<String, (f64, u64)> = BTreeMap::new();
    let mut robux_revenue: f64 = 0.0;
    let mut total_events: usize = 0;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        total_events += 1;
        let user_id = ev
            .get("user_id")
            .and_then(|u| u.as_str().map(String::from).or_else(|| u.as_u64().map(|n| n.to_string())))
            .unwrap_or_default();
        if !user_id.is_empty() {
            dau_users.insert(user_id.clone());
        }
        let kind = ev.get("kind").and_then(Value::as_str).unwrap_or("");
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
            let amount = ev.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
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
        .map(|(label, amount, events)| {
            json!({
                "label": label,
                "amount": round2(amount),
                "events": events,
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

    Ok(json!({
        "ok": true,
        "dau_sample": dau_sample,
        "total_events": total_events,
        "funnel_steps": funnel_steps,
        "arpdau_robux": arpdau_robux,
        "top_sinks": top_sinks,
        "alerts": alerts,
    }))
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
}
