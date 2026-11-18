# Live systems and shared trees

Read this when the target is running in production, is loaded by a running service, or is being changed by other people, sessions or the system's own agents. Each rule below comes from an incident on a live system.

## Find what actually runs

Follow the running process to the code it loads (process, then its configuration, then any link), not the repository the request names. Record every difference between that tree, its branch and the remote. If the remote branch the request names is not what production runs, do not merge or force it: deliver to a branch of the production lineage and ask the owner once which lineage is canonical.

## Freeze it

If the tree can change under you, work from a frozen copy. These commands commit the tree exactly as it stands without touching its working files, its index, HEAD or any branch:

    cd <live tree>
    export GIT_INDEX_FILE="$(mktemp -u)"
    git read-tree HEAD && git add -A
    frozen=$(git commit-tree "$(git write-tree)" -p HEAD -m "as running, $(date -u +%FT%TZ)")
    rm -f "$GIT_INDEX_FILE"; unset GIT_INDEX_FILE
    git tag "loop-frozen-$(date -u +%Y%m%d-%H%M%S)" "$frozen"
    git worktree add --detach <a folder outside the live tree> "$frozen"

Check `git status` in the live tree before and after: it must read the same. Files that git ignores are not in the frozen copy. If production loads any of them, copy them separately and say so. Do the work, and send every helper, to the frozen copy.

## Who else is writing

List the other writers and what each is working on: running tickets, other sessions, the system's own agents. One writer per file at a time: skip a backlog row that another writer owns, and verify their work instead of racing it. Check what changed in the tree before every step.

## Landing a change

- Never restart, signal or reconfigure the running system to land a change. Record how the change takes effect.
- **Find out first what makes the host load code.** If it reloads from the tree it watches (hot reload, a file watcher, a configuration reconcile), files that are landed but not yet loaded are a trap: an unrelated event loads them at a moment nobody chose. In that case deliver to the branch only, keep the landing patch checked against the live tree with `git apply --check`, and land and restart together as one step the owner approves. Each restart needs a fresh yes.
- Otherwise land with a patch checked against the live tree. Hold if another writer touched the file in the last two minutes. Refuse on any context mismatch: sync again, verify again, try again. Syntax-check after landing. If that fails, reverse only your own patch. Then confirm the patch is present.
- Everything that is not yet landed is reviewed (`helpers.md`) before the owner is asked for the landing. On a live system the kit alone does not decide what reaches production.
- **Soak.** After each landing, run a read-only check that prints one JSON object: the process is alive, its heartbeat is current, intake is polling, and real work is moving. Health comes from work actually progressing; a constant "ok" is not a heartbeat. A failed soak means reversing your patch.
- Follow the project's own publishing procedure and security gate.
- For a live system the bar names a soak window of real work (for example a day of normal use), and the change counts as delivered only when the soak has held over it.

## Guard block for any helper sent at a live system

Paste this into the brief and fill in the paths. Add the list of project scripts that start or drive a real system; the helper must not run them, and neither may you.

    READ-ONLY toward the live system: never write, move or delete under <state>, <config> or
    <live tree>; never change a git ref. NEVER start a second instance of the service or run its
    command line against the live profile. Never start, stop, restart or signal a process or unit.
    Never send anything (chat, mail, alerts). Never print secrets: redact tokens in logs and URLs.
    Do not run these scripts, not even with --help: <list>. Analyse large files with small scripts
    that print aggregates. Run heavy commands under nice and ionice with a timeout. Every claim
    cites the command or file and line, and is labelled CONFIRMED, REFUTED, HYPOTHESIS or
    UNVERIFIED. Existing summaries only tell you where to look.
