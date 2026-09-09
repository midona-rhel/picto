use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptor, MediaDescriptorBuilder, NativeSourceAdapter, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const APP_TOKEN: &str = "33d57ade8c02dbc5a333db99ff9ae26a";
const RULES_PRIMARY: &str =
    "https://raw.githubusercontent.com/DATAHOARDERS/dynamic-rules/main/onlyfans.json";
const RULES_FALLBACK: &str =
    "https://raw.githubusercontent.com/datawhores/onlyfans-dynamic-rules/main/dynamicRules.json";
const RULES_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CURSOR_BYTES: usize = 32 * 1024;
const MAX_CURSOR_IDS: usize = 512;
const MAX_CURSOR_OFFSET: u32 = 10_000_000;
const MAX_ACTIVE_CREATORS: usize = 32;
const MAX_EMPTY_FEED_PAGES: usize = 100;
const FEED_AREAS: [FeedArea; 4] = [
    FeedArea::Timeline,
    FeedArea::Archived,
    FeedArea::Pinned,
    FeedArea::Streams,
];

pub fn adapter() -> OnlyFansAdapter {
    OnlyFansAdapter::default()
}

#[derive(Default)]
pub struct OnlyFansAdapter {
    rules: Mutex<Option<CachedRules>>,
    profiles: Mutex<HashMap<String, Profile>>,
}

#[derive(Clone)]
struct CachedRules {
    fetched_at: Instant,
    value: DynamicRules,
}

#[derive(Debug, Clone, Deserialize)]
struct Profile {
    #[serde(deserialize_with = "deserialize_id")]
    id: String,
    username: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DynamicRules {
    static_param: String,
    #[serde(default)]
    format: Option<String>,
    checksum_indexes: Vec<usize>,
    checksum_constant: i64,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Cursor {
    version: u8,
    partition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    area: Option<FeedArea>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anchor: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    offset: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seen: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FeedArea {
    Timeline,
    Archived,
    Pinned,
    Streams,
}

#[derive(Debug)]
struct ApiPage {
    posts: Vec<Value>,
    has_more: bool,
    tail_marker: Option<String>,
}

impl NativeSourceAdapter for OnlyFansAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "onlyfans",
            display_name: "OnlyFans",
            domain: "onlyfans.com",
            partitions: &["messages", "purchased", "feed"],
            anonymous: false,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalized_username(query).map(|_| ())
    }

    fn preflight<'a>(
        &'a self,
        query: &'a str,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> crate::adapter::PreflightFuture<'a> {
        Box::pin(async move {
            let username = normalized_username(query)?;
            validate_credentials(credentials)?;
            let rules = self.dynamic_rules(http, cancel).await?;
            let profile = self
                .profile(&username, credentials, &rules, http, cancel)
                .await?;
            let cursor = Cursor {
                version: 1,
                partition: "feed".into(),
                area: Some(FeedArea::Timeline),
                anchor: None,
                offset: 0,
                seen: Vec::new(),
            };
            signed_page(
                http,
                feed_url(&profile.id, FeedArea::Timeline, &cursor)?,
                credentials,
                &rules,
                cancel,
            )
            .await?;
            // A locked PPV post does not establish creator-wide access. Discover
            // it normally so accessible older posts and purchases still settle.
            Ok(())
        })
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let username = normalized_username(&request.query)?;
            validate_credentials(credentials)?;
            let rules = self.dynamic_rules(http, cancel).await?;
            let profile = self
                .profile(&username, credentials, &rules, http, cancel)
                .await?;
            match request.partition.0.as_str() {
                "purchased" => {
                    self.discover_purchased(request, credentials, &rules, &profile, http, cancel)
                        .await
                }
                "messages" => {
                    self.discover_messages(request, credentials, &rules, &profile, http, cancel)
                        .await
                }
                "feed" => {
                    self.discover_feed(request, credentials, &rules, &profile, http, cancel)
                        .await
                }
                _ => Err(invalid_query("unknown OnlyFans partition")),
            }
        })
    }
}

