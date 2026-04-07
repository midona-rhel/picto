/**
 * Grid context menu — context-aware entry builder for right-click on grid tiles.
 *
 * Rules:
 *   - Only "Delete Permanently" is ever danger (red). Everything else is normal.
 *   - Reject and Move to Trash use the trash icon, never custom icons.
 *   - Accept/Reject appear at the top in inbox scope.
 */

import {
  IconArrowsMaximize, IconExternalLink, IconFolderSearch, IconBrandFinder, IconAppWindow,
  IconFolderMinus, IconFolderPlus,
  IconCopy, IconClipboardCopy, IconLink, IconBookmark, IconBookmarks,
  IconRefresh, IconTrash, IconArrowBackUp,
  IconSelectAll, IconDeselect,
  IconStack2, IconStackPop, IconSearch,
  IconFolder,
} from '@tabler/icons-react';
import type { MenuItem, MenuSeparator, MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
import { IconFolderNewSelection } from '../../shared/ui/IconPicker/customIcons';
import { buildContextMenuViewEntries } from './GridViewMenu';
import { getShortcut, formatKeysDisplay } from '../../shared/lib/shortcuts';

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

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
  hasFolders?: boolean;
  isMixed?: boolean;
  isFoldersOnly?: boolean;
  scopeKind: 'system' | 'folder' | 'smart_folder' | 'collection' | null;
  collectionId?: number | null;
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
  onOpenNewWindow?: (hash: string) => void;
  onAddToFolder?: () => void;
  onRemoveFromFolder?: () => void;
  onCreateCollection?: () => void;
  onRemoveFromCollection?: () => void;
  onSplitCollection?: () => void;
  onEditCollection?: () => void;
  onOpenTagSelect?: () => void;
  onOpenAiTagger?: () => void;
  onOpenBatchRename?: () => void;
  onAccept?: () => void;
  onReject?: () => void;
  onRename?: () => void;
  onRegenerateThumbnails?: () => void;
  onCopyTags?: () => void;
  onPasteTags?: () => void;
  hasClipboardTags?: boolean;
  singleName?: string | null;
  singleMime?: string | null;
  onCopyLink?: (hash: string, mime: string) => void;
  onNewFolderWithSelection?: () => void;
  onMergeIntoCollection?: () => void;
  onSearchByImage?: (engine: string, hash: string) => void;
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

  // ── Mixed selection (folders + entities) or folders-only: limited menu ──
  if (ctx.isMixed) {
    entries.push(item('Select All', { shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
    entries.push(item('Deselect All', { action: ctx.onDeselectAll }));
    if (ctx.onMoveToTrash) {
      entries.push(sep());
      entries.push(item('Move to Trash', { icon: <IconTrash size={15} />, danger: true, action: ctx.onMoveToTrash }));
    }
    return entries;
  }
  if (ctx.isFoldersOnly) {
    if (singleSelected && singleHash?.startsWith('folder:')) {
      entries.push(item('Open Folder', { icon: <IconFolder size={15} />, action: ctx.onOpen }));
      entries.push(sep());
    }
    entries.push(item('Select All', { shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
    entries.push(item('Deselect All', { action: ctx.onDeselectAll }));
    return entries;
  }

  // ── Inbox accept/reject — top of menu ──
  if (statusFilter === 'inbox' && hasSelection) {
    const acceptLabel = selectionCount > 1 ? `Accept ${selectionCount} Items` : 'Accept';
    entries.push(item(acceptLabel, { icon: <IconArrowBackUp size={15} />, shortcut: kbd('inbox.accept'), action: ctx.onAccept }));
    const rejectLabel = selectionCount > 1 ? `Reject ${selectionCount} Items` : 'Reject';
    entries.push(item(rejectLabel, { icon: <IconTrash size={15} />, shortcut: kbd('inbox.reject'), action: ctx.onReject }));
    entries.push(sep());
  }

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
    entries.push(item('Open in New Window', {
      icon: <IconAppWindow size={15} />,
      shortcut: kbd('file.openNewWindow'),
      action: singleHash && ctx.onOpenNewWindow ? () => ctx.onOpenNewWindow!(singleHash!) : undefined,
    }));
    entries.push(sep());
  }

  // ── View options ──
  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) entries.push(entry);
  entries.push(sep());

  // ── Organization ──
  if (hasSelection) {
    entries.push(item('Add to Folder', {
      icon: <IconFolderPlus size={15} />,
      shortcut: kbd('file.addToFolder'),
      action: ctx.onAddToFolder,
    }));
    if (ctx.onNewFolderWithSelection) {
      entries.push(item('New Folder with Selection', {
        icon: <IconFolderNewSelection size={15} />,
        action: ctx.onNewFolderWithSelection,
      }));
    }
    if (selectionCount > 1 && ctx.scopeKind !== 'collection') {
      if (ctx.hasCollections && ctx.onMergeIntoCollection) {
        // Selection includes a collection — offer merge instead of create
        entries.push(item('Merge into Collection', {
          icon: <IconStack2 size={15} />,
          action: ctx.onMergeIntoCollection,
        }));
      } else {
        entries.push(item('Create Collection', {
          icon: <IconStack2 size={15} />,
          action: ctx.onCreateCollection,
        }));
      }
    }
    if (ctx.scopeKind === 'collection') {
      const removeLabel = selectionCount > 1 ? `Remove ${selectionCount} from Collection` : 'Remove from Collection';
      entries.push(item(removeLabel, {
        icon: <IconStackPop size={15} />,
        action: ctx.onRemoveFromCollection,
      }));
    }
    entries.push(sep());
  }
  // ── Collection actions ──
  if (ctx.hasCollections || ctx.scopeKind === 'collection') {
    entries.push(item('Edit Collection', {
      icon: <IconStack2 size={15} />,
      action: ctx.onEditCollection,
    }));
  }
  if (ctx.scopeKind === 'collection' || (singleSelected && ctx.singleKind === 'collection')) {
    entries.push(item('Split Collection', {
      icon: <IconStackPop size={15} />,
      action: ctx.onSplitCollection,
    }));
    entries.push(sep());
  }

  // ── Rename ──
  if (singleSelected) {
    entries.push(item('Rename', { icon: <IconRename size={15} />, shortcut: kbd('edit.rename'), action: ctx.onRename }));
  }
  if (hasSelection && selectionCount > 1) {
    entries.push(item('Batch Rename', { icon: <IconRename size={15} />, shortcut: kbd('edit.batchRename'), action: ctx.onOpenBatchRename }));
  }
  if (singleSelected || (hasSelection && selectionCount > 1)) {
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
    if (ctx.singleName) {
      entries.push(item('Copy Name', {
        icon: <IconClipboardCopy size={15} />,
        action: ctx.onCopyName ? () => ctx.onCopyName!(ctx.singleName!) : undefined,
      }));
    }
    if (ctx.singleMime && ctx.onCopyLink) {
      entries.push(item('Copy as Link', {
        icon: <IconLink size={15} />,
        action: () => ctx.onCopyLink!(singleHash!, ctx.singleMime!),
      }));
    }
    entries.push(sep());
  }

  // ── Tags ──
  if (hasSelection) {
    entries.push(item('Add Tags', { icon: <IconBookmark size={15} />, shortcut: kbd('organize.addTag'), action: ctx.onOpenTagSelect }));
    entries.push(item('AI Tagger', { icon: <IconBookmarks size={15} />, shortcut: kbd('organize.autoTag'), action: ctx.onOpenAiTagger }));
    entries.push(item('Copy Tags', { icon: <IconBookmark size={15} />, shortcut: kbd('edit.copyTags'), action: ctx.onCopyTags }));
    entries.push(item('Paste Tags', { icon: <IconBookmarks size={15} style={{ transform: 'scaleX(-1)' }} />, shortcut: kbd('edit.pasteTags'), action: ctx.onPasteTags, disabled: !ctx.hasClipboardTags }));
    entries.push(sep());
  }

  // ── Search by Image ──
  if (singleSelected && singleHash && ctx.onSearchByImage) {
    const engines = [
      { key: 'tineye', label: 'TinEye' },
      { key: 'saucenao', label: 'SauceNAO' },
      { key: 'yandex', label: 'Yandex Images' },
      { key: 'bing', label: 'Bing Visual Search' },
    ];
    entries.push({
      submenu: true,
      label: 'Search by Image',
      icon: <IconSearch size={15} />,
      children: engines.map((eng) => ({
        label: eng.label,
        action: () => ctx.onSearchByImage!(eng.key, singleHash!),
      })),
    });
    entries.push(sep());
  }

  // ── Thumbnails ──
  if (hasSelection) {
    const thumbLabel = selectionCount > 1 ? `Regenerate ${selectionCount} Thumbnails` : 'Regenerate Thumbnail';
    entries.push(item(thumbLabel, { icon: <IconRefresh size={15} />, shortcut: kbd('file.regenerateThumbnail'), action: ctx.onRegenerateThumbnails }));
    entries.push(sep());
  }

  // ── Selection ──
  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
  if (hasSelection) {
    entries.push(item('Deselect All', { icon: <IconDeselect size={15} />, shortcut: kbd('edit.deselectAll'), action: ctx.onDeselectAll }));
  }
  entries.push(sep());

  // ── Folder removal ──
  if (scopeKind === 'folder' && hasSelection) {
    const removeLabel = selectionCount > 1 ? `Remove ${selectionCount} from Folder` : 'Remove from Folder';
    entries.push(item(removeLabel, {
      icon: <IconFolderMinus size={15} />,
      shortcut: kbd('file.removeFromFolder'),
      action: ctx.onRemoveFromFolder,
    }));
    entries.push(sep());
  }

  // ── Trash / Delete ──
  // ONLY "Delete Permanently" gets danger:true. Move to Trash is normal.
  if (hasSelection) {
    if (statusFilter === 'trash') {
      const restoreLabel = selectionCount > 1 ? `Restore ${selectionCount} Items` : 'Restore';
      entries.push(item(restoreLabel, { icon: <IconArrowBackUp size={15} />, shortcut: kbd('file.restore'), action: ctx.onRestore }));
      const deleteLabel = selectionCount > 1 ? `Delete ${selectionCount} Permanently` : 'Delete Permanently';
      entries.push(item(deleteLabel, { icon: <IconTrash size={15} />, shortcut: kbd('file.delete'), danger: true, action: ctx.onPermanentDelete }));
    } else {
      const trashLabel = selectionCount > 1 ? `Move ${selectionCount} to Trash` : 'Move to Trash';
      entries.push(item(trashLabel, {
        icon: <IconTrash size={15} />,
        shortcut: kbd('file.delete'),
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
  for (const entry of viewEntries) entries.push(entry);
  entries.push(sep());

  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));

  return entries;
}
