//! The binary, run for real, against throwaway repositories.
//!
//! Nothing here touches a real repository, the real `~/.claude` or a real
//! editor: `HOME` and the XDG directories point into a `TempDir`, and the
//! configured editor is a script that only writes to a log.
// A test that cannot expect is a test that hides its own failure.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

struct Sandbox {
    _dir: TempDir,
    home: PathBuf,
    repo: PathBuf,
    log: PathBuf,
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

fn sandbox() -> Sandbox {
    let dir = TempDir::new().expect("tempdir");
    // canonicalize: on macOS $TMPDIR is a symlink into /private, and git
    // reports the resolved path.
    let base = dir.path().canonicalize().expect("canonicalize");
    let home = base.join("home");
    let repo = base.join("repo");
    let remote = base.join("remote.git");
    std::fs::create_dir_all(home.join(".config/zrush")).expect("mkdir");

    git(
        &base,
        &["init", "-q", "--bare", remote.to_str().expect("utf8")],
    );
    git(
        &base,
        &[
            "clone",
            "-q",
            remote.to_str().expect("utf8"),
            repo.to_str().expect("utf8"),
        ],
    );
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "zrush tests"]);
    std::fs::write(repo.join("f.txt"), "seed").expect("write");
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-qm", "init"]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);

    // A stub editor that logs instead of opening anything.
    let log = base.join("log");
    let ed = base.join("ed");
    std::fs::write(
        &ed,
        format!(
            "#!/bin/sh\nprintf 'EDITOR %s\\n' \"$1\" >> {}\n",
            log.display()
        ),
    )
    .expect("write");
    let mut perms = std::fs::metadata(&ed).expect("stat").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&ed, perms).expect("chmod");

    std::fs::write(
        home.join(".config/zrush/config.toml"),
        format!("editor_cmd = [\"{}\"]\nstatus = false\n", ed.display()),
    )
    .expect("write");

    Sandbox {
        _dir: dir,
        home,
        repo,
        log,
    }
}

fn zrush(sb: &Sandbox) -> Command {
    let mut c = Command::cargo_bin("zrush").expect("binary");
    c.env("HOME", &sb.home)
        .env("XDG_CONFIG_HOME", sb.home.join(".config"))
        .env("XDG_CACHE_HOME", sb.home.join(".cache"))
        .current_dir(&sb.repo);
    c
}

#[test]
fn list_prints_the_main_worktree() {
    let sb = sandbox();
    zrush(&sb)
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("main [root]"));
}

#[test]
fn list_prints_every_worktree() {
    let sb = sandbox();
    git(
        &sb.repo,
        &[
            "worktree",
            "add",
            "-q",
            ".claude/worktrees/feature",
            "-b",
            "feature",
        ],
    );
    zrush(&sb)
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("main [root]").and(predicate::str::contains("feature")));
}

#[test]
fn the_repo_can_be_named_from_outside_it() {
    let sb = sandbox();
    let mut c = Command::cargo_bin("zrush").expect("binary");
    c.env("HOME", &sb.home)
        .env("XDG_CONFIG_HOME", sb.home.join(".config"))
        .current_dir(sb.home.as_path())
        .args(["--repo", sb.repo.to_str().expect("utf8"), "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main [root]"));
}

#[test]
fn outside_any_repo_it_says_so_rather_than_guessing() {
    let sb = sandbox();
    let mut c = Command::cargo_bin("zrush").expect("binary");
    c.env("HOME", &sb.home)
        .env("XDG_CONFIG_HOME", sb.home.join(".config"))
        .current_dir(sb.home.as_path())
        .arg("--list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git repository"));
}

#[test]
fn a_directory_that_is_not_one_is_refused() {
    let sb = sandbox();
    zrush(&sb)
        .args(["--repo", "/definitely/not/here", "--list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a directory"));
}

#[test]
fn an_unknown_agent_named_on_the_command_line_is_an_error() {
    let sb = sandbox();
    zrush(&sb)
        .args(["--agent", "no-such-agent", "--list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no such agent"));
}

#[test]
fn no_sessions_still_lists_the_worktrees() {
    let sb = sandbox();
    zrush(&sb)
        .args(["--no-sessions", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main [root]"));
}

#[test]
fn the_legacy_shell_config_is_imported_on_first_run() {
    let sb = sandbox();
    // Replace the TOML with the file the bash version sourced.
    std::fs::remove_file(sb.home.join(".config/zrush/config.toml")).expect("rm");
    std::fs::write(
        sb.home.join(".config/zrush/config"),
        "ZRUSH_TITLE_WIDTH=33\nZRUSH_STATUS=0\n",
    )
    .expect("write");

    zrush(&sb).arg("--list").assert().success();

    let toml = std::fs::read_to_string(sb.home.join(".config/zrush/config.toml")).expect("read");
    assert!(
        toml.contains("title_width = 33"),
        "the old setting survived: {toml}"
    );
}

#[test]
fn the_help_names_the_session_subcommand() {
    let sb = sandbox();
    zrush(&sb)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("session"));
}

#[test]
fn nothing_opened_an_editor_along_the_way() {
    let sb = sandbox();
    zrush(&sb).arg("--list").assert().success();
    assert!(!sb.log.exists(), "--list must have no side effect");
}
