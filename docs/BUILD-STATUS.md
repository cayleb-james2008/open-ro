# Build and verification status

**Source version:** `0.1.0`  
**Date:** 2026-11-03  
**State:** early functional foundation. The original bootstrap archive was truncated and is not the source of this tree. Dependency install, typecheck, unit tests, and the production renderer/main build have been run in a Linux workspace. A packaged desktop app and a live Electron window have not.

## Included in this source tree

- Electron 45 + React 19 + TypeScript desktop shell.
- Vite renderer build and Electron main-process TypeScript configuration.
- Provider catalogue for OpenAI/Anthropic API, official Codex/Claude Code/OpenCode CLI handoffs, OpenRouter, Ollama and custom OpenAI-compatible endpoints.
- Local project file listing/reading and agent tool foundation.
- Approval-gated file writes and command execution.
- Credential storage guarded by Electron `safeStorage`; no plaintext persistence fallback.
- Single active stdio MCP client with server tool discovery and calls.
- Blender is detected by running `blender --version`. There is no Blender task runner.
- Windows NSIS and Linux AppImage/deb/pacman packaging configuration.
- Basic provider catalogue tests and CI configuration.

## Checks completed on 2026-11-03

Commands run in this workspace after `npm ci` (Node.js 22.23.4):

- `npm run typecheck` passed.
- `npm run lint` passed (rejects type suppressions, skipped tests, and placeholder text).
- `npm test` passed: 13 tests in `tests/providerCatalog.test.ts` and `tests/safety.test.ts`.
- `npm run build` passed. Vite emitted `dist/` and `tsc` emitted `dist-electron/`.
- The renderer was opened in headless Chrome at 1280×800 and 960×700. Chat, Models, Workspace, and Integrations navigation were clicked. The Models screen rendered 8 provider routes. No page errors were logged. The favicon request succeeded.

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

- MCP client supports one active stdio server at a time. Connecting it runs the command the user typed; it is not the same gate as file and shell approvals.
- Chat history is saved locally. The selected project folder is not restored after a restart.
- An approved command's output is shown in the app. It is not sent back to the model unless the user chooses "Send result to the agent".
- There is no Blender task runner, Rojo sync UI, or image/thumbnail/3D adapter.
- Roblox Studio integration needs a live Studio MCP session. Linux Studio through Wine/Vinegar is not claimed.

See [`ROADMAP.md`](ROADMAP.md) for the next milestones and [`RELEASE-GATES.md`](RELEASE-GATES.md) for release requirements.
