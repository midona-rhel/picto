import { describe, expect, it } from 'vitest';
import type { ContextMenuEntry } from '../../../shared/components/ContextMenu';
import {
  buildFolderMultiMenu,
  buildFolderSingleMenu,
  buildFolderSurfaceMenu,
  buildSmartFolderItemMenu,
} from '../contextMenuRegistry';

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

describe('contextMenuRegistry', () => {
  it('keeps core folder actions aligned across sidebar and subfolder grid menus', () => {
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
        iconLabel: 'Change Icon...',
      },
      deleteFolder: () => {},
      deleteLabel: 'Remove Folder',
    });

    const subfolderSingle = buildFolderSingleMenu({
      openFolder: () => {},
      createSubfolder: () => {},
      renameFolder: () => {},
      iconAndColor: {
        iconValue: null,
        colorValue: null,
        onIconChange: () => {},
        onColorChange: () => {},
        iconLabel: 'Icon',
      },
      deleteFolder: () => {},
      deleteLabel: 'Remove Folder',
    });

    const sidebarLabels = labels(sidebarSingle);
    const subfolderLabels = labels(subfolderSingle);

    expect(sidebarLabels).toContain('New Subfolder');
    expect(sidebarLabels).toContain('Rename');
    expect(sidebarLabels).toContain('Remove Folder');

    expect(subfolderLabels).toContain('New Subfolder');
    expect(subfolderLabels).toContain('Rename');
    expect(subfolderLabels).toContain('Remove Folder');
  });

  it('builds multi-folder and surface menus without duplicate separators', () => {
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
      deleteLabel: 'Remove 3 Folders',
    });

    const surface = buildFolderSurfaceMenu({
      createSubfolder: () => {},
    });

    expect(hasInvalidSeparators(multi)).toBe(false);
    expect(hasInvalidSeparators(surface)).toBe(false);
    expect(labels(surface)).toEqual(['New Subfolder']);
  });

  it('builds smart-folder item menu with expected action labels', () => {
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
});
