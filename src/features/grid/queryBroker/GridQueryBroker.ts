import type { GridRuntimeAction } from '../runtime/gridRuntimeReducer';
import type { GridRuntimeState } from '../runtime/gridRuntimeState';
import type { MasonryImageItem } from '../shared';
import type { GridQueryKey } from './gridQueryKey';
import { queryKeyToFetchArgs } from './gridQueryKey';
import { GridController } from '../../../shared/controllers/gridController';
import { toMasonryItem } from '../shared';
import { batchPreloadMediaUrls } from '../enhancedMediaCache';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface DeferredPayload {
  images: MasonryImageItem[];
  nextCursor: string | null;
  hasMore: boolean;
}

export interface BrokerStats {
  replaceRequests: number;
  appendRequests: number;
  coalesced: number;
  cancelled: number;
  committed: number;
  errors: number;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const PAGE_SIZE = 100;
export const MAX_LOADED_ITEMS = 10_000;
const LOOKAHEAD_PAGE_SIZE = 100;

// ---------------------------------------------------------------------------
// GridQueryBroker
// ---------------------------------------------------------------------------

export class GridQueryBroker {
  // --- Wired by hook each render ---
  private dispatch: React.Dispatch<GridRuntimeAction> = () => {};
  private stateRef: { current: GridRuntimeState } = { current: null as unknown as GridRuntimeState };
  private viewModeRef: { current: string } = { current: 'waterfall' };
  private prewarmFn: ((items: MasonryImageItem[]) => Promise<void>) | null = null;
  private onFirstCommit: (() => void) | null = null;

  // --- Internal state ---
  private generation = 0;
  private deferredCommitArmed = false;
  private deferredPayload: DeferredPayload | null = null;
  private pendingReplaceKey: GridQueryKey | null = null;
  private pendingReplaceGen = 0;
  private coalesceMicrotaskScheduled = false;
  private firstCommitDone = false;
  private destroyed = false;
  private reloadAfterTransition = false;
  private onEstimateSampleChanged: ((items: MasonryImageItem[]) => void) | null = null;
  private lookaheadCursor: string | null = null;
  private lookaheadRequestSeq = 0;

  // --- Instrumentation ---
  private stats: BrokerStats = {
    replaceRequests: 0,
    appendRequests: 0,
    coalesced: 0,
    cancelled: 0,
    committed: 0,
    errors: 0,
  };

  // -----------------------------------------------------------------------
  // Wiring (called by hook on each render — cheap ref assignments)
  // -----------------------------------------------------------------------

  wire(
    dispatch: React.Dispatch<GridRuntimeAction>,
    stateRef: { current: GridRuntimeState },
    viewModeRef: { current: string },
    prewarmFn: ((items: MasonryImageItem[]) => Promise<void>) | null,
    onFirstCommit: (() => void) | null,
    onEstimateSampleChanged: ((items: MasonryImageItem[]) => void) | null,
  ): void {
    // Re-activate after React strict-mode cleanup cycle
    this.destroyed = false;
    this.dispatch = dispatch;
    this.stateRef = stateRef;
    this.viewModeRef = viewModeRef;
    this.prewarmFn = prewarmFn;
    this.onFirstCommit = onFirstCommit;
    this.onEstimateSampleChanged = onEstimateSampleChanged;
  }

  // -----------------------------------------------------------------------
  // Public API
  // -----------------------------------------------------------------------

  /**
   * Request a full replace load for the given query key.
   *
   * Coalescing: if another replace is pending in the same microtask,
   * the older one is replaced (only the last key wins).
   *
   * Cancellation: bumps the generation token so any in-flight request
   * for a previous key is silently dropped on completion.
   *
   * During a transition (deferredCommitArmed), the request is deferred
   * to avoid cancelling the in-flight transition replace. A reload is
   * scheduled for after the transition completes.
   */
  requestReplace(key: GridQueryKey): void {
    if (this.destroyed) return;
    this.stats.replaceRequests++;

    // During a transition, don't bump generation — that would cancel the
    // in-flight requestReplaceAsync and leave takeDeferredPayload() empty.
    // Instead, note that a reload is needed after the transition commits.
    if (this.deferredCommitArmed) {
      this.reloadAfterTransition = true;
      this.stats.coalesced++;
      return;
    }

    const gen = ++this.generation;
    this.clearEstimateSample();

    if (this.pendingReplaceKey) {
      this.stats.coalesced++;
    }

    this.pendingReplaceKey = key;
    this.pendingReplaceGen = gen;

    if (!this.coalesceMicrotaskScheduled) {
      this.coalesceMicrotaskScheduled = true;
      Promise.resolve().then(() => {
        this.coalesceMicrotaskScheduled = false;
        const pending = this.pendingReplaceKey;
        const pendingGen = this.pendingReplaceGen;
        this.pendingReplaceKey = null;
        if (pending && pendingGen === this.generation && !this.destroyed) {
          void this.executeReplace(pending, pendingGen);
        }
      });
    }
  }

