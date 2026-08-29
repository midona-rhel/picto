import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  IconAlertTriangle,
  IconBrandDropbox,
  IconBrandGoogleDrive,
  IconBooks,
  IconCheck,
  IconCloud,
  IconCloudOff,
  IconDotsVertical,
  IconFolderOpen,
  IconPencil,
  IconPin,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlus,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import { LibraryAvatar } from './LibraryAvatar';
import { IconPicker } from '../../shared/ui/IconPicker';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { ContextMenu, type MenuEntry, useContextMenu } from '../../shared/ui/ContextMenu';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { invoke, listen } from '../../platform/ipc';
import { MediaCoverDialog } from '../subscriptions/components/SubscriptionCoverDialog';
import { loadLibraryCoverCandidates, saveLibraryCover } from './libraryAppearance';
import type { CloudConfiguration } from '../../shared/types/generated/application/CloudConfiguration';
import type { CloudSyncStatus } from '../../shared/types/generated/application/CloudSyncStatus';
import type { LibraryChanged } from '../../shared/types/generated/application/LibraryChanged';
import type { LibraryStatistics } from '../../shared/types/generated/application/LibraryStatistics';
import {
  estimateRemainingSeconds,
  formatLastSync,
  formatRemainingTime,
  presentCloudSync,
  type SyncRateSample,
} from './librarySyncPresentation';
import styles from './LibraryManager.module.css';

interface LibraryConfigResult {
  libraryHistory?: string[];
  pinnedLibraries?: string[];
  lastLibrary?: string | null;
  libraryMeta?: Record<string, {
    icon?: string | null;
    color?: string | null;
    imageHash?: string | null;
    imageFocusX?: number | null;
    imageFocusY?: number | null;
    imageZoomPercent?: number | null;
    cloudLibraryId?: string | null;
  }>;
  currentPath: string | null;
  libraryFailure?: { path?: string | null; message: string } | null;
  existsMap: Record<string, boolean>;
}

interface DetectedCloudRoot {
  provider: 'google_drive' | 'dropbox';
  account_label: string;
  path: string;
}

interface CloudLibraryChoice {
  root: DetectedCloudRoot;
  library_id: string;
  name: string;
  schema_generation: number;
  created_at: string;
}

const pictoLibrary = () => (window as any).picto.library;
const pictoShell = () => (window as any).picto.shell;

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

function formatBytes(value: number): string {
  if (value < 1024) return `${value.toLocaleString()} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let amount = value / 1024;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${units[unit]}`;
}

