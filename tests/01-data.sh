#!/usr/bin/env bash
# The data layer, with no terminal involved: what ends up in the menu, and
# where each session is filed. This is the cheap half of the suite and it
# covers most of the logic.
set -uo pipefail
# shellcheck source=lib/harness.sh disable=SC1091
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/harness.sh"

echo "  data layer"
sandbox_create data
sandbox_worktree alpha
sandbox_worktree beta
sandbox_session "$REPO"                          aaaa0001-0000-0000-0000-000000000001 "on main"    2026-09-20T10:00:00Z
sandbox_session "$REPO/.claude/worktrees/alpha"  aaaa0002-0000-0000-0000-000000000002 "in alpha"   2026-09-19T10:00:00Z
sandbox_session "$REPO/.claude/worktrees/gone"   aaaa0003-0000-0000-0000-000000000003 "orphan"     2026-09-18T10:00:00Z
sandbox_session /somewhere/else                  aaaa0004-0000-0000-0000-000000000004 "other repo" 2026-09-17T10:00:00Z

ok "three worktrees listed"        "$(rows_of wt)"      3
ok "an orphaned node appears"      "$(rows_of orphans)" 1
ok "the foreign session is absent" "$(menu | grep -c 'other repo' || true)" 0

contains "the orphan row names the worktree it was written for" \
   "$(menu | sed 's/\x1b\[[0-9;]*m//g' | awk -F'\t' '$3=="orphan"{print $1; exit}')" gone

# a name set with -n beats the generated title; an auto name loses to it
ok "session filed under its worktree" \
   "$(menu | awk -F'\t' '$3=="session" && $2 ~ /alpha/{n++} END{print n+0}')" 1

# dates come from the transcript, not the file's mtime
touch -t 210001010000 "$HOME/.claude/projects/$(slug "$REPO")"/aaaa0001*.jsonl
ok "age read from the transcript, not mtime" \
   "$(menu | grep -c '2100' || true)" 0

echo "  purge plan"
d="$SB/state"; mkdir -p "$d"
menu >/dev/null
printf '%s\tid-a\tsess a\tresumable 1d\tresum\t\n' "$REPO/.claude/worktrees/alpha" >  "$d/owned"
printf '%s\tid-b\tsess b\tresumable 2d\tresum\t\n' "$REPO"                        >> "$d/owned"
W_ALPHA="$REPO/.claude/worktrees/alpha"

FZF_PROMPT='purge> ' "$ZRUSH" --state "$d" --repo "$REPO" --ask enter --query '' \
  --rows "l	$W_ALPHA	wt	" "l	$W_ALPHA	session	id-a" >/dev/null 2>&1
ok "a session goes with its marked worktree" "$( [ -s "$d/plan-sess" ] && echo no || echo yes)" yes
ok "the worktree is in the plan"             "$(wc -l < "$d/plan-wts" | tr -d ' ')" 1

FZF_PROMPT='purge> ' "$ZRUSH" --state "$d" --repo "$REPO" --ask enter --query '' \
  --rows "l	$REPO	wt	" >/dev/null 2>&1
ok "the main worktree is never in the plan"  "$(wc -l < "$d/plan-wts" | tr -d ' ')" 0

echo "  confirm state survives empty fields"
"$ZRUSH" --state "$d" --repo "$REPO" --ask delete --path '(orphaned)' --type orphan \
         --id 'id with space' >/dev/null 2>&1
ok "empty n-action does not shift the columns" \
   "$(jq -r '.id' "$d/confirm")" "id with space"
ok "and the action is still readable"        "$(jq -r '.y'  "$d/confirm")" delete-session

sandbox_destroy
report
