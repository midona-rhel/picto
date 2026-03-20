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
  IconGitMerge,
} from '@tabler/icons-react';
import { notifyInfo } from '../../lib/notify';
import type { Dispatch, MutableRefObject, ReactNode, SetStateAction } from 'react';
import type { ContextMenuEntry } from '../ContextMenu';
import { LayoutRow } from '../LayoutRow';
import { SortByRow } from '../SortByRow';
import { DisplayOptionsPanel } from '../../../features/layout/components/DisplayOptionsPanel';
import { IconBing, IconSauceNAO, IconSogou, IconTinEye, IconYandex } from '../SearchEngineIcons';
import type { SmartFolderPredicate } from '../../../features/smart-folders/components/types';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../../../features/grid/runtime';
import { FolderPickerService } from '../../services/folderPickerService';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { notifyError, notifySuccess } from '../../lib/notify';
import { useSettingsStore } from '../../../state/settingsStore';
import { useNavigationImageAdjustmentsStore } from '../../../state/navigationImageAdjustmentsStore';
import type { MediaItem } from '../../../features/grid/shared';
import { api, copyFileToClipboard, copyImageToClipboard, reverseImageSearch } from '#desktop/api';
import { bustThumbnailCache } from '../../lib/mediaUrl';

interface BuildGridImageContextMenuArgs {
  contextPoint: { x: number; y: number };
  isMac: boolean;
  state: GridRuntimeState;
  stateRef: MutableRefObject<GridRuntimeState>;
  imagesRef: MutableRefObject<MediaItem[]>;
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
  handleInboxSelectionAction: (status: 'active' | 'trash') => void;
  handleCopyTags: () => void;
  handlePasteTags: () => void;
  hasCopiedTags: boolean;
  handleOpenDetail: (hash: string) => void;
  collectionEntityId?: number | null;
  navigateToCollection: (collection: { id: number; name: string }) => void;
  setRenameValue: Dispatch<SetStateAction<string>>;
  setRenamingHash: Dispatch<SetStateAction<string | null>>;
  renameCancelledRef: MutableRefObject<boolean>;
  setBatchRenameOpen: Dispatch<SetStateAction<boolean>>;
  rightClickedHash: string | null;
  wasAlreadySelected: boolean;
  hasSelection: boolean;
  singleHash: string | null;
  singleImage: MediaItem | null;
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
    handleInboxSelectionAction,
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags,
    handleOpenDetail,
    collectionEntityId,
    navigateToCollection,
    setRenameValue,
    setRenamingHash,
    renameCancelledRef,
    setBatchRenameOpen,

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
  const imageLookup = imagesRef.current.length > 0 ? imagesRef.current : state.images;
  const hasAnyStillImages = imageLookup.some((entry) => entry.is_collection !== true && !entry.mime.startsWith('video/'));
  const grayscaleChecked = useNavigationImageAdjustmentsStore.getState().grayscaleEnabled;

  if (singleHash) {
    items.push({
      type: 'item',
      label: 'Open',
      icon: <IconArrowsMaximize />,
      shortcut: 'Enter',
      onClick: () => {
        handleOpenDetail(singleHash);
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
        onClick: () => api.files.openDefault(singleHash).catch(err => {
          notifyError(err, 'Open Failed');
        }),
      });
      items.push({
        type: 'item',
        label: isMac ? 'Reveal in Finder' : 'Reveal in Explorer',
        icon: <IconFolderOpen />,
        shortcut: isMac ? '\u2318Enter' : 'Ctrl+Enter',
        onClick: () => api.files.revealInFolder(singleHash).catch(err => {
          notifyError(err, 'Reveal Failed');
        }),
      });
      items.push({
        type: 'item',
        label: 'Open in New Window',
        icon: <IconAppWindow />,
        shortcut: isMac ? '\u2318O' : 'Ctrl+O',
        onClick: async () => {
          const img = stateRef.current.images.find(i => i.hash === singleHash);
          api.files.openInNewWindow(singleHash, img?.width, img?.height).catch(err => {
            notifyError(err, 'New Window Failed');
          });
        },
      });
    }
    items.push({ type: 'separator' });
  }

