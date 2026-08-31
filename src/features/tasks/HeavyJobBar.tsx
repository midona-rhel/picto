import { useEffect, useState } from 'react';
import { invoke } from '../../platform/ipc';
import type { CloudSyncStatus } from '../../shared/types/generated/application/CloudSyncStatus';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import styles from './HeavyJobBar.module.css';

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

export function HeavyJobBar() {
  const [status, setStatus] = useState<CloudSyncStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const next = await invoke<CloudSyncStatus>('cloud.status.get').catch(() => null);
      if (!cancelled) {
        setStatus(next);
        timeout = setTimeout(poll, next?.state === 'reconciling' ? 250 : 1_000);
      }
    };
    void poll();
    return () => {
      cancelled = true;
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

  return (
    <div className={styles.root} data-open={active || undefined} role="status" aria-live="polite">
      <div className={styles.summary}>
        <span className={styles.label}>{status?.message || 'Working…'}</span>
        {detail ? <span className={styles.detail}>{detail}</span> : null}
      </div>
      <ProgressBar
        done={status?.completed_units ?? 0}
        total={status?.total_units ?? 0}
        indeterminate={!exact}
        height={3}
      />
    </div>
  );
}
