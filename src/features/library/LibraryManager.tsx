import { useCallback, useEffect, useMemo, useState } from 'react';
import { IconX, IconBooks } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { IconPicker } from '../../shared/ui/IconPicker';
import { ColorPicker } from '../../shared/ui/ColorPicker';
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

        {message ? <div className={styles.message}>{message}</div> : null}
        {error ? <div className={styles.error}>{error}</div> : null}
      </div>
    </div>
  );
}
