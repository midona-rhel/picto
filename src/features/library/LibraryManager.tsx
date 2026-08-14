import { useCallback, useEffect, useMemo, useState } from 'react';
import { IconX, IconCloud, IconBooks } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { IconPicker } from '../../shared/ui/IconPicker';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import {
  cloudSyncController,
  type RemoteLibraryInfo,
  type ShareRootCandidate,
} from '../../controllers/cloudSyncController';
import styles from './LibraryManager.module.css';

interface LibraryConfigResult {
  libraryHistory?: string[];
  pinnedLibraries?: string[];
  lastLibrary?: string | null;
  libraryMeta?: Record<string, { icon?: string | null; color?: string | null }>;
  currentPath: string | null;
  existsMap: Record<string, boolean>;
}

const pictoLibrary = () => (window as any).picto.library;

const PROVIDER_NAMES: Record<string, string> = {
  'google-drive': 'Google Drive',
  dropbox: 'Dropbox',
  icloud: 'iCloud',
  onedrive: 'OneDrive',
};

function shortServiceName(service: ShareRootCandidate): string {
  return PROVIDER_NAMES[service.provider] ?? service.label;
}

function baseName(path: string): string {
  const last = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  return last.endsWith('.library') ? last.slice(0, -'.library'.length) : last;
}

function parentDir(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  parts.pop();
  const sep = path.includes('\\') ? '\\' : '/';
  return (path.startsWith(sep) ? sep : '') + parts.join(sep);
}

