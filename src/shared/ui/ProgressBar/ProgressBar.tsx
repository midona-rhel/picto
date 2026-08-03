/**
 * ProgressBar — thin determinate progress bar driven by done/total counts.
 * Used for auto-tag run progress (2px hairline) and model download rows.
 */

import styles from './ProgressBar.module.css';

export interface ProgressBarProps {
  done: number;
  total: number;
  /** Bar thickness in px. Defaults to 3. */
  height?: number;
}

export function ProgressBar({ done, total, height = 3 }: ProgressBarProps) {
  const pct = total > 0 ? Math.max(0, Math.min(100, (done / total) * 100)) : 0;
  return (
    <div className={styles.track} style={{ height }}>
      <div className={styles.fill} style={{ width: `${pct}%` }} />
    </div>
  );
}
