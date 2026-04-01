/**
 * Context menu — glass panel with search, keyboard nav, and submenu support.
 */

import { type ReactNode, useEffect, useLayoutEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { IconSearch, IconChevronRight } from '@tabler/icons-react';
import styles from './ContextMenu.module.css';

// ── Types ──

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

export interface MenuSubmenu {
  submenu: true;
  label: string;
  icon?: ReactNode;
  children: MenuEntry[];
}

export type MenuEntry = MenuItem | MenuSeparator | MenuCustom | MenuSubmenu;

function isSeparator(entry: MenuEntry): entry is MenuSeparator {
  return 'separator' in entry;
}

function isCustom(entry: MenuEntry): entry is MenuCustom {
  return 'custom' in entry;
}

function isSubmenu(entry: MenuEntry): entry is MenuSubmenu {
  return 'submenu' in entry;
}

// ── Hook ──

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

  const openAt = useCallback((position: { x: number; y: number }, entries: MenuEntry[]) => {
    setState({ entries, position });
  }, []);

  const close = useCallback(() => setState(null), []);

  return { state, open, openAt, close };
}

// ── Component ──

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
  const [origin, setOrigin] = useState('top left');
  const [search, setSearch] = useState('');
  const [focusIdx, setFocusIdx] = useState(-1);
  const [openSubmenuLabel, setOpenSubmenuLabel] = useState<string | null>(null);
  const submenuTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [closing, setClosing] = useState(false);

  const startClose = useCallback(() => {
    if (closing) return;
    setClosing(true);
    // Unmount after exit animation
    setTimeout(onClose, 80);
  }, [closing, onClose]);

  const cancelSubmenuTimer = useCallback(() => {
    if (submenuTimerRef.current) { clearTimeout(submenuTimerRef.current); submenuTimerRef.current = null; }
  }, []);

  useEffect(() => () => cancelSubmenuTimer(), [cancelSubmenuTimer]);

  const handleSubmenuIntent = useCallback((target: string | null) => {
    cancelSubmenuTimer();
    if (target !== null) {
      setOpenSubmenuLabel(target);
    } else if (openSubmenuLabel !== null) {
      submenuTimerRef.current = setTimeout(() => setOpenSubmenuLabel(null), 150);
    }
  }, [openSubmenuLabel, cancelSubmenuTimer]);

  // Filter
  const query = search.toLowerCase().trim();
  const filtered = query
    ? entries.filter((e) => {
        if (isSeparator(e)) return false;
        if (isCustom(e)) return true;
        if (isSubmenu(e)) return e.label.toLowerCase().includes(query);
        return e.label.toLowerCase().includes(query);
      })
    : entries;
  const cleaned = cleanSeparators(filtered);

  const actionableIndices = cleaned
    .map((e, i) => (!isSeparator(e) && !isCustom(e) ? i : -1))
    .filter((i) => i >= 0);

  // Viewport clamping + transform origin for scale animation
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    let { x, y } = position;
    let ox = 'left';
    let oy = 'top';
    if (x + rect.width > window.innerWidth - 8) { x = window.innerWidth - rect.width - 8; ox = 'right'; }
    if (y + rect.height > window.innerHeight - 8) { y = window.innerHeight - rect.height - 8; oy = 'bottom'; }
    if (x < 8) x = 8;
    if (y < 8) y = 8;
    setPos({ x, y });
    setOrigin(`${oy} ${ox}`);
  }, [position, cleaned.length]);

  useEffect(() => { if (searchable) searchRef.current?.focus(); }, [searchable]);

  // Keyboard
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') { startClose(); return; }
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
        if (!item || isSeparator(item) || isCustom(item)) return;
        if (isSubmenu(item)) { setOpenSubmenuLabel(openSubmenuLabel === item.label ? null : item.label); return; }
        if (!item.disabled) { item.action(); startClose(); }
      }
      if (e.key === 'ArrowRight' && focusIdx >= 0) {
        const item = cleaned[focusIdx];
        if (item && isSubmenu(item)) { e.preventDefault(); setOpenSubmenuLabel(item.label); }
      }
      if (e.key === 'ArrowLeft' && openSubmenuLabel) {
        e.preventDefault();
        setOpenSubmenuLabel(null);
      }
    }
    window.addEventListener('keydown', handleKey, true);
    return () => window.removeEventListener('keydown', handleKey, true);
  }, [startClose, focusIdx, cleaned, actionableIndices, openSubmenuLabel]);

  return createPortal(
    <div className="no-drag-region">
      <div
        className={styles.backdrop}
        onPointerDown={(e) => { e.preventDefault(); e.stopPropagation(); startClose(); }}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); startClose(); }}
      />
      <div
        ref={menuRef}
        className={styles.menu}
        style={{ left: pos.x, top: pos.y, width: width ?? undefined, transformOrigin: origin }}
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
                onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}
              />
            </div>
          </div>
        )}

        <div className={styles.items}>
          {cleaned.map((entry, i) => {
            if (isSeparator(entry)) return <div key={i} className={styles.separator} />;

            if (isCustom(entry)) {
              return (
                <div key={entry.key} className={styles.customItem}
                  onClick={(e) => e.stopPropagation()}
                  onMouseEnter={() => { setFocusIdx(-1); handleSubmenuIntent(null); }}>
                  {entry.render()}
                </div>
              );
            }

            if (isSubmenu(entry)) {
              const isOpen = openSubmenuLabel === entry.label;
              const cls = [styles.item, focusIdx === i ? styles.focused : ''].filter(Boolean).join(' ');
              return (
                <div key={entry.label} data-menu-idx={i}>
                  <div
                    className={cls}
                    onClick={() => setOpenSubmenuLabel(isOpen ? null : entry.label)}
                    onMouseEnter={() => { setFocusIdx(i); handleSubmenuIntent(entry.label); }}
                  >
                    <span className={styles.iconSlot}>{entry.icon ?? null}</span>
                    <span className={styles.label}>{entry.label}</span>
                    <IconChevronRight size={12} className={styles.chevron} />
                  </div>
                  {isOpen && (
                    <SubmenuPanel
                      items={entry.children}
                      parentRef={menuRef}
                      itemIdx={i}
                      onClose={onClose}
                      onMouseEnter={cancelSubmenuTimer}
                    />
                  )}
                </div>
              );
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
                onClick={() => { if (entry.disabled) return; entry.action(); onClose(); }}
                onMouseEnter={() => { setFocusIdx(i); handleSubmenuIntent(null); }}
                onMouseLeave={() => setFocusIdx(-1)}
              >
                <span className={styles.iconSlot}>{entry.icon ?? null}</span>
                <span className={styles.label}>{entry.label}</span>
                {entry.shortcut && <span className={styles.shortcut}>{entry.shortcut}</span>}
              </div>
            );
          })}

          {cleaned.length === 0 && query && <div className={styles.empty}>No results</div>}
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ── Submenu panel ──

