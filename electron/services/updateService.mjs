import { shell } from 'electron';
import fs from 'node:fs/promises';
import path from 'node:path';

const RELEASES_API = 'https://api.github.com/repos/midona-rhel/picto/releases?per_page=20';
const RELEASES_PAGE = 'https://github.com/midona-rhel/picto/releases';
const RELEASE_DOWNLOADS = 'https://github.com/midona-rhel/picto/releases/download';
const APP_RELEASE_TAG = /^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?$/;
const PENDING_RELEASE_FILE = 'pending-update-release.json';

function releaseNotes(value) {
  if (Array.isArray(value)) return value.map((entry) => entry.note).filter(Boolean).join('\n\n');
  return typeof value === 'string' ? value : '';
}

function versionParts(value) {
  return String(value).replace(/^v/, '').split(/[.-]/).map((part) => /^\d+$/.test(part) ? Number(part) : part);
}

function isNewer(candidate, current) {
  const left = versionParts(candidate);
  const right = versionParts(current);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const a = left[index] ?? 0;
    const b = right[index] ?? 0;
    if (a === b) continue;
    if (typeof a === 'number' && typeof b === 'number') return a > b;
    if (typeof a === 'number') return true;
    if (typeof b === 'number') return false;
    return String(a).localeCompare(String(b)) > 0;
  }
  return false;
}

function isAppRelease(release) {
  return !release?.draft && APP_RELEASE_TAG.test(String(release.tag_name ?? ''));
}

