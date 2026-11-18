# OpenRO (Rust + Rune restart)

You describe the game, it builds it — a proactive creative agent in a
plain chat window, full access by default, that can one-shot a game.

Fresh codebase (17 Nov 2026 restart), built apart from the original app.

## What it is

- **Rust binary** serving an embedded web UI (plain HTML/CSS/JS string
  constants, hand-rolled HTTP over `std::net::TcpListener` — no frontend
  framework, no build step). Shell: left sidebar (chat list + New chat +
  the game's name), one scrolling thread, one composer row pinned at the
  bottom, and one status chip — "Studio connected" / "Studio not
  connected" — reflecting the real bridge state.
- **Model route:** OpenCode Go — `POST {base}/chat/completions`, default
  base `https://opencode.ai/zen/go/v1`, default model
  `deepseek-v4.1-flash`, bearer key from `OPENRO_MODEL_API_KEY`. Every
  call carries an `x-opencode-session` header and a browser-like
  User-Agent. With no key, the UI shows "no model key configured" and
  chat refuses honestly.
- **Studio bridge:** an MCP client speaking JSON-RPC 2.0 over stdio to a
  child process named by `OPENRO_MCP_COMMAND` (a real Roblox Studio MCP
  server drops in unchanged). Tools used: `get_place_tree`,
  `write_script`, `delete_script`, `create_instance`, `delete_instance`,
  `playtest`, `get_console` — all seven are required up front; a bridge
  missing one is refused honestly before any ask runs.
- **Rune drives code generation:** five Luau code-generation skills —
  `round_manager`, `leaderboard`, `day_night`, `kill_brick`, `spawn_link`
  — each an embedded Rune script (`src/*.rn`) executed on a Rune VM.
  The registry is the single source for both the system prompt and
  dispatch; Luau comes only from skills, never from hand-written model
  output. Skill arguments are type-, range- and content-validated in
  Rust before a script runs.
- **Agent loop:** the model replies with JSON steps —
  `{"skill": ...}`, `{"tool": ...}`, or `{"done": true, "summary": ...}`.
  Each ask's changes are checkpointed (previous state recorded only
  after a change succeeds) into a per-chat stack of change sets,
  persisted to `changes.json`; `POST /api/revert` pops the most recent
  set and restores it — identity-checked, so a path changed by anyone
  else since the ask is refused, never silently overwritten. A `done`
  is refused while changes are unproven by a playtest. On a failed
  playtest the loop fetches the actual console output, feeds it back
  to the model, and repairs autonomously (bounded attempts, honest
  give-up).

## Test fixture

`src/bin/mock_studio.rs` is a mock Studio MCP server (stdio JSON-RPC,
in-memory place tree), clearly a fixture, not product code. Its playtest
rule: FAILS with `ServerScriptService.RoundManager:4: attempt to index
nil with 'Start'` if any written script contains `workspace.Round.` but
no script creates it (no `Name = "Round"`); otherwise it PASSES.

## Run

```sh
export PATH="$HOME/.local/rust/bin:$PATH"
CARGO_BUILD_JOBS=2 cargo build
OPENRO_MCP_COMMAND=./target/debug/mock_studio \
OPENRO_MODEL_API_KEY=... \
./target/debug/openro-rs        # http://127.0.0.1:8077
```

Env: `OPENRO_MODEL_BASE_URL`, `OPENRO_MODEL_ID`, `OPENRO_MODEL_API_KEY`,
`OPENRO_MCP_COMMAND`, `OPENRO_GAME_NAME`, `OPENRO_PORT` (default 8077).

## Proof

```sh
CARGO_BUILD_JOBS=2 cargo test
```

Integration tests (`tests/integration.rs`) run the full loop against a
stub model server through the real ureq path and the mock Studio spawned
as a real child process: Rune generation → write → failed playtest →
console fed back → Rune regeneration → write → passed playtest → revert
restores the pre-ask tree. Plus shell and no-key honesty tests, over both
the agent API and real HTTP.
