//! Files the system asks the app to open, as production's main process takes
//! them (`associatedFileService.mjs`, `open-file`, `second-instance`): the
//! command line, a second launch on the same copy ([`instance`]), and on
//! macOS `application:openURLs:` (Open With, a drop on the Dock icon,
//! `open -a`). Production opens two kinds and ignores every other file:
//! `.picto-pack` archives, which go to the pack's import dialog one at a
//! time, and `.library` folders, which it switches to.
pub mod instance;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// What an opened file is (`ASSOCIATED_SUFFIXES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Pack,
    Library,
}

/// A file the system asked the app to open.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Associated {
    pub kind: Kind,
    pub path: PathBuf,
}

/// `classifyAssociatedFile`: a `.picto-pack` or `.library` path or `file:`
/// address, made absolute against `working_directory`; flags (`-…`), other
/// files and anything unreadable are `None`.
pub fn classify(value: &str, working_directory: &Path) -> Option<Associated> {
    if value.is_empty() || value.starts_with('-') {
        return None;
    }
    let candidate = if value.starts_with("file://") {
        url::Url::parse(value).ok()?.to_file_path().ok()?
    } else {
        PathBuf::from(value)
    };
    let lower = candidate.to_string_lossy().to_lowercase();
    let kind = if lower.ends_with(".picto-pack") {
        Kind::Pack
    } else if lower.ends_with(".library") {
        Kind::Library
    } else {
        return None;
    };
    let path = if candidate.is_absolute() {
        normalize(&candidate)
    } else {
        normalize(&working_directory.join(candidate))
    };
    Some(Associated { kind, path })
}

/// `associatedFilesFromArguments`: the opened files among `values`, each
/// once, in order. Arguments that are not text are skipped.
pub fn from_arguments<S: AsRef<str>>(values: &[S], working_directory: &Path) -> Vec<Associated> {
    let mut seen = HashSet::new();
    values
        .iter()
        .filter_map(|value| classify(value.as_ref(), working_directory))
        .filter(|entry| seen.insert(entry.clone()))
        .collect()
}

/// `path.normalize`: `.` dropped and `..` taken back, by the text alone.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(normal.components().next_back(), Some(Component::Normal(_))) {
                    normal.pop();
                } else if !normal.has_root() {
                    normal.push("..");
                }
            }
            other => normal.push(other.as_os_str()),
        }
    }
    normal
}

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Launch {
    /// `--make-copy <library>`
    MakeCopy(PathBuf),
    /// `<copy> [--detach | files…]`: the copy the guard accepted, opened (or
    /// detached and left), with the files given after it.
    Open {
        copy: PathBuf,
        detach: bool,
        /// The arguments after the copy, as text, for the instance that
        /// opens them (this one, or the one already open on the copy).
        rest: Vec<String>,
    },
    /// No copy: files for the instance already open, as a file manager
    /// launches the app with only the file. `refused` is why the first
    /// argument is not a copy, for when no instance is open.
    HandOver { files: Vec<String>, refused: String },
}

/// Reads the arguments after the program's name. `guard` is
/// `application::guard`, which only a disposable copy passes.
pub fn parse(
    arguments: Vec<OsString>,
    guard: impl Fn(&Path) -> Result<PathBuf, String>,
) -> Result<Launch, String> {
    const USAGE: &str = "pass the disposable library copy that run.sh or --make-copy made";
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Err(USAGE.into());
    };
    let text = |value: OsString| {
        value
            .into_string()
            .map_err(|value| format!("{} is not text; it is not opened", value.display()))
    };
    if first == "--make-copy" {
        return match arguments.next() {
            Some(source) => Ok(Launch::MakeCopy(PathBuf::from(source))),
            None => Err("--make-copy takes the library to copy".into()),
        };
    }
    let rest = arguments.map(text).collect::<Result<Vec<_>, _>>()?;
    match guard(Path::new(&first)) {
        Ok(copy) => Ok(Launch::Open {
            copy,
            detach: rest.first().is_some_and(|next| next == "--detach"),
            rest,
        }),
        Err(refused) => {
            let mut files = vec![text(first)?];
            files.extend(rest);
            Ok(Launch::HandOver { files, refused })
        }
    }
}

