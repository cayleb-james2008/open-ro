# Ultra-loop report — OpenRO scale-up (17–18 Nov 2026)

**Goal (approved, un-drifted):** you describe the game, it builds it —
proactive creative agent in a plain chat window, full access by default,
can one-shot a game.

**Bar:** `.loop/BAR.md` — conditions C1–C12 + DIRECTION v1 (Studio
workbench at night, blue accent, dark default; signature: every change
set reversible, Revert always present). C11 is `owner`.

**Baseline:** tags `loop-baseline` 10dfece → `loop-start` c51148c.

## Improve pass (impeccable discipline, applied to the surface)
- Themed shell: light + dark, toggle in Settings, dark default; honest
  Studio chip ("Studio connected" / "Studio not connected") reflecting
  real bridge state; game's name shown in the sidebar.
- Scale-up depth behind the surface: Rune skill registry as the single
  source for prompt + dispatch (5 generation skills), per-skill
  generated Luau only, place-tree awareness via the bridge, persistent
  multiple chats, checkpointed change sets with Revert always present.

## Thermonuclear review (fresh-context, both directions, surface consumption)
Four rounds, all findings reproduced before fixing; full detail in
`.loop/LEDGER.md`.
- **R1 (DO NOT SHIP, 4 critical):** hand-written-source bypass removed
  (Luau only from skills), currency escaping, arg type validation,
  done-gate, `isError` honoured, per-chat change sets on every exit
  path, bridge timeouts, documented-format playtest verdicts, strict
  mock. → 26/26.
- **R2 (DO NOT SHIP):** vacuous-playtest claim-flag bypass; revert not
  failure-atomic; checkpoint-before-attempt poisoning; error-key
  heuristic overshoot; console-error aborting repair; one-deep change
  sets → per-chat stack. → 36/36. (Capability strictness — all 7 tools
  required up front — accepted as a stance, C6 PASS WITH NOTES.)
- **R3 (NOT DONE):** claim flag rebuilt sentence-scoped + factual
  ledger line; revert identity-checked (prev AND post recorded);
  outcome lines filed to the ask's own chat; error objects need a
  message; strict changes.json/chats.json loading with backup;
  class_name validation; numeric ceilings. → 43/43.
- **R4 (confirmation):** instance `post` was fabricated from args → now
  read back from the place tree; chats.json entry-level strictness
  (round 3's fix was half true); claim-check apostrophe/semicolon
  edges. → **46/46 green. Loop converged** — remaining items LOW and
  accepted (claim-line idiom noise on a factual line).

## Sealed exam
Independently written + run, black-box, 12 checks: **12/12 PASS**,
exit 0 (run dir `~/workspace/exam-runs/openro-20261118-015134-15949`).
First attempt ran zero checks — exam.sh's extraction tar hit this
filesystem's chown restriction; patched exam-side only
(`--no-same-owner`, checks.py byte-identical), app untouched.

## Weakest remaining point
The real-Studio path is proven only against the mock MCP fixture: the
protocol, capability check and drop-in route are real, but no live
Roblox Studio session has run against this build. Second: the claim
line still has LOW idiom noise (a factual line, not an accusation —
accepted in the ledger). C11 (look) is Cayleb's grade, unmarked by
design.

## Replacement staging
`~/workspace/restart-20261117/replacement/openro/` — README.md (also the
repo's refreshed README), openro.html (résumé-site page in the current
site's editorial style; schematic walk-through from the real
integration scenario, no fabricated captures), CLAIMS.md (every claim
tied to its proof). Nothing pushed or published.
