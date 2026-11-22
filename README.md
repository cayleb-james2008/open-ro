# OpenRO (`v0.9.0` — Directly Embedded Roblox Studio + All 13 Studio MCP Tools + Market Taste Craft Dossier + Tool & Skill Library)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

Describe a Roblox game in plain words, pick a Studio blueprint (including **Untitled Anime Card Game**), browse 28 built-in Roblox game systems, co-build inside the **embedded 3D Roblox Studio IDE** or launch **Directly Embedded Roblox Studio** (live MJPEG viewport, direct mouse/keyboard pass-through, and all 13 official Roblox Studio MCP tools), tap into the built-in **Tool & Skill Library** (Blender 5.2.2 3D mesh generator, procedural `.wav` SFX synth, and Live Roblox Market Taste 8-Dimension Craft Dossier), live-edit and statically verify Luau scripts in the **Studio Explorer**, and export a turnkey **Roblox Studio & Rojo 7 Ship Pack (`.zip`)** or single `.rbxlx` place in one click.

## Architecture

- **Single-binary Rust desktop app & server (`openro-rs`):** Opens in a dedicated chromeless desktop window (`src/desktop.rs` — Linux `chromium --app=`, Windows `msedge.exe --app=`, macOS `open`; pass `--no-open` / `OPENRO_NO_OPEN=1` for headless mode) and serves the embedded single-file React + Tailwind 4 **Game Builder Studio Workbench** (`assets/index.html`, with JS, CSS, bundled card art, and self-hosted `Geist` + `Geist Mono` `.woff2` fonts inlined via `include_str!`) over `std::net::TcpListener`.
  - **Left Studio Session Rail (`256px` desktop, Vaul drawer `<768px`):** Game place header, live backend status pill (`Studio connected` / `Local Place ready` / `Studio not connected` + `Check again`), primary `New chat`, `/` search filter across game sessions, `AnimatedBackground` chat list, `Cmd+K` Command Palette (`Open Embedded Studio`, `Open Tool Library`, `Add 3D Part`, `Redo`, `Download Rojo Ship Pack (.zip)`), `Card Art Studio`, `Tool Library` (`4/9` built-in + installable packs + **Market Taste Radar**), **Settings (`Model, Studio & Appearance`)** `MorphingDialog` (exclusively housing the Light/Dark `SegmentedControl`, local gateway auto-detection, live Model, Image Generation & MCP config), and release version link.
  - **Directly Embedded Roblox Studio & Co-Building 3D Viewport (`src/studio_embed.rs`, `frontend/src/app/EmbeddedStudioPanel.tsx`, `StudioViewport3D.tsx`):** `SegmentedControl` switches between **`Studio 3D`** (zero-dependency Canvas 3D Studio IDE with Roblox baseplate stud grid, Axis ViewCube, `+ Add Part`, `Duplicate`, `Delete`, `Redo`, transform gizmos, live SSE Agent Builder Drone, and step-by-step `Replay Build`), **`Embedded Studio`** (user-triggered Linux `Xvfb :99` + Vinegar or native Windows Studio launch, live multipart MJPEG stream via `/api/studio/stream.mjpg`, interactive pointer & keyboard pass-through, Solo Play/Run Server/Stop, and interactive **All 13 Official Studio MCP Tools** drawer), and **`Playable Stage`** (interactive 7-Weather + Fusion Storm 4v4 **Untitled Anime Card Game** preview with Chain Overdrive).
  - **Built-In Tool & Skill Library + Live Roblox Market Taste Craft Dossier (`src/library.rs`, `src/market_taste.rs`, `src/audio_synth.rs`, `ToolLibraryDialog.tsx`):** Ships with **4 pre-installed default packs** (`roblox-studio-core`, `blender-3d-pipeline`, `anime-fan-art-knowledge`, `roblox-market-taste`) + **5 installable packs** (`9` total), dynamic Agent system-prompt injection, headless **Blender 5.2.2 LTS** procedural 3D `.obj` + PNG preview generation, zero-dependency pure-Rust 44.1 kHz PCM16 `.wav` SFX synthesis, and the **8-Dimension Production Taste Dossier** across 8 profitable Roblox niches with live Explore chart refresh (`apis.roblox.com/explore-api/v1/get-sorts`) and on-demand Roblox Place/Universe URL research (`TASTE-DOSSIER.md`).
  - **Card Art & Asset Studio + Visual Fidelity & Title-Epithet Knowledge Point (`src/art_knowledge.rs`, `src/assets.rs`):** Built-in **Anime Fan-Art Knowledge Point** keeps character art visually recognizable while mapping display titles to iconic epithets (`Joyful Pirate`, `Six-Eyes Sorcerer`, `Shadow Monarch`, `Nine-Tail Shinobi`, `Thunder Flash Blade`, `Steel Alchemist`, `Gravity Ronin`, `Golden Ascended Warrior`).
  - **Right "In Your Game" Studio Explorer Shelf (`300px` `>=1100px`, Vaul bottom drawer `<1100px`):** Familiar Roblox Studio DataModel hierarchy grouping `Workspace` parts/decals and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, inline **Live Luau Script Editor** (line-number gutter, real-time Static Luau Prover badge, `Save & Check`, `Playtest`), inline `Position`/`Size`/`Color`/`Anchored` property editor with `Duplicate`/`Delete`, expandable **Latest Build Diff**, `.rbxlx` **Import**, pinned **Rojo Ship Pack (`.zip`)** + **Export `.rbxlx`**, and **Copy game summary**.
