# zrush-gui — a desktop window over zrush

Date: 2026-11-08 · Status: proposed

## Intent

zrush today is a terminal UI, one repo per run. Its owner works on several
projects at once and wants:

- **a window of its own in the taskbar**, instead of one terminal among
  others;
- **one tab per project**, to move between repos without relaunching;
- **easier navigation**: resize panes with the mouse, scroll through a
  whole conversation, click rather than learn keys.

The window is for **reading** conversations and **launching** things. Work
with the agent still happens elsewhere: in the IDE, or in a terminal window
the GUI opens.

A throwaway spike (branch `spike/egui-gui`, `crates/zrush-gui`) proved the
shape: egui + egui_dock tabs, the worktree/session tree on the left, the
conversation rendered as Markdown on the right, "open in IDE" and "agent in
a terminal" buttons. The owner's verdict: "a bit ugly, but a good start".

## Goals

1. Several repos open at once, one tab each; the set is remembered between
   launches.
2. Per tab: worktrees and their sessions (live and resumable), as the TUI
   shows them, refreshed in the background without freezing the window.
3. The selected session's conversation, rendered Markdown, scrollable:
   every turn core's transcript reader returns — today the tail of the
   transcript (its last 400 lines), user and agent text only. Reading from
   the first message, tool calls and thinking are out of v1 (owner's
   decision, 2026-11-08).
