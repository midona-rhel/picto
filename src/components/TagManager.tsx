import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useDebouncedCallback } from '../hooks/useDebouncedCallback';
import { Loader, Modal, SegmentedControl, TextInput } from '@mantine/core';
import { TextButton } from './ui/TextButton';
import { glassModalStyles } from '../styles/glassModal';
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
import { api } from '#desktop/api';
import { writeText } from '#desktop/api';
import { notifySuccess, notifyError, notifyWarning } from '../lib/notify';
import { getNamespaceColor } from '../lib/namespaceColors';
import { parseTagString } from '../lib/tagParsing';
import { useInlineRename } from '../hooks/useInlineRename';
import { useNavigationStore } from '../stores/navigationStore';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from './ui/ContextMenu';
import { TagRelationsModal } from './TagRelationsModal';
import { registerUndoAction } from '../controllers/undoRedoController';
import { buildTagContextMenu } from './ui/context-actions/tagActions';
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

import type { TagRelation, TagSearchResult } from '../types/api';

function formatTagDisplay(ns: string, subtag: string): string {
  return ns ? `${ns}:${subtag}` : subtag;
}

function normalizeTagRecord(tag: TagRecord): TagRecord {
  const parsed = parseTagString(formatTagDisplay(tag.namespace, tag.subtag));
  return {
    ...tag,
    namespace: parsed.namespace,
    subtag: parsed.subtag,
  };
}

function normalizeNamespaceSummaries(input: NamespaceSummary[]): NamespaceSummary[] {
  const counts = new Map<string, number>();
  for (const entry of input) {
    const parsed = parseTagString(
      entry.namespace ? `${entry.namespace}:x` : 'x',
    );
    const ns = parsed.namespace;
    counts.set(ns, (counts.get(ns) ?? 0) + entry.count);
  }
  return [...counts.entries()]
    .map(([namespace, count]) => ({ namespace, count }))
    .sort((a, b) => b.count - a.count || a.namespace.localeCompare(b.namespace));
}

function nsDotColor(ns: string): string {
  const [r, g, b] = getNamespaceColor(ns, true);
  return `rgb(${r}, ${g}, ${b})`;
}

const ROW_HEIGHT = 27;

type TagSource = 'local' | 'ptr';

