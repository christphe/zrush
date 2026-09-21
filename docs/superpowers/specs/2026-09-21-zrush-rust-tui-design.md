# zrush in Rust: a native TUI

Status: approved, ready for implementation
Date: 2026-09-21

## 1. Why

`bin/zrush` is 1653 lines of bash driving fzf. It works, and the
architecture it is forced into has run out of room:

- fzf cannot draw a dialog over its own list, so every question is asked
  by rewriting the footer, rebinding `y`/`n`/`c`, unbinding six other
  keys, and restoring all of it afterwards. `mode_ask` spends 110 lines
  on that alone, and three of the last ten commits fix bugs in it.
- Every keypress that changes the list re-executes the whole script as
  an fzf callback, through six generated `/bin/sh` wrapper scripts that
  communicate through files in a shared temp directory.
- Nothing is concurrent. The picker blocks on `claude agents`, then on
  one `git status` per worktree, then on the transcript scan, before it
  draws anything.

A native TUI removes all three at once. State lives in memory, a modal
is a field plus a `Clear` widget, and probes run on threads while the
list is already on screen.

## 2. Goals

1. Full behavioural parity with the bash implementation (§9).
2. A modal-based UI in the manner of k9s: persistent header, key grid,
   `:` command bar, `/` filter, `?` help, centred dialogs.
3. The list draws immediately; expensive data fills in as it arrives.
4. Portable source, compiled and tested on Linux, macOS and Windows.
5. Checks everywhere: fmt, clippy with warnings denied, unit, snapshot,
   integration and pty tests, licence and advisory auditing, an MSRV
   floor.

## 3. Non-goals

- No feature of the current tool is dropped.
- No feature is added beyond the k9s chrome named in §6.
- No multi-repository view, no user-configurable colour scheme.
- Windows is compiled and unit-tested, and its terminal/editor spawning
  is written, but it is untested on real hardware and documented as
  best-effort.

## 4. Structure

One binary crate, `zrush`. Edition 2024, MSRV 1.85, raised only if a
dependency demands it and pinned in `Cargo.toml` as `rust-version`.

```
src/
  main.rs              entry point, terminal setup and teardown
  cli.rs               clap definitions and subcommand dispatch
  config.rs            TOML config, env overrides, legacy import
  error.rs             error types

  git.rs               git CLI wrapper
  state.rs             <gitdir>/zrush-session

  claude/
    mod.rs
    agents.rs          `claude agents --json --all`
    transcript.rs      .jsonl parsing: aiTitle, cwd, timestamp
    projects.rs        slugify, project directory discovery
    cache.rs           ~/.cache/zrush/sessions

  model/
    mod.rs             Worktree, Session, SessionKind, Row, RowKind
    tree.rs            rows from worktrees + sessions + fold state
    purge.rs           the purge plan

  ui/
    mod.rs
    app.rs             App state, event loop, key dispatch
    theme.rs           the palette
    header.rs          info block, key grid, logo
    table.rs           the main list
    preview.rs         the right pane
    modal.rs           Modal enum and its rendering
    filter.rs          fuzzy matching
    flash.rs           the transient status line

  actions.rs           editor, terminal, deletion, worktree creation
  platform/
    mod.rs             the Platform trait
    unix.rs
    windows.rs
  session.rs           the `session` subcommand (was bin/zrush_session)
```

The boundary that matters: `model/` knows nothing about ratatui or the
terminal. It turns `(Vec<Worktree>, Vec<Session>, FoldState)` into
`Vec<Row>`. Every rule the awk in `assign_sessions` encodes — longest
path prefix wins, dead worktrees become orphans, per-worktree caps,
`[...more]` — lives there, as ordinary functions over ordinary data,
testable without a terminal.

## 5. Data sources

### git

Shelled out to the `git` binary, not linked against libgit2 or gix.
`git worktree add` and `git worktree remove` have semantics worth not
reimplementing, and a C dependency would complicate the build on three
platforms for no gain. One `Command` per call, parsed from
`--porcelain` output:

