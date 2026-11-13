//! The commands the Subscriptions screen sends to the application, with the
//! arguments of `core/src/ipc.rs` that `subscriptionApi.ts` builds, and the
//! one place they leave from ([`Outbox`]).
use futures::channel::oneshot;
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

/// One command and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub name: &'static str,
    pub args: Value,
}

fn request(name: &'static str, args: Value) -> Request {
    Request { name, args }
}

/// `createSubscription`: a subscription with no sources, on the schedule
/// and run limits Settings gives new ones.
pub fn create(name: &str, schedule: &str, posts_per_run: i64) -> Request {
    request(
        "subscriptions.create",
        json!({
            "name": name,
            "schedule": schedule,
            "initial_post_limit": posts_per_run,
            "periodic_post_limit": posts_per_run,
            "queries": [],
        }),
    )
}

pub fn delete(subscription: i64) -> Request {
    request(
        "subscriptions.delete",
        json!({ "subscription_id": subscription }),
    )
}

/// Clears the handled-file history, so the next run reads every source from
/// the beginning.
pub fn reset(subscription: i64) -> Request {
    request(
        "subscriptions.reset",
        json!({ "subscription_id": subscription }),
    )
}

pub fn rename(subscription: i64, name: &str) -> Request {
    request(
        "subscriptions.rename",
        json!({ "subscription_id": subscription, "name": name }),
    )
}

/// Where downloaded files go: the folders and the tags each receives.
pub fn destination(subscription: i64, folders: &[i64], tags: &[String]) -> Request {
    request(
        "subscriptions.destination",
        json!({
            "subscription_id": subscription,
            "destination": { "target_folder_ids": folders, "automatic_tags": tags },
        }),
    )
}

/// A page of the media the subscription has added, 200 at a time.
pub fn cover_candidates(subscription: i64, cursor: Option<Value>) -> Request {
    request(
        "subscriptions.cover.candidates",
        json!({ "subscription_id": subscription, "cursor": cursor, "limit": 200 }),
    )
}

/// The chosen image and the square framed of it.
pub fn set_cover(subscription: i64, media_item: i64, crop: crate::cover::Crop) -> Request {
    request(
        "subscriptions.cover.set",
        json!({
            "subscription_id": subscription,
            "cover": {
                "media_item_id": media_item,
                "focus_x": crop.focus_x.round() as i64,
                "focus_y": crop.focus_y.round() as i64,
                "zoom_percent": crop.zoom.round() as i64,
            },
        }),
    )
}

/// What a source reads: its site, its query, and how it is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub site: String,
    pub text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub group_posts: bool,
}

impl Source {
    fn json(&self) -> Value {
        json!({
            "site_id": self.site,
            "query_text": self.text,
            "display_name": self.display_name,
            "notes": self.notes,
            "group_posts": self.group_posts,
        })
    }
}

pub fn add_query(subscription: i64, source: &Source) -> Request {
    request(
        "subscriptions.queries.add",
        json!({ "subscription_id": subscription, "query": source.json() }),
    )
}

pub fn update_query(query: i64, source: &Source) -> Request {
    request(
        "subscriptions.queries.update",
        json!({ "query_id": query, "query": source.json() }),
    )
}

pub fn delete_query(query: i64) -> Request {
    request("subscriptions.queries.delete", json!({ "query_id": query }))
}

/// Runs one source now. It downloads.
pub fn run_query(query: i64) -> Request {
    request("subscriptions.queries.run", json!({ "query_id": query }))
}

/// Holds a source out of future runs, or lets it run again.
pub fn pause_query(query: i64, paused: bool) -> Request {
    request(
        "subscriptions.queries.pause",
        json!({ "query_id": query, "paused": paused }),
    )
}

/// Whether a post's media become one group in the library.
pub fn group_query(query: i64, group_posts: bool) -> Request {
    request(
        "subscriptions.queries.grouping",
        json!({ "query_id": query, "group_posts": group_posts }),
    )
}

/// Downloads a gallery as a subscription of its own, which is removed again
/// once its run settles. It downloads.
pub fn start_gallery(url: &str) -> Request {
    request("subscriptions.gallery.start", json!({ "url": url }))
}

