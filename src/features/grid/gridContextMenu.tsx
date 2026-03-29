/**
 * Grid context menu — builds the full entry list for right-click on grid.
 *
 * All 38 legacy items included. Items that need backend wiring are disabled
 * with TODO comments. Functional items are wired to current actions.
 */

import {
  IconArrowsMaximize, IconExternalLink, IconFolderOpen, IconAppWindow,
  IconFolderMinus, IconFolderPlus, IconGitMerge, IconFolderSymlink,
  IconPin, IconPhoto, IconCheck, IconX,
  IconSparkles, IconCursorText, IconCopy, IconCode, IconLink, IconTag, IconTags,
  IconSearch, IconRefresh, IconTrash, IconArrowBackUp,
  IconSelectAll, IconDeselect,
} from '@tabler/icons-react';
import type { MenuItem, MenuSeparator, MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';

const isMac = typeof navigator !== 'undefined' && navigator.platform.includes('Mac');
const mod = isMac ? '⌘' : 'Ctrl+';

interface GridMenuContext {
  /** Number of selected items. */
  selectionCount: number;
  /** Whether selection represents the full query target. */
  querySelectionActive: boolean;
  /** Whether a single entity is selected. */
  singleSelected: boolean;
  /** The single selected entity hash (if exactly one). */
  singleHash: string | null;
  /** Entity kind of the single selection. */
  singleKind: string | null;
  /** Whether any selected item is a collection. */
  hasCollections: boolean;
  /** Current scope kind. */
  scopeKind: 'system' | 'folder' | 'smart_folder' | null;
  /** Status filter: 'inbox' | 'trash' | 'active' | null */
  statusFilter: string | null;
  /** Total loaded items count. */
  loadedCount: number;
  /** Callbacks for functional items. */
  onSelectAll: () => void;
  onDeselectAll: () => void;
  onAddToFolder?: () => void;
  onRemoveFromFolder?: () => void;
  onAcceptInbox?: () => void;
  onRejectInbox?: () => void;
  onMoveToTrash?: () => void;
  onRestore?: () => void;
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

/** Build context menu entries for right-clicking a tile (with selection). */
export function buildTileContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const { selectionCount, singleSelected, singleKind, statusFilter, scopeKind, querySelectionActive } = ctx;
  const hasSelection = selectionCount > 0;
  const entries: MenuEntry[] = [];

  // ── Open actions ──
  if (singleSelected) {
    entries.push(item('Open', { icon: <IconArrowsMaximize size={15} />, shortcut: '↵', disabled: true })); // TODO: detail viewer
    if (singleKind === 'collection') {
      entries.push(item('Edit Collection', { icon: <IconFolderOpen size={15} />, disabled: true })); // TODO: collection editor
    }
    entries.push(item('Open With Default App', { icon: <IconExternalLink size={15} />, shortcut: '⇧↵', disabled: true })); // TODO: shell open
    entries.push(item('Reveal in Finder', { icon: <IconFolderOpen size={15} />, shortcut: `${mod}↵`, disabled: true })); // TODO: shell reveal
    entries.push(item('Open in New Window', { icon: <IconAppWindow size={15} />, shortcut: `${mod}O`, disabled: true })); // TODO: new window
    entries.push(sep());
  }

  // ── Collection operations ──
  if (selectionCount >= 2 && !ctx.hasCollections) {
    entries.push(item('Create Collection', { icon: <IconFolderPlus size={15} />, disabled: true })); // TODO: collection create
  }
  if (selectionCount >= 2 && ctx.hasCollections) {
    entries.push(item('Merge Collections', { icon: <IconGitMerge size={15} />, disabled: true })); // TODO: collection merge
  }
  if (singleSelected && singleKind === 'collection') {
    entries.push(item('Split Collection', { icon: <IconFolderSymlink size={15} />, disabled: true })); // TODO: collection split
  }
  if (entries.length > 0 && entries[entries.length - 1] !== sep()) entries.push(sep());

  // ── Folder management ──
  entries.push(item('Pin to Top', { icon: <IconPin size={15} />, disabled: true })); // TODO: pin
  entries.push(item('Set as Folder Cover', { icon: <IconPhoto size={15} />, disabled: true })); // TODO: folder cover
  entries.push(sep());

  // ── Inbox actions ──
  if (statusFilter === 'inbox' && hasSelection) {
    const acceptLabel = selectionCount > 1 ? `Accept ${selectionCount} Items` : 'Accept';
    const rejectLabel = selectionCount > 1 ? `Reject ${selectionCount} Items` : 'Reject';
    entries.push(item(acceptLabel, { icon: <IconCheck size={15} />, action: ctx.onAcceptInbox }));
    entries.push(item(rejectLabel, { icon: <IconX size={15} />, action: ctx.onRejectInbox }));
    entries.push(sep());
  }

  // ── Organization ──
  if (hasSelection) {
    entries.push(item('Add to Folder...', { icon: <IconFolderPlus size={15} />, shortcut: `${mod}⇧J`, action: ctx.onAddToFolder }));
    entries.push(item('New Folder with Selection', { icon: <IconFolderSymlink size={15} />, disabled: true })); // TODO
    entries.push(item('Auto-Tag...', { icon: <IconSparkles size={15} />, shortcut: `${mod}⇧A`, disabled: true })); // TODO: AI tagger
    entries.push(sep());
  }

  // ── Rename ──
  if (singleSelected) {
    entries.push(item('Rename', { icon: <IconCursorText size={15} />, shortcut: `${mod}R`, disabled: true })); // TODO: inline rename
  }
  if (selectionCount > 1) {
    entries.push(item('Batch Rename...', { icon: <IconCursorText size={15} />, shortcut: `${mod}⇧R`, disabled: true })); // TODO: batch rename
  }
  if (singleSelected || selectionCount > 1) entries.push(sep());

  // ── Copy/Export ──
  if (singleSelected) {
    entries.push(item('Copy', { icon: <IconCopy size={15} />, shortcut: `${mod}C`, disabled: true })); // TODO: clipboard
    entries.push(item('Copy File Path', { icon: <IconCode size={15} />, shortcut: `${mod}⌥C`, disabled: true })); // TODO
    entries.push(item('Copy Name', { icon: <IconCursorText size={15} />, disabled: true })); // TODO
    entries.push(item('Copy as Link', { icon: <IconLink size={15} />, disabled: true })); // TODO
    entries.push(item('Copy Thumbnail', { icon: <IconPhoto size={15} />, disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Tag operations ──
  if (hasSelection) {
    entries.push(item('Copy Tags', { icon: <IconTag size={15} />, shortcut: `${mod}⇧C`, disabled: true })); // TODO
    entries.push(item('Paste Tags', { icon: <IconTags size={15} />, shortcut: `${mod}⇧V`, disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Reverse image search ──
  if (singleSelected) {
    entries.push(item('Search by Image', { icon: <IconSearch size={15} />, disabled: true })); // TODO: submenu
    entries.push(item('Find Visually Similar', { icon: <IconSearch size={15} />, disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Thumbnails ──
  if (hasSelection) {
    const thumbLabel = selectionCount > 1 ? `Regenerate ${selectionCount} Thumbnails` : 'Regenerate Thumbnail';
    entries.push(item(thumbLabel, { icon: <IconRefresh size={15} />, shortcut: `${mod}⇧T`, disabled: true })); // TODO
    entries.push(sep());
  }

  // ── Selection ──
  entries.push(item(querySelectionActive ? 'All Results Selected' : 'Select All Results', {
    icon: <IconSelectAll size={15} />,
    shortcut: `${mod}A`,
    action: querySelectionActive ? undefined : ctx.onSelectAll,
    disabled: querySelectionActive,
  }));
  if (hasSelection) {
    entries.push(item('Deselect All', { icon: <IconDeselect size={15} />, shortcut: 'Esc', action: ctx.onDeselectAll }));
  }
  entries.push(sep());

  // ── Folder-specific removal ──
  if (scopeKind === 'folder' && hasSelection) {
    const removeLabel = selectionCount > 1 ? `Remove ${selectionCount} Items from Folder` : 'Remove from Folder';
    entries.push(item(removeLabel, { icon: <IconFolderMinus size={15} />, shortcut: `${mod}⇧⌫`, action: ctx.onRemoveFromFolder }));
    entries.push(sep());
  }

  // ── Destructive ──
  if (hasSelection) {
    if (statusFilter === 'trash') {
      const restoreLabel = selectionCount > 1 ? `Restore ${selectionCount} Items` : 'Restore';
      entries.push(item(restoreLabel, { icon: <IconArrowBackUp size={15} />, action: ctx.onRestore }));
      const deleteLabel = selectionCount > 1 ? `Permanently Delete ${selectionCount} Items` : 'Permanently Delete';
      entries.push(item(deleteLabel, { icon: <IconTrash size={15} />, danger: true, disabled: true })); // TODO
    } else {
      const trashLabel = selectionCount > 1 ? `Move ${selectionCount} Items to Trash` : 'Move to Trash';
      entries.push(item(trashLabel, { icon: <IconTrash size={15} />, shortcut: `${mod}⌫`, danger: true, action: ctx.onMoveToTrash }));
    }
  }

  return entries;
}

/** Build context menu entries for right-clicking empty grid space. */
export function buildEmptyContextMenu(ctx: GridMenuContext): MenuEntry[] {
  return [
    item(ctx.querySelectionActive ? 'All Results Selected' : 'Select All Results', {
      icon: <IconSelectAll size={15} />,
      shortcut: `${mod}A`,
      action: ctx.querySelectionActive ? undefined : ctx.onSelectAll,
      disabled: ctx.querySelectionActive,
    }),
    sep(),
    item('Import Files...', { icon: <IconFolderPlus size={15} />, disabled: true }), // TODO
    item('Paste', { icon: <IconCopy size={15} />, shortcut: `${mod}V`, disabled: true }), // TODO
  ];
}
