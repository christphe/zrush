//! A tab's body: the worktree tree on the left, what is selected on the
//! right.

use std::time::{Duration, Instant};

use egui_commonmark::CommonMarkCache;
use zrush_core::intent::Question;

use crate::tab::Tab;
use crate::{conversation, tree_view};

/// Draw the tab. A question one of its clicks raised comes back to be put
/// up by the window.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, md: &mut CommonMarkCache) -> Option<Question> {
    // A tab whose repository could not be opened has only its error.
    if tab.zrush().is_none() {
        if let Some(e) = &tab.error {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.colored_label(ui.visuals().error_fg_color, e);
            });
        }
        return None;
    }
    // What an action said, over the tab rather than pushing it down: a
    // success fades on its own, a failure stays until dismissed.
    let area = ui.max_rect();
    if let Some(e) = tab.error.clone()
        && toast(ui.ctx(), area, &tab.repo, &e)
    {
        tab.error = None;
    } else if let Some(f) = tab.flash.clone() {
        flash(ui.ctx(), area, &tab.repo, &f);
    }
    let mut asked = None;
    let palette = crate::theme::palette(ui.ctx());
    let side = palette.map_or(ui.visuals().faint_bg_color, |p| p.panel_bg);
    let main = palette.map_or(ui.visuals().panel_fill, |p| p.window_bg);
    egui::Panel::left(egui::Id::new(("tree", &tab.repo)))
        .resizable(true)
        .default_size(380.0)
        .size_range(260.0..=700.0)
        .frame(egui::Frame::new().fill(side).inner_margin(egui::Margin {
            left: 14,
            right: 12,
            top: 12,
            bottom: 10,
        }))
        .show(ui, |ui| asked = tree_view::show(ui, tab));
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(main)
                .inner_margin(egui::Margin::symmetric(24, 16)),
        )
        .show(ui, |ui| {
            if let Some(q) = conversation::show(ui, tab, md) {
                asked = Some(q);
            }
        });
    asked
}

/// How long a success stays up, and how long it takes to fade.
const FLASH_FOR: Duration = Duration::from_secs(4);
const FADE: f32 = 0.4;

/// Show `text` for `FLASH_FOR` from the first frame it appeared in, fading
/// out at the end.
fn flash(ctx: &egui::Context, area: egui::Rect, repo: &std::path::Path, text: &str) {
    let id = egui::Id::new(("flash", repo));
    let now = Instant::now();
    let since = ctx.data_mut(|d| {
        let seen: Option<(String, Instant)> = d.get_temp(id);
        match seen {
            Some((t, at)) if t == text => at,
            _ => {
                d.insert_temp(id, (text.to_string(), now));
                now
            }
        }
    });
    let left = FLASH_FOR.as_secs_f32() - now.duration_since(since).as_secs_f32();
    if left <= 0.0 {
        return;
    }
    let opacity = (left / FADE).min(1.0);
    egui::Area::new(egui::Id::new(("toast", repo)))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
        .constrain_to(area)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            card(ui, text, false);
        });
    if opacity < 1.0 {
        ctx.request_repaint();
    } else {
        ctx.request_repaint_after(Duration::from_secs_f32(left - FADE));
    }
}

/// A failure, until its ✕ is clicked: true then.
fn toast(ctx: &egui::Context, area: egui::Rect, repo: &std::path::Path, text: &str) -> bool {
    egui::Area::new(egui::Id::new(("toast", repo)))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
        .constrain_to(area)
        .show(ctx, |ui| card(ui, text, true))
        .inner
}

/// The toast itself: a dot in the outcome's colour, the line, said as a
/// sentence; a failure has its ✕. True when that was clicked.
fn card(ui: &mut egui::Ui, text: &str, failed: bool) -> bool {
    let v = ui.visuals().clone();
    let colour = if failed {
        v.error_fg_color
    } else {
        crate::theme::palette(ui.ctx()).map_or(v.selection.bg_fill, |p| p.live)
    };
    let mut dismissed = false;
    egui::Frame::new()
        .fill(v.window_fill)
        .stroke(v.window_stroke)
        .corner_radius(10)
        .shadow(v.popup_shadow)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_max_width(520.0);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, colour);
                ui.add_space(4.0);
                ui.add(egui::Label::new(sentence(text)).wrap());
                if failed {
                    ui.add_space(6.0);
                    dismissed = ui
                        .add(egui::Button::new("✕").frame_when_inactive(false).small())
                        .on_hover_text("Dismiss")
                        .clicked();
                }
            });
        });
    dismissed
}

