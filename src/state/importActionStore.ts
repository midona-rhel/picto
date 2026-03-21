import { create } from 'zustand';

export type ImportRequestKind = 'files' | 'folder';

interface ImportActionState {
  requestToken: number;
  handledToken: number;
  requestKind: ImportRequestKind;
  targetFolderId: number | null;
  requestImportFilesDialog: () => void;
  requestImportFolderDialog: (targetFolderId?: number | null) => void;
  markHandled: (token: number) => void;
}

export const useImportActionStore = create<ImportActionState>((set) => ({
  requestToken: 0,
  handledToken: 0,
  requestKind: 'files',
  targetFolderId: null,
  requestImportFilesDialog: () => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'files',
    targetFolderId: null,
  })),
  requestImportFolderDialog: (targetFolderId = null) => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'folder',
    targetFolderId,
  })),
  markHandled: (token) => set({ handledToken: token }),
}));
