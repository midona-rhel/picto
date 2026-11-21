//! The functions that work out what the Subscriptions screen says about a
//! subscription: its status, its times and its counts. The module also holds
//! the list of sites that the screen offers and the rule that gives the
//! columns of the screen's tables their widths.
use super::data::{Query, Run, Subscription};
use crate::i18n::format::{Unit, relative};
use crate::i18n::{en, t};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use picto_core::subscriptions::sites::{SITES, SiteEntry};

pub use crate::ui::status_badge::Tone;

/// Returns whether the subscription's active run is in progress, which is
/// when the run's status is `pending` or `running`. An active run with
/// another status is paused or has stopped on a failure.
pub fn running(subscription: &Subscription) -> bool {
    subscription.active_run_id.is_some()
        && matches!(
            subscription.run_status.as_deref(),
            Some("pending" | "running")
        )
}

/// Returns whether a source has read every post its site has: the source is
/// not on hold, its first run and its history are complete, it has run with
/// success at least once, and no failure is recorded for it.
pub fn query_completed(query: &Query) -> bool {
    !query.paused
        && query.initial_run_complete
        && query.history_complete
        && query.successful_runs >= 1
        && query.last_success_at.is_some()
        && query.last_failure_kind.is_none()
}

/// Returns whether a subscription is complete: it is not on hold, it has at
/// least one source, and every source is completed (`query_completed`).
pub fn subscription_completed(subscription: &Subscription) -> bool {
    !subscription.paused
        && !subscription.queries.is_empty()
        && subscription.queries.iter().all(query_completed)
}

/// Returns the label of the warning for a failure of the kind `kind`, or
/// `None` for a kind that gives no warning. A failure of the network, a
/// rate limit, a site that is unavailable, a full inbox, a pause and an
/// interrupted run give no warning, because the next run can succeed
/// without the user doing anything. A problem with the login gives "Login
/// required", and every other kind gives "Failed".
fn lasting_failure(kind: &str) -> Option<&'static str> {
    match kind {
        "network" | "rate_limited" | "source_unavailable" | "interrupted" | "stale"
        | "inbox_full" | "paused" => None,
        "auth" | "access" | "unauthorized" | "expired" => Some(t("Login required")),
        _ => Some(t("Failed")),
    }
}

/// Returns the source whose last failure gives a warning
/// (`lasting_failure`). Sources on hold are left out. When several sources
/// have such a failure, the one that failed last is returned.
pub fn failing(subscription: &Subscription) -> Option<&Query> {
    subscription
        .queries
        .iter()
        .filter(|query| !query.paused)
        .filter(|query| {
            query
                .last_failure_kind
                .as_deref()
                .is_some_and(|kind| lasting_failure(kind).is_some())
        })
        .max_by(|left, right| left.last_failure_at.cmp(&right.last_failure_at))
}

/// Returns the tone and the label of a subscription's status badge, which
/// is on its tile and in the header of its page. The badge warns
/// (`Tone::Attention`) in two cases only: a run has stopped on a failure
/// and waits for Retry, or the last failure of a source gives a warning
/// (`lasting_failure`). The label then names the reason. Open problems
/// with single files do not make the badge warn.
pub fn status(subscription: &Subscription) -> (Tone, &'static str) {
    let active = subscription.active_run_id.is_some();
    let running = running(subscription);
    // The status of an active run that is not in progress. It says why the
    // run stopped.
    let stopped = (active && !running)
        .then_some(subscription.run_status.as_deref())
        .flatten();
    if stopped == Some("inbox_full") {
        (Tone::Paused, t("Inbox full"))
    } else if subscription.paused || stopped == Some("paused") {
        (Tone::Paused, t("Paused"))
    } else if running {
        (Tone::Running, t("Syncing"))
    } else if let Some(kind) = stopped {
        // A run that stopped on a failure waits for Retry, whatever the
        // kind of the failure, so the badge warns for every kind.
        (
            Tone::Attention,
            lasting_failure(kind).unwrap_or(t("Failed")),
        )
    } else if let Some(label) = failing(subscription)
        .and_then(|query| query.last_failure_kind.as_deref())
        .and_then(lasting_failure)
    {
        (Tone::Attention, label)
    } else if active {
        (Tone::Running, t("Syncing"))
    } else if subscription_completed(subscription) {
        (Tone::Success, t("Complete"))
    } else {
        (Tone::Idle, t("Idle"))
    }
}

