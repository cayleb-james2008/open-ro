# OpenRO (`v1.4.0`)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)

OpenRO is a **single-binary cross-platform Roblox Studio AI IDE** (`openro-rs`) for building, verifying, and publishing complete Roblox games end-to-end — from Rojo project scaffolding, multi-agent Teamwork, reference-guided art & Blender 3D assets, and live Studio playtesting with GUI click actions, all the way to browser-backed Creator Hub setup and Roblox Open Cloud asset, product, and place publishing.

## Core Capabilities

- **3-Pane Studio Copilot IDE (`frontend/src/`, embedded single-file `assets/index.html`):**
  - **Left Rail:** Rojo Projects manager (create/open/build/diff/sync, 1-click Studio attach/transfer), auto-synced Studio Explorer, chat sessions, Tool & Adapter Library, Live Companion Plugin status, and **Settings** (housing the Light/Dark theme switch, model/provider selector, Studio MCP setup, and Roblox Open Cloud settings).
  - **Center Workspace (`Studio | Swarm | Code | Ship`):**
    - **Embedded Studio & Studio 3D:** Zero-manual-step Roblox Studio transfer and live 12 FPS viewport stream (`Xvfb :99` + Vinegar on Linux, native Roblox Studio on Windows/macOS) with pointer/keyboard pass-through, Start Page `place_open` detection, all 13 official Roblox Studio MCP tools, plus a built-in Local Place Engine + 3D viewport when Studio is closed.
    - **Swarm (Antigravity-Style Multi-Agent Teamwork):** Live workflow diagram (`Coordinator -> Roles -> Verifier`), compact task & file-claim table, append-only `board.jsonl`, `@mention` targeted wakeups, quota-aware concurrency scheduler, Verifier Gate (Playtest Runner `0` console errors for game code, Lune pass for `tools/**/*.luau`), and scoped provenance git commits (`openro(<role>): ...`).
    - **Code & Diff Workbench:** Full-width Luau editor with inline Static Luau Prover, `selene`, `stylua` auto-formatting (`POST /api/projects/:id/format` & `/check { format: true }`), unified diff proposals, and checkpointed Undo/Redo.
    - **Ship Workspace (`Publish | Assets | Art | Playtests`):** Visual-first 4-stage release pipeline, human price-approval monetization table, filterable asset gallery with upload progress bar, categorized Art Studio (`Cards | Weapon Sheets | Store | Refs`) with 1-click Approve/Redraw/Pack, and Playtest screenshot filmstrip + Verification Matrix (`Study -> Build -> Check -> Ready`).
  - **Right Sidebar (`Swarm | Agent | Plan`):** Prompt-first composer with inline adaptive Gen-UI cards (art/weapon previews, playtest `MOVE`/`ACT`/`HUD` summaries, Creator Hub action cards), live collapsible Swarm task checklist, `@Context` live Studio selection chips, and collapsible Agent Run Queue.

- **Creator Hub CDP Automation & Roblox Open Cloud Pipeline (`src/creator_hub.rs`, `src/opencloud.rs`):**
  - Loopback-only Chromium CDP automation (`GET /api/creator-hub/status`, `POST /api/projects/:id/creator-hub/{setup,configure}`) using a dedicated tab, zero `.ROBLOSECURITY` cookie reads, atomic `0600` temp-file API key handoff into `SecretStore`, fail-closed `action_required` stops on login/2FA/CAPTCHA, auto-linked `universeId`/`placeId`, and dashboard setup (Private Servers, Game Icon, Thumbnails, Content Rating).
  - Open Cloud uploads with moderation batch circuit-breaker (`403 User is moderated`), transparent `.wav -> .ogg` (`ffmpeg`) & `.obj -> .fbx` (`blender`) conversion, Decal->Image XML resolution, incremental `assets.lock.json`, `AssetIds.luau` + `ProductIds.luau` codegen, human price-approved Game Pass & Developer Product sync, and `confirm: true`-gated place publishing.
  - All 11 cloud/Creator Hub tools are exposed cleanly to both Agent and Teamwork modes.

