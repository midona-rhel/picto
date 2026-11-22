//! Decides which notifications to show about subscription runs and gallery
//! downloads. The notifications are: a run that an error paused
//! ("Subscription paused" or "Gallery download paused"), a query of a run
//! that finished ("Query completed"), a run that finished ("Subscription
//! completed"), and a gallery download whose run has ended ("Gallery
//! downloaded", "Gallery already exists" or "Gallery import failed"). A
//! gallery download is a subscription of its own, and that subscription is
//! removed when its run has ended.
//!
//! The session reads `subscriptions.list` whenever the application layer
//! reports that subscriptions or tasks changed, and every 1.5 seconds while
//! a subscription is running. It does so whichever destination is open, so
//! the notifications also appear while the Subscriptions screen is not
//! shown. The session gives each answer to `Settle::observe`, which returns
//! the notifications to show and the further reads to make. The answers of
//! those reads go to the other methods of `Settle`. `Settle` reads no clock
//! and sends no command itself, so the tests call it with recorded answers.
use crate::i18n::t;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// The activity of a run whose state in `subscriptions.list` has not changed
/// is read again when 10 seconds have passed since it was last requested.
pub const RUN_ACTIVITY_REFRESH: Duration = Duration::from_secs(10);

/// The state of one subscription that has an active run, as `read_list`
/// takes it from the answer of `subscriptions.list`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// The id of the subscription.
    pub subscription: i64,
    /// The name of the subscription.
    pub name: String,
    /// The id of the subscription's active run.
    pub run: Option<i64>,
    /// The subscription's `status` field, for example `running`.
    pub phase: String,
    /// The subscription's `status` when it is not `pending`, `running`,
    /// `paused` or `inbox_full`. Such a status means that an error stopped
    /// the run.
    pub failure_kind: Option<String>,
    /// The first `last_failure_message` among the subscription's queries.
    pub error: Option<String>,
}

impl Progress {
    /// Returns a text made of the run's phase and its failure kind.
    /// `Settle::observe` asks for the run's activity again when this text
    /// differs from the one of the previous request.
    fn fingerprint(&self) -> String {
        format!(
            "\u{0}{}\u{0}\u{0}{}",
            self.phase,
            self.failure_kind.as_deref().unwrap_or_default()
        )
    }
}

/// A gallery download. It is a subscription of its own that has exactly one
/// query, and the site of that query is `ehentai`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gallery {
    /// The id of the gallery download's subscription.
    pub id: i64,
    /// The name of that subscription.
    pub name: String,
    /// Whether the subscription has an active run whose status is `pending`
    /// or `running`.
    pub running: bool,
}

/// Reads the answer of `subscriptions.list`. Returns the progress of every
/// subscription that has an active run and a status, and every gallery
/// download.
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

/// A notification to show: a success notification or an error notification,
/// with its title and its message.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    Success { title: String, message: String },
    Error { title: String, message: String },
}

/// What `Settle::observe` asks the session to do after a read of
/// `subscriptions.list`.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// The notifications to show now.
    pub popups: Vec<Popup>,
    /// The runs whose activity is to be read with `subscriptions.runs.get`,
    /// to find the queries that have finished. The `Instant` is the time of
    /// this call to `observe`. The session passes it to
    /// `Settle::query_activity_failed` if the read fails.
    pub query_reads: Vec<(Progress, Instant)>,
    /// The runs that were active in the previous read and are no longer
    /// active. Their activity is read to learn how they ended.
    pub finished_reads: Vec<Progress>,
    /// The gallery downloads that are not running. The latest run of each is
    /// read with `subscriptions.runs.list`.
    pub galleries: Vec<Gallery>,
}

/// Whether `status` is a status of a run or a query that finished without
/// an error.
fn successful(status: &str) -> bool {
    matches!(status, "completed" | "succeeded" | "success")
}

/// Returns the text that says how many posts were added to the library, for
/// example "12 posts added to library".
fn posts_added(count: i64) -> String {
    if count == 1 {
        t("1 post added to library").to_string()
    } else {
        crate::tf!("{value0} posts added to library", count).to_string()
    }
}

/// Remembers what earlier reads returned and what has been announced, so
/// that each notification is shown once.
#[derive(Default)]
pub struct Settle {
    /// The progress that the previous read of `subscriptions.list` returned.
    previous: Vec<Progress>,
    /// For each run, by run id: the status of each of its queries, by query
    /// id, when the run's activity was last read.
    queries: HashMap<i64, HashMap<i64, String>>,
    /// For each run, by run id: the run's fingerprint and the time when its
    /// activity was last requested.
    observed: HashMap<i64, (String, Instant)>,
    /// The ids of the runs whose successful end has been announced.
    notified_runs: HashSet<i64>,
    /// The pauses by an error that have been announced. Each is a key made
    /// of the run id, the failure kind and the error message.
    notified_pauses: HashSet<String>,
    /// The ids of the gallery downloads whose latest run is being read or
    /// whose cleanup is in progress. `observe` does not put a gallery
    /// download of this set in its `Plan`.
    settling_galleries: HashSet<i64>,
}

