#!/usr/bin/env bash
# Install zrush: symlinks into ~/.local/bin, a config in ~/.config/zrush, and
# for Zed a task plus a keybinding. Your editor's settings.json is never
# touched — see the note this prints at the end.
#
#   ./install.sh            ask which editor (or keep the configured one)
#   ./install.sh zed        no questions
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zrush"
zed_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed"
zed_tasks="$zed_dir/tasks.json"
zed_keymap="$zed_dir/keymap.json"

# Moving on from the old name: keep the config, drop the dead symlinks.
old_cfg="${XDG_CONFIG_HOME:-$HOME/.config}/wt"
if [ -d "$old_cfg" ] && [ ! -d "$cfg_dir" ]; then
  mv "$old_cfg" "$cfg_dir"
  echo "moved $old_cfg -> $cfg_dir"
fi
for f in wt wt_session wt-zed-shell; do
  if [ -L "$bin_dir/$f" ]; then
    rm -f "$bin_dir/$f"
    echo "removed the old symlink $bin_dir/$f"
  fi
done

mkdir -p "$bin_dir" "$cfg_dir"

# ~/.local/bin/{zrush,zrush_session} -> <repo>/bin/*  (symlinks: `git pull` updates them)
for f in zrush zrush_session; do
  ln -sfn "$here/bin/$f" "$bin_dir/$f"
  echo "linked $bin_dir/$f -> $here/bin/$f"
done

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "warning: $bin_dir is not in your PATH" ;;
esac

if [ ! -f "$cfg_dir/config" ]; then
  cat > "$cfg_dir/config" <<'CFG'
# ~/.config/zrush/config — sourced by zrush (shell syntax). Never put secrets here.

# Repo used when `zrush` runs outside any git repository:
# ZRUSH_DEFAULT_REPO="$HOME/projects/my-repo"

# Editor `enter` opens a worktree in: zed, cursor or code (run with -n).
# ZRUSH_EDITOR=zed
# ZRUSH_EDITOR_CMD=(code --reuse-window)   # or override the whole command line

# Session titles are read from Claude Code transcripts (best effort, cached).
# ZRUSH_TITLES=0              # skip the probe entirely
# ZRUSH_TITLE_WIDTH=48        # truncate titles here

# Per-worktree git status column (~changed ?untracked ^ahead vbehind).
# One `git status` per worktree, 40-250 ms each on a large repo.
# ZRUSH_STATUS=0              # drop the column

# Picker size. Empty (the default) = full screen on the alternate screen.
# ZRUSH_FZF_HEIGHT="80%"      # keep it inline instead
CFG
  echo "created $cfg_dir/config"
fi

# --- which editor ------------------------------------------------------------
editor="${1:-}"
current=$(sed -n 's/^[[:space:]]*ZRUSH_EDITOR=\([a-z]*\).*/\1/p' "$cfg_dir/config" 2>/dev/null | tail -1)

if [ -z "$editor" ]; then
  if [ -t 0 ]; then
  # shellcheck disable=SC2016  # the $-names belong to the printed snippet
    printf 'Which editor should `enter` open a worktree in?\n'
    printf '  1) zed     2) cursor     3) code\n'
    printf 'choice [%s]: ' "${current:-zed}"
    IFS= read -r reply || reply=""
    case "$reply" in
      1|zed)    editor=zed ;;
      2|cursor) editor=cursor ;;
      3|code)   editor=code ;;
      "")       editor="${current:-zed}" ;;
      *)        echo "unknown editor: $reply" >&2; exit 1 ;;
    esac
  else
    editor="${current:-zed}"
  fi
fi
case "$editor" in
  zed|cursor|code) ;;
  *) echo "editor must be zed, cursor or code (got: $editor)" >&2; exit 1 ;;
esac

command -v "$editor" >/dev/null 2>&1 \
  || echo "warning: $editor is not in your PATH (install its shell command)"

