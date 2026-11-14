//! What the Subscriptions screen says about a subscription: its status, its
//! times and counts, as `subscriptionUtils.ts` and the components word them.
use super::data::{Query, Run, Subscription};
use crate::i18n::format::{Unit, relative};
use crate::i18n::{en, t};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use picto_core::subscriptions::sites::{SITES, SiteEntry};

pub use crate::ui::status_badge::Tone;

/// A run that is under way rather than waiting: the application's
/// `runningSubscriptionIds`.
pub fn running(subscription: &Subscription) -> bool {
    subscription.active_run_id.is_some()
        && matches!(
            subscription.run_status.as_deref(),
            Some("pending" | "running")
        )
}

/// `isQueryCompleted`
pub fn query_completed(query: &Query) -> bool {
    !query.paused
        && query.initial_run_complete
        && query.history_complete
        && query.successful_runs >= 1
        && query.last_success_at.is_some()
        && query.last_failure_kind.is_none()
}

/// `isSubscriptionCompleted`
pub fn subscription_completed(subscription: &Subscription) -> bool {
    !subscription.paused
        && !subscription.queries.is_empty()
        && subscription.queries.iter().all(query_completed)
}

/// The badge on a card in the grid (`FollowCard`).
pub fn card_status(subscription: &Subscription) -> (Tone, &'static str) {
    let running = running(subscription);
    let active = subscription.active_run_id.is_some();
    if subscription.run_status.as_deref() == Some("inbox_full") {
        (Tone::Paused, t("Inbox full"))
    } else if subscription.paused || (active && !running) {
        (Tone::Paused, t("Paused"))
    } else if running {
        (Tone::Running, t("Syncing"))
    } else if subscription.open_issues > 0 {
        (Tone::Attention, t("Warning"))
    } else if !active && subscription_completed(subscription) {
        (Tone::Success, t("Complete"))
    } else {
        (Tone::Idle, t("Idle"))
    }
}

/// The badge under the name in the detail rail (`SubscriptionDetail`). An
/// active run that is neither going nor waiting for the user stopped on a
/// failure.
pub fn detail_status(subscription: &Subscription) -> (Tone, &'static str) {
    let active = subscription.active_run_id.is_some();
    let running = running(subscription);
    let status = subscription.run_status.as_deref();
    let waiting_for_inbox = active && status == Some("inbox_full");
    let run_paused = active && !running;
    let run_failed = run_paused && !matches!(status, Some("paused" | "inbox_full"));
    let completed = !active && subscription_completed(subscription);
    let tone = if run_failed {
        Tone::Attention
    } else if waiting_for_inbox {
        Tone::Paused
    } else if running {
        Tone::Running
    } else if subscription.paused || run_paused {
        Tone::Paused
    } else if active {
        // An active run reports progress, which reads as syncing.
        Tone::Running
    } else if subscription.open_issues > 0 {
        Tone::Attention
    } else if completed {
        Tone::Success
    } else {
        Tone::Idle
    };
    let label = if run_failed {
        t("Warning")
    } else if waiting_for_inbox {
        t("Inbox full")
    } else {
        match tone {
            Tone::Running => t("Syncing"),
            Tone::Paused => t("Paused"),
            Tone::Attention => t("Warning"),
            Tone::Success => t("Complete"),
            Tone::Idle => t("Idle"),
        }
    };
    (tone, label)
}

/// The badge of a row in the History tab (`HistoryTab.tsx`).
pub fn run_status(run: &Run) -> (Tone, String) {
    let pending = run.status == "pending";
    let waiting = pending && run.failure_kind.as_deref() == Some("inbox_full");
    let paused = pending && run.failure_kind.as_deref() == Some("paused");
    let tone = match run.status.as_str() {
        _ if waiting || paused => Tone::Paused,
        "failed" => Tone::Attention,
        "running" => Tone::Running,
        "paused" => Tone::Paused,
        "completed" | "succeeded" => Tone::Success,
        _ => Tone::Idle,
    };
    let label = match run.status.as_str() {
        "completed" | "succeeded" => t("Complete").to_owned(),
        _ if waiting => t("Inbox full").to_owned(),
        _ if paused => t("Paused").to_owned(),
        _ => match &run.failure_kind {
            Some(kind) => kind.replace('_', " "),
            None => run.status.clone(),
        },
    };
    (tone, label)
}

