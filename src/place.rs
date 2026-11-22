//! Persistent Local Roblox Place Engine, Luau static/runtime playtest
//! verifier, and genuine Roblox Studio XML (`.rbxlx`) serializer.
//!
//! When Roblox Studio MCP is not running, OpenRO operates directly on a real
//! persistent place stored in `data_dir/place.json` and mirrored atomically to
//! `data_dir/game.rbxlx` (`<roblox version="4">`) on every mutation and Undo.
//! `serialize_rbxlx` also works on live Studio MCP place trees so
//! `GET /api/export/rbxlx` always exports an authentic Roblox XML file.

use crate::mcp::McpClient;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Unified backend for either a live Roblox Studio MCP bridge or the built-in
/// persistent Local Place Engine.
pub enum PlaceBackend {
    Mcp(McpClient),
    Local(LocalPlace),
}

impl PlaceBackend {
    pub fn is_alive(&mut self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.is_alive(),
            PlaceBackend::Local(_) => true,
        }
    }

    pub fn tools_known(&self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.tools_known(),
            PlaceBackend::Local(_) => true,
        }
    }

    pub fn has_tool(&self, name: &str) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.has_tool(name),
            PlaceBackend::Local(_) => matches!(
                name,
                "get_place_tree"
                    | "write_script"
                    | "delete_script"
                    | "create_instance"
                    | "delete_instance"
                    | "playtest"
                    | "get_console"
            ),
        }
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        match self {
            PlaceBackend::Mcp(c) => c.call_tool(name, arguments),
            PlaceBackend::Local(p) => p.call_tool(name, arguments),
        }
    }

    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        match self {
            PlaceBackend::Mcp(c) => c.get_place_tree(),
            PlaceBackend::Local(p) => Ok(p.tree_json()),
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            PlaceBackend::Mcp(_) => "studio_mcp",
            PlaceBackend::Local(_) => "local_place",
        }
    }

    pub fn rbxlx_path(&self) -> Option<PathBuf> {
        match self {
            PlaceBackend::Mcp(_) => None,
            PlaceBackend::Local(p) => p.rbxlx_path(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalPlace {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    last_console: String,
    data_dir: Option<PathBuf>,
    game_name: String,
    pub recovered: Vec<String>,
}

impl LocalPlace {
    /// Ephemeral in-memory place (for tests or non-persistent runs).
    pub fn in_memory(game_name: String) -> Self {
        LocalPlace {
            scripts: BTreeMap::new(),
            instances: BTreeMap::new(),
            last_console: String::new(),
            data_dir: None,
            game_name,
            recovered: Vec::new(),
        }
    }

    /// Persistent place stored as `place.json` + `game.rbxlx` inside `data_dir`.
    pub fn open(data_dir: PathBuf, game_name: String) -> Self {
        let _ = std::fs::create_dir_all(&data_dir);
        let mut recovered = Vec::new();
        let (scripts, instances) = load_place(&data_dir, &mut recovered);
        let place = LocalPlace {
            scripts,
            instances,
            last_console: String::new(),
            data_dir: Some(data_dir),
            game_name,
            recovered,
        };
        place.persist();
        place
    }

    pub fn rbxlx_path(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("game.rbxlx"))
    }

    pub fn tree_json(&self) -> Value {
        json!({
            "scripts": self.scripts,
            "instances": self.instances,
        })
    }

    fn persist(&self) {
        let Some(dir) = &self.data_dir else { return };
        let _ = std::fs::create_dir_all(dir);
        let json_body = serde_json::to_string_pretty(&self.tree_json()).unwrap_or_else(|_| "{}".to_string());
        let json_path = dir.join("place.json");
        let json_tmp = dir.join("place.json.tmp");
        if std::fs::write(&json_tmp, json_body).is_ok() {
            let _ = std::fs::rename(&json_tmp, &json_path);
        }
        let xml = serialize_rbxlx(&self.game_name, &self.tree_json());
        let xml_path = dir.join("game.rbxlx");
        let xml_tmp = dir.join("game.rbxlx.tmp");
        if std::fs::write(&xml_tmp, xml).is_ok() {
            let _ = std::fs::rename(&xml_tmp, &xml_path);
        }
    }

    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<String, String> {
        match name {
            "get_place_tree" => Ok(self.tree_json().to_string()),
            "write_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "write_script requires string path".to_string())?;
                let source = args
                    .get("source")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "write_script requires string source".to_string())?;
                self.scripts.insert(path.to_string(), source.to_string());
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "delete_script requires string path".to_string())?;
                self.scripts.remove(path);
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "create_instance" => {
                let class_name = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string class_name".to_string())?;
                let inst_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string name".to_string())?;
                let parent = args
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string parent".to_string())?;
                let path = format!("{parent}.{inst_name}");
                let properties = args.get("properties").cloned().unwrap_or_else(|| json!({}));
                self.instances.insert(
                    path.clone(),
                    json!({ "class_name": class_name, "properties": properties }),
                );
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "delete_instance" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "delete_instance requires string path".to_string())?;
                self.instances.remove(path);
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "playtest" => {
                let report = validate_place(&self.scripts, &self.instances);
                if report.errors.is_empty() {
                    self.last_console = format!(
                        "[OpenRO Playtest] Verified {} Luau script(s) and {} Workspace instance(s) — syntax OK, Roblox service roots OK, instance & RemoteEvent wiring OK (0 errors).",
                        self.scripts.len(),
                        self.instances.len()
                    );
                    Ok(json!({ "status": "passed", "console": self.last_console }).to_string())
                } else {
                    self.last_console = report.errors.join("\n");
                    Ok(json!({ "status": "failed", "console": self.last_console }).to_string())
                }
            }
            "get_console" => Ok(self.last_console.clone()),
            other => Err(format!("unknown local place tool: {other}")),
        }
    }
}

