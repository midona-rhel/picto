import { useMemo } from 'react';

import { useExportProgressStore } from '../../../state/exportProgressStore';
import { useManualImportStore } from '../../../state/manualImportStore';
import styles from './MainViewProgressBar.module.css';

export function MainViewProgressBar() {
  const exportProgress = useExportProgressStore();
  const importProgress = useManualImportStore();
  const { visible, status, label, done, total } = exportProgress.visible ? exportProgress : importProgress;

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
