//! The agent loop.
//!
//! One user ask runs a model-driven loop. The model replies with exactly
//! one JSON step at a time:
//!   {"skill": "<rune skill>", "args": {...}}            — generate Luau
//!   {"tool": "write_script"|"delete_script"|"create_instance"|
//!            "delete_instance"|"playtest"|"get_console"|
//!            "get_place_tree", "args": {...}}           — Studio bridge
//!   {"done": true, "summary": "..."}                    — finish
//!
//! Luau comes ONLY from Rune skills: `write_script` writes a skill's
//! generated source (by skill name, or the most recent generation).
//! There is no way for the model to hand the loop raw Luau — an earlier
//! version accepted `args.source` directly, which bypassed the whole
//! skill system; that path is gone and the prompt no longer offers it.
//!
//! Before the first change to a path in an ask, the loop checkpoints
//! that path's previous state (script source, instance record, or its
//! absence). The change set belongs to the CHAT the ask ran in, and it
//! is harvested on EVERY exit path — success, honest failure, or a
//! mid-ask error — so `revert_chat` can always undo the most recent
//! change-making ask in that chat, even one that went wrong.
//!
//! Honesty rules the loop enforces itself, rather than trusting the
//! model or the bridge:
//!   - a `done` with changes that no playtest has passed since is fed
//!     back, not accepted;
//!   - a playtest verdict is read only in the documented bridge format
//!     (JSON `{"status": "passed"|"failed"}`, or the bare words
//!     "passed"/"failed"); anything else is "unproven", never a pass;
//!   - bridge tool errors surface as errors (see mcp.rs) and are fed
//!     back to the model — they are never narrated as success;
//!   - after MAX_REPAIRS failed playtests the ask ends in an honest
//!     failure naming the console output, change set still revertable.

use crate::mcp::McpClient;
use crate::model::ModelClient;
use crate::place::{
    serialize_rbxlx, CreatedPreset, DeleteResult, DuplicateResult, LocalPlace, PlaceBackend,
    ScriptUpdateResult,
};
use crate::rojo_export::{build_rojo_ship_pack, ShipPackAsset, ShipPackBundle};
use crate::skills;
use crate::snapshots::{compute_diff, PlaceDiff};
use crate::sse::BuildEventHub;
use crate::studio_stream::{StudioActionInput, StudioActionKind, StudioFrame, StudioStreamHub};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// 40 steps: a one-shot complete game (many skills, instances, a
/// playtest, repairs) legitimately needs more than a one-script ask.
const MAX_STEPS: usize = 40;
const MAX_REPAIRS: usize = 3;
/// Cap custom Luau writes (`write_luau` / `multi_edit`) cleanly at 512 KiB.
const MAX_LUAU_BYTES: usize = 512 * 1024;

/// Composer execution mode for `POST /api/chat` (`Agent | Plan | Ask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AskMode {
    #[default]
    Agent,
    Plan,
    Ask,
}

impl AskMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "plan" => AskMode::Plan,
            "ask" => AskMode::Ask,
            _ => AskMode::Agent,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AskMode::Agent => "agent",
            AskMode::Plan => "plan",
            AskMode::Ask => "ask",
        }
    }
}

/// Optional per-request RoPilot execution options for `POST /api/chat`.
#[derive(Clone, Debug, Default)]
pub struct AskOptions {
    pub mode: AskMode,
    pub reasoning: String,
    pub context_paths: Vec<String>,
    pub review_diffs: bool,
}

/// Outcome of a staged proposal run (`mode == "plan"` or `review_diffs == true`) executed
/// on an isolated shadow `LocalPlace` with zero live Studio mutations.
#[derive(Debug)]
pub struct StagedAskOutcome {
    pub outcome: RunOutcome,
    pub before_tree: Value,
    pub after_tree: Value,
    pub diagnostics: Value,
}

/// Returned by `revert_chat` when the chat has no change set waiting.
pub const NOTHING_TO_REVERT: &str = "nothing to revert";
/// Returned by `redo_chat` when the chat has no undone change set waiting.
pub const NOTHING_TO_REDO: &str = "nothing to redo";

/// Distill a large skill/dossier prompt block down to its highest-signal lines
/// (pack headers, active niche, Craft Bar, Anti-Slop Rules, and Automatic Twists)
/// so local loopback models with smaller context windows never truncate the core
/// JSON tool protocol.
fn distill_prompt_for_local_model(raw: &str, max_chars: usize) -> String {
    if raw.len() <= max_chars {
        return raw.to_string();
    }
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let high_signal = t.starts_with("##")
            || t.starts_with("Active Market Niche:")
            || t.starts_with("Top-Trending Craft Bar:")
            || t.starts_with("Craft Bar:")
            || t.starts_with("Visual Fidelity")
            || t.starts_with("Title-Epithet")
            || t.starts_with("8. ")
            || t.starts_with("- **Twist")
            || t.starts_with("- Anti-Slop")
            || t.contains("roblox-market-taste");
        if !high_signal {
            continue;
        }
        if out.len() + t.len() + 1 > max_chars {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(t);
    }
    if out.is_empty() {
        let mut end = max_chars.min(raw.len());
        while end > 0 && !raw.is_char_boundary(end) {
            end -= 1;
        }
        raw[..end].to_string()
    } else {
        out
    }
}

/// Official Roblox Studio MCP tools exposed via `POST /api/studio/tools/:tool`
/// and natively usable by the OpenRO agent when a live Studio MCP bridge is attached.
pub const OFFICIAL_STUDIO_MCP_TOOLS: [&str; 13] = [
    "list_roblox_studios",
    "get_studio_state",
    "search_game_tree",
    "inspect_instance",
    "script_read",
    "script_grep",
    "multi_edit",
    "execute_luau",
    "get_console_output",
    "screen_capture",
    "start_stop_play",
    "user_mouse_input",
    "user_keyboard_input",
];

pub fn is_official_studio_tool(name: &str) -> bool {
    OFFICIAL_STUDIO_MCP_TOOLS.contains(&name)
}

/// The system prompt, generated from the skill registry so the model is
/// told exactly the game systems and RoPilot Studio tools that exist.
pub fn system_prompt() -> String {
    let mut skill_lines = String::new();
    for def in skills::registry() {
        skill_lines.push_str(&format!(
            "- {{\"skill\": \"{}\", \"args\": {}}} — {}\n",
            def.name, def.params, def.description
        ));
    }
    format!(
        "You are OpenRO, a RoPilot-grade Roblox Studio Copilot agent that builds and edits complete Roblox games. \
The user describes a game change in plain English and you inspect, author, and verify it. \
Reply with exactly ONE JSON object per turn, no prose outside the JSON. You may batch multiple tool calls in one turn with {{\"steps\": [{{\"tool\": \"...\", \"args\": {{...}}}}, ...], \"done\": true, \"summary\": \"...\"}} (aborts on first error).\n\n\
VERIFIED GAME-SYSTEM SKILLS (instant battle-tested Roblox systems):\n{skill_lines}\n\
STUDIO & WHOLE-GAME TOOLS:\n\
- {{\"tool\": \"write_script\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"skill\": \"<skill name>\"}}}} writes a generated skill script (\"use_generated\": true writes the most recent skill result)\n\
- {{\"tool\": \"write_luau\", \"args\": {{\"path\": \"ServerScriptService.CustomSystem\", \"source\": \"-- custom Luau...\"}}}} writes custom server-authoritative Luau checked by the Static Luau Prover before saving\n\
- {{\"tool\": \"multi_edit\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"edits\": [{{\"old_str\": \"...\", \"new_str\": \"...\"}}]}}}} applies targeted multi-hunk edits to an existing Luau script (Static Luau Prover gated)\n\
- {{\"tool\": \"create_instance\", \"args\": {{\"class_name\": \"SpawnLocation\", \"name\": \"SpawnPad\", \"parent\": \"Workspace\", \"properties\": {{...}}}}}} creates or updates a place instance\n\
- {{\"tool\": \"delete_instance\", \"args\": {{\"path\": \"Workspace.SpawnPad\"}}}} deletes an instance\n\
- {{\"tool\": \"delete_script\", \"args\": {{\"path\": \"ServerScriptService.Name\"}}}} deletes a script\n\
- {{\"tool\": \"get_place_tree\", \"args\": {{}}}} lists the place's scripts and instances\n\
- {{\"tool\": \"search_game_tree\", \"args\": {{\"query\": \"...\"}}}} searches instances and scripts across the place\n\
- {{\"tool\": \"inspect_instance\", \"args\": {{\"path\": \"Workspace.SpawnPad\"}}}} inspects properties, children, or script source\n\
- {{\"tool\": \"script_read\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"start_line\": 1, \"end_line\": 120}}}} reads a Luau script\n\
- {{\"tool\": \"script_grep\", \"args\": {{\"query\": \"RemoteEvent\"}}}} searches across all Luau scripts\n\
- {{\"tool\": \"execute_luau\", \"args\": {{\"code\": \"...\"}}}} executes Luau inside live Roblox Studio\n\
- {{\"tool\": \"get_console_output\", \"args\": {{}}}} / {{\"tool\": \"get_console\", \"args\": {{}}}} reads Studio Output console lines\n\
- {{\"tool\": \"screen_capture\", \"args\": {{}}}} captures the live Studio 3D viewport\n\
- {{\"tool\": \"start_stop_play\", \"args\": {{\"mode\": \"start_play|run_server|stop\"}}}} controls live Studio play mode\n\
- {{\"tool\": \"user_mouse_input\", \"args\": {{\"action\": \"click\", \"x\": 0.5, \"y\": 0.5}}}} and {{\"tool\": \"user_keyboard_input\", \"args\": {{\"key\": \"W\"}}}} send live input to Studio\n\
\nPROJECT FILE, BUILD & VERIFICATION TOOLS (for Rojo projects on disk):\n\
- {{\"tool\": \"project_list\", \"args\": {{}}}} lists workspace projects\n\
- {{\"tool\": \"project_files\", \"args\": {{\"dir\": \"src\"}}}} lists files/dirs inside the active project\n\
- {{\"tool\": \"project_read_file\", \"args\": {{\"path\": \"src/shared/Config.luau\", \"start_line\": 1, \"end_line\": 200}}}} reads a project file\n\
- {{\"tool\": \"project_write_file\", \"args\": {{\"path\": \"src/server/Main.server.luau\", \"content\": \"...\"}}}} writes a project file (auto-checked by Static Luau Prover)\n\
- {{\"tool\": \"project_edit_file\", \"args\": {{\"path\": \"src/server/Main.server.luau\", \"edits\": [{{\"old\": \"...\", \"new\": \"...\"}}]}}}} atomic multi-hunk edit (auto-checked)\n\
- {{\"tool\": \"project_delete_file\", \"args\": {{\"path\": \"src/old.luau\"}}}} deletes a project file\n\
- {{\"tool\": \"project_grep\", \"args\": {{\"query\": \"RemoteEvent\", \"dir\": \"src\"}}}} searches across project files\n\
- {{\"tool\": \"project_update_manifest\", \"args\": {{\"patch\": {{\"assets\": {{\"authorised\": [\"art/cards/**\"]}}}}}}}} safely updates openro.project.json (cannot auto-approve product prices)\n\
- {{\"tool\": \"project_check\", \"args\": {{}}}} runs selene, stylua --check, and static Luau checks across the project\n\
- {{\"tool\": \"project_run_lune\", \"args\": {{\"script\": \"tools/sim.luau\"}}}} runs a project-relative Lune test/sim script with timeout\n\
- {{\"tool\": \"sandbox_playtest\", \"args\": {{\"timeout_secs\": 15}}}} runs the headless local Roblox Luau runtime sandbox (<2s, no Studio needed; executes server/client scripts, remotes, DataStores, ProcessReceipt idempotency, PlayerGui)\n\
- {{\"tool\": \"project_build\", \"args\": {{\"engine\": \"auto\"}}}} builds the project to .rbxlx\n\
- {{\"tool\": \"project_scene_get\", \"args\": {{}}}} returns the unified Rojo workspace bundle (scripts, Workspace instances, meshes, lighting, lights, terrain, diagnostics)\n\
- {{\"tool\": \"project_scene_mutate\", \"args\": {{\"ops\": [{{\"op\": \"upsert|delete|duplicate|reparent|set_lighting|set_terrain|undo|redo\", \"path\": \"Workspace.SpawnPad\", \"class_name\": \"Part|Model|SpawnLocation|MeshPart|PointLight|SpotLight|SurfaceLight|ParticleEmitter|Sound|SurfaceAppearance\", \"properties\": {{\"Position\": [0, 1, 0], \"Size\": [8, 1, 8], \"Anchored\": true}}}}]}}}} mutates scene/workspace.model.json, scene/terrain.json, & Lighting (auto-wiring default.project.json; supports set_terrain {{enabled?, generator?: {{kind:\"flat|hills|island|canyon\",seed,amplitude,water_level}}, regions?, paint_box?: {{id?,min,max,material}}, clear?}} and reparent {{path,new_parent}})\n\
- {{\"tool\": \"gui_save\", \"args\": {{\"slug\": \"main-hud\", \"preset\": \"rpg\"}}}} saves ScreenDoc v1 to ui/<slug>.screen.json and runs multi-device safe-area checks\n\
- {{\"tool\": \"gui_compile\", \"args\": {{\"slug\": \"main-hud\", \"emit_model_json\": false}}}} compiles ScreenDoc v1 to --!strict Luau create(parent, callbacks) + Rojo .model.json\n\
- {{\"tool\": \"project_sync\", \"args\": {{\"prune\": false}}}} one-way syncs the project to Studio\n\
- {{\"tool\": \"project_playtest\", \"args\": {{\"sync_first\": true, \"suite\": \"playtest/suite.json\"}}}} runs the Playtest Runner (checks, screenshots, console errors)\n\n\
RESEARCH, ART, 3D, AUDIO & UPLOAD TOOLS:\n\
- {{\"tool\": \"research_roblox_game\", \"args\": {{\"target\": \"<place/universe id or URL>\"}}}} fetches real Roblox metadata & thumbnails into refs/<universeId>/\n\
- {{\"tool\": \"web_search\", \"args\": {{\"query\": \"...\", \"limit\": 5}}}} and {{\"tool\": \"web_fetch\", \"args\": {{\"url\": \"https://...\"}}}} read-only public web research\n\
- {{\"tool\": \"reference_search\", \"args\": {{\"query\": \"...\", \"character\": \"...\", \"show\": \"...\"}}}} gathers action reference shots into gitignored art/references/\n\
- {{\"tool\": \"art_generate\", \"args\": {{\"prompt\": \"single-line eye-level three-quarter prompt\", \"out_path\": \"art/cards/x/art.png\", \"references\": [\"art/references/...\"]}}}} generates art (ChatGPT shared browser default; never Higgsfield)\n\
- {{\"tool\": \"image_process\", \"args\": {{\"input\": \"...\", \"output\": \"...\", \"ops\": [...]}}}} crops/resizes/composites/removes background on PNGs\n\
- {{\"tool\": \"art_pack_card\", \"args\": {{\"card_id\": \"...\"}}}} packs card art/icon/full variants + contact sheet\n\
- {{\"tool\": \"mesh_generate\", \"args\": {{\"name\": \"PropName\", \"description\": \"...\", \"script\": \"...\"}}}} (alias `blender_generate`) generates smart-UV AO-baked .obj + PNG + manifest.json (<=20k tris) into assets/meshes/\n\
- {{\"tool\": \"audio_generate\", \"args\": {{\"name\": \"SfxName\", \"preset\": \"...\", \"recipe\": {{...}}, \"duration_ms\": 1200}}}} synthesizes 44.1kHz WAV into assets/audio/\n\
- {{\"tool\": \"adapter_list\", \"args\": {{\"capability\": \"image_gen|reference_search|mesh_gen|audio_gen|custom\"}}}} lists installed CLI/HTTP tool adapters and availability\n\
- {{\"tool\": \"adapter_invoke\", \"args\": {{\"capability\": \"...\", \"adapter_id\": \"...?\", \"params\": {{...}}}}}} runs an installed CLI/HTTP adapter (or capability priority chain)\n\
- {{\"tool\": \"opencloud_status\", \"args\": {{}}}} / {{\"tool\": \"creator_hub_status\", \"args\": {{\"check_login\": true, \"save_creator\": true}}}} checks Open Cloud key/creator & open loopback Chromium Creator Hub status\n\
- {{\"tool\": \"creator_hub_setup\", \"args\": {{\"ensure_experience\": true}}}} / {{\"tool\": \"creator_hub_configure\", \"args\": {{\"private_servers\": {{\"enabled\": true, \"price_robux\": 0}}}}}} automates Creator Hub via loopback CDP (stops cleanly on `action_required` — never bypass login/2FA/CAPTCHA)\n\
- {{\"tool\": \"assets_authorise\", \"args\": {{\"paths\": [\"art/cards/**\"]}}}} / {{\"tool\": \"assets_upload\", \"args\": {{\"paths\": [...]}}}} / {{\"tool\": \"assets_codegen\", \"args\": {{}}}} authorises, uploads, and generates `src/shared/AssetIds.luau`\n\
- {{\"tool\": \"products_sync\", \"args\": {{}}}} / {{\"tool\": \"products_codegen\", \"args\": {{}}}} syncs approved Game Passes/Dev Products and generates `src/shared/ProductIds.luau`\n\
- {{\"tool\": \"universe_get\", \"args\": {{}}}} / {{\"tool\": \"universe_set\", \"args\": {{\"settings\": {{\"displayName\": \"...\", \"description\": \"...\"}}}}}} reads/updates Roblox Experience metadata\n\
- {{\"tool\": \"liveops_datastore_list\", \"args\": {{\"store\": \"...?\", \"scope\": \"global\", \"prefix\": \"...?\"}}}} / {{\"tool\": \"liveops_datastore_get\", \"args\": {{\"store\": \"...\", \"scope\": \"global\", \"key\": \"...\"}}}} inspects Open Cloud v2 DataStores\n\
- {{\"tool\": \"liveops_datastore_set\", \"args\": {{\"store\": \"...\", \"scope\": \"global\", \"key\": \"...\", \"value\": {{...}}, \"expected_revision\": \"...\", \"confirm\": true}}}} writes a DataStore entry with optimistic concurrency & backup (REQUIRES owner `confirm: true`)\n\
- {{\"tool\": \"liveops_messaging_publish\", \"args\": {{\"topic\": \"...\", \"message\": {{...}}, \"confirm\": true}}}} broadcasts via MessagingService (REQUIRES owner `confirm: true`)\n\
- {{\"tool\": \"liveops_remote_config_push\", \"args\": {{\"config\": {{...}}, \"generate_luau\": true, \"push_to_datastore\": false}}}} updates `liveops/remote-config.json` & `src/shared/LiveConfig.luau` (`push_to_datastore: true` REQUIRES `confirm: true`)\n\
- {{\"tool\": \"liveops_telemetry_summary\", \"args\": {{}}}} summarizes player onboarding/funnel drop-off, ARPDAU Robux, and top economy sinks\n\
- {{\"tool\": \"project_publish\", \"args\": {{\"versionType\": \"Saved\"}}}} publishes built `.rbxlx` (`versionType: \"Published\"` requires `confirm: true` ONLY after explicit owner approval)\n\
- {{\"tool\": \"evidence_record\", \"args\": {{\"summary\": \"...\", \"confidence\": \"observed|derived|inferred\", \"kind\": \"...\", \"source\": \"...\"}}}} appends canonical SHA-256 evidence (`ev_*`) to `.openro/investigation/evidence.jsonl`\n\
- {{\"tool\": \"evidence_list\", \"args\": {{\"confidence\": \"...?\", \"kind\": \"...?\", \"limit\": 50}}}} lists recorded investigation evidence\n\
- {{\"tool\": \"unknown_add\", \"args\": {{\"title\": \"...\", \"question\": \"...\", \"severity\": \"high\", \"domain\": \"mechanics\"}}}} logs an open unknown (`unk_*`)\n\
- {{\"tool\": \"unknown_update\", \"args\": {{\"id\": \"unk_...\", \"expected_revision\": 1, \"status\": \"resolved\", \"resolution\": \"...\", \"evidence_for\": [\"ev_...\"]}}}} updates/resolves an unknown (resolving fails closed without `resolution` + verified `ev_*` IDs)\n\
- {{\"tool\": \"unknowns_list\", \"args\": {{\"status\": \"open\"}}}} lists tracked unknowns\n\
- {{\"tool\": \"mechanics_check\", \"args\": {{\"path\": \"docs/mechanics.json\", \"strict\": false}}}} validates mechanics.json provenance (warns if literal config is not runtime-verified)\n\
- {{\"tool\": \"study_status\", \"args\": {{}}}} returns Study -> Build -> Check -> Ready stage, clean_room flag, evidence/unknowns, and kept-check status\n\
- {{\"tool\": \"playtest\", \"args\": {{}}}} runs static + live playtest verification\n\
- {{\"done\": true, \"summary\": \"...\"}} finishes.\n\n\
3D, VFX & UI GUARDRAILS:\n\
- PartType.Cylinder runs along its LOCAL X axis: upright columns/pillars MUST set Size = Vector3.new(height, diameter, diameter) with CFrame.Angles(0, 0, math.rad(90)).\n\
- Mobile StreamingEnabled budgets: <=20,000 tris/mesh, <=40 active ParticleEmitters, <=12 non-shadow lights. Clean up every transient VFX/Part/Sound via Debris:AddItem or pooling.\n\
- Respect Settings-only Light/Dark theme and FlashEnabled/ReducedMotion toggles; wire meshes via assets/meshes/manifest.json + src/shared/AssetIds.luau.\n\n\
Workflow — Study -> Build -> Check -> Refine (ONE JSON object per turn): First Study (respect `clean_room`; record verified facts with `evidence_record`, log open questions with `unknown_add`, validate `mechanics_check`), then Build (`project_write_file`/`project_edit_file`), then Check (`project_check`/`project_run_lune`/`project_sync`/`project_playtest`, resolving unknowns via `unknown_update` only with verified `ev_*` IDs), then Refine until `study_status` and verification pass cleanly before {{\"done\": true, \"summary\": \"...\"}}.\n\n{}",
        crate::art_knowledge::prompt_guidance()
    )
}

/// Lean, high-signal system prompt used when an active Rojo project is open:
/// drops legacy v0.9 canned skill essays to cut argv/token size by ~70% and
/// enables batched `steps` execution (multiple tool actions per model turn).
pub fn project_system_prompt() -> String {
    format!(
        "You are OpenRO, a Roblox Studio AI IDE agent working on an active Rojo project on disk. \
Reply with ONLY one JSON object per turn — no prose outside the JSON.\n\n\
BATCHED STEPS (RECOMMENDED — saves turns and quota):\n\
You may execute multiple actions in one turn using:\n\
{{\"steps\": [{{\"tool\": \"...\", \"args\": {{...}}}}, ...], \"done\": true, \"summary\": \"...\"}}\n\
Steps run in order and abort immediately on the first error (returning all completed results plus the error). \
Omit \"done\" if you need to inspect tool results before continuing. Single-step {{\"tool\": \"...\", \"args\": {{...}}}} and {{\"done\": true, \"summary\": \"...\"}} also work.\n\n\
PROJECT FILE & BUILD TOOLS:\n\
- project_list {{}}\n\
- project_files {{\"dir\": \"src\"}}\n\
- project_read_file {{\"path\": \"src/...\", \"start_line\": 1, \"end_line\": 200}}\n\
- project_write_file {{\"path\": \"src/server/Main.server.luau\", \"content\": \"...\"}} (auto-checked by Static Luau Prover)\n\
- project_edit_file {{\"path\": \"src/...\", \"edits\": [{{\"old\": \"...\", \"new\": \"...\"}}]}} (atomic multi-hunk edit)\n\
- project_delete_file {{\"path\": \"src/old.luau\"}}\n\
- project_grep {{\"query\": \"RemoteEvent\", \"dir\": \"src\"}}\n\
- project_update_manifest {{\"patch\": {{...}}}} (safely updates openro.project.json; cannot auto-approve product prices)\n\
- project_check {{}} (runs selene, stylua --check, static Luau checks)\n\
- project_run_lune {{\"script\": \"tools/sim.luau\"}} (timeout-bounded Lune execution)\n\
- sandbox_playtest {{\"timeout_secs\": 15, \"journey\": [...]}} (<2s headless Roblox Luau runtime sandbox: server/client, remotes, DataStores, ProcessReceipt idempotency, PlayerGui)\n\
- project_build {{\"engine\": \"auto\"}} (.rbxlx build)\n\
- project_scene_get {{}} (unified workspace: scripts, instances, meshes, lighting, lights, terrain, diagnostics)\n\
- project_scene_mutate {{\"ops\": [{{\"op\": \"upsert|delete|duplicate|reparent|set_lighting|set_terrain|undo|redo\", \"path\": \"Workspace.Name\", \"class_name\": \"Part|Model|SpawnLocation|MeshPart|PointLight|SpotLight|SurfaceLight|ParticleEmitter|Sound|SurfaceAppearance\", ...}}]}} (workspace.model.json, terrain.json & Lighting; set_terrain: generator/paint_box/clear; reparent: new_parent)\n\
- gui_save {{\"slug\": \"main-hud\", \"doc\": {{...}} | \"preset\": \"rpg|obby|tycoon|gacha\"}} (ui/<slug>.screen.json + safe-area)\n\
- gui_compile {{\"slug\": \"main-hud\", \"emit_model_json\": false}} (--!strict Luau + Rojo .model.json)\n\
- project_sync {{\"prune\": false}} / project_playtest {{\"sync_first\": true, \"suite\": \"playtest/suite.json\"}}\n\n\
RESEARCH, ART, 3D, AUDIO & UPLOAD TOOLS:\n\
- research_roblox_game {{\"target\": \"<URL or ID>\"}} (Roblox API + thumbs -> refs/<universeId>/)\n\
- web_search {{\"query\": \"...\", \"limit\": 5}} / web_fetch {{\"url\": \"https://...\"}}\n\
- reference_search {{\"query\": \"...\", \"character\": \"...\", \"show\": \"...\"}} (-> art/references/)\n\
- art_generate {{\"prompt\": \"single-line eye-level neutral three-quarter prompt\", \"out_path\": \"art/cards/x/art.png\", \"references\": [...]}} (ChatGPT browser default; never Higgsfield)\n\
- image_process {{\"input\": \"...\", \"output\": \"...\", \"ops\": [...]}} / art_pack_card {{\"card_id\": \"...\"}}\n\
- mesh_generate {{\"name\": \"Prop\", \"description\": \"...\", \"script\": \"...\"}} (AO-baked .obj + PNG <=20k tris -> assets/meshes/)\n\
- audio_generate {{\"name\": \"Sfx\", \"preset\": \"...\", \"recipe\": {{...}}, \"duration_ms\": 1200}} (-> assets/audio/)\n\
- adapter_list {{\"capability\": \"...?\"}} / adapter_invoke {{\"capability\": \"...\", \"adapter_id\": \"...?\", \"params\": {{...}}}}\n\
- opencloud_status {{}} / creator_hub_status {{\"check_login\": true, \"save_creator\": true}} / creator_hub_setup {{...}} / creator_hub_configure {{...}} (loopback CDP; stop on action_required)\n\
- assets_authorise {{\"paths\": [...]}} / assets_upload {{\"paths\": [...]}} / assets_codegen {{}} (-> AssetIds.luau)\n\
- products_sync {{}} / products_codegen {{}} (-> ProductIds.luau) / universe_get {{}} / universe_set {{...}}\n\
- liveops_datastore_list {{}} / liveops_datastore_get {{\"store\":\"...\",\"key\":\"...\"}} / liveops_datastore_set {{...,\"expected_revision\":\"...\",\"confirm\":true}} / liveops_messaging_publish {{\"topic\":\"...\",\"message\":{{...}},\"confirm\":true}} / liveops_remote_config_push {{\"config\":{{...}},\"generate_luau\":true,\"push_to_datastore\":false}} / liveops_telemetry_summary {{}} (live writes need confirm:true)\n\
- project_publish {{\"versionType\": \"Saved\"}} (live \"Published\" requires confirm: true ONLY after owner approval)\n\n\
STUDY & EVIDENCE TOOLS (Study -> Build -> Check -> Refine):\n\
- evidence_record {{\"summary\":\"...\",\"confidence\":\"observed|derived|inferred\",\"kind\":\"...\",\"source\":\"...\"}} / evidence_list {{\"confidence\":\"...?\",\"limit\":50}}\n\
- unknown_add {{\"title\":\"...\",\"question\":\"...\",\"severity\":\"high\",\"domain\":\"...\"}} / unknowns_list {{\"status\":\"open\"}}\n\
- unknown_update {{\"id\":\"unk_...\",\"expected_revision\":1,\"status\":\"resolved\",\"resolution\":\"...\",\"evidence_for\":[\"ev_...\"]}} (needs resolution + ev_*)\n\
- mechanics_check {{\"path\":\"docs/mechanics.json\",\"strict\":false}} / study_status {{}}\n\n\
3D, VFX & UI GUARDRAILS:\n\
- PartType.Cylinder runs along LOCAL X: upright pillars MUST use Size=Vector3.new(h,d,d) + CFrame.Angles(0,0,math.rad(90)).\n\
- Mobile budgets: <=20k tris/mesh, <=40 ParticleEmitters, <=12 non-shadow lights; clean transient VFX/Parts/Sounds via Debris:AddItem or pooling.\n\
- Enforce Settings-only Light/Dark theme and FlashEnabled/ReducedMotion; wire meshes/art via assets/meshes/manifest.json + src/shared/AssetIds.luau.\n\
- Workflow Study -> Build -> Check -> Refine: record evidence, resolve unknowns with ev_* IDs, batch edits, verify with mechanics_check, project_check / project_run_lune, and project_playtest (sync_first: true).\n\n{}",
        crate::art_knowledge::prompt_guidance()
    )
}

#[derive(Debug)]
pub struct RunOutcome {
    pub summary: String,
    /// Plain-words lines for the chat thread, produced by this ask.
    pub thread: Vec<String>,
    /// Step-by-step record of what the loop did (skills, tools, console).
    pub transcript: Vec<String>,
    /// Skills that generated Luau during this run — the server feeds
    /// these to the creativity engine, so follow-up pitches know what
    /// the game already has.
    pub skills_used: Vec<String>,
    /// True when the ask was refused rather than completed: `summary`
    /// then holds the plain refusal, not the model's claim of success.
    pub refused: bool,
    /// True when this ask (or plan) really changed the game. Only then
    /// does the server record skills as built and pitch the next set.
    pub changed: bool,
}

/// A failed ask, carrying the thread and transcript lines produced
/// before the failure — a failed ask is still part of the chat's
/// record, and its change set stays revertable.
#[derive(Debug)]
pub struct AskFailure {
    pub message: String,
    pub thread: Vec<String>,
    pub transcript: Vec<String>,
    /// Skills that generated before the failure. A failed build can
    /// still have landed scripts in the place, so the server records
    /// these as built — otherwise the pitch engine re-offers systems
    /// the game already (partly) has.
    pub skills_used: Vec<String>,
}

