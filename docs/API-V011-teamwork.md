# OpenRO v0.11 API: Teamwork Mode (Multi-Agent Board, Scheduler & Verifier Gate)

Contract for `@openro-ui` and `qa-openro`. Served by `openro-rs` on loopback only
(non-loopback `Host` gets `403`, cross-origin `POST` gets `403`). Request bodies are JSON
(`Content-Type: application/json`). Every JSON reply carries `"ok": bool`; failures carry
`"error": "<plain words>"`. Status codes: `400` bad input, `403` forbidden, `404` unknown run or
agent, `409` state or file-claim conflict, `503` SSE client cap reached or model unconfigured.

Teamwork runs persist under `<project_dir>/.openro/teamwork/<run_id>/` (or
`<data_dir>/teamwork-default/.openro/teamwork/<run_id>/` when no project is active) as:
- `run.json` — run metadata, scheduler settings, quota state, roster summary
- `board.jsonl` — append-only JSONL board (`seq` 1, 2, 3, ...)
- `tasks.json` — tasks (`owner`, `status`, `claimed_files`, `verify_report`, `commit_sha`) and active file claims
- `agents/<name>.json` — per-agent chat history and board cursor (`cursor_seq`)
- `traces/<name>.jsonl` — append-only per-agent trace (prompts, model replies, tool calls/results, quota backoffs, state changes)

---

## 1. Endpoints

| Method + path | Body / query | Reply |
|---|---|---|
| `POST /api/teamwork` | `{ goal, project_id?, roles?, max_concurrency?, model_id? }` | `{ run: RunSummary }` (`201`) |
| `GET /api/teamwork` | `?project_id=...` | `{ runs: [RunSummary] }` newest first |
| `GET /api/teamwork/:id` | `?since=<board_seq>` | `{ run: RunDetail, roster: [AgentState], tasks: [Task], claims: [FileClaim], board: [BoardPost] }` |
| `POST /api/teamwork/:id/post` | `{ text, author? }` | `{ message: BoardPost, woken: [name] }` (`201`) |
| `POST /api/teamwork/:id/stop` | `{}` | `{ run: RunSummary }` |
| `POST /api/teamwork/:id/resume` | `{}` | `{ run: RunSummary, woken: [name] }` |
| `POST /api/teamwork/:id/agents/:name/kill` | `{ reason? }` | `{ agent: AgentState }` |
| `POST /api/teamwork/:id/agents/:name/restart` | `{ reset?, brief? }` | `{ agent: AgentState }` |
| `GET /api/teamwork/:id/agents/:name/trace` | `?since=<trace_seq>` | `{ agent: AgentState, trace: [TraceEntry], messages: [ChatMessage] }` |
| `GET /api/teamwork/:id/stream` | `Last-Event-ID` or `?since=<event_seq>` | SSE stream (`text/event-stream`) |

### `POST /api/teamwork`

Starts a multi-agent run for `goal`.

- `goal` (required, non-empty string): plain-English goal for the team.
- `project_id` (optional string): slug from `GET /api/projects`; defaults to the active project.
- `roles` (optional array): defaults to `["researcher", "builder", "art", "verifier"]`.
  Elements may be role strings (`"systems"`) or `{ "name": "systems", "role": "systems", "brief"?: "...", "model"?: "..." }`.
  A `coordinator` agent is always created automatically, and at least one `verifier` agent is always included.
- `max_concurrency` (optional integer, default `2`, clamped `1..=16`): maximum simultaneous model calls across agents.
- `model_id` (optional string): model override for the run; defaults to OpenRO's configured model (e.g. `gemini-4-hydrogen-high` via Antigravity `agy`).

### `GET /api/teamwork/:id`

Returns full state for run `:id`.

- **`run` (`RunDetail`)**:
  - `id`: string (`"team-<ms>-<n>"`)
  - `goal`: string
  - `project_id`: string | null
  - `project_dir`: string
  - `storage_dir`: string
  - `status`: `"running" | "completed" | "stopped" | "failed"`
  - `model_id`: string
  - `provider`: `"antigravity" | "openai"`
  - `max_concurrency`: number
  - `active_model_calls`: number
  - `board_seq`: number (highest `seq` on `board.jsonl`)
  - `studio_lease`: `{ "holder_agent": string, "task_id": string | null, "operation": "sync" | "playtest", "acquired_at_ms": number } | null`
  - `quota`: `{ "active": bool, "reset_in": string | null, "resume_at_ms": number | null, "message": string | null, "retries": number }`
  - `created_at_ms`: number, `updated_at_ms`: number

