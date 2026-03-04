import { useCallback } from 'react';
import {
  IconArrowsMaximize,
  IconArrowsSort,
  IconAdjustments,
  IconSelectAll,
  IconDeselect,
  IconTrash,
  IconExternalLink,
  IconFolderOpen,
  IconCopy,
  IconCode,
  IconTag,
  IconTags,
  IconLink,
  IconPhoto,
  IconSearch,
  IconFolderPlus,
  IconCursorText,
  IconAppWindow,
  IconCheck,
  IconX,
  IconArrowBackUp,
  IconFolderMinus,
  IconPin,
  IconPhoto as IconSetCover,
  IconFolderSymlink,
  IconRefresh,
} from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import { LayoutRow } from '../LayoutRow';
import { SortByRow } from '../SortByRow';
import { DisplayOptionsPanel } from '../DisplayOptionsPanel';
import { IconTinEye, IconSauceNAO, IconYandex, IconSogou, IconBing } from '../../ui/SearchEngineIcons';
import { prefetchMetadata } from '../metadataPrefetch';
import { FileController } from '../../../controllers/fileController';
import { FolderController } from '../../../controllers/folderController';
import { FolderPickerService } from '../../../services/folderPickerService';
import { registerUndoAction } from '../../../controllers/undoRedoController';
import { api } from '#desktop/api';
import { notifyError, notifyInfo, notifySuccess } from '../../../lib/notify';
import { useSettingsStore } from '../../../stores/settingsStore';
import { bustThumbnailCache } from '../../../lib/mediaUrl';
import { useCacheStore } from '../../../stores/cacheStore';
import { useDomainStore } from '../../../stores/domainStore';
import { SidebarController } from '../../../controllers/sidebarController';
import { SelectionController } from '../../../controllers/selectionController';
import { type ContextMenuEntry, useContextMenu } from '../../ui/ContextMenu';
import type { MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../smart-folders/types';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../runtime';
import type { LayoutItem } from '../VirtualGrid';

const GENERATED_NAME_RE = /^(?:[a-f0-9]{24,}|image[_-]?\d+|img[_-]?\d+|file[_-]?\d+)$/i;

function isGeneratedName(name: string): boolean {
  return GENERATED_NAME_RE.test(name.trim());
}

function normalizeNameBase(name: string): string {
  return name
    .trim()
    .replace(/\.[a-z0-9]{2,5}$/i, '')
    .replace(/(?:[\s._-]|\s*\(\s*)\d+\s*\)?$/g, '')
    .trim()
    .toLowerCase();
}

interface UseGridContextMenuArgs {
  scrollRef: React.RefObject<HTMLDivElement>;
  getCanvasOffsetTop: () => number;
  canvasLayoutRef: React.MutableRefObject<LayoutItem[]>;
  imagesRef: React.MutableRefObject<MasonryImageItem[]>;
  state: GridRuntimeState;
  stateRef: React.MutableRefObject<GridRuntimeState>;
  effectiveSelectedHashes: Set<string>;
  dispatch: React.Dispatch<GridRuntimeAction>;
  viewMode: GridViewMode;
  onViewModeChange?: (mode: GridViewMode) => void;
  sortField: string;
  sortOrder: string;
  onSortFieldChange?: (field: string) => void;
  onSortOrderChange?: (order: string) => void;
  smartFolderPredicate?: SmartFolderPredicate;
  smartFolderSortField?: string;
  smartFolderSortOrder?: string;
  folderId?: number | null;
  statusFilter?: string | null;
  contextMenu: ReturnType<typeof useContextMenu>;
  activateVirtualSelectAll: () => void;
  handleDeleteSelected: () => void;
  handleRestoreSelected: () => void;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
  handleInboxAction: (hash: string, status: 'active' | 'trash') => void;
  handleCopyTags: () => void;
  handlePasteTags: () => void;
  hasCopiedTags: boolean;
  collectionEntityId?: number | null;
  navigateToCollection: (collection: { id: number; name: string }) => void;
  setRenameValue: React.Dispatch<React.SetStateAction<string>>;
  setRenamingHash: React.Dispatch<React.SetStateAction<string | null>>;
  renameCancelledRef: React.MutableRefObject<boolean>;
  setBatchRenameOpen: React.Dispatch<React.SetStateAction<boolean>>;
  requestGridReload: () => void;
}

export function useGridContextMenu({
  scrollRef,
  getCanvasOffsetTop,
  canvasLayoutRef,
  imagesRef,
  state,
  stateRef,
  effectiveSelectedHashes,
  dispatch,
  viewMode,
  onViewModeChange,
  sortField,
  sortOrder,
  onSortFieldChange,
  onSortOrderChange,
  smartFolderPredicate,
  smartFolderSortField,
  smartFolderSortOrder,
  folderId,
  statusFilter,
  contextMenu,
  activateVirtualSelectAll,
  handleDeleteSelected,
  handleRestoreSelected,
  handleRemoveFromFolder,
  handleRemoveFromCollection,
  handleInboxAction,
  handleCopyTags,
  handlePasteTags,
  hasCopiedTags,
  collectionEntityId,
  navigateToCollection,
  setRenameValue,
  setRenamingHash,
  renameCancelledRef,
  setBatchRenameOpen,
  requestGridReload,
}: UseGridContextMenuArgs) {
  return useCallback((e: React.MouseEvent) => {
    const contextPoint = { x: e.clientX, y: e.clientY };
    const target = e.target as HTMLElement;
    if (target.closest('[data-subfolder-grid]')) {
      e.preventDefault();
      return;
    }
    // Hit-test right-click position against layout positions (canvas — no DOM tiles)
    let rightClickedHash: string | null = null;
    const container = scrollRef.current;
    if (container) {
      const containerRect = container.getBoundingClientRect();
      const canvasOffsetTop = getCanvasOffsetTop();
      const mx = e.clientX - containerRect.left + container.scrollLeft;
      const my = e.clientY - containerRect.top + container.scrollTop - canvasOffsetTop;
      const positions = canvasLayoutRef.current;
      const imgs = imagesRef.current;
      for (let i = 0; i < positions.length && i < imgs.length; i++) {
        const pos = positions[i];
        if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
          rightClickedHash = imgs[i].hash;
          break;
        }
      }
      if (rightClickedHash && !effectiveSelectedHashes.has(rightClickedHash)) {
        dispatch({ type: 'SELECT_HASHES', hashes: new Set([rightClickedHash]) });
        dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
        dispatch({ type: 'SET_LAST_CLICKED', hash: rightClickedHash });
        prefetchMetadata(rightClickedHash);
      }
    }

    const isMac = navigator.platform.includes('Mac');
    // After right-click hit-test, compute selection state accounting for the
    // just-applied selection (state updates are async, so selectedHashes is stale).
    const wasAlreadySelected = rightClickedHash && effectiveSelectedHashes.has(rightClickedHash);
    const effectiveSize = rightClickedHash && !wasAlreadySelected ? 1 : state.selectedHashes.size;
    const effectiveVirtual = rightClickedHash && !wasAlreadySelected ? null : state.virtualAllSelection;
    const hasSingleSelection = !effectiveVirtual && effectiveSize === 1;
    const hasSelection = !!effectiveVirtual || effectiveSize > 0 || !!rightClickedHash;
    const singleHash = hasSingleSelection
      ? (rightClickedHash && !wasAlreadySelected ? rightClickedHash : [...state.selectedHashes][0])
      : rightClickedHash;
    const singleImage = singleHash ? state.images.find((img) => img.hash === singleHash) : null;
    const singleIsCollection = singleImage?.is_collection === true;
    const singleCollectionId = singleImage?.entity_id ?? null;

    const activeSortField = smartFolderPredicate ? (smartFolderSortField ?? 'imported_at') : sortField;
    const activeSortOrder = smartFolderPredicate ? (smartFolderSortOrder ?? 'desc') : sortOrder;

    const items: ContextMenuEntry[] = [];

    // Open / View section (only when a single image is contextually available)
    if (singleHash) {
      items.push({
        type: 'item',
        label: 'Open',
        icon: <IconArrowsMaximize />,
        shortcut: 'Enter',
        onClick: () => {
          dispatch({ type: 'OPEN_DETAIL', hash: singleHash });
        },
      });
      if (singleIsCollection && singleCollectionId != null) {
        items.push({
          type: 'item',
          label: 'Edit Collection',
          icon: <IconFolderOpen />,
          onClick: () => {
            navigateToCollection({
              id: singleCollectionId,
              name: singleImage?.name ?? `Collection ${singleCollectionId}`,
            });
          },
        });
      }
      if (!singleIsCollection) {
        items.push({
          type: 'item',
          label: 'Open With Default App',
          icon: <IconExternalLink />,
          shortcut: isMac ? '\u21E7Enter' : 'Shift+Enter',
          onClick: () => FileController.openDefault(singleHash).catch(err => {
            notifyError(err, 'Open Failed');
          }),
        });
      }
      items.push({
        type: 'item',
        label: isMac ? 'Reveal in Finder' : 'Reveal in Explorer',
        icon: <IconFolderOpen />,
        shortcut: isMac ? '\u2318Enter' : 'Ctrl+Enter',
        disabled: singleIsCollection,
        onClick: () => FileController.revealInFolder(singleHash).catch(err => {
          notifyError(err, 'Reveal Failed');
        }),
      });
      items.push({
        type: 'item',
        label: 'Open in New Window',
        icon: <IconAppWindow />,
        shortcut: isMac ? '\u2318O' : 'Ctrl+O',
        disabled: singleIsCollection,
        onClick: async () => {
          const img = stateRef.current.images.find(i => i.hash === singleHash);
          FileController.openInNewWindow(singleHash, img?.width, img?.height).catch(err => {
            notifyError(err, 'New Window Failed');
          });
        },
      });
      items.push({ type: 'separator' });
    }

    if (hasSelection && !effectiveVirtual) {
      items.push({
        type: 'item',
        label: 'Create Collection from Selection',
        icon: <IconFolderPlus />,
        onClick: async () => {
          const selectedHashSet = (() => {
            if (rightClickedHash && !wasAlreadySelected) return new Set([rightClickedHash]);
            return new Set(stateRef.current.selectedHashes);
          })();
          const selectedImages = stateRef.current.images.filter((img) => selectedHashSet.has(img.hash));
          const memberHashes = selectedImages
            .filter((img) => !img.is_collection)
            .map((img) => img.hash);
          if (memberHashes.length === 0) {
            notifyInfo('No single-image items selected', 'Collections');
            return;
          }
          const now = new Date();
          const fallbackName = `Collection ${now.toLocaleDateString()} ${now.toLocaleTimeString()}`;
          const memberNames = selectedImages
            .filter((img) => !img.is_collection)
            .map((img) => (img.name ?? '').trim())
            .filter((n) => n.length > 0);
          const allGenerated = memberNames.length > 0 && memberNames.every(isGeneratedName);
          const normalizedBases = memberNames.map(normalizeNameBase).filter(Boolean);
          const uniqueBases = new Set(normalizedBases);
          const sharedBaseName = uniqueBases.size === 1 && normalizedBases.length > 0
            ? memberNames.find((n) => normalizeNameBase(n) === normalizedBases[0]) ?? fallbackName
            : null;

          let collectionName = fallbackName;
          if (sharedBaseName) {
            collectionName = sharedBaseName;
          } else if (!allGenerated && memberNames.length > 0) {
            const suggested = memberNames[0];
            const entered = window.prompt('Collection name:', suggested);
            if (entered == null) return;
            const trimmed = entered.trim();
            if (!trimmed) return;
            collectionName = trimmed;
          }
          try {
            const id = await api.collections.create({ name: collectionName.trim() });
            const added = await api.collections.addMembers({ id, hashes: memberHashes });
            notifySuccess(`Created collection with ${added} item${added === 1 ? '' : 's'}`, 'Collections');
            requestGridReload();
            navigateToCollection({ id, name: collectionName.trim() });
          } catch (err) {
            notifyError(err, 'Create Collection Failed');
          }
        },
      });
      if (singleIsCollection) {
        items.push({
          type: 'item',
          label: 'Split Collection',
          icon: <IconFolderSymlink />,
          disabled: singleCollectionId == null,
          onClick: async () => {
            if (singleCollectionId == null) return;
            try {
              await api.collections.delete(singleCollectionId);
              notifySuccess('Collection split', 'Collections');
              requestGridReload();
            } catch (err) {
              notifyError(err, 'Split Collection Failed');
            }
          },
        });
      }
      items.push({ type: 'separator' });
    }

    // Pin / Set as Cover (not yet implemented)
    if (singleHash && folderId) {
      items.push({ type: 'item', label: 'Pin to Top', icon: <IconPin />, disabled: true, onClick: () => {} });
      items.push({ type: 'item', label: 'Set as Folder Cover', icon: <IconSetCover />, disabled: true, onClick: () => {} });
      items.push({ type: 'separator' });
    }

    // Inbox accept/reject
    if (statusFilter === 'inbox' && singleHash) {
      items.push({
        type: 'item',
        label: 'Accept',
        icon: <IconCheck />,
        onClick: () => handleInboxAction(singleHash, 'active'),
      });
      items.push({
        type: 'item',
        label: 'Reject',
        icon: <IconX />,
        onClick: () => handleInboxAction(singleHash, 'trash'),
      });
      items.push({ type: 'separator' });
    }

    // Add to Folder
    if (hasSelection) {
      items.push({
        type: 'item',
        label: 'Add to Folder...',
        icon: <IconFolderPlus />,
        shortcut: isMac ? '\u2318\u21E7J' : 'Ctrl+Shift+J',
        onClick: () => {
          const anchor = document.querySelector('[data-grid-container]') as HTMLElement ?? document.body;
          FolderPickerService.open({
            anchorEl: anchor,
            anchorPoint: contextPoint,
            selectedFolderIds: [],
            onToggle: (fId, _name, added) => {
              if (!added) return;
              const s = stateRef.current;
              const hashes = s.virtualAllSelection
                ? s.images.filter(i => !s.virtualAllSelection!.excludedHashes.has(i.hash)).map(i => i.hash)
                : [...s.selectedHashes];
              FolderController.addFilesToFolderBatch(fId, hashes)
                .then(() => {
                  notifySuccess(`${hashes.length} file(s) added to folder`, 'Added');
                })
                .catch(err => notifyError(err, 'Add Failed'));
            },
          });
        },
      });
      items.push({ type: 'item', label: 'New Folder with Selection', icon: <IconFolderSymlink />, disabled: true, onClick: () => {} });
      items.push({ type: 'separator' });
    }

    // Rename
    if (singleHash) {
      items.push({
        type: 'item',
        label: 'Rename',
        icon: <IconCursorText />,
        shortcut: isMac ? '\u2318R' : 'Ctrl+R',
        onClick: () => {
          // Ensure hash is selected then start inline rename
          dispatch({ type: 'SELECT_HASHES', hashes: new Set([singleHash]) });
          const img = stateRef.current.images.find(i => i.hash === singleHash);
          if (renameCancelledRef.current != null) {
            renameCancelledRef.current = false;
          }
          setRenameValue(img?.name ?? '');
          setRenamingHash(singleHash);
        },
      });
    }

    // Batch Rename (multi-selection)
    if (effectiveVirtual || effectiveSize > 1) {
      items.push({
        type: 'item',
        label: 'Batch Rename...',
        icon: <IconCursorText />,
        shortcut: isMac ? '⌘⇧R' : 'Ctrl+Shift+R',
        onClick: () => setBatchRenameOpen(true),
      });
    }

    // Copy (file to clipboard)
    if (singleHash) {
      items.push({
        type: 'item',
        label: 'Copy',
        icon: <IconCopy />,
        shortcut: isMac ? '\u2318C' : 'Ctrl+C',
        onClick: () => {
          FileController.copyToClipboard(singleHash)
            .then(() => notifySuccess('File copied to clipboard', 'Copied'))
            .catch(err => notifyError(err, 'Copy Failed'));
        },
      });
    }

    if (singleHash) {
      items.push({
        type: 'item',
        label: 'Copy File Path',
        icon: <IconCode />,
        shortcut: isMac ? '\u2318\u2325C' : 'Ctrl+Alt+C',
        onClick: async () => {
          try {
            const path = await FileController.resolveFilePath(singleHash);
            await navigator.clipboard.writeText(path);
            notifySuccess('File path copied to clipboard', 'Copied');
          } catch (err) {
            notifyError(err, 'Copy Failed');
          }
        },
      });

      // Copy submenu
      items.push({
        type: 'submenu',
        label: 'Copy...',
        icon: <IconCopy />,
        children: [
          {
            type: 'item',
            label: 'Copy Name',
            icon: <IconCursorText />,
            onClick: async () => {
              const name = singleImage?.name ?? singleHash;
              await navigator.clipboard.writeText(name);
              notifySuccess('Name copied to clipboard', 'Copied');
            },
          },
          {
            type: 'item',
            label: 'Copy as Link',
            icon: <IconLink />,
            onClick: async () => {
              await navigator.clipboard.writeText(`picto://file/${singleHash}`);
              notifySuccess('Link copied', 'Copied');
            },
          },
          {
            type: 'item',
            label: 'Copy Thumbnail',
            icon: <IconPhoto />,
            onClick: () => {
              FileController.copyThumbnailToClipboard(singleHash)
                .then(() => notifySuccess('Thumbnail copied to clipboard', 'Copied'))
                .catch(err => notifyError(err, 'Copy Failed'));
            },
          },
        ],
      });
    }

    if (hasSelection) {
      items.push({
        type: 'item',
        label: 'Copy Tags',
        icon: <IconTag />,
        shortcut: isMac ? '\u2318\u21E7C' : 'Ctrl+Shift+C',
        onClick: () => handleCopyTags(),
      });
      items.push({
        type: 'item',
        label: 'Paste Tags',
        icon: <IconTags />,
        shortcut: isMac ? '\u2318\u21E7V' : 'Ctrl+Shift+V',
        disabled: !hasCopiedTags,
        onClick: () => handlePasteTags(),
      });
    }

    // Search by Image submenu — only show engines the user has enabled
    if (singleHash) {
      const { enabledSearchEngines } = useSettingsStore.getState().settings;
      const engineDefs: { key: typeof enabledSearchEngines[number]; label: string; icon: React.ReactNode }[] = [
        { key: 'tineye', label: 'TinEye', icon: <IconTinEye /> },
        { key: 'saucenao', label: 'SauceNAO', icon: <IconSauceNAO /> },
        { key: 'yandex', label: 'Yandex Images', icon: <IconYandex /> },
        { key: 'sogou', label: 'Sogou', icon: <IconSogou /> },
        { key: 'bing', label: 'Bing Visual Search', icon: <IconBing /> },
      ];
      const children: ContextMenuEntry[] = engineDefs
        .filter(e => enabledSearchEngines.includes(e.key))
        .map(e => ({
          type: 'item' as const,
          label: e.label,
          icon: e.icon,
          onClick: () => {
            notifications.show({ title: 'Searching...', message: `Uploading image to ${e.label}`, autoClose: 3000, loading: true });
            FileController.searchByImage(singleHash, e.key)
              .catch(err => notifyError(err, 'Search Failed'));
          },
        }));
      if (children.length > 0) {
        items.push({ type: 'separator' });
        items.push({
          type: 'submenu',
          label: 'Search by Image',
          icon: <IconSearch />,
          children,
        });
      }
    }

    if (folderId) {
      if (items.length > 0) items.push({ type: 'separator' });
      items.push({
        type: 'item',
        label: 'New Subfolder',
        icon: <IconFolderPlus />,
        onClick: () => {
          void FolderController.createFolder({ name: 'New Folder', parentId: folderId })
            .then(() => {
              notifySuccess('Subfolder created', 'Folders');
            })
            .catch((err) => notifyError(err, 'Create Subfolder Failed'));
        },
      });
    }

    // New Folder from Selection
    if (hasSelection) {
      items.push({
        type: 'item',
        label: 'New Folder from Selection',
        icon: <IconFolderSymlink />,
        onClick: async () => {
          const hashes = effectiveVirtual
            ? state.images.filter(i => !effectiveVirtual!.excludedHashes.has(i.hash)).map(i => i.hash)
            : [...state.selectedHashes];
          if (hashes.length === 0) return;
          try {
            const folder = await FolderController.createFolder({ name: 'New Folder' });
            await FolderController.addFilesToFolderBatch(folder.folder_id, hashes);
            notifySuccess(`Created folder with ${hashes.length} file(s)`, 'Folder Created');
          } catch (err) {
            notifyError(err, 'Create Folder Failed');
          }
        },
      });
    }

    // Regenerate Thumbnail(s)
    if (hasSelection) {
      // Compute correct hashes accounting for right-click-on-unselected
      const regenHashes: string[] = effectiveVirtual
        ? state.images.filter(i => !effectiveVirtual!.excludedHashes.has(i.hash)).map(i => i.hash)
        : effectiveSize === 1 && singleHash
          ? [singleHash]
          : [...state.selectedHashes];
      if (regenHashes.length > 0) {
        items.push({ type: 'separator' });
        items.push({
          type: 'item',
          label: regenHashes.length === 1 ? 'Regenerate Thumbnail' : `Regenerate Thumbnails (${regenHashes.length})`,
          icon: <IconRefresh />,
          shortcut: isMac ? '\u2318\u21E7T' : 'Ctrl+Shift+T',
          onClick: () => {
            notifications.show({ title: 'Regenerating...', message: `Regenerating ${regenHashes.length} thumbnail(s)`, autoClose: 3000, loading: true });
            FileController.regenerateThumbnailsBatch(regenHashes)
              .then(r => {
                notifySuccess(`Regenerated ${r.regenerated} thumbnail(s)`, 'Thumbnails');
                bustThumbnailCache(regenHashes);
                useCacheStore.getState().bumpGridRefresh();
              })
              .catch(err => notifyError(err, 'Regenerate Failed'));
          },
        });
      }
    }

    if (items.length > 0) items.push({ type: 'separator' });

    // Layout & Sort rows
    items.push({
      type: 'custom',
      key: 'layout',
      render: () => (
        <LayoutRow viewMode={viewMode} onChange={(m) => onViewModeChange?.(m)} />
      ),
    });
    // Sort rows
    if (folderId) {
      // Folder view: one-time sort actions that rearrange position_rank
      const reloadGrid = () => {
        useCacheStore.getState().invalidateAll();
        useCacheStore.getState().bumpGridRefresh();
      };
      const sortAndReload = (sortBy: string, dir: string) =>
        FolderController.sortFolderItems(folderId, sortBy, dir).then(reloadGrid);
      const reverseAndReload = (hashes?: string[]) =>
        FolderController.reverseFolderItems(folderId, hashes).then(reloadGrid);
      items.push({
        type: 'submenu',
        label: 'Sort by',
        icon: <IconArrowsSort size={16} />,
        children: [
          { type: 'item', label: 'Name A\u2192Z', onClick: () => sortAndReload('name', 'asc') },
          { type: 'item', label: 'Name Z\u2192A', onClick: () => sortAndReload('name', 'desc') },
          { type: 'separator' },
          { type: 'item', label: 'Date Newest First', onClick: () => sortAndReload('imported_at', 'desc') },
          { type: 'item', label: 'Date Oldest First', onClick: () => sortAndReload('imported_at', 'asc') },
          { type: 'separator' },
          { type: 'item', label: 'Size Largest First', onClick: () => sortAndReload('size', 'desc') },
          { type: 'item', label: 'Size Smallest First', onClick: () => sortAndReload('size', 'asc') },
          { type: 'separator' },
          { type: 'item', label: 'Rating', onClick: () => sortAndReload('rating', 'desc') },
          { type: 'item', label: 'Type', onClick: () => sortAndReload('mime', 'asc') },
          { type: 'separator' },
          { type: 'item', label: 'Reverse Order', onClick: () => reverseAndReload() },
          {
            type: 'item',
            label: 'Reverse Selected',
            disabled: effectiveSelectedHashes.size === 0,
            onClick: () => {
              const hashes = [...effectiveSelectedHashes];
              if (hashes.length > 0) reverseAndReload(hashes);
            },
          },
        ],
      });
    } else {
      items.push({
        type: 'custom',
        key: 'sortby',
        render: () => (
          <SortByRow
            field={activeSortField}
            order={activeSortOrder}
            onFieldChange={(f) => onSortFieldChange?.(f)}
            onOrderChange={(o) => onSortOrderChange?.(o)}
          />
        ),
      });
    }

    // Display options submenu
    items.push({
      type: 'submenu',
      label: 'Display',
      icon: <IconAdjustments size={16} />,
      children: [
        {
          type: 'custom',
          key: 'display-panel',
          render: () => <DisplayOptionsPanel />,
        },
      ],
    });

    items.push({ type: 'separator' });

    // Selection
    items.push({
      type: 'item',
      label: 'Select All',
      icon: <IconSelectAll />,
      shortcut: isMac ? '\u2318A' : 'Ctrl+A',
      onClick: () => activateVirtualSelectAll(),
    });
    if (hasSelection) {
      items.push({
        type: 'item',
        label: 'Deselect All',
        icon: <IconDeselect />,
        shortcut: 'Esc',
        onClick: () => { dispatch({ type: 'CLEAR_SELECTION' }); },
      });
    }

    // Remove from folder
    if (hasSelection && folderId) {
      items.push({ type: 'separator' });
      const selCount = effectiveVirtual
        ? (state.virtualAllSelectedCount ?? effectiveSize)
        : effectiveSize;
      const freshHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;
      items.push({
        type: 'item',
        label: `Remove ${selCount > 1 ? `${selCount} Images` : 'Image'} from Folder`,
        icon: <IconFolderMinus size={16} />,
        shortcut: isMac ? '\u2318\u21E7\u232B' : 'Ctrl+Shift+Del',
        onClick: () => {
          if (freshHash && folderId) {
            dispatch({ type: 'FILTER_IMAGES', predicate: i => i.hash !== freshHash });
            dispatch({ type: 'CLEAR_SELECTION' });
            FolderController.removeFilesFromFolderBatch(folderId, [freshHash])
              .then(() => {
                registerUndoAction({
                  label: 'Remove from folder',
                  undo: async () => {
                    await FolderController.addFilesToFolderBatch(folderId, [freshHash]);
                    requestGridReload();
                  },
                  redo: async () => {
                    await FolderController.removeFilesFromFolderBatch(folderId, [freshHash]);
                    requestGridReload();
                  },
                });
              })
              .catch(err => notifyError(err, 'Remove from Folder Failed'));
          } else {
            handleRemoveFromFolder();
          }
        },
      });
    }

    // Remove from collection
    if (hasSelection && collectionEntityId) {
      items.push({ type: 'separator' });
      const freshHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;
      items.push({
        type: 'item',
        label: 'Remove from Collection',
        icon: <IconFolderMinus size={16} />,
        shortcut: isMac ? '\u2318\u21E7\u232B' : 'Ctrl+Shift+Del',
        onClick: () => {
          if (freshHash && collectionEntityId) {
            dispatch({ type: 'FILTER_IMAGES', predicate: i => i.hash !== freshHash });
            dispatch({ type: 'CLEAR_SELECTION' });
            api.collections.removeMembers({ id: collectionEntityId, hashes: [freshHash] })
              .then(() => {
                registerUndoAction({
                  label: 'Remove from collection',
                  undo: async () => {
                    await api.collections.addMembers({ id: collectionEntityId, hashes: [freshHash] });
                    requestGridReload();
                  },
                  redo: async () => {
                    await api.collections.removeMembers({ id: collectionEntityId, hashes: [freshHash] });
                    requestGridReload();
                  },
                });
              })
              .catch(err => notifyError(err, 'Remove from Collection Failed'));
          } else {
            handleRemoveFromCollection();
          }
        },
      });
    }

    // Restore / Delete
    if (hasSelection) {
      items.push({ type: 'separator' });
      const count = effectiveSize;
      const virtualCount = effectiveVirtual ? state.virtualAllSelectedCount : null;
      const inTrash = statusFilter === 'trash';
      const refreshAfterLifecycleMutation = () => {
        SelectionController.invalidateSummary();
        void useDomainStore.getState().fetchSidebarTree();
        SidebarController.requestRefresh();
        useCacheStore.getState().invalidateAll();
        useCacheStore.getState().bumpGridRefresh();
        requestGridReload();
      };

      // When right-click selected a single new image, the handlers have stale
      // state. Capture the hash here so onClick operates on the correct target.
      const freshSingleHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;

      const doRestore = () => {
        if (freshSingleHash) {
          dispatch({ type: 'FILTER_IMAGES', predicate: i => i.hash !== freshSingleHash });
          dispatch({ type: 'CLEAR_SELECTION' });
          api.file.setStatus(freshSingleHash, 'active')
            .then(() => {
              registerUndoAction({
                label: 'Restore image',
                undo: async () => {
                  await api.file.setStatus(freshSingleHash, 'trash');
                  requestGridReload();
                },
                redo: async () => {
                  await api.file.setStatus(freshSingleHash, 'active');
                  requestGridReload();
                },
              });
              refreshAfterLifecycleMutation();
            })
            .catch(err => notifyError(err, 'Restore Failed'));
        } else {
          handleRestoreSelected();
        }
      };

      const doDelete = () => {
        if (freshSingleHash) {
          dispatch({ type: 'FILTER_IMAGES', predicate: i => i.hash !== freshSingleHash });
          dispatch({ type: 'CLEAR_SELECTION' });
          if (inTrash) {
            api.file.delete(freshSingleHash)
              .then(() => {
                refreshAfterLifecycleMutation();
              })
              .catch(err => notifyError(err, 'Delete Failed'));
          } else {
            const previousStatus = imagesRef.current.find((img) => img.hash === freshSingleHash)?.status ?? (statusFilter ?? 'active');
            api.file.setStatus(freshSingleHash, 'trash')
              .then(() => {
                registerUndoAction({
                  label: 'Move image to trash',
                  undo: async () => {
                    await api.file.setStatus(freshSingleHash, previousStatus);
                    requestGridReload();
                  },
                  redo: async () => {
                    await api.file.setStatus(freshSingleHash, 'trash');
                    requestGridReload();
                  },
                });
                refreshAfterLifecycleMutation();
              })
              .catch(err => notifyError(err, 'Delete Failed'));
          }
        } else {
          handleDeleteSelected();
        }
      };

      if (inTrash) {
        items.push({
          type: 'item',
          label: effectiveVirtual
            ? (virtualCount != null
              ? `Restore ${virtualCount.toLocaleString()} Image${virtualCount === 1 ? '' : 's'}`
              : 'Restore All Matching Images')
            : `Restore ${count} Image${count > 1 ? 's' : ''}`,
          icon: <IconArrowBackUp />,
          onClick: doRestore,
        });
      }
      items.push({
        type: 'item',
        label: inTrash
          ? (effectiveVirtual
            ? (virtualCount != null
              ? `Permanently Delete ${virtualCount.toLocaleString()} Image${virtualCount === 1 ? '' : 's'}`
              : 'Permanently Delete All')
            : `Permanently Delete ${count} Image${count > 1 ? 's' : ''}`)
          : (effectiveVirtual
            ? (virtualCount != null
              ? `Move ${virtualCount.toLocaleString()} Image${virtualCount === 1 ? '' : 's'} to Trash`
              : 'Move All Matching Images to Trash')
            : `Move ${count} Image${count > 1 ? 's' : ''} to Trash`),
        icon: <IconTrash />,
        shortcut: isMac ? '\u2318\u232B' : 'Del',
        danger: inTrash,
        onClick: doDelete,
      });
    }

    contextMenu.open(e, items);
  }, [
    viewMode,
    onViewModeChange,
    sortField,
    sortOrder,
    onSortFieldChange,
    onSortOrderChange,
    smartFolderPredicate,
    smartFolderSortField,
    smartFolderSortOrder,
    state.selectedHashes,
    state.virtualAllSelection,
    state.virtualAllSelectedCount,
    effectiveSelectedHashes,
    handleDeleteSelected,
    handleRestoreSelected,
    handleRemoveFromFolder,
    handleInboxAction,
    statusFilter,
    state.images,
    contextMenu,
    activateVirtualSelectAll,
    handleCopyTags,
    handlePasteTags,
    folderId,
    getCanvasOffsetTop,
    dispatch,
    navigateToCollection,
    requestGridReload,
    hasCopiedTags,
    scrollRef,
    canvasLayoutRef,
    imagesRef,
    stateRef,
    setRenameValue,
    setRenamingHash,
    renameCancelledRef,
  ]);
}
