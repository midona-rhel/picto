/**
 * Inspector state — persistent right-hand context panel for grid view.
 *
 * Live navigation/selection state is not rendered directly. The inspector reads
 * a displayed grid snapshot so scope content stays aligned with the displayed
 * grid during scope fades.
 */

import { atom } from 'jotai';
import type {
  CanonicalEntityDetails,
  CanonicalEntityGridItem,
  SidebarNodeDto,
} from '../shared/types/canonical';
import { gridActiveAtom } from './grid';
import { activeNodeIdAtom } from './navigation';
import { selectedEntityHashAtom } from './selection';
import { sidebarNodesAtom } from './sidebar';

export type InspectorTarget =
  | { kind: 'none' }
  | { kind: 'scope'; nodeId: string }
  | { kind: 'entity'; entityHash: string };

export type DisplayedGridSnapshot = {
  nodeId: string;
  previewItems: CanonicalEntityGridItem[];
  totalCount: number | null;
  totalSizeBytes: number | null;
  searchText: string;
};

export const liveInspectorTargetAtom = atom<InspectorTarget>((get) => {
  if (!get(gridActiveAtom)) return { kind: 'none' };
  const selectedHash = get(selectedEntityHashAtom);
  if (selectedHash) return { kind: 'entity', entityHash: selectedHash };
  const activeNodeId = get(activeNodeIdAtom);
  return activeNodeId ? { kind: 'scope', nodeId: activeNodeId } : { kind: 'none' };
});

export const displayedGridSnapshotAtom = atom<DisplayedGridSnapshot | null>(null);
export const displayedInspectorTargetAtom = atom<InspectorTarget>({ kind: 'none' });
export const displayedInspectorEntityDataAtom = atom<CanonicalEntityDetails | null>(null);
export const inspectorLoadingAtom = atom(false);
export const inspectorErrorAtom = atom<string | null>(null);

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
  sort_field?: unknown;
  sort_order?: unknown;
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

const SYSTEM_SCOPE_DESCRIPTIONS: Record<string, string> = {
  'system:active': 'All active media in the current library.',
  'system:inbox': 'New or unreviewed media waiting to be processed.',
  'system:trash': 'Media currently marked for removal.',
  'system:uncategorized': 'Active media that is not assigned to any folder.',
  'system:untagged': 'Active media that has no tags yet.',
};

export const displayedSidebarNodeAtom = atom<SidebarNodeDto | null>((get) => {
  const snapshot = get(displayedGridSnapshotAtom);
  if (!snapshot) return null;
  return get(sidebarNodesAtom).find((node) => node.id === snapshot.nodeId) ?? null;
});

export const scopeInspectorViewModelAtom = atom((get) => {
  const snapshot = get(displayedGridSnapshotAtom);
  const node = get(displayedSidebarNodeAtom);
  if (!snapshot || !node || !get(gridActiveAtom)) return null;

  const nodes = get(sidebarNodesAtom);
  const parentName = node.parent_id
    ? nodes.find((candidate) => candidate.id === node.parent_id)?.name ?? null
    : null;

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
    totalCount: snapshot.totalCount ?? node.count ?? 0,
    totalSizeBytes: snapshot.totalSizeBytes,
    searchText: snapshot.searchText,
    previewItems: snapshot.previewItems,
    description: node.kind === 'system' ? SYSTEM_SCOPE_DESCRIPTIONS[node.id] ?? null : null,
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
            sortField: asString(smartMeta.sort_field),
            sortOrder: asString(smartMeta.sort_order),
          }
        : null,
  };
});
