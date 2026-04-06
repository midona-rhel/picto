import { listen } from '../platform/ipc';

const appSettingsReloadCallbacks = new Set<() => void>();

export function registerAppSettingsReload(callback: () => void): () => void {
  appSettingsReloadCallbacks.add(callback);
  return () => {
    appSettingsReloadCallbacks.delete(callback);
  };
}

export function startAppSettingsSettle(): () => void {
  let cancelled = false;
  const unlistenPromise = listen<{ changes?: { view_prefs_changed?: boolean } }>(
    'runtime/state_changed',
    (event) => {
      if (cancelled) return;
      if (!event.payload.changes?.view_prefs_changed) return;
      for (const callback of appSettingsReloadCallbacks) {
        try {
          callback();
        } catch (error) {
          console.error('app settings settle callback failed', error);
        }
      }
    },
  );

  return () => {
    cancelled = true;
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
