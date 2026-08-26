use super::adapters::adapter_for_json;
use crate::subscriptions::source_adapter::ParsedMetadata;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use std::collections::HashSet;
use std::sync::OnceLock;

pub(super) fn canonical_metadata_category(category: &str) -> &str {
    match category {
        "pixivuser" => "pixiv",
        "gelbooru_v02" => "gelbooru",
        _ => category,
    }
}

fn value_text(value: &serde_json::Value) -> Option<String> {
    if let Some(value) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_i64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_u64() {
        return Some(value.to_string());
    }
    value.as_f64().map(|value| value.to_string())
}

fn field_text(json: &serde_json::Value, key: &str) -> Option<String> {
    json.get(key).and_then(value_text)
}

fn value_u32(value: &serde_json::Value) -> Option<u32> {
    value_text(value)?.parse().ok()
}

fn artstation_project_field_text(json: &serde_json::Value, key: &str) -> Option<String> {
    field_text(json, key).or_else(|| {
        json.get("project")
            .and_then(|project| field_text(project, key))
    })
}

fn artstation_content_page_info(
    json: &serde_json::Value,
    page_num: Option<u32>,
    page_count: Option<u32>,
) -> (Option<u32>, Option<u32>) {
    if canonical_metadata_category(&field_text(json, "category").unwrap_or_default())
        != "artstation"
    {
        return (page_num, page_count);
    }

    let Some(asset) = json.get("asset") else {
        return (page_num, page_count);
    };
    let Some(position) = asset.get("position").and_then(value_u32) else {
        return (page_num, page_count);
    };

    // ArtStation can include a presentation cover as position 0 in the
    // project asset list. gallery-dl does not emit it because it is not a
    // downloadable content asset. Its cover URL uses the dedicated /covers/
    // path, which lets us remove it from the content count without changing
    // counts for projects whose assets start at position 0.
    let has_project_cover = page_count.is_some_and(|count| count > 1)
        && field_text(json, "cover_url").is_some_and(|url| {
            url.split('?')
                .next()
                .is_some_and(|path| path.contains("/covers/"))
        });
    if !has_project_cover {
        return (page_num, page_count);
    }

    (
        (position > 0).then_some(position).or(page_num),
        page_count.map(|count| count.saturating_sub(1)),
    )
}

fn hentaifoundry_creator(json: &serde_json::Value) -> Option<String> {
    ["artist", "user"]
        .into_iter()
        .find_map(|field| field_text(json, field))
}

fn canonical_mastodon_status_url(json: &serde_json::Value, host: &str) -> Option<String> {
    ["url", "uri"].into_iter().find_map(|field| {
        let raw = field_text(json, field)?;
        let mut url = url::Url::parse(&raw).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str() != Some(host)
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        url.set_scheme("https").ok()?;
        url.set_query(None);
        url.set_fragment(None);
        Some(url.to_string())
    })
}

fn html_to_plain_text(raw: &str) -> Option<String> {
    static TAGS: OnceLock<regex::Regex> = OnceLock::new();
    static NUMERIC_ENTITIES: OnceLock<regex::Regex> = OnceLock::new();
    static NAMED_ENTITIES: OnceLock<regex::Regex> = OnceLock::new();
    static SPACE_BEFORE_PUNCTUATION: OnceLock<regex::Regex> = OnceLock::new();
    let tags = TAGS.get_or_init(|| regex::Regex::new(r"(?s)<[^>]*>").expect("valid HTML regex"));
    let numeric_entities = NUMERIC_ENTITIES
        .get_or_init(|| regex::Regex::new(r"&#(x?[0-9A-Fa-f]+);").expect("valid entity regex"));
    let named_entities = NAMED_ENTITIES.get_or_init(|| {
        regex::Regex::new(r"&([A-Za-z][A-Za-z0-9]+);").expect("valid entity regex")
    });
    let space_before_punctuation = SPACE_BEFORE_PUNCTUATION
        .get_or_init(|| regex::Regex::new(r"\s+([.,!?;:])").expect("valid punctuation regex"));
    let plain = tags
        .replace_all(raw, " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");
    let plain = named_entities
        .replace_all(&plain, |captures: &regex::Captures| {
            match &captures[1] {
                "nbsp" => " ",
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "quot" => "\"",
                "apos" | "lsquo" | "rsquo" => "'",
                "ldquo" | "rdquo" => "\"",
                "ndash" | "mdash" => "-",
                "hellip" => "...",
                "bull" => "*",
                "middot" => "·",
                "copy" => "©",
                "reg" => "®",
                "trade" => "™",
                "laquo" => "«",
                "raquo" => "»",
                _ => return captures[0].to_string(),
            }
            .to_string()
        })
        .into_owned();
    let plain = numeric_entities
        .replace_all(&plain, |captures: &regex::Captures| {
            let raw = &captures[1];
            let codepoint = raw
                .strip_prefix(['x', 'X'])
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| raw.parse::<u32>().ok());
            codepoint
                .and_then(char::from_u32)
                .map(|value| value.to_string())
                .unwrap_or_default()
        })
        .into_owned();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = space_before_punctuation
        .replace_all(&plain, "$1")
        .into_owned();
    (!plain.is_empty()).then_some(plain)
}

fn normalize_metadata_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('<') && trimmed.contains('>') {
        return html_to_plain_text(trimmed);
    }
    if trimmed.contains('&') && trimmed.contains(';') {
        return html_to_plain_text(trimmed);
    }
    Some(trimmed.to_string())
}

fn mastodon_description(json: &serde_json::Value) -> Option<String> {
    let raw = field_text(json, "content")?;
    html_to_plain_text(&raw)
}

fn deviantart_description(json: &serde_json::Value) -> Option<String> {
    let raw = field_text(json, "description")?;
    html_to_plain_text(&raw)
}

fn tumblr_description(json: &serde_json::Value) -> Option<String> {
    ["caption", "body", "summary"]
        .into_iter()
        .find_map(|field| field_text(json, field))
        .and_then(|raw| html_to_plain_text(&raw))
}