export function LibraryManager() {
  const [config, setConfig] = useState<LibraryConfigResult | null>(null);
  const [services, setServices] = useState<ShareRootCandidate[]>([]);
  const [cloudLibraries, setCloudLibraries] = useState<Record<string, RemoteLibraryInfo[]>>({});
  const [cloudNames, setCloudNames] = useState<Record<string, string>>({});
  const [showLocalCreate, setShowLocalCreate] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [showIconEditor, setShowIconEditor] = useState(false);
  const [localName, setLocalName] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setConfig(await pictoLibrary().getConfig());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refreshCloud = useCallback(async () => {
    try {
      const detected = await cloudSyncController.detectShareRoots();
      setServices(detected);
      const byService: Record<string, RemoteLibraryInfo[]> = {};
      for (const service of detected) {
        try {
          byService[service.path] = await cloudSyncController.listRemoteLibraries(service.path);
        } catch {
          byService[service.path] = [];
        }
      }
      setCloudLibraries(byService);
    } catch (e) {
      setServices([]);
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    refreshCloud();
  }, [refresh, refreshCloud]);

  const localEntries = useMemo(() => {
    if (!config) return [];
    const pinned = config.pinnedLibraries ?? [];
    const history = config.libraryHistory ?? [];
    const ordered = [...pinned, ...history.filter((p) => !pinned.includes(p))];
    return ordered.map((path) => ({
      path,
      name: baseName(path),
      dir: parentDir(path),
      exists: config.existsMap[path] !== false,
      pinned: pinned.includes(path),
      current: config.currentPath === path,
      icon: config.libraryMeta?.[path]?.icon ?? null,
      color: config.libraryMeta?.[path]?.color ?? null,
    }));
  }, [config]);

  const run = useCallback(
    async (key: string, action: () => Promise<void>) => {
      setBusy(key);
      setError(null);
      setMessage(null);
      try {
        await action();
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  const switchTo = (path: string) =>
    run(`switch:${path}`, async () => {
      await pictoLibrary().switch(path);
    });

  const createLocal = () =>
    run('create-local', async () => {
      const name = localName.trim();
      if (!name) return;
      const result = await (window as any).picto.dialog.open({
        properties: ['openDirectory'],
        multiple: false,
        title: 'Choose where to store the library',
      });
      const dir = Array.isArray(result) ? result[0] : result;
      if (!dir) return;
      await pictoLibrary().create(name, dir);
      setLocalName('');
      setShowLocalCreate(false);
      setMessage(`Created "${name}".`);
    });

  /// Where to put a local copy of a cloud library: next to the current
  /// library if there is one, otherwise ask.
  const defaultLocalDir = useCallback(async (): Promise<string | null> => {
    const anchor = config?.currentPath ?? config?.lastLibrary ?? config?.libraryHistory?.[0];
    if (anchor) return parentDir(anchor);
    const result = await (window as any).picto.dialog.open({
      properties: ['openDirectory'],
      multiple: false,
      title: 'Choose where to keep this library on this device',
    });
    const dir = Array.isArray(result) ? result[0] : result;
    return dir ?? null;
  }, [config]);

  const openCloudLibrary = (service: ShareRootCandidate, remote: RemoteLibraryInfo) =>
    run(`cloud-open:${service.path}:${remote.name}`, async () => {
      // Already materialized on this device? Just switch to it.
      const local = localEntries.find((entry) => entry.name === remote.name && entry.exists);
      if (local) {
        if (!local.current) await pictoLibrary().switch(local.path);
        await cloudSyncController.connectRemoteLibrary(service.path, remote.name);
        setMessage(`Opened "${remote.name}" — synced with ${service.label}.`);
        return;
      }
      const dir = await defaultLocalDir();
      if (!dir) return;
      await pictoLibrary().create(remote.name, dir);
      await cloudSyncController.connectRemoteLibrary(service.path, remote.name);
      setMessage(`Opened "${remote.name}" from ${service.label}. Syncing…`);
    });

  /// One click on a local library: publish it to a cloud service and keep
  /// it synced. Switches to the library first (sync binds to the open one).
  const publishLibrary = (entry: { path: string; name: string; current: boolean }, service: ShareRootCandidate) =>
    run(`publish:${entry.path}`, async () => {
      if (!entry.current) await pictoLibrary().switch(entry.path);
      await cloudSyncController.createRemoteLibrary(service.path, entry.name);
      setMessage(`"${entry.name}" now syncs to ${service.label}.`);
      await refreshCloud();
    });

  const createCloudLibrary = (service: ShareRootCandidate) =>
    run(`cloud-create:${service.path}`, async () => {
      const name = (cloudNames[service.path] ?? '').trim();
      if (!name) return;
      const dir = await defaultLocalDir();
      if (!dir) return;
      await pictoLibrary().create(name, dir);
      await cloudSyncController.createRemoteLibrary(service.path, name);
      setCloudNames((prev) => ({ ...prev, [service.path]: '' }));
      setMessage(`Created "${name}" on ${service.label}.`);
      await refreshCloud();
    });

  return (
    <div className={styles.root}>
      <div className={styles.header}>
        <div className={styles.title}>Libraries</div>
        <button className={styles.closeBtn} onClick={() => window.close()} aria-label="Close">
          <IconX size={14} />
        </button>
      </div>
      <div className={styles.body}>
        <div>
          <div className={styles.sectionTitle}>On this device</div>
          {localEntries.length === 0 ? (
            <div className={styles.empty}>No libraries yet — create one below.</div>
          ) : (
            <div className={styles.list}>
              {localEntries.map((entry) => (
                <div key={entry.path}>
                <div
                  className={
                    selectedPath === entry.path
                      ? styles.rowSelected
                      : entry.current
                        ? styles.rowCurrent
                        : entry.exists
                          ? styles.row
                          : styles.rowDisabled
                  }
                  onClick={() => {
                    setSelectedPath((prev) => (prev === entry.path ? null : entry.path));
                    setShowIconEditor(false);
                  }}
                  onDoubleClick={() => entry.exists && !entry.current && switchTo(entry.path)}
                  role="button"
                  tabIndex={0}
                >
                  <span className={styles.rowIcon} style={entry.color ? { color: entry.color } : undefined}>
                    {entry.icon ? (
                      <DynamicIcon name={entry.icon} size={16} color={entry.color} />
                    ) : (
                      <IconBooks size={16} />
                    )}
                  </span>
                  <div className={styles.rowMain}>
                    <span className={styles.rowName}>{entry.name}</span>
                    <span className={styles.rowPath}>{entry.dir}</span>
                  </div>
                  {entry.pinned ? <span className={styles.badge}>Pinned</span> : null}
                  {entry.current ? <span className={styles.badgeCurrent}>Current</span> : null}
                  {!entry.exists ? <span className={styles.badgeMissing}>Missing</span> : null}
                </div>
                {selectedPath === entry.path ? (
                  <div className={styles.detail}>
                    <div className={styles.detailTop}>
                      {!entry.current && entry.exists ? (
                        <button
                          className={styles.btnPrimary}
                          onClick={() => switchTo(entry.path)}
                          disabled={busy !== null}
                        >
                          Open
                        </button>
                      ) : null}
                      <span className={styles.detailSpacer} />
                      <button
                        className={styles.linkBtn}
                        onClick={() => run(`pin:${entry.path}`, () => pictoLibrary().togglePin(entry.path))}
                        disabled={busy !== null}
                      >
                        {entry.pinned ? 'Unpin' : 'Pin'}
                      </button>
                      <button
                        className={styles.linkDanger}
                        disabled={busy !== null || entry.current}
                        onClick={() => {
                          if (
                            window.confirm(
                              `Remove "${entry.name}" from this list?\n\nThe library and all its files stay on disk — this only hides it here. You can bring it back with Open Existing….`,
                            )
                          ) {
                            setSelectedPath(null);
                            void run(`remove:${entry.path}`, () => pictoLibrary().remove(entry.path));
                          }
                        }}
                      >
                        Remove from list
                      </button>
                    </div>
                    <div className={styles.detailGrid}>
                      <span className={styles.detailLabel}>Sync to</span>
                      <span className={styles.detailControls}>
                        {services.map((service) => (
                          <button
                            key={service.path}
                            className={styles.btn}
                            title={service.label}
                            onClick={() => publishLibrary(entry, service)}
                            disabled={busy !== null || !entry.exists}
                          >
                            {shortServiceName(service)}
                          </button>
                        ))}
                      </span>
                      <span className={styles.detailLabel}>Color</span>
                      <span className={styles.detailControls}>
                        <span className={styles.colorConstraint}>
                        <ColorPicker
                          value={entry.color}
                          onChange={(color) =>
                            void run(`meta:${entry.path}`, () => pictoLibrary().setMeta(entry.path, { color }))
                          }
                        />
                        </span>
                      </span>
                      <span className={styles.detailLabel}>Icon</span>
                      <span className={styles.detailControls}>
                        <button
                          className={styles.btn}
                          onClick={() => setShowIconEditor((v) => !v)}
                          disabled={busy !== null}
                        >
                          {showIconEditor ? 'Done' : 'Change…'}
                        </button>
                      </span>
                    </div>
                    {showIconEditor ? (
                      <div className={styles.iconEditor}>
                        <IconPicker
                          value={entry.icon}
                          onChange={(icon) =>
                            void run(`meta:${entry.path}`, () => pictoLibrary().setMeta(entry.path, { icon }))
                          }
                        />
                      </div>
                    ) : null}
                  </div>
                ) : null}
                </div>
              ))}
            </div>
          )}
          <div className={styles.actionsRow}>
            <button
              className={styles.btnPrimary}
              onClick={() => setShowLocalCreate((v) => !v)}
              disabled={busy !== null}
            >
              New Library…
            </button>
            <button
              className={styles.btn}
              onClick={() => run('open-existing', () => pictoLibrary().open())}
              disabled={busy !== null}
            >
              Open Existing…
            </button>
          </div>
          {showLocalCreate ? (
            <div className={styles.inlineForm}>
              <input
                className={styles.nameInput}
                placeholder="Library name (e.g. My Inspirations)"
                value={localName}
                onChange={(e) => setLocalName(e.target.value)}
                autoFocus
              />
              <button
                className={styles.btn}
                onClick={createLocal}
                disabled={busy !== null || !localName.trim()}
              >
                {busy === 'create-local' ? 'Creating…' : 'Create'}
              </button>
            </div>
          ) : null}
        </div>

        <div>
          <div className={styles.sectionTitle}>Cloud</div>
          {services.length === 0 ? (
            <div className={styles.empty}>
              No sync service detected on this computer. Picto syncs through the Google Drive or
              Dropbox app you already use — there is nothing to sign into inside Picto. Install
              Google Drive for Desktop or Dropbox and libraries can sync automatically.
            </div>
          ) : (
          <div className={styles.cloudList}>
            {services.map((service) => {
              const libraries = cloudLibraries[service.path] ?? [];
              return (
                <div key={service.path} className={styles.serviceBlock}>
                  <div className={styles.serviceHeader}>
                    <IconCloud size={14} />
                    {service.label}
                  </div>
                  {libraries.length === 0 ? (
                    <div className={styles.empty}>No Picto libraries here yet.</div>
                  ) : (
                    <div className={styles.list}>
                      {libraries.map((remote) => {
                        const local = localEntries.find(
                          (entry) => entry.name === remote.name && entry.exists,
                        );
                        return (
                          <div key={remote.name} className={styles.row}>
                            <div className={styles.rowMain}>
                              <span className={styles.rowName}>{remote.name}</span>
                              <span className={styles.rowPath}>
                                {remote.valid
                                  ? `Created ${remote.created_at?.slice(0, 10) ?? '—'}${local ? ' · on this device' : ''}`
                                  : 'Invalid library data'}
                              </span>
                            </div>
                            <button
                              className={styles.btn}
                              onClick={() => openCloudLibrary(service, remote)}
                              disabled={busy !== null || !remote.valid}
                            >
                              {busy === `cloud-open:${service.path}:${remote.name}`
                                ? 'Opening…'
                                : local?.current
                                  ? 'Sync'
                                  : 'Open'}
                            </button>
                          </div>
                        );
                      })}
                    </div>
                  )}
                  <div className={styles.inlineForm}>
                    <input
                      className={styles.nameInput}
                      placeholder={`New library on ${service.label}`}
                      value={cloudNames[service.path] ?? ''}
                      onChange={(e) =>
                        setCloudNames((prev) => ({ ...prev, [service.path]: e.target.value }))
                      }
                    />
                    <button
                      className={styles.btn}
                      onClick={() => createCloudLibrary(service)}
                      disabled={busy !== null || !(cloudNames[service.path] ?? '').trim()}
                    >
                      {busy === `cloud-create:${service.path}` ? 'Creating…' : 'Create'}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
          )}
          <div className={styles.hint}>
            No sign-in needed — Picto syncs through the cloud app already on this computer.
            Cloud libraries live under a Picto folder on the service and stay in sync on every
            device that opens them. Picto never deletes or overwrites a library on the share.
          </div>
        </div>

        {message ? <div className={styles.message}>{message}</div> : null}
        {error ? <div className={styles.error}>{error}</div> : null}
      </div>
    </div>
  );
}
