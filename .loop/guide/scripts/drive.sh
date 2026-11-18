#!/usr/bin/env bash
# Ultra Loop driver for Claude Code.
# Starts a headless agent in a project, then starts it again with a fresh context
# while line 1 of .loop/STATE.md reads "status: setup" or "status: running".
#
# usage: bash drive.sh <project folder> [max runs] [max minutes] [model]
#   defaults: 12 runs, 120 minutes, model "sonnet"
#
# Optional environment:
#   ULTRA_BRIEF        a file with the owner's brief for the first run
#                      (default: ULTRA-BRIEF.md in the project folder, if present)
#   ULTRA_CLAUDE_ARGS  extra flags for the agent's command line, e.g. "--effort high"
#   ULTRA_DENY         deny rules, comma-separated, e.g. 'Bash(git push:*),Bash(sudo:*)'
#                      (default: none; the driver denies nothing on its own)
#
# After each run it runs loop_check.sh. A failed check is given to the next run.
# It stops on status "waiting"; on "done" with a clean check; on a usage limit;
# when a run changes nothing; when the same check failure comes back twice in a
# row; or at a cap. The agent may edit files and run commands in the project
# folder and may write the sealed checks beside it (<project folder>.exam).
# It gets no MCP servers. Logs go to <project folder>/.loop/tmp/driver/. The last
# line of driver.log starts with "stopped:" when the driver finished on its own.
set -u
PROJ="${1:?usage: drive.sh <project folder> [max runs] [max minutes] [model]}"
MAXRUNS="${2:-12}"; MAXMIN="${3:-120}"; MODEL="${4:-sonnet}"
PROJ="$(cd "$PROJ" && pwd)" || exit 2
PARENT="$(dirname "$PROJ")"
SKILL="$(cd "$(dirname "$0")/.." && pwd -P)"
LOG="$PROJ/.loop/tmp/driver"; mkdir -p "$LOG"
BRIEF="${ULTRA_BRIEF:-$PROJ/ULTRA-BRIEF.md}"
EXTRA=(); [ -n "${ULTRA_CLAUDE_ARGS:-}" ] && read -r -a EXTRA <<< "$ULTRA_CLAUDE_ARGS"
DENY=(); if [ -n "${ULTRA_DENY:-}" ]; then IFS=',' read -r -a RULES <<< "$ULTRA_DENY"; DENY=(--disallowedTools "${RULES[@]}"); fi
# The run reads its procedure from .loop/guide/, a copy pinned when the run starts.
if [ ! -d "$PROJ/.loop/guide" ]; then
  mkdir -p "$PROJ/.loop/guide" && cp -r "$SKILL/SKILL.md" "$SKILL/references" "$SKILL/scripts" "$PROJ/.loop/guide/" || exit 2
fi
CHECK="$PROJ/.loop/guide/scripts/loop_check.sh"; [ -f "$CHECK" ] || CHECK="$SKILL/scripts/loop_check.sh"
FIRST="Run the ultra loop on this project: a project run. The procedure is already copied into .loop/guide/ (SKILL.md, references/, scripts/). Read .loop/guide/SKILL.md and .loop/guide/references/project.md first, then follow them."
NEXT="Continue the ultra loop: read .loop/STATE.md, .loop/guide/SKILL.md and .loop/guide/references/project.md."
NOTE="This is an unattended run: it ends the moment your turn ends, so wait for every helper you start."
log() { echo "$*" | tee -a "$LOG/driver.log"; }
mark() { echo "$(git rev-parse HEAD 2>/dev/null)|$( { cksum < .loop/STATE.md; } 2>/dev/null)"; }
cd "$PROJ" || exit 2
T0=$(date +%s); N=0; CHK=""; LASTCHK=""; LASTMARK="$(mark)"
while :; do
  N=$((N+1)); LEFT=$(( MAXMIN*60 - ($(date +%s) - T0) ))
  if [ "$LEFT" -lt 120 ]; then log "stopped: time cap"; break; fi
  if [ -f .loop/STATE.md ]; then MSG="$NEXT"; else MSG="$FIRST"; [ -f "$BRIEF" ] && MSG="$MSG

Owner brief:
$(cat "$BRIEF")"; fi
  [ -n "$CHK" ] && MSG="$MSG

The loop check failed after the last run. Fix these first, then work out the status again from the status rule:
$CHK"
  printf '%s\n\n%s\n' "$MSG" "$NOTE" | timeout "$LEFT" claude -p --model "$MODEL" ${EXTRA[@]+"${EXTRA[@]}"} \
    --permission-mode dontAsk --permission-prompts none \
    --allowedTools "Read,Write,Edit,Glob,Grep,Bash,Agent,Skill,ToolSearch,TodoWrite,NotebookEdit" \
    ${DENY[@]+"${DENY[@]}"} --add-dir "$PARENT" --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
    --output-format json > "$LOG/result-$N.json" 2> "$LOG/stderr-$N.log"
  RC=$?
  ST="$(head -n1 .loop/STATE.md 2>/dev/null || echo 'no state file')"
  CHK=""; CS=skipped
  if [ -f .loop/STATE.md ]; then CS=ok; CHK="$(bash "$CHECK" 2>&1)" && CHK="" || CS=failed; fi
  log "run=$N exit=$RC elapsed=$(( $(date +%s) - T0 ))s state=[$ST] check=[$CS]"
  [ -n "$CHK" ] && echo "$CHK" >> "$LOG/driver.log"
  if python3 -c 'import json,re,sys
d=json.load(open(sys.argv[1]))
sys.exit(0 if d.get("is_error") and re.search("limit", str(d.get("result","")), re.I) else 1)' "$LOG/result-$N.json" 2>/dev/null \
     || { [ "$RC" -ne 0 ] && grep -q "reached your .* limit" "$LOG/result-$N.json" 2>/dev/null; }; then log "stopped: usage limit"; break; fi
  M="$(mark)"
  if [ "$M" = "$LASTMARK" ]; then log "stopped: run $N changed nothing (exit $RC); see $LOG/stderr-$N.log"; break; fi
  LASTMARK="$M"
  if [ "$N" -ge "$MAXRUNS" ]; then log "stopped: run cap"; break; fi
  if [ "$ST" = "status: waiting" ]; then log "stopped: $ST"; break; fi
  if [ -n "$CHK" ] && [ "$CHK" = "$LASTCHK" ]; then log "stopped: the loop check failed twice in a row with the same result"; break; fi
  LASTCHK="$CHK"
  if [ "$ST" = "status: done" ] && [ -z "$CHK" ]; then log "stopped: $ST"; break; fi
done
