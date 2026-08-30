use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AdapterFuture, DiscoveryBatch, DiscoveryRequest, HttpRuntime, NativeSourceAdapter,
    ProviderDescriptor, RequestCredentials, SourceError,
};

pub trait JsonPageSource: Send + Sync + 'static {
    type Response: DeserializeOwned + Send;

    fn descriptor(&self) -> ProviderDescriptor;
    fn user_agent(&self) -> Option<&'static str> {
        None
    }
    fn validate_query(&self, query: &str) -> Result<(), SourceError>;
    fn request_url(&self, request: &DiscoveryRequest) -> Result<Url, SourceError>;
    fn normalize(
        &self,
        request: &DiscoveryRequest,
        response: Self::Response,
    ) -> Result<DiscoveryBatch, SourceError>;
}

pub struct JsonSourceAdapter<S>(S);

impl<S> JsonSourceAdapter<S> {
    pub const fn new(source: S) -> Self {
        Self(source)
    }
}

impl<S: JsonPageSource> NativeSourceAdapter for JsonSourceAdapter<S> {
    fn descriptor(&self) -> ProviderDescriptor {
        self.0.descriptor()
    }

    fn user_agent(&self) -> Option<&'static str> {
        self.0.user_agent()
    }

    fn validate_query(&self, query: &str) -> Result<(), SourceError> {
        self.0.validate_query(query)
    }

    fn discover<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        credentials: &'a RequestCredentials,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> AdapterFuture<'a> {
        Box::pin(async move {
            self.0.validate_query(&request.query)?;
            let response = http
                .get_json::<S::Response>(self.0.request_url(request)?, credentials, cancel)
                .await?;
            self.0.normalize(request, response)
        })
    }
}
