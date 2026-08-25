/**
 * Context menu — glass panel with search, keyboard nav, and submenu support.
 */

import { type ReactNode, useEffect, useLayoutEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconChevronRight, IconSearch, IconX } from '@tabler/icons-react';
import { getZoomFactor, getViewportCSS } from '../../lib/zoomCompensation';
import { useShortcutScope } from '../../hooks/useShortcutScope';
import styles from './ContextMenu.module.css';

// ── Types ──

export interface MenuItem {
  label: string;
  /** Alternate terms used by reference application-style command search. */
  keywords?: string | string[];
  icon?: ReactNode;
  shortcut?: string;
  action: () => void;
  /** Alternate action invoked by right-clicking an reference application-style facet value. */
  contextAction?: () => void;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  excluded?: boolean;
  /** Toggle items stay open so several options can be changed together. */
  keepOpen?: boolean;
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
  keywords?: string | string[];
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
  showSearch: boolean;
}

interface ContextMenuOpenOptions {
  showSearch?: boolean;
}

export function useContextMenu() {
  const [state, setState] = useState<ContextMenuState | null>(null);

  const open = useCallback((e: React.MouseEvent, entries: MenuEntry[], options?: ContextMenuOpenOptions) => {
    e.preventDefault();
    e.stopPropagation();
    setState({ entries, position: { x: e.clientX, y: e.clientY }, showSearch: options?.showSearch ?? true });
  }, []);

  const openAt = useCallback((position: { x: number; y: number }, entries: MenuEntry[], options?: ContextMenuOpenOptions) => {
    setState({ entries, position, showSearch: options?.showSearch ?? true });
  }, []);

  const close = useCallback(() => setState(null), []);

  return { state, open, openAt, close };
}

// ── Component ──

interface ContextMenuProps {
  entries: MenuEntry[];
  position: { x: number; y: number };
  onClose: () => void;
  showSearch?: boolean;
  /** Content-specific width while preserving the one shared menu renderer/chrome. */
  width?: number;
}

