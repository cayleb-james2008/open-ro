# OpenRO (`v0.10.0` — RoPilot 3-Pane Roblox Studio AI IDE + Custom Luau + Unified Diff Proposals + Live Studio Companion Plugin)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

OpenRO is a **3-Pane Roblox Studio AI IDE**: inspect your auto-synced game hierarchy and Rojo projects on the left, work inside a full-bleed **Embedded Roblox Studio** (`12 FPS` X11 `x11grab` / MCP MJPEG viewport), **Studio 3D IDE**, **Code & Diff Workbench** (unified `+`/`-` hunks with per-file checkboxes and live Luau diagnostics), **Teamwork**, or **Ship** in the center, and steer a docked **OpenRO Copilot** (`Agent | Plan | Ask` modes, `@Context` live selection chips, `Review diffs`, `Low | Med | High` reasoning, custom freeform `write_luau` + `multi_edit`, 28 built-in Roblox game systems, all 13 official Roblox Studio MCP tools, and the live **OpenRO Companion Plugin**) on the right.

## Architecture

- **Single-binary Rust desktop app & server (`openro-rs`):** Opens in a dedicated chromeless desktop window (`src/desktop.rs` — Linux `chromium --app=`, Windows `msedge.exe --app=`, macOS `open`; pass `--no-open` / `OPENRO_NO_OPEN=1` for headless mode) and serves the embedded single-file React + Tailwind 4 **RoPilot Studio IDE** (`assets/index.html`, with JS, CSS, and self-hosted `Geist` + `Geist Mono` `.woff2` fonts inlined via `include_str!`) over `std::net::TcpListener`.
  - **Left Game Explorer & Sessions Rail (`252px` desktop, mobile tab `<768px`):** Auto-syncs live Roblox Studio parts & Luau scripts automatically on attach/launch, groups `Workspace`, `ServerScriptService`, `ReplicatedStorage`, `StarterGui`, `StarterPlayer`, `Lighting`, and `SoundService` with quick filter, inline script/instance inspection, `Chats` switcher, `Tool Library` (`3/8`), **Live Studio Companion Plugin** status, **Settings (`Model, Studio & Appearance`)** `MorphingDialog` (exclusively housing the Light/Dark `SegmentedControl`), and release version link.
  - **Center Full-Bleed Studio Stage & Bottom Drawer (`EmbeddedStudioPanel.tsx`, `StudioViewport3D.tsx`, `CodeDiffWorkbench.tsx`, `StudioBottomDrawer.tsx`, `TeamworkPanel.tsx`, `ShipPanel.tsx`):** Single-row top tab bar switching between **`Embedded Studio`** (user-triggered Linux `Xvfb :99` + Vinegar + 12 FPS `ffmpeg x11grab` MJPEG pump or native Windows Studio launch, direct pointer/keyboard pass-through, Solo Play/Run Server/Stop), **`Studio 3D`** (zero-dependency Canvas 3D Studio IDE with `+ Add Part`, `Duplicate`, `Delete`, `Redo`, transform gizmos, live SSE Agent Builder Drone, `Replay Build`), **`Code & Diff`** (unified green `+` / red `-` hunks, per-file checkboxes, full-width Luau editor + Static Luau Prover diagnostics, `Accept & Apply to Studio` / `Reject`), **`Teamwork`** (multi-agent board, roster, tasks, file claims, and traces), and **`Ship`** (Publish & Monetize, Assets, Playtests) — paired with a collapsible Bottom Studio Drawer (`Output Console`, `Agent Trace`, **`Studio MCP Tools (13)`**, `Properties`) and 1-click `Playtest`, `.rbxlx`, `Rojo .zip`, and `Import`.
  - **Right Docked RoPilot Copilot (`380px`, collapsible, `CopilotComposer.tsx`, `Thread.tsx`, `StudioPluginDialog.tsx`):** Active chat feed with inline **`ProposalDiffCard`** (`Inspect Diff`, `Accept & Apply`, `Reject`), clickable `+N / -M` trace diff pills, on-demand **`Starters`** dialog (`BlueprintWorkbench` + 28 Game Systems), docked composer with `@Context` chips (including live Studio Selection), **`Agent | Plan | Ask`** modes, **`Review diffs`** toggle, **`Low | Med | High`** reasoning selector, inline `Undo`, and **`StudioPluginDialog`** for 1-click `OpenRoCompanion.lua` install/download.
  - **Built-In Tool & Skill Library + Live Roblox Market Taste Craft Dossier (`src/library.rs`, `src/market_taste.rs`, `src/audio_synth.rs`, `ToolLibraryDialog.tsx`):** Ships with **3 universal pre-installed default packs** (`roblox-studio-core`, `blender-3d-pipeline`, `roblox-market-taste`) + **5 one-click installable packs** (`image-gen-studio` carrying the Stylized Fan-Art & Title-Epithet Creative Knowledge point when installed, `model-3d-gen`, `video-gen-studio`, `audio-sfx-gen`, `rojo-open-cloud` = `8` total), dynamic Agent system-prompt injection, headless **Blender 5.2.2 LTS** procedural 3D `.obj` + PNG preview generation, zero-dependency pure-Rust 44.1 kHz PCM16 `.wav` SFX synthesis, and the **8-Dimension Production Taste Dossier** defaulting to **`top-trending`** across all 8 profitable Roblox genres with live Explore chart refresh (`apis.roblox.com/explore-api/v1/get-sorts`) and on-demand Roblox Place/Universe URL research (`TASTE-DOSSIER.md`).
