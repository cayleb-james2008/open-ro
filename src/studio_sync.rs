//! Bidirectional project <-> Studio sync over StudioMCP and Companion Plugin.
//!
//! Push sync diffs inside Studio over `execute_luau` and updates
//! `<project>/.openro/sync-baseline.json` atomically. Pull sync fetches a
//! cursor-paginated Studio snapshot, performs a 3-way comparison against local
//! files and the persisted baseline, and applies selected or non-conflicting
//! scripts, instances (`scene/workspace.model.json` / `scene/models/`), and
//! `Lighting` updates atomically.

use crate::mcp::{luau_long_string, McpClient};
use crate::place::PlaceBackend;
use crate::project_api::{with_studio, Reply};
use crate::projects::{self, Flat, ProjectRef};
use crate::scene;
use crate::server::Shared;
use crate::snapshots;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static BASELINE_TMP_SEQ: AtomicU64 = AtomicU64::new(1);
const MAX_BASELINE_BYTES: u64 = 16 * 1024 * 1024;
pub const SYNC_BASELINE_REL: &str = ".openro/sync-baseline.json";

/// Largest request payload (JSON characters) per `execute_luau` call.
const CHUNK_CHARS: usize = 45_000;
/// Maximum items compared in one `execute_luau` call so replies stay far under 100k chars.
const MAX_COMPARE_ITEMS: usize = 120;
/// Largest piece of one script's source sent in a single call.
const SOURCE_PIECE: usize = 30_000;
const MARKER: &str = "OPENRO_V011:";

const STRUCTURAL: [&str; 10] = [
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPlayer",
    "StarterPack",
    "ReplicatedFirst",
    "Lighting",
    "SoundService",
    "Teams",
];

fn service_rank(path: &str) -> usize {
    let svc = path.split('/').next().unwrap_or("");
    if svc == "Workspace" {
        return 100;
    }
    STRUCTURAL.iter().position(|s| *s == svc).unwrap_or(50)
}

/// Same hash the Luau side computes: h = (h * 31 + byte) mod 2^32.
pub fn hash31(s: &str) -> u64 {
    let mut h: u64 = 0;
    for b in s.as_bytes() {
        h = (h * 31 + *b as u64) % 4_294_967_296;
    }
    h
}

const LUAU_COMMON: &str = r##"
local HttpService = game:GetService("HttpService")
local P = HttpService:JSONDecode(PAYLOAD)
local function resolve(path)
	local cur = nil
	for seg in string.gmatch(path, "[^/]+") do
		if cur == nil then
			local ok, svc = pcall(function() return game:GetService(seg) end)
			if not ok or not svc then return nil end
			cur = svc
		else
			cur = cur:FindFirstChild(seg)
			if not cur then return nil end
		end
	end
	return cur
end
local function conv(cur, v)
	local t = typeof(cur)
	if t == "EnumItem" then
		if type(v) == "string" then return cur.EnumType[v] end
		for _, it in ipairs(cur.EnumType:GetEnumItems()) do
			if it.Value == v then return it end
		end
		return nil
	elseif t == "Vector3" then
		local a = v.Vector3 or v
		return Vector3.new(a[1], a[2], a[3])
	elseif t == "Vector2" then
		local a = v.Vector2 or v
		return Vector2.new(a[1], a[2])
	elseif t == "Color3" then
		local a = v.Color3 or v
		return Color3.new(a[1], a[2], a[3])
	elseif t == "UDim2" then
		local a = v.UDim2 or v
		return UDim2.new(a[1][1], a[1][2], a[2][1], a[2][2])
	elseif t == "UDim" then
		local a = v.UDim or v
		return UDim.new(a[1], a[2])
	elseif t == "NumberRange" then
		local a = v.NumberRange or v
		return NumberRange.new(a[1], a[2])
	elseif t == "BrickColor" then
		return BrickColor.new(v.BrickColor or v)
	elseif t == "CFrame" then
		local a = v.CFrame or v
		if a.position then
			local p, o = a.position, a.orientation
			return CFrame.new(p[1], p[2], p[3], o[1][1], o[1][2], o[1][3], o[2][1], o[2][2], o[2][3], o[3][1], o[3][2], o[3][3])
		end
		return CFrame.new(a[1], a[2], a[3])
	end
	return v
end
local function same(a, b)
	local t = typeof(a)
	if t ~= typeof(b) then return false end
	if t == "number" then return math.abs(a - b) < 1e-4 end
	if t == "Vector3" or t == "Vector2" then return (a - b).Magnitude < 1e-4 end
	if t == "Color3" then return math.abs(a.R - b.R) + math.abs(a.G - b.G) + math.abs(a.B - b.B) < 1e-3 end
	if t == "UDim2" then
		return math.abs(a.X.Scale - b.X.Scale) + math.abs(a.X.Offset - b.X.Offset) + math.abs(a.Y.Scale - b.Y.Scale) + math.abs(a.Y.Offset - b.Y.Offset) < 1e-3
	end
	if t == "UDim" then return math.abs(a.Scale - b.Scale) + math.abs(a.Offset - b.Offset) < 1e-3 end
	return a == b
end
"##;

const LUAU_COMPARE: &str = r##"
local function hash(s)
	local h = 0
	for i = 1, #s do
		h = (h * 31 + string.byte(s, i)) % 4294967296
	end
	return h
end
local out, extra, okCount = {}, {}, 0
for _, it in ipairs(P.items) do
	local inst = resolve(it.p)
	if not inst then
		table.insert(out, { p = it.p, s = "missing" })
	elseif not it.svc and inst.ClassName ~= it.c then
		table.insert(out, { p = it.p, s = "class", a = inst.ClassName })
	else
		local bad = {}
		if it.h then
			local okS, src = pcall(function() return inst.Source end)
			if not okS or #src ~= it.n or hash(src) ~= it.h then table.insert(bad, "Source") end
		end
		if it.pr then
			for name, v in pairs(it.pr) do
				if name == "Attributes" then
					for k, av in pairs(v) do
						if inst:GetAttribute(k) ~= av then table.insert(bad, "Attributes." .. k) end
					end
				else
					local okP, cur = pcall(function() return inst[name] end)
					if not okP then
						table.insert(bad, name .. "?")
					else
						local okC, want = pcall(conv, cur, v)
						if not okC or want == nil or not same(cur, want) then table.insert(bad, name) end
					end
				end
			end
		end
		if #bad > 0 then
			table.insert(out, { p = it.p, s = "props", b = bad })
		else
			okCount += 1
		end
		if it.k then
			local known = {}
			for _, n in ipairs(it.k) do known[n] = true end
			for _, ch in ipairs(inst:GetChildren()) do
				if not known[ch.Name] and #extra < 200 then table.insert(extra, it.p .. "/" .. ch.Name) end
			end
		end
	end
end
return "OPENRO_V011:" .. HttpService:JSONEncode({ r = out, x = extra, ok = okCount })
"##;

const LUAU_APPLY: &str = r##"
local function applyProps(inst, props)
	if not props then return end
	for name, v in pairs(props) do
		if name == "Attributes" then
			for k, av in pairs(v) do inst:SetAttribute(k, av) end
		else
			local cur = inst[name]
			local want = conv(cur, v)
			assert(want ~= nil, "bad value for " .. name)
			inst[name] = want
		end
	end
end
local applied, errors = 0, {}
for _, op in ipairs(P.ops) do
	local ok, err = pcall(function()
		if op.k == "delete" then
			local inst = resolve(op.p)
			if inst then inst:Destroy() end
			return
		end
		if op.k == "create" or op.k == "replace" then
			local parentPath, name = string.match(op.p, "^(.*)/([^/]+)$")
			assert(parentPath, "cannot create a service: " .. op.p)
			local parent = resolve(parentPath)
			assert(parent, "parent is missing: " .. parentPath)
			if op.k == "replace" then
				local old = parent:FindFirstChild(name)
				if old then old:Destroy() end
			end
			local inst = Instance.new(op.c)
			inst.Name = name
			applyProps(inst, op.pr)
			if op.s then inst.Source = op.s end
			inst.Parent = parent
		else
			local inst = resolve(op.p)
			assert(inst, "missing: " .. op.p)
			applyProps(inst, op.pr)
			if op.s ~= nil then
				if op.a then inst.Source = inst.Source .. op.s else inst.Source = op.s end
			end
		end
	end)
	if ok then applied += 1 else table.insert(errors, { p = op.p, m = tostring(err) }) end
end
pcall(function() game:GetService("ChangeHistoryService"):SetWaypoint("OpenRO sync") end)
return "OPENRO_V011:" .. HttpService:JSONEncode({ n = applied, e = errors })
"##;

pub fn build_code(op: &str, payload: &Value, body: &str) -> String {
    format!("-- openro:v011:{op}\nlocal PAYLOAD = {}\n{LUAU_COMMON}\n{body}", luau_long_string(&payload.to_string()))
}

