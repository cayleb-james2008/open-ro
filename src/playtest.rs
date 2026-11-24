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

    let rel = |f: &str| format!("build/reports/{build_id}/{run_id}/{f}");
    let at = projects::now_ms();
    let mut report = json!({
        "run_id": run_id, "passed": passed, "build_id": build_id, "mode": mode,
        "project": r.id, "at": at, "duration_ms": started.elapsed().as_millis() as u64,
        "kept": false, "cached": false, "fingerprint": fp,
        "actions": action_results,
        "checks": check_results,
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

/// Summary of the most recent stored playtest report for `study/status` (#62).
pub fn latest_report_summary(dir: &Path) -> Option<Value> {
    let mut best: Option<(String, Value)> = None;
    let Ok(builds) = std::fs::read_dir(reports_root(dir)) else { return None };
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
    let (_, rep) = best?;
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
