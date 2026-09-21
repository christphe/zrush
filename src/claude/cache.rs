//! A two-level cache for transcript metadata.
//!
//! A matching size+mtime answers without reading the transcript at all; a
//! matching content hash answers when the file was touched but its tail did
//! not change. The hash is xxh3 rather than md5, so there is no dependency
//! on an external binary and no difference between platforms.
//!
//! Derived data only: the whole directory is safe to delete.

use std::path::Path;

use crate::claude::transcript::{self, Meta, TAIL_LINES};
use crate::config::Dirs;

/// `<stamp> \t <hash> \t <title> \t <cwd> \t <last ts>`
fn read_entry(path: &Path) -> Option<(String, String, Meta)> {
    let line = std::fs::read_to_string(path).ok()?;
    let mut it = line.trim_end().split('\t');
    let stamp = it.next()?.to_string();
    let hash = it.next()?.to_string();
    let title = it.next()?.to_string();
    let cwd = it.next()?.to_string();
    let ts = it
        .next()
        .and_then(|t| t.parse::<i64>().ok())
        .filter(|t| *t > 0);
    if cwd.is_empty() {
        return None;
    }
    Some((
        stamp,
        hash,
        Meta {
            title: (!title.is_empty()).then_some(title),
            cwd: Some(cwd.into()),
            last_ts: ts,
        },
    ))
}

fn write_entry(path: &Path, stamp: &str, hash: &str, meta: &Meta) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!(
        "{stamp}\t{hash}\t{}\t{}\t{}\n",
        meta.title.as_deref().unwrap_or(""),
        meta.cwd
            .as_deref()
            .map(Path::to_string_lossy)
            .unwrap_or_default(),
        meta.last_ts.unwrap_or(0),
    );
    let _ = std::fs::write(path, line);
}

/// `known_mtime` is the mtime the directory scan already read. Passing it
/// saves a `stat` per transcript, and a repo can hold hundreds.
fn stamp_of(path: &Path, known_mtime: Option<i64>) -> Option<String> {
    if let Some(m) = known_mtime {
        return Some(format!("m{m}"));
    }
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{}-{mtime}", md.len()))
}

pub fn meta_cached(dirs: &Dirs, path: &Path, id: &str, known_mtime: Option<i64>) -> Option<Meta> {
    let entry_path = dirs.cache.join(id);
    let stamp = stamp_of(path, known_mtime)?;
    let cached = read_entry(&entry_path);

    if let Some((s, _, meta)) = &cached {
        if *s == stamp {
            return Some(meta.clone());
        }
    }

    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let tail = lines[start..].join("\n");
    let hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(tail.as_bytes()));

    if let Some((_, h, meta)) = &cached {
        if *h == hash {
            write_entry(&entry_path, &stamp, &hash, meta);
            return Some(meta.clone());
        }
    }

    let meta = transcript::read_meta(path, TAIL_LINES)?;
    write_entry(&entry_path, &stamp, &hash, &meta);
    Some(meta)
}

/// Drop whatever has not been touched in `older_than_days`.
pub fn sweep(dirs: &Dirs, older_than_days: u64) {
    let Ok(rd) = std::fs::read_dir(&dirs.cache) else {
        return;
    };
    let Some(cutoff) = std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(
        older_than_days * 24 * 60 * 60,
    )) else {
        return;
    };
    for e in rd.flatten() {
        if e.metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|m| m < cutoff)
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dirs_under;

    fn transcript_with(dirs: &Dirs, title: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(&dirs.claude_projects).unwrap();
        let p = dirs.claude_projects.join("x.jsonl");
        std::fs::write(
            &p,
            format!("{{\"type\":\"ai-title\",\"aiTitle\":\"{title}\",\"cwd\":\"/a\"}}\n"),
        )
        .unwrap();
        p
    }

    #[test]
    fn a_first_read_populates_the_cache() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let p = transcript_with(&d, "t");

        let m = meta_cached(&d, &p, "x", None).unwrap();
        assert_eq!(m.title.as_deref(), Some("t"));
        assert!(d.cache.join("x").exists());
    }

    #[test]
    fn a_second_read_of_an_unchanged_file_agrees_with_the_first() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let p = transcript_with(&d, "t");
        let first = meta_cached(&d, &p, "x", None).unwrap();
        let second = meta_cached(&d, &p, "x", None).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_changed_transcript_yields_the_new_title() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let p = transcript_with(&d, "old");
        meta_cached(&d, &p, "x", None).unwrap();
        std::fs::write(
            &p,
            "{\"type\":\"ai-title\",\"aiTitle\":\"a much longer new title\",\"cwd\":\"/a\"}\n",
        )
        .unwrap();
        let m = meta_cached(&d, &p, "x", None).unwrap();
        assert_eq!(m.title.as_deref(), Some("a much longer new title"));
    }

    #[test]
    fn a_corrupt_cache_entry_is_ignored_rather_than_fatal() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        std::fs::create_dir_all(&d.cache).unwrap();
        std::fs::write(d.cache.join("x"), "nonsense with no tabs").unwrap();
        let p = transcript_with(&d, "t");
        assert_eq!(
            meta_cached(&d, &p, "x", None).unwrap().title.as_deref(),
            Some("t")
        );
    }

    #[test]
    fn a_missing_transcript_is_a_miss_not_a_panic() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        assert!(meta_cached(&d, &td.path().join("nope.jsonl"), "x", None).is_none());
    }
}
