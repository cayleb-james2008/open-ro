# A project run

Improve the project in the working directory until it meets a strict written bar. There is no iteration limit. The order is fixed: fix what is already known to be wrong, cheaply; then hunt for what was missed; then prove it. The rules in `SKILL.md` hold throughout. This file adds the state on disk and the three phases.

Your context does not survive between runs. Everything the next run needs is on disk under `.loop/`, and every run starts from disk.

## Each run

1. **First run** (no `.loop/STATE.md`): copy this skill's `SKILL.md`, `references/` and `scripts/` into `.loop/guide/`, unless a driver already has, so that later runs and helpers read the same version from disk. Then follow `references/setup.md`.
2. **Later runs:** read `.loop/STATE.md`. If `.loop/ANSWERS.md` holds anything, apply each answer, then move it into the ledger with what you did, leaving that file empty. A run started while the status is `waiting` on a checkpoint is the owner's word to carry on: set the next checkpoint if he gave a repeating one. Then work out the status again. Read `references/helpers.md` before you start any helper, and `references/hosts.md` when a label in the state file says something is missing or a helper call fails.
3. Do what the `next:` line says, and keep working while the status is `setup` or `running`.
4. **Ending a run.** End a run only when the status is `waiting` or `done`, or when your context is nearly full. You have no clock for anyone's time or budget cap: the driver or the owner stops a run for those, never you. Before you end: stop any server you started, commit the project, update `.loop/STATE.md`, write the report to `.loop/REPORT.md`, then run `bash .loop/guide/scripts/loop_check.sh` and fix whatever it prints. The report is also your closing message.

A later run is started with one line: `Continue the ultra loop: read .loop/STATE.md, .loop/guide/SKILL.md and .loop/guide/references/project.md.` In a session with the owner, keep going through context resets: after one, start again at step 2. The driver in `hosts.md` is for running with nobody there.

## Files and status

| File | Holds |
|---|---|
| `.loop/guide/` | This skill, as copied on the first run. |
| `.loop/STATE.md` | Line 1: `status:` and one of `setup`, `running`, `waiting`, `done`. Line 2: `next:` and the next action. Then: phase; last kept commit; host labels; each condition's latest result (met, not met, not judged, not run, owner, disputed); the sealed checks' hash; kept changes since the last review; the checkpoint and when it is next due; the last report's commit; a line `tags: <prefix>` if this run's tags carry one; "Owner brief"; "Needs from you". Short and current. |
| `.loop/ANSWERS.md` | Where the owner writes: answers to numbered items ("N1: yes"), verdicts on `owner` conditions ("C7: met"), new wishes. Empty once applied. |
| `.loop/BAR.md` | The numbered pass conditions, as text only. Append-only: to tighten a condition, add a new line (`C4b`) that says which one it replaces. No line is edited or removed after the tag `loop-start`, unless the owner tells you to change one; then make the change and add the line `noted: bar <what he said>` to the ledger. |
| `.loop/BACKLOG.md` | A table with one row per defect or unmet condition, its status in the last column: `open`, `kept`, `waiting`, `disputed`, `rejected` or `skipped`. A finding that did not reproduce is recorded as `rejected` with one line of why. A row this host cannot work is `skipped`, with the label. Rows are never deleted, unless the owner tells you to drop one; then add the line `noted: backlog <id> <what he said>` to the ledger. A defect that was handed to you and that you have not reproduced yet is not a row: it goes in a plain list headed "Leads" under the table. |
| `.loop/LEDGER.md` | One appended row per attempt (a change and its repairs): row id, the check that failed first and the message it failed with, commit, kept or reverted or no change, the cause in one line. Also the owner's applied answers. Only appended to, unless the owner tells you to correct or remove a line; then add the line `noted: ledger <what he said>`. Read it before choosing work, and do not repeat an approach it shows failed. |
| `.loop/REPORT.md` | The latest report. |
| `.loop/live/` | Read-only checks of a running system, such as the soak in `live.md`. Never part of the kit. |
| `.loop/verify`, `.loop/checks/` | The kit: one command, `bash .loop/verify <label>`, that runs every check, and the check scripts it calls. |

