use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;
use xitter_txid::ClientTransaction;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, MediaFallback, NativeSourceAdapter, OpaqueCursor,
    ProviderDescriptor, RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DOMAIN: &str = "x.com";
const CURSOR: OpaqueCursor = OpaqueCursor::new(2_048);
const MAX_EMPTY_TIMELINE_PAGES: usize = 8;
const TRANSACTION_CACHE_TTL: Duration = Duration::from_secs(10_800);
const PUBLIC_BEARER: &str = "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const USER_LOOKUP_OPERATION: &str = "ck5KkZ8t5cOmoLssopN99Q/UserByScreenName";
const USER_MEDIA_OPERATION: &str = "jCRhbOzdgOHp6u9H4g2tEg/UserMedia";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    TwitterSource::default()
}

#[derive(Default)]
struct TwitterSource {
    transaction: Mutex<Option<CachedClientTransaction>>,
}

struct CachedClientTransaction {
    client: ClientTransaction,
    expires_at: Instant,
}

impl TwitterSource {
    async fn graphql_credentials(
        &self,
        credentials: &RequestCredentials,
        http: &HttpRuntime,
        url: &Url,
        cancel: &CancellationToken,
    ) -> Result<RequestCredentials, SourceError> {
        // Match the website flow: bootstrap transaction keys with the normal
        // cookie session, then attach GraphQL headers only to the API call.
        api_credentials(credentials, http)?;
        let path = transaction_path(url);
        let transaction_id = self
            .transaction_id(http, credentials, &path, cancel)
            .await?;

        // Bootstrap responses can rotate ct0. Re-read the runtime jar before
        // binding CSRF and transaction headers to the actual API request.
        let mut api = api_credentials(credentials, http)?;
        api.headers
            .insert("x-client-transaction-id".to_string(), transaction_id);
        Ok(api)
    }

    async fn transaction_id(
        &self,
        http: &HttpRuntime,
        credentials: &RequestCredentials,
        path: &str,
        cancel: &CancellationToken,
    ) -> Result<String, SourceError> {
        let mut cached = self.transaction.lock().await;
        if cached
            .as_ref()
            .is_none_or(|cached| cached.expires_at <= Instant::now())
        {
            let home = http
                .get_browser_text(twitter_home_url(), credentials, cancel)
                .await?;
            let script_url = transaction_script_url(&home)?;
            let script_url = Url::parse(&script_url).map_err(|error| {
                SourceError::new(
                    SourceErrorKind::InvalidResponse,
                    format!("Twitter / X returned an invalid transaction script URL: {error}"),
                    false,
                )
            })?;
            let script = http
                .get_browser_text(script_url, credentials, cancel)
                .await?;
            let client =
                ClientTransaction::new(&home, &script).map_err(transaction_response_error)?;
            *cached = Some(CachedClientTransaction {
                client,
                expires_at: Instant::now() + TRANSACTION_CACHE_TTL,
            });
        }

        Ok(cached
            .as_ref()
            .expect("transaction client was initialized")
            .client
            .generate_transaction_id("GET", path))
    }
}

impl NativeSourceAdapter for TwitterSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "twitter",
            display_name: "Twitter / X",
            domain: DOMAIN,
            partitions: &["media"],
            anonymous: false,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_username(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let username = normalize_username(&request.query)?;
            let mut state = request
                .cursor
                .as_deref()
                .filter(|cursor| !cursor.is_empty())
                .map(decode_cursor)
                .transpose()?;
            if state
                .as_ref()
                .is_some_and(|state| state.timeline_cursor.is_none() && state.page_index == 0)
            {
                return Ok(DiscoveryBatch {
                    posts: Vec::new(),
                    exhausted: true,
                });
            }

            let user_id = match state.as_ref() {
                Some(state) => state.user_id.clone(),
                None => {
                    let url = user_lookup_url(&username);
                    let api_credentials = self
                        .graphql_credentials(credentials, http, &url, cancel)
                        .await?;
                    let response = http
                        .get_browser_json::<Value>(url, &api_credentials, cancel)
                        .await?;
                    parse_user_id(&response)?
                }
            };
            for _ in 0..MAX_EMPTY_TIMELINE_PAGES {
                let url = user_timeline_url(
                    &user_id,
                    state
                        .as_ref()
                        .and_then(|state| state.timeline_cursor.as_deref()),
                );
                let api_credentials = self
                    .graphql_credentials(credentials, http, &url, cancel)
                    .await?;
                let response = http
                    .get_browser_json::<Value>(url, &api_credentials, cancel)
                    .await?;
                let page = parse_timeline(&response)?;
                if page.tweets.is_empty() && page.continue_on_empty {
                    if let Some(cursor) = page.bottom_cursor.as_ref().filter(|cursor| {
                        state
                            .as_ref()
                            .and_then(|state| state.timeline_cursor.as_ref())
                            != Some(*cursor)
                    }) {
                        state = Some(CursorState {
                            user_id: user_id.clone(),
                            timeline_cursor: Some(cursor.clone()),
                            page_index: 0,
                            next_tweet_id: None,
                        });
                        continue;
                    }
                }
                return normalize_timeline_page(request, &username, &user_id, state.as_ref(), page);
            }
            Err(SourceError::new(
                SourceErrorKind::InvalidResponse,
                "Twitter / X returned too many empty continuation pages",
                true,
            ))
        })
    }
}