impl OnlyFansAdapter {
    async fn dynamic_rules(
        &self,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DynamicRules, SourceError> {
        if let Some(cached) = self
            .rules
            .lock()
            .expect("OnlyFans rules cache mutex poisoned")
            .as_ref()
            .filter(|cached| cached.fetched_at.elapsed() < RULES_TTL)
        {
            return Ok(cached.value.clone());
        }

        let anonymous = RequestCredentials::default();
        let mut last_error = None;
        for source in [RULES_PRIMARY, RULES_FALLBACK] {
            let url = Url::parse(source).expect("pinned dynamic-rules URL is valid");
            match http
                .get_json::<DynamicRules>(url, &anonymous, cancel)
                .await
                .and_then(validate_rules)
            {
                Ok(rules) => {
                    *self
                        .rules
                        .lock()
                        .expect("OnlyFans rules cache mutex poisoned") = Some(CachedRules {
                        fetched_at: Instant::now(),
                        value: rules.clone(),
                    });
                    return Ok(rules);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            SourceError::new(
                SourceErrorKind::Network,
                "OnlyFans dynamic rules are unavailable",
                true,
            )
        }))
    }

    async fn profile(
        &self,
        username: &str,
        credentials: &RequestCredentials,
        rules: &DynamicRules,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<Profile, SourceError> {
        if let Some(profile) = self
            .profiles
            .lock()
            .expect("OnlyFans profile cache mutex poisoned")
            .get(username)
            .cloned()
        {
            return Ok(profile);
        }
        let url = profile_url(username)?;
        let profile: Profile = signed_get(http, url, credentials, rules, cancel).await?;
        if profile.id.is_empty() || profile.username.trim().is_empty() {
            return Err(invalid_response("OnlyFans profile has no creator identity"));
        }
        let profile_username = normalized_username(&profile.username)?;
        let profile = Profile {
            id: profile.id,
            username: profile_username,
        };
        let mut profiles = self
            .profiles
            .lock()
            .expect("OnlyFans profile cache mutex poisoned");
        if profiles.len() >= MAX_ACTIVE_CREATORS && !profiles.contains_key(username) {
            if let Some(key) = profiles.keys().next().cloned() {
                profiles.remove(&key);
            }
        }
        profiles.insert(username.to_string(), profile.clone());
        Ok(profile)
    }

    async fn discover_purchased(
        &self,
        request: &DiscoveryRequest,
        credentials: &RequestCredentials,
        rules: &DynamicRules,
        profile: &Profile,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let mut cursor = decode_cursor(request, None)?;
        #[cfg(debug_assertions)]
        log_page_request(&cursor);
        let url = purchased_url(&profile.username, cursor.offset)?;
        let page = signed_page(http, url, credentials, rules, cancel).await?;
        let Some(raw) = page.posts.into_iter().next() else {
            if page.has_more {
                return Err(incomplete_page(
                    "purchased page is empty but has more posts",
                ));
            }
            return Ok(empty_terminal());
        };
        cursor.offset = cursor.offset.checked_add(1).ok_or_else(invalid_cursor)?;
        let exhausted = !page.has_more;
        one_post(
            normalize_post(request, profile, credentials, raw, encode_cursor(&cursor)?)?,
            exhausted,
        )
    }

    async fn discover_messages(
        &self,
        request: &DiscoveryRequest,
        credentials: &RequestCredentials,
        rules: &DynamicRules,
        profile: &Profile,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let mut cursor = decode_cursor(request, None)?;
        loop {
            #[cfg(debug_assertions)]
            log_page_request(&cursor);
            let url = messages_url(&profile.id, cursor.anchor.as_deref())?;
            let page = signed_page(http, url, credentials, rules, cancel).await?;
            let Some(raw) = page.posts.into_iter().next() else {
                if page.has_more {
                    return Err(incomplete_page(
                        "message page is empty but has more messages",
                    ));
                }
                return Ok(empty_terminal());
            };
            let id = post_id(&raw)?;
            cursor.anchor = Some(id.clone());
            let exhausted = !page.has_more;
            if !is_creator_message(&raw, &profile.id) {
                if exhausted {
                    return Ok(empty_terminal());
                }
                continue;
            }
            return one_post(
                normalize_post(request, profile, credentials, raw, encode_cursor(&cursor)?)?,
                exhausted,
            );
        }
    }

    async fn discover_feed(
        &self,
        request: &DiscoveryRequest,
        credentials: &RequestCredentials,
        rules: &DynamicRules,
        profile: &Profile,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DiscoveryBatch, SourceError> {
        let mut cursor = decode_cursor(request, Some(FeedArea::Timeline))?;
        let mut empty_pages = 0;
        loop {
            let area = cursor.area.unwrap_or(FeedArea::Timeline);
            #[cfg(debug_assertions)]
            log_page_request(&cursor);
            let url = feed_url(&profile.id, area, &cursor)?;
            let page = signed_page(http, url, credentials, rules, cancel).await?;
            if page.posts.is_empty() && page.has_more {
                empty_pages += 1;
                if empty_pages > MAX_EMPTY_FEED_PAGES {
                    return Err(incomplete_page(
                        "too many empty feed pages with more history",
                    ));
                }
                continue_empty_feed_page(&mut cursor, &page)?;
                #[cfg(debug_assertions)]
                tracing::debug!(
                    area = ?area,
                    next_anchor = diagnostic_number(cursor.anchor.as_deref()),
                    empty_pages,
                    "OnlyFans pagination following empty-page tail marker"
                );
                continue;
            }
            let Some(raw) = page.posts.into_iter().next() else {
                let advanced = advance_feed_area(&mut cursor);
                #[cfg(debug_assertions)]
                tracing::debug!(
                    area = ?area,
                    has_more = page.has_more,
                    next_area = ?cursor.area,
                    exhausted = !advanced,
                    "OnlyFans pagination empty page; advancing feed area"
                );
                if !advanced {
                    return Ok(empty_terminal());
                }
                continue;
            };
            let id = post_id(&raw)?;
            if area == FeedArea::Pinned {
                cursor.offset = cursor.offset.checked_add(1).ok_or_else(invalid_cursor)?;
            } else {
                cursor.anchor = Some(post_timestamp(&raw)?);
            }
            let seen_in_feed = cursor.seen.iter().any(|seen| seen == &id);
            remember_cursor_id(&mut cursor.seen, &id);
            let exhausted_area = !page.has_more;
            let mut after = cursor.clone();
            let exhausted = exhausted_area && !advance_feed_area(&mut after);
            #[cfg(debug_assertions)]
            tracing::debug!(
                area = ?area,
                post_id = diagnostic_number(Some(&id)),
                next_anchor = diagnostic_number(after.anchor.as_deref()),
                seen_in_feed,
                has_more = page.has_more,
                next_area = ?after.area,
                exhausted,
                "OnlyFans pagination post decision"
            );
            if seen_in_feed {
                cursor = after;
                if exhausted {
                    return Ok(empty_terminal());
                }
                continue;
            }
            return one_post(
                normalize_post(request, profile, credentials, raw, encode_cursor(&after)?)?,
                exhausted,
            );
        }
    }
}

async fn signed_page(
    http: &HttpRuntime,
    url: Url,
    credentials: &RequestCredentials,
    rules: &DynamicRules,
    cancel: &CancellationToken,
) -> Result<ApiPage, SourceError> {
    let value: Value = signed_get(http, url, credentials, rules, cancel).await?;
    parse_page(value)
}

fn parse_page(value: Value) -> Result<ApiPage, SourceError> {
    #[cfg(debug_assertions)]
    tracing::debug!(
        response_type = json_kind(Some(&value)),
        list_type = json_kind(value.get("list")),
        has_more_type = json_kind(value.get("hasMore")),
        has_more = value.get("hasMore").and_then(|v| v.as_bool()),
        has_error = value.get("error").is_some(),
        "OnlyFans pagination response shape"
    );
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("OnlyFans API page is not an object"))?;
    let posts = object
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| incomplete_page("API page has no valid post list"))?;
    let has_more = object
        .get("hasMore")
        .and_then(Value::as_bool)
        .ok_or_else(|| incomplete_page("API page has no valid hasMore flag"))?;
    let tail_marker = object.get("tailMarker").and_then(scalar);
    #[cfg(debug_assertions)]
    tracing::debug!(
        posts = posts.len(),
        tail_marker = diagnostic_number(tail_marker.as_deref()),
        first_post_id = diagnostic_number(
            posts
                .first()
                .and_then(|p| p.get("id"))
                .and_then(scalar)
                .as_deref()
        ),
        last_post_id = diagnostic_number(
            posts
                .last()
                .and_then(|p| p.get("id"))
                .and_then(scalar)
                .as_deref()
        ),
        "OnlyFans pagination parsed page"
    );
    Ok(ApiPage {
        posts,
        has_more,
        tail_marker,
    })
}

fn incomplete_page(reason: &str) -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        format!("OnlyFans pagination could not continue: {reason}; source history is not complete"),
        true,
    )
}

