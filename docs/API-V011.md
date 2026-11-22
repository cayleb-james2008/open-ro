# OpenRO v0.11 API: projects, sync, playtest runner, Open Cloud

Contract for `@openro-ui` and the game workers. Everything below is served by `openro-rs`
(loopback only: a non-loopback `Host` gets `403`). Request bodies are JSON
(`Content-Type: application/json`). Every reply is JSON `{ "ok": bool, ... }`; failures carry
`"error": "<plain words>"`. Status codes: `400` bad input, `404` unknown project/run, `409` busy
or conflict, `502` Studio or Open Cloud failed, `503` Studio not connected / no Open Cloud key.

`{id}` is the project id (slug of its folder name, from `GET /api/projects`).
Paths inside a project are always **relative to the project folder** and must stay inside it
(`..`, absolute paths and symlinks that leave the folder are refused with `400`).

## 1. Projects

A project is a folder with a Rojo layout (`default.project.json`) and, optionally, the manifest
`openro.project.json`:

```json
{
  "version": 1,
  "name": "Untitled Anime Card Game",
  "rojoProject": "default.project.json",
  "universeId": 123456789,
  "placeId": 987654321,
  "products": [
    { "key": "luck_potion_1", "kind": "developer_product", "name": "Luck Potion", "description": "...",
      "priceRobux": 49, "forSale": true, "icon": "art/icons/luck.png", "id": null },
    { "key": "vip", "kind": "game_pass", "name": "VIP", "description": "...", "priceRobux": 399,
      "forSale": true, "icon": "art/icons/vip.png", "id": null }
  ],
  "assets": {
    "lockFile": "assets.lock.json",
    "authorised": ["art/cards/**", "art/ui/*.png"],
    "types": { "art/audio/**": "Audio" }
  },
  "playtest": { "suite": "playtest/suite.json" }
}
```

`products[].id` is written back by `products/sync`. `assets.authorised` is the explicit set of
files OpenRO may upload by default (globs: `*` one path segment, `**` any depth).

| Method + path | Body | Reply |
|---|---|---|
| `GET /api/projects` | | `{ roots: [path], active: id\|null, projects: [Summary] }` |
| `POST /api/projects/workspace` | `{ path }` | adds a folder whose children are projects; `{ roots }` |
| `POST /api/projects/workspace/remove` | `{ path }` | `{ roots }` |
| `POST /api/projects/create` | `{ name, parent?, universeId?, placeId? }` | creates the Rojo layout + manifest; `{ project: Summary }` (`201`) |
| `POST /api/projects/open` | `{ path }` | registers and activates a folder that has `default.project.json` or the manifest; `{ project: Summary }` |
| `GET /api/projects/{id}` | | `{ project: Detail }` |
| `POST /api/projects/{id}/manifest` | partial manifest | shallow-merges top-level keys, rewrites the file; `{ manifest }` |
| `POST /api/projects/{id}/build` | `{ engine?: "auto"\|"rojo"\|"builtin", out? }` | `Build` (below) |
| `GET /api/projects/{id}/builds` | | `{ builds: [Build] }` newest first |

`Summary` = `{ id, name, path, universeId, placeId, has_manifest, has_rojo, last_build: Build|null }`.
`Detail` = Summary + `{ manifest, tree: { instances, scripts, services: [..] }, skipped: [..], assets: { total, authorised, uploaded }, products: [..] }`.

`Build` = `{ build_id, engine: "rojo"|"builtin", path (relative), bytes, sha256, instances, scripts, warnings: [..], duration_ms, at }`.
`engine: "auto"` uses `rojo build` when `rojo` is on PATH (or in `~/.local/bin`), otherwise the built-in
writer (scripts, folders and plain properties only; models/enums need rojo, and the build says so in `warnings`).
Output default: `build/<slug>.rbxlx`. Each build also writes `build/builds/<build_id>.json`.

## 2. One-way sync to Studio