export function TagManager() {
  const [source, setSource] = useState<TagSource>('local');

  const [namespaces, setNamespaces] = useState<NamespaceSummary[]>([]);
  const [selectedNs, setSelectedNs] = useState<string | null>(null);
  const [totalTagCount, setTotalTagCount] = useState(0);

  const [tags, setTags] = useState<TagRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [listMode, setListMode] = useState(false);
  const [hasMore, setHasMore] = useState(true);

  const [containerWidth, setContainerWidth] = useState(800);

  const [mergeSource, setMergeSource] = useState<TagRecord | null>(null);
  const [mergeSearch, setMergeSearch] = useState('');
  const [mergeResults, setMergeResults] = useState<TagSearchResult[]>([]);
  const [mergeTarget, setMergeTarget] = useState<TagRecord | null>(null);

  const [relationModal, setRelationModal] = useState<{ type: 'sibling' | 'parent' | 'child'; source: TagRecord } | null>(null);
  const [relationSearch, setRelationSearch] = useState('');
  const [relationResults, setRelationResults] = useState<TagSearchResult[]>([]);
  const [relationTarget, setRelationTarget] = useState<TagRecord | null>(null);

  const [relationsTag, setRelationsTag] = useState<TagRecord | null>(null);

  const ctxMenu = useContextMenu();

  const scrollRef = useRef<HTMLDivElement>(null);
  const loadingMoreRef = useRef(false);
  const normalizeRanRef = useRef(false);

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
      const oldDisplay = oldTag ? formatTagDisplay(oldTag.namespace, oldTag.subtag) : null;
      const result = await api.tags.rename(tagId, newName);
      if (oldDisplay && oldDisplay !== newName && !result.merged_into) {
        registerUndoAction({
          label: 'Rename tag',
          undo: async () => {
            await api.tags.rename(tagId, oldDisplay);
            await refreshAll();
          },
          redo: async () => {
            await api.tags.rename(tagId, newName);
            await refreshAll();
          },
        });
      } else if (result.merged_into) {
        notifyWarning('Rename merged into an existing tag; undo is not available for this rename.', 'Rename Merged');
      }
      notifySuccess(`Renamed to "${newName}"`, 'Tag Renamed');
      await refreshAll();
    } catch (err) {
      notifyError(err);
    }
  });

  const fetchNamespaces = useCallback(async () => {
    try {
      const result = source === 'ptr'
        ? await api.ptr.getNamespaceSummary()
        : await api.tags.getNamespaceSummary();
      const normalized = normalizeNamespaceSummaries(result);
      setNamespaces(normalized);
      setTotalTagCount(normalized.reduce((sum, ns) => sum + ns.count, 0));
    } catch (err) {
      console.error('Failed to load namespace summary:', err);
    }
  }, [source]);

  const fetchTags = useCallback(
    async (cursor?: string) => {
      try {
        const params = {
          namespace: selectedNs ?? undefined,
          search: searchQuery || undefined,
          cursor: cursor ?? undefined,
          limit: 500,
        };
        const resultRaw = source === 'ptr'
          ? await api.ptr.getTagsPaginated(params)
          : await api.tags.getPaginated(params);
        const result = resultRaw.map(normalizeTagRecord);
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
    [selectedNs, searchQuery, source],
  );

  const refreshAll = useCallback(async () => {
    setLoading(true);
    await Promise.all([fetchNamespaces(), fetchTags()]);
    setLoading(false);
  }, [fetchNamespaces, fetchTags]);

  const fetchAllHashesForTag = useCallback(async (tagDisplay: string): Promise<string[]> => {
    const limit = 5000;
    let offset = 0;
    const all: string[] = [];
    while (true) {
      const batch = await api.tags.findFilesByTags([tagDisplay], limit, offset);
      if (!batch || batch.length === 0) break;
      all.push(...batch);
      if (batch.length < limit) break;
      offset += batch.length;
    }
    return [...new Set(all)];
  }, []);

  const deleteTagByDisplay = useCallback(async (tagDisplay: string): Promise<void> => {
    const candidates = await api.tags.search(tagDisplay, 50);
    const exact = candidates.find((c) => {
      const formatted = formatTagDisplay(c.namespace, c.subtag);
      return formatted === tagDisplay || c.display === tagDisplay;
    });
    if (!exact) return;
    await api.tags.delete(exact.tag_id);
  }, []);

  useEffect(() => {
    setSelectedNs(null);
    setTags([]);
    setHasMore(true);
    setSearchQuery('');
  }, [source]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      if (source === 'local' && !normalizeRanRef.current) {
        normalizeRanRef.current = true;
        try {
          await api.tags.normalizeNamespaces();
        } catch (err) {
          console.warn('Namespace normalization command failed:', err);
        }
      }
      await fetchNamespaces();
      await fetchTags();
      if (!cancelled) setLoading(false);
    })();
    return () => { cancelled = true; };
  }, [fetchNamespaces, fetchTags]);

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

  const handleTagContextMenu = useCallback(
    async (e: React.MouseEvent, tag: TagRecord) => {
      e.preventDefault();
      e.stopPropagation();
      const pos = { x: e.clientX, y: e.clientY };
      const display = formatTagDisplay(tag.namespace, tag.subtag);
      const isPtr = source === 'ptr';

      const [siblings, relations] = await Promise.all([
        (isPtr ? api.ptr.getTagSiblings(tag.tag_id) : api.tags.getSiblings(tag.tag_id)).catch(() => [] as TagRelation[]),
        (isPtr ? api.ptr.getTagParents(tag.tag_id) : api.tags.getParents(tag.tag_id)).catch(() => [] as TagRelation[]),
      ]);

      const parentTags = relations.filter((r) => r.relation === 'parent');
      const childTags = relations.filter((r) => r.relation === 'child');

      const navToTag = (ns: string, st: string) =>
        useNavigationStore.getState().navigateToFilterTags([formatTagDisplay(ns, st)]);
      const items: ContextMenuEntry[] = buildTagContextMenu({
        tag,
        source,
        siblings,
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
        onAddSibling: () => setRelationModal({ type: 'sibling', source: tag }),
        onAddParent: () => setRelationModal({ type: 'parent', source: tag }),
        onAddChild: () => setRelationModal({ type: 'child', source: tag }),
        onDelete: async () => {
          try {
            const snapshotHashes = await fetchAllHashesForTag(display);
            await api.tags.delete(tag.tag_id);
            registerUndoAction({
              label: `Delete tag "${display}"`,
              undo: async () => {
                if (snapshotHashes.length > 0) {
                  await api.tags.addBatch(snapshotHashes, [display]);
                }
                await refreshAll();
              },
              redo: async () => {
                await deleteTagByDisplay(display);
                await refreshAll();
              },
            });
            notifySuccess(`"${display}" deleted`, 'Tag Deleted');
            await refreshAll();
          } catch (err) {
            notifyError(err);
          }
        },
      });

      ctxMenu.openAt(pos, items);
    },
    [rename, refreshAll, ctxMenu, source, fetchAllHashesForTag, deleteTagByDisplay],
  );

  useEffect(() => {
    if (!mergeSource) return;
    const timer = setTimeout(async () => {
      try {
        const results = await api.tags.search(mergeSearch, 20);
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
      await api.tags.merge(
        sourceDisplay,
        targetDisplay,
      );
      const targetSet = new Set(targetHashes);
      const sourceOnly = sourceHashes.filter((h) => !targetSet.has(h));
      registerUndoAction({
        label: `Merge tag "${sourceDisplay}" into "${targetDisplay}"`,
        undo: async () => {
          if (sourceHashes.length > 0) await api.tags.addBatch(sourceHashes, [sourceDisplay]);
          if (sourceOnly.length > 0) await api.tags.removeBatch(sourceOnly, [targetDisplay]);
          await refreshAll();
        },
        redo: async () => {
          await api.tags.merge(sourceDisplay, targetDisplay);
          await refreshAll();
        },
      });
      notifySuccess(
        `"${sourceDisplay}" merged into "${targetDisplay}"`,
        'Tags Merged',
      );
      setMergeSource(null);
      await refreshAll();
    } catch (err) {
      notifyError(err);
    }
  }, [mergeSource, mergeTarget, refreshAll, fetchAllHashesForTag]);

  useEffect(() => {
    if (!relationModal) return;
    const timer = setTimeout(async () => {
      try {
        const results = await api.tags.search(relationSearch, 20);
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
      if (relationModal.type === 'sibling') {
        await api.tags.setAlias(sourceDisplay, targetDisplay);
        registerUndoAction({
          label: `Set sibling "${sourceDisplay}"`,
          undo: async () => {
            await api.tags.removeAlias(sourceDisplay);
            await refreshAll();
          },
          redo: async () => {
            await api.tags.setAlias(sourceDisplay, targetDisplay);
            await refreshAll();
          },
        });
        notifySuccess(`"${sourceDisplay}" → "${targetDisplay}"`, 'Sibling Added');
      } else if (relationModal.type === 'parent') {
        await api.tags.addParent(sourceDisplay, targetDisplay);
        registerUndoAction({
          label: `Add parent "${targetDisplay}"`,
          undo: async () => {
            await api.tags.removeParent(sourceDisplay, targetDisplay);
            await refreshAll();
          },
          redo: async () => {
            await api.tags.addParent(sourceDisplay, targetDisplay);
            await refreshAll();
          },
        });
        notifySuccess(`"${targetDisplay}" is now parent of "${sourceDisplay}"`, 'Parent Added');
      } else {
        await api.tags.addParent(targetDisplay, sourceDisplay);
        registerUndoAction({
          label: `Add child "${targetDisplay}"`,
          undo: async () => {
            await api.tags.removeParent(targetDisplay, sourceDisplay);
            await refreshAll();
          },
          redo: async () => {
            await api.tags.addParent(targetDisplay, sourceDisplay);
            await refreshAll();
          },
        });
        notifySuccess(`"${sourceDisplay}" is now parent of "${targetDisplay}"`, 'Child Added');
      }
      setRelationModal(null);
      await refreshAll();
    } catch (err) {
      notifyError(err);
    }
  }, [relationModal, relationTarget, refreshAll]);

  const renderTag = useCallback(
    (tag: TagRecord) => {
      const isRenaming = rename.renamingId === String(tag.tag_id);
      const display = formatTagDisplay(tag.namespace, tag.subtag);

      return (
        <div
          key={tag.tag_id}
          className={classes.tag}
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
    [rename, handleTagContextMenu],
  );

  const sidebarContent = useMemo(
    () => (
      <div className={classes.sidebar}>
        <div className={classes.sourceToggle}>
          <SegmentedControl
            size="xs"
            fullWidth
            value={source}
            onChange={(v) => setSource(v as TagSource)}
            data={[
              { label: 'Local', value: 'local' },
              { label: 'PTR', value: 'ptr' },
            ]}
          />
        </div>

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
    [namespaces, selectedNs, totalTagCount, source],
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
        title={`Add ${relationModal?.type ?? ''} for "${relationModal ? formatTagDisplay(relationModal.source.namespace, relationModal.source.subtag) : ''}"…`}
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
            {relationModal?.type === 'sibling' ? <IconArrowsExchange size={16} /> :
              relationModal?.type === 'parent' ? <IconArrowUp size={16} /> :
              <IconArrowDown size={16} />}
            {relationModal?.type === 'sibling' ? 'Add Sibling' :
             relationModal?.type === 'parent' ? 'Add Parent' : 'Add Child'}
          </TextButton>
        </div>
      </Modal>

      <TagRelationsModal
        opened={!!relationsTag}
        onClose={() => setRelationsTag(null)}
        tag={relationsTag}
        source={source}
      />
    </div>
  );
}
