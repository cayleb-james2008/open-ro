# Helpers

Each helper gets a fresh context and none of your reasoning. Use a different model from yours when the host offers one, and name the model in the call. Follow the owner's rule for models; without one, use a cheap model for finders and the exam runner, and a strong one for reviewers, judges and the examiner. Send each brief as written, with the bracketed parts filled in.

Two lines go into every brief:

    Budget: about 30 tool calls (an examiner may use 80). Do not start timers or background jobs.
    Do not run these project scripts, not even with --help: <those that start or drive a real system, or "none">.

For a live system, add the guard block from `live.md`. Some cheap models refuse to write report files: ask every helper to return its findings as text, and write any file yourself.

In a project run, reviewers, finders and judges work in a frozen copy of the commit under review. It sits outside the project, beside it, so that neither your notes nor your git history reach them:

    F="$(dirname "$PWD")/$(basename "$PWD").frozen"; rm -rf "$F" && mkdir -p "$F"
    git archive <commit> | tar -x -C "$F"
    (cd "$F/.loop" && rm -rf BACKLOG.md LEDGER.md STATE.md REPORT.md ANSWERS.md guide)
    git diff loop-baseline <commit> -- . ':(exclude).loop' > "$F/CHANGES.diff"
    git diff loop-start <commit> -- .loop/BAR.md > "$F/BAR-CHANGES.diff"

Give the helper that folder as the project: it holds the project, the bar, the kit, the changes since the start and the changes to the bar, and none of your notes. Use the run's tag prefix if it has one, and also remove any older loop's records from the copy (for example `.omni-loop/`). Remove the folder when the helper has reported. If the parent folder is not writable, use `mktemp -d`. Without version control, copy the project there by hand and leave the same files out. Give a reviewer any `loop check note:` lines the loop check printed; each is a lead to confirm or dismiss.

## Maker

Use this when the owner's rule for models sends the building itself to a helper. One maker at a time per file. Its work is verified by you and reviewed by a different helper, like any other change.

    You are fixing one defect in a project: <row id and what is wrong>, reproduced by <command or steps>.
    1. Write the test or check first and see it fail for that reason. Then make the smallest complete
       change that fixes the cause. Add nothing else.
    2. Commit after each working stage, so that nothing is lost if you are stopped. Each message starts
       with the row id (<id>: ...). Do not touch anything under .loop/.
    3. Run <the kit command> and return: the commit ids, the check that failed first and its message,
       the kit's result line, and anything you could not do.

## Reviewer

    You are reviewing work you did not make. Your job is to find where it fails, not to approve it.
    You have the bar, the changes to examine, and how to run the project. You do not have the maker's
    reasoning; do not look for it. Do not edit anything.
    1. Run or open the thing yourself. For each bar condition the changes touch, answer met, not met or
       cannot tell, with the command and output, or the steps and what happened. A yes needs something you ran.
    2. Try to break it: empty, missing and hostile input, boundaries, ten times the size the tests use,
       repeated runs, the narrowest and widest screen, the keyboard alone.
    3. Attack the checks: would each new test still pass with its fix removed? Was any test, threshold or
       check weakened? Did anything change that no condition asked for, or did a documented feature
       go missing? If the folder holds BAR-CHANGES.diff, read it: a removed or edited line of the bar is a
       defect.
    4. Report every defect you reproduced, with the steps. High: a bar condition fails, data is lost or
       exposed, a fact is invented. Medium: a robustness, edge-case, accessibility or missing-state gap.
       Low: cosmetic. Say for each whether these changes introduced it or it was there before.
       List feature ideas separately; they are not defects.
    State the commit or the files you examined and list what you actually ran. A verdict with nothing run
    does not count.

Adversarial line, added for a second reviewer only:

    Assume the change is wrong and its checks are weak. Show it.

## Finder

    You are looking for defects in a project through one lens: <lens>. You did not build it. Read
    .loop/BAR.md, then use the project the way its users would. Report only what you reproduced: exact
    steps or a command, the result, what the bar or the documents say should happen, and a severity.
    Do not edit files. Do not read .loop/BACKLOG.md or .loop/LEDGER.md. Say plainly if you found nothing.

Checks finder, used once in every hunt in place of a lens:

    You are looking for weaknesses in a project's checks. You did not build it. Read .loop/BAR.md,
    .loop/verify, the tests and CHANGES.diff. Look for: a defect the checks cannot see; a test that would
    still pass if its fix were removed (show it by undoing the fix in a scratch copy); a test that depends
    on the clock, the network or the order it runs in; anything that got worse in CHANGES.diff (size,
    duplication, speed, a feature that went missing). Report only what you reproduced, with the steps.
    Do not edit the project. Say plainly if you found nothing.

## Judge

The question and the two examples are the ones written in the bar, unchanged from round to round.

    Answer one yes-or-no question about an artifact: <question>. First answer it for these two examples:
    <example that should pass>, <example that should fail>. Then answer it for <the artifact>, opening or
    running it yourself. If the answer is no, name the single most concrete defect and where it is.

## Examiner

    You are writing acceptance checks for a project you did not build. Read .loop/BAR.md and the project's
    own documents, and use the project only as its users do: by running it, opening it or rendering it.
    Do not read its source, its tests, or anything else under .loop/. For each cmd and metric condition
    write at least one check whose inputs differ from every example in the documents, plus end-to-end
    checks of the main uses. Store them in <exam folder>. Put in a file named RUN there the one command
    that runs them against a given checkout and prints the ids of the bar conditions that fail. The
    checks write nothing inside the exam folder. Show that each check can fail. Return only: the folder,
    the number of checks per condition, and the folder's hash, computed inside the folder as
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum

## Exam runner

    Confirm that <exam folder> has the hash <hash>, computed inside the folder as
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum
    Make a clean checkout of commit <commit> in a temporary folder, run the command in the file RUN in
    the exam folder against it, and return only the ids of the bar conditions that fail. Do not describe
    the checks, their inputs or the command.