fn continue_empty_feed_page(cursor: &mut Cursor, page: &ApiPage) -> Result<(), SourceError> {
    // OnlyFans can filter every post from a page while still returning a valid
    // continuation. Descending feeds must follow tailMarker, not abandon the area.
    if cursor.area == Some(FeedArea::Pinned) {
        return Err(incomplete_page("pinned page is empty but has more posts"));
    }
    let marker = page
        .tail_marker
        .as_deref()
        .ok_or_else(|| incomplete_page("empty feed page has no tail marker"))?;
    let timestamp = |value: &str| {
        value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite() && *number >= 0.0)
    };
    let next = timestamp(marker)
        .ok_or_else(|| incomplete_page("empty feed page has an invalid tail marker"))?;
    if let Some(anchor) = cursor.anchor.as_deref() {
        let previous = timestamp(anchor)
            .ok_or_else(|| incomplete_page("feed cursor has an invalid timestamp"))?;
        if next >= previous {
            return Err(incomplete_page(
                "empty feed page tail marker did not advance",
            ));
        }
    }
    if !valid_cursor_value(marker) {
        return Err(incomplete_page(
            "empty feed page tail marker exceeds cursor bounds",
        ));
    }
    cursor.anchor = Some(marker.to_owned());
    Ok(())
}

// Diagnostics deliberately omit response bodies, URLs, names and credentials.
#[cfg(debug_assertions)]
fn log_page_request(cursor: &Cursor) {
    tracing::debug!(
        partition = %cursor.partition,
        area = ?cursor.area,
        anchor = diagnostic_number(cursor.anchor.as_deref()),
        offset = cursor.offset,
        seen_posts = cursor.seen.len(),
        "OnlyFans pagination request"
    );
}

#[cfg(debug_assertions)]
fn diagnostic_number(value: Option<&str>) -> &str {
    match value {
        None => "none",
        Some(value)
            if !value.is_empty()
                && value.len() <= 32
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || c == b'.' || c == b'-') =>
        {
            value
        }
        Some(_) => "redacted",
    }
}

#[cfg(debug_assertions)]
fn json_kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

async fn signed_get<T: serde::de::DeserializeOwned>(
    http: &HttpRuntime,
    url: Url,
    credentials: &RequestCredentials,
    rules: &DynamicRules,
    cancel: &CancellationToken,
) -> Result<T, SourceError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_response("system clock is before the Unix epoch"))?
        .as_millis()
        .to_string();
    let mut signed = credentials.clone();
    let auth_id = cookie(credentials, "auth_id")?;
    let user_agent = header(credentials, "user-agent")?;
    let x_bc = header(credentials, "x-bc")?;
    let signature = request_signature(rules, &url, auth_id, &timestamp)?;
    signed.headers.extend([
        ("accept".into(), "application/json, text/plain, */*".into()),
        ("app-token".into(), APP_TOKEN.into()),
        ("referer".into(), "https://onlyfans.com/".into()),
        ("user-agent".into(), user_agent.into()),
        ("user-id".into(), auth_id.into()),
        ("x-bc".into(), x_bc.into()),
        ("sign".into(), signature),
        ("time".into(), timestamp),
    ]);
    http.get_json(url, &signed, cancel).await
}

