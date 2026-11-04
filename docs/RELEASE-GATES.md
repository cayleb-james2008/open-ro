# Release gates

A package build is not a release. A release is the combination of exact source identity, successful automated checks, installation tests, and primary user journeys on the platforms claimed.

## Gate A — source and dependencies

- [ ] Clean checkout on the exact release commit.
- [ ] Lockfile committed and dependency install succeeds from a clean cache.
- [ ] No secrets, local paths, temporary artifacts, or credentials in the diff.
- [ ] Third-party licenses/notices included and reviewed.

## Gate B — automated checks

- [ ] `npm run typecheck` passes.
- [ ] `npm test` passes.
- [ ] Renderer build passes.
- [ ] Electron main/preload build passes.
- [ ] Security checks cover path traversal, renderer isolation, command approval and secure credential handling.
- [ ] CI has green exact-commit checks for Windows and Linux.

## Gate C — Windows acceptance

- [x] NSIS installer installs for current user. (0.1.0 from `1d9146e`, per-user, silent exit 0.)
- [x] App launches from Start Menu and desktop shortcut. (Start Menu shortcut launch verified; both shortcuts created and removed by uninstall.)
- [ ] API key can be saved securely and used; key never appears in logs/UI after save.
- [ ] Official CLI provider setup/check works.
- [ ] Project browse/read/write approval works and rejected writes do not mutate files.
- [ ] Shell approval shows exact executable/arguments/cwd and cancellation blocks execution.
- [ ] Roblox Studio MCP can connect, inspect, edit, playtest, capture evidence and stop cleanly.
- [ ] Upgrade, uninstall, app data retention/removal and clean reinstall are tested.

## Gate D — Omarchy / Arch acceptance

- [ ] pacman artifact installs and uninstalls cleanly.
- [ ] AppImage starts on a supported desktop session.
- [ ] Keyring/Secret Service is detected; insecure storage mode refuses credential persistence.
- [ ] Project path permissions, file dialogs, child processes and shell approval work under a normal user.
- [ ] Blender/Git/Rojo capability detection works when tools are present/absent.
- [ ] Native desktop update path is tested where supported.
- [ ] Wine/Vinegar Studio connection is separately marked experimental until its full Studio test journey passes.

## Gate E — core user journeys

- [ ] Fresh install → connect one model → send first prompt.
- [ ] Open project → inspect files → propose diff → approve/reject → rollback.
- [ ] Add optional capability → complete guided setup → test connection → disable/remove.
- [ ] Studio task → inspect project → code edit → playtest → screenshot/log evidence → repair → re-test.
- [ ] Creative task → create/import asset → review in project.
- [ ] New user can skip every optional integration without being blocked from core chat.

## Gate F — release integrity

- [ ] Build artifacts map to the exact release commit.
- [ ] Windows installer signing and Linux artifact checksums are verified where configured.
- [ ] Release notes list implemented, changed, experimental, and planned capabilities.
- [ ] Documentation lists tested platform and dependency versions.
- [ ] Rollback/recovery and known limitations are published.

A box may be checked only with a saved CI run, test result, or reproducible machine receipt. A code path, mock, README claim, or model statement is not evidence by itself.
