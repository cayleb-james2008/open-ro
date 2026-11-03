---
name: Omni-Loop
description: >
  Recursive, evidence-gated project evolution engine (v4.0 APEX edition). Use when
  the user asks to run "the omni-loop", or asks to evolve / complete / improve / finish
  a project or artifact to a verified high bar. Runs one bounded, verifiable atomic
  improvement per iteration, gated by executable oracles and a clean-room evaluation,
  until the Definition of Done is met. Also handles greenfield genesis compilation
  (spec DAG + spine + oracles before feature waves).
---

# OMNI-LOOP v4.0 (APEX Edition) — operating procedure

You are operating a persistent, mathematically governed, evidence-gated project
evolution system. The complete unabridged specification — research mapping matrix,
UEVAM/ASPO calculus, Ponytail/Unslop/Impeccable/Kowalski craft protocols, 16-Dimension
scorecard, adversarial playbooks, JSON schemas, verification recipes, and the full
master prompt — is in `references/omni-loop-v4.0-apex.md`.

**Read `references/omni-loop-v4.0-apex.md` sections as needed for detail; this file is
the operational spine. When this digest and the reference disagree, the reference wins.**

## 0. Deterministic controller

All state lives under `.omni-loop/` in the project root (gitignored, local institutional
memory; the repo records ship when the repo has none). Drive the deterministic parts
through the Python controller bundled with this skill:

```bash
python skills/omni-loop/scripts/omni_loop_controller.py init            # create/repair state records (never overwrites)
python skills/omni-loop/scripts/omni_loop_controller.py status          # resume brief: mode, scores, next action
python skills/omni-loop/scripts/omni_loop_controller.py hash FILE..     # SHA-256 pre-edit hashes for the contract
python skills/omni-loop/scripts/omni_loop_controller.py verify-contract # validate .omni-loop/current-contract.json
python skills/omni-loop/scripts/omni_loop_controller.py uevam ...       # compute the Universal Expected Value score
python skills/omni-loop/scripts/omni_loop_controller.py audit-artifact [--changed-only <rev>]   # anti-tamper / placeholder / secret scan
python skills/omni-loop/scripts/omni_loop_controller.py compact-memory --window-size 5           # L1 -> L2 compaction
python skills/omni-loop/scripts/omni_loop_controller.py smoke           # run state.json["verification"]["smoke"] commands
```

Resolve the skill's base directory before the first call (the `skill` tool reports it).

## 1. Non-negotiable operating laws (fail closed)

1. One atomic improvement (or a strictly file-disjoint parallel wave) per iteration:
   max 5 files and 250 net LOC unless a written `indivisibility_proof` justifies more.
2. Smallest complete change that fixes the root cause. Zero unmotivated churn.
3. Never claim any check passed without executing it and observing the exit code.
4. Never delete/weaken tests, or add suppressions (`@ts-ignore`, `eslint-disable`,
   `# noqa`, `.skip()`, `.only()`) to force a gate green.
5. Never emit placeholders (`TBD`, `[Insert X]`, `... existing code ...`).
6. Every factual/citation claim links an exact primary URL (never a bare domain root).
7. Pre-flight checkpoint before edits: `git tag -f omni-loop/iter-<N>-pre` (or stash).
8. Hard gates (data destruction, unauthorized external action, missing credentials,
   baseline ambiguity, weakened security, evaluator starvation) stop the iteration as
   `BLOCKED`/`INCONCLUSIVE` with the exact blocker — never bypassed.
9. Report honestly: verified scores, CI on failures/inconclusives, no perfection claims.

## 2. Phase 0 intake (choose exactly one entry mode)

- **Mode 0A — greenfield genesis:** compile the prompt into `.omni-loop/genesis-dag.json`
  (explicit requirements, implicit domain standards, active Raven harnesses H1–H6,
  unslop 5-line direction, topological waves, Definition of Done). Wave 0 builds the
  architectural spine **and** the executable verification harness before anything else.
- **Mode 0B — brownfield baseline:** map the repo (`.omni-loop/product-map.md`),
  run safe read-only checks (build/typecheck/lint/tests), quarantine every
  pre-existing failure verbatim in `.omni-loop/baseline.md`, build
  `.omni-loop/feature-inventory.md` (FEAT-xxx with VERIFIED / PARTIAL / BROKEN /
  MISSING / UNKNOWN only backed by runtime evidence — code presence is never proof),
  and score the 16-dimension scorecard into `quality-scorecard.md`.

In both modes finish by initializing/refreshing `.omni-loop/state.json` (controller
`init`), setting `entry_mode`, and picking the first candidate.

## 3. The governing loop (repeat until convergence)

1. **Discover (Phase 1):** parallel fan-out of probes (requirements diff, runtime/live
   checks, test-coverage gaps, a11y/perf/security audit, visual redline). Pack outputs
   into a defect table with exact file:line or command evidence. Classify strictly:
   `Confirmed Defect` (reproduced) vs `Testable Hypothesis`.
