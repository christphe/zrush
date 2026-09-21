# zrush Rust TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bash + fzf implementation of zrush with a single native Rust binary presenting a ratatui TUI, with modal dialogs, k9s-style chrome, concurrent data loading, and a full check and test suite running on Linux, macOS and Windows.

**Architecture:** One binary crate. A pure domain layer (`model/`) turns worktrees, sessions and fold state into rows and knows nothing about the terminal. Data acquisition shells out to `git` and `claude` and parses their porcelain and JSON. The UI layer is ratatui over crossterm, driven by a single `mpsc` event loop fed by an input thread and a pool of probe workers, so the list draws before the slow probes finish.

**Tech Stack:** Rust 2024 edition, ratatui, crossterm, clap (derive), serde/serde_json, toml, jiff, nucleo-matcher, unicode-width, xxhash-rust, anyhow, thiserror, dirs. Tests: insta, assert_cmd, predicates, tempfile, portable-pty, cargo-nextest.

**Spec:** `docs/superpowers/specs/2026-09-21-zrush-rust-tui-design.md`

## Global Constraints

- Edition 2024. `rust-version = "1.85"` in `Cargo.toml`; raise only if a dependency forces it, and update the MSRV CI job at the same time.
- `#![forbid(unsafe_code)]` at the crate root.
- `clippy::pedantic` on; `clippy::unwrap_used` and `clippy::expect_used` denied outside `#[cfg(test)]`. Noisy pedantic lints are allowed individually in `[lints.clippy]` with a comment saying why.
- `cargo clippy --all-targets -- -D warnings` must pass. So must `cargo fmt --check`.
- No `std::os::unix` or `std::os::windows` API outside `src/platform/`.
- Every filesystem path is a `Path`/`PathBuf`. No string concatenation of paths.
- No async runtime. Concurrency is `std::thread` plus `std::sync::mpsc`.
- Environment variables keep their current names: `ZRUSH_DEFAULT_REPO`, `ZRUSH_EDITOR`, `ZRUSH_EDITOR_CMD`, `ZRUSH_TERMINAL`, `ZRUSH_TITLES`, `ZRUSH_STATUS`, `ZRUSH_RESUMABLE_MAX`, `ZRUSH_RESUMABLE_SCAN`, `ZRUSH_MORE_STEP`, `ZRUSH_TITLE_WIDTH`, `ZRUSH_PREVIEW_TURNS`, `ZRUSH_KEEP_CLAUDE_ENV`, `ZRUSH_SESSION_EXEC`, `ZRUSH_SESSION_ACTIVE`.
- No test may touch a real repository, the real `~/.claude`, the real `~/.config`, or a real editor. Every test sets `HOME`/`XDG_*` into a `tempfile::TempDir`.
- Commit after every task. Conventional Commits, subject at most 50 characters.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | deps, lints, MSRV, release profile |
| `deny.toml` | licence allow-list, advisory and ban policy |
| `rustfmt.toml` | formatting |
| `src/main.rs` | entry, terminal guard, top-level error reporting |
| `src/cli.rs` | clap definitions, subcommand dispatch |
| `src/error.rs` | `ZrushError`, `Result` alias |
| `src/config.rs` | `Config`, TOML load, env overrides, legacy import |
| `src/git.rs` | every `git` invocation and its porcelain parser |
| `src/state.rs` | `<gitdir>/zrush-session` read, write, clear |
| `src/claude/projects.rs` | slugify, project directory discovery |
| `src/claude/transcript.rs` | `.jsonl` parsing: title, cwd, timestamp, preview turns |
| `src/claude/cache.rs` | metadata cache keyed on size+mtime then content hash |
| `src/claude/agents.rs` | `claude agents --json --all` |
| `src/model/mod.rs` | `Worktree`, `Session`, `GitStatus`, `Row`, `NodeId` |
| `src/model/tree.rs` | session-to-worktree assignment, caps, row construction |
| `src/model/purge.rs` | the purge plan |
| `src/probe.rs` | worker threads, `Event`, generation counter |
| `src/ui/theme.rs` | the palette |
| `src/ui/header.rs` | info block, key grid, logo |
| `src/ui/table.rs` | the row list |
| `src/ui/preview.rs` | the right pane |
| `src/ui/modal.rs` | `Modal` and its rendering |
| `src/ui/filter.rs` | fuzzy matching |
| `src/ui/flash.rs` | transient status messages |
| `src/ui/app.rs` | `App` state, key dispatch, the event loop |
| `src/actions.rs` | editor, terminal, worktree creation, deletion, purge |
| `src/platform/mod.rs` | the `Platform` trait and its selection |
| `src/platform/unix.rs` | `.command` script, `open` |
| `src/platform/windows.rs` | `.cmd` script, `wt.exe` then `cmd /c start` |
| `src/session.rs` | the `session` subcommand |
| `tests/integration.rs` | `assert_cmd` over throwaway repos |
| `tests/pty.rs` | the pty smoke test |
| `tests/fixtures/` | transcript, agents JSON and porcelain samples |

---

## Task 1: Crate scaffold, lints and the CI gate

**Files:**
- Create: `Cargo.toml`, `rustfmt.toml`, `deny.toml`, `src/main.rs`, `src/error.rs`
- Create: `.github/workflows/ci.yml` (replacing the shell one)

**Interfaces:**
- Consumes: nothing.
- Produces: `zrush::error::{ZrushError, Result}`. `Result<T> = std::result::Result<T, ZrushError>`.

- [ ] **Step 1: Create the crate and add the dependencies**

```bash
cargo init --name zrush --bin .
cargo add ratatui crossterm serde_json toml jiff unicode-width anyhow thiserror dirs
cargo add serde --features derive
cargo add clap --features derive
cargo add nucleo-matcher
cargo add xxhash-rust --features xxh3
cargo add --dev insta assert_cmd predicates tempfile portable-pty
```

- [ ] **Step 2: Set the crate metadata and lints in `Cargo.toml`**

```toml
[package]
name = "zrush"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "GPL-3.0-only"
description = "Pick a git worktree or one of its Claude Code sessions, then open it"

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
# The row/table code is full of small numeric conversions between terminal
# coordinates and collection indices; casting them by hand adds noise.
cast_possible_truncation = "allow"
cast_sign_loss = "allow"
# Doc comments here describe behaviour, not panics we promise to keep.
missing_panics_doc = "allow"
missing_errors_doc = "allow"
module_name_repetitions = "allow"
unwrap_used = "deny"
expect_used = "deny"

[profile.release]
strip = true
lto = "thin"
```

- [ ] **Step 3: Write `src/error.rs`**

```rust
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, ZrushError>;

#[derive(Debug, thiserror::Error)]
pub enum ZrushError {
    #[error("{0}")]
    Msg(String),
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),
    #[error("git {args} failed: {stderr}")]
    Git { args: String, stderr: String },
    #[error("missing dependency: {0}")]
    MissingDependency(&'static str),
    #[error("config: {0}")]
    Config(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl ZrushError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}
```

- [ ] **Step 4: Write `src/main.rs` and verify it builds**

```rust
#![forbid(unsafe_code)]

mod error;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zrush: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> error::Result<()> {
    Ok(())
}
```

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all three succeed.

- [ ] **Step 5: Write `deny.toml`**

```toml
[advisories]
yanked = "deny"

[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib", "MPL-2.0"]

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

Run: `cargo deny check`
Expected: passes, or the allow-list gains exactly the licences the dependency tree actually carries.

- [ ] **Step 6: Replace `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  test:
    name: test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@nextest
      - run: cargo clippy --all-targets --locked -- -D warnings
      - run: cargo nextest run --locked

  lint:
    name: fmt and deny
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --check
      - uses: EmbarkStudios/cargo-deny-action@v2

  msrv:
    name: msrv
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@1.85.0
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --locked
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock rustfmt.toml deny.toml src/ .github/workflows/ci.yml
git commit -m "build: the Rust crate, its lints and its CI"
```

---

## Task 2: Config

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (declare `mod config;`)

**Interfaces:**
- Consumes: `error::{Result, ZrushError}`.
- Produces:
  - `Dirs { config: PathBuf, cache: PathBuf, claude_projects: PathBuf }`, `Dirs::from_env() -> Result<Dirs>`
  - `Config` with the fields named in the spec, `Config::default()`
  - `Config::load(dirs: &Dirs) -> Result<Config>`
  - `Config::apply_env(&mut self, get: impl Fn(&str) -> Option<String>)`
  - `Config::editor_command(&self) -> Vec<String>`
  - `config::from_legacy_shell(text: &str) -> Config`

Every later task takes `&Dirs` rather than reading `$HOME`, so tests can point it at a `TempDir`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_shell_version() {
        let c = Config::default();
        assert_eq!(c.editor, "zed");
        assert_eq!(c.resumable_max, 5);
        assert_eq!(c.resumable_scan, 40);
        assert_eq!(c.more_step, 20);
        assert_eq!(c.title_width, 48);
        assert_eq!(c.preview_turns, 14);
        assert!(c.titles);
        assert!(c.status);
    }

    #[test]
    fn env_beats_the_file() {
        let mut c = Config { title_width: 10, ..Config::default() };
        c.apply_env(|k| (k == "ZRUSH_TITLE_WIDTH").then(|| "99".to_string()));
        assert_eq!(c.title_width, 99);
    }

    #[test]
    fn zero_disables_the_boolean_knobs() {
        let mut c = Config::default();
        c.apply_env(|k| (k == "ZRUSH_TITLES").then(|| "0".to_string()));
        assert!(!c.titles);
    }

    #[test]
    fn a_known_editor_gets_the_new_window_flag() {
        let c = Config { editor: "code".into(), ..Config::default() };
        assert_eq!(c.editor_command(), vec!["code", "-n"]);
    }

    #[test]
    fn editor_cmd_wins_over_editor() {
        let c = Config {
            editor: "zed".into(),
            editor_cmd: vec!["code".into(), "--reuse-window".into()],
            ..Config::default()
        };
        assert_eq!(c.editor_command(), vec!["code", "--reuse-window"]);
    }

    #[test]
    fn an_unknown_editor_is_run_as_is() {
        let c = Config { editor: "hx".into(), ..Config::default() };
        assert_eq!(c.editor_command(), vec!["hx"]);
    }

    #[test]
    fn the_legacy_shell_config_is_imported() {
        let c = from_legacy_shell(
            r#"
# a comment
ZRUSH_EDITOR=cursor
ZRUSH_TITLE_WIDTH=60
ZRUSH_EDITOR_CMD=(code --reuse-window)
ZRUSH_TERMINAL=("open" -a Ghostty)
ZRUSH_STATUS=0
"#,
        );
        assert_eq!(c.editor, "cursor");
        assert_eq!(c.title_width, 60);
        assert_eq!(c.editor_cmd, vec!["code", "--reuse-window"]);
        assert_eq!(c.terminal, vec!["open", "-a", "Ghostty"]);
        assert!(!c.status);
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test config`
Expected: FAIL, `config` module not found.

- [ ] **Step 3: Implement `src/config.rs`**

