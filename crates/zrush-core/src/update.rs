//! Is there a newer release, and how to install it from here.
//!
//! The check is a convenience, never an obstacle: every way it can fail
//! (offline, rate-limited, a tag that does not parse) ends in silence, and
//! nothing is cached on failure so the next start tries again.
//!
//! Which installer runs is decided from the shell zrush is running in now,
//! not the one it was installed from: installed from PowerShell, launched
//! from Git Bash today. Both installers put everything in the same places,
//! so switching between them from one update to the next is safe.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Dirs;
use crate::error::{Result, ZrushError};

const REPO: &str = "christphe/zrush";
const LATEST: &str = "https://api.github.com/repos/christphe/zrush/releases/latest";

/// One check a day: GitHub allows 60 unauthenticated calls an hour.
pub const DAY: u64 = 24 * 60 * 60;

/// How long the check may take before it is given up on.
const TIMEOUT: Duration = Duration::from_secs(3);

/// What kind of build this is, decided by whoever compiled it: CI says
/// `release` on a tag and `nightly` on main; a build from a checkout says
/// nothing and is `dev`. Only a release offers updates — a nightly has no
/// version to compare, and a dev build is updated with git.
pub fn channel() -> &'static str {
    option_env!("ZRUSH_CHANNEL").unwrap_or("dev")
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A release version: `v1.2.3`, `cli-v1.2.3`, `1.2.3`, `1.2.3-rc.1`. Build metadata is
/// ignored, anything else is not a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: (u64, u64, u64),
    pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        // CLI releases are tagged `cli-vX.Y.Z` from 0.0.5 on, beside the
        // GUI's `gui-vX.Y.Z`; any other prefix is not the CLI's to install.
        let s = s
            .strip_prefix("cli-v")
            .or_else(|| s.strip_prefix('v'))
            .unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let major = parts.next()??;
        let minor = parts.next()??;
        let patch = parts.next()??;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            core: (major, minor, patch),
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            // A pre-release comes before the release it leads to.
            match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// What the last check found, and the one release the user said to skip.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Cache {
    /// Seconds since the epoch.
    #[serde(default)]
    checked: u64,
    #[serde(default)]
    latest: Option<String>,
    #[serde(default)]
    skipped: Option<String>,
}

fn read(path: &Path) -> Cache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn write(path: &Path, cache: &Cache) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(cache).map_err(|e| ZrushError::msg(e.to_string()))?;
    std::fs::write(path, text)?;
    Ok(())
}

/// The release to offer, if any. `fetch` is only called when the cache is
/// a day old or empty; a failure, or a tag that is not a version, offers
/// nothing and leaves the cache as it was.
pub fn check(
    dirs: &Dirs,
    current: &str,
    now: u64,
    fetch: impl FnOnce() -> Result<String>,
) -> Option<String> {
    let path = dirs.update_file();
    let mut cache = read(&path);
    let fresh = now.saturating_sub(cache.checked) < DAY;
    let latest = match cache.latest.clone() {
        Some(tag) if fresh => tag,
        _ => {
            let tag = fetch().ok()?;
            Version::parse(&tag)?;
            cache.checked = now;
            cache.latest = Some(tag.clone());
            let _ = write(&path, &cache);
            tag
        }
    };
    newer(&latest, current, cache.skipped.as_deref())
}

/// `latest`, when it is above `current` and not the one being skipped.
pub fn newer(latest: &str, current: &str, skipped: Option<&str>) -> Option<String> {
    if skipped == Some(latest) {
        return None;
    }
    let l = Version::parse(latest)?;
    let c = Version::parse(current)?;
    (l > c).then(|| latest.to_string())
}

/// Never offer this release again; a newer one still shows.
pub fn skip(dirs: &Dirs, tag: &str) -> Result<()> {
    let path = dirs.update_file();
    let mut cache = read(&path);
    cache.skipped = Some(tag.to_string());
    write(&path, &cache)
}

/// Where the latest release is asked for. `ZRUSH_UPDATE_URL` points it
/// elsewhere; `file:` reads a file, which is how the tests answer.
pub fn latest_url() -> String {
    std::env::var("ZRUSH_UPDATE_URL").unwrap_or_else(|_| LATEST.to_string())
}

/// The latest release's tag, from GitHub's API or a `file:` fixture.
pub fn fetch_latest(url: &str) -> Result<String> {
    let body = if let Some(path) = url.strip_prefix("file:") {
        std::fs::read_to_string(path)?
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        agent
            .get(url)
            .header("User-Agent", &format!("zrush/{}", current()))
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| ZrushError::msg(e.to_string()))?
            .into_body()
            .read_to_string()
            .map_err(|e| ZrushError::msg(e.to_string()))?
    };
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ZrushError::msg(e.to_string()))?;
    json.get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ZrushError::msg("no tag_name in the answer"))
}

