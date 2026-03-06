import { listen, type UnlistenFn } from '#desktop/api';
import { useCacheStore } from './cacheStore';
import { useDomainStore } from './domainStore';
import { useLibraryStore } from './libraryStore';
import { SidebarController } from '../controllers/sidebarController';
import { SelectionController } from '../controllers/selectionController';
import { invalidateMetadata } from '../components/image-grid/metadataPrefetch';
import type {
  FlowFinishedEvent,
  GridSnapshotInvalidatedEvent,
  LibrarySwitchedEvent,
  LibrarySwitchingEvent,
  SidebarInvalidatedEvent,
  StateChangedEvent,
  SubscriptionFinishedEvent,
  SubscriptionStartedEvent,
} from '../types/api';

let unlisteners: UnlistenFn[] = [];
let isSetup = false;

/**
 * Dedupe guard: track the last processed state-changed seq so targeted events
 * emitted from the same mutation don't cause duplicate refreshes (PBI-021).
 */
let lastSidebarSeq = -1;
let lastGridSeq = -1;

export function shouldInvalidateGridScope(activeScope: string | null, scopes: string[]): boolean {
  if (!activeScope) return true;
  if (scopes.includes(activeScope)) return true;
  if (activeScope.startsWith('folder:') && scopes.includes('folder:all')) return true;
  if (activeScope.startsWith('smart:') && scopes.includes('smart:all')) return true;
  // Legacy behavior: system:all was historically emitted as a wildcard.
  const hasNonSystemScope = scopes.some((scope) => !scope.startsWith('system:'));
  if (!hasNonSystemScope && scopes.includes('system:all')) return true;
  return false;
}

export async function setupEventBridge(): Promise<void> {
  if (isSetup) return;

  const results = await Promise.allSettled([
    listen<StateChangedEvent>('state-changed', (event) => {
      const e = event.payload;

      if (e.invalidate?.metadata_hashes?.length) {
        for (const hash of e.invalidate.metadata_hashes) {
          useCacheStore.getState().invalidateHash(hash);
          useCacheStore.getState().markHashInvalidated(hash);
          invalidateMetadata(hash);
        }
      }

      if (e.invalidate?.selection_summary) {
        SelectionController.invalidateSummary();
      }

      if (e.invalidate?.grid_scopes?.length) {
        const activeScope = useCacheStore.getState().activeGridScope;
        const scopes = e.invalidate.grid_scopes;
        const matches = shouldInvalidateGridScope(activeScope, scopes);
        const skipInboxReplaceForSubscriptionImport =
          activeScope === 'system:inbox'
          && e.origin_command === 'subscription_import'
          && scopes.includes('system:inbox');
        if (matches && !skipInboxReplaceForSubscriptionImport) {
          useCacheStore.getState().invalidateAll();
          useCacheStore.getState().bumpGridRefresh();
        }
      }

      if (e.sidebar_counts) {
        useDomainStore.getState().applySidebarCounts(e.sidebar_counts);
      }

      if (e.invalidate?.sidebar_tree || e.compiler_batch_done) {
        lastSidebarSeq = e.seq;
        SidebarController.requestRefresh();
      }

      if (e.invalidate?.grid_scopes?.length) {
        lastGridSeq = e.seq;
      }
    }),
    listen<SidebarInvalidatedEvent>('sidebar-invalidated', (event) => {
      const seq = event.payload?.seq;
      if (typeof seq === 'number' && seq === lastSidebarSeq) return; // already handled
      SidebarController.requestRefresh();
    }),
    listen<GridSnapshotInvalidatedEvent>('grid-snapshot-invalidated', (event) => {
      const seq = event.payload?.seq;
      if (typeof seq === 'number' && seq === lastGridSeq) return; // already handled
      const scopeKey = event.payload?.scope_key;
      const activeScope = useCacheStore.getState().activeGridScope;
      const matches = !scopeKey || shouldInvalidateGridScope(activeScope, [scopeKey]);
      if (matches) {
        useCacheStore.getState().invalidateAll();
        useCacheStore.getState().bumpGridRefresh();
      }
    }),

    listen<SubscriptionStartedEvent>('subscription-started', () => {
      SidebarController.requestRefresh();
    }),
    listen<SubscriptionFinishedEvent>('subscription-finished', () => {
      SidebarController.requestRefresh();
      useCacheStore.getState().invalidateAll();
      useCacheStore.getState().bumpGridRefresh();
    }),

    listen<FlowFinishedEvent>('flow-finished', () => {
      SidebarController.requestRefresh();
      useCacheStore.getState().invalidateAll();
      useCacheStore.getState().bumpGridRefresh();
    }),

    listen<LibrarySwitchingEvent>('library-switching', () => {
      useLibraryStore.getState().setSwitching(true);
    }),
    listen<LibrarySwitchedEvent>('library-switched', () => {
      useCacheStore.getState().invalidateAll();
      useCacheStore.getState().bumpGridRefresh();
      SidebarController.requestRefresh();
      SelectionController.invalidateSummary();
      useLibraryStore.getState().setSwitching(false);
      useLibraryStore.getState().loadConfig();
    }),
  ]);

  // Rollback on partial failure: unlisten successfully registered listeners.
  const fulfilled: UnlistenFn[] = [];
  let anyFailed = false;
  for (const r of results) {
    if (r.status === 'fulfilled') fulfilled.push(r.value);
    else anyFailed = true;
  }

  if (anyFailed) {
    for (const fn of fulfilled) fn();
    throw new Error('Event bridge setup failed: some listeners could not be registered');
  }

  unlisteners = fulfilled;
  isSetup = true;
}

export function teardownEventBridge(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
  isSetup = false;
  lastSidebarSeq = -1;
  lastGridSeq = -1;
}
