//! The changes that the Library Manager makes to a library on disk, as the
//! host carries them out: Rename ([`rename`]), Move… ([`move_library`]) and
//! Delete… ([`delete`]). [`permit`] decides whether a library may be changed
//! or revealed at this moment. [`consent_message`] is the text of the question
//! that asks the user's consent for a library, and [`agree`] stores the user's
//! yes.
use super::*;

/// Why the host did not change a library on disk ([`rename`],
/// [`move_library`], [`delete`]) or would not let the Library Manager
/// reveal it ([`permit`]).
pub enum Denied {
    /// The library passed every rule of `admission::admit`, but the user has
    /// not yet given consent for it. The Library Manager asks the user
    /// ([`consent_message`]), stores a yes with [`agree`], and then carries
    /// out the action. `root` is the library's canonical folder, and
    /// `identity` is what the consent is stored for.
    Ask {
        root: PathBuf,
        identity: crate::libraries::Identity,
    },
    /// The change was refused or it failed. The value is the message to show,
    /// in the Electron app's wording.
    Error(String),
    /// The path is a disposable copy of a library, and it is not the open
    /// library. The Library Manager never lists such a copy, so it shows
    /// nothing for this answer.
    Copy,
}

/// The error message for a rename, a move or a delete that is asked for while
/// the host is busy with another library operation. The wording is the
/// Electron app's.
const BUSY: &str = "A library operation is already in progress.";

/// Decides whether the library at `path` may be changed on disk, or revealed
/// in the file manager, from the Library Manager at this moment. The open
/// library is allowed, because it passed the rules when it was opened. For any
/// other library the answer is what `admission::admit` decides for a library
/// that the user opens from the Library Manager (`Origin::Manager`). So when
/// the user has not yet given consent for the library, the answer is
/// `Denied::Ask`, and the user is asked first, as when the library is opened.
pub fn permit(path: &Path, cx: &mut App) -> Result<(), Denied> {
    if current(cx).is_some_and(|(root, _)| root == path) {
        return Ok(());
    }
    let probes = host(cx).read(cx).probes.clone();
    let decision = admission::admit(
        path,
        Origin::Manager,
        &crate::libraries::facts(),
        &crate::libraries::consents(cx),
        &*probes,
    );
    match decision {
        Decision::Real { .. } => Ok(()),
        Decision::Ask { root, identity } => Err(Denied::Ask { root, identity }),
        Decision::Copy(_) => Err(Denied::Copy),
        Decision::Refused(refusal) => Err(Denied::Error(refusal.words().1)),
    }
}

/// Does the Library Manager's Rename (`renameLibrary` in the Electron app):
/// renames the library's folder `from` to `to`, which is in the same parent
/// folder. The library keeps its position in the list of libraries and among
/// the pinned libraries, its colour and icon, and the user's consent. If the
/// library is open, it is closed before the rename and opened again
/// afterwards, at the new path after a success and at the old path after a
/// failure. The result is passed to `done`.
pub fn rename(
    from: PathBuf,
    to: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<(), Denied>, &mut App) + 'static,
) {
    relocate(from, to, Relocation::Rename, cx, done);
}

/// Does the Library Manager's Move… (`relocateLibrary` in the Electron app):
/// moves the library's folder `from` to the path `to`, which is in another
/// folder (`files::move_to`). Everything else is done as [`rename`] does it:
/// the same checks are made, the open library is closed and opened again, the
/// library keeps its position in the list of libraries and among the pinned
/// libraries, its colour and its icon, and the result is passed to `done`.
/// After a move to another volume the user is asked for consent again, because
/// the database file is then a new file.
pub fn move_library(
    from: PathBuf,
    to: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<(), Denied>, &mut App) + 'static,
) {
    relocate(from, to, Relocation::Move, cx, done);
}

#[derive(Clone, Copy, PartialEq)]
enum Relocation {
    Rename,
    Move,
}

