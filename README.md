# zrush

[![CI](https://github.com/christphe/zrush/actions/workflows/ci.yml/badge.svg)](https://github.com/christphe/zrush/actions/workflows/ci.yml)

Git worktree **and coding-agent session** router for your editor. A terminal
interface, in the manner of k9s.

One binary, no daemon, no database. The only persistent state is one file per
worktree, inside that worktree's own gitdir. No secrets stored.

```
 Repo       wt                            <enter> open       <ctrl-o> new session   ╱╴ ╭─╮ ╭─╮ ╷ ╷ ╭─╴ ╷ ╷
 Branch     main                          <ctrl-w> worktree  <ctrl-d> delete        ╱  ╭╯  ├┬╯ │ │ ╰─╮ ├─┤
 Agent      claude                        </> filter         <?> help               ╱  ╰─╴ ╵╰╴ ╰─╯ ╶─╯ ╵ ╵
 Worktrees  2  ● 2 live  ◌ 17 resumable

╭ Worktrees(2) ──────────────────────────────────────╮╭ Preview ──────────────────────────────────╮
│ ▾ main [root]          ◌ 12 resumable  ~3 ?1       ││ you  port it to rust                      │
│   ├─ ● refonte picker  busy 4m                     ││                                           │
│   └─ ◌ fix CI zip      resumable 2d                ││   ·  Compris. Je tranche les trois points  │
│▌▾ rust                 ● 1 live        ~12 ↑2      ││      moi-même.                            │
│   └─ ● port ratatui    busy 0m                     ││                                           │
╰────────────────────────────────────────────────────╯╰───────────────────────────────────────────╯
 <esc> quit                                            worktree 'rust' bound to f3a91c2e
```

## How the pieces fit

`zrush` runs in your own terminal and is the cockpit: it lists the worktrees of
a repo and, under each, the agent sessions that belong there. Picking a row
records a choice and opens your editor.

`zrush session` runs inside the editor's terminal panel. It reads that choice
and either resumes the conversation or starts a new one.

The choice lives in the worktree's gitdir:

```
<git rev-parse --absolute-git-dir>/zrush-session
    session_id=<id>
    agent=claude
```

That is `repo/.git/worktrees/<name>` for a linked worktree, `repo/.git` for the
main one. Per-worktree by construction, so several editor windows can each hold
their own session with no global state to collide over. `git worktree remove`
takes the association with it.

## Install

Needs Rust ([rustup.rs](https://rustup.rs)) and `git`.

```sh
./install.sh          # asks which editor, or keeps the one already configured
./install.sh zed      # no questions
```

It builds in release mode, installs the binary into `~/.local/bin`, writes
`~/.config/zrush/config.toml` if there is none, and for Zed installs a task and
a keybinding. It also leaves a `zrush_session` shim, so editor settings written
for the previous version keep working.

### Zed

One thing to add yourself, in `settings.json`, so the terminal panel resumes
this worktree's session:

```json
"terminal": { "shell": { "with_arguments": {
    "program": "zrush", "args": ["session"] } } }
```

### Cursor and Code

Set `editor` in the config and use the built-in terminal profile of your
choice, pointing it at `zrush session`.

## Use

Run `zrush` from anywhere inside a repository.

| Key | What it does |
|---|---|
| `↑` `↓` / `k` `j` | move |
| `←` `→` / `h` `l` | fold, unfold a worktree's rows |
| `enter` | **worktree**: drop the association and open the editor, so the panel starts a fresh session. **session**: bind it and open, so the panel resumes it. **`[…more]`**: load older sessions |
| `ctrl-o` | start a session in a terminal of its own, leaving the association and the editor alone |
| `ctrl-w` | create a worktree. On a session row, hand that session to the new worktree |
| `ctrl-d` | delete a conversation, or remove a worktree |
| `ctrl-p` | purge: mark rows with `space`, `enter` removes the lot |
| `ctrl-l` | reload |
| `/` | filter, fuzzy by default. `'sub` `=exact` `^start` `end$` `!not`, as in fzf. A node and its children travel together, so the list stays a tree |
| `:` | command bar — `:agent`, `:help`, `:q` |
| `?` | every key, in full |
| `esc` | close a dialog, leave a mode, or quit |

### Command line

```
zrush [OPTIONS] [COMMAND]

  -C, --repo <PATH>   use this repo instead of working it out from $PWD
  -a, --agent <ID>    which agent to list; outranks the configured default
  -l, --list          print the rows and exit, drawing nothing
  -n, --no-sessions   skip the session lookup entirely (faster)
  -S, --no-status     skip the git status column (faster on huge repos)

  zrush session       resume this worktree's session, or start a new one
  zrush sessions <p>  one line per session of a worktree
```

## What the rows say

```
▾ main [root]          ◌ 12 resumable  ~3 ?1 ↑2 ↓1
  ├─ ● refonte picker  busy 4m
  └─ ◌ fix CI zip      resumable 2d
```

- `▾` `▸` — unfolded, folded. Blank when a worktree has nothing under it.
- `[root]` — the primary worktree, the one holding `.git`. git refuses to
  remove it, and so does zrush.
- `🔒` `⚠` — locked, prunable, as `git worktree list` reports them.
- `●` — a conversation running now. `◌` — one that is over but resumable.
- `~3` changed tracked files, `?1` untracked, `↑2 ↓1` against the upstream,
  `⚠` no upstream at all.

The session badge is the cheap count while the transcript scan is still
running — one `readdir`, available immediately — and becomes the number of
sessions actually attached once the scan lands. So the badge and the rows
cannot end up contradicting each other.

### Where the numbers come from

Running sessions come from the agent's own listing. Past ones are found by
reading the transcripts the agent keeps, which is best effort and bounded:
only the newest `resumable_scan` files are examined, and at most
`resumable_max` land under any one worktree. `[…more]` raises that cap.

Nothing here is ever used to resume: the agent's own resume command stays the
only mechanism.

### Filtering

`/` matches the branch or the session title — not the path, and not the tree
glyphs. Fuzzy by default, with fzf's vocabulary for the rest:

| Typed | Matches |
|---|---|
| `rust` | fuzzy |
| `'rust` | contains |
| `=rust` | is exactly |
| `^rust` | starts with |
| `rust$` | ends with |
| `!rust` | does not contain |

Fuzzy is loose on short needles: `rust` finds `r`, `u`, `s` and `t` in order
inside `z-rush agent orchestrator`. The characters that matched are marked, so
a surprising row explains itself, and `'` is the way out.

A node and its children travel together: filtering to a worktree keeps its
sessions, and a matching session keeps the worktree that says where it lives.
A row with no marks is one that came along with a neighbour.

## Agents

zrush lists one agent at a time, the way k9s looks at one context. Listing
several at once would cost one CLI call and one transcript scan per agent on
every refresh, and produce rows nobody can tell apart.

Which one it starts on:

1. `--agent <id>` on the command line. A name that does not exist is an error.
2. `agent = "…"` in the config. If that agent is not installed *here* — a
   config travels between machines — zrush asks instead, and says why.
3. The only agent installed, with no question asked.
4. Otherwise it asks.

`:agent` switches at any time. The agent is recorded **in the binding**, so a
worktree bound under one agent still resumes with that agent even while you
are looking at another.

Claude Code is the agent this version ships with. Adding another means one
file implementing `zrush_core::agent::Agent` and one line in `agent::all()`.

## Configuration

`~/.config/zrush/config.toml`. Every value can be overridden by the matching
`ZRUSH_*` environment variable.

```toml
default_repo = "/Users/you/projects/thing"  # used outside any repository
agent = "claude"                            # unset means ask
editor = "zed"                              # zed | cursor | code, run with -n
editor_cmd = ["code", "--reuse-window"]     # or override the whole command
terminal = ["open", "-a", "Ghostty"]        # how ctrl-o opens a terminal
titles = true                               # read titles from transcripts
status = true                               # the git status column
resumable_max = 5                           # dead sessions per worktree
resumable_scan = 40                         # newest transcripts examined
more_step = 20                              # rows added by one [...more]
title_width = 48                            # truncate session titles here
preview_turns = 14                          # turns shown in the preview
theme = "auto"                              # auto | dark | light
```

The shell-syntax `config` the previous version sourced is imported once, on
first run, and left in place.

### Colours

zrush never paints its own background. k9s does, so that it looks the same
everywhere; here the terminal's own theme shows through, which is how the
version this replaced looked at home on whatever you had set. Named ANSI
colours are used throughout, so a terminal theme has already tuned them.

Two choices do depend on which way the background goes — the muted text used
for labels and paths, and the text on the cursor bar. `theme` picks; `auto`
reads `COLORFGBG` and assumes dark when the terminal says nothing, which most
do.

Cache: `~/.cache/zrush/sessions/`. Derived data only, safe to delete.

## Layout

```
crates/
  zrush-core/   the domain and its two ports. No terminal, no CLI.
  zrush/        the terminal interface.
```

`zrush-core` cannot draw: it does not depend on ratatui, crossterm or clap.
Two ports face outwards — `Agent`, for how a coding agent's sessions are
discovered and resumed, and `Host`, for where a worktree gets shown and where
an agent runs. The terminal implements `Host` with processes; another front
end implements it differently and never spawns an editor.

## Development

```sh
./check        # fmt, clippy with warnings denied, the whole test suite
./check -f     # fix the formatting instead of complaining about it
```

CI runs the same three on Linux, macOS and Windows, plus `cargo deny` and a
build against the MSRV, and produces a binary per platform.

## Windows

Compiled and unit-tested in CI, but untested on real hardware: opening an
editor and starting an agent in a new terminal window go through `wt.exe`,
falling back to `cmd /c start`. Treat it as best effort.

## Shell helper (optional)

`--list` prints tab-separated fields: label, session badge, git badge, path.
Only worktree rows carry a path.

```sh
# cd into one of this repo's worktrees, picked with fzf
zcd() {
  local p
  p=$(zrush --list | awk -F'\t' '$4 != "" {print $4}' | fzf) || return
  cd "$p" || return
}
```

## License

GPL-3.0-only. See [LICENSE](LICENSE).