Project to Studio, never the reverse. Needs a connected Studio bridge (`503` otherwise).
Reads and writes go through StudioMCP `execute_luau` in chunks, with each reply capped at
60,000 characters (StudioMCP truncates at 100,000). The walk covers structural services first
(`ReplicatedStorage`, `ServerScriptService`, `ServerStorage`, `StarterGui`, `StarterPlayer`,
`StarterPack`, `ReplicatedFirst`, `Lighting`, `SoundService`, `Teams`) and `Workspace` last.
Instance paths here use `/` (`ServerScriptService/Game/Main`).

| Method + path | Body | Reply |
|---|---|---|
| `POST /api/projects/{id}/sync/diff` | `{}` | read-only. `{ summary: { create, update, unchanged, extra }, changes: [{ op: "create"\|"update"\|"extra", path, class, reason }], chunks }` |
| `POST /api/projects/{id}/sync` | `{ prune?: false, dry_run?: false }` | diff, then apply. `{ summary, applied: { created, updated, deleted }, errors: [..], chunks, verify: { remaining } }` |

`update` means the class, a property or a script's source differs. `extra` = instances that exist
in Studio under a project-owned folder but not in the project; they are only deleted when
`prune: true`. After applying, the sync re-reads Studio and reports `verify.remaining` (a clean
sync is `0`). A `Workspace` write only touches instances the project declares.

## 3. Playtest runner

`POST /api/projects/{id}/playtest`

```json
{
  "suite": "playtest/suite.json",
  "mode": "start_play",
  "settle_ms": 1500,
  "sync_first": false,
  "build_id": "b-1700000000000",
  "checks": [
    { "name": "cards table loads", "luau": "return #require(game.ReplicatedStorage.Cards).list", "datamodel": "Server",
      "expect": { "equals": 40 } },
    { "name": "remote exists", "luau": "return game.ReplicatedStorage:FindFirstChild('Roll') ~= nil", "expect": { "truthy": true } }
  ],
  "screenshots": [ { "name": "hub", "camera_position": [0, 40, 60], "camera_look_at": [0, 0, 0] } ],
  "allow_errors": ["Infinite yield possible"]
}
```

`suite` (optional) is a project-relative JSON file with the same `checks` / `screenshots` /
`allow_errors` / `mode` / `settle_ms`; request fields override it. `expect` is one of
`{ "equals": v }`, `{ "truthy": true }`, `{ "contains": "text" }`, `{ "matches": "regex-free substring" }`
(omitted = the check passes when the Luau runs without error). `luau` runs through
`execute_luau` in the given `datamodel` (`Server` default, `Client`, `Edit`) while Studio is playing.

The run: optional sync, `start_stop_play` start, wait `settle_ms`, run each check, capture each
screenshot (`screen_capture`), read `get_console_output`, `start_stop_play` stop. Console lines that
look like errors (`error`, `attempt to`, `is not a valid member`, `Stack Begin`) and are not matched by
`allow_errors` fail the run. Reply:

```json
{ "ok": true, "run_id": "p-1700000000123", "passed": false, "build_id": "b-...",
  "mode": "start_play", "duration_ms": 4210,
  "checks": [ { "name": "...", "passed": true, "value": 40, "error": null, "datamodel": "Server" } ],
  "console": { "lines": 31, "errors": ["..."], "warnings": ["..."] },
  "screenshots": [ { "name": "hub", "file": "shots/hub.png", "bytes": 48211, "width": 1280, "height": 720 } ],
  "report": { "json": "build/reports/<build_id>/p-.../report.json", "html": "build/reports/<build_id>/p-.../report.html" },
  "urls": { "json": "/api/projects/{id}/playtests/p-...", "html": "/api/projects/{id}/playtests/p-.../report.html" } }
```

`ok` is false only when the run could not happen (no Studio, bridge error); a failing check or a
console error gives `ok: true, passed: false`. Files go to
`<project>/build/reports/<build_id>/<run_id>/{report.json,report.html,shots/*.png}`
(`build_id` is the latest build, or `unbuilt`).

