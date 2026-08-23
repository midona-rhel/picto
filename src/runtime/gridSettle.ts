import { getDefaultStore } from 'jotai';
import { gridActiveAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import { libraryInvalidation } from './libraryInvalidation';

const store = getDefaultStore();

/** Re-query the current canonical grid after committed library changes. */
export function startGridSettle(): () => void {
  let cancelled = false;
  let reloadPending = false;

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
  const unsubscribePhase = store.sub(gridTransitionPhaseAtom, () => {
    if (!reloadPending) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'idle' || phase === 'fading_in') reload();
  });

  return () => {
    cancelled = true;
    unregister();
    unsubscribePhase();
  };
}
