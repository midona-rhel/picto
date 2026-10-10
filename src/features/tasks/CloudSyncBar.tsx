import { useEffect, useState } from 'react';
import { invoke } from '../../platform/ipc';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import type { CloudSyncStatus } from '../../shared/types/generated/application/CloudSyncStatus';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { ProgressDialog } from '../../shared/ui/ProgressDialog';
import styles from './CloudSyncBar.module.css';

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let amount = value;
  let unit = -1;
  do {
    amount /= 1024;
    unit += 1;
  } while (amount >= 1024 && unit < units.length - 1);
  return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${units[unit]}`;
}

export function CloudSyncBar() {
  const [status, setStatus] = useState<CloudSyncStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    // Status changes arrive as `cloud` invalidations. Progress written during a
    // reconcile does not publish, so poll quickly only while one runs, slowly
    // while sync is configured, and not at all while it is disabled.
    const poll = async () => {
      if (timeout) clearTimeout(timeout);
      timeout = undefined;
      const next = await invoke<CloudSyncStatus>('cloud.status.get').catch(() => null);
      if (cancelled) return;
      setStatus(next);
      const delay = next?.state === 'reconciling' ? 250 : next && next.state !== 'disabled' ? 15_000 : null;
      if (delay !== null) timeout = setTimeout(poll, delay);
    };
    void poll();
    const unregister = libraryInvalidation.register('cloud', () => { void poll(); });
    return () => {
      cancelled = true;
      unregister();
      if (timeout) clearTimeout(timeout);
    };
  }, []);

  const active = status?.state === 'reconciling';
  const exact = active && status.total_units !== null && status.total_units > 0;
  const detail = exact
    ? status.phase === 'blobs'
      ? `${formatBytes(status.completed_units)} / ${formatBytes(status.total_units ?? 0)}`
      : `${status.completed_units} / ${status.total_units}`
    : null;
  const message = status?.message || 'Working…';
  const barActive = Boolean(active && !status?.blocking);

  return (
    <>
      <ProgressDialog
        open={Boolean(active && status?.blocking)}
        message={message}
        detail={detail}
        done={status?.completed_units ?? 0}
        total={status?.total_units ?? 0}
        indeterminate={!exact}
      />
      <div
        className={styles.root}
        data-open={barActive || undefined}
        role="status"
        aria-live="polite"
        aria-hidden={!barActive}
      >
        <div className={styles.progress}>
          <ProgressBar
            done={status?.completed_units ?? 0}
            total={status?.total_units ?? 0}
            indeterminate={!exact}
            height={4}
          />
        </div>
        <div className={styles.message}>
          <span className={styles.label}>{message}</span>
          {detail ? <span className={styles.detail}>{detail}</span> : null}
        </div>
      </div>
    </>
  );
}
