//! Which repositories the window opens with.

use std::path::PathBuf;

use zrush_core::config::Config;

/// The tabs saved last time; with none saved, the repository the window
/// was started in (or the configured default); otherwise none.
pub fn initial(cfg: &Config, cwd: impl FnOnce() -> std::io::Result<PathBuf>) -> Vec<PathBuf> {
    if !cfg.gui_repos.is_empty() {
        return cfg.gui_repos.clone();
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
        assert_eq!(initial(&saved(vec![a.clone()]), || Ok(b.clone())), vec![a]);
    }

    #[test]
    fn with_nothing_saved_the_current_repo_opens() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        assert_eq!(initial(&Config::default(), || Ok(repo.clone())), vec![repo]);
    }

    #[test]
    fn with_nothing_at_all_no_tab_opens() {
        let td = tempfile::TempDir::new().unwrap();
        let outside = td.path().to_path_buf();
        assert_eq!(
            initial(&Config::default(), || Ok(outside.clone())),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn a_current_directory_that_is_gone_opens_nothing() {
        let gone = || Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(initial(&Config::default(), gone), Vec::<PathBuf>::new());
    }
}
