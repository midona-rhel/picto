import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { atom, useAtom, useAtomValue } from 'jotai';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  IconEdit,
  IconGitMerge,
  IconBookmark,
  IconBookmarks,
  IconPlus,
  IconSearch,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { listen } from '../../platform/ipc';
import { tagsController } from '../../controllers/tagsController';
import type {
  CanonicalNamespaceSummary,
  CanonicalTagRecord,
  CanonicalTagRelation,
} from '../../shared/types/canonical';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import { GlassInput } from '../../shared/ui/GlassInput/GlassInput';
import { GlassModal } from '../../shared/ui/GlassModal';
import { ConfirmModal } from '../modals/ConfirmModal';
import { EmptyState } from '../subscriptions/components/EmptyState';
import { ActionButton } from '../subscriptions/components/ActionButton';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { tagGroupColor, tagGroupOrder, tagGroupPresentation } from './tagGroupPresentation';
import styles from './TagManagerScreen.module.css';

const PAGE_SIZE = 100;
const PICKER_PAGE_SIZE = 40;
const tagManagerQueryAtom = atom('');

type RelationMode = 'alias' | 'parent' | 'child';
type EditorAction =
  | { kind: 'merge' }
  | { kind: 'relation'; mode: RelationMode }
  | { kind: 'delete' }
  | null;

function tagKey(tag: Pick<CanonicalTagRecord, 'namespace' | 'subtag'>): string {
  return tag.namespace ? `${tag.namespace}:${tag.subtag}` : tag.subtag;
}

function relationKey(relation: CanonicalTagRelation): string {
  return relation.namespace ? `${relation.namespace}:${relation.subtag}` : relation.subtag;
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

interface RelationPickerProps {
  title: string;
  excludeTagId: number;
  onChoose: (tag: CanonicalTagRecord) => void;
  onClose: () => void;
}

function RelationPicker({ title, excludeTagId, onChoose, onClose }: RelationPickerProps) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<CanonicalTagRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);

  useEffect(() => {
    const trimmed = query.trim();
    const generation = ++requestGeneration.current;
    if (!trimmed) {
      setResults([]);
      setError(null);
      setLoading(false);
      return undefined;
    }

    setLoading(true);
    setError(null);
    void tagsController.getPaginated({
      namespace: null,
      search: trimmed,
      cursor: null,
      limit: PICKER_PAGE_SIZE,
    }).then((page) => {
      if (generation !== requestGeneration.current) return;
      setResults(page.items.filter((item) => item.tag_id !== excludeTagId));
    }).catch((reason: unknown) => {
      if (generation === requestGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (generation === requestGeneration.current) setLoading(false);
    });
    return undefined;
  }, [excludeTagId, query]);

  return (
    <GlassModal open onClose={onClose} title={title} size="md">
      <div className={styles.pickerBody}>
        <GlassInput
          search
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search existing tags"
          aria-label="Search existing tags"
        />
        {loading && <div className={styles.pickerHint}>Searching...</div>}
        {error && <div className={styles.error} role="alert">{error}</div>}
        {!loading && !error && query.trim() && results.length === 0 && (
          <div className={styles.pickerHint}>No existing tags found.</div>
        )}
        <div className={styles.pickerResults}>
          {results.map((tag) => (
            <button
              key={tag.tag_id}
              className={styles.pickerResult}
              onClick={() => onChoose(tag)}
              type="button"
            >
              <TagChip namespace={tag.namespace} subtag={tag.subtag} />
              <span className={styles.pickerCount}>{tag.file_count}</span>
            </button>
          ))}
        </div>
      </div>
    </GlassModal>
  );
}

