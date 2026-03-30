import styles from '../SubscriptionsScreen.module.css';

export function StatusBadge({
  tone,
  label,
}: {
  tone: 'running' | 'paused' | 'attention' | 'idle';
  label: string;
}) {
  const toneClass = tone === 'running'
    ? styles.statusRunning
    : tone === 'paused'
      ? styles.statusPaused
      : tone === 'attention'
        ? styles.statusAttention
        : styles.statusIdle;

  return (
    <span className={`${styles.statusBadge} ${toneClass}`.trim()}>
      {label}
    </span>
  );
}
