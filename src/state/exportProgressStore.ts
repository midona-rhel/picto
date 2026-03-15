import { create } from 'zustand';

import type { MediaExportProgressEvent } from '../shared/types/api/events';

type ExportStatus = 'idle' | 'running' | 'completed';

interface ExportProgressState {
  visible: boolean;
  status: ExportStatus;
  label: string;
  done: number;
  total: number;
  exported: number;
  skipped: number;
  errors: number;
  start: (total: number, label?: string) => void;
  update: (progress: MediaExportProgressEvent) => void;
  finish: (result: { total?: number; exported: number; skipped: number; errors: number }) => void;
  fail: () => void;
  clear: () => void;
}

let hideTimer: number | null = null;

function clearHideTimer(): void {
  if (hideTimer != null) {
    window.clearTimeout(hideTimer);
    hideTimer = null;
  }
}

export const useExportProgressStore = create<ExportProgressState>((set, get) => ({
  visible: false,
  status: 'idle',
  label: 'Exporting files',
  done: 0,
  total: 0,
  exported: 0,
  skipped: 0,
  errors: 0,

  start: (total, label = 'Exporting files') => {
    clearHideTimer();
    set({
      visible: true,
      status: 'running',
      label,
      done: 0,
      total,
      exported: 0,
      skipped: 0,
      errors: 0,
    });
  },

  update: (progress) => {
    if (get().status === 'idle') {
      get().start(progress.total);
    }
    set({
      visible: true,
      status: 'running',
      done: progress.done,
      total: progress.total,
      exported: progress.exported,
      skipped: progress.skipped,
      errors: progress.errors,
    });
  },

  finish: ({ total, exported, skipped, errors }) => {
    clearHideTimer();
    const resolvedTotal = total ?? get().total;
    set({
      visible: true,
      status: 'completed',
      done: resolvedTotal,
      total: resolvedTotal,
      exported,
      skipped,
      errors,
    });
    hideTimer = window.setTimeout(() => {
      get().clear();
    }, 1600);
  },

  fail: () => {
    clearHideTimer();
    set({
      visible: false,
      status: 'idle',
      done: 0,
      total: 0,
      exported: 0,
      skipped: 0,
      errors: 0,
    });
  },

  clear: () => {
    clearHideTimer();
    set({
      visible: false,
      status: 'idle',
      done: 0,
      total: 0,
      exported: 0,
      skipped: 0,
      errors: 0,
    });
  },
}));
