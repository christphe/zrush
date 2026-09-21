# zrush

Git worktree **and Claude Code session** router for your editor, driven by `fzf`.

Two shell scripts, no daemon, no database. The only persistent state is one
line per worktree, inside that worktree's own gitdir. No secrets stored.

## How the pieces fit

`zrush` runs in your own terminal and is the cockpit: it lists the worktrees of a
repo and, under each, the Claude Code sessions running there. Picking a row
records a choice and opens Zed.

`zrush_session` runs inside Zed's terminal panel. It reads that choice and either
resumes the session or starts a new one.

The choice lives in the worktree's gitdir:

```
<git rev-parse --absolute-git-dir>/zrush-session      # session_id=<uuid>
```

That is `repo/.git/worktrees/<name>` for a linked worktree, `repo/.git` for the
main one. Per-worktree by construction, so several Zed windows can each hold
their own session with no global state to collide over. `git worktree remove`
takes the association with it.

## Install

```sh
./install.sh
```

- symlinks `~/.local/bin/zrush` and `~/.local/bin/zrush_session` → `bin/*`
  (so `git pull` updates the commands)
- creates `~/.config/zrush/config`
- installs `~/.config/zed/tasks.json` if absent

Your `~/.config/zed/settings.json` is **not** touched. Wiring `zrush_session` into
Zed's terminal is opt-in, see below.

Dependencies: `git`, `fzf`, `zed`, `claude`. Optional: `jq` (session rows and
live badges — without it you still get the worktree list).

## Use

Run `zrush` from anywhere inside a repo or one of its worktrees — `git worktree
list` reports the whole set from any of them. Outside a repo, `zrush` falls back to
`ZRUSH_DEFAULT_REPO` from the config, or takes `--repo <path>`.

The picker **loops**: opening the editor, or quitting `claude`, brings you back
to the refreshed list, cursor still on the row you acted on. Only `esc` /
`ctrl-c` / `ctrl-q` leave `zrush`. Folding and `[…more]` never leave fzf at all —
they go through its `reload`, so they redraw instead of restarting the picker.

| key | on a **worktree** row | on a **session** row |
| --- | --- | --- |
| `enter` | **always a new session**: open the editor, `zrush_session` starts a fresh Claude | resume that one: its id is written down, the editor opens, `zrush_session` picks it up |
| `ctrl-o` | a new session in a terminal of its own, association untouched | same, for that session's worktree |
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
`zrush` never touches the network.

Flags: `-C/--repo`, `-p/--print`, `-l/--list`, `-n/--no-sessions`,
`-S/--no-status`, `-1/--once`, `-h`.

The picker takes the **whole terminal** (fzf's alternate screen, so your
scrollback comes back untouched on exit). `ZRUSH_FZF_HEIGHT="80%"` keeps it
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
repo, so `-S` / `ZRUSH_STATUS=0` drops the column when that starts to bite. `zrush`
reads what git already knows and never fetches, so `↓N` is only as fresh as your
last `git fetch`.

## `zrush_session`

In a Zed terminal (`cmd-j` opens the panel):

```sh
zrush_session
```

It first clears the `CLAUDE_CODE_*` markers the terminal may have inherited.
An editor launched from inside a Claude session passes its environment to
every terminal it opens, and Claude Code then refuses to save a transcript:

```
Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker
```

A session with no transcript cannot be resumed and shows no title, which is
the whole point of `zrush`. This terminal is a top-level session, so the markers
go. `ZRUSH_KEEP_CLAUDE_ENV=1` leaves them alone.

It then resolves the worktree, reads `<gitdir>/zrush-session`, and runs
`claude --resume <id>` — or plain `claude` when there is no association, the
file is malformed, or the session is gone. Outside a repo, or without `git` /
`claude` / a TTY, it just hands you your login shell. Claude does not replace
the shell, so quitting it leaves the panel alive; set `ZRUSH_SESSION_EXEC=1` for a
plain `exec` instead.

To make every Zed terminal do this on its own, add to
`~/.config/zed/settings.json`:

```json
"terminal": {
  "shell": {
    "with_arguments": { "program": "/Users/you/.local/bin/zrush_session", "args": [] }
  }
}
```

Be aware of what that implies: every terminal you open in Zed starts a Claude
session. Running `zrush_session` by hand is the lighter option.

### Session rows

Each row carries how long ago the session was last touched — `40m`, `6h`, `3d`,
`5mo` — and under a worktree they are sorted **newest first**, running and
resumable interleaved. Age comes from `startedAt` for a running session and
from the transcript's mtime for a resumable one.