fn canonical_patreon_url(json: &serde_json::Value) -> Option<String> {
    for field in ["patreon_url", "url"] {
        let Some(raw) = field_text(json, field) else {
            continue;
        };
        let Ok(mut url) = url::Url::parse(raw.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(url.host_str(), Some("patreon.com" | "www.patreon.com"))
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            continue;
        }
        url.set_scheme("https").ok()?;
        url.set_host(Some("www.patreon.com")).ok()?;
        url.set_query(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }
    None
}

fn canonical_fanbox_url(json: &serde_json::Value, post_id: Option<&str>) -> Option<String> {
    for field in ["postUrl", "post_url", "url"] {
        let Some(raw) = field_text(json, field) else {
            continue;
        };
        let Ok(mut url) = url::Url::parse(raw.trim()) else {
            continue;
        };
        let host = url.host_str().unwrap_or_default();
        if !matches!(url.scheme(), "http" | "https")
            || !(host == "fanbox.cc" || host == "www.fanbox.cc" || host.ends_with(".fanbox.cc"))
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            continue;
        }
        url.set_scheme("https").ok()?;
        url.set_query(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }

    let creator = field_text(json, "creatorId")?;
    let post_id = post_id?;
    Some(format!("https://{creator}.fanbox.cc/posts/{post_id}"))
}

fn canonical_subscribestar_url(json: &serde_json::Value, post_id: Option<&str>) -> Option<String> {
    for field in ["post_url", "url"] {
        let Some(raw) = field_text(json, field) else {
            continue;
        };
        let Ok(mut url) = url::Url::parse(raw.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(url.host_str(), Some("subscribestar.art"))
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            continue;
        }
        url.set_scheme("https").ok()?;
        url.set_host(Some("subscribestar.art")).ok()?;
        url.set_query(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }

    post_id.map(|post_id| format!("https://subscribestar.art/posts/{post_id}"))
}

fn generic_creator_identifier(json: &serde_json::Value) -> Option<String> {
    for field in [
        "artist",
        "author",
        "author_name",
        "author_nick",
        "creatorId",
        "creator_id",
        "user",
        "username",
    ] {
        if let Some(value) = field_text(json, field) {
            return Some(value);
        }
    }

    for object_field in ["creator", "user", "userinfo", "artist"] {
        let Some(value) = json.get(object_field) else {
            continue;
        };
        for nested_field in ["slug", "username", "userId", "id", "name"] {
            if let Some(value) = value.get(nested_field).and_then(value_text) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

fn webtoons_identity(json: &serde_json::Value) -> Option<(String, String)> {
    let title_no = field_text(json, "title_no")?;
    let episode_no = field_text(json, "episode_no")?;
    if title_no.is_empty() || episode_no.is_empty() {
        return None;
    }
    Some((title_no, episode_no))
}

fn webtoons_episode_candidates(json: &serde_json::Value, key: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(parent) = json.get("_parent") {
        if let Some(value) = field_text(parent, key) {
            candidates.push(value);
        }
    }
    if let Some(value) = field_text(json, key) {
        candidates.push(value);
    }
    candidates
}

fn canonical_webtoons_episode_url(raw: &str, title_no: &str, episode_no: &str) -> Option<String> {
    let mut url = url::Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(url.host_str(), Some("webtoons.com" | "www.webtoons.com"))
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let last_segment = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .next_back();
    if last_segment != Some("viewer") {
        return None;
    }

    let pairs: Vec<_> = url.query_pairs().collect();
    let actual_title = pairs
        .iter()
        .find(|(key, _)| key == "title_no")
        .map(|(_, value)| value.as_ref());
    let actual_episode = pairs
        .iter()
        .find(|(key, _)| key == "episode_no")
        .map(|(_, value)| value.as_ref());
    if actual_title != Some(title_no) || actual_episode != Some(episode_no) {
        return None;
    }

    url.set_scheme("https").ok()?;
    url.set_host(Some("www.webtoons.com")).ok()?;
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_port(None).ok()?;
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("title_no", title_no)
        .append_pair("episode_no", episode_no);
    url.set_fragment(None);
    Some(url.to_string())
}

fn fallback_webtoons_episode_url(
    json: &serde_json::Value,
    title_no: &str,
    episode_no: &str,
) -> Option<String> {
    let lang = field_text(json, "lang")?;
    let genre = field_text(json, "genre")?;
    let comic = field_text(json, "comic")?;
    if lang.is_empty() || genre.is_empty() || comic.is_empty() {
        return None;
    }

    let mut url = url::Url::parse("https://www.webtoons.com").ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.push(&lang)
            .push(&genre)
            .push(&comic)
            .push("episode")
            .push("viewer");
    }
    url.query_pairs_mut()
        .append_pair("title_no", title_no)
        .append_pair("episode_no", episode_no);
    Some(url.to_string())
}

fn push_unique_url(urls: &mut Vec<String>, value: Option<String>) {
    let Some(value) = value else { return };
    if !urls.iter().any(|existing| existing == &value) {
        urls.push(value);
    }
}

fn normalize_created_at(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(parsed.with_timezone(&Utc).to_rfc3339());
    }

    for fmt in ["%Y-%m-%d %H:%M:%S%:z", "%Y-%m-%d %H:%M:%S%.f%:z"] {
        if let Ok(parsed) = DateTime::parse_from_str(trimmed, fmt) {
            return Some(parsed.with_timezone(&Utc).to_rfc3339());
        }
    }

    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
        }
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let parsed = parsed.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y%m%d") {
        let parsed = parsed.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc).to_rfc3339());
    }

    Some(trimmed.to_string())
}

fn parse_created_at(json: &serde_json::Value) -> Option<String> {
    for key in [
        "date",
        "created_at",
        "create_date",
        "published_at",
        "publishedDatetime",
        "published",
        "upload_date",
    ] {
        let Some(value) = json.get(key) else {
            continue;
        };
        if let Some(raw) = value.as_str() {
            if let Some(normalized) = normalize_created_at(raw) {
                return Some(normalized);
            }
        }
        if let Some(timestamp) = value.as_i64().or_else(|| value.as_f64().map(|v| v as i64)) {
            if let Some(date) = DateTime::from_timestamp(timestamp, 0) {
                return Some(date.to_rfc3339());
            }
        }
    }
    None
}

fn post_id(json: &serde_json::Value) -> Option<String> {
    match canonical_metadata_category(&field_text(json, "category").unwrap_or_default()) {
        "webtoons" => {
            let (title_no, episode_no) = webtoons_identity(json)?;
            Some(format!("{title_no}:{episode_no}"))
        }
        "artstation" => artstation_project_field_text(json, "hash_id")
            .or_else(|| artstation_project_field_text(json, "project_hash_id"))
            .or_else(|| artstation_project_field_text(json, "project_id")),
        "deviantart" => field_text(json, "deviationid"),
        "hentaifoundry" => field_text(json, "index").filter(|index| index.parse::<u64>().is_ok()),
        "newgrounds" => field_text(json, "index").filter(|index| index.parse::<u64>().is_ok()),
        "twitter" => field_text(json, "tweet_id").or_else(|| field_text(json, "id")),
        "subscribestar" => field_text(json, "post_id").or_else(|| field_text(json, "id")),
        _ => field_text(json, "id").or_else(|| field_text(json, "post_id")),
    }
}

