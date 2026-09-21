use std::path::{Path, PathBuf};

use crate::config::Dirs;
use crate::model::Worktree;

/// Claude Code files a session's transcript under a directory named after
/// the cwd it was launched in, with every non-alphanumeric character
/// replaced by a dash. This must match `sed 's/[^A-Za-z0-9]/-/g'` exactly.
pub fn slugify(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn transcript_path(dirs: &Dirs, launch_cwd: &Path, id: &str) -> PathBuf {
    dirs.claude_projects
        .join(slugify(launch_cwd))
        .join(format!("{id}.jsonl"))
}

/// How many transcripts Claude Code keeps for a worktree. A read-only hint
/// for the badge; `claude --resume` is still what resumes one.
pub fn history_count(dirs: &Dirs, worktree: &Path) -> usize {
    let dir = dirs.claude_projects.join(slugify(worktree));
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .count()
}

/// Project directories that can hold this repo's sessions: the main
/// worktree's slug and everything filed under it (linked worktrees,
/// `claude --worktree`), plus each worktree's own slug, for worktrees kept
/// outside the repo.
pub fn candidate_dirs(dirs: &Dirs, main_root: &Path, worktrees: &[Worktree]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let prefix = slugify(main_root);
    if let Ok(rd) = std::fs::read_dir(&dirs.claude_projects) {
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix) && e.path().is_dir())
            .map(|e| e.path())
            .collect();
        found.sort();
        out.append(&mut found);
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

#[cfg(test)]
mod tests {
    use super::*;

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
