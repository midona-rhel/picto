import { useMemo } from 'react';

import { useTaskStore } from '../../../state/taskStore';
import styles from './MainViewProgressBar.module.css';

export function MainViewProgressBar() {
  const familyProgress = useTaskStore((s) => s.familyProgress);

  // Show whichever family is active: export takes priority, then import
  const active = familyProgress.export.visible ? familyProgress.export
    : familyProgress.import.visible ? familyProgress.import
    : null;

  const visible = !!active?.visible;
  const status = active?.completed ? 'completed' : active?.failed ? 'failed' : 'running';
  const done = active?.progress?.done ?? 0;
  const total = active?.progress?.total ?? 0;
  const label = active?.progress?.statusText ?? active?.label ?? '';

  const progress = useMemo(() => {
    if (total <= 0) return 0;
    return Math.max(0, Math.min(100, (done / total) * 100));
  }, [done, total]);

  if (!visible || total <= 0) return null;

  return (
    <div className={styles.root}>
      <div className={styles.panel}>
        <div className={styles.track}>
          <div
            className={`${styles.fill} ${status === 'completed' ? styles.completed : ''}`}
            style={{ width: `${progress}%` }}
          />
        </div>
        <div className={styles.label}>
          {label}: {Math.min(done, total)}/{total}
        </div>
      </div>
    </div>
  );
}
