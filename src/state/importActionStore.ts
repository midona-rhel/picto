import { create } from 'zustand';

interface ImportActionState {
  requestToken: number;
  requestImportDialog: () => void;
}

export const useImportActionStore = create<ImportActionState>((set) => ({
  requestToken: 0,
  requestImportDialog: () => set((state) => ({ requestToken: state.requestToken + 1 })),
}));
