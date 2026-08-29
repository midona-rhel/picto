import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { atom, useAtom, useAtomValue } from 'jotai';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  IconEdit,
  IconFolderQuestion,
  IconGitMerge,
  IconBookmark,
  IconBookmarks,
  IconPlus,
  IconSearch,
  IconStar,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { tagsController } from '../../controllers/tagsController';
import type {
  CanonicalNamespaceSummary,
  CanonicalTagRecord,
} from '../../shared/types/canonical';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import { GlassInput } from '../../shared/ui/GlassInput/GlassInput';
import { GlassModal } from '../../shared/ui/GlassModal';
import { ConfirmModal } from '../modals/ConfirmModal';
import { ActionButton } from '../subscriptions/components/ActionButton';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { tagGroupColor, tagGroupOrder } from './tagGroupPresentation';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { showTagManagerItems } from '../../controllers/gridNavigationController';
import {
  buildCommonTagContextEntries,
  tagName,
  tagNameInGroup,
} from './tagContextMenu';
import {
  removeTagGroupColor,
  replaceTagGroupColor,
  replaceStarredTag,
  setTagGroupColor,
  setTagStarred,
  useTagPreferences,
} from './tagPreferences';
import styles from './TagManagerScreen.module.css';

const PAGE_SIZE = 500;
const PICKER_PAGE_SIZE = 40;
const tagManagerQueryAtom = atom('');

type TagBrowseRow =
  | { kind: 'section'; key: string; label: string; count: number }
  | { kind: 'tags'; key: string; tags: CanonicalTagRecord[]; sectionEnd: boolean }
  | { kind: 'skeleton'; key: string };

type EditorAction =
  | { kind: 'merge' }
  | { kind: 'delete' }
  | null;
type GroupAction = {
  kind: 'rename' | 'delete';
  namespaceId: number;
  namespace: string;
} | null;

function tagKey(tag: Pick<CanonicalTagRecord, 'namespace' | 'subname'>): string {
  return tagName(tag);
}

export function mergeUniqueTags(
  current: CanonicalTagRecord[],
  incoming: CanonicalTagRecord[],
): CanonicalTagRecord[] {
  const merged = new Map(current.map((tag) => [tag.tag_id, tag]));
  for (const tag of incoming) merged.set(tag.tag_id, tag);
  return [...merged.values()];
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
      setResults(page.tags.filter((item) => item.tag_id !== excludeTagId));
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
              <TagChip namespace={tag.namespace} subtag={tag.subname} />
              <span className={styles.pickerCount}>{tag.active_count}</span>
            </button>
          ))}
        </div>
      </div>
    </GlassModal>
  );
}

function RenameTagGroupModal({
  namespace,
  busy,
  onClose,
  onRename,
}: {
  namespace: string;
  busy: boolean;
  onClose: () => void;
  onRename: (name: string) => void;
}) {
  const [name, setName] = useState(namespace);
  const normalized = name.trim().toLowerCase().replace(/\s+/g, '_');
  const canRename = normalized.length > 0 && normalized !== namespace && !normalized.includes(':');
  return (
    <GlassModal
      open
      onClose={onClose}
      title="Rename tag group"
      size="sm"
      footer={(
        <>
          <ActionButton onClick={onClose} disabled={busy}>Cancel</ActionButton>
          <ActionButton onClick={() => onRename(normalized)} disabled={busy || !canRename}>Rename</ActionButton>
        </>
      )}
    >
      <GlassInput
        autoFocus
        aria-label="Tag group name"
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => { if (event.key === 'Enter' && canRename) onRename(normalized); }}
      />
    </GlassModal>
  );
}

function CreateTagGroupModal({
  busy,
  onClose,
  onCreate,
}: {
  busy: boolean;
  onClose: () => void;
  onCreate: (name: string) => void;
}) {
  const [name, setName] = useState('');
  const normalized = name.trim().toLowerCase().replace(/\s+/g, '_');
  const canCreate = normalized.length > 0 && !normalized.includes(':');
  return (
    <GlassModal
      open
      onClose={onClose}
      title="Create tag group"
      size="sm"
      footer={(
        <>
          <ActionButton onClick={onClose} disabled={busy}>Cancel</ActionButton>
          <ActionButton onClick={() => onCreate(normalized)} disabled={busy || !canCreate}>Create</ActionButton>
        </>
      )}
    >
      <GlassInput
        autoFocus
        aria-label="Tag group name"
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => { if (event.key === 'Enter' && canCreate) onCreate(normalized); }}
        placeholder="Group name"
      />
    </GlassModal>
  );
}

