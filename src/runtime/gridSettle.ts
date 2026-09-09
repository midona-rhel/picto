import { getDefaultStore } from 'jotai';
import { gridActiveAtom, gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import { libraryInvalidation } from './libraryInvalidation';
import { listenDominantColorChanged } from '../shared/lib/thumbnailChanges';
import { hexToLab, labToHex } from '../shared/lib/labColor';

const store = getDefaultStore();

/** Re-query the current canonical grid after committed library changes. */
export function startGridSettle(): () => void {
  let cancelled = false;
  let removeColorListener: (() => void) | undefined;

  const scheduleReconcile = () => {
    if (cancelled || !store.get(gridActiveAtom)) return;
    gridController.requestReconcile();
  };

  const unregister = libraryInvalidation.register('library', scheduleReconcile);
  const unregisterRecentlyViewed = libraryInvalidation.register('recently_viewed', () => {
    if (store.get(gridSessionAtom).scope.kind === 'recently_viewed') scheduleReconcile();
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
      if (item.content_hash !== fileHash || labToHex(item.palette[0]) === dominantColorHex) {
        return item;
      }
      changed = true;
      return dominantColorHex == null
        ? { ...item, palette: [] }
        : { ...item, palette: [hexToLab(dominantColorHex), ...item.palette.slice(1)] };
    });
    if (changed) store.set(gridSessionAtom, { ...session, items });
  }).then((remove) => {
    if (cancelled) remove();
    else removeColorListener = remove;
  }).catch(() => {});
  const unsubscribePhase = store.sub(gridTransitionPhaseAtom, () => {
    gridController.flushPendingRefresh();
  });

  return () => {
    cancelled = true;
    unregister();
    unregisterRecentlyViewed();
    removeColorListener?.();
    unsubscribePhase();
  };
}
