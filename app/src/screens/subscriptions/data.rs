//! Reads what the Subscriptions screen shows from the library. The screen
//! runs these functions on the library's thread (`Session::run`).
//!
//! The library keeps subscriptions in tables of its database, and keeps their
//! lifetime counts in the projection (`subscription_counts`). The functions
//! here read what the application commands `subscriptions.list`,
//! `subscriptions.runs.list` and `subscriptions.issues.list` read.
//! [`subscriptions`] runs the SQL files that `subscriptions.list` runs, and
//! reads the tables and the projection together with
//! `Library::auxiliary_read_consistent`.
use picto_library::database::WorkPriority;
use picto_library::{Library, ProjectionSnapshot, Result};
use rusqlite::{Transaction, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One subscription as the list shows it.
#[derive(Clone, Debug, Default)]
pub struct Subscription {
    pub id: i64,
    pub name: String,
    pub schedule: String,
    /// Whether the subscription is paused. A paused subscription starts no
    /// new runs.
    pub paused: bool,
    /// The id of the subscription's run that is pending or running, when
    /// there is one.
    pub active_run_id: Option<i64>,
    /// The status of the active run, or of the latest run when no run is
    /// active. For an active run that is pending and has a failure kind, it
    /// is that failure kind.
    pub run_status: Option<String>,
    /// The number of posts that the subscription has added to the library
    /// since it was created.
    pub items: i64,
    pub open_issues: i64,
    pub posts_per_run: i64,
    /// The counts of the active run, or of the latest run when no run is
    /// active.
    pub progress: Progress,
    pub folder_ids: Vec<i64>,
    pub automatic_tags: Vec<String>,
    pub cover: Option<Cover>,
    pub queries: Vec<Query>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub posts_traversed: i64,
    pub posts_added: i64,
    pub posts_skipped: i64,
    pub downloaded: i64,
    pub files_already_in_library: i64,
}

/// The image that a subscription shows as its cover: a thumbnail file, and
/// the square part of it that is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub thumbnail: PathBuf,
    /// The width and height in pixels of the cover's source image, when the
    /// library has them. It is `None` for a pre-cropped rendering.
    pub size: Option<(u32, u32)>,
    pub crop: crate::cover::Crop,
}

/// One source that a subscription follows.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub id: i64,
    pub site_id: String,
    pub text: String,
    /// The name that the user gave the source. When it is set, it is shown
    /// instead of the query text.
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub group_posts: bool,
    pub paused: bool,
    pub initial_run_complete: bool,
    pub history_complete: bool,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
    pub posts: i64,
    pub media: i64,
    pub successful_runs: i64,
}

/// One row of the History tab.
#[derive(Clone, Debug)]
pub struct Run {
    pub id: i64,
    pub status: String,
    pub started_at: String,
    pub failure_kind: Option<String>,
    pub error_message: Option<String>,
    pub downloaded: i64,
    pub already_in_library: i64,
}

/// One row of the Problems tab.
#[derive(Clone, Debug)]
pub struct Issue {
    pub id: i64,
    pub kind: String,
    pub message: String,
    /// More text about the failure. The row's tooltip shows it with the
    /// message.
    pub detail: Option<String>,
    pub last_seen_at: String,
    pub target: Option<String>,
    pub url: Option<String>,
    /// The id of the source that the problem is about, when it is about a
    /// single source.
    pub query: Option<i64>,
}

/// A page of open issues, newest first.
#[derive(Clone, Debug, Default)]
pub struct Issues {
    pub items: Vec<Issue>,
    pub total: i64,
    /// The `last_seen_at` and the id of the page's last row, when more issues
    /// follow it. [`more_issues`] takes it to read the next page.
    pub next: Option<(String, i64)>,
}

/// What the History and Problems tabs of one subscription show: its latest
/// runs and the first page of its open issues.
#[derive(Clone, Debug)]
pub struct Detail {
    pub subscription: i64,
    pub runs: Vec<Run>,
    pub issues: Issues,
}

/// The number of runs that [`detail`] reads for the History tab, latest
/// first.
const RUN_LIMIT: i64 = 20;
/// The number of open issues in one page of the Problems tab.
const ISSUE_PAGE: i64 = 50;