| Need | Command |
|---|---|
| worktree list | `worktree list --porcelain` |
| status badge | `status --porcelain=v2 --branch` |
| branch list | `for-each-ref --sort=-committerdate --format=%(refname:short) refs/heads` |
| default base | `symbolic-ref --quiet --short refs/remotes/origin/HEAD`, then `origin/main`, `origin/master`, `main`, `master`, then `HEAD` |
| create | `worktree add [--no-track -b <name>] <dest> <base>` |
| remove | `worktree remove <path>` |
| validate a name | `check-ref-format --branch <name>` |

### Claude sessions

Live sessions come from `claude agents --json --all`: entries with
`kind == "interactive"` and a non-empty `sessionId`, grouped by session
id, keeping the newest process per group.

Resumable sessions come from the transcript files under
`~/.claude/projects/<slug>/<uuid>.jsonl`. There is no non-interactive
listing, so the newest `resumable_scan` files are read.

For both, the transcript itself supplies the title and the effective
cwd: the tail is scanned for `aiTitle`, `cwd` and `timestamp`. A title
written early in a long conversation falls out of the tail window, so a
miss falls back to scanning the whole file for the last `aiTitle` line.
A name set with `claude -n` beats the generated title; a name matching
`<basename>-<2 hex>` is Claude's own and loses.

Results are cached in `~/.cache/zrush/sessions/<id>`, keyed first on
size+mtime and then on a hash of the tail, so a touched but unchanged
transcript is not re-parsed. The hash is xxh3, not md5: no dependency
on an external binary.

Nothing here is ever used to resume. `claude --resume <id>` remains the
only mechanism.

## 6. The interface

### Layout

```
 Repo       <name>                      <enter> open        <ctrl-o> new claude    <logo>
 Branch     <branch>                    <ctrl-w> worktree   <ctrl-d> delete
 Worktrees  N  ●N live  ◌N resumable    </> filter          <?> help

┌ Worktrees(N) ──────────────────────┐┌ Preview ────────────────────┐
│ WORKTREE      SESSIONS    GIT       ││                             │
│ ▾ main [root] ◌ 12 resum  ~3 ?1     ││                             │
│   ├─ ● title  running 4m            ││                             │
│▌▾ rust        ● 1 live    ~12 ↑2    ││                             │
└─────────────────────────────────────┘└─────────────────────────────┘
 <esc> back                          <flash message>
```

The tree glyphs, the `●`/`◌` markers, the `~N ?N ↑N ↓N ⚠` status badge
and the `[root]` label are carried over unchanged. Column widths are
computed from content and measured with `unicode-width`, not byte
length: labels routinely carry accents and box-drawing glyphs.

### Keys

| Key | Action |
|---|---|
| `↑` `↓` `k` `j` | move |
| `enter` | worktree: clear the association, open the editor. session: bind it, open the editor. `[...more]`: raise this worktree's cap |
| `ctrl-o` | new `claude` in its own terminal |
| `ctrl-w` | branch modal, the only way to create a worktree; on a session row, hand that session to the new worktree |
| `ctrl-d` | delete modal |
| `ctrl-p` | purge mode |
| `space` | mark a row (purge mode) |
| `←` `→` `h` `l` | fold, unfold |
| `ctrl-l` | reload |
| `/` | filter |
| `:` | command bar |
| `?` | help |
| `esc` | close a modal, leave a mode, or quit |
| `ctrl-c` `ctrl-q` | quit |

### Modals

A modal is a variant of an enum held by `App`, drawn as a centred
`Clear` plus a bordered block over the list. It owns the keyboard while
it is up; there is no rebinding of anything.

- `Confirm { title, body, choices }` — a vertical choice list, not a
  blind y/n. Deletion and purge use it.
- `Input { title, prompt, value, suggestions }` — the branch name, with
  the existing branches listed and filtered underneath.
- `Help` — the full key table.
- `Command` — `:worktrees`, `:sessions`, `:orphans`, `:q`.