| Method + path | Reply |
|---|---|
| `GET /api/projects/{id}/playtests` | `{ runs: [{ run_id, build_id, passed, at, checks, errors }] }` newest first |
| `GET /api/projects/{id}/playtests/{run}` | the stored `report.json` |
| `GET /api/projects/{id}/playtests/{run}/report.html` | the HTML report (`text/html`) |
| `GET /api/projects/{id}/playtests/{run}/shots/{file}` | PNG |

## 4. Open Cloud, assets, publish, products, universe settings

Owned by `@openro-assets`; see [API-V011-assets.md](API-V011-assets.md). Their routes sit under
`/api/opencloud*` and `/api/projects/{id}/{assets,publish,products,universe}`; the project routes
above deliberately do not match those paths, so the two tables never overlap.

## 5. Project files (the agent's tools)

Read and write files inside a project folder, so OpenRO's agent (and the UI) never needs a path
outside it. Same loopback and path rules as above. Hidden `.git/` and `.openro/` are blocked for
both reads and writes; `build/`, `openro.project.json` (use `/manifest`) and `*.lock.json` are not
writable here.

| Method + path | Body / query | Reply |
|---|---|---|
| `GET /api/projects/{id}/files?dir=src` | `dir` project-relative, default root | `{ dir, entries: [{ path, type: "file"\|"dir", bytes }] }` |
| `GET /api/projects/{id}/file?path=src/server/Main.server.luau` | | `{ path, content, bytes, sha256 }` (text up to 1 MB) |
| `POST /api/projects/{id}/file` | `{ path, content, expect_sha256? }` | `{ path, created, bytes, sha256 }`; creates folders; `409` if `expect_sha256` no longer matches |
| `POST /api/projects/{id}/file/edit` | `{ path, edits: [{ old, new }], expect_sha256? }` | each `old` must occur exactly once or nothing is written (`409`); `{ path, edits, bytes, sha256 }` |
| `POST /api/projects/{id}/file/delete` | `{ path }` | `{ path, deleted: true }` |
| `POST /api/projects/{id}/check` | `{}` | runs Luau static analyzer + `selene` + `stylua --check`; `{ passed, luau, selene, stylua }` |
| `POST /api/projects/{id}/run-lune` | `{ script, args?: [...], timeout_secs?: 30 }` | timeout-bounded `lune run <script>`; `{ script, passed, exit_code, timed_out, duration_ms, stdout, stderr }` |
| `GET /api/projects/{id}/preview?path=art/cards/a.png` | `path` project-relative image | raw `image/png\|jpeg\|webp\|gif` (up to 10 MB) for OpenRO UI preview |

Manifest price-approval gate: `POST /api/projects/{id}/manifest` with `"actor": "agent"` (or Rust
`projects::merge_manifest_for_actor(current, patch, false)`) forces `products[].priceApproved = false`
on any new or price-changed product; only human UI calls (`actor` omitted) can approve prices.

Agent tool contract (names for `@openro-agent` to wire; each calls the route on the right):
`project_list` (`GET /api/projects`), `project_files` (`GET .../files`), `project_read_file` (`GET .../file`),
`project_write_file` (`POST .../file`), `project_edit_file` (`POST .../file/edit`),
`project_delete_file` (`POST .../file/delete`), `project_update_manifest` (`POST .../manifest` with `"actor": "agent"`),
`project_check` (`POST .../check`), `project_run_lune` (`POST .../run-lune`),
`project_build` (`POST .../build`), `project_sync` (`POST .../sync`, run after edits),
`project_playtest` (`POST .../playtest`, run after a sync; read `console.errors` and failed `checks` to repair).
A step that changes files should end with check, sync, then playtest.

## 6. Project git (commit-per-task & revert-a-task)

Local git operations strictly scoped to the project folder (never pushes, never touches files
outside the project, skips `.git/`, `.openro/`, `build/`, `art/references/`, `refs/`, and refuses
files over 5 MB).

