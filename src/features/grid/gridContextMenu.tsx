/**
 * Grid context menu — context-aware entry builder for right-click on grid tiles.
 *
 * Rules:
 *   - Only "Delete Permanently" is ever danger (red). Everything else is normal.
 *   - Reject and Move to Trash use the trash icon, never custom icons.
 *   - Accept/Reject appear at the top in inbox scope.
 */

import {
  IconApps, IconExternalLink, IconFolderSearch, IconBrandFinder, IconAppWindow,
  IconFolderMinus, IconFolderPlus,
  IconCopy, IconClipboardCopy, IconLink, IconBookmark, IconBookmarks,
  IconRefresh, IconTrash, IconArrowBackUp, IconClipboard, IconFilterPlus, IconFileImport,
  IconDots, IconSearch,
  IconContrast, IconFileExport, IconFolder, IconPhoto, IconStar,
} from '@tabler/icons-react';
import type { MenuItem, MenuSeparator, MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import { IconAutoTag, IconPasteTags, IconRename } from '../../shared/ui/icons/sidebar-menu-icons';
import { IconFolderNewSelection } from '../../shared/ui/IconPicker/customIcons';
import { buildContextMenuViewEntries } from './GridViewMenu';
import { getShortcut, formatKeysDisplay } from '../../shared/lib/shortcuts';
import {
  GroupCreateIcon,
  GroupEditIcon,
  GroupRemoveIcon,
} from '../../shared/ui/icons/group-icons';
import {
  REVERSE_IMAGE_SEARCH_ENGINES,
  type OpenWithOptions,
  type ReverseImageSearchEngine,
} from '../../platform/shellApi';
import type { ContentSortField } from '../../platform/folderApi';
import { contentSortSubmenu } from '../folders/folderContextMenu';
import { t } from '../../i18n';

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

function kbd(id: string): string | undefined {
  const def = getShortcut(id);
  return def ? formatKeysDisplay(def.keys) : undefined;
}

export interface GridMenuContext {
  surface?: 'grid' | 'viewer';
  selectionCount: number;
  querySelectionActive: boolean;
  aiTagEnabled?: boolean;
  singleSelected: boolean;
  singleHash: string | null;
  singleItemId?: number | null;
  hasFolders?: boolean;
  isMixed?: boolean;
  isFoldersOnly?: boolean;
  scopeKind: 'system' | 'folder' | 'smart_folder' | null;
  statusFilter: string | null;
  loadedCount: number;
  onOpen?: () => void;
  onOpenDefault?: (hash: string) => void;
  openWithOptions?: OpenWithOptions | null;
  openWithPending?: boolean;
  onOpenWithApplication?: (hash: string, applicationPath: string) => void;
  onOpenWithChooser?: (hash: string) => void;
  onRevealInFolder?: (hash: string) => void;
  onCopyFilePath?: (hash: string) => void;
  onCopyFile?: (hash: string) => void;
  onCopySelection?: () => void;
  onCopySelectionPaths?: () => void;
  onCopySelectionNames?: () => void;
  onCopySelectionLinks?: () => void;
  onCopyName?: (name: string) => void;
  onMoveToTrash?: () => void;
  onRestore?: () => void;
  onPermanentDelete?: () => void;
  onOpenNewWindow?: () => void;
  lastUsedFolderName?: string | null;
  onAddToFolder?: () => void;
  onAddToLastUsedFolder?: () => void;
  onRemoveFromFolder?: () => void;
  onOpenTagSelect?: () => void;
  onOpenAiTagger?: () => void;
  onAccept?: () => void;
  onReject?: () => void;
  onRename?: () => void;
  onBatchRename?: () => void;
  onRegenerateThumbnails?: () => void;
  onSetLibraryCover?: (hash: string) => void;
  onSetFolderCover?: () => void;
  onCopyTags?: () => void;
  onPasteTags?: () => void;
  hasClipboardTags?: boolean;
  singleName?: string | null;
  singleMime?: string | null;
  singleKind?: 'media' | 'collection' | null;
  containsGroup?: boolean;
  onCopyLink?: (link: string) => void;
  onNewFolderWithSelection?: () => void;
  onFindMediaMatches?: (itemId: number) => void;
  onSearchByImage?: (engine: ReverseImageSearchEngine, hash: string) => void;
  onSetRating?: (rating: number) => void;
  onExport?: () => void;
  onExportOriginals?: () => void;
  onExportPictoPack?: () => void;
  onOrganizeGroup?: () => void;
  onEditGroup?: () => void;
  onUngroup?: () => void;
  onNewFolder?: () => void;
  onNewSmartFolder?: () => void;
  onImportFiles?: () => void;
  onImportFolder?: () => void;
  onPasteImport?: () => void;
  onSortContents?: (field: ContentSortField) => void;
  sortFields?: readonly ContentSortField[];
  grayscale?: boolean;
  onToggleGrayscale?: () => void;
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
    checked?: boolean;
    keepOpen?: boolean;
  } = {},
): MenuItem {
  return {
    label,
    icon: opts.icon,
    shortcut: opts.shortcut,
    action: opts.action ?? (() => {}),
    danger: opts.danger,
    disabled: opts.disabled ?? !opts.action,
    checked: opts.checked,
    keepOpen: opts.keepOpen,
  };
}

