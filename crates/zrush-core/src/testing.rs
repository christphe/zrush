//! Scratch repositories for tests. Nothing here touches a real repository,
//! a real `~/.claude` or a real editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::Zrush;
use crate::config::{Config, dirs_under};
use crate::host::RecordingHost;

pub fn git(cwd: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} failed");
}

/// A repo with one commit on `main`, cloned from a bare remote so
/// `origin/main` exists.
pub fn scratch_repo(td: &tempfile::TempDir) -> PathBuf {
    let base = crate::paths::real(td.path());
    let remote = base.join("remote.git");
    let repo = base.join("repo");
    git(&base, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(
        &base,
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "zrush tests"]);
    std::fs::write(repo.join("f.txt"), "seed").unwrap();
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-qm", "init"]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    repo
}

pub fn zrush(td: &tempfile::TempDir, repo: PathBuf) -> Arc<Zrush> {
    Arc::new(
        Zrush::new(
            dirs_under(td.path()),
            Config::default(),
            repo,
            crate::agent::all(),
            Box::new(RecordingHost::default()),
            None,
        )
        .unwrap(),
    )
}