fn canonical_deviantart_url(
    json: &serde_json::Value,
    post_id: Option<&str>,
    item_url: Option<&str>,
) -> Option<String> {
    for raw in [
        field_text(json, "url"),
        field_text(json, "post_url"),
        item_url.map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    {
        let Ok(mut url) = url::Url::parse(raw.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https")
            || !matches!(
                url.host_str(),
                Some("deviantart.com" | "www.deviantart.com")
            )
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            continue;
        }
        url.set_scheme("https").ok()?;
        url.set_host(Some("www.deviantart.com")).ok()?;
        url.set_query(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }

    post_id.map(|post_id| format!("https://www.deviantart.com/view/{post_id}"))
}

fn tumblr_blog_name(json: &serde_json::Value) -> Option<String> {
    let name = json
        .get("blog")
        .and_then(|blog| field_text(blog, "name"))
        .or_else(|| field_text(json, "blog_name"))?;
    (!name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then_some(name)
}

fn canonical_tumblr_url(json: &serde_json::Value, post_id: Option<&str>) -> Option<String> {
    for field in ["post_url", "permalink_url", "short_url"] {
        let Some(raw) = field_text(json, field) else {
            continue;
        };
        let Ok(mut url) = url::Url::parse(raw.trim()) else {
            continue;
        };
        let host = url.host_str().unwrap_or_default();
        if !matches!(url.scheme(), "http" | "https")
            || !(host == "tumblr.com" || host.ends_with(".tumblr.com"))
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
        {
            continue;
        }
        url.set_scheme("https").ok()?;
        url.set_query(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }

    let blog = tumblr_blog_name(json)?;
    let post_id = post_id?;
    let mut url = url::Url::parse("https://www.tumblr.com").ok()?;
    url.path_segments_mut().ok()?.push(&blog).push(post_id);
    Some(url.to_string())
}

fn canonical_newgrounds_url(json: &serde_json::Value) -> Option<String> {
    let raw = field_text(json, "post_url")?;
    let mut url = url::Url::parse(raw.trim()).ok()?;
    let host = url.host_str().unwrap_or_default();
    if !matches!(url.scheme(), "http" | "https")
        || !(host == "newgrounds.com"
            || host == "www.newgrounds.com"
            || host.ends_with(".newgrounds.com"))
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

fn canonical_post_url(
    json: &serde_json::Value,
    category: Option<&str>,
    post_id: Option<&str>,
    item_url: Option<&str>,
) -> Option<String> {
    match category {
        Some("webtoons") => {
            let (title_no, episode_no) = webtoons_identity(json)?;
            for candidate in ["episode_url", "viewer_url", "page_url", "url", "_url"] {
                for candidate in webtoons_episode_candidates(json, candidate) {
                    if let Some(url) =
                        canonical_webtoons_episode_url(&candidate, &title_no, &episode_no)
                    {
                        return Some(url);
                    }
                }
            }
            if let Some(url) =
                item_url.and_then(|url| canonical_webtoons_episode_url(url, &title_no, &episode_no))
            {
                return Some(url);
            }
            return fallback_webtoons_episode_url(json, &title_no, &episode_no);
        }
        Some("artstation") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://www.artstation.com/projects/{post_id}"));
            }
        }
        Some("deviantart") => return canonical_deviantart_url(json, post_id, item_url),
        Some("tumblr") => return canonical_tumblr_url(json, post_id),
        Some("newgrounds") => return canonical_newgrounds_url(json),
        Some("patreon") => return canonical_patreon_url(json),
        Some("fanbox") => return canonical_fanbox_url(json, post_id),
        Some("subscribestar") => return canonical_subscribestar_url(json, post_id),
        Some("furaffinity") => {
            let post_id = post_id?;
            return Some(format!("https://www.furaffinity.net/view/{post_id}/"));
        }
        Some("hentaifoundry") => {
            let creator = hentaifoundry_creator(json)?;
            let post_id = post_id?;
            let mut url = url::Url::parse("https://www.hentai-foundry.com").ok()?;
            url.path_segments_mut()
                .ok()?
                .push("pictures")
                .push("user")
                .push(&creator)
                .push(post_id)
                .push("picto");
            return Some(url.to_string());
        }
        Some("twitter") => {
            let post_id = post_id?;
            let username = json
                .get("author")
                .and_then(|author| author.get("name").or_else(|| author.get("nick")))
                .and_then(serde_json::Value::as_str)?;
            return Some(format!("https://x.com/{username}/status/{post_id}"));
        }
        Some("baraag") => return canonical_mastodon_status_url(json, "baraag.net"),
        Some("idolcomplex") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://www.idolcomplex.com/en/posts/{post_id}"));
            }
        }
        Some("sankaku") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://sankaku.app/posts/{post_id}"));
            }
        }
        Some("pixiv") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://www.pixiv.net/en/artworks/{post_id}"));
            }
        }
        Some("gelbooru") => {
            if let Some(post_id) = post_id {
                return Some(format!(
                    "https://gelbooru.com/index.php?page=post&s=view&id={post_id}"
                ));
            }
        }
        Some("rule34") => {
            if let Some(post_id) = post_id {
                return Some(format!(
                    "https://rule34.xxx/index.php?page=post&s=view&id={post_id}"
                ));
            }
        }
        Some("danbooru") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://danbooru.donmai.us/posts/{post_id}"));
            }
        }
        Some("yandere") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://yande.re/post/show/{post_id}"));
            }
        }
        Some("konachan") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://konachan.com/post/show/{post_id}"));
            }
        }
        Some("safebooru") => {
            if let Some(post_id) = post_id {
                return Some(format!(
                    "https://safebooru.org/index.php?page=post&s=view&id={post_id}"
                ));
            }
        }
        Some("e621") => {
            if let Some(post_id) = post_id {
                return Some(format!("https://e621.net/posts/{post_id}"));
            }
        }
        _ => {}
    }
    None
}

fn media_url(
    json: &serde_json::Value,
    category: Option<&str>,
    item_url: Option<&str>,
) -> Option<String> {
    if category == Some("deviantart") {
        return json
            .get("content")
            .and_then(|content| field_text(content, "src"))
            .or_else(|| field_text(json, "file_url"))
            .or_else(|| field_text(json, "media_url"));
    }
    if category == Some("artstation") {
        return item_url
            .map(str::trim)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(ToOwned::to_owned)
            .or_else(|| {
                json.get("asset")
                    .and_then(|asset| {
                        field_text(asset, "image_url").or_else(|| field_text(asset, "url"))
                    })
                    .or_else(|| field_text(json, "url"))
            });
    }
    if category == Some("hentaifoundry") {
        return ["src", "media", "file_url", "media_url"]
            .into_iter()
            .find_map(|field| {
                field_text(json, field)
                    .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            })
            .or_else(|| {
                item_url
                    .map(str::trim)
                    .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                    .map(ToOwned::to_owned)
            });
    }
    if category == Some("baraag") {
        return json
            .get("media")
            .and_then(|media| field_text(media, "url"))
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .or_else(|| {
                item_url
                    .map(str::trim)
                    .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                    .map(ToOwned::to_owned)
            });
    }
    field_text(json, "file_url")
        .or_else(|| field_text(json, "media_url"))
        .or_else(|| json.get("file").and_then(|file| field_text(file, "url")))
        .or_else(|| {
            (category == Some("pixiv"))
                .then(|| field_text(json, "url"))
                .flatten()
        })
        .or_else(|| {
            item_url
                .map(str::trim)
                .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                .map(ToOwned::to_owned)
        })
}

