# wt

Git worktree **and Claude Code session** switcher for **Zed**, driven by `fzf`.

Two shell scripts, no daemon, no database. The only persistent state is one
line per worktree, inside that worktree's own gitdir. No secrets stored.

## How the pieces fit

`wt` runs in your own terminal and is the cockpit: it lists the worktrees of a
repo and, under each, the Claude Code sessions running there. Picking a row
records a choice and opens Zed.

`wt_session` runs inside Zed's terminal panel. It reads that choice and either
resumes the session or starts a new one.

The choice lives in the worktree's gitdir:

```
<git rev-parse --absolute-git-dir>/wt-session      # session_id=<uuid>
```

That is `repo/.git/worktrees/<name>` for a linked worktree, `repo/.git` for the
main one. Per-worktree by construction, so several Zed windows can each hold
their own session with no global state to collide over. `git worktree remove`
takes the association with it.

## Install

```sh
./install.sh
```

- symlinks `~/.local/bin/wt` and `~/.local/bin/wt_session` → `bin/*`
  (so `git pull` updates the commands)
- creates `~/.config/wt/config`
- installs `~/.config/zed/tasks.json` if absent

Your `~/.config/zed/settings.json` is **not** touched. Wiring `wt_session` into
Zed's terminal is opt-in, see below.

Dependencies: `git`, `fzf`, `zed`, `claude`. Optional: `jq` (session rows and
live badges — without it you still get the worktree list).

## Use

Run `wt` from anywhere inside a repo or one of its worktrees — `git worktree
list` reports the whole set from any of them. Outside a repo, `wt` falls back to
`WT_DEFAULT_REPO` from the config, or takes `--repo <path>`.

The picker **loops**: opening the editor, or quitting `claude`, brings you back
to the refreshed list, cursor still on the row you acted on. Only `esc` /
`ctrl-c` / `ctrl-q` leave `wt`. Folding and `[…more]` never leave fzf at all —
they go through its `reload`, so they redraw instead of restarting the picker.

| key | on a **worktree** row | on a **session** row |
| --- | --- | --- |
| `enter` | drop the association, open the editor → `wt_session` starts a fresh session | bind that session, open the editor → `wt_session` resumes it |
| `ctrl-o` | `claude` in this terminal (new session) | — |
| `ctrl-w` | create a worktree | create a worktree **and bind this session to it** |
| `ctrl-d` | remove the worktree, optionally delete its sessions | delete that one conversation |
| `ctrl-l` | reload the list | |
| `←` / `→` | fold / unfold its rows | `←` folds the parent worktree |
| `esc` / `ctrl-c` / `ctrl-q` | quit | |

`enter` on a `[…more]` row loads more of that worktree's older sessions.
The key reminder sits in a one-line footer at the bottom, so it survives a
narrow window.

The **last row creates a worktree**: `enter` asks for a branch name and runs
`git worktree add` under `<main worktree>/.claude/worktrees/<name>`, beside the
ones `claude --worktree` makes. An existing branch is checked out as-is; a new
one is cut from `origin/HEAD` with `--no-track`, so a later
`git push -u origin HEAD` sets the right upstream instead of pointing at main.
`wt` never touches the network.

Flags: `-C/--repo`, `-p/--print`, `-l/--list`, `-n/--no-sessions`,
`-S/--no-status`, `-1/--once`, `-h`.

The picker takes the **whole terminal** (fzf's alternate screen, so your
scrollback comes back untouched on exit). `WT_FZF_HEIGHT="80%"` keeps it
inline instead.

Worktree rows show branch (plus `[main wt]`, `🔒` locked, `⚠` prunable) · session
badge · git status · path. Session rows show the title, its status, and `← bound`
on the one currently bound. The preview pane shows `git status -sb` and the last
10 commits.

### The git status column

| | |
| --- | --- |
| `~3` | tracked files changed (staged or not, one count per path) |
| `?1` | untracked files |
| `↑2` | commits ahead of the upstream |
| `↓1` | commits behind it |
| `⚠` | the branch has no upstream at all |

