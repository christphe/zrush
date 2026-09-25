//! Where a conversation is opened, as opposed to what is opened.
//!
//! `Agent` says what a session is and how to resume it; `Host` says where
//! things happen. Neither can answer one question on its own: an editor
//! that has a native surface for a *particular* agent — the Claude
//! extension for VS Code, and nothing else so far — is addressed by a
//! string that mixes both sides, `vscode://anthropic.claude-code/…`.
//!
//! So the pair gets a port of its own. The implementations are shared
//! between pairs rather than written per pair:
//!
//! - [`Terminal`] composes the two ports that already exist — the agent's
//!   command, the host's terminal — and works for every pair, known or
//!   not. It is what a pair falls back to, and it is never wrong, only
//!   plainer.
//! - [`Uri`] carries the one string that neither port can produce.
//!
//! A pair with no configured surface is not a hole: it is [`Terminal`].

use std::path::Path;

use crate::agent::Agent;
use crate::error::Result;
use crate::host::Host;

/// The conversation to open, and where it lives.
pub struct SessionRef<'a> {
    pub worktree: &'a Path,
    pub agent: &'a dyn Agent,
    /// `None` starts a fresh conversation rather than resuming one.
    pub session_id: Option<&'a str>,
}

pub trait Surface: Send + Sync {
    /// How the config names this kind.
    fn id(&self) -> &'static str;

    /// What opening it would do, for `zrush tab --print`. A surface whose
    /// target cannot be read before it acts is not one you can debug.
    fn describe(&self, session: &SessionRef<'_>) -> String;

    fn open(&self, session: &SessionRef<'_>, host: &dyn Host) -> Result<()>;
}

/// The agent's own command, in a terminal the host provides.
///
/// `command` overrides what the agent would have said, for the case where
/// a machine wants its own flags; unset means ask the agent, which is what
/// makes this work for an agent this build has never heard of.
#[derive(Debug, Default, Clone)]
pub struct Terminal {
    pub command: Option<String>,
}

impl Terminal {
    fn command_for(&self, session: &SessionRef<'_>) -> Vec<String> {
        match (&self.command, session.session_id) {
            (Some(t), _) => expand(t, session)
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            (None, Some(id)) => session.agent.resume_command(id),
            (None, None) => session.agent.start_command(),
        }
    }
}

impl Surface for Terminal {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn describe(&self, session: &SessionRef<'_>) -> String {
        self.command_for(session).join(" ")
    }

    fn open(&self, session: &SessionRef<'_>, host: &dyn Host) -> Result<()> {
        host.run_agent(session.worktree, &self.command_for(session))
    }
}

/// A URL the host hands to the desktop, which routes it to whatever
/// registered that scheme. `{id}` and `{agent}` are filled in.
#[derive(Debug, Clone)]
pub struct Uri {
    pub template: String,
}

impl Surface for Uri {
    fn id(&self) -> &'static str {
        "uri"
    }

    fn describe(&self, session: &SessionRef<'_>) -> String {
        self.url(session)
    }

    fn open(&self, session: &SessionRef<'_>, host: &dyn Host) -> Result<()> {
        host.open_url(&self.url(session))
    }
}

impl Uri {
    fn url(&self, session: &SessionRef<'_>) -> String {
        drop_empty_params(&expand(&self.template, session))
    }
}

/// `session=` with nothing after it is not the same as no `session` at
/// all: the Claude extension validates what it was given, fails, and
/// returns without opening anything. An empty parameter means "I have no
/// value for this", which in a URL is said by leaving it out.
fn drop_empty_params(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.ends_with('='))
        .collect();
    if kept.is_empty() {
        return base.to_string();
    }
    format!("{base}?{}", kept.join("&"))
}