2. **Prioritize (Phase 2):** score every candidate with the UEVAM equation (controller
   `uevam`); any verified P0 blocker gets +1000 priority. Calibrate confidence:
   runtime-reproduced 0.9–1.0, static/read-only 0.65–0.85 max 0.5. Select the single
   highest-UEVAM atomic candidate. Three stagnant loops on one dimension → fork an
   exploratory worktree branch instead of polishing the same rock.
3. **Research & route (Phase 3):** for consequential decisions compare >=2 competing
   approaches and log the winner's rationale in `decisions.md`. Gate craft skills by
   relevance; apply Ponytail 7-rung ladder, and for UI: Unslop 9 passes, Impeccable
   tokens/surface modes, Kowalski motion standards (full protocols in the reference).
4. **Contract (Phase 4):** write `.omni-loop/current-contract.json` (problem,
   reproduction evidence, atomic file list, non-goals, acceptance oracles, regression
   commands, rollback tag, uevam inputs) and run `verify-contract`. Record pre-edit
   SHA-256 hashes of existing tests/lint config via controller `hash`.
5. **Implement (Phase 5):** checkpoint tag first, then the smallest complete change
   inside the contracted files. No drive-by refactors. Add deterministic tests for the
   new behavior and its boundary conditions.
6. **Verify (Phase 6):** run every acceptance oracle, regression suite, typecheck,
   lint, build; then controller `audit-artifact` (0 deletions via hash compare, 0
   suppressions, 0 placeholders, 0 bare homepage URLs, 0 secrets). For surfaces/visual
   deliverables: render and verify the 8 interactive states at 1440x900 / 768x1024
   / 375x812 (OpenCode browser tools or repo scripts).
7. **Clean-room evaluate (Phase 7):** fresh-context evaluation with the implementer's
   self-justification stripped — pass only contract, raw diff, verifier stdout and
   screenshots to an independent reviewer. In OpenCode use a fresh `subagent` session
   (it has no implementation chain-of-thought). Six lenses: Product/Intent, Design/
   Visual, Systems/Analytical, Epistemic/Citations, Adversarial Red-Team fuzzing,
   Completeness/DoD. Write `.omni-loop/last-verdict.json` with ACCEPT / REVISE /
   HYBRIDIZE / REVERT / INCONCLUSIVE and severity-ranked findings.
8. **Decide (Phase 8):** ACCEPT on all oracles + audit + `ΔS > 0` with no P0/P1
   defects; REVISE (max 2 in-loop passes, re-run 6/7); HYBRIDIZE two archive elites
   on orthogonal dimensions; REVERT to the checkpoint tag on any regression
   (`git reset --hard omni-loop/iter-<N>-pre`), logging the lesson in
   `rejected-candidates.md`; INCONCLUSIVE when evidence/permissions are missing.
9. **Preserve (Phase 9):** update `state.json`, `feature-inventory.md`,
   `quality-scorecard.md`; append the append-only iteration record (template:
   reference §9.1) to `iteration-log.md`. If Phase 7 caught a defect Phase 6 missed,
   codify a new deterministic check now (grader co-evolution). Keep the L1 log at
   the last 5 entries via `compact-memory`.
10. **Reassess (Phase 10):** recompute UEVAM across the backlog (completed candidates
    unlock downstream nodes). Iterate Phases 1–10. Every 5 iterations or after 2
    consecutive REVERTs run the Loop D meta-audit (reference §7) before continuing.

## 4. Convergence and honesty

- Iteration stops at ACCEPTED / HYBRIDIZED / REVERTED / BLOCKED / INCONCLUSIVE.
- Declare **Quality-Stable Pareto Convergence** only when 100% of explicit
  requirements and critical journeys are VERIFIED, zero P0/P1 defects remain, and
  every active scorecard dimension >= 4.5 with all hard-gate floors met.
- "Ludicrous quality" pursuit still obeys the loop: queue ever-higher-leverage
  candidates, never churn. If the backlog drains below the risk/churn threshold,
  say so plainly instead of inventing work.
- External/paid/publishing/destructive actions require explicit user authorization.

## 5. Environment mapping (OpenCode)

| Loop concept | OpenCode mechanism |
|---|---|
| Discovery fan-out / research | Parallel subagent (`explore`), webfetch, shell probes in one turn |
| Implementation | Direct edits, `git` CLI via shell, checkpoint tags before edits |
| Deterministic verification | Shell execution of project scripts/tests (observe exit codes) |
| Visual verification | OpenCode browser tools (open, screenshot, console logs) |
| Clean-room Phase 7 | Fresh `subagent` (general) receiving contract + diff + raw verifier output |
| Parallel disjoint waves | `git worktree add` + file-scope lease locks in `.omni-loop/locks/` |
| State / records | `.omni-loop/` files + controller script |

## 6. Tracks this project environment

- Windows host: prefer `powershell`-safe quoting; use `git` from
  `C:\Program Files\Git\cmd\git.exe`; verify `python`, `node`, `npm` on PATH before
  assuming them.
- Never poll/sleep against agent-tool side effects in `execute` (Code Mode) — the
  runtime has no timers; use shell subcommands for sleeps.
