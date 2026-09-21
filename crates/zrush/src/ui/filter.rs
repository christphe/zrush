//! Filtering the list.
//!
//! Three rules, all learned by typing `/rust` and getting everything back:
//!
//! - **Substring by default, fuzzy on request.** fzf can afford fuzzy
//!   matching because it ranks: the best match goes to the top and the rest
//!   is noise you scroll past. This list is a tree, and its order means
//!   something, so there is nowhere to put the noise. `rust` finds `r`,
//!   `u`, `s` and `t` in order inside `z-rush agent orchestrator` without
//!   trying. A leading `~` asks for fuzzy explicitly.
//! - **Match `Row::search`** — the branch or the session title — and
//!   nothing else. The display label carries tree glyphs and status dots,
//!   and the path carries most of the alphabet.
//! - **A node and its children travel together.** Matching a worktree
//!   keeps its sessions, and matching a session keeps the worktree that
//!   says where it lives.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use zrush_core::model::tree::{Row, RowKind};

/// Indices of the rows to keep, in order. An empty needle keeps everything.
pub fn apply(rows: &[Row], needle: &str) -> Vec<usize> {
    let raw = needle.trim();
    if raw.is_empty() {
        return (0..rows.len()).collect();
    }
    let fuzzy = raw.starts_with('~');
    let needle = raw.strip_prefix('~').unwrap_or(raw).trim();
    if needle.is_empty() {
        return (0..rows.len()).collect();
    }

    let mut hit = vec![false; rows.len()];
    if fuzzy {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(needle, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            if !r.search.is_empty() {
                hit[i] = pattern
                    .score(Utf32Str::new(&r.search, &mut buf), &mut matcher)
                    .is_some();
            }
        }
    } else {
        // Smart case, as every editor's search box does it: a needle typed
        // in lower case ignores case, one with a capital means it.
        let cased = needle.chars().any(char::is_uppercase);
        let lowered = needle.to_lowercase();
        for (i, r) in rows.iter().enumerate() {
            if r.search.is_empty() {
                continue;
            }
            hit[i] = if cased {
                r.search.contains(needle)
            } else {
                r.search.to_lowercase().contains(&lowered)
            };
        }
    }

    // Pull each node in when any of its children matched, and vice versa.
    let mut keep = hit.clone();
    let mut node = None;
    for i in 0..rows.len() {
        if rows[i].is_child() {
            if let Some(n) = node {
                if hit[i] {
                    keep[n] = true;
                }
                if hit[n] {
                    keep[i] = true;
                }
            }
        } else {
            node = Some(i);
        }
    }

    // A `[…more]` row says nothing on its own: it belongs to whatever node
    // is still on screen above it.
    let mut node_kept = false;
    (0..rows.len())
        .filter(|&i| {
            if !rows[i].is_child() {
                node_kept = keep[i];
            }
            if rows[i].kind == RowKind::More {
                return node_kept;
            }
            keep[i]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zrush_core::model::tree::NodeId;

    fn node(label: &str, location: &str) -> Row {
        Row {
            kind: RowKind::Worktree,
            node: NodeId::Worktree(location.into()),
            session_id: None,
            glyph: "▾ ".into(),
            label: label.into(),
            search: label.into(),
            badge: String::new(),
            status: String::new(),
            location: location.into(),
        }
    }

    fn child(title: &str, of: &str) -> Row {
        Row {
            kind: RowKind::Session,
            node: NodeId::Worktree(of.into()),
            session_id: Some(title.into()),
            glyph: String::new(),
            label: format!("   ├─ ● {title}"),
            search: title.into(),
            badge: String::new(),
            status: String::new(),
            location: String::new(),
        }
    }

    /// The shape that exposed both bugs: a repo whose path contains r, u, s
    /// and t in order, and sessions whose titles do too.
    fn real() -> Vec<Row> {
        vec![
            node("main [root]", "/Users/christphe/projects/lafourche/wt"),
            child(
                "z-rush agent orchestrator",
                "/Users/christphe/projects/lafourche/wt",
            ),
            child(
                "Publier zrush sur GitHub",
                "/Users/christphe/projects/lafourche/wt",
            ),
            node(
                "rust",
                "/Users/christphe/projects/lafourche/wt/.claude/worktrees/rust",
            ),
            child(
                "Conversion en Rust",
                "/Users/christphe/projects/lafourche/wt/.claude/worktrees/rust",
            ),
        ]
    }

    #[test]
    fn an_empty_needle_keeps_everything() {
        assert_eq!(apply(&real(), "  ").len(), 5);
    }

    #[test]
    fn the_path_is_not_searched() {
        // `/Users/christphe/projects/lafourche/wt` holds r, u, s and t in
        // order. Searching it meant every row matched every needle.
        let rows = vec![node("main", "/Users/christphe/projects/lafourche/wt")];
        assert!(apply(&rows, "rust").is_empty());
    }

    #[test]
    fn the_tree_glyphs_are_not_searched() {
        let rows = vec![child("nothing alike", "/w")];
        assert!(apply(&rows, "●").is_empty());
    }

    fn kept(needle: &str) -> Vec<String> {
        let rows = real();
        apply(&rows, needle)
            .into_iter()
            .map(|i| rows[i].search.clone())
            .collect()
    }

    /// The case that started this: every session in the repo is named after
    /// zrush, and `z-rush agent orchestrator` holds r, u, s and t in order.
    #[test]
    fn a_plain_needle_does_not_match_letters_scattered_through_a_title() {
        let k = kept("rust");
        assert!(
            !k.iter().any(|s| s.contains("z-rush")),
            "fuzzy leaked in: {k:?}"
        );
        assert!(
            !k.iter().any(|s| s.contains("Publier")),
            "fuzzy leaked in: {k:?}"
        );
    }

    #[test]
    fn a_matching_worktree_brings_its_sessions() {
        let k = kept("rust");
        assert!(
            k.contains(&"rust".to_string()),
            "the worktree itself: {k:?}"
        );
        assert!(
            k.contains(&"Conversion en Rust".to_string()),
            "and its session: {k:?}"
        );
        assert!(
            !k.contains(&"main [root]".to_string()),
            "not the other worktree: {k:?}"
        );
    }

    #[test]
    fn a_tilde_asks_for_fuzzy_explicitly() {
        let k = kept("~rust");
        assert!(
            k.iter().any(|s| s.contains("z-rush")),
            "~ should find letters in order: {k:?}"
        );
    }

    #[test]
    fn matching_ignores_case_until_you_type_a_capital() {
        assert!(kept("conversion").contains(&"Conversion en Rust".to_string()));
        assert!(kept("Conversion").contains(&"Conversion en Rust".to_string()));
        assert!(!kept("CONVERSION").contains(&"Conversion en Rust".to_string()));
    }

    #[test]
    fn a_matching_session_brings_its_worktree() {
        assert_eq!(
            apply(&real(), "Publier"),
            vec![0, 2],
            "the session, and its worktree"
        );
    }

    #[test]
    fn fuzzy_still_works_when_asked_for() {
        assert_eq!(apply(&real(), "~cnvrsn"), vec![3, 4]);
    }

    #[test]
    fn nothing_matching_yields_nothing() {
        assert!(apply(&real(), "zzzzzz").is_empty());
    }

    #[test]
    fn a_more_row_never_survives_its_worktree() {
        let mut rows = vec![node("main", "/w"), node("other", "/o")];
        rows.insert(
            1,
            Row {
                kind: RowKind::More,
                node: NodeId::Worktree("/w".into()),
                session_id: None,
                glyph: String::new(),
                label: "   └─ […more]".into(),
                search: String::new(),
                badge: "+3 older".into(),
                status: String::new(),
                location: String::new(),
            },
        );
        assert_eq!(apply(&rows, "other"), vec![2]);
    }

    #[test]
    fn a_more_row_stays_with_a_worktree_that_matched() {
        let mut rows = vec![node("main", "/w")];
        rows.push(Row {
            kind: RowKind::More,
            node: NodeId::Worktree("/w".into()),
            session_id: None,
            glyph: String::new(),
            label: "   └─ […more]".into(),
            search: String::new(),
            badge: "+3 older".into(),
            status: String::new(),
            location: String::new(),
        });
        assert_eq!(apply(&rows, "main"), vec![0, 1]);
    }
}
