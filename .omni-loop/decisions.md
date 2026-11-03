# Decisions

## 2026-11-03 — switched to OMNI-LOOP v4.0

The in-progress v3 wave was committed first as `ecf91db` so the branch was not left dirty. From that commit on, records follow Appendix A (`genesis-dag.json`, `current-contract.json`, `last-verdict.json`) and Appendix D (`state.json` handoff fields).

v4 changed the process in three ways that affected the work:

- New edits are atomic. The motion and viewport fix is its own iteration instead of another sweep through `App.tsx`.
- A hard-gate failure is reported as `HARD_GATE_BLOCKED` instead of a single informal score. U05 is N/A because there is no spreadsheet.
- The UI craft recipe was implemented with system Chrome, not a new test dependency.

Possibly unreal arXiv identifiers inside the harness text were not copied into the repository.

## 2026-11-03 — iteration 003

## 2026-11-03 — iteration 003

- The truncated `.bootstrap` archive stays in history and is not a source import. The working tree is the product.
- Approval payload stays in the main process. The renderer only sends an id. That is stronger than echoing the file body with a token.
- Official CLI model routes start when the user sends a message. They do not get a second approval dialog. File writes and shell commands do.
- Blender is detection only. Docs that claimed a task runner were corrected.
- `/brag` (latent-spaces/brag) is a launch-video skill. This iteration polished the repo page with real screenshots and did not render a Hyperframes video.
