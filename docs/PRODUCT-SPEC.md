# Product specification

This document translates [`../PROJECT.md`](../PROJECT.md) into user journeys and testable requirements. It is intentionally product-level; implementation details live in `ARCHITECTURE.md` and milestone status lives in `ROADMAP.md`.

## 1. Core onboarding journey

1. Install the desktop application.
2. Start Open-Ro without creating an Open-Ro account.
3. Choose a model connection method: API key, official CLI, aggregator, OpenCode, local runtime, or custom endpoint.
4. Follow the provider-specific instructions and confirm the connection.
5. Optionally choose a workspace folder.
6. Land in the workspace and start a task.

**Acceptance:** an API provider can be entered, validated, saved securely, selected, and used for a basic request. An official CLI route can be detected, launched for its own sign-in, and validated without reading its auth files. User may skip all tool integrations.

## 2. Provider selection and model routing

### Requirements

- Store more than one provider connection and allow switching active provider/model.
- Show provider-specific authentication instructions and official setup links.
- Separate subscription CLI routes from API-key routes.
- Show billing ownership and whether inference is local, vendor API, or aggregator-routed.
- Support model ID selection and provider-specific capability metadata.
- Allow future per-task model routing and fallback policies.
- Never claim a subscription connection is valid by merely finding a binary; run a supported status/check command.

### Acceptance

- Provider secrets are never displayed after saving.
- A failed secure-store check does not fall back to plaintext persistence.
- Unknown models and invalid credentials show actionable errors.
- A local Ollama endpoint can be used without entering a cloud key.
- Subscription CLI processes use official provider flows and do not inherit unrelated API key variables.

## 3. Workspace and project context

### Requirements

- Project list, open/add project, and clear active project.
- Project-root-aware context and ignore rules.
- Conversation history linked to project and provider.
- File tree, read/search, diff review, safe file write, checkpoint, undo/restore.
- User can inspect the exact path and diff before accepting mutations.
- Multi-project context must never leak files from one selected workspace into another.

### Acceptance

- Reopen a conversation and retain its project reference and messages.
- Agent cannot silently write outside the selected workspace.
- User can reject a diff with no filesystem change.
- User can restore the last accepted checkpoint.

## 4. Agent and tools

### Requirements

- Plan a task before significant multi-file actions.
- Show planned steps and tools/capabilities needed.
- Present tool calls, results, errors, duration, and completion state.
- Ask for approval before writes, shell commands, destructive operations, uploads, and publish actions.
- Use typed tool permissions and risk levels; don't expose all tools by default.
- Support cancellation, timeout, retry, and resumability where safe.
- Track evidence separately from model-generated summaries.

### Acceptance

- Denied tool call performs no action.
- Tool timeout is visible and does not show success.
- A task report distinguishes completed, failed, skipped, and unverified operations.
- A command preview shows executable, arguments, working directory, and expected effect.

## 5. Roblox Studio integration

### Intended capabilities

- Discover and connect to Roblox Studio's documented MCP endpoint.
- Read game tree / DataModel, inspect instance properties, and search scripts.
- Read and edit Luau scripts within the selected place/project.
- Run allowed Studio-side operations and inspect output.
- Start/stop playtest and observe client/server state as the interface permits.
- Simulate keyboard/mouse input for reproducible tests.
- Capture viewport screenshots and use them as task evidence.
- Search/insert supported assets and inspect mesh/material operations where exposed.

### Acceptance

- Connection state and active Studio place are visible.
- Read-only inspection is available without granting mutation permissions.
- Script edits produce a reviewable diff and a rollback point.
- Playtest start/stop is confirmed by Studio state, not inferred from a button click.
- Input simulation reports the target window/session and result.
- Screenshots are tied to a test step and timestamp.
- No play/publish/save action occurs without the corresponding permission.

## 6. Test-and-repair loop

- Each task can define acceptance checks (e.g. object exists, script compiles, UI appears, interaction works).
- Tests are executed against the exact current project state.
- Output logs, screenshots, and structured results are captured.
- Failures are mapped to likely source files/scripts with supporting evidence.
- Repairs are proposed as focused diffs and reviewed before application.
- Re-run the failed acceptance checks after the repair.
- Stop after a configured budget or repeated failure and report unresolved issues.

**Acceptance:** report includes test identity, environment, result, evidence references, repair diff IDs, and any skipped/unverified checks.

## 7. Optional creative and project integrations

### Blender

- Detect executable/version and installation path.
- Run a bounded background Python task with explicit approval.
- Review preview/mesh/animation outputs before importing.
- Support export/import between Blender and Studio using documented formats.

### Image, icon, thumbnail, audio, and 3D generation

- Present provider choices as optional adapters.
- Show provider terms and cost before paid generation.
- Prefer the local ComfyUI route for 2026-only models; show release date, license, steps and hardware floor before generation.
- Never substitute a 2025 model when the current hardware does not fit; report the no-fit reason instead.
- Keep prompts, media, and outputs local unless a selected provider receives them.
- Review outputs and allow regenerate/replace/approve.
- Preserve attribution/asset licensing notes.

### Rojo and Git

- Detect and guide installation of CLI/plugin.
- Read Rojo project config and show mapping before sync.
- Make sync direction and overwrite scope explicit.
- Show Git status/diff and create checkpoints/branches when approved.

## 8. Capability catalog

Each capability card must display:

| Field | Required information |
|---|---|
| Purpose | User job it unlocks |
| State | Available, needs install, needs sign-in, experimental, or unavailable |
| Platforms | Windows/Linux and version constraints |
| Dependencies | Exact package/binary, supported version, official download |
| Account | Whether signup or key is required |
| Cost | Free, paid, usage-based, or unknown (must explain) |
| Permissions | Filesystem, process, network, Studio mutation, upload, etc. |
| Data flow | Data sent to local tool or external service |
| Setup | Walkthrough with official source links |
| Verification | Exact connection/test result |
| Removal | Disable/uninstall and credential revocation instructions |

## 9. Accessibility and usability

- Keyboard navigation for core tasks and approval dialogs.
- Visible focus, readable contrast, scalable UI, and clear error text.
- Avoid requiring knowledge of command-line flags for common setup.
- Keep advanced controls available without forcing them into beginner onboarding.
- Every progress state must have a textual equivalent and must not rely on color alone.
