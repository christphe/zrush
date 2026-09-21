//! The Claude Code adapter.
//!
//! Live sessions come from `claude agents --json`; past ones are found by
//! reading the transcripts Claude Code keeps under `~/.claude/projects`.
//! Nothing here is ever used to resume: `claude --resume <id>` stays the
//! only mechanism.
//!
//! Everything Claude-specific lives under this module. The rest of zrush
//! sees only `agent::Agent`.

pub mod agents;
pub mod cache;
pub mod projects;
pub mod transcript;

use std::collections::HashSet;
use std::path::Path;

use crate::agent::{Agent, Scope, Turn};
use crate::config::{Config, Dirs};
use crate::error::Result;
use crate::model::{Session, SessionKind, age, truncate_title};

/// Claude Code session ids are UUIDs. Anything else must never reach
/// `claude --resume`.
pub const ID: &str = "claude";

pub struct ClaudeCode;

/// Sessions that are no longer running but can still be resumed. Claude Code
/// offers no non-interactive listing for them, so this reads the transcripts
/// it keeps — best effort, and bounded: only the newest `resumable_scan`
/// files are examined, unless something has been expanded.
fn find_resumable(dirs: &Dirs, cfg: &Config, scope: &Scope<'_>) -> Vec<Session> {
    let (main_root, worktrees, live_ids, uncapped) = (
        scope.main_root,
        scope.worktrees,
        scope.live_ids,
        scope.uncapped,
    );
    if cfg.resumable_max == 0 {
        return Vec::new();
    }
    let mut files: Vec<(i64, std::path::PathBuf)> = Vec::new();
    for d in projects::candidate_dirs(dirs, main_root, worktrees) {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
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
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if !uncapped {
        files.truncate(cfg.resumable_scan);
    }

    let now = jiff::Timestamp::now().as_second();
    let mut seen: HashSet<String> = live_ids.clone();
    let mut out: Vec<Session> = Vec::new();
    for (mtime, f) in files {
        let Some(id) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if !crate::state::is_uuid(&id) || !seen.insert(id.clone()) {
            continue;
        }
        // A transcript's mtime is useless as an age: Claude Code rewrites
        // them in batches, so conversations from different days share one
        // mtime to the minute. The last entry's timestamp is the truth.
        let Some(meta) = cache::meta_cached(dirs, &f, &id, Some(mtime)) else {
            continue;
        };
        let Some(cwd) = meta.cwd.clone() else {
            continue;
        };
        let ts = meta.last_ts.unwrap_or(mtime);
        let title = meta.title.unwrap_or_else(|| id.chars().take(8).collect());
        out.push(Session {
            agent: ID,
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

impl Agent for ClaudeCode {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn available(&self) -> bool {
        super::on_path("claude")
    }

    fn valid_id(&self, id: &str) -> bool {
        crate::state::is_uuid(id)
    }

    fn live(&self, dirs: &Dirs, cfg: &Config) -> Vec<Session> {
        agents::live(dirs, cfg)
    }

    fn resumable(&self, dirs: &Dirs, cfg: &Config, scope: &Scope<'_>) -> Vec<Session> {
        find_resumable(dirs, cfg, scope)
    }

    fn history_count(&self, dirs: &Dirs, worktree: &Path) -> usize {
        projects::history_count(dirs, worktree)
    }

    fn preview(&self, dirs: &Dirs, id: &str, turns: usize) -> Vec<Turn> {
        projects::find_transcript(dirs, id)
            .map(|p| transcript::turns(&p, transcript::TAIL_LINES, turns))
            .unwrap_or_default()
    }

    /// The transcript wherever Claude Code filed it, plus the sidecar
    /// directory and our cached title.
    fn delete(&self, dirs: &Dirs, id: &str) -> Result<usize> {
        let mut gone = 0;
        let Ok(rd) = std::fs::read_dir(&dirs.claude_projects) else {
            return Ok(0);
        };
        for e in rd.flatten() {
            let f = e.path().join(format!("{id}.jsonl"));
            if f.is_file() && std::fs::remove_file(&f).is_ok() {
                gone += 1;
            }
            let sidecar = e.path().join(id);
            if sidecar.is_dir() {
                let _ = std::fs::remove_dir_all(&sidecar);
            }
        }
        let _ = std::fs::remove_file(dirs.cache.join(id));
        Ok(gone)
    }

    fn resume_command(&self, id: &str) -> Vec<String> {
        vec!["claude".into(), "--resume".into(), id.into()]
    }

    fn start_command(&self) -> Vec<String> {
        vec!["claude".into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dirs_under;

    fn scope<'a>(main_root: &'a Path, live: &'a HashSet<String>) -> Scope<'a> {
        Scope {
            main_root,
            new_root: main_root,
            worktrees: &[],
            live_ids: live,
            uncapped: false,
        }
    }

    fn fixture(dirs: &Dirs, slug_of: &Path, id: &str, title: &str, cwd: &str) {
        let d = dirs.claude_projects.join(projects::slugify(slug_of));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(format!("{id}.jsonl")),
            format!(
                "{{\"type\":\"ai-title\",\"aiTitle\":\"{title}\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-09-20T10:00:00.000Z\"}}\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn transcripts_become_resumable_sessions() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        fixture(
            &dirs,
            root,
            "aaaaaaaa-1111-2222-3333-444455556666",
            "old work",
            "/home/u/repo",
        );

        let s = find_resumable(&dirs, &Config::default(), &scope(root, &HashSet::new()));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "old work");
        assert_eq!(s[0].kind, SessionKind::Resumable);
        assert!(s[0].status.starts_with("resumable "));
    }

    #[test]
    fn a_session_already_live_is_not_listed_twice() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        let id = "aaaaaaaa-1111-2222-3333-444455556666";
        fixture(&dirs, root, id, "old work", "/home/u/repo");

        let live: HashSet<String> = std::iter::once(id.to_string()).collect();
        assert!(find_resumable(&dirs, &Config::default(), &scope(root, &live)).is_empty());
    }

    #[test]
    fn a_filename_that_is_not_a_uuid_is_skipped() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        let d = dirs.claude_projects.join(projects::slugify(root));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("notes.jsonl"), "{}\n").unwrap();
        assert!(
            find_resumable(&dirs, &Config::default(), &scope(root, &HashSet::new())).is_empty()
        );
    }

    #[test]
    fn the_scan_is_bounded_unless_uncapped() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        for i in 0..10u8 {
            let id = format!("aaaaaaaa-1111-2222-3333-4444555566{i:02}");
            fixture(&dirs, root, &id, "t", "/home/u/repo");
        }
        let cfg = Config {
            resumable_scan: 3,
            ..Config::default()
        };
        assert_eq!(
            find_resumable(&dirs, &cfg, &scope(root, &HashSet::new())).len(),
            3
        );
        assert_eq!(
            find_resumable(
                &dirs,
                &cfg,
                &Scope {
                    uncapped: true,
                    ..scope(root, &HashSet::new())
                }
            )
            .len(),
            10
        );
    }

    #[test]
    fn a_zero_cap_disables_the_scan_entirely() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        fixture(
            &dirs,
            root,
            "aaaaaaaa-1111-2222-3333-444455556666",
            "t",
            "/home/u/repo",
        );
        let cfg = Config {
            resumable_max: 0,
            ..Config::default()
        };
        assert!(find_resumable(&dirs, &cfg, &scope(root, &HashSet::new())).is_empty());
    }
}
