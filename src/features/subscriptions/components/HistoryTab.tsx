import { useState } from 'react';
import type { SubscriptionRunRecord } from '../../../shared/types/subscriptions';
import { formatDateTime } from '../subscriptionUtils';
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
    <div className={styles.historyTable}>
      <div className={`${styles.historyRow} ${styles.historyHeader}`}>
        <span>Started</span>
        <span>Status</span>
        <span className={styles.qCellNum}>Fetched</span>
        <span className={styles.qCellNum}>Already in library</span>
        <span>Notes</span>
      </div>
      {visible.map((run) => {
        const tone = run.status === 'failed' ? 'attention' : run.status === 'running' ? 'running' : 'idle';
        const dotClass =
          tone === 'running' ? styles.qDotRunning : tone === 'attention' ? styles.qDotAttention : styles.qDotIdle;
        return (
          <div key={run.run_id} className={styles.historyRow}>
            <span className={styles.qCellTime}>{formatDateTime(run.started_at)}</span>
            <span className={styles.qCellStatus}>
              <span className={`${styles.qDot} ${dotClass}`.trim()} />
              {run.failure_kind ? run.failure_kind.split('_').join(' ') : run.status}
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
