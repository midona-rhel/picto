use tokio_util::sync::CancellationToken;

use crate::{
    AdapterFuture, DiscoveryRequest, HttpRuntime, MediaDescriptor, MediaFuture,
    NativeSourceAdapter, PostFuture, ProviderDescriptor, RequestCredentials, SourceError,
    SourcePost,
};

use super::pixiv::{validate_numeric_user, PixivApi};

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
            self.api
                .discover_user(request, &user_id, credentials, http, cancel)
                .await
        })
    }

    fn resolve_post<'a>(
        &'a self,
        post: SourcePost,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> PostFuture<'a> {
        Box::pin(async move { self.api.recover_post(post, credentials, http, cancel).await })
    }

    fn resolve_media<'a>(
        &'a self,
        media: MediaDescriptor,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> MediaFuture<'a> {
        Box::pin(async move {
            self.api
                .resolve_ugoira(media, credentials, http, cancel)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_numeric_user_query_semantics() {
        assert_eq!(validate_numeric_user(" 173530 ").unwrap(), "173530");
        assert!(validate_numeric_user("@artist").is_err());
        assert!(validate_numeric_user("https://www.pixiv.net/users/173530").is_err());
        assert!(validate_numeric_user("0").is_err());
    }
}