- **Custom Luau + Whole-Game Agent & Transparent Studio MCP (`src/agent.rs`, `src/mcp.rs`, `src/luau_check.rs`, `src/studio_plugin.rs`):** Supports custom freeform Luau via `write_luau` and `multi_edit` (pre-write Static Luau Prover gated, `512 KiB` cap, Undo checkpointed) alongside all 28 sandboxed game-system generators, transparent translation onto official Roblox Studio MCP (`execute_luau`, `start_stop_play`, `get_console_output`, `screen_capture`), and live bidirectional sync with `OpenRoCompanion.lua` over token-authenticated loopback long-poll (`X-OpenRo-Plugin-Token`, `0600`).
- **Model route (`src/model.rs`):** OpenAI-compatible `POST {base}/chat/completions`. Loopback gateways (`127.0.0.1`, `localhost`, `[::1]`) work without an API key, and when `OPENRO_MODEL_BASE_URL` and `OPENRO_MODEL_API_KEY` are unset (and `OPENRO_NO_AUTO_MODEL` is not `1`), `openro-rs` auto-probes running local gateways (`127.0.0.1:11434/v1`, `8789/v1`, `8790/v1`). Remote endpoints still require an API key and fail closed when unconfigured. Settings persist live to `data_dir/config.json` (`0600`) via loopback-guarded `GET/POST /api/config`.
- **Real Local Place Engine, Static Luau Prover & Studio MCP (`src/place.rs`, `src/luau_check.rs`, `src/mcp.rs`):** Connects to `OPENRO_MCP_COMMAND` when explicitly set, passively detects a running Studio MCP bridge, accepts live `OpenRoCompanion.lua` plugin sessions, or launches Embedded Studio on user click via `POST /api/studio/embed/launch` (never auto-launching Vinegar/Studio on boot). Otherwise runs on its built-in persistent Local Place Engine (`data_dir/place.json` + atomic `data_dir/game.rbxlx` `<roblox version="4">` XML), bidirectional `.rbxlx` import/export, and full static Luau + cross-reference verification.
- **28 Verified Roblox Game Systems (`src/skills.rs`):** Sandboxed embedded game-system generators produce server-authoritative Luau (`round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`, `currency`, `shop`, `game_gui`, `checkpoints`, `soundtrack`, `jump_pad`, `moving_platform`, `datastore_save`, `npc_patrol`, `portal_teleporter`, `combat_sword`, `inventory_backpack`, `quest_tracker`, `vehicle_spawner`, `weather_cycle`, `tycoon_dropper`, `admin_spectate`, `card_gacha_roller`, `deck_builder`, `card_battle_arena`, `tower_progression`, `potions_weather`, and `card_gallery_world`).
- **Standalone Example Project Built With OpenRO (`examples/untitled-anime-card-game/`):** Complete sunlit **Untitled Anime Card Game** Roblox Studio place (`UntitledAnimeCardGame.rbxlx`, `TASTE-DOSSIER.md`, 215-Part sunlit floating archipelago, 4 Blender 5.2.2 props, 5 PCM16 `.wav` SFX cues, 8 character portraits, **Chain Overdrive**, **Fusion Weather Storms**, and **Living 3D Lead-Card Guardian Aura**) built with OpenRO and verified live in headless Roblox Studio (`0` Luau runtime errors).
- **Playtest gate, repair loop, versioned snapshots & checkpointed Undo:**
  - Every ask records an identity-checked change set in `changes.json` (for one-click `Undo`) and a versioned snapshot with instance/script diff in `snapshots.json`.
  - `done` is refused until a playtest passes after the latest change (`proof.json` persists verdicts across restarts).
  - Failed playtests feed Studio console errors back into the model for up to 3 automatic repair attempts before reporting a plain failure.

