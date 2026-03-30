/**
 * Grid context menu — context-aware entry builder for right-click on grid tiles.
 *
 * Icons follow platform conventions (Finder/Explorer), actions are wired where
 * backend support exists, disabled items have TODO comments.
 */

import {
  IconArrowsMaximize, IconExternalLink, IconFolderSearch, IconBrandFinder, IconAppWindow,
  IconFolderMinus, IconFolderPlus,
  IconCopy, IconClipboardCopy, IconBookmark, IconBookmarks,
  IconRefresh, IconTrash, IconArrowBackUp,
  IconSelectAll, IconDeselect,
} from '@tabler/icons-react';
import type { MenuItem, MenuSeparator, MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
import { buildContextMenuViewEntries } from './GridViewMenu';
import { getShortcut, formatKeysDisplay } from '../../shared/lib/shortcuts';

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

/** Format a shortcut id to its display string, or return undefined if not found. */
function kbd(id: string): string | undefined {
  const def = getShortcut(id);
  return def ? formatKeysDisplay(def.keys) : undefined;
}

interface GridMenuContext {
  selectionCount: number;
  querySelectionActive: boolean;
  singleSelected: boolean;
  singleHash: string | null;
  singleKind: string | null;
  hasCollections: boolean;
  scopeKind: 'system' | 'folder' | 'smart_folder' | null;
  statusFilter: string | null;
  loadedCount: number;
  onSelectAll: () => void;
  onDeselectAll: () => void;
  onOpen?: () => void;
  onOpenDefault?: (hash: string) => void;
  onRevealInFolder?: (hash: string) => void;
  onCopyFilePath?: (hash: string) => void;
  onCopyFile?: (hash: string) => void;
  onCopyName?: (name: string) => void;
  onMoveToTrash?: () => void;
  onRestore?: () => void;
  onPermanentDelete?: () => void;
  onAddToFolder?: () => void;
  onRemoveFromFolder?: () => void;
}

function sep(): MenuSeparator {
  return { separator: true };
}

function item(
  label: string,
  opts: {
    icon?: React.ReactNode;
    shortcut?: string;
    action?: () => void;
    danger?: boolean;
    disabled?: boolean;
  } = {},
): MenuItem {
  return {
    label,
    icon: opts.icon,
    shortcut: opts.shortcut,
    action: opts.action ?? (() => {}),
    danger: opts.danger,
    disabled: opts.disabled ?? !opts.action,
  };
}

/** Build context menu entries for right-clicking a tile. */
export function buildTileContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const { selectionCount, singleSelected, singleHash, statusFilter, scopeKind } = ctx;
  const hasSelection = selectionCount > 0;
  const entries: MenuEntry[] = [];

  // ── Open actions ──
  if (singleSelected) {
    entries.push(item('Open', { icon: <IconArrowsMaximize size={15} />, shortcut: kbd('view.detailView'), action: ctx.onOpen }));
    entries.push(item('Open with Default App', {
      icon: <IconExternalLink size={15} />,
      shortcut: kbd('file.openDefaultApp'),
      action: singleHash && ctx.onOpenDefault ? () => ctx.onOpenDefault!(singleHash!) : undefined,
    }));
    entries.push(item(isMac ? 'Reveal in Finder' : 'Show in Explorer', {
      icon: isMac ? <IconBrandFinder size={15} /> : <IconFolderSearch size={15} />,
      shortcut: kbd('file.revealInFolder'),
      action: singleHash && ctx.onRevealInFolder ? () => ctx.onRevealInFolder!(singleHash!) : undefined,
    }));
    entries.push(item('Open in New Window', { icon: <IconAppWindow size={15} />, shortcut: kbd('file.openNewWindow'), disabled: true })); // TODO: detail window
    entries.push(sep());
  }

  // ── View options — layout/sort inline, display toggles as custom panel ──
  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) {
    entries.push(entry);
  }
  entries.push(sep());

  // ── Organization ──
  if (hasSelection) {
    entries.push(item('Add to Folder', {
      icon: <IconFolderPlus size={15} />,
      shortcut: kbd('file.addToFolder'),
      action: ctx.onAddToFolder,
    }));
    entries.push(sep());
  }

  // ── Rename ──
  if (singleSelected) {
    entries.push(item('Rename', { icon: <IconRename size={15} />, shortcut: kbd('edit.rename'), disabled: true })); // TODO: inline rename
    entries.push(sep());
  }

  // ── Copy ──
  if (singleSelected && singleHash) {
    entries.push(item('Copy', {
      icon: <IconCopy size={15} />,
      shortcut: kbd('edit.copy'),
      action: ctx.onCopyFile ? () => ctx.onCopyFile!(singleHash!) : undefined,
    }));
    entries.push(item('Copy File Path', {
      icon: <IconClipboardCopy size={15} />,
      shortcut: kbd('edit.copyFilePath'),
      action: ctx.onCopyFilePath ? () => ctx.onCopyFilePath!(singleHash!) : undefined,
    }));
    entries.push(sep());
  }

  // ── Tags ──
  if (hasSelection) {
    entries.push(item('Copy Tags', { icon: <IconBookmark size={15} />, shortcut: kbd('edit.copyTags'), disabled: true })); // TODO
    entries.push(item('Paste Tags', { icon: <IconBookmarks size={15} style={{ transform: 'scaleX(-1)' }} />, shortcut: kbd('edit.pasteTags'), disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Thumbnails ──
  if (hasSelection) {
    const thumbLabel = selectionCount > 1 ? `Regenerate ${selectionCount} Thumbnails` : 'Regenerate Thumbnail';
    entries.push(item(thumbLabel, { icon: <IconRefresh size={15} />, shortcut: kbd('file.regenerateThumbnail'), disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Selection ──
  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
  if (hasSelection) {
    entries.push(item('Deselect All', { icon: <IconDeselect size={15} />, shortcut: kbd('edit.deselectAll'), action: ctx.onDeselectAll }));
  }
  entries.push(sep());

  // ── Folder-specific removal ──
  if (scopeKind === 'folder' && hasSelection) {
    const removeLabel = selectionCount > 1 ? `Remove ${selectionCount} from Folder` : 'Remove from Folder';
    entries.push(item(removeLabel, {
      icon: <IconFolderMinus size={15} />,
      shortcut: kbd('file.removeFromFolder'),
      action: ctx.onRemoveFromFolder,
    }));
    entries.push(sep());
  }

  // ── Destructive ──
  if (hasSelection) {
    if (statusFilter === 'trash') {
      const restoreLabel = selectionCount > 1 ? `Restore ${selectionCount} Items` : 'Restore';
      entries.push(item(restoreLabel, { icon: <IconArrowBackUp size={15} />, shortcut: kbd('file.restore'), action: ctx.onRestore }));
      const deleteLabel = selectionCount > 1 ? `Delete ${selectionCount} Permanently` : 'Delete Permanently';
      entries.push(item(deleteLabel, { icon: <IconTrash size={15} />, shortcut: kbd('file.permanentDelete'), danger: true, action: ctx.onPermanentDelete }));
    } else {
      const trashLabel = selectionCount > 1 ? `Move ${selectionCount} to Trash` : 'Move to Trash';
      entries.push(item(trashLabel, {
        icon: <IconTrash size={15} />,
        shortcut: kbd('file.delete'),
        danger: true,
        action: ctx.onMoveToTrash,
      }));
    }
  }

  return entries;
}

/** Build context menu entries for right-clicking empty grid space. */
export function buildEmptyContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const entries: MenuEntry[] = [];

  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) {
    entries.push(entry);
  }
  entries.push(sep());

  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));

  return entries;
}
