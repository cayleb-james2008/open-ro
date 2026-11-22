# OpenRO (`v0.7.0` — Embedded Realtime 3D Roblox Studio Viewport + Live Agent Build Viewer + 28 Game Systems + Card Art Studio)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

Describe a Roblox game in plain words, pick a Studio blueprint (including **Anime Card Battles: Rebirth — Apex Edition**), browse 28 built-in Roblox game systems, upload or generate card artwork in the **Card Art & Asset Studio**, or import an existing `.rbxlx` place — and watch OpenRO build it live inside an **embedded realtime 3D Roblox Studio Viewport** (with animated agent placement beams, live Studio MCP viewport frame capture, `Select`/`Move`/`Scale`/`Rotate` 3D gizmos, `Replay Build`, and docked Studio Output Console), prove every Luau script with zero syntax/cross-reference errors, export genuine `.rbxlx` XML with `Decal`/`ImageLabel` bindings, playtest directly in the stage, and revert in one click.

## Architecture

- **Single-binary Rust server (`openro-rs`):** Serves the embedded single-file React + Tailwind 4 **Game Builder Studio Workbench** (`assets/index.html`, with JS, CSS, bundled card art, and self-hosted `Geist` + `Geist Mono` `.woff2` fonts inlined via `include_str!`) over `std::net::TcpListener`, alongside the JSON/XML/Binary/SSE API.
  - **Left Studio Session Rail (`256px` desktop, Vaul drawer `<768px`):** Game place header, live backend status pill (`Studio connected` / `Local Place ready` / `Studio not connected` + `Check again`), primary `New chat`, `/` search filter across game sessions, `AnimatedBackground` chat list, `Cmd+K` Command Palette, `Card Art` Studio & `Snapshots` triggers, **Settings (`Model, Studio & Appearance`)** `MorphingDialog` (exclusively housing the Light/Dark `SegmentedControl`, local gateway auto-detection, live Model, Image Generation & MCP config), and release version link.
  - **Embedded Realtime 3D Roblox Studio Viewport & Live Agent Build Viewer (`frontend/src/app/StudioViewport3D.tsx`, `src/studio_stream.rs`):** Zero-dependency HTML5 Canvas 3D Studio viewport with Roblox baseplate stud grid, interactive 3D Axis ViewCube, perspective-mapped 3D Card Gallery billboards, camera Orbit/Pan/Zoom/`WASD`+`QE`/`Follow Agent`, 3D click-to-select + `Select`/`Move`/`Scale`/`Rotate` transform gizmos persisting via `POST /api/place/instance`, live SSE **Agent Builder Drone + Laser Placement Beam** driven by `GET /api/studio/stream`, step-by-step **`Replay Build`**, **`Playtest`** button (`POST /api/studio/playtest`), collapsible **Studio Output & Live Agent Stream Console**, and `SegmentedControl` toggling between **`Studio 3D`**, **`Studio Feed`** (live Roblox Studio MCP PNG frames via `/api/studio/frame`), and **`Playable Stage`** (playable Anime Card Battles or 60fps WASD Obby).
  - **Center Studio Stage, 28-System Library & Build Trace:** Featured **Anime Card Battles: Rebirth — Apex Edition** banner + equal-height **Game Blueprint Starter Cards** + searchable **28 Game Systems** library (`Movement`, `Economy`, `Combat`, `World`, `Persistence`, `UI`, `Card Battler`); live SSE build progress stream (`Planning -> Generating Luau -> Static Luau Check -> Playtest Proof`); active **Studio Build Trace** with Tremor `ProgressCircle`, inline `SlidingNumber` change counts, **Playtest Verdict** banner with expandable Luau console logs, and hairline Composer Dock (`Enter` / `Shift+Enter`, `Undo` `MorphingDialog` confirmation, and self-update strip).
  - **Card Art & Asset Studio + Creative Homage Knowledge Point (`src/art_knowledge.rs`, `src/assets.rs`):** Built-in **Anime Fan-Art & Creative Homage Knowledge Point** ensures OpenRO translates anime-inspired game concepts into original, DMCA-safe cel-shaded designs rather than refusing creative asks. Upload PNG/JPEG/WebP artwork or configure a Google Gemini (Nano Banana) / OpenAI Image API key in Settings; stored assets persist in `data_dir/game-assets/`, bind automatically to `Workspace.CardGallery` `Decal` & `ImageLabel` nodes in exported `.rbxlx` files, and export with `asset-manifest.json`.
  - **Right "In Your Game" Studio Explorer Shelf (`300px` `>=1100px`, Vaul bottom drawer `<1100px`):** Familiar Roblox Studio DataModel hierarchy grouping `Workspace` parts/decals and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, syntax-highlighted Luau code viewer with line numbers, **Static Luau Audit** badges, inline `Position`/`Size`/`Color`/`Anchored` property editor, expandable **Latest Build Diff**, `.rbxlx` **Import** (button & drag-and-drop), pinned **Export .rbxlx**, and **Copy game summary**.
