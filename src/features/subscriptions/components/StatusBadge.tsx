import styles from '../SubscriptionsScreen.module.css';

export function StatusBadge({
  tone,
  label,
}: {
  tone: 'running' | 'paused' | 'attention' | 'success' | 'idle';
  label: string;
}) {
  const toneClass = tone === 'running'
    ? styles.statusRunning
    : tone === 'paused'
      ? styles.statusPaused
      : tone === 'attention'
        ? styles.statusAttention
        : tone === 'success'
          ? styles.statusSuccess
          : styles.statusIdle;

  return (
    <span className={`${styles.statusBadge} ${toneClass}`.trim()}>
      {label}
    </span>
  );
}
