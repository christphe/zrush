//! Fuzzy filtering, on the same engine fzf-like pickers use.
//!
//! A child row matching keeps its parent, so a filtered list still reads as
//! a tree rather than as a flat pile of session titles.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use zrush_core::model::tree::{Row, RowKind};

/// Indices of the rows to keep, in order. An empty needle keeps everything.
pub fn apply(rows: &[Row], needle: &str) -> Vec<usize> {
    if needle.trim().is_empty() {
        return (0..rows.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(needle, CaseMatching::Ignore, Normalization::Smart);

    let mut keep = vec![false; rows.len()];
    // The node a child belongs to, so a match can pull its parent back in.
    let mut parent: Option<usize> = None;
    for (i, r) in rows.iter().enumerate() {
        if !r.is_child() {
            parent = Some(i);
        }
        let haystack = format!("{} {} {}", r.label, r.badge, r.location);
        let mut buf = Vec::new();
        if pattern
            .score(
                nucleo_matcher::Utf32Str::new(&haystack, &mut buf),
                &mut matcher,
            )
            .is_some()
        {
            keep[i] = true;
            if r.is_child() {
                if let Some(p) = parent {
                    keep[p] = true;
                }
            }
        }
    }
    // A `[…more]` row on its own says nothing; drop it unless its node stayed.
    let mut parent_kept = false;
    (0..rows.len())
        .filter(|&i| {
            if !rows[i].is_child() {
                parent_kept = keep[i];
            }
            if rows[i].kind == RowKind::More {
                return keep[i] && parent_kept;
            }
            keep[i]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zrush_core::model::tree::NodeId;

    fn row(kind: RowKind, label: &str) -> Row {
        Row {
            kind,
            node: NodeId::Orphans,
            session_id: None,
            glyph: String::new(),
            label: label.into(),
            badge: String::new(),
            status: String::new(),
            location: String::new(),
        }
    }

    fn fixture() -> Vec<Row> {
        vec![
            row(RowKind::Worktree, "main"),
            row(RowKind::Session, "port zrush to rust"),
            row(RowKind::Worktree, "hotfix-tva"),
            row(RowKind::Session, "fix the vat rounding"),
        ]
    }

    #[test]
    fn an_empty_needle_keeps_everything() {
        assert_eq!(apply(&fixture(), "  ").len(), 4);
    }

    #[test]
    fn a_matching_worktree_is_kept() {
        assert_eq!(apply(&fixture(), "hotfix"), vec![2]);
    }

    #[test]
    fn a_matching_session_drags_its_worktree_back_in() {
        // Otherwise the row would float with nothing saying where it lives.
        assert_eq!(apply(&fixture(), "vat"), vec![2, 3]);
    }

    #[test]
    fn matching_is_fuzzy_and_case_insensitive() {
        assert_eq!(apply(&fixture(), "PRT"), vec![0, 1]);
    }

    #[test]
    fn nothing_matching_yields_nothing() {
        assert!(apply(&fixture(), "zzzzzz").is_empty());
    }

    #[test]
    fn a_more_row_never_survives_its_worktree() {
        let rows = vec![
            row(RowKind::Worktree, "main"),
            row(RowKind::More, "   └─ […more]"),
            row(RowKind::Worktree, "other"),
        ];
        assert_eq!(apply(&rows, "other"), vec![2]);
    }
}
