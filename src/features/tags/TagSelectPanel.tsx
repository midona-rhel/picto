/**
 * TagSelectPanel — floating panel for tag selection.
 *
 * 540×480 with the group rail, 340×480 without it.
 * Virtual scrolling for large tag sets. Cursor-based pagination on scroll.
 * Header = search input + pin button (no title bar).
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconAdjustmentsHorizontal,
  IconBookmark,
  IconCheck,
  IconLayoutGrid,
  IconLayoutList,
  IconLayoutSidebar,
  IconPlus,
  IconSearch,
  IconStar,
  IconX,
} from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { tagSelectPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import * as entityMutations from '../../controllers/entityMutations';
import { tagsController } from '../../controllers/tagsController';
import type { CanonicalTagRecord, CanonicalNamespaceSummary, SetMatchMode } from '../../shared/types/canonical';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import { tagGroupColor, tagGroupOrder } from './tagGroupPresentation';
import styles from './TagSelectPanel.module.css';
import { FilterLogicTabs } from '../../shared/ui/FilterLogicTabs';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
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
type TagLayout = 'grid' | 'list';
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
  const selectedTagFilters = portalState.selectedTagFilters;
  const excludedTagFilters = portalState.excludedTagFilters;
  const onApplyTags = portalState.onApplyTags;
  const onApplyTagFilter = portalState.onApplyTagFilter;
  const closePortal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [layout, setLayout] = useState<TagLayout>(() => (
    localStorage.getItem('picto-tag-picker-layout') === 'list' ? 'list' : 'grid'
  ));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showCounts, setShowCounts] = useState(() => localStorage.getItem('picto-tag-picker-counts') !== 'false');
  const [query, setQuery] = useState('');
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [assignedTagKeys, setAssignedTagKeys] = useState<Set<string>>(new Set());
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set());
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [matchMode, setMatchMode] = useState<SetMatchMode>('any');
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('all');
  const [activeNamespace, setActiveNamespace] = useState<string | null>(null);
  const [focusIdx, setFocusIdx] = useState(-1);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const loadingCursorRef = useRef<string | null>(null);
  const requestGenerationRef = useRef(0);
  const contextMenu = useContextMenu();
  const tagPreferences = useTagPreferences();

  // Tags already on the selected entity (for "Selected" sidebar mode)
  const entityTagKeys = useMemo(() => selectedTagFilters
    ? new Set(selectedTagFilters.map((tag) => tag.name))
    : customTags ? new Set(customTags) : assignedTagKeys,
  [assignedTagKeys, customTags, selectedTagFilters]);

  useEffect(() => {
    let cancelled = false;
    const tagIds = entityData?.tag_ids ?? [];
    if (!open || customTags || selectedTagFilters || tagIds.length === 0) {
      setAssignedTagKeys(new Set());
      return;
    }
    void tagsController.getById(tagIds).then((records) => {
      if (!cancelled) setAssignedTagKeys(new Set(records.map(tagName)));
    }).catch(() => {
      if (!cancelled) setAssignedTagKeys(new Set());
    });
    return () => { cancelled = true; };
  }, [customTags, entityData?.tag_ids.join(','), open, selectedTagFilters]);

  const tagChoices = useMemo(() => new Map([
    ...(selectedTagFilters ?? []).map((tag) => [tag.name, tag] as const),
    ...(excludedTagFilters ?? []).map((tag) => [tag.name, tag] as const),
    ...tags.filter((tag) => tag.tag_id > 0).map((tag) => [formatTag(tag), {
      tag_id: tag.tag_id,
      name: formatTag(tag),
    }] as const),
  ]), [excludedTagFilters, selectedTagFilters, tags]);

  // Load initial tags + namespaces
  const loadTags = useCallback((search: string, ns?: string | null) => {
    const generation = ++requestGenerationRef.current;
    loadingCursorRef.current = null;
    setLoadingMore(false);
    const params: Parameters<typeof tagsController.getPaginated>[0] = {
      limit: PAGE_SIZE,
      search: search.trim() || null,
      namespace: ns ?? null,
    };
    void tagsController.getPaginated(params).then((result) => {
      if (generation !== requestGenerationRef.current) return;
      setTags(result.tags);
      setCursor(result.next_cursor);
      setFocusIdx(-1);
    }).catch(() => {});
  }, []);

  const loadMore = useCallback(() => {
    if (!cursor || loadingCursorRef.current === cursor) return;
    const generation = requestGenerationRef.current;
    loadingCursorRef.current = cursor;
    setLoadingMore(true);
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    void tagsController.getPaginated({
      limit: PAGE_SIZE,
      search: query.trim() || null,
      namespace: ns,
      cursor,
    }).then((result) => {
      if (generation !== requestGenerationRef.current) return;
      setTags((prev) => {
        const known = new Set(prev.map((tag) => tag.tag_id));
        return [...prev, ...result.tags.filter((tag) => !known.has(tag.tag_id))];
      });
      setCursor(result.next_cursor);
    }).finally(() => {
      loadingCursorRef.current = null;
      setLoadingMore(false);
    });
  }, [cursor, query, sidebarMode, activeNamespace]);

  // Load namespaces on open
  useEffect(() => {
    if (!open) return;
    void tagsController.getNamespaceSummary().then((ns) => setNamespaces(ns ?? [])).catch(() => {});
  }, [open]);

  // One request owner for search and namespace changes. The generation guard
  // prevents a slower previous view from replacing the current result.
  useEffect(() => {
    if (!open) return;
    if (listRef.current) listRef.current.scrollTop = 0;
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    const ns = sidebarMode === 'namespace' ? activeNamespace : null;
    if (!query.trim()) {
      loadTags(query, ns);
      return;
    }
    searchTimerRef.current = setTimeout(() => loadTags(query, ns), 150);
    return () => { if (searchTimerRef.current) clearTimeout(searchTimerRef.current); };
  }, [query, open, loadTags, sidebarMode, activeNamespace]);

  // Reset on open
  useEffect(() => {
    if (open) {
      setQuery('');
      setSettingsOpen(false);
      setSelectedTags(new Set(entityTagKeys));
      setExcluded(new Set(excludedTagFilters?.map((tag) => tag.name) ?? customExcludedTags ?? []));
      setMatchMode(portalState.filterMatchMode ?? 'any');
      setSidebarMode('all');
      setActiveNamespace(null);
      setFocusIdx(-1);
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open, customExcludedTags, excludedTagFilters, portalState.filterMatchMode]); // entityTagKeys is the opening snapshot

  // Display tags — "selected" mode shows entity's tags directly (not from paginated search)
  const displayTags = useMemo(() => {
    let candidates: CanonicalTagRecord[];
    if (sidebarMode === 'selected') {
      candidates = [...entityTagKeys].map(tagRecord).sort((a, b) => {
        const nsA = (a.namespace ?? '').toLowerCase();
        const nsB = (b.namespace ?? '').toLowerCase();
        if (nsA !== nsB) return nsA.localeCompare(nsB);
        return a.subname.localeCompare(b.subname);
      });
    } else if (sidebarMode === 'starred') {
      candidates = tagPreferences.starredTags.map(tagRecord).sort((left, right) => (
        formatTag(left).localeCompare(formatTag(right))
      ));
    } else {
      candidates = tags;
    }

    const normalizedQuery = query.trim().toLocaleLowerCase();
    const seen = new Set<string>();
    return candidates.filter((tag) => {
      const visibleName = (tag.subname || formatTag(tag)).toLocaleLowerCase();
      if (seen.has(visibleName)) return false;
      seen.add(visibleName);
      const searchableName = normalizedQuery.includes(':')
        ? formatTag(tag).toLocaleLowerCase()
        : visibleName;
      return !normalizedQuery || searchableName.includes(normalizedQuery);
    });
  }, [tags, sidebarMode, entityTagKeys, query, tagPreferences.starredTags]);

  const createTag = useMemo(() => {
    const trimmed = query.trim();
    if (!trimmed || onApplyTagFilter || sidebarMode === 'selected' || sidebarMode === 'starred') return null;
    const name = sidebarMode === 'namespace' && activeNamespace && !trimmed.includes(':')
      ? `${activeNamespace}:${trimmed}`
      : trimmed;
    const alreadyExists = tags.some((tag) => formatTag(tag).toLocaleLowerCase() === name.toLocaleLowerCase());
    return alreadyExists ? null : tagRecord(name);
  }, [activeNamespace, onApplyTagFilter, query, sidebarMode, tags]);

  // Namespace groups for sidebar
  const nsGroups = useMemo(() => {
    return namespaces
      .filter((group) => group.name !== '' && group.name !== 'general')
      .sort((a, b) => tagGroupOrder(a.name) - tagGroupOrder(b.name));
  }, [namespaces]);

  const columnCount = layout === 'grid' ? 2 : 1;
  const navigableTags = createTag ? [createTag, ...displayTags] : displayTags;
  const displayPages = useMemo(() => Array.from(
    { length: Math.ceil(displayTags.length / PAGE_SIZE) },
    (_, index) => displayTags.slice(index * PAGE_SIZE, (index + 1) * PAGE_SIZE),
  ), [displayTags]);

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

  // Keep keyboard focus visible without coupling layout to the tag data source.
  useEffect(() => {
    if (focusIdx < 0) return;
    listRef.current?.querySelector<HTMLElement>(`[data-tag-index="${focusIdx}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [focusIdx]);

  const changeLayout = useCallback((next: TagLayout) => {
    setLayout(next);
    localStorage.setItem('picto-tag-picker-layout', next);
  }, []);

  const toggleCounts = useCallback(() => {
    setShowCounts((current) => {
      localStorage.setItem('picto-tag-picker-counts', String(!current));
      return !current;
    });
  }, []);

  const selectedChoices = useCallback((names: Set<string>) => [...names]
    .map((name) => tagChoices.get(name))
    .filter((tag): tag is NonNullable<typeof tag> => tag != null), [tagChoices]);

  const toggleTag = useCallback((tag: CanonicalTagRecord) => {
    const name = formatTag(tag);
    if (onApplyTagFilter && tag.tag_id <= 0) return;
    const nextSelected = new Set(selectedTags);
    const nextExcluded = new Set(excluded);
    const removing = nextSelected.delete(name);
    if (!removing) nextSelected.add(name);
    nextExcluded.delete(name);
    setSelectedTags(nextSelected);
    setExcluded(nextExcluded);

    if (onApplyTagFilter) {
      onApplyTagFilter(selectedChoices(nextSelected), selectedChoices(nextExcluded), matchMode);
      return;
    }
    if (onApplyTags) {
      onApplyTags([...nextSelected]);
    } else if (target) {
      const mutation = removing
        ? entityMutations.removeTargetTags(target, [name])
        : entityMutations.addTargetTags(target, [name]);
      void mutation.then(() => {
        if (tag.tag_id > 0 || removing) return;
        const namespace = sidebarMode === 'namespace' ? activeNamespace : null;
        loadTags(query, namespace);
        void tagsController.getNamespaceSummary().then((groups) => setNamespaces(groups ?? []));
      });
    }
  }, [activeNamespace, excluded, loadTags, matchMode, onApplyTagFilter, onApplyTags, query, selectedChoices, selectedTags, sidebarMode, target]);

  const toggleExcludedTag = useCallback((tag: CanonicalTagRecord) => {
    if (!onApplyTagFilter || tag.tag_id <= 0) return;
    const name = formatTag(tag);
    const nextSelected = new Set(selectedTags);
    const nextExcluded = new Set(excluded);
    nextSelected.delete(name);
    if (!nextExcluded.delete(name)) nextExcluded.add(name);
    setSelectedTags(nextSelected);
    setExcluded(nextExcluded);
    onApplyTagFilter(selectedChoices(nextSelected), selectedChoices(nextExcluded), matchMode);
  }, [excluded, matchMode, onApplyTagFilter, selectedChoices, selectedTags]);

  const changeMatchMode = useCallback((mode: SetMatchMode) => {
    setMatchMode(mode);
    onApplyTagFilter?.(selectedChoices(selectedTags), selectedChoices(excluded), mode);
  }, [excluded, onApplyTagFilter, selectedChoices, selectedTags]);

  // Keyboard
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setFocusIdx((i) => Math.min(i + columnCount, navigableTags.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setFocusIdx((i) => Math.max(i - columnCount, -1));
    } else if (e.key === 'ArrowRight' && columnCount > 1) {
      e.preventDefault();
      setFocusIdx((i) => Math.min(i + 1, navigableTags.length - 1));
    } else if (e.key === 'ArrowLeft' && columnCount > 1) {
      e.preventDefault();
      setFocusIdx((i) => Math.max(i - 1, -1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const idx = focusIdx >= 0 ? focusIdx : 0;
      if (navigableTags[idx]) toggleTag(navigableTags[idx]);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      if (sidebarMode === 'all') {
        if (nsGroups.length > 0) { setSidebarMode('namespace'); setActiveNamespace(nsGroups[0].name); }
        else setSidebarMode('selected');
      } else if (sidebarMode === 'namespace') {
        const idx = nsGroups.findIndex((g) => g.name === activeNamespace);
        if (idx < nsGroups.length - 1) setActiveNamespace(nsGroups[idx + 1].name);
        else setSidebarMode('selected');
      } else {
        setSidebarMode('all');
      }
    }
  }, [activeNamespace, columnCount, focusIdx, navigableTags, nsGroups, sidebarMode, toggleTag]);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={closePortal}
      width={showSidebar ? 540 : 340}
      height={480}
      pinned={pinned}
      anchorPosition={anchorPosition}
      anchorPlacement={portalState.anchorPlacement}
      onPinnedChange={setPinned}
      header={
        <>
          <div
            className={shellStyles.searchRow}
            style={{ flex: 1 }}
            onMouseDown={(event) => {
              event.stopPropagation();
              searchRef.current?.focus();
            }}
          >
            <IconSearch size={14} className={shellStyles.searchIcon} />
            <input
              ref={searchRef}
              className={shellStyles.searchInput}
              placeholder="Search..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
            />
          </div>
          {onApplyTagFilter ? <FilterLogicTabs value={matchMode} onChange={changeMatchMode} /> : null}
          <KbdTooltip label={showSidebar ? 'Hide sidebar' : 'Show sidebar'}><button
            className={shellStyles.pinBtn}
            onClick={() => setShowSidebar((v) => !v)}
            type="button"
            aria-label={showSidebar ? 'Hide sidebar' : 'Show sidebar'}
          >
            <IconLayoutSidebar size={14} />
          </button></KbdTooltip>
          <KbdTooltip label="Tag picker settings"><button
            className={`${shellStyles.pinBtn} ${settingsOpen ? shellStyles.pinBtnActive : ''}`}
            onClick={() => setSettingsOpen((current) => !current)}
            type="button"
            aria-label="Tag picker settings"
          ><IconAdjustmentsHorizontal size={14} /></button></KbdTooltip>
        </>
      }
      footer={
        <>
          {showSidebar ? <span className={shellStyles.kbdHint}>Switch <span className={shellStyles.kbd}>Tab</span></span> : null}
          <span className={shellStyles.kbdHint}>
            Move <span className={shellStyles.kbd}>↑</span><span className={shellStyles.kbd}>↓</span>
            {layout === 'grid' ? <><span className={shellStyles.kbd}>←</span><span className={shellStyles.kbd}>→</span></> : null}
            Select <span className={shellStyles.kbd}>↵</span>
          </span>
          <span className={shellStyles.kbdHint}>Close <span className={shellStyles.kbd}>Esc</span></span>
        </>
      }
    >
      <div className={styles.panelBody}>
        {/* Sidebar — always mounted, fades with collapse */}
        <div className={`${styles.sidebar} ${!showSidebar ? styles.sidebarHidden : ''}`}>
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'all' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('all'); setActiveNamespace(null); }}
          >
            <IconBookmark className={styles.sidebarIcon} size={10} fill="currentColor" fillOpacity={0.28} />
            <span className={styles.sidebarName}>All</span>
            <span className={styles.sidebarBadge}>
              {namespaces.reduce((sum, n) => sum + n.tag_count, 0).toLocaleString()}
            </span>
          </div>
          {tagPreferences.showTagGroups && nsGroups.map((ns) => (
            <div
              key={ns.namespace_id}
              className={`${styles.sidebarItem} ${sidebarMode === 'namespace' && activeNamespace === ns.name ? styles.sidebarItemActive : ''}`}
              onClick={() => { setSidebarMode('namespace'); setActiveNamespace(ns.name); }}
            >
              <span className={styles.sidebarDot} style={{ background: tagGroupColor(ns.name, tagPreferences.tagGroupColors) }} />
              <span className={styles.sidebarName}>{ns.name || 'general'}</span>
              <span className={styles.sidebarBadge}>{ns.tag_count.toLocaleString()}</span>
            </div>
          ))}
          <div className={styles.sidebarSep} />
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'selected' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('selected'); setActiveNamespace(null); }}
          >
            <IconBookmark className={styles.sidebarIcon} size={10} fill="currentColor" fillOpacity={0.28} />
            <span className={styles.sidebarName}>Selected</span>
            <span className={styles.sidebarBadge}>{entityTagKeys.size}</span>
          </div>
          <div
            className={`${styles.sidebarItem} ${sidebarMode === 'starred' ? styles.sidebarItemActive : ''}`}
            onClick={() => { setSidebarMode('starred'); setActiveNamespace(null); }}
          >
            <IconStar className={styles.sidebarStar} size={10} />
            <span className={styles.sidebarName}>Starred</span>
            <span className={styles.sidebarBadge}>{tagPreferences.starredTags.length}</span>
          </div>
        </div>

        {/* Content */}
        <div className={`${styles.content} ${!showSidebar ? styles.contentExpanded : ''}`}>
          <div ref={listRef} className={styles.tagListScroller}>
            {navigableTags.length === 0 ? (
              <div className={styles.emptyState}>
                {sidebarMode === 'selected' ? 'No tags on this entity' : 'No tags found'}
              </div>
            ) : (
              <div className={styles.tagPages}>
                {createTag ? (
                  <div
                    data-tag-index={0}
                    className={`${styles.tagRow} ${styles.createTagRow} ${focusIdx === 0 ? styles.tagRowFocused : ''}`}
                    onClick={() => toggleTag(createTag)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                    }}
                  >
                    <IconPlus aria-hidden="true" className={styles.createTagIcon} size={10} />
                    <span className={styles.tagName}>
                      Create &quot;<strong>{formatTag(createTag)}</strong>&quot;
                    </span>
                  </div>
                ) : null}
                {displayPages.map((page, pageIndex) => (
                  <div
                    className={styles.tagGrid}
                    key={pageIndex}
                    style={{
                      gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
                    }}
                  >
                    {page.map((tag, pageItemIndex) => {
                      const itemIndex = (createTag ? 1 : 0) + pageIndex * PAGE_SIZE + pageItemIndex;
                      const fullTag = formatTag(tag);
                      const isExcluded = excluded.has(fullTag);
                      const showChecked = selectedTags.has(fullTag);
                      const isFocused = itemIndex === focusIdx;
                      return (
                        <div
                          key={fullTag}
                          data-tag-index={itemIndex}
                          className={`${styles.tagRow} ${isFocused ? styles.tagRowFocused : ''} ${showChecked ? styles.tagRowSelected : ''} ${isExcluded ? styles.tagRowExcluded : ''}`}
                          style={{
                            '--tag-color': tagGroupColor(tag.namespace, tagPreferences.tagGroupColors),
                          } as React.CSSProperties}
                          onClick={() => toggleTag(tag)}
                          onContextMenu={(event) => {
                        if (onApplyTagFilter) {
                          event.preventDefault();
                          event.stopPropagation();
                          toggleExcludedTag(tag);
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
                          {onApplyTagFilter ? (
                            <div className={`${shellStyles.checkBox} ${isExcluded ? shellStyles.checkBoxExcluded : showChecked ? shellStyles.checkBoxFilterChecked : ''}`}>
                              {isExcluded ? <IconX size={10} /> : showChecked ? <IconCheck size={10} /> : null}
                            </div>
                          ) : null}
                          {tagPreferences.starredTags.includes(fullTag) ? (
                            <IconStar aria-hidden="true" className={styles.tagStar} size={10} />
                          ) : (
                            <IconBookmark
                              aria-hidden="true"
                              className={styles.tagBookmark}
                              size={10}
                              stroke={showChecked && !onApplyTagFilter ? 2 : 1.6}
                              fill="currentColor"
                              fillOpacity={showChecked && !onApplyTagFilter ? 0.58 : 0.28}
                            />
                          )}
                          <span className={styles.tagName}>
                            {query.trim()
                              ? highlightMatch(displayTagName(tag, tagPreferences.showTagPrefixes), query.trim())
                              : displayTagName(tag, tagPreferences.showTagPrefixes)}
                          </span>
                          {showCounts ? <span className={styles.tagBadge}>({tag.active_count.toLocaleString()})</span> : null}
                        </div>
                      );
                    })}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
        {settingsOpen ? (
          <>
            <button className={styles.settingsBackdrop} type="button" aria-label="Close tag picker settings" onClick={() => setSettingsOpen(false)} />
            <div className={styles.settingsPanel}>
              <div className={styles.settingsRow}>
                <span>Layout</span>
                <div className={shellStyles.viewTabs} role="group" aria-label="Tag layout">
                  <KbdTooltip label="List"><button type="button" className={`${shellStyles.viewTab} ${layout === 'list' ? shellStyles.viewTabActive : ''}`} aria-label="List tags" aria-pressed={layout === 'list'} onClick={() => changeLayout('list')}><IconLayoutList size={14} /></button></KbdTooltip>
                  <KbdTooltip label="Grid"><button type="button" className={`${shellStyles.viewTab} ${layout === 'grid' ? shellStyles.viewTabActive : ''}`} aria-label="Grid tags" aria-pressed={layout === 'grid'} onClick={() => changeLayout('grid')}><IconLayoutGrid size={14} /></button></KbdTooltip>
                </div>
              </div>
              <button className={styles.settingsRow} type="button" onClick={toggleCounts}>
                <span>Show counts</span>
                <span className={`${shellStyles.checkBox} ${showCounts ? shellStyles.checkBoxChecked : ''}`}>{showCounts ? <IconCheck size={10} /> : null}</span>
              </button>
            </div>
          </>
        ) : null}
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

function displayTagName(tag: CanonicalTagRecord, showPrefix: boolean): string {
  const fullName = formatTag(tag);
  if (showPrefix) return fullName;
  const separator = fullName.indexOf(':');
  return separator < 0 ? fullName : fullName.slice(separator + 1);
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