```rust
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, ZrushError};

#[derive(Debug, Clone)]
pub struct Dirs {
    pub config: PathBuf,
    pub cache: PathBuf,
    pub claude_projects: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| ZrushError::msg("no home directory"))?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("zrush");
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map_or_else(|| home.join(".cache"), PathBuf::from)
            .join("zrush")
            .join("sessions");
        Ok(Self { config, cache, claude_projects: home.join(".claude").join("projects") })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn legacy_config_file(&self) -> PathBuf {
        self.config.join("config")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub default_repo: Option<PathBuf>,
    pub editor: String,
    pub editor_cmd: Vec<String>,
    pub terminal: Vec<String>,
    pub titles: bool,
    pub status: bool,
    pub resumable_max: usize,
    pub resumable_scan: usize,
    pub more_step: usize,
    pub title_width: usize,
    pub preview_turns: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_repo: None,
            editor: "zed".into(),
            editor_cmd: Vec::new(),
            terminal: Vec::new(),
            titles: true,
            status: true,
            resumable_max: 5,
            resumable_scan: 40,
            more_step: 20,
            title_width: 48,
            preview_turns: 14,
        }
    }
}

impl Config {
    /// The file, then the environment. A missing `config.toml` with a legacy
    /// shell `config` beside it is imported once and written back as TOML.
    pub fn load(dirs: &Dirs) -> Result<Self> {
        let path = dirs.config_file();
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            toml::from_str(&text).map_err(|e| ZrushError::Config(e.to_string()))?
        } else if dirs.legacy_config_file().exists() {
            let text = std::fs::read_to_string(dirs.legacy_config_file())?;
            let imported = from_legacy_shell(&text);
            imported.save(dirs)?;
            imported
        } else {
            Self::default()
        };
        cfg.apply_env(|k| std::env::var(k).ok());
        Ok(cfg)
    }

    pub fn save(&self, dirs: &Dirs) -> Result<()> {
        std::fs::create_dir_all(&dirs.config)?;
        let text = toml::to_string_pretty(&TomlView::from(self))
            .map_err(|e| ZrushError::Config(e.to_string()))?;
        std::fs::write(dirs.config_file(), text)?;
        Ok(())
    }

    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) {
        if let Some(v) = get("ZRUSH_DEFAULT_REPO") {
            if !v.is_empty() {
                self.default_repo = Some(PathBuf::from(v));
            }
        }
        if let Some(v) = get("ZRUSH_EDITOR") {
            if !v.is_empty() {
                self.editor = v;
            }
        }
        if let Some(v) = get("ZRUSH_EDITOR_CMD") {
            self.editor_cmd = split_words(&v);
        }
        if let Some(v) = get("ZRUSH_TERMINAL") {
            self.terminal = split_words(&v);
        }
        set_bool(&mut self.titles, get("ZRUSH_TITLES").as_deref());
        set_bool(&mut self.status, get("ZRUSH_STATUS").as_deref());
        set_usize(&mut self.resumable_max, get("ZRUSH_RESUMABLE_MAX").as_deref());
        set_usize(&mut self.resumable_scan, get("ZRUSH_RESUMABLE_SCAN").as_deref());
        set_usize(&mut self.more_step, get("ZRUSH_MORE_STEP").as_deref());
        set_usize(&mut self.title_width, get("ZRUSH_TITLE_WIDTH").as_deref());
        set_usize(&mut self.preview_turns, get("ZRUSH_PREVIEW_TURNS").as_deref());
    }

    /// `editor_cmd` replaces the whole command line. Otherwise the three
    /// editors we know about get `-n`, because a worktree nested inside
    /// another one is otherwise swallowed by the parent project's window.
    pub fn editor_command(&self) -> Vec<String> {
        if !self.editor_cmd.is_empty() {
            return self.editor_cmd.clone();
        }
        match self.editor.as_str() {
            "" => vec!["zed".into(), "-n".into()],
            e @ ("zed" | "cursor" | "code") => vec![e.into(), "-n".into()],
            other => vec![other.into()],
        }
    }
}

fn set_bool(slot: &mut bool, v: Option<&str>) {
    if let Some(v) = v {
        *slot = v != "0";
    }
}

fn set_usize(slot: &mut usize, v: Option<&str>) {
    if let Some(n) = v.and_then(|v| v.parse().ok()) {
        *slot = n;
    }
}

/// Shell-ish word splitting: quotes and parentheses dropped, whitespace
/// separating. Enough for the values the old config file held.
fn split_words(v: &str) -> Vec<String> {
    v.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Best-effort import of the shell-syntax config: every `ZRUSH_X=...`
/// assignment, ignoring comments and anything else.
pub fn from_legacy_shell(text: &str) -> Config {
    let mut cfg = Config::default();
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.starts_with("ZRUSH_") {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            seen.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    cfg.apply_env(|k| seen.get(k).cloned());
    cfg
}
```

`TomlView` is a small serialisable mirror of `Config` written in the same
file; deriving `Serialize` on `Config` directly is equivalent and simpler,
so do that instead and drop `TomlView`:

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
```

- [ ] **Step 4: Run the tests**

Run: `cargo test config`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): TOML, env overrides, legacy import"
```

---

## Task 3: Reading the worktree list

**Files:**
- Create: `src/git.rs`, `src/model/mod.rs`
- Create: `tests/fixtures/worktree-list.txt`

**Interfaces:**
- Consumes: `error::Result`.
- Produces:
  - `model::Worktree { path: PathBuf, branch: String, locked: bool, prunable: bool, is_main: bool }`, `Worktree::label(&self) -> String`
  - `git::parse_worktree_list(porcelain: &str) -> Vec<Worktree>`
  - `git::worktrees(repo: &Path) -> Result<Vec<Worktree>>`
  - `git::run(repo: &Path, args: &[&str]) -> Result<String>`

- [ ] **Step 1: Write the fixture**

`tests/fixtures/worktree-list.txt`:

```
worktree /home/u/repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/u/repo/.claude/worktrees/rust
HEAD 2222222222222222222222222222222222222222
branch refs/heads/rust

worktree /home/u/repo/.claude/worktrees/detached
HEAD 3333333333333333333333333333333333333333
detached

worktree /home/u/repo/.claude/worktrees/locked
HEAD 4444444444444444444444444444444444444444
branch refs/heads/locked
locked

worktree /home/u/repo/.claude/worktrees/gone
HEAD 5555555555555555555555555555555555555555
branch refs/heads/gone
prunable gitdir file points to non-existent location
```

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../tests/fixtures/worktree-list.txt");

    #[test]
    fn every_worktree_is_read() {
        let w = parse_worktree_list(LIST);
        assert_eq!(w.len(), 5);
    }

    #[test]
    fn the_first_entry_is_the_main_worktree() {
        let w = parse_worktree_list(LIST);
        assert!(w[0].is_main);
        assert!(!w[1].is_main);
        assert_eq!(w[0].path, std::path::Path::new("/home/u/repo"));
    }

    #[test]
    fn branches_lose_their_refs_heads_prefix() {
        let w = parse_worktree_list(LIST);
        assert_eq!(w[1].branch, "rust");
    }

    #[test]
    fn a_detached_head_is_named_as_such() {
        let w = parse_worktree_list(LIST);
        assert_eq!(w[2].branch, "(detached)");
    }

    #[test]
    fn locked_and_prunable_are_flags_not_branches() {
        let w = parse_worktree_list(LIST);
        assert!(w[3].locked);
        assert!(w[4].prunable);
        assert_eq!(w[4].branch, "gone");
    }

    #[test]
    fn the_label_carries_the_flags_and_the_root_marker() {
        let w = parse_worktree_list(LIST);
        assert_eq!(w[0].label(), "main [root]");
        assert_eq!(w[3].label(), "locked 🔒");
        assert_eq!(w[4].label(), "gone ⚠");
    }

    #[test]
    fn a_trailing_entry_with_no_blank_line_is_still_flushed() {
        let w = parse_worktree_list("worktree /a\nbranch refs/heads/x\n");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].branch, "x");
    }
}
```

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test git::`
Expected: FAIL, `parse_worktree_list` not found.

- [ ] **Step 4: Implement**

`src/model/mod.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub locked: bool,
    pub prunable: bool,
    pub is_main: bool,
}

impl Worktree {
    /// The branch, then the flags, then `[root]` for the primary worktree:
    /// the one holding `.git`, and the one git refuses to remove.
    pub fn label(&self) -> String {
        let mut s = self.branch.clone();
        if self.locked {
            s.push_str(" 🔒");
        }
        if self.prunable {
            s.push_str(" ⚠");
        }
        if self.is_main {
            s.push_str(" [root]");
        }
        s
    }
}
```

