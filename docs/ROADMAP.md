# Roadmap and parity checklist

The roadmap is ordered by user value and dependency. A feature is complete only when its acceptance criteria are backed by tests or real-machine evidence. This is an independent implementation based on public behavior and documented interfaces; it does not reuse Ropilot source code, private APIs, assets, or branding.

## Current release: 0.1.0 foundation

Implemented in source: Electron/React desktop shell, provider selection foundation, API and official CLI routes, local project tools, approval-gated shell/file operations, secure credential-storage guard, single-server MCP client with approval-gated server start and agent tool calls, a playtest evidence gate (no runner yet), Blender detection via `blender --version` (no task runner), and Windows/Linux packaging configuration.

Not established: a dependency-resolved build, native installers, live Studio integration, complete Blender asset transfer, image/thumbnail generation, or complete parity. See [`BUILD-STATUS.md`](BUILD-STATUS.md).

## Milestones

### M0 — Foundation and safe desktop core

**Goal:** A stable Windows/Linux app where users connect one model and safely work with local project files.

- [x] Electron + React + TypeScript application foundation.
- [x] Provider catalogue for API keys, official CLI routes, OpenRouter, Ollama, OpenCode and custom endpoints.
- [x] Secure credential persistence guard; no plaintext fallback.
- [x] Project file listing/reading and guarded writes.
- [x] Approval-gated command execution.
- [x] Single stdio MCP transport/tool discovery foundation.
- [ ] End-to-end onboarding acceptance on Windows and Omarchy.
- [ ] Persistent multi-project chat history, robust checkpoints and diff rollback.
- [ ] Live inference and provider-switch verification for every documented route.

**Exit gate:** fresh install → connect one model → open local project → propose/review/apply a change → restart and recover session. Windows and Omarchy receipts required.

### M1 — Agent workspace and model router

**Goal:** Make the core daily coding workspace reliable before adding broad integrations.

- [ ] Persistent local database for projects, chats, task runs, approvals and checkpoints.
- [ ] Streaming agent responses and typed tool calls across every provider route.
- [ ] Per-task model selection (planning, coding, review, vision) and user-configurable fallback.
- [ ] Model catalogue/metadata, context limits and capability-aware routing.
- [ ] Project indexing, ignore rules, targeted retrieval and context controls.
- [ ] Reviewable unified diffs, staged apply, undo and restore checkpoints.
- [ ] Task plan, progress timeline, retries, cancellation and resumability.
- [ ] Local usage/cost estimates when provider data is available; clearly mark estimates.

**Exit gate:** a multi-file task completes with a saved plan, traceable tool activity, approved diff, rollback point, and a truthful result report.

### M2 — Optional capabilities and guided setup

**Goal:** Everything except one model connection is installable or selectable as needed.

- [x] Multi-server MCP registry with start/stop/reconnect/health and scoped tools. (Milestone 3 Iterations 1 & 2 complete: `McpServerPool` manages concurrent child processes with 6-server connection pooling, LRU hibernation, active 30s ping heartbeats with 5s timeout, auto-restart backoff and circuit breaker, tree kill zombie prevention, namespaced `mcp__<server>__<tool>` and direct alias routing, and fault-isolated staged approval session binding. Iteration 2 remediation preserves `mcpServerId` across the Electron IPC approval queue, enforces $\le 6$ active server capacity on auto-wake via mutexed `wakeServer`/`onBeforeWake`, retains tool discovery schemas across hibernation, and ensures robust multi-hyphen server ID parsing via dual-layer RFC-4122/token extraction.)
- [ ] Capability catalog with OS-aware detection, setup guides and official signup/download links.
- [ ] Install/uninstall manager with source, version, publisher, checksum and permissions.
- [ ] Per-capability permissions for filesystem, shell, network, Studio mutation and uploads.
- [ ] Free/paid/usage-based labels and data-flow details.
- [ ] Guided setup tests and troubleshooting.
- [ ] Optional Git adapter, Rojo adapter, Blender adapter and documentation MCP examples.

**Exit gate:** user can add/test/disable an optional integration without creating an Open-Ro account or losing access to core chat.

### M3 — Roblox Studio closed loop

**Goal:** Complete an observable build → playtest → diagnose → repair loop.

