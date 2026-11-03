# Iteration log — latest five

## Iteration 006 (Wave 3) - 2026-11-03T14:26:00Z
- **Status:** ACCEPTED
- **Active Harness & Target Dimension:** H1_SYSTEMS_CODE -> U06 User experience
- **UEVAM Score & Breakdown:** 11.2 (Impact=4, Severity=3, Conf=0.85, Unlocks=2, VoI=1, ASPO_Sub=1.5, Effort=2, Risk=0.4)
- **Baseline Commit & Rollback Tag:** `bbbddc2379a50eece43f20b79606e81e17bef4f8` / `omni-loop/iter-6-pre`
- **Atomic Improvement Attempted:** Persist the selected project folder in app state and restore it on startup.
- **Problem Reproduction Evidence:** `projectRoot` lived only in a `let` in `electron/main.ts`.
- **Skills Routed (RADEG Passed):** Ponytail rung 2. The existing provider state file already owns `state.json`.
- **Files Changed & Net LOC:** `electron/providerState.ts`, `electron/main.ts`, `tests/safety.test.ts`.
- **Acceptance & Metamorphic Oracles Run:** `npm test` exit 0, 14 passed. `npm run typecheck` exit 0. `npm run lint` exit 0.
- **Anti-Tamper Audit:** PASS. The new test adds an assertion. No existing assertion was weakened.
- **Browser / Visual Evidence:** No UI change.
- **Clean-Room 6-Role Evaluator Verdict:** `.omni-loop/last-verdict.json`, decision ACCEPT. Electron was not launched, so startup restore is code plus a pure round-trip test.
- **Scorecard Delta:** U06 3.5 -> 3.8. Composite 3.35 -> 3.36. Still HARD_GATE_BLOCKED.
- **Grader Co-Evolution / Reusable Lesson:** Empty, whitespace, and `.` roots stay absent instead of becoming the working directory.
- **Next Highest-UEVAM Opportunity:** ITER-007 render error boundary.

# Earlier iterations

## Iteration 005 (Wave 2) - 2026-11-03T14:21:00Z
- **Status:** ACCEPTED
- **Active Harness & Target Dimension:** H1_SYSTEMS_CODE -> U09 Accessibility
- **UEVAM Score & Breakdown:** 10.5 (Impact=4, Severity=4, Conf=0.8, Unlocks=1, VoI=1, ASPO_Sub=1.4, Effort=2, Risk=0.4)
- **Baseline Commit & Rollback Tag:** `67e60d920b05981a94328c5fbb35fe655aace9f6` / `omni-loop/iter-5-pre`
- **Atomic Improvement Attempted:** Sentence-case the shell labels and raise primary controls to 44px.
- **Problem Reproduction Evidence:** Uppercase strings in `src/App.tsx` and control heights of 26–36px in `src/styles.css`.
- **Skills Routed (RADEG Passed):** Unslop pass 5 (sentence case). WCAG 2.2 target size. Ponytail rung 4, CSS only.
- **Files Changed & Net LOC:** `src/App.tsx`, `src/styles.css`, and loop records.
- **Acceptance & Metamorphic Oracles Run:** `node scripts/verify-ui-craft.mjs` exit 0. `npm test` exit 0, 13 passed. `npm run typecheck` exit 0.
- **Anti-Tamper Audit:** PASS. `tests/safety.test.ts` hash unchanged.
- **Browser / Visual Evidence:** Craft gate screenshots refreshed for desktop, tablet, and mobile.
- **Clean-Room 6-Role Evaluator Verdict:** `.omni-loop/last-verdict.json`, decision ACCEPT.
- **Scorecard Delta:** Composite 3.31 -> 3.35. U07 3.0 -> 3.4. U09 3.6 -> 3.9. U09 floor 4.0 is still not claimed.
- **Grader Co-Evolution / Reusable Lesson:** The same craft gate catches overflow after the height change.
- **Next Highest-UEVAM Opportunity:** ITER-006 persist the selected project folder.

# Earlier iterations

