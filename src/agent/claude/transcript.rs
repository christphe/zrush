//! Reading what a transcript says about its session.
//!
//! `claude agents --json` reports the cwd a session was LAUNCHED in, not the
//! one it has since moved to, and no title at all. Both live in the
//! transcript:
//!   `{"type":"ai-title","aiTitle":…}`  rewritten as the session goes on
//!   every assistant/user line carries `.cwd` and `.timestamp`
//! Best-effort in every sense: any failure falls back to the session's name
//! and its launch cwd.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::agent::{Role, Turn};

/// How far back we look in a transcript.
pub const TAIL_LINES: usize = 400;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub last_ts: Option<i64>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<serde_json::Value>,
}

/// Later lines win for each field independently: the title is rewritten as
/// the conversation goes on, and the cwd changes when a session moves into a
/// worktree.
pub fn parse_tail(tail: &str) -> Meta {
    let mut m = Meta::default();
    for line in tail.lines() {
        let Ok(l) = serde_json::from_str::<Line>(line) else {
            continue;
        };
        if let Some(t) = l.ai_title.filter(|t| !t.is_empty()) {
            m.title = Some(squash(&t));
        }
        if let Some(c) = l.cwd.filter(|c| !c.is_empty()) {
            m.cwd = Some(PathBuf::from(c));
        }
        if let Some(ts) = l.timestamp.as_deref().and_then(parse_ts) {
            m.last_ts = Some(ts);
        }
    }
    m
}

fn parse_ts(s: &str) -> Option<i64> {
    s.parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_second)
}

/// The tail first. The title is written once, early, so a long conversation
/// pushes it out of the window; on a miss, scan the whole file for the last
/// line that carries one. One extra pass, only when it is needed.
pub fn read_meta(path: &Path, tail_lines: usize) -> Option<Meta> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    let mut m = parse_tail(&lines[start..].join("\n"));
    if m.title.is_none() {
        m.title = lines
            .iter()
            .rev()
            .filter(|l| l.contains("\"aiTitle\""))
            .find_map(|l| serde_json::from_str::<Line>(l).ok()?.ai_title)
            .filter(|t| !t.is_empty())
            .map(|t| squash(&t));
    }
    (m.title.is_some() || m.cwd.is_some()).then_some(m)
}

/// The conversation tail, for the preview pane. Tool calls and thinking
/// blocks start with `<` and are dropped: what you want before resuming a
/// session is what was said.
pub fn turns(path: &Path, tail_lines: usize, keep: usize) -> Vec<Turn> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    let mut out: Vec<Turn> = Vec::new();
    for line in &lines[start..] {
        let Ok(l) = serde_json::from_str::<Line>(line) else {
            continue;
        };
        let role = match l.kind.as_deref() {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let Some(content) = l.message.and_then(|m| m.content) else {
            continue;
        };
        // Each text part is judged on its own. The bash version joined them
        // first, so an answer opening with a thinking block was dropped
        // whole; here only the block goes.
        let text = match content {
            serde_json::Value::String(s) => visible(&s).unwrap_or_default(),
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .filter_map(|p| p.get("text").and_then(serde_json::Value::as_str))
                .filter_map(visible)
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };
        if text.is_empty() {
            continue;
        }
        out.push(Turn { role, text });
    }
    let drop = out.len().saturating_sub(keep);
    out.drain(..drop);
    out
}

/// Tool calls and thinking blocks start with `<`: what you want before
/// resuming a session is what was said.
fn visible(part: &str) -> Option<String> {
    let t = squash(part);
    (!t.is_empty() && !t.starts_with('<')).then_some(t)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    const T: &str = include_str!("../../../tests/fixtures/transcript.jsonl");

    fn long_transcript(dir: &Path) -> PathBuf {
        let p = dir.join("long.jsonl");
        let mut s = String::from("{\"type\":\"ai-title\",\"aiTitle\":\"early title\"}\n");
        for i in 0..500 {
            let _ = writeln!(
                s,
                "{{\"type\":\"user\",\"message\":{{\"content\":\"line {i}\"}},\"cwd\":\"/home/u/repo\",\"timestamp\":\"2026-09-20T10:00:00.000Z\"}}"
            );
        }
        std::fs::write(&p, s).unwrap();
        p
    }

    #[test]
    fn the_last_title_wins() {
        assert_eq!(parse_tail(T).title.as_deref(), Some("port zrush to rust"));
    }

    #[test]
    fn the_last_cwd_wins_so_a_session_that_moved_is_found() {
        assert_eq!(
            parse_tail(T).cwd.as_deref(),
            Some(Path::new("/home/u/repo/.claude/worktrees/rust"))
        );
    }

    #[test]
    fn the_timestamp_is_the_last_entrys_not_the_files_mtime() {
        let ts = parse_tail(T).last_ts.unwrap();
        let want: i64 = "2026-09-20T10:06:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_second();
        assert_eq!(ts, want);
    }

    #[test]
    fn fractional_seconds_do_not_break_the_parse() {
        let one = "{\"type\":\"user\",\"timestamp\":\"2026-09-20T10:00:05.123Z\"}";
        assert!(parse_tail(one).last_ts.is_some());
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped() {
        let mixed = format!("not json\n{T}");
        assert_eq!(
            parse_tail(&mixed).title.as_deref(),
            Some("port zrush to rust")
        );
    }

    #[test]
    fn a_title_outside_the_tail_window_is_still_found() {
        let td = tempfile::TempDir::new().unwrap();
        let p = long_transcript(td.path());
        let m = read_meta(&p, 10).unwrap();
        assert_eq!(m.title.as_deref(), Some("early title"));
    }

    #[test]
    fn turns_drop_thinking_blocks_and_squash_whitespace() {
        let td = tempfile::TempDir::new().unwrap();
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, T).unwrap();
        let texts: Vec<String> = turns(&p, 400, 14).into_iter().map(|t| t.text).collect();
        assert!(texts.iter().any(|t| t == "and now this"));
        assert!(texts.iter().any(|t| t == "done"));
        assert!(!texts.iter().any(|t| t.contains("hidden")));
    }

    #[test]
    fn turns_keep_at_most_the_requested_number() {
        let td = tempfile::TempDir::new().unwrap();
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, T).unwrap();
        assert_eq!(turns(&p, 400, 2).len(), 2);
    }
}