/// The line as a sentence: zrush's lines start in lower case, as a
/// terminal's status line does.
fn sentence(text: &str) -> String {
    let mut c = text.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use zrush_core::host::{Host, RecordingHost};

    use super::*;
    use crate::tab::Tab;
    use crate::testing::{SESSION_ID, SESSION_TITLE, scratch_repo, with_session};

    /// Lets a test keep a handle on the host the tab's service owns.
    struct Shared(Arc<RecordingHost>);

    impl Host for Shared {
        fn open_workspace(&self, path: &std::path::Path) -> zrush_core::error::Result<()> {
            self.0.open_workspace(path)
        }
        fn run_agent(
            &self,
            cwd: &std::path::Path,
            command: &[String],
        ) -> zrush_core::error::Result<()> {
            self.0.run_agent(cwd, command)
        }
        fn set_editor(&self, command: Vec<String>) {
            self.0.set_editor(command);
        }
    }

    struct State {
        tab: Tab,
        md: egui_commonmark::CommonMarkCache,
        asked: Arc<Mutex<Option<zrush_core::intent::Question>>>,
    }

    fn harness(
        td: &tempfile::TempDir,
        repo: &std::path::Path,
    ) -> (Harness<'static, State>, Arc<RecordingHost>) {
        let host = Arc::new(RecordingHost::default());
        let tab = crate::testing::open_tab(td, repo, Box::new(Shared(Arc::clone(&host))));
        let state = State {
            tab,
            md: egui_commonmark::CommonMarkCache::default(),
            asked: Arc::new(Mutex::new(None)),
        };
        let h = Harness::builder()
            .with_size([1200.0, 800.0])
            .build_ui_state(
                |ui, s: &mut State| {
                    s.tab.poll();
                    if let Some(q) = show(ui, &mut s.tab, &mut s.md) {
                        *s.asked.lock().unwrap() = Some(q);
                    }
                },
                state,
            );
        (h, host)
    }

    fn until(h: &mut Harness<'static, State>, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.query_all_by_label_contains(what).next().is_none() && Instant::now() < deadline {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            h.query_all_by_label_contains(what).next().is_some(),
            "never showed {what}"
        );
    }

    /// The editor is started off the window's thread: wait for it.
    fn opened(h: &mut Harness<'static, State>, host: &RecordingHost) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.opened.lock().unwrap().is_empty() && Instant::now() < deadline {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_tree_lists_worktrees_and_their_sessions() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        with_session(&td, &repo, &["port it to rust", "On y va."]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "main");
        until(&mut h, SESSION_TITLE);
    }

    #[test]
    fn a_detached_worktree_is_named_by_its_folder() {
        // Two detached worktrees would both read "(detached)".
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::git(&repo, &["worktree", "add", "-q", "--detach", "../loose"]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "loose");
        assert!(h.query_by_label("detached").is_some());
        assert!(h.query_by_label("(detached)").is_none());
    }

    #[test]
    fn a_long_branch_name_stops_before_the_status() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let long = "feature/2216_emanager_circuit_breaker_on_http_client_with_a_very_long_name";
        crate::testing::git(&repo, &["checkout", "-q", "-b", long]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "feature/2216");
        // A branch with no upstream: its status says so, once git answers.
        until(&mut h, "⚠");
        h.run_steps(3);
        let name = h.get_by_label_contains("feature/2216").rect();
        let status = h.get_by_label_contains("⚠").rect();
        assert!(
            name.max.x <= status.min.x,
            "the name runs under the status: {name:?} vs {status:?}"
        );
    }

    #[test]
    fn the_filter_keeps_what_matches() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        with_session(&td, &repo, &["port it to rust", "On y va."]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, SESSION_TITLE);
        h.state_mut().tab.filter = "zzz".into();
        h.run_steps(3);
        assert!(h.query_by_label_contains(SESSION_TITLE).is_none());
        assert!(h.query_by_label("No match").is_some());
        h.state_mut().tab.filter = "rust".into();
        h.run_steps(3);
        assert!(h.query_by_label_contains(SESSION_TITLE).is_some());
    }

    #[test]
    fn a_filter_opens_a_folded_worktree() {
        // A worktree that is neither the main one nor running anything
        // starts folded; a match under it must show all the same.
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::git(&repo, &["worktree", "add", "-q", "-b", "side", "../side"]);
        let side = zrush_core::paths::real(&repo.join("../side"));
        with_session(&td, &side, &["port it to rust", "On y va."]);
        let (mut h, _) = harness(&td, &repo);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !h
            .state()
            .tab
            .rows()
            .iter()
            .any(|r| r.search == SESSION_TITLE)
            && Instant::now() < deadline
        {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains(SESSION_TITLE).is_none(), "folded");
        h.state_mut().tab.filter = "rust".into();
        h.run_steps(3);
        assert!(h.query_by_label_contains(SESSION_TITLE).is_some());
    }

    fn pending_confirm(h: &Harness<'static, State>) -> bool {
        matches!(h.state().tab.pending, Some(crate::tab::Pending::Confirm(_)))
    }

    #[test]
    fn right_click_on_a_session_offers_its_actions() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        with_session(&td, &repo, &["port it to rust", "On y va."]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, SESSION_TITLE);
        h.get_by_label_contains(SESSION_TITLE).click_secondary();
        h.run_steps(3);
        for item in [
            "Open in IDE",
            "Resume in a terminal",
            "Move to a new worktree",
            "Copy path",
            "Delete conversation",
        ] {
            assert!(
                h.query_all_by_label(item).next().is_some(),
                "no {item} in the menu"
            );
        }
        h.get_by_label("Delete conversation").click_accesskit();
        h.run_steps(3);
        assert!(pending_confirm(&h));
    }

    #[test]
    fn right_click_on_a_worktree_offers_its_actions() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::git(&repo, &["worktree", "add", "-q", "-b", "side", "../side"]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "side");
        h.get_by_label("side").click_secondary();
        h.run_steps(3);
        for item in [
            "Open in IDE",
            "Agent in a terminal",
            "New worktree",
            "Copy path",
            "Remove worktree",
        ] {
            assert!(
                h.query_all_by_label(item).next().is_some(),
                "no {item} in the menu"
            );
        }
        h.get_by_label("Remove worktree").click_accesskit();
        h.run_steps(3);
        assert!(pending_confirm(&h));
    }

    #[test]
    fn the_tree_has_a_new_worktree_button() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "main");
        h.get_by_label("+").click_accesskit();
        h.run_steps(3);
        assert!(matches!(
            h.state().tab.pending,
            Some(crate::tab::Pending::Branch {
                hand_over: None,
                ..
            })
        ));
    }

    #[test]
    fn purge_mode_ticks_rows_and_asks_before_removing() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::git(&repo, &["worktree", "add", "-q", "-b", "side", "../side"]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "side");
        h.get_by_label("🗑").click_accesskit();
        h.run_steps(3);
        assert!(h.state().tab.purging.is_some());
        until(&mut h, "Tick what goes");
        h.get_by_label("side").click_accesskit();
        h.run_steps(3);
        until(&mut h, "Purge 1");
        h.get_by_label_contains("Purge 1").click_accesskit();
        h.run_steps(3);
        assert!(pending_confirm(&h));
    }

    #[test]
    fn leaving_purge_mode_drops_the_ticks() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, "main");
        h.get_by_label("🗑").click_accesskit();
        h.run_steps(3);
        until(&mut h, "Tick what goes");
        h.get_by_label("Done").click_accesskit();
        h.run_steps(3);
        assert!(h.state().tab.purging.is_none());
    }

    #[test]
    fn clicking_a_session_shows_its_conversation() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        with_session(&td, &repo, &["port it to rust", "On y va."]);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, SESSION_TITLE);
        h.get_by_label_contains(SESSION_TITLE).click();
        until(&mut h, "On y va.");
        assert!(h.query_by_label_contains("port it to rust").is_some());
    }

    #[test]
    fn the_ide_button_opens_the_worktree_unbound() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let (mut h, host) = harness(&td, &repo);
        until(&mut h, "main");
        // It shows under the pointer only.
        h.get_by_label("main").hover();
        h.run_steps(2);
        h.get_by_label("Open").click();
        opened(&mut h, &host);
        assert_eq!(*host.opened.lock().unwrap(), vec![repo.clone()]);
        assert!(h.state().tab.zrush().unwrap().binding(&repo).is_none());
    }

    #[test]
    fn open_in_ide_binds_the_session() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        with_session(&td, &repo, &["port it to rust", "On y va."]);
        let (mut h, host) = harness(&td, &repo);
        until(&mut h, SESSION_TITLE);
        h.get_by_label_contains(SESSION_TITLE).click();
        until(&mut h, "Open in IDE");
        h.get_by_label("Open in IDE").click();
        opened(&mut h, &host);
        assert_eq!(*host.opened.lock().unwrap(), vec![repo.clone()]);
        let bound = h.state().tab.zrush().unwrap().binding(&repo).unwrap();
        assert_eq!(bound.id, SESSION_ID);
    }

    #[test]
    fn a_long_markdown_conversation_renders() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let code =
            "Here:\n\n```rust\nlet a  = 1;\nlet bc = 2;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |";
        let mut turns: Vec<&str> = Vec::new();
        for _ in 0..150 {
            turns.push("next?");
            turns.push(code);
        }
        turns.push("last question");
        turns.push("the very last answer");
        with_session(&td, &repo, &turns);
        let (mut h, _) = harness(&td, &repo);
        until(&mut h, SESSION_TITLE);
        h.get_by_label_contains(SESSION_TITLE).click();
        until(&mut h, "the very last answer");
    }

    #[test]
    fn a_line_is_said_as_a_sentence() {
        assert_eq!(super::sentence("removed side"), "Removed side");
        assert_eq!(super::sentence("élan"), "Élan");
        assert_eq!(super::sentence(""), "");
    }
}
