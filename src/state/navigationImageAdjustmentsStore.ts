import { create } from 'zustand';

export type NavigationImageRotation = 0 | 90 | 180 | 270;

export interface NavigationImageAdjustment {
  grayscale: boolean;
  mirrored: boolean;
  rotation: NavigationImageRotation;
}

export const DEFAULT_NAVIGATION_IMAGE_ADJUSTMENT: NavigationImageAdjustment = Object.freeze({
  grayscale: false,
  mirrored: false,
  rotation: 0,
});

interface NavigationImageAdjustmentsState {
  byHash: Record<string, NavigationImageAdjustment>;
  toggleGrayscale: (hashes: string[]) => void;
  rotateClockwise: (hash: string) => void;
  toggleMirrored: (hash: string) => void;
}

function getNextAdjustment(
  current: NavigationImageAdjustment | undefined,
  patch: Partial<NavigationImageAdjustment>,
): NavigationImageAdjustment | null {
  const next = {
    ...(current ?? DEFAULT_NAVIGATION_IMAGE_ADJUSTMENT),
    ...patch,
  };
  if (!next.grayscale && !next.mirrored && next.rotation === 0) {
    return null;
  }
  return next;
}

export const useNavigationImageAdjustmentsStore = create<NavigationImageAdjustmentsState>((set) => ({
  byHash: {},
  toggleGrayscale: (hashes) => set((state) => {
    const uniqueHashes = Array.from(new Set(hashes.filter(Boolean)));
    if (uniqueHashes.length === 0) return state;
    const shouldEnable = uniqueHashes.some((hash) => !(state.byHash[hash]?.grayscale ?? false));
    const nextByHash = { ...state.byHash };
    for (const hash of uniqueHashes) {
      const next = getNextAdjustment(nextByHash[hash], { grayscale: shouldEnable });
      if (next) nextByHash[hash] = next;
      else delete nextByHash[hash];
    }
    return { byHash: nextByHash };
  }),
  rotateClockwise: (hash) => set((state) => {
    if (!hash) return state;
    const current = state.byHash[hash];
    const currentRotation = current?.rotation ?? 0;
    const nextRotation = ((currentRotation + 90) % 360) as NavigationImageRotation;
    const next = getNextAdjustment(current, { rotation: nextRotation });
    const nextByHash = { ...state.byHash };
    if (next) nextByHash[hash] = next;
    else delete nextByHash[hash];
    return { byHash: nextByHash };
  }),
  toggleMirrored: (hash) => set((state) => {
    if (!hash) return state;
    const current = state.byHash[hash];
    const next = getNextAdjustment(current, { mirrored: !(current?.mirrored ?? false) });
    const nextByHash = { ...state.byHash };
    if (next) nextByHash[hash] = next;
    else delete nextByHash[hash];
    return { byHash: nextByHash };
  }),
}));

export function getNavigationImageAdjustment(hash: string | null | undefined): NavigationImageAdjustment {
  if (!hash) return DEFAULT_NAVIGATION_IMAGE_ADJUSTMENT;
  return useNavigationImageAdjustmentsStore.getState().byHash[hash] ?? DEFAULT_NAVIGATION_IMAGE_ADJUSTMENT;
}
