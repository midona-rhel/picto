/**
 * TagSelectPanel — reusable tag selection/filter panel.
 *
 * Two modes controlled by the presence of `onExclude`:
 * - **filter mode**: 540×480, sidebar (Selected/All/Groups), header with logic tabs,
 *   footer with shortcut tips, right-click exclude. Used by FilterBar.
 * - **simple mode**: 360px, header search only, flat list, "Create new tag" hint.
 *   Used by inspector "Add Tags".
 */

import {
  useState,
  useEffect,
  useRef,
  useCallback,
  useLayoutEffect,
  useMemo,
} from 'react';
import { Modal } from '@mantine/core';
import { IconCheck, IconEqual, IconLayoutSidebar, IconLayersIntersect, IconLayersUnion, IconMinus, IconPin, IconPinFilled, IconPlus } from '@tabler/icons-react';
import { OverlayShell } from '../../../shared/components/OverlayShell';
import { useVirtualizer } from '@tanstack/react-virtual';
import { api } from '#desktop/api';
import { getNamespaceColor } from '../../../shared/lib/namespaceColors';
import { parseTagString } from '../../../shared/lib/tagParsing';
import { registerTagSelectOpenHandler } from './tagSelectService';
import type { TagFilterLogicMode, TagSelectPanelProps } from './tagSelectTypes';
import { KbdTooltip } from '../../../shared/components/KbdTooltip';
import { useGlobalKeydown } from '../../../shared/hooks/useGlobalKeydown';
import { useGlobalPointerDrag } from '../../../shared/hooks/useGlobalPointerDrag';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import st from './TagSelectPanel.module.css';