### Theme

One `theme.rs` holding named colours. Worktrees cyan, sessions magenta,
orphans red, `[...more]` dim. Live is green,
resumable dim. Everything resolves through `ratatui::style::Color`, so
a 16-colour terminal degrades rather than breaks.

## 7. Concurrency

No async runtime. One `mpsc::Sender<Event>` shared by:

- an input thread blocking on `crossterm::event::read`,
- a pool of worker threads, one job per probe.

The main loop blocks on the receiver, applies the event to `App`, and
redraws. Jobs:

1. `git worktree list` — synchronous, it is the only thing needed to
   draw the first frame.
2. One `git status` per worktree, in parallel.
3. `claude agents --json`.
4. The transcript scan.

Each completion sends a message; the table fills in as they land. A
generation counter on every job discards the results of a reload that
has been superseded.

## 8. Configuration

`~/.config/zrush/config.toml`:

```toml
default_repo = "/path/to/repo"
editor = "zed"                  # zed | cursor | code, or a full command
editor_cmd = ["code", "--reuse-window"]
terminal = ["open", "-a", "Ghostty"]
titles = true
status = true
resumable_max = 5
resumable_scan = 40
more_step = 20
title_width = 48
preview_turns = 14
```

`editor` names one of `zed`, `cursor` or `code`, each run with `-n`;
`editor_cmd`, when present, replaces the whole command line and wins
over `editor`. `ZRUSH_FZF_HEIGHT` has no successor — there is no fzf to
size.

Precedence: environment (`ZRUSH_*`, the names unchanged) over the file
over the defaults. At first run, if `config.toml` is absent and the old
shell-syntax `config` is present, the `ZRUSH_*` assignments it holds
are parsed best-effort and written out as TOML, so an existing setup
survives the upgrade. The old file is left in place.

## 9. Parity

Every behaviour of the bash version, and where it lands:

| Behaviour | Where |
|---|---|
| Worktree list with branch, `[root]`, `🔒`, `⚠` flags | `git.rs`, `model/tree.rs` |
| Session association in `<gitdir>/zrush-session` | `state.rs` |
| A malformed session id is refused on write | `state.rs` |
| Live sessions, grouped by session id | `claude/agents.rs` |
| Resumable sessions from transcripts, capped | `claude/transcript.rs`, `model/tree.rs` |
| Titles, with the whole-file fallback | `claude/transcript.rs` |
| `claude -n` names beating generated titles | `claude/agents.rs` |
| Ages from the last message, not the process start | `claude/transcript.rs` |
| Session attached to the longest matching worktree prefix | `model/tree.rs` |
| Orphans: sessions whose worktree is gone, labelled with it | `model/tree.rs` |
| Per-worktree cap and `[...more]` | `model/tree.rs` |
| Fold and unfold | `ui/app.rs` |
| Git status badge | `git.rs` |
| Preview: conversation tail, or status plus log | `ui/preview.rs` |
| Create a worktree under `<main>/.claude/worktrees/`, from `ctrl-w` only | `actions.rs` |
| An existing branch is checked out, a new one cut `--no-track` | `actions.rs` |
| The main worktree can never be removed | `actions.rs` |
| Delete a session: transcript, sidecar directory, cache entry | `actions.rs` |
| A running session is never deleted | `actions.rs` |
| Deleting a bound session clears the association | `actions.rs` |
| Purge: multi-select, worktrees first, failures do not stop it | `model/purge.rs`, `actions.rs` |
| `--print`, `--list`, `--no-sessions`, `--no-status`, `--once`, `--repo` | `cli.rs` |
| `zrush_session`: resume, `-l` picker, env cleanup, fall back to a shell | `session.rs` |

The internal flags that exist only to serve fzf callbacks — `--menu`,
`--render`, `--state`, `--do`, `--ask`, `--answer`, `--show`, `--fold`,
`--unfold`, `--more`, `--rows`, `--query` — have no successor. They
were the cost of the architecture being removed.

