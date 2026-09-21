#!/usr/bin/env bash
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# Build zrush and put it on your PATH, with a config and — for Zed — a task
# and a keybinding. Your editor's settings.json is never touched; the note
# at the end says what to add.
#
#   ./install.sh            ask which editor, or keep the configured one
#   ./install.sh zed        no questions
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zrush"
zed_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed"

command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
}

echo "building (release)…"
cargo build --release --locked --manifest-path "$here/Cargo.toml" -p zrush

mkdir -p "$bin_dir" "$cfg_dir"
install -m 755 "$here/target/release/zrush" "$bin_dir/zrush"
echo "installed $bin_dir/zrush"

# The bash version shipped two commands. `zrush_session` is now a subcommand,
# so a shim keeps existing editor settings working rather than breaking them.
cat > "$bin_dir/zrush_session" <<'SHIM'
#!/bin/sh
# Kept for editor settings that still name it. `zrush session` is the real
# entry point.
exec zrush session "$@"
SHIM
chmod 755 "$bin_dir/zrush_session"
echo "installed $bin_dir/zrush_session (a shim for zrush session)"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "warning: $bin_dir is not in your PATH" ;;
esac

# The old shell-syntax config is imported on first run and left in place, so
# there is nothing to do here for an existing setup.
if [ ! -f "$cfg_dir/config.toml" ] && [ ! -f "$cfg_dir/config" ]; then
  cat > "$cfg_dir/config.toml" <<'CFG'
# ~/.config/zrush/config.toml. Never put secrets here.

# Repo used when zrush runs outside any git repository.
# default_repo = "/Users/you/projects/thing"

# Which coding agent to list. Unset means ask, unless only one is installed.
# agent = "claude"

# Editor that enter opens a worktree in: zed, cursor or code (run with -n).
# editor = "zed"
# editor_cmd = ["code", "--reuse-window"]   # or override the whole command

# How ctrl-o opens a terminal. Empty means `open` on macOS.
# terminal = ["open", "-a", "Ghostty"]

# titles = true          # read session titles from transcripts
# status = true          # the per-worktree git status column
# resumable_max = 5      # dead sessions listed per worktree
# resumable_scan = 40    # newest transcripts examined
# more_step = 20         # rows added by one [...more]
# title_width = 48       # truncate session titles here
# preview_turns = 14     # conversation turns in the preview pane
CFG
  echo "wrote $cfg_dir/config.toml"
fi

editor="${1:-}"
if [ -z "$editor" ] && [ -t 0 ]; then
  printf 'editor [zed/cursor/code, enter to skip]: '
  read -r editor || editor=""
fi

if [ "$editor" = "zed" ]; then
  mkdir -p "$zed_dir"
  for f in tasks.json keymap.json; do
    if [ -f "$zed_dir/$f" ]; then
      echo "note: $zed_dir/$f exists; merge $here/zed/$f yourself"
    else
      cp "$here/zed/$f" "$zed_dir/$f"
      echo "wrote $zed_dir/$f"
    fi
  done
  cat <<'NOTE'

One thing left, in your Zed settings.json, so the terminal panel resumes
this worktree's session:

  "terminal": { "shell": { "with_arguments": {
      "program": "zrush", "args": ["session"] } } }
NOTE
fi

echo
echo "done. Run zrush from inside a git repository."
