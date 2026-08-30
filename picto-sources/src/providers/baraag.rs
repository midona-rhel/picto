use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    normalize_source_text, AdapterFuture, CanonicalTagSet, DiscoveryBatch, DiscoveryRequest,
    HttpRuntime, MediaDescriptorBuilder, NativeSourceAdapter, OpaqueCursor, ProviderDescriptor,
    RequestCredentials, SourceError, SourceErrorKind, SourcePost,
};

const CURSOR: OpaqueCursor = OpaqueCursor::new(160);
const DOMAIN: &str = "baraag.net";

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    BaraagSource
}

struct BaraagSource;

impl NativeSourceAdapter for BaraagSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "baraag",
            display_name: "Baraag",
            domain: DOMAIN,
            partitions: &["posts"],
            anonymous: true,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        normalize_account(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let username = normalize_account(&request.query)?;
            let credentials = api_credentials(credentials);
            let cursor = request
                .cursor
                .as_deref()
                .filter(|cursor| !cursor.is_empty())
                .map(decode_cursor)
                .transpose()?;
            let account_id = match cursor.as_ref() {
                Some(cursor) => cursor.account_id.clone(),
                None => account_id_by_username(&username, &credentials, http, cancel).await?,
            };
            let statuses = http
                .get_json::<Vec<ApiStatus>>(
                    statuses_url(&account_id, cursor.as_ref())?,
                    &credentials,
                    cancel,
                )
                .await?;
            normalize_statuses(request, &account_id, statuses)
        })
    }
}

fn api_credentials(credentials: &RequestCredentials) -> RequestCredentials {
    let mut credentials = credentials.clone();
    credentials.allowed_domains.insert(DOMAIN.to_string());
    let has_authorization = credentials
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization"));
    if !has_authorization {
        if let Some(token) = credentials
            .oauth_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            credentials
                .headers
                .insert("Authorization".to_string(), format!("Bearer {token}"));
        }
    }
    credentials
}

fn lookup_url(username: &str) -> Url {
    let mut url = Url::parse("https://baraag.net/api/v1/accounts/lookup")
        .expect("static Baraag account lookup URL");
    url.query_pairs_mut().append_pair("acct", username);
    url
}

fn search_url(username: &str) -> Url {
    let query = if username.contains('@') {
        format!("@{username}")
    } else {
        format!("@{username}@{DOMAIN}")
    };
    let mut url = Url::parse("https://baraag.net/api/v1/accounts/search")
        .expect("static Baraag account search URL");
    url.query_pairs_mut()
        .append_pair("q", &query)
        .append_pair("limit", "1");
    url
}

async fn account_id_by_username(
    username: &str,
    credentials: &RequestCredentials,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<String, SourceError> {
    if let Ok(account) = http
        .get_json::<ApiAccount>(lookup_url(username), credentials, cancel)
        .await
    {
        return validate_id(&account.id, "Baraag account").map(ToOwned::to_owned);
    }

    let accounts = http
        .get_json::<Vec<ApiAccount>>(search_url(username), credentials, cancel)
        .await?;
    accounts
        .into_iter()
        .find(|account| account.acct.eq_ignore_ascii_case(username))
        .ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::InvalidQuery,
                "Baraag account was not found",
                false,
            )
        })
        .and_then(|account| validate_id(&account.id, "Baraag account").map(ToOwned::to_owned))
}

fn statuses_url(account_id: &str, cursor: Option<&CursorState>) -> Result<Url, SourceError> {
    validate_id(account_id, "Baraag account")?;
    let mut url = Url::parse("https://baraag.net").expect("static Baraag URL");
    url.path_segments_mut()
        .expect("Baraag URL supports path segments")
        .extend(["api", "v1", "accounts", account_id, "statuses"]);
    {
        let mut query = url.query_pairs_mut();
        // The worker must settle this status before it can persist and use max_id.
        query.append_pair("limit", "1");
        query.append_pair("only_media", "true");
        query.append_pair("exclude_replies", "false");
        query.append_pair("exclude_reblogs", "true");
        if let Some(cursor) = cursor {
            query.append_pair("max_id", &cursor.status_id);
        }
    }
    Ok(url)
}