/// One checkpointed change: a script's previous source and the state
/// this ask LEFT it in, or an instance's previous record
/// (`{"class_name": ..., "properties": ...}`) and its left state.
/// `None` means the path did not exist at that point. The left state
/// is what makes revert safe: restore only proceeds when the place
/// still holds exactly what the ask left (or already holds the
/// previous state) — a path changed by anyone else since is refused,
/// never silently overwritten.
#[derive(Clone)]
enum Change {
    Script(String, Option<String>, Option<String>),
    Instance(String, Option<Value>, Option<Value>),
}

impl Change {
    /// True when this change left the path exactly as it found it
    /// (created then deleted, or rewritten with identical content) —
    /// a plan made only of these built nothing, whatever it claims.
    fn is_net_zero(&self) -> bool {
        match self {
            Change::Script(_, prev, post) => prev == post,
            Change::Instance(_, prev, post) => prev == post,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Change::Script(path, prev, post) => json!({
                "kind": "script", "path": path,
                "prev": prev.clone().map(Value::String).unwrap_or(Value::Null),
                "post": post.clone().map(Value::String).unwrap_or(Value::Null),
            }),
            Change::Instance(path, prev, post) => json!({
                "kind": "instance", "path": path,
                "prev": prev.clone().unwrap_or(Value::Null),
                "post": post.clone().unwrap_or(Value::Null),
            }),
        }
    }

    /// Strict: a field of the wrong type makes the entry INVALID
    /// (None) — never coerced. (A numeric `prev` once coerced to
    /// "was absent", and a revert deleted a real script on the
    /// authority of an unreadable record.)
    fn from_json(v: &Value) -> Option<Change> {
        let path = v.get("path")?.as_str()?.to_string();
        fn script_state(v: &Value, key: &str) -> Option<Option<String>> {
            match v.get(key)? {
                Value::Null => Some(None),
                Value::String(s) => Some(Some(s.clone())),
                _ => None,
            }
        }
        fn instance_state(v: &Value, key: &str) -> Option<Option<Value>> {
            match v.get(key)? {
                Value::Null => Some(None),
                Value::Object(o) => {
                    // A record without a string class_name is not a
                    // record — restoring from it would fabricate one.
                    o.get("class_name").filter(|c| c.is_string())?;
                    Some(Some(Value::Object(o.clone())))
                }
                _ => None,
            }
        }
        match v.get("kind")?.as_str()? {
            "script" => Some(Change::Script(
                path,
                script_state(v, "prev")?,
                script_state(v, "post")?,
            )),
            "instance" => Some(Change::Instance(
                path,
                instance_state(v, "prev")?,
                instance_state(v, "post")?,
            )),
            _ => None,
        }
    }
}

/// One change-making ask's set: its checkpoints, and the skills it ran
/// (so a revert can tell the creativity engine what the place lost).
#[derive(Clone)]
struct ChangeSet {
    changes: Vec<Change>,
    skills: Vec<String>,
}

/// Whether a chat's place still holds changes no passing playtest has
/// covered, and the console of the last playtest that failed (until a
/// later pass clears it). Kept per chat, so a later ask cannot say
/// "done" over a game the app's own last test run rejected.
#[derive(Default, Clone)]
struct Proof {
    unproven: bool,
    failed_console: Option<String>,
    /// Skills that Undo took out of the place and nothing has rebuilt.
    undone: Vec<String>,
}

pub struct Agent {
    pub model: ModelClient,
    pub studio: Option<PlaceBackend>,
    /// Luau generated by each skill during this ask, by skill name — a
    /// second generation never overwrites an unwritten first one.
    generated: HashMap<String, String>,
    last_skill: Option<String>,
    /// Change sets per chat id, as a STACK: each change-making ask
    /// pushes its own set, and revert pops the most recent one. (A
    /// single-slot map used to silently replace the previous ask's
    /// set, leaving its changes unrevertable forever; merging instead
    /// would break per-ask prev state — each set's checkpoints are the
    /// place as it was when THAT ask started.)
    changes_by_chat: HashMap<String, Vec<ChangeSet>>,
    /// Undone change sets per chat id, as a STACK for `POST /api/redo`.
    redo_by_chat: HashMap<String, Vec<ChangeSet>>,
    /// Change set being accumulated by the ask currently running.
    current_changes: Vec<Change>,
    /// Where change sets persist across restarts, when set.
    changes_path: Option<std::path::PathBuf>,
    /// Proof state per chat, saved as proof.json next to changes.json.
    proof_by_chat: HashMap<String, Proof>,
    proof_path: Option<std::path::PathBuf>,
    /// Backup file names written when a corrupt changes file was set aside.
    pub recovered: Vec<String>,
    /// Shared set of chats with a non-empty change-set stack (for the page's Undo).
    undo_sink: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
    /// Shared set of chats with a non-empty redo stack (for the page's Redo).
    redo_sink: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
    /// Optional SSE build-progress event hub.
    events: Option<Arc<BuildEventHub>>,
    /// Optional realtime Studio 3D action & MCP frame stream hub.
    studio_stream: Option<Arc<StudioStreamHub>>,
    /// Active build correlation id & chat id for SSE phase updates.
    active_build_id: String,
    active_build_chat: String,
    /// Dynamic skill prompt block injected from installed Tool & Skill Library packs.
    library_prompt: String,
    /// Optional 8-dimension Taste Dossier prompt block from the most recently researched Roblox game.
    market_dossier_prompt: String,
    /// Root directory for generated 3D meshes (`blender_generate`) and `.wav` SFX (`audio_generate`).
    assets_root: Option<std::path::PathBuf>,
    /// Active Rojo project root directory & id (for project-aware context and tools).
    pub project_root: Option<std::path::PathBuf>,
    pub project_id: Option<String>,
    /// Pluggable hooks connecting `openro-core` (sync/playtest/build/check/lune)
    /// and `openro-assets` (reference_search/art_generate/image_process/mesh/audio/upload).
    pub hooks: AgentHooks,
    /// Optional cooperative cancel flag checked between model steps.
    cancel_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Optional per-step progress callback `(step_number, summary_line)`.
    step_sink: Option<StepSink>,
    /// Optional override for maximum agent steps in long runs.
    max_steps_override: Option<usize>,
    /// Previous `/api/studio/playtest` assertion vector for deterministic diffing.
    last_playtest_assertions: Vec<Value>,
}

pub type HookFn = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;
pub type StepSink = Arc<dyn Fn(usize, &str) + Send + Sync>;

/// Optional callbacks injected by the server (or Teamwork) to route agent
/// tools through `openro-core` and `openro-assets` without tight coupling.
#[derive(Clone, Default)]
pub struct AgentHooks {
    pub project_list: Option<HookFn>,
    pub project_build: Option<HookFn>,
    pub project_sync: Option<HookFn>,
    pub project_playtest: Option<HookFn>,
    pub project_check: Option<HookFn>,
    pub project_run_lune: Option<HookFn>,
    pub sandbox_playtest: Option<HookFn>,
    pub reference_search: Option<HookFn>,
    pub art_generate: Option<HookFn>,
    pub image_process: Option<HookFn>,
    pub art_pack_card: Option<HookFn>,
    pub mesh_generate: Option<HookFn>,
    pub audio_generate: Option<HookFn>,
    pub assets_upload: Option<HookFn>,
    pub opencloud_status: Option<HookFn>,
    pub creator_hub_status: Option<HookFn>,
    pub creator_hub_setup: Option<HookFn>,
    pub creator_hub_configure: Option<HookFn>,
    pub assets_authorise: Option<HookFn>,
    pub assets_codegen: Option<HookFn>,
    pub products_sync: Option<HookFn>,
    pub products_codegen: Option<HookFn>,
    pub project_publish: Option<HookFn>,
    pub universe_get: Option<HookFn>,
    pub universe_set: Option<HookFn>,
    pub adapter_list: Option<HookFn>,
    pub adapter_invoke: Option<HookFn>,
}

impl Agent {
    pub fn new(model: ModelClient, studio: Option<McpClient>) -> Agent {
        Self::with_backend(model, studio.map(PlaceBackend::Mcp))
    }

    pub fn with_local_place(model: ModelClient, place: LocalPlace) -> Agent {
        let mut recovered = place.recovered.clone();
        let mut agent = Self::with_backend(model, Some(PlaceBackend::Local(place)));
        agent.recovered.append(&mut recovered);
        agent
    }

    pub fn with_backend(model: ModelClient, studio: Option<PlaceBackend>) -> Agent {
        Agent {
            model,
            studio,
            generated: HashMap::new(),
            last_skill: None,
            changes_by_chat: HashMap::new(),
            redo_by_chat: HashMap::new(),
            current_changes: Vec::new(),
            changes_path: None,
            proof_by_chat: HashMap::new(),
            proof_path: None,
            recovered: Vec::new(),
            undo_sink: None,
            redo_sink: None,
            events: None,
            studio_stream: None,
            active_build_id: String::new(),
            active_build_chat: String::new(),
            library_prompt: String::new(),
            market_dossier_prompt: String::new(),
            assets_root: None,
            project_root: None,
            project_id: None,
            hooks: AgentHooks::default(),
            cancel_flag: None,
            step_sink: None,
            max_steps_override: None,
            last_playtest_assertions: Vec::new(),
        }
    }

    pub fn set_project(&mut self, root: Option<std::path::PathBuf>, id: Option<String>) {
        self.project_root = root;
        self.project_id = id;
    }

    pub fn effective_project_root(&self) -> Option<std::path::PathBuf> {
        if let Some(root) = &self.project_root {
            if root.is_dir() {
                return Some(root.clone());
            }
        }
        std::env::var("OPENRO_PROJECT_DIR")
            .ok()
            .map(|s| std::path::PathBuf::from(s.trim()))
            .filter(|p| p.is_dir())
    }

    pub fn set_hooks(&mut self, hooks: AgentHooks) {
        self.hooks = hooks;
    }

    pub fn set_cancel_flag(&mut self, flag: Option<Arc<std::sync::atomic::AtomicBool>>) {
        self.cancel_flag = flag;
    }

    pub fn set_step_sink(&mut self, sink: Option<StepSink>) {
        self.step_sink = sink;
    }

    pub fn set_max_steps_override(&mut self, max_steps: Option<usize>) {
        self.max_steps_override = max_steps;
    }

    pub fn set_assets_root(&mut self, root: std::path::PathBuf) {
        self.assets_root = Some(root);
    }

    fn effective_assets_root(&self) -> std::path::PathBuf {
        self.assets_root.clone().unwrap_or_else(|| {
            let dir = std::env::temp_dir().join(format!("openro-agent-assets-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            dir
        })
    }

    fn invoke_opencloud_local(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: &Value,
    ) -> Result<Value, String> {
        let data_dir = self.effective_assets_root();
        let store = crate::secrets::SecretStore::for_data_dir(&data_dir);
        let active = self.effective_project_root();
        let active_for_res = active.clone();
        let resolver = move |_id: &str| active_for_res.clone();
        let ctx = crate::opencloud::Ctx {
            data_dir: &data_dir,
            store,
            resolve: &resolver,
            active_project: active,
        };
        let reply = crate::opencloud::route(&ctx, method, path, query, body)
            .ok_or_else(|| format!("Open Cloud endpoint '{method} {path}' not available"))?;
        match reply {
            crate::opencloud::Reply::Json(code, val) => {
                let is_action_required =
                    val.get("status").and_then(|v| v.as_str()) == Some("action_required");
                if code >= 400
                    || (val.get("ok").and_then(|v| v.as_bool()) == Some(false) && !is_action_required)
                {
                    let err = val
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| format!("HTTP {code}: {val}"));
                    Err(err)
                } else {
                    Ok(val)
                }
            }
            crate::opencloud::Reply::Bytes(mime, bytes) => {
                Ok(json!({ "ok": true, "mime": mime, "bytes": bytes.len() }))
            }
        }
    }

    pub fn set_library_prompt(&mut self, prompt: String) {
        self.library_prompt = prompt;
    }

    pub fn set_market_dossier_prompt(&mut self, prompt: String) {
        self.market_dossier_prompt = prompt;
    }

    /// Full system prompt sent to the model: core OpenRO rules + installed Tool & Skill
    /// Library packs (including active Roblox Market Taste Craft Bar & Automatic Twists)
    /// + any recently researched Roblox reference game Taste Dossier.
    ///
    /// Local loopback gateways (e.g. Ollama 3B/7B models with default 4k context) receive
    /// a distilled high-signal Craft Bar & Twists digest so the core JSON tool contract
    /// is never truncated by a small context window; remote/frontier models receive the
    /// complete unabridged dossier.
    pub fn effective_system_prompt(&self) -> String {
        if self.effective_project_root().is_some() {
            return project_system_prompt();
        }
        let mut out = system_prompt();
        let compact = self.model.is_loopback() && !self.model.is_antigravity();
        if !self.library_prompt.trim().is_empty() {
            out.push_str("\n\n");
            if compact {
                out.push_str(&distill_prompt_for_local_model(&self.library_prompt, 1100));
            } else {
                out.push_str(&self.library_prompt);
            }
        }
        if !self.market_dossier_prompt.trim().is_empty() {
            out.push_str("\n\n");
            if compact {
                out.push_str(&distill_prompt_for_local_model(&self.market_dossier_prompt, 450));
            } else {
                out.push_str(&self.market_dossier_prompt);
            }
        }
        if !self.library_prompt.trim().is_empty() || !self.market_dossier_prompt.trim().is_empty() {
            out.push_str("\n\nREMINDER: Reply with exactly ONE JSON object per turn, no prose outside the JSON.");
        }
        out
    }

    pub fn studio_id(&self) -> Option<String> {
        match &self.studio {
            Some(PlaceBackend::Mcp(mcp)) => mcp.studio_id().map(String::from),
            _ => None,
        }
    }

    /// Attach an SSE `BuildEventHub` so build phases stream live to `/api/build/stream`.
    pub fn attach_events(&mut self, hub: Arc<BuildEventHub>) {
        self.events = Some(hub);
    }

    /// Attach a realtime `StudioStreamHub` so granular Studio mutations and live MCP
    /// viewport frames stream to `/api/studio/stream`.
    pub fn attach_studio_stream(&mut self, hub: Arc<StudioStreamHub>) {
        hub.set_studio_mode(self.studio_mode_tag());
        self.studio_stream = Some(hub);
    }

    pub fn studio_mode_tag(&self) -> &'static str {
        match &self.studio {
            Some(PlaceBackend::Mcp(_)) => "live_mcp",
            Some(PlaceBackend::Plugin(_)) => "plugin_live",
            _ => "local_place",
        }
    }

    fn place_counts(&mut self) -> (usize, usize) {
        match &mut self.studio {
            Some(PlaceBackend::Local(local)) => (local.instances().len(), local.scripts().len()),
            Some(PlaceBackend::Mcp(mcp)) => mcp
                .get_place_tree()
                .ok()
                .map(|t| {
                    let ic = t.get("instances").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                    let sc = t.get("scripts").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                    (ic, sc)
                })
                .unwrap_or((0, 0)),
            Some(PlaceBackend::Plugin(_)) | None => (0, 0),
        }
    }

    fn emit_studio_action(&mut self, mut input: StudioActionInput) {
        if self.studio_stream.is_none() {
            return;
        }
        let mode = self.studio_mode_tag().to_string();
        let (inst_count, script_count) = self.place_counts();
        input.studio_mode = Some(mode);
        input.instance_count = inst_count;
        input.script_count = script_count;
        if let Some(hub) = &self.studio_stream {
            hub.record_action(input);
        }
    }

    /// Best-effort live MCP `screen_capture` right after a mutation when attached to
    /// a live Roblox Studio MCP bridge (never fails the underlying build action).
    fn auto_capture_mcp_frame(&mut self) -> Option<u64> {
        let hub = self.studio_stream.as_ref().map(Arc::clone)?;
        let Some(PlaceBackend::Mcp(mcp)) = self.studio.as_mut() else {
            return None;
        };
        if !mcp.has_tool("screen_capture") {
            return None;
        }
        match mcp.screen_capture() {
            Ok(frame) => {
                let stored = hub.store_frame(frame);
                Some(stored.seq)
            }
            Err(_) => None,
        }
    }

    fn take_playtest_frame_seq(&mut self) -> Option<u64> {
        let hub = self.studio_stream.as_ref().map(Arc::clone)?;
        let Some(PlaceBackend::Mcp(mcp)) = self.studio.as_mut() else {
            return None;
        };
        mcp.take_last_playtest_frame()
            .map(|frame| hub.store_frame(frame).seq)
    }