/// Returns the tone and the label of the status badge of a run in the
/// History tab.
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

/// Returns the label of the status badge of a problem in the Problems tab.
/// It is the problem's kind with spaces in place of underscores, except
/// that the kind `download_item` is shown as "download".
pub fn issue_label(kind: &str) -> String {
    if kind == "download_item" {
        // i18n: a badge's label, shown in English in every language
        "download".to_owned()
    } else {
        kind.replace('_', " ")
    }
}

/// Returns the text that tells the user what to do about a source's
/// recorded failure of the kind `kind`. For a kind that has no text of its
/// own, it returns the failure's `message`.
pub fn describe_failure(kind: Option<&str>, message: Option<&str>) -> Option<String> {
    let text = match kind {
        Some("not_found" | "invalid_query") => t("User/query not found — check the handle"),
        Some("unauthorized" | "auth" | "access") => {
            t("Login rejected — check the account in Accounts")
        }
        Some("expired") => t("Session expired — log in again in Accounts"),
        Some("rate_limited") => {
            t("Rate limited by the site — resumes automatically when the limit resets")
        }
        Some("network") => t("Network error — check your connection"),
        Some("environment") => t("Local setup problem — check the logs"),
        Some("stale" | "interrupted") => t("Interrupted by app shutdown"),
        Some("inbox_full") => t("Paused — inbox is full"),
        _ => return message.map(str::to_owned),
    };
    Some(text.to_owned())
}

/// Returns the short text that says why a gallery download failed. The
/// text is chosen by words in the failure's `message`, and by the failure's
/// `kind` when the message has none of those words.
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

/// Returns whether the subscription is a gallery download: a subscription
/// with exactly one source, whose site is E-Hentai. The screen lists a
/// gallery download in the bar above the tiles and does not show it as a
/// tile.
pub fn is_gallery_job(subscription: &Subscription) -> bool {
    subscription.queries.len() == 1 && subscription.queries[0].site_id == "ehentai"
}

/// Returns the sites that the screen offers: the application's catalog of
/// sites (`picto_core::subscriptions::sites::SITES`) without the sites in
/// `REMOVED`.
pub fn sites() -> impl Iterator<Item = &'static SiteEntry> {
    /// The ids of the sites that the screen does not offer.
    const REMOVED: [&str; 1] = ["tumblr"];
    SITES.iter().filter(|site| !REMOVED.contains(&site.id))
}

/// Returns the catalog's entry for the site with the id `site_id`, or
/// `None` when the screen does not offer that site.
pub fn site(site_id: &str) -> Option<&'static SiteEntry> {
    sites().find(|site| site.id == site_id)
}

/// Returns the name of the site with the id `site_id`, or the id itself
/// when the screen does not offer that site.
pub fn site_label(site_id: &str) -> String {
    site(site_id).map_or_else(|| site_id.to_owned(), |site| site.name.to_owned())
}

/// Returns the sites sorted by name. The "Edit query" dialog lists them in
/// this order.
pub fn sites_by_name() -> Vec<&'static SiteEntry> {
    let mut sites = sites().collect::<Vec<_>>();
    sites.sort_by_key(|site| site.name.to_lowercase());
    sites
}

/// Returns the sites that the row that adds a source offers, sorted by
/// name. E-Hentai is left out, because a gallery is added with the "Add
/// gallery" dialog.
pub fn query_sites() -> Vec<&'static SiteEntry> {
    sites_by_name()
        .into_iter()
        .filter(|site| site.id != "ehentai")
        .collect()
}

