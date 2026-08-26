import { useState } from 'react';
import type { SubscriptionRunRecord } from '../../../shared/types/subscriptions';
import { formatDateTime } from '../subscriptionUtils';
import { StatusBadge } from './StatusBadge';
import styles from '../SubscriptionsScreen.module.css';

const INITIAL_ROWS = 20;

/** Dense run-history table. Shows the latest runs, expandable to all. */
export function HistoryTab({ runs }: { runs: SubscriptionRunRecord[] }) {
  const [showAll, setShowAll] = useState(false);

  if (runs.length === 0) {
    return <div className={styles.sectionEmptyLine}>No runs yet.</div>;
  }

  const visible = showAll ? runs : runs.slice(0, INITIAL_ROWS);

  return (
    <div className={`${styles.subscriptionTable} ${styles.historyTable}`.trim()}>
      <div className={`${styles.subscriptionTableRow} ${styles.subscriptionTableHeader} ${styles.historyRow}`}>
        <span>Started</span>
        <span>Status</span>
        <span className={styles.qCellNum}>Fetched</span>
        <span className={styles.qCellNum}>Reused</span>
        <span>Notes</span>
      </div>
      {visible.map((run) => {
        const waiting = run.status === 'pending' && run.failure_kind === 'inbox_full';
        const paused = run.status === 'pending' && run.failure_kind === 'paused';
        const tone = waiting || paused
          ? 'paused' as const
          : run.status === 'failed'
          ? 'attention' as const
          : run.status === 'running'
            ? 'running' as const
            : run.status === 'paused'
              ? 'paused' as const
              : run.status === 'completed' || run.status === 'succeeded'
                ? 'success' as const
                : 'idle' as const;
        const label = run.status === 'completed' || run.status === 'succeeded'
            ? 'Complete'
            : waiting
              ? 'Inbox full'
              : paused
                ? 'Paused'
                : run.failure_kind
                  ? run.failure_kind.split('_').join(' ')
                  : run.status;
        return (
          <div key={run.run_id} className={`${styles.subscriptionTableRow} ${styles.historyRow}`.trim()}>
            <span className={styles.qCellTime}>{formatDateTime(run.started_at)}</span>
            <span className={styles.qCellStatus}>
              <StatusBadge tone={tone} label={label} title={run.error_message ?? undefined} />
            </span>
            <span className={styles.qCellNum}>{run.files_downloaded}</span>
            <span className={styles.qCellNum}>{run.files_skipped}</span>
            <span className={styles.historyNote} title={run.error_message ?? undefined}>
              {run.error_message
                ?? (run.metadata_invalid > 0 ? `${run.metadata_invalid} invalid metadata` : '')}
            </span>
          </div>
        );
      })}
      {runs.length > INITIAL_ROWS && !showAll && (
        <button type="button" className={styles.historyMore} onClick={() => setShowAll(true)}>
          Show all {runs.length} runs
        </button>
      )}
    </div>
  );
}
