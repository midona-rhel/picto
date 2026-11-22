//! The part of the Subscriptions screen that reads from the library and
//! sends the commands of the run and hold controls. It reads the list of
//! subscriptions, reads it again every 1.5 s while a run or a gallery
//! download is in progress, reads the runs and problems of the open
//! subscription, and follows the session's navigation.
use super::*;

impl SubscriptionsScreen {
    /// Reads every subscription from the library again, and the sites'
    /// logins and the checks of those logins from the application. The
    /// screen shows the earlier list until the answer arrives. The answer
    /// is not used when a later read has been started since.
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

    /// Called after each read of the list. It opens the page of a
    /// subscription that the "Add subscription" dialog has just created,
    /// once the list contains it. When the subscription whose page is shown
    /// is no longer in the list, it goes back to the tiles. It then sets
    /// the name in the scope title (`name_place`) and follows the runs and
    /// the gallery downloads (`follow_runs`, `follow_galleries`).
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
            // The subscription whose page is shown is no longer in the
            // list, so the screen goes back to the tiles.
            self.session
                .update(cx, |session, cx| session.open_place(None, cx));
        }
        self.name_place(cx);
        self.follow_runs(cx);
        self.follow_galleries(cx);
    }

    /// Reads the list again and, when a subscription's page is shown, that
    /// subscription's runs and problems.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        if let Some(id) = self.place_seen {
            self.load_detail(id, cx);
        }
    }

    /// Starts a timer of 1.5 s (`session::SUBSCRIPTION_POLL`), after which
    /// the screen reads the subscriptions again (`reload`). The timer runs
    /// only while the screen is shown and either a run is in progress or
    /// `grace_until` has not passed. Otherwise a timer that is running is
    /// dropped.
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

    /// Reads the progress of every gallery download that is running
    /// (`subscriptions.progress.get`) and stores it in `gallery_progress`.
    /// Once the site has reported the number of images in the gallery for a
    /// run, that number is kept when a later answer has none, and the
    /// larger of two reported numbers is kept. The entry of a download that
    /// is no longer running is removed. The session, not this screen, shows
    /// the notification for a download that has ended and removes the
    /// download from the library (`session::settle`).
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

    /// Sends the command that `command` returns for each of `ids`, and
    /// reads the subscriptions again when every answer has arrived. A
    /// subscription for which a command is already waiting for its answer
    /// is left out. Each subscription's id is in `acting` until the answers
    /// have arrived, which disables its run button and its hold button. A
    /// command that fails is shown as the error notification "Subscription
    /// action failed".
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

    /// Sends the command of the run button for the subscription `id`
    /// (`Command::primary`): `Pause`, `Resume` or `Run`, depending on the
    /// state of the subscription's run.
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

    /// Sends the command of the hold button for the subscription `id`
    /// (`Command::hold`): `Stop` while the subscription has an active run,
    /// and otherwise `Hold`, which puts it on hold or releases the hold.
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

    /// Reads the runs and the open problems of the subscription `id` into
    /// `detail`, and then tells the application that the subscription's
    /// open problems have been seen (`subscriptions.issues.acknowledge`).
    /// The answer is not used when a later read has been started since.
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
            // Opening a subscription's page acknowledges its open problems.
            // When the application has accepted that, the subscription's
            // count of open problems in `list` is set to 0. The Problems tab
            // still lists the problems until they are resolved.
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

    /// Reads the next page of open problems and adds it to the Problems
    /// tab. The tab's "Load more" button calls it.
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
                    // A read that fails is shown with the same notification
                    // as a command that fails.
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

    /// Called when the session reports a navigation. It reads the list
    /// each time the Subscriptions screen becomes the destination shown.
    /// When the screen is opened or its place changes, it closes the
    /// context menu, clears the selection, and reads the runs and problems
    /// of the subscription whose page is now shown. While the screen is
    /// shown, it copies the session's place into `place_seen`.
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
                // A context menu that was opened on the tiles or on another
                // subscription's page is closed.
                self.close_menu(cx);
                // No tile is selected when the screen is opened, and none
                // after a subscription's page has been opened or closed.
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
