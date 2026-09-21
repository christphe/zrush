#!/usr/bin/env bash
# The picker writes eight little scripts into its state directory and lets
# fzf call them back. They are assembled with printf, so a flag renamed in the
# argument parser will not fail to compile -- it will just make every key do
# nothing, silently. That happened once. This runs each of them.
set -uo pipefail
# shellcheck source=lib/harness.sh disable=SC1091
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/harness.sh"

echo "  the generated callbacks"
sandbox_create wrappers
sandbox_worktree alpha
sandbox_session "$REPO/.claude/worktrees/alpha" cccc0001-0000-0000-0000-000000000001 "a session" 2026-09-20T10:00:00Z

# Run the picker once with a state directory we keep, so the scripts survive.
ST="$SB/state"; mkdir -p "$ST"
python3 "$LIB/drive.py" "$(python3 - "$ZRUSH" "$REPO" "$ST" <<'PY'
import json, sys
print(json.dumps({"cmd": [sys.argv[1], "--repo", sys.argv[2], "--state", sys.argv[3]],
                  "steps": [[1.0, "\u001b"]], "warmup": 4.0}))
PY
)" >/dev/null 2>&1

WT="$REPO/.claude/worktrees/alpha"
run_wrapper() { "$ST/$1" "${@:2}" >/dev/null 2>&1; }

for w in ask act 'do' ans show rows foot reload posact; do
  ok "$w exists" "$( [ -x "$ST/$w" ] && echo yes || echo no)" yes
done

run_wrapper ask    delete "$WT" wt ''            ; ok "ask accepts its arguments"    "$?" 0
run_wrapper act    --fold "$REPO"                ; ok "act accepts its arguments"    "$?" 0
run_wrapper 'do'   delete-session '' orphan none ; ok "do accepts its arguments"     "$?" 0
run_wrapper ans    c ''                          ; ok "ans accepts its arguments"    "$?" 0
run_wrapper show   "$REPO" wt ''                 ; ok "show accepts its arguments"   "$?" 0
run_wrapper rows   '' "l	$REPO	wt	"            ; ok "rows accepts its arguments"   "$?" 0
run_wrapper foot                                 ; ok "foot accepts its arguments"   "$?" 0
run_wrapper reload                               ; ok "reload accepts its arguments" "$?" 0

# The failure mode that started this file: an unknown flag is not a crash,
# it is a silent no-op, so check stderr as well as the exit status.
err=$("$ST/ask" delete "$WT" wt '' 2>&1 >/dev/null)
ok "ask says nothing on stderr" "$err" ""
err=$("$ST/rows" '' "l	$REPO	wt	" 2>&1 >/dev/null)
ok "rows says nothing on stderr" "$err" ""

sandbox_destroy
report