function nsColor(namespace: string): string {
  const [r, g, b] = getNamespaceColor(namespace, true);
  return `rgb(${r}, ${g}, ${b})`;
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface TagEntry {
  display: string;
  namespace: string;
  subtag: string;
  count: number;
}

type SidebarMode = 'SELECTED' | 'ALL' | 'GROUP';
type LogicMode = TagFilterLogicMode;

// ---------------------------------------------------------------------------
// Recent tags — persisted in localStorage
// ---------------------------------------------------------------------------

const RECENT_KEY = 'picto.tags.recent';
const MAX_RECENT = 30;

function pushRecentTag(display: string) {
  try {
    const stored = localStorage.getItem(RECENT_KEY);
    const recent: string[] = stored ? JSON.parse(stored) : [];
    const filtered = recent.filter((t) => t !== display);
    filtered.unshift(display);
    if (filtered.length > MAX_RECENT) filtered.length = MAX_RECENT;
    localStorage.setItem(RECENT_KEY, JSON.stringify(filtered));
  } catch { /* ignore */ }
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export function TagSelectPortal() {
  const [request, setRequest] = useState<TagSelectPanelProps | null>(null);
  const [openKey, setOpenKey] = useState(0);

  useEffect(() => {
    const unregister = registerTagSelectOpenHandler((req) => {
      setOpenKey((k) => k + 1);
      setRequest(req);
    });
    return unregister;
  }, []);

  const handleClose = useCallback(() => {
    request?.onClose();
    setRequest(null);
  }, [request]);

  if (!request) return null;

  return (
    <TagSelectPanelInner
      key={openKey}
      anchorEl={request.anchorEl ?? null}
      mode={request.mode ?? 'anchored'}
      title={request.title}
      selectedTags={request.selectedTags}
      excludedTags={request.excludedTags}
      logicMode={request.logicMode}
      onToggle={request.onToggle}
      onExclude={request.onExclude}
      onExcludedTagsChange={request.onExcludedTagsChange}
      onLogicChange={request.onLogicChange}
      onClose={handleClose}
    />
  );
}

// ---------------------------------------------------------------------------
// Panel implementation
// ---------------------------------------------------------------------------

function TagSelectPanelInner({
  anchorEl,
  mode,
  title,
  selectedTags,
  excludedTags,
  logicMode,
  onToggle,
  onExclude,
  onExcludedTagsChange,
  onLogicChange,
  onClose,
}: TagSelectPanelProps) {
  const isModalMode = mode === 'modal';
  const isFilterMode = !!onExclude;
  const panelRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const [search, setSearch] = useState('');
  const [logic, setLogic] = useState<LogicMode>(logicMode ?? 'OR');
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('ALL');
  const [selectedNamespace, setSelectedNamespace] = useState<string>('');
  const [allTags, setAllTags] = useState<TagEntry[]>([]);
  const [searchResults, setSearchResults] = useState<TagEntry[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(selectedTags));
  const [excluded, setExcluded] = useState<Set<string>>(() => new Set(excludedTags ?? []));
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Drag state
  const [dragging, setDragging] = useState(false);
  const [dragSessionActive, setDragSessionActive] = useState(false);


  // Pin state
  const [pinned, setPinned] = useState(false);

  // Sidebar toggle — filter mode never shows sidebar
  const [showSidebar, setShowSidebar] = useState(!isFilterMode);

  // Keyboard nav
  const [focusIndex, setFocusIndex] = useState(-1);

  // Initial fetch
  useEffect(() => {
    api.tags.getAll()
      .then((tuples) => {
        const entries: TagEntry[] = tuples.map(([display, namespace, count]) => {
          const parsed = parseTagString(display);
          const subtag = parsed.subtag;
          return { display, namespace: namespace || '', subtag, count };
        });
        entries.sort((a, b) => b.count - a.count);
        setAllTags(entries);
      })
      .catch((e) => console.error('Failed to fetch tags:', e));
  }, []);

  // Server-side search (debounced)
  useEffect(() => {
    if (!search.trim()) {
      setSearchResults(null);
      return;
    }
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      api.tags.search(search.trim(), 200)
        .then((results) => {
          const countMap = new Map(allTags.map((t) => [t.display, t.count]));
          setSearchResults(
            results.map((r) => ({
              display: r.display,
              namespace: r.namespace,
              subtag: r.subtag,
              count: countMap.get(r.display) ?? 0,
            })),
          );
        })
        .catch((e) => {
          console.error('Tag search failed:', e);
          const q = search.toLowerCase();
          setSearchResults(allTags.filter((t) => t.display.toLowerCase().includes(q)));
        });
    }, 150);
    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [search, allTags]);

  // Position: filter mode → below button, left-aligned; inspector mode → left of inspector panel
  useLayoutEffect(() => {
    const el = panelRef.current;
    if (!el || isModalMode || !anchorEl) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    if (isFilterMode) {
      // Filter mode: dropdown below the button, left edge aligned
      let x = anchorRect.left;
      let y = anchorRect.bottom + 4;
      if (x + elRect.width > window.innerWidth - 8) x = window.innerWidth - elRect.width - 8;
      if (x < 8) x = 8;
      if (y + elRect.height > window.innerHeight - 8) y = window.innerHeight - elRect.height - 8;
      if (y < 8) y = 8;
      setPos({ x, y });
    } else {
      // Inspector mode: panel right edge aligns with inspector left edge
      const inspectorEl = anchorEl.closest('[class*="panel"]') as HTMLElement | null;
      const inspectorLeft = inspectorEl ? inspectorEl.getBoundingClientRect().left : anchorRect.left;
      let r = window.innerWidth - inspectorLeft + 4;
      let y = anchorRect.top;
      if (r < 8) r = 8;
      if (window.innerWidth - r - elRect.width < 8) r = window.innerWidth - elRect.width - 8;
      if (y + elRect.height > window.innerHeight - 8) y = window.innerHeight - elRect.height - 8;
      if (y < 8) y = 8;
      setPos({ x: r, y });
    }
  }, [anchorEl, allTags.length, isFilterMode, isModalMode]);

  useEffect(() => { searchRef.current?.focus(); }, []);

  // --- Drag ---
  // Filter mode: left-anchored (pos.x = left). Inspector mode: right-anchored (pos.x = right distance).
  const dragStart = useRef<{ mx: number; my: number; anchor: number; y: number } | null>(null);
  const isFilterModeRef = useRef(isFilterMode);
  isFilterModeRef.current = isFilterMode;

  const onHeaderMouseDown = useCallback((e: React.MouseEvent) => {
    if (isModalMode) return;
    if ((e.target as HTMLElement).closest('input, button, [class*="logicTab"]')) return;
    const el = panelRef.current;
    const rect = el?.getBoundingClientRect();
    const anchor = isFilterModeRef.current
      ? (rect ? rect.left : pos.x)
      : (rect ? window.innerWidth - rect.right : pos.x);
    dragStart.current = { mx: e.clientX, my: e.clientY, anchor, y: pos.y };
    draggingRef.current = false;
    setDragSessionActive(true);
  }, [isModalMode, pos.x, pos.y]);

  const draggingRef = useRef(false);
  const handlePanelDragMove = useCallback((e: MouseEvent) => {
    const ds = dragStart.current;
    if (!ds) return;
    const dx = e.clientX - ds.mx;
    const dy = e.clientY - ds.my;
    if (!draggingRef.current && Math.abs(dx) + Math.abs(dy) < 5) return;
    if (!draggingRef.current) {
      draggingRef.current = true;
      setDragging(true);
    }
    e.preventDefault();
    const el = panelRef.current;
    const w = el?.offsetWidth ?? 0;
    const h = el?.offsetHeight ?? 0;
    let x: number;
    if (isFilterModeRef.current) {
      x = Math.max(8, Math.min(ds.anchor + dx, window.innerWidth - w - 8));
    } else {
      x = Math.max(8, Math.min(ds.anchor - dx, window.innerWidth - w - 8));
    }
    const y = Math.max(8, Math.min(ds.y + dy, window.innerHeight - h - 8));
    setPos({ x, y });
  }, []);
  const handlePanelDragEnd = useCallback(() => {
    dragStart.current = null;
    draggingRef.current = false;
    setDragging(false);
    setDragSessionActive(false);
  }, []);
  useGlobalPointerDrag(
    { onMove: handlePanelDragMove, onEnd: handlePanelDragEnd },
    dragSessionActive,
    { target: 'document' },
  );

  // Reset focus index when search or sidebar changes
  useEffect(() => { setFocusIndex(-1); focusIndexRef.current = -1; }, [search, sidebarMode, selectedNamespace]);

  // --- Keyboard navigation ---
  // Refs for values needed in the keyboard handler (defined later in the component)
  const namespaceGroupsRef = useRef<[string, number][]>([]);
  const displayTagsRef = useRef<TagEntry[]>([]);
  const focusIndexRef = useRef(-1);
  const handleLeftClickRef = useRef<(display: string) => void>(() => {});

  // --- Sidebar data ---
  const namespaceGroups = useMemo(() => {
    const groups = new Map<string, number>();
    for (const tag of allTags) {
      if (!tag.namespace) continue;
      groups.set(tag.namespace, (groups.get(tag.namespace) ?? 0) + 1);
    }
    return [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [allTags]);
  namespaceGroupsRef.current = namespaceGroups;

  const selectedCount = selected.size + excluded.size;

  // --- Tags to display ---
  const displayTags = useMemo(() => {
    // Search overrides everything
    if (searchResults !== null) return searchResults;

    if (sidebarMode === 'SELECTED') {
      return allTags.filter((t) => selected.has(t.display) || excluded.has(t.display));
    }
    if (sidebarMode === 'GROUP' && selectedNamespace) {
      return allTags.filter((t) => t.namespace === selectedNamespace);
    }
    // ALL
    return allTags;
  }, [searchResults, sidebarMode, selectedNamespace, allTags, selected, excluded]);
  displayTagsRef.current = displayTags;

  // Virtual list
  const virtualizer = useVirtualizer({
    count: displayTags.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 28,
    overscan: 15,
  });

  // Scroll focused item into view
  useEffect(() => {
    if (focusIndex >= 0) virtualizer.scrollToIndex(focusIndex, { align: 'auto' });
  }, [focusIndex]);

  // Left-click: toggle include
  const handleLeftClick = useCallback((display: string) => {
    if (isFilterMode) {
      setExcluded((prev) => {
        const next = new Set(prev);
        next.delete(display);
        onExcludedTagsChange?.([...next]);
        return next;
      });
    }
    const wasSelected = selected.has(display);
    const added = !wasSelected;
    setSelected((prev) => {
      const next = new Set(prev);
      if (added) next.add(display); else next.delete(display);
      return next;
    });
    onToggle(display, added);
    pushRecentTag(display);
  }, [onToggle, isFilterMode, selected, onExcludedTagsChange]);

  // Right-click: toggle exclude (filter mode only)
  const handleRightClick = useCallback((e: React.MouseEvent, display: string) => {
    e.preventDefault();
    e.stopPropagation();
    if (!onExclude) return;
    // Remove from included
    setSelected((prev) => {
      const next = new Set(prev);
      next.delete(display);
      return next;
    });
    // Toggle excluded
    const wasExcluded = excluded.has(display);
    setExcluded((prev) => {
      const next = new Set(prev);
      if (wasExcluded) next.delete(display);
      else next.add(display);
      onExcludedTagsChange?.([...next]);
      return next;
    });
    onExclude(display);
  }, [onExclude, excluded, onExcludedTagsChange]);

  // Sync ref for keyboard handler
  handleLeftClickRef.current = handleLeftClick;

  const selectFocusedOrFirst = useCallback(() => {
    const tags = displayTagsRef.current;
    const idx = focusIndexRef.current;
    if (idx >= 0 && idx < tags.length) {
      handleLeftClickRef.current(tags[idx].display);
    } else if (tags.length > 0) {
      handleLeftClickRef.current(tags[0].display);
      setFocusIndex(0);
      focusIndexRef.current = 0;
    }
  }, []);

  // Uses CAPTURE phase so it fires before Mantine useHotkeys and ImageGrid handlers,
  // preventing Escape from deselecting images instead of closing the panel.
  const handlePanelNavigationKeydown = useCallback((e: KeyboardEvent) => {
    if (e.target === searchRef.current) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        e.stopPropagation();
        setFocusIndex(0);
        focusIndexRef.current = 0;
        searchRef.current?.blur();
      } else if (e.key === 'Enter') {
        const q = searchRef.current?.value.trim() ?? '';
        const tags = displayTagsRef.current;
        const hasExactMatch = q && tags.some((t) => t.display.toLowerCase() === q.toLowerCase());
        if (!q || hasExactMatch) {
          e.preventDefault();
          e.stopPropagation();
          selectFocusedOrFirst();
        }
      }
      return;
    }

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      e.stopPropagation();
      setFocusIndex((i) => {
        const next = Math.min(i + 1, displayTagsRef.current.length - 1);
        focusIndexRef.current = next;
        return next;
      });
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      setFocusIndex((i) => {
        const next = i - 1;
        if (next < 0) { searchRef.current?.focus(); focusIndexRef.current = -1; return -1; }
        focusIndexRef.current = next;
        return next;
      });
    } else if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      selectFocusedOrFirst();
    } else if (e.key === 'Tab') {
      e.preventDefault();
      e.stopPropagation();
      const ns = namespaceGroupsRef.current;
      if (sidebarMode === 'ALL') {
        if (ns.length > 0) { setSidebarMode('GROUP'); setSelectedNamespace(ns[0][0]); }
        else { setSidebarMode('SELECTED'); setSelectedNamespace(''); }
      } else if (sidebarMode === 'GROUP') {
        const idx = ns.findIndex(([n]) => n === selectedNamespace);
        if (idx < ns.length - 1) { setSelectedNamespace(ns[idx + 1][0]); }
        else { setSidebarMode('SELECTED'); setSelectedNamespace(''); }
      } else {
        setSidebarMode('ALL'); setSelectedNamespace('');
      }
    }
  }, [selectFocusedOrFirst, sidebarMode, selectedNamespace]);
  useGlobalKeydown(handlePanelNavigationKeydown, true, { capture: true });

  // Select All (group mode)
  const handleSelectAll = useCallback(() => {
    const groupTags = allTags.filter((t) => t.namespace === selectedNamespace);
    const allSelected = groupTags.every((t) => selected.has(t.display));
    if (allSelected) {
      // Deselect all in group
      setSelected((prev) => {
        const next = new Set(prev);
        groupTags.forEach((t) => next.delete(t.display));
        return next;
      });
      groupTags.forEach((t) => onToggle(t.display, false));
    } else {
      // Select all in group
      setSelected((prev) => {
        const next = new Set(prev);
        groupTags.forEach((t) => next.add(t.display));
        return next;
      });
      groupTags.forEach((t) => {
        if (!selected.has(t.display)) onToggle(t.display, true);
      });
    }
  }, [allTags, selectedNamespace, selected, onToggle]);

  const isAllGroupSelected = useMemo(() => {
    if (sidebarMode !== 'GROUP' || !selectedNamespace) return false;
    const groupTags = allTags.filter((t) => t.namespace === selectedNamespace);
    return groupTags.length > 0 && groupTags.every((t) => selected.has(t.display));
  }, [sidebarMode, selectedNamespace, allTags, selected]);

  // Create new tag
  const handleCreateTag = useCallback(() => {
    if (!search.trim()) return;
    const tagName = search.trim();
    setSelected((prev) => {
      const next = new Set(prev);
      next.add(tagName);
      return next;
    });
    onToggle(tagName, true);
    pushRecentTag(tagName);
    setSearch('');
  }, [search, onToggle]);

  const searchExactMatch = search.trim()
    ? displayTags.some((t) => t.display.toLowerCase() === search.trim().toLowerCase())
    : true;

  const panelClass = `${st.panel}${!showSidebar ? ` ${st.panelCollapsed}` : ''}`;

  const panelBody = (
    <div
      ref={panelRef}
      className={`${panelClass}${dragging ? ` ${st.panelDragging}` : ''}${isModalMode ? ` ${st.panelModal}` : ''}`}
      style={
        isModalMode
          ? undefined
          : (isFilterMode ? { left: pos.x, top: pos.y } : { right: pos.x, top: pos.y })
      }
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* Header */}
      <div
        className={`${st.header}${dragging ? ` ${st.headerDragging}` : ''}`}
        onMouseDown={onHeaderMouseDown}
      >
          <div className={st.searchWrap}>
            <input
              ref={searchRef}
              className={st.searchInput}
              type="search"
              placeholder="Search tags..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !searchExactMatch && search.trim()) {
                  e.preventDefault();
                  handleCreateTag();
                }
                e.stopPropagation();
              }}
            />
          </div>
          {isFilterMode && (
            <>
              <span className={st.ruleLabel}>Rule</span>
              <div className={st.logicTabs}>
                {([
                  { mode: 'OR' as LogicMode, icon: <IconLayersUnion size={14} />, title: 'Any' },
                  { mode: 'AND' as LogicMode, icon: <IconLayersIntersect size={14} />, title: 'All' },
                  { mode: 'EQUAL' as LogicMode, icon: <IconEqual size={14} />, title: 'Equal' },
                ]).map(({ mode, icon, title }) => (
                  <KbdTooltip key={mode} label={title} position="top">
                    <div
                      className={`${st.logicTab}${logic === mode ? ` ${st.logicTabActive}` : ''}`}
                      onClick={() => {
                        setLogic(mode);
                        onLogicChange?.(mode);
                      }}
                    >
                      {icon}
                    </div>
                  </KbdTooltip>
                ))}
              </div>
            </>
          )}
          {!isFilterMode && !isModalMode && (
            <KbdTooltip label={showSidebar ? 'Hide sidebar' : 'Show sidebar'}>
              <button
                className={st.pinBtn}
                onClick={() => setShowSidebar((s) => !s)}
              >
                <IconLayoutSidebar size={14} />
              </button>
            </KbdTooltip>
          )}
          {!isModalMode && (
            <KbdTooltip label={pinned ? 'Cancel always on top' : 'Always on top'}>
              <button
                className={st.pinBtn}
                onClick={() => setPinned((p) => !p)}
              >
                {pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
              </button>
            </KbdTooltip>
          )}
      </div>

      {/* Body: sidebar + content */}
      <div className={st.body}>
          {/* Sidebar (filter mode only) */}
          {showSidebar && (
            <div className={st.sidebar}>
              {/* Selected */}
              <div
                className={`${st.sidebarItem}${sidebarMode === 'SELECTED' ? ` ${st.sidebarItemActive}` : ''}`}
                onClick={() => { setSidebarMode('SELECTED'); setSelectedNamespace(''); }}
              >
                <span className={st.sidebarName}>Selected</span>
                {selectedCount > 0 && (
                  <span className={st.sidebarBadge}>{selectedCount}</span>
                )}
              </div>
              {/* All */}
              <div
                className={`${st.sidebarItem}${sidebarMode === 'ALL' ? ` ${st.sidebarItemActive}` : ''}`}
                onClick={() => { setSidebarMode('ALL'); setSelectedNamespace(''); }}
              >
                <span className={st.sidebarName}>All</span>
                {allTags.length > 0 && (
                  <span className={st.sidebarBadge}>{allTags.length}</span>
                )}
              </div>

              {/* Separator */}
              {namespaceGroups.length > 0 && <div className={st.sidebarSeparator} />}

              {/* Namespace groups */}
              {namespaceGroups.map(([ns, count]) => (
                <div
                  key={ns}
                  className={`${st.sidebarItem}${sidebarMode === 'GROUP' && selectedNamespace === ns ? ` ${st.sidebarItemActive}` : ''}`}
                  onClick={() => { setSidebarMode('GROUP'); setSelectedNamespace(ns); }}
                >
                  <span className={st.sidebarDot} style={{ backgroundColor: nsColor(ns) }} />
                  <span className={st.sidebarName}>{ns}</span>
                  <span className={st.sidebarBadge}>{count}</span>
                </div>
              ))}
            </div>
          )}

          {/* Content area */}
          <div className={st.contentArea}>
            <div ref={listRef} className={st.tagsContainer}>
              {displayTags.length === 0 ? (
                <div className={st.emptyState}>
                  {sidebarMode === 'SELECTED' ? 'No selected tags' : 'No tags found'}
                </div>
              ) : (
                <div
                  style={{
                    height: virtualizer.getTotalSize(),
                    width: '100%',
                    position: 'relative',
                  }}
                >
                  {virtualizer.getVirtualItems().map((virtualItem) => {
                    const tag = displayTags[virtualItem.index];
                    const isChecked = selected.has(tag.display);
                    const isExcludedItem = excluded.has(tag.display);
                    const isFocused = virtualItem.index === focusIndex;

                    const itemClass = [
                      st.checkItem,
                      isChecked && st.checkItemChecked,
                      isExcludedItem && st.checkItemExcluded,
                      isFocused && st.checkItemFocused,
                    ].filter(Boolean).join(' ');

                    const checkClass = [
                      st.checkIcon,
                      isChecked && st.checkIconChecked,
                      isExcludedItem && st.checkIconExcluded,
                    ].filter(Boolean).join(' ');

                    return (
                      <div
                        key={tag.display}
                        className={itemClass}
                        style={{
                          position: 'absolute',
                          top: 0,
                          left: 0,
                          width: '100%',
                          height: virtualItem.size,
                          transform: `translateY(${virtualItem.start}px)`,
                        }}
                        onClick={() => handleLeftClick(tag.display)}
                        onContextMenu={(e) => handleRightClick(e, tag.display)}
                      >
                        <span className={checkClass}>
                          {isChecked && (
                            <span className={st.checkMark}>
                              <IconCheck size={10} strokeWidth={3} />
                            </span>
                          )}
                          {isExcludedItem && (
                            <span className={st.checkMark}>
                              <IconMinus size={8} strokeWidth={3} />
                            </span>
                          )}
                        </span>
                        <span
                          className={st.tagIcon}
                          style={{ backgroundColor: nsColor(tag.namespace) }}
                        />
                        <span className={st.itemName}>
                          {search ? highlightMatch(tag.namespace ? tag.subtag : tag.display, search) : (tag.namespace ? tag.subtag : tag.display)}
                        </span>
                        {tag.count > 0 && (
                          <span className={st.itemBadge}>{tag.count.toLocaleString()}</span>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Select All footer (group mode) */}
            {isFilterMode && sidebarMode === 'GROUP' && selectedNamespace && (
              <div className={st.selectAllFooter}>
                <div className={st.checkItem} onClick={handleSelectAll}>
                  <span className={`${st.checkIcon}${isAllGroupSelected ? ` ${st.checkIconChecked}` : ''}`}>
                    {isAllGroupSelected && (
                      <span className={st.checkMark}>
                        <IconCheck size={10} strokeWidth={3} />
                      </span>
                    )}
                  </span>
                  <span className={st.itemName}>Select All</span>
                </div>
              </div>
            )}

            {/* Create new tag (simple mode) */}
            {!isFilterMode && search.trim() && !searchExactMatch && (
              <div className={st.createHint} onClick={handleCreateTag}>
                <IconPlus size={14} />
                Create &ldquo;{search.trim()}&rdquo;
              </div>
            )}
          </div>
      </div>
      {/* Footer */}
      <div className={st.footer}>
        <div className={st.footerLeft}>
          {isFilterMode ? (
            <>
              <span className={st.shortcutTip}>
                Select <span className={st.kbd}>L-click</span>
              </span>
              <span className={st.shortcutTip}>
                Exclude <span className={st.kbd}>R-click</span>
              </span>
            </>
          ) : (
            <>
              <span className={st.shortcutTip}><span className={st.kbd}>&uarr;&darr;</span></span>
              <span className={st.shortcutTip}><span className={st.kbd}>&crarr;</span> Select</span>
              <span className={st.shortcutTip}><span className={st.kbd}>Tab</span></span>
            </>
          )}
        </div>
        <div className={st.footerRight}>
          <span className={st.shortcutTip}>
            <span className={st.kbd}>ESC</span>
          </span>
        </div>
      </div>
    </div>
  );

  if (isModalMode) {
    return (
      <Modal
        opened
        onClose={onClose}
        title={title ?? 'Select Tags'}
        centered
        size="lg"
        styles={glassModalStyles}
      >
        {panelBody}
      </Modal>
    );
  }

  return (
    <OverlayShell open onClose={onClose} pinned={pinned}>
      {panelBody}
    </OverlayShell>
  );
}

// ---------------------------------------------------------------------------
// Highlight search match
// ---------------------------------------------------------------------------

function highlightMatch(text: string, query: string): React.ReactNode {
  if (!query) return text;
  const idx = text.toLowerCase().indexOf(query.toLowerCase());
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <b className={st.matchHighlight}>{text.slice(idx, idx + query.length)}</b>
      {text.slice(idx + query.length)}
    </>
  );
}