/// The badge of an issue in the Problems tab.
pub fn issue_label(kind: &str) -> String {
    if kind == "download_item" {
        // i18n: production's badge, in English
        "download".to_owned()
    } else {
        kind.replace('_', " ")
    }
}

/// `describeFailure`: what to do about a query's recorded failure.
pub fn describe_failure(kind: Option<&str>, message: Option<&str>) -> Option<String> {
    let text = match kind {
        Some("not_found") => t("User/query not found — check the handle"),
        Some("unauthorized") => t("Login rejected — check the account in Accounts"),
        Some("expired") => t("Session expired — log in again in Accounts"),
        Some("rate_limited") => {
            t("Rate limited by the site — resumes automatically when the limit resets")
        }
        Some("network") => t("Network error — check your connection"),
        Some("environment") => t("Local setup problem — check the logs"),
        Some("stale") => t("Interrupted by app shutdown"),
        Some("inbox_full") => t("Paused — inbox is full"),
        _ => return message.map(str::to_owned),
    };
    Some(text.to_owned())
}

/// `describeGalleryFailure`
pub fn describe_gallery_failure(kind: Option<&str>, message: Option<&str>) -> &'static str {
    let message = message.unwrap_or_default().to_lowercase();
    if message.contains("requires gp") {
        t("Original requires GP")
    } else if message.contains("image viewing limit") {
        t("Image limit reached")
    } else if message.contains("temporarily blocked") {
        t("Temporarily rate limited")
    } else if message.contains("web page instead of a file") {
        t("Invalid media response")
    } else {
        match kind {
            Some("unauthorized" | "expired") => t("Login required"),
            Some("rate_limited") => t("Rate limited"),
            _ => t("Download failed"),
        }
    }
}

/// A gallery download: a subscription of one E-Hentai query, which the
/// screen shows in its header rather than as a card.
pub fn is_gallery_job(subscription: &Subscription) -> bool {
    subscription.queries.len() == 1 && subscription.queries[0].site_id == "ehentai"
}

/// The sources the screen offers (`getSubscriptionSites`): the source
/// catalog the application answers `sources.list` with, less the sources the
/// interface leaves out.
pub fn sites() -> impl Iterator<Item = &'static SiteEntry> {
    /// `REMOVED_SITE_IDS`
    const REMOVED: [&str; 1] = ["tumblr"];
    SITES.iter().filter(|site| !REMOVED.contains(&site.id))
}

/// The catalog's entry for a source.
pub fn site(site_id: &str) -> Option<&'static SiteEntry> {
    sites().find(|site| site.id == site_id)
}

/// `getSiteLabel`: the source's name, or its id when it is not offered.
pub fn site_label(site_id: &str) -> String {
    site(site_id).map_or_else(|| site_id.to_owned(), |site| site.name.to_owned())
}

/// The sources sorted by name, as the edit dialog lists them.
pub fn sites_by_name() -> Vec<&'static SiteEntry> {
    let mut sites = sites().collect::<Vec<_>>();
    sites.sort_by_key(|site| site.name.to_lowercase());
    sites
}

/// The sources a query can be added for (`AddQueryBar`), sorted by name;
/// galleries are added elsewhere.
pub fn query_sites() -> Vec<&'static SiteEntry> {
    sites_by_name()
        .into_iter()
        .filter(|site| site.id != "ehentai")
        .collect()
}

/// `getSubscriptionRunTarget`: the posts a run means to settle, the per-run
/// limit for each source that is not on hold.
pub fn run_target(subscription: &Subscription) -> i64 {
    let per_query = if subscription.posts_per_run > 0 {
        subscription.posts_per_run
    } else {
        100
    };
    let active = subscription
        .queries
        .iter()
        .filter(|query| !query.paused)
        .count() as i64;
    per_query * active.max(1)
}

/// The newest success or failure of any query: "Last check".
pub fn last_check(subscription: &Subscription) -> Option<&str> {
    subscription
        .queries
        .iter()
        .flat_map(|query| {
            [
                query.last_success_at.as_deref(),
                query.last_failure_at.as_deref(),
            ]
        })
        .flatten()
        .max()
}