/// The interpreter an installer is handed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shell {
    /// Unix: the POSIX shell, as the README's `curl | sh`.
    Sh,
    /// Git Bash or MSYS2 on Windows: bootstrap.sh, through their bash.
    Bash,
    /// Anything else on Windows: bootstrap.ps1, through this program.
    PowerShell(String),
}

/// Which installer fits the shell zrush runs in now. `MSYSTEM` is set by
/// Git Bash and MSYS2 for everything they start, which no parent-process
/// probing matches: editor panels, `wt` and `cmd /c` all get in the way.
pub fn shell(windows: bool, msystem: Option<&str>, has_pwsh: bool) -> Shell {
    if !windows {
        return Shell::Sh;
    }
    if msystem.is_some_and(|m| !m.is_empty()) {
        return Shell::Bash;
    }
    Shell::PowerShell(if has_pwsh { "pwsh" } else { "powershell" }.to_string())
}

/// The same, read from this process's environment.
pub fn shell_here() -> Shell {
    shell(
        cfg!(windows),
        std::env::var("MSYSTEM").ok().as_deref(),
        on_path("pwsh"),
    )
}

/// The command line that installs `tag`. The bootstrap is taken from that
/// tag, so the script matches the release it installs; it checks the
/// archive's checksum itself.
pub fn command(shell: &Shell, tag: &str) -> Vec<String> {
    let raw = format!("https://raw.githubusercontent.com/{REPO}/{tag}");
    match shell {
        // Downloaded first, then run: piped, a failed download hands sh an
        // empty script, which exits 0, and the update would claim success.
        Shell::Sh | Shell::Bash => vec![
            if *shell == Shell::Sh { "sh" } else { "bash" }.to_string(),
            "-c".into(),
            format!(
                "f=$(mktemp) && curl -fsSL -o \"$f\" {raw}/bootstrap.sh && sh \"$f\" {tag}; rc=$?; rm -f \"$f\"; exit $rc"
            ),
        ],
        Shell::PowerShell(program) => vec![
            program.clone(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-Command".into(),
            format!("& ([scriptblock]::Create((irm {raw}/bootstrap.ps1))) {tag}"),
        ],
    }
}

/// Whether `name` (an executable, `.exe` implied on Windows) is on the
/// PATH.
pub fn on_path(name: &str) -> bool {
    which(name).is_some()
}

/// The first `name` on the PATH, in the PATH's own order. Windows looks in
/// System32 before the PATH, where `bash.exe` is WSL's, not Git's: from
/// Git Bash that would run the installer in the wrong world entirely.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let file: PathBuf = if cfg!(windows) {
        format!("{name}.exe").into()
    } else {
        name.into()
    };
    std::env::split_paths(&path)
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
}