/// Removes a gallery download whose run has settled, answering with the
/// gallery's title and whether it was already in the library.
pub fn clean_up_gallery(subscription: i64) -> Request {
    request(
        "subscriptions.gallery.cleanup",
        json!({ "subscription_id": subscription }),
    )
}

/// The latest runs, newest first (`listRuns`).
pub fn runs(subscription: i64) -> Request {
    request(
        "subscriptions.runs.list",
        json!({ "subscription_id": subscription, "limit": 20 }),
    )
}

/// The run in progress, with the gallery's size once the site reports it.
pub fn progress(subscription: i64) -> Request {
    request(
        "subscriptions.progress.get",
        json!({ "subscription_id": subscription }),
    )
}

pub fn credentials() -> Request {
    request("auth.credentials.list", json!({}))
}

pub fn credential_health() -> Request {
    request("auth.health.list", json!({}))
}

/// Whether a command starts work on the network: a run, a source's run, a
/// run resumed, or a gallery download.
pub fn starts_network(name: &str) -> bool {
    matches!(
        name,
        "subscriptions.run"
            | "subscriptions.run.resume"
            | "subscriptions.queries.run"
            | "subscriptions.gallery.start"
    )
}

/// Where the screen's commands go. Live, they go to the application. Held
/// (`PICTO_GPUI_HOLD_RUNS` set), each is written down, and those that would
/// start work on the network are answered at once instead of sent, so a
/// script can press Run and check what would have been asked for.
#[derive(Clone)]
pub enum Outbox {
    Live,
    Held(Rc<RefCell<Vec<Request>>>),
}

impl Outbox {
    pub fn from_environment() -> Self {
        if std::env::var_os("PICTO_GPUI_HOLD_RUNS").is_some() {
            Outbox::Held(Rc::default())
        } else {
            Outbox::Live
        }
    }