- **`roster` (`[AgentState]`)**:
  - `name`: string (`"coordinator"`, `"researcher"`, `"systems"`, `"ui"`, `"world"`, `"art"`, `"verifier"`, ...)
  - `role`: string (`"coordinator"`, `"verifier"`, or worker role)
  - `state`: `"working" | "waiting" | "stopped"`
  - `killed`: bool (true when terminated via `/kill`; `/restart` clears it)
  - `chat_id`: string
  - `model_id`: string
  - `current_task_id`: string | null
  - `cursor_seq`: number (highest board `seq` delivered to this agent)
  - `turns`: number
  - `trace_count`: number
  - `last_post_seq`: number | null
  - `last_error`: string | null
  - `updated_at_ms`: number

- **`tasks` (`[Task]`)**:
  - `id`: string (e.g. `"t1"`)
  - `title`: string
  - `description`: string
  - `kind`: `"game_code" | "art" | "mesh" | "docs" | "research" | "other"`
  - `owner`: string (agent name)
  - `verifier`: string (verifier agent name, default `"verifier"`)
  - `status`: `"pending" | "in_progress" | "awaiting_verification" | "changes_requested" | "done" | "failed" | "cancelled"`
  - `claimed_files`: `[string]` (project-relative paths)
  - `depends_on`: `[string]`
  - `result_summary`: string | null
  - `verify_report`: `{ "passed": bool, "verifier": string, "playtest_passed": bool | null, "console_errors": number, "notes": string, "at_ms": number } | null`
  - `commit_sha`: string | null (set on auto-commit after verifier pass)
  - `created_at_ms`: number, `updated_at_ms`: number

- **`claims` (`[FileClaim]`)**:
  - `path`: string (normalized project-relative path)
  - `task_id`: string
  - `owner`: string
  - `claimed_at_ms`: number

- **`board` (`[BoardPost]`)**:
  - `seq`: number (`1, 2, 3, ...`, strictly monotonic)
  - `ts_ms`: number
  - `time`: string (RFC3339 UTC timestamp)
  - `author`: string (`"human"`, `"coordinator"`, `"systems"`, ..., or `"system"`)
  - `kind`: `"goal" | "brief" | "post" | "done" | "verdict" | "claim" | "human" | "quota" | "system"`
  - `text`: string
  - `mentions`: `[string]` (parsed `@name` / `@all` targets)
  - `is_done`: bool (true when post starts with `[done]` after optional `@mentions`)
  - `task_id`: string | null

### `POST /api/teamwork/:id/post`

Appends a human/owner post to the board (`author` defaults to `"human"`, `kind: "human"`). Parses
`@name` and `@all` mentions and wakes only the mentioned agents (if the run is `running` and the
agent is not `killed`).

Request:
```json
{ "text": "@systems use 1000 pity for Mythic", "author": "human" }
```
Reply (`201 Created`):
```json
{ "ok": true, "message": { "seq": 12, "author": "human", "kind": "human", "text": "...", "mentions": ["systems"], "is_done": false, "task_id": null, "ts_ms": 1700000015000, "time": "..." }, "woken": ["systems"] }
```

### `POST /api/teamwork/:id/stop`

Stops the run immediately: wakes and aborts any backoff/queue waits, sets `run.status = "stopped"` and
all agents to `state = "stopped"`, releases any held internal Studio lease, and persists state.

### `POST /api/teamwork/:id/resume`

Resumes a `"stopped"` run (or wakes stalled agents / clears a manual pause), restoring `"running"` and
re-queuing agents with unfinished tasks or unread `@mentions`.

### `POST /api/teamwork/:id/agents/:name/kill` and `/restart`

- `kill`: marks agent `:name` as `killed: true, state: "stopped"`, removes it from the active wake queue, releases any uncommitted claims held solely by its pending work (or flags its active task), and posts a system notice. Returns `404` if `:name` is not on the roster.
- `restart`: clears `killed` and `last_error`, sets `state: "waiting"` (or immediately schedules a turn if it has unread `@mentions`, pending tasks, or an optional `brief` is posted), and if `reset: true` clears its chat context before waking.

### `GET /api/teamwork/:id/agents/:name/trace`

Returns the per-agent trace (`trace`: `[{ seq, ts_ms, turn, kind, summary, data }]`) and its chat
history (`messages`: `[{ role, content }]`). Supports `?since=<trace_seq>`.

---

## 2. Board, Scheduling, Claims, Verifier Gate & Git Auto-Commit Semantics

1. **Mention-only wakeups**: An agent in `waiting` sleeps without consuming model calls until a board
   post names `@<agent>` or `@all` (or the coordinator creates/assigns a task for it). A post
   without `@mentions` is still persisted and read the next time the agent wakes, but never wakes an
   idle agent.
2. **File-ownership claims**: Two active tasks may never own or edit the same file. Conflicting
   `claim_files` calls or writes/edits targeting a file owned by another active task are refused (`409`
   / tool error naming the owning task and agent). Claims are released when the task passes
   verification (`done`) or is `cancelled`.
3. **Internal Studio lease**: Only one agent can run `project_sync` or `project_playtest` at a time.
   Acquiring the lease serializes concurrent callers across the entire sync/playtest operation.
