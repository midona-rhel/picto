import { queryApi } from '#desktop/api';
import { commandApi } from '#desktop/api';
import { bustThumbnailCache } from '../shared/lib/mediaUrl';
import { useGridMetadataStore } from '../state/gridMetadataStore';
import { useDomainStore } from '../state/domainStore';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { markEagerInvalidated } from '../runtime/stateChanges/applyGridRefreshTargets';
import type {
  EntityAllMetadata,
  EntityDetails,
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
  // system:all, system:untagged, system:uncategorized, folder:*, smart:* → all show active
  return 'active';
}

/** Does the target status NOT match the current grid scope?
 *  If so, the item leaves the scope (remove from grid). */
function statusLeavesScope(targetStatus: string): boolean {
  const expected = scopeExpectedStatus();
  if (expected === null) return false; // scope doesn't filter by status
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
  const store = useDomainStore.getState();
  if (status === 'active') return store.allActiveCount;
  if (status === 'inbox') return store.inboxCount;
  if (status === 'trash') return store.trashCount;
  return 0;
}

/** Eagerly adjust system sidebar counts (inbox/active/trash) by the number of
 *  top-level items moved, so the sidebar reflects the change instantly. The
 *  backend event's sidebar_counts will reconcile with the true bitmap count. */
function eagerAdjustSystemCounts(count: number, fromStatus: string | null, toStatus: string): void {
  if (count <= 0 || fromStatus === toStatus) return;
  const store = useDomainStore.getState();
  const s = {
    all_active: store.allActiveCount,
    inbox: store.inboxCount,
    trash: store.trashCount,
  };
  // Decrement source
  if (fromStatus === 'active') s.all_active -= count;
  else if (fromStatus === 'inbox') s.inbox -= count;
  else if (fromStatus === 'trash') s.trash -= count;
  // Increment target
  if (toStatus === 'active') s.all_active += count;
  else if (toStatus === 'inbox') s.inbox += count;
  else if (toStatus === 'trash') s.trash += count;
  store.applySidebarCounts(s);
}

function stableSelectionKey(spec: SelectionQuerySpec): string {
  return JSON.stringify(spec);
}

function fetchMetadata(hash: string): Promise<EntityAllMetadata> {
  const existing = metadataInflight.get(hash);
  if (existing) return existing;
  const request = queryApi.file.getAllMetadata(hash).finally(() => metadataInflight.delete(hash));
  metadataInflight.set(hash, request);
  return request;
}

export const filesController = {
  getEntity(hash: string): Promise<EntityDetails | null> {
    return queryApi.file.get(hash);
  },

  getMetadata(hash: string): Promise<EntityAllMetadata> {
    return fetchMetadata(hash);
  },

  prefetchMetadata(hash: string): void {
    void fetchMetadata(hash);
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

  resolveSelectionHashes(spec: SelectionQuerySpec): Promise<string[]> {
    return queryApi.selection.resolveHashes(spec);
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
    const result = await commandApi.file.setStatus(hash, status);
    eagerInvalidate(hash);
    eagerAdjustSystemCounts(1, previousStatus, status);
    if (statusLeavesScope(status)) {
      useGridMetadataStore.getState().queueRemovals([hash]);
    } else if (statusEntersScope(status)) {
      queryApi.file.get(hash).then((entity) => {
        if (entity) useGridMetadataStore.getState().queueInsertions([entity]);
      });
    }
    return result;
  },

  async setStatusSelection(selection: SelectionQuerySpec, status: string) {
    // The original selection count is the number of top-level items the user
    // selected (e.g. 1 collection = 1). resolveHashes may expand collections
    // to include member files, but the eager sidebar count should reflect the
    // user-visible selection, not the expanded set.
    const selectionCount = selection.hashes?.length ?? 0;
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveHashes(selection);
    const previousStatus = scopeExpectedStatus();
    const result = await commandApi.file.setStatusSelection(selection, status);
    const leaves = statusLeavesScope(status);
    const enters = statusEntersScope(status);
    eagerInvalidateMany(hashes);
    // For explicit selection, use the original selection size (top-level items,
    // not expanded collection members). For virtual select-all, use the current
    // scope count — the entire scope is being moved.
    const eagerCount = selectionCount > 0
      ? selectionCount
      : scopeCount(previousStatus);
    eagerAdjustSystemCounts(eagerCount, previousStatus, status);
    if (leaves) {
      useGridMetadataStore.getState().queueRemovals(hashes);
    } else if (enters) {
      Promise.all(hashes.map((h) => queryApi.file.get(h))).then((entities) => {
        const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
        if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
      });
    }
    return result;
  },

  async updateSelectionRating(selection: SelectionQuerySpec, rating: number | null) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveHashes(selection);
    const result = await commandApi.selection.updateRating(selection, rating);
    eagerInvalidateMany(hashes);
    return result;
  },

  async setSelectionSourceUrls(selection: SelectionQuerySpec, urls: string[]) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveHashes(selection);
    const result = await commandApi.selection.setSourceUrls(selection, urls);
    eagerInvalidateMany(hashes);
    return result;
  },

  async setSelectionNotes(selection: SelectionQuerySpec, notes: Record<string, string>) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveHashes(selection);
    const result = await commandApi.selection.setNotes(selection, notes);
    eagerInvalidateMany(hashes);
    return result;
  },

  async deleteMany(hashes: string[]) {
    const result = await commandApi.file.deleteMany(hashes);
    eagerInvalidateMany(hashes);
    useGridMetadataStore.getState().queueRemovals(hashes);
    return result;
  },

  async deleteSelection(selection: SelectionQuerySpec) {
    const hashes = selection.hashes?.length
      ? selection.hashes
      : await queryApi.selection.resolveHashes(selection);
    const result = await commandApi.file.deleteSelection(selection);
    eagerInvalidateMany(hashes);
    useGridMetadataStore.getState().queueRemovals(hashes);
    return result;
  },

  async updateRating(hash: string, rating: number | null) {
    const result = await commandApi.file.updateRating(hash, rating);
    eagerInvalidate(hash);
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
    const result = await commandApi.file.regenerateThumbnail(hash);
    eagerInvalidate(hash);
    return result;
  },

  async regenerateThumbnailsBatch(hashes: string[]) {
    const result = await commandApi.file.regenerateThumbnailsBatch(hashes);
    bustThumbnailCache(hashes);
    eagerInvalidateMany(hashes);
    return result;
  },

  async reanalyzeColors(hash: string) {
    const result = await commandApi.file.reanalyzeColors(hash);
    eagerInvalidate(hash);
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
    await commandApi.file.setName(hash, newName);
    eagerInvalidate(hash);
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
    for (const item of items) {
      await commandApi.file.setName(item.hash, item.name);
    }
    eagerInvalidateMany(items.map((i) => i.hash));
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
    await commandApi.file.setSourceUrls(hash, urls);
    eagerInvalidate(hash);
    registerUndoAction({
      label: 'Update source URLs',
      backward: async () => { await this.setSourceUrls(hash, previousUrls, urls); },
      forward: async () => { await this.setSourceUrls(hash, urls, previousUrls); },
    });
  },

  /** Set notes. Registers undo (suppressed during undo/redo). */
  async setNotes(hash: string, notes: Record<string, string>, previousNotes: Record<string, string>): Promise<void> {
    await commandApi.file.setNotes(hash, notes);
    eagerInvalidate(hash);
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
