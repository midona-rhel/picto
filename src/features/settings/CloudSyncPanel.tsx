import { useCallback, useEffect, useState } from 'react';
import { cloudSyncController, type SyncStatus } from '../../controllers/cloudSyncController';
import styles from './CloudSyncPanel.module.css';

type Busy = null | 'syncing' | 'disconnecting';

export function CloudSyncPanel() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    cloudSyncController
      .getStatus()
      .then((nextStatus) => {
        setStatus(nextStatus);
        setStatusError(null);
      })
      .catch((e) => setStatusError(String(e)));
  }, []);

  useEffect(() => {
    refreshStatus();
    const timer = window.setInterval(refreshStatus, 5_000);
    return () => window.clearInterval(timer);
  }, [refreshStatus]);

  const handleSyncNow = useCallback(() => {
    setBusy('syncing');
    setError(null);
    setMessage(null);
    cloudSyncController
      .syncNow()
      .then(({ report }) => {
        const pending =
          report.waiting_for_prerequisites
            ? ' Waiting for missing media or an earlier remote change.'
            : '';
        setMessage(
          `Synced ${report.blobs_uploaded} up/${report.blobs_downloaded} down media, uploaded ${report.segments_uploaded} change batch(es), and applied ${report.ops_applied} remote change(s).${pending}`,
        );
      })
      .catch((e) => setError(String(e)))
      .finally(() => {
        setBusy(null);
        refreshStatus();
      });
  }, [refreshStatus]);

  const handleDisconnect = useCallback(() => {
    setBusy('disconnecting');
    setError(null);
    setMessage(null);
    cloudSyncController
      .disconnect()
      .then(() => {
        setMessage('Disconnected. The library on the share was left untouched.');
      })
      .catch((e) => setError(String(e)))
      .finally(() => {
        setBusy(null);
        refreshStatus();
      });
  }, [refreshStatus]);

  if (!status) {
    return (
      <div className={styles.panel}>
        {statusError ? (
          <div className={styles.error}>{statusError}</div>
        ) : (
          <div className={styles.hint}>Loading sync status…</div>
        )}
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
          <span className={styles.statusLabel}>Local changes to upload</span>
          <span className={styles.statusValue}>{status.pending_ops}</span>
          <span className={styles.statusLabel}>Remote changes waiting</span>
          <span className={styles.statusValue}>
            {status.pending_remote_ops > 0
              ? status.pending_remote_ops
              : status.more_remote_work
                ? 'More queued'
                : 0}
          </span>
          <span className={styles.statusLabel}>Missing media</span>
          <span className={styles.statusValue}>
            {status.missing_blobs}
            {status.failed_blobs > 0 ? ` (${status.failed_blobs} failed)` : ''}
          </span>
          <span className={styles.statusLabel}>Last successful sync</span>
          <span className={styles.statusValue}>
            {status.last_success_at ? new Date(status.last_success_at).toLocaleString() : 'Never'}
          </span>
          <span className={styles.statusLabel}>This device</span>
          <span className={styles.statusValue}>{status.device_id.slice(0, 12)}…</span>
        </div>
        <div className={styles.actions}>
          <button
            className={styles.btnPrimary}
            onClick={handleSyncNow}
            disabled={busy !== null || status.syncing}
          >
            {busy === 'syncing' || status.syncing ? 'Syncing…' : 'Sync now'}
          </button>
          <button
            className={styles.btn}
            onClick={handleDisconnect}
            disabled={busy !== null || status.syncing}
          >
            Disconnect
          </button>
        </div>
        {message ? <div className={styles.message}>{message}</div> : null}
        {error || statusError || status.last_error ? (
          <div className={styles.error}>{error ?? statusError ?? status.last_error}</div>
        ) : null}
        {status.waiting_for_prerequisites ? (
          <div className={styles.hint}>
            Waiting for missing media or an earlier remote change. Picto will retry automatically.
          </div>
        ) : null}
        <div className={styles.hint}>
          Disconnecting only unlinks this device. It does not delete the remote library or its
          media.
        </div>
      </div>
    </div>
  );
}
