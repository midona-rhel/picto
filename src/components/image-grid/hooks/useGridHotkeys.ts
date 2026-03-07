import { useEffect, useRef } from 'react';
import { useHotkeys } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import { notifyError, notifySuccess } from '../../../shared/lib/notify';
import { FolderController } from '../../../controllers/folderController';
import { FolderPickerService } from '../../../shared/services/folderPickerService';
import { FileController } from '../../../shared/controllers/fileController';
import { bustThumbnailCache } from '../../../shared/lib/mediaUrl';
import { useCacheStore } from '../../../state/cacheStore';
import { useSettingsStore, type AppSettings } from '../../../state/settingsStore';
import { getShortcut, matchesShortcutDef } from '../../../shared/lib/shortcuts';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../runtime';
import type { DetailViewControls, DetailViewState } from '../DetailView';

let lastUsedFolder: { id: number; name: string } | null = null;

interface UseGridHotkeysArgs {
  stateRef: { current: GridRuntimeState };
  dispatch: React.Dispatch<GridRuntimeAction>;
  onDetailViewStateChange?: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  activateVirtualSelectAll: () => void;
  handleOpenWithDefaultApp: () => void;
  handleRevealInFolder: () => void;
  handleOpenInNewWindow: () => void;
  handleDeleteSelected: () => void;
  handleCopyFilePath: () => void;
  handleCopyTags: () => void;
  handlePasteTags: () => void;
  onViewModeChange?: (mode: GridViewMode) => void;
  updateSetting: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  grayscalePreview: boolean;
  setSlideshowOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setBatchRenameOpen: React.Dispatch<React.SetStateAction<boolean>>;
  startInlineRename: () => void;
  folderId?: number | null;
  collectionEntityId?: number | null;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
  handleGridNavigation: (key: string, shiftKey: boolean) => void;
  handleRateSelected: (rating: number) => void;
  handleOpenQuickLook: () => void;
  handleOpenDetail: () => void;
  statusFilter?: string | null;
  handleInboxAction?: (hash: string, status: 'active' | 'trash') => void;
}