export function createUpdateService({
  app,
  net,
  sendToAllWindows,
  platform = process.platform,
  loadUpdaterModule = () => import('electron-updater'),
}) {
  let updater = null;
  let checkPromise = null;
  let selectedRelease = null;
  const pendingReleasePath = () => path.join(app.getPath('userData'), PENDING_RELEASE_FILE);
  let state = {
    status: app.isPackaged ? 'idle' : 'unavailable',
    currentVersion: app.getVersion(),
    platform,
    automaticInstall: platform !== 'darwin',
    version: null,
    releaseName: null,
    releaseDate: null,
    releaseNotes: '',
    releaseUrl: RELEASES_PAGE,
    progress: null,
    error: app.isPackaged ? null : 'Update checks are available in packaged builds.',
  };

  const publish = (patch) => {
    state = { ...state, ...patch };
    sendToAllWindows('picto:update-state', state);
    return state;
  };

  const normalizeInfo = (info) => ({
    version: info?.version ?? null,
    releaseName: info?.releaseName ?? selectedRelease?.name ?? null,
    releaseDate: info?.releaseDate ?? selectedRelease?.published_at ?? null,
    releaseNotes: releaseNotes(info?.releaseNotes) || selectedRelease?.body || '',
  });

  const releaseSnapshot = (source = state) => ({
    version: source.version,
    releaseName: source.releaseName,
    releaseDate: source.releaseDate,
    releaseNotes: source.releaseNotes,
    releaseUrl: source.releaseUrl,
  });

  async function persistPendingRelease(source = state) {
    const snapshot = releaseSnapshot(source);
    if (!snapshot.version) return;
    await fs.mkdir(app.getPath('userData'), { recursive: true });
    await fs.writeFile(pendingReleasePath(), JSON.stringify(snapshot), 'utf8');
  }

  async function restoreInstalledRelease() {
    let pending;
    try {
      pending = JSON.parse(await fs.readFile(pendingReleasePath(), 'utf8'));
    } catch {
      return false;
    }
    if (!pending?.version || pending.version !== app.getVersion()) {
      if (pending?.version && isNewer(app.getVersion(), pending.version)) {
        await fs.rm(pendingReleasePath(), { force: true });
      }
      return false;
    }
    publish({
      status: 'installed',
      version: pending.version,
      releaseName: typeof pending.releaseName === 'string' ? pending.releaseName : null,
      releaseDate: typeof pending.releaseDate === 'string' ? pending.releaseDate : null,
      releaseNotes: typeof pending.releaseNotes === 'string' ? pending.releaseNotes : '',
      releaseUrl: typeof pending.releaseUrl === 'string' ? pending.releaseUrl : RELEASES_PAGE,
      progress: null,
      error: null,
    });
    return true;
  }

  async function latestAppRelease() {
    const response = await net.fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'Picto-Updater' },
    });
    if (!response.ok) throw new Error(`Release check failed (${response.status})`);
    const releases = await response.json();
    return releases.find((entry) => isAppRelease(entry) && isNewer(entry.tag_name, app.getVersion())) ?? null;
  }

  async function checkMac() {
    const release = await latestAppRelease();
    if (!release) return publish({ status: 'current', error: null });
    return publish({
      status: 'available',
      version: String(release.tag_name).replace(/^v/, ''),
      releaseName: release.name || null,
      releaseDate: release.published_at || null,
      releaseNotes: release.body || '',
      releaseUrl: release.html_url || RELEASES_PAGE,
      error: null,
    });
  }

  async function ensureUpdater() {
    if (updater) return updater;
    const module = await loadUpdaterModule();
    const updaterModule = module?.default ?? module;
    updater = updaterModule?.autoUpdater ?? module?.autoUpdater;
    if (!updater) throw new Error('The packaged update service is unavailable.');
    updater.autoDownload = true;
    updater.autoInstallOnAppQuit = true;
    // GitHub release assets do not support electron-updater's multi-range
    // differential request reliably. Download the Windows installer directly.
    updater.disableDifferentialDownload = platform === 'win32';
    updater.allowPrerelease = true;
    updater.fullChangelog = true;
    updater.logger = console;
    updater.on('checking-for-update', () => publish({ status: 'checking', error: null }));
    updater.on('update-available', (info) => publish({
      status: 'available',
      ...normalizeInfo(info),
      progress: null,
      error: null,
    }));
    updater.on('update-not-available', () => publish({ status: 'current', error: null }));
    updater.on('download-progress', (progress) => publish({
      status: 'downloading',
      progress: { percent: progress.percent, transferred: progress.transferred, total: progress.total },
    }));
    updater.on('update-downloaded', (info) => {
      const downloaded = publish({
        status: 'downloaded',
        ...normalizeInfo(info),
        progress: { percent: 100, transferred: info.downloadedFile ? 1 : 0, total: 1 },
        error: null,
      });
      void persistPendingRelease(downloaded).catch((error) => {
        console.error('[updates] could not preserve release notes:', error);
      });
    });
    updater.on('error', (error) => publish({ status: 'error', error: error?.message ?? String(error) }));
    return updater;
  }

  async function check() {
    if (!app.isPackaged) return state;
    if (state.status === 'installed') return state;
    if (checkPromise) return checkPromise;
    checkPromise = (async () => {
      publish({ status: 'checking', error: null });
      try {
        if (platform === 'darwin') return await checkMac();
        selectedRelease = await latestAppRelease();
        if (!selectedRelease) return publish({ status: 'current', error: null });
        const activeUpdater = await ensureUpdater();
        activeUpdater.channel = 'latest';
        activeUpdater.setFeedURL({
          provider: 'generic',
          url: `${RELEASE_DOWNLOADS}/${encodeURIComponent(selectedRelease.tag_name)}/`,
          channel: 'latest',
        });
        await activeUpdater.checkForUpdates();
        return state;
      } catch (error) {
        return publish({ status: 'error', error: error?.message ?? String(error) });
      } finally {
        checkPromise = null;
      }
    })();
    return checkPromise;
  }

  async function install() {
    if (platform === 'darwin') return openRelease();
    if (state.status !== 'downloaded') throw new Error('The update has not finished downloading.');
    await persistPendingRelease();
    const activeUpdater = await ensureUpdater();
    setImmediate(() => activeUpdater.quitAndInstall(false, true));
  }

  async function openRelease() {
    if (state.version) await persistPendingRelease();
    return shell.openExternal(state.releaseUrl || RELEASES_PAGE);
  }

  async function acknowledgeInstalled() {
    if (state.status !== 'installed') return state;
    await fs.rm(pendingReleasePath(), { force: true });
    return publish({
      status: 'current',
      version: null,
      releaseName: null,
      releaseDate: null,
      releaseNotes: '',
      releaseUrl: RELEASES_PAGE,
      progress: null,
      error: null,
    });
  }

  async function start() {
    if (!app.isPackaged) return;
    await restoreInstalledRelease();
    const initial = setTimeout(() => void check(), 15_000);
    initial.unref?.();
    const interval = setInterval(() => void check(), 6 * 60 * 60 * 1000);
    interval.unref?.();
  }

  return { acknowledgeInstalled, check, getState: () => state, install, openRelease, start };
}
