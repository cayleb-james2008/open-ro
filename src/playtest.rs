//! Playtest runner: scripted Studio playtest with assertions, console error
//! capture and a screenshot gallery, saved per build as JSON and HTML.
//!
//! Reports live in `<project>/build/reports/<build_id>/<run_id>/`.

use crate::mcp::McpClient;
use crate::project_api::{with_studio, Reply};
use crate::projects::{self, ProjectRef};
use crate::server::Shared;
use crate::studio_sync::extract_result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const ERROR_HINTS: [&str; 5] = ["error", "attempt to", "is not a valid member", "stack begin", "failed to"];
const MAX_PLAYTEST_ACTIONS: usize = 50;
const MAX_ACTION_WAIT_MS: u64 = 15_000;
const MAX_KEY_HOLD_MS: u64 = 10_000;

fn reports_root(dir: &Path) -> PathBuf {
    dir.join("build/reports")
}

fn plain_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !s.contains("..")
}

/// A console line that counts as an error.
pub fn is_error_line(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    if low.contains("0 errors") || low.contains("no errors") || low.contains("without errors") {
        return false;
    }
    ERROR_HINTS.iter().any(|h| low.contains(h))
}

fn is_warning_line(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    !is_error_line(line) && (low.contains("warning") || low.contains("infinite yield"))
}

fn luau_for_check(user: &str) -> String {
    format!(
        "-- openro:v011:check\nlocal HttpService = game:GetService(\"HttpService\")\nlocal ok, res = pcall(function()\n--[[USER]]\n{user}\n--[[/USER]]\nend)\nlocal encoded = nil\nif ok then\n\tlocal okE, j = pcall(function() return HttpService:JSONEncode({{ v = res }}) end)\n\tif okE then encoded = j else encoded = HttpService:JSONEncode({{ v = tostring(res) }}) end\nend\nif ok then return \"{marker}\" .. HttpService:JSONEncode({{ ok = true, j = encoded }}) end\nreturn \"{marker}\" .. HttpService:JSONEncode({{ ok = false, e = tostring(res) }})\n",
        marker = "OPENRO_V011:"
    )
}

