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
      checkAndUpdateOnStartup: () => Promise.resolve(),
    };
  }

  autoUpdater.autoDownload = true;
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

  // Periodic check every 4 hours (startup check is handled by checkAndUpdateOnStartup)
  const PERIODIC_CHECK_MS = 4 * 60 * 60 * 1000;
  setInterval(() => {
    autoUpdater.checkForUpdates().catch((err) => {
      console.warn('[auto-updater] Periodic check failed:', err?.message ?? err);
    });
  }, PERIODIC_CHECK_MS);

  /**
   * Blocking startup update check.
   * If an update is found within the timeout, downloads and installs it
   * (app restarts). If no update or timeout, resolves and app continues.
   */
  async function checkAndUpdateOnStartup(timeoutMs = 3000) {
    try {
      const result = await Promise.race([
        autoUpdater.checkForUpdates(),
        new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
      ]);

      if (!result || !result.updateInfo || result.updateInfo.version === app.getVersion()) {
        console.info('[auto-updater] No update available or timed out, proceeding');
        return;
      }

      console.info(`[auto-updater] Update ${result.updateInfo.version} found, downloading...`);

      // autoDownload=true means download already started — wait for it
      await new Promise((resolve, reject) => {
        autoUpdater.once('update-downloaded', resolve);
        autoUpdater.once('error', reject);
      });

      console.info('[auto-updater] Update downloaded, installing and restarting...');
      autoUpdater.quitAndInstall();
    } catch (err) {
      console.warn('[auto-updater] Startup update failed, proceeding:', err?.message ?? err);
    }
  }

  return {
    checkForUpdates: () => autoUpdater.checkForUpdates(),
    downloadUpdate: () => autoUpdater.downloadUpdate(),
    quitAndInstall: () => autoUpdater.quitAndInstall(),
    checkAndUpdateOnStartup,
  };
}
