#!/usr/bin/env bash
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# Put a zrush binary on your PATH, with a config and — for Zed — a task and
# a keybinding. Nothing is built here: the binary is the one next to this
# script, which is how the release archives are laid out. To build from a
# checkout instead, run ./install-dev.sh.
#
# Your editor's settings.json is never touched; the note at the end says
# what to add.
#
#   ./install.sh                  ask which editor, or keep the configured one
#   ./install.sh zed              no questions
#   ./install.sh --bin path/zrush install that binary
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_dir="$HOME/.local/bin"
cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zrush"
zed_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed"

editor=""
bin=""
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) bin="${2:-}"; shift 2 ;;
    -h|--help) sed -n '6,17p' "$0"; exit 0 ;;
    *) editor="$1"; shift ;;
  esac
done

# Next to this script in a release archive; under target/ in a checkout that
# has already been built.
if [ -z "$bin" ]; then
  for c in "$here/zrush" "$here/target/release/zrush"; do
    if [ -x "$c" ]; then bin="$c"; break; fi
  done
fi
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
  cat >&2 <<'NOBIN'
no zrush binary found next to this script.

  from a checkout:  ./install-dev.sh          (builds it, needs cargo)
  from a release:   https://github.com/christphe/zrush/releases
NOBIN
  exit 1
fi

mkdir -p "$bin_dir" "$cfg_dir"

# A previous install left a symlink here, pointing into a checkout. Writing
# through it edits that checkout instead of installing, so unlink first.
if [ -L "$bin_dir/zrush" ]; then
  echo "removing the old symlink $bin_dir/zrush -> $(readlink "$bin_dir/zrush")"
  rm -f "$bin_dir/zrush"
fi

install -m 755 "$bin" "$bin_dir/zrush"
echo "installed $bin_dir/zrush ($("$bin_dir/zrush" --version 2>/dev/null || echo unknown))"

# The bash version shipped a second command. `zrush session` replaced it, so
# the old one is taken away rather than kept alive as a shim.
if [ -e "$bin_dir/zrush_session" ] || [ -L "$bin_dir/zrush_session" ]; then
  rm -f "$bin_dir/zrush_session"
  echo "removed $bin_dir/zrush_session; the command is zrush session"
fi

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
# Also settable from the interface, with :editor.
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

# Piped from curl there is no one to ask, so the editor step is skipped
# rather than guessed at.
if [ -z "$editor" ] && [ -t 0 ]; then
  printf 'editor [zed/cursor/code, enter to skip]: '
  read -r editor || editor=""
fi

# Asking which editor and then not setting it is what the first version
# did, and it left every install on the default.
if [ -n "$editor" ]; then
  if grep -q '^editor = ' "$cfg_dir/config.toml" 2>/dev/null; then
    tmp="$cfg_dir/config.toml.new"
    sed "s|^editor = .*|editor = \"$editor\"|" "$cfg_dir/config.toml" > "$tmp"
    mv "$tmp" "$cfg_dir/config.toml"
  else
    printf 'editor = "%s"\n' "$editor" >> "$cfg_dir/config.toml"
  fi
  echo "editor = \"$editor\" in $cfg_dir/config.toml"
fi

if [ "$editor" = "zed" ]; then
  if [ ! -d "$here/zed" ]; then
    echo "note: no zed/ next to this script; get tasks.json and keymap.json"
    echo "      from https://github.com/christphe/zrush/tree/main/zed"
  else
    mkdir -p "$zed_dir"
    skipped=""
    for f in tasks.json keymap.json; do
      if [ -f "$zed_dir/$f" ]; then
        echo "note: $zed_dir/$f exists; merge $here/zed/$f yourself"
        skipped="$skipped $f"
      else
        cp "$here/zed/$f" "$zed_dir/$f"
        echo "wrote $zed_dir/$f"
      fi
    done
    # The keybinding names a task by its label, so half an install leaves a
    # key bound to nothing.
    case " $skipped " in
      *" tasks.json "*)
        case " $skipped " in
          *" keymap.json "*) ;;
          *) echo "note: the keybinding just written needs the task in $here/zed/tasks.json" ;;
        esac
        ;;
    esac
  fi
  cat <<'NOTE'

One thing left, in your Zed settings.json, so the terminal panel resumes
this worktree's session:

  "terminal": { "shell": { "with_arguments": {
      "program": "zrush", "args": ["session"] } } }
NOTE
fi

echo
echo "done. Run zrush from inside a git repository."