A fix and its tests go in one commit, and its message starts with the row id (`B7: reject negative quantities`). The `.loop/` bookkeeping for an attempt goes in one commit after it, and its message starts with `loop:`. The kit changes only in a bookkeeping commit; it finds the project's tests by pattern, so a new test needs no change to it. Leave no scratch files in the project; scratch goes under `.loop/tmp/`.

**Status** is the first of these that holds:

1. Set-up is unfinished: `setup`.
2. The owner's checkpoint is due: `waiting`.
3. There is work you can do, meaning an `open` row or an unfinished step of the current phase: `running`.
4. A row is `waiting` on the owner: `waiting`.
5. Otherwise: `done`.

No other word is a status. A run that is stuck, blocked or out of ideas still has the status this rule gives. A step this host cannot do is skipped, recorded with its label, and is not work. `disputed` and `skipped` rows, `owner` conditions and skipped steps do not hold up `done`; the report lists them all. If the owner says to stop or to ship it as it stands, give each row that is still `open` or `waiting` the status `disputed` with the note "owner: stop"; the rule then gives `done`. `done` is not a claim of perfection. It means every condition the loop could check passes and fresh helpers found nothing more that reproduces; on a host with no helpers the report says "done, self-checked". `waiting` and `done` end the run.

An owner's checkpoint is a count of kept changes or the end of a phase, never a time.

## The loop

### Phase 1: fix what is known

Repeat while the backlog has an `open` row:

1. **Pick** one row: a regression first; then data loss, a security hole or a broken main job; then the most reproduced impact for the least effort. Rows that are `waiting` on the owner are never picked.
2. **Reproduce** it with a test or check that fails for the reason you expect. If it fails for another reason, fix the check. Work with no natural failing check (a refactor, a speed-up, a prose rewrite) needs a threshold or a `judge` condition written before the change. A `judge` condition is checked by one fresh helper who first gets its example pair right. With no helper, use a stand-in a script can check (the quoted text is in the source, the link resolves, the number recomputes), or, where no stand-in exists, mark the row `skipped` with the label `review=self` and list it in the report. `waiting` is only for something the owner can supply.
3. **Change** the project (rule 4) and commit the change with its tests.
4. **Run `.loop/verify`.** Keep the change when its check passes and nothing that passed before fails. For a change aimed at a `metric`, measure three times before and after and keep it only if the medians differ by more than the spread. A change that touches security, money, sign-in, data loss or anything irreversible is also reviewed before it is kept, by two reviewers, one of them adversarial.
5. **Repair or revert.** A change that does not hold may be repaired, at most twice. A repair that fails for the same cause as the one before means the plan is wrong: revert with a revert commit, then try a different plan. If that fails too, the row is stuck: mark a high or medium one `waiting` with the cause and list it under "Needs from you", and a low one `disputed`. `disputed` otherwise means the condition cannot be met honestly or its check is wrong. "No change is justified" is a valid result.
6. **Log** one ledger row and update the backlog and the state file.

**Judges.** A judge's question and its example pair are part of the bar and stay as written. No condition is judged more than twice in a run. Never reword the question, and never remove something the documents require, to get a yes.

**Review.** At the end of each phase, before `done`, and at least every ten kept changes, give one reviewer everything kept since the last review, in a frozen copy outside the project that holds none of your notes (`helpers.md`). Reproduce each finding: a reproduced one becomes a row with a failing check; one that does not reproduce becomes a `rejected` row. A finding about something older than the changes under review is a new row, not a failed review.

**Plateau.** If six attempts in a row are not kept, counting from the last kept change or the last answer from the owner, stop repeating. Try one different approach from the last kept commit and log it as such. If that also fails, the rows involved are stuck: mark them as in step 5 and go on with the others.

**Checkpoint.** If the owner set one, record it in the state file. When it is reached, run a review, report, and end the run with `status: waiting`.

### Phase 2: hunt

