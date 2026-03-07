import { IconArrowsSort, IconCopy, IconCursorText, IconFolderMinus, IconPencil } from '@tabler/icons-react';
import type { ContextMenuEntry } from '../ContextMenu';
import { compactMenu, iconAndColorEntries, invoke, type MenuAction } from './menuUtils';

export interface SmartFolderMenuOptions {
  editSmartFolder: MenuAction;
  renameSmartFolder: MenuAction;
  setSortField: (field: string) => void;
  setSortOrder: (order: 'asc' | 'desc') => void;
  currentSortField: string;
  currentSortOrder: 'asc' | 'desc';
  duplicateSmartFolder: MenuAction;
  iconValue?: string | null;
  colorValue?: string | null;
  onIconChange: (icon: string | null) => void | Promise<void>;
  onColorChange: (color: string | null) => void | Promise<void>;
  deleteSmartFolder: MenuAction;
}

export function buildSmartFolderItemMenu(opts: SmartFolderMenuOptions): ContextMenuEntry[] {
  const sortChildren: ContextMenuEntry[] = [
    { type: 'check', label: 'Date Imported', checked: opts.currentSortField === 'imported_at', onClick: () => opts.setSortField('imported_at') },
    { type: 'check', label: 'Name', checked: opts.currentSortField === 'name', onClick: () => opts.setSortField('name') },
    { type: 'check', label: 'File Size', checked: opts.currentSortField === 'file_size', onClick: () => opts.setSortField('file_size') },
    { type: 'check', label: 'Rating', checked: opts.currentSortField === 'rating', onClick: () => opts.setSortField('rating') },
    { type: 'separator' },
    { type: 'check', label: 'Ascending', checked: opts.currentSortOrder === 'asc', onClick: () => opts.setSortOrder('asc') },
    { type: 'check', label: 'Descending', checked: opts.currentSortOrder === 'desc', onClick: () => opts.setSortOrder('desc') },
  ];

  const items: ContextMenuEntry[] = [
    {
      type: 'item',
      label: 'Edit Smart Folder...',
      icon: <IconPencil size={14} />,
      onClick: () => invoke(opts.editSmartFolder),
    },
    {
      type: 'item',
      label: 'Rename',
      icon: <IconCursorText size={14} />,
      onClick: () => invoke(opts.renameSmartFolder),
    },
    { type: 'separator' },
    {
      type: 'submenu',
      label: 'Sort By',
      icon: <IconArrowsSort size={14} />,
      children: sortChildren,
    },
    { type: 'separator' },
    {
      type: 'item',
      label: 'Duplicate',
      icon: <IconCopy size={14} />,
      onClick: () => invoke(opts.duplicateSmartFolder),
    },
    { type: 'separator' },
    ...iconAndColorEntries({
      iconValue: opts.iconValue,
      colorValue: opts.colorValue,
      onIconChange: opts.onIconChange,
      onColorChange: opts.onColorChange,
      iconLabel: 'Change Icon...',
    }),
    { type: 'separator' },
    {
      type: 'item',
      label: 'Remove Smart Folder',
      icon: <IconFolderMinus size={14} />,
      danger: true,
      onClick: () => invoke(opts.deleteSmartFolder),
    },
  ];
  return compactMenu(items);
}
