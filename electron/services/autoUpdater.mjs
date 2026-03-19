import pkg from 'electron-updater';
const { autoUpdater } = pkg;

/**
 * Auto-update service using electron-updater with GitHub releases.
 *
 * Works on all platforms:
 * - macOS: Uses zip target (dmg cannot auto-update; zip is required)
 * - Windows: Uses nsis installer
 * - Linux: Uses AppImage (deb does not support auto-update)
 *
 * The updater checks GitHub releases for a `latest-{platform}.yml` file
 * that electron-builder generates automatically during `--publish`.
 */
export function createAutoUpdaterService({ app, isDev, sendToAllWindows }) {
  // Don't run in dev — no packaged app to update
  if (isDev) {
    return {
      checkForUpdates: () => Promise.resolve(null),
      downloadUpdate: () => Promise.resolve(),
      quitAndInstall: () => {},
    };
  }

  autoUpdater.autoDownload = false;
  autoUpdater.autoInstallOnAppQuit = true;

  // Forward all updater events to renderer windows
  autoUpdater.on('checking-for-update', () => {
    sendToAllWindows('updater:status', { status: 'checking' });
  });

  autoUpdater.on('update-available', (info) => {
    sendToAllWindows('updater:status', {
      status: 'available',
      version: info.version,
      releaseNotes: info.releaseNotes ?? null,
      releaseDate: info.releaseDate ?? null,
    });
  });

  autoUpdater.on('update-not-available', (info) => {
    sendToAllWindows('updater:status', {
      status: 'up-to-date',
      version: info.version,
    });
  });

  autoUpdater.on('download-progress', (progress) => {
    sendToAllWindows('updater:status', {
      status: 'downloading',
      percent: progress.percent,
      bytesPerSecond: progress.bytesPerSecond,
      transferred: progress.transferred,
      total: progress.total,
    });
  });

  autoUpdater.on('update-downloaded', (info) => {
    sendToAllWindows('updater:status', {
      status: 'ready',
      version: info.version,
    });
  });

  autoUpdater.on('error', (err) => {
    console.error('[auto-updater] Error:', err?.message ?? err);
    sendToAllWindows('updater:status', {
      status: 'error',
      error: err?.message ?? String(err),
    });
  });

  // Check once on startup (after a short delay so the window is ready)
  const STARTUP_CHECK_DELAY_MS = 10_000;
  setTimeout(() => {
    autoUpdater.checkForUpdates().catch((err) => {
      console.warn('[auto-updater] Startup check failed:', err?.message ?? err);
    });
  }, STARTUP_CHECK_DELAY_MS);

  // Periodic check every 4 hours
  const PERIODIC_CHECK_MS = 4 * 60 * 60 * 1000;
  setInterval(() => {
    autoUpdater.checkForUpdates().catch((err) => {
      console.warn('[auto-updater] Periodic check failed:', err?.message ?? err);
    });
  }, PERIODIC_CHECK_MS);

  return {
    checkForUpdates: () => autoUpdater.checkForUpdates(),
    downloadUpdate: () => autoUpdater.downloadUpdate(),
    quitAndInstall: () => autoUpdater.quitAndInstall(),
  };
}
