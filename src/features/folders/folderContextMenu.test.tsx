import { describe, expect, it, vi } from 'vitest';
import {
  buildBulkFolderContextMenu,
  buildFolderContextMenu,
  topLevelSelectedFolderIds,
} from './folderContextMenu';

describe('buildFolderContextMenu', () => {
  it('exposes folder operations instead of media selection actions', () => {
    const noop = vi.fn();
    const entries = buildFolderContextMenu({
      inQuickAccess: false,
      watchEnabled: false,
      onOpen: noop,
      onNewSubfolder: noop,
      onToggleQuickAccess: noop,
      onRename: noop,
      onMove: noop,
      onDuplicate: noop,
      onSetAutoTags: noop,
      onImport: noop,
      onAttachWatch: noop,
      onSortTree: noop,
      onSortContents: noop,
      onExport: noop,
      onDelete: noop,
    });
    const labels = entries.flatMap((entry) => 'label' in entry ? [entry.label] : []);

    expect(labels).toContain('Open Folder');
    expect(labels).toContain('New Subfolder');
    expect(labels).toContain('Rename');
    expect(labels).toContain('Move to...');
    expect(labels).toContain('Set Auto Tags...');
    expect(labels).toContain('Delete');
    expect(labels).not.toContain('Select All');
  });

  it('offers every supported folder-content sort field', () => {
    const onSortContents = vi.fn();
    const entries = buildFolderContextMenu({
      inQuickAccess: false,
      watchEnabled: false,
      onOpen: vi.fn(),
      onNewSubfolder: vi.fn(),
      onToggleQuickAccess: vi.fn(),
      onRename: vi.fn(),
      onMove: vi.fn(),
      onDuplicate: vi.fn(),
      onSetAutoTags: vi.fn(),
      onImport: vi.fn(),
      onAttachWatch: vi.fn(),
      onSortTree: vi.fn(),
      onSortContents,
      onExport: vi.fn(),
      onDelete: vi.fn(),
    });
    const submenu = entries.find(
      (entry) => 'submenu' in entry && entry.submenu && entry.label === 'Sort by',
    );
    if (!submenu || !('children' in submenu)) throw new Error('missing Sort by submenu');

    expect(submenu.children.map((entry) => 'label' in entry ? entry.label : '')).toEqual([
      'Name',
      'Import Date',
      'Date Created',
      'Date Modified',
      'Size',
      'Notes',
    ]);
    const modified = submenu.children.find(
      (entry) => 'label' in entry && entry.label === 'Date Modified',
    );
    if (!modified || !('action' in modified)) throw new Error('missing Date Modified action');
    modified.action();
    expect(onSortContents).toHaveBeenCalledWith('modified_at');
  });

  it('uses bulk-safe actions for a multi-folder selection', () => {
    const noop = vi.fn();
    const entries = buildBulkFolderContextMenu({
      allInQuickAccess: false,
      count: 3,
      onToggleQuickAccess: noop,
      onDuplicate: noop,
      onMove: noop,
      onSetAutoTags: noop,
      onSortContents: noop,
      onDelete: noop,
    });
    const labels = entries.flatMap((entry) => 'label' in entry ? [entry.label] : []);
    expect(labels).toContain('Delete 3 Folders');
    expect(labels).not.toContain('Rename');
  });

  it('moves a selected parent without separately moving its selected child', () => {
    expect(topLevelSelectedFolderIds([
      { id: 'folder:1', parent_id: null },
      { id: 'folder:2', parent_id: 'folder:1' },
      { id: 'folder:3', parent_id: null },
    ], [1, 2, 3])).toEqual([1, 3]);
  });
});