export function LibraryManager() {
  const [config, setConfig] = useState<LibraryConfigResult | null>(null);
  const [showLocalCreate, setShowLocalCreate] = useState(false);
  const [showCloudOpen, setShowCloudOpen] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [showIconEditor, setShowIconEditor] = useState(false);
  const [localName, setLocalName] = useState('');
  const [cloudName, setCloudName] = useState('');
  const [selectedCloudLibrary, setSelectedCloudLibrary] = useState<CloudLibraryChoice | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [coverPath, setCoverPath] = useState<string | null>(null);
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [cloudRoots, setCloudRoots] = useState<DetectedCloudRoot[]>([]);
  const [cloudConfiguration, setCloudConfiguration] = useState<CloudConfiguration | null>(null);
  const [cloudStatus, setCloudStatus] = useState<CloudSyncStatus | null>(null);
  const [libraryStatistics, setLibraryStatistics] = useState<LibraryStatistics | null>(null);
  const [cloudLibraries, setCloudLibraries] = useState<CloudLibraryChoice[]>([]);
  const [syncEtaSeconds, setSyncEtaSeconds] = useState<number | null>(null);
  const [initialLoadComplete, setInitialLoadComplete] = useState(false);
  const syncRateSamples = useRef<SyncRateSample[]>([]);
  const syncWorkKey = useRef<string | null>(null);
  const didRequestWindowShow = useRef(false);
  const libraryMenu = useContextMenu();

  const refresh = useCallback(async (): Promise<LibraryConfigResult | null> => {
    try {
      const nextConfig = await pictoLibrary().getConfig();
      setConfig(nextConfig);
      if (nextConfig.libraryFailure?.message) setError(nextConfig.libraryFailure.message);
      return nextConfig;
    } catch (e) {
      setError(String(e));
      return null;
    }
  }, []);

  const refreshCloudStatus = useCallback(async () => {
    try {
      const [configuration, status] = await Promise.all([
        invoke<CloudConfiguration>('cloud.configuration.get'),
        invoke<CloudSyncStatus>('cloud.status.get'),
      ]);
      setCloudConfiguration(configuration);
      setCloudStatus(status);
    } catch {
      setCloudConfiguration(null);
      setCloudStatus(null);
    }
  }, []);

  const refreshLibraryStatistics = useCallback(async () => {
    try {
      setLibraryStatistics(await invoke<LibraryStatistics>('library.stats'));
    } catch {
      setLibraryStatistics(null);
    }
  }, []);

  const refreshCloud = useCallback(async () => {
    try {
      const roots = await invoke<DetectedCloudRoot[]>('cloud.providers.detect');
      setCloudRoots(roots);
      const discovered = await Promise.all(roots.map(async (root) => {
        try {
          const libraries = await invoke<Omit<CloudLibraryChoice, 'root'>[]>('cloud.libraries.discover', { root_path: root.path });
          return libraries.map((library) => ({ ...library, root }));
        } catch {
          return [];
        }
      }));
      setCloudLibraries(discovered.flat());
    } catch {
      setCloudRoots([]);
      setCloudLibraries([]);
    }
    await refreshCloudStatus();
  }, [refreshCloudStatus]);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      refresh(),
      refreshCloud(),
      refreshLibraryStatistics(),
    ]).then(([nextConfig]) => {
      if (cancelled) return;
      if (nextConfig) {
        const pinned = nextConfig.pinnedLibraries ?? [];
        const history = nextConfig.libraryHistory ?? [];
        const paths = [...pinned, ...history.filter((path) => !pinned.includes(path))];
        const current = nextConfig.currentPath;
        setSelectedPath(current && paths.includes(current) ? current : paths[0] ?? null);
      }
      setInitialLoadComplete(true);
    });
    return () => { cancelled = true; };
  }, [refresh, refreshCloud, refreshLibraryStatistics]);

  useEffect(() => {
    if (!initialLoadComplete || didRequestWindowShow.current) return;
    didRequestWindowShow.current = true;
    let secondFrame = 0;
    const firstFrame = window.requestAnimationFrame(() => {
      secondFrame = window.requestAnimationFrame(() => {
        void (window as any).picto.api.window.call('show');
      });
    });
    return () => {
      window.cancelAnimationFrame(firstFrame);
      if (secondFrame) window.cancelAnimationFrame(secondFrame);
    };
  }, [initialLoadComplete]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen('library-meta-changed', refresh).then((value) => { unlisten = value; });
    return () => unlisten?.();
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const updateCloud = () => { if (!disposed) void refreshCloudStatus(); };
    const updateStatistics = () => { if (!disposed) void refreshLibraryStatistics(); };
    const interval = window.setInterval(updateCloud, 1_500);
    void listen<LibraryChanged>('library/changed', ({ payload }) => {
      if (payload.resources.includes('cloud') || payload.resources.includes('tasks')) updateCloud();
      if (payload.resources.some((resource) => ['library', 'sidebar', 'tags', 'folders', 'smart_folders', 'subscriptions'].includes(resource))) updateStatistics();
    }).then((value) => {
      if (disposed) value();
      else unlisten = value;
    });
    return () => {
      disposed = true;
      window.clearInterval(interval);
      unlisten?.();
    };
  }, [refreshCloudStatus, refreshLibraryStatistics]);

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
      imageHash: config.libraryMeta?.[path]?.imageHash ?? null,
      imageFocusX: config.libraryMeta?.[path]?.imageFocusX ?? null,
      imageFocusY: config.libraryMeta?.[path]?.imageFocusY ?? null,
      imageZoomPercent: config.libraryMeta?.[path]?.imageZoomPercent ?? null,
      libraryPath: path,
      cloudLibraryId: config.libraryMeta?.[path]?.cloudLibraryId ?? null,
    }));
  }, [config]);

  const addedCloudLibraries = useMemo(() => {
    const added = new Map<string, string>();
    for (const entry of localEntries) {
      if (entry.cloudLibraryId) added.set(entry.cloudLibraryId, entry.name);
    }
    const current = localEntries.find((entry) => entry.current);
    if (current && cloudConfiguration?.library_id) {
      added.set(cloudConfiguration.library_id, current.name);
    }
    return added;
  }, [cloudConfiguration?.library_id, localEntries]);

  const selectedEntry = useMemo(
    () => localEntries.find((entry) => entry.path === selectedPath) ?? null,
    [localEntries, selectedPath],
  );

  const syncPresentation = useMemo(
    () => presentCloudSync(cloudStatus, cloudConfiguration?.provider ?? null),
    [cloudConfiguration?.provider, cloudStatus],
  );

  useEffect(() => {
    if (syncPresentation.workKey === null || syncPresentation.remaining === null || syncPresentation.remaining <= 0) {
      syncRateSamples.current = [];
      syncWorkKey.current = null;
      setSyncEtaSeconds(null);
      return;
    }
    const now = Date.now();
    if (syncWorkKey.current !== syncPresentation.workKey) {
      syncWorkKey.current = syncPresentation.workKey;
      syncRateSamples.current = [{ at: now, remaining: syncPresentation.remaining }];
      setSyncEtaSeconds(null);
      return;
    }
    const previous = syncRateSamples.current[syncRateSamples.current.length - 1];
    if (!previous || syncPresentation.remaining > previous.remaining) {
      syncRateSamples.current = [{ at: now, remaining: syncPresentation.remaining }];
      setSyncEtaSeconds(null);
      return;
    }
    if (syncPresentation.remaining !== previous.remaining) {
      syncRateSamples.current = [
        ...syncRateSamples.current.filter((sample) => now - sample.at <= 30_000),
        { at: now, remaining: syncPresentation.remaining },
      ].slice(-12);
    }
    setSyncEtaSeconds(estimateRemainingSeconds(syncRateSamples.current, syncPresentation.remaining));
  }, [syncPresentation]);

  useEffect(() => {
    if (showLocalCreate || showCloudOpen || selectedPath || localEntries.length === 0) return;
    setSelectedPath(localEntries.find((entry) => entry.current)?.path ?? localEntries[0].path);
  }, [localEntries, selectedPath, showCloudOpen, showLocalCreate]);

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

  const joinCloud = () => {
    if (!selectedCloudLibrary || !cloudName.trim()) return;
    void run(`join-cloud:${selectedCloudLibrary.library_id}`, async () => {
      const result = await pictoLibrary().joinCloud({
        provider: selectedCloudLibrary.root.provider,
        accountLabel: selectedCloudLibrary.root.account_label,
        rootPath: selectedCloudLibrary.root.path,
        libraryId: selectedCloudLibrary.library_id,
        name: cloudName.trim(),
      });
      if (!result) return;
      setSelectedPath(result.path);
      setShowCloudOpen(false);
      setSelectedCloudLibrary(null);
      setCloudName('');
      setMessage(`Opened "${baseName(result.path)}" from cloud.`);
      await refreshCloud();
    });
  };

  const beginRename = (path: string, name: string) => {
    setRenamingPath(path);
    setRenameValue(name);
    setError(null);
  };

  const commitRename = (path: string) =>
    run(`rename:${path}`, async () => {
      const name = renameValue.trim();
      if (!name) return;
      const result = await pictoLibrary().rename(path, name);
      setSelectedPath(result.newPath);
      setRenamingPath(null);
      setRenameValue('');
    });

  const configureCloud = (root: DetectedCloudRoot) =>
    run(`cloud:${root.path}`, async () => {
      await invoke('cloud.configure', {
        provider: root.provider,
        account_label: root.account_label,
        root_path: root.path,
      });
      await refreshCloud();
      setMessage(`Cloud sync enabled through ${root.provider === 'google_drive' ? 'Google Drive' : 'Dropbox'}.`);
    });

  const setCloudPaused = (paused: boolean) =>
    run('cloud-pause', async () => {
      await invoke('cloud.pause', { paused });
      await refreshCloud();
    });

  const disableCloud = () => {
    if (!window.confirm('Stop syncing this library?\n\nLocal files and the existing cloud copy will not be deleted.')) return;
    void run('cloud-disable', async () => {
      await invoke('cloud.disable');
      await refreshCloud();
      setMessage('Cloud sync stopped.');
    });
  };

  const syncNow = () =>
    run('cloud-sync', async () => {
      await invoke('cloud.reconcile');
      await refreshCloudStatus();
    });

  const createCloudSnapshot = () =>
    run('cloud-snapshot', async () => {
      await invoke('cloud.snapshot.create');
      await refreshCloudStatus();
      setMessage('Recovery snapshot created.');
    });

  const removeFromList = (path: string, name: string) => {
    if (!window.confirm(`Remove "${name}" from this list?\n\nThe library and its files remain on disk.`)) return;
    setSelectedPath(null);
    void run(`remove:${path}`, () => pictoLibrary().remove(path));
  };

  const openLibraryMenu = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (!selectedEntry) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const entries: MenuEntry[] = [
      {
        label: selectedEntry.pinned ? 'Remove from Quick Access' : 'Add to Quick Access',
        icon: <IconPin size={15} />,
        action: () => { void run(`pin:${selectedEntry.path}`, () => pictoLibrary().togglePin(selectedEntry.path)); },
      },
      {
        label: 'Rename',
        icon: <IconPencil size={15} />,
        disabled: !selectedEntry.exists,
        action: () => beginRename(selectedEntry.path, selectedEntry.name),
      },
      {
        label: 'Show in File Manager',
        icon: <IconFolderOpen size={15} />,
        disabled: !selectedEntry.exists,
        action: () => { void pictoShell().showInFolder(selectedEntry.path); },
      },
    ];

    if (selectedEntry.current) {
      entries.push({ separator: true });
      if (cloudConfiguration?.root_path) {
        entries.push(
          {
            label: 'Sync Now',
            icon: <IconRefresh size={15} />,
            disabled: busy !== null || syncPresentation.active,
            action: () => { void syncNow(); },
          },
          {
            label: cloudStatus?.state === 'paused' ? 'Resume Sync' : 'Pause Sync',
            icon: cloudStatus?.state === 'paused' ? <IconPlayerPlay size={15} /> : <IconPlayerPause size={15} />,
            disabled: busy !== null,
            action: () => { void setCloudPaused(cloudStatus?.state !== 'paused'); },
          },
          {
            label: 'Create Recovery Snapshot',
            icon: <IconCloud size={15} />,
            disabled: busy !== null || syncPresentation.active,
            action: () => { void createCloudSnapshot(); },
          },
          {
            label: 'Show Cloud Folder',
            icon: <IconFolderOpen size={15} />,
            action: () => { void pictoShell().showInFolder(cloudConfiguration.root_path); },
          },
          {
            label: 'Stop Syncing',
            icon: <IconCloudOff size={15} />,
            disabled: busy !== null,
            action: disableCloud,
          },
        );
      } else if (cloudRoots.length > 0) {
        entries.push({
          submenu: true,
          label: 'Enable Cloud Sync',
          icon: <IconCloud size={15} />,
          children: cloudRoots.map((root) => ({
            label: `${root.provider === 'google_drive' ? 'Google Drive' : 'Dropbox'} · ${root.account_label}`,
            action: () => { void configureCloud(root); },
          })),
        });
      }
    }

    entries.push(
      { separator: true },
      {
        label: 'Remove from List',
        icon: <IconX size={15} />,
        danger: true,
        disabled: selectedEntry.current,
        action: () => removeFromList(selectedEntry.path, selectedEntry.name),
      },
    );
    libraryMenu.openAt({ x: rect.right, y: rect.bottom + 4 }, entries, { showSearch: false });
  };

  return (
    <div className={styles.root}>
      <section className={styles.panel}>
        <header className={styles.header} data-window-drag-region="">
          <div className={styles.title}>Library Manager</div>
          <button className={styles.closeBtn} onClick={() => window.close()} aria-label="Close">
            <IconX size={14} />
          </button>
        </header>

        <div className={styles.workspace}>
          <aside className={styles.sidebar}>
            <div className={styles.sectionTitle}>On this device</div>
            <div className={styles.list}>
              {localEntries.length === 0 ? (
                <div className={styles.empty}>No libraries yet.</div>
              ) : localEntries.map((entry) => (
                <button
                  key={entry.path}
                  type="button"
                  className={selectedPath === entry.path && !showLocalCreate ? styles.rowSelected : styles.row}
                  onClick={() => {
                    setSelectedPath(entry.path);
                    setShowLocalCreate(false);
                    setShowCloudOpen(false);
                    setShowIconEditor(false);
                  }}
                  onDoubleClick={() => entry.exists && !entry.current && switchTo(entry.path)}
                >
                  <LibraryAvatar
                    appearance={entry}
                    size={42}
                    className={styles.rowIcon}
                    highlighted={selectedPath === entry.path && !showLocalCreate}
                  />
                  <span className={styles.rowMain}>
                    <span className={styles.rowName}>{entry.name}</span>
                    <span className={styles.rowPath}>{entry.dir}</span>
                  </span>
                  <span className={styles.rowStatus}>
                    {entry.current && cloudConfiguration?.root_path ? (
                      <span
                        className={`${styles.rowSyncStatus} ${styles[`syncState_${syncPresentation.tone}`]}`}
                        title={`Cloud sync: ${syncPresentation.label}`}
                        aria-label={`Cloud sync: ${syncPresentation.label}`}
                      >
                        <span className={styles.syncDot} />
                      </span>
                    ) : null}
                    {entry.current ? <IconCheck size={13} /> : entry.pinned ? <IconPin size={12} /> : !entry.exists ? <IconAlertTriangle size={12} /> : null}
                  </span>
                </button>
              ))}
            </div>
          </aside>

          <main className={styles.detailPane}>
            {showCloudOpen ? (
              <div className={styles.createPane}>
                <span className={styles.heroIcon}><IconCloud size={26} /></span>
                <div className={styles.heroTitle}>Open a cloud library</div>
                <p className={styles.heroDescription}>Choose a verified Picto library found in an installed desktop sync folder.</p>
                {cloudLibraries.length > 0 ? (
                  <div className={styles.cloudLibraryList}>
                    {cloudLibraries.map((library) => {
                      const addedTo = addedCloudLibraries.get(library.library_id);
                      const unavailable = Boolean(addedTo);
                      return (
                        <button
                          key={`${library.root.path}:${library.library_id}`}
                          type="button"
                          className={selectedCloudLibrary === library ? styles.cloudLibrarySelected : styles.cloudLibraryRow}
                          onClick={() => {
                            if (unavailable) return;
                            setSelectedCloudLibrary(library);
                            setCloudName(library.name);
                          }}
                          aria-disabled={unavailable}
                          title={unavailable ? `Already added · synced to ${addedTo}` : `Open ${library.name}`}
                        >
                          <span>{library.name}</span>
                          <span className={styles.rowPath}>
                            {unavailable
                              ? `Already added · synced to ${addedTo}`
                              : `${library.root.provider === 'google_drive' ? 'Google Drive' : 'Dropbox'} · ${library.root.account_label}`}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                ) : (
                  <p className={styles.cardDescription}>No Picto recovery snapshots were found in the installed cloud folders.</p>
                )}
                <label className={styles.fieldLabel} htmlFor="cloud-library-name">Local library name</label>
                <input
                  id="cloud-library-name"
                  className={styles.nameInput}
                  value={cloudName}
                  onChange={(event) => setCloudName(event.target.value)}
                  disabled={!selectedCloudLibrary}
                />
                <button className={styles.btnPrimary} onClick={joinCloud} disabled={busy !== null || !selectedCloudLibrary || !cloudName.trim()}>
                  {busy?.startsWith('join-cloud:') ? 'Opening…' : 'Choose Location…'}
                </button>
              </div>
            ) : showLocalCreate ? (
              <div className={styles.createPane}>
                <span className={styles.heroIcon}><IconPlus size={26} /></span>
                <div className={styles.heroTitle}>Create a new library</div>
                <p className={styles.heroDescription}>Choose a name now; Picto will ask where the library should be stored.</p>
                <label className={styles.fieldLabel} htmlFor="library-name">Library name</label>
                <input
                  id="library-name"
                  className={styles.nameInput}
                  placeholder="My Inspirations"
                  value={localName}
                  onChange={(event) => setLocalName(event.target.value)}
                  autoFocus
                />
                <button className={styles.btnPrimary} onClick={createLocal} disabled={busy !== null || !localName.trim()}>
                  {busy === 'create-local' ? 'Creating…' : 'Choose Location…'}
                </button>
              </div>
            ) : selectedEntry ? (
              <>
                <div className={styles.hero}>
                  <LibraryAvatar appearance={selectedEntry} size={84} className={styles.heroLibraryIcon} highlighted />
                  <span className={styles.heroInfo}>
                    {renamingPath === selectedEntry.path ? (
                      <span className={styles.renameRow}>
                        <input
                          className={styles.renameInput}
                          value={renameValue}
                          onChange={(event) => setRenameValue(event.target.value)}
                          onKeyDown={(event) => {
                            if (event.key === 'Enter') void commitRename(selectedEntry.path);
                            if (event.key === 'Escape') setRenamingPath(null);
                          }}
                          autoFocus
                        />
                        <button className={styles.btnPrimary} onClick={() => void commitRename(selectedEntry.path)} disabled={busy !== null || !renameValue.trim()}>Save</button>
                        <button className={styles.btn} onClick={() => setRenamingPath(null)} disabled={busy !== null}>Cancel</button>
                      </span>
                    ) : <span className={styles.heroTitle}>{selectedEntry.name}</span>}
                    <span className={styles.heroPath}>{selectedEntry.path}</span>
                  </span>
                  <span className={styles.heroActions}>
                    {!selectedEntry.current && selectedEntry.exists ? (
                      <button className={styles.btnPrimary} onClick={() => switchTo(selectedEntry.path)} disabled={busy !== null}>Open Library</button>
                    ) : null}
                    <button
                      type="button"
                      className={styles.iconButton}
                      aria-label="Library actions"
                      aria-haspopup="menu"
                      aria-expanded={libraryMenu.state ? true : undefined}
                      onClick={openLibraryMenu}
                      disabled={busy !== null}
                    >
                      <IconDotsVertical size={16} />
                    </button>
                  </span>
                </div>

                <div className={styles.statusLine}>
                  {selectedEntry.current ? <span><IconCheck size={13} /> Current library</span> : null}
                  {selectedEntry.pinned ? <span><IconPin size={12} /> Pinned</span> : null}
                  {!selectedEntry.exists ? <span className={styles.missing}><IconAlertTriangle size={12} /> Missing on disk</span> : null}
                </div>

                <section className={styles.appearanceCard}>
                  <div className={styles.cardTitle}>Appearance</div>
                  <div className={styles.detailGrid}>
                    <span className={styles.detailLabel}>Color</span>
                    <span className={styles.colorConstraint}>
                      <ColorPicker value={selectedEntry.color} onChange={(color) => void run(`meta:${selectedEntry.path}`, () => pictoLibrary().setMeta(selectedEntry.path, { color }))} />
                    </span>
                    <span className={styles.detailLabel}>Icon</span>
                    <button className={styles.btn} onClick={() => setShowIconEditor((value) => !value)} disabled={busy !== null}>
                      {showIconEditor ? 'Done' : 'Change…'}
                    </button>
                    <span className={styles.detailLabel}>Cover</span>
                    <KbdTooltip label={selectedEntry.current ? 'Choose a media item and crop the library cover' : 'Open this library before choosing its media'}>
                      <button
                        className={styles.btn}
                        onClick={() => setCoverPath(selectedEntry.path)}
                        disabled={busy !== null || !selectedEntry.current}
                      >
                        Choose…
                      </button>
                    </KbdTooltip>
                  </div>
                  {showIconEditor ? (
                    <div className={styles.iconEditor}>
                      <IconPicker compact defaultLabel="Use default library icon" value={selectedEntry.icon} onChange={(icon) => void run(`meta:${selectedEntry.path}`, () => pictoLibrary().setMeta(selectedEntry.path, { icon, imageHash: null }))} />
                    </div>
                  ) : null}
                </section>

                <section className={styles.statisticsCard}>
                  <div className={styles.cardTitle}>Library</div>
                  {!selectedEntry.current ? (
                    <p className={styles.cardDescription}>Open this library to view its statistics.</p>
                  ) : libraryStatistics ? (
                    <div className={styles.statisticsGrid}>
                      <span><strong>{libraryStatistics.active_items.toLocaleString()}</strong><small>All</small></span>
                      <span><strong>{libraryStatistics.inbox_items.toLocaleString()}</strong><small>Inbox</small></span>
                      <span><strong>{libraryStatistics.trash_items.toLocaleString()}</strong><small>Trash</small></span>
                      <span><strong>{libraryStatistics.media_assets.toLocaleString()}</strong><small>Media</small></span>
                      <span><strong>{libraryStatistics.image_assets.toLocaleString()}</strong><small>Images</small></span>
                      <span><strong>{libraryStatistics.tags.toLocaleString()}</strong><small>Tags</small></span>
                      <span><strong>{libraryStatistics.subscriptions.toLocaleString()}</strong><small>Subscriptions</small></span>
                      <span><strong>{formatBytes(libraryStatistics.original_bytes)}</strong><small>Size</small></span>
                    </div>
                  ) : <p className={styles.cardDescription}>Loading library statistics…</p>}
                </section>

                <section className={styles.cloudCard}>
                  <div className={styles.cardTitle}>Cloud Sync</div>
                  {!selectedEntry.current ? (
                    <p className={styles.cardDescription}>Open this library to configure its cloud location.</p>
                  ) : cloudConfiguration?.root_path ? (
                    <>
                      <div className={styles.cloudSummary}>
                        <span className={styles.detailLabel}>Provider</span>
                        <span>{cloudConfiguration.provider === 'google_drive' ? 'Google Drive' : 'Dropbox'} · {cloudConfiguration.account_label}</span>
                        <span className={styles.detailLabel}>State</span>
                        <span className={`${styles.syncState} ${styles[`syncState_${syncPresentation.tone}`]}`}>
                          <span className={styles.syncDot} />
                          {syncPresentation.label}
                        </span>
                        <span className={styles.detailLabel}>Last sync</span>
                        <span>{formatLastSync(cloudStatus?.last_sync_at ?? null)}</span>
                        <span className={styles.detailLabel}>Changes</span>
                        <span>{cloudStatus?.pending_mutations ?? 0} pending</span>
                        <span className={styles.detailLabel}>Files</span>
                        <span>{cloudStatus?.pending_blobs ?? 0} pending{cloudStatus?.missing_blobs ? ` · ${cloudStatus.missing_blobs} unavailable` : ''}</span>
                      </div>
                      {syncPresentation.active ? (
                        <div className={styles.syncProgress}>
                          <div className={styles.syncProgressText}>
                            <span>{cloudStatus?.message || 'Syncing library changes'}</span>
                            <span>{formatRemainingTime(syncEtaSeconds) ?? (syncPresentation.total !== null ? `${syncPresentation.completed} of ${syncPresentation.total}` : 'Estimating time remaining…')}</span>
                          </div>
                          <ProgressBar
                            done={syncPresentation.completed}
                            total={syncPresentation.total ?? 0}
                            indeterminate={syncPresentation.total === null}
                            height={3}
                          />
                        </div>
                      ) : cloudStatus?.message ? <p className={styles.cloudMessage}>{cloudStatus.message}</p> : null}
                    </>
                  ) : cloudRoots.length > 0 ? (
                    <div className={styles.cloudProviderActions}>
                      {cloudRoots.map((root) => {
                        const isDropbox = root.provider === 'dropbox';
                        const ProviderIcon = isDropbox ? IconBrandDropbox : IconBrandGoogleDrive;
                        const providerName = isDropbox ? 'Dropbox' : 'Google Drive';
                        return (
                          <button
                            key={`${root.provider}:${root.path}`}
                            type="button"
                            className={styles.cloudProviderButton}
                            onClick={() => void configureCloud(root)}
                            disabled={busy !== null}
                          >
                            <ProviderIcon size={18} />
                            <span>Synchronize Library with {providerName}</span>
                            <small>{root.account_label}</small>
                          </button>
                        );
                      })}
                    </div>
                  ) : (
                    <p className={styles.cardDescription}>Install and sign in to Google Drive or Dropbox on this computer, then reopen Library Manager.</p>
                  )}
                </section>
              </>
            ) : (
              <div className={styles.emptyDetail}><IconBooks size={28} /><span>Select a library</span></div>
            )}

            {message ? <div className={styles.message}>{message}</div> : null}
            {error ? <div className={styles.error}>{error}</div> : null}
          </main>
        </div>

        <footer className={styles.footer}>
          <button className={styles.btn} onClick={() => { setShowLocalCreate(true); setShowCloudOpen(false); setShowIconEditor(false); }} disabled={busy !== null}>
            <IconPlus size={14} /> New Library…
          </button>
          <button className={styles.btn} onClick={() => run('open-existing', () => pictoLibrary().open())} disabled={busy !== null}>
            <IconFolderOpen size={14} /> Open Existing…
          </button>
          <button
            className={styles.btn}
            onClick={() => {
              setShowCloudOpen(true);
              setShowLocalCreate(false);
              setSelectedPath(null);
              setShowIconEditor(false);
            }}
            disabled={busy !== null || cloudRoots.length === 0}
          >
            Open from Cloud…
          </button>
        </footer>
      </section>

      <MediaCoverDialog<string>
        target={coverPath && selectedEntry ? { id: coverPath, name: selectedEntry.name } : null}
        busy={busy !== null}
        instructions="Select a media item from this library, then adjust its position and zoom."
        emptyText="This library has no media available for a cover."
        onLoad={(path, cursor) => loadLibraryCoverCandidates(path, cursor ?? null)}
        onSave={async (path, candidate, crop) => {
          try {
            await saveLibraryCover(path, candidate, crop);
            await refresh();
            return true;
          } catch (reason) {
            setError(reason instanceof Error ? reason.message : String(reason));
            return false;
          }
        }}
        onClose={() => setCoverPath(null)}
      />
      {libraryMenu.state ? (
        <ContextMenu
          entries={libraryMenu.state.entries}
          position={libraryMenu.state.position}
          onClose={libraryMenu.close}
          showSearch={false}
          width={236}
        />
      ) : null}
    </div>
  );
}