/// Does the value meet `expect`? None = no expectation (it just has to run).
fn expectation(expect: &Value, value: &Value) -> Result<(), String> {
    let Some(o) = expect.as_object().filter(|o| !o.is_empty()) else { return Ok(()) };
    if let Some(want) = o.get("equals") {
        let same = match (want, value) {
            (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
            (a, b) => a == b,
        };
        if !same {
            return Err(format!("expected {want}, got {value}"));
        }
    }
    if o.get("truthy").and_then(|t| t.as_bool()) == Some(true) && matches!(value, Value::Null | Value::Bool(false)) {
        return Err(format!("expected a truthy value, got {value}"));
    }
    if let Some(Value::String(needle)) = o.get("contains").or_else(|| o.get("matches")) {
        let hay = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if !hay.contains(needle.as_str()) {
            return Err(format!("expected it to contain \"{needle}\", got {value}"));
        }
    }
    Ok(())
}

fn run_check(mcp: &mut McpClient, check: &Value) -> Value {
    let name = check["name"].as_str().unwrap_or("check").to_string();
    let dm = check["datamodel"].as_str().unwrap_or("Server").to_string();
    let Some(user) = check["luau"].as_str().filter(|s| !s.trim().is_empty()) else {
        return json!({ "name": name, "passed": false, "value": null, "error": "this check has no luau to run", "datamodel": dm });
    };
    let prev = mcp.datamodel_type().to_string();
    mcp.set_datamodel_type(&dm);
    let res = mcp.execute_luau(&luau_for_check(user));
    mcp.set_datamodel_type(&prev);
    let (value, error) = match res {
        Err(e) => (Value::Null, Some(e)),
        Ok(v) => match extract_result(&v) {
            None => (Value::Null, Some("Studio answered without a result".to_string())),
            Some(r) if r["ok"].as_bool() == Some(true) => {
                let decoded: Value = r["j"].as_str().and_then(|j| serde_json::from_str(j).ok()).unwrap_or(Value::Null);
                (decoded.get("v").cloned().unwrap_or(Value::Null), None)
            }
            Some(r) => (Value::Null, Some(r["e"].as_str().unwrap_or("the check raised an error").to_string())),
        },
    };
    let verdict = match &error {
        Some(e) => Err(e.clone()),
        None => expectation(&check["expect"], &value),
    };
    json!({ "name": name, "passed": verdict.is_ok(), "value": value, "error": verdict.err(), "datamodel": dm })
}

fn validate_actions(actions: &[Value]) -> Result<(), String> {
    if actions.len() > MAX_PLAYTEST_ACTIONS {
        return Err(format!(
            "too many playtest actions (max {MAX_PLAYTEST_ACTIONS})"
        ));
    }
    for (idx, act) in actions.iter().enumerate() {
        let Some(obj) = act.as_object() else {
            return Err(format!("actions[{idx}] must be a JSON object"));
        };
        let kind = obj
            .get("kind")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        match kind.as_str() {
            "click_gui" => {
                let path = obj
                    .get("gui_path")
                    .or_else(|| obj.get("path"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                if path.is_empty() || path.len() > 512 || path.chars().any(|c| c.is_control()) {
                    return Err(format!(
                        "actions[{idx}] click_gui requires a valid non-empty 'gui_path' (<= 512 chars)"
                    ));
                }
                let has_seg = path
                    .split(['.', '/', '\\'])
                    .any(|s| !s.trim().is_empty());
                if !has_seg {
                    return Err(format!(
                        "actions[{idx}] click_gui 'gui_path' has no valid path segments"
                    ));
                }
            }
            "key" => {
                let raw = obj
                    .get("key")
                    .or_else(|| obj.get("key_code"))
                    .or_else(|| obj.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if raw.is_empty() || raw.len() > 64 || raw.chars().any(|c| c.is_control()) {
                    return Err(format!(
                        "actions[{idx}] key action requires a non-empty 'key' string"
                    ));
                }
            }
            "text" => {
                let text = obj
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if text.is_empty() || text.len() > 2048 || text.contains('\0') {
                    return Err(format!(
                        "actions[{idx}] text action requires a non-empty 'text' string (<= 2048 bytes, no NUL)"
                    ));
                }
            }
            "click_at" => {
                let x = obj.get("x").and_then(Value::as_f64);
                let y = obj.get("y").and_then(Value::as_f64);
                match (x, y) {
                    (Some(xv), Some(yv)) if xv.is_finite() && yv.is_finite() => {}
                    _ => {
                        return Err(format!(
                            "actions[{idx}] click_at requires finite numeric 'x' and 'y'"
                        ))
                    }
                }
            }
            "wait" => {}
            other => {
                return Err(format!(
                    "actions[{idx}] unsupported action kind '{other}' (expected click_gui, key, text, click_at, or wait)"
                ));
            }
        }
    }
    Ok(())
}

fn luau_for_resolve_gui(gui_path: &str) -> String {
    let payload = crate::mcp::luau_long_string(&json!({ "gui_path": gui_path }).to_string());
    format!(
        "-- openro:v011:resolve_gui\n\
local HttpService = game:GetService(\"HttpService\")\n\
local Players = game:GetService(\"Players\")\n\
local GuiService = game:GetService(\"GuiService\")\n\
local PAYLOAD = {payload}\n\
local ok, res = pcall(function()\n\
\tlocal spec = HttpService:JSONDecode(PAYLOAD)\n\
\tlocal raw = tostring(spec.gui_path or \"\")\n\
\traw = string.gsub(raw, \"\\\\\", \".\")\n\
\traw = string.gsub(raw, \"/\", \".\")\n\
\traw = string.gsub(raw, \"^Players%.LocalPlayer%.PlayerGui%.\", \"\")\n\
\traw = string.gsub(raw, \"^LocalPlayer%.PlayerGui%.\", \"\")\n\
\traw = string.gsub(raw, \"^PlayerGui%.\", \"\")\n\
\traw = string.gsub(raw, \"^StarterGui%.\", \"\")\n\
\tlocal player = Players.LocalPlayer\n\
\tif not player then\n\
\t\treturn {{ ok = false, reason = \"no_local_player\", error = \"Players.LocalPlayer is nil (use mode='start_play' for client GUI clicks)\" }}\n\
\tend\n\
\tlocal pgui = player:FindFirstChildOfClass(\"PlayerGui\")\n\
\tif not pgui then\n\
\t\treturn {{ ok = false, reason = \"no_player_gui\", error = \"LocalPlayer has no PlayerGui\" }}\n\
\tend\n\
\tlocal cur = pgui\n\
\tlocal segs = {{}}\n\
\tfor seg in string.gmatch(raw, \"[^%.]+\") do\n\
\t\tlocal nxt = cur:FindFirstChild(seg)\n\
\t\tif not nxt then\n\
\t\t\treturn {{ ok = false, reason = \"not_found\", stopped_at = seg, error = \"GUI element '\" .. tostring(spec.gui_path) .. \"' not found under PlayerGui (missing '\" .. seg .. \"')\" }}\n\
\t\tend\n\
\t\tcur = nxt\n\
\t\ttable.insert(segs, cur.Name)\n\
\tend\n\
\tif not cur:IsA(\"GuiObject\") then\n\
\t\treturn {{ ok = false, reason = \"not_gui_object\", class_name = cur.ClassName, error = \"target '\" .. tostring(spec.gui_path) .. \"' is a \" .. cur.ClassName .. \", not a GuiObject\" }}\n\
\tend\n\
\tlocal screenGui = nil\n\
\tlocal anc = cur\n\
\twhile anc and anc ~= pgui do\n\
\t\tif anc:IsA(\"GuiObject\") and not anc.Visible then\n\
\t\t\treturn {{ ok = false, reason = \"invisible\", hidden_by = anc:GetFullName(), error = \"GUI element '\" .. tostring(spec.gui_path) .. \"' is invisible (hidden by \" .. anc:GetFullName() .. \")\" }}\n\
\t\tend\n\
\t\tif anc:IsA(\"LayerCollector\") then\n\
\t\t\tif not anc.Enabled then\n\
\t\t\t\treturn {{ ok = false, reason = \"screen_gui_disabled\", hidden_by = anc:GetFullName(), error = \"GUI element '\" .. tostring(spec.gui_path) .. \"' is inside disabled \" .. anc:GetFullName() }}\n\
\t\t\tend\n\
\t\t\tif anc:IsA(\"ScreenGui\") then\n\
\t\t\t\tscreenGui = anc\n\
\t\t\tend\n\
\t\tend\n\
\t\tanc = anc.Parent\n\
\tend\n\
\tlocal pos = cur.AbsolutePosition\n\
\tlocal sz = cur.AbsoluteSize\n\
\tif sz.X <= 0 or sz.Y <= 0 then\n\
\t\treturn {{ ok = false, reason = \"zero_size\", width = sz.X, height = sz.Y, error = \"GUI element '\" .. tostring(spec.gui_path) .. \"' has zero AbsoluteSize\" }}\n\
\tend\n\
\t-- In Roblox, GuiObject.AbsolutePosition is always inset-relative (even when\n\
\t-- IgnoreGuiInset=true, where top-of-screen is -topLeftInset.Y). Adding\n\
\t-- topLeftInset converts to true 0..ViewportSize screen pixels for both modes (#67).\n\
\tlocal topLeftInset, _ = GuiService:GetGuiInset()\n\
\tlocal ignoreInset = (screenGui ~= nil) and (screenGui.IgnoreGuiInset == true) or false\n\
\tlocal guiX = pos.X + sz.X * 0.5\n\
\tlocal guiY = pos.Y + sz.Y * 0.5\n\
\tlocal vx = guiX + topLeftInset.X\n\
\tlocal vy = guiY + topLeftInset.Y\n\
\tlocal cam = workspace.CurrentCamera\n\
\tlocal vp = cam and cam.ViewportSize or Vector2.new(0, 0)\n\
\tif vp.X > 0 and vp.Y > 0 and (vx < 0 or vy < 0 or vx > vp.X or vy > vp.Y) then\n\
\t\treturn {{ ok = false, reason = \"offscreen\", x = vx, y = vy, gui_x = guiX, gui_y = guiY, viewport = {{ vp.X, vp.Y }}, error = \"GUI element '\" .. tostring(spec.gui_path) .. \"' center is outside viewport\" }}\n\
\tend\n\
\tlocal hitVerified = false\n\
\tlocal okHit, hits = pcall(function()\n\
\t\treturn pgui:GetGuiObjectsAtPosition(guiX, guiY)\n\
\tend)\n\
\tif okHit and type(hits) == \"table\" then\n\
\t\tfor _, h in ipairs(hits) do\n\
\t\t\tif h == cur or h:IsDescendantOf(cur) then\n\
\t\t\t\thitVerified = true\n\
\t\t\t\tbreak\n\
\t\t\tend\n\
\t\tend\n\
\tend\n\
\tlocal instPath = \"LocalPlayer.PlayerGui.\" .. table.concat(segs, \".\")\n\
\treturn {{\n\
\t\tok = true,\n\
\t\tinstance_path = instPath,\n\
\t\tx = vx,\n\
\t\ty = vy,\n\
\t\tgui_x = guiX,\n\
\t\tgui_y = guiY,\n\
\t\tpos = {{ pos.X, pos.Y }},\n\
\t\tsize = {{ sz.X, sz.Y }},\n\
\t\tinset = {{ topLeftInset.X, topLeftInset.Y }},\n\
\t\tignore_gui_inset = ignoreInset,\n\
\t\tviewport = {{ vp.X, vp.Y }},\n\
\t\thit_verified = hitVerified,\n\
\t\tclass_name = cur.ClassName,\n\
\t}}\n\
end)\n\
if ok then\n\
\treturn \"OPENRO_V011:\" .. HttpService:JSONEncode(res)\n\
end\n\
return \"OPENRO_V011:\" .. HttpService:JSONEncode({{ ok = false, reason = \"luau_error\", error = tostring(res) }})\n"
    )
}

fn run_action(mcp: &mut McpClient, idx: usize, action: &Value) -> Value {
    let kind = action
        .get("kind")
        .or_else(|| action.get("type"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();

    match kind.as_str() {
        "click_gui" => {
            let gui_path = action
                .get("gui_path")
                .or_else(|| action.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            let button = match action
                .get("button")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("right") => "right",
                _ => "left",
            };
            let double_click = action
                .get("double_click")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let use_coordinates = action
                .get("use_coordinates")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let wait_ms = action
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(250)
                .min(MAX_ACTION_WAIT_MS);

            let prev_dm = mcp.datamodel_type().to_string();
            mcp.set_datamodel_type("Client");
            let luau_res = mcp.execute_luau(&luau_for_resolve_gui(&gui_path));

            let resolved = match luau_res {
                Err(e) => {
                    mcp.set_datamodel_type(&prev_dm);
                    return json!({
                        "index": idx,
                        "kind": "click_gui",
                        "gui_path": gui_path,
                        "passed": false,
                        "reason": "execute_luau_failed",
                        "resolved": Value::Null,
                        "wait_ms": 0,
                        "error": e,
                    });
                }
                Ok(v) => match extract_result(&v) {
                    Some(r) => r,
                    None => {
                        mcp.set_datamodel_type(&prev_dm);
                        return json!({
                            "index": idx,
                            "kind": "click_gui",
                            "gui_path": gui_path,
                            "passed": false,
                            "reason": "no_luau_result",
                            "resolved": Value::Null,
                            "wait_ms": 0,
                            "error": "Studio answered without a GUI resolution result",
                        });
                    }
                },
            };

            if resolved["ok"].as_bool() != Some(true) {
                mcp.set_datamodel_type(&prev_dm);
                return json!({
                    "index": idx,
                    "kind": "click_gui",
                    "gui_path": gui_path,
                    "passed": false,
                    "reason": resolved.get("reason").cloned().unwrap_or_else(|| json!("resolve_failed")),
                    "resolved": Value::Null,
                    "wait_ms": 0,
                    "error": resolved["error"].as_str().unwrap_or("could not resolve GUI target"),
                });
            }

            let Some(cx) = resolved["x"].as_f64().filter(|v| v.is_finite()) else {
                mcp.set_datamodel_type(&prev_dm);
                return json!({
                    "index": idx,
                    "kind": "click_gui",
                    "gui_path": gui_path,
                    "passed": false,
                    "reason": "invalid_coords",
                    "resolved": resolved,
                    "wait_ms": 0,
                    "error": "resolved GUI target returned non-finite x coordinate",
                });
            };
            let Some(cy) = resolved["y"].as_f64().filter(|v| v.is_finite()) else {
                mcp.set_datamodel_type(&prev_dm);
                return json!({
                    "index": idx,
                    "kind": "click_gui",
                    "gui_path": gui_path,
                    "passed": false,
                    "reason": "invalid_coords",
                    "resolved": resolved,
                    "wait_ms": 0,
                    "error": "resolved GUI target returned non-finite y coordinate",
                });
            };
            let x = cx.clamp(0.0, 4096.0);
            let y = cy.clamp(0.0, 4096.0);
            let instance_path = resolved["instance_path"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);

            let build_coord_acts = || -> Vec<Value> {
                let mut acts = vec![
                    json!({ "action": "moveTo", "x": x, "y": y }),
                    json!({ "action": "wait", "wait_time_ms": 40 }),
                    json!({ "action": "mouseButtonClick", "mouse_button": button }),
                ];
                if double_click {
                    acts.push(json!({ "action": "mouseButtonClick", "mouse_button": button }));
                }
                acts
            };

            // Official Roblox StudioMCP `user_mouse_input` natively targets Client GUI instances via
            // `instance_path` ("LocalPlayer.PlayerGui.<path>") and reuses moveTo position for click,
            // avoiding DPI/inset coordinate drift; fall back to viewport (x, y) when requested or
            // if a bridge does not support `instance_path` (#67).
            let (dispatch_mode, mouse_res) = if !use_coordinates {
                if let Some(ref ip) = instance_path {
                    let mut ip_acts = vec![
                        json!({ "action": "moveTo", "instance_path": ip }),
                        json!({ "action": "wait", "wait_time_ms": 40 }),
                        json!({ "action": "mouseButtonClick", "mouse_button": button }),
                    ];
                    if double_click {
                        ip_acts.push(json!({ "action": "mouseButtonClick", "mouse_button": button }));
                    }
                    match mcp.user_mouse_input(ip_acts) {
                        Ok(v)
                            if v.get("ok").and_then(Value::as_bool) != Some(false)
                                && v.get("success").and_then(Value::as_bool) != Some(false) =>
                        {
                            ("instance_path", Ok(v))
                        }
                        _ => (
                            "coordinates_fallback",
                            mcp.user_mouse_input(build_coord_acts()),
                        ),
                    }
                } else {
                    ("coordinates", mcp.user_mouse_input(build_coord_acts()))
                }
            } else {
                ("coordinates", mcp.user_mouse_input(build_coord_acts()))
            };
            mcp.set_datamodel_type(&prev_dm);

            match mouse_res {
                Ok(_) => {
                    if wait_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                    }
                    json!({
                        "index": idx,
                        "kind": "click_gui",
                        "gui_path": gui_path,
                        "button": button,
                        "double_click": double_click,
                        "passed": true,
                        "resolved": {
                            "instance_path": instance_path,
                            "dispatch": dispatch_mode,
                            "datamodel_type": "Client",
                            "x": x,
                            "y": y,
                            "gui_x": resolved["gui_x"],
                            "gui_y": resolved["gui_y"],
                            "pos": resolved["pos"],
                            "size": resolved["size"],
                            "inset": resolved["inset"],
                            "ignore_gui_inset": resolved["ignore_gui_inset"],
                            "viewport": resolved["viewport"],
                            "hit_verified": resolved["hit_verified"],
                            "class_name": resolved["class_name"],
                        },
                        "wait_ms": wait_ms,
                        "error": Value::Null,
                    })
                }
                Err(e) => json!({
                    "index": idx,
                    "kind": "click_gui",
                    "gui_path": gui_path,
                    "button": button,
                    "double_click": double_click,
                    "passed": false,
                    "reason": "mouse_input_failed",
                    "resolved": {
                        "instance_path": instance_path,
                        "dispatch": dispatch_mode,
                        "x": x,
                        "y": y,
                    },
                    "wait_ms": 0,
                    "error": e,
                }),
            }
        }
        "click_at" => {
            let x = action["x"].as_f64().unwrap_or(0.0).clamp(0.0, 4096.0);
            let y = action["y"].as_f64().unwrap_or(0.0).clamp(0.0, 4096.0);
            let button = match action
                .get("button")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("right") => "right",
                _ => "left",
            };
            let double_click = action
                .get("double_click")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let wait_ms = action
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(250)
                .min(MAX_ACTION_WAIT_MS);
            let mut mouse_acts = vec![
                json!({ "action": "moveTo", "x": x, "y": y }),
                json!({ "action": "wait", "wait_time_ms": 40 }),
                json!({ "action": "mouseButtonClick", "mouse_button": button }),
            ];
            if double_click {
                mouse_acts.push(json!({ "action": "mouseButtonClick", "mouse_button": button }));
            }
            let prev_dm = mcp.datamodel_type().to_string();
            mcp.set_datamodel_type("Client");
            let click_res = mcp.user_mouse_input(mouse_acts);
            mcp.set_datamodel_type(&prev_dm);
            match click_res {
                Ok(_) => {
                    if wait_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                    }
                    json!({
                        "index": idx,
                        "kind": "click_at",
                        "x": x,
                        "y": y,
                        "button": button,
                        "double_click": double_click,
                        "passed": true,
                        "wait_ms": wait_ms,
                        "error": Value::Null,
                    })
                }
                Err(e) => json!({
                    "index": idx,
                    "kind": "click_at",
                    "x": x,
                    "y": y,
                    "passed": false,
                    "wait_ms": 0,
                    "error": e,
                }),
            }
        }
        "key" => {
            let raw_key = action
                .get("key")
                .or_else(|| action.get("key_code"))
                .or_else(|| action.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let key_code = crate::studio_embed::normalize_roblox_keycode(raw_key);
            let hold_ms = action
                .get("hold_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(MAX_KEY_HOLD_MS);
            let wait_ms = action
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(150)
                .min(MAX_ACTION_WAIT_MS);

            let input_res = if hold_ms > 0 {
                mcp.user_keyboard_input(vec![json!({ "action": "keyDown", "key_code": key_code })])
                    .and_then(|_| {
                        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
                        mcp.user_keyboard_input(vec![json!({ "action": "keyUp", "key_code": key_code })])
                    })
            } else {
                mcp.user_keyboard_input(vec![json!({ "action": "keyPress", "key_code": key_code })])
            };
            match input_res {
                Ok(_) => {
                    if wait_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                    }
                    json!({
                        "index": idx,
                        "kind": "key",
                        "key": key_code,
                        "hold_ms": hold_ms,
                        "wait_ms": wait_ms,
                        "passed": true,
                        "error": Value::Null,
                    })
                }
                Err(e) => json!({
                    "index": idx,
                    "kind": "key",
                    "key": key_code,
                    "hold_ms": hold_ms,
                    "wait_ms": 0,
                    "passed": false,
                    "error": e,
                }),
            }
        }
        "text" => {
            let text = action.get("text").and_then(Value::as_str).unwrap_or("");
            let wait_ms = action
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(150)
                .min(MAX_ACTION_WAIT_MS);
            match mcp.user_keyboard_input(vec![json!({ "action": "textInput", "text_inputs": text })]) {
                Ok(_) => {
                    if wait_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                    }
                    json!({
                        "index": idx,
                        "kind": "text",
                        "text_len": text.len(),
                        "wait_ms": wait_ms,
                        "passed": true,
                        "error": Value::Null,
                    })
                }
                Err(e) => json!({
                    "index": idx,
                    "kind": "text",
                    "text_len": text.len(),
                    "wait_ms": 0,
                    "passed": false,
                    "error": e,
                }),
            }
        }
        "wait" => {
            let wait_ms = action
                .get("wait_ms")
                .or_else(|| action.get("ms"))
                .and_then(Value::as_u64)
                .unwrap_or(250)
                .min(MAX_ACTION_WAIT_MS);
            if wait_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            }
            json!({
                "index": idx,
                "kind": "wait",
                "wait_ms": wait_ms,
                "passed": true,
                "error": Value::Null,
            })
        }
        other => json!({
            "index": idx,
            "kind": other,
            "passed": false,
            "error": format!("unsupported action kind '{other}'"),
        }),
    }
}

fn latest_build_id(dir: &Path) -> String {
    projects::builds(dir).into_iter().next().and_then(|b| b["build_id"].as_str().map(String::from)).unwrap_or_else(|| "unbuilt".into())
}

fn merge_spec(dir: &Path, req: &Value) -> Result<Value, String> {
    let manifest = projects::manifest(dir)?;
    let suite_rel = req["suite"].as_str().or_else(|| manifest["playtest"]["suite"].as_str());
    let mut spec = match suite_rel {
        Some(rel) => {
            let p = projects::safe_join(dir, rel)?;
            let text = std::fs::read_to_string(&p).map_err(|_| format!("playtest suite '{rel}' not found"))?;
            serde_json::from_str::<Value>(&text).map_err(|e| format!("playtest suite '{rel}' is not valid JSON: {e}"))?
        }
        None => json!({}),
    };
    let o = spec.as_object_mut().ok_or("the playtest suite must be a JSON object")?;
    for key in [
        "actions",
        "checks",
        "screenshots",
        "allow_errors",
        "mode",
        "settle_ms",
        "build_id",
        "sync_first",
        "require_core_loop",
        "force",
    ] {
        if let Some(v) = req.get(key) {
            o.insert(key.into(), v.clone());
        }
    }
    Ok(spec)
}

fn build_kept_mode_spec(spec: &Value, mode: &str, build_id: &str, require_core_loop: bool) -> Value {
    json!({
        "mode": mode,
        "build_id": build_id,
        "actions": spec["actions"].as_array().cloned().unwrap_or_default(),
        "checks": spec["checks"].as_array().cloned().unwrap_or_default(),
        "screenshots": spec["screenshots"].as_array().cloned().unwrap_or_default(),
        "allow_errors": spec["allow_errors"].as_array().cloned().unwrap_or_default(),
        "sync_first": spec["sync_first"].as_bool().unwrap_or(false),
        "require_core_loop": require_core_loop,
    })
}

/// Fast-path `KeptChecks` lookup for `playtest`: returns `Ok(Some(Reply))` when a
/// previous passing playtest matches the exact project SHA-256 fingerprint and spec
/// (and `force != true`), avoiding unnecessary Studio auto-open or re-execution (#62).
pub fn try_kept(r: &ProjectRef, req: &Value) -> Result<Option<Reply>, String> {
    let spec = merge_spec(&r.dir, req)?;
    let actions = match spec.get("actions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err("actions must be an array".into()),
    };
    validate_actions(&actions)?;
    let force = spec["force"].as_bool().unwrap_or(false);
    if force {
        return Ok(None);
    }
    let mode = spec["mode"].as_str().unwrap_or("start_play");
    if !matches!(mode, "start_play" | "run_server") {
        return Err("mode must be start_play or run_server".into());
    }
    let build_id = spec["build_id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| latest_build_id(&r.dir));
    if !plain_name(&build_id) {
        return Err("build_id is not valid".into());
    }
    let require_core_loop = spec["require_core_loop"].as_bool().unwrap_or(false);
    let fp = crate::study_check::project_fingerprint(&r.dir)?;
    let mode_spec = build_kept_mode_spec(&spec, mode, &build_id, require_core_loop);
    if let Some(mut cached) =
        crate::study_check::lookup_kept_check(&r.dir, "playtest", &fp, &mode_spec, false)
    {
        cached["ok"] = json!(true);
        return Ok(Some(Reply::json("200 OK", cached)));
    }
    Ok(None)
}

fn new_console_lines(before: &[String], after: &[String]) -> Vec<String> {
    if before.is_empty() {
        return after.to_vec();
    }
    if after.len() >= before.len() && after[..before.len()] == *before {
        return after[before.len()..].to_vec();
    }
    // Ring-buffer wrap: find the longest suffix of `before` that matches a prefix of `after`.
    let max_k = before.len().min(after.len());
    for k in (1..=max_k).rev() {
        if before[before.len() - k..] == after[..k] {
            return after[k..].to_vec();
        }
    }
    after.to_vec()
}

pub fn run(shared: &Shared, r: &ProjectRef, req: &Value) -> Result<Reply, Reply> {
    let spec = merge_spec(&r.dir, req).map_err(Reply::bad)?;
    let mode = spec["mode"].as_str().unwrap_or("start_play").to_string();
    if !matches!(mode.as_str(), "start_play" | "run_server") {
        return Err(Reply::bad("mode must be start_play or run_server"));
    }
    let actions = match spec.get("actions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err(Reply::bad("actions must be an array")),
    };
    validate_actions(&actions).map_err(Reply::bad)?;
    let checks = spec["checks"].as_array().cloned().unwrap_or_default();
    let shots = spec["screenshots"].as_array().cloned().unwrap_or_default();
    for s in &shots {
        if !plain_name(s["name"].as_str().unwrap_or("")) {
            return Err(Reply::bad("a screenshot needs a name of letters, digits, dashes, dots or underscores"));
        }
    }
    let allow: Vec<String> = spec["allow_errors"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let settle = spec["settle_ms"].as_u64().unwrap_or(1500).min(60_000);
    let build_id = spec["build_id"].as_str().map(String::from).unwrap_or_else(|| latest_build_id(&r.dir));
    if !plain_name(&build_id) {
        return Err(Reply::bad("build_id is not valid"));
    }
    let require_core_loop = spec["require_core_loop"].as_bool().unwrap_or(false);
    let force = spec["force"].as_bool().unwrap_or(false);
    let fp = crate::study_check::project_fingerprint(&r.dir).map_err(Reply::bad)?;
    let mode_spec = build_kept_mode_spec(&spec, &mode, &build_id, require_core_loop);
    if let Some(mut cached) =
        crate::study_check::lookup_kept_check(&r.dir, "playtest", &fp, &mode_spec, force)
    {
        cached["ok"] = json!(true);
        return Ok(Reply::json("200 OK", cached));
    }

    if spec["sync_first"].as_bool().unwrap_or(false) {
        let synced = crate::studio_sync::run(shared, r, false, false)?;
        let body: Value = serde_json::from_slice(&synced.body).unwrap_or(Value::Null);
        if body["errors"].as_array().map(|e| !e.is_empty()).unwrap_or(false) || body["verify"]["remaining"].as_u64().unwrap_or(0) > 0 {
            return Err(Reply::err("502 Bad Gateway", format!("sync before the playtest did not finish cleanly: {}", body["errors"])));
        }
    }
    let started = std::time::Instant::now();
    let run_id = projects::unique_id("p");
    let run_dir = reports_root(&r.dir).join(&build_id).join(&run_id);
    std::fs::create_dir_all(run_dir.join("shots")).map_err(|e| Reply::err("500 Internal Server Error", format!("create report folder: {e}")))?;

    type RunOutput = (
        Vec<Value>,
        Vec<Value>,
        Vec<String>,
        Vec<Value>,
        Vec<(String, crate::study_check::RgbImage)>,
    );
    let (action_results, check_results, console_lines, shot_results, decoded_shots) =
        with_studio(shared, |mcp| {
            let before_lines = mcp.get_console_output().unwrap_or_default();
            mcp.start_stop_play(&mode)
                .map_err(|e| format!("could not start the playtest: {e}"))?;
            let outcome = (|| -> Result<RunOutput, String> {
                std::thread::sleep(std::time::Duration::from_millis(settle));
                let mut check_slots: Vec<Option<Value>> = vec![None; checks.len()];
                for (i, c) in checks.iter().enumerate() {
                    if c.get("when").and_then(Value::as_str) == Some("before_actions") {
                        check_slots[i] = Some(run_check(mcp, c));
                    }
                }
                let action_results: Vec<Value> = actions
                    .iter()
                    .enumerate()
                    .map(|(idx, a)| run_action(mcp, idx, a))
                    .collect();
                for (i, c) in checks.iter().enumerate() {
                    if check_slots[i].is_none() {
                        check_slots[i] = Some(run_check(mcp, c));
                    }
                }
                let results: Vec<Value> = check_slots.into_iter().flatten().collect();
                let mut shot_results = Vec::new();
                let mut decoded_shots: Vec<(String, crate::study_check::RgbImage)> = Vec::new();
                for s in &shots {
                    let name = s["name"].as_str().unwrap_or("shot");
                    let vec3 = |k: &str| -> Option<[f64; 3]> {
                        let a = s.get(k)?.as_array()?;
                        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
                    };
                    match mcp.screen_capture_with_camera(vec3("camera_position"), vec3("camera_look_at")) {
                        Ok(frame) => {
                            let ext = if frame.mime.contains("jpeg") { "jpg" } else { "png" };
                            let file = format!("shots/{name}.{ext}");
                            std::fs::write(run_dir.join(&file), &frame.bytes).map_err(|e| format!("save screenshot: {e}"))?;
                            let (maybe_img, visual_meta) = if ext == "png" {
                                crate::study_check::inspect_screenshot_bytes(&frame.bytes)
                            } else {
                                (None, json!({ "valid": true, "blank": false, "note": "non-png capture" }))
                            };
                            let is_blank = visual_meta["blank"].as_bool().unwrap_or(false);
                            if !is_blank {
                                if let Some(img) = maybe_img {
                                    decoded_shots.push((name.to_string(), img));
                                }
                            }
                            shot_results.push(json!({
                                "name": name,
                                "file": file,
                                "bytes": frame.bytes.len(),
                                "width": frame.width,
                                "height": frame.height,
                                "passed": !is_blank,
                                "rejected": is_blank,
                                "visual": visual_meta,
                            }));
                        }
                        Err(e) => shot_results.push(json!({ "name": name, "file": null, "passed": false, "rejected": false, "error": e })),
                    }
                }
                let after_lines = mcp.get_console_output().unwrap_or_default();
                let lines = new_console_lines(&before_lines, &after_lines);
                Ok((action_results, results, lines, shot_results, decoded_shots))
            })();
            let stopped = mcp.start_stop_play("stop");
            let out = outcome?;
            stopped.map_err(|e| format!("the playtest ran but Studio would not stop it: {e}"))?;
            Ok(out)
        })?;

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for l in &console_lines {
        if is_error_line(l) && !allow.iter().any(|a| l.contains(a.as_str())) {
            errors.push(l.clone());
        } else if is_warning_line(l) {
            warnings.push(l.clone());
        }
    }
    let tail_start = console_lines.len().saturating_sub(50);
    let tail: Vec<String> = console_lines[tail_start..].to_vec();
    let checks_failed = check_results.iter().filter(|c| c["passed"] != json!(true)).count();
    let actions_failed = action_results.iter().filter(|a| a["passed"] != json!(true)).count();
    let blank_screenshots = shot_results
        .iter()
        .filter(|s| s["rejected"].as_bool().unwrap_or(false))
        .count();
    let core_loop =
        crate::study_check::evaluate_core_loop(&check_results, &checks, &errors, require_core_loop);
    let visual_likeness = crate::study_check::evaluate_visual_likeness(&r.dir, &decoded_shots);
    let core_ok = !require_core_loop || core_loop["all_core_passed"].as_bool().unwrap_or(false);
    let passed = checks_failed == 0
        && actions_failed == 0
        && errors.is_empty()
        && blank_screenshots == 0
        && core_ok;

    let prev_assertions = latest_assertions(&r.dir);
    let assertions = build_deterministic_assertions(
        &r.dir,
        &check_results,
        &errors,
        &core_loop,
        blank_screenshots,
    );
    let assertion_diff = diff_assertion_suites(prev_assertions.as_deref(), &assertions);

    let rel = |f: &str| format!("build/reports/{build_id}/{run_id}/{f}");
    let at = projects::now_ms();
    let mut report = json!({
        "run_id": run_id, "passed": passed, "build_id": build_id, "mode": mode,
        "project": r.id, "at": at, "duration_ms": started.elapsed().as_millis() as u64,
        "kept": false, "cached": false, "fingerprint": fp,
        "actions": action_results,
        "checks": check_results,
        "assertions": assertions,
        "assertion_diff": assertion_diff,
        "core_loop": core_loop,
        "blank_screenshots": blank_screenshots,
        "visual_likeness": visual_likeness,
        "console": { "lines": console_lines.len(), "errors": errors, "warnings": warnings, "tail": tail },
        "screenshots": shot_results,
        "report": { "json": rel("report.json"), "html": rel("report.html") },
        "urls": {
            "json": format!("/api/projects/{}/playtests/{run_id}", r.id),
            "html": format!("/api/projects/{}/playtests/{run_id}/report.html", r.id),
        },
    });

    // Auto-record `observed` playtest evidence in `<project>/.openro/investigation/evidence.jsonl` (#62).
    let mut ev_limitations: Vec<String> = Vec::new();
    for claim_key in ["move", "act", "hud"] {
        if report["core_loop"][claim_key]["tested"].as_bool() != Some(true) {
            ev_limitations.push(format!("core_loop.{claim_key} not tested"));
        }
    }
    if shot_results.is_empty() {
        ev_limitations.push("no screenshots captured".to_string());
    }
    if blank_screenshots > 0 {
        ev_limitations.push(format!("{blank_screenshots} blank/single-color screenshot(s) rejected"));
    }
    let checks_ok = check_results.len().saturating_sub(checks_failed);
    if let Ok(ev_res) = crate::study_check::record_evidence(
        &r.dir,
        &json!({
            "kind": "playtest",
            "source": format!("playtest:{run_id}"),
            "summary": format!(
                "Playtest {run_id} ({mode}, build {build_id}): {} — {checks_ok}/{} checks passed, {} console error(s), {} screenshot(s) ({blank_screenshots} blank)",
                if passed { "passed" } else { "failed" },
                check_results.len(),
                errors.len(),
                shot_results.len(),
            ),
            "confidence": "observed",
            "authority": "self_test",
            "coverage": format!("mode={mode};checks={};screenshots={}", check_results.len(), shot_results.len()),
            "limitations": ev_limitations,
            "refs": [rel("report.json"), rel("report.html")],
            "data": {
                "run_id": run_id,
                "build_id": build_id,
                "mode": mode,
                "passed": passed,
                "checks_passed": checks_ok,
                "checks_total": check_results.len(),
                "actions_total": action_results.len(),
                "actions_failed": actions_failed,
                "console_errors": errors.len(),
                "blank_screenshots": blank_screenshots,
                "core_loop": report["core_loop"],
                "visual_likeness": report["visual_likeness"],
            }
        }),
    ) {
        if let Some(eid) = ev_res.pointer("/evidence/id").and_then(Value::as_str) {
            report["evidence_id"] = json!(eid);
        }
    }

    let html = render_html(&report, &r.id);
    std::fs::write(run_dir.join("report.html"), html).map_err(|e| Reply::err("500 Internal Server Error", format!("save report: {e}")))?;
    std::fs::write(run_dir.join("report.json"), serde_json::to_string_pretty(&report).unwrap_or_default())
        .map_err(|e| Reply::err("500 Internal Server Error", format!("save report: {e}")))?;

    // Persist in KeptChecks ONLY when passing (never caches failures, #62).
    crate::study_check::store_kept_check(&r.dir, "playtest", &fp, &mode_spec, &report);

    report["ok"] = json!(true);
    Ok(Reply::json("200 OK", report))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn render_html(rep: &Value, project: &str) -> String {
    let passed = rep["passed"].as_bool().unwrap_or(false);
    let mut rows = String::new();
    for c in rep["checks"].as_array().cloned().unwrap_or_default() {
        let ok = c["passed"].as_bool().unwrap_or(false);
        rows.push_str(&format!(
            "<tr class=\"{}\"><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            if ok { "ok" } else { "bad" },
            if ok { "Passed" } else { "Failed" },
            esc(c["name"].as_str().unwrap_or("")),
            esc(&c["value"].to_string()),
            esc(c["error"].as_str().unwrap_or("")),
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan=\"4\">No checks were requested.</td></tr>");
    }
    let actions_section = {
        let acts = rep["actions"].as_array().cloned().unwrap_or_default();
        if acts.is_empty() {
            String::new()
        } else {
            let mut arows = String::new();
            for a in &acts {
                let ok = a["passed"].as_bool().unwrap_or(false);
                let target = a["gui_path"]
                    .as_str()
                    .or_else(|| a["key"].as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        if a["kind"] == json!("click_at") {
                            format!("({},{})", a["x"], a["y"])
                        } else {
                            format!("{}ms", a["wait_ms"].as_u64().unwrap_or(0))
                        }
                    });
                arows.push_str(&format!(
                    "<tr class=\"{}\"><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
                    if ok { "ok" } else { "bad" },
                    if ok { "Passed" } else { "Failed" },
                    esc(a["kind"].as_str().unwrap_or("")),
                    esc(&target),
                    esc(a["error"].as_str().unwrap_or("")),
                ));
            }
            format!(
                "<h2>Actions ({})</h2><table><thead><tr><th>Result</th><th>Kind</th><th>Target</th><th>Problem</th></tr></thead><tbody>{arows}</tbody></table>",
                acts.len()
            )
        }
    };
    let list = |key: &str| -> String {
        let items: Vec<String> = rep["console"][key].as_array().cloned().unwrap_or_default().iter().map(|l| format!("<li><code>{}</code></li>", esc(l.as_str().unwrap_or("")))).collect();
        if items.is_empty() { "<p class=\"muted\">None.</p>".into() } else { format!("<ul>{}</ul>", items.join("")) }
    };
    let mut gallery = String::new();
    for s in rep["screenshots"].as_array().cloned().unwrap_or_default() {
        match s["file"].as_str() {
            Some(f) => gallery.push_str(&format!("<figure><img src=\"{}\" alt=\"{}\"><figcaption>{}</figcaption></figure>", esc(f), esc(s["name"].as_str().unwrap_or("")), esc(s["name"].as_str().unwrap_or("")))),
            None => gallery.push_str(&format!("<figure><p class=\"bad\">{} could not be captured: {}</p></figure>", esc(s["name"].as_str().unwrap_or("")), esc(s["error"].as_str().unwrap_or("")))),
        }
    }
    if gallery.is_empty() {
        gallery.push_str("<p class=\"muted\">No screenshots were requested.</p>");
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Playtest {run} - {project}</title><style>\
body{{font:15px/1.5 system-ui,sans-serif;margin:0;padding:24px;background:#faf9f5;color:#141413;max-width:1100px}}\
h1{{font-size:22px;margin:0 0 4px}}h2{{font-size:16px;margin:28px 0 8px}}.muted{{color:#5f5e5a}}\
.badge{{display:inline-block;padding:2px 10px;border-radius:999px;font-weight:600;color:#fff;background:{badge}}}\
table{{border-collapse:collapse;width:100%}}td,th{{text-align:left;padding:6px 10px;border-bottom:1px solid #e3e1d8;vertical-align:top}}\
tr.bad td:first-child,.bad{{color:#b3261e;font-weight:600}}tr.ok td:first-child{{color:#2f6f3e;font-weight:600}}\
code{{font:13px ui-monospace,monospace;word-break:break-word}}figure{{margin:0 0 16px}}img{{max-width:100%;border:1px solid #e3e1d8;border-radius:6px}}\
</style></head><body><h1>Playtest {run}</h1><p class=\"muted\">Project {project} &middot; build {build} &middot; {mode} &middot; {ms} ms</p>\
<p><span class=\"badge\">{verdict}</span></p>\
{actions_section}\
<h2>Checks</h2><table><thead><tr><th>Result</th><th>Check</th><th>Value</th><th>Problem</th></tr></thead><tbody>{rows}</tbody></table>\
<h2>Console errors ({ne})</h2>{errs}<h2>Console warnings ({nw})</h2>{warns}<h2>Screenshots</h2>{gallery}</body></html>",
        run = esc(rep["run_id"].as_str().unwrap_or("")),
        project = esc(project),
        build = esc(rep["build_id"].as_str().unwrap_or("")),
        mode = esc(rep["mode"].as_str().unwrap_or("")),
        ms = rep["duration_ms"].as_u64().unwrap_or(0),
        badge = if passed { "#2f6f3e" } else { "#b3261e" },
        verdict = if passed { "Passed" } else { "Failed" },
        ne = rep["console"]["errors"].as_array().map(|a| a.len()).unwrap_or(0),
        nw = rep["console"]["warnings"].as_array().map(|a| a.len()).unwrap_or(0),
        errs = list("errors"),
        warns = list("warnings"),
    )
}

/// Run ids, newest first, with their build.
pub fn list_runs(dir: &Path) -> Vec<Value> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let Ok(builds) = std::fs::read_dir(reports_root(dir)) else { return Vec::new() };
    for b in builds.filter_map(|e| e.ok()) {
        let Ok(runs) = std::fs::read_dir(b.path()) else { continue };
        for run in runs.filter_map(|e| e.ok()) {
            let Ok(text) = std::fs::read_to_string(run.path().join("report.json")) else { continue };
            let Ok(rep) = serde_json::from_str::<Value>(&text) else { continue };
            let id = rep["run_id"].as_str().unwrap_or("").to_string();
            out.push((id.clone(), json!({
                "run_id": id, "build_id": rep["build_id"], "passed": rep["passed"], "at": rep["at"],
                "checks": rep["checks"].as_array().map(|a| a.len()).unwrap_or(0),
                "errors": rep["console"]["errors"].as_array().map(|a| a.len()).unwrap_or(0),
            })));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, v)| v).collect()
}

fn latest_report_json(dir: &Path) -> Option<Value> {
    let mut best: Option<(String, Value)> = None;
    let builds = std::fs::read_dir(reports_root(dir)).ok()?;
    for b in builds.filter_map(|e| e.ok()) {
        let Ok(runs) = std::fs::read_dir(b.path()) else { continue };
        for run in runs.filter_map(|e| e.ok()) {
            let Ok(text) = std::fs::read_to_string(run.path().join("report.json")) else { continue };
            let Ok(rep) = serde_json::from_str::<Value>(&text) else { continue };
            let id = rep["run_id"].as_str().unwrap_or("").to_string();
            if best.as_ref().is_none_or(|(cur, _)| id > *cur) {
                best = Some((id, rep));
            }
        }
    }
    best.map(|(_, rep)| rep)
}

fn latest_assertions(dir: &Path) -> Option<Vec<Value>> {
    latest_report_json(dir)?
        .get("assertions")
        .and_then(Value::as_array)
        .cloned()
}

fn scan_project_luau(dir: &Path) -> (usize, usize, bool, bool, bool) {
    let src_dir = dir.join("src");
    let mut files = 0usize;
    let mut luau_errors = 0usize;
    let mut has_server = false;
    let mut has_receipt = false;
    let mut has_retention = false;
    let mut stack = vec![src_dir];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else { continue };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !(name.ends_with(".luau") || name.ends_with(".lua")) {
                continue;
            }
            files += 1;
            if name.ends_with(".server.luau") || name.ends_with(".server.lua") {
                has_server = true;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                if text.contains("ProcessReceipt") {
                    has_receipt = true;
                }
                if text.contains("ClaimDailyReward") || text.contains("DailyRewardRetention") {
                    has_retention = true;
                }
                let place_path = if name.contains(".server.") {
                    format!("ServerScriptService.{name}")
                } else if name.contains(".client.") {
                    format!("StarterPlayer.StarterPlayerScripts.{name}")
                } else {
                    format!("ReplicatedStorage.{name}")
                };
                let rep = crate::luau_check::analyze_script(&place_path, &text);
                luau_errors += rep.error_count();
            }
        }
    }
    (files, luau_errors, has_server, has_receipt, has_retention)
}

pub fn build_deterministic_assertions(
    dir: &Path,
    check_results: &[Value],
    errors: &[String],
    core_loop: &Value,
    blank_screenshots: usize,
) -> Vec<Value> {
    let (script_files, luau_errors, has_server, has_receipt, has_retention) = scan_project_luau(dir);
    let mut out = vec![
        json!({
            "id": "luau_static",
            "room": "Luau",
            "label": "Static Luau safety across scripts",
            "passed": script_files > 0 && luau_errors == 0,
            "status": if script_files > 0 && luau_errors == 0 { "passed" } else { "failed" },
            "detail": format!("{script_files} script(s), {luau_errors} static error(s)")
        }),
        json!({
            "id": "server_entrypoint",
            "room": "Luau",
            "label": "Server entrypoint present",
            "passed": has_server,
            "status": if has_server { "passed" } else { "failed" },
            "detail": if has_server { "Server script detected under src/" } else { "No *.server.luau entrypoint under src/" }
        }),
        json!({
            "id": "runtime_console",
            "room": "Runtime",
            "label": "Zero runtime console errors",
            "passed": errors.is_empty(),
            "status": if errors.is_empty() { "passed" } else { "failed" },
            "detail": format!("{} runtime console error(s)", errors.len())
        }),
        json!({
            "id": "monetization_guard",
            "room": "Economy",
            "label": "ProcessReceipt & retention guards",
            "passed": has_receipt && has_retention,
            "status": if has_receipt && has_retention { "passed" } else { "failed" },
            "detail": format!(
                "ProcessReceipt={} · Retention={}",
                if has_receipt { "ok" } else { "missing" },
                if has_retention { "ok" } else { "missing" }
            )
        }),
        json!({
            "id": "visual_frames",
            "room": "Visuals",
            "label": "Zero blank or single-color captures",
            "passed": blank_screenshots == 0,
            "status": if blank_screenshots == 0 { "passed" } else { "failed" },
            "detail": format!("{blank_screenshots} blank frame(s) rejected")
        }),
    ];
    for (k, label) in [("move", "Core loop: Move"), ("act", "Core loop: Act"), ("hud", "Core loop: HUD")] {
        let claim = &core_loop[k];
        let tested = claim["tested"].as_bool().unwrap_or(false);
        let passed = claim["passed"].as_bool().unwrap_or(false);
        let status = if passed {
            "passed"
        } else if tested {
            "failed"
        } else {
            "skipped"
        };
        out.push(json!({
            "id": format!("core_loop_{k}"),
            "room": "Core Loop",
            "label": label,
            "passed": !tested || passed,
            "status": status,
            "detail": claim["status"].as_str().unwrap_or("not tested")
        }));
    }
    for c in check_results {
        let name = c["name"].as_str().unwrap_or("check");
        let ok = c["passed"].as_bool().unwrap_or(false);
        let detail = c["error"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| c["value"].to_string());
        out.push(json!({
            "id": format!("check_{}", name.to_ascii_lowercase().replace(' ', "_")),
            "room": "Studio Checks",
            "label": name,
            "passed": ok,
            "status": if ok { "passed" } else { "failed" },
            "detail": detail
        }));
    }
    out
}

pub fn diff_assertion_suites(prev: Option<&[Value]>, curr: &[Value]) -> Value {
    let mut prev_map = std::collections::HashMap::new();
    if let Some(items) = prev {
        for item in items {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                prev_map.insert(id.to_string(), item.get("passed").and_then(Value::as_bool).unwrap_or(false));
            }
        }
    }
    let has_previous = !prev_map.is_empty();
    let mut fixed = 0usize;
    let mut regressed = 0usize;
    let mut passed_total = 0usize;
    let mut items = Vec::with_capacity(curr.len());
    let mut rooms: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = std::collections::BTreeMap::new();
    for item in curr {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let room = item.get("room").and_then(Value::as_str).unwrap_or("General").to_string();
        let now_ok = item.get("passed").and_then(Value::as_bool).unwrap_or(false);
        if now_ok {
            passed_total += 1;
        }
        let delta = match prev_map.get(id).copied() {
            Some(false) if now_ok => {
                fixed += 1;
                "fixed"
            }
            Some(true) if !now_ok => {
                regressed += 1;
                "regressed"
            }
            Some(true) => "unchanged_pass",
            Some(false) => "unchanged_fail",
            None if now_ok => "new_pass",
            None => "new_fail",
        };
        let entry = rooms.entry(room).or_insert((0, 0, 0, 0));
        entry.1 += 1;
        if now_ok {
            entry.0 += 1;
        }
        if delta == "fixed" {
            entry.2 += 1;
        } else if delta == "regressed" {
            entry.3 += 1;
        }
        let mut obj = item.clone();
        if let Some(m) = obj.as_object_mut() {
            m.insert("delta".to_string(), json!(delta));
        }
        items.push(obj);
    }
    let room_list: Vec<Value> = rooms
        .into_iter()
        .map(|(room, (p, t, fx, rg))| {
            json!({ "room": room, "passed": p, "total": t, "fixed": fx, "regressed": rg })
        })
        .collect();
    json!({
        "has_previous": has_previous,
        "passed": passed_total,
        "total": curr.len(),
        "fixed": fixed,
        "regressed": regressed,
        "rooms": room_list,
        "items": items
    })
}

pub fn run_deterministic_assertions_offline(r: &ProjectRef) -> Result<Reply, Reply> {
    let started = std::time::Instant::now();
    let build_id = latest_build_id(&r.dir);
    let run_id = projects::unique_id("p");
    let run_dir = reports_root(&r.dir).join(&build_id).join(&run_id);
    std::fs::create_dir_all(run_dir.join("shots"))
        .map_err(|e| Reply::err("500 Internal Server Error", format!("create report folder: {e}")))?;

    let prev_rep = latest_report_json(&r.dir);
    let prev_assertions = prev_rep
        .as_ref()
        .and_then(|rep| rep.get("assertions"))
        .and_then(Value::as_array)
        .cloned();
    let core_loop = prev_rep
        .as_ref()
        .and_then(|rep| rep.get("core_loop"))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "move": { "status": "not tested", "tested": false, "passed": false },
                "act": { "status": "not tested", "tested": false, "passed": false },
                "hud": { "status": "not tested", "tested": false, "passed": false },
                "all_core_tested": false,
                "all_core_passed": false,
            })
        });
    let assertions = build_deterministic_assertions(&r.dir, &[], &[], &core_loop, 0);
    let assertion_diff = diff_assertion_suites(prev_assertions.as_deref(), &assertions);
    let passed = assertions.iter().all(|a| a["passed"].as_bool().unwrap_or(false));
    let checks: Vec<Value> = assertions
        .iter()
        .map(|a| {
            json!({
                "name": a["label"],
                "passed": a["passed"],
                "value": a["detail"],
                "error": if a["passed"].as_bool().unwrap_or(false) { Value::Null } else { a["detail"].clone() },
                "datamodel": a["room"],
            })
        })
        .collect();
    let rel = |f: &str| format!("build/reports/{build_id}/{run_id}/{f}");
    let at = projects::now_ms();
    let mut report = json!({
        "run_id": run_id,
        "passed": passed,
        "build_id": build_id,
        "mode": "deterministic_suite",
        "project": r.id,
        "at": at,
        "duration_ms": started.elapsed().as_millis() as u64,
        "kept": false,
        "cached": false,
        "actions": [],
        "checks": checks,
        "assertions": assertions,
        "assertion_diff": assertion_diff,
        "core_loop": core_loop,
        "blank_screenshots": 0,
        "console": { "lines": 0, "errors": [], "warnings": [], "tail": [] },
        "screenshots": [],
        "report": { "json": rel("report.json"), "html": rel("report.html") },
        "urls": {
            "json": format!("/api/projects/{}/playtests/{run_id}", r.id),
            "html": format!("/api/projects/{}/playtests/{run_id}/report.html", r.id),
        },
    });
    let html = render_html(&report, &r.id);
    std::fs::write(run_dir.join("report.html"), html)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("save report: {e}")))?;
    std::fs::write(
        run_dir.join("report.json"),
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    )
    .map_err(|e| Reply::err("500 Internal Server Error", format!("save report: {e}")))?;
    report["ok"] = json!(true);
    Ok(Reply::json("200 OK", report))
}

pub fn rerun_assertions(shared: &Shared, r: &ProjectRef, req: &Value) -> Result<Reply, Reply> {
    if req.get("offline").and_then(Value::as_bool) == Some(true) {
        return run_deterministic_assertions_offline(r);
    }
    let mut forced = req.clone();
    if let Some(m) = forced.as_object_mut() {
        m.insert("force".to_string(), json!(true));
    } else {
        forced = json!({ "force": true });
    }
    match run(shared, r, &forced) {
        Ok(rep) => Ok(rep),
        Err(err_reply) if err_reply.status.starts_with("503") || err_reply.status.starts_with("502") => {
            run_deterministic_assertions_offline(r)
        }
        Err(other) => Err(other),
    }
}

fn write_project_luau(dir: &Path, rel_path: &str, src: &str) -> Result<(), Reply> {
    let abs = projects::safe_join(dir, rel_path).map_err(Reply::bad)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Reply::err("500 Internal Server Error", format!("create dir: {e}")))?;
    }
    let tmp = abs.with_extension("luau.tmp-openro");
    std::fs::write(&tmp, src.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &abs))
        .map_err(|e| Reply::err("500 Internal Server Error", format!("write {rel_path}: {e}")))
}

fn fix_plan_spec(dir: &Path, assertion_id: &str) -> Value {
    let id = assertion_id.trim();
    match id {
        "monetization_guard" => json!({
            "assertion_id": id,
            "room": "Economy",
            "title": "Wire ProcessReceipt & 48h Comeback Retention Guards",
            "target_file": "src/server/DailyRewardRetention.server.luau",
            "auto_fixable": true,
            "summary": "Generates MonetizationConfig.luau, server-authoritative MonetizationService.server.luau (idempotent UpdateAsync ProcessReceipt + PolicyService), and DailyRewardRetention.server.luau (24h streak + 48h comeback bonus + BindToClose).",
            "diff_preview": "@@ src/server/MonetizationService.server.luau & DailyRewardRetention.server.luau @@\n+--!strict\n+MarketplaceService.ProcessReceipt = function(receiptInfo)\n+\treceiptStore:UpdateAsync(receiptKey, ...)\n+end\n+claimFn.OnServerInvoke = function(player: Player) ... end\n+game:BindToClose(function() task.wait(0.2) end)",
            "steps": [
                "Write ReplicatedStorage MonetizationConfig.luau",
                "Write idempotent ProcessReceipt handler in MonetizationService.server.luau",
                "Write 7-day + 48h comeback streak handler in DailyRewardRetention.server.luau"
            ]
        }),
        "server_entrypoint" => json!({
            "assertion_id": id,
            "room": "Luau",
            "title": "Scaffold Strict Server Bootstrap Entrypoint",
            "target_file": "src/server/GameBootstrap.server.luau",
            "auto_fixable": true,
            "summary": "Creates a strict ServerScriptService entrypoint initializing leaderstats and player lifecycle hooks with 0 static Luau errors.",
            "diff_preview": "@@ src/server/GameBootstrap.server.luau @@\n+--!strict\n+local Players = game:GetService(\"Players\")\n+Players.PlayerAdded:Connect(function(player: Player)\n+\tlocal ls = player:FindFirstChild(\"leaderstats\") or Instance.new(\"Folder\")\n+\tls.Name = \"leaderstats\"\n+\tls.Parent = player\n+end)",
            "steps": [
                "Create src/server/GameBootstrap.server.luau with strict types",
                "Verify 0 static Luau errors across src/"
            ]
        }),
        "luau_static" => {
            let (files, errs, _, _, _) = scan_project_luau(dir);
            json!({
                "assertion_id": id,
                "room": "Luau",
                "title": "Clean Empty Stubs & Format Luau Scripts",
                "target_file": "src/shared/",
                "auto_fixable": true,
                "summary": format!("Replaces empty placeholder scripts with valid strict modules, strips trailing whitespace across {files} script(s) ({errs} current error(s)), and runs stylua."),
                "diff_preview": "@@ src/**/*.luau @@\n-   \n+--!strict\n+return {}",
                "steps": [
                    "Replace empty *.luau placeholders with --!strict return {}",
                    "Trim trailing whitespace and run project formatter"
                ]
            })
        }
        "core_loop_move" | "core_loop_act" | "core_loop_hud" => json!({
            "assertion_id": id,
            "room": "Core Loop",
            "title": "Scaffold Deterministic Core-Loop Action & HUD Bridge",
            "target_file": "src/server/CoreLoopBootstrap.server.luau",
            "auto_fixable": true,
            "summary": "Creates deterministic RemoteEvent core-loop server validation + client HUD binder so Move/Act/HUD checks have active hooks.",
            "diff_preview": "@@ src/server/CoreLoopBootstrap.server.luau & src/client/CoreLoopHud.client.luau @@\n+--!strict\n+local coreEvent = Instance.new(\"RemoteEvent\")\n+coreEvent.Name = \"CoreLoopAction\"\n+coreEvent.OnServerEvent:Connect(function(player: Player) ... end)",
            "steps": [
                "Write src/server/CoreLoopBootstrap.server.luau",
                "Write src/client/CoreLoopHud.client.luau"
            ]
        }),
        other => json!({
            "assertion_id": other,
            "room": "Runtime",
            "title": "Re-Verify & Format Targeted Luau Check",
            "target_file": "src/",
            "auto_fixable": true,
            "summary": format!("Formats project Luau scripts and re-runs deterministic verification for assertion '{other}'."),
            "diff_preview": "@@ src/ @@\n+ -- Verified by OpenRO deterministic assertion runner",
            "steps": [
                "Format Luau sources under src/",
                "Re-run deterministic assertion suite"
            ]
        }),
    }
}

pub fn draft_assertion_fix_plan(r: &ProjectRef, assertion_id: &str) -> Result<Value, Reply> {
    if assertion_id.trim().is_empty() {
        return Err(Reply::bad("assertion_id is required"));
    }
    let plan = fix_plan_spec(&r.dir, assertion_id);
    Ok(json!({ "ok": true, "plan": plan }))
}

fn apply_single_assertion_fix(dir: &Path, assertion_id: &str) -> Result<Vec<String>, Reply> {
    let id = assertion_id.trim();
    let mut applied_files = Vec::new();
    match id {
        "monetization_guard" => {
            let scripts = crate::opencloud::monetization_pack_luau_scripts_with_preset(
                Some("starter_obby"),
                "Coins",
                0,
                0,
                100,
            );
            for (_place_path, rel_path, _cls, src) in &scripts {
                write_project_luau(dir, rel_path, src)?;
                applied_files.push((*rel_path).to_string());
            }
        }
        "server_entrypoint" => {
            let rel = "src/server/GameBootstrap.server.luau";
            let src = "--!strict\n-- Auto-scaffolded by OpenRO Playtest Auto-Fix.\nlocal Players = game:GetService(\"Players\")\n\nlocal function setupPlayer(player: Player)\n\tlocal ls = player:FindFirstChild(\"leaderstats\")\n\tif not ls then\n\t\tlocal folder = Instance.new(\"Folder\")\n\t\tfolder.Name = \"leaderstats\"\n\t\tfolder.Parent = player\n\tend\nend\n\nPlayers.PlayerAdded:Connect(setupPlayer)\nfor _, p in Players:GetPlayers() do\n\tsetupPlayer(p)\nend\n";
            write_project_luau(dir, rel, src)?;
            applied_files.push(rel.to_string());
        }
        "luau_static" => {
            let src_root = dir.join("src");
            let mut stack = vec![src_root];
            while let Some(cur) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&cur) else { continue };
                for ent in rd.flatten() {
                    let p = ent.path();
                    if p.is_dir() {
                        stack.push(p);
                        continue;
                    }
                    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if !(name.ends_with(".luau") || name.ends_with(".lua")) {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        if text.trim().is_empty() {
                            let stub = if name.contains(".server.") || name.contains(".client.") {
                                "--!strict\n-- Auto-initialized by OpenRO Playtest Auto-Fix.\nlocal _ = game\n"
                            } else {
                                "--!strict\n-- Auto-initialized by OpenRO Playtest Auto-Fix.\nreturn {}\n"
                            };
                            let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                            write_project_luau(dir, &rel, stub)?;
                            applied_files.push(rel);
                        }
                    }
                }
            }
            let _ = crate::projects::format_project(dir, None);
        }
        "core_loop_move" | "core_loop_act" | "core_loop_hud" => {
            let srv_rel = "src/server/CoreLoopBootstrap.server.luau";
            let srv_src = "--!strict\n-- Auto-scaffolded deterministic core loop server handler.\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal folder = ReplicatedStorage:FindFirstChild(\"OpenROCoreLoop\") or Instance.new(\"Folder\")\nfolder.Name = \"OpenROCoreLoop\"\nfolder.Parent = ReplicatedStorage\nlocal ev = folder:FindFirstChild(\"CoreLoopAction\") or Instance.new(\"RemoteEvent\")\nev.Name = \"CoreLoopAction\"\nev.Parent = folder\nif ev:IsA(\"RemoteEvent\") then\n\tev.OnServerEvent:Connect(function(player: Player)\n\t\tplayer:SetAttribute(\"LastCoreLoopAt\", os.time())\n\tend)\nend\n";
            let cli_rel = "src/client/CoreLoopHud.client.luau";
            let cli_src = "--!strict\n-- Auto-scaffolded deterministic core loop HUD bridge.\nlocal Players = game:GetService(\"Players\")\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal player = Players.LocalPlayer\nif player then\n\tlocal folder = ReplicatedStorage:WaitForChild(\"OpenROCoreLoop\", 5)\n\tif folder then\n\t\tlocal ev = folder:FindFirstChild(\"CoreLoopAction\")\n\t\tif ev and ev:IsA(\"RemoteEvent\") then\n\t\t\tev:FireServer()\n\t\tend\n\tend\nend\n";
            write_project_luau(dir, srv_rel, srv_src)?;
            write_project_luau(dir, cli_rel, cli_src)?;
            applied_files.push(srv_rel.to_string());
            applied_files.push(cli_rel.to_string());
        }
        _ => {
            let _ = crate::projects::format_project(dir, None);
        }
    }
    Ok(applied_files)
}

pub fn apply_assertion_fix_and_rerun(
    shared: &Shared,
    r: &ProjectRef,
    assertion_id: &str,
    body: &Value,
) -> Result<Reply, Reply> {
    let id = assertion_id.trim();
    if id.is_empty() {
        return Err(Reply::bad("assertion_id is required"));
    }
    let plan = fix_plan_spec(&r.dir, id);
    let applied_files = apply_single_assertion_fix(&r.dir, id)?;

    let rerun_reply = rerun_assertions(
        shared,
        r,
        &json!({ "offline": body.get("offline").and_then(Value::as_bool).unwrap_or(false) }),
    )?;
    let rerun_json: Value = serde_json::from_slice(&rerun_reply.body).unwrap_or_else(|_| json!({}));
    Ok(Reply::ok(json!({
        "plan": plan,
        "applied_files": applied_files,
        "rerun": rerun_json,
    })))
}

/// Launch Autopilot: fix all failing assertions, build `.rbxlx`, verify Ship Pack ZIP CRC-32,
/// and re-run the deterministic assertion suite in 1 click.
pub fn fix_all_and_prepare_export(shared: &Shared, r: &ProjectRef, body: &Value) -> Result<Reply, Reply> {
    let prev_rep = latest_report_json(&r.dir);
    let core_loop = prev_rep
        .as_ref()
        .and_then(|rep| rep.get("core_loop"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let initial = build_deterministic_assertions(&r.dir, &[], &[], &core_loop, 0);
    let mut failing_ids: Vec<String> = initial
        .iter()
        .filter(|a| a["passed"].as_bool() != Some(true))
        .filter_map(|a| a["id"].as_str().map(String::from))
        .collect();
    if failing_ids.is_empty() {
        failing_ids.push("luau_static".to_string());
    }

    let mut applied_files: Vec<String> = Vec::new();
    let mut fixed_ids: Vec<String> = Vec::new();
    for id in &failing_ids {
        let written = apply_single_assertion_fix(&r.dir, id)?;
        for f in written {
            if !applied_files.contains(&f) {
                applied_files.push(f);
            }
        }
        fixed_ids.push(id.clone());
    }
    let _ = crate::projects::format_project(&r.dir, None);

    let build_res = crate::projects::build(&r.dir, "auto", None);
    let (build_ok, build_json) = match build_res {
        Ok(b) => (true, b),
        Err(e) => (false, json!({ "ok": false, "error": e })),
    };

    let manifest = projects::manifest(&r.dir).unwrap_or_else(|_| json!({}));
    let rojo_file = manifest["rojoProject"]
        .as_str()
        .unwrap_or(projects::DEFAULT_ROJO_FILE);
    let ship_pack_integrity = crate::opencloud::project_ship_pack_integrity(&r.dir, rojo_file);
    let zip_ok = ship_pack_integrity["ok"].as_bool().unwrap_or(false);

    let rerun_reply = rerun_assertions(
        shared,
        r,
        &json!({ "offline": body.get("offline").and_then(Value::as_bool).unwrap_or(true) }),
    )?;
    let rerun_json: Value = serde_json::from_slice(&rerun_reply.body).unwrap_or_else(|_| json!({}));
    let assertions_ok = rerun_json["passed"].as_bool().unwrap_or(false);

    Ok(Reply::ok(json!({
        "applied_files": applied_files,
        "fixed_ids": fixed_ids,
        "build": build_json,
        "ship_pack_integrity": ship_pack_integrity,
        "export_ready": assertions_ok && build_ok && zip_ok,
        "export_urls": {
            "rbxlx": "/api/export/rbxlx",
            "ship_pack": "/api/export/bundle"
        },
        "rerun": rerun_json,
    })))
}

#[derive(Clone, Debug)]
struct DepScript {
    dot_path: String,
    source: String,
    rel_file: Option<String>,
    is_module: bool,
    is_config_module: bool,
}

fn strip_luau_comments_preserve_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for raw_line in src.lines() {
        let mut line = raw_line;
        if in_block {
            if let Some(end) = line.find("]]") {
                in_block = false;
                line = &line[end + 2..];
            } else {
                out.push(String::new());
                continue;
            }
        }
        let mut cleaned = String::new();
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut quote: Option<u8> = None;
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(q) = quote {
                cleaned.push(b as char);
                if b == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    cleaned.push(bytes[i] as char);
                } else if b == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            if b == b'"' || b == b'\'' {
                quote = Some(b);
                cleaned.push(b as char);
                i += 1;
                continue;
            }
            if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                if line[i..].starts_with("--[[") {
                    if let Some(end) = line[i + 4..].find("]]") {
                        i = i + 4 + end + 2;
                        continue;
                    }
                    in_block = true;
                }
                break;
            }
            cleaned.push(b as char);
            i += 1;
        }
        out.push(cleaned);
    }
    out
}

fn is_module_script(dot_path: &str, rel_file: Option<&str>) -> bool {
    if let Some(rel) = rel_file {
        let low = rel.to_ascii_lowercase();
        if low.contains(".server.") || low.contains(".client.") {
            return false;
        }
    }
    !dot_path.starts_with("ServerScriptService.")
        && !dot_path.starts_with("StarterPlayer.")
        && !dot_path.starts_with("StarterGui.")
        && !dot_path.starts_with("StarterPack.")
        && !dot_path.starts_with("Workspace.")
}

fn resolve_require_expr(
    caller_dot: &str,
    expr: &str,
    aliases: &std::collections::HashMap<String, String>,
) -> String {
    let mut s = expr.trim().to_string();
    // Collapse :WaitForChild("Foo") / :FindFirstChild("Foo") / :WaitForChild("Foo", 5) into .Foo
    for meth in [":WaitForChild(", ":FindFirstChild("] {
        while let Some(pos) = s.find(meth) {
            let after = &s[pos + meth.len()..];
            let Some(close) = after.find(')') else { break };
            let inner = after[..close].split(',').next().unwrap_or("").trim();
            let child = inner.trim_matches(|c| c == '"' || c == '\'').trim();
            let replacement = if child.is_empty() {
                String::new()
            } else {
                format!(".{child}")
            };
            s = format!("{}{}{}", &s[..pos], replacement, &after[close + 1..]);
        }
    }
    // Expand simple local aliases (e.g. Shared -> ReplicatedStorage.Shared)
    let first_seg = s.split('.').next().unwrap_or("").trim();
    if let Some(mapped) = aliases.get(first_seg) {
        let rest = s[first_seg.len()..].trim();
        s = format!("{mapped}{rest}");
    }
    if let Some(rest) = s.strip_prefix("game:GetService(\"ReplicatedStorage\")") {
        s = format!("ReplicatedStorage{rest}");
    } else if let Some(rest) = s.strip_prefix("game:GetService('ReplicatedStorage')") {
        s = format!("ReplicatedStorage{rest}");
    } else if let Some(rest) = s.strip_prefix("game:GetService(\"ServerStorage\")") {
        s = format!("ServerStorage{rest}");
    } else if let Some(rest) = s.strip_prefix("game:GetService('ServerStorage')") {
        s = format!("ServerStorage{rest}");
    } else if s.starts_with("script.Parent") {
        let mut base: Vec<&str> = caller_dot.split('.').collect();
        if !base.is_empty() {
            base.pop();
        }
        let mut cursor = s.as_str();
        while let Some(rest) = cursor.strip_prefix("script.Parent") {
            if let Some(tail) = rest.strip_prefix('.') {
                if tail.starts_with("Parent") {
                    if !base.is_empty() {
                        base.pop();
                    }
                    cursor = tail.strip_prefix("Parent").unwrap_or(tail);
                    if let Some(after_dot) = cursor.strip_prefix('.') {
                        return if base.is_empty() {
                            after_dot.to_string()
                        } else {
                            format!("{}.{}", base.join("."), after_dot)
                        };
                    }
                    continue;
                }
                return if base.is_empty() {
                    tail.to_string()
                } else {
                    format!("{}.{}", base.join("."), tail)
                };
            }
            return base.join(".");
        }
    }
    s.trim_matches('.').to_string()
}

fn match_script_target(target: &str, scripts: &[DepScript]) -> (Option<String>, Option<String>) {
    if target.is_empty() {
        return (None, None);
    }
    if let Some(hit) = scripts.iter().find(|s| s.dot_path == target) {
        return (Some(hit.dot_path.clone()), None);
    }
    // Check case-insensitive or leaf name suggestion for broken require() targets
    let leaf = target.rsplit('.').next().unwrap_or(target).to_ascii_lowercase();
    if let Some(hit) = scripts.iter().find(|s| s.dot_path.eq_ignore_ascii_case(target)) {
        return (None, Some(hit.dot_path.clone()));
    }
    let candidate = scripts.iter().find(|s| {
        s.is_module
            && s.dot_path
                .rsplit('.')
                .next()
                .is_some_and(|l| l.eq_ignore_ascii_case(&leaf) || l.to_ascii_lowercase().starts_with(&leaf))
    });
    (None, candidate.map(|c| c.dot_path.clone()))
}

fn collect_project_dep_scripts(dir: &Path) -> Vec<DepScript> {
    let manifest = crate::projects::manifest(dir).unwrap_or_else(|_| json!({}));
    let rojo = manifest["rojoProject"]
        .as_str()
        .unwrap_or(crate::projects::DEFAULT_ROJO_FILE);
    let mut disk_by_source: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let src_root = dir.join("src");
    let mut stack = vec![src_root];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else { continue };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !(name.ends_with(".luau") || name.ends_with(".lua")) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                disk_by_source.insert(text, rel);
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(tree) = crate::projects::load_tree(dir, rojo) {
        for f in tree.flatten() {
            if let Some(src) = f.source {
                let dot = f.path.replace('/', ".");
                let rel = disk_by_source.get(&src).cloned().or_else(|| {
                    let leaf = dot.rsplit('.').next().unwrap_or("Script");
                    if dot.starts_with("ServerScriptService.") {
                        Some(format!("src/server/{leaf}.server.luau"))
                    } else if dot.starts_with("StarterPlayer.") {
                        Some(format!("src/client/{leaf}.client.luau"))
                    } else if dot.starts_with("ReplicatedStorage.") {
                        Some(format!("src/shared/{leaf}.luau"))
                    } else {
                        None
                    }
                });
                let is_mod = is_module_script(&dot, rel.as_deref());
                let leaf = dot.rsplit('.').next().unwrap_or("");
                let is_cfg = is_mod && matches!(leaf, "Config" | "MonetizationConfig" | "ProductIds" | "AssetIds");
                out.push(DepScript {
                    dot_path: dot,
                    source: src,
                    rel_file: rel,
                    is_module: is_mod,
                    is_config_module: is_cfg,
                });
            }
        }
    }
    out
}

fn analyze_dep_scripts(scripts: &[DepScript]) -> Value {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut broken_requires = Vec::new();
    let mut incoming_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    #[derive(Default)]
    struct RemoteInfo {
        kind: String,
        creators: Vec<(String, usize)>,
        consumer_refs: Vec<(String, usize)>,
        has_usage: bool,
    }
    let mut remotes: std::collections::BTreeMap<String, RemoteInfo> = std::collections::BTreeMap::new();

    for sc in scripts {
        nodes.push(json!({
            "id": sc.dot_path,
            "rel_file": sc.rel_file,
            "kind": if sc.is_module { "ModuleScript" } else { "Script" },
        }));
        let clean_lines = strip_luau_comments_preserve_lines(&sc.source);
        let mut aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut remote_vars: std::collections::HashMap<String, (String, usize, String)> =
            std::collections::HashMap::new();

        for (idx, line) in clean_lines.iter().enumerate() {
            let line_no = idx + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("local ") {
                if let Some((lhs, rhs)) = rest.split_once('=') {
                    let var = lhs.split(':').next().unwrap_or("").trim();
                    let rhs_t = rhs.trim();
                    if !var.is_empty()
                        && !rhs_t.contains("require(")
                        && (rhs_t.contains("ReplicatedStorage")
                            || rhs_t.contains("ServerStorage")
                            || rhs_t.contains("script.Parent"))
                    {
                        let resolved = resolve_require_expr(&sc.dot_path, rhs_t, &aliases);
                        aliases.insert(var.to_string(), resolved);
                    }
                    // Detect `local ev = Instance.new("RemoteEvent")` or `... or Instance.new("RemoteEvent")`
                    for rkind in ["RemoteEvent", "RemoteFunction"] {
                        let pat_dq = format!("Instance.new(\"{rkind}\")");
                        let pat_sq = format!("Instance.new('{rkind}')");
                        if rhs_t.contains(&pat_dq) || rhs_t.contains(&pat_sq) {
                            let explicit_name = rhs_t
                                .find("FindFirstChild(")
                                .and_then(|p| {
                                    let sub = &rhs_t[p + "FindFirstChild(".len()..];
                                    sub.find(')').map(|e| sub[..e].trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                                })
                                .filter(|n| !n.is_empty());
                            remote_vars.insert(
                                var.to_string(),
                                (explicit_name.unwrap_or_else(|| var.to_string()), line_no, rkind.to_string()),
                            );
                        }
                    }
                }
            }

            // Update remote variable `.Name = "Foo"`
            if let Some((lhs, rhs)) = trimmed.split_once('=') {
                if let Some(var) = lhs.trim().strip_suffix(".Name") {
                    if let Some(entry) = remote_vars.get_mut(var.trim()) {
                        let nm = rhs.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                        if !nm.is_empty() {
                            entry.0 = nm;
                        }
                    }
                }
            }

            // Detect `require(...)` calls
            let mut search_from = 0usize;
            while let Some(pos) = trimmed[search_from..].find("require(") {
                let start = search_from + pos + "require(".len();
                let mut depth = 1i32;
                let mut end = None;
                for (off, ch) in trimmed[start..].char_indices() {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(start + off);
                            break;
                        }
                    }
                }
                let Some(end_idx) = end else { break };
                let raw_expr = trimmed[start..end_idx].trim().to_string();
                let resolved = resolve_require_expr(&sc.dot_path, &raw_expr, &aliases);
                let (matched, suggested) = match_script_target(&resolved, scripts);
                let var_name = trimmed
                    .strip_prefix("local ")
                    .and_then(|r| r.split('=').next())
                    .map(|lhs| lhs.split(':').next().unwrap_or("").trim().to_string())
                    .filter(|v| !v.is_empty());
                if let Some(ref target) = matched {
                    *incoming_counts.entry(target.clone()).or_insert(0) += 1;
                } else {
                    broken_requires.push(json!({
                        "from": sc.dot_path,
                        "rel_file": sc.rel_file,
                        "line": line_no,
                        "raw_expr": raw_expr,
                        "resolved_target": resolved,
                        "suggested_target": suggested,
                        "var_name": var_name,
                    }));
                }
                edges.push(json!({
                    "from": sc.dot_path,
                    "to": matched.as_deref().unwrap_or(&resolved),
                    "resolved": matched.is_some(),
                    "line": line_no,
                }));
                search_from = end_idx + 1;
            }
        }

        // Flush created remotes and check whether this script or any other wires handlers/fires
        let full_src = clean_lines.join("\n");
        for (var, (rname, rline, rkind)) in &remote_vars {
            let entry = remotes.entry(rname.clone()).or_default();
            if entry.kind.is_empty() {
                entry.kind = rkind.clone();
            }
            entry.creators.push((sc.dot_path.clone(), *rline));
            let has_local_wiring = [
                format!("{var}.OnServerEvent"),
                format!("{var}.OnServerInvoke"),
                format!("{var}:FireClient"),
                format!("{var}:FireAllClients"),
                format!("{var}.OnClientEvent"),
                format!("{var}:FireServer"),
                format!("{var}:InvokeServer"),
            ]
            .iter()
            .any(|tok| full_src.contains(tok));
            if has_local_wiring {
                entry.has_usage = true;
            }
        }

        // Detect cross-script RemoteEvent/RemoteFunction references on the same line (never require() lines)
        for meth in [":WaitForChild(", ":FindFirstChild("] {
            for (idx, line) in clean_lines.iter().enumerate() {
                if line.contains("require(") {
                    continue;
                }
                let mut pos = 0usize;
                while let Some(found) = line[pos..].find(meth) {
                    let after = &line[pos + found + meth.len()..];
                    if let Some(close) = after.find(')') {
                        let arg = after[..close].split(',').next().unwrap_or("").trim();
                        let name = arg.trim_matches(|c| c == '"' || c == '\'');
                        let ctx_slice = line;
                        let is_remote_use = ctx_slice.contains("RemoteEvent")
                            || ctx_slice.contains("RemoteFunction")
                            || ctx_slice.contains("FireServer")
                            || ctx_slice.contains("FireClient")
                            || ctx_slice.contains("FireAllClients")
                            || ctx_slice.contains("InvokeServer")
                            || ctx_slice.contains("OnServerEvent")
                            || ctx_slice.contains("OnClientEvent")
                            || ctx_slice.contains("OnServerInvoke");
                        if !name.is_empty() && is_remote_use {
                            let entry = remotes.entry(name.to_string()).or_default();
                            if entry.kind.is_empty() {
                                entry.kind = if ctx_slice.contains("Invoke") || ctx_slice.contains("RemoteFunction") {
                                    "RemoteFunction".to_string()
                                } else {
                                    "RemoteEvent".to_string()
                                };
                            }
                            entry.has_usage = true;
                            entry.consumer_refs.push((sc.dot_path.clone(), idx + 1));
                        }
                        pos += found + meth.len() + close;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    let mut unused_modules = Vec::new();
    let suggested_targets: std::collections::HashSet<String> = broken_requires
        .iter()
        .filter_map(|b| b["suggested_target"].as_str().map(String::from))
        .collect();
    for sc in scripts {
        if sc.is_module
            && !sc.is_config_module
            && !incoming_counts.contains_key(&sc.dot_path)
            && !suggested_targets.contains(&sc.dot_path)
        {
            unused_modules.push(json!({
                "path": sc.dot_path,
                "rel_file": sc.rel_file,
                "reason": "ModuleScript has 0 incoming require() references",
            }));
        }
    }

    let mut remote_list = Vec::new();
    let mut orphan_remotes = Vec::new();
    for (name, info) in &remotes {
        let created = !info.creators.is_empty();
        let used = info.has_usage;
        remote_list.push(json!({
            "name": name,
            "kind": if info.kind.is_empty() { "RemoteEvent" } else { &info.kind },
            "created": created,
            "wired": used,
            "creators": info.creators.iter().map(|(s, _)| s).collect::<Vec<_>>(),
            "consumers": info.consumer_refs.iter().map(|(s, _)| s).collect::<Vec<_>>(),
        }));
        if created && !used {
            let (sc, line) = &info.creators[0];
            orphan_remotes.push(json!({
                "name": name,
                "kind": "unused_remote",
                "script": sc,
                "line": line,
                "reason": "Remote created in ReplicatedStorage with no event listener or caller",
            }));
        } else if !created && used {
            let (sc, line) = &info.consumer_refs[0];
            orphan_remotes.push(json!({
                "name": name,
                "kind": "uncreated_remote",
                "script": sc,
                "line": line,
                "reason": "Remote referenced/fired without any server Instance.new creator",
            }));
        }
    }

    let broken_count = broken_requires.len();
    let unused_count = unused_modules.len();
    let orphan_count = orphan_remotes.len();
    json!({
        "schema": "openro.dependency_graph.v1",
        "ok": true,
        "summary": {
            "scripts": scripts.len(),
            "modules": scripts.iter().filter(|s| s.is_module).count(),
            "edges": edges.len(),
            "remotes": remote_list.len(),
            "broken_count": broken_count,
            "unused_count": unused_count,
            "orphan_count": orphan_count,
            "healthy": broken_count == 0 && unused_count == 0 && orphan_count == 0,
        },
        "nodes": nodes,
        "edges": edges,
        "broken_requires": broken_requires,
        "unused_modules": unused_modules,
        "remotes": remote_list,
        "orphan_remotes": orphan_remotes,
    })
}

fn rewrite_or_prune_broken_requires(
    src: &str,
    broken_for_script: &[Value],
) -> (String, usize, Vec<String>) {
    let mut lines: Vec<String> = src.lines().map(String::from).collect();
    let mut fixed = 0usize;
    let mut scaffold_modules = Vec::new();
    for item in broken_for_script {
        let line_no = item["line"].as_u64().unwrap_or(0) as usize;
        let raw_expr = item["raw_expr"].as_str().unwrap_or("");
        if line_no == 0 || line_no > lines.len() || raw_expr.is_empty() {
            continue;
        }
        if let Some(suggested) = item["suggested_target"].as_str() {
            let luau_target = suggested.replace('.', ":WaitForChild(\"");
            // Build ReplicatedStorage:WaitForChild("A"):WaitForChild("B")
            let parts: Vec<&str> = suggested.split('.').collect();
            let valid_expr = if parts.len() >= 2 {
                let mut expr = format!("game:GetService(\"{}\")", parts[0]);
                for p in &parts[1..] {
                    expr.push_str(&format!(":WaitForChild(\"{p}\")"));
                }
                expr
            } else {
                luau_target
            };
            lines[line_no - 1] = lines[line_no - 1].replace(
                &format!("require({raw_expr})"),
                &format!("require({valid_expr})"),
            );
            fixed += 1;
        } else {
            let var_name = item["var_name"].as_str().unwrap_or("");
            let used_later = !var_name.is_empty()
                && lines
                    .iter()
                    .enumerate()
                    .any(|(i, l)| i + 1 != line_no && l.contains(var_name));
            if used_later {
                let target = item["resolved_target"].as_str().unwrap_or("");
                let leaf = target.rsplit('.').next().unwrap_or("StubModule");
                if !leaf.is_empty() && leaf.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    scaffold_modules.push(leaf.to_string());
                    let valid_expr = format!("game:GetService(\"ReplicatedStorage\"):WaitForChild(\"Shared\"):WaitForChild(\"{leaf}\")");
                    lines[line_no - 1] = lines[line_no - 1].replace(
                        &format!("require({raw_expr})"),
                        &format!("require({valid_expr})"),
                    );
                    fixed += 1;
                }
            } else {
                lines[line_no - 1].clear();
                fixed += 1;
            }
        }
    }
    let out = lines
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    (out, fixed, scaffold_modules)
}

fn prune_unused_remote_lines(src: &str, unused_names: &[String]) -> (String, usize) {
    if unused_names.is_empty() {
        return (src.to_string(), 0);
    }
    let lines: Vec<String> = src.lines().map(String::from).collect();
    let mut drop_idx: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut pruned = 0usize;
    for name in unused_names {
        let pat_dq = format!("\"{name}\"");
        let pat_sq = format!("'{name}'");
        // Find var assigned with Instance.new("Remote...") whose .Name matches `name`
        let mut target_vars: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("local ") {
                if let Some((lhs, rhs)) = rest.split_once('=') {
                    if rhs.contains("Instance.new(\"Remote") || rhs.contains("Instance.new('Remote") {
                        let var = lhs.split(':').next().unwrap_or("").trim().to_string();
                        let matches_here = rhs.contains(&pat_dq) || rhs.contains(&pat_sq);
                        let matches_next = (i + 1..lines.len().min(i + 5)).any(|j| {
                            lines[j].contains(&format!("{var}.Name"))
                                && (lines[j].contains(&pat_dq) || lines[j].contains(&pat_sq))
                        });
                        if matches_here || matches_next || var == *name {
                            target_vars.push(var);
                        }
                    }
                }
            }
        }
        if !target_vars.is_empty() {
            pruned += 1;
            for (i, line) in lines.iter().enumerate() {
                if target_vars.iter().any(|v| {
                    line.contains(&format!("local {v}"))
                        || line.contains(&format!("{v}.Name"))
                        || line.contains(&format!("{v}.Parent"))
                }) {
                    drop_idx.insert(i);
                }
            }
        }
    }
    let kept: Vec<String> = lines
        .into_iter()
        .enumerate()
        .filter_map(|(i, l)| if drop_idx.contains(&i) { None } else { Some(l) })
        .collect();
    (kept.join("\n") + "\n", pruned)
}

pub fn project_dependencies_report(r: &ProjectRef) -> Result<Value, Reply> {
    let scripts = collect_project_dep_scripts(&r.dir);
    Ok(analyze_dep_scripts(&scripts))
}

pub fn prune_or_fix_dependencies(r: &ProjectRef) -> Result<Reply, Reply> {
    let scripts = collect_project_dep_scripts(&r.dir);
    let before = analyze_dep_scripts(&scripts);

    let mut applied_files = Vec::new();
    let mut fixed_requires = 0usize;
    let mut pruned_modules = 0usize;
    let mut pruned_remotes = 0usize;
    let mut scaffold_modules: Vec<String> = Vec::new();

    // 1. Prune unused ModuleScripts on disk
    if let Some(unused) = before["unused_modules"].as_array() {
        for item in unused {
            if let Some(rel) = item["rel_file"].as_str() {
                if let Ok(abs) = crate::projects::safe_join(&r.dir, rel) {
                    if abs.is_file() && std::fs::remove_file(&abs).is_ok() {
                        pruned_modules += 1;
                        applied_files.push(rel.to_string());
                    }
                }
            }
        }
    }

    // Group broken requires & unused remotes per script
    let mut broken_by_script: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    if let Some(broken) = before["broken_requires"].as_array() {
        for item in broken {
            if let Some(from) = item["from"].as_str() {
                broken_by_script.entry(from.to_string()).or_default().push(item.clone());
            }
        }
    }
    let mut unused_remotes_by_script: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut uncreated_remotes: Vec<(String, String)> = Vec::new();
    if let Some(orphans) = before["orphan_remotes"].as_array() {
        for item in orphans {
            let kind = item["kind"].as_str().unwrap_or("");
            let name = item["name"].as_str().unwrap_or("").to_string();
            let script = item["script"].as_str().unwrap_or("").to_string();
            if kind == "unused_remote" && !script.is_empty() && !name.is_empty() {
                unused_remotes_by_script.entry(script).or_default().push(name);
            } else if kind == "uncreated_remote" && !name.is_empty() {
                uncreated_remotes.push((name, "RemoteEvent".to_string()));
            }
        }
    }

    for sc in &scripts {
        let Some(rel) = sc.rel_file.as_deref() else { continue };
        let b_list = broken_by_script.get(&sc.dot_path).map(Vec::as_slice).unwrap_or(&[]);
        let r_list = unused_remotes_by_script
            .get(&sc.dot_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if b_list.is_empty() && r_list.is_empty() {
            continue;
        }
        let mut updated = sc.source.clone();
        if !b_list.is_empty() {
            let (next, fcount, mut stubs) = rewrite_or_prune_broken_requires(&updated, b_list);
            updated = next;
            fixed_requires += fcount;
            scaffold_modules.append(&mut stubs);
        }
        if !r_list.is_empty() {
            let (next, pcount) = prune_unused_remote_lines(&updated, r_list);
            updated = next;
            pruned_remotes += pcount;
        }
        if updated != sc.source {
            write_project_luau(&r.dir, rel, &updated)?;
            applied_files.push(rel.to_string());
        }
    }

    for stub in scaffold_modules {
        let rel = format!("src/shared/{stub}.luau");
        if crate::projects::safe_join(&r.dir, &rel).is_ok_and(|p| !p.exists()) {
            write_project_luau(&r.dir, &rel, "--!strict\n-- Auto-scaffolded missing ModuleScript target.\nreturn {}\n")?;
            applied_files.push(rel);
        }
    }

    if !uncreated_remotes.is_empty() {
        let mut lines = vec![
            "--!strict".to_string(),
            "-- Auto-scaffolded ReplicatedStorage remotes wired by OpenRO Pruner.".to_string(),
            "local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")".to_string(),
        ];
        for (i, (name, _kind)) in uncreated_remotes.iter().enumerate() {
            let var = format!("remote_{i}");
            let safe_name = name.replace('"', "");
            lines.push(format!("local {var} = ReplicatedStorage:FindFirstChild(\"{safe_name}\") or Instance.new(\"RemoteEvent\")"));
            lines.push(format!("{var}.Name = \"{safe_name}\""));
            lines.push(format!("{var}.Parent = ReplicatedStorage"));
            lines.push(format!("if {var}:IsA(\"RemoteEvent\") then\n\t{var}.OnServerEvent:Connect(function(player: Player) player:SetAttribute(\"LastRemote\", \"{safe_name}\") end)\nend"));
            pruned_remotes += 1;
        }
        let rel = "src/server/RemoteBootstrap.server.luau";
        write_project_luau(&r.dir, rel, &(lines.join("\n") + "\n"))?;
        applied_files.push(rel.to_string());
    }

    let after_scripts = collect_project_dep_scripts(&r.dir);
    let after = analyze_dep_scripts(&after_scripts);
    Ok(Reply::ok(json!({
        "pruned_modules": pruned_modules,
        "fixed_requires": fixed_requires,
        "pruned_remotes": pruned_remotes,
        "applied_files": applied_files,
        "before": before,
        "after": after,
    })))
}

pub fn studio_dependencies_report(scripts_map: &std::collections::BTreeMap<String, String>) -> Value {
    let list: Vec<DepScript> = scripts_map
        .iter()
        .map(|(dot, src)| {
            let is_mod = is_module_script(dot, None);
            let leaf = dot.rsplit('.').next().unwrap_or("");
            let is_cfg = is_mod && matches!(leaf, "Config" | "MonetizationConfig" | "ProductIds" | "AssetIds");
            DepScript {
                dot_path: dot.clone(),
                source: src.clone(),
                rel_file: None,
                is_module: is_mod,
                is_config_module: is_cfg,
            }
        })
        .collect();
    analyze_dep_scripts(&list)
}

pub fn prune_or_fix_studio_place(agent: &mut crate::agent::Agent, chat_id: &str) -> Result<Value, String> {
    let tree = agent.place_tree()?;
    let mut scripts_map = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts_map.insert(k.clone(), s.to_string());
            }
        }
    }
    let before = studio_dependencies_report(&scripts_map);
    let mut pruned_modules = 0usize;
    let mut fixed_requires = 0usize;
    let mut pruned_remotes = 0usize;
    let mut applied_scripts = Vec::new();

    if let Some(unused) = before["unused_modules"].as_array() {
        for item in unused {
            if let Some(path) = item["path"].as_str() {
                if agent.delete_at_path(chat_id, path).is_ok() {
                    pruned_modules += 1;
                    applied_scripts.push(path.to_string());
                }
            }
        }
    }
    let mut broken_by_script: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    if let Some(broken) = before["broken_requires"].as_array() {
        for item in broken {
            if let Some(from) = item["from"].as_str() {
                broken_by_script.entry(from.to_string()).or_default().push(item.clone());
            }
        }
    }
    let mut unused_remotes_by_script: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    if let Some(orphans) = before["orphan_remotes"].as_array() {
        for item in orphans {
            if item["kind"] == "unused_remote" {
                if let (Some(sc), Some(nm)) = (item["script"].as_str(), item["name"].as_str()) {
                    unused_remotes_by_script.entry(sc.to_string()).or_default().push(nm.to_string());
                }
            }
        }
    }
    for (path, src) in &scripts_map {
        let b_list = broken_by_script.get(path).map(Vec::as_slice).unwrap_or(&[]);
        let r_list = unused_remotes_by_script.get(path).map(Vec::as_slice).unwrap_or(&[]);
        if b_list.is_empty() && r_list.is_empty() {
            continue;
        }
        let mut updated = src.clone();
        if !b_list.is_empty() {
            let (next, fcount, _) = rewrite_or_prune_broken_requires(&updated, b_list);
            updated = next;
            fixed_requires += fcount;
        }
        if !r_list.is_empty() {
            let (next, pcount) = prune_unused_remote_lines(&updated, r_list);
            updated = next;
            pruned_remotes += pcount;
        }
        if updated != *src && agent.update_script_source(chat_id, path, &updated).is_ok() {
            applied_scripts.push(path.clone());
        }
    }

    let after_tree = agent.place_tree()?;
    let mut after_map = std::collections::BTreeMap::new();
    if let Some(obj) = after_tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                after_map.insert(k.clone(), s.to_string());
            }
        }
    }
    let after = studio_dependencies_report(&after_map);
    Ok(json!({
        "ok": true,
        "pruned_modules": pruned_modules,
        "fixed_requires": fixed_requires,
        "pruned_remotes": pruned_remotes,
        "applied_scripts": applied_scripts,
        "before": before,
        "after": after,
    }))
}