/// `{id}` and `{agent}`, and nothing else: a template is config, and a
/// config that can run arbitrary substitutions is a config that surprises.
///
/// An absent session leaves `{id}` empty, which is what an extension's
/// "open a fresh one" looks like.
fn expand(template: &str, session: &SessionRef<'_>) -> String {
    template
        .replace("{id}", session.session_id.unwrap_or_default())
        .replace("{agent}", session.agent.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::RecordingHost;

    struct Fake;

    impl Agent for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn available(&self) -> bool {
            true
        }
        fn valid_id(&self, _id: &str) -> bool {
            true
        }
        fn live(
            &self,
            _d: &crate::config::Dirs,
            _c: &crate::config::Config,
        ) -> Vec<crate::model::Session> {
            Vec::new()
        }
        fn resumable(
            &self,
            _d: &crate::config::Dirs,
            _c: &crate::config::Config,
            _s: &crate::agent::Scope<'_>,
        ) -> Vec<crate::model::Session> {
            Vec::new()
        }
        fn history_count(&self, _d: &crate::config::Dirs, _w: &Path) -> usize {
            0
        }
        fn preview(
            &self,
            _d: &crate::config::Dirs,
            _i: &str,
            _t: usize,
        ) -> Vec<crate::agent::Turn> {
            Vec::new()
        }
        fn delete(&self, _d: &crate::config::Dirs, _i: &str) -> Result<usize> {
            Ok(0)
        }
        fn resume_command(&self, id: &str) -> Vec<String> {
            vec!["fake".into(), "--resume".into(), id.into()]
        }
        fn start_command(&self) -> Vec<String> {
            vec!["fake".into()]
        }
    }

    fn session<'a>(id: Option<&'a str>, agent: &'a dyn Agent) -> SessionRef<'a> {
        SessionRef {
            worktree: Path::new("/repo"),
            agent,
            session_id: id,
        }
    }

    /// The point of the terminal surface: it knows no pair. What to run
    /// comes from the agent, where to run it from the host.
    #[test]
    fn a_terminal_asks_the_agent_what_to_run() {
        let host = RecordingHost::default();
        Terminal::default()
            .open(&session(Some("abc"), &Fake), &host)
            .unwrap();
        Terminal::default()
            .open(&session(None, &Fake), &host)
            .unwrap();

        let ran = host.ran.lock().unwrap();
        assert_eq!(ran[0].1, vec!["fake", "--resume", "abc"]);
        assert_eq!(ran[1].1, vec!["fake"], "no session means a fresh one");
    }

    #[test]
    fn a_terminal_command_overrides_the_agent() {
        let host = RecordingHost::default();
        let s = Terminal {
            command: Some("fake --resume {id} --yolo".into()),
        };
        s.open(&session(Some("abc"), &Fake), &host).unwrap();
        assert_eq!(
            host.ran.lock().unwrap()[0].1,
            vec!["fake", "--resume", "abc", "--yolo"]
        );
    }

    #[test]
    fn describe_says_what_open_would_do() {
        let s = session(Some("abc"), &Fake);
        assert_eq!(Terminal::default().describe(&s), "fake --resume abc");
        assert_eq!(
            Uri {
                template: "x://open?session={id}".into()
            }
            .describe(&s),
            "x://open?session=abc"
        );
    }

    #[test]
    fn a_uri_fills_in_the_session_and_the_agent() {
        let host = RecordingHost::default();
        let s = Uri {
            template: "x://{agent}/open?session={id}".into(),
        };
        s.open(&session(Some("abc"), &Fake), &host).unwrap();
        assert_eq!(
            host.opened_urls.lock().unwrap()[0],
            "x://fake/open?session=abc"
        );
    }

    /// No binding yet is a real case, and `session=` with nothing after it
    /// is worse than no parameter: the Claude extension validates it,
    /// fails, and returns without opening anything or saying so.
    #[test]
    fn a_uri_with_no_session_drops_the_parameter_rather_than_emptying_it() {
        let host = RecordingHost::default();
        let s = Uri {
            template: "x://open?session={id}".into(),
        };
        s.open(&session(None, &Fake), &host).unwrap();
        assert_eq!(host.opened_urls.lock().unwrap()[0], "x://open");
    }

    #[test]
    fn only_the_empty_parameters_go() {
        let s = Uri {
            template: "x://open?a={id}&agent={agent}&b=1".into(),
        };
        assert_eq!(s.describe(&session(None, &Fake)), "x://open?agent=fake&b=1");
        assert_eq!(
            s.describe(&session(Some("abc"), &Fake)),
            "x://open?a=abc&agent=fake&b=1"
        );
    }

    #[test]
    fn a_url_with_no_query_is_left_alone() {
        let s = Uri {
            template: "x://open/{agent}".into(),
        };
        assert_eq!(s.describe(&session(None, &Fake)), "x://open/fake");
    }
}
