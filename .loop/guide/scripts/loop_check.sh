#!/usr/bin/env bash
# Ultra Loop check: the rules of a project run that a script can check.
# Run it in the project folder:  bash .loop/guide/scripts/loop_check.sh
# Prints one line per broken rule and exits 1, or prints "loop check: ok" and exits 0.
# Lines that start "loop check note:" are leads for a reviewer; they never fail the check.
# It reads only. It changes nothing. Every failure it prints says how to clear it,
# and nothing here can stop the owner: a ledger line "noted: ..." records his word.
set -u
L=.loop; n=0
bad()  { echo "loop check: $*"; n=$((n+1)); }
note() { echo "loop check note: $*"; }
finish() { [ "$n" -eq 0 ] && echo "loop check: ok"; exit $(( n > 0 )); }
noted() { grep -Eq "^noted:.*$1" "$L/LEDGER.md" 2>/dev/null; }
# Backlog rows as "id<TAB>status": first and last cell of each table row, backticks dropped.
rows() { tr -d '`' | awk -F'|' 'NF>3 && $2 !~ /^[ :-]*$/ { last=($NF ~ /^[ \t\r]*$/)?NF-1:NF; id=$2; st=$last;
  gsub(/^[ \t]+|[ \t\r]+$/,"",id); gsub(/^[ \t]+|[ \t\r]+$/,"",st); print id "\t" tolower(st) }'; }

[ -f "$L/STATE.md" ] || { echo "loop check: $L/STATE.md is missing"; exit 1; }
st="$(head -n1 "$L/STATE.md")"
case "$st" in
  "status: setup"|"status: running"|"status: waiting"|"status: done") ;;
  *) bad "line 1 of STATE.md reads '$st'. It must be 'status: ' followed by setup, running, waiting or done. Work it out from the status rule";;
esac
sed -n 2p "$L/STATE.md" | grep -q '^next: ' || bad "line 2 of STATE.md must start with 'next: '"
[ "$st" = "status: setup" ] && finish
# A run may prefix its two tags (a line "tags: <prefix>" in STATE.md) when the repository holds another run.
TP="$(sed -n 's/^tags: *//p' "$L/STATE.md" | head -n1)"; TS="${TP}loop-start"; TB="${TP}loop-baseline"

for f in BAR.md BACKLOG.md LEDGER.md verify; do
  [ -e "$L/$f" ] || bad "$L/$f is missing"
done

