import { create } from 'zustand';

export type ImportRequestKind = 'files' | 'folder';

interface ImportActionState {
  requestToken: number;
  handledToken: number;
  requestKind: ImportRequestKind;
  requestImportFilesDialog: () => void;
  requestImportFolderDialog: () => void;
  markHandled: (token: number) => void;
}

export const useImportActionStore = create<ImportActionState>((set) => ({
  requestToken: 0,
  handledToken: 0,
  requestKind: 'files',
  requestImportFilesDialog: () => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'files',
  })),
  requestImportFolderDialog: () => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'folder',
  })),
  markHandled: (token) => set({ handledToken: token }),
}));
