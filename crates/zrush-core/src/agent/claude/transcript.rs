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

/// The conversation tail, for the preview pane. Thinking blocks and tool
/// calls are cut out: what you want before resuming a session is what was
/// said.
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
        // Each text part is judged on its own, and each is stripped of the
        // machinery inside it. The bash version joined them first, so an
        // answer opening with a thinking block was dropped whole.
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

/// Machinery rather than conversation. A thinking block is often followed
/// by the answer in the same part, so the block is cut out of the text
/// instead of the whole part being thrown away — that answer is exactly
/// what you want to read before resuming.
const NOISE: &[&str] = &[
    "thinking",
    "think",
    "antml:thinking",
    "system-reminder",
    "command-name",
    "command-message",
    "command-args",
    "local-command-stdout",
    "function_calls",
    "function_results",
];

fn visible(part: &str) -> Option<String> {
    let t = squash(&strip_noise(part));
    // Whatever is left opening with `<` is a tool-use blob, not speech.
    (!t.is_empty() && !t.starts_with('<')).then_some(t)
}

/// Cut every noise block out of a part. A block with no closing tag runs
/// to the end: an unterminated `<thinking>` has nothing after it worth
/// keeping either.
fn strip_noise(part: &str) -> String {
    let mut s = part.to_string();
    for tag in NOISE {
        let close = format!("</{tag}>");
        while let Some(open) = open_tag(&s, tag) {
            let end = s[open..]
                .find(&close)
                .map_or(s.len(), |j| open + j + close.len());
            s.replace_range(open..end, " ");
        }
    }
    s
}

/// Where `<tag>` opens, if it does. The name has to end there: `<think`
/// must not match `<thinking>`, or its closing tag would never be found
/// and the rest of the answer would go with it.
fn open_tag(s: &str, tag: &str) -> Option<usize> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = s[from..].find(&open) {
        let at = from + i;
        let after = &s[at + open.len()..];
        if after.starts_with(['>', '/']) || after.starts_with(char::is_whitespace) {
            return Some(at);
        }
        from = at + open.len();
    }
    None
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
    fn a_thinking_block_is_cut_out_of_the_part_that_carries_the_answer() {
        let td = tempfile::TempDir::new().unwrap();
        let p = td.path().join("t.jsonl");
        std::fs::write(
            &p,
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"<thinking>hidden</thinking> the answer\"}]}}\n",
        )
        .unwrap();
        let texts: Vec<String> = turns(&p, 400, 14).into_iter().map(|t| t.text).collect();
        assert_eq!(texts, vec!["the answer".to_string()]);
    }

    #[test]
    fn a_short_think_tag_does_not_swallow_a_thinking_block() {
        assert_eq!(
            visible("<think>a</think> kept <thinking>b</thinking> too"),
            Some("kept too".to_string())
        );
    }

    #[test]
    fn an_unterminated_noise_block_takes_the_rest_with_it() {
        assert_eq!(
            visible("said this <system-reminder>and then junk"),
            Some("said this".to_string())
        );
    }

    #[test]
    fn turns_keep_at_most_the_requested_number() {
        let td = tempfile::TempDir::new().unwrap();
        let p = td.path().join("t.jsonl");
        std::fs::write(&p, T).unwrap();
        assert_eq!(turns(&p, 400, 2).len(), 2);
    }
}