| Method + path | Body / query | Reply |
|---|---|---|
| `GET /api/projects/{id}/git/status` | | `{ is_git_repo, branch, head, dirty, changed: [{ status, path }] }` |
| `GET /api/projects/{id}/git/log?limit=20` | `limit` 1..100 | `{ commits: [{ commit, short, at, message }] }` |
| `POST /api/projects/{id}/git/commit` | `{ message, files?: [...] }` (`paths` alias accepted) | `{ committed: bool, commit: sha\|null, short?, message?, note? }` |
| `POST /api/projects/{id}/git/revert` | `{ commit }` | `{ reverted: true, original, commit, short }`; `409` on conflict (aborted cleanly) |

Rust hook for `@openro-team`: `projects::git_commit(dir: &Path, files: &[String], message: &str) -> Result<Option<String>, String>` and `projects::git_revert(dir: &Path, commit: &str) -> Result<String, String>`.

## 7. Dev content preview (decision)

Local project images are previewed in the OpenRO UI via `GET /api/projects/{id}/preview?path=...`.
Studio-side local PNG preview before Open Cloud upload is intentionally omitted: Roblox Studio
refuses arbitrary filesystem paths, `rbxasset://` requires writing inside Studio's version-hashed
install directory (fragile across updates and read-only in Flatpak), and `EditableImage` is
runtime-only, ID-verified gated, and non-persistent in `.rbxlx`. Real Studio textures use uploaded
`rbxassetid://` entries from `assets.lock.json`.

## 8. Studio connection, live embedded auth, Start Page detection & grabbing existing Studio (#24, #29, #35, #47, #118)

- Auto backend (`workspace_mode: "auto"`) only enters `studio_mcp` when `list_roblox_studios`
  returns at least one open Studio place (`studio_id`); otherwise it falls back cleanly to
  `local_place`. Explicit `POST /api/studio/mcp/connect` returns `502` with plain words when no
  Studio place is open.
- `GET /api/config`, `POST /api/config`, and `GET /api/studio/plugin/status` expose
  `auto_install_plugin` (boolean, default `true`, persisted in `studio_prefs.json`). On startup
  with Vinegar detected, OpenRO auto-installs `OpenRoCompanion.lua` into the verified `Plugins/`
  dir if not yet present (`OPENRO_NO_AUTO_PLUGIN=1` disables).
- `GET /api/studio/embed/status` (passive, never launches Studio; cached 2s):
  - Top-level detection & session fields:
    - `studio_running`: `bool` (Linux `/proc` scan for `RobloxStudioBeta.exe`/Vinegar, Windows `tasklist`, macOS `pgrep`, or active embedded session)
    - `studio_display`: `string | null` (actual X11 `DISPLAY` of running Studio on Linux, e.g. `":0"`, `":1"`, `":99"`; `null` on Windows/macOS)
    - `already_logged_in`: `bool` (`true` when `running_studios` is non-empty or live MCP is connected)
    - `place_open`: `bool` (`true` only when passive `get_studio_state` confirms a place is actually open; `false` when Studio is on the Start Page with `"Place is not open"` / `mode: "Unavailable"`)
    - `guidance`: plain user guidance (shows `"Studio is on the Start Page — open a blank Baseplate to sync/playtest."` when `already_logged_in && !place_open`)
    - `running_studios`: `[{ studio_id, place_name, datamodel_type, place_open, studio_state }]`
    - `phase`: `"idle" | "starting" | "awaiting_signin" | "connected" | "stopped" | "error"`
    - `state`: `"idle" | "launching" | "awaiting_signin" | "running" | "error"`
    - `awaiting_signin`: `bool` (`phase == "awaiting_signin"`)
    - `running`: `bool` (`true` while streaming in `"awaiting_signin"` or `"connected"`)
    - `x11grab_active`: `bool`, `mcp_connected`: `bool`, `studio_id`: `string | null`
    - `stream_url`: `"/api/studio/stream.mjpg"`, `frame_url`: `"/api/studio/frame"`, `has_live_frame`: `bool`, `frame_seq`: `u64`
    - `detection`: `{ platform, studio_installed, studio_running, studio_display, already_logged_in, place_open, running_studios, xvfb_available, display_ready, studio_exe, mcp_command, guidance }`
