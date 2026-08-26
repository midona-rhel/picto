import { useCallback, useRef, type MouseEvent } from 'react';
import { useSetAtom } from 'jotai';
import { IconPhoto, IconPlayerPause, IconPlayerPlay, IconPlayerStop, IconVolume, IconVolumeOff } from '@tabler/icons-react';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { viewerController } from '../../controllers/viewerController';
import * as entityMutations from '../../controllers/entityMutations';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildTileContextMenu } from '../grid/gridContextMenu';
import { aiTaggerPortalAtom, folderPickerPortalAtom, inspectorAnchor, tagSelectPortalAtom } from '../../state/portals';
import { confirmModalAtom, exportModalAtom } from '../../state/modals';
import type { ItemKind } from '../../shared/types/generated/application/ItemKind';
import type { Lifecycle } from '../../shared/types/generated/application/Lifecycle';
import type { ItemTarget } from '../../shared/types/generated/application/ItemTarget';
import type { FlashPlaybackController } from './document/FlashPlayer';
import type { CurrentFrameCapture } from './currentFrameCapture';
import { openCurrentLibraryCoverPicker } from '../library/libraryAppearance';
import { showErrorNotification } from '../../shared/lib/notifications';
import { reverseImageSearch } from '../../platform/shellApi';

interface ViewerEntityContextMenuOptions {
  hash: string | null;
  itemId?: number | null;
  kind?: ItemKind | null;
  lifecycle?: Lifecycle | null;
  name?: string | null;
  mime?: string | null;
  width?: number | null;
  height?: number | null;
  flashPlayback?: FlashPlaybackController | null;
  captureCurrentFrame?: CurrentFrameCapture | null;
}

export function buildFlashPlaybackContextEntries(playback: FlashPlaybackController) {
  return [
    {
      label: playback.isPlaying ? 'Pause' : 'Play',
      icon: playback.isPlaying ? <IconPlayerPause size={15} /> : <IconPlayerPlay size={15} />,
      action: playback.togglePlay,
    },
    { label: 'Stop', icon: <IconPlayerStop size={15} />, action: playback.stop },
    {
      label: playback.muted ? 'Unmute' : 'Mute',
      icon: playback.muted ? <IconVolume size={15} /> : <IconVolumeOff size={15} />,
      action: playback.toggleMute,
    },
  ];
}

function waitForContextMenuPaintRemoval(): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = window.setTimeout(() => {
      observer.disconnect();
      reject(new Error('The context menu did not leave the capture surface.'));
    }, 1_000);
    const afterUnmount = () => {
      observer.disconnect();
      requestAnimationFrame(() => requestAnimationFrame(() => {
        window.clearTimeout(deadline);
        resolve();
      }));
    };
    const observer = new MutationObserver(() => {
      if (!document.querySelector('[role="menu"]')) afterUnmount();
    });
    if (!document.querySelector('[role="menu"]')) {
      afterUnmount();
      return;
    }
    observer.observe(document.body, { childList: true, subtree: true });
  });
}

