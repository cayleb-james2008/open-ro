# <img src="docs/logo.svg" alt="OpenRO Studio logo" width="28" height="28" align="absmiddle" /> OpenRO (`v1.22.0`)

[![Release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/cayleb-james2008/open-ro/release.yml?branch=main&label=build)](https://github.com/cayleb-james2008/open-ro/actions)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Stack](https://img.shields.io/badge/stack-Rust%20%2B%20React%2019%20%2B%20TypeScript-0f172a.svg)](#architecture)

OpenRO is a single-binary Roblox Studio IDE written in Rust with a React 19 interface. It builds, checks, executes headless Luau journeys, monetizes, and publishes Roblox games from Rojo project folders or an embedded Local Place engine, with an offline browser demo (`web-dist/index.html`) that runs the same workflows locally with no external network requests.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/hero-dark.png" />
    <img src="docs/screenshots/hero-light.png" alt="OpenRO three-pane Studio IDE showing geometric Studio icon, Explorer, Agent thread, and Viewport" width="100%" />
  </picture>
</p>

## What it does

- The visual ScreenGui and HUD designer (`src/gui_designer.rs`, `GuiDesignerWorkbench.tsx`, `/api/projects/:id/gui*`, `/api/studio/gui*`, and the `gui_save` and `gui_compile` tools) lets you drag and resize a 12-class Roblox UI hierarchy (`Frame`, `TextLabel`, `TextButton`, `ImageLabel`, `ImageButton`, `ScrollingFrame`, `UICorner`, `UIStroke`, `UIListLayout`, `UIGridLayout`, `UIAspectRatioConstraint`, `UIPadding`) over Desktop (`1920x1080`), Tablet (`1024x768`), and Mobile Landscape (`844x390`) viewports. Live overlays check the 58px TopBar inset, Mobile thumbstick and jump zones, offscreen bounds, and 44x44px tap targets, while `Compile to Luau` emits deterministic `--!strict` `ScreenModule.create(parent, callbacks)` scripts and Rojo `.model.json` files validated by `luau_check` and `stylua`.
- The headless local Luau runtime sandbox (`POST /api/projects/:id/sandbox/run`, `POST /api/studio/sandbox/run`, and the `sandbox_playtest` agent and teamwork tool) materializes your Rojo project or in-memory Local Place into an isolated scratch run and executes real Server and Client Luau scripts via Lune `0.11.1` (`lune_roblox_shim`) against an embedded Roblox `DataModel`, `DataStoreService`, `MarketplaceService.ProcessReceipt`, `RemoteEvent`, `RemoteFunction`, and `PlayerGui` shim. You can run Boot + Join, Purchase Receipt Idempotency, Save/Rejoin Persistence, or custom multi-step journeys and inspect console traces (with click-to-jump line links), DataStores, leaderstats, receipts, and PlayerGui trees without launching Roblox Studio, mirrored offline in the browser simulator (`browser_sim`).

- The three-pane workspace puts the Place explorer on the left, the Agent/Plan/Ask thread in the middle, and tabbed panels (`Viewport`, `Code`, `UI`, `Assets`, `Ship`, `Swarm`) on the right. Opening a Rojo project populates Explorer (`Services` and `Files`), `Code` (saving directly to `.luau` files on disk and jumping straight to reported line numbers), Lighting controls, and the 3D Viewport from `scene/workspace.model.json` via `/api/projects/:id/{workspace,scene/mutate,mesh-asset}` while `projects::builtin_rbxlx` natively serializes `CFrame`, `Enum`, `Content`, `Color3uint8`, and `Lighting` properties.
- The economy balance and progression simulator parses Luau earn, price, rebirth, and GamePass constants to project a deterministic 60-minute player progression curve, flagging paywall dead-ends, runaway inflation, or under-2-minute exhaustion with a 1-click balance suggestion preview and safe tuning apply.
- The monetization and retention safety audit checks Luau scripts for missing `MarketplaceService.ProcessReceipt` idempotency (`PurchaseId` ledger and `PurchaseGranted` return), missing `DataStore` `BindToClose` shutdown flushes, unguarded `PromptGamePassPurchase` or `UserOwnsGamePassAsync` calls without `pcall`, and missing first-60s onboarding or daily streak hooks, with one-click jump and safe snippet repair.
- Before exporting or publishing, the preflight Luau and asset readiness guard checks for orphan asset refs (`rbxassetid://0`, local file paths, or missing `AssetIds` keys), broken `RemoteEvent` and `require()` bindings, unsafe `loadstring`, `getfenv`, `setfenv`, or numeric `require()` calls, and empty spawn or shop hooks, with one-click Jump to Issue and Apply Safe Autofix actions.
- Before publishing, the Rojo live-sync conflict resolver compares local `.luau` files on disk against your synced Place baseline, shows line diffs, and lets you accept local changes, restore the Place version, or save a backup snapshot in one click.
- The preflight hook-integrity check catches dropped Robux GamePass or DevProduct IDs, `ProcessReceipt` handlers, `Workspace.SpawnLocation`, and `--!strict` headers against your last verified snapshot, restoring missing hooks without overwriting new feature scripts.
- Playable snapshots track script diffs and assertion deltas between checkpoints so you can roll back to a verified build after taking an automatic safety checkpoint.
- The dependency audit and monetization tools inspect `require()` edges, remove unused ModuleScripts or orphan remotes, run deterministic economy funnel projections (`ARPDAU`, `D1`/`D7` retention, GamePass conversion), and keep Robux pricing gated until an owner approves it (`priceApproved: false`).
- Deterministic playtest suites verify Luau syntax, runtime logs, core loop checks, and economy wiring, with one-click fixes that rebuild `.rbxl` and verify Ship Pack `.zip` archives.

## Feature gallery

<table>
  <tr>
    <td width="50%" align="center">
      <strong>Ship readiness autopilot and pre-publish gate</strong>
      <br /><br />
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/ship-readiness-dark.png" />
        <img src="docs/screenshots/ship-readiness-light.png" alt="OpenRO Ship Readiness Autopilot, 5-pillar score, and Pre-Publish Gate" width="100%" />
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>Monetization tier and 48h comeback retention designer</strong>
      <br /><br />
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/monetization-dark.png" />
        <img src="docs/screenshots/monetization-light.png" alt="OpenRO Monetization Tier and Retention Curve Designer" width="100%" />
      </picture>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <strong>Visual ScreenGui designer and safe-area simulator (`v1.22.0`)</strong>
      <br /><br />
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/hardening-dark.png" />
        <img src="docs/screenshots/hardening-light.png" alt="OpenRO v1.22.0 hardened Visual ScreenGui Designer and error-bounded workbench" width="100%" />
      </picture>
    </td>
    <td width="50%" align="center">
      <strong>Headless Luau runtime sandbox (`lune_roblox_shim`)</strong>
      <br /><br />
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/sandbox-runner-dark.png" />
        <img src="docs/screenshots/sandbox-runner-light.png" alt="OpenRO Headless Luau Runtime Sandbox executing multi-step journeys via Lune with Console, DataStores, leaderstats, and Receipts inspector" width="100%" />
      </picture>
    </td>
  </tr>
  <tr>
    <td colspan="2" align="center">
      <strong>Unified Rojo workspace, 3D scene graph, and lighting preview</strong>
      <br /><br />
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/unified-workspace-dark.png" />
        <img src="docs/screenshots/unified-workspace-light.png" alt="OpenRO Unified Rojo Explorer and Declarative 3D Scene Preview" width="60%" />
      </picture>
    </td>
  </tr>
</table>

## Architecture

```mermaid
flowchart LR
  UI["3-Pane React 19 UI<br/>(gz shell + pre-gz /chunks/*.js & .woff2)"] -->|HTTP + SSE| SRV["openro-rs Server<br/>(Rust, Loopback Guard, Mallopt)"]
  SRV --> LP["Unified Workspace &amp; Scene Graph<br/>scene/workspace.model.json + .rbxlx + CWE-22 /mesh-asset"]
  SRV --> GD["ScreenGui Designer &amp; Compiler<br/>12-Class Allowlist + Safe-Area + --!strict Luau"]
  SRV --> SB["Headless Luau Runtime Sandbox<br/>Lune 0.11.1 DataModel/DataStore/Receipt Shim"]
  SRV --> PJ["Rojo Project &amp; Ship Pack<br/>ZIP CRC-32 + Playtest Assertions"]
  SRV --> MCP["Roblox Studio MCP &amp; Open Cloud<br/>13 Live Tools + Price Approval Gate"]
```

## Verified benchmarks (`v1.22.0` vs `v1.2.0`)

| Metric | `v1.2.0` baseline | `v1.22.0` verified (`#154` pinned) | Delta |
| --- | --- | --- | --- |
| Release binary size | `19.00 MB` | **`9.53 MiB`** (`9,995,128 B`) | **`-49.8%`** |
| Initial `/` payload | `~1,368 KB` raw / `~459 KB` gz | **`229.9 KB`** raw (`235,427 B`) / **`55.3 KB`** gz (`56,631 B`) | **`-88.0%` wire** |
| Cold boot (`#154` pinned) | `1,470 ms` | **`29.28 ms`** | **`-98.0%`** |
| Peak RSS (`VmHWM`) | `115.20 MB` | **`13.05 MB`** (`13,368 KB`, `scale-audit` + 10 SSE) | **`-88.7%`** |
| Concurrent HTTP (`16w`) | `875 req/s` (`141.7 ms` p95) | **`5,438.0 req/s`** Python cold (`2.72 ms` p50, `4.23 ms` p95, `0/13,603` errs) | **`6.21x` (`-97.0%` p95)** |
| Security and UI gates | `43/43` fuzz | **`43/43` fuzz, `10/10` SSE, `0` sloplint fails/warns, `339/339` Rust tests (`4/4` `v122_cycle2_hardening`), `gate 110/110`, `replay 11/11`** | Pass |

## Quick start

```sh
# 1. Build the embedded single-file frontend and Rust release binary
npm --prefix frontend run build
touch src/lib.rs src/server.rs && cargo build --release --locked

# 2. Launch OpenRO (opens chromeless Studio window at http://127.0.0.1:8077; use --no-open for headless)
./target/release/openro-rs --no-open --port 8077

# 3. Verify Rust tests, Windows cross-check, and UI gates
cargo test -- --test-threads=1
cargo xwin check --target x86_64-pc-windows-msvc
npm --prefix frontend run check:ui -- --lints --skip-build
```

## Platform notes

OpenRO builds for Linux (`x86_64`, `aarch64`, `Xvfb`/Vinegar Studio embed), Windows (`x86_64-pc-windows-msvc`), and macOS (`aarch64-apple-darwin`). API references live in [Projects, Sync & Playtest](docs/API-V011.md), [Multi-Agent Teamwork](docs/API-V011-teamwork.md), [Creator Hub & Open Cloud](docs/API-V011-creator-hub.md), [Assets & Mesh](docs/API-V011-assets.md), [Study Ledger](docs/API-V011-study.md), and [Tool Adapters](docs/API-V011-adapters.md).