fn normalize_statuses(
    request: &DiscoveryRequest,
    account_id: &str,
    statuses: Vec<ApiStatus>,
) -> Result<DiscoveryBatch, SourceError> {
    if statuses.len() > 1 {
        return Err(invalid_response(
            "Baraag returned more than one status for a one-status request",
        ));
    }
    let exhausted = statuses.is_empty();
    let posts = statuses
        .into_iter()
        .map(|status| normalize_status(request, account_id, status))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DiscoveryBatch { posts, exhausted })
}

fn normalize_status(
    request: &DiscoveryRequest,
    account_id: &str,
    status: ApiStatus,
) -> Result<SourcePost, SourceError> {
    let status_id = validate_id(&status.id, "Baraag status")?.to_string();
    let creator = status.account.local_username()?;
    let canonical_url = status
        .url
        .filter(|url| is_canonical_status_url(url))
        .unwrap_or_else(|| format!("https://baraag.net/@{creator}/{status_id}"));
    let mut tags = CanonicalTagSet::default();
    tags.insert("creator", &creator);
    for tag in status.tags {
        insert_source_tag(&mut tags, &tag.name);
    }

    let media = status
        .media_attachments
        .into_iter()
        .enumerate()
        .map(|(position, attachment)| {
            let fallback_extension = attachment.extension();
            let Some(url) = attachment
                .url
                .filter(|url| !url.trim().is_empty())
                .or_else(|| attachment.remote_url.filter(|url| !url.trim().is_empty()))
            else {
                return Ok(None);
            };
            let attachment_id = validate_id(&attachment.id, "Baraag media attachment")?.to_string();
            let file_name = file_name_from_url(&url)
                .unwrap_or_else(|| format!("baraag_{status_id}_{position}.{fallback_extension}"));
            Ok(Some(
                MediaDescriptorBuilder::new(
                    format!("baraag:{status_id}:{attachment_id}"),
                    position as u32,
                    url,
                )
                .canonical_url(&canonical_url)
                .file_name(file_name)
                .build(),
            ))
        })
        .collect::<Result<Vec<_>, SourceError>>()?
        .into_iter()
        .flatten()
        .collect();

    Ok(SourcePost {
        site_id: "baraag".to_string(),
        partition: request.partition.clone(),
        stable_id: status_id.clone(),
        canonical_url: Some(canonical_url),
        creator: Some(creator),
        name: None,
        notes: normalize_source_text(&status.content),
        created_at: nonempty(status.created_at),
        tags: tags.into_vec(),
        media,
        resume_cursor_after: Some(encode_cursor(account_id, &status_id)?),
    })
}

fn normalize_account(raw: &str) -> Result<String, SourceError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_query("Baraag subscriptions require a username"));
    }
    let username = if let Ok(url) = Url::parse(trimmed) {
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str() != Some(DOMAIN)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_query(
                "Baraag subscriptions require a local profile URL",
            ));
        }
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        match segments.as_slice() {
            [profile] if profile.starts_with('@') => profile[1..].to_string(),
            [profile, "media"] if profile.starts_with('@') => profile[1..].to_string(),
            ["users", profile] => (*profile).to_string(),
            _ => {
                return Err(invalid_query(
                    "Baraag subscriptions require a local profile URL",
                ))
            }
        }
    } else {
        trimmed.strip_prefix('@').unwrap_or(trimmed).to_string()
    };
    if !valid_account_name(&username) {
        return Err(invalid_query(
            "Baraag subscriptions require a safe account handle",
        ));
    }
    Ok(username)
}

fn valid_account_name(value: &str) -> bool {
    let mut parts = value.split('@');
    let username = parts.next().unwrap_or_default();
    let domain = parts.next();
    if parts.next().is_some()
        || username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return false;
    }
    domain.is_none_or(valid_remote_domain)
}

