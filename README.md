# OpenRO (Rust + Rune)

[![latest release](https://img.shields.io/github/v/release/cayleb-james2008/open-ro)](https://github.com/cayleb-james2008/open-ro/releases)


You describe the game, it builds it. OpenRO is a proactive creative agent
in a plain chat window. It builds a Roblox game in Roblox Studio through a
Studio MCP bridge, and it can one-shot a complete game.

## What it is

- **Rust binary** serving an embedded web UI (plain HTML/CSS/JS string
  constants, hand-rolled HTTP over `std::net::TcpListener`, no frontend
  framework, no build step). Left sidebar: chat list, New chat, the game's
  name. One scrolling thread, an ideas bar with one-tap builds, one composer
  pinned at the bottom, and one status chip, "Studio connected" or "Studio
  not connected", reflecting the real bridge state. Light and dark themes
  (toggle in Settings), dark by default.
- **Model route:** OpenCode Go. `POST {base}/chat/completions`, default base
  `https://opencode.ai/zen/go/v1`, default model `deepseek-v4.1-flash`,
  bearer key from `OPENRO_MODEL_API_KEY`. Every call carries an
  `x-opencode-session` header and a User-Agent. With no key, the UI shows
  "no model key configured" and chat refuses.
- **Studio bridge:** an MCP client speaking JSON-RPC 2.0 over stdio to the
  child process named by `OPENRO_MCP_COMMAND`: a Roblox Studio MCP server, or
  the mock fixture below. The tools used are `get_place_tree`,
  `write_script`, `delete_script`, `create_instance`, `delete_instance`,
  `playtest` and `get_console`. All seven are required up front; a bridge
  missing one is refused before any ask runs. Each request times out after
  `OPENRO_BRIDGE_TIMEOUT_SECS` (default set in `src/mcp.rs`).
- **Rune drives code generation:** ten Luau skills, each an embedded Rune
  script (`src/*.rn`): `round_manager`, `leaderboard`, `day_night`,
  `kill_brick`, `spawn_link`, `currency` (leaderstats and DataStore save and
  load with a shutdown save), `shop` (server-side purchases over a
  RemoteEvent), `game_gui` (title, live balance, shop button), `checkpoints`
  (stage pads and respawn) and `soundtrack`. The registry feeds both the
  system prompt and dispatch. Luau comes only from skills. Skill arguments
  are validated in Rust before a script runs.
- **It pitches, unprompted:** on an empty chat it offers complete games (a
  lava obby, a coin tycoon, a round arena); after a build it offers the next
  fitting feature. Each idea has a built-in plan that runs through the same
  checkpoint, playtest and repair machinery with no model call. A build whose
  prerequisite is missing is refused in plain words, naming the prerequisite
  by its display name, e.g. `A shop for the money needs "Money that saves"
  first. Build "Money that saves" first, then ask again.` The chat path and
  the idea button give the same sentence.
- **Agent loop:** the model replies with one JSON step per turn:
  `{"skill": ...}`, `{"tool": ...}`, or `{"done": true, "summary": ...}`.
  - Each ask's changes are checkpointed into a per-chat stack of change sets,
    saved to `changes.json`. `POST /api/revert` (the UI's "Undo last build")
    pops the most recent set and restores it. The restore is identity-checked:
    a path changed by anyone else since the ask is refused, not overwritten.
  - A `done` is refused while changes are unproven by a playtest, or while
    the chat's last playtest failed. Verdicts are kept per chat in
    `proof.json`, so they survive a restart. A corrupt or missing
    `proof.json` leaves every chat that has change sets unproven until a
    playtest passes.
  - An ask that changed nothing because a step failed is refused in plain
    words (`ok: false`). A playtest in an ask that changed nothing is
    reported as not run. An ask that changed nothing and had no failed step
    keeps its `ok` and its text, with a note that nothing changed.
  - A failed playtest feeds the console output back to the model and the
    loop repairs (at most 3 attempts). After that it gives up honestly and
    names the script left in the game.
  - An idea step that creates an instance already in place with the same
    class is skipped, not failed.

## Data

Set `OPENRO_DATA_DIR` (default `openro-data`). The app keeps:

- `chats.json`: chats and their lines (active chat).
- `changes.json`: change-set stacks for Undo.
- `proof.json`: per-chat playtest verdicts and skills that Undo removed.
- `ideas.json`: per-chat built skills and the last pitch.

A corrupt file is backed up beside itself as `<name>.json.bak` (then `.bak2`,
and so on) and the app starts with empty state for that file. The backup
name is listed in `store_recovered` on `GET /api/status`, and the page shows
a dismissible banner naming it.

## HTTP API

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/` | the page |
| GET | `/api/status` | `studio_connected`, `model_configured`, `game_name`, `store_recovered` |
| GET | `/api/chats` | `chats`, `active`, and `can_undo` (the active chat has a change set to undo) |
| POST | `/api/chats` | new chat (201) |
| POST | `/api/chats/select` | `{"id": ...}` |
| GET | `/api/thread` | the active chat's lines |
| POST | `/api/chat` | `{"message": ...}`, runs an ask |
| GET | `/api/place-tree` | scripts and instances in the Studio place |
| POST | `/api/ideas` | the pitch for the active chat |
| POST | `/api/ideas/build` | `{"id": ...}`, builds one idea |
| POST | `/api/revert` | undoes the most recent change set of the active chat |

## Same-site guard and limits

- Only loopback Host names are answered: `localhost`, `127.0.0.1`, `[::1]`.
  Other Host names are refused with 403 JSON unless listed in
  `OPENRO_ALLOWED_HOSTS` (comma-separated; a port in an entry is ignored).
- A state-changing request (POST, PUT, PATCH, DELETE) is refused with 403
  JSON when its `Origin` authority differs from its `Host`, or when
  `Sec-Fetch-Site` is `cross-site`. A request with a body that is not
  `application/json` is refused with 415 JSON.
- A request body over 1 MiB gets 413 JSON.
- Each connection's reads and writes time out after 10 seconds.
- At most 64 connections are handled at once; beyond that the server answers
  503 JSON. Asks can run for minutes; the limit applies to the connection,
  not the ask.

## Run

```sh
export PATH="$HOME/.local/rust/bin:$PATH"
CARGO_BUILD_JOBS=2 cargo build
OPENRO_MCP_COMMAND=./target/debug/mock_studio \
OPENRO_MODEL_API_KEY=... \
./target/debug/openro-rs        # http://127.0.0.1:8077
```

Environment:

- `OPENRO_MODEL_BASE_URL`, `OPENRO_MODEL_ID`, `OPENRO_MODEL_API_KEY`
- `OPENRO_MCP_COMMAND`: the Studio MCP server command (unset = not connected)
- `OPENRO_BRIDGE_TIMEOUT_SECS`: per-request bridge timeout
- `OPENRO_GAME_NAME`, `OPENRO_PORT` (default 8077), `OPENRO_DATA_DIR`
- `OPENRO_ALLOWED_HOSTS`: extra Host names to answer (see above)
- `OPENRO_UPDATE_MODE`, `OPENRO_UPDATE_REPO`, `OPENRO_UPDATE_API`,
  `OPENRO_UPDATE_DEV` (see "Versions and updates")

## Versions and updates

**Releasing:** bump `version` in `Cargo.toml`, add a `## X.Y.Z - date` entry to `CHANGELOG.md`, and merge to `main`. `.github/workflows/release.yml` sees the new version, builds every platform and publishes the GitHub Release `vX.Y.Z` with the binaries and `SHA256SUMS`; a push that does not change the version publishes nothing, and a new version without a changelog entry fails the workflow. Pushing a `vX.Y.Z` tag by hand does the same.

The version is printed by `openro-rs --version` (or `-V`), reported by
`GET /api/version` and `GET /api/status`, and every release is tagged
`vX.Y.Z` with a changelog entry in `CHANGELOG.md`. Merging a new version to `main` (or pushing a tag) runs
`.github/workflows/release.yml`: it builds `openro-rs` for each platform (the
mock Studio binaries are test fixtures and are not released), checks that the
tag matches `Cargo.toml`, writes a `SHA256SUMS` file, and publishes the GitHub
Release.

The updater (`src/update.rs`) asks the newest GitHub release of
`cayleb-james2008/open-ro` whether a newer version exists. The first check is
30 seconds after start, then every 6 hours; only the `openro-rs` binary does
this, and tests never call out.

- `GET /api/update` shows the state without contacting GitHub. Its fields are
  `name`, `current`, `mode`, `repo`, `platform`, `checked_at`, `available`,
  `latest`, `notes`, `release_url`, `error`, `staged`, `can_apply` and
  `dev_build`.
- `POST /api/update/check` asks GitHub now.
- `POST /api/update/apply` downloads the release binary for this platform,
  checks its SHA-256 against `SHA256SUMS`, runs it with `--version` to confirm
  it reports the expected version, and only then replaces the running file.
  The new build runs after a restart. Any failed step leaves the installed
  program untouched.
- `POST /api/update/restart` starts the staged build. It is refused with 409
  when nothing is staged and while an ask, idea build or revert is running.

Apply and restart are refused unless the request's Host is loopback
(`localhost`, `127.0.0.1`, `[::1]`); `OPENRO_ALLOWED_HOSTS` does not permit
them. A cargo-built binary (under `target/`) is never replaced unless
`OPENRO_UPDATE_DEV=1`.

| Variable | Default |
| --- | --- |
| `OPENRO_UPDATE_MODE` | `notify` (check and tell); `auto` also downloads and installs (a restart is still yours to start); `off` never contacts GitHub |
| `OPENRO_UPDATE_REPO` | `cayleb-james2008/open-ro` |
| `OPENRO_UPDATE_API` | `https://api.github.com` (plain http only for a loopback address) |
| `OPENRO_UPDATE_DEV` | unset; `1` allows replacing a cargo-built binary (tests) |

## Test fixture

`src/bin/mock_studio.rs` is a mock Studio MCP server (stdio JSON-RPC, an
in-memory place tree). It is a test fixture, not product code, and its place
resets when it restarts. Its playtest applies fixed rules for the failure
shapes a build can produce: a Round object referenced but not created, a
missing spawn pad, a checkpoints folder referenced but not created, a
DataStore used without a shutdown save, a game screen waiting on a shop
event nobody created, and a shop with no currency behind it. It passes only
when none of these fire.

## Tests

```sh
CARGO_BUILD_JOBS=2 cargo test   # 98 tests, all passing: 10 in src/update.rs, 88 in tests/integration.rs
```

The integration tests run the full loop against a stub model server (through
the real HTTP client path) and the mock Studio spawned as a child process over
stdio. They cover generation, a failed playtest, console feedback, repair,
revert, persistence across restarts, the done-gate, the same-site guard and
the request limits, both over the agent API and over real HTTP.

Two results were reported at the 17 November restart and were not re-run for
this README: an independent sealed acceptance exam (14 black-box checks)
passed 14/14, and a front-end lint reported 0 failures. Neither is in this
repository.
