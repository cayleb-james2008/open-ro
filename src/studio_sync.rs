//! One-way project -> Studio sync over StudioMCP `execute_luau`.
//!
//! The diff is computed inside Studio: OpenRO sends chunks of the project's
//! instances (class, a hash of each script's source, declared properties) and
//! Studio answers only with what differs. Applying sends small create/update
//! batches. Every request and every reply stays far below StudioMCP's
//! 100,000-character return limit, and instances are walked structural
//! services first, `Workspace` last.

use crate::mcp::{luau_long_string, McpClient};
use crate::project_api::{with_studio, Reply};
use crate::projects::{self, Flat, ProjectRef};
use crate::server::Shared;
use serde_json::{json, Value};

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
        out["applied"] = json!({ "created": a.created, "updated": a.updated, "deleted": a.deleted });
        out["errors"] = json!(a.errors);
        out["chunks"] = json!(d.chunks + a.chunks + after.chunks);
        out["verify"] = json!({ "remaining": remaining, "extra": after.extra.len() });
        Ok(Reply::ok(out))
    })
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
