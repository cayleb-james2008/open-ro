# Build and verification status

**Source version:** `0.1.0`  
**Date:** 2026-11-03  
**State:** early functional foundation. Dependency install, typecheck, lint, unit tests, and the production build run on Windows 11 (Node 26.13.0) and in GitHub Actions on `ubuntu-latest` and `windows-latest`. The packaged app has been launched from `release/win-unpacked` on Windows 11: the window rendered with the preload bridge and loaded provider state over IPC. A live Roblox Studio MCP round trip succeeded on a Baseplate (`get_studio_state` + `execute_luau`). The NSIS installer has not been installed, and the file dialog and OS keyring have not been exercised.

## Included in this source tree

- Electron 45 + React 19 + TypeScript desktop shell.
- Vite renderer build and Electron main-process TypeScript configuration.
- Provider catalogue with the local Ollama and llama.cpp routes first, then OpenAI/Anthropic API, official Codex/Claude Code/OpenCode CLI handoffs, OpenRouter and custom OpenAI-compatible endpoints.
- Local project file listing/reading and agent tool foundation.
- Chat history is scoped to the project that created it (chats without a project stay visible in every project) and, together with provider state, persists under `userData` using atomic writes (temp file, fsync, rename); the last selected project folder is restored on startup.
- Agent turns use the chat's project: the renderer sends the chat's root, the main process validates it is an existing folder and switches the active root when it differs (clearing pending approvals and studio reads), and a project switch moves the active chat to a visible one instead of leaving a hidden chat open.
- Agent turns can be stopped from the chat composer: the Stop control aborts the in-flight HTTP request or the CLI process tree, and the cancelled turn is recorded in the chat.
- Provider requests retry transient failures up to three attempts: HTTP 429/5xx and network errors back off (250 ms, 500 ms, capped at 4 s) and honor Retry-After; aborts win, and other 4xx responses fail immediately.
- OpenAI-compatible provider turns stream text over SSE when a delta listener is attached: deltas are emitted in order, tool-call arguments split across chunks are assembled by index, and a non-SSE response falls back to one delta. The main process forwards `agent:delta` {sessionId, text} and preload exposes `onAgentDelta`. The chat UI still waits for the final result (incremental rendering is the next slice); Anthropic and CLI routes remain non-streaming.
- A deterministic project index (metadata only) enumerates the active project: ignored segments and secret files are skipped, symlinks/junctions are not followed, paths are sorted posix-relative with byte sizes, and caps (2,000 files / 4 MB / depth 12) set a truncated flag. Each HTTP agent turn receives a compact map from this index (max 150 entries / 6,000 characters) inside the system prompt, explicitly labeled untrusted data whose file names are never instructions; an empty index injects nothing.
- The Workspace screen shows the indexed project map: total file count, a truncation notice when caps applied, and the first 40 entries with byte sizes; clicking an entry opens the file.
- Approval-gated file writes and command execution. File checkpoint restores are staged as ordinary write approvals (diff review plus a fresh checkpoint of the overwritten state), so a restore can itself be restored. Creation checkpoints — where Open-Ro created the file — restore through a reviewable delete approval that checkpoints the file's current contents before removal.
- Credential storage guarded by Electron `safeStorage`; no plaintext persistence fallback.
- Single active stdio MCP client. It sends newline-delimited JSON-RPC as the MCP stdio transport requires, lists the server's tools, answers server `ping`, and stops the process when the handshake fails. Starting a server and each agent tool call (`mcp__<tool>`) are staged in the approval queue; the exact command or argument JSON runs only after approval. A staged tool call is bound to the server session it was staged on. It is refused after a reconnect, after a crash and restart (including during the new server's handshake), and on a different server; `tests/mcpServerBinding.test.ts` and `tests/mcpCrashRestart.test.ts` cover these cases.
- Playtest evidence gate (`report_playtest`): a result is PASSED only with at least one assertion, one state check, and one PNG screenshot inside the project, and every expected value equals the actual one. A mismatch is FAILED with the failing check; anything else, including more than 50 items in a list or a value over 2,000 characters, is NO_EVIDENCE. There is no playtest runner yet, so the agent supplies the values and the screenshot path, and Open-Ro does not verify them against a run.
- Blender is detected by running `blender --version`. There is no Blender task runner.
- Windows NSIS and Linux AppImage/deb/pacman packaging configuration, now under the electron-builder `build` key with the author email the deb/pacman targets require and `--publish never` in every packaging script.
- Cross-platform deterministic checkouts: `.gitattributes` pins text files to LF.
- Basic provider catalogue tests and CI configuration.

## Checks completed on 2026-11-03

Commands run in this Windows 11 workspace:

