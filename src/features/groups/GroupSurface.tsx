import { useCallback, useEffect, useMemo, useState, type SyntheticEvent } from 'react';
import { useSetAtom } from 'jotai';
import { viewerController } from '../../controllers/viewerController';
import { MediaView } from '../viewer/MediaView';
import { QuickLook } from '../viewer/QuickLook';
import { DetailMediaRenderer } from '../viewer/document/DetailMediaRenderer';
import { detailRendererKind } from '../viewer/document/detailRendererKind';
import { CanvasGrid } from '../grid/canvas/CanvasGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import type { MediaDetails } from '../../shared/types/generated/application/MediaDetails';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { detachItems, reorderGroup, ungroup } from '../../platform/entityApi';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import { batchRenameModalAtom, confirmModalAtom, exportModalAtom } from '../../state/modals';
import { aiTaggerPortalAtom, folderPickerPortalAtom, inspectorAnchor, tagSelectPortalAtom } from '../../state/portals';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { buildTileContextMenu } from '../grid/gridContextMenu';
import styles from './GroupSurface.module.css';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { GroupRemoveIcon, SelectAllIcon } from '../../shared/ui/icons/group-icons';
import * as entityMutations from '../../controllers/entityMutations';
import { openCurrentLibraryCoverPicker } from '../library/libraryAppearance';
import { showErrorNotification } from '../../shared/lib/notifications';

export interface GroupSurfaceProps {
  groupId: number;
  initialMode?: 'reader' | 'editor';
  presentation?: 'detail' | 'quicklook';
  rootCurrentIndex: number;
  rootTotal: number;
  onNavigateRoot: (delta: number) => void;
  onClose: () => void;
}

