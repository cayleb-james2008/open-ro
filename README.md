# OpenRO (`v0.3.0` — Rust + Rune + House Frontend)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

Describe a Roblox game in plain words or pick a Studio blueprint — OpenRO generates Luau from verified Rune skills, writes instances and scripts into Roblox Studio over MCP, runs a playtest before calling any build done, and reverts the last build in one click.

## Architecture

- **Single-binary Rust server (`openro-rs`):** Serves the embedded single-file React + Tailwind 4 **Game Builder Studio Workbench** (`assets/index.html`, with JS, CSS, and self-hosted `Geist` + `Geist Mono` `.woff2` fonts inlined via `include_str!`) over `std::net::TcpListener`, alongside the JSON API.
  - **Left Studio Session Rail (`256px` desktop, Vaul drawer `<768px`):** Game place header, live Studio MCP status pill (`Studio connected` / `Studio not connected` + `Check again`), primary `New chat`, `/` search filter across game sessions, `AnimatedBackground` chat list, Light/Dark `SegmentedControl` (Light default, full Dark mode), and release version link.
  - **Center Studio Stage & Build Trace:** Equal-height **Game Blueprint Starter Cards** (`Lava Obby`, `Coin Tycoon`, `Round Arena`, plus follow-up feature blueprints) with category tags, deterministic part/script count chips, `"Adds to Studio"` capability pills, `"Already in your game"` tracking, and one-click Studio builds; active **Studio Build Trace** with Tremor `ProgressCircle`, inline `SlidingNumber` change counts, **Playtest Verdict** banner (`Passed` / `Failed` / `Not proven yet`) with expandable Luau console logs, and hairline Composer Dock (`Enter` / `Shift+Enter`, `Undo` `MorphingDialog` confirmation, and self-update strip).
  - **Right "In Your Game" Studio Explorer Shelf (`300px` `>=1100px`, Vaul bottom drawer `<1100px`):** Familiar Roblox Studio DataModel hierarchy grouping `Workspace` parts and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, expandable DataModel path & Luau source previews, and pinned `Copy game summary`.
- **Model route:** OpenAI-compatible `POST {base}/chat/completions` (default base `https://opencode.ai/zen/go/v1`, default model `deepseek-v4.1-flash`, bearer key from `OPENRO_MODEL_API_KEY`). Without a key, the UI displays an explicit no-key banner while built-in Studio blueprints remain runnable when Studio is connected.
- **Studio MCP bridge:** JSON-RPC 2.0 over stdio to the child process configured via `OPENRO_MCP_COMMAND` (a live Roblox Studio MCP bridge, or `mock_studio` in tests). Requires all seven tools up front (`get_place_tree`, `write_script`, `delete_script`, `create_instance`, `delete_instance`, `playtest`, `get_console`) with per-call timeouts (`OPENRO_BRIDGE_TIMEOUT_SECS`).
- **Rune Luau skill engine:** Ten embedded Rune scripts (`src/*.rn`) generate all Luau: `round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`, `currency` (leaderstats + DataStore load/save + `BindToClose`), `shop` (server-validated RemoteEvent purchases), `game_gui` (balance HUD + shop button), `checkpoints` (stage pads + respawn), and `soundtrack`. Arbitrary raw Luau outside registered skills is refused, and skill arguments are strictly type-validated in Rust.
- **Playtest gate, repair loop & checkpointed Undo:**
  - Every ask records an identity-checked change set in `changes.json` so `POST /api/revert` (`Undo`) restores only untouched scripts and instances.
  - `done` is refused until a playtest passes after the latest change (`proof.json` persists verdicts across restarts).
  - Failed playtests feed Studio console errors back into the model for up to 3 automatic repair attempts before reporting a plain failure.

## Persistence (`OPENRO_DATA_DIR`, default `openro-data`)

- `chats.json`: chat sessions, titles, thread lines, and active chat id.
- `changes.json`: per-chat change-set stacks for `Undo`.
- `proof.json`: per-chat playtest verdicts and reverted skill history.
- `ideas.json`: per-chat built skills and pitched blueprints.

Corrupt state files are preserved aside as `<name>.json.bak` (`bak2`, ...) and surfaced in `GET /api/status` (`store_recovered`) and a dismissible UI banner.

## HTTP API

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/` | Embedded single-file Studio Workbench UI |
| `GET` | `/api/version` | `{ "name": "openro-rs", "version": "0.3.0" }` |
| `GET` | `/api/status` | `studio_connected`, `model_configured`, `game_name`, `version`, `store_recovered` |
| `GET` | `/api/chats` | `chats`, `active`, and `can_undo` |
| `POST` | `/api/chats` | Create and select a new chat (`201 Created`) |
| `POST` | `/api/chats/select` | Switch active chat (`{"id": "..."}`) |
| `GET` | `/api/thread` | Thread lines for the active chat |
| `POST` | `/api/chat` | Run a natural-language Studio ask (`{"message": "..."}`) |
| `GET` | `/api/place-tree` | Current Studio `scripts` and `instances` map |
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

# 3. Run locally (http://127.0.0.1:8077)
OPENRO_MCP_COMMAND=./target/debug/mock_studio \
OPENRO_MODEL_API_KEY=... \
./target/debug/openro-rs
```

## Verification

```sh
# Rust unit + integration suite (17 unit + 90 integration = 107 tests)
cargo test

# Full Playwright UI & worst-case replay suite (1440px, 768px, 390px, 320px in Light & Dark)
cd frontend && npm run check:ui
```