/// Returns the number of posts that a run of the subscription is expected
/// to process: the "Posts per run" limit multiplied by the number of
/// sources that are not on hold. A limit that is not set counts as 100,
/// and the number of sources counts as at least 1.
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

/// Returns the time of the latest success or failure of any of the
/// subscription's sources. The rail shows it as "Last check".
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

/// Returns the time of the source's latest success or failure.
pub fn query_last_check(query: &Query) -> Option<&str> {
    [
        query.last_success_at.as_deref(),
        query.last_failure_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// Returns the text of the State row in a source's details: "Paused" for a
/// source on hold, "Failed" for one with a recorded failure, "Ready" for
/// one whose first run is complete, and "Initial sync" for any other.
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
    /// The status of the last check of the login, or `None` when the login
    /// has not been checked.
    pub health: Option<String>,
    pub last_error: Option<String>,
}

/// Returns the text of the warning that a source's row shows when its site
/// needs a login before the source can run, or `None` when no warning is
/// needed. A warning is needed when the last check of the site's login
/// failed, or when the site cannot run without a login and none is saved.
/// The text is "Sign in required" when no login is saved. Otherwise it is
/// the error of the last check, or "Sign in again" when that check has no
/// error text.
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

/// Returns the gallery service that the address `url` belongs to,
/// `exhentai` or `ehentai`, or `None` for any other address. The "Add
/// gallery" dialog sets its Service control from the address that is typed.
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

/// Returns the text that a tile shows after its status badge: the number of
/// items, followed by the number of sources when there is at least one.
pub fn card_meta(items: i64, sources: usize) -> String {
    // The word "item" and the plural ending "s" are translated separately.
    let mut meta = format!(
        "{} {}{}",
        crate::i18n::format::grouped(items),
        t("item"),
        if items == 1 { "" } else { t("s") }
    );
    if sources > 0 {
        meta.push_str(&format!(
            " · {sources} source{}",
            // i18n: "source" is shown in English in every language
            if sources == 1 { "" } else { "s" }
        ));
    }
    meta
}

/// Reads a stored time. It accepts a time in RFC 3339 form, and a date and
/// time without a time zone, which it takes as local time.
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

/// Rounds to the nearest whole number. A value exactly halfway between two
/// whole numbers is rounded up, so -1.5 gives -1.
fn round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// Returns the time `value` relative to `now` in the system's language, for
/// example "2 hours ago" (`i18n::format::relative`). The time is given in
/// minutes when it is less than 60 minutes away, in hours when it is less
/// than 48 hours away, in days when it is less than 30 days away, and in
/// months otherwise. Returns "Never" when there is no time, and `value`
/// itself when it cannot be read as a time.
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

/// Returns the time `value` relative to `now` as [`relative_time`] does,
/// except that a time 48 hours or more away is always given in days and
/// never in months. The "Site accounts" dialog uses it.
pub fn relative_time_in_days(value: Option<&str>, now: DateTime<Local>) -> String {
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
    relative(round(hours as f64 / 24.), Unit::Day)
}

/// Returns the time `value` as a date and a time of day, for example
/// "Nov 3, 2026, 9:05 AM" (`i18n::format::medium_date_time`). Returns
/// "None" when there is no time, and `value` itself when it cannot be read
/// as a time.
pub fn date_time(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return t("None").to_owned();
    };
    parse_time(value).map_or_else(
        || value.to_owned(),
        |time| crate::i18n::format::medium_date_time(time.naive_local()),
    )
}

/// Sorts the subscriptions into the order their tiles are shown in: by name
/// (`text::natural_order`), and subscriptions with the same name by the
/// text `sub:<id>`.
pub fn card_order(subscriptions: &mut [&Subscription]) {
    subscriptions.sort_by(|left, right| {
        crate::text::natural_order(&left.name, &right.name)
            .then_with(|| format!("sub:{}", left.id).cmp(&format!("sub:{}", right.id)))
    });
}