fn valid_remote_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn insert_source_tag(tags: &mut CanonicalTagSet, raw: &str) {
    let raw = raw.trim().trim_start_matches('#');
    if let Some((namespace, value)) = raw.split_once(':') {
        if matches!(
            namespace.to_ascii_lowercase().as_str(),
            "creator" | "character" | "series" | "species" | "rating"
        ) {
            tags.insert(namespace.to_ascii_lowercase(), value);
            return;
        }
    }
    tags.insert("", raw);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CursorState {
    account_id: String,
    status_id: String,
}

fn encode_cursor(account_id: &str, status_id: &str) -> Result<String, SourceError> {
    validate_id(account_id, "Baraag account")?;
    validate_id(status_id, "Baraag status")?;
    let cursor = format!("a{account_id}:b{status_id}");
    CURSOR.validate(&cursor)?;
    Ok(cursor)
}

fn decode_cursor(raw: &str) -> Result<CursorState, SourceError> {
    let raw = CURSOR.validate(raw)?;
    let (account_id, status_id) = raw
        .strip_prefix('a')
        .and_then(|cursor| cursor.split_once(":b"))
        .ok_or_else(invalid_cursor)?;
    validate_id(account_id, "Baraag account").map_err(|_| invalid_cursor())?;
    validate_id(status_id, "Baraag status").map_err(|_| invalid_cursor())?;
    Ok(CursorState {
        account_id: account_id.to_string(),
        status_id: status_id.to_string(),
    })
}

fn validate_id<'a>(value: &'a str, subject: &str) -> Result<&'a str, SourceError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_response(format!(
            "{subject} response contained an invalid ID"
        )));
    }
    Ok(value)
}

fn is_canonical_status_url(raw: &str) -> bool {
    Url::parse(raw)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str() == Some(DOMAIN))
}

fn file_name_from_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()?
        .path_segments()?
        .next_back()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
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

#[derive(Debug, Deserialize)]
struct ApiAccount {
    id: String,
    #[serde(default)]
    acct: String,
}

#[derive(Debug, Deserialize)]
struct StatusAccount {
    #[serde(default)]
    username: String,
    #[serde(default)]
    acct: String,
}

impl StatusAccount {
    fn local_username(&self) -> Result<String, SourceError> {
        let username = self.username.trim();
        if !username.is_empty() {
            return Ok(username.to_string());
        }
        self.acct
            .trim()
            .split('@')
            .next()
            .filter(|username| !username.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| invalid_response("Baraag status is missing its account identity"))
    }
}