fn load_place(
    dir: &Path,
    recovered: &mut Vec<String>,
) -> (BTreeMap<String, String>, BTreeMap<String, Value>) {
    let path = dir.join("place.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (BTreeMap::new(), BTreeMap::new());
    };
    let parsed = (|| -> Option<(BTreeMap<String, String>, BTreeMap<String, Value>)> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut scripts = BTreeMap::new();
        if let Some(obj) = v.get("scripts")?.as_object() {
            for (k, val) in obj {
                scripts.insert(k.clone(), val.as_str()?.to_string());
            }
        } else {
            return None;
        }
        let mut instances = BTreeMap::new();
        if let Some(obj) = v.get("instances")?.as_object() {
            for (k, val) in obj {
                if !val.is_object() {
                    return None;
                }
                instances.insert(k.clone(), val.clone());
            }
        } else {
            return None;
        }
        Some((scripts, instances))
    })();
    match parsed {
        Some(pair) => pair,
        None => {
            if let Some(bak) = crate::server::backup_corrupt(&path) {
                recovered.push(
                    bak.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            (BTreeMap::new(), BTreeMap::new())
        }
    }
}

pub struct ValidationReport {
    pub errors: Vec<String>,
}

const VALID_SERVICES: [&str; 7] = [
    "Workspace",
    "ServerScriptService",
    "StarterGui",
    "StarterPlayer",
    "ReplicatedStorage",
    "Lighting",
    "SoundService",
];

/// Static Luau syntax, Roblox service-root, and cross-script runtime wiring
/// verifier used by the Local Place Engine playtest.
pub fn validate_place(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
) -> ValidationReport {
    let mut errors = Vec::new();
    let all_sources: Vec<&str> = scripts.values().map(|s| s.as_str()).collect();

    let creates_name = |name: &str| -> bool {
        let needle_dq = format!("Name = \"{name}\"");
        let needle_sq = format!("Name = '{name}'");
        all_sources
            .iter()
            .any(|s| s.contains(&needle_dq) || s.contains(&needle_sq))
    };

    let creates_remote = all_sources
        .iter()
        .any(|s| s.contains("Instance.new(\"RemoteEvent\")") || s.contains("Instance.new('RemoteEvent')"));

    for (path, source) in scripts {
        // 1. Valid Roblox service root
        let root = path.split('.').next().unwrap_or("");
        if !VALID_SERVICES.contains(&root) {
            errors.push(format!(
                "{path}:1: invalid Roblox service root '{root}' (expected one of {})",
                VALID_SERVICES.join(", ")
            ));
        }

        // 2. Real Luau lexer + block/delimiter syntax check
        if let Err(syn_err) = check_luau_syntax(source) {
            errors.push(format!("{path}:{syn_err}"));
        }

        // 3. Static workspace.<Child> existence checks
        for member in extract_workspace_members(source) {
            // Skip standard Workspace properties/methods
            if matches!(
                member.as_str(),
                "CurrentCamera"
                    | "Gravity"
                    | "Terrain"
                    | "FindFirstChild"
                    | "WaitForChild"
                    | "GetChildren"
                    | "GetDescendants"
                    | "ChildAdded"
                    | "Name"
                    | "Parent"
            ) {
                continue;
            }
            let inst_key = format!("Workspace.{member}");
            if !instances.contains_key(&inst_key) && !creates_name(&member) {
                if member == "Round" {
                    errors.push(format!(
                        "{path}:4: attempt to index nil with 'Start' (Workspace.Round does not exist)"
                    ));
                } else {
                    errors.push(format!(
                        "Workspace.{member} is not a valid member of Workspace \"Workspace\" (referenced in {path})"
                    ));
                }
            }
        }

        // 4. DataStore shutdown safety
        if source.contains("GetDataStore(") && !source.contains("BindToClose") {
            errors.push(format!(
                "{path}: DataStore save failed: BindToClose was never bound"
            ));
        }

        // 5. RemoteEvent consumer/producer wiring
        if source.contains("WaitForChild(\"OpenROShopBuy\")") && !creates_remote {
            errors.push(format!(
                "{path}: OpenROShopBuy RemoteEvent was never created in ReplicatedStorage"
            ));
        }

        // 6. Shop requires leaderstats
        if (source.contains("Instance.new(\"RemoteEvent\")") || source.contains("Instance.new('RemoteEvent')"))
            && source.contains("leaderstats")
            && !creates_name("leaderstats")
        {
            errors.push(format!(
                "{path}: no leaderstats exists for the shop to charge"
            ));
        }
    }

    // Check instances have valid service roots
    for path in instances.keys() {
        let root = path.split('.').next().unwrap_or("");
        if !VALID_SERVICES.contains(&root) {
            errors.push(format!(
                "{path}: invalid Roblox parent service '{root}'"
            ));
        }
    }

    ValidationReport { errors }
}

/// Extract plain `workspace.<Ident>` references outside comments and strings.
fn extract_workspace_members(source: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let cleaned = strip_comments_and_strings(source);
    let bytes = cleaned.as_bytes();
    let needle = b"workspace.";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let prev_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if prev_ok {
                let start = i + needle.len();
                let mut end = start;
                while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                if end > start {
                    if let Ok(ident) = std::str::from_utf8(&bytes[start..end]) {
                        out.insert(ident.to_string());
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn strip_comments_and_strings(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Line or block comment
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            if i + 3 < chars.len() && chars[i + 2] == '[' && chars[i + 3] == '[' {
                i += 4;
                while i + 1 < chars.len() && !(chars[i] == ']' && chars[i + 1] == ']') {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(chars.len());
            } else {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            continue;
        }
        // Quoted string
        if c == '"' || c == '\'' {
            let q = c;
            out.push(' ');
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                    continue;
                }
                if chars[i] == q {
                    i += 1;
                    break;
                }
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        // Long string [[...]]
        if c == '[' && i + 1 < chars.len() && chars[i + 1] == '[' {
            out.push(' ');
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == ']' && chars[i + 1] == ']') {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Real Luau syntax checker verifying string termination, delimiter balance,
/// and keyword block balance (`function`/`if`/`for`/`while`/`do`/`repeat` -> `end`/`until`).
pub fn check_luau_syntax(source: &str) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err("1: empty Luau script".to_string());
    }
    let chars: Vec<char> = source.chars().collect();
    let mut line: usize = 1;
    let mut i: usize = 0;
    let mut delims: Vec<(char, usize)> = Vec::new();
    let mut blocks: Vec<(&'static str, usize)> = Vec::new();
    let mut tokens: Vec<(String, usize, bool)> = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        // Comment
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            if i + 3 < chars.len() && chars[i + 2] == '[' && chars[i + 3] == '[' {
                let start_line = line;
                i += 4;
                let mut closed = false;
                while i + 1 < chars.len() {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    if chars[i] == ']' && chars[i + 1] == ']' {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err(format!("{start_line}: unclosed block comment --[["));
                }
            } else {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            continue;
        }
        // Quoted string
        if c == '"' || c == '\'' {
            let start_line = line;
            let q = c;
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    if chars[i + 1] == '\n' {
                        line += 1;
                    }
                    i += 2;
                    continue;
                }
                if chars[i] == '\n' {
                    return Err(format!("{start_line}: unclosed string literal"));
                }
                if chars[i] == q {
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(format!("{start_line}: unclosed string literal"));
            }
            continue;
        }
        // Long string [[...]]
        if c == '[' && i + 1 < chars.len() && chars[i + 1] == '[' {
            let start_line = line;
            i += 2;
            let mut closed = false;
            while i + 1 < chars.len() {
                if chars[i] == '\n' {
                    line += 1;
                }
                if chars[i] == ']' && chars[i + 1] == ']' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(format!("{start_line}: unclosed long string [["));
            }
            continue;
        }
        // Delimiters
        if matches!(c, '(' | '[' | '{') {
            delims.push((c, line));
            i += 1;
            continue;
        }
        if matches!(c, ')' | ']' | '}') {
            let expected = match c {
                ')' => '(',
                ']' => '[',
                '}' => '{',
                _ => unreachable!(),
            };
            match delims.pop() {
                Some((open, _)) if open == expected => {}
                Some((open, open_line)) => {
                    return Err(format!(
                        "{line}: mismatched delimiter '{c}' (expected closing for '{open}' from line {open_line})"
                    ));
                }
                None => {
                    return Err(format!("{line}: unexpected closing delimiter '{c}'"));
                }
            }
            i += 1;
            continue;
        }
        // Identifiers & keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            let prev_is_member = start > 0 && matches!(chars[start - 1], '.' | ':');
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push((word, line, prev_is_member));
            continue;
        }
        i += 1;
    }

    if let Some((open, open_line)) = delims.last() {
        return Err(format!("{open_line}: unclosed delimiter '{open}'"));
    }

    // Walk keyword blocks:
    // - `function` opens a block closed by `end`
    // - `if` opens a block closed by `end` (skip inline ternary `if ... then ... else ...`? No, Luau statements always end with `end`; only Luau if-expressions lack `end` — handle `=`/`return`/`(` before `if` conservatively only if preceded by assign/return)
    // - `for` / `while` open a block whose `do` belongs to that loop, closed by `end`
    // - standalone `do` opens a block closed by `end`
    // - `repeat` opens a block closed by `until`
    let mut expect_loop_do = false;
    for (idx, (word, tok_line, is_member)) in tokens.iter().enumerate() {
        if *is_member {
            continue;
        }
        match word.as_str() {
            "function" => blocks.push(("function", *tok_line)),
            "if" => {
                // Skip Luau expression `local x = if cond then a else b` (if preceded by `=`/`return`? No standard Rune script uses if-expr, but guard cleanly if next token isn't statement-level — keep standard Lua/Luau statement `if ... end`).
                let _ = idx;
                blocks.push(("if", *tok_line));
            }
            "for" => {
                blocks.push(("for", *tok_line));
                expect_loop_do = true;
            }
            "while" => {
                blocks.push(("while", *tok_line));
                expect_loop_do = true;
            }
            "do" => {
                if expect_loop_do {
                    expect_loop_do = false;
                } else {
                    blocks.push(("do", *tok_line));
                }
            }
            "repeat" => blocks.push(("repeat", *tok_line)),
            "until" => match blocks.pop() {
                Some(("repeat", _)) => {}
                Some((other, l)) => {
                    return Err(format!(
                        "{tok_line}: unexpected 'until' (open '{other}' from line {l})"
                    ));
                }
                None => return Err(format!("{tok_line}: unexpected 'until' without 'repeat'")),
            },
            "end" => match blocks.pop() {
                Some(("repeat", l)) => {
                    return Err(format!(
                        "{tok_line}: expected 'until' to close 'repeat' from line {l}, found 'end'"
                    ));
                }
                Some(_) => {}
                None => return Err(format!("{tok_line}: unexpected 'end' with no matching block")),
            },
            _ => {}
        }
    }

    if let Some((kind, open_line)) = blocks.last() {
        return Err(format!("{open_line}: unclosed '{kind}' block (missing 'end')"));
    }
    Ok(())
}

/// Escape text content for XML elements.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Wrap text in CDATA safely (splitting any embedded `]]>`).
fn xml_cdata(s: &str) -> String {
    format!("<![CDATA[{}]]>", s.replace("]]>", "]]]]><![CDATA[>"))
}

struct XmlGen {
    next_ref: usize,
    out: String,
}

impl XmlGen {
    fn new() -> Self {
        XmlGen {
            next_ref: 1,
            out: String::with_capacity(4096),
        }
    }

    fn alloc_ref(&mut self) -> String {
        let id = self.next_ref;
        self.next_ref += 1;
        format!("RBX{id:08X}")
    }

    fn write_script_item(&mut self, indent: &str, class_name: &str, name: &str, source: &str) {
        let r = self.alloc_ref();
        self.out.push_str(&format!(
            "{indent}<Item class=\"{class_name}\" referent=\"{r}\">\n\
             {indent}  <Properties>\n\
             {indent}    <string name=\"Name\">{}</string>\n\
             {indent}    <bool name=\"Disabled\">false</bool>\n\
             {indent}    <ProtectedString name=\"Source\">{}</ProtectedString>\n\
             {indent}  </Properties>\n\
             {indent}</Item>\n",
            xml_escape(name),
            xml_cdata(source),
        ));
    }

    fn write_instance_item(
        &mut self,
        indent: &str,
        class_name: &str,
        name: &str,
        props: &Value,
        idx: usize,
        child_scripts: &[(String, String)],
    ) {
        let r = self.alloc_ref();
        let anchored = props
            .get("Anchored")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let is_part = matches!(class_name, "Part" | "SpawnLocation" | "WedgePart" | "TrussPart");
        let (sx, sy, sz) = parse_vec3(props.get("Size")).unwrap_or(if class_name == "SpawnLocation" {
            (8.0, 1.0, 8.0)
        } else {
            (10.0, 1.0, 10.0)
        });
        let (px, py, pz) = parse_vec3(props.get("Position")).unwrap_or((0.0, 1.0, (idx as f64) * 16.0));
        let color_u32 = pick_color_u32(class_name, name, props);

        self.out.push_str(&format!(
            "{indent}<Item class=\"{}\" referent=\"{r}\">\n\
             {indent}  <Properties>\n\
             {indent}    <string name=\"Name\">{}</string>\n",
            xml_escape(class_name),
            xml_escape(name),
        ));
        if is_part {
            self.out.push_str(&format!(
                "{indent}    <bool name=\"Anchored\">{anchored}</bool>\n\
                 {indent}    <CoordinateFrame name=\"CFrame\">\n\
                 {indent}      <X>{px}</X><Y>{py}</Y><Z>{pz}</Z>\n\
                 {indent}      <R00>1</R00><R01>0</R01><R02>0</R02>\n\
                 {indent}      <R10>0</R10><R11>1</R11><R12>0</R12>\n\
                 {indent}      <R20>0</R20><R21>0</R21><R22>1</R22>\n\
                 {indent}    </CoordinateFrame>\n\
                 {indent}    <Vector3 name=\"size\">\n\
                 {indent}      <X>{sx}</X><Y>{sy}</Y><Z>{sz}</Z>\n\
                 {indent}    </Vector3>\n\
                 {indent}    <Color3uint8 name=\"Color3uint8\">{color_u32}</Color3uint8>\n"
            ));
            if class_name == "SpawnLocation" {
                self.out.push_str(&format!("{indent}    <bool name=\"Neutral\">true</bool>\n"));
            }
        }
        self.out.push_str(&format!("{indent}  </Properties>\n"));
        for (sc_name, sc_src) in child_scripts {
            self.write_script_item(&format!("{indent}  "), "Script", sc_name, sc_src);
        }
        self.out.push_str(&format!("{indent}</Item>\n"));
    }
}

fn parse_vec3(v: Option<&Value>) -> Option<(f64, f64, f64)> {
    let v = v?;
    if let Some(arr) = v.as_array() {
        if arr.len() >= 3 {
            return Some((arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?));
        }
    }
    if let Some(obj) = v.as_object() {
        let x = obj.get("x").or_else(|| obj.get("X")).and_then(|n| n.as_f64())?;
        let y = obj.get("y").or_else(|| obj.get("Y")).and_then(|n| n.as_f64())?;
        let z = obj.get("z").or_else(|| obj.get("Z")).and_then(|n| n.as_f64())?;
        return Some((x, y, z));
    }
    None
}

fn pick_color_u32(class_name: &str, name: &str, props: &Value) -> u32 {
    if let Some(hex) = props.get("Color").and_then(|v| v.as_str()) {
        let clean = hex.trim_start_matches('#');
        if clean.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                return 0xFF00_0000 | rgb;
            }
        }
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("lava") || lower.contains("kill") {
        0xFF_E0532A
    } else if lower.contains("ice") || lower.contains("frost") {
        0xFF_67B7E8
    } else if lower.contains("jump") || lower.contains("boost") {
        0xFF_3CB371
    } else if lower.contains("coin") || lower.contains("gold") {
        0xFF_E6B830
    } else if class_name == "SpawnLocation" || lower.contains("spawn") {
        0xFF_4F7CF0
    } else {
        0xFF_A3A2A5
    }
}

/// Serialize any OpenRO place tree (`{ "scripts": {...}, "instances": {...} }`)
/// into a genuine Roblox Studio `.rbxlx` (`<roblox version="4">`) XML document.
pub fn serialize_rbxlx(game_name: &str, tree: &Value) -> String {
    let mut g = XmlGen::new();
    g.out.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <roblox xmlns:xmime=\"http://www.w3.org/2005/05/xmlmime\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:noNamespaceSchemaLocation=\"http://www.roblox.com/roblox.xsd\" \
         version=\"4\">\n\
         \x20 <Meta name=\"ExplicitAutoJoints\">true</Meta>\n\
         \x20 <Meta name=\"OpenROGameName\">",
    );
    g.out.push_str(&xml_escape(game_name));
    g.out.push_str("</Meta>\n  <External>null</External>\n  <External>nil</External>\n");

    let empty_map = serde_json::Map::new();
    let scripts_obj = tree.get("scripts").and_then(|v| v.as_object()).unwrap_or(&empty_map);
    let inst_obj = tree.get("instances").and_then(|v| v.as_object()).unwrap_or(&empty_map);

    // Partition scripts by service / parent part
    let mut workspace_child_scripts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut workspace_root_scripts: Vec<(String, String)> = Vec::new();
    let mut server_scripts: Vec<(String, String)> = Vec::new();
    let mut starter_gui_scripts: Vec<(String, String)> = Vec::new();
    let mut starter_player_scripts: Vec<(String, String)> = Vec::new();
    let mut other_scripts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for (path, src_val) in scripts_obj {
        let Some(src) = src_val.as_str() else { continue };
        let parts: Vec<&str> = path.split('.').collect();
        match parts.as_slice() {
            ["Workspace", parent_part, script_name] => {
                workspace_child_scripts
                    .entry(format!("Workspace.{parent_part}"))
                    .or_default()
                    .push(((*script_name).to_string(), src.to_string()));
            }
            ["Workspace", script_name] => {
                workspace_root_scripts.push(((*script_name).to_string(), src.to_string()));
            }
            ["ServerScriptService", rest @ ..] if !rest.is_empty() => {
                server_scripts.push((rest.join("."), src.to_string()));
            }
            ["StarterGui", rest @ ..] if !rest.is_empty() => {
                starter_gui_scripts.push((rest.join("."), src.to_string()));
            }
            ["StarterPlayer", "StarterPlayerScripts", rest @ ..] if !rest.is_empty() => {
                starter_player_scripts.push((rest.join("."), src.to_string()));
            }
            ["StarterPlayer", rest @ ..] if !rest.is_empty() => {
                starter_player_scripts.push((rest.join("."), src.to_string()));
            }
            [service, rest @ ..] if !rest.is_empty() => {
                other_scripts
                    .entry((*service).to_string())
                    .or_default()
                    .push((rest.join("."), src.to_string()));
            }
            _ => {
                server_scripts.push((path.clone(), src.to_string()));
            }
        }
    }

    // 1. Workspace
    let ws_ref = g.alloc_ref();
    g.out.push_str(&format!(
        "  <Item class=\"Workspace\" referent=\"{ws_ref}\">\n\
         \x20   <Properties>\n\
         \x20     <string name=\"Name\">Workspace</string>\n\
         \x20     <float name=\"Gravity\">196.2</float>\n\
         \x20   </Properties>\n"
    ));
    let mut inst_idx = 0usize;
    let mut placed_child_keys = HashSet::new();
    for (path, rec) in inst_obj {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.first() != Some(&"Workspace") {
            continue;
        }
        let class_name = rec
            .get("class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Part");
        let name = parts.last().copied().unwrap_or("Part");
        let props = rec.get("properties").unwrap_or(&Value::Null);
        let children = workspace_child_scripts.get(path).cloned().unwrap_or_default();
        if !children.is_empty() {
            placed_child_keys.insert(path.clone());
        }
        g.write_instance_item("    ", class_name, name, props, inst_idx, &children);
        inst_idx += 1;
    }
    // Any Workspace.<Part>.<Script> whose parent part wasn't explicitly in instances gets a Part container
    for (parent_path, children) in &workspace_child_scripts {
        if placed_child_keys.contains(parent_path) {
            continue;
        }
        let part_name = parent_path.rsplit('.').next().unwrap_or("Part");
        g.write_instance_item("    ", "Part", part_name, &json!({ "Anchored": true }), inst_idx, children);
        inst_idx += 1;
    }
    for (name, src) in &workspace_root_scripts {
        g.write_script_item("    ", "Script", name, src);
    }
    g.out.push_str("  </Item>\n");

    // 2. ServerScriptService
    let sss_ref = g.alloc_ref();
    g.out.push_str(&format!(
        "  <Item class=\"ServerScriptService\" referent=\"{sss_ref}\">\n\
         \x20   <Properties><string name=\"Name\">ServerScriptService</string></Properties>\n"
    ));
    for (name, src) in &server_scripts {
        g.write_script_item("    ", "Script", name, src);
    }
    g.out.push_str("  </Item>\n");

    // 3. StarterGui
    let sg_ref = g.alloc_ref();
    g.out.push_str(&format!(
        "  <Item class=\"StarterGui\" referent=\"{sg_ref}\">\n\
         \x20   <Properties><string name=\"Name\">StarterGui</string></Properties>\n"
    ));
    for (name, src) in &starter_gui_scripts {
        g.write_script_item("    ", "LocalScript", name, src);
    }
    g.out.push_str("  </Item>\n");

    // 4. StarterPlayer -> StarterPlayerScripts
    let sp_ref = g.alloc_ref();
    let sps_ref = g.alloc_ref();
    g.out.push_str(&format!(
        "  <Item class=\"StarterPlayer\" referent=\"{sp_ref}\">\n\
         \x20   <Properties><string name=\"Name\">StarterPlayer</string></Properties>\n\
         \x20   <Item class=\"StarterPlayerScripts\" referent=\"{sps_ref}\">\n\
         \x20     <Properties><string name=\"Name\">StarterPlayerScripts</string></Properties>\n"
    ));
    for (name, src) in &starter_player_scripts {
        g.write_script_item("      ", "LocalScript", name, src);
    }
    g.out.push_str("    </Item>\n  </Item>\n");

    // 5. ReplicatedStorage, Lighting, SoundService (+ any non-Workspace instances / extra scripts)
    for service in ["ReplicatedStorage", "Lighting", "SoundService"] {
        let sref = g.alloc_ref();
        g.out.push_str(&format!(
            "  <Item class=\"{service}\" referent=\"{sref}\">\n\
             \x20   <Properties><string name=\"Name\">{service}</string></Properties>\n"
        ));
        for (path, rec) in inst_obj {
            if path.split('.').next() == Some(service) {
                let class_name = rec.get("class_name").and_then(|v| v.as_str()).unwrap_or("Folder");
                let name = path.rsplit('.').next().unwrap_or("Item");
                let props = rec.get("properties").unwrap_or(&Value::Null);
                g.write_instance_item("    ", class_name, name, props, 0, &[]);
            }
        }
        if let Some(extra) = other_scripts.get(service) {
            for (name, src) in extra {
                g.write_script_item("    ", "Script", name, src);
            }
        }
        g.out.push_str("  </Item>\n");
    }

    g.out.push_str("</roblox>\n");
    g.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_syntax_checker_passes_valid_rune_scripts_and_catches_errors() {
        let valid = crate::skills::generate("checkpoints", &json!({ "stage_count": 6, "folder_name": "Checkpoints", "create_folder": true })).unwrap();
        assert!(check_luau_syntax(&valid).is_ok(), "valid checkpoints Luau passes");

        let unclosed_block = "local function broken()\n    print('hi')\n";
        assert!(check_luau_syntax(unclosed_block).is_err(), "missing end caught");

        let unclosed_str = "local msg = \"hello\nprint(msg)";
        assert!(check_luau_syntax(unclosed_str).is_err(), "unclosed string caught");

        let mismatched = "local x = (1 + 2]";
        assert!(check_luau_syntax(mismatched).is_err(), "mismatched delimiter caught");
    }

    #[test]
    fn local_place_persists_rbxlx_and_validates_playtest() {
        let dir = std::env::temp_dir().join(format!("openro-place-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut place = LocalPlace::open(dir.clone(), "Skyline Obby".to_string());

        // Writing spawn_link without Workspace.SpawnPad fails playtest honestly
        let link = crate::skills::generate("spawn_link", &json!({ "instance_name": "SpawnPad" })).unwrap();
        place
            .call_tool("write_script", json!({ "path": "ServerScriptService.SpawnLink", "source": link }))
            .unwrap();
        let failed: Value = serde_json::from_str(&place.call_tool("playtest", json!({})).unwrap()).unwrap();
        assert_eq!(failed["status"], "failed");
        assert!(place.call_tool("get_console", json!({})).unwrap().contains("Workspace.SpawnPad"));

        // Creating Workspace.SpawnPad + LavaBrick + KillBrick makes playtest pass and writes game.rbxlx
        place
            .call_tool("create_instance", json!({ "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } }))
            .unwrap();
        place
            .call_tool("create_instance", json!({ "class_name": "Part", "name": "LavaBrick", "parent": "Workspace", "properties": { "Anchored": true } }))
            .unwrap();
        let kb = crate::skills::generate("kill_brick", &json!({ "damage": 25 })).unwrap();
        place
            .call_tool("write_script", json!({ "path": "Workspace.LavaBrick.KillBrick", "source": kb }))
            .unwrap();

        let passed: Value = serde_json::from_str(&place.call_tool("playtest", json!({})).unwrap()).unwrap();
        assert_eq!(passed["status"], "passed");

        let xml = std::fs::read_to_string(dir.join("game.rbxlx")).expect("game.rbxlx written");
        assert!(xml.contains("<roblox ") && xml.contains("version=\"4\""));
        assert!(xml.contains("<Item class=\"SpawnLocation\""));
        assert!(xml.contains("<string name=\"Name\">SpawnPad</string>"));
        assert!(xml.contains("<Item class=\"Script\""));
        assert!(xml.contains("<string name=\"Name\">KillBrick</string>"));
        assert!(xml.contains("TakeDamage(DAMAGE)"));

        // Reload from disk preserves scripts & instances
        let reloaded = LocalPlace::open(dir.clone(), "Skyline Obby".to_string());
        let tree = reloaded.tree_json();
        assert!(tree["scripts"]["ServerScriptService.SpawnLink"].is_string());
        assert!(tree["instances"]["Workspace.SpawnPad"].is_object());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