/// The rule for the width of one column of a table.
#[derive(Clone, Copy, Debug)]
pub enum Track {
    Fixed(f32),
    /// A column that is at least the first number of pixels wide and gets
    /// a share of the free width in proportion to the second number.
    Flex(f32, f32),
}

/// Returns the width of each of `tracks` when they share `width` with `gap`
/// between them. A fixed track gets its size. The width that is left is
/// shared among the flexible tracks in proportion to their fractions, and
/// a flexible track is never narrower than its minimum.
pub fn tracks(width: f32, gap: f32, tracks: &[Track]) -> Vec<f32> {
    let gaps = gap * tracks.len().saturating_sub(1) as f32;
    let mut sizes: Vec<f32> = tracks
        .iter()
        .map(|track| match *track {
            Track::Fixed(size) | Track::Flex(size, _) => size,
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
        Login, Query, Run, Subscription, Tone, Track, auth_warning, card_meta, date_time,
        describe_failure, gallery_service, query_sites, query_state, relative_time, run_status,
        run_target, site_label, status, tracks,
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
    fn a_subscription_warns_only_when_it_will_stay_broken() {
        let mut subscription = Subscription {
            queries: vec![query(true)],
            ..Subscription::default()
        };
        assert_eq!(status(&subscription), (Tone::Success, "Complete"));
        // Open problems with files that could not be downloaded do not make
        // the badge warn.
        subscription.open_issues = 2;
        assert_eq!(status(&subscription), (Tone::Success, "Complete"));
        // A failure that the next run can get past gives no warning. Any
        // other failure gives a warning that names it.
        for (kind, expected) in [
            ("network", (Tone::Idle, "Idle")),
            ("inbox_full", (Tone::Idle, "Idle")),
            ("auth", (Tone::Attention, "Login required")),
            ("invalid_output", (Tone::Attention, "Failed")),
        ] {
            subscription.queries[0].last_failure_kind = Some(kind.into());
            assert_eq!(status(&subscription), expected, "{kind}");
        }
        subscription.active_run_id = Some(4);
        subscription.run_status = Some("running".into());
        assert_eq!(status(&subscription), (Tone::Running, "Syncing"));
        // A run that stopped gives the warning "Failed", unless it stopped
        // on a pause or a full inbox.
        subscription.run_status = Some("network".into());
        assert_eq!(status(&subscription), (Tone::Attention, "Failed"));
        subscription.run_status = Some("paused".into());
        assert_eq!(status(&subscription), (Tone::Paused, "Paused"));
        subscription.run_status = Some("inbox_full".into());
        assert_eq!(status(&subscription), (Tone::Paused, "Inbox full"));
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
        // A half is rounded toward positive infinity, so -1.5 hours is
        // "1 hour ago".
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
        // The row that adds a source starts on the first site by name. It
        // offers neither E-Hentai nor the sites that the screen leaves out.
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
        // Five columns with 10px gaps: one of at least 90px with the
        // fraction 0.28, one without a minimum with the fraction 1, and
        // fixed columns of 76px, 76px and 128px.
        let columns = [
            Track::Flex(90., 0.28),
            Track::Flex(0., 1.),
            Track::Fixed(76.),
            Track::Fixed(76.),
            Track::Fixed(128.),
        ];
        let wide = tracks(920., 10., &columns);
        // The 600px that are left are shared in the proportion 0.28 to 1.
        assert!((wide[0] - 131.25).abs() < 0.01 && (wide[1] - 468.75).abs() < 0.01);
        // In a narrower table the first column keeps its minimum of 90px.
        let narrow = tracks(560., 10., &columns);
        assert_eq!(narrow[0], 90.);
        assert!((narrow[1] - 150.).abs() < 0.01);
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
        // A status that is not one of the four failed statuses gives no
        // warning.
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