function TagEditorModal({
  tag,
  busy,
  onClose,
  onRename,
  onAction,
}: {
  tag: CanonicalTagRecord;
  busy: boolean;
  onClose: () => void;
  onRename: (name: string) => void;
  onAction: (action: Exclude<EditorAction, null>) => void;
}) {
  const [name, setName] = useState(tagKey(tag));

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
            <TagChip namespace={tag.namespace} subtag={tag.subname} />
            <div className={styles.editorCount}>{tag.assignment_count} media items</div>
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

      </div>
    </GlassModal>
  );
}

export function TagManagerScreen() {
  const [tags, setTags] = useState<CanonicalTagRecord[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const [unusedCount, setUnusedCount] = useState(0);
  const query = useAtomValue(tagManagerQueryAtom);
  const [namespace, setNamespace] = useState<string | null>(null);
  const [showStarred, setShowStarred] = useState(false);
  const [selected, setSelected] = useState<CanonicalTagRecord | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editorAction, setEditorAction] = useState<EditorAction>(null);
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [groupAction, setGroupAction] = useState<GroupAction>(null);
  const [createGroupOpen, setCreateGroupOpen] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);
  const [viewEpoch, setViewEpoch] = useState(0);
  const [viewTransition, setViewTransition] = useState<'idle' | 'leaving' | 'entering'>('idle');
  const listGeneration = useRef(0);
  const loadingCursor = useRef<string | null>(null);
  const summaryGeneration = useRef(0);
  const viewTransitionTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const invalidationTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastInvalidationRefreshAt = useRef(0);
  const browseIdentity = useRef<string | null>(null);
  const selectedRef = useRef<CanonicalTagRecord | null>(null);
  const tagCanvasRef = useRef<HTMLDivElement>(null);
  const [columnCount, setColumnCount] = useState(3);
  const contextMenu = useContextMenu();
  const tagPreferences = useTagPreferences();
  const tagGroupColors = tagPreferences.tagGroupColors ?? {};
  selectedRef.current = selected;

  const reloadNamespaceSummary = useCallback(async () => {
    const generation = ++summaryGeneration.current;
    try {
      const [items, unused] = await Promise.all([
        tagsController.getNamespaceSummary(),
        tagsController.getUnusedCount(),
      ]);
      if (generation === summaryGeneration.current) {
        setNamespaces(items);
        setUnusedCount(unused);
      }
    } catch (reason: unknown) {
      if (generation === summaryGeneration.current) setError(errorMessage(reason));
    }
  }, []);

  useEffect(() => {
    void reloadNamespaceSummary();
  }, [reloadNamespaceSummary]);

  useEffect(() => {
    const generation = ++listGeneration.current;
    loadingCursor.current = null;
    setLoadingMore(false);
    let cancelled = false;
    const nextBrowseIdentity = `${namespace ?? '*'}\u0000${query}\u0000${showStarred ? 'starred' : 'browse'}`;
    const browseChanged = browseIdentity.current !== nextBrowseIdentity;
    browseIdentity.current = nextBrowseIdentity;
    if (browseChanged) setLoading(true);
    const request = showStarred
      ? Promise.all(tagPreferences.starredTags.map(async (name) => {
          const separator = name.indexOf(':');
          const starredNamespace = separator >= 0 ? name.slice(0, separator) : '';
          const subname = separator >= 0 ? name.slice(separator + 1) : name;
          const page = await tagsController.getPaginated({
            namespace: starredNamespace,
            search: subname,
            cursor: null,
            limit: PAGE_SIZE,
          });
          return page.tags.find((tag) => tagKey(tag) === name) ?? null;
        })).then((records) => ({
          tags: records
            .filter((tag): tag is CanonicalTagRecord => tag != null)
            .filter((tag) => !query.trim() || tag.subname.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())),
          next_cursor: null,
          revision: 0,
        }))
      : tagsController.getPaginated({
          namespace,
          search: query.trim() || null,
          cursor: null,
          limit: PAGE_SIZE,
        });
    void request.then((page) => {
      if (cancelled || generation !== listGeneration.current) return;
      setTags(mergeUniqueTags([], page.tags));
      setCursor(page.next_cursor);
      if (browseChanged) {
        setViewEpoch((value) => value + 1);
        setViewTransition('entering');
        if (viewTransitionTimer.current) clearTimeout(viewTransitionTimer.current);
        viewTransitionTimer.current = setTimeout(() => setViewTransition('idle'), 140);
      }
    }).catch((reason: unknown) => {
      if (!cancelled && generation === listGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (!cancelled && generation === listGeneration.current) setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [namespace, query, reloadToken, showStarred, tagPreferences.starredTags]);

  useEffect(() => () => {
    if (viewTransitionTimer.current) clearTimeout(viewTransitionTimer.current);
    if (invalidationTimer.current) clearTimeout(invalidationTimer.current);
  }, []);

  useEffect(() => {
    setSelected(null);
    setEditorAction(null);
  }, [query]);

  const refreshData = useCallback(async () => {
    setReloadToken((value) => value + 1);
    await reloadNamespaceSummary();
  }, [reloadNamespaceSummary]);

  const scheduleInvalidationRefresh = useCallback(() => {
    if (invalidationTimer.current) return;
    const elapsed = Date.now() - lastInvalidationRefreshAt.current;
    const delay = Math.max(0, 500 - elapsed);
    invalidationTimer.current = setTimeout(() => {
      invalidationTimer.current = null;
      lastInvalidationRefreshAt.current = Date.now();
      void refreshData();
    }, delay);
  }, [refreshData]);

  useEffect(() => {
    let cancelled = false;
    const unregister = libraryInvalidation.register('tags', () => {
      if (cancelled) return;
      scheduleInvalidationRefresh();
    });
    libraryInvalidation.start();
    return () => {
      cancelled = true;
      unregister();
    };
  }, [scheduleInvalidationRefresh]);

  const resetBrowse = useCallback(() => {
    listGeneration.current += 1;
    loadingCursor.current = null;
    setTags([]);
    setCursor(null);
    setLoading(true);
    setError(null);
    setSelected(null);
    setEditorAction(null);
  }, []);

  const handleNamespaceChange = useCallback((value: string | null) => {
    if (!showStarred && value === namespace) return;
    if (viewTransitionTimer.current) clearTimeout(viewTransitionTimer.current);
    setViewTransition('leaving');
    viewTransitionTimer.current = setTimeout(() => {
      resetBrowse();
      setShowStarred(false);
      setNamespace(value);
    }, 100);
  }, [namespace, resetBrowse, showStarred]);

  const handleStarredChange = useCallback(() => {
    if (showStarred) return;
    if (viewTransitionTimer.current) clearTimeout(viewTransitionTimer.current);
    setViewTransition('leaving');
    viewTransitionTimer.current = setTimeout(() => {
      resetBrowse();
      setShowStarred(true);
    }, 100);
  }, [resetBrowse, showStarred]);

  const loadMore = useCallback(() => {
    if (!cursor || loadingCursor.current === cursor) return;
    const generation = listGeneration.current;
    const requestedCursor = cursor;
    loadingCursor.current = requestedCursor;
    setLoadingMore(true);
    void tagsController.getPaginated({
      namespace,
      search: query.trim() || null,
      cursor: requestedCursor,
      limit: PAGE_SIZE,
    }).then((page) => {
      if (generation !== listGeneration.current) return;
      setTags((current) => mergeUniqueTags(current, page.tags));
      // A repeated cursor is a malformed/non-advancing page. Stop rather than
      // repeatedly fetching and reconciling the same tag records.
      setCursor(page.next_cursor === requestedCursor ? null : page.next_cursor);
    }).catch((reason: unknown) => {
      if (generation === listGeneration.current) setError(errorMessage(reason));
    }).finally(() => {
      if (generation === listGeneration.current && loadingCursor.current === requestedCursor) {
        loadingCursor.current = null;
        setLoadingMore(false);
      }
    });
  }, [cursor, namespace, query]);

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
    if (showStarred) return tagPreferences.starredTags.length;
    if (namespace === null) return namespaces.reduce((sum, item) => sum + item.tag_count, 0);
    return namespaces.find((item) => item.name === namespace)?.tag_count ?? tags.length;
  }, [namespace, namespaces, query, showStarred, tagPreferences.starredTags.length, tags.length]);

  const virtualItemCount = cursor || loading
    ? Math.max(tags.length, expectedTagCount)
    : tags.length;
  const browseRows = useMemo<TagBrowseRow[]>(() => {
    const rows: TagBrowseRow[] = [];
    const ordered = [...tags].sort((left, right) => (
      left.subname.localeCompare(right.subname, undefined, { sensitivity: 'base', numeric: true })
    ));
    const sections = new Map<string, CanonicalTagRecord[]>();
    for (const tag of ordered) {
      const first = tag.subname.trim().charAt(0).toLocaleUpperCase();
      const label = /[\p{L}\p{N}]/u.test(first) ? first : '#';
      const section = sections.get(label) ?? [];
      section.push(tag);
      sections.set(label, section);
    }
    for (const [label, sectionTags] of sections) {
      rows.push({ kind: 'section', key: `section-${label}`, label, count: sectionTags.length });
      for (let offset = 0; offset < sectionTags.length; offset += columnCount) {
        rows.push({
          kind: 'tags',
          key: `tags-${label}-${offset}`,
          tags: sectionTags.slice(offset, offset + columnCount),
          sectionEnd: offset + columnCount >= sectionTags.length,
        });
      }
    }
    const remaining = Math.max(0, virtualItemCount - tags.length);
    for (let index = 0; index < Math.ceil(remaining / columnCount); index += 1) {
      rows.push({ kind: 'skeleton', key: `skeleton-row-${index}` });
    }
    return rows;
  }, [columnCount, tags, virtualItemCount]);
  const tagVirtualizer = useVirtualizer({
    count: browseRows.length,
    getScrollElement: () => tagCanvasRef.current,
    estimateSize: (index) => browseRows[index]?.kind === 'section' ? 42 : 27,
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
    if (lastVirtualRowIndex >= browseRows.length - 4) loadMore();
  }, [browseRows.length, cursor, lastVirtualRowIndex, loadMore, loadingMore]);

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
        setEditorAction(null);
      }
      return true;
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      return false;
    } finally {
      setBusy(false);
    }
  }, [closeEditor, refreshData]);

  const namespaceLabel = (value: string) => value || 'general';
  const sortedNamespaces = useMemo(
    () => namespaces
      .filter((group) => group.name !== '' && group.name !== 'general')
      .sort((left, right) => tagGroupOrder(left.name) - tagGroupOrder(right.name)),
    [namespaces],
  );
  const openTagContextMenu = useCallback((event: React.MouseEvent, tag: CanonicalTagRecord) => {
    const entries: MenuEntry[] = [
      ...buildCommonTagContextEntries({
        tag,
        namespaces,
        starred: tagPreferences.starredTags.includes(tagKey(tag)),
        onFilter: showTagManagerItems,
        onStarChange: (name, starred) => { void setTagStarred(name, starred); },
        onMoveToGroup: (targetNamespace) => {
          const previousName = tagKey(tag);
          const nextName = tagNameInGroup(tag, targetNamespace);
          void runMutation(async () => {
            await tagsController.rename(tag.tag_id, nextName);
            await replaceStarredTag(previousName, nextName);
          });
        },
      }),
      { separator: true },
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
  }, [contextMenu, namespaces, runMutation, tagPreferences.starredTags]);

  const openGroupContextMenu = useCallback((
    event: React.MouseEvent,
    group: CanonicalNamespaceSummary,
  ) => {
    contextMenu.open(event, [
      {
        label: 'Show Tags in Group',
        icon: <IconBookmark size={16} />,
        action: () => handleNamespaceChange(group.name),
      },
      { separator: true },
      {
        label: 'Rename Group…',
        icon: <IconEdit size={16} />,
        action: () => setGroupAction({
          kind: 'rename',
          namespaceId: group.namespace_id,
          namespace: group.name,
        }),
      },
      {
        custom: true,
        key: 'group-color',
        render: () => (
          <ColorPicker
            value={tagGroupColors[group.name] ?? null}
            onChange={(color) => { void setTagGroupColor(group.name, color); }}
          />
        ),
      },
      {
        label: 'Delete Group',
        icon: <IconTrash size={16} />,
        action: () => setGroupAction({
          kind: 'delete',
          namespaceId: group.namespace_id,
          namespace: group.name,
        }),
      },
    ]);
  }, [contextMenu, handleNamespaceChange, tagGroupColors]);

  return (
    <div className={styles.root}>
      {error && <div className={styles.error} role="alert">{error}</div>}

      <div className={styles.content}>
        <nav className={styles.namespaceRail} aria-label="Tag groups">
          <button
            className={`${styles.namespaceItem} ${!showStarred && namespace === null ? styles.namespaceItemActive : ''}`}
            onClick={() => handleNamespaceChange(null)}
            type="button"
          >
            <span className={styles.groupIdentity}><IconBookmarks size={16} /><span>All</span></span>
            <span className={styles.namespaceCount}>{namespaces.reduce((sum, item) => sum + item.tag_count, 0)}</span>
          </button>
          <button
            className={`${styles.namespaceItem} ${!showStarred && namespace === '' ? styles.namespaceItemActive : ''}`}
            onClick={() => handleNamespaceChange('')}
            type="button"
          >
            <span className={styles.groupIdentity}><IconFolderQuestion size={16} /><span>Uncategorized</span></span>
            <span className={styles.namespaceCount}>{namespaces.find((item) => item.name === '')?.tag_count ?? 0}</span>
          </button>
          <button
            className={`${styles.namespaceItem} ${showStarred ? styles.namespaceItemActive : ''}`}
            onClick={handleStarredChange}
            type="button"
          >
            <span className={styles.groupIdentity}><IconStar size={16} /><span>Starred</span></span>
            <span className={styles.namespaceCount}>{tagPreferences.starredTags.length}</span>
          </button>
          <div className={styles.railHeading}>
            <span>Groups({sortedNamespaces.length})</span>
            <button
              aria-label="Create tag group"
              className={styles.addGroupButton}
              disabled={busy}
              onClick={() => setCreateGroupOpen(true)}
              title="Create tag group"
              type="button"
            >
              <IconPlus size={15} />
            </button>
          </div>
          {sortedNamespaces.map((item) => {
            return (
              <button
                className={`${styles.namespaceItem} ${namespace === item.name ? styles.namespaceItemActive : ''}`}
                key={item.namespace_id}
                onClick={() => handleNamespaceChange(item.name)}
                onContextMenu={(event) => openGroupContextMenu(event, item)}
                type="button"
              >
                <span className={styles.groupIdentity} style={{ color: tagGroupColor(item.name, tagGroupColors) }}>
                  <IconBookmark size={16} fill="currentColor" fillOpacity={0.35} />
                  <span className={styles.groupName}>{namespaceLabel(item.name)}</span>
                </span>
                <span className={styles.namespaceCount}>{item.tag_count}</span>
              </button>
            );
          })}
          <button
            className={styles.cleanupTagsButton}
            disabled={unusedCount === 0 || busy}
            onClick={() => setCleanupOpen(true)}
            type="button"
          >
            <IconTrash size={14} />
            <span>Delete Unused</span>
            <span className={styles.namespaceCount}>{unusedCount}</span>
          </button>
        </nav>

        <section
          className={`${styles.browseSurface} ${viewTransition === 'leaving' ? styles.viewLeaving : ''} ${viewTransition === 'entering' ? styles.viewEntering : ''}`}
          aria-label="Tags"
        >
          <div className={styles.tagCanvas} ref={tagCanvasRef}>
            {!loading && tags.length === 0 && (
              <div className={styles.noTags}>No tags</div>
            )}
            <div
              className={styles.tagListInner}
              key={viewEpoch}
              style={{ height: tagVirtualizer.getTotalSize() }}
            >
              {virtualRows.map((virtualRow) => {
                const row = browseRows[virtualRow.index];
                if (!row) return null;
                return row.kind === 'section' ? (
                  <div
                    className={styles.tagSectionHeader}
                    key={virtualRow.key}
                    style={{ transform: `translateY(${virtualRow.start}px)` }}
                  >
                    <span>{row.label}</span>
                    <span className={styles.tagSectionCount}>({row.count})</span>
                  </div>
                ) : (
                  <div
                    className={`${styles.tagVirtualRow} ${row.kind === 'tags' && row.sectionEnd ? styles.tagVirtualRowEnd : ''}`}
                    key={virtualRow.key}
                    style={{
                      gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                  >
                    {row.kind === 'tags' ? row.tags.map((tag) => (
                      <button
                        className={styles.tagCard}
                        key={tag.tag_id}
                        onClick={() => { setSelected(tag); setEditorAction(null); }}
                        onContextMenu={(event) => openTagContextMenu(event, tag)}
                        type="button"
                      >
                        <span className={styles.tagDot} style={{ background: tagGroupColor(tag.namespace, tagGroupColors) }} />
                        <span className={styles.tagName}>{tag.subname}</span>
                        {tagPreferences.starredTags.includes(tagKey(tag)) && (
                          <IconStar aria-label="Starred" className={styles.starredIcon} size={12} fill="currentColor" />
                        )}
                        <span className={styles.tagCardCount}>({tag.assignment_count})</span>
                      </button>
                    )) : Array.from({ length: columnCount }, (_, columnIndex) => (
                      <div className={styles.tagSkeleton} key={`skeleton-${virtualRow.index}-${columnIndex}`} />
                    ))}
                  </div>
                );
              })}
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
          busy={busy}
          onClose={closeEditor}
          onRename={(name) => void runMutation(() => tagsController.rename(selected.tag_id, name), true)}
          onAction={setEditorAction}
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
      {groupAction?.kind === 'rename' && (
        <RenameTagGroupModal
          namespace={groupAction.namespace}
          busy={busy}
          onClose={() => setGroupAction(null)}
          onRename={(newNamespace) => {
            const previousNamespace = groupAction.namespace;
            void runMutation(async () => {
              await tagsController.renameGroup(groupAction.namespaceId, newNamespace);
              await replaceTagGroupColor(previousNamespace, newNamespace);
            })
              .then((succeeded) => {
                if (!succeeded) return;
                setGroupAction(null);
                if (namespace === previousNamespace) handleNamespaceChange(newNamespace);
              });
          }}
        />
      )}
      {groupAction?.kind === 'delete' && (
        <ConfirmModal
          open
          onClose={() => setGroupAction(null)}
          onConfirm={() => {
            const deletedNamespace = groupAction.namespace;
            void runMutation(async () => {
              await tagsController.deleteGroup(groupAction.namespaceId);
              await removeTagGroupColor(deletedNamespace);
            })
              .then((succeeded) => {
                if (!succeeded) return;
                setGroupAction(null);
                if (namespace === deletedNamespace) handleNamespaceChange(null);
              });
          }}
          title="Delete tag group"
          message={`Delete the ${groupAction.namespace} group? Its tags will move to General; no tags or media assignments will be deleted.`}
          confirmLabel="Delete group"
          loading={busy}
        />
      )}
      {createGroupOpen && (
        <CreateTagGroupModal
          busy={busy}
          onClose={() => setCreateGroupOpen(false)}
          onCreate={(name) => {
            void runMutation(() => tagsController.createGroup(name)).then((succeeded) => {
              if (!succeeded) return;
              setCreateGroupOpen(false);
              handleNamespaceChange(name);
            });
          }}
        />
      )}
      <ConfirmModal
        open={cleanupOpen}
        onClose={() => setCleanupOpen(false)}
        onConfirm={() => {
          setCleanupOpen(false);
          void runMutation(() => tagsController.deleteUnused());
        }}
        title="Delete unused tags"
        message={`Delete ${unusedCount.toLocaleString()} ${unusedCount === 1 ? 'tag' : 'tags'} with no assignments anywhere in the library? Tags used by Inbox or Trash items are kept.`}
        confirmLabel="Delete unused tags"
        danger
        loading={busy}
      />
    </div>
  );
}

export function TagsToolbar() {
  const [query, setQuery] = useAtom(tagManagerQueryAtom);
  const inputRef = useRef<HTMLInputElement>(null);

  useShortcutScope((event) => {
    const search = getShortcut('nav.search');
    if (!search || !matchesShortcutDef(event, search)) return;
    inputRef.current?.focus();
    return true;
  }, { priority: 30 });

  return (
    <div className={styles.titlebarToolbar} data-window-drag-region="">
      <div
        className={styles.titlebarSearch}
        onMouseDown={(event) => {
          if (!(event.target as Element).closest('button')) inputRef.current?.focus();
        }}
      >
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