    /// Sends `request` through `dispatch` unless it is held, and hands back
    /// the answer.
    pub fn send(
        &self,
        request: Request,
        dispatch: impl FnOnce(&'static str, Value) -> oneshot::Receiver<Result<Value, String>>,
    ) -> oneshot::Receiver<Result<Value, String>> {
        match self {
            Outbox::Live => dispatch(request.name, request.args),
            Outbox::Held(log) => {
                eprintln!(
                    "picto: subscriptions command {} {}",
                    request.name, request.args
                );
                log.borrow_mut().push(request.clone());
                if starts_network(request.name) {
                    let (answer, receiver) = oneshot::channel();
                    let _ = answer.send(Ok(json!({ "held": true })));
                    receiver
                } else {
                    dispatch(request.name, request.args)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picto_core::ipc;

    /// Each request's arguments read as the application's own input type.
    macro_rules! reads_as {
        ($input:ty, $request:expr) => {{
            let request: Request = $request;
            if let Err(error) = serde_json::from_value::<$input>(request.args.clone()) {
                panic!("{} {}: {error}", request.name, request.args);
            }
        }};
    }

    fn source() -> Source {
        Source {
            site: "e621".into(),
            text: "canine solo".into(),
            display_name: Some("Dogs".into()),
            notes: None,
            group_posts: false,
        }
    }

    #[test]
    fn arguments_are_the_ones_the_application_reads() {
        reads_as!(
            picto_core::subscription_catalog::NewSubscription,
            create("A", "manual", 100)
        );
        reads_as!(ipc::SubscriptionInput, delete(3));
        reads_as!(ipc::SubscriptionInput, reset(3));
        reads_as!(ipc::RenameSubscriptionInput, rename(3, "B"));
        reads_as!(
            ipc::SubscriptionDestinationInput,
            destination(3, &[4, 5], &["artist:x".into()])
        );
        reads_as!(
            ipc::SubscriptionCoverCandidatesInput,
            cover_candidates(3, None)
        );
        reads_as!(
            ipc::SubscriptionCoverCandidatesInput,
            cover_candidates(
                3,
                Some(json!({ "imported_at": "2026-01-01T00:00:00Z", "media_item_id": 9 })),
            )
        );
        reads_as!(
            ipc::SubscriptionCoverInput,
            set_cover(3, 9, crate::cover::Crop::default())
        );
        reads_as!(ipc::AddSubscriptionQueryInput, add_query(3, &source()));
        reads_as!(
            ipc::UpdateSubscriptionQueryInput,
            update_query(7, &source())
        );
        reads_as!(ipc::SubscriptionQueryInput, delete_query(7));
        reads_as!(ipc::SubscriptionQueryInput, run_query(7));
        reads_as!(ipc::PauseSubscriptionQueryInput, pause_query(7, true));
        reads_as!(
            ipc::SetSubscriptionQueryGroupingInput,
            group_query(7, false)
        );
        reads_as!(
            ipc::GalleryImportInput,
            start_gallery("https://e-hentai.org/g/1/a/")
        );
        reads_as!(ipc::SubscriptionInput, clean_up_gallery(3));
        reads_as!(ipc::SubscriptionRunsInput, runs(3));
        reads_as!(ipc::SubscriptionInput, progress(3));
    }

    #[test]
    fn requests_carry_what_the_electron_screen_sends() {
        assert_eq!(
            create("Artists", "daily", 250),
            Request {
                name: "subscriptions.create",
                args: json!({
                    "name": "Artists",
                    "schedule": "daily",
                    "initial_post_limit": 250,
                    "periodic_post_limit": 250,
                    "queries": [],
                }),
            }
        );
        assert_eq!(
            add_query(3, &source()).args,
            json!({
                "subscription_id": 3,
                "query": {
                    "site_id": "e621",
                    "query_text": "canine solo",
                    "display_name": "Dogs",
                    "notes": null,
                    "group_posts": false,
                },
            })
        );
        assert_eq!(
            set_cover(
                3,
                9,
                crate::cover::Crop {
                    focus_x: 250.4,
                    focus_y: 700.,
                    zoom: 135.,
                }
            )
            .args,
            json!({
                "subscription_id": 3,
                "cover": { "media_item_id": 9, "focus_x": 250, "focus_y": 700, "zoom_percent": 135 },
            })
        );
        assert_eq!(
            destination(3, &[4], &[]).args,
            json!({
                "subscription_id": 3,
                "destination": { "target_folder_ids": [4], "automatic_tags": [] },
            })
        );
        assert_eq!(run_query(7).name, "subscriptions.queries.run");
        assert_eq!(start_gallery("u").args, json!({ "url": "u" }));
    }

    #[test]
    fn only_runs_and_gallery_downloads_reach_the_network() {
        for name in [
            "subscriptions.run",
            "subscriptions.run.resume",
            "subscriptions.queries.run",
            "subscriptions.gallery.start",
        ] {
            assert!(starts_network(name), "{name}");
        }
        for request in [
            create("A", "manual", 1),
            delete(1),
            reset(1),
            rename(1, "B"),
            destination(1, &[], &[]),
            set_cover(1, 2, crate::cover::Crop::default()),
            add_query(1, &source()),
            update_query(1, &source()),
            delete_query(1),
            pause_query(1, true),
            group_query(1, true),
            clean_up_gallery(1),
        ] {
            assert!(!starts_network(request.name), "{}", request.name);
        }
    }

    #[test]
    fn a_held_outbox_writes_down_every_command_and_sends_no_run() {
        let outbox = Outbox::Held(Rc::default());
        let sent = RefCell::new(Vec::new());
        let dispatch = |name: &'static str, _: Value| {
            sent.borrow_mut().push(name);
            oneshot::channel().1
        };
        let mut answer = outbox.send(run_query(7), dispatch);
        assert_eq!(
            answer.try_recv().unwrap(),
            Some(Ok(json!({ "held": true })))
        );
        let _ = outbox.send(start_gallery("u"), dispatch);
        let _ = outbox.send(pause_query(7, true), dispatch);
        assert_eq!(*sent.borrow(), vec!["subscriptions.queries.pause"]);
        let Outbox::Held(log) = &outbox else {
            unreachable!()
        };
        assert_eq!(
            log.borrow()
                .iter()
                .map(|request| request.name)
                .collect::<Vec<_>>(),
            vec![
                "subscriptions.queries.run",
                "subscriptions.gallery.start",
                "subscriptions.queries.pause",
            ]
        );
    }
}