fn validate_credentials(credentials: &RequestCredentials) -> Result<(), SourceError> {
    cookie(credentials, "sess")?;
    cookie(credentials, "auth_id")?;
    header(credentials, "x-bc")?;
    header(credentials, "user-agent")?;
    if !credentials.permits("onlyfans.com") {
        return Err(authentication_error(
            "captured OnlyFans credentials are not scoped to onlyfans.com",
        ));
    }
    Ok(())
}

fn cookie<'a>(credentials: &'a RequestCredentials, name: &str) -> Result<&'a str, SourceError> {
    credentials
        .cookies
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| authentication_error(format!("captured OnlyFans session is missing {name}")))
}

fn header<'a>(credentials: &'a RequestCredentials, name: &str) -> Result<&'a str, SourceError> {
    credentials
        .headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| authentication_error(format!("captured OnlyFans session is missing {name}")))
}

fn request_signature(
    rules: &DynamicRules,
    url: &Url,
    user_id: &str,
    timestamp: &str,
) -> Result<String, SourceError> {
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let digest = sha1_hex(
        format!(
            "{}\n{}\n{}\n{}",
            rules.static_param, timestamp, path, user_id
        )
        .as_bytes(),
    );
    let checksum = rules
        .checksum_indexes
        .iter()
        .try_fold(rules.checksum_constant, |sum, index| {
            digest
                .as_bytes()
                .get(*index)
                .map(|value| sum + i64::from(*value))
                .ok_or_else(|| invalid_response("OnlyFans dynamic rules contain an invalid index"))
        })?
        .unsigned_abs();
    if let (Some(prefix), Some(suffix)) = (&rules.prefix, &rules.suffix) {
        return Ok(format!("{prefix}:{digest}:{checksum:x}:{suffix}"));
    }
    let format = rules
        .format
        .as_deref()
        .ok_or_else(|| invalid_response("OnlyFans dynamic rules contain no signature format"))?;
    let with_digest = format.replacen("{}", &digest, 1);
    let formatted = if with_digest.contains("{:x}") {
        with_digest.replacen("{:x}", &format!("{checksum:x}"), 1)
    } else if with_digest.contains("{:X}") {
        with_digest.replacen("{:X}", &format!("{checksum:X}"), 1)
    } else {
        return Err(invalid_response(
            "OnlyFans dynamic rules contain an unsupported signature format",
        ));
    };
    if formatted.contains('{') || formatted.contains('}') {
        return Err(invalid_response(
            "OnlyFans dynamic rules contain an unsupported signature format",
        ));
    }
    Ok(formatted)
}

fn validate_rules(rules: DynamicRules) -> Result<DynamicRules, SourceError> {
    if rules.static_param.is_empty()
        || rules.checksum_indexes.is_empty()
        || rules.checksum_indexes.iter().any(|index| *index >= 40)
        || (rules.format.is_none() && (rules.prefix.is_none() || rules.suffix.is_none()))
    {
        return Err(invalid_response("OnlyFans dynamic rules are invalid"));
    }
    Ok(rules)
}

fn purchased_url(username: &str, offset: u32) -> Result<Url, SourceError> {
    let mut url = api_url("posts/paid/all")?;
    url.query_pairs_mut()
        .append_pair("limit", "1")
        .append_pair("skip_users", "all")
        .append_pair("format", "infinite")
        .append_pair("offset", &offset.to_string())
        .append_pair("author", username);
    Ok(url)
}

fn messages_url(model_id: &str, anchor: Option<&str>) -> Result<Url, SourceError> {
    let mut url = api_url(&format!("chats/{model_id}/messages"))?;
    url.query_pairs_mut()
        .append_pair("limit", "1")
        .append_pair("order", "desc")
        .append_pair("skip_users", "all")
        .append_pair("skip_users_dups", "1");
    if let Some(anchor) = anchor {
        url.query_pairs_mut().append_pair("id", anchor);
    }
    Ok(url)
}

fn feed_url(model_id: &str, area: FeedArea, cursor: &Cursor) -> Result<Url, SourceError> {
    let suffix = match area {
        FeedArea::Timeline | FeedArea::Pinned => "posts",
        FeedArea::Archived => "posts/archived",
        FeedArea::Streams => "posts/streams",
    };
    let mut url = api_url(&format!("users/{model_id}/{suffix}"))?;
    url.query_pairs_mut()
        .append_pair("limit", "1")
        .append_pair("skip_users", "all")
        .append_pair("skip_users_dups", "1")
        .append_pair("format", "infinite");
    if area == FeedArea::Pinned {
        url.query_pairs_mut()
            .append_pair("pinned", "1")
            .append_pair("counters", &cursor.offset.to_string());
    } else {
        url.query_pairs_mut()
            .append_pair("order", "publish_date_desc");
        if area == FeedArea::Timeline {
            url.query_pairs_mut().append_pair("pinned", "0");
        }
        if let Some(anchor) = cursor.anchor.as_deref() {
            url.query_pairs_mut()
                .append_pair("beforePublishTime", anchor);
        }
    }
    Ok(url)
}

fn profile_url(username: &str) -> Result<Url, SourceError> {
    api_url(&format!("users/{username}"))
}

fn api_url(path: &str) -> Result<Url, SourceError> {
    Url::parse(&format!("https://onlyfans.com/api2/v2/{path}"))
        .map_err(|error| invalid_query(error.to_string()))
}

