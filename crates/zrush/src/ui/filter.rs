//! Filtering the list.
//!
//! Fuzzy by default, with fzf's vocabulary for everything else — which is
//! the nearest thing to a convention, and where anyone reaching for `/`
//! here is coming from.
//!
//! | Typed     | Matches          |
//! |-----------|------------------|
//! | `rust`    | fuzzy            |
//! | `'rust`   | contains         |
//! | `=rust`   | is exactly       |
//! | `^rust`   | starts with      |
//! | `rust$`   | ends with        |
//! | `!rust`   | does not contain |
//!
//! Fuzzy is loose on short needles: `rust` finds `r`, `u`, `s` and `t` in
//! order inside `z-rush agent orchestrator`. fzf lives with that because it
//! ranks, and the noise sinks; this list is a tree whose order means
//! something, so the noise stays where it lands. `'` is the way out, and
//! the matched characters are marked so a surprising row explains itself.
//!
//! Two further rules, both learned by typing `/rust` and getting the whole
//! list back:
//!
//! - Match `Row::search` — the branch or the session title — and nothing
//!   else. The display label carries tree glyphs and status dots, and the
//!   path carries most of the alphabet, so a fuzzy needle finds anything
//!   in it.
//! - A node and its children travel together. Matching a worktree keeps its
//!   sessions, and matching a session keeps the worktree that says where it
//!   lives.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use zrush_core::model::tree::{Row, RowKind};