## Iteration 004 (Wave 1) - 2026-11-03T14:19:00Z
- **Status:** ACCEPTED
- **Active Harness & Target Dimension:** H1_SYSTEMS_CODE -> U08 Motion, with U09 viewport overflow
- **UEVAM Score & Breakdown:** 9.4 (Impact=4, Severity=3, Conf=0.9, Unlocks=1, VoI=2, ASPO_Sub=1.2, Effort=1.5, Risk=0.3)
- **Baseline Commit & Rollback Tag:** `ecf91dbc50ab12ca482a94be76daaaf792ba6f57` / `omni-loop/iter-4-pre`
- **Atomic Improvement Attempted:** Replace shorthand transitions and stop the narrow shell from overflowing.
- **Problem Reproduction Evidence:** `transition: .15s ease` computes to `transition-property: all`. A 375px row layout with a 252px sidebar overflows.
- **Skills Routed (RADEG Passed):** Emil Kowalski motion (explicit properties, 160ms, reduced-motion color fade). Ponytail rung 4 (Chrome DevTools already on the machine, no new package).
- **Files Changed & Net LOC:** `src/styles.css`, `scripts/verify-ui-craft.mjs`, `package.json`, plus v4 loop records.
- **Acceptance & Metamorphic Oracles Run:** `node scripts/verify-ui-craft.mjs` exit 0. `npm test` exit 0, 13 passed. `tsc` exit 0.
- **Anti-Tamper Audit:** PASS. Test hashes unchanged from `ecf91db`. 0 suppressions.
- **Browser / Visual Evidence:** `.omni-loop/screenshots/route_desktop.png`, `route_tablet.png`, `route_mobile.png`.
- **Clean-Room 6-Role Evaluator Verdict:** see `.omni-loop/last-verdict.json`. Decision ACCEPT. Project remains HARD_GATE_BLOCKED.
- **Scorecard Delta:** Universal composite 3.21 -> 3.31. U08 2.5 -> 3.6. U09 3.2 -> 3.6.
- **Grader Co-Evolution / Reusable Lesson:** Appendix C.1 is now `scripts/verify-ui-craft.mjs` without adding Playwright.
- **Next Highest-UEVAM Opportunity:** ITER-005 sentence case and 44px targets.

# Earlier iterations

## Iteration 003 — 2026-11-03
- **Status:** ACCEPTED after local verification. Not yet a desktop or installer proof.
- **Branch:** `cursor/quality-waves-7ac4`
- **What changed:**
  - Project tools ignore a renderer-supplied root and reject symlink escapes, prefix siblings, ignored folders, and likely secret names.
  - Agent writes and shell commands are queued in the main process. Approval uses an id. Rejecting does not write or run anything.
  - MCP stdio accepts Content-Length frames and newline JSON, and child processes get a scrubbed environment.
  - Provider records keep model IDs. Ollama stays visible without a key. Deleting the active provider clears it. Custom endpoints may omit a key.
  - Sidebar reaches Chat, Models, Workspace, and Integrations. Git is listed. Studio MCP links to `https://create.roblox.com/docs/studio/mcp`.
  - Chat sessions persist under user data. External links stay on https and do not navigate the window away.
  - Readable type and contrast floors, dialog focus trap, and a favicon that matches the in-app mark.
- **Commands:** `npm ci`; `npm run typecheck` pass; `npm run lint` pass; `npm test` 13 pass; `npm run build` pass. Headless Chrome clicked the four screens at 1280×800 and 960×700 with no page errors.
- **Not run:** Electron, `package:win`, `package:linux`, live model calls.
- **Rollback:** revert the iteration 003 commit. Do not restore `.bootstrap` import.
- **Next:** desktop launch and packaging receipts, then persisted project root and tool-result continuation. See `.omni-loop/state.json`.

# Earlier iterations

## Iteration 001 — 2026-11-03
- **Status:** INCONCLUSIVE (blocked at evidentiary gate; no code change to product)
- **Baseline commit:** `137fc161065cbd43f07de30793c790547a5c8aa6` (main)
- **Improvement attempted:** Establish verified baseline per OMNI-LOOP intake (Phase B).
- **Problem evidence:** Working tree has no source; real source exists only inside
  `.bootstrap/parts/*.b64` fragments reconstructed by `source-import.yml`.
- **Actions and evidence gathered:**
  1. Reconstructed `open-ro.tar.zst` locally: `base64 --decode` of concatenated
     parts → 55,832 bytes, sha256 `98e29e8fc1d1e2ba9dd0a4f2caaeb8725ddbab95355da4a44b690e9c74763093`.
  2. zstd decode (python `zstandard`): 126,976 bytes decompressed OK, then
     `Data corruption detected` at input byte 55,827 — two bytes from archive end.
     Incremental per-byte decode confirms every byte before that decodes
     sequentially ⇒ the frame is **truncated** (missing tail), not photo-corrupted.
  3. Parsed the valid tar prefix: 39 headers, 29 complete members recovered;
     corruption cuts inside `open-ro/src/styles.css` (28,133 bytes declared,
     ~24.8 KB recovered).
  4. `git ls-remote origin` → only `main`. Full GitHub repo inventory (paginated,
     asc+desc) → no alternate Open-Ro source repo/snapshot. `talos-workspace`
     (recovery-hands hub) last pushed 2026-10-08, predates open-ro (2026-11-02),
     contains no open-ro pin.
  5. Git history of parts shows repeated mid-run repairs (commits `f5027e4`,
     `55932a3`, `da1936f`→`3f7d7c2`), matching the "import diagnostics" commit
     `137fc16` — the staging pipeline has known instability.
- **Files changed:** added `OMNI_LOOP.md`, `.omni-loop/{baseline.md,
  opportunity-backlog.md, iteration-log.md, recovery-report.txt}`,
  `.omni-loop/recovered-2026-11-03/` (29 recovered files + 1 partial). Uncommitted.
- **Acceptance criteria / result:** "Verified, reproducible baseline of the
  product" — NOT MET; blocked. Documentation baseline: MET (baseline.md).
- **Tests/commands actually run:** git status/log/ls-remote, base64+zstd
  reconstruction, tar prefix parse + guarded extraction. No project tests can run
  (no package.json).
