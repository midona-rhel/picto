import { queryApi } from '#desktop/api';
import { commandApi } from '#desktop/api';
import { bustThumbnailCache } from '../shared/lib/mediaUrl';
import { useGridMetadataStore } from '../state-legacy/gridMetadataStore';
import { store as jotaiStore } from '../state/store';
import { scopeCountsAtom, applySidebarCountsAtom } from '../state/sidebar';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { markEagerInvalidated } from '../runtime/stateChanges/applyGridRefreshTargets';
import type {
  EntityAllMetadata,
  EntityDetails,
  EntityGridItem,
  EntityMetadataBatchResponse,
  GridPageSlimQuery,
  GridPageSlimResponse,
  SelectionQuerySpec,
  SelectionSummary,
} from '../shared/types/api';

const metadataInflight = new Map<string, Promise<EntityAllMetadata>>();
const selectionInflight = new Map<string, Promise<SelectionSummary>>();

/** Eagerly invalidate cached metadata so the grid tile updates without waiting
 *  for the backend state-change event roundtrip. */
function eagerInvalidate(hash: string): void {
  metadataInflight.delete(hash);
  useGridMetadataStore.getState().dropCachedMetadata(hash);
  useGridMetadataStore.getState().markMetadataChanged(hash);
  markEagerInvalidated(hash);
}

function eagerInvalidateMany(hashes: string[]): void {
  for (const hash of hashes) {
    metadataInflight.delete(hash);
    useGridMetadataStore.getState().dropCachedMetadata(hash);
    useGridMetadataStore.getState().markMetadataChanged(hash);
    markEagerInvalidated(hash);
  }
}

/** Which status value does the current grid scope expect to see?
 *  Returns null if the scope doesn't filter by status (collection). */
function scopeExpectedStatus(): string | null {
  const scope = useGridMetadataStore.getState().activeGridScope;
  if (!scope) return null;
  if (scope === 'system:inbox') return 'inbox';
  if (scope === 'system:trash') return 'trash';
  // collections don't filter by status
  if (scope.startsWith('collection:')) return null;
  // system:active, system:untagged, system:uncategorized, folder:*, smart:* → all show active
  return 'active';
}

/** Does the target status NOT match the current grid scope?
 *  If so, the item leaves the scope (remove from grid).
 *  Trashing always leaves non-trash scopes (folders, smart folders, etc.). */
function statusLeavesScope(targetStatus: string): boolean {
  const expected = scopeExpectedStatus();
  if (expected === null) {
    // Scope doesn't filter by status (folder, smart folder, tag view).
    // Trashing still removes items from these views since trashed
    // items are only visible in the trash scope.
    return targetStatus === 'trash';
  }
  return targetStatus !== expected;
}

/** Does the target status match the current grid scope?
 *  If so, the item is entering the scope (insert into grid). */
function statusEntersScope(targetStatus: string): boolean {
  const expected = scopeExpectedStatus();
  if (expected === null) return false;
  return targetStatus === expected;
}

/** Get the current count for a system scope status. */
function scopeCount(status: string | null): number {
  const counts = jotaiStore.get(scopeCountsAtom);
  if (status === 'active') return counts.active;
  if (status === 'inbox') return counts.inbox;
  if (status === 'trash') return counts.trash;
  return 0;
}

/** Eagerly adjust system sidebar counts (inbox/active/trash) by the number of
 *  top-level items moved, so the sidebar reflects the change instantly. The
 *  backend event's sidebar_counts will reconcile with the true bitmap count. */
function eagerAdjustSystemCounts(count: number, fromStatus: string | null, toStatus: string): void {
  if (count <= 0 || fromStatus === toStatus) return;
  const counts = { ...jotaiStore.get(scopeCountsAtom) };
  if (fromStatus === 'active') counts.active -= count;
  else if (fromStatus === 'inbox') counts.inbox -= count;
  else if (fromStatus === 'trash') counts.trash -= count;
  if (toStatus === 'active') counts.active += count;
  else if (toStatus === 'inbox') counts.inbox += count;
  else if (toStatus === 'trash') counts.trash += count;
  jotaiStore.set(applySidebarCountsAtom, counts);
}