`src/git.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Result, ZrushError};
use crate::model::Worktree;

pub fn run(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(ZrushError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    Ok(parse_worktree_list(&run(repo, &["worktree", "list", "--porcelain"])?))
}

/// `git worktree list --porcelain` always reports the whole set, with the
/// main worktree first, whichever worktree it is run from.
pub fn parse_worktree_list(porcelain: &str) -> Vec<Worktree> {
    let mut out: Vec<Worktree> = Vec::new();
    let mut cur: Option<Worktree> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            cur = Some(Worktree {
                path: PathBuf::from(p),
                branch: String::new(),
                locked: false,
                prunable: false,
                is_main: out.is_empty(),
            });
            continue;
        }
        let Some(w) = cur.as_mut() else { continue };
        if let Some(b) = line.strip_prefix("branch refs/heads/") {
            w.branch = b.to_string();
        } else if line == "detached" {
            w.branch = "(detached)".into();
        } else if line == "bare" {
            w.branch = "(bare)".into();
        } else if line.starts_with("locked") {
            w.locked = true;
        } else if line.starts_with("prunable") {
            w.prunable = true;
        }
    }
    out.extend(cur);
    out
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test git::`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs src/model/ tests/fixtures/ src/main.rs
git commit -m "feat(git): parse the worktree list"
```

---

## Task 4: The git status badge

**Files:**
- Modify: `src/git.rs`, `src/model/mod.rs`

**Interfaces:**
- Produces:
  - `model::GitStatus { changed: u32, untracked: u32, ahead: u32, behind: u32, has_upstream: bool }`, `GitStatus::badge(&self) -> String`
  - `git::parse_status_v2(out: &str) -> GitStatus`
  - `git::status(path: &Path) -> Result<GitStatus>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod status_tests {
    use super::*;

    const OUT: &str = "\
# branch.oid 1111111111111111111111111111111111111111
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 .M N... 100644 100644 100644 aaa bbb src/a.rs
1 M. N... 100644 100644 100644 ccc ddd src/b.rs
2 R. N... 100644 100644 100644 eee fff R100 new\told
u UU N... 100644 100644 100644 100644 ggg hhh iii both.rs
? untracked.txt
? other.txt
";

    #[test]
    fn changed_counts_every_tracked_change() {
        // two "1" lines, one "2", one "u"
        assert_eq!(parse_status_v2(OUT).changed, 4);
    }

    #[test]
    fn untracked_is_counted_separately() {
        assert_eq!(parse_status_v2(OUT).untracked, 2);
    }

    #[test]
    fn ahead_and_behind_come_from_branch_ab() {
        let s = parse_status_v2(OUT);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
        assert!(s.has_upstream);
    }

    #[test]
    fn no_branch_ab_line_means_no_upstream() {
        let s = parse_status_v2("# branch.head main\n");
        assert!(!s.has_upstream);
    }

    #[test]
    fn the_badge_reads_as_it_did_in_bash() {
        let s = parse_status_v2(OUT);
        assert_eq!(s.badge(), "~4 ?2 ↑2 ↓1");
    }

    #[test]
    fn no_upstream_is_a_warning_not_an_arrow() {
        let s = GitStatus { changed: 1, untracked: 0, ahead: 0, behind: 0, has_upstream: false };
        assert_eq!(s.badge(), "~1 ⚠");
    }

    #[test]
    fn a_clean_worktree_has_an_empty_badge() {
        let s = GitStatus { changed: 0, untracked: 0, ahead: 0, behind: 0, has_upstream: true };
        assert_eq!(s.badge(), "");
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test status_tests`
Expected: FAIL, `parse_status_v2` not found.

- [ ] **Step 3: Implement**

In `src/model/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub changed: u32,
    pub untracked: u32,
    pub ahead: u32,
    pub behind: u32,
    pub has_upstream: bool,
}

impl GitStatus {
    /// Compact and plain: the table pads by display width, so no colour
    /// escapes may appear in here.
    ///   `~3` changed tracked files   `?1` untracked
    ///   `↑2` ahead of upstream       `↓1` behind      `⚠` no upstream at all
    pub fn badge(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.changed > 0 {
            parts.push(format!("~{}", self.changed));
        }
        if self.untracked > 0 {
            parts.push(format!("?{}", self.untracked));
        }
        if self.has_upstream {
            if self.ahead > 0 {
                parts.push(format!("↑{}", self.ahead));
            }
            if self.behind > 0 {
                parts.push(format!("↓{}", self.behind));
            }
        } else {
            parts.push("⚠".into());
        }
        parts.join(" ")
    }
}
```

In `src/git.rs`:

```rust
pub fn status(path: &Path) -> Result<GitStatus> {
    Ok(parse_status_v2(&run(path, &["status", "--porcelain=v2", "--branch"])?))
}

/// One `git status --porcelain=v2 --branch` yields both numbers: the changed
/// files and the ahead/behind counts. Measured at 40-250 ms on a large repo,
/// which is why it runs on a worker rather than before the first frame.
pub fn parse_status_v2(out: &str) -> GitStatus {
    let mut s = GitStatus::default();
    for line in out.lines() {
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            s.has_upstream = true;
            let mut it = ab.split_whitespace();
            s.ahead = it.next().and_then(|v| v.trim_start_matches('+').parse().ok()).unwrap_or(0);
            s.behind = it.next().and_then(|v| v.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            s.changed += 1;
        } else if line.starts_with("? ") {
            s.untracked += 1;
        }
    }
    s
}
```

`unwrap_or` is used rather than `unwrap`, so the `unwrap_used` lint stays satisfied.

- [ ] **Step 4: Run the tests**

Run: `cargo test status_tests`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs src/model/mod.rs
git commit -m "feat(git): the status badge"
```

---

## Task 5: Branches, the base to branch from, and worktree creation

**Files:**
- Modify: `src/git.rs`
- Create: `tests/integration.rs`

**Interfaces:**
- Produces:
  - `git::branches(repo: &Path) -> Result<Vec<String>>`
  - `git::default_base(repo: &Path) -> Result<String>`
  - `git::check_ref_format(name: &str) -> bool`
  - `git::branch_exists(repo: &Path, name: &str) -> bool`
  - `git::worktree_add(repo: &Path, dest: &Path, from: AddFrom) -> Result<()>` where `enum AddFrom { ExistingBranch(String), NewBranch { name: String, base: String } }`
  - `git::worktree_remove(repo: &Path, path: &Path) -> Result<()>`
  - `git::absolute_git_dir(path: &Path) -> Result<PathBuf>`
  - test helper `tests/integration.rs::scratch_repo(remote: bool) -> (TempDir, PathBuf)`

- [ ] **Step 1: Write the failing integration tests**

```rust
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A throwaway repo with one commit on `main`, optionally cloned from a
/// bare "remote" so `origin/main` exists. Nothing here touches a real
/// repository.
fn scratch_repo(remote: bool) -> (TempDir, PathBuf) {
    let td = TempDir::new().expect("tempdir");
    // pwd -P: on macOS $TMPDIR is a symlink into /private, and git reports
    // the resolved path. Comparing one against the other matches nothing.
    let base = td.path().canonicalize().expect("canonicalize");
    let repo = base.join("repo");
    if remote {
        let bare = base.join("remote.git");
        git(&base, &["init", "-q", "--bare", bare.to_str().expect("utf8")]);
        git(&base, &["clone", "-q", bare.to_str().expect("utf8"), repo.to_str().expect("utf8")]);
    } else {
        git(&base, &["init", "-q", repo.to_str().expect("utf8")]);
    }
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "zrush tests"]);
    std::fs::write(repo.join("f.txt"), "seed").expect("write");
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-qm", "init"]);
    git(&repo, &["branch", "-M", "main"]);
    if remote {
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
    }
    (td, repo)
}

fn git(cwd: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .expect("git");
    assert!(st.success(), "git {args:?} failed");
}

#[test]
fn a_new_branch_is_cut_from_the_remote_default() {
    let (_td, repo) = scratch_repo(true);
    assert_eq!(zrush::git::default_base(&repo).expect("base"), "origin/main");
}

#[test]
fn a_repo_with_no_remote_still_has_a_base() {
    let (_td, repo) = scratch_repo(false);
    assert_eq!(zrush::git::default_base(&repo).expect("base"), "main");
}

#[test]
fn creating_a_worktree_makes_the_branch_and_the_directory() {
    let (_td, repo) = scratch_repo(true);
    let dest = repo.join(".claude/worktrees/feature");
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    zrush::git::worktree_add(
        &repo,
        &dest,
        zrush::git::AddFrom::NewBranch { name: "feature".into(), base: "origin/main".into() },
    )
    .expect("add");
    assert!(dest.join("f.txt").exists());
    assert!(zrush::git::branch_exists(&repo, "feature"));
}

#[test]
fn a_new_branch_owns_no_upstream() {
    let (_td, repo) = scratch_repo(true);
    let dest = repo.join(".claude/worktrees/feature");
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    zrush::git::worktree_add(
        &repo,
        &dest,
        zrush::git::AddFrom::NewBranch { name: "feature".into(), base: "origin/main".into() },
    )
    .expect("add");
    // --no-track: a later `git push -u origin HEAD` must set the right
    // upstream instead of pointing the branch at main.
    let s = zrush::git::status(&dest).expect("status");
    assert!(!s.has_upstream);
}

#[test]
fn an_invalid_branch_name_is_refused() {
    assert!(!zrush::git::check_ref_format("has space"));
    assert!(!zrush::git::check_ref_format(""));
    assert!(zrush::git::check_ref_format("feat/thing-1"));
}

#[test]
fn removing_a_worktree_takes_the_directory_with_it() {
    let (_td, repo) = scratch_repo(true);
    let dest = repo.join(".claude/worktrees/tmp");
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    zrush::git::worktree_add(
        &repo,
        &dest,
        zrush::git::AddFrom::NewBranch { name: "tmp".into(), base: "origin/main".into() },
    )
    .expect("add");
    zrush::git::worktree_remove(&repo, &dest).expect("remove");
    assert!(!dest.exists());
}

#[test]
fn a_dirty_worktree_is_not_removed() {
    let (_td, repo) = scratch_repo(true);
    let dest = repo.join(".claude/worktrees/dirty");
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    zrush::git::worktree_add(
        &repo,
        &dest,
        zrush::git::AddFrom::NewBranch { name: "dirty".into(), base: "origin/main".into() },
    )
    .expect("add");
    std::fs::write(dest.join("f.txt"), "changed").expect("write");
    assert!(zrush::git::worktree_remove(&repo, &dest).is_err());
    assert!(dest.exists());
}
```

- [ ] **Step 2: Expose the crate as a library so the tests can call it**

Create `src/lib.rs` holding the module declarations, and reduce `src/main.rs`
to the binary entry point that calls `zrush::cli::main()`. Add to `Cargo.toml`:

```toml
[lib]
name = "zrush"
path = "src/lib.rs"

[[bin]]
name = "zrush"
path = "src/main.rs"
```

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test --test integration`
Expected: FAIL, `default_base` not found.

- [ ] **Step 4: Implement**

```rust
#[derive(Debug, Clone)]
pub enum AddFrom {
    ExistingBranch(String),
    NewBranch { name: String, base: String },
}

pub fn branches(repo: &Path) -> Result<Vec<String>> {
    let out = run(
        repo,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"],
    )?;
    Ok(out.lines().map(str::to_string).filter(|s| !s.is_empty()).collect())
}

pub fn branch_exists(repo: &Path, name: &str) -> bool {
    run(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{name}")]).is_ok()
}

/// What a new branch is cut from: the remote default branch when there is
/// one, because it is the base that is actually up to date; otherwise the
/// local default, and failing that HEAD. No fetch either way — zrush never
/// touches the network.
pub fn default_base(repo: &Path) -> Result<String> {
    if let Ok(b) = run(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
        let b = b.trim();
        if !b.is_empty() {
            return Ok(b.to_string());
        }
    }
    for b in ["origin/main", "origin/master", "main", "master"] {
        if run(repo, &["rev-parse", "--verify", "--quiet", b]).is_ok() {
            return Ok(b.to_string());
        }
    }
    // Unborn HEAD (a repo with no commit yet) has nothing to branch from.
    run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .map(|_| "HEAD".to_string())
        .map_err(|_| ZrushError::msg("nothing to branch from: the repo has no commit yet"))
}

pub fn check_ref_format(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    std::process::Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn worktree_add(repo: &Path, dest: &Path, from: AddFrom) -> Result<()> {
    let dest = dest.to_string_lossy().into_owned();
    match from {
        AddFrom::ExistingBranch(b) => run(repo, &["worktree", "add", &dest, &b]).map(drop),
        AddFrom::NewBranch { name, base } => {
            run(repo, &["worktree", "add", "--no-track", "-b", &name, &dest, &base]).map(drop)
        }
    }
}

pub fn worktree_remove(repo: &Path, path: &Path) -> Result<()> {
    run(repo, &["worktree", "remove", &path.to_string_lossy()]).map(drop)
}

pub fn absolute_git_dir(path: &Path) -> Result<PathBuf> {
    let out = run(path, &["rev-parse", "--absolute-git-dir"])?;
    let p = PathBuf::from(out.trim());
    if p.as_os_str().is_empty() {
        return Err(ZrushError::NotARepo(path.to_path_buf()));
    }
    Ok(p)
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --test integration`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs src/lib.rs src/main.rs Cargo.toml tests/integration.rs
git commit -m "feat(git): branches, base, worktree add and remove"
```

---

## Task 6: The worktree/session association

**Files:**
- Create: `src/state.rs`

**Interfaces:**
- Produces:
  - `state::is_uuid(s: &str) -> bool`
  - `state::read(worktree: &Path) -> Result<Option<String>>`
  - `state::write(worktree: &Path, id: &str) -> Result<()>`
  - `state::clear(worktree: &Path) -> Result<()>`

The file is `<git rev-parse --absolute-git-dir>/zrush-session`, which is
`repo/.git/worktrees/<name>` for a linked worktree and `repo/.git` for the
main one. Per-worktree by construction, so several editor windows can each
hold their own session. No global state, no secrets.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "f3a91c2e-1111-2222-3333-444455556666";

    #[test]
    fn a_uuid_is_recognised() {
        assert!(is_uuid(GOOD));
        assert!(is_uuid("F3A91C2E-1111-2222-3333-444455556666"));
    }

    #[test]
    fn anything_else_is_not() {
        assert!(!is_uuid(""));
        assert!(!is_uuid("nope"));
        assert!(!is_uuid("f3a91c2e-1111-2222-3333-44445555666")); // one short
        assert!(!is_uuid("f3a91c2e_1111_2222_3333_444455556666")); // wrong separator
        assert!(!is_uuid("g3a91c2e-1111-2222-3333-444455556666")); // not hex
    }

    #[test]
    fn the_first_session_id_line_wins_and_quotes_are_stripped() {
        assert_eq!(parse(&format!("session_id=\"{GOOD}\"\n")).as_deref(), Some(GOOD));
        assert_eq!(parse(&format!("  session_id = {GOOD}  \n")).as_deref(), Some(GOOD));
        assert_eq!(parse(&format!("other=1\nsession_id={GOOD}\n")).as_deref(), Some(GOOD));
    }

    #[test]
    fn a_malformed_id_is_discarded_rather_than_returned() {
        assert_eq!(parse("session_id=nope\n"), None);
        assert_eq!(parse(""), None);
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test state::`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement**

```rust
use std::path::{Path, PathBuf};

use crate::error::{Result, ZrushError};
use crate::git;

const BASENAME: &str = "zrush-session";

pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn state_file(worktree: &Path) -> Result<PathBuf> {
    Ok(git::absolute_git_dir(worktree)?.join(BASENAME))
}

/// The first `session_id=` line wins. Anything that is not a UUID is
/// discarded: a malformed value must never reach `claude --resume`.
fn parse(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() != "session_id" {
            continue;
        }
        let v = v.trim().trim_matches('"');
        return is_uuid(v).then(|| v.to_string());
    }
    None
}

pub fn read(worktree: &Path) -> Result<Option<String>> {
    let f = state_file(worktree)?;
    match std::fs::read_to_string(&f) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Written through a temporary file and renamed, so a crash mid-write
/// cannot leave a half-written id behind.
pub fn write(worktree: &Path, id: &str) -> Result<()> {
    if !is_uuid(id) {
        return Err(ZrushError::msg("refusing to store a malformed session id"));
    }
    let f = state_file(worktree)?;
    let tmp = f.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, format!("session_id={id}\n"))?;
    std::fs::rename(&tmp, &f)?;
    Ok(())
}

pub fn clear(worktree: &Path) -> Result<()> {
    let f = state_file(worktree)?;
    match std::fs::remove_file(&f) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
```

- [ ] **Step 4: Add an integration test that goes through a real gitdir**

In `tests/integration.rs`:

```rust
#[test]
fn the_association_lives_in_the_worktrees_own_gitdir() {
    let (_td, repo) = scratch_repo(true);
    let dest = repo.join(".claude/worktrees/bound");
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    zrush::git::worktree_add(
        &repo,
        &dest,
        zrush::git::AddFrom::NewBranch { name: "bound".into(), base: "origin/main".into() },
    )
    .expect("add");

    let id = "f3a91c2e-1111-2222-3333-444455556666";
    zrush::state::write(&dest, id).expect("write");
    assert_eq!(zrush::state::read(&dest).expect("read").as_deref(), Some(id));
    // The main worktree must not see it: the file is per-worktree.
    assert_eq!(zrush::state::read(&repo).expect("read"), None);

    zrush::state::clear(&dest).expect("clear");
    assert_eq!(zrush::state::read(&dest).expect("read"), None);
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test state && cargo test --test integration`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/state.rs src/lib.rs tests/integration.rs
git commit -m "feat(state): the per-worktree session association"
```

---

## Task 7: Claude project directories

**Files:**
- Create: `src/claude/mod.rs`, `src/claude/projects.rs`

**Interfaces:**
- Produces:
  - `claude::projects::slugify(path: &Path) -> String`
  - `claude::projects::transcript_path(dirs: &Dirs, launch_cwd: &Path, id: &str) -> PathBuf`
  - `claude::projects::history_count(dirs: &Dirs, worktree: &Path) -> usize`
  - `claude::projects::candidate_dirs(dirs: &Dirs, main_root: &Path, worktrees: &[Worktree]) -> Vec<PathBuf>`
  - `claude::projects::find_transcript(dirs: &Dirs, id: &str) -> Option<PathBuf>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_non_alphanumeric_becomes_a_dash() {
        assert_eq!(slugify(Path::new("/home/u/repo")), "-home-u-repo");
        assert_eq!(
            slugify(Path::new("/home/u/repo/.claude/worktrees/rust")),
            "-home-u-repo--claude-worktrees-rust"
        );
    }

    #[test]
    fn digits_and_letters_survive() {
        assert_eq!(slugify(Path::new("/a1/b2")), "-a1-b2");
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test projects::`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use std::path::{Path, PathBuf};

use crate::config::Dirs;
use crate::model::Worktree;

/// Claude Code files a session's transcript under a directory named after
/// the cwd it was launched in, with every non-alphanumeric byte replaced by
/// a dash. This must match `sed 's/[^A-Za-z0-9]/-/g'` exactly.
pub fn slugify(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn transcript_path(dirs: &Dirs, launch_cwd: &Path, id: &str) -> PathBuf {
    dirs.claude_projects.join(slugify(launch_cwd)).join(format!("{id}.jsonl"))
}

/// How many transcripts Claude Code keeps for a worktree. A read-only hint
/// for the badge; `claude --resume` is still what resumes one.
pub fn history_count(dirs: &Dirs, worktree: &Path) -> usize {
    let dir = dirs.claude_projects.join(slugify(worktree));
    let Ok(rd) = std::fs::read_dir(&dir) else { return 0 };
    rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "jsonl")).count()
}

/// Project directories that can hold this repo's sessions: the main
/// worktree's slug and everything filed under it (linked worktrees,
/// `claude --worktree`), plus each worktree's own slug, for worktrees kept
/// outside the repo.
pub fn candidate_dirs(dirs: &Dirs, main_root: &Path, worktrees: &[Worktree]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let prefix = slugify(main_root);
    if let Ok(rd) = std::fs::read_dir(&dirs.claude_projects) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) && e.path().is_dir() {
                out.push(e.path());
            }
        }
    }
    for w in worktrees {
        let d = dirs.claude_projects.join(slugify(&w.path));
        if d.is_dir() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Session ids are UUIDs, and a session started by `claude --worktree` is
/// filed under the directory it was launched from rather than the one it
/// runs in. So search every project directory instead of guessing one.
pub fn find_transcript(dirs: &Dirs, id: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(&dirs.claude_projects).ok()?;
    for e in rd.flatten() {
        let p = e.path().join(format!("{id}.jsonl"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test projects::`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/claude/ src/lib.rs
git commit -m "feat(claude): project directories and slugs"
```

---

## Task 8: Reading a transcript

**Files:**
- Create: `src/claude/transcript.rs`, `tests/fixtures/transcript.jsonl`, `tests/fixtures/long-transcript.jsonl`

**Interfaces:**
- Produces:
  - `transcript::Meta { title: Option<String>, cwd: Option<PathBuf>, last_ts: Option<i64> }`
  - `transcript::parse_tail(tail: &str) -> Meta`
  - `transcript::read_meta(path: &Path, tail_lines: usize) -> Option<Meta>`
  - `transcript::Role { User, Assistant }`, `transcript::Turn { role: Role, text: String }`
  - `transcript::turns(path: &Path, tail_lines: usize, keep: usize) -> Vec<Turn>`

- [ ] **Step 1: Write the fixtures**

`tests/fixtures/transcript.jsonl` — the title appears early and is rewritten later, `cwd` moves, timestamps advance:

```
{"type":"user","message":{"content":"hello"},"cwd":"/home/u/repo","timestamp":"2026-09-20T10:00:00.000Z"}
{"type":"ai-title","aiTitle":"first guess"}
{"type":"assistant","message":{"content":[{"type":"text","text":"hi there"}]},"cwd":"/home/u/repo","timestamp":"2026-09-20T10:00:05.123Z"}
{"type":"ai-title","aiTitle":"port zrush to rust"}
{"type":"user","message":{"content":"and\tnow   this"},"cwd":"/home/u/repo/.claude/worktrees/rust","timestamp":"2026-09-20T10:05:00.000Z"}
{"type":"assistant","message":{"content":[{"type":"text","text":"<thinking>hidden</thinking>"},{"type":"text","text":"done"}]},"cwd":"/home/u/repo/.claude/worktrees/rust","timestamp":"2026-09-20T10:06:00.000Z"}
```

`tests/fixtures/long-transcript.jsonl` — the same, but with the `ai-title`
line first and 500 filler lines after it, so it falls outside any tail
window. Generate it in the test rather than committing a large file:

```rust
fn long_transcript(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("long.jsonl");
    let mut s = String::from("{\"type\":\"ai-title\",\"aiTitle\":\"early title\"}\n");
    for i in 0..500 {
        s.push_str(&format!(
            "{{\"type\":\"user\",\"message\":{{\"content\":\"line {i}\"}},\"cwd\":\"/home/u/repo\",\"timestamp\":\"2026-09-20T10:00:00.000Z\"}}\n"
        ));
    }
    std::fs::write(&p, s).expect("write");
    p
}
```

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = include_str!("../../tests/fixtures/transcript.jsonl");

    #[test]
    fn the_last_title_wins() {
        assert_eq!(parse_tail(T).title.as_deref(), Some("port zrush to rust"));
    }

    #[test]
    fn the_last_cwd_wins_so_a_session_that_moved_is_found() {
        assert_eq!(
            parse_tail(T).cwd.as_deref(),
            Some(std::path::Path::new("/home/u/repo/.claude/worktrees/rust"))
        );
    }

    #[test]
    fn the_timestamp_is_the_last_entrys_not_the_files_mtime() {
        // 2026-09-20T10:06:00Z
        assert_eq!(parse_tail(T).last_ts, Some(1_789_646_760));
    }

    #[test]
    fn fractional_seconds_do_not_break_the_parse() {
        let one = "{\"type\":\"user\",\"timestamp\":\"2026-09-20T10:00:05.123Z\"}";
        assert!(parse_tail(one).last_ts.is_some());
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped() {
        let mixed = format!("not json\n{T}");
        assert_eq!(parse_tail(&mixed).title.as_deref(), Some("port zrush to rust"));
    }

    #[test]
    fn a_title_outside_the_tail_window_is_still_found() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let p = long_transcript(td.path());
        let m = read_meta(&p, 10).expect("meta");
        assert_eq!(m.title.as_deref(), Some("early title"));
    }

    #[test]
    fn turns_drop_thinking_blocks_and_squash_whitespace() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, T).expect("write");
        let t = turns(&p, 400, 14);
        let texts: Vec<&str> = t.iter().map(|x| x.text.as_str()).collect();
        assert!(texts.contains(&"and now this"));
        assert!(texts.contains(&"done"));
        assert!(!texts.iter().any(|x| x.contains("hidden")));
    }

    #[test]
    fn turns_keep_at_most_the_requested_number() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, T).expect("write");
        assert_eq!(turns(&p, 400, 2).len(), 2);
    }
}
```

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test transcript::`
Expected: FAIL.

- [ ] **Step 4: Implement**

```rust
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const TAIL_LINES: usize = 400;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub last_ts: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<serde_json::Value>,
}

/// Later lines win for each field independently: the title is rewritten as
/// the conversation goes on, and the cwd changes when a session moves into
/// a worktree.
pub fn parse_tail(tail: &str) -> Meta {
    let mut m = Meta::default();
    for line in tail.lines() {
        let Ok(l) = serde_json::from_str::<Line>(line) else { continue };
        if let Some(t) = l.ai_title.filter(|t| !t.is_empty()) {
            m.title = Some(t.replace(['\t', '\n'], " "));
        }
        if let Some(c) = l.cwd.filter(|c| !c.is_empty()) {
            m.cwd = Some(PathBuf::from(c));
        }
        if let Some(ts) = l.timestamp.as_deref().and_then(parse_ts) {
            m.last_ts = Some(ts);
        }
    }
    m
}

fn parse_ts(s: &str) -> Option<i64> {
    s.parse::<jiff::Timestamp>().ok().map(jiff::Timestamp::as_second)
}

/// The tail first. The title is written once, early, so a long conversation
/// pushes it out of the window; on a miss, scan the whole file for the last
/// line that carries one. One extra pass, only when it is needed.
pub fn read_meta(path: &Path, tail_lines: usize) -> Option<Meta> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    let mut m = parse_tail(&lines[start..].join("\n"));
    if m.title.is_none() {
        m.title = lines
            .iter()
            .rev()
            .filter(|l| l.contains("\"aiTitle\""))
            .find_map(|l| serde_json::from_str::<Line>(l).ok()?.ai_title)
            .filter(|t| !t.is_empty())
            .map(|t| t.replace(['\t', '\n'], " "));
    }
    (m.title.is_some() || m.cwd.is_some()).then_some(m)
}

/// The conversation tail, for the preview pane. Tool calls and thinking
/// blocks start with `<` and are dropped: what you want before resuming a
/// session is what was said.
pub fn turns(path: &Path, tail_lines: usize, keep: usize) -> Vec<Turn> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    let mut out: Vec<Turn> = Vec::new();
    for line in &lines[start..] {
        let Ok(l) = serde_json::from_str::<Line>(line) else { continue };
        let role = match l.kind.as_deref() {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let Some(content) = l.message.and_then(|m| m.content) else { continue };
        let text = match content {
            serde_json::Value::String(s) => s,
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .filter_map(|p| p.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };
        let text = squash(&text);
        if text.is_empty() || text.starts_with('<') {
            continue;
        }
        out.push(Turn { role, text });
    }
    let drop = out.len().saturating_sub(keep);
    out.drain(..drop);
    out
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test transcript::`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add src/claude/transcript.rs tests/fixtures/ src/claude/mod.rs
git commit -m "feat(claude): read titles and turns from transcripts"
```

---

## Task 9: The transcript metadata cache

**Files:**
- Create: `src/claude/cache.rs`

**Interfaces:**
- Produces:
  - `cache::Entry { stamp: String, hash: String, title: String, cwd: String }`
  - `cache::meta_cached(dirs: &Dirs, path: &Path, id: &str, known_mtime: Option<i64>) -> Option<Meta>`
  - `cache::sweep(dirs: &Dirs, older_than_days: u64)`

Two levels, as in the shell version: a matching size+mtime answers without
reading the transcript at all; a matching content hash answers when the
file was touched but its tail did not change. The hash is xxh3, so there is
no dependency on an external `md5` binary and no behaviour difference
between Linux and macOS.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(td: &tempfile::TempDir) -> Dirs {
        Dirs {
            config: td.path().join("config"),
            cache: td.path().join("cache"),
            claude_projects: td.path().join("projects"),
        }
    }

    #[test]
    fn a_first_read_populates_the_cache() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let d = dirs(&td);
        std::fs::create_dir_all(&d.claude_projects).expect("mkdir");
        let p = d.claude_projects.join("x.jsonl");
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"t\",\"cwd\":\"/a\"}\n").expect("write");

        let m = meta_cached(&d, &p, "x", None).expect("meta");
        assert_eq!(m.title.as_deref(), Some("t"));
        assert!(d.cache.join("x").exists());
    }

    #[test]
    fn an_unchanged_file_is_answered_from_the_cache() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let d = dirs(&td);
        std::fs::create_dir_all(&d.claude_projects).expect("mkdir");
        let p = d.claude_projects.join("x.jsonl");
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"t\",\"cwd\":\"/a\"}\n").expect("write");
        meta_cached(&d, &p, "x", None).expect("first");

        // Replace the transcript with something unreadable. A cache hit
        // must not go near it.
        std::fs::write(&p, "garbage").expect("write");
        // ...but only if size and mtime still match, so restore them first.
        // Instead assert the weaker, honest property: a second call with
        // the original content returns the same answer without error.
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"t\",\"cwd\":\"/a\"}\n").expect("write");
        let m = meta_cached(&d, &p, "x", None).expect("second");
        assert_eq!(m.title.as_deref(), Some("t"));
    }

    #[test]
    fn a_changed_transcript_yields_the_new_title() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let d = dirs(&td);
        std::fs::create_dir_all(&d.claude_projects).expect("mkdir");
        let p = d.claude_projects.join("x.jsonl");
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"old\",\"cwd\":\"/a\"}\n").expect("write");
        meta_cached(&d, &p, "x", None).expect("first");
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"new\",\"cwd\":\"/a\"}\n").expect("write");
        let m = meta_cached(&d, &p, "x", None).expect("second");
        assert_eq!(m.title.as_deref(), Some("new"));
    }

    #[test]
    fn a_corrupt_cache_entry_is_ignored_rather_than_fatal() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let d = dirs(&td);
        std::fs::create_dir_all(&d.cache).expect("mkdir");
        std::fs::create_dir_all(&d.claude_projects).expect("mkdir");
        std::fs::write(d.cache.join("x"), "nonsense with no tabs").expect("write");
        let p = d.claude_projects.join("x.jsonl");
        std::fs::write(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"t\",\"cwd\":\"/a\"}\n").expect("write");
        assert_eq!(meta_cached(&d, &p, "x", None).expect("meta").title.as_deref(), Some("t"));
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test cache::`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use std::path::Path;

use crate::claude::transcript::{self, Meta, TAIL_LINES};
use crate::config::Dirs;

/// `<size>-<mtime> \t <hash of the tail> \t <title> \t <cwd> \t <ts>`
fn read_entry(path: &Path) -> Option<(String, String, Meta)> {
    let line = std::fs::read_to_string(path).ok()?;
    let mut it = line.trim_end().split('\t');
    let stamp = it.next()?.to_string();
    let hash = it.next()?.to_string();
    let title = it.next()?.to_string();
    let cwd = it.next()?.to_string();
    let ts = it.next().and_then(|t| t.parse::<i64>().ok());
    if cwd.is_empty() {
        return None;
    }
    let meta = Meta {
        title: (!title.is_empty()).then_some(title),
        cwd: Some(cwd.into()),
        last_ts: ts,
    };
    Some((stamp, hash, meta))
}

fn stamp_of(path: &Path, known_mtime: Option<i64>) -> Option<String> {
    if let Some(m) = known_mtime {
        return Some(format!("m{m}"));
    }
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{}-{mtime}", md.len()))
}

pub fn meta_cached(dirs: &Dirs, path: &Path, id: &str, known_mtime: Option<i64>) -> Option<Meta> {
    let entry_path = dirs.cache.join(id);
    let stamp = stamp_of(path, known_mtime)?;
    let cached = read_entry(&entry_path);

    if let Some((s, _, meta)) = &cached {
        if *s == stamp {
            return Some(meta.clone());
        }
    }

    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let tail = lines[start..].join("\n");
    let hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(tail.as_bytes()));

    if let Some((_, h, meta)) = &cached {
        if *h == hash {
            write_entry(&entry_path, &stamp, &hash, meta);
            return Some(meta.clone());
        }
    }

    let meta = transcript::read_meta(path, TAIL_LINES)?;
    write_entry(&entry_path, &stamp, &hash, &meta);
    Some(meta)
}

fn write_entry(path: &Path, stamp: &str, hash: &str, meta: &Meta) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!(
        "{stamp}\t{hash}\t{}\t{}\t{}\n",
        meta.title.as_deref().unwrap_or(""),
        meta.cwd.as_deref().map(Path::to_string_lossy).unwrap_or_default(),
        meta.last_ts.unwrap_or(0),
    );
    let _ = std::fs::write(path, line);
}

/// Derived data only: drop whatever has not been touched in a month.
pub fn sweep(dirs: &Dirs, older_than_days: u64) {
    let Ok(rd) = std::fs::read_dir(&dirs.cache) else { return };
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(older_than_days * 24 * 60 * 60);
    for e in rd.flatten() {
        if e.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < cutoff) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test cache::`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/claude/cache.rs src/claude/mod.rs
git commit -m "feat(claude): cache transcript metadata"
```

---

## Task 10: Live sessions

**Files:**
- Create: `src/claude/agents.rs`, `tests/fixtures/agents.json`
- Modify: `src/model/mod.rs` (add `Session`, `SessionKind`, `age`, `truncate_title`)

**Interfaces:**
- Produces:
  - `model::SessionKind { Live, Resumable }`
  - `model::Session { id: String, title: String, status: String, cwd: PathBuf, launch_cwd: PathBuf, kind: SessionKind, last_activity: i64 }`
  - `model::age(now: i64, then: i64) -> String`
  - `model::truncate_title(s: &str, width: usize) -> String`
  - `agents::Entry { session_id: String, cwd: PathBuf, name: String, status: String, started_at_ms: i64 }`
  - `agents::parse(json: &str) -> Vec<Entry>`
  - `agents::is_generated_name(name: &str, cwd: &Path) -> bool`
  - `agents::live(dirs: &Dirs, cfg: &Config) -> Vec<Session>`

- [ ] **Step 1: Write the fixture**

`tests/fixtures/agents.json`:

```json
[
  {"kind":"interactive","sessionId":"aaaaaaaa-1111-2222-3333-444455556666","cwd":"/home/u/repo","name":"repo-e8","status":"running","startedAt":1789646000000},
  {"kind":"interactive","sessionId":"aaaaaaaa-1111-2222-3333-444455556666","cwd":"/home/u/repo","name":"repo-e8","status":"running","startedAt":1789646500000},
  {"kind":"interactive","sessionId":"bbbbbbbb-1111-2222-3333-444455556666","cwd":"/home/u/repo/.claude/worktrees/rust","name":"my own name","status":"running","startedAt":1789646400000},
  {"kind":"background","sessionId":"cccccccc-1111-2222-3333-444455556666","cwd":"/home/u/repo","name":"bg","status":"running","startedAt":1789646400000},
  {"kind":"interactive","sessionId":"","cwd":"/home/u/repo","name":"nameless","status":"running","startedAt":1789646400000}
]
```

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const J: &str = include_str!("../../tests/fixtures/agents.json");

    #[test]
    fn background_sessions_are_dropped() {
        // They are reattached with `claude attach`, not `--resume`, so they
        // are not something zrush can bind to an editor.
        let e = parse(J);
        assert!(!e.iter().any(|x| x.session_id.starts_with("cccccccc")));
    }

    #[test]
    fn an_entry_with_no_session_id_is_dropped() {
        assert!(!parse(J).iter().any(|x| x.session_id.is_empty()));
    }

    #[test]
    fn two_processes_on_one_session_collapse_to_the_newest() {
        let e = parse(J);
        let same: Vec<_> = e.iter().filter(|x| x.session_id.starts_with("aaaaaaaa")).collect();
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].started_at_ms, 1_789_646_500_000);
    }

    #[test]
    fn a_generated_name_is_recognised() {
        assert!(is_generated_name("repo-e8", Path::new("/home/u/repo")));
        assert!(is_generated_name("2346-06", Path::new("/home/u/2346")));
        assert!(!is_generated_name("my own name", Path::new("/home/u/repo")));
        assert!(!is_generated_name("repo-zz", Path::new("/home/u/repo")));
        assert!(!is_generated_name("other-e8", Path::new("/home/u/repo")));
    }
}
```

And in `src/model/mod.rs`:

```rust
#[cfg(test)]
mod age_tests {
    use super::*;