/// How a needle is read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Query<'a> {
    Fuzzy(&'a str),
    Contains(&'a str),
    Equals(&'a str),
    StartsWith(&'a str),
    EndsWith(&'a str),
    Excludes(&'a str),
}

impl<'a> Query<'a> {
    /// The prefix wins over the suffix, so `^x$` anchors the start. Nobody
    /// needs both, and picking one keeps the rule sayable.
    fn parse(raw: &'a str) -> Option<Self> {
        let raw = raw.trim();
        let q = match raw.as_bytes().first()? {
            b'\'' => Self::Contains(raw[1..].trim()),
            b'=' => Self::Equals(raw[1..].trim()),
            b'^' => Self::StartsWith(raw[1..].trim()),
            b'!' => Self::Excludes(raw[1..].trim()),
            _ => match raw.strip_suffix('$') {
                Some(rest) => Self::EndsWith(rest.trim()),
                None => Self::Fuzzy(raw),
            },
        };
        (!q.needle().is_empty()).then_some(q)
    }

    fn needle(&self) -> &'a str {
        match self {
            Self::Fuzzy(n)
            | Self::Contains(n)
            | Self::Equals(n)
            | Self::StartsWith(n)
            | Self::EndsWith(n)
            | Self::Excludes(n) => n,
        }
    }

    /// Smart case, as every editor's search box does it: a needle typed in
    /// lower case ignores case, one with a capital means it.
    fn cased(&self) -> bool {
        self.needle().chars().any(char::is_uppercase)
    }

    /// Where it matched in `hay`, as a character range, or `None`.
    /// `Excludes` marks nothing: there is no "this is why" to point at.
    fn find(&self, hay: &str) -> Option<std::ops::Range<usize>> {
        let (hay, needle) = if self.cased() {
            (hay.to_string(), self.needle().to_string())
        } else {
            (hay.to_lowercase(), self.needle().to_lowercase())
        };
        let chars = needle.chars().count();
        match self {
            Self::Contains(_) => {
                let at = hay.find(&needle)?;
                let from = hay[..at].chars().count();
                Some(from..from + chars)
            }
            Self::Equals(_) => (hay == needle).then_some(0..chars),
            Self::StartsWith(_) => hay.starts_with(&needle).then_some(0..chars),
            Self::EndsWith(_) => {
                let from = hay.chars().count().checked_sub(chars)?;
                hay.ends_with(&needle).then_some(from..from + chars)
            }
            Self::Excludes(_) => (!hay.contains(&needle)).then_some(0..0),
            Self::Fuzzy(_) => None,
        }
    }
}

/// A row that survived the filter, and which of its characters earned it.
///
/// `matched` is empty when the row was pulled in by its node or by one of
/// its children. Nothing else is needed to explain the list: a row with no
/// highlight is a row that came along with a neighbour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub index: usize,
    /// Character positions within `Row::search`.
    pub matched: Vec<usize>,
}

/// Just the indices, for callers that do not draw.
pub fn apply(rows: &[Row], needle: &str) -> Vec<usize> {
    hits(rows, needle).into_iter().map(|h| h.index).collect()
}

/// The rows to keep, in order, with what matched in each.
pub fn hits(rows: &[Row], needle: &str) -> Vec<Hit> {
    let Some(query) = Query::parse(needle) else {
        return (0..rows.len())
            .map(|index| Hit {
                index,
                matched: Vec::new(),
            })
            .collect();
    };

    let mut marks: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
    let mut hit = vec![false; rows.len()];

    if let Query::Fuzzy(n) = query {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(n, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let mut idx = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            if r.search.is_empty() {
                continue;
            }
            idx.clear();
            let haystack = Utf32Str::new(&r.search, &mut buf);
            if pattern.indices(haystack, &mut matcher, &mut idx).is_some() {
                hit[i] = true;
                idx.sort_unstable();
                idx.dedup();
                marks[i] = idx.iter().map(|c| *c as usize).collect();
            }
        }
    } else {
        for (i, r) in rows.iter().enumerate() {
            if r.search.is_empty() {
                continue;
            }
            if let Some(range) = query.find(&r.search) {
                hit[i] = true;
                marks[i] = range.collect();
            }
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
        .map(|index| Hit {
            index,
            matched: std::mem::take(&mut marks[index]),
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
        // order, so with fuzzy matching every row matched every needle.
        let rows = vec![node("main", "/Users/christphe/projects/lafourche/wt")];
        assert!(apply(&rows, "rust").is_empty());
    }

    #[test]
    fn the_tree_glyphs_are_not_searched() {
        let rows = vec![child("nothing alike", "/w")];
        assert!(apply(&rows, "'●").is_empty());
    }

    fn kept(needle: &str) -> Vec<String> {
        let rows = real();
        apply(&rows, needle)
            .into_iter()
            .map(|i| rows[i].search.clone())
            .collect()
    }

    /// The default is loose on purpose, and the marks are what make it
    /// legible: `z-rush agent orchestrator` really does hold r, u, s and t
    /// in order.
    #[test]
    fn the_default_is_fuzzy() {
        let k = kept("rust");
        assert!(k.iter().any(|s| s.contains("z-rush")), "not fuzzy: {k:?}");
    }

    #[test]
    fn a_quote_asks_for_a_plain_substring() {
        let k = kept("'rust");
        assert!(
            k.contains(&"rust".to_string()),
            "the worktree itself: {k:?}"
        );
        assert!(
            k.contains(&"Conversion en Rust".to_string()),
            "and its session: {k:?}"
        );
        assert!(
            !k.iter().any(|s| s.contains("z-rush")),
            "fuzzy leaked in: {k:?}"
        );
    }

    #[test]
    fn a_matching_worktree_brings_its_sessions() {
        let k = kept("'rust");
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
    fn equals_means_exactly_that_and_not_contains() {
        // fzf's `=`: the whole name, not a piece of it.
        assert!(kept("=rust").contains(&"rust".to_string()));
        assert!(!kept("=rus").contains(&"rust".to_string()));
        assert!(kept("=Conversion en Rust").contains(&"Conversion en Rust".to_string()));
    }

    #[test]
    fn a_caret_anchors_the_start() {
        assert!(kept("^Publier").contains(&"Publier zrush sur GitHub".to_string()));
        assert!(!kept("^zrush").contains(&"Publier zrush sur GitHub".to_string()));
    }

    #[test]
    fn a_dollar_anchors_the_end() {
        assert!(kept("GitHub$").contains(&"Publier zrush sur GitHub".to_string()));
        assert!(!kept("Publier$").contains(&"Publier zrush sur GitHub".to_string()));
    }

    #[test]
    fn a_bang_excludes() {
        let k = kept("!rust");
        assert!(
            !k.contains(&"rust".to_string()),
            "the worktree named rust is out: {k:?}"
        );
        assert!(k.contains(&"Publier zrush sur GitHub".to_string()));
    }

    #[test]
    fn an_exclusion_marks_nothing() {
        // There is no "this is why" to point at when the reason is absence.
        let rows = real();
        let h = hits(&rows, "!zzz");
        assert!(h.iter().all(|x| x.matched.is_empty()));
    }

    #[test]
    fn a_prefix_on_its_own_filters_nothing() {
        assert_eq!(apply(&real(), "=").len(), 5);
        assert_eq!(apply(&real(), "'  ").len(), 5);
        assert_eq!(apply(&real(), "$").len(), 5);
    }

    #[test]
    fn matching_ignores_case_until_you_type_a_capital() {
        assert!(kept("'conversion").contains(&"Conversion en Rust".to_string()));
        assert!(kept("'Conversion").contains(&"Conversion en Rust".to_string()));
        assert!(!kept("'CONVERSION").contains(&"Conversion en Rust".to_string()));
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
    fn fuzzy_finds_letters_in_order() {
        assert_eq!(apply(&real(), "cnvrsn"), vec![3, 4]);
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
