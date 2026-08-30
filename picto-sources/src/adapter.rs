use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::{
    DiscoveryBatch, DiscoveryRequest, HttpRuntime, MediaDescriptor, RequestCredentials,
    SourceError, SourcePartition, SourcePost,
};

pub type AdapterFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DiscoveryBatch, SourceError>> + Send + 'a>>;
pub type PostFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SourcePost, SourceError>> + Send + 'a>>;
pub type MediaFuture<'a> =
    Pin<Box<dyn Future<Output = Result<MediaDescriptor, SourceError>> + Send + 'a>>;
pub type PreflightFuture<'a> = Pin<Box<dyn Future<Output = Result<(), SourceError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub domain: &'static str,
    pub partitions: &'static [&'static str],
    pub anonymous: bool,
}

pub trait NativeSourceAdapter: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    /// Most gallery-dl extractors use a Firefox browser identity. Providers
    /// whose public API requires an application identity override it here.
    fn user_agent(&self) -> Option<&'static str> {
        None
    }

    /// Additional first-party domains that may receive the provider's saved
    /// session. Media CDNs remain excluded unless explicitly declared here.
    fn credential_domains(&self) -> &'static [&'static str] {
        &[]
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError>;

    fn preflight<'a>(
        &'a self,
        _query: &'a str,
        _credentials: &'a RequestCredentials,
        _http: &'a HttpRuntime,
        _cancel: &'a CancellationToken,
    ) -> PreflightFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a>;

    fn resolve_post<'a>(
        &'a self,
        post: SourcePost,
        _credentials: &'a RequestCredentials,
        _http: &'a HttpRuntime,
        _cancel: &'a CancellationToken,
    ) -> PostFuture<'a> {
        Box::pin(async move { Ok(post) })
    }

    /// Resolve a provider media page into its final downloadable asset. Most
    /// providers already return direct URLs and use this identity default.
    fn resolve_media<'a>(
        &'a self,
        media: MediaDescriptor,
        _credentials: &'a RequestCredentials,
        _http: &'a HttpRuntime,
        _cancel: &'a CancellationToken,
    ) -> MediaFuture<'a> {
        Box::pin(async move { Ok(media) })
    }

    /// Limit media tasks when a provider must resolve and download each asset
    /// as one serial operation. Direct-media providers use the worker limit.
    fn media_concurrency(&self) -> usize {
        usize::MAX
    }

    fn partition_order(&self) -> Vec<SourcePartition> {
        self.descriptor()
            .partitions
            .iter()
            .map(|partition| SourcePartition::new(*partition))
            .collect()
    }
}

#[derive(Default)]
pub struct ProviderRegistry {
    adapters: BTreeMap<&'static str, Arc<dyn NativeSourceAdapter>>,
}

impl ProviderRegistry {
    pub fn native() -> Self {
        let mut registry = Self::default();
        registry.register(crate::providers::baraag::adapter());
        registry.register(crate::providers::danbooru::adapter());
        registry.register(crate::providers::deviantart::adapter());
        registry.register(crate::providers::e621::adapter());
        registry.register(crate::providers::ehentai::adapter());
        registry.register(crate::providers::fanbox::adapter());
        registry.register(crate::providers::furaffinity::adapter());
        registry.register(crate::providers::gelbooru::adapter());
        registry.register(crate::providers::hentaifoundry::adapter());
        registry.register(crate::providers::idolcomplex::adapter());
        registry.register(crate::providers::konachan::adapter());
        registry.register(crate::providers::newgrounds::adapter());
        registry.register(crate::providers::onlyfans::adapter());
        registry.register(crate::providers::pawchive::adapter());
        registry.register(crate::providers::patreon::adapter());
        registry.register(crate::providers::pixiv::adapter());
        registry.register(crate::providers::pixivuser::adapter());
        registry.register(crate::providers::rule34::adapter());
        registry.register(crate::providers::safebooru::adapter());
        registry.register(crate::providers::sankaku::adapter());
        registry.register(crate::providers::subscribestar::adapter());
        registry.register(crate::providers::twitter::adapter());
        registry.register(crate::providers::yandere::adapter());
        registry
    }

    pub fn register(&mut self, adapter: impl NativeSourceAdapter + 'static) {
        let id = adapter.descriptor().id;
        let replaced = self.adapters.insert(id, Arc::new(adapter));
        assert!(replaced.is_none(), "duplicate native provider id: {id}");
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn NativeSourceAdapter>> {
        self.adapters.get(id).cloned()
    }

    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        self.adapters
            .values()
            .map(|adapter| adapter.descriptor())
            .collect()
    }
}
