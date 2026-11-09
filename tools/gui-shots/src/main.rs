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
    for theme in THEMES {
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
        .with_pixels_per_point(1.0)
        .build_eframe(|_cc| a);
    // Wait for the scan: sessions listed under their worktrees.
    settle(&mut h, |h| {
        h.state()
            .tabs()
            .any(|t| t.snap.scanned && !t.rows().is_empty())
    });
    save(&mut h, out, &format!("{theme}-1-main"));

    // The newest session's conversation.
    let session = h.state().tabs().find_map(|t| {
        t.rows()
            .iter()
            .find(|r| r.kind == RowKind::Session)
            .map(|r| r.search.clone())
    });
    if let Some(title) = session
        && let Some(node) = h.query_by_label_contains(&title)
    {
        node.click();
        settle(&mut h, |h| h.query_all_by_label("Agent").next().is_some());
        save(&mut h, out, &format!("{theme}-2-conversation"));
    }

    h.state_mut().open_settings();
    h.run_steps(4);
    save(&mut h, out, &format!("{theme}-3-settings"));
}
