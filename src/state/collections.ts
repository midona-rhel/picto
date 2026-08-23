import { atom } from 'jotai';

export interface CollectionChromeState {
  label: string;
  parentLabel: string;
  mode: 'reader' | 'editor';
  memberViewerOpen: boolean;
  close: () => void;
  edit: () => void;
  finishEditing: () => void;
}

/** Published by the active collection surface and rendered by AppShell. */
export const collectionChromeAtom = atom<CollectionChromeState | null>(null);
