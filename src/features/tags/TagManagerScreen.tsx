import { useCallback, useEffect, useRef, useState } from 'react';
import {
  IconEdit,
  IconGitMerge,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconTag,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
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
import styles from './TagManagerScreen.module.css';

const PAGE_SIZE = 100;
const PICKER_PAGE_SIZE = 40;
const MAX_RENDERED_TAGS = PAGE_SIZE * 3;

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
          <div className={styles.editorIcon}><IconTag size={18} /></div>
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
  const [query, setQuery] = useState('');
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
      const relations = await tagsController.getRelations(tag.tag_id);
      if (generation !== relationGeneration.current) return;
      setAliases(relations.aliases);
      setImplications(relations.implications);
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
      setTags(page.items.slice(-MAX_RENDERED_TAGS));
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
    void loadRelations(selected);
  }, [loadRelations, selected]);

  const refreshData = useCallback(async () => {
    setReloadToken((value) => value + 1);
    await reloadNamespaceSummary();
  }, [reloadNamespaceSummary]);

  useEffect(() => {
    let cancelled = false;
    const unregister = libraryInvalidation.register('tags', () => {
      if (cancelled) return;
      void refreshData();
      if (selectedRef.current) void loadRelations(selectedRef.current);
    });
    libraryInvalidation.start();
    return () => {
      cancelled = true;
      unregister();
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

  const handleQueryChange = useCallback((value: string) => {
    resetBrowse();
    setQuery(value);
  }, [resetBrowse]);

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
      setTags((current) => [...current, ...page.items].slice(-MAX_RENDERED_TAGS));
      setCursor(page.next_cursor);
    }).catch((reason: unknown) => {
      if (generation === listGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (generation === listGeneration.current) setLoadingMore(false);
    });
  }, [cursor, loadingMore, namespace, query]);

  const refresh = useCallback(() => {
    setError(null);
    void refreshData();
  }, [refreshData]);

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
    if (mode === 'alias') {
      void runMutation(() => tagsController.setAlias(target.tag_id, currentTag.tag_id));
    } else if (mode === 'parent') {
      void runMutation(() => tagsController.setImplication(currentTag.tag_id, target.tag_id, true));
    } else {
      void runMutation(() => tagsController.setImplication(target.tag_id, currentTag.tag_id, true));
    }
  }, [runMutation]);

  const removeRelation = useCallback((relation: CanonicalTagRelation) => {
    const currentTag = selectedRef.current;
    if (!currentTag) return;
    if (relation.relation === 'alias_outgoing') {
      void runMutation(() => tagsController.setAlias(currentTag.tag_id, null));
    } else if (relation.relation === 'alias_incoming') {
      void runMutation(() => tagsController.setAlias(relation.tag_id, null));
    } else if (relation.relation === 'parent') {
      void runMutation(() => tagsController.setImplication(currentTag.tag_id, relation.tag_id, false));
    } else if (relation.relation === 'child') {
      void runMutation(() => tagsController.setImplication(relation.tag_id, currentTag.tag_id, false));
    }
  }, [runMutation]);

  const namespaceLabel = (value: string) => value || 'general';
  const totalLoadedLabel = `${tags.length}${cursor ? '+' : ''} tags loaded`;

  return (
    <div className={styles.root}>
      <header className={styles.header}>
        <div>
          <h1 className={styles.title}>Tag Manager</h1>
          <p className={styles.subtitle}>Browse every tag, including tags with no current media.</p>
        </div>
        <button className={styles.iconButton} onClick={refresh} title="Refresh tags" aria-label="Refresh tags" type="button">
          <IconRefresh size={17} />
        </button>
      </header>

      <div className={styles.toolbar}>
        <div className={styles.searchField}>
          <IconSearch size={15} />
          <input
            value={query}
            onChange={(event) => handleQueryChange(event.target.value)}
            placeholder="Search tags"
            aria-label="Search tags"
          />
          {query && <button onClick={() => handleQueryChange('')} aria-label="Clear tag search" type="button"><IconX size={14} /></button>}
        </div>
        <span className={styles.toolbarHint}>{totalLoadedLabel}</span>
      </div>

      {error && <div className={styles.error} role="alert">{error}</div>}

      <div className={styles.content}>
        <nav className={styles.namespaceRail} aria-label="Tag namespaces">
          <div className={styles.railHeading}>Namespaces</div>
          <button
            className={`${styles.namespaceItem} ${namespace === null ? styles.namespaceItemActive : ''}`}
            onClick={() => handleNamespaceChange(null)}
            type="button"
          >
            <span>All tags</span>
            <span className={styles.namespaceCount}>{namespaces.reduce((sum, item) => sum + item.count, 0)}</span>
          </button>
          {namespaces.map((item) => (
            <button
              className={`${styles.namespaceItem} ${namespace === item.namespace ? styles.namespaceItemActive : ''}`}
              key={item.namespace || '__general__'}
              onClick={() => handleNamespaceChange(item.namespace)}
              type="button"
            >
              <span>{namespaceLabel(item.namespace)}</span>
              <span className={styles.namespaceCount}>{item.count}</span>
            </button>
          ))}
        </nav>

        <section className={styles.browseSurface} aria-label="Tags">
          <div className={styles.canvasHeader}>
            <div>
              <span className={styles.canvasTitle}>
                {namespace === null ? 'All tags' : namespaceLabel(namespace)}
              </span>
              {query && <span className={styles.canvasQuery}> matching "{query}"</span>}
            </div>
            {loading && <span className={styles.muted}>Loading...</span>}
          </div>
          <div className={styles.tagCanvas}>
            {!loading && tags.length === 0 && (
              <EmptyState title="No tags found" description="Try a different search or namespace." />
            )}
            {tags.map((tag) => (
              <button
                className={styles.tagCard}
                key={tag.tag_id}
                onClick={() => { setSelected(tag); setEditorAction(null); }}
                type="button"
              >
                <span className={styles.tagCardMark}><IconTag size={13} /></span>
                <TagChip namespace={tag.namespace} subtag={tag.subtag} />
                <span className={styles.tagCardCount}>{tag.file_count} media</span>
              </button>
            ))}
          </div>
          {cursor && (
            <button className={styles.loadMore} onClick={loadMore} disabled={loadingMore} type="button">
              {loadingMore ? 'Loading...' : 'Load more tags'}
            </button>
          )}
        </section>
      </div>

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
            void runMutation(() => tagsController.merge(selected.tag_id, tagKey(target)), true);
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
