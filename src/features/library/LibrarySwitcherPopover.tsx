import { useEffect, useMemo, useRef, useState } from 'react';
import { IconBooks, IconCheck, IconPlus, IconFolderOpen, IconAdjustments } from '@tabler/icons-react';
import { invoke } from '../../platform/ipc';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { rectToCSS, getViewportCSS } from '../../shared/lib/zoomCompensation';
import styles from './LibrarySwitcherPopover.module.css';

interface Entry {
  path: string;
  name: string;
  dir: string;
  current: boolean;
  icon: string | null;
  color: string | null;
}

/// reference application-style library switcher panel, anchored under the sidebar button:
/// search header, icon + name + path rows with an opened check, and a
/// function list (create / open / manager) at the bottom.
export function LibrarySwitcherPopover({
  anchor,
  onClose,
}: {
  anchor: DOMRect;
  onClose: () => void;
}) {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [query, setQuery] = useState('');
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    (window as any).picto.library
      .getConfig()
      .then((config: any) => {
        const pinned: string[] = config.pinnedLibraries ?? [];
        const history: string[] = config.libraryHistory ?? [];
        const ordered = [...pinned, ...history.filter((p) => !pinned.includes(p))];
        setEntries(
          ordered
            .filter((path) => config.existsMap?.[path] !== false)
            .map((path) => {
              const parts = path.split(/[\\/]/).filter(Boolean);
              const base = parts.pop() ?? path;
              return {
                path,
                name: base.endsWith('.library') ? base.slice(0, -'.library'.length) : base,
                dir: parts.join('/'),
                current: config.currentPath === path,
                icon: config.libraryMeta?.[path]?.icon ?? null,
                color: config.libraryMeta?.[path]?.color ?? null,
              };
            }),
        );
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((entry) => entry.name.toLowerCase().includes(q));
  }, [entries, query]);

  const rect = rectToCSS(anchor);
  const viewport = getViewportCSS();
  const top = Math.min(rect.bottom + 4, viewport.height - 430);
  const left = Math.max(8, Math.min(rect.left, viewport.width - rect.width - 8));

  return (
    <div
      ref={rootRef}
      className={styles.popover}
      style={{ top, left, width: rect.width }}
    >
      <div className={styles.header}>
        <div className={styles.headerTitle}>Libraries</div>
        <input
          className={styles.search}
          placeholder="Search libraries…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
      </div>
      <div className={styles.list}>
        {filtered.length === 0 ? (
          <div className={styles.empty}>No Result</div>
        ) : (
          filtered.map((entry) => (
            <button
              key={entry.path}
              className={styles.item}
              onClick={() => {
                if (!entry.current) void (window as any).picto.library.switch(entry.path);
                onClose();
              }}
            >
              <span
                className={styles.itemIcon}
                style={entry.color ? { color: entry.color } : undefined}
              >
                {entry.icon ? (
                  <DynamicIcon name={entry.icon} size={15} color={entry.color} />
                ) : (
                  <IconBooks size={15} />
                )}
              </span>
              <span className={styles.itemInfo}>
                <span className={styles.itemName}>{entry.name}</span>
                <span className={styles.itemPath} title={entry.dir}>{entry.dir}</span>
              </span>
              {entry.current ? (
                <span className={styles.itemCheck}><IconCheck size={14} /></span>
              ) : null}
            </button>
          ))
        )}
      </div>
      <div className={styles.functions}>
        <button
          className={styles.functionItem}
          onClick={() => {
            void invoke('open_library_manager', {});
            onClose();
          }}
        >
          <IconPlus size={14} /> Create Library…
        </button>
        <button
          className={styles.functionItem}
          onClick={() => {
            void (window as any).picto.library.open();
            onClose();
          }}
        >
          <IconFolderOpen size={14} /> Open Library…
        </button>
        <button
          className={styles.functionItem}
          onClick={() => {
            void invoke('open_library_manager', {});
            onClose();
          }}
        >
          <IconAdjustments size={14} /> Library Manager…
        </button>
      </div>
    </div>
  );
}
