#!/usr/bin/env bash
# The picker itself, driven through a pty. Slow, so it only covers what the
# data tests cannot see: what the screen shows, and which keys leave fzf.
set -uo pipefail
# shellcheck source=lib/harness.sh disable=SC1091
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/harness.sh"

echo "  folding and reload stay inside fzf"
sandbox_create picker
sandbox_worktree alpha
sandbox_session "$REPO/.claude/worktrees/alpha" bbbb0001-0000-0000-0000-000000000001 "a session" 2026-09-20T10:00:00Z

before=$(menu | wc -l | tr -d ' ')
cap=$(drive "$(down_to "$(row_at wt alpha)" '[[1.5,"\u001b[D"],[1.5,"\u001b[C"],[1.2,"\u001b"]]')")
ok "one alternate screen for fold, unfold and quit" \
   "$(printf '%s' "$cap" | grep -c $'\033\[?1049h' || true)" 1
ok "the list is unchanged afterwards" "$(menu | wc -l | tr -d ' ')" "$before"

echo "  a question replaces the footer and keeps the list"
cap=$(drive "$(down_to "$(row_at wt alpha)" '[[3.0,"\u0004"],[2.0,""]]')")
shot=$(printf '%s' "$cap" | frame)
contains "the question is up"            "$shot" "remove  alpha"
contains "the list is still there"       "$shot" "a session"
ok "the key reminder is gone"            "$(printf '%s' "$shot" | grep -c '\^o claude' || true)" 0

echo "  escape cancels the question without quitting"
cap=$(drive "$(down_to "$(row_at wt alpha)" '[[2.5,"\u0004"],[2.0,"\u001b"],[2.0,""]]')")
shot=$(printf '%s' "$cap" | frame)
contains "the reminder is back" "$shot" "^o claude"
ok "the worktree is still there" "$(git -C "$REPO" worktree list | grep -c alpha || true)" 1

echo "  answering deletes, and says so without wrecking the frame"
cap=$(drive "$(down_to "$(row_at wt alpha)" '[[3.0,"\u0004"],[4.0,"y"],[2.5,""]]')")
shot=$(printf '%s' "$cap" | frame)
ok "the worktree is gone"        "$(git -C "$REPO" worktree list | grep -c alpha || true)" 0
contains "the result is reported" "$shot" "deleted 1 transcript"
ok "one prompt on screen"        "$(printf '%s' "$shot" | grep -c 'worktree>' || true)" 1

sandbox_destroy
report
