use tokio_util::sync::CancellationToken;

use crate::{
    AdapterFuture, DiscoveryRequest, HttpRuntime, NativeSourceAdapter, ProviderDescriptor,
    RequestCredentials, SourceError,
};

use super::pixiv::{api_url, current_offset, validate_numeric_user, PixivApi};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    PixivUserSource::default()
}

#[derive(Default)]
struct PixivUserSource {
    api: PixivApi,
}

impl NativeSourceAdapter for PixivUserSource {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "pixivuser",
            display_name: "Pixiv (user)",
            domain: "pixiv.net",
            partitions: &["illustrations"],
            anonymous: false,
        }
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        validate_numeric_user(query).map(|_| ())
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            let user_id = validate_numeric_user(&request.query)?;
            let offset = current_offset(request)?;
            let mut url = api_url("/v1/user/illusts");
            url.query_pairs_mut()
                .append_pair("user_id", &user_id)
                .append_pair("offset", &offset.to_string());
            self.api
                .discover_one("pixivuser", request, offset, url, credentials, http, cancel)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::SourcePartition;

    fn request(cursor: Option<&str>) -> DiscoveryRequest {
        DiscoveryRequest {
            query: "173530".into(),
            partition: SourcePartition::new("illustrations"),
            cursor: cursor.map(ToOwned::to_owned),
            page_size: 10,
        }
    }

    #[test]
    fn preserves_numeric_user_query_semantics() {
        assert_eq!(validate_numeric_user(" 173530 ").unwrap(), "173530");
        assert!(validate_numeric_user("@artist").is_err());
        assert!(validate_numeric_user("https://www.pixiv.net/users/173530").is_err());
        assert!(validate_numeric_user("0").is_err());
    }

    #[test]
    fn user_fixture_matches_the_direct_api_envelope() {
        let response: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/pixivuser/user.json")).unwrap();
        let batch =
            super::super::pixiv::normalize_response("pixivuser", &request(None), 0, response)
                .unwrap();
        let post = &batch.posts[0];
        assert_eq!(post.site_id, "pixivuser");
        assert_eq!(post.stable_id, "22334455");
        assert_eq!(post.creator.as_deref(), Some("Aoi Artist"));
        assert_eq!(post.media.len(), 2);
        assert_eq!(post.resume_cursor_after.as_deref(), Some("1"));
    }

    #[test]
    fn user_cursor_is_bounded() {
        assert_eq!(current_offset(&request(Some("9"))).unwrap(), 9);
        assert!(current_offset(&request(Some("next-page"))).is_err());
    }
}
