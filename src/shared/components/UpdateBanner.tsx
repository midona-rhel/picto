import { IconDownload, IconRefresh, IconX } from '@tabler/icons-react';
import { useUpdaterStore } from '../../state-legacy/updaterStore';
import { formatFileSize } from '../lib/formatters';
import styles from './UpdateBanner.module.css';

export function UpdateBanner() {
  const { status, version, percent, transferred, total, dismissed } = useUpdaterStore();
  const { dismiss, installUpdate } = useUpdaterStore();

  // Only show for actionable states, and not if dismissed
  if (dismissed) return null;
  if (status !== 'available' && status !== 'downloading' && status !== 'ready') return null;

  return (
    <div className={styles.banner}>
      {status === 'available' && (
        <span className={styles.text}>
          <IconDownload size={13} style={{ verticalAlign: -2, marginRight: 4 }} />
          Update <strong>v{version}</strong> found — downloading…
        </span>
      )}

      {status === 'downloading' && (
        <>
          <span className={styles.text}>
            Downloading update… {Math.round(percent)}%
            {total > 0 && (
              <span className={styles.detail}>
                {' '}({formatFileSize(transferred)} / {formatFileSize(total)})
              </span>
            )}
          </span>
          <div className={styles.progressTrack}>
            <div className={styles.progressFill} style={{ width: `${percent}%` }} />
          </div>
        </>
      )}

      {status === 'ready' && (
        <>
          <span className={styles.text}>
            Update ready — <strong>v{version}</strong>
          </span>
          <button className={styles.action} onClick={installUpdate}>
            <IconRefresh size={13} />
            Restart to update
          </button>
        </>
      )}

      <button className={styles.dismiss} onClick={dismiss} aria-label="Dismiss">
        <IconX size={12} />
      </button>
    </div>
  );
}
