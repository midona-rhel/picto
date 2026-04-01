/**
 * Modal state — open/close state for all application modals.
 */

import { atom } from 'jotai';

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
  initial?: { name?: string; icon?: string | null; color?: string | null; id?: number };
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

// ── Create subscription group modal ──
export const createGroupModalAtom = atom({ open: false });