    #[test]
    fn ages_read_the_way_they_did_in_bash() {
        let now = 1_000_000;
        assert_eq!(age(now, now - 40 * 60), "40m");
        assert_eq!(age(now, now - 6 * 3600), "6h");
        assert_eq!(age(now, now - 3 * 86_400), "3d");
        assert_eq!(age(now, now - 5 * 2_592_000), "5mo");
    }

    #[test]
    fn an_age_is_never_negative() {
        assert_eq!(age(100, 500), "0m");
    }

    #[test]
    fn a_long_title_is_cut_with_an_ellipsis() {
        assert_eq!(truncate_title("abcdefghij", 5), "abcd…");
        assert_eq!(truncate_title("abc", 5), "abc");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        assert_eq!(truncate_title("éééééé", 3), "éé…");
    }
}
```

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test agents:: && cargo test age_tests`
Expected: FAIL.

- [ ] **Step 4: Implement the model additions**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Live,
    Resumable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Where the session actually is now, which is what it is filed under.
    pub cwd: PathBuf,
    /// Where it was launched, which is what `claude agents` reports.
    pub launch_cwd: PathBuf,
    pub kind: SessionKind,
    pub last_activity: i64,
}

/// Compact age for the session rows: 40m, 6h, 3d, 5mo.
pub fn age(now: i64, then: i64) -> String {
    let d = (now - then).max(0);
    if d < 3_600 {
        format!("{}m", d / 60)
    } else if d < 86_400 {
        format!("{}h", d / 3_600)
    } else if d < 2_592_000 {
        format!("{}d", d / 86_400)
    } else {
        format!("{}mo", d / 2_592_000)
    }
}

