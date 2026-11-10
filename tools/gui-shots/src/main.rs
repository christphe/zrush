//! Render zrush-gui, as it is now, to PNG files: the main window, a
//! conversation, the settings — once per theme. Real repositories and
//! sessions, a recording host: nothing is opened, nothing is run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use zrush_core::config::{Config, Dirs};
use zrush_core::host::RecordingHost;
use zrush_core::model::SessionKind;
use zrush_core::model::tree::RowKind;
use zrush_gui::app::GuiApp;

const THEMES: &[&str] = &["vscode-dark", "vscode-light", "solarized-dark", "zed-light"];

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: gui-shots OUT_DIR [REPO]"));
    let repo = args
        .next()
        .map_or_else(|| std::env::current_dir().expect("cwd"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("out dir");
    let dirs = Dirs::from_env().expect("dirs");
    // ZRUSH_THEMES=a,b keeps those only.
    let only = std::env::var("ZRUSH_THEMES").ok();
    for theme in THEMES
        .iter()
        .filter(|t| only.as_deref().is_none_or(|o| o.split(',').any(|x| x == **t)))
    {
        shoot(&dirs, &repo, theme, &out);
    }
}

fn app(dirs: &Dirs, repo: &Path, theme: &str) -> GuiApp {
    let cfg = Config {
        gui_theme: (*theme).to_string(),
        ..Config::load(dirs).unwrap_or_default()
    };
    GuiApp::new(
        dirs.clone(),
        cfg,
        vec![repo.to_path_buf()],
        Box::new(|_| Box::new(RecordingHost::default())),
        Arc::new(|| {}),
    )
    .with_titlebar_tabs()
}

fn settle(h: &mut Harness<'_, GuiApp>, until: impl Fn(&Harness<'_, GuiApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !until(h) && Instant::now() < deadline {
        h.step();
        std::thread::sleep(Duration::from_millis(30));
    }
    h.run_steps(4);
}

fn save(h: &mut Harness<'_, GuiApp>, out: &Path, name: &str) {
    let img = h.render().expect("render");
    let path = out.join(format!("{name}.png"));
    img.save(&path).expect("save");
    println!("{}", path.display());
}

fn shoot(dirs: &Dirs, repo: &Path, theme: &str, out: &Path) {
    let a = app(dirs, repo, theme);
    let mut h = Harness::builder()
        .with_size([1400.0, 860.0])
        // ZRUSH_PPP=1 renders as a screen that is not Retina does.
        .with_pixels_per_point(
            std::env::var("ZRUSH_PPP")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(2.0),
        )
        .build_eframe(|_cc| a);
    // Wait for the scan: sessions listed under their worktrees.
    settle(&mut h, |h| {
        h.state()
            .tabs()
            .any(|t| t.snap.scanned && !t.rows().is_empty())
    });
    save(&mut h, out, &format!("{theme}-1-main"));

    // The newest session that is not running: one zrush would let you
    // delete, for the confirmation below.
    let session = h.state().tabs().find_map(|t| {
        t.rows()
            .iter()
            .filter(|r| r.kind == RowKind::Session)
            .find(|r| {
                r.session_id
                    .as_deref()
                    .and_then(|id| t.snap.session(id))
                    .is_some_and(|s| s.kind != SessionKind::Live)
            })
            .map(|r| r.search.clone())
    });
    if let Some(title) = session
        && let Some(node) = h.query_by_label_contains(&title)
    {
        node.click();
        settle(&mut h, |h| h.query_all_by_label("Agent").next().is_some());
        save(&mut h, out, &format!("{theme}-2-conversation"));

        // Its start: scrolled to the top, where the first things said are.
        let pane = egui::pos2(900.0, 500.0);
        h.hover_at(pane);
        for _ in 0..40 {
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 600.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        h.run_steps(4);
        save(&mut h, out, &format!("{theme}-2b-conversation-top"));

        // Its right click, then the confirmation. Escape answers Cancel:
        // nothing is deleted. The title is in the tree and now in the
        // pane's heading too: the tree's comes first.
        let opened = h
            .query_all_by_label_contains(&title)
            .next()
            .map(|n| n.click_secondary())
            .is_some();
        h.run_steps(3);
        let asked = opened
            && h
                .query_by_label("Delete conversation")
                .map(|n| n.click())
                .is_some();
        if asked {
            h.run_steps(4);
            save(&mut h, out, &format!("{theme}-4-delete-session"));
            h.key_press(egui::Key::Escape);
            h.run_steps(3);
        }
    }

    // A running session: its end, where both sides have spoken lately.
    let running = h.state().tabs().find_map(|t| {
        t.rows()
            .iter()
            .filter(|r| r.kind == RowKind::Session)
            .find(|r| {
                r.session_id
                    .as_deref()
                    .and_then(|id| t.snap.session(id))
                    .is_some_and(|s| s.kind == SessionKind::Live)
            })
            .map(|r| r.search.clone())
    });
    if let Some(title) = running {
        let clicked = h
            .query_all_by_label_contains(&title)
            .next()
            .map(|n| n.click())
            .is_some();
        if clicked {
            // Its own conversation, not the one still shown from before
            // the click was handled.
            settle(&mut h, |h| {
                h.state().tabs().any(|t| {
                    t.selected_row().is_some_and(|r| r.search == title)
                        && matches!(t.preview(), zrush_core::loader::Preview::Conversation(_))
                })
            });
            save(&mut h, out, &format!("{theme}-2c-running"));
        }
    }

    // A worktree with sessions: its answers are too long to sit side by
    // side. Put up directly, and closed the same way.
    if let Some(t) = h.state_mut().tabs_mut().next() {
        t.pending = Some(zrush_gui::tab::Pending::Confirm(
            zrush_gui::tab::Confirm::RemoveWorktree {
                path: repo.join("../side"),
                name: "feat/gui-tree-actions".into(),
                sessions: 3,
            },
        ));
    }
    h.run_steps(4);
    save(&mut h, out, &format!("{theme}-5-remove-worktree"));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    // What a removal says once done.
    if let Some(t) = h.state_mut().tabs_mut().next() {
        t.flash = Some("removed side, and its 3 conversations; 1 session still running, kept".into());
    }
    h.run_steps(3);
    save(&mut h, out, &format!("{theme}-7-toast"));

    // The palette, a few letters typed.
    h.state_mut().open_palette();
    h.run_steps(2);
    h.event(egui::Event::Text("gui".into()));
    h.run_steps(4);
    save(&mut h, out, &format!("{theme}-6-palette"));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);

    h.state_mut().open_settings();
    h.run_steps(4);
    save(&mut h, out, &format!("{theme}-3-settings"));
    // Further down: the switches and the fields.
    h.hover_at(egui::pos2(780.0, 500.0));
    for _ in 0..6 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(4);
    save(&mut h, out, &format!("{theme}-3b-settings-rows"));
}
