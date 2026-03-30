import type { SubscriptionIssueRecord, SubscriptionRunRecord } from '../../../shared/types/subscriptions';
import { formatDateTime, formatRelativeTime } from '../subscriptionUtils';
import { EmptyState } from './EmptyState';
import { StatusBadge } from './StatusBadge';
import styles from '../SubscriptionsScreen.module.css';

export function RunsTab({
  runs,
  issues,
}: {
  runs: SubscriptionRunRecord[];
  issues: SubscriptionIssueRecord[];
}) {
  return (
    <div className={styles.section}>
      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <div className={styles.sectionTitle}>Recent Runs</div>
        </div>
        {runs.map((run) => (
          <div key={run.run_id} className={styles.runCard}>
            <div className={styles.runHeader}>
              <div className={styles.sectionTitle}>Run #{run.run_id}</div>
              <StatusBadge tone={run.status === 'failed' ? 'attention' : 'idle'} label={run.status} />
            </div>
            <div className={styles.runStats}>
              <span className={styles.smallBadge}>{run.files_downloaded} downloaded</span>
              <span className={styles.smallBadge}>{run.files_skipped} skipped</span>
              <span className={styles.smallBadge}>{run.metadata_invalid} invalid metadata</span>
            </div>
            <div className={styles.muted}>
              Started {formatDateTime(run.started_at)} · Finished {formatDateTime(run.finished_at)}
            </div>
            {(run.error_message || run.failure_kind) && (
              <div className={styles.muted}>{run.failure_kind ?? 'run'} · {run.error_message ?? 'No retained message'}</div>
            )}
          </div>
        ))}
        {!runs.length && (
          <EmptyState title="No runs yet" description="Run the subscription or one of its queries to populate history." />
        )}
      </div>

      <div className={styles.divider} />

      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <div className={styles.sectionTitle}>Issues</div>
        </div>
        {issues.map((issue) => (
          <div key={issue.issue_id} className={styles.issueCard}>
            <div className={styles.issueHeader}>
              <div className={styles.sectionTitle}>{issue.issue_kind}</div>
              <StatusBadge tone={issue.status === 'resolved' ? 'idle' : 'attention'} label={issue.status} />
            </div>
            <div>{issue.message}</div>
            {issue.detail && <div className={styles.muted}>{issue.detail}</div>}
            <div className={styles.issueStats}>
              {issue.query_id != null && <span className={styles.smallBadge}>query {issue.query_id}</span>}
              <span className={styles.smallBadge}>seen {formatRelativeTime(issue.last_seen_at)}</span>
            </div>
          </div>
        ))}
        {!issues.length && (
          <EmptyState title="No issues" description="Auth, rate-limit, extractor, and import issues will show here when they persist." />
        )}
      </div>
    </div>
  );
}
