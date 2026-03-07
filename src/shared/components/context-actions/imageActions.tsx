import {
  IconAdjustments,
  IconAppWindow,
  IconArrowBackUp,
  IconArrowsMaximize,
  IconArrowsSort,
  IconCheck,
  IconCode,
  IconCopy,
  IconCursorText,
  IconDeselect,
  IconExternalLink,
  IconFolderMinus,
  IconFolderOpen,
  IconFolderPlus,
  IconFolderSymlink,
  IconLink,
  IconPhoto,
  IconPin,
  IconPhoto as IconSetCover,
  IconRefresh,
  IconSearch,
  IconSelectAll,
  IconTag,
  IconTags,
  IconTrash,
  IconX,
} from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import type { Dispatch, MutableRefObject, ReactNode, SetStateAction } from 'react';
import type { ContextMenuEntry } from '../ContextMenu';
import { LayoutRow } from '../../../components/image-grid/LayoutRow';
import { SortByRow } from '../../../components/image-grid/SortByRow';
import { DisplayOptionsPanel } from '../../../components/image-grid/DisplayOptionsPanel';
import { IconBing, IconSauceNAO, IconSogou, IconTinEye, IconYandex } from '../SearchEngineIcons';
import type { SmartFolderPredicate } from '../../../components/smart-folders/types';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../../../components/image-grid/runtime';
import { FileController } from '../../controllers/fileController';
import { FolderController } from '../../../controllers/folderController';
import { FolderPickerService } from '../../../shared/services/folderPickerService';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { notifyError, notifySuccess } from '../../../shared/lib/notify';
import { useSettingsStore } from '../../../state/settingsStore';
import { applyGridMutationEffects } from '../../../domain/actions/mutationEffects';
import { deleteHashesWithLifecycleEffects, setFileStatusWithLifecycleEffects } from '../../../domain/actions/fileLifecycleActions';
import type { MasonryImageItem } from '../../../components/image-grid/shared';
import { api } from '#desktop/api';
import { bustThumbnailCache } from '../../../shared/lib/mediaUrl';
import { useCacheStore } from '../../../state/cacheStore';

interface BuildGridImageContextMenuArgs {
  contextPoint: { x: number; y: number };
  isMac: boolean;
  state: GridRuntimeState;
  stateRef: MutableRefObject<GridRuntimeState>;
  imagesRef: MutableRefObject<MasonryImageItem[]>;
  dispatch: Dispatch<GridRuntimeAction>;
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
  effectiveSelectedHashes: Set<string>;
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
  setRenameValue: Dispatch<SetStateAction<string>>;
  setRenamingHash: Dispatch<SetStateAction<string | null>>;
  renameCancelledRef: MutableRefObject<boolean>;
  setBatchRenameOpen: Dispatch<SetStateAction<boolean>>;
  requestGridReload: () => void;
  rightClickedHash: string | null;
  wasAlreadySelected: boolean;
  hasSelection: boolean;
  singleHash: string | null;
  singleImage: MasonryImageItem | null;
  singleIsCollection: boolean;
  singleCollectionId: number | null;
  effectiveVirtual: GridRuntimeState['virtualAllSelection'] | null;
  effectiveSize: number;
}

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

