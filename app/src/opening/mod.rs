//! This module handles the files that the system asks the app to open.
//! They arrive in three ways: on the command line, from a second launch on
//! the same copy ([`instance`]), and on macOS through
//! `application:openURLs:` (Open With, a drop on the Dock icon, `open -a`).
//!
//! The app acts on two kinds of files and ignores every other file. A
//! `.picto-pack` archive goes to the pack's import dialog, one archive at a
//! time. A `.library` folder that is not the open library is refused with a
//! notification ([`accept`]).
pub mod instance;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The kind of a file that the app acts on, which its name's ending gives.
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

/// Returns the file that `value` names, when `value` is the path or the
/// `file:` address of a `.picto-pack` or a `.library`. A relative path is
/// made absolute against `working_directory`. Returns `None` for an argument
/// that starts with `-`, for a file of another kind and for an address that
/// cannot be read.
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

/// Returns the files among `values` that the app acts on, in the order
/// given. A file that is named more than once is returned once.
pub fn from_arguments<S: AsRef<str>>(values: &[S], working_directory: &Path) -> Vec<Associated> {
    let mut seen = HashSet::new();
    values
        .iter()
        .filter_map(|value| classify(value.as_ref(), working_directory))
        .filter(|entry| seen.insert(entry.clone()))
        .collect()
}

/// Returns `path` without its `.` components, and with each `..` component
/// removed together with the component before it. Only the text of the path
/// is used; the file system is not read.
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
    /// `<copy> [files…]`: open the copy that the guard accepted, and then
    /// the files given after it.
    Open {
        copy: PathBuf,
        /// The arguments after the copy, as text. The instance that has the
        /// copy open opens them: this process, or a process that was started
        /// earlier.
        rest: Vec<String>,
    },
    /// The first argument is not a copy, so all arguments are files. A file
    /// manager starts the app this way, with only the file. `refused` says
    /// why the first argument is not a copy. It is printed when no running
    /// instance can take the files.
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
        Ok(copy) => Ok(Launch::Open { copy, rest }),
        Err(refused) => {
            let mut files = vec![text(first)?];
            files.extend(rest);
            Ok(Launch::HandOver { files, refused })
        }
    }
}

/// The files that the running app is asked to open at one time. `forward`
/// says whether the main window is brought to the front afterwards. It is
/// `true` for files from a second launch or from the system, and `false` for
/// the files of this launch's own command line, whose window is opening
/// anyway.
#[derive(Debug)]
pub struct Incoming {
    pub entries: Vec<Associated>,
    pub forward: bool,
}

/// Opens the files of `incoming`. Each pack is queued for its import
/// dialog. For a library that is the open one, nothing is done. Any other
/// library is refused with a notification, because a file that the system
/// hands over never opens a library (`libraries::admission`). Then the main
/// window is brought to the front, if `incoming.forward` asks for that.
pub fn accept(incoming: Incoming, cx: &mut gpui::App) {
    let root = crate::libraries::host::current(cx).map(|(root, _)| root);
    for entry in incoming.entries {
        match entry.kind {
            Kind::Pack => {
                if let Some(exports) = crate::export::get(cx) {
                    exports.update(cx, |exports, cx| exports.queue_pack(entry.path, cx));
                }
            }
            Kind::Library => {
                let same = entry
                    .path
                    .canonicalize()
                    .is_ok_and(|path| Some(&path) == root.as_ref());
                if !same {
                    // TODO: open the other library. The app does not
                    // switch to a library that the system hands over.
                    // The notification's title has no key in the
                    // translation files, so it is shown in English.
                    crate::ui::notify::error(
                        crate::i18n::en("Could not open Picto Library"),
                        format!(
                            "{}\n{}",
                            entry.path.display(),
                            crate::i18n::en(
                                "This is not the library open here. Start the app on a disposable copy of it instead."
                            )
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
                rest: vec!["a.picto-pack".into()],
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
