# Changelog

All notable changes to the `zrush` command line. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

Releases up to 0.0.4 are tagged `vX.Y.Z`; later ones `cli-vX.Y.Z`, beside the
desktop app's own `gui-vX.Y.Z`.

## [Unreleased]

### Fixed

- A purge deletes the transcript of a session marked along with its
  worktree; it used to remove the worktree and leave the transcript.
- "Purge, and their other sessions too" deletes the marked worktrees' other
  sessions; it did the same as "Purge only what is marked".
- A worktree git refuses to remove keeps its sessions through a purge.
- On Windows, the desktop app no longer opens an empty console window for
  every git call it makes.
- On Windows, the default terminal runs the agent's script, and links
  open: both used to be handed to `cmd`, which ignored them.

## [0.0.4] - 2026-11-08

### Changed

- Building from source needs Rust 1.99 or later (was 1.88).
- Dependencies updated, among them `dirs` 7 and `toml` 1.2.
- The update check understands the `cli-vX.Y.Z` tags the next releases will
  carry, so this release keeps offering updates after the switch.
- `zrush --list` adds `· filed elsewhere` to the status of a session the agent
  would not find from the worktree it is listed under, as the interface does.

### Fixed

- Two zrush windows changing settings no longer overwrite each other's
  changes in `config.toml`.
- A setting coming from the environment (`ZRUSH_EDITOR`, `ZRUSH_AGENT`, …) is
  no longer written into `config.toml` when another setting is saved.
- `-S` / `--no-status` holds for the whole run instead of being saved into
  `config.toml` by the first settings change.
- `config.toml` is replaced in one step when saved, so a zrush reading it at
  that moment never sees it empty.

## [0.0.3] - 2026-11-07

- Offers the newer release at startup, and installs it from there.

[0.0.4]: https://github.com/christphe/zrush/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/christphe/zrush/releases/tag/v0.0.3
