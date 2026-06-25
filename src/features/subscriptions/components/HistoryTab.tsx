import type { SubscriptionRunRecord } from '../../../shared/types/subscriptions';
import { formatDateTime, formatRelativeTime } from '../subscriptionUtils';
import { EmptyState } from './EmptyState';
import { StatusBadge } from './StatusBadge';
import styles from '../SubscriptionsScreen.module.css';

export function HistoryTab({ runs }: { runs: SubscriptionRunRecord[] }) {
  if (runs.length === 0) {
    return <EmptyState title="No runs yet" description="Run the subscription to see its history here." />;
  }
  return (
    <div className={styles.tabPanel}>
      {runs.map((run) => (
        <div key={run.run_id} className={styles.runCard}>
          <div className={styles.runHeader}>
            <span className={styles.sectionTitle}>{formatDateTime(run.started_at)}</span>
            <StatusBadge
              tone={run.status === 'failed' ? 'attention' : run.status === 'running' ? 'running' : 'idle'}
              label={run.failure_kind ? `${run.status} · ${run.failure_kind.split('_').join(' ')}` : run.status}
            />
            {run.finished_at && (
              <span className={styles.muted}>finished {formatRelativeTime(run.finished_at)}</span>
            )}
          </div>
          <div className={styles.runStats}>
            <span className={styles.smallBadge}>{run.files_downloaded} downloaded</span>
            <span className={styles.smallBadge}>{run.files_skipped} skipped</span>
            {run.metadata_invalid > 0 && (
              <span className={styles.smallBadge}>{run.metadata_invalid} invalid metadata</span>
            )}
          </div>
          {run.error_message && <div className={styles.muted}>{run.error_message}</div>}
        </div>
      ))}
    </div>
  );
}