    /// Explicitly capture one live Roblox Studio MCP frame (`POST /api/studio/capture`).
    pub fn capture_studio_frame(&mut self) -> Result<StudioFrame, String> {
        let frame = match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(mcp)) => mcp.screen_capture()?,
            Some(PlaceBackend::Plugin(_)) => {
                return Err(
                    "Live Studio Frame capture requires Embedded Studio / MCP (Companion Plugin uses Studio's native viewport)."
                        .to_string(),
                );
            }
            Some(PlaceBackend::Local(_)) => {
                return Err(
                    "Live Studio Frame capture requires a connected Roblox Studio MCP session (currently running Local Place Engine). Switch to Studio 3D or attach Roblox Studio MCP."
                        .to_string(),
                );
            }
            None => return Err("Studio not connected".to_string()),
        };
        if let Some(hub) = &self.studio_stream {
            Ok(hub.store_frame(frame))
        } else {
            Ok(frame)
        }
    }

    /// Execute a Studio playtest directly (`POST /api/studio/playtest`), streaming
    /// `PlaytestStarted`, `PlaytestLog`, and `PlaytestPassed`/`PlaytestFailed` actions
    /// over `StudioStreamHub` (plus capturing a live MCP frame when connected).
    pub fn run_studio_playtest(&mut self) -> Result<Value, String> {
        let mode = self.studio_mode_tag().to_string();
        let mut start_ev = StudioActionInput::new(
            StudioActionKind::PlaytestStarted,
            if mode == "live_mcp" {
                "Started live Roblox Studio playtest via MCP"
            } else {
                "Running Local Place static Luau & playtest verification"
            },
        );
        start_ev.path = "Workspace".to_string();
        start_ev.class_name = "RunService".to_string();
        self.emit_studio_action(start_ev);

        let (passed, lines, frame_seq) = match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(mcp)) => {
                let start_res = mcp.start_stop_play("start_play");
                let start_ok = match &start_res {
                    Ok(v) => {
                        let st = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                        !st.eq_ignore_ascii_case("failed") && v.get("ok").and_then(|b| b.as_bool()) != Some(false)
                    }
                    Err(_) => false,
                };
                let console_lines = mcp.get_console_output().unwrap_or_default();
                let fseq = self.auto_capture_mcp_frame();
                if let Some(PlaceBackend::Mcp(mcp_stop)) = self.studio.as_mut() {
                    let _ = mcp_stop.start_stop_play("stop");
                }
                let has_err = console_lines.iter().any(|l| {
                    let low = l.to_ascii_lowercase();
                    low.contains("[error]") || low.contains("syntax error") || low.contains("runtime error")
                });
                let mut out_lines = console_lines;
                if let Err(e) = start_res {
                    out_lines.push(format!("playtest error: {e}"));
                }
                (start_ok && !has_err, out_lines, fseq)
            }
            Some(PlaceBackend::Plugin(_)) => {
                let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
                let verdict = playtest_verdict(&text);
                let mut out_lines: Vec<String> = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v.get("lines")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                v.get("console").and_then(|c| c.as_str()).map(|s| {
                                    s.lines()
                                        .map(str::trim)
                                        .filter(|l| !l.is_empty())
                                        .map(String::from)
                                        .collect::<Vec<_>>()
                                })
                            })
                    })
                    .unwrap_or_default();
                if out_lines.is_empty() {
                    out_lines.push(if matches!(verdict, Verdict::Passed) {
                        "Companion Studio Plugin playtest passed.".to_string()
                    } else {
                        text.clone()
                    });
                }
                (matches!(verdict, Verdict::Passed), out_lines, None)
            }
            Some(PlaceBackend::Local(_)) => {
                let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
                let verdict = playtest_verdict(&text);
                let raw_console = self
                    .studio_mut()?
                    .call_tool("get_console", json!({}))
                    .unwrap_or_default();
                let mut out_lines: Vec<String> = raw_console
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                if out_lines.is_empty() {
                    out_lines.push(if matches!(verdict, Verdict::Passed) {
                        "Local Place playtest passed with 0 Luau errors.".to_string()
                    } else {
                        text.clone()
                    });
                }
                (matches!(verdict, Verdict::Passed), out_lines, None)
            }
            None => return Err("Studio not connected".to_string()),
        };

        for line in lines.iter().take(16) {
            let mut log_ev = StudioActionInput::new(StudioActionKind::PlaytestLog, line.clone());
            log_ev.path = "Output".to_string();
            log_ev.class_name = "LogService".to_string();
            log_ev.console_line = Some(line.clone());
            log_ev.frame_seq = frame_seq;
            self.emit_studio_action(log_ev);
        }

        let mut verdict_ev = StudioActionInput::new(
            if passed {
                StudioActionKind::PlaytestPassed
            } else {
                StudioActionKind::PlaytestFailed
            },
            if passed {
                format!("Studio playtest passed ({} console line(s))", lines.len())
            } else {
                format!("Studio playtest failed ({} console line(s))", lines.len())
            },
        );
        verdict_ev.path = "Workspace".to_string();
        verdict_ev.class_name = "RunService".to_string();
        verdict_ev.console_line = lines.last().cloned();
        verdict_ev.frame_seq = frame_seq;
        self.emit_studio_action(verdict_ev);

        let mut assertions = vec![
            json!({
                "id": "runtime_console",
                "room": "Runtime",
                "label": "Studio runtime & Luau check",
                "passed": passed,
                "status": if passed { "passed" } else { "failed" },
                "detail": lines.last().cloned().unwrap_or_else(|| "Playtest completed".to_string())
            }),
        ];
        if let Ok(tree) = self.place_tree() {
            let scripts = tree.get("scripts").and_then(Value::as_object);
            let insts = tree.get("instances").and_then(Value::as_object);
            let script_count = scripts.map(|s| s.len()).unwrap_or(0);
            let inst_count = insts.map(|i| i.len()).unwrap_or(0);
            let has_monetization = scripts.is_some_and(|s| s.contains_key("ServerScriptService.MonetizationService"));
            let has_retention = scripts.is_some_and(|s| s.contains_key("ServerScriptService.DailyRewardRetention"));
            assertions.push(json!({
                "id": "place_scripts",
                "room": "Luau",
                "label": "Place script tree populated",
                "passed": script_count > 0,
                "status": if script_count > 0 { "passed" } else { "failed" },
                "detail": format!("{script_count} script(s), {inst_count} instance(s)")
            }));
            assertions.push(json!({
                "id": "economy_guard",
                "room": "Economy",
                "label": "Monetization & retention guards",
                "passed": has_monetization && has_retention,
                "status": if has_monetization && has_retention { "passed" } else { "failed" },
                "detail": format!(
                    "Service={} · Retention={}",
                    if has_monetization { "ok" } else { "missing" },
                    if has_retention { "ok" } else { "missing" }
                )
            }));
        }
        let prev = if self.last_playtest_assertions.is_empty() {
            None
        } else {
            Some(self.last_playtest_assertions.as_slice())
        };
        let assertion_diff = crate::playtest::diff_assertion_suites(prev, &assertions);
        self.last_playtest_assertions = assertions.clone();

        Ok(json!({
            "ok": true,
            "passed": passed,
            "status": if passed { "passed" } else { "failed" },
            "studio_mode": mode,
            "console": lines.join("\n"),
            "console_lines": lines,
            "frame_seq": frame_seq,
            "assertions": assertions,
            "assertion_diff": assertion_diff,
        }))
    }

    fn begin_build_trace(&mut self, chat_id: &str, label: &str) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        self.active_build_id = format!("build-{nonce}");
        self.active_build_chat = chat_id.to_string();
        self.emit_event("planning", "Planning", label, None, None, 10);
    }

    fn emit_event(
        &self,
        phase: &str,
        label: &str,
        detail: &str,
        skill: Option<&str>,
        path: Option<&str>,
        progress: u8,
    ) {
        if let Some(hub) = &self.events {
            hub.publish(
                self.active_build_id.clone(),
                self.active_build_chat.clone(),
                phase,
                label,
                detail,
                skill.map(String::from),
                path.map(String::from),
                progress,
            );
        }
    }

    /// Persist change sets to `path` (loaded now, saved on change), so
    /// a restart does not silently orphan every revert. Loading is
    /// strict and loud: an unparseable file, a wrong shape, or ANY
    /// malformed entry backs the whole file up and starts empty —
    /// a partially-read change set is worse than none, because a
    /// revert acting on coerced records can destroy real work.
    pub fn set_changes_file(&mut self, path: std::path::PathBuf) {
        let proof_path = path.with_file_name("proof.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let loaded = (|| -> Option<HashMap<String, Vec<ChangeSet>>> {
                let v: Value = serde_json::from_str(&text).ok()?;
                let obj = v.as_object()?;
                let mut out = HashMap::new();
                for (chat, stack) in obj {
                    let sets = stack.as_array()?;
                    let mut parsed_stack = Vec::new();
                    for set in sets {
                        let parsed = parse_set(set)?;
                        if !parsed.changes.is_empty() {
                            parsed_stack.push(parsed);
                        }
                    }
                    if !parsed_stack.is_empty() {
                        out.insert(chat.clone(), parsed_stack);
                    }
                }
                Some(out)
            })();
            match loaded {
                Some(map) => self.changes_by_chat = map,
                None => {
                    if let Some(bak) = crate::server::backup_corrupt(&path) {
                        self.recovered.push(bak.file_name().unwrap_or_default().to_string_lossy().into_owned());
                    }
                }
            }
        }
        self.changes_path = Some(path);
        self.sync_undo();
        // A chat with change sets but no saved verdict (proof file corrupt
        // or missing) is unproven until a passing playtest: conservative.
        self.proof_by_chat = load_proof(&proof_path, &mut self.recovered);
        for (chat, stack) in &self.changes_by_chat {
            if !stack.is_empty() {
                self.proof_by_chat.entry(chat.clone()).or_insert(Proof {
                    unproven: true,
                    ..Proof::default()
                });
            }
        }
        self.proof_path = Some(proof_path);
        self.save_proof();
    }

    fn save_changes(&self) {
        let Some(path) = &self.changes_path else { return };
        let mut obj = serde_json::Map::new();
        for (chat, stack) in &self.changes_by_chat {
            obj.insert(
                chat.clone(),
                Value::Array(
                    stack
                        .iter()
                        .map(|set| {
                            json!({
                                "changes": set.changes.iter().map(Change::to_json).collect::<Vec<_>>(),
                                "skills": set.skills,
                            })
                        })
                        .collect(),
                ),
            );
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, Value::Object(obj).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Keep the shared Undo set in step with the change-set stacks.
    pub fn attach_undo_sink(
        &mut self,
        sink: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) {
        self.undo_sink = Some(sink);
        self.sync_undo();
    }

    /// Keep the shared Redo set in step with the undone change-set stacks.
    pub fn attach_redo_sink(
        &mut self,
        sink: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) {
        self.redo_sink = Some(sink);
        self.sync_undo();
    }

    pub fn can_undo_chat(&self, chat_id: &str) -> bool {
        self.changes_by_chat
            .get(chat_id)
            .is_some_and(|s| !s.is_empty())
    }

    pub fn can_redo_chat(&self, chat_id: &str) -> bool {
        self.redo_by_chat
            .get(chat_id)
            .is_some_and(|s| !s.is_empty())
    }

    fn sync_undo(&self) {
        if let Some(sink) = &self.undo_sink {
            let mut set = sink.lock().unwrap();
            set.clear();
            set.extend(
                self.changes_by_chat
                    .iter()
                    .filter(|(_, stack)| !stack.is_empty())
                    .map(|(chat, _)| chat.clone()),
            );
        }
        if let Some(sink) = &self.redo_sink {
            let mut set = sink.lock().unwrap();
            set.clear();
            set.extend(
                self.redo_by_chat
                    .iter()
                    .filter(|(_, stack)| !stack.is_empty())
                    .map(|(chat, _)| chat.clone()),
            );
        }
    }

    fn save_proof(&self) {
        let Some(path) = &self.proof_path else { return };
        let obj: serde_json::Map<String, Value> = self
            .proof_by_chat
            .iter()
            .map(|(chat, p)| {
                (
                    chat.clone(),
                    json!({ "unproven": p.unproven, "failed_console": p.failed_console, "undone": p.undone }),
                )
            })
            .collect();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, Value::Object(obj).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// The prerequisite a skill needs (per the creativity catalogue) when
    /// that prerequisite was undone in this chat and nothing rebuilt it.
    fn undone_prerequisite(&self, chat_id: &str, skill: &str) -> Option<String> {
        let proof = self.proof_by_chat.get(chat_id)?;
        let kept = self.chat_skills(chat_id);
        crate::ideas::catalogue()
            .into_iter()
            .filter(|i| i.skills.contains(&skill))
            .flat_map(|i| i.needs.into_iter().map(String::from).collect::<Vec<_>>())
            .find(|need| proof.undone.contains(need) && !kept.contains(need))
    }

    pub fn studio_connected(&mut self) -> bool {
        match &mut self.studio {
            Some(s) => s.is_alive(),
            None => false,
        }
    }

    pub fn backend_mode(&self) -> &'static str {
        match &self.studio {
            Some(b) => b.mode_name(),
            None => "disconnected",
        }
    }

    pub fn rbxlx_path(&self) -> Option<std::path::PathBuf> {
        self.studio.as_ref().and_then(|b| b.rbxlx_path())
    }

    pub fn export_rbxlx(&mut self, game_name: &str) -> Result<String, String> {
        let tree = self.place_tree()?;
        Ok(serialize_rbxlx(game_name, &tree))
    }

    /// Export a complete Roblox Studio & Rojo 7 Ship Pack `.zip` archive containing
    /// `game.rbxlx`, `default.project.json`, `src/{server,client,shared}/*.luau`,
    /// `asset-manifest.json`, `assets/*`, `luau-audit-report.json`, and `STUDIO-UPLOAD-CHECKLIST.md`.
    pub fn export_rojo_ship_pack(
        &mut self,
        game_name: &str,
        assets: &[ShipPackAsset],
    ) -> Result<ShipPackBundle, String> {
        match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => {
                if local.game_name().trim().is_empty() && !game_name.trim().is_empty() {
                    local.set_game_name(game_name.to_string());
                }
                build_rojo_ship_pack(local, assets)
            }
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let tree = self.place_tree()?;
                let mut tmp = LocalPlace::in_memory(game_name.to_string());
                tmp.replace_tree(&tree);
                build_rojo_ship_pack(&tmp, assets)
            }
            None => Err("Studio not connected".to_string()),
        }
    }

    /// Record a direct Studio IDE edit (part preset, duplicate, delete, live Luau edit,
    /// or property update) onto the active chat's per-chat Undo stack and clear Redo.
    pub fn record_direct_edit(&mut self, chat_id: &str, before: &Value, after: &Value) {
        let changes = changeset_from_trees(before, after);
        if !changes.is_empty() {
            let stack = self
                .changes_by_chat
                .entry(chat_id.to_string())
                .or_default();
            if stack.len() >= crate::place::MAX_HISTORY_DEPTH {
                stack.remove(0);
            }
            stack.push(ChangeSet {
                changes,
                skills: Vec::new(),
            });
            self.redo_by_chat.remove(chat_id);
            self.save_changes();
            self.sync_undo();
        }
    }

    /// Create a typed 3D part preset (`block`, `sphere`, `cylinder`, `spawn`,
    /// `jump_pad`, `kill_brick`, `card_billboard`) plus any companion Luau script
    /// and record it on the active chat's Undo stack.
    pub fn create_part_preset(
        &mut self,
        chat_id: &str,
        preset: &str,
        name: Option<&str>,
        position: Option<[f64; 3]>,
        size: Option<[f64; 3]>,
        color: Option<&str>,
    ) -> Result<CreatedPreset, String> {
        let before = self.place_tree()?;
        let created = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => {
                local.create_part_preset(preset, name, position, size, color)?
            }
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let c = tmp.create_part_preset(preset, name, position, size, color)?;
                let target_tree = tmp.tree_json();
                self.apply_place_tree(&target_tree)?;
                c
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let props = created.instance.get("properties");
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartCreated,
            format!("Added {} ({}) to Workspace", created.path, created.class_name),
        );
        ev.path = created.path.clone();
        ev.class_name = created.class_name.clone();
        ev.position = extract_vec3_prop(props, "Position");
        ev.size = extract_vec3_prop(props, "Size");
        ev.color = extract_color_prop(props);
        self.emit_studio_action(ev);

        if let Some(sc_path) = &created.companion_script {
            let src = after
                .pointer("/scripts")
                .and_then(|s| s.get(sc_path))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut sev = StudioActionInput::new(
                StudioActionKind::ScriptWritten,
                format!("Wired companion script {sc_path} ({} bytes)", src.len()),
            );
            sev.path = sc_path.clone();
            sev.class_name = "Script".to_string();
            sev.code_snippet = Some(snippet_from_source(src));
            self.emit_studio_action(sev);
        }
        let _ = self.auto_capture_mcp_frame();
        Ok(created)
    }

    /// Duplicate an existing Workspace instance (and any descendants) with a +[4,0,4] offset.
    pub fn duplicate_instance(
        &mut self,
        chat_id: &str,
        path: &str,
    ) -> Result<DuplicateResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let dup = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => local.duplicate_instance(path)?,
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let d = tmp.duplicate_instance(path)?;
                let target_tree = tmp.tree_json();
                self.apply_place_tree(&target_tree)?;
                d
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let props = dup.instance.get("properties");
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartCreated,
            format!("Duplicated {} -> {}", dup.source_path, dup.new_path),
        );
        ev.path = dup.new_path.clone();
        ev.class_name = dup.class_name.clone();
        ev.position = extract_vec3_prop(props, "Position");
        ev.size = extract_vec3_prop(props, "Size");
        ev.color = extract_color_prop(props);
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(dup)
    }

    /// Delete an instance (with descendants) or a standalone Luau script at `path`.
    pub fn delete_at_path(&mut self, chat_id: &str, path: &str) -> Result<DeleteResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let is_instance = before
            .pointer("/instances")
            .and_then(|m| m.as_object())
            .is_some_and(|m| m.contains_key(path));
        let is_script = before
            .pointer("/scripts")
            .and_then(|m| m.as_object())
            .is_some_and(|m| m.contains_key(path));

        if !is_instance && !is_script {
            return Err(format!("no instance or script at {path}"));
        }

        let deleted = if is_instance {
            match self.studio.as_mut() {
                Some(PlaceBackend::Local(local)) => local.delete_instance(path)?,
                Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                    let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                    tmp.replace_tree(&before);
                    let d = tmp.delete_instance(path)?;
                    let target_tree = tmp.tree_json();
                    self.apply_place_tree(&target_tree)?;
                    d
                }
                None => return Err("Studio not connected".to_string()),
            }
        } else {
            self.studio_mut()?
                .call_tool("delete_script", json!({ "path": path }))?;
            DeleteResult {
                path: path.to_string(),
                removed_instances: Vec::new(),
                removed_scripts: vec![path.to_string()],
            }
        };

        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let mut ev = StudioActionInput::new(
            StudioActionKind::PartUpdated,
            format!(
                "Deleted {} ({} instance(s), {} script(s))",
                path,
                deleted.removed_instances.len(),
                deleted.removed_scripts.len()
            ),
        );
        ev.path = path.to_string();
        ev.class_name = if is_instance {
            "Instance".to_string()
        } else {
            "Script".to_string()
        };
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(deleted)
    }

    /// Save live-edited Luau source for `path` and run immediate Static Luau Prover checks.
    pub fn update_script_source(
        &mut self,
        chat_id: &str,
        path: &str,
        source: &str,
    ) -> Result<ScriptUpdateResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let res = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => local.update_script_source(path, source)?,
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let checked = tmp.update_script_source(path, source)?;
                self.studio_mut()?
                    .call_tool("write_script", json!({ "path": path, "source": source }))?;
                checked
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let mut ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!(
                "Saved Luau script {path} ({} lines, {} error(s))",
                res.lines,
                res.place_report.error_count()
            ),
        );
        ev.path = path.to_string();
        ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else {
            "Script".to_string()
        };
        ev.code_snippet = Some(snippet_from_source(source));
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(res)
    }

    /// List the official Roblox Studio MCP tools currently usable on the active backend.
    pub fn available_studio_tools(&self) -> Vec<String> {
        match &self.studio {
            Some(PlaceBackend::Mcp(mcp)) => {
                if mcp.tools_known() {
                    OFFICIAL_STUDIO_MCP_TOOLS
                        .iter()
                        .filter(|t| mcp.has_tool(t))
                        .map(|t| (*t).to_string())
                        .collect()
                } else {
                    OFFICIAL_STUDIO_MCP_TOOLS
                        .iter()
                        .map(|t| (*t).to_string())
                        .collect()
                }
            }
            Some(PlaceBackend::Plugin(_)) => [
                "list_roblox_studios",
                "get_studio_state",
                "search_game_tree",
                "inspect_instance",
                "script_read",
                "script_grep",
                "multi_edit",
                "execute_luau",
                "get_console_output",
                "start_stop_play",
            ]
            .iter()
            .map(|t| (*t).to_string())
            .collect(),
            Some(PlaceBackend::Local(_)) => [
                "list_roblox_studios",
                "get_studio_state",
                "search_game_tree",
                "inspect_instance",
                "script_read",
                "script_grep",
                "multi_edit",
                "get_console_output",
                "start_stop_play",
            ]
            .iter()
            .map(|t| (*t).to_string())
            .collect(),
            None => Vec::new(),
        }
    }

    /// Invoke one of the 13 official Roblox Studio MCP tools (`POST /api/studio/tools/:tool`
    /// or `POST /api/studio/input/{mouse,keyboard}`), streaming matching `StudioActionEvent`
    /// records and auto-capturing a fresh PNG frame on interactive/mutating tools.
    pub fn invoke_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: Value,
    ) -> Result<(Value, Option<u64>), String> {
        if !is_official_studio_tool(tool) {
            return Err(format!("unknown or unsupported Studio tool: {tool}"));
        }
        let args_obj = if args.is_object() { args } else { json!({}) };
        match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(_)) => {
                if tool == "screen_capture" {
                    let frame = self.capture_studio_frame()?;
                    return Ok((frame.meta_json(), Some(frame.seq)));
                }
                if tool == "get_console_output" {
                    let lines = match self.studio.as_mut() {
                        Some(PlaceBackend::Mcp(mcp)) => mcp.get_console_output()?,
                        _ => Vec::new(),
                    };
                    return Ok((
                        json!({
                            "ok": true,
                            "lines": lines,
                            "console": lines.join("\n"),
                        }),
                        None,
                    ));
                }
                if tool == "start_stop_play" {
                    let mode = args_obj
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("start_play")
                        .to_string();
                    let val = match self.studio.as_mut() {
                        Some(PlaceBackend::Mcp(mcp)) => mcp.start_stop_play(&mode)?,
                        _ => return Err("Studio not connected".to_string()),
                    };
                    let fseq = self.auto_capture_mcp_frame();
                    let mut ev = StudioActionInput::new(
                        if mode == "stop" {
                            StudioActionKind::PlaytestPassed
                        } else {
                            StudioActionKind::PlaytestStarted
                        },
                        format!("Studio start_stop_play({mode})"),
                    );
                    ev.path = "Workspace".to_string();
                    ev.class_name = "RunService".to_string();
                    ev.frame_seq = fseq;
                    self.emit_studio_action(ev);
                    return Ok((val, fseq));
                }

                let parsed = match self.studio.as_mut() {
                    Some(PlaceBackend::Mcp(mcp)) => match tool {
                        "user_mouse_input" => crate::studio_embed::forward_mouse(mcp, &args_obj)?,
                        "user_keyboard_input" => {
                            crate::studio_embed::forward_keyboard(mcp, &args_obj)?
                        }
                        _ => mcp.call_official_tool(tool, args_obj.clone())?,
                    },
                    _ => return Err("Studio not connected".to_string()),
                };

                let captures_frame = matches!(
                    tool,
                    "multi_edit" | "execute_luau" | "user_mouse_input" | "user_keyboard_input"
                );
                let fseq = if captures_frame {
                    self.auto_capture_mcp_frame()
                } else {
                    None
                };

                let (kind, path, class_name, summary) = match tool {
                    "execute_luau" => (
                        StudioActionKind::LuauExecuted,
                        "ServerScriptService".to_string(),
                        "Script".to_string(),
                        "Executed Luau inside live Roblox Studio".to_string(),
                    ),
                    "multi_edit" => (
                        StudioActionKind::ScriptWritten,
                        args_obj
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ServerScriptService")
                            .to_string(),
                        "Script".to_string(),
                        "Applied multi_edit to Studio script".to_string(),
                    ),
                    "user_mouse_input" => (
                        StudioActionKind::CameraFocus,
                        "Workspace.CurrentCamera".to_string(),
                        "Camera".to_string(),
                        format!(
                            "Mouse {} ({}, {}) in Embedded Studio",
                            args_obj
                                .get("action")
                                .and_then(|v| v.as_str())
                                .unwrap_or("click"),
                            args_obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            args_obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        ),
                    ),
                    "user_keyboard_input" => (
                        StudioActionKind::CameraFocus,
                        "Workspace.CurrentCamera".to_string(),
                        "UserInputService".to_string(),
                        format!(
                            "Keyboard {} in Embedded Studio",
                            args_obj
                                .get("key")
                                .or_else(|| args_obj.get("text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("input")
                        ),
                    ),
                    _ => (
                        StudioActionKind::CameraFocus,
                        "Workspace".to_string(),
                        "StudioService".to_string(),
                        format!("Called Roblox Studio tool {tool}"),
                    ),
                };
                if captures_frame {
                    let mut ev = StudioActionInput::new(kind, summary);
                    ev.path = path;
                    ev.class_name = class_name;
                    ev.frame_seq = fseq;
                    if tool == "execute_luau" {
                        if let Some(code) = args_obj
                            .get("code")
                            .or_else(|| args_obj.get("source"))
                            .and_then(|v| v.as_str())
                        {
                            ev.code_snippet = Some(snippet_from_source(code));
                        }
                    }
                    self.emit_studio_action(ev);
                }
                Ok((parsed, fseq))
            }
            Some(PlaceBackend::Plugin(_)) => self.invoke_plugin_studio_tool(chat_id, tool, &args_obj),
            Some(PlaceBackend::Local(_)) => self.invoke_local_studio_tool(chat_id, tool, &args_obj),
            None => Err("Studio not connected".to_string()),
        }
    }

    fn invoke_plugin_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: &Value,
    ) -> Result<(Value, Option<u64>), String> {
        match tool {
            "screen_capture" | "user_mouse_input" | "user_keyboard_input" => Err(format!(
                "Tool '{tool}' requires Embedded Studio / MCP (Companion Plugin uses Studio's native viewport)."
            )),
            "execute_luau" | "get_console_output" | "start_stop_play" => {
                let text = self.studio_mut()?.call_tool(tool, args.clone())?;
                let parsed = serde_json::from_str::<Value>(&text)
                    .unwrap_or_else(|_| json!({ "ok": true, "output": text }));
                Ok((parsed, None))
            }
            "list_roblox_studios" => Ok((
                json!({ "studios": ["companion_plugin"], "studio_mode": "plugin_live" }),
                None,
            )),
            "get_studio_state" => {
                let tree = self.place_tree()?;
                let ic = tree.get("instances").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                let sc = tree.get("scripts").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                Ok((
                    json!({
                        "studio_mode": "plugin_live",
                        "datamodel_type": "Edit",
                        "is_playing": false,
                        "instance_count": ic,
                        "script_count": sc,
                    }),
                    None,
                ))
            }
            "search_game_tree" | "inspect_instance" | "script_read" | "script_grep" => {
                let tree = self.place_tree()?;
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&tree);
                let mut helper = Agent::with_local_place(self.model.clone(), tmp);
                helper.invoke_local_studio_tool(chat_id, tool, args)
            }
            "multi_edit" => {
                let path = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "multi_edit requires string 'path'".to_string())?;
                let edits = args
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "multi_edit requires array 'edits'".to_string())?;
                let mut src = self
                    .prev_script_current(path)?
                    .ok_or_else(|| format!("no script at {path}"))?;
                let mut applied = 0usize;
                for ed in edits {
                    let old_s = ed
                        .get("old_str")
                        .or_else(|| ed.get("old_text"))
                        .or_else(|| ed.get("find"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'old_str'".to_string())?;
                    let new_s = ed
                        .get("new_str")
                        .or_else(|| ed.get("new_text"))
                        .or_else(|| ed.get("replace"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'new_str'".to_string())?;
                    if !src.contains(old_s) {
                        return Err(format!("multi_edit target not found in {path}"));
                    }
                    src = src.replacen(old_s, new_s, 1);
                    applied += 1;
                }
                let updated = self.update_script_source(chat_id, path, &src)?;
                let mut out = updated.to_json();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("edits_applied".to_string(), json!(applied));
                }
                Ok((out, None))
            }
            other => Err(format!("unsupported Studio tool: {other}")),
        }
    }

    fn invoke_local_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: &Value,
    ) -> Result<(Value, Option<u64>), String> {
        match tool {
            "screen_capture" | "user_mouse_input" | "user_keyboard_input" | "execute_luau" => Err(
                format!(
                    "Tool '{tool}' requires a connected live Roblox Studio MCP session (currently running Local Place Engine). Click Launch & Embed Studio or Connect MCP."
                ),
            ),
            "list_roblox_studios" => Ok((
                json!({
                    "studios": [],
                    "studio_mode": "local_place",
                    "note": "Running Local Place Engine; launch or connect Roblox Studio MCP to attach live instances."
                }),
                None,
            )),
            "get_studio_state" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                Ok((
                    json!({
                        "studio_mode": "local_place",
                        "datamodel_type": "Edit",
                        "is_playing": false,
                        "game_name": local.game_name(),
                        "instance_count": local.instances().len(),
                        "script_count": local.scripts().len(),
                    }),
                    None,
                ))
            }
            "search_game_tree" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let q = args
                    .get("query")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                let class_filter = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty());
                let mut matches = Vec::new();
                for (path, rec) in local.instances() {
                    let cls = rec
                        .get("class_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Instance");
                    if !q.is_empty() && !path.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if let Some(cf) = &class_filter {
                        if cls.to_ascii_lowercase() != *cf {
                            continue;
                        }
                    }
                    matches.push(json!({
                        "path": path,
                        "name": path.rsplit('.').next().unwrap_or(path),
                        "class_name": cls,
                        "kind": "instance",
                    }));
                }
                for (path, src) in local.scripts() {
                    let cls = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.")
                    {
                        "LocalScript"
                    } else if path.starts_with("ReplicatedStorage.") {
                        "ModuleScript"
                    } else {
                        "Script"
                    };
                    if !q.is_empty() && !path.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if let Some(cf) = &class_filter {
                        if cls.to_ascii_lowercase() != *cf {
                            continue;
                        }
                    }
                    matches.push(json!({
                        "path": path,
                        "name": path.rsplit('.').next().unwrap_or(path),
                        "class_name": cls,
                        "kind": "script",
                        "lines": src.lines().count(),
                    }));
                }
                Ok((json!({ "count": matches.len(), "matches": matches }), None))
            }
            "inspect_instance" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "inspect_instance requires string 'path'".to_string())?;
                if let Some(rec) = local.instances().get(path) {
                    let prefix = format!("{path}.");
                    let children: Vec<String> = local
                        .instances()
                        .keys()
                        .chain(local.scripts().keys())
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    return Ok((
                        json!({
                            "path": path,
                            "kind": "instance",
                            "class_name": rec.get("class_name").and_then(|v| v.as_str()).unwrap_or("Instance"),
                            "properties": rec.get("properties").cloned().unwrap_or_else(|| json!({})),
                            "children": children,
                        }),
                        None,
                    ));
                }
                if let Some(src) = local.scripts().get(path) {
                    return Ok((
                        json!({
                            "path": path,
                            "kind": "script",
                            "class_name": if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") { "LocalScript" } else { "Script" },
                            "lines": src.lines().count(),
                            "bytes": src.len(),
                            "source": src,
                        }),
                        None,
                    ));
                }
                Err(format!("no instance or script at {path}"))
            }
            "script_read" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "script_read requires string 'path'".to_string())?;
                let src = local
                    .scripts()
                    .get(path)
                    .ok_or_else(|| format!("no script at {path}"))?;
                let all_lines: Vec<&str> = src.lines().collect();
                let start = args
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1) as usize;
                let end = args
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(all_lines.len())
                    .min(all_lines.len());
                let slice = if start <= end && start >= 1 {
                    all_lines[start - 1..end].join("\n")
                } else {
                    String::new()
                };
                Ok((
                    json!({
                        "path": path,
                        "total_lines": all_lines.len(),
                        "start_line": start,
                        "end_line": end,
                        "source": slice,
                    }),
                    None,
                ))
            }
            "script_grep" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let needle = args
                    .get("query")
                    .or_else(|| args.get("pattern"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "script_grep requires string 'query' or 'pattern'".to_string())?;
                let low = needle.to_ascii_lowercase();
                let mut hits = Vec::new();
                for (path, src) in local.scripts() {
                    for (idx, line) in src.lines().enumerate() {
                        if line.to_ascii_lowercase().contains(&low) {
                            hits.push(json!({
                                "path": path,
                                "line": idx + 1,
                                "text": line,
                            }));
                            if hits.len() >= 200 {
                                break;
                            }
                        }
                    }
                    if hits.len() >= 200 {
                        break;
                    }
                }
                Ok((
                    json!({
                        "query": needle,
                        "count": hits.len(),
                        "matches": hits,
                    }),
                    None,
                ))
            }
            "multi_edit" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "multi_edit requires string 'path'".to_string())?;
                let edits = args
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "multi_edit requires array 'edits'".to_string())?;
                let mut src = match self.studio.as_ref() {
                    Some(PlaceBackend::Local(local)) => local
                        .scripts()
                        .get(path)
                        .cloned()
                        .ok_or_else(|| format!("no script at {path}"))?,
                    _ => return Err("Studio not connected".to_string()),
                };
                let mut applied = 0usize;
                for ed in edits {
                    let old_s = ed
                        .get("old_str")
                        .or_else(|| ed.get("old_text"))
                        .or_else(|| ed.get("find"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'old_str'".to_string())?;
                    let new_s = ed
                        .get("new_str")
                        .or_else(|| ed.get("new_text"))
                        .or_else(|| ed.get("replace"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'new_str'".to_string())?;
                    if !src.contains(old_s) {
                        return Err(format!("multi_edit target not found in {path}"));
                    }
                    src = src.replacen(old_s, new_s, 1);
                    applied += 1;
                }
                let updated = self.update_script_source(chat_id, path, &src)?;
                let mut out = updated.to_json();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("edits_applied".to_string(), json!(applied));
                }
                Ok((out, None))
            }
            "get_console_output" => {
                let console = self
                    .studio_mut()?
                    .call_tool("get_console", json!({}))
                    .unwrap_or_default();
                let lines: Vec<String> = console
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                Ok((json!({ "lines": lines, "console": lines.join("\n") }), None))
            }
            "start_stop_play" => {
                let mode = args
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("start_play");
                if mode == "stop" {
                    return Ok((json!({ "ok": true, "mode": "stop", "status": "stopped" }), None));
                }
                let res = self.run_studio_playtest()?;
                Ok((res, None))
            }
            other => Err(format!("unsupported Studio tool: {other}")),
        }
    }

    /// Bind uploaded/generated game card art assets into the active LocalPlace's
    /// `Workspace.CardGallery` as 3D Showcase Parts with `Decal` (`Texture`) and
    /// `SurfaceGui` -> `ImageLabel` (`Image`) instances for `.rbxlx` export.
    pub fn bind_assets(&mut self, assets: &[crate::assets::AssetMetadata]) -> usize {
        let count = match self.studio.as_mut() {
            Some(PlaceBackend::Local(place)) => crate::assets::bind_assets_to_place(place, assets),
            _ => 0,
        };
        if count > 0 && self.studio_stream.is_some() {
            let total = assets.len();
            for (idx, asset) in assets.iter().enumerate() {
                let safe_id = asset.card_id.replace('-', "_");
                let x_pos = ((idx as f64) - ((total as f64 - 1.0) / 2.0)) * 9.0;
                let mut ev = StudioActionInput::new(
                    StudioActionKind::DecalBound,
                    format!("Bound {} ({}) onto 3D CardGallery showcase", asset.title, asset.filename),
                );
                ev.path = format!("Workspace.CardGallery.Showcase_{safe_id}.CardArtDecal");
                ev.class_name = "Decal".to_string();
                ev.position = Some([x_pos, 5.0, 28.0]);
                ev.size = Some([6.0, 8.5, 0.8]);
                ev.color = Some("#4F7CF0".to_string());
                self.emit_studio_action(ev);
            }
        }
        count
    }

    /// Create or update a single Instance at `path` (e.g. `Workspace.JumpPad`),
    /// merging new properties onto any existing properties and preserving its
    /// existing `class_name` when `class_name` is `None`.
    pub fn set_instance_properties(
        &mut self,
        path: &str,
        class_name: Option<&str>,
        properties: &Value,
    ) -> Result<Value, String> {
        check_path(path)?;
        let (parent, name) = path
            .rsplit_once('.')
            .ok_or_else(|| format!("instance path must be Parent.Name (got '{path}')"))?;
        if !properties.is_object() {
            return Err("properties must be a JSON object".to_string());
        }
        let existing = self.prev_instance(path)?;
        let resolved_class = class_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|v| v.get("class_name").and_then(|c| c.as_str()).map(String::from))
            })
            .unwrap_or_else(|| "Part".to_string());
        let mut merged_props = existing
            .as_ref()
            .and_then(|v| v.get("properties").and_then(|p| p.as_object()).cloned())
            .unwrap_or_default();
        if let Some(new_obj) = properties.as_object() {
            for (k, v) in new_obj {
                merged_props.insert(k.clone(), v.clone());
            }
        }
        let props_val = Value::Object(merged_props);
        self.studio_mut()?.call_tool(
            "create_instance",
            json!({
                "class_name": resolved_class,
                "name": name,
                "parent": parent,
                "properties": props_val,
            }),
        )?;
        let kind = if existing.is_some() {
            StudioActionKind::PartUpdated
        } else {
            StudioActionKind::PartCreated
        };
        let mut ev = StudioActionInput::new(
            kind,
            if existing.is_some() {
                format!("Updated {path} ({resolved_class})")
            } else {
                format!("Created {path} ({resolved_class})")
            },
        );
        ev.path = path.to_string();
        ev.class_name = resolved_class.clone();
        ev.position = extract_vec3_prop(Some(&props_val), "Position");
        ev.size = extract_vec3_prop(Some(&props_val), "Size");
        ev.color = extract_color_prop(Some(&props_val));
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(json!({
            "path": path,
            "class_name": resolved_class,
            "properties": props_val,
        }))
    }

    /// Delete an Instance at `path` cleanly.
    pub fn delete_instance_at(&mut self, path: &str) -> Result<(), String> {
        check_path(path)?;
        self.studio_mut()?
            .call_tool("delete_instance", json!({ "path": path }))?;
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartUpdated,
            format!("Removed {path} from Place"),
        );
        ev.path = path.to_string();
        ev.class_name = "Instance".to_string();
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(())
    }

    /// Replace the active Place tree with `target` (`{"instances": {...}, "scripts": {...}}`),
    /// using an O(N) atomic bulk replace for `LocalPlace` or exact delta calls for `Mcp`,
    /// and returning the computed `PlaceDiff`.
    pub fn apply_place_tree(&mut self, target: &Value) -> Result<PlaceDiff, String> {
        let before = self.place_tree()?;
        let diff = compute_diff(&before, target);
        if let Some(PlaceBackend::Local(local)) = self.studio.as_mut() {
            local.replace_tree(target);
            return Ok(diff);
        }
        let empty = serde_json::Map::new();
        let target_instances = target
            .get("instances")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty);
        let target_scripts = target
            .get("scripts")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty);

        for path in &diff.removed_scripts {
            self.studio_mut()?
                .call_tool("delete_script", json!({ "path": path }))?;
        }
        for path in &diff.removed_instances {
            self.studio_mut()?
                .call_tool("delete_instance", json!({ "path": path }))?;
        }
        for path in diff
            .added_instances
            .iter()
            .chain(diff.modified_instances.iter())
        {
            check_path(path)?;
            let Some((parent, name)) = path.rsplit_once('.') else {
                continue;
            };
            let rec = target_instances.get(path);
            let class_name = rec
                .and_then(|r| r.get("class_name").and_then(|c| c.as_str()))
                .unwrap_or("Part");
            let properties = rec
                .and_then(|r| r.get("properties"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            self.studio_mut()?.call_tool(
                "create_instance",
                json!({
                    "class_name": class_name,
                    "name": name,
                    "parent": parent,
                    "properties": properties,
                }),
            )?;
        }
        for path in diff.added_scripts.iter().chain(diff.modified_scripts.iter()) {
            check_path(path)?;
            let src = target_scripts
                .get(path)
                .and_then(|s| s.as_str())
                .unwrap_or("");
            self.studio_mut()?
                .call_tool("write_script", json!({ "path": path, "source": src }))?;
        }
        Ok(diff)
    }

    /// Refuse honestly when the connected bridge is known to lack a
    /// tool the agent cannot build without. Accepts:
    /// - Persistent `LocalPlace` or connected Companion Studio `Plugin`,
    /// - Official Roblox Studio MCP (`execute_luau` + `start_stop_play` + `get_console_output`), or
    /// - Legacy 7-tool Studio MCP bridges.
    fn check_bridge_capabilities(&self) -> Result<(), String> {
        let Some(studio) = &self.studio else {
            return Ok(());
        };
        if matches!(studio, PlaceBackend::Local(_) | PlaceBackend::Plugin(_)) {
            return Ok(());
        }
        if !studio.tools_known() {
            return Ok(());
        }
        const REQUIRED: [&str; 7] = [
            "write_script",
            "delete_script",
            "create_instance",
            "delete_instance",
            "playtest",
            "get_console",
            "get_place_tree",
        ];
        if let PlaceBackend::Mcp(mcp) = studio {
            let advertises_any_legacy = REQUIRED.iter().any(|t| mcp.has_exact_tool(t));
            if mcp.has_official_bridge() && !advertises_any_legacy {
                return Ok(());
            }
        }
        let missing: Vec<&str> = REQUIRED
            .iter()
            .filter(|t| !studio.has_tool(t))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "Studio bridge does not offer these tools: {} — this bridge cannot build. \
                 Connect a Studio MCP server that offers {} (or official execute_luau + start_stop_play + get_console_output).",
                missing.join(", "),
                REQUIRED.join(", ")
            ));
        }
        Ok(())
    }

    /// Run one ask in one chat (default `Agent` mode). Whatever happens —
    /// success, honest failure, mid-ask error — the changes this ask checkpointed
    /// are harvested into that chat's revertable change set.
    pub fn run_ask(&mut self, chat_id: &str, ask: &str) -> Result<RunOutcome, AskFailure> {
        self.run_ask_with_options(chat_id, ask, AskOptions::default())
    }

    /// Run one ask with explicit RoPilot composer options (`mode`, `reasoning`, `context_paths`).
    pub fn run_ask_with_options(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: AskOptions,
    ) -> Result<RunOutcome, AskFailure> {
        self.current_changes.clear();
        self.begin_build_trace(chat_id, ask);
        let result = self.run_ask_inner(chat_id, ask, &opts, false);
        match &result {
            Ok(o) if o.refused => self.emit_event("failed", "Build refused", &o.summary, None, None, 100),
            Ok(o) => self.emit_event("done", "Build verified", &o.summary, None, None, 100),
            Err(f) => self.emit_event("failed", "Build failed", &f.message, None, None, 100),
        }
        if opts.mode != AskMode::Ask {
            let skills = match &result {
                Ok(o) => o.skills_used.clone(),
                Err(f) => f.skills_used.clone(),
            };
            self.harvest(chat_id, skills);
        } else {
            self.current_changes.clear();
        }
        result
    }

    /// Run an ask in isolated staged/proposal mode (`mode == "plan"` or `review_diffs == true`):
    /// clones the active place tree into a shadow `LocalPlace`, runs all agent steps against the
    /// shadow copy (ZERO live Studio mutation!), computes Static Luau Prover diagnostics on the
    /// staged tree, and returns before/after trees for unified diff review & selective apply.
    pub fn run_ask_staged(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: AskOptions,
    ) -> Result<StagedAskOutcome, AskFailure> {
        let fail = |message: String| AskFailure {
            message,
            thread: Vec::new(),
            transcript: Vec::new(),
            skills_used: Vec::new(),
        };
        if !self.model.is_configured() {
            return Err(fail(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
            ));
        }
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
            ));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e));
        }
        let before_tree = self.place_tree().map_err(fail)?;
        let game_name = match &self.studio {
            Some(PlaceBackend::Local(l)) => l.game_name().to_string(),
            _ => "OpenRO".to_string(),
        };
        let mut shadow = LocalPlace::in_memory(game_name);
        shadow.replace_tree(&before_tree);

        let mut shadow_agent = Agent::with_local_place(self.model.clone(), shadow);
        shadow_agent.set_library_prompt(self.library_prompt.clone());
        shadow_agent.set_market_dossier_prompt(self.market_dossier_prompt.clone());
        if let Some(root) = &self.assets_root {
            shadow_agent.set_assets_root(root.clone());
        }
        if let Some(ev) = &self.events {
            shadow_agent.attach_events(Arc::clone(ev));
        }
        shadow_agent.begin_build_trace(chat_id, ask);
        let outcome = shadow_agent.run_ask_inner(chat_id, ask, &opts, true)?;
        let (after_tree, diagnostics) = match shadow_agent.studio.as_ref() {
            Some(PlaceBackend::Local(local)) => (local.tree_json(), local.analyze().to_json()),
            _ => (
                before_tree.clone(),
                json!({ "ok": true, "error_count": 0, "warning_count": 0, "diagnostics": [] }),
            ),
        };
        shadow_agent.emit_event("done", "Proposal ready", &outcome.summary, None, None, 100);
        Ok(StagedAskOutcome {
            outcome,
            before_tree,
            after_tree,
            diagnostics,
        })
    }

    /// Push this ask's checkpoints (if it changed anything) onto the
    /// chat's change-set stack, with the skills it ran.
    fn harvest(&mut self, chat_id: &str, skills: Vec<String>) {
        let harvested = std::mem::take(&mut self.current_changes);
        if !harvested.is_empty() {
            self.changes_by_chat
                .entry(chat_id.to_string())
                .or_default()
                .push(ChangeSet { changes: harvested, skills });
            self.redo_by_chat.remove(chat_id);
            self.save_changes();
            self.sync_undo();
        }
    }

    /// Skills the chat's newest change set ran (what a revert takes back).
    pub fn top_skills(&self, chat_id: &str) -> Vec<String> {
        self.changes_by_chat
            .get(chat_id)
            .and_then(|stack| stack.last())
            .map(|set| set.skills.clone())
            .unwrap_or_default()
    }

    /// Skills still held by the chat's remaining change sets.
    pub fn chat_skills(&self, chat_id: &str) -> Vec<String> {
        self.changes_by_chat
            .get(chat_id)
            .map(|stack| stack.iter().flat_map(|set| set.skills.clone()).collect())
            .unwrap_or_default()
    }

    /// Build one of the creativity engine's ideas by id: the idea's
    /// built-in plan runs through `run_plan`.
    pub fn build_idea(&mut self, chat_id: &str, idea_id: &str) -> Result<RunOutcome, AskFailure> {
        let idea = crate::ideas::find(idea_id).ok_or_else(|| AskFailure {
            message: format!("unknown idea: {idea_id}"),
            thread: Vec::new(),
            transcript: Vec::new(),
            skills_used: Vec::new(),
        })?;
        self.run_plan(chat_id, idea.title, &idea.plan)
    }

    /// Run a built-in plan (from the creativity engine) deterministically:
    /// the same step machinery as a model ask — skills generate, tools
    /// apply, changes checkpoint — but no model is consulted, and the
    /// thread says so. The done gate is identical: the plan is only
    /// called built when a playtest has passed since its last change;
    /// a failed playtest ends the build in an honest failure naming
    /// the console output, change set still revertable. Change-set
    /// harvesting matches `run_ask`: whatever the plan changed belongs
    /// to the chat, on every exit path.
    pub fn run_plan(
        &mut self,
        chat_id: &str,
        label: &str,
        steps: &[Value],
    ) -> Result<RunOutcome, AskFailure> {
        self.current_changes.clear();
        self.begin_build_trace(chat_id, label);
        let result = self.run_plan_inner(chat_id, label, steps);
        // Every failed idea build says Undo last build when any of its
        // changes landed (the playtest message already says it).
        // The pointer only when this plan's own changes landed; a try that
        // added nothing must not send the user to undo the previous build.
        let result = match result {
            Err(mut f) if !f.message.contains("Undo last build") && !f.message.contains("Nothing from this try") => {
                if !f.message.ends_with('.') {
                    f.message.push('.');
                }
                if self.current_changes.iter().any(|c| !c.is_net_zero()) {
                    f.message.push_str(" Undo last build undoes what it changed.");
                } else {
                    f.message.push_str(" Nothing from this try was added to your game.");
                }
                Err(f)
            }
            other => other,
        };
        match &result {
            Ok(o) => self.emit_event("done", "Blueprint verified", &o.summary, None, None, 100),
            Err(f) => self.emit_event("failed", "Blueprint stopped", &f.message, None, None, 100),
        }
        let skills = match &result {
            Ok(o) => o.skills_used.clone(),
            Err(f) => f.skills_used.clone(),
        };
        self.harvest(chat_id, skills);
        result
    }

    /// Record a playtest verdict for the chat's place.
    fn record_verdict(&mut self, chat_id: &str, outcome: &PlaytestOutcome) {
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        match outcome {
            PlaytestOutcome::Passed(_) => {
                proof.unproven = false;
                proof.failed_console = None;
            }
            PlaytestOutcome::Failed(_, console) => {
                proof.unproven = true;
                proof.failed_console = Some(console.clone());
            }
            PlaytestOutcome::Unreadable(..) => {
                proof.unproven = true;
                proof.failed_console = None;
            }
        }
        self.save_proof();
    }

    /// A change landed in the chat's place: it is unproven until a pass.
    fn note_change(&mut self, chat_id: &str) {
        self.proof_by_chat.entry(chat_id.to_string()).or_default().unproven = true;
        self.save_proof();
    }

    fn run_plan_inner(
        &mut self,
        chat_id: &str,
        label: &str,
        steps: &[Value],
    ) -> Result<RunOutcome, AskFailure> {
        let mut thread: Vec<String> = Vec::new();
        let mut transcript: Vec<String> = Vec::new();
        let mut skills_used: Vec<String> = Vec::new();
        let fail = |message: String,
                    thread: &Vec<String>,
                    transcript: &Vec<String>,
                    skills_used: &Vec<String>| AskFailure {
            message,
            thread: thread.clone(),
            transcript: transcript.clone(),
            skills_used: skills_used.clone(),
        };
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
                &thread, &transcript, &skills_used));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e, &thread, &transcript, &skills_used));
        }
        self.generated.clear();
        self.last_skill = None;
        thread.push(
            "OpenRO: Building it now — these are my built-in steps, no model call involved."
                .to_string(),
        );
        transcript.push(format!("built-in plan: {label} ({} steps)", steps.len()));
        let mut writes: HashMap<String, String> = HashMap::new();
        let mut changes_made = 0usize;
        let mut unproven_changes = false;

        // One playtest, plan semantics: a pass clears the done gate; a
        // failure ends the build honestly (a built-in plan has no
        // repairer — the failure names the console and the changes
        // stay revertable).
        macro_rules! plan_playtest {
            () => {
                match self.run_playtest(&mut transcript, &mut thread) {
                    Ok(outcome) => {
                        self.record_verdict(chat_id, &outcome);
                        match outcome {
                            PlaytestOutcome::Passed(_) => {
                                unproven_changes = false;
                            }
                            PlaytestOutcome::Failed(_, console) => {
                                // run_playtest narrated "reading the console
                                // and repairing" — a plan does not repair, so
                                // correct the line before it misleads.
                                if let Some(last) = thread.last_mut() {
                                    if last.starts_with("Playtest failed") {
                                        *last = "Playtest failed.".to_string();
                                    }
                                }
                                return Err(fail(
                                    format!(
                                        "my built-in plan for “{label}” failed its playtest — nothing \
                                         is called done. Console output: {console} Undo last build \
                                         undoes what it changed."
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            PlaytestOutcome::Unreadable(_, raw) => {
                                return Err(fail(
                                    format!(
                                        "my built-in plan for “{label}” got a playtest result that \
                                         could not be read as a verdict ({raw}) — the build counts \
                                         as unproven, not done"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                        }
                    }
                    Err(e) => {
                        transcript.push(format!("bridge detail: {e}"));
                        return Err(fail(plain_bridge(&e), &thread, &transcript, &skills_used));
                    }
                }
            };
        }

        let mut skipped = 0usize;
        for step in steps {
            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(_) => skills_used.push(skill.to_string()),
                    Err(e) => {
                        return Err(fail(
                            format!("my built-in plan for “{label}” stopped while generating a step: {e}"),
                            &thread, &transcript, &skills_used))
                    }
                }
                continue;
            }
            if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                if tool == "playtest" {
                    plan_playtest!();
                    continue;
                }
                if tool == "write_script" {
                    match self.run_write_script(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        None,
                    ) {
                        Ok(_) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                        }
                        Err(e) => {
                            transcript.push(format!("bridge detail: {e}"));
                            return Err(fail(
                                format!("built-in plan stopped at write_script: {}", plain_bridge(&e)),
                                &thread, &transcript, &skills_used))
                        }
                    }
                    continue;
                }
                // An instance the place already holds with the same
                // class is a success: the step is skipped, not failed.
                if tool == "create_instance" {
                    match self.already_in_place(&args) {
                        Ok(true) => {
                            skipped += 1;
                            transcript.push(format!("create_instance skipped: {args} already in place"));
                            continue;
                        }
                        Ok(false) => {}
                        Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
                    }
                }
                match self.run_tool(tool, &args, &mut transcript, &mut thread) {
                    Ok(_) => {
                        if matches!(
                            tool,
                            "delete_script" | "create_instance" | "delete_instance"
                        ) {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                        }
                    }
                    Err(e) => {
                        transcript.push(format!("bridge detail: {e}"));
                        return Err(fail(
                            format!("my built-in plan for “{label}” stopped while saving to the game: {}", plain_bridge(&e)),
                            &thread, &transcript, &skills_used))
                    }
                }
                continue;
            }
            return Err(fail(
                format!("built-in plan held an unreadable step: {step}"),
                &thread, &transcript, &skills_used));
        }

        if changes_made == 0 && skipped > 0 {
            return Err(fail(
                format!("my built-in plan for “{label}” is already in your game, so there was nothing to add."),
                &thread, &transcript, &skills_used));
        }
        if changes_made == 0 {
            return Err(fail(
                format!(
                    "my built-in plan for “{label}” made no changes — the plan is broken, \
                     and I won't pretend otherwise"
                ),
                &thread, &transcript, &skills_used));
        }
        if unproven_changes {
            plan_playtest!();
            debug_assert!(!unproven_changes);
        }
        // A plan whose changes cancel out (created then deleted,
        // rewritten identically) built nothing — the model path flags
        // the equivalent claim with a factual line; a plan is held to
        // the same standard and fails honestly instead.
        if self.current_changes.iter().all(|c| c.is_net_zero()) {
            return Err(fail(
                format!(
                    "my built-in plan for “{label}” left the game exactly as it found it — \
                     nothing was actually built, and I won't call it built"
                ),
                &thread,
                &transcript,
                &skills_used,
            ));
        }
        /* The verdict band directly above already states the pass —
           the summary names the build, once (elevation checker note). */
        let summary = format!("Built “{label}”.");
        thread.push(format!("OpenRO: {summary}"));
        transcript.push(format!("plan done: {summary}"));
        Ok(RunOutcome {
            summary,
            thread,
            transcript,
            skills_used,
            refused: false,
            changed: true,
        })
    }

    fn run_ask_inner(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: &AskOptions,
        staged: bool,
    ) -> Result<RunOutcome, AskFailure> {
        let read_only = opts.mode == AskMode::Ask;
        let mut thread: Vec<String> = Vec::new();
        let mut transcript: Vec<String> = Vec::new();
        let mut skills_used: Vec<String> = Vec::new();
        let fail = |message: String,
                    thread: &Vec<String>,
                    transcript: &Vec<String>,
                    skills_used: &Vec<String>| AskFailure {
            message,
            thread: thread.clone(),
            transcript: transcript.clone(),
            skills_used: skills_used.clone(),
        };
        if !self.model.is_configured() {
            return Err(fail(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
                &thread, &transcript, &skills_used));
        }
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
                &thread, &transcript, &skills_used));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e, &thread, &transcript, &skills_used));
        }
        self.generated.clear();
        self.last_skill = None;
        let mut full_system = self.effective_system_prompt();
        if read_only {
            full_system.push_str(
                "\n\nACTIVE MODE: ASK (READ-ONLY). You may inspect the game with get_place_tree, \
                 search_game_tree, inspect_instance, script_read, script_grep, or get_console_output. \
                 Do NOT call mutating tools (write_luau, write_script, multi_edit, create_instance, \
                 delete_instance, delete_script, execute_luau, playtest). Reply with {\"done\": true, \"summary\": \"...\"}.",
            );
        } else if staged {
            full_system.push_str(
                "\n\nACTIVE MODE: PLAN / STAGED DIFF REVIEW. Your edits run in an isolated preview so \
                 the user can review exact unified diffs before applying them to Roblox Studio.",
            );
        }
        let reasoning_norm = opts.reasoning.trim().to_ascii_lowercase();
        if matches!(reasoning_norm.as_str(), "low" | "medium" | "high") {
            full_system.push_str(&format!(
                "\nREASONING DEPTH: {reasoning_norm}."
            ));
        }

        // Inject compact Whole-Game Context Summary + requested context_paths on turn 1.
        let compact = self.model.is_loopback() && !self.model.is_antigravity();
        let mut turn1_parts = vec![ask.to_string()];
        if let Some(proj_root) = self.effective_project_root() {
            let budget = if compact { 2_400 } else { 14_000 };
            let pctx = build_project_context(&proj_root, ask, &opts.context_paths, budget);
            if !pctx.is_empty() {
                turn1_parts.push(pctx);
            }
        }
        if let Ok(tree) = self.place_tree() {
            let ctx = build_whole_game_context(&tree, &opts.context_paths, compact);
            if !ctx.is_empty() {
                turn1_parts.push(ctx);
            }
        }
        let turn1_user = turn1_parts.join("\n\n");

        let mut messages = vec![
            json!({ "role": "system", "content": full_system }),
            json!({ "role": "user", "content": turn1_user }),
        ];
        let mut summary: Option<String> = None;
        let mut failed_playtests = 0usize;
        let mut writes: HashMap<String, String> = HashMap::new();
        let mut last_failed_console: Option<String> = None;
        // Changes no passing playtest has covered yet (the done gate),
        // and whether this ask changed anything at all (the claim flag
        // on an empty-handed "done").
        let mut unproven_changes = false;
        let mut changes_made = 0usize;
        // Plain-words reasons for each step that failed in this ask, and
        // the last script this ask wrote (named when the ask gives up).
        let mut problems: Vec<String> = Vec::new();
        // The user-facing sentence when a prerequisite gate refused a step.
        let mut gate_sentence: Option<String> = None;
        let mut last_written: Option<String> = None;

        let step_limit = self.max_steps_override.unwrap_or(MAX_STEPS).clamp(4, 120);
        let mut consecutive_parse_errors = 0usize;
        for step_idx in 0..step_limit {
            if self
                .cancel_flag
                .as_ref()
                .is_some_and(|f| f.load(std::sync::atomic::Ordering::SeqCst))
            {
                return Err(fail(
                    "Run was cancelled.".to_string(),
                    &thread,
                    &transcript,
                    &skills_used,
                ));
            }
            let content = match self.model.chat(&messages) {
                Ok(c) => c,
                Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
            };
            messages.push(json!({ "role": "assistant", "content": content }));
            transcript.push(format!("model step: {content}"));
            let step = match parse_step(&content) {
                Ok(s) => {
                    consecutive_parse_errors = 0;
                    s
                }
                Err(e) => {
                    consecutive_parse_errors += 1;
                    transcript.push(format!("unparseable model step ({consecutive_parse_errors}): {e}"));
                    if consecutive_parse_errors >= 4 {
                        return Err(fail(
                            format!("The model did not finish within {consecutive_parse_errors} attempts (repeatedly sent invalid JSON steps). Last error: {e}"),
                            &thread,
                            &transcript,
                            &skills_used,
                        ));
                    }
                    messages.push(json!({
                        "role": "user",
                        "content": format!(
                            "Format error: {e}. Reply with ONLY one valid JSON object on a single message — no prose or markdown outside the JSON — e.g. {{\"tool\":\"project_read_file\",\"args\":{{\"path\":\"...\"}}}} or {{\"done\":true,\"summary\":\"...\"}}."
                        )
                    }));
                    continue;
                }
            };
            if let Some(sink) = &self.step_sink {
                let label = step
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .map(|t| format!("tool:{t}"))
                    .or_else(|| step.get("skill").and_then(|v| v.as_str()).map(|k| format!("skill:{k}")))
                    .unwrap_or_else(|| if step.get("done").and_then(|v| v.as_bool()) == Some(true) { "done".to_string() } else { "step".to_string() });
                sink(step_idx + 1, &label);
            }

            if let Some(batch) = step.get("steps").and_then(|v| v.as_array()) {
                if batch.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": "Empty steps array. Provide at least one tool/skill step or {\"done\": true, \"summary\": \"...\"}."
                    }));
                    continue;
                }
                let mut results: Vec<String> = Vec::new();
                let mut batch_failed = false;
                for (b_idx, sub) in batch.iter().take(24).enumerate() {
                    if self
                        .cancel_flag
                        .as_ref()
                        .is_some_and(|f| f.load(std::sync::atomic::Ordering::SeqCst))
                    {
                        return Err(fail("Run was cancelled.".to_string(), &thread, &transcript, &skills_used));
                    }
                    let Some(tool) = sub.get("tool").and_then(|v| v.as_str()) else {
                        results.push(format!("step[{b_idx}] error: missing 'tool' string"));
                        batch_failed = true;
                        break;
                    };
                    let args = sub.get("args").cloned().unwrap_or(json!({}));
                    if read_only && !is_read_only_tool(tool) {
                        results.push(format!("step[{b_idx}] {tool} refused: ask mode is read-only"));
                        batch_failed = true;
                        break;
                    }
                    match tool {
                        "write_luau" => match self.run_write_luau(
                            &args,
                            &mut transcript,
                            &mut thread,
                            &mut writes,
                            last_failed_console.as_deref(),
                        ) {
                            Ok(note) => {
                                changes_made += 1;
                                unproven_changes = true;
                                self.note_change(chat_id);
                                last_written = args.get("path").and_then(|v| v.as_str()).map(String::from);
                                results.push(format!("step[{b_idx}] {note}"));
                            }
                            Err(e) => {
                                problems.push(format!("batch step {b_idx} ({tool}) failed: {e}"));
                                results.push(format!("step[{b_idx}] error: {e}"));
                                batch_failed = true;
                                break;
                            }
                        },
                        "multi_edit" => match self.run_multi_edit(
                            &args,
                            &mut transcript,
                            &mut thread,
                            &mut writes,
                            last_failed_console.as_deref(),
                        ) {
                            Ok(note) => {
                                changes_made += 1;
                                unproven_changes = true;
                                self.note_change(chat_id);
                                last_written = args.get("path").and_then(|v| v.as_str()).map(String::from);
                                results.push(format!("step[{b_idx}] {note}"));
                            }
                            Err(e) => {
                                problems.push(format!("batch step {b_idx} ({tool}) failed: {e}"));
                                results.push(format!("step[{b_idx}] error: {e}"));
                                batch_failed = true;
                                break;
                            }
                        },
                        "playtest" => match self.run_playtest(&mut transcript, &mut thread) {
                            Ok(PlaytestOutcome::Passed(note)) => {
                                unproven_changes = false;
                                self.record_verdict(chat_id, &PlaytestOutcome::Passed(note.clone()));
                                results.push(format!("step[{b_idx}] {note}"));
                            }
                            Ok(PlaytestOutcome::Failed(_note, console)) => {
                                failed_playtests += 1;
                                last_failed_console = Some(console.clone());
                                self.record_verdict(chat_id, &PlaytestOutcome::Failed("failed".to_string(), console.clone()));
                                if failed_playtests >= MAX_REPAIRS {
                                    return Err(fail(
                                        format!("playtest still failing after {failed_playtests} attempts. Console: {console}"),
                                        &thread,
                                        &transcript,
                                        &skills_used,
                                    ));
                                }
                                results.push(format!("step[{b_idx}] playtest FAILED: {console}"));
                                batch_failed = true;
                                break;
                            }
                            Ok(PlaytestOutcome::Unreadable(_note, raw)) => {
                                results.push(format!("step[{b_idx}] playtest unreadable: {raw}"));
                                batch_failed = true;
                                break;
                            }
                            Err(e) => {
                                results.push(format!("step[{b_idx}] playtest error: {e}"));
                                batch_failed = true;
                                break;
                            }
                        },
                        "project_playtest" => match self.run_project_playtest(&args, &mut transcript, &mut thread) {
                            Ok((true, note)) => {
                                unproven_changes = false;
                                self.record_verdict(chat_id, &PlaytestOutcome::Passed(note.clone()));
                                results.push(format!("step[{b_idx}] {note}"));
                            }
                            Ok((false, console)) => {
                                failed_playtests += 1;
                                last_failed_console = Some(console.clone());
                                self.record_verdict(chat_id, &PlaytestOutcome::Failed("failed".to_string(), console.clone()));
                                if failed_playtests >= MAX_REPAIRS {
                                    return Err(fail(
                                        format!("project_playtest still failing after {failed_playtests} attempts. Console/check output: {console}"),
                                        &thread,
                                        &transcript,
                                        &skills_used,
                                    ));
                                }
                                results.push(format!("step[{b_idx}] project_playtest FAILED: {console}"));
                                batch_failed = true;
                                break;
                            }
                            Err(e) => {
                                results.push(format!("step[{b_idx}] project_playtest error: {e}"));
                                batch_failed = true;
                                break;
                            }
                        },
                        other => match self.run_tool(other, &args, &mut transcript, &mut thread) {
                            Ok(note) => {
                                if matches!(
                                    other,
                                    "delete_script"
                                        | "create_instance"
                                        | "delete_instance"
                                        | "project_write_file"
                                        | "project_edit_file"
                                        | "project_delete_file"
                                        | "project_update_manifest"
                                        | "project_scene_mutate"
                                        | "gui_save"
                                        | "gui_compile"
                                ) {
                                    changes_made += 1;
                                    let path_arg = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                    let needs_playtest = !matches!(other, "project_update_manifest" | "gui_save")
                                        && (matches!(other, "delete_script" | "create_instance" | "delete_instance" | "project_scene_mutate" | "gui_compile")
                                            || path_arg.ends_with(".luau")
                                            || path_arg.ends_with(".lua")
                                            || path_arg == "default.project.json");
                                    if needs_playtest {
                                        unproven_changes = true;
                                        self.note_change(chat_id);
                                    }
                                } else if matches!(
                                    other,
                                    "art_generate"
                                        | "image_process"
                                        | "art_pack_card"
                                        | "mesh_generate"
                                        | "blender_generate"
                                        | "audio_generate"
                                        | "assets_upload"
                                        | "creator_hub_setup"
                                        | "creator_hub_configure"
                                        | "assets_authorise"
                                        | "assets_codegen"
                                        | "products_sync"
                                        | "products_codegen"
                                        | "project_publish"
                                        | "universe_set"
                                        | "liveops_datastore_set"
                                        | "liveops_messaging_publish"
                                        | "liveops_remote_config_push"
                                        | "adapter_invoke"
                                        | "evidence_record"
                                        | "unknown_add"
                                        | "unknown_update"
                                ) {
                                    changes_made += 1;
                                }
                                results.push(format!("step[{b_idx}] {note}"));
                            }
                            Err(e) => {
                                problems.push(format!("batch step {b_idx} ({other}) failed: {e}"));
                                transcript.push(format!("batch step {b_idx} ({other}) error: {e}"));
                                results.push(format!("step[{b_idx}] {other} error: {e} (remaining steps in batch aborted)"));
                                batch_failed = true;
                                break;
                            }
                        },
                    }
                }
                let wants_done = step.get("done").and_then(|d| d.as_bool()) == Some(true);
                if batch_failed || !wants_done {
                    messages.push(json!({
                        "role": "user",
                        "content": format!("batch_results:\n{}", results.join("\n"))
                    }));
                    continue;
                }
                // All steps in the batch succeeded AND the model also set `"done": true` — fall through to the done gate!
            }
            if step.get("done").and_then(|d| d.as_bool()) == Some(true) {
                if unproven_changes && !staged && !read_only {
                    if self.effective_project_root().is_some() {
                        match self.run_project_playtest(
                            &json!({ "sync_first": true }),
                            &mut transcript,
                            &mut thread,
                        ) {
                            Ok((true, _note)) => {
                                self.record_verdict(chat_id, &PlaytestOutcome::Passed("ok".to_string()));
                            }
                            Ok((false, console)) => {
                                failed_playtests += 1;
                                last_failed_console = Some(console.clone());
                                self.record_verdict(
                                    chat_id,
                                    &PlaytestOutcome::Failed("failed".to_string(), console.clone()),
                                );
                                if failed_playtests >= MAX_REPAIRS {
                                    return Err(fail(
                                        format!("auto-verify playtest still failing after {failed_playtests} attempts. Console/check output: {console}"),
                                        &thread,
                                        &transcript,
                                        &skills_used,
                                    ));
                                }
                                messages.push(json!({
                                    "role": "user",
                                    "content": format!("Auto-verify Playtest Runner failed before finishing. Fix these errors and re-verify:\n{console}")
                                }));
                                continue;
                            }
                            Err(e) => {
                                transcript.push(format!("auto-verify unavailable: {e}"));
                                messages.push(json!({
                                    "role": "user",
                                    "content": format!("Automatic project playtest failed to run ({e}). Run project_check or project_playtest before finishing.")
                                }));
                                continue;
                            }
                        }
                    } else {
                        transcript.push("done refused: changes are unproven by a playtest".to_string());
                        messages.push(json!({
                            "role": "user",
                            "content": "You changed the game but no playtest has passed since the last change. Run a playtest before finishing — OpenRO only calls work done once it is proven."
                        }));
                        continue;
                    }
                }
                let s = step
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Done.")
                    .to_string();
                // An ask that changed nothing because a step failed is
                // refused in plain words: its own summary is not the
                // result, and no pitch follows it.
                if !read_only && changes_made == 0 && !problems.is_empty() {
                    let message = match &gate_sentence {
                        Some(sentence) => sentence.clone(),
                        None => format!(
                            "I could not build that, so nothing in your game changed. What went wrong: {}.",
                            problems.join("; ")
                        ),
                    };
                    thread.push(format!("OpenRO: {message}"));
                    transcript.push("done refused: nothing changed and a step failed".to_string());
                    return Ok(RunOutcome {
                        summary: message,
                        thread,
                        transcript,
                        skills_used: Vec::new(),
                        refused: true,
                        changed: false,
                    });
                }
                // The place still holds a change an earlier ask made
                // that no passing playtest has covered (or the app's
                // last playtest failed): a success summary would be false.
                let proof = self.proof_by_chat.get(chat_id).cloned().unwrap_or_default();
                if !read_only && !staged && proof.unproven {
                    let why = match &proof.failed_console {
                        Some(console) => format!(
                            "the last test run of your game failed. Console output: {console}"
                        ),
                        None => "changes to your game have not passed a test run yet".to_string(),
                    };
                    transcript.push("done refused: the place is not proven".to_string());
                    let tail = if changes_made > 0 {
                        "Undo last build puts the game back as it was before that build."
                    } else {
                        "Nothing from this try was added to your game."
                    };
                    return Err(fail(
                        format!("I can't call this done: {why}. {tail}"),
                        &thread, &transcript, &skills_used));
                }
                // A vacuous playtest (nothing changed, so nothing to
                // break) does NOT excuse a build claim: when the
                // summary claims building work but the ask changed
                // nothing, the ledger line comes FIRST, then the claim.
                if !read_only && changes_made == 0 && claims_building(&s) {
                    thread.push(
                        "OpenRO: Note — this ask made no changes to the game.".to_string(),
                    );
                }
                thread.push(format!("OpenRO: {s}"));
                transcript.push(format!("done: {s}"));
                summary = Some(s);
                break;
            }

            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                if read_only {
                    let msg = format!(
                        "Ask mode is read-only: skill '{skill}' is disabled. Use read-only inspection tools and reply with {{\"done\": true, \"summary\": \"...\"}}."
                    );
                    transcript.push(format!("skill {skill} refused (ask mode): {msg}"));
                    messages.push(json!({ "role": "user", "content": format!("skill_result: error: {msg}") }));
                    continue;
                }
                let args = step.get("args").cloned().unwrap_or(json!({}));
                // A skill whose prerequisite was undone in this chat and
                // not rebuilt since is refused, not generated.
                if let Some(need) = self.undone_prerequisite(chat_id, skill) {
                    let msg = format!("{skill} needs {need} first, and {need} was undone. Build {need} first, then {skill}");
                    transcript.push(format!("skill {skill} refused: {msg}"));
                    problems.push(msg.clone());
                    gate_sentence = Some(plain_prerequisite(skill, &need));
                    messages.push(json!({
                        "role": "user",
                        "content": format!("skill_result: error: {msg} Choose a different step.")
                    }));
                    continue;
                }
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(note) => {
                        skills_used.push(skill.to_string());
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
                        problems.push(format!("a building step failed: {e}"));
                        transcript.push(format!("skill {skill} error: {e}"));
                        messages.push(json!({
                            "role": "user",
                            "content": format!("skill_result: error: {e}. Choose a different step.")
                        }));
                    }
                }
                continue;
            }

            if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                if read_only && !is_read_only_tool(tool) {
                    let msg = format!(
                        "Ask mode is read-only: tool '{tool}' cannot mutate the place. \
                         Use read-only tools (get_place_tree, search_game_tree, inspect_instance, \
                         script_read, script_grep, get_console_output) and reply with {{\"done\": true, \"summary\": \"...\"}}."
                    );
                    transcript.push(format!("tool {tool} refused (ask mode): {msg}"));
                    messages.push(json!({
                        "role": "user",
                        "content": format!("tool_result: error: {msg}")
                    }));
                    continue;
                }
                if tool == "write_luau" {
                    match self.run_write_luau(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args.get("path").and_then(|v| v.as_str()).map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("the script");
                            problems.push(format!("saving {path} failed: {e}"));
                            transcript.push(format!("tool write_luau error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Fix the error and try again.")
                            }));
                        }
                    }
                    continue;
                }
                if tool == "multi_edit" {
                    match self.run_multi_edit(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args
                                .get("path")
                                .or_else(|| args.get("file_path"))
                                .and_then(|v| v.as_str())
                                .map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args
                                .get("path")
                                .or_else(|| args.get("file_path"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("the script");
                            problems.push(format!("editing {path} failed: {e}"));
                            transcript.push(format!("tool multi_edit error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Fix the error and try again.")
                            }));
                        }
                    }
                    continue;
                }
                if tool == "playtest" {
                    // Nothing changed in this ask: there is nothing to
                    // test, and a run now would be a vacuous pass.
                    if changes_made == 0 {
                        transcript.push("playtest not run: this ask changed nothing".to_string());
                        thread.push("Playtest not run — nothing has changed yet, so there is nothing to test.".to_string());
                        messages.push(json!({
                            "role": "user",
                            "content": "tool_result: playtest not run: this ask has not changed the game, so there is nothing to test. Make the change first, then playtest."
                        }));
                        continue;
                    }
                    let verdict = self.run_playtest(&mut transcript, &mut thread);
                    if let Ok(outcome) = &verdict {
                        self.record_verdict(chat_id, outcome);
                    }
                    match verdict {
                        Ok(PlaytestOutcome::Passed(note)) => {
                            unproven_changes = false;
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Ok(PlaytestOutcome::Failed(note, console)) => {
                            failed_playtests += 1;
                            last_failed_console = Some(console.clone());
                            transcript.push(format!(
                                "repair attempt {failed_playtests}: playtest failed"
                            ));
                            if failed_playtests >= MAX_REPAIRS {
                                return Err(fail(
                                    format!(
                                        "playtest still failing after {failed_playtests} attempts — \
                                         I could not get this build to pass. Last console output: {console}"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Ok(PlaytestOutcome::Unreadable(note, raw)) => {
                            failed_playtests += 1;
                            transcript.push(format!(
                                "repair attempt {failed_playtests}: playtest verdict unreadable: {raw}"
                            ));
                            if failed_playtests >= MAX_REPAIRS {
                                return Err(fail(
                                    format!(
                                        "playtest verdict could not be read after {failed_playtests} attempts — \
                                         the bridge must return {{\"status\": \"passed\"}} or {{\"status\": \"failed\"}}. \
                                         Last playtest result: {raw}"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
                    }
                    continue;
                }
                if tool == "write_script" {
                    // Local models frequently combine skill generation and script
                    // writing into one `write_script` call (passing `skill` + optional
                    // skill parameters without raw `source`). Compile the requested
                    // Rune skill on demand — still strictly refusing raw `source` and
                    // enforcing undone-prerequisite gates identically to a separate
                    // skill step.
                    if args.get("source").is_none() {
                        if let Some(skill_name) = args.get("skill").and_then(|v| v.as_str()) {
                            let inline_args = extract_inline_skill_args(&args);
                            let needs_compile = !self.generated.contains_key(skill_name)
                                || inline_args.is_some();
                            if needs_compile {
                                if let Some(need) = self.undone_prerequisite(chat_id, skill_name) {
                                    let msg = format!(
                                        "{skill_name} needs {need} first, and {need} was undone. Build {need} first, then {skill_name}"
                                    );
                                    transcript.push(format!("skill {skill_name} refused: {msg}"));
                                    problems.push(msg.clone());
                                    gate_sentence = Some(plain_prerequisite(skill_name, &need));
                                    messages.push(json!({
                                        "role": "user",
                                        "content": format!("skill_result: error: {msg} Choose a different step.")
                                    }));
                                    continue;
                                }
                                let skill_args = inline_args.unwrap_or_else(|| json!({}));
                                match self.run_skill(
                                    skill_name,
                                    &skill_args,
                                    &mut transcript,
                                    &mut thread,
                                ) {
                                    Ok(_) => skills_used.push(skill_name.to_string()),
                                    Err(e) => {
                                        problems.push(format!("a building step failed: {e}"));
                                        transcript.push(format!("skill {skill_name} error: {e}"));
                                        messages.push(json!({
                                            "role": "user",
                                            "content": format!("skill_result: error: {e}. Choose a different step.")
                                        }));
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    match self.run_write_script(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args.get("path").and_then(|v| v.as_str()).map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("the script");
                            problems.push(format!("saving {path} failed: {e}"));
                            transcript.push(format!("tool write_script error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Choose a different step.")
                            }));
                        }
                    }
                    continue;
                }
                if tool == "project_playtest" {
                    match self.run_project_playtest(&args, &mut transcript, &mut thread) {
                        Ok((true, note)) => {
                            unproven_changes = false;
                            self.record_verdict(chat_id, &PlaytestOutcome::Passed(note.clone()));
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Ok((false, console)) => {
                            failed_playtests += 1;
                            last_failed_console = Some(console.clone());
                            self.record_verdict(
                                chat_id,
                                &PlaytestOutcome::Failed("project_playtest failed".to_string(), console.clone()),
                            );
                            transcript.push(format!("repair attempt {failed_playtests}: project_playtest failed"));
                            if failed_playtests >= MAX_REPAIRS {
                                return Err(fail(
                                    format!("project_playtest still failing after {failed_playtests} attempts. Console/check output: {console}"),
                                    &thread,
                                    &transcript,
                                    &skills_used,
                                ));
                            }
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: project_playtest FAILED. Inspect the errors below, repair the scripts/files, and re-run project_playtest:\n{console}")
                            }));
                        }
                        Err(e) => {
                            problems.push(format!("project playtest failed: {e}"));
                            transcript.push(format!("tool project_playtest error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}")
                            }));
                        }
                    }
                    continue;
                }
                match self.run_tool(tool, &args, &mut transcript, &mut thread) {
                    Ok(note) => {
                        if matches!(
                            tool,
                            "delete_script"
                                | "create_instance"
                                | "delete_instance"
                                | "project_write_file"
                                | "project_edit_file"
                                | "project_delete_file"
                                | "project_update_manifest"
                                | "project_scene_mutate"
                                | "gui_save"
                                | "gui_compile"
                        ) {
                            changes_made += 1;
                            let path_arg = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                            let needs_playtest = !matches!(tool, "project_update_manifest" | "gui_save")
                                && (matches!(tool, "delete_script" | "create_instance" | "delete_instance" | "project_scene_mutate" | "gui_compile")
                                    || path_arg.ends_with(".luau")
                                    || path_arg.ends_with(".lua")
                                    || path_arg == "default.project.json");
                            if needs_playtest {
                                unproven_changes = true;
                                self.note_change(chat_id);
                            }
                        } else if matches!(
                            tool,
                            "art_generate"
                                | "image_process"
                                | "art_pack_card"
                                | "mesh_generate"
                                | "blender_generate"
                                | "audio_generate"
                                | "assets_upload"
                                | "creator_hub_setup"
                                | "creator_hub_configure"
                                | "assets_authorise"
                                | "assets_codegen"
                                | "products_sync"
                                | "products_codegen"
                                | "project_publish"
                                | "universe_set"
                                | "liveops_datastore_set"
                                | "liveops_messaging_publish"
                                | "liveops_remote_config_push"
                                | "adapter_invoke"
                                | "evidence_record"
                                | "unknown_add"
                                | "unknown_update"
                        ) {
                            changes_made += 1;
                        }
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
                        problems.push(format!("a game step failed: {e}"));
                        transcript.push(format!("tool error: {e}"));
                        messages.push(json!({
                            "role": "user",
                            "content": format!("tool_result: error: {e}. Choose a different step.")
                        }));
                    }
                }
                continue;
            }

            messages.push(json!({
                "role": "user",
                "content": "That step was not recognised. Reply with one JSON step: a skill, a tool, or done."
            }));
        }

        match summary {
            Some(summary) => Ok(RunOutcome {
                summary,
                thread,
                transcript,
                skills_used,
                refused: false,
                changed: changes_made > 0,
            }),
            None => {
                let mut message = format!("agent loop did not finish within {MAX_STEPS} steps");
                if let Some(path) = &last_written {
                    let proof = self.proof_by_chat.get(chat_id).cloned().unwrap_or_default();
                    let status = if proof.failed_console.is_some() {
                        "its last test run failed"
                    } else {
                        "no test run has passed it"
                    };
                    message.push_str(&format!(
                        ". The game was left with the script {path}, and {status}. Undo last build undoes it."
                    ));
                }
                Err(fail(message, &thread, &transcript, &skills_used))
            }
        }
    }

    fn run_skill(
        &mut self,
        skill: &str,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<String, String> {
        self.emit_event(
            "compiling_rune",
            "Generating Luau",
            &format!("Generating Luau for '{skill}'"),
            Some(skill),
            None,
            35,
        );
        // The Luau is produced by executing the skill's Rune script.
        let source = skills::generate(skill, args)?;
        let args_note = args
            .as_object()
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        transcript.push(format!(
            "skill {skill} (Rune) {args_note} generated {} bytes of Luau",
            source.len()
        ));
        thread.push("Generated the script for that part.".to_string());
        self.generated.insert(skill.to_string(), source.clone());
        self.last_skill = Some(skill.to_string());
        Ok(format!(
            "skill_result: the {skill} Rune skill generated this Luau source:\n{source}\n\
             Next, write it with {{\"tool\":\"write_script\",\"args\":{{\"path\":\"<where it belongs>\",\"skill\":\"{skill}\"}}}} and then playtest."
        ))
    }

    fn run_write_script(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("write_script needs args.path")?
            .to_string();
        check_path(&path)?;
        let source = self.resolve_source(args)?;
        // Checkpoint AFTER the bridge call succeeds: recording the
        // previous state before the attempt used to poison the change
        // set with changes that never happened when the call failed.
        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": source.clone() }))?;
        self.record_script(&path, prev, Some(source.clone()));
        self.emit_event(
            "writing",
            "Writing to Place",
            &format!("Saved {path} ({} bytes)", source.len()),
            args.get("skill").and_then(|v| v.as_str()),
            Some(&path),
            60,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Wrote {path} ({} bytes)", source.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&source));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!("tool write_script {path} ({} bytes)", source.len()));
        thread.push(format!("Wrote {path}."));
        let mut note = format!(
            "tool_result: write_script {path} succeeded. Write any remaining scripts/instances requested, or call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        // A byte-identical rewrite of a version that just failed cannot
        // fix anything — tell the model so, with the console text.
        if let (Some(prev), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev == source {
                transcript.push(format!(
                    "note: write_script {path} was byte-identical to the failing version"
                ));
                note = format!(
                    "tool_result: write_script {path} succeeded — but this source is byte-identical \
                     to the version that just failed the playtest with: {console}\n\
                     Rewriting the same source cannot change the result. Change something material \
                     (create what it references, or generate it differently), then playtest again."
                );
            }
        }
        writes.insert(path, source);
        Ok(note)
    }

    /// RoPilot custom Luau writer (`write_luau`): validates `path`, caps size at `512 KiB`,
    /// runs `luau_check::analyze_source` (rejecting with exact line-numbered errors before
    /// writing if any `Severity::Error` is present), checkpoints previous state onto the
    /// per-chat Undo stack, writes to the active backend, emits `ScriptWritten` + auto-capture.
    fn run_write_luau(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("write_luau needs args.path")?
            .to_string();
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or("write_luau needs args.source")?
            .to_string();
        check_path(&path)?;
        if source.len() > MAX_LUAU_BYTES {
            return Err(format!(
                "write_luau source exceeds maximum allowed size ({MAX_LUAU_BYTES} bytes)"
            ));
        }
        if source.contains('\0') {
            return Err("write_luau source must not contain NUL bytes".to_string());
        }
        let report = crate::luau_check::analyze_source(&path, &source);
        if report.error_count() > 0 {
            let formatted = format_luau_errors(&report);
            return Err(format!(
                "Static Luau Prover rejected {path} ({} error(s)):\n{formatted}",
                report.error_count()
            ));
        }
        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": source.clone() }))?;
        self.record_script(&path, prev, Some(source.clone()));
        self.emit_event(
            "writing",
            "Writing Custom Luau",
            &format!("Saved {path} ({} bytes)", source.len()),
            None,
            Some(&path),
            60,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Wrote custom Luau {path} ({} bytes)", source.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else if path.starts_with("ReplicatedStorage.") {
            "ModuleScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&source));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!("tool write_luau {path} ({} bytes)", source.len()));
        thread.push(format!("Wrote {path}."));
        let warn_suffix = if report.warning_count() > 0 {
            format!(
                " ({} static warning(s): {})",
                report.warning_count(),
                report.warnings().join("; ")
            )
        } else {
            String::new()
        };
        let mut note = format!(
            "tool_result: write_luau {path} succeeded{warn_suffix}. Write any remaining scripts/instances, or call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        if let (Some(prev_written), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev_written == source {
                transcript.push(format!(
                    "note: write_luau {path} was byte-identical to the failing version"
                ));
                note = format!(
                    "tool_result: write_luau {path} succeeded — but this source is byte-identical \
                     to the version that just failed the playtest with: {console}\n\
                     Change something material before playtesting again."
                );
            }
        }
        writes.insert(path, source);
        Ok(note)
    }

    /// RoPilot targeted multi-hunk Luau editor (`multi_edit`): validates `path`, applies
    /// ordered `old_str -> new_str` replacements, caps size at `512 KiB`, runs
    /// `luau_check::analyze_source` (rejecting on any `Severity::Error` before writing),
    /// checkpoints previous state onto per-chat Undo, writes & emits `ScriptWritten`.
    fn run_multi_edit(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("multi_edit needs args.path")?
            .to_string();
        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or("multi_edit needs non-empty args.edits array")?;
        check_path(&path)?;

        let current_opt = self.prev_script_current(&path)?;
        let mut updated = match current_opt {
            Some(s) => s,
            None => {
                let first_empty = edits
                    .first()
                    .and_then(|e| {
                        e.get("old_str")
                            .or_else(|| e.get("old_text"))
                            .or_else(|| e.get("find"))
                            .and_then(|v| v.as_str())
                    })
                    .is_some_and(|s| s.is_empty());
                if first_empty {
                    String::new()
                } else {
                    return Err(format!(
                        "no script at {path} — use write_luau to create it first"
                    ));
                }
            }
        };

        let mut applied = 0usize;
        for (idx, ed) in edits.iter().enumerate() {
            let old_s = ed
                .get("old_str")
                .or_else(|| ed.get("old_text"))
                .or_else(|| ed.get("find"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("multi_edit edit #{} needs string 'old_str'", idx + 1))?;
            let new_s = ed
                .get("new_str")
                .or_else(|| ed.get("new_text"))
                .or_else(|| ed.get("replace"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("multi_edit edit #{} needs string 'new_str'", idx + 1))?;
            if old_s.is_empty() {
                if !updated.is_empty() && !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push_str(new_s);
            } else {
                if !updated.contains(old_s) {
                    return Err(format!(
                        "multi_edit edit #{} target not found in {path} (read the script with script_read first)",
                        idx + 1
                    ));
                }
                updated = updated.replacen(old_s, new_s, 1);
            }
            applied += 1;
        }

        if updated.len() > MAX_LUAU_BYTES {
            return Err(format!(
                "multi_edit result exceeds maximum allowed size ({MAX_LUAU_BYTES} bytes)"
            ));
        }
        if updated.contains('\0') {
            return Err("multi_edit result must not contain NUL bytes".to_string());
        }
        let report = crate::luau_check::analyze_source(&path, &updated);
        if report.error_count() > 0 {
            let formatted = format_luau_errors(&report);
            return Err(format!(
                "Static Luau Prover rejected multi_edit on {path} ({} error(s)):\n{formatted}",
                report.error_count()
            ));
        }

        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": updated.clone() }))?;
        self.record_script(&path, prev, Some(updated.clone()));
        self.emit_event(
            "writing",
            "Editing Luau",
            &format!("Applied {applied} edit(s) to {path} ({} bytes)", updated.len()),
            None,
            Some(&path),
            62,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Edited {path} ({applied} hunk(s), {} bytes)", updated.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else if path.starts_with("ReplicatedStorage.") {
            "ModuleScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&updated));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!(
            "tool multi_edit {path} ({applied} edit(s), {} bytes)",
            updated.len()
        ));
        thread.push(format!("Edited {path} ({applied} change(s))."));
        let mut note = format!(
            "tool_result: multi_edit {path} applied {applied} edit(s). Call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        if let (Some(prev_written), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev_written == updated {
                note = format!(
                    "tool_result: multi_edit {path} resulted in byte-identical source to the failing version ({console}). Make a material fix before playtesting again."
                );
            }
        }
        writes.insert(path, updated);
        Ok(note)
    }

    /// Resolve the source for a write_script step: a named skill's
    /// generated source, or the most recent generation. There is no
    /// hand-written-source path — Luau comes only from Rune skills.
    fn resolve_source(&self, args: &Value) -> Result<String, String> {
        if args.get("source").is_some() {
            return Err(
                "write_script does not take a hand-written \"source\" — Luau comes only from \
                 Rune skills. Run a skill first, then write its generated source."
                    .to_string(),
            );
        }
        if let Some(name) = args.get("skill").and_then(|v| v.as_str()) {
            return self
                .generated
                .get(name)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "no generated source for that part yet — run its step first: {{\"skill\": \"{name}\", \"args\": {{}}}}"
                    )
                });
        }
        match args.get("use_generated") {
            Some(Value::String(name)) => self
                .generated
                .get(name)
                .cloned()
                .ok_or_else(|| "no generated source for that part yet — run its step first".to_string()),
            Some(Value::Bool(true)) => {
                let name = self
                    .last_skill
                    .as_ref()
                    .ok_or("write_script needs args.skill or a prior skill result")?;
                self.generated
                    .get(name)
                    .cloned()
                    .ok_or_else(|| "generated source missing".to_string())
            }
            _ => Err("write_script needs args.skill (Luau comes only from Rune skills) or a prior skill result".to_string()),
        }
    }

    fn run_playtest(
        &mut self,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<PlaytestOutcome, String> {
        let check_detail = match self.studio.as_ref() {
            Some(PlaceBackend::Local(local)) => {
                let rep = local.analyze();
                format!(
                    "Checked {} script(s): {} error(s), {} warning(s)",
                    local.scripts().len(),
                    rep.error_count(),
                    rep.warning_count()
                )
            }
            _ => "Checking Luau syntax, service references, and script wiring".to_string(),
        };
        self.emit_event(
            "static_luau_check",
            "Static Luau Check",
            &check_detail,
            None,
            None,
            78,
        );
        self.emit_event(
            "playtest_proof",
            "Playtest Proof",
            "Running Studio playtest verification",
            None,
            None,
            88,
        );
        let mut pt_start = StudioActionInput::new(
            StudioActionKind::PlaytestStarted,
            "Running Studio playtest verification",
        );
        pt_start.path = "Workspace".to_string();
        pt_start.class_name = "RunService".to_string();
        self.emit_studio_action(pt_start);
        let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
        transcript.push(format!("tool playtest -> {text}"));
        match playtest_verdict(&text) {
            Verdict::Failed => {
                self.emit_event(
                    "auto_repair",
                    "Auto-Repair",
                    "Playtest reported an error — reading console to repair",
                    None,
                    None,
                    70,
                );
                thread.push("Playtest failed — reading the console and repairing.".to_string());
                // Feed the ACTUAL console output back to the model. A
                // console-fetch failure must not abort the repair loop
                // (it once turned one failed playtest into a dead ask):
                // fall back to the console embedded in the playtest
                // result itself, or say plainly it is unavailable.
                let console = match self.studio_mut()?.call_tool("get_console", json!({})) {
                    Ok(c) => c,
                    Err(e) => serde_json::from_str::<Value>(&text)
                        .ok()
                        .and_then(|v| {
                            v.get("console")
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| format!("(could not read the console: {e})")),
                };
                let fseq = self
                    .take_playtest_frame_seq()
                    .or_else(|| self.auto_capture_mcp_frame());
                let mut pt_fail = StudioActionInput::new(
                    StudioActionKind::PlaytestFailed,
                    "Playtest failed — reading console output",
                );
                pt_fail.path = "Output".to_string();
                pt_fail.class_name = "RunService".to_string();
                pt_fail.console_line = Some(console.chars().take(240).collect());
                pt_fail.frame_seq = fseq;
                self.emit_studio_action(pt_fail);
                transcript.push(format!("console after failed playtest: {console}"));
                thread.push(format!("Console from the failed test run: {console}"));
                Ok(PlaytestOutcome::Failed(
                    format!(
                        "tool_result: playtest FAILED. Actual console output: {console}\n\
                         Repair the game from that console output (for example regenerate the \
                         script so it creates what it references, or create the missing instance), \
                         apply the fix, and playtest again."
                    ),
                    console,
                ))
            }
            Verdict::Passed => {
                self.emit_event(
                    "playtest_proof",
                    "Playtest Passed",
                    "Playtest ran cleanly with zero errors",
                    None,
                    None,
                    95,
                );
                let fseq = self
                    .take_playtest_frame_seq()
                    .or_else(|| self.auto_capture_mcp_frame());
                let mut pt_pass = StudioActionInput::new(
                    StudioActionKind::PlaytestPassed,
                    "Playtest passed with 0 Luau errors",
                );
                pt_pass.path = "Workspace".to_string();
                pt_pass.class_name = "RunService".to_string();
                pt_pass.frame_seq = fseq;
                self.emit_studio_action(pt_pass);
                thread.push("Playtest passed.".to_string());
                Ok(PlaytestOutcome::Passed(format!(
                    "tool_result: playtest PASSED ({text}). You may finish with done."
                )))
            }
            Verdict::Unreadable => {
                thread.push(
                    "Playtest result was not in the documented format — treating the build as unproven."
                        .to_string(),
                );
                Ok(PlaytestOutcome::Unreadable(
                    format!(
                        "tool_result: the playtest result could not be read as a verdict: {text}\n\
                         The bridge must return {{\"status\": \"passed\"}} or {{\"status\": \"failed\"}}. \
                         The build counts as UNPROVEN, not passed. Playtest again."
                    ),
                    text,
                ))
            }
        }
    }

    fn run_tool(
        &mut self,
        tool: &str,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<String, String> {
        match tool {
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("delete_script needs args.path")?
                    .to_string();
                check_path(&path)?;
                let prev = self.prev_script(&path)?;
                self.studio_mut()?
                    .call_tool("delete_script", json!({ "path": path }))?;
                self.record_script(&path, prev, None);
                transcript.push(format!("tool delete_script {path}"));
                Ok(format!("tool_result: delete_script {path} succeeded."))
            }
            "create_instance" => {
                let class_name = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.class_name")?
                    .to_string();
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.name")?
                    .to_string();
                let parent = args
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.parent")?
                    .to_string();
                if name.is_empty()
                    || name.chars().any(|c| c == '.' || c == '/' || c == '\\' || c.is_control())
                {
                    return Err(format!(
                        "create_instance name must be a single non-empty name without dots, separators or control characters, got: {name}"
                    ));
                }
                if class_name.is_empty()
                    || class_name
                        .chars()
                        .any(|c| c == '.' || c == '/' || c == '\\' || c.is_control())
                {
                    return Err(format!(
                        "create_instance class_name must be a single class name without separators or control characters, got: {class_name}"
                    ));
                }
                check_path(&parent)?;
                let path = format!("{parent}.{name}");
                check_path(&path)?;
                let prev = self.prev_instance(&path)?;
                self.studio_mut()?
                    .call_tool("create_instance", args.clone())?;
                // The state this ask leaves is the record the BRIDGE
                // holds, read back from the tree — never a record
                // fabricated from the ask's args. A real place adds
                // its own default properties to every instance; an
                // args-shaped post would make every legitimate
                // instance revert refuse forever against it.
                let post = match self.prev_instance_current(&path) {
                    Ok(Some(record)) => Some(record),
                    _ => Some(json!({
                        "class_name": class_name,
                        "properties": args.get("properties").cloned().unwrap_or(json!({})),
                    })),
                };
                let was_existing = prev.is_some();
                let post_props = post.as_ref().and_then(|r| r.get("properties")).cloned();
                self.record_instance(&path, prev, post);
                let mut ev = StudioActionInput::new(
                    if was_existing {
                        StudioActionKind::PartUpdated
                    } else {
                        StudioActionKind::PartCreated
                    },
                    format!("Placed {path} ({class_name})"),
                );
                ev.path = path.clone();
                ev.class_name = class_name.clone();
                ev.position = extract_vec3_prop(post_props.as_ref(), "Position");
                ev.size = extract_vec3_prop(post_props.as_ref(), "Size");
                ev.color = extract_color_prop(post_props.as_ref());
                self.emit_studio_action(ev);
                let _ = self.auto_capture_mcp_frame();
                transcript.push(format!("tool create_instance {path} ({class_name})"));
                thread.push(format!("Created {path} ({class_name})."));
                Ok(format!("tool_result: create_instance {path} succeeded."))
            }
            "delete_instance" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("delete_instance needs args.path")?
                    .to_string();
                check_path(&path)?;
                let prev = self.prev_instance(&path)?;
                self.studio_mut()?
                    .call_tool("delete_instance", json!({ "path": path }))?;
                self.record_instance(&path, prev, None);
                let mut ev = StudioActionInput::new(
                    StudioActionKind::PartUpdated,
                    format!("Deleted {path}"),
                );
                ev.path = path.clone();
                ev.class_name = "Instance".to_string();
                self.emit_studio_action(ev);
                let _ = self.auto_capture_mcp_frame();
                transcript.push(format!("tool delete_instance {path}"));
                thread.push(format!("Deleted {path}."));
                Ok(format!("tool_result: delete_instance {path} succeeded."))
            }
            "get_console" | "get_place_tree" => {
                let text = self.studio_mut()?.call_tool(tool, json!({}))?;
                transcript.push(format!("tool {tool} -> {text}"));
                Ok(format!("tool_result: {tool}: {text}"))
            }
            "project_list" => {
                let res = if let Some(hook) = &self.hooks.project_list {
                    hook(args)?
                } else if let Some(root) = self.effective_project_root() {
                    json!({
                        "ok": true,
                        "active": self.project_id,
                        "projects": [{
                            "id": self.project_id.clone().unwrap_or_else(|| "active".to_string()),
                            "path": root.to_string_lossy(),
                        }]
                    })
                } else {
                    json!({ "ok": true, "active": null, "projects": [] })
                };
                let text = res.to_string();
                transcript.push(format!("tool project_list -> {text}"));
                Ok(format!("tool_result: project_list: {text}"))
            }
            "project_files" => {
                let root = self.require_project_root()?;
                let dir = args
                    .get("dir")
                    .or_else(|| args.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let res = project_files_list(&root, dir)?;
                let text = res.to_string();
                transcript.push(format!("tool project_files({dir}) -> {} entries", res["entries"].as_array().map(|a| a.len()).unwrap_or(0)));
                Ok(format!("tool_result: project_files: {text}"))
            }
            "project_read_file" => {
                let root = self.require_project_root()?;
                let rel = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("project_read_file needs args.path")?;
                let start = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let end = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let res = project_read_file(&root, rel, start, end)?;
                let text = res.to_string();
                transcript.push(format!("tool project_read_file {rel} ({} bytes)", res["bytes"]));
                Ok(format!("tool_result: project_read_file: {text}"))
            }
            "project_write_file" => {
                let root = self.require_project_root()?;
                let rel = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("project_write_file needs args.path")?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("project_write_file needs args.content")?;
                let expect = args.get("expect_sha256").and_then(|v| v.as_str());
                let res = project_write_file(&root, rel, content, expect)?;
                let text = res.to_string();
                transcript.push(format!("tool project_write_file {rel} ({} bytes)", res["bytes"]));
                thread.push(format!("Wrote project file {rel}."));
                Ok(format!("tool_result: project_write_file: {text}"))
            }
            "project_edit_file" => {
                let root = self.require_project_root()?;
                let rel = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("project_edit_file needs args.path")?;
                let edits = args
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .ok_or("project_edit_file needs args.edits array")?;
                let expect = args.get("expect_sha256").and_then(|v| v.as_str());
                let res = project_edit_file(&root, rel, edits, expect)?;
                let text = res.to_string();
                transcript.push(format!("tool project_edit_file {rel} ({} edits)", res["edits"]));
                thread.push(format!("Edited project file {rel}."));
                Ok(format!("tool_result: project_edit_file: {text}"))
            }
            "project_delete_file" => {
                let root = self.require_project_root()?;
                let rel = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("project_delete_file needs args.path")?;
                let res = project_delete_file(&root, rel)?;
                let text = res.to_string();
                transcript.push(format!("tool project_delete_file {rel}"));
                thread.push(format!("Deleted project file {rel}."));
                Ok(format!("tool_result: project_delete_file: {text}"))
            }
            "project_grep" => {
                let root = self.require_project_root()?;
                let query = args
                    .get("query")
                    .or_else(|| args.get("pattern"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or("project_grep needs args.query (or args.pattern)")?;
                let dir = args
                    .get("dir")
                    .or_else(|| args.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let max_m = args.get("max_matches").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let res = project_grep_files(&root, query, dir, max_m)?;
                let text = res.to_string();
                transcript.push(format!("tool project_grep({query}) -> {} matches", res["count"]));
                Ok(format!("tool_result: project_grep: {text}"))
            }
            "project_update_manifest" => {
                let root = self.require_project_root()?;
                let patch = args.get("patch").unwrap_or(args);
                let res = project_update_manifest_safe(&root, patch)?;
                let text = res.to_string();
                transcript.push("tool project_update_manifest succeeded".to_string());
                thread.push("Updated openro.project.json manifest (prices remain unapproved until owner approval).".to_string());
                Ok(format!("tool_result: project_update_manifest: {text}"))
            }
            "project_check" => {
                let res = if let Some(hook) = &self.hooks.project_check {
                    hook(args)?
                } else {
                    let root = self.require_project_root()?;
                    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
                    crate::projects::check_project_with_full_options(&root, false, None, force)?
                };
                let text = res.to_string();
                transcript.push(format!("tool project_check -> passed={}", res["passed"]));
                Ok(format!("tool_result: project_check: {text}"))
            }
            "evidence_record" => {
                let root = self.require_project_root()?;
                let res = crate::study_check::record_evidence(&root, args)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                let eid = res
                    .pointer("/evidence/id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ev");
                transcript.push(format!("tool evidence_record -> {eid}"));
                Ok(format!("tool_result: evidence_record: {text}"))
            }
            "evidence_list" => {
                let root = self.require_project_root()?;
                let filter = crate::study_check::EvidenceFilter {
                    confidence: args
                        .get("confidence")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    kind: args
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    source: args
                        .get("source")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    limit: args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize),
                };
                let res = crate::study_check::list_evidence(&root, &filter)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                transcript.push(format!("tool evidence_list -> matched={}", res["matched"]));
                Ok(format!("tool_result: evidence_list: {text}"))
            }
            "unknown_add" => {
                let root = self.require_project_root()?;
                let res = crate::study_check::add_unknown(&root, args)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                let uid = res
                    .pointer("/unknown/id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unk");
                transcript.push(format!("tool unknown_add -> {uid}"));
                Ok(format!("tool_result: unknown_add: {text}"))
            }
            "unknown_update" => {
                let root = self.require_project_root()?;
                let id = args
                    .get("id")
                    .or_else(|| args.get("unknown_id"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or("unknown_update needs args.id")?;
                let res = crate::study_check::update_unknown(&root, id, args)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                let st = res
                    .pointer("/unknown/status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("updated");
                transcript.push(format!("tool unknown_update({id}) -> {st}"));
                Ok(format!("tool_result: unknown_update: {text}"))
            }
            "unknowns_list" => {
                let root = self.require_project_root()?;
                let status = args
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let severity = args
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let domain = args
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let res = crate::study_check::list_unknowns(&root, status, severity, domain)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                transcript.push(format!("tool unknowns_list -> open={}", res["open_count"]));
                Ok(format!("tool_result: unknowns_list: {text}"))
            }
            "mechanics_check" => {
                let root = self.require_project_root()?;
                let rel_path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let strict = args.get("strict").and_then(|v| v.as_bool()).unwrap_or(false);
                let res = crate::study_check::check_mechanics(&root, rel_path, strict)
                    .map_err(|e| e.message)?;
                let text = res.to_string();
                transcript.push(format!("tool mechanics_check -> passed={}", res["passed"]));
                Ok(format!("tool_result: mechanics_check: {text}"))
            }
            "study_status" => {
                let root = self.require_project_root()?;
                let res = crate::study_check::study_status(&root).map_err(|e| e.message)?;
                let text = res.to_string();
                transcript.push(format!("tool study_status -> stage={}", res["stage"]));
                Ok(format!("tool_result: study_status: {text}"))
            }
            "project_run_lune" => {
                let res = if let Some(hook) = &self.hooks.project_run_lune {
                    hook(args)?
                } else {
                    let root = self.require_project_root()?;
                    project_run_lune_local(&root, args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool project_run_lune -> exit={}", res["exit_code"]));
                Ok(format!("tool_result: project_run_lune: {text}"))
            }
            "sandbox_playtest" => {
                let res = if let Some(hook) = &self.hooks.sandbox_playtest {
                    hook(args)?
                } else if let Some(root) = self.effective_project_root() {
                    crate::sandbox::run_project_sandbox(&root, args)?
                } else if let Some(PlaceBackend::Local(local)) = &self.studio {
                    crate::sandbox::run_local_place_sandbox(
                        local.game_name(),
                        local.scripts(),
                        local.instances(),
                        args,
                    )?
                } else {
                    return Err("sandbox_playtest requires an active project or LocalPlace".to_string());
                };
                let passed = res.get("passed").and_then(Value::as_bool).unwrap_or(false);
                let err_count = res
                    .pointer("/console/errors")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                transcript.push(format!(
                    "tool sandbox_playtest -> passed={passed} errors={err_count}"
                ));
                Ok(format!("tool_result: sandbox_playtest: {res}"))
            }
            "project_build" => {
                let res = if let Some(hook) = &self.hooks.project_build {
                    hook(args)?
                } else {
                    let root = self.require_project_root()?;
                    project_build_local(&root, args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool project_build -> {text}"));
                thread.push("Built project .rbxlx.".to_string());
                Ok(format!("tool_result: project_build: {text}"))
            }
            "project_scene_get" => {
                let root = self.require_project_root()?;
                let pid = self.project_id.as_deref().unwrap_or("project");
                let res = crate::projects::load_workspace_bundle(&root, pid)?;
                let text = res.to_string();
                let inst_count = res["instances"].as_object().map(|m| m.len()).unwrap_or(0);
                transcript.push(format!("tool project_scene_get -> {inst_count} instances"));
                Ok(format!("tool_result: project_scene_get: {text}"))
            }
            "project_scene_mutate" => {
                let root = self.require_project_root()?;
                let pid = self.project_id.as_deref().unwrap_or("project");
                let res = crate::scene::mutate_scene(&root, pid, args)?;
                let text = res.to_string();
                transcript.push(format!("tool project_scene_mutate -> rev={}", res["scene_revision"]));
                thread.push(format!("Updated 3D scene (revision {}).", res["scene_revision"]));
                Ok(format!("tool_result: project_scene_mutate: {text}"))
            }
            "gui_save" => {
                let root = self
                    .effective_project_root()
                    .unwrap_or_else(|| self.effective_assets_root());
                let raw_slug = args
                    .get("slug")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        args.pointer("/doc/name")
                            .or_else(|| args.get("name"))
                            .and_then(|v| v.as_str())
                            .map(crate::projects::slug)
                    })
                    .or_else(|| {
                        args.get("preset")
                            .and_then(|v| v.as_str())
                            .map(crate::projects::slug)
                    })
                    .ok_or("gui_save needs args.slug (plus args.doc or args.preset)")?;
                let res = crate::gui_designer::save_project_screen(&root, &raw_slug, args)?;
                let text = res.to_string();
                let warn_count = res["safe_area_warnings"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                transcript.push(format!("tool gui_save -> {} warnings={warn_count}", res["path"]));
                thread.push(format!("Saved ScreenGui {} ({warn_count} safe-area warning(s)).", res["slug"]));
                Ok(format!("tool_result: gui_save: {text}"))
            }
            "gui_compile" => {
                let root = self
                    .effective_project_root()
                    .unwrap_or_else(|| self.effective_assets_root());
                let raw_slug = args
                    .get("slug")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        args.pointer("/doc/name")
                            .or_else(|| args.get("name"))
                            .and_then(|v| v.as_str())
                            .map(crate::projects::slug)
                    })
                    .or_else(|| {
                        args.get("preset")
                            .and_then(|v| v.as_str())
                            .map(crate::projects::slug)
                    })
                    .ok_or("gui_compile needs args.slug")?;
                let res = crate::gui_designer::compile_project_screen(&root, &raw_slug, args)?;
                let text = res.to_string();
                transcript.push(format!("tool gui_compile -> {}", res["luau_path"]));
                thread.push(format!("Compiled ScreenGui to {}.", res["luau_path"]));
                Ok(format!("tool_result: gui_compile: {text}"))
            }
            "project_sync" => {
                let res = if let Some(hook) = &self.hooks.project_sync {
                    hook(args)?
                } else {
                    let root = self.require_project_root()?;
                    self.project_sync_local(&root, args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool project_sync -> {text}"));
                thread.push("Synced project files to Studio.".to_string());
                Ok(format!("tool_result: project_sync: {text}"))
            }
            "research_roblox_game" => {
                let target = args
                    .get("target")
                    .or_else(|| args.get("url"))
                    .or_else(|| args.get("place_id"))
                    .or_else(|| args.get("universe_id"))
                    .and_then(|v| v.as_str())
                    .ok_or("research_roblox_game needs args.target (Roblox game URL, place ID, or universe ID)")?;
                let niche = args.get("niche").and_then(|v| v.as_str());
                let res = if let Some(proj) = self.effective_project_root() {
                    crate::market_taste::research_roblox_game_to_project(&proj, target, niche)?
                } else {
                    let root = self.effective_assets_root();
                    crate::market_taste::research_roblox_game(&root, target, niche)?
                };
                let name = res.get("name").and_then(|v| v.as_str()).unwrap_or(target);
                let text = res.to_string();
                transcript.push(format!("tool research_roblox_game({target}) -> {name}"));
                thread.push(format!("Researched Roblox game {name}."));
                Ok(format!("tool_result: research_roblox_game: {text}"))
            }
            "web_fetch" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("web_fetch needs args.url")?;
                let max_chars = args.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(12_000) as usize;
                let res = crate::market_taste::web_fetch(url, max_chars)?;
                let text = res.to_string();
                transcript.push(format!("tool web_fetch({url}) -> {} bytes", res["bytes"]));
                Ok(format!("tool_result: web_fetch: {text}"))
            }
            "web_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("web_search needs args.query")?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(6) as usize;
                let res = crate::market_taste::web_search(query, limit)?;
                let text = res.to_string();
                transcript.push(format!("tool web_search({query}) -> {} results", res["count"]));
                Ok(format!("tool_result: web_search: {text}"))
            }
            "reference_search" => {
                let hook = self.hooks.reference_search.as_ref().ok_or(
                    "reference_search tool is not connected on this server instance yet (needs openro-assets hook).",
                )?;
                let res = hook(args)?;
                let text = res.to_string();
                transcript.push(format!("tool reference_search -> {text}"));
                Ok(format!("tool_result: reference_search: {text}"))
            }
            "art_generate" => {
                let hook = self.hooks.art_generate.as_ref().ok_or(
                    "art_generate tool is not connected on this server instance yet (needs openro-assets hook).",
                )?;
                let res = hook(args)?;
                let text = res.to_string();
                transcript.push(format!("tool art_generate -> {text}"));
                thread.push("Generated artwork.".to_string());
                Ok(format!("tool_result: art_generate: {text}"))
            }
            "image_process" => {
                let hook = self.hooks.image_process.as_ref().ok_or(
                    "image_process tool is not connected on this server instance yet (needs openro-assets hook).",
                )?;
                let res = hook(args)?;
                let text = res.to_string();
                transcript.push(format!("tool image_process -> {text}"));
                Ok(format!("tool_result: image_process: {text}"))
            }
            "art_pack_card" => {
                let hook = self.hooks.art_pack_card.as_ref().ok_or(
                    "art_pack_card tool is not connected on this server instance yet (needs openro-assets hook).",
                )?;
                let res = hook(args)?;
                let text = res.to_string();
                transcript.push(format!("tool art_pack_card -> {text}"));
                Ok(format!("tool_result: art_pack_card: {text}"))
            }
            "assets_upload" => {
                let res = if let Some(hook) = &self.hooks.assets_upload {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/assets/upload", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool assets_upload -> {text}"));
                thread.push("Uploaded authorised assets to Roblox Open Cloud.".to_string());
                Ok(format!("tool_result: assets_upload: {text}"))
            }
            "opencloud_status" => {
                let res = if let Some(hook) = &self.hooks.opencloud_status {
                    hook(args)?
                } else {
                    self.invoke_opencloud_local("GET", "/api/opencloud", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool opencloud_status -> {text}"));
                Ok(format!("tool_result: opencloud_status: {text}"))
            }
            "creator_hub_status" => {
                let res = if let Some(hook) = &self.hooks.creator_hub_status {
                    hook(args)?
                } else {
                    let q = creator_hub_status_query(args);
                    self.invoke_opencloud_local("GET", "/api/creator-hub/status", &q, args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool creator_hub_status -> {text}"));
                Ok(format!("tool_result: creator_hub_status: {text}"))
            }
            "creator_hub_setup" => {
                let res = if let Some(hook) = &self.hooks.creator_hub_setup {
                    hook(args)?
                } else if self.effective_project_root().is_some() {
                    self.invoke_opencloud_local("POST", "/api/projects/_active/creator-hub/setup", "", args)?
                } else {
                    self.invoke_opencloud_local("POST", "/api/creator-hub/setup", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool creator_hub_setup -> {text}"));
                thread.push("Checked/configured Roblox Creator Hub setup.".to_string());
                Ok(format!("tool_result: creator_hub_setup: {text}"))
            }
            "creator_hub_configure" => {
                let res = if let Some(hook) = &self.hooks.creator_hub_configure {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/creator-hub/configure", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool creator_hub_configure -> {text}"));
                thread.push("Configured Roblox Creator Hub experience settings.".to_string());
                Ok(format!("tool_result: creator_hub_configure: {text}"))
            }
            "assets_authorise" => {
                let res = if let Some(hook) = &self.hooks.assets_authorise {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/assets/authorise", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool assets_authorise -> {text}"));
                thread.push("Updated authorised asset paths in openro.project.json.".to_string());
                Ok(format!("tool_result: assets_authorise: {text}"))
            }
            "assets_codegen" => {
                let res = if let Some(hook) = &self.hooks.assets_codegen {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/assets/codegen", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool assets_codegen -> {text}"));
                thread.push("Generated src/shared/AssetIds.luau.".to_string());
                Ok(format!("tool_result: assets_codegen: {text}"))
            }
            "products_sync" => {
                let res = if let Some(hook) = &self.hooks.products_sync {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/products/sync", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool products_sync -> {text}"));
                thread.push("Synced approved Game Passes and Developer Products via Open Cloud.".to_string());
                Ok(format!("tool_result: products_sync: {text}"))
            }
            "products_codegen" => {
                let res = if let Some(hook) = &self.hooks.products_codegen {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/products/codegen", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool products_codegen -> {text}"));
                thread.push("Generated src/shared/ProductIds.luau.".to_string());
                Ok(format!("tool_result: products_codegen: {text}"))
            }
            "project_publish" => {
                // Never auto-inject `confirm: true` — live `versionType: "Published"` stays gated.
                let res = if let Some(hook) = &self.hooks.project_publish {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/publish", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool project_publish -> {text}"));
                thread.push("Published place build via Roblox Open Cloud.".to_string());
                Ok(format!("tool_result: project_publish: {text}"))
            }
            "universe_get" => {
                let res = if let Some(hook) = &self.hooks.universe_get {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("GET", "/api/projects/_active/universe", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool universe_get -> {text}"));
                Ok(format!("tool_result: universe_get: {text}"))
            }
            "universe_set" => {
                let res = if let Some(hook) = &self.hooks.universe_set {
                    hook(args)?
                } else {
                    let _ = self.require_project_root()?;
                    self.invoke_opencloud_local("POST", "/api/projects/_active/universe", "", args)?
                };
                let text = res.to_string();
                transcript.push(format!("tool universe_set -> {text}"));
                thread.push("Updated Roblox Experience settings.".to_string());
                Ok(format!("tool_result: universe_set: {text}"))
            }
            "liveops_datastore_list" => {
                let _ = self.require_project_root()?;
                let q = build_query_from_args(args, &["store", "scope", "prefix", "cursor", "limit", "kind"]);
                let res = self.invoke_opencloud_local(
                    "GET",
                    "/api/projects/_active/liveops/datastores",
                    &q,
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_datastore_list -> {text}"));
                Ok(format!("tool_result: liveops_datastore_list: {text}"))
            }
            "liveops_datastore_get" => {
                let _ = self.require_project_root()?;
                let q = build_query_from_args(args, &["store", "scope", "key", "kind"]);
                let res = self.invoke_opencloud_local(
                    "GET",
                    "/api/projects/_active/liveops/datastores/entry",
                    &q,
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_datastore_get -> {text}"));
                Ok(format!("tool_result: liveops_datastore_get: {text}"))
            }
            "liveops_datastore_set" => {
                if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                    return Err(
                        "Live DataStore writes require owner confirmation (confirm: true). Ask the owner first."
                            .to_string(),
                    );
                }
                let _ = self.require_project_root()?;
                let res = self.invoke_opencloud_local(
                    "POST",
                    "/api/projects/_active/liveops/datastores/entry",
                    "",
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_datastore_set -> {text}"));
                thread.push("Updated live DataStore entry (pre-mutation backup saved).".to_string());
                Ok(format!("tool_result: liveops_datastore_set: {text}"))
            }
            "liveops_messaging_publish" => {
                if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                    return Err(
                        "Live MessagingService broadcasts require owner confirmation (confirm: true). Ask the owner first."
                            .to_string(),
                    );
                }
                let _ = self.require_project_root()?;
                let res = self.invoke_opencloud_local(
                    "POST",
                    "/api/projects/_active/liveops/messaging/publish",
                    "",
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_messaging_publish -> {text}"));
                thread.push("Broadcasted live MessagingService topic message.".to_string());
                Ok(format!("tool_result: liveops_messaging_publish: {text}"))
            }
            "liveops_remote_config_push" => {
                if args.get("push_to_datastore").and_then(Value::as_bool) == Some(true)
                    && args.get("confirm").and_then(Value::as_bool) != Some(true)
                {
                    return Err(
                        "Pushing Remote Config to live DataStores requires owner confirmation (confirm: true). Ask the owner first."
                            .to_string(),
                    );
                }
                let _ = self.require_project_root()?;
                let res = self.invoke_opencloud_local(
                    "POST",
                    "/api/projects/_active/liveops/remote-config",
                    "",
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_remote_config_push -> {text}"));
                thread.push("Updated liveops/remote-config.json and src/shared/LiveConfig.luau.".to_string());
                Ok(format!("tool_result: liveops_remote_config_push: {text}"))
            }
            "liveops_telemetry_summary" => {
                let _ = self.require_project_root()?;
                let res = self.invoke_opencloud_local(
                    "GET",
                    "/api/projects/_active/liveops/telemetry/summary",
                    "",
                    args,
                )?;
                let text = res.to_string();
                transcript.push(format!("tool liveops_telemetry_summary -> {text}"));
                Ok(format!("tool_result: liveops_telemetry_summary: {text}"))
            }
            "blender_generate" | "mesh_generate" => {
                let res = if let Some(hook) = &self.hooks.mesh_generate {
                    hook(args)?
                } else {
                    let root = self
                        .effective_project_root()
                        .unwrap_or_else(|| self.effective_assets_root());
                    crate::library::generate_blender_mesh(&root, args, None)?
                };
                let text = res.to_string();
                let name = res
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("3D mesh");
                transcript.push(format!("tool {tool} -> {text}"));
                thread.push(format!("Generated 3D mesh {name}."));
                Ok(format!("tool_result: {tool}: {text}"))
            }
            "audio_generate" => {
                let res = if let Some(hook) = &self.hooks.audio_generate {
                    hook(args)?
                } else {
                    let root = self
                        .effective_project_root()
                        .unwrap_or_else(|| self.effective_assets_root());
                    crate::audio_synth::generate_audio_asset(&root, args)?
                };
                let text = res.to_string();
                let name = res
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SFX audio");
                transcript.push(format!("tool audio_generate -> {text}"));
                thread.push(format!("Synthesized WAV audio {name}."));
                Ok(format!("tool_result: audio_generate: {text}"))
            }
            "adapter_list" => {
                let res = if let Some(hook) = &self.hooks.adapter_list {
                    hook(args)?
                } else {
                    let data_dir = self.effective_assets_root();
                    let store = crate::secrets::SecretStore::for_data_dir(&data_dir);
                    let cap = args.get("capability").and_then(|v| v.as_str());
                    crate::tool_adapters::AdapterRegistry::load(&data_dir).list_json(&store, cap)
                };
                let text = res.to_string();
                transcript.push(format!("tool adapter_list -> {text}"));
                Ok(format!("tool_result: adapter_list: {text}"))
            }
            "adapter_invoke" => {
                let res = if let Some(hook) = &self.hooks.adapter_invoke {
                    hook(args)?
                } else {
                    let data_dir = self.effective_assets_root();
                    let store = crate::secrets::SecretStore::for_data_dir(&data_dir);
                    let proj = self.effective_project_root();
                    let cap = args.get("capability").and_then(|v| v.as_str());
                    let id = args.get("adapter_id").and_then(|v| v.as_str());
                    let params = args.get("params").unwrap_or(args);
                    let ctx = crate::tool_adapters::InvokeCtx {
                        data_dir: &data_dir,
                        assets_root: &data_dir,
                        store: &store,
                        project_dir: proj.as_deref(),
                    };
                    crate::tool_adapters::invoke(&ctx, cap, id, params)?
                };
                let text = res.to_string();
                let aid = res
                    .get("adapter_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("adapter");
                transcript.push(format!("tool adapter_invoke -> {text}"));
                thread.push(format!("Ran tool adapter {aid}."));
                Ok(format!("tool_result: adapter_invoke: {text}"))
            }
            t if is_official_studio_tool(t) => {
                let chat_id = self.active_build_chat.clone();
                let (val, _) = self.invoke_studio_tool(&chat_id, t, args.clone())?;
                let text = val.to_string();
                transcript.push(format!("tool {t} -> {text}"));
                Ok(format!("tool_result: {t}: {text}"))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    /// The previous source of `path` (or None if absent), read WITHOUT
    /// recording anything. Callers record it only after the change
    /// itself succeeds — a failed tool call must never enter the
    /// change set.
    /// The previous state of a script path recorded by THIS ask.
    /// Outer None = not recorded by this ask; inner None = recorded
    /// as absent. Stored sets from earlier asks are never consulted:
    /// their checkpoints describe the place before THOSE asks, and a
    /// new ask's prev must come from the tree as it is now.
    fn recorded_prev_script(&self, path: &str) -> Option<Option<String>> {
        self.current_changes.iter().find_map(|c| match c {
            Change::Script(p, prev, _) if p == path => Some(prev.clone()),
            _ => None,
        })
    }

    fn prev_script(&mut self, path: &str) -> Result<Option<String>, String> {
        if let Some(prev) = self.recorded_prev_script(path) {
            return Ok(prev);
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Record a successful change: the first touch of a path in an
    /// ask fixes its `prev`; every touch updates the `post` (the
    /// state the ask most recently left), which is what a later
    /// revert compares the live place against.
    fn record_script(&mut self, path: &str, prev: Option<String>, post: Option<String>) {
        for c in self.current_changes.iter_mut() {
            if let Change::Script(p, _, existing_post) = c {
                if p == path {
                    *existing_post = post;
                    return;
                }
            }
        }
        self.current_changes
            .push(Change::Script(path.to_string(), prev, post));
    }

    fn recorded_prev_instance(&self, path: &str) -> Option<Option<Value>> {
        self.current_changes.iter().find_map(|c| match c {
            Change::Instance(p, prev, _) if p == path => Some(prev.clone()),
            _ => None,
        })
    }

    /// True when the place already holds an instance at this step's path
    /// with the same class.
    fn already_in_place(&mut self, args: &Value) -> Result<bool, String> {
        let (Some(parent), Some(name), Some(class)) = (
            args.get("parent").and_then(|v| v.as_str()),
            args.get("name").and_then(|v| v.as_str()),
            args.get("class_name").and_then(|v| v.as_str()),
        ) else {
            return Ok(false);
        };
        let path = format!("{parent}.{name}");
        Ok(self
            .prev_instance_current(&path)?
            .and_then(|r| r.get("class_name").and_then(|c| c.as_str()).map(|c| c == class))
            .unwrap_or(false))
    }

    /// The previous instance record at `path` (or None if absent),
    /// read without recording.
    fn prev_instance(&mut self, path: &str) -> Result<Option<Value>, String> {
        if let Some(prev) = self.recorded_prev_instance(path) {
            return Ok(prev);
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/instances")
            .and_then(|s| s.get(path))
            .cloned())
    }

    fn record_instance(&mut self, path: &str, prev: Option<Value>, post: Option<Value>) {
        for c in self.current_changes.iter_mut() {
            if let Change::Instance(p, _, existing_post) = c {
                if p == path {
                    *existing_post = post;
                    return;
                }
            }
        }
        self.current_changes
            .push(Change::Instance(path.to_string(), prev, post));
    }

    /// Restore one chat's change set: rewrite previous content, delete
    /// what the asks created — scripts and instances alike. Returns the
    /// restored paths. An empty (or absent) set is NOTHING_TO_REVERT,
    /// never a fabricated success.
    ///
    /// The restore is failure-atomic and idempotent: the set is only
    /// consumed once EVERY entry is restored, and each restore first
    /// checks the current state — restoring an already-absent instance
    /// to absent is success, not an error. A failed revert keeps its
    /// set, so it can be retried and converges.
    pub fn revert_chat(&mut self, chat_id: &str) -> Result<Vec<String>, String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to revert".to_string());
        }
        let changes = match self.changes_by_chat.get(chat_id) {
            Some(stack) => match stack.last() {
                Some(c) if !c.changes.is_empty() => c.changes.clone(),
                _ => return Err(NOTHING_TO_REVERT.to_string()),
            },
            None => return Err(NOTHING_TO_REVERT.to_string()),
        };
        let popped_skills = self.top_skills(chat_id);
        let mut restored = Vec::new();
        for change in changes.iter().rev() {
            let path = match change {
                Change::Script(p, _, _) | Change::Instance(p, _, _) => p.clone(),
            };
            if let Err(e) = self.restore_one(change) {
                return Err(format!(
                    "revert stopped at {path}: {e} — {} change(s) were restored, the rest are \
                     untouched, and the change set is kept so revert can be tried again",
                    restored.len()
                ));
            }
            restored.push(path);
        }
        // Success: pop this ask's set and save it onto the chat's Redo stack.
        // Earlier asks' sets stay on the undo stack, still revertable.
        if let Some(stack) = self.changes_by_chat.get_mut(chat_id) {
            if let Some(popped) = stack.pop() {
                let rstack = self.redo_by_chat.entry(chat_id.to_string()).or_default();
                if rstack.len() >= crate::place::MAX_HISTORY_DEPTH {
                    rstack.remove(0);
                }
                rstack.push(popped);
            }
            if stack.is_empty() {
                self.changes_by_chat.remove(chat_id);
            }
        }
        // The place is back to an earlier state. With no set left it is
        // the game as it was before any build; otherwise its verdict is
        // not known, so it stays unproven (and the failed run is gone).
        let remaining = self.changes_by_chat.contains_key(chat_id);
        let kept = self.chat_skills(chat_id);
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        proof.unproven = remaining;
        proof.failed_console = None;
        for skill in popped_skills {
            if !kept.contains(&skill) && !proof.undone.contains(&skill) {
                proof.undone.push(skill);
            }
        }
        self.save_proof();
        self.save_changes();
        self.sync_undo();
        let mut rev_ev = StudioActionInput::new(
            StudioActionKind::Reverted,
            format!("Reverted {} change(s) in Studio", restored.len()),
        );
        rev_ev.path = "Workspace".to_string();
        rev_ev.class_name = "ChangeHistoryService".to_string();
        self.emit_studio_action(rev_ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(restored)
    }

    /// Re-apply the most recently undone change set for `chat_id` (`POST /api/redo`).
    /// Returns `(reapplied_paths, reapplied_skills)`.
    pub fn redo_chat(&mut self, chat_id: &str) -> Result<(Vec<String>, Vec<String>), String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to redo".to_string());
        }
        let set = match self.redo_by_chat.get(chat_id) {
            Some(stack) => match stack.last() {
                Some(c) if !c.changes.is_empty() => c.clone(),
                _ => return Err(NOTHING_TO_REDO.to_string()),
            },
            None => return Err(NOTHING_TO_REDO.to_string()),
        };
        let mut restored = Vec::new();
        for change in &set.changes {
            let path = match change {
                Change::Script(p, _, _) | Change::Instance(p, _, _) => p.clone(),
            };
            if let Err(e) = self.reapply_one(change) {
                return Err(format!(
                    "redo stopped at {path}: {e} — {} change(s) were re-applied",
                    restored.len()
                ));
            }
            restored.push(path);
        }
        if let Some(stack) = self.redo_by_chat.get_mut(chat_id) {
            stack.pop();
            if stack.is_empty() {
                self.redo_by_chat.remove(chat_id);
            }
        }
        let skills = set.skills.clone();
        self.changes_by_chat
            .entry(chat_id.to_string())
            .or_default()
            .push(set);
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        proof.undone.retain(|s| !skills.contains(s));
        self.save_proof();
        self.save_changes();
        self.sync_undo();
        let mut redo_ev = StudioActionInput::new(
            StudioActionKind::Restored,
            format!("Redid {} change(s) in Studio", restored.len()),
        );
        redo_ev.path = "Workspace".to_string();
        redo_ev.class_name = "ChangeHistoryService".to_string();
        self.emit_studio_action(redo_ev);
        let _ = self.auto_capture_mcp_frame();
        Ok((restored, skills))
    }

    /// Re-apply a single undone change (`prev` -> `post`), verifying that the
    /// live place still holds `prev` (or already holds `post`).
    fn reapply_one(&mut self, change: &Change) -> Result<(), String> {
        match change {
            Change::Script(path, prev, post) => {
                let current = self.prev_script_current(path)?;
                if current == *post {
                    return Ok(());
                }
                if current != *prev {
                    return Err(format!(
                        "{path} changed since it was undone — refusing to overwrite newer edits"
                    ));
                }
                match post {
                    Some(source) => {
                        self.studio_mut()?.call_tool(
                            "write_script",
                            json!({ "path": path, "source": source }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_script", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
            Change::Instance(path, prev, post) => {
                let current = self.prev_instance_current(path)?;
                if current == *post {
                    return Ok(());
                }
                if current != *prev {
                    return Err(format!(
                        "{path} changed since it was undone — refusing to overwrite newer edits"
                    ));
                }
                match post {
                    Some(record) => {
                        let (parent, name) = match path.rsplit_once('.') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), path.clone()),
                        };
                        let class_name = record
                            .get("class_name")
                            .and_then(|c| c.as_str())
                            .ok_or_else(|| {
                                format!("the redo record for {path} is unreadable (no class name)")
                            })?;
                        if current.is_some() {
                            self.studio_mut()?
                                .call_tool("delete_instance", json!({ "path": path }))?;
                        }
                        self.studio_mut()?.call_tool(
                            "create_instance",
                            json!({
                                "class_name": class_name,
                                "name": name,
                                "parent": parent,
                                "properties": record.get("properties").cloned().unwrap_or(json!({})),
                            }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_instance", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Restore a single change. Identity-checked, not just
    /// existence-checked: the restore proceeds only when the place
    /// still holds exactly what this ask left (restore to prev), or
    /// already holds prev (nothing to do). Anything else means the
    /// path was changed by someone else after the ask — another chat,
    /// a hand edit in Studio — and revert REFUSES rather than
    /// silently destroying work it does not own.
    fn restore_one(&mut self, change: &Change) -> Result<(), String> {
        match change {
            Change::Script(path, prev, post) => {
                let current = self.prev_script_current(path)?;
                if current == *prev {
                    return Ok(()); // already at the previous state
                }
                if current != *post {
                    return Err(format!(
                        "{path} has changed since this ask — it no longer holds what the ask \
                         left, so reverting would destroy work this chat does not own"
                    ));
                }
                match prev {
                    Some(source) => {
                        self.studio_mut()?.call_tool(
                            "write_script",
                            json!({ "path": path, "source": source }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_script", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
            Change::Instance(path, prev, post) => {
                let current = self.prev_instance_current(path)?;
                if current == *prev {
                    return Ok(());
                }
                if current != *post {
                    return Err(format!(
                        "{path} has changed since this ask — it no longer holds what the ask \
                         left, so reverting would destroy work this chat does not own"
                    ));
                }
                match prev {
                    Some(record) => {
                        let (parent, name) = match path.rsplit_once('.') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), path.clone()),
                        };
                        let class_name = record
                            .get("class_name")
                            .and_then(|c| c.as_str())
                            .ok_or_else(|| {
                                format!("the change record for {path} is unreadable (no class name)")
                            })?;
                        if current.is_some() {
                            self.studio_mut()?
                                .call_tool("delete_instance", json!({ "path": path }))?;
                        }
                        self.studio_mut()?.call_tool(
                            "create_instance",
                            json!({
                                "class_name": class_name,
                                "name": name,
                                "parent": parent,
                                "properties": record.get("properties").cloned().unwrap_or(json!({})),
                            }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_instance", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Current state reads for the revert path (no recorded-history
    /// shortcuts — the place as it is NOW).
    fn prev_script_current(&mut self, path: &str) -> Result<Option<String>, String> {
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    fn prev_instance_current(&mut self, path: &str) -> Result<Option<Value>, String> {
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/instances")
            .and_then(|s| s.get(path))
            .cloned())
    }

    pub fn place_tree(&mut self) -> Result<Value, String> {
        self.studio_mut()?.get_place_tree()
    }

    pub fn place_tree_bytes(&mut self) -> Result<Vec<u8>, String> {
        self.studio_mut()?.get_place_tree_bytes()
    }

    pub fn local_place_revision(&self) -> Option<u64> {
        self.studio.as_ref().and_then(|s| s.local_revision())
    }

    pub fn analyze_local_luau(&self) -> Option<crate::luau_check::AnalysisReport> {
        self.studio.as_ref().and_then(|s| s.analyze_local_luau())
    }

    fn studio_mut(&mut self) -> Result<&mut PlaceBackend, String> {
        self.studio
            .as_mut()
            .ok_or_else(|| "Studio not connected".to_string())
    }
}

enum PlaytestOutcome {
    Passed(String),
    Failed(String, String),
    Unreadable(String, String),
}

fn changeset_from_trees(before: &Value, after: &Value) -> Vec<Change> {
    let empty = serde_json::Map::new();
    let b_inst = before
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_inst = after
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let b_scr = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_scr = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    let mut changes = Vec::new();
    let mut inst_keys: BTreeSet<&str> = BTreeSet::new();
    for k in b_inst.keys().chain(a_inst.keys()) {
        inst_keys.insert(k.as_str());
    }
    for k in inst_keys {
        let prev = b_inst.get(k).cloned();
        let post = a_inst.get(k).cloned();
        if prev != post {
            changes.push(Change::Instance(k.to_string(), prev, post));
        }
    }

    let mut scr_keys: BTreeSet<&str> = BTreeSet::new();
    for k in b_scr.keys().chain(a_scr.keys()) {
        scr_keys.insert(k.as_str());
    }
    for k in scr_keys {
        let prev = b_scr.get(k).and_then(|v| v.as_str()).map(String::from);
        let post = a_scr.get(k).and_then(|v| v.as_str()).map(String::from);
        if prev != post {
            changes.push(Change::Script(k.to_string(), prev, post));
        }
    }
    changes
}

fn extract_vec3_prop(props: Option<&Value>, key: &str) -> Option<[f64; 3]> {
    let val = props?.get(key)?;
    if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let x = arr[0].as_f64()?;
            let y = arr[1].as_f64()?;
            let z = arr[2].as_f64()?;
            if x.is_finite() && y.is_finite() && z.is_finite() {
                return Some([x, y, z]);
            }
        }
    }
    if let Some(obj) = val.as_object() {
        let x = obj.get("x").or_else(|| obj.get("X")).and_then(|v| v.as_f64())?;
        let y = obj.get("y").or_else(|| obj.get("Y")).and_then(|v| v.as_f64())?;
        let z = obj.get("z").or_else(|| obj.get("Z")).and_then(|v| v.as_f64())?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn extract_color_prop(props: Option<&Value>) -> Option<String> {
    let val = props?.get("Color").or_else(|| props?.get("BrickColor"))?;
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let r = arr[0].as_f64().unwrap_or(0.5);
            let g = arr[1].as_f64().unwrap_or(0.5);
            let b = arr[2].as_f64().unwrap_or(0.5);
            let scale = |c: f64| -> u8 {
                if (0.0..=1.0).contains(&c) {
                    (c * 255.0).round() as u8
                } else {
                    c.clamp(0.0, 255.0).round() as u8
                }
            };
            return Some(format!("#{:02X}{:02X}{:02X}", scale(r), scale(g), scale(b)));
        }
    }
    None
}

fn snippet_from_source(source: &str) -> String {
    let lines: Vec<&str> = source.lines().take(6).collect();
    let joined = lines.join("\n");
    joined.chars().take(280).collect()
}

/// Does this summary claim building work? Sentence-scoped: a claim
/// verb in a sentence with no suppressor (negation, or "already" for
/// pre-existing work) counts; suppressors in OTHER sentences do not
/// bleed across the boundary. Token-based, so "padded" never trips
/// "added". The flag this drives is a factual ledger line, not an
/// accusation — the line states what the ask did (nothing), and the
/// reader judges the summary against it.
fn claims_building(summary: &str) -> bool {
    const CLAIMS: [&str; 16] = [
        "built", "created", "wrote", "added", "placed", "generated", "constructed",
        "installed", "made", "shipped", "implemented", "assembled", "wired", "spawned",
        "fixed", "repaired",
    ];
    const SUPPRESSORS: [&str; 14] = [
        "nothing", "not", "no", "never", "didn't", "didnt", "wasn't", "wasnt", "haven't",
        "havent", "isn't", "isnt", "already", "couldn't",
    ];
    // Normalise curly apostrophes first: models emit them routinely,
    // and a U+2019 inside "wasn't" would otherwise split the token
    // and smuggle the claim past its own suppressor.
    let summary = summary.replace('\u{2019}', "'");
    for sentence in summary.split(['.', '!', '?', '\n', ';']) {
        let tokens: Vec<String> = sentence
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .collect();
        if tokens.iter().any(|t| SUPPRESSORS.contains(&t.as_str())) {
            continue;
        }
        if tokens.iter().any(|t| CLAIMS.contains(&t.as_str())) {
            return true;
        }
    }
    false
}

/// One change set from changes.json: the current object form
/// (`changes` + `skills`) or the older bare array of changes (no skills
/// recorded). Anything else is malformed.
fn parse_set(set: &Value) -> Option<ChangeSet> {
    let (items, skills) = match set {
        Value::Array(items) => (items, Vec::new()),
        Value::Object(_) => {
            let skills = set
                .get("skills")?
                .as_array()?
                .iter()
                .map(|s| s.as_str().map(|x| x.to_string()))
                .collect::<Option<Vec<_>>>()?;
            (set.get("changes")?.as_array()?, skills)
        }
        _ => return None,
    };
    let mut changes = Vec::new();
    for item in items {
        changes.push(Change::from_json(item)?);
    }
    Some(ChangeSet { changes, skills })
}

/// The proof file: a corrupt one is set aside and the chats start with
/// no verdicts recorded (recorded in `recovered`).
fn load_proof(path: &std::path::Path, recovered: &mut Vec<String>) -> HashMap<String, Proof> {
    let Ok(text) = std::fs::read_to_string(path) else { return HashMap::new() };
    let parsed = (|| -> Option<HashMap<String, Proof>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut out = HashMap::new();
        for (chat, p) in v.as_object()? {
            let unproven = p.get("unproven")?.as_bool()?;
            let failed_console = match p.get("failed_console")? {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => return None,
            };
            let undone = p
                .get("undone")?
                .as_array()?
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()?;
            out.insert(chat.clone(), Proof { unproven, failed_console, undone });
        }
        Some(out)
    })();
    match parsed {
        Some(map) => map,
        None => {
            if let Some(bak) = crate::server::backup_corrupt(path) {
                recovered.push(bak.file_name().unwrap_or_default().to_string_lossy().into_owned());
            }
            HashMap::new()
        }
    }
}

/// The display name of the idea that adds a skill (e.g. "Money that saves"
/// for currency). Internal skill ids never reach the user through this.
pub fn idea_title_for_skill(sk: &str) -> String {
    let all = crate::ideas::catalogue();
    let id = format!("add-{sk}");
    all.iter()
        .find(|i| i.id == id.as_str())
        .or_else(|| all.iter().find(|i| i.skills.contains(&sk)))
        .map(|i| i.title.to_string())
        .unwrap_or_else(|| "That".to_string())
}

/// The one plain refusal used on every path (chat asks and idea buttons)
/// when a build needs a prerequisite the game does not have.
pub fn prerequisite_sentence(blocked_title: &str, need_skill: &str) -> String {
    let needed = idea_title_for_skill(need_skill);
    format!("{blocked_title} needs \"{needed}\" first. Build \"{needed}\" first, then ask again.")
}

/// The user's sentence for a chat-path step refused because its
/// prerequisite skill was undone: the display names, never skill ids.
fn plain_prerequisite(skill: &str, need: &str) -> String {
    prerequisite_sentence(&idea_title_for_skill(skill), need)
}

/// A plain sentence for a Studio refusal; the raw text goes to the transcript.
fn plain_bridge(e: &str) -> String {
    if e.contains("Studio bridge error") {
        "Studio did not accept that step.".to_string()
    } else {
        e.to_string()
    }
}

pub fn parse_step(content: &str) -> Result<Value, String> {
    let mut cleaned = content.trim();
    while let Some(after_open) = cleaned.strip_prefix("<think>") {
        if let Some(end_idx) = after_open.find("</think>") {
            cleaned = after_open[end_idx + "</think>".len()..].trim();
        } else {
            break;
        }
    }
    if let Some(rest) = cleaned.strip_prefix("```") {
        let after_lang = rest.find('\n').map(|i| &rest[i + 1..]).unwrap_or(rest);
        if let Some(end) = after_lang.rfind("```") {
            cleaned = after_lang[..end].trim();
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        if v.is_object() {
            return Ok(v);
        }
    }
    // Gap #42: if the reply contains a `{"steps": [...]}` batch envelope, ONLY
    // accept the complete outer batch object (direct or repaired). Never scan
    // past the broken envelope into an inner step object and silently drop the
    // rest of the batch.
    if let Some(env_start) = find_steps_envelope_start(cleaned) {
        let slice = &cleaned[env_start..];
        let mut de = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
        let direct_err = match de.next() {
            Some(Ok(v)) if v.is_object() && v.get("steps").is_some_and(|x| x.is_array()) => {
                return Ok(v);
            }
            Some(Err(e)) => Some(e.to_string()),
            _ => None,
        };
        let repaired = repair_llm_json(slice);
        let mut de2 = serde_json::Deserializer::from_str(&repaired).into_iter::<Value>();
        let repair_err = match de2.next() {
            Some(Ok(v)) if v.is_object() && v.get("steps").is_some_and(|x| x.is_array()) => {
                return Ok(v);
            }
            Some(Err(e)) => e.to_string(),
            _ => direct_err.unwrap_or_else(|| "invalid JSON object".to_string()),
        };
        return Err(format!(
            "batched {{\"steps\": [...]}} JSON failed to parse ({repair_err}). \
             Nothing in the batch was executed — fix any unescaped quotes or backslashes inside strings, \
             or write only 1-2 files per turn, and resend valid JSON."
        ));
    }
    if let Some(v) = scan_first_json_object(cleaned) {
        return Ok(v);
    }
    let repaired = repair_llm_json(cleaned);
    if let Ok(v) = serde_json::from_str::<Value>(&repaired) {
        if v.is_object() {
            return Ok(v);
        }
    }
    if let Some(v) = scan_first_json_object(&repaired) {
        return Ok(v);
    }
    let preview: String = content.trim().chars().take(240).collect();
    Err(format!("model reply was not a JSON step: {preview}"))
}

/// Return the byte offset of the outer `{` enclosing a `"steps": [` batch
/// envelope (at JSON key position, ignoring any `"steps": [` text inside
/// quoted string values or prose before the first `{`).
fn find_steps_envelope_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut brace_stack: Vec<usize> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if brace_stack.is_empty() {
            if b == b'{' {
                brace_stack.push(i);
            }
            i += 1;
            continue;
        }
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'{' => {
                brace_stack.push(i);
                i += 1;
            }
            b'}' => {
                brace_stack.pop();
                i += 1;
            }
            b'"' => {
                if bytes[i..].starts_with(b"\"steps\"") {
                    let mut j = i + b"\"steps\"".len();
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b':' {
                        j += 1;
                        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b'[' {
                            return brace_stack.last().copied();
                        }
                    }
                }
                in_str = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

fn scan_first_json_object(text: &str) -> Option<Value> {
    let mut scan = text;
    while let Some(start) = scan.find('{') {
        let slice = &scan[start..];
        let mut de = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
        if let Some(Ok(v)) = de.next() {
            if v.is_object() {
                return Some(v);
            }
        }
        scan = &slice[1..];
    }
    None
}

fn repair_llm_json(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 32);
    let mut in_str = false;
    let mut esc = false;
    for ch in raw.chars() {
        if in_str {
            if esc {
                out.push(ch);
                esc = false;
                continue;
            }
            match ch {
                '\\' => {
                    out.push(ch);
                    esc = true;
                }
                '"' => {
                    out.push(ch);
                    in_str = false;
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {}
                c => out.push(c),
            }
        } else {
            if ch == '"' {
                in_str = true;
            }
            out.push(ch);
        }
    }
    let chars: Vec<char> = out.chars().collect();
    let mut clean = String::with_capacity(out.len());
    let mut in_s = false;
    let mut e = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_s {
            clean.push(c);
            if e {
                e = false;
            } else if c == '\\' {
                e = true;
            } else if c == '"' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_s = true;
            clean.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        clean.push(c);
        i += 1;
    }
    clean
}

/// Extract optional inline Rune skill parameters passed inside a combined
/// `write_script` call (either nested under `args`/`skill_args` or as extra
/// keys beside `path`/`skill`/`use_generated`). Returns `None` when no inline
/// parameters were provided.
fn extract_inline_skill_args(write_args: &Value) -> Option<Value> {
    if let Some(nested) = write_args
        .get("args")
        .or_else(|| write_args.get("skill_args"))
        .filter(|v| v.is_object())
    {
        return Some(nested.clone());
    }
    let obj = write_args.as_object()?;
    let mut extras = serde_json::Map::new();
    for (k, v) in obj {
        if !matches!(k.as_str(), "path" | "skill" | "use_generated" | "source") {
            extras.insert(k.clone(), v.clone());
        }
    }
    if extras.is_empty() {
        None
    } else {
        Some(Value::Object(extras))
    }
}

/// A dotted place path the bridge can be trusted with: non-empty
/// segments, no `.`/`..` segments, no separators or control characters.
/// The agent validates before forwarding — model output is not passed
/// to the bridge unchecked.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    for seg in path.split('.') {
        if seg.is_empty() || seg == ".." {
            return Err(format!("invalid path: {path}"));
        }
        if seg.chars().any(|c| c == '/' || c == '\\' || c.is_control()) {
            return Err(format!("invalid path: {path}"));
        }
    }
    Ok(())
}

enum Verdict {
    Passed,
    Failed,
    Unreadable,
}

/// Read a playtest result in the documented bridge format only:
/// JSON with a `status` of "passed"/"failed", or the bare word
/// "passed"/"failed". Anything else is Unreadable — the old substring
/// heuristic called "Playtest passed. 0 failures, 0 errors." a failure
/// (it contains "fail") and would have called the reverse a pass; a
/// verdict this product acts on must be read exactly or not at all.
fn playtest_verdict(result_text: &str) -> Verdict {
    if let Ok(v) = serde_json::from_str::<Value>(result_text) {
        if let Some(status) = v.get("status").and_then(|s| s.as_str()) {
            return match status {
                "passed" => Verdict::Passed,
                "failed" => Verdict::Failed,
                _ => Verdict::Unreadable,
            };
        }
        return Verdict::Unreadable;
    }
    let t = result_text.trim().trim_end_matches('.').to_lowercase();
    match t.as_str() {
        "passed" | "playtest passed" => Verdict::Passed,
        "failed" | "playtest failed" => Verdict::Failed,
        _ => Verdict::Unreadable,
    }
}

fn is_read_only_tool(tool: &str) -> bool {
    matches!(
        tool,
        "get_place_tree"
            | "search_game_tree"
            | "inspect_instance"
            | "script_read"
            | "script_grep"
            | "get_console_output"
            | "get_console"
            | "list_roblox_studios"
            | "get_studio_state"
            | "project_list"
            | "project_files"
            | "project_read_file"
            | "project_grep"
            | "project_check"
            | "project_run_lune"
            | "sandbox_playtest"
            | "project_scene_get"
            | "research_roblox_game"
            | "web_fetch"
            | "web_search"
            | "reference_search"
            | "adapter_list"
            | "evidence_list"
            | "unknowns_list"
            | "mechanics_check"
            | "study_status"
            | "opencloud_status"
            | "creator_hub_status"
            | "universe_get"
            | "liveops_datastore_list"
            | "liveops_datastore_get"
            | "liveops_telemetry_summary"
    )
}

/// Build an encoded query string from string/number keys in `args`.
pub fn build_query_from_args(args: &Value, keys: &[&str]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for &k in keys {
        let val_str = match args.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        if let Some(v) = val_str {
            let mut enc = String::with_capacity(v.len() + 8);
            for b in v.bytes() {
                if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                    enc.push(b as char);
                } else {
                    enc.push_str(&format!("%{b:02X}"));
                }
            }
            parts.push(format!("{k}={enc}"));
        }
    }
    parts.join("&")
}

/// Build the query string for `GET /api/creator-hub/status?...` from tool args.
pub fn creator_hub_status_query(args: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(url) = args
        .get("cdp_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let mut enc = String::with_capacity(url.len() + 16);
        for b in url.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                enc.push(b as char);
            } else {
                enc.push_str(&format!("%{b:02X}"));
            }
        }
        parts.push(format!("cdp_url={enc}"));
    }
    if args.get("check_login").and_then(|v| v.as_bool()) == Some(true) {
        parts.push("check_login=1".to_string());
    }
    if args.get("save_creator").and_then(|v| v.as_bool()) == Some(true) {
        parts.push("save_creator=1".to_string());
    }
    parts.join("&")
}

fn format_luau_errors(report: &crate::luau_check::AnalysisReport) -> String {
    report
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::luau_check::Severity::Error)
        .map(|d| format!("Line {} [{}]: {}", d.line, d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a compact Whole-Game Context Summary injected into turn 1 so the model sees
/// the open Roblox place immediately (service counts, instance/script paths, and full
/// contents of caller-selected `context_paths` capped sensibly).
fn build_whole_game_context(tree: &Value, context_paths: &[String], compact: bool) -> String {
    let empty = serde_json::Map::new();
    let instances = tree
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let scripts = tree
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    if instances.is_empty() && scripts.is_empty() && context_paths.is_empty() {
        return String::new();
    }

    let mut svc_counts: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for k in instances.keys() {
        let root = k.split('.').next().unwrap_or("Other");
        svc_counts.entry(root).or_default().0 += 1;
    }
    for k in scripts.keys() {
        let root = k.split('.').next().unwrap_or("Other");
        svc_counts.entry(root).or_default().1 += 1;
    }

    let mut out = String::from("[OPENRO WHOLE-GAME CONTEXT]\n");
    let svc_summary: Vec<String> = svc_counts
        .iter()
        .map(|(svc, (ic, sc))| format!("{svc}: {ic} instance(s), {sc} script(s)"))
        .collect();
    if !svc_summary.is_empty() {
        out.push_str("Services: ");
        out.push_str(&svc_summary.join(" | "));
        out.push('\n');
    }

    let path_cap = if compact { 16 } else { 40 };
    if !scripts.is_empty() {
        let mut s_paths: Vec<&str> = scripts.keys().map(String::as_str).collect();
        s_paths.sort();
        let shown: Vec<&str> = s_paths.iter().take(path_cap).copied().collect();
        out.push_str(&format!(
            "Scripts ({}): {}{}\n",
            s_paths.len(),
            shown.join(", "),
            if s_paths.len() > path_cap { ", ..." } else { "" }
        ));
    }
    if !instances.is_empty() {
        let mut i_paths: Vec<&str> = instances.keys().map(String::as_str).collect();
        i_paths.sort();
        let shown: Vec<&str> = i_paths.iter().take(path_cap).copied().collect();
        out.push_str(&format!(
            "Instances ({}): {}{}\n",
            i_paths.len(),
            shown.join(", "),
            if i_paths.len() > path_cap { ", ..." } else { "" }
        ));
    }

    if !context_paths.is_empty() {
        let max_per_file = if compact { 2048 } else { 8192 };
        let max_total = if compact { 4096 } else { 24576 };
        let mut used = 0usize;
        let mut seen = BTreeSet::new();
        for raw in context_paths {
            let p = raw.trim();
            if p.is_empty() || !seen.insert(p.to_string()) || check_path(p).is_err() {
                continue;
            }
            if used >= max_total {
                break;
            }
            if let Some(src) = scripts.get(p).and_then(|v| v.as_str()) {
                let budget = (max_total - used).min(max_per_file);
                let mut end = budget.min(src.len());
                while end > 0 && !src.is_char_boundary(end) {
                    end -= 1;
                }
                let slice = &src[..end];
                out.push_str(&format!(
                    "\n--- Attached Script: {p} ---\n{slice}{}\n",
                    if end < src.len() { "\n-- [truncated]" } else { "" }
                ));
                used += slice.len();
            } else if let Some(inst) = instances.get(p) {
                let text = inst.to_string();
                let budget = (max_total - used).min(max_per_file);
                let mut end = budget.min(text.len());
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                let slice = &text[..end];
                out.push_str(&format!("\n--- Attached Instance: {p} ---\n{slice}\n"));
                used += slice.len();
            }
        }
    }

    out
}

// ============================================================================
// P1: Project-aware context, safe project file tools, project verification,
// and Plan -> Execute long runs with checkpoints, resume & FIFO run queue.
// ============================================================================

impl Agent {
    fn require_project_root(&self) -> Result<std::path::PathBuf, String> {
        self.effective_project_root().ok_or_else(|| {
            "No active Rojo project is open. Open or create a project first (or pass project_root)."
                .to_string()
        })
    }

    fn project_sync_local(&mut self, root: &std::path::Path, args: &Value) -> Result<Value, String> {
        let prune = args.get("prune").and_then(|v| v.as_bool()).unwrap_or(false);
        let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        // When a live Studio MCP bridge is attached, use openro-core's real chunked push-sync directly (zero deadlock).
        if let Some(PlaceBackend::Mcp(mcp)) = self.studio.as_mut() {
            let manifest = crate::projects::manifest(root)?;
            let rojo = manifest["rojoProject"]
                .as_str()
                .unwrap_or(crate::projects::DEFAULT_ROJO_FILE)
                .to_string();
            let tree = crate::projects::load_tree(root, &rojo)?;
            let flat = crate::studio_sync::ordered(tree.flatten());
            let d = crate::studio_sync::diff(mcp, &flat)?;
            if dry_run {
                return Ok(json!({
                    "ok": true,
                    "dry_run": true,
                    "changes": d.changes.len(),
                    "extra": d.extra.len(),
                    "chunks": d.chunks,
                }));
            }
            let a = crate::studio_sync::apply(mcp, &flat, &d, prune)?;
            let after = crate::studio_sync::diff(mcp, &flat)?;
            let remaining = after.changes.len() + if prune { after.extra.len() } else { 0 };
            return Ok(json!({
                "ok": a.errors.is_empty() && remaining == 0,
                "applied": { "created": a.created, "updated": a.updated, "deleted": a.deleted },
                "errors": a.errors,
                "chunks": d.chunks + a.chunks + after.chunks,
                "verify": { "remaining": remaining, "extra": after.extra.len() },
            }));
        }
        let src = root.join("src");
        let mut synced = 0usize;
        let mut errors = Vec::new();
        if src.is_dir() {
            let mut files = Vec::new();
            collect_project_files(root, &src, &mut files, 400);
            for rel in files {
                if !(rel.ends_with(".luau") || rel.ends_with(".lua")) {
                    continue;
                }
                let Some(place_path) = rojo_rel_to_place_path(&rel) else {
                    continue;
                };
                if let Ok(source) = std::fs::read_to_string(root.join(&rel)) {
                    if let Ok(studio) = self.studio_mut() {
                        match studio.call_tool(
                            "write_script",
                            json!({ "path": place_path, "source": source }),
                        ) {
                            Ok(_) => synced += 1,
                            Err(e) => errors.push(format!("{rel}: {e}")),
                        }
                    }
                }
            }
        }
        Ok(json!({
            "ok": errors.is_empty(),
            "synced_scripts": synced,
            "errors": errors,
        }))
    }

    fn run_project_playtest(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<(bool, String), String> {
        if let Some(hook) = &self.hooks.project_playtest {
            let res = hook(args)?;
            let passed = res.get("passed").and_then(|v| v.as_bool()).unwrap_or(false);
            let errors = res
                .pointer("/console/errors")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let failed_checks = res
                .get("checks")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|c| c.get("passed").and_then(|p| p.as_bool()) == Some(false))
                        .map(|c| {
                            format!(
                                "check '{}' failed: {}",
                                c.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                                c.get("error")
                                    .and_then(|e| e.as_str())
                                    .unwrap_or("assertion mismatch")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            transcript.push(format!("tool project_playtest -> passed={passed}"));
            if passed {
                thread.push("Project playtest passed.".to_string());
                Ok((true, format!("tool_result: project_playtest PASSED: {res}")))
            } else {
                thread.push("Project playtest found issues — repairing.".to_string());
                let detail = [errors, failed_checks]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let summary = if detail.is_empty() { res.to_string() } else { detail };
                Ok((false, summary))
            }
        } else {
            let root = self.require_project_root()?;
            let check = project_check_local(&root)?;
            if check.get("passed").and_then(|v| v.as_bool()) == Some(false) {
                let err_text = check.to_string();
                transcript.push(format!("tool project_playtest (check failed) -> {err_text}"));
                return Ok((false, err_text));
            }
            if args.get("sync_first").and_then(|v| v.as_bool()).unwrap_or(true) {
                let _ = self.project_sync_local(&root, &json!({}));
            }
            if self.studio.is_some() {
                match self.run_playtest(transcript, thread)? {
                    PlaytestOutcome::Passed(note) => Ok((true, note)),
                    PlaytestOutcome::Failed(_note, console) => Ok((false, console)),
                    PlaytestOutcome::Unreadable(_note, raw) => Ok((false, raw)),
                }
            } else {
                transcript.push("tool project_playtest (static check passed, no Studio attached)".to_string());
                thread.push("Project static checks passed.".to_string());
                Ok((true, format!("tool_result: project_playtest (static): {check}")))
            }
        }
    }
}

fn rojo_rel_to_place_path(rel: &str) -> Option<String> {
    let clean = rel.trim_start_matches("./");
    let stem = clean
        .strip_suffix(".server.luau")
        .or_else(|| clean.strip_suffix(".client.luau"))
        .or_else(|| clean.strip_suffix(".luau"))
        .or_else(|| clean.strip_suffix(".lua"))?;
    let rest = if let Some(r) = stem.strip_prefix("src/server/") {
        format!("ServerScriptService.{}", r.replace('/', "."))
    } else if let Some(r) = stem.strip_prefix("src/client/") {
        format!("StarterPlayer.StarterPlayerScripts.{}", r.replace('/', "."))
    } else if let Some(r) = stem.strip_prefix("src/shared/") {
        format!("ReplicatedStorage.{}", r.replace('/', "."))
    } else if let Some(r) = stem.strip_prefix("ui/") {
        format!("StarterGui.{}", r.replace('/', "."))
    } else {
        return None;
    };
    Some(rest)
}

fn is_hidden_or_blocked_segment(seg: &str) -> bool {
    seg == ".git"
        || seg == ".openro"
        || seg == "node_modules"
        || seg == "target"
        || seg.eq_ignore_ascii_case(".env")
        || seg.to_ascii_lowercase().starts_with(".env.")
}

pub fn project_safe_path(
    root: &std::path::Path,
    rel: &str,
    for_write: bool,
) -> Result<(String, std::path::PathBuf), String> {
    let trimmed = rel.trim().replace('\\', "/");
    let trimmed = trimmed.trim_start_matches("./");
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains('\0') {
        return Err(format!("invalid project-relative path '{rel}'"));
    }
    let mut parts = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains(':') || is_hidden_or_blocked_segment(seg) {
            return Err(format!("forbidden path segment '{seg}' in '{rel}'"));
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        return Err(format!("empty project-relative path '{rel}'"));
    }
    let norm = parts.join("/");
    if for_write {
        if norm == "openro.project.json" {
            return Err(
                "openro.project.json cannot be overwritten directly; use project_update_manifest"
                    .to_string(),
            );
        }
        if parts[0] == "build" || norm.ends_with(".lock.json") {
            return Err(format!("'{norm}' is managed by OpenRO and cannot be written directly"));
        }
    }
    let canon_root = std::fs::canonicalize(root)
        .map_err(|e| format!("project root '{}' unavailable: {e}", root.display()))?;
    let full = canon_root.join(&norm);
    let mut probe = full.as_path();
    while !probe.exists() {
        match probe.parent() {
            Some(p) => probe = p,
            None => break,
        }
    }
    let canon_probe = std::fs::canonicalize(probe)
        .map_err(|e| format!("cannot resolve path '{norm}': {e}"))?;
    if !canon_probe.starts_with(&canon_root) {
        return Err(format!("path '{rel}' escapes project root"));
    }
    Ok((norm, full))
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn collect_project_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<String>,
    cap: usize,
) {
    if out.len() >= cap {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= cap {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_or_blocked_segment(&name) || name == "build" || name == "references" {
            continue;
        }
        let path = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                collect_project_files(root, &path, out, cap);
            } else if meta.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
}

pub fn build_project_context(
    root: &std::path::Path,
    ask: &str,
    explicit_paths: &[String],
    max_chars: usize,
) -> String {
    let mut all_files = Vec::new();
    collect_project_files(root, root, &mut all_files, 240);
    if all_files.is_empty() {
        return String::new();
    }

    let mut out = String::from("[Active Project Context]\n");
    out.push_str(&format!("Project files ({} shown):\n", all_files.len().min(120)));
    for f in all_files.iter().take(120) {
        out.push_str(&format!("- {f}\n"));
    }

    let keywords: Vec<String> = ask
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect();

    let mut scored: Vec<(i32, String)> = Vec::new();
    for f in &all_files {
        let lower = f.to_ascii_lowercase();
        let is_text = lower.ends_with(".luau")
            || lower.ends_with(".lua")
            || lower.ends_with(".md")
            || lower.ends_with(".json")
            || lower.ends_with(".toml");
        if !is_text {
            continue;
        }
        let mut score = 0i32;
        if explicit_paths.iter().any(|p| p == f) {
            score += 200;
        }
        if lower == "openro.project.json" || lower == "default.project.json" {
            score += 120;
        } else if lower.starts_with("docs/") && lower.ends_with(".md") {
            score += 70;
        } else if lower == "readme.md"
            || lower == "playtest/suite.json"
            || lower == "assets/meshes/manifest.json"
            || lower == "assets.lock.json"
        {
            score += 55;
        }
        for kw in &keywords {
            if lower.contains(kw) {
                score += 35;
            }
        }
        if score > 0 {
            scored.push((score, f.clone()));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    for (_score, rel) in scored.into_iter().take(12) {
        if out.len() + 256 >= max_chars {
            break;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        let remaining = max_chars.saturating_sub(out.len()).min(2_600);
        let snippet: String = text.chars().take(remaining).collect();
        out.push_str(&format!("\n--- {rel} ---\n{snippet}"));
        if text.chars().count() > remaining {
            out.push_str("\n[...truncated — call project_read_file for full contents...]");
        }
        out.push('\n');
    }
    out
}

pub fn project_files_list(root: &std::path::Path, dir: &str) -> Result<Value, String> {
    let target = if dir.trim().is_empty() || dir.trim() == "." {
        std::fs::canonicalize(root).map_err(|e| format!("project root unavailable: {e}"))?
    } else {
        project_safe_path(root, dir, false)?.1
    };
    if !target.is_dir() {
        return Err(format!("'{dir}' is not a directory"));
    }
    let canon_root = std::fs::canonicalize(root).map_err(|e| format!("project root: {e}"))?;
    let mut entries = Vec::new();
    let rd = std::fs::read_dir(&target).map_err(|e| format!("read dir: {e}"))?;
    let mut items: Vec<_> = rd.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_or_blocked_segment(&name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        let rel = path
            .strip_prefix(&canon_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(json!({
            "path": rel,
            "type": if meta.is_dir() { "dir" } else { "file" },
            "bytes": if meta.is_file() { meta.len() } else { 0 },
        }));
    }
    Ok(json!({ "ok": true, "dir": dir, "entries": entries }))
}

pub fn project_read_file(
    root: &std::path::Path,
    rel: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value, String> {
    let (norm, full) = project_safe_path(root, rel, false)?;
    let meta = std::fs::metadata(&full).map_err(|e| format!("read '{norm}': {e}"))?;
    if !meta.is_file() {
        return Err(format!("'{norm}' is not a file"));
    }
    if meta.len() > 1024 * 1024 {
        return Err(format!("'{norm}' exceeds 1 MB text read cap ({} bytes)", meta.len()));
    }
    let bytes = std::fs::read(&full).map_err(|e| format!("read '{norm}': {e}"))?;
    let sha = sha256_hex_bytes(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| format!("'{norm}' is not valid UTF-8 text"))?;
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();
    let content = match (start_line, end_line) {
        (None, None) => text,
        _ => {
            let s = start_line.unwrap_or(1).max(1);
            let e = end_line.unwrap_or(total_lines).min(total_lines);
            if s > e || total_lines == 0 {
                String::new()
            } else {
                lines[s - 1..e].join("\n")
            }
        }
    };
    Ok(json!({
        "ok": true,
        "path": norm,
        "bytes": meta.len(),
        "total_lines": total_lines,
        "sha256": sha,
        "content": content,
    }))
}

pub fn project_write_file(
    root: &std::path::Path,
    rel: &str,
    content: &str,
    expect_sha256: Option<&str>,
) -> Result<Value, String> {
    let (norm, full) = project_safe_path(root, rel, true)?;
    if content.len() > MAX_LUAU_BYTES * 2 {
        return Err(format!("file '{norm}' exceeds 1 MB write cap"));
    }
    let created = !full.exists();
    if let Some(expected) = expect_sha256 {
        if !created {
            let cur = std::fs::read(&full).map_err(|e| format!("read existing '{norm}': {e}"))?;
            let actual = sha256_hex_bytes(&cur);
            if actual != expected {
                return Err(format!(
                    "conflict on '{norm}': expected sha256 {expected}, found {actual}"
                ));
            }
        }
    }
    let mut luau_diagnostics = Value::Null;
    if norm.ends_with(".luau") || norm.ends_with(".lua") {
        let luau_path = rojo_rel_to_place_path(&norm).unwrap_or_else(|| "ReplicatedStorage.Module".to_string());
        let report = crate::luau_check::analyze_source(&luau_path, content);
        let errs = report.errors();
        luau_diagnostics = report.to_json();
        if !errs.is_empty() {
            return Err(format!(
                "Static Luau Prover refused '{norm}': {}",
                errs.join("; ")
            ));
        }
    }
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir for '{norm}': {e}"))?;
    }
    let tmp = full.with_extension("openro.tmp");
    std::fs::write(&tmp, content.as_bytes()).map_err(|e| format!("write '{norm}': {e}"))?;
    std::fs::rename(&tmp, &full).map_err(|e| format!("commit '{norm}': {e}"))?;
    Ok(json!({
        "ok": true,
        "path": norm,
        "created": created,
        "bytes": content.len(),
        "sha256": sha256_hex_bytes(content.as_bytes()),
        "luau_diagnostics": luau_diagnostics,
    }))
}

pub fn project_edit_file(
    root: &std::path::Path,
    rel: &str,
    edits: &[Value],
    expect_sha256: Option<&str>,
) -> Result<Value, String> {
    if edits.is_empty() {
        return Err("project_edit_file requires a non-empty edits array".to_string());
    }
    let (norm, full) = project_safe_path(root, rel, true)?;
    let bytes = std::fs::read(&full).map_err(|e| format!("read '{norm}': {e}"))?;
    if let Some(expected) = expect_sha256 {
        let actual = sha256_hex_bytes(&bytes);
        if actual != expected {
            return Err(format!(
                "conflict on '{norm}': expected sha256 {expected}, found {actual}"
            ));
        }
    }
    let mut text = String::from_utf8(bytes).map_err(|_| format!("'{norm}' is not UTF-8"))?;
    for (idx, edit) in edits.iter().enumerate() {
        let old = edit
            .get("old")
            .or_else(|| edit.get("old_str"))
            .or_else(|| edit.get("old_string"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("edit #{idx} missing 'old' string"))?;
        let new = edit
            .get("new")
            .or_else(|| edit.get("new_str"))
            .or_else(|| edit.get("new_string"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("edit #{idx} missing 'new' string"))?;
        if old.is_empty() {
            return Err(format!("edit #{idx} 'old' string must not be empty"));
        }
        let matches = text.matches(old).count();
        if matches == 0 {
            return Err(format!("edit #{idx} target not found in '{norm}' (nothing written)"));
        }
        if matches > 1 {
            return Err(format!(
                "edit #{idx} target matched {matches} times in '{norm}' — provide more surrounding context (nothing written)"
            ));
        }
        text = text.replacen(old, new, 1);
    }
    let mut res = project_write_file(root, &norm, &text, None)?;
    res["edits"] = json!(edits.len());
    Ok(res)
}

pub fn project_delete_file(root: &std::path::Path, rel: &str) -> Result<Value, String> {
    let (norm, full) = project_safe_path(root, rel, true)?;
    if !full.is_file() {
        return Err(format!("file '{norm}' does not exist"));
    }
    std::fs::remove_file(&full).map_err(|e| format!("delete '{norm}': {e}"))?;
    Ok(json!({ "ok": true, "path": norm, "deleted": true }))
}

pub fn project_grep_files(
    root: &std::path::Path,
    query: &str,
    dir: &str,
    max_matches: usize,
) -> Result<Value, String> {
    if query.is_empty() {
        return Err("project_grep query must not be empty".to_string());
    }
    let start_dir = if dir.trim().is_empty() || dir.trim() == "." {
        std::fs::canonicalize(root).map_err(|e| format!("project root: {e}"))?
    } else {
        project_safe_path(root, dir, false)?.1
    };
    let canon_root = std::fs::canonicalize(root).map_err(|e| format!("project root: {e}"))?;
    let mut files = Vec::new();
    if start_dir.is_file() {
        if let Ok(rel) = start_dir.strip_prefix(&canon_root) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    } else {
        collect_project_files(&canon_root, &start_dir, &mut files, 500);
    }
    let cap = max_matches.clamp(1, 200);
    let mut matches = Vec::new();
    for rel in files {
        if matches.len() >= cap {
            break;
        }
        let Ok(meta) = std::fs::metadata(canon_root.join(&rel)) else { continue };
        if meta.len() > 512 * 1024 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(canon_root.join(&rel)) else { continue };
        for (i, line) in text.lines().enumerate() {
            if line.contains(query) {
                let preview: String = line.trim().chars().take(180).collect();
                matches.push(json!({ "path": rel, "line": i + 1, "text": preview }));
                if matches.len() >= cap {
                    break;
                }
            }
        }
    }
    Ok(json!({ "ok": true, "query": query, "count": matches.len(), "matches": matches }))
}

pub fn project_update_manifest_safe(
    root: &std::path::Path,
    patch: &Value,
) -> Result<Value, String> {
    let current = crate::projects::manifest(root)?;
    let merged = crate::projects::merge_manifest_for_actor(&current, patch, false)?;
    crate::projects::write_manifest(root, &merged)?;
    Ok(json!({ "ok": true, "manifest": merged }))
}

pub fn project_check_local(root: &std::path::Path) -> Result<Value, String> {
    crate::projects::check_project(root)
}

pub fn project_run_lune_local(root: &std::path::Path, args: &Value) -> Result<Value, String> {
    let script = args
        .get("script")
        .or_else(|| args.get("path"))
        .and_then(|v| v.as_str())
        .ok_or("project_run_lune needs args.script")?;
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .clamp(1, 60);
    let extra: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).take(32).collect())
        .unwrap_or_default();
    crate::projects::run_lune(root, script, &extra, timeout_secs)
}

pub fn project_build_local(root: &std::path::Path, args: &Value) -> Result<Value, String> {
    let engine = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let out_rel = args.get("out").and_then(|v| v.as_str());
    crate::projects::build(root, engine, out_rel)
}

/// Persistent FIFO Run Queue + Plan -> Execute Checkpoints + Resume (Gap #5).
pub mod runs {
    use super::*;
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum RunStatus {
        Queued,
        Planning,
        Running,
        Paused,
        Completed,
        Failed,
        Cancelled,
    }

    impl RunStatus {
        pub fn as_str(&self) -> &'static str {
            match self {
                RunStatus::Queued => "queued",
                RunStatus::Planning => "planning",
                RunStatus::Running => "running",
                RunStatus::Paused => "paused",
                RunStatus::Completed => "completed",
                RunStatus::Failed => "failed",
                RunStatus::Cancelled => "cancelled",
            }
        }
        pub fn parse(s: &str) -> Self {
            match s {
                "queued" => RunStatus::Queued,
                "planning" => RunStatus::Planning,
                "running" => RunStatus::Running,
                "paused" => RunStatus::Paused,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Paused,
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct PlanPhase {
        pub id: String,
        pub title: String,
        pub goal: String,
        pub files: Vec<String>,
        pub status: String,
        pub summary: Option<String>,
    }

    impl PlanPhase {
        pub fn to_json(&self) -> Value {
            json!({
                "id": self.id,
                "title": self.title,
                "goal": self.goal,
                "files": self.files,
                "status": self.status,
                "summary": self.summary,
            })
        }
        pub fn from_json(v: &Value) -> Option<Self> {
            Some(PlanPhase {
                id: v.get("id")?.as_str()?.to_string(),
                title: v.get("title")?.as_str()?.to_string(),
                goal: v.get("goal").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                files: v
                    .get("files")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                status: v.get("status").and_then(|x| x.as_str()).unwrap_or("pending").to_string(),
                summary: v.get("summary").and_then(|x| x.as_str()).map(String::from),
            })
        }
    }

    #[derive(Clone, Debug)]
    pub struct AgentRun {
        pub run_id: String,
        pub chat_id: String,
        pub project_id: Option<String>,
        pub project_root: Option<String>,
        pub ask: String,
        pub mode: String,
        pub status: RunStatus,
        pub plan: Vec<PlanPhase>,
        pub current_phase: usize,
        pub step: usize,
        pub max_steps: usize,
        pub checkpoint_seq: u64,
        pub summary: Option<String>,
        pub error: Option<String>,
        pub quota_reset_in: Option<String>,
        pub resume_hint: Option<String>,
        pub thread: Vec<String>,
        pub transcript: Vec<String>,
        pub created_at_ms: u64,
        pub updated_at_ms: u64,
    }

    impl AgentRun {
        pub fn summary_json(&self) -> Value {
            let done_phases = self.plan.iter().filter(|p| p.status == "done").count();
            json!({
                "run_id": self.run_id,
                "chat_id": self.chat_id,
                "project_id": self.project_id,
                "ask": self.ask,
                "mode": self.mode,
                "status": self.status.as_str(),
                "phases_total": self.plan.len(),
                "phases_done": done_phases,
                "current_phase": self.current_phase,
                "step": self.step,
                "max_steps": self.max_steps,
                "checkpoint_seq": self.checkpoint_seq,
                "summary": self.summary,
                "error": self.error,
                "quota_reset_in": self.quota_reset_in,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            })
        }

        pub fn detail_json(&self) -> Value {
            json!({
                "run_id": self.run_id,
                "chat_id": self.chat_id,
                "project_id": self.project_id,
                "project_root": self.project_root,
                "ask": self.ask,
                "mode": self.mode,
                "status": self.status.as_str(),
                "plan": self.plan.iter().map(PlanPhase::to_json).collect::<Vec<_>>(),
                "current_phase": self.current_phase,
                "step": self.step,
                "max_steps": self.max_steps,
                "checkpoint_seq": self.checkpoint_seq,
                "summary": self.summary,
                "error": self.error,
                "quota_reset_in": self.quota_reset_in,
                "resume_hint": self.resume_hint,
                "thread": self.thread,
                "transcript": self.transcript,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            })
        }

        pub fn from_json(v: &Value) -> Option<Self> {
            Some(AgentRun {
                run_id: v.get("run_id")?.as_str()?.to_string(),
                chat_id: v.get("chat_id").and_then(|x| x.as_str()).unwrap_or("default").to_string(),
                project_id: v.get("project_id").and_then(|x| x.as_str()).map(String::from),
                project_root: v.get("project_root").and_then(|x| x.as_str()).map(String::from),
                ask: v.get("ask")?.as_str()?.to_string(),
                mode: v.get("mode").and_then(|x| x.as_str()).unwrap_or("plan_execute").to_string(),
                status: RunStatus::parse(v.get("status").and_then(|x| x.as_str()).unwrap_or("paused")),
                plan: v
                    .get("plan")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(PlanPhase::from_json).collect())
                    .unwrap_or_default(),
                current_phase: v.get("current_phase").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                step: v.get("step").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                max_steps: v.get("max_steps").and_then(|x| x.as_u64()).unwrap_or(40) as usize,
                checkpoint_seq: v.get("checkpoint_seq").and_then(|x| x.as_u64()).unwrap_or(0),
                summary: v.get("summary").and_then(|x| x.as_str()).map(String::from),
                error: v.get("error").and_then(|x| x.as_str()).map(String::from),
                quota_reset_in: v.get("quota_reset_in").and_then(|x| x.as_str()).map(String::from),
                resume_hint: v.get("resume_hint").and_then(|x| x.as_str()).map(String::from),
                thread: v
                    .get("thread")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                transcript: v
                    .get("transcript")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                created_at_ms: v.get("created_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
                updated_at_ms: v.get("updated_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            })
        }
    }

    pub struct EnqueueRun {
        pub chat_id: String,
        pub ask: String,
        pub mode: String,
        pub project_id: Option<String>,
        pub project_root: Option<String>,
        pub max_steps: usize,
    }

    struct Inner {
        runs: HashMap<String, AgentRun>,
        order: Vec<String>,
        queue: VecDeque<String>,
        active: Option<String>,
        cancels: HashMap<String, Arc<AtomicBool>>,
        seq: u64,
    }

    pub struct RunStore {
        dir: Option<PathBuf>,
        inner: Mutex<Inner>,
        wake: Condvar,
    }

    impl RunStore {
        pub fn in_memory() -> Arc<Self> {
            Arc::new(RunStore {
                dir: None,
                inner: Mutex::new(Inner {
                    runs: HashMap::new(),
                    order: Vec::new(),
                    queue: VecDeque::new(),
                    active: None,
                    cancels: HashMap::new(),
                    seq: 0,
                }),
                wake: Condvar::new(),
            })
        }

        pub fn open(data_dir: &Path) -> Arc<Self> {
            let runs_dir = data_dir.join("runs");
            let _ = std::fs::create_dir_all(&runs_dir);
            let mut runs = HashMap::new();
            let mut list: Vec<(u64, String)> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&runs_dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let Ok(text) = std::fs::read_to_string(&path) else { continue };
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    let Some(mut run) = AgentRun::from_json(&v) else { continue };
                    if matches!(
                        run.status,
                        RunStatus::Queued | RunStatus::Planning | RunStatus::Running
                    ) {
                        run.status = RunStatus::Paused;
                        run.error = Some(
                            "Interrupted by server restart — ready to resume from last checkpoint."
                                .to_string(),
                        );
                        run.updated_at_ms = now_ms();
                        let _ = persist_run(&runs_dir, &run);
                    }
                    list.push((run.created_at_ms, run.run_id.clone()));
                    runs.insert(run.run_id.clone(), run);
                }
            }
            list.sort_by(|a, b| b.0.cmp(&a.0));
            let order = list.into_iter().map(|(_, id)| id).collect();
            Arc::new(RunStore {
                dir: Some(runs_dir),
                inner: Mutex::new(Inner {
                    runs,
                    order,
                    queue: VecDeque::new(),
                    active: None,
                    cancels: HashMap::new(),
                    seq: 0,
                }),
                wake: Condvar::new(),
            })
        }

        pub fn enqueue(&self, req: EnqueueRun) -> AgentRun {
            let mut g = self.inner.lock().unwrap();
            g.seq += 1;
            let now = now_ms();
            let run_id = format!("run-{now}-{}", g.seq);
            let run = AgentRun {
                run_id: run_id.clone(),
                chat_id: req.chat_id,
                project_id: req.project_id,
                project_root: req.project_root,
                ask: req.ask,
                mode: if req.mode.trim().is_empty() {
                    "plan_execute".to_string()
                } else {
                    req.mode
                },
                status: RunStatus::Queued,
                plan: Vec::new(),
                current_phase: 0,
                step: 0,
                max_steps: req.max_steps.clamp(4, 120),
                checkpoint_seq: 1,
                summary: None,
                error: None,
                quota_reset_in: None,
                resume_hint: None,
                thread: Vec::new(),
                transcript: Vec::new(),
                created_at_ms: now,
                updated_at_ms: now,
            };
            if let Some(dir) = &self.dir {
                let _ = persist_run(dir, &run);
            }
            g.runs.insert(run_id.clone(), run.clone());
            g.order.insert(0, run_id.clone());
            g.queue.push_back(run_id.clone());
            g.cancels.insert(run_id, Arc::new(AtomicBool::new(false)));
            self.wake.notify_one();
            run
        }

        pub fn list_json(&self) -> Value {
            let g = self.inner.lock().unwrap();
            let runs: Vec<Value> = g
                .order
                .iter()
                .filter_map(|id| g.runs.get(id).map(AgentRun::summary_json))
                .collect();
            json!({
                "ok": true,
                "active_run_id": g.active,
                "queued": g.queue.len(),
                "runs": runs,
            })
        }

        pub fn get(&self, run_id: &str) -> Option<AgentRun> {
            self.inner.lock().unwrap().runs.get(run_id).cloned()
        }

        pub fn resume(&self, run_id: &str, hint: Option<String>) -> Result<AgentRun, String> {
            let mut g = self.inner.lock().unwrap();
            let run = g
                .runs
                .get_mut(run_id)
                .ok_or_else(|| format!("unknown run '{run_id}'"))?;
            if matches!(
                run.status,
                RunStatus::Queued | RunStatus::Planning | RunStatus::Running
            ) {
                return Err(format!("run '{run_id}' is already {}", run.status.as_str()));
            }
            if run.status == RunStatus::Completed {
                return Err(format!("run '{run_id}' is already completed"));
            }
            run.status = RunStatus::Queued;
            run.error = None;
            run.quota_reset_in = None;
            run.resume_hint = hint.filter(|h| !h.trim().is_empty());
            run.checkpoint_seq += 1;
            run.updated_at_ms = now_ms();
            let cloned = run.clone();
            if let Some(dir) = &self.dir {
                let _ = persist_run(dir, &cloned);
            }
            g.cancels
                .insert(run_id.to_string(), Arc::new(AtomicBool::new(false)));
            g.queue.push_back(run_id.to_string());
            self.wake.notify_one();
            Ok(cloned)
        }

        pub fn cancel(&self, run_id: &str) -> Result<AgentRun, String> {
            let mut g = self.inner.lock().unwrap();
            if !g.runs.contains_key(run_id) {
                return Err(format!("unknown run '{run_id}'"));
            }
            if let Some(flag) = g.cancels.get(run_id) {
                flag.store(true, Ordering::SeqCst);
            }
            g.queue.retain(|id| id != run_id);
            let cloned = {
                let run = g.runs.get_mut(run_id).unwrap();
                if matches!(
                    run.status,
                    RunStatus::Queued | RunStatus::Planning | RunStatus::Running | RunStatus::Paused
                ) {
                    run.status = RunStatus::Cancelled;
                    run.error = Some("Cancelled by user.".to_string());
                    run.checkpoint_seq += 1;
                    run.updated_at_ms = now_ms();
                }
                run.clone()
            };
            if let Some(dir) = &self.dir {
                let _ = persist_run(dir, &cloned);
            }
            Ok(cloned)
        }

        pub fn pop_next(&self) -> (String, Arc<AtomicBool>) {
            let mut g = self.inner.lock().unwrap();
            loop {
                if let Some(id) = g.queue.pop_front() {
                    if let Some(run) = g.runs.get(&id) {
                        if run.status == RunStatus::Queued {
                            g.active = Some(id.clone());
                            let flag = g
                                .cancels
                                .entry(id.clone())
                                .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                                .clone();
                            return (id, flag);
                        }
                    }
                    continue;
                }
                g = self.wake.wait(g).unwrap();
            }
        }

        pub fn update<F: FnOnce(&mut AgentRun)>(&self, run_id: &str, f: F) {
            let mut g = self.inner.lock().unwrap();
            if let Some(run) = g.runs.get_mut(run_id) {
                f(run);
                run.checkpoint_seq += 1;
                run.updated_at_ms = now_ms();
                if let Some(dir) = &self.dir {
                    let _ = persist_run(dir, run);
                }
            }
        }

        pub fn finish_active(&self, run_id: &str) {
            let mut g = self.inner.lock().unwrap();
            if g.active.as_deref() == Some(run_id) {
                g.active = None;
            }
        }

        pub fn execute_run(
            store: &Arc<RunStore>,
            run_id: &str,
            cancel: Arc<AtomicBool>,
            agent: &mut Agent,
        ) {
            let Some(initial) = store.get(run_id) else {
                store.finish_active(run_id);
                return;
            };
            let prev_root = agent.project_root.clone();
            let prev_id = agent.project_id.clone();
            if let Some(pr) = &initial.project_root {
                agent.set_project(Some(PathBuf::from(pr)), initial.project_id.clone());
            }
            agent.set_cancel_flag(Some(Arc::clone(&cancel)));
            agent.set_max_steps_override(Some(initial.max_steps));

            let store_step = Arc::clone(store);
            let rid_step = run_id.to_string();
            agent.set_step_sink(Some(Arc::new(move |step_n, label| {
                store_step.update(&rid_step, |r| {
                    r.step = step_n;
                    r.transcript.push(format!("checkpoint step {step_n}: {label}"));
                });
            })));

            let mut plan = initial.plan.clone();
            if plan.is_empty() {
                if initial.mode == "plan_execute" {
                    store.update(run_id, |r| {
                        r.status = RunStatus::Planning;
                        r.thread.push("OpenRO: Planning phases for this run...".to_string());
                    });
                    match generate_plan_phases(agent, &initial.ask) {
                        Ok(phases) if !phases.is_empty() => plan = phases,
                        Ok(_) => {
                            plan = vec![PlanPhase {
                                id: "phase-1".to_string(),
                                title: "Execute request".to_string(),
                                goal: initial.ask.clone(),
                                files: Vec::new(),
                                status: "pending".to_string(),
                                summary: None,
                            }];
                        }
                        Err(e) => {
                            let quota = crate::model::agy_last_issue()
                                .filter(|i| i.kind == "quota")
                                .and_then(|i| i.resets_in);
                            let status = if quota.is_some() {
                                RunStatus::Paused
                            } else {
                                RunStatus::Failed
                            };
                            store.update(run_id, |r| {
                                r.status = status;
                                r.error = Some(e.clone());
                                r.quota_reset_in = quota;
                            });
                            restore_agent(agent, prev_root, prev_id);
                            store.finish_active(run_id);
                            return;
                        }
                    }
                } else {
                    plan = vec![PlanPhase {
                        id: "phase-1".to_string(),
                        title: "Execute request".to_string(),
                        goal: initial.ask.clone(),
                        files: Vec::new(),
                        status: "pending".to_string(),
                        summary: None,
                    }];
                }
                store.update(run_id, |r| {
                    r.plan = plan.clone();
                    r.status = RunStatus::Running;
                });
            } else {
                store.update(run_id, |r| {
                    r.status = RunStatus::Running;
                    r.thread.push(format!(
                        "OpenRO: Resuming run at phase {}/{}.",
                        r.current_phase + 1,
                        r.plan.len()
                    ));
                });
            }

            let start_idx = initial.current_phase;
            let mut completed_notes: Vec<String> = plan
                .iter()
                .take(start_idx)
                .filter_map(|p| p.summary.as_ref().map(|s| format!("- {}: {s}", p.title)))
                .collect();

            for idx in start_idx..plan.len() {
                if cancel.load(Ordering::SeqCst) {
                    store.update(run_id, |r| {
                        r.status = RunStatus::Cancelled;
                        r.current_phase = idx;
                    });
                    restore_agent(agent, prev_root, prev_id);
                    store.finish_active(run_id);
                    return;
                }
                let phase = plan[idx].clone();
                if phase.status == "done" {
                    continue;
                }
                store.update(run_id, |r| {
                    r.current_phase = idx;
                    if let Some(p) = r.plan.get_mut(idx) {
                        p.status = "running".to_string();
                    }
                });

                let mut phase_prompt = if plan.len() == 1 {
                    initial.ask.clone()
                } else {
                    format!(
                        "Overall goal: {}\n\nExecute Phase {}/{} — {}: {}",
                        initial.ask,
                        idx + 1,
                        plan.len(),
                        phase.title,
                        phase.goal
                    )
                };
                if !completed_notes.is_empty() {
                    phase_prompt.push_str("\n\nCompleted earlier phases:\n");
                    phase_prompt.push_str(&completed_notes.join("\n"));
                }
                if let Some(hint) = &initial.resume_hint {
                    phase_prompt.push_str(&format!("\n\nResume guidance: {hint}"));
                }

                let opts = AskOptions {
                    mode: AskMode::parse(&initial.mode),
                    reasoning: "high".to_string(),
                    context_paths: phase.files.clone(),
                    review_diffs: false,
                };

                match agent.run_ask_with_options(&initial.chat_id, &phase_prompt, opts) {
                    Ok(outcome) => {
                        completed_notes.push(format!("- {}: {}", phase.title, outcome.summary));
                        store.update(run_id, |r| {
                            if let Some(p) = r.plan.get_mut(idx) {
                                p.status = "done".to_string();
                                p.summary = Some(outcome.summary.clone());
                            }
                            r.current_phase = idx + 1;
                            r.thread.extend(outcome.thread);
                            r.transcript.extend(outcome.transcript);
                        });
                    }
                    Err(fail) => {
                        let cancelled = cancel.load(Ordering::SeqCst)
                            || fail.message.to_ascii_lowercase().contains("cancelled");
                        let quota_reset = crate::model::agy_last_issue()
                            .filter(|i| i.kind == "quota")
                            .and_then(|i| i.resets_in);
                        let is_quota = quota_reset.is_some()
                            || fail.message.to_ascii_lowercase().contains("out of quota");
                        let next_status = if cancelled {
                            RunStatus::Cancelled
                        } else if is_quota {
                            RunStatus::Paused
                        } else {
                            RunStatus::Failed
                        };
                        store.update(run_id, |r| {
                            if let Some(p) = r.plan.get_mut(idx) {
                                p.status = if is_quota || cancelled {
                                    "pending".to_string()
                                } else {
                                    "failed".to_string()
                                };
                            }
                            r.current_phase = idx;
                            r.status = next_status;
                            r.error = Some(fail.message.clone());
                            r.quota_reset_in = quota_reset;
                            r.thread.extend(fail.thread);
                            r.transcript.extend(fail.transcript);
                        });
                        restore_agent(agent, prev_root, prev_id);
                        store.finish_active(run_id);
                        return;
                    }
                }
            }

            let final_summary = if completed_notes.is_empty() {
                "Run completed.".to_string()
            } else {
                completed_notes.join("\n")
            };
            store.update(run_id, |r| {
                r.status = RunStatus::Completed;
                r.summary = Some(final_summary);
                r.error = None;
                r.quota_reset_in = None;
            });
            restore_agent(agent, prev_root, prev_id);
            store.finish_active(run_id);
        }
    }

    fn restore_agent(
        agent: &mut Agent,
        prev_root: Option<PathBuf>,
        prev_id: Option<String>,
    ) {
        agent.set_project(prev_root, prev_id);
        agent.set_cancel_flag(None);
        agent.set_step_sink(None);
        agent.set_max_steps_override(None);
    }

    fn persist_run(dir: &Path, run: &AgentRun) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create runs dir: {e}"))?;
        let path = dir.join(format!("{}.json", run.run_id));
        let tmp = dir.join(format!("{}.json.tmp", run.run_id));
        let body = serde_json::to_string_pretty(&run.detail_json())
            .map_err(|e| format!("serialize run: {e}"))?;
        std::fs::write(&tmp, body.as_bytes()).map_err(|e| format!("write run: {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("commit run: {e}"))?;
        Ok(())
    }

    fn generate_plan_phases(agent: &Agent, ask: &str) -> Result<Vec<PlanPhase>, String> {
        let mut user_msg = format!(
            "Break the request below into 1 to 6 small, concrete, sequential execution phases for a Roblox project.\n\
             Reply with ONLY one JSON object: {{\"plan\": [{{\"id\": \"phase-1\", \"title\": \"...\", \"goal\": \"...\", \"files\": [\"src/...\"]}}]}}.\n\nRequest: {ask}"
        );
        if let Some(root) = agent.effective_project_root() {
            let pctx = build_project_context(&root, ask, &[], 6_000);
            if !pctx.is_empty() {
                user_msg.push_str("\n\n");
                user_msg.push_str(&pctx);
            }
        }
        let msgs = vec![
            json!({
                "role": "system",
                "content": "You are OpenRO's build planner. Output ONLY valid JSON matching {\"plan\":[{\"id\":\"phase-1\",\"title\":\"...\",\"goal\":\"...\",\"files\":[...]}]}."
            }),
            json!({ "role": "user", "content": user_msg }),
        ];
        let raw = agent.model.chat(&msgs)?;
        let parsed = parse_step(&raw)?;
        let mut phases = Vec::new();
        if let Some(arr) = parsed.get("plan").and_then(|v| v.as_array()) {
            for (i, item) in arr.iter().take(8).enumerate() {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Build step")
                    .trim()
                    .to_string();
                let goal = item
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&title)
                    .trim()
                    .to_string();
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("phase-{}", i + 1));
                let files = item
                    .get("files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                phases.push(PlanPhase {
                    id,
                    title,
                    goal,
                    files,
                    status: "pending".to_string(),
                    summary: None,
                });
            }
        }
        Ok(phases)
    }
}