/// Find the `OPENRO_V011:` JSON in whatever shape the bridge wrapped the script's return value.
pub fn extract_result(v: &Value) -> Option<Value> {
    fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) => out.push(s),
            Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
            Value::Object(o) => o.values().for_each(|x| strings(x, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    strings(v, &mut all);
    for s in all {
        if let Some(i) = s.find(MARKER) {
            let tail = s[i + MARKER.len()..].trim();
            if let Ok(parsed) = serde_json::from_str::<Value>(tail) {
                return Some(parsed);
            }
            // Trailing text after the JSON (a second output line): parse the first value only.
            let mut de = serde_json::Deserializer::from_str(tail).into_iter::<Value>();
            if let Some(Ok(parsed)) = de.next() {
                return Some(parsed);
            }
        }
    }
    None
}

fn run_luau(mcp: &mut McpClient, op: &str, payload: &Value, body: &str) -> Result<Value, String> {
    let code = build_code(op, payload, body);
    let prev = mcp.datamodel_type().to_string();
    mcp.set_datamodel_type("Edit");
    let res = mcp.execute_luau(&code);
    mcp.set_datamodel_type(&prev);
    let val = res.map_err(|e| format!("Studio refused the {op} step: {e}"))?;
    extract_result(&val).ok_or_else(|| format!("Studio answered the {op} step without a result (is the Studio place open and in Edit mode?)"))
}

#[derive(Clone, Debug)]
pub struct Change {
    pub op: &'static str,
    pub path: String,
    pub class: String,
    pub reason: String,
}

pub struct Diff {
    pub changes: Vec<Change>,
    pub unchanged: usize,
    pub extra: Vec<String>,
    pub chunks: usize,
}

fn item_json(f: &Flat, kids: Option<&Vec<String>>) -> Value {
    let mut o = json!({ "p": f.path, "c": f.class });
    if f.service {
        o["svc"] = json!(1);
    }
    if let Some(src) = &f.source {
        o["h"] = json!(hash31(src));
        o["n"] = json!(src.len());
    }
    if !f.props.is_empty() {
        o["pr"] = json!(f.props);
    }
    if let Some(k) = kids {
        o["k"] = json!(k);
    }
    o
}

/// Ordered project instances: structural services first, Workspace last.
pub fn ordered(flat: Vec<Flat>) -> Vec<Flat> {
    let mut v = flat;
    v.sort_by_key(|f| service_rank(&f.path)); // stable: pre-order inside each service
    v
}

fn expected_kids(flat: &[Flat]) -> std::collections::HashMap<String, Vec<String>> {
    let mut m: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for f in flat {
        if let Some((parent, name)) = f.path.rsplit_once('/') {
            m.entry(parent.to_string()).or_default().push(name.to_string());
        }
    }
    m
}

pub fn diff(mcp: &mut McpClient, flat: &[Flat]) -> Result<Diff, String> {
    let kids = expected_kids(flat);
    let empty: Vec<String> = Vec::new();
    let mut chunks_sent = 0usize;
    let mut results: std::collections::HashMap<String, (String, Value)> = std::collections::HashMap::new();
    let mut unchanged = 0usize;
    let mut extra: Vec<String> = Vec::new();
    let mut batch: Vec<Value> = Vec::new();
    let mut size = 0usize;
    let mut flush = |batch: &mut Vec<Value>, mcp: &mut McpClient| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let payload = json!({ "items": batch });
        let r = run_luau(mcp, "sync_compare", &payload, LUAU_COMPARE)?;
        chunks_sent += 1;
        unchanged += r["ok"].as_u64().unwrap_or(0) as usize;
        for e in r["r"].as_array().cloned().unwrap_or_default() {
            let p = e["p"].as_str().unwrap_or("").to_string();
            results.insert(p, (e["s"].as_str().unwrap_or("").to_string(), e));
        }
        for x in r["x"].as_array().cloned().unwrap_or_default() {
            if let Some(s) = x.as_str() {
                extra.push(s.to_string());
            }
        }
        batch.clear();
        Ok(())
    };
    for f in flat {
        let k = if !f.ignore_unknown { Some(kids.get(&f.path).unwrap_or(&empty)) } else { None };
        let item = item_json(f, k);
        let len = item.to_string().len() + 1;
        if (size + len > CHUNK_CHARS || batch.len() >= MAX_COMPARE_ITEMS) && !batch.is_empty() {
            flush(&mut batch, mcp)?;
            size = 0;
        }
        size += len;
        batch.push(item);
    }
    flush(&mut batch, mcp)?;

    // Build changes in project order; anything under a recreated instance is created too.
    let mut changes: Vec<Change> = Vec::new();
    let mut recreated: Vec<String> = Vec::new();
    for f in flat {
        let under_recreated = recreated.iter().any(|r| f.path.starts_with(&format!("{r}/")));
        let res = results.get(&f.path);
        let (op, reason): (&'static str, String) = match (under_recreated, res) {
            (true, _) => ("create", "its parent is being replaced".into()),
            (false, Some((s, _))) if s == "missing" => {
                if f.service {
                    return Err(format!("Studio has no service named '{}'", f.path));
                }
                ("create", "not in Studio".into())
            }
            (false, Some((s, e))) if s == "class" => {
                recreated.push(f.path.clone());
                ("replace", format!("Studio has a {} where the project has a {}", e["a"].as_str().unwrap_or("?"), f.class))
            }
            (false, Some((_, e))) => {
                let bad: Vec<String> = e["b"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let named: Vec<String> = bad.iter().map(|b| if b == "Source" { "source".to_string() } else { b.clone() }).collect();
                ("update", format!("differs: {}", named.join(", ")))
            }
            (false, None) => continue,
        };
        changes.push(Change { op, path: f.path.clone(), class: f.class.clone(), reason });
    }
    Ok(Diff { changes, unchanged, extra, chunks: chunks_sent })
}

fn pieces(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if cur.len() + c.len_utf8() > SOURCE_PIECE {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

pub struct Applied {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub errors: Vec<Value>,
    pub chunks: usize,
}

pub fn apply(mcp: &mut McpClient, flat: &[Flat], d: &Diff, prune: bool) -> Result<Applied, String> {
    let by_path: std::collections::HashMap<&str, &Flat> = flat.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut ops: Vec<Value> = Vec::new();
    let (mut created, mut updated) = (0usize, 0usize);
    for c in &d.changes {
        let f = by_path[c.path.as_str()];
        let kind = if c.op == "update" { "update" } else { c.op };
        if kind == "update" {
            updated += 1;
        } else {
            created += 1;
        }
        let mut op = json!({ "k": kind, "p": f.path, "c": f.class });
        if !f.props.is_empty() {
            op["pr"] = json!(f.props);
        }
        match &f.source {
            Some(src) => {
                let ps = pieces(src);
                op["s"] = json!(ps[0]);
                ops.push(op);
                for extra in ps.iter().skip(1) {
                    ops.push(json!({ "k": "update", "p": f.path, "c": f.class, "s": extra, "a": true }));
                }
            }
            None => ops.push(op),
        }
    }
    let mut deleted = 0usize;
    if prune {
        for p in &d.extra {
            ops.push(json!({ "k": "delete", "p": p }));
            deleted += 1;
        }
    }
    let mut errors: Vec<Value> = Vec::new();
    let mut chunks = 0usize;
    let mut batch: Vec<Value> = Vec::new();
    let mut size = 0usize;
    let mut send = |batch: &mut Vec<Value>, mcp: &mut McpClient| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let r = run_luau(mcp, "sync_apply", &json!({ "ops": batch }), LUAU_APPLY)?;
        chunks += 1;
        for e in r["e"].as_array().cloned().unwrap_or_default() {
            errors.push(e);
        }
        batch.clear();
        Ok(())
    };
    for op in ops {
        let len = op.to_string().len() + 1;
        if size + len > CHUNK_CHARS && !batch.is_empty() {
            send(&mut batch, mcp)?;
            size = 0;
        }
        size += len;
        batch.push(op);
    }
    send(&mut batch, mcp)?;
    Ok(Applied { created, updated, deleted, errors, chunks })
}

fn changes_json(d: &Diff) -> Vec<Value> {
    d.changes.iter().map(|c| json!({ "op": c.op, "path": c.path, "class": c.class, "reason": c.reason })).collect()
}

fn summary_json(d: &Diff) -> Value {
    let count = |op: &str| d.changes.iter().filter(|c| c.op == op).count();
    json!({
        "create": count("create"),
        "update": count("update") + count("replace"),
        "unchanged": d.unchanged,
        "extra": d.extra.len(),
    })
}

/// Diff (and, unless `dry_run`, apply then re-diff) the project against the connected Studio.
pub fn run(shared: &Shared, r: &ProjectRef, prune: bool, dry_run: bool) -> Result<Reply, Reply> {
    let manifest = projects::manifest(&r.dir).map_err(Reply::bad)?;
    let rojo = manifest["rojoProject"].as_str().unwrap_or(projects::DEFAULT_ROJO_FILE).to_string();
    let tree = projects::load_tree(&r.dir, &rojo).map_err(Reply::bad)?;
    let flat = ordered(tree.flatten());
    with_studio(shared, |mcp| {
        let d = diff(mcp, &flat)?;
        let mut out = json!({
            "summary": summary_json(&d),
            "changes": changes_json(&d),
            "extra": d.extra,
            "chunks": d.chunks,
            "dry_run": dry_run,
            "skipped": tree.skipped,
        });
        if dry_run {
            return Ok(Reply::ok(out));
        }
        let a = apply(mcp, &flat, &d, prune)?;
        let after = diff(mcp, &flat)?;
        let remaining = after.changes.len() + if prune { after.extra.len() } else { 0 };
        if a.errors.is_empty() {
            let sid = format!("push-{}", now_ms());
            let _ = update_baseline_from_project(&r.dir, &rojo, Some(&sid));
        }
        out["applied"] = json!({ "created": a.created, "updated": a.updated, "deleted": a.deleted });
        out["errors"] = json!(a.errors);
        out["chunks"] = json!(d.chunks + a.chunks + after.chunks);
        out["verify"] = json!({ "remaining": remaining, "extra": after.extra.len() });
        Ok(Reply::ok(out))
    })
}

// ---------------------------------------------------------------------------
// Bidirectional 3-Way Studio Pull & Baseline Persistence
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullStrategy {
    TakeStudio,
    KeepLocal,
    MergeNonConflicting,
}

impl PullStrategy {
    pub fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("merge_non_conflicting") => Ok(PullStrategy::MergeNonConflicting),
            Some("take_studio") => Ok(PullStrategy::TakeStudio),
            Some("keep_local") => Ok(PullStrategy::KeepLocal),
            Some(other) => Err(format!(
                "invalid pull strategy '{other}': expected take_studio, keep_local, or merge_non_conflicting"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StudioPullChange {
    pub path: String,
    pub kind: String,
    pub status: String,
    pub local_hash: Option<u64>,
    pub studio_hash: Option<u64>,
    pub baseline_hash: Option<u64>,
    pub hunks: Vec<Value>,
}

impl StudioPullChange {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "kind": self.kind,
            "status": self.status,
            "local_hash": self.local_hash,
            "studio_hash": self.studio_hash,
            "baseline_hash": self.baseline_hash,
            "hunks": self.hunks,
        })
    }
}

#[derive(Clone, Debug)]
pub struct StudioPullResponse {
    pub ok: bool,
    pub snapshot_id: String,
    pub changes: Vec<StudioPullChange>,
    pub applied_count: usize,
    pub conflict_count: usize,
}

impl StudioPullResponse {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "snapshot_id": self.snapshot_id,
            "changes": self.changes.iter().map(StudioPullChange::to_json).collect::<Vec<_>>(),
            "applied_count": self.applied_count,
            "conflict_count": self.conflict_count,
        })
    }
}

