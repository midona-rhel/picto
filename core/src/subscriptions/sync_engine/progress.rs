use crate::subscriptions::runtime_tasks::publish_running_progress;

use super::{SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) fn emit_progress(
        &mut self,
        subscription_id: &str,
        progress: &SyncProgress,
        status_text: &str,
    ) {
        self.emit_progress_inner(subscription_id, progress, status_text, false);
    }

    pub(super) fn emit_progress_force(
        &mut self,
        subscription_id: &str,
        progress: &SyncProgress,
        status_text: &str,
    ) {
        self.emit_progress_inner(subscription_id, progress, status_text, true);
    }

    pub(super) fn set_phase(&mut self, phase: &str) {
        self.current_phase = phase.to_string();
    }

    fn emit_progress_inner(
        &mut self,
        subscription_id: &str,
        progress: &SyncProgress,
        status_text: &str,
        force: bool,
    ) {
        let now = std::time::Instant::now();
        if !force && now.duration_since(self.last_progress_emit).as_millis() < 300 {
            return;
        }
        self.last_progress_emit = now;
        publish_running_progress(
            subscription_id,
            &self.subscription_name,
            &self.progress_mode,
            self.group_name.as_deref(),
            self.current_query_id.map(|id| id.to_string()),
            self.current_query_name.clone(),
            progress,
            status_text,
            &self.current_phase,
        );
    }
}