function RelationList({
  relations,
  emptyLabel,
  onRemove,
}: {
  relations: CanonicalTagRelation[];
  emptyLabel: string;
  onRemove: (relation: CanonicalTagRelation) => void;
}) {
  if (relations.length === 0) return <div className={styles.relationEmpty}>{emptyLabel}</div>;
  return (
    <div className={styles.relationList}>
      {relations.map((relation) => (
        <div className={styles.relationRow} key={`${relation.relation}:${relation.tag_id}`}>
          <TagChip namespace={relation.namespace} subtag={relation.subtag} />
          <span className={styles.relationKind}>{relation.relation}</span>
          <button
            className={styles.iconButton}
            title="Remove relation"
            aria-label={`Remove ${relationKey(relation)}`}
            onClick={() => onRemove(relation)}
            type="button"
          >
            <IconX size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

function TagEditorModal({
  tag,
  aliases,
  implications,
  detailsLoading,
  busy,
  onClose,
  onRename,
  onAction,
  onRemoveRelation,
}: {
  tag: CanonicalTagRecord;
  aliases: CanonicalTagRelation[];
  implications: CanonicalTagRelation[];
  detailsLoading: boolean;
  busy: boolean;
  onClose: () => void;
  onRename: (name: string) => void;
  onAction: (action: Exclude<EditorAction, null>) => void;
  onRemoveRelation: (relation: CanonicalTagRelation) => void;
}) {
  const [name, setName] = useState(tagKey(tag));
  const parents = implications.filter((relation) => relation.relation === 'parent');
  const children = implications.filter((relation) => relation.relation === 'child');

  const submitRename = () => {
    const value = name.trim();
    if (value && value !== tagKey(tag)) onRename(value);
  };

  return (
    <GlassModal open onClose={onClose} title="Edit tag" size="lg">
      <div className={styles.editor}>
        <div className={styles.editorIntro}>
          <div className={styles.editorIcon}><IconBookmark size={18} /></div>
          <div>
            <TagChip namespace={tag.namespace} subtag={tag.subtag} />
            <div className={styles.editorCount}>{tag.file_count} media items</div>
          </div>
        </div>

        <section className={styles.editorSection} aria-labelledby="tag-name-heading">
          <div className={styles.sectionHeader}>
            <h2 id="tag-name-heading">Tag name</h2>
            <ActionButton compact onClick={submitRename} disabled={busy || !name.trim()}>
              <IconEdit size={14} /> Rename
            </ActionButton>
          </div>
          <GlassInput
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') submitRename(); }}
            placeholder="namespace:tag"
            aria-label="New tag name"
            disabled={busy}
          />
        </section>

        <div className={styles.editorActions}>
          <ActionButton compact onClick={() => onAction({ kind: 'merge' })} disabled={busy}>
            <IconGitMerge size={14} /> Merge into...
          </ActionButton>
          <ActionButton compact variant="danger" onClick={() => onAction({ kind: 'delete' })} disabled={busy}>
            <IconTrash size={14} /> Delete
          </ActionButton>
        </div>

        {detailsLoading && <div className={styles.muted}>Loading relations...</div>}
        <RelationEditorSection
          title="Aliases"
          addLabel="Add alias"
          emptyLabel="No aliases"
          relations={aliases}
          onAdd={() => onAction({ kind: 'relation', mode: 'alias' })}
          onRemove={onRemoveRelation}
        />
        <RelationEditorSection
          title="Parents"
          addLabel="Add parent"
          emptyLabel="No parent implications"
          relations={parents}
          onAdd={() => onAction({ kind: 'relation', mode: 'parent' })}
          onRemove={onRemoveRelation}
        />
        <RelationEditorSection
          title="Children"
          addLabel="Add child"
          emptyLabel="No child implications"
          relations={children}
          onAdd={() => onAction({ kind: 'relation', mode: 'child' })}
          onRemove={onRemoveRelation}
        />
      </div>
    </GlassModal>
  );
}

function RelationEditorSection({
  title,
  addLabel,
  emptyLabel,
  relations,
  onAdd,
  onRemove,
}: {
  title: string;
  addLabel: string;
  emptyLabel: string;
  relations: CanonicalTagRelation[];
  onAdd: () => void;
  onRemove: (relation: CanonicalTagRelation) => void;
}) {
  return (
    <section className={styles.editorSection} aria-labelledby={`${title.toLowerCase()}-heading`}>
      <div className={styles.sectionHeader}>
        <h2 id={`${title.toLowerCase()}-heading`}>{title}</h2>
        <ActionButton compact onClick={onAdd}>
          <IconPlus size={14} /> {addLabel}
        </ActionButton>
      </div>
      <RelationList relations={relations} emptyLabel={emptyLabel} onRemove={onRemove} />
    </section>
  );
}

export function TagManagerScreen() {
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const query = useAtomValue(tagManagerQueryAtom);
  const [namespace, setNamespace] = useState<string | null>(null);
  const [selected, setSelected] = useState<CanonicalTagRecord | null>(null);
  const [aliases, setAliases] = useState<CanonicalTagRelation[]>([]);
  const [implications, setImplications] = useState<CanonicalTagRelation[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [detailsLoading, setDetailsLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editorAction, setEditorAction] = useState<EditorAction>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const listGeneration = useRef(0);
  const summaryGeneration = useRef(0);
  const relationGeneration = useRef(0);
  const selectedRef = useRef<CanonicalTagRecord | null>(null);
  const tagCanvasRef = useRef<HTMLDivElement>(null);
  const [columnCount, setColumnCount] = useState(3);
  const contextMenu = useContextMenu();
  selectedRef.current = selected;

  const reloadNamespaceSummary = useCallback(async () => {
    const generation = ++summaryGeneration.current;
    try {
      const items = await tagsController.getNamespaceSummary();
      if (generation === summaryGeneration.current) setNamespaces(items);
    } catch (reason: unknown) {
      if (generation === summaryGeneration.current) setError(errorMessage(reason));
    }
  }, []);

  const loadRelations = useCallback(async (tag: CanonicalTagRecord | null) => {
    const generation = ++relationGeneration.current;
    if (!tag) {
      setAliases([]);
      setImplications([]);
      setDetailsLoading(false);
      return;
    }
    setDetailsLoading(true);
    try {
      const [aliasResult, implicationResult] = await Promise.all([
        tagsController.getRelations(tag.tag_id, 'aliases'),
        tagsController.getRelations(tag.tag_id, 'implications'),
      ]);
      if (generation !== relationGeneration.current) return;
      setAliases(aliasResult);
      setImplications(implicationResult);
    } catch (reason: unknown) {
      if (generation === relationGeneration.current) setError(errorMessage(reason));
    } finally {
      if (generation === relationGeneration.current) setDetailsLoading(false);
    }
  }, []);

  useEffect(() => {
    void reloadNamespaceSummary();
  }, [reloadNamespaceSummary]);

  useEffect(() => {
    const generation = ++listGeneration.current;
    let cancelled = false;
    setLoading(true);
    void tagsController.getPaginated({
      namespace,
      search: query.trim() || null,
      cursor: null,
      limit: PAGE_SIZE,
    }).then((page) => {
      if (cancelled || generation !== listGeneration.current) return;
      setTags(page.items);
      setCursor(page.next_cursor);
    }).catch((reason: unknown) => {
      if (!cancelled && generation === listGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (!cancelled && generation === listGeneration.current) setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [namespace, query, reloadToken]);

  useEffect(() => {
    setSelected(null);
    setEditorAction(null);
  }, [query]);

  useEffect(() => {
    void loadRelations(selected);
  }, [loadRelations, selected]);

  const refreshData = useCallback(async () => {
    setReloadToken((value) => value + 1);
    await reloadNamespaceSummary();
  }, [reloadNamespaceSummary]);

  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen<{ changes?: Record<string, unknown> }>('runtime/state_changed', ({ payload }) => {
      if (cancelled) return;
      const changes = payload?.changes ?? {};
      if (changes.tags_changed || changes.tag_structure_changed) {
        void refreshData();
        if (selectedRef.current) void loadRelations(selectedRef.current);
      }
    }).catch((reason: unknown) => {
      if (!cancelled) setError(errorMessage(reason));
      return () => {};
    });
    return () => {
      cancelled = true;
      unlistenPromise.then((unlisten) => unlisten()).catch((reason: unknown) => {
        if (!cancelled) setError(errorMessage(reason));
      });
    };
  }, [loadRelations, refreshData]);

  const resetBrowse = useCallback(() => {
    listGeneration.current += 1;
    setTags([]);
    setCursor(null);
    setLoading(true);
    setError(null);
    setSelected(null);
    setEditorAction(null);
  }, []);

  const handleNamespaceChange = useCallback((value: string | null) => {
    resetBrowse();
    setNamespace(value);
  }, [resetBrowse]);

  const loadMore = useCallback(() => {
    if (!cursor || loadingMore) return;
    const generation = listGeneration.current;
    const requestedCursor = cursor;
    setLoadingMore(true);
    void tagsController.getPaginated({
      namespace,
      search: query.trim() || null,
      cursor: requestedCursor,
      limit: PAGE_SIZE,
    }).then((page) => {
      if (generation !== listGeneration.current) return;
      setTags((current) => [...current, ...page.items]);
      setCursor(page.next_cursor);
    }).catch((reason: unknown) => {
      if (generation === listGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (generation === listGeneration.current) setLoadingMore(false);
    });
  }, [cursor, loadingMore, namespace, query]);

  useEffect(() => {
    const element = tagCanvasRef.current;
    if (!element || typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? element.clientWidth;
      setColumnCount(Math.max(1, Math.floor((width - 30) / 201)));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const expectedTagCount = useMemo(() => {
    if (query.trim()) return tags.length;
    if (namespace === null) return namespaces.reduce((sum, item) => sum + item.count, 0);
    return namespaces.find((item) => item.namespace === namespace)?.count ?? tags.length;
  }, [namespace, namespaces, query, tags.length]);

  const virtualItemCount = cursor || loading
    ? Math.max(tags.length, expectedTagCount)
    : tags.length;
  const virtualRowCount = Math.ceil(virtualItemCount / columnCount);
  const tagVirtualizer = useVirtualizer({
    count: virtualRowCount,
    getScrollElement: () => tagCanvasRef.current,
    estimateSize: () => 27,
    overscan: 12,
    initialRect: { width: 720, height: 540 },
    observeElementRect: (instance, callback) => {
      const element = instance.scrollElement;
      if (!element) return undefined;
      const publish = () => {
        const rect = element.getBoundingClientRect();
        callback({ width: rect.width || 720, height: rect.height || 540 });
      };
      publish();
      const Observer = instance.targetWindow?.ResizeObserver;
      if (!Observer) return undefined;
      const observer = new Observer(publish);
      observer.observe(element);
      return () => observer.disconnect();
    },
  });
  const virtualRows = tagVirtualizer.getVirtualItems();
  const lastVirtualRowIndex = virtualRows.length > 0
    ? virtualRows[virtualRows.length - 1].index
    : -1;

  useEffect(() => {
    if (!cursor || loadingMore || lastVirtualRowIndex < 0) return;
    const lastVisibleItem = (lastVirtualRowIndex + 1) * columnCount - 1;
    if (lastVisibleItem >= tags.length - columnCount * 3) loadMore();
  }, [columnCount, cursor, lastVirtualRowIndex, loadMore, loadingMore, tags.length]);

  const closeEditor = useCallback(() => {
    setEditorAction(null);
    setSelected(null);
  }, []);

  const runMutation = useCallback(async (
    operation: () => Promise<unknown>,
    closeAfterSuccess = false,
  ) => {
    setBusy(true);
    setError(null);
    try {
      await operation();
      await refreshData();
      const currentTag = selectedRef.current;
      if (closeAfterSuccess) {
        closeEditor();
      } else if (currentTag) {
        await loadRelations(currentTag);
        setEditorAction(null);
      }
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }, [closeEditor, loadRelations, refreshData]);

  const handleRelation = useCallback((mode: RelationMode, target: CanonicalTagRecord) => {
    const currentTag = selectedRef.current;
    if (!currentTag) return;
    const currentKey = tagKey(currentTag);
    const targetKey = tagKey(target);
    if (mode === 'alias') {
      void runMutation(() => tagsController.setAlias(targetKey, currentKey));
    } else if (mode === 'parent') {
      void runMutation(() => tagsController.setImplication(currentKey, targetKey, 'add'));
    } else {
      void runMutation(() => tagsController.setImplication(targetKey, currentKey, 'add'));
    }
  }, [runMutation]);

  const removeRelation = useCallback((relation: CanonicalTagRelation) => {
    const currentTag = selectedRef.current;
    if (!currentTag) return;
    const currentKey = tagKey(currentTag);
    const targetKey = relationKey(relation);
    if (relation.relation === 'to' || relation.relation === 'from') {
      void runMutation(() => tagsController.setAlias(
        relation.relation === 'to' ? currentKey : targetKey,
        null,
      ));
    } else {
      void runMutation(() => tagsController.setImplication(
        relation.relation === 'parent' ? currentKey : targetKey,
        relation.relation === 'parent' ? targetKey : currentKey,
        'remove',
      ));
    }
  }, [runMutation]);

  const namespaceLabel = (value: string) => value || 'general';
  const sortedNamespaces = useMemo(
    () => [...namespaces].sort((left, right) => tagGroupOrder(left.namespace) - tagGroupOrder(right.namespace)),
    [namespaces],
  );
  const openTagContextMenu = useCallback((event: React.MouseEvent, tag: CanonicalTagRecord) => {
    const entries: MenuEntry[] = [
      {
        label: 'Edit Tag',
        icon: <IconEdit size={16} />,
        action: () => { setSelected(tag); setEditorAction(null); },
      },
      {
        label: 'Merge Into…',
        icon: <IconGitMerge size={16} />,
        action: () => { setSelected(tag); setEditorAction({ kind: 'merge' }); },
      },
      { separator: true },
      {
        label: 'Delete Tag',
        icon: <IconTrash size={16} />,
        danger: true,
        action: () => { setSelected(tag); setEditorAction({ kind: 'delete' }); },
      },
    ];
    contextMenu.open(event, entries);
  }, [contextMenu]);

  return (
    <div className={styles.root}>
      {error && <div className={styles.error} role="alert">{error}</div>}

      <div className={styles.content}>
        <nav className={styles.namespaceRail} aria-label="Tag groups">
          <div className={styles.railHeading}>Groups ({namespaces.length})</div>
          <button
            className={`${styles.namespaceItem} ${namespace === null ? styles.namespaceItemActive : ''}`}
            onClick={() => handleNamespaceChange(null)}
            type="button"
          >
            <span className={styles.groupIdentity}><IconBookmarks size={16} /><span>All tags</span></span>
            <span className={styles.namespaceCount}>{namespaces.reduce((sum, item) => sum + item.count, 0)}</span>
          </button>
          {sortedNamespaces.map((item) => {
            const GroupIcon = tagGroupPresentation(item.namespace).icon;
            return (
              <button
                className={`${styles.namespaceItem} ${namespace === item.namespace ? styles.namespaceItemActive : ''}`}
                key={item.namespace || '__general__'}
                onClick={() => handleNamespaceChange(item.namespace)}
                type="button"
              >
                <span className={styles.groupIdentity} style={{ color: tagGroupColor(item.namespace) }}>
                  <GroupIcon size={16} />
                  <span className={styles.groupName}>{namespaceLabel(item.namespace)}</span>
                </span>
                <span className={styles.namespaceCount}>{item.count}</span>
              </button>
            );
          })}
        </nav>

        <section className={styles.browseSurface} aria-label="Tags">
          <div className={styles.canvasHeader}>
            <div>
              <span className={styles.canvasTitle}>
                {namespace === null ? 'All tags' : namespaceLabel(namespace)}
              </span>
              <span className={styles.canvasCount}>
                ({query.trim() ? `${tags.length}${cursor ? '+' : ''}` : expectedTagCount})
              </span>
              {query && <span className={styles.canvasQuery}> matching "{query}"</span>}
            </div>
          </div>
          <div className={styles.tagCanvas} ref={tagCanvasRef}>
            {!loading && tags.length === 0 && (
              <EmptyState title="No tags found" description="Try a different search or group." />
            )}
            <div className={styles.tagListInner} style={{ height: tagVirtualizer.getTotalSize() }}>
              {virtualRows.map((virtualRow) => (
                <div
                  className={styles.tagVirtualRow}
                  key={virtualRow.key}
                  style={{
                    gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  {Array.from({ length: columnCount }, (_, columnIndex) => {
                    const itemIndex = virtualRow.index * columnCount + columnIndex;
                    if (itemIndex >= virtualItemCount) return null;
                    const tag = tags[itemIndex];
                    if (!tag) return <div className={styles.tagSkeleton} key={`skeleton-${itemIndex}`} />;
                    return (
                      <button
                        className={styles.tagCard}
                        key={tag.tag_id}
                        onClick={() => { setSelected(tag); setEditorAction(null); }}
                        onContextMenu={(event) => openTagContextMenu(event, tag)}
                        type="button"
                      >
                        <span className={styles.tagDot} style={{ background: tagGroupColor(tag.namespace) }} />
                        <span className={styles.tagName}>{tagKey(tag)}</span>
                        <span className={styles.tagCardCount}>({tag.file_count})</span>
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
        </section>
      </div>

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}

      {selected && (
        <TagEditorModal
          key={selected.tag_id}
          tag={selected}
          aliases={aliases}
          implications={implications}
          detailsLoading={detailsLoading}
          busy={busy}
          onClose={closeEditor}
          onRename={(name) => void runMutation(() => tagsController.rename(selected.tag_id, name), true)}
          onAction={setEditorAction}
          onRemoveRelation={removeRelation}
        />
      )}
      {selected && editorAction?.kind === 'merge' && (
        <RelationPicker
          title="Merge into existing tag"
          excludeTagId={selected.tag_id}
          onClose={() => setEditorAction(null)}
          onChoose={(target) => {
            setEditorAction(null);
            void runMutation(() => tagsController.merge(tagKey(selected), tagKey(target)), true);
          }}
        />
      )}
      {selected && editorAction?.kind === 'relation' && (
        <RelationPicker
          title={editorAction.mode === 'alias' ? 'Add existing alias' : editorAction.mode === 'parent' ? 'Add existing parent' : 'Add existing child'}
          excludeTagId={selected.tag_id}
          onClose={() => setEditorAction(null)}
          onChoose={(target) => {
            setEditorAction(null);
            handleRelation(editorAction.mode, target);
          }}
        />
      )}
      {selected && editorAction?.kind === 'delete' && (
        <ConfirmModal
          open
          onClose={() => setEditorAction(null)}
          onConfirm={() => void runMutation(() => tagsController.delete(selected.tag_id), true)}
          title="Delete tag"
          message={`Delete ${tagKey(selected)}? Existing media will remain untouched.`}
          confirmLabel="Delete tag"
          danger
          loading={busy}
        />
      )}
    </div>
  );
}

export function TagsToolbar() {
  const [query, setQuery] = useAtom(tagManagerQueryAtom);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const focusSearch = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'f') {
        event.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', focusSearch);
    return () => window.removeEventListener('keydown', focusSearch);
  }, []);

  return (
    <div className={styles.titlebarToolbar}>
      <div className={styles.titlebarSearch}>
        <IconSearch size={13} className={styles.titlebarSearchIcon} />
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search tags"
          aria-label="Search tags"
        />
        {query && (
          <button onClick={() => setQuery('')} aria-label="Clear tag search" type="button">
            <IconX size={12} />
          </button>
        )}
      </div>
    </div>
  );
}
