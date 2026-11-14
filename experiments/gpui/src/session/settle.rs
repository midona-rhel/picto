//! What production's subscription settle store announces as runs go and end
//! (`subscriptionsSettle.ts`): a run paused by an error ("Subscription
//! paused", "Gallery download paused"), each query that finishes ("Query
//! completed"), a run that finishes ("Subscription completed"), and a
//! gallery download that has settled ("Gallery downloaded", "Gallery
//! already exists", "Gallery import failed"), whose transient subscription
//! is then removed. The session reads `subscriptions.list` whenever the
//! library says subscriptions or tasks changed, and every 1.5 s while one
//! runs, for the whole app (the Subscriptions screen need not show); this
//! model decides from those reads and from the run and gallery reads it
//! asks for what to announce. It holds no clock or command of its own, so
//! production's own test cases drive it here as recorded sequences.
use crate::i18n::t;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// `RUN_ACTIVITY_REFRESH_MS`: an unchanged run's activity is read again
/// after this long at most.
pub const RUN_ACTIVITY_REFRESH: Duration = Duration::from_secs(10);

/// One running subscription's state, as production's `runningProgress`
/// keeps it (`mapProgress` of `subscriptions.list`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub subscription: i64,
    pub name: String,
    pub run: Option<i64>,
    pub phase: String,
    pub failure_kind: Option<String>,
    pub error: Option<String>,
}

impl Progress {
    /// `runProgressFingerprint`: what makes a run's activity worth reading
    /// again.
    fn fingerprint(&self) -> String {
        format!(
            "\u{0}{}\u{0}\u{0}{}",
            self.phase,
            self.failure_kind.as_deref().unwrap_or_default()
        )
    }
}

/// A gallery download, a subscription of its own (`isGalleryImportJob`).
#[derive(Clone, Debug, PartialEq)]
pub struct Gallery {
    pub id: i64,
    pub name: String,
    pub running: bool,
}

/// What `subscriptions.list` says about runs: the progress of every
/// subscription with an active run, and the gallery downloads.
pub fn read_list(list: &Value) -> (Vec<Progress>, Vec<Gallery>) {
    let mut progress = Vec::new();
    let mut galleries = Vec::new();
    for subscription in list["subscriptions"].as_array().into_iter().flatten() {
        let id = subscription["subscription_id"].as_i64().unwrap_or_default();
        let name = subscription["name"].as_str().unwrap_or_default().to_owned();
        let status = subscription["status"].as_str();
        let run = subscription["active_run_id"].as_i64();
        let running = run.is_some() && matches!(status, Some("pending" | "running"));
        let queries = subscription["queries"].as_array();
        if queries.is_some_and(|queries| {
            queries.len() == 1 && queries[0]["site_id"].as_str() == Some("ehentai")
        }) {
            galleries.push(Gallery {
                id,
                name: name.clone(),
                running,
            });
        }
        let (Some(run), Some(status)) = (run, status) else {
            continue;
        };
        let error = queries
            .into_iter()
            .flatten()
            .find_map(|query| query["last_failure_message"].as_str())
            .map(str::to_owned);
        progress.push(Progress {
            subscription: id,
            name,
            run: Some(run),
            phase: status.to_owned(),
            failure_kind: (!matches!(status, "pending" | "running" | "paused" | "inbox_full"))
                .then(|| status.to_owned()),
            error,
        });
    }
    (progress, galleries)
}

/// A notification to show.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    Success { title: String, message: String },
    Error { title: String, message: String },
}

/// What to do after a read of the list.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub popups: Vec<Popup>,
    /// Runs whose activity to read for finished queries
    /// (`subscriptions.runs.get`), with when they were looked at.
    pub query_reads: Vec<(Progress, Instant)>,
    /// Runs that ended since the last read, to read for their outcome.
    pub finished_reads: Vec<Progress>,
    /// Gallery downloads no longer running, to read the latest run of
    /// (`subscriptions.runs.list`).
    pub galleries: Vec<Gallery>,
}

/// `isSuccessfulTerminal`.
fn successful(status: &str) -> bool {
    matches!(status, "completed" | "succeeded" | "success")
}

/// `completionSummary`.
fn posts_added(count: i64) -> String {
    if count == 1 {
        t("1 post added to library").to_string()
    } else {
        crate::tf!("{value0} posts added to library", count).to_string()
    }
}

