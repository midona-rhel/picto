use tracing::warn;

use crate::credential_store::{self, SiteCredential};
use crate::subscriptions::gallery_dl_runner;

use super::{SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) async fn load_run_credential(
        &mut self,
        site_id: &str,
        url: &str,
        subscription_id: &str,
        progress: &SyncProgress,
    ) -> Option<SiteCredential> {
        let site_entry = gallery_dl_runner::site_by_id(site_id);
        let auth_supported = site_entry.is_some_and(|site| site.auth_supported);
        let auth_required = site_entry.is_some_and(|site| site.auth_required_for_full_access);
        let mut credential = None;

        if auth_supported {
            let mut credential_categories = vec![site_id.to_string()];
            if let Some(site) = site_entry {
                credential_categories.push(site.domain.to_string());
                credential_categories.push(site.domain.trim_start_matches("www.").to_string());
            }
            if let Some(domain) = gallery_dl_runner::extract_domain(url) {
                credential_categories.push(domain.clone());
                credential_categories.push(domain.trim_start_matches("www.").to_string());
            }
            credential_categories.sort();
            credential_categories.dedup();

            for category in credential_categories {
                match credential_store::get_credential(&category) {
                    Ok(Some(c)) => {
                        credential = Some(c);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(site = %category, error = %e, "Failed to load credential");
                    }
                }
            }
        }

        if auth_supported && credential.is_none() && auth_required {
            self.emit_progress(
                subscription_id,
                progress,
                "No credential configured for this site; some content may be inaccessible",
            );
            self.update_credential_health(
                site_id,
                "missing",
                Some("No credential configured for a site that commonly requires auth"),
            )
            .await;
        }

        credential
    }

    pub(super) async fn update_credential_health(
        &self,
        site_category: &str,
        health_status: &str,
        last_error: Option<&str>,
    ) {
        if let Err(e) = self
            .db
            .upsert_credential_health(site_category, health_status, last_error)
            .await
        {
            warn!(
                site = %site_category,
                status = %health_status,
                error = %e,
                "Failed to persist credential health"
            );
        }
    }
}