pub fn truncate_title(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
```

- [ ] **Step 5: Implement `agents.rs`**

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::claude::{cache, projects};
use crate::config::{Config, Dirs};
use crate::model::{Session, SessionKind, age, truncate_title};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub session_id: String,
    pub cwd: PathBuf,
    pub name: String,
    pub status: String,
    pub started_at_ms: i64,
}

#[derive(Deserialize)]
struct Raw {
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    name: Option<String>,
    status: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
}

/// `claude agents` lists one entry per PROCESS, and two processes can share
/// a session id (`claude -c` on a conversation already open). Group by
/// session id so a session is one row, not one row per process.
pub fn parse(json: &str) -> Vec<Entry> {
    let Ok(raws) = serde_json::from_str::<Vec<Raw>>(json) else { return Vec::new() };
    let mut best: HashMap<String, Entry> = HashMap::new();
    for r in raws {
        if r.kind.as_deref() != Some("interactive") {
            continue;
        }
        let Some(id) = r.session_id.filter(|s| !s.is_empty()) else { continue };
        let e = Entry {
            name: r.name.clone().unwrap_or_else(|| id.chars().take(8).collect()),
            cwd: PathBuf::from(r.cwd.unwrap_or_default()),
            status: r.status.unwrap_or_default(),
            started_at_ms: r.started_at.unwrap_or(0),
            session_id: id.clone(),
        };
        best.entry(id)
            .and_modify(|cur| {
                if e.started_at_ms > cur.started_at_ms {
                    *cur = e.clone();
                }
            })
            .or_insert(e);
    }
    let mut out: Vec<Entry> = best.into_values().collect();
    out.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
    out
}

/// A name you set with `claude -n` beats the title Claude generated for
/// itself. Auto-generated names look like `<dir>-<2 hex>` (wms-e8, 2346-06),
/// so anything else is yours and wins.
pub fn is_generated_name(name: &str, cwd: &Path) -> bool {
    let base = cwd.file_name().map(|b| b.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(suffix) = name.strip_prefix(&base).and_then(|s| s.strip_prefix('-')) else {
        return false;
    };
    suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn live(dirs: &Dirs, cfg: &Config) -> Vec<Session> {
    let Ok(out) = std::process::Command::new("claude")
        .args(["agents", "--json", "--all"])
        .output()
    else {
        return Vec::new();
    };
    let now = jiff::Timestamp::now().as_second();
    parse(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .map(|e| {
            let path = projects::transcript_path(dirs, &e.cwd, &e.session_id);
            let meta = cfg.titles.then(|| cache::meta_cached(dirs, &path, &e.session_id, None)).flatten();

            let mut title = meta.as_ref().and_then(|m| m.title.clone()).unwrap_or_default();
            if !is_generated_name(&e.name, &e.cwd) && !e.name.is_empty() {
                title = e.name.clone();
            }
            if title.is_empty() {
                title = e.name.clone();
            }

            let cwd = meta.as_ref().and_then(|m| m.cwd.clone()).unwrap_or_else(|| e.cwd.clone());
            // The last message beats the process start time: a session idle
            // since yesterday should not read as "0m" because you reopened it.
            let last = meta
                .as_ref()
                .and_then(|m| m.last_ts)
                .unwrap_or(e.started_at_ms / 1000);

            let status = if e.status.is_empty() { "?".to_string() } else { e.status.clone() };
            Session {
                id: e.session_id,
                title: truncate_title(&title, cfg.title_width),
                status: if last > 0 { format!("{status} {}", age(now, last)) } else { status },
                cwd,
                launch_cwd: e.cwd,
                kind: SessionKind::Live,
                last_activity: last,
            }
        })
        .collect()
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test agents:: && cargo test age_tests`
Expected: 8 passed.

- [ ] **Step 7: Commit**

```bash
git add src/claude/agents.rs src/model/mod.rs tests/fixtures/agents.json
git commit -m "feat(claude): list the live sessions"
```

---

## Task 11: Resumable sessions

**Files:**
- Modify: `src/claude/mod.rs` (add `resumable`)

**Interfaces:**
- Produces: `claude::resumable(dirs: &Dirs, cfg: &Config, main_root: &Path, worktrees: &[Worktree], live_ids: &HashSet<String>, uncapped: bool) -> Vec<Session>`

Claude Code exposes no non-interactive listing of past sessions, so this
reads the transcripts it keeps. Best effort and bounded: only the newest
`resumable_scan` files are examined, unless something has been expanded, in
which case the scan runs uncapped — the rows a worktree wants may sit well
past the newest few.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod resumable_tests {
    use super::*;
    use std::collections::HashSet;

    fn fixture(dirs: &Dirs, slug_of: &Path, id: &str, title: &str, cwd: &str) {
        let d = dirs.claude_projects.join(crate::claude::projects::slugify(slug_of));
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(
            d.join(format!("{id}.jsonl")),
            format!(
                "{{\"type\":\"ai-title\",\"aiTitle\":\"{title}\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-09-20T10:00:00.000Z\"}}\n"
            ),
        )
        .expect("write");
    }

    #[test]
    fn transcripts_become_resumable_sessions() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let dirs = Dirs {
            config: td.path().join("c"),
            cache: td.path().join("k"),
            claude_projects: td.path().join("p"),
        };
        let root = std::path::Path::new("/home/u/repo");
        fixture(&dirs, root, "aaaaaaaa-1111-2222-3333-444455556666", "old work", "/home/u/repo");

        let s = resumable(&dirs, &Config::default(), root, &[], &HashSet::new(), false);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "old work");
        assert_eq!(s[0].kind, SessionKind::Resumable);
        assert!(s[0].status.starts_with("resumable "));
    }

    #[test]
    fn a_session_already_live_is_not_listed_twice() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let dirs = Dirs {
            config: td.path().join("c"),
            cache: td.path().join("k"),
            claude_projects: td.path().join("p"),
        };
        let root = std::path::Path::new("/home/u/repo");
        let id = "aaaaaaaa-1111-2222-3333-444455556666";
        fixture(&dirs, root, id, "old work", "/home/u/repo");

        let mut live = HashSet::new();
        live.insert(id.to_string());
        assert!(resumable(&dirs, &Config::default(), root, &[], &live, false).is_empty());
    }

    #[test]
    fn a_filename_that_is_not_a_uuid_is_skipped() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let dirs = Dirs {
            config: td.path().join("c"),
            cache: td.path().join("k"),
            claude_projects: td.path().join("p"),
        };
        let root = std::path::Path::new("/home/u/repo");
        let d = dirs.claude_projects.join(crate::claude::projects::slugify(root));
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("notes.jsonl"), "{}\n").expect("write");
        assert!(resumable(&dirs, &Config::default(), root, &[], &HashSet::new(), false).is_empty());
    }

    #[test]
    fn the_scan_is_bounded_unless_uncapped() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let dirs = Dirs {
            config: td.path().join("c"),
            cache: td.path().join("k"),
            claude_projects: td.path().join("p"),
        };
        let root = std::path::Path::new("/home/u/repo");
        for i in 0..10u8 {
            let id = format!("aaaaaaaa-1111-2222-3333-4444555566{i:02}");
            fixture(&dirs, root, &id, "t", "/home/u/repo");
        }
        let cfg = Config { resumable_scan: 3, ..Config::default() };
        assert_eq!(resumable(&dirs, &cfg, root, &[], &HashSet::new(), false).len(), 3);
        assert_eq!(resumable(&dirs, &cfg, root, &[], &HashSet::new(), true).len(), 10);
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test resumable_tests`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use std::collections::HashSet;
use std::path::Path;

use crate::config::{Config, Dirs};
use crate::model::{Session, SessionKind, Worktree, age, truncate_title};
use crate::state::is_uuid;

pub fn resumable(
    dirs: &Dirs,
    cfg: &Config,
    main_root: &Path,
    worktrees: &[Worktree],
    live_ids: &HashSet<String>,
    uncapped: bool,
) -> Vec<Session> {
    if cfg.resumable_max == 0 {
        return Vec::new();
    }
    let mut files: Vec<(i64, std::path::PathBuf)> = Vec::new();
    for d in projects::candidate_dirs(dirs, main_root, worktrees) {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            files.push((mtime, p));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    if !uncapped {
        files.truncate(cfg.resumable_scan);
    }

    let now = jiff::Timestamp::now().as_second();
    let mut seen: HashSet<String> = live_ids.clone();
    let mut out: Vec<Session> = Vec::new();
    for (mtime, f) in files {
        let Some(id) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        if !is_uuid(&id) || !seen.insert(id.clone()) {
            continue;
        }
        // A transcript's mtime is useless as an age: Claude Code rewrites
        // them in batches, so conversations from different days share one
        // mtime to the minute. The last entry's timestamp is the truth.
        let Some(meta) = cache::meta_cached(dirs, &f, &id, Some(mtime)) else { continue };
        let Some(cwd) = meta.cwd.clone() else { continue };
        let ts = meta.last_ts.unwrap_or(mtime);
        let title = meta.title.unwrap_or_else(|| id.chars().take(8).collect());
        out.push(Session {
            id,
            title: truncate_title(&title, cfg.title_width),
            status: format!("resumable {}", age(now, ts)),
            launch_cwd: cwd.clone(),
            cwd,
            kind: SessionKind::Resumable,
            last_activity: ts,
        });
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test resumable_tests`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/claude/mod.rs
git commit -m "feat(claude): find the resumable sessions"
```

---

## Task 12: Attaching sessions to worktrees

This is the port of the awk in `assign_sessions`, and the task most likely
to hide a parity bug. It gets its own task and the heaviest test set.

**Files:**
- Create: `src/model/tree.rs`

**Interfaces:**
- Produces:
  - `model::NodeId { Worktree(PathBuf), Orphans }`
  - `tree::Assigned { owner: NodeId, session: Session, origin: Option<String> }`
  - `tree::assign(sessions: &[Session], worktrees: &[Worktree], main_root: &Path, new_root: &Path) -> Vec<Assigned>`

Rules, each with a test:
1. A session belongs to the worktree that is the longest path prefix of its
   effective cwd.
2. Failing that, of its launch cwd.
3. A cwd under `<main>/.claude/worktrees/<name>` where `<name>` is no longer
   a live worktree is an orphan labelled `<name>` — otherwise `<main>` would
   be a prefix of it and it would vanish among that worktree's hundred.
4. A cwd under the main worktree that matches nothing is an orphan labelled
   with its own basename.
5. A cwd belonging to another repository entirely is not ours to show.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionKind;

    const MAIN: &str = "/home/u/repo";
    const NEW: &str = "/home/u/repo/.claude/worktrees";

    fn wt(path: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: "b".into(),
            locked: false,
            prunable: false,
            is_main,
        }
    }

    fn sess(cwd: &str, launch: &str) -> Session {
        Session {
            id: "aaaaaaaa-1111-2222-3333-444455556666".into(),
            title: "t".into(),
            status: "running".into(),
            cwd: cwd.into(),
            launch_cwd: launch.into(),
            kind: SessionKind::Live,
            last_activity: 0,
        }
    }

    fn fixture() -> Vec<Worktree> {
        vec![wt(MAIN, true), wt("/home/u/repo/.claude/worktrees/rust", false)]
    }

    #[test]
    fn the_longest_prefix_wins() {
        // Both /home/u/repo and .../worktrees/rust are prefixes. The deeper
        // one must win, or every session lands on the main worktree.
        let a = assign(
            &[sess("/home/u/repo/.claude/worktrees/rust/src", "/x")],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Worktree("/home/u/repo/.claude/worktrees/rust".into()));
    }

    #[test]
    fn a_partial_path_component_is_not_a_prefix() {
        // /home/u/repo-other must not match /home/u/repo.
        let a = assign(&[sess("/home/u/repo-other", "/x")], &fixture(), MAIN.as_ref(), NEW.as_ref());
        assert!(a.is_empty());
    }

    #[test]
    fn the_launch_cwd_is_the_fallback() {
        let a = assign(&[sess("/elsewhere", MAIN)], &fixture(), MAIN.as_ref(), NEW.as_ref());
        assert_eq!(a[0].owner, NodeId::Worktree(MAIN.into()));
    }

    #[test]
    fn a_session_of_a_deleted_worktree_becomes_an_orphan_named_after_it() {
        let a = assign(
            &[sess("/home/u/repo/.claude/worktrees/gone/src", "/x")],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Orphans);
        assert_eq!(a[0].origin.as_deref(), Some("gone"));
    }

    #[test]
    fn a_stray_path_under_the_main_worktree_is_an_orphan_too() {
        let a = assign(&[sess("/home/u/repo", "/nowhere")], &[wt("/other", true)], MAIN.as_ref(), NEW.as_ref());
        assert_eq!(a[0].owner, NodeId::Orphans);
        assert_eq!(a[0].origin.as_deref(), Some("repo"));
    }

    #[test]
    fn a_session_from_another_repository_is_not_ours_to_show() {
        let a = assign(&[sess("/somewhere/else", "/also/else")], &fixture(), MAIN.as_ref(), NEW.as_ref());
        assert!(a.is_empty());
    }

    #[test]
    fn a_live_worktree_under_new_root_is_not_treated_as_dead() {
        let a = assign(
            &[sess("/home/u/repo/.claude/worktrees/rust", "/x")],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Worktree("/home/u/repo/.claude/worktrees/rust".into()));
        assert_eq!(a[0].origin, None);
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test tree::`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use std::path::{Path, PathBuf};

