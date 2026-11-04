# Changelog

All notable changes to Open-Ro Studio are documented here. This project is in early development; version numbers describe source milestones, not a guarantee that every configured package has passed release acceptance.

## [Unreleased]

### Added
- Project acceptance definitions: `.openro/acceptance.json` version 1 supports assertion, state and file checks, screenshots and a repair hint. The repair plan injects only the matching scenario (200-character key); `report_playtest` enforces declared file checks after grading and fails closed on invalid, unreadable or oversized definitions. The final packaged build passed a three-phase gate smoke (missing file -> FAILED and plan open; approved write -> PASSED and plan closed; invalid definition -> FAILED and plan open). A real qwen2.5:3b run wrote `repair-probe.txt` after approval and passed after one manual re-test continuation; a llama3.2:3b run was recorded FAILED for the missing file. Fully autonomous real-model repair remains open.
- Windows 0.1.0 installer acceptance: built from `1d9146e` (122,002,550 bytes, sha256 `3089d983…`), silent per-user install to `%LOCALAPPDATA%\Programs\Open-Ro Studio`, HKCU uninstall entry, Start Menu and desktop shortcuts, installed window launched and responding, silent uninstall removed files/registry/shortcuts and preserved user data. Upgrade and Authenticode signing remain open.
- Local media planning: a hardware profile (RAM, CPU, per-GPU NVIDIA memory, free disk) and a 2026-only catalog of 9 image/audio/voice/video/3D models with release dates, licenses, steps, VRAM/RAM floors and confidence. Recommendations match per-device memory, prefer the newest fitting model, allow explicit CPU-only fallbacks, and report no-fit rather than using a 2025 model.
- ComfyUI bridge: loopback health (`/system_stats`, `/object_info`), two 2026 ACE-Step audio presets, frozen one-time plans, `/prompt` submission, history polling and a `userData/media` sink with traversal/symlink/size guards. Two live 5s CPU generations landed in the production sink: 124,348 bytes (sha256 `9ddc4b63…`) and 65,198 bytes (sha256 `7e85de4a…`).
- Media integration: Workspace Local media panel (status, preset, prompt/lyrics/duration/seed, prepare, generate, result path/bytes/sha), `media_generate` agent tool, and a first-class `media` approval with structured preset/prompt/duration/seed fields. The agent->approval->execution path was live-driven with a deterministic mock provider and wrote a 115,467-byte MP3 (sha256 `8e92c0c6...`) to the production sink; rejecting runs nothing. A FLUX.2 klein 4B image preset, distilled API workflow and 13GB+ CUDA/ROCm gate are mock-tested with a PNG sink; no live image ran on this CPU. No video/3D presets or model downloads.
- Playtest repair loop: the ledger derives the newest unresolved failure and attempts since the last pass; a FAILED report_playtest returns a repair instruction, and the Workspace Open repair card runs that exact instruction in a fresh chat. Writes still need approval. A live one-click run with a deterministic mock provider staged a fix, was approved, recorded PASSED and closed the plan; a real-model repair is not verified.

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