fn source_urls(
    json: &serde_json::Value,
    canonical_post_url: Option<String>,
    media_url: Option<String>,
) -> Vec<String> {
    let mut urls = Vec::new();
    push_unique_url(&mut urls, canonical_post_url);
    push_unique_url(&mut urls, field_text(json, "source"));
    if let Some(sources) = json.get("sources").and_then(|value| value.as_array()) {
        for source in sources {
            push_unique_url(&mut urls, value_text(source));
        }
    }
    push_unique_url(&mut urls, media_url);
    urls
}

/// Normalize raw gallery-dl metadata into Picto's one subscription metadata shape.
pub fn parse_metadata(json: &serde_json::Value) -> ParsedMetadata {
    parse_metadata_with_url(json, None)
}

pub(super) fn parse_metadata_with_url(
    json: &serde_json::Value,
    item_url: Option<&str>,
) -> ParsedMetadata {
    let category = field_text(json, "category")
        .map(|category| canonical_metadata_category(&category).to_string());
    let adapter = adapter_for_json(json);
    let mut tags = adapter
        .map(|adapter| adapter.parse_tags(json))
        .unwrap_or_default();
    let mut seen_tags = HashSet::with_capacity(tags.len());
    tags.retain(|tag| seen_tags.insert(tag.clone()));
    if let Some(creator) = adapter
        .and_then(|adapter| adapter.extract_creator_identifier(json))
        .or_else(|| generic_creator_identifier(json))
    {
        if !tags
            .iter()
            .any(|(namespace, subtag)| namespace == "creator" && subtag == &creator)
        {
            tags.push(("creator".to_string(), creator));
        }
    }

    let description = if category.as_deref() == Some("baraag") {
        mastodon_description(json)
    } else if category.as_deref() == Some("deviantart") {
        deviantart_description(json)
    } else if category.as_deref() == Some("tumblr") {
        tumblr_description(json)
    } else {
        json.get("artist_commentary")
            .and_then(|commentary| field_text(commentary, "original_description"))
            .or_else(|| {
                [
                    "description",
                    "caption",
                    "body",
                    "content",
                    "text",
                    "html",
                    "substring",
                ]
                .into_iter()
                .find_map(|key| field_text(json, key))
            })
            .and_then(|raw| normalize_metadata_text(&raw))
    };
    let title = json
        .get("artist_commentary")
        .and_then(|commentary| field_text(commentary, "original_title"))
        .or_else(|| {
            ["title", "subject"]
                .into_iter()
                .find_map(|key| field_text(json, key))
                .or_else(|| field_text(json, "episode_name"))
        })
        .or_else(|| {
            (category.as_deref() == Some("tumblr"))
                .then(|| field_text(json, "summary"))
                .flatten()
        })
        .and_then(|raw| normalize_metadata_text(&raw));
    let post_id = post_id(json);
    let canonical_post_url =
        canonical_post_url(json, category.as_deref(), post_id.as_deref(), item_url);
    let media_url = media_url(json, category.as_deref(), item_url);
    let source_urls = source_urls(json, canonical_post_url.clone(), media_url.clone());
    let source_url = canonical_post_url
        .clone()
        .or_else(|| source_urls.first().cloned());
    let raw_page_num = json.get("num").and_then(value_u32);
    let raw_page_count = json
        .get("count")
        .or_else(|| json.get("page_count"))
        .and_then(value_u32);
    let (page_num, page_count) = artstation_content_page_info(json, raw_page_num, raw_page_count);
    let item_target = post_id
        .as_deref()
        .or(canonical_post_url.as_deref())
        .or(media_url.as_deref())
        .or(item_url.map(str::trim).filter(|url| !url.is_empty()));
    let item_key = item_target.map(|target| {
        format!(
            "{}:{target}:{}",
            category.as_deref().unwrap_or("unknown"),
            page_num.unwrap_or(0)
        )
    });

    tracing::debug!(
        category = category.as_deref().unwrap_or("?"),
        post_id = post_id.as_deref().unwrap_or("?"),
        tags = tags.len(),
        page_num = ?page_num,
        page_count = ?page_count,
        "gallery-dl metadata normalized"
    );

    ParsedMetadata {
        tags,
        description,
        source_url,
        source_urls,
        media_url,
        rating: json.get("rating").and_then(value_text),
        title,
        post_id,
        created_at: parse_created_at(json),
        category,
        page_num,
        page_count,
        canonical_post_url,
        item_key,
        raw_metadata: Some(json.clone()),
    }
}

pub fn parse_tags(json: &serde_json::Value) -> Vec<(String, String)> {
    adapter_for_json(json)
        .map(|adapter| adapter.parse_tags(json))
        .unwrap_or_default()
}

pub fn extract_creator_identifier(json: &serde_json::Value) -> Option<String> {
    adapter_for_json(json)
        .and_then(|adapter| adapter.extract_creator_identifier(json))
        .or_else(|| generic_creator_identifier(json))
}

#[cfg(test)]
mod tests {
    use super::{parse_metadata, parse_metadata_with_url};
    use serde_json::json;

