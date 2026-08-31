import {
  IconCopy,
  IconFolderOpen,
  IconFolderPlus,
  IconStar,
  IconStarOff,
  IconTrash,
  IconUpload,
} from '@tabler/icons-react';
import {
  IconChangeIcon,
  IconAutoTags,
  IconNewSubfolder,
  IconRename,
  IconSort,
  IconWatchFolder,
} from '../../shared/ui/icons/sidebar-menu-icons';
import type { MenuEntry } from '../../shared/ui/ContextMenu';
import type { ContentSortField } from '../../platform/folderApi';
import { t } from '../../i18n';

const CONTENT_SORT_OPTIONS: ReadonlyArray<{ field: ContentSortField; label: string }> = [
  { field: 'name', label: t("Name") },
  { field: 'imported_at', label: t("Import Date") },
  { field: 'created_at', label: t("Date Created") },
  { field: 'modified_at', label: t("Date Modified") },
  { field: 'size', label: t("Size") },
  { field: 'notes', label: t("Notes") },
];

export function contentSortSubmenu(
  onSort: (field: ContentSortField) => void,
  fields: readonly ContentSortField[] = CONTENT_SORT_OPTIONS.map(({ field }) => field),
): MenuEntry {
  const included = new Set(fields);
  return {
    submenu: true,
    label: t("Sort by"),
    icon: <IconSort size={14} />,
    children: CONTENT_SORT_OPTIONS
      .filter(({ field }) => included.has(field))
      .map(({ field, label }) => ({ label, action: () => onSort(field) })),
  };
}

interface FolderContextMenuOptions {
  inQuickAccess: boolean;
  watchEnabled: boolean;
  onOpen: () => void;
  onNewSubfolder: () => void;
  onToggleQuickAccess: () => void;
  onRename: () => void;
  onMove: () => void;
  onDuplicate: () => void;
  onSetAutoTags: () => void;
  onImport: () => void;
  onAttachWatch: () => void;
  onRemoveWatch?: () => void;
  onSortTree: (descending: boolean, recursive: boolean) => void;
  onSortContents: (field: ContentSortField) => void;
  iconPickerEntry?: MenuEntry;
  colorPickerEntry?: MenuEntry;
  onExport: () => void;
  onExportPictoPack?: () => void;
  onDelete: () => void;
}

interface BulkFolderContextMenuOptions {
  allInQuickAccess: boolean;
  count: number;
  onToggleQuickAccess: () => void;
  onDuplicate: () => void;
  onMove: () => void;
  onSetAutoTags: () => void;
  onSortContents: (field: ContentSortField) => void;
  onDelete: () => void;
}

/** Folder operations shared by every surface that presents a folder context menu. */
export function buildFolderContextMenu(options: FolderContextMenuOptions): MenuEntry[] {
  return [
    { label: t("Open Folder"), icon: <IconFolderOpen size={14} />, action: options.onOpen },
    { label: t("New Subfolder"), icon: <IconNewSubfolder size={14} />, action: options.onNewSubfolder },
    { separator: true },
    {
      label: options.inQuickAccess ? t("Remove from Quick Access") : t("Add to Quick Access"),
      icon: options.inQuickAccess ? <IconStarOff size={14} /> : <IconStar size={14} />,
      action: options.onToggleQuickAccess,
    },
    { label: t("Rename"), icon: <IconRename size={14} />, action: options.onRename },
    { label: t("Move to..."), icon: <IconFolderOpen size={14} />, action: options.onMove },
    { label: t("Duplicate"), icon: <IconCopy size={14} />, action: options.onDuplicate },
    { label: t("Set Auto Tags..."), icon: <IconAutoTags size={14} />, action: options.onSetAutoTags },
    { separator: true },
    { label: t("Import Folder Here..."), icon: <IconFolderPlus size={14} />, action: options.onImport },
    { label: t("Attach Watched Folder..."), icon: <IconWatchFolder size={14} />, action: options.onAttachWatch },
    ...(options.watchEnabled && options.onRemoveWatch ? [{
      label: t("Remove Watched Folder"),
      icon: <IconWatchFolder size={14} />,
      action: options.onRemoveWatch,
    } satisfies MenuEntry] : []),
    { separator: true },
    {
      submenu: true,
      label: t("Sort Folders"),
      icon: <IconSort size={14} />,
      children: [
        { label: t("This Level A-Z"), action: () => options.onSortTree(false, false) },
        { label: t("This Level Z-A"), action: () => options.onSortTree(true, false) },
        { label: t("This Level and Descendants A-Z"), action: () => options.onSortTree(false, true) },
        { label: t("This Level and Descendants Z-A"), action: () => options.onSortTree(true, true) },
      ],
    },
    contentSortSubmenu(options.onSortContents),
    ...(options.iconPickerEntry || options.colorPickerEntry ? [
      { separator: true } satisfies MenuEntry,
      ...(options.iconPickerEntry ? [{
        submenu: true,
        label: t("Change Icon"),
        icon: <IconChangeIcon size={14} />,
        children: [options.iconPickerEntry],
      } satisfies MenuEntry] : []),
      ...(options.colorPickerEntry ? [options.colorPickerEntry] : []),
    ] : []),
    { separator: true },
    { label: t("Export to Computer..."), icon: <IconUpload size={14} />, action: options.onExport },
    ...(options.onExportPictoPack ? [{
      label: t("Export as Picto Pack..."),
      icon: <IconUpload size={14} />,
      action: options.onExportPictoPack,
    } satisfies MenuEntry] : []),
    { separator: true },
    { label: t("Delete"), icon: <IconTrash size={14} />, danger: true, action: options.onDelete },
  ];
}

