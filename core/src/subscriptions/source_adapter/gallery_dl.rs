use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use tokio::sync::mpsc::Sender;

use crate::subscriptions::gallery_dl_runner::{self, GalleryDlRunner, RunOptions, RunSummary};

use super::{
    describe_site, runner_key_for_site, validate_query_kind, DownloadedItem, SiteAdapterDescriptor,
};

pub trait SubscriptionSourceAdapter: Send + Sync {
    fn descriptor(&self, site_id: &str) -> Option<SiteAdapterDescriptor>;
    fn validate_query_kind(&self, site_id: &str, query_kind: &str) -> Result<(), String>;
    fn runner_key(&self, site_id: &str) -> String;
    fn build_url(&self, site_id: &str, query_text: &str) -> Option<String>;
    fn extract_domain(&self, url: &str) -> Option<String>;
    fn run<'a>(
        &'a self,
        opts: &'a RunOptions,
        item_tx: Sender<DownloadedItem>,
    ) -> Pin<Box<dyn Future<Output = Result<RunSummary, String>> + Send + 'a>>;
}

pub struct GalleryDlSourceAdapter {
    runner: GalleryDlRunner,
}

impl GalleryDlSourceAdapter {
    pub fn new(binary_path: PathBuf) -> Self {
        Self {
            runner: GalleryDlRunner::new(binary_path),
        }
    }
}

impl SubscriptionSourceAdapter for GalleryDlSourceAdapter {
    fn descriptor(&self, site_id: &str) -> Option<SiteAdapterDescriptor> {
        describe_site(site_id)
    }

    fn validate_query_kind(&self, site_id: &str, query_kind: &str) -> Result<(), String> {
        validate_query_kind(site_id, query_kind)
    }

    fn runner_key(&self, site_id: &str) -> String {
        runner_key_for_site(site_id)
    }

    fn build_url(&self, site_id: &str, query_text: &str) -> Option<String> {
        gallery_dl_runner::build_url(site_id, query_text)
    }

    fn extract_domain(&self, url: &str) -> Option<String> {
        gallery_dl_runner::extract_domain(url)
    }

    fn run<'a>(
        &'a self,
        opts: &'a RunOptions,
        item_tx: Sender<DownloadedItem>,
    ) -> Pin<Box<dyn Future<Output = Result<RunSummary, String>> + Send + 'a>> {
        Box::pin(async move { self.runner.run(opts, item_tx).await })
    }
}