fn api_credentials(
    credentials: &RequestCredentials,
    http: &HttpRuntime,
) -> Result<RequestCredentials, SourceError> {
    let runtime_csrf = http.cookie_value(&twitter_home_url(), "ct0");
    api_credentials_with_csrf(credentials, runtime_csrf.as_deref())
}

fn api_credentials_with_csrf(
    credentials: &RequestCredentials,
    runtime_csrf: Option<&str>,
) -> Result<RequestCredentials, SourceError> {
    let mut api = credentials.clone();
    api.allowed_domains.insert(DOMAIN.to_string());
    api.headers
        .insert("Authorization".to_string(), PUBLIC_BEARER.to_string());
    api.headers.insert("Accept".to_string(), "*/*".to_string());
    api.headers
        .insert("content-type".to_string(), "application/json".to_string());
    api.headers
        .insert("Referer".to_string(), "https://x.com/".to_string());
    api.headers
        .insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    api.headers
        .insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    api.headers
        .insert("Sec-Fetch-Site".to_string(), "same-origin".to_string());
    api.headers
        .insert("x-twitter-active-user".to_string(), "yes".to_string());
    api.headers
        .insert("x-twitter-client-language".to_string(), "en".to_string());

    if !api.cookies.contains_key("auth_token") {
        return Err(SourceError::new(
            SourceErrorKind::Authentication,
            "Twitter / X requires a signed-in direct-site session",
            false,
        ));
    }
    let csrf = runtime_csrf
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| api.cookies.get("ct0").map(|value| value.trim().to_string()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(generate_csrf_token);
    api.cookies.insert("ct0".to_string(), csrf.clone());
    api.headers.insert("x-csrf-token".to_string(), csrf);
    api.headers.insert(
        "x-twitter-auth-type".to_string(),
        "OAuth2Session".to_string(),
    );
    Ok(api)
}

fn twitter_home_url() -> Url {
    Url::parse("https://x.com/").expect("static Twitter home URL")
}

fn transaction_path(url: &Url) -> String {
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

fn transaction_script_url(home: &str) -> Result<String, SourceError> {
    if let Ok(url) = ClientTransaction::extract_ondemand_url(home) {
        return Ok(url);
    }

    let marker_position = home.find("\"ondemand.s\"").ok_or_else(|| {
        SourceError::new(
            SourceErrorKind::InvalidResponse,
            "Twitter / X transaction bootstrap did not advertise its request script",
            false,
        )
    })?;
    let key_start = home[..marker_position]
        .rfind(',')
        .map(|position| position + 1)
        .ok_or_else(|| invalid_transaction_bootstrap("bundle key"))?;
    let key_end = home[key_start..]
        .find(':')
        .map(|position| key_start + position)
        .ok_or_else(|| invalid_transaction_bootstrap("bundle key"))?;
    let key = home[key_start..key_end].trim();
    if key.is_empty() {
        return Err(invalid_transaction_bootstrap("bundle key"));
    }
    let hash_marker = format!("{key}:\"");
    let hash_start = home[marker_position..]
        .find(&hash_marker)
        .map(|position| marker_position + position + hash_marker.len())
        .ok_or_else(|| invalid_transaction_bootstrap("script hash"))?;
    let hash_end = home[hash_start..]
        .find('"')
        .map(|position| hash_start + position)
        .ok_or_else(|| invalid_transaction_bootstrap("script hash"))?;
    let hash = &home[hash_start..hash_end];
    if hash.is_empty()
        || !hash
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(invalid_transaction_bootstrap("script hash"));
    }
    Ok(format!(
        "https://abs.twimg.com/responsive-web/client-web/ondemand.s.{hash}a.js"
    ))
}

fn invalid_transaction_bootstrap(component: &str) -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        format!("Twitter / X transaction bootstrap returned an invalid {component}"),
        false,
    )
}

fn transaction_response_error(error: xitter_txid::Error) -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidResponse,
        format!("Twitter / X transaction bootstrap failed: {error}"),
        false,
    )
}

fn generate_csrf_token() -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut random = rand::thread_rng();
    (0..32)
        .map(|_| HEX[random.gen_range(0..HEX.len())] as char)
        .collect()
}

fn user_lookup_url(username: &str) -> Url {
    let variables = json!({
        "screen_name": username,
        "withGrokTranslatedBio": false
    });
    let field_toggles = json!({"withAuxiliaryUserLabels": true});
    graphql_url(
        USER_LOOKUP_OPERATION,
        variables,
        user_features(),
        Some(field_toggles),
    )
}