- `npm run typecheck` passed.
- `npm run lint` passed (rejects type suppressions, skipped tests, and placeholder text).
- `npm test` passed: 187 tests in 30 files under `tests/`.
- `npm run build` passed. Vite emitted `dist/` and `tsc` emitted `dist-electron/`.
- `npm run package:win` passed with electron-builder 26.18.0: produced `release/Open-Ro Studio Setup 0.1.0.exe` (NSIS, one-click disabled, unsigned).
- `npm run package:linux` was not run in this workspace; it requires the Linux packaging toolchain and runs in CI.
- GitHub Actions (`CI and desktop packages`): the `checks` matrix passes on `ubuntu-latest` and `windows-latest`. Packaging is green for commit `509a080` (run 24) and commit `02f2860` (run 27): the `windows` job built and uploaded `open-ro-windows-x64` (116.3 MB) and the `linux` job built and uploaded `open-ro-linux-x64` (305.7 MB: AppImage, deb, pacman).
- The inference adapters were exercised over real HTTP against a local mock provider (`tests/inferenceRoutes.test.ts`): OpenAI-compatible connection check (request path, Bearer header, model, response parsing) plus a 401 rejection, Anthropic connection check (`GET /v1/models` with key and version headers), a full agent turn with a tool call and tool-result round trip, and a refused-connection failure path. All servers bind ephemeral loopback ports and close after each test.
- Real local inference was verified end to end through the app's exported adapters on this machine: Ollama 0.40.14 serving `qwen2.5:0.5b`, `testConnection` returned "Model responded: open-ro connection check", and `runAgentTurn` returned "I am Open-Ro, a development agent inside a local Roblox workspace." No cloud key or account was used; the in-app provider form was not exercised.
- Live streaming was verified through the same adapter against Ollama: 19 text deltas arrived before completion (first at ~4.8 s, total ~5.6 s) and their concatenation equals the final assistant text.

Earlier verification preserved from the prior workspace run:

- With `OPENRO_LUAU_BIN` set to a Luau CLI, `npm test` also runs `tests/luau` (7 more tests): the write and read Luau the Studio tools generate runs against the mock DataModel in `tests/fixtures/studio-datamodel.luau`. It covers update, exact read-back of CRLF and NUL sources, stale-source refusal (including a CRLF/LF difference), a script that appeared after the review, duplicate names, a removed script, a class change, and whether the undo step was recorded.
- The renderer was driven in headless Chrome with a stubbed IPC bridge: the agent staged two actions, one was approved and one rejected, and the renderer sent one follow-up turn with both results. The Electron main process was not running.
- Open-Ro's own Studio pipeline ran live against a Baseplate in this session: the app's MCP client connected to the official bridge (33 tools), a staged-and-approved `studio_snapshot` read 5 instances (Workspace camera/baseplate/terrain, StarterPlayer scripts), a `studio_read_script` of a missing path reported it absent, a staged-and-approved `studio_write_script` created `ServerScriptService.OpenRoLiveProbe` and Studio read back the same path, class and source, and a re-read returned the exact source. Studio did not record an undo step for the MCP write; the script remains in the unsaved test place.
- The MCP client completed the handshake, listed tools, and called `echo` against `@modelcontextprotocol/server-everything` 2.1.0 over stdio, both directly and as a staged call approved through the queue.

These checks do **not** prove an installer install/uninstall journey, cloud-provider inference or CLI sign-in, or a full Roblox Studio playtest/input/screenshot/repair workflow.

## Not verified / not produced

- A running Electron application window: the packaged app from `release/win-unpacked` launched on Windows 11 at 1240×816 with the preload bridge active (the renderer showed loaded provider/connection state, not the missing-bridge notice). The file dialog, OS keyring, and installer install/upgrade/uninstall journey remain unverified.
- Windows installer install/upgrade/uninstall journey and Authenticode signing. The unsigned NSIS artifact builds; the journey is not recorded.
- Linux AppImage, Debian package, or pacman install/uninstall or keyring behavior on Omarchy. CI packaging builds the artifacts; they are not installed.
- Live inference against cloud providers and official CLI sign-ins; no cloud keys or accounts were used in this workspace. The local Ollama route is verified above.
- Full native CLI sign-in/connection validation on Windows and Linux.
- Live Roblox Studio workflows beyond single-instance reads and writes: no playtest, input simulation, viewport capture, or repair loop has run, and the open Baseplate now contains an unsaved test script. The app's approved Studio write flow is verified on that Baseplate.
- Live Blender workflow, asset import/export, Rojo synchronization.
- Image, thumbnail, icon, audio or 3D generation integrations.

CI runs the checks matrix on both supported operating systems and builds the packaging artifacts (green run 24 for commit `509a080`). A green run proves the build and unit checks for that commit; it does not prove install behavior or a live app session.

## Known limitations

- MCP client supports one active stdio server at a time. It answers server `ping`; other server-to-client requests such as `roots/list` get method-not-found because the client declares no capabilities.
- Chat history is saved locally in `userData/sessions.json` with atomic writes, the last selected project folder is restored on startup, and the Recent list is scoped to the open project (project-less and legacy chats stay visible). Retention is capped at 40 sessions / 100 messages per session.
- File checkpoint restores are reviewable, including creation checkpoints: restoring one stages a delete approval that checkpoints the file's current contents before removal. Delete approvals render the removed lines and use the same expand-before-approve gate as writes.
- The automatic follow-up waits until every approval from the agent turn is decided, and until the chat that staged them is open. Results of approvals you start yourself, such as connecting an MCP server, are not sent to the agent. A result card shows only in the chat it belongs to.
- Tool output in the follow-up is sent inside a closed data block with a random per-message nonce, after the instructions. This labels it as data for the model; it does not guarantee that a model ignores instructions written in it.
- There is no Blender task runner, Rojo sync UI, or image/thumbnail/3D adapter.
- Roblox Studio integration needs a live Studio MCP session. Linux Studio through Wine/Vinegar is not claimed.

See [`ROADMAP.md`](ROADMAP.md) for the next milestones and [`RELEASE-GATES.md`](RELEASE-GATES.md) for release requirements.
