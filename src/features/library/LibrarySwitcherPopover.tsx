import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconPlus, IconFolderOpen, IconAdjustments, IconSearch, IconX } from '@tabler/icons-react';
import { invoke } from '../../platform/ipc';
import { LibraryAvatar } from './LibraryAvatar';
import { rectToCSS, getViewportCSS } from '../../shared/lib/zoomCompensation';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import styles from './LibrarySwitcherPopover.module.css';

interface Entry {
  path: string;
  name: string;
  dir: string;
  current: boolean;
  icon: string | null;
  color: string | null;
  imageHash: string | null;
  imageFocusX: number | null;
  imageFocusY: number | null;
  imageZoomPercent: number | null;
}

/// Library switcher panel anchored under the sidebar button:
/// search header, icon + name + path rows with an opened check, and a
/// function list (create / open / manager) at the bottom.
export function LibrarySwitcherPopover({
  anchor,
  trigger,
  onClose,
}: {
  anchor: DOMRect;
  trigger: HTMLElement | null;
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
                imageHash: config.libraryMeta?.[path]?.imageHash ?? null,
                imageFocusX: config.libraryMeta?.[path]?.imageFocusX ?? null,
                imageFocusY: config.libraryMeta?.[path]?.imageFocusY ?? null,
                imageZoomPercent: config.libraryMeta?.[path]?.imageZoomPercent ?? null,
                libraryPath: path,
              };
            }),
        );
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target) || trigger?.contains(target)) return;
      onClose();
    };
    window.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('mousedown', onDown);
    };
  }, [onClose, trigger]);

  useShortcutScope((event) => {
    if (event.key !== 'Escape') return;
    onClose();
    return true;
  }, { priority: 100, allowInEditable: true });

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((entry) => entry.name.toLowerCase().includes(q));
  }, [entries, query]);

  const rect = rectToCSS(anchor);
  const viewport = getViewportCSS();
  const panelWidth = Math.min(300, viewport.width - 16);
  const top = Math.max(8, Math.min(rect.bottom + 8, viewport.height - 430));
  const left = Math.max(8, Math.min(rect.left, viewport.width - panelWidth - 8));

  return createPortal(
    <div
      ref={rootRef}
      className={`${styles.popover} floatingGlassSurface no-drag-region`}
      style={{ top, left, width: panelWidth }}
    >
      <div className={styles.content}>
        <div className={styles.header}>
          <IconSearch className={styles.searchIcon} size={16} />
          <input
            className={styles.search}
            placeholder="Search libraries…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            autoFocus
          />
          <button className={styles.closeButton} type="button" onClick={onClose} aria-label="Close library switcher">
            <IconX size={14} />
          </button>
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
                <LibraryAvatar appearance={entry} size={36} className={styles.itemIcon} highlighted={entry.current} />
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
    </div>,
    document.body,
  );
}
