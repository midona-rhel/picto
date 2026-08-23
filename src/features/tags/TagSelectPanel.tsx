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
import { IconSearch, IconCheck, IconLayoutSidebar } from '@tabler/icons-react';
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

type SidebarMode = 'selected' | 'all' | 'namespace';
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
  const target = useAtomValue(selectionTargetAtom);
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;
  const closePortal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [query, setQuery] = useState('');
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const [checked, setChecked] = useState<Set<string>>(new Set());    // tags to ADD
  const [unchecked, setUnchecked] = useState<Set<string>>(new Set()); // tags to REMOVE
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('all');
  const [activeNamespace, setActiveNamespace] = useState<string | null>(null);
  const [focusIdx, setFocusIdx] = useState(-1);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Tags already on the selected entity (for "Selected" sidebar mode)
  const entityTagKeys = useMemo(() => {
    return commonItemTags(entityData);
  }, [entityData]);

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
      setChecked(new Set());
      setUnchecked(new Set());
      setSidebarMode('all');
      setActiveNamespace(null);
      setFocusIdx(-1);
      loadTags('');
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open, loadTags]);

  // Sidebar mode change → reload
  useEffect(() => {
    if (!open) return;
    if (sidebarMode === 'selected') return; // client-side filter, no reload needed
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    loadTags(query, ns);
  }, [sidebarMode, activeNamespace]); // eslint-disable-line react-hooks/exhaustive-deps

  // Display tags — "selected" mode shows entity's tags directly (not from paginated search)
  const displayTags = useMemo(() => {
    if (sidebarMode === 'selected') {
      return (entityData?.aggregate_tags ?? []).map(tagRecord).sort((a, b) => {
        const nsA = (a.namespace ?? '').toLowerCase();
        const nsB = (b.namespace ?? '').toLowerCase();
        if (nsA !== nsB) return nsA.localeCompare(nsB);
        return a.subtag.localeCompare(b.subtag);
      });
    }
    return tags;
  }, [tags, sidebarMode, entityData]);

  // Namespace groups for sidebar
  const nsGroups = useMemo(() => {
    return [...namespaces].sort((a, b) => tagGroupOrder(a.namespace) - tagGroupOrder(b.namespace));
  }, [namespaces]);

  // Estimated total count for the current view (for scroll height estimation)
  const estimatedTotal = useMemo(() => {
    if (sidebarMode === 'selected') return displayTags.length; // client-side, exact
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
    const isOnEntity = entityTagKeys.has(tag);
    if (isOnEntity) {
      // Tag is already on entity — toggle unchecked (pending removal)
      setUnchecked((prev) => {
        const next = new Set(prev);
        if (next.has(tag)) next.delete(tag);
        else next.add(tag);
        return next;
      });
      // Remove from checked if it was there
      setChecked((prev) => { const next = new Set(prev); next.delete(tag); return next; });
    } else {
      // Tag is not on entity — toggle checked (pending addition)
      setChecked((prev) => {
        const next = new Set(prev);
        if (next.has(tag)) next.delete(tag);
        else next.add(tag);
        return next;
      });
    }
  }, [entityTagKeys]);

  const applyTags = useCallback(() => {
    if (!target || (checked.size === 0 && unchecked.size === 0)) return;
    if (checked.size > 0) void entityMutations.addTargetTags(target, [...checked]);
    if (unchecked.size > 0) void entityMutations.removeTargetTags(target, [...unchecked]);
    if (!pinned) closePortal();
    setChecked(new Set());
    setUnchecked(new Set());
  }, [target, checked, unchecked, pinned, closePortal]);

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
            <span className={shellStyles.kbd}>↑↓</span>
            <span className={shellStyles.kbd}>↩</span> Select
            <span className={shellStyles.kbd}>Tab</span>
          </span>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            {(checked.size > 0 || unchecked.size > 0) && (
              <button
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={applyTags}
                type="button"
              >
                Apply ({checked.size + unchecked.size})
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
          {nsGroups.length > 0 && <div className={styles.sidebarSep} />}
          {nsGroups.map((ns) => (
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
        <div className={styles.content}>
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
                  const isOnEntity = entityTagKeys.has(fullTag);
                  const isPendingAdd = checked.has(fullTag);
                  const isPendingRemove = unchecked.has(fullTag);
                  // Show checked if: on entity and not pending removal, OR pending addition
                  const showChecked = (isOnEntity && !isPendingRemove) || isPendingAdd;
                  const isFocused = vItem.index === focusIdx;
                  return (
                    <div
                      key={vItem.index}
                      className={`${styles.tagRow} ${isFocused ? styles.tagRowFocused : ''}`}
                      style={{ height: vItem.size, transform: `translateY(${vItem.start}px)` }}
                      onClick={() => toggleTag(fullTag)}
                    >
                      <div className={`${shellStyles.checkBox} ${showChecked ? shellStyles.checkBoxChecked : ''}`}>
                        {showChecked && <IconCheck size={10} />}
                      </div>
                      <span className={styles.tagDot} style={{ background: tagGroupColor(tag.namespace) }} />
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
    </OverlayShell>
  );
}

function formatTag(tag: CanonicalTagRecord): string {
  // Use the backend's namespace field — don't parse colons in subtag
  return tag.namespace ? `${tag.namespace}:${tag.subtag}` : tag.subtag;
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
