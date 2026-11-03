# Build and verification status

**Source version:** `0.1.0`  
**Date:** 2026-11-02  
**State:** early functional foundation; source archive integrity verified; native app build and runtime not verified.

## Included in this source tree

- Electron 45 + React 19 + TypeScript desktop shell.
- Vite renderer build and Electron main-process TypeScript configuration.
- Provider catalogue for OpenAI/Anthropic API, official Codex/Claude Code/OpenCode CLI handoffs, OpenRouter, Ollama and custom OpenAI-compatible endpoints.
- Local project file listing/reading and agent tool foundation.
- Approval-gated file writes and command execution.
- Credential storage guarded by Electron `safeStorage`; no plaintext persistence fallback.
- Single active stdio MCP client with server tool discovery and calls.
- Blender Python task runner foundation.
- Windows NSIS and Linux AppImage/deb/pacman packaging configuration.
- Basic provider catalogue tests and CI configuration.

## Checks actually completed for this source drop

- Source archive created and ZIP integrity checked.
- Archive listing inspected: 37 original source files before repository documentation refresh.
- Twelve TypeScript/TSX files passed syntax transpilation during initial assembly.
- Eight provider entries and packaging metadata passed structural checks during initial assembly.
- Project docs refreshed on 2026-11-02; documentation changes have not been through app runtime tests.

These checks do **not** prove dependency-resolved type checking, unit tests, or a working packaged desktop application.

## Not verified / not produced

- npm dependency installation and lockfile reproducibility.
- `npm run typecheck`, `npm test`, or full Vite/Electron production build against installed dependencies.
- A running Electron application.
- Windows NSIS installer, install/upgrade/uninstall journey, Authenticode signing.
- Linux AppImage, Debian package, or pacman artifact, install/uninstall or keyring behavior on Omarchy.
- Live model inference for any provider.
- Full native CLI sign-in/connection validation on Windows and Linux.
- Live Roblox Studio MCP workflow, playtest/input/screenshot/repair loop.
- Live Blender workflow, asset import/export, Rojo synchronization.
- Image, thumbnail, icon, audio or 3D generation integrations.

The initial build environment could not access the npm registry, so dependencies were not installed and packages were not built. CI is present as configuration; no green CI result is asserted here.

## Known limitations

- MCP client currently supports one active stdio server at a time.
- Full multi-provider model routing, per-task policies and durable task history are incomplete.
- Diff/rollback/checkpoint experience requires further implementation.
- Blender task runner is not a complete model/animation/asset pipeline.
- Rojo UI and synchronization are not implemented.
- Image/thumbnail/3D-generation adapters are not implemented.
- Roblox Studio integration needs live Windows validation; Linux Studio through Wine/Vinegar is experimental.

See [`ROADMAP.md`](ROADMAP.md) for the next milestones and [`RELEASE-GATES.md`](RELEASE-GATES.md) for release requirements.