- `POST /api/projects/{id}/open-in-studio` (loopback-only; body `{ rebuild?: true, engine?: "auto"|"rojo"|"builtin", display?: ":0"|":99", dry_run?: false }`):
  - Builds the project's `.rbxlx` (or reuses the latest built `.rbxlx` when `rebuild: false` and it exists) and opens it directly in Roblox Studio (Linux Vinegar `flatpak run --filesystem=... --file-forwarding org.vinegarhq.Vinegar @@ <rbxlx> @@`, Windows `RobloxStudioBeta.exe`, macOS `open -a RobloxStudio`, or `OPENRO_OPEN_STUDIO_CMD` test hook) so developers never hit a blind Start Page error.
  - Returns `200 { ok, project_id, build_id, rebuilt, rbxlx_path, rbxlx_rel, launched, dry_run, pid, display, platform, command, message }`.
- `POST /api/studio/embed/attach` (loopback-only; body `{}` or `{ display?, mcp_command? }`):
  - Grabs an **already-running** Roblox Studio without ever spawning a duplicate Studio process (`launch_studio_process = false`).
  - On Linux, detects the running Studio's real X11 `DISPLAY` and resolution and immediately starts the 12 FPS `ffmpeg x11grab` MJPEG stream on `/api/studio/stream.mjpg`.
  - If Studio is already signed in with an open place -> attaches live MCP immediately and returns `200 { ok: true, running: true, connected: true, awaiting_signin: false, phase: "connected", studio_connected: true, ... }`.
  - If Studio is running at the Sign-In / QR screen (no `studio_id` yet) -> keeps the live viewport stream running, starts a background MCP auto-attacher (2s poll, `OPENRO_EMBED_POLL_MS` override), and returns `200 { ok: true, running: true, connected: false, awaiting_signin: true, phase: "awaiting_signin", x11grab_active, stream_url, message, status }`. As soon as the user finishes signing in / opens a place inside the embedded viewport, `GET /api/studio/embed/status` automatically promotes to `phase: "connected"`, `mcp_connected: true`, `already_logged_in: true`.
  - Returns `502` if no running Studio (and no explicit `mcp_command`) is detected.
- `POST /api/studio/embed/launch` (loopback-only; body `{ display?, width?, height?, place_file?, mcp_command?, launch_studio_process? }`):
  - If Studio is already running, **auto-attaches** instead of launching a duplicate Studio process (same live-stream + auto-promote flow as `/attach`).
  - Otherwise spawns Studio (on Linux: `Xvfb` + Flatpak Vinegar using saved login; on Windows: `RobloxStudioBeta.exe`) and starts streaming immediately (`phase: "awaiting_signin"` until signed in, then auto-promotes to `"connected"`).
  - On non-X11 platforms (Windows, macOS, Wayland), once connected with `!x11grab_active`, pushes live MCP `screen_capture` frames into `/api/studio/stream.mjpg` whenever a browser tab is subscribed.
- `POST /api/studio/input/mouse` & `POST /api/studio/input/keyboard` (loopback-only):
  - While `x11grab_active && !mcp_connected` (`phase: "awaiting_signin"`), forwards clicks/moves/wheel and keystrokes/typed text directly to the embedded X11 display (`transport: "x11"`, `studio_mode: "awaiting_signin"`) so users can click Sign In / type credentials or scan QR inside OpenRO without leaving the app. Once MCP attaches, routes through official Studio MCP `user_mouse_input` / `user_keyboard_input`.
- `POST /api/studio/launch-and-continue`: 1-click helper for UI actions needing live Studio;
  returns `{ ok, ready, studio_connected, already_connected? | attached_existing? }` or delegates
  to Embedded Studio launch/attach.
