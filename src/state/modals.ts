/**
 * Modal state — open/close state for all application modals.
 */

import { atom } from 'jotai';
import type { EntityTarget } from '../shared/types/canonical';

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
  /** Creation shows the complete form; existing folders edit metadata and rules separately. */
  editor?: 'all' | 'details' | 'rules';
  initial?: {
    id?: number;
    name?: string;
    parent_id?: number | null;
    icon?: string | null;
    color?: string | null;
    notes?: string | null;
    view?: import('../shared/types/canonical').ViewQuerySpec;
  };
}

export const smartFolderModalAtom = atom<SmartFolderModalState>({ open: false, mode: 'create', editor: 'all' });

// ── Folder watch modal ──
export interface FolderWatchModalState {
  open: boolean;
  folderId?: number;
  initial?: { watchPath?: string; enabled?: boolean; subfolders?: boolean; importStatusMode?: string };
}

export const folderWatchModalAtom = atom<FolderWatchModalState>({ open: false });

// ── Folder auto tags modal ──
export interface FolderAutoTagsModalState {
  open: boolean;
  folderIds: number[];
  initialTags: string[];
}

export const folderAutoTagsModalAtom = atom<FolderAutoTagsModalState>({
  open: false,
  folderIds: [],
  initialTags: [],
});

// ── Export modal ──
export interface ExportModalState {
  open: boolean;
  fileCount: number;
  target?: import('../shared/types/canonical').EntityTarget;
}
export const exportModalAtom = atom<ExportModalState>({ open: false, fileCount: 0 });

export type PictoPackModalState =
  | { open: false }
  | {
      open: true;
      mode: 'export';
      source: import('../platform/pictoPackApi').PictoPackSource;
      itemCount: number;
      suggestedName: string;
    }
  | {
      open: true;
      mode: 'import';
      path: string;
      summary: import('../platform/pictoPackApi').PictoPackSummary;
    };

export const pictoPackModalAtom = atom<PictoPackModalState>({ open: false });

export interface BatchRenameModalState {
  open: boolean;
  items: Array<{ root_id: number; name: string }>;
}
export const batchRenameModalAtom = atom<BatchRenameModalState>({ open: false, items: [] });

export interface LibraryCoverModalState {
  open: boolean;
  path: string;
  name: string;
  initialCandidate: {
    media_item_id: number;
    file_hash: string;
    name: string | null;
    pixel_width: number | null;
    pixel_height: number | null;
    mime_type?: string | null;
  } | null;
}

export const libraryCoverModalAtom = atom<LibraryCoverModalState>({
  open: false,
  path: '',
  name: '',
  initialCandidate: null,
});

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

// ── Multi-file import choice ──
export interface MultiFileImportModalState {
  open: boolean;
  paths: string[];
  lifecycle: import('../shared/types/generated/application/Lifecycle').Lifecycle;
  parentFolderId: number | null;
  tags: string[];
  sourceUrls: string[];
  preserveStructure: boolean;
  deleteAfterIngest: boolean;
}

export const multiFileImportModalAtom = atom<MultiFileImportModalState>({
  open: false,
  paths: [],
  lifecycle: 'active',
  parentFolderId: null,
  tags: [],
  sourceUrls: [],
  preserveStructure: false,
  deleteAfterIngest: false,
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
  target: EntityTarget | null;
  coverRootId: number | null;
  groups: GroupCandidate[];
  notes: string;
  notesMaximumBytes: number;
  onBeforeSubmit?: () => void;
  onComplete?: (groupId: number) => void;
}

export const groupOrganizerModalAtom = atom<GroupOrganizerModalState>({
  open: false,
  target: null,
  coverRootId: null,
  groups: [],
  notes: '',
  notesMaximumBytes: 65_536,
});

export const updateModalAtom = atom({ open: false });
