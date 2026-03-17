import { create } from 'zustand';

interface FolderWatchActionState {
  requestToken: number;
  handledToken: number;
  folderId: number | null;
  requestOpen: (folderId: number) => void;
  markHandled: (token: number) => void;
  close: () => void;
}

export const useFolderWatchActionStore = create<FolderWatchActionState>((set) => ({
  requestToken: 0,
  handledToken: 0,
  folderId: null,
  requestOpen: (folderId) => set((state) => ({
    requestToken: state.requestToken + 1,
    folderId,
  })),
  markHandled: (token) => set({ handledToken: token }),
  close: () => set({ folderId: null }),
}));
