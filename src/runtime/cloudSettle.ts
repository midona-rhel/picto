import { invoke } from '../platform/ipc';
import {
  showErrorNotification,
  showInfoNotification,
  showSuccessNotification,
} from '../shared/lib/notifications';
import { libraryInvalidation } from './libraryInvalidation';
import type { CloudSyncStatus } from '../shared/types/generated/application/CloudSyncStatus';

let previous: CloudSyncStatus | null = null;
let refreshInFlight: Promise<void> | null = null;
let reconnectStartedAt = 0;

function viewProgress() {
  window.dispatchEvent(new CustomEvent('picto:open-diagnostics'));
}

async function refresh(): Promise<void> {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = invoke<CloudSyncStatus>('cloud.status.get')
    .then((next) => {
      const before = previous;
      previous = next;
      if (!before) return;

      if (next.state === 'reconciling' && !next.blocking && before.state !== 'reconciling') {
        reconnectStartedAt = Date.now();
        showInfoNotification({
          title: 'Cloud changes found',
          message: 'Picto is updating your library.',
          action: { label: 'View progress', onClick: viewProgress },
        });
        return;
      }

      if (next.state === 'error' && (before.state !== 'error' || before.message !== next.message)) {
        showErrorNotification({
          title: 'Cloud sync needs attention',
          message: next.message,
          action: { label: 'View details', onClick: viewProgress },
        });
        return;
      }

      if (before.state === 'reconciling' && next.state === 'idle') {
        const elapsed = reconnectStartedAt ? Date.now() - reconnectStartedAt : 0;
        reconnectStartedAt = 0;
        if (elapsed >= 5_000) {
          showSuccessNotification({
            title: 'Cloud update complete',
            message: 'Your library is up to date.',
          });
        }
      }
    })
    .catch(() => {
      // Library shutdown and transient folder unavailability are represented
      // by persisted status on the next successful query.
    })
    .finally(() => {
      refreshInFlight = null;
    });
  return refreshInFlight;
}

export function startCloudSettle(): () => void {
  let stopped = false;
  void refresh();
  const unregister = libraryInvalidation.register('cloud', () => {
    if (!stopped) void refresh();
  });
  return () => {
    stopped = true;
    unregister();
    previous = null;
    reconnectStartedAt = 0;
  };
}