fn relocate(
    from: PathBuf,
    to: PathBuf,
    how: Relocation,
    cx: &mut App,
    done: impl FnOnce(Result<(), Denied>, &mut App) + 'static,
) {
    if host(cx).read(cx).busy {
        return done(Err(Denied::Error(BUSY.to_owned())), cx);
    }
    let probes = host(cx).read(cx).probes.clone();
    // The Electron app renames or moves a library only after it has closed it.
    // This app cannot tell which library the Electron app has open, so it
    // refuses the change whenever the Electron app is running, and also when
    // it cannot find out whether the Electron app is running. When the
    // Electron app is not running, `permits_library` still refuses a library
    // that this process may not open.
    let refused = match probes.picto_process() {
        Ok(None) => crate::libraries::facts().permits_library(&from).err(),
        Ok(Some(process)) => Some(Refusal::PictoRunning(process)),
        Err(error) => Some(Refusal::ProbeFailed(error)),
    };
    if let Some(refusal) = refused {
        return done(Err(Denied::Error(refusal.words().1)), cx);
    }
    if let Err(denied) = permit(&from, cx) {
        return done(Err(denied), cx);
    }
    let is_open = current(cx).is_some_and(|(root, _)| root == from);
    let main = host(cx).read(cx).main;
    let previous = match (is_open, main) {
        (true, Some(main)) => {
            close_views(main, Gate::Loading(crate::libraries::name(&to.to_string_lossy())), cx)
        }
        _ => None,
    };
    host(cx).update(cx, |host, _| host.busy = true);
    cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            close_application(previous, cx).await;
        }
        let (source, target) = (from.clone(), to.clone());
        let moved = cx
            .background_executor()
            .spawn(async move {
                match how {
                    Relocation::Rename => crate::libraries::files::rename_to(&source, &target),
                    Relocation::Move => crate::libraries::files::move_to(&source, &target),
                }
            })
            .await;
        cx.update(|cx| {
            host(cx).update(cx, |host, cx| {
                host.busy = false;
                cx.notify();
            });
            match moved {
                Ok(()) => {
                    let (from_text, to_text) = (
                        from.to_string_lossy().into_owned(),
                        to.to_string_lossy().into_owned(),
                    );
                    crate::state::update(cx, |state| {
                        history::rename(
                            &mut state.library_history,
                            &mut state.pinned_libraries,
                            &mut state.last_library,
                            &from_text,
                            &to_text,
                        );
                        history::rename_look(&mut state.library_meta, &from_text, &to_text);
                        // The stored consent is changed to the new path. A
                        // consent is the pair of the library's path and the
                        // identity of its database file, so it still applies
                        // only if the database file is the same file. A move
                        // across volumes copies the database into a new file,
                        // so after such a move the user is asked for consent
                        // again.
                        for consent in &mut state.library_consents {
                            if consent.0 == from_text {
                                consent.0 = to_text.clone();
                            }
                        }
                    });
                    crate::set_app_menus(cx);
                    host(cx).update(cx, |_, cx| cx.notify());
                    if is_open {
                        request(&to, Origin::Manager, cx);
                    }
                    done(Ok(()), cx);
                }
                // The error is passed to `done`, which shows it in the Library
                // Manager, where the user asked for the change. When the
                // rename or the move itself fails, the message has the
                // Electron app's wording: "Failed to rename library: …" or
                // "Failed to move library: …". If the library was open, it is
                // opened again at its old path.
                Err(error) => {
                    if is_open {
                        request(&from, Origin::Manager, cx);
                    }
                    done(Err(Denied::Error(error)), cx);
                }
            }
        });
    })
    .detach();
}

/// Does the Library Manager's Delete… (`deleteLibrary` in the Electron app),
/// after the user has confirmed it. It first checks the library again with
/// [`permit`], because things may have changed since the user chose Delete…:
/// the Electron app may have been started, or the consent may have been
/// withdrawn. It then removes the library's folder and everything in it
/// (`files::delete`). The folder is not moved to the system's Trash, and the
/// open library is never deleted. After a success the library is taken out of
/// the list of libraries and its stored consent is removed. The result is
/// passed to `done`.
pub fn delete(
    path: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<(), Denied>, &mut App) + 'static,
) {
    if host(cx).read(cx).busy {
        return done(Err(Denied::Error(BUSY.to_owned())), cx);
    }
    if let Err(denied) = permit(&path, cx) {
        return done(Err(denied), cx);
    }
    let open = current(cx).map(|(root, _)| root);
    host(cx).update(cx, |host, _| host.busy = true);
    let target = path.clone();
    let work = cx
        .background_executor()
        .spawn(async move { crate::libraries::files::delete(&target, open.as_deref()) });
    cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| {
            host(cx).update(cx, |host, cx| {
                host.busy = false;
                cx.notify();
            });
            if result.is_ok() {
                let text = path.to_string_lossy().into_owned();
                crate::state::update(cx, |state| {
                    history::remove(
                        &mut state.library_history,
                        &mut state.pinned_libraries,
                        &mut state.last_library,
                        &text,
                    );
                    // The stored consent for this path is removed, so that the
                    // user is asked again if a library is later put at the
                    // same path.
                    state.library_consents.retain(|(path, _)| *path != text);
                });
                crate::set_app_menus(cx);
            }
            done(result.map_err(Denied::Error), cx);
        });
    })
    .detach();
}

/// Returns the message of the dialog that asks the user's consent for the
/// library at `root`: the library's path, an empty line, and the explanation
/// of what the consent allows.
pub fn consent_message(root: &Path) -> String {
    format!(
        "{}\n\n{}",
        root.display(),
        en(
            "This app will read and write this library directly: what you change here changes the library itself, with no copy in between. Picto must not have it open at the same time. You are asked once for each library."
        )
    )
}

/// Stores the user's consent for `identity` in the app's state
/// (`library_consents`), unless it is already stored.
pub fn agree(identity: &crate::libraries::Identity, cx: &mut App) {
    crate::state::update(cx, |state| {
        let pair = (identity.path.clone(), identity.database.clone());
        if !state.library_consents.contains(&pair) {
            state.library_consents.push(pair);
        }
    });
}
