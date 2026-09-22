//! Keeping the README honest.
//!
//! A screenshot typed by hand drifts: the one this replaced still showed a
//! logo that had been redrawn, keys that had been renamed and counts in a
//! format that no longer existed. It is rendered by the real drawing code
//! instead, and the flags it advertises are checked against clap.

// A test that cannot expect is a test that hides its own failure.
#![allow(clippy::expect_used)]

use std::path::Path;

const FENCE: &str = "```";

fn readme() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    std::fs::read_to_string(path).expect("README.md")
}

/// Every long flag the README advertises still exists.
///
/// The wording is the README's own — shorter than clap's, and worth
/// keeping — but a flag that was renamed or dropped must not survive here.
#[test]
fn the_readme_advertises_flags_that_exist() {
    use clap::CommandFactory;

    let text = readme();
    let help = zrush::cli::Cli::command().render_long_help().to_string();

    // Only the usage block: other fenced blocks quote git's flags, not ours.
    let at = text.find("zrush [OPTIONS]").expect("the usage block");
    let block = &text[at..at + text[at..].find(FENCE).expect("its closing fence")];

    let mut checked = 0;
    for word in block.split_whitespace() {
        let flag = word.trim_matches(|c: char| !c.is_ascii_graphic() || c == ',');
        if !flag.starts_with("--") || flag.len() < 4 {
            continue;
        }
        assert!(
            help.contains(flag),
            "the README offers {flag}, which zrush does not"
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "only {checked} flags checked; did the block move?"
    );
}

/// Unix only. The screenshot holds paths, and a path is drawn with the
/// platform's separator: the same frame reads `~/wt` here and `~\wt` on
/// Windows. One platform catching drift is enough, and the file is
/// committed from this one.
#[cfg(unix)]
mod screenshot {
    use std::path::Path;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use zrush::ui::app::App;
    use zrush::ui::screen;
    use zrush_core::agent::{Role, Turn};
    use zrush_core::app::Zrush;
    use zrush_core::config::{Config, dirs_under};
    use zrush_core::host::RecordingHost;
    use zrush_core::model::{Session, SessionKind};

    use super::{FENCE, readme};

    const WIDTH: u16 = 150;
    const HEIGHT: u16 = 13;

    fn git(cwd: &Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("git");
        assert!(st.success(), "git {args:?} failed");
    }

    fn session(id: &str, title: &str, cwd: &Path, kind: SessionKind, age: &str) -> Session {
        Session {
            agent: "claude",
            id: id.into(),
            title: title.into(),
            status: match kind {
                SessionKind::Live => format!("busy {age}"),
                SessionKind::Resumable => format!("resumable {age}"),
            },
            cwd: cwd.to_path_buf(),
            launch_cwd: cwd.to_path_buf(),
            kind,
            last_activity: 0,
        }
    }

    fn render() -> String {
        let td = tempfile::TempDir::new().expect("tempdir");
        let base = zrush_core::paths::real(td.path());
        let repo = base.join("wt");
        git(&base, &["init", "-q", repo.to_str().expect("utf8")]);
        git(&repo, &["config", "user.email", "t@example.invalid"]);
        git(&repo, &["config", "user.name", "zrush"]);
        std::fs::write(repo.join("f.txt"), "seed").expect("write");
        git(&repo, &["add", "f.txt"]);
        git(&repo, &["commit", "-qm", "init"]);
        git(&repo, &["branch", "-M", "main"]);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                ".claude/worktrees/rust",
                "-b",
                "rust",
            ],
        );

        let z = Zrush::new(
            dirs_under(td.path()),
            Config::default(),
            repo.clone(),
            zrush_core::agent::all(),
            Box::new(RecordingHost::default()),
            None,
        )
        .expect("zrush");

        let rust = repo.join(".claude/worktrees/rust");
        let mut app = App::new(20, 5);
        app.active_agent = "claude".into();
        app.agent_names = vec!["claude".into()];
        app.worktrees = z.worktrees().expect("worktrees");
        app.live = vec![
            session("a", "refonte du picker", &repo, SessionKind::Live, "4m"),
            session("b", "port ratatui", &rust, SessionKind::Live, "0m"),
        ];
        app.resumable = vec![session(
            "c",
            "fix CI zip",
            &repo,
            SessionKind::Resumable,
            "2d",
        )];
        app.scanned = true;
        app.cursor = 3;
        app.home = Some(base);
        screen::rebuild(&mut app, &z);

        // The preview is fetched on a worker, so nothing would be in it
        // here. Seed the row the cursor is on: an empty pane says nothing
        // about what the pane is for.
        if let Some(key) = app.current().and_then(App::preview_key) {
            app.previews.insert(
                key,
                zrush::ui::preview::Preview::Conversation(vec![
                    Turn {
                        role: Role::User,
                        text: "port it to rust".into(),
                    },
                    Turn {
                        role: Role::Assistant,
                        text: "On y va. Le port garde tout ce que faisait la version sh.".into(),
                    },
                ]),
            );
        }

        let mut term = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
        let preview = screen::preview_for(&app);
        term.draw(|f| screen::draw(f, &app, &z, &preview))
            .expect("draw");

        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            let mut row = String::new();
            for x in 0..buf.area.width {
                row.push_str(buf[(x, y)].symbol());
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_readme_shows_what_zrush_draws() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
        let text = readme();
        let frame = render();

        let start = text.find(FENCE).expect("a fenced block");
        let body_at = start + FENCE.len() + 1;
        let end = text[body_at..].find(FENCE).expect("its closing fence") + body_at;
        let current = &text[body_at..end];

        if std::env::var_os("UPDATE_README").is_some() {
            let updated = format!("{}{frame}{}", &text[..body_at], &text[end..]);
            std::fs::write(&path, updated).expect("write");
            return;
        }

        assert_eq!(
            current.trim_end(),
            frame.trim_end(),
            "the README's screenshot has drifted. UPDATE_README=1 cargo test -p zrush \
             --test readme"
        );
    }
}
