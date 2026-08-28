/**
 * Inspector state — persistent right-hand context panel for grid view.
 *
 * Live navigation/selection state is not rendered directly. The inspector reads
 * a displayed grid snapshot so scope content stays aligned with the displayed
 * grid during scope fades.
 */

import { atom } from 'jotai';
import type {
  CanonicalEntityGridItem,
  SidebarNodeDto,
} from '../shared/types/canonical';
import type { ItemFilters } from '../shared/lib/itemFilters';
import { gridActiveAtom } from './grid';
import { displayedSurfaceNodeIdAtom } from './navigation';
import {
  gridSelectionAtom,
  selectedItemIdAtom,
  selectionCountAtom,
  selectedSubfolderNodeIdAtom,
  selectionModeAtom,
} from './selection';
import { sidebarNodesAtom } from './sidebar';
import { viewerSessionAtom } from './viewer';

export type InspectorTarget =
  | { kind: 'none' }
  | { kind: 'scope'; nodeId: string }
  | { kind: 'item'; itemId: number }
  | { kind: 'multi'; count: number; selectionMode: 'explicit' | 'query_results' };

export type DisplayedGridSnapshot = {
  nodeId: string;
  previewItems: CanonicalEntityGridItem[];
  totalCount: number | null;
  totalSizeBytes: number | null;
  searchText: string;
  filters: ItemFilters;
  /** Frozen sidebar node at commit time — prevents live sidebar changes from leaking into inspector during transitions. */
  sidebarNode: SidebarNodeDto | null;
};

export const liveInspectorTargetAtom = atom<InspectorTarget>((get) => {
  if (!get(gridActiveAtom)) return { kind: 'none' };
  const viewerSession = get(viewerSessionAtom);
  if (viewerSession) return { kind: 'item', itemId: viewerSession.currentItemId };
  const selection = get(gridSelectionAtom);
  const selectionMode = get(selectionModeAtom);
  const selectionCount = get(selectionCountAtom);
  if (selectionCount > 0 && selection.folderNodeIds.size > 0) {
    return { kind: 'multi', count: selectionCount + selection.folderNodeIds.size, selectionMode };
  }
  if (selectionMode === 'query_results' && selectionCount > 0) {
    return { kind: 'multi', count: selectionCount, selectionMode };
  }
  if (selectionCount > 1) {
    return { kind: 'multi', count: selectionCount, selectionMode };
  }
  const selectedSubfolderNodeId = get(selectedSubfolderNodeIdAtom);
  if (selectedSubfolderNodeId) {
    return { kind: 'scope', nodeId: selectedSubfolderNodeId };
  }
  const selectedItemId = get(selectedItemIdAtom);
  if (selectedItemId != null) {
    return { kind: 'item', itemId: selectedItemId };
  }
  const displayedNodeId = get(displayedSurfaceNodeIdAtom);
  return displayedNodeId ? { kind: 'scope', nodeId: displayedNodeId } : { kind: 'none' };
});

export const displayedGridSnapshotAtom = atom<DisplayedGridSnapshot | null>(null);
/** Scope label — reads live sidebar node so renames propagate immediately. */
export const displayedScopeLabelAtom = atom((get) => {
  const node = get(displayedSidebarNodeAtom);
  if (node) return node.name;
  const snapshot = get(displayedGridSnapshotAtom);
  if (!snapshot) return '';
  const fallbacks: Record<string, string> = {
    'system:active': 'All',
    'system:inbox': 'Inbox',
    'system:trash': 'Trash',
    'system:uncategorized': 'Uncategorized',
    'system:untagged': 'Untagged',
    'system:random': 'Random',
  };
  return fallbacks[snapshot.nodeId] ?? '';
});

export const displayedInspectorTargetAtom = atom<InspectorTarget>({ kind: 'none' });

/** Preview data for a selected subfolder tile (populated by GridScreen effect). */
export const subfolderPreviewAtom = atom<{
  nodeId: string;
  items: CanonicalEntityGridItem[];
  totalCount: number | null;
  totalSizeBytes: number | null;
} | null>(null);
export const displayedInspectorItemDetailsAtom = atom<import('../shared/types/canonical').CanonicalEntityDetails | null>(null);
export const inspectorLoadingAtom = atom(false);
export const inspectorErrorAtom = atom<string | null>(null);