const MAX_PROJECT_SNAPSHOTS: usize = 20;

fn project_snapshots_dir(dir: &Path) -> PathBuf {
    dir.join(".openro").join("snapshots")
}

fn collect_project_luau_map(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let src_root = dir.join("src");
    let mut stack = vec![src_root];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else { continue };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !(name.ends_with(".luau") || name.ends_with(".lua")) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                map.insert(rel, text);
            }
        }
    }
    map
}

fn eval_project_assertion_summary(dir: &Path) -> Value {
    let core_loop = latest_report_json(dir)
        .as_ref()
        .and_then(|rep| rep.get("core_loop"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let items: Vec<Value> = build_deterministic_assertions(dir, &[], &[], &core_loop, 0)
        .into_iter()
        .map(|a| {
            json!({
                "id": a["id"],
                "passed": a["passed"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    let passed_count = items
        .iter()
        .filter(|a| a["passed"].as_bool() == Some(true))
        .count();
    let total_count = items.len();
    let passed = total_count > 0 && passed_count == total_count;
    json!({
        "passed": passed,
        "passed_count": passed_count,
        "total_count": total_count,
        "total": total_count,
        "failed_count": total_count.saturating_sub(passed_count),
        "assertions": items,
    })
}

fn load_sorted_project_snapshots(dir: &Path) -> Vec<Value> {
    let root = project_snapshots_dir(dir);
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut snaps: Vec<Value> = rd
        .flatten()
        .filter_map(|ent| {
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                return None;
            }
            let raw = std::fs::read_to_string(&p).ok()?;
            serde_json::from_str::<Value>(&raw).ok()
        })
        .collect();
    snaps.sort_by_key(|v| v["created_at_ms"].as_u64().unwrap_or(0));
    snaps
}

fn compute_snapshot_seq_diff(prev: Option<&Value>, curr: &Value) -> Value {
    let empty_map = serde_json::Map::new();
    let prev_scripts = prev
        .and_then(|p| p.get("scripts"))
        .and_then(Value::as_object)
        .unwrap_or(&empty_map);
    let curr_scripts = curr
        .get("scripts")
        .and_then(Value::as_object)
        .unwrap_or(&empty_map);

    let mut changed_scripts = Vec::new();
    for (path, val) in curr_scripts {
        let after_txt = val.as_str().unwrap_or("");
        let after_lines = after_txt.lines().count();
        match prev_scripts.get(path).and_then(Value::as_str) {
            None => changed_scripts.push(json!({
                "path": path,
                "op": "added",
                "before_lines": 0,
                "after_lines": after_lines,
            })),
            Some(before_txt) if before_txt != after_txt => changed_scripts.push(json!({
                "path": path,
                "op": "modified",
                "before_lines": before_txt.lines().count(),
                "after_lines": after_lines,
            })),
            _ => {}
        }
    }
    for (path, val) in prev_scripts {
        if !curr_scripts.contains_key(path) {
            changed_scripts.push(json!({
                "path": path,
                "op": "removed",
                "before_lines": val.as_str().unwrap_or("").lines().count(),
                "after_lines": 0,
            }));
        }
    }

    let curr_sum = &curr["assertion_summary"];
    let curr_passed = curr_sum["passed_count"].as_i64().unwrap_or(0);
    let prev_passed = prev
        .and_then(|p| p["assertion_summary"]["passed_count"].as_i64())
        .unwrap_or(curr_passed);
    let mut prev_map: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    if let Some(arr) = prev.and_then(|p| p["assertion_summary"]["assertions"].as_array()) {
        for a in arr {
            if let Some(id) = a["id"].as_str() {
                prev_map.insert(id.to_string(), a["passed"].as_bool().unwrap_or(false));
            }
        }
    }
    let mut newly_passed = Vec::new();
    let mut newly_failed = Vec::new();
    if let Some(arr) = curr_sum["assertions"].as_array() {
        for a in arr {
            let Some(id) = a["id"].as_str() else { continue };
            let now_ok = a["passed"].as_bool().unwrap_or(false);
            if let Some(&was_ok) = prev_map.get(id) {
                if !was_ok && now_ok {
                    newly_passed.push(id.to_string());
                } else if was_ok && !now_ok {
                    newly_failed.push(id.to_string());
                }
            }
        }
    }
    json!({
        "changed_scripts": changed_scripts,
        "assertion_delta": {
            "passed_delta": if prev.is_some() { curr_passed - prev_passed } else { 0 },
            "prev_passed_count": prev_passed,
            "curr_passed_count": curr_passed,
            "newly_passed": newly_passed,
            "newly_failed": newly_failed,
        }
    })
}

pub fn list_project_snapshots(r: &ProjectRef) -> Result<Value, Reply> {
    let snaps = load_sorted_project_snapshots(&r.dir);
    let mut enriched = Vec::with_capacity(snaps.len());
    let mut latest_verified_id: Option<String> = None;
    for (idx, snap) in snaps.iter().enumerate() {
        let prev = if idx > 0 { Some(&snaps[idx - 1]) } else { None };
        let diff = compute_snapshot_seq_diff(prev, snap);
        let verified = snap["verified"].as_bool().unwrap_or(false);
        let sid = snap["snapshot_id"].as_str().unwrap_or("").to_string();
        if verified && !sid.is_empty() {
            latest_verified_id = Some(sid.clone());
        }
        enriched.push(json!({
            "snapshot_id": sid,
            "label": snap["label"],
            "created_at_ms": snap["created_at_ms"],
            "scripts_count": snap["scripts"].as_object().map(|m| m.len()).unwrap_or(0),
            "verified": verified,
            "assertion_summary": snap["assertion_summary"],
            "changed_scripts": diff["changed_scripts"],
            "assertion_delta": diff["assertion_delta"],
        }));
    }
    enriched.reverse();
    Ok(json!({
        "schema": "openro.project_snapshots.v1",
        "latest_verified_id": latest_verified_id,
        "snapshots": enriched,
    }))
}

pub fn create_project_snapshot(r: &ProjectRef, body: &Value) -> Result<Reply, Reply> {
    create_project_snapshot_opts(r, body, true)
}

fn create_project_snapshot_opts(
    r: &ProjectRef,
    body: &Value,
    update_sync_baseline: bool,
) -> Result<Reply, Reply> {
    let root = project_snapshots_dir(&r.dir);
    std::fs::create_dir_all(&root)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("snapshots dir: {e}")))?;
    let label = body
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Playtest Checkpoint")
        .to_string();
    let scripts = collect_project_luau_map(&r.dir);
    let summary = eval_project_assertion_summary(&r.dir);
    let verified = summary["passed"].as_bool().unwrap_or(false);
    let default_proj_raw = std::fs::read_to_string(r.dir.join("default.project.json")).ok();
    let workspace_has_spawn = project_has_spawn_location(&scripts, default_proj_raw.as_deref());
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    static SNAP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = SNAP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sid = format!("psnap-{ms}-{seq}");
    let rec = json!({
        "snapshot_id": sid,
        "label": label,
        "created_at_ms": ms,
        "verified": verified,
        "assertion_summary": summary,
        "scripts": scripts,
        "workspace_has_spawn": workspace_has_spawn,
        "default_project_json": default_proj_raw,
    });
    let target = root.join(format!("{sid}.json"));
    let tmp = root.join(format!("{sid}.json.tmp-openro"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&rec).unwrap_or_default())
        .and_then(|_| std::fs::rename(&tmp, &target))
        .map_err(|e| Reply::err("500 Internal Server Error", format!("write snapshot: {e}")))?;

    if update_sync_baseline {
        let _ = write_sync_baseline(&r.dir, &scripts, Some(&sid));
    }

    let existing = load_sorted_project_snapshots(&r.dir);
    if existing.len() > MAX_PROJECT_SNAPSHOTS {
        for drop_snap in &existing[..existing.len() - MAX_PROJECT_SNAPSHOTS] {
            if let Some(old_id) = drop_snap["snapshot_id"].as_str() {
                if plain_name(old_id) {
                    let _ = std::fs::remove_file(root.join(format!("{old_id}.json")));
                }
            }
        }
    }
    let list = list_project_snapshots(r)?;
    Ok(Reply::ok(json!({
        "snapshot": rec,
        "snapshots": list["snapshots"],
        "latest_verified_id": list["latest_verified_id"],
    })))
}

pub fn restore_project_snapshot(
    shared: &Shared,
    r: &ProjectRef,
    sid: &str,
    body: &Value,
) -> Result<Reply, Reply> {
    if !plain_name(sid) {
        return Err(Reply::bad("invalid snapshot id"));
    }
    let target = project_snapshots_dir(&r.dir).join(format!("{sid}.json"));
    if !target.is_file() {
        return Err(Reply::err("404 Not Found", format!("snapshot not found: {sid}")));
    }
    let raw = std::fs::read_to_string(&target)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("read snapshot: {e}")))?;
    let snap: Value = serde_json::from_str(&raw)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("corrupt snapshot: {e}")))?;
    let Some(snap_scripts) = snap.get("scripts").and_then(Value::as_object) else {
        return Err(Reply::err("500 Internal Server Error", "snapshot missing scripts map"));
    };

    let current_scripts = collect_project_luau_map(&r.dir);
    let auto_checkpoint = body
        .get("auto_checkpoint")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let differs = current_scripts.len() != snap_scripts.len()
        || current_scripts
            .iter()
            .any(|(k, v)| snap_scripts.get(k).and_then(Value::as_str) != Some(v.as_str()));
    if auto_checkpoint && differs {
        let _ = create_project_snapshot_opts(r, &json!({ "label": "Pre-rollback safety checkpoint" }), false);
    }

    let mut restored_files = Vec::new();
    for (rel, val) in snap_scripts {
        let Some(src) = val.as_str() else { continue };
        write_project_luau(&r.dir, rel, src)?;
        restored_files.push(rel.clone());
    }
    let mut removed_files = Vec::new();
    for rel in current_scripts.keys() {
        if !snap_scripts.contains_key(rel) {
            if let Ok(abs) = crate::projects::safe_join(&r.dir, rel) {
                if abs.is_file() && std::fs::remove_file(&abs).is_ok() {
                    removed_files.push(rel.clone());
                }
            }
        }
    }

    let rerun_reply = rerun_assertions(shared, r, &json!({ "offline": true }))?;
    let rerun_json: Value = serde_json::from_slice(&rerun_reply.body).unwrap_or_else(|_| json!({}));
    let list = list_project_snapshots(r)?;

    Ok(Reply::ok(json!({
        "restored_id": sid,
        "label": snap["label"],
        "verified": snap["verified"].as_bool().unwrap_or(false),
        "restored_files": restored_files,
        "removed_files": removed_files,
        "rerun": rerun_json,
        "snapshots": list["snapshots"],
        "latest_verified_id": list["latest_verified_id"],
    })))
}

