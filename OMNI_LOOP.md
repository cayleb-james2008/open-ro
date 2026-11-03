# OMNI_LOOP.md — open-ro operating state

**System:** OMNI-LOOP v3.0, brownfield mode 0B.
**Project:** Open-Ro Studio — local-first AI development studio for Roblox.
**Repo:** `cayleb-james2008/open-ro`

## Resume here

Read `.omni-loop/state.json`, `.omni-loop/opportunity-backlog.md`, and the latest entries in `.omni-loop/iteration-log.md`.

The source tree is the working application. The truncated archive under `.bootstrap/parts/` is historical and must not be re-imported. `.omni-loop/recovered-2026-11-03/` is evidence from that archive, not a second source of truth.

## Last accepted iteration

Iteration 003 on 2026-11-03. Path containment, approval queue, provider records, navigation, local chat history, and the repo-page docs were updated. Verification recorded in `.omni-loop/state.json`.

## Next waves

The ordered list is `nextWaves` in `.omni-loop/state.json`. Do not mark Electron launch, installers, live inference, or Studio as verified until those commands have been run and their output saved.

## Rules

- One wave at a time. Verify before accepting. Record the result in `.omni-loop/iteration-log.md`.
- Do not delete tests or weaken assertions to make a check pass.
- Do not commit secrets.
- Keep claims in README, SECURITY, and BUILD-STATUS tied to a command that was actually run.
