# wt

Git worktree switcher for **Zed** + **Claude Code**, driven by `fzf`.

Two shell scripts, no daemon, no state beyond `~/.config/wt/`. No secrets stored.

## Install

```sh
./install.sh
```

- symlinks `~/.local/bin/wt` and `~/.local/bin/wt-zed-shell` → `bin/*`
  (so `git pull` updates the commands)
- creates `~/.config/wt/config` and `~/.config/wt/pending/`
- installs `~/.config/zed/tasks.json` if absent
- points Zed's terminal at `wt-zed-shell` in `~/.config/zed/settings.json`
  (backup kept, diff printed, skipped if a `"terminal"` key already exists)

Dependencies: `git`, `fzf`, `zed`, `claude`. Optional: `jq` (live-session badges).

## Use

Run `wt` from anywhere inside a repo or one of its worktrees — `git worktree
list` reports the whole set from any of them. Outside a repo, `wt` falls back to
`WT_DEFAULT_REPO` from the config, or takes `--repo <path>`.

The picker **loops**: opening Zed, or quitting `claude`, brings you back to the
refreshed list. Only `esc` / `ctrl-c` / `ctrl-q` / `ctrl-y` leave `wt`.

| key | action |
| --- | --- |
| `enter` | Zed **+** `claude -c` inside Zed's terminal panel |
| `ctrl-z` | Zed only |
| `ctrl-o` | `claude` in the current terminal (new session) |
| `ctrl-r` | `claude --resume` in the current terminal |
| `ctrl-e` | Zed **+** `claude` in the current terminal |
| `ctrl-y` | print the path and exit |
| `esc` / `ctrl-c` / `ctrl-q` | quit |

Flags: `-C/--repo`, `-p/--print`, `-l/--list`, `-n/--no-sessions`, `-1/--once`, `-h`.

Each row shows: branch (plus `[main wt]`, `🔒` locked, `⚠` prunable) · session
badge · path. The preview pane shows `git status -sb` and the last 10 commits.

## Claude inside Zed's terminal (what `enter` does)

Zed's CLI has no flag to spawn a terminal or a task — `zed --help` only offers
`-n/-a/-e/-w`, `--diff`, `--dev-container`. So `wt` goes through the shell Zed
starts in its terminal panel:

1. `wt` writes a one-shot flag `~/.config/wt/pending/<sanitized-path>`
   containing a mode (`continue` by default), then runs `zed <path>`.
2. Zed's terminal runs `wt-zed-shell` (set in `~/.config/zed/settings.json`).
3. The wrapper looks for a flag matching its `$PWD`. Found and fresh
   (< 10 min) → it deletes it, runs `claude -c` (falling back to a new session
   when the worktree has no conversation yet), then execs your login shell.
   No flag → it execs your login shell immediately, so every other terminal
   behaves exactly as before.

**Press `ctrl-`` once** in a freshly opened worktree to show the terminal panel.
Zed remembers the panel per project, so the next `enter` on that worktree opens
Zed with Claude already running.

Modes the flag understands: `continue` (default), `resume`, `new`.

### Session badges

- `● N live (name, +k)` — from `claude agents --json --all`, the official
  scriptable listing of running sessions.
- `◌ N resumable` — a plain **count** of the transcript files Claude Code keeps
  under `~/.claude/projects/<slug>`. Claude Code has no non-interactive way to
  list past sessions, so `wt` never parses them: `ctrl-r` hands the job to
  `claude --resume`, which shows the real picker.
- Without `jq`, live badges are skipped and everything else still works.

## Zed tasks

`~/.config/zed/tasks.json` carries two tasks:

- **wt: worktrees (global)** — repo detected from the terminal's cwd
- **wt: worktrees (this project)** — `wt --repo $ZED_WORKTREE_ROOT`

Run them with `cmd-shift-p` → `task: spawn`. They open a centered terminal
(`reveal_target: "center"`), which is where a TUI belongs.

To bind a key, add to `~/.config/zed/keymap.json`:

```json
[
  {
    "context": "Workspace",
    "bindings": {
      "cmd-shift-w": ["task::Spawn", { "task_name": "wt: worktrees (global)" }]
    }
  }
]
```

## Shell helper (optional)

A child process cannot `cd` its parent shell, so to jump into a worktree add to
`~/.zshrc`:

```zsh
# wt: cd into a worktree picked with wt
wtc() { local d; d=$(wt -p "$@") || return; [ -n "$d" ] && cd "$d"; }
```

## Roadmap

- `wt new <branch>` — `git worktree add` + Zed + named claude session
- `wt rm` — guarded removal (never the main worktree, refuse dirty without `--force`)
- session titles via `claude -n <title>`, `wt rename`
- worktree ↔ session links in `~/.config/wt/links.tsv`, so `ctrl-r` resumes a
  known `sessionId` directly
- background sessions via `claude --bg` / `attach`