- **Evaluator findings:** boundaries and conditions per omni-loop §9.1 hard gates —
  evidence needed to proceed (full archive) is not obtainable in this environment.
- **Risks:** none introduced; workspace changes are additive and uncommitted.
- **Rollback/checkpoint:** delete `OMNI_LOOP.md` and `.omni-loop/` to revert.
- **Scorecard changes:** baseline established (see baseline.md); product scores stay "unknown".
- **Reusable lesson:** validate archive integrity (frame size, checksum, tar
  round-trip, member count vs manifest) **before staging parts**, not inside CI.
- **Next opportunity:** B1 in opportunity-backlog.md — acquire/verify the full
  archive (owner-gated), then B2 verified build baseline.

## Iteration 002 — 2026-11-03
- **Status:** ACCEPTED — verified within state boundaries; explicit caveat applies
- **Baseline commit:** `137fc161065cbd43f07de30793c790547a5c8aa6` (main); all changes
  remain uncommitted on `freebuff/8651cb99a120ef99f53ffd58` (no push authorization).
- **Improvement:** Rebuild the missing 0.1.0 source tree by hand. **User-authorized**
  decision after iteration 001 ended BLOCKED (user chose the rebuild option after
  being told omni-loop normally forbids fabricated source).
- **Honesty caveat (kept visible):** the rebuilt code is a reconstruction from the
  recovered docs/specs (PROJECT.md, ARCHITECTURE.md, AUTH.md, PRODUCT-SPEC.md,
  BUILD-STATUS.md, ci.yml, tests) — it is NOT provenance-verified original 0.1.0
  source. The 29 recovered originals are exact; the rebuilt modules are faithful
  reconstructions, and BUILD-STATUS vocabulary (Rebuilt vs Recovered) is used in the
  report. Runtime remains unverified (headless sandbox cannot launch the GUI app).
- **Rebuilt files:** package.json (+electron-builder config, 8 npm scripts per
  recovered README/INSTALL/CI contract), electron/{types,providerCatalog,
  secureStore,capabilities,mcp,agent,main,preload}.ts (8 modules per the
  ARCHITECTURE §2 table), src/{main.tsx,App.tsx,api.ts,vite-env.d.ts},
  src/styles.css (recovered partial bytes 1–202 preserved verbatim; lost ~5.6 KB
  tail reconstructed), vitest.config.ts (scoping). Restored all 29 recovered
  originals to their real paths (docs/, tests/, configs, README, LICENSE, .github/).
- **Toolchain accommodations (logged):** TypeScript 7.0.2 removed the legacy
  `moduleResolution: Node` (node10) used by the recovered tsconfig.electron.json —
  replaced with module/moduleResolution Node16 (CJS output unchanged; no "type":
  "module" in package.json). No other recovered file was edited.
- **Commands actually run and results:**
  - `npm install --no-audit --no-fund` → OK, 364 packages, 22 s.
  - `npm run typecheck` → PASS (tsconfig.electron + tsconfig.renderer --noEmit).
  - `npm test` → PASS, 3/3 (recovered tests/providerCatalog.test.ts unchanged,
    against rebuilt electron/providerCatalog.ts).
  - `npm run build` → PASS: tsc electron emitted dist-electron/*.js (9 modules),
    `vite build` produced dist/ (index 0.78 kB, JS 250.6 kB, CSS 21.4 kB gzip 5.47).
- **Not verified (kept honest):** Electron runtime/app launch (no GUI in sandbox),
  live provider inference, packaging (`npm run package:*`), secureStorage under a
  real keyring, Studio/Blender workflows. These map 1:1 to the recovered
  BUILD-STATUS "Not verified" list — same boundary as the original 0.1.0 drop.
- **Files changed (full set uncommitted):** everything above + OMNI_LOOP.md,
  .omni-loop/* (loop records), .omni-loop/recovered-2026-11-03/* (original 0.1.0
  source recovery of the archive; kept as evidence). No committed/pushed changes.
- **Rollback/checkpoint:** `git clean -fdx` + restore .bootstrap staging is not
  required—unstaged; removal of untracked set below is the revert path:
  `git checkout -- .gitignore LICENSE README.md && git clean -fdx -e .omni-loop`.
- **Scorecard changes:** Reliability 0→3 (import pipeline bypassed by local build,
  checks green), Maintainability 2→4, Architecture still 4/5 (implementation now
  exists and matches documented module table), Operations 1→2. Product
  functionality remains Until runtime verification (Electron launch + real-machine
  receipts) nothing may be promoted to Verified (ROADMAP status vocabulary).
- **Reusable lesson:** when a repo is bootstrap-staged, validate the whole frame
  before staging parts; when source is unrecoverable, spec-driven reconstruction
  plus the recovered pre-existing test suite (executed for real) is the honest
  alternative — but every acceptance is conditional until runtime evidence.
- **Next opportunity:** B2 remains open at reduced scope (runtime + packaging
  proof), and B7/M0 exit-gate prep is the top value item now that the tree builds.
