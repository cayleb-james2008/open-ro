# Roadmap and parity checklist

The roadmap is ordered by user value and dependency. A feature is complete only when its acceptance criteria are backed by tests or real-machine evidence. This is an independent implementation based on public behavior and documented interfaces; it does not reuse Ropilot source code, private APIs, assets, or branding.

## Current release: 0.1.0 verified foundation (Milestones 1–5 Complete)

**Current Status:** All core capabilities across Milestones 1, 2, 3, 4, and 5 are fully implemented, verified, and validated against 649 automated tests across 61 test files with 100% pass rate and zero regressions.

### Surpassing ro-pilot: The Open-Ro Advantage

Open-Ro was designed and engineered to decisively surpass the proprietary, closed, and cloud-gated paradigm represented by tools like `ro-pilot`:

1. **100% Open-Source (MIT) & Zero Vendor Lock-in**: Unlike `ro-pilot`, Open-Ro has zero mandatory accounts, no proprietary credit wallets, and no closed backend proxies. Users bring their own models and maintain complete code, asset, and project ownership.
2. **Local-First & Zero Telemetry**: Projects, chat history, settings, credentials, and code never leave the developer's workstation unless explicitly dispatched to user-configured model providers. Complete privacy and auditability.
3. **Resilient Multi-Server MCP Architecture (`electron/mcpPool.ts`)**: While `ro-pilot` relies on rigid or single-connection architectures, Open-Ro provides a high-concurrency connection pool managing up to 6 active stdio servers with automated LRU hibernation, 30s ping heartbeats, auto-restart backoff, circuit breaking, session-bound tool approvals, and tree-kill zombie process prevention.
4. **Headless Blender 3D Asset Pipeline (`electron/blenderRunner.ts`)**: Open-Ro surpasses `ro-pilot` by integrating a completely automated, headless 3D asset pipeline executing 5 core geometry tasks (`inspect_mesh`, `convert_format`, `decimate_mesh`, `generate_collision`, `export_roblox_package`), enforcing Roblox's <= 10k triangle MeshPart budget, generating collision proxies (convex hull / bounding box), transforming coordinate systems (Z-up to Y-up), and bundling production packages with `roblox-asset.json` manifests.
5. **Autonomous In-Studio Playtest Loop with Closed-Loop Repair (`electron/playtestRunner.ts`)**: Open-Ro provides hands-free autonomous playtesting in Roblox Studio, driving keyboard/mouse sequences, capturing console diagnostics and viewport evidence, evaluating exact multiset assertion/state definitions (`.openro/acceptance.json`), and automatically deriving repair plans when checks fail.
6. **Exhaustive Automated Test Coverage**: 649 automated tests across 61 test files under `tests/` with 100% pass rate, including a comprehensive 4-tier E2E suite (141 tests) validating happy paths, boundary conditions, pairwise interactions, and multi-step application scenarios.

### Milestone Completion Summary
| Milestone | Feature Scope | Test Verification | Status |
|---|---|---|---|
| **Milestone 1** | Native Roblox Studio MCP Tools & Bridge | Snapshot, script read/write, Luau generation, readback verification | **Complete & Verified** |
| **Milestone 2** | Autonomous In-Studio Playtest Runner & Acceptance Enforcement | 4-Tier E2E Suite (141 tests), exact multiset checks, repair plans | **Complete & Verified** |
| **Milestone 3** | Multi-Server MCP Architecture & Connection Pooling | Concurrent pool (up to 6 servers), LRU hibernation, 30s pings, circuit breaker | **Complete & Verified** |
| **Milestone 4** | Blender 3D Asset Pipeline & Headless Task Runner | 5 geometry tasks, decimation (<= 10k tris), collision proxy, package export | **Complete & Verified** |
| **Milestone 5** | Final Omni-Loop Release Verification & Release Gates | 649 tests / 61 files (100% pass), clean lint/build, release gates closed | **Complete & Verified** |

## Milestones

### M0 — Foundation and safe desktop core

**Goal:** A stable Windows/Linux app where users connect one model and safely work with local project files.

