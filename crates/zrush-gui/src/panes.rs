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
    if let Some(e) = tab.error.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, e);
            if ui.small_button("✕").clicked() {
                tab.error = None;
            }
        });
    }
    if let Some(f) = tab.flash.clone() {
        flash(ui, &tab.repo, &f);
    }
    // A tab whose repository could not be opened has only its error.
    tab.zrush()?;
    let mut asked = None;
    let palette = crate::theme::palette(ui.ctx());
    let side = palette.map_or(ui.visuals().faint_bg_color, |p| p.panel_bg);
    let main = palette.map_or(ui.visuals().panel_fill, |p| p.window_bg);
    egui::Panel::left(egui::Id::new(("tree", &tab.repo)))
        .resizable(true)
        .default_size(380.0)
        .size_range(260.0..=700.0)
        .frame(
            egui::Frame::new()
                .fill(side)
                .inner_margin(egui::Margin::symmetric(10, 10)),
        )
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

/// How long a flash line stays up.
const FLASH_FOR: Duration = Duration::from_secs(5);

/// Show `text` for `FLASH_FOR` from the first frame it appeared in.
fn flash(ui: &mut egui::Ui, repo: &std::path::Path, text: &str) {
    let id = egui::Id::new(("flash", repo));
    let now = Instant::now();
    let since = ui.ctx().data_mut(|d| {
        let seen: Option<(String, Instant)> = d.get_temp(id);
        match seen {
            Some((t, at)) if t == text => at,
            _ => {
                d.insert_temp(id, (text.to_string(), now));
                now
            }
        }
    });
    let shown = now.duration_since(since);
    if shown < FLASH_FOR {
        ui.weak(text);
        ui.ctx()
            .request_repaint_after(FLASH_FOR.saturating_sub(shown));
    }
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
}