  if (hasSelection && collectionEntityId) {
    const freshHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;
    items.push({
      type: 'item',
      label: 'Remove from Collection',
      icon: <IconFolderMinus size={16} />,
      shortcut: isMac ? '\u2318\u21E7\u232B' : 'Ctrl+Shift+Del',
      onClick: () => {
        if (freshHash && collectionEntityId) {
          dispatch({ type: 'CLEAR_SELECTION' });
          api.collections.removeMembers({ id: collectionEntityId, hashes: [freshHash] })
            .then(() => {
              registerUndoAction({
                label: 'Remove from collection',
                undo: async () => {
                  await api.collections.addMembers({ id: collectionEntityId, hashes: [freshHash] });
                },
                redo: async () => {
                  await api.collections.removeMembers({ id: collectionEntityId, hashes: [freshHash] });
                },
              });
            })
            .catch(err => notifyError(err, 'Remove from Collection Failed'));
        } else {
          handleRemoveFromCollection();
        }
      },
    });
    items.push({ type: 'separator' });
  }

  if (hasSelection && !effectiveVirtual && !collectionEntityId) {
    const selectedHashSet = (() => {
      if (rightClickedHash && !wasAlreadySelected) return new Set([rightClickedHash]);
      return new Set(stateRef.current.selectedHashes);
    })();
    const selectedImages = stateRef.current.images.filter((img) => selectedHashSet.has(img.hash));
    const selCollections = selectedImages.filter((img) => img.is_collection);
    const selSingles = selectedImages.filter((img) => !img.is_collection);

    if (selCollections.length === 0 && selSingles.length >= 2) {
      // Only images selected → Create Collection
      items.push({
        type: 'item',
        label: 'Create Collection',
        icon: <IconFolderPlus />,
        onClick: async () => {
          const memberHashes = selSingles.map((img) => img.hash);
          if (memberHashes.length === 0) return;
          const now = new Date();
          const fallbackName = `Collection ${now.toLocaleDateString()} ${now.toLocaleTimeString()}`;
          const memberNames = selSingles
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
            collectionName = memberNames[0];
          }
          try {
            const id = await api.collections.create({ name: collectionName.trim() });
            const added = await api.collections.addMembers({ id, hashes: memberHashes });
            notifySuccess(`Created collection with ${added} item${added === 1 ? '' : 's'}`, 'Collections');
            navigateToCollection({ id, name: collectionName.trim() });
          } catch (err) {
            notifyError(err, 'Create Collection Failed');
          }
        },
      });
    } else if (selCollections.length === 1 && selSingles.length > 0) {
      // 1 collection + images → Merge into Collection
      items.push({
        type: 'item',
        label: 'Merge into Collection',
        icon: <IconGitMerge />,
        onClick: async () => {
          const targetId = selCollections[0].entity_id;
          if (targetId == null) return;
          const hashes = selSingles.map((img) => img.hash);
          try {
            const added = await api.collections.addMembers({ id: targetId, hashes });
            notifySuccess(`Added ${added} item${added === 1 ? '' : 's'} to collection`, 'Collections');
          } catch (err) {
            notifyError(err, 'Merge Failed');
          }
        },
      });
    } else if (selCollections.length >= 2) {
      // 2+ collections → Merge Collections
      items.push({
        type: 'item',
        label: 'Merge Collections',
        icon: <IconGitMerge />,
        onClick: async () => {
          const target = selCollections[0];
          const targetId = target.entity_id;
          if (targetId == null) return;
          const others = selCollections.slice(1);
          try {
            // Move members from each non-target collection into target
            for (const other of others) {
              if (other.entity_id == null) continue;
              const memberHashes = await api.collections.listMemberHashes(other.entity_id);
              if (memberHashes.length > 0) {
                await api.collections.addMembers({ id: targetId, hashes: memberHashes });
              }
              await api.collections.delete(other.entity_id);
            }
            // Also add any loose singles
            if (selSingles.length > 0) {
              await api.collections.addMembers({ id: targetId, hashes: selSingles.map((img) => img.hash) });
            }
            notifySuccess(`Merged ${others.length + 1} collections into "${target.name ?? 'Untitled'}"`, 'Collections');
            navigateToCollection({ id: targetId, name: target.name ?? 'Untitled' });
          } catch (err) {
            notifyError(err, 'Merge Collections Failed');
          }
        },
      });
    }

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
          } catch (err) {
            notifyError(err, 'Split Collection Failed');
          }
        },
      });
    }
    items.push({ type: 'separator' });
  }

  if (singleHash && folderId && !singleIsCollection) {
    items.push({ type: 'item', label: 'Pin to Top', icon: <IconPin />, disabled: true, onClick: () => {} });
    items.push({ type: 'item', label: 'Set as Folder Cover', icon: <IconSetCover />, disabled: true, onClick: () => {} });
    items.push({ type: 'separator' });
  }

  if (statusFilter === 'inbox' && (singleHash || hasSelection)) {
    const bulkInboxAction = !!effectiveVirtual || effectiveSize > 1;
    items.push({
      type: 'item',
      label: bulkInboxAction ? `Accept ${effectiveSize} Item${effectiveSize === 1 ? '' : 's'}` : 'Accept',
      icon: <IconCheck />,
      onClick: () => {
        if (bulkInboxAction) handleInboxSelectionAction('active');
        else if (singleHash) handleInboxAction(singleHash, 'active');
      },
    });
    items.push({
      type: 'item',
      label: bulkInboxAction ? `Reject ${effectiveSize} Item${effectiveSize === 1 ? '' : 's'}` : 'Reject',
      icon: <IconX />,
      onClick: () => {
        if (bulkInboxAction) handleInboxSelectionAction('trash');
        else if (singleHash) handleInboxAction(singleHash, 'trash');
      },
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
            const hashes = (s.virtualAllSelection
              ? s.images.filter(i => !s.virtualAllSelection!.excludedHashes.has(i.hash))
              : s.images.filter(i => s.selectedHashes.has(i.hash))
            ).filter(i => !i.is_collection).map(i => i.hash);
            if (hashes.length === 0) return;
            api.folders.addFiles(fId, hashes)
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

  if (singleHash && !singleIsCollection) {
    items.push({
      type: 'item',
      label: 'Copy',
      icon: <IconCopy />,
      shortcut: isMac ? '\u2318C' : 'Ctrl+C',
      onClick: () => {
        api.files.resolvePath(singleHash).then(copyFileToClipboard)
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
          const path = await api.files.resolvePath(singleHash);
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
            api.files.resolveThumbnailPath(singleHash).then(copyImageToClipboard)
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

  if (singleHash && !singleIsCollection) {
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
          notifyInfo(`Uploading to ${e.label}`);
          api.files.resolvePath(singleHash).then(path => reverseImageSearch(path, e.key))
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
        void api.folders.create({ name: 'New Folder', parent_id: folderId })
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
        const hashes = (effectiveVirtual
          ? state.images.filter(i => !effectiveVirtual.excludedHashes.has(i.hash))
          : state.images.filter(i => state.selectedHashes.has(i.hash))
        ).filter(i => !i.is_collection).map(i => i.hash);
        if (hashes.length === 0) return;
        try {
          const folder = await api.folders.create({ name: 'New Folder' });
          await api.folders.addFiles(folder.folder_id, hashes);
          notifySuccess(`Created folder with ${hashes.length} file(s)`, 'Folder Created');
        } catch (err) {
          notifyError(err, 'Create Folder Failed');
        }
      },
    });
  }

  if (hasSelection) {
    const regenHashes: string[] = (effectiveVirtual
      ? state.images.filter(i => !effectiveVirtual.excludedHashes.has(i.hash))
      : effectiveSize === 1 && singleHash
        ? state.images.filter(i => i.hash === singleHash)
        : state.images.filter(i => state.selectedHashes.has(i.hash))
    ).filter(i => !i.is_collection).map(i => i.hash);
    if (regenHashes.length > 0) {
      items.push({ type: 'separator' });
      items.push({
        type: 'item',
        label: regenHashes.length === 1 ? 'Regenerate Thumbnail' : `Regenerate Thumbnails (${regenHashes.length})`,
        icon: <IconRefresh />,
        shortcut: isMac ? '\u2318\u21E7T' : 'Ctrl+Shift+T',
        onClick: () => {
          notifyInfo(`Regenerating ${regenHashes.length} thumbnail(s)`);
          api.files.regenerateThumbnailsBatch(regenHashes)
            .then(r => {
              notifySuccess(`Regenerated ${r.regenerated} thumbnail(s)`, 'Thumbnails');
              bustThumbnailCache(regenHashes);
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
    const sortAndReload = (sortBy: string, dir: string) =>
      api.folders.sortItems(folderId, sortBy, dir);
    const reverseAndReload = (hashes?: string[]) =>
      api.folders.reverseItems(folderId, hashes);
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
  if (hasAnyStillImages) {
    items.push({
      type: 'check',
      label: 'Show in Grayscale',
      icon: <IconAdjustments size={16} />,
      checked: grayscaleChecked,
      onClick: () => {
        useNavigationImageAdjustmentsStore.getState().toggleGrayscale();
      },
    });
  }

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
      label: `Remove ${selCount > 1 ? `${selCount} Items` : 'Item'} from Folder`,
      icon: <IconFolderMinus size={16} />,
      shortcut: isMac ? '\u2318\u21E7\u232B' : 'Ctrl+Shift+Del',
      onClick: () => {
        if (freshHash && folderId) {
          dispatch({ type: 'CLEAR_SELECTION' });
          api.folders.removeFiles(folderId, [freshHash])
            .then(() => {
              registerUndoAction({
                label: 'Remove from folder',
                undo: async () => {
                  await api.folders.addFiles(folderId, [freshHash]);
                        },
                redo: async () => {
                  await api.folders.removeFiles(folderId, [freshHash]);
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

  if (hasSelection) {
    items.push({ type: 'separator' });
    const count = effectiveSize;
    const virtualCount = effectiveVirtual ? state.virtualAllSelectedCount : null;
    const inTrash = statusFilter === 'trash';
    const freshSingleHash = rightClickedHash && !wasAlreadySelected ? rightClickedHash : null;

    const doRestore = () => {
      if (freshSingleHash) {
        dispatch({ type: 'CLEAR_SELECTION' });
        api.files.setStatus(freshSingleHash, 'active')
          .then(() => {
            registerUndoAction({
              label: 'Restore item',
              undo: async () => {
                await api.files.setStatus(freshSingleHash, 'trash');
                    },
              redo: async () => {
                await api.files.setStatus(freshSingleHash, 'active');
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
        dispatch({ type: 'CLEAR_SELECTION' });
        if (inTrash) {
          api.files.deleteMany([freshSingleHash])
            .catch(err => notifyError(err, 'Delete Failed'));
        } else {
          const previousStatus = imagesRef.current.find((img) => img.hash === freshSingleHash)?.status ?? (statusFilter ?? 'active');
          api.files.setStatus(freshSingleHash, 'trash')
            .then(() => {
              registerUndoAction({
                label: 'Move to trash',
                undo: async () => {
                  await api.files.setStatus(freshSingleHash, previousStatus);
                        },
                redo: async () => {
                  await api.files.setStatus(freshSingleHash, 'trash');
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
            ? `Restore ${virtualCount.toLocaleString()} Item${virtualCount === 1 ? '' : 's'}`
            : 'Restore All')
          : `Restore ${count} Item${count > 1 ? 's' : ''}`,
        icon: <IconArrowBackUp />,
        onClick: doRestore,
      });
    }
    if (statusFilter !== 'inbox') {
      items.push({
        type: 'item',
        label: inTrash
          ? (effectiveVirtual
            ? (virtualCount != null
              ? `Permanently Delete ${virtualCount.toLocaleString()} Item${virtualCount === 1 ? '' : 's'}`
              : 'Permanently Delete All')
            : `Permanently Delete ${count} Item${count > 1 ? 's' : ''}`)
          : (effectiveVirtual
            ? (virtualCount != null
              ? `Move ${virtualCount.toLocaleString()} Item${virtualCount === 1 ? '' : 's'} to Trash`
              : 'Move All to Trash')
            : `Move ${count} Item${count > 1 ? 's' : ''} to Trash`),
        icon: <IconTrash />,
        shortcut: isMac ? '\u2318\u232B' : 'Del',
        danger: inTrash,
        onClick: doDelete,
      });
    }
  }

  return items;
}
