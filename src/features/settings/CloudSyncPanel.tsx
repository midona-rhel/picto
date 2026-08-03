import { useCallback, useEffect, useState } from 'react';
import { cloudSyncController, type SyncStatus } from '../../controllers/cloudSyncController';
import styles from './CloudSyncPanel.module.css';

type Busy = null | 'syncing' | 'disconnecting';

export function CloudSyncPanel() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    cloudSyncController
      .getStatus()
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  const handleSyncNow = useCallback(() => {
    setBusy('syncing');
    setError(null);
    setMessage(null);
    cloudSyncController
      .syncNow()
      .then(({ report }) => {
        setMessage(
          `Synced. Uploaded ${report.segments_uploaded} segment(s), applied ${report.ops_applied} change(s) from other devices.`,
        );
        refreshStatus();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  }, [refreshStatus]);

  const handleDisconnect = useCallback(() => {
    setBusy('disconnecting');
    setError(null);
    setMessage(null);
    cloudSyncController
      .disconnect()
      .then(() => {
        setMessage('Disconnected. The library on the share was left untouched.');
        refreshStatus();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  }, [refreshStatus]);

  if (!status) {
    return (
      <div className={styles.panel}>
        <div className={styles.hint}>Loading sync status…</div>
      </div>
    );
  }

  if (!status.bound) {
    return (
      <div className={styles.panel}>
        <div className={styles.block}>
          <div className={styles.blockTitle}>Cloud Sync</div>
          <div className={styles.hint}>
            This library is not synced to a cloud service. Libraries are created on and opened
            from cloud services in the Library Manager — open it with{' '}
            <strong>File → Library Manager</strong> (⌘L) and pick a service under “Cloud”.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.panel}>
      <div className={styles.block}>
        <div className={styles.blockTitle}>Cloud Sync</div>
        <div className={styles.statusGrid}>
          <span className={styles.statusLabel}>Remote library</span>
          <span className={styles.statusValue}>{status.library_name}</span>
          <span className={styles.statusLabel}>Location</span>
          <span className={styles.statusValue}>{status.share_root}</span>
          <span className={styles.statusLabel}>Pending changes</span>
          <span className={styles.statusValue}>{status.pending_ops}</span>
          <span className={styles.statusLabel}>This device</span>
          <span className={styles.statusValue}>{status.device_id.slice(0, 12)}…</span>
        </div>
        <div className={styles.actions}>
          <button className={styles.btnPrimary} onClick={handleSyncNow} disabled={busy !== null}>
            {busy === 'syncing' ? 'Syncing…' : 'Sync now'}
          </button>
          <button className={styles.btn} onClick={handleDisconnect} disabled={busy !== null}>
            Disconnect
          </button>
        </div>
        {message ? <div className={styles.message}>{message}</div> : null}
        {error ? <div className={styles.error}>{error}</div> : null}
        <div className={styles.hint}>
          Disconnecting only unlinks this device. Nothing on the share is ever deleted or
          overwritten by Picto.
        </div>
      </div>
    </div>
  );
}
