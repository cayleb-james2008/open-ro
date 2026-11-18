# What this host can do, and unattended runs

Find out by trying, in set-up, what you are about to use, and record a label for every row, also when the capability is there (`review=helper`, `render=script`, `network=yes`). A helper or browser call that fails twice in a later run changes the label for that run. Every report carries the labels.

| Capability | Without it | Label |
|---|---|---|
| A shell that returns exit codes | Do no work. Tell the owner this loop needs a host that runs commands. | none |
| Version control | A saved copy of the files you will change, in `.loop/copies/<n>/`, stands in for a commit. "Commit" means saving one, "revert" means copying it back, `loop-baseline` is copy 0, `loop-start` is a copy of `.loop/BAR.md`, and a "clean checkout" is a copy of the tree. The loop check skips what it needs git for. | `commits=copies` |
| A helper with its own fresh context | Do reviews and hunts as separate passes that re-read only their inputs, and call the results self-checked. Skip sealed checks and judges. A `done` reached this way is reported as "done, self-checked". | `review=self` |
| A place the maker cannot read | `seal=enforced` when a sandbox or a separate account denies you the read. Otherwise the checks live in `<project folder>.exam` beside the project (or `.loop/exam/` if the parent is not writable), unopened by you and hash-checked before each use. With no helper there are no sealed checks. | `seal=enforced`, `seal=convention` or `seal=none` |
| A scripted browser | A script may use any browser automation already on the host: under `.loop/checks/` when it is a check to keep, under `.loop/tmp/` when it is a one-off. In the Claude desktop app the built-in browser pane also serves: preview a local server or file, check at desktop and phone width, read the console. Read a published Artifact back before calling it delivered. With none, a fresh helper looks at the page and the result is reported as observed, not as passed by a command. With no way to render at all, no rendered condition is marked met. | `render=script`, `render=observed` or `render=none` |
| Network | Link, install and fetch checks are "not run". | `network=none` |
| A named skill | The rules in this guide that stand in for it. A skill is installed if the host lists it or its folder exists. | named in the report |

Wait for every helper inside the same run: when your turn ends in an unattended run, a helper left in the background is lost. Use an isolated headless browser with a temporary profile, never the owner's.

## Running unattended

A driver starts the agent in the project folder, then starts it again with the one-line continue message while line 1 of `.loop/STATE.md` reads `status: setup` or `status: running`. It stops on `waiting` or `done`, on a usage-limit message, or at its own cap of runs or minutes.

This skill ships one for Claude Code:

    bash <skill folder>/scripts/drive.sh <project folder> [max runs] [max minutes] [model]

The owner's brief for the first run can be put in a file named `ULTRA-BRIEF.md` in the project folder, or in any file named by the environment variable `ULTRA_BRIEF`; the driver appends it to the first message. `ULTRA_CLAUDE_ARGS` adds flags to the agent's command line (for example `--effort high`). The driver copies the skill into `.loop/guide/` when a run starts and that copy stays for the life of the run; to move a run to a newer version of the skill, delete `.loop/guide/` and start the driver again. The driver denies nothing by default. To lock a run down, for example one beside a live system, list deny rules in `ULTRA_DENY`, separated by commas: `ULTRA_DENY='Bash(git push:*),Bash(sudo:*),Bash(systemctl:*)'`. After each run the driver runs `scripts/loop_check.sh`, which checks the rules a script can check (a valid status, an append-only bar, one row per commit, a report before `done`). If the check fails, the next run is told what failed; a run that ends `done` with a failing check is started again, and the same failure twice in a row stops the driver. Its log is `.loop/tmp/driver/driver.log`: a driver that finished wrote a last line starting `stopped:`, and a log without one, with no agent process left, means the driver died.

Start a long run outside the session that launches it. A job started from an agent session stays in that app's process group even with `setsid` or `nohup`: when the app is restarted, or killed for memory, the job dies with it. On a machine with systemd, give the run its own scope and a memory limit:

    systemd-run --user --scope --unit=ultra-loop-<name> -p MemoryMax=7G -p MemorySwapMax=1G -- \
      setsid nohup bash <skill folder>/scripts/drive.sh <project folder> > drive.out 2>&1 &

Run one at a time on a busy machine. A run that is killed anyway loses only the work since its last commit: start the driver again and it continues from `.loop/STATE.md`.

Limits that must hold belong in the host (a sandbox, deny rules, hooks) where it supports them: a rule in a prompt can be lost after a context reset, a rule in the host cannot.
