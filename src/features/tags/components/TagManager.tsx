import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useDebouncedCallback } from '../../../shared/hooks/useDebouncedCallback';
import { Loader, Modal, TextInput } from '@mantine/core';
import { TextButton } from '../../../shared/components/TextButton';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  IconLayoutGrid,
  IconList,
  IconSearch,
  IconGitMerge,
  IconBookmark,
  IconFolderQuestion,
  IconArrowsExchange,
  IconArrowUp,
  IconArrowDown,
} from '@tabler/icons-react';
import { writeText } from '#desktop/api';
import { tagsController } from '../../../controllers/tagsController';
import { notifySuccess, notifyError } from '../../../shared/lib/notify';
import { getNamespaceColor } from '../../../shared/lib/namespaceColors';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import { useNavigationStore } from '../../../state/navigationStore';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from '../../../shared/components/ContextMenu';
import { TagRelationsModal } from './TagRelationsModal';
import { buildTagContextMenu } from '../../../shared/components/context-actions/tagActions';
import { useTagListStore } from '../../../state/tagListStore';
import classes from './TagManager.module.css';

interface TagRecord {
  tag_id: number;
  namespace: string;
  subtag: string;
  file_count: number;
}

interface NamespaceSummary {
  namespace: string;
  count: number;
}

import type { TagRelation, TagSearchResult } from '../../../shared/types/api';

function formatTagDisplay(ns: string, subtag: string): string {
  return ns ? `${ns}:${subtag}` : subtag;
}

function nsDotColor(ns: string): string {
  const [r, g, b] = getNamespaceColor(ns, true);
  return `rgb(${r}, ${g}, ${b})`;
}

function relationActionLabel(
  kind: 'alias' | 'implication' | 'reverse_implication',
): string {
  if (kind === 'alias') return 'alias';
  if (kind === 'implication') return 'implication';
  return 'implied-by relation';
}

const ROW_HEIGHT = 27;