impl Settle {
    /// Takes the result of a new read of `subscriptions.list`, made at the
    /// time `now`, and returns the notifications to show and the reads to
    /// make next.
    pub fn observe(&mut self, current: Vec<Progress>, galleries: &[Gallery], now: Instant) -> Plan {
        let gallery_ids: HashSet<i64> = galleries.iter().map(|gallery| gallery.id).collect();
        let mut plan = Plan::default();
        // Announce each run that an error stopped, once for each combination
        // of run, failure kind and error message. A run that the user paused,
        // or that stopped because the Inbox is full, is not announced.
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
        // Ask for the activity of each run whose fingerprint has changed, or
        // whose activity was last requested 10 seconds ago or longer
        // (`RUN_ACTIVITY_REFRESH`). Runs of gallery downloads are skipped.
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
        // Ask for the activity of each run that was active in the previous
        // read and is not active now, unless its successful end has already
        // been announced. Runs of gallery downloads are skipped.
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
        // Ask for the latest run of each gallery download that is not
        // running and is not being handled already.
        for gallery in galleries {
            if !gallery.running && self.settling_galleries.insert(gallery.id) {
                plan.galleries.push(gallery.clone());
            }
        }
        self.previous = current;
        plan
    }

    /// Takes the activity of the run of `entry`, which is the answer of
    /// `subscriptions.runs.get`. Returns a "Query completed" notification
    /// for each query that has finished successfully since the run's
    /// activity was last read. The first read of a run returns no
    /// notification; it only records the status of the run's queries.
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

    /// Called when the activity of the run of `entry` could not be read.
    /// `checked` is the time that `observe` returned with the request. The
    /// record of that request is removed, so that the next call to `observe`
    /// asks for the activity again even if the run's state has not changed.
    /// Nothing is removed when a newer request is recorded for the run.
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

    /// Takes the activity of a run that is no longer active. Returns a
    /// "Subscription completed" notification if the run finished
    /// successfully, and records the run so that it is announced only once.
    /// Returns `None` for a run that ended in any other way.
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

    /// Takes the answer of `subscriptions.runs.list` for a gallery download
    /// and returns the gallery download's latest run if that run has ended.
    /// The session then removes the gallery download with
    /// `subscriptions.gallery.cleanup` and gives the answer to
    /// [`Settle::gallery_cleaned`], which returns the notification. Returns
    /// `None` when the gallery download has no run yet, or when its latest
    /// run is pending or running. In that case a later call to `observe`
    /// puts the gallery download in its `Plan` again.
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

    /// Returns the notification for a gallery download whose latest run
    /// `run` has ended. `cleanup` is the answer of
    /// `subscriptions.gallery.cleanup`, which gives the gallery's title and
    /// whether the gallery was already in the library. A run that succeeded
    /// gives "Gallery downloaded" or "Gallery already exists", a cancelled
    /// run gives no notification, and any other run gives "Gallery import
    /// failed". After this call, `observe` can put the gallery download in
    /// its `Plan` again if `subscriptions.list` still lists it.
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

    /// Called when the read of a gallery download's runs or its cleanup
    /// failed. A later call to `observe` puts the gallery download in its
    /// `Plan` again.
    pub fn gallery_failed(&mut self, gallery: &Gallery) {
        self.settling_galleries.remove(&gallery.id);
    }
}

/// Returns the queries in a run's activity.
fn queries(activity: &Value) -> impl Iterator<Item = &Value> {
    activity["queries"].as_array().into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Returns an answer of `subscriptions.runs.get` for run 44 of
    /// subscription 7. The run has one query. `run_status` is the status of
    /// the run and `query_status` is the status of the query.
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

    /// Returns an entry of `subscriptions.list` for subscription 7, which
    /// has the active run 44 and the status `status`.
    fn feed(status: &str) -> Value {
        json!({ "subscription_id": 7, "name": "Active feed", "status": status,
            "active_run_id": 44, "queries": [{ "site_id": "pixiv", "last_failure_message": null }] })
    }

    /// Returns an entry of `subscriptions.list` for the gallery download
    /// with subscription id 9, with the given status and active run.
    fn gallery_job(status: Option<&str>, run: Option<i64>) -> Value {
        json!({ "subscription_id": 9, "name": "E-Hentai Gallery 12345", "status": status,
            "active_run_id": run, "queries": [{ "site_id": "ehentai" }] })
    }

    /// Returns the progress that `read_list` reads from these entries of
    /// `subscriptions.list`.
    fn list(subscriptions: Vec<Value>) -> Vec<Progress> {
        read_list(&json!({ "subscriptions": subscriptions })).0
    }

    #[test]
    fn reads_running_progress_and_galleries_as_the_earlier_version_mapped_them() {
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
        // The first read of the activity only records the status of the
        // queries.
        assert!(
            settle
                .query_activity(&entry, &run_activity("running", "running"))
                .is_empty()
        );
        // The run's phase has changed, so its activity is requested again.
        // This time the query has finished.
        let plan = settle.observe(list(vec![feed("settling")]), &[], now);
        assert_eq!(plan.query_reads.len(), 1);
        assert_eq!(
            settle.query_activity(&entry, &run_activity("running", "succeeded")),
            [Popup::Success {
                title: "Query completed".into(),
                message: "Active feed · artist-name · 12 posts added to library".into(),
            }]
        );
        // The same activity, read again, gives no second notification.
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
        // The activity is requested again when the previous read failed, or
        // when 10 seconds have passed since the previous request.
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
        // The end of a run is announced once. A run that failed is not
        // announced.
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
        // A run that the user paused, or that stopped because the Inbox is
        // full, is not announced as an error.
        let mut quiet = Settle::default();
        assert!(
            quiet
                .observe(list(vec![feed("paused"), feed("inbox_full")]), &[], now)
                .popups
                .is_empty()
        );
        // A gallery download is announced with its own title. When the list
        // gives no error message, the standard message is shown.
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
        // While a gallery download is running, neither the activity of its
        // run nor its latest run is requested.
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
        // First the gallery download has no run, then its latest run is
        // pending. In both cases `gallery_runs` returns no run to clean up,
        // and the next call to `observe` puts the gallery download in its
        // plan again.
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
        // While the latest run of a gallery download is being read, `observe`
        // does not put the gallery download in its plan a second time.
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
