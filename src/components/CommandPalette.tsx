import { useCallback, useEffect, useRef, useState, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { IconSearch } from '@tabler/icons-react';
import { useGlobalKeydown } from '../hooks/useGlobalKeydown';
import classes from './CommandPalette.module.css';

// ── Types ──────────────────────────────────────────────────────────────────

export interface CommandAction {
  id: string;
  label: string;
  description?: string;
  group: string;
  icon?: React.ReactNode;
  shortcut?: string;
  execute: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  mode: 'all' | 'navigation';
  actions: CommandAction[];
}

// ── Helpers ────────────────────────────────────────────────────────────────

/** Group ordering (matching shortcut registry convention). */
const GROUP_ORDER = ['Navigation', 'File', 'Edit', 'View', 'Rating', 'Inbox', 'Video'];

function groupSort(a: string, b: string): number {
  const ai = GROUP_ORDER.indexOf(a);
  const bi = GROUP_ORDER.indexOf(b);
  return (ai < 0 ? 999 : ai) - (bi < 0 ? 999 : bi);
}

function matchesQuery(action: CommandAction, query: string): boolean {
  const q = query.toLowerCase();
  if (action.label.toLowerCase().includes(q)) return true;
  if (action.description?.toLowerCase().includes(q)) return true;
  if (action.group.toLowerCase().includes(q)) return true;
  return false;
}

// ── Component ──────────────────────────────────────────────────────────────

export function CommandPalette({ open, onClose, mode, actions }: CommandPaletteProps) {
  const [search, setSearch] = useState('');
  const [focusIdx, setFocusIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);

  // Reset on open
  useEffect(() => {
    if (open) {
      setSearch('');
      setFocusIdx(0);
      // defer focus to next tick so portal is mounted
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  // Filter & group
  const filtered = useMemo(() => {
    let pool = actions;
    if (mode === 'navigation') {
      pool = pool.filter(a => a.group === 'Navigation');
    }
    if (search.trim()) {
      pool = pool.filter(a => matchesQuery(a, search.trim()));
    }
    return pool;
  }, [actions, mode, search]);

  // Group results for display
  const grouped = useMemo(() => {
    const map = new Map<string, CommandAction[]>();
    for (const a of filtered) {
      let arr = map.get(a.group);
      if (!arr) { arr = []; map.set(a.group, arr); }
      arr.push(a);
    }
    const groups = [...map.keys()].sort(groupSort);
    const flat: { type: 'group'; label: string }[] | { type: 'action'; action: CommandAction; flatIdx: number }[] = [];
    let idx = 0;
    for (const g of groups) {
      (flat as any[]).push({ type: 'group', label: g });
      for (const a of map.get(g)!) {
        (flat as any[]).push({ type: 'action', action: a, flatIdx: idx });
        idx++;
      }
    }
    return { rows: flat as ({ type: 'group'; label: string } | { type: 'action'; action: CommandAction; flatIdx: number })[], actionCount: idx };
  }, [filtered]);

  // Clamp focus
  useEffect(() => {
    if (focusIdx >= grouped.actionCount) setFocusIdx(Math.max(0, grouped.actionCount - 1));
  }, [grouped.actionCount, focusIdx]);

  const executeAction = useCallback((action: CommandAction) => {
    onClose();
    // Defer execution so the palette closes first
    requestAnimationFrame(() => action.execute());
  }, [onClose]);

  // Keyboard
  const onKeyDown = useCallback((e: KeyboardEvent) => {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setFocusIdx(prev => (prev + 1) % Math.max(1, grouped.actionCount));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setFocusIdx(prev => (prev - 1 + grouped.actionCount) % Math.max(1, grouped.actionCount));
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      // Find the focused action
      let count = 0;
      for (const row of grouped.rows) {
        if (row.type === 'action') {
          if (count === focusIdx) {
            executeAction(row.action);
            return;
          }
          count++;
        }
      }
    }
  }, [open, onClose, grouped, focusIdx, executeAction]);
  useGlobalKeydown(onKeyDown, open, { capture: true });

  // Scroll focused item into view
  useEffect(() => {
    const container = resultsRef.current;
    if (!container) return;
    const el = container.querySelector(`[data-palette-idx="${focusIdx}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: 'nearest' });
  }, [focusIdx]);

  if (!open) return null;

  return createPortal(
    <div className="no-drag-region">
      <div className={classes.backdrop} onClick={onClose} />
      <div className={classes.container}>
        <div className={classes.panel} onPointerDown={(e) => e.stopPropagation()}>
          <div className={classes.searchArea}>
            <IconSearch size={16} stroke={1.5} className={classes.searchIcon} />
            <input
              ref={inputRef}
              className={classes.searchInput}
              value={search}
              onChange={(e) => { setSearch(e.target.value); setFocusIdx(0); }}
              placeholder={mode === 'navigation' ? 'Go to folder...' : 'Type a command...'}
              onKeyDown={(e) => {
                // Let the global handler deal with these
                if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === 'Escape') {
                  e.preventDefault();
                }
              }}
            />
            {mode === 'navigation' && <span className={classes.modeBadge}>Folders</span>}
          </div>

          <div ref={resultsRef} className={classes.results}>
            {grouped.rows.map((row) => {
              if (row.type === 'group') {
                return <div key={`g-${row.label}`} className={classes.groupLabel}>{row.label}</div>;
              }
              const { action, flatIdx } = row;
              return (
                <div
                  key={action.id}
                  data-palette-idx={flatIdx}
                  className={`${classes.item} ${flatIdx === focusIdx ? classes.itemFocused : ''}`}
                  onClick={() => executeAction(action)}
                  onMouseEnter={() => setFocusIdx(flatIdx)}
                >
                  <span className={classes.itemIcon}>
                    {action.icon ?? null}
                  </span>
                  <span className={classes.itemLabel}>{action.label}</span>
                  {action.shortcut && <span className={classes.itemShortcut}>{action.shortcut}</span>}
                </div>
              );
            })}

            {grouped.actionCount === 0 && (
              <div className={classes.empty}>No matching commands</div>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