  /**
   * Request a replace load that returns a promise. Bypasses coalescing —
   * executes immediately. Used by the transition orchestrator which needs
   * to await completion.
   */
  requestReplaceAsync(key: GridQueryKey): Promise<void> {
    if (this.destroyed) return Promise.resolve();
    this.stats.replaceRequests++;
    const gen = ++this.generation;
    this.clearEstimateSample();

    // Cancel any pending coalesced replace
    this.pendingReplaceKey = null;

    return this.executeReplace(key, gen);
  }

  /**
   * Request an append (next page) for the given query key.
   * Returns a promise that resolves when the page has been committed.
   *
   * Does NOT bump generation — appends are additive. If a replace happens
   * while an append is in-flight, the append is silently dropped.
   */
  async requestAppend(key: GridQueryKey): Promise<void> {
    if (this.destroyed) return;
    this.stats.appendRequests++;

    const gen = this.generation; // capture, don't bump
    const cursor = this.stateRef.current?.defaultGridCursor ?? null;
    if (!cursor) {
      // Defensive: prevent endless load-more loops when state says hasMore
      // but no cursor exists (invalid pagination state).
      if (this.stateRef.current?.hasMore) {
        this.dispatch({ type: 'SET_HAS_MORE', hasMore: false });
      }
      return;
    }

    try {
      const args = queryKeyToFetchArgs(key, cursor, PAGE_SIZE);
      const page = await GridController.fetchGridPage(args);

      if (gen !== this.generation || this.destroyed) {
        this.stats.cancelled++;
        return;
      }

      const items = page.items.map(toMasonryItem);
      const nextCursor = page.next_cursor;
      const cursorAdvanced = nextCursor !== null && nextCursor !== cursor;
      const safeHasMore = page.has_more && cursorAdvanced;

      this.dispatch({ type: 'SET_CURSOR', cursor: nextCursor, hasMore: safeHasMore });
      this.dispatch({ type: 'SET_RESPONSE_TOTAL_COUNT', count: page.total_count ?? null });
      this.dispatch({ type: 'APPEND_IMAGES', images: items, maxItems: MAX_LOADED_ITEMS });
      this.stats.committed++;

      if (items.length > 0) {
        batchPreloadMediaUrls(items, 'thumb512', 'high');
        void GridController.prefetchVisibleMetadata(items.map(i => i.hash));
      }
      void this.prefetchEstimateSample(key, page.next_cursor, gen);
    } catch (err) {
      if (gen !== this.generation || this.destroyed) {
        this.stats.cancelled++;
        return;
      }
      console.error('GridQueryBroker: append failed', err);
      this.dispatch({ type: 'SET_ERROR', error: String(err) });
      this.stats.errors++;
    }
  }

  // -----------------------------------------------------------------------
  // Deferred commit API (for transition orchestrator)
  // -----------------------------------------------------------------------

  armDeferredCommit(): void {
    this.deferredCommitArmed = true;
    this.deferredPayload = null;
  }

  takeDeferredPayload(): DeferredPayload | null {
    const payload = this.deferredPayload;
    this.deferredPayload = null;
    this.deferredCommitArmed = false;
    return payload;
  }

  disarmDeferredCommit(): void {
    this.deferredCommitArmed = false;
    this.deferredPayload = null;
  }

  /**
   * Returns true (and clears the flag) if a requestReplace was deferred
   * during an active transition. The caller should fire a fresh
   * requestReplace to pick up the changes that were suppressed.
   */
  popReloadAfterTransition(): boolean {
    const needed = this.reloadAfterTransition;
    this.reloadAfterTransition = false;
    return needed;
  }

  // -----------------------------------------------------------------------
  // Instrumentation
  // -----------------------------------------------------------------------