- **Model route (`src/model.rs`):** OpenAI-compatible `POST {base}/chat/completions`. Loopback gateways (`127.0.0.1`, `localhost`, `[::1]`) work without an API key, and when `OPENRO_MODEL_BASE_URL` and `OPENRO_MODEL_API_KEY` are unset (and `OPENRO_NO_AUTO_MODEL` is not `1`), `openro-rs` auto-probes running local gateways (`127.0.0.1:11434/v1`, `8789/v1`, `8790/v1`). Remote endpoints still require an API key and fail closed when unconfigured. Settings persist live to `data_dir/config.json` (`0600`) via loopback-guarded `GET/POST /api/config`.
- **Real Local Place Engine, Static Luau Prover & Studio MCP auto-detect (`src/place.rs`, `src/luau_check.rs`, `src/mcp.rs`):** Connects to `OPENRO_MCP_COMMAND` when explicitly set or passively detects a running Vinegar Flatpak Roblox Studio instance with `~/.local/bin/roblox-studio-mcp` (never launching Vinegar unprompted). Otherwise runs on its built-in persistent Local Place Engine (`data_dir/place.json` + atomic `data_dir/game.rbxlx` `<roblox version="4">` XML with real `CoordinateFrame`/`Vector3`/`Color3uint8`/`Material`/`Transparency`/`<Content name="Texture|Image">`/`<token name="Face">`/CDATA `ProtectedString`), bidirectional `.rbxlx` import/export, and full static Luau + cross-reference verification.
- **28 Verified Roblox Game Systems (`src/skills.rs`):** Sandboxed embedded game-system generators produce all server-authoritative Luau: `round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`, `currency`, `shop`, `game_gui`, `checkpoints`, `soundtrack`, `jump_pad`, `moving_platform`, `datastore_save`, `npc_patrol`, `portal_teleporter`, `combat_sword`, `inventory_backpack`, `quest_tracker`, `vehicle_spawner`, `weather_cycle`, `tycoon_dropper`, `admin_spectate`, `card_gacha_roller`, `deck_builder`, `card_battle_arena`, `tower_progression`, `potions_weather`, and `card_gallery_world`. Arbitrary raw Luau outside registered systems is refused, and all parameters are strictly type-validated in Rust.
- **Flagship End-to-End Example (`examples/anime-card-battles-apex/`):** Complete **Anime Card Battles: Rebirth — Apex Edition** Roblox Studio export (`AnimeCardBattlesApex.rbxlx`, `asset-manifest.json`, and 8 high-res anime card illustrations) built end-to-end through `openro-rs 0.6.0`.
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
| `GET` | `/api/version` | `{ "name": "openro-rs", "version": "0.7.0" }` |
| `GET` | `/api/status` | `studio_connected`, `model_configured`, `workspace_mode`, `studio_mode`, `studio_stream`, `model_id`, `sse_build_stream`, `skills_count`, `game_name`, `version`, `store_recovered` |
| `GET` | `/api/studio/stream` | Live Server-Sent Events stream (`status`, `studio_action`, `studio_frame`) with `Last-Event-ID`/`?since` replay, 15s keepalive & 32-client cap |
| `GET` | `/api/studio/actions` | Bounded 500-event ring snapshot of granular agent Studio mutations (`?since=<seq>`) + live frame metadata |
| `GET` | `/api/studio/frame` | Latest captured live Roblox Studio MCP viewport PNG/JPEG (`X-Frame-Seq`, `Cache-Control: no-store`, or honest `404` when uncaptured) |
| `POST` | `/api/studio/capture` | Capture a live viewport PNG frame from Roblox Studio MCP (`screen_capture`) or return honest `503` on Local Place Engine |
| `POST` | `/api/studio/playtest` | Run live MCP `start_stop_play` + `get_console_output` or Local Place playtest, streaming console logs & verdict over SSE |
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
| `POST` | `/api/import/rbxlx` | Import a Roblox Studio `.rbxlx` XML file (JSON `{"xml"}` or raw `application/xml`, up to 16 MB) |
| `GET` | `/api/export/rbxlx` | Downloads genuine Roblox Studio XML (`<roblox version="4">`) for the current place |
| `GET` | `/api/config` | Loopback-only Model & Studio runtime config + detected local gateways |
| `POST` | `/api/config` | Loopback-only live Model & Studio settings update (saved to `config.json`) |
| `GET` | `/api/chats` | `chats`, `active`, and `can_undo` |
| `POST` | `/api/chats` | Create and select a new chat (`201 Created`) |
| `POST` | `/api/chats/select` | Switch active chat (`{"id": "..."}`) |
| `GET` | `/api/thread` | Thread lines for the active chat |
| `POST` | `/api/chat` | Run a natural-language Studio ask (`{"message": "..."}`) |
| `GET` | `/api/place-tree` | Current Studio / Local Place `scripts` and `instances` map |
| `POST` | `/api/ideas` | Contextual blueprints (`ideas`, `built_titles`, `pitched_new`) |
| `POST` | `/api/ideas/build` | Execute a built-in blueprint plan (`{"id": "..."}`) |
| `POST` | `/api/revert` | Revert the latest change set in the active chat |
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

# 3. Run locally (http://127.0.0.1:8077 — auto-probes local gateways & runs Local Place Engine out of the box)
./target/release/openro-rs

# Optional: build the self-contained browser web app (Embedded 3D Studio Viewport + 28 Luau Compilers + Playable Anime Card Battles + .rbxlx Import/Export)
cd frontend && npm run build:web
```

## Verification

```sh
# Rust unit + integration + Studio viewer & scale suite (143 tests)
cargo test

# Full Playwright UI & worst-case replay suite (1440px, 768px, 390px, 320px in Light & Dark)
cd frontend && npm run check:ui
```