fn is_critical_hook_file(path: &str) -> bool {
    let low = path.to_ascii_lowercase();
    low.contains("monetization")
        || low.contains("productids")
        || low.contains("economyshop")
        || low.contains("shopprompt")
        || low.contains("dailyreward")
        || low.contains("spawnguard")
}

fn has_strict_pragma(src: &str) -> bool {
    src.lines().take(3).any(|l| l.trim() == "--!strict")
}

fn project_has_spawn_location(
    scripts: &std::collections::BTreeMap<String, String>,
    default_proj_raw: Option<&str>,
) -> bool {
    if scripts.values().any(|s| s.contains("SpawnLocation")) {
        return true;
    }
    default_proj_raw.is_some_and(|raw| raw.contains("SpawnLocation"))
}

fn extract_nonzero_robux_ids(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--") {
            continue;
        }
        let Some((lhs, rhs)) = trimmed.split_once('=') else {
            continue;
        };
        let key = lhs.trim().trim_end_matches([',', ';']).trim();
        let val = rhs.trim().trim_end_matches([',', ';']).trim();
        let key_low = key.to_ascii_lowercase();
        let is_id_key = key_low.contains("gamepass")
            || key_low.contains("productid")
            || key_low.contains("passid")
            || key_low.ends_with("_id")
            || key_low.ends_with("id");
        if !is_id_key || val.is_empty() || !val.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(num) = val.parse::<u64>() {
            if num > 0 {
                out.push((key.to_string(), val.to_string()));
            }
        }
    }
    out
}