- **Reference Search, Roblox-Safe Art, Weapon Sheets, Blender 3D & Audio (`src/assets.rs`, `src/reference_search.rs`, `src/mesh.rs`, `src/audio_synth.rs`, `src/tool_adapters.rs`):**
  - Dedicated character action-shot and isolated prop/weapon reference ranking (`kind: "character" | "weapon" | "prop"`) with `auto_reference` sourcing for orthographic `weapon_sheet` generation (`art/weapons/<slug>.png`).
  - Reference-guided card art generation with automatic Roblox-safe zero-blood/zero-gore and zero-tobacco prompt guardrails, exit-9 visual review approval (`/api/art/approve`), Pillow card packing (`art/pack-card`), headless Blender UV-unwrapped Cycles AO-baked meshes (20k-tri cap), pure-Rust 44.1 kHz PCM16 `.wav` SFX synthesis, and declarative Universal CLI/HTTP Tool Adapters (`playwright-cdp-cli`, `craft-anything-cli`, `rea-cli`, ComfyUI, fal.ai, Meshy, ElevenLabs).

- **Study & Evidence Ledger + Automated Playtest Runner (`src/study_check.rs`, `src/playtest.rs`):**
  - Canonical `<project>/.openro/investigation/` ledger (`evidence.jsonl`, optimistic-revision `unknowns.json` with fail-closed resolution, `clean_room` mode), `mechanics.json` provenance checker, and `KeptChecks` SHA-256 caching (never caches failures).
  - Live Roblox Studio Playtest Runner with native StudioMCP `instance_path` GUI clicking (`click_gui` on `Client` DataModel), keyboard/text input actions, core-loop claims (`move`/`act`/`hud`), blank/single-color PNG rejection, `640x360` visual likeness scoring, and JSON/HTML reports.

- **Flagship Project Built 100% With OpenRO (`projects/untitled-anime-card-game/`):**
  - **Untitled Anime Card Game** — built end-to-end by OpenRO Teamwork (54-card 5-world roster, 54/54 live Roblox CDN `Completed` portraits, 24 prop-backed weapon sheets, 4 Blender meshes, 6 audio cues, 22 live Game Passes & Developer Products, 14 server services, golden-hour Nexus Plaza Hub, verified live in Roblox Studio with `0` console errors/warnings).

## Cross-Platform Support & Persistence

- **Platforms:** Linux (`x86_64` & `aarch64`, chromeless desktop launcher + `Xvfb`/Vinegar Studio embed), Windows (`x86_64-pc-windows-msvc`, native Studio + `msedge --app`), and macOS (`aarch64-apple-darwin`).
- **Model Providers:** Antigravity CLI (`agy`, default `gemini-4-hydrogen-high`), local loopback OpenAI-compatible gateways (`127.0.0.1` / `localhost`, zero key required), or configured remote OpenAI-compatible endpoints.
- **Data & Secrets:** Runtime state persists in `OPENRO_DATA_DIR` (`--data-dir`, default `openro-data`); credentials persist separately in `OPENRO_SECRETS_DIR` (`0700` dir / `0600` files, strictly masked and scrubbed from all responses and logs).

## Versions and updates

Each release is tagged `vMAJOR.MINOR.PATCH` and published via GitHub Releases with pre-built binaries for Linux (`x86_64`, `aarch64`), macOS (`aarch64`), and Windows (`x86_64`) alongside a `SHA256SUMS` manifest. OpenRO checks `GET /api/update` passively (no unsolicited network calls), and loopback users can trigger `POST /api/update/{check,apply,restart}` to verify checksums, probe `--version`, and stage updates cleanly.

## Build & Run

```sh
# 1. Build single-file frontend bundle into assets/index.html (when editing frontend/)
npm --prefix frontend run build
touch src/lib.rs src/server.rs

# 2. Build Rust release binary
cargo build --release

# 3. Launch OpenRO (opens desktop app at http://127.0.0.1:8077; pass --no-open for headless)
./target/release/openro-rs
```

## Verification & API Docs

```sh
# Rust clippy + full unit/integration suite
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=4

# Windows cross-check (Linux host with cargo-xwin)
cargo xwin check --target x86_64-pc-windows-msvc
```

Detailed API references live in `docs/`:
- `docs/API-V011.md` (Projects, Sync, Playtest Runner & `click_gui` Actions)
- `docs/API-V011-teamwork.md` (Multi-Agent Teamwork Mode)
- `docs/API-V011-creator-hub.md` (Creator Hub CDP Automation & Open Cloud Upgrades)
- `docs/API-V011-assets.md` & `docs/API-V011-mesh.md` (Reference Search, Art, Weapon Sheets, Blender & Audio)
- `docs/API-V011-study.md` (Study Ledger, Mechanics Provenance & `KeptChecks`)
- `docs/API-V011-adapters.md` (Universal CLI & HTTP Tool Adapters)