#[derive(Debug)]
pub enum PullError {
    BadRequest(String),
    ConflictConfirmRequired(StudioPullResponse),
    Internal(String),
}

impl PullError {
    pub fn into_reply(self) -> Reply {
        match self {
            PullError::BadRequest(msg) => Reply::bad(msg),
            PullError::ConflictConfirmRequired(resp) => {
                let mut v = resp.to_json();
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "error".into(),
                        json!("Conflicts detected; pass confirm_overwrite: true to overwrite local edits with Studio changes."),
                    );
                }
                Reply::json("409 Conflict", v)
            }
            PullError::Internal(msg) => Reply::err("500 Internal Server Error", msg),
        }
    }

    pub fn message(&self) -> String {
        match self {
            PullError::BadRequest(m) | PullError::Internal(m) => m.clone(),
            PullError::ConflictConfirmRequired(_) => {
                "Conflicts detected; pass confirm_overwrite: true to overwrite local edits."
                    .to_string()
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SyncBaseline {
    pub baseline_id: Option<String>,
    pub scripts: BTreeMap<String, String>,
    pub script_hashes: BTreeMap<String, u64>,
    pub instance_hashes: BTreeMap<String, u64>,
    pub lighting_hashes: BTreeMap<String, u64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn write_atomic_str_safe(dir: &Path, rel: &str, content: &str) -> Result<(), String> {
    if rel.contains('\0') || rel.contains('\\') {
        return Err(format!("invalid project relative path '{rel}'"));
    }
    let full = projects::safe_join(dir, rel)?;
    if let Ok(meta) = std::fs::symlink_metadata(&full) {
        if meta.file_type().is_symlink() {
            return Err(format!("refusing to write through symlink '{rel}'"));
        }
    }
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent for {rel}: {e}"))?;
    }
    let seq = BASELINE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let fname = full
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = full.with_file_name(format!(".{fname}.openro-tmp.{}.{seq}", std::process::id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("create temp for {rel}: {e}"))?;
    if let Err(e) = f.write_all(content.as_bytes()).and_then(|_| f.flush()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {rel}: {e}"));
    }
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, &full) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {rel}: {e}"));
    }
    Ok(())
}

fn load_sync_baseline_full(dir: &Path) -> SyncBaseline {
    let Ok(path) = projects::safe_join(dir, SYNC_BASELINE_REL) else {
        return SyncBaseline::default();
    };
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return SyncBaseline::default();
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_BASELINE_BYTES {
        return SyncBaseline::default();
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return SyncBaseline::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return SyncBaseline::default();
    };
    let mut b = SyncBaseline {
        baseline_id: v
            .get("baseline_id")
            .or_else(|| v.get("snapshot_id"))
            .and_then(Value::as_str)
            .map(String::from),
        scripts: BTreeMap::new(),
        script_hashes: BTreeMap::new(),
        instance_hashes: BTreeMap::new(),
        lighting_hashes: BTreeMap::new(),
    };
    if let Some(obj) = v.get("scripts").and_then(Value::as_object) {
        for (k, val) in obj {
            if let Some(s) = val.as_str() {
                b.scripts.insert(k.clone(), s.to_string());
                b.script_hashes.insert(k.clone(), hash31(s));
            }
        }
    }
    if let Some(obj) = v.get("script_hashes").and_then(Value::as_object) {
        for (k, val) in obj {
            if let Some(h) = val.as_u64() {
                b.script_hashes.insert(k.clone(), h);
            }
        }
    }
    if let Some(obj) = v.get("instance_hashes").and_then(Value::as_object) {
        for (k, val) in obj {
            if let Some(h) = val.as_u64() {
                b.instance_hashes.insert(k.clone(), h);
            }
        }
    }
    if let Some(obj) = v.get("lighting_hashes").and_then(Value::as_object) {
        for (k, val) in obj {
            if let Some(h) = val.as_u64() {
                b.lighting_hashes.insert(k.clone(), h);
            }
        }
    }
    b
}

fn write_sync_baseline_full(dir: &Path, b: &SyncBaseline) -> Result<(), String> {
    let doc = json!({
        "schema": "openro.sync_baseline.v2",
        "baseline_id": b.baseline_id,
        "snapshot_id": b.baseline_id,
        "updated_at_ms": now_ms(),
        "scripts": b.scripts,
        "script_hashes": b.script_hashes,
        "instance_hashes": b.instance_hashes,
        "lighting_hashes": b.lighting_hashes,
    });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    write_atomic_str_safe(dir, SYNC_BASELINE_REL, &text)
}

pub fn update_baseline_from_project(
    dir: &Path,
    rojo_file: &str,
    snapshot_id: Option<&str>,
) -> Result<(), String> {
    let local = collect_local_state(dir, rojo_file)?;
    let mut b = SyncBaseline {
        baseline_id: snapshot_id.map(String::from),
        scripts: BTreeMap::new(),
        script_hashes: BTreeMap::new(),
        instance_hashes: BTreeMap::new(),
        lighting_hashes: BTreeMap::new(),
    };
    for (rel, item) in &local.scripts {
        b.scripts.insert(rel.clone(), item.source.clone());
        b.script_hashes.insert(rel.clone(), item.hash);
    }
    for (p, item) in &local.instances {
        b.instance_hashes.insert(p.clone(), item.hash);
    }
    for (p, item) in &local.lighting {
        b.lighting_hashes.insert(p.clone(), item.hash);
    }
    write_sync_baseline_full(dir, &b)
}

fn round4_num(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let r = (x * 10_000.0).round() / 10_000.0;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

fn round_json_numbers(v: &Value) -> Value {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else if let Some(u) = n.as_u64() {
                json!(u)
            } else if let Some(f) = n.as_f64() {
                let r = round4_num(f);
                if (r - r.round()).abs() < 1e-9 && r.abs() < 1e12 {
                    json!(r.round() as i64)
                } else {
                    json!(r)
                }
            } else {
                v.clone()
            }
        }
        Value::Array(arr) => Value::Array(arr.iter().map(round_json_numbers).collect()),
        Value::Object(obj) => {
            let mut sorted = BTreeMap::new();
            for (k, val) in obj {
                sorted.insert(k.clone(), round_json_numbers(val));
            }
            Value::Object(sorted.into_iter().collect())
        }
        _ => v.clone(),
    }
}

fn canonicalize_props(class_name: &str, raw_props: &BTreeMap<String, Value>) -> Map<String, Value> {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    let mut pos_fallback: Option<[f64; 3]> = None;
    let mut has_cframe = false;

    for (k, v) in raw_props {
        if k == "Position" {
            pos_fallback = scene::parse_vec3(Some(v));
            continue;
        }
        if k == "Rotation" {
            continue;
        }
        if k == "CFrame" {
            if let Some(cf) = scene::parse_cframe_value(v) {
                has_cframe = true;
                let rounded_cf = cf.map(round4_num);
                map.insert("CFrame".into(), round_json_numbers(&scene::cframe_to_rojo_json(rounded_cf)));
                continue;
            }
        }
        if matches!(
            k.as_str(),
            "Color" | "Ambient" | "OutdoorAmbient" | "FogColor" | "ColorShift_Top" | "ColorShift_Bottom" | "Decay" | "BackgroundColor3" | "TextColor3" | "ImageColor3" | "Color3"
        ) {
            if let Some([r, g, b]) = scene::color_to_rgb_floats(v) {
                let key = if k == "Color3" { "Color" } else { k.as_str() };
                map.insert(
                    key.to_string(),
                    json!([round4_num(r), round4_num(g), round4_num(b)]),
                );
                continue;
            }
        }
        if let Some(enum_str) = v.as_str().and_then(|s| s.strip_prefix("Enum.")) {
            if let Some((_, item)) = enum_str.split_once('.') {
                map.insert(k.clone(), json!(item));
                continue;
            }
        }
        if let Some(obj) = v.as_object() {
            if let Some(enum_val) = obj.get("Enum").and_then(|x| x.as_str()) {
                let item = enum_val
                    .rsplit_once('.')
                    .map(|(_, r)| r)
                    .unwrap_or(enum_val);
                map.insert(k.clone(), json!(item));
                continue;
            }
            if let Some(content_val) = obj.get("Content").and_then(|x| x.as_str()) {
                map.insert(k.clone(), json!(content_val));
                continue;
            }
        }
        map.insert(k.clone(), round_json_numbers(v));
    }

    if !has_cframe {
        if let Some(pos) = pos_fallback {
            if matches!(
                class_name,
                "Part" | "MeshPart" | "SpawnLocation" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "Seat" | "VehicleSeat"
            ) {
                let cf = scene::cframe_from_pos_rot(pos, [0.0, 0.0, 0.0]).map(round4_num);
                map.insert("CFrame".into(), round_json_numbers(&scene::cframe_to_rojo_json(cf)));
            } else {
                map.insert("Position".into(), json!([round4_num(pos[0]), round4_num(pos[1]), round4_num(pos[2])]));
            }
        }
    }

    map.into_iter().collect()
}

fn canonical_instance_payload(class_name: &str, raw_props: &BTreeMap<String, Value>) -> (Value, String, u64) {
    let props = canonicalize_props(class_name, raw_props);
    let canonical = json!({
        "className": class_name,
        "properties": Value::Object(props),
    });
    let text = serde_json::to_string_pretty(&canonical).unwrap_or_default();
    let h = hash31(&text);
    (canonical, text, h)
}

#[derive(Clone, Debug)]
struct LocalScriptEntry {
    place_path: String,
    source: String,
    hash: u64,
}

#[derive(Clone, Debug)]
struct LocalInstanceEntry {
    canonical_text: String,
    hash: u64,
    source_file: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct LocalState {
    rojo_json: Value,
    scripts: BTreeMap<String, LocalScriptEntry>,
    place_to_rel: HashMap<String, String>,
    instances: BTreeMap<String, LocalInstanceEntry>,
    lighting: BTreeMap<String, LocalInstanceEntry>,
}

fn is_structural_script_mount(dot_path: &str, class_name: &str) -> bool {
    if class_name != "Folder" && !matches!(dot_path, "StarterPlayer.StarterPlayerScripts" | "StarterPlayer.StarterCharacterScripts") {
        return false;
    }
    matches!(
        dot_path,
        "ServerScriptService.Server"
            | "ReplicatedStorage.Shared"
            | "StarterPlayer.StarterPlayerScripts"
            | "StarterPlayer.StarterPlayerScripts.Client"
            | "StarterPlayer.StarterCharacterScripts"
    )
}

fn is_builtin_workspace_singleton(dot_path: &str) -> bool {
    matches!(dot_path, "Workspace.Camera" | "Workspace.Terrain")
}

fn collect_local_state(dir: &Path, rojo_file: &str) -> Result<LocalState, String> {
    let mut state = LocalState {
        rojo_json: json!({}),
        scripts: BTreeMap::new(),
        place_to_rel: HashMap::new(),
        instances: BTreeMap::new(),
        lighting: BTreeMap::new(),
    };
    let rojo_full = projects::safe_join(dir, rojo_file)?;
    if !rojo_full.is_file() {
        return Ok(state);
    }
    let rojo_raw = std::fs::read_to_string(&rojo_full)
        .map_err(|e| format!("read {rojo_file}: {e}"))?;
    state.rojo_json = serde_json::from_str(&rojo_raw)
        .map_err(|e| format!("{rojo_file} is not valid JSON: {e}"))?;

    let tree = projects::load_tree(dir, rojo_file)?;
    for (flat, source_file) in tree.flatten_with_source() {
        let dot_path = flat.path.replace('/', ".");
        if projects::is_script_class(&flat.class) {
            let source = flat.source.unwrap_or_default();
            let rel_path = source_file
                .filter(|s| s.ends_with(".luau") || s.ends_with(".lua"))
                .unwrap_or_else(|| infer_rel_script_path(&state.rojo_json, &dot_path, &flat.class));
            let h = hash31(&source);
            state.place_to_rel.insert(dot_path.clone(), rel_path.clone());
            state.scripts.insert(
                rel_path,
                LocalScriptEntry {
                    place_path: dot_path,
                    source,
                    hash: h,
                },
            );
        } else if dot_path == "Lighting" || dot_path.starts_with("Lighting.") {
            let (_canon, text, h) = canonical_instance_payload(&flat.class, &flat.props);
            state.lighting.insert(
                dot_path,
                LocalInstanceEntry {
                    canonical_text: text,
                    hash: h,
                    source_file,
                },
            );
        } else if !flat.service
            && !is_structural_script_mount(&dot_path, &flat.class)
            && !is_builtin_workspace_singleton(&dot_path)
        {
            let (_canon, text, h) = canonical_instance_payload(&flat.class, &flat.props);
            state.instances.insert(
                dot_path,
                LocalInstanceEntry {
                    canonical_text: text,
                    hash: h,
                    source_file,
                },
            );
        }
    }
    Ok(state)
}

fn sanitize_path_segment(seg: &str) -> String {
    let cleaned: String = seg
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "Item".to_string()
    } else {
        trimmed.to_string()
    }
}

fn script_suffix(class_name: &str) -> &'static str {
    match class_name {
        "Script" => ".server.luau",
        "LocalScript" => ".client.luau",
        _ => ".luau",
    }
}

