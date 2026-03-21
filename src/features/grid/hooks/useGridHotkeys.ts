import { useEffect, useRef } from 'react';
import { useHotkeys } from '@mantine/hooks';
import { notifyInfo } from '../../../shared/lib/notify';
import { api } from '#desktop/api';
import { notifyError, notifySuccess } from '../../../shared/lib/notify';
import { FolderPickerService } from '../../../shared/services/folderPickerService';
import { bustThumbnailCache } from '../../../shared/lib/mediaUrl';
import type { AppSettings } from '../../../state/settingsStore';
import { useNavigationStore } from '../../../state/navigationStore';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../runtime';

let lastUsedFolder: { id: number; name: string } | null = null;

interface UseGridHotkeysArgs {
  stateRef: { current: GridRuntimeState };
  dispatch: React.Dispatch<GridRuntimeAction>;
  activateVirtualSelectAll: () => void;
  handleOpenWithDefaultApp: () => void;
  handleRevealInFolder: () => void;
  handleOpenInNewWindow: () => void;
  handleBasicExport: () => void | Promise<void>;
  handleAdvancedExport: () => void | Promise<void>;
  handleDeleteSelected: () => void;
  handleCopyFilePath: () => void;
  handleCopyTags: () => void;
  handlePasteTags: () => void;
  onViewModeChange?: (mode: GridViewMode) => void;
  updateSetting: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  grayscalePreview: boolean;
  openSlideshow: (hash?: string | null) => void;
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
  viewerOpen: boolean;
  closeViewer: (exitHash?: string) => void;
  statusFilter?: string | null;
}

export function useGridHotkeys({
  stateRef,
  dispatch,
  activateVirtualSelectAll,
  handleOpenWithDefaultApp,
  handleRevealInFolder,
  handleOpenInNewWindow,
  handleBasicExport,
  handleAdvancedExport,
  handleDeleteSelected,
  handleCopyFilePath,
  handleCopyTags,
  handlePasteTags,
  onViewModeChange,
  updateSetting,
  grayscalePreview,
  openSlideshow,
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
  viewerOpen,
  closeViewer,
  statusFilter: _statusFilter,
}: UseGridHotkeysArgs): void {
  const handleGridNavigationRef = useRef(handleGridNavigation);
  handleGridNavigationRef.current = handleGridNavigation;

  const handleRenameRef = useRef(startInlineRename);
  handleRenameRef.current = startInlineRename;

  useHotkeys([
    ['mod+a', () => activateVirtualSelectAll()],
    ['shift+Enter', handleOpenWithDefaultApp],
    ['mod+Enter', handleRevealInFolder],
    ['mod+o', handleOpenInNewWindow],
    ['mod+e', () => { void handleBasicExport(); }],
    ['mod+shift+e', () => { void handleAdvancedExport(); }],
    [
      'escape',
      () => {
        if (viewerOpen) {
          closeViewer();
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
    [
      'F5',
      () => {
        if (stateRef.current.images.length > 0) {
          const selectedHash = stateRef.current.selectedHashes.size === 1
            ? [...stateRef.current.selectedHashes][0]
            : null;
          openSlideshow(selectedHash);
        }
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
        api.folders.create({ name: 'New Folder' })

          .catch((err) => notifyError(err, 'Create Folder Failed'));
      },
    ],
    [
      'alt+n',
      () => {
        if (!folderId) return;
        api.folders.create({ name: 'New Folder', parent_id: folderId })

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
            api.folders.addFiles(fId, hashes)
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
        api.folders.addFiles(lastUsedFolder.id, hashes)
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
        notifyInfo(`Regenerating ${hashes.length} thumbnail(s)`);
        api.file.regenerateThumbnailsBatch(hashes)
          .then((r) => {
            notifySuccess(`Regenerated ${r.regenerated} thumbnail(s)`, 'Thumbnails');
            bustThumbnailCache(hashes);
          })
          .catch((err) => notifyError(err, 'Regenerate Failed'));
      },
    ],
    [
      'mod+shift+s',
      () => {
        const s = stateRef.current;
        const hashes = [...s.selectedHashes];
        if (hashes.length !== 1) return;
        const hash = hashes[0];
        const img = s.images.find((i) => i.hash === hash);
        if (!img || img.is_collection) return;
        api.duplicates.findSimilar(hash)
          .then((result) => {
            if (result.items.length === 0) {
              notifyInfo('No visually similar images found');
              return;
            }
            useNavigationStore.getState().navigateToSimilar(hash, result.items.map((i: { hash: string }) => i.hash));
          })
          .catch((err) => notifyError(err, 'Find Similar Failed'));
      },
    ],
  ]);

  const handleRateSelectedRef = useRef(handleRateSelected);
  handleRateSelectedRef.current = handleRateSelected;
  const handleOpenQuickLookRef = useRef(handleOpenQuickLook);
  handleOpenQuickLookRef.current = handleOpenQuickLook;
  const handleOpenDetailRef = useRef(handleOpenDetail);
  handleOpenDetailRef.current = handleOpenDetail;
  const viewerOpenRef = useRef(viewerOpen);
  viewerOpenRef.current = viewerOpen;
  useEffect(() => {
    const handleNativeKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (viewerOpenRef.current) return;

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
  }, [openSlideshow, closeViewer]);
}
