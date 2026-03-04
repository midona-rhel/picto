import {
  IconArrowsSort,
  IconBinaryTree2,
  IconChevronRight,
  IconCopy,
  IconCursorText,
  IconFolderMinus,
  IconFolderPlus,
  IconFolders,
  IconListDetails,
  IconListTree,
  IconMoodSmile,
  IconPencil,
  IconSortAZ,
  IconSortZA,
  IconUpload,
} from '@tabler/icons-react';
import { FolderColorPicker } from '../smart-folders/FolderColorPicker';
import { FolderIconPicker } from '../smart-folders/FolderIconPicker';
import type { ContextMenuEntry } from '../ui/ContextMenu';

type MenuAction = () => void | Promise<void>;

function invoke(action?: MenuAction) {
  if (!action) return;
  void action();
}

function compactMenu(items: ContextMenuEntry[]): ContextMenuEntry[] {
  const compacted: ContextMenuEntry[] = [];
  for (const item of items) {
    if (item.type === 'separator') {
      const prev = compacted[compacted.length - 1];
      if (!prev || prev.type === 'separator') continue;
    }
    compacted.push(item);
  }
  while (compacted[0]?.type === 'separator') compacted.shift();
  while (compacted[compacted.length - 1]?.type === 'separator') compacted.pop();
  return compacted;
}

function sortSubmenu(opts?: {
  currentLevelAsc?: MenuAction;
  currentLevelDesc?: MenuAction;
  allLevelsAsc?: MenuAction;
  allLevelsDesc?: MenuAction;
}): ContextMenuEntry | null {
  if (!opts) return null;
  const children: ContextMenuEntry[] = [];
  if (opts.currentLevelAsc) {
    children.push({
      type: 'item',
      label: 'Current Level A -> Z',
      icon: <IconSortAZ size={14} />,
      onClick: () => invoke(opts.currentLevelAsc),
    });
  }
  if (opts.currentLevelDesc) {
    children.push({
      type: 'item',
      label: 'Current Level Z -> A',
      icon: <IconSortZA size={14} />,
      onClick: () => invoke(opts.currentLevelDesc),
    });
  }
  if (opts.allLevelsAsc || opts.allLevelsDesc) {
    if (children.length > 0) children.push({ type: 'separator' });
    if (opts.allLevelsAsc) {
      children.push({
        type: 'item',
        label: 'All Levels A -> Z',
        icon: <IconSortAZ size={14} />,
        onClick: () => invoke(opts.allLevelsAsc),
      });
    }
    if (opts.allLevelsDesc) {
      children.push({
        type: 'item',
        label: 'All Levels Z -> A',
        icon: <IconSortZA size={14} />,
        onClick: () => invoke(opts.allLevelsDesc),
      });
    }
  }
  if (children.length === 0) return null;
  return {
    type: 'submenu',
    label: 'Sort By',
    icon: <IconArrowsSort size={14} />,
    children,
  };
}

function iconAndColorEntries(opts?: {
  iconValue?: string | null;
  colorValue?: string | null;
  onIconChange?: (icon: string | null) => void | Promise<void>;
  onColorChange?: (color: string | null) => void | Promise<void>;
  iconLabel?: string;
}): ContextMenuEntry[] {
  if (!opts || (!opts.onIconChange && !opts.onColorChange)) return [];
  const entries: ContextMenuEntry[] = [];
  if (opts.onIconChange) {
    const iconLabel = opts.iconLabel ?? 'Icon';
    entries.push({
      type: 'submenu',
      label: iconLabel,
      icon: <IconMoodSmile size={14} />,
      children: [{
        type: 'custom',
        key: `folder-icon-${iconLabel.toLowerCase()}`,
        render: () => (
          <FolderIconPicker
            value={opts.iconValue ?? null}
            onChange={(icon) => { void opts.onIconChange?.(icon); }}
          />
        ),
      }],
    });
  }
  if (opts.onColorChange) {
    entries.push({
      type: 'custom',
      key: 'folder-color',
      render: () => (
        <FolderColorPicker
          value={opts.colorValue ?? null}
          onChange={(hex) => { void opts.onColorChange?.(hex); }}
        />
      ),
    });
  }
  return entries;
}

export interface FolderSingleMenuOptions {
  openFolder?: MenuAction;
  createFolder?: MenuAction;
  createSubfolder?: MenuAction;
  renameFolder?: MenuAction;
  sortBy?: {
    currentLevelAsc?: MenuAction;
    currentLevelDesc?: MenuAction;
    allLevelsAsc?: MenuAction;
    allLevelsDesc?: MenuAction;
  };
  expandActions?: {
    toggleFolder?: MenuAction;
    toggleSameLevel?: MenuAction;
    toggleAll?: MenuAction;
  };
  iconAndColor?: {
    iconValue?: string | null;
    colorValue?: string | null;
    onIconChange?: (icon: string | null) => void | Promise<void>;
    onColorChange?: (color: string | null) => void | Promise<void>;
    iconLabel?: string;
  };
  deleteFolder?: MenuAction;
  deleteLabel?: string;
  showDuplicate?: boolean;
  showExport?: boolean;
}

