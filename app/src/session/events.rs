//! Handles the events that the application layer reports. After an event the
//! session reads again what the event may have changed: the sidebar's
//! destinations, the grid's results, and the state of subscriptions. This
//! file also records the recently used folders and tags and writes them to
//! the app's state.
use super::edits::when_answered;
use super::{Change, SUBSCRIPTION_POLL, Session, SubscriptionActivity, settle};
use crate::application::{self, Event};
use crate::backend;
use crate::i18n::t;
use crate::screens::subscriptions::requests;
use gpui::*;

/// Shows each notification that the model in `settle.rs` returned.
fn show(popups: impl IntoIterator<Item = settle::Popup>, cx: &mut App) {
    for popup in popups {
        match popup {
            settle::Popup::Success { title, message } => {
                crate::ui::notify::success(title, message, cx)
            }
            settle::Popup::Error { title, message } => crate::ui::notify::error(title, message, cx),
        }
    }
}

impl Session {
    // The methods below handle the application layer's events, read the
    // state of subscriptions, and record the recently used folders and tags.

    /// Turns the grayscale drawing of the grid's thumbnails on or off. The
    /// setting is not saved: it lasts until the session ends.
    pub fn toggle_grayscale(&mut self, cx: &mut Context<Self>) {
        self.grayscale = !self.grayscale;
        cx.emit(Change::Results);
    }

    /// Tells the views that this app wrote the thumbnail of the file with
    /// content hash `hash` again, because it was regenerated or replaced by
    /// a captured video frame. The views receive the same `Event::Thumbnail`
    /// that the application layer sends for the thumbnails it writes itself.
    pub fn thumbnail_written(&mut self, hash: String, cx: &mut Context<Self>) {
        cx.emit(Event::Thumbnail(hash));
    }

    /// Takes the events that the application layer reported since the last
    /// call and merges the reported changes into one. Starts the reads that
    /// the change makes necessary, and then emits the events to the views
    /// that subscribe to [`Event`].
    pub(super) fn receive_events(&mut self, cx: &mut Context<Self>) {
        let (changed, thumbnails) = application::coalesce(self.events.try_iter());
        for hash in thumbnails {
            cx.emit(Event::Thumbnail(hash));
        }
        let Some(changed) = changed else {
            return;
        };
        // A change to any of these resources can change the sidebar's
        // destinations or their counts, so the sidebar's list is read again.
        if ["sidebar", "folders", "smart_folders", "tags", "duplicates"]
            .iter()
            .any(|resource| application::touches(&changed, resource))
        {
            self.refresh_navigation(cx);
        }
        if application::touches(&changed, "library") {
            // The grids saved for Back and Forward may now hold items that
            // are out of date, so they are dropped. Going back to such a
            // destination reads it again. The grid on screen is refreshed in
            // place, which keeps its scroll position and its selection.
            self.saved.clear();
            self.request_refresh(cx);
            let shown = self
                .selected_details()
                .map(|details| details.root.root_id.0);
            if shown.is_some_and(|id| changed.item_ids.contains(&id)) {
                self.refresh_details(cx);
            }
        }
        // A change to subscriptions or to tasks can mean that a run started
        // or ended, so the state of subscriptions is read again.
        if application::touches(&changed, "subscriptions")
            || application::touches(&changed, "tasks")
        {
            self.refresh_activity(cx);
        }
        cx.emit(Event::Changed(changed));
    }

    /// Reads the sidebar's destinations and counts again. When this is
    /// called while a read is running, no second read starts at the same
    /// time. One more read runs after the running one has finished.
    pub fn refresh_navigation(&mut self, cx: &mut Context<Self>) {
        if !self.navigation_reading.begin() {
            return;
        }
        let answer = self.run(backend::navigation);
        when_answered(answer, cx, |this: &mut Self, result, cx| {
            let again = this.navigation_reading.end();
            match result {
                Some(Ok((nav, tags))) => this.replace_navigation(nav, tags, cx),
                Some(Err(error)) => eprintln!("picto: {error}"),
                None => {}
            }
            if again {
                this.refresh_navigation(cx);
            }
        });
    }

