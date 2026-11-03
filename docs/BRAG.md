# Open-Ro project proof log

This file is a factual project-change log for release notes, project reviews, and future showcase material. Add an entry only when the change exists in source or is supported by a test/build receipt. Keep measured results separate from goals and estimates.

## 2026-11-03 — safety gates, approval tray, and a checked build

- **Project boundary:** file and command tools use the main-process project root. `realpath` containment, ignored-folder denial, and secret-name denial are covered by `tests/safety.test.ts`.
- **Approvals:** writes and shell commands are queued in the main process. The renderer approves an id and cannot replace the staged body or command.
- **Providers:** every saved route keeps its model id. Ollama does not require a cloud key. Custom OpenAI-compatible endpoints may omit a key and must include a base URL.
- **Verification run:** `npm ci`, `npm run typecheck`, `npm run lint`, `npm test` (13 passed), and `npm run build` completed in this Linux workspace on Node.js 22.23.4. Headless Chrome clicked the four primary screens at 1280×800. Electron itself was not launched, and no installer was built.
- **Not claimed:** live inference, Roblox Studio, packaging, or a desktop keyring.

## 2026-11-02 — project direction and repository foundation

- **Mission clarified:** local-first, open-source Roblox development studio with one required model connection and optional installable capabilities.
- **Platform target recorded:** native Windows and Linux/Omarchy desktop app; Roblox Studio on Linux is separately experimental because Roblox does not officially support it.
- **Standing goals documented:** provider freedom, optional capability routing, Roblox Studio closed loop, creative asset workflow, portability, safety, and evidence-backed releases.
- **Source state:** Electron/React/TypeScript 0.1.0 foundation. Current app capabilities and known gaps are documented in `docs/BUILD-STATUS.md`.
- **Verification boundary:** documentation and repo structure updates do not imply that dependencies installed, desktop binaries built, or live Studio/Blender workflows passed.
