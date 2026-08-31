import { useCallback, useEffect, useMemo, useRef, useState, type SyntheticEvent } from 'react';
import { useSetAtom } from 'jotai';
import { viewerController } from '../../controllers/viewerController';
import { MediaView } from '../viewer/MediaView';
import { QuickLook } from '../viewer/QuickLook';
import { DetailMediaRenderer } from '../viewer/document/DetailMediaRenderer';
import { detailRendererKind } from '../viewer/document/detailRendererKind';
import { CanvasGrid } from '../grid/canvas/CanvasGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import type { CanonicalEntityDetails, CanonicalEntityGridItem, MediaRecord } from '../../shared/types/canonical';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { detachItems, reorderGroup, ungroup } from '../../platform/entityApi';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import { confirmModalAtom, exportModalAtom } from '../../state/modals';
import { aiTaggerPortalAtom, folderPickerPortalAtom, inspectorAnchor, tagSelectPortalAtom } from '../../state/portals';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { buildTileContextMenu } from '../grid/gridContextMenu';
import styles from './GroupSurface.module.css';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { GroupRemoveIcon } from '../../shared/ui/icons/group-icons';
import * as entityMutations from '../../controllers/entityMutations';
import { openCurrentLibraryCoverPicker } from '../library/libraryAppearance';
import { showErrorNotification } from '../../shared/lib/notifications';
import { reverseImageSearch } from '../../platform/shellApi';
import { tagsController } from '../../controllers/tagsController';
import { tagName } from '../tags/tagContextMenu';
import { navigateToNode } from '../../state/navigationHistory';
import { contentSortSubmenu } from '../folders/folderContextMenu';
import { useRecordMediaView } from '../viewer/hooks/useRecordMediaView';

export interface GroupSurfaceProps {
  groupId: number;
  initialMode?: 'reader' | 'editor';
  presentation?: 'detail' | 'quicklook';
  breadcrumbParent?: string;
  rootCurrentIndex: number;
  rootTotal: number;
  onNavigateRoot: (delta: number) => void;
  onClose: () => void;
  /** Root recorded as recently viewed. Use null when history itself is being browsed. */
  recordItemId?: number | null;
}

function memberToGridItem(details: CanonicalEntityDetails, media: MediaRecord): CanonicalEntityGridItem {
  return {
    root_id: media.media_id,
    kind: 'media',
    lifecycle: details.lifecycle,
    name: media.media_name,
    cover_media_id: media.media_id,
    content_hash: media.facts.content_hash,
    mime: media.facts.mime,
    width: media.facts.width,
    height: media.facts.height,
    duration_ms: media.facts.duration_ms,
    frame_count: media.facts.frame_count,
    palette: media.facts.palette,
    imported_at_ms: details.root.imported_at_ms,
    captured_at_ms: details.root.captured_at_ms,
    modified_at_ms: details.root.modified_at_ms,
    rating: details.rating,
    media_count: 1,
    total_size_bytes: media.facts.size_bytes,
  };
}

export function groupSelectionForClick(
  current: ReadonlySet<number>,
  orderedIds: readonly number[],
  itemId: number,
  anchorId: number | null,
  modifiers: { toggle: boolean; range: boolean },
): { selected: Set<number>; anchorId: number } {
  if (modifiers.range && anchorId != null) {
    const anchorIndex = orderedIds.indexOf(anchorId);
    const itemIndex = orderedIds.indexOf(itemId);
    if (anchorIndex >= 0 && itemIndex >= 0) {
      const [start, end] = anchorIndex <= itemIndex
        ? [anchorIndex, itemIndex]
        : [itemIndex, anchorIndex];
      return { selected: new Set(orderedIds.slice(start, end + 1)), anchorId };
    }
  }
  if (modifiers.toggle) {
    const selected = new Set(current);
    if (selected.has(itemId)) selected.delete(itemId); else selected.add(itemId);
    return { selected, anchorId: itemId };
  }
  return { selected: new Set([itemId]), anchorId: itemId };
}

