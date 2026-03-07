import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useGlobalKeydown } from '../hooks/useGlobalKeydown';
import { createPortal } from 'react-dom';
import { IconCheck, IconChevronRight, IconSearch } from '@tabler/icons-react';
import { OverlayShell } from './OverlayShell';
import classes from './ContextMenu.module.css';

// ── Types ──────────────────────────────────────────────────────────────────

export type ContextMenuEntry =
  | { type: 'item'; label: string; icon?: React.ReactNode; shortcut?: string; onClick: () => void; disabled?: boolean; danger?: boolean }
  | { type: 'check'; label: string; icon?: React.ReactNode; shortcut?: string; checked: boolean; onClick: () => void; keepOpen?: boolean }
  | { type: 'separator' }
  | { type: 'submenu'; label: string; icon?: React.ReactNode; children: ContextMenuEntry[] }
  | { type: 'custom'; key: string; render: (onClose: () => void) => React.ReactNode };

export interface ContextMenuProps {
  items: ContextMenuEntry[];
  position: { x: number; y: number };
  onClose: () => void;
  searchable?: boolean;
  iconGutter?: boolean;
  panelWidth?: number;
}

// ── Hook ───────────────────────────────────────────────────────────────────

export function useContextMenu() {
  const [state, setState] = useState<{ items: ContextMenuEntry[]; position: { x: number; y: number } } | null>(null);

  const open = useCallback((e: React.MouseEvent, items: ContextMenuEntry[]) => {
    e.preventDefault();
    e.stopPropagation();
    setState({ items, position: { x: e.clientX, y: e.clientY } });
  }, []);

  const openAt = useCallback((position: { x: number; y: number }, items: ContextMenuEntry[]) => {
    setState({ items, position });
  }, []);

  const close = useCallback(() => setState(null), []);

  return { state, open, openAt, close };
}

// ── Component ──────────────────────────────────────────────────────────────