    /// Reads `subscriptions.list` to learn whether any subscription is
    /// running and whether all subscriptions are paused, and gives the
    /// answer to the model in `settle.rs`. While a subscription is running,
    /// the read is repeated every 1.5 seconds (`SUBSCRIPTION_POLL`). When
    /// this is called while a read is running, one more read runs after the
    /// running one has finished, so a run that started meanwhile is seen.
    pub fn refresh_activity(&mut self, cx: &mut Context<Self>) {
        if !self.activity_reading.begin() {
            return;
        }
        let answer = self.dispatch("subscriptions.list", serde_json::json!({}));
        when_answered(answer, cx, |this: &mut Self, result, cx| {
            let again = this.activity_reading.end();
            if let Some(Ok(list)) = result {
                let activity = SubscriptionActivity {
                    busy: this.subscriptions.busy,
                    ..SubscriptionActivity::from_list(&list)
                };
                if activity != this.subscriptions {
                    this.subscriptions = activity;
                    cx.emit(Change::Activity);
                }
                this.settle_runs(&list, cx);
                if !again {
                    this.poll_activity(cx);
                }
            }
            if again {
                this.refresh_activity(cx);
            }
        });
    }

    /// Gives the answer of `subscriptions.list` to the model in `settle.rs`
    /// and shows the notifications the model returns. Then starts the reads
    /// the model asks for: the activity of runs (`subscriptions.runs.get`)
    /// and the latest run of each gallery download that has stopped running.
    /// The answers of those reads go to the model too, and the notifications
    /// it returns for them are shown.
    fn settle_runs(&mut self, list: &serde_json::Value, cx: &mut Context<Self>) {
        let (progress, galleries) = settle::read_list(list);
        let plan = self
            .settle
            .observe(progress, &galleries, std::time::Instant::now());
        show(plan.popups, cx);
        let activity = |this: &Self, run: i64| {
            this.dispatch(
                "subscriptions.runs.get",
                serde_json::json!({ "run_id": run, "source_item_limit": 1 }),
            )
        };
        for (entry, checked) in plan.query_reads {
            let Some(run) = entry.run else { continue };
            when_answered(
                activity(self, run),
                cx,
                move |this: &mut Self, result, cx| match result {
                    Some(Ok(activity)) => {
                        let popups = this.settle.query_activity(&entry, &activity);
                        show(popups, cx);
                    }
                    _ => this.settle.query_activity_failed(&entry, checked),
                },
            );
        }
        for entry in plan.finished_reads {
            let Some(run) = entry.run else { continue };
            // When this read fails, nothing is shown. The failed read does
            // not affect the run itself.
            when_answered(
                activity(self, run),
                cx,
                move |this: &mut Self, result, cx| {
                    if let Some(Ok(activity)) = result {
                        let popup = this.settle.run_finished(&entry, &activity);
                        show(popup, cx);
                    }
                },
            );
        }
        for gallery in plan.galleries {
            let runs = requests::runs(gallery.id);
            let runs = self.dispatch(runs.name, runs.args);
            when_answered(runs, cx, move |this: &mut Self, result, cx| {
                let Some(Ok(runs)) = result else {
                    this.settle.gallery_failed(&gallery);
                    return;
                };
                let Some(run) = this.settle.gallery_runs(&gallery, &runs) else {
                    return;
                };
                let cleanup = requests::clean_up_gallery(gallery.id);
                let cleanup = this.dispatch(cleanup.name, cleanup.args);
                when_answered(cleanup, cx, move |this: &mut Self, result, cx| {
                    // When the cleanup fails, the error is logged and no
                    // notification is shown. `gallery_failed` makes a later
                    // read of the list handle this gallery again.
                    let cleaned = match result {
                        Some(Ok(cleaned)) => cleaned,
                        Some(Err(error)) => {
                            eprintln!("picto: gallery import cleanup failed: {error}");
                            this.settle.gallery_failed(&gallery);
                            return;
                        }
                        None => {
                            this.settle.gallery_failed(&gallery);
                            return;
                        }
                    };
                    let popup = this.settle.gallery_cleaned(
                        &gallery,
                        &run,
                        Some(&cleaned).filter(|value| !value.is_null()),
                    );
                    show(popup, cx);
                });
            });
        }
    }

