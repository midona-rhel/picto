//! What the Subscriptions screen says about a subscription: its status, its
//! times and counts, as `subscriptionUtils.ts` and the components word them.
use super::data::{Query, Run, Subscription};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

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
        (Tone::Paused, "Inbox full")
    } else if subscription.paused || (active && !running) {
        (Tone::Paused, "Paused")
    } else if running {
        (Tone::Running, "Syncing")
    } else if subscription.open_issues > 0 {
        (Tone::Attention, "Warning")
    } else if !active && subscription_completed(subscription) {
        (Tone::Success, "Complete")
    } else {
        (Tone::Idle, "Idle")
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
        "Warning"
    } else if waiting_for_inbox {
        "Inbox full"
    } else {
        match tone {
            Tone::Running => "Syncing",
            Tone::Paused => "Paused",
            Tone::Attention => "Warning",
            Tone::Success => "Complete",
            Tone::Idle => "Idle",
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
        "completed" | "succeeded" => "Complete".to_owned(),
        _ if waiting => "Inbox full".to_owned(),
        _ if paused => "Paused".to_owned(),
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
        "download".to_owned()
    } else {
        kind.replace('_', " ")
    }
}

/// `describeFailure`: what to do about a query's recorded failure.
pub fn describe_failure(kind: Option<&str>, message: Option<&str>) -> Option<String> {
    let text = match kind {
        Some("not_found") => "User/query not found — check the handle",
        Some("unauthorized") => "Login rejected — check the account in Accounts",
        Some("expired") => "Session expired — log in again in Accounts",
        Some("rate_limited") => {
            "Rate limited by the site — resumes automatically when the limit resets"
        }
        Some("network") => "Network error — check your connection",
        Some("environment") => "Local setup problem — check the logs",
        Some("stale") => "Interrupted by app shutdown",
        Some("inbox_full") => "Paused — inbox is full",
        _ => return message.map(str::to_owned),
    };
    Some(text.to_owned())
}

/// `describeGalleryFailure`
pub fn describe_gallery_failure(kind: Option<&str>, message: Option<&str>) -> &'static str {
    let message = message.unwrap_or_default().to_lowercase();
    if message.contains("requires gp") {
        "Original requires GP"
    } else if message.contains("image viewing limit") {
        "Image limit reached"
    } else if message.contains("temporarily blocked") {
        "Temporarily rate limited"
    } else if message.contains("web page instead of a file") {
        "Invalid media response"
    } else {
        match kind {
            Some("unauthorized" | "expired") => "Login required",
            Some("rate_limited") => "Rate limited",
            _ => "Download failed",
        }
    }
}

/// A gallery download: a subscription of one E-Hentai query, which the
/// screen shows in its header rather than as a card.
pub fn is_gallery_job(subscription: &Subscription) -> bool {
    subscription.queries.len() == 1 && subscription.queries[0].site_id == "ehentai"
}

/// The sources the application offers, by id: their display names and
/// example queries. These mirror the source catalog (`sources.list`), which
/// lives with the source runners rather than in the library.
const SITES: &[(&str, &str, &str)] = &[
    ("pixiv", "Pixiv", "風景"),
    ("pixivuser", "Pixiv (user)", "12345"),
    ("gelbooru", "Gelbooru", "1girl solo"),
    ("rule34", "Rule34.xxx", "1girl solo"),
    ("danbooru", "Danbooru", "1girl solo"),
    ("hentaifoundry", "Hentai Foundry", "username"),
    ("deviantart", "DeviantArt", "username"),
    ("twitter", "Twitter / X", "username"),
    ("newgrounds", "Newgrounds", "username"),
    ("furaffinity", "Fur Affinity", "username"),
    ("patreon", "Patreon", "creator-slug"),
    ("fanbox", "pixivFANBOX", "creator-slug"),
    (
        "subscribestar",
        "SubscribeStar",
        "https://www.subscribestar.com/creator-slug",
    ),
    ("onlyfans", "OnlyFans", "creator-name"),
    ("yandere", "Yande.re", "landscape"),
    ("safebooru", "Safebooru", "1girl solo"),
    ("e621", "e621", "canine solo"),
    (
        "ehentai",
        "E-Hentai",
        "https://e-hentai.org/g/12345/67890abcde/",
    ),
    (
        "pawchive",
        "Pawchive",
        "https://pawchive.pw/patreon/user/12345",
    ),
];

/// `getSiteLabel`: the source's name, or its id when it is not offered.
pub fn site_label(site_id: &str) -> String {
    SITES
        .iter()
        .find(|(id, _, _)| *id == site_id)
        .map_or_else(|| site_id.to_owned(), |(_, name, _)| (*name).to_owned())
}