export function ContextMenu({
  items,
  position,
  onClose,
  searchable = true,
  iconGutter = true,
  panelWidth,
}: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [search, setSearch] = useState('');
  const [focusIdx, setFocusIdx] = useState(-1);
  const [adjustedPos, setAdjustedPos] = useState(position);
  const [openSubmenu, setOpenSubmenu] = useState<string | null>(null);
  const submenuCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelSubmenuClose = useCallback(() => {
    if (submenuCloseTimer.current) {
      clearTimeout(submenuCloseTimer.current);
      submenuCloseTimer.current = null;
    }
  }, []);

  // Cleanup timer on unmount
  useEffect(() => () => cancelSubmenuClose(), [cancelSubmenuClose]);

  // Wraps setOpenSubmenu with delayed close for diagonal mouse movement.
  // When moving from a submenu trigger to the submenu panel, the cursor
  // briefly passes over adjacent items. Without a delay those items would
  // immediately close the submenu via setOpenSubmenu(null).
  const handleSubmenuIntent = useCallback((target: string | null) => {
    cancelSubmenuClose();
    if (target !== null) {
      // Opening a submenu — do it immediately
      setOpenSubmenu(target);
    } else if (openSubmenu !== null) {
      // Closing — delay so diagonal movement to the panel can cancel it
      submenuCloseTimer.current = setTimeout(() => {
        setOpenSubmenu(null);
      }, 150);
    }
  }, [openSubmenu, cancelSubmenuClose]);

  const filtered = search
    ? items.filter((item) => item.type !== 'separator' && item.type !== 'custom' && 'label' in item && item.label.toLowerCase().includes(search.toLowerCase()))
    : items;

  const actionableIndices = filtered
    .map((item, i) => (item.type !== 'separator' && item.type !== 'custom' ? i : -1))
    .filter((i) => i >= 0);

  // Reposition to stay in viewport
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    let { x, y } = position;
    if (x + rect.width > window.innerWidth - 8) x = window.innerWidth - rect.width - 8;
    if (y + rect.height > window.innerHeight - 8) y = window.innerHeight - rect.height - 8;
    if (x < 8) x = 8;
    if (y < 8) y = 8;
    setAdjustedPos({ x, y });
  }, [position, filtered.length]);

  useEffect(() => {
    if (searchable) searchRef.current?.focus();
  }, [searchable]);

  // Keyboard navigation
  const onKeyDown = useCallback((e: KeyboardEvent) => {
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
      const item = filtered[focusIdx];
      if (item && item.type !== 'separator' && item.type !== 'custom') {
        if (item.type === 'submenu') {
          setOpenSubmenu(openSubmenu === item.label ? null : item.label);
        } else {
          item.onClick();
          if (item.type === 'check' && 'keepOpen' in item && item.keepOpen) { /* stay open */ }
          else onClose();
        }
      }
    }
  }, [onClose, focusIdx, filtered, actionableIndices, openSubmenu]);
  useGlobalKeydown(onKeyDown, true, { capture: true });

  return (
    <OverlayShell open onClose={onClose}>
      <div
        ref={menuRef}
        className={classes.panel}
        onPointerDown={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.preventDefault()}
        style={{ left: adjustedPos.x, top: adjustedPos.y, width: panelWidth }}
      >
        {searchable && (
          <div className={classes.searchArea}>
            <div className={classes.searchRow}>
              <IconSearch size={14} stroke={1.5} className={classes.searchIcon} />
              <input
                ref={searchRef}
                className={classes.searchInput}
                value={search}
                onChange={(e) => { setSearch(e.target.value); setFocusIdx(-1); }}
                placeholder="Search..."
              />
            </div>
          </div>
        )}

        <div className={classes.items}>
          {filtered.map((item, idx) => {
            if (item.type === 'separator') {
              return <div key={`sep-${idx}`} className={classes.separator} />;
            }

            if (item.type === 'custom') {
              return (
                <div key={item.key} className={classes.customItem} onMouseEnter={() => { setFocusIdx(-1); handleSubmenuIntent(null); }}>
                  {item.render(onClose)}
                </div>
              );
            }

            const isSubmenu = item.type === 'submenu';
            const isSubmenuOpen = isSubmenu && openSubmenu === item.label;

            return (
              <ItemRow
                key={item.label}
                item={item}
                idx={idx}
                isFocused={focusIdx === idx}
                onClose={onClose}
                onHover={() => {
                  setFocusIdx(idx);
                  handleSubmenuIntent(isSubmenu ? item.label : null);
                }}
                onClearFocus={() => setFocusIdx(-1)}
                onToggleSubmenu={isSubmenu ? () => setOpenSubmenu(isSubmenuOpen ? null : item.label) : undefined}
                submenuContent={isSubmenuOpen && isSubmenu ? (
                  <SubmenuPanel
                    items={item.children}
                    parentRef={menuRef}
                    itemIdx={idx}
                    onClose={onClose}
                    onMouseEnterSubmenu={cancelSubmenuClose}
                    iconGutter={iconGutter}
                  />
                ) : null}
                iconGutter={iconGutter}
              />
            );
          })}

          {filtered.length === 0 && (
            <div className={classes.empty}>No results</div>
          )}
        </div>
      </div>
    </OverlayShell>
  );
}

// ── Shared item row ────────────────────────────────────────────────────────

function ItemRow({
  item,
  idx,
  isFocused,
  onClose,
  onHover,
  onClearFocus,
  onToggleSubmenu,
  submenuContent,
  iconGutter,
}: {
  item: Exclude<ContextMenuEntry, { type: 'separator' } | { type: 'custom' }>;
  idx: number;
  isFocused: boolean;
  onClose: () => void;
  onHover: () => void;
  onClearFocus: () => void;
  onToggleSubmenu?: () => void;
  submenuContent?: React.ReactNode;
  iconGutter: boolean;
}) {
  const isDisabled = 'disabled' in item && item.disabled;
  const isDanger = item.type === 'item' && item.danger;
  const isCheck = item.type === 'check';
  const isSubmenu = item.type === 'submenu';

  const className = [
    classes.item,
    isFocused && classes.itemFocused,
    isDisabled && classes.itemDisabled,
    isDanger && classes.itemDanger,
  ].filter(Boolean).join(' ');

  return (
    <div data-menu-idx={idx}>
      <div
        className={className}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => {
          if (isDisabled) return;
          if (isSubmenu) { onToggleSubmenu?.(); return; }
          item.onClick();
          if (isCheck && 'keepOpen' in item && item.keepOpen) return;
          onClose();
        }}
        onMouseEnter={onHover}
        onMouseLeave={() => !isSubmenu && onClearFocus()}
      >
        {isCheck ? (
          <span className={`${classes.checkIcon} ${item.checked ? classes.checkIconChecked : ''}`}>
            {item.checked && <IconCheck size={10} strokeWidth={3} />}
          </span>
        ) : iconGutter ? (
          <span className={classes.iconSlot}>
            {'icon' in item && item.icon ? item.icon : null}
          </span>
        ) : null}

        <span className={classes.label}>{item.label}</span>

        {isSubmenu ? (
          <IconChevronRight className={classes.chevron} />
        ) : (
          'shortcut' in item && item.shortcut && (
            <span className={classes.shortcut}>{item.shortcut}</span>
          )
        )}
      </div>
      {submenuContent}
    </div>
  );
}