4. **Verifier gate**: Worker agents submit tasks (`awaiting_verification`); only a `verifier` agent
   can pass (`done`) or reject (`changes_requested`) a task via `verify_task`. For `game_code` tasks
   (or files under `src/` or `ui/`), `verify_task(passed=true)` requires a Playtest Runner pass
   with `0` console errors recorded after the task's latest edit. For `tools/**/*.luau` tasks not
   touching `src/` or `ui/`, `verify_task(passed=true)` requires a passing `project_run_lune` check
   (`passed: true`) after the latest edit instead of Studio playtesting.
5. **Auto-commit on task done**: When a task passes verification, OpenRO commits the task's changed
   files through the project git hook (`projects::git_commit`) with message:
   `openro(<role>): <task> [team <run> task <id>]`
   and stores `commit_sha` on the task.
6. **Quota-aware scheduler**: At most `max_concurrency` (default `2`) model calls execute
   concurrently. On an `agy` quota/429 error, the scheduler extracts the reset phrase (e.g.
   `resets in 2h13m`), sets `run.quota`, posts a `quota` event to the board and SSE stream, and
   pauses model calls until the reset window or manual stop/resume.
7. **Restart resume**: On OpenRO startup, `TeamworkManager` scans `<data_dir>` and registered project
   `.openro/teamwork/*` directories, restores `run.json`, `board.jsonl`, `tasks.json`, agent chats,
   and traces, clears any stale Studio lease, and resumes runs that were `"running"` before restart.

---

## 3. SSE Stream (`GET /api/teamwork/:id/stream`)

Streams live events as `text/event-stream` (`Cache-Control: no-cache, no-transform`, keepalive
`: keepalive\n\n` every 15 s). Supports `Last-Event-ID` and `?since=<event_seq>`.

Initial frame:
```
event: status
data: {"run_id":"team-1","status":"running","event_seq":5,"board_seq":4,"roster":[...],"tasks":[...],"quota":{...},"studio_lease":null,"recent":[...]}

```

Incremental event frames:
```
id: 6
event: board
data: {"event_seq":6,"ts_ms":1700000010000,"run_id":"team-1","message":{"seq":5,"author":"systems","kind":"done","text":"@verifier [done] Net.luau ready","mentions":["verifier"],"is_done":true,"task_id":"t1"}}

```

Event types: `status` (initial snapshot), `board` (new board post), `task` (task status/claim/verify/commit update), `roster` (agent `working`/`waiting`/`stopped`/`killed` change), `trace` (new per-agent trace step), `quota` (quota backoff active/cleared), `run` (run status `running`/`completed`/`stopped`/`failed`).

---

## 4. Agent Step Loop & Tools

Each Teamwork agent runs a pure-text JSON step loop on OpenRO's active model (`gemini-4-hydrogen-high` via `agy` or configured HTTP endpoint) inside an isolated `agy-sandbox` directory. One turn returns one of:

- Single tool call: `{"tool": "<name>", "args": {...}}`
- Batched tool calls (aborts on first error): `{"steps": [{"tool": "<name>", "args": {...}}, ...]}`
- Park / finish wake turn: `{"done": true, "summary": "...", "post": "..."?}`

Available tools:
- Board & tasks: `board_read`, `board_post`, `create_task` (strips `openro.project.json` with a note instead of failing), `claim_files`, `release_files`, `submit_task`, `verify_task`, `finish_run`
- Project & verification: `project_list`, `project_files` (`args.dir` or `args.path` alias), `project_read_file`, `project_grep`, `project_write_file`, `project_edit_file` (`args.edits` or flat `old_string`/`new_string`), `project_delete_file` (same-owner writes auto-route across that agent's active tasks), `project_update_manifest` (`products[].id` aliases to `key`, appends `openro.project.json` to `task.written_files` for auto-commit, enforces `priceApproved: false` for agents), `luau_check`, `project_check`, `project_run_lune`, `project_build`, `project_sync` (leased), `project_playtest` (leased; required only when `src/` or `ui/` files are touched)
- Research & web: `research_roblox_game`, `web_search`, `web_fetch` (SSRF-blocked, `max_chars` default `6000` / max `10000`)
- Assets & media: `reference_search`, `reference_select`, `art_generate` (ChatGPT default, Higgsfield refused), `art_approve`, `image_process`, `art_pack_card`, `mesh_generate` / `blender_generate` (project-local + `manifest.json` + `AssetIds.luau`), `audio_generate` (project-local + `AssetIds.luau`), `assets_codegen`, `assets_upload`

Context & history hygiene: live `Tasks:` status is injected transiently per model turn (never accumulated in persisted `agents/<name>.json`), and `tool_result` entries plus large `project_write_file` / step payloads older than 2 turns are compacted to 1-line summaries. Polling `board_read` when the board `head` has not changed auto-parks the agent `waiting` until a new message arrives.