/// A source's own last check: its newest success or failure.
pub fn query_last_check(query: &Query) -> Option<&str> {
    [
        query.last_success_at.as_deref(),
        query.last_failure_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// The source details' State: on hold, failed, caught up, or still reading
/// its history for the first time.
pub fn query_state(query: &Query) -> &'static str {
    if query.paused {
        t("Paused")
    } else if query.last_failure_message.is_some() {
        t("Failed")
    } else if query.initial_run_complete {
        t("Ready")
    } else {
        t("Initial sync")
    }
}

/// A site's stored login, as `auth.credentials.list` and
/// `auth.health.list` report it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Login {
    pub site: String,
    /// The last check's status, when it has been checked.
    pub health: Option<String>,
    pub last_error: Option<String>,
}

/// `getQueryAuthState`, for the chip a source shows when its site needs a
/// login before it can run: the chip's text, or `None` when nothing blocks.
pub fn auth_warning(site_id: &str, logins: &[Login], health: &[Login]) -> Option<String> {
    let site = site(site_id)?;
    let owner = site.credential_owner_site_id;
    let missing = !logins.iter().any(|login| login.site == owner);
    let checked = health.iter().find(|login| login.site == owner);
    let status = checked
        .and_then(|login| login.health.as_deref())
        .unwrap_or_default()
        .to_lowercase();
    let broken = matches!(
        status.as_str(),
        "unauthorized" | "expired" | "error" | "missing"
    );
    if !(broken || (site.auth_strictly_required && missing)) {
        return None;
    }
    Some(if missing {
        t("Sign in required").to_owned()
    } else {
        checked
            .and_then(|login| login.last_error.as_deref())
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .unwrap_or(en("Sign in again"))
            .to_owned()
    })
}

/// The gallery service an address belongs to, as the Add Gallery dialog
/// follows what is typed: `exhentai` or `ehentai`.
pub fn gallery_service(url: &str) -> Option<&'static str> {
    let url = url.trim().to_lowercase();
    let rest = url
        .strip_prefix("https://")
        // i18n: a URL's scheme
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    if rest.starts_with("exhentai.org/") {
        Some("exhentai")
    } else if rest.starts_with("e-hentai.org/") {
        Some("ehentai")
    } else {
        None
    }
}

/// The query whose failure is the latest, for the warning's explanation.
pub fn latest_failure(subscription: &Subscription) -> Option<&Query> {
    subscription
        .queries
        .iter()
        .filter(|query| query.last_failure_message.is_some())
        .max_by(|left, right| left.last_failure_at.cmp(&right.last_failure_at))
}

/// What a card says under its name: items, then sources when there are any.
pub fn card_meta(items: i64, sources: usize) -> String {
    // `{items} {t("item")}{items === 1 ? '' : t("s")}`
    let mut meta = format!(
        "{} {}{}",
        crate::grid::format::grouped(items),
        t("item"),
        if items == 1 { "" } else { t("s") }
    );
    if sources > 0 {
        meta.push_str(&format!(
            " · {sources} source{}",
            // i18n: production writes the sources in English
            if sources == 1 { "" } else { "s" }
        ));
    }
    meta
}

/// A stored time, as `Date.parse` reads it: RFC 3339, or a plain date and
/// time in local time.
fn parse_time(value: &str) -> Option<DateTime<Local>> {
    if let Ok(time) = DateTime::parse_from_rfc3339(value) {
        return Some(time.with_timezone(&Local));
    }
    [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
    ]
    .into_iter()
    .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
    .and_then(|naive| Local.from_local_datetime(&naive).earliest())
}

/// JavaScript's `Math.round`: halves go up.
fn round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// `formatRelativeTime`, relative to `now`, in the system's language as
/// `Intl.RelativeTimeFormat` without a locale writes it.
pub fn relative_time(value: Option<&str>, now: DateTime<Local>) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return t("Never").to_owned();
    };
    let Some(time) = parse_time(value) else {
        return value.to_owned();
    };
    let minutes = round((time - now).num_milliseconds() as f64 / 60_000.);
    if minutes.abs() < 60 {
        return relative(minutes, Unit::Minute);
    }
    let hours = round(minutes as f64 / 60.);
    if hours.abs() < 48 {
        return relative(hours, Unit::Hour);
    }
    let days = round(hours as f64 / 24.);
    if days.abs() < 30 {
        return relative(days, Unit::Day);
    }
    relative(round(days as f64 / 30.), Unit::Month)
}

