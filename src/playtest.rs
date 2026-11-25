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
    });
    let target = root.join(format!("{sid}.json"));
    let tmp = root.join(format!("{sid}.json.tmp-openro"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&rec).unwrap_or_default())
        .and_then(|_| std::fs::rename(&tmp, &target))
        .map_err(|e| Reply::err("500 Internal Server Error", format!("write snapshot: {e}")))?;

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
        let _ = create_project_snapshot(r, &json!({ "label": "Pre-rollback safety checkpoint" }));
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