## Persistence (`OPENRO_DATA_DIR`, default `openro-data`)

- `place.json` & `game.rbxlx`: live local Roblox place tree and genuine Roblox Studio XML file.
- `snapshots.json`: bounded versioned snapshot history with instance/script diffs for restore & `.rbxlx` export.
- `config.json`: loopback-managed Model provider & Studio backend settings (`0600` on Unix).
- `chats.json`: chat sessions, titles, thread lines, and active chat id.
- `changes.json`: per-chat change-set stacks for `Undo`.
- `proof.json`: per-chat playtest verdicts and reverted skill history.
- `ideas.json`: per-chat built skills and pitched blueprints.

Corrupt state files are preserved aside as `<name>.json.bak` (`bak2`, ...) and surfaced in `GET /api/status` (`store_recovered`) and a dismissible UI banner.

## HTTP & SSE API

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/` | Embedded single-file Studio Workbench UI |
| `GET` | `/api/version` | `{ "name": "openro-rs", "version": "0.10.0" }` |
| `GET` | `/api/status` | `studio_connected`, `model_configured`, `workspace_mode`, `studio_mode`, `ropilot_v0100`, `proposals_api`, `plugin_bridge`, `studio_stream`, `studio_embed`, `studio_mjpeg`, `library_api`, `market_taste_api`, `active_niche`, `embedded_running`, `studio_tools`, `ide_api`, `ide_v080`, `rojo_export`, `can_undo`, `can_redo`, `model_id`, `sse_build_stream`, `skills_count`, `game_name`, `version`, `store_recovered` |
| `GET` | `/api/proposals` | List staged shadow-build unified diff proposals awaiting review (`PendingProposal[]`) |
| `POST` | `/api/proposals/:id/apply` | Loopback-only atomic apply of reviewed proposal (`{ selected_paths? }`) with Undo checkpoint, Snapshot & Playtest proof |
| `POST` | `/api/proposals/:id/reject` | Loopback-only discard of a staged proposal without mutating Studio |
| `GET` | `/api/studio/plugin/status` | Live Companion Plugin connection state, selection paths, and Studio plugins folder availability (never echoes token) |
| `GET` | `/api/studio/plugin/download` | Download `OpenRoCompanion.lua` (`HttpService` + `ChangeHistoryService` + `Selection` + `LogService`) |
| `POST` | `/api/studio/plugin/install` | Loopback-only 1-click install into canonical OS `Roblox/Plugins/OpenRoCompanion.lua` |
| `POST` | `/api/studio/plugin/{handshake,poll,result}` | Token-authenticated (`X-OpenRo-Plugin-Token`, `0600`) loopback long-poll bridge for live Roblox Studio plugin sessions |
| `GET` | `/api/studio/embed/status` | Embedded Roblox Studio host detection, lifecycle state, active display, play mode & official tool count |
| `POST` | `/api/studio/embed/launch` | Loopback-only user-triggered launch & embed of Roblox Studio (`Xvfb :99` + Vinegar or Windows Studio / MCP command) |
| `POST` | `/api/studio/embed/stop` | Loopback-only clean stop of Embedded Roblox Studio returning to persistent Local Place Engine |
| `GET` | `/api/studio/stream.mjpg` | Live multipart MJPEG/MPNG viewport stream (`multipart/x-mixed-replace; boundary=openro-frame`, `?once=1`/`?max_frames=N`) |
| `POST` | `/api/studio/input/mouse` | Loopback-only mouse pointer/click/drag/wheel pass-through directly into Studio (`user_mouse_input`) |
| `POST` | `/api/studio/input/keyboard` | Loopback-only keyboard press/text pass-through directly into Studio (`user_keyboard_input`) |
| `POST` | `/api/studio/tools/:tool` | Loopback-only direct invocation of any of the 13 official Roblox Studio MCP tools |
| `GET` | `/api/library` | List all 8 Tool & Skill Library packs (`3` universal pre-installed defaults + `5` installable), Blender detection & active tools |
| `POST` | `/api/library/{install,uninstall,configure,test}` | Loopback-only pack management (`0600` masked secrets in `library.json`, live Agent prompt injection) |
| `POST` | `/api/tools/blender/generate` | Loopback-only headless Blender 5.2.2 LTS procedural `.obj`/`.mtl` + PNG preview generator (`GET /api/assets/meshes/:file`) |
| `POST` | `/api/tools/audio/generate` | Loopback-only pure-Rust 44.1 kHz PCM16 `.wav` SFX synthesizer (`GET /api/assets/audio/:file`) |
| `GET` | `/api/market-taste` | Live Roblox Market Taste & 8-Dimension Craft Dossier (`?niche=...`, zero unsolicited network on boot) |
| `POST` | `/api/market-taste/{niche,refresh,research}` | Loopback-only active niche switch, live Roblox Explore chart refresh, and on-demand Roblox Place/Universe URL Taste Dossier research |
| `GET` | `/api/studio/stream` | Live Server-Sent Events stream (`status`, `studio_action`, `studio_frame`) with `Last-Event-ID`/`?since` replay, 15s keepalive & 32-client cap |
| `GET` | `/api/studio/actions` | Bounded 500-event ring snapshot of granular agent Studio mutations (`?since=<seq>`) + live frame metadata |
| `GET` | `/api/studio/frame` | Latest captured live Roblox Studio MCP viewport PNG/JPEG (`X-Frame-Seq`, `Cache-Control: no-store`, or honest `404` when uncaptured) |
| `POST` | `/api/studio/capture` | Capture a live viewport PNG frame from Roblox Studio MCP (`screen_capture`) or return honest `503` on Local Place Engine |
| `POST` | `/api/studio/playtest` | Run live MCP `start_stop_play` + `get_console_output` or Local Place playtest, streaming console logs & verdict over SSE |
| `POST` | `/api/studio/mcp/connect` | Loopback-only dynamic attach to a running Roblox Studio MCP bridge (auto-captures initial live frame; never launches Vinegar) |
| `POST` | `/api/studio/mcp/disconnect` | Loopback-only detach from live Studio MCP bridge back to persistent Local Place Engine |
| `GET` | `/api/art-knowledge` | Built-in Anime Fan-Art & Creative Homage Knowledge Point (principles, style rules, 8 homage archetypes & prompt templates) |
| `GET` | `/api/assets` | List stored card/game artwork in `data_dir/game-assets/` with metadata & `/api/assets/:file` URLs |
| `GET` | `/api/assets/manifest` | Export portable `openro-asset-manifest-v1` JSON with Roblox `Decal`/`ImageLabel` binding paths |
| `GET/DELETE` | `/api/assets/:file` | Serve verified PNG/JPEG/WebP artwork (strict filename/symlink jail) or remove an asset |
| `POST` | `/api/assets/upload` | Upload PNG/JPEG/WebP artwork (JSON base64 or raw binary, magic-byte checked) & bind into `Workspace.CardGallery` |
| `POST` | `/api/assets/generate` | Generate card art via Google Gemini (Nano Banana) or OpenAI Image API (honest `503` setup hint when unconfigured) |
| `GET` | `/api/build/stream` | Live Server-Sent Events (`text/event-stream`) build phase stream with `Last-Event-ID` replay & 15s keepalive |
| `GET` | `/api/build/events` | JSON polling fallback for recent build events (`?since=<seq>`) |
| `GET` | `/api/skills` | Catalog of all 28 built-in Roblox game systems |
| `POST` | `/api/skills/build` | Generate, write, and playtest a built-in Roblox game system (`{"skill": "..."}`) |
| `GET/POST` | `/api/luau/check` | Static Luau syntax, loop-yield, DataStore `pcall`, RemoteEvent & Place cross-reference prover |
| `GET` | `/api/snapshots` | Versioned Place snapshots & latest instance/script build diff |
| `POST` | `/api/snapshots` | Record a manual Place snapshot (`{"label": "..."}`) |
| `POST` | `/api/snapshots/:id/restore` | Restore a historical snapshot into the active Place |
| `GET` | `/api/snapshots/:id/rbxlx` | Export a historical snapshot as genuine `<roblox version="4">` `.rbxlx` XML |
| `GET` | `/api/place/diff` | Latest build diff (`added`/`modified`/`removed` instances & scripts) |
| `POST` | `/api/place/instance` | Create, edit properties on, or delete an Instance in the active Place |
| `POST` | `/api/place/create` | Insert a typed 3D part preset (`block`, `sphere`, `cylinder`, `spawn`, `jump_pad`, `kill_brick`, `card_billboard`) + companion Luau script |
| `POST` | `/api/place/duplicate` | Clone an existing Instance (and descendants) with `+[4, 0, 4]` stud offset |
| `POST` | `/api/place/delete` | Delete an Instance (and descendants) or Luau script cleanly |
| `POST` | `/api/place/script` | Save live-edited Luau source and return immediate Static Luau Prover diagnostics |
| `POST` | `/api/import/rbxlx` | Import a Roblox Studio `.rbxlx` XML file (JSON `{"xml"}` or raw `application/xml`, up to 16 MB) |
| `GET` | `/api/export/rbxlx` | Downloads genuine Roblox Studio XML (`<roblox version="4">`) for the current place |
| `GET` | `/api/export/bundle` | Downloads a turnkey Roblox Studio & Rojo 7 Ship Pack `.zip` (`game.rbxlx`, `default.project.json`, `src/**/*.luau`, `assets/*`, audit report, checklist) |
| `GET` | `/api/config` | Loopback-only Model & Studio runtime config + detected local gateways |
| `POST` | `/api/config` | Loopback-only live Model & Studio settings update (saved to `config.json`) |
| `GET` | `/api/chats` | `chats`, `active`, `can_undo`, and `can_redo` |
| `POST` | `/api/chats` | Create and select a new chat (`201 Created`) |
| `POST` | `/api/chats/select` | Switch active chat (`{"id": "..."}`) |
| `GET` | `/api/thread` | Thread lines for the active chat |
| `POST` | `/api/chat` | Run a Studio ask (`{"message": "...", "mode": "agent|plan|ask", "reasoning": "low|med|high", "context_paths": [...], "review_diffs": false}`) |
| `GET` | `/api/place-tree` | Current Studio / Local Place `scripts` and `instances` map |
| `POST` | `/api/ideas` | Contextual blueprints (`ideas`, `built_titles`, `pitched_new`) |
| `POST` | `/api/ideas/build` | Execute a built-in blueprint plan (`{"id": "..."}`) |
| `POST` | `/api/revert` | Revert the latest change set in the active chat (populates Redo stack) |
| `POST` | `/api/redo` | Re-apply the most recently undone change set in the active chat |
| `GET` | `/api/update` | Self-updater snapshot (no network call) |
| `POST` | `/api/update/check` | Check latest GitHub Release now (loopback only) |
| `POST` | `/api/update/apply` | Download, verify `SHA256SUMS`, `--version` probe, and stage binary (loopback only) |
| `POST` | `/api/update/restart` | Restart into staged update when idle (loopback only) |

## Build & Run

```sh
# 1. Build single-file frontend bundle into assets/index.html (only needed when editing frontend/)
cd frontend && npm run build && cd ..
touch src/server.rs

# 2. Build Rust binary + mock Studio fixture
cargo build --bins

# 3. Run locally (opens chromeless desktop app at http://127.0.0.1:8077; pass --no-open for headless mode)
./target/release/openro-rs

# Optional: build the self-contained browser web app (Embedded 3D Studio Viewport + 28 Luau Compilers + Market Taste + .rbxlx/.zip Export)
cd frontend && npm run build:web
```

## Verification

```sh
# Rust unit + integration + RoPilot v0.10.0, Studio embed, library, market-taste & viewer suite (179 tests)
cargo test

# Full Playwright UI & worst-case replay suite (1440px, 768px, 390px, 320px in Light & Dark)
cd frontend && npm run check:ui
```