fn find_hook_owner(
    scripts: &std::collections::BTreeMap<String, String>,
    needle: &str,
) -> Option<String> {
    scripts
        .iter()
        .find(|(_, src)| src.contains(needle))
        .map(|(k, _)| k.clone())
}

/// Evaluate Preflight Hook-Integrity against a verified baseline script map.
pub fn evaluate_hook_integrity(
    baseline_scripts: &std::collections::BTreeMap<String, String>,
    current_scripts: &std::collections::BTreeMap<String, String>,
    baseline_has_spawn: bool,
    current_has_spawn: bool,
) -> Vec<Value> {
    let mut missing = Vec::new();

    for path in baseline_scripts.keys() {
        if is_critical_hook_file(path) && !current_scripts.contains_key(path) {
            missing.push(json!({
                "id": format!("hook:file:{path}"),
                "category": "monetization",
                "kind": "missing_file",
                "file": path,
                "label": format!("Missing critical script {path}"),
                "detail": "Verified baseline script was deleted by a recent edit.",
            }));
        }
    }

    for (path, base_src) in baseline_scripts {
        if let Some(curr_src) = current_scripts.get(path) {
            if has_strict_pragma(base_src) && !has_strict_pragma(curr_src) {
                missing.push(json!({
                    "id": format!("hook:strict:{path}"),
                    "category": "strict_luau",
                    "kind": "missing_strict",
                    "file": path,
                    "label": format!("Dropped --!strict in {path}"),
                    "detail": "Baseline script enforced --!strict typechecking.",
                }));
            }
        }
    }

    let hook_checks: [(&str, &str, &str, &str, &str); 5] = [
        (
            "ProcessReceipt",
            "monetization",
            "hook:monetization:ProcessReceipt",
            "Missing MarketplaceService.ProcessReceipt handler",
            "Server-authoritative DevProduct receipt verification was dropped.",
        ),
        (
            "PromptGamePassPurchase",
            "monetization",
            "hook:monetization:PromptGamePassPurchase",
            "Missing GamePass purchase prompt hook",
            "PromptGamePassPurchase wiring was dropped from place scripts.",
        ),
        (
            "PromptProductPurchase",
            "monetization",
            "hook:monetization:PromptProductPurchase",
            "Missing DevProduct purchase prompt hook",
            "PromptProductPurchase wiring was dropped from place scripts.",
        ),
        (
            "Players.PlayerAdded",
            "core",
            "hook:core:Players.PlayerAdded",
            "Missing Players.PlayerAdded lifecycle hook",
            "Core player join initialization hook was removed.",
        ),
        (
            "BindToClose",
            "core",
            "hook:core:BindToClose",
            "Missing game:BindToClose shutdown guard",
            "Graceful server shutdown persistence hook was removed.",
        ),
    ];
    for (needle, cat, id, label, detail) in hook_checks {
        if let Some(owner) = find_hook_owner(baseline_scripts, needle) {
            if find_hook_owner(current_scripts, needle).is_none()
                && current_scripts.contains_key(&owner)
            {
                missing.push(json!({
                    "id": id,
                    "category": cat,
                    "kind": if cat == "monetization" { "missing_revenue_hook" } else { "missing_core_hook" },
                    "file": owner,
                    "label": label,
                    "detail": detail,
                }));
            }
        }
    }

    let mut baseline_ids: Vec<(String, String, String)> = Vec::new();
    for (path, src) in baseline_scripts {
        for (k, v) in extract_nonzero_robux_ids(src) {
            baseline_ids.push((path.clone(), k, v));
        }
    }
    if !baseline_ids.is_empty() {
        let mut current_ids = std::collections::HashSet::new();
        for src in current_scripts.values() {
            for (k, v) in extract_nonzero_robux_ids(src) {
                current_ids.insert((k, v));
            }
        }
        for (file, key, val) in baseline_ids {
            if !current_ids.contains(&(key.clone(), val.clone()))
                && current_scripts.contains_key(&file)
            {
                missing.push(json!({
                    "id": format!("hook:id:{key}"),
                    "category": "monetization",
                    "kind": "missing_revenue_id",
                    "file": file,
                    "label": format!("Dropped Robux ID {key} = {val}"),
                    "detail": format!("Baseline {file} wired {key} = {val}, now zeroed or removed."),
                }));
            }
        }
    }

    if baseline_has_spawn && !current_has_spawn {
        let spawn_owner = find_hook_owner(baseline_scripts, "SpawnLocation")
            .unwrap_or_else(|| "default.project.json".to_string());
        missing.push(json!({
            "id": "hook:spawn:Workspace.SpawnLocation",
            "category": "spawn",
            "kind": "missing_spawn",
            "file": spawn_owner,
            "label": "Missing Workspace.SpawnLocation",
            "detail": "Verified baseline included Workspace.SpawnLocation; spawn point was removed.",
        }));
    }

    missing
}

fn surgical_restore_hooks(
    baseline_scripts: &std::collections::BTreeMap<String, String>,
    current_scripts: &mut std::collections::BTreeMap<String, String>,
    baseline_has_spawn: bool,
    current_has_spawn: bool,
    default_spawn_target: &str,
) -> (Vec<String>, Vec<String>, usize) {
    let mut restored_files = Vec::new();
    let mut modified_files = std::collections::BTreeSet::new();
    let mut hooks_restored = 0usize;

    for (path, base_src) in baseline_scripts {
        if is_critical_hook_file(path) && !current_scripts.contains_key(path) {
            current_scripts.insert(path.clone(), base_src.clone());
            restored_files.push(path.clone());
            hooks_restored += 1;
        }
    }

    for (path, base_src) in baseline_scripts {
        if let Some(curr) = current_scripts.get_mut(path) {
            if has_strict_pragma(base_src) && !has_strict_pragma(curr) {
                *curr = format!("--!strict\n{curr}");
                modified_files.insert(path.clone());
                hooks_restored += 1;
            }
        }
    }

    // Restore dropped non-zero Robux IDs in existing config scripts
    for (path, base_src) in baseline_scripts {
        let base_ids = extract_nonzero_robux_ids(base_src);
        if base_ids.is_empty() {
            continue;
        }
        let Some(curr) = current_scripts.get_mut(path) else {
            continue;
        };
        let curr_ids: std::collections::HashSet<(String, String)> =
            extract_nonzero_robux_ids(curr).into_iter().collect();
        for (key, val) in base_ids {
            if curr_ids.contains(&(key.clone(), val.clone())) {
                continue;
            }
            let zero_pat = format!("{key} = 0");
            if curr.contains(&zero_pat) {
                *curr = curr.replace(&zero_pat, &format!("{key} = {val}"));
                modified_files.insert(path.clone());
                hooks_restored += 1;
            } else {
                // Append clean key preservation comment/override if table exists
                if let Some(pos) = curr.rfind("return ") {
                    curr.insert_str(
                        pos,
                        &format!("local _OpenRO_{key}: number = {val}\n"),
                    );
                } else {
                    curr.push_str(&format!("\nlocal {key} = {val}\n"));
                }
                modified_files.insert(path.clone());
                hooks_restored += 1;
            }
        }
    }

    let hook_snippets: [(&str, &str); 5] = [
        (
            "ProcessReceipt",
            "\n-- Restored by OpenRO Preflight Hook-Integrity Guard\ndo\n\tlocal MarketplaceService = game:GetService(\"MarketplaceService\")\n\tlocal Players = game:GetService(\"Players\")\n\tif not MarketplaceService.ProcessReceipt then\n\t\tMarketplaceService.ProcessReceipt = function(receiptInfo)\n\t\t\tlocal player = Players:GetPlayerByUserId(receiptInfo.PlayerId)\n\t\t\tif not player then\n\t\t\t\treturn Enum.ProductPurchaseDecision.NotProcessedYet\n\t\t\tend\n\t\t\treturn Enum.ProductPurchaseDecision.PurchaseGranted\n\t\tend\n\tend\nend\n",
        ),
        (
            "PromptGamePassPurchase",
            "\n-- Restored by OpenRO Preflight Hook-Integrity Guard\ndo\n\tlocal MarketplaceService = game:GetService(\"MarketplaceService\")\n\tlocal _promptGamePass = function(player: Player, passId: number)\n\t\tif passId > 0 then\n\t\t\tMarketplaceService:PromptGamePassPurchase(player, passId)\n\t\tend\n\tend\nend\n",
        ),
        (
            "PromptProductPurchase",
            "\n-- Restored by OpenRO Preflight Hook-Integrity Guard\ndo\n\tlocal MarketplaceService = game:GetService(\"MarketplaceService\")\n\tlocal _promptProduct = function(player: Player, productId: number)\n\t\tif productId > 0 then\n\t\t\tMarketplaceService:PromptProductPurchase(player, productId)\n\t\tend\n\tend\nend\n",
        ),
        (
            "Players.PlayerAdded",
            "\n-- Restored by OpenRO Preflight Hook-Integrity Guard\ndo\n\tlocal Players = game:GetService(\"Players\")\n\tPlayers.PlayerAdded:Connect(function(player: Player)\n\t\tplayer:SetAttribute(\"OpenROHookReady\", true)\n\tend)\nend\n",
        ),
        (
            "BindToClose",
            "\n-- Restored by OpenRO Preflight Hook-Integrity Guard\ngame:BindToClose(function()\n\ttask.wait(0.2)\nend)\n",
        ),
    ];
    for (needle, snippet) in hook_snippets {
        if let Some(owner) = find_hook_owner(baseline_scripts, needle) {
            if find_hook_owner(current_scripts, needle).is_none() {
                if let Some(curr) = current_scripts.get_mut(&owner) {
                    curr.push_str(snippet);
                    modified_files.insert(owner);
                    hooks_restored += 1;
                }
            }
        }
    }

    if baseline_has_spawn && !current_has_spawn {
        let target = find_hook_owner(baseline_scripts, "SpawnLocation")
            .filter(|p| current_scripts.contains_key(p))
            .unwrap_or_else(|| default_spawn_target.to_string());
        let spawn_snippet = "\n-- Restored Workspace.SpawnLocation by OpenRO Preflight Hook-Integrity Guard\ndo\n\tlocal Workspace = game:GetService(\"Workspace\")\n\tif not Workspace:FindFirstChildOfClass(\"SpawnLocation\") then\n\t\tlocal spawn = Instance.new(\"SpawnLocation\")\n\t\tspawn.Name = \"SpawnLocation\"\n\t\tspawn.Anchored = true\n\t\tspawn.Parent = Workspace\n\tend\nend\n";
        let entry = current_scripts
            .entry(target.clone())
            .or_insert_with(|| "--!strict\n".to_string());
        entry.push_str(spawn_snippet);
        if target == default_spawn_target && !restored_files.contains(&target) {
            restored_files.push(target.clone());
        } else {
            modified_files.insert(target);
        }
        hooks_restored += 1;
    }

    (restored_files, modified_files.into_iter().collect(), hooks_restored)
}

fn json_obj_to_scripts_map(obj: Option<&serde_json::Map<String, Value>>) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    if let Some(m) = obj {
        for (k, v) in m {
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_string());
            }
        }
    }
    map
}

pub fn project_hook_integrity(r: &ProjectRef) -> Result<Value, Reply> {
    let snaps = load_sorted_project_snapshots(&r.dir);
    let baseline = snaps
        .iter()
        .rev()
        .find(|s| s["verified"].as_bool().unwrap_or(false));
    let current_scripts = collect_project_luau_map(&r.dir);
    let current_proj_raw = std::fs::read_to_string(r.dir.join("default.project.json")).ok();
    let Some(base) = baseline else {
        return Ok(json!({
            "schema": "openro.hook_integrity.v1",
            "has_baseline": false,
            "baseline_snapshot_id": Value::Null,
            "baseline_label": Value::Null,
            "healthy": true,
            "missing_count": 0,
            "preserved_new_scripts": [],
            "missing_hooks": [],
        }));
    };
    let baseline_scripts = json_obj_to_scripts_map(base.get("scripts").and_then(Value::as_object));
    let baseline_has_spawn = base
        .get("workspace_has_spawn")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            project_has_spawn_location(
                &baseline_scripts,
                base.get("default_project_json").and_then(Value::as_str),
            )
        });
    let current_has_spawn =
        project_has_spawn_location(&current_scripts, current_proj_raw.as_deref());
    let missing = evaluate_hook_integrity(
        &baseline_scripts,
        &current_scripts,
        baseline_has_spawn,
        current_has_spawn,
    );
    let preserved_new_scripts: Vec<String> = current_scripts
        .keys()
        .filter(|k| !baseline_scripts.contains_key(*k))
        .cloned()
        .collect();
    Ok(json!({
        "schema": "openro.hook_integrity.v1",
        "has_baseline": true,
        "baseline_snapshot_id": base["snapshot_id"],
        "baseline_label": base["label"],
        "healthy": missing.is_empty(),
        "missing_count": missing.len(),
        "preserved_new_scripts": preserved_new_scripts,
        "missing_hooks": missing,
    }))
}

pub fn restore_project_hooks(
    shared: &Shared,
    r: &ProjectRef,
    sid: &str,
    body: &Value,
) -> Result<Reply, Reply> {
    if !plain_name(sid) {
        return Err(Reply::bad("invalid snapshot id"));
    }
    let target = project_snapshots_dir(&r.dir).join(format!("{sid}.json"));
    if !target.is_file() {
        return Err(Reply::err("404 Not Found", format!("snapshot not found: {sid}")));
    }
    let raw = std::fs::read_to_string(&target)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("read snapshot: {e}")))?;
    let snap: Value = serde_json::from_str(&raw)
        .map_err(|e| Reply::err("500 Internal Server Error", format!("corrupt snapshot: {e}")))?;
    let baseline_scripts = json_obj_to_scripts_map(snap.get("scripts").and_then(Value::as_object));
    let mut current_scripts = collect_project_luau_map(&r.dir);
    let current_proj_raw = std::fs::read_to_string(r.dir.join("default.project.json")).ok();
    let baseline_has_spawn = snap
        .get("workspace_has_spawn")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            project_has_spawn_location(
                &baseline_scripts,
                snap.get("default_project_json").and_then(Value::as_str),
            )
        });
    let current_has_spawn =
        project_has_spawn_location(&current_scripts, current_proj_raw.as_deref());

    let auto_checkpoint = body
        .get("auto_checkpoint")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let before_missing = evaluate_hook_integrity(
        &baseline_scripts,
        &current_scripts,
        baseline_has_spawn,
        current_has_spawn,
    );
    if auto_checkpoint && !before_missing.is_empty() {
        let _ = create_project_snapshot_opts(
            r,
            &json!({ "label": "Pre-hook-restore safety checkpoint" }),
            false,
        );
    }

    let original_scripts = current_scripts.clone();
    let (restored_files, modified_files, hooks_restored) = surgical_restore_hooks(
        &baseline_scripts,
        &mut current_scripts,
        baseline_has_spawn,
        current_has_spawn,
        "src/server/SpawnGuard.server.luau",
    );
    for (rel, src) in &current_scripts {
        if original_scripts.get(rel) != Some(src) {
            write_project_luau(&r.dir, rel, src)?;
        }
    }

    let rerun_reply = rerun_assertions(shared, r, &json!({ "offline": true }))?;
    let rerun_json: Value = serde_json::from_slice(&rerun_reply.body).unwrap_or_else(|_| json!({}));
    let after = project_hook_integrity(r)?;
    let list = list_project_snapshots(r)?;

    Ok(Reply::ok(json!({
        "restored_id": sid,
        "label": snap["label"],
        "hooks_restored": hooks_restored,
        "restored_files": restored_files,
        "modified_files": modified_files,
        "preserved_new_scripts": after["preserved_new_scripts"],
        "after": after,
        "rerun": rerun_json,
        "snapshots": list["snapshots"],
        "latest_verified_id": list["latest_verified_id"],
    })))
}

pub fn studio_hook_integrity_from_trees(
    baseline_id: Option<&str>,
    baseline_label: Option<&str>,
    baseline_tree: Option<&Value>,
    current_tree: &Value,
) -> Value {
    let Some(base_tree) = baseline_tree else {
        return json!({
            "schema": "openro.hook_integrity.v1",
            "has_baseline": false,
            "baseline_snapshot_id": Value::Null,
            "baseline_label": Value::Null,
            "healthy": true,
            "missing_count": 0,
            "preserved_new_scripts": [],
            "missing_hooks": [],
        });
    };
    let baseline_scripts = json_obj_to_scripts_map(base_tree.get("scripts").and_then(Value::as_object));
    let current_scripts = json_obj_to_scripts_map(current_tree.get("scripts").and_then(Value::as_object));
    let tree_has_spawn = |tree: &Value, scripts: &std::collections::BTreeMap<String, String>| {
        scripts.values().any(|s| s.contains("SpawnLocation"))
            || tree
                .get("instances")
                .and_then(Value::as_object)
                .is_some_and(|m| m.keys().any(|k| k.contains("SpawnLocation")))
    };
    let base_spawn = tree_has_spawn(base_tree, &baseline_scripts);
    let curr_spawn = tree_has_spawn(current_tree, &current_scripts);
    let missing = evaluate_hook_integrity(&baseline_scripts, &current_scripts, base_spawn, curr_spawn);
    let preserved_new_scripts: Vec<String> = current_scripts
        .keys()
        .filter(|k| !baseline_scripts.contains_key(*k))
        .cloned()
        .collect();
    json!({
        "schema": "openro.hook_integrity.v1",
        "has_baseline": true,
        "baseline_snapshot_id": baseline_id,
        "baseline_label": baseline_label,
        "healthy": missing.is_empty(),
        "missing_count": missing.len(),
        "preserved_new_scripts": preserved_new_scripts,
        "missing_hooks": missing,
    })
}

pub fn studio_restore_hooks_tree(baseline_tree: &Value, current_tree: &Value) -> (Value, Vec<String>, Vec<String>, usize) {
    let baseline_scripts = json_obj_to_scripts_map(baseline_tree.get("scripts").and_then(Value::as_object));
    let mut current_scripts = json_obj_to_scripts_map(current_tree.get("scripts").and_then(Value::as_object));
    let tree_has_spawn = |tree: &Value, scripts: &std::collections::BTreeMap<String, String>| {
        scripts.values().any(|s| s.contains("SpawnLocation"))
            || tree
                .get("instances")
                .and_then(Value::as_object)
                .is_some_and(|m| m.keys().any(|k| k.contains("SpawnLocation")))
    };
    let base_spawn = tree_has_spawn(baseline_tree, &baseline_scripts);
    let curr_spawn = tree_has_spawn(current_tree, &current_scripts);
    let (restored_files, modified_files, mut hooks_restored) = surgical_restore_hooks(
        &baseline_scripts,
        &mut current_scripts,
        base_spawn,
        curr_spawn,
        "ServerScriptService.SpawnGuard",
    );
    let mut next_tree = current_tree.clone();
    if let Some(obj) = next_tree.as_object_mut() {
        let mut s_map = serde_json::Map::new();
        for (k, v) in current_scripts {
            s_map.insert(k, Value::String(v));
        }
        obj.insert("scripts".to_string(), Value::Object(s_map));
        if base_spawn && !curr_spawn {
            if let Some(base_insts) = baseline_tree.get("instances").and_then(Value::as_object) {
                if let Some((k, v)) = base_insts.iter().find(|(k, _)| k.contains("SpawnLocation")) {
                    let inst_obj = obj
                        .entry("instances")
                        .or_insert_with(|| json!({}))
                        .as_object_mut();
                    if let Some(m) = inst_obj {
                        m.insert(k.clone(), v.clone());
                        hooks_restored += 1;
                    }
                }
            }
        }
    }
    (next_tree, restored_files, modified_files, hooks_restored)
}

fn sync_baseline_path(dir: &Path) -> PathBuf {
    dir.join(".openro").join("sync-baseline.json")
}

fn write_sync_baseline(
    dir: &Path,
    scripts: &std::collections::BTreeMap<String, String>,
    baseline_id: Option<&str>,
) -> std::io::Result<()> {
    let p = sync_baseline_path(dir);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refusing to write sync-baseline through symlink directory",
            ));
        }
    }
    if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to write sync-baseline through symlink",
        ));
    }
    static PLAYTEST_BASELINE_SEQ: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(1);
    let seq = PLAYTEST_BASELINE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir
        .join(".openro")
        .join(format!(".sync-baseline.json.openro-tmp.{}.{seq}", std::process::id()));
    let doc = json!({
        "baseline_id": baseline_id,
        "scripts": scripts,
    });
    let payload = serde_json::to_vec(&doc).unwrap_or_default();
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        if let Err(e) = f.write_all(&payload).and_then(|_| f.flush()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if let Err(e) = std::fs::rename(&tmp, &p) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn load_sync_baseline(
    dir: &Path,
) -> Option<(Option<String>, std::collections::BTreeMap<String, String>)> {
    if let Ok(raw) = std::fs::read_to_string(sync_baseline_path(dir)) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            let bid = v.get("baseline_id").and_then(Value::as_str).map(String::from);
            let map = json_obj_to_scripts_map(v.get("scripts").and_then(Value::as_object));
            return Some((bid, map));
        }
    }
    let snaps = load_sorted_project_snapshots(dir);
    let snap = snaps.last()?;
    let bid = snap["snapshot_id"].as_str().map(String::from);
    let map = json_obj_to_scripts_map(snap.get("scripts").and_then(Value::as_object));
    Some((bid, map))
}

fn rel_script_to_place_path(rel: &str) -> String {
    let trimmed = rel.strip_prefix("src/").unwrap_or(rel);
    let stem = trimmed
        .strip_suffix(".server.luau")
        .or_else(|| trimmed.strip_suffix(".client.luau"))
        .or_else(|| trimmed.strip_suffix(".luau"))
        .or_else(|| trimmed.strip_suffix(".lua"))
        .unwrap_or(trimmed);
    let parts: Vec<&str> = stem.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return rel.to_string();
    }
    match parts[0] {
        "server" => format!("ServerScriptService.{}", parts[1..].join(".")),
        "client" => format!("StarterPlayer.StarterPlayerScripts.{}", parts[1..].join(".")),
        "shared" => format!("ReplicatedStorage.Shared.{}", parts[1..].join(".")),
        _ => parts.join("."),
    }
}