// ── Submenu panel ──────────────────────────────────────────────────────────

function SubmenuPanel({
  items,
  parentRef,
  itemIdx,
  onClose,
  onMouseEnterSubmenu,
  iconGutter,
}: {
  items: ContextMenuEntry[];
  parentRef: React.RefObject<HTMLDivElement | null>;
  itemIdx: number;
  onClose: () => void;
  onMouseEnterSubmenu?: () => void;
  iconGutter: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });

  useLayoutEffect(() => {
    const parent = parentRef.current;
    const el = ref.current;
    if (!parent || !el) return;

    const parentRect = parent.getBoundingClientRect();
    const triggerEl = parent.querySelector(`[data-menu-idx="${itemIdx}"]`);
    const itemRow = triggerEl?.querySelector(`.${classes.item}`) ?? triggerEl;
    const itemRect = itemRow?.getBoundingClientRect() ?? parentRect;

    let left = parentRect.right + 4;
    // Offset by submenu's top padding (3px) so the first item aligns with the trigger row
    let top = itemRect.top - 3;

    const elRect = el.getBoundingClientRect();
    if (left + elRect.width > window.innerWidth - 8) left = parentRect.left - elRect.width - 4;
    if (top + elRect.height > window.innerHeight - 8) top = window.innerHeight - elRect.height - 8;
    if (top < 8) top = 8;

    setPos({ left, top });
  }, [parentRef, itemIdx]);

  return createPortal(
    <div
      ref={ref}
      className={`${classes.submenu} no-drag-region`}
      onPointerDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
      onMouseEnter={onMouseEnterSubmenu}
      style={{ left: pos.left, top: pos.top }}
    >
      {items.map((item, idx) => {
        if (item.type === 'separator') {
          return <div key={`sep-${idx}`} className={classes.separator} />;
        }
        if (item.type === 'custom') {
          return (
            <div key={item.key} className={classes.customItem}>
              {item.render(onClose)}
            </div>
          );
        }
        return (
          <SubmenuItemRow
            key={'label' in item ? item.label : `item-${idx}`}
            item={item}
            onClose={onClose}
            iconGutter={iconGutter}
          />
        );
      })}
    </div>,
    document.body,
  );
}

function SubmenuItemRow({
  item,
  onClose,
  iconGutter,
}: {
  item: Exclude<ContextMenuEntry, { type: 'separator' } | { type: 'custom' }>;
  onClose: () => void;
  iconGutter: boolean;
}) {
  const [hovered, setHovered] = useState(false);
  const isDisabled = 'disabled' in item && item.disabled;
  const isDanger = item.type === 'item' && item.danger;
  const isCheck = item.type === 'check';

  const className = [
    classes.item,
    hovered && classes.itemFocused,
    isDisabled && classes.itemDisabled,
    isDanger && classes.itemDanger,
  ].filter(Boolean).join(' ');

  return (
    <div
      className={className}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={() => {
        if (isDisabled || item.type === 'submenu') return;
        item.onClick();
        if (isCheck && 'keepOpen' in item && item.keepOpen) return;
        onClose();
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {isCheck ? (
        <span className={`${classes.checkIcon} ${'checked' in item && item.checked ? classes.checkIconChecked : ''}`}>
          {'checked' in item && item.checked && <IconCheck size={10} strokeWidth={3} />}
        </span>
      ) : iconGutter ? (
        <span className={classes.iconSlot}>
          {'icon' in item && item.icon ? item.icon : null}
        </span>
      ) : null}
      <span className={classes.label}>{item.label}</span>
      {'shortcut' in item && item.shortcut && (
        <span className={classes.shortcut}>{item.shortcut}</span>
      )}
    </div>
  );
}
