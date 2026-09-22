//! Every `git` invocation, and the parsers for what it prints.
//!
//! Shelled out rather than linked: `git worktree add` and `git worktree
//! remove` have semantics worth not reimplementing, and a C dependency
//! would complicate the build on three platforms for no gain.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Result, ZrushError};
use crate::model::{GitStatus, Worktree};

#[derive(Debug, Clone)]
pub enum AddFrom {
    ExistingBranch(String),
    NewBranch { name: String, base: String },
}

pub fn run(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
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
    Ok(parse_worktree_list(&run(
        repo,
        &["worktree", "list", "--porcelain"],
    )?))
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

pub fn status(path: &Path) -> Result<GitStatus> {
    Ok(parse_status_v2(&run(
        path,
        &["status", "--porcelain=v2", "--branch"],
    )?))
}

/// One `git status --porcelain=v2 --branch` yields both numbers: the changed
/// files and the ahead/behind counts against the upstream. Measured at
/// 40-250 ms on a large repo, which is why it runs on a worker rather than
/// before the first frame.
pub fn parse_status_v2(out: &str) -> GitStatus {
    let mut s = GitStatus::default();
    for line in out.lines() {
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            s.has_upstream = true;
            let mut it = ab.split_whitespace();
            s.ahead = it
                .next()
                .and_then(|v| v.trim_start_matches('+').parse().ok())
                .unwrap_or(0);
            s.behind = it
                .next()
                .and_then(|v| v.trim_start_matches('-').parse().ok())
                .unwrap_or(0);
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            s.changed += 1;
        } else if line.starts_with("? ") {
            s.untracked += 1;
        }
    }
    s
}

pub fn branches(repo: &Path) -> Result<Vec<String>> {
    let out = run(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )?;
    Ok(out
        .lines()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect())
}

pub fn branch_exists(repo: &Path, name: &str) -> bool {
    run(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
    .is_ok()
}

pub fn current_branch(repo: &Path) -> Option<String> {
    run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// What a new branch is cut from: the remote default branch when there is
/// one, because it is the base that is actually up to date; otherwise the
/// local default, and failing that HEAD. No fetch either way — zrush never
/// touches the network.
pub fn default_base(repo: &Path) -> Result<String> {
    if let Ok(b) = run(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
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
    Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn worktree_add(repo: &Path, dest: &Path, from: &AddFrom) -> Result<()> {
    let dest = dest.to_string_lossy().into_owned();
    match from {
        AddFrom::ExistingBranch(b) => run(repo, &["worktree", "add", &dest, b]).map(drop),
        // --no-track: the branch starts at `base` but owns no upstream, so a
        // later `git push -u origin HEAD` sets the right one instead of
        // pointing at main.
        AddFrom::NewBranch { name, base } => run(
            repo,
            &["worktree", "add", "--no-track", "-b", name, &dest, base],
        )
        .map(drop),
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

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../tests/fixtures/worktree-list.txt");

    const STATUS: &str = "\
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
    fn every_worktree_is_read() {
        assert_eq!(parse_worktree_list(LIST).len(), 5);
    }

    #[test]
    fn the_first_entry_is_the_main_worktree() {
        let w = parse_worktree_list(LIST);
        assert!(w[0].is_main);
        assert!(!w[1].is_main);
        assert_eq!(w[0].path, Path::new("/home/u/repo"));
    }

    #[test]
    fn branches_lose_their_refs_heads_prefix() {
        assert_eq!(parse_worktree_list(LIST)[1].branch, "rust");
    }

    #[test]
    fn a_detached_head_is_named_as_such() {
        assert_eq!(parse_worktree_list(LIST)[2].branch, "(detached)");
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

    #[test]
    fn changed_counts_every_tracked_change() {
        assert_eq!(parse_status_v2(STATUS).changed, 4);
    }

    #[test]
    fn untracked_is_counted_separately() {
        assert_eq!(parse_status_v2(STATUS).untracked, 2);
    }

    #[test]
    fn ahead_and_behind_come_from_branch_ab() {
        let s = parse_status_v2(STATUS);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
        assert!(s.has_upstream);
    }

    #[test]
    fn no_branch_ab_line_means_no_upstream() {
        assert!(!parse_status_v2("# branch.head main\n").has_upstream);
    }

    #[test]
    fn an_invalid_branch_name_is_refused() {
        assert!(!check_ref_format("has space"));
        assert!(!check_ref_format(""));
        assert!(check_ref_format("feat/thing-1"));
    }
}
