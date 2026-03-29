/**
 * Context menu primitive — glass panel with search and keyboard nav.
 * Ported from legacy ContextMenu.tsx structure and behavior.
 */

import { type ReactNode, useEffect, useLayoutEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { IconSearch } from '@tabler/icons-react';
import styles from './ContextMenu.module.css';

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  action: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export interface MenuSeparator {
  separator: true;
}

export interface MenuCustom {
  custom: true;
  key: string;
  render: () => ReactNode;
}

export type MenuEntry = MenuItem | MenuSeparator | MenuCustom;

function isSeparator(entry: MenuEntry): entry is MenuSeparator {
  return 'separator' in entry;
}

function isCustom(entry: MenuEntry): entry is MenuCustom {
  return 'custom' in entry;
}

interface ContextMenuProps {
  entries: MenuEntry[];
  position: { x: number; y: number };
  onClose: () => void;
  searchable?: boolean;
  width?: number;
}

export function ContextMenu({ entries, position, onClose, searchable = true, width }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [pos, setPos] = useState(position);
  const [search, setSearch] = useState('');
  const [focusIdx, setFocusIdx] = useState(-1);

  // Filter — custom entries always shown, separators hidden during search
  const query = search.toLowerCase().trim();
  const filtered = query
    ? entries.filter((e) => {
        if (isSeparator(e)) return false;
        if (isCustom(e)) return true;
        return e.label.toLowerCase().includes(query);
      })
    : entries;
  const cleaned = cleanSeparators(filtered);

  // Actionable indices for keyboard nav (only MenuItem, not separators or custom)
  const actionableIndices = cleaned
    .map((e, i) => (!isSeparator(e) && !isCustom(e) ? i : -1))
    .filter((i) => i >= 0);

  // Viewport clamping
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    let { x, y } = position;
    if (x + rect.width > window.innerWidth - 8) x = window.innerWidth - rect.width - 8;
    if (y + rect.height > window.innerHeight - 8) y = window.innerHeight - rect.height - 8;
    if (x < 8) x = 8;
    if (y < 8) y = 8;
    setPos({ x, y });
  }, [position, cleaned.length]);

  // Focus search on open
  useEffect(() => {
    if (searchable) searchRef.current?.focus();
  }, [searchable]);

  // Keyboard navigation
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') { onClose(); return; }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setFocusIdx((prev) => {
          const cur = actionableIndices.indexOf(prev);
          return actionableIndices[cur < actionableIndices.length - 1 ? cur + 1 : 0] ?? -1;
        });
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setFocusIdx((prev) => {
          const cur = actionableIndices.indexOf(prev);
          return actionableIndices[cur > 0 ? cur - 1 : actionableIndices.length - 1] ?? -1;
        });
      }
      if (e.key === 'Enter' && focusIdx >= 0) {
        e.preventDefault();
        const item = cleaned[focusIdx];
        if (item && !isSeparator(item) && !isCustom(item) && !item.disabled) {
          item.action();
          onClose();
        }
      }
    }
    window.addEventListener('keydown', handleKey, true);
    return () => window.removeEventListener('keydown', handleKey, true);
  }, [onClose, focusIdx, cleaned, actionableIndices]);

  return createPortal(
    <div className="no-drag-region">
      <div
        className={styles.backdrop}
        onPointerDown={(e) => { e.preventDefault(); e.stopPropagation(); onClose(); }}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onClose(); }}
      />
      <div
        ref={menuRef}
        className={styles.menu}
        style={{ left: pos.x, top: pos.y, width: width ?? undefined }}
        onPointerDown={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.preventDefault()}
      >
        {searchable && (
          <div className={styles.searchArea}>
            <div className={styles.searchRow}>
              <IconSearch size={16} stroke={1.5} className={styles.searchIcon} />
              <input
                ref={searchRef}
                className={styles.searchInput}
                value={search}
                onChange={(e) => { setSearch(e.target.value); setFocusIdx(-1); }}
                placeholder="Search..."
                onKeyDown={(e) => {
                  if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
                }}
              />
            </div>
          </div>
        )}

        <div className={styles.items}>
          {cleaned.map((entry, i) => {
            if (isSeparator(entry)) {
              return <div key={i} className={styles.separator} />;
            }
            if (isCustom(entry)) {
              return <div key={entry.key} className={styles.customItem} onClick={(e) => e.stopPropagation()}>{entry.render()}</div>;
            }
            const cls = [
              styles.item,
              focusIdx === i ? styles.focused : '',
              entry.disabled ? styles.disabled : '',
              entry.danger ? styles.danger : '',
            ].filter(Boolean).join(' ');

            return (
              <div
                key={i}
                className={cls}
                onClick={() => {
                  if (entry.disabled) return;
                  entry.action();
                  onClose();
                }}
                onMouseEnter={() => setFocusIdx(i)}
                onMouseLeave={() => setFocusIdx(-1)}
              >
                <span className={styles.iconSlot}>
                  {entry.icon ?? null}
                </span>
                <span className={styles.label}>{entry.label}</span>
                {entry.shortcut && (
                  <span className={styles.shortcut}>{entry.shortcut}</span>
                )}
              </div>
            );
          })}

          {cleaned.length === 0 && query && (
            <div className={styles.empty}>No results</div>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

function cleanSeparators(entries: MenuEntry[]): MenuEntry[] {
  const result: MenuEntry[] = [];
  for (const entry of entries) {
    if (isSeparator(entry)) {
      if (result.length > 0 && !isSeparator(result[result.length - 1])) {
        result.push(entry);
      }
    } else {
      result.push(entry);
    }
  }
  if (result.length > 0 && isSeparator(result[result.length - 1])) result.pop();
  return result;
}

// ── Hook ─────────────────────────────────────────────────────────

interface ContextMenuState {
  entries: MenuEntry[];
  position: { x: number; y: number };
}

export function useContextMenu() {
  const [state, setState] = useState<ContextMenuState | null>(null);

  const open = useCallback((e: React.MouseEvent, entries: MenuEntry[]) => {
    e.preventDefault();
    e.stopPropagation();
    setState({ entries, position: { x: e.clientX, y: e.clientY } });
  }, []);

  /** Open at explicit coordinates — no event required. */
  const openAt = useCallback((position: { x: number; y: number }, entries: MenuEntry[]) => {
    setState({ entries, position });
  }, []);

  const close = useCallback(() => setState(null), []);

  return { state, open, openAt, close };
}