fn user_timeline_url(user_id: &str, cursor: Option<&str>) -> Url {
    let mut variables = json!({
        "userId": user_id,
        // X enforces a server-side minimum. A persisted page index still
        // exposes exactly one source post between settlements.
        "count": 20,
        "includePromotedContent": false,
        "withClientEventToken": false,
        "withBirdwatchNotes": false,
        "withVoice": true
    });
    if let Some(cursor) = cursor {
        variables["cursor"] = Value::String(cursor.to_string());
    }
    graphql_url(
        USER_MEDIA_OPERATION,
        variables,
        timeline_features(),
        Some(json!({"withArticlePlainText": false})),
    )
}

fn graphql_url(
    operation: &str,
    variables: Value,
    features: Value,
    field_toggles: Option<Value>,
) -> Url {
    let mut url = Url::parse("https://x.com/i/api/graphql").expect("static Twitter API URL");
    url.path_segments_mut()
        .expect("Twitter API URL supports path segments")
        .extend(operation.split('/'));
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("variables", &variables.to_string());
        query.append_pair("features", &features.to_string());
        if let Some(field_toggles) = field_toggles {
            query.append_pair("fieldToggles", &field_toggles.to_string());
        }
    }
    url
}

fn user_features() -> Value {
    json!({
        "hidden_profile_subscriptions_enabled": true,
        "payments_enabled": false,
        "rweb_xchat_enabled": false,
        "profile_label_improvements_pcf_label_in_post_enabled": true,
        "rweb_tipjar_consumption_enabled": true,
        "verified_phone_label_enabled": false,
        "highlights_tweets_tab_ui_enabled": true,
        "responsive_web_twitter_article_notes_tab_enabled": true,
        "subscriptions_feature_can_gift_premium": true,
        "creator_subscriptions_tweet_preview_api_enabled": true,
        "responsive_web_graphql_timeline_navigation_enabled": true,
        "responsive_web_graphql_skip_user_profile_image_extensions_enabled": false
    })
}

fn timeline_features() -> Value {
    json!({
        "rweb_video_screen_enabled": false,
        "payments_enabled": false,
        "rweb_xchat_enabled": false,
        "profile_label_improvements_pcf_label_in_post_enabled": true,
        "rweb_tipjar_consumption_enabled": true,
        "verified_phone_label_enabled": false,
        "creator_subscriptions_tweet_preview_api_enabled": true,
        "responsive_web_graphql_timeline_navigation_enabled": true,
        "responsive_web_graphql_skip_user_profile_image_extensions_enabled": false,
        "premium_content_api_read_enabled": false,
        "communities_web_enable_tweet_community_results_fetch": true,
        "c9s_tweet_anatomy_moderator_badge_enabled": true,
        "responsive_web_grok_analyze_button_fetch_trends_enabled": false,
        "responsive_web_grok_analyze_post_followups_enabled": true,
        "responsive_web_jetfuel_frame": true,
        "responsive_web_grok_share_attachment_enabled": true,
        "articles_preview_enabled": true,
        "responsive_web_edit_tweet_api_enabled": true,
        "graphql_is_translatable_rweb_tweet_is_translatable_enabled": true,
        "view_counts_everywhere_api_enabled": true,
        "longform_notetweets_consumption_enabled": true,
        "responsive_web_twitter_article_tweet_consumption_enabled": true,
        "tweet_awards_web_tipping_enabled": false,
        "responsive_web_grok_show_grok_translated_post": false,
        "responsive_web_grok_analysis_button_from_backend": true,
        "creator_subscriptions_quote_tweet_preview_enabled": false,
        "freedom_of_speech_not_reach_fetch_enabled": true,
        "standardized_nudges_misinfo": true,
        "tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled": true,
        "longform_notetweets_rich_text_read_enabled": true,
        "longform_notetweets_inline_media_enabled": true,
        "responsive_web_grok_image_annotation_enabled": true,
        "responsive_web_grok_imagine_annotation_enabled": true,
        "responsive_web_grok_community_note_auto_translation_is_enabled": false,
        "responsive_web_enhance_cards_enabled": false
    })
}

#[cfg(test)]
fn normalize_timeline(
    request: &DiscoveryRequest,
    query_username: &str,
    user_id: &str,
    state: Option<&CursorState>,
    response: &Value,
) -> Result<DiscoveryBatch, SourceError> {
    normalize_timeline_page(
        request,
        query_username,
        user_id,
        state,
        parse_timeline(response)?,
    )
}

