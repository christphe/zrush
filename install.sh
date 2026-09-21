#!/usr/bin/env bash
# Install wt: symlinks into ~/.local/bin, ~/.config/wt, Zed tasks + terminal shell.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/wt"
zed_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed"
zed_tasks="$zed_dir/tasks.json"
zed_settings="$zed_dir/settings.json"

mkdir -p "$bin_dir" "$cfg_dir/pending"

# ~/.local/bin/{wt,wt-zed-shell} -> <repo>/bin/*  (symlinks: `git pull` updates them)
for f in wt wt-zed-shell; do
  ln -sfn "$here/bin/$f" "$bin_dir/$f"
  echo "linked $bin_dir/$f -> $here/bin/$f"
done

if [ ! -f "$cfg_dir/config" ]; then
  cat > "$cfg_dir/config" <<'CFG'
# ~/.config/wt/config — sourced by wt (shell syntax). Never put secrets here.

# Repo used when `wt` runs outside any git repository:
# WT_DEFAULT_REPO="$HOME/projects/my-repo"
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

# --- Zed terminal shell ------------------------------------------------------
# Makes Zed's terminal panel start wt-zed-shell, which runs Claude once when wt
# armed this worktree and otherwise just execs your login shell.
# settings.json is JSONC (comments, trailing commas), so we insert text rather
# than reserialize it, and only when no "terminal" key is there yet.
if [ -f "$zed_settings" ]; then
  if grep -q 'wt-zed-shell' "$zed_settings"; then
    echo "Zed terminal shell already points at wt-zed-shell — unchanged"
  elif grep -q '^[[:space:]]*"terminal"[[:space:]]*:' "$zed_settings"; then
    cat <<MSG
note: $zed_settings already has a "terminal" key — add this inside it yourself:
    "shell": { "with_arguments": { "program": "$bin_dir/wt-zed-shell", "args": [] } }
MSG
  else
    backup="$zed_settings.bak-$(date +%Y%m%d%H%M%S)"
    cp "$zed_settings" "$backup"
    awk -v prog="$bin_dir/wt-zed-shell" '
      !patched && /^\{[[:space:]]*$/ {
        print
        print "  \"terminal\": {"
        print "    \"shell\": {"
        print "      \"with_arguments\": { \"program\": \"" prog "\", \"args\": [] }"
        print "    }"
        print "  },"
        patched = 1
        next
      }
      { print }
    ' "$backup" > "$zed_settings"
    echo "patched $zed_settings (backup: $backup)"
    diff -u "$backup" "$zed_settings" || true
  fi
else
  mkdir -p "$zed_dir"
  cat > "$zed_settings" <<JSON
{
  "terminal": {
    "shell": {
      "with_arguments": { "program": "$bin_dir/wt-zed-shell", "args": [] }
    }
  }
}
JSON
  echo "created $zed_settings"
fi

for c in git fzf zed claude jq; do
  command -v "$c" >/dev/null 2>&1 || echo "warning: missing dependency: $c"
done