function memberToGridItem(details: ItemDetails, media: MediaDetails): CanonicalEntityGridItem {
  return {
    item_id: media.media_item_id,
    kind: 'media',
    lifecycle: details.lifecycle,
    name: media.name,
    display_file_hash: media.file_hash,
    display_mime_type: media.mime_type,
    pixel_width: media.pixel_width,
    pixel_height: media.pixel_height,
    duration_ms: media.duration_ms,
    frame_count: media.frame_count,
    dominant_color_hex: media.dominant_color_hex,
    rating: media.rating,
    media_count: 1,
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

function GroupImage({ item }: { item: CanonicalEntityGridItem }) {
  const [fullVisible, setFullVisible] = useState(false);
  const aspectRatio = item.pixel_width && item.pixel_height
    ? `${item.pixel_width} / ${item.pixel_height}`
    : undefined;

  return (
    <div className={styles.imageFrame} style={{ aspectRatio }}>
      <ThumbnailImage
        className={styles.thumbnailImage}
        src={mediaThumbnailUrl(item.display_file_hash)}
        alt=""
        draggable={false}
      />
      <img
        className={`${styles.fullImage} ${fullVisible ? styles.fullImageVisible : ''}`}
        src={mediaFileUrl(item.display_file_hash, item.display_mime_type)}
        alt={item.name ?? ''}
        loading="lazy"
        decoding="async"
        onLoad={(event: SyntheticEvent<HTMLImageElement>) => {
          const reveal = () => setFullVisible(true);
          if (typeof event.currentTarget.decode === 'function') {
            event.currentTarget.decode().then(reveal).catch(reveal);
          } else reveal();
        }}
      />
    </div>
  );
}

export function GroupSurface({
  groupId,
  initialMode = 'reader',
  presentation = 'detail',
  rootCurrentIndex,
  rootTotal,
  onNavigateRoot,
  onClose,
}: GroupSurfaceProps) {
  const [details, setDetails] = useState<ItemDetails | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState(initialMode);
  const [selectedItemIds, setSelectedItemIds] = useState<Set<number>>(new Set());
  const [selectionAnchorId, setSelectionAnchorId] = useState<number | null>(null);
  const [viewerIndex, setViewerIndex] = useState<number | null>(null);
  const [quickLookIndex, setQuickLookIndex] = useState<number | null>(null);
  const contextMenu = useContextMenu();
  const setDisplayState = useSetAtom(viewerDisplayStateAtom);
  const setDisplayControls = useSetAtom(viewerDisplayControlsAtom);
  const setConfirmModal = useSetAtom(confirmModalAtom);
  const setTagPortal = useSetAtom(tagSelectPortalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);
  const setAiPortal = useSetAtom(aiTaggerPortalAtom);
  const setExportModal = useSetAtom(exportModalAtom);
  const setBatchRenameModal = useSetAtom(batchRenameModalAtom);
  const members = useMemo(
    () => details?.media.map((media) => memberToGridItem(details, media)) ?? [],
    [details],
  );
  const mediaByItemId = useMemo(
    () => new Map(details?.media.map((media) => [media.media_item_id, media]) ?? []),
    [details],
  );

  const refresh = useCallback(async () => {
    try {
      const next = await viewerController.getItemDetails(groupId);
      if (next.kind !== 'collection') {
        onClose();
        return;
      }
      setDetails(next);
      setSelectedItemIds((current) => {
        const memberIds = new Set(next.media.map((media) => media.media_item_id));
        return new Set([...current].filter((itemId) => memberIds.has(itemId)));
      });
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [groupId, onClose]);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(
    () => libraryInvalidation.register(`item:${groupId}`, () => { void refresh(); }),
    [groupId, refresh],
  );

  useEffect(() => {
    if (!details || viewerIndex !== null || presentation === 'quicklook') return;
    setDisplayState({ currentIndex: rootCurrentIndex, total: rootTotal });
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
  }, [details, initialMode, mode, onClose, onNavigateRoot, presentation, rootCurrentIndex, rootTotal, setDisplayControls, setDisplayState, viewerIndex]);

  const selectedItems = useMemo(
    () => members.filter((item) => selectedItemIds.has(item.item_id)),
    [members, selectedItemIds],
  );

  useShortcutScope((event) => {
      const closeDef = getShortcut('view.closeDetail')!;
      const detailDef = getShortcut('view.detailView')!;
      const quickLookDef = getShortcut('view.quicklook')!;
      const selectAllDef = getShortcut('edit.selectAll')!;
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
        setQuickLookIndex(members.findIndex((item) => item.item_id === selectedItems[0].item_id));
        return true;
      }
      if (mode === 'editor' && matchesShortcutDef(event, selectAllDef) && members.length > 0) {
        event.preventDefault();
        setSelectedItemIds(new Set(members.map((item) => item.item_id)));
        setSelectionAnchorId(members[0]?.item_id ?? null);
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
  const memberIds = useMemo(() => members.map((item) => item.item_id), [members]);

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

  const selectAll = useCallback(() => {
    setSelectedItemIds(new Set(memberIds));
    setSelectionAnchorId(memberIds[0] ?? null);
  }, [memberIds]);

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
        media_item_ids: selected.map((item) => item.item_id),
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

  const openMemberMenu = useCallback((
    item: CanonicalEntityGridItem,
    position: { x: number; y: number },
    selectionEnabled: boolean,
  ) => {
    const selected = selectionEnabled && selectedItemIds.has(item.item_id) ? selectedItems : [item];
    if (selectionEnabled) {
      setSelectedItemIds(new Set(selected.map((selectedItem) => selectedItem.item_id)));
      if (!selectedItemIds.has(item.item_id)) setSelectionAnchorId(item.item_id);
    }
    const openIndex = members.findIndex((member) => member.item_id === item.item_id);
    const target = { kind: 'explicit' as const, item_ids: selected.map((entry) => entry.item_id) };
    const single = selected.length === 1 ? selected[0] : null;
    const copiedTags = selected.length === 0 ? [] : selected
      .map((entry) => new Set(mediaByItemId.get(entry.item_id)?.tags ?? []))
      .reduce((shared, tags) => new Set([...shared].filter((tag) => tags.has(tag))));
    const entries = buildTileContextMenu({
      surface: selectionEnabled ? 'grid' : 'viewer',
      selectionCount: selected.length,
      querySelectionActive: false,
      aiTagEnabled: selected.every((entry) => entry.display_mime_type.startsWith('image/')),
      singleSelected: single != null,
      singleHash: single?.display_file_hash ?? null,
      singleKind: single?.kind ?? null,
      singleName: single?.name ?? null,
      singleMime: single?.display_mime_type ?? null,
      containsGroup: false,
      scopeKind: null,
      statusFilter: null,
      loadedCount: members.length,
      onSelectAll: selectAll,
      onDeselectAll: deselectAll,
      onOpen: single ? () => setViewerIndex(openIndex) : undefined,
      onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
      onRevealInFolder: (hash) => { void filesController.revealHashInFolder(hash); },
      onOpenNewWindow: single ? (hash) => { void windowController.openDetailWindow({
        hash,
        width: single.pixel_width,
        height: single.pixel_height,
      }); } : undefined,
      onCopyFile: (hash) => { void filesController.copyFileForHash(hash); },
      onCopyFilePath: (hash) => { void filesController.copyFilePath(hash); },
      onCopyName: (name) => filesController.copyText(name),
      onCopyLink: (link) => filesController.copyText(link),
      onAddToFolder: () => setFolderPortal({ open: true, target, anchor: inspectorAnchor() }),
      onOpenTagSelect: () => setTagPortal({ open: true, target, anchor: inspectorAnchor() }),
      onOpenAiTagger: () => setAiPortal({ open: true, target, anchor: inspectorAnchor() }),
      onCopyTags: () => {
        const tags = [...copiedTags];
        filesController.copyText(JSON.stringify(tags));
        (window as Window & { __pictoClipboardTags?: string[] }).__pictoClipboardTags = tags;
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
      onBatchRename: selected.length > 1 ? () => setBatchRenameModal({
        open: true,
        items: selected.map((entry) => ({ item_id: entry.item_id, name: entry.name ?? 'Untitled' })),
      }) : undefined,
      onSearchByImage: (engine, hash) => {
        const bases: Record<string, string> = {
          tineye: 'https://tineye.com/search/?url=',
          saucenao: 'https://saucenao.com/search.php?url=',
          yandex: 'https://yandex.com/images/search?rpt=imageview&url=',
          bing: 'https://www.bing.com/images/search?view=detailv2&iss=sbi&form=SBIVSP&sbisrc=UrlPaste&q=imgurl:',
        };
        if (bases[engine]) void (window as any).picto?.shell?.openExternal(`${bases[engine]}${encodeURIComponent(mediaThumbnailUrl(hash))}`);
      },
      onRegenerateThumbnails: () => { void filesController.regenerateThumbnailsBatch(selected.map((entry) => entry.display_file_hash)); },
      onSetLibraryCover: single ? (hash) => {
        void openCurrentLibraryCoverPicker({
          media_item_id: single.item_id,
          file_hash: hash,
          name: single.name,
          pixel_width: single.pixel_width,
          pixel_height: single.pixel_height,
          mime_type: single.display_mime_type,
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
  }, [contextMenu, deselectAll, detachMembers, mediaByItemId, members, selectAll, selectedItemIds, selectedItems, setAiPortal, setBatchRenameModal, setExportModal, setFolderPortal, setTagPortal]);

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
      {
        label: 'Select All',
        icon: <SelectAllIcon size={15} />,
        action: selectAll,
        disabled: members.length === 0,
      },
      { separator: true },
      {
        label: 'Ungroup...',
        icon: <GroupRemoveIcon size={15} />,
        action: confirmUngroup,
      },
    ]);
  }, [confirmUngroup, contextMenu, members.length, selectAll]);

  const saveOrder = useCallback(async (orderedItemIds: number[]) => {
    try {
      await reorderGroup({ collection_id: groupId, media_item_ids: orderedItemIds });
      await announceUndoableMutation('collections.reorder');
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [groupId, refresh]);

  if (loading && !details) {
    return <div className={styles.surface}><div className={styles.status}>Loading group...</div></div>;
  }
  if (error && !details) {
    return <div className={styles.surface}><div className={`${styles.status} ${styles.error}`}>{error}</div></div>;
  }
  if (!details) return null;

  return (
    <section className={styles.surface} aria-label={details.label ?? 'Group'}>
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
                selectMember(item.item_id, event);
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
                  data-group-member={item.item_id}
                  key={item.item_id}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    openMemberMenu(item, { x: event.clientX, y: event.clientY }, false);
                  }}
                  onDoubleClick={() => setViewerIndex(index)}
                >
                  {detailRendererKind(item.display_mime_type) !== 'image' ? (
                    <div
                      className={styles.mediaFrame}
                      style={{ aspectRatio: item.pixel_width && item.pixel_height
                        ? `${item.pixel_width} / ${item.pixel_height}`
                        : undefined }}
                    >
                      <DetailMediaRenderer
                        hash={item.display_file_hash}
                        mimeType={item.display_mime_type}
                        displayName={item.name}
                        mediaKeyboardShortcutsEnabled={false}
                        mediaAutoPlay={false}
                        mediaLoop={false}
                        mediaMuted={false}
                      />
                    </div>
                  ) : (
                    <GroupImage item={item} />
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
          recordItemId={groupId}
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