function SubmenuPanel({
  items, parentRef, itemIdx, onClose, onMouseEnter,
}: {
  items: MenuEntry[];
  parentRef: React.RefObject<HTMLDivElement | null>;
  itemIdx: number;
  onClose: () => void;
  onMouseEnter: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });
  const [subOrigin, setSubOrigin] = useState('top left');

  useLayoutEffect(() => {
    const parent = parentRef.current;
    const el = ref.current;
    if (!parent || !el) return;
    const parentRect = parent.getBoundingClientRect();
    const triggerEl = parent.querySelector(`[data-menu-idx="${itemIdx}"]`);
    const itemRect = triggerEl?.getBoundingClientRect() ?? parentRect;

    let left = parentRect.right + 4;
    let top = itemRect.top - 3;
    let ox = 'left';
    const elRect = el.getBoundingClientRect();
    if (left + elRect.width > window.innerWidth - 8) { left = parentRect.left - elRect.width - 4; ox = 'right'; }
    if (top + elRect.height > window.innerHeight - 8) top = window.innerHeight - elRect.height - 8;
    if (top < 8) top = 8;
    setPos({ left, top });
    setSubOrigin(`top ${ox}`);
  }, [parentRef, itemIdx]);

  const cleaned = cleanSeparators(items);

  return createPortal(
    <div
      ref={ref}
      className={styles.menu}
      style={{ left: pos.left, top: pos.top, transformOrigin: subOrigin }}
      onPointerDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
      onMouseEnter={onMouseEnter}
    >
      <div className={styles.items}>
        {cleaned.map((entry, i) => {
          if (isSeparator(entry)) return <div key={i} className={styles.separator} />;
          if (isCustom(entry)) return <div key={entry.key} className={styles.customItem}>{entry.render()}</div>;
          if (isSubmenu(entry)) return null; // No nested submenus
          const cls = [styles.item, entry.disabled ? styles.disabled : '', entry.danger ? styles.danger : ''].filter(Boolean).join(' ');
          return (
            <div key={i} className={cls}
              onClick={() => { if (entry.disabled) return; entry.action(); onClose(); }}>
              <span className={styles.iconSlot}>{entry.icon ?? null}</span>
              <span className={styles.label}>{entry.label}</span>
              {entry.shortcut && <span className={styles.shortcut}>{entry.shortcut}</span>}
            </div>
          );
        })}
      </div>
    </div>,
    document.body,
  );
}

// ── Helpers ──

function cleanSeparators(entries: MenuEntry[]): MenuEntry[] {
  const result: MenuEntry[] = [];
  for (const entry of entries) {
    if (isSeparator(entry)) {
      if (result.length > 0 && !isSeparator(result[result.length - 1])) result.push(entry);
    } else {
      result.push(entry);
    }
  }
  if (result.length > 0 && isSeparator(result[result.length - 1])) result.pop();
  return result;
}