fn normalize_post(
    request: &DiscoveryRequest,
    profile: &Profile,
    credentials: &RequestCredentials,
    raw: Value,
    resume_cursor_after: String,
) -> Result<SourcePost, SourceError> {
    let id = post_id(&raw)?;
    let notes = raw
        .get("text")
        .and_then(Value::as_str)
        .and_then(normalize_source_text);
    let title = raw
        .get("title")
        .and_then(Value::as_str)
        .and_then(normalize_source_text)
        .or_else(|| notes.as_deref().map(short_title));
    let canonical_url = if request.partition.0 == "messages" {
        Some(format!(
            "https://onlyfans.com/my/chats/{}/?id={id}",
            profile.id
        ))
    } else {
        Some(format!("https://onlyfans.com/{}/{id}", profile.username))
    };
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &profile.username);
    Ok(SourcePost {
        site_id: "onlyfans".into(),
        partition: request.partition.clone(),
        stable_id: id,
        canonical_url,
        creator: Some(profile.username.clone()),
        name: title,
        notes,
        created_at: optional_scalar(&raw, &["postedAt", "createdAt", "postedAtPrecise"]),
        tags: tags.into_vec(),
        media: downloadable_media(&raw, credentials),
        resume_cursor_after: Some(resume_cursor_after),
    })
}

fn downloadable_media(raw: &Value, credentials: &RequestCredentials) -> Vec<MediaDescriptor> {
    raw.get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(position, media)| {
            if media.get("canView").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            let direct = primary_media_url(media);
            let protected = direct.is_none();
            let url = direct.or_else(|| protected_manifest_url(media))?;
            let stable_id = scalar(media.get("id")?)?;
            let mut builder =
                MediaDescriptorBuilder::new(stable_id.clone(), position as u32, url.as_str())
                    .canonical_url(url.as_str());
            if protected {
                builder = builder.unavailable(
                    "OnlyFans DRM-protected media is unsupported: a provisioned CDM/device and content key are required",
                );
            }
            if is_manifest_url(&url) {
                builder = builder.file_name(format!("{stable_id}.mp4"));
            } else if let Some(file_name) = url
                .path_segments()
                .and_then(Iterator::last)
                .filter(|name| !name.is_empty())
            {
                builder = builder.file_name(file_name.to_string());
            }
            let expected_size = media
                .get("size")
                .and_then(Value::as_u64)
                .or_else(|| media.pointer("/files/full/size").and_then(Value::as_u64));
            let mut headers = std::collections::BTreeMap::new();
            headers.insert("referer".into(), "https://onlyfans.com/".into());
            if let Ok(user_agent) = header(credentials, "user-agent") {
                headers.insert("user-agent".into(), user_agent.into());
            }
            Some(
                builder
                    .expected_size(expected_size)
                    .headers(headers)
                    .build(),
            )
        })
        .collect()
}

