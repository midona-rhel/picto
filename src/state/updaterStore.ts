import { create } from 'zustand';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'up-to-date'
  | 'downloading'
  | 'ready'
  | 'error';

interface UpdaterState {
  status: UpdateStatus;
  version: string | null;
  percent: number;
  bytesPerSecond: number;
  transferred: number;
  total: number;
  error: string | null;
  dismissed: boolean;

  handleStatusEvent: (event: {
    status: string;
    version?: string;
    percent?: number;
    bytesPerSecond?: number;
    transferred?: number;
    total?: number;
    error?: string;
  }) => void;
  dismiss: () => void;
  checkForUpdates: () => Promise<void>;
  downloadUpdate: () => Promise<void>;
  installUpdate: () => void;
}

export const useUpdaterStore = create<UpdaterState>((set) => ({
  status: 'idle',
  version: null,
  percent: 0,
  bytesPerSecond: 0,
  transferred: 0,
  total: 0,
  error: null,
  dismissed: false,

  handleStatusEvent: (event) => {
    switch (event.status) {
      case 'checking':
        set({ status: 'checking' });
        break;
      case 'available':
        set({ status: 'available', version: event.version ?? null, dismissed: false });
        break;
      case 'up-to-date':
        set({ status: 'up-to-date', version: event.version ?? null });
        break;
      case 'downloading':
        set({
          status: 'downloading',
          percent: event.percent ?? 0,
          bytesPerSecond: event.bytesPerSecond ?? 0,
          transferred: event.transferred ?? 0,
          total: event.total ?? 0,
          dismissed: false,
        });
        break;
      case 'ready':
        set({ status: 'ready', version: event.version ?? null, percent: 100, dismissed: false });
        break;
      case 'error':
        set({ status: 'error', error: event.error ?? 'Unknown error' });
        break;
    }
  },

  dismiss: () => set({ dismissed: true }),

  checkForUpdates: async () => {
    try {
      await window.picto?.updater?.check();
    } catch (e) {
      console.warn('Update check failed:', e);
    }
  },

  downloadUpdate: async () => {
    try {
      await window.picto?.updater?.download();
    } catch (e) {
      console.warn('Update download failed:', e);
    }
  },

  installUpdate: () => {
    window.picto?.updater?.install();
  },
}));
