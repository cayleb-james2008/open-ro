# OpenRO (`v0.4.0` — Rust + Rune + Local Place Engine + House Frontend)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

Describe a Roblox game in plain words or pick a Studio blueprint — OpenRO generates Luau from verified Rune skills, builds into a persistent local Roblox place (`game.rbxlx`) or live Roblox Studio over MCP, proves every build with a playtest before calling it done, exports genuine `.rbxlx` XML for Roblox Studio, and reverts the last build in one click.

## Architecture

- **Single-binary Rust server (`openro-rs`):** Serves the embedded single-file React + Tailwind 4 **Game Builder Studio Workbench** (`assets/index.html`, with JS, CSS, and self-hosted `Geist` + `Geist Mono` `.woff2` fonts inlined via `include_str!`) over `std::net::TcpListener`, alongside the JSON/XML API.
  - **Left Studio Session Rail (`256px` desktop, Vaul drawer `<768px`):** Game place header, live backend status pill (`Studio connected` / `Local Place ready` / `Studio not connected` + `Check again`), primary `New chat`, `/` search filter across game sessions, `AnimatedBackground` chat list, **Settings (`Model, Studio & Appearance`)** `MorphingDialog` (housing Light/Dark `SegmentedControl`, local gateway auto-detection, live Model & MCP config), and release version link.
  - **Center Studio Stage, Playable Preview & Build Trace:** Equal-height **Game Blueprint Starter Cards** (`Lava Obby`, `Coin Tycoon`, `Round Arena`, plus follow-up feature blueprints) with category tags, deterministic part/script count chips, `"Adds to Studio"` capability pills, `"Already in your game"` tracking, and one-click builds; interactive HTML5 Canvas **Playable Stage Preview** synced to placed Workspace objects; active **Studio Build Trace** with Tremor `ProgressCircle`, inline `SlidingNumber` change counts, **Playtest Verdict** banner (`Passed` / `Failed` / `Not proven yet`) with expandable Luau console logs, and hairline Composer Dock (`Enter` / `Shift+Enter`, `Undo` `MorphingDialog` confirmation, and self-update strip).
  - **Right "In Your Game" Studio Explorer Shelf (`300px` `>=1100px`, Vaul bottom drawer `<1100px`):** Familiar Roblox Studio DataModel hierarchy grouping `Workspace` parts and `ServerScriptService` / `StarterGui` / `StarterPlayer` Luau scripts with `SlidingNumber` totals, quick filter, full copyable Luau source code & Instance property viewers, pinned **Export .rbxlx** download button, and **Copy game summary**.
- **Model route (`src/model.rs`):** OpenAI-compatible `POST {base}/chat/completions`. Loopback gateways (`127.0.0.1`, `localhost`, `[::1]`) work without an API key, and when `OPENRO_MODEL_BASE_URL` and `OPENRO_MODEL_API_KEY` are unset (and `OPENRO_NO_AUTO_MODEL` is not `1`), `openro-rs` auto-probes running local gateways (`127.0.0.1:11434/v1`, `8789/v1`, `8790/v1`). Remote endpoints still require an API key and fail closed when unconfigured. Settings persist live to `data_dir/config.json` (`0600`) via loopback-guarded `GET/POST /api/config`.
- **Real Local Place Engine + Studio MCP auto-detect (`src/place.rs`, `src/mcp.rs`):** Connects to `OPENRO_MCP_COMMAND` when explicitly set or passively detects a running Vinegar Flatpak Roblox Studio instance with `~/.local/bin/roblox-studio-mcp` (never launching Vinegar unprompted). When Studio is not open, OpenRO runs on its built-in persistent Local Place Engine (`data_dir/place.json` + atomic `data_dir/game.rbxlx` `<roblox version="4">` XML) with real Luau lexer/syntax, service-root, and runtime-wiring playtest verification.
- **Rune Luau skill engine:** Ten embedded Rune scripts (`src/*.rn`) generate all Luau: `round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`, `currency` (leaderstats + DataStore load/save + `BindToClose`), `shop` (server-validated RemoteEvent purchases), `game_gui` (balance HUD + shop button), `checkpoints` (stage pads + respawn), and `soundtrack`. Arbitrary raw Luau outside registered skills is refused, and skill arguments are strictly type-validated in Rust.
- **Playtest gate, repair loop & checkpointed Undo:**
  - Every ask records an identity-checked change set in `changes.json` so `POST /api/revert` (`Undo`) restores untouched scripts, instances, and `game.rbxlx` atomically.
  - `done` is refused until a playtest passes after the latest change (`proof.json` persists verdicts across restarts).
  - Failed playtests feed Studio console errors back into the model for up to 3 automatic repair attempts before reporting a plain failure.

## Persistence (`OPENRO_DATA_DIR`, default `openro-data`)

- `place.json` & `game.rbxlx`: live local Roblox place tree and genuine Roblox Studio XML file.
- `config.json`: loopback-managed Model provider & Studio backend settings (`0600` on Unix).
- `chats.json`: chat sessions, titles, thread lines, and active chat id.
- `changes.json`: per-chat change-set stacks for `Undo`.
- `proof.json`: per-chat playtest verdicts and reverted skill history.
- `ideas.json`: per-chat built skills and pitched blueprints.

Corrupt state files are preserved aside as `<name>.json.bak` (`bak2`, ...) and surfaced in `GET /api/status` (`store_recovered`) and a dismissible UI banner.

## HTTP API

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/` | Embedded single-file Studio Workbench UI |
| `GET` | `/api/version` | `{ "name": "openro-rs", "version": "0.4.0" }` |
| `GET` | `/api/status` | `studio_connected`, `model_configured`, `workspace_mode`, `model_id`, `game_name`, `version`, `store_recovered` |
| `GET` | `/api/config` | Loopback-only Model & Studio runtime config + detected local gateways |
| `POST` | `/api/config` | Loopback-only live Model & Studio settings update (saved to `config.json`) |
| `GET` | `/api/export/rbxlx` | Downloads genuine Roblox Studio XML (`<roblox version="4">`) for the current place |
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

# Optional: build the self-contained browser web app (Parametric Luau Compiler + Stage Preview + .rbxlx export)
cd frontend && npm run build:web
```

## Verification

```sh
# Rust unit + integration suite (21 unit + 92 integration = 113 tests)
cargo test

# Full Playwright UI & worst-case replay suite (1440px, 768px, 390px, 320px in Light & Dark)
cd frontend && npm run check:ui
```
