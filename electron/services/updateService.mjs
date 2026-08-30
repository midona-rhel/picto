import { shell } from 'electron';

const RELEASES_API = 'https://api.github.com/repos/midona-rhel/picto/releases?per_page=20';
const RELEASES_PAGE = 'https://github.com/midona-rhel/picto/releases';

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

export function createUpdateService({ app, net, sendToAllWindows, platform = process.platform }) {
  let updater = null;
  let checkPromise = null;
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
    releaseName: info?.releaseName ?? null,
    releaseDate: info?.releaseDate ?? null,
    releaseNotes: releaseNotes(info?.releaseNotes),
  });

  async function checkMac() {
    const response = await net.fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'Picto-Updater' },
    });
    if (!response.ok) throw new Error(`Release check failed (${response.status})`);
    const releases = await response.json();
    const release = releases.find((entry) => !entry.draft && isNewer(entry.tag_name, app.getVersion()));
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
    const module = await import('electron-updater');
    updater = module.autoUpdater;
    updater.autoDownload = true;
    updater.autoInstallOnAppQuit = true;
    updater.allowPrerelease = true;
    updater.fullChangelog = true;
    updater.logger = console;
    updater.on('checking-for-update', () => publish({ status: 'checking', error: null }));
    updater.on('update-available', (info) => publish({ status: 'downloading', ...normalizeInfo(info), error: null }));
    updater.on('update-not-available', () => publish({ status: 'current', error: null }));
    updater.on('download-progress', (progress) => publish({
      status: 'downloading',
      progress: { percent: progress.percent, transferred: progress.transferred, total: progress.total },
    }));
    updater.on('update-downloaded', (info) => publish({
      status: 'downloaded',
      ...normalizeInfo(info),
      progress: { percent: 100, transferred: info.downloadedFile ? 1 : 0, total: 1 },
      error: null,
    }));
    updater.on('error', (error) => publish({ status: 'error', error: error?.message ?? String(error) }));
    return updater;
  }

  async function check() {
    if (!app.isPackaged) return state;
    if (checkPromise) return checkPromise;
    checkPromise = (async () => {
      publish({ status: 'checking', error: null });
      try {
        if (platform === 'darwin') return await checkMac();
        const activeUpdater = await ensureUpdater();
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
    if (platform === 'darwin') return shell.openExternal(state.releaseUrl || RELEASES_PAGE);
    if (state.status !== 'downloaded') throw new Error('The update has not finished downloading.');
    const activeUpdater = await ensureUpdater();
    setImmediate(() => activeUpdater.quitAndInstall(false, true));
  }

  function start() {
    if (!app.isPackaged) return;
    const initial = setTimeout(() => void check(), 15_000);
    initial.unref?.();
    const interval = setInterval(() => void check(), 6 * 60 * 60 * 1000);
    interval.unref?.();
  }

  return { check, getState: () => state, install, openRelease: () => shell.openExternal(state.releaseUrl || RELEASES_PAGE), start };
}