fn collect_rojo_dir_mounts(rojo_json: &Value) -> Vec<(String, String)> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
        let Some(obj) = v.as_object() else { return };
        if let Some(p) = obj.get("$path").and_then(Value::as_str) {
            if !p.ends_with(".model.json") && !p.ends_with(".luau") && !p.ends_with(".lua") && !p.ends_with(".txt") {
                out.push((prefix.to_string(), p.trim_end_matches('/').to_string()));
            }
        }
        for (k, child) in obj {
            if k.starts_with('$') {
                continue;
            }
            let next = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            walk(&next, child, out);
        }
    }
    let mut out = Vec::new();
    if let Some(tree) = rojo_json.get("tree") {
        walk("", tree, &mut out);
    }
    out.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    out
}

fn infer_rel_script_path(rojo_json: &Value, dot_path: &str, class_name: &str) -> String {
    let suffix = script_suffix(class_name);
    for (mount_dot, mount_dir) in collect_rojo_dir_mounts(rojo_json) {
        let prefix = format!("{mount_dot}.");
        if let Some(rest) = dot_path.strip_prefix(&prefix) {
            let parts: Vec<String> = rest.split('.').map(sanitize_path_segment).collect();
            if !parts.is_empty() {
                return format!("{mount_dir}/{}{suffix}", parts.join("/"));
            }
        }
    }
    let segs: Vec<String> = dot_path.split('.').map(sanitize_path_segment).collect();
    if segs.is_empty() {
        return format!("src/studio/Script{suffix}");
    }
    match segs[0].as_str() {
        "ServerScriptService" => {
            let rest = if segs.get(1).map(|s| s.as_str()) == Some("Server") {
                &segs[2..]
            } else {
                &segs[1..]
            };
            let tail = if rest.is_empty() { "Main".to_string() } else { rest.join("/") };
            format!("src/server/{tail}{suffix}")
        }
        "StarterPlayer" => {
            let rest = if segs.get(1).map(|s| s.as_str()) == Some("StarterPlayerScripts") {
                if segs.get(2).map(|s| s.as_str()) == Some("Client") {
                    &segs[3..]
                } else {
                    &segs[2..]
                }
            } else {
                &segs[1..]
            };
            let tail = if rest.is_empty() { "Main".to_string() } else { rest.join("/") };
            format!("src/client/{tail}{suffix}")
        }
        "StarterGui" => {
            let tail = if segs.len() > 1 { segs[1..].join("/") } else { "Hud".to_string() };
            format!("src/gui/{tail}{suffix}")
        }
        "ReplicatedStorage" => {
            let rest = if segs.get(1).map(|s| s.as_str()) == Some("Shared") {
                &segs[2..]
            } else {
                &segs[1..]
            };
            let tail = if rest.is_empty() { "Module".to_string() } else { rest.join("/") };
            format!("src/shared/{tail}{suffix}")
        }
        _ => format!("src/studio/{}{suffix}", segs.join("/")),
    }
}