/// Returns every subscription, ordered by name. `blobs` is the library's blob
/// folder, in which the cover thumbnails are looked up.
pub fn subscriptions(library: &Library, blobs: &Path) -> Result<Vec<Subscription>> {
    library.auxiliary_read_consistent(WorkPriority::VisibleRead, |connection, projection| {
        let mut queries = queries(connection, projection)?;
        let mut settings = settings(connection, projection, blobs)?;
        let counts = &projection.subscription_counts;
        let mut statement = connection.prepare(include_str!(
            "../../../../core/src/subscription_views.sql"
        ))?;
        let rows = statement.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let initial: Option<i64> = row.get(4)?;
            let periodic: Option<i64> = row.get(5)?;
            Ok(Subscription {
                id,
                name: row.get(1)?,
                schedule: row.get(2)?,
                paused: row.get(3)?,
                active_run_id: row.get(7)?,
                run_status: row.get(8)?,
                items: counts.posts(id),
                open_issues: row.get(10)?,
                posts_per_run: periodic.or(initial).unwrap_or(100),
                progress: Progress {
                    posts_traversed: row.get(11)?,
                    posts_added: row.get(12)?,
                    posts_skipped: row.get(13)?,
                    downloaded: row.get(15)?,
                    files_already_in_library: row.get(19)?,
                },
                ..Subscription::default()
            })
        })?;
        let mut subscriptions = Vec::new();
        for row in rows {
            let mut subscription = row?;
            subscription.queries = queries.remove(&subscription.id).unwrap_or_default();
            if let Some(setting) = settings.remove(&subscription.id) {
                subscription.folder_ids = setting.folder_ids;
                subscription.automatic_tags = setting.automatic_tags;
                subscription.cover = setting.cover;
            }
            subscriptions.push(subscription);
        }
        Ok(subscriptions)
    })
}

fn queries(
    connection: &Transaction<'_>,
    projection: &ProjectionSnapshot,
) -> Result<HashMap<i64, Vec<Query>>> {
    let counts = &projection.subscription_counts;
    let mut statement = connection.prepare(include_str!(
        "../../../../core/src/subscription_query_views.sql"
    ))?;
    let rows = statement.query_map([], |row| {
        let id: i64 = row.get(1)?;
        let totals = counts.query(id);
        Ok((
            row.get::<_, i64>(0)?,
            Query {
                id,
                site_id: row.get(2)?,
                text: row.get(4)?,
                display_name: row.get(5)?,
                notes: row.get(6)?,
                group_posts: row.get(7)?,
                paused: row.get(8)?,
                initial_run_complete: row.get(9)?,
                history_complete: row.get(10)?,
                last_success_at: row.get(11)?,
                last_failure_at: row.get(12)?,
                last_failure_kind: row.get(13)?,
                last_failure_message: row.get(14)?,
                posts: totals.posts,
                media: totals.media,
                successful_runs: totals.successful_runs,
            },
        ))
    })?;
    let mut by_subscription: HashMap<i64, Vec<Query>> = HashMap::new();
    for row in rows {
        let (subscription, query) = row?;
        by_subscription.entry(subscription).or_default().push(query);
    }
    Ok(by_subscription)
}

#[derive(Default)]
struct Setting {
    folder_ids: Vec<i64>,
    automatic_tags: Vec<String>,
    cover: Option<Cover>,
}

