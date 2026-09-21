use std::path::PathBuf;

pub mod tree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub locked: bool,
    pub prunable: bool,
    pub is_main: bool,
}

impl Worktree {
    /// The branch, then the flags, then `[root]` for the primary worktree:
    /// the one holding `.git`, and the one git refuses to remove.
    pub fn label(&self) -> String {
        let mut s = self.branch.clone();
        if self.locked {
            s.push_str(" 🔒");
        }
        if self.prunable {
            s.push_str(" ⚠");
        }
        if self.is_main {
            s.push_str(" [root]");
        }
        s
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub changed: u32,
    pub untracked: u32,
    pub ahead: u32,
    pub behind: u32,
    pub has_upstream: bool,
}

impl GitStatus {
    /// Compact and plain: the table pads by display width, so no colour
    /// escape may appear in here.
    ///   `~3` changed tracked files   `?1` untracked
    ///   `↑2` ahead of upstream       `↓1` behind      `⚠` no upstream at all
    pub fn badge(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.changed > 0 {
            parts.push(format!("~{}", self.changed));
        }
        if self.untracked > 0 {
            parts.push(format!("?{}", self.untracked));
        }
        if self.has_upstream {
            if self.ahead > 0 {
                parts.push(format!("↑{}", self.ahead));
            }
            if self.behind > 0 {
                parts.push(format!("↓{}", self.behind));
            }
        } else {
            parts.push("⚠".into());
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Live,
    Resumable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Where the session actually is now, which is what it is filed under.
    pub cwd: PathBuf,
    /// Where it was launched, which is what `claude agents` reports.
    pub launch_cwd: PathBuf,
    pub kind: SessionKind,
    pub last_activity: i64,
}

/// Compact age for the session rows: 40m, 6h, 3d, 5mo.
pub fn age(now: i64, then: i64) -> String {
    let d = (now - then).max(0);
    if d < 3_600 {
        format!("{}m", d / 60)
    } else if d < 86_400 {
        format!("{}h", d / 3_600)
    } else if d < 2_592_000 {
        format!("{}d", d / 86_400)
    } else {
        format!("{}mo", d / 2_592_000)
    }
}

/// Counts characters, not bytes: titles routinely carry accents.
pub fn truncate_title(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_the_way_they_did_in_bash() {
        let now = 1_000_000;
        assert_eq!(age(now, now - 40 * 60), "40m");
        assert_eq!(age(now, now - 6 * 3600), "6h");
        assert_eq!(age(now, now - 3 * 86_400), "3d");
        assert_eq!(age(now, now - 5 * 2_592_000), "5mo");
    }

    #[test]
    fn an_age_is_never_negative() {
        assert_eq!(age(100, 500), "0m");
    }

    #[test]
    fn a_long_title_is_cut_with_an_ellipsis() {
        assert_eq!(truncate_title("abcdefghij", 5), "abcd…");
        assert_eq!(truncate_title("abc", 5), "abc");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        assert_eq!(truncate_title("éééééé", 3), "éé…");
    }

    #[test]
    fn the_badge_reads_as_it_did_in_bash() {
        let s = GitStatus {
            changed: 4,
            untracked: 2,
            ahead: 2,
            behind: 1,
            has_upstream: true,
        };
        assert_eq!(s.badge(), "~4 ?2 ↑2 ↓1");
    }

    #[test]
    fn no_upstream_is_a_warning_not_an_arrow() {
        let s = GitStatus {
            changed: 1,
            untracked: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
        };
        assert_eq!(s.badge(), "~1 ⚠");
    }

    #[test]
    fn a_clean_worktree_has_an_empty_badge() {
        let s = GitStatus {
            changed: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            has_upstream: true,
        };
        assert_eq!(s.badge(), "");
    }
}