- **Model route (`src/model.rs`):** OpenAI-compatible `POST {base}/chat/completions`. Loopback gateways (`127.0.0.1`, `localhost`, `[::1]`) work without an API key, and when `OPENRO_MODEL_BASE_URL` and `OPENRO_MODEL_API_KEY` are unset (and `OPENRO_NO_AUTO_MODEL` is not `1`), `openro-rs` auto-probes running local gateways (`127.0.0.1:11434/v1`, `8789/v1`, `8790/v1`). Remote endpoints still require an API key and fail closed when unconfigured. Settings persist live to `data_dir/config.json` (`0600`) via loopback-guarded `GET/POST /api/config`.
- **Real Local Place Engine, Static Luau Prover & Studio MCP (`src/place.rs`, `src/luau_check.rs`, `src/mcp.rs`):** Connects to `OPENRO_MCP_COMMAND` when explicitly set, passively detects a running Studio MCP bridge, or launches on user click via `POST /api/studio/embed/launch` (never auto-launching Vinegar/Studio on boot). Otherwise runs on its built-in persistent Local Place Engine (`data_dir/place.json` + atomic `data_dir/game.rbxlx` `<roblox version="4">` XML), bidirectional `.rbxlx` import/export, and full static Luau + cross-reference verification.
- **28 Verified Roblox Game Systems (`src/skills.rs`):** Sandboxed embedded game-system generators produce all server-authoritative Luau (`round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`, `currency`, `shop`, `game_gui`, `checkpoints`, `soundtrack`, `jump_pad`, `moving_platform`, `datastore_save`, `npc_patrol`, `portal_teleporter`, `combat_sword`, `inventory_backpack`, `quest_tracker`, `vehicle_spawner`, `weather_cycle`, `tycoon_dropper`, `admin_spectate`, `card_gacha_roller`, `deck_builder`, `card_battle_arena`, `tower_progression`, `potions_weather`, and `card_gallery_world`).
- **Flagship Sunlit Studio Example (`examples/untitled-anime-card-game/`):** Complete **Untitled Anime Card Game** Roblox Studio place (`UntitledAnimeCardGame.rbxlx`, `TASTE-DOSSIER.md`, 215-Part sunlit floating archipelago, 4 Blender 5.2.2 props, 5 PCM16 `.wav` SFX cues, 8 Nano Banana character portraits, **Chain Overdrive**, **Fusion Weather Storms**, and **Living 3D Lead-Card Guardian Aura**) verified live in headless Roblox Studio (`0` Luau runtime errors).
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
| `GET` | `/api/version` | `{ "name": "openro-rs", "version": "0.9.0" }` |
| `GET` | `/api/status` | `studio_connected`, `model_configured`, `workspace_mode`, `studio_mode`, `studio_stream`, `studio_embed`, `studio_mjpeg`, `library_api`, `market_taste_api`, `active_niche`, `embedded_running`, `studio_tools`, `ide_api`, `ide_v080`, `rojo_export`, `can_undo`, `can_redo`, `model_id`, `sse_build_stream`, `skills_count`, `game_name`, `version`, `store_recovered` |
| `GET` | `/api/studio/embed/status` | Embedded Roblox Studio host detection, lifecycle state, active display, play mode & official tool count |
| `POST` | `/api/studio/embed/launch` | Loopback-only user-triggered launch & embed of Roblox Studio (`Xvfb :99` + Vinegar or Windows Studio / MCP command) |
| `POST` | `/api/studio/embed/stop` | Loopback-only clean stop of Embedded Roblox Studio returning to persistent Local Place Engine |
| `GET` | `/api/studio/stream.mjpg` | Live multipart MJPEG/MPNG viewport stream (`multipart/x-mixed-replace; boundary=openro-frame`, `?once=1`/`?max_frames=N`) |
| `POST` | `/api/studio/input/mouse` | Loopback-only mouse pointer/click/drag/wheel pass-through directly into Studio (`user_mouse_input`) |
| `POST` | `/api/studio/input/keyboard` | Loopback-only keyboard press/text pass-through directly into Studio (`user_keyboard_input`) |
| `POST` | `/api/studio/tools/:tool` | Loopback-only direct invocation of any of the 13 official Roblox Studio MCP tools |
| `GET` | `/api/library` | List all 9 Tool & Skill Library packs (`4` pre-installed defaults + `5` installable), Blender detection & active tools |
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
| `POST` | `/api/chat` | Run a natural-language Studio ask (`{"message": "..."}`) |
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

# Optional: build the self-contained browser web app (Embedded 3D Studio Viewport + 28 Luau Compilers + Playable Untitled Anime Card Game + Market Taste + .rbxlx/.zip Export)
cd frontend && npm run build:web
```

## Verification

```sh
# Rust unit + integration + Studio embed, library, market-taste & viewer suite (166 tests)
cargo test

# Full Playwright UI & worst-case replay suite (1440px, 768px, 390px, 320px in Light & Dark)
cd frontend && npm run check:ui
```
