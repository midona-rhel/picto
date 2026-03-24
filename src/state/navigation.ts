/**
 * Navigation state — where the user is in the app.
 *
 * One atom per navigable dimension. Derived atoms compute composite
 * values like the active grid scope string.
 */

import { atom } from 'jotai';

// ── Authoritative navigation state ─────────────────────────────

export type ViewKind = 'images' | 'subscriptions' | 'settings' | 'duplicates';

export const activeViewAtom = atom<ViewKind>('images');
export const activeFolderIdAtom = atom<number | null>(null);
export const activeSmartFolderIdAtom = atom<number | null>(null);
export const activeCollectionIdAtom = atom<number | null>(null);
export const activeStatusFilterAtom = atom<string | null>(null);

// ── Derived navigation state ───────────────────────────────────

/** The canonical grid scope key, derived from navigation atoms.
 *  This replaces gridMetadataStore.activeGridScope. */
export const activeGridScopeAtom = atom((get) => {
  const folderId = get(activeFolderIdAtom);
  if (folderId != null) return `folder:${folderId}`;

  const collectionId = get(activeCollectionIdAtom);
  if (collectionId != null) return `collection:${collectionId}`;

  const smartFolderId = get(activeSmartFolderIdAtom);
  if (smartFolderId != null) return `smart:${smartFolderId}`;

  const statusFilter = get(activeStatusFilterAtom);
  if (statusFilter === 'inbox') return 'system:inbox';
  if (statusFilter === 'trash') return 'system:trash';
  if (statusFilter === 'untagged') return 'system:untagged';
  if (statusFilter === 'uncategorized') return 'system:uncategorized';

  return 'system:active';
});

// ── History ────────────────────────────────────────────────────

export interface NavigationEntry {
  view: ViewKind;
  folderId: number | null;
  smartFolderId: number | null;
  collectionId: number | null;
  statusFilter: string | null;
  scrollTop: number;
}

export const navigationHistoryAtom = atom<NavigationEntry[]>([]);
export const navigationIndexAtom = atom(0);