/// Seconds since the epoch, for the cache.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_parse_with_or_without_a_v() {
        assert_eq!(v("v0.0.3"), v("0.0.3"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
    }

    #[test]
    fn garbage_is_not_a_version() {
        for s in ["", "v", "nightly", "1.2", "1.2.3.4", "1.x.3", "1.2.3-"] {
            assert_eq!(Version::parse(s), None, "{s}");
        }
    }

    #[test]
    fn a_cli_tag_is_a_version() {
        assert_eq!(v("cli-v0.0.5"), v("0.0.5"));
        assert_eq!(
            newer("cli-v0.0.5", "0.0.4", None).as_deref(),
            Some("cli-v0.0.5")
        );
    }

    #[test]
    fn a_gui_tag_is_never_offered_to_the_cli() {
        assert_eq!(newer("gui-v9.9.9", "0.0.4", None), None);
        for s in ["gui-v1.2.3", "desktop-v1.2.3", "cli-1.2.3", "xcli-v1.2.3"] {
            assert_eq!(Version::parse(s), None, "{s}");
        }
    }

    #[test]
    fn versions_order_by_number_not_by_text() {
        assert!(v("0.0.10") > v("0.0.9"));
        assert!(v("0.1.0") > v("0.0.99"));
        assert!(v("1.0.0") > v("0.99.99"));
    }

    #[test]
    fn a_pre_release_comes_before_its_release() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-rc.1") > v("0.9.9"));
        assert!(v("1.0.0-rc.2") > v("1.0.0-rc.1"));
    }

    #[test]
    fn only_a_newer_release_is_offered() {
        assert_eq!(newer("v0.0.3", "0.0.2", None).as_deref(), Some("v0.0.3"));
        assert_eq!(newer("v0.0.2", "0.0.2", None), None);
        assert_eq!(newer("v0.0.1", "0.0.2", None), None);
        assert_eq!(newer("garbage", "0.0.2", None), None);
    }

    #[test]
    fn a_skipped_release_is_not_offered_but_the_next_one_is() {
        assert_eq!(newer("v0.0.3", "0.0.2", Some("v0.0.3")), None);
        assert_eq!(
            newer("v0.0.4", "0.0.2", Some("v0.0.3")).as_deref(),
            Some("v0.0.4")
        );
    }

    fn dirs() -> (tempfile::TempDir, Dirs) {
        let t = tempfile::tempdir().unwrap();
        let d = crate::config::dirs_under(t.path());
        (t, d)
    }

    #[test]
    fn a_fresh_cache_answers_without_the_network() {
        let (_t, d) = dirs();
        let first = check(&d, "0.0.2", 1000, || Ok("v0.0.3".into()));
        assert_eq!(first.as_deref(), Some("v0.0.3"));
        let again = check(&d, "0.0.2", 1000 + DAY - 1, || {
            panic!("fetched although the cache was fresh")
        });
        assert_eq!(again.as_deref(), Some("v0.0.3"));
    }

    #[test]
    fn a_day_old_cache_asks_again() {
        let (_t, d) = dirs();
        check(&d, "0.0.2", 1000, || Ok("v0.0.3".into()));
        let later = check(&d, "0.0.2", 1000 + DAY, || Ok("v0.0.4".into()));
        assert_eq!(later.as_deref(), Some("v0.0.4"));
    }

    #[test]
    fn a_failed_fetch_offers_nothing_and_caches_nothing() {
        let (_t, d) = dirs();
        let r = check(&d, "0.0.2", 1000, || Err(ZrushError::msg("offline")));
        assert_eq!(r, None);
        assert!(!d.update_file().exists());
        // So the next start tries again rather than trusting the failure.
        let r = check(&d, "0.0.2", 1001, || Ok("v0.0.3".into()));
        assert_eq!(r.as_deref(), Some("v0.0.3"));
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_not_cached() {
        let (_t, d) = dirs();
        assert_eq!(check(&d, "0.0.2", 1000, || Ok("nightly".into())), None);
        assert!(!d.update_file().exists());
    }

    #[test]
    fn skip_holds_across_checks() {
        let (_t, d) = dirs();
        check(&d, "0.0.2", 1000, || Ok("v0.0.3".into()));
        skip(&d, "v0.0.3").unwrap();
        assert_eq!(check(&d, "0.0.2", 1001, || Ok("v0.0.3".into())), None);
        let next = check(&d, "0.0.2", 1000 + DAY, || Ok("v0.0.4".into()));
        assert_eq!(next.as_deref(), Some("v0.0.4"));
    }

    #[test]
    fn a_file_url_reads_the_fixture() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("latest.json");
        std::fs::write(&p, r#"{"tag_name": "v9.9.9", "name": "x"}"#).unwrap();
        let tag = fetch_latest(&format!("file:{}", p.display())).unwrap();
        assert_eq!(tag, "v9.9.9");
    }

    #[test]
    fn the_shell_is_the_one_running_now() {
        assert_eq!(shell(false, None, false), Shell::Sh);
        // MSYSTEM means nothing off Windows.
        assert_eq!(shell(false, Some("MINGW64"), true), Shell::Sh);
        assert_eq!(shell(true, Some("MINGW64"), true), Shell::Bash);
        assert_eq!(shell(true, Some("MSYS"), false), Shell::Bash);
        assert_eq!(shell(true, None, true), Shell::PowerShell("pwsh".into()));
        assert_eq!(
            shell(true, None, false),
            Shell::PowerShell("powershell".into())
        );
        assert_eq!(
            shell(true, Some(""), false),
            Shell::PowerShell("powershell".into())
        );
    }

    #[test]
    fn the_bootstrap_comes_from_the_tag_it_installs() {
        let sh = command(&Shell::Sh, "v0.0.3");
        assert_eq!(sh[0], "sh");
        assert!(
            sh[2].contains(
                "curl -fsSL -o \"$f\" https://raw.githubusercontent.com/christphe/zrush/v0.0.3/bootstrap.sh"
            ),
            "{}",
            sh[2]
        );
        assert!(sh[2].contains("sh \"$f\" v0.0.3"), "{}", sh[2]);
        assert_eq!(command(&Shell::Bash, "v0.0.3")[0], "bash");

        let ps = command(&Shell::PowerShell("pwsh".into()), "v0.0.3");
        assert_eq!(ps[0], "pwsh");
        assert!(ps.contains(&"Bypass".to_string()));
        assert_eq!(
            ps.last().unwrap(),
            "& ([scriptblock]::Create((irm https://raw.githubusercontent.com/christphe/zrush/v0.0.3/bootstrap.ps1))) v0.0.3"
        );
    }
}
