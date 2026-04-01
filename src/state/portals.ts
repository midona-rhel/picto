/**
 * Portal state — open/close flags + optional anchor positions for floating workflow panels.
 */

import { atom } from 'jotai';

export interface PortalState {
  open: boolean;
  anchor?: { x: number; y: number } | null;
}

const closed: PortalState = { open: false, anchor: null };

export const tagSelectPortalAtom = atom<PortalState>(closed);
export const folderPickerPortalAtom = atom<PortalState>(closed);
export const aiTaggerPortalAtom = atom<PortalState>(closed);
export const batchRenamePortalAtom = atom<PortalState>(closed);

// Convenience: simple boolean atoms for backward compat
export const tagSelectOpenAtom = atom(
  (get) => get(tagSelectPortalAtom).open,
  (_get, set, open: boolean) => set(tagSelectPortalAtom, open ? { open: true } : closed),
);
export const folderPickerOpenAtom = atom(
  (get) => get(folderPickerPortalAtom).open,
  (_get, set, open: boolean) => set(folderPickerPortalAtom, open ? { open: true } : closed),
);
export const aiTaggerOpenAtom = atom(
  (get) => get(aiTaggerPortalAtom).open,
  (_get, set, open: boolean) => set(aiTaggerPortalAtom, open ? { open: true } : closed),
);
export const batchRenameOpenAtom = atom(
  (get) => get(batchRenamePortalAtom).open,
  (_get, set, open: boolean) => set(batchRenamePortalAtom, open ? { open: true } : closed),
);

/** Open a portal with an anchor position (e.g., from the inspector + button). */
export function openPortalAnchored(
  set: (atom: any, value: any) => void,
  portalAtom: typeof tagSelectPortalAtom,
  buttonEl: HTMLElement,
) {
  const rect = buttonEl.getBoundingClientRect();
  set(portalAtom, { open: true, anchor: { x: rect.left, y: rect.top } });
}
