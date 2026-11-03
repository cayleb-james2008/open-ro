# OMNI_LOOP.md

**Harness:** v4.0 APEX, brownfield. Switched from v3.0 on 2026-11-03 after commit `ecf91db`.
**Project:** Open-Ro Studio (`cayleb-james2008/open-ro`).
**State:** `.omni-loop/state.json`. Composite 3.31 / 5. `HARD_GATE_BLOCKED` on U01, U03, U04, U09, U12, U14.

## Resume

1. Read this file and `.omni-loop/state.json` before any edit.
2. `incumbent_commit` is the last commit that does not exist solely to stamp that field. If `HEAD` is one commit ahead and the diff only updates the stamp, continue.
3. Create tag `omni-loop/iter-5-pre` at the current HEAD, then execute `.omni-loop/current-contract.json` (sentence case and 44px targets in `src/App.tsx` and `src/styles.css`).
4. Smoke: `npm test`, then `npm run verify:ui` with the renderer on port 5173.

## Do not

Do not re-import `.bootstrap/`. Do not mark Electron, installers, live models, or Studio as verified until those commands have been run. Do not delete or weaken tests.

## Last verification

Iteration 4: `node scripts/verify-ui-craft.mjs` passed at 1440×900, 768×1024, and 375×812. `npm test` passed 13 tests. Typecheck passed.