fn normalize_timeline_page(
    request: &DiscoveryRequest,
    query_username: &str,
    user_id: &str,
    state: Option<&CursorState>,
    page: TimelinePage,
) -> Result<DiscoveryBatch, SourceError> {
    let persisted_page_index = state.map_or(0, |state| state.page_index as usize);
    let stable_page_index = state
        .and_then(|state| state.next_tweet_id.as_deref())
        .and_then(|next_tweet_id| {
            page.tweets
                .iter()
                .position(|tweet| tweet_stable_id(tweet) == Some(next_tweet_id))
        });
    if let Some(expected_tweet_id) = state
        .and_then(|state| state.next_tweet_id.as_deref())
        .filter(|_| stable_page_index.is_none())
    {
        tracing::warn!(
            expected_tweet_id,
            persisted_page_index,
            current_page_size = page.tweets.len(),
            "Twitter / X timeline changed; falling back to the persisted position"
        );
    }
    let page_index = stable_page_index.unwrap_or_else(|| {
        if !page.tweets.is_empty() && persisted_page_index >= page.tweets.len() {
            tracing::warn!(
                persisted_page_index,
                current_page_size = page.tweets.len(),
                "Twitter / X timeline changed; resuming from the last available post"
            );
        }
        persisted_page_index.min(page.tweets.len().saturating_sub(1))
    });
    let exhausted = page.tweets.is_empty()
        && (!page.continue_on_empty || page.bottom_cursor.as_deref().is_none());
    let posts = page
        .tweets
        .get(page_index)
        .cloned()
        .map(|tweet| {
            let (timeline_cursor, next_page_index, next_tweet_id) =
                if page_index + 1 < page.tweets.len() {
                    (
                        state.and_then(|state| state.timeline_cursor.clone()),
                        (page_index + 1) as u16,
                        page.tweets
                            .get(page_index + 1)
                            .and_then(tweet_stable_id)
                            .map(ToOwned::to_owned),
                    )
                } else {
                    (page.bottom_cursor, 0, None)
                };
            normalize_tweet(
                request,
                query_username,
                tweet,
                encode_cursor(&CursorState {
                    user_id: user_id.to_string(),
                    timeline_cursor,
                    page_index: next_page_index,
                    next_tweet_id,
                })?,
            )
        })
        .transpose()?
        .into_iter()
        .collect();
    Ok(DiscoveryBatch { posts, exhausted })
}

#[derive(Debug)]
struct TimelinePage {
    tweets: Vec<Value>,
    bottom_cursor: Option<String>,
    continue_on_empty: bool,
}

fn parse_timeline(response: &Value) -> Result<TimelinePage, SourceError> {
    reject_api_errors(response)?;
    let instructions = find_instructions(response).ok_or_else(|| {
        invalid_response("Twitter / X media response is missing timeline instructions")
    })?;
    let mut entries = Vec::new();
    for instruction in instructions {
        match instruction.get("type").and_then(Value::as_str) {
            Some("TimelineAddEntries") => {
                if let Some(values) = instruction.get("entries").and_then(Value::as_array) {
                    entries.extend(values);
                }
            }
            Some("TimelineAddToModule") => {
                if let Some(values) = instruction.get("moduleItems").and_then(Value::as_array) {
                    entries.extend(values);
                }
            }
            Some("TimelineReplaceEntry") => {
                if let Some(entry) = instruction.get("entry") {
                    entries.push(entry);
                }
            }
            _ => {}
        }
    }

    let mut tweets = Vec::new();
    let mut bottom = None;
    for entry in entries {
        if let Some(cursor) = bottom_cursor(entry) {
            bottom = Some(cursor);
        }
        tweet_results(entry, &mut tweets);
    }
    let (bottom_cursor, continue_on_empty) = bottom
        .map(|cursor: TimelineCursor| (Some(cursor.value), !cursor.stop_on_empty))
        .unwrap_or((None, false));
    Ok(TimelinePage {
        tweets,
        bottom_cursor,
        continue_on_empty,
    })
}

fn normalize_tweet(
    request: &DiscoveryRequest,
    query_username: &str,
    raw: Value,
    resume_cursor_after: String,
) -> Result<SourcePost, SourceError> {
    let tweet = unwrap_tweet(&raw);
    let legacy = tweet
        .get("legacy")
        .ok_or_else(|| invalid_response("Twitter / X post is missing legacy fields"))?;
    let stable_id = tweet
        .get("rest_id")
        .and_then(Value::as_str)
        .or_else(|| legacy.get("id_str").and_then(Value::as_str))
        .filter(|value| valid_numeric_token(value))
        .ok_or_else(|| invalid_response("Twitter / X post has an invalid ID"))?
        .to_string();
    let creator = tweet
        .pointer("/core/user_results/result/legacy/screen_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(query_username)
        .to_string();
    let canonical_url = format!("https://x.com/{creator}/status/{stable_id}");
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator);
    if let Some(hashtags) = legacy
        .pointer("/entities/hashtags")
        .and_then(Value::as_array)
    {
        for hashtag in hashtags {
            if let Some(value) = hashtag
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                tags.insert("", value);
            }
        }
    }

    let is_retweet = legacy.get("retweeted_status_result").is_some()
        || legacy
            .get("full_text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.starts_with("RT @"));
    let media = if is_retweet {
        Vec::new()
    } else {
        normalize_media(legacy, &stable_id, &canonical_url)?
    };
    Ok(SourcePost {
        site_id: "twitter".to_string(),
        partition: request.partition.clone(),
        stable_id,
        canonical_url: Some(canonical_url),
        creator: Some(creator),
        name: None,
        notes: legacy
            .get("full_text")
            .and_then(Value::as_str)
            .and_then(normalize_source_text),
        created_at: legacy
            .get("created_at")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(resume_cursor_after),
    })
}

