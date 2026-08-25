import { useCallback, useEffect, useMemo, useState } from 'react';
import { IconX, IconBooks, IconCheck, IconPin, IconAlertTriangle, IconPlus, IconFolderOpen } from '@tabler/icons-react';
import { LibraryAvatar } from './LibraryAvatar';
import { IconPicker } from '../../shared/ui/IconPicker';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { listen } from '../../platform/ipc';
import { MediaCoverDialog } from '../subscriptions/components/SubscriptionCoverDialog';
import { loadLibraryCoverCandidates, saveLibraryCover } from './libraryAppearance';
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
  }>;
  currentPath: string | null;
  existsMap: Record<string, boolean>;
}

const pictoLibrary = () => (window as any).picto.library;

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
  const [showLocalCreate, setShowLocalCreate] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [showIconEditor, setShowIconEditor] = useState(false);
  const [localName, setLocalName] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [coverPath, setCoverPath] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setConfig(await pictoLibrary().getConfig());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen('library-meta-changed', refresh).then((value) => { unlisten = value; });
    return () => unlisten?.();
  }, [refresh]);

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
    }));
  }, [config]);

  const selectedEntry = useMemo(
    () => localEntries.find((entry) => entry.path === selectedPath) ?? null,
    [localEntries, selectedPath],
  );

  useEffect(() => {
    if (showLocalCreate || selectedPath || localEntries.length === 0) return;
    setSelectedPath(localEntries.find((entry) => entry.current)?.path ?? localEntries[0].path);
  }, [localEntries, selectedPath, showLocalCreate]);

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

  return (
    <div className={styles.root}>
      <section className={styles.panel}>
        <header className={styles.header}>
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
                    setShowIconEditor(false);
                  }}
                  onDoubleClick={() => entry.exists && !entry.current && switchTo(entry.path)}
                >
                  <LibraryAvatar appearance={entry} size={28} className={styles.rowIcon} />
                  <span className={styles.rowMain}>
                    <span className={styles.rowName}>{entry.name}</span>
                    <span className={styles.rowPath}>{entry.dir}</span>
                  </span>
                  <span className={styles.rowStatus}>
                    {entry.current ? <IconCheck size={13} /> : entry.pinned ? <IconPin size={12} /> : !entry.exists ? <IconAlertTriangle size={12} /> : null}
                  </span>
                </button>
              ))}
            </div>
          </aside>

          <main className={styles.detailPane}>
            {showLocalCreate ? (
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
                  <LibraryAvatar appearance={selectedEntry} size={56} className={styles.heroLibraryIcon} />
                  <span className={styles.heroInfo}>
                    <span className={styles.heroTitle}>{selectedEntry.name}</span>
                    <span className={styles.heroPath}>{selectedEntry.path}</span>
                  </span>
                </div>

                <div className={styles.statusLine}>
                  {selectedEntry.current ? <span><IconCheck size={13} /> Current library</span> : null}
                  {selectedEntry.pinned ? <span><IconPin size={12} /> Pinned</span> : null}
                  {!selectedEntry.exists ? <span className={styles.missing}><IconAlertTriangle size={12} /> Missing on disk</span> : null}
                </div>

                <div className={styles.detailActions}>
                  {!selectedEntry.current && selectedEntry.exists ? (
                    <button className={styles.btnPrimary} onClick={() => switchTo(selectedEntry.path)} disabled={busy !== null}>Open Library</button>
                  ) : null}
                  <button className={styles.btn} onClick={() => run(`pin:${selectedEntry.path}`, () => pictoLibrary().togglePin(selectedEntry.path))} disabled={busy !== null}>
                    {selectedEntry.pinned ? 'Unpin' : 'Pin'}
                  </button>
                  <button
                    className={styles.btnDanger}
                    disabled={busy !== null || selectedEntry.current}
                    onClick={() => {
                      if (window.confirm(`Remove "${selectedEntry.name}" from this list?\n\nThe library and its files remain on disk.`)) {
                        setSelectedPath(null);
                        void run(`remove:${selectedEntry.path}`, () => pictoLibrary().remove(selectedEntry.path));
                      }
                    }}
                  >
                    Remove from List
                  </button>
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
                    <button
                      className={styles.btn}
                      onClick={() => setCoverPath(selectedEntry.path)}
                      disabled={busy !== null || !selectedEntry.current}
                      title={selectedEntry.current ? 'Choose a media item and crop the library cover' : 'Open this library before choosing its media'}
                    >
                      Choose…
                    </button>
                  </div>
                  {showIconEditor ? (
                    <div className={styles.iconEditor}>
                      <IconPicker value={selectedEntry.icon} onChange={(icon) => void run(`meta:${selectedEntry.path}`, () => pictoLibrary().setMeta(selectedEntry.path, { icon, imageHash: null }))} />
                    </div>
                  ) : null}
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
          <button className={styles.btn} onClick={() => { setShowLocalCreate(true); setShowIconEditor(false); }} disabled={busy !== null}>
            <IconPlus size={14} /> New Library…
          </button>
          <button className={styles.btn} onClick={() => run('open-existing', () => pictoLibrary().open())} disabled={busy !== null}>
            <IconFolderOpen size={14} /> Open Existing…
          </button>
        </footer>
      </section>

      <MediaCoverDialog<number>
        target={coverPath && selectedEntry ? { id: coverPath, name: selectedEntry.name } : null}
        busy={busy !== null}
        instructions="Select a media item from this library, then adjust its position and zoom."
        emptyText="This library has no media available for a cover."
        onLoad={(path, offset) => loadLibraryCoverCandidates(path, offset ?? 0)}
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
    </div>
  );
}
