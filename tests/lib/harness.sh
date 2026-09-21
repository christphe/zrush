#!/usr/bin/env bash
# Shared by every test: a throwaway repo, an isolated HOME for config, stub
# editors, and helpers to drive the picker.
#
# Nothing here ever touches a real repository. The sandbox lives under
# $BASE/<name>, the Claude project directories it creates are removed by
# sandbox_destroy, and ZRUSH_EDITOR_CMD points at a stub that only logs.

ZRUSH="${ZRUSH:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)/bin/zrush}"
LIB="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# pwd -P: on macOS $TMPDIR is a symlink into /private, and git reports the
# resolved path. Comparing one against the other silently matches nothing.
BASE="$(cd -- "${TMPDIR:-/tmp}" && pwd -P)/zrush-tests.$$"
PASS=0; FAIL=0

slug() { printf '%s' "$1" | sed 's/[^A-Za-z0-9]/-/g'; }

ok() {  # $1 = what, $2 = got, $3 = want
  if [ "$2" = "$3" ]; then
    PASS=$((PASS + 1)); printf '    ok    %s\n' "$1"
  else
    FAIL=$((FAIL + 1)); printf '    FAIL  %s: got [%s], want [%s]\n' "$1" "$2" "$3"
  fi
}

contains() {  # $1 = what, $2 = haystack, $3 = needle
  case "$2" in
    *"$3"*) PASS=$((PASS + 1)); printf '    ok    %s\n' "$1" ;;
    *) FAIL=$((FAIL + 1)); printf '    FAIL  %s: [%s] does not contain [%s]\n' "$1" "$2" "$3" ;;
  esac
}

sandbox_create() {  # $1 = name -> sets SB, REPO, HOMEDIR, LOG
  SB="$BASE/$1"; REPO="$SB/repo"; LOG="$SB/log"
  mkdir -p "$SB/xdg/zrush"
  git init -q --bare "$SB/remote.git"
  git clone -q "$SB/remote.git" "$REPO" 2>/dev/null
  git -C "$REPO" config user.email t@example.invalid
  git -C "$REPO" config user.name  "zrush tests"
  echo seed > "$REPO/f.txt"
  git -C "$REPO" add f.txt
  git -C "$REPO" commit -qm init
  git -C "$REPO" branch -M main
  git -C "$REPO" push -q -u origin main
  # shellcheck disable=SC2016  # "$1" belongs to the stub being written
  printf '#!/bin/sh\nprintf "EDITOR %%s\\n" "$1" >> %s\n' "$LOG" > "$SB/xdg/ed"
  # shellcheck disable=SC2016  # same here
  printf '#!/bin/sh\nprintf "TERMINAL %%s\\n" "$1" >> %s\n' "$LOG" > "$SB/xdg/term"
  chmod +x "$SB/xdg/ed" "$SB/xdg/term"
  {
    printf 'ZRUSH_EDITOR_CMD=("%s")\n' "$SB/xdg/ed"
    printf 'ZRUSH_TERMINAL=("%s")\n'   "$SB/xdg/term"
    printf 'ZRUSH_STATUS=0\n'
  } > "$SB/xdg/zrush/config"
  : > "$LOG"
  export XDG_CONFIG_HOME="$SB/xdg"
}

sandbox_worktree() { git -C "$REPO" worktree add -q ".claude/worktrees/$1" -b "$1"; }

sandbox_session() {  # $1 = worktree path, $2 = id, $3 = title, $4 = iso date
  local d; d="$HOME/.claude/projects/$(slug "$1")"
  mkdir -p "$d"
  printf '{"aiTitle":"%s","cwd":"%s","timestamp":"%s"}\n' "$3" "$1" "$4" > "$d/$2.jsonl"
}

sandbox_destroy() {
  local w
  for w in $(git -C "$REPO" worktree list --porcelain 2>/dev/null \
             | awk '/^worktree /{sub(/^worktree /,"");print}'); do
    rm -rf "${HOME:?}/.claude/projects/$(slug "$w")"
  done
  # worktrees the test removed are gone from the list, so sweep by prefix
  find "${HOME:?}/.claude/projects" -maxdepth 1 -name "$(slug "$REPO")*" \
       -exec rm -rf {} + 2>/dev/null
  rm -rf "$SB"
  unset XDG_CONFIG_HOME
}

menu()    { "$ZRUSH" --menu --repo "$REPO" 2>/dev/null; }
row_at()  { menu | awk -F'\t' -v t="$1" -v m="${2:-.}" '$3==t && ($2 ~ m || $1 ~ m){print NR-1; exit}'; }
rows_of() { menu | awk -F'\t' -v t="$1" '$3==t{n++} END{print n+0}'; }

drive() {  # $1 = json steps array -> prints the raw capture
  python3 "$LIB/drive.py" "$(python3 - "$ZRUSH" "$REPO" "$1" <<'PY'
import json, os, sys
print(json.dumps({"cmd": [sys.argv[1], "--repo", sys.argv[2]],
                  "steps": json.loads(sys.argv[3]),
                  "warmup": float(os.environ.get("WARMUP", 3.5))}))
PY
)" 2>/dev/null
}

down_to() {  # $1 = how many ctrl-n, $2 = json of the steps that follow
  python3 - "$1" "$2" <<'PY'
import json, sys
print(json.dumps([[0.3, "\u000e"]] * int(sys.argv[1]) + json.loads(sys.argv[2])))
PY
}

frame() { rows=32 cols=140 python3 "$LIB/screen.py"; }

report() {
  printf '\n  %s passed, %s failed\n' "$PASS" "$FAIL"
  [ "$FAIL" -eq 0 ]
}