export function buildLibraryCoverContextEntry(
  hash: string,
  onSetLibraryCover: (hash: string) => void,
): MenuItem {
  return item(t("Set as Library Cover"), {
    icon: <IconPhoto size={15} />,
    action: () => onSetLibraryCover(hash),
  });
}

export function buildExportContextEntry({
  onExportOriginals,
  onExportAs,
  onExportPictoPack,
}: {
  onExportOriginals?: () => void;
  onExportAs?: () => void;
  onExportPictoPack?: () => void;
}): MenuEntry {
  return {
    submenu: true,
    label: t("Export"),
    icon: <IconFileExport size={15} />,
    children: [
      item(t("Export Originals..."), { action: onExportOriginals }),
      item(t("Export As..."), { action: onExportAs }),
      ...(onExportPictoPack ? [item(t("Export as Picto Pack..."), { action: onExportPictoPack })] : []),
    ],
  };
}

export function buildEntityOpenContextEntries({
  hash,
  onOpenDefault,
  openWithOptions,
  openWithPending,
  onOpenWithApplication,
  onOpenWithChooser,
  onRevealInFolder,
  onOpenNewWindow,
}: {
  hash: string;
  onOpenDefault?: (hash: string) => void;
  openWithOptions?: OpenWithOptions | null;
  openWithPending?: boolean;
  onOpenWithApplication?: (hash: string, applicationPath: string) => void;
  onOpenWithChooser?: (hash: string) => void;
  onRevealInFolder?: (hash: string) => void;
  onOpenNewWindow?: () => void;
}): MenuEntry[] {
  const entries: MenuEntry[] = [];
  if (onOpenDefault) {
    entries.push(item(t("Open with Default App"), {
      icon: <IconExternalLink size={15} />,
      shortcut: kbd('file.openDefaultApp'),
      action: () => onOpenDefault(hash),
    }));
  }
  if (openWithPending) {
    entries.push({
      submenu: true,
      label: t("Open With Other"),
      icon: <IconApps size={15} />,
      children: [item(t("Loading applications..."), { disabled: true })],
    });
  } else if (openWithOptions?.mode === 'submenu' && openWithOptions.applications.length > 0 && onOpenWithApplication) {
    entries.push({
      submenu: true,
      label: t("Open With Other"),
      icon: <IconApps size={15} />,
      children: openWithOptions.applications.map((application) => ({
        label: application.isDefault ? t("{value0} (Default)", { value0: application.name }) : application.name,
        icon: application.iconDataUrl
          ? <img alt="" height={16} src={application.iconDataUrl} width={16} />
          : <IconApps size={15} />,
        action: () => onOpenWithApplication(hash, application.path),
      })),
    });
  } else if (openWithOptions?.mode === 'chooser' && onOpenWithChooser) {
    entries.push(item(t("Open With Other..."), {
      icon: <IconApps size={15} />,
      shortcut: kbd('file.openOtherApp'),
      action: () => onOpenWithChooser(hash),
    }));
  }
  if (onRevealInFolder) {
    entries.push(item(isMac ? t("Reveal in Finder") : t("Show in Explorer"), {
      icon: isMac ? <IconBrandFinder size={15} /> : <IconFolderSearch size={15} />,
      shortcut: kbd('file.revealInFolder'),
      action: () => onRevealInFolder(hash),
    }));
  }
  if (onOpenNewWindow) {
    entries.push(item(t("Open in New Window"), {
      icon: <IconAppWindow size={15} />,
      shortcut: kbd('file.openNewWindow'),
      action: onOpenNewWindow,
    }));
  }
  return entries;
}

export function mediaLinkFor(hash: string, mime: string): string {
  const extensions: Record<string, string> = {
    'image/jpeg': 'jpg',
    'image/png': 'png',
    'image/gif': 'gif',
    'image/webp': 'webp',
    'video/mp4': 'mp4',
    'video/webm': 'webm',
  };
  return `media://localhost/file/${hash}.${extensions[mime] ?? 'bin'}`;
}

