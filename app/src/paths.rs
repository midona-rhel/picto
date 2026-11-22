//! Resolves paths in the form the app works with.
//!
//! `Path::canonicalize` returns an absolute path without links. On Windows it
//! returns the path in its extended form, which starts with `\\?\`, for
//! example `\\?\E:\Main.library`. Other programs do not accept that form, a
//! user does not recognise it, and Picto 0.6.19 and earlier stored a
//! library's path in its ordinary form, `E:\Main.library`. The same folder
//! would therefore be known under two texts. So the app resolves every path
//! with [`Canonical::canonical`], which returns the ordinary form wherever
//! the path can be written in it, and never calls `canonicalize` itself.
use std::path::{Path, PathBuf};

/// Resolves a path as `Path::canonicalize` does and returns it in its
/// ordinary form.
pub trait Canonical {
    /// Returns the absolute path without links. On Windows the result has
    /// the ordinary form (`E:\Main.library`), and the extended form
    /// (`\\?\E:\Main.library`) only when the path cannot be written in the
    /// ordinary one, which is the case for a path longer than 260
    /// characters and for a reserved name. On other systems the result is
    /// that of `Path::canonicalize`.
    fn canonical(&self) -> std::io::Result<PathBuf>;
}

impl Canonical for Path {
    fn canonical(&self) -> std::io::Result<PathBuf> {
        dunce::canonicalize(self).map(|path| without_share_prefix(&path))
    }
}

/// Returns a path on a network share in its ordinary form:
/// `\\?\UNC\server\share\…` becomes `\\server\share\…`. `dunce` leaves the
/// extended form of such a path as it is. Every other path is returned
/// unchanged.
fn without_share_prefix(path: &Path) -> PathBuf {
    match path.to_string_lossy().strip_prefix(r"\\?\UNC\") {
        Some(share) => PathBuf::from(format!(r"\\{share}")),
        None => path.to_path_buf(),
    }
}

/// Returns the ordinary form of a path that is stored as text, without
/// reading the disk: on Windows `\\?\E:\Main.library` becomes
/// `E:\Main.library` and `\\?\UNC\server\share\Main.library` becomes
/// `\\server\share\Main.library`. A path that cannot be written in the
/// ordinary form is returned as it is. On other systems the text is returned
/// unchanged.
pub fn ordinary(path: &str) -> String {
    without_share_prefix(dunce::simplified(Path::new(path)))
        .to_string_lossy()
        .into_owned()
}

#[cfg(all(test, windows))]
mod tests {
    use super::{Canonical as _, ordinary};

    #[test]
    fn a_stored_path_is_read_in_its_ordinary_form() {
        assert_eq!(ordinary(r"\\?\E:\Main.library"), r"E:\Main.library");
        assert_eq!(ordinary(r"E:\Main.library"), r"E:\Main.library");
        assert_eq!(
            ordinary(r"\\?\UNC\server\share\Main.library"),
            r"\\server\share\Main.library"
        );
    }

    #[test]
    fn a_resolved_path_has_no_extended_prefix() {
        let folder = tempfile::tempdir().unwrap();
        let resolved = folder.path().canonical().unwrap();
        assert!(!resolved.to_string_lossy().starts_with(r"\\?\"), "{resolved:?}");
    }
}
