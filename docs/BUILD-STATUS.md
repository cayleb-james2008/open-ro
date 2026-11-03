# Build and verification status

**Source version:** `0.1.0`  
**Date:** 2026-11-03  
**State:** early functional foundation. Dependency install, typecheck, lint, unit tests, and the production build run on Windows 11 (Node 26.13.0) and in GitHub Actions on `ubuntu-latest` and `windows-latest`. A Windows NSIS installer was built locally (`release/Open-Ro Studio Setup 0.1.0.exe`); it has not been installed or launched, and no Electron window has been opened in this workspace.

## Included in this source tree

- Electron 45 + React 19 + TypeScript desktop shell.
- Vite renderer build and Electron main-process TypeScript configuration.
- Provider catalogue with the local Ollama and llama.cpp routes first, then OpenAI/Anthropic API, official Codex/Claude Code/OpenCode CLI handoffs, OpenRouter and custom OpenAI-compatible endpoints.
- Local project file listing/reading and agent tool foundation.
- Chat history and provider state persist under `userData` with atomic writes (temp file, fsync, rename), so a crash cannot truncate them; the last selected project folder is restored on startup.
- Approval-gated file writes and command execution. File checkpoint restores are staged as ordinary write approvals (diff review plus a fresh checkpoint of the overwritten state), so a restore can itself be restored. Creation checkpoints — where Open-Ro created the file — refuse to restore instead of deleting silently.
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
- `npm test` passed: 126 tests in 25 files under `tests/`.
- `npm run build` passed. Vite emitted `dist/` and `tsc` emitted `dist-electron/`.
- `npm run package:win` passed with electron-builder 26.18.0: produced `release/Open-Ro Studio Setup 0.1.0.exe` (NSIS, one-click disabled, unsigned).
- `npm run package:linux` was not run in this workspace; it requires the Linux packaging toolchain and runs in CI.
- GitHub Actions (`CI and desktop packages`): the `checks` matrix passes on `ubuntu-latest` and `windows-latest`. Packaging is green for commit `509a080` (run 24): the `windows` job built and uploaded `open-ro-windows-x64` (116.3 MB) and the `linux` job built and uploaded `open-ro-linux-x64` (305.7 MB: AppImage, deb, pacman).

Earlier verification preserved from the prior workspace run:

- With `OPENRO_LUAU_BIN` set to a Luau CLI, `npm test` also runs `tests/luau` (7 more tests): the write and read Luau the Studio tools generate runs against the mock DataModel in `tests/fixtures/studio-datamodel.luau`. It covers update, exact read-back of CRLF and NUL sources, stale-source refusal (including a CRLF/LF difference), a script that appeared after the review, duplicate names, a removed script, a class change, and whether the undo step was recorded.
- The renderer was driven in headless Chrome with a stubbed IPC bridge: the agent staged two actions, one was approved and one rejected, and the renderer sent one follow-up turn with both results. The Electron main process was not running.
- The MCP client completed the handshake, listed tools, and called `echo` against `@modelcontextprotocol/server-everything` 2.1.0 over stdio, both directly and as a staged call approved through the queue.

These checks do **not** prove an Electron window launch, an installer install/uninstall journey, live model inference, or a Roblox Studio session.

## Not verified / not produced

- A running Electron application, including the preload bridge, file dialog, and keyring.
- Windows installer install/upgrade/uninstall journey and Authenticode signing. The unsigned NSIS artifact builds; the journey is not recorded.
- Linux AppImage, Debian package, or pacman install/uninstall or keyring behavior on Omarchy. CI packaging is configured to build the artifacts; they are not installed.
- Live model inference for any provider.
- Full native CLI sign-in/connection validation on Windows and Linux.
- Live Roblox Studio MCP workflow, playtest/input/screenshot/repair loop.
- Live Blender workflow, asset import/export, Rojo synchronization.
- Image, thumbnail, icon, audio or 3D generation integrations.

CI runs the checks matrix on both supported operating systems and builds the packaging artifacts (green run 24 for commit `509a080`). A green run proves the build and unit checks for that commit; it does not prove install behavior or a live app session.

## Known limitations

- MCP client supports one active stdio server at a time. It answers server `ping`; other server-to-client requests such as `roots/list` get method-not-found because the client declares no capabilities.
- Chat history is saved locally in `userData/sessions.json` with atomic writes, and the last selected project folder is restored on startup. History is still global rather than scoped per project, and retention is capped at 40 sessions / 100 messages per session.
- File checkpoint restores are reviewable. Creation checkpoints (a file Open-Ro created) cannot be restored from the UI yet; the app refuses rather than deleting without review.
- The automatic follow-up waits until every approval from the agent turn is decided, and until the chat that staged them is open. Results of approvals you start yourself, such as connecting an MCP server, are not sent to the agent. A result card shows only in the chat it belongs to.
- Tool output in the follow-up is sent inside a closed data block with a random per-message nonce, after the instructions. This labels it as data for the model; it does not guarantee that a model ignores instructions written in it.
- There is no Blender task runner, Rojo sync UI, or image/thumbnail/3D adapter.
- Roblox Studio integration needs a live Studio MCP session. Linux Studio through Wine/Vinegar is not claimed.

See [`ROADMAP.md`](ROADMAP.md) for the next milestones and [`RELEASE-GATES.md`](RELEASE-GATES.md) for release requirements.