- [ ] Studio connection discovery and health state.
- [ ] DataModel tree, instance properties, script search and inspection. (Tree snapshot and script read ran live on a Baseplate; properties/search not yet exposed.)
- [ ] Luau edit with scoped diffs and rollback. (Approval diff, stale check, read-back and checkpoint restore exist; a create write with exact read-back ran live, Studio restore not yet exercised.)
- [x] Studio output and error capture. (Console output read live during a playtest.)
- [ ] Playtest start/stop and environment identity. (Start/stop verified live; environment identity pending.)
- [x] Keyboard/mouse input simulation and reproducible interaction scripts. (Keyboard and mouse sequences reported Success live.)
- [x] Viewport screenshot capture and evidence attachments. (A live JPEG capture backed a PASSED playtest report.)
- [x] Acceptance-check definitions and file-check enforcement. (`.openro/acceptance.json` version 1 supports assertion/state/file checks, screenshots and a repair hint; the repair plan injects only the matching 200-character scenario; missing or invalid definitions fail closed. The final packaged build passed a three-phase gate smoke; a real qwen2.5:3b run staged the fix, the human only approved the write, and the automatic re-test produced PASSED in about 197s.)
- [x] Exact assertion/state-set enforcement. (The report's assertion and state multisets must match the declared checks; missing, extra, duplicate-count and wrong-expected checks fail. A final-build smoke rejected an extra assertion and accepted the exact set. Whitespace is trimmed for comparison; screenshots are not yet an exact declared set.)
- [x] Automatic repair continuation after approval. (A staged tool call ends the turn. The app sends one automatic write nudge if the first turn stages nothing, and one automatic report nudge if the follow-up skips `report_playtest` and stages no new approval. Messages are marked `[automatic repair step]` and bound to the staging chat.)
- [ ] Real playtest runner and exact screenshot-set enforcement. (The gate enforces the declared assertion/state multiset; whitespace is trimmed for comparison, screenshots are not yet an exact declared set, and the 29-byte smoke PNG is a fixture rather than a real capture. Writes still require human approval.)
- [ ] Asset search/insertion and mesh/material tool coverage where documented.
- [ ] Dedicated Studio plugin/bridge only if the built-in MCP surface proves insufficient.

**Exit gate:** Windows test experience demonstrates inspect → edit → run/playtest → input → screenshot/log → diagnose → repair → re-test, with all changes and evidence visible. Nothing is auto-published.

### M4 — Project sync, collaboration and recovery

**Goal:** Keep Roblox place files, Studio and version control aligned.

- [ ] Rojo project configuration editor/validator.
- [ ] Bidirectional sync status and conflict detection.
- [ ] Git status, diff, branch, commit and checkpoint actions.
- [ ] Backup/restore for file trees and approved Studio changes where supported.
- [ ] Import/export profiles and portable project metadata.

**Exit gate:** user can safely sync an existing project, inspect conflicts, restore a prior checkpoint and reproduce the project on a second machine.

### M5 — Creative asset pipeline

**Goal:** Make visual and 3D assets optional but first-class.

- [ ] Provider adapters for image/icon/thumbnail generation. (FLUX.2 klein 4B has a distilled ComfyUI workflow, Workspace option and 13GB+ CUDA/ROCm gate; no live image has run on this CPU. Video/3D and icon/thumbnail adapters pending.)
- [ ] Cost and terms preview before paid generation.
- [ ] Asset review, versioning and replace/approve flow.
- [ ] Blender install/version detection and project isolation.
- [ ] Mesh/texture/material/rig/animation generation or assisted authoring.
- [ ] Preview and validation in Blender before Studio import.
- [ ] Asset import/export with metadata and source attribution.
- [ ] Optional audio generation/editing and asset review. (ACE-Step 1.5 turbo/XL are connected through the ComfyUI bridge; Workspace UI and first-class agent approval exist, with live production-sink clips including an agent->approval run. Audio editing/review and image/video/3D remain unverified.)

**Exit gate:** complete image or 3D asset workflow from prompt/reference through review into a Roblox project with clear licensing and file provenance.

### M6 — Native releases and ecosystem

**Goal:** Publish trustworthy desktop builds and support community capabilities.

- [ ] Windows NSIS signed installer, upgrade and uninstall acceptance. (Unsigned 0.1.0 install/launch/uninstall is recorded; signing and upgrade pending.)
- [ ] Linux AppImage and pacman package acceptance on Omarchy.
- [ ] Reproducible versioned release artifacts/checksums.
- [ ] Crash reporting opt-in, logs with secret redaction and recovery diagnostics.
- [ ] Capability manifest validation and signed/verified extension sources.
- [ ] Community contribution docs, examples, and compatibility matrix.
- [ ] Accessibility and keyboard navigation review.

**Exit gate:** `docs/RELEASE-GATES.md` fully satisfied for every platform claimed in the release.

## Cross-cutting backlog

- [ ] Permission matrix and audit log.
- [ ] Prompt injection/data-as-instructions defenses for project files, tool outputs and web content.
- [ ] Secrets redaction from app logs, screenshots, tool traces and crash reports.
- [ ] Task budgets, cancel/stop, retry caps and runaway-process handling.
- [ ] Project context isolation and no writes outside user-selected scope.
- [ ] Keyboard-first workflows and accessible contrast/focus.
- [ ] User-facing cost disclosure and external data-flow disclosure.
- [ ] Documentation and integration version drift checks.

## Status vocabulary

- **Implemented:** source code exists and the named automated checks pass.
- **Partially implemented:** some code exists, but one or more user acceptance journeys are incomplete.
- **Experimental:** works only in selected environments or requires manual configuration; compatibility is not guaranteed.
- **Planned:** not implemented yet.
- **Verified:** a reproducible test or hardware receipt exists for the exact source and platform.

Do not promote a feature to Verified based on code inspection alone.
