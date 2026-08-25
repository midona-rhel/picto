/**
 * Portal state — open/close flags + optional anchor positions for floating workflow panels.
 */

import { atom } from 'jotai';
import type { FilterMatchMode } from '../shared/types/generated/application/FilterMatchMode';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';

export interface PortalState {
  open: boolean;
  target?: ItemTarget;
  anchor?: { x: number; y: number } | null;
  anchorPlacement?: 'left' | 'below' | 'above';
  selectedTags?: string[];
  excludedTags?: string[];
  selectedFolderIds?: number[];
  excludedFolderIds?: number[];
  filterMatchMode?: FilterMatchMode;
  availableFolderIds?: number[];
  onApplyTags?: (tags: string[]) => void;
  onApplyTagFilter?: (includedTags: string[], excludedTags: string[], mode: FilterMatchMode) => void;
  onApplyFolders?: (folderIds: number[]) => void;
  onApplyFolderParent?: (folderId: number | null) => void;
  onApplyFolderFilter?: (includedFolderIds: number[], excludedFolderIds: number[], mode: FilterMatchMode) => void;
}

const closed: PortalState = { open: false, anchor: null };

export const tagSelectPortalAtom = atom<PortalState>(closed);
export const folderPickerPortalAtom = atom<PortalState>(closed);
export const aiTaggerPortalAtom = atom<PortalState>(closed);

/**
 * Anchor for panels that dock to the inspector's left edge, used when the
 * opener isn't an inspector button (context menu, keyboard shortcut).
 * Returns null when the inspector isn't in the DOM — OverlayShell then
 * falls back to centered placement.
 */
export function inspectorAnchor(): { x: number; y: number } | null {
  const panel = document.querySelector('[data-inspector-panel]') as HTMLElement | null;
  if (!panel) return null;
  const rect = panel.getBoundingClientRect();
  return { x: rect.left, y: rect.top + 80 };
}

/** Open a portal with an anchor position (e.g., from the inspector + button). */
export function openPortalAnchored(
  set: (atom: any, value: any) => void,
  portalAtom: typeof tagSelectPortalAtom,
  buttonEl: HTMLElement,
) {
  const rect = buttonEl.getBoundingClientRect();
  set(portalAtom, { open: true, anchor: { x: rect.left, y: rect.top } });
}