# Record it, replacing a previous choice rather than stacking lines.
if grep -q '^[[:space:]]*ZRUSH_EDITOR=' "$cfg_dir/config" 2>/dev/null; then
  tmp=$(mktemp); sed "s|^[[:space:]]*ZRUSH_EDITOR=.*|ZRUSH_EDITOR=$editor|" "$cfg_dir/config" > "$tmp"
  mv "$tmp" "$cfg_dir/config"
else
  printf '\nZRUSH_EDITOR=%s\n' "$editor" >> "$cfg_dir/config"
fi
echo "editor: $editor"

# --- Zed tasks and keybinding ------------------------------------------------
# These files are JSONC: comments and trailing commas are legal, and rewriting
# them through a JSON parser would silently drop the comments. So: copy when
# absent, overwrite when the file is one we wrote before (it carries our
# marker), merge only when it is strict JSON, and otherwise keep quiet and
# print what to add. A backup is kept whenever anything is overwritten.
install_zed_json() {  # $1 = source, $2 = destination, $3 = marker, $4 = kind
  local src="$1" dst="$2" marker="$3" kind="$4" backup

  if [ ! -f "$dst" ]; then
    mkdir -p "$zed_dir"
    cp "$src" "$dst"
    echo "installed Zed $kind -> $dst"
    return 0
  fi

  if grep -q "$marker" "$dst" 2>/dev/null; then
    backup="$dst.bak-$(date +%Y%m%d%H%M%S)"
    cp "$dst" "$backup"
    cp "$src" "$dst"
    echo "replaced Zed $kind -> $dst (backup: $backup)"
    return 0
  fi

  if python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$dst" 2>/dev/null; then
    backup="$dst.bak-$(date +%Y%m%d%H%M%S)"
    cp "$dst" "$backup"
    if python3 - "$src" "$dst" "$kind" <<'MERGE'
import json, sys
src, dst, kind = sys.argv[1], sys.argv[2], sys.argv[3]
import re
mine = json.loads(re.sub(r'^\s*//.*$', '', open(src).read(), flags=re.M))
theirs = json.load(open(dst))
if not isinstance(theirs, list):
    sys.exit(1)
if kind == "tasks":
    have = {e.get("label") for e in theirs if isinstance(e, dict)}
    added = [e for e in mine if e.get("label") not in have]
else:
    added = [e for e in mine if e not in theirs]
if not added:
    print("nothing to add")
    sys.exit(0)
json.dump(theirs + added, open(dst, "w"), indent=2)
open(dst, "a").write("\n")
print("added %d entr%s" % (len(added), "y" if len(added) == 1 else "ies"))
MERGE
    then
      echo "merged Zed $kind -> $dst (backup: $backup)"
    else
      mv -f "$backup" "$dst"
      echo "note: could not merge $dst — add the entries from $src yourself"
    fi
    return 0
  fi

  cat <<MSG
note: $dst has comments, so it is left alone.
      Add the entries from $src yourself.
MSG
}

if [ "$editor" = "zed" ]; then
  install_zed_json "$here/zed/tasks.json"  "$zed_tasks"  "tasks for Zed"  tasks
  install_zed_json "$here/zed/keymap.json" "$zed_keymap" "keymap for Zed" keymap
else
  echo "note: no editor-side task installed for $editor"
  echo "      run \`zrush_session\` in its terminal to pick up the worktree's session"
fi

for c in git fzf zed claude jq; do
  command -v "$c" >/dev/null 2>&1 || echo "warning: missing dependency: $c"
done

cat <<MSG

Done. Editor: $editor.
MSG

if [ "$editor" = "zed" ]; then
  cat <<MSG
In Zed, cmd-shift-j opens a terminal on the worktree and picks up the session
bound to it (task "Claude session"). Nothing starts unless you ask for it.

To have *every* Zed terminal do it instead, add this to $zed_dir/settings.json
yourself — install.sh will not edit that file:

    "terminal": {
      "shell": {
        "with_arguments": { "program": "$bin_dir/zrush_session", "args": [] }
      }
    }
MSG
else
  cat <<MSG
In $editor's terminal, run \`zrush_session\` to pick up the session bound to
that worktree.
MSG
fi