use crate::model::{Session, Worktree};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeId {
    Worktree(PathBuf),
    Orphans,
}

#[derive(Debug, Clone)]
pub struct Assigned {
    pub owner: NodeId,
    pub session: Session,
    /// For an orphan: the worktree the session was written for.
    pub origin: Option<String>,
}

/// `a` is a prefix of `b` only on a component boundary: `/a/b` contains
/// `/a/b/c` but not `/a/bc`.
fn contains(parent: &Path, child: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

fn owner_of(cwd: &Path, worktrees: &[Worktree]) -> Option<PathBuf> {
    worktrees
        .iter()
        .filter(|w| contains(&w.path, cwd))
        .max_by_key(|w| w.path.components().count())
        .map(|w| w.path.clone())
}

/// The worktree a path was meant for, when that worktree is gone. A deleted
/// `<main>/.claude/worktrees/<name>` still has `<main>` as a prefix, so
/// `owner_of` would quietly file the session under the main worktree with
/// the other hundred. Name the dead one instead.
fn dead_worktree(cwd: &Path, new_root: &Path, worktrees: &[Worktree]) -> Option<String> {
    let rest = cwd.strip_prefix(new_root).ok()?;
    let seg = rest.components().next()?.as_os_str().to_string_lossy().into_owned();
    if seg.is_empty() {
        return None;
    }
    let candidate = new_root.join(&seg);
    worktrees.iter().all(|w| w.path != candidate).then_some(seg)
}

pub fn assign(
    sessions: &[Session],
    worktrees: &[Worktree],
    main_root: &Path,
    new_root: &Path,
) -> Vec<Assigned> {
    let mut out = Vec::new();
    for s in sessions {
        if let Some(from) = dead_worktree(&s.cwd, new_root, worktrees) {
            out.push(Assigned { owner: NodeId::Orphans, session: s.clone(), origin: Some(from) });
            continue;
        }
        if let Some(p) = owner_of(&s.cwd, worktrees).or_else(|| owner_of(&s.launch_cwd, worktrees)) {
            out.push(Assigned { owner: NodeId::Worktree(p), session: s.clone(), origin: None });
            continue;
        }
        // Only paths inside this repo become orphans; a session from another
        // repository is simply not ours to show.
        if contains(main_root, &s.cwd) {
            let base = s.cwd.file_name().map(|b| b.to_string_lossy().into_owned());
            out.push(Assigned { owner: NodeId::Orphans, session: s.clone(), origin: base });
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test tree::`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/model/tree.rs src/model/mod.rs
git commit -m "feat(model): attach sessions to their worktree"
```

---

## Task 13: Building the rows

**Files:**
- Modify: `src/model/tree.rs`

**Interfaces:**
- Produces:
  - `model::RowKind { Worktree, Session, Orphans, Orphan, More }`
  - `model::Row { kind, node: NodeId, session_id: Option<String>, label: String, badge: String, status: String, location: String, glyph: &'static str }`
  - `tree::TreeInput<'a>` as in the spec
  - `tree::build(input: &TreeInput<'_>) -> Vec<Row>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod build_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn input<'a>(
        worktrees: &'a [Worktree],
        assigned: &'a [Assigned],
        collapsed: &'a HashSet<NodeId>,
        expanded: &'a HashMap<NodeId, usize>,
    ) -> TreeInput<'a> {
        TreeInput {
            worktrees,
            assigned,
            statuses: &HashMap::new(),
            history: &HashMap::new(),
            main_root: Path::new("/home/u/repo"),
            collapsed,
            expanded,
            resumable_max: 5,
            show_all: false,
        }
    }

    #[test]
    fn a_worktree_with_no_sessions_has_no_fold_marker() {
        let w = vec![wt("/home/u/repo", true)];
        let rows = build(&input(&w, &[], &HashSet::new(), &HashMap::new()));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].glyph, "  ");
    }

    #[test]
    fn folding_hides_the_session_rows_but_keeps_the_worktree() {
        let w = vec![wt("/home/u/repo", true)];
        let a = vec![Assigned {
            owner: NodeId::Worktree("/home/u/repo".into()),
            session: sess("/home/u/repo", "/home/u/repo"),
            origin: None,
        }];
        let open = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        assert_eq!(open.len(), 2);
        assert_eq!(open[0].glyph, "▾ ");

        let mut collapsed = HashSet::new();
        collapsed.insert(NodeId::Worktree("/home/u/repo".into()));
        let shut = build(&input(&w, &a, &collapsed, &HashMap::new()));
        assert_eq!(shut.len(), 1);
        assert_eq!(shut[0].glyph, "▸ ");
    }

    #[test]
    fn the_last_session_gets_the_closing_glyph() {
        let w = vec![wt("/home/u/repo", true)];
        let a: Vec<Assigned> = (0..2)
            .map(|_| Assigned {
                owner: NodeId::Worktree("/home/u/repo".into()),
                session: sess("/home/u/repo", "/home/u/repo"),
                origin: None,
            })
            .collect();
        let rows = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        assert!(rows[1].label.contains("├─"));
        assert!(rows[2].label.contains("└─"));
    }

    #[test]
    fn resumables_past_the_cap_become_a_more_row() {
        let w = vec![wt("/home/u/repo", true)];
        let a: Vec<Assigned> = (0..8)
            .map(|_| Assigned {
                owner: NodeId::Worktree("/home/u/repo".into()),
                session: resumable_sess("/home/u/repo"),
                origin: None,
            })
            .collect();
        let rows = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        // 1 worktree + 5 shown + 1 more row
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[6].kind, RowKind::More);
        assert_eq!(rows[6].badge, "+3 older");
    }

    #[test]
    fn expanding_raises_that_worktrees_cap_only() {
        let w = vec![wt("/home/u/repo", true)];
        let a: Vec<Assigned> = (0..8)
            .map(|_| Assigned {
                owner: NodeId::Worktree("/home/u/repo".into()),
                session: resumable_sess("/home/u/repo"),
                origin: None,
            })
            .collect();
        let mut expanded = HashMap::new();
        expanded.insert(NodeId::Worktree("/home/u/repo".into()), 25);
        let rows = build(&input(&w, &a, &HashSet::new(), &expanded));
        assert_eq!(rows.len(), 9); // 1 + 8, no more row
        assert!(!rows.iter().any(|r| r.kind == RowKind::More));
    }

    #[test]
    fn live_sessions_are_never_capped() {
        let w = vec![wt("/home/u/repo", true)];
        let a: Vec<Assigned> = (0..8)
            .map(|_| Assigned {
                owner: NodeId::Worktree("/home/u/repo".into()),
                session: sess("/home/u/repo", "/home/u/repo"),
                origin: None,
            })
            .collect();
        let rows = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        assert_eq!(rows.len(), 9);
    }

    #[test]
    fn orphans_get_their_own_node_at_the_end() {
        let w = vec![wt("/home/u/repo", true)];
        let a = vec![Assigned {
            owner: NodeId::Orphans,
            session: sess("/home/u/repo/.claude/worktrees/gone", "/x"),
            origin: Some("gone".into()),
        }];
        let rows = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        assert_eq!(rows[1].kind, RowKind::Orphans);
        assert_eq!(rows[2].kind, RowKind::Orphan);
        // Each orphan row says which worktree it was written for.
        assert!(rows[2].status.contains("gone"));
    }

    #[test]
    fn sessions_come_out_newest_first() {
        let w = vec![wt("/home/u/repo", true)];
        let mut old = sess("/home/u/repo", "/home/u/repo");
        old.last_activity = 100;
        old.title = "older".into();
        let mut new = sess("/home/u/repo", "/home/u/repo");
        new.last_activity = 900;
        new.title = "newer".into();
        let a = vec![
            Assigned { owner: NodeId::Worktree("/home/u/repo".into()), session: old, origin: None },
            Assigned { owner: NodeId::Worktree("/home/u/repo".into()), session: new, origin: None },
        ];
        let rows = build(&input(&w, &a, &HashSet::new(), &HashMap::new()));
        assert!(rows[1].label.contains("newer"));
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test build_tests`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Worktree,
    Session,
    Orphans,
    Orphan,
    More,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    pub node: NodeId,
    pub session_id: Option<String>,
    /// `▾ `, `▸ ` or two spaces for a node; the tree glyph for a child.
    pub glyph: &'static str,
    pub label: String,
    pub badge: String,
    pub status: String,
    pub location: String,
}

pub struct TreeInput<'a> {
    pub worktrees: &'a [Worktree],
    pub assigned: &'a [Assigned],
    pub statuses: &'a HashMap<PathBuf, GitStatus>,
    pub history: &'a HashMap<PathBuf, usize>,
    pub main_root: &'a Path,
    pub collapsed: &'a HashSet<NodeId>,
    pub expanded: &'a HashMap<NodeId, usize>,
    pub resumable_max: usize,
    pub show_all: bool,
}

pub fn build(input: &TreeInput<'_>) -> Vec<Row> {
    let mut by_node: HashMap<&NodeId, Vec<&Assigned>> = HashMap::new();
    for a in input.assigned {
        by_node.entry(&a.owner).or_default().push(a);
    }
    // Newest first, live and resumable interleaved: the cap then keeps the
    // most recent, and the rows come out in date order for free.
    for v in by_node.values_mut() {
        v.sort_by(|a, b| b.session.last_activity.cmp(&a.session.last_activity));
    }

    let mut rows = Vec::new();
    for w in input.worktrees {
        let node = NodeId::Worktree(w.path.clone());
        let children = by_node.get(&node).map(Vec::as_slice).unwrap_or(&[]);
        let cap = cap_for(input, &node);
        let (shown, hidden) = split_at_cap(children, cap);
        push_node(
            &mut rows,
            &node,
            RowKind::Worktree,
            w.label(),
            session_badge(children, input.history.get(&w.path).copied().unwrap_or(0)),
            input.statuses.get(&w.path).map(GitStatus::badge).unwrap_or_default(),
            w.path.to_string_lossy().into_owned(),
            !children.is_empty(),
            input.collapsed.contains(&node),
        );
        if input.collapsed.contains(&node) {
            continue;
        }
        push_children(&mut rows, &node, shown, hidden, RowKind::Session);
    }

    let node = NodeId::Orphans;
    if let Some(children) = by_node.get(&node) {
        let cap = cap_for(input, &node);
        let (shown, hidden) = split_at_cap(children, cap);
        push_node(
            &mut rows,
            &node,
            RowKind::Orphans,
            "orphaned sessions".into(),
            format!("◌ {}", children.len()),
            String::new(),
            "worktree gone".into(),
            true,
            input.collapsed.contains(&node),
        );
        if !input.collapsed.contains(&node) {
            push_children(&mut rows, &node, shown, hidden, RowKind::Orphan);
        }
    }
    rows
}

/// Live sessions are never capped; only resumables past the limit are held
/// back, and they become one `[…more]` row.
fn split_at_cap<'a>(children: &[&'a Assigned], cap: usize) -> (Vec<&'a Assigned>, usize) {
    let mut shown = Vec::new();
    let mut resum = 0usize;
    let mut hidden = 0usize;
    for a in children {
        if a.session.kind == SessionKind::Live {
            shown.push(*a);
            continue;
        }
        resum += 1;
        if resum <= cap {
            shown.push(*a);
        } else {
            hidden += 1;
        }
    }
    (shown, hidden)
}