function stableSelectionKey(spec: SelectionQuerySpec): string {
  return JSON.stringify(spec);
}

function fetchMetadata(hash: string): Promise<EntityAllMetadata> {
  const existing = metadataInflight.get(hash);
  if (existing) return existing;
  const request = queryApi.file.getAllMetadata(hash)
    .finally(() => metadataInflight.delete(hash));
  metadataInflight.set(hash, request);
  return request;
}

export const entityController = {
  getEntityDetails(hash: string): Promise<EntityDetails | null> {
    return queryApi.file.getDetails(hash);
  },

  getMetadata(hash: string): Promise<EntityAllMetadata> {
    return fetchMetadata(hash);
  },

  prefetchMetadata(hash: string): void {
    fetchMetadata(hash).catch(() => {});
  },

  async prefetchMetadataBatch(hashes: string[]): Promise<void> {
    const unique = [...new Set(hashes)].filter(Boolean);
    if (unique.length === 0) return;
    const maxBatch = 200;
    for (let index = 0; index < unique.length; index += maxBatch) {
      await queryApi.grid.getEntitiesMetadataBatch(unique.slice(index, index + maxBatch));
    }
  },

  getMetadataBatch(hashes: string[]): Promise<EntityMetadataBatchResponse> {
    return queryApi.grid.getEntitiesMetadataBatch(hashes);
  },

  getGridItems(hashes: string[]): Promise<EntityGridItem[]> {
    return queryApi.file.getGridItems(hashes);
  },

  getGridPage(query: GridPageSlimQuery): Promise<GridPageSlimResponse> {
    return queryApi.grid.getPageSlim(query);
  },

  getSelectionSummary(spec: SelectionQuerySpec): Promise<SelectionSummary> {
    const key = stableSelectionKey(spec);
    const existing = selectionInflight.get(key);
    if (existing) return existing;
    const request = queryApi.selection.getSummary(spec).finally(() => selectionInflight.delete(key));
    selectionInflight.set(key, request);
    return request;
  },

  resolveSelectionEntityHashes(spec: SelectionQuerySpec): Promise<string[]> {
    return queryApi.selection.resolveEntityHashes(spec);
  },

  findSimilar(hash: string) {
    return queryApi.duplicates.findSimilar(hash);
  },

  noteMetadataChanged(hash: string): void {
    eagerInvalidate(hash);
  },
  noteManyMetadataChanged(hashes: string[]): void {
    eagerInvalidateMany(hashes);
  },
  noteSelectionSummaryChanged(): void {
    selectionInflight.clear();
  },

  resolvePath(hash: string): Promise<string> {
    return queryApi.file.resolvePath(hash);
  },

  resolveThumbnailPath(hash: string): Promise<string> {
    return queryApi.file.resolveThumbnailPath(hash);
  },

  async setStatus(hash: string, status: string) {
    const previousStatus = scopeExpectedStatus();
    // Eager BEFORE the backend call
    eagerInvalidate(hash);
    eagerAdjustSystemCounts(1, previousStatus, status);
    if (statusLeavesScope(status)) {
      useGridMetadataStore.getState().queueRemovals([hash]);
    } else if (statusEntersScope(status)) {
      queryApi.file.getGridItems([hash]).then((entities) => {
        if (entities.length > 0) useGridMetadataStore.getState().queueInsertions(entities);
      });
    }
    const result = await commandApi.file.setStatus(hash, status);
    return result;
  },

  async setStatusSelection(selection: SelectionQuerySpec, status: string) {
    const isExplicit = (selection.hashes?.length ?? 0) > 0;
    const hashes = isExplicit
      ? selection.hashes!
      : await queryApi.selection.resolveEntityHashes(selection);
    const previousStatus = scopeExpectedStatus();
    // Eager BEFORE the backend call
    eagerInvalidateMany(hashes);
    const eagerCount = isExplicit
      ? hashes.length
      : scopeCount(previousStatus);
    eagerAdjustSystemCounts(eagerCount, previousStatus, status);
    if (statusLeavesScope(status)) {
      if (!isExplicit) {
        // Virtual select-all: clear the entire grid rather than trying to
        // match individual entity hashes from a broad selection.
        useGridMetadataStore.getState().queueClearAll();
      } else {
        useGridMetadataStore.getState().queueRemovals(hashes);
      }
    } else if (statusEntersScope(status)) {
      queryApi.file.getGridItems(hashes).then((entities) => {
        if (entities.length > 0) useGridMetadataStore.getState().queueInsertions(entities);
      });
    }
    const result = await commandApi.file.setStatusSelection(selection, status);
    return result;
  },

  async updateSelectionRating(selection: SelectionQuerySpec, rating: number | null) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveEntityHashes(selection);
    eagerInvalidateMany(hashes);
    const result = await commandApi.selection.updateRating(selection, rating);
    return result;
  },

  async setSelectionSourceUrls(selection: SelectionQuerySpec, urls: string[]) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveEntityHashes(selection);
    eagerInvalidateMany(hashes);
    const result = await commandApi.selection.setSourceUrls(selection, urls);
    return result;
  },

  async setSelectionNotes(selection: SelectionQuerySpec, notes: Record<string, string>) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveEntityHashes(selection);
    eagerInvalidateMany(hashes);
    const result = await commandApi.selection.setNotes(selection, notes);
    return result;
  },

  async deleteMany(hashes: string[]) {
    eagerInvalidateMany(hashes);
    useGridMetadataStore.getState().queueRemovals(hashes);
    const result = await commandApi.file.deleteMany(hashes);
    return result;
  },

  async deleteSelection(selection: SelectionQuerySpec) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveEntityHashes(selection);
    eagerInvalidateMany(hashes);
    useGridMetadataStore.getState().queueRemovals(hashes);
    const result = await commandApi.file.deleteSelection(selection);
    return result;
  },

  async updateRating(hash: string, rating: number | null) {
    eagerInvalidate(hash);
    const result = await commandApi.file.updateRating(hash, rating);
    return result;
  },


  openDefault(hash: string) {
    return commandApi.file.openDefault(hash);
  },

  revealInFolder(hash: string) {
    return commandApi.file.revealInFolder(hash);
  },

  openInNewWindow(hash: string, width?: number | null, height?: number | null) {
    return commandApi.file.openInNewWindow(hash, width, height);
  },

  ensureThumbnail(hash: string) {
    return commandApi.file.ensureThumbnail(hash);
  },

  async regenerateThumbnail(hash: string) {
    eagerInvalidate(hash);
    const result = await commandApi.file.regenerateThumbnail(hash);
    return result;
  },

  async regenerateThumbnailsBatch(hashes: string[]) {
    bustThumbnailCache(hashes);
    eagerInvalidateMany(hashes);
    const result = await commandApi.file.regenerateThumbnailsBatch(hashes);
    return result;
  },

  async reanalyzeColors(hash: string) {
    eagerInvalidate(hash);
    const result = await commandApi.file.reanalyzeColors(hash);
    return result;
  },

  // ── Workflow methods (own backend call + eager consequence + undo) ──

  /** Trash selected items with undo. Returns the affected count. */
  async trashSelection(
    spec: SelectionQuerySpec,
    previousStatuses?: Array<{ hash: string; status: string }>,
  ): Promise<number> {
    const count = Number(await this.setStatusSelection(spec, 'trash') ?? 0);
    const specSnapshot = structuredClone(spec);
    if (previousStatuses?.length) {
      // Explicit selection: undo restores each item to its original status.
      const prev = [...previousStatuses];
      registerUndoAction({
        label: `Move ${count} item${count === 1 ? '' : 's'} to trash`,
        backward: async () => {
          const buckets = new Map<string, string[]>();
          for (const item of prev) {
            const bucket = buckets.get(item.status || 'active');
            if (bucket) bucket.push(item.hash);
            else buckets.set(item.status || 'active', [item.hash]);
          }
          for (const [status, hashes] of buckets) {
            await this.setStatusSelection({ ...specSnapshot, mode: 'explicit_hashes', hashes }, status);
          }
        },
        forward: async () => { await this.setStatusSelection(specSnapshot, 'trash'); },
      });
    } else {
      // Virtual all-selection: undo restores them all to the scope's expected status.
      const undoStatus = scopeExpectedStatus() ?? 'active';
      registerUndoAction({
        label: `Move ${count} item${count === 1 ? '' : 's'} to trash`,
        backward: async () => { await this.setStatusSelection(specSnapshot, undoStatus); },
        forward: async () => { await this.setStatusSelection(specSnapshot, 'trash'); },
      });
    }
    return count;
  },

  /** Permanently delete selected items. No undo. */
  async permanentDeleteSelection(spec: SelectionQuerySpec): Promise<number> {
    if (spec.hashes?.length) {
      await this.deleteMany(spec.hashes);
      return spec.hashes.length;
    }
    return Number(await this.deleteSelection(spec) ?? 0);
  },

  /** Restore items from trash to active with undo. */
  async restoreSelection(spec: SelectionQuerySpec): Promise<number> {
    const count = Number(await this.setStatusSelection(spec, 'active') ?? 0);
    const specSnapshot = structuredClone(spec);
    registerUndoAction({
      label: `Restore ${count} item${count === 1 ? '' : 's'}`,
      backward: async () => { await this.setStatusSelection(specSnapshot, 'trash'); },
      forward: async () => { await this.setStatusSelection(specSnapshot, 'active'); },
    });
    return count;
  },

  /** Accept/reject inbox items with undo. */
  async inboxAction(hash: string, status: 'active' | 'trash'): Promise<void> {
    await this.setStatus(hash, status);
    registerUndoAction({
      label: status === 'active' ? 'Accept inbox item' : 'Reject inbox item',
      backward: async () => { await this.setStatus(hash, 'inbox'); },
      forward: async () => { await this.setStatus(hash, status); },
    });
  },

  /** Accept/reject inbox selection with undo. */
  async inboxSelectionAction(spec: SelectionQuerySpec, status: 'active' | 'trash'): Promise<number> {
    const count = Number(await this.setStatusSelection(spec, status) ?? 0);
    const specSnapshot = structuredClone(spec);
    const verb = status === 'active' ? 'Accept' : 'Reject';
    registerUndoAction({
      label: `${verb} ${count} item${count === 1 ? '' : 's'}`,
      backward: async () => { await this.setStatusSelection(specSnapshot, 'inbox'); },
      forward: async () => { await this.setStatusSelection(specSnapshot, status); },
    });
    return count;
  },

  /** Rate selection with undo (captures previous ratings for explicit hashes). */
  async rateSelection(
    spec: SelectionQuerySpec,
    rating: number | null,
    previousRatings?: Map<string, number | null>,
  ): Promise<void> {
    if (spec.hashes?.length) {
      await Promise.all(spec.hashes.map((h) => this.updateRating(h, rating)));
      if (previousRatings) {
        const hashes = [...spec.hashes];
        const prev = new Map(previousRatings);
        registerUndoAction({
          label: `Rate ${hashes.length} item${hashes.length === 1 ? '' : 's'}`,
          backward: async () => { await Promise.all(hashes.map((h) => this.updateRating(h, prev.get(h) ?? null))); },
          forward: async () => { await Promise.all(hashes.map((h) => this.updateRating(h, rating))); },
        });
      }
    } else {
      await this.updateSelectionRating(spec, rating);
      const specSnapshot = structuredClone(spec);
      registerUndoAction({
        label: `Rate all to ${rating ?? 0} stars`,
        backward: async () => { await this.updateSelectionRating(specSnapshot, null); },
        forward: async () => { await this.updateSelectionRating(specSnapshot, rating); },
      });
    }
  },

  /** Rename a file. Registers undo (suppressed during undo/redo execution). */
  async rename(hash: string, newName: string | null, oldName: string | null): Promise<void> {
    eagerInvalidate(hash);
    await commandApi.file.setName(hash, newName);
    registerUndoAction({
      label: 'Rename file',
      backward: async () => { await this.rename(hash, oldName, newName); },
      forward: async () => { await this.rename(hash, newName, oldName); },
    });
  },

  /** Batch rename files. Registers undo (suppressed during undo/redo). */
  async batchRename(
    items: Array<{ hash: string; name: string | null }>,
    previousNames: Array<{ hash: string; name: string | null }>,
  ): Promise<void> {
    eagerInvalidateMany(items.map((i) => i.hash));
    for (const item of items) {
      await commandApi.file.setName(item.hash, item.name);
    }
    const itemsSnapshot = [...items];
    const prevSnapshot = [...previousNames];
    registerUndoAction({
      label: `Rename ${items.length} file${items.length === 1 ? '' : 's'}`,
      backward: async () => { await this.batchRename(prevSnapshot, itemsSnapshot); },
      forward: async () => { await this.batchRename(itemsSnapshot, prevSnapshot); },
    });
  },

  /** Set source URLs. Registers undo (suppressed during undo/redo). */
  async setSourceUrls(hash: string, urls: string[], previousUrls: string[]): Promise<void> {
    eagerInvalidate(hash);
    await commandApi.file.setSourceUrls(hash, urls);
    registerUndoAction({
      label: 'Update source URLs',
      backward: async () => { await this.setSourceUrls(hash, previousUrls, urls); },
      forward: async () => { await this.setSourceUrls(hash, urls, previousUrls); },
    });
  },

  /** Set notes. Registers undo (suppressed during undo/redo). */
  async setNotes(hash: string, notes: Record<string, string>, previousNotes: Record<string, string>): Promise<void> {
    eagerInvalidate(hash);
    await commandApi.file.setNotes(hash, notes);
    registerUndoAction({
      label: 'Update notes',
      backward: async () => { await this.setNotes(hash, previousNotes, notes); },
      forward: async () => { await this.setNotes(hash, notes, previousNotes); },
    });
  },

  /** Change status of a single item. Registers undo. */
  async changeStatus(hash: string, targetStatus: string, previousStatus: string, label: string): Promise<void> {
    await this.setStatus(hash, targetStatus);
    registerUndoAction({
      label,
      backward: async () => { await this.setStatus(hash, previousStatus); },
      forward: async () => { await this.setStatus(hash, targetStatus); },
    });
  },

  /** Change status of a selection (explicit hashes). Registers undo. */
  async changeStatusSelection(
    spec: SelectionQuerySpec,
    targetStatus: string,
    previousStatus: string,
  ): Promise<number> {
    const count = Number(await this.setStatusSelection(spec, targetStatus) ?? 0);
    const specSnapshot = structuredClone(spec);
    const statusLabel = targetStatus === 'inbox' ? 'Inbox' : targetStatus === 'trash' ? 'Trash' : 'Active';
    registerUndoAction({
      label: `Move ${count} item${count === 1 ? '' : 's'} to ${statusLabel}`,
      backward: async () => { await this.setStatusSelection(specSnapshot, previousStatus); },
      forward: async () => { await this.setStatusSelection(specSnapshot, targetStatus); },
    });
    return count;
  },
};
