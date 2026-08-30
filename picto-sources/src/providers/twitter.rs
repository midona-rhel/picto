use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const DOMAIN: &str = "x.com";
const CURSOR: OpaqueCursor = OpaqueCursor::new(2_048);
const PUBLIC_BEARER: &str = "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const USER_LOOKUP_OPERATION: &str = "ck5KkZ8t5cOmoLssopN99Q/UserByScreenName";
const USER_MEDIA_OPERATION: &str = "jCRhbOzdgOHp6u9H4g2tEg/UserMedia";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    TwitterSource
}

struct TwitterSource;

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
            let state = request
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

            let api_credentials = api_credentials(credentials)?;
            let user_id = match state.as_ref() {
                Some(state) => state.user_id.clone(),
                None => {
                    let response = http
                        .get_json::<Value>(user_lookup_url(&username), &api_credentials, cancel)
                        .await?;
                    parse_user_id(&response)?
                }
            };
            let response = http
                .get_json::<Value>(
                    user_timeline_url(
                        &user_id,
                        state
                            .as_ref()
                            .and_then(|state| state.timeline_cursor.as_deref()),
                    ),
                    &api_credentials,
                    cancel,
                )
                .await?;
            normalize_timeline(request, &username, &user_id, state.as_ref(), &response)
        })
    }
}

fn api_credentials(credentials: &RequestCredentials) -> Result<RequestCredentials, SourceError> {
    let mut api = credentials.clone();
    api.allowed_domains.insert(DOMAIN.to_string());
    api.headers
        .insert("Authorization".to_string(), PUBLIC_BEARER.to_string());
    api.headers.insert("Accept".to_string(), "*/*".to_string());
    api.headers
        .insert("Referer".to_string(), "https://x.com/".to_string());
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
    let csrf = api
        .cookies
        .get("ct0")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::Authentication,
                "Twitter / X session is missing its CSRF cookie",
                false,
            )
        })?;
    api.headers
        .insert("x-csrf-token".to_string(), csrf.to_string());
    api.headers.insert(
        "x-twitter-auth-type".to_string(),
        "OAuth2Session".to_string(),
    );
    Ok(api)
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

fn normalize_timeline(
    request: &DiscoveryRequest,
    query_username: &str,
    user_id: &str,
    state: Option<&CursorState>,
    response: &Value,
) -> Result<DiscoveryBatch, SourceError> {
    reject_api_errors(response)?;
    let entries = find_entries(response).ok_or_else(|| {
        invalid_response("Twitter / X media response is missing timeline entries")
    })?;
    let tweets = entries
        .iter()
        .filter_map(direct_tweet_result)
        .cloned()
        .collect::<Vec<_>>();
    let page_index = state.map_or(0, |state| state.page_index as usize);
    if page_index >= tweets.len() && (page_index != 0 || !tweets.is_empty()) {
        return Err(invalid_response(
            "Twitter / X timeline changed before its persisted page index",
        ));
    }
    let bottom_cursor = entries.iter().find_map(bottom_cursor);
    let exhausted = tweets.is_empty();
    let posts = tweets
        .get(page_index)
        .cloned()
        .map(|tweet| {
            let (timeline_cursor, page_index) = if page_index + 1 < tweets.len() {
                (
                    state.and_then(|state| state.timeline_cursor.clone()),
                    (page_index + 1) as u16,
                )
            } else {
                (bottom_cursor, 0)
            };
            normalize_tweet(
                request,
                query_username,
                tweet,
                encode_cursor(&CursorState {
                    user_id: user_id.to_string(),
                    timeline_cursor,
                    page_index,
                })?,
            )
        })
        .transpose()?
        .into_iter()
        .collect();
    Ok(DiscoveryBatch { posts, exhausted })
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
            best_video(&entity)
        } else {
            image_media(&entity)
        };
        let Some((url, extension)) = selected else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let mut headers = BTreeMap::new();
        headers.insert("Referer".to_string(), canonical_url.to_string());
        media.push(
            MediaDescriptorBuilder::new(format!("twitter:{tweet_id}:{media_id}"), position, url)
                .canonical_url(canonical_url)
                .file_name(format!("twitter_{tweet_id}_{position}.{extension}"))
                .headers(headers)
                .build(),
        );
    }
    Ok(media)
}

fn image_media(entity: &Value) -> Option<(String, String)> {
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
    Some((format!("{base}?format={extension}&name=orig"), extension))
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
    let message = errors
        .first()
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Twitter / X API rejected the request");
    Err(SourceError::new(
        SourceErrorKind::Authentication,
        message,
        false,
    ))
}

fn find_entries(value: &Value) -> Option<&Vec<Value>> {
    match value {
        Value::Object(object) => {
            if let Some(entries) = object.get("entries").and_then(Value::as_array) {
                return Some(entries);
            }
            object.values().find_map(find_entries)
        }
        Value::Array(values) => values.iter().find_map(find_entries),
        _ => None,
    }
}

fn direct_tweet_result(entry: &Value) -> Option<&Value> {
    entry
        .pointer("/content/itemContent/tweet_results/result")
        .or_else(|| entry.pointer("/content/itemContent/tweet_results"))
        .or_else(|| {
            entry
                .pointer("/content/items")?
                .as_array()?
                .iter()
                .find_map(|item| {
                    item.pointer("/item/itemContent/tweet_results/result")
                        .or_else(|| item.pointer("/item/itemContent/tweet_results"))
                })
        })
}

fn bottom_cursor(entry: &Value) -> Option<String> {
    let content = entry.get("content")?;
    let cursor_type = content
        .get("cursorType")
        .or_else(|| content.get("cursor_type"))?
        .as_str()?;
    if cursor_type != "Bottom" {
        return None;
    }
    content
        .get("value")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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
    fn requires_a_complete_direct_site_session_before_network_access() {
        let missing = api_credentials(&RequestCredentials::default()).unwrap_err();
        assert_eq!(missing.kind, SourceErrorKind::Authentication);

        let mut incomplete = RequestCredentials::default();
        incomplete
            .cookies
            .insert("auth_token".to_string(), "session".to_string());
        let missing_csrf = api_credentials(&incomplete).unwrap_err();
        assert_eq!(missing_csrf.kind, SourceErrorKind::Authentication);

        incomplete
            .cookies
            .insert("ct0".to_string(), "csrf".to_string());
        let complete = api_credentials(&incomplete).unwrap();
        assert_eq!(
            complete
                .headers
                .get("x-twitter-auth-type")
                .map(String::as_str),
            Some("OAuth2Session")
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
        assert_eq!(post.media[1].mime_hint.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn cursor_preserves_only_bounded_provider_state() {
        let state = CursorState {
            user_id: "783214".to_string(),
            timeline_cursor: Some("DAABCgAB".to_string()),
            page_index: 4,
        };
        let encoded = encode_cursor(&state).unwrap();
        assert_eq!(decode_cursor(&encoded).unwrap(), state);
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
        entries.insert(1, entries[0].clone());
        let batch =
            normalize_timeline(&request(None), "OpenAI", "783214", None, &response).unwrap();
        assert_eq!(batch.posts.len(), 1);
        let state = decode_cursor(batch.posts[0].resume_cursor_after.as_deref().unwrap()).unwrap();
        assert_eq!(state.page_index, 1);
        assert!(state.timeline_cursor.is_none());
    }
}