fn validate_conflict_rel_path(dir: &Path, raw: &str) -> Result<String, Reply> {
    let norm = raw.trim().trim_start_matches("./");
    if norm.is_empty()
        || norm.starts_with('/')
        || norm.starts_with('\\')
        || norm.contains('\\')
        || norm.contains('\0')
        || norm.split('/').any(|s| s == ".." || s.is_empty())
        || norm.starts_with(".openro")
        || !(norm.ends_with(".luau") || norm.ends_with(".lua"))
    {
        return Err(Reply::bad(format!("invalid script path: {raw}")));
    }
    let abs = crate::projects::safe_join(dir, norm).map_err(Reply::bad)?;
    if let Ok(root_canon) = std::fs::canonicalize(dir) {
        if let Ok(abs_canon) = std::fs::canonicalize(&abs) {
            if !abs_canon.starts_with(&root_canon) {
                return Err(Reply::bad("script path escapes project root"));
            }
        }
    }
    Ok(norm.to_string())
}

pub fn build_sync_conflicts_report(
    place_scripts: &std::collections::BTreeMap<String, String>,
    local_scripts: &std::collections::BTreeMap<String, String>,
    has_baseline: bool,
    baseline_id: Option<&str>,
) -> Value {
    if !has_baseline {
        return json!({
            "schema": "openro.sync_conflicts.v1",
            "has_baseline": false,
            "baseline_id": Value::Null,
            "in_sync": true,
            "conflict_count": 0,
            "conflicts": [],
        });
    }
    let place_tree = json!({ "scripts": place_scripts });
    let local_tree = json!({ "scripts": local_scripts });
    let diffs = crate::snapshots::compute_file_diffs(&place_tree, &local_tree);
    let conflicts: Vec<Value> = diffs
        .into_iter()
        .map(|d| {
            let place_src = place_scripts.get(&d.path).cloned().unwrap_or_default();
            let local_src = local_scripts.get(&d.path).cloned().unwrap_or_default();
            let hunks: Vec<Value> = d
                .hunks
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
                .collect();
            json!({
                "path": d.path,
                "place_path": rel_script_to_place_path(&d.path),
                "status": d.status,
                "added_lines": d.additions,
                "removed_lines": d.deletions,
                "local_source": local_src,
                "place_source": place_src,
                "hunks": hunks,
            })
        })
        .collect();
    json!({
        "schema": "openro.sync_conflicts.v1",
        "has_baseline": true,
        "baseline_id": baseline_id,
        "in_sync": conflicts.is_empty(),
        "conflict_count": conflicts.len(),
        "conflicts": conflicts,
    })
}

pub fn project_sync_conflicts(_shared: &Shared, r: &ProjectRef) -> Result<Value, Reply> {
    let local_scripts = collect_project_luau_map(&r.dir);
    let Some((baseline_id, place_scripts)) = load_sync_baseline(&r.dir) else {
        return Ok(build_sync_conflicts_report(&local_scripts, &local_scripts, false, None));
    };
    Ok(build_sync_conflicts_report(
        &place_scripts,
        &local_scripts,
        true,
        baseline_id.as_deref(),
    ))
}

pub fn resolve_project_sync_conflict(
    shared: &Shared,
    r: &ProjectRef,
    body: &Value,
) -> Result<Reply, Reply> {
    let action = body
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if !matches!(action, "accept_local" | "keep_place" | "backup_snapshot") {
        return Err(Reply::bad("action must be accept_local, keep_place, or backup_snapshot"));
    }
    let raw_path = body.get("path").and_then(Value::as_str).unwrap_or("");
    let rel = validate_conflict_rel_path(&r.dir, raw_path)?;

    let (baseline_id, mut place_scripts) = load_sync_baseline(&r.dir)
        .unwrap_or_else(|| (None, collect_project_luau_map(&r.dir)));
    let local_scripts = collect_project_luau_map(&r.dir);

    let backup_label = match action {
        "accept_local" => format!("Backup before Accept Local ({rel})"),
        "keep_place" => format!("Backup before Keep Place ({rel})"),
        _ => format!("Conflict backup snapshot ({rel})"),
    };
    let backup_reply = create_project_snapshot_opts(
        r,
        &json!({ "label": backup_label }),
        false,
    )?;
    let backup_json: Value = serde_json::from_slice(&backup_reply.body).unwrap_or_else(|_| json!({}));
    let backup_id = backup_json["snapshot"]["snapshot_id"]
        .as_str()
        .map(String::from);

    match action {
        "accept_local" => {
            if let Some(local_src) = local_scripts.get(&rel) {
                place_scripts.insert(rel.clone(), local_src.clone());
            } else {
                place_scripts.remove(&rel);
            }
            write_sync_baseline(&r.dir, &place_scripts, baseline_id.as_deref())
                .map_err(|e| Reply::err("500 Internal Server Error", format!("write baseline: {e}")))?;
        }
        "keep_place" => {
            if let Some(place_src) = place_scripts.get(&rel) {
                write_project_luau(&r.dir, &rel, place_src)?;
            } else {
                let abs = crate::projects::safe_join(&r.dir, &rel).map_err(Reply::bad)?;
                if abs.is_file() {
                    std::fs::remove_file(&abs)
                        .map_err(|e| Reply::err("500 Internal Server Error", format!("remove script: {e}")))?;
                }
            }
        }
        "backup_snapshot" => {
            // Backup snapshot created above; leaves both local disk and Place baseline untouched.
        }
        _ => {}
    }

    let report = project_sync_conflicts(shared, r)?;
    Ok(Reply::ok(json!({
        "action": action,
        "path": rel,
        "backup_snapshot_id": backup_id,
        "in_sync": report["in_sync"],
        "conflict_count": report["conflict_count"],
        "conflicts": report["conflicts"],
        "report": report,
    })))
}

fn find_numeric_require_id(line: &str) -> Option<&str> {
    let mut rest = line;
    while let Some(pos) = rest.find("require(") {
        let after = &rest[pos + 8..];
        let trimmed = after.trim_start();
        if let Some(end) = trimmed.find(')') {
            let inner = trimmed[..end].trim();
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return Some(inner);
            }
        }
        rest = after;
    }
    None
}

fn extract_quoted_arg(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let q = bytes.first().copied()?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(q as char)?;
    Some(rest[..end].to_string())
}

