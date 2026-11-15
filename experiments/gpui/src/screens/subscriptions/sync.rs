//! The screen's reads and its following of the library: loading the
//! subscriptions, polling runs and gallery downloads while they go,
//! commands on chosen subscriptions, an opened subscription's detail and
//! issues, and the screen's place in the navigation.
use super::*;

impl SubscriptionsScreen {
    /// Reads every subscription again, and the sites' logins. What shows
    /// stays until the answer arrives.
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let blobs = self.session.read(cx).blobs().to_path_buf();
        let answer = self.session.read(cx).run(move |library| {
            data::subscriptions(library, &blobs).map_err(|error| error.to_string())
        });
        let logins = self.send(requests::credentials(), cx);
        let health = self.send(requests::credential_health(), cx);
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let logins = logins.await.ok().and_then(Result::ok);
            let health = health.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                if let Some(logins) = logins {
                    this.logins = read_logins(&logins);
                }
                if let Some(health) = health {
                    this.login_health = read_logins(&health);
                }
                match result {
                    Ok(list) => {
                        // A subscription that is gone is no longer selected.
                        this.selected
                            .retain(|id| list.iter().any(|entry| entry.id == *id));
                        this.list = Some(list);
                    }
                    Err(error) => {
                        crate::ui::notify::error(t("Subscriptions unavailable"), error, cx)
                    }
                }
                this.settled(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What follows a new read of the list: a subscription just created
    /// opens, a deleted one that was open gives way to the grid, and runs
    /// and gallery downloads are followed.
    pub(super) fn settled(&mut self, cx: &mut Context<Self>) {
        let exists = |id: i64| {
            self.list
                .iter()
                .flatten()
                .any(|entry| entry.id == id && !format::is_gallery_job(entry))
        };
        if let Some(id) = self.pending_open.filter(|id| exists(*id)) {
            self.pending_open = None;
            self.open(id, cx);
        } else if self.shown && self.list.is_some() && self.place_seen.is_some_and(|id| !exists(id))
        {
            // `refreshSubscriptionsWorkspace`: the workspace of a
            // subscription that is gone closes.
            self.session
                .update(cx, |session, cx| session.open_place(None, cx));
        }
        self.name_place(cx);
        self.follow_runs(cx);
        self.follow_galleries(cx);
    }

    /// Reads the list again, and the open subscription's runs and issues.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        if let Some(id) = self.place_seen {
            self.load_detail(id, cx);
        }
    }

    /// Reads progress again in 1.5 s while a run is going (or was just
    /// started) and the screen shows.
    pub(super) fn follow_runs(&mut self, cx: &mut Context<Self>) {
        let running = self
            .list
            .iter()
            .flatten()
            .any(|subscription| format::running(subscription));
        let starting = self.grace_until.is_some_and(|until| Instant::now() < until);
        if !self.shown || !(running || starting) {
            self.poll = None;
            return;
        }
        if self.poll.is_none() {
            self.poll = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::session::SUBSCRIPTION_POLL)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.poll = None;
                    this.reload(cx);
                });
            }));
        }
    }

    /// Gallery downloads: a running one's progress is read, with its total
    /// kept once known (`retainGalleryProgressTotals`). One whose run has
    /// settled is announced and removed by the session, for the whole app
    /// (`session::settle`).
    pub(super) fn follow_galleries(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(i64, bool)> = self
            .list
            .iter()
            .flatten()
            .filter(|subscription| format::is_gallery_job(subscription))
            .map(|job| (job.id, format::running(job)))
            .collect();
        self.gallery_progress
            .retain(|id, _| jobs.iter().any(|(job, running)| job == id && *running));
        for (id, running) in jobs {
            if running {
                let answer = self.send(requests::progress(id), cx);
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(progress)) = answer.await else {
                        return;
                    };
                    let Ok(Some(progress)) = serde_json::from_value::<
                        Option<picto_core::subscription_activity::CurrentSubscriptionProgress>,
                    >(progress) else {
                        return;
                    };
                    let _ = this.update(cx, |this, cx| {
                        let known = this
                            .gallery_progress
                            .get(&id)
                            .filter(|known| known.run == progress.run_id)
                            .and_then(|known| known.total);
                        let total = match (known, progress.gallery_total_items) {
                            (Some(known), Some(reported)) => Some(known.max(reported)),
                            (known, reported) => known.or(reported),
                        };
                        this.gallery_progress.insert(
                            id,
                            GalleryProgress {
                                run: progress.run_id,
                                downloaded: progress.counts.downloaded,
                                total,
                            },
                        );
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    /// Sends `command` for each of `ids`, then reads what changed. The
    /// controls of a subscription wait while its command is on its way.
    pub(super) fn perform(
        &mut self,
        ids: Vec<i64>,
        command: impl Fn(i64) -> Command,
        cx: &mut Context<Self>,
    ) {
        let mut answers = Vec::new();
        let mut starts = false;
        for id in ids {
            if !self.acting.insert(id) {
                continue;
            }
            let command = command(id);
            starts |= command.starts();
            answers.push((id, self.send(command.request(id), cx)));
        }
        if answers.is_empty() {
            return;
        }
        if starts {
            self.grace_until = Some(Instant::now() + RUN_GRACE);
        }
        cx.spawn(async move |this, cx| {
            let mut settled = Vec::new();
            for (id, answer) in answers {
                settled.push((id, answer.await));
            }
            let _ = this.update(cx, |this, cx| {
                for (id, result) in settled {
                    this.acting.remove(&id);
                    if let Ok(Err(error)) = result {
                        crate::ui::notify::error(t("Subscription action failed"), error, cx);
                    }
                }
                this.session
                    .update(cx, |session, cx| session.refresh_activity(cx));
                this.reload(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The run control of one subscription.
    pub(super) fn run_control(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(command) = self.subscription(id).map(|subscription| {
            Command::primary(
                subscription.active_run_id.is_some(),
                format::running(subscription),
            )
        }) else {
            return;
        };
        self.perform(vec![id], |_| command, cx);
    }

    /// The hold or stop control of one subscription.
    pub(super) fn hold_control(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(command) = self.subscription(id).map(|subscription| {
            Command::hold(subscription.active_run_id.is_some(), subscription.paused)
        }) else {
            return;
        };
        self.perform(vec![id], |_| command, cx);
    }

    pub(super) fn subscription(&self, id: i64) -> Option<&data::Subscription> {
        self.list.as_ref()?.iter().find(|entry| entry.id == id)
    }

    pub(super) fn subscription_mut(&mut self, id: i64) -> Option<&mut data::Subscription> {
        self.list.as_mut()?.iter_mut().find(|entry| entry.id == id)
    }

    /// Reads the open subscription's runs and issues.
    pub(super) fn load_detail(&mut self, id: i64, cx: &mut Context<Self>) {
        self.detail_generation += 1;
        let generation = self.detail_generation;
        self.loading_more = false;
        let answer = self
            .session
            .read(cx)
            .run(move |library| data::detail(library, id).map_err(|error| error.to_string()));
        cx.spawn(async move |this, cx| {
            let detail = match answer.await {
                Ok(Ok(detail)) => detail,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.detail_generation == generation {
                            crate::ui::notify::error(
                                t("Subscription details unavailable"),
                                error,
                                cx,
                            );
                        }
                    });
                    return;
                }
                Err(_) => return,
            };
            // `refreshDetail`: looking at a subscription acknowledges its
            // open issues, so its badge stops warning; the Problems tab
            // still lists them until they are resolved.
            let Ok(acknowledged) = this.update(cx, |this, cx| {
                this.send(
                    Request {
                        name: "subscriptions.issues.acknowledge",
                        args: serde_json::json!({ "subscription_id": id }),
                    },
                    cx,
                )
            }) else {
                return;
            };
            let acknowledged = matches!(acknowledged.await, Ok(Ok(_)));
            let _ = this.update(cx, |this, cx| {
                if this.detail_generation == generation {
                    this.detail = Some(detail);
                    if acknowledged && let Some(entry) = this.subscription_mut(id) {
                        entry.open_issues = 0;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Adds the next page of open issues to the Problems tab.
    pub(super) fn load_more_issues(&mut self, cx: &mut Context<Self>) {
        let Some((id, after)) = self
            .detail
            .as_ref()
            .and_then(|detail| Some((detail.subscription, detail.issues.next.clone()?)))
        else {
            return;
        };
        if self.loading_more {
            return;
        }
        self.loading_more = true;
        let generation = self.detail_generation;
        let answer = self.session.read(cx).run(move |library| {
            data::more_issues(library, id, after).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                if this.detail_generation != generation {
                    return;
                }
                match (result, this.detail.as_mut()) {
                    (Ok(Ok(page)), Some(detail)) => {
                        detail.issues.items.extend(page.items);
                        detail.issues.next = page.next;
                        detail.issues.total = page.total;
                    }
                    // `loadMoreHealth` runs through `act`.
                    (Ok(Err(error)), _) => {
                        ui::notify::error(t("Subscription action failed"), error, cx)
                    }
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Follows the session's navigation: reads the list each time the
    /// screen is opened, and the detail when a subscription is.
    pub(super) fn navigated(&mut self, cx: &mut Context<Self>) {
        let (showing, place) = {
            let session = self.session.read(cx);
            let showing = session
                .nav
                .get(session.scope)
                .is_some_and(|entry| entry.screen == Some(Screen::Subscriptions));
            (showing, session.place())
        };
        let entering = showing && !self.shown;
        if entering {
            self.load(cx);
        }
        if !showing {
            self.close_menu(cx);
        }
        self.shown = showing;
        if showing {
            if entering || place != self.place_seen {
                // A menu belongs to the surface it was opened on.
                self.close_menu(cx);
                // The grid opens with nothing selected, as it does each time
                // the Electron grid mounts.
                self.selected.clear();
                self.anchor = None;
                if let Some(id) = place {
                    if self.detail.as_ref().map(|detail| detail.subscription) != Some(id) {
                        self.tab = SOURCES;
                    }
                    self.load_detail(id, cx);
                }
            }
            self.place_seen = place;
            self.name_place(cx);
        }
        cx.notify();
    }
}