/// The source the add-query bar starts on: the first by name, galleries
/// aside, and its example query.
pub fn first_query_site() -> (&'static str, &'static str) {
    SITES
        .iter()
        .filter(|(id, _, _)| *id != "ehentai")
        .min_by_key(|(_, name, _)| name.to_lowercase())
        .map(|(_, name, example)| (*name, *example))
        .unwrap_or(("", ""))
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

/// The query whose failure is the latest, for the warning's explanation.
pub fn latest_failure(subscription: &Subscription) -> Option<&Query> {
    subscription
        .queries
        .iter()
        .filter(|query| query.last_failure_message.is_some())
        .max_by(|left, right| left.last_failure_at.cmp(&right.last_failure_at))
}

/// `toLocaleString()` for a count: digits grouped by thousands.
pub fn count(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// `n thing` or `n things`, with the count grouped.
pub fn counted(value: i64, singular: &str) -> String {
    format!(
        "{} {singular}{}",
        count(value),
        if value == 1 { "" } else { "s" }
    )
}

/// What a card says under its name: items, then sources when there are any.
pub fn card_meta(items: i64, sources: usize) -> String {
    let mut meta = counted(items, "item");
    if sources > 0 {
        meta.push_str(&format!(
            " · {sources} source{}",
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

/// `Intl.RelativeTimeFormat` in English with `numeric: 'auto'`.
fn relative(amount: i64, unit: &str) -> String {
    match (amount, unit) {
        (0, "minute") => "this minute".to_owned(),
        (1, "day") => "tomorrow".to_owned(),
        (-1, "day") => "yesterday".to_owned(),
        (1, "month") => "next month".to_owned(),
        (-1, "month") => "last month".to_owned(),
        _ => {
            let plural = if amount.abs() == 1 { "" } else { "s" };
            if amount < 0 {
                format!("{} {unit}{plural} ago", -amount)
            } else {
                format!("in {amount} {unit}{plural}")
            }
        }
    }
}

/// `formatRelativeTime`, relative to `now`.
pub fn relative_time(value: Option<&str>, now: DateTime<Local>) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return "Never".to_owned();
    };
    let Some(time) = parse_time(value) else {
        return value.to_owned();
    };
    let minutes = round((time - now).num_milliseconds() as f64 / 60_000.);
    if minutes.abs() < 60 {
        return relative(minutes, "minute");
    }
    let hours = round(minutes as f64 / 60.);
    if hours.abs() < 48 {
        return relative(hours, "hour");
    }
    let days = round(hours as f64 / 24.);
    if days.abs() < 30 {
        return relative(days, "day");
    }
    relative(round(days as f64 / 30.), "month")
}

/// `formatDateTime`: the medium date and short time, as en-US writes them.
pub fn date_time(value: Option<&str>) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return "None".to_owned();
    };
    parse_time(value).map_or_else(
        || value.to_owned(),
        |time| time.format("%b %-d, %Y, %-I:%M %p").to_string(),
    )
}

/// `localeCompare(…, { sensitivity: 'base', numeric: true })`, closely
/// enough for names: letters compare without case, and runs of digits by
/// their value.
pub fn name_order(left: &str, right: &str) -> std::cmp::Ordering {
    fn chunks(text: &str) -> Vec<(bool, String)> {
        let mut chunks: Vec<(bool, String)> = Vec::new();
        for character in text.chars() {
            let digit = character.is_ascii_digit();
            match chunks.last_mut() {
                Some((was_digit, chunk)) if *was_digit == digit => chunk.push(character),
                _ => chunks.push((digit, character.to_string())),
            }
        }
        chunks
    }
    let (left, right) = (chunks(left), chunks(right));
    for ((left_digit, left), (right_digit, right)) in left.iter().zip(&right) {
        let order = if *left_digit && *right_digit {
            let (a, b) = (left.trim_start_matches('0'), right.trim_start_matches('0'));
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        } else {
            left.to_lowercase().cmp(&right.to_lowercase())
        };
        if order.is_ne() {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// The grid's order: by name, then by the card's key.
pub fn card_order(subscriptions: &mut [&Subscription]) {
    subscriptions.sort_by(|left, right| {
        name_order(&left.name, &right.name)
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
        Query, Run, Subscription, Tone, Track, card_meta, card_status, count, date_time,
        describe_failure, detail_status, first_query_site, name_order, relative_time, run_status,
        run_target, site_label, tracks,
    };
    use chrono::{Duration, Local, TimeZone};
    use std::cmp::Ordering;

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
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1234567), "1,234,567");
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
        assert_eq!(first_query_site(), ("Danbooru", "1girl solo"));
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
    fn names_sort_without_case_and_by_number() {
        assert_eq!(name_order("apple", "Banana"), Ordering::Less);
        assert_eq!(name_order("Artist 9", "artist 10"), Ordering::Less);
        assert_eq!(name_order("artist 010", "Artist 10"), Ordering::Equal);
        assert_eq!(name_order("art", "artist"), Ordering::Less);
    }
}
