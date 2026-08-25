/**
 * Modal state — open/close state for all application modals.
 */

import { atom } from 'jotai';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';

// ── Confirm modal ──
export interface ConfirmModalState {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
}

const confirmClosed: ConfirmModalState = {
  open: false, title: '', message: '', onConfirm: () => {},
};

export const confirmModalAtom = atom<ConfirmModalState>(confirmClosed);

/** Helper to open the confirm modal from anywhere. */
export function openConfirm(
  set: (atom: typeof confirmModalAtom, value: ConfirmModalState) => void,
  opts: Omit<ConfirmModalState, 'open'>,
) {
  set(confirmModalAtom, { ...opts, open: true });
}

// ── Smart folder modal ──
export interface SmartFolderModalState {
  open: boolean;
  mode: 'create' | 'edit';
  initial?: {
    id?: number;
    name?: string;
    parent_id?: number | null;
    icon?: string | null;
    color?: string | null;
    notes?: string | null;
    predicate?: import('../shared/types/canonical').SmartFolderPredicate;
    display_order?: number | null;
  };
}

export const smartFolderModalAtom = atom<SmartFolderModalState>({ open: false, mode: 'create' });

// ── Folder watch modal ──
export interface FolderWatchModalState {
  open: boolean;
  folderId?: number;
  initial?: { watchPath?: string; enabled?: boolean; subfolders?: boolean; importStatusMode?: string };
}

export const folderWatchModalAtom = atom<FolderWatchModalState>({ open: false });

// ── Export modal ──
export interface ExportModalState {
  open: boolean;
  fileCount: number;
  target?: import('../shared/types/canonical').EntityTarget;
}
export const exportModalAtom = atom<ExportModalState>({ open: false, fileCount: 0 });

export interface BatchRenameModalState {
  open: boolean;
  items: Array<{ item_id: number; name: string }>;
}
export const batchRenameModalAtom = atom<BatchRenameModalState>({ open: false, items: [] });

// ── Folder import modal (shown when dropping a folder into the app) ──
export interface FolderImportModalState {
  open: boolean;
  path: string;
  targetFolderId: number | null;
  lifecycle: import('../shared/types/generated/application/Lifecycle').Lifecycle;
}

export const folderImportModalAtom = atom<FolderImportModalState>({
  open: false, path: '', targetFolderId: null, lifecycle: 'active',
});

// ── Tag select modal (wider modal version, opened from context menu / keyboard) ──
export const tagSelectModalAtom = atom({ open: false });

// ── Folder picker modal (wider modal version, opened from context menu / keyboard) ──
export const folderPickerModalAtom = atom({ open: false });

// ── Group organizer modal ──
export interface GroupCandidate {
  collection_id: number;
  label: string | null;
  member_count: number;
}

export interface GroupOrganizerModalState {
  open: boolean;
  target: ItemTarget | null;
  groups: GroupCandidate[];
  onComplete?: (groupId: number) => void;
}

export const groupOrganizerModalAtom = atom<GroupOrganizerModalState>({
  open: false,
  target: null,
  groups: [],
});