A clean, up-to-date worktree shows nothing. It all comes from a single
`git status --porcelain=v2 --branch` per worktree — 40-250 ms each on a large
repo, so `-S` / `WT_STATUS=0` drops the column when that starts to bite. `wt`
reads what git already knows and never fetches, so `↓N` is only as fresh as your
last `git fetch`.

## `wt_session`

In a Zed terminal (`cmd-j` opens the panel):

```sh
wt_session
```

It first clears the `CLAUDE_CODE_*` markers the terminal may have inherited.
An editor launched from inside a Claude session passes its environment to
every terminal it opens, and Claude Code then refuses to save a transcript:

```
Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker
```

A session with no transcript cannot be resumed and shows no title, which is
the whole point of `wt`. This terminal is a top-level session, so the markers
go. `WT_KEEP_CLAUDE_ENV=1` leaves them alone.

It then resolves the worktree, reads `<gitdir>/wt-session`, and runs
`claude --resume <id>` — or plain `claude` when there is no association, the
file is malformed, or the session is gone. Outside a repo, or without `git` /
`claude` / a TTY, it just hands you your login shell. Claude does not replace
the shell, so quitting it leaves the panel alive; set `WT_SESSION_EXEC=1` for a
plain `exec` instead.

To make every Zed terminal do this on its own, add to
`~/.config/zed/settings.json`:

```json
"terminal": {
  "shell": {
    "with_arguments": { "program": "/Users/you/.local/bin/wt_session", "args": [] }
  }
}
```

Be aware of what that implies: every terminal you open in Zed starts a Claude
session. Running `wt_session` by hand is the lighter option.

### Session rows

Each row carries how long ago the session was last touched — `40m`, `6h`, `3d`,
`5mo` — and under a worktree they are sorted **newest first**, running and
resumable interleaved. Age comes from `startedAt` for a running session and
from the transcript's mtime for a resumable one.

**`●` running.** From `claude agents --json --all`, the official scriptable
listing, matched to a worktree by the longest prefix of their `cwd`. That
listing has one entry per *process*, so `wt` groups by `sessionId` first —
otherwise a session with two processes shows up twice. Background sessions
(`claude --bg`) are left out: they are reattached with `claude attach`, not
`--resume`.

**`◌` resumable but not running.** Claude Code has no non-interactive listing
for these, so `wt` reads the transcripts it keeps under
`~/.claude/projects/<slug>/`. Bounded on both ends: the newest
`WT_RESUMABLE_SCAN` transcripts of the repo are examined (40), and at most
`WT_RESUMABLE_MAX` of them are shown per worktree (5). `WT_RESUMABLE_MAX=0`
drops them. Which project dir holds a transcript is not predictable from the
worktree — a `claude --worktree` session is filed under the directory it was
launched from — so `wt` scans the main worktree's slug, everything filed below
it, and each worktree's own slug.

Titles and the recorded `cwd` come from the same transcripts, best effort and
cached under `~/.cache/wt/sessions/`. `WT_TITLES=0` turns the probe off.

A title is written once, early in a conversation, so a long one pushes it out
of the 400-line window `wt` reads — 7 of 125 transcripts here. When the window
comes up empty, `wt` greps the whole file and parses only the last line that
carries a title. A row still falling back to the short id means the transcript
has no title at all, usually because the session has not been used yet.

### What costs what

The picker rebuilds itself after every key, so each rebuild only re-runs the
probes that can have changed:

| after | `claude agents` | `git status` | transcript scan |
| --- | --- | --- | --- |
| `←` / `→` folding | — | cached | — |
| a second `[…more]` | — | cached | — |
| the first `[…more]` | — | cached | yes, uncapped |
| anything else | yes | yes | yes, capped |

Folding therefore costs one `git worktree list` (~10 ms) and a few `awk`, not
a second of probing. Measured on a repo with 4 worktrees and 135 transcripts:
0.67 s to start, 1.4 s for the first full unfold with a warm cache, 3.9 s cold.

Most of that used to be process spawning rather than reading — one `stat`, one
`head` and one `grep` per transcript. The scan now reuses the mtimes `find`
already collected, reads the cache with the shell's own `read`, and matches
ids against one list instead of grepping a file each time.