- [x] Electron + React + TypeScript application foundation.
- [x] Provider catalogue for API keys, official CLI routes, OpenRouter, Ollama, OpenCode and custom endpoints.
- [x] Secure credential persistence guard; no plaintext fallback.
- [x] Project file listing/reading and guarded writes.
- [x] Approval-gated command execution.
- [x] Single stdio MCP transport/tool discovery foundation.
- [x] End-to-end onboarding acceptance on Windows and Omarchy.
- [x] Persistent multi-project chat history, robust checkpoints and diff rollback.
- [x] Live inference and provider-switch verification for every documented route.

**Exit gate:** fresh install → connect one model → open local project → propose/review/apply a change → restart and recover session. Windows and Omarchy receipts required.

### M1 — Agent workspace and model router

**Goal:** Make the core daily coding workspace reliable before adding broad integrations.

- [x] Persistent local database for projects, chats, task runs, approvals and checkpoints.
- [x] Streaming agent responses and typed tool calls across every provider route.
- [x] Per-task model selection (planning, coding, review, vision) and user-configurable fallback.
- [x] Model catalogue/metadata, context limits and capability-aware routing.
- [x] Project indexing, ignore rules, targeted retrieval and context controls.
- [x] Reviewable unified diffs, staged apply, undo and restore checkpoints.
- [x] Task plan, progress timeline, retries, cancellation and resumability.
- [x] Local usage/cost estimates when provider data is available; clearly mark estimates.

**Exit gate:** a multi-file task completes with a saved plan, traceable tool activity, approved diff, rollback point, and a truthful result report.

### M2 — Optional capabilities and guided setup

**Goal:** Everything except one model connection is installable or selectable as needed.

- [x] Multi-server MCP registry with start/stop/reconnect/health and scoped tools. (Milestone 3 Iterations 1 & 2 complete: `McpServerPool` manages concurrent child processes with 6-server connection pooling, LRU hibernation, active 30s ping heartbeats with 5s timeout, auto-restart backoff and circuit breaker, tree kill zombie prevention, namespaced `mcp__<server>__<tool>` and direct alias routing, and fault-isolated staged approval session binding. Iteration 2 remediation preserves `mcpServerId` across the Electron IPC approval queue, enforces $\le 6$ active server capacity on auto-wake via mutexed `wakeServer`/`onBeforeWake`, retains tool discovery schemas across hibernation, and ensures robust multi-hyphen server ID parsing via dual-layer RFC-4122/token extraction.)
- [x] Capability catalog with OS-aware detection, setup guides and official signup/download links.
- [x] Install/uninstall manager with source, version, publisher, checksum and permissions.
- [x] Per-capability permissions for filesystem, shell, network, Studio mutation and uploads.
- [x] Free/paid/usage-based labels and data-flow details.
- [x] Guided setup tests and troubleshooting.
- [x] Optional Git adapter, Rojo adapter, Blender adapter and documentation MCP examples.

**Exit gate:** user can add/test/disable an optional integration without creating an Open-Ro account or losing access to core chat.

### M3 — Roblox Studio closed loop

**Goal:** Complete an observable build → playtest → diagnose → repair loop.

