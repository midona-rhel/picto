import { getDefaultStore } from 'jotai';
import { gridActiveAtom, gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import { libraryInvalidation } from './libraryInvalidation';
import { listenDominantColorChanged } from '../shared/lib/thumbnailChanges';

const store = getDefaultStore();
const RECONCILE_INTERVAL_MS = 2_000;

/** Re-query the current canonical grid after committed library changes. */
export function startGridSettle(): () => void {
  let cancelled = false;
  let reconcilePending = false;
  let reconcileRunning = false;
  const affectedItemIds = new Set<number>();
  let reconcileTimer: ReturnType<typeof setTimeout> | undefined;
  let lastReconcileCompletedAt = -Infinity;
  let removeColorListener: (() => void) | undefined;

  const runReconcile = async () => {
    reconcileTimer = undefined;
    if (cancelled || !store.get(gridActiveAtom)) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'fading_out' || phase === 'waiting') {
      reconcilePending = true;
      return;
    }
    if (reconcileRunning || !reconcilePending) return;
    reconcilePending = false;
    reconcileRunning = true;
    const affected = [...affectedItemIds];
    affectedItemIds.clear();
    const reconciled = await gridController.reconcile(affected);
    reconcileRunning = false;
    lastReconcileCompletedAt = Date.now();
    const session = store.get(gridSessionAtom);
    if (!reconciled && session.active && (session.status === 'loading' || session.status === 'appending')) {
      reconcilePending = true;
    }
    if (reconcilePending) scheduleReconcile();
  };

  const scheduleReconcile = (payload?: { item_ids: number[] }) => {
    if (cancelled || !store.get(gridActiveAtom)) return;
    payload?.item_ids.forEach((itemId) => affectedItemIds.add(itemId));
    reconcilePending = true;
    if (reconcileRunning || reconcileTimer) return;
    const delay = Math.max(0, RECONCILE_INTERVAL_MS - (Date.now() - lastReconcileCompletedAt));
    if (delay === 0) void runReconcile();
    else reconcileTimer = setTimeout(() => { void runReconcile(); }, delay);
  };

  const unregister = libraryInvalidation.register('library', scheduleReconcile);
  const unregisterRecentlyViewed = libraryInvalidation.register('recently_viewed', (payload) => {
    if (store.get(gridSessionAtom).scope.kind === 'recently_viewed') scheduleReconcile(payload);
  });
  void listenDominantColorChanged(({ fileHash, dominantColorHex }) => {
    if (cancelled || !store.get(gridActiveAtom)) return;
    const session = store.get(gridSessionAtom);
    if (session.filters.color_hex) {
      scheduleReconcile();
      return;
    }
    let changed = false;
    const items = session.items.map((item) => {
      if (item.display_file_hash !== fileHash || item.dominant_color_hex === dominantColorHex) {
        return item;
      }
      changed = true;
      return { ...item, dominant_color_hex: dominantColorHex };
    });
    if (changed) store.set(gridSessionAtom, { ...session, items });
  }).then((remove) => {
    if (cancelled) remove();
    else removeColorListener = remove;
  }).catch(() => {});
  const unsubscribePhase = store.sub(gridTransitionPhaseAtom, () => {
    if (!reconcilePending) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'idle' || phase === 'fading_in') scheduleReconcile();
  });

  return () => {
    cancelled = true;
    if (reconcileTimer) clearTimeout(reconcileTimer);
    unregister();
    unregisterRecentlyViewed();
    removeColorListener?.();
    unsubscribePhase();
  };
}