  getStats(): Readonly<BrokerStats> {
    return { ...this.stats };
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  destroy(): void {
    this.destroyed = true;
    this.pendingReplaceKey = null;
    this.deferredPayload = null;
    this.deferredCommitArmed = false;
    this.reloadAfterTransition = false;
    this.onEstimateSampleChanged = null;
    this.lookaheadCursor = null;
  }

  // -----------------------------------------------------------------------
  // Internal
  // -----------------------------------------------------------------------

  private async executeReplace(key: GridQueryKey, gen: number): Promise<void> {
    try {
      this.dispatch({ type: 'SET_ERROR', error: null });

      const args = queryKeyToFetchArgs(key, null, PAGE_SIZE);
      const page = await GridController.fetchGridPage(args);

      if (gen !== this.generation || this.destroyed) {
        this.stats.cancelled++;
        return;
      }

      const items = page.items.map(toMasonryItem);

      // Prewarm thumbnails when deferred + waterfall (transition midpoint)
      if (this.deferredCommitArmed && this.viewModeRef.current === 'waterfall' && items.length > 0 && this.prewarmFn) {
        await this.prewarmFn(items);
        if (gen !== this.generation || this.destroyed) {
          this.stats.cancelled++;
          return;
        }
      }

      // Always dispatch cursor + count (they don't affect visual layout)
      const safeHasMore = page.has_more && !!page.next_cursor;
      this.dispatch({ type: 'SET_CURSOR', cursor: page.next_cursor, hasMore: safeHasMore });
      this.dispatch({ type: 'SET_RESPONSE_TOTAL_COUNT', count: page.total_count ?? null });

      if (this.deferredCommitArmed) {
        // Buffer for transition orchestrator
        this.deferredPayload = {
          images: items,
          nextCursor: page.next_cursor,
          hasMore: safeHasMore,
        };
      } else {
        // Direct commit
        this.dispatch({ type: 'SET_IMAGES', images: items });
      }
      this.stats.committed++;

      // Side-effects
      if (items.length > 0) {
        batchPreloadMediaUrls(items, 'thumb512', 'high');
        void GridController.prefetchVisibleMetadata(items.map(i => i.hash));
      }
      void this.prefetchEstimateSample(key, page.next_cursor, gen);

      // First-commit callback (sets initialLoadDone in ImageGrid)
      if (!this.firstCommitDone) {
        this.firstCommitDone = true;
        this.onFirstCommit?.();
      }
    } catch (err) {
      if (gen !== this.generation || this.destroyed) {
        this.stats.cancelled++;
        return;
      }
      console.error('GridQueryBroker: replace failed', err);
      this.dispatch({ type: 'SET_ERROR', error: String(err) });
      this.stats.errors++;

      // Mark first commit even on error so empty state shows
      if (!this.firstCommitDone) {
        this.firstCommitDone = true;
        this.onFirstCommit?.();
      }
    }
  }

  private clearEstimateSample(): void {
    this.lookaheadCursor = null;
    this.onEstimateSampleChanged?.([]);
  }

  private async prefetchEstimateSample(
    key: GridQueryKey,
    cursor: string | null,
    gen: number,
  ): Promise<void> {
    if (!cursor) {
      this.clearEstimateSample();
      return;
    }
    if (this.lookaheadCursor === cursor) return;
    this.lookaheadCursor = cursor;
    const seq = ++this.lookaheadRequestSeq;
    try {
      const args = queryKeyToFetchArgs(key, cursor, LOOKAHEAD_PAGE_SIZE);
      const page = await GridController.fetchGridPage(args);
      if (gen !== this.generation || this.destroyed || seq !== this.lookaheadRequestSeq) return;
      const loaded = this.stateRef.current?.images ?? [];
      if (loaded.length > 0 && page.items.length > 0) {
        const loadedHashes = new Set(loaded.map((item) => item.hash));
        let overlap = 0;
        for (const item of page.items) {
          if (loadedHashes.has(item.hash)) overlap++;
        }
        // If this "next page" is largely already loaded, cursor semantics likely overlapped.
        // Ignore it for estimation to avoid doubling/warping scroll prediction.
        if (overlap / page.items.length >= 0.25) {
          this.onEstimateSampleChanged?.([]);
          return;
        }
      }
      const sample = page.items.map(toMasonryItem);
      this.onEstimateSampleChanged?.(sample);
    } catch {
      if (gen !== this.generation || this.destroyed || seq !== this.lookaheadRequestSeq) return;
      this.onEstimateSampleChanged?.([]);
    }
  }
}
