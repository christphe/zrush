//! The worktree <-> session association.
//!
//! It lives in the worktree's own gitdir, at
//! `<git rev-parse --absolute-git-dir>/zrush-session`: that is
//! `repo/.git/worktrees/<name>` for a linked worktree and `repo/.git` for
//! the main one. Per-worktree by construction, so several editor windows can
//! each hold their own session. No global state, no secrets.

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

/// The first `session_id=` line wins. Anything that is not a UUID is
/// discarded: a malformed value must never reach `claude --resume`.
fn parse(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != "session_id" {
            continue;
        }
        let v = v.trim().trim_matches('"');
        return is_uuid(v).then(|| v.to_string());
    }
    None
}

pub fn read(worktree: &Path) -> Result<Option<String>> {
    let f = state_file(worktree)?;
    match std::fs::read_to_string(&f) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Written through a temporary file and renamed, so a crash mid-write cannot
/// leave half an id behind.
pub fn write(worktree: &Path, id: &str) -> Result<()> {
    if !is_uuid(id) {
        return Err(ZrushError::msg("refusing to store a malformed session id"));
    }
    let f = state_file(worktree)?;
    let tmp = f.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, format!("session_id={id}\n"))?;
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
        assert_eq!(
            parse(&format!("session_id=\"{GOOD}\"\n")).as_deref(),
            Some(GOOD)
        );
        assert_eq!(
            parse(&format!("  session_id = {GOOD}  \n")).as_deref(),
            Some(GOOD)
        );
        assert_eq!(
            parse(&format!("other=1\nsession_id={GOOD}\n")).as_deref(),
            Some(GOOD)
        );
    }

    #[test]
    fn a_malformed_id_is_discarded_rather_than_returned() {
        assert_eq!(parse("session_id=nope\n"), None);
        assert_eq!(parse(""), None);
    }
}