export function buildFolderSingleMenu(opts: FolderSingleMenuOptions): ContextMenuEntry[] {
  const items: ContextMenuEntry[] = [];
  if (opts.openFolder) {
    items.push({
      type: 'item',
      label: 'Open Folder',
      icon: <IconChevronRight size={14} />,
      onClick: () => invoke(opts.openFolder),
    });
    items.push({ type: 'separator' });
  }

  if (opts.createFolder) {
    items.push({
      type: 'item',
      label: 'New Folder',
      icon: <IconFolderPlus size={14} />,
      onClick: () => invoke(opts.createFolder),
    });
  }
  if (opts.createSubfolder) {
    items.push({
      type: 'item',
      label: 'New Subfolder',
      icon: <IconFolders size={14} />,
      onClick: () => invoke(opts.createSubfolder),
    });
  }
  if (opts.renameFolder) {
    if (opts.createFolder || opts.createSubfolder) items.push({ type: 'separator' });
    items.push({
      type: 'item',
      label: 'Rename',
      icon: <IconCursorText size={14} />,
      onClick: () => invoke(opts.renameFolder),
    });
  }

  const sort = sortSubmenu(opts.sortBy);
  if (sort) {
    items.push({ type: 'separator' });
    items.push(sort);
  }

  if (opts.expandActions) {
    const expandEntries: ContextMenuEntry[] = [];
    if (opts.expandActions.toggleFolder) {
      expandEntries.push({
        type: 'item',
        label: 'Expand/Collapse Folder',
        icon: <IconListTree size={14} />,
        onClick: () => invoke(opts.expandActions?.toggleFolder),
      });
    }
    if (opts.expandActions.toggleSameLevel) {
      expandEntries.push({
        type: 'item',
        label: 'Expand/Collapse Same Level Folders',
        icon: <IconListDetails size={14} />,
        onClick: () => invoke(opts.expandActions?.toggleSameLevel),
      });
    }
    if (opts.expandActions.toggleAll) {
      expandEntries.push({
        type: 'item',
        label: 'Expand/Collapse All Folders',
        icon: <IconBinaryTree2 size={14} />,
        onClick: () => invoke(opts.expandActions?.toggleAll),
      });
    }
    if (expandEntries.length > 0) {
      items.push({ type: 'separator' });
      items.push(...expandEntries);
    }
  }

  const iconAndColor = iconAndColorEntries(opts.iconAndColor);
  if (iconAndColor.length > 0) {
    items.push({ type: 'separator' });
    items.push(...iconAndColor);
  }

  if (opts.showDuplicate) {
    items.push({ type: 'separator' });
    items.push({
      type: 'item',
      label: 'Duplicate',
      icon: <IconCopy size={14} />,
      disabled: true,
      onClick: () => {},
    });
  }
  if (opts.showExport) {
    items.push({
      type: 'submenu',
      label: 'Export...',
      icon: <IconUpload size={14} />,
      children: [{ type: 'item', label: 'Export as Folder', disabled: true, onClick: () => {} }],
    });
  }

  if (opts.deleteFolder) {
    items.push({ type: 'separator' });
    items.push({
      type: 'item',
      label: opts.deleteLabel ?? 'Delete',
      icon: <IconFolderMinus size={14} />,
      danger: true,
      onClick: () => invoke(opts.deleteFolder),
    });
  }

  return compactMenu(items);
}

export interface FolderMultiMenuOptions {
  createSubfolder?: MenuAction;
  sortBy?: {
    currentLevelAsc?: MenuAction;
    currentLevelDesc?: MenuAction;
    allLevelsAsc?: MenuAction;
    allLevelsDesc?: MenuAction;
  };
  iconAndColor?: {
    onIconChange?: (icon: string | null) => void | Promise<void>;
    onColorChange?: (color: string | null) => void | Promise<void>;
    iconLabel?: string;
  };
  deleteFolders?: MenuAction;
  deleteLabel: string;
}

export function buildFolderMultiMenu(opts: FolderMultiMenuOptions): ContextMenuEntry[] {
  const items: ContextMenuEntry[] = [];
  if (opts.createSubfolder) {
    items.push({
      type: 'item',
      label: 'New Subfolder',
      icon: <IconFolderPlus size={14} />,
      onClick: () => invoke(opts.createSubfolder),
    });
  }
  const sort = sortSubmenu(opts.sortBy);
  if (sort) {
    items.push({ type: 'separator' });
    items.push(sort);
  }
  const iconAndColor = iconAndColorEntries({
    onIconChange: opts.iconAndColor?.onIconChange,
    onColorChange: opts.iconAndColor?.onColorChange,
    iconLabel: opts.iconAndColor?.iconLabel,
  });
  if (iconAndColor.length > 0) {
    items.push({ type: 'separator' });
    items.push(...iconAndColor);
  }
  if (opts.deleteFolders) {
    items.push({ type: 'separator' });
    items.push({
      type: 'item',
      label: opts.deleteLabel,
      icon: <IconFolderMinus size={14} />,
      danger: true,
      onClick: () => invoke(opts.deleteFolders),
    });
  }
  return compactMenu(items);
}

export function buildFolderSurfaceMenu(opts: { createSubfolder: MenuAction; label?: string }): ContextMenuEntry[] {
  return compactMenu([{
    type: 'item',
    label: opts.label ?? 'New Subfolder',
    icon: <IconFolderPlus size={14} />,
    onClick: () => invoke(opts.createSubfolder),
  }]);
}

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