When no `open` row remains, look for what the backlog missed. Start finders with fresh contexts: one per lens, two lenses on a small project and four on a large or high-stakes one, plus one checks finder every time. The lenses are the floors for this kind of project (`references/kinds.md`, `references/interfaces.md`). The checks finder looks for a defect the kit cannot see; for tests that would still pass with a fix removed, or that depend on the clock, the network or the order they run in; and for anything that got worse since `loop-baseline` (size, duplication, speed, a feature that went missing). Reproduce each finding yourself. A reproduced finding adds a condition to the bar, a failing check and an `open` row, and you return to phase 1. A finding is cosmetic when it breaks nothing that the bar, the project's documents or the floors require: a preference for one word or one layout over another is cosmetic however easy it is to write a check for it. Cosmetic findings are listed in the report and not worked. A hunt that adds nothing above cosmetic is a dry hunt.

### Phase 3: prove

After a dry hunt, in this order, cheapest first:

1. **The whole diff.** Read everything changed since `loop-baseline` for anything that recognises a check: a hard-coded expected value, a special case for a test input, a guard switched off.
2. **The kit.** In a code project, undo each fix in turn in a scratch copy and confirm some check fails. Add a check wherever none does.
3. **Sealed checks.** Have an examiner write acceptance checks from the bar, outside the project, and record the folder and the hash it returns. An exam runner then runs them on a clean checkout of your last kept commit and tells you only which conditions failed. You never open that folder or run its command yourself. Write in the ledger which helper ran them, on which commit, and what it returned. For each failed condition, strengthen your own check until it fails, then fix in phase 1. If two tries cannot reproduce a sealed failure, ask a reviewer to look at that one case and say whether the check or the project is wrong. Sealed failures of `disputed` conditions are listed, not worked. When the bar has gained conditions since the checks were written, a fresh examiner adds to them and returns a new hash.
4. **Judges.** For each `judge` condition, three fresh judges, each of whom first gets its example pair right, grade the artifact. It is met when at least two say yes. A defect that a judge names becomes a row if you can reproduce it, and its fix does not send the condition back to the judges. A condition that fewer than two judges pass gets one more round after its fixes; if it fails again, mark it `disputed`.
5. **A second hunt**, with new finders.

A fix found in phase 3 goes through phase 1 and a review, and then phase 3 starts again at step 1 on the new commit, with step 4 under its own limit of two rounds; do not repeat phase 2. When all five steps pass on one commit and the last review found nothing that reproduces, no work remains, and the status follows from the rule above.

## The report

End every run with this, built from the state file, the ledger and the backlog, and save it as `.loop/REPORT.md`. Give the lists in full; keep everything else under 150 words.

    <line 1 of STATE.md>   phase: <1|2|3>   <host labels>
    Kept: one line per kept change since the last report, with its commit
    Reverted, disputed or rejected: one line each, with the cause
    Bar: how many conditions are met, not met, not judged, not run, disputed; the sealed result if phase 3 ran
    Owner review: every owner condition, with where to look
    Not verified: what no check covered; steps this host skipped; anything nobody watched or heard
    Needs from you: each item with its number (N1, N2) and the exact step or question, and the assumption taken.
                    Answer in .loop/ANSWERS.md, one line each ("N1: yes"), then start the loop again

Do not describe the work as better than the evidence shows.

## Red flags in a project run

| Thought | Do instead |
|---|---|
| "I will build the whole kit and every check before I fix anything." | Set-up is short. Start fixing; checks are added as defects are worked. |
| "The tests pass, so the project is done." | Done is what the status rule gives after a dry hunt and phase 3. |
| "I am blocked, so the status is blocked." | Work out the status from the rule. A blocker is an item under "Needs from you". |
| "Time must be nearly up, so I will stop here." | You have no clock. Keep working until the status ends the run. |
| "The judge still says no, so I will rewrite the question, or cut the part it dislikes." | Two attempts, then `disputed`. The question and the required content stay. |
| "I have the exam command, so I will run it myself." | Only the exam runner runs it. |
| "These three fixes are related, so one commit will do." | One row, one commit. |
| "I will start the helpers and end my turn while they work." | Wait for them in the same run. In an unattended run a helper left behind is lost. |
