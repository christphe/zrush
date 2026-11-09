//! Scratch repositories for tests. Nothing here touches a real repository,
//! a real `~/.claude` or a real editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use zrush_core::app::Zrush;
use zrush_core::config::{Config, dirs_under};
use zrush_core::host::RecordingHost;

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
    let base = zrush_core::paths::real(td.path());
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
            zrush_core::agent::all(),
            Box::new(RecordingHost::default()),
            None,
        )
        .unwrap(),
    )
}

/// A tab over `repo` that knows every agent, whether or not the machine
/// running the tests has it installed: a CI runner has none.
pub fn open_tab(
    td: &tempfile::TempDir,
    repo: &Path,
    host: Box<dyn zrush_core::host::Host>,
) -> crate::tab::Tab {
    let dirs = dirs_under(td.path());
    let cfg = Config::default();
    let made = Zrush::new(
        dirs,
        cfg.clone(),
        repo.to_path_buf(),
        zrush_core::agent::all(),
        host,
        None,
    );
    crate::tab::Tab::over(repo, &cfg, made, Arc::new(|| {}))
}

pub const SESSION_ID: &str = "aaaaaaaa-1111-2222-3333-444455556666";
pub const SESSION_TITLE: &str = "port zrush to rust";

/// A resumable conversation held in `cwd`, filed where Claude Code would
/// file it under the scratch `claude_projects`: `turns` alternate user and
/// agent, starting with the user.
pub fn with_session(td: &tempfile::TempDir, cwd: &Path, turns: &[&str]) {
    let dir = dirs_under(td.path())
        .claude_projects
        .join(zrush_core::agent::claude::projects::slugify(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let cwd_json = serde_json_string(&cwd.to_string_lossy());
    let mut lines = vec![format!(
        "{{\"type\":\"ai-title\",\"aiTitle\":\"{SESSION_TITLE}\"}}"
    )];
    for (i, text) in turns.iter().enumerate() {
        let text = serde_json_string(text);
        let line = if i % 2 == 0 {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"content\":{text}}},\"cwd\":{cwd_json},\"timestamp\":\"2026-11-08T10:00:00.000Z\"}}"
            )
        } else {
            format!(
                "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":{text}}}]}},\"cwd\":{cwd_json},\"timestamp\":\"2026-11-08T10:00:01.000Z\"}}"
            )
        };
        lines.push(line);
    }
    std::fs::write(
        dir.join(format!("{SESSION_ID}.jsonl")),
        lines.join("\n") + "\n",
    )
    .unwrap();
}

/// A JSON string literal, quotes included. Rust's `Debug` escaping is
/// JSON's for everything these fixtures hold.
fn serde_json_string(s: &str) -> String {
    format!("{s:?}")
}
