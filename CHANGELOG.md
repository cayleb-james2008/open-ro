# Changelog

All notable changes to Open-Ro Studio are documented here. This project is in early development; version numbers describe source milestones, not a guarantee that every configured package has passed release acceptance.

## [Unreleased]

### Added
- Windows 0.1.0 installer acceptance: built from `1d9146e` (122,002,550 bytes, sha256 `3089d983…`), silent per-user install to `%LOCALAPPDATA%\Programs\Open-Ro Studio`, HKCU uninstall entry, Start Menu and desktop shortcuts, installed window launched and responding, silent uninstall removed files/registry/shortcuts and preserved user data. Upgrade and Authenticode signing remain open.
- Local media planning: a hardware profile (RAM, CPU, per-GPU NVIDIA memory, free disk) and a 2026-only catalog of 9 image/audio/voice/video/3D models with release dates, licenses, steps, VRAM/RAM floors and confidence. Recommendations match per-device memory, prefer the newest fitting model, allow explicit CPU-only fallbacks, and report no-fit rather than using a 2025 model.
- ComfyUI bridge: loopback health (`/system_stats`, `/object_info`), two 2026 ACE-Step audio presets, frozen one-time plans, `/prompt` submission, history polling and a `userData/media` sink with traversal/symlink/size guards. A live 5s CPU generation produced a 124,348-byte MP3 (sha256 `9ddc4b63…`). No media UI, agent tool or production-sink exercise yet.
- Playtest repair loop state: the ledger now derives the newest unresolved failure and counts attempts since the last pass; a FAILED `report_playtest` result returns a "Repair loop: attempt N" instruction to the agent, and the plan is exposed to the renderer. It is instructive only and never auto-runs a repair.

### Fixed
- Project tools stay inside the folder selected in the main process, including symlink escapes, and they refuse likely secret files.
- Agent writes and shell commands remain staged until the user approves that exact action. Rejecting one does not change the project.
- Saved providers keep their model IDs. Local Ollama stays visible without an API key. Deleting the active provider clears it.
- Chat, Models, Workspace, and Integrations are all reachable. Git detection is listed. Studio MCP points at Roblox's Studio MCP documentation.
- Chat history is stored in the app's user-data directory. The renderer no longer navigates itself away when opening the repository link.

### Corrected
- Documentation no longer says a Blender task runner or an OS approval dialog exists. Blender detection is `blender --version` only.

## [0.1.0] — 2026-11-02

### Added
- Electron + React + TypeScript desktop foundation for Windows and Linux.
- Provider catalogue for API keys, official provider CLI routes, OpenRouter, Ollama, OpenCode and custom OpenAI-compatible endpoints.
- Local project tools and approval-gated file/command operations.
- Secure credential-storage guard with no plaintext fallback.
- Single-server MCP stdio client foundation and capability detection.
- Blender Python task foundation.
- Windows NSIS, Linux AppImage, Debian and pacman packaging configuration.
- Product charter, product specification, roadmap, architecture, platform support and release gates.

### Known limitations
- Native binaries and real-machine install journeys are not yet verified.
- Full Roblox Studio playtest/repair loop, Rojo sync UI, complete Blender asset pipeline, image/thumbnail generation and multi-server MCP registry remain incomplete.
