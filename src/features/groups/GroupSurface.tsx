import { useCallback, useEffect, useMemo, useState, type SyntheticEvent } from 'react';
import {
  IconClipboardCopy,
  IconCopy,
  IconLink,
  IconPhoto,
  IconRefresh,
  IconTrash,
} from '@tabler/icons-react';
import { useSetAtom } from 'jotai';
import { viewerController } from '../../controllers/viewerController';
import { MediaView } from '../viewer/MediaView';
import { QuickLook } from '../viewer/QuickLook';
import { DetailMediaRenderer } from '../viewer/document/DetailMediaRenderer';
import { detailRendererKind } from '../viewer/document/detailRendererKind';
import { CanvasGrid } from '../grid/canvas/CanvasGrid';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import type { MediaDetails } from '../../shared/types/generated/application/MediaDetails';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { detachItems, reorderGroup, ungroup } from '../../platform/entityApi';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import { confirmModalAtom } from '../../state/modals';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { buildEntityOpenContextEntries } from '../grid/gridContextMenu';
import styles from './GroupSurface.module.css';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { DeselectAllIcon, GroupRemoveIcon, SelectAllIcon } from '../../shared/ui/icons/group-icons';

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

function memberMenu(
  selected: CanonicalEntityGridItem[],
  totalCount: number,
  onOpen: () => void,
  selectionEnabled: boolean,
  onSelectAll: () => void,
  onDeselectAll: () => void,
  onCopyTags: (() => void) | null,
  onRemove: () => void,
  onTrash: () => void,
): MenuEntry[] {
  const count = selected.length;
  const single = count === 1 ? selected[0] : null;
  return [
    ...(single ? [
      {
        label: 'Open',
        icon: <IconPhoto size={15} />,
        action: onOpen,
      },
      ...buildEntityOpenContextEntries({
        hash: single.display_file_hash,
        onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
        onRevealInFolder: (hash) => { void filesController.revealHashInFolder(hash); },
        onOpenNewWindow: (hash) => { void windowController.openDetailWindow({
          hash,
          width: single.pixel_width,
          height: single.pixel_height,
        }); },
      }),
      { separator: true } as const,
      {
        label: 'Copy',
        icon: <IconCopy size={15} />,
        action: () => { void filesController.copyFileForHash(single.display_file_hash); },
      },
      {
        label: 'Copy File Path',
        icon: <IconClipboardCopy size={15} />,
        action: () => { void filesController.copyFilePath(single.display_file_hash); },
      },
      ...(single.name ? [{
        label: 'Copy Name',
        icon: <IconClipboardCopy size={15} />,
        action: () => filesController.copyText(single.name!),
      }] : []),
      {
        label: 'Copy as Link',
        icon: <IconLink size={15} />,
        action: () => filesController.copyText(mediaFileUrl(single.display_file_hash, single.display_mime_type)),
      },
      ...(onCopyTags ? [{
        label: 'Copy Tags',
        icon: <IconClipboardCopy size={15} />,
        action: onCopyTags,
      }] : []),
      { separator: true } as const,
    ] : []),
    {
      label: count > 1 ? `Regenerate ${count} Thumbnails` : 'Regenerate Thumbnail',
      icon: <IconRefresh size={15} />,
      action: () => { void filesController.regenerateThumbnailsBatch(selected.map((item) => item.display_file_hash)); },
    },
    { separator: true },
    ...(selectionEnabled ? [
      {
        label: 'Select All',
        icon: <SelectAllIcon size={15} />,
        action: onSelectAll,
        disabled: totalCount === 0 || count === totalCount,
      },
      {
        label: 'Deselect All',
        icon: <DeselectAllIcon size={15} />,
        action: onDeselectAll,
        disabled: count === 0,
      },
      { separator: true } as const,
    ] : []),
    {
      label: count > 1 ? `Remove ${count} from Group` : 'Remove from Group',
      icon: <GroupRemoveIcon size={15} />,
      action: onRemove,
    },
    {
      label: count > 1 ? `Move ${count} to Trash` : 'Move to Trash',
      icon: <IconTrash size={15} />,
      action: onTrash,
    },
  ];
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
      if (mode === 'editor' && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a' && members.length > 0) {
        setSelectedItemIds(new Set(members.map((item) => item.item_id)));
        setSelectionAnchorId(members[0]?.item_id ?? null);
        return true;
      }
      if (mode === 'editor' && selectedItems.length > 0 && (event.key === 'Backspace' || event.key === 'Delete')) {
        void detachMembers(selectedItems);
        return true;
      }
      if (mode === 'reader' && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        onNavigateRoot(event.key === 'ArrowLeft' ? -1 : 1);
        return true;
      }
      if (event.key !== 'Escape') return;
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
    contextMenu.openAt(position, memberMenu(
      selected,
      members.length,
      () => setViewerIndex(openIndex),
      selectionEnabled,
      selectAll,
      deselectAll,
      selected.length === 1
        ? () => {
            const tags = mediaByItemId.get(selected[0].item_id)?.tags ?? [];
            filesController.copyText(JSON.stringify(tags));
            (window as Window & { __pictoClipboardTags?: string[] }).__pictoClipboardTags = tags;
          }
        : null,
      () => { void detachMembers(selected); },
      () => { void detachMembers(selected, 'trash'); },
    ));
  }, [contextMenu, deselectAll, detachMembers, mediaByItemId, members, selectAll, selectedItemIds, selectedItems]);

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
