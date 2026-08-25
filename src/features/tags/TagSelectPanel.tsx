/**
 * TagSelectPanel — floating glass panel for tag selection.
 *
 * 540×480 with namespace sidebar (200px) + content area (340px).
 * Virtual scrolling for large tag sets. Cursor-based pagination on scroll.
 * Header = search input + pin button (no title bar).
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { useVirtualizer } from '@tanstack/react-virtual';
import { IconSearch, IconCheck, IconLayoutSidebar, IconX, IconBookmark } from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { tagSelectPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import * as entityMutations from '../../controllers/entityMutations';
import { tagsController } from '../../controllers/tagsController';
import type { CanonicalTagRecord, CanonicalNamespaceSummary } from '../../shared/types/canonical';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import { tagGroupColor, tagGroupOrder } from './tagGroupPresentation';
import styles from './TagSelectPanel.module.css';
import { commonItemTags } from '../../shared/lib/itemDetails';
import { FilterLogicTabs } from '../../shared/ui/FilterLogicTabs';
import type { FilterMatchMode } from '../../shared/types/generated/application/FilterMatchMode';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu/ContextMenu';
import { showTagItems } from '../../controllers/gridNavigationController';
import { showErrorNotification } from '../../shared/lib/notifications';
import {
  buildCommonTagContextEntries,
  tagName,
  tagNameInGroup,
} from './tagContextMenu';
import {
  replaceStarredTag,
  setTagStarred,
  useTagPreferences,
} from './tagPreferences';

type SidebarMode = 'selected' | 'starred' | 'all' | 'namespace';
const PAGE_SIZE = 100;


function tagRecord(name: string): CanonicalTagRecord {
  const separator = name.indexOf(':');
  return {
    tag_id: 0,
    namespace: separator < 0 ? '' : name.slice(0, separator),
    subtag: separator < 0 ? name : name.slice(separator + 1),
    file_count: 0,
  };
}

export function TagSelectPanel() {
  const portalState = useAtomValue(tagSelectPortalAtom);
  const setPortalState = useSetAtom(tagSelectPortalAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const target = portalState.target ?? selectionTarget;
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;
  const customTags = portalState.selectedTags;
  const customExcludedTags = portalState.excludedTags;
  const onApplyTags = portalState.onApplyTags;
  const onApplyTagFilter = portalState.onApplyTagFilter;
  const closePortal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [query, setQuery] = useState('');
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set());
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [matchMode, setMatchMode] = useState<FilterMatchMode>('any');
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('all');
  const [activeNamespace, setActiveNamespace] = useState<string | null>(null);
  const [focusIdx, setFocusIdx] = useState(-1);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const contextMenu = useContextMenu();
  const tagPreferences = useTagPreferences();

  // Tags already on the selected entity (for "Selected" sidebar mode)
  const entityTagKeys = useMemo(() => {
    return customTags ? new Set(customTags) : commonItemTags(entityData);
  }, [customTags, entityData]);

  // Load initial tags + namespaces
  const loadTags = useCallback((search: string, ns?: string | null) => {
    const params: Parameters<typeof tagsController.getPaginated>[0] = {
      limit: PAGE_SIZE,
      search: search.trim() || null,
      namespace: ns ?? null,
    };
    void tagsController.getPaginated(params).then((result) => {
      setTags(result.items);
      setCursor(result.next_cursor);
      setFocusIdx(-1);
    }).catch(() => {});
  }, []);

  const loadMore = useCallback(() => {
    if (!cursor || loadingMore) return;
    setLoadingMore(true);
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    void tagsController.getPaginated({
      limit: PAGE_SIZE,
      search: query.trim() || null,
      namespace: ns,
      cursor,
    }).then((result) => {
      setTags((prev) => [...prev, ...result.items]);
      setCursor(result.next_cursor);
      setLoadingMore(false);
    }).catch(() => { setLoadingMore(false); });
  }, [cursor, loadingMore, query, sidebarMode, activeNamespace]);

  // Load namespaces on open
  useEffect(() => {
    if (!open) return;
    void tagsController.getNamespaceSummary().then((ns) => setNamespaces(ns ?? [])).catch(() => {});
  }, [open]);

  // Search with debounce — reload tags
  useEffect(() => {
    if (!open) return;
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      const ns = sidebarMode === 'namespace' ? activeNamespace : null;
      loadTags(query, ns);
    }, 150);
    return () => { if (searchTimerRef.current) clearTimeout(searchTimerRef.current); };
  }, [query, open, loadTags, sidebarMode, activeNamespace]);

  // Reset on open
  useEffect(() => {
    if (open) {
      setQuery('');
      setSelectedTags(new Set(entityTagKeys));
      setExcluded(new Set(customExcludedTags ?? []));
      setMatchMode(portalState.filterMatchMode ?? 'any');
      setSidebarMode('all');
      setActiveNamespace(null);
      setFocusIdx(-1);
      loadTags('');
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open, loadTags, customExcludedTags, portalState.filterMatchMode]); // entityTagKeys is the opening snapshot

  // Sidebar mode change → reload
  useEffect(() => {
    if (!open) return;
    if (sidebarMode === 'selected' || sidebarMode === 'starred') return; // client-side views
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    loadTags(query, ns);
  }, [sidebarMode, activeNamespace]); // eslint-disable-line react-hooks/exhaustive-deps

  // Display tags — "selected" mode shows entity's tags directly (not from paginated search)
  const displayTags = useMemo(() => {
    if (sidebarMode === 'selected') {
      return [...entityTagKeys].map(tagRecord).sort((a, b) => {
        const nsA = (a.namespace ?? '').toLowerCase();
        const nsB = (b.namespace ?? '').toLowerCase();
        if (nsA !== nsB) return nsA.localeCompare(nsB);
        return a.subtag.localeCompare(b.subtag);
      });
    }
    if (sidebarMode === 'starred') {
      return tagPreferences.starredTags.map(tagRecord).sort((left, right) => (
        formatTag(left).localeCompare(formatTag(right))
      ));
    }
    return tags;
  }, [tags, sidebarMode, entityTagKeys, tagPreferences.starredTags]);

  // Namespace groups for sidebar
  const nsGroups = useMemo(() => {
    return namespaces
      .filter((group) => group.namespace !== '' && group.namespace !== 'general')
      .sort((a, b) => tagGroupOrder(a.namespace) - tagGroupOrder(b.namespace));
  }, [namespaces]);

  // Estimated total count for the current view (for scroll height estimation)
  const estimatedTotal = useMemo(() => {
    if (sidebarMode === 'selected' || sidebarMode === 'starred') return displayTags.length; // client-side, exact
    if (query.trim()) return displayTags.length; // search results, can't estimate beyond loaded
    if (sidebarMode === 'namespace' && activeNamespace != null) {
      const ns = namespaces.find((n) => n.namespace === activeNamespace);
      return ns ? ns.count : displayTags.length;
    }
    // "all" mode — total from namespace summary
    const total = namespaces.reduce((sum, n) => sum + n.count, 0);
    return total > 0 ? total : displayTags.length;
  }, [sidebarMode, activeNamespace, namespaces, displayTags.length, query]);

  // Virtual list — use estimated total so scrollbar reflects all items, not just loaded
  const virtualCount = cursor ? Math.max(displayTags.length, estimatedTotal) : displayTags.length;
  const virtualizer = useVirtualizer({
    count: virtualCount,
    getScrollElement: () => listRef.current,
    estimateSize: () => 28,
    overscan: 15,
  });

  // Load more on scroll near bottom
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const handleScroll = () => {
      const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (distFromBottom < 200 && cursor && !loadingMore) loadMore();
    };
    el.addEventListener('scroll', handleScroll);
    return () => el.removeEventListener('scroll', handleScroll);
  }, [cursor, loadingMore, loadMore]);

  // Scroll focused into view
  useEffect(() => {
    if (focusIdx >= 0) virtualizer.scrollToIndex(focusIdx, { align: 'auto' });
  }, [focusIdx, virtualizer]);

  const toggleTag = useCallback((tag: string) => {
    setExcluded((current) => {
      if (!current.has(tag)) return current;
      const next = new Set(current);
      next.delete(tag);
      return next;
    });
    const next = new Set(selectedTags);
    const removing = next.delete(tag);
    if (!removing) next.add(tag);
    setSelectedTags(next);

    if (onApplyTagFilter) return;
    if (onApplyTags) {
      onApplyTags([...next]);
    } else if (target) {
      void (removing
        ? entityMutations.removeTargetTags(target, [tag])
        : entityMutations.addTargetTags(target, [tag]));
    }
  }, [onApplyTagFilter, onApplyTags, selectedTags, target]);

  const toggleExcludedTag = useCallback((tag: string) => {
    if (!onApplyTagFilter) return;
    setSelectedTags((current) => { const next = new Set(current); next.delete(tag); return next; });
    setExcluded((current) => {
      const next = new Set(current);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
  }, [onApplyTagFilter]);

  const selectionChanged = [...new Set([...entityTagKeys, ...selectedTags])]
    .filter((tag) => entityTagKeys.has(tag) !== selectedTags.has(tag)).length;

  const applyFilter = useCallback(() => {
    const excludedChanged = onApplyTagFilter
      && [...excluded].sort().join('\n') !== [...(customExcludedTags ?? [])].sort().join('\n');
    const modeChanged = onApplyTagFilter && matchMode !== (portalState.filterMatchMode ?? 'any');
    if (!onApplyTagFilter || (!selectionChanged && !excludedChanged && !modeChanged)) return;
    onApplyTagFilter([...selectedTags], [...excluded], matchMode);
    if (!pinned) closePortal();
  }, [closePortal, customExcludedTags, excluded, matchMode, onApplyTagFilter, pinned, portalState.filterMatchMode, selectedTags, selectionChanged]);

  const excludedChanged = Boolean(onApplyTagFilter
    && [...excluded].sort().join('\n') !== [...(customExcludedTags ?? [])].sort().join('\n'));
  const modeChanged = Boolean(onApplyTagFilter && matchMode !== (portalState.filterMatchMode ?? 'any'));
  const changeCount = selectionChanged + (excludedChanged ? 1 : 0) + (modeChanged ? 1 : 0);

  // Keyboard
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setFocusIdx((i) => Math.min(i + 1, displayTags.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setFocusIdx((i) => Math.max(i - 1, -1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const idx = focusIdx >= 0 ? focusIdx : 0;
      if (displayTags[idx]) toggleTag(formatTag(displayTags[idx]));
    } else if (e.key === 'Tab') {
      e.preventDefault();
      if (sidebarMode === 'all') {
        if (nsGroups.length > 0) { setSidebarMode('namespace'); setActiveNamespace(nsGroups[0].namespace); }
        else setSidebarMode('selected');
      } else if (sidebarMode === 'namespace') {
        const idx = nsGroups.findIndex((g) => g.namespace === activeNamespace);
        if (idx < nsGroups.length - 1) setActiveNamespace(nsGroups[idx + 1].namespace);
        else setSidebarMode('selected');
      } else {
        setSidebarMode('all');
      }
    }
  }, [displayTags, focusIdx, toggleTag, sidebarMode, nsGroups, activeNamespace]);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={closePortal}
      width={showSidebar ? 540 : 340}
      pinned={pinned}
      anchorPosition={anchorPosition}
      anchorPlacement={portalState.anchorPlacement}
      onPinnedChange={setPinned}
      header={
        <>
          <div className={shellStyles.searchRow} style={{ flex: 1 }}>
            <IconSearch size={14} className={shellStyles.searchIcon} />
            <input
              ref={searchRef}
              className={shellStyles.searchInput}
              placeholder="Search tags..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
            />
          </div>
          {onApplyTagFilter ? <FilterLogicTabs value={matchMode} onChange={setMatchMode} /> : null}
          <button
            className={shellStyles.pinBtn}
            onClick={() => setShowSidebar((v) => !v)}
            type="button"
            title={showSidebar ? 'Hide sidebar' : 'Show sidebar'}
          >
            <IconLayoutSidebar size={14} />
          </button>
        </>
      }
      footer={
        <>
          <span className={shellStyles.kbdHint}>
            <span className={shellStyles.kbd}>Click</span> Select
            {onApplyTagFilter ? <><span className={shellStyles.kbd}>Right-click</span> Exclude</> : null}
          </span>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            {onApplyTagFilter && changeCount > 0 && (
              <button
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={applyFilter}
                type="button"
              >
                Apply ({changeCount})
              </button>
            )}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        {/* Sidebar — always mounted, fades with collapse */}
        <div className={`${styles.sidebar} ${!showSidebar ? styles.sidebarHidden : ''}`}>
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'selected' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('selected'); setActiveNamespace(null); }}
          >
            <span className={styles.sidebarName}>Selected</span>
            <span className={styles.sidebarBadge}>{entityTagKeys.size}</span>
          </div>
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'all' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('all'); setActiveNamespace(null); }}
          >
            <span className={styles.sidebarName}>All</span>
            <span className={styles.sidebarBadge}>
              {namespaces.reduce((sum, n) => sum + n.count, 0).toLocaleString()}
            </span>
          </div>
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'starred' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('starred'); setActiveNamespace(null); }}
          >
            <span className={styles.sidebarName}>Starred</span>
            <span className={styles.sidebarBadge}>{tagPreferences.starredTags.length}</span>
          </div>
          {tagPreferences.showTagGroups && nsGroups.length > 0 && <div className={styles.sidebarSep} />}
          {tagPreferences.showTagGroups && nsGroups.map((ns) => (
            <div
              key={ns.namespace || '__general'}
              className={`${styles.sidebarItem} ${sidebarMode === 'namespace' && activeNamespace === ns.namespace ? styles.sidebarItemActive : ''}`}
              onClick={() => { setSidebarMode('namespace'); setActiveNamespace(ns.namespace); }}
            >
              <span className={styles.sidebarDot} style={{ background: tagGroupColor(ns.namespace) }} />
              <span className={styles.sidebarName}>{ns.namespace || 'general'}</span>
              <span className={styles.sidebarBadge}>{ns.count.toLocaleString()}</span>
            </div>
          ))}
        </div>

        {/* Content */}
        <div className={`${styles.content} ${!showSidebar ? styles.contentExpanded : ''}`}>
          <div ref={listRef} className={styles.tagListScroller}>
            {displayTags.length === 0 ? (
              <div className={styles.emptyState}>
                {sidebarMode === 'selected' ? 'No tags on this entity' : 'No tags found'}
              </div>
            ) : (
              <div className={styles.tagListInner} style={{ height: virtualizer.getTotalSize() }}>
                {virtualizer.getVirtualItems().map((vItem) => {
                  const tag = displayTags[vItem.index];
                  // Trigger load-more when rendering near end of loaded data
                  if (!tag) {
                    if (cursor && !loadingMore && vItem.index >= displayTags.length) loadMore();
                    return null;
                  }
                  const fullTag = formatTag(tag);
                  const isExcluded = excluded.has(fullTag);
                  const showChecked = selectedTags.has(fullTag);
                  const isFocused = vItem.index === focusIdx;
                  return (
                    <div
                      key={vItem.index}
                      className={`${styles.tagRow} ${isFocused ? styles.tagRowFocused : ''} ${isExcluded ? styles.tagRowExcluded : ''}`}
                      style={{ height: vItem.size, transform: `translateY(${vItem.start}px)` }}
                      onClick={() => toggleTag(fullTag)}
                      onContextMenu={(event) => {
                        if (onApplyTagFilter) {
                          event.preventDefault();
                          event.stopPropagation();
                          toggleExcludedTag(fullTag);
                          return;
                        }
                        contextMenu.open(event, buildCommonTagContextEntries({
                          tag,
                          namespaces,
                          starred: tagPreferences.starredTags.includes(fullTag),
                          onFilter: (name) => { closePortal(); showTagItems(name); },
                          onStarChange: (name, starred) => { void setTagStarred(name, starred); },
                          onMoveToGroup: tag.tag_id > 0 ? (targetNamespace) => {
                            const nextName = tagNameInGroup(tag, targetNamespace);
                            void tagsController.rename(tag.tag_id, nextName).then(async () => {
                              await replaceStarredTag(tagName(tag), nextName);
                              loadTags(query, sidebarMode === 'namespace' ? activeNamespace : null);
                            }).catch((reason: unknown) => showErrorNotification({
                              title: 'Unable to move tag',
                              message: reason instanceof Error ? reason.message : String(reason),
                            }));
                          } : undefined,
                        }));
                      }}
                    >
                      <div className={`${shellStyles.checkBox} ${isExcluded ? shellStyles.checkBoxExcluded : showChecked ? (onApplyTagFilter ? shellStyles.checkBoxFilterChecked : shellStyles.checkBoxChecked) : ''}`}>
                        {isExcluded ? <IconX size={10} /> : showChecked ? <IconCheck size={10} /> : null}
                      </div>
                      <IconBookmark
                        aria-hidden="true"
                        className={styles.tagBookmark}
                        size={13}
                        stroke={1.6}
                        fill="currentColor"
                        fillOpacity={0.32}
                        style={{ color: tagGroupColor(tag.namespace) }}
                      />
                      <span className={styles.tagName}>
                        {query.trim() ? highlightMatch(tag.subtag ?? fullTag, query.trim()) : (tag.subtag || fullTag)}
                      </span>
                      <span className={styles.tagBadge}>{(tag.file_count ?? 0).toLocaleString()}</span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}
    </OverlayShell>
  );
}

function formatTag(tag: CanonicalTagRecord): string {
  return tagName(tag);
}

function highlightMatch(text: string, q: string): React.ReactNode {
  const idx = text.toLowerCase().indexOf(q.toLowerCase());
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <span className={styles.matchHighlight}>{text.slice(idx, idx + q.length)}</span>
      {text.slice(idx + q.length)}
    </>
  );
}
