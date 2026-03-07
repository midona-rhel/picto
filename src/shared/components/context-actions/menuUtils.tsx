import {
  IconArrowsSort,
  IconListDetails,
  IconListTree,
  IconBinaryTree2,
  IconSortAZ,
  IconSortZA,
  IconMoodSmile,
} from '@tabler/icons-react';
import { FolderColorPicker } from '../../../features/smart-folders/components/FolderColorPicker';
import { FolderIconPicker } from '../../../features/smart-folders/components/FolderIconPicker';
import type { ContextMenuEntry } from '../ContextMenu';

export type MenuAction = () => void | Promise<void>;

export function invoke(action?: MenuAction) {
  if (!action) return;
  void action();
}

export function compactMenu(items: ContextMenuEntry[]): ContextMenuEntry[] {
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

export function sortSubmenu(opts?: {
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

export function expandEntries(opts?: {
  toggleFolder?: MenuAction;
  toggleSameLevel?: MenuAction;
  toggleAll?: MenuAction;
}): ContextMenuEntry[] {
  if (!opts) return [];
  const out: ContextMenuEntry[] = [];
  if (opts.toggleFolder) {
    out.push({
      type: 'item',
      label: 'Expand/Collapse Folder',
      icon: <IconListTree size={14} />,
      onClick: () => invoke(opts.toggleFolder),
    });
  }
  if (opts.toggleSameLevel) {
    out.push({
      type: 'item',
      label: 'Expand/Collapse Same Level Folders',
      icon: <IconListDetails size={14} />,
      onClick: () => invoke(opts.toggleSameLevel),
    });
  }
  if (opts.toggleAll) {
    out.push({
      type: 'item',
      label: 'Expand/Collapse All Folders',
      icon: <IconBinaryTree2 size={14} />,
      onClick: () => invoke(opts.toggleAll),
    });
  }
  return out;
}

export function iconAndColorEntries(opts?: {
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