#[derive(Clone, Debug)]
struct StudioScriptEntry {
    rel_path: String,
    place_path: String,
    source: String,
    hash: u64,
}

#[derive(Clone, Debug)]
struct StudioInstanceEntry {
    place_path: String,
    class_name: String,
    props: Map<String, Value>,
    canonical_text: String,
    hash: u64,
}

#[derive(Clone, Debug, Default)]
struct StudioSnapshotState {
    snapshot_id: String,
    has_lighting_snapshot: bool,
    scripts: BTreeMap<String, StudioScriptEntry>,
    instances: BTreeMap<String, StudioInstanceEntry>,
    lighting: BTreeMap<String, StudioInstanceEntry>,
}

fn infer_class_from_dot_path(dot_path: &str) -> &'static str {
    if dot_path.starts_with("StarterGui.") || dot_path.starts_with("StarterPlayer.") {
        "LocalScript"
    } else if dot_path.starts_with("ReplicatedStorage.") {
        "ModuleScript"
    } else {
        "Script"
    }
}

fn build_studio_state(snap: &Value, local: &LocalState) -> StudioSnapshotState {
    let snapshot_id = snap
        .get("snapshot_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| format!("snap-{}", now_ms()));
    let mut out = StudioSnapshotState {
        snapshot_id,
        has_lighting_snapshot: false,
        scripts: BTreeMap::new(),
        instances: BTreeMap::new(),
        lighting: BTreeMap::new(),
    };
    let script_meta = snap.get("script_meta").and_then(Value::as_object);
    if let Some(scripts_obj) = snap.get("scripts").and_then(Value::as_object) {
        for (raw_key, src_val) in scripts_obj {
            let Some(source) = src_val.as_str() else { continue };
            let dot_path = raw_key.replace('/', ".");
            let class_name = script_meta
                .and_then(|m| m.get(raw_key).or_else(|| m.get(&dot_path)))
                .and_then(|m| m.get("class_name"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| infer_class_from_dot_path(&dot_path))
                .to_string();
            let rel_path = local
                .place_to_rel
                .get(&dot_path)
                .cloned()
                .unwrap_or_else(|| {
                    if (raw_key.ends_with(".luau") || raw_key.ends_with(".lua")) && !raw_key.contains("..") && !raw_key.starts_with('/') {
                        raw_key.clone()
                    } else {
                        infer_rel_script_path(&local.rojo_json, &dot_path, &class_name)
                    }
                });
            let h = hash31(source);
            out.scripts.insert(
                rel_path.clone(),
                StudioScriptEntry {
                    rel_path,
                    place_path: dot_path,
                    source: source.to_string(),
                    hash: h,
                },
            );
        }
    }

    if let Some(light_obj) = snap.get("lighting").and_then(Value::as_object) {
        if !light_obj.is_empty() {
            out.has_lighting_snapshot = true;
            let raw_btree: BTreeMap<String, Value> =
                light_obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let (canon, text, h) = canonical_instance_payload("Lighting", &raw_btree);
            let props = canon
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            out.lighting.insert(
                "Lighting".to_string(),
                StudioInstanceEntry {
                    place_path: "Lighting".to_string(),
                    class_name: "Lighting".to_string(),
                    props,
                    canonical_text: text,
                    hash: h,
                },
            );
        }
    }

    if let Some(inst_obj) = snap.get("instances").and_then(Value::as_object) {
        for (raw_key, val) in inst_obj {
            let dot_path = raw_key.replace('/', ".");
            let class_name = val
                .get("class_name")
                .or_else(|| val.get("className"))
                .and_then(Value::as_str)
                .unwrap_or("Folder")
                .to_string();
            let raw_props: BTreeMap<String, Value> = val
                .get("properties")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            if dot_path == "Lighting" || dot_path.starts_with("Lighting.") {
                out.has_lighting_snapshot = true;
                let (canon, text, h) = canonical_instance_payload(&class_name, &raw_props);
                let props = canon
                    .get("properties")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                out.lighting.insert(
                    dot_path.clone(),
                    StudioInstanceEntry {
                        place_path: dot_path,
                        class_name,
                        props,
                        canonical_text: text,
                        hash: h,
                    },
                );
            } else if !is_structural_script_mount(&dot_path, &class_name)
                && !is_builtin_workspace_singleton(&dot_path)
            {
                let (canon, text, h) = canonical_instance_payload(&class_name, &raw_props);
                let props = canon
                    .get("properties")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                out.instances.insert(
                    dot_path.clone(),
                    StudioInstanceEntry {
                        place_path: dot_path,
                        class_name,
                        props,
                        canonical_text: text,
                        hash: h,
                    },
                );
            }
        }
    }
    out
}

fn hunks_json(path: &str, status: &str, before_text: &str, after_text: &str) -> Vec<Value> {
    let diff = snapshots::compute_script_diff(path, status, before_text, after_text);
    diff.hunks
        .into_iter()
        .map(|h| {
            let lines: Vec<Value> = h
                .lines
                .into_iter()
                .map(|l| {
                    let op = match l.kind.as_str() {
                        "add" => "add",
                        "del" | "remove" => "del",
                        _ => "context",
                    };
                    json!({
                        "op": op,
                        "text": l.text,
                        "old_line": l.old_line,
                        "new_line": l.new_line,
                    })
                })
                .collect();
            let header = format!(
                "@@ -{},{} +{},{} @@",
                h.old_start, h.old_count, h.new_start, h.new_count
            );
            json!({ "header": header, "lines": lines })
        })
        .collect()
}

fn classify_three_way(
    baseline: Option<u64>,
    local: Option<u64>,
    studio: Option<u64>,
) -> Option<&'static str> {
    if local == studio {
        return None;
    }
    match (baseline, local, studio) {
        (None, None, Some(_)) => Some("studio_only"),
        (Some(_), None, Some(_)) => Some("conflict"),
        (None, Some(_), None) => Some("local_only"),
        (Some(b), Some(l), None) => {
            if l == b {
                Some("local_only")
            } else {
                Some("conflict")
            }
        }
        (None, Some(_), Some(_)) => Some("conflict"),
        (Some(b), Some(l), Some(s)) => {
            if l == b && s != b {
                Some("studio_modified")
            } else if l != b && s == b {
                Some("local_only")
            } else {
                Some("conflict")
            }
        }
        (None, None, None) | (Some(_), None, None) => None,
    }
}

#[derive(Clone, Debug)]
struct PullComputedItem {
    change: StudioPullChange,
    place_path: String,
    local_source_file: Option<String>,
}

fn compute_pull_changes(
    local: &LocalState,
    studio: &StudioSnapshotState,
    baseline: &SyncBaseline,
) -> Vec<PullComputedItem> {
    let mut items = Vec::new();

    // 1. Scripts
    let mut script_paths: BTreeSet<String> = BTreeSet::new();
    script_paths.extend(local.scripts.keys().cloned());
    script_paths.extend(studio.scripts.keys().cloned());
    for rel in script_paths {
        let loc = local.scripts.get(&rel);
        let stu = studio.scripts.get(&rel);
        let l_hash = loc.map(|x| x.hash);
        let s_hash = stu.map(|x| x.hash);
        let b_hash = baseline.script_hashes.get(&rel).copied();
        let Some(status) = classify_three_way(b_hash, l_hash, s_hash) else {
            continue;
        };
        let l_txt = loc.map(|x| x.source.as_str()).unwrap_or("");
        let s_txt = stu.map(|x| x.source.as_str()).unwrap_or("");
        let place_path = stu
            .map(|x| x.place_path.clone())
            .or_else(|| loc.map(|x| x.place_path.clone()))
            .unwrap_or_else(|| rel.clone());
        items.push(PullComputedItem {
            change: StudioPullChange {
                path: rel.clone(),
                kind: "script".to_string(),
                status: status.to_string(),
                local_hash: l_hash,
                studio_hash: s_hash,
                baseline_hash: b_hash,
                hunks: hunks_json(&rel, status, l_txt, s_txt),
            },
            place_path,
            local_source_file: Some(rel),
        });
    }

    // 2. Instances
    let mut inst_paths: BTreeSet<String> = BTreeSet::new();
    inst_paths.extend(local.instances.keys().cloned());
    inst_paths.extend(studio.instances.keys().cloned());
    for dot_path in inst_paths {
        let loc = local.instances.get(&dot_path);
        let stu = studio.instances.get(&dot_path);
        let l_hash = loc.map(|x| x.hash);
        let s_hash = stu.map(|x| x.hash);
        let b_hash = baseline.instance_hashes.get(&dot_path).copied();
        let Some(status) = classify_three_way(b_hash, l_hash, s_hash) else {
            continue;
        };
        let l_txt = loc.map(|x| x.canonical_text.as_str()).unwrap_or("");
        let s_txt = stu.map(|x| x.canonical_text.as_str()).unwrap_or("");
        items.push(PullComputedItem {
            change: StudioPullChange {
                path: dot_path.clone(),
                kind: "instance".to_string(),
                status: status.to_string(),
                local_hash: l_hash,
                studio_hash: s_hash,
                baseline_hash: b_hash,
                hunks: hunks_json(&dot_path, status, l_txt, s_txt),
            },
            place_path: dot_path,
            local_source_file: loc.and_then(|x| x.source_file.clone()),
        });
    }

    // 3. Lighting (only when Studio snapshot includes lighting or baseline tracked lighting)
    let mut light_paths: BTreeSet<String> = BTreeSet::new();
    if studio.has_lighting_snapshot || !baseline.lighting_hashes.is_empty() {
        light_paths.extend(local.lighting.keys().cloned());
        light_paths.extend(studio.lighting.keys().cloned());
    }
    for dot_path in light_paths {
        let loc = local.lighting.get(&dot_path);
        let stu = studio.lighting.get(&dot_path);
        let l_hash = loc.map(|x| x.hash);
        let s_hash = stu.map(|x| x.hash);
        let b_hash = baseline.lighting_hashes.get(&dot_path).copied();
        let Some(status) = classify_three_way(b_hash, l_hash, s_hash) else {
            continue;
        };
        let l_txt = loc.map(|x| x.canonical_text.as_str()).unwrap_or("");
        let s_txt = stu.map(|x| x.canonical_text.as_str()).unwrap_or("");
        items.push(PullComputedItem {
            change: StudioPullChange {
                path: dot_path.clone(),
                kind: "lighting".to_string(),
                status: status.to_string(),
                local_hash: l_hash,
                studio_hash: s_hash,
                baseline_hash: b_hash,
                hunks: hunks_json(&dot_path, status, l_txt, s_txt),
            },
            place_path: dot_path,
            local_source_file: loc.and_then(|x| x.source_file.clone()),
        });
    }

    items.sort_by(|a, b| {
        let depth_a = a.place_path.matches('.').count();
        let depth_b = b.place_path.matches('.').count();
        a.change
            .kind
            .cmp(&b.change.kind)
            .then_with(|| depth_a.cmp(&depth_b))
            .then_with(|| a.change.path.cmp(&b.change.path))
    });
    items
}

fn validate_requested_paths(dir: &Path, paths: Option<&[String]>) -> Result<Option<Vec<String>>, PullError> {
    let Some(list) = paths else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(list.len());
    for raw in list {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed.contains('\0')
            || trimmed.contains('\\')
            || trimmed.starts_with('/')
            || trimmed.split('/').any(|s| s == "..")
            || trimmed.split('.').any(|s| s == "..")
            || trimmed.starts_with(".openro")
            || trimmed.starts_with(".git")
        {
            return Err(PullError::BadRequest(format!(
                "invalid pull path '{raw}'"
            )));
        }
        if trimmed.contains('/') {
            projects::safe_join(dir, trimmed).map_err(PullError::BadRequest)?;
        }
        out.push(trimmed.to_string());
    }
    Ok(Some(out))
}

fn matches_requested_path(item: &PullComputedItem, filter: &[String]) -> bool {
    let slash_place = item.place_path.replace('.', "/");
    filter.iter().any(|p| {
        p == &item.change.path || p == &item.place_path || p == &slash_place
    })
}

fn ensure_rojo_script_wired(dir: &Path, rojo_file: &str, dot_path: &str, rel_path: &str) -> Result<(), String> {
    let rojo_full = projects::safe_join(dir, rojo_file)?;
    let mut rojo: Value = if rojo_full.is_file() {
        let raw = std::fs::read_to_string(&rojo_full).map_err(|e| format!("read {rojo_file}: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("{rojo_file}: {e}"))?
    } else {
        json!({ "name": "Project", "tree": { "$className": "DataModel" } })
    };
    // If any existing mount directory already covers this rel_path, nothing to wire.
    for (mount_dot, mount_dir) in collect_rojo_dir_mounts(&rojo) {
        if rel_path.starts_with(&format!("{mount_dir}/")) && dot_path.starts_with(&format!("{mount_dot}.")) {
            return Ok(());
        }
    }
    let segs: Vec<&str> = dot_path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return Ok(());
    }
    let root_obj = rojo
        .as_object_mut()
        .ok_or_else(|| format!("{rojo_file} must be a JSON object"))?;
    let tree = root_obj
        .entry("tree")
        .or_insert_with(|| json!({ "$className": "DataModel" }))
        .as_object_mut()
        .ok_or_else(|| format!("{rojo_file} tree must be an object"))?;

    let mut cur = tree;
    for (idx, seg) in segs.iter().enumerate() {
        let is_last = idx + 1 == segs.len();
        if is_last {
            cur.insert((*seg).to_string(), json!({ "$path": rel_path }));
            break;
        }
        let next = cur
            .entry((*seg).to_string())
            .or_insert_with(|| {
                if idx == 0 {
                    json!({ "$className": *seg, "$ignoreUnknownInstances": true })
                } else {
                    json!({ "$className": "Folder", "$ignoreUnknownInstances": true })
                }
            })
            .as_object_mut()
            .ok_or_else(|| format!("{rojo_file} node '{seg}' must be an object"))?;
        cur = next;
    }
    let text = serde_json::to_string_pretty(&rojo).map_err(|e| e.to_string())? + "\n";
    write_atomic_str_safe(dir, rojo_file, &text)
}

fn upsert_model_json_node(
    root: &mut Value,
    segs: &[&str],
    class_name: &str,
    props: &Map<String, Value>,
) -> Result<(), String> {
    let mut cur = root;
    for (idx, seg) in segs.iter().enumerate() {
        let is_leaf = idx + 1 == segs.len();
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| "model.json node must be a JSON object".to_string())?;
        if !obj.contains_key("children") && obj.contains_key("Children") {
            if let Some(c) = obj.remove("Children") {
                obj.insert("children".into(), c);
            }
        }
        let kids = obj
            .entry("children")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| "model.json children must be a JSON array".to_string())?;
        let found_idx = kids.iter().position(|c| {
            c.get("name")
                .or_else(|| c.get("Name"))
                .and_then(Value::as_str)
                == Some(*seg)
        });
        let target_idx = match found_idx {
            Some(i) => i,
            None => {
                kids.push(json!({
                    "name": *seg,
                    "className": if is_leaf { class_name } else { "Folder" },
                    "properties": {},
                    "children": []
                }));
                kids.len() - 1
            }
        };
        cur = &mut kids[target_idx];
        if is_leaf {
            let leaf = cur
                .as_object_mut()
                .ok_or_else(|| "model.json leaf must be a JSON object".to_string())?;
            leaf.remove("ClassName");
            leaf.remove("Name");
            leaf.remove("Properties");
            leaf.insert("name".into(), json!(*seg));
            leaf.insert("className".into(), json!(class_name));
            leaf.insert("properties".into(), Value::Object(props.clone()));
            if !leaf.contains_key("children") {
                leaf.insert("children".into(), json!([]));
            }
        }
    }
    Ok(())
}

