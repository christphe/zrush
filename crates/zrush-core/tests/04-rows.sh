#!/usr/bin/env bash
# Press enter on every kind of row and check something happens.
#
# This file exists because the "+ new worktree" row had silently stopped
# working and nothing noticed: it was the only row enter was never pressed on
# in a test. Every type gets a turn here, so the next one to break says so.
set -uo pipefail
# shellcheck source=lib/harness.sh disable=SC1091
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/harness.sh"

echo "  enter, on each kind of row"
sandbox_create rows
sandbox_worktree alpha
WT="$REPO/.claude/worktrees/alpha"
# two sessions under one worktree, with the cap at one, so a [...more] appears
printf 'ZRUSH_RESUMABLE_MAX=1\n' >> "$SB/xdg/zrush/config"
sandbox_session "$WT"                          eeee0001-0000-0000-0000-000000000001 "newer" 2026-09-20T10:00:00Z
sandbox_session "$WT"                          eeee0002-0000-0000-0000-000000000002 "older" 2026-09-19T10:00:00Z
sandbox_session "$REPO/.claude/worktrees/gone" eeee0003-0000-0000-0000-000000000003 "lost"  2026-09-18T10:00:00Z

for t in wt session orphan orphans more; do
  ok "a $t row exists" "$( [ -n "$(row_at "$t")" ] && echo yes || echo no)" yes
done

gitdir_of() { git -C "$1" rev-parse --absolute-git-dir 2>/dev/null; }

echo "  worktree row: always a fresh session, editor opens"
printf 'session_id=eeee0001-0000-0000-0000-000000000001\n' > "$(gitdir_of "$WT")/zrush-session"
: > "$LOG"
drive "$(down_to "$(row_at wt alpha)" '[[4.0,"\r"],[1.0,"\u001b"]]')" >/dev/null
contains "the editor was asked to open it" "$(cat "$LOG")" "EDITOR $WT"
ok "the binding was dropped" \
   "$( [ -f "$(gitdir_of "$WT")/zrush-session" ] && echo kept || echo dropped)" dropped

echo "  session row: that session is handed over, editor opens"
: > "$LOG"
drive "$(down_to "$(row_at session)" '[[4.0,"\r"],[1.0,"\u001b"]]')" >/dev/null
contains "the editor was asked to open it" "$(cat "$LOG")" "EDITOR $WT"
contains "the session was written down" \
   "$(cat "$(gitdir_of "$WT")/zrush-session" 2>/dev/null)" "session_id=eeee000"

echo "  orphan row: asks for a branch to give it"
shot=$(drive "$(down_to "$(row_at orphan)" '[[3.5,"\r"],[1.5,""]]')" | frame)
contains "the branch picker is up" "$shot" "branch>"
contains "and it says what for"    "$shot" "type a new branch name"

echo "  the orphaned-sessions node: does nothing at all"
before=$(menu | wc -l | tr -d ' ')
shot=$(drive "$(down_to "$(row_at orphans)" '[[3.0,"\r"],[1.5,""]]')" | frame)
contains "the picker is still on its own prompt" "$shot" "worktree>"
ok "the list is unchanged" "$(menu | wc -l | tr -d ' ')" "$before"

echo "  [...more]: loads the rest"
# The row count does not grow: the [...more] row is replaced by the session it
# reveals. So look at what is on screen, not at how many rows there are.
shot=$(drive "$(down_to "$(row_at more)" '[[4.0,"\r"],[2.0,""]]')" | frame)
contains "the older session is now listed" "$shot" "older"
ok "the [...more] row is gone" "$(printf '%s' "$shot" | grep -c 'more\]' || true)" 0

sandbox_destroy
report
