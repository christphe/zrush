//! One smoke test on a real pseudo-terminal.
//!
//! The snapshot tests render through `TestBackend`, which proves the layout
//! but never touches a terminal. This proves the other half: that the
//! binary puts a real terminal into raw mode, draws, answers a key, and
//! gives the terminal back.
//!
//! Unix only: there is no pty to drive on Windows.
#![cfg(unix)]
// A test that cannot expect is a test that hides its own failure.
#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tempfile::TempDir;

fn git(cwd: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .expect("git");
    assert!(st.success(), "git {args:?} failed");
}

/// Read until `needle` shows up or the deadline passes.
fn wait_for(reader: &mut dyn Read, needle: &str, within: Duration) -> String {
    let deadline = Instant::now() + within;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match reader.read(&mut buf) {
            Ok(n) if n > 0 => {
                seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                if seen.contains(needle) {
                    return seen;
                }
            }
            // End of file, or the pty went away: nothing more is coming.
            Ok(_) | Err(_) => break,
        }
    }
    seen
}

#[test]
fn the_picker_boots_on_a_pty_draws_and_quits() {
    let td = TempDir::new().expect("tempdir");
    let base = td.path().canonicalize().expect("canonicalize");
    let home = base.join("home");
    let repo = base.join("repo");
    std::fs::create_dir_all(home.join(".config/zrush")).expect("mkdir");

    git(&base, &["init", "-q", repo.to_str().expect("utf8")]);
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "zrush tests"]);
    std::fs::write(repo.join("f.txt"), "seed").expect("write");
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-qm", "init"]);
    git(&repo, &["branch", "-M", "main"]);
    std::fs::write(home.join(".config/zrush/config.toml"), "status = false\n").expect("write");

    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_zrush"));
    cmd.cwd(&repo);
    cmd.env("HOME", &home);
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
    cmd.env("XDG_CACHE_HOME", home.join(".cache"));
    cmd.env("TERM", "xterm-256color");
    let mut child = pty.slave.spawn_command(cmd).expect("spawn");
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().expect("reader");
    // The first frame goes up before any probe has answered — it says
    // "loading…" and Worktrees(0). Waiting for the branch name proves both
    // halves: the terminal was taken over, and the data arrived into it.
    let screen = wait_for(&mut reader, "main", Duration::from_secs(20));
    assert!(
        screen.contains("Worktrees"),
        "never drew the list; saw:\n{screen}"
    );
    assert!(
        screen.contains("main"),
        "the worktree is missing from:\n{screen}"
    );

    // Keep draining. A pty has a finite buffer: stop reading and the next
    // write blocks, so the picker would hang on the escape sequences that
    // restore the terminal rather than exiting.
    std::thread::spawn(move || {
        let mut sink = [0u8; 4096];
        while reader.read(&mut sink).is_ok_and(|n| n > 0) {}
    });

    // ctrl-q rather than esc: a lone 0x1b is ambiguous on the wire, and a
    // terminal parser has to wait to see whether an escape sequence
    // follows. The esc path is covered by the keyboard tests, which do not
    // go through a parser at all.
    let mut writer = pty.master.take_writer().expect("writer");
    writer.write_all(&[0x11]).expect("write");
    writer.flush().expect("flush");

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "exited badly: {status:?}");
                return;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                panic!("the picker did not quit on esc");
            }
            Err(e) => panic!("wait failed: {e}"),
        }
    }
}
