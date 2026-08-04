use crate::subscriptions::credential_service::{
    ResolvedRunCredential, SubscriptionCredentialService,
};
use crate::subscriptions::gallery_dl_runner::FailureKind;

use super::{SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) async fn load_run_credential(
        &mut self,
        subscription_id: i64,
        query_id: i64,
        site_id: &str,
        url: &str,
        subscription_id_str: &str,
        progress: &SyncProgress,
    ) -> ResolvedRunCredential {
        let resolved = SubscriptionCredentialService::new(self.db)
            .resolve_for_run(subscription_id, Some(query_id), site_id, url)
            .await;

        if resolved.auth_supported
            && resolved.auth_required_for_full_access
            && !resolved.has_credential()
        {
            self.emit_progress(
                subscription_id_str,
                progress,
                "No credential configured for this site; some content may be inaccessible",
            );
        }

        resolved
    }

    pub(super) async fn note_run_auth_failure(
        &self,
        subscription_id: i64,
        query_id: i64,
        site_id: &str,
        failure_kind: FailureKind,
        detail: Option<&str>,
    ) {
        SubscriptionCredentialService::new(self.db)
            .note_run_auth_failure(
                subscription_id,
                Some(query_id),
                site_id,
                failure_kind,
                detail,
            )
            .await;
    }

    pub(super) async fn note_run_success(
        &self,
        subscription_id: i64,
        query_id: i64,
        site_id: &str,
        used_credential: bool,
    ) {
        SubscriptionCredentialService::new(self.db)
            .note_run_success(subscription_id, Some(query_id), site_id, used_credential)
            .await;
    }
}