`--sessions <path>` is kept: `zrush session -l` consumes it.

## 10. Platform

```rust
trait Platform {
    fn open_editor(&self, cmd: &[String], path: &Path) -> Result<()>;
    fn spawn_terminal(&self, cfg: &Config, cwd: &Path, args: &[String]) -> Result<()>;
}
```

Unix keeps the current mechanism: write an executable `.command` script
that unsets the `CLAUDE_CODE_*` markers, `cd`s, runs `claude` and
deletes itself, then hand it to the configured opener.

Windows writes the equivalent `.cmd`, and runs it through `wt.exe` when
Windows Terminal is present, falling back to `cmd /c start`. Written,
compiled and unit-tested; untested on real hardware, and the README
says so.

Everything else is already portable, provided no `std::os::unix` API is
used and every path goes through `PathBuf`.

## 11. Testing

| Level | Tool | Covers |
|---|---|---|
| Unit | `cargo test` | slugify, age formatting, porcelain and transcript parsing, prefix assignment, caps, purge plan, config precedence and legacy import |
| Snapshot | `insta` + ratatui `TestBackend` | the full frame for fixed fixtures: table, header, preview, every modal, filter and purge modes |
| Integration | `assert_cmd` + `tempfile` | real throwaway repos: `--list`, `--print`, worktree creation and removal, the state file, refusing the main worktree |
| End to end | `portable-pty` | one smoke test: the binary boots on a pty, takes keys, exits clean |

Integration tests stub the editor and the terminal by pointing
`editor_cmd` and `terminal` at a script that appends to a log, exactly
as `tests/lib/harness.sh` does today. No test touches a real
repository, a real `~/.claude`, or a real editor.

Snapshot tests are the direct replacement for `tests/lib/screen.py` and
the pty-driven picker tests. They are deterministic, they compare a
whole frame rather than a grep of one, and they do not depend on fzf's
timing.

## 12. Checks

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`, with `clippy::pedantic`
  enabled and the noisy lints allowed individually in `Cargo.toml`
- `#![forbid(unsafe_code)]`, `clippy::unwrap_used` and
  `clippy::expect_used` denied outside tests
- `cargo nextest run`
- `cargo deny check` — licences, advisories, bans, sources
- `cargo build --locked` on a pinned MSRV

## 13. CI

Replacing the current shellcheck workflow:

| Job | Runner | Does |
|---|---|---|
| `test` | ubuntu, macos, windows | build, clippy, nextest |
| `lint` | ubuntu | fmt, deny |
| `msrv` | ubuntu | build against the MSRV toolchain |
| `release` | all three | binaries for macOS arm64 and x86_64, Linux gnu and musl, Windows x86_64, uploaded as artifacts |

The pty test is skipped on Windows. The `release` job replaces the
current zip.

## 14. Migration

- `bin/zrush` and `bin/zrush_session` are deleted.
- `tests/` — the bash harness, the python screen driver and the three
  test files — is deleted, replaced by the Rust suites.
- `install.sh` builds with `cargo build --release`, installs the binary
  into `~/.local/bin/zrush`, and writes a `zrush_session` shim that
  execs `zrush session`, so existing Zed `settings.json` files keep
  working.
- The Zed task and keymap files are unchanged.
- The README is rewritten: build instructions, the TOML config, the new
  keys, and a note on Windows.

## 15. Risks

| Risk | Handling |
|---|---|
| A parity bug hides in the awk rules | They are ported as pure functions and tested first, against cases taken from the current behaviour |
| Windows spawning is wrong | Isolated behind the trait, documented as best-effort, compiled and unit-tested in CI |
| Snapshot tests are brittle on width | Fixed `TestBackend` dimensions, and no real repository in any fixture |
| The config format changes under existing users | The legacy importer runs once, and the old file is left alone |
| `claude agents --json` output changes | Parsed leniently with serde defaults; a parse failure degrades to no live sessions rather than an error |
