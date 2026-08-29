import pkg from 'electron-updater';
const { autoUpdater } = pkg;

function errorMessage(error) {
  return error?.message ?? String(error);
}

export function isMissingUpdateMetadataError(error) {
  const message = errorMessage(error);
  return /\b404\b/.test(message)
    && /Cannot find latest(?:-[\w]+)?\.ya?ml\b/i.test(message);
}

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
export function createAutoUpdaterService({ app, isDev, isSmoke = false, sendToAllWindows, updater = autoUpdater }) {
  // Smoke launches are intentionally offline and must exercise app startup, not update delivery.
  if (isDev || isSmoke) {
    return {
      checkForUpdates: () => Promise.resolve(null),
      downloadUpdate: () => Promise.resolve(),
      quitAndInstall: () => {},
      checkAndUpdateOnStartup: () => Promise.resolve(),
    };
  }

  updater.logger = null;
  updater.autoDownload = true;
  updater.autoInstallOnAppQuit = true;
  updater.allowPrerelease = true;

  let lastReportedError = null;
  let lastReportedAt = 0;
  const reportUpdateError = (error) => {
    if (isMissingUpdateMetadataError(error)) return;
    const message = errorMessage(error);
    const now = Date.now();
    if (message === lastReportedError && now - lastReportedAt < 1000) return;
    lastReportedError = message;
    lastReportedAt = now;
    console.error('[auto-updater] Error:', message);
    sendToAllWindows('updater:status', { status: 'error', error: message });
  };

  // Forward all updater events to renderer windows
  updater.on('checking-for-update', () => {
    sendToAllWindows('updater:status', { status: 'checking' });
  });

  updater.on('update-available', (info) => {
    sendToAllWindows('updater:status', {
      status: 'available',
      version: info.version,
      releaseNotes: info.releaseNotes ?? null,
      releaseDate: info.releaseDate ?? null,
    });
  });

  updater.on('update-not-available', (info) => {
    sendToAllWindows('updater:status', {
      status: 'up-to-date',
      version: info.version,
    });
  });

  updater.on('download-progress', (progress) => {
    sendToAllWindows('updater:status', {
      status: 'downloading',
      percent: progress.percent,
      bytesPerSecond: progress.bytesPerSecond,
      transferred: progress.transferred,
      total: progress.total,
    });
  });

  updater.on('update-downloaded', (info) => {
    sendToAllWindows('updater:status', {
      status: 'ready',
      version: info.version,
    });
  });

  updater.on('error', reportUpdateError);

  // Periodic check every 4 hours (startup check is handled by checkAndUpdateOnStartup)
  const PERIODIC_CHECK_MS = 4 * 60 * 60 * 1000;
  setInterval(() => {
    updater.checkForUpdates().catch(reportUpdateError);
  }, PERIODIC_CHECK_MS);

  /**
   * Blocking startup update check.
   * If an update is found within the timeout, downloads and installs it
   * (app restarts). If no update or timeout, resolves and app continues.
   */
  async function checkAndUpdateOnStartup(timeoutMs = 3000) {
    try {
      const result = await Promise.race([
        updater.checkForUpdates(),
        new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
      ]);

      if (!result || !result.updateInfo || result.updateInfo.version === app.getVersion()) {
        console.info('[auto-updater] No update available or timed out, proceeding');
        return;
      }

      console.info(`[auto-updater] Update ${result.updateInfo.version} found, downloading...`);

      // autoDownload=true means download already started — wait for it
      await new Promise((resolve, reject) => {
        updater.once('update-downloaded', resolve);
        updater.once('error', reject);
      });

      console.info('[auto-updater] Update downloaded, installing and restarting...');
      updater.quitAndInstall();
    } catch (err) {
      reportUpdateError(err);
    }
  }

  return {
    checkForUpdates: () => updater.checkForUpdates(),
    downloadUpdate: () => updater.downloadUpdate(),
    quitAndInstall: () => updater.quitAndInstall(),
    checkAndUpdateOnStartup,
  };
}