export function buildEntityCopyContextEntries({
  hash,
  name,
  mime,
  onCopyFile,
  onCopyFilePath,
  onCopyName,
  onCopyLink,
}: {
  hash: string;
  name?: string | null;
  mime?: string | null;
  onCopyFile?: (hash: string) => void;
  onCopyFilePath?: (hash: string) => void;
  onCopyName?: (name: string) => void;
  onCopyLink?: (link: string) => void;
}): MenuEntry[] {
  const entries: MenuEntry[] = [];
  if (onCopyFile) {
    entries.push(item(t("Copy"), {
      icon: <IconCopy size={15} />,
      shortcut: kbd('edit.copy'),
      action: () => onCopyFile(hash),
    }));
  }
  if (onCopyFilePath) {
    entries.push(item(t("Copy File Path"), {
      icon: <IconClipboardCopy size={15} />,
      shortcut: kbd('edit.copyFilePath'),
      action: () => onCopyFilePath(hash),
    }));
  }
  if (name && onCopyName) {
    entries.push(item(t("Copy Name"), {
      icon: <IconClipboardCopy size={15} />,
      action: () => onCopyName(name),
    }));
  }
  if (mime && onCopyLink) {
    entries.push(item(t("Copy as Link"), {
      icon: <IconLink size={15} />,
      action: () => onCopyLink(mediaLinkFor(hash, mime)),
    }));
  }
  return entries;
}

