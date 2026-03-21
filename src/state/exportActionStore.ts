import { create } from 'zustand';

export type ExportRequestKind = 'basic' | 'advanced';

interface ExportActionState {
  requestToken: number;
  handledToken: number;
  requestKind: ExportRequestKind;
  requestBasicExport: () => void;
  requestAdvancedExport: () => void;
  markHandled: (token: number) => void;
}

export const useExportActionStore = create<ExportActionState>((set) => ({
  requestToken: 0,
  handledToken: 0,
  requestKind: 'basic',
  requestBasicExport: () => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'basic',
  })),
  requestAdvancedExport: () => set((state) => ({
    requestToken: state.requestToken + 1,
    requestKind: 'advanced',
  })),
  markHandled: (token) => set({ handledToken: token }),
}));
