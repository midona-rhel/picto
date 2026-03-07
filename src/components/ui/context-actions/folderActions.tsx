import {
  IconChevronRight,
  IconCopy,
  IconCursorText,
  IconFolderMinus,
  IconFolderPlus,
  IconFolders,
  IconTag,
  IconUpload,
} from '@tabler/icons-react';
import type { ContextMenuEntry } from '../ContextMenu';
import { compactMenu, expandEntries, iconAndColorEntries, invoke, sortSubmenu, type MenuAction } from './menuUtils';

export interface FolderSingleMenuOptions {
  openFolder?: MenuAction;
  createFolder?: MenuAction;
  createSubfolder?: MenuAction;
  setAutoTags?: MenuAction;
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
  if (opts.setAutoTags) {
    if (opts.createFolder || opts.createSubfolder || opts.renameFolder) items.push({ type: 'separator' });
    items.push({
      type: 'item',
      label: 'Set Auto-Tags...',
      icon: <IconTag size={14} />,
      onClick: () => invoke(opts.setAutoTags),
    });
  }

  const sort = sortSubmenu(opts.sortBy);
  if (sort) {
    items.push({ type: 'separator' });
    items.push(sort);
  }

  const expand = expandEntries(opts.expandActions);
  if (expand.length > 0) {
    items.push({ type: 'separator' });
    items.push(...expand);
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
