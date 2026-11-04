# Acceptance checks

Open-Ro can read a project-scoped acceptance definition from
`.openro/acceptance.json`. The definition is versioned with the project and is
injected into the recorded repair plan for the matching playtest scenario. The
playtest gate also requires the report's assertion and state multisets to match
the declared checks exactly, and it enforces declared file checks, so a model
cannot claim a pass with missing/extra checks or a required file missing.

The file is optional. The app never writes it, downloads models, or approves a
change on its own.

## Schema (version 1)

```json
{
  "version": 1,
  "checks": [
    {
      "id": "coin-count",
      "scenario": "Coin pickup",
      "type": "assertion",
      "description": "coin count",
      "expected": "1"
    },
    {
      "id": "door-enabled",
      "scenario": "Coin pickup",
      "type": "state",
      "instancePath": "Workspace.Door",
      "property": "Enabled",
      "expected": "true"
    },
    {
      "id": "coin-report",
      "scenario": "Coin pickup",
      "type": "file",
      "path": "reports/coin.txt",
      "contains": "collected"
    }
  ],
  "screenshots": ["reports/coin.png"],
  "repairHint": "Make one focused change, then re-run report_playtest with the checks above."
}
```

- `version` must be `1`.
- `checks` is a list of 1-50 checks. Each check needs a unique `id`, a
  `scenario`, a `type`, and an `expected`/`contains` value where applicable.
- `assertion` checks use `description` and `expected`.
- `state` checks use `instancePath`, `property` and `expected`.
- `file` checks use a project-relative `path` plus `contains` and/or `expected`.
  `expected` compares the whole file. `contains` is a substring.
- `screenshots` is an optional list of up to 10 project-relative image paths.
  They are included in the repair prompt as evidence paths.
- `repairHint` is optional project-authored guidance included only in the repair
  prompt. It is never executed.

## Limits and safety

- Scenario keys are capped at 200 characters, matching the ledger key. The
  parser, repair plan and gate all use the same key.
- A definition is capped at 256 KB on disk, 12,000 raw text characters, and a
  worst-case rendered prompt of 15,000 characters (JSON escaping included).
- File and screenshot paths must be relative, stay inside the project, and may
  not use ignored folders (`.git`, `node_modules`, `dist`, `dist-electron`,
  `release`, `coverage`) or likely secret names (`.env`, keys, credentials).
- A malformed or unreadable definition makes `report_playtest` fail closed. A
  missing definition is simply ignored.
- Assertion and state grading is unchanged. The report's assertion/state
  multisets must match the declared checks for the scenario; missing, extra,
  duplicate-count and wrong-expected checks fail. File checks run after grading
  and can only make a verdict stricter. A missing file or set mismatch turns a
  PASSED report into a FAILED verdict and preserves any earlier failing check.
- The repair plan still requires approval for writes, commands, Studio edits and
  media. There is no auto-approval.

## Workflow

1. Record a playtest failure with `report_playtest`.
2. The ledger derives the newest open repair plan.
3. The Workspace shows the scenario, failing check, attempt and reason. **Run
   repair** sends the plan instruction, including only the matching scenario's
   acceptance definition, in a fresh project-bound chat.
4. The agent proposes and stages a change. If the first turn stages nothing, one
   automatic write nudge derived from the file checks asks for the focused write.
   Approve it explicitly; nothing runs before approval.
5. The app automatically continues the same chat with the plan checks. If the
   model skips the re-test, one automatic nudge supplies the exact JSON shape
   (with `<observed>` placeholders). The agent calls `report_playtest` again; the
   gate grades the assertions and state checks, verifies declared files, and
   records the verdict. The report's assertion and state sets must match the
   declared multisets exactly for the scenario; missing or extra checks fail.
   A follow-up that stages another approval is not nudged again until that
   approval is decided.
6. A PASS closes the plan; another failure reopens it with the preserved failing
   check.

## Verification

- Unit and integration tests: `npx.cmd vitest run tests/acceptanceChecks.test.ts`.
- Full suite: `npm test` (297 tests in 37 files at this checkpoint).
- Final-build gate smoke: `node C:\Users\KK\AppData\Local\Temp\opencode\cdp-final-gate-smoke.cjs`
  (missing file -> FAILED, approved write -> PASSED, invalid definition ->
  FAILED).
- Exact-set smoke: `node C:\Users\KK\AppData\Local\Temp\opencode\cdp-acceptance-set.cjs`
  (extra assertion -> FAILED `unexpected "Enabled property / true"`; exact set ->
  PASSED).
- Real-model receipt verifier: `node C:\Users\KK\AppData\Local\Temp\opencode\verify-ollama-auto-repair.cjs`.

On the final packaged build, real qwen2.5:3b staged `repair-probe.txt` and the
human only approved the write. The automatic report nudge produced PASSED in
about 197s with exactly 1 assertion, 1 state check and 1 screenshot; the plan
closed. An earlier run needed the automatic write nudge because the first turn
staged nothing. Automatic messages are marked `[automatic repair step]`. A
llama3.2:3b run reported without the file and the gate recorded it FAILED with
`acceptance file ... is missing or unreadable`.

The final-build exact-set smoke rejected an extra assertion with
`Acceptance assertion/state set failed: assertion set does not match (unexpected
"Enabled property / true")`, then accepted the declared set.

Residual limits: the set comparator trims whitespace and does not compare the
report's `actual` values to the declared expected values beyond the existing
`gradePlaytest` check; screenshots are verified for readability but not enforced
as an exact declared set. The 29-byte fixture PNG passes the pre-existing
signature-only image check. Unattended approval-free repair is not a goal;
writes still need a human approval.