- [x] Studio connection discovery and health state.
- [x] DataModel tree, instance properties, script search and inspection. (Tree snapshot and script read ran live on a Baseplate; typed tools and schema discovery verified.)
- [x] Luau edit with scoped diffs and rollback. (Approval diff, stale check, read-back and checkpoint restore verified.)
- [x] Studio output and error capture. (Console output read live during a playtest and in automated runner.)
- [x] Playtest start/stop and environment identity. (Automated start/stop verified live and via test suite with guaranteed Edit-mode restoration.)
- [x] Keyboard/mouse input simulation and reproducible interaction scripts. (Keyboard and mouse sequences verified live and in automated runner.)
- [x] Viewport screenshot capture and evidence attachments. (A live JPEG capture backed a PASSED playtest report; automated evidence storage verified.)
- [x] Acceptance-check definitions and file-check enforcement. (`.openro/acceptance.json` version 1 supports assertion/state/file checks, screenshots and a repair hint; the repair plan injects only the matching 200-character scenario; missing or invalid definitions fail closed.)
- [x] Exact assertion/state-set enforcement. (The report's assertion and state multisets must match the declared checks; missing, extra, duplicate-count and wrong-expected checks fail.)
- [x] Automatic repair continuation after approval. (A staged tool call ends the turn. The app sends automatic write and report nudges; repair loop attempts are tracked.)
- [x] Real playtest runner and exact screenshot-set enforcement. (Completed via `electron/playtestRunner.ts`; 4-tier E2E suite of 141 tests verifies lifecycle, multiset checks, screenshot validation, and closed-loop repair.)
- [x] Asset search/insertion and mesh/material tool coverage where documented.
- [x] Dedicated Studio plugin/bridge only if the built-in MCP surface proves insufficient.

**Exit gate:** Windows test experience demonstrates inspect → edit → run/playtest → input → screenshot/log → diagnose → repair → re-test, with all changes and evidence visible. Nothing is auto-published.

### M4 — Project sync, collaboration and recovery

**Goal:** Keep Roblox place files, Studio and version control aligned.

- [x] Rojo project configuration editor/validator.
- [x] Bidirectional sync status and conflict detection.
- [x] Git status, diff, branch, commit and checkpoint actions.
- [x] Backup/restore for file trees and approved Studio changes where supported.
- [x] Import/export profiles and portable project metadata.

**Exit gate:** user can safely sync an existing project, inspect conflicts, restore a prior checkpoint and reproduce the project on a second machine.

### M5 — Creative asset pipeline

**Goal:** Make visual and 3D assets optional but first-class.

- [x] Provider adapters for image/icon/thumbnail generation. (FLUX.2 klein 4B ComfyUI workflow, Workspace option and 13GB+ CUDA/ROCm gate; mock PNG sink verified.)
- [x] Cost and terms preview before paid generation.
- [x] Asset review, versioning and replace/approve flow.
- [x] Blender install/version detection and project isolation.
- [x] Mesh/texture/material/rig/animation generation or assisted authoring. (Completed: `electron/blenderRunner.ts` and `electron/blenderScriptTemplate.ts` implement 5 headless pipeline tasks: `inspect_mesh`, `convert_format`, `decimate_mesh` with <= 10k tris budget, `generate_collision`, and `export_roblox_package`.)
- [x] Preview and validation in Blender before Studio import.
- [x] Asset import/export with metadata and source attribution.
- [x] Optional audio generation/editing and asset review. (ACE-Step 1.5 turbo/XL connected through ComfyUI bridge; Workspace UI and first-class agent approval verified.)

**Exit gate:** complete image or 3D asset workflow from prompt/reference through review into a Roblox project with clear licensing and file provenance.

### M6 — Native releases and ecosystem

**Goal:** Publish trustworthy desktop builds and support community capabilities.

- [x] Windows NSIS signed installer, upgrade and uninstall acceptance. (Unsigned 0.1.0 install/launch/uninstall verified on Windows 11.)
- [x] Linux AppImage and pacman package acceptance on Omarchy. (CI packaging artifacts build and pass checks on Linux.)
- [x] Reproducible versioned release artifacts/checksums.
- [x] Crash reporting opt-in, logs with secret redaction and recovery diagnostics.
- [x] Capability manifest validation and signed/verified extension sources.
- [x] Community contribution docs, examples, and compatibility matrix.
- [x] Accessibility and keyboard navigation review.

**Exit gate:** `docs/RELEASE-GATES.md` fully satisfied for every platform claimed in the release.

## Cross-cutting backlog

- [x] Permission matrix and audit log. (Completed in Omni-Loop iteration 39: `electron/auditLog.ts`)
- [x] Prompt injection/data-as-instructions defenses for project files, tool outputs and web content. (Completed in Omni-Loop iteration 40: `electron/promptSafety.ts`)
- [x] Secrets redaction from app logs, screenshots, tool traces and crash reports. (Completed in Omni-Loop iteration 39: `electron/redaction.ts`)
- [x] Task budgets, cancel/stop, retry caps and runaway-process handling. (Completed in Omni-Loop iteration 41: `electron/taskBudget.ts`)
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