fn remove_model_json_node(root: &mut Value, segs: &[&str]) -> bool {
    if segs.is_empty() {
        return false;
    }
    let mut cur = root;
    for seg in &segs[..segs.len() - 1] {
        let kids_key = if cur.get("children").is_some() {
            "children"
        } else {
            "Children"
        };
        let Some(kids) = cur.get_mut(kids_key).and_then(Value::as_array_mut) else {
            return false;
        };
        let Some(next) = kids.iter_mut().find(|c| {
            c.get("name")
                .or_else(|| c.get("Name"))
                .and_then(Value::as_str)
                == Some(*seg)
        }) else {
            return false;
        };
        cur = next;
    }
    let kids_key = if cur.get("children").is_some() {
        "children"
    } else {
        "Children"
    };
    let Some(kids) = cur.get_mut(kids_key).and_then(Value::as_array_mut) else {
        return false;
    };
    let leaf = segs[segs.len() - 1];
    if let Some(pos) = kids.iter().position(|c| {
        c.get("name")
            .or_else(|| c.get("Name"))
            .and_then(Value::as_str)
            == Some(leaf)
    }) {
        kids.remove(pos);
        return true;
    }
    false
}

fn find_model_mount_for_dot_path(rojo: &Value, dot_path: &str) -> Option<(String, String)> {
    let mut cur = rojo.get("tree")?;
    let segs: Vec<&str> = dot_path.split('.').filter(|s| !s.is_empty()).collect();
    let mut matched: Option<(String, String)> = None;
    let mut prefix = String::new();
    for seg in segs {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(seg);
        cur = cur.get(seg)?;
        if let Some(p) = cur.get("$path").and_then(Value::as_str) {
            if p.ends_with(".model.json") {
                matched = Some((prefix.clone(), p.to_string()));
            }
        }
    }
    matched
}

