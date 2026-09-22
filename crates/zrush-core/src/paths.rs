//! Path handling that differs between platforms.

use std::path::{Path, PathBuf};

/// The real path, in a form other programs accept.
///
/// `canonicalize` is needed because on macOS `$TMPDIR` is a symlink into
/// `/private`, and git reports the resolved path: comparing one against the
/// other matches nothing. On Windows the same call returns a verbatim UNC
/// path (`\\?\C:\…`), which git refuses outright — `fatal: cannot mkdir
/// \\?\C:\…: Invalid argument`. So the prefix comes back off.
///
/// Falls back to the path as given when it cannot be resolved, which is
/// what a caller wants for a path that does not exist yet.
pub fn real(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    strip_verbatim(&resolved)
}

#[cfg(windows)]
fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    // \\?\C:\… is a drive path; \\?\UNC\server\share is a network one and
    // has no plain form, so it is left alone.
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(not(windows))]
fn strip_verbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_does_not_exist_comes_back_as_given() {
        let p = Path::new("/definitely/not/here");
        assert_eq!(real(p), p);
    }

    #[test]
    fn an_existing_directory_resolves() {
        let td = tempfile::TempDir::new().unwrap();
        let r = real(td.path());
        assert!(r.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn the_verbatim_prefix_is_removed_because_git_refuses_it() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\a\b")),
            PathBuf::from(r"C:\a\b")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_network_path_keeps_its_prefix_having_no_plain_form() {
        let unc = Path::new(r"\\?\UNC\server\share");
        assert_eq!(strip_verbatim(unc), unc);
    }

    #[cfg(not(windows))]
    #[test]
    fn nothing_is_stripped_off_windows() {
        let p = Path::new("/a/b");
        assert_eq!(strip_verbatim(p), p);
    }
}