    /// Sets a timer that calls `refresh_activity` after 1.5 seconds
    /// (`SUBSCRIPTION_POLL`), if a subscription is running and no such timer
    /// is set yet. Removes the timer when no subscription is running.
    fn poll_activity(&mut self, cx: &mut Context<Self>) {
        if !self.subscriptions.running {
            self.activity_poll = None;
            return;
        }
        if self.activity_poll.is_some() {
            return;
        }
        self.activity_poll = Some(cx.spawn(async move |session, cx| {
            cx.background_executor().timer(SUBSCRIPTION_POLL).await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.activity_poll = None;
                this.refresh_activity(cx);
            });
        }));
    }

    /// Pauses all subscriptions, or resumes them when they are paused, with
    /// the `subscriptions.pause_all` command. The button of the sidebar's
    /// Subscriptions row calls this. Does nothing while an earlier call has
    /// not been answered. Shows an error notification if the command fails.
    pub fn toggle_all_subscriptions(&mut self, cx: &mut Context<Self>) {
        if self.subscriptions.busy {
            return;
        }
        let paused = !self.subscriptions.paused;
        self.subscriptions.busy = true;
        cx.emit(Change::Activity);
        let answer = self.dispatch(
            "subscriptions.pause_all",
            serde_json::json!({ "paused": paused }),
        );
        when_answered(answer, cx, move |this: &mut Self, result, cx| {
            this.subscriptions.busy = false;
            if let Some(Err(error)) = result {
                let title = if paused {
                    t("Could not pause subscriptions")
                } else {
                    t("Could not resume subscriptions")
                };
                crate::ui::notify::error(title, error, cx);
            }
            cx.emit(Change::Activity);
            this.refresh_activity(cx);
        });
    }

    /// Reads the open destination's results again from the beginning and
    /// discards the grids saved for Back and Forward. The sidebar calls this
    /// after an edit that changes which items the destination holds, and
    /// the grid's Retry button calls it after a read has failed.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.saved.clear();
        self.query(cx);
    }

    /// Records that items were added to `folder`: moves the folder to the
    /// front of the recent folders. The list holds each folder once and at
    /// most 30 folders.
    pub fn record_folder_use(&mut self, folder: picto_library::FolderId) {
        record_recent(&mut self.recent_folders, folder);
        self.recents_changed = true;
    }

    /// Records that `tags` were added to items: puts them at the front of
    /// the recent tags, in the order given. The list holds each tag once and
    /// at most 30 tags.
    pub fn record_tags_use(&mut self, tags: &[String]) {
        record_recent_names(&mut self.recent_tags, tags);
        self.recents_changed = true;
    }

    /// Writes the recent folders and the recent tags to the app's state, if
    /// either list changed since it was last written.
    pub(super) fn keep_recents(&mut self, cx: &mut App) {
        if !std::mem::take(&mut self.recents_changed) {
            return;
        }
        let library = self.library_key.clone();
        let folders = self.recent_folders.iter().map(|folder| folder.0).collect();
        let tags = self.recent_tags.clone();
        crate::state::update(cx, |state| {
            state.recent_folders.insert(library, folders);
            state.recent_tags = tags;
        });
    }
}

/// Moves `folder` to the front of `recent`, so that the list holds it once,
/// and cuts the list to 30 folders.
fn record_recent(recent: &mut Vec<picto_library::FolderId>, folder: picto_library::FolderId) {
    recent.retain(|id| *id != folder);
    recent.insert(0, folder);
    recent.truncate(30);
}

/// Puts `names` at the front of `recent` in the order given, followed by the
/// names that were already in the list. Each name is kept once, and the list
/// is cut to 30 names. Does nothing when `names` is empty.
fn record_recent_names(recent: &mut Vec<String>, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let mut next = Vec::with_capacity(30);
    for name in names.iter().chain(recent.iter()) {
        if !next.contains(name) {
            next.push(name.clone());
        }
    }
    next.truncate(30);
    *recent = next;
}

#[cfg(test)]
mod tests {
    use super::record_recent;
    use picto_library::FolderId;

    #[test]
    fn recent_tags_put_the_new_ones_first_in_their_order() {
        let mut recent = vec!["b".to_owned(), "c".to_owned()];
        super::record_recent_names(&mut recent, &["a".to_owned(), "c".to_owned()]);
        assert_eq!(recent, ["a", "c", "b"]);
        let many = (0..40).map(|n| n.to_string()).collect::<Vec<_>>();
        super::record_recent_names(&mut recent, &many);
        assert_eq!(recent.len(), 30);
        assert_eq!(recent[0], "0");
    }

    #[test]
    fn recent_folders_are_newest_first_each_once_and_at_most_thirty() {
        let mut recent = vec![FolderId(1), FolderId(2), FolderId(3)];
        record_recent(&mut recent, FolderId(3));
        assert_eq!(recent, [FolderId(3), FolderId(1), FolderId(2)]);
        record_recent(&mut recent, FolderId(9));
        assert_eq!(recent[0], FolderId(9));
        for id in 100..140 {
            record_recent(&mut recent, FolderId(id));
        }
        assert_eq!(recent.len(), 30);
        assert_eq!(recent[0], FolderId(139));
    }
}
