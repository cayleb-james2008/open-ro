# Feature inventory

Status values follow OMNI-LOOP v4: source code alone is never `VERIFIED`.

| ID | Title | Acceptance | Status | Files and tests |
|---|---|---|---|---|
| FEAT-001 | Choose one model route | A saved provider keeps its model id and can be selected without showing the secret | PARTIAL | `electron/providerState.ts`, `tests/safety.test.ts`. Keyring and live HTTP were not run. |
| FEAT-002 | Local project folder | Tools cannot leave the selected root, including through a symlink | PARTIAL | `electron/pathSafety.ts`, `tests/safety.test.ts`. The folder is not restored after restart. |
| FEAT-003 | Approve a write or command | Rejecting changes nothing. Approving uses the stored body, not a renderer substitute | PARTIAL | `electron/approvals.ts`, `electron/main.ts`, `tests/safety.test.ts`. The queue was not driven through Electron IPC. |
| FEAT-004 | Chat history | Sessions round-trip through the sanitizer and drop unexpected fields | PARTIAL | `electron/sessions.ts`, `tests/safety.test.ts`. Disk IO runs only inside Electron. |
| FEAT-005 | Integrations catalog | Studio, Git, Rojo, Blender, and provider CLIs are listed. Studio is not auto-started | PARTIAL | `electron/capabilities.ts`. Detection was not executed in this session. |
| FEAT-006 | MCP stdio client | Frames reassemble and the child environment is scrubbed | PARTIAL | `electron/mcpFraming.ts`, `tests/safety.test.ts`. No live Studio server was connected. |
| FEAT-007 | Desktop shell | Four screens render, and 1440, 768, and 375 have no horizontal overflow | PARTIAL | `src/App.tsx`, `scripts/verify-ui-craft.mjs`. Electron window was not opened. |
| FEAT-008 | Packaging | Windows NSIS and Linux AppImage, deb, and pacman are configured | MISSING | `package.json`, `.github/workflows/ci.yml`. `package:win` and `package:linux` were not run. |
| FEAT-009 | Roblox Studio loop | Inspect, edit, playtest, and repair through Studio MCP | MISSING | Docs only. No Studio session. |
| FEAT-010 | Credential storage | Refuses to persist a key when safeStorage is unavailable | UNKNOWN | `electron/secureStore.ts`. No keyring was present in this workspace. |
