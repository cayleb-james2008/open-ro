# OMNI_LOOP.md

**Harness:** v4.0 APEX, brownfield. Switched from v3.0 on 2026-11-03 after commit `ecf91db`.
**Project:** Open-Ro Studio (`cayleb-james2008/open-ro`).
**State:** `.omni-loop/state.json`. Composite 3.38 / 5. `HARD_GATE_BLOCKED` on U01, U03, U04, U09, U12, U14.

## Resume

1. Read this file and `.omni-loop/state.json` before any edit.
2. Pull request 1 was rebase-merged, so the pre-merge SHAs are not on main. `incumbent_commit` is `01ccd8ebacd5fa763ef20d5150ac9ed40efac2f6`. If `origin/main` is ahead of that by a screenshot refresh, resume from the tip.
3. Create tag `omni-loop/iter-8-pre` at the current HEAD, then execute `.omni-loop/current-contract.json` (flatten the connection-card gradient and status glow).
4. Smoke: `npm test`, then `npm run verify:ui` with the renderer on port 5173.

## Do not

Do not re-import `.bootstrap/`. Do not mark Electron, installers, live models, or Studio as verified until those commands have been run. Do not delete or weaken tests.

## Last verification

Iteration 5: sentence case and 44px controls. `node scripts/verify-ui-craft.mjs` passed at 1440×900, 768×1024, and 375×812. `npm test` passed 13 tests. Typecheck passed.