/** When pinned, inspector ignores selection changes and stays on current entity/scope. */
export const inspectorPinnedAtom = atom(false);

type ScopeInspectorFolderMeta = {
  folder_id?: number;
  notes?: unknown;
  auto_tags?: unknown;
  watch_enabled?: unknown;
};

type ScopeInspectorSmartMeta = {
  smart_folder_id?: number;
  parent_id?: number | null;
  notes?: unknown;
  predicate?: unknown;
};

function asString(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function asBoolean(value: unknown): boolean | null {
  return typeof value === 'boolean' ? value : null;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && item.length > 0)
    : [];
}

export const displayedSidebarNodeAtom = atom<SidebarNodeDto | null>((get) => {
  const snapshot = get(displayedGridSnapshotAtom);
  if (!snapshot) return null;
  // If a subfolder tile is selected, show that folder's node instead of the scope
  const target = get(displayedInspectorTargetAtom);
  const nodeId = (target.kind === 'scope' && target.nodeId !== snapshot.nodeId)
    ? target.nodeId
    : snapshot.nodeId;
  // Read LIVE sidebar node by ID — not the frozen snapshot copy.
  // The frozen copy goes stale after rename/color/meta updates.
  return get(sidebarNodesAtom).find((n) => n.id === nodeId) ?? snapshot.sidebarNode ?? null;
});

export const scopeInspectorViewModelAtom = atom((get) => {
  const snapshot = get(displayedGridSnapshotAtom);
  const node = get(displayedSidebarNodeAtom);
  if (!snapshot || !node || !get(gridActiveAtom)) return null;
  // When showing a selected subfolder (node differs from snapshot scope), use the node's own data
  const isSubfolderSelection = node.id !== snapshot.nodeId;

  const parentName: string | null = null; // Parent lookup removed (no live sidebar read)

  const meta = (node.meta ?? {}) as Record<string, unknown>;
  const folderMeta = meta as ScopeInspectorFolderMeta;
  const smartMeta = meta as ScopeInspectorSmartMeta;

  return {
    node,
    parentName:
      parentName &&
      parentName !== 'Folders' &&
      parentName !== 'Smart Folders' &&
      parentName !== 'Library'
        ? parentName
        : null,
    totalCount: isSubfolderSelection
      ? (get(subfolderPreviewAtom)?.nodeId === node.id ? (get(subfolderPreviewAtom)?.totalCount ?? node.count ?? 0) : (node.count ?? 0))
      : (snapshot.totalCount ?? node.count ?? 0),
    totalSizeBytes: isSubfolderSelection
      ? (get(subfolderPreviewAtom)?.nodeId === node.id ? get(subfolderPreviewAtom)?.totalSizeBytes ?? null : null)
      : (snapshot.totalSizeBytes ?? ((meta as Record<string, unknown>)?.total_size_bytes as number | undefined) ?? null),
    searchText: isSubfolderSelection ? '' : snapshot.searchText,
    previewItems: isSubfolderSelection
      ? (get(subfolderPreviewAtom)?.nodeId === node.id ? get(subfolderPreviewAtom)?.items ?? [] : [])
      : snapshot.previewItems,
    description: null,
    folder:
      node.kind === 'folder'
        ? {
            folderId: typeof folderMeta.folder_id === 'number' ? folderMeta.folder_id : null,
            notes: asString(folderMeta.notes),
            autoTags: asStringArray(folderMeta.auto_tags),
            watchEnabled: asBoolean(folderMeta.watch_enabled),
          }
        : null,
    smartFolder:
      node.kind === 'smart_folder'
        ? {
            smartFolderId:
              typeof smartMeta.smart_folder_id === 'number'
                ? smartMeta.smart_folder_id
                : null,
            parentId:
              typeof smartMeta.parent_id === 'number' || smartMeta.parent_id === null
                ? (smartMeta.parent_id ?? null)
                : null,
            notes: asString(smartMeta.notes),
            predicate: smartMeta.predicate,
          }
        : null,
  };
});
