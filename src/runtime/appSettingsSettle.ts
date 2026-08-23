import { libraryInvalidation } from './libraryInvalidation';

const appSettingsReloadCallbacks = new Set<() => void>();

export function registerAppSettingsReload(callback: () => void): () => void {
  appSettingsReloadCallbacks.add(callback);
  return () => {
    appSettingsReloadCallbacks.delete(callback);
  };
}

export function startAppSettingsSettle(): () => void {
  const unregister = libraryInvalidation.register('settings', () => {
    for (const callback of appSettingsReloadCallbacks) {
      try {
        callback();
      } catch (error) {
        console.error('app settings settle callback failed', error);
      }
    }
  });
  libraryInvalidation.start();

  return () => {
    unregister();
    libraryInvalidation.stop();
  };
}