export function useGridHotkeys({
  stateRef,
  dispatch,
  onDetailViewStateChange,
  activateVirtualSelectAll,
  handleOpenWithDefaultApp,
  handleRevealInFolder,
  handleOpenInNewWindow,
  handleDeleteSelected,
  handleCopyFilePath,
  handleCopyTags,
  handlePasteTags,
  onViewModeChange,
  updateSetting,
  grayscalePreview,
  setSlideshowOpen,
  setBatchRenameOpen,
  startInlineRename,
  folderId,
  collectionEntityId,
  handleRemoveFromFolder,
  handleRemoveFromCollection,
  handleGridNavigation,
  handleRateSelected,
  handleOpenQuickLook,
  handleOpenDetail,
  statusFilter,
  handleInboxAction,
}: UseGridHotkeysArgs): void {
  const handleGridNavigationRef = useRef(handleGridNavigation);
  handleGridNavigationRef.current = handleGridNavigation;

  const handleRenameRef = useRef(startInlineRename);
  handleRenameRef.current = startInlineRename;

  useHotkeys([
    ['mod+f', () => console.log('Focus search')],
    ['mod+a', () => activateVirtualSelectAll()],
    ['shift+Enter', handleOpenWithDefaultApp],
    ['mod+Enter', handleRevealInFolder],
    ['mod+o', handleOpenInNewWindow],
    [
      'escape',
      () => {
        if (stateRef.current.detailHash) {
          dispatch({ type: 'CLOSE_DETAIL' });
          onDetailViewStateChange?.(null, null);
          return;
        }
        if (stateRef.current.quickLookHash) {
          dispatch({ type: 'CLOSE_QUICK_LOOK' });
          return;
        }
        dispatch({ type: 'CLEAR_SELECTION' });
      },
    ],
    ['mod+Backspace', () => handleDeleteSelected()],
    ['Delete', () => handleDeleteSelected()],
    ['mod+alt+c', () => handleCopyFilePath()],
    ['mod+shift+c', () => handleCopyTags()],
    ['mod+shift+v', () => handlePasteTags()],
    ['alt+1', () => onViewModeChange?.('grid')],
    ['alt+2', () => onViewModeChange?.('waterfall')],
    ['alt+3', () => onViewModeChange?.('justified')],
    ['mod+alt+g', () => updateSetting('grayscalePreview', !grayscalePreview)],
    ['mod+alt+8', () => updateSetting('showMinimap', !useSettingsStore.getState().settings.showMinimap)],
    [
      'F5',
      () => {
        if (stateRef.current.images.length > 0) setSlideshowOpen(true);
      },
    ],
    ['mod+r', () => handleRenameRef.current()],
    [
      'mod+shift+r',
      () => {
        if (stateRef.current.selectedHashes.size > 1 || stateRef.current.virtualAllSelection) {
          setBatchRenameOpen(true);
        }
      },
    ],
    [
      'mod+shift+n',
      () => {
        FolderController.createFolder({ name: 'New Folder' })
          
          .catch((err) => notifyError(err, 'Create Folder Failed'));
      },
    ],
    [
      'alt+n',
      () => {
        if (!folderId) return;
        FolderController.createFolder({ name: 'New Folder', parentId: folderId })
          
          .catch((err) => notifyError(err, 'Create Subfolder Failed'));
      },
    ],
    ['mod+shift+backspace', () => {
      if (collectionEntityId) handleRemoveFromCollection();
      else handleRemoveFromFolder();
    }],
    [
      'mod+shift+j',
      () => {
        const anchor = (document.querySelector('[data-grid-container]') as HTMLElement) ?? document.body;
        const s = stateRef.current;
        FolderPickerService.open({
          anchorEl: anchor,
          selectedFolderIds: [],
          onToggle: (fId, name, added) => {
            if (!added) return;
            lastUsedFolder = { id: fId, name };
            const hashes = s.virtualAllSelection
              ? s.images
                  .filter((i) => !s.virtualAllSelection!.excludedHashes.has(i.hash))
                  .map((i) => i.hash)
              : [...s.selectedHashes];
            FolderController.addFilesToFolderBatch(fId, hashes)
              .then(() => {
                notifySuccess(`${hashes.length} file(s) added to folder`, 'Added');
              })
              .catch((err) => notifyError(err, 'Add to Folder Failed'));
          },
        });
      },
    ],
    [
      'shift+d',
      () => {
        if (!lastUsedFolder) return;
        const s = stateRef.current;
        const hashes = s.virtualAllSelection
          ? s.images
              .filter((i) => !s.virtualAllSelection!.excludedHashes.has(i.hash))
              .map((i) => i.hash)
          : [...s.selectedHashes];
        if (hashes.length === 0) return;
        FolderController.addFilesToFolderBatch(lastUsedFolder.id, hashes)
          .then(() => {
            notifySuccess(`${hashes.length} file(s) added to "${lastUsedFolder!.name}"`, 'Added');
          })
          .catch((err) => notifyError(err, 'Add to Folder Failed'));
      },
    ],
    [
      'mod+shift+t',
      () => {
        const s = stateRef.current;
        const hashes = s.virtualAllSelection
          ? s.images
              .filter((i) => !s.virtualAllSelection!.excludedHashes.has(i.hash))
              .map((i) => i.hash)
          : [...s.selectedHashes];
        if (hashes.length === 0) return;
        notifications.show({
          title: 'Regenerating...',
          message: `Regenerating ${hashes.length} thumbnail(s)`,
          autoClose: 3000,
          loading: true,
        });
        FileController.regenerateThumbnailsBatch(hashes)
          .then((r) => {
            notifySuccess(`Regenerated ${r.regenerated} thumbnail(s)`, 'Thumbnails');
            bustThumbnailCache(hashes);
            useCacheStore.getState().bumpGridRefresh();
          })
          .catch((err) => notifyError(err, 'Regenerate Failed'));
      },
    ],
  ]);

  const handleRateSelectedRef = useRef(handleRateSelected);
  handleRateSelectedRef.current = handleRateSelected;
  const handleOpenQuickLookRef = useRef(handleOpenQuickLook);
  handleOpenQuickLookRef.current = handleOpenQuickLook;
  const handleOpenDetailRef = useRef(handleOpenDetail);
  handleOpenDetailRef.current = handleOpenDetail;
  const statusFilterRef = useRef(statusFilter);
  statusFilterRef.current = statusFilter;
  const handleInboxActionRef = useRef(handleInboxAction);
  handleInboxActionRef.current = handleInboxAction;
  const detailHashRef = useRef(stateRef.current.detailHash);
  detailHashRef.current = stateRef.current.detailHash;
  const quickLookHashRef = useRef(stateRef.current.quickLookHash);
  quickLookHashRef.current = stateRef.current.quickLookHash;
  useEffect(() => {
    const handleNativeKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (detailHashRef.current || quickLookHashRef.current) return;

      const wasdMap: Record<string, string> = {
        w: 'ArrowUp',
        a: 'ArrowLeft',
        s: 'ArrowDown',
        d: 'ArrowRight',
      };
      const mappedKey = wasdMap[e.key] ?? e.key;
      const isArrow =
        mappedKey === 'ArrowLeft' ||
        mappedKey === 'ArrowRight' ||
        mappedKey === 'ArrowUp' ||
        mappedKey === 'ArrowDown';
      const isHomeEnd = mappedKey === 'Home' || mappedKey === 'End';
      const isPageUpDown = mappedKey === 'PageUp' || mappedKey === 'PageDown';
      if (isArrow || isHomeEnd || isPageUpDown) {
        if (e.metaKey || e.ctrlKey || e.altKey) return;
        e.preventDefault();
        handleGridNavigationRef.current(mappedKey, e.shiftKey);
        return;
      }

      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        const digit = parseInt(e.key, 10);
        if (digit >= 0 && digit <= 5) {
          e.preventDefault();
          handleRateSelectedRef.current(digit);
          return;
        }
      }

      // Inbox reject from grid — Backspace rejects selected image(s)
      if (statusFilterRef.current === 'inbox' && handleInboxActionRef.current) {
        const rejectDef = getShortcut('inbox.reject');
        if (rejectDef && matchesShortcutDef(e, rejectDef)) {
          const s = stateRef.current;
          const hashes = s.virtualAllSelection
            ? s.images.filter((i) => !s.virtualAllSelection!.excludedHashes.has(i.hash)).map((i) => i.hash)
            : [...s.selectedHashes];
          if (hashes.length > 0) {
            e.preventDefault();
            for (const hash of hashes) handleInboxActionRef.current!(hash, 'trash');
          }
          return;
        }
      }

      if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      if (e.key === ' ' || e.code === 'Space') {
        e.preventDefault();
        handleOpenQuickLookRef.current();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        handleOpenDetailRef.current();
      }
    };
    window.addEventListener('keydown', handleNativeKey);
    return () => window.removeEventListener('keydown', handleNativeKey);
  }, []);
}
