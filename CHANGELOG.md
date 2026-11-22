# Changelog

Each release is tagged `vMAJOR.MINOR.PATCH` and published on GitHub Releases with
binaries and a `SHA256SUMS` file. The running app shows its version and can update
itself from the newest release (see README, "Versions and updates").

## Unreleased (0.11.0)

- **Rojo Projects API, Chunked Studio Sync, Playtest Runner & Project Git Hooks (`src/projects.rs`, `src/project_api.rs`, `src/studio_sync.rs`, `src/playtest.rs`, `docs/API-V011.md`):**
  - Added full Rojo project management (`/api/projects*`, manifest with server-enforced human `priceApproved` gate, path-safe file read/write/edit/delete, `.rbxlx` builds via `rojo` or built-in writer, `/check` for `selene`/`stylua`/Luau, `/run-lune` runner, and scoped `/git/{status,log,commit,revert}`).
  - Added chunked one-way Studio push-sync (`/sync/diff`, `/sync` with structural services before `Workspace`, >30k script splitting, post-apply verification) and automated Playtest Runner (`/playtest`, `/playtests`, JSON/HTML reports, PNG screenshot gallery, stale console-error diffing).
  - Fixed Auto-mode Studio detection (`McpClient::has_live_studio` falls back to `local_place` when no Studio place is open), upright `PartType.Cylinder` orientation & `<Content><url>` XML export, mesh/audio bundle inclusion, `luau_check` cylinder/VFX warnings, `--data-dir` CLI flag, Vinegar first-launch plugin auto-install, and `/api/studio/launch-and-continue`.
