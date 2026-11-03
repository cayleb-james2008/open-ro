# Open-Ro project proof log

This file is a factual project-change log for release notes, project reviews, and future showcase material. Add an entry only when the change exists in source or is supported by a test/build receipt. Keep measured results separate from goals and estimates.

## 2026-11-03 — safety gates, approval tray, MCP and Studio tools, and a checked build

- **Project boundary:** file and command tools use the main-process project root. `realpath` containment, ignored-folder denial, and secret-name denial are covered by `tests/safety.test.ts`.
- **Approvals:** writes and shell commands are queued in the main process. The renderer approves an id and cannot replace the staged body or command.
- **Providers:** every saved route keeps its model id. Ollama does not require a cloud key. Custom OpenAI-compatible endpoints may omit a key and must include a base URL.
- **MCP:** starting a stdio server and every agent call to its tools need approval. A staged call runs only on the server session it was staged on, including after a crash and restart (`tests/mcpServerBinding.test.ts`, `tests/mcpCrashRestart.test.ts`). Outgoing U+2028 and U+2029 are escaped for line-reading servers (`tests/mcpLineSeparators.test.ts`). The client was run against `@modelcontextprotocol/server-everything` 2.1.0.
- **Playtest reports:** marked passed only when the agent reports assertions, state checks and a PNG path, and every expected value equals the actual one. Those values are agent-reported, not checked against a run.
- **Studio tools:** place snapshot, script read and script write through the Studio MCP `execute_luau` tool, each behind approval. The generated Luau was run in the Luau CLI against a mock DataModel, not in Studio.
- **Verification run:** `npm ci`, `npm run typecheck`, `npm run lint`, `npm test` (106 passed in 22 files), `npm run build` and `npm run verify:ui` completed in this Linux workspace on Node.js 24.22.1. Electron itself was not launched, and no installer was built.
- **Not claimed:** live inference, a live Roblox Studio session, packaging, or a desktop keyring.

## 2026-11-02 — project direction and repository foundation

- **Mission clarified:** local-first, open-source Roblox development studio with one required model connection and optional installable capabilities.
- **Platform target recorded:** native Windows and Linux/Omarchy desktop app; Roblox Studio on Linux is separately experimental because Roblox does not officially support it.
- **Standing goals documented:** provider freedom, optional capability routing, Roblox Studio closed loop, creative asset workflow, portability, safety, and evidence-backed releases.
- **Source state:** Electron/React/TypeScript 0.1.0 foundation. Current app capabilities and known gaps are documented in `docs/BUILD-STATUS.md`.
- **Verification boundary:** documentation and repo structure updates do not imply that dependencies installed, desktop binaries built, or live Studio/Blender workflows passed.
