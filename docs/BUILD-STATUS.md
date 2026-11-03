# Build and verification status

**Source version:** `0.1.0`  
**Date:** 2026-11-03  
**State:** early functional foundation. The original bootstrap archive was truncated and is not the source of this tree. Dependency install, typecheck, unit tests, and the production renderer/main build have been run in a Linux workspace. A packaged desktop app and a live Electron window have not.

## Included in this source tree

- Electron 45 + React 19 + TypeScript desktop shell.
- Vite renderer build and Electron main-process TypeScript configuration.
- Provider catalogue with the local Ollama and llama.cpp routes first, then OpenAI/Anthropic API, official Codex/Claude Code/OpenCode CLI handoffs, OpenRouter and custom OpenAI-compatible endpoints.
- Local project file listing/reading and agent tool foundation.
- Approval-gated file writes and command execution.
- Credential storage guarded by Electron `safeStorage`; no plaintext persistence fallback.
- Single active stdio MCP client. It sends newline-delimited JSON-RPC as the MCP stdio transport requires, lists the server's tools, answers server `ping`, and stops the process when the handshake fails. Starting a server and each agent tool call (`mcp__<tool>`) are staged in the approval queue; the exact command or argument JSON runs only after approval. A staged tool call is bound to the server session it was staged on. It is refused after a reconnect, after a crash and restart (including during the new server's handshake), and on a different server; `tests/mcpServerBinding.test.ts` and `tests/mcpCrashRestart.test.ts` cover these cases.
- Playtest evidence gate (`report_playtest`): a result is PASSED only with at least one assertion, one state check, and one PNG screenshot inside the project, and every expected value equals the actual one. A mismatch is FAILED with the failing check; anything else, including more than 50 items in a list or a value over 2,000 characters, is NO_EVIDENCE. There is no playtest runner yet, so the agent supplies the values and the screenshot path, and Open-Ro does not verify them against a run.
- Blender is detected by running `blender --version`. There is no Blender task runner.
- Windows NSIS and Linux AppImage/deb/pacman packaging configuration.
- Basic provider catalogue tests and CI configuration.

## Checks completed on 2026-11-03

Commands run in this workspace after `npm ci` (Node.js 24.22.1):

- `npm run typecheck` passed.
- `npm run lint` passed (rejects type suppressions, skipped tests, and placeholder text).
- `npm test` passed: 73 tests in 14 files under `tests/`.
- The Luau that the Studio tools generate ran in the Luau CLI against a small mock DataModel: snapshot, read, create, update, stale-source refusal, duplicate names, and read-back. A stdio MCP test server that runs `execute_luau` code the same way was driven through the approval queue. Neither is Roblox Studio.
- The MCP client completed the handshake, listed tools, and called `echo` against `@modelcontextprotocol/server-everything` 2.1.0 over stdio, both directly and as a staged call approved through the queue. Before the framing fix the same `initialize` request timed out after 15 s.
- `npm run build` passed. Vite emitted `dist/` and `tsc` emitted `dist-electron/`.
- The renderer was opened in headless Chrome at 1280×800 and 960×700. Chat, Models, Workspace, and Integrations navigation were clicked. The Models screen rendered 9 provider routes, with Ollama and llama.cpp first. No page errors were logged. The favicon request succeeded.

These checks do **not** prove an Electron window launch, a signed installer, live model inference, or a Roblox Studio session.

## Not verified / not produced

- A running Electron application, including the preload bridge, file dialog, and keyring.
- Windows NSIS installer, install/upgrade/uninstall journey, Authenticode signing.
- Linux AppImage, Debian package, or pacman artifact, install/uninstall or keyring behavior on Omarchy.
- Live model inference for any provider.
- Full native CLI sign-in/connection validation on Windows and Linux.
- Live Roblox Studio MCP workflow, playtest/input/screenshot/repair loop.
- Live Blender workflow, asset import/export, Rojo synchronization.
- Image, thumbnail, icon, audio or 3D generation integrations.

CI is present as configuration. This document does not claim a green GitHub Actions run.

## Known limitations

- MCP client supports one active stdio server at a time. It answers server `ping`; other server-to-client requests such as `roots/list` get method-not-found because the client declares no capabilities.
- Chat history is saved locally. The selected project folder is not restored after a restart.
- An approved command's output is shown in the app. It is not sent back to the model unless the user chooses "Send result to the agent".
- There is no Blender task runner, Rojo sync UI, or image/thumbnail/3D adapter.
- Roblox Studio integration needs a live Studio MCP session. Linux Studio through Wine/Vinegar is not claimed.

See [`ROADMAP.md`](ROADMAP.md) for the next milestones and [`RELEASE-GATES.md`](RELEASE-GATES.md) for release requirements.