#[derive(Default)]
pub struct Settle {
    /// The last read's progress (`previousSnapshot.runningProgress`).
    previous: Vec<Progress>,
    /// Each run's queries and their status when last read.
    queries: HashMap<i64, HashMap<i64, String>>,
    /// Each run's fingerprint and when its activity was last read.
    observed: HashMap<i64, (String, Instant)>,
    notified_runs: HashSet<i64>,
    notified_pauses: HashSet<String>,
    settling_galleries: HashSet<i64>,
}

impl Settle {
    /// A new read of the list at `now`.
    pub fn observe(&mut self, current: Vec<Progress>, galleries: &[Gallery], now: Instant) -> Plan {
        let gallery_ids: HashSet<i64> = galleries.iter().map(|gallery| gallery.id).collect();
        let mut plan = Plan::default();
        // `observePausedFailures`
        for entry in &current {
            let (Some(run), Some(kind)) = (entry.run, entry.failure_kind.as_deref()) else {
                continue;
            };
            if matches!(kind, "paused" | "inbox_full") {
                continue;
            }
            let key = format!(
                "{run}:{kind}:{}",
                entry.error.as_deref().unwrap_or_default()
            );
            if !self.notified_pauses.insert(key) {
                continue;
            }
            plan.popups.push(Popup::Error {
                title: if gallery_ids.contains(&entry.subscription) {
                    t("Gallery download paused")
                } else {
                    t("Subscription paused")
                }
                .to_string(),
                message: entry.error.clone().unwrap_or_else(|| {
                    "The run paused after an error. Retry it when ready.".to_owned()
                }),
            });
        }
        // `observeQueryCompletions`: a run whose state changed, or that has
        // not been looked at for a while.
        for entry in &current {
            let Some(run) = entry.run else { continue };
            if gallery_ids.contains(&entry.subscription) {
                continue;
            }
            let fingerprint = entry.fingerprint();
            if self.observed.get(&run).is_some_and(|(seen, at)| {
                *seen == fingerprint && now.duration_since(*at) < RUN_ACTIVITY_REFRESH
            }) {
                continue;
            }
            self.observed.insert(run, (fingerprint, now));
            plan.query_reads.push((entry.clone(), now));
        }
        // `observeSubscriptionCompletions`: runs of the last read that are
        // no longer active.
        let active: HashSet<i64> = current.iter().filter_map(|entry| entry.run).collect();
        for entry in &self.previous {
            let Some(run) = entry.run else { continue };
            if gallery_ids.contains(&entry.subscription)
                || active.contains(&run)
                || self.notified_runs.contains(&run)
            {
                continue;
            }
            plan.finished_reads.push(entry.clone());
        }
        // `settleFinishedGalleryImports`
        for gallery in galleries {
            if !gallery.running && self.settling_galleries.insert(gallery.id) {
                plan.galleries.push(gallery.clone());
            }
        }
        self.previous = current;
        plan
    }