/// `formatDateTime`: the medium date and short time, as en-US writes them.
pub fn date_time(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return t("None").to_owned();
    };
    parse_time(value).map_or_else(
        || value.to_owned(),
        |time| crate::i18n::format::medium_date_time(time.naive_local()),
    )
}

/// The grid's order: by name, then by the card's key.
pub fn card_order(subscriptions: &mut [&Subscription]) {
    subscriptions.sort_by(|left, right| {
        crate::text::natural_order(&left.name, &right.name)
            .then_with(|| format!("sub:{}", left.id).cmp(&format!("sub:{}", right.id)))
    });
}

/// A CSS grid track.
#[derive(Clone, Copy, Debug)]
pub enum Track {
    Fixed(f32),
    /// `minmax(min, max)` with lengths: as wide as the room allows.
    Between(f32, f32),
    /// `minmax(min, <n>fr)`
    Flex(f32, f32),
}

/// The widths CSS grid gives these tracks across `width` with `gap` between
/// them: fixed tracks first, bounded ones up to their maximum, and the rest
/// shared by fraction, a flexible track never narrower than its minimum.
pub fn tracks(width: f32, gap: f32, tracks: &[Track]) -> Vec<f32> {
    let gaps = gap * tracks.len().saturating_sub(1) as f32;
    let minimum: f32 = tracks
        .iter()
        .map(|track| match *track {
            Track::Fixed(size) => size,
            Track::Between(min, _) | Track::Flex(min, _) => min,
        })
        .sum();
    // Bounded tracks grow toward their maximum with whatever the minimums
    // leave over, before the flexible tracks share what is left.
    let mut spare = (width - gaps - minimum).max(0.);
    let mut sizes: Vec<f32> = tracks
        .iter()
        .map(|track| match *track {
            Track::Fixed(size) => size,
            Track::Between(min, max) => {
                let grown = (max - min).min(spare);
                spare -= grown;
                min + grown
            }
            Track::Flex(min, _) => min,
        })
        .collect();
    let fixed: f32 = tracks
        .iter()
        .zip(&sizes)
        .filter(|(track, _)| !matches!(track, Track::Flex(..)))
        .map(|(_, size)| size)
        .sum();
    let mut free = (width - gaps - fixed).max(0.);
    let mut flexible: Vec<usize> = (0..tracks.len())
        .filter(|index| matches!(tracks[*index], Track::Flex(..)))
        .collect();
    // A track whose share would fall under its minimum keeps the minimum and
    // leaves the others to share what remains.
    loop {
        let fractions: f32 = flexible
            .iter()
            .map(|index| match tracks[*index] {
                Track::Flex(_, fraction) => fraction,
                _ => 0.,
            })
            .sum();
        if fractions <= 0. {
            break;
        }
        let unit = free / fractions.max(1.);
        let under = flexible.iter().position(|index| match tracks[*index] {
            Track::Flex(min, fraction) => unit * fraction < min,
            _ => false,
        });
        match under {
            Some(position) => {
                let index = flexible.remove(position);
                if let Track::Flex(min, _) = tracks[index] {
                    sizes[index] = min;
                    free = (free - min).max(0.);
                }
            }
            None => {
                for index in &flexible {
                    if let Track::Flex(_, fraction) = tracks[*index] {
                        sizes[*index] = unit * fraction;
                    }
                }
                break;
            }
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::{
        Login, Query, Run, Subscription, Tone, Track, auth_warning, card_meta, card_status,
        date_time, describe_failure, detail_status, gallery_service, query_sites, query_state,
        relative_time, run_status, run_target, site_label, tracks,
    };
    use chrono::{Duration, Local, TimeZone};

    fn query(completed: bool) -> Query {
        Query {
            initial_run_complete: completed,
            history_complete: completed,
            successful_runs: completed as i64,
            last_success_at: completed.then(|| "2026-11-01T00:00:00Z".to_owned()),
            ..Query::default()
        }
    }

    #[test]
    fn a_card_reads_its_state_in_the_order_the_grid_does() {
        let mut subscription = Subscription {
            queries: vec![query(true)],
            ..Subscription::default()
        };
        assert_eq!(card_status(&subscription), (Tone::Success, "Complete"));
        subscription.open_issues = 2;
        assert_eq!(card_status(&subscription), (Tone::Attention, "Warning"));
        subscription.active_run_id = Some(4);
        subscription.run_status = Some("running".into());
        assert_eq!(card_status(&subscription), (Tone::Running, "Syncing"));
        // An active run that is not going is paused, and a full inbox says so.
        subscription.run_status = Some("paused".into());
        assert_eq!(card_status(&subscription), (Tone::Paused, "Paused"));
        subscription.run_status = Some("inbox_full".into());
        assert_eq!(card_status(&subscription), (Tone::Paused, "Inbox full"));
        let idle = Subscription {
            queries: vec![query(false)],
            ..Subscription::default()
        };
        assert_eq!(card_status(&idle), (Tone::Idle, "Idle"));
    }

    #[test]
    fn the_rail_calls_a_stopped_run_a_warning() {
        let mut subscription = Subscription {
            active_run_id: Some(1),
            run_status: Some("network".into()),
            ..Subscription::default()
        };
        assert_eq!(detail_status(&subscription), (Tone::Attention, "Warning"));
        subscription.run_status = Some("paused".into());
        assert_eq!(detail_status(&subscription), (Tone::Paused, "Paused"));
        subscription.run_status = Some("inbox_full".into());
        assert_eq!(detail_status(&subscription), (Tone::Paused, "Inbox full"));
        subscription.run_status = Some("pending".into());
        assert_eq!(detail_status(&subscription), (Tone::Running, "Syncing"));
        subscription.active_run_id = None;
        subscription.paused = true;
        assert_eq!(detail_status(&subscription), (Tone::Paused, "Paused"));
    }

    #[test]
    fn history_rows_name_the_failure() {
        let run = |status: &str, kind: Option<&str>| Run {
            id: 1,
            status: status.into(),
            started_at: String::new(),
            failure_kind: kind.map(str::to_owned),
            error_message: None,
            downloaded: 0,
            already_in_library: 0,
        };
        assert_eq!(
            run_status(&run("succeeded", None)),
            (Tone::Success, "Complete".into())
        );
        assert_eq!(
            run_status(&run("failed", Some("rate_limited"))),
            (Tone::Attention, "rate limited".into())
        );
        assert_eq!(
            run_status(&run("pending", Some("inbox_full"))),
            (Tone::Paused, "Inbox full".into())
        );
        assert_eq!(
            run_status(&run("cancelled", None)),
            (Tone::Idle, "cancelled".into())
        );
    }

    #[test]
    fn counts_are_grouped_and_counted() {
        assert_eq!(card_meta(1, 0), "1 item");
        assert_eq!(card_meta(2400, 1), "2,400 items · 1 source");
        assert_eq!(card_meta(0, 3), "0 items · 3 sources");
    }

    #[test]
    fn relative_times_round_as_the_browser_does() {
        let now = Local.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap();
        let at = |offset: Duration| Some((now + offset).to_rfc3339());
        let say = |offset| relative_time(at(offset).as_deref(), now);
        assert_eq!(relative_time(None, now), "Never");
        assert_eq!(say(Duration::seconds(10)), "this minute");
        assert_eq!(say(-Duration::minutes(1)), "1 minute ago");
        assert_eq!(say(-Duration::minutes(59)), "59 minutes ago");
        // `Math.round` takes a half toward positive infinity.
        assert_eq!(say(-Duration::minutes(90)), "1 hour ago");
        assert_eq!(say(-Duration::minutes(150)), "2 hours ago");
        assert_eq!(say(Duration::hours(3)), "in 3 hours");
        assert_eq!(say(-Duration::hours(50)), "2 days ago");
        assert_eq!(say(-Duration::days(40)), "last month");
        assert_eq!(say(-Duration::days(100)), "3 months ago");
        assert_eq!(relative_time(Some("soon"), now), "soon");
    }

    #[test]
    fn date_times_are_medium_and_short() {
        assert_eq!(date_time(None), "None");
        assert_eq!(
            date_time(Some("2026-11-03 09:05:00")),
            "Nov 3, 2026, 9:05 AM"
        );
    }

    #[test]
    fn failures_and_sources_are_worded_for_people() {
        assert_eq!(
            describe_failure(Some("expired"), Some("401")).as_deref(),
            Some("Session expired — log in again in Accounts")
        );
        assert_eq!(
            describe_failure(Some("other"), Some("boom")).as_deref(),
            Some("boom")
        );
        assert_eq!(describe_failure(None, None), None);
        assert_eq!(site_label("furaffinity"), "Fur Affinity");
        assert_eq!(site_label("unknown"), "unknown");
        // The add-query bar starts on the first by name, galleries aside;
        // sources the interface leaves out are not offered.
        let first = query_sites()[0];
        assert_eq!((first.id, first.example_query), ("danbooru", "1girl solo"));
        assert!(
            query_sites()
                .iter()
                .all(|site| !["ehentai", "tumblr"].contains(&site.id))
        );
    }

    #[test]
    fn a_run_aims_for_the_limit_of_each_active_source() {
        let mut subscription = Subscription {
            posts_per_run: 50,
            queries: vec![query(false), query(false), query(false)],
            ..Subscription::default()
        };
        assert_eq!(run_target(&subscription), 150);
        subscription.queries[0].paused = true;
        assert_eq!(run_target(&subscription), 100);
        subscription.queries.clear();
        assert_eq!(run_target(&subscription), 50);
    }

    #[test]
    fn grid_tracks_share_the_room_as_css_does() {
        // The sources table: minmax(90px, .28fr) minmax(0, 1fr) 76px 76px
        // 128px with 10px gaps.
        let columns = [
            Track::Flex(90., 0.28),
            Track::Flex(0., 1.),
            Track::Fixed(76.),
            Track::Fixed(76.),
            Track::Fixed(128.),
        ];
        let wide = tracks(920., 10., &columns);
        // 600px shared .28 : 1.
        assert!((wide[0] - 131.25).abs() < 0.01 && (wide[1] - 468.75).abs() < 0.01);
        // Narrower, the first column holds its minimum.
        let narrow = tracks(560., 10., &columns);
        assert_eq!(narrow[0], 90.);
        assert!((narrow[1] - 150.).abs() < 0.01);
        // A bounded track takes its maximum when there is room.
        let bounded = tracks(
            800.,
            10.,
            &[Track::Flex(100., 1.), Track::Between(82., 110.)],
        );
        assert_eq!(bounded, vec![680., 110.]);
    }

    #[test]
    fn a_source_needs_a_login_only_when_its_site_cannot_run_without_one() {
        let login = |site: &str, health: Option<&str>, error: Option<&str>| Login {
            site: site.into(),
            health: health.map(Into::into),
            last_error: error.map(Into::into),
        };
        // Pixiv cannot run without a login; e621 can.
        assert_eq!(
            auth_warning("pixiv", &[], &[]),
            Some("Sign in required".to_owned())
        );
        assert_eq!(auth_warning("e621", &[], &[]), None);
        // Pixiv (user) logs in through Pixiv.
        let saved = [login("pixiv", None, None)];
        assert_eq!(auth_warning("pixivuser", &saved, &[]), None);
        assert_eq!(
            auth_warning(
                "pixivuser",
                &saved,
                &[login("pixiv", Some("Expired"), Some(" Token expired "))]
            ),
            Some("Token expired".to_owned())
        );
        assert_eq!(
            auth_warning(
                "e621",
                &[login("e621", None, None)],
                &[login("e621", Some("error"), None)]
            ),
            Some("Sign in again".to_owned())
        );
        // A status the rule does not name is no warning.
        assert_eq!(
            auth_warning(
                "newgrounds",
                &[login("newgrounds", None, None)],
                &[login("newgrounds", Some("invalid"), Some("403"))]
            ),
            None
        );
        assert_eq!(auth_warning("unknown", &[], &[]), None);
    }

    #[test]
    fn a_gallery_address_names_its_service() {
        assert_eq!(
            gallery_service(" https://exhentai.org/g/1/2/"),
            Some("exhentai")
        );
        assert_eq!(
            gallery_service("http://www.E-Hentai.org/g/1/2/"),
            Some("ehentai")
        );
        assert_eq!(gallery_service("https://e-hentai.org"), None);
        assert_eq!(gallery_service("e-hentai.org/g/1/2/"), None);
    }

    #[test]
    fn a_source_states_where_it_is() {
        let mut query = Query::default();
        assert_eq!(query_state(&query), "Initial sync");
        query.initial_run_complete = true;
        assert_eq!(query_state(&query), "Ready");
        query.last_failure_message = Some("403".into());
        assert_eq!(query_state(&query), "Failed");
        query.paused = true;
        assert_eq!(query_state(&query), "Paused");
    }
}