fn tweet_stable_id(raw: &Value) -> Option<&str> {
    let tweet = unwrap_tweet(raw);
    tweet
        .get("rest_id")
        .and_then(Value::as_str)
        .or_else(|| tweet.pointer("/legacy/id_str").and_then(Value::as_str))
        .filter(|value| valid_numeric_token(value))
}

fn normalize_media(
    legacy: &Value,
    tweet_id: &str,
    canonical_url: &str,
) -> Result<Vec<crate::MediaDescriptor>, SourceError> {
    let entities = legacy
        .pointer("/extended_entities/media")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut media = Vec::new();
    let mut seen = BTreeSet::new();
    for entity in entities {
        let position = media.len() as u32;
        let media_id = entity
            .get("media_key")
            .or_else(|| entity.get("id_str"))
            .and_then(Value::as_str)
            .unwrap_or("media");
        let selected = if entity.get("video_info").is_some() {
            best_video(&entity).map(|(url, extension)| (url, extension, Vec::new()))
        } else {
            image_media(&entity)
        };
        let Some((url, extension, fallbacks)) = selected else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let mut headers = BTreeMap::new();
        headers.insert("Referer".to_string(), canonical_url.to_string());
        let file_name = format!("twitter_{tweet_id}_{position}.{extension}");
        let mut builder =
            MediaDescriptorBuilder::new(format!("twitter:{tweet_id}:{media_id}"), position, url)
                .canonical_url(canonical_url)
                .file_name(&file_name)
                .headers(headers);
        for fallback in fallbacks {
            builder = builder.fallback(MediaFallback {
                url: fallback,
                file_name: Some(file_name.clone()),
                mime_hint: mime_guess::from_path(&file_name)
                    .first_raw()
                    .map(ToOwned::to_owned),
                expected_size: None,
                html_marker: None,
            });
        }
        media.push(builder.build());
    }
    Ok(media)
}

fn image_media(entity: &Value) -> Option<(String, String, Vec<String>)> {
    let raw = entity.get("media_url_https")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let extension = Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()?
        .rsplit_once('.')?
        .1
        .to_ascii_lowercase();
    let base = raw.strip_suffix(&format!(".{extension}")).unwrap_or(raw);
    let original = format!("{base}?format={extension}&name=orig");
    let fallbacks = ["4096x4096", "large", "medium", "small"]
        .into_iter()
        .map(|size| format!("{base}?format={extension}&name={size}"))
        .collect();
    Some((original, extension, fallbacks))
}

fn best_video(entity: &Value) -> Option<(String, String)> {
    entity
        .pointer("/video_info/variants")?
        .as_array()?
        .iter()
        .filter(|variant| variant.get("content_type").and_then(Value::as_str) == Some("video/mp4"))
        .filter_map(|variant| {
            let url = variant.get("url")?.as_str()?.trim();
            (!url.is_empty()).then(|| {
                (
                    variant.get("bitrate").and_then(Value::as_u64).unwrap_or(0),
                    url.to_string(),
                )
            })
        })
        .max_by_key(|(bitrate, _)| *bitrate)
        .map(|(_, url)| (url, "mp4".to_string()))
}

fn parse_user_id(response: &Value) -> Result<String, SourceError> {
    reject_api_errors(response)?;
    response
        .pointer("/data/user/result/rest_id")
        .or_else(|| response.pointer("/data/user/result/result/rest_id"))
        .and_then(Value::as_str)
        .filter(|value| valid_numeric_token(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_response("Twitter / X account was not found"))
}

fn reject_api_errors(response: &Value) -> Result<(), SourceError> {
    let Some(errors) = response.get("errors").and_then(Value::as_array) else {
        return Ok(());
    };
    let messages = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let message = messages
        .first()
        .copied()
        .unwrap_or("Twitter / X API rejected the request");
    let lower = message.to_ascii_lowercase();
    if lower.contains("rate limit") || lower.contains("too many requests") {
        return Err(SourceError::new(
            SourceErrorKind::RateLimited,
            message,
            true,
        ));
    }
    if lower.contains("authenticate")
        || lower.contains("authorization")
        || lower.contains("not authorized")
        || lower.contains("login required")
        || lower.contains("temporarily locked")
    {
        return Err(SourceError::new(
            SourceErrorKind::Authentication,
            message,
            false,
        ));
    }
    let server_error = errors.iter().any(|error| {
        error.get("source").and_then(Value::as_str) == Some("Server")
            || error
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.to_ascii_lowercase().starts_with("timeout"))
    });
    Err(SourceError::new(
        if server_error {
            SourceErrorKind::Network
        } else {
            SourceErrorKind::InvalidResponse
        },
        message,
        true,
    ))
}

fn find_instructions(value: &Value) -> Option<&Vec<Value>> {
    match value {
        Value::Object(object) => {
            if let Some(instructions) = object.get("instructions").and_then(Value::as_array) {
                return Some(instructions);
            }
            object.values().find_map(find_instructions)
        }
        Value::Array(values) => values.iter().find_map(find_instructions),
        _ => None,
    }
}

