import { Fragment } from 'react';
import type { SubscriptionProgressEvent } from '../../../shared/types/subscriptions';
import styles from '../SubscriptionsScreen.module.css';

type Phase = 'fetching' | 'downloading' | 'importing';

function derivePhase(progress: SubscriptionProgressEvent): Phase {
  if (progress.ingesting > 0 || (progress.queued_for_ingest > 0 && progress.files_downloaded > 0)) {
    return 'importing';
  }
  if (progress.files_downloaded > 0) return 'downloading';
  return 'fetching';
}

const PHASES: Array<{ key: Phase; label: string }> = [
  { key: 'fetching', label: 'Fetching' },
  { key: 'downloading', label: 'Downloading' },
  { key: 'importing', label: 'Importing' },
];

/** Live run status — phase stepper, counters, current post ticker. */
export function LiveProgressCard({ progress }: { progress: SubscriptionProgressEvent }) {
  const phase = derivePhase(progress);
  const phaseIndex = PHASES.findIndex((p) => p.key === phase);

  const counters: Array<{ label: string; value: number }> = [
    { label: 'Downloaded', value: progress.files_downloaded },
    { label: 'Skipped', value: progress.files_skipped },
    { label: 'Queued', value: progress.queued_for_ingest },
    { label: 'Imported', value: progress.ingested },
  ];
  if (progress.reused > 0) counters.push({ label: 'Reused', value: progress.reused });
  if (progress.failed_ingest > 0) counters.push({ label: 'Failed', value: progress.failed_ingest });

  return (
    <div className={styles.progressCard}>
      <div className={styles.progressPhases}>
        {PHASES.map((entry, index) => (
          <Fragment key={entry.key}>
            {index > 0 && <span className={styles.progressPhaseDivider} />}
            <span
              className={`${styles.progressPhase} ${
                index === phaseIndex ? styles.progressPhaseActive : ''
              }`.trim()}
            >
              {entry.label}
            </span>
          </Fragment>
        ))}
      </div>
      <div className={styles.progressCounters}>
        {counters.map((counter) => (
          <div key={counter.label} className={styles.progressCounter}>
            <span className={styles.progressCounterValue}>{counter.value.toLocaleString()}</span>
            <span className={styles.progressCounterLabel}>{counter.label}</span>
          </div>
        ))}
      </div>
      <div className={styles.progressTicker}>
        {progress.query_name ? `${progress.query_name} — ` : ''}
        {progress.current_post_id ? `post ${progress.current_post_id}` : progress.status_text}
        {progress.last_error ? ` · ${progress.last_error}` : ''}
      </div>
    </div>
  );
}
