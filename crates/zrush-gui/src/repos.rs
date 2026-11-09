//! Which repositories the window opens with.

use std::path::PathBuf;

use zrush_core::config::Config;

/// The tabs saved last time, then the repositories named on the command
/// line. With neither, the repository the window was started in (or the
/// configured default); otherwise none.
pub fn initial(
    cfg: &Config,
    named: Vec<PathBuf>,
    cwd: impl FnOnce() -> std::io::Result<PathBuf>,
) -> Vec<PathBuf> {
    let mut repos = cfg.gui_repos.clone();
    repos.extend(named);
    if !repos.is_empty() {
        return repos;
    }
    zrush_core::app::locate(None, cwd, cfg)
        .map(|r| vec![r])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch_repo;

    fn saved(repos: Vec<PathBuf>) -> Config {
        Config {
            gui_repos: repos,
            ..Config::default()
        }
    }

    #[test]
    fn saved_repos_win_over_the_current_directory() {
        let td = tempfile::TempDir::new().unwrap();
        let a = td.path().join("a");
        let b = scratch_repo(&td);
        assert_eq!(
            initial(&saved(vec![a.clone()]), Vec::new(), || Ok(b.clone())),
            vec![a]
        );
    }

    #[test]
    fn named_repos_open_beside_the_saved_ones() {
        let td = tempfile::TempDir::new().unwrap();
        let a = td.path().join("a");
        let b = td.path().join("b");
        let cwd = scratch_repo(&td);
        assert_eq!(
            initial(&saved(vec![a.clone()]), vec![b.clone()], || Ok(cwd.clone())),
            vec![a, b]
        );
    }

    #[test]
    fn named_repos_replace_the_current_directory() {
        let td = tempfile::TempDir::new().unwrap();
        let b = td.path().join("b");
        let cwd = scratch_repo(&td);
        assert_eq!(
            initial(&Config::default(), vec![b.clone()], || Ok(cwd.clone())),
            vec![b]
        );
    }

    #[test]
    fn with_nothing_saved_the_current_repo_opens() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        assert_eq!(
            initial(&Config::default(), Vec::new(), || Ok(repo.clone())),
            vec![repo]
        );
    }

    #[test]
    fn with_nothing_at_all_no_tab_opens() {
        let td = tempfile::TempDir::new().unwrap();
        let outside = td.path().to_path_buf();
        assert_eq!(
            initial(&Config::default(), Vec::new(), || Ok(outside.clone())),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn a_current_directory_that_is_gone_opens_nothing() {
        let gone = || Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(
            initial(&Config::default(), Vec::new(), gone),
            Vec::<PathBuf>::new()
        );
    }
}