    /// A run's activity (`subscriptions.runs.get`), read for `entry` at
    /// `checked`: each query that has just finished well is announced. The
    /// first read of a run only records where its queries are.
    pub fn query_activity(&mut self, entry: &Progress, activity: &Value) -> Vec<Popup> {
        let Some(run) = entry.run else {
            return Vec::new();
        };
        let current: HashMap<i64, String> = queries(activity)
            .map(|query| {
                (
                    query["query_id"].as_i64().unwrap_or_default(),
                    query["status"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let Some(previous) = self.queries.insert(run, current) else {
            return Vec::new();
        };
        queries(activity)
            .filter(|query| {
                let status = query["status"].as_str().unwrap_or_default();
                let id = query["query_id"].as_i64().unwrap_or_default();
                successful(status) && !previous.get(&id).is_some_and(|was| successful(was))
            })
            .map(|query| Popup::Success {
                title: t("Query completed").to_string(),
                message: format!(
                    "{} · {} · {}",
                    entry.name,
                    query["query_text"].as_str().unwrap_or_default(),
                    posts_added(query["counts"]["posts_added"].as_i64().unwrap_or_default())
                ),
            })
            .collect()
    }

    /// A run's activity could not be read: unchanged progress is read again
    /// on the next look.
    pub fn query_activity_failed(&mut self, entry: &Progress, checked: Instant) {
        if let Some(run) = entry.run
            && self
                .observed
                .get(&run)
                .is_some_and(|(_, at)| *at == checked)
        {
            self.observed.remove(&run);
        }
    }

    /// The activity of a run that ended: announced once if it finished
    /// well.
    pub fn run_finished(&mut self, entry: &Progress, activity: &Value) -> Option<Popup> {
        let run = entry.run?;
        self.queries.remove(&run);
        self.observed.remove(&run);
        let summary = &activity["summary"];
        if !successful(summary["status"].as_str().unwrap_or_default()) {
            return None;
        }
        self.notified_runs.insert(run);
        let completed = queries(activity)
            .filter(|query| successful(query["status"].as_str().unwrap_or_default()))
            .count();
        Some(Popup::Success {
            title: t("Subscription completed").to_string(),
            message: format!(
                "{} · {completed} quer{} completed · {}",
                entry.name,
                if completed == 1 { "y" } else { "ies" },
                posts_added(
                    summary["counts"]["posts_added"]
                        .as_i64()
                        .unwrap_or_default()
                )
            ),
        })
    }

    /// What a gallery download's latest run says, from
    /// `subscriptions.runs.list`: `None` while it has no run or one still
    /// pending or running (looked at again on a later read); otherwise
    /// whether to clean it up. The announcement waits for the cleanup's
    /// answer ([`Settle::gallery_cleaned`]).
    pub fn gallery_runs(&mut self, gallery: &Gallery, runs: &Value) -> Option<Value> {
        // i18n: a JSON field
        let latest = runs["runs"]
            .as_array()
            .and_then(|runs| runs.first())
            .cloned();
        let settled =
            latest.filter(|run| !matches!(run["status"].as_str(), Some("pending" | "running")));
        if settled.is_none() {
            self.settling_galleries.remove(&gallery.id);
        }
        settled
    }

    /// The gallery download's announcement once `subscriptions.gallery.cleanup`
    /// answered `cleanup` for its latest run `run`. It may be settled again
    /// on a later read.
    pub fn gallery_cleaned(
        &mut self,
        gallery: &Gallery,
        run: &Value,
        cleanup: Option<&Value>,
    ) -> Option<Popup> {
        self.settling_galleries.remove(&gallery.id);
        let title = cleanup
            .and_then(|cleanup| cleanup["title"].as_str())
            .unwrap_or(&gallery.name)
            .to_owned();
        let exists = cleanup
            .and_then(|cleanup| cleanup["already_exists"].as_bool())
            .unwrap_or(false);
        match run["status"].as_str().unwrap_or_default() {
            "succeeded" if exists => Some(Popup::Success {
                title: t("Gallery already exists").to_string(),
                message: format!("{title} is already in the library."),
            }),
            "succeeded" => Some(Popup::Success {
                title: t("Gallery downloaded").to_string(),
                message: format!("{title} has been downloaded."),
            }),
            "cancelled" => None,
            status => Some(Popup::Error {
                title: t("Gallery import failed").to_string(),
                message: run["error_message"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!(
                            "Gallery worker failed ({}).",
                            run["failure_kind"].as_str().unwrap_or(status)
                        )
                    }),
            }),
        }
    }

    /// A gallery's reads failed: it is looked at again on a later read.
    pub fn gallery_failed(&mut self, gallery: &Gallery) {
        self.settling_galleries.remove(&gallery.id);
    }
}

fn queries(activity: &Value) -> impl Iterator<Item = &Value> {
    activity["queries"].as_array().into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `runActivity` of production's test.
    fn run_activity(run_status: &str, query_status: &str) -> Value {
        let counts = json!({
            "posts_traversed": 100, "posts_added": 12, "fetched": 12, "downloaded": 12,
            "queued": 12, "ingested": 12, "failed": 0, "deleted": 0,
        });
        json!({
            "summary": { "run_id": 44, "subscription_id": 7, "requested_by": "manual",
                "status": run_status, "counts": counts, "query_count": 1 },
            "queries": [{ "run_query_id": 45, "run_id": 44, "query_id": 46, "site_id": "pixiv",
                "query_text": "artist-name", "status": query_status, "attempt_count": 1,
                "counts": counts, "source_items": [] }],
            "new_library_item_keys": [],
        })
    }

    fn feed(status: &str) -> Value {
        json!({ "subscription_id": 7, "name": "Active feed", "status": status,
            "active_run_id": 44, "queries": [{ "site_id": "pixiv", "last_failure_message": null }] })
    }

    fn gallery_job(status: Option<&str>, run: Option<i64>) -> Value {
        json!({ "subscription_id": 9, "name": "E-Hentai Gallery 12345", "status": status,
            "active_run_id": run, "queries": [{ "site_id": "ehentai" }] })
    }

    fn list(subscriptions: Vec<Value>) -> Vec<Progress> {
        read_list(&json!({ "subscriptions": subscriptions })).0
    }

    #[test]
    fn reads_running_progress_and_galleries_as_production_maps_them() {
        let (progress, galleries) = read_list(&json!({ "subscriptions": [
            feed("running"),
            { "subscription_id": 8, "name": "Idle", "status": null, "active_run_id": null, "queries": [] },
            { "subscription_id": 10, "name": "Broken", "status": "failed", "active_run_id": 50,
              "queries": [{ "site_id": "pixiv", "last_failure_message": "Rate limited" }] },
            gallery_job(Some("running"), Some(90)),
        ]}));
        assert_eq!(progress.len(), 3);
        assert_eq!(progress[0].failure_kind, None);
        assert_eq!(progress[1].failure_kind.as_deref(), Some("failed"));
        assert_eq!(progress[1].error.as_deref(), Some("Rate limited"));
        assert_eq!(
            galleries,
            [Gallery {
                id: 9,
                name: "E-Hentai Gallery 12345".into(),
                running: true
            }]
        );
    }

    #[test]
    fn notifies_when_a_persisted_query_crosses_into_a_successful_terminal_state() {
        let mut settle = Settle::default();
        let now = Instant::now();
        let plan = settle.observe(list(vec![feed("running")]), &[], now);
        assert_eq!(plan.query_reads.len(), 1);
        let entry = plan.query_reads[0].0.clone();
        // The first read only records where the queries are.
        assert!(
            settle
                .query_activity(&entry, &run_activity("running", "running"))
                .is_empty()
        );
        // The phase changed: read again, and the query has finished.
        let plan = settle.observe(list(vec![feed("settling")]), &[], now);
        assert_eq!(plan.query_reads.len(), 1);
        assert_eq!(
            settle.query_activity(&entry, &run_activity("running", "succeeded")),
            [Popup::Success {
                title: "Query completed".into(),
                message: "Active feed · artist-name · 12 posts added to library".into(),
            }]
        );
        // Announced once.
        assert!(
            settle
                .query_activity(&entry, &run_activity("running", "succeeded"))
                .is_empty()
        );
    }

    #[test]
    fn does_not_reread_run_activity_while_reported_run_state_is_unchanged() {
        let mut settle = Settle::default();
        let now = Instant::now();
        assert_eq!(
            settle
                .observe(list(vec![feed("running")]), &[], now)
                .query_reads
                .len(),
            1
        );
        let later = now + Duration::from_millis(1500);
        assert!(
            settle
                .observe(list(vec![feed("running")]), &[], later)
                .query_reads
                .is_empty()
        );
        // Unless the read failed, or ten seconds have passed.
        let mut failed = Settle::default();
        let plan = failed.observe(list(vec![feed("running")]), &[], now);
        failed.query_activity_failed(&plan.query_reads[0].0, plan.query_reads[0].1);
        assert_eq!(
            failed
                .observe(list(vec![feed("running")]), &[], later)
                .query_reads
                .len(),
            1
        );
        assert_eq!(
            settle
                .observe(list(vec![feed("running")]), &[], now + RUN_ACTIVITY_REFRESH)
                .query_reads
                .len(),
            1
        );
    }

    #[test]
    fn notifies_when_a_persisted_subscription_run_completes() {
        let mut settle = Settle::default();
        let now = Instant::now();
        settle.observe(list(vec![feed("running")]), &[], now);
        let plan = settle.observe(Vec::new(), &[], now);
        assert_eq!(plan.finished_reads.len(), 1);
        let entry = &plan.finished_reads[0];
        assert_eq!(
            settle.run_finished(entry, &run_activity("succeeded", "succeeded")),
            Some(Popup::Success {
                title: "Subscription completed".into(),
                message: "Active feed · 1 query completed · 12 posts added to library".into(),
            })
        );
        // Announced once; a failed or cancelled run is not announced.
        settle.observe(list(vec![feed("running")]), &[], now);
        assert!(
            settle
                .observe(Vec::new(), &[], now)
                .finished_reads
                .is_empty()
        );
        let mut failed = Settle::default();
        failed.observe(list(vec![feed("running")]), &[], now);
        let plan = failed.observe(Vec::new(), &[], now);
        assert_eq!(
            failed.run_finished(&plan.finished_reads[0], &run_activity("failed", "failed")),
            None
        );
    }

    #[test]
    fn announces_a_run_paused_by_an_error_once() {
        let mut settle = Settle::default();
        let now = Instant::now();
        let failed = json!({ "subscription_id": 7, "name": "Active feed", "status": "failed",
            "active_run_id": 44, "queries": [{ "site_id": "pixiv", "last_failure_message": "Login expired" }] });
        let plan = settle.observe(list(vec![failed.clone()]), &[], now);
        assert_eq!(
            plan.popups,
            [Popup::Error {
                title: "Subscription paused".into(),
                message: "Login expired".into()
            }]
        );
        assert!(
            settle
                .observe(list(vec![failed]), &[], now)
                .popups
                .is_empty()
        );
        // A pause the user asked for, or a full Inbox, is not an error.
        let mut quiet = Settle::default();
        assert!(
            quiet
                .observe(list(vec![feed("paused"), feed("inbox_full")]), &[], now)
                .popups
                .is_empty()
        );
        // A gallery download's own title, and production's words without a
        // message.
        let mut gallery = Settle::default();
        let (progress, galleries) =
            read_list(&json!({ "subscriptions": [gallery_job(Some("failed"), Some(90))] }));
        assert_eq!(
            gallery.observe(progress, &galleries, now).popups,
            [Popup::Error {
                title: "Gallery download paused".into(),
                message: "The run paused after an error. Retry it when ready.".into()
            }]
        );
    }

    #[test]
    fn removes_a_completed_gallery_import_without_treating_it_as_a_subscription() {
        let mut settle = Settle::default();
        let now = Instant::now();
        let (progress, galleries) =
            read_list(&json!({ "subscriptions": [gallery_job(Some("running"), Some(90))] }));
        let plan = settle.observe(progress, &galleries, now);
        // A running gallery's queries are not followed.
        assert!(plan.query_reads.is_empty() && plan.galleries.is_empty());
        let (progress, galleries) =
            read_list(&json!({ "subscriptions": [gallery_job(None, None)] }));
        let plan = settle.observe(progress, &galleries, now);
        assert!(plan.finished_reads.is_empty());
        assert_eq!(plan.galleries.len(), 1);
        let gallery = &plan.galleries[0];
        let run = settle
            .gallery_runs(
                gallery,
                &json!({ "runs": [{ "status": "succeeded", "error_message": null }] }),
            )
            .unwrap();
        assert_eq!(
            settle.gallery_cleaned(
                gallery,
                &run,
                Some(&json!({ "title": "Example Gallery", "already_exists": false }))
            ),
            Some(Popup::Success {
                title: "Gallery downloaded".into(),
                message: "Example Gallery has been downloaded.".into()
            })
        );
    }

    #[test]
    fn reports_an_all_known_gallery_and_a_failed_one() {
        let mut settle = Settle::default();
        let gallery = Gallery {
            id: 9,
            name: "E-Hentai Gallery 12345".into(),
            running: false,
        };
        let run = json!({ "status": "succeeded", "error_message": null });
        assert_eq!(
            settle.gallery_cleaned(
                &gallery,
                &run,
                Some(&json!({ "title": "Example Gallery", "already_exists": true }))
            ),
            Some(Popup::Success {
                title: "Gallery already exists".into(),
                message: "Example Gallery is already in the library.".into()
            })
        );
        let failed = json!({ "status": "failed", "failure_kind": "authentication",
            "error_message": "AuthorizationError: ExHentai rejected the saved cookies" });
        assert_eq!(
            settle.gallery_cleaned(&gallery, &failed, None),
            Some(Popup::Error {
                title: "Gallery import failed".into(),
                message: "AuthorizationError: ExHentai rejected the saved cookies".into()
            })
        );
        let cancelled = json!({ "status": "cancelled" });
        assert_eq!(settle.gallery_cleaned(&gallery, &cancelled, None), None);
    }

    #[test]
    fn keeps_a_gallery_import_until_its_run_has_ended() {
        let mut settle = Settle::default();
        let now = Instant::now();
        let (progress, galleries) =
            read_list(&json!({ "subscriptions": [gallery_job(None, None)] }));
        let gallery = settle.observe(progress.clone(), &galleries, now).galleries[0].clone();
        // No run record yet, then a pending run: nothing is cleaned up, and
        // the gallery is looked at again.
        assert_eq!(settle.gallery_runs(&gallery, &json!({ "runs": [] })), None);
        assert_eq!(
            settle
                .observe(progress.clone(), &galleries, now)
                .galleries
                .len(),
            1
        );
        assert_eq!(
            settle.gallery_runs(&gallery, &json!({ "runs": [{ "status": "pending" }] })),
            None
        );
        // While one is being settled it is not settled twice.
        assert_eq!(
            settle
                .observe(progress.clone(), &galleries, now)
                .galleries
                .len(),
            1
        );
        assert!(
            settle
                .observe(progress, &galleries, now)
                .galleries
                .is_empty()
        );
    }
}