fn protected_manifest_url(media: &Value) -> Option<Url> {
    [
        media
            .pointer("/files/drm/manifest/dash")
            .and_then(Value::as_str),
        media
            .pointer("/files/drm/manifest/hls")
            .and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .filter_map(|candidate| Url::parse(candidate).ok())
    .find(is_manifest_url)
}

fn primary_media_url(media: &Value) -> Option<Url> {
    for candidate in [
        media.pointer("/files/full/url").and_then(Value::as_str),
        media.pointer("/source/source").and_then(Value::as_str),
        media.get("source").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    {
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        if is_direct_file_url(&url) || is_manifest_url(&url) {
            return Some(url);
        }
    }
    // A rendition alongside a DRM-only original must not become a silent
    // substitute for that original. Full/source URLs above remain preferred.
    if protected_manifest_url(media).is_some() {
        return None;
    }
    media
        .get("videoSources")
        .and_then(Value::as_object)
        .and_then(|sources| {
            sources
                .iter()
                .filter_map(|(quality, value)| {
                    let url = value.as_str().and_then(|value| Url::parse(value).ok())?;
                    if !is_direct_file_url(&url) {
                        return None;
                    }
                    let rank = quality.trim_end_matches('p').parse::<u32>().unwrap_or(0);
                    Some((rank, url))
                })
                .max_by_key(|(rank, _)| *rank)
                .map(|(_, url)| url)
        })
}

fn is_direct_file_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    ![".m3u8", ".mpd", ".rar", ".7z", ".tar", ".tar.gz", ".tgz"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn is_manifest_url(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".mpd")
}

fn decode_cursor(
    request: &DiscoveryRequest,
    initial_area: Option<FeedArea>,
) -> Result<Cursor, SourceError> {
    let Some(raw) = request.cursor.as_deref() else {
        return Ok(Cursor {
            version: 1,
            partition: request.partition.0.clone(),
            area: initial_area,
            anchor: None,
            offset: 0,
            seen: Vec::new(),
        });
    };
    if raw.len() > MAX_CURSOR_BYTES || raw.chars().any(char::is_control) {
        return Err(invalid_cursor());
    }
    let cursor: Cursor = serde_json::from_str(raw).map_err(|_| invalid_cursor())?;
    if cursor.version != 1
        || cursor.partition != request.partition.0
        || cursor.offset > MAX_CURSOR_OFFSET
        || cursor.seen.len() > MAX_CURSOR_IDS
        || cursor
            .anchor
            .as_deref()
            .is_some_and(|value| !valid_cursor_value(value))
        || cursor.seen.iter().any(|value| !valid_cursor_value(value))
        || (request.partition.0 == "feed" && cursor.area.is_none())
        || (request.partition.0 != "feed" && cursor.area.is_some())
    {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn encode_cursor(cursor: &Cursor) -> Result<String, SourceError> {
    let encoded = serde_json::to_string(cursor).map_err(|_| invalid_cursor())?;
    if encoded.len() > MAX_CURSOR_BYTES {
        return Err(invalid_cursor());
    }
    Ok(encoded)
}

fn valid_cursor_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn remember_cursor_id(seen: &mut Vec<String>, id: &str) {
    if !seen.iter().any(|value| value == id) {
        seen.push(id.to_string());
        if seen.len() > MAX_CURSOR_IDS {
            seen.remove(0);
        }
    }
}

fn advance_feed_area(cursor: &mut Cursor) -> bool {
    let current = cursor.area.unwrap_or(FeedArea::Timeline);
    let Some(index) = FEED_AREAS.iter().position(|area| *area == current) else {
        return false;
    };
    let Some(next) = FEED_AREAS.get(index + 1).copied() else {
        return false;
    };
    cursor.area = Some(next);
    cursor.anchor = None;
    cursor.offset = 0;
    true
}

fn post_id(raw: &Value) -> Result<String, SourceError> {
    raw.get("id")
        .and_then(scalar)
        .filter(|id| valid_cursor_value(id))
        .ok_or_else(|| invalid_response("OnlyFans post has no valid id"))
}

fn post_timestamp(raw: &Value) -> Result<String, SourceError> {
    optional_scalar(raw, &["postedAtPrecise"])
        .filter(|value| valid_cursor_value(value))
        .ok_or_else(|| invalid_response("OnlyFans feed post has no pagination timestamp"))
}

fn is_creator_message(raw: &Value, model_id: &str) -> bool {
    raw.pointer("/fromUser/id")
        .and_then(scalar)
        .or_else(|| raw.pointer("/author/id").and_then(scalar))
        .as_deref()
        == Some(model_id)
}

fn optional_scalar(raw: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| raw.get(*key).and_then(scalar))
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn short_title(text: &str) -> String {
    if text.chars().count() <= 120 {
        return text.to_string();
    }
    let shortened = text.chars().take(120).collect::<String>();
    let boundary = shortened
        .rfind(char::is_whitespace)
        .unwrap_or(shortened.len());
    format!("{}...", shortened[..boundary].trim_end())
}

fn normalized_username(query: &str) -> Result<String, SourceError> {
    let trimmed = query.trim();
    let candidate = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed).map_err(|_| invalid_query("invalid OnlyFans creator URL"))?;
        let host = url.host_str().unwrap_or_default();
        if host != "onlyfans.com" && host != "www.onlyfans.com" {
            return Err(invalid_query("OnlyFans creator URL has the wrong domain"));
        }
        url.path_segments()
            .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
            .map(ToOwned::to_owned)
            .ok_or_else(|| invalid_query("OnlyFans creator URL has no username"))?
    } else {
        trimmed.strip_prefix('@').unwrap_or(trimmed).to_string()
    };
    if candidate.is_empty()
        || candidate.len() > 64
        || candidate.eq_ignore_ascii_case("api2")
        || candidate.eq_ignore_ascii_case("my")
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid_query("invalid OnlyFans creator username"));
    }
    Ok(candidate.to_ascii_lowercase())
}

fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    scalar(&value).ok_or_else(|| serde::de::Error::custom("id must be a string or number"))
}