- **Projects, Ship & Teamwork Studio IDE UI + Playable Stage Removal (`frontend/src/app/ship/`, `frontend/src/app/teamwork/`, `frontend/src/App.tsx`, `SettingsDialog.tsx`):**
  - Removed `Playable Stage` (`StagePreview.tsx`) completely across UI and docs.
  - Added Left-dock **Projects** rail (create/open/build/diff/sync), **Ship** workspace (`Publish & Monetize` with human price-approval gate & inline Universe/Place ID dialog, `Assets` queue/lock/moderation, `Playtests` report & screenshot viewer), **Teamwork** multi-agent board pane (live SSE feed, roster states, tasks & file claims, traces, human post box, quota banner), **Antigravity** model picker with quota/reset display, and write-only masked **Roblox Open Cloud** Settings.
  - Fixed live UX audit items (#25–#28): unclipped Settings buttons & close on Save, `bottom-right` toasts, wrapped drawer output, `OpenRO` naming, full-width Luau editor, auto-attached selection context chips, Color swatch + `Enum.Material` picker, and relocated Undo beside `Effort`.
- **Antigravity CLI Model Provider — Gemini 4 Hydrogen Default (`src/model.rs`, `src/config.rs`, `src/server.rs`, `tests/v011_agent_agy.rs`):**
  - Added pure-text Antigravity CLI (`agy -p ... --output-format stream-json`) model provider defaulting to `gemini-4-hydrogen-high`, with cached `agy models` discovery (`GET /api/models`), persisted provider/model selection (`GET`/`POST /api/config`), strict deny-all tool sandbox (`DATA_DIR/agy-sandbox`), and clear quota/reset-time error surfacing in `/api/status` and `/api/chat`.

## 0.10.0 - 2026-11-22

- **RoPilot 3-Pane Studio IDE Layout (`frontend/src/App.tsx`, `CodeDiffWorkbench.tsx`, `CopilotComposer.tsx`, `StudioBottomDrawer.tsx`, `Shelf.tsx`):**
  - Re-architected OpenRO into a full-bleed **3-Pane Studio IDE**: Left **Game Explorer & Chats** rail (`252px`, auto-syncing live Studio instances and scripts automatically on attach/launch), Center **Full-Bleed Studio Stage** (`Embedded Studio | Studio 3D | Code & Diff | Playable Stage`) with a collapsible bottom Studio Drawer (`Output Console`, `Agent Trace`, **`Studio MCP Tools (13)`**, `Properties`), and Right **Docked RoPilot Copilot** (`380px`, collapsible) with on-demand `Starters` blueprints and clean `390×844` mobile tab bar.
- **Custom Freeform Luau Generation, Whole-Game Context & Transparent Official Studio MCP Translation (`src/agent.rs`, `src/mcp.rs`, `src/luau_check.rs`):**
  - Added freeform `write_luau` and `multi_edit` agent tools (preserving `write_script` skill-only safety) gated by pre-write Static Luau Prover verification (`512 KiB` cap, line-numbered diagnostics, per-chat Undo checkpointing), turn-1 Whole-Game Context Summary + `@Context` paths + reasoning depth (`Low | Med | High`), and whole-game/asset tools (`search_game_tree`, `inspect_instance`, `script_read`, `script_grep`, `execute_luau`, `get_console_output`, `screen_capture`, `start_stop_play`, `user_mouse_input`, `user_keyboard_input`, `blender_generate`, `audio_generate`).
  - Upgraded `McpClient` and `check_bridge_capabilities()` to translate high-level reads/writes/playtests transparently onto official Roblox Studio MCP (`execute_luau`, `start_stop_play`, `get_console_output`, `screen_capture`) with collision-proof dynamic long-bracket escaping and `ChangeHistoryService:SetWaypoint`.
- **Composer Modes (`Agent | Plan | Ask`) + Unified Diff Proposal Workbench (`src/snapshots.rs`, `src/server.rs`, `CodeDiffWorkbench.tsx`):**
  - Extended `POST /api/chat` backward-compatibly with `mode` (`agent | plan | ask`), `reasoning`, `context_paths`, and `review_diffs`: `Ask` enforces strict read-only tools (`0` mutations), `Plan` and `Review diffs` execute on an isolated shadow `LocalPlace` generating reviewable unified green `+` / red `-` hunks, and `GET /api/proposals` + `POST /api/proposals/:id/{apply,reject}` apply selected files atomically with Undo checkpoint, Snapshot, and Playtest proof.
- **Live Roblox Studio Companion Plugin Bridge (`src/studio_plugin.rs`, `StudioPluginDialog.tsx`):**
  - Embedded `OpenRoCompanion.lua` (`HttpService`, `ChangeHistoryService`, `Selection`, `LogService`) with 256-bit OS CSPRNG `X-OpenRo-Plugin-Token` (`0600` in `data_dir/plugin_token`, constant-time verification), canonical OS `Roblox/Plugins` 1-click installer, live selection attachment, and long-poll endpoints (`/api/studio/plugin/{status,download,install,handshake,poll,result}`).

## 0.9.0 - 2026-11-22

- **Directly Embedded Roblox Studio + Real-Time 12 FPS X11/MCP MJPEG Viewport + Input Pass-Through (`src/studio_embed.rs`, `src/studio_stream.rs`, `frontend/src/app/EmbeddedStudioPanel.tsx`):**
  - Launch and embed Roblox Studio directly inside OpenRO's first-class **`Embedded Studio`** workspace tab (`GET /api/studio/embed/status`, `POST /api/studio/embed/launch` accepting optional `place_file`, `POST /api/studio/embed/stop`, strictly user-triggered on loopback — never auto-launched on boot or tests), supporting Linux `Xvfb :99` + Flatpak Vinegar (with 60s startup timeout + real-time 12 FPS `ffmpeg x11grab` MJPEG pump pushing frames directly into `StudioStreamHub`) and native Windows `RobloxStudioBeta.exe` / `StudioMCP.exe`.
  - Added Condvar-backed live multipart MJPEG/MPNG streaming (`GET /api/studio/stream.mjpg` with `?once=1`/`?max_frames=N` and `X-Studio-Frame-Seq`) plus direct pointer and keyboard input forwarding (`POST /api/studio/input/mouse`, `POST /api/studio/input/keyboard`) into Studio.
- **General-Purpose Roblox Studio AI IDE Cleanup (`frontend/src/`):**
  - Removed hardcoded `"Card Art Studio"` and anime-card hero coupling from core OpenRO UI chrome so OpenRO ships cleanly as a general-purpose Roblox Studio AI IDE (`Embedded Studio` + `Studio 3D` + general `Playable Stage` + `Tool & Skill Library`), reducing embedded `assets/index.html` by `623 kB` (`1210 kB`).
- **All 13 Official Roblox Studio MCP Tools (`src/mcp.rs`, `src/agent.rs`, `src/bin/mock_studio.rs`, `POST /api/studio/tools/:tool`):**
  - Full live MCP and Local Place fallback support for all 13 official Studio tools (`list_roblox_studios`, `get_studio_state`, `search_game_tree`, `inspect_instance`, `script_read`, `script_grep`, `multi_edit`, `execute_luau`, `get_console_output`, `screen_capture`, `start_stop_play`, `user_mouse_input`, `user_keyboard_input`), exposed both to OpenRO's autonomous Agent loop and the interactive Studio Tools drawer.
- **Built-In Developer Skill & Connection Library + Blender 5.2.2 LTS + Procedural `.wav` Synth (`src/library.rs`, `src/audio_synth.rs`, `frontend/src/app/ToolLibraryDialog.tsx`):**
  - Ships with **3 universal pre-installed default packs** (`roblox-studio-core`, `blender-3d-pipeline`, `roblox-market-taste`) plus **5 one-click installable packs** (`image-gen-studio` carrying the Stylized Fan-Art Visual Fidelity & Iconic Title-Epithet Knowledge point when installed, `model-3d-gen`, `video-gen-studio`, `audio-sfx-gen`, `rojo-open-cloud` = `8` total), persisted in `data_dir/library.json` (`0600`, masked secrets), with dynamic system-prompt injection into `Agent`.
  - Added headless **Blender 5.2.2 LTS** procedural 3D `.obj`/`.mtl` + PNG preview generation (`POST /api/tools/blender/generate`, `GET /api/assets/meshes/:file`) and zero-dependency pure-Rust 44.1 kHz PCM16 `.wav` SFX synthesis (`POST /api/tools/audio/generate`, `GET /api/assets/audio/:file`).
- **Live Roblox Market Taste & 8-Dimension Craft Dossier + Game Research (`src/market_taste.rs`, `GET /api/market-taste`, `POST /api/market-taste/{niche,refresh,research}`):**
  - Built-in **Top-Trending Roblox Craft & Anti-Slop Quality Bar** defaulting to **`top-trending`** across all 8 profitable genres (`anime-rng-cards`, `viral-sim-tycoon`, `physics-crash-ragdoll`, `survival-expedition`, `horror-coop-mascot`, `tactical-arena-pvp`, `anime-action-rpg-td`, `social-rp-fashion`) with updating live Roblox Explore chart refresh (`apis.roblox.com/explore-api/v1/get-sorts`, strictly user-clicked), live Place/Universe URL research persisting `TASTE-DOSSIER.md`, and automatic injection of 8-dimension Craft Bar + Anti-Slop Guardrails + Automatic Twists into `src/agent.rs` (distilled automatically for small local loopback models).
- **Standalone Example Built With OpenRO (`examples/untitled-anime-card-game/`):**
  - 215-Part sunlit floating archipelago hub (emerald grass terraces, marble plaza, turquoise koi moat, translucent crimson Japanese Torii Summon Shrine, Boss Raid Colosseum, Hall of Legends with 8 iconic anime title-epithet pedestals, 4 Blender 5.2.2 props, 5 PCM16 `.wav` SFX cues, 3,833+ lines of Luau), implementing all 3 Taste-Skill twists (**Chain Overdrive**, **Fusion Weather Storms**, and **Living 3D Lead-Card Guardian Aura**) and verified live inside headless Roblox Studio (`Xvfb :99` + Vinegar + `roblox-studio-mcp`, `0` Luau runtime errors).
- **Cross-Platform Chromeless Desktop App Launcher (`src/desktop.rs`, `src/main.rs`):**
  - Launches OpenRO in a dedicated chromeless desktop window (`chromium --app=` on Linux, `msedge.exe --app=` on Windows, `open` on macOS) with explicit `--no-open`, `--headless`, `--port`, `--version`, and `--help` CLI flags.

## 0.8.0 - 2026-11-22

- **Co-Building 3D Studio IDE — Direct Part Authoring, Duplicate, Delete & Unified Undo/Redo (`src/place.rs`, `src/agent.rs`, `src/server.rs`, `StudioViewport3D.tsx`):**
  - Added typed 3D part preset creation (`POST /api/place/create` supporting `block`, `sphere`, `cylinder`, `spawn`, `jump_pad`, `kill_brick`, and `card_billboard` with automatic companion Luau controllers for `jump_pad` and `kill_brick` and `.rbxlx` `<token name="shape">` round-trip), instance cloning with `+[4, 0, 4]` offset (`POST /api/place/duplicate`, `Ctrl+D`), instance/script deletion (`POST /api/place/delete`), and unified per-chat Undo/Redo (`POST /api/revert` + `POST /api/redo`, `Ctrl+Y` / `Cmd+Shift+Z`, `can_undo`/`can_redo` in `GET /api/chats` & `GET /api/status`) across AI builds, blueprints, 3D edits, and live Luau edits.
  - Upgraded `StudioViewport3D.tsx` with a Studio Ribbon **`+ Add Part`** preset picker, **`Duplicate`**, **`Delete`**, **`Redo`** actions, and radial-shaded 3D **Sphere (`Ball`)** & 10-segment **Cylinder** geometry rendering.
- **In-Explorer Live Luau Script Editor (`POST /api/place/script`, `frontend/src/app/Shelf.tsx`):**
  - Added an inline **Live Luau Script Editor** inside the Studio Explorer using COSS `Textarea` with a 1:1 pixel-aligned line-number gutter, real-time Static Luau Prover diagnostics (`Clean` / `N err` / `N warn`), one-click **`Save & Check`**, and instant **`Playtest`**.
- **One-Click Roblox Studio & Rojo 7 Ship Pack `.zip` Exporter (`src/rojo_export.rs`, `GET /api/export/bundle`, `frontend/src/app/zipBuilder.ts`):**
  - Zero-dependency pure-Rust PKWARE ZIP 2.0 archive writer (IEEE CRC-32, UTF-8 names, ZIP-slip hardened) bundling `game.rbxlx`, `default.project.json`, `src/{server,client,shared}/*.luau`, `asset-manifest.json`, uploaded `assets/*` artwork, `luau-audit-report.json`, and `STUDIO-UPLOAD-CHECKLIST.md` — mirrored 1-to-1 by a zero-dep client-side PKZIP writer in the standalone browser web bundle.
- **Dynamic Live Studio MCP Bridge Connect & Disconnect (`POST /api/studio/mcp/connect`, `POST /api/studio/mcp/disconnect`):**
  - Added loopback-guarded (`403` non-loopback, `409` while busy) live MCP bridge connect/disconnect directly inside `Studio Feed` so creators can hot-attach a running Roblox Studio MCP session (streaming real PNG viewport frames via `/api/studio/frame`) and detach back to the persistent Local Place Engine without restarting `openro-rs` or ever launching Vinegar/Wine automatically.

## 0.7.0 - 2026-11-22

- **Embedded Realtime 3D Roblox Studio Viewport & Live Agent Build Viewer (`frontend/src/app/StudioViewport3D.tsx`, `StagePreview.tsx`):**
  - Zero-dependency interactive HTML5 Canvas 3D perspective viewport featuring painter's algorithm depth sorting, directional face shading, Roblox Studio baseplate stud grid, interactive 3D Axis ViewCube (`X/Y/Z` + `Iso`/`Top`/`Front`), realistic 3D Parts (`Position`/`Size`/`Color`/`Transparency`/`Material`/`Rotation`), Neon rims, `SpawnLocation` decal, and perspective-mapped **Card Art Gallery Billboards**.
  - Full interactive camera (`Orbit`, `Pan`, `Zoom`, `WASD`/`QE` fly, `Focus (F)`, `Reset Cam (R)`, **`Follow Agent` ON by default**), 3D click hit-testing selecting items in Studio Explorer, and interactive `Select` / `Move` / `Scale` / `Rotate` 3D gizmos persisting edits directly via `POST /api/place/instance`.
  - Live **3D Agent Builder Drone + Laser Placement Beam + Part Pop-In Glow + Action Callout** animated in real time from `GET /api/studio/stream`, step-by-step **`Replay Build`**, **`Playtest`** trigger (`POST /api/studio/playtest`), collapsible **Studio Output & Live Agent Stream Console**, and `SegmentedControl` switching seamlessly between **`Studio 3D`**, **`Studio Feed`** (live Roblox Studio MCP PNG frames), and **`Playable Stage`** (playable Anime Card Battles or 60fps WASD Obby).
- **Realtime Studio Action & Frame Stream Hub + Live MCP Capture (`src/studio_stream.rs`, `src/mcp.rs`, `src/agent.rs`, `src/server.rs`):**
  - Added bounded 500-event `StudioStreamHub` with 32-client SSE fan-out (`GET /api/studio/stream`, `Last-Event-ID`/`?since` replay, 15s keepalive), `GET /api/studio/actions`, `GET /api/studio/frame` (`X-Frame-Seq`, `Cache-Control: no-store`, honest `404`), `POST /api/studio/capture` (live MCP `screen_capture()` PNG or honest `503` on Local Place Engine), and `POST /api/studio/playtest` (`start_stop_play` + `get_console_output` or Local Place playtest).
  - Emits granular `StudioActionEvent` records (`part_created`, `part_updated`, `script_written`, `decal_bound`, `playtest_started`, `playtest_log`, `playtest_passed`, `playtest_failed`, `studio_frame`, `reverted`, `imported`, `restored`) and auto-captures live MCP PNG frames on build mutations.
  - Upgraded `mock_studio` with a zero-dependency spec-compliant dynamic 3D isometric PNG `screen_capture`, `get_console_output`, and `start_stop_play` so end-to-end live MCP frame pipelines are mechanically verifiable without launching external binaries.
- **Standalone Browser Web Parity (`frontend/web-engine/install.ts`):**
  - Mirrored the embedded `Studio 3D` viewport, live action stream, IndexedDB action ring buffer, `Replay Build`, `Playtest`, and honest `Studio Feed` mode into the single-file browser bundle.

## 0.6.0 - 2026-11-22

- **28 Verified Roblox Game Systems (+6 Anime Card-Battler Systems, `src/skills.rs`, `src/ideas.rs`):**
  - Added six composable, server-authoritative card-battler systems passing the Static Luau Prover with `0` errors / `0` warnings: `card_gacha_roller` (7 rarities, 5 foil borders, luck/weather/potion/rebirth multipliers, pity & auto-roll), `deck_builder` (6-slot deck, collection bonuses, 1★–5★ fusion, rebirth & `pcall`-guarded DataStore persistence), `card_battle_arena` (5-element synergy wheel, turn order, Ultimate Burst Gauge, Active Counter/Interrupt timing, Impossible Boss raids), `tower_progression` (50-floor Endless Tower across 5 themed worlds + persistent `OrderedDataStore` leaderboard), `potions_weather` (server weather lighting cycles, craftable potions, promo codes), and `card_gallery_world` (3D Studio Lobby with showcase pedestals, `Decal` + `SurfaceGui` `ImageLabel` bindings, tower gate, boss shrine, and leaderboard wall).
  - Registered the flagship **`anime-card-battles-apex`** (*Anime Card Battles: Rebirth — Apex Edition*) starter blueprint composing all 6 card systems alongside `leaderboard`, `datastore_save`, `shop`, `game_gui`, `soundtrack`, and 3D lobby parts, plus 6 standalone card-battler feature ideas.
- **Anime Fan-Art & Creative Homage Knowledge Point (`src/art_knowledge.rs`, `GET /api/art-knowledge`):**
  - Built-in creative homage policy injected directly into the agent system prompt so OpenRO never knee-jerk refuses anime-inspired Roblox game concepts, instead translating iconic archetypes into original DMCA-safe cel-shaded characters, balanced kits, and watermark-free trading-card art prompts.
- **Game Art Asset Manager & `.rbxlx` `Decal`/`ImageLabel` Bindings (`src/assets.rs`, `src/place.rs`):**
  - Persistent `data_dir/game-assets/` manager with strict filename sanitization, PNG/JPEG/WebP magic-byte verification, symlink-escape containment, atomic manifest persistence, live Google Gemini (Nano Banana) / OpenAI Image generation (failing honestly with `503` setup guidance when unconfigured), `GET /api/assets`, `GET|DELETE /api/assets/:file`, `GET /api/assets/manifest`, `POST /api/assets/upload`, and `POST /api/assets/generate`.
  - Upgraded `.rbxlx` XML serializer/parser (`src/place.rs`) to emit and losslessly round-trip `<Content name="Texture"><url>...</url></Content>`, `<Content name="Image"><url>...</url></Content>`, and `<token name="Face">`, automatically binding uploaded card art into `Workspace.CardGallery` on upload, blueprint build, and `.rbxlx` export.
- **Card Art & Asset Studio + Playable Anime Card Battles Stage (`frontend/`):**
  - Added the **Card Art & Asset Studio** dialog (`Card Gallery`, `Upload & Generate`, `Anime Style Guide`), **Card Art Image Generation** keys inside Settings (`Model, Studio & Appearance`), a Featured Apex landing showcase, and a fully playable in-workbench **Anime Card Battles: Rebirth — Apex Edition** stage (`Roll Pack`, `Deck & Fusion`, `Boss & Tower Arena`, plus `3D Lobby Map` view) backed by 8 real high-craft anime card illustrations — mirrored 1-to-1 in the standalone browser web bundle.
- **End-to-End Flagship Example (`examples/anime-card-battles-apex/`):**
  - Shipped a complete Roblox Studio export (`AnimeCardBattlesApex.rbxlx` + `asset-manifest.json` + 8 bound card illustrations) built end-to-end exclusively through `openro-rs 0.6.0`.

## 0.5.1 - 2026-11-22

- **Clear Professional UI Terminology:** Replaced internal engine wording across the Studio Workbench UI, live SSE build progress labels (`Generating Luau`), standalone browser web bundle (`22 Game Systems`, `Studio Workbench`), and `README.md` with clear, standard Roblox Studio terminology while keeping all 22 embedded game-system generators, APIs, and `.rbxlx` fidelity 100% intact.

## 0.5.0 - 2026-11-22

- **22 Verified Roblox Game-System Rune Skills (`src/*.rn`, `src/skills.rs`, `src/ideas.rs`):**
  - Expanded the Rune Luau skill engine from 10 to **22 composable, server-authoritative Roblox systems** (preserving all 10 legacy skills backwards-compatibly), adding `jump_pad`, `moving_platform`, `datastore_save`, `npc_patrol`, `portal_teleporter`, `combat_sword`, `inventory_backpack`, `quest_tracker`, `vehicle_spawner`, `weather_cycle`, `tycoon_dropper`, and `admin_spectate`, plus `GET /api/skills` and one-click `POST /api/skills/build`.
- **Static Luau Analyzer & Cross-Reference Prover (`src/luau_check.rs`, `GET/POST /api/luau/check`):**
  - Full Luau lexer, block/delimiter balance checker, unyielded `while true do` loop detector, unguarded `DataStoreService` & missing `BindToClose` warnings, unvalidated `RemoteEvent` parameter checks, and Place cross-reference verification for `WaitForChild`/`FindFirstChild`/`workspace.<Member>` — wired directly into Local Place playtest verification and Explorer audit badges.
- **True Roblox Studio `.rbxlx` XML Fidelity & Bidirectional Import (`src/place.rs`, `POST /api/import/rbxlx`):**
  - Emits authentic Roblox `<Properties>` (`CoordinateFrame` CFrame, `Vector3` Size, `Color3uint8`, `Anchored`, `CanCollide`, `Material` token table, `Transparency`, CDATA-safe `ProtectedString` Luau source) and parses imported `.rbxlx` files back into the Place DataModel (up to 16 MB, fuzz-hardened against XXE/NUL/depth-64/truncation, 500+ instances & 50+ scripts round-tripped in milliseconds).
- **Live SSE Build Stream, Versioned Snapshots & Live Property Editor (`src/sse.rs`, `src/snapshots.rs`):**
  - Added true Server-Sent Events stream `GET /api/build/stream` (`Planning -> Compiling Rune -> Static Luau Check -> Playtest Proof -> Auto-Repair`) with `Last-Event-ID` replay and 15s keepalive, persistent versioned Place snapshots (`GET/POST /api/snapshots`, `POST /api/snapshots/:id/restore`, `GET /api/snapshots/:id/rbxlx`, `GET /api/place/diff`), and inline Instance property editing (`POST /api/place/instance`).
- **Upgraded 60fps Studio Workbench & Standalone Browser Web Engine (`frontend/`):**
  - Added 60fps `WASD`/Arrow/Space interactive Playable Stage Sandbox (`Top-Down` & `Side-View` cameras, canvas part hit-test selecting Explorer items), syntax-highlighted Luau viewer with line numbers, inline property editor, `Cmd+K` Studio Command Palette, 22-system Blueprint Library browser, `.rbxlx` drag-and-drop import, Version Snapshots timeline dialog, and upgraded standalone browser web bundle (all 22 Luau compilers, bidirectional `.rbxlx` import/export, IndexedDB snapshots, Settings-only Light/Dark).

## 0.4.0 - 2026-11-21

- **Real Local Place Engine + `.rbxlx` Export + Studio MCP Auto-Detect:**
  - Added a persistent Local Roblox Place Engine (`src/place.rs`) backed by `data_dir/place.json` and mirrored atomically to genuine Roblox Studio XML (`data_dir/game.rbxlx`, `<roblox version="4">`) on every build and Undo, with a real Luau lexer/syntax/service-root/runtime-wiring playtest verifier so OpenRO builds, playtests, reverts, and exports real Roblox places even when Roblox Studio is not currently open.
  - Added passive Vinegar Flatpak Roblox Studio auto-detection (`$HOME/.local/bin/roblox-studio-mcp` when `org.vinegarhq.Vinegar` is actively running; never launches Vinegar or downloads anything).
  - Added `GET /api/export/rbxlx` and a pinned **Export .rbxlx** button in the `"In Your Game"` Studio Explorer, plus full real Luau source code display and one-click `Copy Luau` inside each expanded Explorer script.
  - Added an interactive HTML5 Canvas **Playable Stage Preview** (`StagePreview.tsx`) synced live to Workspace parts, checkpoints, hazards, currency pickups, shop items, and Day/Night lighting.
- **Live Keyless Local Model Connectivity & Settings (`Model, Studio & Appearance`):**
  - Loopback OpenAI-compatible gateways (`127.0.0.1`, `localhost`, `[::1]`) now run without requiring an API key; when `OPENRO_MODEL_BASE_URL` and `OPENRO_MODEL_API_KEY` are unset (and `OPENRO_NO_AUTO_MODEL` is not `1`), `openro-rs` auto-probes running local gateways (`11434`, `8789`, `8790`) while preserving strict fail-closed no-key refusal for remote hosts.
  - Added loopback-guarded `GET/POST /api/config` (`data_dir/config.json`, `0600` permissions) and a **Model, Studio & Appearance** `MorphingDialog` in the Left Rail footer housing the Light/Dark `SegmentedControl` (moved off the main rail), live gateway switcher, custom URL/model/key fields, and Studio/Local Place mode switching without restarting.
- **Real Functional Standalone Web Engine (`npm run build:web`):**
  - Replaced legacy fixture-demo scaffolding with a real in-browser Roblox Studio engine (`frontend/web-engine/`) featuring a parametric Luau compiler, Luau syntax & wiring playtest validator, interactive Stage Preview, genuine `.rbxlx` XML download, IndexedDB persistence, optional BYOK LLM (`sessionStorage` only), and one-click Connect to Local `openro-rs` (`127.0.0.1:8077`).

## 0.3.0 - 2026-11-21

- **Game Builder Studio Workbench (House Frontend):** Replaced the legacy chat/quest layout with a three-region Studio Workbench built on the House Frontend component stack (Motion Primitives, COSS UI, Cult UI, Kibo UI, Tremor, Vaul, Sonner, Motion) with self-hosted Geist and Geist Mono `.woff2` fonts in Light (default) and Dark themes, embedded into the single `openro-rs` binary.
  - **Left Studio Session Rail:** `256px` desktop rail (Vaul drawer below `768px`) with live Studio MCP status pill and `Check again` action, `/` keyboard-accessible chat search filter, gliding `AnimatedBackground` chat switcher, `New chat`, Light/Dark `SegmentedControl`, and release version link.
  - **Center Studio Stage & Build Trace:** Equal-height Game Blueprint Starter Cards (`Lava Obby`, `Coin Tycoon`, `Round Arena`, plus contextual feature blueprints) with category badges, part/script count chips, capability pills, `"Already in your game"` tracking (`built_titles` on `POST /api/ideas`), and one-click Studio builds; active Studio Build Trace with Tremor `ProgressCircle`, single-line `SlidingNumber` change counts, Playtest Verdict banner (`Passed` / `Failed` / `Not proven yet`), and expandable Luau console logs.
  - **Hairline Composer Dock & Update Strip:** Responsive `InputGroup` composer (`Enter` / `Shift+Enter`), `Undo` `MorphingDialog` confirmation with keyboard focus trap and return, and hardened self-update banner (loopback-only `Update now`, reachable `Show update` / `Check updates` at `390px`, focus preservation across apply/restart/dismiss, and plain-English recovery copy).
  - **Right "In Your Game" Studio Explorer Shelf:** `300px` desktop DataModel Explorer (Vaul bottom sheet below `1100px`) grouping `Workspace` parts and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, expandable hierarchy & Luau previews, and pinned `Copy game summary`.
- **Verification & E2E Suite:** Added Playwright E2E gate (`78` checks across `1440px`, `768px`, `390px`, and `320px` in Light and Dark), worst-case API replay (`replay.mjs`), `JOB.md`, `ROLE-COVERAGE.md`, and `ALIVE.md`.

## 0.2.0 - 2026-11-19

- New front end: chat shell with the build timeline, ideas bar, one-tap builds, and an Undo control that is enabled only when there is something to undo.
- Honest results: an ask that changed nothing because a step failed is refused in plain words; a playtest in an ask with no changes is reported as not run; a shop build refuses a missing prerequisite by its display name.
- Proof: playtest verdicts are kept per chat in `proof.json`, so a failed run still blocks "done" after a restart.
- Safety: same-site guard (Host, Origin, `Sec-Fetch-Site`, JSON-only POST), `OPENRO_ALLOWED_HOSTS`, 10 s connection timeouts, a 64-connection cap, and 413 JSON for bodies over 1 MiB.
- Recovery: corrupt saved files are backed up and named on the page and in `/api/status`.
- `--version`, `GET /api/version`, and self-update from GitHub releases (`OPENRO_UPDATE_MODE`).

## 0.1.0 - 2026-11-17

- First Rust + Rune build: one binary serving the web shell, the agent loop, the Studio bridge, Rune code generation for ten skills, the creativity engine, and revert with change sets.
