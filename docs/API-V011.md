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

## 4. Open Cloud

The key is typed only into Settings; it is stored `0600` in `<data_dir>/opencloud.json`, shown only
as a mask (`••••abcd`), never returned, never logged, and scrubbed from any upstream error text.
Base URL `https://apis.roblox.com` (tests override with `OPENRO_OPENCLOUD_BASE`).

| Method + path | Body | Reply |
|---|---|---|
| `GET /api/opencloud` | | `{ has_key, key_masked, creator: { userId?\|groupId? }, base: "https://apis.roblox.com", scopes_needed: [..] }` |
| `POST /api/opencloud/key` | `{ api_key }` or `{ clear: true }` | same as `GET` |
| `POST /api/opencloud/settings` | `{ creator: { userId } \| { groupId } }` | same as `GET` |

Needed key scopes: `asset:read asset:write` (uploads), `universe-places:write` (publish),
`game-pass:write`, `developer-product:write` (+ their `:read`), `universe:write` (settings).
Without a key every call below answers `503 { error: "Add your Open Cloud key in Settings first." }`.

### Assets (`POST https://apis.roblox.com/assets/v1/assets`, then poll `operations/{id}`)

| Method + path | Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/assets` | | `{ files: [{ path, kind, bytes, sha256, authorised, locked: { assetId, state, sha256 }\|null, stale }] }` |
| `POST /api/projects/{id}/assets/authorise` | `{ paths: [..], revoke?: false }` | edits `assets.authorised` in the manifest; `{ authorised: [..] }` |
| `POST /api/projects/{id}/assets/upload` | see below | `{ results: [..], summary }` |
| `GET /api/projects/{id}/assets/lock` | | the `assets.lock.json` content |

Upload selection (exactly the rule: authorised by default, everything only on an explicit flag):

* `{}` or `{ "authorised": true }`: the files matching `assets.authorised`. Empty set = nothing is
  uploaded and `summary.note` says to authorise some.
* `{ "paths": ["art/cards/a.png"] }`: those files only (explicit list).
* `{ "everything": true }`: every uploadable file under the project (separate explicit flag; combining
  it with `paths` is `400`).
* `{ "dry_run": true }` with any of the above: lists what would upload and uploads nothing.
* `{ "force": true }` re-uploads files whose sha256 already has an asset id.

Result item: `{ path, state: "uploaded"|"reused"|"unchanged"|"skipped"|"failed", assetId, assetType, sha256, operation, error? }`.
`kind` by extension: `png/jpg/jpeg/bmp/tga` -> `Image` (falls back to `Decal` once if Roblox rejects `Image`),
`mp3/ogg/wav/flac` -> `Audio`, `fbx/gltf/glb/rbxm/rbxmx` -> `Model`, `mp4/mov` -> `Video`;
a manifest `assets.types` glob (or per-request `types`) can force `Decal` or `Animation` (an Animation
must be a Studio-exported `.rbxm`/`.rbxmx`). Files over 20 MB are `skipped`.
`assets.lock.json`: `{ "version": 1, "assets": { "<relative path>": { "sha256", "assetId", "assetType", "state", "uploadedAt" } } }`
(`state` comes from the operation's moderation result when Roblox returns one).
`rbxassetid://<assetId>` is the usable id. Operation polling backs off 0.5 s -> 4 s, 120 s cap per file
(`OPENRO_OPENCLOUD_POLL_MS` shortens it for tests).

### Place publish

`POST /api/projects/{id}/publish` `{ versionType?: "Published"|"Saved" (default "Saved"), build_id? }`
Publishes the build's `.rbxlx` (the latest build, or `build_id`) via
`POST /universes/v1/{universeId}/places/{placeId}/versions?versionType=`. Needs `universeId` and
`placeId` in the manifest (`400` otherwise). Reply `{ versionNumber, versionType, build_id, bytes }`.
`Published` is a live deploy: callers must be told to ask first. Files over 10 MB are refused with `400`.

### Game passes and developer products

| Method + path | Body | Reply |
|---|---|---|
| `GET /api/projects/{id}/products` | | `{ manifest: [..], remote: { developer_products: [..], game_passes: [..] } }` (remote needs a key) |
| `POST /api/projects/{id}/products/sync` | `{ keys?: [..], dry_run?: false }` | `{ results: [{ key, kind, action: "created"\|"updated"\|"adopted"\|"unchanged", id, error? }] }`, ids written back to the manifest |

Matching: a manifest product with an `id` is updated when name, description, price or sale flag
differ from the remote copy; without an `id`, a remote product with the same name is adopted,
otherwise a new one is created. Endpoints used: `developer-products/v2/universes/{u}/developer-products`
and `game-passes/v1/universes/{u}/game-passes` (multipart `name, description, price, isForSale, imageFile`;
list is paged by `nextPageToken`).

### Universe settings

`GET /api/projects/{id}/universe` returns the Open Cloud universe (`GET /cloud/v2/universes/{u}`).
`POST /api/projects/{id}/universe` `{ settings: { displayName?, description?, visibility?, voiceChatEnabled?,
desktopEnabled?, mobileEnabled?, tabletEnabled?, consoleEnabled?, vrEnabled? } }` patches those fields
(`PATCH /cloud/v2/universes/{u}?updateMask=...`). Other keys are `400`. Not covered by Open Cloud at
this writing (use the Creator Dashboard): game icon and thumbnails, monetisation policy, age questionnaire.
