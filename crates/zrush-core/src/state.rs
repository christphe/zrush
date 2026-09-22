//! The worktree <-> session association.
//!
//! It lives in the worktree's own gitdir, at
//! `<git rev-parse --absolute-git-dir>/zrush-session`: that is
//! `repo/.git/worktrees/<name>` for a linked worktree and `repo/.git` for
//! the main one. Per-worktree by construction, so several editor windows can
//! each hold their own session. No global state, no secrets.
//!
//! Shell-ish, one key per line, unknown keys ignored:
//!
//! ```text
//! session_id=<id>
//! agent=claude
//! ```
//!
//! A file with no `agent=` line was written before zrush knew about more
//! than one, and means the first available agent. This module stores and
//! returns the id verbatim; whether it is well formed is the agent's
//! question, answered by `Agent::valid_id`.

use std::path::{Path, PathBuf};

use crate::error::{Result, ZrushError};
use crate::git;

const BASENAME: &str = "zrush-session";

pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn state_file(worktree: &Path) -> Result<PathBuf> {
    Ok(git::absolute_git_dir(worktree)?.join(BASENAME))
}

/// What a worktree is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    /// `None` for a file written before agents were a concept.
    pub agent: Option<String>,
}

/// The first value for each key wins.
fn parse(text: &str) -> Option<Binding> {
    let mut id: Option<String> = None;
    let mut agent: Option<String> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        if v.is_empty() {
            continue;
        }
        match k.trim() {
            "session_id" if id.is_none() => id = Some(v),
            "agent" if agent.is_none() => agent = Some(v),
            _ => {}
        }
    }
    id.map(|id| Binding { id, agent })
}

pub fn read(worktree: &Path) -> Result<Option<Binding>> {
    let f = state_file(worktree)?;
    match std::fs::read_to_string(&f) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Written through a temporary file and renamed, so a crash mid-write cannot
/// leave half an id behind. The caller has already asked the agent whether
/// the id is well formed.
pub fn write(worktree: &Path, agent: &str, id: &str) -> Result<()> {
    if id.is_empty() || id.contains(['\n', '\r']) {
        return Err(ZrushError::msg("refusing to store a malformed session id"));
    }
    let f = state_file(worktree)?;
    let tmp = f.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, format!("session_id={id}\nagent={agent}\n"))?;
    std::fs::rename(&tmp, &f)?;
    Ok(())
}

pub fn clear(worktree: &Path) -> Result<()> {
    let f = state_file(worktree)?;
    match std::fs::remove_file(&f) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "f3a91c2e-1111-2222-3333-444455556666";

    #[test]
    fn a_uuid_is_recognised() {
        assert!(is_uuid(GOOD));
        assert!(is_uuid("F3A91C2E-1111-2222-3333-444455556666"));
    }

    #[test]
    fn anything_else_is_not() {
        assert!(!is_uuid(""));
        assert!(!is_uuid("nope"));
        assert!(!is_uuid("f3a91c2e-1111-2222-3333-44445555666"));
        assert!(!is_uuid("f3a91c2e_1111_2222_3333_444455556666"));
        assert!(!is_uuid("g3a91c2e-1111-2222-3333-444455556666"));
    }

    #[test]
    fn the_first_session_id_line_wins_and_quotes_are_stripped() {
        assert_eq!(parse(&format!("session_id=\"{GOOD}\"\n")).unwrap().id, GOOD);
        assert_eq!(
            parse(&format!("  session_id = {GOOD}  \n")).unwrap().id,
            GOOD
        );
        assert_eq!(
            parse(&format!("other=1\nsession_id={GOOD}\n")).unwrap().id,
            GOOD
        );
    }

    #[test]
    fn a_file_with_no_id_binds_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("agent=claude\n"), None);
        assert_eq!(parse("session_id=\n"), None);
    }

    #[test]
    fn the_agent_is_read_back_when_it_is_there() {
        let b = parse(&format!("session_id={GOOD}\nagent=codex\n")).unwrap();
        assert_eq!(b.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn a_file_written_before_agents_existed_names_none() {
        let b = parse(&format!("session_id={GOOD}\n")).unwrap();
        assert_eq!(b.agent, None);
    }

    #[test]
    fn a_non_uuid_id_is_kept_because_another_agent_may_use_one() {
        // is_uuid is Claude Code's rule, applied by its adapter, not here.
        let b = parse("session_id=01JB2Q3K4\nagent=codex\n").unwrap();
        assert_eq!(b.id, "01JB2Q3K4");
    }
}
