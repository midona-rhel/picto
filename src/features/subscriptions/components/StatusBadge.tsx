import styles from '../SubscriptionsScreen.module.css';

export function StatusBadge({
  tone,
  label,
  title,
}: {
  tone: 'running' | 'paused' | 'attention' | 'success' | 'idle';
  label: string;
  title?: string;
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
    <span className={`${styles.statusBadge} ${toneClass}`.trim()} title={title}>
      {label}
    </span>
  );
}