/// What reaches the running app to open: files, and whether the window
/// comes forward (a second launch, as `focusMainWindow`; the files of this
/// launch's own command line do not need it).
#[derive(Debug)]
pub struct Incoming {
    pub entries: Vec<Associated>,
    pub forward: bool,
}

/// Opens what arrived, as `drainAssociatedFiles` does: each pack is queued
/// for its import dialog, a library is this copy (nothing to do) or
/// refused, and the main window comes forward.
pub fn accept(incoming: Incoming, root: &Path, cx: &mut gpui::App) {
    for entry in incoming.entries {
        match entry.kind {
            Kind::Pack => {
                if let Some(exports) = crate::export::get(cx) {
                    exports.update(cx, |exports, cx| exports.queue_pack(entry.path, cx));
                }
            }
            Kind::Library => {
                let same = crate::application::guard(&entry.path).is_ok_and(|copy| copy == root);
                if !same {
                    // TODO: open another library (production's
                    // `switchLibrary`); the app opens only the copy it was
                    // started on.
                    crate::ui::notify::error(
                        "Could not open Picto Library".to_owned(),
                        format!(
                            "{} is not the library open here. Start the app on a disposable copy of it instead.",
                            entry.path.display()
                        ),
                        cx,
                    );
                }
            }
        }
    }
    if incoming.forward {
        crate::app_menu::bring_forward(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\work")
        } else {
            PathBuf::from("/work")
        }
    }

    #[test]
    fn packs_and_libraries_are_opened_and_other_arguments_ignored() {
        let cwd = root();
        let found = from_arguments(
            &[
                "--flag",
                "Portfolio.picto-pack",
                "./sub/../Portfolio.picto-pack",
                "photo.png",
                "Other.LIBRARY",
                "",
            ],
            &cwd,
        );
        assert_eq!(
            found,
            vec![
                Associated {
                    kind: Kind::Pack,
                    path: cwd.join("Portfolio.picto-pack"),
                },
                Associated {
                    kind: Kind::Library,
                    path: cwd.join("Other.LIBRARY"),
                },
            ]
        );
    }

    #[test]
    fn a_file_address_is_its_path() {
        let (address, path) = if cfg!(windows) {
            (
                "file:///C:/packs/a%20b.picto-pack",
                r"C:\packs\a b.picto-pack",
            )
        } else {
            ("file:///packs/a%20b.picto-pack", "/packs/a b.picto-pack")
        };
        assert_eq!(
            classify(address, &root()),
            Some(Associated {
                kind: Kind::Pack,
                path: PathBuf::from(path),
            })
        );
        assert_eq!(classify("file://not a url", &root()), None);
    }

    fn guard(path: &Path) -> Result<PathBuf, String> {
        if path.ends_with("Copy.library") {
            Ok(PathBuf::from("/canonical/Copy.library"))
        } else {
            Err(format!("{} is not a copy", path.display()))
        }
    }

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_first_argument_is_the_copy_and_the_rest_are_files() {
        assert_eq!(
            parse(arguments(&["x/Copy.library", "a.picto-pack"]), guard),
            Ok(Launch::Open {
                copy: PathBuf::from("/canonical/Copy.library"),
                detach: false,
                rest: vec!["a.picto-pack".into()],
            })
        );
        assert_eq!(
            parse(arguments(&["x/Copy.library", "--detach"]), guard),
            Ok(Launch::Open {
                copy: PathBuf::from("/canonical/Copy.library"),
                detach: true,
                rest: vec!["--detach".into()],
            })
        );
        assert_eq!(
            parse(arguments(&["--make-copy", "/l/Main.library"]), guard),
            Ok(Launch::MakeCopy(PathBuf::from("/l/Main.library")))
        );
        assert!(parse(arguments(&["--make-copy"]), guard).is_err());
        assert!(parse(Vec::new(), guard).is_err());
    }

    #[test]
    fn without_a_copy_the_files_go_to_the_open_instance() {
        assert_eq!(
            parse(arguments(&["a.picto-pack", "b.picto-pack"]), guard),
            Ok(Launch::HandOver {
                files: vec!["a.picto-pack".into(), "b.picto-pack".into()],
                refused: "a.picto-pack is not a copy".into(),
            })
        );
    }
}