const FULL_MEDIA_VISIBILITY_DELAY_MS = 200;
const MAX_WARM_GROUP_MEDIA = 100;

export function retainWarmGroupMedia(
  current: readonly number[],
  itemId: number,
  limit = MAX_WARM_GROUP_MEDIA,
): number[] {
  return [...current.filter((currentId) => currentId !== itemId), itemId].slice(-limit);
}

function useWarmGroupMediaBudget(groupId: number) {
  const orderRef = useRef<number[]>([]);
  const loadedRef = useRef<Set<number>>(new Set());
  const [loadedItemIds, setLoadedItemIds] = useState<Set<number>>(new Set());

  useEffect(() => {
    orderRef.current = [];
    loadedRef.current = new Set();
    setLoadedItemIds(new Set());
  }, [groupId]);

  const touch = useCallback((itemId: number) => {
    if (!loadedRef.current.has(itemId)) return;
    orderRef.current = retainWarmGroupMedia(orderRef.current, itemId);
  }, []);

  const request = useCallback((itemId: number) => {
    const nextOrder = retainWarmGroupMedia(orderRef.current, itemId);
    orderRef.current = nextOrder;
    const nextLoaded = new Set(nextOrder);
    loadedRef.current = nextLoaded;
    setLoadedItemIds(nextLoaded);
  }, []);

  return { loadedItemIds, request, touch };
}

function useDeferredVisibleMedia(
  itemId: number,
  loadFullMedia: boolean,
  onRequest: (itemId: number) => void,
  onVisible: (itemId: number) => void,
) {
  const frameRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;

    let timer: ReturnType<typeof setTimeout> | null = null;
    let observer: IntersectionObserver | null = null;
    const cancel = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const schedule = () => {
      if (timer !== null) return;
      timer = setTimeout(() => {
        timer = null;
        onRequest(itemId);
      }, FULL_MEDIA_VISIBILITY_DELAY_MS);
    };

    if (typeof IntersectionObserver === 'undefined') {
      onVisible(itemId);
      if (!loadFullMedia) schedule();
    } else {
      observer = new IntersectionObserver(([entry]) => {
        if (entry?.isIntersecting) {
          onVisible(itemId);
          if (!loadFullMedia) schedule();
        } else cancel();
      }, { threshold: 0.01 });
      observer.observe(frame);
    }

    return () => {
      cancel();
      observer?.disconnect();
    };
  }, [itemId, loadFullMedia, onRequest, onVisible]);

  return frameRef;
}

interface GroupDeferredMediaProps {
  item: CanonicalEntityGridItem;
  loadFullMedia: boolean;
  onRequest: (itemId: number) => void;
  onVisible: (itemId: number) => void;
}

function GroupImage({ item, loadFullMedia, onRequest, onVisible }: GroupDeferredMediaProps) {
  const [fullVisible, setFullVisible] = useState(false);
  const frameRef = useDeferredVisibleMedia(item.root_id, loadFullMedia, onRequest, onVisible);
  useEffect(() => {
    if (!loadFullMedia) setFullVisible(false);
  }, [loadFullMedia]);
  const aspectRatio = item.width && item.height
    ? `${item.width} / ${item.height}`
    : undefined;

  return (
    <div ref={frameRef} className={styles.imageFrame} style={{ aspectRatio }}>
      <ThumbnailImage
        className={styles.thumbnailImage}
        src={mediaThumbnailUrl(item.content_hash)}
        alt=""
        draggable={false}
      />
      {loadFullMedia && (
        <img
          className={`${styles.fullImage} ${fullVisible ? styles.fullImageVisible : ''}`}
          src={mediaFileUrl(item.content_hash, item.mime)}
          alt={item.name ?? ''}
          decoding="async"
          onLoad={(event: SyntheticEvent<HTMLImageElement>) => {
            const reveal = () => setFullVisible(true);
            if (typeof event.currentTarget.decode === 'function') {
              event.currentTarget.decode().then(reveal).catch(reveal);
            } else reveal();
          }}
        />
      )}
    </div>
  );
}