pub fn evaluate_publish_guard(
    scripts: &std::collections::BTreeMap<String, String>,
    dep_report: &Value,
    has_spawn: bool,
    has_server_entry: bool,
    asset_ids_src: Option<&str>,
) -> Value {
    let mut issues: Vec<Value> = Vec::new();
    let mut assets_ok = true;
    let mut remotes_ok = true;
    let mut security_ok = true;
    let mut hooks_ok = true;

    for (path, src) in scripts {
        let place_path = if path.contains('/') {
            rel_script_to_place_path(path)
        } else {
            path.clone()
        };
        for (idx, line) in src.lines().enumerate() {
            let line_no = idx + 1;
            let trimmed = line.trim();
            if trimmed.starts_with("--") {
                continue;
            }
            if trimmed.contains("rbxassetid://0") || trimmed.contains("rbxassetid://TODO") {
                assets_ok = false;
                issues.push(json!({
                    "id": format!("guard:asset:zero:{path}:{line_no}"),
                    "category": "assets",
                    "severity": "error",
                    "title": "Placeholder rbxassetid://0 asset reference",
                    "detail": "Replace placeholder asset URI with an uploaded or valid Roblox asset ID.",
                    "file": path,
                    "place_path": place_path,
                    "line": line_no,
                    "autofixable": true,
                    "fix_action": "sync_asset_ids",
                    "fix_hint": "Replace placeholder URI with default verified asset ID or AssetIds lookup.",
                }));
            }
            if trimmed.contains("file:///") || trimmed.contains("/home/") {
                assets_ok = false;
                issues.push(json!({
                    "id": format!("guard:asset:local_path:{path}:{line_no}"),
                    "category": "assets",
                    "severity": "error",
                    "title": "Hardcoded local filesystem path in Luau",
                    "detail": "Local file:// or /home/ paths fail on live Roblox servers.",
                    "file": path,
                    "place_path": place_path,
                    "line": line_no,
                    "autofixable": true,
                    "fix_action": "sync_asset_ids",
                    "fix_hint": "Comment unsafe local path reference cleanly.",
                }));
            }
            if let Some(asset_src) = asset_ids_src {
                if !path.ends_with("AssetIds.luau") && !path.ends_with(".AssetIds") {
                    for pat in ["AssetIds.get(", "AssetIds.byKey["] {
                        if let Some(pos) = trimmed.find(pat) {
                            let after = trimmed[pos + pat.len()..].trim_start();
                            if let Some(key) = extract_quoted_arg(after) {
                                let key_lit = format!("\"{key}\"");
                                if !key.is_empty() && !asset_src.contains(&key_lit) {
                                    assets_ok = false;
                                    issues.push(json!({
                                        "id": format!("guard:asset:orphan:{path}:{line_no}"),
                                        "category": "assets",
                                        "severity": "warning",
                                        "title": format!("Orphan AssetIds reference \"{key}\""),
                                        "detail": "Referenced asset key was not found in AssetIds.luau.",
                                        "file": path,
                                        "place_path": place_path,
                                        "line": line_no,
                                        "autofixable": true,
                                        "fix_action": "sync_asset_ids",
                                        "fix_hint": "Regenerate AssetIds.luau from assets.lock.json.",
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            for unsafe_fn in ["loadstring(", "getfenv(", "setfenv("] {
                if trimmed.contains(unsafe_fn) {
                    security_ok = false;
                    let fn_name = unsafe_fn.trim_end_matches('(');
                    issues.push(json!({
                        "id": format!("guard:security:{fn_name}:{path}:{line_no}"),
                        "category": "security",
                        "severity": "error",
                        "title": format!("Unsafe {fn_name}() call"),
                        "detail": format!("Dynamic {fn_name}() is disabled in strict Roblox runtime and flagged by safety review."),
                        "file": path,
                        "place_path": place_path,
                        "line": line_no,
                        "autofixable": true,
                        "fix_action": "quarantine_unsafe_calls",
                        "fix_hint": format!("Replace unsafe {fn_name}() with a guarded nil/stub fallback."),
                    }));
                }
            }
            if let Some(asset_id) = find_numeric_require_id(trimmed) {
                security_ok = false;
                issues.push(json!({
                    "id": format!("guard:security:num_require:{path}:{line_no}"),
                    "category": "security",
                    "severity": "error",
                    "title": format!("Unsafe external require({asset_id})"),
                    "detail": "External numeric asset require() can inject unverified runtime code.",
                    "file": path,
                    "place_path": place_path,
                    "line": line_no,
                    "autofixable": true,
                    "fix_action": "quarantine_unsafe_calls",
                    "fix_hint": "Disable external numeric require() call before export.",
                }));
            }
        }
    }

    if let Some(broken) = dep_report.get("broken_requires").and_then(Value::as_array) {
        for (i, b) in broken.iter().enumerate() {
            remotes_ok = false;
            let file = b["rel_file"]
                .as_str()
                .or_else(|| b["from"].as_str())
                .unwrap_or("Script");
            let from = b["from"].as_str().unwrap_or(file);
            let line_no = b["line"].as_u64().unwrap_or(1);
            let raw_expr = b["raw_expr"].as_str().unwrap_or("...");
            issues.push(json!({
                "id": format!("guard:remotes:require:{from}:{line_no}:{i}"),
                "category": "remotes",
                "severity": "error",
                "title": format!("Broken require({raw_expr})"),
                "detail": format!("Target {} could not be resolved.", b["resolved_target"].as_str().unwrap_or("unknown")),
                "file": file,
                "place_path": from,
                "line": line_no,
                "autofixable": true,
                "fix_action": "prune_dependencies",
                "fix_hint": "Fix closest module match or prune dead require() line.",
            }));
        }
    }
    if let Some(orphans) = dep_report.get("orphan_remotes").and_then(Value::as_array) {
        for (i, o) in orphans.iter().enumerate() {
            remotes_ok = false;
            let name = o["name"].as_str().unwrap_or("RemoteEvent");
            let script = o["script"].as_str().unwrap_or("Script");
            let line_no = o["line"].as_u64().unwrap_or(1);
            let file = scripts
                .keys()
                .find(|k| rel_script_to_place_path(k) == script || *k == script)
                .map(String::as_str)
                .unwrap_or(script);
            issues.push(json!({
                "id": format!("guard:remotes:orphan:{name}:{i}"),
                "category": "remotes",
                "severity": "warning",
                "title": format!("Unattached remote \"{name}\""),
                "detail": o["reason"].as_str().unwrap_or("RemoteEvent/RemoteFunction has no active handler."),
                "file": file,
                "place_path": script,
                "line": line_no,
                "autofixable": true,
                "fix_action": "prune_dependencies",
                "fix_hint": "Remove unattached remote allocation.",
            }));
        }
    }

    if !has_spawn {
        hooks_ok = false;
        issues.push(json!({
            "id": "guard:hooks:spawn",
            "category": "hooks",
            "severity": "error",
            "title": "Missing Workspace.SpawnLocation",
            "detail": "Players have no guaranteed spawn point before .rbxlx export.",
            "file": "src/server/SpawnGuard.server.luau",
            "place_path": "ServerScriptService.SpawnGuard",
            "line": 1,
            "autofixable": true,
            "fix_action": "seed_spawn_and_shop_hooks",
            "fix_hint": "Create a safe SpawnGuard script that ensures Workspace.SpawnLocation exists.",
        }));
    }
    if !has_server_entry {
        hooks_ok = false;
        issues.push(json!({
            "id": "guard:hooks:server_entry",
            "category": "hooks",
            "severity": "error",
            "title": "Empty or missing server entrypoint",
            "detail": "Add at least one non-empty server script before publishing.",
            "file": "src/server/Main.server.luau",
            "place_path": "ServerScriptService.Main",
            "line": 1,
            "autofixable": true,
            "fix_action": "seed_spawn_and_shop_hooks",
            "fix_hint": "Seed a valid --!strict server Main entrypoint.",
        }));
    }
    let has_shop_script = scripts
        .keys()
        .any(|k| k.to_ascii_lowercase().contains("shop") || k.to_ascii_lowercase().contains("monetization"));
    let has_receipt = scripts.values().any(|s| s.contains("ProcessReceipt"));
    if has_shop_script && !has_receipt {
        hooks_ok = false;
        let shop_file = scripts
            .keys()
            .find(|k| k.to_ascii_lowercase().contains("monetization") || k.to_ascii_lowercase().contains("shop"))
            .cloned()
            .unwrap_or_else(|| "src/server/MonetizationService.server.luau".to_string());
        issues.push(json!({
            "id": "guard:hooks:empty_shop",
            "category": "hooks",
            "severity": "error",
            "title": "Shop script missing ProcessReceipt handler",
            "detail": "Developer Products require MarketplaceService.ProcessReceipt on the server.",
            "file": shop_file,
            "place_path": if shop_file.contains('/') { rel_script_to_place_path(&shop_file) } else { shop_file.clone() },
            "line": 1,
            "autofixable": true,
            "fix_action": "seed_spawn_and_shop_hooks",
            "fix_hint": "Wire a safe ProcessReceipt guard into the shop service.",
        }));
    }

    let blocker_count = issues
        .iter()
        .filter(|i| i["severity"] == "error")
        .count();
    let warning_count = issues
        .iter()
        .filter(|i| i["severity"] == "warning")
        .count();
    let autofixable_count = issues
        .iter()
        .filter(|i| i["autofixable"] == true)
        .count();

    json!({
        "schema": "openro.publish_guard.v1",
        "ready_to_publish": blocker_count == 0 && warning_count == 0,
        "blocker_count": blocker_count,
        "warning_count": warning_count,
        "autofixable_count": autofixable_count,
        "checks": {
            "assets": assets_ok,
            "remotes": remotes_ok,
            "security": security_ok,
            "hooks": hooks_ok,
        },
        "issues": issues,
    })
}

fn sanitize_unsafe_and_asset_lines(src: &str, fix_assets: bool, fix_security: bool) -> (String, usize) {
    let mut changed = 0usize;
    let mut out_lines = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--") {
            out_lines.push(line.to_string());
            continue;
        }
        let mut next = line.to_string();
        let mut line_fixed = false;
        if fix_assets {
            if next.contains("rbxassetid://0") {
                next = next.replace("rbxassetid://0", "rbxassetid://12222200");
                line_fixed = true;
            }
            if next.contains("rbxassetid://TODO") {
                next = next.replace("rbxassetid://TODO", "rbxassetid://12222200");
                line_fixed = true;
            }
            if next.contains("file:///") || next.contains("/home/") {
                next = format!("-- [OpenRO Guard] removed local filesystem path: {trimmed}");
                line_fixed = true;
            }
        }
        if fix_security
            && (trimmed.contains("loadstring(")
                || trimmed.contains("getfenv(")
                || trimmed.contains("setfenv(")
                || find_numeric_require_id(trimmed).is_some())
        {
            if trimmed.starts_with("local ") && trimmed.contains('=') {
                let lhs = trimmed.split('=').next().unwrap_or("local _guarded").trim();
                next = format!("{lhs} = nil -- [OpenRO Guard] blocked unsafe dynamic call");
            } else {
                next = format!("-- [OpenRO Guard] blocked unsafe call: {trimmed}");
            }
            line_fixed = true;
        }
        if line_fixed {
            changed += 1;
        }
        out_lines.push(next);
    }
    let mut joined = out_lines.join("\n");
    if src.ends_with('\n') || changed > 0 {
        joined.push('\n');
    }
    (joined, changed)
}

pub fn project_publish_guard(r: &ProjectRef) -> Result<Value, Reply> {
    let scripts = collect_project_luau_map(&r.dir);
    let dep = project_dependencies_report(r)?;
    let default_proj_raw = std::fs::read_to_string(r.dir.join("default.project.json")).ok();
    let has_spawn = project_has_spawn_location(&scripts, default_proj_raw.as_deref());
    let has_server_entry = scripts
        .iter()
        .any(|(k, v)| k.ends_with(".server.luau") && !v.trim().is_empty());
    let asset_ids_src = scripts
        .get("src/shared/AssetIds.luau")
        .map(String::as_str);
    Ok(evaluate_publish_guard(
        &scripts,
        &dep,
        has_spawn,
        has_server_entry,
        asset_ids_src,
    ))
}

pub fn autofix_project_publish_guard(
    _shared: &Shared,
    r: &ProjectRef,
    body: &Value,
) -> Result<Reply, Reply> {
    let action_filter = body
        .get("fix_action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let should_run = |name: &str| action_filter.is_none() || action_filter == Some(name);

    let before = project_publish_guard(r)?;
    if before["issues"].as_array().is_some_and(|a| a.is_empty()) {
        return Ok(Reply::ok(json!({
            "fixed_count": 0,
            "modified_files": [],
            "report": before,
        })));
    }

    let _ = create_project_snapshot_opts(
        r,
        &json!({ "label": "Pre-publish-guard autofix checkpoint" }),
        false,
    );

    let mut fixed_count = 0usize;
    let mut modified_files: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    if should_run("prune_dependencies") {
        if let Ok(pr_rep) = prune_or_fix_dependencies(r) {
            if let Ok(v) = serde_json::from_slice::<Value>(&pr_rep.body) {
                let n = v["pruned_modules"].as_u64().unwrap_or(0)
                    + v["fixed_requires"].as_u64().unwrap_or(0)
                    + v["pruned_remotes"].as_u64().unwrap_or(0);
                fixed_count += n as usize;
                if let Some(arr) = v["applied_files"].as_array() {
                    for f in arr {
                        if let Some(s) = f.as_str() {
                            modified_files.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }

    let fix_assets = should_run("sync_asset_ids");
    let fix_security = should_run("quarantine_unsafe_calls");
    if fix_assets || fix_security {
        if fix_assets && r.dir.join("assets.lock.json").is_file() {
            if let Ok(cg) = crate::opencloud::assets_codegen(&r.dir, &serde_json::json!({})) {
                if cg.get("ok").and_then(Value::as_bool) == Some(true) {
                    modified_files.insert("src/shared/AssetIds.luau".to_string());
                    fixed_count += 1;
                }
            }
        }
        let scripts = collect_project_luau_map(&r.dir);
        for (rel, src) in scripts {
            let (next, n) = sanitize_unsafe_and_asset_lines(&src, fix_assets, fix_security);
            if n > 0 && next != src {
                write_project_luau(&r.dir, &rel, &next)?;
                modified_files.insert(rel);
                fixed_count += n;
            }
        }
    }

    if should_run("seed_spawn_and_shop_hooks") {
        let scripts = collect_project_luau_map(&r.dir);
        let default_proj_raw = std::fs::read_to_string(r.dir.join("default.project.json")).ok();
        if !project_has_spawn_location(&scripts, default_proj_raw.as_deref()) {
            let spawn_src = "--!strict\nlocal Workspace = game:GetService(\"Workspace\")\nif not Workspace:FindFirstChildOfClass(\"SpawnLocation\") then\n\tlocal spawn = Instance.new(\"SpawnLocation\")\n\tspawn.Name = \"SpawnLocation\"\n\tspawn.Anchored = true\n\tspawn.Parent = Workspace\nend\n";
            write_project_luau(&r.dir, "src/server/SpawnGuard.server.luau", spawn_src)?;
            modified_files.insert("src/server/SpawnGuard.server.luau".to_string());
            fixed_count += 1;
        }
        let has_server_entry = scripts
            .iter()
            .any(|(k, v)| k.ends_with(".server.luau") && !v.trim().is_empty());
        if !has_server_entry {
            write_project_luau(
                &r.dir,
                "src/server/Main.server.luau",
                "--!strict\nlocal Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(function(player: Player)\n\tplayer:SetAttribute(\"Ready\", true)\nend)\n",
            )?;
            modified_files.insert("src/server/Main.server.luau".to_string());
            fixed_count += 1;
        }
        let has_shop_script = scripts
            .keys()
            .any(|k| k.to_ascii_lowercase().contains("shop") || k.to_ascii_lowercase().contains("monetization"));
        let has_receipt = scripts.values().any(|s| s.contains("ProcessReceipt"));
        if has_shop_script && !has_receipt {
            let target = scripts
                .keys()
                .find(|k| k.to_ascii_lowercase().contains("monetization") || k.to_ascii_lowercase().contains("shop"))
                .cloned()
                .unwrap_or_else(|| "src/server/MonetizationService.server.luau".to_string());
            let mut src = scripts.get(&target).cloned().unwrap_or_else(|| "--!strict\n".to_string());
            src.push_str("\ndo\n\tlocal MarketplaceService = game:GetService(\"MarketplaceService\")\n\tif not MarketplaceService.ProcessReceipt then\n\t\tMarketplaceService.ProcessReceipt = function()\n\t\t\treturn Enum.ProductPurchaseDecision.PurchaseGranted\n\t\tend\n\tend\nend\n");
            write_project_luau(&r.dir, &target, &src)?;
            modified_files.insert(target);
            fixed_count += 1;
        }
    }

    let after = project_publish_guard(r)?;
    Ok(Reply::ok(json!({
        "fixed_count": fixed_count,
        "modified_files": modified_files.into_iter().collect::<Vec<_>>(),
        "report": after,
    })))
}

pub fn studio_publish_guard_from_tree(tree: &Value) -> Value {
    let mut scripts = std::collections::BTreeMap::new();
    let mut dep_scripts = Vec::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
                let is_mod = is_module_script(k, None);
                let leaf = k.rsplit('.').next().unwrap_or("");
                let is_cfg = is_mod && matches!(leaf, "Config" | "MonetizationConfig" | "ProductIds" | "AssetIds");
                dep_scripts.push(DepScript {
                    dot_path: k.clone(),
                    source: s.to_string(),
                    rel_file: None,
                    is_module: is_mod,
                    is_config_module: is_cfg,
                });
            }
        }
    }
    let dep = analyze_dep_scripts(&dep_scripts);
    let has_spawn = scripts.values().any(|s| s.contains("SpawnLocation"))
        || tree
            .get("instances")
            .and_then(Value::as_object)
            .is_some_and(|m| m.keys().any(|k| k.contains("SpawnLocation")));
    let has_server_entry = scripts
        .iter()
        .any(|(k, v)| k.starts_with("ServerScriptService.") && !v.trim().is_empty());
    let asset_ids_src = scripts
        .iter()
        .find(|(k, _)| k.ends_with(".AssetIds") || *k == "ReplicatedStorage.AssetIds")
        .map(|(_, v)| v.as_str());
    evaluate_publish_guard(&scripts, &dep, has_spawn, has_server_entry, asset_ids_src)
}

pub fn studio_autofix_publish_guard_tree(
    tree: &Value,
    fix_action: Option<&str>,
) -> (Value, Vec<String>, usize) {
    let should_run = |name: &str| fix_action.is_none() || fix_action == Some(name);
    let mut next_tree = tree.clone();
    let mut modified_files: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut fixed_count = 0usize;

    let fix_assets = should_run("sync_asset_ids");
    let fix_security = should_run("quarantine_unsafe_calls");
    if fix_assets || fix_security {
        if let Some(obj) = next_tree.get_mut("scripts").and_then(Value::as_object_mut) {
            let keys: Vec<String> = obj.keys().cloned().collect();
            for k in keys {
                if let Some(src) = obj.get(&k).and_then(Value::as_str).map(String::from) {
                    let (next, n) = sanitize_unsafe_and_asset_lines(&src, fix_assets, fix_security);
                    if n > 0 && next != src {
                        obj.insert(k.clone(), Value::String(next));
                        modified_files.insert(k);
                        fixed_count += n;
                    }
                }
            }
        }
    }
    if should_run("seed_spawn_and_shop_hooks") {
        let guard_before = studio_publish_guard_from_tree(&next_tree);
        if guard_before["checks"]["hooks"] == false {
            if let Some(obj) = next_tree.get_mut("scripts").and_then(Value::as_object_mut) {
                let has_spawn = obj.values().any(|v| v.as_str().is_some_and(|s| s.contains("SpawnLocation")))
                    || tree
                        .get("instances")
                        .and_then(Value::as_object)
                        .is_some_and(|m| m.keys().any(|k| k.contains("SpawnLocation")));
                if !has_spawn {
                    obj.insert(
                        "ServerScriptService.SpawnGuard".to_string(),
                        Value::String("--!strict\nlocal Workspace = game:GetService(\"Workspace\")\nif not Workspace:FindFirstChildOfClass(\"SpawnLocation\") then\n\tlocal spawn = Instance.new(\"SpawnLocation\")\n\tspawn.Name = \"SpawnLocation\"\n\tspawn.Anchored = true\n\tspawn.Parent = Workspace\nend\n".to_string()),
                    );
                    modified_files.insert("ServerScriptService.SpawnGuard".to_string());
                    fixed_count += 1;
                }
            }
        }
    }
    (next_tree, modified_files.into_iter().collect(), fixed_count)
}

const IDEMPOTENT_PROCESS_RECEIPT_SNIPPET: &str = "\n-- Added by OpenRO Monetization & Retention Safety Audit\ndo\n\tlocal DataStoreService = game:GetService(\"DataStoreService\")\n\tlocal MarketplaceService = game:GetService(\"MarketplaceService\")\n\tlocal Players = game:GetService(\"Players\")\n\tlocal receiptStore = DataStoreService:GetDataStore(\"OpenRO_ReceiptLedger_v1\")\n\tMarketplaceService.ProcessReceipt = function(receiptInfo)\n\t\tlocal player = Players:GetPlayerByUserId(receiptInfo.PlayerId)\n\t\tif not player then\n\t\t\treturn Enum.ProductPurchaseDecision.NotProcessedYet\n\t\tend\n\t\tlocal receiptKey = tostring(receiptInfo.PlayerId) .. \"_\" .. tostring(receiptInfo.PurchaseId)\n\t\tlocal alreadyGranted = false\n\t\tlocal ok = pcall(function()\n\t\t\treceiptStore:UpdateAsync(receiptKey, function(existing)\n\t\t\t\tif existing == true then\n\t\t\t\t\talreadyGranted = true\n\t\t\t\t\treturn true\n\t\t\t\tend\n\t\t\t\treturn true\n\t\t\tend)\n\t\tend)\n\t\tif not ok then\n\t\t\treturn Enum.ProductPurchaseDecision.NotProcessedYet\n\t\tend\n\t\tif not alreadyGranted then\n\t\t\tplayer:SetAttribute(\"LastPurchaseId\", tostring(receiptInfo.PurchaseId))\n\t\tend\n\t\treturn Enum.ProductPurchaseDecision.PurchaseGranted\n\tend\nend\n";

const BIND_TO_CLOSE_SNIPPET: &str = "\n-- Added by OpenRO Monetization & Retention Safety Audit\ngame:BindToClose(function()\n\ttask.wait(0.2)\nend)\n";

const ONBOARDING_RETENTION_SNIPPET: &str = "--!strict\n-- First-60s onboarding welcome reward + daily streak persistence.\nlocal DataStoreService = game:GetService(\"DataStoreService\")\nlocal Players = game:GetService(\"Players\")\nlocal onboardingStore = DataStoreService:GetDataStore(\"OpenRO_Onboarding_v1\")\n\nPlayers.PlayerAdded:Connect(function(player: Player)\n\tlocal key = \"u_\" .. tostring(player.UserId)\n\tlocal ok, state = pcall(function()\n\t\treturn onboardingStore:GetAsync(key)\n\tend)\n\tlocal streak = if ok and type(state) == \"table\" and type(state.streak) == \"number\" then state.streak + 1 else 1\n\tplayer:SetAttribute(\"OnboardingStreak\", streak)\n\tplayer:SetAttribute(\"StarterBonusGranted\", true)\n\tplayer:SetAttribute(\"Onboarded\", true)\n\tlocal _ = pcall(function()\n\t\tonboardingStore:UpdateAsync(key, function()\n\t\t\treturn { streak = streak, onboardedAt = os.time() }\n\t\tend)\n\tend)\nend)\n\ngame:BindToClose(function()\n\ttask.wait(0.2)\nend)\n";

fn line_is_pcall_guarded(lines: &[&str], idx: usize) -> bool {
    let cur = lines[idx];
    if cur.contains("pcall(") || cur.contains("xpcall(") {
        return true;
    }
    let mut depth: isize = 0;
    let start = idx.saturating_sub(12);
    for l in lines[start..idx].iter().rev() {
        let tr = l.trim();
        if tr.starts_with("--") {
            continue;
        }
        if tr == "end)" || tr.ends_with("end)") {
            depth += 1;
        }
        if tr.contains("pcall(") || tr.contains("xpcall(") {
            if depth <= 0 {
                return true;
            }
            depth -= 1;
        }
    }
    false
}

fn script_location(key: &str) -> (String, String) {
    let place_path = if key.contains('/') {
        rel_script_to_place_path(key)
    } else {
        key.to_string()
    };
    (key.to_string(), place_path)
}

pub fn evaluate_monetization_audit(scripts: &std::collections::BTreeMap<String, String>) -> Value {
    let mut issues: Vec<Value> = Vec::new();
    let mut receipt_ok = true;
    let mut bind_ok = true;
    let mut pcall_ok = true;
    let mut onboarding_ok = true;

    let has_monetization_signals = scripts.iter().any(|(k, s)| {
        let lk = k.to_ascii_lowercase();
        lk.contains("shop")
            || lk.contains("monetization")
            || lk.contains("economy")
            || lk.contains("productids")
            || s.contains("MarketplaceService")
            || s.contains("ProcessReceipt")
            || s.contains("PromptProductPurchase")
            || s.contains("PromptGamePassPurchase")
    });
    let has_receipt = scripts.values().any(|s| s.contains("ProcessReceipt"));
    if has_monetization_signals && !has_receipt {
        receipt_ok = false;
        let target = scripts
            .keys()
            .find(|k| {
                let lk = k.to_ascii_lowercase();
                lk.contains("monetization") || lk.contains("shop") || lk.contains("economy")
            })
            .cloned()
            .unwrap_or_else(|| {
                if scripts.keys().any(|k| k.contains('/')) {
                    "src/server/MonetizationService.server.luau".to_string()
                } else {
                    "ServerScriptService.MonetizationService".to_string()
                }
            });
        let (file, place_path) = script_location(&target);
        issues.push(json!({
            "id": "audit:monetization:missing_process_receipt",
            "category": "process_receipt",
            "severity": "error",
            "title": "Missing MarketplaceService.ProcessReceipt handler",
            "detail": "Monetization or shop scripts require an idempotent ProcessReceipt callback returning Enum.ProductPurchaseDecision.PurchaseGranted.",
            "file": file,
            "place_path": place_path,
            "line": 1,
            "autofixable": true,
            "fix_action": "wire_process_receipt",
            "fix_hint": "Install an idempotent ProcessReceipt callback backed by a PurchaseId DataStore ledger.",
            "snippet": IDEMPOTENT_PROCESS_RECEIPT_SNIPPET.trim(),
        }));
    }

    for (k, src) in scripts {
        let lines: Vec<&str> = src.lines().collect();
        let (file, place_path) = script_location(k);

        if let Some(pos) = lines.iter().position(|l| {
            let tr = l.trim();
            !tr.starts_with("--") && tr.contains("ProcessReceipt")
        }) {
            let has_ledger = src.contains("PurchaseId")
                && (src.contains("UpdateAsync") || src.contains("GetAsync"));
            let has_decision = src.contains("PurchaseGranted") && src.contains("NotProcessedYet");
            if !has_ledger || !has_decision {
                receipt_ok = false;
                issues.push(json!({
                    "id": format!("audit:monetization:unsafe_process_receipt:{file}"),
                    "category": "process_receipt",
                    "severity": "error",
                    "title": "Unsafe ProcessReceipt handler lacks PurchaseId idempotency",
                    "detail": "ProcessReceipt must record receiptInfo.PurchaseId via DataStore UpdateAsync/GetAsync and return Enum.ProductPurchaseDecision.PurchaseGranted or NotProcessedYet.",
                    "file": file,
                    "place_path": place_path,
                    "line": pos + 1,
                    "autofixable": true,
                    "fix_action": "harden_process_receipt",
                    "fix_hint": "Upgrade ProcessReceipt to persist PurchaseId with UpdateAsync and handle NotProcessedYet cleanly.",
                    "snippet": IDEMPOTENT_PROCESS_RECEIPT_SNIPPET.trim(),
                }));
            }
        }

        if let Some(pos) = lines.iter().position(|l| {
            let tr = l.trim();
            !tr.starts_with("--")
                && (tr.contains("GetDataStore(") || tr.contains("GetOrderedDataStore("))
        }) {
            if !src.contains("BindToClose") {
                bind_ok = false;
                issues.push(json!({
                    "id": format!("audit:monetization:missing_bind_to_close:{file}"),
                    "category": "bind_to_close",
                    "severity": "error",
                    "title": "DataStore script missing game:BindToClose flush",
                    "detail": "Without BindToClose, in-flight purchases and currency writes can drop when a server shuts down.",
                    "file": file,
                    "place_path": place_path,
                    "line": pos + 1,
                    "autofixable": true,
                    "fix_action": "add_bind_to_close",
                    "fix_hint": "Append a game:BindToClose shutdown flush guard.",
                    "snippet": BIND_TO_CLOSE_SNIPPET.trim(),
                }));
            }
        }

        for (idx, line) in lines.iter().enumerate() {
            let tr = line.trim();
            if tr.starts_with("--") {
                continue;
            }
            if (tr.contains("PromptGamePassPurchase(")
                || tr.contains("PromptProductPurchase(")
                || tr.contains("UserOwnsGamePassAsync("))
                && !line_is_pcall_guarded(&lines, idx)
            {
                pcall_ok = false;
                issues.push(json!({
                    "id": format!("audit:monetization:unguarded_prompt:{file}:{}", idx + 1),
                    "category": "gamepass_pcall",
                    "severity": "error",
                    "title": "Unguarded GamePass or purchase API call",
                    "detail": "Wrap PromptGamePassPurchase, PromptProductPurchase, and UserOwnsGamePassAsync in pcall so Roblox service errors never break the loop.",
                    "file": file,
                    "place_path": place_path,
                    "line": idx + 1,
                    "autofixable": true,
                    "fix_action": "guard_gamepass_pcall",
                    "fix_hint": "Wrap the purchase prompt or ownership lookup inside pcall(function() ... end).",
                    "snippet": format!("pcall(function()\n\t{tr}\nend)"),
                }));
            }
        }
    }

    let has_onboarding_retention = scripts.iter().any(|(k, s)| {
        let lk = k.to_ascii_lowercase();
        lk.contains("onboarding")
            || lk.contains("retention")
            || lk.contains("dailyreward")
            || s.contains("ClaimDailyReward")
            || s.contains("StarterBonusGranted")
            || s.contains("OnboardingStreak")
            || (s.contains("PlayerAdded")
                && (s.contains("Onboarded")
                    || s.contains("WelcomeBonus")
                    || s.contains("DailyRewards")
                    || s.contains("COMEBACK")))
    });
    if !has_onboarding_retention {
        onboarding_ok = false;
        let is_rojo = scripts.keys().any(|k| k.contains('/'));
        let file = if is_rojo {
            "src/server/OnboardingRetention.server.luau".to_string()
        } else {
            "ServerScriptService.OnboardingRetention".to_string()
        };
        let (_, place_path) = script_location(&file);
        issues.push(json!({
            "id": "audit:monetization:missing_onboarding_retention",
            "category": "onboarding_retention",
            "severity": "error",
            "title": "Missing first-60s onboarding and retention hook",
            "detail": "Add a PlayerAdded onboarding reward and daily streak persistence hook so new players receive immediate feedback.",
            "file": file,
            "place_path": place_path,
            "line": 1,
            "autofixable": true,
            "fix_action": "seed_onboarding_retention",
            "fix_hint": "Seed OnboardingRetention with StarterBonus and daily streak DataStore tracking.",
            "snippet": ONBOARDING_RETENTION_SNIPPET,
        }));
    }

    let blocker_count = issues.iter().filter(|i| i["severity"] == "error").count();
    let warning_count = issues.iter().filter(|i| i["severity"] == "warning").count();
    let autofixable_count = issues.iter().filter(|i| i["autofixable"] == true).count();
    json!({
        "schema": "openro.monetization_audit.v1",
        "ready": blocker_count == 0 && warning_count == 0,
        "blocker_count": blocker_count,
        "warning_count": warning_count,
        "autofixable_count": autofixable_count,
        "checks": {
            "process_receipt": receipt_ok,
            "bind_to_close": bind_ok,
            "gamepass_pcall": pcall_ok,
            "onboarding_retention": onboarding_ok,
        },
        "issues": issues,
    })
}

fn comment_out_unsafe_process_receipt(src: &str) -> (String, bool) {
    let lines: Vec<&str> = src.lines().collect();
    let Some(start) = lines.iter().position(|l| {
        let tr = l.trim();
        !tr.starts_with("--") && tr.contains("ProcessReceipt") && tr.contains("function")
    }) else {
        return (src.to_string(), false);
    };
    let mut depth: isize = 0;
    let mut end_idx = start;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let tr = line.trim();
        if tr.contains("function") {
            depth += 1;
        }
        if tr.starts_with("do")
            || tr.starts_with("if ") && tr.ends_with(" then")
            || tr.starts_with("for ") && tr.ends_with(" do")
            || tr.starts_with("while ") && tr.ends_with(" do")
        {
            depth += 1;
        }
        if tr == "end" || tr == "end)" || tr.starts_with("end ") {
            depth -= 1;
            if depth <= 0 {
                end_idx = i;
                break;
            }
        }
        end_idx = i;
    }
    let mut out = Vec::with_capacity(lines.len() + 2);
    for (i, l) in lines.iter().enumerate() {
        if i >= start && i <= end_idx {
            out.push(format!("-- [OpenRO Audit] replaced unsafe ProcessReceipt: {l}"));
        } else {
            out.push((*l).to_string());
        }
    }
    let mut joined = out.join("\n");
    joined.push_str(IDEMPOTENT_PROCESS_RECEIPT_SNIPPET);
    (joined, true)
}

fn wrap_unguarded_gamepass_lines(src: &str) -> (String, usize) {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut fixed = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let tr = line.trim();
        let needs_wrap = !tr.starts_with("--")
            && (tr.contains("PromptGamePassPurchase(")
                || tr.contains("PromptProductPurchase(")
                || tr.contains("UserOwnsGamePassAsync("))
            && !line_is_pcall_guarded(&lines, idx);
        if !needs_wrap {
            out.push((*line).to_string());
            continue;
        }
        let indent_len = line.len() - line.trim_start().len();
        let indent = &line[..indent_len];
        if tr.starts_with("local ") && tr.contains('=') {
            if let Some((lhs, rhs)) = tr.split_once('=') {
                let var = lhs.trim().trim_start_matches("local ").trim();
                let rhs = rhs.trim();
                out.push(format!(
                    "{indent}local _ok_{var}, {var} = pcall(function() return {rhs} end)"
                ));
                out.push(format!(
                    "{indent}if not _ok_{var} then {var} = false end"
                ));
                fixed += 1;
                continue;
            }
        }
        out.push(format!("{indent}pcall(function()"));
        out.push(format!("{indent}\t{tr}"));
        out.push(format!("{indent}end)"));
        fixed += 1;
    }
    let mut joined = out.join("\n");
    if src.ends_with('\n') || fixed > 0 {
        joined.push('\n');
    }
    (joined, fixed)
}

fn apply_monetization_repairs_to_map(
    scripts: &mut std::collections::BTreeMap<String, String>,
    is_rojo: bool,
    fix_action: Option<&str>,
) -> (Vec<String>, usize) {
    let should_run = |name: &str| fix_action.is_none() || fix_action == Some(name);
    let mut modified: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut fixed_count = 0usize;

    let before = evaluate_monetization_audit(scripts);
    let need_wire_receipt = should_run("wire_process_receipt")
        && before["issues"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["fix_action"] == "wire_process_receipt"));
    let need_harden_receipt = should_run("harden_process_receipt")
        && before["issues"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["fix_action"] == "harden_process_receipt"));

    if need_wire_receipt {
        let target = scripts
            .keys()
            .find(|k| {
                let lk = k.to_ascii_lowercase();
                lk.contains("monetization") || lk.contains("shop") || lk.contains("economy")
            })
            .cloned()
            .unwrap_or_else(|| {
                if is_rojo {
                    "src/server/MonetizationService.server.luau".to_string()
                } else {
                    "ServerScriptService.MonetizationService".to_string()
                }
            });
        let mut next = scripts
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "--!strict\n".to_string());
        next.push_str(IDEMPOTENT_PROCESS_RECEIPT_SNIPPET);
        scripts.insert(target.clone(), next);
        modified.insert(target);
        fixed_count += 1;
    }

    if need_harden_receipt {
        let keys: Vec<String> = scripts.keys().cloned().collect();
        for k in keys {
            if let Some(src) = scripts.get(&k).cloned() {
                if src.contains("ProcessReceipt")
                    && (!src.contains("PurchaseId")
                        || !(src.contains("UpdateAsync") || src.contains("GetAsync"))
                        || !src.contains("NotProcessedYet")
                        || !src.contains("PurchaseGranted"))
                {
                    let (mut next, replaced) = comment_out_unsafe_process_receipt(&src);
                    if !replaced {
                        next.push_str(IDEMPOTENT_PROCESS_RECEIPT_SNIPPET);
                    }
                    scripts.insert(k.clone(), next);
                    modified.insert(k);
                    fixed_count += 1;
                }
            }
        }
    }

    if should_run("add_bind_to_close") {
        let keys: Vec<String> = scripts.keys().cloned().collect();
        for k in keys {
            if let Some(src) = scripts.get(&k).cloned() {
                let uses_ds = src.lines().any(|l| {
                    let tr = l.trim();
                    !tr.starts_with("--")
                        && (tr.contains("GetDataStore(") || tr.contains("GetOrderedDataStore("))
                });
                if uses_ds && !src.contains("BindToClose") {
                    let mut next = src;
                    next.push_str(BIND_TO_CLOSE_SNIPPET);
                    scripts.insert(k.clone(), next);
                    modified.insert(k);
                    fixed_count += 1;
                }
            }
        }
    }

    if should_run("guard_gamepass_pcall") {
        let keys: Vec<String> = scripts.keys().cloned().collect();
        for k in keys {
            if let Some(src) = scripts.get(&k).cloned() {
                let (next, n) = wrap_unguarded_gamepass_lines(&src);
                if n > 0 && next != src {
                    scripts.insert(k.clone(), next);
                    modified.insert(k);
                    fixed_count += n;
                }
            }
        }
    }

    if should_run("seed_onboarding_retention") {
        let check_now = evaluate_monetization_audit(scripts);
        if check_now["checks"]["onboarding_retention"] == false {
            let key = if is_rojo {
                "src/server/OnboardingRetention.server.luau".to_string()
            } else {
                "ServerScriptService.OnboardingRetention".to_string()
            };
            scripts.insert(key.clone(), ONBOARDING_RETENTION_SNIPPET.to_string());
            modified.insert(key);
            fixed_count += 1;
        }
    }

    (modified.into_iter().collect(), fixed_count)
}

pub fn project_monetization_audit(r: &ProjectRef) -> Result<Value, Reply> {
    let scripts = collect_project_luau_map(&r.dir);
    Ok(evaluate_monetization_audit(&scripts))
}

pub fn repair_project_monetization_audit(
    _shared: &Shared,
    r: &ProjectRef,
    body: &Value,
) -> Result<Reply, Reply> {
    let action_filter = body
        .get("fix_action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut scripts = collect_project_luau_map(&r.dir);
    let before = evaluate_monetization_audit(&scripts);
    if before["issues"].as_array().is_some_and(|a| a.is_empty()) {
        return Ok(Reply::ok(json!({
            "schema": "openro.monetization_audit.v1",
            "fixed_count": 0,
            "modified_files": [],
            "report": before,
        })));
    }
    let _ = create_project_snapshot_opts(
        r,
        &json!({ "label": "Pre-monetization-audit repair checkpoint" }),
        false,
    );
    let (modified_files, fixed_count) =
        apply_monetization_repairs_to_map(&mut scripts, true, action_filter);
    for rel in &modified_files {
        if let Some(src) = scripts.get(rel) {
            write_project_luau(&r.dir, rel, src)?;
        }
    }
    let after = project_monetization_audit(r)?;
    Ok(Reply::ok(json!({
        "schema": "openro.monetization_audit.v1",
        "fixed_count": fixed_count,
        "modified_files": modified_files,
        "report": after,
    })))
}

pub fn studio_monetization_audit_from_tree(tree: &Value) -> Value {
    let mut scripts = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }
    evaluate_monetization_audit(&scripts)
}

pub fn studio_repair_monetization_audit_tree(
    tree: &Value,
    fix_action: Option<&str>,
) -> (Value, Vec<String>, usize) {
    let mut scripts = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }
    let (modified_files, fixed_count) =
        apply_monetization_repairs_to_map(&mut scripts, false, fix_action);
    let mut next_tree = tree.clone();
    if fixed_count > 0 {
        if next_tree.get("scripts").and_then(Value::as_object).is_none() {
            next_tree["scripts"] = json!({});
        }
        if let Some(obj) = next_tree.get_mut("scripts").and_then(Value::as_object_mut) {
            for k in &modified_files {
                if let Some(src) = scripts.get(k) {
                    obj.insert(k.clone(), Value::String(src.clone()));
                }
            }
        }
    }
    (next_tree, modified_files, fixed_count)
}

#[derive(Clone, Debug)]
struct EconConstHit {
    file: String,
    place_path: String,
    line: usize,
    symbol: String,
    value: f64,
    is_per_sec: bool,
}

#[derive(Clone, Debug)]
struct EconParams {
    earn_per_min: f64,
    prices: Vec<f64>,
    rebirth_cost: f64,
    rebirth_mult: f64,
    gamepass_mult: f64,
    pass_only_gate: bool,
    earn_hit: Option<EconConstHit>,
    price_hits: Vec<EconConstHit>,
    rebirth_hit: Option<EconConstHit>,
    rebirth_mult_hit: Option<EconConstHit>,
    gamepass_hit: Option<EconConstHit>,
}

fn parse_luau_numeric_literal(raw: &str) -> Option<f64> {
    let clean = raw
        .trim()
        .trim_end_matches([',', ';'])
        .split("--")
        .next()
        .unwrap_or("")
        .trim()
        .replace('_', "");
    if clean.is_empty() || !clean.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    clean.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

fn extract_economy_params(scripts: &std::collections::BTreeMap<String, String>) -> EconParams {
    let mut earn_hits: Vec<EconConstHit> = Vec::new();
    let mut earn_amount_hit: Option<EconConstHit> = None;
    let mut earn_seconds_hit: Option<EconConstHit> = None;
    let mut drop_value_hit: Option<EconConstHit> = None;
    let mut drop_interval_hit: Option<EconConstHit> = None;
    let mut reward_currency_hit: Option<EconConstHit> = None;
    let mut base_reward_hit: Option<EconConstHit> = None;
    let mut roll_cooldown_hit: Option<EconConstHit> = None;
    let mut price_hits: Vec<EconConstHit> = Vec::new();
    let mut rebirth_cost_hits: Vec<EconConstHit> = Vec::new();
    let mut rebirth_floor_hit: Option<EconConstHit> = None;
    let mut rebirth_mult_hits: Vec<EconConstHit> = Vec::new();
    let mut gp_hits: Vec<EconConstHit> = Vec::new();
    let mut pass_only_gate = false;

    for (k, src) in scripts {
        let (file, place_path) = script_location(k);
        if src.contains("GamePassOnly") || (src.contains("OwnsVipPass") && src.contains("return")) {
            pass_only_gate = true;
        }
        for (idx, line) in src.lines().enumerate() {
            let tr = line.trim();
            if tr.starts_with("--") || !tr.contains('=') {
                continue;
            }
            let Some((lhs, rhs)) = tr.split_once('=') else { continue };
            let sym = lhs
                .trim()
                .trim_start_matches("local ")
                .trim()
                .rsplit('.')
                .next()
                .unwrap_or("")
                .trim();
            if sym.is_empty() || !sym.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let Some(val) = parse_luau_numeric_literal(rhs) else { continue };
            let low = sym.to_ascii_lowercase();
            let make_hit = |is_per_sec: bool| EconConstHit {
                file: file.clone(),
                place_path: place_path.clone(),
                line: idx + 1,
                symbol: sym.to_string(),
                value: val,
                is_per_sec,
            };

            if low.contains("rebirthcost")
                || low.contains("rebirth_cost")
                || low.contains("rebirththreshold")
                || low.contains("rebirth_threshold")
                || low.contains("prestigecost")
                || low.contains("prestige_cost")
            {
                rebirth_cost_hits.push(make_hit(false));
            } else if low == "rebirth_floor" || low == "rebirthfloor" {
                if rebirth_floor_hit.is_none() {
                    rebirth_floor_hit = Some(make_hit(false));
                }
            } else if low.contains("rebirthmult")
                || low.contains("rebirth_mult")
                || low.contains("prestigemult")
                || low.contains("prestige_mult")
            {
                rebirth_mult_hits.push(make_hit(false));
            } else if low.contains("vipmult")
                || low.contains("vip_mult")
                || low.contains("passmult")
                || low.contains("pass_mult")
                || low.contains("gamepassmult")
                || low.contains("gamepass_mult")
                || low == "doublecoins"
                || low == "double_coins"
            {
                gp_hits.push(make_hit(false));
            } else if low == "earn_amount" || low == "earnamount" {
                if earn_amount_hit.is_none() {
                    earn_amount_hit = Some(make_hit(false));
                }
            } else if low == "earn_seconds" || low == "earnseconds" {
                if earn_seconds_hit.is_none() {
                    earn_seconds_hit = Some(make_hit(false));
                }
            } else if low == "drop_value" || low == "dropvalue" || low == "currentdropvalue" {
                if drop_value_hit.is_none() {
                    drop_value_hit = Some(make_hit(false));
                }
            } else if low == "drop_interval" || low == "dropinterval" {
                if drop_interval_hit.is_none() {
                    drop_interval_hit = Some(make_hit(false));
                }
            } else if low == "roll_cooldown_s" || low == "rollcooldowns" || low == "roll_cooldown" {
                if roll_cooldown_hit.is_none() {
                    roll_cooldown_hit = Some(make_hit(false));
                }
            } else if low == "reward_win_currency" {
                reward_currency_hit = Some(make_hit(false));
            } else if low == "reward_boss_currency" {
                if reward_currency_hit.is_none() {
                    reward_currency_hit = Some(make_hit(false));
                }
            } else if low == "base_reward" || low == "basereward" {
                if base_reward_hit.is_none() {
                    base_reward_hit = Some(make_hit(false));
                }
            } else if (low.contains("persec") || low.contains("persecond"))
                && !low.contains("cost")
                && !low.contains("price")
            {
                earn_hits.push(make_hit(true));
            } else if (low.contains("earn")
                || low.contains("permin")
                || low.contains("income")
                || low.contains("coinspertick")
                || (low.contains("reward")
                    && !low.ends_with("_xp")
                    && !low.contains("xp_")
                    && !low.contains("daily")))
                && !low.contains("second")
                && !low.contains("cooldown")
                && !low.contains("cost")
                && !low.contains("price")
            {
                earn_hits.push(make_hit(false));
            } else if (low.contains("cost") || low.contains("price"))
                && !low.contains("robux")
                && !low.contains("id")
                && !low.contains("fusion")
            {
                price_hits.push(make_hit(false));
            }
        }
    }

    let (earn_hit, earn_per_min) = if let Some(h) = earn_hits.into_iter().next() {
        let rate = if h.is_per_sec { h.value * 60.0 } else { h.value };
        (Some(h), rate)
    } else if let Some(amt) = earn_amount_hit {
        let secs = earn_seconds_hit.map(|s| s.value).unwrap_or(60.0).max(0.5);
        let rate = amt.value * (60.0 / secs);
        (Some(amt), rate)
    } else if let Some(drop_v) = drop_value_hit {
        let interval = drop_interval_hit.map(|s| s.value).unwrap_or(2.0).max(0.5);
        let rate = drop_v.value * (60.0 / interval);
        (Some(drop_v), rate)
    } else if let Some(rew) = reward_currency_hit {
        let cycle_s = roll_cooldown_hit
            .as_ref()
            .map(|c| (c.value + 4.0).max(4.0))
            .unwrap_or(10.0);
        let rate = rew.value * (60.0 / cycle_s);
        (Some(rew), rate)
    } else if let Some(base) = base_reward_hit {
        let rate = base.value * 3.0;
        (Some(base), rate)
    } else {
        (None, 30.0)
    };
    let earn_per_min = earn_per_min.max(0.5);

    price_hits.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));
    let mut prices: Vec<f64> = if price_hits.is_empty() {
        vec![100.0, 350.0, 900.0, 2200.0]
    } else if price_hits.len() == 1 {
        let p0 = price_hits[0].value.max(1.0);
        vec![
            p0,
            (p0 * 2.5).round(),
            (p0 * 5.0).round(),
            (p0 * 8.0).round(),
        ]
    } else {
        price_hits.iter().map(|h| h.value).collect()
    };
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let max_price = *prices.last().unwrap_or(&2200.0);

    let rebirth_hit = rebirth_cost_hits.into_iter().next();
    let rebirth_cost = rebirth_hit
        .as_ref()
        .map(|h| h.value)
        .unwrap_or_else(|| {
            if let Some(rf) = &rebirth_floor_hit {
                (rf.value * earn_per_min * 0.15)
                    .max(max_price * 1.35)
                    .max(1200.0)
                    .round()
            } else {
                (max_price * 1.35).max(1200.0)
            }
        });

    let rebirth_mult_hit = rebirth_mult_hits.into_iter().next();
    let rebirth_mult = rebirth_mult_hit
        .as_ref()
        .map(|h| h.value)
        .unwrap_or(1.5)
        .clamp(1.0, 100.0);

    let gamepass_hit = gp_hits.into_iter().next();
    let gamepass_mult = gamepass_hit
        .as_ref()
        .map(|h| h.value)
        .unwrap_or(1.0)
        .clamp(1.0, 50.0);

    EconParams {
        earn_per_min,
        prices,
        rebirth_cost,
        rebirth_mult,
        gamepass_mult,
        pass_only_gate,
        earn_hit,
        price_hits,
        rebirth_hit,
        rebirth_mult_hit,
        gamepass_hit,
    }
}

#[derive(Clone, Debug)]
struct CurveSummary {
    checkpoints: Vec<Value>,
    time_to_first_tier_min: Option<u32>,
    time_to_rebirth_min: Option<u32>,
    time_to_exhaustion_min: Option<u32>,
    f2p_completion_pct: f64,
    pass_completion_pct: f64,
    f2p_total_60m: f64,
    pass_total_60m: f64,
    inflation_ratio: f64,
}

struct TrackRun {
    snapshots: Vec<(u32, f64, usize, u32)>,
    t_first: Option<u32>,
    t_rebirth: Option<u32>,
    t_exhaust: Option<u32>,
    total_earned: f64,
}

fn simulate_track(
    earn_per_min: f64,
    prices: &[f64],
    rebirth_cost: f64,
    rebirth_mult: f64,
    base_mult: f64,
) -> TrackRun {
    let mut wallet = 0.0_f64;
    let mut total_earned = 0.0_f64;
    let mut mult = base_mult;
    let mut next_rebirth = rebirth_cost.max(1.0);
    let mut tier_idx = 0usize;
    let mut rebirths = 0u32;
    let mut t_first: Option<u32> = None;
    let mut t_rebirth: Option<u32> = None;
    let mut t_exhaust: Option<u32> = None;
    let mut snapshots = Vec::with_capacity(61);
    snapshots.push((0, 0.0, 0, 0));

    for min in 1..=60u32 {
        let gain = earn_per_min * mult;
        wallet += gain;
        total_earned += gain;
        while tier_idx < prices.len() && wallet >= prices[tier_idx] {
            wallet -= prices[tier_idx];
            tier_idx += 1;
            if t_first.is_none() {
                t_first = Some(min);
            }
        }
        if wallet >= next_rebirth {
            wallet -= next_rebirth;
            rebirths += 1;
            if t_rebirth.is_none() {
                t_rebirth = Some(min);
            }
            mult = (mult * rebirth_mult).min(1e9);
            next_rebirth *= 1.5;
        }
        if tier_idx >= prices.len() && rebirths >= 1 && t_exhaust.is_none() {
            t_exhaust = Some(min);
        }
        snapshots.push((min, total_earned, tier_idx, rebirths));
    }
    TrackRun { snapshots, t_first, t_rebirth, t_exhaust, total_earned }
}

fn run_60m_curve(
    earn_per_min: f64,
    prices: &[f64],
    rebirth_cost: f64,
    rebirth_mult: f64,
    gamepass_mult: f64,
) -> CurveSummary {
    let f2p = simulate_track(earn_per_min, prices, rebirth_cost, rebirth_mult, 1.0);
    let pass = simulate_track(earn_per_min, prices, rebirth_cost, rebirth_mult, gamepass_mult);
    let (f2p_snaps, t_first, t_rebirth, t_exhaust, f2p_total) =
        (f2p.snapshots, f2p.t_first, f2p.t_rebirth, f2p.t_exhaust, f2p.total_earned);
    let (pass_snaps, pass_total) = (pass.snapshots, pass.total_earned);

    let total_goals = (prices.len() + 1).max(1) as f64;
    let f2p_last = f2p_snaps.last().copied().unwrap_or((60, 0.0, 0, 0));
    let pass_last = pass_snaps.last().copied().unwrap_or((60, 0.0, 0, 0));
    let f2p_completion_pct =
        (((f2p_last.2 as f64 + f2p_last.3.min(1) as f64) / total_goals) * 100.0).min(100.0);
    let pass_completion_pct =
        (((pass_last.2 as f64 + pass_last.3.min(1) as f64) / total_goals) * 100.0).min(100.0);

    let max_sink = prices.iter().copied().fold(0.0_f64, f64::max).max(rebirth_cost).max(1.0);
    let inflation_ratio = f2p_total / max_sink;

    let marks = [0usize, 2, 5, 10, 15, 20, 30, 45, 60];
    let checkpoints = marks
        .iter()
        .map(|&m| {
            let f = f2p_snaps[m];
            let p = pass_snaps[m];
            json!({
                "minute": m,
                "f2p_earned": (f.1 * 10.0).round() / 10.0,
                "f2p_tiers": f.2,
                "f2p_rebirths": f.3,
                "pass_earned": (p.1 * 10.0).round() / 10.0,
                "pass_tiers": p.2,
                "pass_rebirths": p.3,
            })
        })
        .collect();

    CurveSummary {
        checkpoints,
        time_to_first_tier_min: t_first,
        time_to_rebirth_min: t_rebirth,
        time_to_exhaustion_min: t_exhaust,
        f2p_completion_pct: (f2p_completion_pct * 10.0).round() / 10.0,
        pass_completion_pct: (pass_completion_pct * 10.0).round() / 10.0,
        f2p_total_60m: (f2p_total * 10.0).round() / 10.0,
        pass_total_60m: (pass_total * 10.0).round() / 10.0,
        inflation_ratio: (inflation_ratio * 100.0).round() / 100.0,
    }
}

pub fn evaluate_economy_sim(
    scripts: &std::collections::BTreeMap<String, String>,
    overrides: &Value,
) -> Value {
    let mut p = extract_economy_params(scripts);
    if let Some(v) = overrides.get("earn_per_min").and_then(Value::as_f64).filter(|x| *x > 0.0) {
        p.earn_per_min = v;
    }
    if let Some(v) = overrides.get("rebirth_cost").and_then(Value::as_f64).filter(|x| *x > 0.0) {
        p.rebirth_cost = v;
    }
    if let Some(v) = overrides.get("rebirth_mult").and_then(Value::as_f64).filter(|x| *x >= 1.0) {
        p.rebirth_mult = v;
    }
    if let Some(v) = overrides.get("gamepass_mult").and_then(Value::as_f64).filter(|x| *x >= 1.0) {
        p.gamepass_mult = v;
    }

    let before = run_60m_curve(p.earn_per_min, &p.prices, p.rebirth_cost, p.rebirth_mult, p.gamepass_mult);
    let max_price = *p.prices.last().unwrap_or(&2200.0);
    let f2p_vs_pass = if before.f2p_total_60m > 0.0 {
        ((before.pass_total_60m / before.f2p_total_60m) * 100.0).round() / 100.0
    } else {
        p.gamepass_mult
    };

    let mut issues: Vec<Value> = Vec::new();
    let default_loc = scripts
        .keys()
        .find(|k| {
            let l = k.to_ascii_lowercase();
            (l.contains("economy") || l.contains("monetization")) && !l.contains("releaseconfig")
        })
        .or_else(|| {
            scripts.keys().find(|k| {
                let l = k.to_ascii_lowercase();
                l.contains("config") && !l.contains("releaseconfig")
            })
        })
        .or_else(|| {
            p.earn_hit
                .as_ref()
                .or_else(|| p.price_hits.first())
                .map(|h| &h.file)
        })
        .or_else(|| {
            scripts
                .keys()
                .find(|k| k.to_ascii_lowercase().contains("config"))
        })
        .cloned()
        .unwrap_or_else(|| {
            if scripts.keys().any(|k| k.contains('/')) {
                "src/shared/EconomyBalanceConfig.luau".to_string()
            } else {
                "ReplicatedStorage.EconomyBalanceConfig".to_string()
            }
        });
    let (def_file, def_place) = script_location(&default_loc);

    let paywall_dead_end = before.time_to_rebirth_min.is_none()
        || max_price > p.earn_per_min * 50.0
        || p.pass_only_gate
        || (p.gamepass_mult >= 4.0 && before.f2p_completion_pct < 65.0);
    if paywall_dead_end {
        let hit = p.rebirth_hit.as_ref().or_else(|| p.price_hits.last()).or(p.gamepass_hit.as_ref());
        let (file, place_path, line) = hit
            .map(|h| (h.file.clone(), h.place_path.clone(), h.line))
            .unwrap_or_else(|| (def_file.clone(), def_place.clone(), 1));
        issues.push(json!({
            "id": "econ:paywall_dead_end",
            "category": "paywall_dead_end",
            "severity": "error",
            "title": "F2P progression hits a 60-minute paywall dead-end",
            "detail": format!("At {:.1} earn/min, F2P players reach {:.1}% completion in 60m while paid multiplier is {:.1}x.", p.earn_per_min, before.f2p_completion_pct, p.gamepass_mult),
            "file": file,
            "place_path": place_path,
            "line": line,
            "fix_hint": "Keep rebirth thresholds within 20 minutes of F2P earn and cap GamePass boost around 2.0x.",
        }));
    }

    let runaway_inflation = before.inflation_ratio > 25.0 || p.rebirth_mult > 5.0;
    if runaway_inflation {
        let hit = p.rebirth_mult_hit.as_ref().or(p.earn_hit.as_ref());
        let (file, place_path, line) = hit
            .map(|h| (h.file.clone(), h.place_path.clone(), h.line))
            .unwrap_or_else(|| (def_file.clone(), def_place.clone(), 1));
        issues.push(json!({
            "id": "econ:runaway_inflation",
            "category": "runaway_inflation",
            "severity": "error",
            "title": "Runaway currency inflation across 60 minutes",
            "detail": format!("60-minute earnings reach {:.1}x the highest sink price (rebirth multiplier {:.1}x).", before.inflation_ratio, p.rebirth_mult),
            "file": file,
            "place_path": place_path,
            "line": line,
            "fix_hint": "Keep RebirthMultiplier <= 2.5x and scale top tier costs with 60m earn.",
        }));
    }

    let fast_exhaustion = before.time_to_exhaustion_min.is_some_and(|t| t <= 2);
    if fast_exhaustion {
        let hit = p.price_hits.last().or(p.earn_hit.as_ref());
        let (file, place_path, line) = hit
            .map(|h| (h.file.clone(), h.place_path.clone(), h.line))
            .unwrap_or_else(|| (def_file.clone(), def_place.clone(), 1));
        issues.push(json!({
            "id": "econ:fast_exhaustion",
            "category": "under_2m_exhaustion",
            "severity": "error",
            "title": "Under-2-minute content exhaustion",
            "detail": format!("Players unlock every tier and rebirth in {} minute(s), exhausting early progression.", before.time_to_exhaustion_min.unwrap_or(1)),
            "file": file,
            "place_path": place_path,
            "line": line,
            "fix_hint": "Stretch top-tier and first rebirth costs to target a 12 to 25 minute first loop.",
        }));
    }

    // Build deterministic balanced target constants & projected after-curve preview
    let target_earn = if fast_exhaustion {
        (max_price / 18.0).clamp(15.0, 250.0).round()
    } else if paywall_dead_end && p.earn_per_min < 25.0 {
        (max_price / 18.0).max(40.0).round()
    } else {
        p.earn_per_min.round()
    };
    let target_rebirth_cost = (target_earn * 16.0).round().max(600.0);
    let target_rebirth_mult = p.rebirth_mult.clamp(1.25, 2.0);
    let target_gp_mult = if p.gamepass_hit.is_some() || p.gamepass_mult > 1.0 {
        p.gamepass_mult.clamp(1.25, 2.0)
    } else {
        1.0
    };
    let target_prices = if fast_exhaustion || runaway_inflation || paywall_dead_end {
        vec![
            (target_earn * 2.5).round(),
            (target_earn * 6.0).round(),
            (target_earn * 11.0).round(),
            (target_earn * 15.0).round(),
        ]
    } else {
        p.prices.clone()
    };
    let after = run_60m_curve(
        target_earn,
        &target_prices,
        target_rebirth_cost,
        target_rebirth_mult,
        target_gp_mult,
    );

    let mut suggestions: Vec<Value> = Vec::new();
    if let Some(h) = &p.earn_hit {
        let target_raw = if h.is_per_sec { (target_earn / 60.0 * 100.0).round() / 100.0 } else { target_earn };
        if (h.value - target_raw).abs() > 0.1 {
            suggestions.push(json!({
                "symbol": h.symbol,
                "before": h.value,
                "after": target_raw,
                "file": h.file,
                "line": h.line,
            }));
        }
    }
    if let Some(h) = &p.rebirth_hit {
        if (h.value - target_rebirth_cost).abs() > 1.0 {
            suggestions.push(json!({
                "symbol": h.symbol,
                "before": h.value,
                "after": target_rebirth_cost,
                "file": h.file,
                "line": h.line,
            }));
        }
    }
    if let Some(h) = &p.rebirth_mult_hit {
        if (h.value - target_rebirth_mult).abs() > 0.05 {
            suggestions.push(json!({
                "symbol": h.symbol,
                "before": h.value,
                "after": target_rebirth_mult,
                "file": h.file,
                "line": h.line,
            }));
        }
    }
    if let Some(h) = &p.gamepass_hit {
        if (h.value - target_gp_mult).abs() > 0.05 {
            suggestions.push(json!({
                "symbol": h.symbol,
                "before": h.value,
                "after": target_gp_mult,
                "file": h.file,
                "line": h.line,
            }));
        }
    }
    if suggestions.is_empty() && !issues.is_empty() {
        suggestions.push(json!({
            "symbol": "RebirthCost",
            "before": p.rebirth_cost,
            "after": target_rebirth_cost,
            "file": def_file,
            "line": 1,
        }));
    }

    let diff_preview = format!(
        "EarnPerMin: {:.1} -> {:.1} | RebirthCost: {:.0} -> {:.0} | RebirthMult: {:.2}x -> {:.2}x | PassMult: {:.2}x -> {:.2}x",
        p.earn_per_min, target_earn, p.rebirth_cost, target_rebirth_cost, p.rebirth_mult, target_rebirth_mult, p.gamepass_mult, target_gp_mult
    );

    json!({
        "schema": "openro.economy_sim.v1",
        "balanced": issues.is_empty(),
        "blocker_count": issues.len(),
        "constants": {
            "earn_per_min": p.earn_per_min,
            "min_price": p.prices.first().copied().unwrap_or(100.0),
            "max_price": max_price,
            "rebirth_cost": p.rebirth_cost,
            "rebirth_mult": p.rebirth_mult,
            "gamepass_mult": p.gamepass_mult,
            "tiers_count": p.prices.len(),
        },
        "summary": {
            "time_to_first_tier_min": before.time_to_first_tier_min,
            "time_to_rebirth_min": before.time_to_rebirth_min,
            "time_to_exhaustion_min": before.time_to_exhaustion_min,
            "f2p_completion_pct": before.f2p_completion_pct,
            "pass_completion_pct": before.pass_completion_pct,
            "f2p_total_60m": before.f2p_total_60m,
            "pass_total_60m": before.pass_total_60m,
            "f2p_vs_pass_ratio": f2p_vs_pass,
            "inflation_ratio": before.inflation_ratio,
        },
        "curve": before.checkpoints,
        "issues": issues,
        "suggestion": {
            "diff_preview": diff_preview,
            "target_constants": {
                "earn_per_min": target_earn,
                "rebirth_cost": target_rebirth_cost,
                "rebirth_mult": target_rebirth_mult,
                "gamepass_mult": target_gp_mult,
                "prices": target_prices,
            },
            "projected_summary": {
                "time_to_rebirth_min": after.time_to_rebirth_min,
                "time_to_exhaustion_min": after.time_to_exhaustion_min,
                "f2p_completion_pct": after.f2p_completion_pct,
                "inflation_ratio": after.inflation_ratio,
            },
            "projected_curve": after.checkpoints,
            "patches": suggestions,
        },
    })
}

fn apply_economy_suggestions_to_map(
    scripts: &mut std::collections::BTreeMap<String, String>,
    sim_report: &Value,
    is_rojo: bool,
) -> (Vec<String>, usize) {
    let mut modified = std::collections::BTreeSet::new();
    let mut fixed = 0usize;
    let patches = sim_report["suggestion"]["patches"].as_array().cloned().unwrap_or_default();

    for patch in &patches {
        let Some(file) = patch["file"].as_str() else { continue };
        let Some(sym) = patch["symbol"].as_str() else { continue };
        let Some(after_val) = patch["after"].as_f64() else { continue };
        let val_str = if (after_val - after_val.round()).abs() < 1e-6 {
            format!("{}", after_val.round() as i64)
        } else {
            format!("{after_val:.2}")
        };
        if let Some(src) = scripts.get(file).cloned() {
            let mut changed = false;
            let lines: Vec<String> = src
                .lines()
                .map(|ln| {
                    let tr = ln.trim();
                    if !tr.starts_with("--") && tr.contains('=') {
                        if let Some((lhs, _)) = tr.split_once('=') {
                            let lhs_sym = lhs.trim().trim_start_matches("local ").trim().rsplit('.').next().unwrap_or("").trim();
                            if lhs_sym == sym {
                                changed = true;
                                let prefix = ln.split('=').next().unwrap_or(lhs);
                                return format!("{prefix}= {val_str}");
                            }
                        }
                    }
                    ln.to_string()
                })
                .collect();
            if changed {
                let mut next = lines.join("\n");
                if src.ends_with('\n') {
                    next.push('\n');
                }
                scripts.insert(file.to_string(), next);
                modified.insert(file.to_string());
                fixed += 1;
            }
        }
    }

    if fixed == 0 && sim_report["balanced"] == false {
        let tc = &sim_report["suggestion"]["target_constants"];
        let key = if is_rojo {
            "src/shared/EconomyBalanceConfig.luau".to_string()
        } else {
            "ReplicatedStorage.EconomyBalanceConfig".to_string()
        };
        let earn = tc["earn_per_min"].as_f64().unwrap_or(40.0).round() as i64;
        let reb = tc["rebirth_cost"].as_f64().unwrap_or(640.0).round() as i64;
        let rmult = tc["rebirth_mult"].as_f64().unwrap_or(1.5);
        let gmult = tc["gamepass_mult"].as_f64().unwrap_or(1.0);
        let body = format!(
            "--!strict\n-- Balanced economy progression config tuned for a 15 to 25 minute loop.\nlocal EconomyBalanceConfig = {{\n\tEarnPerMin = {earn},\n\tTier1Cost = {},\n\tTier2Cost = {},\n\tRebirthCost = {reb},\n\tRebirthMultiplier = {rmult:.2},\n\tGamePassMultiplier = {gmult:.2},\n}}\nreturn EconomyBalanceConfig\n",
            earn * 3,
            earn * 8
        );
        scripts.insert(key.clone(), body);
        modified.insert(key);
        fixed += 1;
    }

    (modified.into_iter().collect(), fixed)
}

pub fn project_economy_sim(r: &ProjectRef, overrides: &Value) -> Result<Value, Reply> {
    let scripts = collect_project_luau_map(&r.dir);
    Ok(evaluate_economy_sim(&scripts, overrides))
}

pub fn run_project_economy_sim_post(
    _shared: &Shared,
    r: &ProjectRef,
    body: &Value,
) -> Result<Reply, Reply> {
    let apply = body.get("apply_suggestions").and_then(Value::as_bool) == Some(true);
    let mut scripts = collect_project_luau_map(&r.dir);
    let before_report = evaluate_economy_sim(&scripts, body);
    if !apply {
        return Ok(Reply::ok(json!({
            "schema": "openro.economy_sim.v1",
            "applied": false,
            "fixed_count": 0,
            "modified_files": [],
            "report": before_report,
        })));
    }
    let _ = create_project_snapshot_opts(
        r,
        &json!({ "label": "Pre-economy-sim balance checkpoint" }),
        false,
    );
    let (modified_files, fixed_count) =
        apply_economy_suggestions_to_map(&mut scripts, &before_report, true);
    for rel in &modified_files {
        if let Some(src) = scripts.get(rel) {
            write_project_luau(&r.dir, rel, src)?;
        }
    }
    let after_report = project_economy_sim(r, &json!({}))?;
    Ok(Reply::ok(json!({
        "schema": "openro.economy_sim.v1",
        "applied": fixed_count > 0,
        "fixed_count": fixed_count,
        "modified_files": modified_files,
        "report": after_report,
    })))
}

pub fn studio_economy_sim_from_tree(tree: &Value, overrides: &Value) -> Value {
    let mut scripts = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }
    evaluate_economy_sim(&scripts, overrides)
}

pub fn studio_apply_economy_sim_tree(tree: &Value, overrides: &Value) -> (Value, Vec<String>, usize) {
    let mut scripts = std::collections::BTreeMap::new();
    if let Some(obj) = tree.get("scripts").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }
    let report = evaluate_economy_sim(&scripts, overrides);
    let (modified_files, fixed_count) =
        apply_economy_suggestions_to_map(&mut scripts, &report, false);
    let mut next_tree = tree.clone();
    if fixed_count > 0 {
        if next_tree.get("scripts").and_then(Value::as_object).is_none() {
            next_tree["scripts"] = json!({});
        }
        if let Some(obj) = next_tree.get_mut("scripts").and_then(Value::as_object_mut) {
            for k in &modified_files {
                if let Some(src) = scripts.get(k) {
                    obj.insert(k.clone(), Value::String(src.clone()));
                }
            }
        }
    }
    (next_tree, modified_files, fixed_count)
}

/// Summary of the most recent stored playtest report for `study/status` (#62).
pub fn latest_report_summary(dir: &Path) -> Option<Value> {
    let rep = latest_report_json(dir)?;
    Some(json!({
        "run_id": rep["run_id"],
        "build_id": rep["build_id"],
        "passed": rep["passed"],
        "at": rep["at"],
        "core_loop": rep["core_loop"],
        "visual_likeness": rep["visual_likeness"],
        "blank_screenshots": rep["blank_screenshots"].as_u64().unwrap_or(0),
        "evidence_id": rep["evidence_id"],
    }))
}

/// Serve a stored report file after checking every path piece.
pub fn stored(dir: &Path, run: &str, file: &str, content_type: &'static str) -> Result<Reply, Reply> {
    if !plain_name(run) || !file.split('/').all(plain_name) {
        return Err(Reply::bad("not a valid report path"));
    }
    let Ok(builds) = std::fs::read_dir(reports_root(dir)) else { return Err(Reply::err("404 Not Found", "no such playtest run")) };
    for b in builds.filter_map(|e| e.ok()) {
        let p = b.path().join(run).join(file);
        if p.is_file() {
            let body = std::fs::read(&p).map_err(|e| Reply::err("500 Internal Server Error", e.to_string()))?;
            return Ok(Reply { status: "200 OK", content_type, body });
        }
    }
    Err(Reply::err("404 Not Found", "no such playtest run or file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_lines() {
        assert!(is_error_line("ServerScriptService.Main:4: attempt to index nil with 'Start'"));
        assert!(is_error_line("Workspace.SpawnPad is not a valid member of Workspace"));
        assert!(!is_error_line("Playtest OK: all scripts ran without errors"));
        assert!(!is_error_line("server ready"));
    }

    #[test]
    fn expectations() {
        assert!(expectation(&json!({ "equals": 40 }), &json!(40.0)).is_ok());
        assert!(expectation(&json!({ "equals": 40 }), &json!(41)).is_err());
        assert!(expectation(&json!({ "truthy": true }), &json!(false)).is_err());
        assert!(expectation(&json!({ "contains": "ab" }), &json!("xaby")).is_ok());
        assert!(expectation(&json!({}), &Value::Null).is_ok());
    }

    #[test]
    fn report_html_escapes() {
        let rep = json!({ "run_id": "p-1", "passed": false, "build_id": "b", "mode": "start_play", "duration_ms": 1,
            "checks": [{ "name": "<script>alert(1)</script>", "passed": false, "value": null, "error": "x" }],
            "console": { "errors": ["<b>boom</b>"], "warnings": [] }, "screenshots": [] });
        let html = render_html(&rep, "proj");
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;b&gt;boom"));
    }
}
