# OpenRO (`v1.16.0`)

[![Release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/cayleb-james2008/open-ro/release.yml?branch=main&label=build)](https://github.com/cayleb-james2008/open-ro/actions)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Stack](https://img.shields.io/badge/stack-Rust%20%2B%20React%2019%20%2B%20TypeScript-0f172a.svg)](#architecture)

OpenRO is a single-binary Roblox Studio IDE written in Rust with a React 19 interface. It builds, checks, monetizes, and publishes Roblox games from Rojo project folders or an embedded Local Place engine, with an offline browser demo (`web-dist/index.html`) that runs the same workflows locally with no external network requests.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/hero-dark.png" />
    <img src="docs/screenshots/hero-light.png" alt="OpenRO three-pane Studio IDE showing Explorer, Agent thread, and Luau code editor" width="100%" />
  </picture>
</p>

## What it does

- The three-pane workspace puts the Place explorer on the left, the Agent/Plan/Ask thread in the middle, and tabbed panels (`Viewport`, `Code`, `Assets`, `Ship`, `Swarm`) on the right.
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
</table>

## Architecture

```mermaid
flowchart LR
  UI["3-Pane React 19 UI<br/>(56 KB gz shell + pre-gz /chunks/*.js & .woff2)"] -->|HTTP + SSE| SRV["openro-rs Server<br/>(Rust, Loopback Guard, Mallopt)"]
  SRV --> LP["Local Place Engine<br/>.rbxlx + Static Luau Prover"]
  SRV --> PJ["Rojo Project &amp; Ship Pack<br/>ZIP CRC-32 + Playtest Assertions"]
  SRV --> MCP["Roblox Studio MCP<br/>13 Live Tools + Playtest Runner"]
  SRV --> OC["Open Cloud &amp; Creator Hub<br/>0600 Secrets + Price Approval Gate"]
```

## Verified benchmarks (`v1.16.0` vs `v1.2.0`)

| Metric | `v1.2.0` baseline | `v1.16.0` verified (`#154` pinned) | Delta |
| --- | --- | --- | --- |
| Release binary size | `19.00 MB` | **`9.06 MB`** (`9,499,480 B`) | **`-52.3%`** |
| Initial `/` payload | `~1,368 KB` raw / `~459 KB` gz | **`236.7 KB`** raw (`242,332 B`) / **`56.4 KB`** gz (`57,799 B`) | **`-87.7%` wire** |
| Cold boot (`#154` pinned) | `1,470 ms` | **`32.01 ms`** | **`-97.8%`** |
| Peak RSS (`VmHWM`) | `115.20 MB` | **`9.48 MB`** (`9,708 KB`, 16w) / **`13.79 MB`** (`14,120 KB`, `scale-audit` + 10 SSE) | **`-91.8%`** |
| Concurrent HTTP (`16w`) | `875 req/s` (`141.7 ms` p95) | **`65,014.6 req/s`** keep-alive (`0.44 ms` p95) / **`59,340.3 req/s`** close (`0.50 ms` p95) / **`8,088.9 req/s`** Python cold (`2.47 ms` p95, `0` errs) | **`74.3x` (`-98.3%` p95)** |
| Security and UI gates | `43/43` fuzz | **`36/36` fuzz, `10/10` SSE, `0` sloplint fails/warns, `314` Rust tests, `gate 56/56`, `replay 9/9`** | Pass |

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
