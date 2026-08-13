/**
 * ProgressBar — thin determinate progress bar driven by done/total counts.
 * Used for auto-tag run progress (2px hairline) and model download rows.
 */

import styles from './ProgressBar.module.css';

export interface ProgressBarProps {
  done?: number;
  total?: number;
  indeterminate?: boolean;
  /** Bar thickness in px. Defaults to 3. */
  height?: number;
}

export function ProgressBar({ done = 0, total = 0, indeterminate = false, height = 3 }: ProgressBarProps) {
  const pct = total > 0 ? Math.max(0, Math.min(100, (done / total) * 100)) : 0;
  return (
    <div
      className={styles.track}
      style={{ height }}
      role="progressbar"
      aria-valuemin={indeterminate ? undefined : 0}
      aria-valuemax={indeterminate ? undefined : 100}
      aria-valuenow={indeterminate ? undefined : Math.round(pct)}
    >
      <div
        className={`${styles.fill} ${indeterminate ? styles.indeterminate : ''}`}
        style={indeterminate ? undefined : { width: `${pct}%` }}
      />
    </div>
  );
}