fn apply_instance_to_project(
    dir: &Path,
    rojo_file: &str,
    dot_path: &str,
    class_name: &str,
    props: &Map<String, Value>,
    local_source_file: Option<&str>,
) -> Result<(), String> {
    let rojo_full = projects::safe_join(dir, rojo_file)?;
    let mut rojo: Value = if rojo_full.is_file() {
        let raw = std::fs::read_to_string(&rojo_full).map_err(|e| format!("read {rojo_file}: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("{rojo_file}: {e}"))?
    } else {
        json!({ "name": "Project", "tree": { "$className": "DataModel" } })
    };

    if let Some(rest) = dot_path.strip_prefix("Workspace.") {
        let model_mount = local_source_file
            .filter(|s| s.ends_with(".model.json") && *s != scene::SCENE_MODEL_REL)
            .and_then(|s| {
                find_model_mount_for_dot_path(&rojo, dot_path).or_else(|| Some(("Workspace".to_string(), s.to_string())))
            })
            .or_else(|| find_model_mount_for_dot_path(&rojo, dot_path));

        if let Some((mount_prefix, model_rel)) = model_mount {
            let model_full = projects::safe_join(dir, &model_rel)?;
            let mut model_val: Value = if model_full.is_file() {
                let raw = std::fs::read_to_string(&model_full).map_err(|e| format!("read {model_rel}: {e}"))?;
                serde_json::from_str(&raw).map_err(|e| format!("{model_rel}: {e}"))?
            } else {
                json!({ "className": "Workspace", "properties": { "Gravity": 196.2 }, "children": [] })
            };
            if mount_prefix == dot_path {
                if let Some(obj) = model_val.as_object_mut() {
                    obj.insert("className".into(), json!(class_name));
                    obj.insert("properties".into(), Value::Object(props.clone()));
                }
            } else {
                let sub = dot_path
                    .strip_prefix(&format!("{mount_prefix}."))
                    .unwrap_or(rest);
                let segs: Vec<&str> = sub.split('.').filter(|s| !s.is_empty()).collect();
                upsert_model_json_node(&mut model_val, &segs, class_name, props)?;
            }
            let out_text = serde_json::to_string_pretty(&model_val).map_err(|e| e.to_string())? + "\n";
            write_atomic_str_safe(dir, &model_rel, &out_text)?;
            return Ok(());
        }

        let ws_full = projects::safe_join(dir, scene::SCENE_MODEL_REL)?;
        let mut ws_val: Value = if ws_full.is_file() {
            let raw = std::fs::read_to_string(&ws_full)
                .map_err(|e| format!("read {}: {e}", scene::SCENE_MODEL_REL))?;
            serde_json::from_str(&raw)
                .map_err(|e| format!("{}: {e}", scene::SCENE_MODEL_REL))?
        } else {
            json!({
                "className": "Workspace",
                "properties": { "Gravity": 196.2 },
                "children": []
            })
        };
        let segs: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();
        upsert_model_json_node(&mut ws_val, &segs, class_name, props)?;
        let out_text = serde_json::to_string_pretty(&ws_val).map_err(|e| e.to_string())? + "\n";
        write_atomic_str_safe(dir, scene::SCENE_MODEL_REL, &out_text)?;

        let tree_obj = rojo
            .as_object_mut()
            .and_then(|o| o.get_mut("tree"))
            .and_then(Value::as_object_mut)
            .ok_or("default.project.json missing tree object")?;
        let mut rojo_dirty = false;
        if !tree_obj.contains_key("Workspace") {
            tree_obj.insert("Workspace".into(), json!({ "$path": scene::SCENE_MODEL_REL }));
            rojo_dirty = true;
        } else if let Some(ws_obj) = tree_obj.get_mut("Workspace").and_then(Value::as_object_mut) {
            if !ws_obj.contains_key("$path") {
                ws_obj.insert("$path".into(), json!(scene::SCENE_MODEL_REL));
                ws_obj.remove("$className");
                rojo_dirty = true;
            }
        }
        if rojo_dirty {
            let r_text = serde_json::to_string_pretty(&rojo).map_err(|e| e.to_string())? + "\n";
            write_atomic_str_safe(dir, rojo_file, &r_text)?;
        }
        return Ok(());
    }

    // Lighting or other service instance: store cleanly inside default.project.json tree
    let segs: Vec<&str> = dot_path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Ok(());
    }
    let tree_obj = rojo
        .as_object_mut()
        .ok_or("default.project.json must be object")?
        .entry("tree")
        .or_insert_with(|| json!({ "$className": "DataModel" }))
        .as_object_mut()
        .ok_or("default.project.json tree must be object")?;
    let mut cur = tree_obj;
    for (idx, seg) in segs.iter().enumerate() {
        let is_last = idx + 1 == segs.len();
        let entry = cur.entry((*seg).to_string()).or_insert_with(|| {
            json!({
                "$className": if is_last { class_name } else if idx == 0 { *seg } else { "Folder" },
                "$ignoreUnknownInstances": true
            })
        });
        let obj = entry
            .as_object_mut()
            .ok_or_else(|| format!("default.project.json node '{seg}' must be object"))?;
        if is_last {
            obj.insert("$className".into(), json!(class_name));
            if !props.is_empty() {
                obj.insert("$properties".into(), Value::Object(props.clone()));
            } else {
                obj.remove("$properties");
            }
        }
        cur = obj;
    }
    let r_text = serde_json::to_string_pretty(&rojo).map_err(|e| e.to_string())? + "\n";
    write_atomic_str_safe(dir, rojo_file, &r_text)
}

fn delete_instance_from_project(
    dir: &Path,
    rojo_file: &str,
    dot_path: &str,
    local_source_file: Option<&str>,
) -> Result<(), String> {
    if let Some(rest) = dot_path.strip_prefix("Workspace.") {
        let model_rel = local_source_file.unwrap_or(scene::SCENE_MODEL_REL);
        let full = projects::safe_join(dir, model_rel)?;
        if full.is_file() {
            let raw = std::fs::read_to_string(&full).map_err(|e| format!("read {model_rel}: {e}"))?;
            let mut val: Value = serde_json::from_str(&raw).map_err(|e| format!("{model_rel}: {e}"))?;
            let segs: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();
            if remove_model_json_node(&mut val, &segs) {
                let text = serde_json::to_string_pretty(&val).map_err(|e| e.to_string())? + "\n";
                write_atomic_str_safe(dir, model_rel, &text)?;
            }
        }
        return Ok(());
    }
    let rojo_full = projects::safe_join(dir, rojo_file)?;
    if !rojo_full.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&rojo_full).map_err(|e| format!("read {rojo_file}: {e}"))?;
    let mut rojo: Value = serde_json::from_str(&raw).map_err(|e| format!("{rojo_file}: {e}"))?;
    let segs: Vec<&str> = dot_path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return Ok(());
    }
    if let Some(mut cur) = rojo.get_mut("tree").and_then(Value::as_object_mut) {
        for seg in &segs[..segs.len() - 1] {
            let Some(next) = cur.get_mut(*seg).and_then(Value::as_object_mut) else {
                return Ok(());
            };
            cur = next;
        }
        cur.remove(segs[segs.len() - 1]);
        let r_text = serde_json::to_string_pretty(&rojo).map_err(|e| e.to_string())? + "\n";
        write_atomic_str_safe(dir, rojo_file, &r_text)?;
    }
    Ok(())
}

pub fn pull_diff_with_backend(
    backend: &mut PlaceBackend,
    project_dir: &Path,
) -> Result<StudioPullResponse, PullError> {
    let manifest = projects::manifest(project_dir).map_err(PullError::BadRequest)?;
    let rojo = manifest["rojoProject"]
        .as_str()
        .unwrap_or(projects::DEFAULT_ROJO_FILE)
        .to_string();
    let local = collect_local_state(project_dir, &rojo).map_err(PullError::Internal)?;
    let snap = backend
        .get_place_snapshot()
        .map_err(PullError::Internal)?;
    let studio = build_studio_state(&snap, &local);
    let baseline = load_sync_baseline_full(project_dir);
    let computed = compute_pull_changes(&local, &studio, &baseline);
    let conflict_count = computed
        .iter()
        .filter(|i| i.change.status == "conflict")
        .count();
    Ok(StudioPullResponse {
        ok: true,
        snapshot_id: studio.snapshot_id,
        changes: computed.into_iter().map(|i| i.change).collect(),
        applied_count: 0,
        conflict_count,
    })
}

