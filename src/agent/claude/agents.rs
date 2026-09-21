//! The official, scriptable source of truth for running sessions:
//! `claude agents --json`.
//!
//! Background sessions are dropped: they are reattached with `claude
//! attach`, not `--resume`, so they are not something zrush can bind to an
//! editor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::agent::claude::{cache, projects};
use crate::config::{Config, Dirs};
use crate::model::{Session, SessionKind, age, truncate_title};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub session_id: String,
    pub cwd: PathBuf,
    pub name: String,
    pub status: String,
    pub started_at_ms: i64,
}

#[derive(Deserialize)]
struct Raw {
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    name: Option<String>,
    status: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
}

/// `claude agents` lists one entry per PROCESS, and two processes can share
/// a session id (`claude -c` on a conversation already open). Group by
/// session id so a session is one row, not one row per process.
pub fn parse(json: &str) -> Vec<Entry> {
    let Ok(raws) = serde_json::from_str::<Vec<Raw>>(json) else {
        return Vec::new();
    };
    let mut best: HashMap<String, Entry> = HashMap::new();
    for r in raws {
        if r.kind.as_deref() != Some("interactive") {
            continue;
        }
        let Some(id) = r.session_id.filter(|s| !s.is_empty()) else {
            continue;
        };
        let e = Entry {
            name: r.name.unwrap_or_else(|| id.chars().take(8).collect()),
            cwd: PathBuf::from(r.cwd.unwrap_or_default()),
            status: r.status.unwrap_or_default(),
            started_at_ms: r.started_at.unwrap_or(0),
            session_id: id.clone(),
        };
        match best.get(&id) {
            Some(cur) if cur.started_at_ms >= e.started_at_ms => {}
            _ => {
                best.insert(id, e);
            }
        }
    }
    let mut out: Vec<Entry> = best.into_values().collect();
    out.sort_by(|a, b| {
        b.started_at_ms
            .cmp(&a.started_at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    out
}

/// A name you set with `claude -n` beats the title Claude generated for
/// itself. Auto-generated names look like `<dir>-<2 hex>` (wms-e8, 2346-06),
/// so anything else is yours and wins.
pub fn is_generated_name(name: &str, cwd: &Path) -> bool {
    let base = cwd
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    if base.is_empty() {
        return false;
    }
    let Some(suffix) = name.strip_prefix(&base).and_then(|s| s.strip_prefix('-')) else {
        return false;
    };
    suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Turn one `claude agents` entry into a session row, enriched from its
/// transcript.
pub fn to_session(dirs: &Dirs, cfg: &Config, e: Entry, now: i64) -> Session {
    let path = projects::transcript_path(dirs, &e.cwd, &e.session_id);
    let meta = if cfg.titles {
        cache::meta_cached(dirs, &path, &e.session_id, None)
    } else {
        None
    };

    let from_transcript = meta
        .as_ref()
        .and_then(|m| m.title.clone())
        .unwrap_or_default();
    let yours = !e.name.is_empty() && !is_generated_name(&e.name, &e.cwd);
    let title = if yours || from_transcript.is_empty() {
        e.name.clone()
    } else {
        from_transcript
    };

    let cwd = meta
        .as_ref()
        .and_then(|m| m.cwd.clone())
        .unwrap_or_else(|| e.cwd.clone());
    // The last message beats the process start time: a session idle since
    // yesterday should not read as "0m" because you just reopened it.
    let last = meta
        .as_ref()
        .and_then(|m| m.last_ts)
        .unwrap_or(e.started_at_ms / 1000);
    let status = if e.status.is_empty() {
        "?".to_string()
    } else {
        e.status.clone()
    };

    Session {
        agent: super::ID,
        id: e.session_id,
        title: truncate_title(&title, cfg.title_width),
        status: if last > 0 {
            format!("{status} {}", age(now, last))
        } else {
            status
        },
        cwd,
        launch_cwd: e.cwd,
        kind: SessionKind::Live,
        last_activity: last,
    }
}

pub fn live(dirs: &Dirs, cfg: &Config) -> Vec<Session> {
    let Ok(out) = std::process::Command::new("claude")
        .args(["agents", "--json", "--all"])
        .output()
    else {
        return Vec::new();
    };
    let now = jiff::Timestamp::now().as_second();
    parse(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .map(|e| to_session(dirs, cfg, e, now))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dirs_under;

    const J: &str = include_str!("../../../tests/fixtures/agents.json");

    #[test]
    fn background_sessions_are_dropped() {
        assert!(
            !parse(J)
                .iter()
                .any(|x| x.session_id.starts_with("cccccccc"))
        );
    }

    #[test]
    fn an_entry_with_no_session_id_is_dropped() {
        assert!(!parse(J).iter().any(|x| x.session_id.is_empty()));
    }

    #[test]
    fn two_processes_on_one_session_collapse_to_the_newest() {
        let e = parse(J);
        let same: Vec<_> = e
            .iter()
            .filter(|x| x.session_id.starts_with("aaaaaaaa"))
            .collect();
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].started_at_ms, 1_789_646_500_000);
    }

    #[test]
    fn malformed_json_yields_no_sessions_rather_than_an_error() {
        assert!(parse("{not json").is_empty());
    }

    #[test]
    fn a_generated_name_is_recognised() {
        assert!(is_generated_name("repo-e8", Path::new("/home/u/repo")));
        assert!(is_generated_name("2346-06", Path::new("/home/u/2346")));
        assert!(!is_generated_name("my own name", Path::new("/home/u/repo")));
        assert!(!is_generated_name("repo-zz", Path::new("/home/u/repo")));
        assert!(!is_generated_name("other-e8", Path::new("/home/u/repo")));
    }

    #[test]
    fn a_name_you_chose_beats_the_generated_title() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let e = parse(J)
            .into_iter()
            .find(|e| e.session_id.starts_with("bbbbbbbb"))
            .unwrap();
        let s = to_session(&d, &Config::default(), e, 1_789_646_500);
        assert_eq!(s.title, "my own name");
    }

    #[test]
    fn a_generated_name_leaves_room_for_the_transcript_title() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let e = parse(J)
            .into_iter()
            .find(|e| e.session_id.starts_with("aaaaaaaa"))
            .unwrap();
        let dir = d.claude_projects.join(projects::slugify(&e.cwd));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{}.jsonl", e.session_id)),
            "{\"type\":\"ai-title\",\"aiTitle\":\"from the transcript\",\"cwd\":\"/home/u/repo\"}\n",
        )
        .unwrap();
        let s = to_session(&d, &Config::default(), e, 1_789_646_500);
        assert_eq!(s.title, "from the transcript");
    }
}
