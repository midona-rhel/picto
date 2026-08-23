import {
  IconArrowsMaximize, IconExternalLink, IconFolderSearch, IconBrandFinder, IconAppWindow,
  IconFolderMinus, IconFolderPlus,
  IconCopy, IconClipboardCopy, IconLink, IconBookmark, IconBookmarks,
  IconRefresh, IconTrash, IconArrowBackUp,
  IconSelectAll, IconDeselect,
  IconSearch,
  IconFileExport, IconFolder, IconStar,
} from '@tabler/icons-react';
import type { MenuItem, MenuSeparator, MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconAutoTag, IconPasteTags, IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
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
  aiTagEnabled?: boolean;
  singleSelected: boolean;
  singleHash: string | null;
  isMixed?: boolean;
  isFoldersOnly?: boolean;
  scopeKind: 'system' | 'folder' | 'smart_folder' | null;
  statusFilter: string | null;
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
  onOpenTagSelect?: () => void;
  onOpenAiTagger?: () => void;
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
  onSearchByImage?: (engine: string, hash: string) => void;
  onSetRating?: (rating: number) => void;
  onExport?: () => void;
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

export function buildTileContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const { selectionCount, singleSelected, singleHash, statusFilter, scopeKind } = ctx;
  const hasSelection = selectionCount > 0;
  const aiTagEnabled = ctx.aiTagEnabled ?? !!ctx.onOpenAiTagger;
  const entries: MenuEntry[] = [];

  if (ctx.isMixed) {
    entries.push(item('Select All', { shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
    entries.push(item('Deselect All', { action: ctx.onDeselectAll }));
    if (ctx.onMoveToTrash) {
      entries.push(sep());
      entries.push(item('Move to Trash', { icon: <IconTrash size={15} />, action: ctx.onMoveToTrash }));
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

  if (statusFilter === 'inbox' && hasSelection) {
    const acceptLabel = selectionCount > 1 ? `Accept ${selectionCount} Items` : 'Accept';
    entries.push(item(acceptLabel, { icon: <IconArrowBackUp size={15} />, shortcut: kbd('inbox.accept'), action: ctx.onAccept }));
    const rejectLabel = selectionCount > 1 ? `Reject ${selectionCount} Items` : 'Reject';
    entries.push(item(rejectLabel, { icon: <IconTrash size={15} />, shortcut: kbd('inbox.reject'), action: ctx.onReject }));
    entries.push(sep());
  }

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

  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) entries.push(entry);
  entries.push(sep());

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
    entries.push(sep());
  }

  if (singleSelected) {
    entries.push(item('Rename', { icon: <IconRename size={15} />, shortcut: kbd('edit.rename'), action: ctx.onRename }));
  }
  if (singleSelected) {
    entries.push(sep());
  }

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

  if (hasSelection) {
    entries.push(item('Add Tags', { icon: <IconBookmark size={15} />, shortcut: kbd('organize.addTag'), action: ctx.onOpenTagSelect }));
    entries.push(item(selectionCount > 1 ? `Auto Tag ${selectionCount} Images` : 'Auto Tag', {
      icon: <IconAutoTag size={15} />,
      shortcut: kbd('organize.autoTag'),
      action: ctx.onOpenAiTagger,
      disabled: !aiTagEnabled,
    }));
    entries.push(item('Copy Tags', { icon: <IconBookmarks size={15} />, shortcut: kbd('edit.copyTags'), action: ctx.onCopyTags }));
    entries.push(item('Paste Tags', { icon: <IconPasteTags size={15} />, shortcut: kbd('edit.pasteTags'), action: ctx.onPasteTags, disabled: !ctx.hasClipboardTags }));
    entries.push(sep());
  }

  if (hasSelection && ctx.onSetRating) {
    entries.push({
      submenu: true,
      label: 'Set Rating',
      icon: <IconStar size={15} />,
      children: [0, 1, 2, 3, 4, 5].map((r) => ({
        label: r === 0 ? 'No Rating' : '★'.repeat(r),
        action: () => ctx.onSetRating!(r),
      })),
    });
  }

  if (hasSelection && ctx.onExport) {
    entries.push(item('Export...', { icon: <IconFileExport size={15} />, action: ctx.onExport }));
    entries.push(sep());
  }

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

  if (hasSelection) {
    const thumbLabel = selectionCount > 1 ? `Regenerate ${selectionCount} Thumbnails` : 'Regenerate Thumbnail';
    entries.push(item(thumbLabel, { icon: <IconRefresh size={15} />, shortcut: kbd('file.regenerateThumbnail'), action: ctx.onRegenerateThumbnails }));
    entries.push(sep());
  }

  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));
  if (hasSelection) {
    entries.push(item('Deselect All', { icon: <IconDeselect size={15} />, shortcut: kbd('edit.deselectAll'), action: ctx.onDeselectAll }));
  }
  entries.push(sep());

  if (scopeKind === 'folder' && hasSelection) {
    const removeLabel = selectionCount > 1 ? `Remove ${selectionCount} from Folder` : 'Remove from Folder';
    entries.push(item(removeLabel, {
      icon: <IconFolderMinus size={15} />,
      shortcut: kbd('file.removeFromFolder'),
      action: ctx.onRemoveFromFolder,
    }));
    entries.push(sep());
  }

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

export function buildEmptyContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const entries: MenuEntry[] = [];

  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) entries.push(entry);
  entries.push(sep());

  entries.push(item('Select All', { icon: <IconSelectAll size={15} />, shortcut: kbd('edit.selectAll'), action: ctx.onSelectAll }));

  return entries;
}
