/**
 * TagSelectModal — wide GlassModal for tag selection.
 *
 * Opened from grid context menu and keyboard shortcuts.
 * Richer than the OverlayShell portal: wider, namespace labels,
 * recent tags sidebar, selection summary in footer.
 *
 * The OverlayShell-based TagSelectPanel (Inspector) is separate.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { useVirtualizer } from '@tanstack/react-virtual';
import { IconSearch, IconCheck } from '@tabler/icons-react';
import { GlassModal } from '../../shared/ui/GlassModal';
import { tagSelectModalAtom } from '../../state/modals';
import { selectionCountAtom, selectionTargetAtom } from '../../state/selection';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import { useRecentItems } from '../../shared/hooks/useRecentItems';
import * as entityMutations from '../../controllers/entityMutations';
import { tagsController } from '../../controllers/tagsController';
import type { CanonicalTagRecord, CanonicalNamespaceSummary } from '../../shared/types/canonical';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import { tagGroupColor, tagGroupOrder } from '../tags/tagGroupPresentation';
import styles from './TagSelectModal.module.css';
import { tagName } from '../tags/tagContextMenu';

type SidebarMode = 'recent' | 'selected' | 'all' | 'namespace';
const PAGE_SIZE = 100;


function tagRecord(name: string): CanonicalTagRecord {
  const separator = name.indexOf(':');
  return {
    tag_id: 0,
    namespace_id: 0,
    namespace: separator < 0 ? '' : name.slice(0, separator),
    subname: separator < 0 ? name : name.slice(separator + 1),
    active_count: 0,
    assignment_count: 0,
  };
}

export function TagSelectModal() {
  const modalState = useAtomValue(tagSelectModalAtom);
  const setModalState = useSetAtom(tagSelectModalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const open = modalState.open;
  const close = useCallback(() => setModalState({ open: false }), [setModalState]);

  const [recentTagKeys, recordRecent] = useRecentItems('picto-recent-tags');
  const [query, setQuery] = useState('');
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [entityTagKeys, setEntityTagKeys] = useState<Set<string>>(new Set());
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [unchecked, setUnchecked] = useState<Set<string>>(new Set());
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('all');
  const [activeNamespace, setActiveNamespace] = useState<string | null>(null);
  const [focusIdx, setFocusIdx] = useState(-1);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    const tagIds = entityData?.tag_ids ?? [];
    if (!open || tagIds.length === 0) {
      setEntityTagKeys(new Set());
      return;
    }
    void tagsController.getById(tagIds).then((records) => {
      if (!cancelled) setEntityTagKeys(new Set(records.map(tagName)));
    }).catch(() => {
      if (!cancelled) setEntityTagKeys(new Set());
    });
    return () => { cancelled = true; };
  }, [entityData?.tag_ids.join(','), open]);

  const loadTags = useCallback((search: string, ns?: string | null) => {
    void tagsController.getPaginated({
      limit: PAGE_SIZE,
      search: search.trim() || null,
      namespace: ns ?? null,
    }).then((result) => {
      setTags(result.tags);
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
      setTags((prev) => [...prev, ...result.tags]);
      setCursor(result.next_cursor);
      setLoadingMore(false);
    }).catch(() => { setLoadingMore(false); });
  }, [cursor, loadingMore, query, sidebarMode, activeNamespace]);

  useEffect(() => {
    if (!open) return;
    void tagsController.getNamespaceSummary().then((ns) => setNamespaces(ns ?? [])).catch(() => {});
  }, [open]);

  useEffect(() => {
    if (!open) return;
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      const ns = sidebarMode === 'namespace' ? activeNamespace : null;
      loadTags(query, ns);
    }, 150);
    return () => { if (searchTimerRef.current) clearTimeout(searchTimerRef.current); };
  }, [query, open, loadTags, sidebarMode, activeNamespace]);

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

  useEffect(() => {
    if (!open) return;
    if (sidebarMode === 'selected' || sidebarMode === 'recent') return;
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    loadTags(query, ns);
  }, [sidebarMode, activeNamespace]); // eslint-disable-line react-hooks/exhaustive-deps

  const displayTags = useMemo(() => {
    if (sidebarMode === 'selected') {
      return [...entityTagKeys].map(tagRecord).sort((a, b) => {
        const nsA = (a.namespace ?? '').toLowerCase();
        const nsB = (b.namespace ?? '').toLowerCase();
        if (nsA !== nsB) return nsA.localeCompare(nsB);
        return a.subname.localeCompare(b.subname);
      });
    }
    if (sidebarMode === 'recent') {
      const tagMap = new Map(tags.map((t) => [formatTag(t), t]));
      const result: CanonicalTagRecord[] = [];
      for (const key of recentTagKeys) {
        const tag = tagMap.get(key);
        if (tag) result.push(tag);
        if (result.length >= 30) break;
      }
      return result;
    }
    return tags;
  }, [tags, sidebarMode, entityTagKeys, recentTagKeys]);

  const nsGroups = useMemo(() =>
    [...namespaces].sort((a, b) => tagGroupOrder(a.name) - tagGroupOrder(b.name)),
  [namespaces]);

  const estimatedTotal = useMemo(() => {
    if (sidebarMode === 'selected' || sidebarMode === 'recent') return displayTags.length;
    if (query.trim()) return displayTags.length;
    if (sidebarMode === 'namespace' && activeNamespace != null) {
      const ns = namespaces.find((n) => n.name === activeNamespace);
      return ns ? ns.tag_count : displayTags.length;
    }
    const total = namespaces.reduce((sum, n) => sum + n.tag_count, 0);
    return total > 0 ? total : displayTags.length;
  }, [sidebarMode, activeNamespace, namespaces, displayTags.length, query]);

  const virtualCount = cursor ? Math.max(displayTags.length, estimatedTotal) : displayTags.length;
  const virtualizer = useVirtualizer({
    count: virtualCount,
    getScrollElement: () => listRef.current,
    estimateSize: () => 28,
    overscan: 15,
  });

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

  useEffect(() => {
    if (focusIdx >= 0) virtualizer.scrollToIndex(focusIdx, { align: 'auto' });
  }, [focusIdx, virtualizer]);

  const toggleTag = useCallback((tag: string) => {
    const isOnEntity = entityTagKeys.has(tag);
    if (isOnEntity) {
      setUnchecked((prev) => {
        const next = new Set(prev);
        if (next.has(tag)) next.delete(tag); else next.add(tag);
        return next;
      });
      setChecked((prev) => { const next = new Set(prev); next.delete(tag); return next; });
    } else {
      setChecked((prev) => {
        const next = new Set(prev);
        if (next.has(tag)) next.delete(tag); else next.add(tag);
        return next;
      });
    }
  }, [entityTagKeys]);

  const applyTags = useCallback(() => {
    if (!target || (checked.size === 0 && unchecked.size === 0)) return;
    if (checked.size > 0) void entityMutations.addTargetTags(target, [...checked]);
    if (unchecked.size > 0) void entityMutations.removeTargetTags(target, [...unchecked]);
    if (checked.size > 0) recordRecent([...checked]);
    close();
    setChecked(new Set());
    setUnchecked(new Set());
  }, [target, checked, unchecked, close, recordRecent]);

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
        if (nsGroups.length > 0) { setSidebarMode('namespace'); setActiveNamespace(nsGroups[0].name); }
        else setSidebarMode('selected');
      } else if (sidebarMode === 'namespace') {
        const idx = nsGroups.findIndex((g) => g.name === activeNamespace);
        if (idx < nsGroups.length - 1) setActiveNamespace(nsGroups[idx + 1].name);
        else setSidebarMode('selected');
      } else if (sidebarMode === 'selected') {
        setSidebarMode('recent');
      } else {
        setSidebarMode('all');
      }
    }
  }, [displayTags, focusIdx, toggleTag, sidebarMode, nsGroups, activeNamespace]);

  const pendingCount = checked.size + unchecked.size;
  const summaryParts: string[] = [];
  if (checked.size > 0) summaryParts.push(`+${checked.size}`);
  if (unchecked.size > 0) summaryParts.push(`−${unchecked.size}`);
  const summaryText = summaryParts.length > 0
    ? `${summaryParts.join(', ')}${selectionCount > 0 ? ` · ${selectionCount} file${selectionCount !== 1 ? 's' : ''}` : ''}`
    : (selectionCount > 0 ? `${selectionCount} file${selectionCount !== 1 ? 's' : ''} selected` : '');

  return (
    <GlassModal
      open={open}
      onClose={close}
      title="Add Tags"
      size="lg"
      flush
      footer={
        <>
          <span className={styles.footerLeft}>
            {summaryText && <span className={styles.summaryText}>{summaryText}</span>}
            {!summaryText && (
              <span className={shellStyles.kbdHint}>
                <span className={shellStyles.kbd}>↑↓</span>
                <span className={shellStyles.kbd}>↩</span> Select
                <span className={shellStyles.kbd}>Tab</span> Switch
              </span>
            )}
          </span>
          <div className={btnStyles.btnGroup}>
            <button className={btnStyles.btn} onClick={close} type="button">Cancel</button>
            {pendingCount > 0 && (
              <button data-modal-primary="true" className={`${btnStyles.btn} ${btnStyles.btnPrimary}`} onClick={applyTags} type="button">
                Apply ({pendingCount})
              </button>
            )}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        {/* Search bar */}
        <div className={styles.searchBar}>
          <IconSearch size={14} className={styles.searchIcon} />
          <input
            ref={searchRef}
            className={styles.searchInput}
            placeholder="Search tags..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </div>

        <div className={styles.mainArea}>
          {/* Sidebar — always visible, no collapse */}
          <div className={styles.sidebar}>
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'recent' ? styles.sidebarItemActive : ''}`}
              onClick={() => { setSidebarMode('recent'); setActiveNamespace(null); }}
            >
              <span className={styles.sidebarName}>Recent</span>
              <span className={styles.sidebarBadge}>{recentTagKeys.length}</span>
            </div>
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'selected' ? styles.sidebarItemActive : ''}`}
              onClick={() => { setSidebarMode('selected'); setActiveNamespace(null); }}
            >
              <span className={styles.sidebarName}>Selected</span>
              <span className={styles.sidebarBadge}>{entityTagKeys.size}</span>
            </div>
            <div className={styles.sidebarSep} />
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'all' ? styles.sidebarItemActive : ''}`}
              onClick={() => { setSidebarMode('all'); setActiveNamespace(null); }}
            >
              <span className={styles.sidebarName}>All</span>
              <span className={styles.sidebarBadge}>
                {namespaces.reduce((sum, n) => sum + n.tag_count, 0).toLocaleString()}
              </span>
            </div>
            {nsGroups.length > 0 && <div className={styles.sidebarSep} />}
            {nsGroups.map((ns) => (
              <div
                key={ns.namespace_id}
                className={`${styles.sidebarItem} ${sidebarMode === 'namespace' && activeNamespace === ns.name ? styles.sidebarItemActive : ''}`}
                onClick={() => { setSidebarMode('namespace'); setActiveNamespace(ns.name); }}
              >
                <span className={styles.sidebarDot} style={{ background: tagGroupColor(ns.name) }} />
                <span className={styles.sidebarName}>{ns.name || 'general'}</span>
                <span className={styles.sidebarBadge}>{ns.tag_count.toLocaleString()}</span>
              </div>
            ))}
          </div>

          {/* Tag list */}
          <div className={styles.content}>
            <div ref={listRef} className={styles.tagListScroller}>
              {displayTags.length === 0 ? (
                <div className={styles.emptyState}>
                  {sidebarMode === 'selected' ? 'No tags on this entity'
                    : sidebarMode === 'recent' ? 'No recently used tags'
                    : 'No tags found'}
                </div>
              ) : (
                <div className={styles.tagListInner} style={{ height: virtualizer.getTotalSize() }}>
                  {virtualizer.getVirtualItems().map((vItem) => {
                    const tag = displayTags[vItem.index];
                    if (!tag) {
                      if (cursor && !loadingMore && vItem.index >= displayTags.length) loadMore();
                      return null;
                    }
                    const fullTag = formatTag(tag);
                    const ns = (tag.namespace ?? '').toLowerCase();
                    const isOnEntity = entityTagKeys.has(fullTag);
                    const isPendingRemove = unchecked.has(fullTag);
                    const isPendingAdd = checked.has(fullTag);
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
                        {ns && ns !== 'general' && ns !== '' && (
                          <span className={styles.tagNs}>{tag.namespace}:</span>
                        )}
                        <span className={styles.tagName}>
                          {query.trim() ? highlightMatch(tag.subname || fullTag, query.trim()) : (tag.subname || fullTag)}
                        </span>
                        <span className={styles.tagBadge}>{tag.active_count.toLocaleString()}</span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </GlassModal>
  );
}

function formatTag(tag: CanonicalTagRecord): string {
  return tag.namespace ? `${tag.namespace}:${tag.subname}` : tag.subname;
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