fn tweet_results(entry: &Value, tweets: &mut Vec<Value>) {
    if let Some(tweet) = entry
        .pointer("/content/itemContent/tweet_results/result")
        .or_else(|| entry.pointer("/content/itemContent/tweet_results"))
        .or_else(|| entry.pointer("/item/itemContent/tweet_results/result"))
        .or_else(|| entry.pointer("/item/itemContent/tweet_results"))
    {
        push_tweet_result(tweets, tweet);
    }
    if let Some(items) = entry.pointer("/content/items").and_then(Value::as_array) {
        for item in items {
            if let Some(tweet) = item
                .pointer("/item/itemContent/tweet_results/result")
                .or_else(|| item.pointer("/item/itemContent/tweet_results"))
            {
                push_tweet_result(tweets, tweet);
            }
        }
    }
}

fn push_tweet_result(tweets: &mut Vec<Value>, tweet: &Value) {
    if unwrap_tweet(tweet).get("legacy").is_some() {
        tweets.push(tweet.clone());
    }
}

struct TimelineCursor {
    value: String,
    stop_on_empty: bool,
}

fn bottom_cursor(entry: &Value) -> Option<TimelineCursor> {
    let content = entry.get("content").or_else(|| entry.get("item"))?;
    let content = content.get("itemContent").unwrap_or(content);
    let cursor_type = content
        .get("cursorType")
        .or_else(|| content.get("cursor_type"))
        .and_then(Value::as_str);
    let entry_is_bottom = entry
        .get("entryId")
        .and_then(Value::as_str)
        .is_some_and(|id| id.starts_with("cursor-bottom-"));
    if cursor_type != Some("Bottom") && !entry_is_bottom {
        return None;
    }
    let value = content
        .get("value")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)?;
    Some(TimelineCursor {
        value,
        stop_on_empty: content
            .get("stopOnEmptyResponse")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn unwrap_tweet(mut value: &Value) -> &Value {
    for _ in 0..3 {
        if let Some(tweet) = value.get("tweet") {
            value = tweet;
        } else if value.get("legacy").is_none() {
            if let Some(result) = value.get("result") {
                value = result;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    value
}

fn normalize_username(raw: &str) -> Result<String, SourceError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_query(
            "Twitter / X subscriptions require a username",
        ));
    }
    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("x.com" | "www.x.com" | "twitter.com" | "www.twitter.com")
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "Twitter / X subscriptions require a canonical profile URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [username] | [username, "media"] => username.trim_start_matches('@').to_string(),
            _ => {
                return Err(invalid_query(
                    "Twitter / X subscriptions require a profile or media URL",
                ))
            }
        }
    } else {
        trimmed.trim_start_matches('@').to_string()
    };
    if username.is_empty()
        || username.len() > 15
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(invalid_query(
            "Twitter / X subscriptions require a valid username",
        ));
    }
    Ok(username)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CursorState {
    user_id: String,
    timeline_cursor: Option<String>,
    page_index: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_tweet_id: Option<String>,
}

fn encode_cursor(state: &CursorState) -> Result<String, SourceError> {
    validate_cursor_state(state)?;
    let raw = serde_json::to_string(state)
        .map_err(|error| invalid_response(format!("invalid Twitter cursor: {error}")))?;
    CURSOR.validate(&raw)?;
    Ok(raw)
}

fn decode_cursor(raw: &str) -> Result<CursorState, SourceError> {
    let raw = CURSOR.validate(raw)?;
    let state = serde_json::from_str::<CursorState>(raw).map_err(|_| invalid_cursor())?;
    validate_cursor_state(&state)?;
    Ok(state)
}

fn validate_cursor_state(state: &CursorState) -> Result<(), SourceError> {
    if !valid_numeric_token(&state.user_id)
        || state.page_index > 100
        || state
            .next_tweet_id
            .as_deref()
            .is_some_and(|value| !valid_numeric_token(value))
        || state.timeline_cursor.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control)
        })
    {
        return Err(invalid_cursor());
    }
    Ok(())
}

fn valid_numeric_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn invalid_query(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidQuery, message, false)
}