function GroupDeferredRenderer({ item, loadFullMedia, onRequest, onVisible }: GroupDeferredMediaProps) {
  const frameRef = useDeferredVisibleMedia(item.root_id, loadFullMedia, onRequest, onVisible);
  const aspectRatio = item.width && item.height
    ? `${item.width} / ${item.height}`
    : undefined;

  return (
    <div ref={frameRef} className={styles.mediaFrame} style={{ aspectRatio }}>
      {!loadFullMedia && (
        <ThumbnailImage
          className={styles.deferredThumbnail}
          src={mediaThumbnailUrl(item.content_hash)}
          alt=""
          draggable={false}
        />
      )}
      {loadFullMedia && (
        <DetailMediaRenderer
          hash={item.content_hash}
          mimeType={item.mime}
          displayName={item.name}
          mediaKeyboardShortcutsEnabled={false}
          mediaAutoPlay={false}
          mediaLoop={false}
          mediaMuted={false}
        />
      )}
    </div>
  );
}

export function GroupSurface({
  groupId,
  initialMode = 'reader',
  presentation = 'detail',
  breadcrumbParent = 'Collections',
  rootCurrentIndex,
  rootTotal,
  onNavigateRoot,
  onClose,
  recordItemId,
}: GroupSurfaceProps) {
  useRecordMediaView(recordItemId === undefined ? groupId : recordItemId);
  const [details, setDetails] = useState<CanonicalEntityDetails | null>(
    () => viewerController.takePrefetchedItemDetails?.(groupId) ?? null,
  );
  const skipInitialRefreshRef = useRef(details !== null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState(initialMode);
  const [selectedItemIds, setSelectedItemIds] = useState<Set<number>>(new Set());
  const [selectionAnchorId, setSelectionAnchorId] = useState<number | null>(null);
  const [viewerIndex, setViewerIndex] = useState<number | null>(null);
  const [quickLookIndex, setQuickLookIndex] = useState<number | null>(null);
  const warmMedia = useWarmGroupMediaBudget(groupId);
  const contextMenu = useContextMenu();
  const setDisplayState = useSetAtom(viewerDisplayStateAtom);
  const setDisplayControls = useSetAtom(viewerDisplayControlsAtom);
  const setConfirmModal = useSetAtom(confirmModalAtom);
  const setTagPortal = useSetAtom(tagSelectPortalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);
  const setAiPortal = useSetAtom(aiTaggerPortalAtom);
  const setExportModal = useSetAtom(exportModalAtom);
  const members = useMemo(
    () => details?.media.map((media) => memberToGridItem(details, media)) ?? [],
    [details],
  );

  const refresh = useCallback(async () => {
    try {
      const next = await viewerController.getItemDetails(groupId);
      if (next.root.kind !== 'collection') {
        onClose();
        return;
      }
      setDetails(next);
      setSelectedItemIds((current) => {
        const memberIds = new Set(next.media.map((media) => media.media_id));
        return new Set([...current].filter((itemId) => memberIds.has(itemId)));
      });
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [groupId, onClose]);

  useEffect(() => {
    if (skipInitialRefreshRef.current) {
      skipInitialRefreshRef.current = false;
      return;
    }
    void refresh();
  }, [refresh]);
  useEffect(
    () => libraryInvalidation.register(`item:${groupId}`, () => { void refresh(); }),
    [groupId, refresh],
  );

  useEffect(() => {
    if (!details || viewerIndex !== null || presentation === 'quicklook') return;
    setDisplayState({
      currentIndex: rootCurrentIndex,
      total: rootTotal,
      breadcrumb: mode === 'editor' ? {
        parent: breadcrumbParent,
        current: details.root.name || 'Untitled',
      } : undefined,
    });
    setDisplayControls(mode === 'reader' ? {
      close: onClose,
      navigate: onNavigateRoot,
      edit: () => setMode('editor'),
    } : {
      close: onClose,
      backLabel: 'Back to grid',
    });
    return () => {
      setDisplayState(null);
      setDisplayControls(null);
    };
  }, [breadcrumbParent, details, initialMode, mode, onClose, onNavigateRoot, presentation, rootCurrentIndex, rootTotal, setDisplayControls, setDisplayState, viewerIndex]);

  const selectedItems = useMemo(
    () => members.filter((item) => selectedItemIds.has(item.root_id)),
    [members, selectedItemIds],
  );

  useShortcutScope((event) => {
      const closeDef = getShortcut('view.closeDetail')!;
      const detailDef = getShortcut('view.detailView')!;
      const quickLookDef = getShortcut('view.quicklook')!;
      const selectAllDef = getShortcut('edit.selectAll')!;
      const copyDef = getShortcut('edit.copy')!;
      const removeMembersDef = getShortcut('group.removeMembers')!;
      const previousDef = getShortcut('view.prevImage')!;
      const nextDef = getShortcut('view.nextImage')!;
      if (mode === 'reader' && (
        matchesShortcutDef(event, closeDef)
        || matchesShortcutDef(event, detailDef)
        || matchesShortcutDef(event, quickLookDef)
      )) {
        event.preventDefault();
        onClose();
        return true;
      }
      if (mode === 'editor' && matchesShortcutDef(event, quickLookDef) && selectedItems.length === 1) {
        event.preventDefault();
        setQuickLookIndex(members.findIndex((item) => item.root_id === selectedItems[0].root_id));
        return true;
      }
      if (mode === 'editor' && matchesShortcutDef(event, selectAllDef) && members.length > 0) {
        event.preventDefault();
        setSelectedItemIds(new Set(members.map((item) => item.root_id)));
        setSelectionAnchorId(members[0]?.root_id ?? null);
        return true;
      }
      if (mode === 'editor' && matchesShortcutDef(event, copyDef) && selectedItems.length > 0) {
        event.preventDefault();
        void filesController.copyHashes(selectedItems.map((item) => item.content_hash));
        return true;
      }
      if (mode === 'reader' && matchesShortcutDef(event, copyDef)) {
        event.preventDefault();
        void filesController.copyTarget({ kind: 'explicit', root_ids: [groupId] });
        return true;
      }
      if (mode === 'editor' && selectedItems.length > 0 && matchesShortcutDef(event, removeMembersDef)) {
        event.preventDefault();
        void detachMembers(selectedItems);
        return true;
      }
      if (mode === 'reader' && (matchesShortcutDef(event, previousDef) || matchesShortcutDef(event, nextDef))) {
        event.preventDefault();
        onNavigateRoot(matchesShortcutDef(event, previousDef) ? -1 : 1);
        return true;
      }
      if (!matchesShortcutDef(event, closeDef)) return;
      if (mode === 'editor' && selectedItemIds.size > 0) {
        setSelectedItemIds(new Set());
        setSelectionAnchorId(null);
        return true;
      }
      onClose();
      return true;
  }, { enabled: viewerIndex === null && quickLookIndex === null, priority: 30 });
  const memberIds = useMemo(() => members.map((item) => item.root_id), [members]);

  useEffect(() => {
    if (selectionAnchorId !== null && !memberIds.includes(selectionAnchorId)) {
      setSelectionAnchorId(null);
    }
  }, [memberIds, selectionAnchorId]);

  const selectMember = useCallback((itemId: number, event?: Pick<React.MouseEvent, 'metaKey' | 'ctrlKey' | 'shiftKey'>) => {
    const next = groupSelectionForClick(
      selectedItemIds,
      memberIds,
      itemId,
      selectionAnchorId,
      { toggle: Boolean(event?.metaKey || event?.ctrlKey), range: Boolean(event?.shiftKey) },
    );
    setSelectedItemIds(next.selected);
    setSelectionAnchorId(next.anchorId);
  }, [memberIds, selectedItemIds, selectionAnchorId]);

  const deselectAll = useCallback(() => {
    setSelectedItemIds(new Set());
    setSelectionAnchorId(null);
  }, []);

  const detachMembers = useCallback(async (
    selected: CanonicalEntityGridItem[],
    targetLifecycle: 'trash' | null = null,
  ) => {
    if (selected.length === 0) return;
    try {
      await detachItems({
        collection_id: groupId,
        media_ids: selected.map((item) => item.root_id),
        target_lifecycle: targetLifecycle,
      });
      await announceUndoableMutation('collections.detach');
      setSelectedItemIds(new Set());
      setSelectionAnchorId(null);
      if (members.length - selected.length <= 1) onClose();
      else await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [groupId, members.length, onClose, refresh]);

  const saveOrder = useCallback(async (orderedItemIds: number[]) => {
    try {
      await reorderGroup({ collection_id: groupId, media_ids: orderedItemIds });
      await announceUndoableMutation('collections.reorder');
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [groupId, refresh]);

  const sortCollectionContents = useCallback((field: 'name' | 'size') => {
    const ordered = [...members].sort((left, right) => {
      if (field === 'name') {
        const compared = (left.name ?? '').localeCompare(right.name ?? '', undefined, {
          numeric: true,
          sensitivity: 'base',
        });
        return compared || left.root_id - right.root_id;
      }
      const leftSize = BigInt(left.total_size_bytes);
      const rightSize = BigInt(right.total_size_bytes);
      if (leftSize !== rightSize) return leftSize > rightSize ? -1 : 1;
      return left.root_id - right.root_id;
    });
    void saveOrder(ordered.map((item) => item.root_id));
  }, [members, saveOrder]);

  const openMemberMenu = useCallback((
    item: CanonicalEntityGridItem,
    position: { x: number; y: number },
    selectionEnabled: boolean,
  ) => {
    const selected = selectionEnabled && selectedItemIds.has(item.root_id) ? selectedItems : [item];
    if (selectionEnabled) {
      setSelectedItemIds(new Set(selected.map((selectedItem) => selectedItem.root_id)));
      if (!selectedItemIds.has(item.root_id)) setSelectionAnchorId(item.root_id);
    }
    const openIndex = members.findIndex((member) => member.root_id === item.root_id);
    const target = { kind: 'explicit' as const, root_ids: [groupId] };
    const single = selected.length === 1 ? selected[0] : null;
    const entries = buildTileContextMenu({
      surface: selectionEnabled ? 'grid' : 'viewer',
      selectionCount: selected.length,
      querySelectionActive: false,
      aiTagEnabled: selected.every((entry) => entry.mime.startsWith('image/')),
      singleSelected: single != null,
      singleHash: single?.content_hash ?? null,
      singleItemId: single?.root_id ?? null,
      singleKind: single?.kind ?? null,
      singleName: single?.name ?? null,
      singleMime: single?.mime ?? null,
      containsGroup: false,
      scopeKind: null,
      statusFilter: null,
      loadedCount: members.length,
      onSortContents: selectionEnabled ? (field) => {
        if (field === 'name' || field === 'size') sortCollectionContents(field);
      } : undefined,
      sortFields: ['name', 'size'],
      onOpen: single ? () => setViewerIndex(openIndex) : undefined,
      onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
      onRevealInFolder: (hash) => { void filesController.revealHashInFolder(hash); },
      onOpenNewWindow: single ? () => { void windowController.openDetailWindow({
        hash: single.content_hash,
        width: single.width,
        height: single.height,
      }); } : undefined,
      onCopyFile: (hash) => { void filesController.copyFileForHash(hash); },
      onCopySelection: () => { void filesController.copyHashes(selected.map((entry) => entry.content_hash)); },
      onCopySelectionPaths: () => { void filesController.copyHashPaths(selected.map((entry) => entry.content_hash)); },
      onCopySelectionNames: () => filesController.copyText(
        selected.map((entry) => entry.name ?? 'Untitled').join('\n'),
      ),
      onCopySelectionLinks: () => { void filesController.copyHashLinks(selected.map((entry) => entry.content_hash)); },
      onCopyFilePath: (hash) => { void filesController.copyFilePath(hash); },
      onCopyName: (name) => filesController.copyText(name),
      onCopyLink: (link) => filesController.copyText(link),
      onAddToFolder: () => setFolderPortal({ open: true, target, anchor: inspectorAnchor() }),
      onOpenTagSelect: () => setTagPortal({ open: true, target, anchor: inspectorAnchor() }),
      onOpenAiTagger: () => setAiPortal({ open: true, target, anchor: inspectorAnchor() }),
      onCopyTags: () => {
        void tagsController.getById(details?.tag_ids ?? []).then((records) => {
          const tags = records.map((tag) => tagName(tag));
          filesController.copyText(JSON.stringify(tags));
          (window as Window & { __pictoClipboardTags?: string[] }).__pictoClipboardTags = tags;
        });
      },
      onPasteTags: () => {
        const tags = (window as Window & { __pictoClipboardTags?: string[] }).__pictoClipboardTags;
        if (tags?.length) void entityMutations.addTargetTags(target, tags);
      },
      hasClipboardTags: Boolean((window as Window & { __pictoClipboardTags?: string[] }).__pictoClipboardTags?.length),
      onSetRating: (rating) => { void entityMutations.setTargetRating(target, rating); },
      onExport: () => setExportModal({ open: true, fileCount: selected.length, target }),
      onExportOriginals: () => {
        void (async () => {
          const result = await (window as any).picto.dialog.open({
            properties: ['openDirectory'], multiple: false, title: 'Export originals',
          });
          const outputDir = typeof result === 'string' ? result : result?.[0];
          if (outputDir) await filesController.exportMedia(target, { output_dir: outputDir, format: 'original' });
        })();
      },
      onSearchByImage: (engine, hash) => {
        void reverseImageSearch(hash, engine).catch((reason) => showErrorNotification({
          title: 'Reverse image search failed',
          message: reason instanceof Error ? reason.message : String(reason),
        }));
      },
      onFindMediaMatches: (itemId) => {
        onClose();
        navigateToNode(`media-matches:${itemId}`);
      },
      onRegenerateThumbnails: () => { void filesController.regenerateThumbnailsBatch(selected.map((entry) => entry.content_hash)); },
      onSetLibraryCover: single ? (hash) => {
        void openCurrentLibraryCoverPicker({
          media_item_id: single.root_id,
          file_hash: hash,
          name: single.name,
          pixel_width: single.width,
          pixel_height: single.height,
          mime_type: single.mime,
        }).catch((reason) => showErrorNotification({
          title: 'Could not set library cover',
          message: reason instanceof Error ? reason.message : String(reason),
        }));
      } : undefined,
      onMoveToTrash: () => { void detachMembers(selected, 'trash'); },
    });
    const removeEntry = {
      label: selected.length > 1 ? `Remove ${selected.length} from Group` : 'Remove from Group',
      icon: <GroupRemoveIcon size={15} />,
      action: () => { void detachMembers(selected); },
    };
    const trashIndex = entries.findIndex((entry) => 'label' in entry && entry.label.startsWith('Move') && entry.label.endsWith('to Trash'));
    if (trashIndex >= 0) entries.splice(trashIndex, 0, { separator: true }, removeEntry);
    else entries.push({ separator: true }, removeEntry);
    contextMenu.openAt(position, entries);
  }, [contextMenu, details?.tag_ids, detachMembers, groupId, members, onClose, selectedItemIds, selectedItems, setAiPortal, setExportModal, setFolderPortal, setTagPortal, sortCollectionContents]);

  const confirmUngroup = useCallback(() => {
    setConfirmModal({
      open: true,
      title: 'Ungroup?',
      message: 'The media will return to the library as separate items. Files and metadata will not be deleted.',
      confirmLabel: 'Ungroup',
      onConfirm: () => {
        void ungroup(groupId)
          .then(() => announceUndoableMutation('collections.ungroup'))
          .then(onClose)
          .catch((reason) => {
            setError(reason instanceof Error ? reason.message : String(reason));
          });
      },
    });
  }, [groupId, onClose, setConfirmModal]);

  const openEmptyMenu = useCallback((position: { x: number; y: number }) => {
    contextMenu.openAt(position, [
      contentSortSubmenu((field) => {
        if (field === 'name' || field === 'size') sortCollectionContents(field);
      }, ['name', 'size']),
      { separator: true },
      {
        label: 'Ungroup...',
        icon: <GroupRemoveIcon size={15} />,
        action: confirmUngroup,
      },
    ]);
  }, [confirmUngroup, contextMenu, sortCollectionContents]);

  if (error && !details) {
    return <div className={styles.surface}><div className={`${styles.status} ${styles.error}`}>{error}</div></div>;
  }
  if (!details) return null;

  return (
    <section className={styles.surface} aria-label={details.root.name || 'Group'}>
      {error && <div className={styles.inlineError}>{error}</div>}

      {mode === 'editor' ? (
        <div className={styles.editor}>
          <div className={styles.gridHost}>
            <CanvasGrid
              items={members}
              viewMode="grid"
              targetSize={180}
              showName
              showExtension={false}
              selectedItemIds={selectedItemIds}
              onSelectionChange={(itemIds) => {
                setSelectedItemIds(itemIds);
                setSelectionAnchorId(itemIds.size === 1 ? itemIds.values().next().value ?? null : null);
              }}
              onMarqueeSelectionChange={({ itemIds }) => {
                setSelectedItemIds(itemIds);
                setSelectionAnchorId(itemIds.size === 1 ? itemIds.values().next().value ?? null : null);
              }}
              onTileClick={(_index, item, event) => {
                selectMember(item.root_id, event);
              }}
              onTileDoubleClick={(index) => setViewerIndex(index)}
              onTileContextMenu={(_index, item, position) => openMemberMenu(item, position, true)}
              onEmptyClick={deselectAll}
              onEmptyContextMenu={openEmptyMenu}
              onReorder={(orderedItemIds) => { void saveOrder(orderedItemIds); }}
            />
          </div>
        </div>
      ) : (
        <main
          className={styles.reader}
          onContextMenu={(event) => {
            if ((event.target as HTMLElement).closest('[data-group-member]')) return;
            event.preventDefault();
            openEmptyMenu({ x: event.clientX, y: event.clientY });
          }}
        >
          {members.length === 0 ? (
            <div className={styles.empty}>This group is empty.</div>
          ) : (
            <div className={styles.memberStack}>
              {members.map((item, index) => (
                <figure
                  className={styles.member}
                  data-group-member={item.root_id}
                  key={item.root_id}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    openMemberMenu(item, { x: event.clientX, y: event.clientY }, false);
                  }}
                  onDoubleClick={() => setViewerIndex(index)}
                >
                  {detailRendererKind(item.mime) !== 'image' ? (
                    <GroupDeferredRenderer
                      item={item}
                      loadFullMedia={warmMedia.loadedItemIds.has(item.root_id)}
                      onRequest={warmMedia.request}
                      onVisible={warmMedia.touch}
                    />
                  ) : (
                    <GroupImage
                      item={item}
                      loadFullMedia={warmMedia.loadedItemIds.has(item.root_id)}
                      onRequest={warmMedia.request}
                      onVisible={warmMedia.touch}
                    />
                  )}
                </figure>
              ))}
            </div>
          )}
        </main>
      )}

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}
      {viewerIndex !== null && (
        <MediaView
          items={members}
          currentIndex={viewerIndex}
          totalCount={members.length}
          backLabel="Back to group"
          recordItemId={null}
          ratingItemId={null}
          onNavigate={(delta) => setViewerIndex((index) => Math.max(
            0,
            Math.min(members.length - 1, (index ?? 0) + delta),
          ))}
          onClose={() => setViewerIndex(null)}
        />
      )}
      {quickLookIndex !== null && (
        <QuickLook
          items={members}
          currentIndex={quickLookIndex}
          metadataRootId={groupId}
          recordItemId={null}
          totalCount={members.length}
          onNavigate={(delta) => setQuickLookIndex((index) => Math.max(
            0,
            Math.min(members.length - 1, (index ?? 0) + delta),
          ))}
          onClose={() => setQuickLookIndex(null)}
        />
      )}
    </section>
  );
}
