import { getDefaultStore } from 'jotai';
import { gridActiveAtom, gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import { libraryInvalidation } from './libraryInvalidation';
import { listenDominantColorChanged } from '../shared/lib/thumbnailChanges';

const store = getDefaultStore();

/** Re-query the current canonical grid after committed library changes. */
export function startGridSettle(): () => void {
  let cancelled = false;
  let reloadPending = false;
  let removeColorListener: (() => void) | undefined;

  const reload = () => {
    if (cancelled || !store.get(gridActiveAtom)) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'fading_out' || phase === 'waiting') {
      reloadPending = true;
      return;
    }
    reloadPending = false;
    void gridController.loadFirstPage({ preserveItems: true });
  };

  const unregister = libraryInvalidation.register('library', reload);
  void listenDominantColorChanged(({ fileHash, dominantColorHex }) => {
    if (cancelled || !store.get(gridActiveAtom)) return;
    const session = store.get(gridSessionAtom);
    if (session.filters.color_hex) {
      reload();
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
    if (!reloadPending) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'idle' || phase === 'fading_in') reload();
  });

  return () => {
    cancelled = true;
    unregister();
    removeColorListener?.();
    unsubscribePhase();
  };
}