4. Actions: open a worktree or a session in the IDE (same binding semantics
   as the TUI's enter), start or resume the agent in a new terminal window,
   refresh.
5. Settings: every setting the TUI's settings screen exposes
   (`zrush_core::config::SETTINGS`), editing the same `config.toml`.
6. The taskbar entry says something: live session count in the window title.
7. A look the owner is not embarrassed by (see Look).

## Non-goals

- Replying to the agent from the window, or embedding a terminal.
- Replacing the TUI. Both ship; neither loses features to the other.
- In v1: creating, deleting or purging worktrees and sessions, the fuzzy
  filter. They come once the shared
  decisions live in core (Phase 1), as later increments.

## Anti-goals

- Logic forked between the TUI and the GUI. Anything that decides
  something lives in `zrush-core`; the two crates only draw and route input.
- Breaking the CLI's one-line install, or the update path of CLIs already
  installed.
- The GUI depending on the TUI crate (the spike does, for `ProcessHost`).

## Constraints

- Rust workspace, MSRV 1.99, `unsafe_code = "forbid"`, clippy pedantic with
  `-D warnings`, `unwrap`/`expect` denied outside tests. The GUI crate opts
  into `[lints] workspace = true` like the others.
- License GPL-3.0-only; `cargo deny` runs in CI, so every new dependency's
  license must pass `deny.toml`.
- Windows, macOS and Linux, as the TUI. CI already builds all three.
- egui ecosystem versions must agree: today eframe/egui 0.36,
  egui_dock 0.21, egui_commonmark 0.25 (0.26 wants egui 0.37, which
  egui_dock does not support yet).

## Approaches considered

| | Trade-off |
|---|---|
| **egui (eframe + egui_dock)** — picked | Draggable/splittable tabs and resizable panels out of the box, pure Rust, one binary, no system dependency beyond a GPU driver. Looks like a dev tool unless themed. |
| Tauri (web UI) | Best typography for reading conversations; but the UI is HTML/TS on top of Rust, and Linux needs WebKitGTK installed — breaks the single binary. |
| Slint | Native-ish, one binary, GPL-compatible; but tabs-with-docking and Markdown would be built by hand. |
| Qt (cxx-qt) | Rejected: C++ toolchain and Qt libraries on three OSes for a look Slint already gives. |

The owner picked egui after seeing the spike.

## Design

### Phase 1 — move the shared decisions into core

The TUI crate holds logic the GUI would otherwise copy (audit of
2026-11-08). Move, in this order, keeping the TUI's behaviour and tests
unchanged:

1. **Process host.** `crates/zrush/src/host.rs` (`ProcessHost`: launching
   the editor, the macOS raise, the URL opener, the new-terminal script,
   the inherited-env scrub) moves to `zrush-core` as `host::process`.
   Dedupe the `INHERITED` list copied in `session.rs`.
2. **Background loading.** `crates/zrush/src/probe.rs`: the refresh
   pipeline (worktrees, then status/history over a worker pool, branches,
   live then resumable sessions), the generation counter that drops stale
   results, `spawn_action`, `request_preview` (including the worktree
   preview `git status`/`git log`). Keyboard/resize events stay in the TUI,
   which wraps core events in its own enum. The loader takes a waker
   callback besides its `Sender`, so egui can `request_repaint()`.
   `ui::preview::Preview` (plain data) moves with it.
3. **Snapshot and rows.** `ui/screen.rs` `rebuild` + `mark_misfiled` and
   `apply` (minus the key arm) become a core `RepoSnapshot` that owns
   worktrees, statuses, history, live/resumable sessions and `scanned`,
   applies loader events, and builds `tree::Row`s. `cli.rs` `list` uses it
   too.
4. **Action bodies.** The bodies behind adopt, create, remove, purge,
   run-agent (the session's own agent outranks the listed one) and
   switch-agent in `ui/screen.rs` become `Zrush` methods returning the
   message to show.
5. **Small pieces.** `ui/filter.rs` (nucleo over `Row`, UI-free) and
   `cli.rs` `locate`/`build`, and `table.rs` `shorten` (home as `~`).

6. **What enter means on a row** (builder's call, 2026-11-08): the
   decision part of `ui/app.rs` `enter` — worktree opens unbound, session
   opens bound, misfiled session needs a question, orphan needs a
   question, "More" raises the cap — becomes a core function from the row
   and the snapshot to an intent enum. "More" is a "show more" link in
   the GUI.
7. **Misfiled and orphan sessions, whole** (owner's decision): detection
   (`mark_misfiled`), the question each one raises and its answers (adopt
   the session into this worktree, recreate the deleted worktree when its
   path is known, pick another worktree) and the bodies that carry the
   answer out all live in core. A question is data — a prompt and its
   choices — that each interface renders its own way: modals in the TUI,
   a dialog in the GUI. The GUI v1 therefore offers the same choices as
   the TUI.

Not moved: key handling, the `:` command language, modals, theme, spinner,
`scroll_to`, monospace width helpers, `session.rs` (the editor-panel
shell), the self-update flow. The rest of the intent logic tangled with
modals in `ui/app.rs` (delete gating, purge plan) is **not** part of
Phase 1; it moves when the GUI grows those
actions.

`Zrush::edit_config` re-reads the file before applying a change, so a TUI
and a GUI open together do not overwrite each other. GUI-only overrides
(the conversation read depth) never reach the saved config.

Phase 1 is done when the TUI runs entirely on the moved code, `./check`
is green, and `crates/zrush` no longer contains loading or action logic.
It ships as a CLI release, `v0.0.4`: still a plain `v` tag so installed
CLIs see it, and the release that teaches `update.rs` and the bootstrap
scripts to read `cli-v*` tags (see Distribution). Phase 2 starts after it.

### Release sequence

1. `v0.0.4` — Phase 1 (core move) and the `cli-v` transition.
2. `gui-v0.1.0` — Phase 2, the first GUI release, with the `gui-latest`
   feed.
3. Later CLI releases are tagged `cli-vX.Y.Z`.

### Phase 2 — the GUI

New crate `crates/zrush-gui`, binary `zrush-gui`, depending on
`zrush-core` only.

- **Window**: title `zrush`, plus ` — ● N live` when any tab has live
  sessions. Remembers its size and position (eframe persistence).
- **Top bar**: "add a repo" (path field and a folder picker), settings
  button.
- **Tabs** (egui_dock): one per repo, titled with the main worktree's
  folder name and its live count. Closable, reorderable, splittable. The
  set of open repos is stored in `config.toml` as `gui_repos` (list of
  paths); `default_repo`, or the current directory's repo, opens when the
  list is empty. A repo that no longer exists opens as a tab showing the
  error, not a crash.
- **Left panel** (resizable): worktrees as collapsible headers, an "IDE"
  button on each; sessions under them, newest first, live marked, age
  shown. Data comes from that tab's `RepoSnapshot` via the core loader,
  refreshed on open, on the refresh button, and on a timer.
- **Right panel**: the selected session's conversation as the transcript
  reader returns it (no turn cap for the GUI's read, which never reaches
  the user's config; the 400-line tail stays), re-read when the tab
  refreshes. The reader joins a turn's text parts with a space, which can
  break Markdown; joining with a newline is the builder's call. User and
  agent turns are distinguished, Markdown rendered, scrolled to the
  bottom on open. A selected worktree shows its git status/log preview.
  Header buttons: "Open in IDE" (binds the session, as the TUI's enter)
  and "Agent in a terminal" (new window, resuming that session).
- **Settings**: a window listing `config::SETTINGS` with an editor per
  kind (text, flag, number, choice). Saves through the same path as the
  TUI (`Zrush::edit_config`), and reloads every tab.
- **Errors**: shown in the tab or as a dismissable banner; never a panic.

### Look

The spike reads as "egui default". Target: calm, readable, close to the
TUI's identity.

- **Themes are files** (owner's decision). A theme is a TOML file of
  named colours for the GUI's semantic roles (builder's call on the exact
  list: window and panel backgrounds, text, muted text, borders,
  selection, accent, live, resumable, warning, error, code block
  background, user turn and agent turn). Built-in themes ship embedded in
  the binary, in the spirit of the editors the owner uses: a VS Code-like
  dark and light, a Zed-like dark and light, Solarized dark and light, and a couple
  of other well-known palettes. Users add their own by dropping a file in
  `<config dir>/themes/`; a file with a built-in's name overrides it.
- The setting `gui_theme` names the theme; `auto` (the default) picks the
  default dark or light theme from the OS setting. The settings window
  lists every theme found and applies a change live. A broken theme file
  is reported and skipped; a missing colour falls back to the default
  theme's.
- The TUI keeps its named ANSI colours, which inherit the terminal's own
  palette; themes are for the GUI only.
- A bundled font that has the glyphs zrush uses (●, ◌, tree lines);
  egui's default font renders ● and ◌ as boxes today.
- Conversation as a reading view: max line width, spacing between turns,
  role shown as a subtle label rather than coloured text.
- Status as small coloured dots/pills, not text glyphs.

### Distribution

The CLI and the GUI are **separate artifacts with separate releases**
(owner's decision, 2026-11-08).

- **CLI**: unchanged — the per-target archives, `install.sh` /
  `install.ps1`, `bootstrap.sh` / `bootstrap.ps1`.
- **GUI**, installed like a desktop app:
  - macOS: a `.dmg` holding `zrush.app` (bundle with icon). Signing and
    notarization are out of v1; the README documents the first-launch
    Gatekeeper bypass.
  - Windows: an `.msi` (Start Menu shortcut, icon, uninstall entry).
  - Linux: an AppImage. Low priority for the owner; x86_64 only, and the
    arm64 target is not built for the GUI.

Release streams, modelled on the owner's `homequarters` repo (one product
per tag prefix, `server-v*` / `desktop-v*` there):

- Tags `cli-vX.Y.Z` and `gui-vX.Y.Z`; each product has its own version,
  `CHANGELOG.md` and release workflow (`ci.yml` on `cli-v*`, a new
  `gui.yml` on `gui-v*`). The GUI crate stops using the workspace version.
- A fixed pre-release **`gui-latest`**, never marked latest, holds the
  update feed `latest.json` (version, notes, per-platform package URL and
  `.sha256`). A feed job rewrites it only when the tag is the newest
  `gui-v*`, so rebuilding an old release never offers it as an update
  (homequarters' `desktop/scripts/update-feed.sh`).
- GitHub's "latest release" always stays a CLI release (GUI releases are
  created with `--latest=false`), because installed CLIs and the bootstrap
  scripts ask `/releases/latest`.
- Transition (builder's call): CLIs already installed (≤ v0.0.3) only
  understand `vX.Y.Z` tags. The next CLI release is still tagged `v0.0.4`
  and teaches `update.rs` and the bootstrap scripts to read `cli-v`; CLI
  releases switch to `cli-v*` after that.

Updates (owner's decision): the GUI updates itself the way the CLI does —
`update_check` once a day, an offer, "skip this version", install — but
reads the `gui-latest` feed instead of `/releases/latest`. Installing
means downloading the package for the platform, checking its `.sha256`,
handing over to the platform installer (`msiexec`; mounting the `.dmg`
and replacing the `.app`; replacing the AppImage file), then quitting.

## Left to the builder

- Refresh timer interval, worker-pool size, debounce of repaint wakes.
- Exact module boundaries inside core for the moved code.
- How the GUI shows loading progress.
- Which font, within license constraints.
- Test strategy for the GUI beyond core's tests (egui_kittest snapshot
  tests are an option).
- Packaging tools (cargo-bundle / cargo-packager, WiX, appimagetool) and
  the app icon's design.