if git rev-parse --git-dir >/dev/null 2>&1; then
  if git rev-parse -q --verify "refs/tags/$TS" >/dev/null 2>&1; then
    # The bar is append-only, unless the owner said otherwise.
    r="$(git diff --numstat --ignore-space-at-eol "$TS" -- "$L/BAR.md" | awk '{print $2+0}')"
    [ "${r:-0}" -eq 0 ] || noted bar || bad "$r line(s) of the bar were edited or removed since loop-start. The bar is append-only: restore them (git show loop-start:$L/BAR.md) and tighten with a new line. If the owner told you to change the bar, add the ledger line 'noted: bar <what he said>'"
    # The ledger is append-only.
    r="$(git diff --numstat --ignore-space-at-eol "$TS" -- "$L/LEDGER.md" | awk '{print $2+0}')"
    [ "${r:-0}" -eq 0 ] || noted ledger || bad "$r line(s) of the ledger were edited or removed since loop-start. The ledger is only appended to: restore them from git. If the owner told you to correct or remove a line, add the ledger line 'noted: ledger <what he said>'"
    # Backlog rows are never deleted.
    if [ -f "$L/BACKLOG.md" ]; then
      now="$(rows < "$L/BACKLOG.md" | cut -f1)"
      for id in $(git show "$TS:$L/BACKLOG.md" 2>/dev/null | rows | cut -f1); do
        echo "$now" | grep -qxF "$id" || noted "backlog $id( |\$)" || bad "backlog row $id was deleted. Rows are never deleted: restore it and give it a status. If the owner told you to drop it, add the ledger line 'noted: backlog $id <what he said>'"
      done
    fi
    # One row per commit: a fix names its row, bookkeeping says 'loop: '.
    # A commit that is already made is never rewritten: it is accepted once the
    # ledger holds a line 'noted: <hash> <what it covers>'.
    k=0
    while read -r c s; do
      [ -n "$c" ] || continue
      case "$s" in "loop: "*|"Revert "*) continue;; esac
      id="$(printf '%s' "$s" | sed -nE 's/^([A-Za-z]+-?[0-9]+[a-z]?): .*/\1/p')"
      if [ -n "$id" ] && { rows < "$L/BACKLOG.md" 2>/dev/null | cut -f1 | grep -qxF "$id" || grep -qw "$id" "$L/BAR.md" 2>/dev/null; }; then continue; fi
      noted "$c" && continue
      k=$((k+1)); [ "$k" -gt 5 ] && { n=$((n+1)); continue; }   # print the first five only
      bad "commit $c ('$s') does not start with one row id that is in the backlog or the bar. Start a fix's message with its row id (B7: ...) and bookkeeping with 'loop: '. Do not rewrite this commit: add the ledger line 'noted: $c <the rows it covers>'"
    done < <(git log --format='%h %s' "$TS"..HEAD)
    # The kit (.loop/verify and .loop/checks/) changes only in a bookkeeping commit.
    for c in $(git log --format=%h "$TS"..HEAD -- "$L/verify" "$L/checks"); do
      noted "$c" && continue
      git show --name-only --format= "$c" | grep -v '^$' | grep -qv '^\.loop/' \
        && bad "commit $c changes the kit together with project files. The kit changes only in a bookkeeping commit. Do not rewrite this commit: add the ledger line 'noted: $c kit changed with project files'"
    done
  else
    bad "the tag $TS is missing. Set-up ends by tagging the commit that holds .loop/ (git tag $TS)"
  fi
  # Uncommitted project files. The owner's brief file is not the loop's to commit,
  # and files the ledger notes as the owner's own edits are left alone.
  unc="$(git status --porcelain -- . ':(exclude).loop' ':(exclude)ULTRA-BRIEF.md' 2>/dev/null | cut -c4- | while IFS= read -r f; do noted "owner-edits.*$f" || printf '%s ' "$f"; done)"
  [ -z "$unc" ] || bad "the project has uncommitted changes outside .loop/: $unc. Commit the ones you made. Never revert a change you did not make: it is the owner's. Leave it and add the ledger line 'noted: owner-edits <the files>'"
  # Leads for a reviewer. These never fail the check.
  if git rev-parse -q --verify "refs/tags/$TB" >/dev/null 2>&1; then
    added="$(git diff "$TB" HEAD -- . ':(exclude).loop' 2>/dev/null | grep '^+[^+]' || true)"
    printf '%s\n' "$added" | grep -Eq 'AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,}|sk-[A-Za-z0-9_-]{24,}|xox[baprs]-[A-Za-z0-9-]{10,}' \
      && note "a line added since loop-baseline looks like a secret (a key or a token)"
    printf '%s\n' "$added" | grep -Eq '@(unittest\.)?skip|pytest\.mark\.(skip|xfail)|\.skip\(|(^|[^a-z])xit\(|# *noqa|# *type: *ignore|eslint-disable|@ts-ignore|\|\| *true' \
      && note "a line added since loop-baseline skips or silences a check (skip, xfail, noqa, eslint-disable, '|| true')"
    del="$(git diff --diff-filter=D --name-only "$TB" HEAD 2>/dev/null | grep -Ei '(^|/)(tests?|spec|__tests__)/|(^|/)test_[^/]*$|[._](test|spec)\.[a-z]+$' | head -3 | tr '\n' ' ')"
    [ -n "$del" ] && note "test files were deleted since loop-baseline: $del"
  fi
fi

if [ "$st" = "status: done" ]; then
  [ -s "$L/REPORT.md" ] || bad "the status is done but $L/REPORT.md is missing. Write the report there"
  if [ -f "$L/BACKLOG.md" ]; then
    rows < "$L/BACKLOG.md" | cut -f2 | grep -qx 'open' && bad "the status is done but the backlog still has open rows. The status rule gives 'status: running'"
    rows < "$L/BACKLOG.md" | cut -f2 | grep -qx 'waiting' && bad "the status is done but a backlog row is waiting on the owner. The status rule gives 'status: waiting'"
  fi
fi
finish
