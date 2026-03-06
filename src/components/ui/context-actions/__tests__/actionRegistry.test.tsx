import { describe, expect, it, vi } from 'vitest';
import type { ContextMenuEntry } from '../../ContextMenu';
import { buildFolderMultiMenu, buildFolderSingleMenu, buildFolderSurfaceMenu } from '../folderActions';
import { buildSmartFolderItemMenu } from '../smartFolderActions';
import { buildTagContextMenu } from '../tagActions';
import { buildGridImageContextMenu } from '../imageActions';

function labels(items: ContextMenuEntry[]): string[] {
  const out: string[] = [];
  for (const item of items) {
    if (item.type === 'item' || item.type === 'check' || item.type === 'submenu') {
      out.push(item.label);
      if (item.type === 'submenu') out.push(...labels(item.children));
    }
  }
  return out;
}

function hasInvalidSeparators(items: ContextMenuEntry[]): boolean {
  if (items.length === 0) return false;
  if (items[0].type === 'separator') return true;
  if (items[items.length - 1].type === 'separator') return true;
  for (let i = 1; i < items.length; i++) {
    if (items[i].type === 'separator' && items[i - 1].type === 'separator') return true;
  }
  return false;
}

describe('context action registries', () => {
  it('keeps folder actions aligned across single/multi/surface contexts', () => {
    const sidebarSingle = buildFolderSingleMenu({
      createFolder: () => {},
      createSubfolder: () => {},
      renameFolder: () => {},
      sortBy: {
        currentLevelAsc: () => {},
        currentLevelDesc: () => {},
        allLevelsAsc: () => {},
        allLevelsDesc: () => {},
      },
      iconAndColor: {
        iconValue: null,
        colorValue: null,
        onIconChange: () => {},
        onColorChange: () => {},
      },
      deleteFolder: () => {},
      deleteLabel: 'Remove Folder',
    });
    const multi = buildFolderMultiMenu({
      createSubfolder: () => {},
      sortBy: {
        currentLevelAsc: () => {},
        currentLevelDesc: () => {},
      },
      iconAndColor: {
        onIconChange: () => {},
        onColorChange: () => {},
      },
      deleteFolders: () => {},
      deleteLabel: 'Remove 2 Folders',
    });
    const surface = buildFolderSurfaceMenu({ createSubfolder: () => {} });

    expect(labels(sidebarSingle)).toContain('New Subfolder');
    expect(labels(sidebarSingle)).toContain('Rename');
    expect(labels(multi)).toContain('New Subfolder');
    expect(labels(surface)).toContain('New Subfolder');
    expect(hasInvalidSeparators(sidebarSingle)).toBe(false);
    expect(hasInvalidSeparators(multi)).toBe(false);
  });

  it('builds smart-folder menu with expected core actions', () => {
    const items = buildSmartFolderItemMenu({
      editSmartFolder: () => {},
      renameSmartFolder: () => {},
      setSortField: () => {},
      setSortOrder: () => {},
      currentSortField: 'imported_at',
      currentSortOrder: 'desc',
      duplicateSmartFolder: () => {},
      iconValue: null,
      colorValue: null,
      onIconChange: () => {},
      onColorChange: () => {},
      deleteSmartFolder: () => {},
    });
    const menuLabels = labels(items);
    expect(menuLabels).toContain('Edit Smart Folder...');
    expect(menuLabels).toContain('Sort By');
    expect(menuLabels).toContain('Duplicate');
    expect(menuLabels).toContain('Remove Smart Folder');
    expect(hasInvalidSeparators(items)).toBe(false);
  });

  it('preserves tag action matrix for local vs ptr context', () => {
    const baseArgs = {
      tag: { tag_id: 1, namespace: 'artist', subtag: 'alice' },
      siblings: [],
      parents: [],
      children: [],
      formatTagDisplay: (ns: string, subtag: string) => (ns ? `${ns}:${subtag}` : subtag),
      onShowImages: () => {},
      onRename: () => {},
      onMerge: () => {},
      onCopy: () => {},
      onViewRelations: () => {},
      onNavigateTag: () => {},
      onAddSibling: () => {},
      onAddParent: () => {},
      onAddChild: () => {},
      onDelete: () => {},
    };

    const localItems = buildTagContextMenu({ ...baseArgs, source: 'local' });
    const ptrItems = buildTagContextMenu({ ...baseArgs, source: 'ptr' });
    const localLabels = labels(localItems);
    const ptrLabels = labels(ptrItems);

    expect(localLabels).toContain('Rename');
    expect(localLabels).toContain('Merge into…');
    expect(localLabels).toContain('Delete');
    expect(ptrLabels).not.toContain('Rename');
    expect(ptrLabels).not.toContain('Delete');
  });

  it('keeps image action availability consistent for single and collection context', () => {
    const dispatch = vi.fn();
    const itemsSingle = buildGridImageContextMenu({
      contextPoint: { x: 10, y: 10 },
      isMac: false,
      state: {
        selectedHashes: new Set<string>(),
        virtualAllSelection: null,
        virtualAllSelectedCount: null,
        images: [],
      } as never,
      stateRef: { current: { selectedHashes: new Set<string>(), virtualAllSelection: null, images: [] } } as never,
      imagesRef: { current: [] } as never,
      dispatch,
      viewMode: 'waterfall',
      sortField: 'imported_at',
      sortOrder: 'desc',
      effectiveSelectedHashes: new Set<string>(),
      activateVirtualSelectAll: () => {},
      handleDeleteSelected: () => {},
      handleRestoreSelected: () => {},
      handleRemoveFromFolder: () => {},
      handleRemoveFromCollection: () => {},
      handleInboxAction: () => {},
      handleCopyTags: () => {},
      handlePasteTags: () => {},
      hasCopiedTags: false,
      navigateToCollection: () => {},
      setRenameValue: () => {},
      setRenamingHash: () => {},
      renameCancelledRef: { current: false },
      setBatchRenameOpen: () => {},
      requestGridReload: () => {},
      rightClickedHash: 'abc',
      wasAlreadySelected: false,
      hasSelection: true,
      singleHash: 'abc',
      singleImage: { hash: 'abc', name: 'x', mime: 'image/jpeg' } as never,
      singleIsCollection: false,
      singleCollectionId: null,
      effectiveVirtual: null,
      effectiveSize: 1,
    });

    const itemsCollection = buildGridImageContextMenu({
      contextPoint: { x: 10, y: 10 },
      isMac: false,
      state: {
        selectedHashes: new Set<string>(['abc']),
        virtualAllSelection: null,
        virtualAllSelectedCount: 1,
        images: [{ hash: 'abc', is_collection: true, entity_id: 12 }],
      } as never,
      stateRef: { current: { selectedHashes: new Set<string>(['abc']), virtualAllSelection: null, images: [{ hash: 'abc', is_collection: true, entity_id: 12 }] } } as never,
      imagesRef: { current: [] } as never,
      dispatch,
      viewMode: 'waterfall',
      sortField: 'imported_at',
      sortOrder: 'desc',
      effectiveSelectedHashes: new Set<string>(['abc']),
      activateVirtualSelectAll: () => {},
      handleDeleteSelected: () => {},
      handleRestoreSelected: () => {},
      handleRemoveFromFolder: () => {},
      handleRemoveFromCollection: () => {},
      handleInboxAction: () => {},
      handleCopyTags: () => {},
      handlePasteTags: () => {},
      hasCopiedTags: false,
      collectionEntityId: 12,
      navigateToCollection: () => {},
      setRenameValue: () => {},
      setRenamingHash: () => {},
      renameCancelledRef: { current: false },
      setBatchRenameOpen: () => {},
      requestGridReload: () => {},
      rightClickedHash: 'abc',
      wasAlreadySelected: true,
      hasSelection: true,
      singleHash: 'abc',
      singleImage: { hash: 'abc', is_collection: true, entity_id: 12, name: 'Collection 12', mime: 'image/jpeg' } as never,
      singleIsCollection: true,
      singleCollectionId: 12,
      effectiveVirtual: null,
      effectiveSize: 1,
    });

    const singleLabels = labels(itemsSingle);
    const collectionLabels = labels(itemsCollection);
    expect(singleLabels).toContain('Open');
    expect(singleLabels).toContain('Open With Default App');
    expect(collectionLabels).toContain('Edit Collection');
  });
});