fn sha1_hex(input: &[u8]) -> String {
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in message.as_chunks::<64>().0 {
        let mut words = [0_u32; 80];
        for (index, bytes) in chunk.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*bytes);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

fn one_post(post: SourcePost, exhausted: bool) -> Result<DiscoveryBatch, SourceError> {
    Ok(DiscoveryBatch {
        posts: vec![post],
        exhausted,
    })
}

fn empty_terminal() -> DiscoveryBatch {
    DiscoveryBatch {
        posts: Vec::new(),
        exhausted: true,
    }
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn invalid_cursor() -> SourceError {
    invalid_query("invalid OnlyFans pagination cursor")
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

fn authentication_error(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::Authentication, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePartition;

    #[test]
    fn malformed_pages_cannot_report_history_exhaustion() {
        let page = parse_page(serde_json::json!({"list": [{"id": 1}], "hasMore": true})).unwrap();
        assert_eq!(page.posts.len(), 1);
        assert!(page.has_more);
        for value in [
            serde_json::json!({"list": [], "hasMore": null}),
            serde_json::json!({"list": [], "hasMore": "true"}),
            serde_json::json!({"list": []}),
            serde_json::json!({"unexpected": [], "hasMore": false}),
            serde_json::json!({"error": {"message": "fixture"}}),
        ] {
            let error = parse_page(value).unwrap_err();
            assert!(error.retryable);
            assert_eq!(error.kind, SourceErrorKind::InvalidResponse);
        }
        let page = parse_page(serde_json::json!({"list": [], "hasMore": false})).unwrap();
        assert!(page.posts.is_empty());
        assert!(!page.has_more);
        assert!(parse_page(serde_json::json!([])).is_err());
    }

    #[test]
    fn empty_timeline_page_continues_at_server_marker_and_round_trips() {
        let page = parse_page(
            serde_json::from_str(include_str!(
                "../../tests/fixtures/onlyfans/empty-timeline-continuation.json"
            ))
            .unwrap(),
        )
        .unwrap();
        assert!(page.posts.is_empty());
        assert!(page.has_more);
        let mut cursor = decode_cursor(&request("feed", None), Some(FeedArea::Timeline)).unwrap();
        cursor.anchor = Some("1774040408.000000".into());
        cursor.seen = vec!["9101".into()];
        continue_empty_feed_page(&mut cursor, &page).unwrap();
        assert_eq!(cursor.area, Some(FeedArea::Timeline));
        assert_eq!(cursor.seen, ["9101"]);
        assert_eq!(cursor.anchor.as_deref(), Some("1773864009.000000"));
        let resumed = decode_cursor(
            &request("feed", Some(encode_cursor(&cursor).unwrap())),
            Some(FeedArea::Timeline),
        )
        .unwrap();
        assert_eq!(resumed, cursor);
        assert!(feed_url("42", FeedArea::Timeline, &resumed)
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "beforePublishTime" && value == "1773864009.000000"));
    }

    #[test]
    fn unusable_empty_page_markers_preserve_the_resume_cursor() {
        for marker in [
            Value::Null,
            serde_json::json!("NaN"),
            serde_json::json!("inf"),
            serde_json::json!(-1),
            serde_json::json!(1774040408.0),
            serde_json::json!(1774040409.0),
        ] {
            let page =
                parse_page(serde_json::json!({"list": [], "hasMore": true, "tailMarker": marker}))
                    .unwrap();
            let mut cursor =
                decode_cursor(&request("feed", None), Some(FeedArea::Timeline)).unwrap();
            cursor.anchor = Some("1774040408.000000".into());
            let before = cursor.clone();
            assert!(
                continue_empty_feed_page(&mut cursor, &page)
                    .unwrap_err()
                    .retryable
            );
            assert_eq!(cursor, before);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn pagination_diagnostics_only_expose_numeric_cursor_values() {
        assert_eq!(
            diagnostic_number(Some("1774040408.000000")),
            "1774040408.000000"
        );
        assert_eq!(diagnostic_number(None), "none");
        for value in [
            "sess=secret",
            "https://example.test/?token=secret",
            "123\nsecret",
            "",
        ] {
            assert_eq!(diagnostic_number(Some(value)), "redacted");
        }
        assert_eq!(json_kind(None), "missing");
        assert_eq!(json_kind(Some(&serde_json::json!("secret"))), "string");
    }

    fn request(partition: &str, cursor: Option<String>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "FixtureCreator".into(),
            partition: SourcePartition::new(partition),
            cursor,
            page_size: 25,
        }
    }

    fn fixture(name: &str) -> Value {
        serde_json::from_str(match name {
            "purchased" => include_str!("../../tests/fixtures/onlyfans/purchased.json"),
            "messages" => include_str!("../../tests/fixtures/onlyfans/messages.json"),
            "timeline" => include_str!("../../tests/fixtures/onlyfans/timeline.json"),
            "archived" => include_str!("../../tests/fixtures/onlyfans/archived.json"),
            "pinned" => include_str!("../../tests/fixtures/onlyfans/pinned.json"),
            "streams" => include_str!("../../tests/fixtures/onlyfans/streams.json"),
            _ => panic!("unknown fixture"),
        })
        .unwrap()
    }

    fn profile() -> Profile {
        serde_json::from_str(include_str!("../../tests/fixtures/onlyfans/profile.json")).unwrap()
    }

    #[test]
    fn partitions_are_prioritized_and_queries_resolve_to_usernames() {
        let adapter = adapter();
        assert_eq!(
            adapter.descriptor().partitions,
            &["messages", "purchased", "feed"]
        );
        assert_eq!(
            normalized_username("https://onlyfans.com/FixtureCreator/").unwrap(),
            "fixturecreator"
        );
        assert!(normalized_username("https://example.test/creator").is_err());
    }

    #[test]
    fn signature_matches_the_pinned_ofscraper_algorithm() {
        let rules: DynamicRules = serde_json::from_str(include_str!(
            "../../tests/fixtures/onlyfans/dynamic_rules.json"
        ))
        .unwrap();
        let url = Url::parse("https://onlyfans.com/api2/v2/users/42?limit=1").unwrap();
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            request_signature(&rules, &url, "123456", "1700000000123").unwrap(),
            "prefix:00dbc5c273096f9693af54c5e49cc4f0b242acb2:14f:suffix"
        );
    }

    #[test]
    fn cursors_are_partition_bound_and_bounded() {
        let cursor = Cursor {
            version: 1,
            partition: "feed".into(),
            area: Some(FeedArea::Archived),
            anchor: Some("1700000000.125".into()),
            offset: 0,
            seen: vec!["9001".into()],
        };
        let encoded = encode_cursor(&cursor).unwrap();
        assert_eq!(
            decode_cursor(&request("feed", Some(encoded)), None).unwrap(),
            cursor
        );
        assert!(decode_cursor(&request("messages", Some("{}".into())), None).is_err());

        let oversized = "x".repeat(MAX_CURSOR_BYTES + 1);
        assert!(decode_cursor(&request("feed", Some(oversized)), None).is_err());
    }

    #[test]
    fn purchased_fixture_keeps_direct_media_in_creator_order() {
        let raw = fixture("purchased")["list"][0].clone();
        let post = normalize_post(
            &request("purchased", None),
            &profile(),
            &RequestCredentials::default(),
            raw,
            "next".into(),
        )
        .unwrap();
        assert_eq!(post.stable_id, "9001");
        assert_eq!(post.creator.as_deref(), Some("fixturecreator"));
        assert_eq!(post.name.as_deref(), Some("Hello & welcome bold text"));
        assert_eq!(post.notes.as_deref(), Some("Hello & welcome bold text"));
        assert_eq!(
            post.tags,
            vec![crate::CanonicalTag::new("creator", "fixturecreator")]
        );
        assert_eq!(
            post.media
                .iter()
                .map(|media| (media.stable_id.as_str(), media.position))
                .collect::<Vec<_>>(),
            vec![
                ("501", 0),
                ("502", 1),
                ("503", 2),
                ("504", 3),
                ("505", 4),
                ("506", 5),
                ("508", 7),
            ]
        );
        assert!(post.media.iter().any(|media| media.url.ends_with(".zip")));
    }

    #[test]
    fn locked_post_is_individually_traversable_before_accessible_history() {
        let mut locked = fixture("timeline")["list"][0].clone();
        locked["media"][0]["canView"] = Value::Bool(false);
        let page =
            parse_page(serde_json::json!({ "list": [locked.clone()], "hasMore": true })).unwrap();
        assert!(page.has_more);
        let post = normalize_post(
            &request("feed", None),
            &profile(),
            &RequestCredentials::default(),
            locked,
            "older".into(),
        )
        .unwrap();
        assert!(post.media.is_empty());
        assert_eq!(post.resume_cursor_after.as_deref(), Some("older"));
        let older = normalize_post(
            &request("feed", None),
            &profile(),
            &RequestCredentials::default(),
            fixture("timeline")["list"][0].clone(),
            "next".into(),
        )
        .unwrap();
        assert!(!older.media.is_empty());
    }

    #[test]
    fn fixtures_cover_creator_messages_and_every_feed_area() {
        let message = &fixture("messages")["list"][0];
        assert!(is_creator_message(message, "42"));
        assert!(!is_creator_message(&fixture("messages")["list"][1], "42"));

        for (name, area, expected_id) in [
            ("timeline", FeedArea::Timeline, "9101"),
            ("archived", FeedArea::Archived, "9102"),
            ("pinned", FeedArea::Pinned, "9103"),
            ("streams", FeedArea::Streams, "9104"),
        ] {
            let value = fixture(name);
            assert_eq!(post_id(&value["list"][0]).unwrap(), expected_id);
            let cursor = Cursor {
                version: 1,
                partition: "feed".into(),
                area: Some(area),
                anchor: None,
                offset: 0,
                seen: Vec::new(),
            };
            let url = feed_url("42", area, &cursor).unwrap();
            assert_eq!(url.host_str(), Some("onlyfans.com"));
            let expected_path = match area {
                FeedArea::Timeline | FeedArea::Pinned => "/api2/v2/users/42/posts",
                FeedArea::Archived => "/api2/v2/users/42/posts/archived",
                FeedArea::Streams => "/api2/v2/users/42/posts/streams",
            };
            assert_eq!(url.path(), expected_path);
        }
    }

    #[test]
    fn partition_urls_use_independent_bounded_continuations() {
        let purchased = purchased_url("fixturecreator", 7).unwrap();
        assert_eq!(purchased.path(), "/api2/v2/posts/paid/all");
        assert!(purchased.query().unwrap().contains("offset=7"));
        assert!(purchased.query().unwrap().contains("author=fixturecreator"));

        let messages = messages_url("42", Some("9002")).unwrap();
        assert_eq!(messages.path(), "/api2/v2/chats/42/messages");
        assert!(messages.query().unwrap().contains("id=9002"));

        let cursor = Cursor {
            version: 1,
            partition: "feed".into(),
            area: Some(FeedArea::Timeline),
            anchor: Some("1787918400.25".into()),
            offset: 0,
            seen: Vec::new(),
        };
        let timeline = feed_url("42", FeedArea::Timeline, &cursor).unwrap();
        assert!(timeline
            .query()
            .unwrap()
            .contains("beforePublishTime=1787918400.25"));
    }

    #[test]
    fn clear_protected_and_zip_media_are_reported() {
        let raw = fixture("purchased")["list"][0].clone();
        let media = downloadable_media(&raw, &RequestCredentials::default());
        let ids = media
            .iter()
            .map(|descriptor| descriptor.stable_id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"503"));
        assert!(ids.contains(&"504"));
        assert!(ids.contains(&"505"));
        assert!(ids.contains(&"508"));
        let hls = media.iter().find(|media| media.stable_id == "504").unwrap();
        assert_eq!(hls.file_name.as_deref(), Some("504.mp4"));
        assert_eq!(hls.delivery(), crate::MediaDelivery::Hls);
        let protected = media.iter().find(|media| media.stable_id == "503").unwrap();
        assert_eq!(protected.file_name.as_deref(), Some("503.mp4"));
        assert_eq!(protected.delivery(), crate::MediaDelivery::Dash);
        assert!(protected
            .unavailable_reason
            .as_deref()
            .unwrap()
            .contains("CDM/device"));
        assert!(hls.unavailable_reason.is_none());
        let full_manifest = media.iter().find(|media| media.stable_id == "508").unwrap();
        assert_eq!(full_manifest.url, "https://cdn.example.test/media/508.m3u8");
        assert_eq!(full_manifest.delivery(), crate::MediaDelivery::Hls);
    }

    #[test]
    fn drm_original_does_not_silently_become_a_video_rendition() {
        let raw = serde_json::json!({"media": [{
            "id": 700, "canView": true,
            "files": {"drm": {"manifest": {"dash": "https://cdn.example.test/protected.mpd"}}},
            "videoSources": {"720": "https://cdn.example.test/rendition.mp4"}
        }]});
        let media = downloadable_media(&raw, &RequestCredentials::default());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].url, "https://cdn.example.test/protected.mpd");
        assert!(media[0].unavailable_reason.is_some());
    }
}
