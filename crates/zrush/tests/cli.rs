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
use zrush_core::config::{Config, Dirs};

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
    let base = zrush_core::paths::real(dir.path());
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

    // A stub editor that logs instead of opening anything. Written for
    // whichever shell will run it: no test may reach a real editor.
    let log = base.join("log");
    let ed = stub_editor(&base, &log);

    // Written by the same code that writes a real one. Hand-rolled TOML got
    // this wrong on Windows, where a path is full of backslashes and a
    // double-quoted string treats those as escapes.
    let cfg = Config {
        editor_cmd: vec![ed.to_string_lossy().into_owned()],
        status: false,
        ..Config::default()
    };
    cfg.save(&Dirs {
        config: home.join(".config/zrush"),
        cache: home.join(".cache/zrush"),
        claude_projects: home.join(".claude/projects"),
    })
    .expect("config");

    Sandbox {
        _dir: dir,
        home,
        repo,
        log,
    }
}

/// Writes the stub and returns the path to invoke it by.
#[cfg(unix)]
fn stub_editor(base: &Path, log: &Path) -> PathBuf {
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
    ed
}

#[cfg(windows)]
fn stub_editor(base: &Path, log: &Path) -> PathBuf {
    let ed = base.join("ed.cmd");
    std::fs::write(&ed, format!("@echo EDITOR %1>>\"{}\"\r\n", log.display())).expect("write");
    ed
}

fn zrush(sb: &Sandbox) -> Command {
    let mut c = Command::cargo_bin("zrush").expect("binary");
    c.env("HOME", &sb.home)
        .env("XDG_CONFIG_HOME", sb.home.join(".config"))
        .env("XDG_CACHE_HOME", sb.home.join(".cache"))
        .current_dir(&sb.repo);
    c
}

/// `zrush tab` is what an editor's key binding runs. The editor is the
/// caller, so the pair it resolves to is the caller's business — and a
/// pair with no row must still work.
///
/// Nothing here requires the agent to be installed: the runners have no
/// `claude`, and an editor's own surface goes through the editor anyway.
#[test]
fn tab_resolves_the_pair_the_editor_asks_for() {
    let sb = sandbox();
    std::fs::write(
        sb.repo.join(".git/zrush-session"),
        "session_id=aaaaaaaa-1111-2222-3333-444455556666\nagent=claude\n",
    )
    .expect("write");

    // No row for zed: the agent's own command, in a terminal.
    zrush(&sb)
        .args(["tab", "--editor", "zed", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "terminal\tclaude --resume aaaaaaaa-1111-2222-3333-444455556666",
        ));

    // Cursor has a row, and it is not VS Code's: its own URL scheme.
    zrush(&sb)
        .args(["tab", "--editor", "cursor", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "uri\tcursor://anthropic.claude-code/open?session=aaaaaaaa-1111-2222-3333-444455556666",
        ));
}

#[test]
fn tab_with_no_binding_opens_a_fresh_conversation() {
    let sb = sandbox();
    zrush(&sb)
        .args(["tab", "--editor", "zed", "--print"])
        .assert()
        .success()
        // No id to resume: the agent's start command, not `--resume `.
        .stdout(predicate::str::contains("terminal\tclaude\n"));
}

#[test]
fn tab_outside_a_repository_says_so() {
    let sb = sandbox();
    zrush(&sb)
        .current_dir(sb.home.clone())
        .args(["tab", "--print"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git worktree"));
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

#[test]
fn no_sessions_and_an_agent_name_do_not_fight() {
    // -n empties the agent list, so a named agent has nothing left to
    // select. It must be dropped rather than fail to match an empty list.
    let sb = sandbox();
    zrush(&sb)
        .args(["--no-sessions", "--agent", "claude", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main [root]"));
}

#[test]
fn a_worktree_row_carries_its_path_for_a_shell_helper() {
    let sb = sandbox();
    let out = zrush(&sb).arg("--list").output().expect("run");
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text
        .lines()
        .next()
        .expect("a row")
        .split('\t')
        .nth(3)
        .expect("a path field");
    assert_eq!(Path::new(path), sb.repo.as_path());
}