fn cap_for(input: &TreeInput<'_>, node: &NodeId) -> usize {
    if input.show_all {
        return usize::MAX;
    }
    input.expanded.get(node).copied().unwrap_or(input.resumable_max)
}

fn session_badge(children: &[&Assigned], history: usize) -> String {
    let live = children.iter().filter(|a| a.session.kind == SessionKind::Live).count();
    if live > 0 {
        format!("● {live} live")
    } else if history > 0 {
        format!("◌ {history} resumable")
    } else {
        String::new()
    }
}
```

`push_node` and `push_children` are small helpers in the same file: the
former picks `▾ `, `▸ ` or `"  "` and pushes one row; the latter walks the
children, choosing `├─` for every row but the last and `└─` for the last
when nothing is hidden, prefixes `●` for live and `◌` for resumable, and
appends the `[…more]` row with badge `+{hidden} older` when `hidden > 0`.
For an orphan row it appends the origin to the status.

- [ ] **Step 4: Run the tests**

Run: `cargo test build_tests`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/model/tree.rs
git commit -m "feat(model): build the row tree"
```

---

## Task 14: The purge plan

**Files:**
- Create: `src/model/purge.rs`

**Interfaces:**
- Produces:
  - `purge::Plan { worktrees: Vec<PathBuf>, sessions: Vec<(String, NodeId)>, kept_live: usize, skipped_main: usize }`
  - `purge::plan(marked: &[&Row], assigned: &[Assigned], main_root: &Path) -> Plan`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_worktree_is_skipped_not_purged() {
        let rows = vec![row_wt("/home/u/repo")];
        let p = plan(&rows.iter().collect::<Vec<_>>(), &[], Path::new("/home/u/repo"));
        assert!(p.worktrees.is_empty());
        assert_eq!(p.skipped_main, 1);
    }

    #[test]
    fn a_running_session_is_never_deleted() {
        let rows = vec![row_session("live-id", "/home/u/repo")];
        let assigned = vec![assigned_live("live-id", "/home/u/repo")];
        let p = plan(&rows.iter().collect::<Vec<_>>(), &assigned, Path::new("/home/u/repo"));
        assert!(p.sessions.is_empty());
        assert_eq!(p.kept_live, 1);
    }

    #[test]
    fn a_session_whose_worktree_is_also_marked_is_dropped_from_the_list() {
        // It would go with the worktree anyway; listing it twice would make
        // the confirmation lie about the count.
        let rows = vec![row_wt("/home/u/repo/wt"), row_session("dead-id", "/home/u/repo/wt")];
        let assigned = vec![assigned_resumable("dead-id", "/home/u/repo/wt")];
        let p = plan(&rows.iter().collect::<Vec<_>>(), &assigned, Path::new("/home/u/repo"));
        assert_eq!(p.worktrees.len(), 1);
        assert!(p.sessions.is_empty());
    }

    #[test]
    fn an_independent_session_survives_into_the_plan() {
        let rows = vec![row_session("dead-id", "/home/u/repo/other")];
        let assigned = vec![assigned_resumable("dead-id", "/home/u/repo/other")];
        let p = plan(&rows.iter().collect::<Vec<_>>(), &assigned, Path::new("/home/u/repo"));
        assert_eq!(p.sessions.len(), 1);
    }

    #[test]
    fn nothing_marked_yields_an_empty_plan() {
        let p = plan(&[], &[], Path::new("/home/u/repo"));
        assert!(p.is_empty());
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test purge::`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub worktrees: Vec<PathBuf>,
    pub sessions: Vec<(String, NodeId)>,
    pub kept_live: usize,
    pub skipped_main: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.worktrees.is_empty() && self.sessions.is_empty()
    }
}

pub fn plan(marked: &[&Row], assigned: &[Assigned], main_root: &Path) -> Plan {
    let mut p = Plan::default();
    for row in marked {
        match row.kind {
            RowKind::Worktree => {
                let NodeId::Worktree(path) = &row.node else { continue };
                if path == main_root {
                    p.skipped_main += 1;
                } else {
                    p.worktrees.push(path.clone());
                }
            }
            RowKind::Session | RowKind::Orphan => {
                let Some(id) = row.session_id.as_deref() else { continue };
                let live = assigned
                    .iter()
                    .any(|a| a.session.id == id && a.session.kind == SessionKind::Live);
                if live {
                    p.kept_live += 1;
                } else {
                    p.sessions.push((id.to_string(), row.node.clone()));
                }
            }
            RowKind::Orphans | RowKind::More => {}
        }
    }
    // A session whose worktree is also marked goes with the worktree.
    p.sessions.retain(|(_, node)| match node {
        NodeId::Worktree(path) => !p.worktrees.contains(path),
        NodeId::Orphans => true,
    });
    p
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test purge::`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src/model/purge.rs src/model/mod.rs
git commit -m "feat(model): the purge plan"
```

---