#[derive(Debug, Deserialize)]
struct ApiTag {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ApiAttachment {
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    remote_url: Option<String>,
}

impl ApiAttachment {
    fn extension(&self) -> &'static str {
        match self.kind.as_str() {
            "image" => "jpg",
            "gifv" | "video" => "mp4",
            "audio" => "m4a",
            _ => "media",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiStatus {
    id: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    content: String,
    #[serde(default)]
    url: Option<String>,
    account: StatusAccount,
    #[serde(default)]
    tags: Vec<ApiTag>,
    #[serde(default)]
    media_attachments: Vec<ApiAttachment>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{CanonicalTag, SourcePartition};

    const ACCOUNT: &str = include_str!("../../tests/fixtures/baraag/account.json");
    const STATUSES: &str = include_str!("../../tests/fixtures/baraag/statuses.json");

    fn request(cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "Blue_".to_string(),
            partition: SourcePartition::new("posts"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 32,
        }
    }

    #[test]
    fn accepts_current_ui_account_forms_and_rejects_cross_site_or_deep_urls() {
        for query in [
            "Blue_",
            "@Blue_",
            "Blue_@remote.example",
            "https://baraag.net/@Blue_",
            "https://baraag.net/@Blue_@remote.example",
            "https://baraag.net/@Blue_/media",
            "https://baraag.net/users/Blue_",
        ] {
            let expected = if query.contains("remote.example") {
                "Blue_@remote.example"
            } else {
                "Blue_"
            };
            assert_eq!(normalize_account(query).unwrap(), expected);
        }
        for query in [
            "",
            "https://example.com/@Blue_",
            "https://baraag.net/@Blue_/123",
            "https://baraag.net/@Blue_?page=2",
            "bad-name",
            "Blue_@bad_domain.example",
            "Blue_@@remote.example",
        ] {
            assert!(normalize_account(query).is_err(), "accepted {query}");
        }
    }

    #[test]
    fn maps_status_media_text_creator_and_canonical_or_general_tags() {
        let account: ApiAccount = serde_json::from_str(ACCOUNT).unwrap();
        let statuses: Vec<ApiStatus> = serde_json::from_str(STATUSES).unwrap();
        let batch = normalize_statuses(&request(None), &account.id, statuses).unwrap();
        assert!(!batch.exhausted);
        assert_eq!(batch.posts.len(), 1);
        let post = &batch.posts[0];
        assert_eq!(post.stable_id, "117023587543794517");
        assert_eq!(post.creator.as_deref(), Some("Blue_"));
        assert_eq!(post.notes.as_deref(), Some("A readable post & note."));
        assert_eq!(post.media.len(), 2);
        assert_eq!(post.media[0].position, 0);
        assert_eq!(post.media[0].file_name.as_deref(), Some("image.jpg"));
        assert_eq!(post.media[1].mime_hint.as_deref(), Some("video/mp4"));
        assert!(post.tags.contains(&CanonicalTag::new("creator", "Blue_")));
        assert!(post.tags.contains(&CanonicalTag::new("", "doodlegirl")));
        assert!(post
            .tags
            .contains(&CanonicalTag::new("character", "wolf_girl")));
        assert_eq!(
            post.resume_cursor_after.as_deref(),
            Some("a109876543210:b117023587543794517")
        );
    }

    #[test]
    fn cursor_is_bounded_and_status_request_fetches_only_the_next_post() {
        let cursor = decode_cursor("a109876543210:b117023587543794517").unwrap();
        assert_eq!(cursor.account_id, "109876543210");
        assert_eq!(cursor.status_id, "117023587543794517");
        assert!(decode_cursor("117023587543794517").is_err());
        assert!(decode_cursor("a1:bnot-a-number").is_err());

        let url = statuses_url(&cursor.account_id, Some(&cursor)).unwrap();
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(query.get("limit").map(|value| value.as_ref()), Some("1"));
        assert_eq!(
            query.get("max_id").map(|value| value.as_ref()),
            Some("117023587543794517")
        );
        assert_eq!(
            query.get("exclude_replies").map(|value| value.as_ref()),
            Some("false")
        );
        assert_eq!(url.path(), "/api/v1/accounts/109876543210/statuses");
    }

    #[test]
    fn account_search_fallback_uses_federated_handles() {
        let local = search_url("Blue_");
        let local_query = local.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            local_query.get("q").map(|value| value.as_ref()),
            Some("@Blue_@baraag.net")
        );

        let remote = search_url("Blue_@remote.example");
        let remote_query = remote.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            remote_query.get("q").map(|value| value.as_ref()),
            Some("@Blue_@remote.example")
        );
    }

    #[test]
    fn oauth_access_token_is_scoped_to_baraag_api_requests() {
        let credentials = api_credentials(&RequestCredentials {
            oauth_token: Some("access-token".to_string()),
            ..RequestCredentials::default()
        });
        assert_eq!(
            credentials.headers.get("Authorization").map(String::as_str),
            Some("Bearer access-token")
        );
        assert!(credentials.permits("api.baraag.net"));
        assert!(!credentials.permits("example.com"));
    }

    #[test]
    fn empty_status_page_exhausts_without_inventing_a_post() {
        let batch = normalize_statuses(&request(None), "109876543210", Vec::new()).unwrap();
        assert!(batch.exhausted);
        assert!(batch.posts.is_empty());
    }
}
