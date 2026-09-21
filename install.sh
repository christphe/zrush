#!/usr/bin/env bash
# Install wt: symlink into ~/.local/bin, create ~/.config/wt, offer the Zed tasks.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/wt"
zed_tasks="${XDG_CONFIG_HOME:-$HOME/.config}/zed/tasks.json"

mkdir -p "$bin_dir" "$cfg_dir"

# ~/.local/bin/wt -> <repo>/bin/wt  (symlink, so `git pull` updates the command)
ln -sfn "$here/bin/wt" "$bin_dir/wt"
echo "linked $bin_dir/wt -> $here/bin/wt"

if [ ! -f "$cfg_dir/config" ]; then
  cat > "$cfg_dir/config" <<'CFG'
# ~/.config/wt/config — sourced by wt (shell syntax). Never put secrets here.

# Repo used when `wt` runs outside any git repository:
# WT_DEFAULT_REPO="$HOME/projects/my-repo"
CFG
  echo "created $cfg_dir/config"
fi

if [ ! -f "$zed_tasks" ]; then
  mkdir -p "$(dirname "$zed_tasks")"
  cp "$here/zed/tasks.json" "$zed_tasks"
  echo "installed Zed tasks -> $zed_tasks"
else
  echo "note: $zed_tasks already exists — merge the entries from $here/zed/tasks.json yourself"
fi

for c in git fzf zed claude jq; do
  command -v "$c" >/dev/null 2>&1 || echo "warning: missing dependency: $c"
done
