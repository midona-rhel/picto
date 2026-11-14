//! The application's events and what follows them: the sidebar's
//! destinations and subscription activity read again, and the recents kept.
use super::edits::when_answered;
use super::{Change, SUBSCRIPTION_POLL, Session, SubscriptionActivity};
use crate::application::{self, Event};
use crate::backend;
use crate::i18n::t;
use gpui::*;

impl Session {
    // ── The application's events ────────────────────────────────

    /// Grayscale on or off for the grid's thumbnails (`gridGrayscaleAtom`,
    /// kept for the session only).
    pub fn toggle_grayscale(&mut self, cx: &mut Context<Self>) {
        self.grayscale = !self.grayscale;
        cx.emit(Change::Results);
    }

    /// Tells the views that the thumbnail of `hash` was written again by
    /// this app (regenerated, or a captured frame), as the application's
    /// `picto:thumbnail-changed` does for what it writes.
    pub fn thumbnail_written(&mut self, hash: String, cx: &mut Context<Self>) {
        cx.emit(Event::Thumbnail(hash));
    }

    /// Takes what the application reported since the last look, merged into
    /// one invalidation, does what the Electron runtime stores do with it,
    /// and passes it on to the views that subscribe to [`Event`].
    pub(super) fn receive_events(&mut self, cx: &mut Context<Self>) {
        let (changed, thumbnails) = application::coalesce(self.events.try_iter());
        for hash in thumbnails {
            cx.emit(Event::Thumbnail(hash));
        }
        let Some(changed) = changed else {
            return;
        };
        // `sidebarSettle.ts`: the counts and the destination list.
        if ["sidebar", "folders", "smart_folders", "tags", "duplicates"]
            .iter()
            .any(|resource| application::touches(&changed, resource))
        {
            self.refresh_navigation(cx);
        }
        if application::touches(&changed, "library") {
            // What other destinations held may no longer be true; going
            // back to one reads it again. The grid on screen is read again
            // in place (`gridSettle.ts`), keeping its scroll and selection.
            self.saved.clear();
            self.request_refresh(cx);
            let shown = self
                .selected_details()
                .map(|details| details.root.root_id.0);
            if shown.is_some_and(|id| changed.item_ids.contains(&id)) {
                self.refresh_details(cx);
            }
        }
        // `subscriptionsSettle.ts`: the running state follows runs.
        if application::touches(&changed, "subscriptions")
            || application::touches(&changed, "tasks")
        {
            self.refresh_activity(cx);
        }
        cx.emit(Event::Changed(changed));
    }

    /// Reads the sidebar's destinations and counts again. Changes that
    /// arrive while a read is under way are folded into one more read.
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

    /// Reads whether subscriptions are running or held, and keeps reading
    /// while one runs. A change that arrives while a read is under way is
    /// read once more after it, so a run that starts meanwhile is seen.
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
                if !again {
                    this.poll_activity(cx);
                }
            }
            if again {
                this.refresh_activity(cx);
            }
        });
    }

    /// Polls while a run is going, as the renderer does every 1.5 s.
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

    /// The sidebar row's button: holds every subscription, or lets them run
    /// again (`subscriptions.pause_all`).
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

    /// Fetches the current destination again, after an edit made elsewhere.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.saved.clear();
        self.query(cx);
    }

    /// `recordRecentFolderUse`: the folder moves to the front of the recent
    /// folders, each kept once, at most 30.
    pub fn record_folder_use(&mut self, folder: picto_library::FolderId) {
        record_recent(&mut self.recent_folders, folder);
        self.recents_changed = true;
    }

    /// `recordRecentItems('picto-recent-tags', …)`: the tags go to the front
    /// in their order, each kept once, at most 30.
    pub fn record_tags_use(&mut self, tags: &[String]) {
        record_recent_names(&mut self.recent_tags, tags);
        self.recents_changed = true;
    }

    /// Keeps the recents in the app's state once they changed.
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

/// `recordRecentFolderUse`: `folder` first, each folder once, at most 30.
fn record_recent(recent: &mut Vec<picto_library::FolderId>, folder: picto_library::FolderId) {
    recent.retain(|id| *id != folder);
    recent.insert(0, folder);
    recent.truncate(30);
}

/// `recordRecentItems`: `names` first, in their order, then the others,
/// each once, at most 30.
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