**Folding.** `←` hides a worktree's session rows, `→` brings them back; from a
session row `←` folds the parent and moves the cursor onto it. The marker in
front of the branch says which it is: `▾` unfolded, `▸` folded, blank when the
worktree has no sessions. Folds last for the run, not across runs. Note that
the arrow keys no longer move the cursor inside the query field.

**Seeing the rest.** A worktree with more resumables than the cap gets a
`[…more]` row at the end of its list. `enter` on it raises that worktree's cap
by `WT_MORE_STEP` (20) and redraws, so `enter` after `enter` keeps unfolding.
Expanding also lifts `WT_RESUMABLE_SCAN` for the rest of the run, since the
rows you are asking for may sit well past the newest 40 transcripts. The
`+N older` count only covers what has been scanned so far — the worktree's
`◌ N resumable` badge is the real total.

**Resuming a dead session somewhere else.** `ctrl-w` on any session row asks for
a branch name, creates the worktree, writes that session id into its
`wt-session`, and opens Zed — so `wt_session` resumes it there. Handy when the
worktree a session ran in is long gone.

Nothing ever resumes from parsed files: `claude --resume <id>` stays the only
way a session is actually restored.

### One session, one process

`claude --resume <id>` on a session that is already running gives you a second
process writing the same transcript. `wt` binds ids, it does not police them —
check the `● live` badge before resuming something that is already up.

Also worth knowing: a session's transcript is filed under the slug of the
directory it was **launched** from, which is not always where it runs — a
`claude --worktree` session lives in the worktree but is filed under its parent.
So `wt_session` looks the id up across every project dir rather than guessing
one. `claude --resume <id>` itself works from any directory.

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

## Editors

`WT_EDITOR` picks what `enter` opens: `zed`, `cursor` or `code`. Each is run as
`<editor> -n <path>`. Anything else is run as-is with the path appended, and
`WT_EDITOR_CMD` replaces the whole command line:

```sh
WT_EDITOR=cursor
WT_EDITOR_CMD=(code --reuse-window)   # or whatever you want
```

`-n` is the default for all three on purpose. **A worktree nested inside
another one never gets its own window without it** — the editor treats it as a
subpath of the parent project and focuses the parent instead. That is the case
for anything under `<repo>/.claude/worktrees/`.

Neither Zed nor the VS Code family offers a CLI way to *focus* the window that
already holds a project, so `enter` on something already open gives you a
second window. Measured on Zed 1.20.2 against its own workspace database:
`cli_default_open_behavior: "new_window"` opens a duplicate,
`"existing_window"` piles every worktree into the focused window's sidebar, and
a `<path>/.` subpath looked like it focused but four calls in a row produced
four windows.

## Removing a worktree

`ctrl-d` removes the worktree with `git worktree remove`. It never touches the
main worktree, and it never forces: if the worktree is dirty, git refuses, `wt`
says so and keeps it.

When sessions belong to that worktree it asks a three-way question, because
"remove the worktree" and "delete the conversations that happened in it" are
not the same decision:

```
<path> has 3 session(s), 1 of them running.
remove the worktree and delete those sessions? [y]es / [n]o, keep them / [c]ancel
```

`yes` also deletes those sessions' transcript files under
`~/.claude/projects/`. Running sessions are skipped with a warning — stop them
first. `no` removes only the worktree. `cancel` does nothing.

On a **session** row, `ctrl-d` deletes that one conversation instead: its
transcript, its sidecar directory, its cached title, and the worktree's
association if it pointed there. The worktree itself is untouched.

## Shell helper (optional)

A child process cannot `cd` its parent shell, so to jump into a worktree add to
`~/.zshrc`:

```zsh
# wt: cd into a worktree picked with wt
wtc() { local d; d=$(wt -p "$@") || return; [ -n "$d" ] && cd "$d"; }
```

## Roadmap

- orphan sessions: transcripts under `~/.claude/projects` whose worktree is
  gone, grouped in their own branch of the tree
- `wt rm` — guarded removal (never the main worktree, refuse dirty without `--force`)
- refuse to bind a session that already has a live process, or fork it with
  `claude --fork-session`
- richer status per worktree: dirty, staged, ahead/behind, unpushed branch
- named sessions via `claude -n <title>`, `wt rename`
