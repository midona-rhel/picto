//! A library replaced from the cloud: Restore in Settings › Cloud, and a
//! library joined from a cloud folder.
use super::*;

/// How long a restore waits for this app to let the library's connections
/// go before it gives up and replaces nothing.
const RELEASE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);

/// Restore in Settings › Cloud (`cloud.restore.start`), once the user
/// confirmed: the library's views close and this app lets go of every
/// connection to its database (otherwise the database could not be renamed
/// on Windows, and elsewhere a connection left open would go on reading
/// the file set aside); the core then sets the current database aside as
/// an emergency copy, puts the snapshot's in its place and opens it, and
/// this app opens the library again. A copy keeps the cloud folder it was
/// connected to. `done` hears how it went.
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
                // Whatever the core has open now (the restored library, or
                // the previous one put back) is closed, and opened again
                // below as any library is.
                restoring.close();
                drop(restoring);
                let _ = sent.send(result);
            })
            .ok();
        let result = crate::application::answer(answered).await;
        drop(application);
        cx.update(|cx| {
            // The restored database is a new file; the user's consent to
            // this library, given before they restored it here, is kept
            // for it once it opens.
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

/// Open from Cloud… (`joinCloudLibrary`), once the user chose where: the
/// library opened here (if any) closes, the core makes `target` from the
/// cloud library's newest verified snapshot (its media are then restored
/// in the background, blocking the window), and it opens as a library the
/// user made here. A failed join removes what it made and opens the
/// previous library again. `done` hears how it went.
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
                // An application of its own for the join, whose events go
                // nowhere: the library is opened again as any other below.
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
                    let root = target.canonicalize().unwrap_or(target);
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

/// What a failed join left at `target`, which did not exist before it:
/// removed only while it is a plain `.library` folder (not a link) with no
/// library in it.
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