export function buildBulkFolderContextMenu(options: BulkFolderContextMenuOptions): MenuEntry[] {
  return [
    {
      label: options.allInQuickAccess ? t("Remove from Quick Access") : t("Add to Quick Access"),
      icon: options.allInQuickAccess ? <IconStarOff size={14} /> : <IconStar size={14} />,
      action: options.onToggleQuickAccess,
    },
    { label: t("Duplicate {value0} Folders", { value0: options.count }), icon: <IconCopy size={14} />, action: options.onDuplicate },
    { label: t("Move to..."), icon: <IconFolderOpen size={14} />, action: options.onMove },
    { label: t("Set Auto Tags..."), icon: <IconAutoTags size={14} />, action: options.onSetAutoTags },
    contentSortSubmenu(options.onSortContents),
    { separator: true },
    {
      label: t("Delete {value0} Folders", { value0: options.count }),
      icon: <IconTrash size={14} />,
      danger: true,
      action: options.onDelete,
    },
  ];
}

export function availableFolderMoveTargets(
  nodes: Array<{ id: string; parent_id?: string | null }>,
  movingFolderId: number,
): number[] {
  const parentById = new Map<number, number | null>();
  for (const node of nodes) {
    const id = parseFolderId(node.id);
    if (id == null) continue;
    parentById.set(id, parseFolderId(node.parent_id ?? ''));
  }
  return [...parentById.keys()].filter((candidate) => {
    if (candidate === movingFolderId) return false;
    let ancestor: number | null | undefined = candidate;
    while (ancestor != null) {
      if (ancestor === movingFolderId) return false;
      ancestor = parentById.get(ancestor);
    }
    return true;
  });
}

export function availableBulkFolderMoveTargets(
  nodes: Array<{ id: string; parent_id?: string | null }>,
  movingFolderIds: number[],
): number[] {
  if (movingFolderIds.length === 0) return [];
  const allowed = new Set(availableFolderMoveTargets(nodes, movingFolderIds[0]));
  for (const folderId of movingFolderIds.slice(1)) {
    const next = new Set(availableFolderMoveTargets(nodes, folderId));
    for (const candidate of allowed) {
      if (!next.has(candidate)) allowed.delete(candidate);
    }
  }
  return [...allowed];
}

/** Moving a selected parent already moves its selected descendants. */
export function topLevelSelectedFolderIds(
  nodes: Array<{ id: string; parent_id?: string | null }>,
  selectedFolderIds: number[],
): number[] {
  const selected = new Set(selectedFolderIds);
  const parentById = new Map<number, number | null>();
  for (const node of nodes) {
    const id = parseFolderId(node.id);
    if (id != null) parentById.set(id, parseFolderId(node.parent_id ?? ''));
  }
  return selectedFolderIds.filter((folderId) => {
    let parent = parentById.get(folderId);
    while (parent != null) {
      if (selected.has(parent)) return false;
      parent = parentById.get(parent);
    }
    return true;
  });
}

function parseFolderId(nodeId: string): number | null {
  if (!nodeId.startsWith('folder:')) return null;
  const id = Number.parseInt(nodeId.slice(7), 10);
  return Number.isNaN(id) ? null : id;
}
