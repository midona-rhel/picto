import { create } from 'zustand';

import type { ManualImportProgressEvent } from '../shared/types/api/events';

type ManualImportStatus = 'idle' | 'running' | 'completed';

interface ManualImportState {
  visible: boolean;
  status: ManualImportStatus;
  label: string;
  done: number;
  total: number;
  imported: number;
  skipped: number;
  errors: number;
  start: (total: number, label?: string) => void;
  update: (progress: ManualImportProgressEvent) => void;
  setProgress: (progress: { done: number; total: number; imported: number; skipped: number; errors: number; label?: string }) => void;
  finish: (result: { imported: number; skipped: number; errors: number }) => void;
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

export const useManualImportStore = create<ManualImportState>((set, get) => ({
  visible: false,
  status: 'idle',
  label: 'Adding files',
  done: 0,
  total: 0,
  imported: 0,
  skipped: 0,
  errors: 0,

  start: (total, label = 'Adding files') => {
    clearHideTimer();
    set({
      visible: total > 0,
      status: 'running',
      label,
      done: 0,
      total,
      imported: 0,
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
      imported: progress.imported,
      skipped: progress.skipped,
      errors: progress.errors,
    });
  },

  setProgress: (progress) => {
    if (get().status === 'idle') {
      get().start(progress.total, progress.label);
    }
    set((state) => ({
      visible: true,
      status: 'running',
      label: progress.label ?? state.label,
      done: progress.done,
      total: progress.total,
      imported: progress.imported,
      skipped: progress.skipped,
      errors: progress.errors,
    }));
  },

  finish: ({ imported, skipped, errors }) => {
    clearHideTimer();
    const total = get().total;
    set({
      visible: true,
      status: 'completed',
      done: total,
      total,
      imported,
      skipped,
      errors,
    });
    hideTimer = window.setTimeout(() => {
      get().clear();
    }, 1400);
  },

  fail: () => {
    clearHideTimer();
    set({
      visible: false,
      status: 'idle',
      done: 0,
      total: 0,
      imported: 0,
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
      imported: 0,
      skipped: 0,
      errors: 0,
    });
  },
}));
