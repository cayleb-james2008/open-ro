# Acceptance checks

Open-Ro can read a project-scoped acceptance definition from
`.openro/acceptance.json`. The definition is versioned with the project and is
injected into the recorded repair plan for the matching playtest scenario. The
playtest gate also enforces declared file checks, so a model cannot claim a pass
while a required file is missing.

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
- Assertion and state grading is unchanged. File checks run after grading and
  can only make a verdict stricter. A missing file turns a PASSED report into a
  FAILED verdict and preserves any earlier failing check.
- The repair plan still requires approval for writes, commands, Studio edits and
  media. There is no auto-approval.

## Workflow

1. Record a playtest failure with `report_playtest`.
2. The ledger derives the newest open repair plan.
3. The Workspace shows the scenario, failing check, attempt and reason. **Run
   repair** sends the plan instruction, including only the matching scenario's
   acceptance definition, in a fresh project-bound chat.
4. The agent proposes and stages a change. Approve it explicitly.
5. The agent calls `report_playtest` again. The gate grades the assertions and
   state checks, verifies declared files, and records the verdict.
6. A PASS closes the plan; another failure reopens it with the preserved failing
   check.

## Verification

- Unit and integration tests: `npx.cmd vitest run tests/acceptanceChecks.test.ts`.
- Full suite: `npm test` (284 tests in 37 files at this checkpoint).
- Final-build gate smoke: `node C:\Users\KK\AppData\Local\Temp\opencode\cdp-final-gate-smoke.cjs`
  (missing file -> FAILED, approved write -> PASSED, invalid definition ->
  FAILED).
- Real-model receipt verifier: `node C:\Users\KK\AppData\Local\Temp\opencode\verify-ollama-write-receipt.cjs`.

The real qwen2.5:3b run wrote `repair-probe.txt` after approval and needed one
manual re-test continuation to reach PASSED. A llama3.2:3b run reported without
the file and the gate recorded it FAILED with `acceptance file ... is missing or
unreadable`. Fully autonomous real-model repair is not verified.