fn invalid_response(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

fn invalid_cursor() -> SourceError {
    SourceError::new(
        SourceErrorKind::InvalidQuery,
        "invalid source cursor",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const USER: &str = include_str!("../../tests/fixtures/twitter/user.json");
    const MEDIA: &str = include_str!("../../tests/fixtures/twitter/media.json");

    fn request(cursor: Option<String>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "OpenAI".to_string(),
            partition: SourcePartition::new("media"),
            cursor,
            page_size: 1,
        }
    }

    #[test]
    fn requires_authentication_and_uses_the_freshest_csrf_cookie() {
        let missing = api_credentials_with_csrf(&RequestCredentials::default(), None).unwrap_err();
        assert_eq!(missing.kind, SourceErrorKind::Authentication);

        let mut incomplete = RequestCredentials::default();
        incomplete
            .cookies
            .insert("auth_token".to_string(), "session".to_string());
        let generated = api_credentials_with_csrf(&incomplete, None).unwrap();
        let generated_csrf = generated.cookies.get("ct0").unwrap();
        assert_eq!(generated_csrf.len(), 32);
        assert_eq!(generated.headers.get("x-csrf-token"), Some(generated_csrf));

        incomplete
            .cookies
            .insert("ct0".to_string(), "stale".to_string());
        let complete = api_credentials_with_csrf(&incomplete, Some("rotated")).unwrap();
        assert_eq!(
            complete.cookies.get("ct0").map(String::as_str),
            Some("rotated")
        );
        assert_eq!(
            complete.headers.get("x-csrf-token").map(String::as_str),
            Some("rotated")
        );
        assert_eq!(
            complete
                .headers
                .get("x-twitter-auth-type")
                .map(String::as_str),
            Some("OAuth2Session")
        );
        assert_eq!(
            complete.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            complete.headers.get("Sec-Fetch-Dest").map(String::as_str),
            Some("empty")
        );
        assert_eq!(
            complete.headers.get("Sec-Fetch-Mode").map(String::as_str),
            Some("cors")
        );
        assert_eq!(
            complete.headers.get("Sec-Fetch-Site").map(String::as_str),
            Some("same-origin")
        );
    }

    #[test]
    fn transaction_input_includes_the_exact_graphql_path_and_query() {
        let url = user_lookup_url("OpenAI");
        let path = transaction_path(&url);
        assert!(path.starts_with("/i/api/graphql/"));
        assert!(path.contains("?variables="));
        assert!(path.contains("&features="));
        assert!(!path.contains("x.com"));
    }

    #[test]
    fn transaction_script_supports_the_current_indirect_bundle_mapping() {
        let home = r#"before,"bundleKey":"ondemand.s",after "bundleKey":"abc123" after"#;
        assert_eq!(
            transaction_script_url(home).unwrap(),
            "https://abs.twimg.com/responsive-web/client-web/ondemand.s.abc123a.js"
        );
    }

    #[test]
    fn accepts_existing_username_profile_and_media_forms() {
        for value in [
            "@OpenAI",
            "https://x.com/OpenAI",
            "https://twitter.com/OpenAI/media/",
        ] {
            assert_eq!(normalize_username(value).unwrap(), "OpenAI");
        }
        assert!(normalize_username("https://x.com/OpenAI/status/1").is_err());
        assert!(normalize_username("https://x.com.evil.test/OpenAI").is_err());
    }

    #[test]
    fn maps_user_identity_and_one_complete_media_post() {
        let user: Value = serde_json::from_str(USER).unwrap();
        assert_eq!(parse_user_id(&user).unwrap(), "783214");

        let media: Value = serde_json::from_str(MEDIA).unwrap();
        let batch = normalize_timeline(&request(None), "OpenAI", "783214", None, &media).unwrap();
        assert_eq!(batch.posts.len(), 1);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "1960123456789012345");
        assert_eq!(post.media.len(), 2);
        assert!(post.tags.contains(&CanonicalTag::new("creator", "OpenAI")));
        assert!(post.tags.contains(&CanonicalTag::new("", "Picto")));
        assert_eq!(post.media[0].fallbacks.len(), 4);
        assert!(post.media[0].fallbacks[0]
            .url
            .ends_with("?format=jpg&name=4096x4096"));
        assert_eq!(post.media[1].mime_hint.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn cursor_preserves_only_bounded_provider_state() {
        let state = CursorState {
            user_id: "783214".to_string(),
            timeline_cursor: Some("DAABCgAB".to_string()),
            page_index: 4,
            next_tweet_id: Some("1960123456789012345".to_string()),
        };
        let encoded = encode_cursor(&state).unwrap();
        assert_eq!(decode_cursor(&encoded).unwrap(), state);
        let legacy =
            decode_cursor(r#"{"user_id":"783214","timeline_cursor":null,"page_index":4}"#).unwrap();
        assert_eq!(legacy.page_index, 4);
        assert!(legacy.next_tweet_id.is_none());
        assert!(decode_cursor("{\"user_id\":\"bad\"}").is_err());
    }

    #[test]
    fn bounded_page_exposes_only_one_post_and_persists_the_next_index() {
        let mut response: Value = serde_json::from_str(MEDIA).unwrap();
        let entries = response
            .pointer_mut("/data/user/result/timeline_v2/timeline/instructions/0/entries")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let mut second = entries[0].clone();
        second["content"]["itemContent"]["tweet_results"]["result"]["rest_id"] =
            Value::String("1960123456789012344".into());
        second["content"]["itemContent"]["tweet_results"]["result"]["legacy"]["id_str"] =
            Value::String("1960123456789012344".into());
        entries.insert(1, second);
        let batch =
            normalize_timeline(&request(None), "OpenAI", "783214", None, &response).unwrap();
        assert_eq!(batch.posts.len(), 1);
        let state = decode_cursor(batch.posts[0].resume_cursor_after.as_deref().unwrap()).unwrap();
        assert_eq!(state.page_index, 1);
        assert!(state.timeline_cursor.is_none());
        assert_eq!(state.next_tweet_id.as_deref(), Some("1960123456789012344"));
    }

    #[test]
    fn stable_cursor_survives_timeline_entries_moving_between_requests() {
        let mut response: Value = serde_json::from_str(MEDIA).unwrap();
        let entries = response
            .pointer_mut("/data/user/result/timeline_v2/timeline/instructions/0/entries")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let original_first = entries[0].clone();
        let mut original_second = original_first.clone();
        original_second["content"]["itemContent"]["tweet_results"]["result"]["rest_id"] =
            Value::String("1960123456789012344".into());
        original_second["content"]["itemContent"]["tweet_results"]["result"]["legacy"]["id_str"] =
            Value::String("1960123456789012344".into());
        entries.insert(1, original_second);

        let first =
            normalize_timeline(&request(None), "OpenAI", "783214", None, &response).unwrap();
        let state = decode_cursor(first.posts[0].resume_cursor_after.as_deref().unwrap()).unwrap();

        let entries = response
            .pointer_mut("/data/user/result/timeline_v2/timeline/instructions/0/entries")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let mut newly_inserted = original_first;
        newly_inserted["content"]["itemContent"]["tweet_results"]["result"]["rest_id"] =
            Value::String("1960123456789012346".into());
        newly_inserted["content"]["itemContent"]["tweet_results"]["result"]["legacy"]["id_str"] =
            Value::String("1960123456789012346".into());
        entries.insert(0, newly_inserted);

        let resumed =
            normalize_timeline(&request(None), "OpenAI", "783214", Some(&state), &response)
                .unwrap();
        assert_eq!(resumed.posts[0].stable_id, "1960123456789012344");
    }

    #[test]
    fn flattens_every_tweet_from_add_entries_and_module_instructions() {
        let mut response: Value = serde_json::from_str(MEDIA).unwrap();
        let instructions = response
            .pointer_mut("/data/user/result/timeline_v2/timeline/instructions")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let result = instructions[0]
            .pointer("/entries/0/content/itemContent/tweet_results/result")
            .unwrap()
            .clone();
        let mut second = result.clone();
        second["rest_id"] = Value::String("1960123456789012346".into());
        second["legacy"]["id_str"] = Value::String("1960123456789012346".into());
        let mut third = result;
        third["rest_id"] = Value::String("1960123456789012347".into());
        third["legacy"]["id_str"] = Value::String("1960123456789012347".into());
        instructions.push(json!({
            "type": "TimelineAddToModule",
            "moduleItems": [
                {"item": {"itemContent": {"tweet_results": {"result": second}}}},
                {"item": {"itemContent": {"tweet_results": {"result": third}}}},
                {"item": {"itemContent": {"tweet_results": {"result": {
                    "__typename": "TweetTombstone",
                    "tombstone": {"text": {"text": "Unavailable"}}
                }}}}}
            ]
        }));

        let page = parse_timeline(&response).unwrap();
        assert_eq!(page.tweets.len(), 3);
        let state = CursorState {
            user_id: "783214".into(),
            timeline_cursor: None,
            page_index: 2,
            next_tweet_id: None,
        };
        let batch = normalize_timeline_page(&request(None), "OpenAI", "783214", Some(&state), page)
            .unwrap();
        assert_eq!(batch.posts[0].stable_id, "1960123456789012347");
    }

    #[test]
    fn empty_page_continues_only_when_the_bottom_cursor_requires_it() {
        let response = json!({
            "data": {"timeline": {"instructions": [{
                "type": "TimelineAddEntries",
                "entries": [{
                    "entryId": "cursor-bottom-1",
                    "content": {
                        "cursorType": "Bottom",
                        "value": "NEXT",
                        "stopOnEmptyResponse": false
                    }
                }]
            }]}}
        });
        let page = parse_timeline(&response).unwrap();
        assert!(page.tweets.is_empty());
        assert!(page.continue_on_empty);
        assert_eq!(page.bottom_cursor.as_deref(), Some("NEXT"));
        let batch =
            normalize_timeline(&request(None), "OpenAI", "783214", None, &response).unwrap();
        assert!(batch.posts.is_empty());
        assert!(!batch.exhausted);
    }

    #[test]
    fn classifies_graphql_errors_without_turning_transients_into_auth_failures() {
        let timeout = reject_api_errors(&json!({
            "errors": [{"message": "Timeout while fetching timeline"}]
        }))
        .unwrap_err();
        assert_eq!(timeout.kind, SourceErrorKind::Network);
        assert!(timeout.retryable);

        let server = reject_api_errors(&json!({
            "errors": [{"message": "Backend unavailable", "source": "Server"}]
        }))
        .unwrap_err();
        assert_eq!(server.kind, SourceErrorKind::Network);
        assert!(server.retryable);

        let auth = reject_api_errors(&json!({
            "errors": [{"message": "Could not authenticate you"}]
        }))
        .unwrap_err();
        assert_eq!(auth.kind, SourceErrorKind::Authentication);
        assert!(!auth.retryable);
    }
}
