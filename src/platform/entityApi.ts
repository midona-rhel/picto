import { invoke } from './ipc';
import type {
  DetachCollectionInput,
  EntityTarget,
  EntityViewPage,
  EntityViewQuery,
  Lifecycle,
  MediaEntityPatch,
  MutationReceipt,
  MimeFacets,
  OrganizeCollectionInput,
  CollectionNoteDraft,
  OrganizeCollectionResult,
  QueryPage,
  ReorderCollectionInput,
  SelectionSummary,
} from '../shared/types/canonical';

export function queryItems(query: EntityViewQuery, page: QueryPage): Promise<EntityViewPage> {
  return invoke<EntityViewPage>('items.query', { query, page });
}

export interface QueryRequest { channel: string; generation: number }

export type WindowPosition = { kind: 'ordinal'; value: number } | { kind: 'fraction'; value: number };
export interface ItemWindowRequest { position: WindowPosition; limit: number }
export interface EntityViewWindow extends EntityViewPage {
  start: number;
  head_items: EntityViewPage['items'];
}

export async function queryItemWindow(query: EntityViewQuery, window: ItemWindowRequest, request?: QueryRequest, timeoutMs = 10_000): Promise<EntityViewWindow> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      invoke<EntityViewWindow>('items.window', { query, window, request }),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          if (request) void supersedeItemWindow({ ...request, generation: request.generation + 1 })
            .catch(error => console.warn('Could not cancel timed-out grid query', error));
          reject(new Error('The library view took too long to load. Please retry.'));
        }, Math.max(0, timeoutMs));
      }),
    ]);
  } finally {
    if (timer != null) clearTimeout(timer);
  }
}

export function supersedeItemWindow(request: QueryRequest): Promise<void> {
  return invoke('items.supersede_window', { ...request });
}

export function queryMimeFacets(query: EntityViewQuery): Promise<MimeFacets> {
  return invoke<MimeFacets>('items.mime_facets', { query });
}

export function recordMediaView(rootId: number): Promise<unknown> {
  return invoke('items.record_view', { root_id: rootId });
}

export function clearRecentViews(): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.clear_recent_views', {});
}

export function renameItem(rootId: number, name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename', { root_id: rootId, name });
}

export function renameItems(renames: Array<{ root_id: number; name: string }>): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename_many', { renames });
}

export function patchMediaEntities(target: EntityTarget, patch: MediaEntityPatch): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.patch_metadata', { target, patch });
}

export function applyEntityTags(
  target: EntityTarget,
  operation: 'add' | 'remove',
  tags: string[],
): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.apply_tags', {
    target,
    tags,
    add: operation === 'add',
  });
}

export function setItemLifecycle(target: EntityTarget, lifecycle: Lifecycle): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.set_lifecycle', { target, lifecycle });
}

export function deleteItems(target: EntityTarget, requireUndoable = true): Promise<unknown> {
  return invoke('items.delete', { target, require_undoable: requireUndoable });
}

/** Reject Inbox items; dropping the undo requirement needs explicit confirmation. */
export function rejectDeleteItems(target: EntityTarget, requireUndoable = true): Promise<unknown> {
  return invoke('items.reject_delete', { target, require_undoable: requireUndoable });
}

/** Current positions of roots in a view's order; null for a root the view no longer holds. */
export function getRootPositions(query: EntityViewQuery, rootIds: number[]): Promise<Array<number | null>> {
  return invoke<Array<number | null>>('items.positions', { query, root_ids: rootIds });
}

/** Whether each candidate root still belongs to the view, without resolving its order. */
export function getRootMembership(query: EntityViewQuery, rootIds: number[]): Promise<boolean[]> {
  return invoke<boolean[]>('items.membership', { query, root_ids: rootIds });
}

export function getSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return invoke<SelectionSummary>('items.selection_summary', { target });
}

export function resolveImageSelection(target: EntityTarget): Promise<number[]> {
  return invoke<number[]>('items.resolve_image_selection', { target });
}

export function getCollectionNoteDraft(target: EntityTarget): Promise<CollectionNoteDraft> {
  return invoke<CollectionNoteDraft>('items.collection_note_draft', { target });
}

export function organizeIntoGroup(input: OrganizeCollectionInput): Promise<OrganizeCollectionResult> {
  return invoke<OrganizeCollectionResult>('items.organize_into_collection', { ...input });
}

export function detachItems(input: DetachCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.detach', { ...input });
}

/**
 * Ungroup the collections in `target` (other items in it are left alone) as
 * one change, undone in one step. The backend resolves the selection.
 */
export function ungroup(target: EntityTarget): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.ungroup', { target });
}

export function reorderGroup(input: ReorderCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.reorder_collection', { ...input });
}
