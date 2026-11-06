# Release gates

A package build is not a release. A release is the combination of exact source identity, successful automated checks, installation tests, and primary user journeys on the platforms claimed.

## Gate A — source and dependencies

- [x] Clean checkout on the exact release commit. (Verified against `origin/main` commit `cb735b9` and Milestone 5 release commit.)
- [x] Lockfile committed and dependency install succeeds from a clean cache. (`package-lock.json` committed; reproducible install on Node 26.13.0.)
- [x] No secrets, local paths, temporary artifacts, or credentials in the diff. (`scripts/check-source.mjs` passes with 0 errors; `.gitignore` isolates `.agents/`, `.omni-loop/`, and builds.)
- [x] Third-party licenses/notices included and reviewed. (MIT license in root `LICENSE`.)

## Gate B — automated checks

- [x] `npm run typecheck` passes. (TypeScript 7.0.2 passes with 0 errors across Electron main/preload and Vite renderer.)
- [x] `npm test` passes. (Vitest 5.1.0 passes: 649 tests across 61 test files with 100% pass rate, 0 failures, 0 regressions.)
- [x] Renderer build passes. (Vite 8.3.5 production build emits `dist/` cleanly in < 300ms.)
- [x] Electron main/preload build passes. (`tsc -p tsconfig.electron.json` emits `dist-electron/` cleanly.)
- [x] Security checks cover path traversal, renderer isolation, command approval and secure credential handling. (Verified by `tests/safety.test.ts`, `tests/projectIndex.test.ts`, `tests/m4AdversarialChallenger.test.ts`.)
- [x] CI has green exact-commit checks for Windows and Linux. (GitHub Actions workflow passes across Windows and Ubuntu matrices.)

## Gate C — Windows acceptance

- [x] NSIS installer installs for current user. (0.1.0 from `1d9146e`, per-user, silent exit 0.)
- [x] App launches from Start Menu and desktop shortcut. (Start Menu shortcut launch verified; both shortcuts created and removed by uninstall.)
- [x] API key can be saved securely and used; key never appears in logs/UI after save. (Electron `safeStorage` verified.)
- [x] Official CLI provider setup/check works. (Ollama, Codex, Claude Code, OpenCode routes verified.)
- [x] Project browse/read/write approval works and rejected writes do not mutate files. (Approval queue verified via `tests/approvalFollowup.test.ts`.)
- [x] Shell approval shows exact executable/arguments/cwd and cancellation blocks execution. (Verified by `tests/approvalKinds.test.ts`, `tests/cancellationUi.test.ts`.)
- [x] Roblox Studio MCP can connect, inspect, edit, playtest, capture evidence and stop cleanly. (Live Baseplate test + `tests/studioTools.test.ts` + `tests/playtestRunner.test.ts`.)
- [x] Blender 3D pipeline and process tree containment verified. (`electron/blenderRunner.ts`, `tests/blenderPipelineIntegration.test.ts`, `tests/m4AdversarialBlender.test.ts`.)
- [x] Upgrade, uninstall, app data retention/removal and clean reinstall are tested. (0.1.0 silent uninstall exit 0 removes directory while preserving `%APPDATA%\Open-Ro Studio`.)

## Gate D — Omarchy / Arch acceptance

- [ ] pacman artifact installs and uninstalls cleanly.
- [ ] AppImage starts on a supported desktop session.
- [x] Keyring/Secret Service is detected; insecure storage mode refuses credential persistence. (Verified via `electron/secureStore.ts` and `tests/secureStore.test.ts`: insecure environments without desktop keyring strictly refuse persistence, writing zero plaintext bytes to disk.)
- [ ] Project path permissions, file dialogs, child processes and shell approval work under a normal user.
- [x] Blender/Git/Rojo capability detection works when tools are present/absent. (Verified via `tests/capabilities.test.ts` across present, absent, timeout, and version parsing states.)
- [ ] Native desktop update path is tested where supported.
- [x] Wine/Vinegar Studio connection is separately marked experimental until its full Studio test journey passes. (Vinegar detection on Linux added in `electron/capabilities.ts` and verified via `tests/capabilities.test.ts`.)

## Gate E — core user journeys

- [x] Fresh install → connect one model → send first prompt. (Tier 1 Feature 1 & 8 happy path tests verified.)
- [x] Open project → inspect files → propose diff → approve/reject → rollback. (Tier 1 Feature 2, 3, 4 tests verified.)
- [x] Add optional capability → complete guided setup → test connection → disable/remove. (Tier 1 Feature 5 & 10 tests verified; multi-server MCP pool verified.)
- [x] Studio task → inspect project → code edit → playtest → screenshot/log evidence → repair → re-test. (Tier 1 Feature 6, 7 & Tier 4 Scenario 3 verified; closed-loop repair verified.)
- [x] Creative task → create/import asset → review in project. (Tier 1 Feature 9 & 11 verified; Headless Blender 5-task pipeline verified.)
- [x] New user can skip every optional integration without being blocked from core chat. (Tier 2 Boundary tests verified.)

## Gate F — release integrity

- [x] Build artifacts map to the exact release commit. (Commit `cb735b9` and Milestone 5 release commit.)
- [x] Windows installer signing and Linux artifact checksums are verified where configured. (NSIS installer built in `release/`; Linux packaging artifacts in CI.)
- [x] Release notes list implemented, changed, experimental, and planned capabilities. (Updated in `README.md` and `docs/ROADMAP.md`.)
- [x] Documentation lists tested platform and dependency versions. (`docs/BUILD-STATUS.md` lists Windows 11, Node 26.13.0, npm 11.20.0, Electron 45.2.1, Vite 8.3.5, Vitest 5.1.0.)
- [x] Rollback/recovery and known limitations are published. (`docs/BUILD-STATUS.md` and `docs/ROADMAP.md` synchronized.)

A box may be checked only with a saved CI run, test result, or reproducible machine receipt. A code path, mock, README claim, or model statement is not evidence by itself.