export function TagManager() {
  const [namespaces, setNamespaces] = useState<NamespaceSummary[]>([]);
  const [selectedNs, setSelectedNs] = useState<string | null>(null);
  const [totalTagCount, setTotalTagCount] = useState(0);

  const [tags, setTags] = useState<TagRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [listMode, setListMode] = useState(false);
  const [hasMore, setHasMore] = useState(true);

  const [selectedTagIds, setSelectedTagIds] = useState<Set<number>>(new Set());
  const [selectAll, setSelectAll] = useState(false);
  const lastClickedTagIdRef = useRef<number | null>(null);

  const [containerWidth, setContainerWidth] = useState(800);

  const [mergeSource, setMergeSource] = useState<TagRecord | null>(null);
  const [mergeSearch, setMergeSearch] = useState('');
  const [mergeResults, setMergeResults] = useState<TagSearchResult[]>([]);
  const [mergeTarget, setMergeTarget] = useState<TagRecord | null>(null);

  const [relationModal, setRelationModal] = useState<{ type: 'alias' | 'implication' | 'reverse_implication'; source: TagRecord } | null>(null);
  const [relationSearch, setRelationSearch] = useState('');
  const [relationResults, setRelationResults] = useState<TagSearchResult[]>([]);
  const [relationTarget, setRelationTarget] = useState<TagRecord | null>(null);

  const [relationsTag, setRelationsTag] = useState<TagRecord | null>(null);

  const ctxMenu = useContextMenu();

  const scrollRef = useRef<HTMLDivElement>(null);
  const loadingMoreRef = useRef(false);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setContainerWidth(entry.contentRect.width);
      }
    });
    ro.observe(el);
    setContainerWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const columns = listMode ? 1 : Math.max(1, Math.floor(containerWidth / 200));

  const activeCount = selectedNs === null
    ? totalTagCount
    : namespaces.find((n) => n.namespace === selectedNs)?.count ?? 0;

  const rename = useInlineRename(async (id, newName) => {
      const tagId = parseInt(id);
      try {
        const oldTag = tags.find((t) => t.tag_id === tagId);
        const oldDisplay = oldTag ? formatTagDisplay(oldTag.namespace, oldTag.subtag) : '';
        await tagsController.rename(tagId, newName, oldDisplay);
      notifySuccess(`Renamed to "${newName}"`, 'Tag Renamed');
    } catch (err) {
      notifyError(err);
    }
  });

  const fetchNamespaces = useCallback(async () => {
    try {
      const result = await tagsController.getNamespaceSummary();
      setNamespaces(result);
      setTotalTagCount(result.reduce((sum: number, ns: NamespaceSummary) => sum + ns.count, 0));
    } catch (err) {
      console.error('Failed to load namespace summary:', err);
    }
  }, []);

  const fetchTags = useCallback(
    async (cursor?: string) => {
      try {
        const params = {
          namespace: selectedNs ?? undefined,
          search: searchQuery || undefined,
          cursor: cursor ?? undefined,
          limit: 500,
        };
        const result = await tagsController.getPaginated(params);
        if (cursor) {
          setTags((prev) => [...prev, ...result]);
        } else {
          setTags(result);
        }
        setHasMore(result.length === 500);
        return result;
      } catch (err) {
        console.error('Failed to load tags:', err);
        return [];
      }
    },
    [selectedNs, searchQuery],
  );

  const fetchAllHashesForTag = useCallback(async (tagDisplay: string): Promise<string[]> => {
    const limit = 5000;
    let offset = 0;
    const all: string[] = [];
    while (true) {
      const batch = await tagsController.findFilesByTags([tagDisplay], limit, offset);
      if (!batch || batch.length === 0) break;
      all.push(...batch);
      if (batch.length < limit) break;
      offset += batch.length;
    }
    return [...new Set(all)];
  }, []);

  const deleteTagByDisplay = useCallback(async (tagDisplay: string): Promise<void> => {
    const candidates = await tagsController.search(tagDisplay, 50);
    const exact = candidates.find((c) => {
      const formatted = formatTagDisplay(c.namespace, c.subtag);
      return formatted === tagDisplay || c.display === tagDisplay;
    });
    if (!exact) return;
    let affectedHashes: string[] = [];
    try {
      affectedHashes = await tagsController.findFilesByTags([tagDisplay]);
    } catch { /* best effort */ }
    await tagsController.deleteTag(exact.tag_id, tagDisplay, affectedHashes);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      await fetchNamespaces();
      await fetchTags();
      if (!cancelled) setLoading(false);
    })();
    return () => { cancelled = true; };
  }, [fetchNamespaces, fetchTags]);

  // Subscribe to eager tag list mutations from the controller.
  const pendingTagRemovals = useTagListStore((s) => s.pendingRemovals);
  const pendingTagRenames = useTagListStore((s) => s.pendingRenames);
  useEffect(() => {
    if (pendingTagRemovals.length === 0 && pendingTagRenames.length === 0) return;
    const { removals, renames } = useTagListStore.getState().drainMutations();

    if (removals.length > 0) {
      setTags((prev) =>
        prev.filter((t) => {
          for (const r of removals) {
            if (r.tagId != null && t.tag_id === r.tagId) return false;
            if (r.namespace != null && r.subtag != null && t.namespace === r.namespace && t.subtag === r.subtag) return false;
          }
          return true;
        }),
      );
      // Update namespace counts
      setNamespaces((prev) => {
        const decrements = new Map<string, number>();
        for (const r of removals) {
          const ns = r.namespace ?? '';
          decrements.set(ns, (decrements.get(ns) ?? 0) + 1);
        }
        return prev
          .map((ns) => {
            const dec = decrements.get(ns.namespace) ?? 0;
            return dec > 0 ? { ...ns, count: Math.max(0, ns.count - dec) } : ns;
          })
          .filter((ns) => ns.count > 0);
      });
      setTotalTagCount((prev) => Math.max(0, prev - removals.length));
    }

    if (renames.length > 0) {
      setTags((prev) =>
        prev.map((t) => {
          const rename = renames.find((r) => r.tagId === t.tag_id);
          return rename ? { ...t, namespace: rename.namespace, subtag: rename.subtag } : t;
        }),
      );
    }
  }, [pendingTagRemovals, pendingTagRenames]);

  const debouncedSearch = useDebouncedCallback((val: string) => setSearchQuery(val), 150);

  const loadMore = useCallback(async () => {
    if (loadingMoreRef.current || !hasMore || tags.length === 0) return;
    loadingMoreRef.current = true;
    const lastTag = tags[tags.length - 1];
    await fetchTags(`${lastTag.subtag}\0${lastTag.tag_id}`);
    loadingMoreRef.current = false;
  }, [tags, hasMore, fetchTags]);

  // When not searching, use the exact count from namespace summary for correct
  // scrollbar sizing. When searching, we only know loaded results.
  const knownTagCount = searchQuery ? tags.length : activeCount;
  const totalRows = Math.ceil((knownTagCount || tags.length) / columns);
  const loadedRows = Math.ceil(tags.length / columns);

  const virtualizer = useVirtualizer({
    count: totalRows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 30,
  });

  const virtualItems = virtualizer.getVirtualItems();
  useEffect(() => {
    if (virtualItems.length === 0 || !hasMore) return;
    const lastItem = virtualItems[virtualItems.length - 1];
    if (lastItem && lastItem.index >= loadedRows - 5) {
      loadMore();
    }
  }, [virtualItems, loadedRows, hasMore, loadMore]);

  const handleDeleteSelected = useCallback(async () => {
    if (!selectAll && selectedTagIds.size === 0) return;
    try {
      if (selectAll) {
        let deleted = 0;
        // eslint-disable-next-line no-constant-condition
        while (true) {
          const batch = await tagsController.getPaginated({
            namespace: selectedNs ?? undefined,
            search: searchQuery || undefined,
            limit: 500,
          });
          if (batch.length === 0) break;
          for (const tag of batch) {
            const tagFullName = formatTagDisplay(tag.namespace, tag.subtag);
            let affectedHashes: string[] = [];
            try { affectedHashes = await tagsController.findFilesByTags([tagFullName]); } catch { /* best effort */ }
            await tagsController.deleteTag(tag.tag_id, tagFullName, affectedHashes);
          }
          deleted += batch.length;
        }
        notifySuccess(`Deleted ${deleted} tag${deleted !== 1 ? 's' : ''}`);
      } else {
        const toDelete = tags.filter(t => selectedTagIds.has(t.tag_id));
        for (const tag of toDelete) {
          const tagFullName = formatTagDisplay(tag.namespace, tag.subtag);
          let affectedHashes: string[] = [];
          try { affectedHashes = await tagsController.findFilesByTags([tagFullName]); } catch { /* best effort */ }
          await tagsController.deleteTag(tag.tag_id, tagFullName, affectedHashes);
        }
        notifySuccess(`Deleted ${toDelete.length} tag${toDelete.length !== 1 ? 's' : ''}`);
      }
      setSelectAll(false);
      setSelectedTagIds(new Set());
      // Each tagsController.deleteTag() already signals queueRemoval — tags
      // disappear eagerly from the list via the tagListStore subscription.
    } catch (err) {
      notifyError(err);
    }
  }, [selectAll, selectedTagIds, tags, selectedNs, searchQuery]);

  const handleTagContextMenu = useCallback(
    async (e: React.MouseEvent, tag: TagRecord) => {
      e.preventDefault();
      e.stopPropagation();
      const pos = { x: e.clientX, y: e.clientY };
      const display = formatTagDisplay(tag.namespace, tag.subtag);

      const [aliases, relations] = await Promise.all([
        tagsController.getRelations(tag.tag_id, 'aliases').catch(() => [] as TagRelation[]),
        tagsController.getRelations(tag.tag_id, 'implications').catch(() => [] as TagRelation[]),
      ]);

      const parentTags = relations.filter((r) => r.relation === 'parent');
      const childTags = relations.filter((r) => r.relation === 'child');

      const navToTag = (ns: string, st: string) =>
        useNavigationStore.getState().navigateToFilterTags([formatTagDisplay(ns, st)]);
      const items: ContextMenuEntry[] = buildTagContextMenu({
        tag,
        aliases,
        parents: parentTags,
        children: childTags,
        formatTagDisplay,
        onShowImages: () => navToTag(tag.namespace, tag.subtag),
        onRename: () => rename.startRename(String(tag.tag_id), display),
        onMerge: () => {
          setMergeSource(tag);
          setMergeSearch('');
          setMergeResults([]);
          setMergeTarget(null);
        },
        onCopy: () => writeText(display),
        onViewRelations: () => setRelationsTag(tag),
        onNavigateTag: navToTag,
        onAddAlias: () => setRelationModal({ type: 'alias', source: tag }),
        onAddParent: () => setRelationModal({ type: 'implication', source: tag }),
        onAddChild: () => setRelationModal({ type: 'reverse_implication', source: tag }),
        onDelete: async () => {
          // If select-all or multi-selection includes this tag, use batch delete
          if (selectAll || (selectedTagIds.size > 1 && selectedTagIds.has(tag.tag_id))) {
            await handleDeleteSelected();
          } else {
            try {
              const tagFullName = tag.namespace ? `${tag.namespace}:${tag.subtag}` : tag.subtag;
              let affectedHashes: string[] = [];
              try {
                affectedHashes = await tagsController.findFilesByTags([tagFullName]);
              } catch { /* best effort */ }
              await tagsController.deleteTag(tag.tag_id, tagFullName, affectedHashes);
              notifySuccess(`"${display}" deleted`);
              setSelectedTagIds(new Set());
            } catch (err) {
              notifyError(err);
            }
          }
        },
      });

      ctxMenu.openAt(pos, items);
    },
    [rename, ctxMenu, fetchAllHashesForTag, deleteTagByDisplay, selectedTagIds, selectAll, tags, handleDeleteSelected],
  );

  // ── Multi-select ──────────────────────────────────────────────────────

  const handleTagClick = useCallback((e: React.MouseEvent, tag: TagRecord) => {
    setSelectAll(false);
    if (e.metaKey || e.ctrlKey) {
      setSelectedTagIds(prev => {
        const next = new Set(prev);
        if (next.has(tag.tag_id)) next.delete(tag.tag_id); else next.add(tag.tag_id);
        return next;
      });
    } else if (e.shiftKey && lastClickedTagIdRef.current != null) {
      const startIdx = tags.findIndex(t => t.tag_id === lastClickedTagIdRef.current);
      const endIdx = tags.findIndex(t => t.tag_id === tag.tag_id);
      if (startIdx !== -1 && endIdx !== -1) {
        const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
        setSelectedTagIds(prev => {
          const next = new Set(prev);
          for (let i = lo; i <= hi; i++) next.add(tags[i].tag_id);
          return next;
        });
      }
    } else {
      setSelectedTagIds(new Set([tag.tag_id]));
    }
    lastClickedTagIdRef.current = tag.tag_id;
  }, [tags]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // Don't handle keys while renaming or in modals
      if (document.activeElement?.tagName === 'INPUT') return;
      if ((e.metaKey || e.ctrlKey) && e.key === 'a') {
        e.preventDefault();
        setSelectAll(true);
        setSelectedTagIds(new Set(tags.map(t => t.tag_id)));
      }
      if ((e.key === 'Delete' || e.key === 'Backspace') && selectedTagIds.size > 0) {
        e.preventDefault();
        handleDeleteSelected();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [tags, selectedTagIds, handleDeleteSelected]);

  // Clear selection on namespace/search change
  useEffect(() => {
    setSelectedTagIds(new Set());
    setSelectAll(false);
    lastClickedTagIdRef.current = null;
  }, [selectedNs, searchQuery]);

  useEffect(() => {
    if (!mergeSource) return;
    const timer = setTimeout(async () => {
      try {
        const results = await tagsController.search(mergeSearch, 20);
        setMergeResults(results.filter((t) => t.tag_id !== mergeSource.tag_id));
      } catch (err) {
        console.error('Merge search failed:', err);
      }
    }, 100);
    return () => clearTimeout(timer);
  }, [mergeSearch, mergeSource]);

  const handleMerge = useCallback(async () => {
    if (!mergeSource || !mergeTarget) return;
    try {
      const sourceDisplay = formatTagDisplay(mergeSource.namespace, mergeSource.subtag);
      const targetDisplay = formatTagDisplay(mergeTarget.namespace, mergeTarget.subtag);
      const [sourceHashes, targetHashes] = await Promise.all([
        fetchAllHashesForTag(sourceDisplay),
        fetchAllHashesForTag(targetDisplay),
      ]);
      const targetSet = new Set(targetHashes);
      const sourceOnly = sourceHashes.filter((h) => !targetSet.has(h));
      await tagsController.mergeTag(sourceDisplay, targetDisplay, sourceHashes, sourceOnly);
      notifySuccess(
        `"${sourceDisplay}" merged into "${targetDisplay}"`,
        'Tags Merged',
      );
      setMergeSource(null);
    } catch (err) {
      notifyError(err);
    }
  }, [mergeSource, mergeTarget, fetchAllHashesForTag]);

  useEffect(() => {
    if (!relationModal) return;
    const timer = setTimeout(async () => {
      try {
        const results = await tagsController.search(relationSearch, 20);
        setRelationResults(results.filter((t) => t.tag_id !== relationModal.source.tag_id));
      } catch (err) {
        console.error('Relation search failed:', err);
      }
    }, 100);
    return () => clearTimeout(timer);
  }, [relationSearch, relationModal]);

  const handleRelationAdd = useCallback(async () => {
    if (!relationModal || !relationTarget) return;
    const sourceDisplay = formatTagDisplay(relationModal.source.namespace, relationModal.source.subtag);
    const targetDisplay = formatTagDisplay(relationTarget.namespace, relationTarget.subtag);
    try {
      if (relationModal.type === 'alias') {
        await tagsController.setAlias(sourceDisplay, targetDisplay);
        notifySuccess(`"${sourceDisplay}" now resolves to "${targetDisplay}"`, 'Alias Added');
      } else if (relationModal.type === 'implication') {
        await tagsController.setImplication(sourceDisplay, targetDisplay, 'add');
        notifySuccess(`"${sourceDisplay}" now implies "${targetDisplay}"`, 'Implication Added');
      } else {
        await tagsController.setImplication(targetDisplay, sourceDisplay, 'add');
        notifySuccess(`"${targetDisplay}" now implies "${sourceDisplay}"`, 'Reverse Implication Added');
      }
      setRelationModal(null);
    } catch (err) {
      notifyError(err);
    }
  }, [relationModal, relationTarget]);

  const renderTag = useCallback(
    (tag: TagRecord) => {
      const isRenaming = rename.renamingId === String(tag.tag_id);
      const display = formatTagDisplay(tag.namespace, tag.subtag);

      return (
        <div
          key={tag.tag_id}
          className={`${classes.tag} ${selectAll || selectedTagIds.has(tag.tag_id) ? classes.tagSelected : ''}`}
          onClick={(e) => handleTagClick(e, tag)}
          onContextMenu={(e) => handleTagContextMenu(e, tag)}
          onDoubleClick={() => rename.startRename(String(tag.tag_id), display)}
        >
          <div className={classes.tagIcon}>
            <div
              className={classes.tagDot}
              style={{ backgroundColor: nsDotColor(tag.namespace) }}
            />
          </div>
          {isRenaming ? (
            <input
              ref={rename.renameInputRef as React.RefObject<HTMLInputElement>}
              className={classes.renameInput}
              value={rename.renameValue}
              onChange={(e) => rename.setRenameValue(e.target.value)}
              onKeyDown={rename.renameKeyHandler}
              onBlur={rename.commitRename}
            />
          ) : (
            <div className={classes.tagName} title={display}>
              {display}
            </div>
          )}
          <span className={classes.tagCount}>({tag.file_count})</span>
        </div>
      );
    },
    [rename, handleTagContextMenu, handleTagClick, selectedTagIds],
  );

  const sidebarContent = useMemo(
    () => (
      <div className={classes.sidebar}>
        <div
          className={`${classes.sidebarItem} ${selectedNs === null ? classes.sidebarItemActive : ''}`}
          onClick={() => setSelectedNs(null)}
        >
          <div className={classes.sidebarItemIcon}>
            <IconBookmark size={16} />
          </div>
          <div className={classes.sidebarItemName}>All Tags</div>
          {totalTagCount > 0 && (
            <div className={classes.sidebarItemCount}>{totalTagCount}</div>
          )}
        </div>

        {namespaces.some((ns) => ns.namespace === '') && (
          <div
            className={`${classes.sidebarItem} ${selectedNs === '' ? classes.sidebarItemActive : ''}`}
            onClick={() => setSelectedNs('')}
          >
            <div className={classes.sidebarItemIcon}>
              <IconFolderQuestion size={16} />
            </div>
            <div className={classes.sidebarItemName}>Unfiled</div>
            <div className={classes.sidebarItemCount}>
              {namespaces.find((n) => n.namespace === '')?.count ?? 0}
            </div>
          </div>
        )}

        <div className={classes.sidebarLabel}>
          <span className={classes.sidebarLabelText}>Groups</span>
          {namespaces.filter((n) => n.namespace !== '').length > 0 && (
            <span className={classes.sidebarLabelCount}>
              ({namespaces.filter((n) => n.namespace !== '').length})
            </span>
          )}
        </div>

        {namespaces
          .filter((ns) => ns.namespace !== '')
          .map((ns) => (
            <div
              key={ns.namespace}
              className={`${classes.sidebarItem} ${selectedNs === ns.namespace ? classes.sidebarItemActive : ''}`}
              onClick={() => setSelectedNs(ns.namespace)}
            >
              <div className={classes.sidebarItemIcon}>
                <div
                  className={classes.tagDot}
                  style={{
                    backgroundColor: nsDotColor(ns.namespace),
                    width: 8,
                    height: 8,
                    opacity: 1,
                  }}
                />
              </div>
              <div className={classes.sidebarItemName}>{ns.namespace}</div>
              <div className={classes.sidebarItemCount}>{ns.count}</div>
            </div>
          ))}
      </div>
    ),
    [namespaces, selectedNs, totalTagCount],
  );

  const activeNsLabel = selectedNs === null ? 'All Tags' : selectedNs === '' ? 'Unfiled' : selectedNs;

  return (
    <div className={classes.root}>
      {sidebarContent}

      <div className={classes.container}>
        <div className={classes.groupHeader}>
          <div className={classes.groupName}>
            {activeNsLabel} <span className={classes.groupCount}>({activeCount})</span>
          </div>
        </div>

        <div className={classes.toolbar}>
          <div className={classes.searchWrap}>
            <IconSearch size={14} className={classes.searchIcon} />
            <input
              className={classes.searchInput}
              placeholder="Search tags…"
              defaultValue={searchQuery}
              onChange={(e) => debouncedSearch(e.target.value)}
            />
          </div>
          <div className={classes.viewToggle}>
            <button
              className={`${classes.viewBtn} ${!listMode ? classes.viewBtnActive : ''}`}
              onClick={() => setListMode(false)}
              title="Grid view"
            >
              <IconLayoutGrid size={16} />
            </button>
            <button
              className={`${classes.viewBtn} ${listMode ? classes.viewBtnActive : ''}`}
              onClick={() => setListMode(true)}
              title="List view"
            >
              <IconList size={16} />
            </button>
          </div>
        </div>

        <div className={classes.scrollArea} ref={scrollRef}>
          {loading && tags.length === 0 ? (
            <div className={classes.loadingRow}>
              <Loader size="sm" />
            </div>
          ) : tags.length === 0 ? (
            <div className={classes.emptyState}>
              {searchQuery ? 'No tags match your search.' : 'No tags in this group.'}
            </div>
          ) : (
            <div
              className={classes.virtualContainer}
              style={{ height: virtualizer.getTotalSize() }}
            >
              {virtualItems.map((virtualRow) => {
                const startIdx = virtualRow.index * columns;
                const rowTags = tags.slice(startIdx, startIdx + columns);

                return (
                  <div
                    key={virtualRow.key}
                    className={listMode ? classes.virtualRowList : classes.virtualRow}
                    style={{
                      height: ROW_HEIGHT,
                      top: virtualRow.start,
                      gridTemplateColumns: listMode ? undefined : `repeat(${columns}, 1fr)`,
                      gap: listMode ? undefined : '0 2px',
                    }}
                  >
                    {rowTags.map(renderTag)}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {ctxMenu.state && (
        <ContextMenu
          items={ctxMenu.state.items}
          position={ctxMenu.state.position}
          onClose={ctxMenu.close}
          searchable={false}
        />
      )}

      <Modal
        opened={!!mergeSource}
        onClose={() => setMergeSource(null)}
        title={`Merge "${mergeSource ? formatTagDisplay(mergeSource.namespace, mergeSource.subtag) : ''}" into…`}
        centered
        size="sm"
        styles={glassModalStyles}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <TextInput
            placeholder="Search for target tag…"
            leftSection={<IconSearch size={14} />}
            value={mergeSearch}
            onChange={(e) => setMergeSearch(e.currentTarget.value)}
            autoFocus
          />
          <div className={classes.mergeSearchResults}>
            {mergeResults.map((t) => (
              <div
                key={t.tag_id}
                className={`${classes.mergeSearchItem} ${mergeTarget?.tag_id === t.tag_id ? classes.mergeSearchItemActive : ''}`}
                onClick={() => setMergeTarget(t)}
              >
                <div
                  className={classes.mergeDot}
                  style={{ backgroundColor: nsDotColor(t.namespace) }}
                />
                <span>{formatTagDisplay(t.namespace, t.subtag)}</span>
                <span className={classes.tagCount}>({t.file_count})</span>
              </div>
            ))}
            {mergeResults.length === 0 && mergeSearch && (
              <div className={classes.emptyState} style={{ padding: '12px' }}>
                No matching tags
              </div>
            )}
          </div>
          <TextButton onClick={handleMerge} disabled={!mergeTarget}>
            <IconGitMerge size={16} />
            Merge
          </TextButton>
        </div>
      </Modal>

      <Modal
        opened={!!relationModal}
        onClose={() => { setRelationModal(null); setRelationSearch(''); setRelationResults([]); setRelationTarget(null); }}
        title={`Add ${
          relationModal ? relationActionLabel(relationModal.type) : 'relation'
        } for "${relationModal ? formatTagDisplay(relationModal.source.namespace, relationModal.source.subtag) : ''}"…`}
        centered
        size="sm"
        styles={glassModalStyles}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <TextInput
            placeholder="Search for tag…"
            leftSection={<IconSearch size={14} />}
            value={relationSearch}
            onChange={(e) => { setRelationSearch(e.currentTarget.value); setRelationTarget(null); }}
            autoFocus
          />
          <div className={classes.mergeSearchResults}>
            {relationResults.map((t) => (
              <div
                key={t.tag_id}
                className={`${classes.mergeSearchItem} ${relationTarget?.tag_id === t.tag_id ? classes.mergeSearchItemActive : ''}`}
                onClick={() => setRelationTarget(t)}
              >
                <div
                  className={classes.mergeDot}
                  style={{ backgroundColor: nsDotColor(t.namespace) }}
                />
                <span>{formatTagDisplay(t.namespace, t.subtag)}</span>
                <span className={classes.tagCount}>({t.file_count})</span>
              </div>
            ))}
            {relationResults.length === 0 && relationSearch && (
              <div className={classes.emptyState} style={{ padding: '12px' }}>
                No matching tags
              </div>
            )}
          </div>
          <TextButton onClick={handleRelationAdd} disabled={!relationTarget}>
            {relationModal?.type === 'alias' ? <IconArrowsExchange size={16} /> :
              relationModal?.type === 'implication' ? <IconArrowUp size={16} /> :
              <IconArrowDown size={16} />}
            {relationModal?.type === 'alias' ? 'Add Alias' :
             relationModal?.type === 'implication' ? 'Add Implication' : 'Add Implied-By'}
          </TextButton>
        </div>
      </Modal>

      <TagRelationsModal
        opened={!!relationsTag}
        onClose={() => setRelationsTag(null)}
        tag={relationsTag}
      />
    </div>
  );
}