/// Returns, by subscription id, the destination (folders and automatic tags)
/// and the cover that each subscription stores in the `setting` table.
fn settings(
    connection: &Transaction<'_>,
    projection: &ProjectionSnapshot,
    blobs: &Path,
) -> Result<HashMap<i64, Setting>> {
    // The cover's source file is the file of the media item that the stored
    // cover names. It is found only while a post of the subscription links
    // that media item through a source item whose state is `ingested`.
    let mut statement = connection.prepare(
        "SELECT s.subscription_id, destination.value_json, cover.value_json,
                file.content_hash, file.width, file.height
         FROM subscription s
         LEFT JOIN setting destination
           ON destination.key = 'subscription.' || s.subscription_id || '.destination'
         LEFT JOIN setting cover
           ON cover.key = 'subscription.' || s.subscription_id || '.cover'
         LEFT JOIN media_file file ON file.content_hash = (
             SELECT linked_file.content_hash
             FROM subscription_source_post ssp
             JOIN source_item si ON si.source_post_id = ssp.source_post_id
             JOIN media_item media ON media.media_id = si.media_item_id
             JOIN media_file linked_file ON linked_file.file_id = media.file_id
             WHERE ssp.subscription_id = s.subscription_id
               AND si.media_item_id = CASE
                   WHEN json_valid(cover.value_json)
                   THEN CAST(json_extract(cover.value_json, '$.media_item_id') AS INTEGER)
               END
               AND si.state = 'ingested'
             LIMIT 1
         )
         ORDER BY s.subscription_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })?;
    let mut settings = HashMap::new();
    for row in rows {
        let (id, destination, cover, source, width, height) = row?;
        let mut setting = Setting::default();
        if let Some(destination) = destination.and_then(|json| parse(&json)) {
            setting.folder_ids = destination
                .get("target_folder_ids")
                .and_then(serde_json::Value::as_array)
                .map(|ids| ids.iter().filter_map(serde_json::Value::as_i64).collect())
                .unwrap_or_default();
            if let Some(id) = destination
                .get("target_folder_id")
                .and_then(serde_json::Value::as_i64)
                && !setting.folder_ids.contains(&id)
            {
                setting.folder_ids.push(id);
            }
            setting.automatic_tags = destination
                .get("automatic_tags")
                .and_then(serde_json::Value::as_array)
                .map(|tags| {
                    tags.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|tag| !tag.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
        }
        if let (Some(cover), Some(source)) = (cover.and_then(|json| parse(&json)), source) {
            setting.cover = cover_of(&cover, &source, width, height, projection, blobs);
        }
        settings.insert(id, setting);
    }
    Ok(settings)
}

fn parse(json: &str) -> Option<serde_json::Value> {
    serde_json::from_str(json).ok()
}

/// Returns the cover that `stored` describes. When `stored` names a
/// pre-cropped rendering (`rendered_hash`) whose thumbnail file exists, the
/// cover is that file with the default crop. Otherwise the cover is the
/// thumbnail of `source`, the hash of the source file, with the stored crop.
/// Returns `None` when the library item that owns the cover's media item is
/// not in `projection.active()`, or when the thumbnail file does not exist.
fn cover_of(
    stored: &serde_json::Value,
    source: &str,
    width: Option<i64>,
    height: Option<i64>,
    projection: &ProjectionSnapshot,
    blobs: &Path,
) -> Option<Cover> {
    let number = |key: &str| stored.get(key).and_then(serde_json::Value::as_i64);
    let media = u32::try_from(number("media_item_id")?).ok()?;
    let root = projection.media_owner.get(media)?;
    if !projection.active().contains(root.0) {
        return None;
    }
    let rendered = stored
        .get("rendered_hash")
        .and_then(serde_json::Value::as_str)
        .and_then(|hash| thumbnail(blobs, hash));
    if let Some(thumbnail) = rendered {
        return Some(Cover {
            thumbnail,
            size: None,
            crop: crate::cover::Crop::default(),
        });
    }
    Some(Cover {
        thumbnail: thumbnail(blobs, source)?,
        size: width.zip(height).and_then(|(width, height)| {
            Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
        }),
        crop: crate::cover::Crop {
            focus_x: number("focus_x").unwrap_or(500) as f32,
            focus_y: number("focus_y").unwrap_or(500) as f32,
            zoom: number("zoom_percent").unwrap_or(100) as f32,
        },
    })
}

/// Returns the path of the thumbnail file of `hash` in the blob folder. The
/// file is `t/<first two characters of the hash>/<next two>/<hash>.jpg`. When
/// that file does not exist, it is the file of the same name ending in
/// `.png`, which is the older thumbnail format. Returns `None` when neither
/// file exists, or when `hash` is shorter than four characters or is not
/// hexadecimal.
pub fn thumbnail(blobs: &Path, hash: &str) -> Option<PathBuf> {
    if hash.len() < 4 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let directory = blobs.join("t").join(&hash[..2]).join(&hash[2..4]);
    ["jpg", "png"]
        .into_iter()
        .map(|extension| directory.join(format!("{hash}.{extension}")))
        .find(|path| path.is_file())
}

/// Returns the latest runs and the first page of open issues of one
/// subscription.
pub fn detail(library: &Library, subscription: i64) -> Result<Detail> {
    library.auxiliary_read(WorkPriority::VisibleRead, |connection| {
        Ok(Detail {
            subscription,
            runs: runs(connection, subscription)?,
            issues: issues(connection, subscription, None)?,
        })
    })
}

/// Returns the page of open issues that follows the row `after`, which is the
/// `next` of the page before.
pub fn more_issues(library: &Library, subscription: i64, after: (String, i64)) -> Result<Issues> {
    library.auxiliary_read(WorkPriority::VisibleRead, |connection| {
        issues(connection, subscription, Some(after))
    })
}

fn runs(connection: &rusqlite::Connection, subscription: i64) -> Result<Vec<Run>> {
    let mut statement = connection.prepare(
        "WITH target_runs AS (
             SELECT * FROM subscription_run
             WHERE subscription_id = ?1
             ORDER BY created_at DESC, run_id DESC
             LIMIT ?2
         ),
         counts AS (
             SELECT srq.run_id,
                    COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained')
                                        THEN file.file_attempt_id END) AS downloaded
             FROM target_runs tr
             JOIN subscription_run_query srq ON srq.run_id = tr.run_id
             LEFT JOIN source_post_attempt attempt ON attempt.run_query_id = srq.run_query_id
             LEFT JOIN source_file_attempt file USING(attempt_id)
             GROUP BY srq.run_id
         )
         SELECT sr.run_id, sr.status, COALESCE(sr.started_at, sr.created_at),
                sr.failure_kind, sr.error_message, COALESCE(c.downloaded, 0),
                (SELECT COUNT(*)
                 FROM subscription_run_query query
                 JOIN source_post_attempt duplicate USING(run_query_id)
                 JOIN source_item existing USING(source_post_id)
                 WHERE query.run_id = sr.run_id
                   AND ((duplicate.state = 'skipped'
                         AND duplicate.terminal_reason = 'exact_duplicate'
                         AND existing.media_item_id IS NOT NULL)
                        OR EXISTS(SELECT 1 FROM source_file_attempt file
                                  WHERE file.attempt_id = duplicate.attempt_id
                                    AND file.source_item_id = existing.source_item_id
                                    AND file.state = 'duplicate')))
         FROM target_runs sr
         LEFT JOIN counts c ON c.run_id = sr.run_id
         ORDER BY sr.created_at DESC, sr.run_id DESC",
    )?;
    let rows = statement.query_map(params![subscription, RUN_LIMIT], |row| {
        Ok(Run {
            id: row.get(0)?,
            status: row.get(1)?,
            started_at: row.get(2)?,
            failure_kind: row.get(3)?,
            error_message: row.get(4)?,
            downloaded: row.get(5)?,
            already_in_library: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn issues(
    connection: &rusqlite::Connection,
    subscription: i64,
    after: Option<(String, i64)>,
) -> Result<Issues> {
    let total = connection.query_row(
        "SELECT COUNT(*) FROM subscription_issue
         WHERE subscription_id = ?1 AND status != 'resolved'",
        [subscription],
        |row| row.get(0),
    )?;
    let (after_time, after_id) = after.unzip();
    // The key of a `download_item` issue contains the id of its source item.
    // The post of that source item gives the row its title and its URL.
    let mut statement = connection.prepare(
        "SELECT issue.issue_id, issue.issue_kind, issue.message,
                issue.last_seen_at, item.item_key, post.post_key, post.title,
                COALESCE(post.canonical_url, item.canonical_url), issue.detail,
                issue.query_id
         FROM subscription_issue issue
         LEFT JOIN source_item item ON item.source_item_id = CASE
             WHEN issue.issue_kind = 'download_item'
             THEN CAST(substr(issue.issue_key, 13, length(issue.issue_key) - 21) AS INTEGER)
         END
         LEFT JOIN source_post post ON post.source_post_id = item.source_post_id
         WHERE issue.subscription_id = ?1
           AND issue.status != 'resolved'
           AND (?2 IS NULL
                OR issue.last_seen_at < ?2
                OR (issue.last_seen_at = ?2 AND issue.issue_id < ?3))
         ORDER BY issue.last_seen_at DESC, issue.issue_id DESC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![subscription, after_time, after_id, ISSUE_PAGE + 1],
        |row| {
            let item_key: Option<String> = row.get(4)?;
            let post_key: Option<String> = row.get(5)?;
            let title: Option<String> = row.get(6)?;
            Ok(Issue {
                id: row.get(0)?,
                kind: row.get(1)?,
                message: row.get(2)?,
                last_seen_at: row.get(3)?,
                target: title
                    .map(|title| title.trim().to_owned())
                    .filter(|title| !title.is_empty())
                    .or(post_key)
                    .or(item_key),
                url: row.get(7)?,
                detail: row.get(8)?,
                query: row.get(9)?,
            })
        },
    )?;
    let mut items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if items.len() as i64 > ISSUE_PAGE {
        items.truncate(ISSUE_PAGE as usize);
        items
            .last()
            .map(|issue| (issue.last_seen_at.clone(), issue.id))
    } else {
        None
    };
    Ok(Issues { items, total, next })
}

#[cfg(test)]
mod tests {
    use super::{detail, subscriptions};
    use std::path::PathBuf;

    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_snapshot_subscriptions_read() {
        let root = PathBuf::from(std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap());
        let root = crate::application::guard(&root).unwrap();
        let blobs = root.join("blobs");
        let library = picto_library::Library::open(root.join("library.sqlite")).unwrap();
        let list = subscriptions(&library, &blobs).unwrap();
        for subscription in &list {
            let detail = detail(&library, subscription.id).unwrap();
            eprintln!(
                "{} {:?}: {} items, {} sources, status {:?}, cover {}, {} runs, {} of {} issues",
                subscription.id,
                subscription.name,
                subscription.items,
                subscription.queries.len(),
                subscription.run_status,
                subscription.cover.is_some(),
                detail.runs.len(),
                detail.issues.items.len(),
                detail.issues.total,
            );
            assert!(detail.issues.items.len() as i64 <= detail.issues.total);
        }
        assert!(!list.is_empty());
    }
}