export function useViewerEntityContextMenu({
  hash,
  itemId,
  kind,
  lifecycle,
  name,
  mime,
  width,
  height,
  flashPlayback,
  captureCurrentFrame,
}: ViewerEntityContextMenuOptions) {
  const contextMenu = useContextMenu();
  const setTagPortal = useSetAtom(tagSelectPortalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);
  const setAiPortal = useSetAtom(aiTaggerPortalAtom);
  const setExportModal = useSetAtom(exportModalAtom);
  const setConfirmModal = useSetAtom(confirmModalAtom);
  const captureInFlightRef = useRef(false);
  const captureThumbnail = useCallback(async () => {
    if (!hash || !captureCurrentFrame || captureInFlightRef.current) return;
    captureInFlightRef.current = true;
    contextMenu.close();
    try {
      await waitForContextMenuPaintRemoval();
      const png = await captureCurrentFrame();
      await filesController.setThumbnail(hash, png);
    } finally {
      captureInFlightRef.current = false;
    }
  }, [captureCurrentFrame, contextMenu, hash]);

  const open = useCallback((event: MouseEvent) => {
    if (!hash) return;
    const target: ItemTarget | null = itemId == null ? null : { kind: 'explicit', item_ids: [itemId] };
    const menuKind = kind ?? 'media';
    const canAutoTag = menuKind === 'media' && Boolean(mime?.startsWith('image/'));
    const commonEntries = buildTileContextMenu({
      surface: 'viewer',
      selectionCount: target ? 1 : 0,
      querySelectionActive: false,
      aiTagEnabled: canAutoTag,
      singleSelected: true,
      singleHash: hash,
      singleKind: menuKind,
      containsGroup: menuKind === 'collection',
      scopeKind: null,
      statusFilter: lifecycle ?? null,
      loadedCount: 1,
      onSelectAll: () => {},
      onDeselectAll: () => {},
      onOpenDefault: (fileHash) => { void filesController.openDefaultAppForHash(fileHash); },
      onRevealInFolder: (fileHash) => { void filesController.revealHashInFolder(fileHash); },
      onOpenNewWindow: () => {
        if (menuKind === 'collection' && itemId != null) {
          void windowController.openDetailWindow({ item_id: itemId });
        } else {
          void windowController.openDetailWindow({ hash, width, height });
        }
      },
      onCopyFile: (fileHash) => { void filesController.copyFileForHash(fileHash); },
      onCopyFilePath: (fileHash) => { void filesController.copyFilePath(fileHash); },
      onCopyName: (value) => filesController.copyText(value),
      singleName: name,
      singleMime: mime,
      onCopyLink: (link) => filesController.copyText(link),
      onAddToFolder: target ? () => setFolderPortal({ open: true, target, anchor: inspectorAnchor() }) : undefined,
      onOpenTagSelect: target ? () => setTagPortal({ open: true, target, anchor: inspectorAnchor() }) : undefined,
      onOpenAiTagger: target && canAutoTag
        ? () => setAiPortal({ open: true, target, anchor: inspectorAnchor() })
        : undefined,
      onCopyTags: target ? () => {
        void viewerController.getItemDetails(itemId!).then((details) => {
          filesController.copyText(JSON.stringify(details.aggregate_tags));
          (window as any).__pictoClipboardTags = details.aggregate_tags;
        });
      } : undefined,
      onPasteTags: target ? () => {
        const tags = (window as any).__pictoClipboardTags as string[] | undefined;
        if (tags?.length) void entityMutations.addTargetTags(target, tags);
      } : undefined,
      hasClipboardTags: Boolean((window as any).__pictoClipboardTags?.length),
      onSetRating: target ? (rating) => { void entityMutations.setTargetRating(target, rating); } : undefined,
      onExport: target ? () => setExportModal({ open: true, fileCount: 1, target }) : undefined,
      onSearchByImage: menuKind === 'media' ? (engine, fileHash) => {
        void reverseImageSearch(fileHash, engine).catch((reason) => showErrorNotification({
          title: 'Reverse image search failed',
          message: reason instanceof Error ? reason.message : String(reason),
        }));
      } : undefined,
      onRegenerateThumbnails: menuKind === 'media'
        ? () => { void filesController.regenerateThumbnailsBatch([hash]); }
        : undefined,
      onSetLibraryCover: menuKind === 'media'
        ? (fileHash) => {
            void openCurrentLibraryCoverPicker({
              media_item_id: itemId ?? -1,
              file_hash: fileHash,
              name: name ?? null,
              pixel_width: width ?? null,
              pixel_height: height ?? null,
              mime_type: mime ?? null,
            }).catch((reason) => showErrorNotification({
              title: 'Could not set library cover',
              message: reason instanceof Error ? reason.message : String(reason),
            }));
          }
        : undefined,
      onMoveToTrash: target && lifecycle !== 'trash'
        ? () => { void entityMutations.setTargetLifecycle(target, 'trash'); }
        : undefined,
      onRestore: target && lifecycle === 'trash'
        ? () => { void entityMutations.setTargetLifecycle(target, 'active'); }
        : undefined,
      onPermanentDelete: target && lifecycle === 'trash' ? () => setConfirmModal({
        open: true,
        title: 'Delete Permanently?',
        message: 'This item and any unreferenced files will be deleted. This cannot be undone.',
        confirmLabel: 'Delete Permanently',
        danger: true,
        onConfirm: () => { void entityMutations.permanentlyDeleteTarget(target); },
      }) : undefined,
    });
    const playbackEntries = flashPlayback ? buildFlashPlaybackContextEntries(flashPlayback) : [];
    const thumbnailEntries = captureCurrentFrame ? [{
      label: 'Set as Thumbnail',
      icon: <IconPhoto size={15} />,
      action: () => { void captureThumbnail().catch(() => {}); },
    }] : [];
    const localEntries = [...playbackEntries, ...thumbnailEntries];
    const entries = [
      ...localEntries,
      ...(localEntries.length ? [{ separator: true as const }] : []),
      ...commonEntries,
    ];
    contextMenu.open(event, entries, { showSearch: false });
  }, [captureCurrentFrame, captureThumbnail, contextMenu, flashPlayback, hash, height, itemId, kind, lifecycle, mime, name, setAiPortal, setConfirmModal, setExportModal, setFolderPortal, setTagPortal, width]);

  const menu = contextMenu.state ? (
    <ContextMenu
      entries={contextMenu.state.entries}
      position={contextMenu.state.position}
      onClose={contextMenu.close}
      showSearch={contextMenu.state.showSearch}
    />
  ) : null;

  return { open, menu };
}