**`●` running.** From `claude agents --json --all`, the official scriptable
listing, matched to a worktree by the longest prefix of their `cwd`. That
listing has one entry per *process*, so `zrush` groups by `sessionId` first —
otherwise a session with two processes shows up twice. Background sessions
(`claude --bg`) are left out: they are reattached with `claude attach`, not
`--resume`.

**`◌` resumable but not running.** Claude Code has no non-interactive listing
for these, so `zrush` reads the transcripts it keeps under
`~/.claude/projects/<slug>/`. Bounded on both ends: the newest
`ZRUSH_RESUMABLE_SCAN` transcripts of the repo are examined (40), and at most
`ZRUSH_RESUMABLE_MAX` of them are shown per worktree (5). `ZRUSH_RESUMABLE_MAX=0`
drops them. Which project dir holds a transcript is not predictable from the
worktree — a `claude --worktree` session is filed under the directory it was
launched from — so `zrush` scans the main worktree's slug, everything filed below
it, and each worktree's own slug.

Titles and the recorded `cwd` come from the same transcripts, best effort and
cached under `~/.cache/zrush/sessions/`. `ZRUSH_TITLES=0` turns the probe off.

A title is written once, early in a conversation, so a long one pushes it out
of the 400-line window `zrush` reads — 7 of 125 transcripts here. When the window
comes up empty, `zrush` greps the whole file and parses only the last line that
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
by `ZRUSH_MORE_STEP` (20) and redraws, so `enter` after `enter` keeps unfolding.
Expanding also lifts `ZRUSH_RESUMABLE_SCAN` for the rest of the run, since the
rows you are asking for may sit well past the newest 40 transcripts. The
`+N older` count only covers what has been scanned so far — the worktree's
`◌ N resumable` badge is the real total.

**Resuming a dead session somewhere else.** `ctrl-w` on any session row asks for
a branch name, creates the worktree, writes that session id into its
`zrush-session`, and opens Zed — so `zrush_session` resumes it there. Handy when the
worktree a session ran in is long gone.

Nothing ever resumes from parsed files: `claude --resume <id>` stays the only
way a session is actually restored.

### One session, one process

`claude --resume <id>` on a session that is already running gives you a second
process writing the same transcript. `zrush` binds ids, it does not police them —
check the `● live` badge before resuming something that is already up.

Also worth knowing: a session's transcript is filed under the slug of the
directory it was **launched** from, which is not always where it runs — a
`claude --worktree` session lives in the worktree but is filed under its parent.
So `zrush_session` looks the id up across every project dir rather than guessing
one. `claude --resume <id>` itself works from any directory.

## Installing

```sh
./install.sh          # asks which editor, or keeps the one already configured
./install.sh cursor   # no questions
```

It symlinks `zrush` and `zrush_session` into `~/.local/bin`, writes
`ZRUSH_EDITOR` into `~/.config/zrush/config`, and for Zed installs a task and
a keybinding.

### Zed

`~/.config/zed/tasks.json` gets three tasks:

| task | what it runs |
| --- | --- |
| **Claude session** | `zrush_session` in a new terminal, at `$ZED_WORKTREE_ROOT` |
| **zrush: worktrees (global)** | `zrush`, repo taken from the terminal's cwd |
| **zrush: worktrees (this project)** | `zrush --repo $ZED_WORKTREE_ROOT` |

`~/.config/zed/keymap.json` binds **`cmd-shift-j`** to *Claude session*: it
opens a terminal on the worktree and picks up the session bound to it. That
is the lighter alternative to wiring `zrush_session` in as Zed's terminal
shell — no session starts unless you ask for one.

Both files are JSONC, so the installer will not reserialize them blindly:

- absent → copied
- written by a previous `./install.sh` (it carries a marker comment) → replaced, backup kept
- strict JSON → the missing entries are merged in, backup kept
- has comments and is not ours → left alone, and you are told what to add

`settings.json` is never touched.

### Cursor and Code

Only `ZRUSH_EDITOR` is set; no editor-side task is installed. Run
`zrush_session` in the editor's terminal to pick up the worktree's session.

## Shell helper (optional)

A child process cannot `cd` its parent shell, so to jump into a worktree add to
`~/.zshrc`:

```zsh
# zrush: cd into a worktree picked with zrush
zc() { local d; d=$(zrush -p "$@") || return; [ -n "$d" ] && cd "$d"; }
```

## Roadmap

- orphan sessions: transcripts under `~/.claude/projects` whose worktree is
  gone, grouped in their own branch of the tree
- `zrush rm` — guarded removal (never the main worktree, refuse dirty without `--force`)
- refuse to bind a session that already has a live process, or fork it with
  `claude --fork-session`
- richer status per worktree: dirty, staged, ahead/behind, unpushed branch
- named sessions via `claude -n <title>`, `zrush rename`
