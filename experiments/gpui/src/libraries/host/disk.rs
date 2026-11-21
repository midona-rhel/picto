//! A library changed on disk from the Library Manager: whether it may be
//! now (`permit`), Rename, Move…, Delete…, and the consent the user gives.
use super::*;

/// Why the host did not change a library on disk ([`rename`],
/// [`move_library`], [`delete`]) or would not let the Library Manager
/// reveal it ([`permit`]).
pub enum Denied {
    /// Every rule held but the user has not agreed to this library yet:
    /// ask, keep the answer, and try again.
    Ask {
        root: PathBuf,
        identity: crate::libraries::Identity,
    },
    /// Refused or failed, in production's words.
    Error(String),
    /// A disposable copy that is not open, which the manager never lists.
    Copy,
}

/// What a second change is told while one runs (production's words).
const BUSY: &str = "A library operation is already in progress.";

/// Whether the library at `path` may be changed on disk or revealed from
/// the Library Manager right now. The open library was let through when it
/// opened; any other is what `admission::admit` decides for an open from
/// the manager, so a library the user has not agreed to yet is asked about
/// first, as opening does.
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

/// Rename (`renameLibrary`): the folder renamed beside itself, keeping its
/// place in the list, its colour and icon and the user's consent; the open
/// library is closed first and opened again after. Hands the outcome to
/// `done`.
pub fn rename(
    from: PathBuf,
    to: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<(), Denied>, &mut App) + 'static,
) {
    relocate(from, to, Relocation::Rename, cx, done);
}

/// Move… (`relocateLibrary`): the folder moved into another (`files`), as
/// a rename is.
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
    // Production renames only what it has closed; this app cannot see
    // what the Electron app has open.
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
                    Relocation::Rename => std::fs::rename(&source, &target)
                        .map_err(|error| format!("Failed to rename library: {error}")),
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
                        // The consent follows the path; it holds only while
                        // the database file is the same one (a move across
                        // volumes makes a new one, which is asked about).
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
                // Reported where it was asked for, in production's words
                // ("Failed to rename library: …", the move's own).
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

/// Delete… (`deleteLibrary`), once the user confirmed: checked again
/// ([`permit`]), as the moment may have changed (the Electron app started,
/// the consent was withdrawn), then the library's folder removed for good
/// (`files::delete`), never the open one, and taken out of the list. Hands
/// the outcome to `done`.
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
                    // A library put there later is asked about afresh.
                    state.library_consents.retain(|(path, _)| *path != text);
                });
                crate::set_app_menus(cx);
            }
            done(result.map_err(Denied::Error), cx);
        });
    })
    .detach();
}

/// The consent question's message for the library at `root`.
pub fn consent_message(root: &Path) -> String {
    format!(
        "{}\n\n{}",
        root.display(),
        en(
            "This app will read and write this library directly: what you change here changes the library itself, with no copy in between. Picto must not have it open at the same time. You are asked once for each library."
        )
    )
}

/// Keeps the user's consent for `identity`.
pub fn agree(identity: &crate::libraries::Identity, cx: &mut App) {
    crate::state::update(cx, |state| {
        let pair = (identity.path.clone(), identity.database.clone());
        if !state.library_consents.contains(&pair) {
            state.library_consents.push(pair);
        }
    });
}