export function buildGridImageContextMenu(args: BuildGridImageContextMenuArgs): ContextMenuEntry[] {
  const {
    contextPoint,
    isMac,
    state,
    stateRef,
    imagesRef,
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
    effectiveSelectedHashes,
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
    rightClickedHash,
    wasAlreadySelected,
    hasSelection,
    singleHash,
    singleImage,
    singleIsCollection,
    singleCollectionId,
    effectiveVirtual,
    effectiveSize,
  } = args;

  const activeSortField = smartFolderPredicate ? (smartFolderSortField ?? 'imported_at') : sortField;
  const activeSortOrder = smartFolderPredicate ? (smartFolderSortOrder ?? 'desc') : sortOrder;
  const items: ContextMenuEntry[] = [];

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
        if (memberHashes.length === 0) return;
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

  if (singleHash && folderId) {
    items.push({ type: 'item', label: 'Pin to Top', icon: <IconPin />, disabled: true, onClick: () => {} });
    items.push({ type: 'item', label: 'Set as Folder Cover', icon: <IconSetCover />, disabled: true, onClick: () => {} });
    items.push({ type: 'separator' });
  }

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

  if (singleHash) {
    items.push({
      type: 'item',
      label: 'Rename',
      icon: <IconCursorText />,
      shortcut: isMac ? '\u2318R' : 'Ctrl+R',
      onClick: () => {
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
  if (effectiveVirtual || effectiveSize > 1) {
    items.push({
      type: 'item',
      label: 'Batch Rename...',
      icon: <IconCursorText />,
      shortcut: isMac ? '⌘⇧R' : 'Ctrl+Shift+R',
      onClick: () => setBatchRenameOpen(true),
    });
  }

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

  if (singleHash) {
    const { enabledSearchEngines } = useSettingsStore.getState().settings;
    const engineDefs: { key: typeof enabledSearchEngines[number]; label: string; icon: ReactNode }[] = [
      { key: 'tineye', label: 'TinEye', icon: <IconTinEye /> },
      { key: 'saucenao', label: 'SauceNAO', icon: <IconSauceNAO /> },
      { key: 'yandex', label: 'Yandex Images', icon: <IconYandex /> },
      { key: 'sogou', label: 'Sogou', icon: <IconSogou /> },
      { key: 'bing', label: 'Bing Visual Search', icon: <IconBing /> },
    ];
    const children: ContextMenuEntry[] = engineDefs
      .filter(e => enabledSearchEngines.includes(e.key))
      .map(e => ({
        type: 'item',
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
          .then(() => notifySuccess('Subfolder created', 'Folders'))
          .catch((err) => notifyError(err, 'Create Subfolder Failed'));
      },
    });
  }

  if (hasSelection) {
    items.push({
      type: 'item',
      label: 'New Folder from Selection',
      icon: <IconFolderSymlink />,
      onClick: async () => {
        const hashes = effectiveVirtual
          ? state.images.filter(i => !effectiveVirtual.excludedHashes.has(i.hash)).map(i => i.hash)
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

  if (hasSelection) {
    const regenHashes: string[] = effectiveVirtual
      ? state.images.filter(i => !effectiveVirtual.excludedHashes.has(i.hash)).map(i => i.hash)
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
  items.push({
    type: 'custom',
    key: 'layout',
    render: () => (
      <LayoutRow viewMode={viewMode} onChange={(m) => onViewModeChange?.(m)} />
    ),
  });

  if (folderId) {
    const reloadGrid = () => applyGridMutationEffects(requestGridReload);
    const sortAndReload = (sortBy: string, dir: string) =>
      FolderController.sortFolderItems(folderId, sortBy, dir).then(reloadGrid);
    const reverseAndReload = (hashes?: string[]) =>
      FolderController.reverseFolderItems(folderId, hashes).then(reloadGrid);
    items.push({
      type: 'submenu',
      label: 'Sort by',
      icon: <IconArrowsSort size={16} />,
      children: [
        { type: 'item', label: 'Name A→Z', onClick: () => sortAndReload('name', 'asc') },
        { type: 'item', label: 'Name Z→A', onClick: () => sortAndReload('name', 'desc') },
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

  items.push({
    type: 'submenu',
    label: 'Display',
    icon: <IconAdjustments size={16} />,
    children: [{ type: 'custom', key: 'display-panel', render: () => <DisplayOptionsPanel /> }],
  });

  items.push({ type: 'separator' });
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

  if (hasSelection) {
    items.push({ type: 'separator' });
    const count = effectiveSize;
    const virtualCount = effectiveVirtual ? state.virtualAllSelectedCount : null;
    const inTrash = statusFilter === 'trash';
    const freshSingleHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;

    const doRestore = () => {
      if (freshSingleHash) {
        dispatch({ type: 'FILTER_IMAGES', predicate: i => i.hash !== freshSingleHash });
        dispatch({ type: 'CLEAR_SELECTION' });
        setFileStatusWithLifecycleEffects(freshSingleHash, 'active', { gridReload: requestGridReload })
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
          deleteHashesWithLifecycleEffects([freshSingleHash], { gridReload: requestGridReload })
            .catch(err => notifyError(err, 'Delete Failed'));
        } else {
          const previousStatus = imagesRef.current.find((img) => img.hash === freshSingleHash)?.status ?? (statusFilter ?? 'active');
          setFileStatusWithLifecycleEffects(freshSingleHash, 'trash', { gridReload: requestGridReload })
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

  return items;
}