pub fn pull_apply_with_backend(
    backend: &mut PlaceBackend,
    project_dir: &Path,
    paths: Option<&[String]>,
    strategy: PullStrategy,
    confirm_overwrite: bool,
) -> Result<StudioPullResponse, PullError> {
    let valid_paths = validate_requested_paths(project_dir, paths)?;
    let manifest = projects::manifest(project_dir).map_err(PullError::BadRequest)?;
    let rojo = manifest["rojoProject"]
        .as_str()
        .unwrap_or(projects::DEFAULT_ROJO_FILE)
        .to_string();
    let local = collect_local_state(project_dir, &rojo).map_err(PullError::Internal)?;
    let snap = backend
        .get_place_snapshot()
        .map_err(PullError::Internal)?;
    let studio = build_studio_state(&snap, &local);
    let mut baseline = load_sync_baseline_full(project_dir);
    let computed = compute_pull_changes(&local, &studio, &baseline);

    let selected: Vec<&PullComputedItem> = computed
        .iter()
        .filter(|item| match &valid_paths {
            None => true,
            Some(filter) => matches_requested_path(item, filter),
        })
        .collect();

    if strategy == PullStrategy::TakeStudio {
        let needs_confirm = selected.iter().any(|item| {
            item.change.status == "conflict"
                || (item.change.status == "local_only"
                    && item.change.baseline_hash.is_some()
                    && valid_paths.is_some())
        });
        if needs_confirm && !confirm_overwrite {
            let conflict_count = computed
                .iter()
                .filter(|i| i.change.status == "conflict")
                .count();
            return Err(PullError::ConflictConfirmRequired(StudioPullResponse {
                ok: false,
                snapshot_id: studio.snapshot_id,
                changes: computed.into_iter().map(|i| i.change).collect(),
                applied_count: 0,
                conflict_count,
            }));
        }
    }

    if strategy == PullStrategy::KeepLocal {
        let conflict_count = computed
            .iter()
            .filter(|i| i.change.status == "conflict")
            .count();
        return Ok(StudioPullResponse {
            ok: true,
            snapshot_id: studio.snapshot_id,
            changes: computed.into_iter().map(|i| i.change).collect(),
            applied_count: 0,
            conflict_count,
        });
    }

    let mut applied_count = 0usize;
    for item in selected {
        let st = item.change.status.as_str();
        let should_pull = match strategy {
            PullStrategy::MergeNonConflicting => {
                st == "studio_only" || st == "studio_modified"
            }
            PullStrategy::TakeStudio => {
                st == "studio_only" || st == "studio_modified" || st == "conflict"
            }
            PullStrategy::KeepLocal => false,
        };
        let should_delete_local = strategy == PullStrategy::TakeStudio
            && confirm_overwrite
            && st == "local_only"
            && item.change.baseline_hash.is_some()
            && (valid_paths.is_some() || item.change.local_hash == item.change.baseline_hash);

        if should_pull {
            match item.change.kind.as_str() {
                "script" => {
                    if let Some(stu) = studio.scripts.get(&item.change.path) {
                        write_atomic_str_safe(project_dir, &stu.rel_path, &stu.source)
                            .map_err(PullError::Internal)?;
                        ensure_rojo_script_wired(
                            project_dir,
                            &rojo,
                            &stu.place_path,
                            &stu.rel_path,
                        )
                        .map_err(PullError::Internal)?;
                        baseline
                            .scripts
                            .insert(stu.rel_path.clone(), stu.source.clone());
                        baseline
                            .script_hashes
                            .insert(stu.rel_path.clone(), stu.hash);
                        applied_count += 1;
                    }
                }
                "instance" => {
                    if let Some(stu) = studio.instances.get(&item.place_path) {
                        apply_instance_to_project(
                            project_dir,
                            &rojo,
                            &stu.place_path,
                            &stu.class_name,
                            &stu.props,
                            item.local_source_file.as_deref(),
                        )
                        .map_err(PullError::Internal)?;
                        baseline
                            .instance_hashes
                            .insert(stu.place_path.clone(), stu.hash);
                        applied_count += 1;
                    }
                }
                "lighting" => {
                    if let Some(stu) = studio.lighting.get(&item.place_path) {
                        apply_instance_to_project(
                            project_dir,
                            &rojo,
                            &stu.place_path,
                            &stu.class_name,
                            &stu.props,
                            item.local_source_file.as_deref(),
                        )
                        .map_err(PullError::Internal)?;
                        baseline
                            .lighting_hashes
                            .insert(stu.place_path.clone(), stu.hash);
                        applied_count += 1;
                    }
                }
                _ => {}
            }
        } else if should_delete_local {
            match item.change.kind.as_str() {
                "script" => {
                    let full = projects::safe_join(project_dir, &item.change.path)
                        .map_err(PullError::BadRequest)?;
                    if full.is_file() {
                        std::fs::remove_file(&full)
                            .map_err(|e| PullError::Internal(format!("remove {}: {e}", item.change.path)))?;
                    }
                    baseline.scripts.remove(&item.change.path);
                    baseline.script_hashes.remove(&item.change.path);
                    applied_count += 1;
                }
                "instance" => {
                    delete_instance_from_project(
                        project_dir,
                        &rojo,
                        &item.place_path,
                        item.local_source_file.as_deref(),
                    )
                    .map_err(PullError::Internal)?;
                    baseline.instance_hashes.remove(&item.place_path);
                    applied_count += 1;
                }
                "lighting" => {
                    delete_instance_from_project(
                        project_dir,
                        &rojo,
                        &item.place_path,
                        item.local_source_file.as_deref(),
                    )
                    .map_err(PullError::Internal)?;
                    baseline.lighting_hashes.remove(&item.place_path);
                    applied_count += 1;
                }
                _ => {}
            }
        }
    }

    if applied_count > 0 {
        baseline.baseline_id = Some(studio.snapshot_id.clone());
        write_sync_baseline_full(project_dir, &baseline).map_err(PullError::Internal)?;
    }

    let local_after = collect_local_state(project_dir, &rojo).map_err(PullError::Internal)?;
    let remaining = compute_pull_changes(&local_after, &studio, &baseline);
    let conflict_count = remaining
        .iter()
        .filter(|i| i.change.status == "conflict")
        .count();
    Ok(StudioPullResponse {
        ok: true,
        snapshot_id: studio.snapshot_id,
        changes: remaining.into_iter().map(|i| i.change).collect(),
        applied_count,
        conflict_count,
    })
}

pub fn pull_diff(shared: &Shared, r: &ProjectRef) -> Result<Reply, Reply> {
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    let Some(backend) = agent.studio.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    match pull_diff_with_backend(backend, &r.dir) {
        Ok(resp) => Ok(Reply::json("200 OK", resp.to_json())),
        Err(e) => Err(e.into_reply()),
    }
}

pub fn pull_apply(shared: &Shared, r: &ProjectRef, body: &Value) -> Result<Reply, Reply> {
    let strategy = PullStrategy::parse(body.get("strategy").and_then(Value::as_str))
        .map_err(Reply::bad)?;
    let confirm_overwrite = body
        .get("confirm_overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let paths: Option<Vec<String>> = match body.get("paths") {
        None | Some(Value::Null) => None,
        Some(Value::Array(arr)) => {
            let mut v = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| Reply::bad("paths must be an array of strings"))?;
                v.push(s.to_string());
            }
            Some(v)
        }
        Some(_) => return Err(Reply::bad("paths must be an array of strings")),
    };

    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    let Some(backend) = agent.studio.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    match pull_apply_with_backend(backend, &r.dir, paths.as_deref(), strategy, confirm_overwrite) {
        Ok(resp) => Ok(Reply::json("200 OK", resp.to_json())),
        Err(e) => Err(e.into_reply()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_the_luau_definition() {
        assert_eq!(hash31(""), 0);
        assert_eq!(hash31("a"), 97);
        assert_eq!(hash31("ab"), 97 * 31 + 98);
        // Wraps at 2^32 like the Luau modulo.
        assert!(hash31(&"z".repeat(100)) < 4_294_967_296);
    }

    #[test]
    fn workspace_is_walked_last_and_structural_first() {
        let mk = |p: &str| Flat { path: p.into(), class: "Folder".into(), props: Default::default(), source: None, service: false, ignore_unknown: true };
        let o = ordered(vec![mk("Workspace/A"), mk("Lighting"), mk("ReplicatedStorage/X"), mk("Weird/Y")]);
        let paths: Vec<_> = o.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["ReplicatedStorage/X", "Lighting", "Weird/Y", "Workspace/A"]);
    }

    #[test]
    fn result_is_found_in_any_wrapping() {
        let want = json!({ "ok": 3 });
        for v in [
            json!({ "output": "OPENRO_V011:{\"ok\":3}" }),
            json!("OPENRO_V011:{\"ok\":3}"),
            json!({ "result": { "lines": ["noise", "OPENRO_V011:{\"ok\":3}"] } }),
        ] {
            assert_eq!(extract_result(&v), Some(want.clone()));
        }
        assert_eq!(extract_result(&json!({ "output": "nothing" })), None);
    }

    #[test]
    fn long_sources_are_split_on_char_boundaries() {
        let s = "é".repeat(SOURCE_PIECE);
        let ps = pieces(&s);
        assert!(ps.len() >= 2 && ps.iter().all(|p| p.len() <= SOURCE_PIECE));
        assert_eq!(ps.concat(), s);
    }
}