    #[test]
    fn gelbooru_metadata_uses_post_identity_namespaced_tags_and_source_order() {
        let parsed = parse_metadata(&json!({
            "category": "gelbooru_v02",
            "id": 13753751,
            "file_url": "https://img2.gelbooru.com/example.jpg",
            "source": "https://x.com/artist/status/1",
            "date": "2026-03-30T03:30:30+00:00",
            "created_at": "2026-03-29T22:30:30-05:00",
            "tags_character": "princess_peach",
            "tags_general": "dress",
            "tags_metadata": "highres"
        }));

        let post_url = "https://gelbooru.com/index.php?page=post&s=view&id=13753751";
        assert_eq!(parsed.category.as_deref(), Some("gelbooru"));
        assert_eq!(parsed.canonical_post_url.as_deref(), Some(post_url));
        assert_eq!(parsed.source_url.as_deref(), Some(post_url));
        assert_eq!(
            parsed.source_urls,
            [
                post_url,
                "https://x.com/artist/status/1",
                "https://img2.gelbooru.com/example.jpg"
            ]
        );
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-03-30T03:30:30+00:00")
        );
        assert!(parsed
            .tags
            .contains(&("metadata".to_string(), "highres".to_string())));
        assert!(parsed
            .tags
            .contains(&("character".to_string(), "princess_peach".to_string())));
    }

    #[test]
    fn pixiv_metadata_prefers_artwork_page_for_user_and_search_runs() {
        let raw = json!({
            "category": "pixiv",
            "id": 114223105,
            "url": "https://i.pximg.net/img-original/example_p0.png",
            "tags": ["original"],
            "user": {"id": 1234, "name": "Artist"}
        });
        for query_url in [
            "https://www.pixiv.net/en/users/1234/artworks",
            "https://www.pixiv.net/en/tags/original/artworks?s_mode=s_tag",
        ] {
            let parsed = parse_metadata_with_url(&raw, Some(query_url));
            let post_url = "https://www.pixiv.net/en/artworks/114223105";
            assert_eq!(parsed.source_url.as_deref(), Some(post_url));
            assert_eq!(
                parsed.source_urls,
                [post_url, "https://i.pximg.net/img-original/example_p0.png"]
            );
            assert_eq!(parsed.item_key.as_deref(), Some("pixiv:114223105:0"));
        }
    }

    #[test]
    fn booru_sources_use_their_own_post_page_as_identity() {
        for (category, expected) in [
            ("danbooru", "https://danbooru.donmai.us/posts/42"),
            (
                "gelbooru",
                "https://gelbooru.com/index.php?page=post&s=view&id=42",
            ),
            (
                "rule34",
                "https://rule34.xxx/index.php?page=post&s=view&id=42",
            ),
            ("yandere", "https://yande.re/post/show/42"),
            ("konachan", "https://konachan.com/post/show/42"),
            (
                "safebooru",
                "https://safebooru.org/index.php?page=post&s=view&id=42",
            ),
            ("e621", "https://e621.net/posts/42"),
        ] {
            let parsed = parse_metadata(&json!({
                "category": category,
                "id": 42,
                "source": "https://artist.example/original",
                "file_url": "https://cdn.example/file.jpg"
            }));
            assert_eq!(parsed.canonical_post_url.as_deref(), Some(expected));
            assert_eq!(
                parsed.source_urls,
                [
                    expected,
                    "https://artist.example/original",
                    "https://cdn.example/file.jpg"
                ]
            );
        }
    }

    #[test]
    fn public_booru_categories_preserve_categorized_tags_and_creator_identity() {
        let parsed = parse_metadata_with_url(
            &json!({
                "category": "e621",
                "id": 42,
                "file": {"url": "https://static1.e621.net/data/ab/cd/42.jpg"},
                "sources": ["https://artist.example/original"],
                "tags_artist": ["artist_name"],
                "tags_character": ["character_name"],
                "tags_general": ["solo"],
                "tags_species": ["canine"]
            }),
            Some("https://static1.e621.net/data/ab/cd/42.jpg"),
        );

        assert_eq!(parsed.category.as_deref(), Some("e621"));
        assert_eq!(parsed.post_id.as_deref(), Some("42"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://e621.net/posts/42")
        );
        assert_eq!(
            parsed.source_urls,
            [
                "https://e621.net/posts/42",
                "https://artist.example/original",
                "https://static1.e621.net/data/ab/cd/42.jpg"
            ]
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://static1.e621.net/data/ab/cd/42.jpg")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist_name".to_string())));
        assert!(parsed
            .tags
            .contains(&("character".to_string(), "character_name".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "solo".to_string())));
        assert!(parsed
            .tags
            .contains(&("species".to_string(), "canine".to_string())));
    }

    #[test]
    fn rule34_metadata_uses_actual_gallery_dl_fields() {
        let parsed = parse_metadata(&json!({
            "category": "rule34",
            "id": 987654,
            "file_url": "https://wimg.rule34.xxx/images/ab/cd/example.jpg",
            "source": "https://artist.example/post/987654",
            "created_at": "2026-01-02 03:04:05+00:00",
            "tags_artist": "artist_name",
            "tags_character": "character_name",
            "tags_copyright": "series_name",
            "tags_general": "solo highres",
            "tags_meta": "commentary"
        }));

        assert_eq!(parsed.category.as_deref(), Some("rule34"));
        assert_eq!(parsed.post_id.as_deref(), Some("987654"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://rule34.xxx/index.php?page=post&s=view&id=987654")
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://wimg.rule34.xxx/images/ab/cd/example.jpg")
        );
        assert_eq!(
            parsed.source_urls,
            [
                "https://rule34.xxx/index.php?page=post&s=view&id=987654",
                "https://artist.example/post/987654",
                "https://wimg.rule34.xxx/images/ab/cd/example.jpg"
            ]
        );
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-01-02T03:04:05+00:00")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist_name".to_string())));
        assert!(parsed
            .tags
            .contains(&("character".to_string(), "character_name".to_string())));
        assert!(parsed
            .tags
            .contains(&("copyright".to_string(), "series_name".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "solo".to_string())));
        assert!(parsed
            .tags
            .contains(&("meta".to_string(), "commentary".to_string())));
    }

    #[test]
    fn normalized_tags_remove_duplicates_across_modern_and_legacy_fields() {
        let parsed = parse_metadata(&json!({
            "category": "danbooru",
            "id": 42,
            "tags_artist": ["bonocho"],
            "tag_string_artist": "bonocho",
            "tags_general": ["solo"],
            "tag_string_general": "solo"
        }));

        assert_eq!(
            parsed.tags,
            [
                ("artist".to_string(), "bonocho".to_string()),
                (String::new(), "solo".to_string()),
                ("creator".to_string(), "bonocho".to_string()),
            ]
        );
    }

    #[test]
    fn baraag_metadata_preserves_status_identity_and_plain_text_content() {
        let parsed = parse_metadata(&json!({
            "category": "baraag",
            "subcategory": "user",
            "id": "117023587543794517",
            "url": "https://baraag.net/@Blue_/117023587543794517",
            "uri": "https://baraag.net/users/Blue_/statuses/117023587543794517",
            "date": "2026-08-02 02:37:17",
            "content": "<p>A <strong>readable</strong> post &amp; note.</p>",
            "tags": ["lolicon", "doodlegirl"],
            "account": {"username": "Blue_", "acct": "Blue_"},
            "count": 1,
            "num": 1,
            "media": {
                "type": "image",
                "url": "https://media.baraag.net/file.jpg"
            }
        }));

        assert_eq!(parsed.category.as_deref(), Some("baraag"));
        assert_eq!(parsed.post_id.as_deref(), Some("117023587543794517"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://baraag.net/@Blue_/117023587543794517")
        );
        assert_eq!(
            parsed.description.as_deref(),
            Some("A readable post & note.")
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://media.baraag.net/file.jpg")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "Blue_".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "doodlegirl".to_string())));
    }

    #[test]
    fn idolcomplex_metadata_uses_stable_post_identity_and_categorized_tags() {
        let parsed = parse_metadata(&json!({
            "category": "idolcomplex",
            "subcategory": "tag",
            "id": "60rvNVpQr3A",
            "file_url": "https://iv.sankakucomplex.com/data/example.jpg?expires=1",
            "created_at": 1786632138,
            "rating": "e",
            "tags_artist": ["babykatie666"],
            "tags_general": ["solo"],
            "tags_medium": ["landscape"]
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("60rvNVpQr3A"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.idolcomplex.com/en/posts/60rvNVpQr3A")
        );
        assert_eq!(parsed.rating.as_deref(), Some("e"));
        assert!(parsed.created_at.is_some());
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "babykatie666".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "solo".to_string())));
        assert!(parsed
            .tags
            .contains(&("medium".to_string(), "landscape".to_string())));
    }

    #[test]
    fn sankaku_metadata_uses_stable_post_identity_and_categorized_tags() {
        let parsed = parse_metadata(&json!({
            "category": "sankaku",
            "id": 40001234,
            "file_url": "https://v.sankakucomplex.com/data/example.jpg?expires=1",
            "created_at": 1786632138,
            "rating": "s",
            "tags_artist": ["artist_name"],
            "tags_general": ["solo"],
            "tags_meta": ["high_resolution"]
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("40001234"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://sankaku.app/posts/40001234")
        );
        assert_eq!(parsed.rating.as_deref(), Some("s"));
        assert!(parsed
            .tags
            .contains(&(String::from("creator"), String::from("artist_name"))));
        assert!(parsed.tags.contains(&(String::new(), String::from("solo"))));
        assert!(parsed
            .tags
            .contains(&(String::from("meta"), String::from("high_resolution"))));
    }

    #[test]
    fn artstation_metadata_uses_hash_id_project_identity_and_asset_order() {
        let parsed = parse_metadata_with_url(
            &json!({
                "category": "artstation",
                "hash_id": "abcd1",
                "title": "  Project title  ",
                "description": "Project description",
                "date": "2026-08-14T12:30:00+00:00",
                "count": 2,
                "num": 2,
                "cover_url": "https://cdnb.artstation.com/p/assets/covers/images/cover.jpg",
                "asset": {
                    "id": 987,
                    "position": 1,
                    "asset_type": "image",
                    "image_url": "https://cdn.artstation.com/p/asset-2.jpg"
                },
                "tags": ["environment", {"name": "concept-art"}],
                "userinfo": {"username": "artist-name"}
            }),
            Some("https://cdn.artstation.com/p/asset-2.jpg"),
        );

        assert_eq!(parsed.post_id.as_deref(), Some("abcd1"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.artstation.com/projects/abcd1")
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://cdn.artstation.com/p/asset-2.jpg")
        );
        assert_eq!(parsed.page_num, Some(1));
        assert_eq!(parsed.page_count, Some(1));
        assert_eq!(parsed.title.as_deref(), Some("Project title"));
        assert_eq!(parsed.description.as_deref(), Some("Project description"));
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-08-14T12:30:00+00:00")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "environment".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "concept-art".to_string())));
        assert_eq!(parsed.item_key.as_deref(), Some("artstation:abcd1:1"));
    }

    #[test]
    fn artstation_projects_without_a_cover_keep_all_content_assets() {
        let parsed = parse_metadata(&json!({
            "category": "artstation",
            "hash_id": "abcd2",
            "count": 2,
            "num": 2,
            "cover_url": "https://cdnb.artstation.com/p/assets/images/cover.jpg",
            "asset": {
                "position": 1,
                "asset_type": "image",
                "image_url": "https://cdn.artstation.com/p/asset-2.jpg"
            }
        }));

        assert_eq!(parsed.page_num, Some(2));
        assert_eq!(parsed.page_count, Some(2));
        assert_eq!(parsed.item_key.as_deref(), Some("artstation:abcd2:2"));
    }

    #[test]
    fn artstation_cover_after_first_content_asset_is_not_counted() {
        let parsed = parse_metadata(&json!({
            "category": "artstation",
            "hash_id": "abcd3",
            "count": 2,
            "num": 1,
            "cover_url": "https://cdnb.artstation.com/p/assets/covers/images/cover.jpg",
            "asset": {
                "position": 0,
                "asset_type": "image",
                "image_url": "https://cdn.artstation.com/p/asset-1.jpg"
            }
        }));

        assert_eq!(parsed.page_num, Some(1));
        assert_eq!(parsed.page_count, Some(1));
        assert_eq!(parsed.item_key.as_deref(), Some("artstation:abcd3:1"));
    }

    #[test]
    fn webtoons_episode_metadata_keeps_episode_identity_and_child_order() {
        let parsed = parse_metadata_with_url(
            &json!({
                "category": "webtoons",
                "title_no": 123,
                "episode_no": "7",
                "num": 2,
                "count": 3,
                "author_name": "Creator",
                "title": "Comic title",
                "episode_name": "Episode seven",
                "description": "Episode description",
                "date": "2026-08-14",
                "genre": "Fantasy",
                "lang": "en",
                "language": "English",
                "comic": "comic",
                "file_url": "https://swebtoon-phinf.pstatic.net/page-2.jpg",
                "_url": "https://swebtoon-phinf.pstatic.net/page-2.jpg",
                "_parent": {
                    "_url": "https://webtoons.com/en/fantasy/comic/episode/viewer?title_no=123&episode_no=7"
                }
            }),
            Some("https://swebtoon-phinf.pstatic.net/page-2.jpg"),
        );

        assert_eq!(parsed.category.as_deref(), Some("webtoons"));
        assert_eq!(parsed.post_id.as_deref(), Some("123:7"));
        assert_eq!(parsed.page_num, Some(2));
        assert_eq!(parsed.page_count, Some(3));
        assert_eq!(parsed.item_key.as_deref(), Some("webtoons:123:7:2"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.webtoons.com/en/fantasy/comic/episode/viewer?title_no=123&episode_no=7")
        );
        assert_eq!(parsed.title.as_deref(), Some("Comic title"));
        assert_eq!(parsed.description.as_deref(), Some("Episode description"));
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "Creator".to_string())));
        assert!(!parsed
            .tags
            .iter()
            .any(|(namespace, _)| namespace == "genre" || namespace == "language"));
        assert!(!parsed
            .tags
            .iter()
            .any(|(namespace, _)| namespace.is_empty()));
    }

    #[test]
    fn webtoons_episode_without_viewer_url_uses_structural_fallback() {
        let parsed = parse_metadata(&json!({
            "category": "webtoons",
            "title_no": "456",
            "episode_no": 9,
            "num": "1",
            "count": "2",
            "username": "creator",
            "file_url": "https://swebtoon-phinf.pstatic.net/page-1.jpg",
            "lang": "en",
            "genre": "fantasy",
            "comic": "a comic/title"
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("456:9"));
        assert_eq!(parsed.page_num, Some(1));
        assert_eq!(parsed.page_count, Some(2));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some(
                "https://www.webtoons.com/en/fantasy/a%20comic%2Ftitle/episode/viewer?title_no=456&episode_no=9"
            )
        );
    }

    #[test]
    fn webtoons_episode_without_structural_fields_has_no_canonical_url() {
        let parsed = parse_metadata(&json!({
            "category": "webtoons",
            "title_no": 456,
            "episode_no": 9,
            "file_url": "https://swebtoon-phinf.pstatic.net/page-1.jpg"
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("456:9"));
        assert!(parsed.canonical_post_url.is_none());
    }

    #[test]
    fn artstation_does_not_fall_back_to_unrelated_top_level_id() {
        let parsed = parse_metadata(&json!({
            "category": "artstation",
            "id": 987,
            "asset": {"id": 123, "image_url": "https://cdn.artstation.com/asset.jpg"}
        }));
        assert!(parsed.post_id.is_none());
        assert!(parsed.canonical_post_url.is_none());
    }

    #[test]
    fn hentaifoundry_metadata_uses_post_identity_creator_and_explicit_tags() {
        let parsed = parse_metadata(&json!({
            "category": "hentaifoundry",
            "index": 123456,
            "artist": "artist-name",
            "user": "fallback-name",
            "title": "Picture title",
            "description": "Picture description",
            "date": "2026-08-14",
            "src": "https://cdn.example/image.jpg",
            "source": "https://source.example/image",
            "tags": ["original", "solo"],
            "categories": ["Manga"],
            "ratings": ["Adult"],
            "media": "image"
        }));

        assert_eq!(parsed.category.as_deref(), Some("hentaifoundry"));
        assert_eq!(parsed.post_id.as_deref(), Some("123456"));
        assert_eq!(parsed.item_key.as_deref(), Some("hentaifoundry:123456:0"));
        assert_eq!(parsed.title.as_deref(), Some("Picture title"));
        assert_eq!(parsed.description.as_deref(), Some("Picture description"));
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-08-14T00:00:00+00:00")
        );
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.hentai-foundry.com/pictures/user/artist-name/123456/picto")
        );
        assert_eq!(
            parsed.source_urls,
            [
                "https://www.hentai-foundry.com/pictures/user/artist-name/123456/picto",
                "https://source.example/image",
                "https://cdn.example/image.jpg"
            ]
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://cdn.example/image.jpg")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "original".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "Manga".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "Adult".to_string())));
    }

    fn deviantart_fixture(url: &str) -> serde_json::Value {
        json!({
            "category": "deviantart",
            "deviationid": 123456789,
            "author": {"username": "ArtistName"},
            "tags": [
                {"tag_name": "landscape"},
                {"name": "concept-art"}
            ],
            "description": "<p>A <strong>concise</strong> description.</p><p>With &amp; details.</p>",
            "title": "Artwork title",
            "date": "2026-08-14",
            "num": 2,
            "count": 4,
            "url": url,
            "content": {"src": "https://images.example/artwork.jpg"}
        })
    }

    #[test]
    fn deviantart_metadata_normalizes_identity_creator_tags_description_url_and_media() {
        let parsed = parse_metadata(&deviantart_fixture(
            "http://deviantart.com/ArtistName/art/Artwork-123456789",
        ));

        assert_eq!(parsed.category.as_deref(), Some("deviantart"));
        assert_eq!(parsed.post_id.as_deref(), Some("123456789"));
        assert_eq!(parsed.title.as_deref(), Some("Artwork title"));
        assert_eq!(
            parsed.description.as_deref(),
            Some("A concise description. With & details.")
        );
        assert_eq!(parsed.page_num, Some(2));
        assert_eq!(parsed.page_count, Some(4));
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-08-14T00:00:00+00:00")
        );
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.deviantart.com/ArtistName/art/Artwork-123456789")
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://images.example/artwork.jpg")
        );
        assert!(parsed
            .tags
            .contains(&(String::new(), "landscape".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "concept-art".to_string())));
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "ArtistName".to_string())));
    }

    #[test]
    fn deviantart_canonical_url_rejects_lookalike_hosts_and_uses_stable_fallback() {
        let parsed = parse_metadata(&deviantart_fixture(
            "https://www.deviantart.com.evil.example/ArtistName/art/Artwork-123456789",
        ));

        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.deviantart.com/view/123456789")
        );
    }

    #[test]
    fn tumblr_metadata_uses_post_identity_blog_creator_and_source_order() {
        let parsed = parse_metadata_with_url(
            &json!({
                "category": "tumblr",
                "id": 123456789,
                "blog": {"name": "nasa"},
                "tags": ["space", "science"],
                "summary": "A space update",
                "caption": "<p>A <strong>space</strong> update about Roman&rsquo;s telescope.</p>",
                "post_url": "http://nasa.tumblr.com/post/123456789/a-space-update?ref=feed",
                "date": "2026-08-15 10:20:30 GMT",
                "num": 2,
                "count": 3
            }),
            Some("https://64.media.tumblr.com/photo.jpg"),
        );

        assert_eq!(parsed.category.as_deref(), Some("tumblr"));
        assert_eq!(parsed.post_id.as_deref(), Some("123456789"));
        assert_eq!(parsed.title.as_deref(), Some("A space update"));
        assert_eq!(
            parsed.description.as_deref(),
            Some("A space update about Roman's telescope.")
        );
        assert_eq!(parsed.page_num, Some(2));
        assert_eq!(parsed.page_count, Some(3));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://nasa.tumblr.com/post/123456789/a-space-update")
        );
        assert_eq!(
            parsed.media_url.as_deref(),
            Some("https://64.media.tumblr.com/photo.jpg")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "nasa".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "space".to_string())));
    }

    #[test]
    fn patreon_metadata_normalizes_creator_html_description_and_canonical_url() {
        let parsed = parse_metadata_with_url(
            &json!({
                "category": "patreon",
                "id": 987654321,
                "title": "Behind the scenes",
                "content": "<p>Line <strong>one</strong>.</p><p>Line &amp; two.</p>",
                "published_at": "2026-08-16T12:30:00+00:00",
                "patreon_url": "http://www.patreon.com/posts/behind-the-scenes-987654321?ref=feed",
                "tags": ["exclusive", "wip"],
                "creator": {
                    "url": "https://www.patreon.com/c/creator-name",
                    "full_name": "Creator Name"
                }
            }),
            Some("https://cdn.patreon.com/file.jpg"),
        );

        assert_eq!(parsed.category.as_deref(), Some("patreon"));
        assert_eq!(parsed.post_id.as_deref(), Some("987654321"));
        assert_eq!(parsed.title.as_deref(), Some("Behind the scenes"));
        assert_eq!(parsed.description.as_deref(), Some("Line one. Line & two."));
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-08-16T12:30:00+00:00")
        );
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.patreon.com/posts/behind-the-scenes-987654321")
        );
        assert_eq!(
            parsed.source_url.as_deref(),
            Some("https://www.patreon.com/posts/behind-the-scenes-987654321")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "creator-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "exclusive".to_string())));
        assert_eq!(
            parsed.source_urls,
            [
                "https://www.patreon.com/posts/behind-the-scenes-987654321",
                "https://cdn.patreon.com/file.jpg"
            ]
        );
    }

    #[test]
    fn generic_description_fallback_strips_html_when_no_site_specific_rule_exists() {
        let parsed = parse_metadata(&json!({
            "category": "patreon",
            "id": 1,
            "description": "<p>Hello <strong>world</strong>.</p>"
        }));

        assert_eq!(parsed.description.as_deref(), Some("Hello world."));
    }

    #[test]
    fn generic_description_fallback_decodes_numeric_html_entities_without_markup() {
        let parsed = parse_metadata(&json!({
            "category": "patreon",
            "id": 1,
            "description": "General-Irrelevant&#x27;s character, Melon! She&#39;s a slime."
        }));

        assert_eq!(
            parsed.description.as_deref(),
            Some("General-Irrelevant's character, Melon! She's a slime.")
        );
    }

    #[test]
    fn generic_title_strips_html_markup() {
        let parsed = parse_metadata(&json!({
            "category": "patreon",
            "id": 1,
            "title": "<strong>A rendez vous with Trixy</strong>"
        }));

        assert_eq!(parsed.title.as_deref(), Some("A rendez vous with Trixy"));
    }

    #[test]
    fn fanbox_metadata_normalizes_creator_text_description_and_canonical_url() {
        let parsed = parse_metadata(&json!({
            "category": "fanbox",
            "id": "112233",
            "creatorId": "creator-name",
            "title": "Fanbox post",
            "text": "Creator update",
            "publishedDatetime": "2026-08-16T10:00:00+00:00",
            "tags": ["exclusive"]
        }));

        assert_eq!(parsed.category.as_deref(), Some("fanbox"));
        assert_eq!(parsed.post_id.as_deref(), Some("112233"));
        assert_eq!(parsed.description.as_deref(), Some("Creator update"));
        assert_eq!(
            parsed.created_at.as_deref(),
            Some("2026-08-16T10:00:00+00:00")
        );
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://creator-name.fanbox.cc/posts/112233")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "creator-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "exclusive".to_string())));
    }

    #[test]
    fn subscribestar_metadata_normalizes_creator_html_description_and_canonical_url() {
        let parsed = parse_metadata(&json!({
            "category": "subscribestar",
            "id": 991122,
            "post_id": 778899,
            "author_name": "creator-name",
            "author_nick": "Creator Name",
            "content": "<html><body><h1>Ignored</h1><p>Line <strong>one</strong>.</p></body></html>",
            "tags": ["behind-the-scenes"],
            "date": "2026-08-16T08:30:00+00:00"
        }));

        assert_eq!(parsed.category.as_deref(), Some("subscribestar"));
        assert_eq!(parsed.post_id.as_deref(), Some("778899"));
        assert_eq!(parsed.item_key.as_deref(), Some("subscribestar:778899:0"));
        assert_eq!(parsed.description.as_deref(), Some("Ignored Line one."));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://subscribestar.art/posts/778899")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "creator-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "behind-the-scenes".to_string())));
    }

    #[test]
    fn furaffinity_metadata_uses_submission_identity_artist_and_keywords() {
        let parsed = parse_metadata(&json!({
            "category": "furaffinity",
            "id": 123456,
            "artist": "ExampleArtist",
            "title": "Example work",
            "description": "Example description",
            "tags": ["digital_art", "canine"],
            "date": "2026-08-15T12:00:00+00:00",
            "url": "https://d.furaffinity.net/art/example/file.png"
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("123456"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.furaffinity.net/view/123456/")
        );
        assert_eq!(parsed.title.as_deref(), Some("Example work"));
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "ExampleArtist".to_string())));
        assert!(parsed.tags.contains(&(String::new(), "canine".to_string())));
    }

    #[test]
    fn twitter_metadata_uses_tweet_identity_and_creator() {
        let parsed = parse_metadata(&json!({
            "category": "twitter",
            "tweet_id": "12345",
            "id": "media-id-is-not-the-post-id",
            "author": {"name": "OpenAI"},
            "date": "2026-08-25T12:00:00+00:00",
            "num": 1,
            "count": 2
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("12345"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://x.com/OpenAI/status/12345")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "OpenAI".to_string())));
    }

    #[test]
    fn newgrounds_metadata_preserves_post_artist_tags_and_source() {
        let parsed = parse_metadata(&json!({
            "category": "newgrounds",
            "index": 123456,
            "user": "artist-name",
            "artist": ["artist-name", "collaborator"],
            "title": "Submission title",
            "description": "<p>Submission <strong>description</strong>.</p>",
            "post_url": "https://www.newgrounds.com/art/view/artist-name/submission-title?ref=profile",
            "url": "https://art.ngfiles.com/images/example.png",
            "date": "2026-08-25T12:00:00+00:00",
            "rating": "m",
            "tags": ["digital-art", "character"]
        }));

        assert_eq!(parsed.post_id.as_deref(), Some("123456"));
        assert_eq!(parsed.title.as_deref(), Some("Submission title"));
        assert_eq!(
            parsed.description.as_deref(),
            Some("Submission description.")
        );
        assert_eq!(parsed.rating.as_deref(), Some("m"));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.newgrounds.com/art/view/artist-name/submission-title")
        );
        assert!(parsed
            .tags
            .contains(&("creator".to_string(), "artist-name".to_string())));
        assert!(parsed
            .tags
            .contains(&(String::new(), "digital-art".to_string())));
    }

    #[test]
    fn tumblr_canonical_url_rejects_lookalike_hosts() {
        let parsed = parse_metadata(&json!({
            "category": "tumblr",
            "id": 42,
            "blog_name": "safe-blog",
            "post_url": "https://tumblr.com.evil.example/safe-blog/42"
        }));
        assert_eq!(
            parsed.canonical_post_url.as_deref(),
            Some("https://www.tumblr.com/safe-blog/42")
        );
    }
}
