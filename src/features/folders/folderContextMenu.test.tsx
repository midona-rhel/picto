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
