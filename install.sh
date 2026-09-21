#!/usr/bin/env bash
# Install wt: symlinks into ~/.local/bin, a config in ~/.config/wt, Zed tasks.
# Your Zed settings.json is never touched — see the note this prints at the end.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/wt"
zed_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed"
zed_tasks="$zed_dir/tasks.json"

mkdir -p "$bin_dir" "$cfg_dir"

# ~/.local/bin/{wt,wt_session} -> <repo>/bin/*  (symlinks: `git pull` updates them)
for f in wt wt_session; do
  ln -sfn "$here/bin/$f" "$bin_dir/$f"
  echo "linked $bin_dir/$f -> $here/bin/$f"
done

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "warning: $bin_dir is not in your PATH" ;;
esac

if [ ! -f "$cfg_dir/config" ]; then
  cat > "$cfg_dir/config" <<'CFG'
# ~/.config/wt/config — sourced by wt (shell syntax). Never put secrets here.

# Repo used when `wt` runs outside any git repository:
# WT_DEFAULT_REPO="$HOME/projects/my-repo"

# Editor `enter` opens a worktree in: zed, cursor or code (run with -n).
# WT_EDITOR=zed
# WT_EDITOR_CMD=(code --reuse-window)   # or override the whole command line

# Session titles are read from Claude Code transcripts (best effort, cached).
# WT_TITLES=0              # skip the probe entirely
# WT_TITLE_WIDTH=48        # truncate titles here

# Per-worktree git status column (~changed ?untracked ^ahead vbehind).
# One `git status` per worktree, 40-250 ms each on a large repo.
# WT_STATUS=0              # drop the column

# Picker size. Empty (the default) = full screen on the alternate screen.
# WT_FZF_HEIGHT="80%"      # keep it inline instead
CFG
  echo "created $cfg_dir/config"
fi

# --- Zed tasks ---------------------------------------------------------------
if [ ! -f "$zed_tasks" ]; then
  mkdir -p "$zed_dir"
  cp "$here/zed/tasks.json" "$zed_tasks"
  echo "installed Zed tasks -> $zed_tasks"
else
  echo "note: $zed_tasks exists — merge the entries from $here/zed/tasks.json yourself"
fi

for c in git fzf zed claude jq; do
  command -v "$c" >/dev/null 2>&1 || echo "warning: missing dependency: $c"
done

cat <<MSG

Done. In a Zed terminal, run \`wt_session\` to pick up the session \`wt\` bound
to that worktree (cmd-j opens the panel).

To have every Zed terminal do it on its own, add this to $zed_dir/settings.json
yourself — install.sh will not edit that file:

    "terminal": {
      "shell": {
        "with_arguments": { "program": "$bin_dir/wt_session", "args": [] }
      }
    }
MSG
