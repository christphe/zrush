//! Everything zrush learns about Claude Code sessions.
//!
//! Live ones come from `claude agents --json`; past ones are found by
//! reading the transcripts Claude Code keeps under `~/.claude/projects`.
//! Nothing here is ever used to resume: `claude --resume <id>` stays the
//! only mechanism.

pub mod agents;
pub mod cache;
pub mod projects;
pub mod transcript;

use std::collections::HashSet;
use std::path::Path;

use crate::config::{Config, Dirs};
use crate::model::{Session, SessionKind, Worktree, age, truncate_title};
use crate::state::is_uuid;

/// Sessions that are no longer running but can still be resumed. Claude Code
/// offers no non-interactive listing for them, so this reads the transcripts
/// it keeps — best effort, and bounded: only the newest `resumable_scan`
/// files are examined, unless something has been expanded.
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
        if !is_uuid(&id) || !seen.insert(id.clone()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dirs_under;

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

        let s = resumable(&dirs, &Config::default(), root, &[], &HashSet::new(), false);
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
        assert!(resumable(&dirs, &Config::default(), root, &[], &live, false).is_empty());
    }

    #[test]
    fn a_filename_that_is_not_a_uuid_is_skipped() {
        let td = tempfile::TempDir::new().unwrap();
        let dirs = dirs_under(td.path());
        let root = Path::new("/home/u/repo");
        let d = dirs.claude_projects.join(projects::slugify(root));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("notes.jsonl"), "{}\n").unwrap();
        assert!(resumable(&dirs, &Config::default(), root, &[], &HashSet::new(), false).is_empty());
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
            resumable(&dirs, &cfg, root, &[], &HashSet::new(), false).len(),
            3
        );
        assert_eq!(
            resumable(&dirs, &cfg, root, &[], &HashSet::new(), true).len(),
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
        assert!(resumable(&dirs, &cfg, root, &[], &HashSet::new(), false).is_empty());
    }
}
