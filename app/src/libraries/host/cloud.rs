//! Replaces the open library's database with a snapshot from the cloud
//! (Restore in Settings › Cloud), and makes a library from a cloud folder
//! (Open from Cloud…).
use super::*;

/// How long a restore waits for this app to close every connection to the
/// library's database. After that time the restore stops and replaces
/// nothing.
const RELEASE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);

/// Does Restore in Settings › Cloud (`cloud.restore.start`), after the user
/// has confirmed it. It closes the library's views and waits until this app
/// has closed every connection to the library's database. On Windows the
/// database could not be renamed otherwise, and on the other systems an
/// open connection would go on reading the file that is set aside. The core
/// then sets the current database aside as an emergency copy, puts the
/// snapshot's database in its place and opens it. After that this app opens
/// the library again. A disposable copy keeps the cloud folder it was
/// connected to. The result is passed to `done`.
pub fn restore(
    snapshot: String,
    cx: &mut App,
    done: impl FnOnce(Result<(), String>, &mut App) + 'static,
) {
    let host = host(cx);
    let Some(main) = host.read(cx).main else {
        return;
    };
    if host.read(cx).busy {
        return;
    }
    let Some((root, kind)) = current(cx) else {
        return done(Err(en("No library is open.").to_owned()), cx);
    };
    let Some(Open {
        application,
        instance,
    }) = close_views(main, Gate::Loading(crate::libraries::name(&root.to_string_lossy())), cx)
    else {
        return;
    };
    host.update(cx, |host, _| host.busy = true);
    cx.spawn(async move |cx| {
        let (sent, answered) = futures::channel::oneshot::channel();
        let restoring = application.clone();
        std::thread::Builder::new()
            .name("picto-restore".into())
            .spawn(move || {
                let result = if restoring.wait_released(RELEASE_LIMIT) {
                    restoring.run_blocking(
                        "cloud.restore.start",
                        serde_json::json!({ "snapshot_id": snapshot }),
                    )
                } else {
                    Err(en("The library is still in use; nothing was restored.").to_owned())
                };
                // The core now has open either the restored library or,
                // after a failure, the previous one. It is closed here, and
                // `start_with` below opens it again like any other library.
                restoring.close();
                drop(restoring);
                let _ = sent.send(result);
            })
            .ok();
        let result = crate::application::answer(answered).await;
        drop(application);
        cx.update(|cx| {
            // The restored database is a new file, so the stored consent
            // no longer matches it. Setting `created` makes the host store
            // the consent for the new file when the library has opened,
            // without asking the user again.
            host.update(cx, |host, _| {
                host.busy = false;
                host.created = (kind == Kind::Real).then(|| root.clone());
            });
            start_with(root, kind, instance, main, true, cx);
            done(result.map(|_| ()), cx);
        });
    })
    .detach();
}

/// Does Open from Cloud… (`cloud.library.join`), after the user has chosen
/// where the library goes. It closes the open library, if there is one. The
/// core makes the library at `target` from the newest verified snapshot of
/// the cloud library. The library is then opened as one that the user made
/// in this app, so the user is not asked for consent. Its media files are
/// restored in the background after that, and a progress dialog blocks the
/// window meanwhile (`cloud::bar`). If the join fails, what it made at
/// `target` is removed and the library that was open before is opened again.
/// The result is passed to `done`.
pub fn join(
    choice: crate::cloud::Choice,
    target: PathBuf,
    cx: &mut App,
    done: impl FnOnce(Result<PathBuf, String>, &mut App) + 'static,
) {
    let host = host(cx);
    let Some(main) = host.read(cx).main else {
        return;
    };
    if host.read(cx).busy {
        return;
    }
    let probes = host.read(cx).probes.clone();
    if let Err(refusal) = admission::admit_new(&target, &crate::libraries::facts(), &*probes) {
        let (_, message) = refusal.words();
        refuse(&refusal, cx);
        return done(Err(message), cx);
    }
    let previous = current(cx);
    let open = close_views(
        main,
        Gate::Loading(crate::libraries::name(&target.to_string_lossy())),
        cx,
    );
    host.update(cx, |host, _| host.busy = true);
    cx.spawn(async move |cx| {
        if let Some(open) = open {
            close_application(open, cx).await;
        }
        let (sent, answered) = futures::channel::oneshot::channel();
        let joined = target.clone();
        std::thread::Builder::new()
            .name("picto-join".into())
            .spawn(move || {
                // The join runs in an application of its own, and nothing
                // receives that application's events. After the join,
                // `begin` below opens the library like any other library.
                let (events, _) = std::sync::mpsc::channel();
                let application = Application::new(joined.clone(), Kind::Real, events);
                let result = application.run_unopened(
                    "cloud.library.join",
                    serde_json::json!({
                        "provider": choice.root.provider,
                        "account_label": choice.root.account_label,
                        "root_path": choice.root.path,
                        "library_id": choice.library.library_id,
                        "target_root": joined.to_string_lossy(),
                    }),
                );
                drop(application);
                if result.is_err() {
                    remove_remnants(&joined);
                }
                let _ = sent.send(result.map(|_| choice.library.library_id));
            })
            .ok();
        let result = crate::application::answer(answered).await;
        cx.update(|cx| {
            host.update(cx, |host, cx| {
                host.busy = false;
                cx.notify();
            });
            match result {
                Ok(library_id) => {
                    let root = target.canonical().unwrap_or(target);
                    let path = root.to_string_lossy().into_owned();
                    crate::state::update(cx, |state| {
                        history::add(&mut state.library_history, &mut state.last_library, &path);
                        state
                            .library_meta
                            .entry(path.clone())
                            .or_default()
                            .cloud_library_id = Some(library_id);
                    });
                    host.update(cx, |host, _| host.created = Some(root.clone()));
                    begin(root.clone(), Kind::Real, cx);
                    done(Ok(root), cx);
                }
                Err(error) => {
                    if let Some((root, kind)) = previous {
                        begin(root, kind, cx);
                    }
                    done(Err(error), cx);
                }
            }
        });
    })
    .detach();
}

/// Removes what a failed join left at `target`, which did not exist before
/// the join. The folder is removed only if it is a real folder (not a link)
/// whose name ends in `.library` and it holds no `library.sqlite`.
fn remove_remnants(target: &Path) {
    let plain = target
        .symlink_metadata()
        .is_ok_and(|data| data.file_type().is_dir());
    let named = target
        .extension()
        .is_some_and(|extension| extension == "library");
    if plain && named && !target.join("library.sqlite").exists() {
        let _ = std::fs::remove_dir_all(target);
    }
}