export function ContextMenu({ entries, position, onClose, showSearch = true, width }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const [origin, setOrigin] = useState('top left');
  const [search, setSearch] = useState('');
  const [focusIdx, setFocusIdx] = useState(-1);
  const [openSubmenuLabel, setOpenSubmenuLabel] = useState<string | null>(null);
  const submenuTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [closing, setClosing] = useState(false);
  const hasSearchableEntries = showSearch
    && entries.some((entry) => !isSeparator(entry) && !isCustom(entry));

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
  const filtered = query ? searchMenuEntries(entries, query) : entries;
  const cleaned = cleanSeparators(filtered);
  const hasIcons = cleaned.some((entry) => !isSeparator(entry) && !isCustom(entry) && Boolean(entry.icon));

  const actionableIndices = cleaned
    .map((e, i) => (!isSeparator(e) && !isCustom(e) ? i : -1))
    .filter((i) => i >= 0);

  // Viewport clamping
  useLayoutEffect(() => {
    const placeMenu = () => {
      const el = menuRef.current;
      if (!el) return;

      const prevMaxH = el.style.maxHeight;
      el.style.maxHeight = 'none';
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      el.style.maxHeight = prevMaxH;

      const zoom = getZoomFactor();
      const { width: adjVw, height: adjVh } = getViewportCSS(zoom);
      const margin = 12;
      let x = position.x / zoom;
      let y = position.y / zoom;
      let ox = 'left';
      let oy = 'top';

      if (x + w > adjVw - margin) { x = adjVw - w - margin; ox = 'right'; }
      if (y + h > adjVh - margin) { y = adjVh - h - margin; oy = 'bottom'; }
      if (x < margin) x = margin;
      if (y < margin) y = margin;

      setPos({ x, y });
      setOrigin(`${oy} ${ox}`);
    };

    placeMenu();
    window.addEventListener('resize', placeMenu);
    return () => window.removeEventListener('resize', placeMenu);
  }, [position, cleaned.length, width]);

  useEffect(() => { if (hasSearchableEntries) searchRef.current?.focus(); }, [hasSearchableEntries]);

  useShortcutScope((e) => {
      if (e.key === 'Escape') { startClose(); return true; }
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
        if (!item.disabled) {
          item.action();
          if (!item.keepOpen) startClose();
        }
      }
      if (e.key === 'ArrowRight' && focusIdx >= 0) {
        const item = cleaned[focusIdx];
        if (item && isSubmenu(item)) { e.preventDefault(); setOpenSubmenuLabel(item.label); }
      }
      if (e.key === 'ArrowLeft' && openSubmenuLabel) {
        e.preventDefault();
        setOpenSubmenuLabel(null);
      }
  }, { priority: 110, allowInEditable: true });

  return createPortal(
    <div className="no-drag-region">
      <div
        className={styles.backdrop}
        onPointerDown={(e) => { e.preventDefault(); e.stopPropagation(); startClose(); }}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); startClose(); }}
      />
      <div
        ref={menuRef}
        className={`${styles.menu} floatingGlassSurface ${closing ? styles.menuClosing : ''}`}
        role="menu"
        aria-label="Context menu"
        style={pos
          ? { left: pos.x, top: pos.y, transformOrigin: origin, width }
          : { left: -9999, top: -9999, visibility: 'hidden' as const, width }
        }
        onPointerDown={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.preventDefault()}
      >
        {hasSearchableEntries && (
          <div className={styles.searchArea}>
            <div className={styles.searchRow}>
              <IconSearch size={16} stroke={1.5} className={styles.searchIcon} />
              <input
                ref={searchRef}
                className={styles.searchInput}
                value={search}
                onChange={(e) => { setSearch(e.target.value); setFocusIdx(-1); }}
                placeholder="Search..."
                onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); startClose(); } }}
              />
            </div>
          </div>
        )}

        <div className={styles.items}>
          {cleaned.map((entry, i) => {
            if (isSeparator(entry)) return <div key={i} className={styles.separator} role="separator" />;

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
                <div key={entry.label} data-menu-idx={i} className={styles.submenuItem}>
                  <div
                    className={cls}
                    role="menuitem"
                    aria-haspopup="menu"
                    aria-expanded={isOpen}
                    onClick={() => setOpenSubmenuLabel(isOpen ? null : entry.label)}
                    onMouseEnter={() => { setFocusIdx(i); handleSubmenuIntent(entry.label); }}
                  >
                    {hasIcons && <span className={styles.iconSlot} data-menu-icon-slot="">{entry.icon ?? null}</span>}
                    <span className={styles.label}>{entry.label}</span>
                    <IconChevronRight size={12} className={styles.chevron} />
                  </div>
                  {isOpen && (
                    <SubmenuPanel
                      items={entry.children}
                      parentRef={menuRef}
                      itemIdx={i}
                      onClose={startClose}
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
              entry.excluded ? styles.excluded : '',
            ].filter(Boolean).join(' ');

            return (
              <div
                key={i}
                className={cls}
                role="menuitem"
                aria-disabled={entry.disabled || undefined}
                onClick={() => {
                  if (entry.disabled) return;
                  entry.action();
                  if (!entry.keepOpen) startClose();
                }}
                onContextMenu={(event) => {
                  if (!entry.contextAction || entry.disabled) return;
                  event.preventDefault();
                  event.stopPropagation();
                  entry.contextAction();
                }}
                onMouseEnter={() => { setFocusIdx(i); handleSubmenuIntent(null); }}
                onMouseLeave={() => setFocusIdx(-1)}
              >
                {hasIcons && <span className={styles.iconSlot} data-menu-icon-slot="">{entry.icon ?? null}</span>}
                <span className={styles.label}>{entry.label}</span>
                {(entry.checked !== undefined || entry.excluded) && (
                  <span className={styles.checkSlot}>{entry.excluded ? <IconX size={13} /> : entry.checked ? <IconCheck size={13} /> : null}</span>
                )}
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

    const reposition = () => {
      const zoom = getZoomFactor();
      const { width: adjVw, height: adjVh } = getViewportCSS(zoom);
      const parentRect = parent.getBoundingClientRect();
      const triggerEl = parent.querySelector(`[data-menu-idx="${itemIdx}"]`);
      const itemRect = triggerEl?.getBoundingClientRect() ?? parentRect;
      const margin = 12;

      let left = parentRect.right / zoom + 4;
      let top = itemRect.top / zoom - 3;
      let ox = 'left';
      if (left + el.offsetWidth > adjVw - margin) { left = parentRect.left / zoom - el.offsetWidth - 4; ox = 'right'; }
      if (top + el.offsetHeight > adjVh - margin) top = adjVh - el.offsetHeight - margin;
      if (top < margin) top = margin;
      setPos({ left, top });
      setSubOrigin(`top ${ox}`);
    };

    reposition();
    window.addEventListener('resize', reposition);
    return () => window.removeEventListener('resize', reposition);
  }, [parentRef, itemIdx]);

  const cleaned = cleanSeparators(items);
  const hasIcons = cleaned.some((entry) => !isSeparator(entry) && !isCustom(entry) && !isSubmenu(entry) && Boolean(entry.icon));

  return createPortal(
    <div
      ref={ref}
      className={`${styles.menu} floatingGlassSurface`}
      role="menu"
      aria-label="Context submenu"
      style={{ left: pos.left, top: pos.top, width: 'auto', transformOrigin: subOrigin }}
      onPointerDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
      onMouseEnter={onMouseEnter}
    >
      <div className={styles.items}>
        {cleaned.map((entry, i) => {
          if (isSeparator(entry)) return <div key={i} className={styles.separator} role="separator" />;
          if (isCustom(entry)) return <div key={entry.key} className={styles.customItem}>{entry.render()}</div>;
          if (isSubmenu(entry)) return null; // No nested submenus
          const cls = [styles.item, entry.disabled ? styles.disabled : '', entry.danger ? styles.danger : '', entry.excluded ? styles.excluded : ''].filter(Boolean).join(' ');
          return (
            <div key={i} className={cls}
              role="menuitem"
              aria-disabled={entry.disabled || undefined}
              onClick={() => {
                if (entry.disabled) return;
                entry.action();
                if (!entry.keepOpen) onClose();
              }}
              onContextMenu={(event) => {
                if (!entry.contextAction || entry.disabled) return;
                event.preventDefault();
                event.stopPropagation();
                entry.contextAction();
              }}>
              {hasIcons && <span className={styles.iconSlot} data-menu-icon-slot="">{entry.icon ?? null}</span>}
              <span className={styles.label}>{entry.label}</span>
              {(entry.checked !== undefined || entry.excluded) && (
                <span className={styles.checkSlot}>{entry.excluded ? <IconX size={13} /> : entry.checked ? <IconCheck size={13} /> : null}</span>
              )}
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

function keywordText(entry: MenuItem | MenuSubmenu): string {
  const keywords = Array.isArray(entry.keywords) ? entry.keywords.join(' ') : entry.keywords ?? '';
  return `${entry.label} ${keywords}`.toLocaleLowerCase();
}

/**
 * reference application searches command keywords and submenu actions, not only visible
 * top-level labels. Matching submenu actions are flattened into executable
 * results so search never leads to a dead parent row.
 */
export function searchMenuEntries(entries: MenuEntry[], rawQuery: string): MenuEntry[] {
  const query = rawQuery.trim().toLocaleLowerCase();
  if (!query) return entries;

  const matches: Array<{ entry: MenuItem | MenuSubmenu; score: number; order: number }> = [];
  let order = 0;
  const visit = (items: MenuEntry[]) => {
    for (const entry of items) {
      if (isSeparator(entry) || isCustom(entry)) continue;
      const currentOrder = order++;
      if (!('disabled' in entry) || !entry.disabled) {
        const score = menuSearchScore(keywordText(entry), entry.label.toLocaleLowerCase(), query);
        if (score != null) matches.push({ entry, score, order: currentOrder });
      }
      if (isSubmenu(entry)) visit(entry.children);
    }
  };
  visit(entries);

  return matches
    .sort((left, right) => left.score - right.score || left.order - right.order)
    .map(({ entry }) => entry);
}

function menuSearchScore(text: string, label: string, query: string): number | null {
  if (label.startsWith(query)) return 0;
  const labelIndex = label.indexOf(query);
  if (labelIndex >= 0) return 10 + labelIndex;
  const keywordIndex = text.indexOf(query);
  if (keywordIndex >= 0) return 100 + keywordIndex;

  let queryIndex = 0;
  let gap = 0;
  for (let index = 0; index < text.length && queryIndex < query.length; index += 1) {
    if (text[index] === query[queryIndex]) queryIndex += 1;
    else if (queryIndex > 0) gap += 1;
  }
  return queryIndex === query.length ? 200 + gap : null;
}