/** Build context menu entries for right-clicking a tile. */
export function buildTileContextMenu(ctx: GridMenuContext): MenuEntry[] {
  const { selectionCount, singleSelected, singleHash, statusFilter, scopeKind } = ctx;
  const viewerSurface = ctx.surface === 'viewer';
  const hasSelection = selectionCount > 0;
  const aiTagEnabled = ctx.aiTagEnabled ?? !!ctx.onOpenAiTagger;
  const canSetLibraryCover = singleSelected
    && Boolean(singleHash)
    && ctx.singleKind === 'media'
    && Boolean(ctx.onSetLibraryCover);
  const entries: MenuEntry[] = [];
  const moreEntries: MenuEntry[] = [];

  // ── Mixed selection (folders + entities) or folders-only: limited menu ──
  if (ctx.isMixed) {
    if (ctx.onMoveToTrash) {
      entries.push(item(t("Move to Trash"), { icon: <IconTrash size={15} />, action: ctx.onMoveToTrash }));
    }
    return entries;
  }
  if (ctx.isFoldersOnly) {
    if (singleSelected && singleHash?.startsWith('folder:')) {
      entries.push(item(t("Open Folder"), { icon: <IconFolder size={15} />, action: ctx.onOpen }));
      entries.push(sep());
    }
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
    const openEntries: MenuEntry[] = [];
    if (ctx.singleKind === 'collection') {
      if (ctx.onOpenNewWindow) {
        openEntries.push(item(t("Open in New Window"), {
          icon: <IconAppWindow size={15} />,
          shortcut: kbd('file.openNewWindow'),
          action: ctx.onOpenNewWindow,
        }));
      }
    } else if (singleHash) {
      openEntries.push(...buildEntityOpenContextEntries({
        hash: singleHash,
        onOpenDefault: ctx.onOpenDefault,
        openWithOptions: ctx.openWithOptions,
        openWithPending: ctx.openWithPending,
        onOpenWithApplication: ctx.onOpenWithApplication,
        onOpenWithChooser: ctx.onOpenWithChooser,
        onRevealInFolder: ctx.onRevealInFolder,
        onOpenNewWindow: ctx.onOpenNewWindow,
      }));
    }
    entries.push(...openEntries);
    if (openEntries.length > 0) entries.push(sep());
  }

  if (canSetLibraryCover) {
    moreEntries.push(buildLibraryCoverContextEntry(singleHash!, ctx.onSetLibraryCover!));
  }
  if (singleSelected && ctx.singleItemId != null && ctx.onFindMediaMatches) {
    moreEntries.push(item(t("Find Items with This Media"), {
      icon: <IconSearch size={15} />,
      action: () => ctx.onFindMediaMatches!(ctx.singleItemId!),
    }));
  }

  // ── View options ──
  if (!viewerSurface) {
    if (ctx.onSortContents) {
      entries.push(contentSortSubmenu(ctx.onSortContents, ctx.sortFields));
      entries.push(sep());
    }
    const viewEntries = buildContextMenuViewEntries();
    for (const entry of viewEntries) entries.push(entry);
    entries.push(item(t("Grayscale"), {
      icon: <IconContrast size={15} />,
      shortcut: kbd('view.grayscale'),
      action: ctx.onToggleGrayscale,
      checked: ctx.grayscale ?? false,
      keepOpen: true,
    }));
    entries.push(sep());
  }

  // ── Organization ──
  if (hasSelection) {
    if (ctx.onAddToLastUsedFolder) {
      entries.push(item(ctx.lastUsedFolderName ? t("Add to “{value0}”", { value0: ctx.lastUsedFolderName }) : t("Add to Last Used Folder"), {
        icon: <IconFolderPlus size={15} />,
        shortcut: kbd('file.addToLastFolder'),
        action: ctx.onAddToLastUsedFolder,
      }));
    }
    entries.push(item(t("Add to Folder"), {
      icon: <IconFolderPlus size={15} />,
      shortcut: kbd('file.addToFolder'),
      action: ctx.onAddToFolder,
    }));
    if (ctx.onNewFolderWithSelection) {
      entries.push(item(t("New Folder with Selection"), {
        icon: <IconFolderNewSelection size={15} />,
        action: ctx.onNewFolderWithSelection,
      }));
    }
    if (selectionCount > 1 && ctx.onOrganizeGroup) {
      entries.push(item(t("Group..."), {
        icon: <GroupCreateIcon size={15} />,
        action: ctx.onOrganizeGroup,
      }));
    }
    if (singleSelected && ctx.singleKind === 'collection') {
      entries.push(item(t("Edit Group"), {
        icon: <GroupEditIcon size={15} />,
        action: ctx.onEditGroup,
      }));
      entries.push(item(t("Ungroup..."), {
        icon: <GroupRemoveIcon size={15} />,
        action: ctx.onUngroup,
      }));
    }
    entries.push(sep());
  }

  // ── Rename ──
  if (singleSelected && ctx.onRename) {
    entries.push(item(t("Rename"), { icon: <IconRename size={15} />, shortcut: kbd('edit.rename'), action: ctx.onRename }));
    entries.push(sep());
  }
  if (singleSelected && ctx.scopeKind === 'folder' && ctx.onSetFolderCover) {
    moreEntries.push(item(t("Set as Folder Cover"), {
      icon: <IconPhoto size={15} />,
      action: ctx.onSetFolderCover,
    }));
  }
  if (selectionCount > 1 && !ctx.querySelectionActive && ctx.onBatchRename) {
    entries.push(item(t("Batch Rename {value0} Items...", { value0: selectionCount }), {
      icon: <IconRename size={15} />, shortcut: kbd('edit.rename'), action: ctx.onBatchRename,
    }));
    entries.push(sep());
  }

  // ── Copy ──
  if (singleSelected && singleHash && ctx.singleKind !== 'collection') {
    entries.push(...buildEntityCopyContextEntries({
      hash: singleHash,
      name: ctx.singleName,
      mime: ctx.singleMime,
      onCopyFile: ctx.onCopyFile,
      onCopyFilePath: ctx.onCopyFilePath,
      onCopyName: ctx.onCopyName,
      onCopyLink: ctx.onCopyLink,
    }));
    entries.push(sep());
  } else if (hasSelection && !ctx.querySelectionActive && ctx.onCopySelection) {
    entries.push(item(t("Copy"), {
      icon: <IconCopy size={15} />,
      shortcut: kbd('edit.copy'),
      action: ctx.onCopySelection,
    }));
    if (ctx.onCopySelectionPaths) {
      entries.push(item(t("Copy File Paths"), {
        icon: <IconClipboardCopy size={15} />,
        shortcut: kbd('edit.copyFilePath'),
        action: ctx.onCopySelectionPaths,
      }));
    }
    if (ctx.onCopySelectionNames) {
      entries.push(item(selectionCount === 1 ? t("Copy Name") : t("Copy Names"), {
        icon: <IconClipboardCopy size={15} />,
        action: ctx.onCopySelectionNames,
      }));
    }
    if (ctx.onCopySelectionLinks) {
      entries.push(item(t("Copy as Links"), {
        icon: <IconLink size={15} />,
        action: ctx.onCopySelectionLinks,
      }));
    }
    entries.push(sep());
  }

  // ── Tags ──
  if (hasSelection) {
    entries.push(item(t("Add Tags"), { icon: <IconBookmark size={15} />, shortcut: kbd('organize.addTag'), action: ctx.onOpenTagSelect }));
    entries.push(item(selectionCount > 1 ? t("Auto Tag {value0} Images", { value0: selectionCount }) : t("Auto Tag"), {
      icon: <IconAutoTag size={15} />,
      shortcut: kbd('organize.autoTag'),
      action: ctx.onOpenAiTagger,
      disabled: !aiTagEnabled,
    }));
    entries.push(item(singleSelected ? t("Copy Tags") : t("Copy Shared Tags"), {
      icon: <IconBookmarks size={15} />, shortcut: kbd('edit.copyTags'), action: ctx.onCopyTags,
    }));
    entries.push(item(t("Paste Tags"), { icon: <IconPasteTags size={15} />, shortcut: kbd('edit.pasteTags'), action: ctx.onPasteTags, disabled: !ctx.hasClipboardTags }));
    entries.push(sep());
  }

  // ── Rating ──
  if (hasSelection && ctx.onSetRating) {
    entries.push({
      submenu: true,
      label: t("Set Rating"),
      icon: <IconStar size={15} />,
      children: [0, 1, 2, 3, 4, 5].map((r) => ({
        label: r === 0 ? t("No Rating") : '★'.repeat(r),
        action: () => ctx.onSetRating!(r),
      })),
    });
  }

  // ── Export ──
  if (hasSelection && ctx.onExport) {
    entries.push(buildExportContextEntry({
      onExportOriginals: ctx.onExportOriginals,
      onExportAs: ctx.onExport,
      onExportPictoPack: ctx.onExportPictoPack,
    }));
    entries.push(sep());
  }

  // ── External media search ──
  if (singleSelected && singleHash && ctx.singleMime?.startsWith('image/') && ctx.onSearchByImage) {
    entries.push({
      submenu: true,
      label: t("Search by Image"),
      icon: <IconSearch size={15} />,
      children: REVERSE_IMAGE_SEARCH_ENGINES.map((eng) => ({
        label: eng.label,
        action: () => ctx.onSearchByImage!(eng.key, singleHash!),
      })),
    });
    entries.push(sep());
  }

  // ── Less frequent library maintenance ──
  if (hasSelection && !ctx.containsGroup) {
    const thumbLabel = selectionCount > 1 ? `Regenerate ${selectionCount} Thumbnails` : 'Regenerate Thumbnail';
    moreEntries.push(item(thumbLabel, { icon: <IconRefresh size={15} />, shortcut: kbd('file.regenerateThumbnail'), action: ctx.onRegenerateThumbnails }));
  }
  if (moreEntries.length > 0) {
    entries.push({
      submenu: true,
      label: t("More"),
      icon: <IconDots size={15} />,
      children: moreEntries,
    });
    entries.push(sep());
  }

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

  if (ctx.onNewFolder && ctx.scopeKind !== 'smart_folder') {
    entries.push(item(t("New Folder"), {
      icon: <IconFolderPlus size={15} />,
      shortcut: kbd('file.newFolder'),
      action: ctx.onNewFolder,
    }));
  }
  if (ctx.onNewSmartFolder && ctx.scopeKind !== 'folder' && ctx.scopeKind !== 'smart_folder') {
    entries.push(item(t("New Smart Folder"), {
      icon: <IconFilterPlus size={15} />,
      shortcut: kbd('file.newSmartFolder'),
      action: ctx.onNewSmartFolder,
    }));
  }
  if ((ctx.onNewFolder && ctx.scopeKind !== 'smart_folder')
    || (ctx.onNewSmartFolder && ctx.scopeKind !== 'folder' && ctx.scopeKind !== 'smart_folder')) entries.push(sep());

  if (ctx.onImportFiles) entries.push(item(t("Import Files..."), {
    icon: <IconFileImport size={15} />,
    shortcut: kbd('file.import'),
    action: ctx.onImportFiles,
  }));
  if (ctx.onImportFolder) entries.push(item(t("Import Folder..."), {
    icon: <IconFolderPlus size={15} />,
    action: ctx.onImportFolder,
  }));
  if (ctx.onPasteImport) entries.push(item(t("Paste Import"), {
    icon: <IconClipboard size={15} />,
    shortcut: kbd('edit.paste'),
    action: ctx.onPasteImport,
  }));
  if (ctx.onImportFiles || ctx.onImportFolder || ctx.onPasteImport) entries.push(sep());

  if (ctx.onSortContents) {
    entries.push(contentSortSubmenu(ctx.onSortContents));
    entries.push(sep());
  }

  const viewEntries = buildContextMenuViewEntries();
  for (const entry of viewEntries) entries.push(entry);
  entries.push(sep());

  return entries;
}
